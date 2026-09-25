pub const UDO_FILE_NAME: &str = ".udo.toml";

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

pub mod cli;
pub mod model;
pub mod persist;
pub mod tui;

#[cfg(test)]
mod test_util;
