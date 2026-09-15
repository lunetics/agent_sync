# Rust Migration Phase 4f: Native `adopt`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync adopt` (one file and `--all`) so the binary answers it byte for byte like `cmd_adopt` in `lib/helpers/adopt.sh`, after fixing three Bash bugs the reference turned up.

**Architecture:** `src/cli/adopt.rs` holds `discover_sources`, a `Resolver` that maps a destination to its tool, resource, and source (or a refusal), the single-file plan, and the `--all` batch. The resolver is public because `init` adopts existing files through `adopt_file_quiet` in 4i. `manifest::update_entry` joins `Manifest`. The command reuses `Tool`, `payload::{find_new_override, legacy_override_path, override_path, effective_source}`, `Paths::{resolve_dest, resolve_source}`, and `cli::refuse_outside_tools_dir`; `main` hands it `Project::discover`, whether stdin and stdout are terminals, and `prompts::confirm`. The seam stays the CLI process boundary: `tests/adopt.bats` under `AGENTSYNC_NATIVE=1` plus a parity fixture.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-15-rust-migration-phase-4e-dedupe.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `adopt` writes only the one source file a destination maps to (every OK entry's under `--all`) and only that destination's manifest line; it writes nothing on a refusal, a dry run, a declined prompt, or off a terminal without `--yes`.
- No binary ships to users; without a binary every command runs in Bash. Three Bash changes, Tasks 1–3, each in its own commit with a regression test in `tests/adopt.bats`; `lib/**/*.sh` stays clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state.
- Disk-touching unit tests are `#[cfg(unix)]`.
- Every expected value was captured from the fixed Bash on 2026-09-15: `scratchpad/phase4f/adopt_reference.sh` (every non-interactive branch, output in `adopt_reference.out`), `adopt_tty.sh` (the prompts on a pty), `update_entry_reference.sh`, `mode_probe.sh`, and `opencode_probe.sh`.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-15 under the maintainer's `/decide`: both as recommended.

1. **Fix three Bash bugs before porting.** Each was reproduced on 0.36.0's `adopt.sh`, and each regression test fails there and passes with the fix; `adopt.bats`, `init.bats`, and `init_flow.bats` stay green on the fixed copy.
   - **`adopt` ignores `source.*`.** `_adopt_discover_sources` only looks for `.ai/src/<dir>`, while `sync` reads the project's `source.rules`. With `source.rules: docs/rules`, adopting `.claude/rules/team.md` wrote a new `.ai/src/rules/team.md`, and the next `sync` silently put the old `docs/rules` content back into the output. **Recommended:** apply `source.<key>` (or a root-level `<key>`) over the detected layout, as `resolve_source_override` in `lib/sync.sh` does. A path outside the project stays refused, because `adopt` registers no external roots.
   - **A nested destination maps to the wrong target.** Cline's `commands.dest` `.clinerules/workflows` lies inside `rules.dest` `.clinerules`, and rules are tried first, so an edited workflow was adopted into `.ai/src/rules/workflows/go.md`. **Recommended:** the deepest matching directory wins; equal lengths keep today's order.
   - **The plan's diff prints absolute paths and modification times.** `diff -u <source> <dest>` puts both in its `---`/`+++` lines, so the output depends on the clock. **Recommended:** `--label` the two files with their project paths, as `dedupe` already labels its diff.
   Alternative for all three: port the behaviour as is and record quirks; the first loses user edits, so it is not offered as a quirk.
2. **Quirks and deviations.** Record as known quirks 33–34: `adopt` of a merged rules file such as Zed's `.rules` answers that it is not a recognised output, because the merge refusal is reachable only for a file inside a rules directory; `adopt --all` prints one `✓ adopted` line per destination, so two identical edits of one source name it twice. Record as an accepted deviation that `adopt`'s OpenCode refusal names a shipped settings file as `/<agentsync>/lib/templates/...`. The tool order is the Phase 1 byte-order deviation. **Recommended:** as listed.

## Module closure

```text
lib/helpers/adopt.sh            39-57   _adopt_prepare_context (status 2 for a missing config path)
                                61-97   _adopt_discover_sources
                                101-127 _adopt_dest_for, _adopt_dir_source_root
                                131-196 _adopt_try_tool
                                198-255 _adopt_resolve_agents_source, _adopt_resolve_payload_target
                                257-328 _adopt_resolve_dir_source
                                332-372 _adopt_resolve_dest
                                376-432 _adopt_collect_all, _adopt_flag_source_conflicts
                                434-527 _adopt_all_render_plan, _adopt_all_apply, _adopt_all
                                536-551 adopt_file_quiet (init, Phase 4i)
                                553-713 cmd_adopt
lib/helpers/manifest.sh         205-231 manifest_update_entry
lib/helpers/tool_resolver.sh    332-347 _find_any_payload_override; 379-436 resolve_payload_source; 511-516 list_all_tools
```

Reused: `Project::{discover, user_override_tools, user_tool_file, tools_dir_in_project}`, `catalog::base_tools`, `Tool::{load, value, user_value}`, `payload::{find_new_override, legacy_override_path, override_path, effective_source, legacy_warning, Source::shown}`, `Paths::{on_disk, absolute, canonicalize_with_existing_ancestor, resolve_dest, resolve_source}`, `Manifest::{load, paths, drift}`, `template_manifest::hash`, `yaml_subset::value`, `cli::refuse_outside_tools_dir`, `cli::customize::put`, `prompts::{is_tty, confirm}`, `style`.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in adopt baseline guard init init_flow tmp native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: this plan's commit; `217 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for every file.

---

### Task 1: `adopt` writes into the source directories `sync` reads

**Files:**
- Modify: `lib/helpers/adopt.sh` (`_adopt_discover_sources`, after the subagents detection)
- Test: `tests/adopt.bats`

**Interfaces:** none; `SOURCE_*` now carry the project's `source.<key>` when it is set.

- [x] **Step 1: Write the failing test**

Append to `tests/adopt.bats`:

```bash

@test "adopt: writes an edited rule into the source.rules directory sync reads" {
    mkdir -p docs/rules
    printf '# Team\n' > docs/rules/team.md
    printf 'tools:\n  enabled:\n    - claude\nsource:\n  rules: "docs/rules"\n' > .ai/agent_sync.yaml
    AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" sync >/dev/null
    echo "## Edited" >> .claude/rules/team.md

    run env AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" adopt --yes .claude/rules/team.md
    [ "$status" -eq 0 ]
    grep -q "Edited" docs/rules/team.md
    [ ! -f .ai/src/rules/team.md ]
}
```

Run: `bats --tap -f 'source.rules directory' tests/adopt.bats`
Expected: `not ok 1 adopt: writes an edited rule into the source.rules directory sync reads`.

- [x] **Step 2: Apply the project's source keys**

At the end of `_adopt_discover_sources`, after the `SOURCE_SUBAGENTS` detection:

```bash

    # sync.sh reads the project's source.<key> (or a root-level <key>) over this layout.
    [[ -n "${PROJECT_CONFIG_PATH:-}" && -f "$PROJECT_CONFIG_PATH" ]] || return 0
    local key override
    for key in agents rules skills commands subagents; do
        override=$(parse_yaml_value "$PROJECT_CONFIG_PATH" "source.$key") || true
        [[ -n "$override" ]] || override=$(parse_yaml_value "$PROJECT_CONFIG_PATH" "$key") || true
        [[ -n "$override" ]] || continue
        case "$key" in
            agents)    SOURCE_AGENTS="$override" ;;
            rules)     SOURCE_RULES="$override" ;;
            skills)    SOURCE_SKILLS="$override" ;;
            commands)  SOURCE_COMMANDS="$override" ;;
            subagents) SOURCE_SUBAGENTS="$override" ;;
        esac
    done
```

- [x] **Step 3: Run the tests, confirm green**

```bash
for f in adopt init init_flow; do
    printf '%s ok=%s notok=%s\n' "$f" "$(bats --tap "tests/$f.bats" | grep -c '^ok')" "$(bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
shellcheck -x -S warning -e SC1091 lib/helpers/adopt.sh
```

Expected: `adopt ok=27 notok=0`, `init ok=35 notok=0`, `init_flow ok=14 notok=0`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/adopt.sh tests/adopt.bats docs/plans/2026-09-15-rust-migration-phase-4f-adopt.md
git commit -m "fix(adopt): write into the source directories sync reads"
```

---

### Task 2: A nested destination maps to its deepest target

**Files:**
- Modify: `lib/helpers/adopt.sh` (`_adopt_try_tool`, the directory-target loop)
- Test: `tests/adopt.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing test**

Append to `tests/adopt.bats`:

```bash

@test "adopt: a Cline workflow goes to commands, not the rules directory around it" {
    enable_tools cline
    mkdir -p .ai/src/commands
    printf -- '---\ndescription: Go\n---\nGo.\n' > .ai/src/commands/go.md
    AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" sync >/dev/null
    echo "Edited." >> .clinerules/workflows/go.md

    run env AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" adopt --yes .clinerules/workflows/go.md
    [ "$status" -eq 0 ]
    [[ "$output" == *"resource: commands"* ]]
    grep -q "Edited." .ai/src/commands/go.md
    [ ! -e .ai/src/rules/workflows ]
}
```

Run: `bats --tap -f 'Cline workflow' tests/adopt.bats`
Expected: `not ok 1 adopt: a Cline workflow goes to commands, not the rules directory around it`.

- [x] **Step 2: Pick the deepest directory**

Replace the directory-target loop of `_adopt_try_tool`:

```bash
    # The deepest target wins: Cline's .clinerules/workflows/ is commands, not rules.
    local key dir best_key="" best_dir=""
    for key in rules skills commands subagents; do
        case "$key" in
            rules)     dir="$tool_rules" ;;
            skills)    dir="$tool_skills" ;;
            commands)  dir="$tool_commands" ;;
            subagents) dir="$tool_subagents" ;;
        esac
        [[ -z "$dir" ]] && continue
        if [[ "$target_abs" == "$dir/"* ]] && [[ ${#dir} -gt ${#best_dir} ]]; then
            best_key="$key"
            best_dir="$dir"
        fi
    done
    if [[ -n "$best_key" ]]; then
        _ADOPT_TOOL="$tool"; _ADOPT_RESOURCE="$best_key"
        _adopt_resolve_dir_source "$tool" "$best_key" "$best_dir"
        return 0
    fi
```

- [x] **Step 3: Run the tests, confirm green**

```bash
for f in adopt init init_flow; do
    printf '%s ok=%s notok=%s\n' "$f" "$(bats --tap "tests/$f.bats" | grep -c '^ok')" "$(bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
shellcheck -x -S warning -e SC1091 lib/helpers/adopt.sh
```

Expected: `adopt ok=28 notok=0`, `init ok=35 notok=0`, `init_flow ok=14 notok=0`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/adopt.sh tests/adopt.bats docs/plans/2026-09-15-rust-migration-phase-4f-adopt.md
git commit -m "fix(adopt): map a nested destination to its deepest target"
```

---

### Task 3: The plan's diff names files by project path

**Files:**
- Modify: `lib/helpers/adopt.sh` (`cmd_adopt`, the `diff_output` line)
- Test: `tests/adopt.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing test**

Append to `tests/adopt.bats`:

```bash

@test "adopt: the plan's diff names source and destination by project path" {
    enable_tools claude
    AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" sync >/dev/null
    echo "## Manual addition" >> .claude/rules/core.md

    run env AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" adopt --dry-run .claude/rules/core.md
    [ "$status" -eq 0 ]
    [[ "$output" == *$'    --- .ai/src/rules/core.md\n    +++ .claude/rules/core.md\n'* ]]
}
```

Run: `bats --tap -f 'by project path' tests/adopt.bats`
Expected: `not ok 1`.

- [x] **Step 2: Label the diff**

```bash
        diff_output=$(diff -u --label "$_ADOPT_SOURCE_REL" --label "$_ADOPT_DEST_REL" \
            "$_ADOPT_SOURCE_ABS" "$_ADOPT_DEST_ABS" 2>/dev/null | head -n 40 || true)
```

- [x] **Step 3: Run the tests, confirm green**

```bash
bats --tap tests/adopt.bats | grep -c '^ok'
shellcheck -x -S warning -e SC1091 lib/helpers/adopt.sh
```

Expected: `29`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/adopt.sh tests/adopt.bats docs/plans/2026-09-15-rust-migration-phase-4f-adopt.md
git commit -m "fix(adopt): name the plan diff's files by project path"
```

---

### Task 4: Port `manifest_update_entry`

**Files:**
- Modify: `src/manifest.rs`

**Interfaces:**
- Produces: `pub fn manifest::update_entry(root: &str, rel: &str, hash: &str) -> Result<(), Error>`

- [ ] **Step 1: Write the failing test**

Insert into the `tests` module of `src/manifest.rs`, before `drift_is_a_changed_file_in_manifest_order_and_a_missing_file_is_not`:

```rust
 
    #[cfg(unix)]
    #[test]
    fn one_entry_is_replaced_and_every_other_line_is_kept_as_bash_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(
            dir.path().join(REL),
            "z.md\tzz\n# note\t\nb.md\told\n\ta.md\t\taa\t\nnohash\nb.md\tdup\n",
        )
        .unwrap();
        update_entry(&root, "b.md", "new").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(REL)).unwrap(),
            "# note\t\na.md\taa\nb.md\tnew\nnohash\t\nz.md\tzz\n"
        );
        update_entry(&root, "", "x").unwrap();
        update_entry(&root, "c.md", "").unwrap();
        assert!(
            std::fs::read_to_string(dir.path().join(REL))
                .unwrap()
                .ends_with("z.md\tzz\n")
        );
    }

```

Run: `cargo test --lib manifest 2>&1 | grep -E '^error\[E0425\]' | sort -u`
Expected: `cannot find function `update_entry``.

- [ ] **Step 2: Write the implementation**

Before `sha256_hex` in `src/manifest.rs`:

```rust
/// `manifest_update_entry`: the manifest rewritten with `<rel>\t<hash>` in place
/// of any line for `rel`, every other line kept as `read` split it, `sort -u`.
pub fn update_entry(root: &str, rel: &str, hash: &str) -> Result<(), Error> {
    if rel.is_empty() || hash.is_empty() {
        return Ok(());
    }
    let path = Path::new(root).join(REL);
    let ai = Path::new(root).join(".ai");
    std::fs::create_dir_all(&ai).map_err(|e| Error::io(&ai, e))?;
    let mut lines = BTreeSet::new();
    if path.is_file() {
        let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
        for line in String::from_utf8_lossy(&bytes).split('\n') {
            let line = line.trim_matches('\t');
            let (existing, existing_hash) = match line.find('\t') {
                Some(tab) => (&line[..tab], line[tab..].trim_start_matches('\t')),
                None => (line, ""),
            };
            if existing.is_empty() || existing == rel {
                continue;
            }
            lines.insert(format!("{existing}\t{existing_hash}"));
        }
    }
    lines.insert(format!("{rel}\t{hash}"));
    let mut text = lines.into_iter().collect::<Vec<_>>().join("\n");
    text.push('\n');
    staging::write_beside(&path, text.as_bytes())
}

```

- [ ] **Step 3: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
```

Expected: `218 passed`, `0`, `11`, `1`; clippy exit 0.

- [ ] **Step 4: Commit**

```bash
git add src/manifest.rs docs/plans/2026-09-15-rust-migration-phase-4f-adopt.md
git commit -m "feat(native): port manifest_update_entry"
```

---

### Task 5: Port `adopt`

**Files:**
- Create: `src/cli/adopt.rs`
- Modify: `src/cli/mod.rs` (`pub mod adopt;` before `pub mod check;`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Consumes: Task 4's `manifest::update_entry`; Phase 4e's `template_manifest::hash`.
- Produces:
  - `pub fn discover_sources(project: &Project) -> Result<Sources, Error>`
  - `pub struct Adoption { tool, resource: &'static str, dest_rel, dest_abs, source_abs, source_rel }`
  - `pub struct Resolver<'a>`, `Resolver::new(project: &'a Project, sources: Sources) -> Result<Self, Error>`, `Resolver::resolve(&mut self, raw: &str, err: &mut dyn Write) -> Result<Result<Adoption, String>, Error>`, for `init` in 4i
  - `pub fn adopt(args: &[String], discover: &dyn Fn() -> Result<Project, Error>, style: &Style, interactive: bool, confirm: &mut dyn FnMut(&str) -> bool, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`

- [ ] **Step 1: Parity fixture, Bash side**

Append to `tests/native_parity.bats`:

```bash

# ── adopt ────────────────────────────────────────────────────────────────────

@test "parity: adopt plans, refuses, writes, and records like Bash" {
    enable_tools claude cursor cline amazonq
    mkdir -p .ai/src/commands .ai/src/skills/foo
    printf -- '---\ndescription: Go\n---\nGo.\n' > .ai/src/commands/go.md
    printf -- '---\nname: foo\n---\nSkill.\n' > .ai/src/skills/foo/SKILL.md
    assert_tree_parity adopt
    assert_tree_parity adopt --bogus
    assert_tree_parity adopt a b
    assert_tree_parity adopt --all CLAUDE.md
    assert_tree_parity adopt --help
    assert_tree_parity adopt CLAUDE.md
    assert_tree_parity adopt --all
    printf '# Before sync\n' > CLAUDE.md
    assert_tree_parity adopt -y CLAUDE.md
    rm CLAUDE.md
    _bash_sync
    assert_tree_parity adopt CLAUDE.md
    printf '\nEdited.\n' >> CLAUDE.md
    assert_tree_parity adopt CLAUDE.md
    assert_tree_parity adopt --dry-run CLAUDE.md
    assert_tree_parity adopt --yes CLAUDE.md
    printf 'Workflow edited.\n' >> .clinerules/workflows/go.md
    assert_tree_parity adopt -y .clinerules/workflows/go.md
    assert_tree_parity adopt .cursor/rules/core.mdc
    printf '{"edited": true}\n' > .claude/settings.json
    assert_tree_parity adopt -y .claude/settings.json
    assert_tree_parity adopt ../outside.md
    assert_tree_parity adopt .claude/rules/nope.md
    printf 'new\n' > .claude/rules/new.md
    assert_tree_parity adopt -y .claude/rules/new.md
    printf 'Claude edit.\n' >> .claude/rules/core.md
    printf 'Amazon edit.\n' >> .amazonq/rules/core.md
    printf 'Skill two.\n' >> .claude/skills/foo/SKILL.md
    printf 'cursor edit\n' >> .cursor/rules/core.mdc
    assert_tree_parity adopt --all
    assert_tree_parity adopt --all --dry-run
    assert_tree_parity adopt -a -y
    mkdir -p docs/rules
    printf '# Team\n' > docs/rules/team.md
    printf 'tools:\n  enabled:\n    - claude\nsource:\n  rules: "docs/rules"\n' > .ai/agent_sync.yaml
    _bash_sync --force
    printf 'Docs edit.\n' >> .claude/rules/team.md
    assert_tree_parity adopt -y .claude/rules/team.md
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_tree_parity adopt -y CLAUDE.md
    printf 'tools:\n  enabled:\n    - claude\nsource:\n  tools: "../elsewhere"\n' > .ai/agent_sync.yaml
    assert_tree_parity adopt -y CLAUDE.md
}
```

Run: `bats --tap -f 'adopt plans' tests/native_parity.bats`
Expected: `ok` (the native side still runs Bash).

- [ ] **Step 2: Write the failing tests**

Create `src/cli/adopt.rs` with the tests module only; add `pub mod adopt;` to `src/cli/mod.rs`:

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
            let path = format!("{root}/{rel}");
            std::fs::create_dir_all(paths::parent(&path)).unwrap();
            std::fs::write(path, text).unwrap();
        }
        (dir, root)
    }

    fn manifest_of(root: &str, rels: &[(&str, &str)]) {
        let text: String = rels
            .iter()
            .map(|(rel, content)| format!("{rel}\t{}\n", manifest::sha256_hex(content.as_bytes())))
            .collect();
        std::fs::write(format!("{root}/{}", manifest::REL), text).unwrap();
    }

    fn call(root: &str, args: &[&str], interactive: bool, accept: bool) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = adopt(
            &args,
            &discover,
            &Style::plain(),
            interactive,
            &mut |_| accept,
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

    const SOURCES: [(&str, &str); 4] = [
        (".ai/agent_sync.yaml", "tools:\n  enabled:\n    - claude\n"),
        (".ai/src/AGENTS.md", "# Agents\n"),
        (".ai/src/rules/core.md", "# Core\n\nBody.\n"),
        (".ai/src/commands/go.md", "---\ndescription: Go\n---\nGo.\n"),
    ];

    #[test]
    fn a_destination_maps_to_its_source_or_to_the_bash_refusal() {
        let mut files = SOURCES.to_vec();
        files.extend([
            ("CLAUDE.md", "# Agents\n"),
            (".clinerules/workflows/go.md", "Go.\n"),
            (".github/prompts/go.prompt.md", "Go.\n"),
            (".cursor/rules/core.mdc", "---\n---\n# Core\n"),
            (".claude/settings.json", "{}\n"),
            ("README.md", "readme\n"),
        ]);
        let (_dir, root) = project(&files);
        let project = Project::at(&root).unwrap();
        let mut resolver = Resolver::new(&project, discover_sources(&project).unwrap()).unwrap();
        let mut err = Vec::new();
        let mut resolve = |raw: &str| {
            resolver
                .resolve(raw, &mut err)
                .unwrap()
                .map(|found| (found.tool, found.resource, found.source_rel))
        };
        assert_eq!(
            resolve("CLAUDE.md"),
            Ok((
                "claude".to_string(),
                "agents",
                ".ai/src/AGENTS.md".to_string()
            ))
        );
        assert_eq!(
            resolve(".clinerules/workflows/go.md"),
            Ok((
                "cline".to_string(),
                "commands",
                ".ai/src/commands/go.md".to_string()
            ))
        );
        assert_eq!(
            resolve(".github/prompts/go.prompt.md"),
            Ok((
                "copilot".to_string(),
                "commands",
                ".ai/src/commands/go.md".to_string()
            ))
        );
        assert_eq!(
            resolve(".claude/settings.json"),
            Ok((
                "claude".to_string(),
                "settings",
                ".ai/src/tools/claude/settings.json".to_string()
            ))
        );
        assert_eq!(
            resolve(".cursor/rules/core.mdc"),
            Err("cursor injects a frontmatter header on sync. Adopting would propagate it to other tools' rule files. Edit .ai/src/rules/ instead.".to_string())
        );
        assert_eq!(
            resolve("../outside.md"),
            Err("Path is outside the project: ../outside.md".to_string())
        );
        assert_eq!(
            resolve(".claude/rules/nope.md"),
            Err("Destination file not found: .claude/rules/nope.md".to_string())
        );
        assert_eq!(
            resolve("README.md"),
            Err(
                "README.md is not a recognised AgentSync output (no enabled tool produces it)."
                    .to_string()
            )
        );
    }

    #[test]
    fn the_project_source_keys_move_the_detected_layout() {
        let (_dir, root) = project(&[
            (
                ".ai/agent_sync.yaml",
                "tools:\n  enabled: []\nsource:\n  rules: \"docs/rules\"\ncommands: \"docs/commands\"\n",
            ),
            (".ai/src/AGENTS.md", "# A\n"),
            (".ai/src/rules/core.md", "# Core\n"),
            (".ai/skills/s/SKILL.md", "s\n"),
        ]);
        let project = Project::at(&root).unwrap();
        assert_eq!(
            discover_sources(&project).unwrap(),
            Sources {
                agents: ".ai/src/AGENTS.md".to_string(),
                rules: "docs/rules".to_string(),
                skills: ".ai/skills".to_string(),
                commands: "docs/commands".to_string(),
                subagents: String::new(),
            }
        );
    }

    #[test]
    fn one_file_is_planned_confirmed_written_and_recorded_like_bash() {
        let mut files = SOURCES.to_vec();
        files.push(("CLAUDE.md", "# Agents\n\nEdited.\n"));
        let (_dir, root) = project(&files);
        manifest_of(&root, &[("CLAUDE.md", "# Agents\n")]);
        let plan = "\n  Adopt plan\n    tool:     claude\n    resource: agents\n    from:     CLAUDE.md (destination — your edit)\n    to:       .ai/src/AGENTS.md (source)\n\n    --- .ai/src/AGENTS.md\n    +++ CLAUDE.md\n    @@ -1 +1,3 @@\n     # Agents\n    +\n    +Edited.\n\n";

        assert_eq!(
            call(&root, &["CLAUDE.md"], false, true),
            (
                1,
                plan.to_string(),
                "Error: refusing to adopt non-interactively without --yes.\n".to_string()
            )
        );
        assert_eq!(
            call(&root, &["--dry-run", "CLAUDE.md"], false, true).1,
            format!("{plan}Dry-run — nothing written.\n")
        );
        assert_eq!(
            call(&root, &["CLAUDE.md"], true, false),
            (0, format!("{plan}Cancelled.\n"), String::new())
        );
        assert_eq!(
            std::fs::read_to_string(format!("{root}/.ai/src/AGENTS.md")).unwrap(),
            "# Agents\n"
        );

        assert_eq!(
            call(&root, &["--yes", "CLAUDE.md"], false, false),
            (
                0,
                format!(
                    "{plan}\n✓ Wrote .ai/src/AGENTS.md\n✓ Updated .ai/.sync-manifest\n\nRun agentsync sync to verify everything is consistent.\n"
                ),
                String::new()
            )
        );
        assert_eq!(
            std::fs::read_to_string(format!("{root}/.ai/src/AGENTS.md")).unwrap(),
            "# Agents\n\nEdited.\n"
        );
        assert_eq!(
            std::fs::read_to_string(format!("{root}/{}", manifest::REL)).unwrap(),
            format!(
                "CLAUDE.md\t{}\n",
                manifest::sha256_hex(b"# Agents\n\nEdited.\n")
            )
        );
        assert_eq!(
            call(&root, &["CLAUDE.md"], false, false).1,
            "Nothing to adopt: CLAUDE.md already matches the source.\n"
        );
    }

    #[test]
    fn all_skips_refusals_and_same_source_conflicts_like_bash() {
        let mut files = SOURCES.to_vec();
        files.extend([
            (".claude/rules/core.md", "# Core\n\nClaude edit.\n"),
            (".amazonq/rules/core.md", "# Core\n\nAmazon edit.\n"),
            (".cursor/rules/core.mdc", "edited\n"),
            (".claude/skills/foo/SKILL.md", "Skill two.\n"),
            (".ai/src/skills/foo/SKILL.md", "Skill.\n"),
        ]);
        let (_dir, root) = project(&files);
        manifest_of(
            &root,
            &[
                (".amazonq/rules/core.md", "# Core\n\nBody.\n"),
                (".claude/rules/core.md", "# Core\n\nBody.\n"),
                (".claude/skills/foo/SKILL.md", "Skill.\n"),
                (".cursor/rules/core.mdc", "generated\n"),
            ],
        );
        let plan = "\n  Adopt plan (--all)\n    1 file(s) will be promoted to source:\n\n    claude  .claude/skills/foo/SKILL.md → .ai/src/skills/foo/SKILL.md\n\n    3 skipped (edit .ai/src/ directly):\n    .cursor/rules/core.mdc — cursor injects a frontmatter header on sync. Adopting would propagate it to other tools' rule files. Edit .ai/src/rules/ instead.\n    .amazonq/rules/core.md — multiple edited outputs map to .ai/src/rules/core.md — adopt one explicitly\n    .claude/rules/core.md — multiple edited outputs map to .ai/src/rules/core.md — adopt one explicitly\n\n";
        assert_eq!(
            call(&root, &["--all", "--dry-run"], false, false),
            (
                0,
                format!("{plan}Dry-run — nothing written.\n"),
                String::new()
            )
        );
        assert_eq!(
            call(&root, &["-a", "-y"], false, false).1,
            format!(
                "{plan}\n✓ adopted .ai/src/skills/foo/SKILL.md\n\n✓ Adopted 1 file(s) into .ai/src/ and refreshed .ai/.sync-manifest\n\nRun agentsync sync to verify everything is consistent.\n"
            )
        );
        assert_eq!(
            std::fs::read_to_string(format!("{root}/.ai/src/skills/foo/SKILL.md")).unwrap(),
            "Skill two.\n"
        );
    }

    #[test]
    fn arguments_are_refused_with_the_bash_statuses() {
        let (_dir, root) = project(&SOURCES);
        let refused = |args: &[&str]| {
            let (status, _, err) = call(&root, args, false, false);
            (status, err)
        };
        assert_eq!(
            refused(&[]),
            (2, format!("Error: missing <dest-file>\n{USAGE}"))
        );
        assert_eq!(
            refused(&["--bogus"]),
            (2, "Error: unknown flag: --bogus\n".to_string())
        );
        assert_eq!(
            refused(&["a", "b"]),
            (
                2,
                "Error: adopt accepts a single destination file\n".to_string()
            )
        );
        assert_eq!(
            refused(&["--all", "CLAUDE.md"]),
            (2, "Error: adopt --all takes no <dest-file>\n".to_string())
        );
        assert_eq!(
            refused(&["--all"]),
            (
                1,
                "Error: no .ai/.sync-manifest yet — run 'agentsync sync' first, or adopt one file at a time.\n"
                    .to_string()
            )
        );
        assert_eq!(
            refused(&["CLAUDE.md"]),
            (
                1,
                "Cannot adopt: Destination file not found: CLAUDE.md\n".to_string()
            )
        );
    }
}
```

Run: `cargo test --lib cli::adopt 2>&1 | grep -E '^error' | sort -u | head -8`
Expected: compile errors naming the missing `adopt`, `Resolver`, `discover_sources`, and `Sources`.

- [ ] **Step 3: Write the implementation**

Prepend to `src/cli/adopt.rs`:

```rust
//! `agentsync adopt`: `cmd_adopt` of `lib/helpers/adopt.sh`, which promotes a
//! manual edit in a generated file back into its source.

use std::io::Write;
use std::path::Path;
use std::process::Command;

use super::customize::put;
use crate::log::Log;
use crate::manifest::{self, Manifest};
use crate::paths::{self, Paths};
use crate::payload::{self, Source};
use crate::project::Project;
use crate::style::Style;
use crate::tool::Tool;
use crate::{Error, catalog, template_manifest, yaml_subset};

type Discover<'a> = &'a dyn Fn() -> Result<Project, Error>;
type Confirm<'a> = &'a mut dyn FnMut(&str) -> bool;

const USAGE: &str = "Usage: agentsync adopt [--dry-run] [--yes] <dest-file>\n       agentsync adopt --all [--dry-run] [--yes]\n";

/// `SOURCE_*` as `_adopt_discover_sources` sets them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sources {
    pub agents: String,
    pub rules: String,
    pub skills: String,
    pub commands: String,
    pub subagents: String,
}

/// `_adopt_discover_sources`: the `.ai/src/` layout, else the flat `.ai/`
/// one, then the project's `source.<key>` or root-level `<key>`.
pub fn discover_sources(project: &Project) -> Result<Sources, Error> {
    let root = &project.root;
    let pick = |name: &str, file: bool| {
        [format!(".ai/src/{name}"), format!(".ai/{name}")]
            .into_iter()
            .find(|rel| {
                let path = root.join(rel);
                if file { path.is_file() } else { path.is_dir() }
            })
            .unwrap_or_default()
    };
    let mut sources = Sources {
        agents: pick("AGENTS.md", true),
        rules: pick("rules", false),
        skills: pick("skills", false),
        commands: pick("commands", false),
        subagents: pick("agents", false),
    };
    let Some(config_path) = project.config_path.as_ref().filter(|p| p.is_file()) else {
        return Ok(sources);
    };
    let config = std::fs::read(config_path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|e| Error::io(config_path, e))?;
    for (key, slot) in [
        ("agents", &mut sources.agents),
        ("rules", &mut sources.rules),
        ("skills", &mut sources.skills),
        ("commands", &mut sources.commands),
        ("subagents", &mut sources.subagents),
    ] {
        let mut value = yaml_subset::value(&config, &format!("source.{key}"));
        if value.is_empty() {
            value = yaml_subset::value(&config, key);
        }
        if !value.is_empty() {
            *slot = value;
        }
    }
    Ok(sources)
}

/// A destination mapped to the source it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adoption {
    pub tool: String,
    pub resource: &'static str,
    pub dest_rel: String,
    pub dest_abs: String,
    pub source_abs: String,
    pub source_rel: String,
}

/// What `_adopt_resolve_dest` needs besides the destination.
pub struct Resolver<'a> {
    pub project: &'a Project,
    pub paths: Paths,
    pub sources: Sources,
    tools: Vec<String>,
    warned_legacy: bool,
}

impl<'a> Resolver<'a> {
    pub fn new(project: &'a Project, sources: Sources) -> Result<Self, Error> {
        let root = project.root.to_string_lossy().into_owned();
        let mut tools = catalog::base_tools();
        tools.extend(project.user_override_tools()?);
        tools.sort();
        tools.dedup();
        Ok(Self {
            project,
            paths: Paths::on_disk(&root),
            sources,
            tools,
            warned_legacy: false,
        })
    }

    fn root(&self) -> String {
        self.project.root.to_string_lossy().into_owned()
    }

    fn strip_root(&self, abs: &str) -> String {
        abs.strip_prefix(&format!("{}/", self.root()))
            .unwrap_or(abs)
            .to_string()
    }

    /// `_adopt_resolve_dest`: the adoption, or the refusal reason.
    pub fn resolve(
        &mut self,
        raw: &str,
        err: &mut dyn Write,
    ) -> Result<Result<Adoption, String>, Error> {
        let abs = self.paths.absolute(raw);
        let Some(canonical) = self.paths.canonicalize_with_existing_ancestor(&abs) else {
            return Ok(Err(format!("Cannot resolve path: {raw}")));
        };
        if !paths::is_within(&canonical, &self.paths.root_canonical) {
            return Ok(Err(format!("Path is outside the project: {raw}")));
        }
        if !Path::new(&abs).is_file() {
            return Ok(Err(format!("Destination file not found: {raw}")));
        }
        let dest_rel = self.strip_root(&abs);
        for slug in self.tools.clone() {
            let tool = Tool::load(self.project, &slug)?;
            if let Some(found) = self.try_tool(&tool, &abs, &dest_rel, err)? {
                return Ok(found);
            }
        }
        Ok(Err(format!(
            "{dest_rel} is not a recognised AgentSync output (no enabled tool produces it)."
        )))
    }

    fn dest_for(&self, tool: &Tool, key: &str) -> Option<String> {
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            return None;
        }
        self.paths.resolve_dest(
            &raw,
            &format!("targets.{key}.dest for {}", tool.slug),
            &mut Log::default(),
        )
    }

    fn adoption(&self, tool: &Tool, resource: &'static str, dest: (&str, &str)) -> Adoption {
        Adoption {
            tool: tool.slug.clone(),
            resource,
            dest_abs: dest.0.to_string(),
            dest_rel: dest.1.to_string(),
            source_abs: String::new(),
            source_rel: String::new(),
        }
    }

    /// `_adopt_try_tool`: `None` when the tool produces no such output.
    fn try_tool(
        &mut self,
        tool: &Tool,
        abs: &str,
        dest_rel: &str,
        err: &mut dyn Write,
    ) -> Result<Option<Result<Adoption, String>>, Error> {
        let dest = (abs, dest_rel);
        if self.dest_for(tool, "agents").as_deref() == Some(abs) {
            return Ok(Some(
                self.agents_source(tool, self.adoption(tool, "agents", dest)),
            ));
        }
        if self.dest_for(tool, "settings").as_deref() == Some(abs) {
            if tool.value("targets.mcp.format") == "opencode_json"
                && let Some(mcp) = self.payload_source(tool, "mcp", err)?
            {
                let settings = self
                    .payload_source(tool, "settings", err)?
                    .map(|source| self.strip_root(&source.shown()))
                    .unwrap_or_default();
                return Ok(Some(Err(format!(
                    "OpenCode opencode.json is a multi-source output. Edit {settings} and {} separately.",
                    self.strip_root(&mcp.shown())
                ))));
            }
            return Ok(Some(
                self.payload_target(tool, self.adoption(tool, "settings", dest))?,
            ));
        }
        for resource in ["mcp", "hooks"] {
            if self.dest_for(tool, resource).as_deref() == Some(abs) {
                return Ok(Some(
                    self.payload_target(tool, self.adoption(tool, resource, dest))?,
                ));
            }
        }
        let mut best: Option<(&'static str, String)> = None;
        for key in ["rules", "skills", "commands", "subagents"] {
            let Some(dir) = self.dest_for(tool, key) else {
                continue;
            };
            let inside = abs.starts_with(&format!("{dir}/"));
            if inside && dir.len() > best.as_ref().map_or(0, |(_, d)| d.len()) {
                best = Some((key, dir));
            }
        }
        Ok(best.map(|(key, dir)| self.dir_source(tool, self.adoption(tool, key, dest), &dir)))
    }

    /// `resolve_payload_source`, printing its legacy-layout warning once.
    fn payload_source(
        &mut self,
        tool: &Tool,
        resource: &str,
        err: &mut dyn Write,
    ) -> Result<Option<Source>, Error> {
        let (source, legacy) = payload::effective_source(self.project, tool, resource)?;
        if let Some(path) = legacy
            && !self.warned_legacy
        {
            self.warned_legacy = true;
            put(err, payload::legacy_warning(self.project, &path).as_bytes())?;
        }
        Ok(source)
    }

    /// `_adopt_resolve_agents_source`.
    fn agents_source(&self, tool: &Tool, mut found: Adoption) -> Result<Adoption, String> {
        let over = tool.value("targets.agents.source");
        let raw = if over.is_empty() {
            self.sources.agents.clone()
        } else {
            over
        };
        if raw.is_empty() {
            return Err(format!(
                "No agents source resolved for {} — set source.agents in agent_sync.yaml or place AGENTS.md in .ai/src/.",
                found.tool
            ));
        }
        if raw.starts_with('/') {
            found.source_rel = self.strip_root(&raw);
            found.source_abs = raw;
        } else {
            found.source_abs = format!("{}/{raw}", self.root());
            found.source_rel = raw;
        }
        Ok(found)
    }

    /// `_adopt_resolve_payload_target`.
    fn payload_target(
        &self,
        tool: &Tool,
        mut found: Adoption,
    ) -> Result<Result<Adoption, String>, Error> {
        let resource = found.resource;
        let existing = match payload::find_new_override(self.project, &tool.slug, resource)? {
            Some(path) => Some(path),
            None => payload::legacy_override_path(self.project, tool, resource)
                .filter(|path| path.is_file()),
        };
        let root = self.root();
        let chosen = if let Some(path) = existing {
            Some(path.to_string_lossy().into_owned())
        } else {
            let declared = if self.project.user_tool_file(&tool.slug).is_file() {
                tool.user_value(&format!("targets.{resource}.source"))
            } else {
                String::new()
            };
            let declared_abs = if declared.is_empty() || declared.starts_with('/') {
                declared
            } else {
                format!("{root}/{declared}")
            };
            if declared_abs.starts_with(&format!("{root}/")) {
                Some(declared_abs)
            } else {
                payload::override_path(self.project, tool, resource)
                    .map(|path| path.to_string_lossy().into_owned())
            }
        };
        let Some(abs) = chosen else {
            return Ok(Err(format!(
                "No base template for {} {resource} — cannot pick a canonical override path.",
                tool.slug
            )));
        };
        found.source_rel = self.strip_root(&abs);
        found.source_abs = abs;
        Ok(Ok(found))
    }

    /// `_adopt_resolve_dir_source`.
    fn dir_source(
        &self,
        tool: &Tool,
        mut found: Adoption,
        dest_dir: &str,
    ) -> Result<Adoption, String> {
        let key = found.resource;
        let slug = &tool.slug;
        let value = |path: &str| tool.value(path);
        let refusal = match key {
            "rules" if value("targets.rules.merge_to_file") == "true" => Some(format!(
                "{slug} merges rules into a single file. Edit the source rules in {}/ instead.",
                self.sources.rules
            )),
            "rules" if value("targets.rules.inline_into_agents") == "true" => Some(format!(
                "{slug} inlines rules into AGENTS.md. Edit the source rules in {}/ instead.",
                self.sources.rules
            )),
            "rules"
                if !value("targets.rules.header").is_empty()
                    || !value("targets.rules.scoped_header").is_empty() =>
            {
                Some(format!(
                    "{slug} injects a frontmatter header on sync. Adopting would propagate it to other tools' rule files. Edit {}/ instead.",
                    self.sources.rules
                ))
            }
            "skills" if value("targets.skills.inline_into_agents") == "true" => Some(format!(
                "{slug} inlines a skill index into AGENTS.md. Edit {}/ instead.",
                self.sources.skills
            )),
            "commands" if value("targets.commands.format") == "toml" => Some(format!(
                "{slug} serializes commands as TOML. Conversion is not reversible — edit {}/ instead.",
                self.sources.commands
            )),
            "subagents" => {
                let format = value("targets.subagents.format");
                matches!(format.as_str(), "toml" | "amazonq_json" | "opencode_md").then(|| {
                    format!(
                        "{slug} serializes subagents as {format}. Conversion is not reversible — edit {}/ instead.",
                        self.sources.subagents
                    )
                })
            }
            _ => None,
        };
        if let Some(reason) = refusal {
            return Err(reason);
        }

        let over = value(&format!("targets.{key}.source"));
        let fallback = match key {
            "rules" => &self.sources.rules,
            "skills" => &self.sources.skills,
            "commands" => &self.sources.commands,
            _ => &self.sources.subagents,
        };
        let raw = if over.is_empty() {
            fallback.clone()
        } else {
            over
        };
        let src_root = if raw.is_empty() {
            None
        } else {
            self.paths.resolve_source(
                &raw,
                &format!("targets.{key}.source for {slug}"),
                &mut Log::default(),
            )
        };
        let Some(src_root) = src_root else {
            return Err(format!("No source directory resolved for {slug} {key}."));
        };

        let mut rel_inside = found
            .dest_abs
            .strip_prefix(&format!("{dest_dir}/"))
            .unwrap_or(&found.dest_abs)
            .to_string();
        if key != "skills" {
            let ext = value(&format!("targets.{key}.extension"));
            if !ext.is_empty()
                && let Some(stem) = rel_inside.strip_suffix(&ext)
            {
                rel_inside = format!("{stem}.md");
            }
        }
        found.source_abs = format!("{src_root}/{rel_inside}");
        found.source_rel = self.strip_root(&found.source_abs);
        Ok(found)
    }
}

pub fn adopt(
    args: &[String],
    discover: Discover,
    style: &Style,
    interactive: bool,
    confirm: Confirm,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (mut dry_run, mut assume_yes, mut all) = (false, false, false);
    let mut dest = String::new();
    for arg in args {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--yes" | "-y" => assume_yes = true,
            "--all" | "-a" => all = true,
            "--help" | "-h" => {
                put(
                    out,
                    format!("{USAGE}\nPromote a manual edit in a destination file back into .ai/src/ as the new\ncanonical content. Refuses transformed targets (merged rules, inlined skills,\nformat-converted commands/subagents).\n\nWith --all, adopt every drifted (manually-edited) tracked output at once,\nskipping refused targets and same-source conflicts.\n\nOptions:\n  --all,-a     Adopt every drifted output (no <dest-file>)\n  --dry-run    Show the plan without writing\n  --yes,-y     Skip confirmation (required outside a TTY)\n").as_bytes(),
                )?;
                return Ok(0);
            }
            flag if flag.starts_with('-') => {
                put(
                    err,
                    format!("{}: unknown flag: {flag}\n", style.red("Error")).as_bytes(),
                )?;
                return Ok(2);
            }
            _ if !dest.is_empty() => {
                put(
                    err,
                    format!(
                        "{}: adopt accepts a single destination file\n",
                        style.red("Error")
                    )
                    .as_bytes(),
                )?;
                return Ok(2);
            }
            value => dest = value.to_string(),
        }
    }
    if all && !dest.is_empty() {
        put(
            err,
            format!("{}: adopt --all takes no <dest-file>\n", style.red("Error")).as_bytes(),
        )?;
        return Ok(2);
    }
    if !all && dest.is_empty() {
        put(
            err,
            format!("{}: missing <dest-file>\n{USAGE}", style.red("Error")).as_bytes(),
        )?;
        return Ok(2);
    }

    let project = match discover() {
        Ok(project) => project,
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
            return Ok(2);
        }
        Err(other) => return Err(other),
    };
    if !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, err);
    }
    let sources = discover_sources(&project)?;
    let root = project.root.to_string_lossy().into_owned();
    let loaded = Manifest::load(&root)?;
    if loaded.is_none() && all {
        put(
            err,
            format!(
                "{}: no .ai/.sync-manifest yet — run 'agentsync sync' first, or adopt one file at a time.\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let mut resolver = Resolver::new(&project, sources)?;
    let mut run = Run {
        style,
        root: &root,
        interactive,
        confirm,
        out,
        err,
    };
    match loaded {
        Some(manifest) if all => adopt_all(&mut run, &mut resolver, &manifest, dry_run, assume_yes),
        manifest => adopt_one(
            &mut run,
            &mut resolver,
            manifest.as_ref(),
            &dest,
            dry_run,
            assume_yes,
        ),
    }
}

struct Run<'a> {
    style: &'a Style,
    root: &'a str,
    interactive: bool,
    confirm: Confirm<'a>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

fn hash(path: &str) -> Option<String> {
    template_manifest::hash(Path::new(path))
}

/// `cp <dest> <source>` after `ensure_dir`: an existing source keeps its mode,
/// a new one takes the destination's mode under the umask.
fn copy_into_source(found: &Adoption) -> Result<(), Error> {
    let source = Path::new(&found.source_abs);
    if let Some(parent) = source.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let bytes = std::fs::read(&found.dest_abs).map_err(|e| Error::io(&found.dest_abs, e))?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = std::fs::metadata(&found.dest_abs)
            .map_err(|e| Error::io(&found.dest_abs, e))?
            .permissions()
            .mode();
        options.mode(mode & 0o7777);
    }
    options
        .open(source)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|e| Error::io(source, e))
}

/// Whether to go on: refuses off a terminal without `--yes`, then asks.
fn confirmed(run: &mut Run, assume_yes: bool, question: &str) -> Result<Option<u8>, Error> {
    let style = run.style;
    if assume_yes {
        return Ok(None);
    }
    if !run.interactive {
        put(
            run.err,
            format!(
                "{}: refusing to adopt non-interactively without --yes.\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(Some(1));
    }
    if !(run.confirm)(question) {
        put(run.out, format!("{}\n", style.dim("Cancelled.")).as_bytes())?;
        return Ok(Some(0));
    }
    Ok(None)
}

fn verify_hint(style: &Style) -> String {
    format!(
        "{} {} {}\n",
        style.dim("Run"),
        style.cyan("agentsync sync"),
        style.dim("to verify everything is consistent.")
    )
}

/// `diff -u --label <source> --label <dest> <source> <dest> | head -n 40`.
fn plan_diff(found: &Adoption) -> String {
    let Ok(output) = Command::new("diff")
        .args([
            "-u",
            "--label",
            &found.source_rel,
            "--label",
            &found.dest_rel,
            &found.source_abs,
            &found.dest_abs,
        ])
        .output()
    else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let head: String = text.split_inclusive('\n').take(40).collect();
    head.trim_end_matches('\n').to_string()
}

fn adopt_one(
    run: &mut Run,
    resolver: &mut Resolver,
    manifest: Option<&Manifest>,
    dest: &str,
    dry_run: bool,
    assume_yes: bool,
) -> Result<u8, Error> {
    let style = run.style;
    let found = match resolver.resolve(dest, run.err)? {
        Ok(found) => found,
        Err(reason) => {
            put(
                run.err,
                format!("{}: {reason}\n", style.red("Cannot adopt")).as_bytes(),
            )?;
            return Ok(1);
        }
    };
    if let Some(manifest) = manifest
        && !manifest.paths().contains(&found.dest_rel)
    {
        put(
            run.err,
            format!(
                "{}: {} is not tracked in the manifest.\n  AgentSync only adopts files it produced. Run sync first to register the file.\n",
                style.red("Cannot adopt"),
                found.dest_rel
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let Some(current) = hash(&found.dest_abs) else {
        put(
            run.err,
            format!("{}: cannot hash {}\n", style.red("Error"), found.dest_rel).as_bytes(),
        )?;
        return Ok(1);
    };
    let source_exists = Path::new(&found.source_abs).is_file();
    if source_exists && hash(&found.source_abs).as_deref() == Some(current.as_str()) {
        put(
            run.out,
            format!(
                "{}\n",
                style.dim(&format!(
                    "Nothing to adopt: {} already matches the source.",
                    found.dest_rel
                ))
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }

    let mut plan = format!(
        "\n{}\n    {}     {}\n    {} {}\n    {}     {} {}\n    {}       {} {}\n\n",
        style.bold("  Adopt plan"),
        style.dim("tool:"),
        style.cyan(&found.tool),
        style.dim("resource:"),
        found.resource,
        style.dim("from:"),
        style.yellow(&found.dest_rel),
        style.dim("(destination — your edit)"),
        style.dim("to:"),
        style.green(&found.source_rel),
        style.dim("(source)")
    );
    if !source_exists {
        plan.push_str(&format!(
            "    {}\n\n",
            style.dim("(creating new source file)")
        ));
    } else {
        let diff = plan_diff(&found);
        if !diff.is_empty() {
            for line in diff.split('\n') {
                plan.push_str(&format!("    {line}\n"));
            }
            plan.push('\n');
        }
    }
    put(run.out, plan.as_bytes())?;

    if dry_run {
        put(
            run.out,
            format!("{}\n", style.dim("Dry-run — nothing written.")).as_bytes(),
        )?;
        return Ok(0);
    }
    if let Some(status) = confirmed(run, assume_yes, "Apply this adoption?")? {
        return Ok(status);
    }
    copy_into_source(&found)?;
    let mut done = format!("\n{} Wrote {}\n", style.green("✓"), found.source_rel);
    if manifest.is_some() {
        manifest::update_entry(run.root, &found.dest_rel, &current)?;
        done.push_str(&format!(
            "{} Updated .ai/.sync-manifest\n",
            style.green("✓")
        ));
    }
    done.push('\n');
    done.push_str(&verify_hint(style));
    put(run.out, done.as_bytes())?;
    Ok(0)
}

fn adopt_all(
    run: &mut Run,
    resolver: &mut Resolver,
    manifest: &Manifest,
    dry_run: bool,
    assume_yes: bool,
) -> Result<u8, Error> {
    let style = run.style;
    let mut planned: Vec<(Adoption, String)> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    for rel in manifest.drift(run.root) {
        match resolver.resolve(&format!("{}/{rel}", run.root), run.err)? {
            Err(reason) => skipped.push((rel, reason)),
            Ok(found) => match hash(&found.dest_abs) {
                Some(current) => planned.push((found, current)),
                None => skipped.push((rel, "cannot hash destination".to_string())),
            },
        }
    }
    let ok: Vec<bool> = planned
        .iter()
        .enumerate()
        .map(|(i, (found, current))| {
            !planned.iter().enumerate().any(|(j, (other, other_hash))| {
                i != j && found.source_abs == other.source_abs && current != other_hash
            })
        })
        .collect();
    for ((found, _), fine) in planned.iter().zip(&ok) {
        if !fine {
            skipped.push((
                found.dest_rel.clone(),
                format!(
                    "multiple edited outputs map to {} — adopt one explicitly",
                    found.source_rel
                ),
            ));
        }
    }
    let ok_count = ok.iter().filter(|fine| **fine).count();

    if ok_count == 0 && skipped.is_empty() {
        put(
            run.out,
            format!(
                "{}\n",
                style.dim("Nothing to adopt: every tracked output matches its source.")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }

    let mut plan = format!("\n{}\n", style.bold("  Adopt plan (--all)"));
    if ok_count > 0 {
        plan.push_str(&format!(
            "    {ok_count} file(s) will be promoted to source:\n\n"
        ));
        for ((found, _), _) in planned.iter().zip(&ok).filter(|(_, fine)| **fine) {
            plan.push_str(&format!(
                "    {}  {} {} {}\n",
                style.cyan(&found.tool),
                style.yellow(&found.dest_rel),
                style.dim("→"),
                style.green(&found.source_rel)
            ));
        }
        plan.push('\n');
    }
    if !skipped.is_empty() {
        plan.push_str(&format!(
            "    {}\n",
            style.dim(&format!(
                "{} skipped (edit .ai/src/ directly):",
                skipped.len()
            ))
        ));
        for (rel, reason) in &skipped {
            plan.push_str(&format!(
                "    {} {} {}\n",
                style.yellow(rel),
                style.dim("—"),
                style.dim(reason)
            ));
        }
        plan.push('\n');
    }
    put(run.out, plan.as_bytes())?;

    if ok_count == 0 {
        put(
            run.out,
            format!(
                "{}\n",
                style.dim("No adoptable edits — the drifted files above need manual source edits.")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    if dry_run {
        put(
            run.out,
            format!("{}\n", style.dim("Dry-run — nothing written.")).as_bytes(),
        )?;
        return Ok(0);
    }
    let question = format!("Apply these {ok_count} adoption(s)?");
    if let Some(status) = confirmed(run, assume_yes, &question)? {
        return Ok(status);
    }

    put(run.out, b"\n")?;
    for ((found, current), _) in planned.iter().zip(&ok).filter(|(_, fine)| **fine) {
        copy_into_source(found)?;
        manifest::update_entry(run.root, &found.dest_rel, current)?;
        put(
            run.out,
            format!(
                "{} {} {}\n",
                style.green("✓"),
                style.dim("adopted"),
                found.source_rel
            )
            .as_bytes(),
        )?;
    }
    put(
        run.out,
        format!(
            "\n{} Adopted {ok_count} file(s) into .ai/src/ and refreshed .ai/.sync-manifest\n\n{}",
            style.green("✓"),
            verify_hint(style)
        )
        .as_bytes(),
    )?;
    Ok(0)
}

```

In `src/main.rs`, add `| "adopt"` to the raw-argument command pattern and this arm before `"profile"`:

```rust
            "adopt" => cli::adopt::adopt(
                &rest,
                &Project::discover,
                &style,
                prompts::is_tty(),
                &mut |question: &str| prompts::confirm(question, false),
                &mut out,
                &mut err,
            ),
```

In `bin/agentsync.sh:280` append `adopt`.

- [ ] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in adopt baseline guard tmp; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
bats --tap -f 'adopt plans' tests/native_parity.bats
```

Expected: `223 passed`, `0`, `11`, `1`; `0` for each file; `ok`.

- [ ] **Step 5: Prove the fixture bites, check the terminal and file modes, lint, commit**

Change `— adopt one explicitly` to `— adopt one` in `src/cli/adopt.rs`, rebuild, rerun the fixture: `not ok` with the two conflict lines in the diff; revert and rebuild.

Run `scratchpad/phase4f/adopt_tty.sh 0` and `adopt_tty.sh 1` (a declined and an accepted single adoption and an accepted `--all` on a pty, answers after three seconds) and diff the transcripts: identical. Run `mode_probe.sh`: a new source takes the destination's mode under the umask and an existing source keeps its own, in both engines.

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/adopt.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-15-rust-migration-phase-4f-adopt.md
git commit -m "feat(native): port adopt"
```

---

### Task 6: Verify, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [ ] **Step 1: Spec**

Append to "Known quirks":

```markdown
33. `adopt` of a merged rules file such as Zed's `.rules` answers that it is not
    a recognised output; the merge refusal is reachable only for a file inside
    a rules directory.
34. `adopt --all` prints one `✓ adopted` line per destination, so two identical
    edits of one source name it twice.
```

Append to "Accepted deviations":

```markdown
- Phase 4f: `adopt`'s OpenCode refusal names a shipped settings file as
  `/<agentsync>/lib/templates/...` where Bash printed the install directory.
```

- [ ] **Step 2: Module map and outputs**

Set the `lib/helpers/adopt.sh` row to `→ src/cli/adopt.rs        Phase 4f, ported; Resolver serves init's adopt_file_quiet in 4i`, and add `update_entry (Phase 4f)` to the `lib/helpers/manifest.sh` row. Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [ ] **Step 3: Verify (outside the agent sandbox)**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in adopt baseline guard init init_flow tmp native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `223 passed`, `0`, `11`, `1`; lint exit 0; every line `bash=0 native=0`.

- [ ] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-15-rust-migration-phase-4f-adopt.md
git commit -m "docs(native): map the phase 4f modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, the files in Task 6 are green under both engines, the parity fixture passes, and a `## Completion receipt` records the fresh verification. The next plan is 4g, `migrate`.

## Run log

### 2026-09-15 — Phase 4f planned
- Commits: this plan.
- Verified: the three Bash fixes were applied to a copy of the engine: each new test failed on the committed `adopt.sh` and passed on the copy (`adopt.bats` 29/29), and `init.bats` 35/35 and `init_flow.bats` 14/14 passed there. The Rust in Tasks 4–5 was drafted against that copy and removed from the tree: `cargo test` 223/0/11/1, clippy clean; `adopt_reference.sh` gave identical 517-line transcripts for the fixed Bash and the binary, and an `Updated .ai/.sync-manifest` mutation showed in the diff; with `adopt` in the copy's `_NATIVE_COMMANDS`, `adopt` 29/29, `baseline` 11/11, `guard` 18/18, and `tmp` 15/15 passed in both modes; the parity fixture passed and failed on a `— adopt one explicitly` mutation; `adopt_tty.sh` transcripts were identical; `mode_probe.sh` gave `new=755 existing=600` in both engines; `opencode_probe.sh` showed the shipped-path deviation.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.
