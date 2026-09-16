//! `agentsync doctor`: `cmd_doctor` of `lib/helpers/doctor.sh`, which checks
//! a project's layout, tools, overrides, sources, drift, secrets, skills,
//! rules, tool outputs, and parent duplicates, and exits 0, 1, or 2.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::customize::put;
use crate::manifest::{self, Manifest};
use crate::paths::{self, ExplicitSource, Paths};
use crate::payload::{self, Source};
use crate::project::Project;
use crate::style::Style;
use crate::tool::Tool;
use crate::{
    Error, catalog, convert, edit_paths, format_rev, opencode_json, overlay, template_manifest,
    yaml_subset,
};

/// What `doctor` takes from the process.
pub struct Env<'a> {
    pub version: &'a str,
    pub external_roots: Option<String>,
}

/// `_DOCTOR_SECRET_PATTERNS`, as a prefix and what must follow it.
const SECRET_PATTERNS: [Secret; 7] = [
    Secret::Run("sk-", CharClass::Base64Url, 20),
    Secret::Run("ghp_", CharClass::Alnum, 30),
    Secret::Run("github_pat_", CharClass::AlnumUnderscore, 30),
    Secret::Run("AKIA", CharClass::UpperDigit, 16),
    Secret::Slack,
    Secret::Run("AIza", CharClass::Base64Url, 35),
    Secret::Jwt,
];

/// `_DOCTOR_OUTPUT_DIR_MAP`.
const OUTPUT_DIRS: [(&str, &str); 12] = [
    (".claude", "claude"),
    (".cursor", "cursor"),
    (".codex", "codex"),
    (".kimi-code", "kimi"),
    (".opencode", "opencode"),
    (".windsurf", "windsurf"),
    (".gemini", "gemini"),
    (".junie", "junie"),
    (".cline", "cline"),
    (".amazonq", "amazonq"),
    (".zed", "zed"),
    (".agents", "codex"),
];

#[derive(Clone, Copy)]
enum CharClass {
    /// `[A-Za-z0-9_-]`
    Base64Url,
    /// `[A-Za-z0-9]`
    Alnum,
    /// `[A-Za-z0-9_]`
    AlnumUnderscore,
    /// `[0-9A-Z]`
    UpperDigit,
}

impl CharClass {
    fn matches(self, b: u8) -> bool {
        match self {
            CharClass::Base64Url => b.is_ascii_alphanumeric() || b == b'_' || b == b'-',
            CharClass::Alnum => b.is_ascii_alphanumeric(),
            CharClass::AlnumUnderscore => b.is_ascii_alphanumeric() || b == b'_',
            CharClass::UpperDigit => b.is_ascii_uppercase() || b.is_ascii_digit(),
        }
    }
}

enum Secret {
    /// `<prefix>[class]{min,}`.
    Run(&'static str, CharClass, usize),
    /// `xox[baprs]-[A-Za-z0-9-]{10,}`.
    Slack,
    /// `eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}`.
    Jwt,
}

fn run_len(bytes: &[u8], class: CharClass) -> usize {
    bytes.iter().take_while(|b| class.matches(**b)).count()
}

impl Secret {
    fn found_in(&self, line: &[u8]) -> bool {
        match self {
            Secret::Run(prefix, class, min) => (0..line.len()).any(|i| {
                line[i..].starts_with(prefix.as_bytes())
                    && run_len(&line[i + prefix.len()..], *class) >= *min
            }),
            Secret::Slack => (0..line.len()).any(|i| {
                let rest = &line[i..];
                rest.len() > 5
                    && rest.starts_with(b"xox")
                    && b"baprs".contains(&rest[3])
                    && rest[4] == b'-'
                    && rest[5..]
                        .iter()
                        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'-')
                        .count()
                        >= 10
            }),
            Secret::Jwt => (0..line.len()).any(|i| {
                let rest = &line[i..];
                if !rest.starts_with(b"eyJ") {
                    return false;
                }
                let mut at = 0;
                for part in 0..3 {
                    let len = run_len(&rest[at..], CharClass::Base64Url);
                    if len < if part == 0 { 13 } else { 10 } {
                        return false;
                    }
                    at += len;
                    if part < 2 {
                        if rest.get(at) != Some(&b'.') {
                            return false;
                        }
                        at += 1;
                    }
                }
                true
            }),
        }
    }
}

/// `_doctor_scan_file`: the `grep -n` lines of the first pattern with a hit
/// that is not a placeholder; empty for a clean or binary file.
fn scan_secrets(bytes: &[u8]) -> Vec<String> {
    if bytes.contains(&0) {
        return Vec::new();
    }
    let mut lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    if lines.last() == Some(&&b""[..]) {
        lines.pop();
    }
    for pattern in &SECRET_PATTERNS {
        let mut hits = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if !pattern.found_in(line) {
                continue;
            }
            let text = String::from_utf8_lossy(line);
            if text.contains("${") && text[text.find("${").unwrap_or(0)..].contains('}') {
                continue;
            }
            if text.contains('<')
                && text[text.find('<').unwrap_or(0)..].contains('>')
                && !text.contains("sk-")
            {
                continue;
            }
            hits.push(format!("{}:{text}", index + 1));
        }
        if !hits.is_empty() {
            return hits;
        }
    }
    Vec::new()
}

/// `_doctor_validate_json` as `python3 -c 'json.load(...)'` answers it: RFC
/// 8259 JSON, any top-level value, plus `NaN`, `Infinity`, and `-Infinity`.
fn json_valid(bytes: &[u8]) -> bool {
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

struct External {
    raw: String,
    abs: String,
    refused: bool,
    untrusted: bool,
}

struct Doctor<'a> {
    project: &'a Project,
    root: String,
    config: Option<String>,
    config_shown: String,
    paths: Paths,
    style: &'a Style,
    version: &'a str,
    warnings: usize,
    errors: usize,
    advisories: usize,
    warned_legacy: bool,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl Doctor<'_> {
    fn say(&mut self, text: &str) -> Result<(), Error> {
        put(self.out, text.as_bytes())
    }

    fn ok(&mut self, text: &str) -> Result<(), Error> {
        let line = format!("    {} {text}\n", self.style.green("✓"));
        self.say(&line)
    }

    fn warn(&mut self, text: &str) -> Result<(), Error> {
        self.warnings += 1;
        let line = format!("    {} {text}\n", self.style.yellow("⚠"));
        self.say(&line)
    }

    fn fail(&mut self, text: &str) -> Result<(), Error> {
        self.errors += 1;
        let line = format!("    {} {text}\n", self.style.red("✗"));
        self.say(&line)
    }

    fn info(&mut self, text: &str) -> Result<(), Error> {
        let line = format!("    {} {text}\n", self.style.dim("·"));
        self.say(&line)
    }

    fn advise(&mut self, text: &str) -> Result<(), Error> {
        self.advisories += 1;
        let line = format!("    {} {text}\n", self.style.yellow("⚠"));
        self.say(&line)
    }

    fn heading(&mut self, text: &str) -> Result<(), Error> {
        let line = format!("{}\n", self.style.bold(&format!("  {text}")));
        self.say(&line)
    }

    fn rel(&self, path: &str) -> String {
        path.strip_prefix(&format!("{}/", self.root))
            .unwrap_or(path)
            .to_string()
    }

    /// `_doctor_external_source`.
    fn external_source(&self, key: &str) -> Option<External> {
        let config = self.config.as_deref()?;
        let raw = yaml_subset::value(config, &format!("source.{key}"));
        if raw.is_empty() {
            return None;
        }
        let (refused, untrusted) = match self.paths.classify_explicit_source(&raw) {
            ExplicitSource::Inside => return None,
            ExplicitSource::Outside(_) => (false, false),
            ExplicitSource::Refused(_) => (true, false),
            ExplicitSource::Untrusted(_) => (false, true),
        };
        Some(External {
            abs: self.paths.absolute(&raw),
            raw,
            refused,
            untrusted,
        })
    }

    fn tool(&self, slug: &str) -> Result<Tool, Error> {
        Tool::load(self.project, slug)
    }

    fn display_name(&self, slug: &str) -> Result<String, Error> {
        Ok(self.tool(slug)?.display_name())
    }

    /// `resolve_payload_source` under `[[ -f ]]`: the payload when its file
    /// exists, with the legacy-layout warning on stderr once per run.
    fn resolve(&mut self, tool: &Tool, resource: &str) -> Result<Option<Source>, Error> {
        let (source, legacy) = payload::effective_source(self.project, tool, resource)?;
        if let Some(path) = legacy.filter(|_| !self.warned_legacy) {
            self.warned_legacy = true;
            put(
                self.err,
                payload::legacy_warning(self.project, &path).as_bytes(),
            )?;
        }
        Ok(source.filter(|source| match source {
            Source::Disk(path) => path.is_file(),
            Source::Shipped(_) => true,
        }))
    }

    fn source_shown(&self, source: &Source) -> String {
        self.rel(&source.shown())
    }

    /// `_doctor_check_commands_config`.
    fn check_commands_config(&mut self, tool: &Tool) -> Result<(), Error> {
        let slug = tool.slug.clone();
        let dest = tool.value("targets.commands.dest");
        let as_skills = tool.value("targets.commands.as_skills") == "true";
        let inline = tool.value("targets.commands.inline_into_agents") == "true";
        if !dest.is_empty() && as_skills {
            self.warn(&format!(
                "{slug}: targets.commands.dest and .as_skills both set — dest wins; remove one"
            ))?;
        }
        if !dest.is_empty() && inline {
            self.warn(&format!(
                "{slug}: targets.commands.dest and .inline_into_agents both set — dest wins; remove one"
            ))?;
        }
        if as_skills && inline {
            self.warn(&format!(
                "{slug}: targets.commands.as_skills and .inline_into_agents both true — as_skills wins; remove one"
            ))?;
        }
        if as_skills && tool.value("targets.skills.dest").is_empty() {
            self.warn(&format!(
                "{slug}: targets.commands.as_skills requires targets.skills.dest — option will no-op"
            ))?;
        }
        if inline && tool.value("targets.agents.dest").is_empty() {
            self.warn(&format!(
                "{slug}: targets.commands.inline_into_agents requires targets.agents.dest — option will no-op"
            ))?;
        }
        Ok(())
    }

    /// `_doctor_check_payload_ownership`.
    fn check_payload_ownership(&mut self, tool: &Tool) -> Result<(), Error> {
        match tool.slug.as_str() {
            "opencode" => {
                let settings = self.resolve(tool, "settings")?;
                let mcp = self.resolve(tool, "mcp")?;
                if let (Some(settings), Some(mcp)) = (settings, mcp) {
                    let text = String::from_utf8_lossy(&settings.bytes()?).into_owned();
                    if opencode_json::settings_has_mcp(&text) == Ok(true) {
                        self.fail(&format!(
                            "OpenCode MCP ownership conflict: {} and {} both define mcp. Move the canonical server map into one source.",
                            self.source_shown(&settings),
                            self.source_shown(&mcp)
                        ))?;
                    }
                }
            }
            "kimi" => {
                let mut hook = payload::find_new_override(self.project, "kimi", "hooks")?;
                if hook.is_none() {
                    hook = sorted_entries(&Path::new(&self.root).join(".ai/src/hooks"))
                        .into_iter()
                        .find(|path| {
                            path.is_file()
                                && path
                                    .file_name()
                                    .is_some_and(|n| n.to_string_lossy().starts_with("kimi."))
                        });
                }
                if let Some(path) = hook {
                    let shown = self.rel(&path.to_string_lossy());
                    self.advise(&format!(
                        "Kimi hooks are global-only in $KIMI_CODE_HOME/config.toml; AgentSync leaves it untouched. Remove {shown} from project sources."
                    ))?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// `_doctor_check_guard_wired`.
    fn check_guard_wired(&mut self, tool: &Tool) -> Result<(), Error> {
        if self.resolve(tool, "guard")?.is_none() {
            return Ok(());
        }
        let guard_dest = tool.value("targets.guard.dest");
        let settings_dest = tool.value("targets.settings.dest");
        if guard_dest.is_empty() || settings_dest.is_empty() {
            return Ok(());
        }
        let Some(settings) = self.resolve(tool, "settings")? else {
            return Ok(());
        };
        let guard_name = paths::leaf(&guard_dest);
        let bytes = settings.bytes()?;
        let referenced = bytes
            .windows(guard_name.len())
            .any(|window| window == guard_name.as_bytes());
        if !referenced {
            self.warn(&format!(
                "{}: {guard_dest} is generated but {} never references it — the guard against edits to generated files is inert. Add the hooks block from the shipped base, or delete the override to inherit it.",
                tool.display_name(),
                self.source_shown(&settings)
            ))?;
        }
        Ok(())
    }

    /// `_doctor_check_drift`.
    fn check_drift(&mut self) -> Result<(), Error> {
        let style = self.style;
        let manifest_path = Path::new(&self.root).join(manifest::REL);
        if !manifest_path.is_file() {
            return self.info(&format!(
                "No .sync-manifest yet — run {} to create it",
                style.cyan("agentsync sync")
            ));
        }
        let Some(manifest) = Manifest::load(&self.root)? else {
            return Ok(());
        };
        if manifest.entries().is_empty() {
            return self.info(".sync-manifest is empty");
        }
        let (mut edited, mut missing, mut clean) = (0, 0, 0);
        for (rel, old_hash) in manifest.entries() {
            let dest = Path::new(&self.root).join(rel);
            if !dest.is_file() {
                self.warn(&format!("{rel} — missing (deleted manually)"))?;
                missing += 1;
                continue;
            }
            let Some(current) = template_manifest::hash(&dest) else {
                self.warn(&format!("{rel} — could not hash"))?;
                continue;
            };
            if &current != old_hash {
                self.warn(&format!("{rel} — edited since last sync"))?;
                edited += 1;
            } else {
                clean += 1;
            }
        }
        if edited == 0 && missing == 0 {
            self.ok(&format!("All {clean} tracked file(s) match the manifest"))
        } else {
            self.say(&format!(
                "\n    {} {} {} {} {}\n",
                style.dim("Re-run"),
                style.cyan("agentsync sync"),
                style.dim("to overwrite, or move edits into"),
                style.cyan(".ai/src/"),
                style.dim("first.")
            ))
        }
    }

    /// `_doctor_scan_one_file`; returns (secret hit, invalid JSON).
    fn scan_one_file(&mut self, file: &Path) -> Result<(bool, bool), Error> {
        let style = self.style;
        let shown = self.rel(&file.to_string_lossy());
        let bytes = std::fs::read(file).map_err(|e| Error::io(file, e))?;
        if file.extension().is_some_and(|ext| ext == "json") && !json_valid(&bytes) {
            self.fail(&format!("{shown}: invalid JSON syntax"))?;
            return Ok((false, true));
        }
        let hits = scan_secrets(&bytes);
        if hits.is_empty() {
            return Ok((false, false));
        }
        self.fail(&format!("{shown}: possible secret"))?;
        for hit in hits {
            self.say(&format!("        {}\n", style.dim(&hit)))?;
        }
        Ok((true, false))
    }

    /// `_doctor_scan_overrides`.
    fn scan_overrides(&mut self) -> Result<(), Error> {
        let style = self.style;
        let (mut hits, mut invalid, mut legacy) = (0, 0, 0);
        let tools_root = self.project.user_tools_dir();
        if tools_root.is_dir() {
            for tool_dir in sorted_entries(&tools_root)
                .into_iter()
                .filter(|p| p.is_dir())
            {
                for resource in ["mcp", "settings", "hooks"] {
                    for file in sorted_entries(&tool_dir).into_iter().filter(|p| {
                        p.is_file()
                            && p.file_name().is_some_and(|n| {
                                n.to_string_lossy().starts_with(&format!("{resource}."))
                            })
                    }) {
                        let (hit, bad) = self.scan_one_file(&file)?;
                        hits += usize::from(hit);
                        invalid += usize::from(bad);
                    }
                }
            }
        }
        for resource in ["mcp", "settings", "hooks"] {
            let dir = Path::new(&self.root).join(".ai/src").join(resource);
            if !dir.is_dir() {
                continue;
            }
            for file in sorted_entries(&dir).into_iter().filter(|p| p.is_file()) {
                legacy += 1;
                let (hit, bad) = self.scan_one_file(&file)?;
                hits += usize::from(hit);
                invalid += usize::from(bad);
            }
        }
        if legacy > 0 {
            self.warn(&format!(
                "Legacy payload layout ({legacy} file(s) under .ai/src/{{hooks,mcp,settings}}/). Run {} to move them to .ai/src/tools/<tool>/<resource>.<ext>.",
                style.cyan("agentsync migrate --apply")
            ))?;
        }
        if hits == 0 && invalid == 0 && legacy == 0 {
            self.info("No overrides to scan, or all clean.")?;
        } else if hits > 0 {
            self.say("\n")?;
            self.info(&format!(
                "{}: use ${{ENV_VAR}} placeholders; never commit raw secrets.",
                style.yellow("Reminder")
            ))?;
        }
        Ok(())
    }

    /// `_doctor_check_empty_skills`.
    fn check_empty_skills(&mut self) -> Result<(), Error> {
        let style = self.style;
        let skills = Path::new(&self.root).join(".ai/src/skills");
        if !skills.is_dir() {
            return self.info("No .ai/src/skills/ — nothing to scan.");
        }
        let mut found = 0;
        for dir in sorted_entries(&skills).into_iter().filter(|p| p.is_dir()) {
            if !dir.join("SKILL.md").is_file() {
                let name = dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.advise(&format!(
                    "skills/{name}/ — missing SKILL.md {}",
                    style.dim("(empty skill — populate or remove)")
                ))?;
                found += 1;
            }
        }
        if found == 0 {
            self.ok("All skill directories contain SKILL.md")
        } else {
            self.info(&format!(
                "{} {} {}",
                style.dim("Tip:"),
                style.cyan("agentsync simplify"),
                style.dim("can prune empty skill dirs.")
            ))
        }
    }

    /// `_doctor_check_always_on_rules`.
    fn check_always_on_rules(&mut self) -> Result<(), Error> {
        let style = self.style;
        let rules = Path::new(&self.root).join(".ai/src/rules");
        if !rules.is_dir() {
            return self.info("No .ai/src/rules/ — nothing to scan.");
        }
        let (mut count, mut bytes) = (0usize, 0usize);
        for file in sorted_entries(&rules)
            .into_iter()
            .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "md"))
        {
            let content = std::fs::read(&file).map_err(|e| Error::io(&file, e))?;
            if is_path_scoped(&content) {
                continue;
            }
            count += 1;
            bytes += content.len();
        }
        if count == 0 {
            self.ok("No always-on rules (every rule is paths:-scoped)")
        } else if bytes >= 20000 {
            self.advise(&format!(
                "{count} always-on rule(s) load on every task (~{} KB, ~{} tokens). Add {} frontmatter to domain rules so they load only when matching files are touched — a large always-on set dilutes attention.",
                bytes / 1024,
                bytes / 4,
                style.cyan("paths:")
            ))
        } else {
            self.ok(&format!(
                "Always-on rule context is lean ({count} file(s), ~{} KB)",
                bytes / 1024
            ))
        }
    }

    /// `_doctor_check_orphan_outputs`.
    fn check_orphan_outputs(&mut self) -> Result<(), Error> {
        let style = self.style;
        let enabled = self.project.enabled_tools()?;
        let mut found = 0;
        if Path::new(&self.root).join(".agent").is_dir() {
            self.advise(&format!(
                ".agent/ — legacy pre-v0.6 layout (run {} to preview cleanup)",
                style.cyan("agentsync migrate --legacy")
            ))?;
            found += 1;
        }
        for (dir, tool) in OUTPUT_DIRS {
            if !Path::new(&self.root).join(dir).is_dir() {
                continue;
            }
            if dir == ".agents" && (enabled.contains("codex") || enabled.contains("antigravity")) {
                continue;
            }
            if !enabled.contains(tool) {
                self.advise(&format!(
                    "{dir}/ — orphan (tool '{tool}' not enabled; output left from prior run)"
                ))?;
                found += 1;
            }
        }
        if found == 0 {
            self.ok("No orphan tool-output directories")?;
        }
        Ok(())
    }

    /// `_doctor_check_cross_project`.
    fn check_cross_project(&mut self) -> Result<(), Error> {
        let style = self.style;
        let child_src = format!("{}/.ai/src", self.root);
        if !Path::new(&child_src).is_dir() {
            return self.info("No .ai/src/ in this project — skipping cross-project scan.");
        }
        let mut from_shared = false;
        let parent_src = match self
            .config
            .as_deref()
            .and_then(|config| overlay::shared_parent_src(config, &self.root))
        {
            Some(parent) => {
                from_shared = true;
                Some(parent)
            }
            None => paths::find_parent_ai_src(&self.root),
        };
        let Some(parent_src) = parent_src else {
            return self.info("No parent .ai/src/ found within git boundary.");
        };
        let origin_hint = if from_shared {
            format!(" {}", style.dim("(from shared.path)"))
        } else {
            String::new()
        };
        self.info(&format!(
            "Parent source: {}{origin_hint}",
            style.dim(&parent_src)
        ))?;
        self.say("\n")?;

        let inherited: Vec<&str> = self
            .config
            .as_deref()
            .map(|config| {
                overlay::inherit_categories(&yaml_subset::value(config, "shared.inherit"))
            })
            .unwrap_or_default();
        let parent_root = paths::parent(&parent_src);
        let (mut dupes, mut divergent) = (0, 0);
        let mut pairs: Vec<(String, PathBuf)> = Vec::new();
        for category in ["rules", "commands", "agents"] {
            let dir = Path::new(&parent_src).join(category);
            if !dir.is_dir() {
                continue;
            }
            for file in sorted_entries(&dir)
                .into_iter()
                .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "md"))
            {
                let name = file
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                pairs.push((format!("{category}/{name}"), file));
            }
        }
        let skills = Path::new(&parent_src).join("skills");
        if skills.is_dir() {
            let mut files = Vec::new();
            files_below(&skills, &mut files);
            files.retain(|p| {
                !p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            });
            files.sort();
            for file in files {
                let rel = file
                    .strip_prefix(&parent_src)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                pairs.push((rel, file));
            }
        }
        for (rel, parent_file) in pairs {
            let child_file = Path::new(&child_src).join(&rel);
            if !child_file.is_file() {
                continue;
            }
            let (Some(child_hash), Some(parent_hash)) = (
                template_manifest::hash(&child_file),
                template_manifest::hash(&parent_file),
            ) else {
                continue;
            };
            let category = rel.split('/').next().unwrap_or("");
            if child_hash == parent_hash {
                let hint = if inherited.contains(&category) {
                    format!(" {}", style.dim("(inherited via shared: — safe to delete)"))
                } else {
                    String::new()
                };
                let shown = parent_file
                    .to_string_lossy()
                    .strip_prefix(&format!("{parent_root}/"))
                    .unwrap_or(&parent_file.to_string_lossy())
                    .to_string();
                self.advise(&format!(
                    "{rel} — duplicate of parent's {}{hint}",
                    style.dim(&shown)
                ))?;
                dupes += 1;
            } else {
                let content =
                    std::fs::read(&parent_file).map_err(|e| Error::io(&parent_file, e))?;
                if convert::read_field(&content, "category") == b"governance" {
                    self.advise(&format!(
                        "{rel} — {} {}",
                        style.yellow("governance file diverges from parent"),
                        style.dim("(category: governance — likely a mistake, not an override)")
                    ))?;
                } else {
                    self.info(&format!(
                        "{rel} — diverges from parent {}",
                        style.dim("(review intent)")
                    ))?;
                }
                divergent += 1;
            }
        }
        if dupes == 0 && divergent == 0 {
            self.ok("No source files shared with parent.")
        } else if dupes > 0 {
            self.say("\n")?;
            self.info(&format!(
                "{} {} {}",
                style.dim("Run"),
                style.cyan("agentsync dedupe"),
                style.dim("to remove duplicates interactively.")
            ))
        } else {
            Ok(())
        }
    }
}

/// `sed -n '2,/^---$/p' | grep -q '^paths:[[:space:]]*$'` after a first
/// line of `---`. sed tests the closing address from line 3 on, so a `---`
/// on line 2 does not end the range.
fn is_path_scoped(content: &[u8]) -> bool {
    let mut lines = content.split(|b| *b == b'\n');
    if lines.next() != Some(b"---") {
        return false;
    }
    for (index, line) in lines.enumerate() {
        if line.strip_prefix(b"paths:").is_some_and(|rest| {
            rest.iter()
                .all(|b| matches!(b, b' ' | b'\t' | b'\r' | 0x0b | 0x0c))
        }) {
            return true;
        }
        if index > 0 && line == b"---" {
            return false;
        }
    }
    false
}

/// Directory entries in byte order, as `LC_ALL=C` globs list them.
fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names.into_iter().map(|name| dir.join(name)).collect()
}

/// `find <dir> -type f`.
fn files_below(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            files_below(&path, found);
        } else if meta.is_file() {
            found.push(path);
        }
    }
}

/// `cmd_doctor`: the report on `out`, the tri-state status as the result.
pub fn doctor(
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let project = match discover() {
        Ok(project) => project,
        Err(Error::ConfigPathNotFound(path)) => {
            put(
                err,
                format!(
                    "{}: AGENTSYNC_CONFIG_PATH is set but file not found: {}\n",
                    style.red("Error"),
                    path.display()
                )
                .as_bytes(),
            )?;
            return Ok(2);
        }
        Err(e) => return Err(e),
    };
    let root = project.root.to_string_lossy().into_owned();
    let config = match &project.config_path {
        Some(path) => Some(
            std::fs::read(path)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|e| Error::io(path, e))?,
        ),
        None => None,
    };
    let config_shown = project
        .config_path
        .as_ref()
        .map(|p| {
            let text = p.to_string_lossy();
            text.strip_prefix(&format!("{root}/"))
                .unwrap_or(&text)
                .to_string()
        })
        .unwrap_or_default();
    let mut paths = Paths::on_disk(&root);
    paths.trust_external_roots(env.external_roots.as_deref());
    let mut d = Doctor {
        project: &project,
        root: root.clone(),
        config,
        config_shown,
        paths,
        style,
        version: env.version,
        warnings: 0,
        errors: 0,
        advisories: 0,
        warned_legacy: false,
        out,
        err,
    };

    d.say(&format!(
        "\n{}\n{}\n\n",
        style.bold("  AgentSync Doctor"),
        style.dim(&format!("  {root}"))
    ))?;

    d.heading("Project layout")?;
    if Path::new(&root).join(".ai").is_dir() {
        d.ok(".ai/ directory present")?;
    } else {
        d.fail(".ai/ directory missing — run 'agentsync init'")?;
        d.say("\n")?;
        return Ok(2);
    }
    let agents_found = match d.external_source("agents") {
        Some(external) => Path::new(&external.abs).is_file(),
        None => {
            Path::new(&root).join(".ai/src/AGENTS.md").is_file()
                || Path::new(&root).join(".ai/AGENTS.md").is_file()
        }
    };
    if agents_found {
        d.ok("AGENTS.md source file found")?;
    } else {
        d.fail("No AGENTS.md in .ai/src/ or .ai/ — sync will fail")?;
    }
    if let Some(config) = d.config.clone() {
        let shown = d.config_shown.clone();
        d.ok(&format!("Project config: {}", style.dim(&shown)))?;
        let pinned = yaml_subset::value(&config, "agentsync_version").replace('"', "");
        if !pinned.is_empty() && !d.version.is_empty() && pinned != d.version {
            d.warn(&format!(
                "CLI version {} differs from pinned {} — run {} to align",
                style.dim(&format!("v{}", d.version)),
                style.dim(&format!("v{pinned}")),
                style.cyan("agentsync upgrade-config")
            ))?;
        }
        let engine_rev = format_rev::engine();
        let project_rev = format_rev::project(&config);
        if project_rev < engine_rev {
            d.warn(&format!(
                "Project format {} is behind the engine {} — run {} to preview",
                style.dim(&format!("r{project_rev}")),
                style.dim(&format!("r{engine_rev}")),
                style.cyan("agentsync migrate")
            ))?;
        } else {
            d.ok(&format!(
                "Project format: {}",
                style.dim(&format!("r{project_rev}"))
            ))?;
        }
    } else {
        d.warn("No agent_sync.yaml — using defaults only")?;
    }
    d.say("\n")?;

    d.heading("Enabled tools")?;
    let enabled = project.enabled_tools()?;
    if enabled.is_empty() {
        d.info(&format!(
            "No tools enabled — run {}",
            style.cyan("agentsync enable <slug>")
        ))?;
    } else {
        for slug in &enabled {
            let has_base = catalog::base_tool_yaml(slug).is_some();
            let has_user = project.user_tool_file(slug).is_file();
            if has_base && has_user {
                let display = d.display_name(slug)?;
                d.ok(&format!("{display} {}", style.dim("(customized)")))?;
            } else if has_base {
                let display = d.display_name(slug)?;
                d.ok(&display)?;
            } else if has_user {
                d.warn(&format!(
                    "{slug}: custom tool (no base) — ensure override defines full config"
                ))?;
            } else {
                d.fail(&format!(
                    "{slug}: unknown — no base template and no override"
                ))?;
            }
            let tool = d.tool(slug)?;
            d.check_commands_config(&tool)?;
            d.check_payload_ownership(&tool)?;
            d.check_guard_wired(&tool)?;
        }
    }
    d.say("\n")?;

    if !enabled.is_empty() {
        d.heading("Edit paths")?;
        let known: Vec<String> = {
            let mut all = catalog::base_tools();
            all.extend(project.user_override_tools()?);
            all
        };
        let mut any = false;
        for slug in &enabled {
            if !known.contains(slug) {
                continue;
            }
            let tool = d.tool(slug)?;
            let text = edit_paths::checklist(&project, &tool, style);
            d.say(&text)?;
            any = true;
        }
        if !any {
            d.info("No tools with editable payloads.")?;
        }
        d.say("\n")?;
    }

    d.heading("User overrides")?;
    let overrides = project.user_override_tools()?;
    if overrides.is_empty() {
        d.info("No customizations — all tools inherit fully from base")?;
    } else {
        let configured = project.configured_enabled_tools()?;
        for slug in &overrides {
            let user_file = project.user_tool_file(slug);
            let text = std::fs::read(&user_file)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|e| Error::io(&user_file, e))?;
            if yaml_subset::value(&text, "enabled") == "true" && !configured.contains(slug) {
                d.warn(&format!(
                    "{slug}: uses legacy 'enabled: true' — migrate with {}",
                    style.cyan(&format!("agentsync enable {slug}"))
                ))?;
            } else if catalog::base_tool_yaml(slug).is_some() {
                let display = d.display_name(slug)?;
                d.info(&format!(
                    "{display} — see {}",
                    style.cyan(&format!("agentsync diff {slug}"))
                ))?;
            } else {
                d.info(&format!("{slug} (custom tool, no base)"))?;
            }
        }
    }
    d.say("\n")?;

    d.heading("Source directories")?;
    for (src, key) in [
        ("AGENTS.md", "agents"),
        ("rules", "rules"),
        ("skills", "skills"),
        ("commands", "commands"),
        ("agents", "subagents"),
    ] {
        let (display, abs) = match d.external_source(key) {
            Some(external) => {
                if external.refused {
                    d.fail(&format!(
                        "source.{key} must not be the filesystem root, the home directory, or the project root or its ancestor: {}",
                        external.raw
                    ))?;
                    continue;
                }
                if external.untrusted {
                    d.fail(&format!(
                        "source.{key} points outside the project and AGENTSYNC_EXTERNAL_SOURCE_ROOTS does not list it: {}",
                        external.raw
                    ))?;
                    continue;
                }
                (external.raw, external.abs)
            }
            None => (format!(".ai/src/{src}"), format!("{root}/.ai/src/{src}")),
        };
        if Path::new(&abs).exists() {
            d.ok(&display)?;
        } else if src == "AGENTS.md" {
            d.fail(&format!("{display} missing (required)"))?;
        } else {
            d.info(&format!("{display} not present (optional)"))?;
        }
    }
    d.say("\n")?;

    d.heading("Drift")?;
    d.check_drift()?;
    d.say("\n")?;

    d.heading("Security")?;
    d.scan_overrides()?;
    d.say("\n")?;

    d.heading("Skills")?;
    d.check_empty_skills()?;
    d.say("\n")?;

    d.heading("Rules")?;
    d.check_always_on_rules()?;
    d.say("\n")?;

    d.heading("Tool outputs")?;
    d.check_orphan_outputs()?;
    d.say("\n")?;

    d.heading("Cross-project")?;
    d.check_cross_project()?;
    d.say("\n")?;

    d.say(&format!("  {}\n", "─".repeat(60)))?;
    let advisory_label = if d.advisories > 0 {
        format!(
            ", {}",
            style.dim(&format!("{} advisory(ies)", d.advisories))
        )
    } else {
        String::new()
    };
    if d.errors > 0 {
        d.say(&format!(
            "  {}, {}{advisory_label}\n\n",
            style.red(&format!("{} error(s)", d.errors)),
            style.yellow(&format!("{} warning(s)", d.warnings))
        ))?;
        Ok(2)
    } else if d.warnings > 0 {
        d.say(&format!(
            "  {} with {}{advisory_label}\n\n",
            style.green("OK"),
            style.yellow(&format!("{} warning(s)", d.warnings))
        ))?;
        Ok(1)
    } else if d.advisories > 0 {
        d.say(&format!(
            "  {} with {}\n\n",
            style.green("OK"),
            style.dim(&format!("{} advisory(ies)", d.advisories))
        ))?;
        Ok(0)
    } else {
        d.say(&format!("  {}\n\n", style.green("All checks passed.")))?;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_listed_by_the_first_pattern_that_hits_like_doctor_scan_file() {
        let mcp = br#"{"mcpServers":{"gh":{"env":{"TOKEN":"ghp_abcdefghijklmnopqrstuvwxyz012345678901"}}}}"#;
        let expected = format!("1:{}", String::from_utf8_lossy(mcp));
        assert_eq!(
            scan_secrets(&[mcp.as_slice(), b"\n"].concat()),
            vec![expected]
        );
        let many = br#"{"aws":{"key":"AKIAIOSFODNN7EXAMPLE"},"slack":"xoxb-1234567890-abc","g":"AIzaSyA1234567890abcdefghijklmnopqrstuv","jwt":"eyJabcdefghijk.eyJabcdefghijk.abcdefghijklmn","pat":"github_pat_abcdefghijklmnopqrstuvwxyz0123456789"}"#;
        assert_eq!(scan_secrets(many).len(), 1);
        assert_eq!(
            scan_secrets(b"AKIAIOSFODNN7EXAMPLE\nsk-abcdefghijklmnopqrstuvwxyz\n"),
            vec!["2:sk-abcdefghijklmnopqrstuvwxyz".to_string()]
        );
        assert_eq!(
            scan_secrets(
                b"first xoxp-abcdefghij-k\nsecond eyJabcdefghijk.eyJabcdefghijk.abcdefghijklmn\n"
            ),
            vec!["1:first xoxp-abcdefghij-k".to_string()]
        );
        assert_eq!(
            scan_secrets(
                b"a ${X} ghp_abcdefghijklmnopqrstuvwxyz012345678901\nb AKIAIOSFODNN7EXAMPLE\n"
            ),
            vec!["2:b AKIAIOSFODNN7EXAMPLE".to_string()]
        );
        assert!(
            scan_secrets(b"token: ${GITHUB_TOKEN} ghp_abcdefghijklmnopqrstuvwxyz012345678901\n")
                .is_empty()
        );
        assert!(scan_secrets(b"<ghp_abcdefghijklmnopqrstuvwxyz012345678901>\n").is_empty());
        assert_eq!(
            scan_secrets(b"<sk-abcdefghijklmnopqrstuvwxyz>\n"),
            vec!["1:<sk-abcdefghijklmnopqrstuvwxyz>".to_string()]
        );
        assert!(scan_secrets(b"\0binary sk-abcdefghijklmnopqrstuvwxyz\n").is_empty());
        assert!(scan_secrets(b"sk-short\nxoxb-123\nAKIA1234\n").is_empty());
    }

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

    #[test]
    fn a_paths_key_is_found_like_the_sed_range_does() {
        assert!(is_path_scoped(
            b"---\npaths:\n  - \"**/*.ts\"\n---\n# Scoped\n"
        ));
        assert!(is_path_scoped(b"---\n---\npaths:\n"));
        assert!(is_path_scoped(b"---\npaths:  \n---\n"));
        assert!(!is_path_scoped(b"# Rule\n"));
        assert!(!is_path_scoped(b"---\ndesc: x\n---\npaths:\n"));
        assert!(!is_path_scoped(b"---\npaths: foo\n---\n"));
        assert!(!is_path_scoped(b"---"));
        assert!(!is_path_scoped(b"paths:\n"));
    }

    #[cfg(unix)]
    fn project(files: &[(&str, &str)], dirs: &[&str]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for rel in dirs {
            std::fs::create_dir_all(Path::new(&root).join(rel)).unwrap();
        }
        for (rel, text) in files {
            let path = Path::new(&root).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        (dir, root)
    }

    #[cfg(unix)]
    fn run(root: &str) -> (u8, String, String) {
        let env = Env {
            version: "0.36.0",
            external_roots: None,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = doctor(
            &|| Project::at(root),
            &Style::plain(),
            &env,
            &mut out,
            &mut err,
        )
        .unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[cfg(unix)]
    #[test]
    fn a_project_without_a_config_reports_like_cmd_doctor() {
        let (_dir, root) = project(&[(".ai/src/AGENTS.md", "# Agents\n")], &[".git"]);
        let (status, out, err) = run(&root);
        assert_eq!(status, 1);
        assert_eq!(err, "");
        let expected = format!(
            "\n  AgentSync Doctor\n  {root}\n\n  Project layout\n    ✓ .ai/ directory present\n    ✓ AGENTS.md source file found\n    ⚠ No agent_sync.yaml — using defaults only\n\n  Enabled tools\n    · No tools enabled — run agentsync enable <slug>\n\n  User overrides\n    · No customizations — all tools inherit fully from base\n\n  Source directories\n    ✓ .ai/src/AGENTS.md\n    · .ai/src/rules not present (optional)\n    · .ai/src/skills not present (optional)\n    · .ai/src/commands not present (optional)\n    · .ai/src/agents not present (optional)\n\n  Drift\n    · No .sync-manifest yet — run agentsync sync to create it\n\n  Security\n    · No overrides to scan, or all clean.\n\n  Skills\n    · No .ai/src/skills/ — nothing to scan.\n\n  Rules\n    · No .ai/src/rules/ — nothing to scan.\n\n  Tool outputs\n    ✓ No orphan tool-output directories\n\n  Cross-project\n    · No parent .ai/src/ found within git boundary.\n\n  {}\n  OK with 1 warning(s)\n\n",
            "─".repeat(60)
        );
        assert_eq!(out, expected);
    }

    #[cfg(unix)]
    #[test]
    fn a_pinned_config_stale_manifest_and_orphan_output_report_like_cmd_doctor() {
        let (_dir, root) = project(
            &[
                (".ai/src/AGENTS.md", "# Agents\n"),
                (
                    ".ai/src/rules/scoped.md",
                    "---\npaths:\n  - \"**/*.ts\"\n---\n# Scoped\n",
                ),
                (
                    ".ai/agent_sync.yaml",
                    "agentsync_version: \"0.0.1\"\nformat: 1\ntools:\n  - cursor\n",
                ),
                (
                    ".ai/.sync-manifest",
                    "CLAUDE.md\t0000000000000000000000000000000000000000000000000000000000000000\n",
                ),
            ],
            &[".git", ".ai/src/skills/empty", ".claude"],
        );
        let (status, out, err) = run(&root);
        assert_eq!(status, 1);
        assert_eq!(err, "");
        let expected = format!(
            "\n  AgentSync Doctor\n  {root}\n\n  Project layout\n    ✓ .ai/ directory present\n    ✓ AGENTS.md source file found\n    ✓ Project config: .ai/agent_sync.yaml\n    ⚠ CLI version v0.36.0 differs from pinned v0.0.1 — run agentsync upgrade-config to align\n    ⚠ Project format r1 is behind the engine r2 — run agentsync migrate to preview\n\n  Enabled tools\n    · No tools enabled — run agentsync enable <slug>\n\n  User overrides\n    · No customizations — all tools inherit fully from base\n\n  Source directories\n    ✓ .ai/src/AGENTS.md\n    ✓ .ai/src/rules\n    ✓ .ai/src/skills\n    · .ai/src/commands not present (optional)\n    · .ai/src/agents not present (optional)\n\n  Drift\n    ⚠ CLAUDE.md — missing (deleted manually)\n\n    Re-run agentsync sync to overwrite, or move edits into .ai/src/ first.\n\n  Security\n    · No overrides to scan, or all clean.\n\n  Skills\n    ⚠ skills/empty/ — missing SKILL.md (empty skill — populate or remove)\n    · Tip: agentsync simplify can prune empty skill dirs.\n\n  Rules\n    ✓ No always-on rules (every rule is paths:-scoped)\n\n  Tool outputs\n    ⚠ .claude/ — orphan (tool 'claude' not enabled; output left from prior run)\n\n  Cross-project\n    · No parent .ai/src/ found within git boundary.\n\n  {}\n  OK with 3 warning(s), 2 advisory(ies)\n\n",
            "─".repeat(60)
        );
        assert_eq!(out, expected);
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_ai_directory_exits_2_like_cmd_doctor() {
        let (_dir, root) = project(&[], &[".git"]);
        let (status, out, err) = run(&root);
        assert_eq!(status, 2);
        assert_eq!(err, "");
        assert_eq!(
            out,
            format!(
                "\n  AgentSync Doctor\n  {root}\n\n  Project layout\n    ✗ .ai/ directory missing — run 'agentsync init'\n\n"
            )
        );
    }
}
