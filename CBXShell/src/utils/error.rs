//! Error types for CBXShell
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CbxError {
    #[error("Archive error: {0}")]
    Archive(String),

    #[error("Image processing error: {0}")]
    Image(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Windows error: {0}")]
    Windows(String),

    #[error("Registry error: {0}")]
    Registry(String),

    #[error("No image found in archive")]
    NoImageFound,

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("Invalid file path")]
    InvalidPath,
}

pub type Result<T> = std::result::Result<T, CbxError>;
