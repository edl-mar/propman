use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;
use crate::parser::{self, FileEntry};

#[derive(Debug, Clone)]
pub struct PropertiesFile {
    pub path: PathBuf,
    pub locale: String,
    pub entries: Vec<FileEntry>,
}

impl PropertiesFile {
    /// Look up the value for `key`, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find_map(|e| match e {
            FileEntry::KeyValue { key: k, value, .. } if k == key => Some(value.as_str()),
            _ => None,
        })
    }

    /// Returns the 1-based line number after which a new entry for `key` should
    /// be inserted to preserve the file's existing grouping structure.
    ///
    /// Scans all existing `KeyValue` entries and picks the one that shares the
    /// longest common dot-boundary prefix with `key`; ties are broken by taking
    /// the entry with the highest `last_line` (i.e. the last sibling in file order).
    ///
    /// Returns 0 only for an empty file (no entries at all), in which case the
    /// caller should append after line 0 — i.e. the new entry becomes line 1.
    pub fn insertion_point_for(&self, key: &str) -> usize {
        let mut best_last_line: usize = 0;
        let mut best_prefix_len: usize = 0;

        for entry in &self.entries {
            if let FileEntry::KeyValue { key: k, last_line, .. } = entry {
                let prefix_len = common_dot_prefix_len(key, k);
                if prefix_len > best_prefix_len
                    || (prefix_len == best_prefix_len && *last_line > best_last_line)
                {
                    best_prefix_len = prefix_len;
                    best_last_line = *last_line;
                }
            }
        }

        if best_last_line == 0 {
            // No KeyValue entries: fall back to the last line in the file.
            self.entries.iter().map(|e| match e {
                FileEntry::KeyValue { last_line, .. } => *last_line,
                FileEntry::Comment { line, .. } | FileEntry::Blank { line } => *line,
            }).max().unwrap_or(0)
        } else {
            best_last_line
        }
    }

    /// Look up the line range for `key`, if present.
    /// Returns `(first_line, last_line)`; equal for single-line values.
    pub fn line_of(&self, key: &str) -> Option<(usize, usize)> {
        self.entries.iter().find_map(|e| match e {
            FileEntry::KeyValue { key: k, first_line, last_line, .. } if k == key => {
                Some((*first_line, *last_line))
            }
            _ => None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct FileGroup {
    pub base_name: String,
    /// Always sorted: "default" first, then locales alphabetically.
    pub files: Vec<PropertiesFile>,
}

impl FileGroup {
    pub fn locales(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|f| f.locale.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub groups: Vec<FileGroup>,
    /// All keys across all files, sorted alphabetically. Used as the row index.
    pub merged_keys: Vec<String>,
}

impl Workspace {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        // group_map: base_name → files
        let mut group_map: HashMap<String, Vec<PropertiesFile>> = HashMap::new();

        for entry in WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path().extension().map_or(false, |ext| ext == "properties")
            })
        {
            let path = entry.path().to_path_buf();
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let (base_name, locale) = detect_locale(&stem);
            let (base_name, locale) = (base_name.to_string(), locale.to_string());
            let entries = parser::parse(&path)?;
            group_map
                .entry(base_name)
                .or_default()
                .push(PropertiesFile { path, locale, entries });
        }

        let mut all_keys: HashSet<String> = HashSet::new();
        let mut groups: Vec<FileGroup> = group_map
            .into_iter()
            .map(|(base_name, mut files)| {
                files.sort_by(|a, b| match (a.locale.as_str(), b.locale.as_str()) {
                    ("default", _) => std::cmp::Ordering::Less,
                    (_, "default") => std::cmp::Ordering::Greater,
                    _ => a.locale.cmp(&b.locale),
                });
                for file in &files {
                    for entry in &file.entries {
                        if let FileEntry::KeyValue { key, .. } = entry {
                            all_keys.insert(key.clone());
                        }
                    }
                }
                FileGroup { base_name, files }
            })
            .collect();

        groups.sort_by(|a, b| a.base_name.cmp(&b.base_name));

        let mut merged_keys: Vec<String> = all_keys.into_iter().collect();
        merged_keys.sort();

        Ok(Workspace { groups, merged_keys })
    }

    /// All distinct locale strings across all groups, "default" first.
    pub fn all_locales(&self) -> Vec<String> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut out = Vec::new();
        for group in &self.groups {
            for locale in group.locales() {
                if seen.insert(locale) {
                    out.push(locale.to_string());
                }
            }
        }
        out
    }
}

/// Length of the longest common prefix of `a` and `b` that ends on a `.`
/// boundary. Property keys are ASCII so byte indexing is safe.
///
/// Examples:
///   "app.confirm.new",  "app.confirm.delete"  → 12  ("app.confirm.")
///   "app.confirm.new",  "app.other"            →  4  ("app.")
///   "com.foo",          "app.bar"              →  0  (no common dot boundary)
fn common_dot_prefix_len(a: &str, b: &str) -> usize {
    let common = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    a[..common].rfind('.').map(|i| i + 1).unwrap_or(0)
}

/// Splits a file stem into (base_name, locale).
///
/// "messages_de"   → ("messages", "de")
/// "messages"      → ("messages", "default")
/// "app_en_US"     → ("app", "en_US")   (everything after the first '_')
fn detect_locale(stem: &str) -> (&str, &str) {
    match stem.find('_') {
        Some(idx) if idx + 1 < stem.len() => (&stem[..idx], &stem[idx + 1..]),
        Some(idx) => (&stem[..idx], "default"), // trailing underscore: strip it
        None => (stem, "default"),
    }
}

#[cfg(test)]
mod tests {
    use super::detect_locale;

    #[test]
    fn locale_detection() {
        assert_eq!(detect_locale("messages_de"),   ("messages", "de"));
        assert_eq!(detect_locale("messages"),       ("messages", "default"));
        assert_eq!(detect_locale("app_en_US"),      ("app", "en_US"));
        assert_eq!(detect_locale("messages_"),      ("messages", "default")); // trailing _ → default
    }
}
