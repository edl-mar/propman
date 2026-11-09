//! What `update` asks the outside world to do: things that touch the disk or the terminal.
//!
//! An effect is plain data. `update` returns at most one with the new state, the loop performs
//! it and tells the result as a message. An order of several steps is therefore the chain of
//! messages and effects, not a list: the next step is decided when the result of the last one is
//! known.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {}
