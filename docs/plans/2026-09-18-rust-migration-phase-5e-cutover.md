# Rust Migration Phase 5e: The Cutover

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Make the binary the entry point for every install: `install.sh` downloads the release archive for the platform from GitHub Releases, verifies its sha256, places `~/.agentsync/bin/agentsync`, and links it; a source install moves to the binary the next time `agentsync update` reaches a release that ships one; the dispatcher accepts `bin/agentsync[.exe]` as its binary; the Windows CI job runs the bats suite against the binary unsharded; and the README and `.ai/src/AGENTS.md` describe a single static binary. This is the last of the five Phase 5 slices. Closing it closes Phase 5: the next release is the first binary release and the first point at which the migration branch is merged.

**Architecture:** `install.sh` stays hand-written (design decision 3): `detect_target` maps `uname -s`/`uname -m` to the five cargo-dist targets, `install_binary` fetches `agentsync-<target>.tar.xz` (`.zip` on Windows) and its `.sha256` through `curl -sL -w %{http_code}`, verifies with `sha256sum` or `shasum`, unpacks through `tar -xf`, and stages the binary beside its destination before `mv`; a pinned tag whose archive answers 404 falls back to `install_from_source`, the git clone the installer always made, which alone still writes `AGENTSYNC_HOME` to the shell config. `lib/helpers/update.sh` gains `_update_switch_to_binary`, called by `cmd_update` after the git reconcile: when no binary is present (`_native_bin` finds none), it downloads and verifies the archive of the new version the same way, places `<install>/bin/agentsync[.exe]`, and re-points the `agentsync` link at it; a 404 is silent, any other failure a warning. `check_for_updates` prints one dim line pointing at `agentsync update` for a source install without a binary. `bin/agentsync.sh` `_native_bin` adds `<engine>/bin/agentsync[.exe]` between the developer build and the older `agentsync-native` name. The seam is `tests/install.bats`, rewritten around a `curl` stand-in on `PATH` (the same URL shapes and fixture layout as `tests/update_native.bats`) and a fixture binary release, and running `update` through the installed engine's own dispatcher as the link does. Nothing in `src/` changes.

**Tech stack:** Bash 3.2 and coreutils for the installer and the update module; `curl`, `tar`, `sha256sum` or `shasum` at install time. Rust unchanged: 2024 edition (MSRV 1.85), no new crate. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 5" and its decisions 2 and 3. Previous plan: `docs/plans/2026-09-18-rust-migration-phase-5d-update.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. The installer writes `$INSTALL_DIR/bin/agentsync[.exe]` (or the clone), the `agentsync` link, and the shell config; the switch writes `<install>/bin/agentsync[.exe]` and the link. Nothing else.
- No Rust change; `cargo test`, fmt, and clippy are the Task 0 baseline and are not rerun. The binary is unchanged from 5d.
- Bash changes: `install.sh` rewritten, `lib/helpers/update.sh` gains three helpers and one call plus the clone notice, `bin/agentsync.sh` gains two candidate lines. `_NATIVE_COMMANDS` does not change. ShellCheck clean over every shell entry point.
- Every existing bats file stays green under both engines; `tests/install.bats` grows from 6 to 13 cases and never reaches the network: the `curl` stand-in answers 404 for anything it has no fixture for.
- The one-line clone notice prints only on a terminal, only for a source install (`AGENTSYNC_HOME` is the install, it has no `target/`, and `_native_bin` finds nothing), never for a developer checkout with a build; checked on a pty by `clone_notice_tty.sh`.
- CI: the native job's Windows leg installs bats the way the shard job does and runs the whole suite serially against the binary with `AGENTSYNC_NATIVE=1`; the Bash shard matrix stays until Phase 6. The workflow parses (`ruby -ryaml`).
- Every expected value was captured on 2026-09-18 from the draft on macOS 26.5 arm64: the sha256 of every file the plan writes, the bats counts, the pty harness lines, and `TOTAL bash=0 native=0` over 51 bats files.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

1. **`install.sh` stays hand-written and does the download itself.** The cargo-dist installers ship on every release for those who want them (the README names the PowerShell one), but `curl | bash` of `install.sh` has to keep working for pins older than the first binary release, which the generated installer cannot serve; so the script downloads and verifies the binary and falls back to the clone. Alternative: replace `install.sh` with the generated shell installer; rejected until Phase 6 drops the source path.
2. **A source install switches inside Bash's `update`.** Design decision 2: after the git reconcile the cutover release's Bash downloads the binary and re-links, so a user who runs `agentsync update` once is on the binary from then on, and the checkout stays beside it for the parity suite. A 404 (a version without an archive, such as `main` between releases) is silent; other failures warn and keep the checkout running. Alternative: tell the user to rerun the installer; rejected, one command was the promise.
3. **The binary's name in an install is `bin/agentsync[.exe]`**, what the installers place and what the switch writes; `_native_bin` keeps `agentsync-native` as a second candidate for the name the spec once used. Resolved the question 5c left open.
4. **The clone notice is gated on `AGENTSYNC_HOME` being the install and having no `target/`.** A developer checkout without a build would otherwise see it on every command; a checkout with a build already runs the binary. Alternative: no notice; rejected, design decision 2 asks for it.
5. **Windows runs the suite serially against the binary with a 90-minute step timeout**, unsharded as the spec asks. Whether Git Bash hands POSIX paths in `AGENTSYNC_REPO_ROOT` and `TMPDIR` to the binary in a way the suite accepts cannot be checked on this host; the first push shows it, and the receipt lists it as deferred. Alternative: keep Windows off the native job until Phase 6; rejected, the spec names it for 5e.
6. **`tests/install.bats` runs `update` through the installed engine's dispatcher**, not the repository's: through the repository's, `_native_bin` finds `target/release/agentsync` and the switch never fires. This is also what a real link does.
7. **Task order:** the installer, the switch, and the dispatcher with their tests (Task 1), the docs and CI (Task 2). **Recommended:** as listed.

## Module closure

```text
install.sh                       1-212    rewritten whole
lib/helpers/update.sh            49-152   _update_release_target, _update_file_sha256, _update_switch_to_binary (new)
                                 300      the call after the stash note in cmd_update
                                 567-572  the clone notice in check_for_updates
bin/agentsync.sh                 289-296  the _native_bin candidates
tests/install.bats               1-252    rewritten whole, 13 cases
tests/update_native.bats                  the curl stand-in and fixture layout reused
.github/workflows/ci.yaml        123-174  the native job
README.md                        44-50, 92-118, 238, 970-1001, 1010-1012
.ai/src/AGENTS.md                3, 11, 16-18, 24-25, 36
docs/specs/…design.md            305-312, 320-336
module-map.md                    170      the install.bats row
```

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
grep -c '^@test' tests/install.bats
grep -c '_update_switch_to_binary' lib/helpers/update.sh
grep -c 'bin/agentsync"' bin/agentsync.sh
```

Expected: `41aacd8 docs(native): close phase 5d`; `326 passed`, `0 passed`, `11 passed`, `1 passed`; `6`; `0`; `0`.

---

### Task 1: The installer, the switch, the dispatcher, and their tests

**Files:**
- Rewrite: `install.sh`, `tests/install.bats`
- Modify: `lib/helpers/update.sh`, `bin/agentsync.sh`

**Interfaces:**

```bash
# install.sh
detect_target                  # the cargo-dist target, or return 1
install_binary <tag> <target>  # sets INSTALLED, or sets HTTP_CODE and returns 1
install_from_source            # the git clone pinned to PIN_VERSION; sets INSTALLED
# lib/helpers/update.sh
_update_release_target
_update_file_sha256 <file>
_update_switch_to_binary <install_dir> <version>   # always returns 0
# bin/agentsync.sh _native_bin candidates, in order
#   target/release/agentsync[.exe], bin/agentsync[.exe], bin/agentsync-native[.exe]
```

- [x] **Step 1: Write `install.sh`**

<!-- file: install.sh -->
````bash
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
````

- [x] **Step 2: Apply the update-module and dispatcher patch**

Save the block below as `$TMPDIR/task1.diff` and run `git apply "$TMPDIR/task1.diff"`.

<!-- file: task1.diff -->
````diff
diff --git a/bin/agentsync.sh b/bin/agentsync.sh
index f40645c..3b5d1fb 100755
--- a/bin/agentsync.sh
+++ b/bin/agentsync.sh
@@ -289,6 +289,8 @@ _native_bin() {
     for candidate in \
         "$_AGENTSYNC_ENGINE_ROOT/target/release/agentsync" \
         "$_AGENTSYNC_ENGINE_ROOT/target/release/agentsync.exe" \
+        "$_AGENTSYNC_ENGINE_ROOT/bin/agentsync" \
+        "$_AGENTSYNC_ENGINE_ROOT/bin/agentsync.exe" \
         "$_AGENTSYNC_ENGINE_ROOT/bin/agentsync-native" \
         "$_AGENTSYNC_ENGINE_ROOT/bin/agentsync-native.exe"; do
         if [[ -x "$candidate" ]]; then
diff --git a/lib/helpers/update.sh b/lib/helpers/update.sh
index 05c13e6..8c84d3a 100644
--- a/lib/helpers/update.sh
+++ b/lib/helpers/update.sh
@@ -46,6 +46,107 @@ _update_checkout_version() {
     )
 }
 
+# The cargo-dist target this host runs, as the release archive is named.
+_update_release_target() {
+    local os arch
+    os=$(uname -s 2>/dev/null || echo unknown)
+    arch=$(uname -m 2>/dev/null || echo unknown)
+    case "$os:$arch" in
+        Darwin:arm64|Darwin:aarch64) echo "aarch64-apple-darwin" ;;
+        Darwin:x86_64)               echo "x86_64-apple-darwin" ;;
+        Linux:aarch64|Linux:arm64)   echo "aarch64-unknown-linux-musl" ;;
+        Linux:x86_64)                echo "x86_64-unknown-linux-musl" ;;
+        MINGW*:x86_64|MSYS*:x86_64|CYGWIN*:x86_64) echo "x86_64-pc-windows-msvc" ;;
+        *) return 1 ;;
+    esac
+}
+
+_update_file_sha256() {
+    local file="$1"
+    if command -v sha256sum >/dev/null 2>&1; then
+        sha256sum "$file" | awk '{print $1}'
+    elif command -v shasum >/dev/null 2>&1; then
+        shasum -a 256 "$file" | awk '{print $1}'
+    else
+        return 1
+    fi
+}
+
+# After a source install reaches a release that ships a binary, download it,
+# verify it, place it at <install_dir>/bin/agentsync[.exe], and point the
+# agentsync link at it, so the next run is the binary and `agentsync update`
+# replaces the binary from then on. A release without an archive is silent;
+# any other failure is a warning and the checkout keeps running.
+# Usage: _update_switch_to_binary "<install_dir>" "<version>"
+_update_switch_to_binary() {
+    local install_dir="$1" version="$2"
+    if declare -F _native_bin >/dev/null 2>&1 && _native_bin >/dev/null 2>&1; then
+        return 0
+    fi
+    command -v curl >/dev/null 2>&1 || return 0
+    local target
+    target=$(_update_release_target) || return 0
+    local ext="tar.xz" exe="agentsync"
+    if [[ "$target" == *-windows-* ]]; then
+        ext="zip"; exe="agentsync.exe"
+    fi
+    local archive_name="agentsync-$target.$ext"
+    local base="https://github.com/$AGENTSYNC_REPO/releases/download/$version"
+    local work
+    work=$(tmp_dir) || return 0
+
+    local code
+    code=$(curl -sL --max-time 30 -o "$work/$archive_name" -w '%{http_code}' "$base/$archive_name" 2>/dev/null) || code="000"
+    [[ "$code" != "404" ]] || return 0
+    if [[ "$code" != "200" ]]; then
+        echo "  $(_yellow "Warning"): could not download the agentsync binary (HTTP $code); still running the Bash engine." >&2
+        return 0
+    fi
+    code=$(curl -sL --max-time 30 -o "$work/$archive_name.sha256" -w '%{http_code}' "$base/$archive_name.sha256" 2>/dev/null) || code="000"
+    if [[ "$code" != "200" ]]; then
+        echo "  $(_yellow "Warning"): could not download the checksum of $archive_name (HTTP $code); still running the Bash engine." >&2
+        return 0
+    fi
+    local expected actual
+    expected=$(awk '{print $1; exit}' "$work/$archive_name.sha256")
+    actual=$(_update_file_sha256 "$work/$archive_name") || {
+        echo "  $(_yellow "Warning"): neither sha256sum nor shasum is available to verify $archive_name; still running the Bash engine." >&2
+        return 0
+    }
+    if [[ "$expected" != "$actual" ]]; then
+        echo "  $(_yellow "Warning"): checksum mismatch for $archive_name; still running the Bash engine." >&2
+        return 0
+    fi
+    mkdir -p "$work/unpacked"
+    if ! tar -xf "$work/$archive_name" -C "$work/unpacked" 2>/dev/null; then
+        echo "  $(_yellow "Warning"): could not unpack $archive_name; still running the Bash engine." >&2
+        return 0
+    fi
+    local unpacked="" candidate
+    for candidate in "$work/unpacked/$exe" "$work/unpacked"/*/"$exe"; do
+        if [[ -f "$candidate" ]]; then
+            unpacked="$candidate"
+            break
+        fi
+    done
+    if [[ -z "$unpacked" ]]; then
+        echo "  $(_yellow "Warning"): $archive_name does not contain $exe; still running the Bash engine." >&2
+        return 0
+    fi
+    mkdir -p "$install_dir/bin"
+    cp "$unpacked" "$install_dir/bin/$exe.new"
+    chmod +x "$install_dir/bin/$exe.new"
+    mv -f "$install_dir/bin/$exe.new" "$install_dir/bin/$exe"
+
+    local current_bin
+    current_bin=$(command -v agentsync 2>/dev/null) || true
+    if [[ -n "$current_bin" ]] && [[ -L "$current_bin" ]]; then
+        ln -sf "$install_dir/bin/$exe" "$current_bin" 2>/dev/null \
+            || echo "  $(_yellow "Warning"): could not re-link $current_bin to the binary — run the installer to repair it." >&2
+    fi
+    echo "  $(_green "Switched to the agentsync binary") v$version $(_dim "at $install_dir/bin/$exe; the checkout stays beside it.")"
+}
+
 # `--force` on the tag fetch is load-bearing: the install mirrors upstream and
 # never owns tags, so when a release tag is moved upstream a plain `--tags` fetch
 # rejects it as "would clobber existing tag" and aborts the whole update. On
@@ -196,6 +297,8 @@ cmd_update() {
         echo "  $(_dim "Set aside local edits in the install dir — recoverable via") $(_cyan "git -C \"$install_dir\" stash list")$(_dim ".")"
     fi
 
+    _update_switch_to_binary "$install_dir" "$new_version"
+
     # Show what's new from CHANGELOG.md (all versions between old and new)
     _show_changelog_range "$install_dir" "$old_version" "$new_version"
 
@@ -465,6 +568,13 @@ check_for_updates() {
     install_dir=$(resolve_install_dir 2>/dev/null) || return 0
     [[ -d "$install_dir/.git" ]] || return 0
 
+    # A source install without a binary (a developer checkout with a build is
+    # not one) is told once per run how to move to the binary.
+    if [[ "$install_dir" == "${AGENTSYNC_HOME:-}" ]] && [[ ! -d "$install_dir/target" ]] \
+        && declare -F _native_bin >/dev/null 2>&1 && ! _native_bin >/dev/null 2>&1; then
+        echo "  $(_dim "This install runs the Bash engine;") $(_cyan "agentsync update") $(_dim "moves it to the agentsync binary.")"
+    fi
+
     local cache_file="$install_dir/.update_cache"
 
     # Show banner from cache (written by previous background fetch)
````

- [x] **Step 3: Write `tests/install.bats`**

<!-- file: tests/install.bats -->
````bash
#!/usr/bin/env bats
# install.sh and `update <version>` against a purpose-built origin repository
# and a curl stand-in that serves fixture binary releases. Every path the
# installer touches is redirected into the test project so the developer's
# ~/.agentsync, PATH symlink, and shell rc are never modified.
#
# The origin is a fixture, not this repository: `actions/checkout` clones without
# tags, so pinning to a real release tag fails on CI even though it passes on a
# developer's full clone. The fixture carries its own tags and keeps the tests
# independent of this repository's tag history. The binary release is a fixture
# too: an archive laid out as cargo-dist lays it out, its binary a script.

load test_helper

# Tags the fixture origin publishes. Deliberately outside any real release
# series so they cannot be mistaken for this project's versions.
FIXTURE_NEW="9.9.2"
FIXTURE_OLD="9.9.1"
FIXTURE_ABSENT="999.0.0"

# Build the origin inside the per-test project rather than sharing one from
# setup_file: bats versions differ in whether an exported setup_file variable
# reaches the tests, and an empty origin URL fails every test here for a reason
# that has nothing to do with the installer.
#
# `update` fetches `origin main` by name, so the fixture needs that branch.
# `git branch -M` rather than `init -b`: the latter needs git >= 2.28, and the
# force form is a no-op when the default branch is already called main.
_build_origin_fixture() {
    local origin="$1"
    mkdir -p "$origin"
    (
        cd "$origin" || exit 1
        git init --quiet
        git config user.email "test@test.com"
        git config user.name "Test"

        cp -R "$REPO_ROOT/bin" "$REPO_ROOT/lib" .
        printf '%s\n' "$FIXTURE_OLD" > VERSION
        git add -A
        git commit --quiet -m "fixture engine $FIXTURE_OLD"
        git tag "$FIXTURE_OLD"
        git branch -M main

        printf '%s\n' "$FIXTURE_NEW" > VERSION
        git commit --quiet -am "fixture engine $FIXTURE_NEW"
        git tag "$FIXTURE_NEW"
    )
}

# A `curl` on PATH that serves $FAKE_RELEASES for the URLs the installer and
# `update` build and prints the HTTP status as `curl -w %{http_code}` does.
# Nothing here reaches the network: a tag without a fixture release is a 404.
_install_curl_stub() {
    mkdir -p "$TEST_PROJECT/stub" "$FAKE_RELEASES/tags"
    cat > "$TEST_PROJECT/stub/curl" <<'EOF'
#!/usr/bin/env bash
out=""; url=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        -w|--max-time) shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
    esac
done
case "$url" in
    https://api.github.com/repos/yelmuratoff/agent_sync/releases/latest)
        file="$FAKE_RELEASES/latest.json" ;;
    https://api.github.com/repos/yelmuratoff/agent_sync/git/ref/tags/*)
        file="$FAKE_RELEASES/tags/${url##*/}" ;;
    https://github.com/yelmuratoff/agent_sync/releases/download/*)
        rest="${url#*/releases/download/}"; tag="${rest%%/*}"; name="${rest#*/}"
        case "$name" in
            agentsync-*.sha256) name="archive.tar.xz.sha256" ;;
            agentsync-*) name="archive.tar.xz" ;;
        esac
        file="$FAKE_RELEASES/$tag/$name" ;;
    *) printf '000'; exit 6 ;;
esac
if [ -f "$file" ]; then cp "$file" "$out"; printf '200'; else printf '404'; fi
EOF
    chmod +x "$TEST_PROJECT/stub/curl"
}

# A fixture binary release for <tag>: the archive holds a script that answers
# `version` with the tag, plus a CHANGELOG.md, under one directory.
# Usage: publish_release <tag>
publish_release() {
    local tag="$1"
    local top="$TEST_PROJECT/rel-$tag/agentsync-fixture"
    mkdir -p "$top" "$FAKE_RELEASES/$tag"
    cat > "$top/agentsync" <<EOF
#!/usr/bin/env bash
case "\${1:-}" in
    version) echo "agentsync v$tag" ;;
    *) exit 1 ;;
esac
EOF
    chmod +x "$top/agentsync"
    printf '# Changelog\n\n## %s\n\n- Fixture.\n' "$tag" > "$top/CHANGELOG.md"
    tar -cJf "$FAKE_RELEASES/$tag/archive.tar.xz" -C "$TEST_PROJECT/rel-$tag" agentsync-fixture
    printf '%s  archive.tar.xz\n' "$(file_sha256 "$FAKE_RELEASES/$tag/archive.tar.xz")" \
        > "$FAKE_RELEASES/$tag/archive.tar.xz.sha256"
}

setup() {
    setup_test_project
    export HOME="$TEST_PROJECT/home"
    mkdir -p "$HOME"
    touch "$HOME/.zshrc"

    INSTALL_ORIGIN="$TEST_PROJECT/origin"
    _build_origin_fixture "$INSTALL_ORIGIN"

    FAKE_RELEASES="$TEST_PROJECT/releases"
    export FAKE_RELEASES
    _install_curl_stub

    export AGENTSYNC_REPO_URL="$INSTALL_ORIGIN"
    export AGENTSYNC_INSTALL_DIR="$TEST_PROJECT/engine"
    export AGENTSYNC_BIN_DIR="$TEST_PROJECT/bin"
    # `update` re-links whichever agentsync is on PATH; keep it the test one,
    # and keep curl the stand-in.
    export PATH="$TEST_PROJECT/bin:$TEST_PROJECT/stub:$PATH"
}

teardown() {
    teardown_test_project
}

@test "install: the fixture origin publishes the tags these tests pin to" {
    run git -C "$INSTALL_ORIGIN" tag
    [ "$status" -eq 0 ]
    [[ "$output" == *"$FIXTURE_OLD"* ]]
    [[ "$output" == *"$FIXTURE_NEW"* ]]
    [[ "$output" != *"$FIXTURE_ABSENT"* ]]
    git -C "$INSTALL_ORIGIN" rev-parse --verify --quiet refs/heads/main
}

@test "install: the latest binary release is downloaded, verified, and linked" {
    publish_release "$FIXTURE_NEW"
    printf '{"tag_name":"%s"}\n' "$FIXTURE_NEW" > "$FAKE_RELEASES/latest.json"
    run bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [[ "$output" == *"Installed successfully!"*"v$FIXTURE_NEW"* ]]
    [ -x "$TEST_PROJECT/engine/bin/agentsync" ]
    [ -L "$TEST_PROJECT/bin/agentsync" ]
    [ "$("$TEST_PROJECT/bin/agentsync" version)" = "agentsync v$FIXTURE_NEW" ]
    [ ! -d "$TEST_PROJECT/engine/.git" ]
    ! grep -q "AGENTSYNC_HOME" "$HOME/.zshrc"
}

@test "install: AGENTSYNC_VERSION pins a binary release" {
    publish_release "$FIXTURE_NEW"
    run env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [ "$("$TEST_PROJECT/bin/agentsync" version)" = "agentsync v$FIXTURE_NEW" ]
    [ ! -d "$TEST_PROJECT/engine/.git" ]
}

@test "install: a checksum mismatch aborts before anything is linked" {
    publish_release "$FIXTURE_NEW"
    printf '0000  archive.tar.xz\n' > "$FAKE_RELEASES/$FIXTURE_NEW/archive.tar.xz.sha256"
    run env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 1 ]
    [[ "$output" == *"checksum mismatch"* ]]
    [ ! -e "$TEST_PROJECT/engine/bin/agentsync" ]
    [ ! -e "$TEST_PROJECT/bin/agentsync" ]
}

@test "install: without a pin an unreachable GitHub fails clearly" {
    run bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 1 ]
    [[ "$output" == *"could not read the latest release"* ]]
    [ ! -e "$TEST_PROJECT/bin/agentsync" ]
}

@test "install: AGENTSYNC_VERSION pins a tag without a binary from source" {
    run env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [[ "$output" == *"predates the binary releases"* ]]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_NEW" ]
    [ -e "$TEST_PROJECT/bin/agentsync" ]
    grep -q "AGENTSYNC_HOME" "$HOME/.zshrc"
}

@test "install: an unknown AGENTSYNC_VERSION fails clearly" {
    run env AGENTSYNC_VERSION="$FIXTURE_ABSENT" bash "$REPO_ROOT/install.sh"
    [ "$status" -ne 0 ]
    [[ "$output" == *"$FIXTURE_ABSENT"* ]]
}

@test "install: re-running with a different pin moves an existing source install" {
    env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh" >/dev/null
    run env AGENTSYNC_VERSION="$FIXTURE_OLD" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_OLD" ]
}

@test "install: a binary release replaces an existing source install's link" {
    env AGENTSYNC_VERSION="$FIXTURE_OLD" bash "$REPO_ROOT/install.sh" >/dev/null
    publish_release "$FIXTURE_NEW"
    run env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh"
    [ "$status" -eq 0 ]
    [ "$("$TEST_PROJECT/bin/agentsync" version)" = "agentsync v$FIXTURE_NEW" ]
    [ -d "$TEST_PROJECT/engine/.git" ]
}

@test "update <version>: pins an installed engine to that release" {
    env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh" >/dev/null
    run env AGENTSYNC_HOME="$TEST_PROJECT/engine" bash "$TEST_PROJECT/engine/bin/agentsync.sh" update "$FIXTURE_OLD"
    [ "$status" -eq 0 ]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_OLD" ]
    [[ "$output" == *"$FIXTURE_OLD"* ]]
    [[ "$output" != *"Switched to the agentsync binary"* ]]
}

@test "update <version>: rejects a version that is not a release tag" {
    env AGENTSYNC_VERSION="$FIXTURE_NEW" bash "$REPO_ROOT/install.sh" >/dev/null
    run env AGENTSYNC_HOME="$TEST_PROJECT/engine" bash "$TEST_PROJECT/engine/bin/agentsync.sh" update "$FIXTURE_ABSENT"
    [ "$status" -ne 0 ]
    [[ "$output" == *"$FIXTURE_ABSENT"* ]]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_NEW" ]
}

@test "update <version>: a source install switches to the binary when the release has one" {
    env AGENTSYNC_VERSION="$FIXTURE_OLD" bash "$REPO_ROOT/install.sh" >/dev/null
    publish_release "$FIXTURE_NEW"
    run env AGENTSYNC_HOME="$TEST_PROJECT/engine" bash "$TEST_PROJECT/engine/bin/agentsync.sh" update "$FIXTURE_NEW"
    [ "$status" -eq 0 ]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_NEW" ]
    [[ "$output" == *"Switched to the agentsync binary"* ]]
    [ -x "$TEST_PROJECT/engine/bin/agentsync" ]
    [ "$(readlink "$TEST_PROJECT/bin/agentsync")" = "$TEST_PROJECT/engine/bin/agentsync" ]
    [ "$("$TEST_PROJECT/bin/agentsync" version)" = "agentsync v$FIXTURE_NEW" ]
}

@test "update <version>: a bad checksum keeps the source install on Bash" {
    env AGENTSYNC_VERSION="$FIXTURE_OLD" bash "$REPO_ROOT/install.sh" >/dev/null
    publish_release "$FIXTURE_NEW"
    printf '0000  archive.tar.xz\n' > "$FAKE_RELEASES/$FIXTURE_NEW/archive.tar.xz.sha256"
    run env AGENTSYNC_HOME="$TEST_PROJECT/engine" bash "$TEST_PROJECT/engine/bin/agentsync.sh" update "$FIXTURE_NEW"
    [ "$status" -eq 0 ]
    [ "$(cat "$TEST_PROJECT/engine/VERSION")" = "$FIXTURE_NEW" ]
    [[ "$output" == *"checksum mismatch"* ]]
    [ ! -e "$TEST_PROJECT/engine/bin/agentsync" ]
    [ "$(readlink "$TEST_PROJECT/bin/agentsync")" = "$TEST_PROJECT/engine/bin/agentsync.sh" ]
}
````

Run: `shasum -a 256 install.sh lib/helpers/update.sh bin/agentsync.sh tests/install.bats`
Expected:

```text
6caae46dbf975b4fdf91663177db7878c7f563b5cff2a957b3b5e38cec4cb244  install.sh
28b2737eeb46df20830757dde047a4b6fde432ec12902a42b0d9ba78af96578c  lib/helpers/update.sh
dcd8074610c453cb5bf7766e1e90179e6e2d2f46b32f348e2c97ebc86121007e  bin/agentsync.sh
72de77e3c69d1760d232d5a3f053a0512169b6d536befdb12c301d7ae2569035  tests/install.bats
```

- [x] **Step 4: Lint and the bats files**

```bash
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh; echo "shellcheck=$?"
bats --tap tests/install.bats 2>&1 | grep -c '^ok'
for f in update update_snapshot native_dispatch cli update_native version_pin changelog_render; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" 2>&1 | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" 2>&1 | grep -c '^not ok')"
done
```

Expected: `shellcheck=0`; `13` (the fixture tags, the latest binary release linked and answering `version`, a pinned binary release, a checksum mismatch aborting before the link, an unreachable GitHub, a pinned tag without a binary installing from source with `AGENTSYNC_HOME`, an unknown tag, a moved pin, a binary release replacing a source install's link, `update <version>` pinning, an unknown tag refused, a source install switching to the binary with the link re-pointed, a bad checksum keeping the source install on Bash); `bash=0 native=0` for each of the seven files.

- [x] **Step 5: The clone notice on a pty**

Set `S="$TMPDIR/phase5e"; mkdir -p "$S"` and write `$S/clone_notice_tty.sh`:

<!-- file: clone_notice_tty.sh -->
````bash
#!/usr/bin/env bash
# Usage: clone_notice_tty.sh <repo root> <out file>
# Builds a source install (a git checkout holding bin/ and lib/, no build) and
# runs `list` on a pty through `script` with AGENTSYNC_HOME pointing at it,
# recording how many times the one-line "moves it to the agentsync binary"
# notice prints: once for the install without a binary, never once
# bin/agentsync exists, never for the developer checkout (it has target/).
set -uo pipefail
REPO="$1"; OUT="$2"
: > "$OUT"
work=$(mktemp -d "${TMPDIR:-/tmp}/agentsync_clone_notice.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/install" "$work/project/.ai"
cp -R "$REPO/bin" "$REPO/lib" "$work/install/"
cp "$REPO/VERSION" "$work/install/VERSION"
git -C "$work/install" init --quiet
printf 'format: 2\ntools:\n  enabled: []\n' > "$work/project/.ai/agent_sync.yaml"
run_case() {
    local label="$1"; shift
    local log="$work/$label.log"
    (cd "$work/project" && script -q "$log" env "$@" > /dev/null 2>&1)
    printf '%s clone-notice=%s\n' "$label" "$(grep -c 'moves it to the agentsync binary' "$log")" >> "$OUT"
}
run_case source-install AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$work/install" bash "$work/install/bin/agentsync.sh" list
printf '#!/usr/bin/env bash\necho fake\n' > "$work/install/bin/agentsync"
chmod +x "$work/install/bin/agentsync"
run_case with-binary AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$work/install" bash "$work/install/bin/agentsync.sh" list
run_case developer AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$REPO" bash "$REPO/bin/agentsync.sh" list
rm -f "$work/install/.update_cache" "$REPO/.update_cache"
````

Run outside the agent sandbox: `bash "$S/clone_notice_tty.sh" "$PWD" "$S/clone_notice.out"; cat "$S/clone_notice.out"`
Expected, three lines:

```text
source-install clone-notice=1
with-binary clone-notice=0
developer clone-notice=0
```

- [x] **Step 6: Commit**

```bash
git add install.sh lib/helpers/update.sh bin/agentsync.sh tests/install.bats
git commit -m "feat(install): download the binary and move source installs to it"
```

---

### Task 2: Docs, CI, and the module map

**Files:**
- Modify: `README.md`, `.ai/src/AGENTS.md`, `.github/workflows/ci.yaml`, `docs/specs/2026-09-12-rust-migration-design.md`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest` (regenerated)

- [ ] **Step 1: Apply the docs patch and regenerate the outputs**

Save the block below as `$TMPDIR/task2.diff` and run `git apply "$TMPDIR/task2.diff"`.

<!-- file: task2.diff -->
````diff
diff --git a/.ai/src/AGENTS.md b/.ai/src/AGENTS.md
index c617d81..3f9201d 100644
--- a/.ai/src/AGENTS.md
+++ b/.ai/src/AGENTS.md
@@ -1,6 +1,6 @@
 # AgentSync CLI Agent
 
-You are a senior Bash/Shell engineer working on AgentSync — a CLI tool that syncs AI agent instructions from one `.ai/src/` directory to 13 supported tools: Claude Code, Cursor, Copilot, Gemini CLI, Codex, Windsurf, Junie, Cline, Amazon Q, Zed, Antigravity, Kimi Code, and OpenCode.
+You are a senior Rust and Bash engineer working on AgentSync — a CLI tool, shipped as a single static binary, that syncs AI agent instructions from one `.ai/src/` directory to 13 supported tools: Claude Code, Cursor, Copilot, Gemini CLI, Codex, Windsurf, Junie, Cline, Amazon Q, Zed, Antigravity, Kimi Code, and OpenCode.
 
 ## How to work
 
@@ -8,21 +8,21 @@ You are a senior Bash/Shell engineer working on AgentSync — a CLI tool that sy
 - **Portability** — Every command runs on macOS, Linux, and Git Bash on Windows. Reach for portable flags, `cd "$(dirname "$path")" && pwd` instead of `realpath`, and write-then-`mv` instead of platform-specific `sed -i`.
 - **Strict mode stays on** — Executable entry points enable `set -euo pipefail`; sourced helpers remain safe under it. Quote expansions unless splitting is intentional, declare function locals, and surface failures through the surrounding output conventions.
 - **Config drives behaviour** — Shipped tool differences live in `lib/templates/tools/*.yaml`; `.ai/src/tools/` contains project overrides. Extend with a YAML option and a generic helper rather than branching on tool name inside `lib/sync.sh`.
-- **Pure Bash** — The runtime reaches its goals without `yq`, `jq`, `python`, `node`, `perl`, `eval`, `realpath`, or `readlink -f`. Read supported YAML shapes through the helpers in `lib/helpers/yaml.sh`.
+- **Single static binary** — The runtime reaches its goals without `yq`, `jq`, `python`, `node`, `perl`, `eval`, `realpath`, or `readlink -f`, in Rust and in the Bash reference alike. Read supported YAML shapes through `src/yaml_subset.rs`, which mirrors `lib/helpers/yaml.sh`; no YAML crate.
 - **Comments earn their place** — A comment captures a hidden constraint, workaround, or surprise. If the code already shows the meaning, leave the comment out.
 
 ## Tech Stack
 
-- **Language**: Bash (strict mode: `set -euo pipefail`)
-- **Entry point**: `bin/agentsync.sh` — delegates to `lib/helpers/*.sh` modules
-- **Native engine**: Rust crate at the repo root (`src/`), templates embedded from `lib/templates/`; `bin/agentsync.sh` delegates the commands listed in `_NATIVE_COMMANDS` to `target/release/agentsync`
+- **Language**: Rust (edition 2024, `unsafe_code = "forbid"`) for the shipped engine; Bash (strict mode: `set -euo pipefail`) for the reference engine until Phase 6 retires it
+- **Entry point**: the `agentsync` binary (`src/main.rs`); in the repository `bin/agentsync.sh` is the Bash reference and parity harness, delegating to `lib/helpers/*.sh` modules
+- **Native engine**: Rust crate at the repo root (`src/`), templates embedded from `lib/templates/`; `bin/agentsync.sh` delegates the commands listed in `_NATIVE_COMMANDS` to `target/release/agentsync`; `update` on a binary install replaces the binary from GitHub Releases (`src/cli/update.rs`)
 - **Sync engine**: `lib/sync.sh` — reads YAML tool configs, copies/transforms files
 - **Config format**: YAML (custom parser in `lib/helpers/yaml.sh`, no `yq` dependency)
 - **Templates**: `lib/templates/` — shipped tool/payload bases and init/refresh content
 - **Transactions**: `lib/helpers/backup.sh` — snapshots managed targets for `init`, `sync`, and `rollback`
 - **Tests**: [bats-core](https://github.com/bats-core/bats-core) in `tests/*.bats`
-- **CI**: GitHub Actions — ShellCheck lint + bats tests on Linux/macOS/Windows
-- **Install**: `curl | bash` via `install.sh`, symlinked to `~/.agentsync/`
+- **CI**: GitHub Actions — ShellCheck lint, bats tests on Linux/macOS/Windows against Bash and against the binary, `cargo test`; cargo-dist builds the release (`dist-workspace.toml`, `.github/workflows/release.yml`)
+- **Install**: `curl | bash` via `install.sh`, which downloads the binary for the platform from GitHub Releases, verifies its sha256, and links `~/.agentsync/bin/agentsync`; the cargo-dist installers ship alongside
 
 ## Approach
 
@@ -33,7 +33,7 @@ You are a senior Bash/Shell engineer working on AgentSync — a CLI tool that sy
 
 ## Boundaries
 
-- Stick to Bash and coreutils. Reach for an existing helper before introducing a new tool.
+- Stick to the Rust standard library and the crates already in `Cargo.toml`; in Bash, stick to coreutils. Reach for an existing helper before introducing a new tool.
 - Treat `.ai/src/` as the only source; generated output directories (`.claude/`, `.cursor/`, etc.) are disposable and regenerated by `agentsync sync`.
 - Keep YAML within the shapes supported by `lib/helpers/yaml.sh`; extend the parser only for a concrete configuration need.
 - Preserve transactional safety for mutating commands. A failed `init`, `sync`, or `rollback` must restore the pre-operation state.
diff --git a/.ai/src/skills/native-port/references/module-map.md b/.ai/src/skills/native-port/references/module-map.md
index 02c3c30..fc5c855 100644
--- a/.ai/src/skills/native-port/references/module-map.md
+++ b/.ai/src/skills/native-port/references/module-map.md
@@ -165,7 +165,7 @@ tests/generate.bats 7         generate
 tests/gitignore.bats 7        unit: gitignore.sh
 tests/workspace.bats 7        sync --workspace
 tests/update_native.bats 11   update on a binary install, the binary run directly (Phase 5d)
-tests/install.bats 6          install.sh, update <version>
+tests/install.bats 13         install.sh (binary and source installs, curl stand-in), update <version>, the switch to the binary
 tests/rollback.bats 6         rollback
 tests/update.bats 6           update
 ```
diff --git a/.github/workflows/ci.yaml b/.github/workflows/ci.yaml
index e302aad..b7ccd90 100644
--- a/.github/workflows/ci.yaml
+++ b/.github/workflows/ci.yaml
@@ -123,7 +123,7 @@ jobs:
   native:
     name: Native engine (${{ matrix.os }})
     runs-on: ${{ matrix.os }}
-    timeout-minutes: 40
+    timeout-minutes: 100
     strategy:
       fail-fast: false
       matrix:
@@ -147,9 +147,23 @@ jobs:
           else
             brew install bats-core parallel
           fi
-      # Windows joins in Phase 5, when the binary is the entry point: under
-      # Git Bash the POSIX paths in AGENTSYNC_REPO_ROOT and TMPDIR do not reach
-      # a native executable.
+      - name: Cache bats-core
+        if: runner.os == 'Windows'
+        id: cache-bats
+        uses: actions/cache@v4
+        with:
+          path: ~/.local
+          key: bats-core-${{ runner.os }}-v1
+      - name: Install bats-core
+        if: runner.os == 'Windows' && steps.cache-bats.outputs.cache-hit != 'true'
+        shell: bash
+        run: |
+          git clone --depth 1 https://github.com/bats-core/bats-core.git /tmp/bats-core
+          /tmp/bats-core/install.sh "$HOME/.local"
+      - name: Put bats on PATH
+        if: runner.os == 'Windows'
+        shell: bash
+        run: echo "$HOME/.local/bin" >> "$GITHUB_PATH"
       # Every command that writes outputs goes through the native sync from
       # Phase 3 on, so the whole suite is the native contract.
       - name: The Bash suite against the native engine
@@ -159,3 +173,13 @@ jobs:
           TERM: xterm
           AGENTSYNC_NATIVE: "1"
         run: bats --jobs 4 --tap tests/
+      # Git Bash cannot run bats --jobs; the binary forks nothing, so the
+      # suite runs serially and unsharded here (Phase 5e).
+      - name: The Bash suite against the native engine (serial)
+        if: runner.os == 'Windows'
+        shell: bash
+        timeout-minutes: 90
+        env:
+          TERM: xterm
+          AGENTSYNC_NATIVE: "1"
+        run: bats --tap tests/
diff --git a/README.md b/README.md
index eefc2f0..366b049 100644
--- a/README.md
+++ b/README.md
@@ -45,9 +45,9 @@ The frontmatter above is the always-on default. Give a rule `paths:` frontmatter
 ## Why not just...?
 
 - **...symlink the files?** Tools demand different extensions (`.mdc`, `.instructions.md`), different frontmatter, different nesting. Symlinks can't transform content — AgentSync does.
-- **...a shell script per tool?** You'd be writing the same copy / rename / header-injection logic 13 times. AgentSync is that script, declarative (YAML), already tested on macOS, Linux, and Windows (Git Bash).
+- **...a shell script per tool?** You'd be writing the same copy / rename / header-injection logic 13 times. AgentSync is that script, declarative (YAML), already tested on macOS, Linux, and Windows.
 - **...stick to the one tool I use today?** Teammates pick different ones. Your future self might too. A single source file future-proofs you.
-- **Zero runtime dependencies.** Pure Bash. No Node, Python, `yq`, or `jq`. Install with one `curl | bash`.
+- **Zero runtime dependencies.** A single static binary. No Node, Python, `yq`, or `jq`. Install with one `curl | bash`.
 
 <details>
 <summary><strong>Table of contents</strong></summary>
@@ -91,25 +91,31 @@ The frontmatter above is the always-on default. Give a rule `paths:` frontmatter
 
 ## Installation
 
-Requirements: `git`, `bash`. Works on **macOS** and **Linux** out of the box. On **Windows**, use [WSL](https://learn.microsoft.com/en-us/windows/wsl/install) or [Git Bash](https://gitforwindows.org/) (included with Git for Windows).
+Requirements: `curl` and `tar`. AgentSync is one static binary for **macOS** (Apple silicon and Intel), **Linux** (x86_64 and arm64) and **Windows** (x86_64).
 
 ```bash
-curl -fsSL https://raw.githubusercontent.com/yelmuratoff/agent/main/install.sh | bash
+curl -fsSL https://raw.githubusercontent.com/yelmuratoff/agent_sync/main/install.sh | bash
+```
+
+On Windows, from PowerShell:
+
+```powershell
+irm https://github.com/yelmuratoff/agent_sync/releases/latest/download/agentsync-installer.ps1 | iex
 ```
 
 To install the exact release a project pins in `agentsync_version` — what CI should do when outputs are committed — set `AGENTSYNC_VERSION`; the same variable moves an existing install, and `agentsync update <version>` does it from the CLI:
 
 ```bash
-AGENTSYNC_VERSION=0.35.0 curl -fsSL https://raw.githubusercontent.com/yelmuratoff/agent/main/install.sh | bash
+AGENTSYNC_VERSION=0.36.0 curl -fsSL https://raw.githubusercontent.com/yelmuratoff/agent_sync/main/install.sh | bash
 ```
 
 What the installer does:
 
-1. Clones the repository to `~/.agentsync/`
-2. Creates a symlink `agentsync` in `/usr/local/bin/` (falls back to `~/.local/bin/`)
-3. Adds `AGENTSYNC_HOME` to your shell config (`~/.zshrc` or `~/.bashrc`)
+1. Downloads the release archive for your platform from GitHub Releases and verifies its sha256
+2. Places the binary at `~/.agentsync/bin/agentsync`
+3. Creates a symlink `agentsync` in `/usr/local/bin/` (falls back to `~/.local/bin/`)
 
-Restart your terminal or run `source ~/.zshrc` after installation. Running the installer again updates via `git pull`.
+`agentsync update` replaces the binary with the latest release, and `agentsync update <version>` pins one. Releases before the first binary release have no archive: pinning to one installs from source (a git clone in `~/.agentsync/` with `AGENTSYNC_HOME` in your shell config, as the installer always did), and such an install moves to the binary by itself the next time `agentsync update` reaches a release that ships one.
 
 ## Team Setup
 
@@ -235,7 +241,7 @@ agentsync <command> [options]
 | `export`                 |       | Bundle `.ai/src/` into a shareable archive                                                      |
 | `import <src>`           |       | Import config from a GitHub repo, archive, or directory                                         |
 | `list`                   | `ls`  | Show configured tools and status                                                               |
-| `update`                 |       | Self-update to the latest main, or `update <version>` to pin a release tag                     |
+| `update`                 |       | Replace the binary with the latest release, or `update <version>` to pin a release tag         |
 | `upgrade-config`         |       | Re-pin `agentsync_version` in `agent_sync.yaml`                                                 |
 | `release`                |       | Bump version, tag, and push (maintainer)                                                        |
 | `version`                | `-v`  | Print version                                                                                  |
@@ -969,8 +975,9 @@ bats --jobs "$(( $(getconf _NPROCESSORS_ONLN) * 2 ))" tests/
 bats tests/sync.bats
 ```
 
-CI runs `--jobs 4` on Linux and macOS; Windows falls back to serial because
-GNU parallel isn't available under git-bash.
+CI runs `--jobs 4` on Linux and macOS; the Bash reference on Windows falls
+back to sharded serial runs because GNU parallel isn't available under
+git-bash, and the binary runs the suite there unsharded.
 
 Git Bash copies `ln -s` targets by default. The symlink-safety tests request
 native links with `MSYS=winsymlinks:nativestrict`; enable Windows Developer Mode
@@ -978,19 +985,25 @@ or grant the `Create symbolic links` privilege before running them locally.
 
 ### Native engine
 
-Commands are moving one by one to a Rust binary
-(`docs/specs/2026-09-12-rust-migration-design.md`). The Bash CLI hands a ported
-command to the binary when one is available:
+The engine is a Rust binary; every command is ported
+(`docs/specs/2026-09-12-rust-migration-design.md`). Installs run the binary
+directly. In the repository `bin/agentsync.sh` stays the Bash reference and the
+parity harness until Phase 6 deletes it, and hands a command to the binary when
+one is built:
 
 ```bash
 cargo build --release                 # target/release/agentsync
 agentsync list                        # served natively when the binary exists
 AGENTSYNC_NATIVE=0 agentsync list     # force the Bash implementation
-AGENTSYNC_NATIVE=1 bats tests/        # run the suite against the binary for ported commands
+AGENTSYNC_NATIVE=1 bats tests/        # run the suite against the binary
+tests/update_native.bats              # update on a binary install, the binary run directly
 ```
 
 `cargo test` covers the Rust side; `tests/native_parity.bats` diffs Bash
-against native output for every ported command.
+against native output for every ported command. Releases are built by
+cargo-dist (`dist-workspace.toml`): the auto-tag workflow dispatches
+`release.yml` for the tag it creates from `VERSION`, which publishes the five
+archives, their checksums, and the installers.
 
 ## License
 
@@ -1008,7 +1021,7 @@ source code. Third-party components retain their original licenses; see
 ```bash
 # Global
 rm -rf ~/.agentsync && rm -f /usr/local/bin/agentsync
-# Remove AGENTSYNC_HOME from ~/.zshrc
+# Remove AGENTSYNC_HOME from ~/.zshrc if a source install added it
 
 # Per project
 rm -rf .ai/
diff --git a/docs/specs/2026-09-12-rust-migration-design.md b/docs/specs/2026-09-12-rust-migration-design.md
index af6d6d5..f1ed6c5 100644
--- a/docs/specs/2026-09-12-rust-migration-design.md
+++ b/docs/specs/2026-09-12-rust-migration-design.md
@@ -302,8 +302,14 @@ Exit: `_NATIVE_COMMANDS` lists every command; the whole suite passes with
   release workflow runs on `workflow_dispatch` with the tag, `curl | sh` and
   PowerShell installers into `~/.agentsync/bin`, sha256 sums, artifact
   attestations.
-- `install.sh` becomes the generated installer (or downloads the binary and
-  verifies its checksum); `AGENTSYNC_VERSION=<tag>` still pins.
+- `install.sh` stays hand-written (5e): it detects the cargo-dist target,
+  downloads `agentsync-<target>.tar.xz` (`.zip` on Windows) and its `.sha256`
+  from GitHub Releases through `curl -w %{http_code}`, verifies the sum with
+  `sha256sum` or `shasum`, unpacks it through `tar` into
+  `~/.agentsync/bin/agentsync`, and links it; `AGENTSYNC_VERSION=<tag>` still
+  pins, and a pinned tag whose archive answers 404 installs from source as
+  before (decision 3). The cargo-dist installers ship alongside it. The
+  binary's install needs no `AGENTSYNC_HOME`; a source install still gets it.
 - `update` replaces the binary from GitHub Releases and keeps `update <version>`
   pinning; the `agentsync_version` gate is unchanged. The binary's `update`
   (5d) downloads `agentsync-<target>.tar.xz` (`.zip` on Windows) and its
@@ -319,10 +325,18 @@ Exit: `_NATIVE_COMMANDS` lists every command; the whole suite passes with
   auto-tag workflow triggers the release build.
 - The installed `agentsync` link points at the binary. `bin/agentsync.sh`
   stays the dispatcher in the repository, the parity harness, until Phase 6
-  deletes it.
-- Windows: the binary is the entry point; the bats suite runs against it
-  directly, without sharding. Terminal colour on legacy consoles is enabled
-  with the `anstream` crate if needed.
+  deletes it; `_native_bin` also accepts `<engine>/bin/agentsync[.exe]`, the
+  name the installers and the source-install switch use (5e). A source
+  install moves to the binary through Bash's `update` (5e,
+  `_update_switch_to_binary`): after the git reconcile it downloads and
+  verifies the archive of the new version, places
+  `<install>/bin/agentsync[.exe]`, and points the `agentsync` link at it; a
+  version without an archive is silent, any other failure is a warning and
+  the checkout keeps running. Until then, on a terminal, `check_for_updates`
+  prints one dim line naming `agentsync update` as the way to the binary.
+- Windows: the binary is the entry point; the native CI job runs the bats
+  suite against it serially and unsharded (5e). Terminal colour on legacy
+  consoles is enabled with the `anstream` crate if needed.
 
 Planned in five slices, each its own plan, in this order because each one
 builds on the one before:
````

Run: `shasum -a 256 README.md .ai/src/AGENTS.md .github/workflows/ci.yaml docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md`
Expected:

```text
98be040e3f263d9cbafee7432dba6ed43053664a6f252401d64e66796f5fbc79  README.md
21cabefd10dbe36d4bf54e3bd2e982cd5257773389774b75fe47c67fad9a143c  .ai/src/AGENTS.md
a54fca16f21e2f3c205e63611b70e627facc58e48ba26e668f2eb607609641e4  .github/workflows/ci.yaml
508d93d2b5418d22219290022a489acc4a4962bc1e78ca354a60ef1f70ba6da2  docs/specs/2026-09-12-rust-migration-design.md
41613d30cfc2d554ee38bba42aacced9c1ef1dd87ee655f167462bf2069b4d10  .ai/src/skills/native-port/references/module-map.md
```

Then `ruby -e 'require "yaml"; d=YAML.safe_load(File.read(".github/workflows/ci.yaml"), aliases: true); puts d["jobs"]["native"]["steps"].map{|s| s["name"]||s["uses"]}.compact.last(2); puts d["jobs"]["native"]["timeout-minutes"]'`
Expected: `The Bash suite against the native engine`, `The Bash suite against the native engine (serial)`, `100`.

Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force > /dev/null` (outside the sandbox if it refuses a write) and read `git status --short`: ` M .ai/.sync-manifest` alongside the five files and the plan.

- [ ] **Step 2: Verify (outside the agent sandbox where a file says so)**

Write `$S/native_suite.sh` when it does not exist:

<!-- file: native_suite.sh -->
````bash
#!/usr/bin/env bash
# Usage: native_suite.sh <repo root> <mode 0|1|both> <out file>
# Runs every bats file one at a time under the given engine(s) and prints
# `<file> bash=<failures> native=<failures>` per file, then a total.
set -uo pipefail
REPO="$1"; WHICH="$2"; OUT="$3"
: > "$OUT"
cd "$REPO" || exit 1
total_bash=0; total_native=0
for f in tests/*.bats; do
    name="${f#tests/}"; name="${name%.bats}"
    b="-"; n="-"
    if [[ "$WHICH" == "0" || "$WHICH" == "both" ]]; then
        b=$(AGENTSYNC_NATIVE=0 bats --tap "$f" 2>&1 | grep -c '^not ok')
        total_bash=$((total_bash + b))
    fi
    if [[ "$WHICH" == "1" || "$WHICH" == "both" ]]; then
        n=$(AGENTSYNC_NATIVE=1 bats --tap "$f" 2>&1 | grep -c '^not ok')
        total_native=$((total_native + n))
    fi
    printf '%s bash=%s native=%s\n' "$name" "$b" "$n" >> "$OUT"
done
printf 'TOTAL bash=%s native=%s\n' "$total_bash" "$total_native" >> "$OUT"
````

```bash
cargo build --release 2>&1 | tail -1
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
bash "$S/native_suite.sh" "$PWD" both "$S/suite_both.out" && tail -1 "$S/suite_both.out"
```

Expected: `Finished` (no Rust source changed); lint exit 0; `TOTAL bash=0 native=0` over the 51 bats files under both engines, `native_parity` run outside the sandbox. `sync` and `check` do not change, so no timings are due.

- [ ] **Step 3: Commit**

```bash
git add .ai/.sync-manifest README.md .ai/src/AGENTS.md .github/workflows/ci.yaml docs/specs/2026-09-12-rust-migration-design.md .ai/src/skills/native-port/references/module-map.md
git commit -m "docs(native): describe the single static binary"
```

---

## Completion

The plan is closed when every box is ticked, `tests/install.bats` passes its thirteen cases, every bats file is green under both engines, the pty harness shows the clone notice once for a source install and never otherwise, and a `## Completion receipt` records the fresh verification. The receipt lists as deferred everything only a published release or a Windows host exercises: the installer and the switch against a real GitHub release, the cargo-dist installers, the five runners, the attestation, and the Windows native bats run. Closing this plan closes Phase 5: the migration branch is merged and the first binary release is cut by the maintainer, never by this plan.

## Run log

### 2026-09-18 — Phase 5e planned
- Commits: this plan.
- Verified: the slice was drafted in the tree after 5d closed, verified, and parked in the session scratchpad (`phase5e/draft/`); the plan's blocks were extracted back and compared byte for byte with the parked files. Against the draft: ShellCheck exit 0; `tests/install.bats` 13/13 (after three draft bugs: an empty `PATH` that also hid `bash`, progress lines captured into the install path, and the repository's dispatcher finding its own release build so the switch never fired); `update`, `update_snapshot`, `native_dispatch`, `cli`, `update_native`, `version_pin`, `changelog_render` at 0 failures under both engines; `clone_notice_tty.sh` the three expected lines; the CI workflow parsed with the two native bats steps and a 100-minute job timeout; `sync --force` regenerated the manifest. The full suite under both engines was started on the final tree and is recorded in the execution entry.
- Plan amended: none.
- Next: Task 0 Step 1; the maintainer asked on 2026-09-18 to take the decisions and finish, so execution follows in the same session.
- Blocker: none.
