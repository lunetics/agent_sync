# Bash engine baseline, 2026-09-13

The number to compare the finished migration against. Taken while the Bash
engine still answers every command except `version` and `list`, on a fixture
that is generated rather than borrowed, so the same measurement can be repeated
at the cutover and mean the same thing.

Reproduce with:

```bash
cargo build --release      # needed for the native column
bash scripts/perf/bench.sh --runs 3
```

## Fixture

`scripts/perf/make-fixture.sh` with its defaults: 96 skills with 2 reference
files each, 70 rules, 30 commands, one `AGENTS.md` — **389 source files**,
1.5 MB, and all 13 shipped tools enabled. One `sync` writes **3465 managed
files** (lines in `.ai/.sync-manifest`).

The generator is the pin. Its output is byte-identical for the same arguments on
any machine, and it lives in the repository, so a measurement in Phase 6 sees
exactly this input. Nothing personal is committed: the content is filler text.

## Measurements

Engine at `4be1f83`, rustc 1.98.1, macOS aarch64, 3 runs per cell, best and
median wall seconds. `sync` ran once before the table, so every row measures an
already-synced project — the state a user is in most of the time.

| Command | Bash | Native |
| --- | --- | --- |
| `list` | 0.53 / 0.54 | 0.02 / 0.02 |
| `check` | 63.33 / 64.55 | 63.19 / 64.05 |
| `sync` | 52.56 / 53.99 | 54.83 / 54.93 |
| `sync --if-stale` | 0.12 / 0.12 | 0.12 / 0.12 |
| `list`, binary without the Bash entry point | — | under 0.005 |

`check` and `sync` are not ported, so their native column is the Bash engine
reached through `_native_try`'s fall-through; the two columns agreeing there is
the control that says the harness measures what it claims. `list` is the only
ported command in the table, and the gap between its native column and the
binary-only row — about 20 ms — is the Bash entry point starting up, resolving
`VERSION` and sourcing three helpers before it can delegate. That tax
disappears at the Phase 5 cutover.

## What this does not compare to

The design spec's table (`docs/specs/2026-09-12-rust-migration-design.md`)
records `sync` at 6.2–6.7 s and `check` at 7.0 s for 0.35.2. Those numbers are
not a baseline this one continues, for three reasons found while building the
harness:

- The spec says the fixture was "this repository's own `.ai/src/` (392 source
  files, 98 skills)". This repository's `.ai/src/` holds 38 files and syncs 74
  managed files with 2 tools enabled. 392 files and 96 skills is `~/.ai/src`,
  the maintainer's global source — outside the repository, and edited whenever
  the maintainer edits their own rules.
- The spec's row reads "13 tools, 272 outputs". The same source size here
  produces 3465 managed files. Whatever the 272 counted, it was not manifest
  lines, and the discrepancy is unexplained.
- No method is recorded with those numbers: run count, cold or warm, machine
  load.

Treat the spec's table as the observation that motivated the migration — system
time exceeding user time, the engine being fork-bound — and this file as the
measurement the result is judged against.
