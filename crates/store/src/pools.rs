//! The pools of a store: text, paths and bytes, each stored once with a stable id.
//!
//! The pools are owned by the `Store` and shared by all its units, so that equal text has the
//! same id everywhere. The ids are made here, the only place that knows how, from the index that
//! a pool returns and the generation of the store.

use std::path::Path;

use columns::{Generation, Idx, Pool};

/// The id of a text in the text pool. Equal texts have equal ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextId {
    generation: Generation,
    index: usize,
}

impl Idx for TextId {
    fn idx(self) -> usize {
        self.index
    }

    fn generation(self) -> Generation {
        self.generation
    }
}

/// The id of a path in the path pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathId {
    generation: Generation,
    index: usize,
}

impl Idx for PathId {
    fn idx(self) -> usize {
        self.index
    }

    fn generation(self) -> Generation {
        self.generation
    }
}

/// The id of a byte sequence in the byte pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BytesId {
    generation: Generation,
    index: usize,
}

impl Idx for BytesId {
    fn idx(self) -> usize {
        self.index
    }

    fn generation(self) -> Generation {
        self.generation
    }
}

pub(crate) struct Pools {
    generation: Generation,
    texts: Pool<TextId, str>,
    paths: Pool<PathId, Path>,
    bytes: Pool<BytesId, [u8]>,
}

impl Pools {
    pub(crate) fn new(generation: Generation) -> Self {
        Self {
            generation,
            texts: Pool::new(generation),
            paths: Pool::new(generation),
            bytes: Pool::new(generation),
        }
    }

    pub(crate) fn add_text(&mut self, text: &str) -> TextId {
        TextId {
            generation: self.generation,
            index: self.texts.add(text),
        }
    }

    pub(crate) fn add_path(&mut self, path: &Path) -> PathId {
        PathId {
            generation: self.generation,
            index: self.paths.add(path),
        }
    }

    pub(crate) fn add_bytes(&mut self, bytes: &[u8]) -> BytesId {
        BytesId {
            generation: self.generation,
            index: self.bytes.add(bytes),
        }
    }

    /// The id of an equal path, without adding anything.
    pub(crate) fn find_path(&self, path: &Path) -> Option<PathId> {
        self.paths.find(path).map(|index| PathId {
            generation: self.generation,
            index,
        })
    }

    pub(crate) fn text(&self, id: TextId) -> Option<&str> {
        self.texts.get(id)
    }

    pub(crate) fn path(&self, id: PathId) -> Option<&Path> {
        self.paths.get(id)
    }

    pub(crate) fn bytes(&self, id: BytesId) -> Option<&[u8]> {
        self.bytes.get(id)
    }
}
