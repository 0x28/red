use crate::Row;
use crate::RED_TAB_STOP;
use crate::{editor_row_cursor_to_render, editor_row_render_to_cursor};

use proptest::prelude::*;

#[test]
fn test_render_to_cursor() {
    let mut row = Row::empty(0);

    row.line = "'a'".chars().collect();
    assert_eq!(editor_row_render_to_cursor(&row, 2), 2);

    row.line = "\t'a'".chars().collect();
    assert_eq!(editor_row_render_to_cursor(&row, RED_TAB_STOP + 2), 3);
}

prop_compose! {
    fn line_and_idx ()
        (s in "[ \ta-zA-ZäöüÄÖÜ:;+-/<>*()]+")
        (index in 0..=s.chars().count(), s in Just(s)) -> (String, usize) {
      (s, index)
    }
}

proptest! {
    #[test]
    fn test_render_cursor_loop((line, cx) in line_and_idx()) {
        let mut row = Row::empty(0);

        row.line = line.chars().collect();
        let rx = editor_row_cursor_to_render(&row, cx);
        prop_assert_eq!(editor_row_render_to_cursor(&row, rx), cx);
    }
}
