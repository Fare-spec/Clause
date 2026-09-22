use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

// Each test owns an isolated directory, cleaned up even when an assertion panics.
struct StorageFixture(PathBuf);

impl StorageFixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "clause-storage-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        register(&path, 123).unwrap();
        Self(path)
    }

    fn directory(&self) -> PathBuf {
        self.0.join("123")
    }

    // Sparse files exercise the real quota without allocating 10 MB of test data.
    fn fill_to(&self, total: u64) {
        let file = fs::File::create(self.directory().join("uploads/filler.bin")).unwrap();
        file.set_len(total - usage(&self.directory()).unwrap())
            .unwrap();
    }
}

impl Drop for StorageFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn registration_preserves_existing_files_and_metadata() {
    let fixture = StorageFixture::new();
    let original = read(&fixture.0, 123, LIMIT_FILE).unwrap();
    upload(&fixture.0, 123, "notes.txt", b"keep me").unwrap();
    assert_eq!(register(&fixture.0, 123).unwrap(), fixture.directory());
    assert_eq!(read(&fixture.0, 123, "notes.txt").unwrap(), b"keep me");
    assert_eq!(read(&fixture.0, 123, LIMIT_FILE).unwrap(), original);
}

#[test]
fn upload_list_read_and_remove_preserve_exact_contents_and_usage() {
    let fixture = StorageFixture::new();
    let metadata_size = read(&fixture.0, 123, LIMIT_FILE).unwrap().len() as u64;
    let data = b"\x00hello\xff\n";
    upload(&fixture.0, 123, "z.bin", data).unwrap();
    upload(&fixture.0, 123, "a.txt", b"").unwrap();
    assert_eq!(read(&fixture.0, 123, "z.bin").unwrap(), data);
    let (entries, used) = list(&fixture.0, 123).unwrap();
    assert_eq!(
        entries,
        vec![
            ("a.txt".into(), 0),
            (LIMIT_FILE.into(), metadata_size),
            ("z.bin".into(), data.len() as u64)
        ]
    );
    assert_eq!(used, metadata_size + data.len() as u64);
    remove(&fixture.0, 123, "z.bin").unwrap();
    assert!(read(&fixture.0, 123, "z.bin").is_err());
    assert_eq!(list(&fixture.0, 123).unwrap().1, metadata_size);
}

#[test]
fn guilds_cannot_read_or_delete_each_others_files() {
    let fixture = StorageFixture::new();
    register(&fixture.0, 456).unwrap();
    upload(&fixture.0, 123, "private.txt", b"guild 123").unwrap();
    assert!(read(&fixture.0, 456, "private.txt").is_err());
    assert!(remove(&fixture.0, 456, "private.txt").is_err());
    upload(&fixture.0, 456, "private.txt", b"guild 456").unwrap();
    remove(&fixture.0, 456, "private.txt").unwrap();
    assert_eq!(read(&fixture.0, 123, "private.txt").unwrap(), b"guild 123");
}

#[test]
fn metadata_is_readable_but_cannot_be_uploaded_or_removed() {
    let fixture = StorageFixture::new();
    let original = read(&fixture.0, 123, LIMIT_FILE).unwrap();
    for name in [LIMIT_FILE, "STORAGE-LIMIT.JSON", "Storage-Limit.Json"] {
        assert!(upload(&fixture.0, 123, name, b"changed").is_err(), "{name}");
        assert!(remove(&fixture.0, 123, name).is_err(), "{name}");
    }
    assert_eq!(read(&fixture.0, 123, LIMIT_FILE).unwrap(), original);
    assert_eq!(list(&fixture.0, 123).unwrap().0.len(), 1);
}

#[test]
fn invalid_names_and_paths_are_rejected_without_modifying_storage() {
    let fixture = StorageFixture::new();
    let before = list(&fixture.0, 123).unwrap();
    for name in [
        "",
        ".",
        "..",
        "../456/file.txt",
        "/tmp/file.txt",
        "sub/file.txt",
        "sub\\file.txt",
        ".hidden",
        "file.",
        "file ",
        "bad\nname",
        "bad\0name",
    ] {
        assert!(upload(&fixture.0, 123, name, b"data").is_err(), "{name:?}");
        assert!(read(&fixture.0, 123, name).is_err(), "{name:?}");
        assert!(remove(&fixture.0, 123, name).is_err(), "{name:?}");
    }
    assert!(upload(&fixture.0, 123, &"a".repeat(101), b"data").is_err());
    assert_eq!(list(&fixture.0, 123).unwrap(), before);
    let maximum_name = "a".repeat(100);
    upload(&fixture.0, 123, &maximum_name, b"valid").unwrap();
    assert_eq!(read(&fixture.0, 123, &maximum_name).unwrap(), b"valid");
}

#[test]
fn duplicate_upload_does_not_overwrite_existing_file() {
    let fixture = StorageFixture::new();
    upload(&fixture.0, 123, "notes.txt", b"original").unwrap();
    let before = list(&fixture.0, 123).unwrap();
    assert!(upload(&fixture.0, 123, "notes.txt", b"replacement").is_err());
    assert_eq!(read(&fixture.0, 123, "notes.txt").unwrap(), b"original");
    assert_eq!(list(&fixture.0, 123).unwrap(), before);
}

#[test]
fn quota_accepts_exact_limit_and_rejects_one_extra_byte_without_partial_file() {
    let fixture = StorageFixture::new();
    fixture.fill_to(limit_bytes() - 1);
    upload(&fixture.0, 123, "last-byte.txt", b"x").unwrap();
    assert_eq!(list(&fixture.0, 123).unwrap().1, limit_bytes());
    register(&fixture.0, 123).unwrap();
    assert!(upload(&fixture.0, 123, "overflow.txt", b"x").is_err());
    assert!(!fixture.directory().join("uploads/overflow.txt").exists());
    assert_eq!(list(&fixture.0, 123).unwrap().1, limit_bytes());
}

#[test]
fn removal_can_recover_an_over_quota_folder() {
    let fixture = StorageFixture::new();
    fixture.fill_to(limit_bytes() + 1);
    register(&fixture.0, 123).unwrap();
    assert!(upload(&fixture.0, 123, "more.txt", b"x").is_err());
    remove(&fixture.0, 123, "filler.bin").unwrap();
    upload(&fixture.0, 123, "more.txt", b"x").unwrap();
    register(&fixture.0, 123).unwrap();
}

#[test]
fn nested_files_count_towards_quota_but_directories_cannot_be_removed_as_files() {
    let fixture = StorageFixture::new();
    let initial = list(&fixture.0, 123).unwrap().1;
    let nested = fixture.directory().join("uploads/nested");
    fs::create_dir(&nested).unwrap();
    fs::File::create(nested.join("large.bin"))
        .unwrap()
        .set_len(limit_bytes() - initial)
        .unwrap();
    assert_eq!(list(&fixture.0, 123).unwrap().1, limit_bytes());
    assert!(upload(&fixture.0, 123, "extra.txt", b"x").is_err());
    assert!(read(&fixture.0, 123, "nested").is_err());
    assert!(remove(&fixture.0, 123, "nested").is_err());
    assert!(nested.join("large.bin").exists());
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_expose_files_or_redirect_guild_storage() {
    use std::os::unix::fs::symlink;
    let fixture = StorageFixture::new();
    let original = read(&fixture.0, 123, LIMIT_FILE).unwrap();
    symlink(
        fixture.directory().join(LIMIT_FILE),
        fixture.directory().join("uploads/alias.json"),
    )
    .unwrap();
    assert!(read(&fixture.0, 123, "alias.json").is_err());
    assert!(remove(&fixture.0, 123, "alias.json").is_err());
    assert!(upload(&fixture.0, 123, "alias.json", b"changed").is_err());
    assert!(list(&fixture.0, 123).is_err());
    symlink(fixture.directory(), fixture.0.join("456")).unwrap();
    assert!(register(&fixture.0, 456).is_err());
    assert!(read(&fixture.0, 456, LIMIT_FILE).is_err());
    assert!(upload(&fixture.0, 456, "new.txt", b"changed").is_err());
    assert!(remove(&fixture.0, 456, "alias.json").is_err());
    assert_eq!(read(&fixture.0, 123, LIMIT_FILE).unwrap(), original);
}

#[test]
fn legacy_root_files_move_to_uploads_and_migration_is_idempotent() {
    let fixture = StorageFixture::new();
    let metadata = read(&fixture.0, 123, LIMIT_FILE).unwrap();
    fs::write(fixture.directory().join("legacy.txt"), "existing data").unwrap();
    let before = usage(&fixture.directory()).unwrap();
    layout(&fixture.0, 123).unwrap();
    layout(&fixture.0, 123).unwrap();
    assert!(!fixture.directory().join("legacy.txt").exists());
    assert_eq!(
        fs::read(fixture.directory().join("uploads/legacy.txt")).unwrap(),
        b"existing data"
    );
    assert_eq!(
        read(&fixture.0, 123, "legacy.txt").unwrap(),
        b"existing data"
    );
    assert_eq!(usage(&fixture.directory()).unwrap(), before);
    assert_eq!(read(&fixture.0, 123, LIMIT_FILE).unwrap(), metadata);
}

#[test]
fn migration_never_overwrites_an_existing_upload() {
    let fixture = StorageFixture::new();
    fs::write(fixture.directory().join("duplicate.txt"), "legacy").unwrap();
    fs::write(fixture.directory().join("uploads/duplicate.txt"), "current").unwrap();
    assert!(layout(&fixture.0, 123).is_err());
    assert_eq!(
        fs::read(fixture.directory().join("duplicate.txt")).unwrap(),
        b"legacy"
    );
    assert_eq!(
        fs::read(fixture.directory().join("uploads/duplicate.txt")).unwrap(),
        b"current"
    );
}

#[cfg(unix)]
#[test]
fn log_and_upload_subdirectories_cannot_be_symlinks() {
    for name in ["logs", "uploads"] {
        let fixture = StorageFixture::new();
        fs::remove_dir(fixture.directory().join(name)).unwrap();
        let outside = fixture.0.join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.directory().join(name)).unwrap();
        assert!(layout(&fixture.0, 123).is_err());
        assert!(upload(&fixture.0, 123, "test.txt", b"data").is_err());
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }
}

#[test]
fn storage_breakdown_counts_uploads_logs_and_other_files_per_guild() {
    let fixture = StorageFixture::new();
    register(&fixture.0, 456).unwrap();
    upload(&fixture.0, 123, "file.txt", b"12345").unwrap();
    fs::write(fixture.directory().join("logs/example.json"), b"1234567").unwrap();
    let metadata = read(&fixture.0, 123, LIMIT_FILE).unwrap().len() as u64;
    let report = stats(&fixture.0, 123).unwrap();
    assert_eq!(report.uploads, 5);
    assert_eq!(report.logs, 7);
    assert_eq!(report.other, metadata);
    assert_eq!(report.total, 12 + metadata);
    assert_eq!(report.available(), limit_bytes() - 12 - metadata);
    assert_eq!(stats(&fixture.0, 456).unwrap().uploads, 0);
    assert_eq!(stats(&fixture.0, 456).unwrap().logs, 0);
}

#[test]
fn available_storage_saturates_at_zero_when_at_or_over_quota() {
    let fixture = StorageFixture::new();
    fixture.fill_to(limit_bytes());
    assert_eq!(stats(&fixture.0, 123).unwrap().available(), 0);
    fs::write(fixture.directory().join("logs/extra.json"), b"x").unwrap();
    let report = stats(&fixture.0, 123).unwrap();
    assert_eq!(report.available(), 0);
    assert_eq!(report.total, limit_bytes() + 1);
}
