pub const UDO_FILE_NAME: &str = ".udo.toml";

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

pub mod cli;
pub mod persist;
pub mod tui;
pub mod model;
