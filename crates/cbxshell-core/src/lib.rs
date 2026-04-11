#[path = "../../../CBXShell/src/utils/debug_log.rs"]
pub mod debug_log;

#[path = "../../../CBXShell/src/utils/error.rs"]
pub mod error;

pub mod utils {
    pub use super::debug_log;
    pub use super::error;
}

pub use error::{CbxError, Result};
