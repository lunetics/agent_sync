#!/usr/bin/env bash
# agentsync init — scaffolds the .ai/ directory in a project.
#
# Model:
#   - Tool configs are NOT copied to .ai/src/tools/. They live in the install-dir
#     base and are referenced via tools.enabled list in agent_sync.yaml. Users
#     Per-tool files stay absent until customization so they cannot shadow
#     install-dir defaults.
#   - Per-tool payloads (hooks, mcp, settings) are NOT eagerly copied either.
#     They are scaffolded only for tools the user has opted into (auto-detected
#     from filesystem markers, passed via --tools, or explicitly enabled later).
#     Missing overrides fall back to base templates at sync time.
#   - Source content (AGENTS.md, rules, skills, commands, agents) is selectable
#     via --content (default: all sections). Pass --no-templates to create paths
#     without copying shipped starter files.

# Default set of starter content sections created by init. Users can narrow
# this with --content.
_INIT_CONTENT_DEFAULT="agents,rules,skills,commands,subagents"

# Valid content sections accepted by --content.
_INIT_CONTENT_VALID="agents rules skills commands subagents"

# Transaction state is activated only after validation, planning, and any
# interactive confirmation have completed.
INIT_BACKUP_PATH=""
INIT_TRANSACTION_ACTIVE="false"
INIT_CLEANUP_DONE="false"
INIT_BACKUP_TARGETS=()

# ── Private helpers ───────────────────────────────────────────────────────────

# Does $1 appear in whitespace-separated list $2? Returns 0/1.
_init_list_contains() {
    local needle="$1"
    local haystack=" $2 "
    [[ "$haystack" == *" $needle "* ]]
}

# Normalize a CSV string into space-separated unique tokens (preserves order).
_init_normalize_csv() {
    local csv="$1"
    local out="" token
    IFS=',' read -ra parts <<< "$csv"
    for token in "${parts[@]}"; do
        token="${token// /}"
        [[ -z "$token" ]] && continue
        _init_list_contains "$token" "$out" && continue
        out="${out:+$out }$token"
    done
    echo "$out"
}

# Merge two space-separated lists, deduping while preserving first-seen order.
_init_merge_lists() {
    local a="$1" b="$2" out="" token
    for token in $a $b; do
        [[ -z "$token" ]] && continue
        _init_list_contains "$token" "$out" && continue
        out="${out:+$out }$token"
    done
    echo "$out"
}

# Configure the shared path/tool resolver for the project being initialized.
_init_configure_resolver() {
    local target_dir="$1"
    local templates_dir="$2"

    REPO_ROOT="$target_dir"
    # Cross-module contract: paths.sh validates destinations against this root.
    # shellcheck disable=SC2034
    REPO_ROOT_CANONICAL="$(cd -P "$target_dir" && pwd)"
    # Cross-module contract: tool_resolver.sh resolves base templates from here.
    # shellcheck disable=SC2034
    if [[ -n "$templates_dir" ]]; then
        DEFAULT_REPO_ROOT="$(cd "$templates_dir/../.." && pwd)"
    else
        DEFAULT_REPO_ROOT="${_AGENTSYNC_ENGINE_ROOT:-$target_dir}"
    fi
}

# Print one repo-relative path per existing file under the destinations the
# selected tools claim — the project's own agent config from before AgentSync,
# which the first sync would otherwise replace. LC_ALL=C sorted, deduplicated.
_init_existing_dest_files() {
    local target_dir="$1"
    local tool_list="$2"

    local tool key raw abs file
    for tool in $tool_list; do
        for key in "${AGENTSYNC_TARGET_KEYS[@]}"; do
            if [[ "$(get_tool_bool "$tool" "targets.$key.enabled")" == "false" ]]; then
                continue
            fi
            get_tool_value_r "$tool" "targets.$key.dest"; raw="$REPLY"
            [[ -n "$raw" ]] || continue
            abs=$(resolve_dest_path "$raw" "targets.$key.dest for $tool" 2>/dev/null) || continue
            if [[ -f "$abs" ]]; then
                echo "${abs#"$target_dir"/}"
            elif [[ -d "$abs" ]]; then
                while IFS= read -r file; do
                    [[ -n "$file" ]] || continue
                    echo "${file#"$target_dir"/}"
                done < <(find "$abs" -type f 2>/dev/null)
            fi
        done
    done | LC_ALL=C sort -u
}

# Copy each existing destination file back into .ai/src/ so the first sync
# regenerates the project's own content instead of the shipped templates.
# Two destinations can map to one source (CLAUDE.md and AGENTS.md both become
# .ai/src/AGENTS.md); the first wins and the rest are reported as skipped.
_init_adopt_existing() {
    local target_dir="$1"
    local existing="$2"   # newline-separated repo-relative paths

    local claimed="|" file adopted=0 skipped=0
    local -a skips=()
    while IFS= read -r file; do
        [[ -n "$file" ]] || continue
        _adopt_resolve_dest "$target_dir/$file"
        if [[ -n "$_ADOPT_REFUSAL" ]]; then
            skips+=("$file — $_ADOPT_REFUSAL")
            skipped=$((skipped + 1))
            continue
        fi
        if [[ "$claimed" == *"|$_ADOPT_SOURCE_REL|"* ]]; then
            skips+=("$file — another file already became $_ADOPT_SOURCE_REL")
            skipped=$((skipped + 1))
            continue
        fi
        ensure_dir "$(dirname "$_ADOPT_SOURCE_ABS")"
        cp "$_ADOPT_DEST_ABS" "$_ADOPT_SOURCE_ABS"
        claimed+="$_ADOPT_SOURCE_REL|"
        echo "   $(_green "Adopted") $(_cyan "$file") → $(_dim "$_ADOPT_SOURCE_REL")"
        adopted=$((adopted + 1))
    done <<< "$existing"

    local note
    for note in "${skips[@]+"${skips[@]}"}"; do
        echo "   $(_yellow "Kept as-is") $note"
    done
    if [[ $skipped -gt 0 ]]; then
        echo "   $(_dim "Skipped files are regenerated from .ai/src/ — restore them with 'agentsync rollback' if needed.")"
    fi
    [[ $adopted -gt 0 || $skipped -gt 0 ]] && echo ""
    return 0
}

# Write .github/workflows/agentsync-check.yml from the shipped template, with
# the pinned version substituted. Never overwrites an existing file.
_init_write_ci_workflow() {
    local target_dir="$1"
    local templates_dir="$2"

    local template="$templates_dir/ci/github-agentsync-check.yml"
    [[ -f "$template" ]] || return 0

    local dest="$target_dir/.github/workflows/agentsync-check.yml"
    if [[ -f "$dest" ]]; then
        echo "   $(_yellow "Kept") $(_cyan ".github/workflows/agentsync-check.yml") $(_dim "(already exists)")"
        return 0
    fi

    mkdir -p "$(dirname "$dest")"
    local tmp
    tmp="$(tmp_sibling "$dest")"
    sed -e "s|__AGENTSYNC_VERSION__|${VERSION:-unknown}|g" \
        -e "s|__AGENTSYNC_INSTALL_URL__|https://raw.githubusercontent.com/$AGENTSYNC_REPO/main/install.sh|g" \
        "$template" > "$tmp" && mv "$tmp" "$dest"
    echo "   Created $(_cyan ".github/workflows/agentsync-check.yml") — CI gate (agentsync check)"
}

_init_collect_backup_targets() {
    local target_dir="$1"
    local templates_dir="$2"
    local tool_list="$3"

    _init_configure_resolver "$target_dir" "$templates_dir"

    INIT_BACKUP_TARGETS=(
        "$target_dir/.ai/src"
        "$target_dir/.ai/agent_sync.yaml"
        "$target_dir/.ai/.template-manifest"
    )

    local tool key raw abs
    for tool in $tool_list; do
        for key in "${AGENTSYNC_TARGET_KEYS[@]}"; do
            if [[ "$(get_tool_bool "$tool" "targets.$key.enabled")" == "false" ]]; then
                continue
            fi
            get_tool_value_r "$tool" "targets.$key.dest"; raw="$REPLY"
            [[ -n "$raw" ]] || continue
            abs=$(resolve_dest_path "$raw" "targets.$key.dest for $tool") || continue
            INIT_BACKUP_TARGETS+=("$abs")
        done
    done
}

_init_cleanup() {
    local status="$1"
    [[ "$INIT_CLEANUP_DONE" != "true" ]] || return 0
    INIT_CLEANUP_DONE="true"

    if [[ "$INIT_TRANSACTION_ACTIVE" == "true" ]] && [[ $status -ne 0 ]]; then
        echo "$(_yellow "Warning"): Init failed; restoring pre-init state..." >&2
        if backup_restore "$REPO_ROOT" "$INIT_BACKUP_PATH"; then
            backup_seal "$REPO_ROOT" "$INIT_BACKUP_PATH" || \
                echo "$(_yellow "Warning"): Could not record the restored state ($BACKUP_SEAL_REASON); rolling back backup $(basename "$INIT_BACKUP_PATH") cannot detect later changes." >&2
            echo "Restored pre-init state from ${INIT_BACKUP_PATH#"$REPO_ROOT"/}" >&2
            if ! backup_prune "$REPO_ROOT"; then
                echo "$(_yellow "Warning"): Could not prune old AgentSync backups." >&2
            fi
        else
            echo "$(_red "Error"): Automatic restore failed. Backup retained at ${INIT_BACKUP_PATH#"$REPO_ROOT"/}" >&2
        fi
    fi

    tmp_cleanup || true
}

_init_on_exit() {
    local status=$?
    trap - EXIT INT TERM HUP
    _init_cleanup "$status"
    exit "$status"
}

# $? inside a signal handler is the last completed command's status, not the
# signal, so an interrupted init would skip the restore above. Pass 128+N.
_init_on_signal() {
    trap - EXIT INT TERM HUP
    _init_cleanup "$((128 + $2))"
    kill -"$1" "$$"
    exit "$((128 + $2))"
}

_init_create_directories() {
    local ai_dir="$1"
    local content_list="$2"
    mkdir -p "$ai_dir/src"
    _init_list_contains "rules"     "$content_list" && mkdir -p "$ai_dir/src/rules"
    _init_list_contains "skills"    "$content_list" && mkdir -p "$ai_dir/src/skills"
    _init_list_contains "commands"  "$content_list" && mkdir -p "$ai_dir/src/commands"
    _init_list_contains "subagents" "$content_list" && mkdir -p "$ai_dir/src/agents"
    # tools/ intentionally NOT created — created on demand by `customize`.
    # tools/<tool>/ payload overrides are created on demand by `_init_copy_tool_payloads`
    # only for tools that are enabled.
    return 0
}

_init_copy_source_templates() {
    local ai_dir="$1"
    local templates_dir="$2"
    local content_list="$3"
    local no_templates="${4:-false}"

    if [[ "$no_templates" == "true" ]]; then
        if _init_list_contains "agents" "$content_list"; then
            : > "$ai_dir/src/AGENTS.md"
        fi
        return 0
    fi

    if [[ -n "$templates_dir" ]]; then
        if _init_list_contains "agents" "$content_list"; then
            [[ -f "$templates_dir/AGENTS.md" ]] && cp "$templates_dir/AGENTS.md" "$ai_dir/src/AGENTS.md"
        fi

        if _init_list_contains "rules" "$content_list"; then
            for rule_file in "$templates_dir/rules/"*.md; do
                [[ -f "$rule_file" ]] || continue
                cp "$rule_file" "$ai_dir/src/rules/"
            done
        fi

        if _init_list_contains "skills" "$content_list"; then
            for skill_dir in "$templates_dir/skills/"*/; do
                [[ -d "$skill_dir" ]] || continue
                local skill_name
                skill_name=$(basename "$skill_dir")
                mkdir -p "$ai_dir/src/skills/$skill_name"
                local skill_item
                for skill_item in "$skill_dir"*; do
                    [[ -e "$skill_item" ]] || continue
                    cp -R "$skill_item" "$ai_dir/src/skills/$skill_name/"
                done
            done
        fi

        if _init_list_contains "commands" "$content_list" && [[ -d "$templates_dir/commands" ]]; then
            for cmd_file in "$templates_dir/commands/"*.md; do
                [[ -f "$cmd_file" ]] || continue
                cp "$cmd_file" "$ai_dir/src/commands/"
            done
        fi

        if _init_list_contains "subagents" "$content_list" && [[ -d "$templates_dir/agents" ]]; then
            for agent_file in "$templates_dir/agents/"*.md; do
                [[ -f "$agent_file" ]] || continue
                cp "$agent_file" "$ai_dir/src/agents/"
            done
        fi
    else
        if _init_list_contains "agents" "$content_list"; then
            cat > "$ai_dir/src/AGENTS.md" << 'AGENTS_EOF'
# Project Agent

You are a senior software engineer working on this project. You write clean, correct, and maintainable code.

## Approach

1. **Understand** — Read existing code. Ask questions on ambiguities.
2. **Plan** — Break work into concrete steps.
3. **Implement** — Follow established project patterns. Handle errors explicitly.
4. **Verify** — Run tests, linter, and formatter.

## Principles

- Readability over cleverness. Explicit over implicit.
- Change what's needed, nothing more.
- Test what matters. No hardcoded secrets.
AGENTS_EOF
        fi

        if _init_list_contains "rules" "$content_list"; then
            cat > "$ai_dir/src/rules/core.md" << 'RULE_EOF'
# Core Rules

- Follow the project's established conventions and patterns.
- Prefer readability over cleverness.
- Handle errors explicitly. Don't swallow exceptions.
- Write tests for business logic and error paths.
- Never hardcode secrets, API keys, or credentials.
RULE_EOF
        fi
    fi
}

# Copy per-tool payload files (settings, hooks) ONLY for tools in the list.
# MCP is excluded because an empty per-tool file would shadow the shared
# .ai/src/mcp.json or base template. Use `agentsync add mcp <server>` to create
# a shared source, or `agentsync customize <tool> mcp` for a per-tool override.
#
# Creates destination directories lazily (only when a file is actually copied).
# Returns 0 always. Prints one "tools/<tool>/<resource>.<ext>" path per scaffold
# to stdout — the per-tool layout the resolver treats as canonical.
_init_copy_tool_payloads() {
    local ai_dir="$1"
    local templates_dir="$2"
    local tool_list="$3"   # space-separated tool names

    [[ -z "$templates_dir" ]] && return 0
    [[ -z "$tool_list" ]] && return 0

    local resource tool src_file dest_dir
    for resource in settings hooks; do
        [[ -d "$templates_dir/$resource" ]] || continue
        for tool in $tool_list; do
            for src_file in "$templates_dir/$resource/$tool".*; do
                [[ -f "$src_file" ]] || continue
                dest_dir="$ai_dir/src/tools/$tool"
                mkdir -p "$dest_dir"
                cp "$src_file" "$dest_dir/$resource.${src_file##*.}"
                echo "tools/$tool/$resource.${src_file##*.}"
            done
        done
    done
    return 0
}

# Detect which tools are already used in the current project by looking
# for well-known filesystem markers. Prints one tool name per line.
_init_detect_enabled_tools() {
    local root="$1"

    # Each entry: "tool_name|check1|check2|..."
    # Presence of ANY listed marker triggers detection.
    local -a detectors=(
        "claude|$root/.claude|$root/CLAUDE.md"
        "cursor|$root/.cursor|$root/.cursorrules"
        "copilot|$root/.github/copilot-instructions.md|$root/.github/instructions|$root/.github/prompts"
        "gemini|$root/.gemini|$root/GEMINI.md"
        "codex|$root/.codex"
        "kimi|$root/.kimi-code"
        "opencode|$root/.opencode|$root/opencode.json|$root/opencode.jsonc"
        "windsurf|$root/.windsurf|$root/.windsurfrules"
        "junie|$root/.junie"
        "cline|$root/.clinerules"
        "amazonq|$root/.amazonq"
        "zed|$root/.zed|$root/.rules"
        "antigravity|$root/.agents/rules|$root/.agents/workflows"
    )

    local entry tool marker IFS_BAK="$IFS"
    for entry in "${detectors[@]}"; do
        IFS='|' read -ra parts <<< "$entry"
        tool="${parts[0]}"
        local i=1
        while [[ $i -lt ${#parts[@]} ]]; do
            marker="${parts[$i]}"
            if [[ -e "$marker" ]]; then
                echo "$tool"
                break
            fi
            i=$((i + 1))
        done
    done
    IFS="$IFS_BAK"
}

_init_create_project_config() {
    local target_dir="$1"
    local enabled_list="$2"  # newline-separated tool names (may be empty)
    local outputs_mode="$3"  # committed | local
    local config_file="$target_dir/.ai/agent_sync.yaml"

    if [[ -f "$config_file" ]] || [[ -f "$target_dir/agent_sync.yaml" ]]; then
        return 0
    fi

    {
        cat << 'HEAD'
# AgentSync — Project Configuration
# All keys are optional — remove any that you leave at the default.

HEAD
        # Pin the CLI version that scaffolded this file. `doctor` warns on
        # mismatches so teams can catch drifting toolchains early.
        echo "agentsync_version: \"${VERSION:-unknown}\""
        # A fresh project has nothing to migrate, so it starts current.
        echo "format: $(engine_format "$(dirname "${BASH_SOURCE[0]}")/..")"
        cat << 'HEAD'

# Tools: which ones to sync for this project.
# Each name must match a base tool (see `agentsync list`) or a custom override
# file under .ai/src/tools/<name>.yaml.
tools:
HEAD
        if [[ -z "$enabled_list" ]]; then
            echo "  enabled: []"
        else
            echo "  enabled:"
            while IFS= read -r t; do
                [[ -z "$t" ]] && continue
                echo "    - $t"
            done <<< "$enabled_list"
        fi
        cat << 'TAIL'

# Source paths (override if you use a custom layout).
source:
  agents: ".ai/src/AGENTS.md"
  rules: ".ai/src/rules"
  skills: ".ai/src/skills"
  commands: ".ai/src/commands"
  subagents: ".ai/src/agents"
  tools: ".ai/src/tools"

# Global defaults applied to all tools.
defaults:
  enabled: false
  cleanup: true

# Post-sync hooks run arbitrary shell — enabling them requires an out-of-repo
# signal (AGENTSYNC_ALLOW_POST_SYNC=true or allow: true in the install-dir
# config.yaml), never this in-repo file. `skip: true` here always disables them.
post_sync:
  skip: false

# Where generated tool files live.
#   committed — outputs and .ai/.sync-manifest are committed; teammates get
#               them from `git pull` and CI runs `agentsync check`.
#   local     — outputs and the manifest are gitignored; every clone runs
#               `agentsync sync` (see `agentsync setup-hooks`).
TAIL
        echo "outputs: $outputs_mode"
        cat << 'TAIL'

# .gitignore management (false leaves the managed block untouched).
gitignore:
  update: true
TAIL
    } > "$config_file"
}

_init_print_summary() {
    local ai_dir="$1"
    local enabled_list="$2"       # space-separated (or empty)
    local payload_lines="$3"      # newline-separated "resource/file.ext" (or empty)
    local detect_source="$4"      # "detect" | "flag" | "mixed" | "none"
    local no_templates="${5:-false}"
    local outputs_mode="${6:-committed}"
    local synced="${7:-false}"

    echo ""
    if [[ "$outputs_mode" == "committed" ]]; then
        echo "   Created $(_cyan ".ai/agent_sync.yaml")     — project config (outputs: committed — teammates need only git pull)"
    else
        echo "   Created $(_cyan ".ai/agent_sync.yaml")     — project config (outputs: local — every clone runs agentsync sync)"
    fi

    if [[ -f "$ai_dir/src/AGENTS.md" ]]; then
        if [[ "$no_templates" == "true" ]] && [[ ! -s "$ai_dir/src/AGENTS.md" ]]; then
            echo "   Created $(_cyan ".ai/src/AGENTS.md")      — $(_dim "(empty)")"
        else
            echo "   Created $(_cyan ".ai/src/AGENTS.md")      — agent identity"
        fi
    fi

    local rule_count=0
    if [[ -d "$ai_dir/src/rules" ]]; then
        for f in "$ai_dir/src/rules/"*.md; do [[ -f "$f" ]] && rule_count=$((rule_count + 1)); done
        if [[ $rule_count -gt 0 ]]; then
            echo "   Created $(_cyan ".ai/src/rules/")          — $rule_count rule(s)"
        elif [[ "$no_templates" == "true" ]]; then
            echo "   Created $(_cyan ".ai/src/rules/")          — $(_dim "(empty)")"
        fi
    fi

    local skill_count=0
    if [[ -d "$ai_dir/src/skills" ]]; then
        for d in "$ai_dir/src/skills/"*/; do [[ -d "$d" ]] && skill_count=$((skill_count + 1)); done
        if [[ $skill_count -gt 0 ]]; then
            echo "   Created $(_cyan ".ai/src/skills/")         — $skill_count skill(s)"
        elif [[ "$no_templates" == "true" ]]; then
            echo "   Created $(_cyan ".ai/src/skills/")         — $(_dim "(empty)")"
        fi
    fi

    local cmd_count=0
    if [[ -d "$ai_dir/src/commands" ]]; then
        for f in "$ai_dir/src/commands/"*.md; do [[ -f "$f" ]] && cmd_count=$((cmd_count + 1)); done
        if [[ $cmd_count -gt 0 ]]; then
            echo "   Created $(_cyan ".ai/src/commands/")       — $cmd_count command(s)"
        elif [[ "$no_templates" == "true" ]]; then
            echo "   Created $(_cyan ".ai/src/commands/")       — $(_dim "(empty)")"
        fi
    fi

    local agent_count=0
    if [[ -d "$ai_dir/src/agents" ]]; then
        for f in "$ai_dir/src/agents/"*.md; do [[ -f "$f" ]] && agent_count=$((agent_count + 1)); done
        if [[ $agent_count -gt 0 ]]; then
            echo "   Created $(_cyan ".ai/src/agents/")         — $agent_count subagent(s)"
        elif [[ "$no_templates" == "true" ]]; then
            echo "   Created $(_cyan ".ai/src/agents/")         — $(_dim "(empty)")"
        fi
    fi

    if [[ -n "$payload_lines" ]]; then
        local line
        while IFS= read -r line; do
            [[ -z "$line" ]] && continue
            echo "   Created $(_cyan ".ai/src/$line")"
        done <<< "$payload_lines"
    fi

    echo ""
    if [[ -n "$enabled_list" ]]; then
        local count=0 joined=""
        local t
        for t in $enabled_list; do
            count=$((count + 1))
            joined="${joined:+$joined, }$t"
        done
        case "$detect_source" in
            detect)      echo "   $(_green "Auto-detected $count tool(s):") $joined" ;;
            flag)        echo "   $(_green "Enabled $count tool(s):") $joined $(_dim "(from --tools)")" ;;
            mixed)       echo "   $(_green "Enabled $count tool(s):") $joined $(_dim "(auto-detect + --tools)")" ;;
            interactive) echo "   $(_green "Enabled $count tool(s):") $joined $(_dim "(selected)")" ;;
            *)           echo "   $(_green "Enabled $count tool(s):") $joined" ;;
        esac
    else
        echo "   $(_dim "No tools enabled. Run 'agentsync enable <slug>' to opt in.")"
    fi

    echo ""
    echo "$(_green "Done!")"
    echo ""
    echo "Next steps:"
    local step=1
    if [[ -f "$ai_dir/src/AGENTS.md" ]]; then
        echo "  $step. Edit $(_cyan ".ai/src/AGENTS.md") — customize your agent's identity"
        step=$((step + 1))
    fi
    echo "  $step. Run $(_cyan "agentsync generate")    — print an AI prompt to tailor .ai/src/ to your codebase"
    step=$((step + 1))
    echo "  $step. Run $(_cyan "agentsync list")        — browse all available tools"
    step=$((step + 1))
    if [[ -z "$enabled_list" ]]; then
        echo "  $step. Run $(_cyan "agentsync enable <slug>") — opt in to tools you use"
    else
        echo "  $step. Run $(_cyan "agentsync enable <slug>") — add more tools"
    fi
    step=$((step + 1))
    if [[ "$synced" == "true" ]] && [[ -n "$enabled_list" ]]; then
        echo "  $step. Re-run $(_cyan "agentsync sync")     — after every change to .ai/src/"
    else
        echo "  $step. Run $(_cyan "agentsync sync")        — distribute to enabled tools"
    fi
    echo ""
    echo "Customize:"
    echo "  $(_dim "•") $(_cyan "agentsync add mcp <server>")            — configure shared MCP servers"
    echo "  $(_dim "•") $(_cyan "agentsync customize <tool> <resource>") — override settings/hooks per tool"
    echo ""
}

# Validate --content tokens against the allowed set. Prints error + exits on
# unknown token.
_init_validate_content() {
    local content_list="$1"
    local token
    for token in $content_list; do
        _init_list_contains "$token" "$_INIT_CONTENT_VALID" || {
            echo "$(_red "Error"): Unknown --content section: $token" >&2
            echo "Valid sections: $_INIT_CONTENT_VALID" >&2
            exit 1
        }
    done
}

# List all base tools available in the shipped templates. Prints one per line.
_init_list_available_tools() {
    local templates_dir="$1"
    [[ -z "$templates_dir" ]] && return 0
    local tools_dir="$templates_dir/tools"
    [[ -d "$tools_dir" ]] || return 0
    local f name
    for f in "$tools_dir"/*.yaml; do
        [[ -f "$f" ]] || continue
        name=$(basename "$f" .yaml)
        [[ "$name" == _* ]] && continue
        echo "$name"
    done | sort
}

# Render the plan (what init would create) — used by --dry-run and before
# the final TTY confirmation.
_init_print_plan() {
    local target_dir="$1"
    local tool_list="$2"
    local content_list="$3"
    local templates_dir="$4"
    local detect_source="$5"
    local no_templates="${6:-false}"

    echo "$(_bold "Plan:")"
    echo "  Target:   $(_cyan "$target_dir/.ai/")"
    if [[ -n "$content_list" ]]; then
        local content_joined="" tok
        for tok in $content_list; do content_joined="${content_joined:+$content_joined, }$tok"; done
        if [[ "$no_templates" == "true" ]]; then
            echo "  Content:  $content_joined $(_dim "(no starter templates)")"
        else
            echo "  Content:  $content_joined"
        fi
    else
        echo "  Content:  $(_dim "(none)")"
    fi
    if [[ -n "$tool_list" ]]; then
        local tools_joined="" tok
        for tok in $tool_list; do tools_joined="${tools_joined:+$tools_joined, }$tok"; done
        echo "  Tools:    $tools_joined $(_dim "($detect_source)")"
    else
        echo "  Tools:    $(_dim "(none — opt in later via 'agentsync enable')")"
    fi

    # Preview payload scaffolds. MCP is intentionally excluded — it resolves
    # via the shared .ai/src/mcp.json (or base) in 0.11+.
    if [[ -n "$tool_list" ]] && [[ -n "$templates_dir" ]]; then
        local resource tool src_file any_payload=0
        for resource in settings hooks; do
            [[ -d "$templates_dir/$resource" ]] || continue
            local per_resource=""
            for tool in $tool_list; do
                for src_file in "$templates_dir/$resource/$tool".*; do
                    [[ -f "$src_file" ]] || continue
                    per_resource="${per_resource:+$per_resource, }$(basename "$src_file")"
                    any_payload=1
                done
            done
            if [[ -n "$per_resource" ]]; then
                printf '  %-9s %s\n' "$resource:" "$per_resource"
            fi
        done
        [[ $any_payload -eq 0 ]] && echo "  $(_dim "No payloads to scaffold — tools will use base templates at sync time.")"
    fi
    echo ""
}

# ── agentsync upgrade-config ──────────────────────────────────────────────────

cmd_upgrade_config() {
    local project_dir
    project_dir="${AGENTSYNC_REPO_ROOT:-$(pwd)}"
    project_dir="$(cd "$project_dir" && pwd)"

    local config=""
    if [[ -f "$project_dir/.ai/agent_sync.yaml" ]]; then
        config="$project_dir/.ai/agent_sync.yaml"
    elif [[ -f "$project_dir/agent_sync.yaml" ]]; then
        config="$project_dir/agent_sync.yaml"
    else
        echo "$(_red "Error"): No agent_sync.yaml found in $project_dir" >&2
        echo "Run $(_cyan "agentsync init") first." >&2
        exit 1
    fi

    local current="${VERSION:-unknown}"
    local existing
    existing=$(grep -E '^agentsync_version:' "$config" | head -1 || true)

    if [[ -z "$existing" ]]; then
        # Insert at the top of file (after leading comments if any).
        local tmp
        tmp=$(tmp_sibling "$config")
        {
            # Emit leading comment block as-is, then the version line, then rest.
            awk -v ver="$current" '
                BEGIN { inserted=0 }
                /^[[:space:]]*(#|$)/ && !inserted { print; next }
                !inserted { printf "agentsync_version: \"%s\"\n\n", ver; inserted=1 }
                { print }
                END { if (!inserted) printf "agentsync_version: \"%s\"\n", ver }
            ' "$config"
        } > "$tmp"
        mv "$tmp" "$config"
        echo "$(_green "Added"): agentsync_version: $current → $(_dim "$config")"
    else
        # Replace the existing line.
        local escaped="${current//\//\\/}"
        # Use sed in-place with a backup suffix that we delete, for mac/linux parity.
        sed -i.agentsync_bak -E "s|^agentsync_version:.*|agentsync_version: \"$escaped\"|" "$config"
        rm -f "$config.agentsync_bak"
        echo "$(_green "Updated"): agentsync_version → $current $(_dim "$config")"
    fi
}

# ── Public command ────────────────────────────────────────────────────────────

cmd_init() {
    local target_dir=""
    local tools_flag=""
    local content_flag=""
    local no_detect=false
    local assume_yes=false
    local dry_run=false
    local tools_flag_set=false
    local content_flag_set=false
    local no_templates=false
    local no_templates_flag_set=false
    local outputs_mode="committed"
    local existing_action="adopt"
    local ci_provider=""
    local run_sync=true

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --tools)
                [[ $# -lt 2 ]] && { echo "$(_red "Error"): --tools requires a value" >&2; exit 1; }
                tools_flag="$2"
                tools_flag_set=true
                shift 2
                ;;
            --tools=*)
                tools_flag="${1#--tools=}"
                tools_flag_set=true
                shift
                ;;
            --content)
                [[ $# -lt 2 ]] && { echo "$(_red "Error"): --content requires a value" >&2; exit 1; }
                content_flag="$2"
                content_flag_set=true
                shift 2
                ;;
            --content=*)
                content_flag="${1#--content=}"
                content_flag_set=true
                shift
                ;;
            --no-detect)
                no_detect=true
                shift
                ;;
            --outputs)
                [[ $# -lt 2 ]] && { echo "$(_red "Error"): --outputs requires a value" >&2; exit 1; }
                outputs_mode="$2"
                shift 2
                ;;
            --outputs=*)
                outputs_mode="${1#--outputs=}"
                shift
                ;;
            --existing)
                [[ $# -lt 2 ]] && { echo "$(_red "Error"): --existing requires a value" >&2; exit 1; }
                existing_action="$2"
                shift 2
                ;;
            --existing=*)
                existing_action="${1#--existing=}"
                shift
                ;;
            --ci)
                [[ $# -lt 2 ]] && { echo "$(_red "Error"): --ci requires a value" >&2; exit 1; }
                ci_provider="$2"
                shift 2
                ;;
            --ci=*)
                ci_provider="${1#--ci=}"
                shift
                ;;
            --no-sync)
                run_sync=false
                shift
                ;;
            --no-templates)
                no_templates=true
                no_templates_flag_set=true
                shift
                ;;
            --yes|-y)
                assume_yes=true
                shift
                ;;
            --dry-run)
                dry_run=true
                shift
                ;;
            --help|-h)
                cat << 'HELP'
Usage: agentsync init [<dir>] [OPTIONS]

Scaffold .ai/ in a project. Minimal by default — only tools you opt in to
get per-tool payload scaffolding (settings/mcp/hooks).

Before writing, init snapshots its .ai/ paths and the selected tools' existing
destinations under .ai/backups/ so a partial setup can be restored safely.

In a terminal, `init` opens an interactive wizard that lets you pick tools
and content sections. In non-TTY environments (CI, scripts), it runs silently
with auto-detected defaults. Pass --yes or any of --tools/--content/--no-detect
/--no-templates to skip the wizard.

Options:
  --tools <csv>        Enable these tools (e.g. claude,cursor). Unions with
                       auto-detection unless --no-detect is passed.
  --content <csv>      Which source sections to scaffold. Valid tokens:
                       agents, rules, skills, commands, subagents.
                       Default: all of them.
  --no-detect          Skip filesystem marker auto-detection (tools only).
  --outputs <mode>     Where generated tool files live. `committed` (default)
                       keeps them and .ai/.sync-manifest in git so teammates
                       need only `git pull`; `local` gitignores both and every
                       clone runs `agentsync sync`.
  --existing <action>  What to do with tool config the project already has:
                       `adopt` (default) copies it into .ai/src/ so the first
                       sync reproduces it; `replace` regenerates from the
                       shipped templates.
  --ci <provider>      Write a CI gate that runs `agentsync check`. Only
                       `github` is supported; an existing workflow is kept.
  --no-sync            Skip the first `agentsync sync` at the end.
  --no-templates       Create selected content paths without copying shipped
                       starter files. AGENTS.md is empty when agents is selected.
  -y, --yes            Skip all prompts, accept defaults.
  --dry-run            Show what would be created; don't write anything.
  -h, --help           Show this help.

Examples:
  agentsync init                           # interactive wizard (TTY)
  agentsync init --yes                     # auto-detect + defaults, no prompt
  agentsync init --tools claude            # Claude only, no detection union
  agentsync init --tools claude,cursor --content agents,rules
  agentsync init --no-detect               # no tool auto-detection; pick tools later
  agentsync init --no-templates --no-detect  # empty .ai/src/ layout, no starters
  agentsync init --dry-run                 # preview without writing
HELP
                return 0
                ;;
            -*)
                echo "$(_red "Error"): Unknown flag: $1" >&2
                echo "Run $(_cyan "agentsync init --help") for usage." >&2
                exit 1
                ;;
            *)
                if [[ -z "$target_dir" ]]; then
                    target_dir="$1"
                    shift
                else
                    echo "$(_red "Error"): Unexpected argument: $1" >&2
                    exit 1
                fi
                ;;
        esac
    done

    case "$outputs_mode" in
        committed|local) ;;
        *)
            echo "$(_red "Error"): --outputs must be 'committed' or 'local' (got '$outputs_mode')" >&2
            exit 1
            ;;
    esac
    case "$existing_action" in
        adopt|replace) ;;
        *)
            echo "$(_red "Error"): --existing must be 'adopt' or 'replace' (got '$existing_action')" >&2
            exit 1
            ;;
    esac
    case "$ci_provider" in
        ""|github) ;;
        *)
            echo "$(_red "Error"): --ci only supports 'github' (got '$ci_provider')" >&2
            exit 1
            ;;
    esac

    local requested_dir="${target_dir:-.}"
    target_dir="$(cd "$requested_dir" 2>/dev/null && pwd)" || {
        echo "$(_red "Error"): Directory not found: $requested_dir" >&2
        exit 1
    }

    local _project_root_above_ai=""
    if _project_root_above_ai=$(ai_dir_enclosing_root "$target_dir"); then
        echo "$(_red "Error"): Cannot init inside the .ai/ directory: $target_dir" >&2
        echo "Run agentsync init from the project root (the parent of .ai/):" >&2
        echo "  cd \"$_project_root_above_ai\" && agentsync init" >&2
        exit 2
    fi

    local ai_dir="$target_dir/.ai"

    backup_configure "$target_dir" || return 1

    if [[ -d "$ai_dir/src" ]]; then
        echo "$(_yellow "Warning"): .ai/src/ already exists in $target_dir"
        echo "Skipping init to avoid overwriting your content."
        echo ""
        echo "Run $(_cyan "agentsync sync") to synchronize."
        return 0
    fi

    # Resolve content list.
    local content_list
    if [[ -n "$content_flag" ]]; then
        content_list=$(_init_normalize_csv "$content_flag")
    else
        content_list=$(_init_normalize_csv "$_INIT_CONTENT_DEFAULT")
    fi
    _init_validate_content "$content_list"

    # Resolve tool list: flag ∪ auto-detect (unless --no-detect).
    local tools_from_flag="" tools_from_detect="" tool_list detect_source="none"
    if [[ -n "$tools_flag" ]]; then
        tools_from_flag=$(_init_normalize_csv "$tools_flag")
    fi
    if [[ "$no_detect" != "true" ]]; then
        # _init_detect_enabled_tools prints one per line — flatten to spaces.
        tools_from_detect=$(_init_detect_enabled_tools "$target_dir" | tr '\n' ' ' | sed 's/  */ /g; s/^ //; s/ $//')
    fi
    tool_list=$(_init_merge_lists "$tools_from_flag" "$tools_from_detect")
    if [[ -n "$tools_from_flag" && -n "$tools_from_detect" ]]; then
        detect_source="mixed"
    elif [[ -n "$tools_from_flag" ]]; then
        detect_source="flag"
    elif [[ -n "$tools_from_detect" ]]; then
        detect_source="detect"
    fi

    # Templates directory — resolved early so the wizard can show available tools.
    local templates_dir=""
    local system_dir=""
    system_dir=$(resolve_system_dir 2>/dev/null) || true
    if [[ -n "$system_dir" ]] && [[ -d "$system_dir/templates" ]]; then
        templates_dir="$system_dir/templates"
    fi

    # Interactive wizard triggers when:
    #   - stdin+stdout are a TTY
    #   - no --yes, no --tools, no --content, no --no-templates (explicit automation)
    #   - --dry-run still opens the wizard; we just don't write at the end.
    local interactive=false
    if is_tty \
        && [[ "$assume_yes" != "true" ]] \
        && [[ "$tools_flag_set" != "true" ]] \
        && [[ "$content_flag_set" != "true" ]] \
        && [[ "$no_templates_flag_set" != "true" ]]; then
        interactive=true
    fi

    if [[ "$interactive" == "true" ]]; then
        echo ""
        echo "$(_bold "AgentSync init") — $(_dim "$target_dir")"
        echo ""

        local available_tools
        available_tools=$(_init_list_available_tools "$templates_dir" | tr '\n' ' ' | sed 's/ $//')
        if [[ -n "$available_tools" ]]; then
            local title
            if [[ -n "$tool_list" ]]; then
                title="Tools to enable $(_dim "(detected: $(echo "$tool_list" | tr ' ' ',')):")"
            else
                title="Tools to enable $(_dim "(none auto-detected):")"
            fi
            local picked
            picked=$(prompt_multiselect "$title" "$available_tools" "$tool_list") || {
                echo "$(_yellow "Cancelled.")" >&2
                return 130
            }
            tool_list="$picked"
            detect_source="interactive"
            echo ""
        fi

        local picked_content
        picked_content=$(prompt_multiselect \
            "Content sections:" \
            "$_INIT_CONTENT_VALID" \
            "$content_list") || {
            echo "$(_yellow "Cancelled.")" >&2
            return 130
        }
        content_list="$picked_content"
        echo ""

        echo "$(_dim "Generated files (CLAUDE.md, .claude/, .cursor/, …) can be committed, so")"
        echo "$(_dim "teammates get current rules from git pull and never run agentsync.")"
        if prompt_confirm "Commit generated files?" "y"; then
            outputs_mode="committed"
        else
            outputs_mode="local"
        fi
        echo ""
    fi

    # Existing tool config has to be found before the plan is rendered, so the
    # wizard can offer to keep it and the plan can say what happens to it.
    _init_configure_resolver "$target_dir" "$templates_dir"
    local existing_dests=""
    if [[ -n "$tool_list" ]]; then
        existing_dests=$(_init_existing_dest_files "$target_dir" "$tool_list")
    fi

    if [[ "$interactive" == "true" ]] && [[ -n "$existing_dests" ]]; then
        local existing_count
        existing_count=$(printf '%s\n' "$existing_dests" | grep -c . || true)
        echo "$(_yellow "Found $existing_count existing tool config file(s)") $(_dim "— the first sync regenerates these paths:")"
        local shown=0 line
        while IFS= read -r line; do
            [[ -n "$line" ]] || continue
            shown=$((shown + 1))
            if [[ $shown -le 10 ]]; then
                echo "   $line"
            fi
        done <<< "$existing_dests"
        [[ $existing_count -gt 10 ]] && echo "   $(_dim "… and $((existing_count - 10)) more")"
        if prompt_confirm "Copy them into .ai/src/ first, so sync reproduces them?" "y"; then
            existing_action="adopt"
        else
            existing_action="replace"
        fi
        echo ""
    fi

    if [[ "$interactive" == "true" ]] \
        && [[ -z "$ci_provider" ]] \
        && [[ "$outputs_mode" == "committed" ]] \
        && [[ -d "$target_dir/.github" ]]; then
        if prompt_confirm "Add a GitHub Actions gate that runs 'agentsync check'?" "y"; then
            ci_provider="github"
        fi
        echo ""
    fi

    # Render plan. For dry-run this is the only output; interactive asks to
    # confirm; explicit-flag non-interactive just proceeds.
    _init_print_plan "$target_dir" "$tool_list" "$content_list" "$templates_dir" "$detect_source" "$no_templates"

    if [[ "$dry_run" == "true" ]]; then
        echo "$(_dim "Dry run — nothing was written.")"
        return 0
    fi

    if [[ "$interactive" == "true" ]]; then
        prompt_confirm "Proceed?" "y" || {
            echo "$(_yellow "Cancelled.")"
            return 130
        }
        echo ""
    fi

    echo "$(_bold "Initializing AgentSync") in $(_cyan "$target_dir")"
    echo ""

    _init_collect_backup_targets "$target_dir" "$templates_dir" "$tool_list"
    INIT_BACKUP_PATH=$(backup_create \
        "$target_dir" \
        "init" \
        "${INIT_BACKUP_TARGETS[@]+"${INIT_BACKUP_TARGETS[@]}"}") || {
        echo "$(_red "Error"): Could not back up init targets; no project files were changed." >&2
        return 1
    }
    INIT_TRANSACTION_ACTIVE="true"
    trap _init_on_exit EXIT
    trap '_init_on_signal INT 2' INT
    trap '_init_on_signal TERM 15' TERM
    trap '_init_on_signal HUP 1' HUP

    _init_create_directories "$ai_dir" "$content_list"
    _init_copy_source_templates "$ai_dir" "$templates_dir" "$content_list" "$no_templates"

    local payload_lines
    payload_lines=$(_init_copy_tool_payloads "$ai_dir" "$templates_dir" "$tool_list")

    # Project config: one tool per line (newline-separated), as expected by writer.
    local enabled_newline
    enabled_newline=$(echo "$tool_list" | tr ' ' '\n' | sed '/^$/d')

    _init_create_project_config "$target_dir" "$enabled_newline" "$outputs_mode"

    # Baseline the template manifest so the next `agentsync refresh` can do
    # three-way diffs and only nag on real conflicts.
    if [[ -n "$templates_dir" ]] && declare -F template_manifest_heal_from_match >/dev/null 2>&1; then
        AGENTSYNC_REPO_ROOT="$target_dir" template_manifest_load
        AGENTSYNC_REPO_ROOT="$target_dir" template_manifest_heal_from_match "$templates_dir" "$ai_dir/src"
        AGENTSYNC_REPO_ROOT="$target_dir" template_manifest_write
    fi

    if [[ -n "$existing_dests" ]] && [[ "$existing_action" == "adopt" ]]; then
        echo ""
        AGENTSYNC_REPO_ROOT="$target_dir" _adopt_prepare_context
        AGENTSYNC_REPO_ROOT="$target_dir" _adopt_discover_sources
        _init_adopt_existing "$target_dir" "$existing_dests"
        _init_configure_resolver "$target_dir" "$templates_dir"
    fi

    if [[ "$ci_provider" == "github" ]]; then
        _init_write_ci_workflow "$target_dir" "$templates_dir"
    fi

    _init_print_summary "$ai_dir" "$tool_list" "$payload_lines" "$detect_source" "$no_templates" "$outputs_mode" "$run_sync"
    echo "Backup: ${INIT_BACKUP_PATH#"$target_dir"/}"
    echo ""

    if ! backup_prune "$target_dir"; then
        echo "$(_yellow "Warning"): Could not prune old AgentSync backups." >&2
    fi
    # The handler stays armed: INIT_TRANSACTION_ACTIVE gates the restore, and
    # leaving it in place keeps the run directory's cleanup owner defined.
    INIT_TRANSACTION_ACTIVE="false"
    if ! backup_seal "$target_dir" "$INIT_BACKUP_PATH"; then
        echo "$(_yellow "Warning"): Could not record the post-init state ($BACKUP_SEAL_REASON); rolling back backup $(basename "$INIT_BACKUP_PATH") cannot detect later changes." >&2
    fi

    # First sync, so `init` leaves a project whose outputs already exist —
    # committed mode has nothing to commit until they do. Its own transaction
    # covers the write, so a failure here leaves .ai/ in place.
    if [[ "$run_sync" == "true" ]] && [[ -n "$tool_list" ]] && [[ -n "$system_dir" ]]; then
        echo "$(_bold "Running the first sync")"
        echo ""
        AGENTSYNC_REPO_ROOT="$target_dir" bash "$system_dir/sync.sh" || {
            echo "$(_yellow "Warning"): first sync failed — fix the cause and run $(_cyan "agentsync sync")." >&2
            return 0
        }
        if [[ "$outputs_mode" == "committed" ]]; then
            echo "$(_bold "Commit .ai/ and the generated files") $(_dim "— teammates then need only git pull.")"
            echo ""
        fi
    fi
}
