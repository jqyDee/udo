//! Asking before a command removes something (`rm`, `session rm`).

use std::io::{BufRead, IsTerminal, Write};

use crate::Res;

/// Ask `question` on the terminal (`[y/N]`, on stderr so stdout keeps only
/// results). Without a terminal there is nobody to ask: `--yes` is needed.
pub fn ask_on_terminal(question: &str) -> Res<bool> {
    if !std::io::stdin().is_terminal() {
        return Err("not a terminal: add --yes to confirm".into());
    }
    eprint!("{question} [y/N] ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}
