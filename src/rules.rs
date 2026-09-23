use crate::storage;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::Path,
};

const RULES_DIR: &str = "rules";
const RULES_FILE: &str = "rules.json";
const TMP_FILE: &str = "rules.json.tmp";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuleBook {
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Rule {
    pub id: String,
    pub severity: String,
    pub text: String,
}

#[derive(Default)]
pub(crate) struct Cache {
    books: HashMap<u64, RuleBook>,
}

impl Cache {
    pub(crate) fn get(&mut self, root: &Path, guild: u64) -> io::Result<RuleBook> {
        if let Some(book) = self.books.get(&guild) {
            return Ok(book.clone());
        }
        let book = load(root, guild)?;
        self.books.insert(guild, book.clone());
        Ok(book)
    }

    pub(crate) fn save(&mut self, root: &Path, guild: u64, book: &RuleBook) -> io::Result<()> {
        save(root, guild, book)?;
        self.books.insert(guild, book.clone());
        Ok(())
    }

    pub(crate) fn forget(&mut self, guild: u64) {
        self.books.remove(&guild);
    }
}

pub(crate) fn validate_id(id: &str) -> io::Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err(io::Error::other(
            "Use a rule id of 1-64 ASCII letters, numbers, dots, underscores, or hyphens.",
        ));
    }
    Ok(())
}

pub(crate) fn validate_severity(severity: &str) -> io::Result<()> {
    if matches!(severity, "low" | "medium" | "high" | "critical") {
        Ok(())
    } else {
        Err(io::Error::other(
            "Severity must be low, medium, high, or critical.",
        ))
    }
}

pub(crate) fn upsert(
    book: &mut RuleBook,
    id: &str,
    severity: &str,
    text: Option<&str>,
) -> io::Result<bool> {
    validate_id(id)?;
    validate_severity(severity)?;
    let text = text.map(str::trim).filter(|text| !text.is_empty());
    if let Some(rule) = book.rules.iter_mut().find(|rule| rule.id == id) {
        rule.severity = severity.into();
        if let Some(text) = text {
            rule.text = text.into();
        }
        return Ok(false);
    }
    let text = text.ok_or_else(|| io::Error::other("New rules need rule text."))?;
    if text.len() > 3000 {
        return Err(io::Error::other(
            "Rule text must be 3000 characters or less.",
        ));
    }
    book.rules.push(Rule {
        id: id.into(),
        severity: severity.into(),
        text: text.into(),
    });
    book.rules.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(true)
}

pub(crate) fn remove(book: &mut RuleBook, id: &str) -> io::Result<bool> {
    validate_id(id)?;
    let before = book.rules.len();
    book.rules.retain(|rule| rule.id != id);
    Ok(book.rules.len() != before)
}

pub(crate) fn to_markdown(book: &RuleBook) -> String {
    let visibility = if book.public { "public" } else { "private" };
    let mut out = format!("# Clause rules\n\nVisibility: {visibility}\n\n");
    if book.rules.is_empty() {
        out.push_str("No rules are configured.\n");
        return out;
    }
    for rule in &book.rules {
        out.push_str(&format!(
            "## {} ({})\n\n{}\n\n",
            rule.id, rule.severity, rule.text
        ));
    }
    out
}

pub(crate) fn to_ai_text(book: &RuleBook) -> String {
    serde_json::to_string_pretty(book).unwrap_or_else(|_| "{\"rules\":[]}".into())
}

fn load(root: &Path, guild: u64) -> io::Result<RuleBook> {
    let directory = storage::layout(root, guild)?;
    let rules = storage::subdirectory(&directory, RULES_DIR)?;
    let path = rules.join(RULES_FILE);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(io::Error::other("Rules file is not a regular file."));
            }
            let bytes = fs::read(path)?;
            serde_json::from_slice(&bytes).map_err(|_| io::Error::other("Rules JSON is invalid."))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(RuleBook::default()),
        Err(error) => Err(error),
    }
}

fn save(root: &Path, guild: u64, book: &RuleBook) -> io::Result<()> {
    let directory = storage::layout(root, guild)?;
    let rules = storage::subdirectory(&directory, RULES_DIR)?;
    let path = rules.join(RULES_FILE);
    let data = serde_json::to_vec_pretty(book)?;
    let existing = match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(io::Error::other("Rules file is not a regular file."));
            }
            metadata.len()
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    let used = storage::usage(&directory)?;
    if used
        .saturating_sub(existing)
        .saturating_add(data.len() as u64)
        > storage::limit_bytes()
    {
        return Err(io::Error::other("Rules would exceed guild storage."));
    }
    let tmp = rules.join(TMP_FILE);
    let _ = fs::remove_file(&tmp);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)?;
    if let Err(error) = file.write_all(&data).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(tmp);
        return Err(error);
    }
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "clause-rules-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            storage::register(&root, 1).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rule_book_round_trips_and_updates_cache() {
        let f = Fixture::new();
        let mut cache = Cache::default();
        let mut book = cache.get(&f.0, 1).unwrap();
        assert!(book.rules.is_empty());
        assert!(upsert(&mut book, "spam", "medium", Some("No spam.")).unwrap());
        book.public = true;
        cache.save(&f.0, 1, &book).unwrap();
        let loaded = cache.get(&f.0, 1).unwrap();
        assert_eq!(loaded, book);
        assert!(!upsert(&mut book, "spam", "high", None).unwrap());
        assert_eq!(book.rules[0].severity, "high");
    }

    #[test]
    fn validates_ids_severity_and_markdown_export() {
        let mut book = RuleBook::default();
        assert!(upsert(&mut book, "../bad", "low", Some("bad")).is_err());
        assert!(upsert(&mut book, "one", "urgent", Some("bad")).is_err());
        assert!(upsert(&mut book, "one", "low", Some("Be kind.")).unwrap());
        assert!(to_markdown(&book).contains("## one (low)"));
        assert!(remove(&mut book, "one").unwrap());
        assert!(!remove(&mut book, "one").unwrap());
    }
}
