#!/usr/bin/env bash
# Post-operation witnesses for guarded CLI rollback.
#
# <snapshot>/after.tsv records the state of every target once the operation
# that took the snapshot has finished, bound to the snapshot's targets.tsv.
# Rollback compares the live targets with it, so a path created or edited
# after that operation is never discarded silently. Symlinks are leaves
# compared by link text; their destinations are never read.
#
# Depends on: backup.sh (_backup_canonical_root, _backup_snapshot_path,
# _backup_validate_rel, _backup_safe_target_path_r), manifest.sh
# (_manifest_hash_cmd, _manifest_hash_stream).

BACKUP_WITNESS_SCHEMA="post-state-v2"
BACKUP_SEAL_REASON=""
BACKUP_PREFLIGHT_STATUS=""
BACKUP_PREFLIGHT_DETAIL=""

# Paths and link text are stored with %, tab, LF, and CR percent-encoded so one
# record stays one tab-separated line.
_backup_witness_escape_r() {
    local text="$1" tab=$'\t' lf=$'\n' cr=$'\r'
    text="${text//%/%25}"
    text="${text//$tab/%09}"
    text="${text//$lf/%0A}"
    REPLY="${text//$cr/%0D}"
}

_backup_witness_hash_one_r() {
    local tool="$2" out
    REPLY="?"
    # $tool may be "shasum -a 256" — intentional word split.
    # shellcheck disable=SC2086
    out=$($tool < "$1" 2>/dev/null) || return 1
    REPLY="${out:0:64}"
}

# Appends one record per path below <abs> to the _WITNESS_* arrays of the
# calling _backup_witness_print.
_backup_witness_walk() {
    local abs="$1" rel="$2" index child
    index=${#_WITNESS_PATHS[@]}
    _backup_witness_escape_r "$rel"
    _WITNESS_PATHS[index]="$REPLY"
    _WITNESS_VALUES[index]="-"
    if [[ -L "$abs" ]]; then
        _WITNESS_KINDS[index]="link"
        _WITNESS_LINKS+=("$abs")
        _WITNESS_LINK_SLOTS+=("$index")
    elif [[ -f "$abs" ]]; then
        _WITNESS_KINDS[index]="file"
        [[ ! -x "$abs" ]] || _WITNESS_KINDS[index]="exec"
        _WITNESS_FILES+=("$abs")
        _WITNESS_FILE_SLOTS+=("$index")
    elif [[ -d "$abs" ]]; then
        _WITNESS_KINDS[index]="dir"
        for child in "$abs"/*; do
            _backup_witness_walk "$child" "$rel/${child##*/}"
        done
    elif [[ -e "$abs" ]]; then
        _WITNESS_KINDS[index]="other"
    else
        _WITNESS_KINDS[index]="missing"
    fi
}

# Print the witness of the targets listed in <targets.tsv>. Mode "seal" fails on
# an unreadable file or link; mode "check" records it as "?" so it compares as
# changed. On failure, prints the reason on stderr and returns 1.
# Usage: _backup_witness_print <canonical-root> <targets.tsv> <seal|check>
_backup_witness_print() (
    local root="$1" targets="$2" mode="$3"
    LC_ALL=C
    shopt -s dotglob nullglob

    local tool
    tool=$(_manifest_hash_cmd)
    if [[ -z "$tool" ]]; then
        echo "no SHA-256 tool (sha256sum or shasum) is installed" >&2
        exit 1
    fi
    if [[ -L "$targets" ]] || [[ ! -f "$targets" ]]; then
        echo "the snapshot target list is missing" >&2
        exit 1
    fi

    local -a _WITNESS_KINDS=() _WITNESS_VALUES=() _WITNESS_PATHS=()
    local -a _WITNESS_FILES=("$targets") _WITNESS_FILE_SLOTS=(-1)
    local -a _WITNESS_LINKS=() _WITNESS_LINK_SLOTS=()
    local state rel extra
    while IFS=$'\t' read -r state rel extra || [[ -n "$state$rel$extra" ]]; do
        [[ -n "$state$rel$extra" ]] || continue
        # A trailing slash would make [[ -L ]] follow a symlinked target.
        if [[ "$state" != "present" && "$state" != "missing" ]] || [[ -n "$extra" ]] || \
           [[ "$rel" == */ || "$rel" == *//* ]] || ! _backup_validate_rel "$rel" 2>/dev/null; then
            echo "the snapshot target list is invalid" >&2
            exit 1
        fi
        if ! _backup_safe_target_path_r "$root" "$rel" 2>/dev/null; then
            echo "target is not inside the project: $rel" >&2
            exit 1
        fi
        _backup_witness_walk "$REPLY" "$rel"
    done < "$targets"

    local -a digests=() batch=() batch_index=() lines=()
    local i output line
    for ((i = 0; i < ${#_WITNESS_FILES[@]}; i++)); do
        if [[ "${_WITNESS_FILES[i]}" == *$'\n'* ]]; then
            _backup_witness_hash_one_r "${_WITNESS_FILES[i]}" "$tool" || true
            digests[i]="$REPLY"
        else
            batch+=("${_WITNESS_FILES[i]}")
            batch_index+=("$i")
        fi
    done
    # Hash tools escape a name containing a newline or backslash by prefixing the
    # line with "\"; newline names were hashed from stdin above.
    output=$(printf '%s\0' "${batch[@]}" | _manifest_hash_stream) || output=""
    IFS=$'\n' read -r -d '' -a lines <<< "$output" || true
    for ((i = 0; i < ${#batch[@]}; i++)); do
        if [[ ${#lines[@]} -eq ${#batch[@]} ]]; then
            line="${lines[i]#\\}"
            digests[batch_index[i]]="${line:0:64}"
        else
            _backup_witness_hash_one_r "${batch[i]}" "$tool" || true
            digests[batch_index[i]]="$REPLY"
        fi
    done

    local hex_re='^[0-9a-f]{64}$'
    for ((i = 0; i < ${#_WITNESS_FILES[@]}; i++)); do
        if [[ ! "${digests[i]}" =~ $hex_re ]]; then
            if [[ $i -eq 0 ]] || [[ "$mode" == "seal" ]]; then
                echo "could not hash ${_WITNESS_FILES[i]}" >&2
                exit 1
            fi
            digests[i]="?"
        fi
        [[ $i -eq 0 ]] || _WITNESS_VALUES[_WITNESS_FILE_SLOTS[i]]="${digests[i]}"
    done

    if [[ ${#_WITNESS_LINKS[@]} -gt 0 ]]; then
        lines=()
        output=$(printf '%s\0' "${_WITNESS_LINKS[@]}" | xargs -0 readlink 2>/dev/null) || output=""
        IFS=$'\n' read -r -d '' -a lines <<< "$output" || true
        for ((i = 0; i < ${#_WITNESS_LINKS[@]}; i++)); do
            if [[ ${#lines[@]} -eq ${#_WITNESS_LINKS[@]} ]]; then
                output="${lines[i]}"
            else
                output=$(readlink "${_WITNESS_LINKS[i]}" 2>/dev/null && printf '.') || output=""
                if [[ "$output" != *. ]]; then
                    if [[ "$mode" == "seal" ]]; then
                        echo "could not read link ${_WITNESS_LINKS[i]}" >&2
                        exit 1
                    fi
                    _WITNESS_VALUES[_WITNESS_LINK_SLOTS[i]]="?"
                    continue
                fi
                output="${output%.}"
                output="${output%$'\n'}"
            fi
            _backup_witness_escape_r "$output"
            _WITNESS_VALUES[_WITNESS_LINK_SLOTS[i]]="$REPLY"
        done
    fi

    printf '%s\t%s\n' "$BACKUP_WITNESS_SCHEMA" "${digests[0]}"
    for ((i = 0; i < ${#_WITNESS_PATHS[@]}; i++)); do
        printf '%s\t%s\t%s\n' "${_WITNESS_KINDS[i]}" "${_WITNESS_VALUES[i]}" "${_WITNESS_PATHS[i]}"
    done
)

# Record the post-operation state of a snapshot's targets. Never replaces an
# existing record. On failure the snapshot stays unsealed, nothing is printed,
# and BACKUP_SEAL_REASON says why.
# Usage: backup_seal <project-root> <snapshot-path-or-id>
# shellcheck disable=SC2034
backup_seal() {
    local root snapshot stage reason
    BACKUP_SEAL_REASON=""
    if ! root=$(_backup_canonical_root "$1" 2>/dev/null) || \
       ! snapshot=$(_backup_snapshot_path "$root" "$2" 2>/dev/null); then
        BACKUP_SEAL_REASON="the snapshot is missing or incomplete"
        return 1
    fi
    if [[ -e "$snapshot/after.tsv" ]] || [[ -L "$snapshot/after.tsv" ]]; then
        BACKUP_SEAL_REASON="the snapshot already has a post-operation record"
        return 1
    fi
    # Staged in the store root so _backup_sweep_stale_staging reclaims a record
    # abandoned by a killed run; the rename stays on one filesystem.
    if ! stage=$(mktemp "${snapshot%/*}/.tmp.seal.$$.XXXXXX" 2>/dev/null); then
        BACKUP_SEAL_REASON="could not stage the post-operation record"
        return 1
    fi
    if ! reason=$( { _backup_witness_print "$root" "$snapshot/targets.tsv" seal > "$stage"; } 2>&1); then
        rm -f "$stage"
        BACKUP_SEAL_REASON="${reason:-could not inspect the targets}"
        return 1
    fi
    if ! mv "$stage" "$snapshot/after.tsv" 2>/dev/null; then
        rm -f "$stage"
        BACKUP_SEAL_REASON="could not write the post-operation record"
        return 1
    fi
}

# Print the first path whose record differs between two witness bodies into
# REPLY. Returns 1 when <stored> is not a well-formed body.
_backup_witness_first_difference_r() {
    local -a stored=() current=()
    IFS=$'\n' read -r -d '' -a stored <<< "$1" || true
    IFS=$'\n' read -r -d '' -a current <<< "$2" || true
    [[ ${#stored[@]} -gt 0 ]] || return 1

    local record_re=$'^((missing|dir|other)\t-|(file|exec)\t([0-9a-f]{64})|link\t[^\t]+)\t[^\t]+$'
    local line tab=$'\t'
    for line in "${stored[@]}"; do
        [[ "$line" =~ $record_re ]] || return 1
    done

    local i=0 j stored_path current_path
    while [[ $i -lt ${#stored[@]} && $i -lt ${#current[@]} ]] && \
          [[ "${stored[i]}" == "${current[i]}" ]]; do
        i=$((i + 1))
    done
    if [[ $i -ge ${#stored[@]} && $i -ge ${#current[@]} ]]; then
        return 1
    elif [[ $i -ge ${#current[@]} ]]; then
        REPLY="${stored[i]##*"$tab"}"
        return 0
    elif [[ $i -ge ${#stored[@]} ]]; then
        REPLY="${current[i]##*"$tab"}"
        return 0
    fi

    stored_path="${stored[i]##*"$tab"}"
    current_path="${current[i]##*"$tab"}"
    REPLY="$stored_path"
    [[ "$stored_path" != "$current_path" ]] || return 0
    for ((j = i + 1; j < ${#current[@]}; j++)); do
        if [[ "${current[j]##*"$tab"}" == "$stored_path" ]]; then
            REPLY="$current_path"
            return 0
        fi
    done
}

# Compare the live targets with a snapshot's post-operation record. Read-only.
# Sets BACKUP_PREFLIGHT_STATUS to clean, unsealed, or conflict, and
# BACKUP_PREFLIGHT_DETAIL to why the snapshot counts as unsealed or to the first
# differing path (encoded as in after.tsv).
# Usage: backup_preflight <canonical-root> <snapshot-path>
# shellcheck disable=SC2034
backup_preflight() {
    local root="$1" snapshot="$2"
    local record="$snapshot/after.tsv" stored="" current
    BACKUP_PREFLIGHT_STATUS="unsealed"
    BACKUP_PREFLIGHT_DETAIL="has no post-operation record"
    [[ -f "$record" && ! -L "$record" ]] || return 0

    IFS= read -r -d '' stored < "$record" || true
    stored="${stored%$'\n'}"
    local header_re=$'^post-state-v2\t[0-9a-f]{64}$'
    BACKUP_PREFLIGHT_DETAIL="has a malformed post-operation record"
    [[ "$stored" == *$'\n'* ]] || return 0
    [[ "${stored%%$'\n'*}" =~ $header_re ]] || return 0

    if [[ -z "$(_manifest_hash_cmd)" ]]; then
        BACKUP_PREFLIGHT_DETAIL="cannot be checked without a SHA-256 tool (sha256sum or shasum)"
        return 0
    fi
    if ! current=$(_backup_witness_print "$root" "$snapshot/targets.tsv" check 2>/dev/null); then
        BACKUP_PREFLIGHT_DETAIL="cannot be checked because its targets could not be inspected"
        return 0
    fi
    if [[ "${stored%%$'\n'*}" != "${current%%$'\n'*}" ]]; then
        BACKUP_PREFLIGHT_DETAIL="has a post-operation record that does not match its target list"
        return 0
    fi
    if [[ "$stored" == "$current" ]]; then
        BACKUP_PREFLIGHT_STATUS="clean"
        BACKUP_PREFLIGHT_DETAIL=""
        return 0
    fi
    _backup_witness_first_difference_r "${stored#*$'\n'}" "${current#*$'\n'}" || return 0
    BACKUP_PREFLIGHT_STATUS="conflict"
    BACKUP_PREFLIGHT_DETAIL="$REPLY"
}
