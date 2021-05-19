use std::io::{self, Read};

use termios::{
    self, Termios, BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG,
    ISTRIP, IXON, OPOST, TCSAFLUSH, VMIN, VTIME,
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

fn editor_read_key() -> u8 {
    let mut c = [0; 1];
    while io::stdin().read(&mut c).expect("read failed") != 1 {}

    c[0]
}

fn editor_process_keypress() -> bool {
    match editor_read_key() {
        CTRL_Q => false,
        _ => true,
    }
}

fn enable_raw_mode() {
    let mut attr = Termios::from_fd(STDIN_FILENO).expect("tcgetattr");
    attr.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
    attr.c_oflag &= !(OPOST);
    attr.c_cflag |= CS8;
    attr.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
    attr.c_cc[VMIN] = 0;
    attr.c_cc[VTIME] = 1;
    termios::tcsetattr(STDIN_FILENO, TCSAFLUSH, &attr).expect("tcsetattr");
}

fn main() {
    let _term_reset =
        TerminalReset::new(Termios::from_fd(STDIN_FILENO).expect("tcgetattr"));
    enable_raw_mode();

    while editor_process_keypress() {}
}
