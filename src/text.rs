//! Byte-level string handling the Bash engine gets from its builtins: `read`
//! loops, `[[:space:]]`, `printf '%b'`, and `$(...)` newline stripping.

/// Lines as `while IFS= read -r line || [[ -n "$line" ]]` yields them: split on
/// `\n`, with a final unterminated line kept.
pub fn lines(bytes: &[u8]) -> Vec<&[u8]> {
    let mut out: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    if out.last().is_some_and(|last| last.is_empty()) {
        out.pop();
    }
    out
}

/// `sed 's/^/    /'`: every line, a final unterminated one included, gets four
/// spaces; no newline is added.
pub fn sed_indent(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        out.extend_from_slice(b"    ");
        out.extend_from_slice(line);
    }
    out
}

/// POSIX `[[:space:]]` in the C locale.
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

pub fn trim_start_space(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !is_space(*b))
        .unwrap_or(bytes.len());
    &bytes[start..]
}

pub fn trim_end_space(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|b| !is_space(*b))
        .map_or(0, |i| i + 1);
    &bytes[..end]
}

/// `rest` of `line` when it starts with `key:` followed by optional spaces,
/// as the Bash `^key:[[:space:]]*(.*)` captures it.
pub fn after_key<'a>(line: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let rest = line.strip_prefix(key.as_bytes())?.strip_prefix(b":")?;
    Some(trim_start_space(rest))
}

/// `${v#\"}; ${v%\"}; ${v#\'}; ${v%\'}`: one quote of each kind off each end.
pub fn strip_quotes(value: &[u8]) -> &[u8] {
    let mut v = value;
    for quote in *b"\"'" {
        v = v.strip_prefix(&[quote]).unwrap_or(v);
        v = v.strip_suffix(&[quote]).unwrap_or(v);
    }
    v
}

/// `$(...)` drops every trailing newline.
pub fn strip_trailing_newlines(bytes: &mut Vec<u8>) {
    while bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
}

/// `_json_escape` and `_toml_escape`: backslash, quote, `\n`, `\r`, `\t`.
pub fn json_escape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'"' => out.extend_from_slice(b"\\\""),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            other => out.push(other),
        }
    }
    out
}

/// `s` as a JSON string literal, quotes included: RFC 8259 requires every
/// control character escaped, which `json_escape` (Bash parity) does not.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Bash 3.2 `printf '%b'`. Returns the expansion and whether `\c` stopped
/// all further output.
pub fn printf_b(input: &[u8]) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] != b'\\' || i + 1 >= input.len() {
            out.push(input[i]);
            i += 1;
            continue;
        }
        let next = input[i + 1];
        i += 2;
        let simple = match next {
            b'a' => Some(0x07),
            b'b' => Some(0x08),
            b'e' | b'E' => Some(0x1b),
            b'f' => Some(0x0c),
            b'n' => Some(b'\n'),
            b'r' => Some(b'\r'),
            b't' => Some(b'\t'),
            b'v' => Some(0x0b),
            b'\\' => Some(b'\\'),
            _ => None,
        };
        if let Some(byte) = simple {
            out.push(byte);
            continue;
        }
        match next {
            b'c' => return (out, true),
            b'0'..=b'7' => {
                let (max, mut value) = if next == b'0' {
                    (3, 0u32)
                } else {
                    (2, u32::from(next - b'0'))
                };
                let mut taken = 0;
                while taken < max && i < input.len() && (b'0'..=b'7').contains(&input[i]) {
                    value = value * 8 + u32::from(input[i] - b'0');
                    i += 1;
                    taken += 1;
                }
                out.push((value & 0xff) as u8);
            }
            b'x' if i < input.len() && input[i].is_ascii_hexdigit() => {
                let mut value = 0u32;
                let mut taken = 0;
                while taken < 2 && i < input.len() && input[i].is_ascii_hexdigit() {
                    value = value * 16 + (input[i] as char).to_digit(16).unwrap_or(0);
                    i += 1;
                    taken += 1;
                }
                out.push(value as u8);
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    (out, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_string_quotes_and_escapes_every_control_character() {
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("x\n\t\u{1}"), "\"x\\n\\t\\u0001\"");
        assert_eq!(json_string("é → ~/x"), "\"é → ~/x\"");
    }

    #[test]
    fn lines_keep_an_unterminated_last_line() {
        assert_eq!(lines(b"a\nb"), [&b"a"[..], b"b"]);
        assert_eq!(lines(b"a\n\n"), [&b"a"[..], b""]);
        assert!(lines(b"").is_empty());
    }

    #[test]
    fn quotes_come_off_one_of_each_kind_per_end() {
        assert_eq!(strip_quotes(b"\"'x'\""), b"x");
        assert_eq!(strip_quotes(b"\"x"), b"x");
    }

    #[test]
    fn printf_b_expands_like_bash_3_2() {
        assert_eq!(printf_b(b"---\\nk: v\\n---").0, b"---\nk: v\n---");
        assert_eq!(printf_b(b"\\x41\\0101\\101\\e\\z").0, b"AAA\x1b\\z");
        assert_eq!(printf_b(b"\\u0041").0, b"\\u0041");
        assert_eq!(printf_b(b"a\\cb"), (b"a".to_vec(), true));
        assert_eq!(printf_b(b"\\08").0, b"\x008");
    }

    #[test]
    fn json_escape_covers_the_bash_set_only() {
        assert_eq!(
            json_escape(b"a\"b\\c\nd\te\x01"),
            b"a\\\"b\\\\c\\nd\\te\x01"
        );
    }

    #[test]
    fn sed_indent_prefixes_every_line_and_adds_no_final_newline() {
        assert_eq!(sed_indent(b"a\nb"), b"    a\n    b");
        assert_eq!(sed_indent(b"a\n"), b"    a\n");
        assert_eq!(sed_indent(b"\n"), b"    \n");
        assert_eq!(sed_indent(b""), b"");
    }
}
