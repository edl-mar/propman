//! The store: the single source of truth for the data loaded from the disk and the decisions of
//! the user about it. This is the part that `app` sees.
//!
//! All columns are private. `app` reads through methods that give back plain values and
//! resolved text, and writes through a `StoreEdit` with a fixed set of operations, so no
//! operation can break an invariant. Every id carries the generation of its store; an id of
//! another store is unknown (reading gives nothing, writing is refused and changes nothing).
//!
//! The first unit is the raw unit (see `raw`): what is on the disk, with its interpretation.
//! The pools are owned by the store and shared by its units. Guarantees between units belong
//! here; guarantees inside a unit belong to the unit.
//!
//! What a user decides (the encoding of a file, the assignment of a file to a bundle and
//! locale, whether a bundle is excluded) is a new version of the value with the source `USER`;
//! the version from the disk stays as the base. See `history` for the rules.

use std::path::Path;

use types::{Confidence, Encoding, Fingerprint, LineEnding};

use crate::history::{Edit, EditId, EditKind, Journal, Source, Version};
use crate::pools::{BytesId, PathId, Pools, TextId};
use crate::raw::{BundleId, FileId, Raw};

// ── Values ───────────────────────────────────────────────────────────────────

/// To which bundle and locale a file belongs, and how sure that is. One value, because the
/// three always change together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleAssignment {
    pub bundle: BundleId,
    /// `None` for the default file of the bundle.
    pub locale: Option<TextId>,
    pub confidence: Confidence,
}

/// A line as the store gets it, without the store knowing where it comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineInput<'a> {
    pub content: &'a str,
    pub ending: LineEnding,
}

/// A line as the store gives it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineView<'a> {
    pub content: &'a str,
    pub ending: LineEnding,
}

/// Everything the store needs to know about a file that was read.
#[derive(Debug, Clone, Copy)]
pub struct FileInput<'a> {
    pub path: &'a Path,
    /// The bytes exactly as they were read.
    pub bytes: &'a [u8],
    pub encoding: Encoding,
    pub fingerprint: Fingerprint,
    pub lines: &'a [LineInput<'a>],
    /// The base name of the bundle; its folder is the folder of `path`.
    pub bundle: &'a str,
    pub locale: Option<&'a str>,
    pub confidence: Confidence,
}

/// What an operation of a unit needs besides its own tables: the edit it belongs to and the
/// pools.
pub(crate) struct Ctx<'a, 'j> {
    pub(crate) edit: &'a mut Edit<'j>,
    pub(crate) pools: &'a mut Pools,
}

// ── The store ────────────────────────────────────────────────────────────────

pub struct Store {
    journal: Journal,
    pools: Pools,
    raw: Raw,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    pub fn new() -> Self {
        let mut journal = Journal::new();
        let pools = Pools::new(journal.generation());
        let raw = Raw::new(&mut journal);
        Self {
            journal,
            pools,
            raw,
        }
    }

    pub fn generation(&self) -> columns::Generation {
        self.journal.generation()
    }

    /// Every change is made through an edit, which may bundle several operations.
    pub fn edit(&mut self, kind: EditKind, source: Source) -> StoreEdit<'_> {
        let Store {
            journal,
            pools,
            raw,
        } = self;
        StoreEdit {
            edit: journal.edit(kind, source),
            pools,
            raw,
        }
    }

    // reading: files

    pub fn files(&self) -> Vec<FileId> {
        self.raw.files().to_vec()
    }

    pub fn file_by_path(&self, path: &Path) -> Option<FileId> {
        self.raw.file_by_path(&self.pools, path)
    }

    pub fn file_path(&self, file: FileId) -> Option<&Path> {
        self.pools.path(self.raw.file_path(file)?)
    }

    pub fn file_bytes(&self, file: FileId) -> Option<&[u8]> {
        self.pools.bytes(self.raw.file_bytes(file)?)
    }

    pub fn file_encoding(&self, file: FileId) -> Option<Encoding> {
        self.raw.file_encoding(file)
    }

    pub fn file_fingerprint(&self, file: FileId) -> Option<Fingerprint> {
        self.raw.file_fingerprint(file)
    }

    pub fn file_assignment(&self, file: FileId) -> Option<BundleAssignment> {
        self.raw.file_assignment(file)
    }

    /// The lines of a file in order; empty if the file is unknown.
    pub fn file_lines(&self, file: FileId) -> Vec<LineView<'_>> {
        self.raw.file_lines(&self.pools, file)
    }

    // reading: bundles

    pub fn bundles(&self) -> Vec<BundleId> {
        self.raw.bundles().to_vec()
    }

    pub fn bundle_folder(&self, bundle: BundleId) -> Option<&Path> {
        self.pools.path(self.raw.bundle_folder(bundle)?)
    }

    pub fn bundle_name(&self, bundle: BundleId) -> Option<&str> {
        self.pools.text(self.raw.bundle_name(bundle)?)
    }

    pub fn bundle_excluded(&self, bundle: BundleId) -> Option<bool> {
        self.raw.bundle_excluded(bundle)
    }

    /// The files whose current assignment is this bundle, in the order they were loaded.
    pub fn files_of(&self, bundle: BundleId) -> Vec<FileId> {
        self.raw.files_of(bundle)
    }

    // reading: pools

    pub fn text(&self, id: TextId) -> Option<&str> {
        self.pools.text(id)
    }

    pub fn path(&self, id: PathId) -> Option<&Path> {
        self.pools.path(id)
    }

    pub fn bytes(&self, id: BytesId) -> Option<&[u8]> {
        self.pools.bytes(id)
    }

    // reading: histories and bases

    pub fn encoding_versions(&self, file: FileId) -> Vec<Version<Encoding>> {
        self.raw.encoding_versions(&self.journal, file)
    }

    pub fn assignment_versions(&self, file: FileId) -> Vec<Version<BundleAssignment>> {
        self.raw.assignment_versions(&self.journal, file)
    }

    pub fn excluded_versions(&self, bundle: BundleId) -> Vec<Version<bool>> {
        self.raw.excluded_versions(&self.journal, bundle)
    }

    /// The encoding of the base of `source`: the latest version from it, else version 0.
    pub fn encoding_base(&self, file: FileId, source: Source) -> Option<Encoding> {
        self.raw.encoding_base(&self.journal, file, source)
    }

    /// Whether the encoding or the assignment of a file differs from the base of `source`.
    pub fn file_is_dirty(&self, file: FileId, source: Source) -> bool {
        self.raw.file_is_dirty(&self.journal, file, source)
    }

    /// Whether the excluded flag of a bundle differs from the base of `source`.
    pub fn bundle_is_dirty(&self, bundle: BundleId, source: Source) -> bool {
        self.raw.bundle_is_dirty(&self.journal, bundle, source)
    }
}

/// The fixed set of operations that change the store.
pub struct StoreEdit<'s> {
    edit: Edit<'s>,
    pools: &'s mut Pools,
    raw: &'s mut Raw,
}

impl StoreEdit<'_> {
    /// Adds a file with all its lines, and its bundle if there is none with this folder and
    /// name. Returns `None` if a file with this path exists already.
    pub fn add_file(&mut self, input: FileInput<'_>) -> Option<FileId> {
        let mut ctx = Ctx {
            edit: &mut self.edit,
            pools: &mut *self.pools,
        };
        self.raw.add_file(&mut ctx, input)
    }

    /// The bundle with this folder and name; created if there is none.
    pub fn bundle(&mut self, folder: &Path, name: &str) -> BundleId {
        let mut ctx = Ctx {
            edit: &mut self.edit,
            pools: &mut *self.pools,
        };
        self.raw.bundle(&mut ctx, folder, name)
    }

    /// Returns whether it changed anything: false for the current encoding or an unknown file.
    pub fn set_encoding(&mut self, file: FileId, encoding: Encoding) -> bool {
        let mut ctx = Ctx {
            edit: &mut self.edit,
            pools: &mut *self.pools,
        };
        self.raw.set_encoding(&mut ctx, file, encoding)
    }

    /// Returns whether it changed anything: false for the current assignment, an unknown file or
    /// a bundle of another store.
    pub fn set_assignment(
        &mut self,
        file: FileId,
        bundle: BundleId,
        locale: Option<&str>,
        confidence: Confidence,
    ) -> bool {
        let mut ctx = Ctx {
            edit: &mut self.edit,
            pools: &mut *self.pools,
        };
        self.raw
            .set_assignment(&mut ctx, file, bundle, locale, confidence)
    }

    /// Returns whether it changed anything.
    pub fn exclude_bundle(&mut self, bundle: BundleId, excluded: bool) -> bool {
        let mut ctx = Ctx {
            edit: &mut self.edit,
            pools: &mut *self.pools,
        };
        self.raw.exclude_bundle(&mut ctx, bundle, excluded)
    }

    /// Marks encoding and assignment of a file as saved to the source of this edit. Returns
    /// whether a marker was appended.
    pub fn mark_file_saved(&mut self, file: FileId) -> bool {
        let mut ctx = Ctx {
            edit: &mut self.edit,
            pools: &mut *self.pools,
        };
        self.raw.mark_file_saved(&mut ctx, file)
    }

    /// Marks the excluded flag of a bundle as saved to the source of this edit.
    pub fn mark_bundle_saved(&mut self, bundle: BundleId) -> bool {
        let mut ctx = Ctx {
            edit: &mut self.edit,
            pools: &mut *self.pools,
        };
        self.raw.mark_bundle_saved(&mut ctx, bundle)
    }

    /// Restores what the edit `target` changed.
    pub fn undo(&mut self, target: EditId) {
        let mut ctx = Ctx {
            edit: &mut self.edit,
            pools: &mut *self.pools,
        };
        self.raw.undo(&mut ctx, target);
    }

    pub fn commit(self) -> EditId {
        self.edit.commit()
    }
}
