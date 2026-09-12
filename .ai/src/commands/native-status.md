---
description: Report where the Rust migration stands and name the next task
---

Report the state of the Bash → Rust migration from the facts below; do not start any task.

## Ported commands

!`grep -n '^_NATIVE_COMMANDS=' bin/agentsync.sh || echo "no dispatcher yet: Phase 1 Task 2 not started"`

## Plans and their checklists

!`ls docs/plans/ | grep rust-migration`

Done:

!`grep -c '^- \[x\]' docs/plans/*rust-migration-phase-*.md`

Open:

!`grep -c '^- \[ \]' docs/plans/*rust-migration-phase-*.md`

## Accepted deviations

!`sed -n '/^## Accepted deviations/,/^## Risks/p' docs/specs/2026-09-12-rust-migration-design.md`

## Recent commits with the native scope

!`git log --oneline -15 -E --grep='^(feat|fix|test|refactor|docs|chore)\(native\)'`

## Crate and binary

!`ls src/cli 2>/dev/null || echo "no crate yet"`

!`ls -l target/release/agentsync target/release/agentsync.exe 2>/dev/null || echo "no release binary"`

## Report

In this order, each as one or two sentences:

1. The phase in progress and its plan file.
2. Commands served natively versus still Bash, from `_NATIVE_COMMANDS` against the command list in `bin/agentsync.sh`.
3. Checklist progress per plan file: done and open counts.
4. The count of accepted deviations.
5. The next unchecked task and step, quoted from the plan with its file path.
6. Anything blocking: missing toolchain, a red CI run, a plan with no completion receipt, a phase without a plan.

Then stop.
