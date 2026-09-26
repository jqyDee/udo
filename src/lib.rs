/// File name of the udo data
pub const UDO_FILE_NAME: &str = ".udo.toml";

/// Env var that overrides the root dir (test data, throwaway setups).
pub const ROOT_ENV: &str = "UDO_ROOT";

/// How due dates are typed and shown (CLI `--due`, TUI). Fixed width and
/// ASCII only, so byte ranges == char ranges (see `tui::form::Segment`).
pub const DATE_FMT: &str = "%Y-%m-%d %H:%M";

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

pub mod cli;
pub mod dir;
pub mod model;
pub mod naming;
pub mod persist;
pub mod tui;

#[cfg(test)]
mod test_util;
