#!/usr/bin/env bats
# `agentsync update` on a binary install: the binary run directly, as the
# installer links it, against a curl stand-in on PATH that serves a fixture
# release and a real tar archive. Skips when no binary is built.

load test_helper

setup_file() { seed_project; }
teardown_file() { teardown_seed_project; }

setup() {
    clone_seed
    NATIVE_BIN="${AGENTSYNC_NATIVE_BIN:-$REPO_ROOT/target/release/agentsync}"
    [[ -x "$NATIVE_BIN" ]] || skip "no native binary at $NATIVE_BIN"
    ENGINE_VERSION="$(cat "$REPO_ROOT/VERSION")"

    # A copy is what gets replaced, never the build under target/.
    INSTALL="$TEST_PROJECT/install"
    mkdir -p "$INSTALL/bin"
    cp "$NATIVE_BIN" "$INSTALL/bin/agentsync"
    AGENTSYNC="$INSTALL/bin/agentsync"
    printf '9.9.9\n' > "$INSTALL/.update_cache"

    FAKE_RELEASES="$TEST_PROJECT/releases"
    mkdir -p "$FAKE_RELEASES/tags" stub
    cat > stub/curl <<'EOF'
#!/usr/bin/env bash
# Serves $FAKE_RELEASES for the URLs update builds and prints the HTTP status
# as `curl -w %{http_code}` does; anything else fails as an unreachable host.
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
    chmod +x stub/curl
    export FAKE_RELEASES
    export PATH="$TEST_PROJECT/stub:$PATH"
}

teardown() { teardown_test_project; }

# A fixture release: an archive laid out as cargo-dist lays it out, its binary
# a script answering `version` and `__catalog`, its catalog the running
# binary's unless a dump is given.
# Usage: publish_release <tag> [<catalog dump file>]
publish_release() {
    local tag="$1" dump="${2:-}"
    local top="$TEST_PROJECT/rel-$tag/agentsync-fixture"
    mkdir -p "$top" "$FAKE_RELEASES/$tag"
    if [[ -n "$dump" ]]; then
        cp "$dump" "$top/catalog.dump"
    else
        "$AGENTSYNC" __catalog > "$top/catalog.dump"
    fi
    cat > "$top/agentsync" <<EOF
#!/usr/bin/env bash
case "\${1:-}" in
    version) echo "agentsync v$tag" ;;
    __catalog) cat "\$(dirname "\$0")/catalog.dump" ;;
    *) exit 1 ;;
esac
EOF
    chmod +x "$top/agentsync"
    printf '# Changelog\n\n## %s\n\n### Fixed\n\n- **Something** with `code`.\n\n## 0.1.0\n\n- Ancient.\n' "$tag" > "$top/CHANGELOG.md"
    tar -cJf "$FAKE_RELEASES/$tag/archive.tar.xz" -C "$TEST_PROJECT/rel-$tag" agentsync-fixture
    printf '%s  archive.tar.xz\n' "$(file_sha256 "$FAKE_RELEASES/$tag/archive.tar.xz")" \
        > "$FAKE_RELEASES/$tag/archive.tar.xz.sha256"
}

@test "update native: --help prints the usage without touching the network" {
    run "$AGENTSYNC" update --help
    [ "$status" -eq 0 ]
    [[ "$output" == *"--strict"* ]]
    [[ "$output" == *"the latest release"* ]]
}

@test "update native: an unknown flag is refused with status 2" {
    run "$AGENTSYNC" update --bogus
    [ "$status" -eq 2 ]
    [[ "$output" == *"Unknown flag: --bogus"* ]]
}

@test "update native: the latest release replaces the binary and prints its changelog" {
    publish_release 9.9.9
    printf '{"tag_name":"9.9.9","name":"9.9.9"}\n' > "$FAKE_RELEASES/latest.json"
    run "$AGENTSYNC" update
    [ "$status" -eq 0 ]
    [[ "$output" == *"Updating..."* ]]
    [[ "$output" == *"Updated! v$ENGINE_VERSION → v9.9.9"* ]]
    [[ "$output" == *"What's new in v9.9.9"* ]]
    [[ "$output" == *"• Something with code."* ]]
    [[ "$output" != *"Upstream touched"* ]]
    [ "$("$AGENTSYNC" version)" = "agentsync v9.9.9" ]
    [ ! -f "$INSTALL/.update_cache" ]
    [ ! -f .ai/.pending-resolutions.yaml ]
}

@test "update native: the running version is already up to date" {
    printf '{"tag_name":"%s"}\n' "$ENGINE_VERSION" > "$FAKE_RELEASES/latest.json"
    run "$AGENTSYNC" update
    [ "$status" -eq 0 ]
    [[ "$output" == *"Already up to date! (v$ENGINE_VERSION)"* ]]
    cmp -s "$AGENTSYNC" "$NATIVE_BIN"
    [ -f "$INSTALL/.update_cache" ]
}

@test "update native: <version> pins to that release" {
    publish_release 9.9.9
    run "$AGENTSYNC" update 9.9.9
    [ "$status" -eq 0 ]
    [[ "$output" == *"Pinning to v9.9.9..."* ]]
    [ "$("$AGENTSYNC" version)" = "agentsync v9.9.9" ]
}

@test "update native: a tag that is not a release is refused" {
    run "$AGENTSYNC" update 999.0.0
    [ "$status" -eq 1 ]
    [[ "$output" == *"No AgentSync release is tagged 999.0.0."* ]]
    cmp -s "$AGENTSYNC" "$NATIVE_BIN"
}

@test "update native: a tag older than the binary releases points at the installer" {
    printf '{"ref":"refs/tags/0.1.0"}\n' > "$FAKE_RELEASES/tags/0.1.0"
    run "$AGENTSYNC" update 0.1.0
    [ "$status" -eq 1 ]
    [[ "$output" == *"0.1.0 predates the binary releases"* ]]
    [[ "$output" == *"AGENTSYNC_VERSION=0.1.0 curl -fsSL https://raw.githubusercontent.com/yelmuratoff/agent_sync/main/install.sh | bash"* ]]
    cmp -s "$AGENTSYNC" "$NATIVE_BIN"
}

@test "update native: a checksum mismatch keeps the old binary" {
    publish_release 9.9.9
    printf '0000  archive.tar.xz\n' > "$FAKE_RELEASES/9.9.9/archive.tar.xz.sha256"
    run "$AGENTSYNC" update 9.9.9
    [ "$status" -eq 1 ]
    [[ "$output" == *"checksum mismatch"* ]]
    cmp -s "$AGENTSYNC" "$NATIVE_BIN"
}

@test "update native: an unreachable GitHub is reported" {
    run env PATH="$TEST_PROJECT/nobin" "$AGENTSYNC" update 9.9.9
    [ "$status" -eq 1 ]
    [[ "$output" == *"Failed to fetch updates from GitHub."* ]]
    cmp -s "$AGENTSYNC" "$NATIVE_BIN"
}

@test "update native: a changed overridden field is reported, queued, and fails --strict" {
    "$AGENTSYNC" __catalog > catalog.dump
    grep -q '^claude ' catalog.dump
    # The dump frames each YAML by byte length; a value of the same length
    # keeps the frame intact.
    sed 's|dest: ".claude/rules"|dest: ".claude/rulez"|' catalog.dump > changed.dump
    ! cmp -s catalog.dump changed.dump
    publish_release 9.9.9 changed.dump
    mkdir -p .ai/src/tools
    printf 'targets:\n  rules:\n    dest: ".claude/my-rules"\n' > .ai/src/tools/claude.yaml

    run "$AGENTSYNC" update 9.9.9
    [ "$status" -eq 0 ]
    [[ "$output" == *"Upstream touched fields you have overridden:"* ]]
    [[ "$output" == *"◆ targets.rules.dest"* ]]
    [[ "$output" == *"base: .claude/rules → .claude/rulez"* ]]
    [[ "$output" == *"your override: .claude/my-rules"* ]]
    [[ "$output" == *"Queued in .ai/.pending-resolutions.yaml"* ]]
    grep -q "from_version: \"$ENGINE_VERSION\"" .ai/.pending-resolutions.yaml
    grep -q 'to_version: "9.9.9"' .ai/.pending-resolutions.yaml
    grep -q 'your_override: ".claude/my-rules"' .ai/.pending-resolutions.yaml

    cp "$NATIVE_BIN" "$AGENTSYNC"
    run "$AGENTSYNC" update 9.9.9 --strict
    [ "$status" -eq 1 ]
    [ "$("$AGENTSYNC" version)" = "agentsync v9.9.9" ]
}

@test "update native: __catalog frames every shipped tool by byte length" {
    run "$AGENTSYNC" __catalog
    [ "$status" -eq 0 ]
    [ "$(grep -c '^[a-z]* [0-9]*$' <<<"$output")" -ge 13 ]
    grep -q '^claude [0-9]*$' <<<"$output"
}
