#!/usr/bin/env bash
# Shared test helpers for bats tests.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# The binary under test: the release build, `cargo build --release` first.
AGENTSYNC_BIN="${AGENTSYNC_NATIVE_BIN:-$REPO_ROOT/target/release/agentsync}"
if [[ ! -x "$AGENTSYNC_BIN" ]] && [[ -x "$AGENTSYNC_BIN.exe" ]]; then
    AGENTSYNC_BIN="$AGENTSYNC_BIN.exe"
fi
export AGENTSYNC_BIN

# Out-of-repo trust signals from the developer's shell must not leak into tests,
# and neither may the developer's install: `release` falls back to
# AGENTSYNC_HOME when the working directory is not a checkout.
unset AGENTSYNC_ALLOW_POST_SYNC AGENTSYNC_SKIP_POST_SYNC AGENTSYNC_SKIP_HOOKS AGENTSYNC_EXTERNAL_SOURCE_ROOTS AGENTSYNC_HOME

# The developer's own git config must not decide test outcomes — a global
# core.hooksPath, for one, moves where hooks are installed. A path that does not
# exist reads as empty config on every platform, where /dev/null is a device
# Git Bash on Windows does not treat as a config file.
GIT_CONFIG_GLOBAL="${TMPDIR:-/tmp}/agentsync-tests-absent-gitconfig"
GIT_CONFIG_SYSTEM="$GIT_CONFIG_GLOBAL"
export GIT_CONFIG_GLOBAL GIT_CONFIG_SYSTEM

# Under Git Bash the binary spells paths as `C:/Users/...`, so the test
# project is named the same way and assertions on its path match the output.
host_path() {
    case "$(uname -s 2>/dev/null)" in
        MINGW*|MSYS*|CYGWIN*) cygpath -ml "$1" ;;
        *) printf '%s\n' "$1" ;;
    esac
}

setup_test_project() {
    TEST_PROJECT="$(host_path "$(mktemp -d "${TMPDIR:-/tmp}/agentsync_test.XXXXXX")")"
    cd "$TEST_PROJECT" || return 1
    git init --quiet
    git config user.email "test@test.com"
    git config user.name "Test"
    export AGENTSYNC_HOME="$REPO_ROOT"
}

# rm -rf that never fails teardown. A cleanup race (a settling git task or a CI
# filesystem creating an entry mid-delete) or a locked file on Windows git-bash
# can make rm exit non-zero; swallow that. One immediate retry catches the common
# transient. The target is an ephemeral /tmp dir on a throwaway runner, so a
# leftover never matters — only an asserted test failing on cleanup would. No
# sleep: on Windows the rm frequently needs the retry, and per-test sleeps there
# add up to tens of minutes.
_rm_rf_resilient() {
    local target="${1:-}"
    [[ -n "$target" ]] && [[ -d "$target" ]] || return 0
    rm -rf "$target" 2>/dev/null && return 0
    rm -rf "$target" 2>/dev/null || true
}

# Clean up temporary project
teardown_test_project() {
    [[ -n "${TEST_PROJECT:-}" ]] && _rm_rf_resilient "$TEST_PROJECT"
}

# Run the binary under test, never an installed agentsync.
run_agentsync() {
    "$AGENTSYNC_BIN" "$@"
}

# SHA-256 of one file, in the engine's own detection order. Git Bash on Windows
# ships sha256sum but not shasum, so a bare `shasum` call there prints nothing —
# which makes a before/after comparison pass by comparing two empty strings.
# Fails loudly instead when neither tool exists.
file_sha256() {
    local file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        echo "file_sha256: no sha256sum or shasum on PATH" >&2
        return 1
    fi
}

# Git for Windows deep-copies `ln -s` unless native links are requested explicitly.
create_test_symlink() {
    local target="$1"
    local link="$2"
    local msys_options="${MSYS:-}"

    MSYS="${msys_options:+$msys_options }winsymlinks:nativestrict" \
        ln -s "$target" "$link" || return 1
    if [[ ! -L "$link" ]]; then
        echo "Failed to create test symlink: $link" >&2
        return 1
    fi
}

# Enable tools via agentsync CLI (writes to tools.enabled in agent_sync.yaml).
# Call after `init`. Passes --no-scaffold so existing tests stay deterministic —
# resolver-level tests can still write overrides at whichever layout they test.
# Tests that specifically validate enable's scaffolding call `run_agentsync enable`.
# Usage: enable_tools claude cursor copilot
enable_tools() {
    run_agentsync enable "$@" --no-scaffold >/dev/null
}

# ── Shared seed/clone fixture ────────────────────────────────────────────────
# Pattern: setup_file builds one seeded project (git init + agentsync init);
# each setup() clones it (APFS clonefile on macOS, cp -R elsewhere). Cuts the
# per-test cost of a fresh init from ~250 ms to ~10-30 ms.

# Usage (inside setup_file): seed_project [extra args forwarded to `init`]
seed_project() {
    TEST_SEED="$(host_path "$(mktemp -d "${TMPDIR:-/tmp}/agentsync_seed.XXXXXX")")"
    export TEST_SEED
    (
        cd "$TEST_SEED" || exit 1
        git init --quiet
        git config user.email "test@test.com"
        git config user.name "Test"
        "$AGENTSYNC_BIN" init "$@" >/dev/null
    )
    export AGENTSYNC_HOME="$REPO_ROOT"
}

# Usage (inside teardown_file): clean up the seed dir
teardown_seed_project() {
    [[ -n "${TEST_SEED:-}" ]] && _rm_rf_resilient "$TEST_SEED"
}

# Usage (inside setup): create a per-test clone of the seed and cd into it.
# Uses APFS clonefile via `cp -c -R` when available — near-O(1) on macOS.
clone_seed() {
    TEST_PROJECT="$(host_path "$(mktemp -d "${TMPDIR:-/tmp}/agentsync_clone.XXXXXX")")"
    # mktemp already created the directory; remove so cp can clone into the path.
    rmdir "$TEST_PROJECT"
    if ! cp -c -R "$TEST_SEED" "$TEST_PROJECT" 2>/dev/null; then
        cp -R "$TEST_SEED" "$TEST_PROJECT"
    fi
    cd "$TEST_PROJECT" || return 1
    export AGENTSYNC_HOME="$REPO_ROOT"
}
