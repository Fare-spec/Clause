use super::*;
use std::path::PathBuf;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "clause-retention-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        storage::register(&root, 1).unwrap();
        Self(root)
    }
    fn count(&self) -> usize {
        fs::read_dir(self.0.join("1/logs")).unwrap().count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn policies_only_retain_selected_record_types() {
    let f = Fixture::new();
    assert!(!retain(&f.0, 1, Policy::None, Kind::Message, 100, "hello", 100).unwrap());
    assert!(
        !retain(
            &f.0,
            1,
            Policy::Flagged(7),
            Kind::Message,
            100,
            "hello",
            100
        )
        .unwrap()
    );
    assert!(!retain(&f.0, 1, Policy::Flagged(7), Kind::Action, 100, "hello", 100).unwrap());
    assert!(
        retain(
            &f.0,
            1,
            Policy::Flagged(7),
            Kind::Managed,
            100,
            "flagged",
            100
        )
        .unwrap()
    );
    assert_eq!(f.count(), 1);
    assert!(retain(&f.0, 1, Policy::All(1), Kind::Message, 100, "hello", 100).unwrap());
    assert!(retain(&f.0, 1, Policy::All(1), Kind::Action, 100, "action", 100).unwrap());
    assert!(!retain(&f.0, 1, Policy::All(1), Kind::Event, 100, "event", 100).unwrap());
    assert_eq!(f.count(), 3);
}

#[test]
fn expiry_is_exact_and_none_clears_only_retained_logs() {
    let f = Fixture::new();
    storage::upload(&f.0, 1, "notes.txt", b"keep").unwrap();
    retain(&f.0, 1, Policy::All(1), Kind::Message, 100, "hello", 100).unwrap();
    prune(&f.0, 1, Policy::All(1), 100 + 86400 - 1).unwrap();
    assert_eq!(f.count(), 1);
    prune(&f.0, 1, Policy::All(1), 100 + 86400).unwrap();
    assert_eq!(f.count(), 0);
    retain(&f.0, 1, Policy::All(30), Kind::Message, 200, "hello", 200).unwrap();
    fs::write(f.0.join("1/logs/unrelated.txt"), "keep").unwrap();
    prune(&f.0, 1, Policy::None, 200).unwrap();
    assert_eq!(f.count(), 1);
    assert_eq!(storage::read(&f.0, 1, "notes.txt").unwrap(), b"keep");
    assert!(f.0.join("1/storage-limit.json").exists());
}

#[test]
fn explicit_clear_removes_only_clause_retained_logs() {
    let f = Fixture::new();
    storage::upload(&f.0, 1, "notes.txt", b"keep").unwrap();
    retain(&f.0, 1, Policy::All(30), Kind::Message, 200, "hello", 200).unwrap();
    retain(&f.0, 1, Policy::All(30), Kind::Action, 201, "action", 201).unwrap();
    fs::write(f.0.join("1/logs/unrelated.txt"), "keep").unwrap();
    let metadata = storage::read(&f.0, 1, storage::LIMIT_FILE).unwrap();

    let report = clear(&f.0, 1).unwrap();

    assert_eq!(report.files, 2);
    assert!(report.bytes > 0);
    assert_eq!(f.count(), 1);
    assert_eq!(
        fs::read_to_string(f.0.join("1/logs/unrelated.txt")).unwrap(),
        "keep"
    );
    assert_eq!(storage::read(&f.0, 1, "notes.txt").unwrap(), b"keep");
    assert_eq!(
        storage::read(&f.0, 1, storage::LIMIT_FILE).unwrap(),
        metadata
    );
}

#[test]
fn narrower_policy_removes_unmanaged_records_and_expires_old_managed_records() {
    let f = Fixture::new();
    for kind in [Kind::Message, Kind::Managed, Kind::Action] {
        retain(&f.0, 1, Policy::All(30), kind, 100, "entry", 100).unwrap();
    }
    prune(&f.0, 1, Policy::Flagged(7), 101).unwrap();
    assert_eq!(f.count(), 1);
    prune(&f.0, 1, Policy::Flagged(7), 100 + 7 * 86400).unwrap();
    assert_eq!(f.count(), 0);
}

#[test]
fn logs_share_quota_with_uploads_and_are_not_accessible_as_upload_paths() {
    let f = Fixture::new();
    retain(&f.0, 1, Policy::All(1), Kind::Message, 100, "hello", 100).unwrap();
    let used = storage::list(&f.0, 1).unwrap().1;
    assert!(used > storage::read(&f.0, 1, storage::LIMIT_FILE).unwrap().len() as u64);
    let file = fs::File::create(f.0.join("1/uploads/filler.bin")).unwrap();
    file.set_len(storage::limit_bytes() - used).unwrap();
    assert!(retain(&f.0, 1, Policy::All(1), Kind::Message, 100, "full", 100).is_err());
    assert!(storage::upload(&f.0, 1, "extra.txt", b"x").is_err());
    assert_eq!(f.count(), 1);
    assert!(storage::remove(&f.0, 1, "../logs/anything.json").is_err());
    prune(&f.0, 1, Policy::None, 100).unwrap();
    storage::upload(&f.0, 1, "extra.txt", b"x").unwrap();
}

#[test]
fn retention_does_not_touch_another_guild() {
    let f = Fixture::new();
    storage::register(&f.0, 2).unwrap();
    retain(
        &f.0,
        2,
        Policy::All(1),
        Kind::Message,
        100,
        "guild two",
        100,
    )
    .unwrap();
    prune(&f.0, 1, Policy::None, 100).unwrap();
    assert_eq!(fs::read_dir(f.0.join("2/logs")).unwrap().count(), 1);
    assert_eq!(f.count(), 0);
}

#[test]
fn unsupported_policies_and_non_archive_names_are_rejected() {
    for policy in Policy::OPTIONS {
        assert_eq!(Policy::parse(&policy.key()), Some(policy));
    }
    assert_eq!(Policy::parse("all:0"), None);
    assert_eq!(Policy::parse("all:9999999999"), None);
    assert!(record_name("notes.json").is_none());
    assert!(record_name("clause-100-message-1-extra.json").is_none());
}
