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

_backup_witness_unescape_r() {
    local text="$1" tab=$'\t' lf=$'\n' cr=$'\r'
    if [[ "$text" == *%* ]]; then
        text="${text//%0A/$lf}"
        text="${text//%09/$tab}"
        text="${text//%0D/$cr}"
        text="${text//%25/%}"
    fi
    REPLY="$text"
}

_backup_witness_hash_one_r() {
    local tool="$2" out
    REPLY=""
    # $tool may be "shasum -a 256" — intentional word split.
    # shellcheck disable=SC2086
    out=$($tool < "$1" 2>/dev/null) || return 1
    REPLY="${out:0:64}"
}

# Take the digest of <abs> from the next line of the batched hash stream on fd 3.
# A line for another path (a file the tool could not read produced none) stays
# pending in _WITNESS_PENDING for the next file, and REPLY is left empty.
_backup_witness_stream_digest_r() {
    local abs="$1" line listed
    REPLY=""
    if [[ "$_WITNESS_HAS_PENDING" != "true" ]]; then
        IFS= read -r _WITNESS_PENDING <&3 || return 0
        _WITNESS_HAS_PENDING="true"
    fi
    line="$_WITNESS_PENDING"
    listed="${line:66}"
    # GNU tools prefix a line with "\" and escape backslash, LF, and CR in its name.
    if [[ "$line" == \\* ]]; then
        line="${line#\\}"
        printf -v listed '%b' "${line:66}"
    fi
    [[ "${line:64:2}" == "  " || "${line:64:2}" == " *" ]] || return 0
    [[ "$listed" == "$abs" ]] || return 0
    _WITNESS_HAS_PENDING="false"
    REPLY="${line:0:64}"
}

# Print one "<kind>\t-\t<path>" line per path at and below <abs>.
_backup_witness_walk() {
    local abs="$1" rel="$2" child
    REPLY="$rel"
    [[ "$rel" != *[%$'\t\n\r']* ]] || _backup_witness_escape_r "$rel"
    if [[ -L "$abs" ]]; then
        printf 'link\t-\t%s\n' "$REPLY"
    elif [[ -f "$abs" && -x "$abs" ]]; then
        printf 'exec\t-\t%s\n' "$REPLY"
    elif [[ -f "$abs" ]]; then
        printf 'file\t-\t%s\n' "$REPLY"
    elif [[ -d "$abs" ]]; then
        printf 'dir\t-\t%s\n' "$REPLY"
        for child in "$abs"/*; do
            _backup_witness_walk "$child" "$rel/${child##*/}"
        done
    elif [[ -e "$abs" ]]; then
        printf 'other\t-\t%s\n' "$REPLY"
    else
        printf 'missing\t-\t%s\n' "$REPLY"
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

    local records
    records=$(
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
    ) || exit 1

    # One hash process for every file whose name has no newline; the tool's
    # output line would split on it, so those files are hashed alone below.
    local hashes
    hashes=$(
        {
            printf '%s\0' "$targets"
            while IFS=$'\t' read -r kind value path; do
                [[ "$kind" == "file" || "$kind" == "exec" ]] || continue
                if [[ "$path" == *%* ]]; then
                    _backup_witness_unescape_r "$path"
                    [[ "$REPLY" != *$'\n'* ]] || continue
                    path="$REPLY"
                fi
                printf '%s\0' "$root/$path"
            done <<< "$records"
        } | _manifest_hash_stream
    ) || true
    exec 3<<< "$hashes"

    local _WITNESS_PENDING="" _WITNESS_HAS_PENDING="false"
    local hex_re='^[0-9a-f]{64}$' kind value path abs link line
    _backup_witness_stream_digest_r "$targets"
    [[ -n "$REPLY" ]] || _backup_witness_hash_one_r "$targets" "$tool" || true
    if [[ ! "$REPLY" =~ $hex_re ]]; then
        echo "could not hash $targets" >&2
        exit 1
    fi
    printf '%s\t%s\n' "$BACKUP_WITNESS_SCHEMA" "$REPLY"

    [[ -n "$records" ]] || exit 0
    while IFS=$'\t' read -r kind value path; do
        case "$kind" in
            file|exec)
                abs="$root/$path"
                if [[ "$path" == *%* ]]; then
                    _backup_witness_unescape_r "$path"
                    abs="$root/$REPLY"
                fi
                value=""
                if [[ "$_WITNESS_HAS_PENDING" != "true" && "$abs" != *$'\n'* ]]; then
                    line=""
                    IFS= read -r line <&3 || true
                    if [[ "${line:64}" == "  $abs" ]]; then
                        value="${line:0:64}"
                    elif [[ -n "$line" ]]; then
                        _WITNESS_PENDING="$line"
                        _WITNESS_HAS_PENDING="true"
                    fi
                fi
                if [[ -z "$value" ]]; then
                    REPLY=""
                    [[ "$abs" == *$'\n'* ]] || _backup_witness_stream_digest_r "$abs"
                    [[ -n "$REPLY" ]] || _backup_witness_hash_one_r "$abs" "$tool" || true
                    value="$REPLY"
                fi
                if [[ ! "$value" =~ $hex_re ]]; then
                    if [[ "$mode" == "seal" ]]; then
                        echo "could not hash $abs" >&2
                        exit 1
                    fi
                    value="?"
                fi
                ;;
            link)
                _backup_witness_unescape_r "$path"
                abs="$root/$REPLY"
                link=$(readlink "$abs" 2>/dev/null && printf '.') || link=""
                if [[ "$link" == *. ]]; then
                    link="${link%.}"
                    _backup_witness_escape_r "${link%$'\n'}"
                    value="$REPLY"
                elif [[ "$mode" == "seal" ]]; then
                    echo "could not read link $abs" >&2
                    exit 1
                else
                    value="?"
                fi
                ;;
        esac
        printf '%s\t%s\t%s\n' "$kind" "$value" "$path"
    done <<< "$records"
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

# Print the first path whose record differs between two witness bodies.
# Returns 1 when <stored> is not a well-formed body.
_backup_witness_first_difference() (
    local stored="$1" current="$2" tab=$'\t' line
    local record_re=$'^((missing|dir|other)\t-|(file|exec)\t[0-9a-f]{64}|link\t[^\t]+)\t[^\t]+$'
    while IFS= read -r line; do
        [[ "$line" =~ $record_re ]] || exit 1
    done <<< "$stored"

    exec 3<<< "$stored" 4<<< "$current"
    local stored_line current_line stored_path current_path
    local stored_more current_more
    while :; do
        stored_more="true"
        current_more="true"
        IFS= read -r stored_line <&3 || stored_more="false"
        IFS= read -r current_line <&4 || current_more="false"
        if [[ "$stored_more" != "true" && "$current_more" != "true" ]]; then
            exit 1
        elif [[ "$current_more" != "true" ]]; then
            printf '%s' "${stored_line##*"$tab"}"
            exit 0
        elif [[ "$stored_more" != "true" ]]; then
            printf '%s' "${current_line##*"$tab"}"
            exit 0
        fi
        [[ "$stored_line" != "$current_line" ]] && break
    done

    stored_path="${stored_line##*"$tab"}"
    current_path="${current_line##*"$tab"}"
    if [[ "$stored_path" != "$current_path" ]]; then
        while IFS= read -r line <&4; do
            if [[ "${line##*"$tab"}" == "$stored_path" ]]; then
                printf '%s' "$current_path"
                exit 0
            fi
        done
    fi
    printf '%s' "$stored_path"
)

# Compare the live targets with a snapshot's post-operation record. Read-only.
# Sets BACKUP_PREFLIGHT_STATUS to clean, unsealed, or conflict, and
# BACKUP_PREFLIGHT_DETAIL to why the snapshot counts as unsealed or to the first
# differing path (encoded as in after.tsv).
# Usage: backup_preflight <canonical-root> <snapshot-path>
# shellcheck disable=SC2034
backup_preflight() {
    local root="$1" snapshot="$2"
    local record="$snapshot/after.tsv" stored="" current path
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
    path=$(_backup_witness_first_difference "${stored#*$'\n'}" "${current#*$'\n'}") || return 0
    BACKUP_PREFLIGHT_STATUS="conflict"
    BACKUP_PREFLIGHT_DETAIL="$path"
}
