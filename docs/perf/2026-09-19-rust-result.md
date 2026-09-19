# Rust engine result, 2026-09-19

What the migration bought, measured against
[the Bash baseline](2026-09-13-bash-baseline.md) it was judged by. Both engines
run here on one host, one fixture, one afternoon — the Bash engine from its last
release, the binary from `main` after the bats suite was retired.

Reproduce with:

```bash
cargo build --release
git worktree add /tmp/agentsync-0.37.0 0.37.0
AGENTSYNC_BASH_CLI=/tmp/agentsync-0.37.0/bin/agentsync.sh \
  bash scripts/perf/bench.sh --runs 3 --engines both
```

## Fixture

`scripts/perf/make-fixture.sh` with its defaults, the same generator the
baseline used: 96 skills with 2 reference files each, 70 rules, 30 commands,
one `AGENTS.md` — **389 source files**, and all 13 shipped tools enabled. One
`sync` writes **3465 managed files**. The generator is committed and its output
is byte-identical for the same arguments, so this table and the baseline's
measure the same input.

## Commands

Bash 0.37.0 against the binary at `2c80441`, rustc 1.98.1, macOS aarch64, 3 runs
per cell, best and median wall seconds. `sync` ran once before the table, so
every row measures an already-synced project — the state a user is in most of
the time.

| Command | Bash 0.37.0 | Rust | Faster by |
| --- | --- | --- | --- |
| `list` | 0.57 / 0.74 | under 0.01 | ~60× |
| `check` | 73.64 / 90.47 | 0.25 / 0.26 | ~290× |
| `sync` | 67.54 / 69.54 | 2.54 / 2.69 | ~27× |
| `sync --if-stale` | 0.18 / 0.20 | 0.01 / 0.01 | ~18× |

Startup alone, `version` on an empty directory, 50 runs each: **38.3 ms** in
Bash, **4.8 ms** as a binary. That floor is what every command paid before it
did any work, and it is why `list` gains as much as it does.

`check` is the row that changes how the tool is used. At 74 seconds it could
not sit in a pre-commit hook or a CI gate without being felt; at a quarter of a
second it is free. The gap is wider than `sync`'s because `check` is almost
entirely reading and hashing — the part Bash paid a fork for, per file.

## Why the numbers moved

The baseline's diagnosis was that the engine is fork-bound: system time exceeded
user time, because every value read from a YAML file, every path resolved, and
every hash computed spawned a process. The Rust engine does the same work in
one process with no subprocess at all on the `sync` and `check` paths, and the
13-tool catalog is resolved once per run instead of per tool per field.

Two smaller effects ride along: the tool templates are embedded in the binary
with `include_dir!`, so a run touches no engine directory on disk, and the
version comes from a compiled-in constant rather than a file read at startup.

## The rest of the ledger

| | Bash 0.37.0 | Rust |
| --- | --- | --- |
| Shell in the repository | 18,566 lines | 390 lines |
| Shipped as | a clone plus `bash` | one 2.0 MB static binary |
| Platforms | macOS, Linux, Git Bash | macOS (arm64, x86_64), Linux (x86_64, arm64), Windows (x86_64) |
| Test suite | 41 bats files, 727 cases | `cargo test`: 1066 tests, 42 integration files |
| Windows CI | 12 sharded jobs, 29 job-minutes | 1 job, 5 minutes |
| Runtime dependencies | `bash`, coreutils | none |

The 390 lines of shell that stay are the two a binary cannot replace: the
installer, which runs before a binary exists, and the guard hook, which runs in
a teammate's checkout where the CLI is not installed.

## Caveats

- One host, one fixture. The absolute seconds are a MacBook's; the ratios are
  what carries to another machine.
- The Bash column is 0.37.0, the last release that shipped the Bash engine. It
  is not 0.35.2, the version the spec's motivating table measured — see the
  baseline's "What this does not compare to" for why that table is not a
  baseline either.
- `check` at 90 seconds median against 74 best shows the Bash engine's spread
  under a loaded machine. The binary's spread is under 10 ms on the same runs.
