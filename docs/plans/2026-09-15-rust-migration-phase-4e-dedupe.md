# Rust Migration Phase 4e: Native `dedupe`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync dedupe` so the binary answers it byte for byte like `cmd_dedupe` in `lib/helpers/dedupe.sh`, opening the `template_manifest` family with the template hash, the shipped template set, and the parent `.ai/src/` walk.

**Architecture:** `template_manifest::hash` is the first piece of `lib/helpers/template_manifest.sh`; load, record, write, and heal wait for `refresh` and `init`. `catalog::template_sources` lists the embedded templates as `_dedupe_load_template_set` walks `lib/templates`. `paths::find_parent_ai_src` joins `find_workspace_ai_dirs`. `src/cli/dedupe.rs` holds argument parsing, the single-project run, the workspace fan-out, and both prompts; `main` hands it the logical working directory, `project_root`, and a terminal reader only when stdin and stdout are terminals. The seam stays the CLI process boundary: `tests/dedupe.bats` under `AGENTSYNC_NATIVE=1` plus a parity fixture.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-15-rust-migration-phase-4d-profile-upgrade-config.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `dedupe` deletes only child files whose bytes equal the parent's, prunes only directories it emptied up to `.ai/src`, and appends only `template_overrides.declined` entries for shipped templates.
- No binary ships to users; without a binary every command runs in Bash. One Bash change, Task 1's byte-order fix, in its own commit with a regression test; `lib/**/*.sh` stays clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state.
- Disk-touching unit tests are `#[cfg(unix)]`.
- Every expected value was captured from Bash on 2026-09-15: `scratchpad/phase4/dedupe_reference.sh` (every non-interactive branch, output in `dedupe_reference.out`), `dedupe_tty_reference.sh` and `dedupe_tty_streams.sh` (the prompts on a pty), `find_parent_reference.sh`, and `_dedupe_load_template_set` run directly.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-15 under the maintainer's `/decide`: all three as recommended.

1. **The `template_manifest` family is planned in five slices,** by dependency: 4e `dedupe` with the template hash and set; 4f `adopt`; 4g `migrate`; 4h `refresh`; 4i `init`, which needs `adopt`. **Recommended:** this order, recorded in the spec's Phase 4 note by Task 4. Alternative: one plan for the family, about 3,700 lines of Bash in one review.
2. **Fix Bash's locale-dependent order before porting.** `_dedupe_collect` walks `rules`, `commands`, and `agents` with a glob, which Bash sorts by `LC_COLLATE`: under `en_US.UTF-8` the duplicates `B.md`, `_x.md`, `a.md` print as `_x, a, B`, under `C` as `B, _x, a`. The skills walk already sorts with `LC_ALL=C sort -z`. **Recommended:** Task 1 sorts the glob the same way, as `91d5dc3` did for `.gitignore`; the `native-port` triage puts a locale-dependent reference first. Alternative: cite the Phase 2 deviation for globs and port the byte order without touching Bash, leaving Bash users with an order that depends on their locale.
3. **Quirks and deviations.** Record as known quirks 31–32: `dedupe` removes an emptied category directory such as `.ai/src/rules/`, not only emptied skill folders; `dedupe` ignores `AGENTSYNC_CONFIG_PATH`, even a missing one. Record as accepted deviations: the template set comes from the embedded templates where Bash read `$AGENTSYNC_HOME/lib/templates`; an `AGENTSYNC_REPO_ROOT` that does not exist reports `Error: Repository root not found: <path>` where Bash printed `cd`'s message, with status 1 in both, as every ported command already does. **Recommended:** as listed.

## Module closure

```text
lib/helpers/dedupe.sh           37-67   _dedupe_load_template_set, _dedupe_is_template
                                71-78   _dedupe_config_path
                                82-109  _dedupe_show_diff, _dedupe_show_head
                                113-154 _dedupe_prompt_identical, _dedupe_prompt_diverge
                                162-202 _dedupe_collect
                                207-216 _dedupe_delete_and_prune
                                220-345 _dedupe_run_one
                                347-385 _dedupe_usage
                                387-461 cmd_dedupe
lib/helpers/template_manifest.sh 28-40  template_manifest_hash
lib/helpers/paths.sh            468-505 find_parent_ai_src
lib/helpers/shared.sh           shared_parent_src (ported in Phase 2 as overlay::shared_parent_src)
```

Reused: `overlay::shared_parent_src`, `paths::{find_workspace_ai_dirs, parent, leaf, normalize, is_within, logical_root}`, `yaml_edit::list_append`, `manifest::sha256_hex`, `catalog::engine_files`, `cli::customize::put`, `prompts::{is_tty, read_terminal}`, `style`.

---

### Task 0: Baseline

**Files:** none changed.

- [ ] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in dedupe doctor native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: this plan's commit; `211 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for every file.

---

### Task 1: Fix `dedupe`'s locale-dependent order in Bash

**Files:**
- Modify: `lib/helpers/dedupe.sh:168-184`
- Test: `tests/dedupe.bats`

**Interfaces:** none; `_dedupe_collect` keeps its arrays and their order becomes byte order.

- [x] **Step 1: Write the failing test**

Append to `tests/dedupe.bats`:

```bash

@test "dedupe lists duplicates in byte order whatever the locale" {
    locale -a 2>/dev/null | grep -qix 'en_US.utf-\{0,1\}8' || skip "en_US.UTF-8 locale not installed"
    local parent_dir="$TEST_PROJECT/parent"
    local child_dir="$parent_dir/child"
    local name
    mkdir -p "$parent_dir/.ai/src/rules" "$child_dir/.ai/src/rules"
    for name in a _x B; do
        printf '%s\n' "$name" > "$parent_dir/.ai/src/rules/$name.md"
        cp "$parent_dir/.ai/src/rules/$name.md" "$child_dir/.ai/src/rules/$name.md"
    done

    run bash -c "cd '$child_dir' && LC_ALL=en_US.UTF-8 AGENTSYNC_HOME='$REPO_ROOT' bash '$AGENTSYNC_BIN' dedupe --yes"
    [ "$status" -eq 0 ]
    [[ "$output" == *"rules/B.md (deleted)"*"rules/_x.md (deleted)"*"rules/a.md (deleted)"* ]]
}
```

- [x] **Step 2: Run the test, confirm it fails**

Run: `bats --tap -f 'byte order' tests/dedupe.bats`
Expected: `not ok 1 dedupe lists duplicates in byte order whatever the locale`, failing on the `[[ "$output" == … ]]` line.

- [x] **Step 3: Sort the glob by bytes**

In `_dedupe_collect`, replace the flat-category loop body:

```bash
    for cat in rules commands agents; do
        [[ -d "$parent_src/$cat" ]] || continue
        while IFS= read -r -d '' f; do
            [[ -f "$f" ]] || continue
            rel="$cat/$(basename "$f")"
            cf="$child_src/$rel"
            pf="$f"
            [[ -f "$cf" ]] || continue
            ch=$(template_manifest_hash "$cf") || continue
            ph=$(template_manifest_hash "$pf") || continue
            if [[ "$ch" == "$ph" ]]; then
                _DEDUPE_IDENTICAL+=("$rel|$cf")
            else
                _DEDUPE_DIVERGENT+=("$rel|$cf|$pf")
            fi
        done < <(printf '%s\0' "$parent_src/$cat"/*.md | LC_ALL=C sort -z)
    done
```

The glob still skips dot files, and an unmatched `*.md` stays literal and fails `[[ -f ]]`, as before.

- [x] **Step 4: Run the tests, confirm green**

```bash
bats --tap tests/dedupe.bats | grep -c '^ok'
shellcheck -x -S warning -e SC1091 lib/helpers/dedupe.sh
```

Expected: `12`; ShellCheck exit 0.

- [x] **Step 5: Commit**

```bash
git add lib/helpers/dedupe.sh tests/dedupe.bats docs/plans/2026-09-15-rust-migration-phase-4e-dedupe.md
git commit -m "fix(dedupe): list flat-category duplicates in byte order"
```

---

### Task 2: Port the template hash, the template set, and the parent walk

**Files:**
- Create: `src/template_manifest.rs`
- Modify: `src/lib.rs` (`pub mod template_manifest;` after `pub mod style;`), `src/catalog.rs`, `src/paths.rs`

**Interfaces:**
- Produces:
  - `pub fn template_manifest::hash(path: &Path) -> Option<String>`
  - `pub fn catalog::template_sources() -> Vec<String>` — paths below `.ai/src/`, byte order
  - `pub fn paths::find_parent_ai_src(start: &str) -> Option<String>` — `start` is a logical absolute directory

- [ ] **Step 1: Write the failing tests**

Create `src/template_manifest.rs` with its tests module only, and add `pub mod template_manifest;` to `src/lib.rs`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_file_hashes_as_sha256sum_prints_it_and_anything_else_has_no_hash() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        std::fs::write(&file, "shared rule\n").unwrap();
        assert_eq!(
            hash(&file).as_deref(),
            Some("a5aa98439217de45641258de8f69aea33202a07829b4acf375b5484860ca05b8")
        );
        assert_eq!(hash(dir.path()), None);
        assert_eq!(hash(&dir.path().join("missing.md")), None);
    }
}
```

Append to the `tests` module of `src/catalog.rs`:

```rust

    #[test]
    fn the_template_sources_are_the_set_dedupe_loads() {
        assert_eq!(
            template_sources(),
            [
                "AGENTS.md",
                "agents/code-reviewer.md",
                "commands/fix-issue.md",
                "commands/review.md",
                "rules/comments.md",
                "rules/core.md",
                "rules/git.md",
                "skills/comments/SKILL.md",
                "skills/commit/SKILL.md",
                "skills/debug/SKILL.md",
                "skills/humanizer/SKILL.md",
                "skills/humanizer/references/wikipedia_signs_of_ai_writing.md",
                "skills/humanizer/scripts/strip-ai-chars.sh",
                "skills/prompt-engineering/SKILL.md",
                "skills/prompt-engineering/references/agent-persona.md",
                "skills/prompt-engineering/references/metaprompting.md",
                "skills/prompt-engineering/references/snippets.md",
                "skills/refactor/SKILL.md",
                "skills/review/SKILL.md",
            ]
        );
    }
```

Insert into the `tests` module of `src/paths.rs`, before `a_source_link_escaping_the_project_is_named_unless_trusted`:

```rust
    #[cfg(unix)]
    #[test]
    fn the_parent_ai_src_is_the_nearest_ancestors_inside_the_git_repository() {
        let dir = tempfile::tempdir().unwrap();
        let t = disk_canonical(&dir.path().to_string_lossy()).unwrap();
        for sub in [
            "a/.ai/src",
            "a/b/c/.ai/src",
            "a/repo/.git",
            "a/repo/d",
            "a/sub/.git",
            "a/sub/.ai/src",
            "a/sub/e",
            "a/worktree/f",
        ] {
            std::fs::create_dir_all(format!("{t}/{sub}")).unwrap();
        }
        std::fs::write(format!("{t}/a/worktree/.git"), "gitdir: elsewhere\n").unwrap();
        let found = |start: &str| find_parent_ai_src(&format!("{t}/{start}"));
        assert_eq!(found("a/b/c"), Some(format!("{t}/a/.ai/src")));
        assert_eq!(found("a/b"), Some(format!("{t}/a/.ai/src")));
        assert_eq!(found("a"), None);
        assert_eq!(found("a/repo/d"), None);
        assert_eq!(found("a/sub/e"), Some(format!("{t}/a/sub/.ai/src")));
        assert_eq!(found("a/worktree/f"), None);
        assert_eq!(found("a/missing"), None);
    }

```

- [ ] **Step 2: Run the tests, confirm they fail**

Run: `cargo test --lib 2>&1 | grep -E '^error\[E0425\]' | sort -u`
Expected: `cannot find function` errors for `hash`, `template_sources`, and `find_parent_ai_src`.

- [ ] **Step 3: Write the implementation**

Prepend to `src/template_manifest.rs`:

```rust
//! `lib/helpers/template_manifest.sh`: content hashes of the templates copied
//! into `.ai/src/`.

use std::path::Path;

use crate::manifest::sha256_hex;

/// `template_manifest_hash`: the SHA-256 of a file, links followed; `None`
/// when the path is not a readable regular file.
pub fn hash(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    std::fs::read(path).ok().map(|bytes| sha256_hex(&bytes))
}

```

In `src/catalog.rs`, before `GLOBAL_CONFIG`:

```rust
/// `_dedupe_load_template_set`: the shipped `AGENTS.md`, the `*.md` files of
/// `rules`, `commands`, and `agents`, and every file below `skills` whose name
/// does not start with `.`, as paths below `.ai/src/` in byte order.
pub fn template_sources() -> Vec<String> {
    let mut paths: Vec<String> = engine_files()
        .into_iter()
        .filter_map(|(path, _)| {
            let rel = path.strip_prefix("lib/templates/")?;
            let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
            let shipped = match dir {
                "" => rel == "AGENTS.md",
                "rules" | "commands" | "agents" => name.ends_with(".md") && !name.starts_with('.'),
                _ => (dir == "skills" || dir.starts_with("skills/")) && !name.starts_with('.'),
            };
            shipped.then(|| rel.to_string())
        })
        .collect();
    paths.sort();
    paths
}

```

In `src/paths.rs`, before `find_workspace_ai_dirs`:

```rust
/// `find_parent_ai_src`: the nearest ancestor's `.ai/src` above a logical
/// directory, never the directory's own and never outside its git repository.
pub fn find_parent_ai_src(start: &str) -> Option<String> {
    if !Path::new(start).is_dir() {
        return None;
    }
    let mut git_root = None;
    let mut probe = start.to_string();
    while !probe.is_empty() && probe != "/" {
        let dot_git = Path::new(&probe).join(".git");
        if dot_git.is_dir() || dot_git.is_file() {
            git_root = Some(probe);
            break;
        }
        probe = parent(&probe);
    }
    let mut current = start.to_string();
    let mut up = parent(&current);
    while up != current {
        if let Some(root) = &git_root
            && !is_within(&up, root)
        {
            return None;
        }
        let candidate = format!("{up}/.ai/src");
        if Path::new(&candidate).is_dir() {
            return Some(candidate);
        }
        current = up;
        up = parent(&current);
    }
    None
}

```

- [ ] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
```

Expected: `214 passed`, `0`, `11`, `1`; clippy exit 0.

- [ ] **Step 5: Commit**

```bash
git add src/template_manifest.rs src/lib.rs src/catalog.rs src/paths.rs docs/plans/2026-09-15-rust-migration-phase-4e-dedupe.md
git commit -m "feat(native): port the template hash, template set, and parent walk"
```

---

### Task 3: Port `dedupe`

**Files:**
- Create: `src/cli/dedupe.rs`
- Modify: `src/cli/mod.rs` (`pub mod dedupe;` after `pub mod customize;`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Consumes: Task 2's `template_manifest::hash`, `catalog::template_sources`, `paths::find_parent_ai_src`.
- Produces: `pub fn dedupe<'a>(args: &[String], cwd: &str, root: &dyn Fn() -> Result<String, Error>, style: &'a Style, terminal: Option<&'a mut dyn FnMut() -> String>, out: &'a mut dyn Write, err: &'a mut dyn Write) -> Result<u8, Error>` — `terminal` is `Some` only when stdin and stdout are terminals, and yields one typed line per call.

- [ ] **Step 1: Parity fixture, Bash side**

Append to `tests/native_parity.bats`:

```bash

# ── dedupe ───────────────────────────────────────────────────────────────────

@test "parity: dedupe deletes, declines, prunes, and refuses like Bash" {
    mkdir -p .git .ai/src/skills/foo/ref child/.ai/src/rules child/.ai/src/skills/foo/ref empty
    printf 'tools:\n  enabled: []\n' > child/.ai/agent_sync.yaml
    cp .ai/src/rules/comments.md child/.ai/src/rules/comments.md
    printf 'shared rule\n' | tee .ai/src/rules/shared.md > child/.ai/src/rules/shared.md
    printf 'parent\n' > .ai/src/rules/diverge.md
    printf 'child\n' > child/.ai/src/rules/diverge.md
    printf 'skill\n' | tee .ai/src/skills/foo/SKILL.md > child/.ai/src/skills/foo/SKILL.md
    printf 'ref\n' | tee .ai/src/skills/foo/ref/a.md > child/.ai/src/skills/foo/ref/a.md
    printf '.x\n' | tee .ai/src/skills/foo/.x > child/.ai/src/skills/foo/.x
    PARITY_CWD=child assert_tree_parity dedupe --yes
    PARITY_CWD=child assert_tree_parity dedupe
    PARITY_CWD=child assert_tree_parity dedupe --against .. -y
    PARITY_CWD=child assert_tree_parity dedupe --against=../.ai/src -y
    PARITY_CWD=child assert_tree_parity dedupe --against ../nope -y
    PARITY_CWD=child assert_tree_parity dedupe --against ../.ai -y
    PARITY_CWD=empty assert_tree_parity dedupe --workspace -y
    assert_tree_parity dedupe --workspace --yes
    assert_tree_parity dedupe --bogus
    assert_tree_parity dedupe extra
    assert_tree_parity dedupe --against
    assert_tree_parity dedupe --workspace --against x
    assert_tree_parity dedupe -h
    printf '\nshared:\n  path: ".."\n  inherit: rules\n' >> child/.ai/agent_sync.yaml
    rm .ai/src/rules/diverge.md
    mkdir child/.git
    PARITY_CWD=child assert_tree_parity dedupe -y
    rm child/.ai/agent_sync.yaml
    printf 'tools:\n  enabled: []\n' > child/agent_sync.yaml
    PARITY_CWD=child assert_tree_parity dedupe -y
    rm -rf child/.ai/src
    PARITY_CWD=child assert_tree_parity dedupe -y
}
```

Run: `bats --tap -f 'dedupe deletes' tests/native_parity.bats`
Expected: `ok` (the native side still runs Bash).

- [ ] **Step 2: Write the failing tests**

Create `src/cli/dedupe.rs` with the tests module only; add `pub mod dedupe;` to `src/cli/mod.rs`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        parent: String,
        child: String,
    }

    fn fixture(files: &[(&str, &str, &str)]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let parent = format!("{root}/p");
        let child = format!("{parent}/child");
        std::fs::create_dir_all(format!("{parent}/.git")).unwrap();
        std::fs::create_dir_all(format!("{child}/.ai/src")).unwrap();
        std::fs::write(
            format!("{child}/.ai/agent_sync.yaml"),
            "tools:\n  enabled: []\n",
        )
        .unwrap();
        for (rel, in_parent, in_child) in files {
            for (base, text) in [(&parent, in_parent), (&child, in_child)] {
                let path = format!("{base}/.ai/src/{rel}");
                std::fs::create_dir_all(paths::parent(&path)).unwrap();
                std::fs::write(path, text).unwrap();
            }
        }
        Fixture {
            _dir: dir,
            parent,
            child,
        }
    }

    fn comments_template() -> String {
        let (_, bytes) = catalog::engine_files()
            .into_iter()
            .find(|(path, _)| path == "lib/templates/rules/comments.md")
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn call(fx: &Fixture, args: &[&str], answers: Option<&[&str]>) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let child = fx.child.clone();
        let root = move || Ok(child.clone());
        let mut replies = answers.unwrap_or_default().iter().map(|a| a.to_string());
        let mut answer = move || replies.next().unwrap_or_default();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = dedupe(
            &args,
            &fx.child,
            &root,
            &Style::plain(),
            answers.map(|_| &mut answer as &mut dyn FnMut() -> String),
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

    #[test]
    fn yes_deletes_identical_files_declines_templates_and_prunes_like_bash() {
        let comments = comments_template();
        let fx = fixture(&[
            ("rules/comments.md", &comments, &comments),
            ("rules/shared.md", "shared rule\n", "shared rule\n"),
            ("rules/diverge.md", "parent\n", "child\n"),
            ("skills/foo/SKILL.md", "skill\n", "skill\n"),
            ("skills/foo/ref/a.md", "ref\n", "ref\n"),
            ("skills/foo/.x", ".x\n", ".x\n"),
        ]);
        let (status, out, err) = call(&fx, &["--yes"], None);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            format!(
                "\n  AgentSync Dedupe\n  Parent: {}/.ai/src\n  Identical: 4  Divergent: 1\n\n  − rules/comments.md (deleted)\n  − rules/shared.md (deleted)\n  − skills/foo/SKILL.md (deleted)\n  − skills/foo/ref/a.md (deleted)\n  ~ rules/diverge.md (divergent — skipped under --yes; review interactively)\n\n  Done. Deleted: 4 · Kept: 0 · Skipped: 1\n",
                fx.parent
            )
        );
        assert_eq!(
            std::fs::read_to_string(format!("{}/.ai/agent_sync.yaml", fx.child)).unwrap(),
            "tools:\n  enabled: []\n\ntemplate_overrides:\n  declined:\n    - rules/comments.md\n"
        );
        let src = format!("{}/.ai/src", fx.child);
        assert!(Path::new(&format!("{src}/rules/diverge.md")).is_file());
        assert!(Path::new(&format!("{src}/skills/foo/.x")).is_file());
        assert!(!Path::new(&format!("{src}/skills/foo/ref")).exists());
        assert!(!Path::new(&format!("{src}/rules/shared.md")).exists());
    }

    #[test]
    fn answers_on_the_terminal_view_delete_keep_skip_and_quit_like_bash() {
        let comments = comments_template();
        let fx = fixture(&[
            ("rules/comments.md", &comments, &comments),
            ("rules/keep.md", "one\ntwo\n", "one\ntwo\n"),
            ("rules/diverge.md", "parent\n", "child\n"),
            ("rules/zz.md", "x\n", "y\n"),
        ]);
        let answers = [" V ", "x", "D", "", "v", "s", "q"];
        let (status, out, err) = call(&fx, &[], Some(&answers));
        assert_eq!(status, 0);
        assert_eq!(
            out,
            format!(
                "\n  AgentSync Dedupe\n  Parent: {}/.ai/src\n  Identical: 2  Divergent: 2\n\n    deleted + declined.\n    kept.\n    skipped.\n\n  Cancelled. Decisions already made are kept.\n  Done. Deleted: 1 · Kept: 1 · Skipped: 1\n",
                fx.parent
            )
        );
        let identical = "\n  = IDENTICAL: rules/comments.md  (template-derived — declined entry will be added)\n    [d]elete  [k]eep  [v]iew  [q]uit  > ";
        let head: String = comments
            .split_inclusive('\n')
            .take(20)
            .map(|line| format!("  {line}"))
            .collect();
        assert_eq!(
            err,
            format!(
                "{identical}\n  ──── content (first 20 lines) ────\n\n{head}    …\n\n{identical}    (unknown choice — try d, k, v, q)\n{identical}\n  = IDENTICAL: rules/keep.md  \n    [d]elete  [k]eep  [v]iew  [q]uit  > \n  ~ DIVERGES: rules/diverge.md\n    [v]iew  [s]kip  [q]uit  > \n  ──── diff: yours → parent ────\n\n--- yours\n+++ parent\n@@ -1 +1 @@\n-child\n+parent\n\n\n  ~ DIVERGES: rules/diverge.md\n    [v]iew  [s]kip  [q]uit  > \n  ~ DIVERGES: rules/zz.md\n    [v]iew  [s]kip  [q]uit  > "
            )
        );
        assert!(!Path::new(&format!("{}/.ai/src/rules/comments.md", fx.child)).exists());
        assert!(Path::new(&format!("{}/.ai/src/rules/keep.md", fx.child)).is_file());
    }

    #[test]
    fn arguments_and_setup_errors_are_refused_with_the_bash_statuses() {
        let fx = fixture(&[]);
        let (status, out, err) = call(&fx, &["--bogus"], None);
        assert_eq!((status, out.as_str()), (1, ""));
        assert!(err.starts_with("Error: Unknown option: --bogus\n\n  agentsync dedupe — remove"));
        assert!(err.ends_with("    agentsync dedupe --yes\n"));
        assert_eq!(
            call(&fx, &["--against"], None),
            (
                1,
                String::new(),
                "Error: --against requires a path\n".to_string()
            )
        );
        assert_eq!(
            call(&fx, &["--workspace", "--against", "x"], None),
            (
                1,
                String::new(),
                "Error: --workspace and --against are mutually exclusive.\n".to_string()
            )
        );
        assert_eq!(
            call(&fx, &[], None),
            (
                1,
                String::new(),
                "Error: dedupe needs an interactive TTY (or pass --yes).\n".to_string()
            )
        );
        let missing = format!("{}/nope", fx.parent);
        assert_eq!(
            call(&fx, &["--against", &missing, "-y"], None),
            (
                1,
                "\n  AgentSync Dedupe\n".to_string(),
                format!("Error: --against path does not exist: {missing}\n")
            )
        );
        assert_eq!(
            call(&fx, &["-y"], None).1,
            format!(
                "\n  AgentSync Dedupe\n  · no parent .ai/src/ found for {}\n",
                fx.child
            )
        );
        std::fs::create_dir_all(format!("{}/.ai/src", fx.parent)).unwrap();
        assert_eq!(
            call(&fx, &["-y"], None).1,
            format!(
                "\n  AgentSync Dedupe\n  ✓ nothing shared with parent {}/.ai/src\n",
                fx.parent
            )
        );
    }
}
```

Run: `cargo test --lib cli::dedupe 2>&1 | grep -E '^error\[E0425\]' | sort -u`
Expected: `cannot find function `dedupe``.

- [ ] **Step 3: Write the implementation**

Prepend to `src/cli/dedupe.rs`:

```rust
//! `agentsync dedupe`: `cmd_dedupe` of `lib/helpers/dedupe.sh`, which deletes
//! source files a parent `.ai/src/` already holds byte for byte.

use std::io::Write;
use std::path::Path;
use std::process::Command;

use super::customize::put;
use crate::style::Style;
use crate::{Error, catalog, overlay, paths, template_manifest, yaml_edit};

type Answer<'a> = &'a mut dyn FnMut() -> String;

struct Run<'a> {
    style: &'a Style,
    assume_yes: bool,
    templates: Vec<String>,
    terminal: Option<Answer<'a>>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

pub fn dedupe<'a>(
    args: &[String],
    cwd: &str,
    root: &dyn Fn() -> Result<String, Error>,
    style: &'a Style,
    terminal: Option<Answer<'a>>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
) -> Result<u8, Error> {
    let (mut against, mut workspace, mut assume_yes) = (String::new(), false, false);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--against" => match rest.next() {
                Some(path) => against = path.clone(),
                None => {
                    put(
                        err,
                        format!("{}: --against requires a path\n", style.red("Error")).as_bytes(),
                    )?;
                    return Ok(1);
                }
            },
            "--workspace" => workspace = true,
            "--yes" | "-y" => assume_yes = true,
            "--help" | "-h" => {
                put(out, usage(style).as_bytes())?;
                return Ok(0);
            }
            flag if flag.starts_with("--against=") => {
                against = flag["--against=".len()..].to_string();
            }
            flag if flag.starts_with('-') => {
                put(
                    err,
                    format!(
                        "{}: Unknown option: {flag}\n{}",
                        style.red("Error"),
                        usage(style)
                    )
                    .as_bytes(),
                )?;
                return Ok(1);
            }
            value => {
                put(
                    err,
                    format!(
                        "{}: Unexpected argument: {value}\n{}",
                        style.red("Error"),
                        usage(style)
                    )
                    .as_bytes(),
                )?;
                return Ok(1);
            }
        }
    }
    if workspace && !against.is_empty() {
        put(
            err,
            format!(
                "{}: --workspace and --against are mutually exclusive.\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    if !assume_yes && terminal.is_none() {
        put(
            err,
            format!(
                "{}: dedupe needs an interactive TTY (or pass {}).\n",
                style.red("Error"),
                style.cyan("--yes")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }

    let mut run = Run {
        style,
        assume_yes,
        templates: catalog::template_sources(),
        terminal,
        out,
        err,
    };
    put(
        run.out,
        format!("\n{}\n", style.bold("  AgentSync Dedupe")).as_bytes(),
    )?;

    if workspace {
        let ai_dirs = paths::find_workspace_ai_dirs(cwd);
        if ai_dirs.is_empty() {
            put(
                run.err,
                format!(
                    "  {}: No .ai/ directories found below {cwd}\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(1);
        }
        put(
            run.out,
            format!("  Found {} project(s) below {cwd}\n\n", ai_dirs.len()).as_bytes(),
        )?;
        for ai_dir in &ai_dirs {
            let project_root = paths::parent(ai_dir);
            let rel = if project_root == cwd {
                "."
            } else {
                project_root
                    .strip_prefix(&format!("{cwd}/"))
                    .unwrap_or(&project_root)
            };
            put(run.out, format!("  {} {rel}\n", style.cyan("→")).as_bytes())?;
            let status = run_one(&mut run, &project_root, "", cwd)?;
            if status != 0 {
                return Ok(status);
            }
            put(run.out, b"\n")?;
        }
        return Ok(0);
    }

    let repo_root = root()?;
    run_one(&mut run, &repo_root, &against, cwd)
}

/// `_dedupe_config_path`.
fn config_path(repo_root: &str) -> Option<String> {
    [
        format!("{repo_root}/.ai/agent_sync.yaml"),
        format!("{repo_root}/agent_sync.yaml"),
    ]
    .into_iter()
    .find(|path| Path::new(path).is_file())
}

/// `_dedupe_run_one`.
fn run_one(run: &mut Run, repo_root: &str, against: &str, cwd: &str) -> Result<u8, Error> {
    let style = run.style;
    let child_src = format!("{repo_root}/.ai/src");
    if !Path::new(&child_src).is_dir() {
        put(
            run.out,
            format!("  {}: no .ai/src/ in {repo_root}\n", style.yellow("skip")).as_bytes(),
        )?;
        return Ok(0);
    }

    let logical = |path: &str| {
        if path.starts_with('/') {
            paths::normalize(path)
        } else {
            paths::normalize(&format!("{cwd}/{path}"))
        }
    };
    let (parent_src, from_shared) = if against.is_empty() {
        let shared = config_path(repo_root)
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| {
                overlay::shared_parent_src(&String::from_utf8_lossy(&bytes), repo_root)
            });
        match shared {
            Some(parent) => (Some(parent), true),
            None => (paths::find_parent_ai_src(repo_root), false),
        }
    } else if Path::new(&format!("{against}/.ai/src")).is_dir() {
        (Some(logical(&format!("{against}/.ai/src"))), false)
    } else if Path::new(against).is_dir() && paths::leaf(against) == "src" {
        (Some(logical(against)), false)
    } else {
        let problem = if Path::new(against).is_dir() {
            "has no .ai/src/"
        } else {
            "does not exist"
        };
        put(
            run.err,
            format!(
                "{}: --against path {problem}: {against}\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };

    let Some(parent_src) = parent_src else {
        put(
            run.out,
            format!(
                "  {} no parent .ai/src/ found for {repo_root}\n",
                style.dim("·")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    };

    let (identical, divergent) = collect(&child_src, &parent_src);
    if identical.is_empty() && divergent.is_empty() {
        put(
            run.out,
            format!(
                "  {} nothing shared with parent {parent_src}\n",
                style.green("✓")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }

    let origin_hint = if from_shared {
        format!(" {}", style.dim("(from shared.path)"))
    } else {
        String::new()
    };
    put(
        run.out,
        format!(
            "  {} {parent_src}{origin_hint}\n  {} {}  {} {}\n\n",
            style.dim("Parent:"),
            style.dim("Identical:"),
            identical.len(),
            style.dim("Divergent:"),
            divergent.len()
        )
        .as_bytes(),
    )?;

    let config = config_path(repo_root);
    let (mut deleted, mut kept, mut skipped, mut cancelled) = (0, 0, 0, false);

    for (rel, child_file) in &identical {
        if cancelled {
            break;
        }
        let is_template = run.templates.iter().any(|path| path == rel);
        if run.assume_yes {
            delete_and_prune(child_file, &child_src)?;
            if is_template && let Some(config) = &config {
                yaml_edit::list_append(Path::new(config), "template_overrides.declined", rel)?;
            }
            put(
                run.out,
                format!("  {} {rel} {}\n", style.green("−"), style.dim("(deleted)")).as_bytes(),
            )?;
            deleted += 1;
            continue;
        }
        match prompt_identical(run, rel, child_file, is_template)? {
            Choice::Delete => {
                delete_and_prune(child_file, &child_src)?;
                if is_template && let Some(config) = &config {
                    yaml_edit::list_append(Path::new(config), "template_overrides.declined", rel)?;
                    put(
                        run.out,
                        format!("    {}\n", style.green("deleted + declined.")).as_bytes(),
                    )?;
                } else {
                    put(
                        run.out,
                        format!("    {}\n", style.green("deleted.")).as_bytes(),
                    )?;
                }
                deleted += 1;
            }
            Choice::Keep => {
                put(run.out, format!("    {}\n", style.dim("kept.")).as_bytes())?;
                kept += 1;
            }
            Choice::Quit => cancelled = true,
        }
    }

    for (rel, child_file, parent_file) in &divergent {
        if cancelled {
            break;
        }
        if run.assume_yes {
            put(
                run.out,
                format!(
                    "  {} {rel} {}\n",
                    style.yellow("~"),
                    style.dim("(divergent — skipped under --yes; review interactively)")
                )
                .as_bytes(),
            )?;
            skipped += 1;
            continue;
        }
        if prompt_diverge(run, rel, child_file, parent_file)? {
            cancelled = true;
        } else {
            put(
                run.out,
                format!("    {}\n", style.dim("skipped.")).as_bytes(),
            )?;
            skipped += 1;
        }
    }

    put(run.out, b"\n")?;
    if cancelled {
        put(
            run.out,
            format!(
                "  {} Decisions already made are kept.\n",
                style.yellow("Cancelled.")
            )
            .as_bytes(),
        )?;
    }
    put(
        run.out,
        format!(
            "  {} Deleted: {deleted} · Kept: {kept} · Skipped: {skipped}\n",
            style.green("Done.")
        )
        .as_bytes(),
    )?;
    Ok(0)
}

type Identical = Vec<(String, String)>;
type Divergent = Vec<(String, String, String)>;

/// `_dedupe_collect`: the flat categories' `*.md` in byte order, then every
/// file below `skills` not named `.*`, in byte order, links not followed.
fn collect(child_src: &str, parent_src: &str) -> (Identical, Divergent) {
    let mut parent_files = Vec::new();
    for category in ["rules", "commands", "agents"] {
        let dir = format!("{parent_src}/{category}");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".md") && !name.starts_with('.'))
            .collect();
        names.sort();
        parent_files.extend(
            names
                .into_iter()
                .filter(|name| Path::new(&format!("{dir}/{name}")).is_file())
                .map(|name| format!("{category}/{name}")),
        );
    }
    let mut skills = Vec::new();
    walk_files(&format!("{parent_src}/skills"), &mut skills);
    skills.sort();
    parent_files.extend(skills.into_iter().filter_map(|path| {
        path.strip_prefix(&format!("{parent_src}/"))
            .map(str::to_string)
    }));

    let (mut identical, mut divergent) = (Vec::new(), Vec::new());
    for rel in parent_files {
        let child_file = format!("{child_src}/{rel}");
        let parent_file = format!("{parent_src}/{rel}");
        if !Path::new(&child_file).is_file() {
            continue;
        }
        let (Some(child_hash), Some(parent_hash)) = (
            template_manifest::hash(Path::new(&child_file)),
            template_manifest::hash(Path::new(&parent_file)),
        ) else {
            continue;
        };
        if child_hash == parent_hash {
            identical.push((rel, child_file));
        } else {
            divergent.push((rel, child_file, parent_file));
        }
    }
    (identical, divergent)
}

/// `find <dir> -type f ! -name '.*'`.
fn walk_files(dir: &str, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{dir}/{name}");
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            walk_files(&path, found);
        } else if meta.is_file() && !name.starts_with('.') {
            found.push(path);
        }
    }
}

/// `_dedupe_delete_and_prune`: `rm -f`, then `rmdir` up to `.ai/src`.
fn delete_and_prune(file: &str, stop_at: &str) -> Result<(), Error> {
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(Error::io(file, e)),
        _ => {}
    }
    let mut dir = paths::parent(file);
    while dir != stop_at && dir != "/" {
        if std::fs::remove_dir(&dir).is_err() {
            break;
        }
        dir = paths::parent(&dir);
    }
    Ok(())
}

enum Choice {
    Delete,
    Keep,
    Quit,
}

/// `read -r reply </dev/tty`, lowercased, with the prompt's default.
fn reply(run: &mut Run, default: &str) -> String {
    let reply = run
        .terminal
        .as_mut()
        .map(|answer| answer())
        .unwrap_or_default()
        .trim_matches([' ', '\t'])
        .to_lowercase();
    if reply.is_empty() {
        default.to_string()
    } else {
        reply
    }
}

/// `_dedupe_prompt_identical`.
fn prompt_identical(
    run: &mut Run,
    rel: &str,
    child_file: &str,
    is_template: bool,
) -> Result<Choice, Error> {
    let style = run.style;
    let hint = if is_template {
        style.dim("(template-derived — declined entry will be added)")
    } else {
        String::new()
    };
    loop {
        put(
            run.err,
            format!(
                "\n  {} {}  {hint}\n    [{}]elete  [{}]eep  [v]iew  [q]uit  > ",
                style.yellow("= IDENTICAL:"),
                style.cyan(rel),
                style.green("d"),
                style.yellow("k")
            )
            .as_bytes(),
        )?;
        match reply(run, "k").as_str() {
            "d" | "delete" => return Ok(Choice::Delete),
            "k" | "keep" => return Ok(Choice::Keep),
            "v" | "view" => show_head(run, child_file)?,
            "q" | "quit" => return Ok(Choice::Quit),
            _ => put(
                run.err,
                format!("    {}\n", style.dim("(unknown choice — try d, k, v, q)")).as_bytes(),
            )?,
        }
    }
}

/// `_dedupe_prompt_diverge`: whether the answer was to quit.
fn prompt_diverge(
    run: &mut Run,
    rel: &str,
    child_file: &str,
    parent_file: &str,
) -> Result<bool, Error> {
    let style = run.style;
    loop {
        put(
            run.err,
            format!(
                "\n  {} {}\n    [{}]iew  [{}]kip  [q]uit  > ",
                style.yellow("~ DIVERGES:"),
                style.cyan(rel),
                style.yellow("v"),
                style.yellow("s")
            )
            .as_bytes(),
        )?;
        match reply(run, "s").as_str() {
            "v" | "view" => show_diff(run, child_file, parent_file)?,
            "s" | "skip" => return Ok(false),
            "q" | "quit" => return Ok(true),
            _ => put(
                run.err,
                format!("    {}\n", style.dim("(unknown choice — try v, s, q)")).as_bytes(),
            )?,
        }
    }
}

/// `_dedupe_show_head`: the first 20 lines, each indented.
fn show_head(run: &mut Run, file: &str) -> Result<(), Error> {
    let style = run.style;
    let mut text = format!(
        "\n  {}\n\n",
        style.dim("──── content (first 20 lines) ────")
    );
    let content = std::fs::read(file)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    for (index, line) in content.split_inclusive('\n').enumerate() {
        if index == 20 {
            text.push_str(&format!("  {}\n", style.dim("  …")));
            break;
        }
        text.push_str(&format!("  {}\n", line.strip_suffix('\n').unwrap_or(line)));
    }
    text.push('\n');
    put(run.err, text.as_bytes())
}

/// `_dedupe_show_diff`: `diff -u --label yours --label parent`, on stderr.
fn show_diff(run: &mut Run, child_file: &str, parent_file: &str) -> Result<(), Error> {
    let style = run.style;
    put(
        run.err,
        format!("\n  {}\n\n", style.dim("──── diff: yours → parent ────")).as_bytes(),
    )?;
    match Command::new("diff")
        .args([
            "-u",
            "--label",
            "yours",
            "--label",
            "parent",
            child_file,
            parent_file,
        ])
        .output()
    {
        Ok(output) => {
            put(run.err, &output.stdout)?;
            put(run.err, &output.stderr)?;
        }
        Err(_) => put(
            run.err,
            format!("  {}\n", style.red("(diff command not available)")).as_bytes(),
        )?,
    }
    put(run.err, b"\n")
}

/// `_dedupe_usage`.
fn usage(style: &Style) -> String {
    format!(
        "\n  {} — remove source files that duplicate a parent .ai/src/

  {}
    agentsync dedupe [options]

  {}
    --against <path>   Compare against an explicit .ai/src/ (or a project
                       root containing one). Default: nearest parent
                       .ai/src/ walking up from cwd, bounded by the git
                       repository boundary.
    --workspace        Run dedupe in every .ai/ below cwd, bottom-up
                       alphabetical; each child is deduped against its
                       own nearest parent.
    -y, --yes          Non-interactive: delete every identical-hash file
                       (and add to template_overrides.declined when the
                       file is template-derived). Divergent files are
                       always left alone — pass --yes does not auto-pick
                       a side. Required when stdin is not a TTY.
    -h, --help         Show this help.

  {}
    For each path that exists in both your .ai/src/ and the parent:
      * identical hash, template-derived → offer delete + declined entry
      * identical hash, manual file      → offer delete only
      * different hash                   → show diff, skip by default

    Identical-hash deletions also prune empty parent directories so empty
    skill folders don't linger after their SKILL.md is removed.

  {}
    agentsync dedupe
    agentsync dedupe --against ../
    agentsync dedupe --workspace
    agentsync dedupe --yes
",
        style.bold("agentsync dedupe"),
        style.green("USAGE"),
        style.green("OPTIONS"),
        style.green("BEHAVIOR"),
        style.green("EXAMPLES")
    )
}

```

In `src/main.rs`, before the `upgrade-config` block:

```rust
    if args.first().and_then(|a| a.to_str()) == Some("dedupe") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        let cwd = paths::logical_root(None, &cwd, var("PWD").as_deref());
        return cli::dedupe::dedupe(
            &rest,
            &cwd,
            &project_root,
            &Style::for_stdout(),
            prompts::is_tty().then_some(&mut prompts::read_terminal as &mut dyn FnMut() -> String),
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
```

In `bin/agentsync.sh:280` append `dedupe`.

- [ ] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in dedupe doctor; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
bats --tap -f 'dedupe deletes' tests/native_parity.bats
```

Expected: `217 passed`, `0`, `11`, `1`; `0` for each file; `ok`.

- [ ] **Step 5: Prove the fixture bites, check the terminal, lint, commit**

Change `(from shared.path)` to `(from shared)` in `src/cli/dedupe.rs`, rebuild, rerun the fixture: `not ok` with the `Parent:` line in the diff; revert and rebuild.

Run `scratchpad/phase4/dedupe_tty_reference.sh` against Bash and the binary on a pty (answers ` V `, `x`, `D`, empty, `v`, `s`, `q`) and diff the transcripts after the dispatcher's format notice: identical.

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/dedupe.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-15-rust-migration-phase-4e-dedupe.md
git commit -m "feat(native): port dedupe"
```

---

### Task 4: Verify, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [ ] **Step 1: Spec**

In the Phase 4 section, after the `template_manifest` family bullet, add: "Planned in five slices: 4e `dedupe` with the template hash, the template set, and the parent walk; 4f `adopt`; 4g `migrate`; 4h `refresh`; 4i `init`."

Append to "Known quirks":

```markdown
31. `dedupe` removes a category directory it emptied, such as `.ai/src/rules/`,
    not only emptied skill folders.
32. `dedupe` ignores `AGENTSYNC_CONFIG_PATH`, even a missing one: it reads
    `shared.path` from and appends declined entries to `.ai/agent_sync.yaml`,
    else a root `agent_sync.yaml`.
```

Append to "Accepted deviations":

```markdown
- Phase 4e: `dedupe` takes the shipped template set from the embedded
  templates where Bash walked `$AGENTSYNC_HOME/lib/templates`.
- Phase 4e: an `AGENTSYNC_REPO_ROOT` that does not exist reports
  `Error: Repository root not found: <path>` where Bash printed `cd`'s message;
  the status is 1 in both, as for every ported command since Phase 4a.
```

- [ ] **Step 2: Module map and outputs**

Set the `lib/helpers/template_manifest.sh` row to `→ src/template_manifest.rs   hash (Phase 4e); load, record, write, heal wait for refresh and init`, the `lib/helpers/dedupe.sh` row to `→ src/cli/dedupe.rs       Phase 4e, ported`, and add `find_parent_ai_src` after `find_workspace_ai_dirs` in the `lib/helpers/paths.sh` row. Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [ ] **Step 3: Verify (outside the agent sandbox)**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in dedupe doctor native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `217 passed`, `0`, `11`, `1`; lint exit 0; every line `bash=0 native=0`.

- [ ] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-15-rust-migration-phase-4e-dedupe.md
git commit -m "docs(native): map the phase 4e modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, `dedupe.bats` and `doctor.bats` are green under both engines, the parity fixture passes, and a `## Completion receipt` records the fresh verification. The next plan is 4f, `adopt`.

## Run log

### 2026-09-15 — Phase 4e planned
- Commits: this plan.
- Verified: the Rust in Tasks 2–3 was drafted and run before this plan was written, then removed from the tree. `cargo test` 217/0/11/1, clippy clean; `scratchpad/phase4/dedupe_compare.sh` (every non-interactive branch) and `dedupe_tty_compare.sh` (the prompts on a pty) gave identical transcripts and trees for Bash and the binary, and a `Done.` mutation showed in the diff; with `dedupe` added to `_NATIVE_COMMANDS` for the run, `dedupe.bats` 11/11 and `doctor.bats` 36/36 passed in both modes and the parity fixture passed and failed on a `(from shared.path)` mutation. Task 1's fix was tried on a copy of the engine: the new test failed on the current `dedupe.sh` and passed with the fix, 12/12, ShellCheck clean.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.
