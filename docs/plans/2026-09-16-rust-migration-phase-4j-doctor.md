# Rust Migration Phase 4j: Native `doctor`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync doctor` so the binary answers it byte for byte like `cmd_doctor` in `lib/helpers/doctor.sh`: the eleven report sections (project layout with the version pin and format revision, enabled tools with the commands, ownership, and guard checks, edit paths, user overrides, source directories, drift, the secret and JSON scan, empty skills, always-on rules, orphan outputs, cross-project duplicates), the warning, error, and advisory counters, and the tri-state exit status. One Bash portability bug the reference turned up is fixed first.

**Architecture:** `edit_paths` gains `checklist` (the glyph rows `print_tool_edit_paths_checklist` draws), `Manifest` exposes its entries in file order, and `opencode_json` gains `settings_has_mcp` (the awk program's `inspect-settings` mode). `src/cli/doctor.rs` holds the command: a `Doctor` struct carries the counters and the two writers, one method per Bash section, the secret patterns as prefix-plus-run matchers, a JSON acceptor that answers as `python3 -c 'json.load(...)'` does, and the `sed`/`grep` frontmatter test. `main` hands it `Project::discover`, the engine version, and `AGENTSYNC_EXTERNAL_SOURCE_ROOTS`; arguments are ignored as Bash ignores them. The seam stays the CLI process boundary: the eight bats files that run `doctor` under `AGENTSYNC_NATIVE=1`, two parity fixtures, and a 43-scenario reference harness.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-16-rust-migration-phase-4i-init.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `doctor` writes nothing: no file, no manifest, no backup.
- No binary ships to users; without a binary every command runs in Bash. One Bash change, Task 1, in its own commit with a regression test; `lib/**/*.sh` and `bin/agentsync.sh` stay clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, and exit status, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment.
- Disk-touching unit tests are `#[cfg(unix)]`.
- Every expected value was captured from the fixed Bash on 2026-09-16: `phase4j/doctor_reference.sh` (43 scenarios, a 2064-line transcript), `phase4j/legacy_probe.sh` and `phase4j/minimal_probe.sh` (stdout and stderr apart), `phase4j/helper_probe.sh` and `phase4j/opencode_probe.sh` (the helper answers the unit tests assert). The scripts are reproduced in Task 3 Step 5.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-16 under the maintainer's standing instruction to run Phase 4 to its close; each was checked against Bash before the call.

1. **Fix the summary rule in Bash first.** `log_separator_doctor` runs `printf '  %*s\n' 60 '' | tr ' ' '─'`. `tr` maps the two indent spaces too, and GNU `tr` maps bytes, so on Linux the rule is sixty-two copies of the first byte of `─`, invalid UTF-8; macOS prints sixty-two dashes without the indent. The reference depends on the platform, so the triage order says fix Bash. **Fix:** build the rule with `printf -v` and print it behind the two-space indent the format string asked for (Task 1). Alternative: reproduce the macOS output; rejected because the Linux output is corrupt and the indent was the evident intent.
2. **Validate JSON in-process, as `python3` judges it.** `_doctor_validate_json` uses `python3 -c 'json.load(...)'`, else `node`, else accepts everything, so Bash's verdict depended on the machine. The port accepts RFC 8259 JSON plus `NaN`, `Infinity`, and `-Infinity`, and rejects a BOM, trailing commas, control characters in strings, and an empty file, which is `json.load`'s answer; recorded as an accepted deviation. Alternative: spawn `python3` from the binary; rejected because the binary spawns nothing but `stty` and `diff`, and the Bash fallbacks made the answer machine-dependent anyway.
3. **Quirks and deviations.** Record as known quirks 44–45: the secret scan lists only the lines of the first pattern with a hit, in pattern order, so a file with an AWS key on line 1 and an OpenAI key on line 2 reports only line 2; a line holding `${…}` anywhere, or `<…>` without `sk-`, is never reported, however real the key beside the placeholder. Record as accepted deviations the JSON validator (decision 2) and the byte-order listing of skill directories, rules, overrides, and parent files (the Phase 2 deviation; the harness's `Zeta` and `empty-one` skills swap places). `doctor --bogus extra` runs the report unchanged in both engines because `main` dispatches `doctor` before clap. **Recommended:** as listed.

## Module closure

```text
lib/helpers/doctor.sh            20-38    _doctor_prepare_context (Project::discover, Paths::on_disk)
                                 40-63    _doctor_external_source (Paths::classify_explicit_source, absolute)
                                 65-75    counters and the ok/warn/fail/info/advise printers
                                 82-120   _DOCTOR_SECRET_PATTERNS, _doctor_scan_file
                                 124-137  _doctor_validate_json (decision 2)
                                 142-165  _doctor_scan_one_file
                                 174-224  _doctor_check_drift (Manifest::entries, template_manifest::hash)
                                 226-270  _doctor_scan_overrides
                                 278-313  _doctor_check_commands_config
                                 318-338  _doctor_check_guard_wired (payload::effective_source)
                                 340-371  _doctor_check_payload_ownership (opencode_json::settings_has_mcp, payload::find_new_override)
                                 379-401  _doctor_check_empty_skills
                                 408-431  _doctor_check_always_on_rules
                                 433-492  _DOCTOR_OUTPUT_DIR_MAP, _doctor_check_orphan_outputs (Project::enabled_tools)
                                 511-620  _doctor_check_cross_project (overlay::shared_parent_src, inherit_categories; paths::find_parent_ai_src; convert::read_field)
                                 622-836  cmd_doctor
                                 838-841  log_separator_doctor (Task 1)
lib/helpers/edit_paths.sh        94-128   print_tool_edit_paths_checklist (Task 2)
lib/helpers/opencode.sh          465-473  opencode_settings_has_mcp (Task 2)
lib/helpers/manifest.sh          87       manifest_load (Manifest::entries, Task 2)
lib/helpers/tool_resolver.sh     49       tool_resolver_select_project_config (exit 2 on a missing AGENTSYNC_CONFIG_PATH)
                                 353-365  _warn_legacy_payload_path (payload::legacy_warning, once per run)
                                 497-560  list_user_override_tools, list_all_tools, list_configured_enabled_tools, list_enabled_tools
lib/helpers/shared.sh            404      shared_parent_src; 459 shared_inherits_category
lib/helpers/paths.sh             17       source_abs_path_r; 50 explicit_source_root_r; 468 find_parent_ai_src
lib/helpers/format_conversion.sh 117      read_frontmatter_field
lib/helpers/format.sh            11       engine_format; 24 project_format
bin/agentsync.sh                 280      _NATIVE_COMMANDS; 392 doctor's _need list
```

Reused: `Project::{discover, at, enabled_tools, configured_enabled_tools, user_override_tools, user_tools_dir, user_tool_file}`, `Tool::{load, value, display_name}`, `Paths::{on_disk, trust_external_roots, classify_explicit_source, absolute}`, `paths::{parent, leaf, find_parent_ai_src}`, `payload::{effective_source, find_new_override, legacy_warning, Source}`, `Manifest::load`, `template_manifest::hash`, `catalog::{base_tools, base_tool_yaml}`, `format_rev::{engine, project}`, `overlay::{shared_parent_src, inherit_categories}`, `convert::read_field`, `yaml_subset::value`, `edit_paths::checklist`, `opencode_json::settings_has_mcp`, `style`, `cli::customize::put`.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in doctor drift guard format_migration migrate shared source_overrides config_safety native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: the plan's latest commit; `255 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for every file, with `native_parity` run outside the agent sandbox, which refuses the `diff -` stdin operand `src/cli/diff.rs` uses.

---

### Task 1: The summary rule is drawn without `tr`

**Files:**
- Modify: `lib/helpers/doctor.sh` (`log_separator_doctor`)
- Test: `tests/doctor.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing test**

Append to `tests/doctor.bats`:

```bash
@test "doctor: the summary rule is indented and sixty characters wide" {
    run_agentsync init >/dev/null
    run run_agentsync doctor
    [ "$status" -eq 0 ]
    local rule
    printf -v rule '─%.0s' {1..60}
    [[ "$output" == *$'\n'"  $rule"$'\n'* ]]
}
```

Run: `bats --tap -f 'summary rule' tests/doctor.bats`
Expected: `not ok 1 doctor: the summary rule is indented and sixty characters wide`.

- [x] **Step 2: Build the rule in Bash**

Replace `log_separator_doctor` at the end of `lib/helpers/doctor.sh` with:

```bash
log_separator_doctor() {
    # GNU tr maps single bytes, so `tr ' ' '─'` garbles the rule on Linux.
    local rule
    printf -v rule '─%.0s' {1..60}
    echo "  $rule"
}
```

- [x] **Step 3: Run the tests, confirm green**

```bash
bats --tap tests/doctor.bats | grep -c '^ok'
shellcheck -x -S warning -e SC1091 lib/helpers/doctor.sh
```

Expected: `37`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/doctor.sh tests/doctor.bats docs/plans/2026-09-16-rust-migration-phase-4j-doctor.md
git commit -m "fix(doctor): draw the summary rule without tr"
```

---

### Task 2: Port the doctor helpers

**Files:**
- Modify: `src/edit_paths.rs`, `src/manifest.rs`, `src/opencode_json.rs`

**Interfaces:**

```rust
// src/edit_paths.rs
pub fn checklist(project: &Project, tool: &Tool, style: &Style) -> String;
// src/manifest.rs
impl Manifest { pub fn entries(&self) -> &[(String, String)]; }
// src/opencode_json.rs
pub fn settings_has_mcp(settings: &str) -> Result<bool, ComposeError>;
```

- [x] **Step 1: Write the failing tests**

Append to the `tests` module of `src/edit_paths.rs`:

```rust
    #[test]
    fn the_checklist_glyphs_each_row_like_doctor() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        let claude = Tool::load(&project, "claude").unwrap();
        assert_eq!(
            checklist(&project, &claude, &Style::plain()),
            "      Claude Code\n          ·  settings   agentsync customize claude settings\n          ·  mcp        agentsync add mcp <server> (shared — not yet configured)\n"
        );
        write(dir.path(), ".ai/src/tools/claude/settings.json", "{}");
        write(dir.path(), ".ai/src/mcp.json", "{}");
        assert_eq!(
            checklist(&project, &claude, &Style::plain()),
            "      Claude Code\n          ✓  settings   .ai/src/tools/claude/settings.json\n          ✓  mcp        .ai/src/mcp.json (shared)\n"
        );
        let cursor = Tool::load(&project, "cursor").unwrap();
        write(dir.path(), ".ai/src/tools/cursor/hooks.json", "{}");
        assert_eq!(
            checklist(&project, &cursor, &Style::plain()),
            "      Cursor\n          ✓  hooks      .ai/src/tools/cursor/hooks.json\n          ✓  mcp        .ai/src/mcp.json (shared)\n"
        );
        assert_eq!(
            checklist(
                &project,
                &Tool::load(&project, "mytool").unwrap(),
                &Style::plain()
            ),
            ""
        );
    }
```

Add to the `tests` module of `src/opencode_json.rs`, after the `err` helper:

```rust
    #[test]
    fn a_top_level_mcp_member_is_found_like_opencode_settings_has_mcp() {
        assert_eq!(
            settings_has_mcp("{\"mcp\": {\"srv\": {\"type\": \"local\"}}, \"theme\": \"x\"}"),
            Ok(true)
        );
        assert_eq!(
            settings_has_mcp("{\"theme\": \"x\", \"mcpServers\": {}}"),
            Ok(false)
        );
        assert_eq!(settings_has_mcp("{\"a\": {\"mcp\": {}}}"), Ok(false));
        assert_eq!(settings_has_mcp("{\"mcp\": ").unwrap_err().code, 20);
        assert_eq!(settings_has_mcp("").unwrap_err().code, 20);
    }
```

Run: `cargo test checklist_glyphs mcp_member 2>&1 | grep -E '^error|test result' | head -3`
Expected: a compile error naming `checklist` and `settings_has_mcp`.

- [x] **Step 2: Write the implementation**

In `src/edit_paths.rs`, after `block`:

```rust
/// `print_tool_edit_paths_checklist`: `doctor`'s indented rows with a glyph per
/// state; empty when the tool has no base payloads.
pub fn checklist(project: &Project, tool: &Tool, style: &Style) -> String {
    let rows = rows(project, tool);
    if rows.is_empty() {
        return String::new();
    }
    let mut text = format!("      {}\n", style.bold(&tool.display_name()));
    for row in rows {
        let (glyph, resource, render) = match row {
            Row::Override(resource, path) => (style.green("✓"), resource, path),
            Row::Shared(path) => (
                style.green("✓"),
                "mcp",
                format!("{path} {}", style.dim("(shared)")),
            ),
            Row::CustomizeHint(resource, command) => {
                (style.dim("·"), resource, style.dim(&command))
            }
            Row::SharedHint => (
                style.dim("·"),
                "mcp",
                style.dim("agentsync add mcp <server> (shared — not yet configured)"),
            ),
        };
        text.push_str(&format!("          {glyph}  {resource:<10} {render}\n"));
    }
    text
}
```

In `src/manifest.rs`, after `paths`:

```rust
    /// `MANIFEST_KEYS` and `MANIFEST_VALUES`, in file order.
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }
```

In `src/opencode_json.rs`, before `compose`:

```rust
/// `opencode_settings_has_mcp`: whether the settings JSON has a top-level
/// `mcp` member; malformed settings are the awk program's error 20.
pub fn settings_has_mcp(settings: &str) -> Result<bool, ComposeError> {
    let mut p = Parser::new();
    let settings_text = read_text(settings);
    if !p.walk_root(&settings_text, Mode::Settings, 20) && p.error.is_none() {
        p.fail(20, "malformed settings JSON");
    }
    if let Some(error) = p.take_error() {
        return Err(error);
    }
    Ok(p.settings.keys.iter().any(|k| k == "mcp"))
}
```

- [x] **Step 3: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
```

Expected: `257 passed`, `0`, `11`, `1`; clippy clean. The Bash answers behind the assertions: `opencode_settings_has_mcp` exits 0, 1, 1, 20, 20 on the five inputs (`phase4j/opencode_probe.sh`); the checklist rows are the `Edit paths` section of the `guard-legacy-settings` and `edit-paths-shared-mcp` scenarios.

- [x] **Step 4: Commit**

```bash
git add src/edit_paths.rs src/manifest.rs src/opencode_json.rs docs/plans/2026-09-16-rust-migration-phase-4j-doctor.md
git commit -m "feat(native): port the doctor helpers"
```

---

### Task 3: Port `doctor`

**Files:**
- Create: `src/cli/doctor.rs`
- Modify: `src/cli/mod.rs`, `src/main.rs`, `bin/agentsync.sh`
- Test: `tests/native_parity.bats`

**Interfaces:**

```rust
// src/cli/doctor.rs
pub struct Env<'a> { pub version: &'a str, pub external_roots: Option<String> }
pub fn doctor(
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error>;
```

- [x] **Step 1: Parity fixtures, Bash side**

Append to `tests/native_parity.bats`:

```bash
# ── doctor ───────────────────────────────────────────────────────────────────
# doctor writes nothing, so assert_parity compares the report and the status.

@test "parity: doctor reports layout, tools, overrides, drift, and advisories like Bash" {
    assert_parity doctor
    assert_parity doctor --bogus extra
    enable_tools claude cursor opencode kimi
    _bash_sync
    assert_parity doctor
    printf 'edit\n' >> CLAUDE.md
    rm -f .claude/rules/core.md
    mkdir -p .ai/src/tools/claude .ai/src/tools/cursor .ai/src/tools/opencode .ai/src/settings .ai/src/hooks .ai/src/skills/empty .zed
    printf '{"a": 1,}\n' > .ai/src/tools/cursor/mcp.json
    printf '{"gh": "ghp_abcdefghijklmnopqrstuvwxyz012345678901", "ok": "${TOKEN}"}\n' > .ai/src/tools/claude/mcp.json
    printf '{"s": 2}\n' > .ai/src/settings/claude.json
    printf '[hooks]\n' > .ai/src/hooks/kimi.toml
    printf '{"mcp": {}}\n' > .ai/src/tools/opencode/settings.json
    printf '{"mcpServers": {}}\n' > .ai/src/mcp.json
    assert_parity doctor
    run_agentsync customize claude >/dev/null
    printf 'enabled: true\n' > .ai/src/tools/zed.yaml
    assert_parity doctor
}

@test "parity: doctor fails on a bad config path, refused sources, and a missing layout like Bash" {
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_parity doctor
    mkdir -p parent/.ai/src/rules
    cp .ai/src/rules/core.md parent/.ai/src/rules/core.md
    printf '# other\n' > parent/.ai/src/rules/git.md
    printf 'agentsync_version: "0.0.1"\nformat: 1\nshared:\n  path: parent\n  inherit: rules\nsource:\n  skills: /\n  commands: ../outside/commands\n' > .ai/agent_sync.yaml
    assert_parity doctor
    rm -rf .ai
    assert_parity doctor
}
```

Run, outside the sandbox: `AGENTSYNC_NATIVE=0 bats --tap tests/doctor.bats | grep -c '^ok'`; `bats --tap -f 'parity: doctor' tests/native_parity.bats`
Expected: `37`; `ok 1` and `ok 2` (the native side still runs Bash until Step 3 lists `doctor`).

- [x] **Step 2: Write the failing tests**

Create `src/cli/doctor.rs` with the tests module, and add `pub mod doctor;` to `src/cli/mod.rs` between `diff` and `enable`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_listed_by_the_first_pattern_that_hits_like_doctor_scan_file() {
        let mcp = br#"{"mcpServers":{"gh":{"env":{"TOKEN":"ghp_abcdefghijklmnopqrstuvwxyz012345678901"}}}}"#;
        let expected = format!("1:{}", String::from_utf8_lossy(mcp));
        assert_eq!(
            scan_secrets(&[mcp.as_slice(), b"\n"].concat()),
            vec![expected]
        );
        let many = br#"{"aws":{"key":"AKIAIOSFODNN7EXAMPLE"},"slack":"xoxb-1234567890-abc","g":"AIzaSyA1234567890abcdefghijklmnopqrstuv","jwt":"eyJabcdefghijk.eyJabcdefghijk.abcdefghijklmn","pat":"github_pat_abcdefghijklmnopqrstuvwxyz0123456789"}"#;
        assert_eq!(scan_secrets(many).len(), 1);
        assert_eq!(
            scan_secrets(b"AKIAIOSFODNN7EXAMPLE\nsk-abcdefghijklmnopqrstuvwxyz\n"),
            vec!["2:sk-abcdefghijklmnopqrstuvwxyz".to_string()]
        );
        assert_eq!(
            scan_secrets(
                b"first xoxp-abcdefghij-k\nsecond eyJabcdefghijk.eyJabcdefghijk.abcdefghijklmn\n"
            ),
            vec!["1:first xoxp-abcdefghij-k".to_string()]
        );
        assert_eq!(
            scan_secrets(
                b"a ${X} ghp_abcdefghijklmnopqrstuvwxyz012345678901\nb AKIAIOSFODNN7EXAMPLE\n"
            ),
            vec!["2:b AKIAIOSFODNN7EXAMPLE".to_string()]
        );
        assert!(
            scan_secrets(b"token: ${GITHUB_TOKEN} ghp_abcdefghijklmnopqrstuvwxyz012345678901\n")
                .is_empty()
        );
        assert!(scan_secrets(b"<ghp_abcdefghijklmnopqrstuvwxyz012345678901>\n").is_empty());
        assert_eq!(
            scan_secrets(b"<sk-abcdefghijklmnopqrstuvwxyz>\n"),
            vec!["1:<sk-abcdefghijklmnopqrstuvwxyz>".to_string()]
        );
        assert!(scan_secrets(b"\0binary sk-abcdefghijklmnopqrstuvwxyz\n").is_empty());
        assert!(scan_secrets(b"sk-short\nxoxb-123\nAKIA1234\n").is_empty());
    }

    #[test]
    fn json_is_judged_like_python_json_load() {
        for text in [
            r#"{"a": 1}"#,
            "[1, 2.5e3, -0, \"\u{e9}\", true, null]",
            "NaN",
            "-Infinity",
            " \"x\" \n",
            "{}",
            "[]",
            "-0",
            "{\r\n\"a\":\r\n1}\r\n",
        ] {
            assert!(json_valid(text.as_bytes()), "{text:?} should be valid");
        }
        for text in [
            r#"{"a": 1,}"#,
            "",
            "\u{feff}{}",
            "[1,]",
            "{'a': 1}",
            "01",
            "1.",
            "\"tab\tinside\"",
            "{} x",
            "\0",
            "+1",
            ".5",
        ] {
            assert!(!json_valid(text.as_bytes()), "{text:?} should be invalid");
        }
    }

    #[test]
    fn a_paths_key_is_found_like_the_sed_range_does() {
        assert!(is_path_scoped(
            b"---\npaths:\n  - \"**/*.ts\"\n---\n# Scoped\n"
        ));
        assert!(is_path_scoped(b"---\n---\npaths:\n"));
        assert!(is_path_scoped(b"---\npaths:  \n---\n"));
        assert!(!is_path_scoped(b"# Rule\n"));
        assert!(!is_path_scoped(b"---\ndesc: x\n---\npaths:\n"));
        assert!(!is_path_scoped(b"---\npaths: foo\n---\n"));
        assert!(!is_path_scoped(b"---"));
        assert!(!is_path_scoped(b"paths:\n"));
    }

    #[cfg(unix)]
    fn project(files: &[(&str, &str)], dirs: &[&str]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for rel in dirs {
            std::fs::create_dir_all(Path::new(&root).join(rel)).unwrap();
        }
        for (rel, text) in files {
            let path = Path::new(&root).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        (dir, root)
    }

    #[cfg(unix)]
    fn run(root: &str) -> (u8, String, String) {
        let env = Env {
            version: "0.36.0",
            external_roots: None,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = doctor(
            &|| Project::at(root),
            &Style::plain(),
            &env,
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

    #[cfg(unix)]
    #[test]
    fn a_project_without_a_config_reports_like_cmd_doctor() {
        let (_dir, root) = project(&[(".ai/src/AGENTS.md", "# Agents\n")], &[".git"]);
        let (status, out, err) = run(&root);
        assert_eq!(status, 1);
        assert_eq!(err, "");
        let expected = format!(
            "\n  AgentSync Doctor\n  {root}\n\n  Project layout\n    ✓ .ai/ directory present\n    ✓ AGENTS.md source file found\n    ⚠ No agent_sync.yaml — using defaults only\n\n  Enabled tools\n    · No tools enabled — run agentsync enable <slug>\n\n  User overrides\n    · No customizations — all tools inherit fully from base\n\n  Source directories\n    ✓ .ai/src/AGENTS.md\n    · .ai/src/rules not present (optional)\n    · .ai/src/skills not present (optional)\n    · .ai/src/commands not present (optional)\n    · .ai/src/agents not present (optional)\n\n  Drift\n    · No .sync-manifest yet — run agentsync sync to create it\n\n  Security\n    · No overrides to scan, or all clean.\n\n  Skills\n    · No .ai/src/skills/ — nothing to scan.\n\n  Rules\n    · No .ai/src/rules/ — nothing to scan.\n\n  Tool outputs\n    ✓ No orphan tool-output directories\n\n  Cross-project\n    · No parent .ai/src/ found within git boundary.\n\n  {}\n  OK with 1 warning(s)\n\n",
            "─".repeat(60)
        );
        assert_eq!(out, expected);
    }

    #[cfg(unix)]
    #[test]
    fn a_pinned_config_stale_manifest_and_orphan_output_report_like_cmd_doctor() {
        let (_dir, root) = project(
            &[
                (".ai/src/AGENTS.md", "# Agents\n"),
                (
                    ".ai/src/rules/scoped.md",
                    "---\npaths:\n  - \"**/*.ts\"\n---\n# Scoped\n",
                ),
                (
                    ".ai/agent_sync.yaml",
                    "agentsync_version: \"0.0.1\"\nformat: 1\ntools:\n  - cursor\n",
                ),
                (
                    ".ai/.sync-manifest",
                    "CLAUDE.md\t0000000000000000000000000000000000000000000000000000000000000000\n",
                ),
            ],
            &[".git", ".ai/src/skills/empty", ".claude"],
        );
        let (status, out, err) = run(&root);
        assert_eq!(status, 1);
        assert_eq!(err, "");
        let expected = format!(
            "\n  AgentSync Doctor\n  {root}\n\n  Project layout\n    ✓ .ai/ directory present\n    ✓ AGENTS.md source file found\n    ✓ Project config: .ai/agent_sync.yaml\n    ⚠ CLI version v0.36.0 differs from pinned v0.0.1 — run agentsync upgrade-config to align\n    ⚠ Project format r1 is behind the engine r2 — run agentsync migrate to preview\n\n  Enabled tools\n    · No tools enabled — run agentsync enable <slug>\n\n  User overrides\n    · No customizations — all tools inherit fully from base\n\n  Source directories\n    ✓ .ai/src/AGENTS.md\n    ✓ .ai/src/rules\n    ✓ .ai/src/skills\n    · .ai/src/commands not present (optional)\n    · .ai/src/agents not present (optional)\n\n  Drift\n    ⚠ CLAUDE.md — missing (deleted manually)\n\n    Re-run agentsync sync to overwrite, or move edits into .ai/src/ first.\n\n  Security\n    · No overrides to scan, or all clean.\n\n  Skills\n    ⚠ skills/empty/ — missing SKILL.md (empty skill — populate or remove)\n    · Tip: agentsync simplify can prune empty skill dirs.\n\n  Rules\n    ✓ No always-on rules (every rule is paths:-scoped)\n\n  Tool outputs\n    ⚠ .claude/ — orphan (tool 'claude' not enabled; output left from prior run)\n\n  Cross-project\n    · No parent .ai/src/ found within git boundary.\n\n  {}\n  OK with 3 warning(s), 2 advisory(ies)\n\n",
            "─".repeat(60)
        );
        assert_eq!(out, expected);
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_ai_directory_exits_2_like_cmd_doctor() {
        let (_dir, root) = project(&[], &[".git"]);
        let (status, out, err) = run(&root);
        assert_eq!(status, 2);
        assert_eq!(err, "");
        assert_eq!(
            out,
            format!(
                "\n  AgentSync Doctor\n  {root}\n\n  Project layout\n    ✗ .ai/ directory missing — run 'agentsync init'\n\n"
            )
        );
    }
}
```

Run: `cargo test doctor 2>&1 | grep -E '^error' | head -3`
Expected: compile errors naming `scan_secrets`, `json_valid`, `is_path_scoped`, `Env`, and `doctor`.

- [x] **Step 3: Write the implementation**

Prepend to `src/cli/doctor.rs`:

```rust
//! `agentsync doctor`: `cmd_doctor` of `lib/helpers/doctor.sh`, which checks
//! a project's layout, tools, overrides, sources, drift, secrets, skills,
//! rules, tool outputs, and parent duplicates, and exits 0, 1, or 2.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::customize::put;
use crate::manifest::{self, Manifest};
use crate::paths::{self, ExplicitSource, Paths};
use crate::payload::{self, Source};
use crate::project::Project;
use crate::style::Style;
use crate::tool::Tool;
use crate::{
    Error, catalog, convert, edit_paths, format_rev, opencode_json, overlay, template_manifest,
    yaml_subset,
};

/// What `doctor` takes from the process.
pub struct Env<'a> {
    pub version: &'a str,
    pub external_roots: Option<String>,
}

/// `_DOCTOR_SECRET_PATTERNS`, as a prefix and what must follow it.
const SECRET_PATTERNS: [Secret; 7] = [
    Secret::Run("sk-", CharClass::Base64Url, 20),
    Secret::Run("ghp_", CharClass::Alnum, 30),
    Secret::Run("github_pat_", CharClass::AlnumUnderscore, 30),
    Secret::Run("AKIA", CharClass::UpperDigit, 16),
    Secret::Slack,
    Secret::Run("AIza", CharClass::Base64Url, 35),
    Secret::Jwt,
];

/// `_DOCTOR_OUTPUT_DIR_MAP`.
const OUTPUT_DIRS: [(&str, &str); 12] = [
    (".claude", "claude"),
    (".cursor", "cursor"),
    (".codex", "codex"),
    (".kimi-code", "kimi"),
    (".opencode", "opencode"),
    (".windsurf", "windsurf"),
    (".gemini", "gemini"),
    (".junie", "junie"),
    (".cline", "cline"),
    (".amazonq", "amazonq"),
    (".zed", "zed"),
    (".agents", "codex"),
];

#[derive(Clone, Copy)]
enum CharClass {
    /// `[A-Za-z0-9_-]`
    Base64Url,
    /// `[A-Za-z0-9]`
    Alnum,
    /// `[A-Za-z0-9_]`
    AlnumUnderscore,
    /// `[0-9A-Z]`
    UpperDigit,
}

impl CharClass {
    fn matches(self, b: u8) -> bool {
        match self {
            CharClass::Base64Url => b.is_ascii_alphanumeric() || b == b'_' || b == b'-',
            CharClass::Alnum => b.is_ascii_alphanumeric(),
            CharClass::AlnumUnderscore => b.is_ascii_alphanumeric() || b == b'_',
            CharClass::UpperDigit => b.is_ascii_uppercase() || b.is_ascii_digit(),
        }
    }
}

enum Secret {
    /// `<prefix>[class]{min,}`.
    Run(&'static str, CharClass, usize),
    /// `xox[baprs]-[A-Za-z0-9-]{10,}`.
    Slack,
    /// `eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}`.
    Jwt,
}

fn run_len(bytes: &[u8], class: CharClass) -> usize {
    bytes.iter().take_while(|b| class.matches(**b)).count()
}

impl Secret {
    fn found_in(&self, line: &[u8]) -> bool {
        match self {
            Secret::Run(prefix, class, min) => (0..line.len()).any(|i| {
                line[i..].starts_with(prefix.as_bytes())
                    && run_len(&line[i + prefix.len()..], *class) >= *min
            }),
            Secret::Slack => (0..line.len()).any(|i| {
                let rest = &line[i..];
                rest.len() > 5
                    && rest.starts_with(b"xox")
                    && b"baprs".contains(&rest[3])
                    && rest[4] == b'-'
                    && rest[5..]
                        .iter()
                        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'-')
                        .count()
                        >= 10
            }),
            Secret::Jwt => (0..line.len()).any(|i| {
                let rest = &line[i..];
                if !rest.starts_with(b"eyJ") {
                    return false;
                }
                let mut at = 0;
                for part in 0..3 {
                    let len = run_len(&rest[at..], CharClass::Base64Url);
                    if len < if part == 0 { 13 } else { 10 } {
                        return false;
                    }
                    at += len;
                    if part < 2 {
                        if rest.get(at) != Some(&b'.') {
                            return false;
                        }
                        at += 1;
                    }
                }
                true
            }),
        }
    }
}

/// `_doctor_scan_file`: the `grep -n` lines of the first pattern with a hit
/// that is not a placeholder; empty for a clean or binary file.
fn scan_secrets(bytes: &[u8]) -> Vec<String> {
    if bytes.contains(&0) {
        return Vec::new();
    }
    let mut lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    if lines.last() == Some(&&b""[..]) {
        lines.pop();
    }
    for pattern in &SECRET_PATTERNS {
        let mut hits = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if !pattern.found_in(line) {
                continue;
            }
            let text = String::from_utf8_lossy(line);
            if text.contains("${") && text[text.find("${").unwrap_or(0)..].contains('}') {
                continue;
            }
            if text.contains('<')
                && text[text.find('<').unwrap_or(0)..].contains('>')
                && !text.contains("sk-")
            {
                continue;
            }
            hits.push(format!("{}:{text}", index + 1));
        }
        if !hits.is_empty() {
            return hits;
        }
    }
    Vec::new()
}

/// `_doctor_validate_json` as `python3 -c 'json.load(...)'` answers it: RFC
/// 8259 JSON, any top-level value, plus `NaN`, `Infinity`, and `-Infinity`.
fn json_valid(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut p = Json {
        s: text.as_bytes(),
        pos: 0,
    };
    p.ws();
    if !p.value() {
        return false;
    }
    p.ws();
    p.pos == p.s.len()
}

struct Json<'a> {
    s: &'a [u8],
    pos: usize,
}

impl Json<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn literal(&mut self, word: &str) -> bool {
        if self.s[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> bool {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string(),
            Some(b't') => self.literal("true"),
            Some(b'f') => self.literal("false"),
            Some(b'n') => self.literal("null"),
            Some(b'N') => self.literal("NaN"),
            Some(b'I') => self.literal("Infinity"),
            Some(b'-') if self.s[self.pos..].starts_with(b"-Infinity") => self.literal("-Infinity"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => false,
        }
    }

    fn object(&mut self) -> bool {
        self.pos += 1;
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return true;
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') || !self.string() {
                return false;
            }
            self.ws();
            if self.peek() != Some(b':') {
                return false;
            }
            self.pos += 1;
            self.ws();
            if !self.value() {
                return false;
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return true;
                }
                _ => return false,
            }
        }
    }

    fn array(&mut self) -> bool {
        self.pos += 1;
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return true;
        }
        loop {
            self.ws();
            if !self.value() {
                return false;
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return true;
                }
                _ => return false,
            }
        }
    }

    fn string(&mut self) -> bool {
        self.pos += 1;
        loop {
            match self.peek() {
                None => return false,
                Some(b'"') => {
                    self.pos += 1;
                    return true;
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.pos += 1;
                        }
                        Some(b'u') => {
                            self.pos += 1;
                            for _ in 0..4 {
                                if !self.peek().is_some_and(|b| b.is_ascii_hexdigit()) {
                                    return false;
                                }
                                self.pos += 1;
                            }
                        }
                        _ => return false,
                    }
                }
                Some(b) if b < 0x20 => return false,
                Some(_) => self.pos += 1,
            }
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> bool {
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return false,
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return false;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return false;
            }
        }
        true
    }
}

struct External {
    raw: String,
    abs: String,
    refused: bool,
    untrusted: bool,
}

struct Doctor<'a> {
    project: &'a Project,
    root: String,
    config: Option<String>,
    config_shown: String,
    paths: Paths,
    style: &'a Style,
    version: &'a str,
    warnings: usize,
    errors: usize,
    advisories: usize,
    warned_legacy: bool,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl Doctor<'_> {
    fn say(&mut self, text: &str) -> Result<(), Error> {
        put(self.out, text.as_bytes())
    }

    fn ok(&mut self, text: &str) -> Result<(), Error> {
        let line = format!("    {} {text}\n", self.style.green("✓"));
        self.say(&line)
    }

    fn warn(&mut self, text: &str) -> Result<(), Error> {
        self.warnings += 1;
        let line = format!("    {} {text}\n", self.style.yellow("⚠"));
        self.say(&line)
    }

    fn fail(&mut self, text: &str) -> Result<(), Error> {
        self.errors += 1;
        let line = format!("    {} {text}\n", self.style.red("✗"));
        self.say(&line)
    }

    fn info(&mut self, text: &str) -> Result<(), Error> {
        let line = format!("    {} {text}\n", self.style.dim("·"));
        self.say(&line)
    }

    fn advise(&mut self, text: &str) -> Result<(), Error> {
        self.advisories += 1;
        let line = format!("    {} {text}\n", self.style.yellow("⚠"));
        self.say(&line)
    }

    fn heading(&mut self, text: &str) -> Result<(), Error> {
        let line = format!("{}\n", self.style.bold(&format!("  {text}")));
        self.say(&line)
    }

    fn rel(&self, path: &str) -> String {
        path.strip_prefix(&format!("{}/", self.root))
            .unwrap_or(path)
            .to_string()
    }

    /// `_doctor_external_source`.
    fn external_source(&self, key: &str) -> Option<External> {
        let config = self.config.as_deref()?;
        let raw = yaml_subset::value(config, &format!("source.{key}"));
        if raw.is_empty() {
            return None;
        }
        let (refused, untrusted) = match self.paths.classify_explicit_source(&raw) {
            ExplicitSource::Inside => return None,
            ExplicitSource::Outside(_) => (false, false),
            ExplicitSource::Refused(_) => (true, false),
            ExplicitSource::Untrusted(_) => (false, true),
        };
        Some(External {
            abs: self.paths.absolute(&raw),
            raw,
            refused,
            untrusted,
        })
    }

    fn tool(&self, slug: &str) -> Result<Tool, Error> {
        Tool::load(self.project, slug)
    }

    fn display_name(&self, slug: &str) -> Result<String, Error> {
        Ok(self.tool(slug)?.display_name())
    }

    /// `resolve_payload_source` under `[[ -f ]]`: the payload when its file
    /// exists, with the legacy-layout warning on stderr once per run.
    fn resolve(&mut self, tool: &Tool, resource: &str) -> Result<Option<Source>, Error> {
        let (source, legacy) = payload::effective_source(self.project, tool, resource)?;
        if let Some(path) = legacy.filter(|_| !self.warned_legacy) {
            self.warned_legacy = true;
            put(
                self.err,
                payload::legacy_warning(self.project, &path).as_bytes(),
            )?;
        }
        Ok(source.filter(|source| match source {
            Source::Disk(path) => path.is_file(),
            Source::Shipped(_) => true,
        }))
    }

    fn source_shown(&self, source: &Source) -> String {
        self.rel(&source.shown())
    }

    /// `_doctor_check_commands_config`.
    fn check_commands_config(&mut self, tool: &Tool) -> Result<(), Error> {
        let slug = tool.slug.clone();
        let dest = tool.value("targets.commands.dest");
        let as_skills = tool.value("targets.commands.as_skills") == "true";
        let inline = tool.value("targets.commands.inline_into_agents") == "true";
        if !dest.is_empty() && as_skills {
            self.warn(&format!(
                "{slug}: targets.commands.dest and .as_skills both set — dest wins; remove one"
            ))?;
        }
        if !dest.is_empty() && inline {
            self.warn(&format!(
                "{slug}: targets.commands.dest and .inline_into_agents both set — dest wins; remove one"
            ))?;
        }
        if as_skills && inline {
            self.warn(&format!(
                "{slug}: targets.commands.as_skills and .inline_into_agents both true — as_skills wins; remove one"
            ))?;
        }
        if as_skills && tool.value("targets.skills.dest").is_empty() {
            self.warn(&format!(
                "{slug}: targets.commands.as_skills requires targets.skills.dest — option will no-op"
            ))?;
        }
        if inline && tool.value("targets.agents.dest").is_empty() {
            self.warn(&format!(
                "{slug}: targets.commands.inline_into_agents requires targets.agents.dest — option will no-op"
            ))?;
        }
        Ok(())
    }

    /// `_doctor_check_payload_ownership`.
    fn check_payload_ownership(&mut self, tool: &Tool) -> Result<(), Error> {
        match tool.slug.as_str() {
            "opencode" => {
                let settings = self.resolve(tool, "settings")?;
                let mcp = self.resolve(tool, "mcp")?;
                if let (Some(settings), Some(mcp)) = (settings, mcp) {
                    let text = String::from_utf8_lossy(&settings.bytes()?).into_owned();
                    if opencode_json::settings_has_mcp(&text) == Ok(true) {
                        self.fail(&format!(
                            "OpenCode MCP ownership conflict: {} and {} both define mcp. Move the canonical server map into one source.",
                            self.source_shown(&settings),
                            self.source_shown(&mcp)
                        ))?;
                    }
                }
            }
            "kimi" => {
                let mut hook = payload::find_new_override(self.project, "kimi", "hooks")?;
                if hook.is_none() {
                    hook = sorted_entries(&Path::new(&self.root).join(".ai/src/hooks"))
                        .into_iter()
                        .find(|path| {
                            path.is_file()
                                && path
                                    .file_name()
                                    .is_some_and(|n| n.to_string_lossy().starts_with("kimi."))
                        });
                }
                if let Some(path) = hook {
                    let shown = self.rel(&path.to_string_lossy());
                    self.advise(&format!(
                        "Kimi hooks are global-only in $KIMI_CODE_HOME/config.toml; AgentSync leaves it untouched. Remove {shown} from project sources."
                    ))?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// `_doctor_check_guard_wired`.
    fn check_guard_wired(&mut self, tool: &Tool) -> Result<(), Error> {
        if self.resolve(tool, "guard")?.is_none() {
            return Ok(());
        }
        let guard_dest = tool.value("targets.guard.dest");
        let settings_dest = tool.value("targets.settings.dest");
        if guard_dest.is_empty() || settings_dest.is_empty() {
            return Ok(());
        }
        let Some(settings) = self.resolve(tool, "settings")? else {
            return Ok(());
        };
        let guard_name = paths::leaf(&guard_dest);
        let bytes = settings.bytes()?;
        let referenced = bytes
            .windows(guard_name.len())
            .any(|window| window == guard_name.as_bytes());
        if !referenced {
            self.warn(&format!(
                "{}: {guard_dest} is generated but {} never references it — the guard against edits to generated files is inert. Add the hooks block from the shipped base, or delete the override to inherit it.",
                tool.display_name(),
                self.source_shown(&settings)
            ))?;
        }
        Ok(())
    }

    /// `_doctor_check_drift`.
    fn check_drift(&mut self) -> Result<(), Error> {
        let style = self.style;
        let manifest_path = Path::new(&self.root).join(manifest::REL);
        if !manifest_path.is_file() {
            return self.info(&format!(
                "No .sync-manifest yet — run {} to create it",
                style.cyan("agentsync sync")
            ));
        }
        let Some(manifest) = Manifest::load(&self.root)? else {
            return Ok(());
        };
        if manifest.entries().is_empty() {
            return self.info(".sync-manifest is empty");
        }
        let (mut edited, mut missing, mut clean) = (0, 0, 0);
        for (rel, old_hash) in manifest.entries() {
            let dest = Path::new(&self.root).join(rel);
            if !dest.is_file() {
                self.warn(&format!("{rel} — missing (deleted manually)"))?;
                missing += 1;
                continue;
            }
            let Some(current) = template_manifest::hash(&dest) else {
                self.warn(&format!("{rel} — could not hash"))?;
                continue;
            };
            if &current != old_hash {
                self.warn(&format!("{rel} — edited since last sync"))?;
                edited += 1;
            } else {
                clean += 1;
            }
        }
        if edited == 0 && missing == 0 {
            self.ok(&format!("All {clean} tracked file(s) match the manifest"))
        } else {
            self.say(&format!(
                "\n    {} {} {} {} {}\n",
                style.dim("Re-run"),
                style.cyan("agentsync sync"),
                style.dim("to overwrite, or move edits into"),
                style.cyan(".ai/src/"),
                style.dim("first.")
            ))
        }
    }

    /// `_doctor_scan_one_file`; returns (secret hit, invalid JSON).
    fn scan_one_file(&mut self, file: &Path) -> Result<(bool, bool), Error> {
        let style = self.style;
        let shown = self.rel(&file.to_string_lossy());
        let bytes = std::fs::read(file).map_err(|e| Error::io(file, e))?;
        if file.extension().is_some_and(|ext| ext == "json") && !json_valid(&bytes) {
            self.fail(&format!("{shown}: invalid JSON syntax"))?;
            return Ok((false, true));
        }
        let hits = scan_secrets(&bytes);
        if hits.is_empty() {
            return Ok((false, false));
        }
        self.fail(&format!("{shown}: possible secret"))?;
        for hit in hits {
            self.say(&format!("        {}\n", style.dim(&hit)))?;
        }
        Ok((true, false))
    }

    /// `_doctor_scan_overrides`.
    fn scan_overrides(&mut self) -> Result<(), Error> {
        let style = self.style;
        let (mut hits, mut invalid, mut legacy) = (0, 0, 0);
        let tools_root = self.project.user_tools_dir();
        if tools_root.is_dir() {
            for tool_dir in sorted_entries(&tools_root)
                .into_iter()
                .filter(|p| p.is_dir())
            {
                for resource in ["mcp", "settings", "hooks"] {
                    for file in sorted_entries(&tool_dir).into_iter().filter(|p| {
                        p.is_file()
                            && p.file_name().is_some_and(|n| {
                                n.to_string_lossy().starts_with(&format!("{resource}."))
                            })
                    }) {
                        let (hit, bad) = self.scan_one_file(&file)?;
                        hits += usize::from(hit);
                        invalid += usize::from(bad);
                    }
                }
            }
        }
        for resource in ["mcp", "settings", "hooks"] {
            let dir = Path::new(&self.root).join(".ai/src").join(resource);
            if !dir.is_dir() {
                continue;
            }
            for file in sorted_entries(&dir).into_iter().filter(|p| p.is_file()) {
                legacy += 1;
                let (hit, bad) = self.scan_one_file(&file)?;
                hits += usize::from(hit);
                invalid += usize::from(bad);
            }
        }
        if legacy > 0 {
            self.warn(&format!(
                "Legacy payload layout ({legacy} file(s) under .ai/src/{{hooks,mcp,settings}}/). Run {} to move them to .ai/src/tools/<tool>/<resource>.<ext>.",
                style.cyan("agentsync migrate --apply")
            ))?;
        }
        if hits == 0 && invalid == 0 && legacy == 0 {
            self.info("No overrides to scan, or all clean.")?;
        } else if hits > 0 {
            self.say("\n")?;
            self.info(&format!(
                "{}: use ${{ENV_VAR}} placeholders; never commit raw secrets.",
                style.yellow("Reminder")
            ))?;
        }
        Ok(())
    }

    /// `_doctor_check_empty_skills`.
    fn check_empty_skills(&mut self) -> Result<(), Error> {
        let style = self.style;
        let skills = Path::new(&self.root).join(".ai/src/skills");
        if !skills.is_dir() {
            return self.info("No .ai/src/skills/ — nothing to scan.");
        }
        let mut found = 0;
        for dir in sorted_entries(&skills).into_iter().filter(|p| p.is_dir()) {
            if !dir.join("SKILL.md").is_file() {
                let name = dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.advise(&format!(
                    "skills/{name}/ — missing SKILL.md {}",
                    style.dim("(empty skill — populate or remove)")
                ))?;
                found += 1;
            }
        }
        if found == 0 {
            self.ok("All skill directories contain SKILL.md")
        } else {
            self.info(&format!(
                "{} {} {}",
                style.dim("Tip:"),
                style.cyan("agentsync simplify"),
                style.dim("can prune empty skill dirs.")
            ))
        }
    }

    /// `_doctor_check_always_on_rules`.
    fn check_always_on_rules(&mut self) -> Result<(), Error> {
        let style = self.style;
        let rules = Path::new(&self.root).join(".ai/src/rules");
        if !rules.is_dir() {
            return self.info("No .ai/src/rules/ — nothing to scan.");
        }
        let (mut count, mut bytes) = (0usize, 0usize);
        for file in sorted_entries(&rules)
            .into_iter()
            .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "md"))
        {
            let content = std::fs::read(&file).map_err(|e| Error::io(&file, e))?;
            if is_path_scoped(&content) {
                continue;
            }
            count += 1;
            bytes += content.len();
        }
        if count == 0 {
            self.ok("No always-on rules (every rule is paths:-scoped)")
        } else if bytes >= 20000 {
            self.advise(&format!(
                "{count} always-on rule(s) load on every task (~{} KB, ~{} tokens). Add {} frontmatter to domain rules so they load only when matching files are touched — a large always-on set dilutes attention.",
                bytes / 1024,
                bytes / 4,
                style.cyan("paths:")
            ))
        } else {
            self.ok(&format!(
                "Always-on rule context is lean ({count} file(s), ~{} KB)",
                bytes / 1024
            ))
        }
    }

    /// `_doctor_check_orphan_outputs`.
    fn check_orphan_outputs(&mut self) -> Result<(), Error> {
        let style = self.style;
        let enabled = self.project.enabled_tools()?;
        let mut found = 0;
        if Path::new(&self.root).join(".agent").is_dir() {
            self.advise(&format!(
                ".agent/ — legacy pre-v0.6 layout (run {} to preview cleanup)",
                style.cyan("agentsync migrate --legacy")
            ))?;
            found += 1;
        }
        for (dir, tool) in OUTPUT_DIRS {
            if !Path::new(&self.root).join(dir).is_dir() {
                continue;
            }
            if dir == ".agents" && (enabled.contains("codex") || enabled.contains("antigravity")) {
                continue;
            }
            if !enabled.contains(tool) {
                self.advise(&format!(
                    "{dir}/ — orphan (tool '{tool}' not enabled; output left from prior run)"
                ))?;
                found += 1;
            }
        }
        if found == 0 {
            self.ok("No orphan tool-output directories")?;
        }
        Ok(())
    }

    /// `_doctor_check_cross_project`.
    fn check_cross_project(&mut self) -> Result<(), Error> {
        let style = self.style;
        let child_src = format!("{}/.ai/src", self.root);
        if !Path::new(&child_src).is_dir() {
            return self.info("No .ai/src/ in this project — skipping cross-project scan.");
        }
        let mut from_shared = false;
        let parent_src = match self
            .config
            .as_deref()
            .and_then(|config| overlay::shared_parent_src(config, &self.root))
        {
            Some(parent) => {
                from_shared = true;
                Some(parent)
            }
            None => paths::find_parent_ai_src(&self.root),
        };
        let Some(parent_src) = parent_src else {
            return self.info("No parent .ai/src/ found within git boundary.");
        };
        let origin_hint = if from_shared {
            format!(" {}", style.dim("(from shared.path)"))
        } else {
            String::new()
        };
        self.info(&format!(
            "Parent source: {}{origin_hint}",
            style.dim(&parent_src)
        ))?;
        self.say("\n")?;

        let inherited: Vec<&str> = self
            .config
            .as_deref()
            .map(|config| {
                overlay::inherit_categories(&yaml_subset::value(config, "shared.inherit"))
            })
            .unwrap_or_default();
        let parent_root = paths::parent(&parent_src);
        let (mut dupes, mut divergent) = (0, 0);
        let mut pairs: Vec<(String, PathBuf)> = Vec::new();
        for category in ["rules", "commands", "agents"] {
            let dir = Path::new(&parent_src).join(category);
            if !dir.is_dir() {
                continue;
            }
            for file in sorted_entries(&dir)
                .into_iter()
                .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "md"))
            {
                let name = file
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                pairs.push((format!("{category}/{name}"), file));
            }
        }
        let skills = Path::new(&parent_src).join("skills");
        if skills.is_dir() {
            let mut files = Vec::new();
            files_below(&skills, &mut files);
            files.retain(|p| {
                !p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            });
            files.sort();
            for file in files {
                let rel = file
                    .strip_prefix(&parent_src)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                pairs.push((rel, file));
            }
        }
        for (rel, parent_file) in pairs {
            let child_file = Path::new(&child_src).join(&rel);
            if !child_file.is_file() {
                continue;
            }
            let (Some(child_hash), Some(parent_hash)) = (
                template_manifest::hash(&child_file),
                template_manifest::hash(&parent_file),
            ) else {
                continue;
            };
            let category = rel.split('/').next().unwrap_or("");
            if child_hash == parent_hash {
                let hint = if inherited.contains(&category) {
                    format!(" {}", style.dim("(inherited via shared: — safe to delete)"))
                } else {
                    String::new()
                };
                let shown = parent_file
                    .to_string_lossy()
                    .strip_prefix(&format!("{parent_root}/"))
                    .unwrap_or(&parent_file.to_string_lossy())
                    .to_string();
                self.advise(&format!(
                    "{rel} — duplicate of parent's {}{hint}",
                    style.dim(&shown)
                ))?;
                dupes += 1;
            } else {
                let content =
                    std::fs::read(&parent_file).map_err(|e| Error::io(&parent_file, e))?;
                if convert::read_field(&content, "category") == b"governance" {
                    self.advise(&format!(
                        "{rel} — {} {}",
                        style.yellow("governance file diverges from parent"),
                        style.dim("(category: governance — likely a mistake, not an override)")
                    ))?;
                } else {
                    self.info(&format!(
                        "{rel} — diverges from parent {}",
                        style.dim("(review intent)")
                    ))?;
                }
                divergent += 1;
            }
        }
        if dupes == 0 && divergent == 0 {
            self.ok("No source files shared with parent.")
        } else if dupes > 0 {
            self.say("\n")?;
            self.info(&format!(
                "{} {} {}",
                style.dim("Run"),
                style.cyan("agentsync dedupe"),
                style.dim("to remove duplicates interactively.")
            ))
        } else {
            Ok(())
        }
    }
}

/// `sed -n '2,/^---$/p' | grep -q '^paths:[[:space:]]*$'` after a first
/// line of `---`. sed tests the closing address from line 3 on, so a `---`
/// on line 2 does not end the range.
fn is_path_scoped(content: &[u8]) -> bool {
    let mut lines = content.split(|b| *b == b'\n');
    if lines.next() != Some(b"---") {
        return false;
    }
    for (index, line) in lines.enumerate() {
        if line.strip_prefix(b"paths:").is_some_and(|rest| {
            rest.iter()
                .all(|b| matches!(b, b' ' | b'\t' | b'\r' | 0x0b | 0x0c))
        }) {
            return true;
        }
        if index > 0 && line == b"---" {
            return false;
        }
    }
    false
}

/// Directory entries in byte order, as `LC_ALL=C` globs list them.
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

/// `cmd_doctor`: the report on `out`, the tri-state status as the result.
pub fn doctor(
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let project = match discover() {
        Ok(project) => project,
        Err(Error::ConfigPathNotFound(path)) => {
            put(
                err,
                format!(
                    "{}: AGENTSYNC_CONFIG_PATH is set but file not found: {}\n",
                    style.red("Error"),
                    path.display()
                )
                .as_bytes(),
            )?;
            return Ok(2);
        }
        Err(e) => return Err(e),
    };
    let root = project.root.to_string_lossy().into_owned();
    let config = match &project.config_path {
        Some(path) => Some(
            std::fs::read(path)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|e| Error::io(path, e))?,
        ),
        None => None,
    };
    let config_shown = project
        .config_path
        .as_ref()
        .map(|p| {
            let text = p.to_string_lossy();
            text.strip_prefix(&format!("{root}/"))
                .unwrap_or(&text)
                .to_string()
        })
        .unwrap_or_default();
    let mut paths = Paths::on_disk(&root);
    paths.trust_external_roots(env.external_roots.as_deref());
    let mut d = Doctor {
        project: &project,
        root: root.clone(),
        config,
        config_shown,
        paths,
        style,
        version: env.version,
        warnings: 0,
        errors: 0,
        advisories: 0,
        warned_legacy: false,
        out,
        err,
    };

    d.say(&format!(
        "\n{}\n{}\n\n",
        style.bold("  AgentSync Doctor"),
        style.dim(&format!("  {root}"))
    ))?;

    d.heading("Project layout")?;
    if Path::new(&root).join(".ai").is_dir() {
        d.ok(".ai/ directory present")?;
    } else {
        d.fail(".ai/ directory missing — run 'agentsync init'")?;
        d.say("\n")?;
        return Ok(2);
    }
    let agents_found = match d.external_source("agents") {
        Some(external) => Path::new(&external.abs).is_file(),
        None => {
            Path::new(&root).join(".ai/src/AGENTS.md").is_file()
                || Path::new(&root).join(".ai/AGENTS.md").is_file()
        }
    };
    if agents_found {
        d.ok("AGENTS.md source file found")?;
    } else {
        d.fail("No AGENTS.md in .ai/src/ or .ai/ — sync will fail")?;
    }
    if let Some(config) = d.config.clone() {
        let shown = d.config_shown.clone();
        d.ok(&format!("Project config: {}", style.dim(&shown)))?;
        let pinned = yaml_subset::value(&config, "agentsync_version").replace('"', "");
        if !pinned.is_empty() && !d.version.is_empty() && pinned != d.version {
            d.warn(&format!(
                "CLI version {} differs from pinned {} — run {} to align",
                style.dim(&format!("v{}", d.version)),
                style.dim(&format!("v{pinned}")),
                style.cyan("agentsync upgrade-config")
            ))?;
        }
        let engine_rev = format_rev::engine();
        let project_rev = format_rev::project(&config);
        if project_rev < engine_rev {
            d.warn(&format!(
                "Project format {} is behind the engine {} — run {} to preview",
                style.dim(&format!("r{project_rev}")),
                style.dim(&format!("r{engine_rev}")),
                style.cyan("agentsync migrate")
            ))?;
        } else {
            d.ok(&format!(
                "Project format: {}",
                style.dim(&format!("r{project_rev}"))
            ))?;
        }
    } else {
        d.warn("No agent_sync.yaml — using defaults only")?;
    }
    d.say("\n")?;

    d.heading("Enabled tools")?;
    let enabled = project.enabled_tools()?;
    if enabled.is_empty() {
        d.info(&format!(
            "No tools enabled — run {}",
            style.cyan("agentsync enable <slug>")
        ))?;
    } else {
        for slug in &enabled {
            let has_base = catalog::base_tool_yaml(slug).is_some();
            let has_user = project.user_tool_file(slug).is_file();
            if has_base && has_user {
                let display = d.display_name(slug)?;
                d.ok(&format!("{display} {}", style.dim("(customized)")))?;
            } else if has_base {
                let display = d.display_name(slug)?;
                d.ok(&display)?;
            } else if has_user {
                d.warn(&format!(
                    "{slug}: custom tool (no base) — ensure override defines full config"
                ))?;
            } else {
                d.fail(&format!(
                    "{slug}: unknown — no base template and no override"
                ))?;
            }
            let tool = d.tool(slug)?;
            d.check_commands_config(&tool)?;
            d.check_payload_ownership(&tool)?;
            d.check_guard_wired(&tool)?;
        }
    }
    d.say("\n")?;

    if !enabled.is_empty() {
        d.heading("Edit paths")?;
        let known: Vec<String> = {
            let mut all = catalog::base_tools();
            all.extend(project.user_override_tools()?);
            all
        };
        let mut any = false;
        for slug in &enabled {
            if !known.contains(slug) {
                continue;
            }
            let tool = d.tool(slug)?;
            let text = edit_paths::checklist(&project, &tool, style);
            d.say(&text)?;
            any = true;
        }
        if !any {
            d.info("No tools with editable payloads.")?;
        }
        d.say("\n")?;
    }

    d.heading("User overrides")?;
    let overrides = project.user_override_tools()?;
    if overrides.is_empty() {
        d.info("No customizations — all tools inherit fully from base")?;
    } else {
        let configured = project.configured_enabled_tools()?;
        for slug in &overrides {
            let user_file = project.user_tool_file(slug);
            let text = std::fs::read(&user_file)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|e| Error::io(&user_file, e))?;
            if yaml_subset::value(&text, "enabled") == "true" && !configured.contains(slug) {
                d.warn(&format!(
                    "{slug}: uses legacy 'enabled: true' — migrate with {}",
                    style.cyan(&format!("agentsync enable {slug}"))
                ))?;
            } else if catalog::base_tool_yaml(slug).is_some() {
                let display = d.display_name(slug)?;
                d.info(&format!(
                    "{display} — see {}",
                    style.cyan(&format!("agentsync diff {slug}"))
                ))?;
            } else {
                d.info(&format!("{slug} (custom tool, no base)"))?;
            }
        }
    }
    d.say("\n")?;

    d.heading("Source directories")?;
    for (src, key) in [
        ("AGENTS.md", "agents"),
        ("rules", "rules"),
        ("skills", "skills"),
        ("commands", "commands"),
        ("agents", "subagents"),
    ] {
        let (display, abs) = match d.external_source(key) {
            Some(external) => {
                if external.refused {
                    d.fail(&format!(
                        "source.{key} must not be the filesystem root, the home directory, or the project root or its ancestor: {}",
                        external.raw
                    ))?;
                    continue;
                }
                if external.untrusted {
                    d.fail(&format!(
                        "source.{key} points outside the project and AGENTSYNC_EXTERNAL_SOURCE_ROOTS does not list it: {}",
                        external.raw
                    ))?;
                    continue;
                }
                (external.raw, external.abs)
            }
            None => (format!(".ai/src/{src}"), format!("{root}/.ai/src/{src}")),
        };
        if Path::new(&abs).exists() {
            d.ok(&display)?;
        } else if src == "AGENTS.md" {
            d.fail(&format!("{display} missing (required)"))?;
        } else {
            d.info(&format!("{display} not present (optional)"))?;
        }
    }
    d.say("\n")?;

    d.heading("Drift")?;
    d.check_drift()?;
    d.say("\n")?;

    d.heading("Security")?;
    d.scan_overrides()?;
    d.say("\n")?;

    d.heading("Skills")?;
    d.check_empty_skills()?;
    d.say("\n")?;

    d.heading("Rules")?;
    d.check_always_on_rules()?;
    d.say("\n")?;

    d.heading("Tool outputs")?;
    d.check_orphan_outputs()?;
    d.say("\n")?;

    d.heading("Cross-project")?;
    d.check_cross_project()?;
    d.say("\n")?;

    d.say(&format!("  {}\n", "─".repeat(60)))?;
    let advisory_label = if d.advisories > 0 {
        format!(
            ", {}",
            style.dim(&format!("{} advisory(ies)", d.advisories))
        )
    } else {
        String::new()
    };
    if d.errors > 0 {
        d.say(&format!(
            "  {}, {}{advisory_label}\n\n",
            style.red(&format!("{} error(s)", d.errors)),
            style.yellow(&format!("{} warning(s)", d.warnings))
        ))?;
        Ok(2)
    } else if d.warnings > 0 {
        d.say(&format!(
            "  {} with {}{advisory_label}\n\n",
            style.green("OK"),
            style.yellow(&format!("{} warning(s)", d.warnings))
        ))?;
        Ok(1)
    } else if d.advisories > 0 {
        d.say(&format!(
            "  {} with {}\n\n",
            style.green("OK"),
            style.dim(&format!("{} advisory(ies)", d.advisories))
        ))?;
        Ok(0)
    } else {
        d.say(&format!("  {}\n\n", style.green("All checks passed.")))?;
        Ok(0)
    }
}
```

In `src/main.rs`, before the `init` block:

```rust
    if args.first().and_then(|a| a.to_str()) == Some("doctor") {
        let env = cli::doctor::Env {
            version: engine_version(),
            external_roots: var("AGENTSYNC_EXTERNAL_SOURCE_ROOTS"),
        };
        return cli::doctor::doctor(
            &Project::discover,
            &Style::for_stdout(),
            &env,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
```

In `bin/agentsync.sh:280` append `doctor`:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check sync rollback enable disable customize show diff simplify resolve upgrade-config profile dedupe adopt migrate refresh init doctor "
```

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in doctor drift guard format_migration migrate shared source_overrides config_safety; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
bats --tap -f 'parity: doctor' tests/native_parity.bats
```

Expected: `263 passed`, `0`, `11`, `1`; `0` for every file; `ok 1` and `ok 2`.

- [x] **Step 5: Prove the fixture bites, run the reference, lint, commit**

Change `All checks passed.` to `All checks passed` in `src/cli/doctor.rs`, rebuild, rerun `bats --tap -f 'parity: doctor reports' tests/native_parity.bats`: `not ok 1` with the summary line in the diff; revert and rebuild.

Recreate the harnesses when the session scratchpad no longer holds `phase4j/`. The reference takes `<engine 0|1|2> <repo root> <out file>` (mode 2 calls the debug binary directly, for a command the dispatcher does not delegate yet) and masks the project, the templates path, and the work directory; the probes take `<repo root> <out dir>`.

`phase4j/doctor_reference.sh`:

```bash
#!/usr/bin/env bash
# Usage: doctor_reference.sh <engine 0|1> <repo root> <out file>
# Runs every doctor branch in fresh projects and prints status and masked output.
set -uo pipefail
MODE="$1"; REPO="$2"; OUT="$3"
S="$(cd "$(dirname "$0")" && pwd)"
WORK="$S/work_$MODE"
rm -rf "$WORK"
mkdir -p "$WORK"
: > "$OUT"
export AGENTSYNC_HOME="$REPO"
export AGENTSYNC_NATIVE="$MODE"
export AGENTSYNC_NATIVE_BIN="$REPO/target/release/agentsync"
unset AGENTSYNC_REPO_ROOT AGENTSYNC_CONFIG_PATH AGENTSYNC_EXTERNAL_SOURCE_ROOTS
N=0
P=""
fresh() {
    N=$((N + 1))
    P="$WORK/p$N"
    mkdir -p "$P"
    (cd "$P" && git init --quiet && git config user.email t@t && git config user.name T)
    cd "$P" || exit 1
}
bash_init() { AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" init --no-detect --yes --no-sync "$@" >/dev/null 2>&1; }
bash_cmd() { AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" "$@" >/dev/null 2>&1; }
mask() {
    local text; text=$(cat)
    local phys; phys=$(cd -P "$P" && pwd)
    text=${text//"$phys"/<root>}
    text=${text//"$P"/<root>}
    local from
    for from in "$REPO/lib/templates" "~/${REPO#"$HOME"/}/lib/templates" "/<agentsync>/lib/templates"; do
        text=${text//"$from"/<engine>/lib/templates}
    done
    text=${text//"$WORK"/<work>}
    printf '%s\n' "$text"
}
report() {
    local name="$1" rc="$2" output="$3"
    {
        printf '### %s\n' "$name" | mask
        echo "rc=$rc"
        printf '%s' "$output" | mask
        echo
    } >> "$OUT"
}
# Mode 2 calls the debug binary directly, for a command the dispatcher does
# not delegate yet.
engine() {
    if [[ "$MODE" == 2 ]]; then
        AGENTSYNC_ENGINE_VERSION="$(cat "$REPO/VERSION")" "$REPO/target/debug/agentsync" "$@"
    else
        bash "$REPO/bin/agentsync.sh" "$@"
    fi
}
run() {
    local name="$1"; shift
    local rc=0 output
    output=$(cd "$P" && engine "$@" 2>&1) || rc=$?
    report "$name :: agentsync $*" "$rc" "$output"
}
run_in() {
    local name="$1" dir="$2"; shift 2
    local rc=0 output
    output=$(cd "$P/$dir" && engine "$@" 2>&1) || rc=$?
    report "$name :: (in $dir) agentsync $*" "$rc" "$output"
}
run_env() {
    local name="$1" assignment="$2"; shift 2
    local rc=0 output
    output=$(cd "$P" && export "$assignment" && engine "$@" 2>&1) || rc=$?
    report "$name :: $assignment agentsync $*" "$rc" "$output"
}

fresh; run "no-ai" doctor
mkdir -p .ai; run_in "inside-ai" .ai doctor
fresh; bash_init; run "fresh" doctor
run "extra-args-ignored" doctor --bogus extra
run_env "config-path-missing" AGENTSYNC_CONFIG_PATH=missing.yaml doctor
rm -f .ai/agent_sync.yaml; run "no-config" doctor

fresh; bash_init; bash_cmd enable claude; run "enabled-claude" doctor
bash_cmd customize claude; run "customized-claude" doctor
fresh; bash_init; bash_cmd enable claude cursor kimi windsurf; echo '{"mcpServers":{}}' > .ai/src/mcp.json; run "edit-paths-shared-mcp" doctor
fresh; bash_init; mkdir -p .ai/src/tools; printf 'enabled: true\n' > .ai/src/tools/claude.yaml; run "legacy-enabled" doctor
fresh; bash_init; printf 'tools:\n  enabled:\n    - bogus\n    - mytool\n' > .ai/agent_sync.yaml; mkdir -p .ai/src/tools; printf 'name: "My Tool"\n' > .ai/src/tools/mytool.yaml; run "unknown-and-custom" doctor
fresh; bash_init; sed 's/^agentsync_version:.*/agentsync_version: "0.0.1"/' .ai/agent_sync.yaml > .ai/a.tmp && mv .ai/a.tmp .ai/agent_sync.yaml; run "pinned-differs" doctor
fresh; bash_init; sed 's/^format:.*/format: 1/' .ai/agent_sync.yaml > .ai/a.tmp && mv .ai/a.tmp .ai/agent_sync.yaml; run "format-behind" doctor
fresh; bash_init; sed '/^format:/d' .ai/agent_sync.yaml > .ai/a.tmp && mv .ai/a.tmp .ai/agent_sync.yaml; run "format-absent" doctor

fresh; bash_init; rm -f .ai/src/AGENTS.md; rm -rf .ai/src/rules; run "agents-missing-rules-gone" doctor
fresh; bash_init; EXT="$WORK/ext$N"; mkdir -p "$EXT/rules"; printf 'format: 2\ntools:\n  enabled: []\nsource:\n  agents: "%s/AGENTS.md"\n  rules: "%s/rules"\n' "$EXT" "$EXT" > .ai/agent_sync.yaml
run "external-untrusted" doctor
run_env "external-trusted-missing" "AGENTSYNC_EXTERNAL_SOURCE_ROOTS=$EXT" doctor
printf '# External\n' > "$EXT/AGENTS.md"; run_env "external-trusted-present" "AGENTSYNC_EXTERNAL_SOURCE_ROOTS=$EXT" doctor
fresh; bash_init; printf 'format: 2\ntools:\n  enabled:\n    - mytool\n' > .ai/agent_sync.yaml; mkdir -p .ai/src/tools; printf 'name: "My Tool"\ntargets:\n  commands:\n    as_skills: true\n    inline_into_agents: true\n' > .ai/src/tools/mytool.yaml; run "commands-noops-custom" doctor
fresh; bash_init; printf 'format: 2\ntools:\n  enabled: []\nsource:\n  rules: "/"\n  skills: "%s"\n' "$HOME" > .ai/agent_sync.yaml; run "external-refused" doctor

fresh; bash_init; mkdir -p .ai/src/tools/claude .ai/src/tools/cursor .ai/src/tools/zed
printf '{"mcpServers":{"gh":{"env":{"TOKEN":"ghp_abcdefghijklmnopqrstuvwxyz012345678901"}}}}\n' > .ai/src/tools/claude/mcp.json
printf '{"aws":{"key":"AKIAIOSFODNN7EXAMPLE"},"slack":"xoxb-1234567890-abc","g":"AIzaSyA1234567890abcdefghijklmnopqrstuv","jwt":"eyJabcdefghijk.eyJabcdefghijk.abcdefghijklmn","pat":"github_pat_abcdefghijklmnopqrstuvwxyz0123456789"}\n' > .ai/src/tools/claude/settings.json
printf '{"ok":"${GITHUB_TOKEN}","also":"<YOUR_KEY_HERE>","sk":"<sk-abcdefghijklmnopqrstuvwxyz>"}\n' > .ai/src/tools/cursor/mcp.json
printf '{"broken":\n' > .ai/src/tools/cursor/hooks.json
printf '{"a": NaN}\n' > .ai/src/tools/zed/settings.json
run "secrets-and-json" doctor
fresh; bash_init; mkdir -p .ai/src/tools/cursor; printf '{"a": 1,}\n' > .ai/src/tools/cursor/mcp.json; printf '\0binary sk-abcdefghijklmnopqrstuvwxyz\n' > .ai/src/tools/cursor/hooks.json; : > .ai/src/tools/cursor/settings.json; run "json-variants" doctor
fresh; bash_init; mkdir -p .ai/src/mcp .ai/src/settings; printf '{"mcpServers":{}}\n' > .ai/src/mcp/cursor.json; printf 'sk-abcdefghijklmnopqrstuvwxyz\n' > .ai/src/settings/claude.toml; run "legacy-layout" doctor

fresh; bash_init; bash_cmd enable claude; mkdir -p .ai/src/tools; printf 'targets:\n  commands:\n    dest: ".claude/cmds"\n    as_skills: true\n    inline_into_agents: true\n' > .ai/src/tools/claude.yaml; run "commands-conflicts" doctor
fresh; bash_init; bash_cmd enable claude; mkdir -p .ai/src/tools; printf 'targets:\n  skills:\n    dest: ""\n  agents:\n    dest: ""\n  commands:\n    as_skills: true\n    inline_into_agents: true\n' > .ai/src/tools/claude.yaml; run "commands-noops" doctor
fresh; bash_init; bash_cmd enable claude; mkdir -p .ai/src/tools/claude; printf '{"permissions":{}}\n' > .ai/src/tools/claude/settings.json; run "guard-not-wired" doctor
fresh; bash_init; bash_cmd enable claude; mkdir -p .ai/src/settings; printf '{"hooks":{"agentsync-guard.sh":1}}\n' > .ai/src/settings/claude.json; run "guard-legacy-settings" doctor
fresh; bash_init; bash_cmd enable opencode; mkdir -p .ai/src/tools/opencode; printf '{"mcp":{}}\n' > .ai/src/tools/opencode/settings.json; printf '{"mcpServers":{}}\n' > .ai/src/mcp.json; run "opencode-conflict" doctor
fresh; bash_init; bash_cmd enable kimi; mkdir -p .ai/src/tools/kimi; printf '[hooks]\n' > .ai/src/tools/kimi/hooks.toml; run "kimi-hooks-per-tool" doctor
fresh; bash_init; bash_cmd enable kimi; mkdir -p .ai/src/hooks; printf '[hooks]\n' > .ai/src/hooks/kimi.toml; run "kimi-hooks-legacy" doctor

fresh; bash_init; bash_cmd enable claude; bash_cmd sync; run "drift-clean" doctor
echo edit >> CLAUDE.md; rm -f .claude/rules/core.md; run "drift-edited-missing" doctor
: > .ai/.sync-manifest; run "manifest-empty" doctor

fresh; bash_init; mkdir -p .ai/src/skills/empty-one .ai/src/skills/Zeta; run "empty-skills" doctor
fresh; bash_init; rm -rf .ai/src/skills .ai/src/rules; run "no-skills-no-rules" doctor
fresh; bash_init; rm -f .ai/src/rules/*.md; big=$(printf 'x%.0s' $(seq 1 6000)); for i in 1 2 3 4; do printf '# Rule %s\n\n- %s\n' "$i" "$big" > ".ai/src/rules/bloat-$i.md"; done; run "rules-bloat" doctor
fresh; bash_init; rm -f .ai/src/rules/*.md; printf -- '---\npaths:\n  - "**/*.ts"\n---\n\n# Scoped\n' > .ai/src/rules/scoped.md; run "rules-all-scoped" doctor

fresh; bash_init; mkdir -p .cursor/rules .agent/rules .agents/skills .claude .codex .kimi-code .opencode .windsurf .gemini .junie .cline .amazonq .zed; run "orphans-all" doctor
fresh; bash_init; bash_cmd enable codex antigravity; mkdir -p .agents/skills .agent; run "agents-dir-owned" doctor

fresh; mkdir -p parent/child; (cd parent && git init --quiet && bash_init); echo "shared" > parent/.ai/src/rules/shared.md; printf -- '---\ncategory: governance\n---\n# Gov\n' > parent/.ai/src/rules/gov.md; mkdir -p parent/.ai/src/skills/dup; echo "skill" > parent/.ai/src/skills/dup/SKILL.md
(cd parent/child && bash_init); cp parent/.ai/src/rules/shared.md parent/child/.ai/src/rules/shared.md; printf -- '---\ncategory: governance\n---\n# Gov child\n' > parent/child/.ai/src/rules/gov.md; echo "child" > parent/child/.ai/src/rules/core.md; mkdir -p parent/child/.ai/src/skills/dup; echo "skill" > parent/child/.ai/src/skills/dup/SKILL.md
run_in "cross-project-walk" parent/child doctor
printf '\nshared:\n  path: "../"\n  inherit: rules\n' >> parent/child/.ai/agent_sync.yaml; run_in "cross-project-shared-inherit" parent/child doctor
fresh; mkdir -p outer/inner; (cd outer && git init --quiet && bash_init); echo "dupe" > outer/.ai/src/rules/shared.md; (cd outer/inner && git init --quiet && bash_init); cp outer/.ai/src/rules/shared.md outer/inner/.ai/src/rules/shared.md
run_in "cross-project-git-boundary" outer/inner doctor
printf '\nshared:\n  path: "../"\n  inherit: rules\n' >> outer/inner/.ai/agent_sync.yaml; run_in "cross-project-shared-across-git" outer/inner doctor
```

`phase4j/legacy_probe.sh`:

```bash
#!/usr/bin/env bash
# Usage: legacy_probe.sh <repo> <out dir>: doctor on a project whose only
# claude settings live in the legacy flat layout, both engines, stdout and
# stderr apart.
set -uo pipefail
REPO="$1"; OUT="$2"
P="$OUT/p"
if [[ -d "$OUT" ]]; then rm -r "$OUT"; fi
mkdir -p "$P"
export AGENTSYNC_HOME="$REPO"
unset AGENTSYNC_REPO_ROOT AGENTSYNC_CONFIG_PATH AGENTSYNC_EXTERNAL_SOURCE_ROOTS
git -C "$P" init --quiet
(cd "$P" && AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" init --no-detect --yes --no-sync >/dev/null 2>&1)
(cd "$P" && AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" enable claude >/dev/null 2>&1)
rm "$P/.ai/src/tools/claude/settings.json"
mkdir -p "$P/.ai/src/settings"
printf '{"hooks":{}}\n' > "$P/.ai/src/settings/claude.json"
(cd "$P" && AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" doctor > "$OUT/bash.out" 2> "$OUT/bash.err"); echo "bash rc=$?"
(cd "$P" && AGENTSYNC_ENGINE_VERSION="$(cat "$REPO/VERSION")" "$REPO/target/debug/agentsync" doctor > "$OUT/rust.out" 2> "$OUT/rust.err"); echo "rust rc=$?"
diff "$OUT/bash.out" "$OUT/rust.out" && echo "stdout same"
diff "$OUT/bash.err" "$OUT/rust.err" && echo "stderr same"
echo "--- stderr:"
cat "$OUT/bash.err"
```

`phase4j/minimal_probe.sh`:

```bash
#!/usr/bin/env bash
# Usage: minimal_probe.sh <repo> <out dir>: doctor on the smallest project the
# unit test builds (a .git marker and .ai/src/AGENTS.md), both engines.
set -uo pipefail
REPO="$1"; OUT="$2"
P="$OUT/p"
if [[ -d "$OUT" ]]; then rm -r "$OUT"; fi
mkdir -p "$P/.git" "$P/.ai/src"
printf '# Agents\n' > "$P/.ai/src/AGENTS.md"
export AGENTSYNC_HOME="$REPO"
unset AGENTSYNC_REPO_ROOT AGENTSYNC_CONFIG_PATH AGENTSYNC_EXTERNAL_SOURCE_ROOTS
(cd "$P" && AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" doctor > "$OUT/bash.out" 2> "$OUT/bash.err"); echo "bash rc=$?"
(cd "$P" && AGENTSYNC_ENGINE_VERSION="$(cat "$REPO/VERSION")" "$REPO/target/debug/agentsync" doctor > "$OUT/rust.out" 2> "$OUT/rust.err"); echo "rust rc=$?"
diff "$OUT/bash.out" "$OUT/rust.out" && echo "stdout same"
diff "$OUT/bash.err" "$OUT/rust.err" && echo "stderr same"
echo "--- bash stdout:"
cat "$OUT/bash.out"
echo "--- second fixture: config pinned to another version, claude enabled, a stale manifest entry"
P2="$OUT/p2"
mkdir -p "$P2/.git" "$P2/.ai/src/rules" "$P2/.ai/src/skills/empty" "$P2/.claude"
printf '# Agents\n' > "$P2/.ai/src/AGENTS.md"
printf -- '---\npaths:\n  - "**/*.ts"\n---\n# Scoped\n' > "$P2/.ai/src/rules/scoped.md"
printf 'agentsync_version: "0.0.1"\nformat: 1\ntools:\n  - cursor\n' > "$P2/.ai/agent_sync.yaml"
printf 'CLAUDE.md\t0000000000000000000000000000000000000000000000000000000000000000\n' > "$P2/.ai/.sync-manifest"
(cd "$P2" && AGENTSYNC_NATIVE=0 bash "$REPO/bin/agentsync.sh" doctor > "$OUT/bash2.out" 2> "$OUT/bash2.err"); echo "bash rc=$?"
(cd "$P2" && AGENTSYNC_ENGINE_VERSION="$(cat "$REPO/VERSION")" "$REPO/target/debug/agentsync" doctor > "$OUT/rust2.out" 2> "$OUT/rust2.err"); echo "rust rc=$?"
diff "$OUT/bash2.out" "$OUT/rust2.out" && echo "stdout same"
diff "$OUT/bash2.err" "$OUT/rust2.err" && echo "stderr same"
echo "--- bash stdout 2:"
cat "$OUT/bash2.out"
echo "--- bash stderr 2:"
cat "$OUT/bash2.err"
```

`phase4j/helper_probe.sh`:

```bash
#!/usr/bin/env bash
# Usage: helper_probe.sh <repo> <work dir>: the Bash answers the doctor unit
# tests assert — _doctor_scan_file, python's json.load, the sed paths: test.
set -uo pipefail
REPO="$1"; W="$2"
if [[ -d "$W" ]]; then rm -r "$W"; fi
mkdir -p "$W"
# shellcheck source=/dev/null
source "$REPO/lib/helpers/cli_colors.sh"
# shellcheck source=/dev/null
source "$REPO/lib/helpers/yaml.sh"
# shellcheck source=/dev/null
source "$REPO/lib/helpers/doctor.sh"
scan() {
    local name="$1"
    printf '%s' "$2" > "$W/$name"
    local out rc=0
    out=$(_doctor_scan_file "$W/$name") || rc=$?
    printf 'scan %s rc=%s\n%s\n--\n' "$name" "$rc" "$out"
}
scan mcp $'{"mcpServers":{"gh":{"env":{"TOKEN":"ghp_abcdefghijklmnopqrstuvwxyz012345678901"}}}}\n'
scan many $'{"aws":{"key":"AKIAIOSFODNN7EXAMPLE"},"slack":"xoxb-1234567890-abc","g":"AIzaSyA1234567890abcdefghijklmnopqrstuv","jwt":"eyJabcdefghijk.eyJabcdefghijk.abcdefghijklmn","pat":"github_pat_abcdefghijklmnopqrstuvwxyz0123456789"}\n'
scan two $'AKIAIOSFODNN7EXAMPLE\nsk-abcdefghijklmnopqrstuvwxyz\n'
scan placeholder $'token: ${GITHUB_TOKEN} ghp_abcdefghijklmnopqrstuvwxyz012345678901\n'
scan angle $'<ghp_abcdefghijklmnopqrstuvwxyz012345678901>\n'
scan angle_sk $'<sk-abcdefghijklmnopqrstuvwxyz>\n'
scan binary $'\0binary sk-abcdefghijklmnopqrstuvwxyz\n'
scan short $'sk-short\nxoxb-123\nAKIA1234\n'
scan slack_then_jwt $'first xoxp-abcdefghij-k\nsecond eyJabcdefghijk.eyJabcdefghijk.abcdefghijklmn\n'
scan placeholder_then_hit $'a ${X} ghp_abcdefghijklmnopqrstuvwxyz012345678901\nb AKIAIOSFODNN7EXAMPLE\n'
json() {
    local name="$1"
    printf '%s' "$2" > "$W/$name.json"
    if _doctor_validate_json "$W/$name.json"; then echo "json $name valid"; else echo "json $name invalid"; fi
}
json obj '{"a": 1}'
json arr '[1, 2.5e3, -0, "é", true, null]'
json nan 'NaN'
json neginf '-Infinity'
json str $' "x" \n'
json empty_obj '{}'
json empty_arr '[]'
json trailing_comma '{"a": 1,}'
json empty ''
json bom $'\xef\xbb\xbf{}'
json arr_comma '[1,]'
json single_quotes "{'a': 1}"
json leading_zero '01'
json bare_dot '1.'
json control $'"tab\tinside"'
json extra '{} x'
json nul $'\0'
json plus '+1'
json dot5 '.5'
json neg_zero '-0'
json crlf $'{\r\n"a":\r\n1}\r\n'
scoped() {
    local name="$1"
    printf '%s' "$2" > "$W/$name.md"
    local first
    IFS= read -r first < "$W/$name.md" || true
    if [[ "$first" == "---" ]] && sed -n '2,/^---$/p' "$W/$name.md" | grep -q '^paths:[[:space:]]*$'; then
        echo "scoped $name yes"
    else
        echo "scoped $name no"
    fi
}
scoped normal $'---\npaths:\n  - "**/*.ts"\n---\n# Scoped\n'
scoped none $'# Rule\n'
scoped empty_front $'---\n---\npaths:\n'
scoped after_close $'---\ndesc: x\n---\npaths:\n'
scoped inline $'---\npaths: foo\n---\n'
scoped trailing_space $'---\npaths:  \n---\n'
scoped no_newline_first $'---'
scoped no_frontmatter_paths $'paths:\n'
command -v python3 >/dev/null && echo "validator: python3 $(python3 --version 2>&1)"
```

`phase4j/opencode_probe.sh`:

```bash
#!/usr/bin/env bash
# Usage: opencode_probe.sh <repo> <work dir>: what opencode_settings_has_mcp
# answers for a settings file with mcp, without it, and malformed.
set -uo pipefail
REPO="$1"; W="$2"
if [[ -d "$W" ]]; then rm -r "$W"; fi
mkdir -p "$W"
# shellcheck source=/dev/null
source "$REPO/lib/helpers/cli_colors.sh"
# shellcheck source=/dev/null
source "$REPO/lib/helpers/tmp.sh"
# shellcheck source=/dev/null
source "$REPO/lib/helpers/opencode.sh"
tmp_prime_run_dir
probe() {
    printf '%s' "$2" > "$W/$1.json"
    local rc=0 out
    out=$(opencode_settings_has_mcp "$W/$1.json" 2>&1) || rc=$?
    printf '%s rc=%s out=[%s]\n' "$1" "$rc" "$out"
}
probe with_mcp '{"mcp": {"srv": {"type": "local"}}, "theme": "x"}'
probe without '{"theme": "x", "mcpServers": {}}'
probe nested '{"a": {"mcp": {}}}'
probe malformed '{"mcp": '
probe empty ''
```

```bash
bash phase4j/doctor_reference.sh 0 "$PWD" phase4j/ref_bash.out && bash phase4j/doctor_reference.sh 1 "$PWD" phase4j/ref_native.out
wc -l < phase4j/ref_native.out
diff phase4j/ref_bash.out phase4j/ref_native.out | grep -c '^[<>]'
bash phase4j/legacy_probe.sh "$PWD" phase4j/legacy_probe | grep -c same
bash phase4j/minimal_probe.sh "$PWD" phase4j/minimal_probe | grep -c same
```

Expected: `2064` lines; `2` differing lines, the `empty-skills` scenario's `skills/empty-one/` advisory before and after `skills/Zeta/` (decision 3); `2` and `4`, stdout and stderr identical, the legacy-layout warning on stderr once.

```bash
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/doctor.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-16-rust-migration-phase-4j-doctor.md
git commit -m "feat(native): port doctor"
```

---

### Task 4: Verify, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [x] **Step 1: Spec**

Append to "Known quirks":

```markdown
44. `doctor`'s secret scan lists only the lines of the first pattern with a
    hit, in pattern order, so a file with an AWS key on line 1 and an OpenAI
    key on line 2 reports only line 2.
45. `doctor` never reports a line holding `${…}` anywhere, or `<…>` without
    `sk-`, however real the key beside the placeholder.
```

Append to "Accepted deviations":

```markdown
- Phase 4j: `doctor` validates JSON in-process as `python3 -c 'json.load'`
  judges it (`NaN` and `Infinity` accepted; a BOM, trailing commas, control
  characters, and an empty file rejected), where Bash's verdict depended on
  `python3`, `node`, or neither being installed.
- Phase 4j: `doctor` lists skill directories, rules, overrides, and parent
  files in byte order (the Phase 2 deviation), so `skills/Zeta/` precedes
  `skills/empty-one/` where the locale's glob put it after.
```

- [x] **Step 2: Module map and outputs**

Set the `lib/helpers/doctor.sh` row to `→ src/cli/doctor.rs       Phase 4j, ported; exit 0/1/2 = clean/warnings/errors`, the `lib/helpers/edit_paths.sh` row to `→ src/edit_paths.rs       block for enable (Phase 4a); checklist for doctor (4j)`, the `lib/helpers/opencode.sh` row to `→ src/opencode_json.rs    awk composer, exit codes 20-26; settings_has_mcp (4j)`, and append `, entries (4j)` inside the `lib/helpers/manifest.sh` row's `src/manifest.rs` list. Regenerate outputs outside the sandbox with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [x] **Step 3: Verify (outside the agent sandbox)**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
bash phase4i/native_suite.sh "$PWD" both phase4j/suite_both.out && tail -1 phase4j/suite_both.out
```

Expected: `263 passed`, `0`, `11`, `1`; lint exit 0; `TOTAL bash=0 native=0` over the 49 bats files, each run one at a time under both engines.

- [x] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-16-rust-migration-phase-4j-doctor.md
git commit -m "docs(native): map the phase 4j modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, every bats file is green under both engines, the two parity fixtures pass, and a `## Completion receipt` records the fresh verification. The next plans cover the standalone commands the spec still names: `add`, `export`, `import`, `generate`, `shell-init`, and `setup-hooks`.

## Run log

### 2026-09-16 — Phase 4j planned
- Commits: this plan.
- Verified: every branch of `cmd_doctor` was captured with `doctor_reference.sh` (43 scenarios), the legacy-layout warning and the smallest fixtures with `legacy_probe.sh` and `minimal_probe.sh` (stdout and stderr apart), and the helper answers with `helper_probe.sh` and `opencode_probe.sh`. The reference turned up one Bash portability bug: the summary rule is built with `tr ' ' '─'`, which maps the indent too and garbles the rule under GNU `tr`; the regression test fails on the committed engine. The Rust in Tasks 2–3 was drafted in the tree: `cargo test` 263/0/11/1, fmt and clippy clean; the debug binary, called directly, gave a 2064-line transcript identical to Bash apart from the rule (Task 1) and the byte-order skills listing (decision 3), and identical stdout and stderr on both probes. Baseline `cargo test` 255/0/11/1; `doctor.bats` 36 and `native_parity.bats` 56 cases green in Bash; the 4i suite ran 49 files green under both engines on the tree the 4j draft sits on.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.
