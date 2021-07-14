use std::{error::Error, fmt::Display};

#[derive(Debug)]
pub enum EditorError {
    ParseGetCursorResponse,
    InvalidUtf8Input,
}

impl Error for EditorError {}

impl Display for EditorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditorError::ParseGetCursorResponse => {
                write!(f, "Failed to parse cursor position response")
            }
            EditorError::InvalidUtf8Input => {
                write!(f, "Encountered invalid UTF-8 input")
            }
        }
    }
}
