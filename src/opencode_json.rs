//! `opencode.json` composition: settings plus the canonical MCP source,
//! ported from the awk program in `lib/helpers/opencode.sh`. The first
//! failure wins and later parsing continues, exactly as awk's `fail()` did.

#[derive(Debug, PartialEq, Eq)]
pub struct ComposeError {
    pub code: u8,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Settings,
    Canonical,
    Server,
}

#[derive(Default)]
struct Members {
    keys: Vec<String>,
    raw_keys: Vec<String>,
    values: Vec<String>,
}

struct Parser {
    json: Vec<char>,
    pos: usize,
    active_error: u8,
    error: Option<(u8, String)>,
    last_string: String,
    settings: Members,
    canonical: Members,
    server: Members,
}

/// awk's `getline` loop: every line, the last one included, ends in `\n`.
fn read_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 1);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

fn json_kind(raw: &str) -> &'static str {
    let trimmed = raw.trim_start_matches(is_space);
    match trimmed.chars().next() {
        Some('"') => "string",
        Some('{') => "object",
        Some('[') => "array",
        _ => {
            let word = trimmed.trim_end_matches(is_space);
            if word == "true" || word == "false" {
                "boolean"
            } else if word == "null" {
                "null"
            } else {
                "number"
            }
        }
    }
}

fn is_word_char(c: Option<&char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
}

fn is_timeout(value: &str) -> bool {
    let v: Vec<char> = value.trim_matches(is_space).chars().collect();
    let mut i = 0;
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < v.len() && v[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    if !digits(&mut i) {
        return false;
    }
    if i < v.len() && v[i] == '.' {
        i += 1;
        if !digits(&mut i) {
            return false;
        }
    }
    if i < v.len() && (v[i] == 'e' || v[i] == 'E') {
        i += 1;
        if i < v.len() && (v[i] == '+' || v[i] == '-') {
            i += 1;
        }
        if !digits(&mut i) {
            return false;
        }
    }
    i == v.len()
}

impl Parser {
    fn new() -> Self {
        Self {
            json: Vec::new(),
            pos: 0,
            active_error: 0,
            error: None,
            last_string: String::new(),
            settings: Members::default(),
            canonical: Members::default(),
            server: Members::default(),
        }
    }

    fn fail(&mut self, code: u8, message: impl Into<String>) -> bool {
        if self.error.is_none() {
            self.error = Some((code, message.into()));
        }
        false
    }

    fn peek(&self) -> Option<char> {
        self.json.get(self.pos).copied()
    }

    fn slice(&self, start: usize) -> String {
        self.json[start..self.pos.min(self.json.len())]
            .iter()
            .collect()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(is_space) {
            self.pos += 1;
        }
    }

    fn parse_string(&mut self) -> bool {
        let code = self.active_error;
        if self.peek() != Some('"') {
            return self.fail(code, "expected a JSON string");
        }
        self.pos += 1;
        let mut decoded = String::new();
        while let Some(c) = self.peek() {
            if c == '"' {
                self.pos += 1;
                self.last_string = decoded;
                return true;
            }
            if c == '\\' {
                self.pos += 1;
                let Some(escape) = self.peek() else {
                    return self.fail(code, "unterminated JSON escape");
                };
                if escape == 'u' {
                    let hex: String = self.json.iter().skip(self.pos + 1).take(4).collect();
                    if hex.chars().count() != 4 || !hex.chars().all(|h| h.is_ascii_hexdigit()) {
                        return self.fail(code, "invalid Unicode escape");
                    }
                    let value = u32::from_str_radix(&hex, 16).unwrap_or(0);
                    if value <= 127 {
                        decoded.push(char::from_u32(value).unwrap_or('\0'));
                    } else {
                        decoded.push_str("\\u");
                        decoded.push_str(&hex);
                    }
                    self.pos += 5;
                    continue;
                }
                let mapped = match escape {
                    'b' => '\x08',
                    'f' => '\x0c',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' | '\\' | '/' => escape,
                    _ => return self.fail(code, "invalid JSON escape"),
                };
                decoded.push(mapped);
                self.pos += 1;
                continue;
            }
            if c.is_ascii_control() {
                return self.fail(code, "control character in JSON string");
            }
            decoded.push(c);
            self.pos += 1;
        }
        self.fail(code, "unterminated JSON string")
    }

    fn parse_array(&mut self) -> bool {
        let code = self.active_error;
        self.pos += 1;
        self.skip_space();
        if self.peek() == Some(']') {
            self.pos += 1;
            return true;
        }
        while self.pos < self.json.len() {
            if !self.parse_value() {
                return false;
            }
            self.skip_space();
            match self.peek() {
                Some(']') => {
                    self.pos += 1;
                    return true;
                }
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                }
                _ => return self.fail(code, "expected comma in JSON array"),
            }
        }
        self.fail(code, "unterminated JSON array")
    }

    fn parse_object(&mut self) -> bool {
        let code = self.active_error;
        self.pos += 1;
        self.skip_space();
        if self.peek() == Some('}') {
            self.pos += 1;
            return true;
        }
        while self.pos < self.json.len() {
            if !self.parse_string() {
                return false;
            }
            self.skip_space();
            if self.peek() != Some(':') {
                return self.fail(code, "expected colon in JSON object");
            }
            self.pos += 1;
            self.skip_space();
            if !self.parse_value() {
                return false;
            }
            self.skip_space();
            match self.peek() {
                Some('}') => {
                    self.pos += 1;
                    return true;
                }
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                }
                _ => return self.fail(code, "expected comma in JSON object"),
            }
        }
        self.fail(code, "unterminated JSON object")
    }

    fn parse_value(&mut self) -> bool {
        self.skip_space();
        match self.peek() {
            Some('"') => return self.parse_string(),
            Some('{') => return self.parse_object(),
            Some('[') => return self.parse_array(),
            _ => {}
        }
        for word in ["true", "false", "null"] {
            let len = word.len();
            let matches = self
                .json
                .get(self.pos..self.pos + len)
                .is_some_and(|s| s.iter().copied().eq(word.chars()));
            if matches && !is_word_char(self.json.get(self.pos + len)) {
                self.pos += len;
                return true;
            }
        }
        if let Some(len) = self.number_len() {
            self.pos += len;
            return true;
        }
        let code = self.active_error;
        self.fail(code, "invalid JSON value")
    }

    /// Length of `^-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?` at the cursor.
    fn number_len(&self) -> Option<usize> {
        let s = &self.json[self.pos..];
        let digit = |i: usize| s.get(i).is_some_and(|c| c.is_ascii_digit());
        let mut i = usize::from(s.first() == Some(&'-'));
        match s.get(i) {
            Some('0') => i += 1,
            Some(c) if c.is_ascii_digit() => {
                while digit(i) {
                    i += 1;
                }
            }
            _ => return None,
        }
        if s.get(i) == Some(&'.') && digit(i + 1) {
            i += 1;
            while digit(i) {
                i += 1;
            }
        }
        if matches!(s.get(i), Some('e' | 'E')) {
            let mut j = i + 1;
            if matches!(s.get(j), Some('+' | '-')) {
                j += 1;
            }
            if digit(j) {
                while digit(j) {
                    j += 1;
                }
                i = j;
            }
        }
        Some(i)
    }

    fn store_member(&mut self, mode: Mode, key: String, raw_key: String, raw_value: String) {
        let active = self.active_error;
        let (members, code, label) = match mode {
            Mode::Settings => (&self.settings, 20, "duplicate settings field"),
            Mode::Canonical => (&self.canonical, active, "duplicate canonical MCP field"),
            Mode::Server => (&self.server, 23, "duplicate server field"),
        };
        if members.keys.contains(&key) {
            self.fail(code, format!("{label} '{key}'"));
            return;
        }
        let members = match mode {
            Mode::Settings => &mut self.settings,
            Mode::Canonical => &mut self.canonical,
            Mode::Server => &mut self.server,
        };
        members.keys.push(key);
        members.raw_keys.push(raw_key);
        members.values.push(raw_value);
    }

    fn walk_root(&mut self, text: &str, mode: Mode, code: u8) -> bool {
        self.json = text.chars().collect();
        self.pos = 0;
        self.active_error = code;
        self.skip_space();
        if self.peek() != Some('{') {
            return self.fail(code, "root value must be an object");
        }
        self.pos += 1;
        self.skip_space();
        if self.peek() == Some('}') {
            self.pos += 1;
            self.skip_space();
            return self.pos >= self.json.len();
        }
        while self.pos < self.json.len() {
            let key_start = self.pos;
            if !self.parse_string() {
                return false;
            }
            let key = self.last_string.clone();
            let raw_key = self.slice(key_start);
            self.skip_space();
            if self.peek() != Some(':') {
                return self.fail(code, "expected colon after object key");
            }
            self.pos += 1;
            self.skip_space();
            let value_start = self.pos;
            if !self.parse_value() {
                return false;
            }
            let raw_value = self.slice(value_start);
            self.store_member(mode, key, raw_key, raw_value);
            self.skip_space();
            match self.peek() {
                Some('}') => {
                    self.pos += 1;
                    self.skip_space();
                    if self.pos < self.json.len() {
                        return self.fail(code, "trailing content after JSON object");
                    }
                    return true;
                }
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                }
                _ => return self.fail(code, "expected comma in JSON object"),
            }
        }
        self.fail(code, "unterminated JSON object")
    }

    fn decode_string(&mut self, raw: &str) -> String {
        let saved = (std::mem::take(&mut self.json), self.pos, self.active_error);
        self.json = raw.chars().collect();
        self.pos = 0;
        self.active_error = 23;
        let result = if self.parse_string() {
            self.last_string.clone()
        } else {
            String::new()
        };
        (self.json, self.pos, self.active_error) = saved;
        result
    }

    fn validate_string_array(&mut self, raw: &str, server: &str, field: &str) -> bool {
        self.json = raw.chars().collect();
        self.pos = 1;
        self.active_error = 23;
        self.skip_space();
        if self.peek() == Some(']') {
            return true;
        }
        let invalid = format!("server '{server}' field '{field}' is invalid");
        while self.pos < self.json.len() {
            let start = self.pos;
            if !self.parse_value() {
                return false;
            }
            if json_kind(&self.slice(start)) != "string" {
                return self.fail(
                    23,
                    format!("server '{server}' field '{field}' must contain only strings"),
                );
            }
            self.skip_space();
            match self.peek() {
                Some(']') => return true,
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                }
                _ => return self.fail(23, invalid),
            }
        }
        self.fail(23, invalid)
    }

    fn validate_string_map(&mut self, raw: &str, server: &str, field: &str) -> bool {
        self.json = raw.chars().collect();
        self.pos = 1;
        self.active_error = 23;
        self.skip_space();
        if self.peek() == Some('}') {
            return true;
        }
        let invalid = format!("server '{server}' field '{field}' is invalid");
        while self.pos < self.json.len() {
            if !self.parse_string() {
                return false;
            }
            self.skip_space();
            if self.peek() != Some(':') {
                return self.fail(23, invalid);
            }
            self.pos += 1;
            self.skip_space();
            let value_start = self.pos;
            if !self.parse_value() {
                return false;
            }
            if json_kind(&self.slice(value_start)) != "string" {
                return self.fail(
                    23,
                    format!("server '{server}' field '{field}' values must be strings"),
                );
            }
            self.skip_space();
            match self.peek() {
                Some('}') => return true,
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                }
                _ => return self.fail(23, invalid),
            }
        }
        self.fail(23, invalid)
    }

    fn convert_server(&mut self, name: &str, raw_value: &str) -> Option<String> {
        self.server = Members::default();
        if !self.walk_root(raw_value, Mode::Server, 23) {
            return None;
        }
        let fields: Vec<(String, String)> = self
            .server
            .keys
            .iter()
            .cloned()
            .zip(self.server.values.iter().cloned())
            .collect();
        let mut command = String::new();
        let mut url = String::new();
        let mut type_ = String::new();
        let mut args = String::new();
        let mut env = String::new();
        let mut headers = String::new();
        let mut enabled = String::new();
        let mut timeout = String::new();
        let mut oauth = String::new();
        let must =
            |field: &str, what: &str| format!("server '{name}' field '{field}' must be {what}");
        for (field, value) in fields {
            let kind = json_kind(&value);
            match field.as_str() {
                "command" | "url" => {
                    if kind != "string" {
                        self.fail(23, must(&field, "a string"));
                        return None;
                    }
                    if field == "command" {
                        command = value;
                    } else {
                        url = value;
                    }
                }
                "args" => {
                    if kind != "array" {
                        self.fail(23, must("args", "an array"));
                        return None;
                    }
                    if !self.validate_string_array(&value, name, "args") {
                        return None;
                    }
                    args = value;
                }
                "env" | "headers" => {
                    if kind != "object" {
                        self.fail(23, must(&field, "an object"));
                        return None;
                    }
                    if !self.validate_string_map(&value, name, &field) {
                        return None;
                    }
                    if field == "env" {
                        env = value;
                    } else {
                        headers = value;
                    }
                }
                "type" => {
                    if kind != "string" {
                        self.fail(23, must("type", "a string"));
                        return None;
                    }
                    type_ = self.decode_string(&value);
                }
                "enabled" => {
                    if kind != "boolean" {
                        self.fail(23, must("enabled", "a boolean"));
                        return None;
                    }
                    enabled = value;
                }
                "timeout" => {
                    if kind != "number" || !is_timeout(&value) {
                        self.fail(23, must("timeout", "a non-negative number"));
                        return None;
                    }
                    timeout = value;
                }
                "oauth" => {
                    if kind != "boolean" && kind != "object" {
                        self.fail(23, must("oauth", "a boolean or object"));
                        return None;
                    }
                    oauth = value;
                }
                other => {
                    self.fail(
                        25,
                        format!("server '{name}' has unsupported field '{other}'"),
                    );
                    return None;
                }
            }
        }
        if command.is_empty() == url.is_empty() {
            self.fail(
                26,
                format!("server '{name}' must define exactly one transport: command or url"),
            );
            return None;
        }
        let mut props: Vec<String> = Vec::new();
        if !command.is_empty() {
            if !headers.is_empty() || !oauth.is_empty() {
                self.fail(25, format!("server '{name}' contains remote-only fields"));
                return None;
            }
            if !type_.is_empty() && type_ != "stdio" {
                self.fail(
                    23,
                    format!("server '{name}' field 'type' must be stdio for a local server"),
                );
                return None;
            }
            props.push("\"type\": \"local\"".to_string());
            let inner = array_inner(&args);
            let value = if inner.is_empty() {
                format!("[{command}]")
            } else {
                format!("[{command}, {inner}]")
            };
            props.push(format!("\"command\": {value}"));
            if !env.is_empty() {
                props.push(format!("\"environment\": {env}"));
            }
        } else {
            if !args.is_empty() || !env.is_empty() {
                self.fail(25, format!("server '{name}' contains local-only fields"));
                return None;
            }
            if !type_.is_empty() && !matches!(type_.as_str(), "http" | "sse" | "streamable-http") {
                self.fail(
                    23,
                    format!("server '{name}' field 'type' is not a supported remote transport"),
                );
                return None;
            }
            props.push("\"type\": \"remote\"".to_string());
            props.push(format!("\"url\": {url}"));
            if !headers.is_empty() {
                props.push(format!("\"headers\": {headers}"));
            }
            if !oauth.is_empty() {
                props.push(format!("\"oauth\": {oauth}"));
            }
        }
        if !enabled.is_empty() {
            props.push(format!("\"enabled\": {enabled}"));
        }
        if !timeout.is_empty() {
            props.push(format!("\"timeout\": {timeout}"));
        }
        Some(format!("{{{}}}", props.join(", ")))
    }

    fn take_error(&mut self) -> Option<ComposeError> {
        self.error
            .take()
            .map(|(code, message)| ComposeError { code, message })
    }
}

/// awk `sub(/^[[:space:]]*\[[[:space:]]*/)` and `sub(/[[:space:]]*\][[:space:]]*$/)`.
fn array_inner(raw: &str) -> String {
    let mut value = raw;
    let lead = value.trim_start_matches(is_space);
    if let Some(rest) = lead.strip_prefix('[') {
        value = rest.trim_start_matches(is_space);
    }
    let tail = value.trim_end_matches(is_space);
    if let Some(rest) = tail.strip_suffix(']') {
        value = rest.trim_end_matches(is_space);
    }
    value.to_string()
}

/// The composed `opencode.json` bytes, or the exit code and diagnostic line
/// `_opencode_compose_json` reported.
pub fn compose(settings: &str, mcp: &str) -> Result<String, ComposeError> {
    let mut p = Parser::new();
    let settings_text = read_text(settings);
    if !p.walk_root(&settings_text, Mode::Settings, 20) && p.error.is_none() {
        p.fail(20, "malformed settings JSON");
    }
    if let Some(error) = p.take_error() {
        return Err(error);
    }
    let settings_has_mcp = p.settings.keys.iter().any(|k| k == "mcp");

    let mcp_text = read_text(mcp);
    if !p.walk_root(&mcp_text, Mode::Canonical, 21) && p.error.is_none() {
        p.fail(21, "malformed canonical MCP JSON");
    }
    if let Some(error) = p.take_error() {
        return Err(error);
    }
    if settings_has_mcp {
        return Err(ComposeError {
            code: 24,
            message: "settings and canonical source both define OpenCode MCP ownership".into(),
        });
    }

    let mut mcp_servers = String::new();
    for (key, value) in p.canonical.keys.iter().zip(&p.canonical.values) {
        if key == "mcpServers" {
            mcp_servers = value.clone();
        } else {
            return Err(ComposeError {
                code: 25,
                message: format!("canonical MCP has unsupported top-level field '{key}'"),
            });
        }
    }
    if mcp_servers.is_empty() || json_kind(&mcp_servers) != "object" {
        return Err(ComposeError {
            code: 22,
            message: "mcpServers must be an object".into(),
        });
    }

    p.canonical = Members::default();
    if !p.walk_root(&mcp_servers, Mode::Canonical, 23) {
        let (code, message) = p.error.take().unwrap_or((0, String::new()));
        return Err(ComposeError { code, message });
    }
    let servers: Vec<(String, String, String)> = p
        .canonical
        .keys
        .iter()
        .cloned()
        .zip(p.canonical.raw_keys.iter().cloned())
        .zip(p.canonical.values.iter().cloned())
        .map(|((k, r), v)| (k, r, v))
        .collect();
    let mut converted = Vec::with_capacity(servers.len());
    for (key, raw_key, value) in &servers {
        if json_kind(value) != "object" {
            return Err(ComposeError {
                code: 23,
                message: format!("server '{key}' must be an object"),
            });
        }
        let server = p.convert_server(key, value);
        if let Some(error) = p.take_error() {
            return Err(error);
        }
        converted.push((raw_key.clone(), server.unwrap_or_default()));
    }

    let mut out = String::from("{\n");
    for (raw_key, value) in p.settings.raw_keys.iter().zip(&p.settings.values) {
        out.push_str(&format!("  {raw_key}: {value},\n"));
    }
    out.push_str("  \"mcp\": {\n");
    let count = converted.len();
    for (i, (raw_key, server)) in converted.into_iter().enumerate() {
        let comma = if i + 1 < count { "," } else { "" };
        out.push_str(&format!("    {raw_key}: {server}{comma}\n"));
    }
    out.push_str("  }\n}\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(settings: &str, mcp: &str) -> (u8, String) {
        let e = compose(settings, mcp).unwrap_err();
        (e.code, e.message)
    }

    #[test]
    fn a_local_server_is_composed_after_the_settings_members() {
        let out = compose(
            "{\"$schema\":\"https://opencode.ai/config.json\",\"theme\":\"system\"}\n",
            "{\"mcpServers\":{\"github\":{\"command\":\"npx\",\"args\":[\"-y\",\"@github/mcp\"],\"env\":{\"TOKEN\":\"${GITHUB_TOKEN}\"}}}}\n",
        )
        .unwrap();
        assert_eq!(
            out,
            "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"theme\": \"system\",\n  \"mcp\": {\n    \"github\": {\"type\": \"local\", \"command\": [\"npx\", \"-y\",\"@github/mcp\"], \"environment\": {\"TOKEN\":\"${GITHUB_TOKEN}\"}}\n  }\n}\n"
        );
    }

    #[test]
    fn a_remote_server_keeps_its_options() {
        let out = compose(
            "{}",
            "{\"mcpServers\":{\"r\":{\"url\":\"https://x\",\"type\":\"sse\",\"headers\":{},\"oauth\":false,\"enabled\":true,\"timeout\":5}}}",
        )
        .unwrap();
        assert_eq!(
            out,
            "{\n  \"mcp\": {\n    \"r\": {\"type\": \"remote\", \"url\": \"https://x\", \"headers\": {}, \"oauth\": false, \"enabled\": true, \"timeout\": 5}\n  }\n}\n"
        );
    }

    #[test]
    fn failures_carry_the_awk_exit_codes() {
        assert_eq!(err("[]", "{}"), (20, "root value must be an object".into()));
        assert_eq!(
            err("{\"a\":1,\"a\":2}", "{}"),
            (20, "duplicate settings field 'a'".into())
        );
        assert_eq!(
            err("{}", "{\"mcpServers\":[]}"),
            (22, "mcpServers must be an object".into())
        );
        assert_eq!(
            err("{}", "{\"mcpServers\":{\"s\":1}}"),
            (23, "server 's' must be an object".into())
        );
        assert_eq!(
            err("{\"mcp\":{}}", "{\"mcpServers\":{}}"),
            (
                24,
                "settings and canonical source both define OpenCode MCP ownership".into()
            )
        );
        assert_eq!(
            err("{}", "{\"x\":1}"),
            (
                25,
                "canonical MCP has unsupported top-level field 'x'".into()
            )
        );
        assert_eq!(
            err(
                "{}",
                "{\"mcpServers\":{\"s\":{\"command\":\"a\",\"url\":\"b\"}}}"
            ),
            (
                26,
                "server 's' must define exactly one transport: command or url".into()
            )
        );
        assert_eq!(
            err(
                "{}",
                "{\"mcpServers\":{\"s\":{\"command\":\"a\",\"cwd\":\"b\"}}}"
            ),
            (25, "server 's' has unsupported field 'cwd'".into())
        );
        assert_eq!(
            err(
                "{}",
                "{\"mcpServers\":{\"s\":{\"command\":\"a\",\"args\":[1]}}}"
            ),
            (
                23,
                "server 's' field 'args' must contain only strings".into()
            )
        );
        assert_eq!(err("{} x", "{}"), (20, "malformed settings JSON".into()));
        assert_eq!(
            err("{\"a\":01}", "{}"),
            (20, "expected comma in JSON object".into())
        );
    }

    #[test]
    fn unicode_escapes_above_ascii_stay_escaped_in_decoded_keys() {
        let out = compose("{\"k\\u00e9\":\"\\u0041\"}", "{\"mcpServers\":{}}").unwrap();
        assert_eq!(
            out,
            "{\n  \"k\\u00e9\": \"\\u0041\",\n  \"mcp\": {\n  }\n}\n"
        );
    }
}
