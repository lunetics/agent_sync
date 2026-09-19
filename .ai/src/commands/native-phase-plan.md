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

Test surface still in bats, and the Rust suite:

!`echo "bats files: $(ls tests/*.bats 2>/dev/null | wc -l | tr -d ' '), cases: $(grep -h -c '^@test' tests/*.bats 2>/dev/null | awk '{ s += $1 } END { print s + 0 }')"; echo "cargo test cases: $(grep -rh -c '^\s*#\[test\]' src tests 2>/dev/null | awk '{ s += $1 } END { print s + 0 }')"`

Also read `tests/cli.rs` (the shape every ported test takes), `tests/test_helper.bash` (the fixtures the bats files share), and the `writing-plans` skill.

## Steps

1. If `$ARGUMENTS` is empty or the spec section above is empty, stop and ask for the phase number. Then confirm that the previous phase's plan has no unchecked boxes and carries a completion receipt; if it does not, stop and list what is open.
2. Map the closure of every bats file in this phase: its cases, the helpers it uses from `tests/test_helper.bash`, the Rust tests that already assert the same behaviour, and the cases that only tested the Bash engine's own mechanics.
3. Slice vertically. Each task ends with one bats file ported and deleted, its Rust tests green under `cargo test`, never a half file. A behaviour change a ported test exposes is its own first task, before any port.
4. Every step carries real code, the exact command, and the expected output. No placeholders, no "similar to Task N".
5. Copy Global Constraints from the previous plan and change only what this phase changes. Write an Interfaces block per task with exact signatures, so a task can be executed without reading its neighbours. Every verification block runs `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`, and `bats --jobs 4 tests/ --tap` while `.bats` files remain.
6. Self-review: requirements coverage against the spec section, placeholder scan, type consistency across tasks, test counts in every expected-output line.

Then say `Plan saved to <path>.` and hand it over for review; implementation waits for approval.
