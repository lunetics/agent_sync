//! `agentsync enable` and `agentsync disable`: `cmd_enable` and `cmd_disable`
//! of `lib/helpers/enable.sh`, editing `tools.enabled` with `yaml_edit`.

use crate::paths::DiskText;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::tool::Tool;
use crate::output::style::Style;
use crate::paths;
use crate::project::Project;
use crate::{Error, config::catalog, config::edit_paths, config::payload, config::yaml_edit};

const ENABLE_USAGE: &str =
    "Usage: agentsync enable <slug> [<slug>...] [--no-scaffold|--scaffold] [--yes]

  Add one or more tools to the `tools.enabled` list in agent_sync.yaml.
  After enabling, run `agentsync sync` to write that tool's outputs.

  --scaffold       Always scaffold payload files (settings/hooks/mcp).
  --no-scaffold    Never scaffold; skip the payload prompt.
  --yes, -y        Accept any prompts (e.g. project-config creation).

  Run `agentsync list` to see available tool slugs.
";

const ENABLE_SYNOPSIS: &str =
    "agentsync enable <slug> [<slug>...] [--no-scaffold|--scaffold] [--yes]";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scaffold {
    Auto,
    Always,
    Never,
}

fn put(writer: &mut dyn Write, text: &str) -> Result<(), Error> {
    writer
        .write_all(text.as_bytes())
        .map_err(|e| Error::io("<output>", e))
}

/// `_enable_resolve_or_create_config`.
fn resolve_or_create_config(root: &Path) -> Result<PathBuf, Error> {
    let config = root.join(".ai").join("agent_sync.yaml");
    if config.is_file() {
        return Ok(config);
    }
    let legacy = root.join("agent_sync.yaml");
    if legacy.is_file() {
        return Ok(legacy);
    }
    let ai = root.join(".ai");
    std::fs::create_dir_all(&ai).map_err(|e| Error::io(&ai, e))?;
    std::fs::write(
        &config,
        "# AgentSync — Project Configuration\ntools:\n  enabled: []\n",
    )
    .map_err(|e| Error::io(&config, e))?;
    Ok(config)
}

fn tool_exists(project: &Project, slug: &str) -> Result<bool, Error> {
    Ok(catalog::base_tools().iter().any(|t| t == slug)
        || project.user_override_tools()?.iter().any(|t| t == slug))
}

/// The settings and hooks copies `_enable_scaffold_tool_dir` would write.
fn scaffoldable(project: &Project, tool: &Tool) -> Vec<(PathBuf, &'static [u8])> {
    let mut work = Vec::new();
    for resource in ["settings", "hooks"] {
        let (Some(base), Some(user)) = (
            tool.base_payload(resource),
            payload::override_path(project, tool, resource),
        ) else {
            continue;
        };
        if user.is_file()
            || payload::legacy_override_path(project, tool, resource).is_some_and(|p| p.is_file())
        {
            continue;
        }
        work.push((user, base.contents()));
    }
    work
}

pub fn enable(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    interactive: bool,
    confirm: &mut dyn FnMut(&str) -> bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut scaffold = Scaffold::Auto;
    let mut yes = false;
    let mut tools: Vec<String> = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--scaffold" => scaffold = Scaffold::Always,
            "--no-scaffold" => scaffold = Scaffold::Never,
            "--yes" | "-y" => yes = true,
            "--help" | "-h" => {
                put(out, ENABLE_USAGE)?;
                return Ok(0);
            }
            "--" => tools.extend(rest.by_ref().cloned()),
            flag if flag.starts_with('-') => {
                put(
                    err,
                    &format!(
                        "{}: Unknown flag: {flag}\nUsage: {ENABLE_SYNOPSIS}\n",
                        style.red("Error")
                    ),
                )?;
                return Ok(1);
            }
            slug => tools.push(slug.to_string()),
        }
    }
    if tools.is_empty() {
        put(
            err,
            &format!(
                "{}: {ENABLE_SYNOPSIS}\n\nRun {} to see available tools.\n",
                style.red("Error"),
                style.cyan("agentsync list")
            ),
        )?;
        return Ok(1);
    }

    let project = discover()?;
    if !project.tools_dir_in_project() {
        if scaffold == Scaffold::Always {
            return super::refuse_outside_tools_dir(&project, style, err);
        }
        scaffold = Scaffold::Never;
    }
    let config = resolve_or_create_config(&project.root)?;

    let (mut already, mut unknown, mut added) = (0usize, Vec::new(), Vec::new());
    for slug in &tools {
        if !tool_exists(&project, slug)? {
            unknown.push(slug.clone());
        } else if project.enabled_tools()?.contains(slug) {
            already += 1;
        } else {
            yaml_edit::list_append(&config, "tools.enabled", slug)?;
            added.push(slug.clone());
        }
    }

    put(out, "\n")?;
    if !added.is_empty() {
        put(
            out,
            &format!(
                "{}\n",
                style.green(&format!("Enabled {} tool(s)", added.len()))
            ),
        )?;
        for slug in &added {
            let tool = Tool::load(&project, slug)?;
            put(
                out,
                &format!(
                    "    {} {} {}\n",
                    style.green("●"),
                    tool.display_name(),
                    style.dim(&format!("({slug})"))
                ),
            )?;
        }
    }
    if already > 0 {
        put(
            out,
            &format!(
                "\n{}\n",
                style.dim(&format!("{already} tool(s) were already enabled"))
            ),
        )?;
    }
    if !unknown.is_empty() {
        put(out, &format!("\n{}\n", style.yellow("Unknown tool(s):")))?;
        for slug in &unknown {
            put(out, &format!("    {slug}\n"))?;
        }
        put(
            out,
            &format!(
                "\nRun {} to see available tool slugs.\n",
                style.cyan("agentsync list")
            ),
        )?;
    }
    if added.is_empty() {
        return Ok(0);
    }
    for slug in &added {
        let tool = Tool::load(&project, slug)?;
        let work = scaffoldable(&project, &tool);
        let write = match scaffold {
            Scaffold::Always => true,
            Scaffold::Never => false,
            Scaffold::Auto if work.is_empty() => false,
            Scaffold::Auto if interactive && !yes => confirm(&format!(
                "Scaffold editable copies for {}?",
                tool.display_name()
            )),
            Scaffold::Auto => true,
        };
        if write {
            for (path, bytes) in &work {
                let dir = paths::parent(&path.disk_text());
                std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
                std::fs::write(path, bytes).map_err(|e| Error::io(path, e))?;
            }
        }
        put(out, &edit_paths::block(&project, &tool, style))?;
    }
    put(
        out,
        &format!("\nRun {} to apply.\n\n", style.cyan("agentsync sync")),
    )?;
    Ok(0)
}

pub fn disable(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    if args.is_empty() {
        put(
            err,
            &format!(
                "{}: agentsync disable <slug> [<slug>...]\n",
                style.red("Error")
            ),
        )?;
        return Ok(1);
    }
    let project = discover()?;
    if !project.tools_dir_in_project() {
        for slug in args {
            if Tool::load(&project, slug)?.user_value("enabled") == "true" {
                return super::refuse_outside_tools_dir(&project, style, err);
            }
        }
    }
    let config = resolve_or_create_config(&project.root)?;

    let (mut removed, mut not_enabled) = (0usize, 0usize);
    for slug in args {
        if !project.enabled_tools()?.contains(slug) {
            not_enabled += 1;
            continue;
        }
        yaml_edit::list_remove(&config, "tools.enabled", slug)?;
        let user_file = project.user_tool_file(slug);
        if user_file.is_file() && Tool::load(&project, slug)?.user_value("enabled") == "true" {
            yaml_edit::set_scalar(&user_file, "enabled", "false")?;
        }
        removed += 1;
    }

    put(out, "\n")?;
    if removed > 0 {
        put(
            out,
            &format!("{}\n", style.yellow(&format!("Disabled {removed} tool(s)"))),
        )?;
        let enabled = project.enabled_tools()?;
        for slug in args {
            if !enabled.contains(slug) {
                put(
                    out,
                    &format!(
                        "    {} {} {}\n",
                        style.dim("○"),
                        Tool::load(&project, slug)?.display_name(),
                        style.dim(&format!("({slug})"))
                    ),
                )?;
            }
        }
        put(
            out,
            &format!("\nRun {} to apply cleanup.\n", style.cyan("agentsync sync")),
        )?;
    }
    if not_enabled > 0 && removed == 0 {
        put(
            out,
            &format!("{}\n", style.dim("No matching tools were enabled.")),
        )?;
    }
    put(out, "\n")?;
    Ok(0)
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    struct Run {
        status: u8,
        out: String,
        err: String,
    }

    fn project() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join(".ai")).unwrap();
        std::fs::write(
            root.join(".ai/agent_sync.yaml"),
            "tools:\n  enabled:\n    - cursor\n",
        )
        .unwrap();
        (dir, root)
    }

    fn call(root: &std::path::Path, command: &str, args: &[&str]) -> Run {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = if command == "enable" {
            enable(
                &args,
                &discover,
                &Style::plain(),
                false,
                &mut |_| true,
                &mut out,
                &mut err,
            )
        } else {
            disable(&args, &discover, &Style::plain(), &mut out, &mut err)
        }
        .unwrap();
        Run {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    #[test]
    fn enable_appends_scaffolds_and_reports_like_cmd_enable() {
        let (_dir, root) = project();
        let run = call(&root, "enable", &["claude", "cursor", "nope"]);
        assert_eq!(run.status, 0);
        assert_eq!(run.err, "");
        assert_eq!(
            run.out,
            "\nEnabled 1 tool(s)\n    ● Claude Code (claude)\n\n1 tool(s) were already enabled\n\nUnknown tool(s):\n    nope\n\nRun agentsync list to see available tool slugs.\n\n  Claude Code\n    Edit settings: .ai/src/tools/claude/settings.json\n    MCP:           agentsync add mcp <server>  (shared — not yet configured)\n\nRun agentsync sync to apply.\n\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".ai/agent_sync.yaml")).unwrap(),
            "tools:\n  enabled:\n    - cursor\n    - claude\n"
        );
        assert!(root.join(".ai/src/tools/claude/settings.json").is_file());

        let usage = call(&root, "enable", &[]);
        assert_eq!(usage.status, 1);
        assert_eq!(
            usage.err,
            "Error: agentsync enable <slug> [<slug>...] [--no-scaffold|--scaffold] [--yes]\n\nRun agentsync list to see available tools.\n"
        );
        let flag = call(&root, "enable", &["claude", "--bogus"]);
        assert_eq!(flag.status, 1);
        assert_eq!(
            flag.err,
            "Error: Unknown flag: --bogus\nUsage: agentsync enable <slug> [<slug>...] [--no-scaffold|--scaffold] [--yes]\n"
        );
    }

    #[test]
    fn disable_removes_flips_legacy_flags_and_lists_what_is_off() {
        let (_dir, root) = project();
        std::fs::create_dir_all(root.join(".ai/src/tools")).unwrap();
        std::fs::write(root.join(".ai/src/tools/kimi.yaml"), "enabled: true\n").unwrap();
        let run = call(&root, "disable", &["cursor", "kimi", "nope"]);
        assert_eq!(run.status, 0);
        assert_eq!(
            run.out,
            "\nDisabled 2 tool(s)\n    ○ Cursor (cursor)\n    ○ Kimi Code (kimi)\n    ○ nope (nope)\n\nRun agentsync sync to apply cleanup.\n\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".ai/src/tools/kimi.yaml")).unwrap(),
            "enabled: false\n"
        );
        assert_eq!(
            call(&root, "disable", &["cursor"]).out,
            "\nNo matching tools were enabled.\n\n"
        );
        assert_eq!(
            call(&root, "disable", &[]).err,
            "Error: agentsync disable <slug> [<slug>...]\n"
        );
    }
}
