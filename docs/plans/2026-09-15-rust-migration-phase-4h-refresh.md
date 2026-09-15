# Rust Migration Phase 4h: Native `refresh`

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Port `agentsync refresh` so the binary answers it byte for byte like `cmd_refresh` in `lib/helpers/refresh.sh`: the three-way classification against `.ai/.template-manifest`, the `--yes`, `--dry-run`, `--status`, `--only`, `--include-deleted`, `--include-agents-md`, and `--review` flags, the manifest heal, and the three `/dev/tty` prompts with their view and diff.

**Architecture:** `catalog::template_files` returns the shipped template set as `(path below .ai/src/, bytes)`, the same walk `dedupe` and `template_manifest_heal_from_match` make; `template_sources` becomes its projection. `template_manifest::TemplateManifest` gains `record`, `is_empty`, and `heal_from_match`, completing the module after Phase 4e's hash and 4g's load, lookup, remove, and write. `src/cli/refresh.rs` holds the argument parser, the scope resolver, the classifier, the apply loops, the prompts, and the usage text; `main` hands it the unchecked root (`AGENTSYNC_REPO_ROOT`, else the logical working directory) and an `Env` carrying `prompts::is_tty` and `prompts::read_terminal`. The seam stays the CLI process boundary: `tests/refresh.bats` under `AGENTSYNC_NATIVE=1` plus two tree-parity fixtures.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 4". Previous plan: `docs/plans/2026-09-15-rust-migration-phase-4g-migrate.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `refresh` writes only the templates it adds, restores, auto-updates, or updates under the detected source base and `.ai/.template-manifest`; `--dry-run` and `--status` write nothing; off a terminal a conflict is never overwritten.
- No binary ships to users; without a binary every command runs in Bash. No Bash change in this plan: the reference turned up no bug; `lib/**/*.sh` and `bin/agentsync.sh` stay clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state.
- Disk-touching unit tests are `#[cfg(unix)]`.
- No bats fixture reaches a prompt: the prompts read the terminal device even off a terminal, so a fixture without `--yes` or `--dry-run` needs a new file or a conflict pending, which the TTY gate refuses first. The prompts are proven on a pseudo-terminal in Task 2 Step 5.
- Every expected value was captured from Bash on 2026-09-15: `phase4h/refresh_reference.sh` (59 non-interactive scenarios, a 3658-line transcript with trees, hashes, and modes) and `phase4h/refresh_tty.sh` (7 scenarios on a pty through `script`, 600 lines). Both scripts are reproduced in Task 2 Step 5.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-15 under the maintainer's `/decide`: all three as recommended. Before the call, quirks 39 and 40 were confirmed against Bash off a terminal: `refresh --include-deleted` with only a deleted file pending printed the RESTORE prompt, `refresh.sh: line 687: /dev/tty: Device not configured`, and `still declined.` at exit 0, and a config named by `AGENTSYNC_CONFIG_PATH` with declined templates was ignored by `--status` and `--dry-run`. The manifest-mode fix in Bash stays available if a user reports the `0600` flip on macOS; it was not taken because git tracks only the executable bit, the divergence has stood since 4g, and this plan carries no Bash change.

1. **The mode of a template `refresh` creates.** Bash copies the checkout's file with `cp`, so a re-added `skills/humanizer/scripts/strip-ai-chars.sh` is `0755`; the embedded templates carry bytes only. **Recommended:** a file `refresh` creates is opened with mode `0755` when the template starts with `#!`, the mode every shipped script carries, and with the default `0666` under the umask otherwise; an existing file keeps its mode, as `cp` leaves it. Recorded as an accepted deviation. Alternatives: a `skills/*/scripts/` directory rule (the same result today, blind to a script elsewhere), or plain `0644` (a restored script the user cannot run).
2. **The mode of `.ai/.template-manifest` after a write.** Bash's `template_manifest_write` sorts its staging file with `sort -u -o "$tmp" "$tmp"`; BSD `sort` rewrites through a new `0600` inode (checked on this machine: inode and mode change), so a `0644` manifest from a git checkout comes back `0600` on macOS. Phase 4g's `TemplateManifest::write` keeps an existing file's mode through `staging::write_beside`. **Recommended:** record the difference as an accepted deviation, dated to 4g's writer, since git tracks only the executable bit and the Rust behaviour is platform-independent. Alternative: a `fix(template-manifest)` commit that sorts into a second staging file and copies it back over the first, with a mode assertion in `tests/refresh.bats` that needs a `stat` flag per platform.
3. **Quirks and deviations.** Record as known quirks 38–40: the manifest heal covers every shipped template that matches, including categories outside `--only` and `AGENTS.md` without `--include-agents-md`; off a terminal without `--yes`, `refresh` applies pending auto-updates because the TTY gate looks only at new files and conflicts, and with `--include-deleted` and nothing else pending it prints each RESTORE prompt on stderr and declines it; `template_overrides` are read from `.ai/agent_sync.yaml`, else a root `agent_sync.yaml`, ignoring `AGENTSYNC_CONFIG_PATH`. Record as accepted deviations that `refresh` prints and compares against the embedded templates as `/<agentsync>/lib/templates` where Bash used `$AGENTSYNC_HOME/lib/templates`, and that a prompt whose terminal device cannot be opened declines silently where Bash also printed the shell's `/dev/tty` open error. **Recommended:** as listed.

## Module closure

```text
lib/helpers/refresh.sh           37-62   categories, the *_FILES globals, override lists
                                 64-356  cmd_refresh: options, header, up-to-date branch, summary, TTY gate, apply loops, closing lines
                                 360-413 _refresh_find_templates (embedded in Rust), _refresh_resolve_scope
                                 418-446 _refresh_load_overrides, _refresh_in_array
                                 451-547 _refresh_collect_changes, _refresh_classify
                                 552-594 _refresh_heal_unchanged, _refresh_print_status
                                 598-635 _refresh_list_proposed, _refresh_copy
                                 640-723 _refresh_prompt_new, _refresh_prompt_conflict, _refresh_prompt_deleted, _refresh_show_new, _refresh_show_diff
                                 727-788 _refresh_usage
lib/helpers/export.sh            17-83   _resolve_source_paths; refresh reads only _SRC_BASE (.ai/src, else .ai)
lib/helpers/template_manifest.sh 78-91   template_manifest_record
                                 146-191 template_manifest_heal_from_match
lib/helpers/prompts.sh           10-12   is_tty
lib/helpers/resolve.sh           11-28   resolve_system_dir; the binary reads the embedded templates
bin/agentsync.sh                 280     _NATIVE_COMMANDS; 382 refresh's _need list
```

Reused: `TemplateManifest::{load, lookup, write}`, `template_manifest::hash`, `manifest::sha256_hex`, `catalog::engine_files`, `yaml_subset::list`, `cli::customize::put`, `paths::logical_root`, `prompts::{is_tty, read_terminal}`, `style`. The Bash globs over `rules/*.md`, `commands/*.md`, and `agents/*.md` follow the locale; the shipped names are lowercase ASCII, so byte order gives the same listing (Phase 2 deviation).

---

### Task 0: Baseline

**Files:** none changed.

- [ ] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in refresh native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: this plan's commit; `232 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for both files.

---

### Task 1: Port the template manifest record and heal

**Files:**
- Modify: `src/catalog.rs`, `src/template_manifest.rs`

**Interfaces:**
- Produces:
  - `pub fn catalog::template_files() -> Vec<(String, &'static [u8])>`; `catalog::template_sources()` keeps its signature and becomes the paths of `template_files`
  - `TemplateManifest::record(&mut self, rel: &str, hash: &str)`, `TemplateManifest::is_empty(&self) -> bool`, `TemplateManifest::heal_from_match<'a>(&mut self, templates: impl IntoIterator<Item = (&'a str, &'a [u8])>, user_base: &Path)`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module of `src/catalog.rs`, before `the_template_sources_are_the_set_dedupe_loads`:

```rust
    #[test]
    fn template_files_carry_the_bytes_refresh_hashes_and_copies() {
        let files = template_files();
        assert_eq!(
            files
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>(),
            template_sources()
        );
        let script = files
            .iter()
            .find(|(path, _)| path == "skills/humanizer/scripts/strip-ai-chars.sh")
            .map(|(_, bytes)| *bytes)
            .expect("shipped");
        assert!(script.starts_with(b"#!"));
        assert!(
            files
                .iter()
                .all(|(path, bytes)| path == "AGENTS.md" || !bytes.is_empty())
        );
    }
```

Append to the `tests` module of `src/template_manifest.rs`:

```rust
    #[test]
    fn record_updates_the_first_entry_or_appends_and_ignores_blanks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(dir.path().join(REL), "a.md\t1\na.md\t2\n").unwrap();
        let mut manifest = TemplateManifest::load(dir.path()).unwrap();
        assert!(!manifest.is_empty());
        manifest.record("a.md", "3");
        manifest.record("b.md", "4");
        manifest.record("", "5");
        manifest.record("c.md", "");
        assert_eq!(manifest.lookup("a.md"), Some("3"));
        assert_eq!(manifest.lookup("c.md"), None);
        manifest.write(dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(REL)).unwrap(),
            "a.md\t2\na.md\t3\nb.md\t4\n"
        );
        assert!(TemplateManifest::default().is_empty());
    }

    #[test]
    fn heal_records_only_the_copies_that_match_their_template() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join(".ai/src");
        std::fs::create_dir_all(base.join("rules")).unwrap();
        std::fs::create_dir_all(base.join("skills/a")).unwrap();
        std::fs::write(base.join("AGENTS.md"), "agents\n").unwrap();
        std::fs::write(base.join("rules/same.md"), "same\n").unwrap();
        std::fs::write(base.join("rules/edited.md"), "mine\n").unwrap();
        std::fs::write(base.join("skills/a/SKILL.md"), "skill\n").unwrap();
        let templates: [(&str, &[u8]); 5] = [
            ("AGENTS.md", b"agents\n"),
            ("rules/same.md", b"same\n"),
            ("rules/edited.md", b"theirs\n"),
            ("rules/missing.md", b"new\n"),
            ("skills/a/SKILL.md", b"skill\n"),
        ];
        let mut manifest = TemplateManifest::default();
        manifest.record("rules/edited.md", "old");
        manifest.heal_from_match(templates, &base);
        manifest.write(dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(REL)).unwrap(),
            format!(
                "AGENTS.md\t{}\nrules/edited.md\told\nrules/same.md\t{}\nskills/a/SKILL.md\t{}\n",
                sha256_hex(b"agents\n"),
                sha256_hex(b"same\n"),
                sha256_hex(b"skill\n")
            )
        );
    }
```

Run: `cargo test --lib 2>&1 | grep -E '^error\[E0(425|599)\]' | sort -u`
Expected: errors naming the missing `template_files`, `record`, `is_empty`, and `heal_from_match`.

- [ ] **Step 2: Write the implementation**

In `src/catalog.rs`, replace `template_sources` and its doc comment with:

```rust
/// The template set `refresh` and `dedupe` walk: the shipped `AGENTS.md`, the
/// `*.md` files of `rules`, `commands`, and `agents`, and every file below
/// `skills` whose name does not start with `.`, as `(path below .ai/src/,
/// bytes)` in byte order.
pub fn template_files() -> Vec<(String, &'static [u8])> {
    let mut files: Vec<(String, &'static [u8])> = engine_files()
        .into_iter()
        .filter_map(|(path, bytes)| {
            let rel = path.strip_prefix("lib/templates/")?;
            let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
            let shipped = match dir {
                "" => rel == "AGENTS.md",
                "rules" | "commands" | "agents" => name.ends_with(".md") && !name.starts_with('.'),
                _ => (dir == "skills" || dir.starts_with("skills/")) && !name.starts_with('.'),
            };
            shipped.then(|| (rel.to_string(), bytes))
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// `_dedupe_load_template_set`: the paths of `template_files`.
pub fn template_sources() -> Vec<String> {
    template_files().into_iter().map(|(path, _)| path).collect()
}
```

In `src/template_manifest.rs`, replace `remove` and its doc comment with:

```rust
    /// `template_manifest_record`: the first entry for `rel` takes `hash`, or
    /// one is appended; an empty path or hash is ignored.
    pub fn record(&mut self, rel: &str, hash: &str) {
        if rel.is_empty() || hash.is_empty() {
            return;
        }
        match self.entries.iter_mut().find(|(key, _)| key == rel) {
            Some((_, recorded)) => *recorded = hash.to_string(),
            None => self.entries.push((rel.to_string(), hash.to_string())),
        }
    }

    /// `template_manifest_remove`: every entry for `rel`.
    pub fn remove(&mut self, rel: &str) {
        self.entries.retain(|(key, _)| key != rel);
    }

    /// `${#TEMPLATE_MANIFEST_KEYS[@]} -eq 0`.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `template_manifest_heal_from_match`: records the shipped hash of every
    /// template whose copy under `user_base` matches it byte for byte, whatever
    /// the scope of the run.
    pub fn heal_from_match<'a>(
        &mut self,
        templates: impl IntoIterator<Item = (&'a str, &'a [u8])>,
        user_base: &Path,
    ) {
        for (rel, bytes) in templates {
            let Some(current) = hash(&user_base.join(rel)) else {
                continue;
            };
            let shipped = sha256_hex(bytes);
            if current == shipped {
                self.record(rel, &shipped);
            }
        }
    }
```

- [ ] **Step 3: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
```

Expected: `235 passed`, `0`, `11`, `1`; clippy exit 0.

- [ ] **Step 4: Commit**

```bash
git add src/catalog.rs src/template_manifest.rs docs/plans/2026-09-15-rust-migration-phase-4h-refresh.md
git commit -m "feat(native): port the template manifest record and heal"
```

---

### Task 2: Port `refresh`

**Files:**
- Create: `src/cli/refresh.rs`
- Modify: `src/cli/mod.rs` (`pub mod refresh;` after `pub mod profile;`), `src/main.rs`, `bin/agentsync.sh:280`, `tests/native_parity.bats`

**Interfaces:**
- Consumes: Task 1's `catalog::template_files` and `TemplateManifest::{record, is_empty, heal_from_match}`.
- Produces:
  - `pub struct refresh::Env<'a> { pub interactive: bool, pub read_line: &'a mut dyn FnMut() -> String }`
  - `pub fn refresh::refresh(args: &[String], root: &str, style: &Style, env: &mut Env, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>`

- [ ] **Step 1: Parity fixtures, Bash side**

Append to `tests/native_parity.bats`:

```bash
# ── refresh ──────────────────────────────────────────────────────────────────
# Every call runs off a terminal, so no fixture may reach a prompt: a call
# without --yes or --dry-run needs a new file or a conflict pending, which the
# TTY gate refuses first.

# Forgets one template in the manifest, as one the project has never seen.
_forget_template() {
    grep -v "^$1"$'\t' .ai/.template-manifest > "$BATS_TEST_TMPDIR/manifest" || true
    cp "$BATS_TEST_TMPDIR/manifest" .ai/.template-manifest
}

@test "parity: refresh plans, adds, auto-updates, and skips conflicts like Bash" {
    assert_tree_parity refresh --help
    assert_tree_parity refresh --bogus --help
    assert_tree_parity refresh extra
    assert_tree_parity refresh --yes --only
    assert_tree_parity refresh --yes --only bogus
    assert_tree_parity refresh --yes --only ,
    assert_tree_parity refresh --yes
    assert_tree_parity refresh --status
    rm -f .ai/src/rules/comments.md .ai/src/commands/review.md
    _forget_template rules/comments.md
    printf 'USER LOCAL EDIT\n' >> .ai/src/rules/core.md
    _forget_template rules/core.md
    printf 'rules/core.md\t%s\n' "$(file_sha256 .ai/src/rules/core.md)" >> .ai/.template-manifest
    printf 'EDIT\n' >> .ai/src/rules/git.md
    _forget_template rules/git.md
    printf 'EDIT\n' >> .ai/src/AGENTS.md
    assert_tree_parity refresh --dry-run --include-deleted --include-agents-md
    assert_tree_parity refresh
    assert_tree_parity refresh --include-deleted
    assert_tree_parity refresh --status
    assert_tree_parity refresh --yes --include-deleted --review
    assert_tree_parity refresh --yes
    printf '\ntemplate_overrides:\n  declined:\n    - rules/git.md\n    - commands/review.md\n  pinned:\n    - AGENTS.md\n' >> .ai/agent_sync.yaml
    assert_tree_parity refresh --yes --include-agents-md
    assert_tree_parity refresh --status
}

@test "parity: refresh scopes, heals the manifest, and restores a script like Bash" {
    rm -rf .ai/src/skills/humanizer/scripts .ai/src/skills/comments .ai/src/agents
    rm -f .ai/.template-manifest
    assert_tree_parity refresh --yes --only rules
    assert_tree_parity refresh --yes "--only= skills , subagents ,skills"
    [ -x "$BATS_TEST_TMPDIR/bash/.ai/src/skills/humanizer/scripts/strip-ai-chars.sh" ]
    [ -x "$BATS_TEST_TMPDIR/native/.ai/src/skills/humanizer/scripts/strip-ai-chars.sh" ]
    printf 'USER LOCAL EDIT\n' >> .ai/src/rules/core.md
    assert_tree_parity refresh --review --dry-run
    assert_tree_parity refresh --yes
    mkdir -p legacy
    mv .ai/src/* legacy/
    rmdir .ai/src
    mv legacy/* .ai/
    rmdir legacy
    assert_tree_parity refresh --yes
    assert_tree_parity refresh --status
    rm -rf .ai/rules .ai/skills .ai/commands
    assert_tree_parity refresh --yes
    assert_tree_parity refresh --yes --only commands
    rm -rf .ai
    assert_tree_parity refresh --yes
}
```

Run: `bats --tap -f 'parity: refresh' tests/native_parity.bats`
Expected: `ok 1` and `ok 2` (the native side still runs Bash).

- [ ] **Step 2: Write the failing tests**

Create `src/cli/refresh.rs` with its tests module only, and add `pub mod refresh;` to `src/cli/mod.rs` after `pub mod profile;`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::template_manifest::REL;

    /// A project `init` scaffolded: every template under `.ai/src/` and a
    /// manifest recording each hash.
    fn seeded() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let base = Path::new(&root).join(".ai/src");
        let mut manifest = TemplateManifest::default();
        for (rel, bytes) in catalog::template_files() {
            let path = base.join(&rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            manifest.record(&rel, &sha256_hex(bytes));
        }
        std::fs::write(
            Path::new(&root).join(".ai/agent_sync.yaml"),
            "tools:\n  enabled: []\n",
        )
        .unwrap();
        manifest.write(Path::new(&root)).unwrap();
        (dir, root)
    }

    struct Outcome {
        status: u8,
        out: String,
        err: String,
    }

    fn call(root: &str, args: &[&str], interactive: bool, replies: &[&str]) -> Outcome {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut queue: VecDeque<String> = replies.iter().map(|r| r.to_string()).collect();
        let mut read_line = || queue.pop_front().unwrap_or_default();
        let mut env = Env {
            interactive,
            read_line: &mut read_line,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = refresh(&args, root, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
        Outcome {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    fn manifest_text(root: &str) -> String {
        std::fs::read_to_string(Path::new(root).join(REL)).unwrap_or_default()
    }

    fn drop_entry(root: &str, rel: &str) {
        let kept: String = manifest_text(root)
            .lines()
            .filter(|line| !line.starts_with(&format!("{rel}\t")))
            .map(|line| format!("{line}\n"))
            .collect();
        std::fs::write(Path::new(root).join(REL), kept).unwrap();
    }

    fn set_entry(root: &str, rel: &str, hash: &str) {
        drop_entry(root, rel);
        let mut lines: Vec<String> = manifest_text(root).lines().map(str::to_string).collect();
        lines.push(format!("{rel}\t{hash}"));
        lines.sort();
        std::fs::write(
            Path::new(&root).join(REL),
            lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
        )
        .unwrap();
    }

    fn append(root: &str, rel: &str, text: &str) {
        let path = Path::new(root).join(".ai/src").join(rel);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    fn header(root: &str, scope: &str) -> String {
        format!(
            "\n  AgentSync Refresh\n\n  Templates: /<agentsync>/lib/templates\n  Project:   {root}/.ai/src\n  Scope:     {scope}\n\n"
        )
    }

    const NOT_A_TTY: &str = "  Error: Cannot run interactively (not a TTY).\n  Use --yes to add new files and apply auto-updates\n  (conflicts are always skipped non-interactively).\n  Use --dry-run to preview.\n";

    #[test]
    fn arguments_are_refused_and_help_prints_like_bash() {
        let (_dir, root) = seeded();
        let help = call(&root, &["--help", "--bogus"], false, &[]);
        assert_eq!(help.status, 0);
        assert!(help.out.starts_with(
            "\n  agentsync refresh — pull new template files into an existing .ai/src/\n\n  USAGE\n    agentsync refresh [options]\n\n  DESCRIPTION\n"
        ));
        assert!(
            help.out
                .contains("\n  REMEMBERED SKIPS\n    Picking [s]kip on a conflict")
        );
        assert!(help.out.contains("\n  PERSISTENT OVERRIDES\n"));
        assert!(help.out.ends_with(
            "    agentsync refresh --review            # revisit conflicts you skipped\n"
        ));
        assert_eq!(help.out.lines().count(), 58);

        let bogus = call(&root, &["--bogus", "--help"], false, &[]);
        assert_eq!((bogus.status, bogus.out.as_str()), (1, ""));
        assert_eq!(
            bogus.err,
            format!("Error: Unknown option: --bogus\n{}", help.out)
        );
        let extra = call(&root, &["extra"], false, &[]);
        assert_eq!(
            extra.err,
            format!("Error: Unexpected argument: extra\n{}", help.out)
        );
        let missing = call(&root, &["--yes", "--only"], false, &[]);
        assert_eq!(
            (missing.status, missing.err.as_str()),
            (1, "Error: --only requires a value\n")
        );
        assert_eq!(
            call(&root, &["--yes", "--only", "bogus"], false, &[]).err,
            "Error: Unknown --only value: bogus\nValid: rules, skills, commands, agents (or subagents)\n"
        );
        assert_eq!(
            call(&root, &["--yes", "--only", ","], false, &[]).err,
            "Error: --only must include at least one category\n"
        );
        assert_eq!(manifest_text(&root).lines().count(), 19);
    }

    #[test]
    fn an_untouched_project_is_up_to_date_and_status_reports_nothing_declined() {
        let (_dir, root) = seeded();
        let before = manifest_text(&root);
        let run = call(&root, &["--yes"], false, &[]);
        assert_eq!(run.status, 0);
        assert_eq!(
            run.out,
            format!(
                "{}  Already up to date! 18 file(s) match the current templates.\n\n",
                header(&root, "rules,skills,commands,agents")
            )
        );
        assert_eq!(run.err, "");
        assert_eq!(manifest_text(&root), before);
        assert_eq!(
            call(&root, &["--status"], false, &[]).out,
            "\n  Declined templates\n  Nothing declined.\n\n"
        );

        std::fs::rename(
            Path::new(&root).join(".ai/src"),
            Path::new(&root).join("legacy"),
        )
        .unwrap();
        for entry in std::fs::read_dir(Path::new(&root).join("legacy")).unwrap() {
            let entry = entry.unwrap();
            std::fs::rename(
                entry.path(),
                Path::new(&root).join(".ai").join(entry.file_name()),
            )
            .unwrap();
        }
        let legacy = call(&root, &["--yes"], false, &[]);
        assert!(legacy.out.contains(&format!("  Project:   {root}/.ai\n")));
        assert!(legacy.out.contains("Already up to date! 18 file(s)"));

        std::fs::remove_dir_all(Path::new(&root).join(".ai")).unwrap();
        let gone = call(&root, &["--status"], false, &[]);
        assert_eq!(
            (gone.status, gone.out.as_str(), gone.err),
            (
                1,
                "",
                format!("Error: No .ai/ directory found in {root}\nRun agentsync init first.\n")
            )
        );
    }

    #[test]
    fn new_deleted_auto_update_and_conflict_files_classify_and_apply_like_bash() {
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_file(base.join("rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        append(&root, "rules/core.md", "USER LOCAL EDIT\n");
        set_entry(
            &root,
            "rules/core.md",
            &template_manifest::hash(&base.join("rules/core.md")).unwrap(),
        );
        append(&root, "rules/git.md", "EDIT\n");
        drop_entry(&root, "rules/git.md");
        std::fs::remove_file(base.join("commands/review.md")).unwrap();

        let plan = "  Summary:\n    + 1 new template(s)\n    ↑ 1 auto-update(s) — you hadn't touched them locally\n    ~ 1 conflict(s) — your version differs from the template\n    ? 1 previously declined — pass --include-deleted to revisit\n    · 14 unchanged\n\n  New:\n    + rules/comments.md\n\n  Auto-update: (your version matches the previous template; safe to update)\n    ↑ rules/core.md\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ rules/git.md\n\n  Previously declined:\n    ? commands/review.md\n\n";
        let head = header(&root, "rules,skills,commands,agents");

        let dry = call(&root, &["--dry-run", "--include-deleted"], false, &[]);
        assert_eq!(
            (dry.status, dry.out),
            (0, format!("{head}{plan}  Dry run — no files written.\n\n"))
        );
        assert!(!base.join("rules/comments.md").exists());

        let blocked = call(&root, &["--include-deleted"], false, &[]);
        assert_eq!(
            (blocked.status, blocked.out, blocked.err.as_str()),
            (1, format!("{head}{plan}"), NOT_A_TTY)
        );

        assert_eq!(
            call(&root, &["--status"], false, &[]).out,
            "\n  Declined templates\n  Local       (.template-manifest — deleted from disk; --include-deleted to restore):\n    · commands/review.md\n\n"
        );

        let applied = call(
            &root,
            &["--yes", "--include-deleted", "--review"],
            false,
            &[],
        );
        assert_eq!(
            (applied.status, applied.out),
            (
                0,
                format!(
                    "{head}{plan}  ↑ rules/core.md  (auto-updated; you hadn't touched it)\n  ? commands/review.md (previously declined — skipped under --yes; run interactively)\n  + rules/comments.md\n  ~ rules/git.md (conflict — skipped; run interactively to review)\n\n  Done. Added: 1 · Auto-updated: 1 · Updated: 0 · Skipped: 2 · Unchanged: 14\n\n  Next: agentsync sync to distribute the updates to enabled tools.\n\n"
                )
            )
        );
        let template = |rel: &str| {
            catalog::template_files()
                .into_iter()
                .find(|(path, _)| path == rel)
                .map(|(_, bytes)| bytes.to_vec())
                .unwrap()
        };
        assert_eq!(
            std::fs::read(base.join("rules/comments.md")).unwrap(),
            template("rules/comments.md")
        );
        assert_eq!(
            std::fs::read(base.join("rules/core.md")).unwrap(),
            template("rules/core.md")
        );
        assert!(
            std::fs::read_to_string(base.join("rules/git.md"))
                .unwrap()
                .ends_with("EDIT\n")
        );
        assert!(!base.join("commands/review.md").exists());
        let manifest = TemplateManifest::load(Path::new(&root)).unwrap();
        assert_eq!(
            manifest.lookup("rules/comments.md"),
            Some(sha256_hex(&template("rules/comments.md")).as_str())
        );
        assert_eq!(
            manifest.lookup("rules/core.md"),
            Some(sha256_hex(&template("rules/core.md")).as_str())
        );
        assert_eq!(manifest.lookup("rules/git.md"), None);
        assert!(manifest.lookup("commands/review.md").is_some());

        let again = call(&root, &["--yes"], false, &[]);
        assert_eq!(
            again.out,
            format!(
                "{head}  Summary:\n    ~ 1 conflict(s) — your version differs from the template\n    · 16 unchanged\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ rules/git.md\n\n  ~ rules/git.md (conflict — skipped; run interactively to review)\n\n  Done. Added: 0 · Auto-updated: 0 · Updated: 0 · Skipped: 1 · Unchanged: 16\n\n"
            )
        );
    }

    #[test]
    fn silently_kept_edits_and_deleted_files_show_in_the_up_to_date_summary() {
        let (_dir, root) = seeded();
        append(&root, "rules/core.md", "USER LOCAL EDIT\n");
        std::fs::remove_file(Path::new(&root).join(".ai/src/commands/review.md")).unwrap();
        let head = header(&root, "rules,skills,commands,agents");
        assert_eq!(
            call(&root, &["--yes"], false, &[]).out,
            format!(
                "{head}  Already up to date! 16 file(s) match the current templates.\n  Locally declined (.template-manifest):    1 file(s); --include-deleted to revisit.\n  Pass --status for the full list.\n  1 file(s) differ from the shipped template (local edits or earlier skips); pass --review to revisit.\n\n"
            )
        );
        let review = call(&root, &["--review", "--dry-run"], false, &[]);
        assert_eq!(
            review.out,
            format!(
                "{head}  Summary:\n    ~ 1 conflict(s) — your version differs from the template\n    · 16 unchanged\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ rules/core.md\n\n  Dry run — no files written.\n\n"
            )
        );
        let review = call(&root, &["--review"], false, &[]);
        assert_eq!((review.status, review.err.as_str()), (1, NOT_A_TTY));
        assert!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/rules/core.md"))
                .unwrap()
                .ends_with("USER LOCAL EDIT\n")
        );

        append(&root, "AGENTS.md", "USER LOCAL EDIT\n");
        drop_entry(&root, "AGENTS.md");
        let agents = call(&root, &["--yes", "--include-agents-md"], false, &[]);
        assert_eq!(
            agents.out,
            format!(
                "{}  Summary:\n    ~ 1 conflict(s) — your version differs from the template\n    · 1 silently kept (local edits or earlier skips) — pass --review to revisit\n    · 16 unchanged\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ AGENTS.md\n\n  ~ AGENTS.md (conflict — skipped; run interactively to review)\n\n  Done. Added: 0 · Auto-updated: 0 · Updated: 0 · Skipped: 1 · Unchanged: 16\n\n",
                header(&root, "rules,skills,commands,agents,AGENTS.md")
            )
        );
    }

    #[test]
    fn declined_and_pinned_overrides_silence_templates_and_status_lists_them() {
        let (_dir, root) = seeded();
        let config = Path::new(&root).join(".ai/agent_sync.yaml");
        std::fs::write(
            &config,
            "tools:\n  enabled: []\n\ntemplate_overrides:\n  declined:\n    - rules/comments.md\n    - rules/git.md\n  pinned:\n    - rules/core.md\n",
        )
        .unwrap();
        std::fs::remove_file(Path::new(&root).join(".ai/src/rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        append(&root, "rules/core.md", "USER LOCAL EDIT\n");
        drop_entry(&root, "rules/core.md");
        assert_eq!(
            call(&root, &["--status"], false, &[]).out,
            "\n  Declined templates\n  Persistent  (template_overrides.declined in agent_sync.yaml — never offered):\n    · rules/comments.md\n    · rules/git.md\n\n"
        );
        assert_eq!(
            call(&root, &["--yes"], false, &[]).out,
            format!(
                "{}  Already up to date! 15 file(s) match the current templates.\n  Persistently declined (agent_sync.yaml): 2 file(s).\n  Pass --status for the full list.\n\n",
                header(&root, "rules,skills,commands,agents")
            )
        );
        assert!(!Path::new(&root).join(".ai/src/rules/comments.md").exists());
        assert_eq!(
            TemplateManifest::load(Path::new(&root))
                .unwrap()
                .lookup("rules/core.md"),
            None
        );
        // A declined template that was recorded and then removed is not a local decline.
        std::fs::remove_file(Path::new(&root).join(".ai/src/rules/git.md")).unwrap();
        assert!(
            !call(&root, &["--yes"], false, &[])
                .out
                .contains("Locally declined")
        );
    }

    #[test]
    fn prompts_restore_add_update_skip_view_and_quit_like_bash() {
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_file(base.join("rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        std::fs::remove_file(base.join("commands/review.md")).unwrap();
        append(&root, "rules/git.md", "EDIT\n");
        drop_entry(&root, "rules/git.md");

        let run = call(
            &root,
            &["--include-deleted"],
            true,
            &["v", " A ", "x", "", "view", "Update"],
        );
        assert_eq!(run.status, 0);
        let review = catalog::template_files()
            .into_iter()
            .find(|(path, _)| path == "commands/review.md")
            .map(|(_, bytes)| String::from_utf8(bytes.to_vec()).unwrap())
            .unwrap();
        let shown: String = review.lines().map(|line| format!("  {line}\n")).collect();
        let restore = "\n  ? RESTORE: commands/review.md  (previously declined)\n    [a]dd  [s]kip  [v]iew  [q]uit  > ";
        let new = "\n  + NEW: rules/comments.md\n    [a]dd  [s]kip  [v]iew  [q]uit  > ";
        let conflict = "\n  ~ CONFLICT: rules/git.md\n    [u]pdate  [s]kip  [v]iew  [q]uit  > ";
        let expected_err = format!(
            "{restore}\n  ──── new file content ────\n\n{shown}\n{restore}{new}    (unknown choice — try a, s, v, q)\n{new}{conflict}\n  ──── diff: yours → template ────\n\n--- yours\n+++ template\n@@ "
        );
        assert!(
            run.err.starts_with(&expected_err),
            "stderr was:\n{}",
            run.err
        );
        assert!(run.err.contains("\n-EDIT\n"));
        assert!(run.err.ends_with(&format!("\n{conflict}")));
        assert!(run.out.ends_with(
            "    restored.\n    declined (will not be offered again — use --include-deleted to revisit).\n    updated.\n\n  Done. Added: 1 · Auto-updated: 0 · Updated: 1 · Skipped: 1 · Unchanged: 15\n\n  Next: agentsync sync to distribute the updates to enabled tools.\n\n"
        ));
        assert!(base.join("commands/review.md").is_file());
        assert!(!base.join("rules/comments.md").exists());
        let manifest = TemplateManifest::load(Path::new(&root)).unwrap();
        assert!(manifest.lookup("rules/comments.md").is_some());
        assert_eq!(
            std::fs::read_to_string(base.join("rules/git.md")).unwrap(),
            String::from_utf8(
                catalog::template_files()
                    .into_iter()
                    .find(|(path, _)| path == "rules/git.md")
                    .unwrap()
                    .1
                    .to_vec()
            )
            .unwrap()
        );

        let (_dir, root) = seeded();
        std::fs::remove_file(Path::new(&root).join(".ai/src/rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        append(&root, "rules/git.md", "EDIT\n");
        drop_entry(&root, "rules/git.md");
        let quit = call(&root, &[], true, &["skip", "q"]);
        assert_eq!(quit.status, 0);
        assert!(quit.out.ends_with(
            "    declined (will not be offered again — use --include-deleted to revisit).\n\n  Cancelled. Files already applied are kept.\n  Done. Added: 0 · Auto-updated: 0 · Updated: 0 · Skipped: 1 · Unchanged: 16\n\n"
        ));
        assert!(
            quit.err.ends_with(
                "\n  ~ CONFLICT: rules/git.md\n    [u]pdate  [s]kip  [v]iew  [q]uit  > "
            )
        );
        let manifest = TemplateManifest::load(Path::new(&root)).unwrap();
        assert!(manifest.lookup("rules/comments.md").is_some());
        assert_eq!(manifest.lookup("rules/git.md"), None);
    }

    #[test]
    fn scope_follows_only_and_present_directories_and_heals_every_category() {
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_file(base.join("rules/comments.md")).unwrap();
        std::fs::remove_dir_all(base.join("skills/comments")).unwrap();
        std::fs::remove_dir_all(base.join("agents")).unwrap();
        std::fs::remove_file(Path::new(&root).join(REL)).unwrap();

        let rules = call(&root, &["--yes", "--only", "rules"], false, &[]);
        assert_eq!(
            rules.out,
            format!(
                "{}  Manifest:  none — falling back to two-way diff\n\n  Summary:\n    + 1 new template(s)\n    · 2 unchanged\n\n  New:\n    + rules/comments.md\n\n  + rules/comments.md\n\n  Done. Added: 1 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 2\n\n  Next: agentsync sync to distribute the updates to enabled tools.\n\n",
                header(&root, "rules").strip_suffix('\n').unwrap()
            )
        );
        let healed = manifest_text(&root);
        assert_eq!(healed.lines().count(), 17);
        assert!(healed.contains("AGENTS.md\t"));
        assert!(healed.contains("skills/humanizer/SKILL.md\t"));
        assert!(!healed.contains("skills/comments/SKILL.md\t"));
        assert!(!healed.contains("agents/code-reviewer.md\t"));

        let spaced = call(
            &root,
            &["--yes", "--only= skills , subagents ,skills"],
            false,
            &[],
        );
        assert_eq!(
            spaced.out,
            format!(
                "{}  Summary:\n    + 2 new template(s)\n    · 11 unchanged\n\n  New:\n    + agents/code-reviewer.md\n    + skills/comments/SKILL.md\n\n  + agents/code-reviewer.md\n  + skills/comments/SKILL.md\n\n  Done. Added: 2 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 11\n\n  Next: agentsync sync to distribute the updates to enabled tools.\n\n",
                header(&root, "skills,agents")
            )
        );
        assert_eq!(manifest_text(&root).lines().count(), 19);

        std::fs::remove_dir_all(base.join("commands")).unwrap();
        let absent = call(&root, &["--yes"], false, &[]);
        assert!(absent.out.contains("  Scope:     rules,skills,agents\n"));
        assert!(absent.out.contains("Already up to date! 16 file(s)"));
        assert!(!base.join("commands").exists());

        for category in ["rules", "skills", "agents"] {
            std::fs::remove_dir_all(base.join(category)).unwrap();
        }
        let none = call(&root, &["--yes"], false, &[]);
        assert_eq!(
            (none.status, none.err),
            (
                1,
                format!(
                    "Error: No source content categories present in {root}/.ai/src.\nPass --only rules,skills,commands,agents to opt into specific ones,\nor run agentsync init to scaffold them.\n"
                )
            )
        );
        let recorded = call(&root, &["--yes", "--only", "commands"], false, &[]);
        assert_eq!(
            recorded.out,
            format!(
                "{}  Already up to date! 0 file(s) match the current templates.\n  Locally declined (.template-manifest):    2 file(s); --include-deleted to revisit.\n  Pass --status for the full list.\n\n",
                header(&root, "commands")
            )
        );
        drop_entry(&root, "commands/fix-issue.md");
        drop_entry(&root, "commands/review.md");
        let opted = call(&root, &["--yes", "--only", "commands"], false, &[]);
        assert!(opted.out.contains(
            "  Summary:\n    + 2 new template(s)\n\n  New:\n    + commands/fix-issue.md\n    + commands/review.md\n\n  + commands/fix-issue.md\n  + commands/review.md\n\n  Done. Added: 2 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 0\n"
        ));
        assert!(base.join("commands/review.md").is_file());
    }

    #[test]
    fn a_re_added_script_is_executable_and_a_missing_manifest_heals() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_dir_all(base.join("skills/humanizer/scripts")).unwrap();
        std::fs::remove_dir_all(base.join("skills/humanizer/references")).unwrap();
        std::fs::remove_file(Path::new(&root).join(REL)).unwrap();
        let run = call(&root, &["--yes"], false, &[]);
        assert!(run.out.contains(
            "  New:\n    + skills/humanizer/references/wikipedia_signs_of_ai_writing.md\n    + skills/humanizer/scripts/strip-ai-chars.sh\n\n  + skills/humanizer/references/wikipedia_signs_of_ai_writing.md\n  + skills/humanizer/scripts/strip-ai-chars.sh\n\n  Done. Added: 2 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 16\n"
        ));
        let mode = |rel: &str| {
            std::fs::metadata(base.join(rel))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode("skills/humanizer/scripts/strip-ai-chars.sh"), 0o755);
        assert_eq!(
            mode("skills/humanizer/references/wikipedia_signs_of_ai_writing.md"),
            0o644
        );
        assert_eq!(manifest_text(&root).lines().count(), 19);
        assert_eq!(
            std::fs::metadata(Path::new(&root).join(REL))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
```

Run: `cargo test --lib 2>&1 | grep -E '^error' | sort -u | head -8`
Expected: compile errors naming the missing `refresh`, `Env`, and the module's imports.

- [ ] **Step 3: Write the implementation**

Prepend to `src/cli/refresh.rs`:

```rust
//! `agentsync refresh`: `cmd_refresh` of `lib/helpers/refresh.sh`, which pulls
//! updated templates into `.ai/src/` through a three-way diff against the
//! template manifest, so untouched files update silently and only true
//! conflicts wait for an answer.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::customize::put;
use crate::manifest::sha256_hex;
use crate::style::Style;
use crate::template_manifest::{self, TemplateManifest};
use crate::{Error, catalog, yaml_subset};

/// Printed where Bash printed `$AGENTSYNC_HOME/lib/templates`; the binary
/// reads the embedded copy.
const TEMPLATES_DISPLAY: &str = "/<agentsync>/lib/templates";

const CATEGORIES_VALID: [&str; 5] = ["rules", "skills", "commands", "agents", "subagents"];
const CATEGORIES_DEFAULT: [&str; 4] = ["rules", "skills", "commands", "agents"];

/// What `refresh` takes from the terminal.
pub struct Env<'a> {
    /// `is_tty`: stdin and stdout are both terminals.
    pub interactive: bool,
    /// `read -r reply </dev/tty`, empty when the terminal cannot be read.
    pub read_line: &'a mut dyn FnMut() -> String,
}

struct Options {
    dry_run: bool,
    assume_yes: bool,
    include_agents_md: bool,
    include_deleted: bool,
    review: bool,
    status_only: bool,
    only: String,
}

/// One `<rel>|<template>|<hash>` entry of the `*_FILES` arrays.
struct Candidate {
    rel: String,
    bytes: &'static [u8],
    hash: String,
}

#[derive(Default)]
struct Changes {
    new: Vec<Candidate>,
    conflicts: Vec<Candidate>,
    auto: Vec<Candidate>,
    deleted: Vec<Candidate>,
    unchanged: usize,
    silently_kept: usize,
}

struct Classifier<'a> {
    user_base: &'a Path,
    manifest: &'a TemplateManifest,
    declined: &'a [String],
    pinned: &'a [String],
    review: bool,
    changes: Changes,
}

impl Classifier<'_> {
    /// `_refresh_classify`.
    fn classify(&mut self, rel: &str, bytes: &'static [u8]) {
        if self.declined.iter().any(|item| item == rel) {
            return;
        }
        let t_new = sha256_hex(bytes);
        let t_old = self.manifest.lookup(rel);
        let candidate = || Candidate {
            rel: rel.to_string(),
            bytes,
            hash: t_new.clone(),
        };
        let dest = self.user_base.join(rel);
        if !dest.is_file() {
            if t_old.is_some() {
                self.changes.deleted.push(candidate());
            } else {
                self.changes.new.push(candidate());
            }
            return;
        }
        let Some(u_cur) = template_manifest::hash(&dest) else {
            return;
        };
        if u_cur == t_new {
            self.changes.unchanged += 1;
            return;
        }
        if self.pinned.iter().any(|item| item == rel) {
            return;
        }
        let Some(t_old) = t_old else {
            self.changes.conflicts.push(candidate());
            return;
        };
        if u_cur == t_old {
            self.changes.auto.push(candidate());
            return;
        }
        if t_old == t_new {
            self.changes.silently_kept += 1;
            if self.review {
                self.changes.conflicts.push(candidate());
            }
            return;
        }
        self.changes.conflicts.push(candidate());
    }
}

struct Run<'a, 'b> {
    style: &'a Style,
    env: &'a mut Env<'b>,
    user_base: PathBuf,
    manifest: TemplateManifest,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

pub fn refresh(
    args: &[String],
    root: &str,
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let options = match parse_args(args, style, out, err)? {
        Ok(options) => options,
        Err(status) => return Ok(status),
    };

    let src_base = if Path::new(root).join(".ai/src").is_dir() {
        ".ai/src"
    } else if Path::new(root).join(".ai").is_dir() {
        ".ai"
    } else {
        put(
            err,
            format!(
                "{}: No .ai/ directory found in {root}\nRun {} first.\n",
                style.red("Error"),
                style.cyan("agentsync init")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let user_base_shown = format!("{root}/{src_base}");
    let user_base = PathBuf::from(&user_base_shown);

    let categories = match resolve_scope(&options.only, &user_base_shown, style, err)? {
        Ok(categories) => categories,
        Err(status) => return Ok(status),
    };

    let manifest = TemplateManifest::load(Path::new(root))?;
    let has_manifest = !manifest.is_empty();
    let (declined, pinned) = load_overrides(root);
    let templates = catalog::template_files();
    let changes = collect(
        &templates,
        &user_base,
        &categories,
        options.include_agents_md,
        &manifest,
        &declined,
        &pinned,
        options.review,
    );

    let mut run = Run {
        style,
        env,
        user_base,
        manifest,
        out,
        err,
    };

    if options.status_only {
        run.print_status(&declined, &changes.deleted)?;
        return Ok(0);
    }

    let mut scope_label = categories.join(",");
    if options.include_agents_md {
        scope_label.push_str(",AGENTS.md");
    }
    let mut header = format!(
        "\n{}\n\n  {} {TEMPLATES_DISPLAY}\n  {}   {user_base_shown}\n  {}     {scope_label}\n",
        style.bold("  AgentSync Refresh"),
        style.dim("Templates:"),
        style.dim("Project:"),
        style.dim("Scope:")
    );
    if !has_manifest {
        header.push_str(&format!(
            "  {}  {}\n",
            style.dim("Manifest:"),
            style.yellow("none — falling back to two-way diff")
        ));
    }
    header.push('\n');
    run.say(&header)?;

    let visible_deleted = options.include_deleted && !changes.deleted.is_empty();

    if changes.new.is_empty()
        && changes.conflicts.is_empty()
        && changes.auto.is_empty()
        && !visible_deleted
    {
        let mut text = format!(
            "  {} {} file(s) match the current templates.\n",
            style.green("Already up to date!"),
            changes.unchanged
        );
        if !declined.is_empty() {
            text.push_str(&format!(
                "  {}\n",
                style.dim(&format!(
                    "Persistently declined (agent_sync.yaml): {} file(s).",
                    declined.len()
                ))
            ));
        }
        if !changes.deleted.is_empty() {
            text.push_str(&format!(
                "  {}\n",
                style.dim(&format!(
                    "Locally declined (.template-manifest):    {} file(s); --include-deleted to revisit.",
                    changes.deleted.len()
                ))
            ));
        }
        if !declined.is_empty() || !changes.deleted.is_empty() {
            text.push_str(&format!(
                "  {}\n",
                style.dim("Pass --status for the full list.")
            ));
        }
        if changes.silently_kept > 0 && !options.review {
            text.push_str(&format!(
                "  {}\n",
                style.dim(&format!(
                    "{} file(s) differ from the shipped template (local edits or earlier skips); pass --review to revisit.",
                    changes.silently_kept
                ))
            ));
        }
        run.say(&text)?;
        run.heal(&templates);
        if !options.dry_run {
            run.manifest.write(Path::new(root))?;
        }
        run.say("\n")?;
        return Ok(0);
    }

    let mut summary = format!("  {}\n", style.green("Summary:"));
    if !changes.new.is_empty() {
        summary.push_str(&format!(
            "    {} {} new template(s)\n",
            style.green("+"),
            changes.new.len()
        ));
    }
    if !changes.auto.is_empty() {
        summary.push_str(&format!(
            "    {} {} auto-update(s) — you hadn't touched them locally\n",
            style.cyan("↑"),
            changes.auto.len()
        ));
    }
    if !changes.conflicts.is_empty() {
        summary.push_str(&format!(
            "    {} {} conflict(s) — your version differs from the template\n",
            style.yellow("~"),
            changes.conflicts.len()
        ));
    }
    if visible_deleted {
        summary.push_str(&format!(
            "    {} {} previously declined — pass --include-deleted to revisit\n",
            style.dim("?"),
            changes.deleted.len()
        ));
    }
    if changes.silently_kept > 0 && !options.review {
        summary.push_str(&format!(
            "    {} {} silently kept (local edits or earlier skips) — pass --review to revisit\n",
            style.dim("·"),
            changes.silently_kept
        ));
    }
    if changes.unchanged > 0 {
        summary.push_str(&format!(
            "    {} {} unchanged\n",
            style.dim("·"),
            changes.unchanged
        ));
    }
    summary.push('\n');
    run.say(&summary)?;

    run.list_proposed(&changes, visible_deleted)?;

    if options.dry_run {
        run.say(&format!(
            "  {} — no files written.\n\n",
            style.yellow("Dry run")
        ))?;
        return Ok(0);
    }

    if !run.env.interactive
        && !options.assume_yes
        && changes.new.len() + changes.conflicts.len() > 0
    {
        put(
            run.err,
            format!(
                "  {}: Cannot run interactively (not a TTY).\n  Use {} to add new files and apply auto-updates\n  (conflicts are always skipped non-interactively).\n  Use {} to preview.\n",
                style.red("Error"),
                style.cyan("--yes"),
                style.cyan("--dry-run")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }

    let (mut added, mut updated, mut auto_applied, mut skipped) = (0, 0, 0, 0);
    let mut cancelled = false;

    for entry in &changes.auto {
        run.copy(entry)?;
        run.say(&format!(
            "  {} {}  {}\n",
            style.cyan("↑"),
            entry.rel,
            style.dim("(auto-updated; you hadn't touched it)")
        ))?;
        auto_applied += 1;
    }

    if visible_deleted {
        for entry in &changes.deleted {
            if cancelled {
                break;
            }
            if options.assume_yes {
                run.say(&format!(
                    "  {} {} {}\n",
                    style.dim("?"),
                    entry.rel,
                    style.dim("(previously declined — skipped under --yes; run interactively)")
                ))?;
                skipped += 1;
                continue;
            }
            match run.prompt_deleted(entry)? {
                'a' => {
                    run.copy(entry)?;
                    run.say(&format!("    {}\n", style.green("restored.")))?;
                    added += 1;
                }
                'q' => cancelled = true,
                _ => {
                    run.say(&format!("    {}\n", style.dim("still declined.")))?;
                    skipped += 1;
                }
            }
        }
    }

    if !changes.new.is_empty() && !cancelled {
        for entry in &changes.new {
            if cancelled {
                break;
            }
            if options.assume_yes {
                run.copy(entry)?;
                run.say(&format!("  {} {}\n", style.green("+"), entry.rel))?;
                added += 1;
                continue;
            }
            match run.prompt_new(entry)? {
                'a' => {
                    run.copy(entry)?;
                    run.say(&format!("    {}\n", style.green("added.")))?;
                    added += 1;
                }
                'q' => cancelled = true,
                _ => {
                    // Skip-as-decline: recorded so the file never reappears as NEW.
                    run.manifest.record(&entry.rel, &entry.hash);
                    run.say(&format!(
                        "    {}\n",
                        style.dim(
                            "declined (will not be offered again — use --include-deleted to revisit)."
                        )
                    ))?;
                    skipped += 1;
                }
            }
        }
    }

    if !changes.conflicts.is_empty() && !cancelled {
        for entry in &changes.conflicts {
            if cancelled {
                break;
            }
            if options.assume_yes {
                run.say(&format!(
                    "  {} {} {}\n",
                    style.yellow("~"),
                    entry.rel,
                    style.dim("(conflict — skipped; run interactively to review)")
                ))?;
                skipped += 1;
                continue;
            }
            match run.prompt_conflict(entry)? {
                'u' => {
                    run.copy(entry)?;
                    run.say(&format!("    {}\n", style.yellow("updated.")))?;
                    updated += 1;
                }
                'q' => cancelled = true,
                _ => {
                    // Recorded at the new template hash so the skip is remembered.
                    run.manifest.record(&entry.rel, &entry.hash);
                    run.say(&format!(
                        "    {}\n",
                        style.dim("skipped (remembered — agentsync refresh --review to revisit).")
                    ))?;
                    skipped += 1;
                }
            }
        }
    }

    run.heal(&templates);
    run.manifest.write(Path::new(root))?;

    let mut closing = String::from("\n");
    if cancelled {
        closing.push_str(&format!(
            "  {} Files already applied are kept.\n",
            style.yellow("Cancelled.")
        ));
    }
    closing.push_str(&format!(
        "  {} Added: {added} · Auto-updated: {auto_applied} · Updated: {updated} · Skipped: {skipped} · Unchanged: {}\n",
        style.green("Done."),
        changes.unchanged
    ));
    if added + auto_applied + updated > 0 {
        closing.push_str(&format!(
            "\n  Next: {} to distribute the updates to enabled tools.\n",
            style.cyan("agentsync sync")
        ));
    }
    closing.push('\n');
    run.say(&closing)?;
    Ok(0)
}

fn parse_args(
    args: &[String],
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Result<Options, u8>, Error> {
    let mut options = Options {
        dry_run: false,
        assume_yes: false,
        include_agents_md: false,
        include_deleted: false,
        review: false,
        status_only: false,
        only: String::new(),
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            "--yes" | "-y" => options.assume_yes = true,
            "--include-agents-md" => options.include_agents_md = true,
            "--include-deleted" => options.include_deleted = true,
            "--review" => options.review = true,
            "--status" => options.status_only = true,
            "--only" => match rest.next() {
                Some(value) => options.only = value.clone(),
                None => {
                    put(
                        err,
                        format!("{}: --only requires a value\n", style.red("Error")).as_bytes(),
                    )?;
                    return Ok(Err(1));
                }
            },
            "--help" | "-h" => {
                put(out, usage(style).as_bytes())?;
                return Ok(Err(0));
            }
            flag if flag.starts_with("--only=") => {
                options.only = flag["--only=".len()..].to_string();
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
                return Ok(Err(1));
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
                return Ok(Err(1));
            }
        }
    }
    Ok(Ok(options))
}

/// `_refresh_resolve_scope`: the categories present under `user_base`, or the
/// validated, deduplicated `--only` list in the order given.
fn resolve_scope(
    only: &str,
    user_base: &str,
    style: &Style,
    err: &mut dyn Write,
) -> Result<Result<Vec<String>, u8>, Error> {
    if only.is_empty() {
        let found: Vec<String> = CATEGORIES_DEFAULT
            .iter()
            .filter(|category| Path::new(user_base).join(category).is_dir())
            .map(|category| category.to_string())
            .collect();
        if found.is_empty() {
            put(
                err,
                format!(
                    "{}: No source content categories present in {user_base}.\nPass {} to opt into specific ones,\nor run {} to scaffold them.\n",
                    style.red("Error"),
                    style.cyan("--only rules,skills,commands,agents"),
                    style.cyan("agentsync init")
                )
                .as_bytes(),
            )?;
            return Ok(Err(1));
        }
        return Ok(Ok(found));
    }
    let mut categories: Vec<String> = Vec::new();
    for token in only.split(',') {
        let token = token.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
        if token.is_empty() {
            continue;
        }
        // `subagents` is the init/--content token; the directory is `agents`.
        let token = if token == "subagents" {
            "agents"
        } else {
            token
        };
        if !CATEGORIES_VALID.contains(&token) {
            put(
                err,
                format!(
                    "{}: Unknown --only value: {token}\nValid: rules, skills, commands, agents (or subagents)\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(Err(1));
        }
        if !categories.iter().any(|known| known == token) {
            categories.push(token.to_string());
        }
    }
    if categories.is_empty() {
        put(
            err,
            format!(
                "{}: --only must include at least one category\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(Err(1));
    }
    Ok(Ok(categories))
}

/// `_refresh_load_overrides`: `template_overrides.declined` and `.pinned` from
/// `.ai/agent_sync.yaml`, else a root `agent_sync.yaml`.
fn load_overrides(root: &str) -> (Vec<String>, Vec<String>) {
    let text = [
        format!("{root}/.ai/agent_sync.yaml"),
        format!("{root}/agent_sync.yaml"),
    ]
    .into_iter()
    .find(|path| Path::new(path).is_file())
    .and_then(|path| std::fs::read(path).ok())
    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    .unwrap_or_default();
    let list = |key: &str| -> Vec<String> {
        yaml_subset::list(&text, key)
            .into_iter()
            .filter(|item| !item.is_empty())
            .collect()
    };
    (
        list("template_overrides.declined"),
        list("template_overrides.pinned"),
    )
}

/// `_refresh_collect_changes`: `AGENTS.md` when asked, then the `*.md` files of
/// `rules`, `commands`, and `agents`, then every file below `skills`.
#[allow(clippy::too_many_arguments)]
fn collect(
    templates: &[(String, &'static [u8])],
    user_base: &Path,
    categories: &[String],
    include_agents_md: bool,
    manifest: &TemplateManifest,
    declined: &[String],
    pinned: &[String],
    review: bool,
) -> Changes {
    let mut classifier = Classifier {
        user_base,
        manifest,
        declined,
        pinned,
        review,
        changes: Changes::default(),
    };
    let in_scope = |category: &str| categories.iter().any(|c| c == category);
    if include_agents_md {
        for (rel, bytes) in templates.iter().filter(|(rel, _)| rel == "AGENTS.md") {
            classifier.classify(rel, bytes);
        }
    }
    for category in ["rules", "commands", "agents"] {
        if !in_scope(category) {
            continue;
        }
        for (rel, bytes) in templates
            .iter()
            .filter(|(rel, _)| rel.rsplit_once('/').is_some_and(|(dir, _)| dir == category))
        {
            classifier.classify(rel, bytes);
        }
    }
    if in_scope("skills") {
        for (rel, bytes) in templates
            .iter()
            .filter(|(rel, _)| rel.starts_with("skills/"))
        {
            classifier.classify(rel, bytes);
        }
    }
    classifier.changes
}

impl Run<'_, '_> {
    fn say(&mut self, text: &str) -> Result<(), Error> {
        put(self.out, text.as_bytes())
    }

    fn tell(&mut self, text: &str) -> Result<(), Error> {
        put(self.err, text.as_bytes())
    }

    /// `_refresh_heal_unchanged`.
    fn heal(&mut self, templates: &[(String, &'static [u8])]) {
        self.manifest.heal_from_match(
            templates.iter().map(|(rel, bytes)| (rel.as_str(), *bytes)),
            &self.user_base,
        );
    }

    /// `_refresh_copy` followed by `template_manifest_record`.
    fn copy(&mut self, entry: &Candidate) -> Result<(), Error> {
        write_template(&self.user_base.join(&entry.rel), entry.bytes)?;
        self.manifest.record(&entry.rel, &entry.hash);
        Ok(())
    }

    /// `_refresh_list_proposed`.
    fn list_proposed(&mut self, changes: &Changes, visible_deleted: bool) -> Result<(), Error> {
        let style = self.style;
        let mut text = String::new();
        let mut section = |title: String, marker: String, entries: &[Candidate]| {
            if entries.is_empty() {
                return;
            }
            text.push_str(&format!("  {title}\n"));
            for entry in entries {
                text.push_str(&format!("    {marker} {}\n", entry.rel));
            }
            text.push('\n');
        };
        section(style.green("New:"), style.green("+"), &changes.new);
        section(
            format!(
                "{} {}",
                style.cyan("Auto-update:"),
                style.dim("(your version matches the previous template; safe to update)")
            ),
            style.cyan("↑"),
            &changes.auto,
        );
        section(
            format!(
                "{} {}",
                style.yellow("Conflicts:"),
                style.dim(
                    "(both your version and the template diverged from the recorded baseline)"
                )
            ),
            style.yellow("~"),
            &changes.conflicts,
        );
        if visible_deleted {
            section(
                style.dim("Previously declined:"),
                style.dim("?"),
                &changes.deleted,
            );
        }
        self.say(&text)
    }

    /// `_refresh_print_status`.
    fn print_status(&mut self, declined: &[String], deleted: &[Candidate]) -> Result<(), Error> {
        let style = self.style;
        let mut text = format!("\n{}\n", style.bold("  Declined templates"));
        if declined.is_empty() && deleted.is_empty() {
            text.push_str(&format!("  {}\n\n", style.dim("Nothing declined.")));
            return self.say(&text);
        }
        if !declined.is_empty() {
            text.push_str(&format!(
                "  {}  {}\n",
                style.yellow("Persistent"),
                style.dim("(template_overrides.declined in agent_sync.yaml — never offered):")
            ));
            for item in declined {
                text.push_str(&format!("    {} {item}\n", style.dim("·")));
            }
            text.push('\n');
        }
        if !deleted.is_empty() {
            text.push_str(&format!(
                "  {}       {}\n",
                style.yellow("Local"),
                style
                    .dim("(.template-manifest — deleted from disk; --include-deleted to restore):")
            ));
            for entry in deleted {
                text.push_str(&format!("    {} {}\n", style.dim("·"), entry.rel));
            }
            text.push('\n');
        }
        self.say(&text)
    }

    /// `read -r reply </dev/tty`, lowercased, `s` when empty.
    fn answer(&mut self) -> String {
        let reply = (self.env.read_line)();
        let reply = reply.trim_matches([' ', '\t']).to_lowercase();
        if reply.is_empty() {
            "s".to_string()
        } else {
            reply
        }
    }

    /// `_refresh_prompt_new`: `a`, `s`, or `q`.
    fn prompt_new(&mut self, entry: &Candidate) -> Result<char, Error> {
        let style = self.style;
        let banner = format!("\n  {} {}\n", style.green("+ NEW:"), style.cyan(&entry.rel));
        self.prompt_add(entry, &banner)
    }

    /// `_refresh_prompt_deleted`: `a`, `s`, or `q`.
    fn prompt_deleted(&mut self, entry: &Candidate) -> Result<char, Error> {
        let style = self.style;
        let banner = format!(
            "\n  {} {}  {}\n",
            style.dim("? RESTORE:"),
            style.cyan(&entry.rel),
            style.dim("(previously declined)")
        );
        self.prompt_add(entry, &banner)
    }

    fn prompt_add(&mut self, entry: &Candidate, banner: &str) -> Result<char, Error> {
        let style = self.style;
        loop {
            self.tell(&format!(
                "{banner}    [{}]dd  [{}]kip  [v]iew  [q]uit  > ",
                style.green("a"),
                style.yellow("s")
            ))?;
            match self.answer().as_str() {
                "a" | "add" => return Ok('a'),
                "s" | "skip" => return Ok('s'),
                "v" | "view" => self.show_new(entry.bytes)?,
                "q" | "quit" => return Ok('q'),
                _ => self.tell(&format!(
                    "    {}\n",
                    style.dim("(unknown choice — try a, s, v, q)")
                ))?,
            }
        }
    }

    /// `_refresh_prompt_conflict`: `u`, `s`, or `q`.
    fn prompt_conflict(&mut self, entry: &Candidate) -> Result<char, Error> {
        let style = self.style;
        loop {
            self.tell(&format!(
                "\n  {} {}\n    [{}]pdate  [{}]kip  [v]iew  [q]uit  > ",
                style.yellow("~ CONFLICT:"),
                style.cyan(&entry.rel),
                style.yellow("u"),
                style.yellow("s")
            ))?;
            match self.answer().as_str() {
                "u" | "update" => return Ok('u'),
                "s" | "skip" => return Ok('s'),
                "v" | "view" => {
                    let dest = self.user_base.join(&entry.rel);
                    self.show_diff(&dest, entry.bytes)?;
                }
                "q" | "quit" => return Ok('q'),
                _ => self.tell(&format!(
                    "    {}\n",
                    style.dim("(unknown choice — try u, s, v, q)")
                ))?,
            }
        }
    }

    /// `_refresh_show_new`: the template, each line indented, on stderr.
    fn show_new(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let style = self.style;
        let mut text = format!("\n  {}\n\n", style.dim("──── new file content ────"));
        let content = String::from_utf8_lossy(bytes);
        let mut lines: Vec<&str> = content.split('\n').collect();
        if content.ends_with('\n') || content.is_empty() {
            lines.pop();
        }
        for line in lines {
            text.push_str(&format!("  {line}\n"));
        }
        text.push('\n');
        self.tell(&text)
    }

    /// `_refresh_show_diff`: `diff -u --label yours --label template`, on stderr.
    fn show_diff(&mut self, dest: &Path, template: &[u8]) -> Result<(), Error> {
        let style = self.style;
        let mut text = format!("\n  {}\n\n", style.dim("──── diff: yours → template ────"));
        match unified_diff(dest, template) {
            Some(hunks) => text.push_str(&String::from_utf8_lossy(&hunks)),
            None => text.push_str(&format!(
                "  {}\n",
                style.red("(diff command not available)")
            )),
        }
        text.push('\n');
        self.tell(&text)
    }
}

/// `diff -u --label yours --label template <yours> <template>`, the embedded
/// template written to a temporary file for the call; `None` when `diff`
/// cannot start.
fn unified_diff(yours: &Path, template: &[u8]) -> Option<Vec<u8>> {
    let staged = stage_template(template)?;
    let output = Command::new("diff")
        .args(["-u", "--label", "yours", "--label", "template"])
        .arg(yours)
        .arg(&staged)
        .stdin(Stdio::null())
        .output();
    let _ = std::fs::remove_file(&staged);
    let output = output.ok()?;
    let mut text = output.stdout;
    text.extend_from_slice(&output.stderr);
    Some(text)
}

fn stage_template(bytes: &[u8]) -> Option<PathBuf> {
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    for attempt in 0u32..100 {
        let path = dir.join(format!("agentsync-refresh-{pid}-{attempt}"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        match options.open(&path) {
            Ok(mut file) => {
                if file.write_all(bytes).is_err() {
                    let _ = std::fs::remove_file(&path);
                    return None;
                }
                return Some(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// `mkdir -p` and `cp <template> <dest>`. An existing file keeps its mode; a new
/// one is created executable when the template starts with `#!`, the mode the
/// shipped scripts carry in the checkout `cp` copied from.
fn write_template(dest: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if bytes.starts_with(b"#!") {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o755);
    }
    options
        .open(dest)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|e| Error::io(dest, e))
}

/// `_refresh_usage`.
fn usage(style: &Style) -> String {
    format!(
        "\n  {} — pull new template files into an existing .ai/src/\n\n  {}\n    agentsync refresh [options]\n\n  {}\n    Compares each shipped template (rules, skills, commands, agents) against\n    your local .ai/src/ using a three-way diff (template-old vs template-new\n    vs your current file) when a template manifest is present. Files you\n    haven't touched auto-update silently; only true conflicts require review.\n    Files in .ai/src/ that aren't part of the templates (your custom content)\n    are left alone.\n\n  {}\n    --only <csv>           Categories to consider: rules, skills, commands, agents\n                           Default: only categories that already have a subdir\n                           in your .ai/src/. Pass --only to opt into a category\n                           you don't have yet.\n    --include-agents-md    Also offer updates to AGENTS.md (off by default —\n                           almost always heavily customized).\n    --include-deleted      Re-offer files you previously declined and removed\n                           from disk so they can be restored.\n    --review               Resurface every local divergence from the shipped\n                           templates, including conflicts you previously\n                           [s]kipped. Use this to revisit earlier decisions\n                           or audit local edits.\n    --status               Print declined breakdown (persistent + local) and\n                           exit. No mutation, no prompts.\n    --dry-run              Print the plan without writing anything.\n    -y, --yes              Apply auto-updates and add new files; skip conflicts\n                           (no prompts). Required in non-interactive contexts.\n    -h, --help             Show this help.\n\n  {}\n    Picking {} on a conflict records the current template hash in\n    .ai/.template-manifest. The divergence stays silent on future refreshes\n    until a newer template ships (at which point it resurfaces automatically\n    so you can review the new change). Pass {} at any time to\n    revisit your skips explicitly.\n\n  {}\n    Edit .ai/agent_sync.yaml to silence specific templates forever (this is\n    stronger than {} — even new template versions stay hidden):\n\n      template_overrides:\n        declined:        # always-skip; never offered\n          - rules/some-rule.md\n        pinned:          # ignore template updates; keep your version\n          - rules/my-version.md\n\n  {}\n    agentsync refresh\n    agentsync refresh --only rules,skills\n    agentsync refresh --dry-run\n    agentsync refresh --yes               # CI-friendly: auto-update + add new\n    agentsync refresh --include-deleted   # revisit previously declined files\n    agentsync refresh --review            # revisit conflicts you skipped\n",
        style.bold("agentsync refresh"),
        style.green("USAGE"),
        style.green("DESCRIPTION"),
        style.green("OPTIONS"),
        style.green("REMEMBERED SKIPS"),
        style.yellow("[s]kip"),
        style.cyan("--review"),
        style.green("PERSISTENT OVERRIDES"),
        style.yellow("[s]kip"),
        style.green("EXAMPLES")
    )
}

```

In `src/main.rs`, before the `upgrade-config` block:

```rust
    if args.first().and_then(|a| a.to_str()) == Some("refresh") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let root = match var("AGENTSYNC_REPO_ROOT").filter(|root| !root.is_empty()) {
            Some(root) => root,
            None => {
                let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
                paths::logical_root(None, &cwd, var("PWD").as_deref())
            }
        };
        let mut env = cli::refresh::Env {
            interactive: prompts::is_tty(),
            read_line: &mut prompts::read_terminal,
        };
        return cli::refresh::refresh(
            &rest,
            &root,
            &Style::for_stdout(),
            &mut env,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
```

In `bin/agentsync.sh:280` append `refresh`:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check sync rollback enable disable customize show diff simplify resolve upgrade-config profile dedupe adopt migrate refresh "
```

- [ ] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/refresh.bats | grep -c '^not ok'
bats --tap -f 'parity: refresh' tests/native_parity.bats
```

Expected: `243 passed`, `0`, `11`, `1`; `0`; `ok 1` and `ok 2`.

- [ ] **Step 5: Prove the fixture bites, check the prompts on a terminal, lint, commit**

Change `(auto-updated; you hadn't touched it)` to `(auto-updated; untouched)` in `src/cli/refresh.rs`, rebuild, rerun `bats --tap -f 'refresh plans' tests/native_parity.bats`: `not ok 1` with the auto-update line in the diff; revert and rebuild.

Recreate the two harnesses when the session scratchpad no longer holds `phase4h/`. Both take `<engine 0|1> <repo root> <out file>`, seed one project with `init` under `phase4h/seed_ref`, and mask the clone and the templates path. The pty harness needs `script`, which the agent sandbox refuses (`script: openpty: Operation not permitted`), so it runs outside the sandbox; both use macOS `stat -f%Lp` for modes.

`phase4h/refresh_reference.sh`:

```bash
#!/usr/bin/env bash
# Usage: refresh_reference.sh <engine 0|1> <repo root> <out file>
# Runs every non-interactive refresh branch on clones of one seeded project and
# prints status, masked output, and the resulting tree with hashes and modes.
set -uo pipefail
MODE="$1"; REPO="$2"; OUT="$3"
S="$(cd "$(dirname "$0")" && pwd)"
SEED="$S/seed_ref"
if [[ ! -d "$SEED/.ai" ]]; then
    mkdir -p "$SEED"
    (cd "$SEED" && git init --quiet && AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$REPO" bash "$REPO/bin/agentsync.sh" init >/dev/null 2>&1)
fi
: > "$OUT"
export AGENTSYNC_HOME="$REPO"
export AGENTSYNC_NATIVE="$MODE"
export AGENTSYNC_NATIVE_BIN="$REPO/target/release/agentsync"
WORK="$S/work_$MODE"
rm -rf "$WORK"
mkdir -p "$WORK"
N=0
CLONE=""
fresh() {
    N=$((N + 1))
    CLONE="$WORK/c$N"
    cp -pR "$SEED" "$CLONE"
    cd "$CLONE" || exit 1
}
mask() {
    local text; text=$(cat)
    local phys; phys=$(cd -P "$CLONE" && pwd)
    text=${text//"$phys"/<root>}
    text=${text//"$CLONE"/<root>}
    local from
    for from in "$REPO/lib/templates" "/<agentsync>/lib/templates"; do
        text=${text//"$from"/<engine>/lib/templates}
    done
    printf '%s\n' "$text"
}
report() {
    local name="$1" rc="$2" output="$3"
    {
        echo "### $name"
        echo "rc=$rc"
        printf '%s' "$output" | mask
        echo "--- tree"
        (cd "$CLONE" && find .ai -type f ! -path '*/.ai/backups/*' -print0 | LC_ALL=C sort -z | while IFS= read -r -d '' f; do
            printf '%s %s %s\n' "$(shasum -a 256 "$f" | cut -c1-12)" "$(stat -f%Lp "$f")" "$f"
        done)
        echo "--- manifest"
        (cd "$CLONE" && { cat .ai/.template-manifest 2>/dev/null | cut -c1-40 || true; })
        echo
    } >> "$OUT"
}
run() {
    local name="$1"; shift
    local rc=0 output
    output=$(cd "$CLONE" && bash "$REPO/bin/agentsync.sh" "$@" 2>&1) || rc=$?
    report "$name :: agentsync $*" "$rc" "$output"
}
run_env_root() {
    local name="$1"; shift
    local rc=0 output
    output=$(cd "$S" && AGENTSYNC_REPO_ROOT="$CLONE" bash "$REPO/bin/agentsync.sh" "$@" 2>&1) || rc=$?
    report "$name :: AGENTSYNC_REPO_ROOT agentsync $*" "$rc" "$output"
}
drop() {
    grep -v $'^'"$1"$'\t' .ai/.template-manifest > .ai/.tm.tmp || true; mv .ai/.tm.tmp .ai/.template-manifest
}
drop_prefix() {
    grep -v $'^'"$1" .ai/.template-manifest > .ai/.tm.tmp || true; mv .ai/.tm.tmp .ai/.template-manifest
}
set_hash() {
    drop "$1"; printf '%s\t%s\n' "$1" "$2" >> .ai/.template-manifest; LC_ALL=C sort -u -o .ai/.template-manifest .ai/.template-manifest
}
h() { shasum -a 256 "$1" | cut -c1-64; }

fresh; run "help" refresh --help
run "help-first" refresh --help --bogus
run "unknown-option" refresh --bogus --help
run "unexpected-arg" refresh extra
run "only-missing-value" refresh --yes --only
run "only-bogus" refresh --yes --only bogus
run "only-empty-token" refresh --yes --only ,
run "up-to-date" refresh --yes
run "up-to-date-again" refresh --yes
run "up-to-date-dry" refresh --dry-run
run "status-empty" refresh --status

fresh; rm -f .ai/src/rules/comments.md; drop "rules/comments.md"
run "new-dry" refresh --dry-run
run "new-non-tty" refresh
run "new-yes" refresh --yes
run "new-yes-again" refresh --yes

fresh; rm -f .ai/src/rules/comments.md
run "deleted-silent" refresh --yes
run "deleted-include-dry" refresh --include-deleted --dry-run
run "deleted-status" refresh --status
run "deleted-include-yes" refresh --yes --include-deleted

fresh; printf '\ntemplate_overrides:\n  declined:\n    - rules/comments.md\n    - rules/git.md\n  pinned:\n    - rules/core.md\n' >> .ai/agent_sync.yaml
rm -f .ai/src/rules/comments.md; drop "rules/comments.md"
echo "USER LOCAL EDIT" >> .ai/src/rules/core.md; drop "rules/core.md"
run "declined-pinned-status" refresh --status
run "declined-pinned-yes" refresh --yes
rm -f .ai/src/rules/git.md
run "declined-and-local-deleted" refresh --yes
run "declined-and-local-status" refresh --status

fresh; echo "USER LOCAL EDIT" >> .ai/src/rules/core.md; set_hash "rules/core.md" "$(h .ai/src/rules/core.md)"
run "auto-update-dry" refresh --dry-run
run "auto-update-non-tty" refresh
run "auto-update-again" refresh --yes

fresh; echo "USER LOCAL EDIT" >> .ai/src/rules/core.md; drop "rules/core.md"
run "conflict-no-baseline-dry" refresh --dry-run
run "conflict-no-baseline-yes" refresh --yes
run "conflict-no-baseline-non-tty" refresh

fresh; echo "USER LOCAL EDIT" >> .ai/src/rules/core.md; set_hash "rules/core.md" "0000000000000000000000000000000000000000000000000000000000000000"
run "conflict-both-moved-yes" refresh --yes

fresh; echo "USER LOCAL EDIT" >> .ai/src/rules/core.md
run "silently-kept" refresh --yes
run "silently-kept-review-dry" refresh --review --dry-run
run "silently-kept-review-yes" refresh --review --yes
run "silently-kept-review-non-tty" refresh --review

fresh; echo "USER LOCAL EDIT" >> .ai/src/AGENTS.md
run "agents-md-excluded" refresh --yes
drop "AGENTS.md"
run "agents-md-included-yes" refresh --yes --include-agents-md
run "agents-md-included-dry" refresh --include-agents-md --dry-run

fresh; rm -f .ai/src/rules/comments.md; rm -rf .ai/src/skills/comments; drop "rules/comments.md"; drop "skills/comments/SKILL.md"
run "only-rules" refresh --yes --only rules
run "only-eq-skills-spaced" refresh --yes "--only= skills , rules ,skills"
fresh; rm -rf .ai/src/agents; drop_prefix "agents/"
run "only-subagents" refresh --yes --only subagents
fresh; rm -rf .ai/src/commands; drop_prefix "commands/"
run "scope-skips-absent" refresh --yes
run "only-commands-opt-in" refresh --yes --only commands

fresh; rm -f .ai/.template-manifest; echo "USER LOCAL EDIT" >> .ai/src/rules/core.md
run "no-manifest-dry" refresh --dry-run
run "no-manifest-yes" refresh --yes
fresh; rm -f .ai/.template-manifest
run "no-manifest-heal" refresh --yes
run "no-manifest-heal-status" refresh --status

fresh; rm -rf .ai/src/skills/humanizer/references .ai/src/skills/humanizer/scripts; drop_prefix "skills/humanizer/references/"; drop_prefix "skills/humanizer/scripts/"
run "nested-new-yes" refresh --yes

fresh; printf '# My Custom Rule\n' > .ai/src/rules/my-custom.md
run "custom-file-kept" refresh --yes

fresh; rm -rf .ai/src/rules .ai/src/skills .ai/src/commands .ai/src/agents
run "no-categories" refresh --yes
run "no-categories-only" refresh --yes --only rules

fresh; mkdir -p legacy; mv .ai/src/* legacy/; rmdir .ai/src; mv legacy/* .ai/; rmdir legacy
run "legacy-base" refresh --yes
run "legacy-base-status" refresh --status

fresh; rm -rf .ai
run "no-ai" refresh --yes
run "no-ai-status" refresh --status

fresh; rm -f .ai/src/rules/comments.md; drop "rules/comments.md"; echo "USER LOCAL EDIT" >> .ai/src/rules/core.md; set_hash "rules/core.md" "$(h .ai/src/rules/core.md)"; echo "EDIT" >> .ai/src/rules/git.md; drop "rules/git.md"; rm -f .ai/src/commands/review.md
run "mixed-dry" refresh --dry-run --include-deleted
run "mixed-yes" refresh --yes --include-deleted --review
run "mixed-after" refresh --yes

fresh; run_env_root "repo-root-env" refresh --yes
fresh; rm -rf .ai; run_env_root "repo-root-env-no-ai" refresh --yes
```

`phase4h/refresh_tty.sh`:

```bash
#!/usr/bin/env bash
# Usage: refresh_tty.sh <engine 0|1> <repo root> <out file>
# Drives the interactive prompts of refresh on a pseudo-terminal through
# `script`, so is_tty holds and /dev/tty is the pty, and records the masked
# transcript plus the resulting tree.
set -uo pipefail
MODE="$1"; REPO="$2"; OUT="$3"
S="$(cd "$(dirname "$0")" && pwd)"
SEED="$S/seed_ref"
WORK="$S/tty_$MODE"
rm -rf "$WORK"
mkdir -p "$WORK"
: > "$OUT"
export AGENTSYNC_HOME="$REPO"
export AGENTSYNC_NATIVE="$MODE"
export AGENTSYNC_NATIVE_BIN="$REPO/target/release/agentsync"
export AGENTSYNC_NO_UPDATE_CHECK=1
N=0
CLONE=""
fresh() {
    N=$((N + 1))
    CLONE="$WORK/c$N"
    cp -pR "$SEED" "$CLONE"
    cd "$CLONE" || exit 1
}
drop() {
    grep -v $'^'"$1"$'\t' .ai/.template-manifest > .ai/.tm.tmp || true; mv .ai/.tm.tmp .ai/.template-manifest
}
mask() {
    local text; text=$(cat)
    local phys; phys=$(cd -P "$CLONE" && pwd)
    text=${text//"$phys"/<root>}
    text=${text//"$CLONE"/<root>}
    local from
    for from in "$REPO/lib/templates" "/<agentsync>/lib/templates"; do
        text=${text//"$from"/<engine>/lib/templates}
    done
    printf '%s\n' "$text" | tr -d '\r'
}
# drive <name> <replies-with-\n> <args...>
drive() {
    local name="$1" replies="$2"; shift 2
    local rc=0 transcript
    transcript=$(cd "$CLONE" && printf '%b' "$replies" | script -q /dev/null bash "$REPO/bin/agentsync.sh" "$@" 2>&1) || rc=$?
    {
        echo "### $name :: agentsync $*"
        echo "rc=$rc"
        printf '%s' "$transcript" | mask
        echo "--- tree"
        (cd "$CLONE" && find .ai -type f ! -path '*/.ai/backups/*' -print0 | LC_ALL=C sort -z | while IFS= read -r -d '' f; do
            printf '%s %s %s\n' "$(shasum -a 256 "$f" | cut -c1-12)" "$(stat -f%Lp "$f")" "$f"
        done)
        echo "--- manifest"
        (cd "$CLONE" && { cat .ai/.template-manifest 2>/dev/null | cut -c1-40 || true; })
        echo
    } >> "$OUT"
}

fresh
rm -f .ai/src/rules/comments.md; drop "rules/comments.md"
rm -f .ai/src/commands/review.md
echo "EDIT" >> .ai/src/rules/git.md; drop "rules/git.md"
drive "view-restore-unknown-skip-diff-update" 'v\na\nx\n\nview\nUpdate\n' refresh --include-deleted

fresh
rm -f .ai/src/rules/comments.md; drop "rules/comments.md"
echo "EDIT" >> .ai/src/rules/git.md; drop "rules/git.md"
drive "skip-then-quit" 'SKIP\nq\n' refresh

fresh
rm -f .ai/src/rules/comments.md; drop "rules/comments.md"
drive "add-on-terminal" 'add\n' refresh

fresh
rm -f .ai/src/commands/review.md
drive "restore-declined-still" 's\n' refresh --include-deleted

fresh
drive "up-to-date-colours" '' refresh
drive "status-colours" '' refresh --status
drive "help-colours" '' refresh --help
```

```bash
bash phase4h/refresh_reference.sh 0 "$PWD" phase4h/ref_bash.out && bash phase4h/refresh_reference.sh 1 "$PWD" phase4h/ref_native.out
diff phase4h/ref_bash.out phase4h/ref_native.out | grep -c '^[<>]'
diff phase4h/ref_bash.out phase4h/ref_native.out | grep '^[<>]' | grep -vc '\.ai/\.template-manifest$'
grep -c '755 .ai/src/skills/humanizer/scripts/strip-ai-chars.sh' phase4h/ref_native.out
bash phase4h/refresh_tty.sh 0 "$PWD" phase4h/tty_bash.out && bash phase4h/refresh_tty.sh 1 "$PWD" phase4h/tty_native.out
wc -l < phase4h/tty_native.out
diff phase4h/tty_bash.out phase4h/tty_native.out | grep '^[<>]' | grep -vc '\.ai/\.template-manifest$'
grep -c '^rc=0' phase4h/tty_native.out
```

Expected: 3658-line transcripts; `34` differing lines, `0` of them outside the manifest mode (the harness rewrites the manifest through a shell redirect, `0644`, which Bash's `sort -o` turns back into `0600` and the binary keeps: decision 2); the restored script is `755` in every scenario that lists it (`52` lines); `600` transcript lines; `0` differences outside the manifest mode; `7` scenarios at `rc=0`, prompts, view, diff, cancellation, and colours identical.

```bash
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/refresh.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/plans/2026-09-15-rust-migration-phase-4h-refresh.md
git commit -m "feat(native): port refresh"
```

---

### Task 3: Verify, Spec, and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [ ] **Step 1: Spec**

Append to "Known quirks":

```markdown
38. `refresh` heals `.ai/.template-manifest` with every shipped template that
    matches its copy, including categories outside `--only` and `AGENTS.md`
    without `--include-agents-md`.
39. Off a terminal without `--yes`, `refresh` applies pending auto-updates,
    because the TTY gate looks only at new files and conflicts; with
    `--include-deleted` and nothing else pending it prints each RESTORE prompt
    on stderr and declines it.
40. `refresh` reads `template_overrides` from `.ai/agent_sync.yaml`, else a
    root `agent_sync.yaml`, ignoring `AGENTSYNC_CONFIG_PATH`.
```

Append to "Accepted deviations":

```markdown
- Phase 4h: `refresh` prints `Templates: /<agentsync>/lib/templates` and
  compares against the embedded templates where Bash printed and read
  `$AGENTSYNC_HOME/lib/templates`.
- Phase 4h: a template `refresh` creates is executable when it starts with
  `#!`, the mode the shipped scripts carry, where Bash's `cp` copied the
  checkout's mode; an existing file keeps its mode in both engines.
- Phase 4h: `refresh` and `migrate` keep an existing `.ai/.template-manifest`
  mode, as `TemplateManifest::write` has since Phase 4g, where Bash's
  `template_manifest_write` lets `sort -o` rewrite the file, which BSD sort
  does through a new `0600` inode.
- Phase 4h: a `refresh` prompt whose terminal device cannot be opened declines
  silently where Bash also printed the shell's `/dev/tty` open error.
```

- [ ] **Step 2: Module map and outputs**

Set the `lib/helpers/refresh.sh` row to `→ src/cli/refresh.rs      Phase 4h, ported` and the `lib/helpers/template_manifest.sh` row to `→ src/template_manifest.rs   hash (4e); load, lookup, remove, write (4g); record and heal (4h)`. Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [ ] **Step 3: Verify (outside the agent sandbox)**

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in refresh native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `243 passed`, `0`, `11`, `1`; lint exit 0; both lines `bash=0 native=0` (`refresh` 38 cases, `native_parity` 54).

- [ ] **Step 4: Commit**

```bash
git add docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-15-rust-migration-phase-4h-refresh.md
git commit -m "docs(native): map the phase 4h modules and quirks"
```

---

## Completion

The plan is closed when every box is ticked, `tests/refresh.bats` and `tests/native_parity.bats` are green under both engines, the two parity fixtures pass, and a `## Completion receipt` records the fresh verification. The next plan is 4i, `init`, the last of the `template_manifest` family.

## Run log

### 2026-09-15 — Phase 4h planned
- Commits: this plan.
- Verified: the reference turned up no Bash bug; every branch of `cmd_refresh` was captured with `refresh_reference.sh` (59 scenarios) on a plain `init` seed. The Rust in Tasks 1–2 was drafted in the tree and removed after verification: `cargo test` 243/0/11/1, fmt and clippy clean; with `refresh` in `_NATIVE_COMMANDS`, `AGENTSYNC_NATIVE=1 bats tests/refresh.bats` 38/38 and the two parity fixtures passed, and failed on an `(auto-updated; untouched)` mutation; `refresh_reference.sh` gave 3658-line transcripts for both engines identical apart from 17 `.ai/.template-manifest` mode lines (Bash `0600` through BSD `sort -o`, native `0644` kept), with the restored `strip-ai-chars.sh` at `755` in both; `refresh_tty.sh` on a pty, outside the sandbox, gave identical 600-line transcripts for view, restore, unknown reply, skip, diff, update, quit, and colours, apart from 3 of the same mode lines. `diff -` on stdin is refused inside the sandbox, so `_refresh_show_diff` stages the template in a temporary file. Baseline `cargo test` 232/0/11/1; `refresh.bats` 38 and `native_parity.bats` 52 cases green in Bash.
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.
