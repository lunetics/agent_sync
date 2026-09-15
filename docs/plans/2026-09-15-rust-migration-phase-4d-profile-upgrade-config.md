# Rust Migration Phase 4d: Native `profile` and `upgrade-config`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync profile` (`add`, `list`, `remove`) and `agentsync upgrade-config` so the binary answers them byte for byte like `lib/helpers/profile.sh` and `cmd_upgrade_config` in `lib/helpers/init.sh`, closing the `yaml_edit` family.

**Architecture:** `src/cli/upgrade_config.rs` is a pure text transform (`upgrade_text`) plus a runner, independent of `init`. `profiles::rewrite_dest` joins the readers already in `src/profiles.rs`. `src/cli/profile.rs` holds the three subcommands on `Project`, `Tool`, `yaml_edit`, and `Paths::resolve_dest`; it maps a missing `AGENTSYNC_CONFIG_PATH` to status 2 as `tool_resolver_select_project_config 2` does. `main` passes raw arguments to both. The seam stays the CLI process boundary: `tests/profiles.bats` under `AGENTSYNC_NATIVE=1` plus parity fixtures.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-15-rust-migration-phase-4c-simplify-resolve.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `profile add` writes variant files, the overlay directory and README, adopted copies, and the `profiles:` entry; `profile remove` deletes only contained config homes, variant files, their payload directories, and the entry; `upgrade-config` rewrites only the selected config's version line.
- No binary ships to users; without a binary every command runs in Bash. No Bash change; `lib/**/*.sh` stays clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state.
- Disk-touching unit tests are `#[cfg(unix)]`.
- Every expected value was captured from Bash on 2026-09-15 by `scratchpad/phase4/profile_reference.sh` (output in `profile_reference.out`).
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-15 under the maintainer's "Go": both as recommended.

1. **`upgrade-config` stays out of the `template_manifest` family.** It shares `init.sh` with `init` but uses none of its helpers. **Recommended:** port it here, closing the `yaml_edit` family. Alternative: wait for `init`, leaving a two-dozen-line command behind a large family.
2. **Quirks kept.** Record as known quirks 26–30: `profile add --tools` keeps spaces around comma-separated names (`'claude, codex'` makes a variant ` codex-hub` and a file `.ai/src/tools/ codex-hub.yaml`) and accepts unknown tools; `profile add` writes `tools: [a,b]` without spaces; `profile add <name> --tools` with no value exits 1 without a message; `profile remove` deletes an adopted config home whose content was copied into the overlay; `upgrade-config` rewrites every `agentsync_version:` line and ignores `AGENTSYNC_CONFIG_PATH`. **Recommended:** reproduce them; the space and silent-exit cases need a user typo.

## Module closure

```text
lib/helpers/profile.sh          13-30   _profile_prepare_context (status 2)
                                33-62   _profile_home_dir, _profile_write_overlay_readme
                                64-117  _profile_variant_targets, _profile_write_variant
                                119-160 _profile_insert_after_line, _profile_register
                                162-195 _profile_adopt_home
                                197-265 _profile_add
                                267-292 _profile_list
                                294-350 _profile_remove
                                352-386 cmd_profile
lib/helpers/profiles.sh         26-32   profile_rewrite_dest
lib/helpers/init.sh             694-736 cmd_upgrade_config
```

Reused: `profiles::{names, overlay_dir, tools, is_active}`, `render::TARGET_KEYS`, `yaml_edit::{find_key_line, remove_key}`, `project::Project`, `tool::Tool` (`value`, `flag`, `display_name`), `paths::Paths::resolve_dest`, `cli::refuse_outside_tools_dir`, `cli::customize::put`, `staging::write_beside`, `prompts::confirm`, `style`.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in profiles guard outputs_mode source_overrides doctor format_migration version_pin native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: this plan's commit; `205 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for every file.

---

### Task 1: Port `upgrade-config`

**Files:**
- Create: `src/cli/upgrade_config.rs`
- Modify: `src/cli/mod.rs` (`pub mod upgrade_config;`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Produces:
  - `pub fn upgrade_text(text: &str, version: &str) -> (String, bool)` — the rewritten config and whether the line was added
  - `pub fn run(root: &Path, version: &str, style: &Style, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`

- [x] **Step 1: Parity fixture, Bash side**

Append to `tests/native_parity.bats`:

```bash
# ── profile / upgrade-config ─────────────────────────────────────────────────

@test "parity: upgrade-config adds or rewrites the pin like Bash" {
    assert_tree_parity upgrade-config
    printf '# head\n\n# more\nformat: 2\nagentsync_version: "0.1"\nagentsync_version: "0.2"' > .ai/agent_sync.yaml
    assert_tree_parity upgrade-config
    printf '# only comments\n  \n' > .ai/agent_sync.yaml
    assert_tree_parity upgrade-config
    rm .ai/agent_sync.yaml
    printf 'tools:\n  enabled: []\n' > agent_sync.yaml
    assert_tree_parity upgrade-config
    rm agent_sync.yaml
    assert_tree_parity upgrade-config
}
```

Run: `bats --tap -f 'upgrade-config adds' tests/native_parity.bats`
Expected: `ok`.

- [x] **Step 2: Write the failing test**

Create `src/cli/upgrade_config.rs` with the tests module only; add `pub mod upgrade_config;`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_is_inserted_after_leading_comments_or_every_line_is_rewritten() {
        let cases: [(&str, &str, bool); 5] = [
            (
                "# AgentSync — Project Configuration\ntools:\n  enabled:\n    - claude\n\n",
                "# AgentSync — Project Configuration\nagentsync_version: \"9.9.9\"\n\ntools:\n  enabled:\n    - claude\n\n",
                true,
            ),
            (
                "# head\n\n# more\nformat: 2\nagentsync_version: \"0.1\"\nagentsync_version: \"0.2\"\n",
                "# head\n\n# more\nformat: 2\nagentsync_version: \"9.9.9\"\nagentsync_version: \"9.9.9\"\n",
                false,
            ),
            (
                "# only comments\n\n",
                "# only comments\n\nagentsync_version: \"9.9.9\"\n",
                true,
            ),
            ("", "agentsync_version: \"9.9.9\"\n", true),
            ("agentsync_version: 1", "agentsync_version: \"9.9.9\"", false),
        ];
        for (text, expected, added) in cases {
            assert_eq!(upgrade_text(text, "9.9.9"), (expected.to_string(), added), "{text:?}");
        }
    }
}
```

Run: `cargo test --lib cli::upgrade_config 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function 'upgrade_text'`.

- [x] **Step 3: Write the implementation**

Above the tests module:

```rust
//! `agentsync upgrade-config`: `cmd_upgrade_config` of `lib/helpers/init.sh`,
//! which pins `agentsync_version` to the running engine.

use std::io::Write;
use std::path::Path;

use super::customize::put;
use crate::style::Style;
use crate::{Error, staging};

const KEY: &str = "agentsync_version:";

/// The `awk` insertion or the `sed` rewrite, as `cmd_upgrade_config` picks it.
pub fn upgrade_text(text: &str, version: &str) -> (String, bool) {
    let pin = format!("agentsync_version: \"{version}\"");
    if text.split('\n').any(|line| line.starts_with(KEY)) {
        let rewritten: Vec<String> = text
            .split('\n')
            .map(|line| if line.starts_with(KEY) { pin.clone() } else { line.to_string() })
            .collect();
        return (rewritten.join("\n"), false);
    }
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\x0b' | '\x0c' | '\r');
    let mut out = String::new();
    let mut inserted = false;
    for line in lines {
        let stripped = line.trim_start_matches(is_space);
        if !inserted && !(stripped.is_empty() || stripped.starts_with('#')) {
            out.push_str(&format!("{pin}\n\n"));
            inserted = true;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !inserted {
        out.push_str(&format!("{pin}\n"));
    }
    (out, true)
}

pub fn run(
    root: &Path,
    version: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let config = [root.join(".ai").join("agent_sync.yaml"), root.join("agent_sync.yaml")]
        .into_iter()
        .find(|path| path.is_file());
    let Some(config) = config else {
        put(
            err,
            format!(
                "{}: No agent_sync.yaml found in {}\nRun {} first.\n",
                style.red("Error"),
                root.to_string_lossy(),
                style.cyan("agentsync init")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let bytes = std::fs::read(&config).map_err(|e| Error::io(&config, e))?;
    let (text, added) = upgrade_text(&String::from_utf8_lossy(&bytes), version);
    staging::write_beside(&config, text.as_bytes())?;
    let shown = style.dim(&config.to_string_lossy());
    let line = if added {
        format!("{}: agentsync_version: {version} → {shown}\n", style.green("Added"))
    } else {
        format!("{}: agentsync_version → {version} {shown}\n", style.green("Updated"))
    };
    put(out, line.as_bytes())?;
    Ok(0)
}
```

In `src/main.rs`, before the raw-dispatch block:

```rust
    if args.first().and_then(|a| a.to_str()) == Some("upgrade-config") {
        let root = project_root()?;
        return cli::upgrade_config::run(
            Path::new(&root),
            engine_version(),
            &Style::for_stdout(),
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
```

adding `use std::path::Path;` next to the `PathBuf` import (`use std::path::{Path, PathBuf};`). In `bin/agentsync.sh:280` append `upgrade-config`.

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in doctor format_migration version_pin; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
bats --tap -f 'upgrade-config adds' tests/native_parity.bats
```

Expected: `206 passed`, `0`, `11`, `1`; `0` for each file; `ok`.

- [x] **Step 5: Prove the fixture bites, lint, commit**

Change `"Updated"` to `"Rewrote"`, rebuild, rerun the fixture: `not ok` with that diff; revert and rebuild.

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/upgrade_config.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-15-rust-migration-phase-4d-profile-upgrade-config.md
git commit -m "feat(native): port upgrade-config"
```

---

### Task 2: Port `profile`

**Files:**
- Modify: `src/profiles.rs` (new `rewrite_dest`; test)
- Create: `src/cli/profile.rs`
- Modify: `src/cli/mod.rs` (`pub mod profile;`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Produces:
  - `profiles::rewrite_dest(base_dest: &str, home: &str) -> String`
  - `pub fn profile(args: &[String], discover: &dyn Fn() -> Result<Project, Error>, style: &Style, interactive: bool, confirm: &mut dyn FnMut(&str) -> bool, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`

- [x] **Step 1: Parity fixture, Bash side**

```bash
@test "parity: profile adds, lists, adopts, and removes like Bash" {
    enable_tools claude cursor
    assert_tree_parity profile
    assert_tree_parity profile --help
    assert_tree_parity profile bogus
    assert_tree_parity profile add
    assert_tree_parity profile add 'bad name'
    assert_tree_parity profile add hub --bogus
    assert_tree_parity profile add hub extra
    assert_tree_parity profile add hub --tools
    assert_tree_parity profile add hub --tools 'claude, codex,nope'
    _run_engine 0 profile add hub --tools 'claude,nope' >/dev/null
    assert_tree_parity profile add hub
    assert_tree_parity profile add work
    _run_engine 0 profile add work >/dev/null
    mkdir -p .claude-home/rules .claude-home/skills/s
    printf 'r\n' > .claude-home/rules/r.md
    printf 's\n' > .claude-home/skills/s/SKILL.md
    printf '# home\n' > .claude-home/CLAUDE.md
    printf '{}\n' > .claude-home/settings.json
    assert_tree_parity profile add home --tools claude --adopt
    _run_engine 0 profile add home --tools claude --adopt >/dev/null
    assert_tree_parity profile list
    assert_tree_parity profile ls extra
    mkdir -p .claude-hub/rules .ai/src/tools/claude-hub
    printf 'x\n' > .claude-hub/rules/x.md
    printf '{}\n' > .ai/src/tools/claude-hub/mcp.json
    assert_tree_parity profile remove
    assert_tree_parity profile remove nope
    assert_tree_parity profile remove hub --yes
    _run_engine 0 profile remove hub --yes >/dev/null
    _run_engine 0 profile remove work -y >/dev/null
    assert_tree_parity profile remove home -y
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_tree_parity profile list
    rm .ai/agent_sync.yaml
    assert_tree_parity profile add x
}
```

Run: `bats --tap -f 'profile adds' tests/native_parity.bats`
Expected: `ok`.

- [x] **Step 2: Write the failing tests**

Append inside the tests module of `src/profiles.rs`:

```rust
    #[test]
    fn a_dest_moves_under_the_config_home_without_its_tool_directory() {
        assert_eq!(rewrite_dest(".claude/rules", ".h"), ".h/rules");
        assert_eq!(rewrite_dest(".amazonq/rules/x.md", ".h"), ".h/rules/x.md");
        assert_eq!(rewrite_dest("CLAUDE.md", ".h"), ".h/CLAUDE.md");
        assert_eq!(rewrite_dest(".mcp.json", ".h"), ".h/.mcp.json");
    }
```

Create `src/cli/profile.rs` with the tests module only; add `pub mod profile;`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().to_string_lossy().into_owned();
        std::fs::create_dir_all(format!("{root}/.ai")).unwrap();
        std::fs::write(format!("{root}/.ai/agent_sync.yaml"), "tools:\n  enabled:\n    - claude\n").unwrap();
        (dir, root)
    }

    fn call(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = profile(&args, &discover, &Style::plain(), false, &mut |_| true, &mut out, &mut err)
            .unwrap();
        (status, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn a_profile_is_added_listed_and_removed_like_bash() {
        let (_dir, root) = project();
        assert_eq!(
            call(&root, &["add", "hub"]),
            (
                0,
                "\n  Creating profile 'hub'\n    ✓ claude → claude-hub  (.claude-hub/)\n\n  Profile 'hub' registered.\n    overlay:  .ai/profiles/hub/src/   (drop profile-only rules/skills here)\n    sync:     agentsync sync --profile hub\n\n".to_string(),
                String::new()
            )
        );
        let config = format!("{root}/.ai/agent_sync.yaml");
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "tools:\n  enabled:\n    - claude\n\nprofiles:\n  hub:\n    overlay: \".ai/profiles/hub\"\n    active: true\n    tools: [claude-hub]\n"
        );
        assert_eq!(
            std::fs::read_to_string(format!("{root}/.ai/src/tools/claude-hub.yaml")).unwrap(),
            "# Claude Code (hub) — AgentSync profile variant (profile: hub).\n# Generated by `agentsync profile add`. Inherits unset fields from base `claude`.\nbase: claude\nname: \"Claude Code (hub)\"\nprofile_home: \".claude-hub\"\ntargets:\n  agents:\n    dest: \".claude-hub/CLAUDE.md\"\n  rules:\n    dest: \".claude-hub/rules\"\n  skills:\n    dest: \".claude-hub/skills\"\n  commands:\n    dest: \".claude-hub/commands\"\n  subagents:\n    dest: \".claude-hub/agents\"\n  settings:\n    dest: \".claude-hub/settings.json\"\n  mcp:\n    dest: \".claude-hub/.mcp.json\"\n  guard:\n    dest: \".claude/hooks/agentsync-guard.sh\"\n"
        );
        assert!(std::path::Path::new(&format!("{root}/.ai/profiles/hub/README.md")).is_file());
        assert_eq!(
            call(&root, &["list"]).1,
            "\n  Profiles\n    hub  [active]\n      overlay: .ai/profiles/hub/src/\n      tool:    claude-hub  →  .claude-hub/\n\n"
        );

        std::fs::create_dir_all(format!("{root}/.claude-hub/rules")).unwrap();
        assert_eq!(
            call(&root, &["remove", "hub", "-y"]).1,
            "\n  Removing profile 'hub'\n    Deletes config-home output, variant tool files, and the profiles entry.\n    Overlay sources under .ai/profiles/hub/ are kept.\n\n    ✓ removed .claude-hub/\n\n  Profile 'hub' removed.\n"
        );
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "tools:\n  enabled:\n    - claude\n\n"
        );
        assert!(!std::path::Path::new(&format!("{root}/.claude-hub")).exists());
        assert!(!std::path::Path::new(&format!("{root}/.ai/src/tools/claude-hub.yaml")).exists());
    }

    #[test]
    fn arguments_are_refused_with_the_bash_statuses() {
        let (_dir, root) = project();
        assert_eq!(
            call(&root, &["bogus"]),
            (2, String::new(), "Error: unknown subcommand: bogus\nTry: agentsync profile --help\n".to_string())
        );
        assert_eq!(
            call(&root, &["add", "bad name"]).2,
            "Error: profile name must be [a-zA-Z0-9_-].\n"
        );
        assert_eq!(call(&root, &["add", "hub", "--tools"]), (1, String::new(), String::new()));
        assert_eq!(
            call(&root, &["remove", "nope"]),
            (1, String::new(), format!("Error: no profile 'nope' in {root}/.ai/agent_sync.yaml\n"))
        );
        assert_eq!(
            call(&root, &[]).1,
            "\n  No profiles. Create one with: agentsync profile add <name>\n"
        );
    }
}
```

Run: `cargo test --lib 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function` for `rewrite_dest` and `profile`.

- [x] **Step 3: Write the implementation**

`src/profiles.rs`, after `all_tools`:

```rust
/// `profile_rewrite_dest`: drop the leading tool directory when there is one
/// and re-root under the config home.
pub fn rewrite_dest(base_dest: &str, home: &str) -> String {
    let rel = base_dest.split_once('/').map_or(base_dest, |(_, rest)| rest);
    format!("{home}/{rel}")
}
```

Above the tests module in `src/cli/profile.rs`:

```rust
//! `agentsync profile`: `cmd_profile` of `lib/helpers/profile.sh`, which
//! scaffolds, lists, and removes config-home variants of tools.

use std::io::Write;
use std::path::Path;

use super::customize::put;
use crate::log::Log;
use crate::paths::Paths;
use crate::project::Project;
use crate::render::TARGET_KEYS;
use crate::style::Style;
use crate::tool::Tool;
use crate::{Error, profiles, yaml_edit};

const USAGE: &str = "Usage: agentsync profile <command>

  add <name> [--tools a,b] [--adopt]   Scaffold variant tools + overlay + config entry
  list                                 Show profiles, their tools and config homes
  remove <name> [--yes]                Delete config-home output, variants, and entry

Sync a profile:  agentsync sync --profile <name>
Active profiles also sync on a plain: agentsync sync
";

type Discover<'a> = &'a dyn Fn() -> Result<Project, Error>;

pub fn profile(
    args: &[String],
    discover: Discover,
    style: &Style,
    interactive: bool,
    confirm: &mut dyn FnMut(&str) -> bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map_or("list", String::as_str) {
        "add" => add(rest, discover, style, out, err),
        "list" | "ls" => list(discover, style, out, err),
        "remove" | "rm" => remove(rest, discover, style, interactive, confirm, out, err),
        "--help" | "-h" | "help" => {
            put(out, USAGE.as_bytes())?;
            Ok(0)
        }
        other => {
            put(
                err,
                format!(
                    "{}: unknown subcommand: {other}\nTry: agentsync profile --help\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            Ok(2)
        }
    }
}

/// `_profile_prepare_context`: a missing explicit config is status 2.
fn context(discover: Discover, style: &Style, err: &mut dyn Write) -> Result<Result<Project, u8>, Error> {
    match discover() {
        Ok(project) => Ok(Ok(project)),
        Err(Error::ConfigPathNotFound(path)) => {
            put(
                err,
                format!(
                    "{}: AGENTSYNC_CONFIG_PATH is set but file not found: {}\n",
                    style.red("Error"),
                    path.to_string_lossy()
                )
                .as_bytes(),
            )?;
            Ok(Err(2))
        }
        Err(other) => Err(other),
    }
}

fn config_text(project: &Project) -> Result<String, Error> {
    match &project.config_path {
        Some(path) => std::fs::read(path)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .map_err(|e| Error::io(path, e)),
        None => Ok(String::new()),
    }
}

fn config_shown(project: &Project) -> String {
    project
        .config_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn usage_error(style: &Style, err: &mut dyn Write, message: &str) -> Result<u8, Error> {
    put(err, format!("{}: {message}\n", style.red("Error")).as_bytes())?;
    Ok(2)
}

/// `_profile_add`.
fn add(
    args: &[String],
    discover: Discover,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (mut name, mut tools_csv, mut adopt) = (String::new(), String::new(), false);
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--tools" => {
                tools_csv = args.get(index + 1).cloned().unwrap_or_default();
                if index + 1 >= args.len() {
                    return Ok(1);
                }
                index += 2;
                continue;
            }
            "--adopt" => adopt = true,
            "--yes" | "-y" => {}
            flag if flag.starts_with('-') => {
                return usage_error(style, err, &format!("unknown flag: {flag}"));
            }
            value if name.is_empty() => name = value.to_string(),
            _ => return usage_error(style, err, "too many arguments."),
        }
        index += 1;
    }
    if name.is_empty() {
        return usage_error(style, err, "agentsync profile add <name> [--tools a,b] [--adopt]");
    }
    if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        return usage_error(style, err, "profile name must be [a-zA-Z0-9_-].");
    }
    let project = match context(discover, style, err)? {
        Ok(project) => project,
        Err(status) => return Ok(status),
    };
    if !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, err);
    }
    let Some(config) = project.config_path.clone() else {
        put(
            err,
            format!("{}: no agent_sync.yaml — run 'agentsync init' first.\n", style.red("Error")).as_bytes(),
        )?;
        return Ok(1);
    };
    let text = config_text(&project)?;
    if yaml_edit::find_key_line(&text, &format!("profiles.{name}")).is_some() {
        put(
            err,
            format!(
                "{} in {}\n",
                style.yellow(&format!("Profile '{name}' already exists")),
                config.to_string_lossy()
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let base_tools: Vec<String> = if tools_csv.is_empty() {
        project.configured_enabled_tools()?
    } else {
        tools_csv.split(',').filter(|t| !t.is_empty()).map(str::to_string).collect()
    };
    if base_tools.is_empty() {
        put(
            err,
            format!("{}: no base tools — enable tools first or pass --tools.\n", style.red("Error")).as_bytes(),
        )?;
        return Ok(1);
    }

    let overlay_rel = format!(".ai/profiles/{name}");
    let overlay_root = project.root.join(&overlay_rel);
    let overlay_src = overlay_root.join("src");
    std::fs::create_dir_all(&overlay_src).map_err(|e| Error::io(&overlay_src, e))?;
    let readme = overlay_root.join("README.md");
    if !readme.is_file() {
        std::fs::write(&readme, readme_text(&name)).map_err(|e| Error::io(&readme, e))?;
    }
    put(out, format!("\n{}\n", style.bold(&format!("  Creating profile '{name}'"))).as_bytes())?;

    let mut variants = Vec::new();
    for base in &base_tools {
        let variant = format!("{base}-{name}");
        let home = format!(".{base}-{name}");
        write_variant(&project, base, &variant, &home, &name)?;
        put(
            out,
            format!("    {} {base} → {}  ({home}/)\n", style.green("✓"), style.cyan(&variant)).as_bytes(),
        )?;
        if adopt {
            adopt_home(&project, &variant, &home, &overlay_src)?;
        }
        variants.push(variant);
    }
    register(&config, &name, &overlay_rel, &variants)?;
    put(
        out,
        format!(
            "\n{}\n    overlay:  {overlay_rel}/src/   {}\n    sync:     {}\n\n",
            style.green(&format!("  Profile '{name}' registered.")),
            style.dim("(drop profile-only rules/skills here)"),
            style.cyan(&format!("agentsync sync --profile {name}"))
        )
        .as_bytes(),
    )?;
    Ok(0)
}

/// `_profile_write_overlay_readme`.
fn readme_text(name: &str) -> String {
    format!(
        "# Profile: {name} — overlay source\n\nDrop profile-only content under `src/`; it layers over the base `.ai/src/` at\nsync time (the profile wins on path conflicts):\n\n  src/AGENTS.md   src/rules/*.md   src/skills/<name>/   src/commands/*.md   src/agents/*.md\n\nAnything not present here is inherited from the base — an empty overlay mirrors\nthe base. Profile-specific MCP/settings/hooks go in `.ai/src/tools/<tool>-{name}/`.\n"
    )
}

/// `_profile_write_variant` with `_profile_variant_targets`.
fn write_variant(project: &Project, base: &str, variant: &str, home: &str, name: &str) -> Result<(), Error> {
    let tool = Tool::load(project, base)?;
    let label = format!("{} ({name})", tool.display_name());
    let mut text = format!(
        "# {label} — AgentSync profile variant (profile: {name}).\n# Generated by `agentsync profile add`. Inherits unset fields from base `{base}`.\nbase: {base}\nname: \"{label}\"\nprofile_home: \"{home}\"\n"
    );
    let mut emitted = false;
    for key in TARGET_KEYS {
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        if !emitted {
            text.push_str("targets:\n");
            emitted = true;
        }
        let dest = if tool.flag(&format!("targets.{key}.profile_scoped")) == Some(false) {
            raw
        } else {
            profiles::rewrite_dest(&raw, home)
        };
        text.push_str(&format!("  {key}:\n    dest: \"{dest}\"\n"));
    }
    let file = project.user_tool_file(variant);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    std::fs::write(&file, text).map_err(|e| Error::io(&file, e))
}

/// `_profile_register`: a new child right after `profiles:`, or a new block.
fn register(config: &Path, name: &str, overlay_rel: &str, variants: &[String]) -> Result<(), Error> {
    let child = format!(
        "  {name}:\n    overlay: \"{overlay_rel}\"\n    active: true\n    tools: [{}]\n",
        variants.join(",")
    );
    let bytes = std::fs::read(config).map_err(|e| Error::io(config, e))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let Some((lineno, _)) = yaml_edit::find_key_line(&text, "profiles") else {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(config)
            .map_err(|e| Error::io(config, e))?;
        return file
            .write_all(format!("\nprofiles:\n{child}").as_bytes())
            .map_err(|e| Error::io(config, e));
    };
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut updated = String::new();
    for (index, line) in lines.into_iter().enumerate() {
        updated.push_str(line);
        updated.push('\n');
        if index + 1 == lineno {
            updated.push_str(&child);
        }
    }
    crate::staging::write_beside(config, updated.as_bytes())
}

/// `cp -RL <src>/. <dst>/`: links followed, unreadable entries skipped.
fn copy_following(src: &Path, dst: &Path) {
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    let _ = std::fs::create_dir_all(dst);
    for entry in entries.filter_map(|e| e.ok()) {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        match std::fs::metadata(&from) {
            Ok(meta) if meta.is_dir() => copy_following(&from, &to),
            Ok(_) => {
                let _ = std::fs::copy(&from, &to);
            }
            Err(_) => {}
        }
    }
}

/// `_profile_adopt_home`.
fn adopt_home(project: &Project, variant: &str, home: &str, overlay_src: &Path) -> Result<(), Error> {
    let home_abs = project.root.join(home);
    if !home_abs.is_dir() {
        return Ok(());
    }
    for item in ["rules", "skills", "commands", "agents"] {
        let from = home_abs.join(item);
        if from.is_dir() {
            copy_following(&from, &overlay_src.join(item));
        }
    }
    let mut markdown: Vec<_> = std::fs::read_dir(&home_abs)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    !name.starts_with('.') && name.ends_with(".md") && p.is_file()
                })
                .collect()
        })
        .unwrap_or_default();
    markdown.sort();
    if let Some(first) = markdown.first() {
        let target = overlay_src.join("AGENTS.md");
        std::fs::copy(first, &target).map_err(|e| Error::io(&target, e))?;
    }
    let payload_dir = project.user_tools_dir().join(variant);
    for (from, to) in [(".mcp.json", "mcp.json"), ("settings.json", "settings.json"), ("hooks.json", "hooks.json")] {
        let source = home_abs.join(from);
        if source.is_file() {
            std::fs::create_dir_all(&payload_dir).map_err(|e| Error::io(&payload_dir, e))?;
            let target = payload_dir.join(to);
            std::fs::copy(&source, &target).map_err(|e| Error::io(&target, e))?;
        }
    }
    Ok(())
}

/// `_profile_list`.
fn list(discover: Discover, style: &Style, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error> {
    let project = match context(discover, style, err)? {
        Ok(project) => project,
        Err(status) => return Ok(status),
    };
    let text = config_text(&project)?;
    let names = profiles::names(&text);
    if names.is_empty() {
        put(
            out,
            format!("\n{}\n", style.dim("  No profiles. Create one with: agentsync profile add <name>")).as_bytes(),
        )?;
        return Ok(0);
    }
    let mut body = format!("\n{}\n", style.bold("  Profiles"));
    for name in &names {
        let label = if profiles::is_active(&text, name) {
            style.green("active")
        } else {
            style.dim("inactive")
        };
        body.push_str(&format!(
            "    {}  [{label}]\n      overlay: {}/src/\n",
            style.cyan(name),
            profiles::overlay_dir(&text, name)
        ));
        for tool in profiles::tools(&text, name) {
            let home = Tool::load(&project, &tool)?.value("profile_home");
            body.push_str(&format!("      tool:    {tool}  →  {home}/\n"));
        }
    }
    body.push('\n');
    put(out, body.as_bytes())?;
    Ok(0)
}

/// `_profile_remove`.
#[allow(clippy::too_many_arguments)]
fn remove(
    args: &[String],
    discover: Discover,
    style: &Style,
    interactive: bool,
    confirm: &mut dyn FnMut(&str) -> bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (mut name, mut assume_yes) = (String::new(), false);
    for arg in args {
        match arg.as_str() {
            "--yes" | "-y" => assume_yes = true,
            flag if flag.starts_with('-') => {
                return usage_error(style, err, &format!("unknown flag: {flag}"));
            }
            value if name.is_empty() => name = value.to_string(),
            _ => return usage_error(style, err, "too many arguments."),
        }
    }
    if name.is_empty() {
        return usage_error(style, err, "agentsync profile remove <name>");
    }
    let project = match context(discover, style, err)? {
        Ok(project) => project,
        Err(status) => return Ok(status),
    };
    if !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, err);
    }
    let text = config_text(&project)?;
    if yaml_edit::find_key_line(&text, &format!("profiles.{name}")).is_none() {
        put(
            err,
            format!("{}: no profile '{name}' in {}\n", style.red("Error"), config_shown(&project)).as_bytes(),
        )?;
        return Ok(1);
    }
    let variants = profiles::tools(&text, &name);
    put(
        out,
        format!(
            "\n{}\n    Deletes config-home output, variant tool files, and the profiles entry.\n    Overlay sources under .ai/profiles/{name}/ are kept.\n\n",
            style.yellow(&format!("  Removing profile '{name}'"))
        )
        .as_bytes(),
    )?;
    if !assume_yes && interactive && !confirm("Proceed?") {
        put(out, format!("{}\n", style.dim("Cancelled.")).as_bytes())?;
        return Ok(0);
    }
    let paths = Paths::on_disk(&project.root.to_string_lossy());
    for variant in &variants {
        let home = Tool::load(&project, variant)?.value("profile_home");
        if !home.is_empty() {
            let mut quiet = Log::default();
            let resolved = paths.resolve_dest(&home, &format!("profile_home for {variant}"), &mut quiet);
            if let Some(abs) = resolved.filter(|abs| Path::new(abs).is_dir()) {
                std::fs::remove_dir_all(&abs).map_err(|e| Error::io(&abs, e))?;
                put(out, format!("    {} removed {home}/\n", style.green("✓")).as_bytes())?;
            }
        }
        let file = project.user_tool_file(variant);
        if file.is_file() {
            std::fs::remove_file(&file).map_err(|e| Error::io(&file, e))?;
        }
        let _ = std::fs::remove_dir_all(project.user_tools_dir().join(variant));
    }
    if let Some(config) = &project.config_path {
        yaml_edit::remove_key(config, &format!("profiles.{name}"))?;
        if profiles::names(&config_text(&project)?).is_empty() {
            yaml_edit::remove_key(config, "profiles")?;
        }
    }
    put(out, format!("\n{}\n", style.green(&format!("  Profile '{name}' removed."))).as_bytes())?;
    Ok(0)
}
```

`Paths::resolve_dest` checks containment through the disk as `resolve_dest_path` does; its log lines are dropped as `2>/dev/null` drops them. `std::fs::remove_dir_all` removes a symlinked home as a link, as `rm -rf` does.

In `src/main.rs`: add `"profile"` to the raw dispatch and the branch

```rust
            "profile" => cli::profile::profile(
                &rest,
                &Project::discover,
                &style,
                prompts::is_tty(),
                &mut |question: &str| prompts::confirm(question, false),
                &mut out,
                &mut err,
            ),
```

In `bin/agentsync.sh:280` append `profile`.

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in profiles guard outputs_mode source_overrides; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
bats --tap -f 'profile adds' tests/native_parity.bats
```

Expected: `209 passed`, `0`, `11`, `1`; `0` for each file; `ok`.

- [x] **Step 5: Prove the fixture bites, lint, commit**

Change `variants.join(",")` to `variants.join(", ")`, rebuild, rerun the fixture: `not ok` with a tree diff in `.ai/agent_sync.yaml`; revert and rebuild.

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/profiles.rs src/cli/profile.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-15-rust-migration-phase-4d-profile-upgrade-config.md
git commit -m "feat(native): port profile"
```

---

### Task 3: Verify, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [x] **Step 1: Spec**

Append to "Known quirks":

```markdown
26. `profile add --tools` keeps spaces around comma-separated names and accepts
    unknown tools: `'claude, codex'` writes `.ai/src/tools/ codex-hub.yaml`.
27. `profile add` writes the profile's `tools:` list as `[a,b]`, without spaces.
28. `profile add <name> --tools` with no value exits 1 without a message.
29. `profile remove` deletes an adopted config home, whose content was copied
    into the overlay.
30. `upgrade-config` rewrites every `agentsync_version:` line and ignores
    `AGENTSYNC_CONFIG_PATH`.
```

- [x] **Step 2: Module map and outputs**

Set the `lib/helpers/profile.sh` row to `→ src/cli/profile.rs      Phase 4d, ported`, the `lib/helpers/init.sh` row to `→ src/cli/{init,upgrade_config}.rs   upgrade_config ported in Phase 4d; init waits for the template_manifest family`, and the `lib/helpers/profiles.sh` row's note to `names, overlay dir, tools, active, rewrite_dest`. Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [x] **Step 3: Verify (outside the agent sandbox)**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in profiles guard outputs_mode source_overrides doctor format_migration version_pin native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `209 passed`, `0`, `11`, `1`; lint exit 0; every line `bash=0 native=0`.

- [x] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-15-rust-migration-phase-4d-profile-upgrade-config.md
git commit -m "docs(native): map the phase 4d modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, `profiles.bats` is green under `AGENTSYNC_NATIVE=1`, the parity fixtures pass, the files in Task 3 pass in both modes, and a `## Completion receipt` records the fresh verification. With it the `yaml_edit` family is closed; the next plan is the `template_manifest` family.

## Completion receipt

### Decisions the review took

Both as recommended, on 2026-09-15, under the maintainer's "Go".

### Global Constraints

| Constraint | Satisfied by |
|---|---|
| `profile` and `upgrade-config` write and delete only what Bash does | `src/cli/profile.rs`, `src/cli/upgrade_config.rs`; the parity fixtures compare whole trees after every call |
| No Bash change | `git diff --stat cc72778..HEAD -- lib` is empty; ShellCheck exit 0 |
| Byte-for-byte parity | `tests/native_parity.bats`: `parity: upgrade-config adds or rewrites the pin like Bash`, `parity: profile adds, lists, adopts, and removes like Bash` |
| `unsafe_code = "forbid"`, fmt and clippy clean, no new dependency; `main.rs` alone reads terminal state | `Cargo.toml` unchanged; `prompts::is_tty` and `prompts::confirm` are called from `src/main.rs` |
| Disk-touching unit tests are `#[cfg(unix)]` | `src/cli/profile.rs` test module; `src/cli/upgrade_config.rs` tests only `upgrade_text` |
| Expected values from Bash | `profile_reference.out` |
| Conventional Commits, at most 72 characters, no trailers | `bd90c8b` … the close commit |

### Fresh verification, 2026-09-15, macOS arm64, outside the agent sandbox

- `cargo test`: 209 passed (lib), 11 passed (cli), 1 passed (interrupt); 210 (lib) after `299a069`.
- `cargo clippy --all-targets -- -D warnings`: exit 0. `cargo fmt --all --check`: exit 0.
- `shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh`: exit 0.
- bats, one file at a time, `AGENTSYNC_NATIVE=0` / `=1` failures: `profiles` 0/0 (20 cases), `guard` 0/0 (18), `outputs_mode` 0/0 (10), `source_overrides` 0/0 (28), `doctor` 0/0 (36), `format_migration` 0/0 (11), `version_pin` 0/0 (13), `native_parity` 0/0 (49).
- Mutations: `Updated` → `Rewrote` in `upgrade-config` and `variants.join(",")` → `join(", ")` in `profile` each failed their fixture with that diff; reverted, rebuilt.

### Skipped, deferred, open

- **The `profile remove` confirmation prompt** had no test in the plan: every unit test passed `interactive: false`, and bats never has a terminal. `299a069` adds `a_declined_remove_prompt_cancels_like_bash` (red with the condition inverted). `scratchpad/phase4/decline_reference.sh` declined the prompt under `script` in both engines: identical `Proceed? [y/N]`, `Cancelled.`, config unchanged. The accepted-prompt branch on a terminal was not run.
- **Full-suite runs** stay off on this machine.
- **Not pushed.**

## Run log

### 2026-09-15 — Phase 4d planned
- Commits: this plan.
- Verified: `scratchpad/phase4/profile_reference.sh` ran every `profile` subcommand and `upgrade-config` in Bash (output in `profile_reference.out`).
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.

### 2026-09-15 — Tasks 0 and 1 done
- Commits: `feat(native): port upgrade-config`.
- Verified: baseline `bash=0` for all eight files; `cargo test` 206/0/11/1; fmt and clippy clean; `AGENTSYNC_NATIVE=1` doctor, format_migration, version_pin `0`; the `upgrade-config adds` fixture `ok`, and `not ok` with `Updated` mutated to `Rewrote`. The fixture ran outside the agent sandbox.
- Plan amended: none.
- Next: Task 2 Step 1.
- Blocker: none.

### 2026-09-15 — Task 2 done
- Commits: `feat(native): port profile`.
- Verified: `cargo test` 209/0/11/1; fmt and clippy clean; `AGENTSYNC_NATIVE=1` profiles, guard, outputs_mode, source_overrides `0`; the `profile adds` fixture `ok`, and `not ok` with `variants.join(",")` mutated to `join(", ")` (diff on the `tools:` list). The fixture ran outside the agent sandbox.
- Plan amended: none.
- Next: Task 3 Step 1.
- Blocker: none.

### 2026-09-15 — Task 3 done, plan closed
- Commits: `299a069 test(native): cover the declined profile remove prompt`; `docs(native): map the phase 4d modules and quirks`.
- Verified: see the completion receipt; every file `bash=0 native=0`.
- Plan amended: none; the receipt records the prompt test added outside the plan's steps.
- Next: Phase 4 plan for the `template_manifest` family (init, refresh, dedupe, migrate, adopt).
- Blocker: none.
