use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{atomic::AtomicBool, Arc};
use std::time::SystemTime;

use crate::Editor;
use crate::Highlight;
use crate::Row;
use crate::SearchDirection;
use crate::RED_QUIT_TIMES;
use crate::RED_STATUS_HEIGHT;

use super::{
    Syntax, SYNTAX_C, SYNTAX_HASKELL, SYNTAX_PYTHON, SYNTAX_RUST, SYNTAX_SHELL,
};

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

fn expect_highlight_lines(
    editor: &mut Editor,
    lines: &[&str],
    highlights: &[&str],
) {
    assert_eq!(lines.len(), highlights.len());
    editor.rows.clear();

    for ((n, line), highlight) in
        lines.iter().enumerate().zip(highlights.iter())
    {
        editor.rows.push(Row {
            index: n,
            line: line.chars().collect(),
            render: vec![],
            highlights: vec![],
            in_comment: false,
        });

        editor.update_row(n);
        editor.update_syntax(n);

        assert_eq!(hl_to_hldesc(&editor.rows[n].highlights), *highlight)
    }
}

fn expect_highlight_line(editor: &mut Editor, line: &str, highlight: &str) {
    expect_highlight_lines(editor, &[line], &[highlight])
}

#[test]
fn test_syntax_rust() {
    let mut editor = test_editor(&SYNTAX_RUST);

    expect_highlight_line(&mut editor, "let x = 100;", "kkk_____000_");
    // TODO dots shouldn't be highlighted as numbers here
    expect_highlight_line(&mut editor, "for 0..100 {}", "kkk_000000___");
    expect_highlight_line(&mut editor, "// test", "ccccccc");
    expect_highlight_line(
        &mut editor,
        "let /*x=1*/ x = ()",
        "kkk_CCCCCCC_______",
    );
    expect_highlight_line(
        &mut editor,
        "as break const f64 f32 i8 str isize",
        "kk_kkkkk_kkkkk_ttt_ttt_tt_ttt_ttttt",
    );

    expect_highlight_line(
        &mut editor,
        "/*some multi line comment*/100",
        "CCCCCCCCCCCCCCCCCCCCCCCCCCC000",
    );
}

#[test]
fn test_syntax_c() {
    let mut editor = test_editor(&SYNTAX_C);

    expect_highlight_line(
        &mut editor,
        "int main(void) {}",
        "ttt______tttt____",
    );
    expect_highlight_line(
        &mut editor,
        r#"char x[] = "hello world";"#,
        r#"tttt_______sssssssssssss_"#,
    );
    expect_highlight_line(
        &mut editor,
        r#"while (1){ printf("test"); }"#,
        r#"kkkkk__0__________ssssss____"#,
    );
    expect_highlight_line(
        &mut editor,
        "int x = 100 + 200 * 2.123 / (10 * sizeof(int))",
        "ttt_____000___000___00000____00___kkkkkk_ttt__",
    );
}

#[test]
fn test_syntax_haskell() {
    let mut editor = test_editor(&SYNTAX_HASKELL);

    expect_highlight_line(
        &mut editor,
        "data Expr = Val Int | App Op Expr Expr",
        "kkkk____________ttt___________________",
    );

    expect_highlight_line(&mut editor, "let x = (+) 2", "kkk_________0");

    expect_highlight_line(
        &mut editor,
        "newtype Parser a = P (String -> [(a, String)])",
        "kkkkkkk_______________tttttt_________tttttt___",
    );

    expect_highlight_line(
        &mut editor,
        "100 * 200 + 300 {- this is a comment -} infix type where -- ...",
        "000___000___000_CCCCCCCCCCCCCCCCCCCCCCC_kkkkk_kkkk_kkkkk_cccccc",
    );
}

#[test]
fn test_syntax_python() {
    let mut editor = test_editor(&SYNTAX_PYTHON);

    expect_highlight_line(&mut editor, "import math", "kkkkkk_____");

    expect_highlight_line(
        &mut editor,
        "inc = lambda x: x + 1",
        "______kkkkkk________0",
    );

    expect_highlight_line(
        &mut editor,
        "100 + 200 'hello world' # some comment",
        "000___000_sssssssssssss_cccccccccccccc",
    );
}

#[test]
fn test_syntax_shell() {
    let mut editor = test_editor(&SYNTAX_SHELL);

    expect_highlight_line(
        &mut editor,
        "alias x='rm -rf /' # this is bad",
        "bbbbb___ssssssssss_ccccccccccccc",
    );

    expect_highlight_line(
        &mut editor,
        r#"function x() { echo "hello world"; }"#,
        r#"kkkkkkkk_______bbbb_sssssssssssss___"#,
    );

    expect_highlight_line(
        &mut editor,
        "bg if bind exec until eval let false true # fg getopts jobs",
        "bb_kk_bbbb_bbbb_kkkkk_bbbb_bbb_bbbbb_bbbb_ccccccccccccccccc",
    );
}

#[test]
fn test_multiline_comment() {
    let mut editor = test_editor(&SYNTAX_RUST);

    expect_highlight_lines(
        &mut editor,
        &[
            "let x = 100; /*",
            "this is a comment for the line let x = 100;",
            "",
            "",
            "*/ let y = 42;",
        ],
        &[
            "kkk_____000__CC",
            "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
            "",
            "",
            "CC_kkk_____00_",
        ],
    );

    expect_highlight_lines(
        &mut editor,
        &["/*", "123", "this comment has no end", "for while 42", "//"],
        &["CC", "CCC", "CCCCCCCCCCCCCCCCCCCCCCC", "CCCCCCCCCCCC", "CC"],
    );

    expect_highlight_lines(
        &mut editor,
        &["9999", "*/", "let c = 'x';"],
        &["0000", "__", "kkk_____sss_"],
    );
}

#[test]
fn test_backslash_highlighting() {
    let mut editor = test_editor(&SYNTAX_C);

    expect_highlight_line(&mut editor, r#"char c = '\\';"#, "tttt_____ssss_");
    expect_highlight_line(&mut editor, r#"char c = '\t';"#, "tttt_____ssss_");
}

#[test]
fn test_select_syntax() {
    let mut editor = test_editor(&SYNTAX_C);
    editor.syntax = None;

    editor.file = Some(PathBuf::from_str("main.c").unwrap());
    editor.select_syntax_highlight();
    assert_eq!(editor.syntax, Some(&SYNTAX_C));

    editor.file = Some(PathBuf::from_str("prog.rs").unwrap());
    editor.select_syntax_highlight();
    assert_eq!(editor.syntax, Some(&SYNTAX_RUST));

    editor.file = Some(PathBuf::from_str("app.hs").unwrap());
    editor.select_syntax_highlight();
    assert_eq!(editor.syntax, Some(&SYNTAX_HASKELL));

    editor.file = Some(PathBuf::from_str("script.py").unwrap());
    editor.select_syntax_highlight();
    assert_eq!(editor.syntax, Some(&SYNTAX_PYTHON));

    editor.file = Some(PathBuf::from_str("start.sh").unwrap());
    editor.select_syntax_highlight();
    assert_eq!(editor.syntax, Some(&SYNTAX_SHELL));

    editor.file = Some(PathBuf::from_str("test.txt").unwrap());
    editor.select_syntax_highlight();
    assert_eq!(editor.syntax, None);

    editor.file = None;
    editor.select_syntax_highlight();
    assert_eq!(editor.syntax, None);
}
