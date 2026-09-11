#!/usr/bin/env bats
# install.sh and `update <version>` against a purpose-built origin repository.
# Every path the installer touches is redirected into the test project so the
# developer's ~/.agentsync, PATH symlink, and shell rc are never modified.
#
# The origin is a fixture, not this repository: `actions/checkout` clones without
# tags, so pinning to a real release tag fails on CI even though it passes on a
# developer's full clone. The fixture carries its own tags and keeps the tests
# independent of this repository's tag history.

load test_helper

# Tags the fixture origin publishes. Deliberately outside any real release
# series so they cannot be mistaken for this project's versions.
FIXTURE_NEW="9.9.2"
FIXTURE_OLD="9.9.1"
FIXTURE_ABSENT="999.0.0"

setup_file() {
    INSTALL_ORIGIN="$(mktemp -d "${TMPDIR:-/tmp}/agentsync_origin.XXXXXX")"
    export INSTALL_ORIGIN

    # `update` fetches `origin main` by name, so the fixture needs that branch.
    # `git branch -m` rather than `init -b`: the latter needs git >= 2.28.
    (
        cd "$INSTALL_ORIGIN" || exit 1
        git init --quiet
        git config user.email "test@test.com"
        git config user.name "Test"

        cp -R "$REPO_ROOT/bin" "$REPO_ROOT/lib" .
        printf '%s\n' "$FIXTURE_OLD" > VERSION
        git add -A
        git commit --quiet -m "fixture engine $FIXTURE_OLD"
        git tag "$FIXTURE_OLD"
        git branch -m main

        printf '%s\n' "$FIXTURE_NEW" > VERSION
        git commit --quiet -am "fixture engine $FIXTURE_NEW"
        git tag "$FIXTURE_NEW"
    )
}

teardown_file() {
    [[ -n "${INSTALL_ORIGIN:-}" ]] && _rm_rf_resilient "$INSTALL_ORIGIN"
}

setup() {
    setup_test_project
    export HOME="$TEST_PROJECT/home"
    mkdir -p "$HOME"
    touch "$HOME/.zshrc"
    export AGENTSYNC_REPO_URL="$INSTALL_ORIGIN"
    export AGENTSYNC_INSTALL_DIR="$TEST_PROJECT/engine"
    export AGENTSYNC_BIN_DIR="$TEST_PROJECT/bin"
    # `update` re-links whichever agentsync is on PATH; keep it the test one.
    export PATH="$TEST_PROJECT/bin:$PATH"
}

teardown() {
    teardown_test_project
}

@test "install: the fixture origin publishes the tags these tests pin to" {
    run git -C "$INSTALL_ORIGIN" tag
    [ "$status" -eq 0 ]
    [[ "$output" == *"$FIXTURE_OLD"* ]]
    [[ "$output" == *"$FIXTURE_NEW"* ]]
    [[ "$output" != *"$FIXTURE_ABSENT"* ]]
    git -C "$INSTALL_ORIGIN" rev-parse --verify --quiet refs/heads/main
}

@test "install: AGENTSYNC_VERSION pins the engine to that release tag" {
    run env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_NEW" ]
    [ -e "$TEST_PROJECT/bin/agentsync" ]
    grep -q "AGENTSYNC_HOME" "$HOME/.zshrc"
}

@test "install: an unknown AGENTSYNC_VERSION fails clearly" {
    run env AGENTSYNC_VERSION="$FIXTURE_ABSENT" bash "$REPO_ROOT/install.sh"
    [ "$status" -ne 0 ]
    [[ "$output" == *"$FIXTURE_ABSENT"* ]]
}

@test "install: re-running with a different pin moves an existing install" {
    env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh" >/dev/null
    run env AGENTSYNC_VERSION="$FIXTURE_OLD" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_OLD" ]
}

@test "update <version>: pins an installed engine to that release" {
    env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh" >/dev/null
    run env AGENTSYNC_HOME="$TEST_PROJECT/engine" bash "$AGENTSYNC_BIN" update "$FIXTURE_OLD"
    [ "$status" -eq 0 ]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_OLD" ]
    [[ "$output" == *"$FIXTURE_OLD"* ]]
}

@test "update <version>: rejects a version that is not a release tag" {
    env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh" >/dev/null
    run env AGENTSYNC_HOME="$TEST_PROJECT/engine" bash "$AGENTSYNC_BIN" update "$FIXTURE_ABSENT"
    [ "$status" -ne 0 ]
    [[ "$output" == *"$FIXTURE_ABSENT"* ]]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_NEW" ]
}
