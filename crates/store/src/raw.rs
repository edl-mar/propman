//! The raw unit: what is on the disk, with its interpretation.
//!
//! Its tables are bundles, files and lines. The unit is only ever filled from the disk, and it
//! guards its own guarantees:
//! - a file is identified by its path and added once; adding it again is refused and leaves
//!   nothing behind;
//! - a bundle is identified by its folder and name, and asking for it finds the existing one;
//! - the assignment of a file always refers to a bundle of this store;
//! - a file and all its lines are created together, in one edit, and every table creates its row
//!   once for all its columns.
//!
//! The lines of a file are one value of the file, a span of rows of the line table, so that a
//! later load can add a new span without changing the old lines. The order of the lines of a
//! file is the order inside its span.
//!
//! The columns are private. The `Store` calls the methods of this unit with the edit and the
//! pools; nothing here is visible outside the crate except the ids.

use std::collections::HashMap;
use std::path::Path;

use columns::{Generation, Idx};
use types::{Confidence, Encoding, Fingerprint, LineEnding};

use crate::history::{Column, Journal, Policy, Restorable, Source, Version};
use crate::pools::{BytesId, PathId, Pools, TextId};
use crate::store::{BundleAssignment, Ctx, FileInput, LineView};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId {
    generation: Generation,
    index: usize,
}

impl Idx for FileId {
    fn idx(self) -> usize {
        self.index
    }

    fn generation(self) -> Generation {
        self.generation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BundleId {
    generation: Generation,
    index: usize,
}

impl Idx for BundleId {
    fn idx(self) -> usize {
        self.index
    }

    fn generation(self) -> Generation {
        self.generation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct LineId {
    generation: Generation,
    index: usize,
}

impl Idx for LineId {
    fn idx(self) -> usize {
        self.index
    }

    fn generation(self) -> Generation {
        self.generation
    }
}

/// The lines that one load of a file created: consecutive rows of the line table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineSpan {
    first: usize,
    len: usize,
}

pub(crate) struct Raw {
    // bundles
    bundle_folder: Column<BundleId, PathId>,
    bundle_name: Column<BundleId, TextId>,
    bundle_excluded: Column<BundleId, bool>,
    bundles: Vec<BundleId>,
    bundle_index: HashMap<(PathId, TextId), BundleId>,
    // files
    file_path: Column<FileId, PathId>,
    file_assignment: Column<FileId, BundleAssignment>,
    file_encoding: Column<FileId, Encoding>,
    file_fingerprint: Column<FileId, Fingerprint>,
    file_bytes: Column<FileId, BytesId>,
    file_lines: Column<FileId, LineSpan>,
    files: Vec<FileId>,
    file_index: HashMap<PathId, FileId>,
    // lines
    line_content: Column<LineId, TextId>,
    line_ending: Column<LineId, LineEnding>,
}

impl Raw {
    pub(crate) fn new(journal: &mut Journal) -> Self {
        Self {
            bundle_folder: journal.column(Policy::Precedence),
            bundle_name: journal.column(Policy::Precedence),
            bundle_excluded: journal.column(Policy::Precedence),
            bundles: Vec::new(),
            bundle_index: HashMap::new(),
            file_path: journal.column(Policy::Precedence),
            file_assignment: journal.column(Policy::Precedence),
            file_encoding: journal.column(Policy::Precedence),
            file_fingerprint: journal.column(Policy::Conflict),
            file_bytes: journal.column(Policy::Conflict),
            file_lines: journal.column(Policy::Conflict),
            files: Vec::new(),
            file_index: HashMap::new(),
            line_content: journal.column(Policy::Conflict),
            line_ending: journal.column(Policy::Conflict),
        }
    }

    fn generation(&self) -> Generation {
        self.bundle_name.generation()
    }

    // ── Writing ──────────────────────────────────────────────────────────────

    /// The bundle with this folder and name; created if there is none.
    pub(crate) fn bundle(&mut self, ctx: &mut Ctx<'_, '_>, folder: &Path, name: &str) -> BundleId {
        let key = (ctx.pools.add_path(folder), ctx.pools.add_text(name));
        if let Some(&existing) = self.bundle_index.get(&key) {
            return existing;
        }
        let index = self.bundle_folder.push_row(ctx.edit, key.0);
        let name_index = self.bundle_name.push_row(ctx.edit, key.1);
        let excluded_index = self.bundle_excluded.push_row(ctx.edit, false);
        debug_assert!(index == name_index && index == excluded_index);
        let id = BundleId {
            generation: self.generation(),
            index,
        };
        self.bundle_index.insert(key, id);
        self.bundles.push(id);
        id
    }

    /// Adds a file with all its lines and, if needed, its bundle. `None` if the path is there.
    pub(crate) fn add_file(
        &mut self,
        ctx: &mut Ctx<'_, '_>,
        input: FileInput<'_>,
    ) -> Option<FileId> {
        let path = ctx.pools.add_path(input.path);
        if self.file_index.contains_key(&path) {
            return None;
        }

        let folder = input.path.parent().unwrap_or(Path::new(""));
        let bundle = self.bundle(ctx, folder, input.bundle);
        let locale = input.locale.map(|locale| ctx.pools.add_text(locale));

        let first = self.line_content.len();
        for line in input.lines {
            let content = ctx.pools.add_text(line.content);
            let content_index = self.line_content.push_row(ctx.edit, content);
            let ending_index = self.line_ending.push_row(ctx.edit, line.ending);
            debug_assert_eq!(content_index, ending_index);
        }
        let span = LineSpan {
            first,
            len: input.lines.len(),
        };

        let assignment = BundleAssignment {
            bundle,
            locale,
            confidence: input.confidence,
        };
        let bytes = ctx.pools.add_bytes(input.bytes);

        let index = self.file_path.push_row(ctx.edit, path);
        let indices = [
            self.file_assignment.push_row(ctx.edit, assignment),
            self.file_encoding.push_row(ctx.edit, input.encoding),
            self.file_fingerprint.push_row(ctx.edit, input.fingerprint),
            self.file_bytes.push_row(ctx.edit, bytes),
            self.file_lines.push_row(ctx.edit, span),
        ];
        debug_assert!(indices.iter().all(|&other| other == index));

        let id = FileId {
            generation: self.generation(),
            index,
        };
        self.file_index.insert(path, id);
        self.files.push(id);
        Some(id)
    }

    pub(crate) fn set_encoding(
        &mut self,
        ctx: &mut Ctx<'_, '_>,
        file: FileId,
        encoding: Encoding,
    ) -> bool {
        ctx.edit.set(&mut self.file_encoding, file, encoding)
    }

    pub(crate) fn set_assignment(
        &mut self,
        ctx: &mut Ctx<'_, '_>,
        file: FileId,
        bundle: BundleId,
        locale: Option<&str>,
        confidence: Confidence,
    ) -> bool {
        // the file and the bundle must belong to this store
        if self.file_assignment.current(file).is_none()
            || self.bundle_excluded.current(bundle).is_none()
        {
            return false;
        }
        let locale = locale.map(|locale| ctx.pools.add_text(locale));
        let assignment = BundleAssignment {
            bundle,
            locale,
            confidence,
        };
        ctx.edit.set(&mut self.file_assignment, file, assignment)
    }

    pub(crate) fn exclude_bundle(
        &mut self,
        ctx: &mut Ctx<'_, '_>,
        bundle: BundleId,
        excluded: bool,
    ) -> bool {
        ctx.edit.set(&mut self.bundle_excluded, bundle, excluded)
    }

    pub(crate) fn mark_file_saved(&mut self, ctx: &mut Ctx<'_, '_>, file: FileId) -> bool {
        // both, so that neither is skipped
        let encoding = ctx.edit.mark_saved(&mut self.file_encoding, file);
        let assignment = ctx.edit.mark_saved(&mut self.file_assignment, file);
        encoding | assignment
    }

    pub(crate) fn mark_bundle_saved(&mut self, ctx: &mut Ctx<'_, '_>, bundle: BundleId) -> bool {
        ctx.edit.mark_saved(&mut self.bundle_excluded, bundle)
    }

    /// Restores what the edit `target` changed. Only the columns that a user can change take
    /// part; the others are only ever written by loading.
    pub(crate) fn undo(&mut self, ctx: &mut Ctx<'_, '_>, target: crate::history::EditId) {
        ctx.edit.undo(
            target,
            &mut [
                &mut self.file_encoding as &mut dyn Restorable,
                &mut self.file_assignment as &mut dyn Restorable,
                &mut self.bundle_excluded as &mut dyn Restorable,
            ],
        );
    }

    // ── Reading ──────────────────────────────────────────────────────────────

    pub(crate) fn files(&self) -> &[FileId] {
        &self.files
    }

    pub(crate) fn bundles(&self) -> &[BundleId] {
        &self.bundles
    }

    pub(crate) fn file_by_path(&self, pools: &Pools, path: &Path) -> Option<FileId> {
        let id = pools.find_path(path)?;
        self.file_index.get(&id).copied()
    }

    pub(crate) fn file_path(&self, file: FileId) -> Option<PathId> {
        self.file_path.current(file)
    }

    pub(crate) fn file_bytes(&self, file: FileId) -> Option<BytesId> {
        self.file_bytes.current(file)
    }

    pub(crate) fn file_encoding(&self, file: FileId) -> Option<Encoding> {
        self.file_encoding.current(file)
    }

    pub(crate) fn file_fingerprint(&self, file: FileId) -> Option<Fingerprint> {
        self.file_fingerprint.current(file)
    }

    pub(crate) fn file_assignment(&self, file: FileId) -> Option<BundleAssignment> {
        self.file_assignment.current(file)
    }

    /// The lines of the current span of a file, in order.
    pub(crate) fn file_lines<'a>(&self, pools: &'a Pools, file: FileId) -> Vec<LineView<'a>> {
        let Some(span) = self.file_lines.current(file) else {
            return Vec::new();
        };
        (span.first..span.first + span.len)
            .filter_map(|index| {
                let line = LineId {
                    generation: self.generation(),
                    index,
                };
                Some(LineView {
                    content: pools.text(self.line_content.current(line)?)?,
                    ending: self.line_ending.current(line)?,
                })
            })
            .collect()
    }

    pub(crate) fn bundle_folder(&self, bundle: BundleId) -> Option<PathId> {
        self.bundle_folder.current(bundle)
    }

    pub(crate) fn bundle_name(&self, bundle: BundleId) -> Option<TextId> {
        self.bundle_name.current(bundle)
    }

    pub(crate) fn bundle_excluded(&self, bundle: BundleId) -> Option<bool> {
        self.bundle_excluded.current(bundle)
    }

    /// The files whose current assignment is this bundle, in the order they were loaded.
    pub(crate) fn files_of(&self, bundle: BundleId) -> Vec<FileId> {
        if self.bundle_excluded.current(bundle).is_none() {
            return Vec::new();
        }
        self.files
            .iter()
            .copied()
            .filter(|&file| {
                self.file_assignment
                    .current(file)
                    .is_some_and(|assignment| assignment.bundle == bundle)
            })
            .collect()
    }

    pub(crate) fn encoding_versions(
        &self,
        journal: &Journal,
        file: FileId,
    ) -> Vec<Version<Encoding>> {
        self.file_encoding.versions(journal, file)
    }

    pub(crate) fn assignment_versions(
        &self,
        journal: &Journal,
        file: FileId,
    ) -> Vec<Version<BundleAssignment>> {
        self.file_assignment.versions(journal, file)
    }

    pub(crate) fn excluded_versions(
        &self,
        journal: &Journal,
        bundle: BundleId,
    ) -> Vec<Version<bool>> {
        self.bundle_excluded.versions(journal, bundle)
    }

    pub(crate) fn encoding_base(
        &self,
        journal: &Journal,
        file: FileId,
        source: Source,
    ) -> Option<Encoding> {
        self.file_encoding.base(journal, file, source)
    }

    pub(crate) fn file_is_dirty(&self, journal: &Journal, file: FileId, source: Source) -> bool {
        self.file_encoding.is_dirty(journal, file, source)
            || self.file_assignment.is_dirty(journal, file, source)
    }

    pub(crate) fn bundle_is_dirty(
        &self,
        journal: &Journal,
        bundle: BundleId,
        source: Source,
    ) -> bool {
        self.bundle_excluded.is_dirty(journal, bundle, source)
    }
}
