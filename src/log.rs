//! The engine's log voice, mirroring `lib/helpers/logging.sh`. `check` captures
//! the render log plain, the way `lib/check.sh` captured `sync.sh` into a file
//! where `_use_colors` is false; `sync` streams it, coloured on a terminal.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

/// Receives each line as it is written, without its newline.
pub type Sink = Box<dyn FnMut(Stream, &str)>;

#[derive(Default)]
pub struct Log {
    lines: Vec<(Stream, String)>,
    sink: Option<Sink>,
    colors: bool,
}

pub const SEPARATOR: &str = "═══════════════════════════════════════════════════════════════";

const RESET: &str = "\x1b[0m";
const BLUE: &str = "\x1b[0;34m";
const GREEN: &str = "\x1b[0;32m";
const YELLOW: &str = "\x1b[0;33m";
const RED: &str = "\x1b[0;31m";

impl Log {
    /// A log that hands every line to `sink` instead of keeping it; `colors` is
    /// `_use_colors`, decided by the caller from stdout and `NO_COLOR`.
    pub fn streaming(colors: bool, sink: Sink) -> Self {
        Self {
            lines: Vec::new(),
            sink: Some(sink),
            colors,
        }
    }

    fn tagged(&mut self, stream: Stream, color: &str, emoji: &str, tag: &str, msg: &str) {
        let line = if self.colors {
            format!("{color}{emoji} {tag}{RESET} {msg}")
        } else {
            format!("{tag} {msg}")
        };
        self.emit(stream, line);
    }

    pub fn info(&mut self, msg: &str) {
        self.tagged(Stream::Out, BLUE, "🔵", "[INFO]", msg);
    }

    pub fn success(&mut self, msg: &str) {
        self.tagged(Stream::Out, GREEN, "✅", "[SUCCESS]", msg);
    }

    pub fn warning(&mut self, msg: &str) {
        self.tagged(Stream::Out, YELLOW, "⚠\u{fe0f} ", "[WARNING]", msg);
    }

    pub fn error(&mut self, msg: &str) {
        self.tagged(Stream::Err, RED, "❌", "[ERROR]", msg);
    }

    pub fn done(&mut self, msg: &str) {
        self.tagged(Stream::Out, GREEN, "✅", "[DONE]", msg);
    }

    pub fn step(&mut self, msg: &str) {
        self.out(format!("   📁 {msg}"));
    }

    pub fn separator(&mut self) {
        self.out(SEPARATOR.to_string());
    }

    pub fn out(&mut self, line: String) {
        self.emit(Stream::Out, line);
    }

    pub fn err(&mut self, line: String) {
        self.emit(Stream::Err, line);
    }

    fn emit(&mut self, stream: Stream, line: String) {
        match &mut self.sink {
            Some(sink) => sink(stream, &line),
            None => self.lines.push((stream, line)),
        }
    }

    pub fn lines(&self) -> &[(Stream, String)] {
        &self.lines
    }

    /// The last `n` lines of both streams in the order they were written, as
    /// `tail -n` shows a `>file 2>&1` capture.
    pub fn tail(&self, n: usize) -> Vec<&str> {
        let skip = self.lines.len().saturating_sub(n);
        self.lines[skip..]
            .iter()
            .map(|(_, line)| line.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn plain_prefixes_match_logging_sh_without_a_terminal() {
        let mut log = Log::default();
        log.info("a");
        log.warning("b");
        log.error("c");
        log.step("d");
        log.success("e");
        log.done("f");
        let lines: Vec<&str> = log.lines().iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            lines,
            [
                "[INFO] a",
                "[WARNING] b",
                "[ERROR] c",
                "   📁 d",
                "[SUCCESS] e",
                "[DONE] f"
            ]
        );
        assert_eq!(log.lines()[2].0, Stream::Err);
    }

    #[test]
    fn the_separator_is_sixty_three_box_characters() {
        assert_eq!(SEPARATOR.chars().count(), 63);
    }

    #[test]
    fn tail_keeps_the_last_lines_in_write_order() {
        let mut log = Log::default();
        for i in 0..5 {
            log.out(i.to_string());
        }
        assert_eq!(log.tail(2), ["3", "4"]);
        assert_eq!(log.tail(40).len(), 5);
    }

    #[test]
    fn a_streaming_log_hands_coloured_lines_to_its_sink_in_order() {
        let seen: Rc<RefCell<Vec<(Stream, String)>>> = Rc::default();
        let sink_seen = Rc::clone(&seen);
        let mut log = Log::streaming(
            true,
            Box::new(move |stream, line| sink_seen.borrow_mut().push((stream, line.to_string()))),
        );
        log.info("Syncing Claude Code...");
        log.warning("w");
        log.error("e");
        log.done("Synced 1/1 tools");
        log.step("s");
        assert!(log.lines().is_empty());
        assert_eq!(
            *seen.borrow(),
            [
                (
                    Stream::Out,
                    "\x1b[0;34m🔵 [INFO]\x1b[0m Syncing Claude Code...".to_string()
                ),
                (
                    Stream::Out,
                    "\x1b[0;33m⚠\u{fe0f}  [WARNING]\x1b[0m w".to_string()
                ),
                (Stream::Err, "\x1b[0;31m❌ [ERROR]\x1b[0m e".to_string()),
                (
                    Stream::Out,
                    "\x1b[0;32m✅ [DONE]\x1b[0m Synced 1/1 tools".to_string()
                ),
                (Stream::Out, "   📁 s".to_string()),
            ]
        );
    }
}
