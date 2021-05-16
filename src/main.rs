use std::io::{self, Read};
use std::os::unix::io::RawFd;

use termios::{self, ECHO, ICANON, ICRNL, IEXTEN, ISIG, IXON};
use termios::{Termios, TCSAFLUSH};

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
        let _ignore =
            termios::tcsetattr(STDIN_FILENO, TCSAFLUSH, &self.original);
    }
}

const STDIN_FILENO: RawFd = 0;
// const STDOUT_FILENO: RawFd = 1;
// const STDERR_FILENO: RawFd = 2;

fn enable_raw_mode() {
    let mut attr = Termios::from_fd(STDIN_FILENO).unwrap();
    attr.c_iflag &= !(ICRNL | IXON);
    attr.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
    termios::tcsetattr(STDIN_FILENO, TCSAFLUSH, &attr).unwrap();
}

fn main() {
    let mut c = [0; 1];
    let _term_reset =
        TerminalReset::new(Termios::from_fd(STDIN_FILENO).unwrap());
    enable_raw_mode();

    while io::stdin().read_exact(&mut c).is_ok() {
        match c[0] as char {
            'q' => break,
            ch if ch.is_control() => println!("{}", c[0]),
            ch => println!("{} ('{}')", c[0], ch),
        }
    }
}
