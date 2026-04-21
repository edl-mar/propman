use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;
use crate::{
    domain::DomainModel,
    parser::{self, FileEntry},
    store::Change,
    writer,
};

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

}

#[derive(Debug, Clone)]
pub struct FileGroup {
    pub base_name: String,
    /// Always sorted: "default" first, then locales alphabetically.
    pub files: Vec<PropertiesFile>,
}

impl FileGroup {
    // Used by workspace.save() — see docs/architectural_debt.md D5.
    #[allow(dead_code)]
    pub fn locales(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|f| f.locale.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub groups: Vec<FileGroup>,
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

        let mut groups: Vec<FileGroup> = group_map
            .into_iter()
            .map(|(base_name, mut files)| {
                files.sort_by(|a, b| match (a.locale.as_str(), b.locale.as_str()) {
                    ("default", _) => std::cmp::Ordering::Less,
                    (_, "default") => std::cmp::Ordering::Greater,
                    _ => a.locale.cmp(&b.locale),
                });
                FileGroup { base_name, files }
            })
            .collect();

        groups.sort_by(|a, b| a.base_name.cmp(&b.base_name));

        Ok(Workspace { groups })
    }

    /// Look up the value for `full_key` (bundle-qualified or bare) in `locale`.
    /// Routes to the correct bundle group, then looks up by the real key.
    /// Used by workspace.save() — see docs/architectural_debt.md D5.
    #[allow(dead_code)]
    pub fn get_value<'a>(&'a self, full_key: &str, locale: &str) -> Option<&'a str> {
        let (bundle, real_key) = split_key(full_key);
        self.groups.iter()
            .filter(|g| bundle.is_empty() || g.base_name == bundle)
            .flat_map(|g| g.files.iter())
            .filter(|f| f.locale == locale)
            .find_map(|f| f.get(real_key))
    }

    /// Locale strings for a specific bundle, "default" first.
    /// Falls back to `all_locales()` for bare (non-bundle) keys (`bundle` is `""`).
    /// Used by workspace.save() — see docs/architectural_debt.md D5.
    #[allow(dead_code)]
    pub fn bundle_locales(&self, bundle: &str) -> Vec<String> {
        if bundle.is_empty() {
            return self.all_locales();
        }
        self.groups.iter()
            .filter(|g| g.base_name == bundle)
            .flat_map(|g| g.locales())
            .map(|l| l.to_string())
            .collect()
    }

    /// All distinct locale strings across all groups, "default" first.
    /// Used by workspace.save() — see docs/architectural_debt.md D5.
    #[allow(dead_code)]
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

impl Workspace {
    // ── File management ───────────────────────────────────────────────────────

    /// `true` when a bundle with this name already exists.
    pub fn has_bundle(&self, name: &str) -> bool {
        self.groups.iter().any(|g| g.base_name == name)
    }

    /// `true` when `bundle` has a file for `locale`.
    pub fn has_locale(&self, bundle: &str, locale: &str) -> bool {
        self.groups.iter()
            .any(|g| g.base_name == bundle && g.files.iter().any(|f| f.locale == locale))
    }

    /// Create a new bundle on disk and register it in the workspace.
    ///
    /// Derives the target directory and first locale from the first existing
    /// bundle; falls back to cwd / `"default"` when no bundles exist yet.
    /// Returns `Ok((filename, first_locale))` or `Err(message)`.
    pub fn create_bundle(&mut self, name: &str) -> Result<(String, String), String> {
        let (dir, first_locale) = {
            let existing = self.groups.iter()
                .find(|g| !g.base_name.is_empty() && !g.files.is_empty());
            let dir = existing
                .and_then(|g| g.files.first())
                .and_then(|f| f.path.parent())
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let locale = existing
                .and_then(|g| g.files.first())
                .map(|f| f.locale.clone())
                .unwrap_or_else(|| "default".to_string());
            (dir, locale)
        };

        let filename = format!("{name}_{first_locale}.properties");
        let new_path = dir.join(&filename);

        std::fs::File::create(&new_path)
            .map_err(|e| format!("Failed to create file: {e}"))?;

        self.groups.push(FileGroup {
            base_name: name.to_string(),
            files: vec![PropertiesFile {
                path: new_path,
                locale: first_locale.clone(),
                entries: Vec::new(),
            }],
        });

        Ok((filename, first_locale))
    }

    /// Create a new locale file for `bundle` and register it in the workspace.
    ///
    /// Returns `Ok(filename)` or `Err(message)`. Returns `Ok("")` silently
    /// when the locale already exists (idempotent).
    pub fn create_locale(&mut self, bundle: &str, locale: &str) -> Result<String, String> {
        if self.has_locale(bundle, locale) {
            return Ok(String::new());
        }

        let dir = self.groups.iter()
            .find(|g| g.base_name == bundle)
            .and_then(|g| g.files.first())
            .and_then(|f| f.path.parent())
            .map(|p| p.to_path_buf())
            .ok_or_else(|| format!("Cannot find directory for bundle '{bundle}'"))?;

        let filename = if locale == "default" {
            format!("{bundle}.properties")
        } else {
            format!("{bundle}_{locale}.properties")
        };
        let new_path = dir.join(&filename);

        std::fs::File::create(&new_path)
            .map_err(|e| format!("Failed to create file: {e}"))?;

        if let Some(group) = self.groups.iter_mut().find(|g| g.base_name == bundle) {
            group.files.push(PropertiesFile {
                path: new_path,
                locale: locale.to_string(),
                entries: Vec::new(),
            });
            group.files.sort_by(|a, b| match (a.locale.as_str(), b.locale.as_str()) {
                ("default", _) => std::cmp::Ordering::Less,
                (_, "default") => std::cmp::Ordering::Greater,
                (a, b)         => a.cmp(b),
            });
        }

        Ok(filename)
    }
}

impl Workspace {
    // ── Save ─────────────────────────────────────────────────────────────────

    /// Flush all pending changes from `dm` to the `.properties` files on disk.
    ///
    /// Processes every `Change` in `dm.change_set()`:
    /// - `Insert` → append new `key=value` entry to the file.
    /// - `Update` → rewrite existing entry's value in-place.
    /// - `Delete` → remove the entry's line(s) from the file.
    ///
    /// Returns `true` when all writes succeed.  On full success `dm.clear_changes()`
    /// is called so the change log is reset to the new on-disk baseline.
    /// Partial failures leave the change log intact for retry.
    pub fn save(&mut self, dm: &mut DomainModel) -> bool {
        let changes: Vec<Change> = dm.change_set().collect();
        let mut all_ok = true;

        for change in &changes {
            let result = match change {
                Change::Insert { entry_id } => {
                    let bundle   = dm.entry_bundle_name(*entry_id).to_string();
                    let locale   = dm.entry_locale_str(*entry_id).to_string();
                    let real_key = dm.entry_real_key(*entry_id);
                    match dm.entry_current_value(*entry_id) {
                        Some(v) => self.write_insert_entry(&bundle, &locale, &real_key, v),
                        None    => Ok(()), // entry deleted before first save — net no-op
                    }
                }
                Change::Update { entry_id } => {
                    let bundle   = dm.entry_bundle_name(*entry_id).to_string();
                    let locale   = dm.entry_locale_str(*entry_id).to_string();
                    let real_key = dm.entry_real_key(*entry_id);
                    match dm.entry_current_value(*entry_id) {
                        Some(v) => self.write_update_entry(&bundle, &locale, &real_key, v),
                        None    => Ok(()),
                    }
                }
                Change::Delete { entry_id } => {
                    let bundle   = dm.entry_bundle_name(*entry_id).to_string();
                    let locale   = dm.entry_locale_str(*entry_id).to_string();
                    let real_key = dm.entry_real_key(*entry_id);
                    self.write_delete_entry(&bundle, &locale, &real_key)
                }
            };
            if result.is_err() {
                all_ok = false;
            }
        }

        if all_ok {
            dm.clear_changes();
        }
        all_ok
    }

    fn find_file_idx(&self, bundle: &str, locale: &str) -> io::Result<(usize, usize)> {
        for (gi, group) in self.groups.iter().enumerate() {
            if !bundle.is_empty() && group.base_name != bundle { continue; }
            for (fi, file) in group.files.iter().enumerate() {
                if file.locale == locale {
                    return Ok((gi, fi));
                }
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound,
            format!("no file for bundle={bundle:?} locale={locale:?}")))
    }

    fn find_entry_idx(&self, gi: usize, fi: usize, real_key: &str) -> io::Result<usize> {
        self.groups[gi].files[fi].entries.iter().position(|e| {
            matches!(e, FileEntry::KeyValue { key, .. } if key == real_key)
        }).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound,
            format!("key not found: {real_key}")))
    }

    fn write_insert_entry(&mut self, bundle: &str, locale: &str, real_key: &str, value: &str) -> io::Result<()> {
        let (gi, fi) = self.find_file_idx(bundle, locale)?;
        let after_line = self.groups[gi].files[fi].insertion_point_for(real_key);
        let n_lines    = value.split('\n').count();
        let path       = self.groups[gi].files[fi].path.clone();

        writer::write_insert(&path, after_line, real_key, value)?;

        for entry in &mut self.groups[gi].files[fi].entries {
            match entry {
                FileEntry::KeyValue { first_line, last_line, .. } => {
                    if *first_line > after_line { *first_line += n_lines; *last_line += n_lines; }
                }
                FileEntry::Comment { line, .. } | FileEntry::Blank { line } => {
                    if *line > after_line { *line += n_lines; }
                }
            }
        }
        self.groups[gi].files[fi].entries.push(FileEntry::KeyValue {
            first_line: after_line + 1,
            last_line:  after_line + n_lines,
            key:   real_key.to_string(),
            value: value.to_string(),
        });
        Ok(())
    }

    fn write_update_entry(&mut self, bundle: &str, locale: &str, real_key: &str, value: &str) -> io::Result<()> {
        let (gi, fi) = self.find_file_idx(bundle, locale)?;
        let ei       = self.find_entry_idx(gi, fi, real_key)?;
        let (path, first_line, last_line) = match &self.groups[gi].files[fi].entries[ei] {
            FileEntry::KeyValue { first_line, last_line, .. } =>
                (self.groups[gi].files[fi].path.clone(), *first_line, *last_line),
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "expected KeyValue")),
        };

        writer::write_change(&path, first_line, last_line, real_key, value)?;

        let old_count = last_line - first_line + 1;
        let new_count = value.split('\n').count();
        let new_last  = first_line + new_count - 1;

        if let FileEntry::KeyValue { value: v, last_line: ll, .. } = &mut self.groups[gi].files[fi].entries[ei] {
            *v  = value.to_string();
            *ll = new_last;
        }

        if new_count != old_count {
            let delta = new_count as isize - old_count as isize;
            for (j, entry) in self.groups[gi].files[fi].entries.iter_mut().enumerate() {
                if j == ei { continue; }
                match entry {
                    FileEntry::KeyValue { first_line: fl, last_line: ll, .. } => {
                        if *fl > last_line {
                            *fl = (*fl as isize + delta) as usize;
                            *ll = (*ll as isize + delta) as usize;
                        }
                    }
                    FileEntry::Comment { line, .. } | FileEntry::Blank { line } => {
                        if *line > last_line { *line = (*line as isize + delta) as usize; }
                    }
                }
            }
        }

        Ok(())
    }

    fn write_delete_entry(&mut self, bundle: &str, locale: &str, real_key: &str) -> io::Result<()> {
        let (gi, fi) = self.find_file_idx(bundle, locale)?;
        let ei       = self.find_entry_idx(gi, fi, real_key)?;
        let (path, first_line, last_line) = match &self.groups[gi].files[fi].entries[ei] {
            FileEntry::KeyValue { first_line, last_line, .. } =>
                (self.groups[gi].files[fi].path.clone(), *first_line, *last_line),
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "expected KeyValue")),
        };

        writer::write_delete(&path, first_line, last_line)?;

        let n_lines = last_line - first_line + 1;
        let rk = real_key.to_string();
        self.groups[gi].files[fi].entries.retain(|e| {
            !matches!(e, FileEntry::KeyValue { key, .. } if *key == rk)
        });
        for entry in &mut self.groups[gi].files[fi].entries {
            match entry {
                FileEntry::KeyValue { first_line: fl, last_line: ll, .. } => {
                    if *fl > last_line { *fl -= n_lines; *ll -= n_lines; }
                }
                FileEntry::Comment { line, .. } | FileEntry::Blank { line } => {
                    if *line > last_line { *line -= n_lines; }
                }
            }
        }
        Ok(())
    }
}

/// Splits a bundle-qualified key into `(bundle, real_key)`.
///
/// `"messages:app.title"` → `("messages", "app.title")`
/// `"app.title"`          → `("", "app.title")`  (no bundle prefix)
fn split_key(full_key: &str) -> (&str, &str) {
    match full_key.find(':') {
        Some(idx) => (&full_key[..idx], &full_key[idx + 1..]),
        None => ("", full_key),
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
