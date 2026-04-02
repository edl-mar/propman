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
