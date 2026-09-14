#!/usr/bin/env bash
# Location of the project's agent_sync.yaml, shared by sync, check, init, and rollback.

# Set REPLY to the project config for <root>: AGENTSYNC_CONFIG_PATH (relative to
# <root> unless absolute), else .ai/agent_sync.yaml, else agent_sync.yaml, else
# empty. Returns 1 when AGENTSYNC_CONFIG_PATH names a missing file, with that
# path in REPLY; an explicit path never falls back to another config.
project_config_path_r() {
    local root="$1"
    local explicit="${AGENTSYNC_CONFIG_PATH:-}"
    REPLY=""
    if [[ -n "$explicit" ]]; then
        [[ "$explicit" == /* ]] || explicit="$root/$explicit"
        REPLY="$explicit"
        [[ -f "$explicit" ]]
        return
    fi
    if [[ -f "$root/.ai/agent_sync.yaml" ]]; then
        REPLY="$root/.ai/agent_sync.yaml"
    elif [[ -f "$root/agent_sync.yaml" ]]; then
        REPLY="$root/agent_sync.yaml"
    fi
    return 0
}
