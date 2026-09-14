//! `lib/helpers/prompts.sh`: questions go to stderr and answers come from the
//! terminal device, so captured output never swallows a prompt.

use std::io::{BufRead, BufReader, IsTerminal, Write};

/// `is_tty`: stdin and stdout are both terminals.
pub fn is_tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// `prompt_confirm`: off a terminal the default answers.
pub fn confirm(question: &str, default_yes: bool) -> bool {
    if !is_tty() {
        return default_yes;
    }
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    let mut stderr = std::io::stderr();
    let _ = write!(stderr, "{question} {hint} ");
    let _ = stderr.flush();
    let reply = read_terminal_line().unwrap_or_default();
    answer_is_yes(&reply, default_yes)
}

/// A line typed on the terminal device, without its newline; empty when none
/// can be read, as `read -r answer < /dev/tty || answer=""` leaves it.
pub fn read_terminal() -> String {
    read_terminal_line()
        .map(|line| line.strip_suffix('\n').unwrap_or(&line).to_string())
        .unwrap_or_default()
}

fn answer_is_yes(reply: &str, default_yes: bool) -> bool {
    let reply = reply.trim_matches([' ', '\t', '\n']).to_lowercase();
    match reply.as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        _ => false,
    }
}

fn read_terminal_line() -> Option<String> {
    #[cfg(windows)]
    const TERMINAL: &str = "CONIN$";
    #[cfg(not(windows))]
    const TERMINAL: &str = "/dev/tty";
    let tty = std::fs::File::open(TERMINAL).ok()?;
    let mut line = String::new();
    BufReader::new(tty).read_line(&mut line).ok()?;
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_read_like_prompt_confirm() {
        assert!(answer_is_yes("Y\n", false));
        assert!(answer_is_yes("  y \n", false));
        assert!(answer_is_yes("yes\n", false));
        assert!(!answer_is_yes("yep\n", true));
        assert!(answer_is_yes("\n", true));
        assert!(!answer_is_yes("", false));
    }
}
