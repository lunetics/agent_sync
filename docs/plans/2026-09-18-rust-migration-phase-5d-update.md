# Rust Migration Phase 5d: `update` and the Update Notice

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Give the binary the `update` command for a binary install and the terminal notice `check_for_updates` prints, so that from the 5e cutover a user who installed the binary can update it, pin it with `update <version>`, see `--strict` conflicts between the catalog embedded in the running binary and the one embedded in the new binary, read the new release's changelog, and see the format notice and the update banner once per run whichever engine answers. This is the fourth of the five Phase 5 slices; 5e links the binary the installers place and moves clone installs to it.

**Architecture:** `src/cli/update.rs` rewrites `cmd_update` for a binary install: the tag comes from the pin or from `releases/latest`, `agentsync-<target>.tar.xz` (`.zip` on Windows) and its `.sha256` are fetched through `curl -sL -w %{http_code}`, the sum is checked in-process with `manifest::sha256_hex`, `tar -xf` unpacks the archive, the new binary is asked for `version` and for `__catalog` (a hidden command every binary answers with its embedded `lib/templates/tools/*.yaml` framed by byte length), `snapshot::diff` and `snapshot::find_conflicts` compare the two catalogs against the project's `.ai/src/tools/*.yaml`, the staged copy is renamed over the running binary, and the report prints the Bash texts: `Updated!`, the changelog sections of the archive's `CHANGELOG.md` between the two versions (`src/changelog.rs`: `_md_plain`, `fold -s`, `_show_changelog_sections`, `sort -V`), the conflict report, the queue in `.ai/.pending-resolutions.yaml`, and the legacy-layout banner. `src/cli/notice.rs` is `check_for_updates`: the format notice from `format_rev::pending_notes` and the banner from `.update_cache` beside the install's `bin/`, refreshed by a detached `agentsync __update-cache <file>` the binary spawns on itself. `main.rs` prints the notice before the commands `bin/agentsync.sh` lists, and the dispatcher's `check_for_updates` runs only when `_native_will_serve` says the binary will not answer, so a terminal sees each notice once. The three process seams (`fetch`, `extract`, `ask`) are closures in `update::Env`, so the whole flow is unit-tested against a fake GitHub in temporary directories, and `tests/update_native.bats` runs the real binary (a copy, never the build under `target/`) with real `tar` and a `curl` stand-in on `PATH`.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new crate. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 5" and its decisions 1 to 4. Previous plan: `docs/plans/2026-09-18-rust-migration-phase-5c-release-build.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `update` writes the binary it runs as (through a staged sibling `.agentsync.new`), removes `.update_cache` beside the install's `bin/`, and writes `.ai/.pending-resolutions.yaml` in the project when a conflict exists and `.ai/` does; the notice writes `.update_cache` alone, from the detached refresh. Nothing else.
- No binary ships to users until 5e; without a binary every command runs in Bash. Bash changes: `bin/agentsync.sh` gains `_native_will_serve` and guards `check_for_updates` with it; `_NATIVE_COMMANDS` does not change, so a checkout keeps Bash's git-based `update` until Phase 6; `lib/**/*.sh` and `install.sh` are untouched, and every shell entry point stays clean under `shellcheck -x -S warning -e SC1091`.
- The `update` mechanics change (design spec, "Phase 5"), so there is no Bash reference for the flow and no parity fixture: the Bash texts that survive (the usage, `Already up to date!`, `Updated!`, the changelog rendering, the conflict report, the queue file, the `Queued in` line, the legacy-layout banner, the format notice, the banner box) are asserted by unit tests against the Bash strings, and the terminal-only notice is checked on a pty by `notice_tty.sh` once per engine. Every existing bats file stays green under both engines; `tests/native_parity.bats` does not change.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency; `Cargo.lock` unchanged. `main.rs` alone reads the environment and terminal state. The binary spawns `curl`, `tar`, `tput`, the downloaded binary, and itself; no HTTP crate (design decision 1).
- `__catalog` and `__update-cache` are hidden commands: the binary's contract with a newer binary and with itself. No dispatcher lists them, `print_usage` does not name them, and `parse_catalog_dump` refuses anything that is not a dump.
- Every expected value was captured on 2026-09-18 from the draft on macOS 26.5 arm64 (`aarch64-apple-darwin`): the sha256 of every file the plan writes, the `cargo test` counts (312 after Task 1, 326 after Task 2), the bats counts, the pty harness lines, and `TOTAL bash=0 native=0` over 51 bats files.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

1. **`update` is not added to `_NATIVE_COMMANDS`.** Through the dispatcher a checkout keeps Bash's git-based `update` (fetch, reconcile, tag checkout, relink), which `tests/install.bats` and `tests/update.bats` grade and which design decision 2 needs until 5e moves clone installs to the binary; the binary's `update` is for a binary install, where there is no dispatcher, and `tests/update_native.bats` runs it directly, as `assert_entry_parity` runs the binary for `help`. Alternative: list it and teach the binary the git flow when `AGENTSYNC_ENGINE_VERSION` is set; rejected as a port of code Phase 6 deletes.
2. **The new catalog through `__catalog`, the changelog from the archive.** The spec asks for conflicts "from the catalogs embedded in the old and the new binary": the running binary runs the downloaded one with `__catalog` and diffs the answer against its own `catalog::base_tool_yaml`, one reader (`yaml_subset::value`) on both sides as Bash used one `parse_yaml_value`. The changelog is `CHANGELOG.md` from the archive, which 5c ships beside the binary (`[misc] CHANGELOG.md, LICENSE, README.md`), so no `__changelog` command is needed. The dump frames each YAML as `<slug> <bytes>\n<yaml>\n`, unambiguous whatever the YAML holds.
3. **The notice moves into the binary now, once per run.** From 5e the binary is the entry point and a binary install has no dispatcher, so the notice has to live in the binary; the dispatcher keeps printing it for the commands it serves itself (`_native_will_serve` is false: `AGENTSYNC_NATIVE=0`, an unlisted command, or no binary). The cache is `.update_cache` beside the install's `bin/` (`~/.agentsync/.update_cache` for an installer install, `target/.update_cache` for a developer build), refreshed by `agentsync __update-cache <file>` spawned detached on the binary itself, which runs `curl -sfL --max-time 5` on `releases/latest` and writes the `tag_name`; Bash read the newest tag, but only a release carries a binary. Alternative: keep the notice in Bash until Phase 6; rejected, the 5e cutover would ship a binary that never tells anyone an update exists.
4. **HTTP status over `curl -f`.** `curl -sL --max-time 30 -o <file> -w %{http_code}` answers the status on stdout, so a pinned tag's 404 is told apart from an unreachable host, and a 404 on the archive of a pinned tag is followed by one probe of `git/ref/tags/<tag>`: 200 means the tag exists without a binary release, so the refusal prints the installer command that pins it (design decision 3); anything else means no such release. No "first binary version" constant exists to get wrong once 5e picks the version.
5. **Seams as closures, real processes in `main`.** `update::Env` carries `fetch`, `extract`, and `ask`; `main.rs` passes `curl_fetch`, `tar_extract`, and `ask_binary`. The unit tests cover every branch with a fake GitHub (a `BTreeMap` of URLs) and a fake archive; `tests/update_native.bats` covers the real `tar`, the real binary, and the `curl` stand-in end to end, eleven cases. Alternative: a trait; rejected, three functions do not need one.
6. **Five accepted deviations and one quirk, recorded in Task 3.** The binary has no git reconcile, autostash line, or relink warning, and refuses an unknown tag, a pre-binary tag, a checksum mismatch, and an archive whose binary does not answer, each with status 1 and the old binary kept; `update --help` says "the latest release" where Bash said "the latest main". A conflict with an empty base value keeps its five columns, where Bash's tab-separated `read` collapsed the field. The changelog wraps by character where `fold` counted bytes or columns. The banner's cache moves beside `bin/` and reads the latest release. Quirk 55: the changelog matches `## <version>` by prefix and appends a later matching section; reproduced with `a_heading_matches_by_prefix_like_bash_does`, confirmed against Bash (`_show_changelog_sections` on `## 9.9.90` and `## 9.9.9` renders both).
7. **Windows is written, not run.** `replace_binary` renames the running `agentsync.exe` to `agentsync.exe.old` before the rename, `tar -xf` opens the `.zip` with the `tar` Windows 10 ships, and the target is `x86_64-pc-windows-msvc`; native bats runs stay off Windows until 5e, so the receipt lists it as deferred.
8. **Task order:** the core modules (Task 1), the command, the notice, the dispatcher guard, and the bats file (Task 2), the docs (Task 3). **Recommended:** as listed.

## Module closure

```text
lib/helpers/update.sh            4        AGENTSYNC_REPO
                                 62-228   cmd_update: args 63-91, the install dir 93-99 (no counterpart), fetch 112-124, already up to date 138-142, the snapshot 144-149, the swap 151-177, the cache 186, Updated! 188-193, the changelog 200, conflicts 202-219, the banner 223, --strict 225-227
                                 232-256  _show_migration_banner
                                 260-286  _show_update_conflicts
                                 291-329  _show_changelog_range
                                 333-338  _md_plain
                                 343-355  _changelog_width
                                 359-380  _print_wrapped
                                 382-422  _show_changelog_sections
                                 424-434  _bg_fetch_latest_version
                                 436-456  _check_project_format
                                 458-499  check_for_updates
lib/helpers/snapshot.sh          17-46    _snapshot_keys (26 keys)
                                 104-137  snapshot_diff
                                 148-166  snapshot_find_conflicts
                                 176-181  _snapshot_yaml_quote
                                 189-227  snapshot_write_pending_resolutions
lib/helpers/format.sh            38-52    format_pending_notes
                                 55-62    format_config_path
lib/helpers/resolve.sh           31-43    resolve_install_dir (no counterpart: the binary is the install)
bin/agentsync.sh                 280      _NATIVE_COMMANDS (unchanged)
                                 282-300  _native_bin, which _native_will_serve calls
                                 331-337  the notice list main runs check_for_updates for
tests/update_snapshot.bats       1-268    the values the snapshot tests assert on
tests/changelog_render.bats      1-135    the values the changelog tests assert on
```

Reused: `catalog::base_tools`, `catalog::base_tool_yaml`, `manifest::sha256_hex`, `staging::write_beside`, `yaml_subset::value`, `format_rev::{engine, project}`, `style::Style`, `cli::customize::put`, `cli::bundle::Scratch` (made `pub(crate)` with a prefix), `paths::logical_root`, `engine_version`.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
ls src/changelog.rs src/cli/notice.rs src/cli/update.rs tests/update_native.bats 2>&1 | grep -c 'No such file'
grep -c '_native_will_serve' bin/agentsync.sh
grep -c '^@test' tests/native_parity.bats tests/update_snapshot.bats tests/changelog_render.bats
ls tests/*.bats | wc -l
```

Expected: the plan's latest commit; `298 passed`, `0 passed`, `11 passed`, `1 passed`; `4`; `0`; `70`, `20`, `13`; `50`.

---

### Task 1: The changelog renderer, the catalog diff, and the format notes

**Files:**
- Create: `src/changelog.rs`
- Modify: `src/snapshot.rs`, `src/format_rev.rs`, `src/lib.rs`

**Interfaces:**

```rust
// src/changelog.rs
pub fn md_plain(text: &str) -> String;
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering;
pub fn clamp_width(cols: Option<&str>) -> usize;
pub fn wrap(text: &str, first: &str, cont: &str, width: usize) -> String;
pub fn sections(changelog: &str, versions: &[String], width: usize, style: &Style) -> String;
pub fn versions_in_range(changelog: &str, old: &str, new: &str) -> Vec<String>;
// src/snapshot.rs
pub const KEYS: [&str; 26];
pub struct Change { pub tool: String, pub field: String, pub before: String, pub after: String }
pub struct Conflict { pub tool: String, pub field: String, pub before: String, pub after: String, pub yours: String }
pub fn diff(old: &[(String, String)], new: &[(String, String)]) -> Vec<Change>;
pub fn find_conflicts(root: &Path, changes: &[Change]) -> Vec<Conflict>;
pub fn pending_resolutions(today: &str, from: &str, to: &str, conflicts: &[Conflict]) -> String;
pub fn write_pending_resolutions(root: &Path, today: &str, from: &str, to: &str, conflicts: &[Conflict]) -> Result<(), Error>;
pub fn utc_date(secs: u64) -> String;
// src/format_rev.rs
pub fn pending_notes(from: u32, to: u32) -> Vec<String>;
pub fn config_path(project_dir: &Path) -> Option<PathBuf>;
```

- [x] **Step 1: Write `src/changelog.rs`**

Write the file with exactly this content:

<!-- file: src/changelog.rs -->
````rust
//! The changelog renderer of `lib/helpers/update.sh`: `_md_plain`,
//! `_print_wrapped` over `fold -s`, `_show_changelog_sections`, the range
//! `_show_changelog_range` selects, and the `sort -V` order they rely on.

use std::cmp::Ordering;

use crate::style::Style;

/// `_md_plain`: `**` and backticks removed.
pub fn md_plain(text: &str) -> String {
    text.replace("**", "").replace('`', "")
}

/// `sort -V` on two versions: digit runs compare as numbers, other runs as
/// bytes, and a version that is a prefix of the other sorts first.
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    let mut left = runs(a).into_iter();
    let mut right = runs(b).into_iter();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some((true, x)), Some((true, y))) => {
                let x = x.trim_start_matches('0');
                let y = y.trim_start_matches('0');
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some((_, x)), Some((_, y))) => {
                let order = x.cmp(y);
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

fn runs(version: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = version.as_bytes();
    while start < bytes.len() {
        let digits = bytes[start].is_ascii_digit();
        let mut end = start;
        while end < bytes.len() && bytes[end].is_ascii_digit() == digits {
            end += 1;
        }
        out.push((digits, &version[start..end]));
        start = end;
    }
    out
}

/// `_changelog_width`: `tput cols` range-checked, 80 when unusable, 100 at most.
pub fn clamp_width(cols: Option<&str>) -> usize {
    let cols = cols.unwrap_or("");
    if cols.is_empty() || !cols.bytes().all(|b| b.is_ascii_digit()) {
        return 80;
    }
    match cols.parse::<usize>() {
        Ok(n) if n < 40 => 80,
        Ok(n) if n > 100 => 100,
        Ok(n) => n,
        Err(_) => 80,
    }
}

/// `fold -s -w <width>`: a line longer than `width` characters breaks after
/// its last space within the width, or at the width when it has none.
fn fold_spaces(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut column = 0usize;
    for c in text.chars() {
        if c == '\n' {
            lines.push(std::mem::take(&mut line));
            column = 0;
            continue;
        }
        if column >= width {
            match line.rfind(' ') {
                Some(at) => {
                    let rest = line.split_off(at + 1);
                    lines.push(std::mem::replace(&mut line, rest));
                    column = line.chars().count();
                }
                None => {
                    lines.push(std::mem::take(&mut line));
                    column = 0;
                }
            }
        }
        line.push(c);
        column += 1;
    }
    lines.push(line);
    lines
}

/// `_print_wrapped`: `text` folded to `width` less the continuation prefix
/// (24 columns at least), trailing whitespace dropped from every piece, the
/// first piece behind `first` and the others behind `cont`. `$(...)` drops
/// the trailing newlines `fold` prints, so an empty text is one empty line.
pub fn wrap(text: &str, first: &str, cont: &str, width: usize) -> String {
    let avail = width.saturating_sub(cont.chars().count()).max(24);
    let mut pieces = fold_spaces(&format!("{text}\n"), avail);
    while pieces.len() > 1 && pieces.last().is_some_and(String::is_empty) {
        pieces.pop();
    }
    let mut out = String::new();
    for (index, piece) in pieces.iter().enumerate() {
        let piece = piece.trim_end_matches(|c: char| c.is_ascii_whitespace());
        let prefix = if index == 0 { first } else { cont };
        out.push_str(prefix);
        out.push_str(piece);
        out.push('\n');
    }
    out
}

/// The lines `while read` yields: every line ended by a newline.
fn complete_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split_inclusive('\n')
        .filter_map(|line| line.strip_suffix('\n'))
}

/// `_show_changelog_sections`: each version's section, a `## <version>`
/// heading matched by prefix as Bash matches it, `### ` as a bold heading,
/// `- ` as a wrapped bullet, other text wrapped as a paragraph.
pub fn sections(changelog: &str, versions: &[String], width: usize, style: &Style) -> String {
    let mut out = String::new();
    for version in versions {
        if version.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "\n  {}\n\n",
            style.cyan(&format!("What's new in v{version}:"))
        ));
        let heading = format!("## {version}");
        let mut in_section = false;
        let mut started = false;
        for line in complete_lines(changelog) {
            if line.starts_with(&heading) {
                in_section = true;
                continue;
            }
            if !in_section {
                continue;
            }
            if line.starts_with("## ") {
                break;
            }
            if line.is_empty() && !started {
                continue;
            }
            started = true;
            if let Some(rest) = line.strip_prefix("### ") {
                out.push_str(&format!("\n  {}\n", style.bold(&md_plain(rest))));
            } else if let Some(rest) = line.strip_prefix("- ") {
                out.push_str(&wrap(
                    &md_plain(rest),
                    &format!("    {} ", style.dim("•")),
                    "      ",
                    width,
                ));
            } else if !line.is_empty() {
                out.push_str(&wrap(&md_plain(line), "    ", "    ", width));
            }
        }
    }
    out
}

/// `_show_changelog_range`'s selection: the first word of every `## ` heading
/// with `old < version <= new` in `sort -V` order, ascending.
pub fn versions_in_range(changelog: &str, old: &str, new: &str) -> Vec<String> {
    let mut found: Vec<String> = complete_lines(changelog)
        .filter_map(|line| line.strip_prefix("## "))
        .map(|rest| rest.split(' ').next().unwrap_or("").to_string())
        .filter(|v| {
            version_cmp(old, v) == Ordering::Less && version_cmp(v, new) != Ordering::Greater
        })
        .collect();
    found.sort_by(|a, b| version_cmp(a, b));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "# Changelog\n\n## 9.9.9\n\nA summary line that is deliberately long enough to need wrapping when it is printed into a terminal of ordinary width, several times over, so the wrap is unmistakable.\n\n### Internal\n\n- **A bold lead-in.** Body text mentioning `code_span` and a second `span`, padded out so this bullet is certainly longer than any sensible terminal width and must wrap onto continuation lines.\n- A short bullet.\n\n## 9.9.8\n\n- Older entry that must not appear.\n";

    #[test]
    fn markdown_markers_are_stripped_like_md_plain() {
        assert_eq!(md_plain("**Bold.** rest"), "Bold. rest");
        assert_eq!(
            md_plain("run `agentsync sync` now"),
            "run agentsync sync now"
        );
        assert_eq!(
            md_plain("plain text — with an em dash"),
            "plain text — with an em dash"
        );
    }

    #[test]
    fn versions_order_like_sort_v() {
        assert_eq!(version_cmp("0.9.0", "0.10.0"), Ordering::Less);
        assert_eq!(version_cmp("0.35.2", "0.36.0"), Ordering::Less);
        assert_eq!(version_cmp("0.36.0", "0.36.0"), Ordering::Equal);
        assert_eq!(version_cmp("1.0.0", "0.99.99"), Ordering::Greater);
        assert_eq!(version_cmp("0.36", "0.36.0"), Ordering::Less);
        assert_eq!(version_cmp("0.0.0-dev", "0.36.0"), Ordering::Less);
        assert_eq!(version_cmp("0.36.0-rc1", "0.36.0"), Ordering::Greater);
    }

    #[test]
    fn the_width_is_clamped_like_changelog_width() {
        assert_eq!(clamp_width(None), 80);
        assert_eq!(clamp_width(Some("")), 80);
        assert_eq!(clamp_width(Some("x")), 80);
        assert_eq!(clamp_width(Some("39")), 80);
        assert_eq!(clamp_width(Some("40")), 40);
        assert_eq!(clamp_width(Some("72")), 72);
        assert_eq!(clamp_width(Some("300")), 100);
    }

    #[test]
    fn wrapping_keeps_every_line_within_the_width_and_indents_continuations() {
        let text = "word ".repeat(80);
        let out = wrap(&text, "    • ", "      ", 72);
        assert!(out.lines().all(|line| line.chars().count() <= 72));
        let out = wrap(&"word ".repeat(40), "    * ", "      ", 60);
        let mut lines = out.lines();
        assert!(lines.next().unwrap().starts_with("    * word"));
        assert!(lines.next().unwrap().starts_with("      word"));
        assert_eq!(wrap("brief", "    • ", "      ", 80), "    • brief\n");
        let out = wrap("alpha beta gamma delta", "  ", "  ", 1);
        assert!(out.contains("alpha"));
        assert_eq!(wrap("", "    ", "    ", 80), "    \n");
    }

    #[test]
    fn a_word_longer_than_the_width_breaks_at_the_width_like_fold() {
        let text = "a".repeat(30);
        assert_eq!(fold_spaces(&text, 24), ["a".repeat(24), "a".repeat(6)]);
        assert_eq!(fold_spaces("ab cd ef", 5), ["ab ", "cd ef"]);
        assert_eq!(fold_spaces("ab cd\n", 5), ["ab cd", ""]);
    }

    #[test]
    fn only_the_requested_section_is_rendered_without_markers() {
        let out = sections(FIXTURE, &["9.9.9".to_string()], 80, &Style::plain());
        assert!(out.starts_with("\n  What's new in v9.9.9:\n\n"));
        assert!(out.contains("\n  Internal\n"));
        assert!(out.contains("    • A short bullet.\n"));
        assert!(out.contains("A bold lead-in. Body text mentioning code_span"));
        assert!(!out.contains("must not appear"));
        assert!(!out.contains("**"));
        assert!(!out.contains('`'));
        assert!(out.lines().all(|line| line.chars().count() <= 80));
        let coloured = sections(FIXTURE, &["9.9.9".to_string()], 80, &Style::colored());
        assert!(coloured.contains("\x1b[36mWhat's new in v9.9.9:\x1b[0m"));
        assert!(coloured.contains("    \x1b[2m•\x1b[0m A short bullet.\n"));
    }

    #[test]
    fn a_heading_matches_by_prefix_like_bash_does() {
        // Known quirk 55.
        let out = sections(
            "## 9.9.90\n\n- Ninety.\n\n## 9.9.9\n\n- Nine.\n",
            &["9.9.9".to_string()],
            80,
            &Style::plain(),
        );
        assert_eq!(
            out,
            "\n  What's new in v9.9.9:\n\n    • Ninety.\n    • Nine.\n"
        );
    }

    #[test]
    fn the_range_excludes_the_old_version_and_includes_the_new_one() {
        let changelog = "## 0.36.0\n\n## 0.35.2 (hotfix)\n\n## 0.35.10\n\n## 0.35.1\n\n## 0.34.0\n";
        assert_eq!(
            versions_in_range(changelog, "0.35.1", "0.36.0"),
            ["0.35.2", "0.35.10", "0.36.0"]
        );
        assert!(versions_in_range(changelog, "0.36.0", "0.36.0").is_empty());
        assert!(versions_in_range(changelog, "0.36.0", "0.35.1").is_empty());
        assert_eq!(
            versions_in_range("## 0.36.0", "0.35.0", "0.36.0"),
            Vec::<String>::new()
        );
    }
}
````

- [x] **Step 2: Apply the core-module patch**

Save the block below as `$TMPDIR/task1.diff` and run `git apply "$TMPDIR/task1.diff"`. It adds the `KEYS`, `Change`, `Conflict`, `diff`, `find_conflicts`, `yaml_quote`, `pending_resolutions`, `write_pending_resolutions`, and `utc_date` to `src/snapshot.rs` with their tests, `pending_notes` and `config_path` to `src/format_rev.rs` with theirs, and `pub mod changelog;` to `src/lib.rs`.

<!-- file: task1.diff -->
````diff
diff --git a/src/snapshot.rs b/src/snapshot.rs
index e1e00d6..54c1c65 100644
--- a/src/snapshot.rs
+++ b/src/snapshot.rs
@@ -1,10 +1,198 @@
-//! The pending-resolutions readers of `lib/helpers/snapshot.sh` that `resolve`
-//! uses. Saving and diffing the catalog belong to `update`.
+//! `lib/helpers/snapshot.sh`: the catalog diff and the conflict queue
+//! `update` writes, and the pending-resolutions readers `resolve` uses.
 
 use std::path::Path;
 
+use crate::Error;
+use crate::yaml_subset;
+
 const PENDING: &str = ".ai/.pending-resolutions.yaml";
 
+/// `_snapshot_keys`, in order.
+pub const KEYS: [&str; 26] = [
+    "name",
+    "enabled",
+    "targets.agents.dest",
+    "targets.rules.dest",
+    "targets.rules.extension",
+    "targets.rules.header",
+    "targets.rules.scoped_header",
+    "targets.rules.append_imports",
+    "targets.rules.merge_to_file",
+    "targets.rules.inline_into_agents",
+    "targets.rules.prepend_agents",
+    "targets.skills.dest",
+    "targets.skills.inline_into_agents",
+    "targets.commands.dest",
+    "targets.commands.format",
+    "targets.commands.as_skills",
+    "targets.commands.inline_into_agents",
+    "targets.subagents.dest",
+    "targets.subagents.format",
+    "targets.settings.source",
+    "targets.settings.dest",
+    "targets.mcp.source",
+    "targets.mcp.dest",
+    "targets.hooks.source",
+    "targets.hooks.dest",
+    "post_sync",
+];
+
+/// One `snapshot_diff` line: a key whose value differs between the catalogs.
+#[derive(Clone, Debug, PartialEq, Eq)]
+pub struct Change {
+    pub tool: String,
+    pub field: String,
+    pub before: String,
+    pub after: String,
+}
+
+/// One `snapshot_find_conflicts` line: a change on a field the project overrides.
+#[derive(Clone, Debug, PartialEq, Eq)]
+pub struct Conflict {
+    pub tool: String,
+    pub field: String,
+    pub before: String,
+    pub after: String,
+    pub yours: String,
+}
+
+/// `snapshot_diff` over two catalogs given as `(slug, yaml)`: the changed
+/// keys, tools in byte order, keys in [`KEYS`] order. A tool in one catalog
+/// only reads as empty on the other side.
+pub fn diff(old: &[(String, String)], new: &[(String, String)]) -> Vec<Change> {
+    let mut tools: Vec<&str> = old
+        .iter()
+        .chain(new)
+        .map(|(slug, _)| slug.as_str())
+        .collect();
+    tools.sort_unstable();
+    tools.dedup();
+    let text_of = |catalog: &[(String, String)], slug: &str| -> Option<String> {
+        catalog
+            .iter()
+            .find(|(name, _)| name == slug)
+            .map(|(_, yaml)| yaml.clone())
+    };
+    let mut changes = Vec::new();
+    for tool in tools {
+        let old_text = text_of(old, tool);
+        let new_text = text_of(new, tool);
+        for key in KEYS {
+            let before = old_text
+                .as_deref()
+                .map(|text| yaml_subset::value(text, key))
+                .unwrap_or_default();
+            let after = new_text
+                .as_deref()
+                .map(|text| yaml_subset::value(text, key))
+                .unwrap_or_default();
+            if before != after {
+                changes.push(Change {
+                    tool: tool.to_string(),
+                    field: key.to_string(),
+                    before,
+                    after,
+                });
+            }
+        }
+    }
+    changes
+}
+
+/// `snapshot_find_conflicts`: the changes whose field the project overrides
+/// with a non-empty value in `.ai/src/tools/<tool>.yaml`.
+pub fn find_conflicts(root: &Path, changes: &[Change]) -> Vec<Conflict> {
+    let tools_dir = root.join(".ai/src/tools");
+    if !tools_dir.is_dir() {
+        return Vec::new();
+    }
+    changes
+        .iter()
+        .filter_map(|change| {
+            let file = tools_dir.join(format!("{}.yaml", change.tool));
+            let text = std::fs::read(file).ok()?;
+            let yours = yaml_subset::value(&String::from_utf8_lossy(&text), &change.field);
+            (!yours.is_empty()).then(|| Conflict {
+                tool: change.tool.clone(),
+                field: change.field.clone(),
+                before: change.before.clone(),
+                after: change.after.clone(),
+                yours,
+            })
+        })
+        .collect()
+}
+
+/// `_snapshot_yaml_quote`: a double-quoted scalar with backslashes, quotes,
+/// tabs, and newlines escaped.
+fn yaml_quote(s: &str) -> String {
+    let escaped = s
+        .replace('\\', "\\\\")
+        .replace('"', "\\\"")
+        .replace('\t', "\\t")
+        .replace('\n', "\\n");
+    format!("\"{escaped}\"")
+}
+
+/// The text `snapshot_write_pending_resolutions` writes.
+pub fn pending_resolutions(today: &str, from: &str, to: &str, conflicts: &[Conflict]) -> String {
+    let mut out = format!(
+        "# AgentSync — pending upstream resolutions from `agentsync update`.\n# Run `agentsync resolve` to walk these fields interactively.\n# Remove this file once you've reviewed every entry.\n\nschema: 1\ngenerated_on: \"{today}\"\nfrom_version: \"{from}\"\nto_version: \"{to}\"\nconflicts:\n"
+    );
+    for conflict in conflicts {
+        out.push_str(&format!(
+            "  - tool: \"{}\"\n    field: \"{}\"\n    base_before: {}\n    base_after: {}\n    your_override: {}\n",
+            conflict.tool,
+            conflict.field,
+            yaml_quote(&conflict.before),
+            yaml_quote(&conflict.after),
+            yaml_quote(&conflict.yours)
+        ));
+    }
+    if conflicts.is_empty() {
+        out.push_str("  []\n");
+    }
+    out
+}
+
+/// `snapshot_write_pending_resolutions`: the queue written beside its
+/// destination, nothing when `.ai/` is missing.
+pub fn write_pending_resolutions(
+    root: &Path,
+    today: &str,
+    from: &str,
+    to: &str,
+    conflicts: &[Conflict],
+) -> Result<(), Error> {
+    if !root.join(".ai").is_dir() {
+        return Ok(());
+    }
+    crate::staging::write_beside(
+        &root.join(PENDING),
+        pending_resolutions(today, from, to, conflicts).as_bytes(),
+    )
+}
+
+/// `date -u +%Y-%m-%d` for a time in seconds since the Unix epoch.
+pub fn utc_date(secs: u64) -> String {
+    let days = (secs / 86_400) as i64 + 719_468;
+    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
+    let day_of_era = days - era * 146_097;
+    let year_of_era =
+        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
+    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
+    let shifted_month = (5 * day_of_year + 2) / 153;
+    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
+    let month = if shifted_month < 10 {
+        shifted_month + 3
+    } else {
+        shifted_month - 9
+    };
+    let year = year_of_era + era * 400 + i64::from(month <= 2);
+    format!("{year:04}-{month:02}-{day:02}")
+}
+
 fn after_label(stripped: &str, label: &str) -> String {
     let value = stripped[label.len()..].trim_start_matches(|c: char| c.is_ascii_whitespace());
     let value = value.strip_suffix('"').unwrap_or(value);
@@ -60,6 +248,130 @@ pub fn clear_pending(root: &Path) {
 mod tests {
     use super::*;
 
+    fn base_tool(name: &str, rules_dest: &str) -> (String, String) {
+        (
+            name.to_string(),
+            format!(
+                "name: {name}\nenabled: true\ntargets:\n  rules:\n    dest: \"{rules_dest}\"\n    extension: .md\n"
+            ),
+        )
+    }
+
+    #[test]
+    fn the_diff_lists_changed_keys_and_added_tools_like_snapshot_diff() {
+        let old = [base_tool("claude", ".claude/rules")];
+        assert!(diff(&old, &old).is_empty());
+        let new = [base_tool("claude", ".claude/rules-v2")];
+        assert_eq!(
+            diff(&old, &new),
+            [Change {
+                tool: "claude".to_string(),
+                field: "targets.rules.dest".to_string(),
+                before: ".claude/rules".to_string(),
+                after: ".claude/rules-v2".to_string(),
+            }]
+        );
+        let added = [
+            base_tool("claude", ".claude/rules"),
+            base_tool("cursor", ".cursor/rules"),
+        ];
+        let changes = diff(&old, &added);
+        assert_eq!(
+            changes
+                .iter()
+                .map(|c| (
+                    c.tool.as_str(),
+                    c.field.as_str(),
+                    c.before.as_str(),
+                    c.after.as_str()
+                ))
+                .collect::<Vec<_>>(),
+            [
+                ("cursor", "name", "", "cursor"),
+                ("cursor", "enabled", "", "true"),
+                ("cursor", "targets.rules.dest", "", ".cursor/rules"),
+                ("cursor", "targets.rules.extension", "", ".md"),
+            ]
+        );
+    }
+
+    #[test]
+    fn conflicts_need_a_non_empty_override_on_the_changed_field() {
+        let dir = tempfile::tempdir().unwrap();
+        let changes = diff(
+            &[base_tool("claude", ".claude/rules")],
+            &[base_tool("claude", ".claude/rules-v2")],
+        );
+        assert!(find_conflicts(dir.path(), &changes).is_empty());
+        std::fs::create_dir_all(dir.path().join(".ai/src/tools")).unwrap();
+        std::fs::write(
+            dir.path().join(".ai/src/tools/claude.yaml"),
+            "targets:\n  rules:\n    extension: .mdc\n",
+        )
+        .unwrap();
+        assert!(find_conflicts(dir.path(), &changes).is_empty());
+        std::fs::write(
+            dir.path().join(".ai/src/tools/claude.yaml"),
+            "targets:\n  rules:\n    dest: \".claude/my-rules\"\n",
+        )
+        .unwrap();
+        assert_eq!(
+            find_conflicts(dir.path(), &changes),
+            [Conflict {
+                tool: "claude".to_string(),
+                field: "targets.rules.dest".to_string(),
+                before: ".claude/rules".to_string(),
+                after: ".claude/rules-v2".to_string(),
+                yours: ".claude/my-rules".to_string(),
+            }]
+        );
+    }
+
+    #[test]
+    fn the_queue_is_the_yaml_snapshot_write_pending_resolutions_writes() {
+        let conflicts = [
+            Conflict {
+                tool: "claude".to_string(),
+                field: "targets.rules.dest".to_string(),
+                before: ".claude/rules".to_string(),
+                after: ".claude/rules-v2".to_string(),
+                yours: ".claude/my-rules".to_string(),
+            },
+            Conflict {
+                tool: "claude".to_string(),
+                field: "targets.rules.header".to_string(),
+                before: "one".to_string(),
+                after: "quoted \"hi\"".to_string(),
+                yours: "back\\slash\tand\nmore".to_string(),
+            },
+        ];
+        assert_eq!(
+            pending_resolutions("2026-09-18", "0.7.0", "0.8.0", &conflicts),
+            "# AgentSync — pending upstream resolutions from `agentsync update`.\n# Run `agentsync resolve` to walk these fields interactively.\n# Remove this file once you've reviewed every entry.\n\nschema: 1\ngenerated_on: \"2026-09-18\"\nfrom_version: \"0.7.0\"\nto_version: \"0.8.0\"\nconflicts:\n  - tool: \"claude\"\n    field: \"targets.rules.dest\"\n    base_before: \".claude/rules\"\n    base_after: \".claude/rules-v2\"\n    your_override: \".claude/my-rules\"\n  - tool: \"claude\"\n    field: \"targets.rules.header\"\n    base_before: \"one\"\n    base_after: \"quoted \\\"hi\\\"\"\n    your_override: \"back\\\\slash\\tand\\nmore\"\n"
+        );
+        assert!(pending_resolutions("2026-09-18", "a", "b", &[]).ends_with("conflicts:\n  []\n"));
+        let dir = tempfile::tempdir().unwrap();
+        write_pending_resolutions(dir.path(), "2026-09-18", "a", "b", &conflicts).unwrap();
+        assert!(!dir.path().join(PENDING).exists());
+        std::fs::create_dir(dir.path().join(".ai")).unwrap();
+        write_pending_resolutions(dir.path(), "2026-09-18", "a", "b", &conflicts).unwrap();
+        assert_eq!(
+            read_pending_pairs(dir.path()),
+            [
+                ("claude".to_string(), "targets.rules.dest".to_string()),
+                ("claude".to_string(), "targets.rules.header".to_string()),
+            ]
+        );
+    }
+
+    #[test]
+    fn dates_read_as_date_u_prints_them() {
+        assert_eq!(utc_date(0), "1970-01-01");
+        assert_eq!(utc_date(1_758_153_600), "2025-09-18");
+        assert_eq!(utc_date(951_782_400), "2000-02-29");
+        assert_eq!(utc_date(4_107_542_399), "2100-02-28");
+    }
+
     #[test]
     fn pending_pairs_are_read_from_the_conflicts_list_and_cleared() {
         let dir = tempfile::tempdir().unwrap();
diff --git a/src/format_rev.rs b/src/format_rev.rs
index 0ba342d..c8d25b4 100644
--- a/src/format_rev.rs
+++ b/src/format_rev.rs
@@ -1,6 +1,8 @@
 //! `lib/helpers/format.sh`: the project format revision, a counter bumped only
 //! when a project needs a migration step.
 
+use std::path::{Path, PathBuf};
+
 use crate::yaml_subset;
 
 const ENGINE_FORMAT_FILE: &str = include_str!("../FORMAT");
@@ -22,10 +24,58 @@ pub fn project(config: &str) -> u32 {
     revision(&yaml_subset::value(config, "format").replace('"', ""))
 }
 
+/// `format_pending_notes`: one line per migration between the two revisions.
+pub fn pending_notes(from: u32, to: u32) -> Vec<String> {
+    (from.saturating_add(1)..=to)
+        .map(|step| match step {
+            2 => "r2  The agentsync skill is engine-owned now. A copy under .ai/src/skills/agentsync/ shadows it, so engine upgrades never reach your agents.".to_string(),
+            _ => format!("r{step}  See CHANGELOG.md for what changed."),
+        })
+        .collect()
+}
+
+/// `format_config_path`: `.ai/agent_sync.yaml`, else `agent_sync.yaml`, when a file.
+pub fn config_path(project_dir: &Path) -> Option<PathBuf> {
+    [".ai/agent_sync.yaml", "agent_sync.yaml"]
+        .iter()
+        .map(|rel| project_dir.join(rel))
+        .find(|path| path.is_file())
+}
+
 #[cfg(test)]
 mod tests {
     use super::*;
 
+    #[test]
+    fn pending_notes_name_each_migration_step() {
+        assert_eq!(
+            pending_notes(1, 3),
+            [
+                "r2  The agentsync skill is engine-owned now. A copy under .ai/src/skills/agentsync/ shadows it, so engine upgrades never reach your agents.",
+                "r3  See CHANGELOG.md for what changed.",
+            ]
+        );
+        assert!(pending_notes(2, 2).is_empty());
+        assert!(pending_notes(3, 2).is_empty());
+    }
+
+    #[test]
+    fn the_config_path_prefers_the_ai_directory() {
+        let dir = tempfile::tempdir().unwrap();
+        assert_eq!(config_path(dir.path()), None);
+        std::fs::write(dir.path().join("agent_sync.yaml"), "").unwrap();
+        assert_eq!(
+            config_path(dir.path()),
+            Some(dir.path().join("agent_sync.yaml"))
+        );
+        std::fs::create_dir(dir.path().join(".ai")).unwrap();
+        std::fs::write(dir.path().join(".ai/agent_sync.yaml"), "").unwrap();
+        assert_eq!(
+            config_path(dir.path()),
+            Some(dir.path().join(".ai/agent_sync.yaml"))
+        );
+    }
+
     #[test]
     fn revisions_read_as_format_sh_reads_them() {
         assert_eq!(engine(), 2);
diff --git a/src/lib.rs b/src/lib.rs
index 6b09af3..54ffd1f 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -3,6 +3,7 @@
 
 pub mod backup;
 pub mod catalog;
+pub mod changelog;
 pub mod cli;
 pub mod convert;
 pub mod edit_paths;
````

Run: `shasum -a 256 src/changelog.rs src/snapshot.rs src/format_rev.rs src/lib.rs`
Expected:

```text
c053f05d8bfc0ed34bc45a110d6ec5e1649259a09679f579c53f3ebea8e8c9d8  src/changelog.rs
a2eb03e68e2e4be5c4745553ea8cb548471c53e7b9953db50a1cfeeb022d0051  src/snapshot.rs
251d803c3e70c45baa4c892079034153ccb5ef028b75895097783551047c8565  src/format_rev.rs
e86881c68707fb201f52b0e116ec08d2b7b8cb01e115d98c196bb22f7be25fbb  src/lib.rs
```

- [x] **Step 3: The Rust gates**

```bash
cargo fmt --all --check; echo "fmt=$?"
cargo clippy --all-targets -- -D warnings > /dev/null 2>&1; echo "clippy=$?"
cargo test 2>&1 | grep 'test result' | head -4
cargo test changelog 2>&1 | grep 'test result' | head -1
```

Expected: `fmt=0`, `clippy=0`, `312 passed`, `0 passed`, `11 passed`, `1 passed`; the `changelog` filter `9 passed` (the module's 8 plus `release`'s `changelog_section` test). The 14 new tests: 8 in `changelog` (markers, `sort -V`, the width clamp, wrapping, `fold` breaks, the fixture of `tests/changelog_render.bats` rendered without markers, quirk 55, the range), 4 in `snapshot` (the diff and added tools, conflicts, the queue text and its round trip through `read_pending_pairs`, dates), 2 in `format_rev` (the notes, the config path).

- [x] **Step 4: Confirm the Bash values the tests assert**

```bash
bash -c 'source lib/helpers/cli_colors.sh; source lib/helpers/update.sh; _md_plain "**Bold.** rest"; echo "$REPLY"'
printf 'ab cd ef\n' | fold -s -w 5 | od -c | head -2
printf '## 9.9.90\n\n- Ninety.\n\n## 9.9.9\n\n- Nine.\n' > "$TMPDIR/cl.md"
bash -c 'source lib/helpers/cli_colors.sh; source lib/helpers/update.sh; _show_changelog_sections "$1" "9.9.9"' _ "$TMPDIR/cl.md"
date -u -r 951782400 +%Y-%m-%d
```

Expected: `Bold. rest`; the `od` line `a   b      \n   c   d       e   f  \n` (fold breaks after the first blank, `ab ` then `cd ef`); the section

```text

  What's new in v9.9.9:

    • Ninety.
    • Nine.
```

and `2000-02-29`.

- [x] **Step 5: Commit**

```bash
git add src/changelog.rs src/snapshot.rs src/format_rev.rs src/lib.rs
git commit -m "feat(native): port the changelog renderer and the catalog diff"
```

---

### Task 2: `update`, the notice, the dispatcher guard, and the bats file

**Files:**
- Create: `src/cli/notice.rs`, `src/cli/update.rs`, `tests/update_native.bats`
- Modify: `src/main.rs`, `src/cli/mod.rs`, `src/cli/bundle.rs`, `bin/agentsync.sh`

**Interfaces:**

```rust
// src/cli/notice.rs
pub const REPO: &str = "yelmuratoff/agent_sync";
pub const CACHE_FILE: &str = ".update_cache";
pub fn wants_notice(command: &str) -> bool;
pub fn format_notice(project_dir: &Path, style: &Style) -> String;
pub fn update_banner(cache: &str, version: &str, style: &Style) -> String;
pub fn latest_release_url() -> String;
pub fn parse_tag_name(json: &str) -> Option<String>;
pub fn refresh_cache(cache_file: &Path);
// src/cli/update.rs
pub const CATALOG_COMMAND: &str = "__catalog";
pub struct Env<'a> {
    pub exe: PathBuf, pub project_dir: String, pub today: String, pub width: usize,
    pub fetch: &'a mut dyn FnMut(&str, &Path) -> Result<u16, String>,
    pub extract: &'a mut dyn FnMut(&Path, &Path) -> bool,
    pub ask: &'a mut dyn FnMut(&Path, &str) -> Option<String>,
}
pub fn target() -> Option<&'static str>;
pub fn catalog_entries() -> Vec<(String, String)>;
pub fn catalog_dump() -> String;
pub fn parse_catalog_dump(text: &str) -> Option<Vec<(String, String)>>;
pub fn curl_fetch(url: &str, to: &Path) -> Result<u16, String>;
pub fn tar_extract(archive: &Path, into: &Path) -> bool;
pub fn ask_binary(binary: &Path, arg: &str) -> Option<String>;
pub fn terminal_width() -> usize;
pub fn update(args: &[String], style: &Style, env: &mut Env, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>;
// src/cli/bundle.rs
pub(crate) struct Scratch(pub(crate) PathBuf);
impl Scratch { pub(crate) fn create(prefix: &str) -> Result<Self, Error>; }
// src/main.rs
fn current_exe() -> std::io::Result<PathBuf>;
fn notice_root() -> Result<String, Error>;
fn check_for_updates() -> Result<(), Error>;
```

```bash
# bin/agentsync.sh
_native_will_serve() { local command="$1"; ...; }   # AGENTSYNC_NATIVE != 0, listed, and a binary found
_native_will_serve "$command" || check_for_updates
```

- [x] **Step 1: Write `src/cli/notice.rs`**

<!-- file: src/cli/notice.rs -->
````rust
//! `check_for_updates` of `lib/helpers/update.sh`: the project-format notice
//! and the update banner the binary prints on a terminal before the commands
//! `bin/agentsync.sh` lists, and the background refresh of the banner's cache.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::format_rev;
use crate::style::Style;

/// The GitHub repository releases come from.
pub const REPO: &str = "yelmuratoff/agent_sync";

/// The cache file below the install root, one tag per line.
pub const CACHE_FILE: &str = ".update_cache";

const NOTICE_COMMANDS: [&str; 26] = [
    "sync",
    "init",
    "rollback",
    "check",
    "list",
    "ls",
    "setup-hooks",
    "export",
    "import",
    "refresh",
    "enable",
    "disable",
    "add",
    "adopt",
    "customize",
    "simplify",
    "migrate",
    "show",
    "diff",
    "resolve",
    "doctor",
    "dedupe",
    "profile",
    "help",
    "--help",
    "-h",
];

/// The commands `main` runs `check_for_updates` for; an empty word is `help`.
pub fn wants_notice(command: &str) -> bool {
    let command = if command.is_empty() { "help" } else { command };
    NOTICE_COMMANDS.contains(&command)
}

/// `_check_project_format`: the notice when the project is behind the engine's
/// format revision, nothing otherwise.
pub fn format_notice(project_dir: &Path, style: &Style) -> String {
    let Some(config) = format_rev::config_path(project_dir) else {
        return String::new();
    };
    let text = std::fs::read(&config)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let engine = format_rev::engine();
    let current = format_rev::project(&text);
    if current >= engine {
        return String::new();
    }
    let mut out = format!(
        "\n  {} {}\n",
        style.yellow("This project's agent config is a migration behind"),
        style.dim(&format!("(format r{current} → r{engine})"))
    );
    for note in format_rev::pending_notes(current, engine) {
        out.push_str(&format!("    {}\n", style.dim(&note)));
    }
    out.push_str(&format!(
        "  Preview it with {}, apply with {}\n\n",
        style.cyan("agentsync migrate"),
        style.cyan("agentsync migrate --apply")
    ));
    out
}

/// `read -r latest_tag < "$cache_file"`: the first line, IFS blanks trimmed.
fn cached_tag(cache: &str) -> &str {
    cache
        .split('\n')
        .next()
        .unwrap_or("")
        .trim_matches([' ', '\t'])
}

/// The banner when the cache names a version newer than `version`.
pub fn update_banner(cache: &str, version: &str, style: &Style) -> String {
    let latest = cached_tag(cache);
    if latest.is_empty()
        || latest == version
        || crate::changelog::version_cmp(version, latest) != std::cmp::Ordering::Less
    {
        return String::new();
    }
    format!(
        "\n  ╭──────────────────────────────────────────────────────╮\n  │  {}: {} → {}              \n  │  Run: {}                                \n  ╰──────────────────────────────────────────────────────╯\n\n",
        style.yellow("Update available"),
        style.dim(&format!("v{version}")),
        style.green(&format!("v{latest}")),
        style.cyan("agentsync update")
    )
}

/// The GitHub API answer the background fetch reads.
pub fn latest_release_url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

/// The `tag_name` of a release JSON, a leading `v` dropped as the Bash `sed`
/// dropped it.
pub fn parse_tag_name(json: &str) -> Option<String> {
    let after = &json[json.find("\"tag_name\"")? + "\"tag_name\"".len()..];
    let after = after.trim_start_matches([' ', '\t', '\n', '\r']);
    let after = after.strip_prefix(':')?;
    let after = after.trim_start_matches([' ', '\t', '\n', '\r']);
    let after = after.strip_prefix('"')?;
    let value = &after[..after.find('"')?];
    let value = value.strip_prefix('v').unwrap_or(value);
    (!value.is_empty()).then(|| value.to_string())
}

/// `_bg_fetch_latest_version`, run by `__update-cache`: `curl -sfL --max-time 5`
/// on the latest release, the tag written to the cache; every failure is silent.
pub fn refresh_cache(cache_file: &Path) {
    let Ok(output) = Command::new("curl")
        .args(["-sfL", "--max-time", "5", &latest_release_url()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    if let Some(tag) = parse_tag_name(&String::from_utf8_lossy(&output.stdout)) {
        let _ = std::fs::write(cache_file, format!("{tag}\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_runs_for_the_dispatcher_list_and_an_empty_word() {
        for command in ["sync", "help", "--help", "-h", "", "ls", "profile"] {
            assert!(wants_notice(command), "{command:?}");
        }
        for command in [
            "update",
            "release",
            "version",
            "generate",
            "shell-init",
            "upgrade-config",
            "nope",
        ] {
            assert!(!wants_notice(command), "{command:?}");
        }
    }

    #[test]
    fn the_format_notice_lists_the_pending_migrations() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(format_notice(dir.path(), &Style::plain()), "");
        std::fs::create_dir(dir.path().join(".ai")).unwrap();
        std::fs::write(
            dir.path().join(".ai/agent_sync.yaml"),
            "tools:\n  enabled: []\n",
        )
        .unwrap();
        assert_eq!(
            format_notice(dir.path(), &Style::plain()),
            "\n  This project's agent config is a migration behind (format r1 → r2)\n    r2  The agentsync skill is engine-owned now. A copy under .ai/src/skills/agentsync/ shadows it, so engine upgrades never reach your agents.\n  Preview it with agentsync migrate, apply with agentsync migrate --apply\n\n"
        );
        std::fs::write(dir.path().join(".ai/agent_sync.yaml"), "format: 2\n").unwrap();
        assert_eq!(format_notice(dir.path(), &Style::plain()), "");
    }

    #[test]
    fn the_banner_shows_only_a_newer_cached_tag() {
        let style = Style::plain();
        assert_eq!(update_banner("", "0.36.0", &style), "");
        assert_eq!(update_banner("0.36.0\n", "0.36.0", &style), "");
        assert_eq!(update_banner("0.35.2\n", "0.36.0", &style), "");
        assert_eq!(
            update_banner("  0.37.0\nignored\n", "0.36.0", &style),
            "\n  ╭──────────────────────────────────────────────────────╮\n  │  Update available: v0.36.0 → v0.37.0              \n  │  Run: agentsync update                                \n  ╰──────────────────────────────────────────────────────╯\n\n"
        );
        assert!(
            update_banner("0.37.0\n", "0.36.0", &Style::colored()).contains(
                "\x1b[33mUpdate available\x1b[0m: \x1b[2mv0.36.0\x1b[0m → \x1b[32mv0.37.0\x1b[0m"
            )
        );
    }

    #[test]
    fn the_tag_name_is_read_from_the_release_json() {
        assert_eq!(
            parse_tag_name(
                "{\"url\":\"x\",\"name\":\"Release 0.37.0\",\"tag_name\":\"0.37.0\",\"assets\":[{\"name\":\"a\"}]}"
            ),
            Some("0.37.0".to_string())
        );
        assert_eq!(
            parse_tag_name("{\n  \"tag_name\" : \"v1.2.3\"\n}"),
            Some("1.2.3".to_string())
        );
        assert_eq!(parse_tag_name("{\"message\":\"Not Found\"}"), None);
        assert_eq!(parse_tag_name("{\"tag_name\":\"\"}"), None);
        assert_eq!(
            latest_release_url(),
            "https://api.github.com/repos/yelmuratoff/agent_sync/releases/latest"
        );
    }
}
````

- [x] **Step 2: Write `src/cli/update.rs`**

<!-- file: src/cli/update.rs -->
````rust
//! `agentsync update`: `lib/helpers/update.sh` for a binary install. The
//! release archive for this platform comes from GitHub Releases through
//! `curl`, its sha256 is checked in-process, `tar` unpacks it, the new binary
//! is asked for its version and its catalog, and then it is moved over the
//! running one. The changelog is the archive's; conflicts with the project's
//! overrides are queued for `resolve` as before.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::bundle::Scratch;
use super::customize::put;
use super::notice::{CACHE_FILE, REPO};
use crate::changelog;
use crate::manifest::sha256_hex;
use crate::snapshot::{self, Conflict};
use crate::style::Style;
use crate::{Error, catalog, engine_version};

const USAGE: &str = "Usage: agentsync update [<version>] [--strict]";
const HELP: &str = "Usage: agentsync update [<version>] [--strict]\n\n  <version>   Pin the install to that release tag (e.g. 0.35.0) instead of\n              the latest release — what a project's agentsync_version asks for.\n  --strict    Exit non-zero if upstream changed a field you have overridden.\n";

/// The hidden command a newer binary answers with its catalog.
pub const CATALOG_COMMAND: &str = "__catalog";

/// What `update` takes from the process.
pub struct Env<'a> {
    /// The running binary with symlinks resolved: the file that is replaced.
    pub exe: PathBuf,
    /// `${AGENTSYNC_REPO_ROOT:-$(pwd)}`: the project whose overrides are checked.
    pub project_dir: String,
    /// `date -u +%Y-%m-%d`.
    pub today: String,
    /// `_changelog_width`.
    pub width: usize,
    /// Fetch `url` into the file: the HTTP status, or why curl could not answer.
    pub fetch: &'a mut dyn FnMut(&str, &Path) -> Result<u16, String>,
    /// Unpack the archive into the directory.
    pub extract: &'a mut dyn FnMut(&Path, &Path) -> bool,
    /// Run the binary with one argument: its stdout when it succeeds.
    pub ask: &'a mut dyn FnMut(&Path, &str) -> Option<String>,
}

/// The cargo-dist target this binary was built for, as its asset is named.
pub fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-musl"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

const fn archive_extension() -> &'static str {
    if cfg!(windows) { "zip" } else { "tar.xz" }
}

const fn binary_name() -> &'static str {
    if cfg!(windows) {
        "agentsync.exe"
    } else {
        "agentsync"
    }
}

/// The shipped catalog as `(slug, yaml)` in byte order.
pub fn catalog_entries() -> Vec<(String, String)> {
    catalog::base_tools()
        .into_iter()
        .filter_map(|slug| {
            let yaml = catalog::base_tool_yaml(&slug)?.to_string();
            Some((slug, yaml))
        })
        .collect()
}

/// `__catalog`: every base tool as `<slug> <bytes>\n<yaml>\n`, so the running
/// binary can diff its catalog against a newer binary's.
pub fn catalog_dump() -> String {
    let mut out = String::new();
    for (slug, yaml) in catalog_entries() {
        out.push_str(&format!("{slug} {}\n{yaml}\n", yaml.len()));
    }
    out
}

/// The entries of a [`catalog_dump`]; `None` when the text is not one.
pub fn parse_catalog_dump(text: &str) -> Option<Vec<(String, String)>> {
    let mut entries = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (header, after) = rest.split_once('\n')?;
        let (slug, len) = header.split_once(' ')?;
        let len: usize = len.parse().ok()?;
        if slug.is_empty() || !after.is_char_boundary(len) {
            return None;
        }
        let tail = after[len..].strip_prefix('\n')?;
        entries.push((slug.to_string(), after[..len].to_string()));
        rest = tail;
    }
    Some(entries)
}

/// `curl -sL --max-time 30 -o <to> -w %{http_code} <url>`.
pub fn curl_fetch(url: &str, to: &Path) -> Result<u16, String> {
    let output = Command::new("curl")
        .args(["-sL", "--max-time", "30", "-o"])
        .arg(to)
        .args(["-w", "%{http_code}", url])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    let code = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u16>()
        .unwrap_or(0);
    if !output.status.success() || code == 0 {
        return Err(format!(
            "curl exited with status {}",
            output.status.code().unwrap_or(1)
        ));
    }
    Ok(code)
}

/// `tar -xf <archive> -C <dir>`, tar's own diagnostics passing through.
pub fn tar_extract(archive: &Path, into: &Path) -> bool {
    Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .stdin(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `<binary> <arg>` with the dispatcher's version guard cleared: its stdout
/// when it exits 0.
pub fn ask_binary(binary: &Path, arg: &str) -> Option<String> {
    let output = Command::new(binary)
        .arg(arg)
        .env_remove("AGENTSYNC_ENGINE_VERSION")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `_changelog_width`: `tput cols` clamped, 80 without a usable answer.
pub fn terminal_width() -> usize {
    let cols = Command::new("tput")
        .arg("cols")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim_end_matches(['\n', '\r'])
                .to_string()
        });
    changelog::clamp_width(cols.as_deref())
}

/// The `.update_cache` beside the install's `bin/`, cleared once current.
fn cache_file(exe: &Path) -> Option<PathBuf> {
    Some(exe.parent()?.parent()?.join(CACHE_FILE))
}

/// `agentsync v<version>` as `version` prints it.
fn version_of(answer: &str) -> Option<String> {
    let line = answer.split('\n').next()?;
    let version = line.strip_prefix("agentsync v")?;
    (!version.is_empty()).then(|| version.to_string())
}

/// The unpacked binary: `agentsync[.exe]` at the top or below the archive's
/// one directory, as cargo-dist lays it out.
fn unpacked_binary(dir: &Path) -> Option<PathBuf> {
    let flat = dir.join(binary_name());
    if flat.is_file() {
        return Some(flat);
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .map(|top| top.join(binary_name()))
        .find(|path| path.is_file())
}

/// The staged copy renamed over the running binary. Windows cannot replace a
/// running executable, so there the old one is renamed aside first.
fn replace_binary(new: &Path, exe: &Path) -> Result<(), Error> {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let name = exe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| binary_name().to_string());
    let staged = dir.join(format!(".{name}.new"));
    std::fs::copy(new, &staged).map_err(|e| Error::io(&staged, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| Error::io(&staged, e))?;
    }
    #[cfg(windows)]
    {
        let old = dir.join(format!("{name}.old"));
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).map_err(|e| Error::io(exe, e))?;
    }
    std::fs::rename(&staged, exe).map_err(|e| Error::io(exe, e))
}

/// `_show_update_conflicts`: grouped by tool, in `LC_ALL=C sort` order.
fn conflicts_report(conflicts: &[Conflict], style: &Style) -> String {
    let mut lines: Vec<String> = conflicts
        .iter()
        .map(|c| {
            format!(
                "{}\t{}\t{}\t{}\t{}",
                c.tool, c.field, c.before, c.after, c.yours
            )
        })
        .collect();
    lines.sort_unstable();
    let unset = style.dim("(unset)");
    let shown = |value: &str| {
        if value.is_empty() {
            unset.clone()
        } else {
            value.to_string()
        }
    };
    let mut out = format!(
        "\n  {}\n\n",
        style.yellow("Upstream touched fields you have overridden:")
    );
    let mut current = String::new();
    for line in &lines {
        let mut parts = line.splitn(5, '\t');
        let tool = parts.next().unwrap_or("");
        let field = parts.next().unwrap_or("");
        let before = parts.next().unwrap_or("");
        let after = parts.next().unwrap_or("");
        let yours = parts.next().unwrap_or("");
        if tool.is_empty() {
            continue;
        }
        if tool != current {
            if !current.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("    {}\n", style.bold(tool)));
            current = tool.to_string();
        }
        out.push_str(&format!("      {} {field}\n", style.yellow("◆")));
        out.push_str(&format!(
            "          {} {} → {}\n",
            style.dim("base:"),
            shown(before),
            shown(after)
        ));
        out.push_str(&format!(
            "          {} {}\n",
            style.dim("your override:"),
            shown(yours)
        ));
    }
    out.push('\n');
    out
}

/// `_show_migration_banner`: a file under `.ai/src/{hooks,mcp,settings}/`,
/// dotfiles skipped as the glob skipped them.
fn migration_banner(project_dir: &Path, style: &Style) -> String {
    let src = project_dir.join(".ai/src");
    if !src.is_dir() {
        return String::new();
    }
    let found = ["hooks", "mcp", "settings"].iter().any(|resource| {
        std::fs::read_dir(src.join(resource))
            .map(|entries| {
                entries.filter_map(|entry| entry.ok()).any(|entry| {
                    !entry.file_name().to_string_lossy().starts_with('.') && entry.path().is_file()
                })
            })
            .unwrap_or(false)
    });
    if !found {
        return String::new();
    }
    format!(
        "\n  {}\n  {} {}{}\n  {} {}{}\n  {} {} {}\n  {}\n\n",
        style.yellow("Legacy payload layout detected"),
        style.dim("Your project has overrides under"),
        style.cyan(".ai/src/{hooks,mcp,settings}/"),
        style.dim(". The canonical"),
        style.dim("layout since 0.11 is"),
        style.cyan(".ai/src/tools/<tool>/<resource>.<ext>"),
        style.dim("."),
        style.dim("Run"),
        style.cyan("agentsync migrate --apply"),
        style.dim("to move them. Legacy paths still read,"),
        style.dim("but will be dropped in 0.12.")
    )
}

fn fetch_failed(err: &mut dyn Write, style: &Style, detail: &str) -> Result<u8, Error> {
    put(
        err,
        format!(
            "  {}: Failed to fetch updates from GitHub.\n    {detail}\n  {}\n",
            style.red("Error"),
            style.dim("Check your network connection and that the remote is reachable.")
        )
        .as_bytes(),
    )?;
    Ok(1)
}

fn refuse(err: &mut dyn Write, style: &Style, message: &str) -> Result<u8, Error> {
    put(
        err,
        format!("  {}: {message}\n", style.red("Error")).as_bytes(),
    )?;
    Ok(1)
}

enum Args {
    Help,
    Refused(String),
    Run { pin: Option<String>, strict: bool },
}

fn parse_args(args: &[String]) -> Args {
    let mut strict = false;
    let mut pin = None;
    for arg in args {
        match arg.as_str() {
            "--strict" => strict = true,
            "--help" | "-h" => return Args::Help,
            flag if flag.starts_with('-') => return Args::Refused(format!("Unknown flag: {flag}")),
            word => {
                if pin.is_some() {
                    return Args::Refused(format!("Unexpected argument: {word}"));
                }
                pin = Some(word.to_string());
            }
        }
    }
    Args::Run { pin, strict }
}

/// What the download produced, once every check passed.
struct Fetched {
    new_binary: PathBuf,
    new_version: String,
    new_catalog: Vec<(String, String)>,
    changelog: Option<String>,
}

/// The tag to install: the pin, or the latest release's.
fn resolve_tag(
    pin: Option<&str>,
    scratch: &Path,
    env: &mut Env,
    style: &Style,
    err: &mut dyn Write,
) -> Result<Result<String, u8>, Error> {
    if let Some(pin) = pin {
        return Ok(Ok(pin.to_string()));
    }
    let url = super::notice::latest_release_url();
    let answer = scratch.join("latest.json");
    match (env.fetch)(&url, &answer) {
        Ok(200) => {}
        Ok(code) => {
            return Ok(Err(fetch_failed(
                err,
                style,
                &format!("HTTP {code} for {url}"),
            )?));
        }
        Err(why) => return Ok(Err(fetch_failed(err, style, &why)?)),
    }
    let json = std::fs::read_to_string(&answer).map_err(|e| Error::io(&answer, e))?;
    match super::notice::parse_tag_name(&json) {
        Some(tag) => Ok(Ok(tag)),
        None => Ok(Err(fetch_failed(
            err,
            style,
            &format!("no tag_name in the answer from {url}"),
        )?)),
    }
}

/// The archive and its checksum downloaded, verified, unpacked, and the new
/// binary asked for its version and catalog.
fn fetch_release(
    tag: &str,
    pinned: bool,
    target: &str,
    scratch: &Path,
    env: &mut Env,
    style: &Style,
    err: &mut dyn Write,
) -> Result<Result<Fetched, u8>, Error> {
    let archive_name = format!("agentsync-{target}.{}", archive_extension());
    let base = format!("https://github.com/{REPO}/releases/download/{tag}");
    let archive = scratch.join(&archive_name);
    match (env.fetch)(&format!("{base}/{archive_name}"), &archive) {
        Ok(200) => {}
        Ok(404) if pinned => {
            let probe = scratch.join("tag.json");
            let tag_url = format!("https://api.github.com/repos/{REPO}/git/ref/tags/{tag}");
            let message = match (env.fetch)(&tag_url, &probe) {
                Ok(200) => format!(
                    "AgentSync {tag} predates the binary releases, so update cannot install it.\n  {}\n    AGENTSYNC_VERSION={tag} curl -fsSL https://raw.githubusercontent.com/{REPO}/main/install.sh | bash",
                    style.dim("Pin it with the installer instead:")
                ),
                _ => format!(
                    "No AgentSync release is tagged {tag}.\n  {} {}",
                    style.dim("List releases at"),
                    style.cyan(&format!("https://github.com/{REPO}/releases"))
                ),
            };
            return Ok(Err(refuse(err, style, &message)?));
        }
        Ok(code) => {
            return Ok(Err(fetch_failed(
                err,
                style,
                &format!("HTTP {code} for {base}/{archive_name}"),
            )?));
        }
        Err(why) => return Ok(Err(fetch_failed(err, style, &why)?)),
    }
    let sum_name = format!("{archive_name}.sha256");
    let sum_file = scratch.join(&sum_name);
    match (env.fetch)(&format!("{base}/{sum_name}"), &sum_file) {
        Ok(200) => {}
        Ok(code) => {
            return Ok(Err(fetch_failed(
                err,
                style,
                &format!("HTTP {code} for {base}/{sum_name}"),
            )?));
        }
        Err(why) => return Ok(Err(fetch_failed(err, style, &why)?)),
    }
    let expected = std::fs::read_to_string(&sum_file)
        .map_err(|e| Error::io(&sum_file, e))?
        .split_ascii_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let actual = sha256_hex(&std::fs::read(&archive).map_err(|e| Error::io(&archive, e))?);
    if expected != actual {
        return Ok(Err(refuse(
            err,
            style,
            &format!(
                "checksum mismatch for {archive_name}.\n  {}",
                style.dim(&format!("expected {expected}, got {actual}"))
            ),
        )?));
    }
    let unpacked = scratch.join("unpacked");
    std::fs::create_dir_all(&unpacked).map_err(|e| Error::io(&unpacked, e))?;
    if !(env.extract)(&archive, &unpacked) {
        return Ok(Err(refuse(
            err,
            style,
            &format!("could not unpack {archive_name}."),
        )?));
    }
    let Some(new_binary) = unpacked_binary(&unpacked) else {
        return Ok(Err(refuse(
            err,
            style,
            &format!("{archive_name} does not contain {}.", binary_name()),
        )?));
    };
    let Some(new_version) = (env.ask)(&new_binary, "version")
        .as_deref()
        .and_then(version_of)
    else {
        return Ok(Err(refuse(
            err,
            style,
            &format!(
                "the downloaded binary does not run: {}",
                new_binary.to_string_lossy()
            ),
        )?));
    };
    let Some(new_catalog) = (env.ask)(&new_binary, CATALOG_COMMAND)
        .as_deref()
        .and_then(parse_catalog_dump)
    else {
        return Ok(Err(refuse(
            err,
            style,
            &format!(
                "the downloaded binary did not answer {CATALOG_COMMAND}: {}",
                new_binary.to_string_lossy()
            ),
        )?));
    };
    let changelog = new_binary
        .parent()
        .and_then(|dir| std::fs::read_to_string(dir.join("CHANGELOG.md")).ok());
    Ok(Ok(Fetched {
        new_binary,
        new_version,
        new_catalog,
        changelog,
    }))
}

/// `cmd_update`.
pub fn update(
    args: &[String],
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (pin, strict) = match parse_args(args) {
        Args::Help => {
            put(out, HELP.as_bytes())?;
            return Ok(0);
        }
        Args::Refused(message) => {
            put(
                err,
                format!("{}: {message}\n{USAGE}\n", style.red("Error")).as_bytes(),
            )?;
            return Ok(2);
        }
        Args::Run { pin, strict } => (pin, strict),
    };
    put(
        out,
        format!(
            "\n{}\n\n  Checking for updates...\n",
            style.bold("  AgentSync Update")
        )
        .as_bytes(),
    )?;
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    let Some(target) = target() else {
        return refuse(
            err,
            style,
            &format!(
                "no release binary is built for this platform ({}/{}).",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        );
    };
    let scratch = Scratch::create("agentsync-update")?;
    let old_version = engine_version();
    let tag = match resolve_tag(pin.as_deref(), &scratch.0, env, style, err)? {
        Ok(tag) => tag,
        Err(status) => return Ok(status),
    };
    if tag == old_version {
        put(
            out,
            format!(
                "  {} (v{old_version})\n\n",
                style.green("Already up to date!")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    if pin.is_some() {
        put(out, format!("  Pinning to v{tag}...\n").as_bytes())?;
    } else {
        put(out, b"  Updating...\n")?;
    }
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    let fetched = match fetch_release(&tag, pin.is_some(), target, &scratch.0, env, style, err)? {
        Ok(fetched) => fetched,
        Err(status) => return Ok(status),
    };
    let project_dir = Path::new(&env.project_dir);
    let changes = snapshot::diff(&catalog_entries(), &fetched.new_catalog);
    let conflicts = snapshot::find_conflicts(project_dir, &changes);
    replace_binary(&fetched.new_binary, &env.exe)?;
    if let Some(cache) = cache_file(&env.exe) {
        let _ = std::fs::remove_file(cache);
    }
    let new_version = fetched.new_version.as_str();
    if old_version == new_version {
        put(
            out,
            format!("\n  {} (v{new_version})\n", style.green("Updated!")).as_bytes(),
        )?;
    } else {
        put(
            out,
            format!(
                "\n  {} v{old_version} → v{new_version}\n",
                style.green("Updated!")
            )
            .as_bytes(),
        )?;
    }
    if let Some(changelog) = &fetched.changelog {
        let versions = changelog::versions_in_range(changelog, old_version, new_version);
        put(
            out,
            changelog::sections(changelog, &versions, env.width, style).as_bytes(),
        )?;
    }
    if !conflicts.is_empty() {
        put(out, conflicts_report(&conflicts, style).as_bytes())?;
        if project_dir.join(".ai").is_dir() {
            snapshot::write_pending_resolutions(
                project_dir,
                &env.today,
                old_version,
                new_version,
                &conflicts,
            )?;
            put(
                out,
                format!(
                    "  {} {}{} {}{}\n\n",
                    style.dim("Queued in"),
                    style.cyan(".ai/.pending-resolutions.yaml"),
                    style.dim(" — run"),
                    style.cyan("agentsync resolve"),
                    style.dim(" to walk them.")
                )
                .as_bytes(),
            )?;
        }
    }
    put(out, b"\n")?;
    put(out, migration_banner(project_dir, style).as_bytes())?;
    Ok(u8::from(strict && !conflicts.is_empty()))
}

#[cfg(all(test, unix))]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn the_catalog_dump_round_trips_the_thirteen_tools() {
        let entries = catalog_entries();
        assert_eq!(entries.len(), 13);
        let dump = catalog_dump();
        assert!(dump.starts_with("amazonq "));
        assert_eq!(parse_catalog_dump(&dump), Some(entries));
        assert_eq!(parse_catalog_dump(""), Some(Vec::new()));
        assert_eq!(parse_catalog_dump("claude 3\nab\n"), None);
        assert_eq!(parse_catalog_dump("claude x\n"), None);
        assert_eq!(
            parse_catalog_dump("a 2\nxy\nb 0\n\n"),
            Some(vec![
                ("a".to_string(), "xy".to_string()),
                ("b".to_string(), String::new())
            ])
        );
    }

    #[test]
    fn the_target_is_one_of_the_five_dist_targets() {
        let target = target().expect("a supported host");
        assert!(
            [
                "aarch64-apple-darwin",
                "x86_64-apple-darwin",
                "aarch64-unknown-linux-musl",
                "x86_64-unknown-linux-musl",
                "x86_64-pc-windows-msvc",
            ]
            .contains(&target)
        );
        assert_eq!(
            version_of("agentsync v0.36.0\n"),
            Some("0.36.0".to_string())
        );
        assert_eq!(version_of("agentsync v"), None);
        assert_eq!(version_of("nope"), None);
    }

    #[test]
    fn conflicts_are_grouped_by_tool_in_byte_order_with_unset_marked() {
        let conflicts = [
            Conflict {
                tool: "cursor".to_string(),
                field: "targets.rules.dest".to_string(),
                before: ".cursor/rules".to_string(),
                after: ".cursor/rules-v2".to_string(),
                yours: "mine".to_string(),
            },
            Conflict {
                tool: "claude".to_string(),
                field: "name".to_string(),
                before: String::new(),
                after: "Claude".to_string(),
                yours: "Mine".to_string(),
            },
        ];
        assert_eq!(
            conflicts_report(&conflicts, &Style::plain()),
            "\n  Upstream touched fields you have overridden:\n\n    claude\n      ◆ name\n          base: (unset) → Claude\n          your override: Mine\n\n    cursor\n      ◆ targets.rules.dest\n          base: .cursor/rules → .cursor/rules-v2\n          your override: mine\n\n"
        );
    }

    #[test]
    fn the_migration_banner_needs_a_visible_file_under_a_legacy_directory() {
        let dir = tempfile::tempdir().unwrap();
        let style = Style::plain();
        assert_eq!(migration_banner(dir.path(), &style), "");
        std::fs::create_dir_all(dir.path().join(".ai/src/mcp")).unwrap();
        std::fs::write(dir.path().join(".ai/src/mcp/.keep"), "").unwrap();
        assert_eq!(migration_banner(dir.path(), &style), "");
        std::fs::write(dir.path().join(".ai/src/mcp/claude.json"), "{}").unwrap();
        assert_eq!(
            migration_banner(dir.path(), &style),
            "\n  Legacy payload layout detected\n  Your project has overrides under .ai/src/{hooks,mcp,settings}/. The canonical\n  layout since 0.11 is .ai/src/tools/<tool>/<resource>.<ext>.\n  Run agentsync migrate --apply to move them. Legacy paths still read,\n  but will be dropped in 0.12.\n\n"
        );
    }

    /// A fake GitHub: URL to file bytes, served through the `fetch` seam; a
    /// fake archive whose "extraction" copies the staged directory; a fake
    /// binary answering `version` and `__catalog` from what the test staged.
    struct Fixture {
        dir: tempfile::TempDir,
        served: BTreeMap<String, Vec<u8>>,
        release_dir: PathBuf,
        version: String,
        catalog: String,
        offline: bool,
    }

    impl Fixture {
        fn new(version: &str, catalog: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join("install/bin")).unwrap();
            std::fs::write(dir.path().join("install/bin/agentsync"), "old binary").unwrap();
            std::fs::write(dir.path().join("install/.update_cache"), "9.9.9\n").unwrap();
            std::fs::create_dir_all(dir.path().join("project/.ai/src/tools")).unwrap();
            let release_dir = dir.path().join("release/agentsync-fixture");
            std::fs::create_dir_all(&release_dir).unwrap();
            std::fs::write(release_dir.join("agentsync"), "new binary").unwrap();
            std::fs::write(
                release_dir.join("CHANGELOG.md"),
                format!("# Changelog\n\n## {version}\n\n### Fixed\n\n- **Something** with `code`.\n\n## 0.1.0\n\n- Ancient.\n"),
            )
            .unwrap();
            Self {
                dir,
                served: BTreeMap::new(),
                release_dir,
                version: version.to_string(),
                catalog: catalog.to_string(),
                offline: false,
            }
        }

        fn exe(&self) -> PathBuf {
            self.dir.path().join("install/bin/agentsync")
        }

        fn project(&self) -> PathBuf {
            self.dir.path().join("project")
        }

        fn publish(&mut self, tag: &str) {
            let target = target().unwrap();
            let base = format!("https://github.com/{REPO}/releases/download/{tag}");
            let archive = b"an archive".to_vec();
            let sum = format!("{}  agentsync-{target}.tar.xz\n", sha256_hex(&archive));
            self.served
                .insert(format!("{base}/agentsync-{target}.tar.xz"), archive);
            self.served.insert(
                format!("{base}/agentsync-{target}.tar.xz.sha256"),
                sum.into_bytes(),
            );
        }

        fn run(&mut self, args: &[&str]) -> (u8, String, String) {
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            let served = self.served.clone();
            let offline = self.offline;
            let mut fetch = |url: &str, to: &Path| -> Result<u16, String> {
                if offline {
                    return Err("curl exited with status 6".to_string());
                }
                match served.get(url) {
                    Some(bytes) => {
                        std::fs::write(to, bytes).unwrap();
                        Ok(200)
                    }
                    None => Ok(404),
                }
            };
            let release_dir = self.release_dir.clone();
            let mut extract = |_archive: &Path, into: &Path| -> bool {
                let top = into.join("agentsync-fixture");
                std::fs::create_dir_all(&top).unwrap();
                for name in ["agentsync", "CHANGELOG.md"] {
                    std::fs::copy(release_dir.join(name), top.join(name)).unwrap();
                }
                true
            };
            let (version, catalog) = (self.version.clone(), self.catalog.clone());
            let mut ask = |_binary: &Path, arg: &str| -> Option<String> {
                match arg {
                    "version" => Some(format!("agentsync v{version}\n")),
                    CATALOG_COMMAND => Some(catalog.clone()),
                    _ => None,
                }
            };
            let mut env = Env {
                exe: self.exe(),
                project_dir: self.project().to_string_lossy().into_owned(),
                today: "2026-09-18".to_string(),
                width: 80,
                fetch: &mut fetch,
                extract: &mut extract,
                ask: &mut ask,
            };
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let status = update(&args, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
            (
                status,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        }
    }

    #[test]
    fn help_and_bad_arguments_answer_like_cmd_update() {
        let mut fixture = Fixture::new("9.9.9", &catalog_dump());
        let (status, out, err) = fixture.run(&["--help"]);
        assert_eq!((status, out.as_str(), err.as_str()), (0, HELP, ""));
        let (status, out, err) = fixture.run(&["--bogus"]);
        assert_eq!(
            (status, out.as_str(), err.as_str()),
            (
                2,
                "",
                "Error: Unknown flag: --bogus\nUsage: agentsync update [<version>] [--strict]\n"
            )
        );
        let (status, _, err) = fixture.run(&["1.0.0", "2.0.0"]);
        assert_eq!(
            (status, err.as_str()),
            (
                2,
                "Error: Unexpected argument: 2.0.0\nUsage: agentsync update [<version>] [--strict]\n"
            )
        );
    }

    #[test]
    fn the_latest_release_replaces_the_binary_and_prints_its_changelog() {
        let mut fixture = Fixture::new("9.9.9", &catalog_dump());
        fixture.served.insert(
            super::super::notice::latest_release_url(),
            b"{\"tag_name\":\"9.9.9\"}".to_vec(),
        );
        fixture.publish("9.9.9");
        let (status, out, err) = fixture.run(&[]);
        assert_eq!(err, "");
        assert_eq!(status, 0);
        assert_eq!(
            out,
            format!(
                "\n  AgentSync Update\n\n  Checking for updates...\n  Updating...\n\n  Updated! v{0} → v9.9.9\n\n  What's new in v9.9.9:\n\n\n  Fixed\n    • Something with code.\n\n",
                engine_version()
            )
        );
        assert_eq!(
            std::fs::read_to_string(fixture.exe()).unwrap(),
            "new binary"
        );
        assert!(!fixture.dir.path().join("install/.update_cache").exists());
        assert!(
            !fixture
                .project()
                .join(".ai/.pending-resolutions.yaml")
                .exists()
        );
    }

    #[test]
    fn the_running_version_is_already_up_to_date() {
        let mut fixture = Fixture::new("9.9.9", &catalog_dump());
        let (status, out, err) = fixture.run(&[engine_version()]);
        assert_eq!(err, "");
        assert_eq!(status, 0);
        assert_eq!(
            out,
            format!(
                "\n  AgentSync Update\n\n  Checking for updates...\n  Already up to date! (v{})\n\n",
                engine_version()
            )
        );
        assert_eq!(
            std::fs::read_to_string(fixture.exe()).unwrap(),
            "old binary"
        );
    }

    #[test]
    fn a_pin_reports_an_unknown_tag_a_pre_binary_tag_and_a_bad_checksum() {
        let mut fixture = Fixture::new("9.9.9", &catalog_dump());
        let (status, out, err) = fixture.run(&["999.0.0"]);
        assert_eq!(status, 1);
        assert!(out.ends_with("  Pinning to v999.0.0...\n"));
        assert_eq!(
            err,
            "  Error: No AgentSync release is tagged 999.0.0.\n  List releases at https://github.com/yelmuratoff/agent_sync/releases\n"
        );
        fixture.served.insert(
            format!("https://api.github.com/repos/{REPO}/git/ref/tags/0.1.0"),
            b"{\"ref\":\"refs/tags/0.1.0\"}".to_vec(),
        );
        let (status, _, err) = fixture.run(&["0.1.0"]);
        assert_eq!(status, 1);
        assert_eq!(
            err,
            "  Error: AgentSync 0.1.0 predates the binary releases, so update cannot install it.\n  Pin it with the installer instead:\n    AGENTSYNC_VERSION=0.1.0 curl -fsSL https://raw.githubusercontent.com/yelmuratoff/agent_sync/main/install.sh | bash\n"
        );
        fixture.publish("9.9.9");
        let target = target().unwrap();
        fixture.served.insert(
            format!(
                "https://github.com/{REPO}/releases/download/9.9.9/agentsync-{target}.tar.xz.sha256"
            ),
            b"0000  nope\n".to_vec(),
        );
        let (status, _, err) = fixture.run(&["9.9.9"]);
        assert_eq!(status, 1);
        assert!(err.starts_with(&format!(
            "  Error: checksum mismatch for agentsync-{target}.tar.xz.\n  expected 0000, got "
        )));
        assert_eq!(
            std::fs::read_to_string(fixture.exe()).unwrap(),
            "old binary"
        );
    }

    #[test]
    fn a_fetch_failure_names_the_cause() {
        let mut fixture = Fixture::new("9.9.9", &catalog_dump());
        let (status, _, err) = fixture.run(&[]);
        assert_eq!(status, 1);
        assert_eq!(
            err,
            "  Error: Failed to fetch updates from GitHub.\n    HTTP 404 for https://api.github.com/repos/yelmuratoff/agent_sync/releases/latest\n  Check your network connection and that the remote is reachable.\n"
        );
        fixture.offline = true;
        let (status, _, err) = fixture.run(&["9.9.9"]);
        assert_eq!(status, 1);
        assert_eq!(
            err,
            "  Error: Failed to fetch updates from GitHub.\n    curl exited with status 6\n  Check your network connection and that the remote is reachable.\n"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.exe()).unwrap(),
            "old binary"
        );
    }

    #[test]
    fn a_changed_overridden_field_is_reported_queued_and_fails_strict() {
        let mut catalog = catalog_dump();
        let claude = catalog::base_tool_yaml("claude").unwrap();
        let changed = claude.replacen(".claude/rules", ".claude/rules-v2", 1);
        assert_ne!(claude, changed);
        catalog = catalog.replace(
            &format!("claude {}\n{claude}", claude.len()),
            &format!("claude {}\n{changed}", changed.len()),
        );
        let mut fixture = Fixture::new("9.9.9", &catalog);
        fixture.publish("9.9.9");
        std::fs::write(
            fixture.project().join(".ai/src/tools/claude.yaml"),
            "targets:\n  rules:\n    dest: \".claude/my-rules\"\n",
        )
        .unwrap();
        let (status, out, err) = fixture.run(&["9.9.9"]);
        assert_eq!(err, "");
        assert_eq!(status, 0);
        assert!(out.contains("\n  Upstream touched fields you have overridden:\n\n    claude\n      ◆ targets.rules.dest\n          base: .claude/rules → .claude/rules-v2\n          your override: .claude/my-rules\n\n  Queued in .ai/.pending-resolutions.yaml — run agentsync resolve to walk them.\n\n\n"));
        let queue =
            std::fs::read_to_string(fixture.project().join(".ai/.pending-resolutions.yaml"))
                .unwrap();
        assert!(queue.contains(&format!(
            "from_version: \"{}\"\nto_version: \"9.9.9\"\n",
            engine_version()
        )));
        assert!(queue.contains("    your_override: \".claude/my-rules\"\n"));
        std::fs::write(fixture.exe(), "old binary").unwrap();
        let (status, _, _) = fixture.run(&["9.9.9", "--strict"]);
        assert_eq!(status, 1);
        assert_eq!(
            std::fs::read_to_string(fixture.exe()).unwrap(),
            "new binary"
        );
    }
}
````

- [x] **Step 3: Apply the wiring patch**

Save the block below as `$TMPDIR/task2.diff` and run `git apply "$TMPDIR/task2.diff"`. It adds the notice, the two hidden commands, and the `update` arm to `src/main.rs` (before `wants_usage`, so `check --help` prints the notice first as Bash does), registers the two modules in `src/cli/mod.rs`, opens `Scratch` to the crate with a prefix in `src/cli/bundle.rs`, and adds `_native_will_serve` with the guard to `bin/agentsync.sh`.

<!-- file: task2.diff -->
````diff
diff --git a/src/main.rs b/src/main.rs
index 9095fc7..6610b36 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -26,6 +26,50 @@ fn main() -> ExitCode {
 
 fn run(args: Vec<OsString>) -> Result<u8, Error> {
     guard_engine_version()?;
+    let first = args.first().and_then(|a| a.to_str()).unwrap_or("");
+    if cli::notice::wants_notice(first) {
+        check_for_updates()?;
+    }
+    if first == cli::update::CATALOG_COMMAND {
+        let mut out = std::io::stdout().lock();
+        return out
+            .write_all(cli::update::catalog_dump().as_bytes())
+            .map(|()| 0)
+            .map_err(|e| Error::io("<stdout>", e));
+    }
+    if first == "__update-cache" {
+        if let Some(cache) = args.get(1) {
+            cli::notice::refresh_cache(Path::new(cache));
+        }
+        return Ok(0);
+    }
+    if first == "update" {
+        let rest: Vec<String> = args[1..]
+            .iter()
+            .map(|a| a.to_string_lossy().into_owned())
+            .collect();
+        let exe = current_exe().map_err(|e| Error::io("<exe>", e))?;
+        let now = std::time::SystemTime::now()
+            .duration_since(std::time::UNIX_EPOCH)
+            .map(|d| d.as_secs())
+            .unwrap_or(0);
+        let mut env = cli::update::Env {
+            exe,
+            project_dir: notice_root()?,
+            today: agentsync::snapshot::utc_date(now),
+            width: cli::update::terminal_width(),
+            fetch: &mut cli::update::curl_fetch,
+            extract: &mut cli::update::tar_extract,
+            ask: &mut cli::update::ask_binary,
+        };
+        return cli::update::update(
+            &rest,
+            &Style::for_stdout(),
+            &mut env,
+            &mut std::io::stdout(),
+            &mut std::io::stderr(),
+        );
+    }
     let words: Vec<String> = args
         .iter()
         .map(|a| a.to_string_lossy().into_owned())
@@ -556,6 +600,60 @@ fn project_root() -> Result<String, Error> {
     Ok(root)
 }
 
+/// The running binary with symlinks resolved.
+fn current_exe() -> std::io::Result<PathBuf> {
+    std::env::current_exe()?.canonicalize()
+}
+
+/// `${AGENTSYNC_REPO_ROOT:-$PWD}`, spelled logically.
+fn notice_root() -> Result<String, Error> {
+    let env_root = var("AGENTSYNC_REPO_ROOT").filter(|root| !root.is_empty());
+    let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
+    Ok(paths::logical_root(
+        env_root.as_deref(),
+        &cwd,
+        var("PWD").as_deref(),
+    ))
+}
+
+/// `check_for_updates`: on a terminal, unless `AGENTSYNC_NO_UPDATE_CHECK` is
+/// set, the project-format notice, the banner from the cache beside the
+/// install's `bin/`, and a detached `__update-cache` run that refreshes the
+/// cache for the next time.
+fn check_for_updates() -> Result<(), Error> {
+    if !std::io::stdout().is_terminal()
+        || var("AGENTSYNC_NO_UPDATE_CHECK").is_some_and(|v| !v.is_empty())
+    {
+        return Ok(());
+    }
+    let style = Style::for_stdout();
+    let root = notice_root()?;
+    let mut out = std::io::stdout().lock();
+    out.write_all(cli::notice::format_notice(Path::new(&root), &style).as_bytes())
+        .map_err(|e| Error::io("<stdout>", e))?;
+    let Some(cache) = current_exe()
+        .ok()
+        .and_then(|exe| Some(exe.parent()?.parent()?.join(cli::notice::CACHE_FILE)))
+    else {
+        return Ok(());
+    };
+    if let Ok(text) = std::fs::read_to_string(&cache) {
+        out.write_all(cli::notice::update_banner(&text, engine_version(), &style).as_bytes())
+            .map_err(|e| Error::io("<stdout>", e))?;
+    }
+    out.flush().map_err(|e| Error::io("<stdout>", e))?;
+    if let Ok(exe) = std::env::current_exe() {
+        let _ = std::process::Command::new(exe)
+            .arg("__update-cache")
+            .arg(&cache)
+            .stdin(std::process::Stdio::null())
+            .stdout(std::process::Stdio::null())
+            .stderr(std::process::Stdio::null())
+            .spawn();
+    }
+    Ok(())
+}
+
 fn print_usage() -> Result<u8, Error> {
     let mut out = std::io::stdout().lock();
     out.write_all(cli::usage::usage(&Style::for_stdout()).as_bytes())
diff --git a/src/cli/mod.rs b/src/cli/mod.rs
index 9d12822..42a29a0 100644
--- a/src/cli/mod.rs
+++ b/src/cli/mod.rs
@@ -11,6 +11,7 @@ pub mod generate;
 pub mod init;
 pub mod list;
 pub mod migrate;
+pub mod notice;
 pub mod profile;
 pub mod refresh;
 pub mod release;
@@ -21,6 +22,7 @@ pub mod shell_init;
 pub mod show;
 pub mod simplify;
 pub mod sync;
+pub mod update;
 pub mod upgrade_config;
 pub mod usage;
 pub mod workspace;
diff --git a/src/cli/bundle.rs b/src/cli/bundle.rs
index 97e3d08..5ae748b 100644
--- a/src/cli/bundle.rs
+++ b/src/cli/bundle.rs
@@ -324,16 +324,15 @@ fn github_segments(source: &str) -> Option<(&str, &str)> {
 
 /// A scratch directory under the system temp dir, removed on drop as the run
 /// directory was.
-struct Scratch(PathBuf);
+pub(crate) struct Scratch(pub(crate) PathBuf);
 
 impl Scratch {
-    fn create() -> Result<Self, Error> {
+    pub(crate) fn create(prefix: &str) -> Result<Self, Error> {
         let nanos = std::time::SystemTime::now()
             .duration_since(std::time::UNIX_EPOCH)
             .map(|d| d.as_nanos())
             .unwrap_or(0);
-        let dir =
-            std::env::temp_dir().join(format!("agentsync-import.{}.{nanos}", std::process::id()));
+        let dir = std::env::temp_dir().join(format!("{prefix}.{}.{nanos}", std::process::id()));
         std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
         Ok(Self(dir))
     }
@@ -617,7 +616,7 @@ pub fn import(
         format!("\n{}\n\n", style.bold("  AgentSync Import")).as_bytes(),
     )?;
     out.flush().map_err(|e| Error::io("<stdout>", e))?;
-    let scratch = Scratch::create()?;
+    let scratch = Scratch::create("agentsync-import")?;
     let tmp = scratch.0.as_path();
     let error = style.red("Error");
 
diff --git a/bin/agentsync.sh b/bin/agentsync.sh
index 6f787ae..f40645c 100755
--- a/bin/agentsync.sh
+++ b/bin/agentsync.sh
@@ -299,6 +299,15 @@ _native_bin() {
     return 1
 }
 
+# True when _native_try will hand the command to a binary, which then prints
+# the update notice itself.
+_native_will_serve() {
+    local command="$1"
+    [[ "${AGENTSYNC_NATIVE:-}" != "0" ]] || return 1
+    [[ "$_NATIVE_COMMANDS" == *" $command "* ]] || return 1
+    _native_bin > /dev/null 2>&1
+}
+
 # Delegate the whole argument list to the native binary when the command is
 # ported and a binary is available. Exits with the binary's status; returns 1
 # to fall through to the Bash implementation.
@@ -332,7 +341,7 @@ main() {
         sync|init|rollback|check|list|ls|setup-hooks|export|import|refresh|enable|disable|add|adopt|customize|simplify|migrate|show|diff|resolve|doctor|dedupe|profile|help|--help|-h)
             # yaml/format back the project-format notice inside the check.
             _need yaml format
-            check_for_updates
+            _native_will_serve "$command" || check_for_updates
             ;;
     esac
 
````

- [x] **Step 4: Write `tests/update_native.bats`**

<!-- file: tests/update_native.bats -->
````bash
#!/usr/bin/env bats
# `agentsync update` on a binary install: the binary run directly, as the
# installer links it, against a curl stand-in on PATH that serves a fixture
# release and a real tar archive. Skips when no binary is built.

load test_helper

setup_file() { seed_project; }
teardown_file() { teardown_seed_project; }

setup() {
    clone_seed
    NATIVE_BIN="${AGENTSYNC_NATIVE_BIN:-$REPO_ROOT/target/release/agentsync}"
    [[ -x "$NATIVE_BIN" ]] || skip "no native binary at $NATIVE_BIN"
    ENGINE_VERSION="$(cat "$REPO_ROOT/VERSION")"

    # A copy is what gets replaced, never the build under target/.
    INSTALL="$TEST_PROJECT/install"
    mkdir -p "$INSTALL/bin"
    cp "$NATIVE_BIN" "$INSTALL/bin/agentsync"
    AGENTSYNC="$INSTALL/bin/agentsync"
    printf '9.9.9\n' > "$INSTALL/.update_cache"

    FAKE_RELEASES="$TEST_PROJECT/releases"
    mkdir -p "$FAKE_RELEASES/tags" stub
    cat > stub/curl <<'EOF'
#!/usr/bin/env bash
# Serves $FAKE_RELEASES for the URLs update builds and prints the HTTP status
# as `curl -w %{http_code}` does; anything else fails as an unreachable host.
out=""; url=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        -w|--max-time) shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
    esac
done
case "$url" in
    https://api.github.com/repos/yelmuratoff/agent_sync/releases/latest)
        file="$FAKE_RELEASES/latest.json" ;;
    https://api.github.com/repos/yelmuratoff/agent_sync/git/ref/tags/*)
        file="$FAKE_RELEASES/tags/${url##*/}" ;;
    https://github.com/yelmuratoff/agent_sync/releases/download/*)
        rest="${url#*/releases/download/}"; tag="${rest%%/*}"; name="${rest#*/}"
        case "$name" in
            agentsync-*.sha256) name="archive.tar.xz.sha256" ;;
            agentsync-*) name="archive.tar.xz" ;;
        esac
        file="$FAKE_RELEASES/$tag/$name" ;;
    *) printf '000'; exit 6 ;;
esac
if [ -f "$file" ]; then cp "$file" "$out"; printf '200'; else printf '404'; fi
EOF
    chmod +x stub/curl
    export FAKE_RELEASES
    export PATH="$TEST_PROJECT/stub:$PATH"
}

teardown() { teardown_test_project; }

# A fixture release: an archive laid out as cargo-dist lays it out, its binary
# a script answering `version` and `__catalog`, its catalog the running
# binary's unless a dump is given.
# Usage: publish_release <tag> [<catalog dump file>]
publish_release() {
    local tag="$1" dump="${2:-}"
    local top="$TEST_PROJECT/rel-$tag/agentsync-fixture"
    mkdir -p "$top" "$FAKE_RELEASES/$tag"
    if [[ -n "$dump" ]]; then
        cp "$dump" "$top/catalog.dump"
    else
        "$AGENTSYNC" __catalog > "$top/catalog.dump"
    fi
    cat > "$top/agentsync" <<EOF
#!/usr/bin/env bash
case "\${1:-}" in
    version) echo "agentsync v$tag" ;;
    __catalog) cat "\$(dirname "\$0")/catalog.dump" ;;
    *) exit 1 ;;
esac
EOF
    chmod +x "$top/agentsync"
    printf '# Changelog\n\n## %s\n\n### Fixed\n\n- **Something** with `code`.\n\n## 0.1.0\n\n- Ancient.\n' "$tag" > "$top/CHANGELOG.md"
    tar -cJf "$FAKE_RELEASES/$tag/archive.tar.xz" -C "$TEST_PROJECT/rel-$tag" agentsync-fixture
    printf '%s  archive.tar.xz\n' "$(file_sha256 "$FAKE_RELEASES/$tag/archive.tar.xz")" \
        > "$FAKE_RELEASES/$tag/archive.tar.xz.sha256"
}

@test "update native: --help prints the usage without touching the network" {
    run "$AGENTSYNC" update --help
    [ "$status" -eq 0 ]
    [[ "$output" == *"--strict"* ]]
    [[ "$output" == *"the latest release"* ]]
}

@test "update native: an unknown flag is refused with status 2" {
    run "$AGENTSYNC" update --bogus
    [ "$status" -eq 2 ]
    [[ "$output" == *"Unknown flag: --bogus"* ]]
}

@test "update native: the latest release replaces the binary and prints its changelog" {
    publish_release 9.9.9
    printf '{"tag_name":"9.9.9","name":"9.9.9"}\n' > "$FAKE_RELEASES/latest.json"
    run "$AGENTSYNC" update
    [ "$status" -eq 0 ]
    [[ "$output" == *"Updating..."* ]]
    [[ "$output" == *"Updated! v$ENGINE_VERSION → v9.9.9"* ]]
    [[ "$output" == *"What's new in v9.9.9"* ]]
    [[ "$output" == *"• Something with code."* ]]
    [[ "$output" != *"Upstream touched"* ]]
    [ "$("$AGENTSYNC" version)" = "agentsync v9.9.9" ]
    [ ! -f "$INSTALL/.update_cache" ]
    [ ! -f .ai/.pending-resolutions.yaml ]
}

@test "update native: the running version is already up to date" {
    printf '{"tag_name":"%s"}\n' "$ENGINE_VERSION" > "$FAKE_RELEASES/latest.json"
    run "$AGENTSYNC" update
    [ "$status" -eq 0 ]
    [[ "$output" == *"Already up to date! (v$ENGINE_VERSION)"* ]]
    cmp -s "$AGENTSYNC" "$NATIVE_BIN"
    [ -f "$INSTALL/.update_cache" ]
}

@test "update native: <version> pins to that release" {
    publish_release 9.9.9
    run "$AGENTSYNC" update 9.9.9
    [ "$status" -eq 0 ]
    [[ "$output" == *"Pinning to v9.9.9..."* ]]
    [ "$("$AGENTSYNC" version)" = "agentsync v9.9.9" ]
}

@test "update native: a tag that is not a release is refused" {
    run "$AGENTSYNC" update 999.0.0
    [ "$status" -eq 1 ]
    [[ "$output" == *"No AgentSync release is tagged 999.0.0."* ]]
    cmp -s "$AGENTSYNC" "$NATIVE_BIN"
}

@test "update native: a tag older than the binary releases points at the installer" {
    printf '{"ref":"refs/tags/0.1.0"}\n' > "$FAKE_RELEASES/tags/0.1.0"
    run "$AGENTSYNC" update 0.1.0
    [ "$status" -eq 1 ]
    [[ "$output" == *"0.1.0 predates the binary releases"* ]]
    [[ "$output" == *"AGENTSYNC_VERSION=0.1.0 curl -fsSL https://raw.githubusercontent.com/yelmuratoff/agent_sync/main/install.sh | bash"* ]]
    cmp -s "$AGENTSYNC" "$NATIVE_BIN"
}

@test "update native: a checksum mismatch keeps the old binary" {
    publish_release 9.9.9
    printf '0000  archive.tar.xz\n' > "$FAKE_RELEASES/9.9.9/archive.tar.xz.sha256"
    run "$AGENTSYNC" update 9.9.9
    [ "$status" -eq 1 ]
    [[ "$output" == *"checksum mismatch"* ]]
    cmp -s "$AGENTSYNC" "$NATIVE_BIN"
}

@test "update native: an unreachable GitHub is reported" {
    run env PATH="$TEST_PROJECT/nobin" "$AGENTSYNC" update 9.9.9
    [ "$status" -eq 1 ]
    [[ "$output" == *"Failed to fetch updates from GitHub."* ]]
    cmp -s "$AGENTSYNC" "$NATIVE_BIN"
}

@test "update native: a changed overridden field is reported, queued, and fails --strict" {
    "$AGENTSYNC" __catalog > catalog.dump
    grep -q '^claude ' catalog.dump
    # The dump frames each YAML by byte length; a value of the same length
    # keeps the frame intact.
    sed 's|dest: ".claude/rules"|dest: ".claude/rulez"|' catalog.dump > changed.dump
    ! cmp -s catalog.dump changed.dump
    publish_release 9.9.9 changed.dump
    mkdir -p .ai/src/tools
    printf 'targets:\n  rules:\n    dest: ".claude/my-rules"\n' > .ai/src/tools/claude.yaml

    run "$AGENTSYNC" update 9.9.9
    [ "$status" -eq 0 ]
    [[ "$output" == *"Upstream touched fields you have overridden:"* ]]
    [[ "$output" == *"◆ targets.rules.dest"* ]]
    [[ "$output" == *"base: .claude/rules → .claude/rulez"* ]]
    [[ "$output" == *"your override: .claude/my-rules"* ]]
    [[ "$output" == *"Queued in .ai/.pending-resolutions.yaml"* ]]
    grep -q "from_version: \"$ENGINE_VERSION\"" .ai/.pending-resolutions.yaml
    grep -q 'to_version: "9.9.9"' .ai/.pending-resolutions.yaml
    grep -q 'your_override: ".claude/my-rules"' .ai/.pending-resolutions.yaml

    cp "$NATIVE_BIN" "$AGENTSYNC"
    run "$AGENTSYNC" update 9.9.9 --strict
    [ "$status" -eq 1 ]
    [ "$("$AGENTSYNC" version)" = "agentsync v9.9.9" ]
}

@test "update native: __catalog frames every shipped tool by byte length" {
    run "$AGENTSYNC" __catalog
    [ "$status" -eq 0 ]
    [ "$(grep -c '^[a-z]* [0-9]*$' <<<"$output")" -ge 13 ]
    grep -q '^claude [0-9]*$' <<<"$output"
}
````

Run: `shasum -a 256 src/cli/notice.rs src/cli/update.rs tests/update_native.bats src/main.rs src/cli/mod.rs src/cli/bundle.rs bin/agentsync.sh`
Expected:

```text
900fb850088a75bfaceb5f299d80356c3bf5a3e66b1ffdbd721659248553d5e8  src/cli/notice.rs
a398f5e3648c3a3040c3449c045735c33d54490f6fdcbf7c6249f8042dd015b9  src/cli/update.rs
028a6b72b2bfe2d19444782f18c1f1a6ae05a05ee6dc1342b5cbeb96f0da1209  tests/update_native.bats
ce27587044eb6fc03030208bb8e7d83cd013aa292f17b5481ca66d9204e1632e  src/main.rs
a2b1a038d7ce78f0dcebca58dfd0f0bed6f9ac6cb1df6e626bf4b81a7faa25fc  src/cli/mod.rs
89c9d680cfbd19260d42e7530b94da0e083f79e35b7636d793044f49ae94b123  src/cli/bundle.rs
086dce3d6d5a97057059a6cde473f8328a81f625f71d8dfc105724d8cc065073  bin/agentsync.sh
```

- [x] **Step 5: The Rust gates and the release build**

```bash
cargo fmt --all --check; echo "fmt=$?"
cargo clippy --all-targets -- -D warnings > /dev/null 2>&1; echo "clippy=$?"
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release 2>&1 | tail -1
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh; echo "shellcheck=$?"
target/release/agentsync __catalog | grep -c '^[a-z]* [0-9]*$'
target/release/agentsync update --help | head -1
```

Expected: `fmt=0`, `clippy=0`, `326 passed`, `0 passed`, `11 passed`, `1 passed` (14 new: 4 in `notice`, 10 in `update`, of which 6 drive the whole flow through the fake GitHub); `Finished`; `shellcheck=0`; `13`; `Usage: agentsync update [<version>] [--strict]`.

- [x] **Step 6: The bats files**

```bash
bats --tap tests/update_native.bats
for f in update_snapshot update install changelog_render version_pin native_dispatch cli bundle customize; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" 2>&1 | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" 2>&1 | grep -c '^not ok')"
done
```

Expected: `1..11` and eleven `ok` lines (the usage, the unknown flag, the latest release replacing the binary with `Updated! v0.36.0 → v9.9.9` and `What's new in v9.9.9`, already up to date, the pin, the unknown tag, the pre-binary tag with the installer command, the checksum mismatch, the unreachable host, the conflict with its queue and `--strict` status 1, the `__catalog` framing); then `bash=0 native=0` for each of the nine files (20, 6, 6, 13, 13, 9, 8, 16, and 13 cases). Then, outside the agent sandbox (Apple `diff` refuses stdin inside it):

```bash
printf 'native_parity bash=%s native=%s\n' \
    "$(AGENTSYNC_NATIVE=0 bats --tap tests/native_parity.bats 2>&1 | grep -c '^not ok')" \
    "$(AGENTSYNC_NATIVE=1 bats --tap tests/native_parity.bats 2>&1 | grep -c '^not ok')"
```

Expected: `native_parity bash=0 native=0` (70 cases each).

- [x] **Step 7: The notice on a pty, once per engine**

Set `S="$TMPDIR/phase5d"; mkdir -p "$S"` and write `$S/notice_tty.sh`:

<!-- file: notice_tty.sh -->
````bash
#!/usr/bin/env bash
# Usage: notice_tty.sh <repo root> <out file>
# Runs `list` on a pty through `script` in a project one format revision
# behind, with an update cache newer than VERSION beside the binary and the
# checkout, and records how many times each notice prints per engine:
#   bash    — AGENTSYNC_NATIVE=0 through the dispatcher (Bash prints)
#   native  — AGENTSYNC_NATIVE=1 through the dispatcher (the binary prints)
#   direct  — the binary alone, as the installer links it
#   quiet   — the binary alone with AGENTSYNC_NO_UPDATE_CHECK=1
#   piped   — the binary alone with stdout not a terminal
set -uo pipefail
REPO="$1"; OUT="$2"
: > "$OUT"
work=$(mktemp -d "${TMPDIR:-/tmp}/agentsync_notice.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/project/.ai"
printf 'format: 1\ntools:\n  enabled: []\n' > "$work/project/.ai/agent_sync.yaml"
printf '99.0.0\n' > "$REPO/target/.update_cache"
printf '99.0.0\n' > "$REPO/.update_cache"
run_case() {
    local label="$1"; shift
    local log="$work/$label.log"
    (cd "$work/project" && script -q "$log" env "$@" > /dev/null 2>&1)
    printf '%s format=%s banner=%s usage=%s\n' "$label" \
        "$(grep -c 'migration behind' "$log")" \
        "$(grep -c 'Update available' "$log")" \
        "$(grep -c 'COMMANDS' "$log")" >> "$OUT"
}
run_case bash AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$REPO" bash "$REPO/bin/agentsync.sh" list
run_case native AGENTSYNC_NATIVE=1 AGENTSYNC_HOME="$REPO" bash "$REPO/bin/agentsync.sh" list
run_case direct "$REPO/target/release/agentsync" list
run_case direct-help "$REPO/target/release/agentsync" check --help
run_case quiet AGENTSYNC_NO_UPDATE_CHECK=1 "$REPO/target/release/agentsync" list
piped=$("$REPO/target/release/agentsync" list 2>&1 | grep -c 'migration behind\|Update available')
printf 'piped notices=%s\n' "$piped" >> "$OUT"
rm -f "$REPO/target/.update_cache" "$REPO/.update_cache"
````

Run outside the agent sandbox (`script` needs a pty): `bash "$S/notice_tty.sh" "$PWD" "$S/notice_tty.out"; cat "$S/notice_tty.out"`
Expected, six lines:

```text
bash format=1 banner=1 usage=0
native format=1 banner=1 usage=0
direct format=1 banner=1 usage=0
direct-help format=1 banner=1 usage=1
quiet format=0 banner=0 usage=0
piped notices=0
```

Bash prints both notices when it serves `list`; through the dispatcher with a binary the binary prints them and Bash does not (one of each, not two); the binary alone prints them, before the usage for `check --help`; `AGENTSYNC_NO_UPDATE_CHECK=1` and a pipe print nothing. The harness removes the two `.update_cache` files it wrote; `git status --short` shows only this task's files.

- [x] **Step 8: Commit**

```bash
git add src/cli/notice.rs src/cli/update.rs tests/update_native.bats src/main.rs src/cli/mod.rs src/cli/bundle.rs bin/agentsync.sh
git commit -m "feat(native): port update and the update notice"
```

---

### Task 3: Verify and Module Map

**Files:**
- Modify: `docs/specs/2026-09-12-rust-migration-design.md` (the dispatcher section, the `update` bullet of Phase 5, quirk 55, five accepted deviations), `.ai/src/skills/native-port/SKILL.md:51`, `.ai/src/skills/native-port/references/module-map.md` (lines 22, 41, 69, the bats table), `.ai/.sync-manifest` (regenerated)

- [x] **Step 1: Apply the docs patch and regenerate the outputs**

Save the block below as `$TMPDIR/task3.diff` and run `git apply "$TMPDIR/task3.diff"`.

<!-- file: task3.diff -->
````diff
diff --git a/docs/specs/2026-09-12-rust-migration-design.md b/docs/specs/2026-09-12-rust-migration-design.md
index 0d1d6e7..af6d6d5 100644
--- a/docs/specs/2026-09-12-rust-migration-design.md
+++ b/docs/specs/2026-09-12-rust-migration-design.md
@@ -130,7 +130,10 @@ tests/*.rs                 # cargo integration tests
 ### Dispatcher
 
 `bin/agentsync.sh` gains `_native_try`, called after `check_for_updates` and
-the `--help` interception, before the command `case`:
+the `--help` interception, before the command `case`. From Phase 5d
+`check_for_updates` runs in Bash only when `_native_will_serve` says the
+binary will not answer the command; the binary prints the same notice for
+the commands it serves.
 
 - `AGENTSYNC_NATIVE=0` — always Bash.
 - `AGENTSYNC_NATIVE=1` — require a binary; fail loudly without one.
@@ -302,7 +305,16 @@ Exit: `_NATIVE_COMMANDS` lists every command; the whole suite passes with
 - `install.sh` becomes the generated installer (or downloads the binary and
   verifies its checksum); `AGENTSYNC_VERSION=<tag>` still pins.
 - `update` replaces the binary from GitHub Releases and keeps `update <version>`
-  pinning; the `agentsync_version` gate is unchanged.
+  pinning; the `agentsync_version` gate is unchanged. The binary's `update`
+  (5d) downloads `agentsync-<target>.tar.xz` (`.zip` on Windows) and its
+  `.sha256` through `curl`, verifies the sum in-process, unpacks through
+  `tar`, asks the new binary for `version` and `__catalog`, diffs the two
+  embedded catalogs against the project's overrides, and renames the new
+  binary over the running one; the changelog comes from the archive. The
+  update banner reads `.update_cache` beside the install's `bin/`, refreshed
+  by a detached `__update-cache` run from `releases/latest`. `update` is not
+  in `_NATIVE_COMMANDS`: a checkout keeps Bash's git-based `update` until
+  Phase 6, and `tests/update_native.bats` runs the binary directly.
 - `release` bumps `VERSION`, `Cargo.toml`, and `Cargo.lock` together; the
   auto-tag workflow triggers the release build.
 - The installed `agentsync` link points at the binary. `bin/agentsync.sh`
@@ -532,6 +544,9 @@ cleanup has a list:
     one, so `--bogus --help` prints the unknown-option error, not the help.
 54. `release` exits 1 with nothing after its `Continue? [Y/n]:` prompt when
     stdin ends there: `read -r confirm` fails and errexit ends the run.
+55. `update`'s changelog renderer matches a `## <version>` heading by prefix,
+    so `## 9.9.90` renders under `9.9.9`, and a later heading that matches
+    again appends its section instead of ending the first.
 
 ## Accepted deviations
 
@@ -647,6 +662,22 @@ Appended one line at a time as they are found, with the phase:
 - Phase 5b: outside a checkout, `release` falls back to `AGENTSYNC_HOME` alone,
   when it holds a `.git`; Bash also tried the dispatcher's own checkout, which
   the binary has no counterpart for.
+- Phase 5d: `update` on a binary install has no git reconcile, no autostash
+  line, and no relink warning; it refuses an unknown tag with a link to the
+  releases page, a tag older than the binary releases with the installer
+  command that pins it, a checksum mismatch, and an archive whose binary does
+  not answer `version` or `__catalog`, each with status 1 and the old binary
+  kept. `update --help` says "the latest release" where Bash said "the latest
+  main".
+- Phase 5d: a conflict whose base value is empty before or after the update
+  keeps its five columns in the report and the queue; Bash's tab-separated
+  `read` collapsed the empty field and shifted the values.
+- Phase 5d: the changelog wraps by character count where `fold -s` counted
+  bytes (GNU) or columns (BSD); the width still comes from `tput cols`.
+- Phase 5d: the update banner's cache is `.update_cache` beside the install's
+  `bin/` (`target/.update_cache` for a developer build), refreshed from the
+  latest GitHub release rather than the newest tag; the checkout's
+  `.update_cache` stays Bash's.
 
 ## Risks
 
diff --git a/.ai/src/skills/native-port/SKILL.md b/.ai/src/skills/native-port/SKILL.md
index 95bb2f4..5c882d1 100644
--- a/.ai/src/skills/native-port/SKILL.md
+++ b/.ai/src/skills/native-port/SKILL.md
@@ -48,7 +48,7 @@ Decide in this order; the first matching line wins.
 
 - `_native_try` runs the binary as a child process, never through `exec`: the EXIT trap must still remove the run tmpdir.
 - The dispatcher passes `AGENTSYNC_ENGINE_VERSION`; a stale `target/release` build refuses to run. Run `cargo build --release` after every `VERSION` change.
-- `check_for_updates` and the format notice run in Bash before delegation and only on a terminal; the binary must not reimplement them.
+- `check_for_updates` and the format notice print once, only on a terminal: from the binary (`src/cli/notice.rs`) for the commands it serves, from Bash when `_native_will_serve` says the binary will not answer. `update` itself stays Bash-served in a checkout; the binary's `update` (Phase 5d) is for a binary install and `tests/update_native.bats` runs it directly.
 - Colour is decided once from stdout being a terminal and `NO_COLOR` being unset or empty; Bash applies the stdout decision to stderr lines too. bats never sees colours, so a terminal-only difference needs a manual check with `script` and a note in the plan.
 - `printf '%-Ns'` in Bash pads styled strings including their escape bytes; `style::pad_right` reproduces that on purpose.
 - `\n` inside a quoted YAML header stays literal until write time (`printf '%b'`). Expand it at the write, never in `yaml_subset`.
diff --git a/.ai/src/skills/native-port/references/module-map.md b/.ai/src/skills/native-port/references/module-map.md
index 7493c58..02c3c30 100644
--- a/.ai/src/skills/native-port/references/module-map.md
+++ b/.ai/src/skills/native-port/references/module-map.md
@@ -19,7 +19,7 @@ lib/helpers/resolve.sh           → (none)                  engine dir lookup;
 Tier 1
 lib/helpers/version.sh           → src/version.rs          version_pin mode, mismatch error and hint; engine_version stays in src/lib.rs
 lib/helpers/project_config.sh    → src/project_config.rs   project_config_path_r over an is_file probe; shared by sync, check, list
-lib/helpers/format.sh            → src/format_rev.rs       engine and project revision (Phase 4g), read by doctor (4j); the terminal notice stays in the dispatcher
+lib/helpers/format.sh            → src/format_rev.rs       engine and project revision (Phase 4g), read by doctor (4j); pending_notes and config_path for the notice (5d)
 lib/helpers/paths.sh             → src/paths.rs            normalise, containment (lexical for check, through the disk for sync), repo-relative, ai_dir_enclosing_root, find_workspace_ai_dirs, find_parent_ai_src; explicit source roots trusted through AGENTSYNC_EXTERNAL_SOURCE_ROOTS, escaping source-link scan
 lib/helpers/tool_resolver.sh     → src/tool.rs, src/catalog.rs, src/payload.rs; source.tools as Session::tools_dir
 lib/helpers/profiles.sh          → src/profiles.rs         names, overlay dir, tools, active, rewrite_dest
@@ -38,7 +38,8 @@ lib/helpers/backup.sh            → src/backup.rs           same on-disk layout
 lib/helpers/backup_state.sh      → src/witness.rs          after.tsv post-state-v2: print, seal, preflight, first difference
 lib/helpers/yaml_edit.sh         → src/yaml_edit.rs        set_scalar, list_append, list_remove, find_key_line (Phase 4a), remove_key (Phase 4c); rename_key waits for a caller
 lib/helpers/template_manifest.sh → src/template_manifest.rs   hash (4e); load, lookup, remove, write (4g); record and heal (4h)
-lib/helpers/snapshot.sh          → src/snapshot.rs         read_pending_pairs, clear_pending (Phase 4c); save, diff, conflicts wait for update
+lib/helpers/snapshot.sh          → src/snapshot.rs         read_pending_pairs, clear_pending (Phase 4c); diff, find_conflicts, the pending queue, utc_date (5d)
+lib/helpers/update.sh (changelog) → src/changelog.rs       md_plain, fold -s wrap, sections, versions_in_range, sort -V (5d)
 lib/helpers/prompts.sh           → src/prompts.rs          confirm on /dev/tty (Phase 3); multiselect through stty (4i)
 lib/helpers/edit_paths.sh        → src/edit_paths.rs       block for enable (Phase 4a); checklist for doctor (4j)
 
@@ -66,7 +67,8 @@ lib/helpers/generate.sh          → src/cli/generate.rs     Phase 4m, ported; p
 lib/helpers/shell_init.sh        → src/cli/shell_init.rs   Phase 4m, ported; stdout carries the snippet alone
 lib/setup_hooks.sh               → src/cli/setup_hooks.rs  Phase 4m, ported; git through the executable
 bin/agentsync.sh print_usage, the --help interception, *) → src/cli/usage.rs   Phase 5a, ported
-lib/helpers/update.sh            → src/cli/update.rs       Phase 5, binary self-replace
+lib/helpers/update.sh            → src/cli/update.rs       Phase 5d, ported for a binary install; curl, tar, and the new binary's __catalog through the executables; not in _NATIVE_COMMANDS, a checkout keeps Bash's git update until Phase 6
+lib/helpers/update.sh check_for_updates → src/cli/notice.rs   Phase 5d; the binary prints the format notice and the banner for the commands it serves, the dispatcher for the rest; __update-cache refreshes the cache
 lib/helpers/release.sh           → src/cli/release.rs      Phase 5b, ported; git through the executable, the tag message on its stdin
 .github/workflows/auto-tag.yaml  → .github/workflows/release.yml  Phase 5c; dist 0.32.0 generates it from dist-workspace.toml (workflow_dispatch), auto-tag dispatches it on the tag
 ```
@@ -162,6 +164,7 @@ tests/config_safety.bats 7    config selection fails closed before a write sync;
 tests/generate.bats 7         generate
 tests/gitignore.bats 7        unit: gitignore.sh
 tests/workspace.bats 7        sync --workspace
+tests/update_native.bats 11   update on a binary install, the binary run directly (Phase 5d)
 tests/install.bats 6          install.sh, update <version>
 tests/rollback.bats 6         rollback
 tests/update.bats 6           update
````

Run: `shasum -a 256 docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/SKILL.md .ai/src/skills/native-port/references/module-map.md; git diff --stat .ai/src docs/specs | cat`
Expected:

```text
046d95cde4a12b52b78c96f6a4d5f9539744ac3e49bdcc6bdf1a51095bac46be  docs/specs/2026-09-12-rust-migration-design.md
0e821c6d2eae2da73b1f12803bc7d0a993de33cae693a3a8ac719e5aaf8528d8  .ai/src/skills/native-port/SKILL.md
e4db8cbf15deb2d8ad1c6ca6a20a10ab72473bde83bb582b5f9c5c6e5acdd67c  .ai/src/skills/native-port/references/module-map.md
```

and the stat `SKILL.md | 2 +-`, `module-map.md | 9 +-`, the spec `| 35 ++-`.

Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force > /dev/null` (outside the sandbox if it refuses a write) and read `git status --short`: ` M .ai/.sync-manifest`, ` M .ai/src/skills/native-port/SKILL.md`, ` M .ai/src/skills/native-port/references/module-map.md`, ` M docs/specs/2026-09-12-rust-migration-design.md`, and the plan. `cmp .ai/src/skills/native-port/SKILL.md .claude/skills/native-port/SKILL.md` prints nothing.

- [x] **Step 2: Verify (outside the agent sandbox where a file says so)**

Write `$S/native_suite.sh` when it does not exist:

<!-- file: native_suite.sh -->
````bash
#!/usr/bin/env bash
# Usage: native_suite.sh <repo root> <mode 0|1|both> <out file>
# Runs every bats file one at a time under the given engine(s) and prints
# `<file> bash=<failures> native=<failures>` per file, then a total.
set -uo pipefail
REPO="$1"; WHICH="$2"; OUT="$3"
: > "$OUT"
cd "$REPO" || exit 1
total_bash=0; total_native=0
for f in tests/*.bats; do
    name="${f#tests/}"; name="${name%.bats}"
    b="-"; n="-"
    if [[ "$WHICH" == "0" || "$WHICH" == "both" ]]; then
        b=$(AGENTSYNC_NATIVE=0 bats --tap "$f" 2>&1 | grep -c '^not ok')
        total_bash=$((total_bash + b))
    fi
    if [[ "$WHICH" == "1" || "$WHICH" == "both" ]]; then
        n=$(AGENTSYNC_NATIVE=1 bats --tap "$f" 2>&1 | grep -c '^not ok')
        total_native=$((total_native + n))
    fi
    printf '%s bash=%s native=%s\n' "$name" "$b" "$n" >> "$OUT"
done
printf 'TOTAL bash=%s native=%s\n' "$total_bash" "$total_native" >> "$OUT"
````

```bash
cargo build --release 2>&1 | tail -1
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
git diff --stat HEAD -- Cargo.lock | cat
bash "$S/native_suite.sh" "$PWD" both "$S/suite_both.out" && tail -1 "$S/suite_both.out"
```

Expected: `Finished` (no source changed since Task 2's build); `326 passed`, `0`, `11`, `1`; lint exit 0; an empty stat; `TOTAL bash=0 native=0` over the 51 bats files, each run one at a time under both engines, with `native_parity` run outside the sandbox. `sync` and `check` do not change in this slice, so no timings are due.

- [x] **Step 3: Commit**

```bash
git add .ai/.sync-manifest .ai/src/skills/native-port/SKILL.md .ai/src/skills/native-port/references/module-map.md docs/specs/2026-09-12-rust-migration-design.md
git commit -m "docs(native): map the phase 5d update"
```

---

## Completion

The plan is closed when every box is ticked, `tests/update_native.bats` passes its eleven cases against the release binary, every bats file is green under both engines, the pty harness shows each notice once per engine, the crate version still equals `VERSION`, and a `## Completion receipt` records the fresh verification. The receipt lists as deferred everything only a published release or a Windows host exercises: `update` against a real GitHub release (the first is the 5e cutover), the detached refresh against the real API, and the `.exe` rename on Windows. `sync` and `check` do not change, so no timings are due. Phase 5 stays open until the 5e plan is closed as well.

## Run log

### 2026-09-18 — Phase 5d planned
- Commits: this plan.
- Verified: the whole slice was drafted in the tree, verified, parked in the session scratchpad (`phase5d/draft/`), and the tree restored to HEAD; the plan's blocks were extracted back and compared byte for byte with the parked files (`extract.sh`, `cmp`), and the three patches applied in order from HEAD with `git apply --check`. Against the draft: `cargo test` 326/0/11/1 (312 with Task 1 alone), fmt and clippy exit 0 at both points, `cargo build --release`; `tests/update_native.bats` 11/11 against the release binary with real `tar -cJf`/`tar -xf` and the `curl` stand-in; `update_snapshot`, `update`, `install`, `changelog_render`, `version_pin`, `native_dispatch`, `cli`, `bundle`, `customize` at 0 failures under both engines; `native_parity` 70/70 under both engines outside the sandbox; ShellCheck exit 0; `notice_tty.sh` outside the sandbox printed the six expected lines (each notice once for Bash-served, binary-served through the dispatcher, and the binary alone; none when quiet or piped). Bash confirmed the values the tests assert: `_md_plain`, `fold -s -w 5` on `ab cd ef` (`ab ` then `cd ef`), `_show_changelog_sections` rendering both `## 9.9.90` and `## 9.9.9` under `9.9.9` (quirk 55), `_snapshot_keys` at 26 keys, `date -u` on four epochs. Two draft bugs were found and fixed before parking: a bats `local a="$1" b="$a"` that expanded `$a` before `local` ran (the fixture tag was empty), and two test expectations that contradicted `fold` and Bash.
- Plan amended: none.
- Next: Task 0 Step 1, after the review.
- Blocker: none.

### 2026-09-18 — Tasks 0 to 3 done
- Commits: `055a650` feat(native): port the changelog renderer and the catalog diff; `2df955d` feat(native): port update and the update notice; this commit, docs(native): map the phase 5d update. The maintainer asked on 2026-09-18 to take the review decisions and finish; all eight taken as recommended.
- Verified: baseline at `1985328` (298/0/11/1, four files absent, no `_native_will_serve`, 70/20/13 cases, 50 bats files). Task 1: the four checksums as planned, fmt and clippy exit 0, `cargo test` 312/0/11/1, the `changelog` filter 9. Task 2: the seven checksums as planned, fmt and clippy exit 0, `cargo test` 326/0/11/1, `cargo build --release`, ShellCheck exit 0, `__catalog` 13 framed tools, the usage line; `update_native.bats` 11 ok; the nine touched bats files `bash=0 native=0`; `native_parity bash=0 native=0` outside the sandbox; `notice_tty.sh` the six expected lines. Task 3: the three checksums and the `2 +-`, `9 +-`, `35 ++-` stat as planned; `sync --force` outside the sandbox, the synced skill `same`; `cargo build --release` up to date; `Cargo.lock` stat empty; `native_suite.sh both` outside the sandbox over the 51 bats files: `TOTAL bash=0 native=0`.
- Plan amended: none.
- Next: close the plan: append the `## Completion receipt` and commit `docs(native): close phase 5d`.
- Blocker: none.
