//! The cursor and the scroll position of a list of rows, where every row has an id.
//!
//! The cursor is the id of a row, and the index is only what the id is at the moment. Every call
//! gets the ids of the rows as they are now, finds the cursor among them first and moves from
//! there, so the cursor stays on its row when the rows change their order, and a caller never
//! converts between an index and an id. If the row of the cursor is gone, the cursor stays at
//! its index, which is then the neighbour. The height of the visible part is told by the caller;
//! 0 means that it is not known. The scroll position moves as little as possible to keep the
//! cursor visible, and not further than the last row at the bottom.

/// A movement of the cursor. A page is as many rows as fit, or one row while the height is
/// unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    Up,
    Down,
    PageUp,
    PageDown,
    First,
    Last,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListState<I> {
    cursor: Option<I>,
    index: usize,
    scroll: usize,
    height: usize,
}

impl<I> Default for ListState<I> {
    fn default() -> Self {
        Self {
            cursor: None,
            index: 0,
            scroll: 0,
            height: 0,
        }
    }
}

impl<I: Clone + PartialEq> ListState<I> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves the cursor and keeps it visible.
    pub fn apply(&mut self, movement: Movement, ids: &[I]) {
        self.locate(ids);
        let last = ids.len().saturating_sub(1);
        let page = self.height.max(1);
        self.index = match movement {
            Movement::Up => self.index.saturating_sub(1),
            Movement::Down => self.index + 1,
            Movement::PageUp => self.index.saturating_sub(page),
            Movement::PageDown => self.index + page,
            Movement::First => 0,
            Movement::Last => last,
        };
        self.finish(ids);
    }

    /// Tells how many rows fit.
    pub fn set_height(&mut self, height: usize, ids: &[I]) {
        self.locate(ids);
        self.height = height;
        self.finish(ids);
    }

    /// Finds the cursor among the rows after they changed.
    pub fn settle(&mut self, ids: &[I]) {
        self.locate(ids);
        self.finish(ids);
    }

    /// Where the row of the cursor is now; if it is gone, the index stays.
    fn locate(&mut self, ids: &[I]) {
        let found = self
            .cursor
            .as_ref()
            .and_then(|cursor| ids.iter().position(|id| id == cursor));
        if let Some(position) = found {
            self.index = position;
        }
    }

    /// Keeps the index inside the list, takes the id of its row as the cursor and scrolls.
    fn finish(&mut self, ids: &[I]) {
        self.index = self.index.min(ids.len().saturating_sub(1));
        self.cursor = ids.get(self.index).cloned();
        self.follow(ids.len());
    }

    /// Moves the scroll position as little as possible to keep the cursor visible, and not
    /// further than the last row at the bottom.
    fn follow(&mut self, count: usize) {
        if self.height == 0 {
            return;
        }
        if self.index < self.scroll {
            self.scroll = self.index;
        } else if self.index >= self.scroll + self.height {
            self.scroll = self.index + 1 - self.height;
        }
        self.scroll = self.scroll.min(count.saturating_sub(self.height));
    }

    /// The id of the row of the cursor, once the list has been settled and is not empty.
    pub fn cursor(&self) -> Option<&I> {
        self.cursor.as_ref()
    }

    /// The index of the row of the cursor.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The index of the first row that is shown.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// The number of rows that fit; 0 as long as it is not known.
    pub fn height(&self) -> usize {
        self.height
    }
}
