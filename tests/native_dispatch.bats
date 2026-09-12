#!/usr/bin/env bats
# Tests for the Bash → native engine delegation in bin/agentsync.sh.

load test_helper

setup() {
    setup_test_project
    FAKE_BIN="$TEST_PROJECT/fake-agentsync"
    cat > "$FAKE_BIN" <<'EOF'
#!/usr/bin/env bash
printf 'native:%s\n' "$1"
printf 'arg:%s\n' "$@"
printf 'engine:%s\n' "${AGENTSYNC_ENGINE_VERSION:-unset}"
exit 42
EOF
    chmod +x "$FAKE_BIN"
    export AGENTSYNC_NATIVE_BIN="$FAKE_BIN"
}

teardown() { teardown_test_project; }

@test "native: a ported command runs the binary and returns its exit status" {
    export AGENTSYNC_NATIVE=1
    run run_agentsync version
    [ "$status" -eq 42 ]
    [[ "$output" == *"native:version"* ]]
}

@test "native: the engine version travels with the call" {
    export AGENTSYNC_NATIVE=1
    run run_agentsync version
    [[ "$output" == *"engine:$(cat "$REPO_ROOT/VERSION")"* ]]
}

@test "native: arguments reach the binary untouched" {
    export AGENTSYNC_NATIVE=1
    run run_agentsync version --flag "two words"
    [[ "$output" == *$'arg:--flag\narg:two words'* ]]
}

@test "native: AGENTSYNC_NATIVE=0 keeps a ported command in Bash" {
    export AGENTSYNC_NATIVE=0
    run run_agentsync version
    [ "$status" -eq 0 ]
    [[ "$output" == agentsync\ v* ]]
}

@test "native: an unported command never reaches the binary" {
    export AGENTSYNC_NATIVE=1
    run run_agentsync help
    [ "$status" -eq 0 ]
    [[ "$output" == *"COMMANDS"* ]]
    [[ "$output" != *"native:"* ]]
}

@test "native: without AGENTSYNC_NATIVE a missing binary falls back to Bash" {
    unset AGENTSYNC_NATIVE
    export AGENTSYNC_NATIVE_BIN="$TEST_PROJECT/missing"
    run run_agentsync version
    [ "$status" -eq 0 ]
    [[ "$output" == agentsync\ v* ]]
}

@test "native: AGENTSYNC_NATIVE=1 without a binary fails loudly" {
    export AGENTSYNC_NATIVE=1
    export AGENTSYNC_NATIVE_BIN="$TEST_PROJECT/missing"
    run run_agentsync version
    [ "$status" -eq 1 ]
    [[ "$output" == *"no native binary"* ]]
}
