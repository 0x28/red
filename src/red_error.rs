use std::{error::Error, fmt::Display};

#[derive(Debug)]
pub enum EditorError {
    ParseGetCursorResponse,
}

impl Error for EditorError {}

impl Display for EditorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditorError::ParseGetCursorResponse => {
                write!(f, "Failed to parse cursor position response")
            }
        }
    }
}
