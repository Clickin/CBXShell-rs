///! Error types for CBXShell
use thiserror::Error;
use windows::core::HRESULT;

#[derive(Error, Debug)]
pub enum CbxError {
    #[error("Archive error: {0}")]
    Archive(String),

    #[error("Image processing error: {0}")]
    Image(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Windows error: {0}")]
    Windows(#[from] windows::core::Error),

    #[error("Registry error: {0}")]
    Registry(String),

    #[error("No image found in archive")]
    NoImageFound,

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("Invalid file path")]
    InvalidPath,
}

impl From<CbxError> for HRESULT {
    fn from(err: CbxError) -> HRESULT {
        match err {
            CbxError::NoImageFound => windows::Win32::Foundation::E_FAIL,
            CbxError::InvalidPath => windows::Win32::Foundation::E_INVALIDARG,
            CbxError::Windows(e) => e.code(),
            _ => windows::Win32::Foundation::E_FAIL,
        }
    }
}

pub type Result<T> = std::result::Result<T, CbxError>;
