//! The JSON validator `doctor` applies to `.json` payload overrides.

/// `_doctor_validate_json` as `python3 -c 'json.load(...)'` answers it: RFC
/// 8259 JSON, any top-level value, plus `NaN`, `Infinity`, and `-Infinity`.
pub(super) fn json_valid(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut p = Json {
        s: text.as_bytes(),
        pos: 0,
    };
    p.ws();
    if !p.value() {
        return false;
    }
    p.ws();
    p.pos == p.s.len()
}

struct Json<'a> {
    s: &'a [u8],
    pos: usize,
}

impl Json<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn literal(&mut self, word: &str) -> bool {
        if self.s[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> bool {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string(),
            Some(b't') => self.literal("true"),
            Some(b'f') => self.literal("false"),
            Some(b'n') => self.literal("null"),
            Some(b'N') => self.literal("NaN"),
            Some(b'I') => self.literal("Infinity"),
            Some(b'-') if self.s[self.pos..].starts_with(b"-Infinity") => self.literal("-Infinity"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => false,
        }
    }

    fn object(&mut self) -> bool {
        self.pos += 1;
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return true;
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') || !self.string() {
                return false;
            }
            self.ws();
            if self.peek() != Some(b':') {
                return false;
            }
            self.pos += 1;
            self.ws();
            if !self.value() {
                return false;
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return true;
                }
                _ => return false,
            }
        }
    }

    fn array(&mut self) -> bool {
        self.pos += 1;
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return true;
        }
        loop {
            self.ws();
            if !self.value() {
                return false;
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return true;
                }
                _ => return false,
            }
        }
    }

    fn string(&mut self) -> bool {
        self.pos += 1;
        loop {
            match self.peek() {
                None => return false,
                Some(b'"') => {
                    self.pos += 1;
                    return true;
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.pos += 1;
                        }
                        Some(b'u') => {
                            self.pos += 1;
                            for _ in 0..4 {
                                if !self.peek().is_some_and(|b| b.is_ascii_hexdigit()) {
                                    return false;
                                }
                                self.pos += 1;
                            }
                        }
                        _ => return false,
                    }
                }
                Some(b) if b < 0x20 => return false,
                Some(_) => self.pos += 1,
            }
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> bool {
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return false,
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return false;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_judged_like_python_json_load() {
        for text in [
            r#"{"a": 1}"#,
            "[1, 2.5e3, -0, \"\u{e9}\", true, null]",
            "NaN",
            "-Infinity",
            " \"x\" \n",
            "{}",
            "[]",
            "-0",
            "{\r\n\"a\":\r\n1}\r\n",
        ] {
            assert!(json_valid(text.as_bytes()), "{text:?} should be valid");
        }
        for text in [
            r#"{"a": 1,}"#,
            "",
            "\u{feff}{}",
            "[1,]",
            "{'a': 1}",
            "01",
            "1.",
            "\"tab\tinside\"",
            "{} x",
            "\0",
            "+1",
            ".5",
        ] {
            assert!(!json_valid(text.as_bytes()), "{text:?} should be invalid");
        }
    }
}
