---
description: Port one AgentSync command from Bash to the Rust engine with parity proof
argument-hint: "<command>"
---

Port `$ARGUMENTS` to the native engine. Follow the `native-port` skill end to end; the parity fixtures come before any Rust.

## Current state

Ported commands:

!`grep -n '^_NATIVE_COMMANDS=' bin/agentsync.sh`

Bash dispatch lines for the command (the `case` arms that name it):

!`grep -n -E "^\s+([a-z|-]+\|)?$ARGUMENTS(\|[a-z|-]+)?\)" bin/agentsync.sh`

bats files that mention it:

!`grep -l -w "$ARGUMENTS" tests/*.bats`

Native commands already in the crate:

!`ls src/cli 2>/dev/null || echo "no src/cli yet"`

## Steps

1. Read `docs/specs/2026-09-12-rust-migration-design.md` and the newest `docs/plans/*rust-migration-phase-*.md`. If no plan task covers `$ARGUMENTS`, stop and say which phase owns it instead of porting ahead of the plan.
2. Inventory the Bash behaviour (skill Step 2), then add the parity fixtures to `tests/native_parity.bats` and run the Bash side. If Bash misbehaves on a fixture, fix Bash first in its own `fix(<cmd>): …` commit with a regression test.
3. Port the missing core modules one commit each, then the command file, the clap variant, the `main.rs` arm, and the `_NATIVE_COMMANDS` entry.
4. Run the proof sequence from the skill and read every line. Triage each difference with the skill's list; record an accepted deviation in the spec in the same commit.
5. Tick the plan's checklist, commit as `feat(native): port $ARGUMENTS`, and report: files changed, each bats file with the mode it ran in, deviations recorded, and the next unchecked plan step.
