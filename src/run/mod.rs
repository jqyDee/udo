//! Run configs: scripts in any language (interpreter by shebang) that open
//! or set up a node. udo knows no program; it finds the script by name
//! (`Library`) and runs it with the node as environment variables
//! (`RunContext`, `launch`). Depends on `model` only.
//!
//! - `library`: `Library`, the scripts in `run_dir`
//! - `context`: `RunContext`, what a script learns (`UDO_*`)
//! - `request`: `RunRequest`, a script found plus its context (`o` / `O`,
//!   `udo run`, `udo add`'s `on_create`)
//! - `launch`:  `launch`, run a script for a context
//! - `child`:   `wait`, `exit_code`: a child with the terminal handed over
//!   (also `udo track run`); `Stdout`: its output off a `--json` report
//! - `error`:   `RunError`

mod child;
mod context;
mod error;
mod launch;
mod library;
mod request;

pub use child::{Stdout, exit_code, wait};
pub use context::{Event, NodeInfo, NodeKind, RunContext, TaskInfo};
pub use error::RunError;
pub use launch::launch;
pub use library::Library;
pub use request::RunRequest;
