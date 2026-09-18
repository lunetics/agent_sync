#!/usr/bin/env bats
# install.sh and `update <version>` against a purpose-built origin repository
# and a curl stand-in that serves fixture binary releases. Every path the
# installer touches is redirected into the test project so the developer's
# ~/.agentsync, PATH symlink, and shell rc are never modified.
#
# The origin is a fixture, not this repository: `actions/checkout` clones without
# tags, so pinning to a real release tag fails on CI even though it passes on a
# developer's full clone. The fixture carries its own tags and keeps the tests
# independent of this repository's tag history. The binary release is a fixture
# too: an archive laid out as cargo-dist lays it out, its binary a script.

load test_helper

# Tags the fixture origin publishes. Deliberately outside any real release
# series so they cannot be mistaken for this project's versions.
FIXTURE_NEW="9.9.2"
FIXTURE_OLD="9.9.1"
FIXTURE_ABSENT="999.0.0"

# Build the origin inside the per-test project rather than sharing one from
# setup_file: bats versions differ in whether an exported setup_file variable
# reaches the tests, and an empty origin URL fails every test here for a reason
# that has nothing to do with the installer.
#
# `update` fetches `origin main` by name, so the fixture needs that branch.
# `git branch -M` rather than `init -b`: the latter needs git >= 2.28, and the
# force form is a no-op when the default branch is already called main.
_build_origin_fixture() {
    local origin="$1"
    mkdir -p "$origin"
    (
        cd "$origin" || exit 1
        git init --quiet
        git config user.email "test@test.com"
        git config user.name "Test"

        cp -R "$REPO_ROOT/bin" "$REPO_ROOT/lib" .
        printf '%s\n' "$FIXTURE_OLD" > VERSION
        git add -A
        git commit --quiet -m "fixture engine $FIXTURE_OLD"
        git tag "$FIXTURE_OLD"
        git branch -M main

        printf '%s\n' "$FIXTURE_NEW" > VERSION
        git commit --quiet -am "fixture engine $FIXTURE_NEW"
        git tag "$FIXTURE_NEW"
    )
}

# A `curl` on PATH that serves $FAKE_RELEASES for the URLs the installer and
# `update` build and prints the HTTP status as `curl -w %{http_code}` does.
# Nothing here reaches the network: a tag without a fixture release is a 404.
_install_curl_stub() {
    mkdir -p "$TEST_PROJECT/stub" "$FAKE_RELEASES/tags"
    cat > "$TEST_PROJECT/stub/curl" <<'EOF'
#!/usr/bin/env bash
out=""; url=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        -w|--max-time) shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
    esac
done
case "$url" in
    https://api.github.com/repos/yelmuratoff/agent_sync/releases/latest)
        file="$FAKE_RELEASES/latest.json" ;;
    https://api.github.com/repos/yelmuratoff/agent_sync/git/ref/tags/*)
        file="$FAKE_RELEASES/tags/${url##*/}" ;;
    https://github.com/yelmuratoff/agent_sync/releases/download/*)
        rest="${url#*/releases/download/}"; tag="${rest%%/*}"; name="${rest#*/}"
        case "$name" in
            agentsync-*.sha256) name="archive.tar.xz.sha256" ;;
            agentsync-*) name="archive.tar.xz" ;;
        esac
        file="$FAKE_RELEASES/$tag/$name" ;;
    *) printf '000'; exit 6 ;;
esac
if [ -f "$file" ]; then cp "$file" "$out"; printf '200'; else printf '404'; fi
EOF
    chmod +x "$TEST_PROJECT/stub/curl"
}

# A fixture binary release for <tag>: the archive holds a script that answers
# `version` with the tag, plus a CHANGELOG.md, under one directory.
# Usage: publish_release <tag>
publish_release() {
    local tag="$1"
    local top="$TEST_PROJECT/rel-$tag/agentsync-fixture"
    mkdir -p "$top" "$FAKE_RELEASES/$tag"
    cat > "$top/agentsync" <<EOF
#!/usr/bin/env bash
case "\${1:-}" in
    version) echo "agentsync v$tag" ;;
    *) exit 1 ;;
esac
EOF
    chmod +x "$top/agentsync"
    printf '# Changelog\n\n## %s\n\n- Fixture.\n' "$tag" > "$top/CHANGELOG.md"
    tar -cJf "$FAKE_RELEASES/$tag/archive.tar.xz" -C "$TEST_PROJECT/rel-$tag" agentsync-fixture
    printf '%s  archive.tar.xz\n' "$(file_sha256 "$FAKE_RELEASES/$tag/archive.tar.xz")" \
        > "$FAKE_RELEASES/$tag/archive.tar.xz.sha256"
}

setup() {
    setup_test_project
    export HOME="$TEST_PROJECT/home"
    mkdir -p "$HOME"
    touch "$HOME/.zshrc"

    INSTALL_ORIGIN="$TEST_PROJECT/origin"
    _build_origin_fixture "$INSTALL_ORIGIN"

    FAKE_RELEASES="$TEST_PROJECT/releases"
    export FAKE_RELEASES
    _install_curl_stub

    export AGENTSYNC_REPO_URL="$INSTALL_ORIGIN"
    export AGENTSYNC_INSTALL_DIR="$TEST_PROJECT/engine"
    export AGENTSYNC_BIN_DIR="$TEST_PROJECT/bin"
    # `update` re-links whichever agentsync is on PATH; keep it the test one,
    # and keep curl the stand-in.
    export PATH="$TEST_PROJECT/bin:$TEST_PROJECT/stub:$PATH"
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

@test "install: the latest binary release is downloaded, verified, and linked" {
    publish_release "$FIXTURE_NEW"
    printf '{"tag_name":"%s"}\n' "$FIXTURE_NEW" > "$FAKE_RELEASES/latest.json"
    run bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [[ "$output" == *"Installed successfully!"*"v$FIXTURE_NEW"* ]]
    [ -x "$TEST_PROJECT/engine/bin/agentsync" ]
    [ -L "$TEST_PROJECT/bin/agentsync" ]
    [ "$("$TEST_PROJECT/bin/agentsync" version)" = "agentsync v$FIXTURE_NEW" ]
    [ ! -d "$TEST_PROJECT/engine/.git" ]
    ! grep -q "AGENTSYNC_HOME" "$HOME/.zshrc"
}

@test "install: AGENTSYNC_VERSION pins a binary release" {
    publish_release "$FIXTURE_NEW"
    run env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [ "$("$TEST_PROJECT/bin/agentsync" version)" = "agentsync v$FIXTURE_NEW" ]
    [ ! -d "$TEST_PROJECT/engine/.git" ]
}

@test "install: a checksum mismatch aborts before anything is linked" {
    publish_release "$FIXTURE_NEW"
    printf '0000  archive.tar.xz\n' > "$FAKE_RELEASES/$FIXTURE_NEW/archive.tar.xz.sha256"
    run env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 1 ]
    [[ "$output" == *"checksum mismatch"* ]]
    [ ! -e "$TEST_PROJECT/engine/bin/agentsync" ]
    [ ! -e "$TEST_PROJECT/bin/agentsync" ]
}

@test "install: without a pin an unreachable GitHub fails clearly" {
    run bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 1 ]
    [[ "$output" == *"could not read the latest release"* ]]
    [ ! -e "$TEST_PROJECT/bin/agentsync" ]
}

@test "install: AGENTSYNC_VERSION pins a tag without a binary from source" {
    run env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [[ "$output" == *"predates the binary releases"* ]]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_NEW" ]
    [ -e "$TEST_PROJECT/bin/agentsync" ]
    grep -q "AGENTSYNC_HOME" "$HOME/.zshrc"
}

@test "install: an unknown AGENTSYNC_VERSION fails clearly" {
    run env AGENTSYNC_VERSION="$FIXTURE_ABSENT" bash "$REPO_ROOT/install.sh"
    [ "$status" -ne 0 ]
    [[ "$output" == *"$FIXTURE_ABSENT"* ]]
}

@test "install: re-running with a different pin moves an existing source install" {
    env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh" >/dev/null
    run env AGENTSYNC_VERSION="$FIXTURE_OLD" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_OLD" ]
}

@test "install: a binary release replaces an existing source install's link" {
    env AGENTSYNC_VERSION="$FIXTURE_OLD" bash "$REPO_ROOT/install.sh" >/dev/null
    publish_release "$FIXTURE_NEW"
    run env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [ "$("$TEST_PROJECT/bin/agentsync" version)" = "agentsync v$FIXTURE_NEW" ]
    [ -d "$TEST_PROJECT/engine/.git" ]
}

@test "update <version>: pins an installed engine to that release" {
    env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh" >/dev/null
    run env AGENTSYNC_HOME="$TEST_PROJECT/engine" bash "$TEST_PROJECT/engine/bin/agentsync.sh" update "$FIXTURE_OLD"
    [ "$status" -eq 0 ]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_OLD" ]
    [[ "$output" == *"$FIXTURE_OLD"* ]]
    [[ "$output" != *"Switched to the agentsync binary"* ]]
}

@test "update <version>: rejects a version that is not a release tag" {
    env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh" >/dev/null
    run env AGENTSYNC_HOME="$TEST_PROJECT/engine" bash "$TEST_PROJECT/engine/bin/agentsync.sh" update "$FIXTURE_ABSENT"
    [ "$status" -ne 0 ]
    [[ "$output" == *"$FIXTURE_ABSENT"* ]]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_NEW" ]
}

@test "update <version>: a source install switches to the binary when the release has one" {
    env AGENTSYNC_VERSION="$FIXTURE_OLD" bash "$REPO_ROOT/install.sh" >/dev/null
    publish_release "$FIXTURE_NEW"
    run env AGENTSYNC_HOME="$TEST_PROJECT/engine" bash "$TEST_PROJECT/engine/bin/agentsync.sh" update "$FIXTURE_NEW"
    [ "$status" -eq 0 ]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_NEW" ]
    [[ "$output" == *"Switched to the agentsync binary"* ]]
    [ -x "$TEST_PROJECT/engine/bin/agentsync" ]
    [ "$(readlink "$TEST_PROJECT/bin/agentsync")" = "$TEST_PROJECT/engine/bin/agentsync" ]
    [ "$("$TEST_PROJECT/bin/agentsync" version)" = "agentsync v$FIXTURE_NEW" ]
}

@test "update <version>: a bad checksum keeps the source install on Bash" {
    env AGENTSYNC_VERSION="$FIXTURE_OLD" bash "$REPO_ROOT/install.sh" >/dev/null
    publish_release "$FIXTURE_NEW"
    printf '0000  archive.tar.xz\n' > "$FAKE_RELEASES/$FIXTURE_NEW/archive.tar.xz.sha256"
    run env AGENTSYNC_HOME="$TEST_PROJECT/engine" bash "$TEST_PROJECT/engine/bin/agentsync.sh" update "$FIXTURE_NEW"
    [ "$status" -eq 0 ]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_NEW" ]
    [[ "$output" == *"checksum mismatch"* ]]
    [ ! -e "$TEST_PROJECT/engine/bin/agentsync" ]
    [ "$(readlink "$TEST_PROJECT/bin/agentsync")" = "$TEST_PROJECT/engine/bin/agentsync.sh" ]
}
