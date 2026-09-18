#!/usr/bin/env bash
# agentsync update — self-update and update-check logic.

readonly AGENTSYNC_REPO="yelmuratoff/agent_sync"

# Reconcile a managed install dir to the already-fetched origin/main, healing
# past local drift. The install dir mirrors a release, not a working branch:
# local edits to tracked files are set aside into a stash (recoverable via
# `git stash list`) rather than allowed to abort the update, and a diverged
# history is hard-reset onto the release. Untracked files (.update_cache) and
# ignored ones (.snapshot/) are left untouched so the conflict snapshot survives.
# Echoes "stashed" when local changes were set aside; returns 1 only if even a
# hard reset fails. Usage: _update_reconcile_install "<install_dir>"
_update_reconcile_install() {
    local dir="$1"
    (
        cd "$dir" || exit 1
        local stashed=false
        if [[ -n "$(git status --porcelain --untracked-files=no 2>/dev/null)" ]]; then
            git stash push --quiet -m "agentsync-update-autostash" 2>/dev/null && stashed=true
        fi
        if git merge --ff-only --quiet origin/main 2>/dev/null \
            || git reset --hard --quiet origin/main 2>/dev/null; then
            [[ "$stashed" == "true" ]] && echo "stashed"
            exit 0
        fi
        exit 1
    )
}

# Pin the install to a release tag on a detached HEAD. Local edits are set aside
# exactly as _update_reconcile_install does; echoes "stashed" when that happened.
# Usage: _update_checkout_version "<install_dir>" "<version>"
_update_checkout_version() {
    local dir="$1"
    local version="$2"
    (
        cd "$dir" || exit 1
        local stashed=false
        if [[ -n "$(git status --porcelain --untracked-files=no 2>/dev/null)" ]]; then
            git stash push --quiet -m "agentsync-update-autostash" 2>/dev/null && stashed=true
        fi
        git checkout --quiet --detach "refs/tags/$version" 2>/dev/null || exit 1
        [[ "$stashed" == "true" ]] && echo "stashed"
        exit 0
    )
}

# The cargo-dist target this host runs, as the release archive is named.
_update_release_target() {
    local os arch
    os=$(uname -s 2>/dev/null || echo unknown)
    arch=$(uname -m 2>/dev/null || echo unknown)
    case "$os:$arch" in
        Darwin:arm64|Darwin:aarch64) echo "aarch64-apple-darwin" ;;
        Darwin:x86_64)               echo "x86_64-apple-darwin" ;;
        Linux:aarch64|Linux:arm64)   echo "aarch64-unknown-linux-musl" ;;
        Linux:x86_64)                echo "x86_64-unknown-linux-musl" ;;
        MINGW*:x86_64|MSYS*:x86_64|CYGWIN*:x86_64) echo "x86_64-pc-windows-msvc" ;;
        *) return 1 ;;
    esac
}

_update_file_sha256() {
    local file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        return 1
    fi
}

# After a source install reaches a release that ships a binary, download it,
# verify it, place it at <install_dir>/bin/agentsync[.exe], and point the
# agentsync link at it, so the next run is the binary and `agentsync update`
# replaces the binary from then on. A release without an archive is silent;
# any other failure is a warning and the checkout keeps running.
# Usage: _update_switch_to_binary "<install_dir>" "<version>"
_update_switch_to_binary() {
    local install_dir="$1" version="$2"
    if declare -F _native_bin >/dev/null 2>&1 && _native_bin >/dev/null 2>&1; then
        return 0
    fi
    command -v curl >/dev/null 2>&1 || return 0
    local target
    target=$(_update_release_target) || return 0
    local ext="tar.xz" exe="agentsync"
    if [[ "$target" == *-windows-* ]]; then
        ext="zip"; exe="agentsync.exe"
    fi
    local archive_name="agentsync-$target.$ext"
    local base="https://github.com/$AGENTSYNC_REPO/releases/download/$version"
    local work
    work=$(tmp_dir) || return 0

    local code
    code=$(curl -sL --max-time 30 -o "$work/$archive_name" -w '%{http_code}' "$base/$archive_name" 2>/dev/null) || code="000"
    [[ "$code" != "404" ]] || return 0
    if [[ "$code" != "200" ]]; then
        echo "  $(_yellow "Warning"): could not download the agentsync binary (HTTP $code); still running the Bash engine." >&2
        return 0
    fi
    code=$(curl -sL --max-time 30 -o "$work/$archive_name.sha256" -w '%{http_code}' "$base/$archive_name.sha256" 2>/dev/null) || code="000"
    if [[ "$code" != "200" ]]; then
        echo "  $(_yellow "Warning"): could not download the checksum of $archive_name (HTTP $code); still running the Bash engine." >&2
        return 0
    fi
    local expected actual
    expected=$(awk '{print $1; exit}' "$work/$archive_name.sha256")
    actual=$(_update_file_sha256 "$work/$archive_name") || {
        echo "  $(_yellow "Warning"): neither sha256sum nor shasum is available to verify $archive_name; still running the Bash engine." >&2
        return 0
    }
    if [[ "$expected" != "$actual" ]]; then
        echo "  $(_yellow "Warning"): checksum mismatch for $archive_name; still running the Bash engine." >&2
        return 0
    fi
    mkdir -p "$work/unpacked"
    if ! tar -xf "$work/$archive_name" -C "$work/unpacked" 2>/dev/null; then
        echo "  $(_yellow "Warning"): could not unpack $archive_name; still running the Bash engine." >&2
        return 0
    fi
    local unpacked="" candidate
    for candidate in "$work/unpacked/$exe" "$work/unpacked"/*/"$exe"; do
        if [[ -f "$candidate" ]]; then
            unpacked="$candidate"
            break
        fi
    done
    if [[ -z "$unpacked" ]]; then
        echo "  $(_yellow "Warning"): $archive_name does not contain $exe; still running the Bash engine." >&2
        return 0
    fi
    mkdir -p "$install_dir/bin"
    cp "$unpacked" "$install_dir/bin/$exe.new"
    chmod +x "$install_dir/bin/$exe.new"
    mv -f "$install_dir/bin/$exe.new" "$install_dir/bin/$exe"

    local current_bin
    current_bin=$(command -v agentsync 2>/dev/null) || true
    if [[ -n "$current_bin" ]] && [[ -L "$current_bin" ]]; then
        ln -sf "$install_dir/bin/$exe" "$current_bin" 2>/dev/null \
            || echo "  $(_yellow "Warning"): could not re-link $current_bin to the binary — run the installer to repair it." >&2
    fi
    echo "  $(_green "Switched to the agentsync binary") v$version $(_dim "at $install_dir/bin/$exe; the checkout stays beside it.")"
}

# `--force` on the tag fetch is load-bearing: the install mirrors upstream and
# never owns tags, so when a release tag is moved upstream a plain `--tags` fetch
# rejects it as "would clobber existing tag" and aborts the whole update. On
# failure, echoes git's own error to stdout so the caller surfaces the real cause.
# Usage: _update_fetch "<install_dir>"
_update_fetch() {
    local dir="$1"
    local out
    out=$(git -C "$dir" fetch --quiet --force --tags origin main 2>&1) && return 0
    printf '%s' "$out"
    return 1
}

cmd_update() {
    local strict=false
    local target_version=""
    local arg
    for arg in "$@"; do
        case "$arg" in
            --strict) strict=true ;;
            --help|-h)
                echo "Usage: agentsync update [<version>] [--strict]"
                echo ""
                echo "  <version>   Pin the install to that release tag (e.g. 0.35.0) instead of"
                echo "              the latest main — what a project's agentsync_version asks for."
                echo "  --strict    Exit non-zero if upstream changed a field you have overridden."
                return 0
                ;;
            -*)
                echo "$(_red "Error"): Unknown flag: $arg" >&2
                echo "Usage: agentsync update [<version>] [--strict]" >&2
                exit 2
                ;;
            *)
                if [[ -n "$target_version" ]]; then
                    echo "$(_red "Error"): Unexpected argument: $arg" >&2
                    echo "Usage: agentsync update [<version>] [--strict]" >&2
                    exit 2
                fi
                target_version="$arg"
                ;;
        esac
    done

    local install_dir
    install_dir=$(resolve_install_dir 2>/dev/null) || {
        echo "$(_red "Error"): AgentSync installation not found." >&2
        echo "Install globally first:" >&2
        echo "  curl -fsSL https://raw.githubusercontent.com/$AGENTSYNC_REPO/main/install.sh | bash" >&2
        exit 1
    }

    # Project dir for pending-resolutions — capture before we cd away.
    local project_dir="${AGENTSYNC_REPO_ROOT:-$(pwd)}"
    project_dir="$(cd "$project_dir" && pwd)"

    echo ""
    _bold "  AgentSync Update"; echo ""
    echo ""

    local old_version="$VERSION"

    echo "  Checking for updates..."
    cd "$install_dir" || exit 1

    local fetch_err
    if ! fetch_err=$(_update_fetch "$install_dir"); then
        echo "  $(_red "Error"): Failed to fetch updates from GitHub." >&2
        if [[ -n "$fetch_err" ]]; then
            while IFS= read -r line; do
                echo "    $line" >&2
            done <<< "$fetch_err"
        fi
        echo "  $(_dim "Check your network connection and that the remote is reachable.")" >&2
        exit 1
    fi

    local local_head remote_head
    local_head=$(git rev-parse HEAD)
    if [[ -n "$target_version" ]]; then
        remote_head=$(git rev-parse -q --verify "refs/tags/${target_version}^{commit}" 2>/dev/null) || {
            echo "  $(_red "Error"): No AgentSync release is tagged $target_version." >&2
            echo "  $(_dim "List releases with") $(_cyan "git -C \"$install_dir\" tag --sort=-v:refname")" >&2
            exit 1
        }
    else
        remote_head=$(git rev-parse origin/main)
    fi

    if [[ "$local_head" == "$remote_head" ]]; then
        echo "  $(_green "Already up to date!") (v${VERSION})"
        echo ""
        return 0
    fi

    # Snapshot the tool catalog BEFORE the swap. Conflict detection compares
    # this against the newly-pulled catalog. Clean up on any exit path.
    local snapshot_dir="$install_dir/.snapshot"
    rm -rf "$snapshot_dir"
    snapshot_save "$install_dir" "$snapshot_dir" || true
    _tmp_record_stray "$snapshot_dir"

    local reconcile_out
    if [[ -n "$target_version" ]]; then
        echo "  Pinning to v${target_version}..."
        reconcile_out=$(_update_checkout_version "$install_dir" "$target_version") || {
            echo "  $(_red "Error"): could not check out v${target_version} at $install_dir." >&2
            exit 1
        }
    else
        echo "  Updating..."
        if ! reconcile_out=$(_update_reconcile_install "$install_dir"); then
            echo "  $(_red "Error"): could not reconcile the install at $install_dir. Try reinstalling:" >&2
            echo "    curl -fsSL https://raw.githubusercontent.com/$AGENTSYNC_REPO/main/install.sh | bash" >&2
            exit 1
        fi
    fi

    # Re-link CLI binary (handles renames across versions)
    local cli_script="$install_dir/bin/agentsync.sh"
    if [[ -f "$cli_script" ]]; then
        chmod +x "$cli_script"
        local current_bin
        current_bin=$(command -v agentsync 2>/dev/null) || true
        if [[ -n "$current_bin" ]] && [[ -L "$current_bin" ]]; then
            ln -sf "$cli_script" "$current_bin" 2>/dev/null \
                || echo "  $(_yellow "Warning"): could not re-link $current_bin — run the installer to repair it." >&2
        fi
    fi

    local new_version="$old_version"
    if [[ -f "$install_dir/VERSION" ]]; then
        read -r new_version < "$install_dir/VERSION" || true
        [[ -n "$new_version" ]] || new_version="$old_version"
    fi

    # Clear the update cache — version is now current
    rm -f "$install_dir/.update_cache" 2>/dev/null || true

    echo ""
    if [[ "$old_version" != "$new_version" ]]; then
        echo "  $(_green "Updated!") v${old_version} → v${new_version}"
    else
        echo "  $(_green "Updated!") (v${new_version})"
    fi

    if [[ "$reconcile_out" == *stashed* ]]; then
        echo "  $(_dim "Set aside local edits in the install dir — recoverable via") $(_cyan "git -C \"$install_dir\" stash list")$(_dim ".")"
    fi

    _update_switch_to_binary "$install_dir" "$new_version"

    # Show what's new from CHANGELOG.md (all versions between old and new)
    _show_changelog_range "$install_dir" "$old_version" "$new_version"

    # Conflict detection: does upstream change any field the project overrides?
    local had_conflicts=false
    if [[ -d "$snapshot_dir/tools" ]]; then
        local conflicts
        conflicts=$(snapshot_diff "$snapshot_dir" "$install_dir" \
            | snapshot_find_conflicts "$project_dir")
        if [[ -n "$conflicts" ]]; then
            had_conflicts=true
            _show_update_conflicts "$conflicts"
            if [[ -d "$project_dir/.ai" ]]; then
                printf '%s\n' "$conflicts" \
                    | snapshot_write_pending_resolutions \
                        "$project_dir" "$old_version" "$new_version"
                echo "  $(_dim "Queued in") $(_cyan ".ai/.pending-resolutions.yaml")$(_dim " — run") $(_cyan "agentsync resolve")$(_dim " to walk them.")"
                echo ""
            fi
        fi
    fi

    echo ""

    _show_migration_banner "$project_dir"

    if [[ "$strict" == "true" ]] && [[ "$had_conflicts" == "true" ]]; then
        exit 1
    fi
}

# Banner for projects still on the pre-0.11 flat-layout overrides. Detected by
# existence of any file under .ai/src/{hooks,mcp,settings}/ in the project.
_show_migration_banner() {
    local project_dir="$1"
    [[ -d "$project_dir/.ai/src" ]] || return 0

    local found=false
    local resource file
    for resource in hooks mcp settings; do
        [[ -d "$project_dir/.ai/src/$resource" ]] || continue
        for file in "$project_dir/.ai/src/$resource"/*; do
            [[ -f "$file" ]] || continue
            found=true
            break 2
        done
    done

    [[ "$found" == "true" ]] || return 0

    echo ""
    echo "  $(_yellow "Legacy payload layout detected")"
    echo "  $(_dim "Your project has overrides under") $(_cyan ".ai/src/{hooks,mcp,settings}/")$(_dim ". The canonical")"
    echo "  $(_dim "layout since 0.11 is") $(_cyan ".ai/src/tools/<tool>/<resource>.<ext>")$(_dim ".")"
    echo "  $(_dim "Run") $(_cyan "agentsync migrate --apply") $(_dim "to move them. Legacy paths still read,")"
    echo "  $(_dim "but will be dropped in 0.12.")"
    echo ""
}

# Pretty-print conflicts grouped by tool. Input: TSV lines on stdin via arg-1.
# Line format: tool<TAB>field<TAB>base_before<TAB>base_after<TAB>your_override
_show_update_conflicts() {
    local conflicts="$1"
    echo ""
    echo "  $(_yellow "Upstream touched fields you have overridden:")"
    echo ""

    local current=""
    local line tool field base_before base_after your_override
    # Sort by tool so grouping works without extra state.
    while IFS=$'\t' read -r tool field base_before base_after your_override; do
        [[ -z "$tool" ]] && continue
        if [[ "$tool" != "$current" ]]; then
            [[ -n "$current" ]] && echo ""
            echo "    $(_bold "$tool")"
            current="$tool"
        fi
        echo "      $(_yellow "◆") $field"
        printf '          %s %s → %s\n' \
            "$(_dim "base:")" \
            "${base_before:-$(_dim "(unset)")}" \
            "${base_after:-$(_dim "(unset)")}"
        printf '          %s %s\n' \
            "$(_dim "your override:")" \
            "${your_override:-$(_dim "(unset)")}"
    done < <(printf '%s\n' "$conflicts" | LC_ALL=C sort)
    echo ""
}

# Show changelog entries for all versions between old_version (exclusive) and new_version (inclusive).
# Falls back to showing only new_version if old_version is unknown or equal.
# Usage: _show_changelog_range "/path/to/install" "0.2.0" "0.2.3"
_show_changelog_range() {
    local install_dir="$1"
    local old_version="$2"
    local new_version="$3"
    local changelog="$install_dir/CHANGELOG.md"

    [[ -f "$changelog" ]] || return 0

    # Collect all version headers from the changelog
    local all_versions=()
    while IFS= read -r line; do
        if [[ "$line" == "## "* ]]; then
            local v="${line#"## "}"
            v="${v%% *}"
            all_versions+=("$v")
        fi
    done < "$changelog"

    # Filter: old_version < v <= new_version
    local in_range=()
    for v in "${all_versions[@]}"; do
        local top_of_old_v
        top_of_old_v=$(printf '%s\n%s' "$old_version" "$v" | sort -V | tail -1)
        local top_of_v_new
        top_of_v_new=$(printf '%s\n%s' "$v" "$new_version" | sort -V | tail -1)
        if [[ "$top_of_old_v" == "$v" ]] && [[ "$v" != "$old_version" ]] \
            && [[ "$top_of_v_new" == "$new_version" ]]; then
            in_range+=("$v")
        fi
    done

    [[ ${#in_range[@]} -eq 0 ]] && return 0

    # Sort ascending so the user reads oldest → newest
    local sorted_versions
    sorted_versions=$(printf '%s\n' "${in_range[@]}" | sort -V)

    _show_changelog_sections "$changelog" "$sorted_versions"
}

# Strip the inline Markdown the changelog uses. A terminal shows `**bold**` and
# backticks literally, so the markers are noise here rather than emphasis.
_md_plain() {
    local text="$1"
    text="${text//\*\*/}"
    text="${text//\`/}"
    REPLY="$text"
}

# Usable width for changelog prose. tput is absent under some Git Bash setups
# and reports nonsense without a TERM, so the result is range-checked; the upper
# bound keeps lines readable on a maximised terminal.
_changelog_width() {
    local cols=""
    if command -v tput >/dev/null 2>&1; then
        cols=$(tput cols 2>/dev/null) || cols=""
    fi
    case "$cols" in
        ""|*[!0-9]*) cols=80 ;;
    esac
    [[ "$cols" -lt 40 ]] && cols=80
    [[ "$cols" -gt 100 ]] && cols=100
    REPLY="$cols"
}

# Print text wrapped to width, with a prefix on the first line and another on
# every continuation line. The first prefix may carry colour escapes, so only
# the continuation prefix is measured.
_print_wrapped() {
    local text="$1"
    local first_prefix="$2"
    local cont_prefix="$3"
    local width="$4"

    local avail=$(( width - ${#cont_prefix} ))
    [[ "$avail" -lt 24 ]] && avail=24

    local first=true line
    while IFS= read -r line; do
        # fold -s breaks at spaces and leaves the separator on the line.
        line="${line%"${line##*[![:space:]]}"}"
        if [[ "$first" == "true" ]]; then
            printf '%s%s\n' "$first_prefix" "$line"
            first=false
        else
            printf '%s%s\n' "$cont_prefix" "$line"
        fi
    done <<< "$(printf '%s\n' "$text" | fold -s -w "$avail")"
}

# Print the changelog body for each version in sorted_versions (newline-separated list).
_show_changelog_sections() {
    local changelog="$1"
    local sorted_versions="$2"

    local width
    _changelog_width
    width="$REPLY"

    while IFS= read -r version; do
        [[ -z "$version" ]] && continue
        echo ""
        echo "  $(_cyan "What's new in v${version}:")"
        echo ""

        local in_section=false
        local started=false
        while IFS= read -r line; do
            if [[ "$line" == "## $version"* ]]; then
                in_section=true
                continue
            fi
            if [[ "$in_section" == "true" ]]; then
                [[ "$line" == "## "* ]] && break
                [[ -z "$line" ]] && [[ "$started" == "false" ]] && continue
                started=true
                if [[ "$line" == "### "* ]]; then
                    _md_plain "${line#"### "}"
                    echo ""
                    echo "  $(_bold "$REPLY")"
                elif [[ "$line" == "- "* ]]; then
                    _md_plain "${line#"- "}"
                    _print_wrapped "$REPLY" "    $(_dim "•") " "      " "$width"
                elif [[ -n "$line" ]]; then
                    _md_plain "$line"
                    _print_wrapped "$REPLY" "    " "    " "$width"
                fi
            fi
        done < "$changelog"
    done <<< "$sorted_versions"
}

# Called as a fire-and-forget subshell — never blocks the main process.
_bg_fetch_latest_version() {
    local cache_file="$1"
    local latest_tag
    latest_tag=$(curl -sfL --max-time 5 \
        "https://api.github.com/repos/$AGENTSYNC_REPO/tags?per_page=1" \
        2>/dev/null | sed -n 's/.*"name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' | head -1) || return 0
    [[ -n "$latest_tag" ]] && printf '%s\n' "$latest_tag" > "$cache_file" 2>/dev/null || true
}

# Tell the user when their project is behind a migration this engine ships.
# Gated on a pending migration rather than rate-limited: the state is finite and
# actionable, and it disappears for good once `migrate --apply` records it.
_check_project_format() {
    local project_dir="${AGENTSYNC_REPO_ROOT:-$PWD}"
    local config
    config=$(format_config_path "$project_dir")
    [[ -n "$config" ]] || return 0

    local engine_rev current_rev
    engine_rev=$(engine_format "$_AGENTSYNC_LIB_DIR/..")
    current_rev=$(project_format "$config")
    [[ "$current_rev" -lt "$engine_rev" ]] || return 0

    echo ""
    echo "  $(_yellow "This project's agent config is a migration behind") $(_dim "(format r$current_rev → r$engine_rev)")"
    local note
    while IFS= read -r note; do
        [[ -n "$note" ]] || continue
        echo "    $(_dim "$note")"
    done < <(format_pending_notes "$current_rev" "$engine_rev")
    echo "  Preview it with $(_cyan "agentsync migrate"), apply with $(_cyan "agentsync migrate --apply")"
    echo ""
}

check_for_updates() {
    [[ -t 1 ]] || return 0
    [[ -z "${AGENTSYNC_NO_UPDATE_CHECK:-}" ]] || return 0

    _check_project_format

    local install_dir
    install_dir=$(resolve_install_dir 2>/dev/null) || return 0
    [[ -d "$install_dir/.git" ]] || return 0

    # A source install without a binary (a developer checkout with a build is
    # not one) is told once per run how to move to the binary.
    if [[ "$install_dir" == "${AGENTSYNC_HOME:-}" ]] && [[ ! -d "$install_dir/target" ]] \
        && declare -F _native_bin >/dev/null 2>&1 && ! _native_bin >/dev/null 2>&1; then
        echo "  $(_dim "This install runs the Bash engine;") $(_cyan "agentsync update") $(_dim "moves it to the agentsync binary.")"
    fi

    local cache_file="$install_dir/.update_cache"

    # Show banner from cache (written by previous background fetch)
    if [[ -f "$cache_file" ]]; then
        local latest_tag
        read -r latest_tag < "$cache_file" 2>/dev/null || latest_tag=""
        if [[ -n "$latest_tag" ]] \
            && [[ "$latest_tag" != "$VERSION" ]] \
            && [[ "$(printf '%s\n%s' "$VERSION" "$latest_tag" | sort -V | tail -1)" == "$latest_tag" ]]; then
            echo ""
            echo "  ╭──────────────────────────────────────────────────────╮"
            echo "  │  $(_yellow "Update available"): $(_dim "v${VERSION}") → $(_green "v${latest_tag}")              "
            echo "  │  Run: $(_cyan "agentsync update")                                "
            echo "  ╰──────────────────────────────────────────────────────╯"
            echo ""
        fi
    fi

    # Kick off background fetch for next run (fire-and-forget)
    ( _bg_fetch_latest_version "$cache_file" ) </dev/null >/dev/null 2>&1 &
    disown 2>/dev/null || true
}
