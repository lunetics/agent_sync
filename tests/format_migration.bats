#!/usr/bin/env bats
# Project format revision: a project that predates a migration is told so, and
# `migrate --apply` retires the stale engine-owned skill copy and records it.

load test_helper

setup() {
    setup_test_project
    ENGINE_FORMAT="$(cat "$REPO_ROOT/FORMAT")"
}

teardown() {
    teardown_test_project
}

# A project as an older engine would have left it: the agentsync skill copied
# into .ai/src/, its hash recorded in the template manifest, no format key.
seed_pre_format_project() {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    grep -v "^format:" .ai/agent_sync.yaml > .ai/agent_sync.yaml.tmp
    mv .ai/agent_sync.yaml.tmp .ai/agent_sync.yaml

    mkdir -p .ai/src/skills/agentsync/references
    cp "$REPO_ROOT/lib/templates/base-src/skills/agentsync/SKILL.md" .ai/src/skills/agentsync/SKILL.md
    local hash
    hash=$(file_sha256 .ai/src/skills/agentsync/SKILL.md)
    printf 'skills/agentsync/SKILL.md\t%s\n' "$hash" >> .ai/.template-manifest
}

@test "format: init records the engine's revision on a fresh project" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    grep -q "^format: $ENGINE_FORMAT$" .ai/agent_sync.yaml
}

@test "format: a fresh project has nothing to migrate" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    run run_agentsync migrate --legacy
    [ "$status" -eq 0 ]
    [[ "$output" == *"Nothing to migrate"* ]]
}

@test "format: doctor reports the revision" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    run run_agentsync doctor
    [[ "$output" == *"Project format"* ]]
}

@test "format: doctor warns when the project is behind" {
    seed_pre_format_project
    run run_agentsync doctor
    [[ "$output" == *"behind the engine"* ]]
    [[ "$output" == *"agentsync migrate"* ]]
}

@test "format: migrate previews the stale skill copy without touching it" {
    seed_pre_format_project
    run run_agentsync migrate --legacy
    [ "$status" -eq 0 ]
    [[ "$output" == *"would remove"* ]]
    [[ "$output" == *"skills/agentsync"* ]]
    [ -f .ai/src/skills/agentsync/SKILL.md ]
    ! grep -q "^format:" .ai/agent_sync.yaml
}

@test "format: migrate --apply removes the unedited copy and records the revision" {
    seed_pre_format_project
    run run_agentsync migrate --apply --yes
    [ "$status" -eq 0 ]
    [ ! -d .ai/src/skills/agentsync ]
    grep -q "^format: $ENGINE_FORMAT$" .ai/agent_sync.yaml
    ! grep -q "skills/agentsync" .ai/.template-manifest
}

@test "format: after migrating, the engine supplies the skill again" {
    seed_pre_format_project
    run_agentsync migrate --apply --yes >/dev/null 2>&1
    run run_agentsync sync
    [ "$status" -eq 0 ]
    [ -f .claude/skills/agentsync/SKILL.md ]
}

@test "format: an edited copy is kept as a deliberate override" {
    seed_pre_format_project
    printf '\nMY OWN NOTE\n' >> .ai/src/skills/agentsync/SKILL.md

    run run_agentsync migrate --apply --yes
    [ "$status" -eq 0 ]
    [[ "$output" == *"keep"* ]]
    grep -q "MY OWN NOTE" .ai/src/skills/agentsync/SKILL.md
    grep -q "^format: $ENGINE_FORMAT$" .ai/agent_sync.yaml
}

@test "format: an edited copy still shadows the engine version after sync" {
    seed_pre_format_project
    printf '\nMY OWN NOTE\n' >> .ai/src/skills/agentsync/SKILL.md
    run_agentsync migrate --apply --yes >/dev/null 2>&1
    run_agentsync sync >/dev/null 2>&1
    grep -q "MY OWN NOTE" .claude/skills/agentsync/SKILL.md
}

@test "format: migrating twice is a no-op" {
    seed_pre_format_project
    run_agentsync migrate --apply --yes >/dev/null 2>&1
    run run_agentsync migrate --legacy
    [ "$status" -eq 0 ]
    [[ "$output" == *"Nothing to migrate"* ]]
}

@test "format: upgrade-config does not silence a pending migration" {
    seed_pre_format_project
    run_agentsync upgrade-config >/dev/null 2>&1
    ! grep -q "^format:" .ai/agent_sync.yaml
}
