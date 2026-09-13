#!/usr/bin/env bats
# agentsync_version pin vs the running engine: fatal when outputs are
# committed (every machine must generate identical files), a warning otherwise.

load test_helper

setup() {
    setup_test_project
}

teardown() {
    teardown_test_project
}

pin_version() {
    sed "s/^agentsync_version: .*/agentsync_version: \"$1\"/" .ai/agent_sync.yaml > .ai/agent_sync.yaml.tmp
    mv .ai/agent_sync.yaml.tmp .ai/agent_sync.yaml
}

set_version_pin_mode() {
    awk -v mode="$1" '
        /^agentsync_version:/ {
            print
            print "version_pin:"
            print "  mode: " mode
            next
        }
        { print }
    ' .ai/agent_sync.yaml > .ai/agent_sync.yaml.tmp
    mv .ai/agent_sync.yaml.tmp .ai/agent_sync.yaml
}

@test "version pin: committed mode refuses to sync with a different engine" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    pin_version 0.1.0
    run run_agentsync sync
    [ "$status" -eq 1 ]
    [[ "$output" == *"pins agentsync 0.1.0"* ]]
    [[ "$output" == *"agentsync update 0.1.0"* ]]
    [[ "$output" == *"agentsync upgrade-config"* ]]
    [ ! -f CLAUDE.md ]
}

@test "version pin: committed mode check fails with the same explanation" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    run_agentsync sync >/dev/null 2>&1
    pin_version 0.1.0
    run run_agentsync check
    [ "$status" -eq 1 ]
    [[ "$output" == *"pins agentsync 0.1.0"* ]]
}

@test "version pin: local mode only warns and still syncs" {
    run_agentsync init --tools claude --yes --no-sync --outputs local >/dev/null 2>&1
    pin_version 0.1.0
    set_version_pin_mode warn
    run run_agentsync sync
    [ "$status" -eq 0 ]
    [[ "$output" == *"pins agentsync 0.1.0"* ]]
    [ -f CLAUDE.md ]
}

@test "version pin: local mode can be made strict for sync" {
    run_agentsync init --tools claude --yes --no-sync --outputs local >/dev/null 2>&1
    pin_version 0.1.0
    set_version_pin_mode strict
    run run_agentsync sync
    [ "$status" -eq 1 ]
    [[ "$output" == *"pins agentsync 0.1.0"* ]]
    [ ! -f CLAUDE.md ]
}

@test "version pin: local strict mode also fails check" {
    run_agentsync init --tools claude --yes --no-sync --outputs local >/dev/null 2>&1
    run_agentsync sync >/dev/null 2>&1
    pin_version 0.1.0
    set_version_pin_mode strict
    run run_agentsync check
    [ "$status" -eq 1 ]
    [[ "$output" == *"pins agentsync 0.1.0"* ]]
}

@test "version pin: unknown mode fails before writing" {
    run_agentsync init --tools claude --yes --no-sync --outputs local >/dev/null 2>&1
    set_version_pin_mode refuse
    run run_agentsync sync
    [ "$status" -eq 1 ]
    [[ "$output" == *"Unknown version_pin.mode 'refuse'"* ]]
    [ ! -f CLAUDE.md ]
}

@test "version pin: a matching pin is silent" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    run run_agentsync sync
    [ "$status" -eq 0 ]
    [[ "$output" != *"pins agentsync"* ]]
}

@test "version pin: no pin means no check" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    grep -v "^agentsync_version:" .ai/agent_sync.yaml > .ai/agent_sync.yaml.tmp
    mv .ai/agent_sync.yaml.tmp .ai/agent_sync.yaml
    run run_agentsync sync
    [ "$status" -eq 0 ]
    [[ "$output" != *"pins agentsync"* ]]
}

@test "version pin: upgrade-config re-pins to the running engine and unblocks sync" {
    run_agentsync init --tools claude --yes --no-sync >/dev/null 2>&1
    pin_version 0.1.0
    run run_agentsync upgrade-config
    [ "$status" -eq 0 ]
    run run_agentsync sync
    [ "$status" -eq 0 ]
}
