#!/usr/bin/env bash
# Wall-time measurement of the CLI's commands on a generated fixture, in one or
# both engines.
#
# Usage: bench.sh [--runs N] [--engines bash|native|both] [--keep]
#
# Prints a Markdown table of best and median wall time per command. The native
# rows go through bin/agentsync.sh, so they carry the Bash startup the strangler
# still pays; the last row times the binary directly, which is what the cutover
# in Phase 5 leaves.

set -euo pipefail

RUNS=3
ENGINES="both"
KEEP=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --runs)    RUNS="$2";    shift 2 ;;
        --engines) ENGINES="$2"; shift 2 ;;
        --keep)    KEEP=true;    shift ;;
        *) echo "Error: unknown flag: $1" >&2; exit 1 ;;
    esac
done

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CLI="$REPO_DIR/bin/agentsync.sh"
BINARY="$REPO_DIR/target/release/agentsync"
[[ -x "$BINARY" ]] || BINARY="$REPO_DIR/target/release/agentsync.exe"

command -v /usr/bin/time >/dev/null 2>&1 || {
    echo "Error: /usr/bin/time is required for sub-second timing." >&2
    exit 1
}

FIXTURE="$(mktemp -d "${TMPDIR:-/tmp}/agentsync_bench.XXXXXX")"
cleanup() { [[ "$KEEP" == "true" ]] || rm -rf "$FIXTURE"; }
trap cleanup EXIT INT TERM HUP

bash "$REPO_DIR/scripts/perf/make-fixture.sh" "$FIXTURE" >&2
git -C "$FIXTURE" init --quiet
git -C "$FIXTURE" config user.email bench@example.com
git -C "$FIXTURE" config user.name Bench

# One run of a command, in seconds with two decimals, from /usr/bin/time -p.
# Only the command's stdout is discarded: `time` reports on stderr, so the
# group's stderr is what carries the measurement.
_time_once() {
    local out
    out=$( { /usr/bin/time -p "$@" >/dev/null; } 2>&1 )
    printf '%s\n' "$out" | /usr/bin/awk '/^real/ { print $2 }'
}

# A sample set that does not parse is a silent zero, so prove the timer reads a
# known duration before any measurement is reported as a finding.
_self_check() {
    local seen
    seen=$(_time_once sleep 1)
    case "$seen" in
        1.0*|0.9*) return 0 ;;
        *)
            echo "Error: timer self-check failed: sleep 1 measured as '${seen:-nothing}'." >&2
            exit 1
            ;;
    esac
}

# Best and median of RUNS runs, as "best median".
_measure() {
    local i samples=""
    for ((i = 0; i < RUNS; i++)); do
        samples="$samples $(_time_once "$@")"
    done
    printf '%s\n' "$samples" | /usr/bin/awk '
        { for (i = 1; i <= NF; i++) v[n++] = $i + 0 }
        END {
            for (i = 0; i < n - 1; i++)
                for (j = 0; j < n - 1 - i; j++)
                    if (v[j] > v[j+1]) { t = v[j]; v[j] = v[j+1]; v[j+1] = t }
            printf "%.2f %.2f\n", v[0], v[int(n/2)]
        }'
}

_row() {
    local label="$1"
    shift
    local bash_result native_result
    bash_result=""
    native_result=""
    if [[ "$ENGINES" != "native" ]]; then
        bash_result=$(_measure env AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$REPO_DIR" \
            AGENTSYNC_REPO_ROOT="$FIXTURE" bash "$CLI" "$@")
    fi
    if [[ "$ENGINES" != "bash" ]]; then
        native_result=$(_measure env AGENTSYNC_NATIVE=1 AGENTSYNC_HOME="$REPO_DIR" \
            AGENTSYNC_REPO_ROOT="$FIXTURE" bash "$CLI" "$@")
    fi
    printf '| %s | %s | %s |\n' "$label" "${bash_result:-—}" "${native_result:-—}"
}

_self_check

# sync writes; every later measurement runs against an already-synced project,
# which is the state a user is in most of the time.
env AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$REPO_DIR" AGENTSYNC_REPO_ROOT="$FIXTURE" \
    bash "$CLI" sync >/dev/null 2>&1

printf '\n'
printf 'Fixture: %s\n' "$(find "$FIXTURE/.ai/src" -type f | wc -l | tr -d ' ') source files, 13 tools enabled"
printf 'Runs per cell: %s (best median, seconds)\n\n' "$RUNS"
printf '| Command | Bash | Native |\n'
printf '| --- | --- | --- |\n'
_row 'list' list
_row 'check' check
_row 'sync' sync
_row 'sync --if-stale' sync --if-stale

if [[ "$ENGINES" != "bash" ]] && [[ -x "$BINARY" ]]; then
    printf '| %s | %s | %s |\n' 'list, binary without the Bash entry point' '—' \
        "$(_measure env AGENTSYNC_REPO_ROOT="$FIXTURE" "$BINARY" list)"
fi
printf '\n'
