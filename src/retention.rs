//! Disk retention is independent of Discord log verbosity.
use crate::storage;
use std::{
    fs,
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Policy {
    None,
    Flagged(u64),
    All(u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Message,
    Managed,
    Action,
    Event,
}
impl Kind {
    fn key(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Managed => "managed",
            Self::Action => "action",
            Self::Event => "event",
        }
    }
}
impl Policy {
    pub(crate) const OPTIONS: [Self; 7] = [
        Self::None,
        Self::Flagged(7),
        Self::Flagged(30),
        Self::Flagged(90),
        Self::All(1),
        Self::All(7),
        Self::All(30),
    ];
    pub(crate) fn key(self) -> String {
        match self {
            Self::None => "none".into(),
            Self::Flagged(days) => format!("flagged:{days}"),
            Self::All(days) => format!("all:{days}"),
        }
    }
    pub(crate) fn label(self) -> String {
        match self {
            Self::None => "None — do not retain logs".into(),
            Self::Flagged(days) => format!("Flagged / managed messages — {days} days"),
            Self::All(days) => format!("All messages and bot actions — {days} days"),
        }
    }
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Self::OPTIONS.into_iter().find(|p| p.key() == value)
    }
    pub(crate) fn allows(self, kind: Kind) -> bool {
        match self {
            Self::None => false,
            Self::Flagged(_) => kind == Kind::Managed,
            Self::All(_) => kind != Kind::Event,
        }
    }
    fn days(self) -> u64 {
        match self {
            Self::None => 0,
            Self::Flagged(days) | Self::All(days) => days,
        }
    }
    fn keeps(self, kind: Kind, timestamp: u64, now: u64) -> bool {
        self.allows(kind) && timestamp.saturating_add(self.days() * 86400) > now
    }
}
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn record_name(name: &str) -> Option<(u64, Kind)> {
    let name = name.strip_prefix("clause-")?.strip_suffix(".json")?;
    let mut parts = name.split('-');
    let timestamp = parts.next()?.parse().ok()?;
    let kind = match parts.next()? {
        "message" => Kind::Message,
        "managed" => Kind::Managed,
        "action" => Kind::Action,
        _ => return None,
    };
    parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((timestamp, kind))
}

// Call under the shared storage lock. Never delete uploads or unknown files.
pub(crate) fn prune(root: &Path, guild: u64, policy: Policy, now: u64) -> io::Result<()> {
    let directory = storage::layout(root, guild)?;
    let logs = storage::subdirectory(&directory, "logs")?;
    for entry in fs::read_dir(logs)? {
        let entry = entry?;
        let Some((timestamp, kind)) = record_name(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        if !fs::symlink_metadata(entry.path())?.is_file() {
            return Err(io::Error::other("Invalid retained log file"));
        }
        if !policy.keeps(kind, timestamp, now) {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ClearReport {
    pub files: u64,
    pub bytes: u64,
}

// Call under the shared storage lock. Removes only Clause-owned retained logs.
pub(crate) fn clear(root: &Path, guild: u64) -> io::Result<ClearReport> {
    let directory = storage::layout(root, guild)?;
    let logs = storage::subdirectory(&directory, "logs")?;
    let mut report = ClearReport { files: 0, bytes: 0 };
    for entry in fs::read_dir(logs)? {
        let entry = entry?;
        if record_name(&entry.file_name().to_string_lossy()).is_none() {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_file() {
            return Err(io::Error::other("Invalid retained log file"));
        }
        report.files += 1;
        report.bytes = report
            .bytes
            .checked_add(metadata.len())
            .ok_or_else(|| io::Error::other("Storage size overflow"))?;
        fs::remove_file(entry.path())?;
    }
    Ok(report)
}

// Caller holds the same lock used for uploads, making shared quota checks atomic.
pub(crate) fn retain(
    root: &Path,
    guild: u64,
    policy: Policy,
    kind: Kind,
    timestamp: u64,
    text: &str,
    current: u64,
) -> io::Result<bool> {
    prune(root, guild, policy, current)?;
    if !policy.keeps(kind, timestamp, current) {
        return Ok(false);
    }
    let directory = storage::layout(root, guild)?;
    let data = serde_json::to_vec(
        &serde_json::json!({"guild_id":guild.to_string(), "recorded_at":timestamp, "kind":kind.key(), "text":text}),
    )?;
    if storage::usage(&directory)?.saturating_add(data.len() as u64) > storage::STORAGE_LIMIT_BYTES
    {
        return Err(io::Error::other(
            "Guild storage full; retained record skipped",
        ));
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = directory
            .join("logs")
            .join(format!("clause-{timestamp}-{}-{sequence}.json", kind.key()));
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        if let Err(error) = file.write_all(&data).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(path);
            return Err(error);
        }
        return Ok(true);
    }
}

#[cfg(test)]
mod tests;
