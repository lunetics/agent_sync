# Rust Migration Phase 4k: Native `add`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync add` so the binary answers it byte for byte like `cmd_add` and `cmd_add_mcp` in `lib/helpers/add.sh`: the option loops with their usage texts, the kind and name validation, the scaffold of a rule, skill, command, or subagent from the shipped content templates with the `{{TITLE}}`, `{{NAME}}`, and sentinel `name:` substitutions, and the splice of one MCP server into the shared `.ai/src/mcp.json` through a port of the awk merge, with the file's mode kept. One Bash bug the reference turned up is fixed first.

**Architecture:** `catalog` gains `content_template` (the embedded `lib/templates/content/<kind>.md`). `src/cli/add.rs` holds the command: `add` parses the generic options and scaffolds, `add_mcp` parses its own flag set, `build_entry` renders the server object as `_add_mcp_build_entry` printed it, and `merge` ports the awk program byte for byte (`skip_string`, `skip_value`, `compact`, the first-`"mcpServers"` search, the exit-2 and exit-3 refusals); `staging::write_beside` replaces the file as `tmp_sibling` plus `mv` did, mode included. `main` hands it the logical project root (`AGENTSYNC_REPO_ROOT` or `PWD`, as Bash took it, no discovery). The seam stays the CLI process boundary: `tests/add.bats` under `AGENTSYNC_NATIVE=1`, two tree-parity fixtures, and a 63-scenario reference harness that records each file's mode and bytes.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-16-rust-migration-phase-4j-doctor.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `add` writes one scaffold below `.ai/src/`, or `.ai/src/mcp.json`; nothing else.
- No binary ships to users; without a binary every command runs in Bash. One Bash change, Task 1, in its own commit with a regression test; `lib/**/*.sh` and `bin/agentsync.sh` stay clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment.
- Disk-touching unit tests are `#[cfg(unix)]`.
- Every expected value was captured from the fixed Bash on 2026-09-16: `phase4k/add_reference.sh` (63 scenarios, 760 lines with each written file's mode and bytes), reproduced in Task 2 Step 5.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-16 under the maintainer's standing instruction to run Phase 4 to its close; each was checked against Bash before the call.

1. **Fix the silent exit on a flag without its value.** `add mcp gh --url` runs `shift 2` on one remaining argument; the builtin fails, `set -e` ends the run with status 1 and no message, and nothing is written. The skill names a silent exit 1 as a fixture the reference must not carry. **Fix:** `Error: --url requires a value.` plus the `add mcp` usage, exit 1 (Task 1). Alternative: port the silence; rejected because the user gets no hint.
2. **Port the awk merge as it is, its data losses included.** `_add_mcp_merge` re-emits only the `mcpServers` member, takes the first `"mcpServers"` anywhere in the file as that member, stops at the first key that is not a string, and replaces a file without the member wholesale. The canonical file `add mcp` writes never has another member, so these bite only a hand-edited file; they are recorded as quirks 46–48 rather than fixed, since fixing them changes what a documented command writes. Alternative: a real JSON rewrite that keeps the other members; deferred to the post-cutover parser the spec already names.
3. **Quirks and deviations.** Record as known quirks 46–49 the three merge behaviours above and this one: `add mcp` creates `.ai/src/mcp.json` with an empty server map before it validates `--env`, so a bad pair leaves the file behind, and `--args` and `--env` read only the first line of their value. Record as an accepted deviation the Rust I/O message where Bash printed the shell's redirect error when `--force` writes onto a directory. **Recommended:** as listed.

## Module closure

```text
lib/helpers/add.sh               15-21    _add_print_usage
                                 23-33    _add_validate_kind
                                 38-62    _add_validate_name
                                 66-77    _add_resolve_dest; 80-85 _add_resolve_template (catalog::content_template)
                                 89-217   cmd_add (the option loop, the title awk, the sed pass)
                                 221-225  _add_mcp_print_usage
                                 227-232  _add_trim; 235-243 _add_json_escape
                                 247-288  _add_mcp_build_entry
                                 295-402  _add_mcp_merge (the awk program)
                                 404-507  cmd_add_mcp (Task 1 at 426-429)
lib/helpers/tmp.sh               100-122  tmp_sibling (staging::write_beside keeps the mode as cp -p did)
lib/templates/content/*.md                embedded through catalog::content_template
bin/agentsync.sh                 280      _NATIVE_COMMANDS; 385 add's _need list
```

Reused: `paths::logical_root`, `staging::write_beside`, `style`, `cli::customize::put`. The project root is the working directory or `AGENTSYNC_REPO_ROOT`, as `cmd_add` took it: `add` never looks for `.ai` above the working directory, so `add rule x` inside `.ai/` writes `.ai/.ai/src/rules/x.md`, in both engines.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in add native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: the plan's latest commit; `263 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for both files, with `native_parity` run outside the agent sandbox, which refuses the `diff -` stdin operand `src/cli/diff.rs` uses.

---

### Task 1: `add mcp` names a flag that is missing its value

**Files:**
- Modify: `lib/helpers/add.sh` (`cmd_add_mcp`, the value-taking options)
- Test: `tests/add.bats`

**Interfaces:** none.

- [x] **Step 1: Write the failing test**

Append to `tests/add.bats`:

```bash
@test "add mcp names a flag that is missing its value" {
    run run_agentsync add mcp gh --url
    [ "$status" -eq 1 ]
    [ -n "$output" ]
    grep -q -- '--url requires a value\.' <<<"$output"
    grep -q 'Usage: agentsync add mcp' <<<"$output"
    [ ! -e ".ai/src/mcp.json" ]
}
```

The assertions are `[ ]` and `grep`, not `[[ ]]`: after this `run`, a `[[ ]]` that fails does not fail the test under Bash 3.2 and bats 1.13 (reproduced with an impossible pattern), while `[ -n "$output" ]` does.

Run: `bats --tap -f 'missing its value' tests/add.bats`
Expected: `not ok 1 add mcp names a flag that is missing its value`.

- [x] **Step 2: Refuse before shifting**

In `cmd_add_mcp`, replace the four `--url`, `--command`, `--args`, and `--env` option lines with:

```bash
            --url|--command|--args|--env)
                if [[ $# -lt 2 ]]; then
                    echo "$(_red "Error"): $1 requires a value." >&2
                    _add_mcp_print_usage
                    exit 1
                fi
                case "$1" in
                    --url)     url="$2" ;;
                    --command) command_str="$2" ;;
                    --args)    args_str="$2" ;;
                    --env)     env_str="$2" ;;
                esac
                shift 2
                ;;
```

- [x] **Step 3: Run the tests, confirm green**

```bash
bats --tap tests/add.bats | grep -c '^ok'
shellcheck -x -S warning -e SC1091 lib/helpers/add.sh
```

Expected: `36`; ShellCheck exit 0.

- [x] **Step 4: Commit**

```bash
git add lib/helpers/add.sh tests/add.bats docs/plans/2026-09-16-rust-migration-phase-4k-add.md
git commit -m "fix(add): name a flag that is missing its value"
```

---

### Task 2: Port `add`

**Files:**
- Create: `src/cli/add.rs`
- Modify: `src/catalog.rs`, `src/cli/mod.rs`, `src/main.rs`, `bin/agentsync.sh`
- Test: `tests/native_parity.bats`

**Interfaces:**

```rust
// src/catalog.rs
pub fn content_template(kind: &str) -> Option<&'static str>;
// src/cli/add.rs
pub fn add(
    args: &[String],
    root: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error>;
```

- [x] **Step 1: Parity fixtures, Bash side**

Append to `tests/native_parity.bats`:

```bash
# ── add ──────────────────────────────────────────────────────────────────────
# add writes one scaffold or the shared MCP source, so every call compares
# whole trees.

@test "parity: add scaffolds each kind and refuses bad names like Bash" {
    assert_tree_parity add
    assert_tree_parity add --help
    assert_tree_parity add rule
    assert_tree_parity add --bogus rule x
    assert_tree_parity add banana x
    assert_tree_parity add rule a b
    assert_tree_parity add rule sub/dir
    assert_tree_parity add rule ..evil
    assert_tree_parity add rule .hidden
    assert_tree_parity add rule "my rule"
    assert_tree_parity add rule testing
    assert_tree_parity add skill my-skill
    assert_tree_parity add command deploy
    assert_tree_parity add subagent reviewer
    assert_tree_parity add rule my_tool-x2--y
    run_agentsync add rule testing >/dev/null
    assert_tree_parity add rule testing
    printf 'custom content\n' > .ai/src/rules/testing.md
    assert_tree_parity add --force rule testing
    assert_tree_parity add -f skill my-skill
    mkdir -p .ai/src/rules/dirname.md
    assert_tree_parity add rule dirname
}

@test "parity: add mcp creates, appends, escapes, and refuses like Bash" {
    assert_tree_parity add mcp
    assert_tree_parity add mcp --help
    assert_tree_parity add mcp gh --bogus
    assert_tree_parity add mcp gh extra --command x
    assert_tree_parity add mcp bad/name --command x
    assert_tree_parity add mcp gh
    assert_tree_parity add mcp gh --url u --command c
    assert_tree_parity add mcp gh --url
    assert_tree_parity add mcp bad --command c --env NOEQ
    rm -f .ai/src/mcp.json
    assert_tree_parity add mcp github --command "npx @github/mcp-server"
    run_agentsync add mcp github --command "npx @github/mcp-server" >/dev/null
    assert_tree_parity add mcp linear --url "https://mcp.linear.app/sse"
    assert_tree_parity add mcp fs --command fs-server --args "--root /tmp   --debug" --env " TOKEN = abc ,DEBUG=1,, "
    assert_tree_parity add mcp github --command other
    assert_tree_parity add mcp github --command other --force
    assert_tree_parity add mcp weird --command 'say "hi" path\to' --env 'MSG=a"b' --args 'x"y z\w'
    printf '{"mcpServers": {"a": { "env": { "X": "}\\"{" }, "args": [ "1", [ "2" ] ] }, "b": "str"}, "trailing": true}\n' > .ai/src/mcp.json
    assert_tree_parity add mcp d --url "https://d"
    printf '{"servers": {}}\n' > .ai/src/mcp.json
    assert_tree_parity add mcp n --url "https://n"
    printf '{"mcpServers": []}\n' > .ai/src/mcp.json
    assert_tree_parity add mcp n --url "https://n"
}
```

Run, outside the sandbox: `AGENTSYNC_NATIVE=0 bats --tap tests/add.bats | grep -c '^ok'`; `bats --tap -f 'parity: add' tests/native_parity.bats`
Expected: `36`; `ok 1` and `ok 2` (the native side still runs Bash until Step 3 lists `add`).

- [x] **Step 2: Write the failing tests**

Create `src/cli/add.rs` with the tests module, and add `pub mod add;` to `src/cli/mod.rs` before `adopt`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_split_on_hyphens_like_the_awk_program() {
        assert_eq!(title("testing"), "Testing");
        assert_eq!(title("my-skill"), "My Skill");
        assert_eq!(title("my_tool-x2--y"), "My_tool X2  Y");
    }

    #[test]
    fn templates_are_filled_like_sed() {
        let skill = render(catalog::content_template("skill").unwrap(), "my-skill");
        assert!(skill.contains("\nname: \"my-skill\"\n"));
        assert!(skill.contains("\n# My Skill\n"));
        let agent = render(catalog::content_template("subagent").unwrap(), "reviewer");
        assert!(agent.starts_with("---\nname: \"reviewer\"\ndescription: >-\n"));
        let rule = render(catalog::content_template("rule").unwrap(), "testing");
        assert!(rule.starts_with("# Testing\n\n- One imperative"));
        assert!(rule.ends_with("native glob trigger.\n"));
    }

    #[test]
    fn names_are_refused_like_add_validate_name() {
        let style = Style::plain();
        let refusal = |name: &str| validate_name(name, &style).unwrap_or_default();
        assert_eq!(refusal(""), "Error: Name is empty.\n");
        assert_eq!(
            refusal("sub/dir"),
            "Error: Name cannot contain path separators: sub/dir\n"
        );
        assert_eq!(
            refusal("a\\b"),
            "Error: Name cannot contain path separators: a\\b\n"
        );
        assert_eq!(
            refusal("..evil"),
            "Error: Name cannot contain '..': ..evil\n"
        );
        assert_eq!(
            refusal(".hidden"),
            "Error: Name cannot start with '.' or '-': .hidden\n"
        );
        assert_eq!(
            refusal("my rule"),
            "Error: Name may only contain letters, digits, hyphens, and underscores: my rule\n"
        );
        assert_eq!(
            refusal("règle"),
            "Error: Name may only contain letters, digits, hyphens, and underscores: règle\n"
        );
        assert_eq!(refusal("my_rule-42"), "");
    }

    #[test]
    fn entries_are_built_like_add_mcp_build_entry() {
        let style = Style::plain();
        let entry = |url: &str, command: &str, args: &str, env: &str| {
            build_entry(url, command, args, env, &style)
        };
        assert_eq!(
            entry("https://mcp.linear.app/sse", "", "", "").unwrap(),
            "{\"type\": \"http\", \"url\": \"https://mcp.linear.app/sse\"}"
        );
        assert_eq!(
            entry(
                "",
                "fs-server",
                "--root /tmp   --debug",
                " TOKEN = abc ,DEBUG=1,, "
            )
            .unwrap(),
            "{\"command\": \"fs-server\", \"args\": [\"--root\", \"/tmp\", \"--debug\"], \"env\": {\"TOKEN\": \" abc\", \"DEBUG\": \"1\"}}"
        );
        assert_eq!(
            entry("", "say \"hi\" path\\to", "x\"y z\\w", "MSG=a\"b").unwrap(),
            "{\"command\": \"say \\\"hi\\\" path\\\\to\", \"args\": [\"x\\\"y\", \"z\\\\w\"], \"env\": {\"MSG\": \"a\\\"b\"}}"
        );
        assert_eq!(entry("", "c", "", ",").unwrap(), "{\"command\": \"c\"}");
        assert_eq!(
            entry("", "a\tb\nc\rd", "", "K=v\tw").unwrap(),
            "{\"command\": \"a\\tb\\nc\\rd\", \"env\": {\"K\": \"v\\tw\"}}"
        );
        assert_eq!(
            entry("https://x", "", "", "A= b =c").unwrap(),
            "{\"type\": \"http\", \"url\": \"https://x\", \"env\": {\"A\": \" b =c\"}}"
        );
        assert_eq!(
            entry("", "c", "", "NOEQ").unwrap_err(),
            "Error: --env entry 'NOEQ' must be KEY=VALUE.\n"
        );
    }

    fn merged(content: &str, server: &str, entry: &str, force: bool) -> String {
        String::from_utf8(
            merge(content.as_bytes(), server, entry, force)
                .ok()
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn servers_are_spliced_like_the_awk_merge() {
        let github = "{\"command\": \"npx @github/mcp-server\"}";
        let one = merged(EMPTY_MCP, "github", github, false);
        assert_eq!(
            one,
            "{\n  \"mcpServers\": {\n    \"github\": {\"command\":\"npx @github/mcp-server\"}\n  }\n}\n"
        );
        let two = merged(
            &one,
            "linear",
            "{\"type\": \"http\", \"url\": \"https://l\"}",
            false,
        );
        assert_eq!(
            two,
            "{\n  \"mcpServers\": {\n    \"github\": {\"command\":\"npx @github/mcp-server\"},\n    \"linear\": {\"type\":\"http\",\"url\":\"https://l\"}\n  }\n}\n"
        );
        assert!(matches!(
            merge(two.as_bytes(), "github", "{\"command\": \"other\"}", false),
            Err(Refusal::Exists)
        ));
        assert_eq!(
            merged(&two, "github", "{\"command\": \"other\"}", true),
            "{\n  \"mcpServers\": {\n    \"github\": {\"command\":\"other\"},\n    \"linear\": {\"type\":\"http\",\"url\":\"https://l\"}\n  }\n}\n"
        );
        assert_eq!(
            merged(
                "{\"mcpServers\":{\"a\":{\"k\":\"v\"}}}",
                "b",
                "{\"type\": \"http\", \"url\": \"https://b\"}",
                false
            ),
            "{\n  \"mcpServers\": {\n    \"a\": {\"k\":\"v\"},\n    \"b\": {\"type\":\"http\",\"url\":\"https://b\"}\n  }\n}\n"
        );
        assert_eq!(
            merged(
                "{\"mcpServers\": {\"esc\\\"aped\": {}}}\n",
                "n",
                "{\"type\": \"http\", \"url\": \"https://n\"}",
                false
            ),
            "{\n  \"mcpServers\": {\n    \"esc\\\"aped\": {},\n    \"n\": {\"type\":\"http\",\"url\":\"https://n\"}\n  }\n}\n"
        );
        assert_eq!(
            merged(
                "{\"mcpServers\": {\"a\": { \"env\": { \"X\": \"}\\\"{\" }, \"args\": [ \"1\", [ \"2\" ] ] }, \"b\": \"str\", \"c\": 12}, \"trailing\": true}\n",
                "d",
                "{\"type\": \"http\", \"url\": \"https://d\"}",
                false
            ),
            "{\n  \"mcpServers\": {\n    \"a\": {\"env\":{\"X\":\"}\\\"{\"},\"args\":[\"1\",[\"2\"]]},\n    \"b\": \"str\",\n    \"c\": 12,\n    \"d\": {\"type\":\"http\",\"url\":\"https://d\"}\n  }\n}\n"
        );
    }

    #[test]
    fn odd_files_are_replaced_dropped_or_refused_like_the_awk_merge() {
        let n = "{\"type\": \"http\", \"url\": \"https://n\"}";
        let fresh = "{\n  \"mcpServers\": {\n    \"n\": {\"type\":\"http\",\"url\":\"https://n\"}\n  }\n}\n";
        assert_eq!(merged("{\"servers\": {}}\n", "n", n, false), fresh);
        assert_eq!(merged("garbage\n", "n", n, false), fresh);
        assert_eq!(merged("", "n", n, false), fresh);
        assert_eq!(
            merged(
                "{\"mcpServers\": {\"a\": {}, 1: {}, \"z\": {}}}\n",
                "n",
                n,
                false
            ),
            "{\n  \"mcpServers\": {\n    \"a\": {},\n    \"n\": {\"type\":\"http\",\"url\":\"https://n\"}\n  }\n}\n"
        );
        assert!(matches!(
            merge(b"{\"mcpServers\": []}\n", "n", n, false),
            Err(Refusal::Malformed)
        ));
        assert!(matches!(
            merge(
                b"{\n  \"other\": {\"mcpServers\": 1},\n  \"mcpServers\": {}\n}\n",
                "n",
                n,
                false
            ),
            Err(Refusal::Malformed)
        ));
    }

    #[cfg(unix)]
    fn run(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = add(&args, root, &Style::plain(), &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[cfg(unix)]
    #[test]
    fn a_rule_is_scaffolded_once_like_cmd_add() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let (status, out, err) = run(&root, &["rule", "testing"]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "\nCreated rule: .ai/src/rules/testing.md\n\nEdit the file, then run agentsync sync to propagate.\n\n"
        );
        let written = std::fs::read_to_string(dir.path().join(".ai/src/rules/testing.md")).unwrap();
        assert_eq!(
            written,
            catalog::content_template("rule")
                .unwrap()
                .replace("{{TITLE}}", "Testing")
        );
        let (status, out, err) = run(&root, &["rule", "testing"]);
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            format!(
                "Error: Already exists: {root}/.ai/src/rules/testing.md\n\nPass --force to overwrite, or pick a different name.\n"
            )
        );
        let (status, _, err) = run(&root, &["rule", "testing", "--force", "extra"]);
        assert_eq!(status, 1);
        assert!(err.starts_with(
            "Error: Unexpected argument: extra\nError: agentsync add <kind> <name> [options]\n"
        ));
        let (status, out, _) = run(&root, &["-h"]);
        assert_eq!((status, out.as_str()), (0, USAGE));
    }

    #[cfg(unix)]
    #[test]
    fn a_server_is_added_once_like_cmd_add_mcp() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let (status, out, err) = run(
            &root,
            &["mcp", "github", "--command", "npx @github/mcp-server"],
        );
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "\nCreated shared MCP source: .ai/src/mcp.json\nAdded server: github\n\nThis MCP source applies to every enabled tool on next agentsync sync.\nAdd a per-tool override with agentsync customize <tool> mcp if needed.\n\n"
        );
        let file = dir.path().join(".ai/src/mcp.json");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "{\n  \"mcpServers\": {\n    \"github\": {\"command\":\"npx @github/mcp-server\"}\n  }\n}\n"
        );
        let (status, out, err) = run(&root, &["mcp", "github", "--command", "other"]);
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "Error: server 'github' already exists. Pass --force to overwrite.\n"
        );
        let (status, out, err) = run(&root, &["mcp", "gh", "--url"]);
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(err, format!("Error: --url requires a value.\n{MCP_USAGE}"));
        let (status, out, err) = run(&root, &["mcp", "bad", "--command", "c", "--env", "NOEQ"]);
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(err, "Error: --env entry 'NOEQ' must be KEY=VALUE.\n");
        let (status, _, err) = run(&root, &["mcp", "-h"]);
        assert_eq!((status, err.as_str()), (0, MCP_USAGE));
    }
}
```

Run: `cargo test cli::add 2>&1 | grep -E '^error' | head -3`
Expected: compile errors naming `title`, `render`, `validate_name`, `build_entry`, `merge`, and `add`.

- [x] **Step 3: Write the implementation**

In `src/catalog.rs`, after `base_tool_yaml`:

```rust
/// Shipped `lib/templates/content/<kind>.md`, the scaffold `add` fills in.
pub fn content_template(kind: &str) -> Option<&'static str> {
    TEMPLATES
        .get_file(format!("content/{kind}.md"))?
        .contents_utf8()
}
```

Prepend to `src/cli/add.rs`:

```rust
//! `agentsync add`: `cmd_add` and `cmd_add_mcp` of `lib/helpers/add.sh`,
//! which scaffold a rule, skill, command, or subagent from the shipped content
//! templates and splice one server into the shared `.ai/src/mcp.json`.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::customize::put;
use crate::style::Style;
use crate::{Error, catalog, staging};

const USAGE: &str = "Usage: agentsync add <kind> <name> [--force]
       agentsync add mcp <server> (--url URL | --command CMD [--args 'a b'] [--env K=V,...])

  Scaffold a new entry under .ai/src/:
    rule       Create .ai/src/rules/<name>.md
    skill      Create .ai/src/skills/<name>/SKILL.md
    command    Create .ai/src/commands/<name>.md
    subagent   Create .ai/src/agents/<name>.md
    mcp        Add an MCP server entry to .ai/src/mcp.json

  --force, -f   Overwrite an existing file.

  Edit the scaffold, then run `agentsync sync` to propagate.
";

const MCP_USAGE: &str = "Usage: agentsync add mcp <server> (--url URL | --command CMD)
                                  [--args \"a b c\"] [--env K=V[,K=V...]]
                                  [--force]
";

const EMPTY_MCP: &str = "{\n  \"mcpServers\": {}\n}\n";

/// `_add_print_usage`.
fn usage(style: &Style, err: &mut dyn Write) -> Result<(), Error> {
    put(
        err,
        format!(
            "{}: agentsync add <kind> <name> [options]\n\nKinds: rule, skill, command, subagent, mcp\n\nMCP: agentsync add mcp <server> (--url URL | --command CMD [--args 'a b'] [--env K=V,...])\n",
            style.red("Error")
        )
        .as_bytes(),
    )
}

/// `_add_validate_name`: the refusal, or nothing.
fn validate_name(name: &str, style: &Style) -> Option<String> {
    let error = style.red("Error");
    if name.is_empty() {
        return Some(format!("{error}: Name is empty.\n"));
    }
    if name.contains('/') || name.contains('\\') {
        return Some(format!(
            "{error}: Name cannot contain path separators: {name}\n"
        ));
    }
    if name.contains("..") {
        return Some(format!("{error}: Name cannot contain '..': {name}\n"));
    }
    if name.starts_with('.') || name.starts_with('-') {
        return Some(format!(
            "{error}: Name cannot start with '.' or '-': {name}\n"
        ));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Some(format!(
            "{error}: Name may only contain letters, digits, hyphens, and underscores: {name}\n"
        ));
    }
    None
}

/// `_add_resolve_dest`, below `.ai/src/`.
fn dest_rel(kind: &str, name: &str) -> String {
    match kind {
        "rule" => format!(".ai/src/rules/{name}.md"),
        "skill" => format!(".ai/src/skills/{name}/SKILL.md"),
        "command" => format!(".ai/src/commands/{name}.md"),
        _ => format!(".ai/src/agents/{name}.md"),
    }
}

/// `{{TITLE}}`: hyphens become spaces and each part starts upper-case, as the
/// awk program splits it, so `my_tool-x2--y` reads `My_tool X2  Y`.
fn title(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `sed` pass: placeholders everywhere, the sentinel `name:` lines whole.
fn render(template: &str, name: &str) -> String {
    let text = template
        .replace("{{TITLE}}", &title(name))
        .replace("{{NAME}}", name);
    text.split('\n')
        .map(|line| {
            if line == "name: \"content\"" || line == "name: \"template-agent\"" {
                format!("name: \"{name}\"")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `cmd_add`: the report on `out`, refusals on `err`, the status as the result.
pub fn add(
    args: &[String],
    root: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    if args.first().map(String::as_str) == Some("mcp") {
        return add_mcp(&args[1..], root, style, out, err);
    }
    let mut force = false;
    let mut kind = String::new();
    let mut name = String::new();
    for arg in args {
        match arg.as_str() {
            "--force" | "-f" => force = true,
            "--help" | "-h" => {
                put(out, USAGE.as_bytes())?;
                return Ok(0);
            }
            flag if flag.starts_with('-') => {
                put(
                    err,
                    format!("{}: Unknown flag: {flag}\n", style.red("Error")).as_bytes(),
                )?;
                return Ok(1);
            }
            positional => {
                if kind.is_empty() {
                    kind = positional.to_string();
                } else if name.is_empty() {
                    name = positional.to_string();
                } else {
                    put(
                        err,
                        format!(
                            "{}: Unexpected argument: {positional}\n",
                            style.red("Error")
                        )
                        .as_bytes(),
                    )?;
                    usage(style, err)?;
                    return Ok(1);
                }
            }
        }
    }
    if kind.is_empty() || name.is_empty() {
        usage(style, err)?;
        return Ok(1);
    }
    if !matches!(kind.as_str(), "rule" | "skill" | "command" | "subagent") {
        put(
            err,
            format!(
                "{}: Unknown kind '{kind}'.\nValid kinds: rule, skill, command, subagent\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    if let Some(refusal) = validate_name(&name, style) {
        put(err, refusal.as_bytes())?;
        return Ok(1);
    }
    let Some(template) = catalog::content_template(&kind) else {
        put(
            err,
            format!(
                "{}: Template missing: {}/lib/templates/content/{kind}.md\n",
                style.red("Error"),
                crate::paths::ENGINE_ROOT
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let rel = dest_rel(&kind, &name);
    let dest = PathBuf::from(format!("{root}/{rel}"));
    if dest.exists() && !force {
        put(
            err,
            format!(
                "{}: Already exists: {}\n\nPass {} to overwrite, or pick a different name.\n",
                style.red("Error"),
                dest.display(),
                style.cyan("--force")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::write(&dest, render(template, &name)).map_err(|e| Error::io(&dest, e))?;
    put(
        out,
        format!(
            "\n{} {rel}\n\nEdit the file, then run {} to propagate.\n\n",
            style.green(&format!("Created {kind}:")),
            style.cyan("agentsync sync")
        )
        .as_bytes(),
    )
    .map(|()| 0)
}

/// `_add_json_escape`.
fn json_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

/// `read -r` on a here-string: the first line only.
fn first_line(text: &str) -> &str {
    text.split('\n').next().unwrap_or("")
}

/// `_add_mcp_build_entry`: the compact-to-be JSON object, or the `--env`
/// refusal.
fn build_entry(
    url: &str,
    command: &str,
    args: &str,
    env: &str,
    style: &Style,
) -> Result<String, String> {
    let mut fields = if !url.is_empty() {
        format!("\"type\": \"http\", \"url\": \"{}\"", json_escape(url))
    } else {
        let mut fields = format!("\"command\": \"{}\"", json_escape(command));
        if !args.is_empty() {
            let list = first_line(args)
                .split([' ', '\t'])
                .filter(|token| !token.is_empty())
                .map(|token| format!("\"{}\"", json_escape(token)))
                .collect::<Vec<_>>()
                .join(", ");
            fields.push_str(&format!(", \"args\": [{list}]"));
        }
        fields
    };
    if !env.is_empty() {
        let mut members = Vec::new();
        for pair in first_line(env).split(',') {
            let pair = pair.trim_matches(is_blank);
            if pair.is_empty() {
                continue;
            }
            let Some((key, value)) = pair.split_once('=') else {
                return Err(format!(
                    "{}: --env entry '{pair}' must be KEY=VALUE.\n",
                    style.red("Error")
                ));
            };
            members.push(format!(
                "\"{}\": \"{}\"",
                json_escape(key.trim_matches(is_blank)),
                json_escape(value)
            ));
        }
        if !members.is_empty() {
            fields.push_str(&format!(", \"env\": {{{}}}", members.join(", ")));
        }
    }
    Ok(format!("{{{fields}}}"))
}

/// Why `_add_mcp_merge` refused: the server exists (awk exit 3), or the
/// `mcpServers` member is not an object (awk exit 2).
enum Refusal {
    Exists,
    Malformed,
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// awk `skip_string`: `at` is the opening quote; the index past the closing
/// quote, or the end.
fn skip_string(s: &[u8], at: usize) -> usize {
    let mut i = at + 1;
    let mut escaped = false;
    while i < s.len() {
        let c = s[i];
        if escaped {
            escaped = false;
        } else if c == b'\\' {
            escaped = true;
        } else if c == b'"' {
            return i + 1;
        }
        i += 1;
    }
    i
}

/// awk `skip_value`: the index past a string, a balanced object or array,
/// or a scalar, blanks before it skipped.
fn skip_value(s: &[u8], at: usize) -> usize {
    let mut i = at;
    while i < s.len() && is_ws(s[i]) {
        i += 1;
    }
    match s.get(i) {
        Some(b'"') => skip_string(s, i),
        Some(b'{' | b'[') => {
            let mut depth = 0usize;
            let mut in_string = false;
            let mut escaped = false;
            while i < s.len() {
                let c = s[i];
                if in_string {
                    if escaped {
                        escaped = false;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == b'"' {
                        in_string = false;
                    }
                } else if c == b'"' {
                    in_string = true;
                } else if c == b'{' || c == b'[' {
                    depth += 1;
                } else if c == b'}' || c == b']' {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                i += 1;
            }
            i
        }
        _ => {
            while i < s.len() {
                let c = s[i];
                if c == b',' || c == b'}' || c == b']' || is_ws(c) {
                    break;
                }
                i += 1;
            }
            i
        }
    }
}

/// awk `compact`: blanks outside strings dropped.
fn compact(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    let mut in_string = false;
    let mut escaped = false;
    for &c in value {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
        } else if c == b'"' {
            in_string = true;
            out.push(c);
        } else if !is_ws(c) {
            out.push(c);
        }
    }
    out
}

fn slice(s: &[u8], from: usize, to: usize) -> &[u8] {
    if from < to && to <= s.len() {
        &s[from..to]
    } else {
        &[]
    }
}

/// `_add_mcp_merge`: the file re-emitted canonically with `entry` spliced in
/// under `server`. The awk program reads the file record by record, so the
/// text it parses ends in one newline; the first `"mcpServers"` anywhere is
/// the member, whatever surrounds it, and without one the file is replaced.
fn merge(content: &[u8], server: &str, entry: &str, force: bool) -> Result<Vec<u8>, Refusal> {
    let mut s = content.to_vec();
    if !s.is_empty() && s.last() != Some(&b'\n') {
        s.push(b'\n');
    }
    let key = b"\"mcpServers\"";
    let Some(k) = s.windows(key.len()).position(|w| w == key) else {
        let mut out = b"{\n  \"mcpServers\": {\n    \"".to_vec();
        out.extend_from_slice(server.as_bytes());
        out.extend_from_slice(b"\": ");
        out.extend_from_slice(&compact(entry.as_bytes()));
        out.extend_from_slice(b"\n  }\n}\n");
        return Ok(out);
    };
    let mut j = k + key.len();
    while j < s.len() && is_ws(s[j]) {
        j += 1;
    }
    if s.get(j) == Some(&b':') {
        j += 1;
    }
    while j < s.len() && is_ws(s[j]) {
        j += 1;
    }
    if s.get(j) != Some(&b'{') {
        return Err(Refusal::Malformed);
    }
    let obj_start = j;
    let obj_end = skip_value(&s, j);
    let mut names: Vec<&[u8]> = Vec::new();
    let mut values: Vec<&[u8]> = Vec::new();
    let mut p = obj_start + 1;
    while p + 1 < obj_end {
        let c = s[p];
        if is_ws(c) || c == b',' {
            p += 1;
            continue;
        }
        if c != b'"' {
            break;
        }
        let key_end = skip_string(&s, p);
        let mut q = key_end;
        while q < s.len() && is_ws(s[q]) {
            q += 1;
        }
        if s.get(q) == Some(&b':') {
            q += 1;
        }
        while q < s.len() && is_ws(s[q]) {
            q += 1;
        }
        let value_end = skip_value(&s, q);
        names.push(slice(&s, p + 1, key_end.saturating_sub(1)));
        values.push(slice(&s, q, value_end));
        p = value_end;
    }
    let mut entries: Vec<(&[u8], Vec<u8>)> = names
        .iter()
        .zip(&values)
        .map(|(name, value)| (*name, value.to_vec()))
        .collect();
    match entries
        .iter()
        .position(|(name, _)| *name == server.as_bytes())
    {
        Some(_) if !force => return Err(Refusal::Exists),
        Some(found) => entries[found].1 = entry.as_bytes().to_vec(),
        None => entries.push((server.as_bytes(), entry.as_bytes().to_vec())),
    }
    let mut out = b"{\n  \"mcpServers\": {".to_vec();
    let last = entries.len() - 1;
    for (index, (name, value)) in entries.iter().enumerate() {
        out.extend_from_slice(b"\n    \"");
        out.extend_from_slice(name);
        out.extend_from_slice(b"\": ");
        out.extend_from_slice(&compact(value));
        if index < last {
            out.push(b',');
        }
    }
    out.extend_from_slice(b"\n  }\n}\n");
    Ok(out)
}

/// `cmd_add_mcp`.
fn add_mcp(
    args: &[String],
    root: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut server = String::new();
    let mut url = String::new();
    let mut command = String::new();
    let mut args_str = String::new();
    let mut env_str = String::new();
    let mut force = false;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--url" | "--command" | "--args" | "--env" => {
                let Some(value) = args.get(i + 1) else {
                    put(
                        err,
                        format!("{}: {arg} requires a value.\n", style.red("Error")).as_bytes(),
                    )?;
                    put(err, MCP_USAGE.as_bytes())?;
                    return Ok(1);
                };
                match arg {
                    "--url" => url = value.clone(),
                    "--command" => command = value.clone(),
                    "--args" => args_str = value.clone(),
                    _ => env_str = value.clone(),
                }
                i += 2;
            }
            "--force" | "-f" => {
                force = true;
                i += 1;
            }
            "-h" | "--help" => {
                put(err, MCP_USAGE.as_bytes())?;
                return Ok(0);
            }
            flag if flag.starts_with('-') => {
                put(
                    err,
                    format!("{}: Unknown flag: {flag}\n", style.red("Error")).as_bytes(),
                )?;
                put(err, MCP_USAGE.as_bytes())?;
                return Ok(1);
            }
            positional => {
                if server.is_empty() {
                    server = positional.to_string();
                    i += 1;
                } else {
                    put(
                        err,
                        format!(
                            "{}: Unexpected argument: {positional}\n",
                            style.red("Error")
                        )
                        .as_bytes(),
                    )?;
                    put(err, MCP_USAGE.as_bytes())?;
                    return Ok(1);
                }
            }
        }
    }
    if server.is_empty() {
        put(err, MCP_USAGE.as_bytes())?;
        return Ok(1);
    }
    if let Some(refusal) = validate_name(&server, style) {
        put(err, refusal.as_bytes())?;
        return Ok(1);
    }
    if url.is_empty() && command.is_empty() {
        put(
            err,
            format!(
                "{}: one of --url or --command is required.\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        put(err, MCP_USAGE.as_bytes())?;
        return Ok(1);
    }
    if !url.is_empty() && !command.is_empty() {
        put(
            err,
            format!(
                "{}: --url and --command are mutually exclusive.\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let mcp_file = PathBuf::from(format!("{root}/.ai/src/mcp.json"));
    let mut created = false;
    if !mcp_file.is_file() {
        let parent = Path::new(root).join(".ai/src");
        std::fs::create_dir_all(&parent).map_err(|e| Error::io(&parent, e))?;
        std::fs::write(&mcp_file, EMPTY_MCP).map_err(|e| Error::io(&mcp_file, e))?;
        created = true;
    }
    let entry = match build_entry(&url, &command, &args_str, &env_str, style) {
        Ok(entry) => entry,
        Err(refusal) => {
            put(err, refusal.as_bytes())?;
            return Ok(1);
        }
    };
    let content = std::fs::read(&mcp_file).map_err(|e| Error::io(&mcp_file, e))?;
    match merge(&content, &server, &entry, force) {
        Ok(bytes) => staging::write_beside(&mcp_file, &bytes)?,
        Err(Refusal::Exists) => {
            put(
                err,
                format!(
                    "{}: server '{server}' already exists. Pass --force to overwrite.\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(1);
        }
        Err(Refusal::Malformed) => {
            put(
                err,
                format!(
                    "{}: failed to update {}\n",
                    style.red("Error"),
                    mcp_file.display()
                )
                .as_bytes(),
            )?;
            return Ok(1);
        }
    }
    let headline = if created {
        style.green("Created shared MCP source:")
    } else {
        style.green("Updated shared MCP source:")
    };
    put(
        out,
        format!(
            "\n{headline} .ai/src/mcp.json\n{} {server}\n\nThis MCP source applies to every enabled tool on next {}.\nAdd a per-tool override with {} if needed.\n\n",
            style.green("Added server:"),
            style.cyan("agentsync sync"),
            style.cyan("agentsync customize <tool> mcp")
        )
        .as_bytes(),
    )
    .map(|()| 0)
}
```

In `src/main.rs`, before the `doctor` block:

```rust
    if args.first().and_then(|a| a.to_str()) == Some("add") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let env_root = var("AGENTSYNC_REPO_ROOT").filter(|root| !root.is_empty());
        let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        let root = paths::logical_root(env_root.as_deref(), &cwd, var("PWD").as_deref());
        return cli::add::add(
            &rest,
            &root,
            &Style::for_stdout(),
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
```

In `bin/agentsync.sh:280` append `add`:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check sync rollback enable disable customize show diff simplify resolve upgrade-config profile dedupe adopt migrate refresh init doctor add "
```

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
printf 'add native=%s\n' "$(AGENTSYNC_NATIVE=1 bats --tap tests/add.bats | grep -c '^not ok')"
bats --tap -f 'parity: add' tests/native_parity.bats
```

Expected: `271 passed`, `0`, `11`, `1`; `0`; `ok 1` and `ok 2`.

- [x] **Step 5: Prove the fixture bites, run the reference, lint, commit**

Change `Created {kind}:` to `Created {kind}` in `src/cli/add.rs`, rebuild, rerun `bats --tap -f 'parity: add scaffolds' tests/native_parity.bats`: `not ok 1` with the `Created rule:` line in the diff; revert and rebuild.

Recreate the harness when the session scratchpad no longer holds `phase4k/`. It takes `<engine 0|1|2> <repo root> <out file>` (mode 2 calls the debug binary directly, for a command the dispatcher does not delegate yet), masks the project and the work directory, and prints each written file's mode (macOS `stat -f%Lp`) and bytes after the call.

`phase4k/add_reference.sh`:

```bash
#!/usr/bin/env bash
# Usage: add_reference.sh <engine 0|1|2> <repo root> <out file>
# Runs every `add` branch in fresh projects and prints status, masked output,
# and the files each call left behind (mode and content). Mode 2 calls the
# debug binary directly, for a command the dispatcher does not delegate yet.
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
unset AGENTSYNC_REPO_ROOT AGENTSYNC_CONFIG_PATH
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
mask() {
    local text; text=$(cat)
    local phys; phys=$(cd -P "$P" && pwd)
    text=${text//"$phys"/<root>}
    text=${text//"$P"/<root>}
    text=${text//"$WORK"/<work>}
    printf '%s\n' "$text"
}
engine() {
    if [[ "$MODE" == 2 ]]; then
        AGENTSYNC_ENGINE_VERSION="$(cat "$REPO/VERSION")" "$REPO/target/debug/agentsync" "$@"
    else
        bash "$REPO/bin/agentsync.sh" "$@"
    fi
}
# The files under .ai/src that the scenario may have touched: mode and bytes.
show() {
    local f
    for f in "$@"; do
        if [[ -f "$P/$f" ]]; then
            printf -- '--- %s mode=%s\n' "$f" "$(stat -f%Lp "$P/$f")"
            cat "$P/$f"
            printf -- '--- end %s\n' "$f"
        elif [[ -d "$P/$f" ]]; then
            printf -- '--- %s is a directory\n' "$f"
        else
            printf -- '--- %s absent\n' "$f"
        fi
    done
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
run_show() {
    local name="$1"; shift
    local files="$1"; shift
    local rc=0 output
    output=$(cd "$P" && engine "$@" 2>&1) || rc=$?
    local shown; shown=$(cd "$P" && show $files)
    report "$name :: agentsync $*" "$rc" "$output"$'\n'"$shown"
}

fresh; bash_init
run "no-args" add
run "only-kind" add rule
run "help" add --help
run "help-short" add -h
run "help-after-positionals" add rule name --help
run "unknown-flag" add --bogus rule x
run "unknown-kind" add banana x
run "extra-arg" add rule a b
run "name-empty" add rule ""
run "name-slash" add rule sub/dir
run "name-backslash" add rule 'a\b'
run "name-dotdot" add rule ..evil
run "name-dot" add rule .hidden
run "name-dash" add rule -x
run "name-space" add rule "my rule"
run "name-ext" add rule testing.md
run "name-unicode" add rule "règle"
run_show "rule" ".ai/src/rules/testing.md" add rule testing
run_show "skill" ".ai/src/skills/my-skill/SKILL.md" add skill my-skill
run_show "command" ".ai/src/commands/deploy.md" add command deploy
run_show "subagent" ".ai/src/agents/reviewer.md" add subagent reviewer
run "subagent-shipped" add subagent code-reviewer
run_show "title-shapes" ".ai/src/rules/my_tool-x2--y.md" add rule my_tool-x2--y
run "exists" add rule testing
printf 'custom content\n' > "$P/.ai/src/rules/testing.md"
run_show "force" ".ai/src/rules/testing.md" add --force rule testing
run "force-short" add -f rule testing
run "force-late" add rule testing --force
run "exists-skill" add skill my-skill
mkdir -p "$P/.ai/src/rules/dirname.md"
run "dest-is-dir" add rule dirname
run "dest-is-dir-force" add rule dirname --force
run_in "in-subdir" .ai add rule inner
show ".ai/.ai/src/rules/inner.md" >> "$OUT"

fresh
run_show "no-project" ".ai/src/rules/lonely.md" add rule lonely

fresh; bash_init
run "mcp-no-server" add mcp
run "mcp-help" add mcp --help
run "mcp-help-short" add mcp -h
run "mcp-unknown-flag" add mcp gh --bogus
run "mcp-extra" add mcp gh extra --command x
run "mcp-bad-name" add mcp bad/name --command x
run "mcp-neither" add mcp gh
run "mcp-both" add mcp gh --url u --command c
run "mcp-missing-value" add mcp gh --url
show ".ai/src/mcp.json" >> "$OUT"
run_show "mcp-create" ".ai/src/mcp.json" add mcp github --command "npx @github/mcp-server"
run_show "mcp-append-url" ".ai/src/mcp.json" add mcp linear --url "https://mcp.linear.app/sse"
run_show "mcp-args-env" ".ai/src/mcp.json" add mcp fs --command fs-server --args "--root /tmp   --debug" --env " TOKEN = abc ,DEBUG=1,, "
run_show "mcp-env-bad" ".ai/src/mcp.json" add mcp bad --command c --env "NOEQ"
run "mcp-dup" add mcp github --command other
run_show "mcp-force" ".ai/src/mcp.json" add mcp github --command other --force
run_show "mcp-escape" ".ai/src/mcp.json" add mcp weird --command 'say "hi" path\to' --env 'MSG=a"b' --args 'x"y z\w'
run_show "mcp-empty-args-env" ".ai/src/mcp.json" add mcp e --command c --args "" --env ","
run_show "mcp-control-chars" ".ai/src/mcp.json" add mcp t --command $'a\tb\nc\rd' --env $'K=v\tw'
run_show "mcp-env-value-keeps-space" ".ai/src/mcp.json" add mcp sp --url "https://x" --env "A= b =c"

fresh; bash_init
run_show "mcp-env-bad-fresh" ".ai/src/mcp.json" add mcp bad --command c --env NOEQ

fresh; bash_init
printf '{\n  "other": {"mcpServers": 1},\n  "mcpServers": {\n    "a": { "env": { "X": "}\\"{" }, "args": [ "1", [ "2" ] ] },\n    "b": "str",\n    "c": 12\n  },\n  "trailing": true\n}\n' > "$P/.ai/src/mcp.json"
run_show "mcp-nested-values" ".ai/src/mcp.json" add mcp d --url "https://d"
printf '{\n  "mcpServers": {\n    "a": { "env": { "X": "}\\"{" }, "args": [ "1", [ "2" ] ] },\n    "b": "str",\n    "c": 12\n  },\n  "trailing": true\n}\n' > "$P/.ai/src/mcp.json"
run_show "mcp-nested-clean" ".ai/src/mcp.json" add mcp d --url "https://d"
printf '{"mcpServers": {"a": {"k": "v"}}, "other": {"deep": [1, {"x": "y"}]}}\n' > "$P/.ai/src/mcp.json"
run_show "mcp-drops-other-keys" ".ai/src/mcp.json" add mcp b --url "https://b"
printf '{"servers": {}}\n' > "$P/.ai/src/mcp.json"
run_show "mcp-no-mcpservers-key" ".ai/src/mcp.json" add mcp n --url "https://n"
printf '{"mcpServers": []}\n' > "$P/.ai/src/mcp.json"
run_show "mcp-servers-not-object" ".ai/src/mcp.json" add mcp n --url "https://n"
printf 'garbage\n' > "$P/.ai/src/mcp.json"
run_show "mcp-garbage" ".ai/src/mcp.json" add mcp g --url "https://g"
printf '{"mcpServers": {"a": {}, 1: {}, "z": {}}}\n' > "$P/.ai/src/mcp.json"
run_show "mcp-nonstring-key" ".ai/src/mcp.json" add mcp n --url "https://n"
printf '{"mcpServers":{"a":{"k":"v"}}}' > "$P/.ai/src/mcp.json"
run_show "mcp-compact-no-newline" ".ai/src/mcp.json" add mcp b --url "https://b"
printf '{"mcpServers": {"esc\\"aped": {}}}\n' > "$P/.ai/src/mcp.json"
run_show "mcp-escaped-key" ".ai/src/mcp.json" add mcp n --url "https://n"
chmod 600 "$P/.ai/src/mcp.json"
run_show "mcp-keeps-mode" ".ai/src/mcp.json" add mcp m --url "https://m"
mkdir -p "$P/sub"
run_in "mcp-in-subdir" sub add mcp s --url "https://s"
show "sub/.ai/src/mcp.json" >> "$OUT"
```

```bash
bash phase4k/add_reference.sh 0 "$PWD" phase4k/ref_bash.out && bash phase4k/add_reference.sh 1 "$PWD" phase4k/ref_native.out
wc -l < phase4k/ref_native.out
diff phase4k/ref_bash.out phase4k/ref_native.out | grep -c '^[<>]'
```

Expected: `760` lines; `2` differing lines, both the `dest-is-dir-force` scenario's message (Bash `add.sh: line 205: <root>/.ai/src/rules/dirname.md: Is a directory`, native `Error: <root>/.ai/src/rules/dirname.md: Is a directory (os error 21)`, decision 3).

```bash
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/add.rs src/catalog.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-16-rust-migration-phase-4k-add.md
git commit -m "feat(native): port add"
```

---

### Task 3: Verify, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [ ] **Step 1: Spec**

Append to "Known quirks":

```markdown
46. `add mcp` re-emits only the `mcpServers` member of `.ai/src/mcp.json`,
    dropping every other top-level member, and replaces a file without a
    `"mcpServers"` substring with a fresh object holding the one server.
47. `add mcp` takes the first `"mcpServers"` anywhere in the file as the
    member, so a nested decoy makes the merge fail with `failed to update`.
48. `add mcp` stops reading the server map at the first key that is not a
    string and drops the servers after it.
49. `add mcp` creates `.ai/src/mcp.json` with an empty server map before it
    validates `--env`, so a bad pair leaves the file behind; `--args` and
    `--env` read only the first line of their value.
```

Append to "Accepted deviations":

```markdown
- Phase 4k: `add --force` onto a destination that is a directory reports the
  Rust I/O error where Bash printed the shell's redirect message (`add.sh:
  line N: <path>: Is a directory`); the status is unchanged.
```

- [ ] **Step 2: Module map and outputs**

Set the `lib/helpers/add.sh` row to `→ src/cli/add.rs          Phase 4k, ported; the awk merge as merge(), content templates through catalog::content_template`. Regenerate outputs outside the sandbox with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [ ] **Step 3: Verify (outside the agent sandbox)**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
bash phase4i/native_suite.sh "$PWD" both phase4k/suite_both.out && tail -1 phase4k/suite_both.out
```

Expected: `271 passed`, `0`, `11`, `1`; lint exit 0; `TOTAL bash=0 native=0` over the 49 bats files, each run one at a time under both engines.

- [ ] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-16-rust-migration-phase-4k-add.md
git commit -m "docs(native): map the phase 4k modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, every bats file is green under both engines, the two parity fixtures pass, and a `## Completion receipt` records the fresh verification. The next plans cover the standalone commands the spec still names: `export` and `import`, then `generate`, `shell-init`, and `setup-hooks`.

## Run log

### 2026-09-16 — Phase 4k planned
- Commits: this plan.
- Verified: every branch of `cmd_add` and `cmd_add_mcp` was captured with `add_reference.sh` (63 scenarios, each written file's mode and bytes included). The reference turned up one Bash bug: a value-taking flag without its value ends the run silently with status 1; the regression test fails on the committed engine. The Rust in Task 2 was drafted in the tree against the fixed behaviour: `cargo test` 271/0/11/1, fmt and clippy clean; the debug binary, called directly, gave a transcript identical to Bash apart from the fixed silence and the redirect message (decision 3). Baseline `cargo test` 263/0/11/1; `add.bats` 35 and `native_parity.bats` 58 cases green in Bash.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.
