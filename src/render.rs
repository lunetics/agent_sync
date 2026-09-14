//! The run of `lib/sync.sh`: config and sources, the `shared:` and engine skill
//! overlays, every enabled tool and selected profile rendered into the
//! workspace, disabled tools cleaned. `check` renders forced and in memory;
//! `cli::sync` runs the stages on disk with its transaction between them.

use std::collections::BTreeSet;

use crate::overlay::{self, Sources};
use crate::rules::{self, Conversion, RuleOptions};
use crate::session::Session;
use crate::tool::Tool;
use crate::{
    Error, catalog, engine_version, file_ops, opencode_json, paths, payload, profiles,
    project_config, version, yaml_subset,
};

pub const TARGET_KEYS: [&str; 9] = [
    "agents",
    "rules",
    "skills",
    "commands",
    "subagents",
    "settings",
    "mcp",
    "hooks",
    "guard",
];

/// A render stopped the way `sync.sh` exits: the status after its log lines.
#[derive(Debug, PartialEq, Eq)]
pub struct Stop(pub u8);

pub type Step = Result<(), Stop>;

/// Environment `sync.sh` reads. `check` sets `skip_post_sync`, as
/// `lib/check.sh` exported `AGENTSYNC_SKIP_POST_SYNC=true`.
#[derive(Default)]
pub struct Env {
    pub config_path: Option<String>,
    pub skip_post_sync: Option<String>,
    pub allow_post_sync: Option<String>,
}

/// `--only`, `--skip`, and `--profile`.
#[derive(Default)]
pub struct Selection {
    pub only: String,
    pub skip: String,
    pub profile: Option<String>,
}

impl Selection {
    /// `should_sync_tool`.
    pub fn includes(&self, slug: &str) -> bool {
        let listed = |csv: &str| format!(",{csv},").contains(&format!(",{slug},"));
        (self.only.is_empty() || listed(&self.only))
            && (self.skip.is_empty() || !listed(&self.skip))
    }
}

/// The globals `sync.sh` fills as it goes.
pub struct Run {
    pub config: Option<String>,
    pub config_path: Option<String>,
    cleanup: String,
    pub update_gitignore: bool,
    pub outputs: &'static str,
    pub version_pin: version::Mode,
    skip_post_sync: bool,
    allow_post_sync: bool,
    pub sources: Sources,
    base_sources: Sources,
    profile_base_src: String,
    selection: Selection,
    profiles: Vec<String>,
    enabled: BTreeSet<String>,
    profile_tools: BTreeSet<String>,
    protected: Vec<String>,
    pub backup_targets: Vec<String>,
    pub gitignore_generated: Vec<String>,
    pub gitignore_profile: Vec<String>,
    tools: Vec<String>,
    printed: bool,
    pub synced: usize,
    pub skipped: usize,
    pub total: usize,
    pub skipped_names: Vec<String>,
}

#[derive(Default)]
struct Dests {
    agents: String,
    rules: String,
    skills: String,
    commands: String,
    subagents: String,
    settings: String,
    mcp: String,
    hooks: String,
    guard: String,
}

fn io(s: &mut Session, error: Error) -> Stop {
    s.log.err(error.to_string());
    Stop(1)
}

/// What `check` needs: `sync.sh --force` without its transaction and without
/// `shared:`, which `lib/check.sh` merged into the workspace beforehand.
pub fn render(s: &mut Session, env: &Env) -> Step {
    let mut run = prepare(s, env, Selection::default())?;
    refuse_configless_cleanup(s, &run)?;
    check_version_pin(s, &run)?;
    banner(s);
    setup_overlays(s, &mut run, false)?;
    build_catalog(s, &mut run);
    run_passes(s, &mut run)
}

/// `_load_run_config` and `_resolve_sources`.
pub fn prepare(s: &mut Session, env: &Env, selection: Selection) -> Result<Run, Stop> {
    let mut run = load_run_config(s, env, selection)?;
    resolve_sources(s, &mut run)?;
    Ok(run)
}

/// `resolve_project_config_path` and `_load_run_config`.
fn load_run_config(s: &mut Session, env: &Env, selection: Selection) -> Result<Run, Stop> {
    let root = s.paths.root.clone();
    let chosen = project_config::select(&root, env.config_path.as_deref(), &|path: &str| {
        s.ws.is_file(path)
    });
    let config_path = match chosen {
        project_config::Selection::Found(path) => Some(path),
        project_config::Selection::None => None,
        project_config::Selection::Missing(path) => {
            s.log.error(&project_config::missing_message(&path));
            return Err(Stop(1));
        }
    };
    let config = match &config_path {
        Some(path) => {
            let bytes = s.ws.read(path).map_err(|e| io(s, e))?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        None => None,
    };

    fn env_set(value: &Option<String>) -> Option<&str> {
        value.as_deref().filter(|v| !v.is_empty())
    }
    let allow_post_sync = match env_set(&env.allow_post_sync) {
        Some(value) => value == "true",
        None => yaml_subset::value(catalog::GLOBAL_CONFIG, "post_sync.allow") == "true",
    };
    let mut skip_post_sync = env_set(&env.skip_post_sync) == Some("true");
    let mut cleanup = "true".to_string();
    let mut update_gitignore = true;
    let mut outputs = "local";
    let mut version_pin = version::Mode::Warn;
    if let (Some(text), Some(path)) = (&config, &config_path) {
        version_pin = match version::mode(text) {
            Ok(mode) => mode,
            Err(value) => {
                let shown = path
                    .strip_prefix(&format!("{root}/"))
                    .unwrap_or(path)
                    .to_string();
                s.log.error(&format!(
                    "Unknown version_pin.mode '{value}' in {shown} — expected 'warn' or 'strict'"
                ));
                return Err(Stop(1));
            }
        };
        let configured = yaml_subset::value(text, "defaults.cleanup");
        if !configured.is_empty() {
            cleanup = configured;
        }
        if env_set(&env.skip_post_sync).is_none()
            && yaml_subset::value(text, "post_sync.skip") == "true"
        {
            skip_post_sync = true;
        }
        update_gitignore = yaml_subset::value(text, "gitignore.update") != "false";
        outputs = match yaml_subset::value(text, "outputs")
            .replace('"', "")
            .as_str()
        {
            "committed" => "committed",
            "local" => "local",
            "" if !update_gitignore => "committed",
            "" => "local",
            other => {
                let shown = path
                    .strip_prefix(&format!("{root}/"))
                    .unwrap_or(path)
                    .to_string();
                s.log.error(&format!(
                    "Unknown outputs mode '{other}' in {shown} — expected 'committed' or 'local'"
                ));
                return Err(Stop(1));
            }
        };
    }

    Ok(Run {
        config,
        config_path,
        cleanup,
        update_gitignore,
        outputs,
        version_pin,
        skip_post_sync,
        allow_post_sync,
        sources: Sources::default(),
        base_sources: Sources::default(),
        profile_base_src: String::new(),
        selection,
        profiles: Vec::new(),
        enabled: BTreeSet::new(),
        profile_tools: BTreeSet::new(),
        protected: Vec::new(),
        backup_targets: Vec::new(),
        gitignore_generated: Vec::new(),
        gitignore_profile: Vec::new(),
        tools: Vec::new(),
        printed: false,
        synced: 0,
        skipped: 0,
        total: 0,
        skipped_names: Vec::new(),
    })
}

/// `_resolve_sources`.
fn resolve_sources(s: &mut Session, run: &mut Run) -> Step {
    let global = catalog::GLOBAL_CONFIG;
    let root = s.paths.root.clone();
    let detect = |s: &Session, is_file: bool, sub: &str| -> Option<String> {
        [format!(".ai/src/{sub}"), format!(".ai/{sub}")]
            .into_iter()
            .find(|rel| {
                let abs = format!("{root}/{rel}");
                if is_file {
                    s.ws.is_file(&abs)
                } else {
                    s.ws.is_dir(&abs)
                }
            })
    };
    let mut sources = Sources {
        agents: yaml_subset::value(global, "source.agents"),
        rules: yaml_subset::value(global, "source.rules"),
        skills: yaml_subset::value(global, "source.skills"),
        commands: String::new(),
        subagents: String::new(),
    };
    for (is_file, sub, slot) in [
        (true, "AGENTS.md", &mut sources.agents),
        (false, "rules", &mut sources.rules),
        (false, "skills", &mut sources.skills),
        (false, "commands", &mut sources.commands),
        (false, "agents", &mut sources.subagents),
    ] {
        if let Some(found) = detect(s, is_file, sub) {
            *slot = found;
        }
    }
    if let Some(text) = &run.config {
        for (key, slot) in [
            ("agents", &mut sources.agents),
            ("rules", &mut sources.rules),
            ("skills", &mut sources.skills),
            ("commands", &mut sources.commands),
            ("subagents", &mut sources.subagents),
        ] {
            let nested = yaml_subset::value(text, &format!("source.{key}"));
            let chosen = if nested.is_empty() {
                yaml_subset::value(text, key)
            } else {
                nested
            };
            if !chosen.is_empty() {
                *slot = chosen;
            }
        }
    }

    let agents_abs = s
        .paths
        .clone()
        .resolve_source(&sources.agents, "source.agents", &mut s.log)
        .ok_or(Stop(1))?;
    if !s.ws.is_file(&agents_abs) {
        s.log
            .error(&format!("Source agents file not found: {agents_abs}"));
        s.log
            .error("Run 'agentsync init' or set source.agents in agent_sync.yaml");
        return Err(Stop(1));
    }
    run.sources = sources;
    Ok(())
}

/// `_refuse_configless_cleanup_or_exit`: without a project config, a write run
/// whose tools are all disabled would only remove every tool's outputs.
pub fn refuse_configless_cleanup(s: &mut Session, run: &Run) -> Step {
    if run.config.is_some() || s.dry_run {
        return Ok(());
    }
    let legacy_enabled = user_tool_slugs(s)
        .iter()
        .any(|slug| load_tool(s, slug).user_value("enabled") == "true");
    if legacy_enabled {
        return Ok(());
    }
    s.log.error(
        "No project configuration found and no tool is enabled; refusing a sync that would remove every tool's outputs. Run 'agentsync enable <tool>' to create .ai/agent_sync.yaml, or set AGENTSYNC_CONFIG_PATH.",
    );
    Err(Stop(1))
}

/// `_check_version_pin_or_exit`.
pub fn check_version_pin(s: &mut Session, run: &Run) -> Step {
    let Some(config) = &run.config else {
        return Ok(());
    };
    let pinned = yaml_subset::value(config, "agentsync_version").replace('"', "");
    let engine = engine_version();
    if pinned.is_empty() || pinned == engine {
        return Ok(());
    }
    let hint = version::hint(&pinned, engine);
    let committed = run.outputs == "committed";
    if committed || run.version_pin == version::Mode::Strict {
        s.log
            .error(&version::mismatch_error(&pinned, engine, committed));
        for line in hint {
            s.log.err(line);
        }
        return Err(Stop(1));
    }
    s.log.warning(&format!(
        "This project pins agentsync {pinned} but you are running {engine}."
    ));
    for line in hint {
        s.log.out(line);
    }
    Ok(())
}

/// `_print_banner`.
pub fn banner(s: &mut Session) {
    s.log.separator();
    if s.dry_run {
        s.log.info("Starting AgentSync Config Sync (DRY RUN)...");
    } else {
        s.log.info("Starting AgentSync Config Sync...");
    }
    s.log.separator();
    s.log.out(String::new());
}

/// `shared_setup_overlay` when `shared` is set, `base_src_setup_overlay`, and
/// `_snapshot_base_sources`.
pub fn setup_overlays(s: &mut Session, run: &mut Run, shared: bool) -> Step {
    let config = run.config.clone();
    let mut child_src = format!("{}/.ai/src", s.paths.root);
    if shared
        && let Some(text) = config.as_deref()
        && let Some(dir) = overlay::setup_shared(s, text, &mut run.sources).map_err(|e| io(s, e))?
    {
        child_src = format!("{dir}/src");
    }
    overlay::setup_base_src(s, config.as_deref(), &child_src, &mut run.sources)
        .map_err(|e| io(s, e))?;
    run.base_sources = run.sources.clone();
    run.profile_base_src = child_src;
    Ok(())
}

/// `_build_tool_catalog`, the `warm_*_cache` sets, and `_collect_protected_dests`.
pub fn build_catalog(s: &mut Session, run: &mut Run) {
    let text = run.config.clone().unwrap_or_default();
    run.profiles = match &run.selection.profile {
        Some(name) => vec![name.clone()],
        None => profiles::names(&text)
            .into_iter()
            .filter(|name| profiles::is_active(&text, name))
            .collect(),
    };
    load_tools(s, run);
    collect_protected_dests(s, run);
}

/// `list_all_tools`, plus the enabled and profile-tool sets `warm_*_cache` build.
fn load_tools(s: &mut Session, run: &mut Run) {
    let mut all: BTreeSet<String> = catalog::base_tools().into_iter().collect();
    if let Some(text) = &run.config {
        run.enabled.extend(yaml_subset::list(text, "tools.enabled"));
        run.profile_tools.extend(profiles::all_tools(text));
    }
    for slug in user_tool_slugs(s) {
        if load_tool(s, &slug).user_value("enabled") == "true" {
            run.enabled.insert(slug.clone());
        }
        all.insert(slug);
    }
    run.tools = all.into_iter().collect();
}

/// The `.ai/src/tools/<slug>.yaml` overrides, `_`-prefixed templates skipped.
fn user_tool_slugs(s: &Session) -> Vec<String> {
    let tools_dir = format!("{}/.ai/src/tools", s.paths.root);
    s.ws.glob(&tools_dir)
        .into_iter()
        .filter_map(|name| {
            let stem = name.strip_suffix(".yaml")?;
            let listed = !stem.starts_with('_') && s.ws.is_file(&format!("{tools_dir}/{name}"));
            listed.then(|| stem.to_string())
        })
        .collect()
}

/// The layered tool with its `.ai/src/tools/<slug>.yaml` read from the workspace.
fn load_tool(s: &Session, slug: &str) -> Tool {
    let path = format!("{}/.ai/src/tools/{slug}.yaml", s.paths.root);
    let user_yaml =
        s.ws.read(&path)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    Tool::new(slug, user_yaml)
}

/// `_collect_protected_dests`: cleanup never removes what an enabled tool or
/// any profile tool claims; the transaction snapshots every dest the run may
/// change; `.gitignore` gets the dests of enabled and profile tools.
fn collect_protected_dests(s: &mut Session, run: &mut Run) {
    let text = run.config.clone().unwrap_or_default();
    let selected_profile_tools: BTreeSet<String> = run
        .profiles
        .iter()
        .flat_map(|name| profiles::tools(&text, name))
        .collect();

    let slugs = run.tools.clone();
    for slug in &slugs {
        if run.profile_tools.contains(slug) {
            continue;
        }
        let tool = load_tool(s, slug);
        if run.enabled.contains(slug) {
            let dests = collect_tool_dests(s, run, &tool, false);
            if run.selection.includes(slug) {
                run.backup_targets.extend(dests);
            }
        } else if run.cleanup == "true" {
            for key in TARGET_KEYS {
                let raw = tool.value(&format!("targets.{key}.dest"));
                if raw.is_empty() {
                    continue;
                }
                let label = format!("targets.{key}.dest for {slug}");
                if let Some(abs) = s.paths.clone().resolve_dest(&raw, &label, &mut s.log) {
                    run.backup_targets.push(abs);
                }
            }
        }
    }
    for slug in run.profile_tools.clone() {
        let tool = load_tool(s, &slug);
        let dests = collect_tool_dests(s, run, &tool, true);
        if selected_profile_tools.contains(&slug) && run.selection.includes(&slug) {
            run.backup_targets.extend(dests);
        }
    }
}

/// `_collect_tool_dests`: the tool's resolved dests, also recorded for cleanup
/// protection and the `.gitignore` payload.
fn collect_tool_dests(s: &mut Session, run: &mut Run, tool: &Tool, profile: bool) -> Vec<String> {
    let mut collected = Vec::new();
    for key in TARGET_KEYS {
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        let label = format!("targets.{key}.dest for {}", tool.slug);
        let Some(abs) = s.paths.clone().resolve_dest(&raw, &label, &mut s.log) else {
            continue;
        };
        run.protected.push(abs.clone());
        collected.push(abs.clone());
        let Some(mut rel) = s.paths.to_repo_relative(&abs) else {
            s.log
                .error(&format!("Path is outside repository root: {abs}"));
            continue;
        };
        if matches!(key, "rules" | "skills" | "commands" | "subagents") {
            rel.push('/');
        }
        if profile && tool.flag(&format!("targets.{key}.profile_scoped")) != Some(false) {
            run.gitignore_profile.push(rel);
        } else {
            run.gitignore_generated.push(rel);
        }
    }
    collected
}

/// `_run_personal_pass` and `_run_profile_passes`.
pub fn run_passes(s: &mut Session, run: &mut Run) -> Step {
    let slugs = run.tools.clone();
    for slug in &slugs {
        if run.profile_tools.contains(slug) {
            continue;
        }
        run.total += 1;
        checkpoint(s)?;
        if run.enabled.contains(slug) {
            sync_tool(s, run, slug)?;
        } else {
            cleanup_tool(s, run, slug);
        }
        if run.printed {
            s.log.out(String::new());
        }
    }

    let text = run.config.clone().unwrap_or_default();
    for profile in run.profiles.clone() {
        let tools: Vec<String> = profiles::tools(&text, &profile)
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect();
        if tools.is_empty() {
            continue;
        }
        s.log.separator();
        s.log.info(&format!("Profile: {profile}"));
        run.sources = run.base_sources.clone();
        let base_src = run.profile_base_src.clone();
        overlay::setup_profile(s, &text, &profile, &base_src, &mut run.sources)
            .map_err(|e| io(s, e))?;
        for slug in tools {
            run.total += 1;
            checkpoint(s)?;
            sync_tool(s, run, &slug)?;
            if run.printed {
                s.log.out(String::new());
            }
        }
        overlay::cleanup_profile(&mut s.ws).map_err(|e| io(s, e))?;
    }
    Ok(())
}

/// `_resolve_one_dest`.
fn resolve_one_dest(s: &mut Session, tool: &Tool, key: &str, display: &str) -> String {
    if tool.flag(&format!("targets.{key}.enabled")) == Some(false) {
        return String::new();
    }
    let raw = tool.value(&format!("targets.{key}.dest"));
    if raw.is_empty() {
        return String::new();
    }
    let label = format!("targets.{key}.dest for {display}");
    s.paths
        .clone()
        .resolve_dest(&raw, &label, &mut s.log)
        .unwrap_or_default()
}

fn resolve_dests(s: &mut Session, tool: &Tool, display: &str) -> Dests {
    Dests {
        agents: resolve_one_dest(s, tool, "agents", display),
        rules: resolve_one_dest(s, tool, "rules", display),
        skills: resolve_one_dest(s, tool, "skills", display),
        commands: resolve_one_dest(s, tool, "commands", display),
        subagents: resolve_one_dest(s, tool, "subagents", display),
        settings: resolve_one_dest(s, tool, "settings", display),
        mcp: resolve_one_dest(s, tool, "mcp", display),
        hooks: resolve_one_dest(s, tool, "hooks", display),
        guard: resolve_one_dest(s, tool, "guard", display),
    }
}

/// `resolve_source_path` as the steps call it: an unsafe root ends the run.
fn source_path(s: &mut Session, raw: &str, label: &str) -> Result<String, Stop> {
    s.paths
        .clone()
        .resolve_source(raw, label, &mut s.log)
        .ok_or(Stop(1))
}

/// `_resolve_tool_src`.
fn tool_source(
    s: &mut Session,
    tool: &Tool,
    key: &str,
    default: &str,
    display: &str,
) -> Result<String, Stop> {
    let configured = tool.value(&format!("targets.{key}.source"));
    let raw = if configured.is_empty() {
        default
    } else {
        &configured
    };
    source_path(s, raw, &format!("targets.{key}.source for {display}"))
}

/// `sync_tool`.
fn sync_tool(s: &mut Session, run: &mut Run, slug: &str) -> Step {
    let tool = load_tool(s, slug);
    let display = tool.display_name();
    if !run.selection.includes(slug) {
        run.skipped_names.push(display);
        run.skipped += 1;
        run.printed = false;
        return Ok(());
    }
    run.printed = true;
    let dests = resolve_dests(s, &tool, &display);
    s.log.info(&format!("Syncing {display}..."));

    if !dests.agents.is_empty() {
        let src = tool_source(s, &tool, "agents", &run.sources.agents, &display)?;
        file_ops::copy_file(s, &src, &dests.agents).map_err(|e| io(s, e))?;
    }
    checkpoint(s)?;
    sync_rules_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_skills_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_commands_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_subagents_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_payloads_step(s, &tool, &dests)?;
    checkpoint(s)?;

    let post_sync = tool.value("post_sync");
    if !s.dry_run && !run_post_sync_hook(s, run, &display, &post_sync)? {
        s.log.error(&format!(
            "Sync failed because post-sync hook failed for {display}"
        ));
        return Err(Stop(1));
    }
    s.log.success(&format!("{display} complete"));
    run.synced += 1;
    Ok(())
}

/// Where a trapped signal ends the run: Bash's trap fires once the command in
/// progress returns.
pub fn checkpoint(s: &Session) -> Step {
    match s.interrupted() {
        Some(status) => Err(Stop(status)),
        None => Ok(()),
    }
}

/// `run_post_sync_hook`: false when the hook ran and failed.
fn run_post_sync_hook(
    s: &mut Session,
    run: &Run,
    display: &str,
    command: &str,
) -> Result<bool, Stop> {
    if command.is_empty() {
        return Ok(true);
    }
    if run.skip_post_sync {
        s.log.info(&format!(
            "Skipping post-sync hook for {display} (AGENTSYNC_SKIP_POST_SYNC=true)"
        ));
        return Ok(true);
    }
    if !run.allow_post_sync {
        s.log.warning(&format!(
            "Skipping post-sync hook for {display} (set AGENTSYNC_ALLOW_POST_SYNC=true to enable)"
        ));
        return Ok(true);
    }
    s.log.info(&format!("Running post-sync hook: {command}"));
    let succeeded = std::process::Command::new("bash")
        .arg("-lc")
        .arg(command)
        .current_dir(&s.paths.root)
        .status()
        .is_ok_and(|status| status.success());
    checkpoint(s)?;
    if !succeeded {
        s.log.warning("Post-sync hook failed");
    }
    Ok(succeeded)
}

fn sync_rules_step(s: &mut Session, run: &Run, tool: &Tool, dests: &Dests, display: &str) -> Step {
    let src_agents = tool_source(s, tool, "agents", &run.sources.agents, display)?;
    let src_rules = tool_source(s, tool, "rules", &run.sources.rules, display)?;
    let include = tool.filter("targets.rules.include");
    let exclude = tool.filter("targets.rules.exclude");

    if tool.value("targets.rules.inline_into_agents") == "true" && !dests.agents.is_empty() {
        if s.dry_run {
            s.log.step(&format!(
                "Would append rule references to {} (dry-run)",
                paths::leaf(&dests.agents)
            ));
        } else {
            inline_rules_into_agents(s, &src_rules, &dests.agents, &include, &exclude)?;
        }
    } else if !dests.rules.is_empty() {
        if tool.value("targets.rules.merge_to_file") == "true" {
            let prepend = (tool.value("targets.rules.prepend_agents") == "true"
                && s.ws.is_file(&src_agents))
            .then_some(src_agents.as_str());
            rules::merge_rules_to_file(s, &src_rules, &dests.rules, &include, &exclude, prepend)
                .map_err(|e| io(s, e))?;
        } else {
            let extension = tool.value("targets.rules.extension");
            let header = tool.value("targets.rules.header");
            let scoped_header = tool.value("targets.rules.scoped_header");
            let opts = RuleOptions {
                extension: &extension,
                header: &header,
                scoped_header: &scoped_header,
                include: &include,
                exclude: &exclude,
            };
            rules::sync_rules(s, &src_rules, &dests.rules, &opts).map_err(|e| io(s, e))?;
            if tool.value("targets.rules.append_imports") == "true" && !s.dry_run {
                if dests.agents.is_empty() {
                    s.log.warning(&format!(
                        "Skipping append_imports for {display} because targets.agents.dest is missing"
                    ));
                } else {
                    rules::append_imports(s, &dests.agents, &dests.rules).map_err(|e| io(s, e))?;
                    s.log.step(&format!(
                        "Appended @rules imports to {}",
                        paths::leaf(&dests.agents)
                    ));
                }
            }
        }
    }

    if !dests.agents.is_empty()
        && !dests.rules.is_empty()
        && dests.agents.starts_with(&format!("{}/", dests.rules))
        && !s.dry_run
    {
        let _ = file_ops::copy_file(s, &src_agents, &dests.agents);
    }
    Ok(())
}

/// `_inline_rules_into_agents`.
fn inline_rules_into_agents(
    s: &mut Session,
    src_rules: &str,
    dest_agents: &str,
    include: &str,
    exclude: &str,
) -> Step {
    if !s.ws.is_dir(src_rules) {
        return Ok(());
    }
    let mut block = "\n\n## Rules\n\nThe following rule files define project constraints. Read them before making changes:\n\n"
        .as_bytes()
        .to_vec();
    for name in s.ws.glob(src_rules) {
        let path = format!("{src_rules}/{name}");
        if !name.ends_with(".md")
            || !s.ws.is_file(&path)
            || !crate::filters::matches(&name, include, exclude)
        {
            continue;
        }
        let bytes = s.ws.read(&path).map_err(|e| io(s, e))?;
        let title = crate::text::lines(&bytes)
            .into_iter()
            .find(|line| line.starts_with(b"#"))
            .map(|line| {
                let without_hashes = &line[line.iter().take_while(|b| **b == b'#').count()..];
                let spaces = without_hashes.iter().take_while(|b| **b == b' ').count();
                without_hashes[spaces..].to_vec()
            })
            .unwrap_or_default();
        block.extend_from_slice(format!("- `{name}` — ").as_bytes());
        block.extend(title);
        block.push(b'\n');
    }
    block.extend_from_slice("\nFind all rules in `.ai/src/rules/`.\n".as_bytes());
    s.ws.append(dest_agents, &block).map_err(|e| io(s, e))?;
    s.record_write(dest_agents);
    s.log.step(&format!(
        "Appended rule references to {}",
        paths::leaf(dest_agents)
    ));
    Ok(())
}

fn skill_description(skill: &[u8]) -> Vec<u8> {
    let lines = crate::text::lines(skill);
    let mut in_range = false;
    let mut first = Vec::new();
    for line in &lines {
        if !in_range {
            in_range = *line == b"---";
            continue;
        }
        if *line == b"---" {
            in_range = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix(b"description:") {
            let rest = crate::text::trim_start_space(rest);
            let rest = match rest.strip_prefix(b">") {
                Some(after) => crate::text::trim_start_space(after),
                None => rest,
            };
            first = rest.to_vec();
            break;
        }
    }
    if first == b">" {
        first.clear();
    }
    if !first.is_empty() {
        return first;
    }
    let mut outer = false;
    let mut inner = false;
    for line in &lines {
        let mut closes_outer = false;
        if !outer {
            if *line != b"---" {
                continue;
            }
            outer = true;
        } else if *line == b"---" {
            closes_outer = true;
        }
        if !inner {
            inner = line.starts_with(b"description:");
        } else if line.first().is_some_and(u8::is_ascii_lowercase) {
            inner = false;
        } else if line.starts_with(b"  ") {
            return crate::text::trim_start_space(line).to_vec();
        }
        if closes_outer {
            outer = false;
        }
    }
    Vec::new()
}

/// `_inline_skills_into_file`.
fn inline_skills_into_file(
    s: &mut Session,
    src_skills: &str,
    target: &str,
    include: &str,
    exclude: &str,
) -> Step {
    let mut entries = Vec::new();
    for name in s.ws.glob(src_skills) {
        let dir = format!("{src_skills}/{name}");
        if !s.ws.is_dir(&dir) || !crate::filters::matches(&name, include, exclude) {
            continue;
        }
        let skill_file = format!("{dir}/SKILL.md");
        let desc = if s.ws.is_file(&skill_file) {
            skill_description(&s.ws.read(&skill_file).map_err(|e| io(s, e))?)
        } else {
            Vec::new()
        };
        entries.extend_from_slice(format!("- `{name}`").as_bytes());
        if !desc.is_empty() {
            entries.extend_from_slice(" — ".as_bytes());
            entries.extend(desc);
        }
        entries.push(b'\n');
    }
    if entries.is_empty() {
        return Ok(());
    }
    let mut block = "\n## Skills\n\nThe following skills provide step-by-step workflows. Find them in `.ai/src/skills/`:\n\n"
        .as_bytes()
        .to_vec();
    block.extend(entries);
    s.ws.append(target, &block).map_err(|e| io(s, e))?;
    s.record_write(target);
    s.log
        .step(&format!("Appended skill index to {}", paths::leaf(target)));
    Ok(())
}

fn sync_skills_step(s: &mut Session, run: &Run, tool: &Tool, dests: &Dests, display: &str) -> Step {
    let src_skills = tool_source(s, tool, "skills", &run.sources.skills, display)?;
    let include = tool.filter("targets.skills.include");
    let exclude = tool.filter("targets.skills.exclude");

    if !dests.skills.is_empty() {
        let effective = if exclude.is_empty() {
            "command-*".to_string()
        } else {
            format!("{exclude} command-*")
        };
        return file_ops::sync_dir(s, &src_skills, &dests.skills, &include, &effective)
            .map_err(|e| io(s, e));
    }
    if tool.value("targets.skills.inline_into_agents") == "true" && s.ws.is_dir(&src_skills) {
        let target = if !dests.agents.is_empty() {
            dests.agents.clone()
        } else if tool.value("targets.rules.merge_to_file") == "true" && s.ws.is_file(&dests.rules)
        {
            dests.rules.clone()
        } else {
            String::new()
        };
        if !target.is_empty() && !s.dry_run {
            inline_skills_into_file(s, &src_skills, &target, &include, &exclude)?;
        } else if s.dry_run {
            s.log.step("Would append skill index (dry-run)");
        }
    }
    Ok(())
}

fn sync_commands_step(
    s: &mut Session,
    run: &Run,
    tool: &Tool,
    dests: &Dests,
    display: &str,
) -> Step {
    if run.sources.commands.is_empty() {
        return Ok(());
    }
    let include = tool.filter("targets.commands.include");
    let exclude = tool.filter("targets.commands.exclude");
    let label = format!("source.commands for {display}");
    let src = source_path(s, &run.sources.commands, &label)?;
    if !s.ws.is_dir(&src) {
        return Ok(());
    }

    if !dests.commands.is_empty() {
        let result = if tool.value("targets.commands.format") == "toml" {
            rules::sync_converted(s, &src, &dests.commands, Conversion::CommandToml)
        } else {
            let extension = tool.value("targets.commands.extension");
            let opts = RuleOptions {
                extension: &extension,
                header: "",
                scoped_header: "",
                include: "",
                exclude: "",
            };
            rules::sync_rules(s, &src, &dests.commands, &opts)
        };
        return result.map_err(|e| io(s, e));
    }
    if tool.value("targets.commands.as_skills") == "true" && !dests.skills.is_empty() {
        s.log.info(&format!(
            "{display} has no native commands surface — generating skills (command-*) instead"
        ));
        return rules::sync_commands_as_skills(s, &src, &dests.skills, &include, &exclude)
            .map_err(|e| io(s, e));
    }
    if tool.value("targets.commands.inline_into_agents") == "true" {
        let target = if !dests.agents.is_empty() {
            dests.agents.clone()
        } else if tool.value("targets.rules.merge_to_file") == "true" && s.ws.is_file(&dests.rules)
        {
            dests.rules.clone()
        } else {
            String::new()
        };
        if !target.is_empty() && s.dry_run {
            s.log.step("Would append command index (dry-run)");
        } else if !target.is_empty() {
            s.log.info(&format!(
                "{display} has no native commands surface — appending command index to {}",
                paths::leaf(&target)
            ));
            rules::inline_commands_to_file(s, &src, &target, &include, &exclude)
                .map_err(|e| io(s, e))?;
        }
    }
    Ok(())
}

fn sync_subagents_step(
    s: &mut Session,
    run: &Run,
    tool: &Tool,
    dests: &Dests,
    display: &str,
) -> Step {
    if dests.subagents.is_empty() || run.sources.subagents.is_empty() {
        return Ok(());
    }
    let label = format!("source.subagents for {display}");
    let src = source_path(s, &run.sources.subagents, &label)?;
    if !s.ws.is_dir(&src) {
        return Ok(());
    }
    let result = match tool.value("targets.subagents.format").as_str() {
        "toml" => rules::sync_converted(s, &src, &dests.subagents, Conversion::AgentToml),
        "amazonq_json" => {
            rules::sync_converted(s, &src, &dests.subagents, Conversion::AgentAmazonqJson)
        }
        "opencode_md" => {
            rules::sync_converted(s, &src, &dests.subagents, Conversion::AgentOpencodeMd)
        }
        _ => {
            let extension = tool.value("targets.subagents.extension");
            let opts = RuleOptions {
                extension: &extension,
                header: "",
                scoped_header: "",
                include: "",
                exclude: "",
            };
            rules::sync_rules(s, &src, &dests.subagents, &opts)
        }
    };
    result.map_err(|e| io(s, e))
}

fn sync_payloads_step(s: &mut Session, tool: &Tool, dests: &Dests) -> Step {
    let root = s.paths.root.clone();
    let src_settings = if dests.settings.is_empty() {
        None
    } else {
        payload::resolve_source(s, tool, "settings")
    }
    .filter(|path| s.ws.is_file(path));
    let src_mcp = if dests.mcp.is_empty() {
        None
    } else {
        payload::resolve_source(s, tool, "mcp")
    }
    .filter(|path| s.ws.is_file(path));

    if tool.value("targets.mcp.format") == "opencode_json" {
        if let Some(settings) = &src_settings {
            if let Some(mcp) = &src_mcp {
                compose_opencode(s, settings, mcp, &dests.settings)?;
                let label = payload::describe_source(&root, mcp, &tool.slug, "mcp");
                if !label.is_empty() {
                    s.log.step(&format!("mcp source: {label}"));
                }
            } else {
                file_ops::copy_file(s, settings, &dests.settings).map_err(|e| io(s, e))?;
            }
        }
    } else {
        if let Some(settings) = &src_settings {
            file_ops::copy_file(s, settings, &dests.settings).map_err(|e| io(s, e))?;
        }
        if let Some(mcp) = &src_mcp {
            let label = payload::describe_source(&root, mcp, &tool.slug, "mcp");
            file_ops::copy_file(s, mcp, &dests.mcp).map_err(|e| io(s, e))?;
            if !label.is_empty() {
                s.log.step(&format!("mcp source: {label}"));
            }
        }
    }

    for (resource, dest) in [("hooks", &dests.hooks), ("guard", &dests.guard)] {
        if dest.is_empty() {
            continue;
        }
        if let Some(src) = payload::resolve_source(s, tool, resource).filter(|p| s.ws.is_file(p)) {
            file_ops::copy_file(s, &src, dest).map_err(|e| io(s, e))?;
            if resource == "guard" && !s.dry_run {
                s.ws.make_executable(dest).map_err(|e| io(s, e))?;
            }
        }
    }
    Ok(())
}

/// `sync_opencode_config`.
fn compose_opencode(s: &mut Session, settings: &str, mcp: &str, dest: &str) -> Step {
    let settings_text =
        String::from_utf8_lossy(&s.ws.read(settings).map_err(|e| io(s, e))?).into_owned();
    let mcp_text = String::from_utf8_lossy(&s.ws.read(mcp).map_err(|e| io(s, e))?).into_owned();
    match opencode_json::compose(&settings_text, &mcp_text) {
        Err(failure) => {
            let (settings_disp, mcp_disp) = (s.display(settings), s.display(mcp));
            s.log.error(&format!(
                "Cannot compose OpenCode config from {settings_disp} and {mcp_disp}: {}",
                failure.message
            ));
            Err(Stop(failure.code))
        }
        Ok(_) if s.dry_run => {
            s.log.step(&format!(
                "Would compose OpenCode settings and MCP → {} (dry-run)",
                s.display(dest)
            ));
            Ok(())
        }
        Ok(composed) => {
            s.ws.create_dir_all(&paths::parent(dest))
                .map_err(|e| io(s, e))?;
            s.ws.remove(dest).map_err(|e| io(s, e))?;
            s.ws.write(dest, composed.into_bytes())
                .map_err(|e| io(s, e))?;
            s.record_write(dest);
            let line = format!(
                "{} + {} → {}",
                s.display(settings),
                s.display(mcp),
                s.display(dest)
            );
            s.log.step(&line);
            Ok(())
        }
    }
}

/// `cleanup_tool`: remove a disabled tool's unprotected outputs.
fn cleanup_tool(s: &mut Session, run: &mut Run, slug: &str) {
    let tool = load_tool(s, slug);
    let display = tool.display_name();
    run.skipped_names.push(display.clone());
    run.skipped += 1;
    run.printed = false;
    if run.cleanup != "true" {
        return;
    }
    let mut cleaned = false;
    for key in TARGET_KEYS {
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        let label = format!("targets.{key}.dest for {display}");
        let Some(abs) = s.paths.clone().resolve_dest(&raw, &label, &mut s.log) else {
            continue;
        };
        if !run.protected.contains(&abs) && file_ops::cleanup_path(s, &abs) {
            cleaned = true;
        }
    }
    if cleaned {
        s.log.info(&format!("Cleaned up {display} (disabled)"));
        run.printed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_session;
    use crate::workspace::Content;

    fn file(s: &mut Session, path: &str, text: &str) {
        s.ws.insert_file(path, Content::Bytes(text.as_bytes().to_vec()));
    }

    fn text_of(s: &Session, path: &str) -> String {
        String::from_utf8(s.ws.read(path).unwrap()).unwrap()
    }

    fn project() -> Session {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/AGENTS.md", "# Agents\n");
        file(&mut s, "/proj/.ai/src/rules/core.md", "# Core\n");
        file(
            &mut s,
            "/proj/.ai/src/commands/review.md",
            "---\ndescription: Review\n---\nBody\n",
        );
        s
    }

    #[test]
    fn a_project_without_agents_md_stops_with_status_one() {
        let mut s = test_session();
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(2),
            [
                "[ERROR] Source agents file not found: /proj/.ai/src/AGENTS.md",
                "[ERROR] Run 'agentsync init' or set source.agents in agent_sync.yaml"
            ]
        );
    }

    #[test]
    fn an_unknown_outputs_mode_stops_before_the_banner() {
        let mut s = project();
        file(&mut s, "/proj/.ai/agent_sync.yaml", "outputs: shared\n");
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(1),
            [
                "[ERROR] Unknown outputs mode 'shared' in .ai/agent_sync.yaml — expected 'committed' or 'local'"
            ]
        );
    }

    #[test]
    fn claude_renders_agents_rules_commands_payloads_and_the_engine_skill() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        render(&mut s, &Env::default()).unwrap();
        assert_eq!(text_of(&s, "/proj/CLAUDE.md"), "# Agents\n");
        assert_eq!(text_of(&s, "/proj/.claude/rules/core.md"), "# Core\n");
        assert!(s.ws.is_file("/proj/.claude/commands/review.md"));
        assert!(s.ws.is_file("/proj/.claude/skills/agentsync/SKILL.md"));
        assert!(s.ws.is_file("/proj/.claude/settings.json"));
        assert!(s.ws.is_file("/proj/.mcp.json"));
        assert!(s.ws.is_file("/proj/.claude/hooks/agentsync-guard.sh"));
        let touched: Vec<&str> = s.touched().iter().map(String::as_str).collect();
        assert!(touched.contains(&".claude/skills/agentsync/references/maintenance.md"));
        assert!(!touched.contains(&"AGENTS.md"));
    }

    #[test]
    fn a_disabled_tool_is_cleaned_unless_an_enabled_tool_claims_the_dest() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [cursor]\n",
        );
        file(&mut s, "/proj/.codex/agents/x.toml", "x");
        render(&mut s, &Env::default()).unwrap();
        assert!(!s.ws.exists("/proj/.codex/agents"));
        assert!(s.ws.is_file("/proj/AGENTS.md"));
        assert!(
            s.log
                .lines()
                .iter()
                .any(|(_, l)| l == "[INFO] Cleaned up OpenAI Codex (disabled)")
        );
    }

    #[test]
    fn inline_indexes_follow_the_agents_copy_for_codex() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [codex]\n",
        );
        render(&mut s, &Env::default()).unwrap();
        let agents = text_of(&s, "/proj/AGENTS.md");
        assert!(agents.starts_with("# Agents\n\n\n## Rules\n"));
        assert!(agents.contains("- `core.md` — Core\n"));
        assert!(s.ws.is_file("/proj/.agents/skills/command-review/SKILL.md"));
    }

    // Design spec, "Known quirks", item 11: `description: >-` indexes as `-`.
    #[test]
    fn skill_descriptions_come_from_the_frontmatter_scalar_or_its_first_folded_line() {
        assert_eq!(
            skill_description(b"---\nname: a\ndescription: Does A\n---\n"),
            b"Does A"
        );
        assert_eq!(
            skill_description(b"---\ndescription: >\n  Folded first\n  second\nname: x\n---\n"),
            b"Folded first"
        );
        assert_eq!(skill_description(b"---\ndescription: >-\n  x\n---\n"), b"-");
        assert_eq!(skill_description(b"no frontmatter\n"), b"");
    }

    #[test]
    fn an_active_profile_renders_its_variant_under_its_overlay() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\nprofiles:\n  hub:\n    active: true\n    tools: [claude-hub]\n",
        );
        file(
            &mut s,
            "/proj/.ai/src/tools/claude-hub.yaml",
            "base: claude\ntargets:\n  agents:\n    dest: \".claude-hub/CLAUDE.md\"\n  rules:\n    dest: \".claude-hub/rules\"\n",
        );
        file(&mut s, "/proj/.ai/profiles/hub/src/rules/hub.md", "# Hub\n");
        render(&mut s, &Env::default()).unwrap();
        assert!(s.ws.is_file("/proj/.claude-hub/rules/hub.md"));
        assert!(s.ws.is_file("/proj/.claude-hub/rules/core.md"));
        assert!(!s.ws.exists("/proj/.claude/rules/hub.md"));
        assert!(!s.ws.exists("/<agentsync-overlay>/profile"));
    }

    fn with_config(path: &str) -> Env {
        Env {
            config_path: Some(path.to_string()),
            ..Env::default()
        }
    }

    #[test]
    fn a_missing_explicit_config_stops_without_falling_back() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        assert_eq!(render(&mut s, &with_config("missing.yaml")), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            ["[ERROR] AGENTSYNC_CONFIG_PATH is set but file not found: /proj/missing.yaml"]
        );
    }

    #[test]
    fn without_a_config_a_run_with_no_enabled_tool_is_refused() {
        let mut s = project();
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            [
                "[ERROR] No project configuration found and no tool is enabled; refusing a sync that would remove every tool's outputs. Run 'agentsync enable <tool>' to create .ai/agent_sync.yaml, or set AGENTSYNC_CONFIG_PATH."
            ]
        );
    }

    #[test]
    fn without_a_config_a_tool_enabled_in_its_own_yaml_still_renders() {
        let mut s = project();
        file(&mut s, "/proj/.ai/src/tools/claude.yaml", "enabled: true\n");
        assert_eq!(render(&mut s, &Env::default()), Ok(()));
        assert_eq!(text_of(&s, "/proj/CLAUDE.md"), "# Agents\n");
    }

    #[test]
    fn a_strict_pin_stops_local_outputs_and_an_unknown_mode_stops_first() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "outputs: local\nagentsync_version: \"0.0.1\"\nversion_pin:\n  mode: strict\n",
        );
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        let engine = engine_version();
        assert_eq!(
            s.log.tail(3),
            [
                format!("[ERROR] This project pins agentsync 0.0.1 but you are running {engine} — version_pin.mode 'strict' requires local outputs to use the pinned version.").as_str(),
                "  • Match the pin:  agentsync update 0.0.1",
                format!("  • Or move it:     agentsync upgrade-config   (re-pins to {engine}; re-sync and commit the outputs)").as_str(),
            ]
        );

        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "version_pin:\n  mode: refuse\noutputs: shared\n",
        );
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            [
                "[ERROR] Unknown version_pin.mode 'refuse' in .ai/agent_sync.yaml — expected 'warn' or 'strict'"
            ]
        );
    }
}
