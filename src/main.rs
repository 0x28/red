use libc::STDIN_FILENO;
use std::error::Error;
use std::io::{self, Read, Write};
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

fn esc_seq_move_cursor(pos_y: usize, pos_x: usize) -> Vec<u8> {
    format!("\x1b[{};{}H", pos_y, pos_x).into_bytes()
}

const RED_VERSION: &str = env!("CARGO_PKG_VERSION");

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

struct EditorConfig {
    original: Termios,
    cursor_x: usize,
    cursor_y: usize,
    screen_rows: usize,
    screen_cols: usize,
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
            screen_rows: rows,
            screen_cols: cols,
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

fn editor_read_key() -> Result<EditorKey, Box<dyn Error>> {
    let mut c = [0; 1];
    while io::stdin().read(&mut c)? != 1 {}

    if c[0] == ESC {
        let mut seq = [0; 3];
        if let Err(_) = io::stdin().read_exact(&mut seq[..2]) {
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
                if let Err(_) = io::stdin().read_exact(&mut seq[2..]) {
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
        EditorKey::ArrowLeft if config.cursor_x > 0 => config.cursor_x -= 1,
        EditorKey::ArrowRight if config.cursor_x < config.screen_cols - 1 => {
            config.cursor_x += 1
        }
        EditorKey::ArrowUp if config.cursor_y > 0 => config.cursor_y -= 1,
        EditorKey::ArrowDown if config.cursor_y < config.screen_rows - 1 => {
            config.cursor_y += 1
        }
        _ => (),
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
            config.cursor_x = config.screen_cols - 1;
            Ok(true)
        }
        EditorKey::PageUp | EditorKey::PageDown => {
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

fn editor_draw_rows(
    config: &EditorConfig,
    dest: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    for y in 0..config.screen_rows {
        if y == config.screen_rows / 3 {
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

        dest.write_all(ESC_SEQ_CLEAR_LINE)?;
        if y < config.screen_rows - 1 {
            dest.write_all(b"\r\n")?;
        }
    }

    Ok(())
}

fn editor_refresh_screen(config: &EditorConfig) -> Result<(), Box<dyn Error>> {
    let mut buffer = vec![];
    let mut stdout = io::stdout();

    buffer.write_all(ESC_SEQ_HIDE_CURSOR)?;
    buffer.write_all(ESC_SEQ_RESET_CURSOR)?;

    editor_draw_rows(&config, &mut buffer)?;
    buffer.write_all(&esc_seq_move_cursor(
        config.cursor_y + 1,
        config.cursor_x + 1,
    ))?;

    buffer.write_all(ESC_SEQ_SHOW_CURSOR)?;

    stdout.write_all(&buffer)?;
    stdout.flush()?;

    Ok(())
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
        editor_refresh_screen(&config)?;
        if !editor_process_keypress(config)? {
            break;
        }
    }

    Ok(())
}

fn main() {
    let mut conf = EditorConfig::new().unwrap();

    if let Err(e) = editor(&mut conf) {
        clear_screen(&mut io::stdout()).unwrap();
        eprintln!("error: {}", e)
    }
}
