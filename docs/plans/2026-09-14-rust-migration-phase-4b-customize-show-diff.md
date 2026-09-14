# Rust Migration Phase 4b: Native `customize`, `show`, and `diff`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync customize`, `agentsync show`, and `agentsync diff` so the binary answers them byte for byte like `lib/helpers/customize.sh`.

**Architecture:** `payload::Source` names a payload as Bash does, a file on disk or a shipped template under the virtual `/<agentsync>/lib/templates` root, and `payload::effective_source` walks `resolve_payload_source` without a `Session`. `text::sed_indent` is `sed 's/^/    /'`. Each command gets its own module (`src/cli/customize.rs`, `src/cli/show.rs`, `src/cli/diff.rs`) and is declared native in the task that ports it. `diff` pipes the shipped template into the system `diff -u` on stdin, as `_diff_payload` runs the same tool, so hunks match on every platform. `main` passes raw arguments to all three, as it does for `enable`. The seam stays the CLI process boundary: `tests/customize.bats` under `AGENTSYNC_NATIVE=1` plus parity fixtures.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-14-rust-migration-phase-4a-enable-disable.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `customize` writes only under the tool override directory and moves only a legacy flat payload into it; `show` and `diff` write nothing.
- No binary ships to users; without a binary every command runs in Bash. No Bash change; `lib/**/*.sh` stays clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state.
- Disk-touching unit tests are `#[cfg(unix)]`. No unit test spawns `diff`; the parity fixtures cover that branch.
- Every expected value was captured from Bash on 2026-09-14 by `scratchpad/phase4/customize_reference.sh` (output in `customize_reference.out`), with the install-dir template path read as `/<agentsync>/lib/templates`.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-14 under `/decide` and the maintainer's instruction to continue: all four as recommended.

1. **Slicing.** 4a's note grouped `simplify` and `resolve` with these three. **Recommended:** 4b is `customize`, `show`, `diff` (one Bash file, `customize.sh`); 4c is `simplify` and `resolve` with `snapshot`; 4d is `profile` and `upgrade-config`. Task 5 rewrites the spec note. Alternative: keep five commands in one plan, which doubles its size.
2. **Template paths.** `show`, `diff`, and `customize` print `$DEFAULT_REPO_ROOT/lib/templates/...`. **Recommended:** print `/<agentsync>/lib/templates/...`, extending the Phase 3 accepted deviation to these commands; the parity masks already fold both spellings, and Phase 5 decides what a binary without an install directory shows. Alternative: pass the engine root from the dispatcher, a Bash change for a path that stops existing at cutover.
3. **`diff` output.** **Recommended:** spawn `diff -u --label base --label override - <override>` with the template on stdin, so the hunks are the platform tool's, as in Bash. Alternative: a Rust unified-diff implementation, whose hunks would differ from GNU and BSD `diff` on real edits.
4. **Quirks kept.** `diff <slug>` prints "No user overrides" and exits 0 when no tool has an override, whatever the slug; `show <slug> <resource>` labels an override `base` when its extension differs from the shipped template's; `diff` selects the project config before it validates the resource while `customize` and `show` validate first. **Recommended:** reproduce them and record them as known quirks 18–20.

## Module closure

```text
lib/helpers/customize.sh        1-22    resources, _validate_resource
                                46-270  cmd_customize, _customize_tool, _customize_payload
                                272-470 cmd_show, _show_payload
                                472-703 cmd_diff, _diff_payload, _diff_one_tool
lib/helpers/tool_resolver.sh    60-72   user_dir_in_project, require_project_user_dir
                                263-330 _find_base_payload, _payload_override_path, _find_new_payload_override, _payload_override_legacy_path
                                338-352 _warn_legacy_payload_path
                                379-436 resolve_payload_source
                                532-560 list_user_override_tools, is_tool_enabled
```

Reused: `project::Project`, `tool::Tool`, `catalog`, `payload::{find_new_override, legacy_override_path, override_path}`, `paths::{Paths, ENGINE_ROOT}`, `style`, `yaml_subset`, `cli::enable`'s tools-dir check (moved by Task 1).

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in customize simplify doctor source_overrides resource_resolver enable native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: this plan's commit; `192 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for every file.

---

### Task 1: Shared Pieces: the Tools-Dir Check, `payload::Source`, `text::sed_indent`

**Files:**
- Modify: `src/project.rs` (new `tools_dir_in_project`), `src/cli/mod.rs` (new `refuse_outside_tools_dir`), `src/cli/enable.rs` (use both), `src/payload.rs` (new `Source`, `base_source`, `effective_source`, `legacy_warning`; test), `src/text.rs` (new `sed_indent`; test)

**Interfaces:**
- Produces:
  - `Project::tools_dir_in_project(&self) -> bool`
  - `cli::refuse_outside_tools_dir(project: &Project, style: &Style, err: &mut dyn Write) -> Result<u8, Error>` — prints and returns `1`
  - `pub enum payload::Source { Disk(PathBuf), Shipped(&'static File<'static>) }` with `shown(&self) -> String` and `bytes(&self) -> Result<Vec<u8>, Error>`
  - `payload::base_source(tool: &Tool, resource: &str) -> Option<Source>`
  - `payload::effective_source(project: &Project, tool: &Tool, resource: &str) -> Result<(Option<Source>, Option<PathBuf>), Error>` — the second value is the legacy path `_warn_legacy_payload_path` names
  - `payload::legacy_warning(project: &Project, path: &Path) -> String`
  - `text::sed_indent(bytes: &[u8]) -> Vec<u8>`

- [x] **Step 1: Write the failing tests**

Append inside the tests module of `src/payload.rs`:

```rust
    #[cfg(unix)]
    #[test]
    fn the_effective_source_walks_override_declared_legacy_shared_then_base() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let project = Project::at(&root).unwrap();
        let cursor = Tool::load(&project, "cursor").unwrap();
        let shown = |project: &Project, tool: &Tool, resource: &str| {
            let (source, warn) = effective_source(project, tool, resource).unwrap();
            (source.map(|s| s.shown()), warn)
        };
        assert_eq!(
            shown(&project, &cursor, "hooks"),
            (Some("/<agentsync>/lib/templates/hooks/cursor.json".to_string()), None)
        );
        write(&root, ".ai/src/hooks/cursor.json", "{}\n");
        let legacy = root.join(".ai/src/hooks/cursor.json");
        assert_eq!(
            shown(&project, &cursor, "hooks"),
            (Some(legacy.to_string_lossy().into_owned()), Some(legacy.clone()))
        );
        assert_eq!(
            legacy_warning(&project, &legacy),
            "⚠  Legacy payload override layout detected: .ai/src/hooks/cursor.json\n   Move to .ai/src/tools/<tool>/<resource>.<ext> (canonical since 0.11).\n   Migrate with: agentsync migrate --legacy\n"
        );
        write(&root, ".ai/src/tools/cursor/hooks.json", "{}\n");
        assert_eq!(
            shown(&project, &cursor, "hooks"),
            (
                Some(root.join(".ai/src/tools/cursor/hooks.json").to_string_lossy().into_owned()),
                None
            )
        );
        let claude = Tool::load(&project, "claude").unwrap();
        write(&root, ".ai/src/mcp.json", "{}\n");
        assert_eq!(
            shown(&project, &claude, "mcp"),
            (Some(root.join(".ai/src/mcp.json").to_string_lossy().into_owned()), None)
        );
        assert_eq!(
            base_source(&claude, "settings").unwrap().shown(),
            "/<agentsync>/lib/templates/settings/claude.json"
        );
    }
```

If the tests module of `src/payload.rs` has no `write(root, rel, text)` helper taking a `&Path`, the existing one at its end takes `&std::path::Path`; call it with `&root`.

Append inside the tests module of `src/text.rs`:

```rust
    #[test]
    fn sed_indent_prefixes_every_line_and_adds_no_final_newline() {
        assert_eq!(sed_indent(b"a\nb"), b"    a\n    b");
        assert_eq!(sed_indent(b"a\n"), b"    a\n");
        assert_eq!(sed_indent(b"\n"), b"    \n");
        assert_eq!(sed_indent(b""), b"");
    }
```

- [x] **Step 2: Run the tests, confirm they fail**

Run: `cargo test --lib 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function` for `effective_source`, `legacy_warning`, `base_source`, and `sed_indent`.

- [x] **Step 3: Write the implementation**

`src/text.rs`, after `lines`:

```rust
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
```

`src/payload.rs`, after `override_path` (add `use std::path::Path;` and `use include_dir::File;` to the imports):

```rust
/// A payload as Bash names it: a file on disk, or a shipped template under the
/// virtual engine root.
pub enum Source {
    Disk(PathBuf),
    Shipped(&'static File<'static>),
}

impl Source {
    pub fn shown(&self) -> String {
        match self {
            Source::Disk(path) => path.to_string_lossy().into_owned(),
            Source::Shipped(file) => {
                format!("{ENGINE_ROOT}/lib/templates/{}", file.path().to_string_lossy())
            }
        }
    }

    pub fn bytes(&self) -> Result<Vec<u8>, Error> {
        match self {
            Source::Disk(path) => std::fs::read(path).map_err(|e| Error::io(path, e)),
            Source::Shipped(file) => Ok(file.contents().to_vec()),
        }
    }
}

/// `_find_base_payload`.
pub fn base_source(tool: &Tool, resource: &str) -> Option<Source> {
    tool.base_payload(resource).map(Source::Shipped)
}

/// `resolve_payload_source` for a CLI command: per-tool override, declared
/// `targets.<resource>.source`, legacy flat layout, shared `mcp.json`, base.
pub fn effective_source(
    project: &Project,
    tool: &Tool,
    resource: &str,
) -> Result<(Option<Source>, Option<PathBuf>), Error> {
    if let Some(path) = find_new_override(project, &tool.slug, resource)? {
        return Ok((Some(Source::Disk(path)), None));
    }
    let declared = tool.value(&format!("targets.{resource}.source"));
    if !declared.is_empty() {
        let abs = if declared.starts_with('/') {
            PathBuf::from(&declared)
        } else {
            project.root.join(&declared)
        };
        if abs.is_file() {
            let legacy = ["hooks", "mcp", "settings"]
                .iter()
                .any(|kind| declared.starts_with(&format!(".ai/src/{kind}/")));
            let warn = legacy.then(|| abs.clone());
            return Ok((Some(Source::Disk(abs)), warn));
        }
    }
    if let Some(legacy) = legacy_override_path(project, tool, resource).filter(|p| p.is_file()) {
        return Ok((Some(Source::Disk(legacy.clone())), Some(legacy)));
    }
    if resource == "mcp" && project.shared_mcp_path().is_file() {
        return Ok((Some(Source::Disk(project.shared_mcp_path())), None));
    }
    Ok((base_source(tool, resource), None))
}

/// `_warn_legacy_payload_path`.
pub fn legacy_warning(project: &Project, path: &Path) -> String {
    let text = path.to_string_lossy();
    let root = format!("{}/", project.root.to_string_lossy());
    let rel = text.strip_prefix(&root).unwrap_or(&text);
    format!(
        "⚠  Legacy payload override layout detected: {rel}\n   Move to .ai/src/tools/<tool>/<resource>.<ext> (canonical since 0.11).\n   Migrate with: agentsync migrate --legacy\n"
    )
}
```

`src/project.rs`, after `user_tools_dir` (add `paths::Paths` to the imports):

```rust
    /// `tool_resolver_user_dir_in_project`.
    pub fn tools_dir_in_project(&self) -> bool {
        let paths = Paths::on_disk(&self.root.to_string_lossy());
        let abs = paths::normalize(&self.tools_dir.to_string_lossy());
        paths
            .canonicalize_with_existing_ancestor(&abs)
            .is_some_and(|canonical| paths::is_within(&canonical, &paths.root_canonical))
    }
```

`src/cli/mod.rs`, after the `pub mod` lines:

```rust
use std::io::Write;

use crate::Error;
use crate::project::Project;
use crate::style::Style;

/// `tool_resolver_require_project_user_dir`: prints why and returns status 1.
pub(crate) fn refuse_outside_tools_dir(
    project: &Project,
    style: &Style,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    err.write_all(
        format!(
            "{}: source.tools resolves outside the project: {}\nAgentSync only reads that catalog; edit its tool overrides where they live.\n",
            style.red("Error"),
            project.user_tools_dir().to_string_lossy()
        )
        .as_bytes(),
    )
    .map_err(|e| Error::io("<stderr>", e))?;
    Ok(1)
}
```

In `src/cli/enable.rs`: delete `tools_dir_in_project` and `outside_tools_dir`, replace `!tools_dir_in_project(&project)` with `!project.tools_dir_in_project()` and `outside_tools_dir(&project, style, err)` with `super::refuse_outside_tools_dir(&project, style, err)`, and drop the imports that become unused (`paths::{self, Paths}` if nothing else uses them).

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/enable.bats | grep -c '^not ok'
```

Expected: `194 passed`, `0`, `11`, `1`; `0`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/project.rs src/cli/mod.rs src/cli/enable.rs src/payload.rs src/text.rs docs/plans/2026-09-14-rust-migration-phase-4b-customize-show-diff.md
git commit -m "feat(native): resolve payload sources for the customize commands"
```

---

### Task 2: Port `customize`

**Files:**
- Create: `src/cli/customize.rs`
- Modify: `src/cli/mod.rs` (`pub mod customize;`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Consumes: Task 1.
- Produces:
  - `pub const VALID_RESOURCES: [&str; 4]` and `pub(crate) fn unknown_resource(style: &Style, resource: &str, err: &mut dyn Write) -> Result<u8, Error>` in `src/cli/customize.rs`, reused by `show` and `diff`
  - `pub fn customize(args: &[String], discover: &dyn Fn() -> Result<Project, Error>, style: &Style, stdin_tty: bool, ask: &mut dyn FnMut(&str) -> String, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`

- [x] **Step 1: Parity fixture, Bash side**

Append to `tests/native_parity.bats`:

```bash
# ── customize / show / diff ──────────────────────────────────────────────────

@test "parity: customize scaffolds tools and payloads like Bash" {
    enable_tools cursor
    assert_tree_parity customize
    assert_tree_parity customize claude
    assert_tree_parity customize cursor --full
    assert_tree_parity customize claude nope
    assert_tree_parity customize nope mcp
    assert_tree_parity customize nope --full
    assert_tree_parity customize a b c
    assert_tree_parity customize --bogus
    assert_tree_parity customize --help
    assert_tree_parity customize cursor hooks
    assert_tree_parity customize cursor hooks --yes
    mkdir -p .ai/src/mcp
    printf '{"marker":"USER"}\n' > .ai/src/mcp/claude.json
    assert_tree_parity customize claude mcp
    _run_engine 0 customize claude >/dev/null
    assert_tree_parity customize claude
    printf 'tools:\n  enabled: [cursor]\nsource:\n  tools: "%s/outside"\n' "$BATS_TEST_TMPDIR" > .ai/agent_sync.yaml
    mkdir -p "$BATS_TEST_TMPDIR/outside"
    assert_tree_parity customize codex
}
```

Run: `bats --tap -f 'customize scaffolds' tests/native_parity.bats`
Expected: `ok` (both sides still run Bash).

- [x] **Step 2: Write the failing tests**

Create `src/cli/customize.rs` with only this tests module and add `pub mod customize;` to `src/cli/mod.rs`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    pub(crate) struct Run {
        pub status: u8,
        pub out: String,
        pub err: String,
    }

    pub(crate) fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(format!("{root}/.ai")).unwrap();
        std::fs::write(
            format!("{root}/.ai/agent_sync.yaml"),
            "tools:\n  enabled:\n    - cursor\n",
        )
        .unwrap();
        (dir, root)
    }

    fn call(root: &str, args: &[&str]) -> Run {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = customize(
            &args,
            &discover,
            &Style::plain(),
            false,
            &mut |_| String::new(),
            &mut out,
            &mut err,
        )
        .unwrap();
        Run {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    #[test]
    fn customize_writes_a_stub_and_refuses_hooks_without_yes() {
        let (_dir, root) = project();
        let created = call(&root, &["claude"]);
        assert_eq!(created.status, 0);
        assert_eq!(
            created.out,
            format!(
                "\nCreated empty override: {root}/.ai/src/tools/claude.yaml\n\nAdd only fields you want to change. Everything else inherits from base.\nSee overridable fields: agentsync show claude --base\n\n"
            )
        );
        assert!(
            std::fs::read_to_string(format!("{root}/.ai/src/tools/claude.yaml"))
                .unwrap()
                .starts_with("# Claude Code — custom override for AgentSync.\n#\n# Only fields")
        );
        assert_eq!(
            call(&root, &["claude"]).out,
            format!(
                "Override already exists: {root}/.ai/src/tools/claude.yaml\n\nEdit it directly, or remove it to start over.\nSee effective config: agentsync show claude\n"
            )
        );

        let hooks = call(&root, &["cursor", "hooks"]);
        assert_eq!(hooks.status, 1);
        assert_eq!(
            hooks.out,
            "\n⚠  You are about to override hooks for Cursor.\nHooks can run shell commands after sync. Review the base template\nbelow before copying it — anything you put here will run locally.\n\n  Base: /<agentsync>/lib/templates/hooks/cursor.json\n\n    {\n      \"version\": 1,\n      \"hooks\": {}\n    }\n\n"
        );
        assert_eq!(
            hooks.err,
            "Error: Refusing to scaffold hook override in non-interactive mode.\nRe-run with --yes to confirm.\n"
        );
        assert!(!std::path::Path::new(&format!("{root}/.ai/src/tools/cursor/hooks.json")).exists());

        let usage = call(&root, &[]);
        assert_eq!(usage.status, 1);
        assert_eq!(
            usage.err,
            "Error: agentsync customize <slug> [<resource>] [--full] [--yes]\n  <resource>: tool hooks mcp settings (default: tool)\n"
        );
    }

    #[test]
    fn a_legacy_payload_moves_into_the_tool_directory() {
        let (_dir, root) = project();
        std::fs::create_dir_all(format!("{root}/.ai/src/mcp")).unwrap();
        std::fs::write(format!("{root}/.ai/src/mcp/claude.json"), "{\"marker\":\"USER\"}\n").unwrap();
        let moved = call(&root, &["claude", "mcp"]);
        assert_eq!(moved.status, 0);
        assert_eq!(
            moved.out,
            format!(
                "Migrated legacy override: .ai/src/mcp/claude.json → .ai/src/tools/claude/mcp.json\nOverride already exists: {root}/.ai/src/tools/claude/mcp.json\n\nEdit it directly, or remove it to start over.\nSee effective source:  agentsync show claude mcp\nSee your vs base diff: agentsync diff claude mcp\n"
            )
        );
        assert_eq!(
            call(&root, &["claude", "nope"]).err,
            "Error: Unknown resource 'nope'.\nValid: tool hooks mcp settings\n"
        );
    }
}
```

Run: `cargo test --lib cli::customize 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function 'customize'` and unresolved `Project` and `Style`.

- [x] **Step 3: Write the implementation**

Above the tests module:

```rust
//! `agentsync customize`: `cmd_customize` of `lib/helpers/customize.sh`, which
//! scaffolds a tool override or copies a shipped payload into the override directory.

use std::io::Write;
use std::path::Path;

use crate::payload::{self, Source};
use crate::project::Project;
use crate::style::Style;
use crate::tool::Tool;
use crate::{Error, catalog, text};

pub const VALID_RESOURCES: [&str; 4] = ["tool", "hooks", "mcp", "settings"];

const USAGE: &str = "Usage: agentsync customize <slug> [<resource>] [--full] [--yes]

  Scaffold a per-tool override at .ai/src/tools/<slug>.yaml so you can change
  fields without forking the whole base template. Empty by default — write
  only the keys you want to win over base; everything else inherits.

  <resource>   Optional payload override scaffold: tool, hooks, mcp, settings.
               Default: tool (the YAML override itself).
  --full       Copy the entire base template into the override. Use when you
               want to see every available field at once; trim what you don't
               need with `agentsync simplify`.
  --yes, -y    Overwrite an existing override without prompting.

  See effective config:  agentsync show <slug>
  See user vs base diff: agentsync diff <slug>
";

const SYNOPSIS: &str = "agentsync customize <slug> [<resource>] [--full] [--yes]";

pub(crate) fn put(writer: &mut dyn Write, bytes: &[u8]) -> Result<(), Error> {
    writer.write_all(bytes).map_err(|e| Error::io("<output>", e))
}

/// `_validate_resource`, printed; the caller returns the status.
pub(crate) fn unknown_resource(
    style: &Style,
    resource: &str,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    put(
        err,
        format!(
            "{}: Unknown resource '{resource}'.\nValid: {}\n",
            style.red("Error"),
            VALID_RESOURCES.join(" ")
        )
        .as_bytes(),
    )?;
    Ok(1)
}

pub(crate) fn relative(project: &Project, path: &Path) -> String {
    let text = path.to_string_lossy();
    let root = format!("{}/", project.root.to_string_lossy());
    text.strip_prefix(&root).unwrap_or(&text).to_string()
}

pub fn customize(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    stdin_tty: bool,
    ask: &mut dyn FnMut(&str) -> String,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (mut full, mut yes) = (false, false);
    let (mut slug, mut resource) = (String::new(), String::new());
    for arg in args {
        match arg.as_str() {
            "--full" => full = true,
            "--yes" | "-y" => yes = true,
            "--help" | "-h" => {
                put(out, USAGE.as_bytes())?;
                return Ok(0);
            }
            flag if flag.starts_with('-') => {
                put(err, format!("{}: Unknown flag: {flag}\n", style.red("Error")).as_bytes())?;
                return Ok(1);
            }
            value if slug.is_empty() => slug = value.to_string(),
            value if resource.is_empty() => resource = value.to_string(),
            _ => {
                put(
                    err,
                    format!("{}: Too many arguments.\nUsage: {SYNOPSIS}\n", style.red("Error")).as_bytes(),
                )?;
                return Ok(1);
            }
        }
    }
    if slug.is_empty() {
        put(
            err,
            format!(
                "{}: {SYNOPSIS}\n  <resource>: {} (default: tool)\n",
                style.red("Error"),
                VALID_RESOURCES.join(" ")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let resource = if resource.is_empty() { "tool".to_string() } else { resource };
    if !VALID_RESOURCES.contains(&resource.as_str()) {
        return unknown_resource(style, &resource, err);
    }
    let project = discover()?;
    if !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, err);
    }
    if resource == "tool" {
        customize_tool(&project, &slug, full, style, out, err)
    } else {
        customize_payload(&project, &slug, &resource, yes, style, stdin_tty, ask, out, err)
    }
}

/// `_customize_tool`.
fn customize_tool(
    project: &Project,
    slug: &str,
    full: bool,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let base = catalog::base_tool_yaml(slug);
    let user_file = project.user_tool_file(slug);
    let shown = user_file.to_string_lossy().into_owned();
    if base.is_none() && full {
        put(
            err,
            format!(
                "{}: No base template for '{slug}' — cannot use --full.\nCreate {shown} manually for a custom tool.\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    if user_file.is_file() {
        put(
            out,
            format!(
                "{}: {shown}\n\nEdit it directly, or remove it to start over.\nSee effective config: {}\n",
                style.yellow("Override already exists"),
                style.cyan(&format!("agentsync show {slug}"))
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    let display = Tool::load(project, slug)?.display_name();
    if let Some(dir) = user_file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    if let (true, Some(base)) = (full, base) {
        std::fs::write(&user_file, base).map_err(|e| Error::io(&user_file, e))?;
        put(
            out,
            format!(
                "\n{} {shown}\n\nThis is a full copy of the base template. Every field you keep\nwins over future base updates. Remove fields you don't need to\ncustomize — those will inherit from base automatically.\n\n",
                style.green("Created full override:")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    let mut stub = format!("# {display} — custom override for AgentSync.\n");
    if base.is_some() {
        stub.push_str(&format!(
            "#\n# Only fields you write here are \"owned\" by you.\n# Everything else inherits from the base template and receives updates.\n#\n# See base fields:          agentsync show {slug} --base\n# See effective config:     agentsync show {slug}\n# See your vs base diff:    agentsync diff {slug}\n"
        ));
    } else {
        stub.push_str("#\n# This is a custom tool — no base template exists.\n# Define the full config here, then add to tools.enabled in agent_sync.yaml.\n");
    }
    stub.push('\n');
    std::fs::write(&user_file, stub).map_err(|e| Error::io(&user_file, e))?;
    put(
        out,
        format!(
            "\n{} {shown}\n\nAdd only fields you want to change. Everything else inherits from base.\nSee overridable fields: {}\n\n",
            style.green("Created empty override:"),
            style.cyan(&format!("agentsync show {slug} --base"))
        )
        .as_bytes(),
    )?;
    Ok(0)
}

/// `_customize_payload`.
#[allow(clippy::too_many_arguments)]
fn customize_payload(
    project: &Project,
    slug: &str,
    resource: &str,
    yes: bool,
    style: &Style,
    stdin_tty: bool,
    ask: &mut dyn FnMut(&str) -> String,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let tool = Tool::load(project, slug)?;
    let (Some(base), Some(user_file)) = (
        payload::base_source(&tool, resource),
        payload::override_path(project, &tool, resource),
    ) else {
        put(
            err,
            format!(
                "{}: No base {resource} template for '{slug}'.\n\nEither '{slug}' is unknown, or this tool doesn't ship a {resource} template.\nRun {} to see available tools.\n",
                style.red("Error"),
                style.cyan("agentsync list")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let shown = user_file.to_string_lossy().into_owned();
    if let Some(legacy) = payload::legacy_override_path(project, &tool, resource)
        && legacy.is_file()
        && !user_file.is_file()
    {
        if let Some(dir) = user_file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        std::fs::rename(&legacy, &user_file).map_err(|e| Error::io(&legacy, e))?;
        put(
            out,
            format!(
                "{} {} → {}\n",
                style.yellow("Migrated legacy override:"),
                relative(project, &legacy),
                relative(project, &user_file)
            )
            .as_bytes(),
        )?;
    }
    if user_file.is_file() {
        put(
            out,
            format!(
                "{}: {shown}\n\nEdit it directly, or remove it to start over.\nSee effective source:  {}\nSee your vs base diff: {}\n",
                style.yellow("Override already exists"),
                style.cyan(&format!("agentsync show {slug} {resource}")),
                style.cyan(&format!("agentsync diff {slug} {resource}"))
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    let base_bytes = base.bytes()?;
    if resource == "hooks" {
        put(
            out,
            format!(
                "\n{}\nHooks can run shell commands after sync. Review the base template\nbelow before copying it — anything you put here will run locally.\n\n{}\n\n",
                style.yellow(&format!("⚠  You are about to override hooks for {}.", tool.display_name())),
                style.dim(&format!("  Base: {}", base.shown()))
            )
            .as_bytes(),
        )?;
        put(out, &text::sed_indent(&base_bytes))?;
        put(out, b"\n")?;
        if !yes {
            if !stdin_tty {
                put(
                    err,
                    format!(
                        "{}: Refusing to scaffold hook override in non-interactive mode.\nRe-run with {} to confirm.\n",
                        style.red("Error"),
                        style.cyan("--yes")
                    )
                    .as_bytes(),
                )?;
                return Ok(1);
            }
            let reply = ask(&style.bold("Create this override? [y/N] "));
            if !matches!(reply.as_str(), "y" | "Y" | "yes" | "YES") {
                put(out, format!("{}\n", style.dim("Cancelled.")).as_bytes())?;
                return Ok(0);
            }
        }
    }
    if let Some(dir) = user_file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    std::fs::write(&user_file, &base_bytes).map_err(|e| Error::io(&user_file, e))?;
    put(
        out,
        format!(
            "\n{} {shown}\n\n{}\n\nEdit this file to customize. Remove it to fall back to the base template.\nSee diff vs base: {}\n\n",
            style.green(&format!("Created {resource} override:")),
            style.dim(&format!("  source (base): {}", base.shown())),
            style.cyan(&format!("agentsync diff {slug} {resource}"))
        )
        .as_bytes(),
    )?;
    Ok(0)
}
```

`Source` is imported for the `base` binding's type and may be unused once the code compiles; drop the import if clippy reports it.

In `src/main.rs`, extend the raw dispatch: add `"customize"` to the matched commands and, in the body, a branch

```rust
            "customize" => cli::customize::customize(
                &rest,
                &Project::discover,
                &style,
                std::io::stdin().is_terminal(),
                &mut |prompt: &str| {
                    eprint!("{prompt}");
                    let _ = std::io::stderr().flush();
                    let mut line = String::new();
                    let _ = std::io::stdin().read_line(&mut line);
                    line.trim_matches([' ', '\t', '\n']).to_string()
                },
                &mut out,
                &mut err,
            ),
```

turning the `if command == "enable"` expression into a `match command`, and adding `use std::io::IsTerminal;` to the imports. In `bin/agentsync.sh:280` append `customize` to `_NATIVE_COMMANDS`.

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/customize.bats | grep -c '^not ok'
bats --tap -f 'customize scaffolds' tests/native_parity.bats
```

Expected: `196 passed`, `0`, `11`, `1`; `0`; `ok`.

- [x] **Step 5: Prove the fixture bites, lint, commit**

Change `"Created {resource} override:"` to `"Made {resource} override:"`, rebuild, rerun the fixture: `not ok` with that diff; revert and rebuild.

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/customize.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-14-rust-migration-phase-4b-customize-show-diff.md
git commit -m "feat(native): port customize"
```

---

### Task 3: Port `show`

**Files:**
- Create: `src/cli/show.rs`
- Modify: `src/cli/mod.rs` (`pub mod show;`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Consumes: Task 1, `customize::{VALID_RESOURCES, unknown_resource, put}`.
- Produces: `pub fn show(args: &[String], discover: &dyn Fn() -> Result<Project, Error>, style: &Style, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`

- [x] **Step 1: Parity fixture, Bash side**

```bash
@test "parity: show prints effective tools and payload sources like Bash" {
    enable_tools cursor
    assert_tree_parity show
    assert_tree_parity show claude
    assert_tree_parity show claude --base
    assert_tree_parity show nope
    assert_tree_parity show nope --base
    assert_tree_parity show claude nope
    assert_tree_parity show a b c
    assert_tree_parity show claude --help
    assert_tree_parity show cursor hooks
    assert_tree_parity show cursor hooks --base
    assert_tree_parity show claude settings
    assert_tree_parity show zed hooks
    _run_engine 0 customize cursor --full >/dev/null
    printf 'targets:\n  rules:\n    dest: ".custom/rules"\n' > .ai/src/tools/claude.yaml
    mkdir -p .ai/src/hooks
    printf '{"legacy":true}\n' > .ai/src/hooks/cursor.json
    assert_tree_parity show claude
    assert_tree_parity show cursor
    assert_tree_parity show cursor hooks
    printf '{"mcpServers":{}}\n' > .ai/src/mcp.json
    assert_tree_parity show claude mcp
}
```

Run: `bats --tap -f 'show prints' tests/native_parity.bats`
Expected: `ok`.

- [x] **Step 2: Write the failing tests**

Create `src/cli/show.rs` with the tests module only; add `pub mod show;`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn call(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = show(&args, &discover, &Style::plain(), &mut out, &mut err).unwrap();
        (status, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().to_string_lossy().into_owned();
        std::fs::create_dir_all(format!("{root}/.ai/src/tools")).unwrap();
        std::fs::write(format!("{root}/.ai/agent_sync.yaml"), "tools:\n  enabled:\n    - cursor\n").unwrap();
        (dir, root)
    }

    #[test]
    fn show_marks_user_and_base_values() {
        let (_dir, root) = project();
        std::fs::write(
            format!("{root}/.ai/src/tools/cursor.yaml"),
            "targets:\n  rules:\n    dest: \".custom/rules\"\n",
        )
        .unwrap();
        let (status, out, _) = call(&root, &["cursor"]);
        assert_eq!(status, 0);
        assert!(out.starts_with(&format!(
            "\n  Cursor  [enabled]\n  override: {root}/.ai/src/tools/cursor.yaml\n  base:     /<agentsync>/lib/templates/tools/cursor.yaml\n\n    base    {:<42}  Cursor\n",
            "name"
        )));
        assert!(out.contains(&format!("    ★ user  {:<42}  .custom/rules\n", "targets.rules.dest")));
        assert!(out.ends_with("\n\n"));
        assert_eq!(
            call(&root, &["nope"]),
            (1, String::new(), "Error: Unknown tool 'nope'.\nRun agentsync list to see available tools.\n".to_string())
        );
    }

    #[test]
    fn show_labels_the_effective_payload_and_warns_about_legacy_layout() {
        let (_dir, root) = project();
        assert_eq!(
            call(&root, &["cursor", "hooks"]).1,
            "\n  Cursor — hooks  [base]\n  effective: /<agentsync>/lib/templates/hooks/cursor.json\n  base:      /<agentsync>/lib/templates/hooks/cursor.json\n\n    {\n      \"version\": 1,\n      \"hooks\": {}\n    }\n\n"
        );
        std::fs::create_dir_all(format!("{root}/.ai/src/hooks")).unwrap();
        std::fs::write(format!("{root}/.ai/src/hooks/cursor.json"), "{}\n").unwrap();
        let (status, out, err) = call(&root, &["cursor", "hooks"]);
        assert_eq!(status, 0);
        assert_eq!(
            err,
            "⚠  Legacy payload override layout detected: .ai/src/hooks/cursor.json\n   Move to .ai/src/tools/<tool>/<resource>.<ext> (canonical since 0.11).\n   Migrate with: agentsync migrate --legacy\n"
        );
        assert_eq!(
            out,
            format!(
                "\n  Cursor — hooks  [★ user override (legacy layout)]\n  effective: {root}/.ai/src/hooks/cursor.json\n  legacy:    {root}/.ai/src/hooks/cursor.json\n  base:      /<agentsync>/lib/templates/hooks/cursor.json\n\n    {{}}\n\n"
            )
        );
    }
}
```

Run: `cargo test --lib cli::show 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function 'show'`.

- [x] **Step 3: Write the implementation**

Above the tests module:

```rust
//! `agentsync show`: `cmd_show` and `_show_payload` of `lib/helpers/customize.sh`.

use std::io::Write;

use super::customize::{VALID_RESOURCES, put, unknown_resource};
use crate::paths::ENGINE_ROOT;
use crate::payload::{self, Source};
use crate::project::Project;
use crate::style::Style;
use crate::tool::Tool;
use crate::{Error, catalog, text, yaml_subset};

const USAGE: &str = "Usage: agentsync show <slug> [<resource>] [--base]

  Print effective configuration for a tool. Each line is marked
  \"base\" (inherited from the shipped template) or \"user\" (overridden in
  .ai/src/tools/<slug>.yaml).

  <resource>   Optional payload resource: tool, hooks, mcp, settings.
               Default: tool (the YAML config).
  --base       Print the base template only, ignoring user overrides.
";

const KEYS: [&str; 30] = [
    "name",
    "enabled",
    "targets.agents.dest",
    "targets.agents.source",
    "targets.rules.dest",
    "targets.rules.source",
    "targets.rules.extension",
    "targets.rules.header",
    "targets.rules.scoped_header",
    "targets.rules.append_imports",
    "targets.rules.merge_to_file",
    "targets.rules.inline_into_agents",
    "targets.rules.prepend_agents",
    "targets.skills.dest",
    "targets.skills.inline_into_agents",
    "targets.commands.dest",
    "targets.commands.format",
    "targets.commands.extension",
    "targets.commands.as_skills",
    "targets.commands.inline_into_agents",
    "targets.subagents.dest",
    "targets.subagents.format",
    "targets.subagents.extension",
    "targets.settings.source",
    "targets.settings.dest",
    "targets.mcp.source",
    "targets.mcp.dest",
    "targets.hooks.source",
    "targets.hooks.dest",
    "post_sync",
];

pub(crate) fn base_tool_shown(slug: &str) -> String {
    format!("{ENGINE_ROOT}/lib/templates/tools/{slug}.yaml")
}

pub(crate) fn read_text(path: &std::path::Path) -> Result<Option<String>, Error> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

pub fn show(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut show_base = false;
    let (mut slug, mut resource) = (String::new(), String::new());
    for arg in args {
        match arg.as_str() {
            "--base" => show_base = true,
            "--help" | "-h" => {
                put(out, USAGE.as_bytes())?;
                return Ok(0);
            }
            flag if flag.starts_with('-') => {
                put(err, format!("{}: Unknown flag: {flag}\n", style.red("Error")).as_bytes())?;
                return Ok(1);
            }
            value if slug.is_empty() => slug = value.to_string(),
            value if resource.is_empty() => resource = value.to_string(),
            _ => {
                put(err, format!("{}: Too many arguments.\n", style.red("Error")).as_bytes())?;
                return Ok(1);
            }
        }
    }
    if slug.is_empty() {
        put(
            err,
            format!(
                "{}: agentsync show <slug> [<resource>] [--base]\n  <resource>: {} (default: tool)\n",
                style.red("Error"),
                VALID_RESOURCES.join(" ")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let resource = if resource.is_empty() { "tool".to_string() } else { resource };
    if !VALID_RESOURCES.contains(&resource.as_str()) {
        return unknown_resource(style, &resource, err);
    }
    let project = discover()?;
    if resource != "tool" {
        return show_payload(&project, &slug, &resource, show_base, style, out, err);
    }

    let base = catalog::base_tool_yaml(&slug);
    let user_file = project.user_tool_file(&slug);
    if show_base {
        let Some(base) = base else {
            put(err, format!("{}: No base template for '{slug}'.\n", style.red("Error")).as_bytes())?;
            return Ok(1);
        };
        put(
            out,
            format!("\n{} {}\n\n", style.bold("  Base template"), style.dim(&base_tool_shown(&slug))).as_bytes(),
        )?;
        put(out, &text::sed_indent(base.as_bytes()))?;
        put(out, b"\n")?;
        return Ok(0);
    }
    let user_text = read_text(&user_file)?;
    if base.is_none() && user_text.is_none() {
        put(
            err,
            format!(
                "{}: Unknown tool '{slug}'.\nRun {} to see available tools.\n",
                style.red("Error"),
                style.cyan("agentsync list")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let label = if project.enabled_tools()?.contains(&slug) {
        style.green("enabled")
    } else {
        style.dim("disabled")
    };
    let tool = Tool::load(&project, &slug)?;
    let mut text = format!("\n{}  [{label}]\n", style.bold(&format!("  {}", tool.display_name())));
    if user_text.is_some() {
        text.push_str(&format!("{}\n", style.dim(&format!("  override: {}", user_file.to_string_lossy()))));
    }
    if base.is_some() {
        text.push_str(&format!("{}\n", style.dim(&format!("  base:     {}", base_tool_shown(&slug)))));
    }
    text.push('\n');
    for key in KEYS {
        let user = user_text.as_deref().map(|t| yaml_subset::value(t, key)).unwrap_or_default();
        let shipped = base.map(|t| yaml_subset::value(t, key)).unwrap_or_default();
        if !user.is_empty() {
            text.push_str(&format!("    {}  {key:<42}  {user}\n", style.yellow("★ user")));
        } else if !shipped.is_empty() {
            text.push_str(&format!("    {}  {key:<42}  {shipped}\n", style.dim("base  ")));
        }
    }
    text.push('\n');
    put(out, text.as_bytes())?;
    Ok(0)
}

/// `_show_payload`.
fn show_payload(
    project: &Project,
    slug: &str,
    resource: &str,
    show_base: bool,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let tool = Tool::load(project, slug)?;
    let base = payload::base_source(&tool, resource);
    let user_file = payload::override_path(project, &tool, resource).filter(|p| p.is_file());
    let legacy = payload::legacy_override_path(project, &tool, resource).filter(|p| p.is_file());
    let (effective, warn) = payload::effective_source(project, &tool, resource)?;
    if let Some(path) = &warn {
        put(err, payload::legacy_warning(project, path).as_bytes())?;
    }

    if show_base {
        let Some(base) = base else {
            put(
                err,
                format!("{}: No base {resource} template for '{slug}'.\n", style.red("Error")).as_bytes(),
            )?;
            return Ok(1);
        };
        put(
            out,
            format!(
                "\n{} {}\n\n",
                style.bold(&format!("  Base {resource}")),
                style.dim(&base.shown())
            )
            .as_bytes(),
        )?;
        put(out, &text::sed_indent(&base.bytes()?))?;
        put(out, b"\n")?;
        return Ok(0);
    }
    let Some(effective) = effective else {
        put(
            err,
            format!(
                "{}: No {resource} source for '{slug}' (neither override nor base).\nRun {} to see available tools.\n",
                style.red("Error"),
                style.cyan("agentsync list")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let is = |path: &Option<std::path::PathBuf>| {
        matches!((&effective, path), (Source::Disk(e), Some(p)) if e == p)
    };
    let label = if is(&user_file) {
        style.yellow("★ user override")
    } else if is(&legacy) {
        style.yellow("★ user override (legacy layout)")
    } else {
        style.dim("base")
    };
    let mut text = format!(
        "\n{}\n{}\n",
        style.bold(&format!("  {} — {resource}  [{label}]", tool.display_name())),
        style.dim(&format!("  effective: {}", effective.shown()))
    );
    if let Some(user) = &user_file {
        text.push_str(&format!("{}\n", style.dim(&format!("  override:  {}", user.to_string_lossy()))));
    }
    if let (Some(legacy), None) = (&legacy, &user_file) {
        text.push_str(&format!("{}\n", style.dim(&format!("  legacy:    {}", legacy.to_string_lossy()))));
    }
    if let Some(base) = &base {
        text.push_str(&format!("{}\n", style.dim(&format!("  base:      {}", base.shown()))));
    }
    text.push('\n');
    put(out, text.as_bytes())?;
    put(out, &text::sed_indent(&effective.bytes()?))?;
    put(out, b"\n")?;
    Ok(0)
}
```

The legacy line condition is `_show_payload`'s `[[ -f legacy ]] && [[ legacy != effective || effective != user ]] && [[ ! -f user ]]`, which reduces to "legacy exists and the override does not": an effective path equal to the override path implies the override exists.

In `src/main.rs`, add `"show"` to the raw dispatch: `"show" => cli::show::show(&rest, &Project::discover, &style, &mut out, &mut err),`. In `bin/agentsync.sh:280` append `show`.

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/customize.bats | grep -c '^not ok'
bats --tap -f 'show prints' tests/native_parity.bats
```

Expected: `198 passed`, `0`, `11`, `1`; `0`; `ok`.

- [x] **Step 5: Prove the fixture bites, lint, commit**

Change `"★ user override (legacy layout)"` to `"★ user override (legacy)"`, rebuild, rerun the fixture: `not ok` with that diff; revert and rebuild.

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/show.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-14-rust-migration-phase-4b-customize-show-diff.md
git commit -m "feat(native): port show"
```

---

### Task 4: Port `diff`

**Files:**
- Create: `src/cli/diff.rs`
- Modify: `src/cli/mod.rs` (`pub mod diff;`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Consumes: Task 1, `customize::{VALID_RESOURCES, unknown_resource, put}`, `show::{base_tool_shown, read_text}`.
- Produces: `pub fn diff(args: &[String], discover: &dyn Fn() -> Result<Project, Error>, style: &Style, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`

- [ ] **Step 1: Parity fixture, Bash side**

```bash
@test "parity: diff reports overrides and payload hunks like Bash" {
    enable_tools cursor
    assert_tree_parity diff
    assert_tree_parity diff claude
    assert_tree_parity diff claude hooks
    assert_tree_parity diff a b c
    assert_tree_parity diff --bogus
    assert_tree_parity diff claude --help
    _run_engine 0 customize cursor --full >/dev/null
    _run_engine 0 customize claude >/dev/null
    printf 'name: "My Claude"\ntargets:\n  rules:\n    dest: ".custom/rules"\n' >> .ai/src/tools/claude.yaml
    printf 'name: "Mine"\n' > .ai/src/tools/mytool.yaml
    assert_tree_parity diff
    assert_tree_parity diff claude
    assert_tree_parity diff zed
    assert_tree_parity diff claude settings
    assert_tree_parity diff cursor nope
    assert_tree_parity diff nope mcp
    _run_engine 0 customize cursor hooks --yes >/dev/null
    assert_tree_parity diff cursor hooks
    printf '{\n  "version": 2,\n  "hooks": {"afterFileEdit": []}\n}\n' > .ai/src/tools/cursor/hooks.json
    assert_tree_parity diff cursor hooks
    mkdir -p .ai/src/mcp
    printf '{"legacy":true}\n' > .ai/src/mcp/claude.json
    assert_tree_parity diff claude mcp
}
```

Run: `bats --tap -f 'diff reports' tests/native_parity.bats`
Expected: `ok`.

- [ ] **Step 2: Write the failing test**

Create `src/cli/diff.rs` with the tests module only; add `pub mod diff;`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn call(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = diff(&args, &discover, &Style::plain(), &mut out, &mut err).unwrap();
        (status, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn diff_reports_overrides_inherited_fields_and_identical_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().to_string_lossy().into_owned();
        std::fs::create_dir_all(format!("{root}/.ai/src/tools/cursor")).unwrap();
        assert_eq!(
            call(&root, &[]).1,
            "\n  No user overrides — all tools inherit fully from base.\n\n"
        );
        std::fs::write(
            format!("{root}/.ai/src/tools/cursor.yaml"),
            "name: Cursor\ntargets:\n  rules:\n    dest: \".custom/rules\"\n",
        )
        .unwrap();
        let (status, out, _) = call(&root, &["cursor"]);
        assert_eq!(status, 0);
        assert!(out.starts_with(&format!(
            "\n  cursor\n    user: {root}/.ai/src/tools/cursor.yaml\n    base: /<agentsync>/lib/templates/tools/cursor.yaml\n\n    Your overrides (win over base):\n      targets.rules.dest\n        you:  .custom/rules\n        base: .cursor/rules\n\n    Inherited from base (remove override to keep inheriting):\n"
        )));
        assert_eq!(
            call(&root, &["claude"]),
            (1, String::new(), "Error: No override found for 'claude'.\n".to_string())
        );

        std::fs::write(
            format!("{root}/.ai/src/tools/cursor/hooks.json"),
            crate::catalog::base_payload("hooks", "cursor").unwrap().contents(),
        )
        .unwrap();
        assert_eq!(
            call(&root, &["cursor", "hooks"]).1,
            format!(
                "\n  Cursor — hooks diff\n    override: {root}/.ai/src/tools/cursor/hooks.json\n    base:     /<agentsync>/lib/templates/hooks/cursor.json\n\n    Identical — override is a byte-for-byte copy of base.\n    Tip: agentsync simplify can remove redundant overrides.\n"
            )
        );
        assert_eq!(
            call(&root, &["claude", "settings"]).1,
            "\n  No override for Claude Code settings — inheriting fully from base.\n  base: /<agentsync>/lib/templates/settings/claude.json\n\n"
        );
    }
}
```

Run: `cargo test --lib cli::diff 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function 'diff'`.

- [ ] **Step 3: Write the implementation**

Above the tests module:

```rust
//! `agentsync diff`: `cmd_diff`, `_diff_payload`, and `_diff_one_tool` of
//! `lib/helpers/customize.sh`. Payload hunks come from the system `diff -u`.

use std::io::Write;
use std::process::{Command, Stdio};

use super::customize::{VALID_RESOURCES, put, unknown_resource};
use super::show::{base_tool_shown, read_text};
use crate::payload;
use crate::project::Project;
use crate::style::Style;
use crate::tool::Tool;
use crate::{Error, catalog, text, yaml_subset};

const USAGE: &str = "Usage: agentsync diff [<slug>] [<resource>]

  Show fields where your user override diverges from the shipped base
  template. With no <slug>, walks every customized tool.

  <resource>   Optional payload resource: tool, hooks, mcp, settings.
               Default: tool (the YAML config).
";

const KEYS: [&str; 26] = [
    "name",
    "enabled",
    "targets.agents.dest",
    "targets.rules.dest",
    "targets.rules.extension",
    "targets.rules.header",
    "targets.rules.scoped_header",
    "targets.rules.append_imports",
    "targets.rules.merge_to_file",
    "targets.rules.inline_into_agents",
    "targets.rules.prepend_agents",
    "targets.skills.dest",
    "targets.skills.inline_into_agents",
    "targets.commands.dest",
    "targets.commands.format",
    "targets.commands.as_skills",
    "targets.commands.inline_into_agents",
    "targets.subagents.dest",
    "targets.subagents.format",
    "targets.settings.source",
    "targets.settings.dest",
    "targets.mcp.source",
    "targets.mcp.dest",
    "targets.hooks.source",
    "targets.hooks.dest",
    "post_sync",
];

pub fn diff(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (mut slug, mut resource) = (String::new(), String::new());
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => {
                put(out, USAGE.as_bytes())?;
                return Ok(0);
            }
            flag if flag.starts_with('-') => {
                put(err, format!("{}: Unknown flag: {flag}\n", style.red("Error")).as_bytes())?;
                return Ok(1);
            }
            value if slug.is_empty() => slug = value.to_string(),
            value if resource.is_empty() => resource = value.to_string(),
            _ => {
                put(err, format!("{}: Too many arguments.\n", style.red("Error")).as_bytes())?;
                return Ok(1);
            }
        }
    }
    let project = discover()?;
    let resource = if resource.is_empty() { "tool".to_string() } else { resource };
    if !VALID_RESOURCES.contains(&resource.as_str()) {
        return unknown_resource(style, &resource, err);
    }
    if resource != "tool" {
        if slug.is_empty() {
            put(err, format!("{}: agentsync diff <slug> <resource>\n", style.red("Error")).as_bytes())?;
            return Ok(1);
        }
        return diff_payload(&project, &slug, &resource, style, out, err);
    }

    let overrides = project.user_override_tools()?;
    if overrides.is_empty() {
        put(
            out,
            format!("\n  {}\n\n", style.dim("No user overrides — all tools inherit fully from base.")).as_bytes(),
        )?;
        return Ok(0);
    }
    let mut any = false;
    for tool in overrides.iter().filter(|t| slug.is_empty() || **t == slug) {
        diff_one_tool(&project, tool, style, out)?;
        any = true;
    }
    if !any {
        put(err, format!("{}: No override found for '{slug}'.\n", style.red("Error")).as_bytes())?;
        return Ok(1);
    }
    Ok(0)
}

/// `_diff_one_tool`.
fn diff_one_tool(project: &Project, slug: &str, style: &Style, out: &mut dyn Write) -> Result<(), Error> {
    let base = catalog::base_tool_yaml(slug);
    let user_file = project.user_tool_file(slug);
    let user_text = read_text(&user_file)?;
    let base_line = if base.is_some() {
        format!("    base: {}", base_tool_shown(slug))
    } else {
        "    base: (none — custom tool)".to_string()
    };
    let mut text = format!(
        "\n{}\n{}\n{}\n\n",
        style.bold(&format!("  {slug}")),
        style.dim(&format!("    user: {}", user_file.to_string_lossy())),
        style.dim(&base_line)
    );
    let values = |key: &str| {
        (
            user_text.as_deref().map(|t| yaml_subset::value(t, key)).unwrap_or_default(),
            base.map(|t| yaml_subset::value(t, key)).unwrap_or_default(),
        )
    };
    let mut printed_override = false;
    for key in KEYS {
        let (user, shipped) = values(key);
        if user.is_empty() || user == shipped {
            continue;
        }
        if !printed_override {
            text.push_str(&format!("    {}\n", style.yellow("Your overrides (win over base):")));
            printed_override = true;
        }
        text.push_str(&format!("      {key}\n        you:  {user}\n"));
        if shipped.is_empty() {
            text.push_str(&format!("        base: {}\n", style.dim("(not in base)")));
        } else {
            text.push_str(&format!("        base: {shipped}\n"));
        }
    }
    if printed_override {
        text.push('\n');
    }
    let mut printed_inherit = false;
    for key in KEYS {
        let (user, shipped) = values(key);
        if !user.is_empty() || shipped.is_empty() {
            continue;
        }
        if !printed_inherit {
            text.push_str(&format!(
                "    {}\n",
                style.dim("Inherited from base (remove override to keep inheriting):")
            ));
            printed_inherit = true;
        }
        text.push_str(&format!("      {key:<42}  {shipped}\n"));
    }
    if printed_inherit {
        text.push('\n');
    }
    if !printed_override && !printed_inherit {
        text.push_str(&format!("{}\n", style.dim("    No diverging fields.")));
    }
    put(out, text.as_bytes())
}

/// `_diff_payload`.
fn diff_payload(
    project: &Project,
    slug: &str,
    resource: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let tool = Tool::load(project, slug)?;
    let base = payload::base_source(&tool, resource);
    let user_file = match payload::find_new_override(project, slug, resource)? {
        Some(path) => Some(path),
        None => payload::legacy_override_path(project, &tool, resource).filter(|p| p.is_file()),
    };
    let display = tool.display_name();
    let (base, user_file) = match (base, user_file) {
        (None, None) => {
            put(
                err,
                format!("{}: No {resource} source for '{slug}' (no base, no override).\n", style.red("Error")).as_bytes(),
            )?;
            return Ok(1);
        }
        (Some(base), None) => {
            put(
                out,
                format!(
                    "\n{}\n{}\n\n",
                    style.dim(&format!("  No override for {display} {resource} — inheriting fully from base.")),
                    style.dim(&format!("  base: {}", base.shown()))
                )
                .as_bytes(),
            )?;
            return Ok(0);
        }
        (None, Some(user)) => {
            put(
                out,
                format!(
                    "\n{}\n{}\n\n",
                    style.yellow(&format!("  Custom {resource} override (no base to diff against):")),
                    style.dim(&format!("  override: {}", user.to_string_lossy()))
                )
                .as_bytes(),
            )?;
            return Ok(0);
        }
        (Some(base), Some(user)) => (base, user),
    };
    put(
        out,
        format!(
            "\n{}\n{}\n{}\n\n",
            style.bold(&format!("  {display} — {resource} diff")),
            style.dim(&format!("    override: {}", user_file.to_string_lossy())),
            style.dim(&format!("    base:     {}", base.shown()))
        )
        .as_bytes(),
    )?;
    let base_bytes = base.bytes()?;
    if std::fs::read(&user_file).is_ok_and(|user| user == base_bytes) {
        put(
            out,
            format!(
                "{}\n{}\n",
                style.dim("    Identical — override is a byte-for-byte copy of base."),
                style.dim(&format!("    Tip: {} can remove redundant overrides.", style.cyan("agentsync simplify")))
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    put(out, &text::sed_indent(&unified_diff(&base_bytes, &user_file)))?;
    put(out, b"\n")?;
    Ok(0)
}

/// `diff -u --label base --label override <base> <override> 2>/dev/null`, with
/// the shipped template on stdin; empty when `diff` cannot run.
fn unified_diff(base: &[u8], override_file: &std::path::Path) -> Vec<u8> {
    let child = Command::new("diff")
        .args(["-u", "--label", "base", "--label", "override", "-"])
        .arg(override_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return Vec::new();
    };
    let feed = child.stdin.take().map(|mut stdin| {
        let bytes = base.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        })
    });
    let output = child.wait_with_output().map(|o| o.stdout).unwrap_or_default();
    if let Some(handle) = feed {
        let _ = handle.join();
    }
    output
}
```

In `src/main.rs` add `"diff" => cli::diff::diff(&rest, &Project::discover, &style, &mut out, &mut err),` to the raw dispatch; in `bin/agentsync.sh:280` append `diff`.

- [ ] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/customize.bats | grep -c '^not ok'
bats --tap -f 'diff reports' tests/native_parity.bats
```

Expected: `199 passed`, `0`, `11`, `1`; `0`; `ok`.

- [ ] **Step 5: Prove the fixture bites, lint, commit**

Change `"--label", "override"` to `"--label", "yours"`, rebuild, rerun the fixture: `not ok` with a `+++` diff line; revert and rebuild.

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/diff.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-14-rust-migration-phase-4b-customize-show-diff.md
git commit -m "feat(native): port diff"
```

---

### Task 5: Every File That Runs These Commands, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [ ] **Step 1: Spec**

Replace the Phase 4 slice note written by 4a with:

```markdown
  Planned in four slices: 4a `enable` and `disable` with `yaml_edit` and
  `edit_paths`; 4b `customize`, `show`, and `diff`; 4c `simplify` and
  `resolve` with `snapshot`; 4d `profile` and `upgrade-config`.
```

Append to "Known quirks":

```markdown
18. `diff <slug>` prints "No user overrides" and exits 0 when no tool has an
    override, whatever the slug.
19. `show <slug> <resource>` labels an override `base` when its extension
    differs from the shipped template's.
20. `diff` selects the project config before it validates the resource;
    `customize` and `show` validate first.
```

Append to "Accepted deviations":

```markdown
- Phase 4b: `customize`, `show`, and `diff` name shipped templates as
  `/<agentsync>/lib/templates/...` where Bash printed the install directory.
```

- [ ] **Step 2: Module map and outputs**

Set the `lib/helpers/customize.sh` row to `→ src/cli/{customize,show,diff}.rs   Phase 4b, ported; diff -u spawned for payload hunks`. Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [ ] **Step 3: Verify**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in customize simplify doctor source_overrides resource_resolver enable init native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `199 passed`, `0`, `11`, `1`; lint exit 0; every line `bash=0 native=0`.

- [ ] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-14-rust-migration-phase-4b-customize-show-diff.md
git commit -m "docs(native): map the phase 4b modules, quirks, and deviation"
```

---

## Completion

The plan is closed when every box is ticked, `customize.bats` is green under `AGENTSYNC_NATIVE=1`, the parity fixtures pass, the files in Task 5 pass in both modes, and a `## Completion receipt` records the fresh verification. The next plan is Phase 4c.

## Run log

### 2026-09-14 — Phase 4b planned
- Commits: this plan.
- Verified: `scratchpad/phase4/customize_reference.sh` ran every command above in Bash (output in `customize_reference.out`); `printf 'a\nb' | sed 's/^/    /'` adds no final newline on macOS; `diff -u --label base --label override - <file>` reads the template from stdin outside the agent sandbox.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.

### 2026-09-14 — Tasks 0 and 1 done
- Commits: "feat(native): resolve payload sources for the customize commands".
- Verified: Task 0 at `bda3a9b`: `customize`, `simplify`, `doctor`, `source_overrides`, `resource_resolver`, `enable`, `native_parity` all `bash=0`. Task 1: `cargo test` 194 lib, 11 cli, 1 interrupt; fmt and clippy exit 0; release build; `AGENTSYNC_NATIVE=1 bats tests/enable.bats` 15 `ok`, 0 `not ok`.
- Plan amended: none.
- Next: Task 2 Step 1 (its module, tests, and `main` dispatch are written and pass `cargo test` at 196).
- Blocker: none.

### 2026-09-14 — Task 2 done
- Commits: "feat(native): port customize".
- Verified: `cargo test` 196 lib; fmt and clippy exit 0; the fixture `ok` on the Bash side and again with `customize` native; `AGENTSYNC_NATIVE=1 bats tests/customize.bats` 13 `ok`, 0 `not ok`. Mutation: `Made {resource} override:` failed the fixture with that diff; reverted, rebuilt.
- Plan amended: the unused `Source` import was dropped, as Step 3 anticipated.
- Next: Task 3 Step 1.
- Blocker: none.

### 2026-09-14 — Task 3 done
- Commits: "feat(native): port show".
- Verified: `cargo test` 198 lib; fmt and clippy exit 0; the fixture `ok` on the Bash side and with `show` native; `AGENTSYNC_NATIVE=1` `customize.bats` 13 `ok`, `source_overrides.bats` 0 `not ok`. Mutation: `★ user override (legacy)` failed the fixture with that diff; reverted, rebuilt.
- Plan amended: none.
- Next: Task 4 Step 1.
- Blocker: none.
