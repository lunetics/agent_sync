---
description: Advance the Rust migration by one unit of work and stop at a checkpoint — run it again until bats is gone
argument-hint: "[max-tasks]"
---

Advance the Bash → Rust migration by one bounded unit of work, then stop and report. Running it again continues from the repository state, so a fresh session needs nothing but this command. The default is one plan task per run; a number in `$ARGUMENTS` allows up to that many tasks in one run, still stopping at the first checkpoint below.

The engine is Rust since Phase 5 (the cutover release, 0.37.0) and the Bash engine was deleted in Phase 6. What remains is Phase 7: the bats suite becomes Rust integration tests, one bats file per commit, until `cargo test` is the whole suite.

## State

Branch, tree, and commits not yet on main:

!`git branch --show-current; git status --short | head -20; echo "ahead of main: $(git log --oneline main..HEAD 2>/dev/null | wc -l | tr -d ' ')"`

Test surface still in bats, and the Rust suite:

!`echo "bats files: $(ls tests/*.bats 2>/dev/null | wc -l | tr -d ' '), cases: $(grep -h -c '^@test' tests/*.bats 2>/dev/null | awk '{ s += $1 } END { print s + 0 }')"; echo "cargo test cases: $(grep -rh -c '^\s*#\[test\]' src tests 2>/dev/null | awk '{ s += $1 } END { print s + 0 }')"`

Plans with open, done, and receipt counts:

!`for f in docs/plans/*rust-migration-phase-*.md; do printf '%s open=%s done=%s receipt=%s\n' "$f" "$(grep -c '^- \[ \]' "$f")" "$(grep -c '^- \[x\]' "$f")" "$(grep -c '^## Completion receipt' "$f")"; done`

Design status:

!`grep -n -m1 '^Status:' docs/specs/2026-09-12-rust-migration-design.md`

Toolchain:

!`command -v cargo >/dev/null 2>&1 && cargo --version || echo "cargo missing"`

Last commits with the native scope:

!`git log --oneline -8 -E --grep='\(native\)'`

Run log of the current plan (the previous runs' handoff notes):

!`f=$(ls docs/plans/*rust-migration-phase-*.md 2>/dev/null | tail -1); [ -n "$f" ] && sed -n '/^## Run log/,$p' "$f" | tail -12 || echo "no plan yet"`

## Decide what this run does

Work through the list in order; the first matching line is this run's unit of work.

1. If the current phase is closed and its branch is not yet on `main`, stop and ask the user to merge and push. Since the cutover a closed phase is a merge point: the maintainer merges it into `main` and, when a release is due, runs `agentsync release`. Merging, pushing, and releasing are never done by this command.
2. If the current branch is not the migration branch, `git switch` to it, creating it from `main` when it does not exist. Nothing is committed on `main`.
3. If the design spec, a plan, or the `.ai/src` migration tooling (commands `native-*`, rule `native-engine.md`) is untracked, commit it on the phase branch as `docs(native): …` before anything else.
4. If `cargo` is missing, stop. Print the rustup command from Phase 1 Task 0 Step 1 and ask the user to run it. A toolchain is never installed by this command.
5. If the current phase has no plan with an unchecked task and is not closed, write the next plan by following `.ai/src/commands/native-phase-plan.md`, commit it as `docs(native): plan phase <n>`, and stop so the plan can be reviewed.
6. If the current phase has a plan with an unchecked task, execute the first one. See "Executing a task".
7. If every task of a plan is checked and that plan has no `## Completion receipt`, close it. See "Closing a phase".
8. If Phase 7 is closed and no `tests/*.bats` file exists, report that the migration is complete and stop.

Definitions. A plan is closed when it has no `- [ ]` left and carries a `## Completion receipt`. A phase is closed when every plan file for it is closed. The current phase is the highest phase number among the plan files, or that number plus one once that phase is closed. The migration branch is the one whose name starts with `feat/native-engine`; each phase since the cutover has its own (`feat/native-engine-phase-6`, `feat/native-engine-phase-7`), cut from `main` after the previous phase merged.

## Phase 7 rules

- One bats file per commit, `test(native): port <file>.bats`. The Rust test names copy the bats test names (spaces and punctuation to `_`) so a reviewer maps them one to one; the commit deletes the bats file it ported.
- The Rust tests live in `tests/` on `assert_cmd`, the shape `tests/cli.rs` already uses, against the same binary `cargo test` builds; no test spawns a shell to do what the binary does.
- A bats case whose behaviour the Rust suite already asserts is retired, not duplicated: the commit message names the Rust test that covers it. A case that only tested the Bash engine's own mechanics (its `source`d helpers, a shell shim) is retired with the reason.
- Windows stays a required check: a ported test that skips on Windows says why in its name or a comment naming the platform quirk.
- The last task of the phase deletes `tests/test_helper.bash`, drops bats and GNU parallel from CI, and removes the Windows shard matrix; the exit is `cargo test` as the whole suite and no `.bats` file left.

## Executing a task

- If the tree is dirty at the start of the run, the previous run was interrupted mid-task. Read `git status` and `git diff`, match the changes to the first unchecked task's steps, tick the steps those changes already satisfy, and continue from the first unsatisfied one. Those changes are never discarded, stashed, or reset. If some changed file belongs to no task of the current plan, stop and ask: it may be the user's own work in progress.
- Before the first step, read the spec's section for the phase, the whole plan, the task's Interfaces block, and the run log.
- Follow the task's steps in order. Run every verification command and read its output; a step is done only when the output matches the expected line. A step whose effect is already in place (a branch that exists, a file already committed by an earlier run) is ticked without repeating it.
- If an expected output does not match, fix the cause in the code. If the plan itself is wrong (a snippet does not compile, a count is off, a path moved), amend the plan file inside the same task and say so in the report.
- Tick each `- [ ]` to `- [x]` as it passes; the plan file edit goes into the task's commit. Add the plan file to the task's `git add` even though the plan's own command line does not list it.
- Commit at the task's commit step with the message the plan gives, with no attribution trailers.
- With a numeric `$ARGUMENTS`, continue with the next task up to that many, unless a checkpoint from the list above or a blocker below is reached.

## Stop and report instead of guessing

- A verification fails twice for the same cause.
- A step needs a decision the plan does not make: a new accepted deviation, a behaviour change a ported test exposes, a dependency to add.
- The sandbox or a permission blocks a write the task needs.
- CI on the branch is red for a reason the task did not introduce.

Leave the task's boxes unticked, leave the tree in a state that `cargo test` and `bats tests/` accept, and describe the blocker with the exact command and its output.

## Closing a phase

Append `## Completion receipt` to the plan file with: each Global Constraint mapped to the file that satisfies it; fresh output of `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check`, `shellcheck -x -S warning -e SC1091 install.sh lib/templates/guard/claude.sh`, and `bats --jobs 4 tests/ --tap` while any `.bats` file remains; timings on the 13-tool fixture (`scripts/perf/bench.sh`) when the phase changed `sync` or `check`; anything skipped or deferred. Commit as `docs(native): close phase <n>`, then stop for the merge.

## Run log

The chat report disappears with the session; the run log does not. Before the final commit of every run, including a run that stopped on a blocker, append one entry to a `## Run log` section at the end of the current plan file (create the section when missing):

```markdown
### <YYYY-MM-DD> — <unit of work, e.g. Task 3 done | Task 4 blocked | phase closed>
- Commits: <hashes and subjects, or none>
- Verified: <each command and its result>
- Plan amended: <what and why, or none>
- Next: <the exact task and step the next run starts from>
- Blocker: <command, output, and what is needed, or none>
```

When the run makes any commit, the entry rides in the last one, with the plan file added to it. When the run makes none (a task without a commit step, or a stop on a blocker), commit the plan file by itself as `docs(native): log run <YYYY-MM-DD>`, so the next run starts from a clean tree with the ticks and the handoff in place.

## Report

End every run with the same six items as the run log entry, in that order, and nothing else.
