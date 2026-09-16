//! `lib/helpers/prompts.sh`: questions go to stderr and answers come from the
//! terminal device, so captured output never swallows a prompt.

use std::io::{BufRead, BufReader, IsTerminal, Read, Write};
use std::process::{Command, Stdio};

use crate::style::Style;

#[cfg(windows)]
const TERMINAL: &str = "CONIN$";
#[cfg(not(windows))]
const TERMINAL: &str = "/dev/tty";

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
    let tty = std::fs::File::open(TERMINAL).ok()?;
    let mut line = String::new();
    BufReader::new(tty).read_line(&mut line).ok()?;
    Some(line)
}

/// A key `prompt_multiselect` reacts to, as `read -rsn1` delivers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Toggle,
    All,
    Clear,
    Enter,
    Cancel,
    Other,
}

/// The list was cancelled with `q` or Escape; the preselected items stand.
#[derive(Debug, PartialEq, Eq)]
pub struct Cancelled(pub Vec<String>);

/// `prompt_multiselect`'s list: what is selected, where the cursor is, and
/// whether a frame is already on screen to be overwritten.
pub struct Multiselect {
    title: String,
    options: Vec<String>,
    selected: Vec<bool>,
    preselected: Vec<String>,
    cursor: usize,
    drawn: bool,
}

impl Multiselect {
    pub fn new(title: &str, options: &[String], preselected: &[String]) -> Self {
        let selected = options
            .iter()
            .map(|option| preselected.iter().any(|p| p == option))
            .collect();
        Self {
            title: title.to_string(),
            options: options.to_vec(),
            selected,
            preselected: preselected.to_vec(),
            cursor: 0,
            drawn: false,
        }
    }

    /// `_redraw`: the next frame, drawn over the previous one after the first.
    pub fn frame(&mut self, style: &Style) -> String {
        let mut out = String::new();
        if self.drawn {
            out.push_str(&format!("\x1b[{}A", self.options.len() + 2));
        }
        self.drawn = true;
        out.push_str(&format!("\r\x1b[K{}\n", self.title));
        out.push_str(&format!(
            "\r\x1b[K{}\n",
            style.dim("  (space: toggle · a: all · n: none · enter: confirm)")
        ));
        for (i, option) in self.options.iter().enumerate() {
            let check = if self.selected[i] {
                format!("[{}]", style.green("x"))
            } else {
                "[ ]".to_string()
            };
            let (marker, name) = if i == self.cursor {
                (style.cyan("›"), style.bold(option))
            } else {
                (" ".to_string(), option.clone())
            };
            out.push_str(&format!("\r\x1b[K {marker} {check} {name}\n"));
        }
        out
    }

    /// One key; `Some` when the list is confirmed or cancelled.
    pub fn press(&mut self, key: Key) -> Option<Result<Vec<String>, Cancelled>> {
        let n = self.options.len();
        match key {
            Key::Up => self.cursor = (self.cursor + n - 1) % n,
            Key::Down => self.cursor = (self.cursor + 1) % n,
            Key::Toggle => self.selected[self.cursor] = !self.selected[self.cursor],
            Key::All => self.selected.iter_mut().for_each(|s| *s = true),
            Key::Clear => self.selected.iter_mut().for_each(|s| *s = false),
            Key::Enter => return Some(Ok(self.picked())),
            Key::Cancel => return Some(Err(Cancelled(self.preselected.clone()))),
            Key::Other => {}
        }
        None
    }

    /// The selected options, in list order.
    pub fn picked(&self) -> Vec<String> {
        self.options
            .iter()
            .zip(&self.selected)
            .filter(|(_, on)| **on)
            .map(|(option, _)| option.clone())
            .collect()
    }
}

/// `prompt_multiselect`: off a terminal the preselected items come back
/// unchanged; on one the list is drawn on `err` and driven by `keys`.
pub fn multiselect(
    title: &str,
    options: &[String],
    preselected: &[String],
    interactive: bool,
    style: &Style,
    keys: &mut dyn FnMut() -> Key,
    err: &mut dyn Write,
) -> Result<Vec<String>, Cancelled> {
    if !interactive {
        return Ok(preselected.to_vec());
    }
    if options.is_empty() {
        return Ok(Vec::new());
    }
    let mut list = Multiselect::new(title, options, preselected);
    let _ = err.write_all(b"\x1b[?25l");
    let _ = err.write_all(list.frame(style).as_bytes());
    let _ = err.flush();
    loop {
        if let Some(outcome) = list.press(keys()) {
            let _ = err.write_all(b"\x1b[?25h");
            let _ = err.flush();
            return outcome;
        }
        let _ = err.write_all(list.frame(style).as_bytes());
        let _ = err.flush();
    }
}

/// `prompt_multiselect` on the terminal device, keys read one byte at a time
/// as `read -rsn1` does. The list is drawn on stderr, so the terminal test is
/// stdin and stderr, as in Bash, whose callers capture stdout.
pub fn multiselect_on_terminal(
    title: &str,
    options: &[String],
    preselected: &[String],
    style: &Style,
) -> Result<Vec<String>, Cancelled> {
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let mut terminal = interactive.then(RawTerminal::open).flatten();
    let mut keys = || match terminal.as_mut() {
        Some(terminal) => terminal.key(),
        None => Key::Enter,
    };
    multiselect(
        title,
        options,
        preselected,
        interactive,
        style,
        &mut keys,
        &mut std::io::stderr(),
    )
}

/// The terminal device with echo and line buffering off, as `read -rsn1`
/// leaves it while it waits; the saved settings come back on drop.
pub struct RawTerminal {
    tty: std::fs::File,
    saved: Option<String>,
}

impl RawTerminal {
    /// `None` when the terminal device cannot be opened, as when `read
    /// </dev/tty` fails.
    pub fn open() -> Option<Self> {
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(TERMINAL)
            .ok()?;
        let saved = stty(&tty, &["-g"]).map(|text| text.trim().to_string());
        stty(&tty, &["-icanon", "-echo", "min", "1", "time", "0"])?;
        Some(Self { tty, saved })
    }

    /// One keypress. An escape is followed by two reads of up to a second for
    /// `[A` and `[B`, the `read -t 1` Bash 3.2 allows; anything else after it,
    /// or nothing, cancels, as in Bash.
    pub fn key(&mut self) -> Key {
        let mut byte = [0u8; 1];
        match self.tty.read(&mut byte) {
            Ok(1) => {}
            _ => return Key::Enter,
        }
        match byte[0] {
            0x1b => {
                let _ = stty(&self.tty, &["min", "0", "time", "10"]);
                let mut seq = [0u8; 2];
                let mut got = 0;
                while got < 2 {
                    match self.tty.read(&mut seq[got..]) {
                        Ok(n) if n > 0 => got += n,
                        _ => break,
                    }
                }
                let _ = stty(&self.tty, &["min", "1", "time", "0"]);
                match &seq[..got] {
                    b"[A" => Key::Up,
                    b"[B" => Key::Down,
                    _ => Key::Cancel,
                }
            }
            b'k' => Key::Up,
            b'j' => Key::Down,
            b' ' => Key::Toggle,
            b'a' | b'A' => Key::All,
            b'n' | b'N' => Key::Clear,
            b'\n' | b'\r' => Key::Enter,
            b'q' => Key::Cancel,
            _ => Key::Other,
        }
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        if let Some(saved) = &self.saved {
            let _ = stty(&self.tty, &[saved.as_str()]);
        }
    }
}

/// `stty <args>` on the terminal device; its stdout on success.
fn stty(tty: &std::fs::File, args: &[&str]) -> Option<String> {
    let output = Command::new("stty")
        .args(args)
        .stdin(tty.try_clone().ok()?)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
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

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_list_draws_and_moves_like_prompt_multiselect() {
        let style = Style::plain();
        let mut list = Multiselect::new("Pick:", &strings(&["a", "b", "c"]), &strings(&["b"]));
        assert_eq!(
            list.frame(&style),
            "\r\x1b[KPick:\n\r\x1b[K  (space: toggle · a: all · n: none · enter: confirm)\n\r\x1b[K › [ ] a\n\r\x1b[K   [x] b\n\r\x1b[K   [ ] c\n"
        );
        assert_eq!(list.press(Key::Up), None);
        assert!(list.frame(&style).starts_with("\x1b[5A\r\x1b[KPick:\n"));
        assert!(list.frame(&style).ends_with("\r\x1b[K › [ ] c\n"));
        assert_eq!(list.press(Key::Toggle), None);
        assert_eq!(list.press(Key::Down), None);
        assert_eq!(list.press(Key::Other), None);
        assert_eq!(list.picked(), strings(&["b", "c"]));
        assert_eq!(list.press(Key::Toggle), None);
        assert_eq!(list.press(Key::Enter), Some(Ok(strings(&["a", "b", "c"]))));
        assert_eq!(list.press(Key::Clear), None);
        assert_eq!(list.press(Key::Enter), Some(Ok(Vec::new())));
        assert_eq!(list.press(Key::All), None);
        assert_eq!(
            list.press(Key::Cancel),
            Some(Err(Cancelled(strings(&["b"]))))
        );
    }

    #[test]
    fn the_prompt_hides_the_cursor_reads_keys_and_returns_preselected_off_a_terminal() {
        let style = Style::plain();
        let options = strings(&["x", "y"]);
        let mut err = Vec::new();
        let mut never = || panic!("no key is read off a terminal");
        assert_eq!(
            multiselect(
                "T",
                &options,
                &strings(&["y"]),
                false,
                &style,
                &mut never,
                &mut err
            ),
            Ok(strings(&["y"]))
        );
        assert!(err.is_empty());

        let mut keys = [Key::Toggle, Key::Enter].into_iter();
        let mut next = || keys.next().unwrap();
        assert_eq!(
            multiselect("T", &options, &[], true, &style, &mut next, &mut err),
            Ok(strings(&["x"]))
        );
        let shown = String::from_utf8(err).unwrap();
        assert!(shown.starts_with("\x1b[?25l\r\x1b[KT\n"));
        assert!(shown.contains("\x1b[4A\r\x1b[KT\n"));
        assert!(shown.ends_with(" › [x] x\n\r\x1b[K   [ ] y\n\x1b[?25h"));
        assert_eq!(
            multiselect("T", &[], &[], true, &style, &mut never, &mut Vec::new()),
            Ok(Vec::new())
        );
    }
}
