---
description: Investigate and fix a GitHub issue
argument-hint: "<issue-number>"
---

Look at issue #$ARGUMENTS in this repo.

!`gh issue view $ARGUMENTS`

1. Understand the bug from the issue description and comments.
2. Trace it to the root cause — check `src/cli/<cmd>.rs`, the engine modules in `src/` (`render.rs`, `rules.rs`, `convert.rs`, `paths.rs`, `backup.rs`), and tool YAML configs.
3. Fix with the smallest correct change. Ensure portability (macOS + Linux + Windows).
4. Run `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`; `shellcheck -x -S warning -e SC1091` on a changed shell script.
5. Write or update a test that would have caught this bug: a unit test in the owning module, or a bats case in `tests/`.
6. Run `cargo build --release` and `bats tests/` to verify all tests pass.
