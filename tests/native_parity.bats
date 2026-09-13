#!/usr/bin/env bats
# Bash and native answers for a ported command must match byte for byte.
# Skips when no binary is built: `cargo build --release` first.

load test_helper

setup_file() { seed_project; }
teardown_file() { teardown_seed_project; }

setup() {
    clone_seed
    NATIVE_BIN="${AGENTSYNC_NATIVE_BIN:-$REPO_ROOT/target/release/agentsync}"
    if [[ ! -x "$NATIVE_BIN" ]] && [[ -x "$NATIVE_BIN.exe" ]]; then
        NATIVE_BIN="$NATIVE_BIN.exe"
    fi
    [[ -x "$NATIVE_BIN" ]] || skip "no native binary at $NATIVE_BIN"
    export AGENTSYNC_NATIVE_BIN="$NATIVE_BIN"
}

teardown() { teardown_test_project; }

_run_engine() {
    local mode="$1"
    shift
    AGENTSYNC_NATIVE="$mode" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" "$@"
}

# Usage: assert_parity <agentsync args...>
# Fails with a diff when stdout+stderr differ, or when the exit status differs.
assert_parity() {
    local bash_out native_out bash_rc=0 native_rc=0
    bash_out=$(_run_engine 0 "$@" 2>&1) || bash_rc=$?
    native_out=$(_run_engine 1 "$@" 2>&1) || native_rc=$?
    if [[ "$bash_rc" -ne "$native_rc" ]]; then
        echo "exit status differs: bash=$bash_rc native=$native_rc" >&2
        return 1
    fi
    if [[ "$bash_out" != "$native_out" ]]; then
        diff <(printf '%s\n' "$bash_out") <(printf '%s\n' "$native_out") >&2 || true
        return 1
    fi
}

@test "parity: version and its flags" {
    assert_parity version
    assert_parity --version
    assert_parity -v
}

@test "parity: list on a fresh project" {
    assert_parity list
    assert_parity ls
}

@test "parity: list with enabled tools" {
    enable_tools claude cursor
    assert_parity list
}

@test "parity: list with a payload override in the per-tool layout" {
    mkdir -p .ai/src/tools/cursor
    echo '{}' > .ai/src/tools/cursor/hooks.json
    assert_parity list
}

@test "parity: list with a legacy flat-layout override" {
    mkdir -p .ai/src/mcp
    echo '{}' > .ai/src/mcp/claude.json
    assert_parity list
}

@test "parity: list with a shared MCP source and one per-tool override" {
    echo '{}' > .ai/src/mcp.json
    mkdir -p .ai/src/tools/kimi
    echo '{}' > .ai/src/tools/kimi/mcp.json
    assert_parity list
}

@test "parity: list with a custom tool enabled the legacy way" {
    mkdir -p .ai/src/tools
    printf 'name: "My Tool"\nenabled: true\n' > .ai/src/tools/mytool.yaml
    assert_parity list
}

@test "parity: list with a profile variant tool" {
    mkdir -p .ai/src/tools
    printf 'base: claude\nprofile_home: ".claude-hub"\n' > .ai/src/tools/claude-hub.yaml
    assert_parity list
}

@test "parity: list without a .ai directory" {
    rm -rf .ai
    assert_parity list
}

# ── check ────────────────────────────────────────────────────────────────────
# Outputs always come from the Bash sync, so a native check that agrees proves
# the render reproduces every managed file byte for byte.

ALL_TOOLS=(amazonq antigravity claude cline codex copilot cursor gemini junie kimi opencode windsurf zed)

_bash_sync() { _run_engine 0 sync "$@" >/dev/null 2>&1; }

# Usage: assert_parity_head <lines> <agentsync args...>
# For an accepted deviation past the first <lines> lines: the exit status and
# those lines must match.
assert_parity_head() {
    local count="$1"
    shift
    local bash_out native_out bash_rc=0 native_rc=0
    bash_out=$(_run_engine 0 "$@" 2>&1) || bash_rc=$?
    native_out=$(_run_engine 1 "$@" 2>&1) || native_rc=$?
    if [[ "$bash_rc" -ne "$native_rc" ]]; then
        echo "exit status differs: bash=$bash_rc native=$native_rc" >&2
        return 1
    fi
    bash_out=$(printf '%s\n' "$bash_out" | head -n "$count")
    native_out=$(printf '%s\n' "$native_out" | head -n "$count")
    if [[ "$bash_out" != "$native_out" ]]; then
        diff <(printf '%s\n' "$bash_out") <(printf '%s\n' "$native_out") >&2 || true
        return 1
    fi
}

_all_tools_fixture() {
    enable_tools "${ALL_TOOLS[@]}"
    printf '%s\n' '---' 'paths:' '  - "**/*.dart"' '---' '' '# Scoped Fixture Rule' '' '- Body.' \
        > .ai/src/rules/scoped-fixture.md
    printf '%s\n' '---' 'description: Explicit-only fixture command' 'disable-model-invocation: true' '---' '' 'Body.' \
        > .ai/src/commands/explicit-only.md
}

@test "parity: check on a project that was never synced" {
    enable_tools claude cursor
    assert_parity check
}

@test "parity: check after a Bash sync of all 13 tools" {
    _all_tools_fixture
    _bash_sync
    assert_parity check
}

@test "parity: check reports edited, deleted, and stale outputs" {
    _all_tools_fixture
    _bash_sync
    echo "edit" >> CLAUDE.md
    rm -f .cursor/rules/core.mdc
    echo "# more" >> .ai/src/rules/core.md
    assert_parity check
}

@test "parity: check after a tool is disabled" {
    enable_tools claude cursor codex
    _bash_sync
    _run_engine 0 disable cursor >/dev/null
    assert_parity check
}

@test "parity: check with shared inheritance and a changed parent" {
    enable_tools claude codex
    mkdir -p 'shared parent/.ai/src/rules' 'shared parent/.ai/src/skills/parent-only' 'shared parent/.ai/src/tools'
    printf 'parent rule\n' > 'shared parent/.ai/src/rules/parent-only.md'
    printf 'parent skill\n' > 'shared parent/.ai/src/skills/parent-only/SKILL.md'
    printf 'targets:\n  agents:\n    dest: "OTHER.md"\n' > 'shared parent/.ai/src/tools/claude.yaml'
    printf '\nshared:\n  path: "shared parent"\n  inherit: rules, skills, tools\n' >> .ai/agent_sync.yaml
    _bash_sync
    assert_parity check
    printf 'changed parent rule\n' > 'shared parent/.ai/src/rules/parent-only.md'
    assert_parity check
}

@test "parity: check with the engine skill layer off and a project copy of it" {
    enable_tools claude kimi
    _bash_sync
    printf '\nbase_skills: false\n' >> .ai/agent_sync.yaml
    assert_parity check
    mkdir -p .ai/src/skills/agentsync
    printf -- '---\nname: agentsync\ndescription: Project copy\n---\n' > .ai/src/skills/agentsync/SKILL.md
    assert_parity check
}

@test "parity: check with an active profile overlay" {
    enable_tools claude
    _run_engine 0 profile add hub --tools claude,codex >/dev/null
    mkdir -p .ai/profiles/hub/src/rules
    printf '# Hub only\n' > .ai/profiles/hub/src/rules/hub.md
    _bash_sync
    assert_parity check
    printf '# Hub changed\n' > .ai/profiles/hub/src/rules/hub.md
    assert_parity check
}

@test "parity: check with composed OpenCode MCP" {
    enable_tools opencode
    mkdir -p .ai/src/tools/opencode
    printf '%s\n' '{"$schema":"https://opencode.ai/config.json","theme":"system"}' > .ai/src/tools/opencode/settings.json
    printf '%s\n' '{"mcpServers":{"github":{"command":"npx","args":["-y","@github/mcp"],"env":{"TOKEN":"${GITHUB_TOKEN}"}},"docs":{"type":"sse","url":"https://example.test/mcp","headers":{"Authorization":"Bearer {env:TOKEN}"},"enabled":false,"timeout":9000,"oauth":false}}}' > .ai/src/mcp.json
    _bash_sync
    assert_parity check
}

@test "parity: check when the OpenCode MCP source is malformed" {
    enable_tools opencode claude
    _bash_sync
    printf '%s\n' '{"mcpServers":[]}' > .ai/src/mcp.json
    assert_parity_head 3 check
}

@test "parity: check with legacy, declared, per-tool, and shared payload sources" {
    enable_tools claude cursor windsurf junie
    mkdir -p .ai/src/mcp .ai/src/tools/cursor config
    echo '{"legacy":true}' > .ai/src/mcp/claude.json
    echo '{"per-tool":true}' > .ai/src/tools/cursor/hooks.json
    echo '{"shared":true}' > .ai/src/mcp.json
    echo '{"declared":true}' > config/windsurf-mcp.json
    printf 'targets:\n  mcp:\n    source: "config/windsurf-mcp.json"\n' > .ai/src/tools/windsurf.yaml
    _bash_sync
    assert_parity check
}

@test "parity: check with skills filters and a custom tool using every rules option" {
    enable_tools claude mytool
    mkdir -p .ai/src/skills/keepme .ai/src/skills/dropme .ai/src/tools
    echo "k" > .ai/src/skills/keepme/SKILL.md
    echo "d" > .ai/src/skills/dropme/SKILL.md
    printf 'targets:\n  skills:\n    exclude:\n      - dropme\n  rules:\n    include: [core.md, git.md]\n' > .ai/src/tools/claude.yaml
    cat > .ai/src/tools/mytool.yaml <<'YAML'
name: "My Tool"
targets:
  agents:
    dest: ".mytool/rules/AGENTS.md"
  rules:
    dest: ".mytool/rules"
    extension: ".txt"
    header: "---\nalways: true\n---"
    append_imports: true
  skills:
    dest: ".mytool/skills"
    include: "keep*"
  commands:
    inline_into_agents: true
  subagents:
    dest: ".mytool/agents"
    format: amazonq_json
YAML
    _bash_sync
    assert_parity check
}

@test "parity: check with a committed version pin mismatch" {
    enable_tools claude
    printf '\noutputs: committed\nagentsync_version: "0.0.1"\n' >> .ai/agent_sync.yaml
    assert_parity check
}

@test "parity: check without AGENTS.md" {
    enable_tools claude
    rm -f .ai/src/AGENTS.md
    assert_parity_head 3 check
}

@test "parity: check without a .ai directory" {
    rm -rf .ai
    assert_parity_head 2 check
}

@test "parity: check with this repository's own .ai/src and all 13 tools" {
    rm -rf .ai/src
    cp -R "$REPO_ROOT/.ai/src" .ai/src
    enable_tools "${ALL_TOOLS[@]}"
    _bash_sync
    assert_parity check
}
