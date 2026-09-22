//! Guild storage registration. Future upload handlers must check total usage
//! against STORAGE_LIMIT_BYTES before writing; this is not an OS disk quota.
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
    Ok(directory)
}

fn usage(directory: &Path) -> io::Result<u64> {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_is_idempotent_preserves_files_and_checks_limit() {
        let root = std::env::temp_dir().join(format!(
            "clause-storage-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let directory = register(&root, 123).unwrap();
        assert_eq!(directory, root.join("123"));
        fs::write(directory.join("existing.txt"), "keep me").unwrap();
        register(&root, 123).unwrap();
        assert_eq!(
            fs::read_to_string(directory.join("existing.txt")).unwrap(),
            "keep me"
        );
        let other = register(&root, 456).unwrap();
        assert!(!other.join("existing.txt").exists());
        let file = fs::File::create(directory.join("large.bin")).unwrap();
        file.set_len(STORAGE_LIMIT_BYTES - usage(&directory).unwrap())
            .unwrap();
        register(&root, 123).unwrap();
        file.set_len(file.metadata().unwrap().len() + 1).unwrap();
        assert!(register(&root, 123).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
