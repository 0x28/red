use libc::STDIN_FILENO;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::time::SystemTime;
use std::{env, error::Error, path::Path};
use std::{fs::File, path::PathBuf};
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

const CTRL_Q: u8 = ctrl('q');
const ESC: u8 = b'\x1b';

const ESC_SEQ_RESET_CURSOR: &[u8] = b"\x1b[H";
const ESC_SEQ_CLEAR_SCREEN: &[u8] = b"\x1b[2J";
const ESC_SEQ_BOTTOM_RIGHT: &[u8] = b"\x1b[999C\x1b[999B";
const ESC_SEQ_QUERY_CURSOR: &[u8] = b"\x1b[6n";
const ESC_SEQ_HIDE_CURSOR: &[u8] = b"\x1b[?25l";
const ESC_SEQ_SHOW_CURSOR: &[u8] = b"\x1b[?25h";
const ESC_SEQ_CLEAR_LINE: &[u8] = b"\x1b[K";
const ESC_SEQ_INVERT_COLORS: &[u8] = b"\x1b[7m";
const ESC_SEQ_RESET_COLORS: &[u8] = b"\x1b[m";

fn esc_seq_move_cursor(pos_y: usize, pos_x: usize) -> Vec<u8> {
    format!("\x1b[{};{}H", pos_y, pos_x).into_bytes()
}

const RED_VERSION: &str = env!("CARGO_PKG_VERSION");
const RED_TAB_STOP: usize = 8;

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

struct Row {
    line: String,
    render: Vec<char>,
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
}

impl EditorConfig {
    fn new() -> Result<EditorConfig, Box<dyn Error>> {
        let original = Termios::from_fd(STDIN_FILENO)?;
        enable_raw_mode()?;
        let (rows, cols) = get_window_size()?;

        Ok(EditorConfig {
            original,
            cursor_x: 0,
            cursor_y: 0,
            render_x: 0,
            screen_rows: rows - 2,
            screen_cols: cols,
            row_offset: 0,
            col_offset: 0,
            rows: vec![],
            file: None,
            status_msg: String::new(),
            status_time: SystemTime::UNIX_EPOCH,
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

fn editor_row_cursor_to_render(row: &Row, cursor_x: usize) -> usize {
    let mut render_x = 0;

    for c in row.line.chars().take(cursor_x) {
        if c == '\t' {
            render_x += (RED_TAB_STOP - 1) - (render_x % RED_TAB_STOP);
        }
        render_x += 1;
    }

    render_x
}

fn editor_update_row(row: &mut Row) {
    row.render.clear();
    let mut idx = 0;
    for c in row.line.chars() {
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
            .to_string();
        let mut row = Row {
            line,
            render: vec![],
        };
        editor_update_row(&mut row);
        config.rows.push(row);
    }

    config.file = Some(file_path.to_owned());

    Ok(())
}

fn editor_read_key() -> Result<EditorKey, Box<dyn Error>> {
    let mut c = [0; 1];
    while io::stdin().read(&mut c)? != 1 {}

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
                if config.cursor_x < row.line.len() {
                    config.cursor_x += 1;
                } else if config.cursor_x == row.line.len() {
                    config.cursor_x = 0;
                    config.cursor_y += 1;
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
    let key = editor_read_key()?;
    match key {
        EditorKey::Other(CTRL_Q) => {
            clear_screen(&mut io::stdout())?;
            Ok(false)
        }
        EditorKey::Home => {
            config.cursor_x = 0;
            Ok(true)
        }
        EditorKey::End => {
            if let Some(row) = config.rows.get(config.cursor_y) {
                config.cursor_x = row.line.len();
            }

            Ok(true)
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

            Ok(true)
        }
        EditorKey::ArrowLeft
        | EditorKey::ArrowRight
        | EditorKey::ArrowUp
        | EditorKey::ArrowDown => {
            editor_move_cursor(config, key);
            Ok(true)
        }
        _ => Ok(true),
    }
}

fn clear_screen(dest: &mut impl Write) -> Result<(), Box<dyn Error>> {
    dest.write_all(ESC_SEQ_CLEAR_SCREEN)?;
    dest.write_all(ESC_SEQ_RESET_CURSOR)?;
    dest.flush()?;

    Ok(())
}

fn editor_scroll(config: &mut EditorConfig) {
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
            let truncated_line = config.rows[filerow]
                .render
                .iter()
                .skip(config.col_offset)
                .take(config.screen_cols)
                .collect::<String>();

            dest.write_all(&truncated_line.into_bytes())?;
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

    let status_left =
        format!("{:.20} - {} lines", file_name, config.rows.len());
    dest.write_all(status_left.as_bytes())?;

    let status_right = format!("{}/{}", config.cursor_y + 1, config.rows.len());

    for len in status_left.len()..config.screen_cols {
        if config.screen_cols - len == status_right.len() {
            dest.write_all(status_right.as_bytes())?;
            break;
        } else {
            dest.write_all(b" ")?;
        }
    }

    dest.write_all(ESC_SEQ_RESET_COLORS)?;
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

macro_rules! editor_set_status_message {
    ($config: expr, $($arg:tt)*) => {
	editor_set_status_message($config, format!($($arg)*));
    };
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

    editor_set_status_message!(&mut conf, "HELP: Ctrl-Q = quit");

    if let Err(e) = editor(&mut conf) {
        clear_screen(&mut io::stdout()).unwrap();
        eprintln!("error: {}", e)
    }
}
