---
name: native-port
description: Port an AgentSync command from the Bash engine to the Rust native engine behind the strangler in bin/agentsync.sh — map the Bash module closure, write parity fixtures first, port core modules with unit tests that mirror lib/helpers semantics, wire clap and _NATIVE_COMMANDS, and prove byte-identical output with AGENTSYNC_NATIVE=1 bats and tests/native_parity.bats. Use this skill when the user asks to port, migrate, or make a command native or Rust, mentions the Rust migration, the native engine, parity, AGENTSYNC_NATIVE, or a rust-migration plan under docs/plans, or when a parity test or a native bats run differs from Bash — even when phrased as "move check to Rust", "why does native list differ", or "continue the migration".
---

# Native Port

Port one Bash command to the Rust engine so it reads, writes, prints, and exits exactly as Bash does. The design is `docs/specs/2026-09-12-rust-migration-design.md`; the current phase plan is the newest `docs/plans/*rust-migration-phase-*.md`. Read both before the first edit, and work from the plan's task for the command when one exists.

## Bundled references (load on demand)

- `references/module-map.md` — read when mapping a command's Bash module closure to Rust modules, choosing the next module to port, or looking up which bats file owns a command.
- `references/bash-semantics.md` — read when porting a helper or a message: the YAML reading rules, config and tool layering, path containment, manifest and backup formats, the two output voices, exit codes, and environment variables, each with the Bash line that proves it.

## Steps

1. **Fix the target.** Find the command's `case` arm with `grep -n -E "^\s+([a-z|-]+\|)?<cmd>(\|[a-z|-]+)?\)" bin/agentsync.sh`. The `_need` list on that line plus every `source` at the top of the helpers it names is the module closure. `sync`, `check`, and `setup-hooks` run `lib/<script>.sh` in a subprocess and source their own list there.
2. **Inventory the behaviour before writing Rust.** For every branch record the exact stdout and stderr text, the exit code, the files read and written, the environment variables consulted, and whether the branch is gated on a terminal (`[[ -t 1 ]]`, `_use_colors`, `check_for_updates`) or reads `/dev/tty`. The command's bats file is the first source; the code is the second.
3. **Write the parity fixtures first.** Add one `@test "parity: <cmd> …"` per distinct situation to `tests/native_parity.bats`, using `assert_parity`. Run the Bash side alone (`AGENTSYNC_NATIVE=0 bats tests/<cmd>.bats`) and read the reference output. If Bash misbehaves on a fixture (a silent exit 1, a locale-dependent order, a leaked temp file), fix Bash in its own `fix(<cmd>): …` commit with a regression test in the command's bats file before any porting. The parity suite needs a correct reference.
4. **Port the core modules the command needs, one module per commit.** Mirror the Bash helper line for line, including the quirks listed in the design spec; improving behaviour while porting is not allowed. Unit tests live inline in `#[cfg(test)]` and assert the value the Bash helper produces. When the expected value is not obvious, run the helper (`bash -c 'source lib/helpers/x.sh; fn args'`) and paste its answer into the assertion.
5. **Write the command in `src/cli/<cmd>.rs`** as `render(…) -> Result<String, Error>` that unit tests assert on, plus `run(…, &mut impl Write) -> Result<(), Error>`. Add the clap variant in `src/cli/mod.rs` with `disable_help_flag = true` (Bash owns `--help`) and the match arm in `src/main.rs`. Every Bash message and exit code goes through `Error` or an explicit `ExitCode` in `main.rs`; core modules never print.
6. **Declare the command ported.** Add its name and aliases to `_NATIVE_COMMANDS` in `bin/agentsync.sh`. From this commit on a present binary answers the command, so the same commit must carry the passing runs below.
7. **Prove parity.** Run in this order and read every line:
   - `cargo test && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings`
   - `cargo build --release`
   - `bats tests/<cmd>.bats` and then `AGENTSYNC_NATIVE=1 bats tests/<cmd>.bats`
   - `bats tests/native_parity.bats`
   - `AGENTSYNC_NATIVE=1 bats --jobs 4 tests/` before the phase closes
8. **Triage every difference** with the list below. Record an accepted deviation as one line under "Accepted deviations" in the design spec, in the same commit as the code.
9. **Close.** Tick the plan's checklist, commit as `feat(native): port <cmd>`, and report which bats files ran in which mode.

## Triage of a parity difference

Decide in this order; the first matching line wins.

- If the Bash output depends on locale, PID, timestamp, or on which hash tool is installed, fix Bash first (Step 3). The reference is wrong.
- If the Bash behaviour is listed under "Known quirks" in the design spec, reproduce it in Rust with a unit test named `…_like_bash_does` that cites the quirk number.
- If the difference is clap rejecting an argument Bash ignored, or byte order versus locale sort, it is already accepted; cite the spec line.
- Otherwise the port is wrong. Fix Rust.

## After a command is native

- Its parity tests document the cutover state. A feature that changes the command's output lands in Rust with its bats assertions updated, and the parity test for that situation is retired in the same commit, because the Bash twin stops being the reference.
- The Bash implementation stays in place, untouched, until Phase 6.
- A feature on an unported command lands in Bash only.

## Gotchas

- `_native_try` runs the binary as a child process, never through `exec`: the EXIT trap must still remove the run tmpdir.
- The dispatcher passes `AGENTSYNC_ENGINE_VERSION`; a stale `target/release` build refuses to run. Run `cargo build --release` after every `VERSION` change.
- `check_for_updates` and the format notice print once, only on a terminal: from the binary (`src/cli/notice.rs`) for the commands it serves, from Bash when `_native_will_serve` says the binary will not answer. `update` itself stays Bash-served in a checkout; the binary's `update` (Phase 5d) is for a binary install and `tests/update_native.bats` runs it directly.
- Colour is decided once from stdout being a terminal and `NO_COLOR` being unset or empty; Bash applies the stdout decision to stderr lines too. bats never sees colours, so a terminal-only difference needs a manual check with `script` and a note in the plan.
- `printf '%-Ns'` in Bash pads styled strings including their escape bytes; `style::pad_right` reproduces that on purpose.
- `\n` inside a quoted YAML header stays literal until write time (`printf '%b'`). Expand it at the write, never in `yaml_subset`.
- A Bash function whose last command is a failed `[[ ]]` returns 1, and under `set -e` a `$(...)` caller dies silently. When a Bash command exits 1 with no message, look there first.
- `AGENTSYNC_REPO_ROOT` wins over the working directory in both engines; the tests rely on it.
- Bash keeps the logical `$PWD` for a symlinked project root; Rust's `current_dir()` is physical. Honour `$PWD` when it names the same directory, or manifest and display paths change.
- Under Git Bash on Windows the POSIX paths in `AGENTSYNC_REPO_ROOT` and `TMPDIR` never reach a native executable. Native bats runs stay off Windows until the binary is the entry point (Phase 5); `cargo test` still runs there.
- Templates are embedded with `include_dir!`, so `DEFAULT_REPO_ROOT` has no Rust equivalent. A Bash path under `lib/templates/` becomes a `catalog::` lookup.
- Interactive prompts read `/dev/tty` in Bash, never stdin. A ported prompt must do the same, or captured-output flows change behaviour.
