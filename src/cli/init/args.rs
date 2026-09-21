//! The `init` flags, their help text, and the csv helpers that read them.

use super::Run;
use crate::Error;

pub(super) const CONTENT_DEFAULT: &str = "agents,rules,skills,commands,subagents";
pub(super) const CONTENT_VALID: [&str; 5] = ["agents", "rules", "skills", "commands", "subagents"];

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

pub(super) struct Options {
    pub(super) target: Option<String>,
    pub(super) tools: Option<String>,
    pub(super) content: Option<String>,
    pub(super) no_detect: bool,
    pub(super) outputs: String,
    pub(super) existing: String,
    pub(super) ci: String,
    pub(super) run_sync: bool,
    pub(super) no_templates: bool,
    pub(super) assume_yes: bool,
    pub(super) dry_run: bool,
}

pub(super) fn parse_args(args: &[String], run: &mut Run) -> Result<Result<Options, u8>, Error> {
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
pub(super) fn normalize_csv(csv: &str) -> Vec<String> {
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
pub(super) fn merge_lists(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in a.iter().chain(b) {
        if !token.is_empty() && !out.contains(token) {
            out.push(token.clone());
        }
    }
    out
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::{call, project, quiet};
    use std::path::Path;

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
}
