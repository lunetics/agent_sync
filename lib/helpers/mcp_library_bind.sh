#!/usr/bin/env bash
# Materialize an explicit library selection as an ordinary per-tool MCP source.
# Deliberately no live bindings, dependency installs, native writes or overwrites.

_mcp_library_render_r() {
    local catalog="$1" default_variant="$2"
    shift 2
    local selection id variant manifest server info seen
    local body="" separator=""
    local -a ids=()
    for selection in "$@"; do
        id="${selection%%@*}"
        variant="$default_variant"
        [[ "$selection" != *@* ]] || variant="${selection#*@}"
        _mcp_library_valid_id "$id" && _mcp_library_valid_id "$variant" || {
            _mcp_library_error "Expected a safe id or id@variant: $selection"
            return 1
        }
        for seen in "${ids[@]+"${ids[@]}"}"; do
            [[ "$seen" != "$id" ]] || {
                _mcp_library_error "Duplicate selected MCP id: $id"
                return 1
            }
        done
        ids+=("$id")
        _mcp_library_manifest_r "$catalog" "$id" || return 1
        manifest="$REPLY"
        server=$(_mcp_library_parser "$manifest" "$id" server "$variant") || return 1
        info=$(_mcp_library_parser "$manifest" "$id" selection "$variant") || return 1
        # The parser escapes all display controls; metadata never enters JSON.
        printf 'Selected %s: %s\n' "$id" "$info" >&2
        body="$body$separator\"$id\":$server"
        separator=","
    done
    REPLY="{\"mcpServers\":{$body}}"
}

# Refuse symlink components even if they currently resolve inside the project.
# Canonicalizing the project root once permits a user-selected root symlink.
_mcp_library_write_path_ok() {
    local target="$1" root="$2" part current="$2" rest
    _mcp_library_path_within "$target" "$root" || return 1
    [[ "$target" != "$root" ]] || return 1
    rest="${target#"$root/"}"
    while [[ -n "$rest" ]]; do
        part="${rest%%/*}"
        [[ "$part" != . && "$part" != .. && -n "$part" ]] || return 1
        current="$current/$part"
        [[ ! -L "$current" ]] || return 1
        if [[ "$rest" == */* ]]; then
            [[ ! -e "$current" || -d "$current" ]] || return 1
            rest="${rest#*/}"
        else
            rest=""
        fi
    done
}

_mcp_library_source_target_r() {
    local tool="$1" payload="$2"
    local target dir path declared user_file
    tool_resolver_init_user_dir
    normalize_absolute_path_r "$TOOL_RESOLVER_USER_DIR/$tool/mcp.json"
    target="$REPLY"
    _mcp_library_write_path_ok "$target" "$REPO_ROOT" || {
        _mcp_library_error "Refusing MCP source outside the project or through a symlink: $target"
        return 1
    }
    # A project declaration is intent, even before its file exists. Distinguish
    # it from the built-in legacy default so normal new projects still work.
    user_file=$(tool_resolver_user_file "$tool")
    parse_yaml_value_r "$user_file" targets.mcp.source
    declared="$REPLY"
    if [[ -n "$declared" ]]; then
        [[ "$declared" == /* ]] || declared="$REPO_ROOT/$declared"
        normalize_absolute_path_r "$declared"
        [[ "$REPLY" == "$target" ]] || {
            _mcp_library_error "Declared MCP source would be shadowed: $declared"
            return 1
        }
    fi
    if [[ -e "$target" ]]; then
        if [[ -f "$target" ]] && cmp -s "$target" <(printf '%s\n' "$payload"); then
            REPLY="$target"
            return 0
        fi
        _mcp_library_error "Existing MCP source differs; use render and merge explicitly: $target"
        return 1
    fi
    # Creating a higher-priority source must not silently shadow user entries.
    dir="${target%/*}"
    for path in "$dir"/mcp.* "$REPO_ROOT/.ai/src/mcp/$tool".* "$REPO_ROOT/.ai/src/mcp.json"; do
        if [[ -e "$path" || -L "$path" ]]; then
            _mcp_library_error "Existing MCP source would be shadowed; use render and merge explicitly: $path"
            return 1
        fi
    done
    get_tool_value_r "$tool" targets.mcp.source
    declared="$REPLY"
    if [[ -n "$declared" ]]; then
        [[ "$declared" == /* ]] || declared="$REPO_ROOT/$declared"
        if [[ -e "$declared" || -L "$declared" ]]; then
            _mcp_library_error "Declared MCP source would be shadowed: $declared"
            return 1
        fi
    fi
    REPLY="$target"
}

cmd_mcp_library_bind() {
    local action="$1" library="" tool="" default_variant="default" apply=false dry_run=false
    shift
    local -a selections=()
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --library|--tool|--variant)
                [[ $# -ge 2 && -n "$2" && "$2" != -* ]] || {
                    _mcp_library_error "$1 requires a non-empty value"
                    return 1
                }
                case "$1" in
                    --library) library="$2" ;;
                    --tool) tool="$2" ;;
                    --variant) default_variant="$2" ;;
                esac
                shift 2 ;;
            --apply) apply=true; shift ;;
            --dry-run) dry_run=true; shift ;;
            --help|-h) _mcp_library_usage; return 0 ;;
            -*) _mcp_library_error "Unknown MCP option: $1"; return 1 ;;
            *) selections+=("$1"); shift ;;
        esac
    done
    (( ${#selections[@]} > 0 && ${#selections[@]} <= MCP_LIBRARY_MAX_ENTRIES )) || {
        _mcp_library_error "Select between 1 and $MCP_LIBRARY_MAX_ENTRIES MCP entries"
        return 1
    }
    [[ "$apply" != true || "$dry_run" != true ]] || {
        _mcp_library_error "--apply and --dry-run are mutually exclusive"; return 1
    }
    _mcp_library_valid_id "$default_variant" || { _mcp_library_error "Unsafe variant"; return 1; }
    if [[ "$action" == render ]]; then
        [[ -z "$tool" && "$apply" == false ]] || {
            _mcp_library_error "render produces a source fragment; use use --tool for materialization"
            return 1
        }
    else
        case "$tool" in
            claude|opencode) ;;
            *) _mcp_library_error "use requires --tool claude or opencode; other adapters are not supported yet"; return 1 ;;
        esac
    fi
    _mcp_library_select_root_r "$library" || return 1
    local catalog="$REPLY" payload
    _mcp_library_render_r "$catalog" "$default_variant" "${selections[@]}" || return 1
    payload="$REPLY"
    if [[ "$action" == render ]]; then
        printf '%s\n' "$payload"
        return 0
    fi

    # Dynamic locals supply the same resolver context as sync without touching
    # any native config. Explicit missing config never falls back.
    local REPO_ROOT DEFAULT_REPO_ROOT PROJECT_CONFIG_PATH TOOL_RESOLVER_USER_DIR
    REPO_ROOT=$(cd -P "${AGENTSYNC_REPO_ROOT:-.}" && pwd) || return 1
    # shellcheck disable=SC2034 # consumed by the shared tool resolver
    DEFAULT_REPO_ROOT="$_AGENTSYNC_ENGINE_ROOT"
    project_config_path_r "$REPO_ROOT" || {
        _mcp_library_error "AGENTSYNC_CONFIG_PATH is set but file not found: $REPLY"; return 1
    }
    PROJECT_CONFIG_PATH="$REPLY"
    [[ -n "$PROJECT_CONFIG_PATH" ]] || { _mcp_library_error "Initialize an AgentSync project before use"; return 1; }
    local enabled=false candidate
    tool_resolver_init_user_dir
    while IFS= read -r candidate; do
        [[ "$candidate" != "$tool" ]] || enabled=true
    done < <(list_enabled_tools)
    [[ "$enabled" == true ]] || { _mcp_library_error "Enable $tool explicitly before use"; return 1; }
    _mcp_library_source_target_r "$tool" "$payload" || return 1
    local target="$REPLY" staging
    if [[ "$apply" != true ]]; then
        printf 'Preview: create %s; native config is unchanged. Add --apply, then run sync --only %s.\n' "$target" "$tool" >&2
        printf '%s\n' "$payload"
        return 0
    fi
    if [[ -f "$target" ]]; then
        printf 'MCP source already matches: %s\n' "$target"
        return 0
    fi
    mkdir -p "${target%/*}" || return 1
    _mcp_library_write_path_ok "$target" "$REPO_ROOT" || return 1
    staging=$(mktemp "${target%/*}/.mcp-library.XXXXXX") || return 1
    if ! printf '%s\n' "$payload" > "$staging"; then
        rm -f "$staging"
        return 1
    fi
    # Publish a complete file without replacing a concurrently created file.
    if [[ -e "$target" || -L "$target" ]] || ! ln "$staging" "$target"; then
        rm -f "$staging"
        _mcp_library_error "MCP source appeared concurrently; nothing overwritten: $target"
        return 1
    fi
    rm -f "$staging"
    printf 'Created MCP source: %s\nRun agentsync sync --only %s to update native config.\n' "$target" "$tool"
}
