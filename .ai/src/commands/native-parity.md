---
description: Diff Bash against the native engine for a ported command and triage every difference
argument-hint: "<command>"
---

Compare both engines for `$ARGUMENTS` and classify every difference.

## Current state

!`grep -n '^_NATIVE_COMMANDS=' bin/agentsync.sh`

!`ls -l target/release/agentsync target/release/agentsync.exe 2>/dev/null || echo "no release binary"`

!`grep -n "^@test \"parity: $ARGUMENTS" tests/native_parity.bats || echo "no parity fixtures for $ARGUMENTS"`

## Steps

1. If `$ARGUMENTS` is not in `_NATIVE_COMMANDS`, stop: there is nothing to compare yet, and say which plan task ports it.
2. If the binary is missing or older than the newest file under `src/`, run `cargo build --release`.
3. Run `bats tests/<file that owns the command>` twice, with `AGENTSYNC_NATIVE=0` and with `AGENTSYNC_NATIVE=1`, then `bats tests/native_parity.bats`. Show the diff `assert_parity` prints for each failure.
4. Classify each difference with the triage list in the `native-port` skill: Bash reference wrong, listed quirk, accepted deviation, or port bug.
5. Apply the fix on the side the classification names. A Bash bug gets its own `fix(<cmd>): …` commit with a regression test; a new accepted deviation is one line in `docs/specs/2026-09-12-rust-migration-design.md`; a port bug is fixed in Rust with a unit test that pins the Bash value.
6. Report a table with one row per fixture: situation, result (same, different, skipped), classification, action taken.
