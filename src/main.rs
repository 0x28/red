use std::error::Error;
use std::io::{self, Read, Write};

use termios::{
    Termios, BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP,
    IXON, OPOST, TCSAFLUSH, VMIN, VTIME,
};

const fn ctrl(c: char) -> u8 {
    c as u8 & 0x1f
}

const CTRL_Q: u8 = ctrl('q');

const STDIN_FILENO: i32 = 0;
// const STDOUT_FILENO: i32 = 1;
// const STDERR_FILENO: i32 = 2;

struct TerminalReset {
    original: Termios,
}

impl TerminalReset {
    fn new(original: Termios) -> TerminalReset {
        TerminalReset { original }
    }
}

impl Drop for TerminalReset {
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
            editor_refresh_screen()?;
            Ok(false)
        }
        _ => Ok(true),
    }
}

fn editor_refresh_screen() -> Result<(), Box<dyn Error>> {
    let mut stdout = io::stdout();

    stdout.write_all(b"\x1b[2J")?;
    stdout.write_all(b"\x1b[H")?;
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

fn editor() -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;

    loop {
        editor_refresh_screen()?;
        if !editor_process_keypress()? {
            break;
        }
    }

    Ok(())
}

fn main() {
    let _term_reset =
        TerminalReset::new(Termios::from_fd(STDIN_FILENO).expect("tcgetattr"));

    if let Err(e) = editor() {
        editor_refresh_screen().unwrap();
        eprintln!("error: {}", e)
    }
}
