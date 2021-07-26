use crate::{RED_TAB_STOP, Row, editor_row_render_to_cursor};

#[test]
fn test_render_to_cursor() {
    let mut row = Row::empty(0);

    row.line = "'a'".chars().collect();
    assert_eq!(editor_row_render_to_cursor(&row, 2), 2);

    row.line = "\t'a'".chars().collect();
    assert_eq!(editor_row_render_to_cursor(&row, RED_TAB_STOP + 2), 3);
}
