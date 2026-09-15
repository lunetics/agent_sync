# Rust Migration Phase 4g: Native `migrate`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync migrate` (the upgrade prompt and `--legacy`) so the binary answers it byte for byte like `cmd_migrate` in `lib/helpers/migrate.sh`, after fixing three Bash bugs the reference turned up.

**Architecture:** `src/format_rev.rs` reads the engine's embedded `FORMAT` and a project's `format:`. `template_manifest::TemplateManifest` adds load, lookup, remove, and write to Phase 4e's hash, sharing the `IFS=$'\t' read` line split with `manifest.rs`. `catalog` embeds `lib/prompts/migrate.md` and lists the engine-owned `base-src` skills. `src/cli/migrate.rs` holds the prompt, the clipboard pipe, and the legacy pass; `main` hands it an `Env` carrying the version, the unchecked prompt root, `AGENTSYNC_NO_CLIPBOARD`, whether stdout and the session are terminals, `prompts::confirm`, and a clipboard closure over `PATH`. The seam stays the CLI process boundary: `tests/migrate.bats` and `tests/format_migration.bats` under `AGENTSYNC_NATIVE=1` plus a parity fixture.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-15-rust-migration-phase-4f-adopt.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `migrate` prints the prompt and writes nothing but the clipboard; `migrate --legacy --apply` moves only legacy flat-layout files into the tool override directory, writes only `.ai/src/mcp.json` on consolidation, removes only `.agent/` and unedited engine-owned skill copies, and sets only `format:`; a dry run writes nothing.
- No binary ships to users; without a binary every command runs in Bash. Three Bash changes, Tasks 1–3, each in its own commit with a regression test in `tests/migrate.bats`; `lib/**/*.sh` and `bin/agentsync.sh` stay clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state.
- Disk-touching unit tests are `#[cfg(unix)]`.
- Tests and runs never touch the developer's clipboard: every bats run and reference sets `AGENTSYNC_NO_CLIPBOARD=1` or a `pbcopy` shim on `PATH`.
- Every expected value was captured from the fixed Bash on 2026-09-15: `scratchpad/phase4g/migrate_reference.sh` (every non-interactive branch and both clipboard outcomes, output in `cmp_bash.out`), `migrate_tty.sh` (the two prompts and the terminal banners), `no_clipboard_reference.sh`, `format_reference.sh`, `tm_reference.sh`, and `mode_probe.sh`.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-15 under the maintainer's `/decide`: both as recommended.

1. **Fix three Bash bugs before porting.** Each was reproduced on the committed `migrate.sh`, each regression test fails there and passes with the fix, and `migrate.bats`, `format_migration.bats`, and `doctor.bats` stay green on the fixed copy.
   - **Consolidation deletes a non-JSON MCP override.** `_migrate_mcp_consolidation_candidate` compares only `mcp/*.json`, but consolidation removes every legacy MCP file. With identical `claude.json` and `cursor.json` next to `codex.toml`, `migrate --apply --yes` reported `consolidated .ai/src/mcp/codex.toml → .ai/src/mcp.json`, left only the JSON in `mcp.json`, and deleted `codex.toml`. **Recommended:** any non-JSON MCP file rules consolidation out, so every file moves per tool.
   - **Moves ignore `source.tools`.** Destinations are hard-coded to `.ai/src/tools/`, while `sync` reads the tool override directory. With `source.tools: catalog`, a migrated `hooks/cursor.json` landed where `sync` no longer read it, and the next sync dropped the custom hooks from `.cursor/hooks.json`. **Recommended:** move into `tool_resolver_user_dir`, and refuse with `tool_resolver_require_project_user_dir` before the first change when that directory lies outside the project, as the other commands that write tool config do. The router's `_need` list for `migrate` gains `logging paths`, which that check calls.
   - **Listings follow the locale.** `_migrate_scan_legacy`, the consolidation candidate, and the `.agent/` listing glob, which Bash sorts by `LC_COLLATE`: `Zed.json`, `_x.json`, `claude.json` plan in that order under `C` and as `_x, claude, Zed` under `en_US.UTF-8`, and the consolidation's `Source file` changes with it. **Recommended:** sort those globs by bytes, as Phase 4e did for `dedupe`.
   Alternative: port the behaviour as is; the first two lose user configuration, so they are not offered as quirks.
2. **Quirks and deviations.** Record as known quirks 35–37: `migrate --apply --yes` prints `removed .agent/ (pre-v0.6 layout)` with no blank line before `Planned moves:`; a legacy file without an extension such as `.ai/src/settings/README` moves to `.ai/src/tools/README/settings.README`; off a terminal without `--yes`, `migrate --apply` consolidates identical MCP files but leaves `.agent/` in place. Record as an accepted deviation that `migrate` prints the embedded `lib/prompts/migrate.md` where Bash read the install directory's copy. **Recommended:** as listed.

## Module closure

```text
lib/helpers/migrate.sh          17-35   _migrate_prepare_context (status 1 for a missing config path)
                                39-66   _migrate_scan_legacy, _migrate_format_move
                                74-120  _migrate_mcp_consolidation_candidate, _migrate_move_one
                                125-158 _migrate_has_legacy_agent_dir, _migrate_remove_legacy_agent_dir, _migrate_cleanup_empty_dirs
                                163-247 _migrate_scan_base_skills, _migrate_retire_base_skills, _migrate_write_format
                                249-472 _cmd_migrate_legacy
                                474-612 _migrate_prompt_file, _migrate_project_version, _migrate_copy_prompt, _cmd_migrate_prompt
                                614-627 cmd_migrate
lib/helpers/format.sh           11-35   engine_format, project_format
lib/helpers/template_manifest.sh 45-133 load, lookup, record, remove, write (record waits for refresh and init)
bin/agentsync.sh                388     migrate's _need list
```

Reused: `Project::{discover, user_tools_dir, tools_dir_in_project}`, `yaml_edit::set_scalar`, `yaml_subset::value`, `template_manifest::hash`, `manifest::sha256_hex`, `catalog::engine_files`, `cli::refuse_outside_tools_dir`, `cli::customize::put`, `paths::logical_root`, `prompts::{is_tty, confirm}`, `style`.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in migrate format_migration doctor native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: this plan's commit; `223 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for every file.

---

### Task 1: Consolidation keeps a non-JSON MCP override

**Files:**
- Modify: `lib/helpers/migrate.sh` (`_migrate_mcp_consolidation_candidate`)
- Test: `tests/migrate.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing test**

Append to `tests/migrate.bats`:

```bash

@test "migrate --apply keeps a non-JSON MCP override next to identical JSON ones" {
    mkdir -p .ai/src/mcp
    printf '{"mcpServers": {}}\n' > .ai/src/mcp/claude.json
    cp .ai/src/mcp/claude.json .ai/src/mcp/cursor.json
    printf '[mcp_servers]\n' > .ai/src/mcp/codex.toml

    run run_agentsync migrate --apply --yes
    [ "$status" -eq 0 ]
    grep -q 'mcp_servers' .ai/src/tools/codex/mcp.toml
    [ ! -f .ai/src/mcp.json ]
}
```

Run: `bats --tap -f 'non-JSON MCP' tests/migrate.bats`
Expected: `not ok 1 migrate --apply keeps a non-JSON MCP override next to identical JSON ones`.

- [x] **Step 2: Rule out consolidation when any MCP file is not JSON**

Replace the collection loop of `_migrate_mcp_consolidation_candidate`:

```bash
    local -a files=()
    local f
    for f in "$mcp_dir"/*; do
        [[ -f "$f" ]] || continue
        # Only JSON folds into mcp.json; any other MCP config moves per tool.
        [[ "$f" == *.json ]] || return 1
        files+=("$f")
    done
```

- [x] **Step 3: Run the tests, confirm green**

```bash
bats --tap tests/migrate.bats | grep -c '^ok'
shellcheck -x -S warning -e SC1091 lib/helpers/migrate.sh
```

Expected: `19`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/migrate.sh tests/migrate.bats docs/plans/2026-09-15-rust-migration-phase-4g-migrate.md
git commit -m "fix(migrate): keep non-JSON MCP overrides out of consolidation"
```

---

### Task 2: Moves go to the tool override directory

**Files:**
- Modify: `lib/helpers/migrate.sh` (`_migrate_format_move`, `_migrate_move_one`, `_cmd_migrate_legacy`), `bin/agentsync.sh:388`
- Test: `tests/migrate.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing tests**

Append to `tests/migrate.bats`:

```bash

@test "migrate --apply moves overrides into the source.tools directory" {
    mkdir -p .ai/src/hooks catalog
    printf '{}\n' > .ai/src/hooks/cursor.json
    printf 'format: 2\ntools:\n  enabled: []\nsource:\n  tools: "catalog"\n' > .ai/agent_sync.yaml

    run run_agentsync migrate --apply --yes
    [ "$status" -eq 0 ]
    [ -f catalog/cursor/hooks.json ]
    [ ! -e .ai/src/tools/cursor/hooks.json ]
}

@test "migrate --apply refuses a source.tools outside the project before changing anything" {
    mkdir -p .ai/src/hooks
    printf '{}\n' > .ai/src/hooks/cursor.json
    printf 'tools:\n  enabled: []\nsource:\n  tools: "../elsewhere"\n' > .ai/agent_sync.yaml

    run run_agentsync migrate --apply --yes
    [ "$status" -eq 1 ]
    [[ "$output" == *"source.tools resolves outside the project"* ]]
    [ -f .ai/src/hooks/cursor.json ]
    ! grep -q '^format:' .ai/agent_sync.yaml
}
```

Run: `bats --tap -f 'source.tools' tests/migrate.bats`
Expected: `not ok 1 migrate --apply moves overrides into the source.tools directory` and `not ok 2 migrate --apply refuses a source.tools outside the project before changing anything`.

- [x] **Step 2: Resolve destinations through the tool override directory**

In both `_migrate_format_move` and `_migrate_move_one`, replace `local dest="$REPO_ROOT/.ai/src/tools/${tool}/${resource}.${ext}"` with:

```bash
    local dest
    dest="$(tool_resolver_user_dir)/${tool}/${resource}.${ext}"
```

In `_cmd_migrate_legacy`, after the `Nothing to migrate.` block and before `if [[ -n "$base_skill_copies" ]]`:

```bash
    # Refuse before the first change: the moves below write into the tool override directory.
    if [[ "$apply" == "true" && -n "$legacy" ]]; then
        tool_resolver_require_project_user_dir
    fi

```

In `bin/agentsync.sh:388`:

```bash
        migrate)       _need prompts yaml yaml_edit logging paths tool_resolver project_config template_manifest format migrate; shift; cmd_migrate "$@" ;;
```

- [x] **Step 3: Run the tests, confirm green**

```bash
for f in migrate format_migration doctor; do
    printf '%s ok=%s notok=%s\n' "$f" "$(bats --tap "tests/$f.bats" | grep -c '^ok')" "$(bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
shellcheck -x -S warning -e SC1091 bin/agentsync.sh lib/helpers/migrate.sh
```

Expected: `migrate ok=21 notok=0`, `format_migration ok=11 notok=0`, `doctor ok=36 notok=0`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/migrate.sh bin/agentsync.sh tests/migrate.bats docs/plans/2026-09-15-rust-migration-phase-4g-migrate.md
git commit -m "fix(migrate): move overrides into the tool override directory"
```

---

### Task 3: Legacy listings in byte order

**Files:**
- Modify: `lib/helpers/migrate.sh` (`_migrate_scan_legacy`, `_migrate_mcp_consolidation_candidate`, `_cmd_migrate_legacy`'s `.agent/` listing)
- Test: `tests/migrate.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing test**

Append to `tests/migrate.bats`:

```bash

@test "migrate --legacy lists legacy files in byte order whatever the locale" {
    locale -a 2>/dev/null | grep -qix 'en_US.utf-\{0,1\}8' || skip "en_US.UTF-8 locale not installed"
    mkdir -p .ai/src/settings
    local name
    for name in claude Zed _x; do
        printf '{}\n' > ".ai/src/settings/$name.json"
    done

    run env LC_ALL=en_US.UTF-8 AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" migrate --legacy
    [ "$status" -eq 0 ]
    [[ "$output" == *"settings/Zed.json"*"settings/_x.json"*"settings/claude.json"* ]]
}
```

Run: `bats --tap -f 'byte order' tests/migrate.bats`
Expected: `not ok 1 migrate --legacy lists legacy files in byte order whatever the locale`.

- [x] **Step 2: Sort the three globs by bytes**

In `_migrate_scan_legacy`:

```bash
        while IFS= read -r -d '' file; do
            [[ -f "$file" ]] || continue
            base=$(basename "$file")
            tool="${base%.*}"
            ext="${base##*.}"
            [[ -z "$tool" ]] && continue
            [[ -z "$ext" ]] && continue
            echo "${resource}|${tool}|${file}|${ext}"
        done < <(printf '%s\0' "$root/$resource"/* | LC_ALL=C sort -z)
```

In `_migrate_mcp_consolidation_candidate`:

```bash
    while IFS= read -r -d '' f; do
        [[ -f "$f" ]] || continue
        # Only JSON folds into mcp.json; any other MCP config moves per tool.
        [[ "$f" == *.json ]] || return 1
        files+=("$f")
    done < <(printf '%s\0' "$mcp_dir"/* | LC_ALL=C sort -z)
```

In the `.agent/` listing of `_cmd_migrate_legacy`:

```bash
        while IFS= read -r -d '' item; do
            [[ -e "$item" ]] || continue
            echo "      · ${item#"$REPO_ROOT/.agent/"}"
        done < <(printf '%s\0' "$REPO_ROOT/.agent"/* | LC_ALL=C sort -z)
```

- [x] **Step 3: Run the tests, confirm green**

```bash
for f in migrate format_migration doctor; do
    printf '%s ok=%s notok=%s\n' "$f" "$(bats --tap "tests/$f.bats" | grep -c '^ok')" "$(bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
shellcheck -x -S warning -e SC1091 lib/helpers/migrate.sh
```

Expected: `migrate ok=22 notok=0`, `format_migration ok=11 notok=0`, `doctor ok=36 notok=0`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/migrate.sh tests/migrate.bats docs/plans/2026-09-15-rust-migration-phase-4g-migrate.md
git commit -m "fix(migrate): list legacy files in byte order"
```

---

### Task 4: Port the format revision and the template manifest file

**Files:**
- Create: `src/format_rev.rs`
- Modify: `src/lib.rs` (`pub mod format_rev;` after `pub mod filters;`), `src/template_manifest.rs`, `src/manifest.rs`

**Interfaces:**
- Produces:
  - `pub fn format_rev::engine() -> u32`, `pub fn format_rev::project(config: &str) -> u32`
  - `pub const template_manifest::REL: &str`, `pub struct TemplateManifest` with `load(root: &Path) -> Result<Self, Error>`, `lookup(&self, rel: &str) -> Option<&str>`, `remove(&mut self, rel: &str)`, `write(&self, root: &Path) -> Result<(), Error>`
  - `pub(crate) fn manifest::hashed_lines(bytes: &[u8]) -> Vec<(String, String)>`

- [x] **Step 1: Write the failing tests**

Create `src/format_rev.rs` with its tests module only, and add `pub mod format_rev;` to `src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_read_as_format_sh_reads_them() {
        assert_eq!(engine(), 2);
        assert_eq!(project("format: 2\n"), 2);
        assert_eq!(project("format: \"3\"\n"), 3);
        assert_eq!(project("tools:\n  enabled: []\n"), 1);
        assert_eq!(project("format: two\n"), 1);
        assert_eq!(project("format: -2\n"), 1);
    }
}
```

Append to the `tests` module of `src/template_manifest.rs`:

```rust

    #[test]
    fn entries_load_look_up_drop_and_write_back_sorted_like_bash() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            TemplateManifest::load(dir.path()).unwrap(),
            TemplateManifest::default()
        );
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(
            dir.path().join(REL),
            "z.md\tzz\n# c\th\nskills/a/SKILL.md\t1\nnohash\n\tb.md\t\tbb\t\nskills/a/SKILL.md\t2\n",
        )
        .unwrap();
        let mut manifest = TemplateManifest::load(dir.path()).unwrap();
        assert_eq!(manifest.lookup("skills/a/SKILL.md"), Some("1"));
        assert_eq!(manifest.lookup("nohash"), None);
        manifest.remove("skills/a/SKILL.md");
        manifest.write(dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(REL)).unwrap(),
            "b.md\tbb\nz.md\tzz\n"
        );
        manifest.remove("b.md");
        manifest.remove("z.md");
        manifest.write(dir.path()).unwrap();
        assert!(!dir.path().join(REL).exists());
    }
```

Run: `cargo test --lib 2>&1 | grep -E '^error\[E04(25|33)\]' | sort -u`
Expected: errors naming the missing `engine`, `project`, `TemplateManifest`, and `REL`.

- [x] **Step 2: Write the implementation**

Prepend to `src/format_rev.rs`:

```rust
//! `lib/helpers/format.sh`: the project format revision, a counter bumped only
//! when a project needs a migration step.

use crate::yaml_subset;

const ENGINE_FORMAT_FILE: &str = include_str!("../FORMAT");

fn revision(text: &str) -> u32 {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return 1;
    }
    text.parse().unwrap_or(1)
}

/// `engine_format`: the first line of `FORMAT`, 1 when it is not a number.
pub fn engine() -> u32 {
    revision(ENGINE_FORMAT_FILE.split('\n').next().unwrap_or_default())
}

/// `project_format`: `format:` without its quotes, 1 when absent or not a number.
pub fn project(config: &str) -> u32 {
    revision(&yaml_subset::value(config, "format").replace('"', ""))
}

```

In `src/manifest.rs`, move the body of `Manifest::parse` into a free function before `sha256_hex`, and let `parse` call it:

```rust
    /// The manifest's `hashed_lines`.
    pub fn parse(bytes: &[u8]) -> Self {
        Self {
            entries: hashed_lines(bytes),
        }
    }
```

```rust
/// `IFS=$'\t' read -r rel hash` per line: tabs around the line are dropped, the
/// hash is the rest after the first run of tabs, and comments and entries
/// without a hash are skipped.
pub(crate) fn hashed_lines(bytes: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(bytes);
    let mut entries = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_matches('\t');
        let (rel, hash) = match line.find('\t') {
            Some(tab) => (&line[..tab], line[tab..].trim_start_matches('\t')),
            None => (line, ""),
        };
        if rel.is_empty() || rel.starts_with('#') || hash.is_empty() {
            continue;
        }
        entries.push((rel.to_string(), hash.to_string()));
    }
    entries
}
```

In `src/template_manifest.rs`, replace everything above the tests module with:

```rust
//! `lib/helpers/template_manifest.sh`: content hashes of the templates copied
//! into `.ai/src/`.

use std::collections::BTreeSet;
use std::path::Path;

use crate::manifest::{hashed_lines, sha256_hex};
use crate::{Error, staging};

pub const REL: &str = ".ai/.template-manifest";

/// `template_manifest_hash`: the SHA-256 of a file, links followed; `None`
/// when the path is not a readable regular file.
pub fn hash(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    std::fs::read(path).ok().map(|bytes| sha256_hex(&bytes))
}

/// `TEMPLATE_MANIFEST_KEYS` and `TEMPLATE_MANIFEST_VALUES`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TemplateManifest {
    entries: Vec<(String, String)>,
}

impl TemplateManifest {
    /// `template_manifest_load`: empty when the file is missing.
    pub fn load(root: &Path) -> Result<Self, Error> {
        let path = root.join(REL);
        if !path.is_file() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
        Ok(Self {
            entries: hashed_lines(&bytes),
        })
    }

    /// `template_manifest_lookup`: the first hash recorded for `rel`.
    pub fn lookup(&self, rel: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(key, _)| key == rel)
            .map(|(_, hash)| hash.as_str())
    }

    /// `template_manifest_remove`: every entry for `rel`.
    pub fn remove(&mut self, rel: &str) {
        self.entries.retain(|(key, _)| key != rel);
    }

    /// `template_manifest_write`: `sort -u` lines, or no file when empty.
    pub fn write(&self, root: &Path) -> Result<(), Error> {
        let ai = root.join(".ai");
        std::fs::create_dir_all(&ai).map_err(|e| Error::io(&ai, e))?;
        let path = root.join(REL);
        if self.entries.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::io(&path, e)),
                _ => Ok(()),
            };
        }
        let lines: BTreeSet<String> = self
            .entries
            .iter()
            .map(|(rel, hash)| format!("{rel}\t{hash}"))
            .collect();
        let mut text = lines.into_iter().collect::<Vec<_>>().join("\n");
        text.push('\n');
        staging::write_beside(&path, text.as_bytes())
    }
}

```

- [x] **Step 3: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
```

Expected: `225 passed`, `0`, `11`, `1`; clippy exit 0.

- [x] **Step 4: Commit**

```bash
git add src/format_rev.rs src/lib.rs src/template_manifest.rs src/manifest.rs docs/plans/2026-09-15-rust-migration-phase-4g-migrate.md
git commit -m "feat(native): port the format revision and template manifest file"
```

---

### Task 5: Port `migrate`

**Files:**
- Create: `src/cli/migrate.rs`
- Modify: `src/catalog.rs`, `src/cli/mod.rs` (`pub mod migrate;` after `pub mod list;`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Consumes: Task 4's `format_rev` and `TemplateManifest`.
- Produces:
  - `pub const catalog::MIGRATE_PROMPT: &str`, `pub fn catalog::base_src_skills() -> Vec<String>`
  - `pub struct migrate::Env<'a> { version, prompt_root, no_clipboard, stdout_tty, interactive, confirm: &mut dyn FnMut(&str, bool) -> bool, copy: &mut dyn FnMut(&str) -> Option<i32> }`
  - `pub fn migrate::copy_to_clipboard(text: &str, path_var: Option<&str>) -> Option<i32>`
  - `pub fn migrate::migrate(args: &[String], discover: &dyn Fn() -> Result<Project, Error>, style: &Style, env: &mut Env, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`

- [x] **Step 1: Parity fixture, Bash side**

Append to `tests/native_parity.bats`:

```bash

# ── migrate ──────────────────────────────────────────────────────────────────

@test "parity: migrate prints the prompt and retires legacy layouts like Bash" {
    export AGENTSYNC_NO_CLIPBOARD=1
    assert_tree_parity migrate
    assert_tree_parity migrate --help
    assert_tree_parity migrate --bogus extra
    assert_tree_parity migrate --legacy --help
    assert_tree_parity migrate --legacy --bogus
    assert_tree_parity migrate --legacy
    assert_tree_parity migrate --apply
    mkdir -p .ai/src/hooks .ai/src/settings .ai/src/mcp .ai/src/tools/cursor .agent/rules
    printf '{"hooks": {}}\n' > .ai/src/hooks/cursor.json
    printf '{"taken": true}\n' > .ai/src/tools/cursor/settings.json
    printf '{"s": 1}\n' > .ai/src/settings/cursor.json
    printf '{"s": 2}\n' > .ai/src/settings/claude.json
    printf 'noext\n' > .ai/src/settings/README
    printf '{"mcpServers": {}}\n' | tee .ai/src/mcp/claude.json > .ai/src/mcp/cursor.json
    printf '# old\n' > .agent/AGENTS.md
    assert_tree_parity migrate --legacy
    assert_tree_parity migrate --legacy --apply
    assert_tree_parity migrate --apply --yes
    printf '[x]\n' > .ai/src/mcp/codex.toml
    assert_tree_parity migrate -y --apply
    rm -rf .agent .ai/src/hooks .ai/src/settings .ai/src/mcp
    mkdir -p .ai/src/skills
    cp -R "$REPO_ROOT/lib/templates/base-src/skills/agentsync" .ai/src/skills/agentsync
    printf 'tools:\n  enabled:\n    - claude\n' > .ai/agent_sync.yaml
    assert_tree_parity migrate --legacy
    assert_tree_parity migrate --apply
    printf 'tools:\n  enabled: []\nsource:\n  tools: "../elsewhere"\n' > .ai/agent_sync.yaml
    mkdir -p .ai/src/hooks
    printf '{}\n' > .ai/src/hooks/claude.json
    assert_tree_parity migrate --legacy --apply
}
```

Run: `bats --tap -f 'migrate prints' tests/native_parity.bats`
Expected: `ok` (the native side still runs Bash).

- [x] **Step 2: Write the failing tests**

Add to `src/catalog.rs`'s tests module:

```rust
    #[test]
    fn the_engine_owns_the_agentsync_skill_and_ships_the_migrate_prompt() {
        assert_eq!(base_src_skills(), ["agentsync"]);
        assert!(MIGRATE_PROMPT.starts_with("I need you to safely migrate"));
    }

```

Create `src/cli/migrate.rs` with its tests module only; add `pub mod migrate;` to `src/cli/mod.rs`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

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
        asked: Vec<String>,
    }

    fn call(
        root: &str,
        args: &[&str],
        interactive: bool,
        answer: bool,
        copied: Option<i32>,
    ) -> Outcome {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let mut asked = Vec::new();
        let mut confirm = |question: &str, _default: bool| {
            asked.push(question.to_string());
            answer
        };
        let mut copy = |_: &str| copied;
        let mut env = Env {
            version: "9.9.9",
            prompt_root: root.to_string(),
            no_clipboard: false,
            stdout_tty: false,
            interactive,
            confirm: &mut confirm,
            copy: &mut copy,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = migrate(
            &args,
            &discover,
            &Style::plain(),
            &mut env,
            &mut out,
            &mut err,
        )
        .unwrap();
        Outcome {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
            asked,
        }
    }

    fn tree(root: &str) -> Vec<String> {
        let mut files = Vec::new();
        files_below(Path::new(root), &mut files);
        let mut rels: Vec<String> = files
            .iter()
            .map(|f| f.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect();
        rels.sort();
        rels
    }

    #[test]
    fn the_prompt_names_both_versions_and_reports_the_clipboard_like_bash() {
        let (_dir, root) = project(&[(".ai/agent_sync.yaml", "agentsync_version: \"0.7.0\"\n")]);
        let copied = call(&root, &[], false, false, Some(0));
        assert_eq!(copied.status, 0);
        assert!(copied.out.starts_with(
            "## AgentSync migration context\n\n- AgentSync CLI that generated this prompt: 9.9.9\n- Project-pinned AgentSync version: 0.7.0\n\n---\n\nI need you to safely migrate"
        ));
        assert!(copied.out.ends_with(&format!(
            "{}\n",
            catalog::MIGRATE_PROMPT.trim_end_matches('\n')
        )));
        assert_eq!(copied.err, "  Copied migration prompt to clipboard.\n");
        assert_eq!(
            call(&root, &[], false, false, None).err,
            "  Clipboard tool not found. Prompt was printed to stdout.\n"
        );
        assert_eq!(
            call(&root, &[], false, false, Some(7)).err,
            "  Could not copy to clipboard. Prompt was printed to stdout.\n"
        );
        std::fs::remove_file(Path::new(&root).join(".ai/agent_sync.yaml")).unwrap();
        assert!(
            call(&root, &[], false, false, Some(0))
                .out
                .contains("- Project-pinned AgentSync version: not detected\n")
        );
    }

    #[test]
    fn arguments_are_refused_with_the_bash_statuses() {
        let (_dir, root) = project(&[(".ai/agent_sync.yaml", "format: 2\n")]);
        let bogus = call(&root, &["--bogus", "extra"], false, false, None);
        assert_eq!(
            (bogus.status, bogus.out.as_str(), bogus.err.as_str()),
            (
                1,
                "",
                "Error: Unknown flag: --bogus\nUsage: agentsync migrate [--legacy [--apply] [--yes]]\n"
            )
        );
        let legacy = call(&root, &["--legacy", "--bogus"], false, false, None);
        assert_eq!(
            (legacy.status, legacy.err.as_str()),
            (
                1,
                "Error: Unknown flag: --bogus\nUsage: agentsync migrate --legacy [--apply] [--yes]\n"
            )
        );
        assert!(
            call(&root, &["-h"], false, false, None)
                .out
                .starts_with("Usage: agentsync migrate\n")
        );
        assert!(
            call(&root, &["--legacy", "--help"], false, false, None)
                .out
                .starts_with(
                    "Usage: agentsync migrate --legacy [--apply] [--yes]\n\n  Moves legacy"
                )
        );
        assert_eq!(
            call(&root, &["--apply"], false, false, None).out,
            format!(
                "\n  AgentSync Migrate\n  {root}\n\n  Nothing to migrate.\n  Canonical layout, no engine-owned skill copies, format r2 is current.\n\n"
            )
        );
    }

    const LEGACY: [(&str, &str); 9] = [
        (
            ".ai/agent_sync.yaml",
            "format: 2\ntools:\n  enabled:\n    - claude\n",
        ),
        (".ai/src/hooks/cursor.json", "{\"hooks\": {}}\n"),
        (".ai/src/tools/cursor/settings.json", "{\"taken\": true}\n"),
        (".ai/src/settings/cursor.json", "{\"s\": 1}\n"),
        (".ai/src/settings/claude.json", "{\"s\": 2}\n"),
        (".ai/src/settings/README", "noext\n"),
        (".ai/src/mcp/claude.json", "{\"mcpServers\": {}}\n"),
        (".ai/src/mcp/cursor.json", "{\"mcpServers\": {}}\n"),
        (".agent/AGENTS.md", "# old\n"),
    ];

    #[test]
    fn legacy_files_are_planned_moved_consolidated_and_skipped_like_bash() {
        let (_dir, root) = project(&LEGACY);
        let header = format!(
            "\n  AgentSync Migrate\n  {root}\n\n  Legacy pre-v0.6 layout:\n    .agent/ — orphan directory from before tool-specific outputs.\n      · AGENTS.md\n\n"
        );
        let plan = "  Planned moves:\n  .ai/src/hooks/cursor.json  →  .ai/src/tools/cursor/hooks.json\n  .ai/src/settings/README  →  .ai/src/tools/README/settings.README\n  .ai/src/settings/claude.json  →  .ai/src/tools/claude/settings.json\n  .ai/src/settings/cursor.json  →  .ai/src/tools/cursor/settings.json\n\n  MCP consolidation:\n    All 2 .ai/src/mcp/*.json are byte-identical — can consolidate into .ai/src/mcp.json.\n    Source file: .ai/src/mcp/claude.json\n\n";

        let dry = call(&root, &["--legacy"], false, false, None);
        assert_eq!(
            dry.out,
            format!(
                "{header}  Dry-run. Re-run with agentsync migrate --apply to remove .agent/.\n\n{plan}  Dry-run. Re-run with agentsync migrate --apply to move files.\n\n"
            )
        );
        assert_eq!(tree(&root).len(), LEGACY.len());

        let applied = call(&root, &["--apply", "--yes"], false, false, None);
        assert_eq!(
            applied.out,
            format!(
                "{header}  removed .agent/ (pre-v0.6 layout)\n{plan}  consolidated .ai/src/mcp/claude.json → .ai/src/mcp.json\n  consolidated .ai/src/mcp/cursor.json → .ai/src/mcp.json\n  moved .ai/src/hooks/cursor.json → .ai/src/tools/cursor/hooks.json\n  moved .ai/src/settings/README → .ai/src/tools/README/settings.README\n  moved .ai/src/settings/claude.json → .ai/src/tools/claude/settings.json\n  skipped (target already exists) .ai/src/tools/cursor/settings.json\n\n  Migration complete.\n    moved:        5\n    skipped:      1 (target already existed)\n    consolidated: .ai/src/mcp.json\n\n  Run agentsync sync to confirm outputs are unchanged.\n\n"
            )
        );
        assert_eq!(
            tree(&root),
            [
                ".ai/agent_sync.yaml",
                ".ai/src/mcp.json",
                ".ai/src/settings/cursor.json",
                ".ai/src/tools/README/settings.README",
                ".ai/src/tools/claude/settings.json",
                ".ai/src/tools/cursor/hooks.json",
                ".ai/src/tools/cursor/settings.json",
            ]
        );
    }

    #[test]
    fn prompts_decide_the_agent_dir_and_the_consolidation_off_the_flags() {
        let (_dir, root) = project(&LEGACY);
        let quiet = call(&root, &["--apply"], false, true, None);
        assert!(quiet.asked.is_empty());
        assert!(quiet.out.contains(
            "  (non-interactive; .agent/ left in place — re-run with --yes to remove)\n\n"
        ));
        assert!(Path::new(&root).join(".agent/AGENTS.md").is_file());
        assert!(Path::new(&root).join(".ai/src/mcp.json").is_file());

        let (_dir, root) = project(&LEGACY);
        let declined = call(&root, &["--apply"], true, false, None);
        assert_eq!(
            declined.asked,
            [
                "Remove .agent/ (review the listing above first)?",
                "Consolidate 2 identical MCP files into .ai/src/mcp.json?"
            ]
        );
        assert!(Path::new(&root).join(".agent/AGENTS.md").is_file());
        assert!(
            declined
                .out
                .contains("  moved .ai/src/mcp/claude.json → .ai/src/tools/claude/mcp.json\n")
        );
        assert!(!Path::new(&root).join(".ai/src/mcp.json").exists());
    }

    #[test]
    fn a_json_mcp_set_with_another_config_moves_per_tool_and_an_outside_catalog_is_refused() {
        let (_dir, root) = project(&[
            (".ai/agent_sync.yaml", "format: 2\n"),
            (".ai/src/mcp/claude.json", "{}\n"),
            (".ai/src/mcp/cursor.json", "{}\n"),
            (".ai/src/mcp/codex.toml", "[x]\n"),
        ]);
        let run = call(&root, &["--apply", "--yes"], false, false, None);
        assert!(
            run.out
                .contains("  moved .ai/src/mcp/codex.toml → .ai/src/tools/codex/mcp.toml\n")
        );
        assert!(
            Path::new(&root)
                .join(".ai/src/tools/codex/mcp.toml")
                .is_file()
        );

        let (_dir, root) = project(&[
            (
                ".ai/agent_sync.yaml",
                "tools:\n  enabled: []\nsource:\n  tools: \"../elsewhere\"\n",
            ),
            (".ai/src/hooks/claude.json", "{}\n"),
        ]);
        let refused = call(&root, &["--legacy", "--apply"], false, false, None);
        assert_eq!(refused.status, 1);
        assert_eq!(refused.out, format!("\n  AgentSync Migrate\n  {root}\n\n"));
        assert_eq!(
            refused.err,
            format!(
                "Error: source.tools resolves outside the project: {root}/../elsewhere\nAgentSync only reads that catalog; edit its tool overrides where they live.\n"
            )
        );
        assert!(Path::new(&root).join(".ai/src/hooks/claude.json").is_file());
    }

    #[test]
    fn engine_owned_skill_copies_are_retired_unless_edited_and_the_format_is_recorded() {
        let skill = |rel: &str| {
            catalog::engine_files()
                .into_iter()
                .find(|(path, _)| path == &format!("lib/templates/base-src/skills/agentsync/{rel}"))
                .map(|(_, bytes)| String::from_utf8(bytes.to_vec()).unwrap())
                .unwrap()
        };
        let files = [
            "SKILL.md",
            "references/maintenance.md",
            "references/writing-skills.md",
        ];
        let copies: Vec<(String, String)> = files
            .iter()
            .map(|rel| (format!(".ai/src/skills/agentsync/{rel}"), skill(rel)))
            .collect();
        let manifest: String = files
            .iter()
            .map(|rel| {
                format!(
                    "skills/agentsync/{rel}\t{}\n",
                    crate::manifest::sha256_hex(skill(rel).as_bytes())
                )
            })
            .chain(["rules/core.md\tabc\n".to_string()])
            .collect();
        let mut fixture: Vec<(&str, &str)> = copies
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        fixture.push((".ai/agent_sync.yaml", "tools:\n  enabled:\n    - claude\n"));
        fixture.push((".ai/.template-manifest", &manifest));

        let (_dir, root) = project(&fixture);
        let dry = call(&root, &["--legacy"], false, false, None);
        assert_eq!(
            dry.out,
            format!(
                "\n  AgentSync Migrate\n  {root}\n\n  Engine-owned skills:\n  would remove  .ai/src/skills/agentsync/ (unedited — the engine supplies it)\n\n  Project format r1 → r2:\n  would set     format: 2 in .ai/agent_sync.yaml\n\n  Dry-run — re-run with agentsync migrate --apply to apply.\n\n"
            )
        );
        let applied = call(&root, &["--apply"], false, false, None);
        assert!(
            applied.out.contains(
                "  removed       .ai/src/skills/agentsync/ (the engine supplies it now)\n"
            )
        );
        assert!(applied.out.ends_with(
            "  set           format: 2 in .ai/agent_sync.yaml\n\n  Migration complete.\n\n"
        ));
        assert_eq!(
            tree(&root),
            [".ai/.template-manifest", ".ai/agent_sync.yaml"]
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/.template-manifest")).unwrap(),
            "rules/core.md\tabc\n"
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/agent_sync.yaml")).unwrap(),
            "tools:\n  enabled:\n    - claude\nformat: 2\n"
        );

        let (_dir, root) = project(&fixture);
        std::fs::write(
            Path::new(&root).join(".ai/src/skills/agentsync/SKILL.md"),
            "edited\n",
        )
        .unwrap();
        let kept = call(&root, &["--apply"], false, false, None);
        assert!(kept.out.contains("  keep          .ai/src/skills/agentsync/ (edited — stays your override; delete it to follow the engine)\n"));
        assert!(
            Path::new(&root)
                .join(".ai/src/skills/agentsync/SKILL.md")
                .is_file()
        );
    }
}
```

Run: `cargo test --lib 2>&1 | grep -E '^error' | sort -u | head -8`
Expected: compile errors naming the missing `migrate`, `Env`, `files_below`, `MIGRATE_PROMPT`, and `base_src_skills`.

- [x] **Step 3: Write the implementation**

In `src/catalog.rs`, before `GLOBAL_CONFIG`:

```rust
/// `lib/prompts/migrate.md`, the upgrade prompt `agentsync migrate` prints.
pub const MIGRATE_PROMPT: &str = include_str!("../lib/prompts/migrate.md");

/// The engine-owned skills under `lib/templates/base-src/skills/`, in byte order.
pub fn base_src_skills() -> Vec<String> {
    TEMPLATES
        .get_dir("base-src/skills")
        .into_iter()
        .flat_map(|dir| dir.dirs())
        .filter_map(|dir| Some(dir.path().file_name()?.to_str()?.to_string()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
```

Prepend to `src/cli/migrate.rs`:

```rust
//! `agentsync migrate`: `cmd_migrate` of `lib/helpers/migrate.sh`, which prints
//! an upgrade prompt or, with `--legacy`, retires pre-0.11 layouts.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::customize::put;
use crate::project::Project;
use crate::style::Style;
use crate::template_manifest::{self, TemplateManifest};
use crate::{Error, catalog, format_rev, yaml_edit, yaml_subset};

type Discover<'a> = &'a dyn Fn() -> Result<Project, Error>;

/// What `migrate` takes from the process and the terminal.
pub struct Env<'a> {
    pub version: &'a str,
    /// `${AGENTSYNC_REPO_ROOT:-$(pwd)}` as the prompt reads it, unchecked.
    pub prompt_root: String,
    pub no_clipboard: bool,
    pub stdout_tty: bool,
    pub interactive: bool,
    pub confirm: &'a mut dyn FnMut(&str, bool) -> bool,
    /// The first clipboard tool's exit status, `None` when none is installed.
    pub copy: &'a mut dyn FnMut(&str) -> Option<i32>,
}

pub fn migrate(
    args: &[String],
    discover: Discover,
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    match args.first().map(String::as_str) {
        Some("--legacy") => legacy(&args[1..], discover, style, env, out, err),
        Some("--apply" | "--yes" | "-y") => legacy(args, discover, style, env, out, err),
        _ => prompt(args, style, env, out, err),
    }
}

/// `_cmd_migrate_prompt`.
fn prompt(
    args: &[String],
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            put(
                out,
                b"Usage: agentsync migrate\n       agentsync migrate --legacy [--apply] [--yes]\n\n  Prints an AI prompt for safely upgrading an existing AgentSync project to\n  the latest documented format and copies it to the system clipboard.\n\n  Legacy layout maintenance:\n    --legacy   Preview old flat-layout file moves without changing files\n    --apply    Apply those moves (backwards-compatible historical behavior)\n    --yes, -y  Accept safe legacy consolidation without prompting\n",
            )?;
            return Ok(0);
        }
        Some(flag) => {
            put(
                err,
                format!(
                    "{}: Unknown flag: {flag}\nUsage: agentsync migrate [--legacy [--apply] [--yes]]\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(1);
        }
        None => {}
    }

    let text = format!(
        "## AgentSync migration context\n\n- AgentSync CLI that generated this prompt: {}\n- Project-pinned AgentSync version: {}\n\n---\n\n{}",
        env.version,
        project_version(&env.prompt_root),
        catalog::MIGRATE_PROMPT.trim_end_matches('\n')
    );
    let status = if env.no_clipboard {
        Some(3)
    } else {
        match (env.copy)(&text) {
            None => Some(2),
            Some(0) => Some(0),
            Some(_) => None,
        }
    };

    if env.stdout_tty {
        put(
            err,
            format!(
                "\n  {}\n\n",
                style.dim("─── migration prompt below ────────────────────────────────")
            )
            .as_bytes(),
        )?;
    }
    put(out, format!("{text}\n").as_bytes())?;
    if env.stdout_tty {
        put(
            err,
            format!(
                "\n  {}\n\n",
                style.dim("─── end of migration prompt ──────────────────────────────")
            )
            .as_bytes(),
        )?;
    }
    let notice = match status {
        Some(0) => format!(
            "  {}\n",
            style.green("Copied migration prompt to clipboard.")
        ),
        Some(2) => format!(
            "  {} Prompt was printed to stdout.\n",
            style.yellow("Clipboard tool not found.")
        ),
        Some(_) => String::new(),
        None => format!(
            "  {} Prompt was printed to stdout.\n",
            style.yellow("Could not copy to clipboard.")
        ),
    };
    put(err, notice.as_bytes())?;
    Ok(0)
}

/// `_migrate_project_version`.
fn project_version(root: &str) -> String {
    let pinned = [
        format!("{root}/.ai/agent_sync.yaml"),
        format!("{root}/agent_sync.yaml"),
    ]
    .into_iter()
    .find(|path| Path::new(path).is_file())
    .and_then(|path| std::fs::read(path).ok())
    .map(|bytes| yaml_subset::value(&String::from_utf8_lossy(&bytes), "agentsync_version"))
    .unwrap_or_default();
    if pinned.is_empty() {
        "not detected".to_string()
    } else {
        pinned
    }
}

/// `_migrate_copy_prompt`'s tool search and pipe: `None` when no clipboard
/// tool is on `PATH`, else its exit status.
pub fn copy_to_clipboard(text: &str, path_var: Option<&str>) -> Option<i32> {
    let tools: [(&str, &[&str]); 5] = [
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
        ("clip.exe", &[]),
    ];
    let (program, args) = tools
        .iter()
        .find_map(|(name, args)| on_path(name, path_var).map(|path| (path, *args)))?;
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
    else {
        return Some(126);
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    Some(child.wait().ok().and_then(|s| s.code()).unwrap_or(1))
}

/// `command -v <name>` over `PATH`.
fn on_path(name: &str, path_var: Option<&str>) -> Option<PathBuf> {
    path_var?
        .split(':')
        .filter(|dir| !dir.is_empty())
        .find_map(|dir| {
            let candidate = Path::new(dir).join(name);
            is_executable(&candidate).then_some(candidate)
        })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// One `resource|tool|path|ext` line of `_migrate_scan_legacy`.
struct Legacy {
    resource: &'static str,
    tool: String,
    src: PathBuf,
    ext: String,
}

struct Run<'a, 'b> {
    project: &'a Project,
    root: String,
    style: &'a Style,
    env: &'a mut Env<'b>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl Run<'_, '_> {
    fn rel(&self, path: &Path) -> String {
        let text = path.to_string_lossy();
        text.strip_prefix(&format!("{}/", self.root))
            .unwrap_or(&text)
            .to_string()
    }

    fn dest(&self, entry: &Legacy) -> PathBuf {
        self.project
            .user_tools_dir()
            .join(&entry.tool)
            .join(format!("{}.{}", entry.resource, entry.ext))
    }

    fn say(&mut self, text: &str) -> Result<(), Error> {
        put(self.out, text.as_bytes())
    }
}

/// Non-hidden entries of a directory in byte order, as `printf '%s\0' dir/* | LC_ALL=C sort -z`.
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

/// `_migrate_scan_legacy`.
fn scan_legacy(root: &Path) -> Vec<Legacy> {
    let mut found = Vec::new();
    for resource in ["hooks", "mcp", "settings"] {
        for file in sorted_entries(&root.join(".ai/src").join(resource)) {
            if !file.is_file() {
                continue;
            }
            let base = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let (tool, ext) = match base.rfind('.') {
                Some(dot) => (base[..dot].to_string(), base[dot + 1..].to_string()),
                None => (base.clone(), base.clone()),
            };
            if tool.is_empty() || ext.is_empty() {
                continue;
            }
            found.push(Legacy {
                resource,
                tool,
                src: file,
                ext,
            });
        }
    }
    found
}

/// `_migrate_mcp_consolidation_candidate`.
fn consolidation_candidate(root: &Path) -> Option<PathBuf> {
    let dir = root.join(".ai/src/mcp");
    if !dir.is_dir() {
        return None;
    }
    let mut files = Vec::new();
    for file in sorted_entries(&dir) {
        if !file.is_file() {
            continue;
        }
        if file.extension().is_none_or(|ext| ext != "json") {
            return None;
        }
        files.push(file);
    }
    let first = files.first()?.clone();
    if root.join(".ai/src/mcp.json").is_file() {
        return None;
    }
    let bytes = std::fs::read(&first).ok()?;
    files[1..]
        .iter()
        .all(|other| std::fs::read(other).is_ok_and(|b| b == bytes))
        .then_some(first)
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

/// `_migrate_scan_base_skills`: each engine-owned skill the project copies, and
/// whether every file still matches its recorded template hash.
fn scan_base_skills(root: &Path) -> Result<Vec<(String, bool)>, Error> {
    let manifest = TemplateManifest::load(root)?;
    let src = root.join(".ai/src");
    let mut copies = Vec::new();
    for name in catalog::base_src_skills() {
        let copy = src.join("skills").join(&name);
        if !copy.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        files_below(&copy, &mut files);
        let edited = files.iter().any(|file| {
            let rel = file
                .strip_prefix(&src)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let current = template_manifest::hash(file).unwrap_or_default();
            manifest
                .lookup(&rel)
                .is_none_or(|recorded| recorded != current)
        });
        copies.push((name, edited));
    }
    Ok(copies)
}

/// `cp <src> <dst>` onto a missing destination: the source's mode under the umask.
fn copy_new(src: &Path, dst: &Path) -> Result<(), Error> {
    let bytes = std::fs::read(src).map_err(|e| Error::io(src, e))?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = std::fs::metadata(src)
            .map_err(|e| Error::io(src, e))?
            .permissions()
            .mode();
        options.mode(mode & 0o7777);
    }
    options
        .open(dst)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|e| Error::io(dst, e))
}

/// `_cmd_migrate_legacy`.
fn legacy(
    args: &[String],
    discover: Discover,
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (mut apply, mut yes) = (false, false);
    for arg in args {
        match arg.as_str() {
            "--apply" => apply = true,
            "--yes" | "-y" => yes = true,
            "--help" | "-h" => {
                put(
                    out,
                    "Usage: agentsync migrate --legacy [--apply] [--yes]\n\n  Moves legacy flat-layout overrides to the canonical per-tool layout:\n    .ai/src/hooks/<tool>.<ext>    → .ai/src/tools/<tool>/hooks.<ext>\n    .ai/src/mcp/<tool>.<ext>      → .ai/src/tools/<tool>/mcp.<ext>\n    .ai/src/settings/<tool>.<ext> → .ai/src/tools/<tool>/settings.<ext>\n\n  When every legacy MCP file is byte-identical, migrate offers to consolidate\n  them into the shared .ai/src/mcp.json. Pass --yes to accept without prompt.\n\n  Dry-run by default — re-run with --apply to move files.\n".as_bytes(),
                )?;
                return Ok(0);
            }
            flag => {
                put(
                    err,
                    format!(
                        "{}: Unknown flag: {flag}\nUsage: agentsync migrate --legacy [--apply] [--yes]\n",
                        style.red("Error")
                    )
                    .as_bytes(),
                )?;
                return Ok(1);
            }
        }
    }

    let project = discover()?;
    let root_path = project.root.clone();
    let mut run = Run {
        project: &project,
        root: root_path.to_string_lossy().into_owned(),
        style,
        env,
        out,
        err,
    };

    let legacy = scan_legacy(&root_path);
    let agent_dir = root_path.join(".agent");
    let has_agent_dir = agent_dir.is_dir();
    let skills = scan_base_skills(&root_path)?;
    let engine_rev = format_rev::engine();
    let config = match &project.config_path {
        Some(path) => Some(
            std::fs::read(path)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|e| Error::io(path, e))?,
        ),
        None => None,
    };
    let current_rev = config.as_deref().map_or(1, format_rev::project);

    run.say(&format!(
        "\n{}\n{}\n\n",
        style.bold("  AgentSync Migrate"),
        style.dim(&format!("  {}", run.root))
    ))?;

    if legacy.is_empty() && !has_agent_dir && skills.is_empty() && current_rev >= engine_rev {
        run.say(&format!(
            "{}\n{}\n\n",
            style.green("  Nothing to migrate."),
            style.dim(&format!(
                "  Canonical layout, no engine-owned skill copies, format r{current_rev} is current."
            ))
        ))?;
        return Ok(0);
    }

    if apply && !legacy.is_empty() && !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, run.err);
    }

    if !skills.is_empty() {
        run.say(&format!("  {}:\n", style.bold("Engine-owned skills")))?;
        retire_base_skills(&mut run, apply, &skills)?;
        run.say("\n")?;
    }

    if current_rev < engine_rev {
        run.say(&format!(
            "  {} {}:\n",
            style.bold("Project format"),
            style.dim(&format!("r{current_rev} → r{engine_rev}"))
        ))?;
        if let Some(path) = &project.config_path {
            let shown = run.rel(path);
            if apply {
                yaml_edit::set_scalar(path, "format", &engine_rev.to_string())?;
                run.say(&format!(
                    "{}           format: {engine_rev} {}\n",
                    style.green("  set"),
                    style.dim(&format!("in {shown}"))
                ))?;
            } else {
                run.say(&format!(
                    "{}     format: {engine_rev} {}\n",
                    style.cyan("  would set"),
                    style.dim(&format!("in {shown}"))
                ))?;
            }
        }
        run.say("\n")?;
    }

    if legacy.is_empty() && !has_agent_dir {
        if apply {
            run.say(&format!("{}\n\n", style.green("  Migration complete.")))?;
        } else {
            run.say(&format!(
                "{} {}{}\n\n",
                style.dim("  Dry-run — re-run with"),
                style.cyan("agentsync migrate --apply"),
                style.dim(" to apply.")
            ))?;
        }
        return Ok(0);
    }

    if has_agent_dir {
        let mut listing = format!(
            "  {}:\n    {} — orphan directory from before tool-specific outputs.\n",
            style.bold("Legacy pre-v0.6 layout"),
            style.yellow(".agent/")
        );
        for item in sorted_entries(&agent_dir) {
            if item.exists() {
                listing.push_str(&format!(
                    "      · {}\n",
                    item.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
        }
        listing.push('\n');
        run.say(&listing)?;

        if apply {
            let remove = if yes {
                true
            } else if run.env.interactive {
                (run.env.confirm)("Remove .agent/ (review the listing above first)?", false)
            } else {
                run.say(&format!(
                    "  {}\n\n",
                    style.dim(
                        "(non-interactive; .agent/ left in place — re-run with --yes to remove)"
                    )
                ))?;
                false
            };
            if remove && agent_dir.is_dir() {
                std::fs::remove_dir_all(&agent_dir).map_err(|e| Error::io(&agent_dir, e))?;
                run.say(&format!(
                    "{}\n",
                    style.green("  removed .agent/ (pre-v0.6 layout)")
                ))?;
            }
        } else {
            run.say(&format!(
                "{} {}{}\n\n",
                style.dim("  Dry-run. Re-run with"),
                style.cyan("agentsync migrate --apply"),
                style.dim(" to remove .agent/.")
            ))?;
        }
        if legacy.is_empty() {
            return Ok(0);
        }
    }

    let (mcp, other): (Vec<&Legacy>, Vec<&Legacy>) =
        legacy.iter().partition(|entry| entry.resource == "mcp");
    let candidate = consolidation_candidate(&root_path);

    let mut plan = format!("  {}:\n", style.bold("Planned moves"));
    let move_line = |run: &Run, entry: &Legacy| {
        format!(
            "  {}  →  {}\n",
            run.rel(&entry.src),
            run.rel(&run.dest(entry))
        )
    };
    for entry in &other {
        plan.push_str(&move_line(&run, entry));
    }
    if let Some(first) = &candidate {
        plan.push_str(&format!(
            "\n  {}:\n    All {} .ai/src/mcp/*.json are byte-identical — can consolidate into .ai/src/mcp.json.\n    {} {}\n",
            style.bold("MCP consolidation"),
            mcp.len(),
            style.dim("Source file:"),
            run.rel(first)
        ));
    } else {
        for entry in &mcp {
            plan.push_str(&move_line(&run, entry));
        }
    }
    plan.push('\n');
    run.say(&plan)?;

    if !apply {
        run.say(&format!(
            "{} {}{}\n\n",
            style.dim("  Dry-run. Re-run with"),
            style.cyan("agentsync migrate --apply"),
            style.dim(" to move files.")
        ))?;
        return Ok(0);
    }

    let (mut applied, mut skipped, mut consolidated) = (0, 0, false);
    let mut count = |moved: bool| {
        if moved {
            applied += 1;
        } else {
            skipped += 1;
        }
    };

    if let Some(first) = &candidate {
        let consolidate = if yes {
            true
        } else if run.env.interactive {
            let question = format!(
                "Consolidate {} identical MCP files into .ai/src/mcp.json?",
                mcp.len()
            );
            (run.env.confirm)(&question, true)
        } else {
            true
        };
        if consolidate {
            copy_new(first, &root_path.join(".ai/src/mcp.json"))?;
            for entry in &mcp {
                match std::fs::remove_file(&entry.src) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        return Err(Error::io(&entry.src, e));
                    }
                    _ => {}
                }
                let text = format!(
                    "{} {} → .ai/src/mcp.json\n",
                    style.green("  consolidated"),
                    run.rel(&entry.src)
                );
                run.say(&text)?;
            }
            for _ in &mcp {
                count(true);
            }
            consolidated = true;
        } else {
            for entry in &mcp {
                count(move_one(&mut run, entry)?);
            }
        }
    } else {
        for entry in &mcp {
            count(move_one(&mut run, entry)?);
        }
    }
    for entry in &other {
        count(move_one(&mut run, entry)?);
    }

    for dir in ["hooks", "mcp", "settings"] {
        let _ = std::fs::remove_dir(root_path.join(".ai/src").join(dir));
    }

    let mut summary = format!(
        "\n{}\n{}\n",
        style.green("  Migration complete."),
        style.dim(&format!("    moved:        {applied}"))
    );
    if skipped > 0 {
        summary.push_str(&format!(
            "{}\n",
            style.yellow(&format!(
                "    skipped:      {skipped} (target already existed)"
            ))
        ));
    }
    if consolidated {
        summary.push_str(&format!(
            "{}\n",
            style.dim("    consolidated: .ai/src/mcp.json")
        ));
    }
    summary.push_str(&format!(
        "\n{} {}{}\n\n",
        style.dim("  Run"),
        style.cyan("agentsync sync"),
        style.dim(" to confirm outputs are unchanged.")
    ));
    run.say(&summary)?;
    Ok(0)
}

/// `_migrate_move_one`: whether the file moved; an existing target is skipped.
fn move_one(run: &mut Run, entry: &Legacy) -> Result<bool, Error> {
    let dest = run.dest(entry);
    if dest.is_file() {
        let text = format!(
            "{} {}\n",
            run.style.yellow("  skipped (target already exists)"),
            run.rel(&dest)
        );
        run.say(&text)?;
        return Ok(false);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::rename(&entry.src, &dest).map_err(|e| Error::io(&entry.src, e))?;
    let text = format!(
        "{} {} → {}\n",
        run.style.green("  moved"),
        run.rel(&entry.src),
        run.rel(&dest)
    );
    run.say(&text)?;
    Ok(true)
}

/// `_migrate_retire_base_skills`.
fn retire_base_skills(run: &mut Run, apply: bool, skills: &[(String, bool)]) -> Result<(), Error> {
    let style = run.style;
    let root = PathBuf::from(&run.root);
    let src = root.join(".ai/src");
    let mut manifest = TemplateManifest::load(&root)?;
    let mut removed = 0;
    for (name, edited) in skills {
        if *edited {
            run.say(&format!(
                "{}          .ai/src/skills/{name}/ {}\n",
                style.yellow("  keep"),
                style.dim("(edited — stays your override; delete it to follow the engine)")
            ))?;
            continue;
        }
        if !apply {
            run.say(&format!(
                "{}  .ai/src/skills/{name}/ {}\n",
                style.cyan("  would remove"),
                style.dim("(unedited — the engine supplies it)")
            ))?;
            continue;
        }
        let copy = src.join("skills").join(name);
        let mut files = Vec::new();
        files_below(&copy, &mut files);
        for file in files {
            if let Ok(rel) = file.strip_prefix(&src) {
                manifest.remove(&rel.to_string_lossy());
            }
        }
        std::fs::remove_dir_all(&copy).map_err(|e| Error::io(&copy, e))?;
        run.say(&format!(
            "{}       .ai/src/skills/{name}/ {}\n",
            style.green("  removed"),
            style.dim("(the engine supplies it now)")
        ))?;
        removed += 1;
    }
    if apply && removed > 0 {
        manifest.write(&root)?;
    }
    Ok(())
}

```

In `src/main.rs`, before the `upgrade-config` block:

```rust
    if args.first().and_then(|a| a.to_str()) == Some("migrate") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let prompt_root = match var("AGENTSYNC_REPO_ROOT").filter(|root| !root.is_empty()) {
            Some(root) => root,
            None => {
                let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
                paths::logical_root(None, &cwd, var("PWD").as_deref())
            }
        };
        let path_var = var("PATH");
        let mut env = cli::migrate::Env {
            version: engine_version(),
            prompt_root,
            no_clipboard: var("AGENTSYNC_NO_CLIPBOARD").as_deref() == Some("1"),
            stdout_tty: std::io::stdout().is_terminal(),
            interactive: prompts::is_tty(),
            confirm: &mut |question: &str, default_yes: bool| {
                prompts::confirm(question, default_yes)
            },
            copy: &mut |text: &str| cli::migrate::copy_to_clipboard(text, path_var.as_deref()),
        };
        return cli::migrate::migrate(
            &rest,
            &Project::discover,
            &Style::for_stdout(),
            &mut env,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
```

In `bin/agentsync.sh:280` append `migrate`.

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in migrate format_migration doctor; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
bats --tap -f 'migrate prints' tests/native_parity.bats
```

Expected: `232 passed`, `0`, `11`, `1`; `0` for each file; `ok`.

- [x] **Step 5: Prove the fixture bites, check the terminal and the clipboard, lint, commit**

Change `"  consolidated"` to `"  folded"` in `src/cli/migrate.rs`, rebuild, rerun the fixture: `not ok` with both consolidation lines in the diff; revert and rebuild.

Point `scratchpad/phase4g/migrate_tty.sh` and `migrate_reference.sh` at this tree (engine `E`/`R` and `AGENTSYNC_NATIVE_BIN`), run each with `0` and `1`, and diff: identical transcripts, including the `pbcopy` shim's success and failure. Run `mode_probe.sh` the same way: `mcp.json=644` in both engines.

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/migrate.rs src/catalog.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-15-rust-migration-phase-4g-migrate.md
git commit -m "feat(native): port migrate"
```

---

### Task 6: Verify, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [x] **Step 1: Spec**

Append to "Known quirks":

```markdown
35. `migrate --apply --yes` prints `removed .agent/ (pre-v0.6 layout)` with no
    blank line before `Planned moves:`.
36. A legacy file without an extension, such as `.ai/src/settings/README`, moves
    to `.ai/src/tools/README/settings.README`.
37. Off a terminal without `--yes`, `migrate --apply` consolidates identical MCP
    files but leaves `.agent/` in place.
```

Append to "Accepted deviations":

```markdown
- Phase 4g: `migrate` prints the embedded `lib/prompts/migrate.md` where Bash
  read the install directory's copy.
```

- [x] **Step 2: Module map and outputs**

Set the `lib/helpers/migrate.sh` row to `→ src/cli/migrate.rs      Phase 4g, ported`, the `lib/helpers/format.sh` row to `→ src/format_rev.rs       engine and project revision (Phase 4g); pending notes wait for doctor`, and the `lib/helpers/template_manifest.sh` row to `→ src/template_manifest.rs   hash (4e); load, lookup, remove, write (4g); record and heal wait for refresh and init`. Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [x] **Step 3: Verify (outside the agent sandbox)**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in migrate format_migration doctor native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `232 passed`, `0`, `11`, `1`; lint exit 0; every line `bash=0 native=0`.

- [x] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-15-rust-migration-phase-4g-migrate.md
git commit -m "docs(native): map the phase 4g modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, the files in Task 6 are green under both engines, the parity fixture passes, and a `## Completion receipt` records the fresh verification. The next plan is 4h, `refresh`.

## Completion receipt

### Decisions the review took

Both as recommended, on 2026-09-15, under `/decide`.

### Global Constraints

| Constraint | Satisfied by |
|---|---|
| The prompt writes only the clipboard; `--legacy --apply` moves, consolidates, removes, and sets only what Bash does; a dry run writes nothing | `src/cli/migrate.rs`; the parity fixture compares whole trees after every call |
| Three Bash changes, each in its own commit with a regression test | `48309a6`, `7015b8c`, `5a12867`, adding four tests to `tests/migrate.bats`; `git diff --stat e344902..HEAD -- lib bin` names only `lib/helpers/migrate.sh` and `bin/agentsync.sh`; ShellCheck exit 0 |
| Byte-for-byte parity | `tests/native_parity.bats`: `parity: migrate prints the prompt and retires legacy layouts like Bash`; identical reference, pty, and mode transcripts |
| `unsafe_code = "forbid"`, fmt and clippy clean, no new dependency; `main.rs` alone reads the environment and terminal state | `Cargo.toml` unchanged; `PATH`, `AGENTSYNC_NO_CLIPBOARD`, `AGENTSYNC_REPO_ROOT`, and the terminal checks are read in `src/main.rs` |
| Disk-touching unit tests are `#[cfg(unix)]` | `src/cli/migrate.rs` and `src/template_manifest.rs` test modules |
| Nothing touches the developer's clipboard | every run set `AGENTSYNC_NO_CLIPBOARD=1` or put the `pbcopy` shim first on `PATH`; unit tests inject the copy closure |
| Expected values from the fixed Bash | `cmp_bash.out`, `migrate_tty.sh`, `no_clipboard_reference.sh`, `format_reference.sh`, `tm_reference.sh`, `mode_probe.sh` |
| Conventional Commits, at most 72 characters, no trailers | `48309a6` … the close commit |

### Fresh verification, 2026-09-15, macOS arm64, outside the agent sandbox

- `cargo test`: 232 passed (lib), 11 passed (cli), 1 passed (interrupt).
- `cargo clippy --all-targets -- -D warnings`: exit 0. `cargo fmt --all --check`: exit 0.
- `shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh`: exit 0.
- bats, one file at a time, `AGENTSYNC_NATIVE=0` / `=1` failures: `migrate` 0/0 (22 cases; the locale case ran), `format_migration` 0/0 (11), `doctor` 0/0 (36), `native_parity` 0/0 (52).
- Each Bash fix: its new tests failed before the fix and passed after; `migrate.sh` matches the verified copy byte for byte.
- Mutation: `"  consolidated"` → `"  folded"` failed the fixture with both consolidation lines; reverted, rebuilt.
- Against this tree: `migrate_reference.sh` gave identical 530-line transcripts for both engines, including the clipboard shim's success and failure; `migrate_tty.sh` gave identical 73-line transcripts (`.agent/` removal and consolidation, declined and accepted, and the prompt banners); `mode_probe.sh`: `mcp.json=644` in both.

### Skipped, deferred, open

- **`format_pending_notes`** stays in Bash until `doctor` is ported.
- **`clip.exe`, `wl-copy`, `xclip`, and `xsel`** are searched in Bash's order but only `pbcopy` was exercised; the Windows `PATH` separator is not handled, as native bats runs stay off Windows until Phase 5.
- **Full-suite runs** stay off on this machine.
- **Not pushed.**

## Run log

### 2026-09-15 — Phase 4g planned
- Commits: this plan.
- Verified: the three Bash fixes were applied to a copy of the engine: the four new tests failed on the committed `migrate.sh` and router and passed on the copy (`migrate.bats` 22/22), with `format_migration.bats` 11/11 and `doctor.bats` 36/36. The first placement of the `source.tools` refusal ran after the skill retirement and was moved before the first change when the reference showed a partial run. The Rust in Tasks 4–5 was drafted against that copy and removed from the tree: `cargo test` 232/0/11/1, clippy clean; `migrate_reference.sh` gave identical 530-line transcripts for the fixed Bash and the binary, including the `pbcopy` shim's success and failure, and a `Nothing to migrate.` mutation showed in the diff; with `migrate` in the copy's `_NATIVE_COMMANDS`, `migrate` 22/22, `format_migration` 11/11, and `doctor` 36/36 passed in both modes; the parity fixture passed and failed on a `consolidated` mutation; `migrate_tty.sh` transcripts (declined and accepted `.agent/` removal, declined and accepted consolidation, prompt banners) were identical; `mode_probe.sh` gave `mcp.json=644` in both engines.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.

### 2026-09-15 — Tasks 0–6 done, plan closed
- Commits: `48309a6 fix(migrate): keep non-JSON MCP overrides out of consolidation`; `7015b8c fix(migrate): move overrides into the tool override directory`; `5a12867 fix(migrate): list legacy files in byte order`; `238c94d feat(native): port the format revision and template manifest file`; `d856afe feat(native): port migrate`; `docs(native): map the phase 4g modules and quirks`.
- Verified: see the completion receipt; every file `bash=0 native=0`.
- Plan amended: none; every file landed byte for byte as the verified draft.
- Next: plan 4h, `refresh`.
- Blocker: none.
