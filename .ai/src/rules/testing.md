# Testing Rules

Tests exercise the in-tree binary against local fixtures only. Behaviour names, deterministic setup, clean teardown.

## Framework

- `cargo test` holds the unit tests inside `src/` and the process tests in `tests/*.rs` (`assert_cmd`, `tempfile`).
- bats-core. `.bats` files live in `tests/` and drive `target/release/agentsync`; run `cargo build --release` first.
- Shared helpers in `tests/test_helper.bash`. Use `setup_test_project` / `teardown_test_project` for temp dirs.
- Run the in-tree binary via the `run_agentsync` helper (`"$AGENTSYNC_BIN" "$@"`; `AGENTSYNC_NATIVE_BIN` overrides the path) so tests exercise the working copy rather than a globally installed version.

## Test Structure

- Use `setup_test_project` for isolated unit-style command tests.
- For expensive initialized projects, use `seed_project` in `setup_file` and `clone_seed` in `setup`; each test still owns an isolated clone.
- Keep integration coverage close to the owning command (`sync.bats`, `refresh.bats`, `profiles.bats`, and similar focused files).
- A pure function gets a Rust unit test in its module; a command's stdout, stderr, and exit status get a bats case or a `tests/cli.rs` case.

## Conventions

- Name tests by behaviour verified: `@test "sync: Claude CLAUDE.md exists"`, not `@test "test_claude"`; `fn a_missing_source_warns_and_returns()` in Rust.
- Use `[ -f ... ]` and `grep -q` for assertions — bats fails on non-zero exit codes.
- Clean up temp dirs in `teardown` or `teardown_file`.
- Keep tests hermetic: local fixtures only, no network, no real GitHub calls.
- Protect the developer environment from side effects such as clipboard writes, global config changes, or edits outside `TEST_PROJECT`.
- Spell a path that lands in config or output through `host_path` so Git Bash sees what the binary prints; `skip_on_windows` (after setup) covers what Git Bash cannot stand in for, with the reason.

## When Adding a New Tool

- Add sync assertions to `tests/sync.bats` verifying output files exist.
- Add filter tests to `tests/sync_options.bats` for `--only` / `--skip`.
- Add check assertions to `tests/check.bats`.

## CI

- `native` runs fmt, clippy, `cargo test`, the release build, and the bats suite on Linux and macOS; `test-windows` builds the binary and runs the suite in twelve shards on Git Bash. A failure on one platform points to a portability issue first.
- ShellCheck runs separately on `install.sh` and `lib/templates/guard/claude.sh`: `shellcheck -x -S warning -e SC1091`.
- The full suite supports parallel execution: `bats --jobs 4 tests/ --tap`.
