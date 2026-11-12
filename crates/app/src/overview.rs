//! The bundle overview as a plain list, derived from the store each time it is needed.
//!
//! The list has three kinds of items: a folder, below it its bundles, below each bundle its
//! files. Folders are sorted by their path, bundles by name, and inside a bundle the default
//! file comes first and then the locales in alphabetical order. A folder is shown relative to the
//! folder that was opened (`.` for that folder itself), with `/` as separator. A file belongs to
//! the bundle of its current assignment. All text is already resolved, so a view only draws.

use std::path::Path;

use store::{BundleId, FileId};
use types::{Confidence, Encoding};

use crate::session::Session;

/// What identifies an item of the overview, whatever its place in the list. A folder has no id in
/// the store, it is a path, so its identity is the path relative to the opened folder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ItemId {
    Folder(String),
    Bundle(BundleId),
    File(FileId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverviewItem {
    Folder {
        /// Relative to the opened folder.
        path: String,
        bundles: usize,
    },
    Bundle {
        id: BundleId,
        name: String,
        files: usize,
        excluded: bool,
    },
    File {
        id: FileId,
        /// The name of the file, without its folder.
        name: String,
        /// `None` for the default file of the bundle.
        locale: Option<String>,
        confidence: Confidence,
        encoding: Encoding,
        lines: usize,
    },
}

impl OverviewItem {
    pub fn id(&self) -> ItemId {
        match self {
            OverviewItem::Folder { path, .. } => ItemId::Folder(path.clone()),
            OverviewItem::Bundle { id, .. } => ItemId::Bundle(*id),
            OverviewItem::File { id, .. } => ItemId::File(*id),
        }
    }
}

/// A folder relative to the opened one, with `/` as separator and `.` for the folder itself.
pub(crate) fn relative_folder(root: &Path, folder: &Path) -> String {
    let relative = folder.strip_prefix(root).unwrap_or(folder);
    let text = relative.display().to_string().replace('\\', "/");
    if text.is_empty() {
        ".".to_owned()
    } else {
        text
    }
}

pub fn overview(session: &Session) -> Vec<OverviewItem> {
    let store = session.store();

    let mut bundles: Vec<(String, String, BundleId)> = store
        .bundles()
        .into_iter()
        .filter_map(|id| {
            let folder = relative_folder(session.root(), store.bundle_folder(id)?);
            Some((folder, store.bundle_name(id)?.to_owned(), id))
        })
        .collect();
    bundles.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));

    let mut items = Vec::new();
    let mut current_folder: Option<&str> = None;
    for (folder, name, id) in &bundles {
        if current_folder != Some(folder.as_str()) {
            current_folder = Some(folder);
            let in_folder = bundles
                .iter()
                .filter(|(other, _, _)| other == folder)
                .count();
            items.push(OverviewItem::Folder {
                path: folder.clone(),
                bundles: in_folder,
            });
        }

        let mut files: Vec<OverviewItem> = store
            .files_of(*id)
            .into_iter()
            .filter_map(|file| {
                let assignment = store.file_assignment(file)?;
                Some(OverviewItem::File {
                    id: file,
                    name: store
                        .file_path(file)?
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    locale: assignment
                        .locale
                        .and_then(|locale| store.text(locale))
                        .map(str::to_owned),
                    confidence: assignment.confidence,
                    encoding: store.file_encoding(file)?,
                    lines: store.file_lines(file).len(),
                })
            })
            .collect();
        // the default file first, then the locales in alphabetical order
        files.sort_by(|a, b| match (a, b) {
            (
                OverviewItem::File {
                    locale: la,
                    name: na,
                    ..
                },
                OverviewItem::File {
                    locale: lb,
                    name: nb,
                    ..
                },
            ) => (la.is_some(), la, na).cmp(&(lb.is_some(), lb, nb)),
            _ => std::cmp::Ordering::Equal,
        });

        items.push(OverviewItem::Bundle {
            id: *id,
            name: name.clone(),
            files: files.len(),
            excluded: store.bundle_excluded(*id).unwrap_or(false),
        });
        items.extend(files);
    }
    items
}
