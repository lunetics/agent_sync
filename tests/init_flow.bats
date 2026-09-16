#!/usr/bin/env bats
# `agentsync init` as the one setup command: pick the outputs mode, keep the
# project's existing tool config, write the CI gate, and run the first sync.

load test_helper

setup() {
    setup_test_project
}

teardown() {
    teardown_test_project
}

@test "init: runs the first sync so outputs exist without a second command" {
    run run_agentsync init --tools claude --yes
    [ "$status" -eq 0 ]
    [ -f CLAUDE.md ]
    [ -f .claude/rules/core.md ]
    [ -f .ai/.sync-manifest ]
}

@test "init: --no-sync leaves the outputs unwritten" {
    run run_agentsync init --tools claude --yes --no-sync
    [ "$status" -eq 0 ]
    [ ! -f CLAUDE.md ]
    [ ! -f .ai/.sync-manifest ]
}

@test "init: no enabled tools means no sync" {
    run run_agentsync init --no-detect --yes
    [ "$status" -eq 0 ]
    [ ! -f .ai/.sync-manifest ]
}

@test "init: an existing CLAUDE.md is adopted, so the first sync reproduces it" {
    printf '# Hand-written team rules\n' > CLAUDE.md
    run run_agentsync init --tools claude --yes
    [ "$status" -eq 0 ]
    [[ "$output" == *"Adopted"* ]]
    grep -q "Hand-written team rules" .ai/src/AGENTS.md
    grep -q "Hand-written team rules" CLAUDE.md
}

@test "init: an existing rule file is adopted into .ai/src/rules/" {
    mkdir -p .claude/rules
    printf '# Legacy rule\n' > .claude/rules/legacy.md
    run run_agentsync init --tools claude --yes
    [ "$status" -eq 0 ]
    grep -q "Legacy rule" .ai/src/rules/legacy.md
    grep -q "Legacy rule" .claude/rules/legacy.md
}

@test "init: --existing replace regenerates instead of keeping" {
    printf '# Hand-written team rules\n' > CLAUDE.md
    run run_agentsync init --tools claude --yes --existing replace
    [ "$status" -eq 0 ]
    ! grep -q "Hand-written team rules" .ai/src/AGENTS.md
    ! grep -q "Hand-written team rules" CLAUDE.md
}

@test "init: rejects an unknown --existing action" {
    run run_agentsync init --tools claude --yes --existing bogus
    [ "$status" -ne 0 ]
    [[ "$output" == *"adopt"* ]]
    [[ "$output" == *"replace"* ]]
}

@test "init: two destinations mapping to one source keep the first and report the rest" {
    printf '# From CLAUDE\n' > CLAUDE.md
    printf '# From AGENTS\n' > AGENTS.md
    run run_agentsync init --tools claude,codex --yes --no-sync
    [ "$status" -eq 0 ]
    [[ "$output" == *"Adopted AGENTS.md"* ]]
    [[ "$output" == *"Kept as-is CLAUDE.md — another file already became .ai/src/AGENTS.md"* ]]
    [ "$(cat .ai/src/AGENTS.md)" = "# From AGENTS" ]
}

@test "init: a file no tool produces is kept with the resolver's reason" {
    mkdir -p .cursor/rules
    printf 'body\n' > .cursor/rules/core.mdc
    run run_agentsync init --tools cursor --yes --no-sync
    [ "$status" -eq 0 ]
    [[ "$output" == *"Kept as-is .cursor/rules/core.mdc — cursor injects a frontmatter header on sync."* ]]
    [ "$(cat .cursor/rules/core.mdc)" = "body" ]
}

@test "init: --ci github writes the check workflow with the pinned version" {
    run run_agentsync init --tools claude --yes --ci github
    [ "$status" -eq 0 ]
    [ -f .github/workflows/agentsync-check.yml ]
    grep -q "agentsync check" .github/workflows/agentsync-check.yml
    grep -q "AGENTSYNC_VERSION=$(cat "$REPO_ROOT/VERSION")" .github/workflows/agentsync-check.yml
    ! grep -q "__AGENTSYNC_VERSION__" .github/workflows/agentsync-check.yml
}

@test "init: the CI workflow ships the autofix job disabled" {
    run_agentsync init --tools claude --yes --ci github >/dev/null 2>&1
    grep -q "autofix:" .github/workflows/agentsync-check.yml
    grep -q "if: false" .github/workflows/agentsync-check.yml
}

@test "init: an existing CI workflow is never overwritten" {
    mkdir -p .github/workflows
    printf 'name: mine\n' > .github/workflows/agentsync-check.yml
    run run_agentsync init --tools claude --yes --ci github
    [ "$status" -eq 0 ]
    [ "$(cat .github/workflows/agentsync-check.yml)" = "name: mine" ]
}

@test "init: no CI workflow without --ci" {
    mkdir -p .github
    run run_agentsync init --tools claude --yes
    [ "$status" -eq 0 ]
    [ ! -f .github/workflows/agentsync-check.yml ]
}

@test "init: rejects an unsupported --ci provider" {
    run run_agentsync init --tools claude --yes --ci gitlab
    [ "$status" -ne 0 ]
    [[ "$output" == *"github"* ]]
}

@test "init: committed mode ends by telling the user to commit" {
    run run_agentsync init --tools claude --yes
    [[ "$output" == *"Commit"* ]]
}
