# Rust Migration Phase 4c: Native `simplify` and `resolve`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync simplify` and `agentsync resolve` so the binary answers them byte for byte like `lib/helpers/simplify.sh` and `lib/helpers/resolve_cmd.sh`.

**Architecture:** `yaml_edit::remove_key` mirrors `yaml_remove_key`; `src/snapshot.rs` holds the two readers `resolve` needs from `lib/helpers/snapshot.sh`, `read_pending_pairs` and `clear_pending` (saving and diffing the catalog stay with `update` in Phase 5). `src/cli/simplify.rs` and `src/cli/resolve.rs` write progressively to the writers `main` hands them; their prompts print on stdout and read the terminal device through an `ask` closure, as the Bash prompts do, so unit tests drive the interactive branches. `main` passes raw arguments, because `cmd_resolve` takes its first argument whatever it is. The seam stays the CLI process boundary: `tests/simplify.bats` under `AGENTSYNC_NATIVE=1` plus parity fixtures for both commands.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-14-rust-migration-phase-4b-customize-show-diff.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `simplify --apply` removes only redundant keys, byte-identical payload copies, their emptied directories, and emptied overrides it is allowed to delete; `resolve` removes only adopted keys and the pending-resolutions file; a dry run writes nothing.
- No binary ships to users; without a binary every command runs in Bash. No Bash change; `lib/**/*.sh` stays clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state.
- Disk-touching unit tests are `#[cfg(unix)]`.
- Every expected value was captured from Bash on 2026-09-15 by `scratchpad/phase4/simplify_reference.sh` (output in `simplify_reference.out`).
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-15 under `/decide`: all three as recommended.

1. **Interactive prompts.** Both commands print the question on stdout and read `/dev/tty`. **Recommended:** an `ask(prompt) -> String` closure that `main` backs with stdout and the terminal device (a new `prompts::read_terminal`), and that unit tests back with scripted answers; bats covers the non-terminal branches. Alternative: leave the interactive branches untested, which ports the most destructive paths blind.
2. **Snapshot scope.** **Recommended:** port only `snapshot_read_pending_pairs` and `snapshot_clear_pending` now; `snapshot_save`, `snapshot_diff`, `snapshot_find_conflicts`, and `snapshot_write_pending_resolutions` serve `update` and move with it in Phase 5. Alternative: the whole module now, with no native caller for four functions.
3. **Quirks kept.** Record as known quirks 21–25: `simplify`'s payload pass scans `.ai/src/tools` even when `source.tools` moves the override directory; `resolve` without a terminal ignores its tool filter and exits 0; `yaml_remove_key` drops blank lines directly after the removed block; `resolve` in a project without overrides deletes the pending-resolutions file, terminal or not; `simplify --apply` without a terminal deletes byte-identical payload copies but keeps an emptied override file. **Recommended:** reproduce them.

## Module closure

```text
lib/helpers/simplify.sh         14-46   _simplify_keys
                                72-104  usage, _simplify_file_has_content
                                106-165 cmd_simplify
                                175-315 _simplify_payload_overrides
                                317-424 _simplify_one_tool
lib/helpers/resolve_cmd.sh      32-62   _resolve_keys (the diff keys)
                                64-145  cmd_resolve, _resolve_is_pending
                                147-220 _resolve_one_tool
lib/helpers/snapshot.sh         232-289 snapshot_read_pending_pairs, snapshot_clear_pending
lib/helpers/yaml_edit.sh        137-180 yaml_remove_key
```

Reused: `yaml_edit::find_key_line`, `project::Project`, `tool::Tool`, `catalog`, `cli::refuse_outside_tools_dir`, `cli::customize::{put, relative}`, `cli::diff::KEYS` (made `pub(crate)`), `prompts`, `style`, `yaml_subset`.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in simplify update_snapshot customize native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: this plan's commit; `199 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for every file.

---

### Task 1: `yaml_edit::remove_key` and `snapshot` Pending Pairs

**Files:**
- Modify: `src/yaml_edit.rs` (new `remove_key_text`, `remove_key`; test)
- Create: `src/snapshot.rs`
- Modify: `src/lib.rs` (`pub mod snapshot;` after `pub mod session;`), `src/prompts.rs` (new `read_terminal`)

**Interfaces:**
- Produces:
  - `yaml_edit::remove_key_text(text: &str, key_path: &str) -> Option<String>` — `None` when the key is absent
  - `yaml_edit::remove_key(file: &Path, key_path: &str) -> Result<(), Error>`
  - `snapshot::read_pending_pairs(root: &Path) -> Vec<(String, String)>`
  - `snapshot::clear_pending(root: &Path)`
  - `prompts::read_terminal() -> String` — one line from the terminal device, newline stripped, empty on failure

- [x] **Step 1: Write the failing tests**

Append inside the tests module of `src/yaml_edit.rs`:

```rust
    #[test]
    fn remove_key_drops_the_block_and_the_blank_lines_right_after_it() {
        let cases: [(&str, &str, &str); 4] = [
            (
                "name: \"X\"\nenabled: true\n\ntargets:\n  rules:\n    dest: \".r\"\n    # note\n\n    extension: \".md\"\n  skills:\n    dest: \".s\"\n",
                "targets.rules.dest",
                "name: \"X\"\nenabled: true\n\ntargets:\n  rules:\n    # note\n\n    extension: \".md\"\n  skills:\n    dest: \".s\"\n",
            ),
            (
                "name: \"X\"\ntargets:\n  rules:\n    dest: \".r\"\n\n  # c\n  skills:\n    dest: \".s\"\n",
                "targets.rules",
                "name: \"X\"\ntargets:\n  # c\n  skills:\n    dest: \".s\"\n",
            ),
            ("name: \"X\"\n\nenabled: true", "name", "enabled: true\n"),
            ("post_sync:\n  - a\n  - b\n# tail\nx: 1", "post_sync", "# tail\nx: 1\n"),
        ];
        for (text, key, expected) in cases {
            assert_eq!(remove_key_text(text, key).as_deref(), Some(expected), "{key}");
        }
        assert_eq!(remove_key_text("a: 1\n", "missing.key"), None);
    }
```

Create `src/snapshot.rs` with only its tests module and register it:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn pending_pairs_are_read_from_the_conflicts_list_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        assert!(read_pending_pairs(dir.path()).is_empty());
        std::fs::write(
            dir.path().join(".ai/.pending-resolutions.yaml"),
            "# c\nschema: 1\nconflicts:\n  - tool: \"cursor\"\n    field: \"targets.rules.dest\"\n    base_before: \"a\"\n  - tool: claude\n    field: name\n\nafter: x\n  - tool: \"zed\"\n    field: \"name\"\n",
        )
        .unwrap();
        assert_eq!(
            read_pending_pairs(dir.path()),
            [
                ("cursor".to_string(), "targets.rules.dest".to_string()),
                ("claude".to_string(), "name".to_string()),
            ]
        );
        clear_pending(dir.path());
        assert!(!dir.path().join(".ai/.pending-resolutions.yaml").exists());
        clear_pending(dir.path());
    }
}
```

- [x] **Step 2: Run the tests, confirm they fail**

Run: `cargo test --lib 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function` for `remove_key_text`, `read_pending_pairs`, and `clear_pending`.

- [x] **Step 3: Write the implementation**

`src/yaml_edit.rs`, after `list_remove_text`:

```rust
/// `yaml_remove_key` on text: the key line, then every blank line and every
/// line indented deeper than the key until the first one that is not.
pub fn remove_key_text(text: &str, key_path: &str) -> Option<String> {
    let (key_lineno, key_indent) = find_key_line(text, key_path)?;
    let mut out = String::new();
    let mut skipping = false;
    for (index, line) in lines(text).into_iter().enumerate() {
        if index + 1 == key_lineno {
            skipping = true;
            continue;
        }
        if skipping {
            let (indent, stripped) = split_indent(line);
            if stripped.is_empty() || indent > key_indent {
                continue;
            }
            skipping = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}
```

and after `list_remove`:

```rust
/// `yaml_remove_key`.
pub fn remove_key(file: &Path, key_path: &str) -> Result<(), Error> {
    let Some(text) = read_existing(file)? else {
        return Ok(());
    };
    match remove_key_text(&text, key_path) {
        Some(updated) => staging::write_beside(file, updated.as_bytes()),
        None => Ok(()),
    }
}
```

Above the tests module in `src/snapshot.rs`:

```rust
//! The pending-resolutions readers of `lib/helpers/snapshot.sh` that `resolve`
//! uses. Saving and diffing the catalog belong to `update`.

use std::path::Path;

const PENDING: &str = ".ai/.pending-resolutions.yaml";

fn after_label<'a>(stripped: &'a str, label: &str) -> String {
    let value = stripped[label.len()..].trim_start_matches(|c: char| c.is_ascii_whitespace());
    let value = value.strip_suffix('"').unwrap_or(value);
    value.strip_prefix('"').unwrap_or(value).to_string()
}

/// `snapshot_read_pending_pairs`: `(tool, field)` per complete conflict.
pub fn read_pending_pairs(root: &Path) -> Vec<(String, String)> {
    let Ok(bytes) = std::fs::read(root.join(PENDING)) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut pairs = Vec::new();
    let (mut in_conflicts, mut tool, mut field) = (false, String::new(), String::new());
    for line in lines {
        let stripped = line.trim_start_matches(|c: char| c.is_ascii_whitespace());
        if stripped.starts_with("conflicts:") {
            in_conflicts = true;
            continue;
        }
        if !in_conflicts {
            continue;
        }
        if !stripped.is_empty() && line == stripped && !stripped.starts_with('-') {
            break;
        }
        if stripped.starts_with("- tool:") {
            if !tool.is_empty() && !field.is_empty() {
                pairs.push((tool.clone(), field.clone()));
            }
            tool = after_label(stripped, "- tool:");
            field.clear();
        } else if stripped.starts_with("field:") {
            field = after_label(stripped, "field:");
        }
    }
    if !tool.is_empty() && !field.is_empty() {
        pairs.push((tool, field));
    }
    pairs
}

/// `snapshot_clear_pending`.
pub fn clear_pending(root: &Path) {
    let _ = std::fs::remove_file(root.join(PENDING));
}
```

`src/prompts.rs`, after `confirm`:

```rust
/// A line typed on the terminal device, without its newline; empty when none
/// can be read, as `read -r answer < /dev/tty || answer=""` leaves it.
pub fn read_terminal() -> String {
    read_terminal_line()
        .map(|line| line.strip_suffix('\n').unwrap_or(&line).to_string())
        .unwrap_or_default()
}
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test 2>&1 | grep 'test result' | head -4`
Expected: `201 passed`, `0`, `11`, `1`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/yaml_edit.rs src/snapshot.rs src/lib.rs src/prompts.rs docs/plans/2026-09-15-rust-migration-phase-4c-simplify-resolve.md
git commit -m "feat(native): port yaml_remove_key and the pending-resolution reader"
```

---

### Task 2: Port `simplify`

**Files:**
- Create: `src/cli/simplify.rs`
- Modify: `src/cli/mod.rs` (`pub mod simplify;`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Consumes: Task 1, `cli::customize::{put, relative}`, `cli::refuse_outside_tools_dir`.
- Produces: `pub fn simplify(args: &[String], discover: &dyn Fn() -> Result<Project, Error>, style: &Style, interactive: bool, ask: &mut dyn FnMut(&str, &mut dyn Write) -> String, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>` — `ask` prints the prompt to the writer it is handed and returns the answer

- [ ] **Step 1: Parity fixture, Bash side**

Append to `tests/native_parity.bats`:

```bash
# ── simplify / resolve ───────────────────────────────────────────────────────

@test "parity: simplify previews and applies like Bash" {
    enable_tools cursor
    assert_tree_parity simplify
    assert_tree_parity simplify --whatever
    assert_tree_parity simplify a b
    assert_tree_parity simplify --help
    _run_engine 0 customize cursor --full >/dev/null
    _run_engine 0 customize claude --full >/dev/null
    printf 'name: "Cursor"\nenabled: true\n\ntargets:\n  rules:\n    dest: ".cursor/rules"\n    extension: ".mdcustom"\n  custom:\n    x: 1\n' > .ai/src/tools/cursor.yaml
    printf 'name: "Mine"\ntargets:\n  rules:\n    dest: ".mine"\n' > .ai/src/tools/mytool.yaml
    assert_tree_parity simplify nope
    assert_tree_parity simplify
    assert_tree_parity simplify --apply
    assert_tree_parity simplify claude --apply -y
    mkdir -p .ai/src/tools/cursor .ai/src/hooks
    cp "$REPO_ROOT/lib/templates/hooks/cursor.json" .ai/src/tools/cursor/hooks.json
    printf '{"edited":true}\n' > .ai/src/tools/cursor/mcp.json
    cp "$REPO_ROOT/lib/templates/hooks/cursor.json" .ai/src/hooks/cursor.json
    assert_tree_parity simplify cursor
    assert_tree_parity simplify cursor --apply -y
    assert_tree_parity simplify --apply
    rm .ai/src/tools/cursor/mcp.json
    assert_tree_parity simplify --apply
}
```

Run: `bats --tap -f 'simplify previews' tests/native_parity.bats`
Expected: `ok`.

- [ ] **Step 2: Write the failing tests**

Create `src/cli/simplify.rs` with the tests module only; add `pub mod simplify;`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().to_string_lossy().into_owned();
        std::fs::create_dir_all(format!("{root}/.ai/src/tools")).unwrap();
        std::fs::write(format!("{root}/.ai/agent_sync.yaml"), "tools:\n  enabled:\n    - cursor\n").unwrap();
        (dir, root)
    }

    fn call(root: &str, args: &[&str], interactive: bool, answer: &str) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = simplify(
            &args,
            &discover,
            &Style::plain(),
            interactive,
            &mut |prompt, out| {
                let _ = out.write_all(format!("  {prompt} ").as_bytes());
                answer.to_string()
            },
            &mut out,
            &mut err,
        )
        .unwrap();
        (status, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn a_dry_run_lists_redundant_and_kept_fields_like_bash() {
        let (_dir, root) = project();
        assert_eq!(
            call(&root, &[], false, "").1,
            "\n  No user overrides — nothing to simplify.\n\n"
        );
        let file = format!("{root}/.ai/src/tools/cursor.yaml");
        std::fs::write(
            &file,
            "name: \"Cursor\"\nenabled: true\n\ntargets:\n  rules:\n    dest: \".cursor/rules\"\n    extension: \".mdcustom\"\n  custom:\n    x: 1\n",
        )
        .unwrap();
        let (status, out, _) = call(&root, &[], false, "");
        assert_eq!(status, 0);
        assert_eq!(
            out,
            format!(
                "\n  Cursor\n  override: .ai/src/tools/cursor.yaml\n  Redundant (match base):\n    - {:<42}  Cursor\n    - {:<42}  .cursor/rules\n\n  Kept (diverge from base):\n    = {:<42}  true\n    = {:<42}  .mdcustom\n\n  → would remove 2 field(s).\n\n\nDry run — pass --apply to persist.\n\n",
                "name", "targets.rules.dest", "enabled", "targets.rules.extension"
            )
        );
        assert_eq!(
            call(&root, &["nope"], false, "").2,
            "Error: No override found for 'nope'.\n"
        );

        let (_, applied, _) = call(&root, &["--apply"], false, "");
        assert!(applied.contains("  Removed 2 field(s).\n\n\nDone.\n  Run agentsync sync to verify outputs are unchanged.\n\n"));
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "enabled: true\n\ntargets:\n  rules:\n    extension: \".mdcustom\"\n  custom:\n    x: 1\n"
        );
    }

    #[test]
    fn an_emptied_override_is_kept_off_a_terminal_and_deleted_on_yes() {
        let (_dir, root) = project();
        let file = format!("{root}/.ai/src/tools/cursor.yaml");
        std::fs::write(&file, "name: \"Cursor\"\n").unwrap();
        let (_, kept, _) = call(&root, &["--apply"], false, "");
        assert!(kept.ends_with("  Removed 1 field(s).\n  Kept empty file — remove manually if desired.\n\n\nDone.\n  Run agentsync sync to verify outputs are unchanged.\n\n"));
        assert!(std::path::Path::new(&file).is_file());

        std::fs::write(&file, "name: \"Cursor\"\n").unwrap();
        let (_, deleted, _) = call(&root, &["--apply"], true, "y");
        assert!(deleted.contains("  Removed 1 field(s).\n  Delete empty override file? [y/N]   Deleted .ai/src/tools/cursor.yaml\n"));
        assert!(!std::path::Path::new(&file).exists());

        std::fs::create_dir_all(format!("{root}/.ai/src/tools/cursor")).unwrap();
        std::fs::write(
            format!("{root}/.ai/src/tools/cursor/hooks.json"),
            crate::catalog::base_payload("hooks", "cursor").unwrap().contents(),
        )
        .unwrap();
        let (_, payload, _) = call(&root, &["--apply"], true, "n");
        assert!(payload.contains("  Delete .ai/src/tools/cursor/hooks.json? [y/N]   Kept .ai/src/tools/cursor/hooks.json\n\n  Removed 0, kept 1.\n"));
        let (_, gone, _) = call(&root, &["--apply", "-y"], false, "");
        assert!(gone.contains("  Deleted .ai/src/tools/cursor/hooks.json\n\n  Removed 1, kept 0.\n"));
        assert!(!std::path::Path::new(&format!("{root}/.ai/src/tools/cursor")).exists());
    }
}
```

Run: `cargo test --lib cli::simplify 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function 'simplify'` and unresolved `Project`, `Style`, `Write`.

- [ ] **Step 3: Write the implementation**

Above the tests module:

```rust
//! `agentsync simplify`: `cmd_simplify` of `lib/helpers/simplify.sh`, which
//! drops override fields equal to the base and byte-identical payload copies.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::customize::{put, relative};
use crate::project::Project;
use crate::style::Style;
use crate::tool::Tool;
use crate::{Error, catalog, yaml_edit, yaml_subset};

const KEYS: [&str; 31] = [
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
    "targets.skills.source",
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

type Ask<'a> = &'a mut dyn FnMut(&str, &mut dyn Write) -> String;

fn yes(answer: &str) -> bool {
    matches!(answer, "y" | "Y" | "yes" | "Yes")
}

fn read_text(path: &Path) -> String {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// `_simplify_file_has_content`.
fn has_content(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r');
    read_text(path).split('\n').any(|line| {
        let stripped = line.trim_start_matches(is_space);
        if line.is_empty() || stripped.starts_with('#') {
            return false;
        }
        let key_end = stripped
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(stripped.len());
        let assignment = key_end > 0
            && stripped[key_end..]
                .strip_prefix(':')
                .is_some_and(|rest| rest.starts_with(is_space) && rest.chars().count() >= 2);
        let item = stripped
            .strip_prefix('-')
            .is_some_and(|rest| rest.starts_with(is_space));
        assignment || item
    })
}

pub fn simplify(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    interactive: bool,
    ask: Ask,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (mut apply, mut auto_yes, mut filter) = (false, false, String::new());
    for arg in args {
        match arg.as_str() {
            "--apply" => apply = true,
            "-y" | "--yes" => auto_yes = true,
            "-h" | "--help" => {
                put(
                    out,
                    format!(
                        "\n{}\n\n  Removes fields from user overrides when they match the base.\n  Dry-run by default — pass {} to persist.\n\n  Flags:\n    --apply    Write changes to disk (default: preview)\n    -y, --yes  Auto-delete empty override files (no prompt)\n\n",
                        style.bold("  agentsync simplify [<tool>] [--apply] [-y]"),
                        style.cyan("--apply")
                    )
                    .as_bytes(),
                )?;
                return Ok(0);
            }
            flag if flag.starts_with('-') => {
                put(err, format!("{}: Unknown flag: {flag}\n", style.red("Error")).as_bytes())?;
                return Ok(1);
            }
            value if filter.is_empty() => filter = value.to_string(),
            _ => {
                put(err, format!("{}: Only one tool at a time.\n", style.red("Error")).as_bytes())?;
                return Ok(1);
            }
        }
    }

    let project = discover()?;
    if apply && !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, err);
    }
    let mut matched = false;
    for slug in project.user_override_tools()? {
        if !filter.is_empty() && slug != filter {
            continue;
        }
        matched = true;
        simplify_tool(&project, &slug, apply, auto_yes, interactive, style, ask, out)?;
    }
    if payload_overrides(&project, apply, auto_yes, &filter, interactive, style, ask, out)? {
        matched = true;
    }
    if !matched {
        if !filter.is_empty() {
            put(err, format!("{}: No override found for '{filter}'.\n", style.red("Error")).as_bytes())?;
            return Ok(1);
        }
        put(
            out,
            format!("\n  {}\n\n", style.dim("No user overrides — nothing to simplify.")).as_bytes(),
        )?;
        return Ok(0);
    }
    let footer = if apply {
        format!(
            "\n{}\n  Run {} to verify outputs are unchanged.\n\n",
            style.green("Done."),
            style.cyan("agentsync sync")
        )
    } else {
        format!("\n{}\n\n", style.dim("Dry run — pass --apply to persist."))
    };
    put(out, footer.as_bytes())?;
    Ok(0)
}

/// `_simplify_one_tool`.
#[allow(clippy::too_many_arguments)]
fn simplify_tool(
    project: &Project,
    slug: &str,
    apply: bool,
    auto_yes: bool,
    interactive: bool,
    style: &Style,
    ask: Ask,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let user_file = project.user_tool_file(slug);
    if !user_file.is_file() {
        return Ok(());
    }
    let base = catalog::base_tool_yaml(slug);
    let rel = relative(project, &user_file);
    put(
        out,
        format!(
            "\n{}\n{}\n",
            style.bold(&format!("  {}", Tool::load(project, slug)?.display_name())),
            style.dim(&format!("  override: {rel}"))
        )
        .as_bytes(),
    )?;
    let user_text = read_text(&user_file);
    let (mut redundant, mut kept, mut user_only) = (Vec::new(), Vec::new(), Vec::new());
    for key in KEYS {
        let user = yaml_subset::value(&user_text, key);
        if user.is_empty() {
            continue;
        }
        let shipped = base.map(|t| yaml_subset::value(t, key)).unwrap_or_default();
        if !shipped.is_empty() && user == shipped {
            redundant.push((key, user));
        } else if shipped.is_empty() {
            user_only.push((key, user));
        } else {
            kept.push((key, user));
        }
    }
    if redundant.is_empty() {
        put(out, format!("{}\n", style.dim("  No redundant fields — already minimal.")).as_bytes())?;
        return Ok(());
    }
    let mut text = format!("  {}\n", style.yellow("Redundant (match base):"));
    for (key, value) in &redundant {
        text.push_str(&format!("    {} {key:<42}  {value}\n", style.dim("-")));
    }
    text.push('\n');
    for (title, list) in [("  Kept (diverge from base):", &kept), ("  Kept (no base value):", &user_only)] {
        if list.is_empty() {
            continue;
        }
        text.push_str(&format!("{}\n", style.dim(title)));
        for (key, value) in list {
            text.push_str(&format!("    {} {key:<42}  {value}\n", style.dim("=")));
        }
        text.push('\n');
    }
    if !apply {
        let hint = if kept.len() + user_only.len() == 0 {
            "  → would delete the override file (all fields match base).".to_string()
        } else {
            format!("  → would remove {} field(s).", redundant.len())
        };
        text.push_str(&format!("{}\n\n", style.dim(&hint)));
        return put(out, text.as_bytes());
    }
    put(out, text.as_bytes())?;
    for (key, _) in &redundant {
        yaml_edit::remove_key(&user_file, key)?;
    }
    put(out, format!("  {} {} field(s).\n", style.green("Removed"), redundant.len()).as_bytes())?;
    if !has_content(&user_file) {
        let delete = auto_yes
            || (interactive && yes(&ask(&style.bold("Delete empty override file? [y/N]"), out)));
        if delete {
            std::fs::remove_file(&user_file).map_err(|e| Error::io(&user_file, e))?;
            put(out, format!("  {} {rel}\n", style.green("Deleted")).as_bytes())?;
        } else {
            put(
                out,
                format!("{}\n", style.dim("  Kept empty file — remove manually if desired.")).as_bytes(),
            )?;
        }
    }
    put(out, b"\n")
}

fn sorted_entries(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut entries: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                .filter(|(name, _)| !name.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

/// `_simplify_payload_overrides`: whether any payload was considered.
#[allow(clippy::too_many_arguments)]
fn payload_overrides(
    project: &Project,
    apply: bool,
    auto_yes: bool,
    filter: &str,
    interactive: bool,
    style: &Style,
    ask: Ask,
    out: &mut dyn Write,
) -> Result<bool, Error> {
    let src = project.root.join(".ai").join("src");
    let (mut redundant, mut kept, mut legacy) = (Vec::new(), Vec::new(), Vec::new());
    let mut matched = false;
    for (tool, dir) in sorted_entries(&src.join("tools")) {
        if !dir.is_dir() || (!filter.is_empty() && tool != filter) {
            continue;
        }
        let loaded = Tool::load(project, &tool)?;
        for resource in ["hooks", "mcp", "settings"] {
            for (name, file) in sorted_entries(&dir) {
                if !name.starts_with(&format!("{resource}.")) || !file.is_file() {
                    continue;
                }
                matched = true;
                let identical = loaded.base_payload(resource).is_some_and(|base| {
                    std::fs::read(&file).is_ok_and(|bytes| bytes == base.contents())
                });
                if identical {
                    redundant.push(file);
                } else {
                    kept.push(file);
                }
            }
        }
    }
    for resource in ["hooks", "mcp", "settings"] {
        for (name, file) in sorted_entries(&src.join(resource)) {
            if !file.is_file() {
                continue;
            }
            let tool = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
            if !filter.is_empty() && tool != filter {
                continue;
            }
            matched = true;
            legacy.push(file);
        }
    }
    if redundant.is_empty() && kept.is_empty() && legacy.is_empty() {
        return Ok(matched);
    }

    let mut text = format!("\n{}\n\n", style.bold("  Payload overrides"));
    if !legacy.is_empty() {
        text.push_str(&format!(
            "  {}\n",
            style.yellow("Legacy layout — move into .ai/src/tools/<tool>/ (flat layout is deprecated):")
        ));
        for file in &legacy {
            text.push_str(&format!("    {} {}\n", style.dim("·"), relative(project, file)));
        }
        text.push_str(&format!(
            "\n{}\n{}\n\n",
            style.dim(&format!("  Run {} to preview the migration,", style.cyan("agentsync migrate --legacy"))),
            style.dim(&format!("  then {} to move these files.", style.cyan("agentsync migrate --apply")))
        ));
    }
    if redundant.is_empty() && kept.is_empty() {
        put(out, text.as_bytes())?;
        return Ok(matched);
    }
    if redundant.is_empty() {
        text.push_str(&format!(
            "{}\n",
            style.dim(&format!("  No byte-identical payload overrides — {} real customization(s).", kept.len()))
        ));
        put(out, text.as_bytes())?;
        return Ok(matched);
    }
    text.push_str(&format!("  {}\n", style.yellow("Byte-identical to base (safe to delete):")));
    for file in &redundant {
        text.push_str(&format!("    {} {}\n", style.dim("-"), relative(project, file)));
    }
    text.push('\n');
    if !kept.is_empty() {
        text.push_str(&format!(
            "{}\n",
            style.dim(&format!("  Kept (diverge from base or no base): {} file(s)", kept.len()))
        ));
    }
    if !apply {
        text.push_str(&format!(
            "{}\n",
            style.dim(&format!("  → would delete {} payload override(s).", redundant.len()))
        ));
        put(out, text.as_bytes())?;
        return Ok(matched);
    }
    put(out, text.as_bytes())?;
    let (mut deleted, mut skipped) = (0usize, 0usize);
    for file in &redundant {
        let rel = relative(project, file);
        let delete = auto_yes
            || !interactive
            || yes(&ask(&style.bold(&format!("Delete {rel}? [y/N]")), out));
        if delete {
            std::fs::remove_file(file).map_err(|e| Error::io(file, e))?;
            if let Some(dir) = file.parent() {
                let _ = std::fs::remove_dir(dir);
            }
            put(out, format!("  {} {rel}\n", style.green("Deleted")).as_bytes())?;
            deleted += 1;
        } else {
            put(out, format!("{}\n", style.dim(&format!("  Kept {rel}"))).as_bytes())?;
            skipped += 1;
        }
    }
    put(
        out,
        format!("\n{}\n", style.dim(&format!("  Removed {deleted}, kept {skipped}."))).as_bytes(),
    )?;
    Ok(matched)
}
```

As in `cmd_simplify`, an empty positional leaves the filter empty, so the next positional still takes its place.

In `src/main.rs`: add `"simplify"` to the raw dispatch and the branch

```rust
            "simplify" => cli::simplify::simplify(
                &rest,
                &Project::discover,
                &style,
                prompts::is_tty(),
                &mut |prompt: &str, out: &mut dyn Write| {
                    let _ = write!(out, "  {prompt} ");
                    let _ = out.flush();
                    prompts::read_terminal()
                },
                &mut out,
                &mut err,
            ),
```

In `bin/agentsync.sh:280` append `simplify`.

- [ ] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/simplify.bats | grep -c '^not ok'
bats --tap -f 'simplify previews' tests/native_parity.bats
```

Expected: `203 passed`, `0`, `11`, `1`; `0`; `ok`.

- [ ] **Step 5: Prove the fixture bites, lint, commit**

Change `"  → would remove {} field(s)."` to `"  → would drop {} field(s)."`, rebuild, rerun the fixture: `not ok` with that diff; revert and rebuild.

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/simplify.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-15-rust-migration-phase-4c-simplify-resolve.md
git commit -m "feat(native): port simplify"
```

---

### Task 3: Port `resolve`

**Files:**
- Create: `src/cli/resolve.rs`
- Modify: `src/cli/mod.rs` (`pub mod resolve;`), `src/cli/diff.rs` (`KEYS` becomes `pub(crate)`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Consumes: Task 1, `cli::diff::KEYS`, `cli::customize::{put, relative}`.
- Produces: `pub fn resolve(args: &[String], discover: &dyn Fn() -> Result<Project, Error>, style: &Style, interactive: bool, ask: &mut dyn FnMut(&str, &mut dyn Write) -> String, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`

- [ ] **Step 1: Parity fixture, Bash side**

```bash
@test "parity: resolve reports read-only and clears the pending queue like Bash" {
    enable_tools cursor
    printf '# q\nconflicts:\n  - tool: "cursor"\n    field: "targets.rules.dest"\n' > .ai/.pending-resolutions.yaml
    assert_tree_parity resolve
    _run_engine 0 customize cursor >/dev/null
    printf 'targets:\n  rules:\n    dest: ".mine"\n' >> .ai/src/tools/cursor.yaml
    assert_tree_parity resolve
    assert_tree_parity resolve nope
    assert_tree_parity resolve --bogus
}
```

Run: `bats --tap -f 'resolve reports' tests/native_parity.bats`
Expected: `ok`.

- [ ] **Step 2: Write the failing tests**

Create `src/cli/resolve.rs` with the tests module only; add `pub mod resolve;`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn call(root: &str, args: &[&str], interactive: bool, answers: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let mut answers = answers.iter();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = resolve(
            &args,
            &discover,
            &Style::plain(),
            interactive,
            &mut |prompt, out| {
                let _ = out.write_all(format!("        {prompt} ").as_bytes());
                answers.next().copied().unwrap_or("").to_string()
            },
            &mut out,
            &mut err,
        )
        .unwrap();
        (status, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().to_string_lossy().into_owned();
        std::fs::create_dir_all(format!("{root}/.ai/src/tools")).unwrap();
        (dir, root)
    }

    #[test]
    fn resolve_without_overrides_clears_the_queue_and_without_a_terminal_only_reports() {
        let (_dir, root) = project();
        let pending = format!("{root}/.ai/.pending-resolutions.yaml");
        std::fs::write(&pending, "conflicts:\n  - tool: \"cursor\"\n    field: \"name\"\n").unwrap();
        assert_eq!(
            call(&root, &[], false, &[]),
            (0, "\n  No user overrides — nothing to resolve.\n\n".to_string(), String::new())
        );
        assert!(!std::path::Path::new(&pending).exists());

        std::fs::write(format!("{root}/.ai/src/tools/cursor.yaml"), "name: Mine\n").unwrap();
        assert_eq!(
            call(&root, &["nope"], false, &[]).1,
            "\n  Resolve (read-only — not a TTY)\n  Run from an interactive shell to review overrides one by one.\n  Use agentsync diff for a full list.\n\n"
        );
    }

    #[test]
    fn an_interactive_walk_adopts_keeps_and_marks_flagged_fields() {
        let (_dir, root) = project();
        let file = format!("{root}/.ai/src/tools/cursor.yaml");
        std::fs::write(&file, "name: Mine\ntargets:\n  rules:\n    dest: \".mine\"\n").unwrap();
        std::fs::write(
            format!("{root}/.ai/.pending-resolutions.yaml"),
            "conflicts:\n  - tool: \"cursor\"\n    field: \"targets.rules.dest\"\n",
        )
        .unwrap();
        let (status, out, _) = call(&root, &[], true, &["k", "a"]);
        assert_eq!(status, 0);
        assert_eq!(
            out,
            "\n  ⚡ 1 field(s) flagged by the last agentsync update\n  Upstream changed base values while you had overrides. Flagged entries\n  are marked with ⚡ below.\n\n  Mine\n  override: .ai/src/tools/cursor.yaml\n  base:     lib/templates/tools/cursor.yaml\n\n    ◆ name\n        user: Mine\n        base: Cursor\n        [k]eep / [a]dopt base / [s]kip         → kept user value\n\n    ⚡ targets.rules.dest\n        user: .mine\n        base: .cursor/rules\n        [k]eep / [a]dopt base / [s]kip         → adopted base value\n\n\nDone.\n  Run agentsync sync to apply any changes.\n\n"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "name: Mine\ntargets:\n  rules:\n");
        assert!(!std::path::Path::new(&format!("{root}/.ai/.pending-resolutions.yaml")).exists());
        assert_eq!(
            call(&root, &["nope"], true, &[]).2,
            "Error: No override found for 'nope'.\n"
        );
    }
}
```

Before running, confirm the prompt spacing against Bash: `printf "        %s " "$(_bold ...)"` prints eight spaces, the prompt, one space, and the answer's echo `echo "        $(_green ...)"` begins on the same line because the answer comes from the terminal, not from stdout. The expected string above encodes that as `[s]kip ` followed by `        → …`.

Run: `cargo test --lib cli::resolve 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function 'resolve'`.

- [ ] **Step 3: Write the implementation**

In `src/cli/diff.rs` change `const KEYS` to `pub(crate) const KEYS`.

Above the tests module in `src/cli/resolve.rs`:

```rust
//! `agentsync resolve`: `cmd_resolve` of `lib/helpers/resolve_cmd.sh`, which
//! walks every overridden field on a terminal and lets the base value win.

use std::io::Write;

use super::customize::{put, relative};
use super::diff::KEYS;
use crate::project::Project;
use crate::style::Style;
use crate::tool::Tool;
use crate::{Error, catalog, snapshot, yaml_edit, yaml_subset};

type Ask<'a> = &'a mut dyn FnMut(&str, &mut dyn Write) -> String;

pub fn resolve(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    interactive: bool,
    ask: Ask,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let filter = args.first().cloned().unwrap_or_default();
    let project = discover()?;
    let pending = snapshot::read_pending_pairs(&project.root);
    let overrides = project.user_override_tools()?;
    if overrides.is_empty() {
        put(
            out,
            format!("\n  {}\n\n", style.dim("No user overrides — nothing to resolve.")).as_bytes(),
        )?;
        if !pending.is_empty() {
            snapshot::clear_pending(&project.root);
        }
        return Ok(0);
    }
    if !interactive {
        put(
            out,
            format!(
                "\n{}\n  Run from an interactive shell to review overrides one by one.\n  Use {} for a full list.\n\n",
                style.bold("  Resolve (read-only — not a TTY)"),
                style.cyan("agentsync diff")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    if !pending.is_empty() {
        put(
            out,
            format!(
                "\n  {} {}\n  {}\n  {} {} {}\n",
                style.yellow(&format!("⚡ {} field(s) flagged by the last", pending.len())),
                style.cyan("agentsync update"),
                style.dim("Upstream changed base values while you had overrides. Flagged entries"),
                style.dim("are marked with"),
                style.yellow("⚡"),
                style.dim("below.")
            )
            .as_bytes(),
        )?;
    }
    let mut matched = false;
    for slug in overrides.iter().filter(|t| filter.is_empty() || **t == filter) {
        resolve_tool(&project, slug, &pending, style, ask, out)?;
        matched = true;
    }
    if !matched {
        put(err, format!("{}: No override found for '{filter}'.\n", style.red("Error")).as_bytes())?;
        return Ok(1);
    }
    if !pending.is_empty() && filter.is_empty() {
        snapshot::clear_pending(&project.root);
    }
    put(
        out,
        format!(
            "\n{}\n  Run {} to apply any changes.\n\n",
            style.green("Done."),
            style.cyan("agentsync sync")
        )
        .as_bytes(),
    )?;
    Ok(0)
}

/// `_resolve_one_tool`.
fn resolve_tool(
    project: &Project,
    slug: &str,
    pending: &[(String, String)],
    style: &Style,
    ask: Ask,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let base = catalog::base_tool_yaml(slug);
    let user_file = project.user_tool_file(slug);
    let base_line = if base.is_some() {
        format!("  base:     lib/templates/tools/{slug}.yaml")
    } else {
        "  base:     (custom tool — no base)".to_string()
    };
    put(
        out,
        format!(
            "\n{}\n{}\n{}\n\n",
            style.bold(&format!("  {}", Tool::load(project, slug)?.display_name())),
            style.dim(&format!("  override: {}", relative(project, &user_file))),
            style.dim(&base_line)
        )
        .as_bytes(),
    )?;
    let mut any = false;
    for key in KEYS {
        let user_text = std::fs::read(&user_file)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let user = yaml_subset::value(&user_text, key);
        let shipped = base.map(|t| yaml_subset::value(t, key)).unwrap_or_default();
        if user.is_empty() || user == shipped {
            continue;
        }
        any = true;
        let flagged = pending.iter().any(|(tool, field)| tool == slug && field == key);
        let marker = style.yellow(if flagged { "⚡" } else { "◆" });
        let shipped_shown = if shipped.is_empty() { style.dim("(not set)") } else { shipped };
        put(
            out,
            format!(
                "    {marker} {key}\n        {} {user}\n        {} {shipped_shown}\n",
                style.dim("user:"),
                style.dim("base:")
            )
            .as_bytes(),
        )?;
        let answer = ask(&style.bold("[k]eep / [a]dopt base / [s]kip"), out);
        let outcome = match answer.as_str() {
            "a" | "A" | "adopt" => {
                yaml_edit::remove_key(&user_file, key)?;
                style.green("→ adopted base value")
            }
            "s" | "S" | "skip" | "" => style.dim("→ skipped"),
            _ => style.dim("→ kept user value"),
        };
        put(out, format!("        {outcome}\n\n").as_bytes())?;
    }
    if !any {
        put(out, format!("{}\n", style.dim("    No diverging fields.")).as_bytes())?;
    }
    Ok(())
}
```

In `src/main.rs`: add `"resolve"` to the raw dispatch and the branch

```rust
            "resolve" => cli::resolve::resolve(
                &rest,
                &Project::discover,
                &style,
                prompts::is_tty(),
                &mut |prompt: &str, out: &mut dyn Write| {
                    let _ = write!(out, "        {prompt} ");
                    let _ = out.flush();
                    prompts::read_terminal()
                },
                &mut out,
                &mut err,
            ),
```

In `bin/agentsync.sh:280` append `resolve`.

- [ ] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
bats --tap -f 'resolve reports' tests/native_parity.bats
```

Expected: `205 passed`, `0`, `11`, `1`; `ok`.

- [ ] **Step 5: Prove the fixture bites, lint, commit**

Change `"No user overrides — nothing to resolve."` to `"No overrides — nothing to resolve."`, rebuild, rerun the fixture: `not ok` with that diff; revert and rebuild.

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/resolve.rs src/cli/diff.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-15-rust-migration-phase-4c-simplify-resolve.md
git commit -m "feat(native): port resolve"
```

---

### Task 4: Verify, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [ ] **Step 1: Spec**

Append to "Known quirks":

```markdown
21. `simplify`'s payload pass scans `.ai/src/tools` even when `source.tools`
    moves the tool override directory.
22. `resolve` without a terminal ignores its tool filter and exits 0.
23. `yaml_remove_key` (`simplify --apply`, `resolve` adopt) drops the blank
    lines directly after the removed block.
24. `resolve` in a project without overrides deletes
    `.ai/.pending-resolutions.yaml`, terminal or not.
25. `simplify --apply` without a terminal deletes byte-identical payload copies
    but keeps an override file it emptied.
```

- [ ] **Step 2: Module map and outputs**

Set the `lib/helpers/simplify.sh` row to `→ src/cli/simplify.rs     Phase 4c, ported`, the `lib/helpers/resolve_cmd.sh` row to `→ src/cli/resolve.rs      Phase 4c, ported`, the `lib/helpers/snapshot.sh` row's note to `read_pending_pairs, clear_pending (Phase 4c); save, diff, conflicts wait for update`, and extend the `lib/helpers/yaml_edit.sh` row's note with `, remove_key (Phase 4c)` in place of `remove_key` in its waiting list. Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [ ] **Step 3: Verify (outside the agent sandbox, since `native_parity` reaches `diff`)**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in simplify update_snapshot customize native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `205 passed`, `0`, `11`, `1`; lint exit 0; every line `bash=0 native=0`.

- [ ] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-15-rust-migration-phase-4c-simplify-resolve.md
git commit -m "docs(native): map the phase 4c modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, `simplify.bats` is green under `AGENTSYNC_NATIVE=1`, the parity fixtures pass, the files in Task 4 pass in both modes, and a `## Completion receipt` records the fresh verification. The next plan is Phase 4d.

## Run log

### 2026-09-15 — Phase 4c planned
- Commits: this plan.
- Verified: `scratchpad/phase4/simplify_reference.sh` ran `simplify`, `resolve`, and `yaml_remove_key` in Bash (output in `simplify_reference.out`); no `tests/*.bats` file runs `resolve`, so the parity fixture is its only CLI coverage.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.

### 2026-09-15 — Tasks 0 and 1 done
- Commits: "feat(native): port yaml_remove_key and the pending-resolution reader".
- Verified: Task 0 at `9074ff3`: `simplify`, `update_snapshot`, `customize`, `native_parity` all `bash=0`. Task 1: `cargo test` 201 lib, 11 cli, 1 interrupt; fmt and clippy exit 0.
- Plan amended: none.
- Next: Task 2 Step 1 (Tasks 2 and 3 are written and pass `cargo test` at 205).
- Blocker: none.
