//! Printing a command's result: as text (`Display`) or, with `--json`, as
//! JSON (`Serialize`). Commands return values; only this file prints.

use std::fmt;

use serde::Serialize;

use crate::Res;

/// A command's result: `Display` is the text form (plain ASCII, no
/// trailing newline), `Serialize` the `--json` form.
pub trait Report: Serialize + fmt::Display {}

/// The text (`json = false`) or JSON form of `report`.
pub fn render(report: &impl Report, json: bool) -> Res<String> {
    Ok(if json {
        serde_json::to_string_pretty(report)?
    } else {
        report.to_string()
    })
}

/// Print `report` to stdout. An empty text prints nothing, not even a
/// newline (`status --short` with nothing running, for status bars).
pub fn emit(report: &impl Report, json: bool) -> Res<()> {
    let out = render(report, json)?;
    if !out.is_empty() {
        println!("{out}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Hello {
        name: String,
    }

    impl fmt::Display for Hello {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "hello {}", self.name)
        }
    }

    impl Report for Hello {}

    fn hello() -> Hello {
        Hello { name: "udo".into() }
    }

    #[test]
    fn text_is_the_display_form() {
        assert_eq!(render(&hello(), false).unwrap(), "hello udo");
    }

    #[test]
    fn json_is_the_serialized_form() {
        let json = render(&hello(), true).unwrap();

        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value, serde_json::json!({ "name": "udo" }));
    }
}
