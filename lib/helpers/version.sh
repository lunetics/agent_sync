#!/usr/bin/env bash
# Engine version and the project's agentsync_version pin. Requires yaml.sh.

# Echo the engine version from the VERSION file beside <lib_dir>; "0.0.0-dev" when absent.
engine_version() {
    local lib_dir="$1"
    local file="$lib_dir/../VERSION" v=""
    if [[ -f "$file" ]]; then
        read -r v < "$file" || true
    fi
    echo "${v:-0.0.0-dev}"
}

# Echo the agentsync_version pinned in <config>, or nothing when unpinned.
pinned_version() {
    local config="$1"
    [[ -f "$config" ]] || return 0
    local v
    v=$(parse_yaml_value "$config" "agentsync_version")
    echo "${v//\"/}"
}

# Echo the configured mismatch policy. The nested form is canonical; the
# scalar form is accepted as a compact compatibility shorthand.
# Defaults to warn, preserving the historical local-output behaviour. An unknown
# value is echoed as well and returns 1, so the caller reports it in its own voice.
version_pin_mode() {
    local config="$1"
    local mode shorthand
    mode=$(parse_yaml_value "$config" "version_pin.mode")
    mode="${mode//\"/}"

    if [[ -z "$mode" ]]; then
        shorthand=$(parse_yaml_value "$config" "version_pin")
        shorthand="${shorthand//\"/}"
        [[ -n "$shorthand" ]] && mode="$shorthand"
    fi

    case "$mode" in
        "") echo "warn" ;;
        warn|strict) echo "$mode" ;;
        *)
            echo "$mode"
            return 1
            ;;
    esac
}

# Print the actionable reason for a version-pin mismatch.
version_pin_mismatch_error() {
    local pinned="$1" engine="$2" outputs="$3"
    if [[ "$outputs" == "committed" ]]; then
        echo "This project pins agentsync $pinned but you are running $engine — committed outputs must come from one version everywhere."
    else
        echo "This project pins agentsync $pinned but you are running $engine — version_pin.mode 'strict' requires local outputs to use the pinned version."
    fi
}

# Print the two ways out of a pin/engine mismatch.
version_pin_mismatch_hint() {
    local pinned="$1" engine="$2"
    echo "  • Match the pin:  agentsync update $pinned"
    echo "  • Or move it:     agentsync upgrade-config   (re-pins to $engine; re-sync and commit the outputs)"
}
