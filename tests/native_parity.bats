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
