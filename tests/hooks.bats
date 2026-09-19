#!/usr/bin/env bats
# Tests for agentsync setup-hooks: which hooks each outputs mode gets, that
# they land where git actually looks, and that the committed-mode pre-commit
# gate blocks a commit whose generated files lag the source.

load test_helper

setup() {
    setup_test_project
}

teardown() {
    teardown_test_project
}

# Scaffold a project in <mode> without templates or a first sync — these tests
# only need agent_sync.yaml and a git repo.
init_mode() {
    run_agentsync init --no-detect --no-templates --yes --outputs "$1" --no-sync >/dev/null 2>&1
}

# Put an `agentsync` on PATH that runs the working copy, so an installed hook
# can call it. Also keeps the developer's real install out of the test.
shim_agentsync_on_path() {
    mkdir -p "$TEST_PROJECT/bin"
    printf '#!/bin/sh\nexec "%s" "$@"\n' "$AGENTSYNC_BIN" > "$TEST_PROJECT/bin/agentsync"
    chmod +x "$TEST_PROJECT/bin/agentsync"
    export PATH="$TEST_PROJECT/bin:$PATH"
}

# ── local mode: sync after pull/checkout ────────────────────────────────────

@test "setup-hooks: local mode creates post-merge and post-checkout hooks" {
    init_mode local
    run run_agentsync setup-hooks
    [ "$status" -eq 0 ]
    [ -x ".git/hooks/post-merge" ]
    [ -x ".git/hooks/post-checkout" ]
    grep -q "AGENTSYNC AUTO SYNC" .git/hooks/post-merge
    grep -q "AGENTSYNC AUTO SYNC" .git/hooks/post-checkout
}

@test "setup-hooks: local mode hook invokes the installed binary" {
    init_mode local
    run_agentsync setup-hooks >/dev/null
    grep -q "command -v agentsync" .git/hooks/post-merge
    grep -q "agentsync sync" .git/hooks/post-merge
}

@test "setup-hooks: local mode hook is non-fatal on sync failure" {
    init_mode local
    run_agentsync setup-hooks >/dev/null
    grep -q "agentsync sync ||" .git/hooks/post-merge
}

@test "setup-hooks: local mode is idempotent" {
    init_mode local
    run_agentsync setup-hooks >/dev/null
    run run_agentsync setup-hooks
    [ "$status" -eq 0 ]
    local count
    count=$(grep -c "AGENTSYNC AUTO SYNC START" .git/hooks/post-merge)
    [ "$count" -eq 1 ]
}

@test "setup-hooks: local mode preserves existing hook content" {
    init_mode local
    printf '#!/bin/sh\necho "existing hook"\n' > .git/hooks/post-merge
    chmod +x .git/hooks/post-merge

    run run_agentsync setup-hooks
    [ "$status" -eq 0 ]
    grep -q "existing hook" .git/hooks/post-merge
    grep -q "AGENTSYNC AUTO SYNC" .git/hooks/post-merge
}

@test "setup-hooks: local mode installs no pre-commit hook by default" {
    init_mode local
    run run_agentsync setup-hooks
    [ "$status" -eq 0 ]
    [ ! -f ".git/hooks/pre-commit" ]
}

@test "setup-hooks: local mode --pre-commit uses --if-stale" {
    init_mode local
    run run_agentsync setup-hooks --pre-commit
    [ "$status" -eq 0 ]
    [ -x ".git/hooks/pre-commit" ]
    grep -q "agentsync sync --if-stale" .git/hooks/pre-commit
}

# ── committed mode: keep outputs in the same commit ─────────────────────────

@test "setup-hooks: committed mode installs only a pre-commit gate" {
    init_mode committed
    run run_agentsync setup-hooks
    [ "$status" -eq 0 ]
    [ -x ".git/hooks/pre-commit" ]
    [ ! -f ".git/hooks/post-merge" ]
    [ ! -f ".git/hooks/post-checkout" ]
}

@test "setup-hooks: committed mode gate reads the manifest" {
    init_mode committed
    run_agentsync setup-hooks >/dev/null
    grep -q ".ai/.sync-manifest" .git/hooks/pre-commit
    grep -q "git add -A" .git/hooks/pre-commit
}

@test "setup-hooks: every hook honours AGENTSYNC_SKIP_HOOKS" {
    init_mode committed
    run_agentsync setup-hooks >/dev/null
    grep -q "AGENTSYNC_SKIP_HOOKS" .git/hooks/pre-commit
}

@test "setup-hooks: committed gate blocks a commit whose outputs lag the source" {
    skip_on_windows "the gate's agentsync shim is a shell script; git on Windows finds the real one first"
    shim_agentsync_on_path
    run_agentsync init --tools claude --yes >/dev/null 2>&1
    run_agentsync setup-hooks >/dev/null
    git add -A
    git commit --quiet -m "add agentsync"

    printf '\n- A rule only in the source.\n' >> .ai/src/rules/core.md
    git add .ai/src/rules/core.md

    run git commit -m "rules: edit source only"
    [ "$status" -ne 0 ]
    [[ "$output" == *"out of date"* ]]
    [[ "$output" == *".claude/rules/core.md"* ]]
}

@test "setup-hooks: committed gate passes once the outputs are staged" {
    shim_agentsync_on_path
    run_agentsync init --tools claude --yes >/dev/null 2>&1
    run_agentsync setup-hooks >/dev/null
    git add -A
    git commit --quiet -m "add agentsync"

    printf '\n- A rule only in the source.\n' >> .ai/src/rules/core.md
    run_agentsync sync >/dev/null 2>&1
    git add -A

    run git commit -m "rules: edit source and outputs"
    [ "$status" -eq 0 ]
    git show --stat --format= HEAD | grep -q ".claude/rules/core.md"
}

@test "setup-hooks: AGENTSYNC_SKIP_HOOKS=1 lets the commit through" {
    shim_agentsync_on_path
    run_agentsync init --tools claude --yes >/dev/null 2>&1
    run_agentsync setup-hooks >/dev/null
    git add -A
    git commit --quiet -m "add agentsync"

    printf '\n- A rule only in the source.\n' >> .ai/src/rules/core.md
    git add .ai/src/rules/core.md

    run env AGENTSYNC_SKIP_HOOKS=1 git commit -m "rules: source only, on purpose"
    [ "$status" -eq 0 ]
}

# ── core.hooksPath and argument handling ────────────────────────────────────

@test "setup-hooks: refuses to write when core.hooksPath points elsewhere" {
    init_mode local
    mkdir -p .githooks
    git config core.hooksPath .githooks

    run run_agentsync setup-hooks
    [ "$status" -eq 0 ]
    [[ "$output" == *"core.hooksPath"* ]]
    [[ "$output" == *"sync --if-stale"* ]]
    [ ! -f ".githooks/post-merge" ]
    [ ! -f ".git/hooks/post-merge" ]
}

@test "setup-hooks: rejects unknown options" {
    init_mode local
    run run_agentsync setup-hooks --bogus
    [ "$status" -eq 2 ]
}

@test "setup-hooks: fails outside a git repository" {
    init_mode local
    rm -rf .git
    run run_agentsync setup-hooks
    [ "$status" -ne 0 ]
    [[ "$output" == *"git repository"* ]]
}
