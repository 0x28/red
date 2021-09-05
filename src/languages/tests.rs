use std::sync::{atomic::AtomicBool, Arc};
use std::time::SystemTime;

use crate::Editor;
use crate::Highlight;
use crate::Row;
use crate::SearchDirection;
use crate::RED_QUIT_TIMES;
use crate::RED_STATUS_HEIGHT;

use super::{Syntax, SYNTAX_C, SYNTAX_HASKELL, SYNTAX_RUST};

fn test_editor(syntax: &'static Syntax) -> Editor {
    Editor {
        original_termios: None,
        cursor_x: 0,
        cursor_y: 0,
        render_x: 0,
        screen_rows: 50 - RED_STATUS_HEIGHT,
        screen_cols: 80,
        editor_cols: 80,
        row_offset: 0,
        col_offset: 0,
        rows: vec![],
        file: None,
        status_msg: String::new(),
        status_time: SystemTime::UNIX_EPOCH,
        dirty: false,
        quit_times: RED_QUIT_TIMES,
        search_dir: SearchDirection::Forward,
        last_match: None,
        win_changed: Arc::new(AtomicBool::new(false)),
        stored_hl: None,
        syntax: Some(syntax),
        mark: None,
        clipboard: String::new(),
    }
}

fn hl_to_hldesc(highlights: &[Highlight]) -> String {
    highlights
        .iter()
        .map(|h| match h {
            Highlight::Normal => '_',
            Highlight::Comment => 'c',
            Highlight::MultiLineComment => 'C',
            Highlight::Keyword => 'k',
            Highlight::Type => 't',
            Highlight::Builtin => 'b',
            Highlight::String => 's',
            Highlight::Number => '0',
            Highlight::Match => 'm',
        })
        .collect()
}

fn expect_highlight(editor: &mut Editor, line: &str, highlight: &str) {
    editor.rows = vec![Row {
        index: 0,
        line: line.chars().collect(),
        render: vec![],
        highlights: vec![],
        in_comment: false,
    }];

    editor.update_row(0);
    editor.update_syntax(0);

    assert_eq!(hl_to_hldesc(&editor.rows[0].highlights), highlight)
}

#[test]
fn test_syntax_rust() {
    let mut editor = test_editor(&SYNTAX_RUST);

    expect_highlight(&mut editor, "let x = 100;", "kkk_____000_");
    // TODO dots shouldn't be highlighted as numbers here
    expect_highlight(&mut editor, "for 0..100 {}", "kkk_000000___");
    expect_highlight(&mut editor, "// test", "ccccccc");
    expect_highlight(&mut editor, "let /*x=1*/ x = ()", "kkk_CCCCCCC_______");
    expect_highlight(
        &mut editor,
        "as break const f64 f32 i8 str isize",
        "kk_kkkkk_kkkkk_ttt_ttt_tt_ttt_ttttt",
    )
}

#[test]
fn test_syntax_c() {
    let mut editor = test_editor(&SYNTAX_C);

    expect_highlight(&mut editor, "int main(void) {}", "ttt______tttt____");
    expect_highlight(
        &mut editor,
        r#"char x[] = "hello world";"#,
        r#"tttt_______sssssssssssss_"#,
    );
    expect_highlight(
        &mut editor,
        r#"while (1){ printf("test"); }"#,
        r#"kkkkk__0__________ssssss____"#,
    );
    expect_highlight(
        &mut editor,
        "int x = 100 + 200 * 2.123 / (10 * sizeof(int))",
        "ttt_____000___000___00000____00___kkkkkk_ttt__",
    );
}

#[test]
fn test_syntax_haskell() {
    let mut editor = test_editor(&SYNTAX_HASKELL);

    expect_highlight(
        &mut editor,
        "data Expr = Val Int | App Op Expr Expr",
        "kkkk____________ttt___________________",
    );

    expect_highlight(&mut editor, "let x = (+) 2", "kkk_________0");

    expect_highlight(
        &mut editor,
        "newtype Parser a = P (String -> [(a, String)])",
        "kkkkkkk_______________tttttt_________tttttt___",
    );

    expect_highlight(
        &mut editor,
        "100 * 200 + 300 {- this is a comment -} infix type where -- ...",
        "000___000___000_CCCCCCCCCCCCCCCCCCCCCCC_kkkkk_kkkk_kkkkk_cccccc",
    );
}
