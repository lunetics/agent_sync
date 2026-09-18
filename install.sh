#!/usr/bin/env bash
# AgentSync Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/yelmuratoff/agent_sync/main/install.sh | bash
#        AGENTSYNC_VERSION=0.36.0 curl -fsSL .../install.sh | bash   # pin a release tag
#
# What it does:
#   1. Downloads the agentsync binary for this platform from GitHub Releases
#      and verifies its sha256, into ~/.agentsync/bin/agentsync
#   2. Creates a symlink: /usr/local/bin/agentsync → ~/.agentsync/bin/agentsync
#
# A release tag older than the first binary release has no archive; pinning to
# one clones the repository into ~/.agentsync/ and links bin/agentsync.sh, as
# the installer did before the binary.
#
# AGENTSYNC_REPO_URL, AGENTSYNC_INSTALL_DIR, and AGENTSYNC_BIN_DIR override the
# defaults so the installer can run against local fixtures in tests.
#
# To uninstall:
#   rm -rf ~/.agentsync && rm -f /usr/local/bin/agentsync

set -euo pipefail

readonly REPO="yelmuratoff/agent_sync"
REPO_URL="${AGENTSYNC_REPO_URL:-https://github.com/$REPO.git}"
INSTALL_DIR="${AGENTSYNC_INSTALL_DIR:-$HOME/.agentsync}"
PIN_VERSION="${AGENTSYNC_VERSION:-}"
readonly REPO_URL INSTALL_DIR PIN_VERSION
readonly BIN_NAME="agentsync"

# ─── Colors ───────────────────────────────────────────────────────────────────
# Evaluate once at startup (not inside subshells where [[ -t 1 ]] would be false)
_USE_COLORS=false
[[ -t 1 ]] && [[ -z "${NO_COLOR:-}" ]] && _USE_COLORS=true

_bold()   { [[ "$_USE_COLORS" == true ]] && printf '\033[1m%s\033[0m' "$1" || printf '%s' "$1"; }
_green()  { [[ "$_USE_COLORS" == true ]] && printf '\033[32m%s\033[0m' "$1" || printf '%s' "$1"; }
_cyan()   { [[ "$_USE_COLORS" == true ]] && printf '\033[36m%s\033[0m' "$1" || printf '%s' "$1"; }
_yellow() { [[ "$_USE_COLORS" == true ]] && printf '\033[33m%s\033[0m' "$1" || printf '%s' "$1"; }
_red()    { [[ "$_USE_COLORS" == true ]] && printf '\033[31m%s\033[0m' "$1" || printf '%s' "$1"; }
_dim()    { [[ "$_USE_COLORS" == true ]] && printf '\033[2m%s\033[0m' "$1" || printf '%s' "$1"; }

# ─── Platform ─────────────────────────────────────────────────────────────────
# The cargo-dist target this host runs, as the release archive is named.
detect_target() {
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

_is_windows_target() { [[ "$1" == *-windows-* ]]; }

# sha256 of a file through whichever tool the host has; fails when neither.
file_sha256() {
    local file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        return 1
    fi
}

# `curl -sL -w %{http_code}`: the HTTP status on stdout, the body in <file>.
fetch() {
    local url="$1" out="$2"
    curl -sL --max-time 30 -o "$out" -w '%{http_code}' "$url" 2>/dev/null
}

# The tag of the latest GitHub release.
latest_release_tag() {
    local body code
    body=$(mktemp "${TMPDIR:-/tmp}/agentsync-latest.XXXXXX")
    code=$(fetch "https://api.github.com/repos/$REPO/releases/latest" "$body") || code="000"
    if [[ "$code" != "200" ]]; then
        rm -f "$body"
        return 1
    fi
    sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' "$body" | head -1
    rm -f "$body"
}

# ─── Binary install ───────────────────────────────────────────────────────────
# Downloads, verifies, and unpacks the release archive for <tag> and <target>
# into $INSTALL_DIR/bin and sets INSTALLED to the binary. On a failure to
# download it sets HTTP_CODE and returns 1: 404 means the tag has no binary
# release. Usage: install_binary <tag> <target>
INSTALLED=""
HTTP_CODE=""
install_binary() {
    local tag="$1" target="$2"
    local ext="tar.xz" exe="$BIN_NAME"
    if _is_windows_target "$target"; then
        ext="zip"; exe="$BIN_NAME.exe"
    fi
    local archive_name="agentsync-$target.$ext"
    local base="https://github.com/$REPO/releases/download/$tag"
    local work
    work=$(mktemp -d "${TMPDIR:-/tmp}/agentsync-install.XXXXXX")

    local code
    code=$(fetch "$base/$archive_name" "$work/$archive_name") || code="000"
    if [[ "$code" != "200" ]]; then
        rm -rf "$work"
        HTTP_CODE="$code"
        return 1
    fi
    code=$(fetch "$base/$archive_name.sha256" "$work/$archive_name.sha256") || code="000"
    if [[ "$code" != "200" ]]; then
        rm -rf "$work"
        HTTP_CODE="$code"
        return 1
    fi

    local expected actual
    expected=$(awk '{print $1; exit}' "$work/$archive_name.sha256")
    actual=$(file_sha256 "$work/$archive_name") || {
        rm -rf "$work"
        echo "$(_red "Error"): neither sha256sum nor shasum is available to verify the download." >&2
        exit 1
    }
    if [[ "$expected" != "$actual" ]]; then
        rm -rf "$work"
        echo "$(_red "Error"): checksum mismatch for $archive_name." >&2
        echo "  $(_dim "expected $expected, got $actual")" >&2
        exit 1
    fi

    mkdir -p "$work/unpacked"
    tar -xf "$work/$archive_name" -C "$work/unpacked" || {
        rm -rf "$work"
        echo "$(_red "Error"): could not unpack $archive_name." >&2
        exit 1
    }
    local unpacked=""
    local candidate
    for candidate in "$work/unpacked/$exe" "$work/unpacked"/*/"$exe"; do
        if [[ -f "$candidate" ]]; then
            unpacked="$candidate"
            break
        fi
    done
    if [[ -z "$unpacked" ]]; then
        rm -rf "$work"
        echo "$(_red "Error"): $archive_name does not contain $exe." >&2
        exit 1
    fi

    mkdir -p "$INSTALL_DIR/bin"
    cp "$unpacked" "$INSTALL_DIR/bin/$exe.new"
    chmod +x "$INSTALL_DIR/bin/$exe.new"
    mv -f "$INSTALL_DIR/bin/$exe.new" "$INSTALL_DIR/bin/$exe"
    rm -rf "$work"
    INSTALLED="$INSTALL_DIR/bin/$exe"
}

# ─── Source install (tags before the first binary release) ────────────────────
install_from_source() {
    if ! command -v git >/dev/null 2>&1; then
        echo "$(_red "Error"): git is required to install v$PIN_VERSION from source." >&2
        exit 1
    fi
    if [[ -d "$INSTALL_DIR/.git" ]]; then
        echo "  Fetching releases..."
        git -C "$INSTALL_DIR" fetch --quiet --force --tags origin 2>/dev/null || {
            echo "  $(_yellow "Warning"): git fetch failed, re-cloning..."
            rm -rf "$INSTALL_DIR"
            git clone --quiet "$REPO_URL" "$INSTALL_DIR"
        }
    else
        if [[ -d "$INSTALL_DIR" ]]; then
            echo "  Cleaning up previous installation..."
            rm -rf "$INSTALL_DIR"
        fi
        echo "  Cloning AgentSync..."
        git clone --quiet "$REPO_URL" "$INSTALL_DIR"
    fi
    echo "  Pinning to $(_cyan "v$PIN_VERSION")..."
    git -C "$INSTALL_DIR" checkout --quiet --detach "refs/tags/$PIN_VERSION" 2>/dev/null || {
        echo "$(_red "Error"): No AgentSync release is tagged $PIN_VERSION." >&2
        exit 1
    }

    local cli_script="$INSTALL_DIR/bin/agentsync.sh"
    if [[ ! -f "$cli_script" ]]; then
        echo "$(_red "Error"): CLI script not found at $cli_script" >&2
        exit 1
    fi
    chmod +x "$cli_script"
    INSTALLED="$cli_script"
}

# ─── Determine where to put the symlink ──────────────────────────────────────
resolve_bin_dir() {
    if [[ -n "${AGENTSYNC_BIN_DIR:-}" ]]; then
        mkdir -p "$AGENTSYNC_BIN_DIR"
        echo "$AGENTSYNC_BIN_DIR"
        return 0
    fi
    # Prefer /usr/local/bin if writable, otherwise ~/.local/bin
    if [[ -d "/usr/local/bin" ]] && [[ -w "/usr/local/bin" ]]; then
        echo "/usr/local/bin"
    elif [[ -d "$HOME/.local/bin" ]]; then
        echo "$HOME/.local/bin"
    else
        mkdir -p "$HOME/.local/bin"
        echo "$HOME/.local/bin"
    fi
}

# ─── Main ─────────────────────────────────────────────────────────────────────
main() {
    echo ""
    _bold "  AgentSync Installer"; echo ""
    echo ""

    if ! command -v curl >/dev/null 2>&1; then
        echo "$(_red "Error"): curl is required but not found." >&2
        exit 1
    fi

    local target=""
    target=$(detect_target) || {
        echo "$(_red "Error"): no release binary is built for $(uname -s 2>/dev/null) $(uname -m 2>/dev/null)." >&2
        exit 1
    }

    # 1. The release to install
    local tag="$PIN_VERSION"
    if [[ -z "$tag" ]]; then
        echo "  Checking the latest release..."
        tag=$(latest_release_tag) || {
            echo "$(_red "Error"): could not read the latest release from GitHub." >&2
            echo "  $(_dim "Check your network connection, or pin a release with AGENTSYNC_VERSION=<tag>.")" >&2
            exit 1
        }
    fi

    # 2. The binary, or the checkout for a tag without one
    echo "  Downloading agentsync $(_cyan "v$tag") for $target..."
    if ! install_binary "$tag" "$target"; then
        if [[ "$HTTP_CODE" == "404" ]] && [[ -n "$PIN_VERSION" ]]; then
            echo "  $(_dim "v$tag predates the binary releases; installing from source.")"
            install_from_source
        elif [[ "$HTTP_CODE" == "404" ]]; then
            echo "$(_red "Error"): release v$tag has no binary for $target." >&2
            exit 1
        else
            echo "$(_red "Error"): could not download release v$tag (HTTP $HTTP_CODE)." >&2
            echo "  $(_dim "Check your network connection and that GitHub is reachable.")" >&2
            exit 1
        fi
    fi

    # 3. Create symlink
    local bin_dir
    bin_dir=$(resolve_bin_dir)
    local symlink_path="$bin_dir/$BIN_NAME"

    # Remove old symlink if exists
    rm -f "$symlink_path" 2>/dev/null || true

    if ln -sf "$INSTALLED" "$symlink_path" 2>/dev/null; then
        echo "  Linked $(_cyan "$symlink_path") → $(_dim "$INSTALLED")"
    else
        # Try with sudo
        echo "  Need sudo to create symlink in $bin_dir..."
        sudo ln -sf "$INSTALLED" "$symlink_path"
        echo "  Linked $(_cyan "$symlink_path") → $(_dim "$INSTALLED")"
    fi

    # 4. Shell config: AGENTSYNC_HOME for a source install, PATH for ~/.local/bin
    local shell_config=""
    if [[ -f "$HOME/.zshrc" ]]; then
        shell_config="$HOME/.zshrc"
    elif [[ -f "$HOME/.bashrc" ]]; then
        shell_config="$HOME/.bashrc"
    elif [[ -f "$HOME/.bash_profile" ]]; then
        shell_config="$HOME/.bash_profile"
    fi

    local needs_path_update=false
    if [[ "$bin_dir" == "$HOME/.local/bin" ]]; then
        # Check if ~/.local/bin is in PATH
        if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
            needs_path_update=true
        fi
    fi

    if [[ -n "$shell_config" ]]; then
        if [[ "$INSTALLED" == *"/bin/agentsync.sh" ]] && ! grep -qF "AGENTSYNC_HOME" "$shell_config" 2>/dev/null; then
            {
                echo ""
                echo "# AgentSync"
                echo "export AGENTSYNC_HOME=\"$INSTALL_DIR\""
            } >> "$shell_config"
            echo "  Added AGENTSYNC_HOME to $(_dim "$shell_config")"
        fi

        if [[ "$needs_path_update" == "true" ]]; then
            if ! grep -q 'local/bin' "$shell_config" 2>/dev/null; then
                echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$shell_config"
                echo "  Added ~/.local/bin to PATH in $(_dim "$shell_config")"
            fi
        fi
    fi

    # 5. Done!
    echo ""
    echo "  $(_green "Installed successfully!") agentsync v$tag"
    echo ""
    echo "  Run $(_cyan "agentsync help") to get started."
    echo ""

    if [[ -n "$shell_config" ]] && { [[ "$needs_path_update" == "true" ]] || [[ "$INSTALLED" == *"/bin/agentsync.sh" ]]; }; then
        echo "  $(_yellow "Restart your terminal") or run:"
        echo "    source $shell_config"
        echo ""
    fi

    echo "  Quick start:"
    echo "    cd your-project"
    echo "    $(_cyan "agentsync init")"
    echo "    $(_cyan "agentsync sync")"
    echo ""

    echo "  To uninstall:"
    _dim "    rm -rf $INSTALL_DIR && rm -f $symlink_path"; echo ""
    echo ""
}

main "$@"
