//! Run configs: scripts in any language (interpreter by shebang) that open
//! or set up a node. udo knows no program; it finds the script by name
//! (`Library`) and hands it the node as environment variables. Depends on
//! `model` only.
//!
//! - `library`: `Library`, the scripts in `run_dir`
//! - `error`:   `RunError`

mod error;
mod library;

pub use error::RunError;
pub use library::Library;
