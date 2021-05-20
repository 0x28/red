use libc::STDIN_FILENO;
use red_ioctl::get_terminal_win_size;
use std::error::Error;
use std::io::{self, Read, Write};
use termios::{
    Termios, BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP,
    IXON, OPOST, TCSAFLUSH, VMIN, VTIME,
};

mod red_ioctl;

const fn ctrl(c: char) -> u8 {
    c as u8 & 0x1f
}

const CTRL_Q: u8 = ctrl('q');

const ESC_SEQ_RESET_CURSOR: &[u8] = b"\x1b[H";
const ESC_SEQ_CLEAR_SCREEN: &[u8] = b"\x1b[2J";

struct EditorConfig {
    original: Termios,
    screen_rows: usize,
    screen_cols: usize,
}

impl EditorConfig {
    fn new(original: Termios) -> Result<EditorConfig, Box<dyn Error>> {
        let (rows, cols) = get_terminal_win_size()?;
        Ok(EditorConfig {
            original,
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

fn editor_read_key() -> Result<u8, Box<dyn Error>> {
    let mut c = [0; 1];
    while io::stdin().read(&mut c)? != 1 {}

    Ok(c[0])
}

fn editor_process_keypress() -> Result<bool, Box<dyn Error>> {
    match editor_read_key()? {
        CTRL_Q => {
            clear_screen()?;
            Ok(false)
        }
        _ => Ok(true),
    }
}

fn clear_screen() -> Result<(), Box<dyn Error>> {
    let mut stdout = io::stdout();

    stdout.write_all(ESC_SEQ_CLEAR_SCREEN)?;
    stdout.write_all(ESC_SEQ_RESET_CURSOR)?;
    stdout.flush()?;

    Ok(())
}

fn editor_draw_rows(config: &EditorConfig) -> Result<(), Box<dyn Error>> {
    let mut stdout = io::stdout();
    for y in 0..config.screen_rows {
        stdout.write_all(b"~")?;
        if y < config.screen_rows - 1 {
            stdout.write_all(b"\r\n")?;
        }
    }

    Ok(())
}

fn editor_refresh_screen(config: &EditorConfig) -> Result<(), Box<dyn Error>> {
    let mut stdout = io::stdout();

    clear_screen()?;
    editor_draw_rows(&config)?;
    stdout.write_all(ESC_SEQ_RESET_CURSOR)?;
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

fn editor(config: &EditorConfig) -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;

    loop {
        editor_refresh_screen(&config)?;
        if !editor_process_keypress()? {
            break;
        }
    }

    Ok(())
}

fn main() {
    let conf =
        EditorConfig::new(Termios::from_fd(STDIN_FILENO).expect("tcgetattr"))
            .unwrap();

    if let Err(e) = editor(&conf) {
        clear_screen().unwrap();
        eprintln!("error: {}", e)
    }
}
