---
name: "fix-issue"
description: >-
  Investigate and fix a GitHub issue in this repo — pull the issue with `gh`,
  trace it to the root cause across `src/`, fix portably, and cover it with a
  regression test. Use this skill when the user asks to fix,
  resolve, or tackle a GitHub issue (by number or URL), debug a reported bug
  from the tracker, or work through an issue end-to-end — including phrasings
  like "fix #42", "tackle issue 17", "resolve this GH issue",
  "figure out the bug from the ticket".
---

# Fix Issue

Resolve a reported GitHub issue: understand it, trace to the root cause, fix portably, and lock the fix in with a test.

## Steps

1. **Pull the issue** — `gh issue view <number>` for title, body, comments. If the user pasted a URL, extract the number from it.
2. **Reproduce** — run the failing flow against a fresh fixture (use `setup_test_project` from `tests/test_helper.bash` so you don't contaminate the working tree). A bug you can't reproduce is a bug you can't verify fixed.
3. **Trace to the root cause** — work outward from the symptom: `src/main.rs` (command routing) → `src/cli/<cmd>.rs` (the command) → `src/render.rs` (per-tool loop) → the engine modules (`file_ops`, `yaml_subset`, `rules`, `convert`, `paths`, `backup`) → `.ai/src/tools/*.yaml`. Don't stop at the first plausible cause.
4. **Fix with the smallest correct change** — touch only the code that owns the bug. Three similar lines beat a premature abstraction.
5. **Keep it portable** — engine paths stay `/`-separated strings through `paths::from_disk`; no `PathBuf` formatted into output. In a shell script: POSIX `sed` (write-then-`mv`), `cd "$(dirname "$path")" && pwd` instead of `realpath`, quoted `"$var"`, `local` in functions, `set -euo pipefail` stays on.
6. **Lint** — `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings`; `shellcheck -x -S warning -e SC1091 <changed-scripts>` for shell. Resolve warnings; don't paper over with `#[allow]` or `# shellcheck disable=`.
7. **Add a regression test** — fails on `main`, passes on the fix. A pure function gets a unit test in its module, named by behaviour; a command's output gets a bats case: `@test "sync: <what the bug broke> stays correct"`, hermetic temp dirs via `setup_test_project` / `teardown_test_project`.
8. **Run the full suite** — `cargo test`, then `cargo build --release` and `bats tests/`. Every test green, not just the new one.
9. **Report the result** — summarize the root cause, changed files, regression test, and verification. Commit only when the user asks; use `Fixes #<issue-number>` in the commit or PR body when automatic closure is intended.

## Gotchas

- Issue authors describe the *symptom*, not the *cause*. "Sync deletes my file" might actually be "cleanup runs on a stale manifest" — fix the chain, not the wording.
- The suite runs on macOS, Linux, and Git Bash on Windows in CI. A path assertion that passes on macOS can fail on Windows when it bypasses `host_path`; a test helper using `sed -i ''` will fail on Linux.
- Tool-specific bugs (cursor, codex, …) — pin the regression test to that tool, but run the full suite so the fix doesn't regress the others.
- Idempotency bugs (sync output differs on the second run) are usually a directory listing that is not sorted byte-wise, or temp filenames based on PID/timestamp. Look there first.
- A fix without a test is a regression waiting to happen. Skipping the test step is non-negotiable.
