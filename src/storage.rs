//! Guild storage. Callers serialize operations with Handler::storage_lock.
//! The application quota includes metadata and is not an OS disk quota.
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(crate) const STORAGE_LIMIT_BYTES: u64 = 50_000_000;

pub(crate) fn register(root: &Path, guild_id: u64) -> io::Result<PathBuf> {
    let directory = root.join(guild_id.to_string());
    fs::create_dir_all(root)?;
    match fs::create_dir(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(&directory)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(io::Error::other(
                    "Guild storage must be a directory, not a symlink",
                ));
            }
        }
        Err(error) => return Err(error),
    }
    let metadata_path = directory.join("storage-limit.json");
    let metadata =
        format!("{{\"guild_id\":\"{guild_id}\",\"limit_bytes\":{STORAGE_LIMIT_BYTES}}}\n");
    let used = usage(&directory)?;
    match fs::symlink_metadata(&metadata_path) {
        Ok(_) => {
            if fs::read_to_string(&metadata_path)? != metadata {
                return Err(io::Error::other(
                    "Guild storage metadata does not match its limit",
                ));
            }
            if used > STORAGE_LIMIT_BYTES {
                return Err(io::Error::other("Guild storage exceeds 50 MB"));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if used.saturating_add(metadata.len() as u64) > STORAGE_LIMIT_BYTES {
                return Err(io::Error::other("Guild storage exceeds 50 MB"));
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(metadata_path)?;
            file.write_all(metadata.as_bytes())?;
        }
        Err(error) => return Err(error),
    }
    layout(root, guild_id)?;
    Ok(directory)
}

pub(crate) fn usage(directory: &Path) -> io::Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let metadata = fs::symlink_metadata(&path)?;
        let size = if metadata.is_dir() {
            usage(&path)?
        } else if metadata.is_file() {
            metadata.len()
        } else {
            return Err(io::Error::other(
                "Guild storage cannot contain symlinks or special files",
            ));
        };
        total = total
            .checked_add(size)
            .ok_or_else(|| io::Error::other("Storage size overflow"))?;
    }
    Ok(total)
}

pub(crate) const LIMIT_FILE: &str = "storage-limit.json";

pub(crate) fn validate_name(name: &str, modifying: bool) -> io::Result<()> {
    if name.is_empty()
        || name.len() > 100
        || name == "."
        || name == ".."
        || name.starts_with('.')
        || name.ends_with('.')
        || name.ends_with(' ')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._- ".contains(&b))
    {
        return Err(io::Error::other(
            "Use a filename of 1–100 ASCII letters, numbers, spaces, dots, underscores or hyphens, without a leading dot or trailing dot/space. Paths are not allowed.",
        ));
    }
    if modifying && name.eq_ignore_ascii_case(LIMIT_FILE) {
        return Err(io::Error::other(
            "storage-limit.json is read-only and cannot be uploaded or removed.",
        ));
    }
    Ok(())
}

fn guild_directory(root: &Path, guild: u64) -> io::Result<PathBuf> {
    let directory = root.join(guild.to_string());
    let metadata = fs::symlink_metadata(&directory)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other(
            "Guild storage is not a regular directory.",
        ));
    }
    Ok(directory)
}

pub(crate) fn subdirectory(directory: &Path, name: &str) -> io::Result<PathBuf> {
    let path = directory.join(name);
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(&path)?.is_dir() {
                return Err(io::Error::other(
                    "Storage subfolder must be a directory, not a file or symlink",
                ));
            }
        }
        Err(error) => return Err(error),
    }
    Ok(path)
}

/// Migrate legacy root uploads without overwriting files, including on restart.
pub(crate) fn layout(root: &Path, guild: u64) -> io::Result<PathBuf> {
    let directory = guild_directory(root, guild)?;
    let uploads = subdirectory(&directory, "uploads")?;
    subdirectory(&directory, "logs")?;
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        if entry.file_name() == LIMIT_FILE {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::other("Guild storage cannot contain symlinks"));
        }
        if metadata.is_file() {
            let target = uploads.join(entry.file_name());
            // A hard link avoids overwriting an existing destination and uses no extra quota.
            fs::hard_link(entry.path(), &target).map_err(|error| if error.kind() == io::ErrorKind::AlreadyExists {
                io::Error::other("Legacy upload conflicts with a file in uploads; resolve the duplicate before continuing")
            } else { error })?;
            fs::remove_file(entry.path())?;
        }
    }
    Ok(directory)
}

pub(crate) fn list(root: &Path, guild: u64) -> io::Result<(Vec<(String, u64)>, u64)> {
    let directory = layout(root, guild)?;
    let used = usage(&directory)?;
    let mut entries = vec![(
        LIMIT_FILE.to_owned(),
        fs::symlink_metadata(directory.join(LIMIT_FILE))?.len(),
    )];
    for entry in fs::read_dir(directory.join("uploads"))? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_file() {
            entries.push((
                entry.file_name().to_string_lossy().into_owned(),
                metadata.len(),
            ));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok((entries, used))
}

pub(crate) fn upload(root: &Path, guild: u64, name: &str, data: &[u8]) -> io::Result<()> {
    validate_name(name, true)?;
    let directory = layout(root, guild)?;
    if usage(&directory)?.saturating_add(data.len() as u64) > STORAGE_LIMIT_BYTES {
        return Err(io::Error::other(
            "This upload would exceed the guild's 50 Mo (50,000,000 byte) limit. Remove files first.",
        ));
    }
    let path = directory.join("uploads").join(name);
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&path)
        .map_err(|error| if error.kind() == io::ErrorKind::AlreadyExists {
            io::Error::other("A file with that name already exists. Remove it first or use another filename.")
        } else { error })?;
    if let Err(error) = file.write_all(data).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

fn regular_file(root: &Path, guild: u64, name: &str, modifying: bool) -> io::Result<PathBuf> {
    validate_name(name, modifying)?;
    let directory = if name == LIMIT_FILE && !modifying {
        guild_directory(root, guild)?
    } else {
        layout(root, guild)?.join("uploads")
    };
    let path = directory.join(name);
    if !fs::symlink_metadata(&path)?.is_file() {
        return Err(io::Error::other(
            "Only regular files can be accessed; folders and symlinks are not allowed.",
        ));
    }
    Ok(path)
}

pub(crate) fn remove(root: &Path, guild: u64, name: &str) -> io::Result<()> {
    fs::remove_file(regular_file(root, guild, name, true)?)
}

pub(crate) fn read(root: &Path, guild: u64, name: &str) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let path = regular_file(root, guild, name, false)?;
    let mut data = Vec::new();
    fs::File::open(path)?
        .take(STORAGE_LIMIT_BYTES + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > STORAGE_LIMIT_BYTES {
        return Err(io::Error::other("File exceeds the 50 Mo limit."));
    }
    Ok(data)
}

pub(crate) fn read_uploads(root: &Path, guild: u64) -> io::Result<Vec<(String, Vec<u8>)>> {
    use std::io::Read;
    let directory = layout(root, guild)?.join("uploads");
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_file() {
            continue;
        }
        let mut data = Vec::new();
        fs::File::open(entry.path())?
            .take(STORAGE_LIMIT_BYTES + 1)
            .read_to_end(&mut data)?;
        if data.len() as u64 > STORAGE_LIMIT_BYTES {
            return Err(io::Error::other("File exceeds the 50 Mo limit."));
        }
        files.push((entry.file_name().to_string_lossy().into_owned(), data));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Usage {
    pub uploads: u64,
    pub logs: u64,
    pub other: u64,
    pub total: u64,
}
impl Usage {
    pub(crate) fn available(&self) -> u64 {
        STORAGE_LIMIT_BYTES.saturating_sub(self.total)
    }
}

/// Caller holds the shared storage lock for a consistent per-guild snapshot.
pub(crate) fn stats(root: &Path, guild: u64) -> io::Result<Usage> {
    let directory = layout(root, guild)?;
    let total = usage(&directory)?;
    let uploads = usage(&subdirectory(&directory, "uploads")?)?;
    let logs = usage(&subdirectory(&directory, "logs")?)?;
    Ok(Usage {
        uploads,
        logs,
        other: total.saturating_sub(uploads).saturating_sub(logs),
        total,
    })
}

#[cfg(test)]
mod tests;
