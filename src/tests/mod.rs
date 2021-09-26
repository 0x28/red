use std::error::Error;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::SystemTime;

use tempfile::NamedTempFile;

use crate::languages::SYNTAX_C;
use crate::languages::SYNTAX_HASKELL;
use crate::languages::SYNTAX_RUST;
use crate::Editor;
use crate::EditorKey;
use crate::Row;
use crate::SearchDirection;
use crate::BACKSPACE;
use crate::ESC;
use crate::ESC_SEQ_INVERT_COLORS;
use crate::ESC_SEQ_RESET_ALL;
use crate::RED_QUIT_TIMES;
use crate::RED_STATUS_HEIGHT;
use crate::RED_TAB_STOP;
use crate::{editor_row_cursor_to_render, editor_row_render_to_cursor};

use proptest::prelude::*;

fn test_file(filename: &str) -> String {
    env!("CARGO_MANIFEST_DIR").to_owned() + "/src/tests/" + filename
}

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

fn dummy_editor<'i, 'o>(
    stdin: Box<dyn Read + 'i>,
    stdout: Box<dyn Write + 'o>,
) -> Editor<'i, 'o> {
    Editor {
        original_termios: None,
        cursor_x: 0,
        cursor_y: 0,
        render_x: 0,
        screen_rows: 50 - RED_STATUS_HEIGHT,
        screen_cols: 60,
        editor_cols: 60,
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
        syntax: None,
        mark: None,
        clipboard: String::new(),
        stdin,
        stdout,
    }
}

#[test]
fn test_read_key() {
    let stdin = b"[Ahello world";
    let stdout = vec![];
    let mut editor = dummy_editor(Box::new(&stdin[..]), Box::new(stdout));

    assert_eq!(editor.read_key().unwrap(), EditorKey::ArrowUp);
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('h'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('e'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('l'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('l'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('o'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other(' '));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('w'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('o'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('r'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('l'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('d'));

    let stdin = b"[B[C[D[F[HOHOF";
    editor.stdin = Box::new(&stdin[..]);

    assert_eq!(editor.read_key().unwrap(), EditorKey::ArrowDown);
    assert_eq!(editor.read_key().unwrap(), EditorKey::ArrowRight);
    assert_eq!(editor.read_key().unwrap(), EditorKey::ArrowLeft);
    assert_eq!(editor.read_key().unwrap(), EditorKey::End);
    assert_eq!(editor.read_key().unwrap(), EditorKey::Home);
    assert_eq!(editor.read_key().unwrap(), EditorKey::Home);
    assert_eq!(editor.read_key().unwrap(), EditorKey::End);

    let stdin = b"";
    editor.stdin = Box::new(&stdin[..]);
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other(ESC));

    let stdin = b"f";
    editor.stdin = Box::new(&stdin[..]);
    assert_eq!(editor.read_key().unwrap(), EditorKey::Meta('f'));

    let stdin = b"[1~[7~[3~[4~[8~[5~[6~";
    editor.stdin = Box::new(&stdin[..]);
    assert_eq!(editor.read_key().unwrap(), EditorKey::Home);
    assert_eq!(editor.read_key().unwrap(), EditorKey::Home);
    assert_eq!(editor.read_key().unwrap(), EditorKey::Delete);
    assert_eq!(editor.read_key().unwrap(), EditorKey::End);
    assert_eq!(editor.read_key().unwrap(), EditorKey::End);
    assert_eq!(editor.read_key().unwrap(), EditorKey::PageUp);
    assert_eq!(editor.read_key().unwrap(), EditorKey::PageDown);

    let stdin = "äÄüÜöÖß".as_bytes();
    editor.stdin = Box::new(&stdin[..]);
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('ä'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('Ä'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('ü'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('Ü'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('ö'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('Ö'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Other('ß'));

    let stdin = b"\x01\x02\x03";
    editor.stdin = Box::new(&stdin[..]);
    assert_eq!(editor.read_key().unwrap(), EditorKey::Ctrl('a'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Ctrl('b'));
    assert_eq!(editor.read_key().unwrap(), EditorKey::Ctrl('c'));
}

fn send_test_string(
    editor: &mut Editor,
    s: &str,
) -> Result<(), Box<dyn Error>> {
    for c in s.chars() {
        assert!(editor.process_keypress(EditorKey::Other(c))?);
    }

    Ok(())
}

#[test]
fn test_process_keypress_simple() {
    let stdin = b"";
    let stdout = vec![];
    let mut editor = dummy_editor(Box::new(&stdin[..]), Box::new(stdout));

    send_test_string(&mut editor, "hello").unwrap();

    assert_eq!(editor.rows.len(), 1);
    assert_eq!(editor.rows[0].line.iter().collect::<String>(), "hello");

    editor.process_keypress(EditorKey::Ctrl('m')).unwrap();
    assert_eq!(editor.rows.len(), 2);
    assert_eq!(editor.rows[0].line.iter().collect::<String>(), "hello");

    send_test_string(&mut editor, "world").unwrap();

    assert_eq!(editor.rows.len(), 2);
    assert_eq!(editor.rows[0].line.iter().collect::<String>(), "hello");
    assert_eq!(editor.rows[1].line.iter().collect::<String>(), "world");
    assert_eq!(editor.cursor_x, 5);
    assert_eq!(editor.cursor_y, 1);

    editor.process_keypress(EditorKey::Home).unwrap();
    assert_eq!(editor.cursor_x, 0);
    assert_eq!(editor.cursor_y, 1);

    send_test_string(&mut editor, "--->").unwrap();

    assert_eq!(editor.rows[1].line.iter().collect::<String>(), "--->world");
}

#[test]
fn test_deletion() {
    let stdin = b"";
    let stdout = vec![];
    let mut editor = dummy_editor(Box::new(&stdin[..]), Box::new(stdout));

    send_test_string(&mut editor, "hello").unwrap();

    editor.process_keypress(EditorKey::Ctrl('m')).unwrap();

    assert_eq!(editor.rows[0].line.iter().collect::<String>(), "hello");
    assert_eq!(editor.rows[1].line.iter().collect::<String>(), "");

    editor
        .process_keypress(EditorKey::Other(BACKSPACE))
        .unwrap();
    editor
        .process_keypress(EditorKey::Other(BACKSPACE))
        .unwrap();
    editor
        .process_keypress(EditorKey::Other(BACKSPACE))
        .unwrap();

    assert_eq!(editor.rows.len(), 1);
    assert_eq!(editor.rows[0].line.iter().collect::<String>(), "hel");

    editor.process_keypress(EditorKey::ArrowLeft).unwrap();
    editor.process_keypress(EditorKey::ArrowLeft).unwrap();
    editor.process_keypress(EditorKey::ArrowLeft).unwrap();

    assert_eq!(editor.cursor_x, 0);
    assert_eq!(editor.cursor_y, 0);

    editor.process_keypress(EditorKey::Delete).unwrap();
    editor.process_keypress(EditorKey::Delete).unwrap();

    assert_eq!(editor.rows.len(), 1);
    assert_eq!(editor.rows[0].line.iter().collect::<String>(), "l");

    editor.process_keypress(EditorKey::Delete).unwrap();
    editor.process_keypress(EditorKey::Delete).unwrap();
    editor.process_keypress(EditorKey::Delete).unwrap();
    editor.process_keypress(EditorKey::Delete).unwrap();
    editor.process_keypress(EditorKey::Delete).unwrap();
    editor.process_keypress(EditorKey::Delete).unwrap();

    assert_eq!(editor.rows.len(), 1);
    assert_eq!(editor.rows[0].line.iter().collect::<String>(), "");
}

#[test]
fn test_copy_paste() {
    let stdin = b"";
    let stdout = vec![];
    let mut editor = dummy_editor(Box::new(&stdin[..]), Box::new(stdout));

    send_test_string(&mut editor, "this is a test").unwrap();

    editor.process_keypress(EditorKey::Home).unwrap();

    assert_eq!(editor.cursor_x, 0);
    assert_eq!(editor.cursor_y, 0);

    editor.process_keypress(EditorKey::Ctrl(' ')).unwrap();
    assert_eq!(editor.mark, Some((0, 0)));

    editor.process_keypress(EditorKey::ArrowRight).unwrap();
    editor.process_keypress(EditorKey::ArrowRight).unwrap();
    editor.process_keypress(EditorKey::ArrowRight).unwrap();
    editor.process_keypress(EditorKey::ArrowRight).unwrap();
    editor.process_keypress(EditorKey::Ctrl('c')).unwrap();
    assert_eq!(editor.clipboard, "this");

    editor.process_keypress(EditorKey::End).unwrap();
    assert_eq!(editor.cursor_x, 14);
    assert_eq!(editor.cursor_y, 0);

    editor.process_keypress(EditorKey::Ctrl('v')).unwrap();
    editor.process_keypress(EditorKey::Ctrl('v')).unwrap();
    editor.process_keypress(EditorKey::Ctrl('v')).unwrap();

    assert_eq!(editor.rows.len(), 1);
    assert_eq!(
        editor.rows[0].line.iter().collect::<String>(),
        "this is a testthisthisthis"
    );
}

#[test]
fn test_draw_status_bar() {
    let stdin = vec![];
    let stdout = vec![];
    let mut status_bar = vec![];
    let mut editor = dummy_editor(Box::new(&stdin[..]), Box::new(stdout));

    send_test_string(&mut editor, "abc").unwrap();
    editor.process_keypress(EditorKey::Ctrl('m')).unwrap();
    send_test_string(&mut editor, "def").unwrap();
    editor.process_keypress(EditorKey::Ctrl('m')).unwrap();
    send_test_string(&mut editor, "ghi").unwrap();
    editor.process_keypress(EditorKey::Ctrl('m')).unwrap();

    let tests = [
        (
            None,
            None,
            "[No Name] - 4 lines (modified)                   no ft | 3/4",
        ),
        (
            Some(&SYNTAX_HASKELL),
            Some(PathBuf::from("main.hs")),
            "main.hs - 4 lines                              haskell | 2/4",
        ),
        (
            Some(&SYNTAX_C),
            Some(PathBuf::from("test.c")),
            "test.c - 4 lines (modified)                          c | 1/4",
        ),
    ];

    editor.dirty = false;

    for (syntax, file, expected) in tests {
        editor.syntax = syntax;
        editor.file = file;

        editor.process_keypress(EditorKey::ArrowUp).unwrap();
        editor.dirty = !editor.dirty;

        editor.draw_status_bar(&mut status_bar).unwrap();
        assert!(status_bar.starts_with(ESC_SEQ_INVERT_COLORS));
        let mut suffix = ESC_SEQ_RESET_ALL.to_vec();
        suffix.extend_from_slice(b"\r\n");
        assert!(status_bar.ends_with(&suffix));

        let status_bar_str = status_bar
            [ESC_SEQ_INVERT_COLORS.len()..status_bar.len() - suffix.len()]
            .iter()
            .map(|b| *b as char)
            .collect::<String>();

        assert_eq!(status_bar_str, expected);
        assert_eq!(status_bar_str.len(), editor.screen_cols);

        status_bar.clear();
    }
}

#[test]
fn test_find() {
    let stdin = b"";
    let stdout = vec![];
    let mut editor = dummy_editor(Box::new(&stdin[..]), Box::new(stdout));

    send_test_string(&mut editor, "text @ line 1").unwrap();
    editor.process_keypress(EditorKey::Ctrl('m')).unwrap();
    send_test_string(&mut editor, "more text @ line 2").unwrap();
    editor.process_keypress(EditorKey::Ctrl('m')).unwrap();
    send_test_string(&mut editor, "find this @ line 3").unwrap();
    editor.process_keypress(EditorKey::Ctrl('m')).unwrap();
    send_test_string(&mut editor, "or this @ line 4").unwrap();
    editor.process_keypress(EditorKey::Ctrl('m')).unwrap();

    editor.process_keypress(EditorKey::Home).unwrap();
    editor.process_keypress(EditorKey::ArrowUp).unwrap();
    editor.process_keypress(EditorKey::ArrowUp).unwrap();
    editor.process_keypress(EditorKey::ArrowUp).unwrap();
    editor.process_keypress(EditorKey::ArrowUp).unwrap();
    editor.process_keypress(EditorKey::ArrowUp).unwrap();

    assert_eq!(editor.cursor_x, 0);
    assert_eq!(editor.cursor_y, 0);

    let stdin = b"line\x06\x0d"; // "line", ctrl-f, enter
    editor.stdin = Box::new(&stdin[..]);
    editor.process_keypress(EditorKey::Ctrl('f')).unwrap();

    assert_eq!(editor.cursor_x, 12);
    assert_eq!(editor.cursor_y, 1);
    assert_eq!(
        editor.rows[editor.cursor_y].line.iter().collect::<String>(),
        "more text @ line 2"
    );

    let stdin = b"text[A\x0d"; // "text", up arrow, enter
    editor.stdin = Box::new(&stdin[..]);
    editor.process_keypress(EditorKey::Ctrl('f')).unwrap();

    assert_eq!(editor.cursor_x, 5);
    assert_eq!(editor.cursor_y, 1);

    let stdin = b"4\x06"; // "4", ctrl-f, escape
    editor.stdin = Box::new(&stdin[..]);
    editor.process_keypress(EditorKey::Ctrl('f')).unwrap();

    assert_eq!(editor.cursor_x, 5);
    assert_eq!(editor.cursor_y, 1);
}

#[test]
fn test_open_file() {
    let mut editor = dummy_editor(Box::new(&b""[..]), Box::new(vec![]));
    let file = test_file("nonexistent.txt");
    assert!(editor.open(&PathBuf::from(file)).is_ok());
    assert!(editor.rows.is_empty());
    assert_eq!(editor.syntax, None);

    let mut editor = dummy_editor(Box::new(&b""[..]), Box::new(vec![]));
    let file = test_file("simple.txt");
    assert!(editor.open(&PathBuf::from(file)).is_ok());
    assert_eq!(editor.rows.len(), 3);
    assert_eq!(editor.syntax, None);

    assert_eq!(editor.rows[0].line.iter().collect::<String>(), "ABC");
    assert_eq!(editor.rows[1].line.iter().collect::<String>(), "DEF");
    assert_eq!(editor.rows[2].line.iter().collect::<String>(), "GHI");

    let mut editor = dummy_editor(Box::new(&b""[..]), Box::new(vec![]));
    let file = test_file("rust_sample.rs");
    assert!(editor.open(&PathBuf::from(file)).is_ok());
    assert_eq!(editor.rows.len(), 3);
    assert_eq!(editor.syntax, Some(&SYNTAX_RUST));

    assert_eq!(
        editor.rows[0].line.iter().collect::<String>(),
        "fn main() {"
    );
    assert_eq!(
        editor.rows[1].line.iter().collect::<String>(),
        "    println!(\"hello world\");"
    );
    assert_eq!(editor.rows[2].line.iter().collect::<String>(), "}");
}

#[test]
fn test_save_file() {
    let file = NamedTempFile::new().unwrap();
    let file_path = file.into_temp_path();
    let mut write_editor = dummy_editor(Box::new(&b""[..]), Box::new(vec![]));

    write_editor.open(&file_path).unwrap();
    send_test_string(&mut write_editor, "this is a test").unwrap();
    assert_eq!(write_editor.dirty, true);
    write_editor.save().unwrap();
    assert_eq!(write_editor.dirty, false);

    let mut read_editor = dummy_editor(Box::new(&b""[..]), Box::new(vec![]));
    read_editor.open(&file_path).unwrap();
    assert_eq!(read_editor.rows.len(), 1);

    assert_eq!(
        read_editor.rows[0].line.iter().collect::<String>(),
        "this is a test"
    );
}
