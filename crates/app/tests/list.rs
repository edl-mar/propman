//! Specification of the cursor and the scroll position of a list of rows with ids (see
//! `app::ListState`): moving, scrolling, and keeping the cursor on its row when the rows change.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use app::{ListState, Movement};

type List = ListState<u32>;

fn ids(count: u32) -> Vec<u32> {
    (1..=count).collect()
}

/// A list with its cursor on the first row and `height` rows visible (0: not known).
fn list(rows: &[u32], height: usize) -> List {
    let mut list = List::new();
    list.settle(rows);
    list.set_height(height, rows);
    list
}

fn go(list: &mut List, movement: Movement, times: usize, rows: &[u32]) {
    for _ in 0..times {
        list.apply(movement, rows);
    }
}

// ── The start ────────────────────────────────────────────────────────────────

#[test]
fn a_new_list_has_no_cursor_and_starts_at_the_top() {
    let list = List::new();
    assert_eq!(list.cursor(), None);
    assert_eq!(list.index(), 0);
    assert_eq!(list.scroll(), 0);
    assert_eq!(list.height(), 0);
}

#[test]
fn settling_puts_the_cursor_on_the_first_row() {
    let mut list = List::new();
    list.settle(&[10, 20, 30]);
    assert_eq!(list.cursor(), Some(&10));
    assert_eq!(list.index(), 0);
}

#[test]
fn an_empty_list_has_no_cursor() {
    let mut list = list(&ids(3), 0);
    list.settle(&[]);
    assert_eq!(list.cursor(), None);
    assert_eq!(list.index(), 0);
}

#[test]
fn moving_in_an_empty_list_does_nothing() {
    let mut list = List::new();
    for movement in [
        Movement::Up,
        Movement::Down,
        Movement::PageUp,
        Movement::PageDown,
        Movement::First,
        Movement::Last,
    ] {
        list.apply(movement, &[]);
        assert_eq!(list.cursor(), None, "{movement:?}");
        assert_eq!(list.index(), 0, "{movement:?}");
    }
}

// ── Moving ───────────────────────────────────────────────────────────────────

#[test]
fn up_and_down_move_by_one_row_and_stop_at_the_ends() {
    let rows = ids(5);
    let mut list = list(&rows, 0);
    go(&mut list, Movement::Down, 2, &rows);
    assert_eq!((list.index(), list.cursor()), (2, Some(&3)));
    go(&mut list, Movement::Down, 10, &rows);
    assert_eq!((list.index(), list.cursor()), (4, Some(&5)));
    go(&mut list, Movement::Up, 10, &rows);
    assert_eq!((list.index(), list.cursor()), (0, Some(&1)));
}

#[test]
fn first_and_last_go_to_the_ends() {
    let rows = ids(5);
    let mut list = list(&rows, 0);
    list.apply(Movement::Last, &rows);
    assert_eq!((list.index(), list.cursor()), (4, Some(&5)));
    list.apply(Movement::First, &rows);
    assert_eq!((list.index(), list.cursor()), (0, Some(&1)));
}

#[test]
fn a_page_is_as_many_rows_as_fit_or_one_row_while_the_height_is_not_known() {
    let rows = ids(10);
    let mut list = list(&rows, 0);
    list.apply(Movement::PageDown, &rows);
    assert_eq!(list.index(), 1, "the height is not known");

    list.set_height(3, &rows);
    list.apply(Movement::PageDown, &rows);
    assert_eq!(list.index(), 4);
    list.apply(Movement::PageUp, &rows);
    assert_eq!(list.index(), 1);
    go(&mut list, Movement::PageDown, 10, &rows);
    assert_eq!(list.index(), 9, "it stops at the last row");
    go(&mut list, Movement::PageUp, 10, &rows);
    assert_eq!(list.index(), 0, "and at the first");
}

// ── Scrolling ────────────────────────────────────────────────────────────────

#[test]
fn the_scroll_position_moves_as_little_as_possible() {
    let rows = ids(10);
    let mut list = list(&rows, 3);
    go(&mut list, Movement::Down, 2, &rows);
    assert_eq!((list.index(), list.scroll()), (2, 0), "still visible");
    go(&mut list, Movement::Down, 2, &rows);
    assert_eq!(
        (list.index(), list.scroll()),
        (4, 2),
        "the last visible row"
    );
    go(&mut list, Movement::Up, 2, &rows);
    assert_eq!(
        (list.index(), list.scroll()),
        (2, 2),
        "the first visible row"
    );
    list.apply(Movement::Up, &rows);
    assert_eq!((list.index(), list.scroll()), (1, 1), "pulled up by one");
}

#[test]
fn the_scroll_position_stops_at_the_last_row() {
    let rows = ids(5);
    let mut list = list(&rows, 3);
    list.apply(Movement::Last, &rows);
    assert_eq!((list.index(), list.scroll()), (4, 2));
}

#[test]
fn an_unknown_height_does_not_scroll() {
    let rows = ids(10);
    let mut list = list(&rows, 0);
    go(&mut list, Movement::Down, 6, &rows);
    assert_eq!((list.index(), list.scroll()), (6, 0));
}

#[test]
fn telling_a_bigger_height_keeps_the_scroll_position_inside_the_list() {
    let rows = ids(10);
    let mut list = list(&rows, 3);
    list.apply(Movement::Last, &rows);
    assert_eq!(list.scroll(), 7);
    list.set_height(5, &rows);
    assert_eq!((list.index(), list.scroll()), (9, 5));
    assert_eq!(list.height(), 5);
}

// ── The cursor stays on its row ──────────────────────────────────────────────

#[test]
fn the_cursor_stays_on_its_row_when_the_rows_change_their_order() {
    let mut list = list(&ids(4), 0);
    go(&mut list, Movement::Down, 2, &ids(4));
    assert_eq!(list.cursor(), Some(&3));

    list.settle(&[3, 1, 2, 4]);

    assert_eq!((list.index(), list.cursor()), (0, Some(&3)));
}

#[test]
fn moving_starts_from_the_new_place_of_the_row() {
    let mut list = list(&ids(4), 0);
    go(&mut list, Movement::Down, 2, &ids(4));
    // the row of the cursor is now the first; by index, `Down` would go to the last row
    list.apply(Movement::Down, &[3, 1, 2, 4]);
    assert_eq!((list.index(), list.cursor()), (1, Some(&1)));
}

#[test]
fn rows_that_come_before_the_cursor_push_it_down() {
    let mut list = list(&ids(3), 0);
    list.apply(Movement::Down, &ids(3));
    list.settle(&[9, 1, 2, 3]);
    assert_eq!((list.index(), list.cursor()), (2, Some(&2)));
}

#[test]
fn a_row_that_is_gone_leaves_the_cursor_at_its_index() {
    let rows = ids(3);
    let mut list = list(&rows, 0);
    list.apply(Movement::Down, &rows);
    assert_eq!(list.cursor(), Some(&2));

    list.settle(&[1, 3]);
    assert_eq!(
        (list.index(), list.cursor()),
        (1, Some(&3)),
        "the neighbour"
    );

    list.settle(&[1]);
    assert_eq!(
        (list.index(), list.cursor()),
        (0, Some(&1)),
        "kept inside the list"
    );

    list.settle(&[]);
    assert_eq!((list.index(), list.cursor()), (0, None), "nothing left");
}

#[test]
fn settling_keeps_the_cursor_visible() {
    let rows = ids(10);
    let mut list = list(&rows, 3);
    list.apply(Movement::Last, &rows);
    assert_eq!((list.index(), list.scroll()), (9, 7));

    let reversed: Vec<u32> = rows.iter().rev().copied().collect();
    list.settle(&reversed);

    assert_eq!(
        (list.index(), list.cursor(), list.scroll()),
        (0, Some(&10), 0)
    );
}

#[test]
fn the_scroll_position_is_pulled_back_when_there_are_fewer_rows() {
    let rows = ids(10);
    let mut list = list(&rows, 3);
    list.apply(Movement::Last, &rows);
    assert_eq!(list.scroll(), 7);

    list.settle(&[1, 2, 3, 4]);

    assert_eq!(
        (list.index(), list.cursor(), list.scroll()),
        (3, Some(&4), 1)
    );
}
