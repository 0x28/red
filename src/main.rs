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

use red_ioctl::get_window_size_ioctl;
mod red_error;
mod red_ioctl;
use red_error::EditorError;

const fn ctrl(c: char) -> u8 {
    c as u8 & 0x1f
}

const CTRL_F: u8 = ctrl('f');
const CTRL_H: u8 = ctrl('h');
const CTRL_L: u8 = ctrl('l');
const CTRL_Q: u8 = ctrl('q');
const CTRL_S: u8 = ctrl('s');
const ESC: u8 = b'\x1b';
const BACKSPACE: u8 = b'\x7f';

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

fn esc_seq_move_cursor(pos_y: usize, pos_x: usize) -> Vec<u8> {
    format!("\x1b[{};{}H", pos_y, pos_x).into_bytes()
}

const RED_VERSION: &str = env!("CARGO_PKG_VERSION");
const RED_TAB_STOP: usize = 8;
const RED_QUIT_TIMES: u8 = 3;
const RED_STATUS_HEIGHT: usize = 2;

macro_rules! editor_set_status_message {
    ($config: expr, $($arg:tt)*) => {
        editor_set_status_message($config, format!($($arg)*));
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
    Other(u8),
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
    line: Vec<char>,
    render: Vec<char>,
    highlights: Vec<Highlight>,
}

#[derive(Clone, PartialEq, Debug)]
enum Highlight {
    Normal,
    Comment,
    MultiLineComment,
    Keyword,
    Type,
    String,
    Number,
    Match,
}

const HIGHLIGHT_NUMBERS: u32 = 1 << 0;
const HIGHLIGHT_STRINGS: u32 = 1 << 1;

struct Syntax {
    name: &'static str,
    extensions: &'static [&'static str],
    single_line_comment: &'static str,
    multi_line_comment: (&'static str, &'static str),
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    flags: u32,
}

const SYNTAXES: &[Syntax] = &[
    Syntax {
        name: "c",
        extensions: &[".c", ".h", ".cpp"],
        single_line_comment: "//",
        multi_line_comment: ("/*", "*/"),
        keywords: &[
            "switch", "if", "while", "for", "break", "continue", "return",
            "else", "struct", "union", "typedef", "static", "enum", "class",
            "case",
        ],
        types: &[
            "int", "long", "double", "float", "char", "unsigned", "signed",
            "void",
        ],
        flags: HIGHLIGHT_NUMBERS | HIGHLIGHT_STRINGS,
    },
    Syntax {
        name: "rust",
        extensions: &[".rs"],
        single_line_comment: "//",
        multi_line_comment: ("/*", "*/"),
        keywords: &[
            "as", "break", "const", "continue", "crate", "else", "enum",
            "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
            "match", "mod", "move", "mut", "pub", "ref", "return", "self",
            "Self", "static", "struct", "super", "trait", "true", "type",
            "unsafe", "use", "where", "while", "async", "await", "dyn",
        ],
        types: &[
            "bool", "char", "f32", "f64", "i128", "i16", "i32", "i64", "i8",
            "isize", "str", "u128", "u16", "u32", "u64", "u8", "usize",
        ],
        flags: HIGHLIGHT_NUMBERS | HIGHLIGHT_STRINGS,
    },
];

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
        }
    }
}

impl Row {
    fn empty() -> Row {
        Row {
            line: vec![],
            render: vec![],
            highlights: vec![],
        }
    }
}

impl Default for Row {
    fn default() -> Self {
        Row::empty()
    }
}

struct EditorConfig {
    original: Termios,
    cursor_x: usize,
    cursor_y: usize,
    render_x: usize,
    screen_rows: usize,
    screen_cols: usize,
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
}

impl EditorConfig {
    fn new() -> Result<EditorConfig, Box<dyn Error>> {
        let original = Termios::from_fd(STDIN_FILENO)?;
        enable_raw_mode()?;
        let (rows, cols) = get_window_size()?;

        let win_changed = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(
            signal_hook::consts::SIGWINCH,
            Arc::clone(&win_changed),
        )?;

        Ok(EditorConfig {
            original,
            cursor_x: 0,
            cursor_y: 0,
            render_x: 0,
            screen_rows: rows - RED_STATUS_HEIGHT,
            screen_cols: cols,
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
        })
    }
}

impl Drop for EditorConfig {
    fn drop(&mut self) {
        // NOTE: Don't panic while dropping!
        if let Err(e) =
            termios::tcsetattr(STDIN_FILENO, TCSAFLUSH, &self.original)
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

fn editor_update_syntax(row: usize, config: &mut EditorConfig) {
    let row = &mut config.rows[row];

    row.highlights.resize(row.render.len(), Highlight::Normal);
    row.highlights.fill(Highlight::Normal);

    let syntax = match config.syntax {
        Some(s) => s,
        None => return,
    };

    let mut prev_sep = true;
    let mut in_string = None;
    let mut in_comment = false;

    let single_line_comment =
        syntax.single_line_comment.chars().collect::<Vec<_>>();
    let multi_line_comment = (
        syntax.multi_line_comment.0.chars().collect::<Vec<_>>(),
        syntax.multi_line_comment.0.chars().collect::<Vec<_>>(),
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
            } else if c == '"' {
                in_string = Some(c);
                row.highlights[idx] = Highlight::String;
                continue;
            }
        }

        if syntax.flags & HIGHLIGHT_NUMBERS != 0
            && (c.is_digit(10) && (prev_sep || prev_hl == Highlight::Number)
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
}

fn editor_select_syntax_highlight(config: &mut EditorConfig) {
    config.syntax = None;
    let file = match &config.file {
        Some(f) => f,
        None => return,
    };

    let file_ext = file.extension().map(OsStr::to_str).flatten();

    config.syntax = SYNTAXES.iter().find(|syntax| {
        syntax.extensions.iter().any(|ext| {
            let is_ext = ext.starts_with('.');
            is_ext && Some(&ext[1..]) == file_ext
                || !is_ext && file.to_string_lossy().contains(ext)
        })
    });

    if config.syntax.is_some() {
        for row in 0..config.rows.len() {
            editor_update_syntax(row, config);
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

fn editor_row_append(row: usize, content: &[char], config: &mut EditorConfig) {
    config.rows[row].line.extend_from_slice(content);
    editor_update_row(row, config);
}

fn editor_update_row(row_idx: usize, config: &mut EditorConfig) {
    let row = &mut config.rows[row_idx];

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

    editor_update_syntax(row_idx, config);
}

fn editor_delete_row(config: &mut EditorConfig, at: usize) {
    if at < config.rows.len() {
        config.rows.remove(at);
        config.dirty = true;
    }
}

fn editor_row_insert_char(
    row_idx: usize,
    mut at: usize,
    c: char,
    config: &mut EditorConfig,
) {
    let row = &mut config.rows[row_idx];
    if at > row.line.len() {
        at = row.line.len();
    }

    row.line.insert(at, c);
    editor_update_row(row_idx, config);
}

fn editor_row_delete_char(
    row_idx: usize,
    at: usize,
    config: &mut EditorConfig,
) {
    let row = &mut config.rows[row_idx];
    if at < row.line.len() {
        row.line.remove(at);
        editor_update_row(row_idx, config);
    }
}

fn editor_insert_char(config: &mut EditorConfig, c: char) {
    if config.cursor_y == config.rows.len() {
        config.rows.push(Row::empty())
    }

    editor_row_insert_char(config.cursor_y, config.cursor_x, c, config);

    config.cursor_x += 1;
    config.dirty = true;
}

fn editor_insert_newline(config: &mut EditorConfig) {
    if config.cursor_x == 0 {
        config.rows.insert(config.cursor_y, Row::empty());
    } else if let Some(current_row) = config.rows.get_mut(config.cursor_y) {
        let next_line = current_row.line[config.cursor_x..].to_vec();
        let next_row = Row {
            line: next_line,
            render: vec![],
            highlights: vec![],
        };
        current_row.line.truncate(config.cursor_x);
        config.rows.insert(config.cursor_y + 1, next_row);
        editor_update_row(config.cursor_y, config);
        editor_update_row(config.cursor_y + 1, config);
    }

    config.cursor_y += 1;
    config.cursor_x = 0;
}

fn editor_delete_char(config: &mut EditorConfig) {
    if config.cursor_x == 0 && config.cursor_y == 0 {
        return;
    }

    if let Some(row) = config.rows.get_mut(config.cursor_y) {
        if config.cursor_x > 0 {
            editor_row_delete_char(
                config.cursor_y,
                config.cursor_x - 1,
                config,
            );
            config.cursor_x -= 1;
            config.dirty = true;
        } else {
            let line = std::mem::take(&mut row.line);
            let prev_row = &mut config.rows[config.cursor_y - 1];
            config.cursor_x = prev_row.line.len();
            editor_row_append(config.cursor_y - 1, &line, config);
            editor_delete_row(config, config.cursor_y);
            config.cursor_y -= 1;
        }
    } else if config.cursor_y == config.rows.len() {
        // NOTE: we are in the last empty line -> nothing to delete
        config.cursor_y -= 1;
        config.cursor_x = config.rows[config.cursor_y].line.len();
    }
}

fn editor_write_rows(
    config: &EditorConfig,
    output: &mut impl Write,
) -> Result<usize, Box<dyn Error>> {
    let mut bytes = 0;
    for row in &config.rows {
        for c in &row.line {
            bytes += output.write(format!("{}", c).as_bytes())?;
        }
        bytes += output.write(b"\n")?;
    }

    Ok(bytes)
}

fn editor_save(config: &mut EditorConfig) -> Result<(), Box<dyn Error>> {
    if config.file.is_none() {
        match editor_prompt(config, "Save as (ESC to cancel)", None)? {
            Some(file) => config.file = Some(PathBuf::from(file)),
            None => {
                editor_set_status_message!(config, "Save aborted");
                return Ok(());
            }
        }
    }
    editor_select_syntax_highlight(config);

    config.dirty = false;
    let mut write_to_file = || -> Result<(), Box<dyn Error>> {
        match &config.file {
            Some(path) => {
                let mut file = BufWriter::new(File::create(path)?);
                let bytes_written = editor_write_rows(config, &mut file)?;
                editor_set_status_message!(
                    config,
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
            editor_set_status_message!(
                config,
                "Can't save! I/O error: {}",
                msg
            );
            Ok(())
        }
    }
}

fn editor_find_callback(
    config: &mut EditorConfig,
    needle: &[char],
    key: EditorKey,
) {
    if needle.is_empty() {
        return;
    }

    if let Some((idx, highlight)) = &config.stored_hl {
        config.rows[*idx].highlights = highlight.clone();
        config.stored_hl = None;
    }

    match key {
        EditorKey::Other(b'\r' | ESC) => {
            config.last_match = None;
            config.search_dir = SearchDirection::Forward;
            return;
        }
        EditorKey::ArrowRight
        | EditorKey::ArrowDown
        | EditorKey::Other(CTRL_F) => {
            config.search_dir = SearchDirection::Forward;
        }
        EditorKey::ArrowLeft | EditorKey::ArrowUp => {
            config.search_dir = SearchDirection::Backward;
        }
        _ => {
            config.last_match = None;
            config.search_dir = SearchDirection::Forward;
        }
    }

    if config.last_match.is_none() {
        config.search_dir = SearchDirection::Forward;
    }

    let mut search_idx = config.last_match.unwrap_or(config.rows.len());

    for _ in 0..config.rows.len() {
        search_idx = config.search_dir.step(search_idx, config.rows.len() - 1);

        let num_rows = config.rows.len();
        let row = config
            .rows
            .get_mut(search_idx)
            .expect("search index should always be valid!");

        if let Some(idx) =
            row.line.windows(needle.len()).position(|hay| hay == needle)
        {
            config.last_match = Some(search_idx);
            config.cursor_y = search_idx;
            config.cursor_x = idx;
            config.row_offset = num_rows;

            config.stored_hl = Some((search_idx, row.highlights.clone()));
            row.highlights[idx..idx + needle.len()].fill(Highlight::Match);
            break;
        }
    }
}

fn editor_find(config: &mut EditorConfig) -> Result<(), Box<dyn Error>> {
    let saved_cx = config.cursor_x;
    let saved_cy = config.cursor_y;
    let saved_coloff = config.col_offset;
    let saved_rowoff = config.row_offset;

    let input = editor_prompt(
        config,
        "Search (ESC/Arrows/Enter)",
        Some(editor_find_callback),
    )?;
    if input.is_none() {
        config.cursor_x = saved_cx;
        config.cursor_y = saved_cy;
        config.col_offset = saved_coloff;
        config.row_offset = saved_rowoff;
    }

    Ok(())
}

fn editor_open(
    config: &mut EditorConfig,
    file_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?
            .trim_end_matches(|c| c == '\n' || c == '\r')
            .chars()
            .collect();
        let row = Row {
            line,
            render: vec![],
            highlights: vec![],
        };
        config.rows.push(row);
        editor_update_row(config.rows.len() - 1, config);
    }

    config.file = Some(file_path.to_owned());
    editor_select_syntax_highlight(config);

    Ok(())
}

fn editor_maybe_update_screen(
    config: &mut EditorConfig,
) -> Result<(), Box<dyn Error>> {
    if config.win_changed.load(atomic::Ordering::Relaxed) {
        let (rows, cols) = get_window_size()?;
        config.screen_rows = rows - RED_STATUS_HEIGHT;
        config.screen_cols = cols;
        editor_refresh_screen(config)?;
        config.win_changed.store(false, atomic::Ordering::Relaxed);
    }

    Ok(())
}

fn editor_read_key(
    config: &mut EditorConfig,
) -> Result<EditorKey, Box<dyn Error>> {
    let mut c = [0; 1];
    while io::stdin().read(&mut c)? != 1 {
        editor_maybe_update_screen(config)?;
    }

    if c[0] == ESC {
        let mut seq = [0; 3];
        if io::stdin().read_exact(&mut seq[..2]).is_err() {
            return Ok(EditorKey::Other(ESC));
        }

        match &seq[..2] {
            b"[A" => Ok(EditorKey::ArrowUp),
            b"[B" => Ok(EditorKey::ArrowDown),
            b"[C" => Ok(EditorKey::ArrowRight),
            b"[D" => Ok(EditorKey::ArrowLeft),
            b"[H" | b"OH" => Ok(EditorKey::Home),
            b"[F" | b"OF" => Ok(EditorKey::End),
            esc_seq if esc_seq[0] == b'[' && esc_seq[1].is_ascii_digit() => {
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
        Ok(EditorKey::Other(c[0]))
    }
}

fn editor_prompt(
    config: &mut EditorConfig,
    prompt: &str,
    callback: Option<fn(&mut EditorConfig, &[char], EditorKey)>,
) -> Result<Option<String>, Box<dyn Error>> {
    let mut str_input = String::new();
    let mut vec_input = vec![];
    let callback = match callback {
        Some(f) => f,
        None => |_: &mut EditorConfig, _: &[char], _: EditorKey| {},
    };

    loop {
        editor_set_status_message!(config, "{}: {}", prompt, str_input);
        editor_refresh_screen(config)?;

        let key = editor_read_key(config)?;
        match key {
            EditorKey::Delete | EditorKey::Other(BACKSPACE | CTRL_H) => {
                str_input.pop();
                vec_input.pop();
            }
            EditorKey::Other(ESC) => {
                editor_set_status_message!(config, "");
                callback(config, &vec_input, key);
                return Ok(None);
            }
            EditorKey::Other(b'\r') if !str_input.is_empty() => {
                editor_set_status_message!(config, "");
                callback(config, &vec_input, key);
                return Ok(Some(str_input));
            }
            EditorKey::Other(c) if !c.is_ascii_control() && c < 128 => {
                str_input.push(c as char);
                vec_input.push(c as char);
            }
            _ => (),
        }

        callback(config, &vec_input, key);
    }
}

fn editor_move_cursor(config: &mut EditorConfig, key: EditorKey) {
    match key {
        EditorKey::ArrowLeft => {
            if config.cursor_x > 0 {
                config.cursor_x -= 1;
            } else if config.cursor_y > 0 {
                config.cursor_y -= 1;
                if let Some(row) = config.rows.get(config.cursor_y) {
                    config.cursor_x = row.line.len();
                }
            }
        }
        EditorKey::ArrowRight => {
            if let Some(row) = config.rows.get(config.cursor_y) {
                match config.cursor_x.cmp(&row.line.len()) {
                    Ordering::Less => config.cursor_x += 1,
                    Ordering::Equal => {
                        config.cursor_x = 0;
                        config.cursor_y += 1;
                    }
                    Ordering::Greater => {}
                }
            }
        }
        EditorKey::ArrowUp if config.cursor_y > 0 => config.cursor_y -= 1,
        EditorKey::ArrowDown if config.cursor_y < config.rows.len() => {
            config.cursor_y += 1
        }
        _ => (),
    }

    if let Some(row) = config.rows.get(config.cursor_y) {
        config.cursor_x = config.cursor_x.clamp(0, row.line.len());
    } else {
        config.cursor_x = 0;
    }
}

fn editor_process_keypress(
    config: &mut EditorConfig,
) -> Result<bool, Box<dyn Error>> {
    let key = editor_read_key(config)?;
    match key {
        EditorKey::Other(b'\r') => {
            editor_insert_newline(config);
        }
        EditorKey::Other(CTRL_Q) => {
            if config.dirty && config.quit_times > 0 {
                editor_set_status_message!(
                    config,
                    "WARNING!!! File has unsaved changes. \
                     Press Ctrl-Q {} more times to quit.",
                    config.quit_times
                );
                config.quit_times -= 1;
                return Ok(true);
            } else {
                clear_screen(&mut io::stdout())?;
                return Ok(false);
            }
        }
        EditorKey::Other(CTRL_S) => {
            editor_save(config)?;
        }
        EditorKey::Home => {
            config.cursor_x = 0;
        }
        EditorKey::End => {
            if let Some(row) = config.rows.get(config.cursor_y) {
                config.cursor_x = row.line.len();
            }
        }
        EditorKey::Other(CTRL_F) => editor_find(config)?,
        EditorKey::Delete | EditorKey::Other(BACKSPACE | CTRL_H) => {
            if key == EditorKey::Delete {
                editor_move_cursor(config, EditorKey::ArrowRight);
            }
            editor_delete_char(config);
        }
        EditorKey::PageUp | EditorKey::PageDown => {
            if key == EditorKey::PageUp {
                config.cursor_y = config.row_offset;
            } else if key == EditorKey::PageDown {
                config.cursor_y = usize::clamp(
                    config.row_offset + config.screen_rows - 1,
                    0,
                    config.rows.len(),
                );
            }

            for _ in 0..config.screen_rows {
                editor_move_cursor(
                    config,
                    if key == EditorKey::PageUp {
                        EditorKey::ArrowUp
                    } else {
                        EditorKey::ArrowDown
                    },
                )
            }
        }
        EditorKey::ArrowLeft
        | EditorKey::ArrowRight
        | EditorKey::ArrowUp
        | EditorKey::ArrowDown => {
            editor_move_cursor(config, key);
        }
        EditorKey::Other(ESC | CTRL_L) => (),
        EditorKey::Other(byte) => {
            editor_insert_char(config, byte as char);
        }
    }

    config.quit_times = RED_QUIT_TIMES;
    Ok(true)
}

fn clear_screen(dest: &mut impl Write) -> Result<(), Box<dyn Error>> {
    dest.write_all(ESC_SEQ_CLEAR_SCREEN)?;
    dest.write_all(ESC_SEQ_RESET_CURSOR)?;
    dest.flush()?;

    Ok(())
}

fn editor_scroll(config: &mut EditorConfig) {
    config.render_x = 0;
    if let Some(row) = config.rows.get(config.cursor_y) {
        config.render_x = editor_row_cursor_to_render(row, config.cursor_x);
    }

    if config.cursor_y < config.row_offset {
        config.row_offset = config.cursor_y;
    }
    if config.cursor_y >= config.row_offset + config.screen_rows {
        config.row_offset = config.cursor_y - config.screen_rows + 1;
    }
    if config.render_x < config.col_offset {
        config.col_offset = config.render_x;
    }
    if config.render_x >= config.col_offset + config.screen_cols {
        config.col_offset = config.render_x - config.screen_cols + 1;
    }
}

fn editor_draw_rows(
    config: &EditorConfig,
    dest: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    for y in 0..config.screen_rows {
        let filerow = y + config.row_offset;
        if filerow >= config.rows.len() {
            if config.rows.is_empty() && y == config.screen_rows / 3 {
                let mut welcome_msg =
                    format!("red editor -- version {}", RED_VERSION);
                welcome_msg.truncate(config.screen_cols);

                let mut padding = (config.screen_cols - welcome_msg.len()) / 2;
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
            for (c, hl) in config.rows[filerow]
                .render
                .iter()
                .zip(config.rows[filerow].highlights.iter())
                .skip(config.col_offset)
                .take(config.screen_cols)
            {
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
        }
        dest.write_all(ESC_SEQ_CLEAR_LINE)?;
        dest.write_all(b"\r\n")?;
    }

    Ok(())
}

fn editor_draw_status_bar(
    config: &EditorConfig,
    dest: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    dest.write_all(ESC_SEQ_INVERT_COLORS)?;

    let file_name = match &config.file {
        Some(path) => path.to_string_lossy().to_string(),
        None => "[No Name]".to_string(),
    };

    let status_left = format!(
        "{:.20} - {} lines {}",
        file_name,
        config.rows.len(),
        if config.dirty { "(modified)" } else { "" }
    );
    dest.write_all(status_left.as_bytes())?;

    let syntax_name = config.syntax.map(|s| s.name).unwrap_or("no ft");
    let status_right = format!(
        "{} | {}/{}",
        syntax_name,
        config.cursor_y + 1,
        config.rows.len()
    );

    for len in status_left.len()..config.screen_cols {
        if config.screen_cols - len == status_right.len() {
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

fn editor_draw_message_bar(
    config: &EditorConfig,
    dest: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    dest.write_all(ESC_SEQ_CLEAR_LINE)?;
    let mut msg = config.status_msg.clone();
    msg.truncate(config.screen_cols);
    let now = SystemTime::now();

    if !msg.is_empty() && now.duration_since(config.status_time)?.as_secs() < 5
    {
        dest.write_all(msg.as_bytes())?;
    }

    Ok(())
}

fn editor_refresh_screen(
    config: &mut EditorConfig,
) -> Result<(), Box<dyn Error>> {
    let mut buffer = vec![];
    let mut stdout = io::stdout();

    editor_scroll(config);

    buffer.write_all(ESC_SEQ_HIDE_CURSOR)?;
    buffer.write_all(ESC_SEQ_RESET_CURSOR)?;

    editor_draw_rows(&config, &mut buffer)?;
    editor_draw_status_bar(&config, &mut buffer)?;
    editor_draw_message_bar(&config, &mut buffer)?;

    buffer.write_all(&esc_seq_move_cursor(
        (config.cursor_y - config.row_offset) + 1,
        (config.render_x - config.col_offset) + 1,
    ))?;

    buffer.write_all(ESC_SEQ_SHOW_CURSOR)?;

    stdout.write_all(&buffer)?;
    stdout.flush()?;

    Ok(())
}

fn editor_set_status_message(config: &mut EditorConfig, msg: String) {
    config.status_msg = msg;
    config.status_time = SystemTime::now();
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

fn editor(config: &mut EditorConfig) -> Result<(), Box<dyn Error>> {
    loop {
        editor_refresh_screen(config)?;
        if !editor_process_keypress(config)? {
            break;
        }
    }

    Ok(())
}

fn main() {
    let mut conf = EditorConfig::new().unwrap();
    let args = env::args().collect::<Vec<_>>();

    if let [_prog, filename] = args.as_slice() {
        editor_open(&mut conf, Path::new(&filename)).expect("open failed!");
    }

    editor_set_status_message!(
        &mut conf,
        "HELP: Ctrl-S = save | Ctrl-Q = quit | Ctrl-F = find"
    );

    if let Err(e) = editor(&mut conf) {
        clear_screen(&mut io::stdout()).unwrap();
        eprintln!("error: {}", e)
    }
}
