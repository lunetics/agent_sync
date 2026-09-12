---
description: Write the executable plan for the next Rust migration phase from the design spec
argument-hint: "<phase-number>"
---

Write the executable plan for Phase $ARGUMENTS of the Rust migration as `docs/plans/<today YYYY-MM-DD>-rust-migration-phase-$ARGUMENTS-<slug>.md`.

## Inputs

The phase section of the design spec:

!`sed -n "/^### Phase $ARGUMENTS /,/^### Phase /p" docs/specs/2026-09-12-rust-migration-design.md`

Existing plans, the previous one is the template for header, task shape, and the Completion section:

!`ls docs/plans/ | grep rust-migration`

Ported commands:

!`grep -n '^_NATIVE_COMMANDS=' bin/agentsync.sh`

Also read the `native-port` skill with its `references/module-map.md` and `references/bash-semantics.md`, and the `writing-plans` skill.

## Steps

1. If `$ARGUMENTS` is empty or the spec section above is empty, stop and ask for the phase number. Then confirm that the previous phase's plan has no unchecked boxes and carries a completion receipt; if it does not, stop and list what is open.
2. Map the module closure of every command in this phase from the module map; list the Bash files with line ranges the tasks will read.
3. Slice vertically. Each task ends with a bats file passing under `AGENTSYNC_NATIVE=1` or a unit-tested module, never a half layer. A Bash bug found while writing fixtures is its own first task, before any port.
4. Every step carries real code, the exact command, and the expected output. No placeholders, no "similar to Task N".
5. Copy Global Constraints from the previous plan and change only what this phase changes. Write an Interfaces block per task with exact signatures, so a task can be executed without reading its neighbours.
6. Self-review: requirements coverage against the spec section, placeholder scan, type consistency across tasks, test counts in every expected-output line.

Then say `Plan saved to <path>.` and hand it over for review; implementation waits for approval.
