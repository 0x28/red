use std::io::Read;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::SystemTime;

use crate::Editor;
use crate::EditorKey;
use crate::Row;
use crate::SearchDirection;
use crate::ESC;
use crate::RED_QUIT_TIMES;
use crate::RED_STATUS_HEIGHT;
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

fn dummy_editor(stdin: Box<dyn Read>, stdout: Box<dyn Write>) -> Editor {
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
