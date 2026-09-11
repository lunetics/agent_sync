#!/usr/bin/env bats
# Two-clone team workflow over a bare origin, in both outputs modes.
#
# local: every clone regenerates outputs, so the manifest (what *this* clone
# generated) must be gitignored with them — a committed manifest beside
# ignored outputs makes every pull look like a manual edit.
# committed: outputs and the manifest travel through git; a teammate who never
# runs agentsync still gets current rules from git pull.

load test_helper

setup() {
    TEST_PROJECT="$(mktemp -d "${TMPDIR:-/tmp}/agentsync_team.XXXXXX")"
    export AGENTSYNC_HOME="$REPO_ROOT"
    export GIT_AUTHOR_NAME=Test GIT_AUTHOR_EMAIL=test@test.com
    export GIT_COMMITTER_NAME=Test GIT_COMMITTER_EMAIL=test@test.com
    git init --quiet --bare "$TEST_PROJECT/origin.git"
}

teardown() {
    teardown_test_project
}

# Clone a: init in the given mode, sync, commit, push. Clone b: fresh clone.
#
# git's stderr is kept: discarding it here once made an intermittent failure on
# the macOS runner undiagnosable — bats reported only "status 128" from a step
# whose own explanation had been thrown away. _git_step names the step instead.
seed_team() {
    local mode="$1"
    _git_step "clone a" clone --quiet "$TEST_PROJECT/origin.git" "$TEST_PROJECT/a"
    cd "$TEST_PROJECT/a"
    run_agentsync init --tools claude --yes --outputs "$mode" >/dev/null 2>&1
    run_agentsync sync >/dev/null 2>&1
    _git_step "add" add -A
    _git_step "commit" commit --quiet -m "init agentsync"
    _git_step "push" push --quiet -u origin HEAD
    _git_step "clone b" clone --quiet "$TEST_PROJECT/origin.git" "$TEST_PROJECT/b"
}

# Run one git step, and on failure say which step it was and what git printed.
_git_step() {
    local label="$1"
    shift
    local out status
    out=$(git "$@" 2>&1)
    status=$?
    if [[ $status -ne 0 ]]; then
        printf 'git %s failed (status %s) in %s\n' "$label" "$status" "$PWD" >&2
        printf '%s\n' "$out" >&2
        return "$status"
    fi
}

# Clone a edits a rule, syncs, and pushes.
push_rule_edit() {
    cd "$TEST_PROJECT/a"
    printf '\n- Team rule added by a.\n' >> .ai/src/rules/core.md
    run_agentsync sync >/dev/null 2>&1
    _git_step "add" add -A
    _git_step "commit" commit --quiet -m "rules: add team rule"
    _git_step "push" push --quiet
}

# ── local mode ──────────────────────────────────────────────────────────────

@test "team/local: sync manifest is gitignored alongside the outputs it describes" {
    seed_team local
    cd "$TEST_PROJECT/a"
    git check-ignore -q .ai/.sync-manifest
    ! git ls-files --error-unmatch .ai/.sync-manifest >/dev/null 2>&1
}

@test "team/local: a rule edit commits only the source, not the manifest" {
    seed_team local
    push_rule_edit
    cd "$TEST_PROJECT/a"
    git show --stat --format= HEAD | grep -q "rules/core.md"
    ! git show --stat --format= HEAD | grep -q "sync-manifest"
}

@test "team/local: teammate's rule edit syncs after git pull without a drift refusal" {
    seed_team local
    cd "$TEST_PROJECT/b"
    run_agentsync sync >/dev/null 2>&1
    push_rule_edit
    cd "$TEST_PROJECT/b"
    _git_step "pull" pull --quiet
    run run_agentsync sync
    [ "$status" -eq 0 ]
    grep -q "Team rule added by a" .claude/rules/core.md
}

@test "team/local: manual edit of a generated file is still refused after a pull" {
    seed_team local
    cd "$TEST_PROJECT/b"
    run_agentsync sync >/dev/null 2>&1
    push_rule_edit
    cd "$TEST_PROJECT/b"
    _git_step "pull" pull --quiet
    run_agentsync sync >/dev/null 2>&1
    printf '\n# hand edit\n' >> .claude/rules/core.md
    run run_agentsync sync
    [ "$status" -ne 0 ]
    [[ "$output" == *"Manual edits detected"* ]]
}

# ── committed mode ──────────────────────────────────────────────────────────

@test "team/committed: a fresh clone has generated outputs without running agentsync" {
    seed_team committed
    cd "$TEST_PROJECT/b"
    [ -f CLAUDE.md ]
    [ -f .claude/rules/core.md ]
    [ -f .ai/.sync-manifest ]
}

@test "team/committed: a rule edit commits the source, its outputs, and the manifest" {
    seed_team committed
    push_rule_edit
    cd "$TEST_PROJECT/a"
    git show --stat --format= HEAD | grep -q ".ai/src/rules/core.md"
    git show --stat --format= HEAD | grep -q ".claude/rules/core.md"
    git show --stat --format= HEAD | grep -q "sync-manifest"
}

@test "team/committed: git pull alone delivers a teammate's rule edit" {
    seed_team committed
    push_rule_edit
    cd "$TEST_PROJECT/b"
    _git_step "pull" pull --quiet
    grep -q "Team rule added by a" .claude/rules/core.md
    run run_agentsync check
    [ "$status" -eq 0 ]
}

@test "team/committed: sync after a pull is a clean no-op" {
    seed_team committed
    push_rule_edit
    cd "$TEST_PROJECT/b"
    _git_step "pull" pull --quiet
    run run_agentsync sync
    [ "$status" -eq 0 ]
    [ -z "$(git status --porcelain)" ]
}
