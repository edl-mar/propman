//! Append-only storage for large values, with deduplication and stable ids.
//!
//! A pool holds values such as text (`str`), bytes (`[u8]`) or paths (`Path`) once each. Adding
//! a value whose content is already there gives back the existing index, so two equal values
//! have the same id and comparing values is comparing ids. Nothing is ever removed or moved to
//! another index. Like `HistoryColumn::push_row`, the pool only returns plain indices and never
//! makes an id: the owner of the pool turns the index into its own id type.
//!
//! Equality is decided by content. A hash narrows the search, and a hit is confirmed by
//! comparing the content, so a collision never mixes up two different values. The hash is
//! never stored or compared outside the pool.
//!
//! A pool belongs to one generation. An id of another generation is unknown to it, even if its
//! index exists in this pool.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use crate::{Generation, Idx};

pub struct Pool<Id, T: ?Sized> {
    generation: Generation,
    values: Vec<Box<T>>,
    /// Indices of the values with a given hash. A value is only ever in the bucket of its hash,
    /// so the content is stored once.
    buckets: HashMap<u64, Vec<usize>>,
    _id: PhantomData<Id>,
}

fn hash_of<T: ?Sized + Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

impl<Id: Idx, T: ?Sized + Hash + Eq> Pool<Id, T> {
    pub fn new(generation: Generation) -> Self {
        Self {
            generation,
            values: Vec::new(),
            buckets: HashMap::new(),
            _id: PhantomData,
        }
    }

    /// The generation this pool belongs to; the owner makes its ids with it.
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// The number of different values.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The index of an equal value in the bucket of `hash`; the content decides, not the hash.
    fn find_hashed(&self, hash: u64, value: &T) -> Option<usize> {
        self.buckets.get(&hash)?.iter().copied().find(|&index| {
            self.values
                .get(index)
                .is_some_and(|stored| &**stored == value)
        })
    }

    /// Adds a value and returns its index, or the existing index if an equal value is there.
    /// The value is only copied if it is new.
    pub fn add(&mut self, value: &T) -> usize
    where
        Box<T>: for<'a> From<&'a T>,
    {
        let hash = hash_of(value);
        if let Some(index) = self.find_hashed(hash, value) {
            return index;
        }
        self.values.push(Box::from(value));
        let index = self.values.len() - 1;
        self.buckets.entry(hash).or_default().push(index);
        index
    }

    /// The index of an equal value, without adding anything.
    pub fn find(&self, value: &T) -> Option<usize> {
        self.find_hashed(hash_of(value), value)
    }

    /// The value of an id; `None` for an unknown index or an id of another generation.
    pub fn get(&self, id: Id) -> Option<&T> {
        if id.generation() != self.generation {
            return None;
        }
        self.values.get(id.idx()).map(|stored| &**stored)
    }
}
