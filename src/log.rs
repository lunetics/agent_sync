//! The engine's log voice, mirroring `lib/helpers/logging.sh` without colour:
//! `check` captures the render log the way `lib/check.sh` captured `sync.sh`
//! into a file, where `_use_colors` is false.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

#[derive(Debug, Default)]
pub struct Log {
    lines: Vec<(Stream, String)>,
}

pub const SEPARATOR: &str = "═══════════════════════════════════════════════════════════════";

impl Log {
    pub fn info(&mut self, msg: &str) {
        self.out(format!("[INFO] {msg}"));
    }

    pub fn success(&mut self, msg: &str) {
        self.out(format!("[SUCCESS] {msg}"));
    }

    pub fn warning(&mut self, msg: &str) {
        self.out(format!("[WARNING] {msg}"));
    }

    pub fn error(&mut self, msg: &str) {
        self.err(format!("[ERROR] {msg}"));
    }

    pub fn step(&mut self, msg: &str) {
        self.out(format!("   📁 {msg}"));
    }

    pub fn separator(&mut self) {
        self.out(SEPARATOR.to_string());
    }

    pub fn out(&mut self, line: String) {
        self.lines.push((Stream::Out, line));
    }

    pub fn err(&mut self, line: String) {
        self.lines.push((Stream::Err, line));
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

    #[test]
    fn plain_prefixes_match_logging_sh_without_a_terminal() {
        let mut log = Log::default();
        log.info("a");
        log.warning("b");
        log.error("c");
        log.step("d");
        log.success("e");
        let lines: Vec<&str> = log.lines().iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            lines,
            [
                "[INFO] a",
                "[WARNING] b",
                "[ERROR] c",
                "   📁 d",
                "[SUCCESS] e"
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
}
