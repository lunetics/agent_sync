//! `agentsync refresh`: `cmd_refresh` of `lib/helpers/refresh.sh`, which pulls
//! updated templates into `.ai/src/` through a three-way diff against the
//! template manifest, so untouched files update silently and only true
//! conflicts wait for an answer.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::customize::put;
use crate::manifest::sha256_hex;
use crate::style::Style;
use crate::template_manifest::{self, TemplateManifest};
use crate::{Error, catalog, yaml_subset};

/// Printed where Bash printed `$AGENTSYNC_HOME/lib/templates`; the binary
/// reads the embedded copy.
const TEMPLATES_DISPLAY: &str = "/<agentsync>/lib/templates";

const CATEGORIES_VALID: [&str; 5] = ["rules", "skills", "commands", "agents", "subagents"];
const CATEGORIES_DEFAULT: [&str; 4] = ["rules", "skills", "commands", "agents"];

/// What `refresh` takes from the terminal.
pub struct Env<'a> {
    /// `is_tty`: stdin and stdout are both terminals.
    pub interactive: bool,
    /// `read -r reply </dev/tty`, empty when the terminal cannot be read.
    pub read_line: &'a mut dyn FnMut() -> String,
}

struct Options {
    dry_run: bool,
    assume_yes: bool,
    include_agents_md: bool,
    include_deleted: bool,
    review: bool,
    status_only: bool,
    only: String,
}

/// One `<rel>|<template>|<hash>` entry of the `*_FILES` arrays.
struct Candidate {
    rel: String,
    bytes: &'static [u8],
    hash: String,
}

#[derive(Default)]
struct Changes {
    new: Vec<Candidate>,
    conflicts: Vec<Candidate>,
    auto: Vec<Candidate>,
    deleted: Vec<Candidate>,
    unchanged: usize,
    silently_kept: usize,
}

struct Classifier<'a> {
    user_base: &'a Path,
    manifest: &'a TemplateManifest,
    declined: &'a [String],
    pinned: &'a [String],
    review: bool,
    changes: Changes,
}

impl Classifier<'_> {
    /// `_refresh_classify`.
    fn classify(&mut self, rel: &str, bytes: &'static [u8]) {
        if self.declined.iter().any(|item| item == rel) {
            return;
        }
        let t_new = sha256_hex(bytes);
        let t_old = self.manifest.lookup(rel);
        let candidate = || Candidate {
            rel: rel.to_string(),
            bytes,
            hash: t_new.clone(),
        };
        let dest = self.user_base.join(rel);
        if !dest.is_file() {
            if t_old.is_some() {
                self.changes.deleted.push(candidate());
            } else {
                self.changes.new.push(candidate());
            }
            return;
        }
        let Some(u_cur) = template_manifest::hash(&dest) else {
            return;
        };
        if u_cur == t_new {
            self.changes.unchanged += 1;
            return;
        }
        if self.pinned.iter().any(|item| item == rel) {
            return;
        }
        let Some(t_old) = t_old else {
            self.changes.conflicts.push(candidate());
            return;
        };
        if u_cur == t_old {
            self.changes.auto.push(candidate());
            return;
        }
        if t_old == t_new {
            self.changes.silently_kept += 1;
            if self.review {
                self.changes.conflicts.push(candidate());
            }
            return;
        }
        self.changes.conflicts.push(candidate());
    }
}

struct Run<'a, 'b> {
    style: &'a Style,
    env: &'a mut Env<'b>,
    user_base: PathBuf,
    manifest: TemplateManifest,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

pub fn refresh(
    args: &[String],
    root: &str,
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let options = match parse_args(args, style, out, err)? {
        Ok(options) => options,
        Err(status) => return Ok(status),
    };

    let src_base = if Path::new(root).join(".ai/src").is_dir() {
        ".ai/src"
    } else if Path::new(root).join(".ai").is_dir() {
        ".ai"
    } else {
        put(
            err,
            format!(
                "{}: No .ai/ directory found in {root}\nRun {} first.\n",
                style.red("Error"),
                style.cyan("agentsync init")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let user_base_shown = format!("{root}/{src_base}");
    let user_base = PathBuf::from(&user_base_shown);

    let categories = match resolve_scope(&options.only, &user_base_shown, style, err)? {
        Ok(categories) => categories,
        Err(status) => return Ok(status),
    };

    let manifest = TemplateManifest::load(Path::new(root))?;
    let has_manifest = !manifest.is_empty();
    let (declined, pinned) = load_overrides(root);
    let templates = catalog::template_files();
    let changes = collect(
        &templates,
        &user_base,
        &categories,
        options.include_agents_md,
        &manifest,
        &declined,
        &pinned,
        options.review,
    );

    let mut run = Run {
        style,
        env,
        user_base,
        manifest,
        out,
        err,
    };

    if options.status_only {
        run.print_status(&declined, &changes.deleted)?;
        return Ok(0);
    }

    let mut scope_label = categories.join(",");
    if options.include_agents_md {
        scope_label.push_str(",AGENTS.md");
    }
    let mut header = format!(
        "\n{}\n\n  {} {TEMPLATES_DISPLAY}\n  {}   {user_base_shown}\n  {}     {scope_label}\n",
        style.bold("  AgentSync Refresh"),
        style.dim("Templates:"),
        style.dim("Project:"),
        style.dim("Scope:")
    );
    if !has_manifest {
        header.push_str(&format!(
            "  {}  {}\n",
            style.dim("Manifest:"),
            style.yellow("none — falling back to two-way diff")
        ));
    }
    header.push('\n');
    run.say(&header)?;

    let visible_deleted = options.include_deleted && !changes.deleted.is_empty();

    if changes.new.is_empty()
        && changes.conflicts.is_empty()
        && changes.auto.is_empty()
        && !visible_deleted
    {
        let mut text = format!(
            "  {} {} file(s) match the current templates.\n",
            style.green("Already up to date!"),
            changes.unchanged
        );
        if !declined.is_empty() {
            text.push_str(&format!(
                "  {}\n",
                style.dim(&format!(
                    "Persistently declined (agent_sync.yaml): {} file(s).",
                    declined.len()
                ))
            ));
        }
        if !changes.deleted.is_empty() {
            text.push_str(&format!(
                "  {}\n",
                style.dim(&format!(
                    "Locally declined (.template-manifest):    {} file(s); --include-deleted to revisit.",
                    changes.deleted.len()
                ))
            ));
        }
        if !declined.is_empty() || !changes.deleted.is_empty() {
            text.push_str(&format!(
                "  {}\n",
                style.dim("Pass --status for the full list.")
            ));
        }
        if changes.silently_kept > 0 && !options.review {
            text.push_str(&format!(
                "  {}\n",
                style.dim(&format!(
                    "{} file(s) differ from the shipped template (local edits or earlier skips); pass --review to revisit.",
                    changes.silently_kept
                ))
            ));
        }
        run.say(&text)?;
        run.heal(&templates);
        if !options.dry_run {
            run.manifest.write(Path::new(root))?;
        }
        run.say("\n")?;
        return Ok(0);
    }

    let mut summary = format!("  {}\n", style.green("Summary:"));
    if !changes.new.is_empty() {
        summary.push_str(&format!(
            "    {} {} new template(s)\n",
            style.green("+"),
            changes.new.len()
        ));
    }
    if !changes.auto.is_empty() {
        summary.push_str(&format!(
            "    {} {} auto-update(s) — you hadn't touched them locally\n",
            style.cyan("↑"),
            changes.auto.len()
        ));
    }
    if !changes.conflicts.is_empty() {
        summary.push_str(&format!(
            "    {} {} conflict(s) — your version differs from the template\n",
            style.yellow("~"),
            changes.conflicts.len()
        ));
    }
    if visible_deleted {
        summary.push_str(&format!(
            "    {} {} previously declined — pass --include-deleted to revisit\n",
            style.dim("?"),
            changes.deleted.len()
        ));
    }
    if changes.silently_kept > 0 && !options.review {
        summary.push_str(&format!(
            "    {} {} silently kept (local edits or earlier skips) — pass --review to revisit\n",
            style.dim("·"),
            changes.silently_kept
        ));
    }
    if changes.unchanged > 0 {
        summary.push_str(&format!(
            "    {} {} unchanged\n",
            style.dim("·"),
            changes.unchanged
        ));
    }
    summary.push('\n');
    run.say(&summary)?;

    run.list_proposed(&changes, visible_deleted)?;

    if options.dry_run {
        run.say(&format!(
            "  {} — no files written.\n\n",
            style.yellow("Dry run")
        ))?;
        return Ok(0);
    }

    if !run.env.interactive
        && !options.assume_yes
        && changes.new.len() + changes.conflicts.len() > 0
    {
        put(
            run.err,
            format!(
                "  {}: Cannot run interactively (not a TTY).\n  Use {} to add new files and apply auto-updates\n  (conflicts are always skipped non-interactively).\n  Use {} to preview.\n",
                style.red("Error"),
                style.cyan("--yes"),
                style.cyan("--dry-run")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }

    let (mut added, mut updated, mut auto_applied, mut skipped) = (0, 0, 0, 0);
    let mut cancelled = false;

    for entry in &changes.auto {
        run.copy(entry)?;
        run.say(&format!(
            "  {} {}  {}\n",
            style.cyan("↑"),
            entry.rel,
            style.dim("(auto-updated; you hadn't touched it)")
        ))?;
        auto_applied += 1;
    }

    if visible_deleted {
        for entry in &changes.deleted {
            if cancelled {
                break;
            }
            if options.assume_yes {
                run.say(&format!(
                    "  {} {} {}\n",
                    style.dim("?"),
                    entry.rel,
                    style.dim("(previously declined — skipped under --yes; run interactively)")
                ))?;
                skipped += 1;
                continue;
            }
            match run.prompt_deleted(entry)? {
                'a' => {
                    run.copy(entry)?;
                    run.say(&format!("    {}\n", style.green("restored.")))?;
                    added += 1;
                }
                'q' => cancelled = true,
                _ => {
                    run.say(&format!("    {}\n", style.dim("still declined.")))?;
                    skipped += 1;
                }
            }
        }
    }

    if !changes.new.is_empty() && !cancelled {
        for entry in &changes.new {
            if cancelled {
                break;
            }
            if options.assume_yes {
                run.copy(entry)?;
                run.say(&format!("  {} {}\n", style.green("+"), entry.rel))?;
                added += 1;
                continue;
            }
            match run.prompt_new(entry)? {
                'a' => {
                    run.copy(entry)?;
                    run.say(&format!("    {}\n", style.green("added.")))?;
                    added += 1;
                }
                'q' => cancelled = true,
                _ => {
                    // Skip-as-decline: recorded so the file never reappears as NEW.
                    run.manifest.record(&entry.rel, &entry.hash);
                    run.say(&format!(
                        "    {}\n",
                        style.dim(
                            "declined (will not be offered again — use --include-deleted to revisit)."
                        )
                    ))?;
                    skipped += 1;
                }
            }
        }
    }

    if !changes.conflicts.is_empty() && !cancelled {
        for entry in &changes.conflicts {
            if cancelled {
                break;
            }
            if options.assume_yes {
                run.say(&format!(
                    "  {} {} {}\n",
                    style.yellow("~"),
                    entry.rel,
                    style.dim("(conflict — skipped; run interactively to review)")
                ))?;
                skipped += 1;
                continue;
            }
            match run.prompt_conflict(entry)? {
                'u' => {
                    run.copy(entry)?;
                    run.say(&format!("    {}\n", style.yellow("updated.")))?;
                    updated += 1;
                }
                'q' => cancelled = true,
                _ => {
                    // Recorded at the new template hash so the skip is remembered.
                    run.manifest.record(&entry.rel, &entry.hash);
                    run.say(&format!(
                        "    {}\n",
                        style.dim("skipped (remembered — agentsync refresh --review to revisit).")
                    ))?;
                    skipped += 1;
                }
            }
        }
    }

    run.heal(&templates);
    run.manifest.write(Path::new(root))?;

    let mut closing = String::from("\n");
    if cancelled {
        closing.push_str(&format!(
            "  {} Files already applied are kept.\n",
            style.yellow("Cancelled.")
        ));
    }
    closing.push_str(&format!(
        "  {} Added: {added} · Auto-updated: {auto_applied} · Updated: {updated} · Skipped: {skipped} · Unchanged: {}\n",
        style.green("Done."),
        changes.unchanged
    ));
    if added + auto_applied + updated > 0 {
        closing.push_str(&format!(
            "\n  Next: {} to distribute the updates to enabled tools.\n",
            style.cyan("agentsync sync")
        ));
    }
    closing.push('\n');
    run.say(&closing)?;
    Ok(0)
}

fn parse_args(
    args: &[String],
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Result<Options, u8>, Error> {
    let mut options = Options {
        dry_run: false,
        assume_yes: false,
        include_agents_md: false,
        include_deleted: false,
        review: false,
        status_only: false,
        only: String::new(),
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            "--yes" | "-y" => options.assume_yes = true,
            "--include-agents-md" => options.include_agents_md = true,
            "--include-deleted" => options.include_deleted = true,
            "--review" => options.review = true,
            "--status" => options.status_only = true,
            "--only" => match rest.next() {
                Some(value) => options.only = value.clone(),
                None => {
                    put(
                        err,
                        format!("{}: --only requires a value\n", style.red("Error")).as_bytes(),
                    )?;
                    return Ok(Err(1));
                }
            },
            "--help" | "-h" => {
                put(out, usage(style).as_bytes())?;
                return Ok(Err(0));
            }
            flag if flag.starts_with("--only=") => {
                options.only = flag["--only=".len()..].to_string();
            }
            flag if flag.starts_with('-') => {
                put(
                    err,
                    format!(
                        "{}: Unknown option: {flag}\n{}",
                        style.red("Error"),
                        usage(style)
                    )
                    .as_bytes(),
                )?;
                return Ok(Err(1));
            }
            value => {
                put(
                    err,
                    format!(
                        "{}: Unexpected argument: {value}\n{}",
                        style.red("Error"),
                        usage(style)
                    )
                    .as_bytes(),
                )?;
                return Ok(Err(1));
            }
        }
    }
    Ok(Ok(options))
}

/// `_refresh_resolve_scope`: the categories present under `user_base`, or the
/// validated, deduplicated `--only` list in the order given.
fn resolve_scope(
    only: &str,
    user_base: &str,
    style: &Style,
    err: &mut dyn Write,
) -> Result<Result<Vec<String>, u8>, Error> {
    if only.is_empty() {
        let found: Vec<String> = CATEGORIES_DEFAULT
            .iter()
            .filter(|category| Path::new(user_base).join(category).is_dir())
            .map(|category| category.to_string())
            .collect();
        if found.is_empty() {
            put(
                err,
                format!(
                    "{}: No source content categories present in {user_base}.\nPass {} to opt into specific ones,\nor run {} to scaffold them.\n",
                    style.red("Error"),
                    style.cyan("--only rules,skills,commands,agents"),
                    style.cyan("agentsync init")
                )
                .as_bytes(),
            )?;
            return Ok(Err(1));
        }
        return Ok(Ok(found));
    }
    let mut categories: Vec<String> = Vec::new();
    for token in only.split(',') {
        let token = token.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
        if token.is_empty() {
            continue;
        }
        // `subagents` is the init/--content token; the directory is `agents`.
        let token = if token == "subagents" {
            "agents"
        } else {
            token
        };
        if !CATEGORIES_VALID.contains(&token) {
            put(
                err,
                format!(
                    "{}: Unknown --only value: {token}\nValid: rules, skills, commands, agents (or subagents)\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(Err(1));
        }
        if !categories.iter().any(|known| known == token) {
            categories.push(token.to_string());
        }
    }
    if categories.is_empty() {
        put(
            err,
            format!(
                "{}: --only must include at least one category\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(Err(1));
    }
    Ok(Ok(categories))
}

/// `_refresh_load_overrides`: `template_overrides.declined` and `.pinned` from
/// `.ai/agent_sync.yaml`, else a root `agent_sync.yaml`.
fn load_overrides(root: &str) -> (Vec<String>, Vec<String>) {
    let text = [
        format!("{root}/.ai/agent_sync.yaml"),
        format!("{root}/agent_sync.yaml"),
    ]
    .into_iter()
    .find(|path| Path::new(path).is_file())
    .and_then(|path| std::fs::read(path).ok())
    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    .unwrap_or_default();
    let list = |key: &str| -> Vec<String> {
        yaml_subset::list(&text, key)
            .into_iter()
            .filter(|item| !item.is_empty())
            .collect()
    };
    (
        list("template_overrides.declined"),
        list("template_overrides.pinned"),
    )
}

/// `_refresh_collect_changes`: `AGENTS.md` when asked, then the `*.md` files of
/// `rules`, `commands`, and `agents`, then every file below `skills`.
#[allow(clippy::too_many_arguments)]
fn collect(
    templates: &[(String, &'static [u8])],
    user_base: &Path,
    categories: &[String],
    include_agents_md: bool,
    manifest: &TemplateManifest,
    declined: &[String],
    pinned: &[String],
    review: bool,
) -> Changes {
    let mut classifier = Classifier {
        user_base,
        manifest,
        declined,
        pinned,
        review,
        changes: Changes::default(),
    };
    let in_scope = |category: &str| categories.iter().any(|c| c == category);
    if include_agents_md {
        for (rel, bytes) in templates.iter().filter(|(rel, _)| rel == "AGENTS.md") {
            classifier.classify(rel, bytes);
        }
    }
    for category in ["rules", "commands", "agents"] {
        if !in_scope(category) {
            continue;
        }
        for (rel, bytes) in templates
            .iter()
            .filter(|(rel, _)| rel.rsplit_once('/').is_some_and(|(dir, _)| dir == category))
        {
            classifier.classify(rel, bytes);
        }
    }
    if in_scope("skills") {
        for (rel, bytes) in templates
            .iter()
            .filter(|(rel, _)| rel.starts_with("skills/"))
        {
            classifier.classify(rel, bytes);
        }
    }
    classifier.changes
}

impl Run<'_, '_> {
    fn say(&mut self, text: &str) -> Result<(), Error> {
        put(self.out, text.as_bytes())
    }

    fn tell(&mut self, text: &str) -> Result<(), Error> {
        put(self.err, text.as_bytes())
    }

    /// `_refresh_heal_unchanged`.
    fn heal(&mut self, templates: &[(String, &'static [u8])]) {
        self.manifest.heal_from_match(
            templates.iter().map(|(rel, bytes)| (rel.as_str(), *bytes)),
            &self.user_base,
        );
    }

    /// `_refresh_copy` followed by `template_manifest_record`.
    fn copy(&mut self, entry: &Candidate) -> Result<(), Error> {
        write_template(&self.user_base.join(&entry.rel), entry.bytes)?;
        self.manifest.record(&entry.rel, &entry.hash);
        Ok(())
    }

    /// `_refresh_list_proposed`.
    fn list_proposed(&mut self, changes: &Changes, visible_deleted: bool) -> Result<(), Error> {
        let style = self.style;
        let mut text = String::new();
        let mut section = |title: String, marker: String, entries: &[Candidate]| {
            if entries.is_empty() {
                return;
            }
            text.push_str(&format!("  {title}\n"));
            for entry in entries {
                text.push_str(&format!("    {marker} {}\n", entry.rel));
            }
            text.push('\n');
        };
        section(style.green("New:"), style.green("+"), &changes.new);
        section(
            format!(
                "{} {}",
                style.cyan("Auto-update:"),
                style.dim("(your version matches the previous template; safe to update)")
            ),
            style.cyan("↑"),
            &changes.auto,
        );
        section(
            format!(
                "{} {}",
                style.yellow("Conflicts:"),
                style.dim(
                    "(both your version and the template diverged from the recorded baseline)"
                )
            ),
            style.yellow("~"),
            &changes.conflicts,
        );
        if visible_deleted {
            section(
                style.dim("Previously declined:"),
                style.dim("?"),
                &changes.deleted,
            );
        }
        self.say(&text)
    }

    /// `_refresh_print_status`.
    fn print_status(&mut self, declined: &[String], deleted: &[Candidate]) -> Result<(), Error> {
        let style = self.style;
        let mut text = format!("\n{}\n", style.bold("  Declined templates"));
        if declined.is_empty() && deleted.is_empty() {
            text.push_str(&format!("  {}\n\n", style.dim("Nothing declined.")));
            return self.say(&text);
        }
        if !declined.is_empty() {
            text.push_str(&format!(
                "  {}  {}\n",
                style.yellow("Persistent"),
                style.dim("(template_overrides.declined in agent_sync.yaml — never offered):")
            ));
            for item in declined {
                text.push_str(&format!("    {} {item}\n", style.dim("·")));
            }
            text.push('\n');
        }
        if !deleted.is_empty() {
            text.push_str(&format!(
                "  {}       {}\n",
                style.yellow("Local"),
                style
                    .dim("(.template-manifest — deleted from disk; --include-deleted to restore):")
            ));
            for entry in deleted {
                text.push_str(&format!("    {} {}\n", style.dim("·"), entry.rel));
            }
            text.push('\n');
        }
        self.say(&text)
    }

    /// `read -r reply </dev/tty`, lowercased, `s` when empty.
    fn answer(&mut self) -> String {
        let reply = (self.env.read_line)();
        let reply = reply.trim_matches([' ', '\t']).to_lowercase();
        if reply.is_empty() {
            "s".to_string()
        } else {
            reply
        }
    }

    /// `_refresh_prompt_new`: `a`, `s`, or `q`.
    fn prompt_new(&mut self, entry: &Candidate) -> Result<char, Error> {
        let style = self.style;
        let banner = format!("\n  {} {}\n", style.green("+ NEW:"), style.cyan(&entry.rel));
        self.prompt_add(entry, &banner)
    }

    /// `_refresh_prompt_deleted`: `a`, `s`, or `q`.
    fn prompt_deleted(&mut self, entry: &Candidate) -> Result<char, Error> {
        let style = self.style;
        let banner = format!(
            "\n  {} {}  {}\n",
            style.dim("? RESTORE:"),
            style.cyan(&entry.rel),
            style.dim("(previously declined)")
        );
        self.prompt_add(entry, &banner)
    }

    fn prompt_add(&mut self, entry: &Candidate, banner: &str) -> Result<char, Error> {
        let style = self.style;
        loop {
            self.tell(&format!(
                "{banner}    [{}]dd  [{}]kip  [v]iew  [q]uit  > ",
                style.green("a"),
                style.yellow("s")
            ))?;
            match self.answer().as_str() {
                "a" | "add" => return Ok('a'),
                "s" | "skip" => return Ok('s'),
                "v" | "view" => self.show_new(entry.bytes)?,
                "q" | "quit" => return Ok('q'),
                _ => self.tell(&format!(
                    "    {}\n",
                    style.dim("(unknown choice — try a, s, v, q)")
                ))?,
            }
        }
    }

    /// `_refresh_prompt_conflict`: `u`, `s`, or `q`.
    fn prompt_conflict(&mut self, entry: &Candidate) -> Result<char, Error> {
        let style = self.style;
        loop {
            self.tell(&format!(
                "\n  {} {}\n    [{}]pdate  [{}]kip  [v]iew  [q]uit  > ",
                style.yellow("~ CONFLICT:"),
                style.cyan(&entry.rel),
                style.yellow("u"),
                style.yellow("s")
            ))?;
            match self.answer().as_str() {
                "u" | "update" => return Ok('u'),
                "s" | "skip" => return Ok('s'),
                "v" | "view" => {
                    let dest = self.user_base.join(&entry.rel);
                    self.show_diff(&dest, entry.bytes)?;
                }
                "q" | "quit" => return Ok('q'),
                _ => self.tell(&format!(
                    "    {}\n",
                    style.dim("(unknown choice — try u, s, v, q)")
                ))?,
            }
        }
    }

    /// `_refresh_show_new`: the template, each line indented, on stderr.
    fn show_new(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let style = self.style;
        let mut text = format!("\n  {}\n\n", style.dim("──── new file content ────"));
        let content = String::from_utf8_lossy(bytes);
        let mut lines: Vec<&str> = content.split('\n').collect();
        if content.ends_with('\n') || content.is_empty() {
            lines.pop();
        }
        for line in lines {
            text.push_str(&format!("  {line}\n"));
        }
        text.push('\n');
        self.tell(&text)
    }

    /// `_refresh_show_diff`: `diff -u --label yours --label template`, on stderr.
    fn show_diff(&mut self, dest: &Path, template: &[u8]) -> Result<(), Error> {
        let style = self.style;
        let mut text = format!("\n  {}\n\n", style.dim("──── diff: yours → template ────"));
        match unified_diff(dest, template) {
            Some(hunks) => text.push_str(&String::from_utf8_lossy(&hunks)),
            None => text.push_str(&format!(
                "  {}\n",
                style.red("(diff command not available)")
            )),
        }
        text.push('\n');
        self.tell(&text)
    }
}

/// `diff -u --label yours --label template <yours> <template>`, the embedded
/// template written to a temporary file for the call; `None` when `diff`
/// cannot start.
fn unified_diff(yours: &Path, template: &[u8]) -> Option<Vec<u8>> {
    let staged = stage_template(template)?;
    let output = Command::new("diff")
        .args(["-u", "--label", "yours", "--label", "template"])
        .arg(yours)
        .arg(&staged)
        .stdin(Stdio::null())
        .output();
    let _ = std::fs::remove_file(&staged);
    let output = output.ok()?;
    let mut text = output.stdout;
    text.extend_from_slice(&output.stderr);
    Some(text)
}

fn stage_template(bytes: &[u8]) -> Option<PathBuf> {
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    for attempt in 0u32..100 {
        let path = dir.join(format!("agentsync-refresh-{pid}-{attempt}"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        match options.open(&path) {
            Ok(mut file) => {
                if file.write_all(bytes).is_err() {
                    let _ = std::fs::remove_file(&path);
                    return None;
                }
                return Some(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// `mkdir -p` and `cp <template> <dest>`. An existing file keeps its mode; a new
/// one is created executable when the template starts with `#!`, the mode the
/// shipped scripts carry in the checkout `cp` copied from.
fn write_template(dest: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if bytes.starts_with(b"#!") {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o755);
    }
    options
        .open(dest)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|e| Error::io(dest, e))
}

/// `_refresh_usage`.
fn usage(style: &Style) -> String {
    format!(
        "\n  {} — pull new template files into an existing .ai/src/\n\n  {}\n    agentsync refresh [options]\n\n  {}\n    Compares each shipped template (rules, skills, commands, agents) against\n    your local .ai/src/ using a three-way diff (template-old vs template-new\n    vs your current file) when a template manifest is present. Files you\n    haven't touched auto-update silently; only true conflicts require review.\n    Files in .ai/src/ that aren't part of the templates (your custom content)\n    are left alone.\n\n  {}\n    --only <csv>           Categories to consider: rules, skills, commands, agents\n                           Default: only categories that already have a subdir\n                           in your .ai/src/. Pass --only to opt into a category\n                           you don't have yet.\n    --include-agents-md    Also offer updates to AGENTS.md (off by default —\n                           almost always heavily customized).\n    --include-deleted      Re-offer files you previously declined and removed\n                           from disk so they can be restored.\n    --review               Resurface every local divergence from the shipped\n                           templates, including conflicts you previously\n                           [s]kipped. Use this to revisit earlier decisions\n                           or audit local edits.\n    --status               Print declined breakdown (persistent + local) and\n                           exit. No mutation, no prompts.\n    --dry-run              Print the plan without writing anything.\n    -y, --yes              Apply auto-updates and add new files; skip conflicts\n                           (no prompts). Required in non-interactive contexts.\n    -h, --help             Show this help.\n\n  {}\n    Picking {} on a conflict records the current template hash in\n    .ai/.template-manifest. The divergence stays silent on future refreshes\n    until a newer template ships (at which point it resurfaces automatically\n    so you can review the new change). Pass {} at any time to\n    revisit your skips explicitly.\n\n  {}\n    Edit .ai/agent_sync.yaml to silence specific templates forever (this is\n    stronger than {} — even new template versions stay hidden):\n\n      template_overrides:\n        declined:        # always-skip; never offered\n          - rules/some-rule.md\n        pinned:          # ignore template updates; keep your version\n          - rules/my-version.md\n\n  {}\n    agentsync refresh\n    agentsync refresh --only rules,skills\n    agentsync refresh --dry-run\n    agentsync refresh --yes               # CI-friendly: auto-update + add new\n    agentsync refresh --include-deleted   # revisit previously declined files\n    agentsync refresh --review            # revisit conflicts you skipped\n",
        style.bold("agentsync refresh"),
        style.green("USAGE"),
        style.green("DESCRIPTION"),
        style.green("OPTIONS"),
        style.green("REMEMBERED SKIPS"),
        style.yellow("[s]kip"),
        style.cyan("--review"),
        style.green("PERSISTENT OVERRIDES"),
        style.yellow("[s]kip"),
        style.green("EXAMPLES")
    )
}

#[cfg(all(test, unix))]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::template_manifest::REL;

    /// A project `init` scaffolded: every template under `.ai/src/` and a
    /// manifest recording each hash.
    fn seeded() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let base = Path::new(&root).join(".ai/src");
        let mut manifest = TemplateManifest::default();
        for (rel, bytes) in catalog::template_files() {
            let path = base.join(&rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            manifest.record(&rel, &sha256_hex(bytes));
        }
        std::fs::write(
            Path::new(&root).join(".ai/agent_sync.yaml"),
            "tools:\n  enabled: []\n",
        )
        .unwrap();
        manifest.write(Path::new(&root)).unwrap();
        (dir, root)
    }

    struct Outcome {
        status: u8,
        out: String,
        err: String,
    }

    fn call(root: &str, args: &[&str], interactive: bool, replies: &[&str]) -> Outcome {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut queue: VecDeque<String> = replies.iter().map(|r| r.to_string()).collect();
        let mut read_line = || queue.pop_front().unwrap_or_default();
        let mut env = Env {
            interactive,
            read_line: &mut read_line,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = refresh(&args, root, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
        Outcome {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    fn manifest_text(root: &str) -> String {
        std::fs::read_to_string(Path::new(root).join(REL)).unwrap_or_default()
    }

    fn drop_entry(root: &str, rel: &str) {
        let kept: String = manifest_text(root)
            .lines()
            .filter(|line| !line.starts_with(&format!("{rel}\t")))
            .map(|line| format!("{line}\n"))
            .collect();
        std::fs::write(Path::new(root).join(REL), kept).unwrap();
    }

    fn set_entry(root: &str, rel: &str, hash: &str) {
        drop_entry(root, rel);
        let mut lines: Vec<String> = manifest_text(root).lines().map(str::to_string).collect();
        lines.push(format!("{rel}\t{hash}"));
        lines.sort();
        std::fs::write(
            Path::new(&root).join(REL),
            lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
        )
        .unwrap();
    }

    fn append(root: &str, rel: &str, text: &str) {
        let path = Path::new(root).join(".ai/src").join(rel);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    fn header(root: &str, scope: &str) -> String {
        format!(
            "\n  AgentSync Refresh\n\n  Templates: /<agentsync>/lib/templates\n  Project:   {root}/.ai/src\n  Scope:     {scope}\n\n"
        )
    }

    const NOT_A_TTY: &str = "  Error: Cannot run interactively (not a TTY).\n  Use --yes to add new files and apply auto-updates\n  (conflicts are always skipped non-interactively).\n  Use --dry-run to preview.\n";

    #[test]
    fn arguments_are_refused_and_help_prints_like_bash() {
        let (_dir, root) = seeded();
        let help = call(&root, &["--help", "--bogus"], false, &[]);
        assert_eq!(help.status, 0);
        assert!(help.out.starts_with(
            "\n  agentsync refresh — pull new template files into an existing .ai/src/\n\n  USAGE\n    agentsync refresh [options]\n\n  DESCRIPTION\n"
        ));
        assert!(
            help.out
                .contains("\n  REMEMBERED SKIPS\n    Picking [s]kip on a conflict")
        );
        assert!(help.out.contains("\n  PERSISTENT OVERRIDES\n"));
        assert!(help.out.ends_with(
            "    agentsync refresh --review            # revisit conflicts you skipped\n"
        ));
        assert_eq!(help.out.lines().count(), 58);

        let bogus = call(&root, &["--bogus", "--help"], false, &[]);
        assert_eq!((bogus.status, bogus.out.as_str()), (1, ""));
        assert_eq!(
            bogus.err,
            format!("Error: Unknown option: --bogus\n{}", help.out)
        );
        let extra = call(&root, &["extra"], false, &[]);
        assert_eq!(
            extra.err,
            format!("Error: Unexpected argument: extra\n{}", help.out)
        );
        let missing = call(&root, &["--yes", "--only"], false, &[]);
        assert_eq!(
            (missing.status, missing.err.as_str()),
            (1, "Error: --only requires a value\n")
        );
        assert_eq!(
            call(&root, &["--yes", "--only", "bogus"], false, &[]).err,
            "Error: Unknown --only value: bogus\nValid: rules, skills, commands, agents (or subagents)\n"
        );
        assert_eq!(
            call(&root, &["--yes", "--only", ","], false, &[]).err,
            "Error: --only must include at least one category\n"
        );
        assert_eq!(manifest_text(&root).lines().count(), 19);
    }

    #[test]
    fn an_untouched_project_is_up_to_date_and_status_reports_nothing_declined() {
        let (_dir, root) = seeded();
        let before = manifest_text(&root);
        let run = call(&root, &["--yes"], false, &[]);
        assert_eq!(run.status, 0);
        assert_eq!(
            run.out,
            format!(
                "{}  Already up to date! 18 file(s) match the current templates.\n\n",
                header(&root, "rules,skills,commands,agents")
            )
        );
        assert_eq!(run.err, "");
        assert_eq!(manifest_text(&root), before);
        assert_eq!(
            call(&root, &["--status"], false, &[]).out,
            "\n  Declined templates\n  Nothing declined.\n\n"
        );

        std::fs::rename(
            Path::new(&root).join(".ai/src"),
            Path::new(&root).join("legacy"),
        )
        .unwrap();
        for entry in std::fs::read_dir(Path::new(&root).join("legacy")).unwrap() {
            let entry = entry.unwrap();
            std::fs::rename(
                entry.path(),
                Path::new(&root).join(".ai").join(entry.file_name()),
            )
            .unwrap();
        }
        let legacy = call(&root, &["--yes"], false, &[]);
        assert!(legacy.out.contains(&format!("  Project:   {root}/.ai\n")));
        assert!(legacy.out.contains("Already up to date! 18 file(s)"));

        std::fs::remove_dir_all(Path::new(&root).join(".ai")).unwrap();
        let gone = call(&root, &["--status"], false, &[]);
        assert_eq!(
            (gone.status, gone.out.as_str(), gone.err),
            (
                1,
                "",
                format!("Error: No .ai/ directory found in {root}\nRun agentsync init first.\n")
            )
        );
    }

    #[test]
    fn new_deleted_auto_update_and_conflict_files_classify_and_apply_like_bash() {
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_file(base.join("rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        append(&root, "rules/core.md", "USER LOCAL EDIT\n");
        set_entry(
            &root,
            "rules/core.md",
            &template_manifest::hash(&base.join("rules/core.md")).unwrap(),
        );
        append(&root, "rules/git.md", "EDIT\n");
        drop_entry(&root, "rules/git.md");
        std::fs::remove_file(base.join("commands/review.md")).unwrap();

        let plan = "  Summary:\n    + 1 new template(s)\n    ↑ 1 auto-update(s) — you hadn't touched them locally\n    ~ 1 conflict(s) — your version differs from the template\n    ? 1 previously declined — pass --include-deleted to revisit\n    · 14 unchanged\n\n  New:\n    + rules/comments.md\n\n  Auto-update: (your version matches the previous template; safe to update)\n    ↑ rules/core.md\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ rules/git.md\n\n  Previously declined:\n    ? commands/review.md\n\n";
        let head = header(&root, "rules,skills,commands,agents");

        let dry = call(&root, &["--dry-run", "--include-deleted"], false, &[]);
        assert_eq!(
            (dry.status, dry.out),
            (0, format!("{head}{plan}  Dry run — no files written.\n\n"))
        );
        assert!(!base.join("rules/comments.md").exists());

        let blocked = call(&root, &["--include-deleted"], false, &[]);
        assert_eq!(
            (blocked.status, blocked.out, blocked.err.as_str()),
            (1, format!("{head}{plan}"), NOT_A_TTY)
        );

        assert_eq!(
            call(&root, &["--status"], false, &[]).out,
            "\n  Declined templates\n  Local       (.template-manifest — deleted from disk; --include-deleted to restore):\n    · commands/review.md\n\n"
        );

        let applied = call(
            &root,
            &["--yes", "--include-deleted", "--review"],
            false,
            &[],
        );
        assert_eq!(
            (applied.status, applied.out),
            (
                0,
                format!(
                    "{head}{plan}  ↑ rules/core.md  (auto-updated; you hadn't touched it)\n  ? commands/review.md (previously declined — skipped under --yes; run interactively)\n  + rules/comments.md\n  ~ rules/git.md (conflict — skipped; run interactively to review)\n\n  Done. Added: 1 · Auto-updated: 1 · Updated: 0 · Skipped: 2 · Unchanged: 14\n\n  Next: agentsync sync to distribute the updates to enabled tools.\n\n"
                )
            )
        );
        let template = |rel: &str| {
            catalog::template_files()
                .into_iter()
                .find(|(path, _)| path == rel)
                .map(|(_, bytes)| bytes.to_vec())
                .unwrap()
        };
        assert_eq!(
            std::fs::read(base.join("rules/comments.md")).unwrap(),
            template("rules/comments.md")
        );
        assert_eq!(
            std::fs::read(base.join("rules/core.md")).unwrap(),
            template("rules/core.md")
        );
        assert!(
            std::fs::read_to_string(base.join("rules/git.md"))
                .unwrap()
                .ends_with("EDIT\n")
        );
        assert!(!base.join("commands/review.md").exists());
        let manifest = TemplateManifest::load(Path::new(&root)).unwrap();
        assert_eq!(
            manifest.lookup("rules/comments.md"),
            Some(sha256_hex(&template("rules/comments.md")).as_str())
        );
        assert_eq!(
            manifest.lookup("rules/core.md"),
            Some(sha256_hex(&template("rules/core.md")).as_str())
        );
        assert_eq!(manifest.lookup("rules/git.md"), None);
        assert!(manifest.lookup("commands/review.md").is_some());

        let again = call(&root, &["--yes"], false, &[]);
        assert_eq!(
            again.out,
            format!(
                "{head}  Summary:\n    ~ 1 conflict(s) — your version differs from the template\n    · 16 unchanged\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ rules/git.md\n\n  ~ rules/git.md (conflict — skipped; run interactively to review)\n\n  Done. Added: 0 · Auto-updated: 0 · Updated: 0 · Skipped: 1 · Unchanged: 16\n\n"
            )
        );
    }

    #[test]
    fn silently_kept_edits_and_deleted_files_show_in_the_up_to_date_summary() {
        let (_dir, root) = seeded();
        append(&root, "rules/core.md", "USER LOCAL EDIT\n");
        std::fs::remove_file(Path::new(&root).join(".ai/src/commands/review.md")).unwrap();
        let head = header(&root, "rules,skills,commands,agents");
        assert_eq!(
            call(&root, &["--yes"], false, &[]).out,
            format!(
                "{head}  Already up to date! 16 file(s) match the current templates.\n  Locally declined (.template-manifest):    1 file(s); --include-deleted to revisit.\n  Pass --status for the full list.\n  1 file(s) differ from the shipped template (local edits or earlier skips); pass --review to revisit.\n\n"
            )
        );
        let review = call(&root, &["--review", "--dry-run"], false, &[]);
        assert_eq!(
            review.out,
            format!(
                "{head}  Summary:\n    ~ 1 conflict(s) — your version differs from the template\n    · 16 unchanged\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ rules/core.md\n\n  Dry run — no files written.\n\n"
            )
        );
        let review = call(&root, &["--review"], false, &[]);
        assert_eq!((review.status, review.err.as_str()), (1, NOT_A_TTY));
        assert!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/rules/core.md"))
                .unwrap()
                .ends_with("USER LOCAL EDIT\n")
        );

        append(&root, "AGENTS.md", "USER LOCAL EDIT\n");
        drop_entry(&root, "AGENTS.md");
        let agents = call(&root, &["--yes", "--include-agents-md"], false, &[]);
        assert_eq!(
            agents.out,
            format!(
                "{}  Summary:\n    ~ 1 conflict(s) — your version differs from the template\n    · 1 silently kept (local edits or earlier skips) — pass --review to revisit\n    · 16 unchanged\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ AGENTS.md\n\n  ~ AGENTS.md (conflict — skipped; run interactively to review)\n\n  Done. Added: 0 · Auto-updated: 0 · Updated: 0 · Skipped: 1 · Unchanged: 16\n\n",
                header(&root, "rules,skills,commands,agents,AGENTS.md")
            )
        );
    }

    #[test]
    fn declined_and_pinned_overrides_silence_templates_and_status_lists_them() {
        let (_dir, root) = seeded();
        let config = Path::new(&root).join(".ai/agent_sync.yaml");
        std::fs::write(
            &config,
            "tools:\n  enabled: []\n\ntemplate_overrides:\n  declined:\n    - rules/comments.md\n    - rules/git.md\n  pinned:\n    - rules/core.md\n",
        )
        .unwrap();
        std::fs::remove_file(Path::new(&root).join(".ai/src/rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        append(&root, "rules/core.md", "USER LOCAL EDIT\n");
        drop_entry(&root, "rules/core.md");
        assert_eq!(
            call(&root, &["--status"], false, &[]).out,
            "\n  Declined templates\n  Persistent  (template_overrides.declined in agent_sync.yaml — never offered):\n    · rules/comments.md\n    · rules/git.md\n\n"
        );
        assert_eq!(
            call(&root, &["--yes"], false, &[]).out,
            format!(
                "{}  Already up to date! 15 file(s) match the current templates.\n  Persistently declined (agent_sync.yaml): 2 file(s).\n  Pass --status for the full list.\n\n",
                header(&root, "rules,skills,commands,agents")
            )
        );
        assert!(!Path::new(&root).join(".ai/src/rules/comments.md").exists());
        assert_eq!(
            TemplateManifest::load(Path::new(&root))
                .unwrap()
                .lookup("rules/core.md"),
            None
        );
        // A declined template that was recorded and then removed is not a local decline.
        std::fs::remove_file(Path::new(&root).join(".ai/src/rules/git.md")).unwrap();
        assert!(
            !call(&root, &["--yes"], false, &[])
                .out
                .contains("Locally declined")
        );
    }

    #[test]
    fn prompts_restore_add_update_skip_view_and_quit_like_bash() {
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_file(base.join("rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        std::fs::remove_file(base.join("commands/review.md")).unwrap();
        append(&root, "rules/git.md", "EDIT\n");
        drop_entry(&root, "rules/git.md");

        let run = call(
            &root,
            &["--include-deleted"],
            true,
            &["v", " A ", "x", "", "view", "Update"],
        );
        assert_eq!(run.status, 0);
        let review = catalog::template_files()
            .into_iter()
            .find(|(path, _)| path == "commands/review.md")
            .map(|(_, bytes)| String::from_utf8(bytes.to_vec()).unwrap())
            .unwrap();
        let shown: String = review.lines().map(|line| format!("  {line}\n")).collect();
        let restore = "\n  ? RESTORE: commands/review.md  (previously declined)\n    [a]dd  [s]kip  [v]iew  [q]uit  > ";
        let new = "\n  + NEW: rules/comments.md\n    [a]dd  [s]kip  [v]iew  [q]uit  > ";
        let conflict = "\n  ~ CONFLICT: rules/git.md\n    [u]pdate  [s]kip  [v]iew  [q]uit  > ";
        let expected_err = format!(
            "{restore}\n  ──── new file content ────\n\n{shown}\n{restore}{new}    (unknown choice — try a, s, v, q)\n{new}{conflict}\n  ──── diff: yours → template ────\n\n--- yours\n+++ template\n@@ "
        );
        assert!(
            run.err.starts_with(&expected_err),
            "stderr was:\n{}",
            run.err
        );
        assert!(run.err.contains("\n-EDIT\n"));
        assert!(run.err.ends_with(&format!("\n{conflict}")));
        assert!(run.out.ends_with(
            "    restored.\n    declined (will not be offered again — use --include-deleted to revisit).\n    updated.\n\n  Done. Added: 1 · Auto-updated: 0 · Updated: 1 · Skipped: 1 · Unchanged: 15\n\n  Next: agentsync sync to distribute the updates to enabled tools.\n\n"
        ));
        assert!(base.join("commands/review.md").is_file());
        assert!(!base.join("rules/comments.md").exists());
        let manifest = TemplateManifest::load(Path::new(&root)).unwrap();
        assert!(manifest.lookup("rules/comments.md").is_some());
        assert_eq!(
            std::fs::read_to_string(base.join("rules/git.md")).unwrap(),
            String::from_utf8(
                catalog::template_files()
                    .into_iter()
                    .find(|(path, _)| path == "rules/git.md")
                    .unwrap()
                    .1
                    .to_vec()
            )
            .unwrap()
        );

        let (_dir, root) = seeded();
        std::fs::remove_file(Path::new(&root).join(".ai/src/rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        append(&root, "rules/git.md", "EDIT\n");
        drop_entry(&root, "rules/git.md");
        let quit = call(&root, &[], true, &["skip", "q"]);
        assert_eq!(quit.status, 0);
        assert!(quit.out.ends_with(
            "    declined (will not be offered again — use --include-deleted to revisit).\n\n  Cancelled. Files already applied are kept.\n  Done. Added: 0 · Auto-updated: 0 · Updated: 0 · Skipped: 1 · Unchanged: 16\n\n"
        ));
        assert!(
            quit.err.ends_with(
                "\n  ~ CONFLICT: rules/git.md\n    [u]pdate  [s]kip  [v]iew  [q]uit  > "
            )
        );
        let manifest = TemplateManifest::load(Path::new(&root)).unwrap();
        assert!(manifest.lookup("rules/comments.md").is_some());
        assert_eq!(manifest.lookup("rules/git.md"), None);
    }

    #[test]
    fn scope_follows_only_and_present_directories_and_heals_every_category() {
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_file(base.join("rules/comments.md")).unwrap();
        std::fs::remove_dir_all(base.join("skills/comments")).unwrap();
        std::fs::remove_dir_all(base.join("agents")).unwrap();
        std::fs::remove_file(Path::new(&root).join(REL)).unwrap();

        let rules = call(&root, &["--yes", "--only", "rules"], false, &[]);
        assert_eq!(
            rules.out,
            format!(
                "{}  Manifest:  none — falling back to two-way diff\n\n  Summary:\n    + 1 new template(s)\n    · 2 unchanged\n\n  New:\n    + rules/comments.md\n\n  + rules/comments.md\n\n  Done. Added: 1 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 2\n\n  Next: agentsync sync to distribute the updates to enabled tools.\n\n",
                header(&root, "rules").strip_suffix('\n').unwrap()
            )
        );
        let healed = manifest_text(&root);
        assert_eq!(healed.lines().count(), 17);
        assert!(healed.contains("AGENTS.md\t"));
        assert!(healed.contains("skills/humanizer/SKILL.md\t"));
        assert!(!healed.contains("skills/comments/SKILL.md\t"));
        assert!(!healed.contains("agents/code-reviewer.md\t"));

        let spaced = call(
            &root,
            &["--yes", "--only= skills , subagents ,skills"],
            false,
            &[],
        );
        assert_eq!(
            spaced.out,
            format!(
                "{}  Summary:\n    + 2 new template(s)\n    · 11 unchanged\n\n  New:\n    + agents/code-reviewer.md\n    + skills/comments/SKILL.md\n\n  + agents/code-reviewer.md\n  + skills/comments/SKILL.md\n\n  Done. Added: 2 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 11\n\n  Next: agentsync sync to distribute the updates to enabled tools.\n\n",
                header(&root, "skills,agents")
            )
        );
        assert_eq!(manifest_text(&root).lines().count(), 19);

        std::fs::remove_dir_all(base.join("commands")).unwrap();
        let absent = call(&root, &["--yes"], false, &[]);
        assert!(absent.out.contains("  Scope:     rules,skills,agents\n"));
        assert!(absent.out.contains("Already up to date! 16 file(s)"));
        assert!(!base.join("commands").exists());

        for category in ["rules", "skills", "agents"] {
            std::fs::remove_dir_all(base.join(category)).unwrap();
        }
        let none = call(&root, &["--yes"], false, &[]);
        assert_eq!(
            (none.status, none.err),
            (
                1,
                format!(
                    "Error: No source content categories present in {root}/.ai/src.\nPass --only rules,skills,commands,agents to opt into specific ones,\nor run agentsync init to scaffold them.\n"
                )
            )
        );
        let recorded = call(&root, &["--yes", "--only", "commands"], false, &[]);
        assert_eq!(
            recorded.out,
            format!(
                "{}  Already up to date! 0 file(s) match the current templates.\n  Locally declined (.template-manifest):    2 file(s); --include-deleted to revisit.\n  Pass --status for the full list.\n\n",
                header(&root, "commands")
            )
        );
        drop_entry(&root, "commands/fix-issue.md");
        drop_entry(&root, "commands/review.md");
        let opted = call(&root, &["--yes", "--only", "commands"], false, &[]);
        assert!(opted.out.contains(
            "  Summary:\n    + 2 new template(s)\n\n  New:\n    + commands/fix-issue.md\n    + commands/review.md\n\n  + commands/fix-issue.md\n  + commands/review.md\n\n  Done. Added: 2 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 0\n"
        ));
        assert!(base.join("commands/review.md").is_file());
    }

    #[test]
    fn a_re_added_script_is_executable_and_a_missing_manifest_heals() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_dir_all(base.join("skills/humanizer/scripts")).unwrap();
        std::fs::remove_dir_all(base.join("skills/humanizer/references")).unwrap();
        std::fs::remove_file(Path::new(&root).join(REL)).unwrap();
        let run = call(&root, &["--yes"], false, &[]);
        assert!(run.out.contains(
            "  New:\n    + skills/humanizer/references/wikipedia_signs_of_ai_writing.md\n    + skills/humanizer/scripts/strip-ai-chars.sh\n\n  + skills/humanizer/references/wikipedia_signs_of_ai_writing.md\n  + skills/humanizer/scripts/strip-ai-chars.sh\n\n  Done. Added: 2 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 16\n"
        ));
        let mode = |rel: &str| {
            std::fs::metadata(base.join(rel))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode("skills/humanizer/scripts/strip-ai-chars.sh"), 0o755);
        assert_eq!(
            mode("skills/humanizer/references/wikipedia_signs_of_ai_writing.md"),
            0o644
        );
        assert_eq!(manifest_text(&root).lines().count(), 19);
        assert_eq!(
            std::fs::metadata(Path::new(&root).join(REL))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
