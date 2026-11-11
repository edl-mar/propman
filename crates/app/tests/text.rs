//! Specification of the line of text that is edited (see `app::TextField`): inserting and
//! removing at the cursor, moving it, and counting in characters.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use app::{TextEdit, TextField};

/// A field that has been given these edits one after the other.
fn edited(edits: &[TextEdit]) -> TextField {
    let mut field = TextField::new();
    for &edit in edits {
        field.apply(edit);
    }
    field
}

fn typed(text: &str) -> Vec<TextEdit> {
    text.chars().map(TextEdit::Insert).collect()
}

#[test]
fn a_new_field_is_empty_with_the_cursor_at_the_start() {
    let field = TextField::new();
    assert_eq!(field.text(), "");
    assert_eq!(field.cursor(), 0);
}

#[test]
fn a_field_with_text_has_the_cursor_behind_it() {
    let field = TextField::with_text("n\u{e4}me");
    assert_eq!(field.text(), "n\u{e4}me");
    assert_eq!(field.cursor(), 4, "four characters, not five bytes");
}

#[test]
fn typing_adds_characters_in_front_of_the_cursor() {
    let field = edited(&typed("edit"));
    assert_eq!((field.text(), field.cursor()), ("edit", 4));
}

#[test]
fn a_character_is_put_in_the_middle_where_the_cursor_is() {
    let mut edits = typed("ei");
    edits.extend([TextEdit::Left, TextEdit::Insert('d')]);
    let field = edited(&edits);
    assert_eq!((field.text(), field.cursor()), ("edi", 2));
}

#[test]
fn the_cursor_counts_characters_and_not_bytes() {
    // an umlaut, the euro sign and a character outside the first plane
    let field = edited(&typed("\u{e4}\u{20ac}\u{1f600}"));
    assert_eq!(field.text(), "\u{e4}\u{20ac}\u{1f600}");
    assert_eq!(field.cursor(), 3);

    let mut edits = typed("\u{e4}\u{20ac}\u{1f600}");
    edits.extend([TextEdit::Left, TextEdit::Left, TextEdit::Insert('x')]);
    let field = edited(&edits);
    assert_eq!(
        (field.text(), field.cursor()),
        ("\u{e4}x\u{20ac}\u{1f600}", 2)
    );

    let mut edits = typed("\u{e4}\u{20ac}\u{1f600}");
    edits.push(TextEdit::Backspace);
    let field = edited(&edits);
    assert_eq!((field.text(), field.cursor()), ("\u{e4}\u{20ac}", 2));
}

#[test]
fn characters_that_cannot_be_part_of_a_line_are_not_inserted() {
    let mut edits = typed("a");
    edits.extend([
        TextEdit::Insert('\n'),
        TextEdit::Insert('\r'),
        TextEdit::Insert('\t'),
        TextEdit::Insert('\u{1}'),
        TextEdit::Insert('\u{7f}'),
    ]);
    edits.extend(typed("b"));
    let field = edited(&edits);
    assert_eq!((field.text(), field.cursor()), ("ab", 2));
}

#[test]
fn backspace_removes_the_character_in_front_of_the_cursor() {
    let mut edits = typed("edit");
    edits.push(TextEdit::Backspace);
    let field = edited(&edits);
    assert_eq!((field.text(), field.cursor()), ("edi", 3));

    let mut edits = typed("edit");
    edits.extend([TextEdit::Left, TextEdit::Backspace]);
    let field = edited(&edits);
    assert_eq!((field.text(), field.cursor()), ("edt", 2));
}

#[test]
fn backspace_at_the_start_does_nothing() {
    let mut edits = typed("ab");
    edits.extend([TextEdit::Home, TextEdit::Backspace]);
    let field = edited(&edits);
    assert_eq!((field.text(), field.cursor()), ("ab", 0));
    assert_eq!(edited(&[TextEdit::Backspace]).text(), "");
}

#[test]
fn delete_removes_the_character_at_the_cursor() {
    let mut edits = typed("edit");
    edits.extend([TextEdit::Home, TextEdit::Delete]);
    let field = edited(&edits);
    assert_eq!((field.text(), field.cursor()), ("dit", 0));
}

#[test]
fn delete_at_the_end_does_nothing() {
    let mut edits = typed("ab");
    edits.push(TextEdit::Delete);
    let field = edited(&edits);
    assert_eq!((field.text(), field.cursor()), ("ab", 2));
}

#[test]
fn left_and_right_move_the_cursor_and_stop_at_the_ends() {
    let mut edits = typed("abc");
    edits.extend([TextEdit::Left; 5]);
    assert_eq!(edited(&edits).cursor(), 0);

    edits.extend([TextEdit::Right; 2]);
    assert_eq!(edited(&edits).cursor(), 2);

    edits.extend([TextEdit::Right; 5]);
    assert_eq!(edited(&edits).cursor(), 3);
}

#[test]
fn home_and_end_go_to_the_ends() {
    let mut edits = typed("abc");
    edits.push(TextEdit::Home);
    assert_eq!(edited(&edits).cursor(), 0);
    edits.push(TextEdit::End);
    assert_eq!(edited(&edits).cursor(), 3);
}

#[test]
fn clear_removes_everything() {
    let mut edits = typed("abc");
    edits.push(TextEdit::Clear);
    let field = edited(&edits);
    assert_eq!((field.text(), field.cursor()), ("", 0));
}

#[test]
fn a_field_with_text_can_be_edited_from_its_end() {
    let mut field = TextField::with_text("ab");
    field.apply(TextEdit::Insert('c'));
    assert_eq!(field.text(), "abc");
}
