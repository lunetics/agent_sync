#!/usr/bin/env bash
# Path resolution utilities for AgentSync sync engine.
# Depends on globals: REPO_ROOT, REPO_ROOT_CANONICAL, DEFAULT_REPO_ROOT (set in sync.sh)
#
# Each resolver has two shapes: a `_r` variant returning through $REPLY, and an
# echo wrapper for the `$(...)` call sites. Hot loops use `_r`.

# Explicit source.* paths may be outside the project root. They are read-only
# inputs selected by the project configuration; destinations use the separate
# resolve_dest_path_r containment check below and are never widened by this
# allowlist.
CONFIGURED_SOURCE_ROOTS=()

register_configured_source_root() {
    local raw_path="$1"
    [[ -n "$raw_path" ]] || return 0

    local abs_path="$raw_path"
    if [[ "$abs_path" != /* ]]; then
        abs_path="${AGENTSYNC_INTERNAL_SOURCE_BASE_ROOT:-$REPO_ROOT}/$abs_path"
    fi

    local canonical_path=""
    canonicalize_with_existing_ancestor_r "$abs_path" 2>/dev/null && canonical_path="$REPLY"
    [[ -n "$canonical_path" ]] || return 0

    local root
    for root in "${CONFIGURED_SOURCE_ROOTS[@]+${CONFIGURED_SOURCE_ROOTS[@]}}"; do
        [[ "$root" == "$canonical_path" ]] && return 0
    done
    CONFIGURED_SOURCE_ROOTS+=("$canonical_path")
}

# Parent directory of a path. Matches `dirname` for every path this module
# handles; a pathname starting with exactly two slashes is implementation-defined
# in POSIX and normalisation collapses it before it can reach here.
_path_parent_r() {
    local path="$1"
    if [[ -z "$path" ]]; then
        REPLY="."
        return 0
    fi
    while [[ "$path" == */ ]] && [[ "$path" != "/" ]]; do
        path="${path%/}"
    done
    if [[ "$path" == "/" ]]; then
        REPLY="/"
        return 0
    fi
    if [[ "$path" != */* ]]; then
        REPLY="."
        return 0
    fi
    path="${path%/*}"
    while [[ "$path" == */ ]] && [[ "$path" != "/" ]]; do
        path="${path%/}"
    done
    [[ -z "$path" ]] && path="/"
    REPLY="$path"
}

# Final component of a path. Matches `basename` with no suffix argument, under
# the same two-slash caveat as _path_parent_r.
_path_leaf_r() {
    local path="$1"
    while [[ "$path" == */ ]] && [[ "$path" != "/" ]]; do
        path="${path%/}"
    done
    if [[ "$path" == "/" ]]; then
        REPLY="/"
        return 0
    fi
    REPLY="${path##*/}"
}

# Memo for `cd -P <dir> && pwd`. A key is a directory that existed when first
# probed, so a directory sync creates later arrives under its own key and cannot
# hit a stale entry. Failures are not cached.
_PATH_CANON_KEYS=()
_PATH_CANON_VALUES=()

_canon_dir_r() {
    local dir="$1"
    local count=${#_PATH_CANON_KEYS[@]}
    local index
    for ((index = 0; index < count; index++)); do
        if [[ "${_PATH_CANON_KEYS[index]}" == "$dir" ]]; then
            REPLY="${_PATH_CANON_VALUES[index]}"
            return 0
        fi
    done

    local canonical
    canonical=$(cd -P "$dir" 2>/dev/null && pwd) || return 1
    [[ -n "$canonical" ]] || return 1
    _PATH_CANON_KEYS+=("$dir")
    _PATH_CANON_VALUES+=("$canonical")
    REPLY="$canonical"
}

# Normalize a path lexically into an absolute path.
# Works even if the target does not exist yet.
normalize_absolute_path_r() {
    local path="$1"
    if [[ "$path" != /* ]]; then
        path="$REPO_ROOT/$path"
    fi

    # Already lexically canonical — no empty, "." or ".." segment to collapse.
    case "$path" in
        */ | *//* | */./* | */../* | */. | */..) ;;
        *)
            REPLY="$path"
            return 0
            ;;
    esac

    local -a segments normalized_segments
    IFS='/' read -r -a segments <<< "$path"
    normalized_segments=()

    local segment
    for segment in "${segments[@]}"; do
        case "$segment" in
            ""|".")
                continue
                ;;
            "..")
                if [[ ${#normalized_segments[@]} -gt 0 ]]; then
                    unset 'normalized_segments[${#normalized_segments[@]}-1]'
                fi
                ;;
            *)
                normalized_segments+=("$segment")
                ;;
        esac
    done

    local normalized="/"
    if [[ ${#normalized_segments[@]} -gt 0 ]]; then
        normalized="/${normalized_segments[0]}"
        local index
        for ((index = 1; index < ${#normalized_segments[@]}; index++)); do
            normalized="$normalized/${normalized_segments[$index]}"
        done
    fi

    REPLY="$normalized"
}

normalize_absolute_path() {
    normalize_absolute_path_r "$1"
    echo "$REPLY"
}

is_path_within_repo_root() {
    local candidate_path="$1"
    [[ "$candidate_path" == "$REPO_ROOT_CANONICAL" || "$candidate_path" == "$REPO_ROOT_CANONICAL/"* ]]
}

resolve_existing_ancestor_r() {
    local current="$1"
    local parent
    while [[ ! -e "$current" ]]; do
        _path_parent_r "$current"
        parent="$REPLY"
        if [[ "$parent" == "$current" ]]; then
            break
        fi
        current="$parent"
    done

    REPLY="$current"
}

resolve_existing_ancestor() {
    resolve_existing_ancestor_r "$1"
    echo "$REPLY"
}

canonicalize_with_existing_ancestor_r() {
    local path="$1"
    resolve_existing_ancestor_r "$path"
    local existing_ancestor="$REPLY"

    local existing_ancestor_canonical
    if [[ -d "$existing_ancestor" ]]; then
        _canon_dir_r "$existing_ancestor" || return 1
        existing_ancestor_canonical="$REPLY"
    else
        _path_parent_r "$existing_ancestor"
        _canon_dir_r "$REPLY" || return 1
        local ancestor_parent_canonical="$REPLY"
        _path_leaf_r "$existing_ancestor"
        existing_ancestor_canonical="$ancestor_parent_canonical/$REPLY"
    fi

    if [[ "$path" == "$existing_ancestor" ]]; then
        REPLY="$existing_ancestor_canonical"
        return 0
    fi

    local suffix="${path#"$existing_ancestor"}"
    if [[ -n "$suffix" ]] && [[ "$suffix" != /* ]]; then
        suffix="/$suffix"
    fi

    normalize_absolute_path_r "$existing_ancestor_canonical$suffix"
}

canonicalize_with_existing_ancestor() {
    canonicalize_with_existing_ancestor_r "$1" || return 1
    echo "$REPLY"
}

resolve_dest_path_r() {
    local raw_path="$1"
    local label="$2"

    if [[ -z "$raw_path" ]]; then
        log_error "$label is empty"
        return 1
    fi

    normalize_absolute_path_r "$raw_path"
    local abs_path="$REPLY"

    if ! canonicalize_with_existing_ancestor_r "$abs_path"; then
        log_error "Failed to canonicalize $label path: $raw_path"
        return 1
    fi
    local canonical_path="$REPLY"

    if ! is_path_within_repo_root "$canonical_path"; then
        log_error "$label resolves outside repository root: $raw_path -> $canonical_path"
        return 1
    fi

    REPLY="$abs_path"
}

resolve_dest_path() {
    resolve_dest_path_r "$1" "$2" || return 1
    echo "$REPLY"
}

is_path_safe_source() {
    local candidate_path="$1"
    if [[ "$candidate_path" == "$REPO_ROOT_CANONICAL" || "$candidate_path" == "$REPO_ROOT_CANONICAL/"* ]]; then
        return 0
    fi
    if [[ "$candidate_path" == "$DEFAULT_REPO_ROOT" || "$candidate_path" == "$DEFAULT_REPO_ROOT/"* ]]; then
        return 0
    fi
    local configured_root
    for configured_root in "${CONFIGURED_SOURCE_ROOTS[@]+${CONFIGURED_SOURCE_ROOTS[@]}}"; do
        if [[ "$candidate_path" == "$configured_root" || "$candidate_path" == "$configured_root/"* ]]; then
            return 0
        fi
    done
    # `shared:` overlay places child + parent files into a tmpdir; sync reads
    # from there. The tmpdir is owned by shared.sh and torn down on EXIT.
    if [[ -n "${SHARED_OVERLAY_DIR_CANONICAL:-}" ]] && \
       { [[ "$candidate_path" == "$SHARED_OVERLAY_DIR_CANONICAL" ]] || \
         [[ "$candidate_path" == "$SHARED_OVERLAY_DIR_CANONICAL/"* ]]; }; then
        return 0
    fi
    # Per-profile overlay tmpdir — same contract as the shared overlay, but a
    # separate global so a profile pass can compose on top of an active shared:.
    if [[ -n "${PROFILE_OVERLAY_DIR_CANONICAL:-}" ]] && \
       { [[ "$candidate_path" == "$PROFILE_OVERLAY_DIR_CANONICAL" ]] || \
         [[ "$candidate_path" == "$PROFILE_OVERLAY_DIR_CANONICAL/"* ]]; }; then
        return 0
    fi
    # Engine-owned base source overlay tmpdir — same contract again.
    if [[ -n "${BASE_SRC_OVERLAY_DIR_CANONICAL:-}" ]] && \
       { [[ "$candidate_path" == "$BASE_SRC_OVERLAY_DIR_CANONICAL" ]] || \
         [[ "$candidate_path" == "$BASE_SRC_OVERLAY_DIR_CANONICAL/"* ]]; }; then
        return 0
    fi
    return 1
}

resolve_source_path_r() {
    local raw_path="$1"
    local label="$2"

    if [[ -z "$raw_path" ]]; then
        log_error "$label is empty"
        return 1
    fi

    # First try resolving relative to the configured source base (normally the
    # user project; check may point it at the original project read-only).
    local source_base="${AGENTSYNC_INTERNAL_SOURCE_BASE_ROOT:-$REPO_ROOT}"
    normalize_absolute_path_r "$raw_path"
    if [[ "$raw_path" != /* ]]; then
        normalize_absolute_path_r "$source_base/$raw_path"
    fi
    local abs_path_target="$REPLY"
    local canonical_path_target=""
    if canonicalize_with_existing_ancestor_r "$abs_path_target" 2>/dev/null; then
        canonical_path_target="$REPLY"
    fi

    if [[ -n "$canonical_path_target" ]] && [[ -e "$canonical_path_target" ]] && is_path_safe_source "$canonical_path_target"; then
        REPLY="$abs_path_target"
        return 0
    fi

    # Fallback to DEFAULT_REPO_ROOT (the shipped package templates)
    local abs_path_fallback
    if [[ "$raw_path" == /* ]]; then
        abs_path_fallback="$raw_path"
    else
        abs_path_fallback="$DEFAULT_REPO_ROOT/$raw_path"
    fi

    local canonical_path_fallback=""
    if canonicalize_with_existing_ancestor_r "$abs_path_fallback" 2>/dev/null; then
        canonical_path_fallback="$REPLY"
    fi

    if [[ -n "$canonical_path_fallback" ]] && is_path_safe_source "$canonical_path_fallback"; then
        REPLY="$abs_path_fallback"
        return 0
    fi

    # If neither exists/valid, log error based on the primary target
    if [[ -n "$canonical_path_target" ]]; then
        if ! is_path_safe_source "$canonical_path_target"; then
            log_error "$label resolves outside safe source roots: $raw_path -> $canonical_path_target"
            return 1
        fi
    fi

    REPLY="$abs_path_target"
    return 0
}

resolve_source_path() {
    resolve_source_path_r "$1" "$2" || return 1
    echo "$REPLY"
}

# Walk up from a starting directory looking for a sibling `.ai/src/` directory,
# stopping at the first hit or the start's git repository boundary (whichever
# first). Does not climb past filesystem root. Skips the start directory
# itself so a project never claims itself as its own parent.
# Echoes the absolute path to the parent `.ai/src/` on success; returns 1 if
# no parent is found within the bounded search.
#
# Usage: find_parent_ai_src <start_dir>
find_parent_ai_src() {
    local start="$1"
    [[ -d "$start" ]] || return 1

    local current
    current=$(cd "$start" && pwd)

    # Determine the start's own git repo root (if any). We are willing to
    # climb to siblings inside the same repo, but not out of it.
    local start_git_root=""
    local probe="$current"
    while [[ -n "$probe" ]] && [[ "$probe" != "/" ]]; do
        if [[ -d "$probe/.git" || -f "$probe/.git" ]]; then
            start_git_root="$probe"
            break
        fi
        probe=$(dirname "$probe")
    done

    local parent
    parent=$(dirname "$current")
    while [[ "$parent" != "$current" ]]; do
        # Refuse to leave the start's git repo: if we have a git root and
        # `parent` is neither it nor inside it, stop the walk.
        if [[ -n "$start_git_root" ]] \
           && [[ "$parent" != "$start_git_root" ]] \
           && [[ "$parent" != "$start_git_root"/* ]]; then
            return 1
        fi
        if [[ -d "$parent/.ai/src" ]]; then
            echo "$parent/.ai/src"
            return 0
        fi
        current="$parent"
        parent=$(dirname "$current")
    done
    return 1
}

# Detect whether a directory lives inside a `.ai/` source tree (or is one).
# AgentSync writes tool outputs as siblings of `.ai/`, so a run rooted inside
# it would nest generated files under the source dir instead of the project.
# Echoes the project root — the parent of the shallowest `.ai/` segment in the
# path — when the directory is inside such a tree; returns 1 when it is not.
#
# Usage: ai_dir_enclosing_root <dir>
ai_dir_enclosing_root() {
    local dir="$1"
    [[ -d "$dir" ]] || return 1
    dir=$(cd "$dir" && pwd)

    local shallowest_ai="" current="$dir"
    while [[ "$current" != "/" && -n "$current" ]]; do
        if [[ "$(basename "$current")" == ".ai" ]]; then
            shallowest_ai="$current"
        fi
        current=$(dirname "$current")
    done

    [[ -n "$shallowest_ai" ]] || return 1
    dirname "$shallowest_ai"
}

# Find every AgentSync-managed `.ai/` directory below a root, in bottom-up
# alphabetical order (deeper paths first; siblings at the same depth sorted
# by LC_ALL=C path order). Used by `sync --workspace` and `dedupe --workspace`
# so cross-project commands behave deterministically between runs and between
# machines.
#
# A `.ai/` directory is included only when it contains either `src/` or
# `agent_sync.yaml` — bare `.ai/` directories from other tooling are skipped.
#
# Vendored trees carry their own `.ai/`, and syncing into `node_modules` writes
# config a package manager will discard. `.ai/` is pruned once matched, so a
# backup snapshot mirroring a project's `.ai/src` cannot be reported as a
# second project either.
#
# Echoes one absolute path per line. Returns 0 even when nothing is found.
#
# Usage: find_workspace_ai_dirs <root>
find_workspace_ai_dirs() {
    local root="$1"
    [[ -d "$root" ]] || return 0
    root=$(cd "$root" && pwd)

    # Two-key sort: depth descending (deeper first), then alphabetical within
    # each depth. Path-component count via awk -F/ keeps it pure coreutils.
    find "$root" \
            \( -name .git -o -name node_modules \) -prune -o \
            -type d -name ".ai" -prune -print 2>/dev/null \
        | while IFS= read -r d; do
            [[ -d "$d/src" || -f "$d/agent_sync.yaml" ]] || continue
            echo "$d"
          done \
        | LC_ALL=C awk -F/ '{ print NF "\t" $0 }' \
        | LC_ALL=C sort -k1,1rn -k2,2 \
        | cut -f2-
}

to_repo_relative_path_r() {
    local abs_path="$1"
    if [[ "$abs_path" == "$REPO_ROOT" ]]; then
        REPLY="."
        return 0
    fi

    if [[ "$abs_path" == "$REPO_ROOT/"* ]]; then
        REPLY="${abs_path#"$REPO_ROOT"/}"
        return 0
    fi

    REPLY=""
    log_error "Path is outside repository root: $abs_path"
    return 1
}

to_repo_relative_path() {
    to_repo_relative_path_r "$1" || return 1
    echo "$REPLY"
}
