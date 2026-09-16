//! `agentsync init`: `cmd_init` of `lib/helpers/init.sh`, which scaffolds
//! `.ai/` inside a backup transaction, adopts the tool config a project already
//! has, writes the CI gate, and runs the first sync.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::adopt::{self, Resolver};
use super::customize::put;
use super::refresh::write_template;
use crate::interrupt::{self, Interrupt};
use crate::log::Log;
use crate::paths::{self, Paths};
use crate::project::Project;
use crate::project_config::{self, Selection};
use crate::prompts::Cancelled;
use crate::render::TARGET_KEYS;
use crate::style::Style;
use crate::template_manifest::TemplateManifest;
use crate::tool::Tool;
use crate::{Error, backup, catalog, format_rev, staging, witness};

const CONTENT_DEFAULT: &str = "agents,rules,skills,commands,subagents";
const CONTENT_VALID: [&str; 5] = ["agents", "rules", "skills", "commands", "subagents"];

/// `AGENTSYNC_REPO` of `lib/helpers/update.sh`, which the CI template's install
/// URL names.
const REPO: &str = "yelmuratoff/agent_sync";

/// `_init_detect_enabled_tools`: a tool is detected when any marker exists.
const DETECTORS: [(&str, &[&str]); 13] = [
    ("claude", &[".claude", "CLAUDE.md"]),
    ("cursor", &[".cursor", ".cursorrules"]),
    (
        "copilot",
        &[
            ".github/copilot-instructions.md",
            ".github/instructions",
            ".github/prompts",
        ],
    ),
    ("gemini", &[".gemini", "GEMINI.md"]),
    ("codex", &[".codex"]),
    ("kimi", &[".kimi-code"]),
    (
        "opencode",
        &[".opencode", "opencode.json", "opencode.jsonc"],
    ),
    ("windsurf", &[".windsurf", ".windsurfrules"]),
    ("junie", &[".junie"]),
    ("cline", &[".clinerules"]),
    ("amazonq", &[".amazonq"]),
    ("zed", &[".zed", ".rules"]),
    ("antigravity", &[".agents/rules", ".agents/workflows"]),
];

const HELP: &str = "Usage: agentsync init [<dir>] [OPTIONS]

Scaffold .ai/ in a project. Minimal by default — only tools you opt in to
get per-tool payload scaffolding (settings/mcp/hooks).

Before writing, init snapshots its .ai/ paths and the selected tools' existing
destinations under .ai/backups/ so a partial setup can be restored safely.

In a terminal, `init` opens an interactive wizard that lets you pick tools
and content sections. In non-TTY environments (CI, scripts), it runs silently
with auto-detected defaults. Pass --yes or any of --tools/--content/--no-detect
/--no-templates to skip the wizard.

Options:
  --tools <csv>        Enable these tools (e.g. claude,cursor). Unions with
                       auto-detection unless --no-detect is passed.
  --content <csv>      Which source sections to scaffold. Valid tokens:
                       agents, rules, skills, commands, subagents.
                       Default: all of them.
  --no-detect          Skip filesystem marker auto-detection (tools only).
  --outputs <mode>     Where generated tool files live. `committed` (default)
                       keeps them and .ai/.sync-manifest in git so teammates
                       need only `git pull`; `local` gitignores both and every
                       clone runs `agentsync sync`.
  --existing <action>  What to do with tool config the project already has:
                       `adopt` (default) copies it into .ai/src/ so the first
                       sync reproduces it; `replace` regenerates from the
                       shipped templates.
  --ci <provider>      Write a CI gate that runs `agentsync check`. Only
                       `github` is supported; an existing workflow is kept.
  --no-sync            Skip the first `agentsync sync` at the end.
  --no-templates       Create selected content paths without copying shipped
                       starter files. AGENTS.md is empty when agents is selected.
  -y, --yes            Skip all prompts, accept defaults.
  --dry-run            Show what would be created; don't write anything.
  -h, --help           Show this help.

Examples:
  agentsync init                           # interactive wizard (TTY)
  agentsync init --yes                     # auto-detect + defaults, no prompt
  agentsync init --tools claude            # Claude only, no detection union
  agentsync init --tools claude,cursor --content agents,rules
  agentsync init --no-detect               # no tool auto-detection; pick tools later
  agentsync init --no-templates --no-detect  # empty .ai/src/ layout, no starters
  agentsync init --dry-run                 # preview without writing
";

/// `prompt_multiselect`: title, options, preselected.
pub type Picker<'a> =
    &'a mut dyn FnMut(&str, &[String], &[String]) -> Result<Vec<String>, Cancelled>;

/// What `init` takes from the process and the terminal.
pub struct Env<'a> {
    pub version: &'a str,
    /// `$(pwd)`, spelled logically.
    pub cwd: String,
    pub config_path: Option<String>,
    pub backup_limit: Option<String>,
    pub backup_max_age: Option<String>,
    /// `is_tty`: stdin and stdout are both terminals.
    pub interactive: bool,
    /// `prompt_confirm`.
    pub confirm: &'a mut dyn FnMut(&str, bool) -> bool,
    pub multiselect: Picker<'a>,
    /// `bash "$system_dir/sync.sh"` for a root: its exit status.
    pub sync: &'a mut dyn FnMut(&str) -> u8,
}

struct Options {
    target: Option<String>,
    tools: Option<String>,
    content: Option<String>,
    no_detect: bool,
    outputs: String,
    existing: String,
    ci: String,
    run_sync: bool,
    no_templates: bool,
    assume_yes: bool,
    dry_run: bool,
}

struct Run<'a, 'b> {
    style: &'a Style,
    env: &'a mut Env<'b>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl Run<'_, '_> {
    fn say(&mut self, text: &str) -> Result<(), Error> {
        put(self.out, text.as_bytes())
    }

    fn tell(&mut self, text: &str) -> Result<(), Error> {
        put(self.err, text.as_bytes())
    }
}

pub fn init(
    args: &[String],
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut run = Run {
        style,
        env,
        out,
        err,
    };
    let options = match parse_args(args, &mut run)? {
        Ok(options) => options,
        Err(status) => return Ok(status),
    };

    if !matches!(options.outputs.as_str(), "committed" | "local") {
        run.tell(&format!(
            "{}: --outputs must be 'committed' or 'local' (got '{}')\n",
            style.red("Error"),
            options.outputs
        ))?;
        return Ok(1);
    }
    if !matches!(options.existing.as_str(), "adopt" | "replace") {
        run.tell(&format!(
            "{}: --existing must be 'adopt' or 'replace' (got '{}')\n",
            style.red("Error"),
            options.existing
        ))?;
        return Ok(1);
    }
    if !matches!(options.ci.as_str(), "" | "github") {
        run.tell(&format!(
            "{}: --ci only supports 'github' (got '{}')\n",
            style.red("Error"),
            options.ci
        ))?;
        return Ok(1);
    }

    let requested = options.target.clone().unwrap_or_else(|| ".".to_string());
    let target = if requested.starts_with('/') {
        paths::normalize(&requested)
    } else {
        paths::normalize(&format!("{}/{requested}", run.env.cwd))
    };
    if !Path::new(&target).is_dir() {
        run.tell(&format!(
            "{}: Directory not found: {requested}\n",
            style.red("Error")
        ))?;
        return Ok(1);
    }

    if let Some(project_root) = paths::ai_dir_enclosing_root(&target) {
        run.tell(&format!(
            "{}: Cannot init inside the .ai/ directory: {target}\nRun agentsync init from the project root (the parent of .ai/):\n  cd \"{project_root}\" && agentsync init\n",
            style.red("Error")
        ))?;
        return Ok(2);
    }

    let ai_dir = format!("{target}/.ai");

    let is_file = |path: &str| Path::new(path).is_file();
    let config = match project_config::select(&target, run.env.config_path.as_deref(), &is_file) {
        Selection::Found(path) => {
            let text = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
            Some((path, String::from_utf8_lossy(&text).into_owned()))
        }
        Selection::None => None,
        Selection::Missing(path) => {
            run.tell(&format!(
                "Error: {}\n",
                project_config::missing_message(&path)
            ))?;
            return Ok(1);
        }
    };
    let retention = match backup::configure(
        config
            .as_ref()
            .map(|(path, text)| (path.as_str(), text.as_str())),
        run.env.backup_limit.as_deref(),
        run.env.backup_max_age.as_deref(),
    ) {
        Ok(retention) => retention,
        Err(e) => {
            report_backup_error(&mut run, &e)?;
            return Ok(1);
        }
    };

    if Path::new(&ai_dir).join("src").is_dir() {
        run.say(&format!(
            "{}: .ai/src/ already exists in {target}\nSkipping init to avoid overwriting your content.\n\nRun {} to synchronize.\n",
            style.yellow("Warning"),
            style.cyan("agentsync sync")
        ))?;
        return Ok(0);
    }

    let mut content_list = normalize_csv(
        options
            .content
            .as_deref()
            .filter(|c| !c.is_empty())
            .unwrap_or(CONTENT_DEFAULT),
    );
    for token in &content_list {
        if !CONTENT_VALID.contains(&token.as_str()) {
            run.tell(&format!(
                "{}: Unknown --content section: {token}\nValid sections: agents rules skills commands subagents\n",
                style.red("Error")
            ))?;
            return Ok(1);
        }
    }

    let tools_from_flag = normalize_csv(options.tools.as_deref().unwrap_or(""));
    let tools_from_detect = if options.no_detect {
        Vec::new()
    } else {
        detect_tools(&target)
    };
    let mut tool_list = merge_lists(&tools_from_flag, &tools_from_detect);
    let mut detect_source = match (tools_from_flag.is_empty(), tools_from_detect.is_empty()) {
        (false, false) => "mixed",
        (false, true) => "flag",
        (true, false) => "detect",
        (true, true) => "none",
    };

    let mut outputs = options.outputs.clone();
    let mut existing_action = options.existing.clone();
    let mut ci = options.ci.clone();

    let interactive = run.env.interactive
        && !options.assume_yes
        && options.tools.is_none()
        && options.content.is_none()
        && !options.no_templates;

    if interactive {
        run.say(&format!(
            "\n{} — {}\n\n",
            style.bold("AgentSync init"),
            style.dim(&target)
        ))?;
        let available = catalog::base_tools();
        if !available.is_empty() {
            let title = if tool_list.is_empty() {
                format!("Tools to enable {}", style.dim("(none auto-detected):"))
            } else {
                format!(
                    "Tools to enable {}",
                    style.dim(&format!("(detected: {}):", tool_list.join(",")))
                )
            };
            match (run.env.multiselect)(&title, &available, &tool_list) {
                Ok(picked) => tool_list = picked,
                Err(Cancelled(_)) => {
                    run.tell(&format!("{}\n", style.yellow("Cancelled.")))?;
                    return Ok(130);
                }
            }
            detect_source = "interactive";
            run.say("\n")?;
        }
        let sections: Vec<String> = CONTENT_VALID.iter().map(|s| s.to_string()).collect();
        match (run.env.multiselect)("Content sections:", &sections, &content_list) {
            Ok(picked) => content_list = picked,
            Err(Cancelled(_)) => {
                run.tell(&format!("{}\n", style.yellow("Cancelled.")))?;
                return Ok(130);
            }
        }
        run.say("\n")?;
        run.say(&format!(
            "{}\n{}\n",
            style.dim("Generated files (CLAUDE.md, .claude/, .cursor/, …) can be committed, so"),
            style.dim("teammates get current rules from git pull and never run agentsync.")
        ))?;
        outputs = if (run.env.confirm)("Commit generated files?", true) {
            "committed".to_string()
        } else {
            "local".to_string()
        };
        run.say("\n")?;
    }

    let project = Project::at(&target)?;
    let existing = if tool_list.is_empty() {
        Vec::new()
    } else {
        existing_dest_files(&project, &target, &tool_list)?
    };

    if interactive && !existing.is_empty() {
        let mut text = format!(
            "{} {}\n",
            style.yellow(&format!(
                "Found {} existing tool config file(s)",
                existing.len()
            )),
            style.dim("— the first sync regenerates these paths:")
        );
        for line in existing.iter().take(10) {
            text.push_str(&format!("   {line}\n"));
        }
        if existing.len() > 10 {
            text.push_str(&format!(
                "   {}\n",
                style.dim(&format!("… and {} more", existing.len() - 10))
            ));
        }
        run.say(&text)?;
        existing_action = if (run.env.confirm)(
            "Copy them into .ai/src/ first, so sync reproduces them?",
            true,
        ) {
            "adopt".to_string()
        } else {
            "replace".to_string()
        };
        run.say("\n")?;
    }

    if interactive
        && ci.is_empty()
        && outputs == "committed"
        && Path::new(&target).join(".github").is_dir()
    {
        if (run.env.confirm)(
            "Add a GitHub Actions gate that runs 'agentsync check'?",
            true,
        ) {
            ci = "github".to_string();
        }
        run.say("\n")?;
    }

    run.say(&plan(
        style,
        &target,
        &tool_list,
        &content_list,
        detect_source,
        options.no_templates,
    ))?;

    if options.dry_run {
        run.say(&format!(
            "{}\n",
            style.dim("Dry run — nothing was written.")
        ))?;
        return Ok(0);
    }

    if interactive {
        if !(run.env.confirm)("Proceed?", true) {
            run.say(&format!("{}\n", style.yellow("Cancelled.")))?;
            return Ok(130);
        }
        run.say("\n")?;
    }

    run.say(&format!(
        "{} in {}\n\n",
        style.bold("Initializing AgentSync"),
        style.cyan(&target)
    ))?;

    let targets = backup_targets(&mut run, &project, &target, &tool_list)?;
    let backup_path = match backup::create(&target, "init", &targets, retention) {
        Ok(path) => path,
        Err(e) => {
            report_backup_error(&mut run, &e)?;
            run.tell(&format!(
                "{}: Could not back up init targets; no project files were changed.\n",
                style.red("Error")
            ))?;
            return Ok(1);
        }
    };
    let shown_backup = backup_path
        .strip_prefix(&format!("{target}/"))
        .unwrap_or(&backup_path)
        .to_string();

    let mut interrupt = Interrupt::arm();
    let scaffold = scaffold(
        &mut run,
        &mut interrupt,
        Scaffold {
            target: &target,
            ai_dir: &ai_dir,
            content: &content_list,
            tools: &tool_list,
            no_templates: options.no_templates,
            outputs: &outputs,
            adopt: existing_action == "adopt",
            existing: &existing,
            ci_github: ci == "github",
            detect_source,
            run_sync: options.run_sync,
            shown_backup: &shown_backup,
        },
    );
    let status = match scaffold {
        Ok(()) => 0,
        Err(failure) => {
            let status = match &failure {
                Failure::Io(e) => {
                    run.tell(&format!("{e}\n"))?;
                    1
                }
                Failure::Signal(sig) => interrupt::status(*sig),
            };
            run.tell(&format!(
                "{}: Init failed; restoring pre-init state...\n",
                style.yellow("Warning")
            ))?;
            match backup::restore(&target, &backup_path) {
                Ok(()) => {
                    if let Err(reason) = witness::seal(&target, &backup_path) {
                        run.tell(&format!(
                            "{}: Could not record the restored state ({reason}); rolling back backup {} cannot detect later changes.\n",
                            style.yellow("Warning"),
                            paths::leaf(&backup_path)
                        ))?;
                    }
                    run.tell(&format!("Restored pre-init state from {shown_backup}\n"))?;
                    prune(&mut run, &target, retention)?;
                }
                Err(e) => {
                    report_backup_error(&mut run, &e)?;
                    run.tell(&format!(
                        "{}: Automatic restore failed. Backup retained at {shown_backup}\n",
                        style.red("Error")
                    ))?;
                }
            }
            if let Failure::Signal(sig) = failure {
                interrupt.resend(sig);
            }
            return Ok(status);
        }
    };
    drop(interrupt);

    prune(&mut run, &target, retention)?;
    if let Err(reason) = witness::seal(&target, &backup_path) {
        run.tell(&format!(
            "{}: Could not record the post-init state ({reason}); rolling back backup {} cannot detect later changes.\n",
            style.yellow("Warning"),
            paths::leaf(&backup_path)
        ))?;
    }

    if options.run_sync && !tool_list.is_empty() {
        run.say(&format!("{}\n\n", style.bold("Running the first sync")))?;
        if (run.env.sync)(&target) != 0 {
            run.tell(&format!(
                "{}: first sync failed — fix the cause and run {}.\n",
                style.yellow("Warning"),
                style.cyan("agentsync sync")
            ))?;
            return Ok(0);
        }
        if outputs == "committed" {
            run.say(&format!(
                "{} {}\n\n",
                style.bold("Commit .ai/ and the generated files"),
                style.dim("— teammates then need only git pull.")
            ))?;
        }
    }
    Ok(status)
}

fn parse_args(args: &[String], run: &mut Run) -> Result<Result<Options, u8>, Error> {
    let style = run.style;
    let mut options = Options {
        target: None,
        tools: None,
        content: None,
        no_detect: false,
        outputs: "committed".to_string(),
        existing: "adopt".to_string(),
        ci: String::new(),
        run_sync: true,
        no_templates: false,
        assume_yes: false,
        dry_run: false,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut valued = |flag: &str, run: &mut Run| -> Result<Result<String, u8>, Error> {
            match rest.next() {
                Some(value) => Ok(Ok(value.clone())),
                None => {
                    run.tell(&format!(
                        "{}: {flag} requires a value\n",
                        style.red("Error")
                    ))?;
                    Ok(Err(1))
                }
            }
        };
        match arg.as_str() {
            "--tools" => match valued("--tools", run)? {
                Ok(value) => options.tools = Some(value),
                Err(status) => return Ok(Err(status)),
            },
            "--content" => match valued("--content", run)? {
                Ok(value) => options.content = Some(value),
                Err(status) => return Ok(Err(status)),
            },
            "--outputs" => match valued("--outputs", run)? {
                Ok(value) => options.outputs = value,
                Err(status) => return Ok(Err(status)),
            },
            "--existing" => match valued("--existing", run)? {
                Ok(value) => options.existing = value,
                Err(status) => return Ok(Err(status)),
            },
            "--ci" => match valued("--ci", run)? {
                Ok(value) => options.ci = value,
                Err(status) => return Ok(Err(status)),
            },
            "--no-detect" => options.no_detect = true,
            "--no-sync" => options.run_sync = false,
            "--no-templates" => options.no_templates = true,
            "--yes" | "-y" => options.assume_yes = true,
            "--dry-run" => options.dry_run = true,
            "--help" | "-h" => {
                run.say(HELP)?;
                return Ok(Err(0));
            }
            flag if flag.starts_with("--tools=") => {
                options.tools = Some(flag["--tools=".len()..].to_string());
            }
            flag if flag.starts_with("--content=") => {
                options.content = Some(flag["--content=".len()..].to_string());
            }
            flag if flag.starts_with("--outputs=") => {
                options.outputs = flag["--outputs=".len()..].to_string();
            }
            flag if flag.starts_with("--existing=") => {
                options.existing = flag["--existing=".len()..].to_string();
            }
            flag if flag.starts_with("--ci=") => {
                options.ci = flag["--ci=".len()..].to_string();
            }
            flag if flag.starts_with('-') => {
                run.tell(&format!(
                    "{}: Unknown flag: {flag}\nRun {} for usage.\n",
                    style.red("Error"),
                    style.cyan("agentsync init --help")
                ))?;
                return Ok(Err(1));
            }
            value => {
                if options.target.is_some() {
                    run.tell(&format!(
                        "{}: Unexpected argument: {value}\n",
                        style.red("Error")
                    ))?;
                    return Ok(Err(1));
                }
                options.target = Some(value.to_string());
            }
        }
    }
    Ok(Ok(options))
}

/// `_init_normalize_csv`: spaces dropped inside each token, empties skipped,
/// first occurrence kept.
fn normalize_csv(csv: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in csv.split(',') {
        let token: String = token.chars().filter(|c| *c != ' ').collect();
        if token.is_empty() || out.contains(&token) {
            continue;
        }
        out.push(token);
    }
    out
}

/// `_init_merge_lists`.
fn merge_lists(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in a.iter().chain(b) {
        if !token.is_empty() && !out.contains(token) {
            out.push(token.clone());
        }
    }
    out
}

/// `_init_detect_enabled_tools`.
fn detect_tools(root: &str) -> Vec<String> {
    DETECTORS
        .iter()
        .filter(|(_, markers)| {
            markers
                .iter()
                .any(|marker| Path::new(root).join(marker).exists())
        })
        .map(|(tool, _)| tool.to_string())
        .collect()
}

/// The destinations a tool's enabled targets name, resolved inside the project.
fn tool_dests(
    project: &Project,
    paths: &Paths,
    slug: &str,
    log: &mut Log,
) -> Result<Vec<String>, Error> {
    let tool = Tool::load(project, slug)?;
    let mut dests = Vec::new();
    for key in TARGET_KEYS {
        if tool.flag(&format!("targets.{key}.enabled")) == Some(false) {
            continue;
        }
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        if let Some(abs) = paths.resolve_dest(&raw, &format!("targets.{key}.dest for {slug}"), log)
        {
            dests.push(abs);
        }
    }
    Ok(dests)
}

/// `_init_existing_dest_files`: repo-relative files under the selected tools'
/// destinations, in byte order, once each.
fn existing_dest_files(
    project: &Project,
    target: &str,
    tools: &[String],
) -> Result<Vec<String>, Error> {
    let paths = Paths::on_disk(target);
    let prefix = format!("{target}/");
    let mut found = BTreeSet::new();
    for slug in tools {
        for abs in tool_dests(project, &paths, slug, &mut Log::default())? {
            let path = Path::new(&abs);
            if path.is_file() {
                found.insert(abs.strip_prefix(&prefix).unwrap_or(&abs).to_string());
            } else if path.is_dir() {
                let mut files = Vec::new();
                files_below(path, &mut files);
                for file in files {
                    let file = file.to_string_lossy().into_owned();
                    found.insert(file.strip_prefix(&prefix).unwrap_or(&file).to_string());
                }
            }
        }
    }
    Ok(found.into_iter().collect())
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

/// `_init_collect_backup_targets`; a destination that cannot be resolved is
/// reported the way `resolve_dest_path` logs it and skipped.
fn backup_targets(
    run: &mut Run,
    project: &Project,
    target: &str,
    tools: &[String],
) -> Result<Vec<String>, Error> {
    let mut targets = vec![
        format!("{target}/.ai/src"),
        format!("{target}/.ai/agent_sync.yaml"),
        format!("{target}/.ai/.template-manifest"),
    ];
    let paths = Paths::on_disk(target);
    let mut log = Log::default();
    for slug in tools {
        targets.extend(tool_dests(project, &paths, slug, &mut log)?);
    }
    for (_, line) in log.lines() {
        run.tell(&format!("{line}\n"))?;
    }
    Ok(targets)
}

/// `_init_print_plan`.
fn plan(
    style: &Style,
    target: &str,
    tools: &[String],
    content: &[String],
    detect_source: &str,
    no_templates: bool,
) -> String {
    let mut text = format!(
        "{}\n  Target:   {}\n",
        style.bold("Plan:"),
        style.cyan(&format!("{target}/.ai/"))
    );
    if content.is_empty() {
        text.push_str(&format!("  Content:  {}\n", style.dim("(none)")));
    } else if no_templates {
        text.push_str(&format!(
            "  Content:  {} {}\n",
            content.join(", "),
            style.dim("(no starter templates)")
        ));
    } else {
        text.push_str(&format!("  Content:  {}\n", content.join(", ")));
    }
    if tools.is_empty() {
        text.push_str(&format!(
            "  Tools:    {}\n",
            style.dim("(none — opt in later via 'agentsync enable')")
        ));
    } else {
        text.push_str(&format!(
            "  Tools:    {} {}\n",
            tools.join(", "),
            style.dim(&format!("({detect_source})"))
        ));
        let mut any_payload = false;
        for resource in ["settings", "hooks"] {
            let names: Vec<String> = tools
                .iter()
                .flat_map(|slug| catalog::base_payloads(resource, slug))
                .filter_map(|file| Some(file.path().file_name()?.to_string_lossy().into_owned()))
                .collect();
            if !names.is_empty() {
                any_payload = true;
                text.push_str(&format!(
                    "  {:<9} {}\n",
                    format!("{resource}:"),
                    names.join(", ")
                ));
            }
        }
        if !any_payload {
            text.push_str(&format!(
                "  {}\n",
                style.dim("No payloads to scaffold — tools will use base templates at sync time.")
            ));
        }
    }
    text.push('\n');
    text
}

struct Scaffold<'a> {
    target: &'a str,
    ai_dir: &'a str,
    content: &'a [String],
    tools: &'a [String],
    no_templates: bool,
    outputs: &'a str,
    adopt: bool,
    existing: &'a [String],
    ci_github: bool,
    detect_source: &'a str,
    run_sync: bool,
    shown_backup: &'a str,
}

enum Failure {
    Io(Error),
    Signal(i32),
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        Failure::Io(e)
    }
}

fn checkpoint(interrupt: &Interrupt) -> Result<(), Failure> {
    match interrupt.received() {
        Some(sig) => Err(Failure::Signal(sig)),
        None => Ok(()),
    }
}

/// The writes between `backup_create` and the summary, each one a point where
/// a failure or a signal restores the snapshot.
fn scaffold(run: &mut Run, interrupt: &mut Interrupt, s: Scaffold) -> Result<(), Failure> {
    let style = run.style;
    let has = |section: &str| s.content.iter().any(|c| c == section);
    let src = format!("{}/src", s.ai_dir);

    create_dir(&src)?;
    if has("rules") {
        create_dir(&format!("{src}/rules"))?;
    }
    if has("skills") {
        create_dir(&format!("{src}/skills"))?;
    }
    if has("commands") {
        create_dir(&format!("{src}/commands"))?;
    }
    if has("subagents") {
        create_dir(&format!("{src}/agents"))?;
    }
    checkpoint(interrupt)?;

    if s.no_templates {
        if has("agents") {
            std::fs::write(format!("{src}/AGENTS.md"), b"")
                .map_err(|e| Error::io(format!("{src}/AGENTS.md"), e))?;
        }
    } else {
        for (rel, bytes) in catalog::template_files() {
            let (dir, _) = rel.rsplit_once('/').unwrap_or(("", rel.as_str()));
            let wanted = match dir {
                "" => has("agents"),
                "rules" => has("rules"),
                "commands" => has("commands"),
                "agents" => has("subagents"),
                _ => has("skills"),
            };
            if wanted {
                write_template(Path::new(&format!("{src}/{rel}")), bytes)?;
            }
        }
    }
    checkpoint(interrupt)?;

    let mut payload_lines = Vec::new();
    for resource in ["settings", "hooks"] {
        for slug in s.tools {
            for file in catalog::base_payloads(resource, slug) {
                let name = file
                    .path()
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let ext = name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or(&name);
                let rel = format!("tools/{slug}/{resource}.{ext}");
                write_template(Path::new(&format!("{src}/{rel}")), file.contents())?;
                payload_lines.push(rel);
            }
        }
    }
    checkpoint(interrupt)?;

    let config_file = format!("{}/agent_sync.yaml", s.ai_dir);
    if !Path::new(&config_file).is_file()
        && !Path::new(&format!("{}/agent_sync.yaml", s.target)).is_file()
    {
        std::fs::write(
            &config_file,
            project_config_text(run.env.version, s.tools, s.outputs),
        )
        .map_err(|e| Error::io(&config_file, e))?;
    }
    checkpoint(interrupt)?;

    let mut manifest = TemplateManifest::load(Path::new(s.target))?;
    let templates = catalog::template_files();
    manifest.heal_from_match(
        templates.iter().map(|(rel, bytes)| (rel.as_str(), *bytes)),
        Path::new(&src),
    );
    manifest.write(Path::new(s.target))?;
    checkpoint(interrupt)?;

    if !s.existing.is_empty() && s.adopt {
        run.say("\n")?;
        adopt_existing(run, s.target, s.existing)?;
    }
    checkpoint(interrupt)?;

    if s.ci_github {
        write_ci_workflow(run, s.target)?;
    }
    checkpoint(interrupt)?;

    run.say(&summary(
        style,
        s.ai_dir,
        s.tools,
        &payload_lines,
        s.detect_source,
        s.no_templates,
        s.outputs,
        s.run_sync,
    ))?;
    run.say(&format!("Backup: {}\n\n", s.shown_backup))?;
    Ok(())
}

fn create_dir(path: &str) -> Result<(), Error> {
    std::fs::create_dir_all(path).map_err(|e| Error::io(path, e))
}

/// `_init_adopt_existing`.
fn adopt_existing(run: &mut Run, target: &str, existing: &[String]) -> Result<(), Error> {
    let style = run.style;
    let project = Project::at(target)?;
    let sources = adopt::discover_sources(&project)?;
    let mut resolver = Resolver::new(&project, sources)?;
    let mut claimed: Vec<String> = Vec::new();
    let mut skips: Vec<String> = Vec::new();
    let mut adopted = 0;
    for file in existing {
        match resolver.resolve(&format!("{target}/{file}"), run.err)? {
            Ok(found) => {
                if claimed.contains(&found.source_rel) {
                    skips.push(format!(
                        "{file} — another file already became {}",
                        found.source_rel
                    ));
                    continue;
                }
                adopt::copy_into_source(&found)?;
                claimed.push(found.source_rel.clone());
                run.say(&format!(
                    "   {} {} → {}\n",
                    style.green("Adopted"),
                    style.cyan(file),
                    style.dim(&found.source_rel)
                ))?;
                adopted += 1;
            }
            Err(reason) => skips.push(format!("{file} — {reason}")),
        }
    }
    for note in &skips {
        run.say(&format!("   {} {note}\n", style.yellow("Kept as-is")))?;
    }
    if !skips.is_empty() {
        run.say(&format!(
            "   {}\n",
            style.dim("Skipped files are regenerated from .ai/src/ — restore them with 'agentsync rollback' if needed.")
        ))?;
    }
    if adopted > 0 || !skips.is_empty() {
        run.say("\n")?;
    }
    Ok(())
}

/// `_init_write_ci_workflow`.
fn write_ci_workflow(run: &mut Run, target: &str) -> Result<(), Error> {
    let style = run.style;
    let dest = format!("{target}/.github/workflows/agentsync-check.yml");
    if Path::new(&dest).is_file() {
        return run.say(&format!(
            "   {} {} {}\n",
            style.yellow("Kept"),
            style.cyan(".github/workflows/agentsync-check.yml"),
            style.dim("(already exists)")
        ));
    }
    create_dir(&format!("{target}/.github/workflows"))?;
    let text = catalog::CI_GITHUB_WORKFLOW
        .replace("__AGENTSYNC_VERSION__", run.env.version)
        .replace(
            "__AGENTSYNC_INSTALL_URL__",
            &format!("https://raw.githubusercontent.com/{REPO}/main/install.sh"),
        );
    staging::write_beside(Path::new(&dest), text.as_bytes())?;
    run.say(&format!(
        "   Created {} — CI gate (agentsync check)\n",
        style.cyan(".github/workflows/agentsync-check.yml")
    ))
}

/// `_init_create_project_config`'s text.
fn project_config_text(version: &str, tools: &[String], outputs: &str) -> String {
    let enabled = if tools.is_empty() {
        "  enabled: []\n".to_string()
    } else {
        let mut text = "  enabled:\n".to_string();
        for tool in tools {
            text.push_str(&format!("    - {tool}\n"));
        }
        text
    };
    format!(
        "# AgentSync — Project Configuration
# All keys are optional — remove any that you leave at the default.

agentsync_version: \"{version}\"
format: {}

# Tools: which ones to sync for this project.
# Each name must match a base tool (see `agentsync list`) or a custom override
# file under .ai/src/tools/<name>.yaml.
tools:
{enabled}
# Source paths (override if you use a custom layout).
source:
  agents: \".ai/src/AGENTS.md\"
  rules: \".ai/src/rules\"
  skills: \".ai/src/skills\"
  commands: \".ai/src/commands\"
  subagents: \".ai/src/agents\"
  tools: \".ai/src/tools\"

# Global defaults applied to all tools.
defaults:
  enabled: false
  cleanup: true

# Post-sync hooks run arbitrary shell — enabling them requires an out-of-repo
# signal (AGENTSYNC_ALLOW_POST_SYNC=true or allow: true in the install-dir
# config.yaml), never this in-repo file. `skip: true` here always disables them.
post_sync:
  skip: false

# Where generated tool files live.
#   committed — outputs and .ai/.sync-manifest are committed; teammates get
#               them from `git pull` and CI runs `agentsync check`.
#   local     — outputs and the manifest are gitignored; every clone runs
#               `agentsync sync` (see `agentsync setup-hooks`).
outputs: {outputs}

# .gitignore management (false leaves the managed block untouched).
gitignore:
  update: true
",
        format_rev::engine()
    )
}

/// `*.md` files directly inside a directory.
fn count_md(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name().to_string_lossy().ends_with(".md")
                        && !e.file_name().to_string_lossy().starts_with('.')
                        && e.path().is_file()
                })
                .count()
        })
        .unwrap_or(0)
}

/// Non-hidden subdirectories of a directory.
fn count_dirs(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.') && e.path().is_dir())
                .count()
        })
        .unwrap_or(0)
}

/// `_init_print_summary`.
#[allow(clippy::too_many_arguments)]
fn summary(
    style: &Style,
    ai_dir: &str,
    tools: &[String],
    payload_lines: &[String],
    detect_source: &str,
    no_templates: bool,
    outputs: &str,
    run_sync: bool,
) -> String {
    let src = Path::new(ai_dir).join("src");
    let mut text = String::from("\n");
    if outputs == "committed" {
        text.push_str(&format!(
            "   Created {}     — project config (outputs: committed — teammates need only git pull)\n",
            style.cyan(".ai/agent_sync.yaml")
        ));
    } else {
        text.push_str(&format!(
            "   Created {}     — project config (outputs: local — every clone runs agentsync sync)\n",
            style.cyan(".ai/agent_sync.yaml")
        ));
    }
    let agents = src.join("AGENTS.md");
    if agents.is_file() {
        let empty = std::fs::metadata(&agents)
            .map(|m| m.len() == 0)
            .unwrap_or(false);
        if no_templates && empty {
            text.push_str(&format!(
                "   Created {}      — {}\n",
                style.cyan(".ai/src/AGENTS.md"),
                style.dim("(empty)")
            ));
        } else {
            text.push_str(&format!(
                "   Created {}      — agent identity\n",
                style.cyan(".ai/src/AGENTS.md")
            ));
        }
    }
    let sections: [(&str, &str, &str, bool); 4] = [
        ("rules", ".ai/src/rules/", "rule(s)", false),
        ("skills", ".ai/src/skills/", "skill(s)", true),
        ("commands", ".ai/src/commands/", "command(s)", false),
        ("agents", ".ai/src/agents/", "subagent(s)", false),
    ];
    for (dir, shown, noun, dirs) in sections {
        let path = src.join(dir);
        if !path.is_dir() {
            continue;
        }
        let count = if dirs {
            count_dirs(&path)
        } else {
            count_md(&path)
        };
        let padded = pad_created(style, shown);
        if count > 0 {
            text.push_str(&format!("{padded}— {count} {noun}\n"));
        } else if no_templates {
            text.push_str(&format!("{padded}— {}\n", style.dim("(empty)")));
        }
    }
    for line in payload_lines {
        text.push_str(&format!(
            "   Created {}\n",
            style.cyan(&format!(".ai/src/{line}"))
        ));
    }
    text.push('\n');
    if tools.is_empty() {
        text.push_str(&format!(
            "   {}\n",
            style.dim("No tools enabled. Run 'agentsync enable <slug>' to opt in.")
        ));
    } else {
        let joined = tools.join(", ");
        let count = tools.len();
        let line = match detect_source {
            "detect" => format!(
                "   {} {joined}\n",
                style.green(&format!("Auto-detected {count} tool(s):"))
            ),
            "flag" => format!(
                "   {} {joined} {}\n",
                style.green(&format!("Enabled {count} tool(s):")),
                style.dim("(from --tools)")
            ),
            "mixed" => format!(
                "   {} {joined} {}\n",
                style.green(&format!("Enabled {count} tool(s):")),
                style.dim("(auto-detect + --tools)")
            ),
            "interactive" => format!(
                "   {} {joined} {}\n",
                style.green(&format!("Enabled {count} tool(s):")),
                style.dim("(selected)")
            ),
            _ => format!(
                "   {} {joined}\n",
                style.green(&format!("Enabled {count} tool(s):"))
            ),
        };
        text.push_str(&line);
    }
    text.push_str(&format!("\n{}\n\nNext steps:\n", style.green("Done!")));
    let mut step = 1;
    if agents.is_file() {
        text.push_str(&format!(
            "  {step}. Edit {} — customize your agent's identity\n",
            style.cyan(".ai/src/AGENTS.md")
        ));
        step += 1;
    }
    text.push_str(&format!(
        "  {step}. Run {}    — print an AI prompt to tailor .ai/src/ to your codebase\n",
        style.cyan("agentsync generate")
    ));
    step += 1;
    text.push_str(&format!(
        "  {step}. Run {}        — browse all available tools\n",
        style.cyan("agentsync list")
    ));
    step += 1;
    if tools.is_empty() {
        text.push_str(&format!(
            "  {step}. Run {} — opt in to tools you use\n",
            style.cyan("agentsync enable <slug>")
        ));
    } else {
        text.push_str(&format!(
            "  {step}. Run {} — add more tools\n",
            style.cyan("agentsync enable <slug>")
        ));
    }
    step += 1;
    if run_sync && !tools.is_empty() {
        text.push_str(&format!(
            "  {step}. Re-run {}     — after every change to .ai/src/\n",
            style.cyan("agentsync sync")
        ));
    } else {
        text.push_str(&format!(
            "  {step}. Run {}        — distribute to enabled tools\n",
            style.cyan("agentsync sync")
        ));
    }
    text.push_str(&format!(
        "\nCustomize:\n  {} {}            — configure shared MCP servers\n  {} {} — override settings/hooks per tool\n\n",
        style.dim("•"),
        style.cyan("agentsync add mcp <server>"),
        style.dim("•"),
        style.cyan("agentsync customize <tool> <resource>")
    ));
    text
}

/// `   Created $(_cyan "<shown>")` padded as Bash's literal spacing pads each
/// section line: the styled name plus the spaces that bring the plain text to
/// the column the `—` starts in.
fn pad_created(style: &Style, shown: &str) -> String {
    let spaces = match shown {
        ".ai/src/rules/" => 10,
        ".ai/src/skills/" => 9,
        ".ai/src/commands/" => 7,
        _ => 9,
    };
    format!("   Created {}{}", style.cyan(shown), " ".repeat(spaces))
}

fn prune(run: &mut Run, target: &str, retention: backup::Retention) -> Result<(), Error> {
    let (limit, max_age) = (run.env.backup_limit.clone(), run.env.backup_max_age.clone());
    if let Err(e) = backup::prune(target, limit.as_deref(), max_age.as_deref(), retention) {
        report_backup_error(run, &e)?;
        let style = run.style;
        run.tell(&format!(
            "{}: Could not prune old AgentSync backups.\n",
            style.yellow("Warning")
        ))?;
    }
    Ok(())
}

/// `_backup_error`: `Error: <message>` on stderr, plain.
fn report_backup_error(run: &mut Run, error: &Error) -> Result<(), Error> {
    match error {
        Error::Backup(message) => run.tell(&format!("Error: {message}\n")),
        other => run.tell(&format!("{other}\n")),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::template_manifest::REL;

    fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for (rel, text) in files {
            let path = Path::new(&root).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        (dir, root)
    }

    struct Outcome {
        status: u8,
        out: String,
        err: String,
        synced: Vec<String>,
        asked: Vec<String>,
        picked: Vec<String>,
    }

    struct Script {
        interactive: bool,
        confirms: Vec<bool>,
        picks: Vec<Result<Vec<String>, Cancelled>>,
        sync_status: u8,
    }

    fn quiet() -> Script {
        Script {
            interactive: false,
            confirms: Vec::new(),
            picks: Vec::new(),
            sync_status: 0,
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn call(cwd: &str, args: &[&str], script: Script) -> Outcome {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut synced = Vec::new();
        let mut asked = Vec::new();
        let mut picked = Vec::new();
        let mut confirms: VecDeque<bool> = script.confirms.into_iter().collect();
        let mut picks: VecDeque<Result<Vec<String>, Cancelled>> =
            script.picks.into_iter().collect();
        let status_code = script.sync_status;
        let mut confirm = |question: &str, _default: bool| {
            asked.push(question.to_string());
            confirms.pop_front().unwrap_or(true)
        };
        let mut multiselect = |title: &str, _options: &[String], preselected: &[String]| {
            picked.push(title.to_string());
            picks
                .pop_front()
                .unwrap_or_else(|| Ok(preselected.to_vec()))
        };
        let mut sync = |root: &str| {
            synced.push(root.to_string());
            status_code
        };
        let mut env = Env {
            version: "9.9.9",
            cwd: cwd.to_string(),
            config_path: None,
            backup_limit: None,
            backup_max_age: None,
            interactive: script.interactive,
            confirm: &mut confirm,
            multiselect: &mut multiselect,
            sync: &mut sync,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = init(&args, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
        Outcome {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
            synced,
            asked,
            picked,
        }
    }

    fn tree(root: &str) -> Vec<String> {
        let mut files = Vec::new();
        files_below(Path::new(root), &mut files);
        let mut rels: Vec<String> = files
            .iter()
            .map(|f| f.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .filter(|rel| !rel.starts_with(".ai/backups/"))
            .collect();
        rels.sort();
        rels
    }

    fn backups(root: &str) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(Path::new(root).join(".ai/backups"))
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    }

    const PLAN_NONE: &str = "Plan:\n  Target:   {root}/.ai/\n  Content:  agents, rules, skills, commands, subagents\n  Tools:    (none — opt in later via 'agentsync enable')\n\n";
    const SUMMARY_FULL: &str = "\n   Created .ai/agent_sync.yaml     — project config (outputs: committed — teammates need only git pull)\n   Created .ai/src/AGENTS.md      — agent identity\n   Created .ai/src/rules/          — 3 rule(s)\n   Created .ai/src/skills/         — 7 skill(s)\n   Created .ai/src/commands/       — 2 command(s)\n   Created .ai/src/agents/         — 1 subagent(s)\n";
    const NEXT_NO_TOOLS: &str = "\n   No tools enabled. Run 'agentsync enable <slug>' to opt in.\n\nDone!\n\nNext steps:\n  1. Edit .ai/src/AGENTS.md — customize your agent's identity\n  2. Run agentsync generate    — print an AI prompt to tailor .ai/src/ to your codebase\n  3. Run agentsync list        — browse all available tools\n  4. Run agentsync enable <slug> — opt in to tools you use\n  5. Run agentsync sync        — distribute to enabled tools\n\nCustomize:\n  • agentsync add mcp <server>            — configure shared MCP servers\n  • agentsync customize <tool> <resource> — override settings/hooks per tool\n\n";

    fn backup_line(root: &str) -> String {
        let snapshot = backups(root).pop().expect("one snapshot");
        format!(
            "Backup: .ai/backups/{}\n\n",
            snapshot.file_name().unwrap().to_string_lossy()
        )
    }

    #[test]
    fn arguments_and_validation_are_refused_like_bash() {
        let (_dir, root) = project(&[]);
        let help = call(&root, &["--help"], quiet());
        assert_eq!(help.status, 0);
        assert!(
            help.out.starts_with(
                "Usage: agentsync init [<dir>] [OPTIONS]\n\nScaffold .ai/ in a project."
            )
        );
        assert!(
            help.out.ends_with(
                "  agentsync init --dry-run                 # preview without writing\n"
            )
        );
        assert_eq!(help.out.lines().count(), 45);
        let cases: [(&[&str], u8, &str); 9] = [
            (
                &["--bogus"],
                1,
                "Error: Unknown flag: --bogus\nRun agentsync init --help for usage.\n",
            ),
            (&["a", "b"], 1, "Error: Unexpected argument: b\n"),
            (&["--tools"], 1, "Error: --tools requires a value\n"),
            (
                &["--outputs", "bogus"],
                1,
                "Error: --outputs must be 'committed' or 'local' (got 'bogus')\n",
            ),
            (
                &["--existing", "bogus"],
                1,
                "Error: --existing must be 'adopt' or 'replace' (got 'bogus')\n",
            ),
            (
                &["--ci", "gitlab"],
                1,
                "Error: --ci only supports 'github' (got 'gitlab')\n",
            ),
            (
                &["missing-dir"],
                1,
                "Error: Directory not found: missing-dir\n",
            ),
            (
                &["--content", "bogus"],
                1,
                "Error: Unknown --content section: bogus\nValid sections: agents rules skills commands subagents\n",
            ),
            (
                &["--tools", "claude", "--content", "agents,bogus"],
                1,
                "Error: Unknown --content section: bogus\nValid sections: agents rules skills commands subagents\n",
            ),
        ];
        for (args, status, err) in cases {
            let run = call(&root, args, quiet());
            assert_eq!(
                (run.status, run.out.as_str(), run.err.as_str()),
                (status, "", err),
                "{args:?}"
            );
        }
        assert!(!Path::new(&root).join(".ai").exists());

        std::fs::create_dir_all(Path::new(&root).join(".ai")).unwrap();
        let inside = call(&format!("{root}/.ai"), &[], quiet());
        assert_eq!(
            (inside.status, inside.err),
            (
                2,
                format!(
                    "Error: Cannot init inside the .ai/ directory: {root}/.ai\nRun agentsync init from the project root (the parent of .ai/):\n  cd \"{root}\" && agentsync init\n"
                )
            )
        );
    }

    #[test]
    fn a_plain_init_scaffolds_backs_up_and_skips_a_second_run_like_bash() {
        let (_dir, root) = project(&[]);
        let run = call(&root, &["--no-detect"], quiet());
        assert_eq!(run.status, 0);
        assert_eq!(run.err, "");
        assert_eq!(
            run.out,
            format!(
                "{}Initializing AgentSync in {root}\n\n{SUMMARY_FULL}{NEXT_NO_TOOLS}{}",
                PLAN_NONE.replace("{root}", &root),
                backup_line(&root)
            )
        );
        assert!(run.synced.is_empty());
        let files = tree(&root);
        assert_eq!(files.len(), 21);
        assert!(files.contains(&".ai/.template-manifest".to_string()));
        assert!(files.contains(&".ai/src/skills/humanizer/scripts/strip-ai-chars.sh".to_string()));
        assert!(!Path::new(&root).join(".ai/src/tools").exists());
        let config = std::fs::read_to_string(Path::new(&root).join(".ai/agent_sync.yaml")).unwrap();
        assert!(config.starts_with("# AgentSync — Project Configuration\n# All keys are optional — remove any that you leave at the default.\n\nagentsync_version: \"9.9.9\"\nformat: 2\n\n# Tools:"));
        assert!(config.contains("\ntools:\n  enabled: []\n\n# Source paths"));
        assert!(config.ends_with("outputs: committed\n\n# .gitignore management (false leaves the managed block untouched).\ngitignore:\n  update: true\n"));
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(REL))
                .unwrap()
                .lines()
                .count(),
            19
        );
        let snapshot = backups(&root).pop().unwrap();
        assert_eq!(
            std::fs::read_to_string(snapshot.join("targets.tsv")).unwrap(),
            "missing\t.ai/src\nmissing\t.ai/agent_sync.yaml\nmissing\t.ai/.template-manifest\n"
        );
        assert!(snapshot.join("after.tsv").is_file());
        assert!(
            std::fs::read_to_string(snapshot.join("metadata"))
                .unwrap()
                .contains("operation=init\n")
        );

        let again = call(&root, &["--tools", "claude", "--no-sync"], quiet());
        assert_eq!(
            (again.status, again.out),
            (
                0,
                format!(
                    "Warning: .ai/src/ already exists in {root}\nSkipping init to avoid overwriting your content.\n\nRun agentsync sync to synchronize.\n"
                )
            )
        );
    }

    #[test]
    fn markers_flags_and_content_shape_the_plan_like_bash() {
        let (_dir, root) = project(&[
            (".claude/x", ""),
            (".cursor/x", ""),
            (".github/instructions/x", ""),
            (".gemini/x", ""),
            (".codex/x", ""),
            (".kimi-code/x", ""),
            (".opencode/x", ""),
            (".windsurf/x", ""),
            (".junie/x", ""),
            (".clinerules", ""),
            (".amazonq/x", ""),
            (".zed/x", ""),
            (".agents/rules/x", ""),
        ]);
        let all = call(&root, &["--dry-run"], quiet());
        assert_eq!(
            all.out,
            format!(
                "Plan:\n  Target:   {root}/.ai/\n  Content:  agents, rules, skills, commands, subagents\n  Tools:    claude, cursor, copilot, gemini, codex, kimi, opencode, windsurf, junie, cline, amazonq, zed, antigravity (detect)\n  settings: claude.json, gemini.json, codex.toml, opencode.json, zed.json\n  hooks:    cursor.json, copilot.json, codex.json, opencode.ts, windsurf.json\n\nDry run — nothing was written.\n"
            )
        );
        assert!(!Path::new(&root).join(".ai").exists());

        let (_dir, root) = project(&[
            ("AGENTS.md", "# generic\n"),
            ("GEMINI.md", ""),
            (".rules", ""),
            ("opencode.json", ""),
        ]);
        let files = call(&root, &["--dry-run"], quiet());
        assert!(files.out.contains("  Tools:    gemini, opencode, zed (detect)\n  settings: gemini.json, opencode.json, zed.json\n  hooks:    opencode.ts\n"));

        let (_dir, root) = project(&[(".cursor/x", "")]);
        let mixed = call(
            &root,
            &[
                "--dry-run",
                "--tools",
                "claude, cursor,claude",
                "--content",
                " rules , skills ",
            ],
            quiet(),
        );
        assert!(mixed.out.contains("  Content:  rules, skills\n  Tools:    claude, cursor (mixed)\n  settings: claude.json\n  hooks:    cursor.json\n"));
        let no_templates = call(
            &root,
            &[
                "--dry-run",
                "--no-templates",
                "--no-detect",
                "--content",
                "agents,rules",
            ],
            quiet(),
        );
        assert_eq!(
            no_templates.out,
            format!(
                "Plan:\n  Target:   {root}/.ai/\n  Content:  agents, rules (no starter templates)\n  Tools:    (none — opt in later via 'agentsync enable')\n\nDry run — nothing was written.\n"
            )
        );
        let empty = call(
            &root,
            &["--dry-run", "--no-detect", "--content", ","],
            quiet(),
        );
        assert!(empty.out.contains("  Content:  (none)\n"));
        let kimi = call(
            &root,
            &["--dry-run", "--no-detect", "--tools", "kimi"],
            quiet(),
        );
        assert!(kimi.out.contains("  Tools:    kimi (flag)\n  No payloads to scaffold — tools will use base templates at sync time.\n"));
    }

    #[test]
    fn payloads_config_and_summary_follow_the_selected_tools() {
        let (_dir, root) = project(&[]);
        let run = call(
            &root,
            &[
                "--tools",
                "claude,cursor",
                "--content",
                "agents,rules",
                "--no-sync",
            ],
            quiet(),
        );
        assert_eq!(
            run.out,
            format!(
                "Plan:\n  Target:   {root}/.ai/\n  Content:  agents, rules\n  Tools:    claude, cursor (flag)\n  settings: claude.json\n  hooks:    cursor.json\n\nInitializing AgentSync in {root}\n\n\n   Created .ai/agent_sync.yaml     — project config (outputs: committed — teammates need only git pull)\n   Created .ai/src/AGENTS.md      — agent identity\n   Created .ai/src/rules/          — 3 rule(s)\n   Created .ai/src/tools/claude/settings.json\n   Created .ai/src/tools/cursor/hooks.json\n\n   Enabled 2 tool(s): claude, cursor (from --tools)\n\nDone!\n\nNext steps:\n  1. Edit .ai/src/AGENTS.md — customize your agent's identity\n  2. Run agentsync generate    — print an AI prompt to tailor .ai/src/ to your codebase\n  3. Run agentsync list        — browse all available tools\n  4. Run agentsync enable <slug> — add more tools\n  5. Run agentsync sync        — distribute to enabled tools\n\nCustomize:\n  • agentsync add mcp <server>            — configure shared MCP servers\n  • agentsync customize <tool> <resource> — override settings/hooks per tool\n\n{}",
                backup_line(&root)
            )
        );
        assert_eq!(
            tree(&root),
            [
                ".ai/.template-manifest",
                ".ai/agent_sync.yaml",
                ".ai/src/AGENTS.md",
                ".ai/src/rules/comments.md",
                ".ai/src/rules/core.md",
                ".ai/src/rules/git.md",
                ".ai/src/tools/claude/settings.json",
                ".ai/src/tools/cursor/hooks.json"
            ]
        );
        let config = std::fs::read_to_string(Path::new(&root).join(".ai/agent_sync.yaml")).unwrap();
        assert!(config.contains("\ntools:\n  enabled:\n    - claude\n    - cursor\n\n"));
        let snapshot = backups(&root).pop().unwrap();
        let targets = std::fs::read_to_string(snapshot.join("targets.tsv")).unwrap();
        assert!(targets.starts_with("missing\t.ai/src\nmissing\t.ai/agent_sync.yaml\nmissing\t.ai/.template-manifest\nmissing\tCLAUDE.md\n"));
        assert!(targets.contains("missing\t.cursor/hooks.json\n"));

        let (_dir, root) = project(&[]);
        let empty = call(
            &root,
            &[
                "--no-templates",
                "--no-detect",
                "--content",
                "rules,agents",
                "--outputs=local",
                "--no-sync",
            ],
            quiet(),
        );
        assert!(empty.out.contains("\n   Created .ai/agent_sync.yaml     — project config (outputs: local — every clone runs agentsync sync)\n   Created .ai/src/AGENTS.md      — (empty)\n   Created .ai/src/rules/          — (empty)\n\n   No tools enabled."));
        assert_eq!(
            std::fs::metadata(Path::new(&root).join(".ai/src/AGENTS.md"))
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            std::fs::read_dir(Path::new(&root).join(".ai/src/rules"))
                .unwrap()
                .count(),
            0
        );
        assert!(!Path::new(&root).join(REL).exists());
        let (_dir, root) = project(&[]);
        let rules_only = call(
            &root,
            &[
                "--no-templates",
                "--no-detect",
                "--content",
                "rules",
                "--no-sync",
            ],
            quiet(),
        );
        assert!(
            rules_only
                .out
                .contains("  Content:  rules (no starter templates)\n")
        );
        assert!(rules_only.out.contains("\n   Created .ai/agent_sync.yaml     — project config (outputs: committed — teammates need only git pull)\n   Created .ai/src/rules/          — (empty)\n\n   No tools enabled."));
        assert!(
            rules_only
                .out
                .contains("Next steps:\n  1. Run agentsync generate")
        );
        assert!(!Path::new(&root).join(".ai/src/AGENTS.md").exists());
    }

    #[test]
    fn existing_outputs_are_adopted_first_wins_or_replaced_like_bash() {
        let (_dir, root) = project(&[
            ("CLAUDE.md", "# Hand-written team rules\n"),
            (".claude/rules/legacy.md", "# Legacy rule\n"),
            (".claude/settings.json", "{\"settings\": true}\n"),
        ]);
        let run = call(&root, &["--tools", "claude", "--yes", "--no-sync"], quiet());
        assert_eq!(run.status, 0);
        assert!(run.out.contains(&format!(
            "Initializing AgentSync in {root}\n\n\n   Adopted .claude/rules/legacy.md → .ai/src/rules/legacy.md\n   Adopted .claude/settings.json → .ai/src/tools/claude/settings.json\n   Adopted CLAUDE.md → .ai/src/AGENTS.md\n\n\n   Created .ai/agent_sync.yaml"
        )));
        assert!(
            run.out
                .contains("   Created .ai/src/rules/          — 4 rule(s)\n")
        );
        assert!(
            run.out
                .contains("   Enabled 1 tool(s): claude (auto-detect + --tools)\n")
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap(),
            "# Hand-written team rules\n"
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/tools/claude/settings.json"))
                .unwrap(),
            "{\"settings\": true}\n"
        );
        let snapshot = backups(&root).pop().unwrap();
        assert!(snapshot.join("files/CLAUDE.md").is_file());
        assert!(snapshot.join("files/.claude/rules/legacy.md").is_file());

        let (_dir, root) = project(&[
            ("CLAUDE.md", "# From CLAUDE\n"),
            ("AGENTS.md", "# From AGENTS\n"),
        ]);
        let two = call(
            &root,
            &["--tools", "claude,codex", "--yes", "--no-sync"],
            quiet(),
        );
        assert!(two.out.contains("\n   Adopted AGENTS.md → .ai/src/AGENTS.md\n   Kept as-is CLAUDE.md — another file already became .ai/src/AGENTS.md\n   Skipped files are regenerated from .ai/src/ — restore them with 'agentsync rollback' if needed.\n\n"));
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap(),
            "# From AGENTS\n"
        );

        let (_dir, root) = project(&[
            (".cursor/rules/core.mdc", "body\n"),
            (".cursor/mcp.json", "{}\n"),
        ]);
        let refused = call(&root, &["--tools", "cursor", "--yes", "--no-sync"], quiet());
        assert!(refused.out.contains("\n   Adopted .cursor/mcp.json → .ai/src/tools/cursor/mcp.json\n   Kept as-is .cursor/rules/core.mdc — cursor injects a frontmatter header on sync. Adopting would propagate it to other tools' rule files. Edit .ai/src/rules/ instead.\n   Skipped files"));

        let (_dir, root) = project(&[("CLAUDE.md", "# Hand-written team rules\n")]);
        let replaced = call(
            &root,
            &[
                "--tools",
                "claude",
                "--yes",
                "--existing",
                "replace",
                "--no-sync",
            ],
            quiet(),
        );
        assert!(!replaced.out.contains("Adopted"));
        assert!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md"))
                .unwrap()
                .starts_with("# ")
        );
        assert_ne!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap(),
            "# Hand-written team rules\n"
        );
    }

    #[test]
    fn the_ci_gate_is_written_once_with_the_pinned_version() {
        let (_dir, root) = project(&[]);
        let run = call(
            &root,
            &["--tools", "claude", "--yes", "--ci", "github", "--no-sync"],
            quiet(),
        );
        assert!(run.out.contains(&format!("Initializing AgentSync in {root}\n\n   Created .github/workflows/agentsync-check.yml — CI gate (agentsync check)\n\n   Created .ai/agent_sync.yaml")));
        let workflow = Path::new(&root).join(".github/workflows/agentsync-check.yml");
        let text = std::fs::read_to_string(&workflow).unwrap();
        assert!(text.contains("AGENTSYNC_VERSION=9.9.9 bash"));
        assert!(
            text.contains(
                "https://raw.githubusercontent.com/yelmuratoff/agent_sync/main/install.sh"
            )
        );
        assert!(!text.contains("__AGENTSYNC_"));
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&workflow).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let (_dir, root) = project(&[(".github/workflows/agentsync-check.yml", "name: mine\n")]);
        let kept = call(
            &root,
            &["--tools", "claude", "--yes", "--ci", "github", "--no-sync"],
            quiet(),
        );
        assert!(
            kept.out
                .contains("\n   Kept .github/workflows/agentsync-check.yml (already exists)\n\n")
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".github/workflows/agentsync-check.yml"))
                .unwrap(),
            "name: mine\n"
        );
        let none = call(
            &root,
            &["--dry-run", "--tools", "claude", "--ci", "github"],
            quiet(),
        );
        assert!(!none.out.contains("workflows"));
    }

    #[test]
    fn the_first_sync_runs_for_enabled_tools_and_reports_committed_mode() {
        let (_dir, root) = project(&[]);
        let run = call(&root, &["--tools", "claude"], quiet());
        assert_eq!(run.synced, std::slice::from_ref(&root));
        assert!(
            run.out
                .contains("  5. Re-run agentsync sync     — after every change to .ai/src/\n")
        );
        assert!(run.out.ends_with(&format!("{}Running the first sync\n\nCommit .ai/ and the generated files — teammates then need only git pull.\n\n", backup_line(&root))));

        let (_dir, root) = project(&[]);
        let local = call(&root, &["--tools", "claude", "--outputs", "local"], quiet());
        assert!(local.out.ends_with("Running the first sync\n\n"));

        let (_dir, root) = project(&[]);
        let failed = call(
            &root,
            &["--tools", "claude"],
            Script {
                sync_status: 1,
                ..quiet()
            },
        );
        assert_eq!(
            (failed.status, failed.err.as_str()),
            (
                0,
                "Warning: first sync failed — fix the cause and run agentsync sync.\n"
            )
        );
        assert!(failed.out.ends_with("Running the first sync\n\n"));

        let (_dir, root) = project(&[]);
        let none = call(&root, &["--no-detect"], quiet());
        assert!(none.synced.is_empty());
        let (_dir, root) = project(&[]);
        let skipped = call(&root, &["--tools", "claude", "--no-sync"], quiet());
        assert!(skipped.synced.is_empty());
        assert!(
            skipped
                .out
                .contains("  5. Run agentsync sync        — distribute to enabled tools\n")
        );
    }

    #[test]
    fn a_scaffold_failure_restores_the_snapshot_like_bash() {
        let (_dir, root) = project(&[(".ai/agent_sync.yaml/sentinel", "keep\n")]);
        let run = call(&root, &["--no-detect"], quiet());
        assert_eq!(run.status, 1);
        assert_eq!(
            run.out,
            format!(
                "{}Initializing AgentSync in {root}\n\n",
                PLAN_NONE.replace("{root}", &root)
            )
        );
        let snapshot = backups(&root).pop().unwrap();
        assert_eq!(
            run.err,
            format!(
                "{root}/.ai/agent_sync.yaml: Is a directory (os error 21)\nWarning: Init failed; restoring pre-init state...\nRestored pre-init state from .ai/backups/{}\n",
                snapshot.file_name().unwrap().to_string_lossy()
            )
        );
        assert_eq!(tree(&root), [".ai/agent_sync.yaml/sentinel"]);
        assert!(snapshot.join("after.tsv").is_file());
    }

    #[test]
    fn preexisting_configs_are_kept_and_validated_like_bash() {
        let (_dir, root) =
            project(&[(".ai/agent_sync.yaml", "tools:\n  enabled:\n    - claude\n")]);
        let run = call(&root, &["--no-detect", "--no-sync"], quiet());
        assert_eq!(run.status, 0);
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/agent_sync.yaml")).unwrap(),
            "tools:\n  enabled:\n    - claude\n"
        );

        let (_dir, root) = project(&[("agent_sync.yaml", "tools:\n  enabled: []\n")]);
        let root_config = call(&root, &["--no-detect", "--no-sync"], quiet());
        assert_eq!(root_config.status, 0);
        assert!(!Path::new(&root).join(".ai/agent_sync.yaml").exists());

        let (_dir, root) = project(&[(".ai/agent_sync.yaml", "backup:\n  retention: typo\n")]);
        let typo = call(&root, &["--no-detect"], quiet());
        assert_eq!(
            (typo.status, typo.err),
            (
                1,
                format!(
                    "Error: Invalid backup.retention 'typo' in {root}/.ai/agent_sync.yaml; expected bounded or preserve\n"
                )
            )
        );
        assert!(!Path::new(&root).join(".ai/src").exists());
    }

    #[test]
    fn the_wizard_picks_tools_content_outputs_existing_and_ci_like_bash() {
        let (_dir, root) = project(&[
            (".github/x", ""),
            ("CLAUDE.md", "# Hand-written\n"),
            (".claude/rules/legacy.md", "# Legacy\n"),
        ]);
        let run = call(
            &root,
            &["--no-sync"],
            Script {
                interactive: true,
                confirms: vec![false, true, true, true],
                picks: vec![
                    Ok(strings(&["claude", "cursor"])),
                    Ok(strings(&["agents", "rules"])),
                ],
                sync_status: 0,
            },
        );
        assert_eq!(run.status, 0);
        assert_eq!(
            run.picked,
            ["Tools to enable (detected: claude):", "Content sections:"]
        );
        assert_eq!(
            run.asked,
            [
                "Commit generated files?",
                "Copy them into .ai/src/ first, so sync reproduces them?",
                "Proceed?"
            ]
        );
        assert!(run.out.starts_with(&format!(
            "\nAgentSync init — {root}\n\n\n\nGenerated files (CLAUDE.md, .claude/, .cursor/, …) can be committed, so\nteammates get current rules from git pull and never run agentsync.\n\nFound 2 existing tool config file(s) — the first sync regenerates these paths:\n   .claude/rules/legacy.md\n   CLAUDE.md\n\nPlan:\n  Target:   {root}/.ai/\n  Content:  agents, rules\n  Tools:    claude, cursor (interactive)\n  settings: claude.json\n  hooks:    cursor.json\n\n\nInitializing AgentSync in {root}\n\n\n   Adopted .claude/rules/legacy.md → .ai/src/rules/legacy.md\n   Adopted CLAUDE.md → .ai/src/AGENTS.md\n\n"
        )));
        assert!(
            run.out
                .contains("(outputs: local — every clone runs agentsync sync)")
        );
        assert!(
            run.out
                .contains("   Enabled 2 tool(s): claude, cursor (selected)\n")
        );
        assert!(!Path::new(&root).join(".github/workflows").exists());

        let (_dir, root) = project(&[(".github/x", "")]);
        let ci = call(
            &root,
            &["--no-sync"],
            Script {
                interactive: true,
                confirms: vec![true, true, true],
                picks: vec![Ok(strings(&["claude"])), Ok(strings(&["rules"]))],
                sync_status: 0,
            },
        );
        assert_eq!(
            ci.asked,
            [
                "Commit generated files?",
                "Add a GitHub Actions gate that runs 'agentsync check'?",
                "Proceed?"
            ]
        );
        assert!(ci.out.contains("\n\n\nPlan:\n"));
        assert!(
            Path::new(&root)
                .join(".github/workflows/agentsync-check.yml")
                .is_file()
        );

        let (_dir, root) = project(&[]);
        let declined = call(
            &root,
            &[],
            Script {
                interactive: true,
                confirms: vec![true, false],
                picks: Vec::new(),
                sync_status: 0,
            },
        );
        assert_eq!(declined.status, 130);
        assert!(declined.out.ends_with(&format!(
            "{}Cancelled.\n",
            PLAN_NONE.replace("{root}", &root)
        )));
        assert_eq!(
            declined.picked,
            ["Tools to enable (none auto-detected):", "Content sections:"]
        );
        assert!(!Path::new(&root).join(".ai").exists());

        let cancelled = call(
            &root,
            &[],
            Script {
                interactive: true,
                confirms: Vec::new(),
                picks: vec![Err(Cancelled(Vec::new()))],
                sync_status: 0,
            },
        );
        assert_eq!(
            (cancelled.status, cancelled.err.as_str()),
            (130, "Cancelled.\n")
        );
        assert_eq!(cancelled.out, format!("\nAgentSync init — {root}\n\n"));

        let dry = call(
            &root,
            &["--dry-run"],
            Script {
                interactive: true,
                confirms: vec![true],
                picks: vec![Ok(Vec::new()), Ok(strings(&["rules"]))],
                sync_status: 0,
            },
        );
        assert_eq!(dry.status, 0);
        assert!(dry.out.ends_with("  Content:  rules\n  Tools:    (none — opt in later via 'agentsync enable')\n\nDry run — nothing was written.\n"));
        assert_eq!(dry.asked, ["Commit generated files?"]);

        let flagged = call(
            &root,
            &["--yes"],
            Script {
                interactive: true,
                confirms: Vec::new(),
                picks: Vec::new(),
                sync_status: 0,
            },
        );
        assert!(flagged.picked.is_empty());
        assert!(flagged.asked.is_empty());
        assert_eq!(flagged.status, 0);
    }
}
