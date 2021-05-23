use std::error::Error;
use std::io;

use libc::winsize;
use libc::STDIN_FILENO;
use libc::TIOCGWINSZ;

pub fn get_window_size_ioctl() -> Result<(usize, usize), Box<dyn Error>> {
    let mut ws = winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    let ioctl_result =
        unsafe { libc::ioctl(STDIN_FILENO, TIOCGWINSZ, &mut ws) };

    if ioctl_result == -1 || ws.ws_row == 0 || ws.ws_col == 0 {
        Err(Box::new(io::Error::last_os_error()))
    } else {
        Ok((ws.ws_row as usize, ws.ws_col as usize))
    }
}
