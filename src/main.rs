use libc::STDIN_FILENO;
use std::cmp::Ordering;
use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{atomic, atomic::AtomicBool, Arc};
use std::time::SystemTime;
use termios::{
    Termios, BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP,
    IXON, OPOST, TCSAFLUSH, VMIN, VTIME,
};

mod languages;
mod red_error;
mod red_ioctl;
use languages::Syntax;
use languages::{
    HIGHLIGHT_CHARS, HIGHLIGHT_NUMBERS, HIGHLIGHT_STRINGS, SYNTAXES,
};
use red_error::EditorError;
use red_ioctl::get_window_size_ioctl;

type Position = (usize, usize);

const ESC: char = '\x1b';
const BACKSPACE: char = '\x7f';

const ESC_SEQ_RESET_CURSOR: &[u8] = b"\x1b[H";
const ESC_SEQ_CLEAR_SCREEN: &[u8] = b"\x1b[2J";
const ESC_SEQ_BOTTOM_RIGHT: &[u8] = b"\x1b[999C\x1b[999B";
const ESC_SEQ_QUERY_CURSOR: &[u8] = b"\x1b[6n";
const ESC_SEQ_HIDE_CURSOR: &[u8] = b"\x1b[?25l";
const ESC_SEQ_SHOW_CURSOR: &[u8] = b"\x1b[?25h";
const ESC_SEQ_CLEAR_LINE: &[u8] = b"\x1b[K";
const ESC_SEQ_INVERT_COLORS: &[u8] = b"\x1b[7m";
const ESC_SEQ_RESET_ALL: &[u8] = b"\x1b[m";
const ESC_SEQ_COLOR_RED: &[u8] = b"\x1b[31m";
const ESC_SEQ_COLOR_GREEN: &[u8] = b"\x1b[32m";
const ESC_SEQ_COLOR_YELLOW: &[u8] = b"\x1b[33m";
const ESC_SEQ_COLOR_BLUE: &[u8] = b"\x1b[34m";
const ESC_SEQ_COLOR_MAGENTA: &[u8] = b"\x1b[35m";
const ESC_SEQ_COLOR_CYAN: &[u8] = b"\x1b[36m";
// const ESC_SEQ_COLOR_WHITE: &[u8] = b"\x1b[37m";
const ESC_SEQ_COLOR_DEFAULT: &[u8] = b"\x1b[39m";
const ESC_SEQ_COLOR_DEFAULT_BG: &[u8] = b"\x1b[49m";
const ESC_SEQ_COLOR_BRIGHT_CYAN: &[u8] = b"\x1b[96m";
const ESC_SEQ_COLOR_GRAY_BG: &[u8] = b"\x1b[100m";

fn esc_seq_move_cursor(pos_y: usize, pos_x: usize) -> Vec<u8> {
    format!("\x1b[{};{}H", pos_y, pos_x).into_bytes()
}

const RED_VERSION: &str = env!("CARGO_PKG_VERSION");
const RED_TAB_STOP: usize = 8;
const RED_QUIT_TIMES: u8 = 3;
const RED_STATUS_HEIGHT: usize = 2;
const RED_LINE_SEP: &str = "│ ";

macro_rules! set_status_message {
    ($editor: expr, $($arg:tt)*) => {
        $editor.set_status_message(format!($($arg)*));
    };
}

#[derive(PartialEq)]
enum EditorKey {
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Delete,
    PageUp,
    PageDown,
    Home,
    End,
    Ctrl(char),
    Meta(char),
    Other(char),
}

enum SearchDirection {
    Forward,
    Backward,
}

impl SearchDirection {
    fn step(&self, value: usize, limit: usize) -> usize {
        match self {
            SearchDirection::Forward => {
                let next = value.wrapping_add(1);
                if next > limit {
                    0
                } else {
                    next
                }
            }
            SearchDirection::Backward => {
                let prev = value.wrapping_sub(1);
                if prev > limit {
                    limit
                } else {
                    prev
                }
            }
        }
    }
}

struct Row {
    index: usize,
    line: Vec<char>,
    render: Vec<char>,
    highlights: Vec<Highlight>,
    in_comment: bool,
}

#[derive(Clone, PartialEq, Debug)]
enum Highlight {
    Normal,
    Comment,
    MultiLineComment,
    Keyword,
    Type,
    Builtin,
    String,
    Number,
    Match,
}

impl Highlight {
    fn color(&self) -> &[u8] {
        #[allow(unreachable_patterns)]
        match self {
            Highlight::Normal => ESC_SEQ_COLOR_DEFAULT,
            Highlight::String => ESC_SEQ_COLOR_MAGENTA,
            Highlight::Number => ESC_SEQ_COLOR_RED,
            Highlight::Match => ESC_SEQ_COLOR_BLUE,
            Highlight::Comment => ESC_SEQ_COLOR_CYAN,
            Highlight::MultiLineComment => ESC_SEQ_COLOR_CYAN,
            Highlight::Keyword => ESC_SEQ_COLOR_YELLOW,
            Highlight::Type => ESC_SEQ_COLOR_GREEN,
            Highlight::Builtin => ESC_SEQ_COLOR_BRIGHT_CYAN,
        }
    }
}

impl Row {
    fn empty(at: usize) -> Row {
        Row {
            index: at,
            line: vec![],
            render: vec![],
            highlights: vec![],
            in_comment: false,
        }
    }
}

struct Editor {
    original_termios: Termios,
    cursor_x: usize,
    cursor_y: usize,
    render_x: usize,
    screen_rows: usize,
    screen_cols: usize,
    editor_cols: usize,
    row_offset: usize,
    col_offset: usize,
    rows: Vec<Row>,
    file: Option<PathBuf>,
    status_msg: String,
    status_time: SystemTime,
    dirty: bool,
    quit_times: u8,
    search_dir: SearchDirection,
    last_match: Option<usize>,
    win_changed: Arc<AtomicBool>,
    stored_hl: Option<(usize, Vec<Highlight>)>,
    syntax: Option<&'static Syntax>,
    mark: Option<Position>,
    clipboard: String,
}

impl Editor {
    fn new() -> Result<Editor, Box<dyn Error>> {
        let original_termios = Termios::from_fd(STDIN_FILENO)?;
        enable_raw_mode()?;
        let (rows, cols) = get_window_size()?;

        let win_changed = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(
            signal_hook::consts::SIGWINCH,
            Arc::clone(&win_changed),
        )?;

        Ok(Editor {
            original_termios,
            cursor_x: 0,
            cursor_y: 0,
            render_x: 0,
            screen_rows: rows - RED_STATUS_HEIGHT,
            screen_cols: cols,
            editor_cols: cols,
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
            win_changed,
            stored_hl: None,
            syntax: None,
            mark: None,
            clipboard: String::new(),
        })
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        // NOTE: Don't panic while dropping!
        if let Err(e) =
            termios::tcsetattr(STDIN_FILENO, TCSAFLUSH, &self.original_termios)
        {
            eprintln!("tcsetattr error: {}", e)
        }
    }
}

fn get_cursor_position() -> Result<(usize, usize), Box<dyn Error>> {
    let mut stdout = io::stdout();
    let mut stdin = io::stdin();
    stdout.write_all(ESC_SEQ_QUERY_CURSOR)?;
    stdout.flush()?;

    let mut c = [0; 1];
    let mut response = String::new();

    loop {
        stdin.read_exact(&mut c)?;
        if c[0] == b'R' {
            break;
        } else {
            response.push(c[0] as char);
        }
    }

    if !response.starts_with("\x1b[") || response.len() <= 2 {
        return Err(Box::new(EditorError::ParseGetCursorResponse));
    }

    let pos: Result<Vec<usize>, _> =
        response[2..].split(';').map(str::parse::<usize>).collect();

    match pos?.as_slice() {
        [row, col] => Ok((*row, *col)),
        _ => Err(Box::new(EditorError::ParseGetCursorResponse)),
    }
}

fn get_window_size() -> Result<(usize, usize), Box<dyn Error>> {
    if let Ok(size) = get_window_size_ioctl() {
        return Ok(size);
    }

    let mut stdout = io::stdout();

    stdout.write_all(ESC_SEQ_BOTTOM_RIGHT)?;
    stdout.flush()?;

    get_cursor_position()
}

fn is_separator(c: char) -> bool {
    c.is_whitespace() || c == '\0' || ",.()+-/*=~%<>[];".contains(c)
}

impl Editor {
    fn update_syntax(&mut self, row_idx: usize) {
        let mut in_comment = row_idx > 0 && self.rows[row_idx - 1].in_comment;
        let num_rows = self.rows.len();
        let row = &mut self.rows[row_idx];

        row.highlights.resize(row.render.len(), Highlight::Normal);
        row.highlights.fill(Highlight::Normal);

        let syntax = match self.syntax {
            Some(s) => s,
            None => return,
        };

        let mut prev_sep = true;
        let mut in_string = None;

        let single_line_comment =
            syntax.single_line_comment.chars().collect::<Vec<_>>();
        let multi_line_comment = (
            syntax.multi_line_comment.0.chars().collect::<Vec<_>>(),
            syntax.multi_line_comment.1.chars().collect::<Vec<_>>(),
        );

        let mut iter = row.render.iter().enumerate();

        while let Some((idx, &c)) = iter.next() {
            let prev_hl = row
                .highlights
                .get(idx.wrapping_sub(1))
                .unwrap_or(&Highlight::Normal)
                .clone();

            if in_string.is_none()
                && !in_comment
                && !single_line_comment.is_empty()
                && row.render[idx..].starts_with(&single_line_comment)
            {
                row.highlights[idx..].fill(Highlight::Comment);
                break;
            }

            if !multi_line_comment.0.is_empty()
                && !multi_line_comment.1.is_empty()
                && in_string.is_none()
            {
                if in_comment {
                    row.highlights[idx] = Highlight::MultiLineComment;
                    if row.render[idx..].starts_with(&multi_line_comment.1) {
                        row.highlights[idx..idx + multi_line_comment.1.len()]
                            .fill(Highlight::MultiLineComment);

                        for _ in 0..multi_line_comment.1.len() - 1 {
                            iter.next();
                        }

                        in_comment = false;
                        prev_sep = true;
                    }
                    continue;
                } else if row.render[idx..].starts_with(&multi_line_comment.0) {
                    row.highlights[idx..idx + multi_line_comment.0.len()]
                        .fill(Highlight::MultiLineComment);

                    for _ in 0..multi_line_comment.0.len() - 1 {
                        iter.next();
                    }

                    in_comment = true;
                    continue;
                }
            }

            if syntax.flags & HIGHLIGHT_CHARS != 0 && c == '\'' {
                let line_idx = editor_row_render_to_cursor(row, idx);
                if line_idx >= 2 && row.line[line_idx - 2] == '\'' {
                    row.highlights[idx - 2..=idx].fill(Highlight::String);
                    continue;
                }
                if line_idx >= 3
                    && row.line[line_idx - 3] == '\''
                    && row.line[line_idx - 2] == '\\'
                {
                    row.highlights[idx - 3..=idx].fill(Highlight::String);
                    continue;
                }
            }

            if syntax.flags & HIGHLIGHT_STRINGS != 0 {
                if let Some(delimit) = in_string {
                    row.highlights[idx] = Highlight::String;
                    if c == '\\' {
                        if let Some((i, _)) = iter.next() {
                            row.highlights[i] = Highlight::String;
                            continue;
                        }
                    } else if c == delimit {
                        in_string = None;
                    }
                    prev_sep = true;
                    continue;
                } else if syntax.string_delimiter.contains(c) {
                    in_string = Some(c);
                    row.highlights[idx] = Highlight::String;
                    continue;
                }
            }

            if syntax.flags & HIGHLIGHT_NUMBERS != 0
                && (c.is_digit(10)
                    && (prev_sep || prev_hl == Highlight::Number)
                    || (c == '.' && prev_hl == Highlight::Number))
            {
                row.highlights[idx] = Highlight::Number;
                prev_sep = false;
                continue;
            }

            if prev_sep {
                let mut found_symbol = false;

                for (hl, list) in [
                    (Highlight::Keyword, syntax.keywords),
                    (Highlight::Type, syntax.types),
                    (Highlight::Builtin, syntax.builtins),
                ] {
                    for symbol in list {
                        let symbol = symbol.chars().collect::<Vec<_>>();
                        if row.render[idx..].starts_with(&symbol)
                            && is_separator(
                                *row.render
                                    .get(idx + symbol.len())
                                    .unwrap_or(&'\0'),
                            )
                        {
                            row.highlights[idx..idx + symbol.len()].fill(hl);

                            for _ in 0..symbol.len() - 1 {
                                iter.next();
                            }

                            found_symbol = true;
                            break;
                        }
                    }
                }

                if found_symbol {
                    prev_sep = false;
                    continue;
                }
            }

            prev_sep = is_separator(c);
        }

        let in_comment_changed = row.in_comment != in_comment;
        row.in_comment = in_comment;
        if in_comment_changed && row.index + 1 < num_rows {
            let idx = row.index;
            self.update_syntax(idx + 1);
        }
    }

    fn select_syntax_highlight(&mut self) {
        self.syntax = None;
        let file = match &self.file {
            Some(f) => f,
            None => return,
        };

        let file_ext = file.extension().map(OsStr::to_str).flatten();

        self.syntax = SYNTAXES.iter().find(|syntax| {
            syntax.extensions.iter().any(|ext| {
                let is_ext = ext.starts_with('.');
                is_ext && Some(&ext[1..]) == file_ext
                    || !is_ext && file.to_string_lossy().contains(ext)
            })
        });

        if self.syntax.is_some() {
            for row in 0..self.rows.len() {
                self.update_syntax(row);
            }
        }
    }
}

fn editor_row_cursor_to_render(row: &Row, cursor_x: usize) -> usize {
    let mut render_x = 0;

    for &c in row.line.iter().take(cursor_x) {
        if c == '\t' {
            render_x += (RED_TAB_STOP - 1) - (render_x % RED_TAB_STOP);
        }
        render_x += 1;
    }

    render_x
}

fn editor_row_render_to_cursor(row: &Row, render_x: usize) -> usize {
    let mut current_render_x = 0;

    for (cursor_x, &c) in row.line.iter().enumerate() {
        if c == '\t' {
            current_render_x +=
                (RED_TAB_STOP - 1) - (current_render_x % RED_TAB_STOP);
        }
        current_render_x += 1;

        if current_render_x > render_x {
            return cursor_x;
        }
    }

    row.line.len()
}

impl Editor {
    fn row_append(&mut self, row: usize, content: &[char]) {
        self.rows[row].line.extend_from_slice(content);
        self.update_row(row);
    }

    fn update_row(&mut self, row_idx: usize) {
        let row = &mut self.rows[row_idx];

        row.render.clear();
        let mut idx = 0;
        for &c in row.line.iter() {
            if c == '\t' {
                row.render.push(' ');
                idx += 1;
                while idx % RED_TAB_STOP != 0 {
                    row.render.push(' ');
                    idx += 1;
                }
            } else {
                row.render.push(c);
                idx += 1;
            }
        }

        self.update_syntax(row_idx);
    }

    fn delete_row(&mut self, at: usize) {
        if at < self.rows.len() {
            self.rows.remove(at);
            self.mark_dirty();
        }

        for idx in at..self.rows.len() {
            self.rows[idx].index = idx;
        }
    }

    fn mark_dirty(&mut self) {
        self.mark = None;
        self.dirty = true;
    }

    fn row_insert_char(&mut self, row_idx: usize, mut at: usize, c: char) {
        let row = &mut self.rows[row_idx];
        if at > row.line.len() {
            at = row.line.len();
        }

        row.line.insert(at, c);
        self.update_row(row_idx);
    }

    fn row_delete_char(&mut self, row_idx: usize, at: usize) {
        let row = &mut self.rows[row_idx];
        if at < row.line.len() {
            row.line.remove(at);
            self.update_row(row_idx);
        }
    }

    fn insert_char(&mut self, c: char) {
        if self.cursor_y == self.rows.len() {
            self.rows.push(Row::empty(self.cursor_y))
        }

        self.row_insert_char(self.cursor_y, self.cursor_x, c);

        self.cursor_x += 1;
        self.mark_dirty();
    }

    fn insert_newline(&mut self) {
        if self.cursor_x == 0 {
            self.rows.insert(self.cursor_y, Row::empty(self.cursor_y));
        } else if let Some(current_row) = self.rows.get_mut(self.cursor_y) {
            let next_line = current_row.line[self.cursor_x..].to_vec();
            let next_row = Row {
                index: self.cursor_y + 1,
                line: next_line,
                render: vec![],
                highlights: vec![],
                in_comment: current_row.in_comment,
            };
            current_row.line.truncate(self.cursor_x);
            self.rows.insert(self.cursor_y + 1, next_row);
            self.update_row(self.cursor_y);
            self.update_row(self.cursor_y + 1);
        }

        for idx in self.cursor_y + 1..self.rows.len() {
            self.rows[idx].index = idx;
        }

        self.mark_dirty();
        self.cursor_y += 1;
        self.cursor_x = 0;
    }

    fn delete_char(&mut self) {
        if self.cursor_x == 0 && self.cursor_y == 0 {
            return;
        }

        if let Some(row) = self.rows.get_mut(self.cursor_y) {
            if self.cursor_x > 0 {
                self.row_delete_char(self.cursor_y, self.cursor_x - 1);
                self.cursor_x -= 1;
                self.mark_dirty();
            } else {
                let line = std::mem::take(&mut row.line);
                let prev_row = &mut self.rows[self.cursor_y - 1];
                self.cursor_x = prev_row.line.len();
                self.row_append(self.cursor_y - 1, &line);
                self.delete_row(self.cursor_y);
                self.cursor_y -= 1;
            }
        } else if self.cursor_y == self.rows.len() {
            // NOTE: we are in the last empty line -> nothing to delete
            self.cursor_y -= 1;
            self.cursor_x = self.rows[self.cursor_y].line.len();
        }
    }

    fn write_rows(
        &self,
        output: &mut impl Write,
    ) -> Result<usize, Box<dyn Error>> {
        let mut bytes = 0;
        for row in &self.rows {
            for c in &row.line {
                bytes += output.write(format!("{}", c).as_bytes())?;
            }
            bytes += output.write(b"\n")?;
        }

        Ok(bytes)
    }

    fn save(&mut self) -> Result<(), Box<dyn Error>> {
        if self.file.is_none() {
            match self.prompt("Save as (ESC to cancel)", None)? {
                Some(file) => self.file = Some(PathBuf::from(file)),
                None => {
                    set_status_message!(self, "Save aborted");
                    return Ok(());
                }
            }
        }
        if self.syntax.is_none() {
            self.select_syntax_highlight();
        }

        self.dirty = false;
        let mut write_to_file = || -> Result<(), Box<dyn Error>> {
            match &self.file {
                Some(path) => {
                    let mut file = BufWriter::new(File::create(path)?);
                    let bytes_written = self.write_rows(&mut file)?;
                    set_status_message!(
                        self,
                        "{} bytes written to disk",
                        bytes_written
                    );

                    Ok(())
                }
                None => Ok(()),
            }
        };

        match write_to_file() {
            Ok(()) => Ok(()),
            Err(msg) => {
                set_status_message!(self, "Can't save! I/O error: {}", msg);
                Ok(())
            }
        }
    }
}

fn editor_find_callback(editor: &mut Editor, needle: &[char], key: EditorKey) {
    if needle.is_empty() {
        return;
    }

    if let Some((idx, highlight)) = &editor.stored_hl {
        editor.rows[*idx].highlights = highlight.clone();
        editor.stored_hl = None;
    }

    match key {
        EditorKey::Ctrl('m') | EditorKey::Other(ESC) => {
            editor.last_match = None;
            editor.search_dir = SearchDirection::Forward;
            return;
        }
        EditorKey::ArrowRight | EditorKey::ArrowDown | EditorKey::Ctrl('f') => {
            editor.search_dir = SearchDirection::Forward;
        }
        EditorKey::ArrowLeft | EditorKey::ArrowUp => {
            editor.search_dir = SearchDirection::Backward;
        }
        _ => {
            editor.last_match = None;
            editor.search_dir = SearchDirection::Forward;
        }
    }

    if editor.last_match.is_none() {
        editor.search_dir = SearchDirection::Forward;
    }

    let mut search_idx = editor.last_match.unwrap_or(editor.rows.len());

    for _ in 0..editor.rows.len() {
        search_idx = editor.search_dir.step(search_idx, editor.rows.len() - 1);

        let num_rows = editor.rows.len();
        let row = editor
            .rows
            .get_mut(search_idx)
            .expect("search index should always be valid!");

        if let Some(idx) =
            row.line.windows(needle.len()).position(|hay| hay == needle)
        {
            editor.last_match = Some(search_idx);
            editor.cursor_y = search_idx;
            editor.cursor_x = idx;
            editor.row_offset = num_rows;

            editor.stored_hl = Some((search_idx, row.highlights.clone()));
            row.highlights[idx..idx + needle.len()].fill(Highlight::Match);
            break;
        }
    }
}

impl Editor {
    fn find(&mut self) -> Result<(), Box<dyn Error>> {
        let saved_cx = self.cursor_x;
        let saved_cy = self.cursor_y;
        let saved_coloff = self.col_offset;
        let saved_rowoff = self.row_offset;

        let input = self
            .prompt("Search (ESC/Arrows/Enter)", Some(editor_find_callback))?;
        if input.is_none() {
            self.cursor_x = saved_cx;
            self.cursor_y = saved_cy;
            self.col_offset = saved_coloff;
            self.row_offset = saved_rowoff;
        }

        Ok(())
    }

    fn open(&mut self, file_path: &Path) -> Result<(), Box<dyn Error>> {
        let reader = match File::open(file_path) {
            Ok(file) => BufReader::new(file),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                self.file = Some(file_path.to_owned());
                self.select_syntax_highlight();
                return Ok(());
            }
            Err(err) => return Err(Box::new(err)),
        };

        for (index, line) in reader.lines().enumerate() {
            let line = line?
                .trim_end_matches(|c| c == '\n' || c == '\r')
                .chars()
                .collect();
            let row = Row {
                index,
                line,
                render: vec![],
                highlights: vec![],
                in_comment: false,
            };
            self.rows.push(row);
            self.update_row(self.rows.len() - 1);
        }

        self.file = Some(file_path.to_owned());
        self.select_syntax_highlight();

        Ok(())
    }

    fn maybe_update_screen(&mut self) -> Result<(), Box<dyn Error>> {
        if self.win_changed.load(atomic::Ordering::Relaxed) {
            let (rows, cols) = get_window_size()?;
            self.screen_rows = rows - RED_STATUS_HEIGHT;
            self.screen_cols = cols;
            self.refresh_screen()?;
            self.win_changed.store(false, atomic::Ordering::Relaxed);
        }

        Ok(())
    }

    fn read_key(&mut self) -> Result<EditorKey, Box<dyn Error>> {
        let mut cbyte = [0; 1];
        while io::stdin().read(&mut cbyte)? != 1 {
            self.maybe_update_screen()?;
        }
        let c = cbyte[0] as char;

        if c == ESC {
            let mut seq = [0; 3];

            if io::stdin().read_exact(&mut seq[..1]).is_err() {
                return Ok(EditorKey::Other(ESC));
            }

            if seq[0] != b'[' || io::stdin().read_exact(&mut seq[1..2]).is_err()
            {
                return Ok(EditorKey::Meta(seq[0] as char));
            }

            match &seq[..2] {
                b"[A" => Ok(EditorKey::ArrowUp),
                b"[B" => Ok(EditorKey::ArrowDown),
                b"[C" => Ok(EditorKey::ArrowRight),
                b"[D" => Ok(EditorKey::ArrowLeft),
                b"[H" | b"OH" => Ok(EditorKey::Home),
                b"[F" | b"OF" => Ok(EditorKey::End),
                esc_seq
                    if esc_seq[0] == b'[' && esc_seq[1].is_ascii_digit() =>
                {
                    if io::stdin().read_exact(&mut seq[2..]).is_err() {
                        return Ok(EditorKey::Other(ESC));
                    }

                    match &seq {
                        b"[1~" | b"[7~" => Ok(EditorKey::Home),
                        b"[3~" => Ok(EditorKey::Delete),
                        b"[4~" | b"[8~" => Ok(EditorKey::End),
                        b"[5~" => Ok(EditorKey::PageUp),
                        b"[6~" => Ok(EditorKey::PageDown),
                        _ => Ok(EditorKey::Other(ESC)),
                    }
                }
                _ => Ok(EditorKey::Other(ESC)),
            }
        } else {
            let key = parse_utf8(cbyte[0], io::stdin())?;
            match key {
                '\0' => Ok(EditorKey::Ctrl(' ')),
                '\x01'..='\x1a' => {
                    Ok(EditorKey::Ctrl((key as u8 + 0x60) as char))
                }
                _ => Ok(EditorKey::Other(key)),
            }
        }
    }

    fn prompt(
        &mut self,
        prompt: &str,
        callback: Option<fn(&mut Editor, &[char], EditorKey)>,
    ) -> Result<Option<String>, Box<dyn Error>> {
        let mut str_input = String::new();
        let mut vec_input = vec![];
        let callback = match callback {
            Some(f) => f,
            None => |_: &mut Editor, _: &[char], _: EditorKey| {},
        };

        loop {
            set_status_message!(self, "{}: {}", prompt, str_input);
            self.refresh_screen()?;

            let key = self.read_key()?;
            match key {
                EditorKey::Delete
                | EditorKey::Other(BACKSPACE)
                | EditorKey::Ctrl('h') => {
                    str_input.pop();
                    vec_input.pop();
                }
                EditorKey::Other(ESC) => {
                    set_status_message!(self, "");
                    callback(self, &vec_input, key);
                    return Ok(None);
                }
                EditorKey::Ctrl('m') if !str_input.is_empty() => {
                    set_status_message!(self, "");
                    callback(self, &vec_input, key);
                    return Ok(Some(str_input));
                }
                EditorKey::Other(c) if !c.is_ascii_control() => {
                    str_input.push(c as char);
                    vec_input.push(c as char);
                }
                _ => (),
            }

            callback(self, &vec_input, key);
        }
    }

    fn move_cursor(&mut self, key: EditorKey) {
        match key {
            EditorKey::ArrowLeft => {
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                } else if self.cursor_y > 0 {
                    self.cursor_y -= 1;
                    if let Some(row) = self.rows.get(self.cursor_y) {
                        self.cursor_x = row.line.len();
                    }
                }
            }
            EditorKey::ArrowRight => {
                if let Some(row) = self.rows.get(self.cursor_y) {
                    match self.cursor_x.cmp(&row.line.len()) {
                        Ordering::Less => self.cursor_x += 1,
                        Ordering::Equal => {
                            self.cursor_x = 0;
                            self.cursor_y += 1;
                        }
                        Ordering::Greater => {}
                    }
                }
            }
            EditorKey::ArrowUp if self.cursor_y > 0 => self.cursor_y -= 1,
            EditorKey::ArrowDown if self.cursor_y < self.rows.len() => {
                self.cursor_y += 1
            }
            _ => (),
        }

        if let Some(row) = self.rows.get(self.cursor_y) {
            self.cursor_x = self.cursor_x.clamp(0, row.line.len());
        } else {
            self.cursor_x = 0;
        }
    }

    fn delete_range(&mut self, (begin, end): (Position, Position)) {
        self.cursor_x = end.0;
        self.cursor_y = end.1;

        while (self.cursor_x, self.cursor_y) != begin {
            self.delete_char();
        }
    }

    fn copy_range(&mut self, (begin, end): (Position, Position)) {
        let old_pos = (self.cursor_x, self.cursor_y);
        self.cursor_x = begin.0;
        self.cursor_y = begin.1;
        let mut copy = String::new();

        while (self.cursor_x, self.cursor_y) != end {
            if let Some(row) = self.rows.get(self.cursor_y) {
                if self.cursor_x >= row.line.len() {
                    copy.push('\n')
                } else {
                    copy.push(row.line[self.cursor_x])
                }
            }
            self.move_cursor(EditorKey::ArrowRight);
        }

        self.mark = None;
        self.clipboard = copy;
        self.cursor_x = old_pos.0;
        self.cursor_y = old_pos.1;
    }

    fn paste(&mut self) {
        let mut clipboard = std::mem::take(&mut self.clipboard);
        for c in clipboard.chars() {
            match c {
                '\n' => self.insert_newline(),
                _ => self.insert_char(c),
            }
        }
        self.clipboard = std::mem::take(&mut clipboard);
    }

    fn process_keypress(&mut self) -> Result<bool, Box<dyn Error>> {
        let key = self.read_key()?;
        match key {
            EditorKey::Ctrl('m') => {
                self.insert_newline();
            }
            EditorKey::Ctrl('q') => {
                if self.dirty && self.quit_times > 0 {
                    set_status_message!(
                        self,
                        "WARNING!!! File has unsaved changes. \
                     Press C-q {} more times to quit.",
                        self.quit_times
                    );
                    self.quit_times -= 1;
                    return Ok(true);
                } else {
                    clear_screen(&mut io::stdout())?;
                    return Ok(false);
                }
            }
            EditorKey::Ctrl('s') => {
                self.save()?;
            }
            EditorKey::Home => {
                self.cursor_x = 0;
            }
            EditorKey::End => {
                if let Some(row) = self.rows.get(self.cursor_y) {
                    self.cursor_x = row.line.len();
                }
            }
            EditorKey::Ctrl('f') => self.find()?,
            EditorKey::Delete
            | EditorKey::Other(BACKSPACE)
            | EditorKey::Ctrl('h') => {
                if let Some(selection) = self.selection() {
                    self.delete_range(selection);
                } else {
                    if key == EditorKey::Delete {
                        self.move_cursor(EditorKey::ArrowRight);
                    }
                    self.delete_char();
                }
            }
            EditorKey::PageUp | EditorKey::PageDown => {
                if key == EditorKey::PageUp {
                    self.cursor_y = self.row_offset;
                } else if key == EditorKey::PageDown {
                    self.cursor_y = usize::clamp(
                        self.row_offset + self.screen_rows - 1,
                        0,
                        self.rows.len(),
                    );
                }

                for _ in 0..self.screen_rows {
                    self.move_cursor(if key == EditorKey::PageUp {
                        EditorKey::ArrowUp
                    } else {
                        EditorKey::ArrowDown
                    })
                }
            }
            EditorKey::ArrowLeft
            | EditorKey::ArrowRight
            | EditorKey::ArrowUp
            | EditorKey::ArrowDown => {
                self.move_cursor(key);
            }
            EditorKey::Other(ESC) | EditorKey::Ctrl('l') => (),
            EditorKey::Ctrl(' ') => {
                if let Some(row) = self.rows.get(self.cursor_y) {
                    self.mark = Some((
                        editor_row_cursor_to_render(row, self.cursor_x),
                        self.cursor_y,
                    ));
                }
            }
            EditorKey::Ctrl('c') => {
                if let Some(selection) = self.selection() {
                    self.copy_range(selection);
                }
            }
            EditorKey::Ctrl('v') => {
                self.paste();
            }
            EditorKey::Meta(c) => {
                set_status_message!(self, "M-{} isn't bound!", c);
            }
            EditorKey::Ctrl(c) => {
                set_status_message!(self, "C-{} isn't bound!", c);
            }
            EditorKey::Other(byte) => {
                self.insert_char(byte as char);
            }
        }

        self.quit_times = RED_QUIT_TIMES;
        Ok(true)
    }
}

fn parse_utf8(
    size_indicator: u8,
    mut input_stream: impl Read,
) -> Result<char, Box<dyn Error>> {
    // NOTE: see https://en.wikipedia.org/wiki/UTF-8#Encoding
    let first_byte = size_indicator as u32;
    let maybe_char = if first_byte & 0x80 == 0 {
        // one byte code point
        Some(size_indicator as char)
    } else if 0xE0 & first_byte == 0xC0 {
        // two byte code point
        let mut rest = [0; 1];
        input_stream.read_exact(&mut rest)?;
        char::from_u32((0x1F & first_byte) << 6 | (0x3F & rest[0] as u32))
    } else if 0xF0 & first_byte == 0xE0 {
        // three byte code point
        let mut rest = [0; 2];
        input_stream.read_exact(&mut rest)?;
        char::from_u32(
            (0x0F & first_byte) << 12
                | (0x3F & rest[0] as u32) << 6
                | (0x3F & rest[1] as u32),
        )
    } else if 0xF8 & first_byte == 0xF0 {
        // four byte code point
        let mut rest = [0; 3];
        input_stream.read_exact(&mut rest)?;
        char::from_u32(
            (0x07 & first_byte) << 18
                | (0x3F & rest[0] as u32) << 12
                | (0x3F & rest[1] as u32) << 6
                | (0x3F & rest[2] as u32),
        )
    } else {
        None
    };
    maybe_char.ok_or_else(|| -> Box<dyn Error> {
        Box::new(EditorError::InvalidUtf8Input)
    })
}

fn clear_screen(dest: &mut impl Write) -> Result<(), Box<dyn Error>> {
    dest.write_all(ESC_SEQ_CLEAR_SCREEN)?;
    dest.write_all(ESC_SEQ_RESET_CURSOR)?;
    dest.flush()?;

    Ok(())
}

impl Editor {
    fn line_number_sep_len() -> usize {
        RED_LINE_SEP.chars().count()
    }

    fn line_number_space(&self) -> usize {
        format!("{}", self.screen_rows + self.row_offset).len()
            + Editor::line_number_sep_len()
    }

    fn scroll(&mut self) {
        self.render_x = 0;
        if let Some(row) = self.rows.get(self.cursor_y) {
            self.render_x = editor_row_cursor_to_render(row, self.cursor_x);
        }

        if self.cursor_y < self.row_offset {
            self.row_offset = self.cursor_y;
        }
        if self.cursor_y >= self.row_offset + self.screen_rows {
            self.row_offset = self.cursor_y - self.screen_rows + 1;
        }

        self.editor_cols = self.screen_cols - self.line_number_space();

        if self.render_x >= self.col_offset + self.editor_cols {
            self.col_offset = self.render_x - self.editor_cols + 1;
        }
        if self.render_x < self.col_offset {
            self.col_offset = self.render_x;
        }
    }

    fn position_less(pos1: &(usize, usize), pos2: &(usize, usize)) -> bool {
        let ((x1, y1), (x2, y2)) = (pos1, pos2);

        y1 < y2 || y1 == y2 && x1 < x2
    }

    fn draw_rows(&self, dest: &mut impl Write) -> Result<(), Box<dyn Error>> {
        let left_padding = self.line_number_space();
        for y in 0..self.screen_rows {
            let filerow = y + self.row_offset;
            if filerow >= self.rows.len() {
                if self.rows.is_empty() && y == self.screen_rows / 3 {
                    let mut welcome_msg =
                        format!("red editor -- version {}", RED_VERSION);
                    welcome_msg.truncate(self.editor_cols);

                    let mut padding =
                        (self.editor_cols - welcome_msg.len()) / 2;
                    if padding > 0 {
                        dest.write_all(b"~")?;
                        padding -= 1;
                    }

                    while padding > 0 {
                        dest.write_all(b" ")?;
                        padding -= 1;
                    }

                    dest.write_all(&welcome_msg.into_bytes())?;
                } else {
                    dest.write_all(b"~")?;
                }
            } else {
                // NOTE: Ensure that only the first screen_cols glyphs of the
                // line are printed!
                let mut prev_color: Option<&Highlight> = None;
                if filerow == self.cursor_y {
                    dest.write_all(ESC_SEQ_INVERT_COLORS)?;
                }
                dest.write_all(
                    format!(
                        "{:>width$}",
                        filerow + 1,
                        width = left_padding - Editor::line_number_sep_len(),
                    )
                    .as_bytes(),
                )?;
                if filerow == self.cursor_y {
                    dest.write_all(ESC_SEQ_RESET_ALL)?;
                }
                dest.write_all(RED_LINE_SEP.as_bytes())?;

                let selection = self.selection();

                for ((column, c), hl) in self.rows[filerow]
                    .render
                    .iter()
                    .enumerate()
                    .zip(self.rows[filerow].highlights.iter())
                    .skip(self.col_offset)
                    .take(self.editor_cols)
                {
                    if let Some(((begin_x, begin_y), (end_x, end_y))) =
                        selection
                    {
                        if end_x <= column && end_y == filerow
                            || end_y < filerow
                        {
                            dest.write_all(ESC_SEQ_COLOR_DEFAULT_BG)?;
                        } else if column >= begin_x && filerow == begin_y
                            || filerow > begin_y
                        {
                            dest.write_all(ESC_SEQ_COLOR_GRAY_BG)?;
                        }
                    }
                    if c.is_ascii_control() {
                        let char_code = *c as u8;
                        let sym = if char_code <= 26 {
                            b'@' + char_code
                        } else {
                            b'?'
                        };
                        dest.write_all(ESC_SEQ_INVERT_COLORS)?;
                        dest.write_all(&[sym])?;
                        dest.write_all(ESC_SEQ_RESET_ALL)?;
                        if let Some(prev_hl) = prev_color {
                            dest.write_all(prev_hl.color())?;
                        }
                    } else {
                        let current_color = Some(hl);
                        if prev_color != current_color {
                            dest.write_all(hl.color())?;
                            prev_color = current_color;
                        }
                        dest.write_all(&c.to_string().into_bytes())?;
                    }
                }
                dest.write_all(ESC_SEQ_COLOR_DEFAULT)?;
                dest.write_all(ESC_SEQ_COLOR_DEFAULT_BG)?;
            }
            dest.write_all(ESC_SEQ_CLEAR_LINE)?;
            dest.write_all(b"\r\n")?;
        }

        Ok(())
    }

    fn selection(&self) -> Option<(Position, Position)> {
        match self.mark {
            Some(mark) => {
                let cursor_pos = (self.cursor_x, self.cursor_y);
                if Editor::position_less(&mark, &cursor_pos) {
                    Some((mark, cursor_pos))
                } else {
                    Some((cursor_pos, mark))
                }
            }
            None => None,
        }
    }

    fn draw_status_bar(
        &self,
        dest: &mut impl Write,
    ) -> Result<(), Box<dyn Error>> {
        dest.write_all(ESC_SEQ_INVERT_COLORS)?;

        let file_name = match &self.file {
            Some(path) => path.to_string_lossy().to_string(),
            None => "[No Name]".to_string(),
        };

        let status_left = format!(
            "{:.20} - {} lines {}",
            file_name,
            self.rows.len(),
            if self.dirty { "(modified)" } else { "" }
        );
        dest.write_all(status_left.as_bytes())?;

        let syntax_name = self.syntax.map(|s| s.name).unwrap_or("no ft");
        let status_right = format!(
            "{} | {}/{}",
            syntax_name,
            self.cursor_y + 1,
            self.rows.len()
        );

        for len in status_left.len()..self.screen_cols {
            if self.screen_cols - len == status_right.len() {
                dest.write_all(status_right.as_bytes())?;
                break;
            } else {
                dest.write_all(b" ")?;
            }
        }

        dest.write_all(ESC_SEQ_RESET_ALL)?;
        dest.write_all(b"\r\n")?;

        Ok(())
    }

    fn draw_message_bar(
        &self,
        dest: &mut impl Write,
    ) -> Result<(), Box<dyn Error>> {
        dest.write_all(ESC_SEQ_CLEAR_LINE)?;
        let mut msg = self.status_msg.clone();
        msg.truncate(self.editor_cols);
        let now = SystemTime::now();

        if !msg.is_empty()
            && now.duration_since(self.status_time)?.as_secs() < 5
        {
            dest.write_all(msg.as_bytes())?;
        }

        Ok(())
    }

    fn refresh_screen(&mut self) -> Result<(), Box<dyn Error>> {
        let mut buffer = vec![];
        let mut stdout = io::stdout();

        self.scroll();

        buffer.write_all(ESC_SEQ_HIDE_CURSOR)?;
        buffer.write_all(ESC_SEQ_RESET_CURSOR)?;

        self.draw_rows(&mut buffer)?;
        self.draw_status_bar(&mut buffer)?;
        self.draw_message_bar(&mut buffer)?;

        buffer.write_all(&esc_seq_move_cursor(
            (self.cursor_y - self.row_offset) + 1,
            (self.render_x - self.col_offset) + 1 + self.line_number_space(),
        ))?;

        buffer.write_all(ESC_SEQ_SHOW_CURSOR)?;

        stdout.write_all(&buffer)?;
        stdout.flush()?;

        Ok(())
    }

    fn set_status_message(&mut self, msg: String) {
        self.status_msg = msg;
        self.status_time = SystemTime::now();
    }

    fn run(&mut self) -> Result<(), Box<dyn Error>> {
        loop {
            self.refresh_screen()?;
            if !self.process_keypress()? {
                break;
            }
        }

        Ok(())
    }
}

fn enable_raw_mode() -> Result<(), Box<dyn Error>> {
    let mut attr = Termios::from_fd(STDIN_FILENO)?;
    attr.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
    attr.c_oflag &= !(OPOST);
    attr.c_cflag |= CS8;
    attr.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
    attr.c_cc[VMIN] = 0;
    attr.c_cc[VTIME] = 1;
    termios::tcsetattr(STDIN_FILENO, TCSAFLUSH, &attr)?;

    Ok(())
}

fn main() {
    let mut editor = Editor::new().unwrap();
    let args = env::args().collect::<Vec<_>>();

    if let [_prog, filename] = args.as_slice() {
        editor.open(Path::new(&filename)).expect("open failed!");
    }

    set_status_message!(
        &mut editor,
        "HELP: C-s = save | C-q = quit | C-f = find | C-SPC = select"
    );

    if let Err(e) = editor.run() {
        clear_screen(&mut io::stdout()).unwrap();
        eprintln!("error: {}", e)
    }
}

#[cfg(test)]
mod tests;
