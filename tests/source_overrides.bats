#!/usr/bin/env bats
# Regression tests for source.* layouts that live outside the default .ai/src tree.

load test_helper

setup() {
    setup_test_project
}

teardown() {
    teardown_test_project
    if [[ -n "${EXTERNAL_TOOLS_ROOT:-}" ]]; then
        _rm_rf_resilient "$EXTERNAL_TOOLS_ROOT"
    fi
    if [[ -n "${OUTSIDE_ROOT:-}" ]]; then
        _rm_rf_resilient "$OUTSIDE_ROOT"
    fi
}

write_project_sources() {
    mkdir -p "$TEST_PROJECT/.ai/src/rules" "$TEST_PROJECT/.ai/src/skills/project-skill"
    printf '%s\n' '# Project Agent' > "$TEST_PROJECT/.ai/src/AGENTS.md"
    printf '%s\n' '# Project Rule' > "$TEST_PROJECT/.ai/src/rules/project.md"
    printf '%s\n' '---' 'name: project-skill' 'description: project fixture skill' '---' '' '# Project Skill' \
        > "$TEST_PROJECT/.ai/src/skills/project-skill/SKILL.md"
}

# Write .ai/agent_sync.yaml enabling Claude, with source.rules set when given.
write_rules_config() {
    local rules_source="${1:-}"
    mkdir -p "$TEST_PROJECT/.ai"
    {
        printf '%s\n' 'format: 2' 'outputs: committed' 'tools:' '  enabled:' '    - claude'
        if [[ -n "$rules_source" ]]; then
            printf '%s\n' 'source:' "  rules: \"$rules_source\""
        fi
    } > "$TEST_PROJECT/.ai/agent_sync.yaml"
}

make_outside_rules() {
    OUTSIDE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/agentsync_outside.XXXXXX")"
    mkdir -p "$OUTSIDE_ROOT/rules"
    printf '%s\n' '# Outside Rule' > "$OUTSIDE_ROOT/rules/outside.md"
}

write_external_fixture() {
    local tools_path="$1"
    local config="$TEST_PROJECT/config/agent_sync.yaml"
    local skills_dest="${2:-.external/skills}"
    local tools_dir="$tools_path"
    [[ "$tools_dir" == /* ]] || tools_dir="$TEST_PROJECT/$tools_dir"

    write_project_sources
    mkdir -p "$TEST_PROJECT/config" \
        "$TEST_PROJECT/sources/rules" \
        "$TEST_PROJECT/sources/skills/external-skill" \
        "$tools_dir/claude"

    printf '%s\n' '# External Agent' > "$TEST_PROJECT/sources/AGENTS.md"
    printf '%s\n' '# External Rule' > "$TEST_PROJECT/sources/rules/external.md"
    printf '%s\n' '---' 'name: external-skill' 'description: external fixture skill' '---' '' '# External Skill' \
        > "$TEST_PROJECT/sources/skills/external-skill/SKILL.md"
    printf '%s\n' '{"external":true}' > "$tools_dir/claude/settings.json"
    printf '%s\n' \
        'name: "External Claude"' \
        'enabled: false' \
        'targets:' \
        '  agents:' \
        '    enabled: false' \
        '  rules:' \
        '    enabled: false' \
        '  skills:' \
        "    dest: \"$skills_dest\"" \
        '  commands:' \
        '    enabled: false' \
        '  subagents:' \
        '    enabled: false' \
        '  settings:' \
        '    dest: ".external/settings.json"' \
        '  mcp:' \
        '    enabled: false' \
        '  hooks:' \
        '    enabled: false' \
        '  guard:' \
        '    enabled: false' \
        > "$tools_dir/claude.yaml"

    mkdir -p "$TEST_PROJECT/.ai"
    printf '%s\n' \
        'format: 2' \
        'outputs: committed' \
        'tools:' \
        '  enabled:' \
        '    - claude' \
        'source:' \
        '  agents: "sources/AGENTS.md"' \
        '  rules: "sources/rules"' \
        '  skills: "sources/skills"' \
        "  tools: \"$tools_path\"" \
        > "$config"
    EXTERNAL_CONFIG="$config"
}

run_external_sync() {
    AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" run_agentsync sync
}

@test "source.tools relative override drives tool YAML and payloads" {
    write_external_fixture "sources/tools"

    run run_external_sync

    [ "$status" -eq 0 ]
    [ -f ".external/skills/external-skill/SKILL.md" ]
    [ "$(cat .external/settings.json)" = '{"external":true}' ]
    [ ! -e "CLAUDE.md" ]
    [ ! -e ".claude/rules" ]
    [ ! -e ".claude/skills" ]
    [ ! -e ".mcp.json" ]
}

@test "source.tools absolute override drives the same layout" {
    EXTERNAL_TOOLS_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/agentsync_external_tools.XXXXXX")"
    write_external_fixture "$EXTERNAL_TOOLS_ROOT"

    run run_external_sync

    [ "$status" -eq 0 ]
    [ -f ".external/skills/external-skill/SKILL.md" ]
    [ "$(cat .external/settings.json)" = '{"external":true}' ]
    [ ! -e "CLAUDE.md" ]
    [ ! -e ".claude/settings.json" ]
}

@test "external config and sources remain valid through isolated check" {
    write_external_fixture "sources/tools"
    run run_external_sync
    [ "$status" -eq 0 ]

    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" check

    [ "$status" -eq 0 ]
    [[ "$output" == *"synced"* ]]
}

@test "isolated check reads the version pin from an external config" {
    write_external_fixture "sources/tools"
    printf '%s\n' 'agentsync_version: "0.0.0"' >> "$EXTERNAL_CONFIG"

    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" check

    [ "$status" -eq 1 ]
    [[ "$output" == *"pins agentsync 0.0.0"* ]]
    [[ "$output" != *"Sync script failed during check"* ]]
}

@test "external source sync is idempotent and detects source drift" {
    write_external_fixture "sources/tools"
    run run_external_sync
    [ "$status" -eq 0 ]

    run run_external_sync
    [ "$status" -eq 0 ]
    [ -z "$(git diff -- .ai/.sync-manifest .gitignore)" ]

    printf '%s\n' '# changed externally' >> "$TEST_PROJECT/sources/skills/external-skill/SKILL.md"
    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" check
    [ "$status" -eq 1 ]
    [[ "$output" == *"out of sync"* ]]
}

@test "source.tools changes trigger sync --if-stale" {
    write_external_fixture "sources/tools"
    run run_external_sync
    [ "$status" -eq 0 ]

    printf '%s\n' '{"external":false}' > "$TEST_PROJECT/sources/tools/claude/settings.json"
    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" sync --if-stale

    [ "$status" -eq 0 ]
    [ "$(cat .external/settings.json)" = '{"external":false}' ]
}

@test "show and doctor resolve an external tool catalog" {
    write_external_fixture "sources/tools"

    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" show claude
    [ "$status" -eq 0 ]
    [[ "$output" == *"External Claude"* ]]

    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" doctor
    [ "$status" -eq 0 ]
    [[ "$output" == *"External Claude"* ]]
}

@test "external source layout preserves foreign skills and sibling files" {
    write_external_fixture "sources/tools"
    mkdir -p ".external/skills/foreign-skill"
    printf '%s\n' 'foreign skill' > ".external/skills/foreign-skill/SKILL.md"
    printf '%s\n' 'foreign plugin' > ".external/foreign-plugin.js"

    run run_external_sync

    [ "$status" -eq 0 ]
    [ -f ".external/skills/foreign-skill/SKILL.md" ]
    [ "$(cat .external/skills/foreign-skill/SKILL.md)" = 'foreign skill' ]
    [ "$(cat .external/foreign-plugin.js)" = 'foreign plugin' ]
}

@test "external source layout cannot widen output targets through traversal" {
    write_external_fixture "sources/tools" "escape-link/skills"
    local outside
    outside="$(mktemp -d "${TMPDIR:-/tmp}/agentsync_output_escape.XXXXXX")"
    create_test_symlink "$outside" "$TEST_PROJECT/escape-link"

    run run_external_sync

    [ "$status" -eq 0 ]
    [[ "$output" == *"outside repository root"* ]]
    [ ! -e "$outside/skills" ]
    [ ! -e "CLAUDE.md" ]
    _rm_rf_resilient "$outside"
}

@test "profile variants use the configured external source.tools directory" {
    write_external_fixture "sources/tools"

    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" profile add hub --tools claude

    [ "$status" -eq 0 ]
    [ -f "$TEST_PROJECT/sources/tools/claude-hub.yaml" ]
    grep -q '^base: claude$' "$TEST_PROJECT/sources/tools/claude-hub.yaml"
}

@test "source containment: default config refuses a .ai/src symlink that escapes the project" {
    write_project_sources
    make_outside_rules
    _rm_rf_resilient "$TEST_PROJECT/.ai/src/rules"
    create_test_symlink "$OUTSIDE_ROOT/rules" "$TEST_PROJECT/.ai/src/rules"
    write_rules_config

    run run_agentsync sync

    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "targets.rules.source for Claude Code resolves outside safe source roots"
    [ ! -e ".claude/rules/outside.md" ]
}

@test "source containment: explicit absolute source.rules outside the project syncs and checks" {
    write_project_sources
    make_outside_rules
    write_rules_config "$OUTSIDE_ROOT/rules"

    run run_agentsync sync
    [ "$status" -eq 0 ]
    [ -f ".claude/rules/outside.md" ]
    [ ! -e ".claude/rules/project.md" ]

    run run_agentsync check
    [ "$status" -eq 0 ]
}

@test "source containment: explicit ../ source.rules resolves from the project root" {
    write_project_sources
    make_outside_rules
    write_rules_config "../$(basename "$OUTSIDE_ROOT")/rules"

    run run_agentsync sync
    [ "$status" -eq 0 ]
    [ -f ".claude/rules/outside.md" ]

    run run_agentsync check
    [ "$status" -eq 0 ]
}

@test "source containment: explicit source root at / is refused" {
    write_project_sources
    write_rules_config "/"

    run run_agentsync sync

    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- 'source.rules must not be the filesystem root, the home directory, or the project root or its ancestor: / -> /'
    [ ! -e ".claude" ]
}

@test "source containment: explicit source root at \$HOME is refused" {
    write_project_sources
    OUTSIDE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/agentsync_home.XXXXXX")"
    printf '%s\n' '# Home Rule' > "$OUTSIDE_ROOT/home.md"
    write_rules_config "$OUTSIDE_ROOT"

    run env HOME="$OUTSIDE_ROOT" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" sync

    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "source.rules must not be the filesystem root, the home directory, or the project root or its ancestor"
    [ ! -e ".claude/rules/home.md" ]
}

@test "source containment: explicit source root at a project ancestor is refused" {
    write_project_sources
    write_rules_config ".."

    run run_agentsync sync

    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "source.rules must not be the filesystem root, the home directory, or the project root or its ancestor: .."
    [ ! -e ".claude" ]
}
