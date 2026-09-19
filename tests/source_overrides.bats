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
    OUTSIDE_ROOT="$(host_path "$(mktemp -d "${TMPDIR:-/tmp}/agentsync_outside.XXXXXX")")"
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
    skip_on_windows "an absolute source.tools outside the project is not applied on Windows (open in the phase 6 receipt)"
    EXTERNAL_TOOLS_ROOT="$(host_path "$(mktemp -d "${TMPDIR:-/tmp}/agentsync_external_tools.XXXXXX")")"
    write_external_fixture "$EXTERNAL_TOOLS_ROOT"

    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$EXTERNAL_TOOLS_ROOT" run run_external_sync

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

    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" "$AGENTSYNC_BIN" check

    [ "$status" -eq 0 ]
    [[ "$output" == *"synced"* ]]
}

@test "isolated check reads the version pin from an external config" {
    write_external_fixture "sources/tools"
    printf '%s\n' 'agentsync_version: "0.0.0"' >> "$EXTERNAL_CONFIG"

    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" "$AGENTSYNC_BIN" check

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
    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" "$AGENTSYNC_BIN" check
    [ "$status" -eq 1 ]
    [[ "$output" == *"out of sync"* ]]
}

@test "source.tools changes trigger sync --if-stale" {
    write_external_fixture "sources/tools"
    run run_external_sync
    [ "$status" -eq 0 ]

    printf '%s\n' '{"external":false}' > "$TEST_PROJECT/sources/tools/claude/settings.json"
    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" "$AGENTSYNC_BIN" sync --if-stale

    [ "$status" -eq 0 ]
    [ "$(cat .external/settings.json)" = '{"external":false}' ]
}

@test "show and doctor resolve an external tool catalog" {
    write_external_fixture "sources/tools"

    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" "$AGENTSYNC_BIN" show claude
    [ "$status" -eq 0 ]
    [[ "$output" == *"External Claude"* ]]

    # The fixture's settings payload does not register the guard, so doctor
    # warns (exit 1) about the unwired guard script.
    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" "$AGENTSYNC_BIN" doctor
    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "External Claude"
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
    outside="$(host_path "$(mktemp -d "${TMPDIR:-/tmp}/agentsync_output_escape.XXXXXX")")"
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

    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" "$AGENTSYNC_BIN" profile add hub --tools claude

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
    printf '%s' "$output" | grep -qF -- "Source symlink .ai/src/rules resolves outside the project"
    [ ! -e ".claude/rules/outside.md" ]
}

@test "source containment: an explicit in-project source.rules symlink escaping the project is refused" {
    write_project_sources
    make_outside_rules
    _rm_rf_resilient "$TEST_PROJECT/.ai/src/rules"
    create_test_symlink "$OUTSIDE_ROOT/rules" "$TEST_PROJECT/.ai/src/rules"
    write_rules_config ".ai/src/rules"

    run run_agentsync sync

    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "Source symlink .ai/src/rules resolves outside the project"
    [ ! -e ".claude/rules/outside.md" ]
}

@test "source containment: explicit absolute source.rules outside the project syncs and checks" {
    write_project_sources
    make_outside_rules
    write_rules_config "$OUTSIDE_ROOT/rules"
    export AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$OUTSIDE_ROOT/rules"

    run run_agentsync sync
    [ "$status" -eq 0 ]
    [ -f ".claude/rules/outside.md" ]
    [ ! -e ".claude/rules/project.md" ]

    run run_agentsync check
    [ "$status" -eq 0 ]
}

@test "source containment: an outside source.rules not listed in AGENTSYNC_EXTERNAL_SOURCE_ROOTS is refused" {
    write_project_sources
    make_outside_rules
    write_rules_config "$OUTSIDE_ROOT/rules"
    local other_root
    other_root="$(host_path "$(mktemp -d "${TMPDIR:-/tmp}/agentsync_other.XXXXXX")")"

    run env -u AGENTSYNC_EXTERNAL_SOURCE_ROOTS "$AGENTSYNC_BIN" sync
    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "source.rules points outside the project at"
    printf '%s' "$output" | grep -qF -- "which AGENTSYNC_EXTERNAL_SOURCE_ROOTS does not list"
    [ ! -e ".claude" ]

    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="relative/path:$other_root" run run_agentsync sync
    _rm_rf_resilient "$other_root"
    [ "$status" -eq 1 ]
    [ ! -e ".claude" ]
}

@test "source containment: a trusted parent directory admits every source below it" {
    write_project_sources
    make_outside_rules
    write_rules_config "$OUTSIDE_ROOT/rules"

    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="/nonexistent-agentsync-root:$OUTSIDE_ROOT" run run_agentsync sync

    [ "$status" -eq 0 ]
    [ -f ".claude/rules/outside.md" ]
}

@test "doctor fails an outside source that AGENTSYNC_EXTERNAL_SOURCE_ROOTS does not list" {
    write_project_sources
    make_outside_rules
    write_rules_config "$OUTSIDE_ROOT/rules"

    run env -u AGENTSYNC_EXTERNAL_SOURCE_ROOTS "$AGENTSYNC_BIN" doctor

    [ "$status" -eq 2 ]
    printf '%s' "$output" | grep -qF -- "source.rules points outside the project and AGENTSYNC_EXTERNAL_SOURCE_ROOTS does not list it"
}

@test "source containment: explicit ../ source.rules resolves from the project root" {
    write_project_sources
    make_outside_rules
    write_rules_config "../$(basename "$OUTSIDE_ROOT")/rules"
    export AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$OUTSIDE_ROOT"

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
    # The canonical spelling of / is C:/ on Windows; the refusal is the same.
    printf '%s' "$output" | grep -qF -- 'source.rules must not be the filesystem root, the home directory, or the project root or its ancestor: / ->'
    [ ! -e ".claude" ]
}

@test "source containment: explicit source root at \$HOME is refused" {
    write_project_sources
    OUTSIDE_ROOT="$(host_path "$(mktemp -d "${TMPDIR:-/tmp}/agentsync_home.XXXXXX")")"
    printf '%s\n' '# Home Rule' > "$OUTSIDE_ROOT/home.md"
    write_rules_config "$OUTSIDE_ROOT"

    run env HOME="$OUTSIDE_ROOT" "$AGENTSYNC_BIN" sync

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

@test "tool resolver ignores an auto-detected flat .ai/tools catalog like show does" {
    write_project_sources
    write_rules_config
    mkdir -p .ai/tools
    printf '%s\n' 'name: "Flat Claude"' > .ai/tools/claude.yaml

    run run_agentsync sync
    [ "$status" -eq 0 ]
    printf '%s' "$output" | grep -qF -- "Claude Code complete"
    [ -z "$(printf '%s' "$output" | grep -F -- "Flat Claude" || true)" ]

    run run_agentsync show claude
    [ "$status" -eq 0 ]
    [ -z "$(printf '%s' "$output" | grep -F -- "Flat Claude" || true)" ]
}

@test "customize refuses to write into an external source.tools directory" {
    EXTERNAL_TOOLS_ROOT="$(host_path "$(mktemp -d "${TMPDIR:-/tmp}/agentsync_external_tools.XXXXXX")")"
    write_external_fixture "$EXTERNAL_TOOLS_ROOT"

    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" "$AGENTSYNC_BIN" customize codex

    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "source.tools resolves outside the project: $EXTERNAL_TOOLS_ROOT"
    [ ! -e "$EXTERNAL_TOOLS_ROOT/codex.yaml" ]
}

@test "profile remove refuses to delete from an external source.tools directory" {
    write_external_fixture "sources/tools"
    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" "$AGENTSYNC_BIN" profile add hub --tools claude
    [ "$status" -eq 0 ]
    mkdir -p "$TEST_PROJECT/sources/tools/claude-hub"
    printf '%s\n' '{}' > "$TEST_PROJECT/sources/tools/claude-hub/settings.json"
    EXTERNAL_TOOLS_ROOT="$(host_path "$(mktemp -d "${TMPDIR:-/tmp}/agentsync_external_tools.XXXXXX")")"
    mv "$TEST_PROJECT/sources/tools" "$EXTERNAL_TOOLS_ROOT/tools"
    local config_tmp="$EXTERNAL_CONFIG.tmp"
    sed "s|^  tools: .*|  tools: \"$EXTERNAL_TOOLS_ROOT/tools\"|" "$EXTERNAL_CONFIG" > "$config_tmp"
    mv "$config_tmp" "$EXTERNAL_CONFIG"

    run env AGENTSYNC_CONFIG_PATH="$EXTERNAL_CONFIG" "$AGENTSYNC_BIN" profile remove hub --yes

    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "source.tools resolves outside the project: $EXTERNAL_TOOLS_ROOT/tools"
    [ -f "$EXTERNAL_TOOLS_ROOT/tools/claude-hub.yaml" ]
    [ -f "$EXTERNAL_TOOLS_ROOT/tools/claude-hub/settings.json" ]
    grep -q '^  hub:' "$EXTERNAL_CONFIG"
}

@test "source symlinks: a rule file linking outside the project is refused before any write" {
    write_project_sources
    make_outside_rules
    create_test_symlink "$OUTSIDE_ROOT/rules/outside.md" "$TEST_PROJECT/.ai/src/rules/leak.md"
    write_rules_config

    run run_agentsync sync

    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "Source symlink .ai/src/rules/leak.md resolves outside the project"
    [ ! -e ".claude" ]
    [ ! -e "CLAUDE.md" ]
}

@test "source symlinks: an AGENTS.md linking outside the project is refused" {
    write_project_sources
    make_outside_rules
    mv "$TEST_PROJECT/.ai/src/AGENTS.md" "$TEST_PROJECT/AGENTS.local.md"
    create_test_symlink "$OUTSIDE_ROOT/rules/outside.md" "$TEST_PROJECT/.ai/src/AGENTS.md"
    write_rules_config

    run run_agentsync sync

    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "Source symlink .ai/src/AGENTS.md resolves outside the project"
    [ ! -e "CLAUDE.md" ]
}

@test "source symlinks: a link to a safe directory is followed and its own links are checked" {
    write_project_sources
    make_outside_rules
    mkdir -p "$TEST_PROJECT/vendor/skills/vendored"
    printf '%s\n' '---' 'name: vendored' 'description: vendored skill' '---' > "$TEST_PROJECT/vendor/skills/vendored/SKILL.md"
    create_test_symlink "$OUTSIDE_ROOT/rules/outside.md" "$TEST_PROJECT/vendor/skills/vendored/leak.md"
    create_test_symlink "$TEST_PROJECT/vendor/skills/vendored" "$TEST_PROJECT/.ai/src/skills/vendored"
    write_rules_config

    run run_agentsync sync

    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "vendor/skills/vendored/leak.md resolves outside the project"
    [ ! -e ".claude" ]
}

@test "source symlinks: links that stay inside the project keep syncing" {
    write_project_sources
    mkdir -p "$TEST_PROJECT/docs"
    printf '%s\n' '# Shared Rule' > "$TEST_PROJECT/docs/shared.md"
    create_test_symlink "$TEST_PROJECT/docs/shared.md" "$TEST_PROJECT/.ai/src/rules/shared.md"
    write_rules_config

    run run_agentsync sync

    [ "$status" -eq 0 ]
    [ "$(cat .claude/rules/shared.md)" = "# Shared Rule" ]
}

@test "source symlinks: a trusted outside target is read" {
    write_project_sources
    make_outside_rules
    create_test_symlink "$OUTSIDE_ROOT/rules/outside.md" "$TEST_PROJECT/.ai/src/rules/outside.md"
    write_rules_config

    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$OUTSIDE_ROOT" run run_agentsync sync

    [ "$status" -eq 0 ]
    [ "$(cat .claude/rules/outside.md)" = "# Outside Rule" ]
}
