#!/usr/bin/env bats
# Configuration selection must fail closed before a write sync can reach
# cleanup or output mutation.

load test_helper

setup() {
    setup_test_project
}

teardown() {
    teardown_test_project
}

run_agentsync_env() {
    local env_name="$1"
    local env_value="$2"
    shift 2
    env "$env_name=$env_value" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" "$@"
}

@test "an invalid explicit config path fails without falling back or mutating outputs" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    mkdir -p .claude/skills
    touch .claude/skills/config-safety-sentinel.md
    local missing_config="$TEST_PROJECT/missing-agent-sync.yaml"

    run run_agentsync_env AGENTSYNC_CONFIG_PATH "$missing_config" sync

    [ "$status" -ne 0 ]
    [[ "$output" == *"AGENTSYNC_CONFIG_PATH is set but file not found"* ]]
    [[ "$output" != *"falling back"* ]]
    [ -f .claude/skills/config-safety-sentinel.md ]
}

@test "a missing config refuses write sync before cleanup defaults can run" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    rm .ai/agent_sync.yaml
    mkdir -p .claude/skills
    touch .claude/skills/config-safety-sentinel.md

    run env -u AGENTSYNC_CONFIG_PATH AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" sync

    [ "$status" -ne 0 ]
    [[ "$output" == *"No project configuration found"* ]]
    [ -f .claude/skills/config-safety-sentinel.md ]
}

@test "a missing config remains usable for a dry-run" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    rm .ai/agent_sync.yaml
    mkdir -p .claude/skills
    touch .claude/skills/config-safety-sentinel.md

    run env -u AGENTSYNC_CONFIG_PATH AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" sync --dry-run

    [ "$status" -eq 0 ]
    [ -f .claude/skills/config-safety-sentinel.md ]
}

@test "check rejects an invalid explicit config path instead of using the local config" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    local missing_config="$TEST_PROJECT/missing-agent-sync.yaml"

    run run_agentsync_env AGENTSYNC_CONFIG_PATH "$missing_config" check

    [ "$status" -ne 0 ]
    [[ "$output" == *"AGENTSYNC_CONFIG_PATH is set but file not found"* ]]
}
