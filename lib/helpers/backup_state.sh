#!/usr/bin/env bash
# Post-operation witnesses for guarded CLI rollback. Sourced by backup.sh.
# Trees are serialized in byte order with NUL-delimited names. Links are leaves:
# hash their link text, never stat/read/traverse their mutable destination.

_backup_hash_stream() (
    set -o pipefail
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256
    else
        _backup_error "SHA-256 is required for verified rollback"
        exit 1
    fi | {
        local digest rest
        read -r digest rest || exit 1
        [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || exit 1
        printf '%s\n' "$digest"
    }
)

_backup_state_mode() {
    local mode
    mode=$(stat -c '%a' "$1" 2>/dev/null) ||
        mode=$(stat -f '%Lp' "$1" 2>/dev/null) || return 1
    [[ "$mode" =~ ^[0-7]+$ ]] || return 1
    printf '%s\0' "$mode"
}

_backup_state_tree() {
    local path="$1" rel="$2" child
    if [[ -L "$path" ]]; then
        printf 'link\0%s\0' "$rel"
        readlink "$path" | _backup_hash_stream || return 1
    elif [[ -f "$path" ]]; then
        printf 'file\0%s\0' "$rel"
        _backup_state_mode "$path" || return 1
        _backup_hash_stream < "$path" || return 1
    elif [[ -d "$path" ]]; then
        [[ -r "$path" && -x "$path" ]] || {
            _backup_error "Cannot inspect directory for rollback: $rel"
            return 1
        }
        printf 'directory\0%s\0' "$rel"
        _backup_state_mode "$path" || return 1
        for child in "$path"/*; do
            _backup_state_tree "$child" "$rel/${child##*/}" || return 1
        done
    elif [[ -e "$path" ]]; then
        _backup_error "Unsupported file type for verified rollback: $rel"
        return 1
    else
        printf 'missing\0%s\0' "$rel"
    fi
}

_backup_tree_fingerprint() (
    set -o pipefail
    export LC_ALL=C
    shopt -s dotglob nullglob
    _backup_state_tree "$1" "$2" | _backup_hash_stream
)

_backup_target_fingerprint() {
    local root="$1" rel="$2" parent="$1" component
    local -a parts
    _backup_validate_rel "$rel" || return 1
    case "$rel" in
        */|*//*)
            _backup_error "Refusing non-normalized rollback target: $rel"
            return 1
            ;;
    esac
    IFS=/ read -ra parts <<< "$rel"
    local index
    for ((index = 0; index < ${#parts[@]} - 1; index++)); do
        component="${parts[$index]}"
        parent="$parent/$component"
        # Never traverse an ancestor link, even one currently pointing inside
        # the project. The linked data is not a rollback-owned subtree.
        if [[ -L "$parent" ]]; then
            _backup_error "Rollback target has a symlink ancestor: $rel"
            return 1
        fi
        if [[ -e "$parent" ]] && [[ ! -d "$parent" || ! -x "$parent" ]]; then
            _backup_error "Cannot inspect rollback target ancestor: $rel"
            return 1
        fi
    done
    _backup_tree_fingerprint "$root/$rel" "$rel"
}

# Seal only after the operation is complete. Never replace an existing witness.
# Bind it to the target list and pre-image as well as the live post-image.
backup_seal() {
    local root snapshot target_hash before_hash temp rel digest
    root=$(_backup_canonical_root "$1") || return 1
    snapshot=$(_backup_snapshot_path "$root" "$2") || return 1
    backup_load_targets "$root" "$snapshot" || return 1
    if [[ -e "$snapshot/after.tsv" || -L "$snapshot/after.tsv" ]]; then
        _backup_error "Snapshot already has a post-operation record"
        return 1
    fi
    target_hash=$(_backup_hash_stream < "$snapshot/targets.tsv") || return 1
    before_hash=$(_backup_tree_fingerprint "$snapshot/files" files) || return 1
    temp=$(mktemp "$snapshot/.after.tmp.XXXXXX") || return 1
    if ! (
        printf 'post-state-v1\t%s\t%s\n' "$target_hash" "$before_hash"
        for rel in "${BACKUP_LOADED_RELS[@]}"; do
            digest=$(_backup_target_fingerprint "$root" "$rel") || exit 1
            printf '%s\t%s\n' "$rel" "$digest"
        done
    ) > "$temp"; then
        rm -f "$temp"
        _backup_error "Could not record verified post-operation state"
        return 1
    fi
    mv "$temp" "$snapshot/after.tsv"
}

# Read-only, fail-closed preflight of ALL targets. No partial restore is started.
backup_preflight() (
    local root snapshot schema target_hash before_hash extra digest rel expected
    root=$(_backup_canonical_root "$1") || return 1
    snapshot=$(_backup_snapshot_path "$root" "$2") || return 1
    backup_load_targets "$root" "$snapshot" || return 1
    if [[ -L "$snapshot/after.tsv" || ! -f "$snapshot/after.tsv" ]]; then
        _backup_error "No verified post-operation state for this snapshot; rollback refused"
        return 1
    fi
    exec 3< "$snapshot/after.tsv" || return 1
    IFS=$'\t' read -r schema target_hash before_hash extra <&3 || return 1
    if [[ "$schema" != post-state-v1 || -n "$extra" ||
          ! "$target_hash" =~ ^[0-9a-f]{64}$ || ! "$before_hash" =~ ^[0-9a-f]{64}$ ]]; then
        _backup_error "Invalid post-operation record; rollback refused"
        return 1
    fi
    digest=$(_backup_hash_stream < "$snapshot/targets.tsv") || return 1
    if [[ "$digest" != "$target_hash" ]]; then
        _backup_error "Snapshot target list differs from its post-operation record"
        return 1
    fi
    digest=$(_backup_tree_fingerprint "$snapshot/files" files) || return 1
    if [[ "$digest" != "$before_hash" ]]; then
        _backup_error "Snapshot recovery data differs from its post-operation record"
        return 1
    fi
    local index
    for ((index = 0; index < ${#BACKUP_LOADED_RELS[@]}; index++)); do
        if ! IFS=$'\t' read -r rel expected extra <&3 ||
           [[ "$rel" != "${BACKUP_LOADED_RELS[$index]}" || -n "$extra" ||
              ! "$expected" =~ ^[0-9a-f]{64}$ ]]; then
            _backup_error "Incomplete or invalid post-operation target record"
            return 1
        fi
        digest=$(_backup_target_fingerprint "$root" "$rel") || {
            _backup_error "Rollback conflict: cannot safely inspect $rel"
            return 1
        }
        if [[ "$digest" != "$expected" ]]; then
            _backup_error "Rollback conflict: $rel changed after the operation; no targets were restored"
            return 1
        fi
    done
    if IFS= read -r extra <&3 || [[ -n "$extra" ]]; then
        _backup_error "Unexpected post-operation records; rollback refused"
        return 1
    fi
)
