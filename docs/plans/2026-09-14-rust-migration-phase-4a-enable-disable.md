# Rust Migration Phase 4a: Native `enable` and `disable`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync enable` and `agentsync disable` so the binary answers them byte for byte like `lib/helpers/enable.sh`, after fixing the two `yaml_list_remove` bugs a fixture found in Bash.

**Architecture:** `src/yaml_edit.rs` mirrors the list and scalar editors of `lib/helpers/yaml_edit.sh` as pure text functions plus file wrappers that write through `staging::write_beside`. `payload::override_path` and `src/edit_paths.rs` mirror `_payload_override_path` and the enable block of `lib/helpers/edit_paths.sh`. `src/cli/enable.rs` holds both commands, writing progressively to the writers `main` hands it so a scaffold prompt on the terminal lands between the lines it belongs to. `main` passes the raw arguments, because clap consumes a leading `--` that `cmd_enable` treats as the start of tool slugs. The seam stays the CLI process boundary: `tests/enable.bats` under `AGENTSYNC_NATIVE=1` plus parity fixtures.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-14-rust-migration-phase-3b-rollback-witness.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `enable` writes only the project config it resolves and scaffolded payload copies under the tool override directory; `disable` writes only the config and legacy `enabled:` flags.
- No binary ships to users; without a binary every command runs in Bash. The only Bash change is Task 1's fix, with its regression tests; `lib/**/*.sh` stays clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and the terminal state.
- Disk-touching unit tests are `#[cfg(unix)]`.
- Every expected value in a unit test was captured from Bash on 2026-09-14 by `scratchpad/phase4/yaml_edit_reference.sh` and `scratchpad/phase4/remove_reference.sh` (the latter against Task 1's fix).
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-14 under `/decide`: all four as recommended.

1. **Slicing the `yaml_edit` family.** The spec names nine commands for one plan. **Recommended:** three plans, as Phase 3b was split: 4a `enable`/`disable` with `yaml_edit` and `edit_paths`; 4b `customize`, `show`, `diff`, `simplify`, `resolve` with `snapshot`; 4c `profile` and `upgrade-config`. Recorded in the spec's Phase 4 section by Task 5. Alternative: one plan of several thousand lines that no review could hold.
2. **The Bash bugs.** `yaml_list_remove` keeps removing matching dash items past the end of `tools.enabled` (a hand-written block list under another key loses the tool) and ignores `enabled: [a, b]` (the command reports "Disabled 1 tool(s)" and changes nothing). **Recommended:** fix Bash first, as the `native-port` skill requires: stop at the first non-comment line indented less than the items, and rewrite an inline list without the item. Alternative: port the bugs as quirks, which ships known data loss into the binary.
3. **Raw arguments for `enable` and `disable`.** **Recommended:** `main` matches the first argument before clap and passes the rest untouched. Alternative: clap variants with `trailing_var_arg`, which drop a leading `--`.
4. **Quirks kept, not fixed.** `enable` writes `.ai/agent_sync.yaml` (or a root `agent_sync.yaml`) even when `AGENTSYNC_CONFIG_PATH` selects another file, while "already enabled" reads the selected file; `disable` creates the config when none exists; a missing `tools.enabled` under an existing `tools:` appends a second `tools:` block; `disable` lists every argument no longer enabled, unknown slugs included. **Recommended:** reproduce them, record them as known quirks 14–17 in the spec, and leave any behaviour change to a Bash-first change later. Alternative: fix them now, which widens a port into a feature change.

## Module closure

```text
lib/helpers/enable.sh           1-303   config resolve/create, context, scaffold, cmd_enable, cmd_disable
lib/helpers/yaml_edit.sh        40-100  _yaml_find_key_line
                                106-135 yaml_set_scalar
                                230-329 yaml_list_append
                                331-366 yaml_list_remove (rewritten by Task 1)
lib/helpers/edit_paths.sh       16-92   tool_edit_paths_rows, print_tool_edit_paths_block
lib/helpers/tool_resolver.sh    36-72   init_user_dir, select_project_config, user_dir_in_project, require_project_user_dir
                                263-330 _find_base_payload, _payload_override_path, _payload_override_legacy_path
                                369     shared_mcp_path
                                546-610 list_enabled_tools, is_tool_enabled, tool_exists, tool_display_name
lib/helpers/yaml.sh             32, 138 _yaml_normalize_scalar, parse_yaml_list
lib/helpers/prompts.sh          10-40   is_tty, prompt_confirm
lib/helpers/tmp.sh              100-125 tmp_sibling
lib/helpers/cli_colors.sh       1-14    colours
```

Already ported and reused: `project::Project` (config selection, `source.tools`, enabled and override sets), `tool::Tool`, `catalog`, `payload::legacy_override_path`, `paths::Paths`, `staging::write_beside`, `prompts`, `style`, `yaml_subset`.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
for f in enable customize config_safety list shared profiles sync_options native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: the Phase 3b close commit; `185 passed` and `11 passed`; `0` for every file.

---

### Task 1: Bash `yaml_list_remove` Stays in Its List and Handles `[a, b]`

**Files:**
- Modify: `lib/helpers/yaml_edit.sh:331-366`
- Test: `tests/enable.bats`

- [x] **Step 1: Write the failing tests**

Append to `tests/enable.bats`:

```bash
@test "disable leaves other lists that name the tool alone" {
    run_agentsync enable claude cursor >/dev/null
    printf 'profiles:\n  hub:\n    tools:\n      - claude\n' >> .ai/agent_sync.yaml
    run run_agentsync disable claude
    [ "$status" -eq 0 ]
    ! grep -q "^    - claude$" .ai/agent_sync.yaml
    grep -q "^      - claude$" .ai/agent_sync.yaml
}

@test "disable removes a tool from an inline tools.enabled list" {
    printf 'tools:\n  enabled: [claude, cursor]\n' > .ai/agent_sync.yaml
    run run_agentsync disable claude
    [ "$status" -eq 0 ]
    grep -qx '  enabled: \[cursor\]' .ai/agent_sync.yaml
    [[ "$output" == *"Claude Code (claude)"* ]]
}
```

- [x] **Step 2: Run them, confirm they fail**

Run: `AGENTSYNC_NATIVE=0 bats --tap tests/enable.bats | grep '^not ok'`
Expected: `not ok 14 disable leaves other lists that name the tool alone` and `not ok 15 disable removes a tool from an inline tools.enabled list`.

- [x] **Step 3: Write the fix**

Replace `yaml_list_remove` in `lib/helpers/yaml_edit.sh` with:

```bash
# Print the items of an inline list body except <value>, joined by ", ".
_yaml_inline_list_without() {
    local items="$1" value="$2" item kept=""
    local reglob=1
    case $- in *f*) reglob=0 ;; esac
    set -f
    local IFS=','
    for item in $items; do
        _yaml_normalize_scalar_reply "$item"
        [[ -n "$REPLY" && "$REPLY" != "$value" ]] || continue
        item="${item#"${item%%[![:space:]]*}"}"
        item="${item%"${item##*[![:space:]]}"}"
        kept+="${kept:+, }$item"
    done
    (( reglob )) && set +f
    printf '%s' "$kept"
}

# Remove an item from the list under <path>, block style or single-line `[a, b]`.
# The block ends at the first non-comment line indented less than its items.
# Usage: yaml_list_remove <file> <dot.path> <value>
yaml_list_remove() {
    local file="$1"
    local path="$2"
    local value="$3"
    [[ -f "$file" ]] || return 0

    local key_line_info
    key_line_info=$(_yaml_find_key_line "$file" "$path")
    [[ -z "$key_line_info" ]] && return 0
    local key_lineno key_indent
    key_lineno=$(echo "$key_line_info" | awk '{print $1}')
    key_indent=$(echo "$key_line_info" | awk '{print $2}')
    local item_indent=$(( key_indent + 2 ))
    local inline_re="^([[:space:]]*${path##*.}:[[:space:]]*)\\[(.*)\\][[:space:]]*$"

    local lineno=0 in_list=true
    {
        while IFS= read -r line || [[ -n "$line" ]]; do
            ((lineno++)) || true
            if [[ $lineno -lt $key_lineno ]] || [[ "$in_list" != "true" ]]; then
                printf '%s\n' "$line"
                continue
            fi
            if [[ $lineno -eq $key_lineno ]]; then
                if [[ "$line" =~ $inline_re ]]; then
                    printf '%s[%s]\n' "${BASH_REMATCH[1]}" "$(_yaml_inline_list_without "${BASH_REMATCH[2]}" "$value")"
                    in_list=false
                else
                    printf '%s\n' "$line"
                fi
                continue
            fi
            local stripped="${line#"${line%%[![:space:]]*}"}"
            local indent=$(( ${#line} - ${#stripped} ))
            if [[ -n "$stripped" && "$stripped" != \#* && $indent -lt $item_indent ]]; then
                in_list=false
                printf '%s\n' "$line"
                continue
            fi
            if [[ "$stripped" =~ ^-[[:space:]]*(.*)$ ]]; then
                local normalized
                normalized=$(_yaml_normalize_scalar "${BASH_REMATCH[1]}")
                [[ "$normalized" == "$value" ]] && continue
            fi
            printf '%s\n' "$line"
        done < "$file"
    } | _yaml_atomic_write "$file"
}
```

- [x] **Step 4: Run the tests, confirm green**

```bash
AGENTSYNC_NATIVE=0 bats --tap tests/enable.bats | grep -c '^not ok'
shellcheck -x -S warning -e SC1091 lib/helpers/yaml_edit.sh; echo "shellcheck $?"
bash scratchpad/phase4/remove_reference.sh | grep -c '^=='
```

Expected: `0`; `shellcheck 0`; `6` (the reference cases run against the committed function once the script sources `lib/helpers/yaml_edit.sh` alone: delete its `source "$S/yaml_list_remove_fixed.sh"` line first, and compare the six outputs with the values in Task 2's `list_remove` test).

- [x] **Step 5: Commit**

```bash
git add lib/helpers/yaml_edit.sh tests/enable.bats docs/plans/2026-09-14-rust-migration-phase-4a-enable-disable.md
git commit -m "fix(enable): keep disable inside tools.enabled and read [a, b]"
```

---

### Task 2: `src/yaml_edit.rs`

**Files:**
- Create: `src/yaml_edit.rs`
- Modify: `src/lib.rs` (`pub mod yaml_edit;` after `pub mod workspace;`)

**Interfaces:**
- Produces:
  - `pub fn find_key_line(text: &str, key_path: &str) -> Option<(usize, usize)>` — 1-based line and indent
  - `pub fn set_scalar_text(text: Option<&str>, key: &str, value: &str) -> String`
  - `pub fn list_append_text(text: Option<&str>, key_path: &str, value: &str) -> Option<String>` — `None` when the value is already listed
  - `pub fn list_remove_text(text: &str, key_path: &str, value: &str) -> Option<String>` — `None` when the key is absent
  - `pub fn set_scalar(file: &Path, key: &str, value: &str) -> Result<(), Error>`
  - `pub fn list_append(file: &Path, key_path: &str, value: &str) -> Result<(), Error>`
  - `pub fn list_remove(file: &Path, key_path: &str, value: &str) -> Result<(), Error>`

- [x] **Step 1: Write the failing tests**

Create `src/yaml_edit.rs` with the tests module only, and register the module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_follows_the_last_item_or_builds_the_path_like_yaml_list_append() {
        let cases: [(&str, Option<&str>); 7] = [
            (
                "# AgentSync — Project Configuration\ntools:\n  enabled: []\n",
                Some("# AgentSync — Project Configuration\ntools:\n  enabled:\n    - claude\n"),
            ),
            (
                "# c\ntools:\n  enabled:\n    - zed\n    # note\n    - cursor\n\nsource:\n  rules: x\n",
                Some("# c\ntools:\n  enabled:\n    - zed\n    # note\n    - cursor\n    - claude\n\nsource:\n  rules: x\n"),
            ),
            ("tools:\n  enabled: [claude, cursor]", None),
            (
                "format: 2\ntools:\n  other: x\n\n\n",
                Some("format: 2\ntools:\n  other: x\n\ntools:\n  enabled:\n    - claude\n"),
            ),
            ("", Some("tools:\n  enabled:\n    - claude\n")),
            (
                "tools:\n  enabled:\n    - zed",
                Some("tools:\n  enabled:\n    - zed\n    - claude\n"),
            ),
            (
                "tools:\n  foo:\n    enabled: x\n",
                Some("tools:\n  foo:\n    enabled:\n      - claude\n"),
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(
                list_append_text(Some(text), "tools.enabled", "claude").as_deref(),
                expected,
                "{text:?}"
            );
        }
        assert_eq!(
            list_append_text(None, "tools.enabled", "claude").as_deref(),
            Some("tools:\n  enabled:\n    - claude\n")
        );
    }

    #[test]
    fn remove_stays_in_its_list_and_rewrites_an_inline_one() {
        let cases: [(&str, &str, &str); 6] = [
            (
                "tools:\n  enabled:\n    - claude\n    - \"cursor\" # q\nprofiles:\n  hub:\n    tools:\n      - claude\n",
                "claude",
                "tools:\n  enabled:\n    - \"cursor\" # q\nprofiles:\n  hub:\n    tools:\n      - claude\n",
            ),
            ("tools:\n  enabled: [claude]\n", "claude", "tools:\n  enabled: []\n"),
            (
                "tools:\n  enabled: [claude, \"cursor\" , zed]",
                "cursor",
                "tools:\n  enabled: [claude, zed]\n",
            ),
            (
                "tools:\n  enabled:\n    - \"cursor\" # q\n  \n",
                "cursor",
                "tools:\n  enabled:\n  \n",
            ),
            (
                "tools:\n  enabled:\n# c\n    - claude\n  other: [claude]\n",
                "claude",
                "tools:\n  enabled:\n# c\n  other: [claude]\n",
            ),
            ("tools:\n  enabled: [*.md, claude]\n", "claude", "tools:\n  enabled: [*.md]\n"),
        ];
        for (text, value, expected) in cases {
            assert_eq!(
                list_remove_text(text, "tools.enabled", value).as_deref(),
                Some(expected),
                "{text:?}"
            );
        }
        assert_eq!(list_remove_text("format: 2\n", "tools.enabled", "claude"), None);
    }

    #[test]
    fn set_scalar_replaces_the_first_root_key_or_appends_it() {
        assert_eq!(
            set_scalar_text(Some("name: X\nenabled: true\nenabled: true\n"), "enabled", "false"),
            "name: X\nenabled: false\nenabled: true\n"
        );
        assert_eq!(
            set_scalar_text(Some("name: X"), "enabled", "false"),
            "name: X\nenabled: false\n"
        );
        assert_eq!(
            set_scalar_text(Some("enabled:\n  x: 1\n"), "enabled", "false"),
            "enabled: false\n  x: 1\n"
        );
        assert_eq!(set_scalar_text(None, "enabled", "false"), "enabled: false\n");
    }

    #[test]
    fn a_key_is_found_only_inside_its_parent_block() {
        let text = "# c\ntools:\n  enabled:\n    - a\nsource:\n  enabled: x\n";
        assert_eq!(find_key_line(text, "tools.enabled"), Some((3, 2)));
        assert_eq!(find_key_line(text, "source.enabled"), Some((6, 2)));
        assert_eq!(find_key_line("tools:\nenabled: x\n", "tools.enabled"), None);
        assert_eq!(find_key_line("  tools:\n", "tools"), None);
    }

    #[cfg(unix)]
    #[test]
    fn file_edits_write_beside_and_skip_a_listed_value() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".ai/agent_sync.yaml");
        list_append(&file, "tools.enabled", "claude").unwrap();
        list_append(&file, "tools.enabled", "claude").unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "tools:\n  enabled:\n    - claude\n"
        );
        list_remove(&file, "tools.enabled", "claude").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "tools:\n  enabled:\n");
        list_remove(&dir.path().join("missing.yaml"), "tools.enabled", "claude").unwrap();
        assert!(!dir.path().join("missing.yaml").exists());
    }
}
```

- [x] **Step 2: Run the tests, confirm they fail**

Run: `cargo test --lib yaml_edit 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function` errors for the seven functions.

- [x] **Step 3: Write the implementation**

Above the tests module:

```rust
//! Line-oriented edits of `lib/helpers/yaml_edit.sh`: comments stay, every
//! rewritten line ends in a newline as `while read` prints it, and files are
//! replaced through a staging file beside them.

use std::path::Path;

use crate::{Error, staging, yaml_subset};

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// `while IFS= read -r line || [[ -n "$line" ]]`.
fn lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

fn split_indent(line: &str) -> (usize, &str) {
    let stripped = line.trim_start_matches(is_space);
    (line.len() - stripped.len(), stripped)
}

fn is_comment(line: &str) -> bool {
    line.trim_start_matches(is_space).starts_with('#')
}

/// `^([a-zA-Z0-9_-]+):`.
fn key_of(stripped: &str) -> Option<&str> {
    let end = stripped
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(stripped.len());
    (end > 0 && stripped[end..].starts_with(':')).then(|| &stripped[..end])
}

/// `_yaml_find_key_line`.
pub fn find_key_line(text: &str, key_path: &str) -> Option<(usize, usize)> {
    let keys: Vec<&str> = key_path.split('.').collect();
    let mut looking = 0;
    let mut in_section = false;
    let mut section_indent = 0;
    for (index, line) in lines(text).into_iter().enumerate() {
        if line.is_empty() || is_comment(line) {
            continue;
        }
        let (indent, stripped) = split_indent(line);
        let Some(key) = key_of(stripped) else {
            continue;
        };
        if in_section && indent <= section_indent {
            return None;
        }
        if (in_section || indent == 0) && key == keys[looking] {
            if looking + 1 == keys.len() {
                return Some((index + 1, indent));
            }
            in_section = true;
            section_indent = indent;
            looking += 1;
        }
    }
    None
}

/// `yaml_set_scalar` on text; `None` is a missing file.
pub fn set_scalar_text(text: Option<&str>, key: &str, value: &str) -> String {
    let replacement = format!("{key}: {value}\n");
    let Some(text) = text else {
        return replacement;
    };
    let mut out = String::new();
    let mut found = false;
    for line in lines(text) {
        let matches = line == format!("{key}:")
            || line
                .strip_prefix(&format!("{key}:"))
                .is_some_and(|rest| rest.starts_with(is_space));
        if !found && matches {
            out.push_str(&replacement);
            found = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !found {
        out.push_str(&replacement);
    }
    out
}

/// `yaml_list_append` on text.
pub fn list_append_text(text: Option<&str>, key_path: &str, value: &str) -> Option<String> {
    if yaml_subset::list(text.unwrap_or(""), key_path)
        .iter()
        .any(|item| item == value)
    {
        return None;
    }
    let found = text.and_then(|text| find_key_line(text, key_path));
    let (Some(text), Some((key_lineno, key_indent))) = (text, found) else {
        let mut out = String::new();
        let existing = text.unwrap_or("").trim_end_matches('\n');
        if !existing.is_empty() {
            out.push_str(existing);
            out.push_str("\n\n");
        }
        let segments: Vec<&str> = key_path.split('.').collect();
        for (depth, segment) in segments.iter().enumerate() {
            out.push_str(&format!("{}{segment}:\n", " ".repeat(2 * depth)));
        }
        out.push_str(&format!("{}- {value}\n", " ".repeat(2 * segments.len())));
        return Some(out);
    };

    let all = lines(text);
    let item_indent = key_indent + 2;
    let leaf = key_path.rsplit('.').next().unwrap_or(key_path);
    let has_inline = all[key_lineno - 1]
        .trim_start_matches(is_space)
        .strip_prefix(&format!("{leaf}:"))
        .is_some_and(|rest| rest.starts_with(is_space) && rest.chars().count() >= 2);
    let mut last_item = key_lineno;
    for (index, line) in all.iter().enumerate().skip(key_lineno) {
        if line.is_empty() || is_comment(line) {
            continue;
        }
        let (indent, stripped) = split_indent(line);
        if indent < item_indent {
            break;
        }
        if stripped.starts_with('-') {
            last_item = index + 1;
        }
    }
    let mut out = String::new();
    for (index, line) in all.iter().enumerate() {
        let lineno = index + 1;
        if lineno == key_lineno && has_inline {
            out.push_str(&format!("{}{leaf}:\n", " ".repeat(key_indent)));
        } else {
            out.push_str(line);
            out.push('\n');
        }
        if lineno == last_item {
            out.push_str(&format!("{}- {value}\n", " ".repeat(item_indent)));
        }
    }
    Some(out)
}

/// `^([[:space:]]*<leaf>:[[:space:]]*)\[(.*)\][[:space:]]*$`: the prefix up to
/// the bracket and the list body.
fn inline_list<'a>(line: &'a str, leaf: &str) -> Option<(&'a str, &'a str)> {
    let (_, stripped) = split_indent(line);
    let rest = stripped
        .strip_prefix(leaf)?
        .strip_prefix(':')?
        .trim_start_matches(is_space);
    let body = rest
        .strip_prefix('[')?
        .trim_end_matches(is_space)
        .strip_suffix(']')?;
    Some((&line[..line.len() - rest.len()], body))
}

/// `_yaml_inline_list_without`.
fn inline_without(body: &str, value: &str) -> String {
    body.split(',')
        .filter(|item| {
            let normalized = yaml_subset::normalize_scalar(item);
            !normalized.is_empty() && normalized != value
        })
        .map(|item| item.trim_matches(is_space))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `yaml_list_remove` on text.
pub fn list_remove_text(text: &str, key_path: &str, value: &str) -> Option<String> {
    let (key_lineno, key_indent) = find_key_line(text, key_path)?;
    let item_indent = key_indent + 2;
    let leaf = key_path.rsplit('.').next().unwrap_or(key_path);
    let mut out = String::new();
    let mut in_list = true;
    for (index, line) in lines(text).into_iter().enumerate() {
        let lineno = index + 1;
        if lineno < key_lineno || !in_list {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if lineno == key_lineno {
            match inline_list(line, leaf) {
                Some((prefix, body)) => {
                    out.push_str(&format!("{prefix}[{}]\n", inline_without(body, value)));
                    in_list = false;
                }
                None => {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            continue;
        }
        let (indent, stripped) = split_indent(line);
        if !stripped.is_empty() && !stripped.starts_with('#') && indent < item_indent {
            in_list = false;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if let Some(item) = stripped.strip_prefix('-')
            && yaml_subset::normalize_scalar(item.trim_start_matches(is_space)) == value
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

fn read_existing(file: &Path) -> Result<Option<String>, Error> {
    if !file.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(file).map_err(|e| Error::io(file, e))?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn ensure_parent(file: &Path) -> Result<(), Error> {
    match file.parent() {
        Some(dir) if !dir.as_os_str().is_empty() && !dir.is_dir() => {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))
        }
        _ => Ok(()),
    }
}

/// `yaml_set_scalar`.
pub fn set_scalar(file: &Path, key: &str, value: &str) -> Result<(), Error> {
    ensure_parent(file)?;
    let text = read_existing(file)?;
    staging::write_beside(file, set_scalar_text(text.as_deref(), key, value).as_bytes())
}

/// `yaml_list_append`.
pub fn list_append(file: &Path, key_path: &str, value: &str) -> Result<(), Error> {
    ensure_parent(file)?;
    let text = read_existing(file)?;
    match list_append_text(text.as_deref(), key_path, value) {
        Some(updated) => staging::write_beside(file, updated.as_bytes()),
        None => Ok(()),
    }
}

/// `yaml_list_remove`.
pub fn list_remove(file: &Path, key_path: &str, value: &str) -> Result<(), Error> {
    let Some(text) = read_existing(file)? else {
        return Ok(());
    };
    match list_remove_text(&text, key_path, value) {
        Some(updated) => staging::write_beside(file, updated.as_bytes()),
        None => Ok(()),
    }
}
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test 2>&1 | grep 'test result' | head -3`
Expected: `190 passed` and `11 passed`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/yaml_edit.rs src/lib.rs docs/plans/2026-09-14-rust-migration-phase-4a-enable-disable.md
git commit -m "feat(native): port the list and scalar editors of yaml_edit.sh"
```

---

### Task 3: `payload::override_path` and `src/edit_paths.rs`

**Files:**
- Modify: `src/payload.rs` (new `override_path`)
- Create: `src/edit_paths.rs`
- Modify: `src/lib.rs` (`pub mod edit_paths;` after `pub mod convert;`)

**Interfaces:**
- Consumes: `project::Project`, `tool::Tool`, `style::Style`.
- Produces:
  - `payload::override_path(project: &Project, tool: &Tool, resource: &str) -> Option<PathBuf>` — `<tools dir>/<slug>/<resource>.<ext of the shipped payload>`
  - `edit_paths::block(project: &Project, tool: &Tool, style: &Style) -> String` — `print_tool_edit_paths_block`

- [x] **Step 1: Write the failing test**

Create `src/edit_paths.rs` with the tests module only:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_block_names_overrides_the_shared_mcp_and_hints_like_bash() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        let claude = Tool::load(&project, "claude").unwrap();
        assert_eq!(
            block(&project, &claude, &Style::plain()),
            "\n  Claude Code\n    Settings:      agentsync customize claude settings\n    MCP:           agentsync add mcp <server>  (shared — not yet configured)\n"
        );

        write(dir.path(), ".ai/src/tools/claude/settings.json", "{}");
        write(dir.path(), ".ai/src/mcp.json", "{}");
        assert_eq!(
            block(&project, &claude, &Style::plain()),
            "\n  Claude Code\n    Edit settings: .ai/src/tools/claude/settings.json\n    Edit mcp:      .ai/src/mcp.json  (shared)\n"
        );

        let windsurf = Tool::load(&project, "windsurf").unwrap();
        write(dir.path(), ".ai/src/tools/windsurf/mcp.json", "{}");
        assert_eq!(
            block(&project, &windsurf, &Style::plain()),
            "\n  Windsurf\n    Hooks:         agentsync customize windsurf hooks\n    Edit mcp:      .ai/src/tools/windsurf/mcp.json\n"
        );
        let amp = Tool::load(&project, "no-such-tool").unwrap();
        assert_eq!(block(&project, &amp, &Style::plain()), "");
    }
}
```

- [x] **Step 2: Run it, confirm it fails**

Run: `cargo test --lib edit_paths 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function 'block'` and unresolved `Project`, `Tool`, `Style`, `Path`.

- [x] **Step 3: Write the implementation**

In `src/payload.rs`, after `legacy_override_path`:

```rust
/// `_payload_override_path`: the canonical write path under the tool override
/// directory, with the shipped payload's extension.
pub fn override_path(project: &Project, tool: &Tool, resource: &str) -> Option<PathBuf> {
    let ext = tool.base_payload(resource)?.path().extension()?.to_str()?.to_string();
    Some(
        project
            .user_tools_dir()
            .join(&tool.slug)
            .join(format!("{resource}.{ext}")),
    )
}
```

Above the tests module in `src/edit_paths.rs`:

```rust
//! `lib/helpers/edit_paths.sh`: where a tool's payload overrides are edited.
//! `enable` prints the block; `doctor` gets its checklist when it is ported.

use std::path::Path;

use crate::payload;
use crate::project::Project;
use crate::style::Style;
use crate::tool::Tool;

enum Row {
    Override(&'static str, String),
    Shared(String),
    CustomizeHint(&'static str, String),
    SharedHint,
}

fn shown(project: &Project, path: &Path) -> String {
    let text = path.to_string_lossy();
    let root = format!("{}/", project.root.to_string_lossy());
    text.strip_prefix(&root).unwrap_or(&text).to_string()
}

/// `tool_edit_paths_rows`.
fn rows(project: &Project, tool: &Tool) -> Vec<Row> {
    let mut rows = Vec::new();
    for resource in ["settings", "hooks"] {
        let Some(path) = payload::override_path(project, tool, resource) else {
            continue;
        };
        if path.is_file() {
            rows.push(Row::Override(resource, shown(project, &path)));
        } else {
            rows.push(Row::CustomizeHint(
                resource,
                format!("agentsync customize {} {resource}", tool.slug),
            ));
        }
    }
    let Some(per_tool) = payload::override_path(project, tool, "mcp") else {
        return rows;
    };
    if per_tool.is_file() {
        rows.push(Row::Override("mcp", shown(project, &per_tool)));
    } else if project.shared_mcp_path().is_file() {
        rows.push(Row::Shared(shown(project, &project.shared_mcp_path())));
    } else {
        rows.push(Row::SharedHint);
    }
    rows
}

/// `print_tool_edit_paths_block`.
pub fn block(project: &Project, tool: &Tool, style: &Style) -> String {
    let rows = rows(project, tool);
    if rows.is_empty() {
        return String::new();
    }
    let mut text = format!("\n{}\n", style.bold(&format!("  {}", tool.display_name())));
    for row in rows {
        text.push_str(&match row {
            Row::Override(resource, path) => {
                format!("    Edit {:<9} {path}\n", format!("{resource}:"))
            }
            Row::Shared(path) => {
                format!("    Edit {:<9} {path}  {}\n", "mcp:", style.dim("(shared)"))
            }
            Row::CustomizeHint(resource, command) => {
                let label = match resource {
                    "settings" => "Settings:",
                    _ => "Hooks:",
                };
                format!("    {label:<14} {}\n", style.dim(&command))
            }
            Row::SharedHint => format!(
                "    {:<14} {}\n",
                "MCP:",
                style.dim("agentsync add mcp <server>  (shared — not yet configured)")
            ),
        });
    }
    text
}
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test 2>&1 | grep 'test result' | head -3`
Expected: `191 passed` and `11 passed`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/payload.rs src/edit_paths.rs src/lib.rs docs/plans/2026-09-14-rust-migration-phase-4a-enable-disable.md
git commit -m "feat(native): port the edit-path block of edit_paths.sh"
```

---

### Task 4: Port `enable` and `disable`

**Files:**
- Create: `src/cli/enable.rs`
- Modify: `src/cli/mod.rs` (`pub mod enable;`), `src/main.rs` (raw dispatch), `bin/agentsync.sh:280` (`_NATIVE_COMMANDS`)
- Test: `tests/native_parity.bats`

**Interfaces:**
- Consumes: Tasks 2 and 3.
- Produces:
  - `pub fn enable(args: &[String], discover: &dyn Fn() -> Result<Project, Error>, style: &Style, interactive: bool, confirm: &mut dyn FnMut(&str) -> bool, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`
  - `pub fn disable(args: &[String], discover: &dyn Fn() -> Result<Project, Error>, style: &Style, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`

- [x] **Step 1: Write the parity fixtures and read the Bash side**

Append to `tests/native_parity.bats`:

```bash
# ── enable / disable ─────────────────────────────────────────────────────────

@test "parity: enable scaffolds, reports, and refuses like Bash" {
    assert_tree_parity enable claude windsurf
    assert_tree_parity enable
    assert_tree_parity enable --bogus claude
    assert_tree_parity enable --help
    assert_tree_parity enable claude bogus_tool --no-scaffold
    assert_tree_parity enable -- claude
    mkdir -p .ai/src/settings
    printf '{"legacy": true}\n' > .ai/src/settings/claude.json
    printf '{"mcpServers":{}}\n' > .ai/src/mcp.json
    assert_tree_parity enable claude cursor --scaffold
    _run_engine 0 enable claude >/dev/null
    assert_tree_parity enable claude cursor
}

@test "parity: disable edits block and inline lists and legacy flags like Bash" {
    _run_engine 0 enable claude cursor >/dev/null
    mkdir -p .ai/src/tools
    printf 'enabled: true\n' > .ai/src/tools/kimi.yaml
    assert_tree_parity disable claude kimi bogus_tool
    assert_tree_parity disable
    assert_tree_parity disable zed
    printf 'tools:\n  enabled: [claude, cursor]\n' > .ai/agent_sync.yaml
    assert_tree_parity disable cursor
    printf 'format: 2\ntools:\n  other: x\n' > .ai/agent_sync.yaml
    assert_tree_parity enable claude
    rm .ai/agent_sync.yaml
    assert_tree_parity disable claude
    assert_tree_parity enable claude claude
}

@test "parity: enable and disable with an explicit config and outside source.tools" {
    local outside="$BATS_TEST_TMPDIR/outside"
    mkdir -p "$outside" config
    printf 'enabled: true\n' > "$outside/kimi.yaml"
    printf 'tools:\n  enabled: [claude]\nsource:\n  tools: "%s"\n' "$outside" > config/a.yaml
    AGENTSYNC_CONFIG_PATH=config/a.yaml assert_tree_parity enable cursor --scaffold
    AGENTSYNC_CONFIG_PATH=config/a.yaml assert_tree_parity enable cursor claude
    AGENTSYNC_CONFIG_PATH=config/a.yaml assert_tree_parity disable claude
    AGENTSYNC_CONFIG_PATH=config/a.yaml assert_tree_parity disable kimi
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_tree_parity enable claude
}
```

Run: `AGENTSYNC_NATIVE=0 bats --tap tests/enable.bats | grep -c '^not ok'` and `bats --tap -f 'enable|disable' tests/native_parity.bats`
Expected: `0`; three `ok` lines (the binary does not answer `enable` yet, so both sides run Bash).

- [x] **Step 2: Write the failing unit tests**

Create `src/cli/enable.rs` with the tests module only and add `pub mod enable;` to `src/cli/mod.rs`:

```rust
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
            enable(&args, &discover, &Style::plain(), false, &mut |_| true, &mut out, &mut err)
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
```

Before running, confirm the display names in the expected strings with `grep -m1 '^name:' lib/templates/tools/{cursor,kimi}.yaml` and adjust the literals to the names printed there.

Run: `cargo test --lib cli::enable 2>&1 | grep -E '^error' | sort | uniq -c`
Expected: `cannot find function 'enable'` and `'disable'`.

- [x] **Step 3: Write the implementation**

Above the tests module in `src/cli/enable.rs`:

```rust
//! `agentsync enable` and `agentsync disable`: `cmd_enable` and `cmd_disable`
//! of `lib/helpers/enable.sh`, editing `tools.enabled` with `yaml_edit`.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::paths::{self, Paths};
use crate::project::Project;
use crate::style::Style;
use crate::tool::Tool;
use crate::{Error, catalog, edit_paths, payload, yaml_edit};

const ENABLE_USAGE: &str = "Usage: agentsync enable <slug> [<slug>...] [--no-scaffold|--scaffold] [--yes]

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

/// `tool_resolver_user_dir_in_project`.
fn tools_dir_in_project(project: &Project) -> bool {
    let paths = Paths::on_disk(&project.root.to_string_lossy());
    let abs = paths::normalize(&project.user_tools_dir().to_string_lossy());
    paths
        .canonicalize_with_existing_ancestor(&abs)
        .is_some_and(|canonical| paths::is_within(&canonical, &paths.root_canonical))
}

/// `tool_resolver_require_project_user_dir`, printed; the caller exits 1.
fn outside_tools_dir(project: &Project, style: &Style, err: &mut dyn Write) -> Result<u8, Error> {
    put(
        err,
        &format!(
            "{}: source.tools resolves outside the project: {}\nAgentSync only reads that catalog; edit its tool overrides where they live.\n",
            style.red("Error"),
            project.user_tools_dir().to_string_lossy()
        ),
    )?;
    Ok(1)
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
    if !tools_dir_in_project(&project) {
        if scaffold == Scaffold::Always {
            return outside_tools_dir(&project, style, err);
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
        put(out, &format!("{}\n", style.green(&format!("Enabled {} tool(s)", added.len()))))?;
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
            &format!("\n{}\n", style.dim(&format!("{already} tool(s) were already enabled"))),
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
                let dir = paths::parent(&path.to_string_lossy());
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
            &format!("{}: agentsync disable <slug> [<slug>...]\n", style.red("Error")),
        )?;
        return Ok(1);
    }
    let project = discover()?;
    if !tools_dir_in_project(&project) {
        for slug in args {
            if Tool::load(&project, slug)?.user_value("enabled") == "true" {
                return outside_tools_dir(&project, style, err);
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
        put(out, &format!("{}\n", style.yellow(&format!("Disabled {removed} tool(s)"))))?;
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
        put(out, &format!("{}\n", style.dim("No matching tools were enabled.")))?;
    }
    put(out, "\n")?;
    Ok(0)
}
```

`user_value("enabled")` reads the override file through `yaml_subset::value`, as `parse_yaml_value "$user_file" "enabled"` does; `Tool::load` reads `project.user_tool_file`, the `source.tools` directory.

In `src/main.rs`, at the top of `run`, after the `--version` check and before `Cli::parse_from`:

```rust
    // `cmd_enable` reads a leading `--` as the start of tool slugs; clap would consume it.
    match args.first().and_then(|a| a.to_str()) {
        Some(command @ ("enable" | "disable")) => {
            let rest: Vec<String> = args[1..]
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let style = Style::for_stdout();
            let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
            return if command == "enable" {
                cli::enable::enable(
                    &rest,
                    &Project::discover,
                    &style,
                    prompts::is_tty(),
                    &mut |question: &str| prompts::confirm(question, true),
                    &mut out,
                    &mut err,
                )
            } else {
                cli::enable::disable(&rest, &Project::discover, &style, &mut out, &mut err)
            };
        }
        _ => {}
    }
```

In `bin/agentsync.sh:280`: `_NATIVE_COMMANDS=" version --version -v list ls check sync rollback enable disable "`.

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/enable.bats | grep -c '^not ok'
bats --tap -f 'enable|disable' tests/native_parity.bats
```

Expected: `193 passed` and `11 passed`; `0`; three `ok` lines.

- [x] **Step 5: Prove the fixtures bite**

Change `"Run {} to apply cleanup."` to `"Run {} to clean up."` in `src/cli/enable.rs`, rebuild, rerun `bats --tap -f 'disable edits' tests/native_parity.bats`: `not ok` with that diff; revert and rebuild.

- [x] **Step 6: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
git add src/cli/enable.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-14-rust-migration-phase-4a-enable-disable.md
git commit -m "feat(native): port enable and disable"
```

---

### Task 5: Every File That Runs `enable`, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md` (Phase 4 slices; known quirks 14–17), `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [ ] **Step 1: Spec**

In the Phase 4 section, after the `yaml_edit` family bullet:

```markdown
  Planned in three slices: 4a `enable` and `disable` with `yaml_edit` and
  `edit_paths`; 4b `customize`, `show`, `diff`, `simplify`, `resolve` with
  `snapshot`; 4c `profile` and `upgrade-config`.
```

Append to "Known quirks", numbered after the last entry:

```markdown
14. `enable` and `disable` edit `.ai/agent_sync.yaml`, or a root
    `agent_sync.yaml`, even when `AGENTSYNC_CONFIG_PATH` selects another file,
    while "already enabled" reads the selected one.
15. `disable` creates `.ai/agent_sync.yaml` when the project has none.
16. `enable` under a `tools:` block without `enabled:` appends a second `tools:`
    block at the end of the file.
17. `disable` lists every argument that is not enabled afterwards, unknown slugs
    and repeated ones included.
```

If the list's last number is not 13, renumber these four to follow it and use those numbers in the plan's decision 4.

- [ ] **Step 2: Module map and outputs**

In `.ai/src/skills/native-port/references/module-map.md`: set the `lib/helpers/yaml_edit.sh` row's note to `set_scalar, list_append, list_remove, find_key_line (Phase 4a); remove_key, rename_key wait for 4b`, the `lib/helpers/edit_paths.sh` row's note to `block for enable (Phase 4a); checklist waits for doctor`, and the `lib/helpers/enable.sh` row to `→ src/cli/enable.rs       Phase 4a, ported`. Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [ ] **Step 3: Verify**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in enable customize config_safety init doctor list shared profiles sync_options native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `193 passed` and `11 passed`; lint exit 0; every line `bash=0 native=0`.

- [ ] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-14-rust-migration-phase-4a-enable-disable.md
git commit -m "docs(native): map the phase 4a modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, `enable.bats` is green under `AGENTSYNC_NATIVE=1`, the parity fixtures pass, the files in Task 5 pass in both modes, and a `## Completion receipt` records the fresh verification. Task 1's Bash fix also belongs on `main` for the next patch release; carrying it there is a separate, confirmed step. The next plan is Phase 4b.

## Run log

### 2026-09-14 — Phase 4a planned
- Commits: this plan.
- Verified: `scratchpad/phase4/yaml_edit_reference.sh` (the append and scalar cases) and `scratchpad/phase4/remove_reference.sh` (the six remove cases against the candidate fix) ran against Bash; `scratchpad/phase4/repro_disable_profile.sh` reproduced `disable claude` on `enabled: [claude, cursor]` printing "Disabled 1 tool(s)" and leaving the file unchanged.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.

### 2026-09-14 — Tasks 0 and 1 done
- Commits: `de7137c` "test(native): raise the trap test's signal in its own process" (outside the plan, below), "fix(enable): keep disable inside tools.enabled and read [a, b]".
- Verified: Task 0 at `64910f7`: `enable`, `customize`, `config_safety`, `list`, `shared`, `profiles`, `sync_options`, `native_parity` all `bash=0`. Task 1: the two new cases `not ok` before the fix, `enable.bats` 0 failures after; ShellCheck exit 0; `remove_reference.sh` against the committed function prints the same six outputs as against the candidate.
- Plan amended: `cargo test --lib` died of `SIGHUP` in 4 of 15 runs: the trap test raised a real signal, which set the flag of a parallel `rollback` or `sync` test's `Interrupt`, and that test re-raised it. The test moved to `tests/interrupt.rs`, its own process; 15 of 15 runs passed after. Unit counts drop by one and integration counts rise to 12: Task 2 expects `190` and `12`, Task 3 `191` and `12`, Tasks 4 and 5 `193` and `12`. This is the unexplained failure recorded in the Phase 3b family 3 receipt.
- Next: Task 2 Step 5 (Tasks 2–4 are written and pass `cargo test`; they commit one at a time).
- Blocker: none.

### 2026-09-14 — Tasks 2, 3, and 4 done
- Commits: `2e66f85` "feat(payload): add override_path and register edit modules" holds Tasks 2 and 3 (`src/yaml_edit.rs`, `src/edit_paths.rs`, `src/payload.rs`, `src/lib.rs`); it was not made by this run but by another session on the same checkout (`mobile-25` was active), 7 seconds after `de7137c`, with the working-tree files unchanged, and it is left as is. Then "feat(native): port enable and disable".
- Verified: Task 2 `cargo test` 190 lib, 11 cli, 1 interrupt; Task 3 190 lib after the move (`edit_paths` test included); Task 4 192 lib, 11 cli, 1 interrupt; fmt and clippy exit 0. `AGENTSYNC_NATIVE=1 bats tests/enable.bats` 15 `ok`, 0 `not ok`; `bats -f 'enable|disable' tests/native_parity.bats` 7 `ok`. Mutation: `Run {} to clean up.` failed `parity: disable edits block and inline lists and legacy flags like Bash` with that diff; reverted, rebuilt.
- Plan amended: counts are one lower than written (the interrupt test left the lib binary): Tasks 4 and 5 expect `192` lib tests plus `11` and `1` integration.
- Next: Task 5 Step 1.
- Blocker: none.
