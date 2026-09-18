# Rust Migration Phase 5b: Native `release` and the Crate Version

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Make `Cargo.toml` and `Cargo.lock` carry `VERSION`, teach `release` to bump the three together, and make the binary answer `release` byte for byte: the prompt on stdout answered from stdin, git as the executable with its own output passing through, the tag message from the changelog section of the new version, and the push of `main` and the tag. `_NATIVE_COMMANDS` gains `release`. This is the second of the five Phase 5 slices; 5c's cargo-dist names its artifacts from the crate version this slice keeps equal to `VERSION`.

**Architecture:** The Cargo bump lands in Bash first (`lib/helpers/release.sh`, two awk helpers keyed on the `name = "agentsync"` line, which names the crate in both files), so the Bash reference is complete before the port. `src/cli/release.rs` holds `release(args, style, &mut Env, out, err) -> Result<u8, Error>` with `Env { cwd, install_dir, read_line }`, the pure helpers `crate_version`, `set_crate_version`, and `changelog_section` mirroring the awk programs, and `git()` spawning the executable on the process streams after flushing `out`, the tag message on git's stdin. `main.rs` derives `install_dir` from `AGENTSYNC_HOME` holding a `.git`. The seam stays the CLI process boundary, with one new helper: `assert_release_parity` in `tests/native_parity.bats` rebuilds a fixture checkout with a bare origin for each engine and compares output, status, and the checkout's state afterwards, because a release rewrites the checkout it runs in.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 5". Previous plan: `docs/plans/2026-09-16-rust-migration-phase-5a-entry.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. `release` writes only inside the checkout it releases: `VERSION`, `Cargo.toml`, `Cargo.lock`, one commit, one tag, and the push.
- No binary ships to users; without a binary every command runs in Bash. Bash changes: `lib/helpers/release.sh` learns the Cargo bump in its own commit before the port, because the parity suite needs the Bash reference until Phase 6, and `bin/agentsync.sh` changes only its `_NATIVE_COMMANDS` line. `lib/**/*.sh` and `bin/agentsync.sh` stay clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the checkout's state after the run, through the dispatcher, except the three accepted deviations Task 3 records: decimal `VERSION` components, the Rust I/O error for a `git` that cannot start or an unreadable `CHANGELOG.md`, and the `AGENTSYNC_HOME`-only fallback. On a terminal the escape codes are those of `cli_colors.sh`.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` alone reads the environment and terminal state. The binary spawns `git` alone, as Bash did.
- `Cargo.toml` and `Cargo.lock` carry `VERSION` from Task 1 on; `src/lib.rs` tests the equality, and `cargo build --release` follows every `VERSION` change.
- `check_for_updates` and the project-format notice stay in the dispatcher until 5d; `release` is not in `main`'s notice list, so nothing moves here.
- `tests/release.bats` seeds from the repository's `VERSION`: its seed carries its own `bin/` copy, whose dispatcher hands the seed's `VERSION` to the binary, and the binary refuses any other.
- Every expected value was captured from Bash on 2026-09-18: `phase5b/release_reference.sh` (20 situations, 423 lines, status, stdout, stderr, and checkout state apart) and `phase5b/release_tty.sh` (4 scenarios on a pty through `script`, 56 lines of `od -c`, 25 of them holding an escape). Both scripts and their fixture builder are reproduced in Task 3 Step 5.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

1. **The Cargo bump lands in Bash first (Task 2), the port mirrors it (Task 3).** Alternative: a Rust-only feature with `release.bats` skipped natively; rejected, because the parity suite needs the Bash reference until Phase 6, and a maintainer releasing from a clone without a built binary would tag a release whose crate still says `0.0.0`.
2. **`release.bats` seeds from the repository's `VERSION`, with `bumped <major|minor|patch>` computing the expected values.** A fixed `1.0.0` seed trips the stale-binary guard once `release` reaches the binary. Alternative: keep `1.0.0` and skip the file natively; rejected, it would leave `release` outside the native contract CI runs (`AGENTSYNC_NATIVE=1 bats tests/` on Linux and macOS).
3. **Three deviations and one quirk.** (a) The three `VERSION` components must be decimal integers: Bash's `$((…))` bumped `1.a.0` to `1.a.1` and died with the shell's syntax error on `1.2.3.4`. (b) A `git` that cannot start and an unreadable `CHANGELOG.md` report the Rust I/O error where Bash printed `command not found` (status 127) or awk's message (status 2); both engines reach the changelog after the commit. (c) The fallback checkout is `AGENTSYNC_HOME` alone when it holds a `.git`; Bash also tried the dispatcher's own checkout, which the binary has no counterpart for. Quirk 54: end of input at the `Continue? [Y/n]:` prompt exits 1 with nothing after the prompt, because `read -r` fails under errexit; the port reproduces it. Alternative for (a): evaluate the components as Bash arithmetic; rejected as an arithmetic evaluator for a maintainer typo.
4. **The tag message goes to `git tag -F -` on stdin**, not to a file in a run directory the binary does not have. Git receives the same bytes and cleans them the same way. Alternative: a file under the system temp dir; rejected as a file nobody reads.
5. **Task order:** the crate version first (Task 1), the Bash feature (Task 2), the port (Task 3), the docs (Task 4). **Recommended:** as listed.

## Module closure

```text
lib/helpers/release.sh           1-128    cmd_release; Task 2 adds _release_crate_version and _release_set_crate_version
lib/helpers/resolve.sh           31-42    resolve_install_dir: AGENTSYNC_HOME, then the dispatcher's checkout, each with a .git
lib/helpers/cli_colors.sh        5-13     _bold _green _cyan _red _dim
lib/helpers/tmp.sh               100-124  tmp_sibling (Task 2's staging), tmp_file (the tag message)
bin/agentsync.sh                 280      _NATIVE_COMMANDS
                                 398      the release arm: _need release; cmd_release
Cargo.toml                       1-11     the crate version, 0.0.0 today
Cargo.lock                       5-7      the crate's own entry
.github/workflows/auto-tag.yaml           tags from VERSION on main when the tag is missing; untouched
tests/release.bats               10 cases, the Bash reference
```

Reused: `style::Style`, `cli::customize::put`, `paths::logical_root`.

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
for f in release native_dispatch native_parity; do
    printf '%s bash=%s\n' "$f" "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
grep -c '^@test' tests/release.bats tests/native_parity.bats
sed -n '5p' Cargo.toml; sed -n '7p' Cargo.lock
```

Expected: the plan's latest commit; `289 passed`, `0 passed`, `11 passed`, `1 passed`; `0` for every file, with `native_parity` run outside the agent sandbox, which refuses the `diff -` stdin operand `src/cli/diff.rs` uses; `10`, `68` cases; `version = "0.0.0"` twice.

---

### Task 1: Carry `VERSION` in `Cargo.toml` and `Cargo.lock`

**Files:**
- Modify: `Cargo.toml:3-5`, `Cargo.lock:7` (rewritten by cargo), `src/lib.rs`, `.ai/src/rules/native-engine.md:16`, `.ai/.sync-manifest` (regenerated)

**Interfaces:**
- Produces: the invariant `env!("CARGO_PKG_VERSION") == agentsync::engine_version()`, tested in `src/lib.rs`; Task 2's `release` keeps it, Phase 5c's cargo-dist reads it.

- [x] **Step 1: Write the failing test**

Append to `src/lib.rs`:

```rust

#[cfg(test)]
mod tests {
    #[test]
    fn the_crate_version_carries_version() {
        assert_eq!(env!("CARGO_PKG_VERSION"), super::engine_version());
    }
}
```

Run: `cargo test --lib the_crate_version_carries_version 2>&1 | grep -E 'assertion|left|right'`
Expected:

```text
assertion `left == right` failed
  left: "0.0.0"
 right: "0.36.0"
```

- [x] **Step 2: Set the crate version**

In `Cargo.toml`, replace lines 3-5:

```toml
# The VERSION file is the release source of truth (auto-tag, `agentsync release`);
# this field stays 0.0.0 until Phase 5 wires cargo-dist to it.
version = "0.0.0"
```

with:

```toml
# Equal to VERSION: `agentsync release` bumps both, and src/lib.rs tests the pair.
version = "0.36.0"
```

Run:

```bash
cargo test 2>&1 | grep 'test result' | head -4
git diff --stat Cargo.lock
sed -n '7p' Cargo.lock
```

Expected: `290 passed`, `0 passed`, `11 passed`, `1 passed`; ` Cargo.lock | 2 +-` (cargo rewrote the crate's own entry); `version = "0.36.0"`.

- [x] **Step 3: The rule and its outputs**

In `.ai/src/rules/native-engine.md`, replace line 16:

```markdown
- `VERSION` is the only version source. The crate reads it with `include_str!`; `Cargo.toml` stays at `0.0.0` until Phase 5 wires cargo-dist.
```

with:

```markdown
- `VERSION` is the release source of truth. The crate reads it with `include_str!`, `Cargo.toml` and `Cargo.lock` carry the same value, `agentsync release` bumps the three together, and a test in `src/lib.rs` fails when the crate version and `VERSION` disagree.
```

Run:

```bash
AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --dry-run 2>&1 | grep -c 'WARNING'
AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force > /dev/null
diff .ai/src/rules/native-engine.md .claude/rules/native-engine.md && echo same
git status --short
```

Expected: `0` warnings; `same`; the status lists exactly ` M .ai/.sync-manifest`, ` M .ai/src/rules/native-engine.md`, ` M Cargo.lock`, ` M Cargo.toml`, ` M src/lib.rs` (the generated `.claude/`, `.agents/`, and `.codex/` outputs are gitignored). If the sandbox refuses a write, run the `sync --force` line outside it.

- [x] **Step 4: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add Cargo.toml Cargo.lock src/lib.rs .ai/src/rules/native-engine.md .ai/.sync-manifest docs/plans/2026-09-18-rust-migration-phase-5b-release.md
git commit -m "feat(native): carry VERSION in Cargo.toml and Cargo.lock"
```

Expected: both exit 0.

---

### Task 2: `release` Bumps `Cargo.toml` and `Cargo.lock` With `VERSION`

**Files:**
- Modify: `lib/helpers/release.sh`, `tests/release.bats`

**Interfaces:**
- Produces, for Task 3 to mirror: the pre-write refusal `Error: Cannot find the agentsync crate version in <Cargo.toml|Cargo.lock>` (status 1, nothing written, before the header), the lines `  Updated Cargo.toml → <version>` and `  Updated Cargo.lock → <version>` after `  Updated VERSION → <version>`, and `git add VERSION Cargo.toml Cargo.lock` before the commit. The version line rewritten is the first `version = "…"` after the line `name = "agentsync"` in either file.

- [x] **Step 1: Write the failing tests**

Replace `tests/release.bats` with:

```bash
#!/usr/bin/env bats
# Tests for agentsync release.

load test_helper

# The version `release <bump>` writes from the seed's VERSION.
bumped() {
    local major minor patch
    IFS='.' read -r major minor patch <<< "$SEED_VERSION"
    case "$1" in
        major) echo "$((major + 1)).0.0" ;;
        minor) echo "$major.$((minor + 1)).0" ;;
        patch) echo "$major.$minor.$((patch + 1))" ;;
    esac
}

setup_file() {
    # Minimal engine seed (no .git): release only needs the CLI entry point,
    # its helpers, the three version files, and a CHANGELOG for the tag body.
    # Copying the whole repo and carrying its .git made each per-test clone race
    # under `bats --jobs` — the copied index disagreed with the freshly-written
    # working tree, so release's clean-tree check tripped intermittently.
    TEST_SEED="$(mktemp -d "${TMPDIR:-/tmp}/agentsync_release_seed.XXXXXX")"
    export TEST_SEED

    cp -R "$REPO_ROOT/bin" "$TEST_SEED/bin"
    cp -R "$REPO_ROOT/lib" "$TEST_SEED/lib"
    # The seed's dispatcher hands its VERSION to the native binary, which refuses
    # any other, so the seed starts from the repository's version.
    cp "$REPO_ROOT/VERSION" "$TEST_SEED/VERSION"
    read -r SEED_VERSION < "$TEST_SEED/VERSION"
    export SEED_VERSION
    printf '[package]\nname = "agentsync"\nversion = "%s"\nedition = "2024"\n\n[dependencies]\nclap = { version = "4.6", features = ["derive"] }\n' \
        "$SEED_VERSION" > "$TEST_SEED/Cargo.toml"
    printf '# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = "agentsync"\nversion = "%s"\ndependencies = [\n "clap",\n]\n\n[[package]]\nname = "clap"\nversion = "4.6.0"\n' \
        "$SEED_VERSION" > "$TEST_SEED/Cargo.lock"
    printf '# Changelog\n\n## %s\n\nA patch.\n\n- one fix\n\n## %s\n\nThe seed.\n' \
        "$(bumped patch)" "$SEED_VERSION" > "$TEST_SEED/CHANGELOG.md"
}

teardown_file() { teardown_seed_project; }

setup() {
    # Copy the static seed, then git-init fresh so the working tree is
    # deterministically clean: index stat info matches the just-written files,
    # leaving no room for a stale-index false positive in release's clean check.
    TEST_PROJECT="$(mktemp -d "${TMPDIR:-/tmp}/agentsync_clone.XXXXXX")"
    rmdir "$TEST_PROJECT"
    cp -c -R "$TEST_SEED" "$TEST_PROJECT" 2>/dev/null \
        || { rm -rf "$TEST_PROJECT"; cp -R "$TEST_SEED" "$TEST_PROJECT"; }
    cd "$TEST_PROJECT"
    git init --quiet
    git symbolic-ref HEAD refs/heads/main
    git config user.email "test@test.com"
    git config user.name "Test"
    git add -A
    git commit -m "seed" --quiet
    export AGENTSYNC_HOME="$TEST_PROJECT"
    # The seed has no target/, so a native run names the repository's binary.
    if [[ -z "${AGENTSYNC_NATIVE_BIN:-}" ]]; then
        local candidate
        for candidate in "$REPO_ROOT/target/release/agentsync" "$REPO_ROOT/target/release/agentsync.exe"; do
            if [[ -x "$candidate" ]]; then
                export AGENTSYNC_NATIVE_BIN="$candidate"
                break
            fi
        done
    fi
}

teardown() {
    _rm_rf_resilient "$TEST_PROJECT.remote.git"
    teardown_test_project
}

@test "release patch bumps version" {
    echo "y" | bash bin/agentsync.sh release patch --no-push
    local version
    read -r version < VERSION
    [ "$version" = "$(bumped patch)" ]
}

@test "release minor bumps version" {
    echo "y" | bash bin/agentsync.sh release minor --no-push
    local version
    read -r version < VERSION
    [ "$version" = "$(bumped minor)" ]
}

@test "release major bumps version" {
    echo "y" | bash bin/agentsync.sh release major --no-push
    local version
    read -r version < VERSION
    [ "$version" = "$(bumped major)" ]
}

@test "release bumps Cargo.toml and Cargo.lock with VERSION" {
    echo "y" | bash bin/agentsync.sh release minor --no-push
    grep -qx "version = \"$(bumped minor)\"" Cargo.toml
    [ "$(grep -c '^version = ' Cargo.toml)" -eq 1 ]
    grep -q 'clap = { version = "4.6"' Cargo.toml
    awk '/^name = "agentsync"$/ { hit = 1; next } hit && /^version = / { print; exit }' Cargo.lock \
        | grep -qx "version = \"$(bumped minor)\""
    grep -qx 'version = 4' Cargo.lock
    grep -qx 'version = "4.6.0"' Cargo.lock
}

@test "release creates git tag" {
    echo "y" | bash bin/agentsync.sh release patch --no-push
    git tag -l | grep -qx "$(bumped patch)"
}

@test "release tags with the CHANGELOG section of the new version" {
    echo "y" | bash bin/agentsync.sh release patch --no-push
    local message
    message="$(git tag -l --format='%(contents)' "$(bumped patch)")"
    [ "$message" = "v$(bumped patch)

A patch.

- one fix" ]
}

@test "release creates commit" {
    echo "y" | bash bin/agentsync.sh release patch --no-push
    git log --oneline -1 | grep -q "release: v$(bumped patch)"
}

@test "release commits VERSION, Cargo.toml, and Cargo.lock together" {
    echo "y" | bash bin/agentsync.sh release patch --no-push
    [ "$(git show --format= --name-only HEAD | tr '\n' ' ')" = "Cargo.lock Cargo.toml VERSION " ]
    [ -z "$(git status --porcelain)" ]
}

@test "release default is patch" {
    echo "y" | bash bin/agentsync.sh release --no-push
    local version
    read -r version < VERSION
    [ "$version" = "$(bumped patch)" ]
}

@test "release fails on dirty working tree" {
    echo "uncommitted" > dirty_file.txt
    run bash -c 'echo "y" | bash bin/agentsync.sh release patch --no-push'
    [ "$status" -eq 1 ]
    [[ "$output" == *"not clean"* ]]
}

@test "release fails with unknown bump type" {
    run bash -c 'echo "y" | bash bin/agentsync.sh release banana'
    [ "$status" -eq 1 ]
    [[ "$output" == *"Unknown bump type"* ]]
}

@test "release fails when Cargo.lock has no agentsync entry and writes nothing" {
    printf 'version = 4\n' > Cargo.lock
    git commit -qam "lock without agentsync"
    run bash -c 'echo "y" | bash bin/agentsync.sh release patch --no-push'
    [ "$status" -eq 1 ]
    [[ "$output" == *"Cannot find the agentsync crate version in Cargo.lock"* ]]
    [[ "$output" != *"Continue?"* ]]
    local version
    read -r version < VERSION
    [ "$version" = "$SEED_VERSION" ]
    [ -z "$(git status --porcelain)" ]
}

@test "release fails without Cargo.toml" {
    git rm -q Cargo.toml
    git commit -qm "no manifest"
    run bash -c 'echo "y" | bash bin/agentsync.sh release patch --no-push'
    [ "$status" -eq 1 ]
    [[ "$output" == *"Cannot find the agentsync crate version in Cargo.toml"* ]]
}

@test "release can be cancelled" {
    echo "n" | bash bin/agentsync.sh release patch --no-push
    local version
    read -r version < VERSION
    [ "$version" = "$SEED_VERSION" ]
}

@test "release exits 1 at end of input on the prompt" {
    run bash bin/agentsync.sh release patch --no-push < /dev/null
    [ "$status" -eq 1 ]
    [[ "$output" == *"Continue? [Y/n]: " ]]
    local version
    read -r version < VERSION
    [ "$version" = "$SEED_VERSION" ]
}

@test "release --no-push does not push" {
    run bash -c 'echo "y" | bash bin/agentsync.sh release patch --no-push'
    [ "$status" -eq 0 ]
    [[ "$output" == *"local only"* ]]
    [[ "$output" == *"--no-push"* ]]
}

@test "release pushes main and the tag to origin" {
    git init --bare --quiet "$TEST_PROJECT.remote.git"
    git remote add origin "$TEST_PROJECT.remote.git"
    run bash -c 'echo "y" | bash bin/agentsync.sh release patch'
    [ "$status" -eq 0 ]
    [[ "$output" == *"Pushing to origin..."* ]]
    [[ "$output" == *"Released v$(bumped patch)!"* ]]
    [ "$(git -C "$TEST_PROJECT.remote.git" log --format=%s -1 main)" = "release: v$(bumped patch)" ]
    git -C "$TEST_PROJECT.remote.git" tag -l | grep -qx "$(bumped patch)"
}

@test "release without an origin stops at the push with git's status" {
    run bash -c 'echo "y" | bash bin/agentsync.sh release patch'
    [ "$status" -eq 128 ]
    [[ "$output" == *"Pushing to origin..."* ]]
    [[ "$output" != *"Released"* ]]
    git tag -l | grep -qx "$(bumped patch)"
}
```

Run: `bats --tap tests/release.bats 2>&1 | grep '^not ok'`
Expected, exactly these four:

```text
not ok 4 release bumps Cargo.toml and Cargo.lock with VERSION
not ok 8 release commits VERSION, Cargo.toml, and Cargo.lock together
not ok 12 release fails when Cargo.lock has no agentsync entry and writes nothing
not ok 13 release fails without Cargo.toml
```

- [x] **Step 2: Write the implementation**

Replace `lib/helpers/release.sh` with:

```bash
#!/usr/bin/env bash
# agentsync release — bump version, tag, and push.

# Print the crate version: the first `version = "…"` line after
# `name = "agentsync"`, which names the crate in Cargo.toml and its entry in
# Cargo.lock alike. Status 1 when the file has none.
_release_crate_version() {
    awk '
        $0 == "name = \"agentsync\"" { hit = 1; next }
        hit && /^version = "/ { sub(/^version = "/, ""); sub(/"$/, ""); print; found = 1; exit }
        END { exit !found }' "$1"
}

# Rewrite that line with the new version, staging beside the file.
_release_set_crate_version() {
    local file="$1" new_version="$2" tmp
    tmp="$(tmp_sibling "$file")" || return 1
    awk -v ver="$new_version" '
        $0 == "name = \"agentsync\"" { hit = 1 }
        hit && /^version = "/ { $0 = "version = \"" ver "\""; hit = 0 }
        { print }' "$file" > "$tmp" && mv "$tmp" "$file"
}

cmd_release() {
    local bump_type="patch"
    local skip_push=false

    # Parse args
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --no-push) skip_push=true; shift ;;
            major|minor|patch) bump_type="$1"; shift ;;
            *)
                echo "$(_red "Error"): Unknown bump type: $1" >&2
                echo "  Usage: agentsync release [major|minor|patch] [--no-push]" >&2
                exit 1
                ;;
        esac
    done

    local install_dir
    install_dir=$(resolve_install_dir 2>/dev/null) || install_dir=""

    # Must be run from the agentsync repo itself
    local repo_dir=""
    if [[ -f "VERSION" ]] && [[ -f "bin/agentsync.sh" ]]; then
        repo_dir="$(pwd)"
    elif [[ -n "$install_dir" ]] && [[ -f "$install_dir/VERSION" ]]; then
        repo_dir="$install_dir"
    else
        echo "$(_red "Error"): Must be run from the AgentSync repository." >&2
        exit 1
    fi

    cd "$repo_dir" || exit 1

    # Check for clean working tree
    if [[ -n "$(git status --porcelain 2>/dev/null)" ]]; then
        echo "$(_red "Error"): Working tree is not clean. Commit or stash changes first." >&2
        exit 1
    fi

    # Read current version
    local current_version=""
    read -r current_version < VERSION || true

    # Parse semver
    local major minor patch
    IFS='.' read -r major minor patch <<< "$current_version"

    if [[ -z "$major" ]] || [[ -z "$minor" ]] || [[ -z "$patch" ]]; then
        echo "$(_red "Error"): Cannot parse VERSION: $current_version" >&2
        exit 1
    fi

    # Cargo.toml and Cargo.lock carry VERSION; checked before anything is written
    local crate_file
    for crate_file in Cargo.toml Cargo.lock; do
        if ! _release_crate_version "$crate_file" >/dev/null 2>&1; then
            echo "$(_red "Error"): Cannot find the agentsync crate version in $crate_file" >&2
            exit 1
        fi
    done

    # Bump
    case "$bump_type" in
        major)
            major=$((major + 1))
            minor=0
            patch=0
            ;;
        minor)
            minor=$((minor + 1))
            patch=0
            ;;
        patch)
            patch=$((patch + 1))
            ;;
    esac

    local new_version="$major.$minor.$patch"

    echo ""
    _bold "  AgentSync Release"; echo ""
    echo ""
    echo "  $(_dim "$current_version") → $(_green "$new_version") ($bump_type)"
    echo ""

    # Confirm
    printf "  %s " "$(_green "▸") Continue? [Y/n]:"
    read -r confirm
    if [[ "$confirm" =~ ^[Nn] ]]; then
        echo "  Cancelled."
        return 0
    fi

    # Update VERSION, Cargo.toml, Cargo.lock
    echo "$new_version" > VERSION
    echo "  Updated $(_cyan "VERSION") → $new_version"
    for crate_file in Cargo.toml Cargo.lock; do
        _release_set_crate_version "$crate_file" "$new_version"
        echo "  Updated $(_cyan "$crate_file") → $new_version"
    done

    # Commit + tag
    git add VERSION Cargo.toml Cargo.lock
    git commit -m "release: v$new_version" --quiet
    echo "  Created commit: $(_dim "release: v$new_version")"

    # Build annotated tag message from CHANGELOG.md
    local tag_body
    tag_body=$(awk -v ver="## ${new_version}" \
        '$0 == ver {found=1; next} found && /^## /{exit} found' CHANGELOG.md)
    local tag_msg_file
    tag_msg_file="$(tmp_file agentsync_tag_msg)"
    printf 'v%s\n\n%s' "$new_version" "$tag_body" > "$tag_msg_file"
    git tag -a "$new_version" -F "$tag_msg_file"
    rm -f "$tag_msg_file"
    echo "  Created tag: $(_cyan "$new_version")"

    # Push
    if [[ "$skip_push" == "true" ]]; then
        echo ""
        echo "  $(_green "Released v$new_version!") (local only, --no-push)"
        echo ""
        echo "  Push manually:"
        echo "    git push origin main && git push origin $new_version"
        echo ""
    else
        echo ""
        echo "  Pushing to origin..."
        git push --quiet origin main
        git push --quiet origin "$new_version"
        echo ""
        echo "  $(_green "Released v$new_version!")"
        echo ""
        echo "  GitHub Release will be created automatically by CI."
        echo "  Users will see the update notification on next run."
        echo ""
    fi
}
```

Run:

```bash
bats --tap tests/release.bats 2>&1 | grep -c '^not ok'
grep -c '^@test' tests/release.bats
```

Expected: `0`; `18`.

- [x] **Step 3: Lint and commit**

```bash
shellcheck -x -S warning -e SC1091 lib/helpers/release.sh bin/agentsync.sh
git add lib/helpers/release.sh tests/release.bats docs/plans/2026-09-18-rust-migration-phase-5b-release.md
git commit -m "feat(release): bump Cargo.toml and Cargo.lock with VERSION"
```

Expected: ShellCheck exit 0.

---

### Task 3: Port `release`

**Files:**
- Create: `src/cli/release.rs`
- Modify: `src/cli/mod.rs`, `src/main.rs:153`, `bin/agentsync.sh:280`, `docs/specs/2026-09-12-rust-migration-design.md` (quirk 54, three deviations)
- Test: `tests/native_parity.bats`

**Interfaces:**

```rust
// src/cli/release.rs
pub struct Env<'a> {
    pub cwd: String,                                   // the working directory, tried first as the checkout
    pub install_dir: Option<String>,                   // AGENTSYNC_HOME when it holds a .git
    pub read_line: &'a mut dyn FnMut() -> Option<String>,  // one stdin line without its newline; None at end of input
}
pub fn crate_version(text: &str) -> Option<String>;
pub fn set_crate_version(text: &str, new_version: &str) -> String;
pub fn changelog_section(changelog: &str, version: &str) -> String;
pub fn release(args: &[String], style: &Style, env: &mut Env, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error>;
```

```bash
# tests/native_parity.bats
[RELEASE_PREPARE=<function>] [RELEASE_CWD=elsewhere] assert_release_parity <answer|-> <release args...>
```

- Consumes: Task 2's messages and write order; `Style`, `put`, `paths::logical_root`.

- [x] **Step 1: Parity fixtures, failing against a binary without the port**

Append to `tests/native_parity.bats`:

```bash

# ── release ──────────────────────────────────────────────────────────────────
# release rewrites the checkout it runs in and reads its answer from stdin, so
# each engine gets a fresh fixture checkout with a bare origin beside it, and
# the checkout's state after the run is compared along with the output.

# Usage: _release_checkout <dir>: a 1.0.0 checkout release accepts, on main.
_release_checkout() {
    local dir="$1"
    rm -rf "$dir" "$dir.remote.git"
    mkdir -p "$dir/bin"
    : > "$dir/bin/agentsync.sh"
    printf '1.0.0\n' > "$dir/VERSION"
    printf '[package]\nname = "agentsync"\n# The crate version follows VERSION.\nversion = "1.0.0"\nedition = "2024"\n\n[dependencies]\nclap = { version = "4.6", features = ["derive"] }\n' > "$dir/Cargo.toml"
    printf '# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = "agentsync"\nversion = "1.0.0"\ndependencies = [\n "clap",\n]\n\n[[package]]\nname = "clap"\nversion = "4.6.0"\n' > "$dir/Cargo.lock"
    printf '# Changelog\n\n## 1.0.1\n\nA patch.\n\n- one fix\n\n## 1.0.0\n\nFirst.\n' > "$dir/CHANGELOG.md"
    git -C "$dir" init --quiet
    git -C "$dir" symbolic-ref HEAD refs/heads/main
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    git -C "$dir" add -A
    git -C "$dir" commit -m "seed" --quiet
    git init --bare --quiet "$dir.remote.git"
    git -C "$dir" remote add origin "$dir.remote.git"
}

# The checkout after a run: the three version files, commits, tags with their
# messages, the tree's cleanliness, and what reached origin.
_release_state() {
    local dir="$1"
    cat "$dir/VERSION" "$dir/Cargo.toml" "$dir/Cargo.lock" 2>/dev/null
    git -C "$dir" log --format=%s 2>/dev/null
    git -C "$dir" tag -l -n99 2>/dev/null
    git -C "$dir" status --porcelain 2>/dev/null
    git -C "$dir.remote.git" log --format=%s main 2>/dev/null
    git -C "$dir.remote.git" tag -l 2>/dev/null
}

_release_dirty() { echo x > dirty.txt; }
_release_two_parts() { printf '1.2\n' > VERSION; git commit -qam "two parts"; }
_release_no_lock_entry() { printf 'version = 4\n' > Cargo.lock; git commit -qam "lock"; }
_release_no_toml() { git rm -q Cargo.toml; git commit -qm "toml"; }
_release_no_version() { git rm -q VERSION; git commit -qm "version"; }
_release_no_origin() { git remote remove origin; }
_release_no_git() { rm -rf .git; }

# Usage: [RELEASE_PREPARE=<function>] [RELEASE_CWD=elsewhere] assert_release_parity <answer|-> <release args...>
# `-` closes stdin. RELEASE_PREPARE runs inside the fresh checkout first;
# RELEASE_CWD=elsewhere runs from an empty directory with the checkout as
# AGENTSYNC_HOME, the fallback resolve_install_dir takes.
assert_release_parity() {
    local answer="$1"
    shift
    local base="$BATS_TEST_TMPDIR/release" mode rc cwd home
    local -a rcs=() outs=() states=()
    for mode in 0 1; do
        _release_checkout "$base/$mode"
        if [[ -n "${RELEASE_PREPARE:-}" ]]; then
            (cd "$base/$mode" && "$RELEASE_PREPARE")
        fi
        cwd="$base/$mode"
        home="$BATS_TEST_TMPDIR/nohome"
        if [[ "${RELEASE_CWD:-}" == "elsewhere" ]]; then
            cwd="$base/elsewhere-$mode"
            home="$base/$mode"
        fi
        mkdir -p "$cwd" "$home"
        rc=0
        if [[ "$answer" == "-" ]]; then
            outs[mode]=$(cd "$cwd" && AGENTSYNC_NATIVE="$mode" AGENTSYNC_HOME="$home" bash "$AGENTSYNC_BIN" release "$@" 2>&1 < /dev/null) || rc=$?
        else
            outs[mode]=$(cd "$cwd" && printf '%s\n' "$answer" | AGENTSYNC_NATIVE="$mode" AGENTSYNC_HOME="$home" bash "$AGENTSYNC_BIN" release "$@" 2>&1) || rc=$?
        fi
        rcs[mode]=$rc
        states[mode]=$(_release_state "$base/$mode")
    done
    if [[ "${rcs[0]}" -ne "${rcs[1]}" ]]; then
        echo "exit status differs for [release $*]: bash=${rcs[0]} native=${rcs[1]}" >&2
        printf '%s\n' "${outs[1]}" >&2
        return 1
    fi
    if [[ "${outs[0]}" != "${outs[1]}" ]]; then
        echo "output differs for [release $*]" >&2
        diff <(printf '%s\n' "${outs[0]}") <(printf '%s\n' "${outs[1]}") >&2 || true
        return 1
    fi
    if [[ "${states[0]}" != "${states[1]}" ]]; then
        echo "checkout differs after [release $*]" >&2
        diff <(printf '%s\n' "${states[0]}") <(printf '%s\n' "${states[1]}") >&2 || true
        return 1
    fi
}

@test "parity: release bumps, commits, tags, and pushes like Bash" {
    assert_release_parity y patch --no-push
    assert_release_parity "" major --no-push
    assert_release_parity yes minor --no-push
    assert_release_parity y patch
    RELEASE_CWD=elsewhere assert_release_parity y patch --no-push
}

@test "parity: release cancels, refuses, and stops where Bash does" {
    assert_release_parity n --no-push
    assert_release_parity " nope" --no-push
    assert_release_parity - patch --no-push
    assert_release_parity - banana
    assert_release_parity - patch extra
    RELEASE_PREPARE=_release_dirty assert_release_parity y patch --no-push
    RELEASE_PREPARE=_release_two_parts assert_release_parity y patch --no-push
    RELEASE_PREPARE=_release_no_lock_entry assert_release_parity y patch --no-push
    RELEASE_PREPARE=_release_no_toml assert_release_parity y patch --no-push
    RELEASE_PREPARE=_release_no_origin assert_release_parity y patch
    RELEASE_PREPARE=_release_no_git assert_release_parity y patch --no-push
    RELEASE_CWD=elsewhere RELEASE_PREPARE=_release_no_version assert_release_parity y patch --no-push
}
```

In `bin/agentsync.sh:280` append `release`:

```bash
_NATIVE_COMMANDS=" version --version -v list ls check sync rollback enable disable customize show diff simplify resolve upgrade-config profile dedupe adopt migrate refresh init doctor add export import generate gen shell-init setup-hooks help --help -h release "
```

Run:

```bash
cargo build --release
bats --tap -f 'parity: release' tests/native_parity.bats 2>&1 | grep -E '^(ok|not ok)|exit status differs'
AGENTSYNC_NATIVE=1 bats --tap tests/release.bats 2>&1 | grep -c '^not ok'
```

Expected: `not ok 1` with `exit status differs for [release patch --no-push]: bash=0 native=1` and `not ok 2` with `exit status differs for [release --no-push]: bash=0 native=1`, because the binary refuses `release` as an unknown command; `16` of the 18 native `release.bats` cases fail (the two asserting status 1 pass by coincidence).

- [x] **Step 2: Write the failing tests**

Add `pub mod release;` to `src/cli/mod.rs` between `pub mod refresh;` and `pub mod resolve;`. Create `src/cli/release.rs` with the module comment, the imports, the fixtures, and the two test modules:

```rust
//! `agentsync release`: `lib/helpers/release.sh`, which bumps `VERSION`,
//! `Cargo.toml`, and `Cargo.lock` together, commits, tags with the changelog
//! section of the new version, and pushes `main` and the tag unless
//! `--no-push`. Git runs as the executable, its own output passing through.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use super::customize::put;
use crate::Error;
use crate::style::Style;

#[cfg(test)]
const TOML: &str = "[package]\nname = \"agentsync\"\n# The crate version follows VERSION.\nversion = \"1.0.0\"\nedition = \"2024\"\n\n[dependencies]\nclap = { version = \"4.6\", features = [\"derive\"] }\n";
#[cfg(test)]
const LOCK: &str = "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"agentsync\"\nversion = \"1.0.0\"\ndependencies = [\n \"clap\",\n]\n\n[[package]]\nname = \"clap\"\nversion = \"4.6.0\"\n";
#[cfg(test)]
const CHANGELOG: &str = "# Changelog\n\n## 1.0.1\n\nA patch.\n\n- one fix\n\n## 1.0.0\n\nFirst.\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_bumps_read_like_cmd_release() {
        assert_eq!(current_version("1.0.0\n"), "1.0.0");
        assert_eq!(current_version("  1.2.3 \nsecond\n"), "1.2.3");
        assert_eq!(current_version(""), "");
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        for bad in ["1.2", "1.a.0", "1.2.3.4", "", "1..3", "1.2.3."] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
        assert_eq!(Bump::parse("major"), Some(Bump::Major));
        assert_eq!(Bump::parse("Patch"), None);
        assert_eq!(Bump::Major.apply((1, 2, 3)), (2, 0, 0));
        assert_eq!(Bump::Minor.apply((1, 2, 3)), (1, 3, 0));
        assert_eq!(Bump::Patch.apply((1, 2, 3)), (1, 2, 4));
    }

    #[test]
    fn the_crate_version_after_the_name_is_read_and_rewritten_like_the_awk() {
        assert_eq!(crate_version(TOML).as_deref(), Some("1.0.0"));
        assert_eq!(crate_version(LOCK).as_deref(), Some("1.0.0"));
        assert_eq!(crate_version("version = 4\n"), None);
        assert_eq!(
            crate_version("version = \"1.0.0\"\nname = \"agentsync\"\n"),
            None
        );
        assert_eq!(
            set_crate_version(TOML, "1.1.0"),
            TOML.replace("version = \"1.0.0\"", "version = \"1.1.0\"")
        );
        let lock = set_crate_version(LOCK, "1.1.0");
        assert_eq!(
            lock,
            LOCK.replace("version = \"1.0.0\"", "version = \"1.1.0\"")
        );
        assert!(lock.contains("\nversion = 4\n") && lock.ends_with("version = \"4.6.0\"\n"));
        assert_eq!(set_crate_version("version = 4\n", "1.1.0"), "version = 4\n");
        assert_eq!(
            set_crate_version("name = \"agentsync\"\nversion = \"1.0.0\"", "2.0.0"),
            "name = \"agentsync\"\nversion = \"2.0.0\"\n"
        );
        assert_eq!(set_crate_version("", "2.0.0"), "");
    }

    #[test]
    fn the_changelog_section_is_cut_like_the_awk() {
        assert_eq!(
            changelog_section(CHANGELOG, "1.0.1"),
            "\nA patch.\n\n- one fix"
        );
        assert_eq!(changelog_section(CHANGELOG, "1.0.0"), "\nFirst.");
        assert_eq!(changelog_section(CHANGELOG, "1.1.0"), "");
        assert_eq!(
            changelog_section("## 2.0.0\nx\n## 2.0.0\ny\n## 1.0.0\n", "2.0.0"),
            "x\ny"
        );
        assert_eq!(
            tag_message("1.0.1", "\nA patch.\n\n- one fix"),
            "v1.0.1\n\n\nA patch.\n\n- one fix"
        );
        assert_eq!(tag_message("1.1.0", ""), "v1.1.0\n\n");
    }
}

#[cfg(all(test, unix))]
mod checkout_tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    /// A checkout `release` accepts, committed on `main`, with the developer's
    /// signing and hooks settings overridden locally.
    fn checkout(version: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/agentsync.sh"), "").unwrap();
        std::fs::write(root.join("VERSION"), format!("{version}\n")).unwrap();
        std::fs::write(root.join("Cargo.toml"), TOML.replace("1.0.0", version)).unwrap();
        std::fs::write(root.join("Cargo.lock"), LOCK.replace("1.0.0", version)).unwrap();
        std::fs::write(root.join("CHANGELOG.md"), CHANGELOG).unwrap();
        sh(&root, &["init", "-q"]);
        sh(&root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        for (key, value) in [
            ("user.email", "test@test.com"),
            ("user.name", "Test"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
            ("core.hooksPath", ".git/hooks"),
        ] {
            sh(&root, &["config", key, value]);
        }
        sh(&root, &["add", "-A"]);
        sh(&root, &["commit", "-q", "-m", "seed"]);
        (dir, root)
    }

    fn run(
        args: &[&str],
        cwd: &Path,
        install_dir: Option<&Path>,
        answer: Option<&str>,
    ) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut answer = answer.map(str::to_string);
        let mut read_line = || answer.take();
        let mut env = Env {
            cwd: cwd.to_string_lossy().into_owned(),
            install_dir: install_dir.map(|dir| dir.to_string_lossy().into_owned()),
            read_line: &mut read_line,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = release(&args, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn a_release_bumps_the_three_files_commits_and_tags_like_release_sh() {
        let (_dir, root) = checkout("1.0.0");
        let (status, out, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "\n  AgentSync Release\n\n  1.0.0 → 1.0.1 (patch)\n\n  ▸ Continue? [Y/n]:   Updated VERSION → 1.0.1\n  Updated Cargo.toml → 1.0.1\n  Updated Cargo.lock → 1.0.1\n  Created commit: release: v1.0.1\n  Created tag: 1.0.1\n\n  Released v1.0.1! (local only, --no-push)\n\n  Push manually:\n    git push origin main && git push origin 1.0.1\n\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("VERSION")).unwrap(),
            "1.0.1\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("Cargo.toml")).unwrap(),
            TOML.replace("1.0.0", "1.0.1")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("Cargo.lock")).unwrap(),
            LOCK.replace("1.0.0", "1.0.1")
        );
        assert_eq!(
            sh(&root, &["log", "--format=%s", "-2"]),
            "release: v1.0.1\nseed\n"
        );
        assert_eq!(
            sh(&root, &["show", "--format=", "--name-only", "HEAD"]),
            "Cargo.lock\nCargo.toml\nVERSION\n"
        );
        assert_eq!(
            sh(&root, &["tag", "-l", "--format=%(contents)", "1.0.1"]),
            "v1.0.1\n\nA patch.\n\n- one fix\n\n"
        );
        assert_eq!(sh(&root, &["status", "--porcelain"]), "");
        let (status, out, _) = run(&["minor", "--no-push"], &root, None, Some(""));
        assert_eq!(status, 0);
        assert!(out.contains("\n  1.0.1 → 1.1.0 (minor)\n"));
        assert_eq!(
            sh(&root, &["tag", "-l", "--format=%(contents)", "1.1.0"]),
            "v1.1.0\n\n"
        );
        let (status, out, _) = run(&["major", "--no-push"], &root, None, Some("yes"));
        assert_eq!(status, 0);
        assert!(out.contains("\n  1.1.0 → 2.0.0 (major)\n"));
    }

    #[test]
    fn the_prompt_cancels_on_n_and_ends_at_end_of_input_like_bash_does() {
        let (_dir, root) = checkout("1.0.0");
        let (status, out, err) = run(&["--no-push"], &root, None, Some(" nope"));
        assert_eq!((status, err.as_str()), (0, ""));
        assert!(out.ends_with("\n  ▸ Continue? [Y/n]:   Cancelled.\n"));
        // Known quirk 54: `read -r` fails at end of input and errexit ends the run.
        let (status, out, err) = run(&["patch", "--no-push"], &root, None, None);
        assert_eq!((status, err.as_str()), (1, ""));
        assert!(out.ends_with("\n  ▸ Continue? [Y/n]: "));
        assert_eq!(
            std::fs::read_to_string(root.join("VERSION")).unwrap(),
            "1.0.0\n"
        );
        assert_eq!(sh(&root, &["log", "--format=%s"]), "seed\n");
    }

    #[test]
    fn refusals_leave_the_checkout_untouched_like_release_sh() {
        let (_dir, root) = checkout("1.0.0");
        let (status, out, err) = run(&["banana"], &root, None, Some("y"));
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "Error: Unknown bump type: banana\n  Usage: agentsync release [major|minor|patch] [--no-push]\n"
        );
        let (_, _, err) = run(&["patch", "extra"], &root, None, Some("y"));
        assert_eq!(
            err,
            "Error: Unknown bump type: extra\n  Usage: agentsync release [major|minor|patch] [--no-push]\n"
        );
        std::fs::write(root.join("dirty.txt"), "x\n").unwrap();
        let (status, out, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "Error: Working tree is not clean. Commit or stash changes first.\n"
        );
        std::fs::remove_file(root.join("dirty.txt")).unwrap();
        std::fs::write(root.join("VERSION"), "1.2\n").unwrap();
        sh(&root, &["commit", "-q", "-am", "two parts"]);
        let (status, _, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!(
            (status, err.as_str()),
            (1, "Error: Cannot parse VERSION: 1.2\n")
        );
        std::fs::write(root.join("VERSION"), "1.a.0\n").unwrap();
        sh(&root, &["commit", "-q", "-am", "alpha"]);
        let (status, _, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!(
            (status, err.as_str()),
            (1, "Error: Cannot parse VERSION: 1.a.0\n")
        );
        std::fs::write(root.join("VERSION"), "1.0.0\n").unwrap();
        std::fs::write(root.join("Cargo.lock"), "version = 4\n").unwrap();
        sh(&root, &["commit", "-q", "-am", "lock"]);
        let (status, out, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "Error: Cannot find the agentsync crate version in Cargo.lock\n"
        );
        std::fs::remove_file(root.join("Cargo.toml")).unwrap();
        sh(&root, &["commit", "-q", "-am", "toml"]);
        let (_, _, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!(
            err,
            "Error: Cannot find the agentsync crate version in Cargo.toml\n"
        );
        assert_eq!(sh(&root, &["status", "--porcelain"]), "");
        assert_eq!(sh(&root, &["tag", "-l"]), "");
    }

    #[test]
    fn the_checkout_comes_from_agentsync_home_when_cwd_is_none_like_resolve_install_dir() {
        let (_dir, root) = checkout("1.0.0");
        let elsewhere = tempfile::tempdir().unwrap();
        let (status, _, err) = run(&["patch", "--no-push"], elsewhere.path(), None, Some("y"));
        assert_eq!(
            (status, err.as_str()),
            (1, "Error: Must be run from the AgentSync repository.\n")
        );
        let (status, out, err) = run(
            &["patch", "--no-push"],
            elsewhere.path(),
            Some(&root),
            Some("y"),
        );
        assert_eq!((status, err.as_str()), (0, ""));
        assert!(out.contains("  Created tag: 1.0.1\n"));
        assert_eq!(
            sh(&root, &["log", "--format=%s", "-1"]),
            "release: v1.0.1\n"
        );
        std::fs::remove_file(root.join("VERSION")).unwrap();
        sh(&root, &["commit", "-q", "-am", "no version"]);
        let (status, _, err) = run(
            &["patch", "--no-push"],
            elsewhere.path(),
            Some(&root),
            Some("y"),
        );
        assert_eq!(
            (status, err.as_str()),
            (1, "Error: Must be run from the AgentSync repository.\n")
        );
    }

    #[test]
    fn the_push_reaches_origin_like_release_sh() {
        let (_dir, root) = checkout("1.0.0");
        let remote = tempfile::tempdir().unwrap();
        sh(remote.path(), &["init", "-q", "--bare"]);
        sh(
            &root,
            &["remote", "add", "origin", &remote.path().to_string_lossy()],
        );
        let (status, out, err) = run(&["patch"], &root, None, Some("y"));
        assert_eq!((status, err.as_str()), (0, ""));
        assert!(out.ends_with(
            "  Created tag: 1.0.1\n\n  Pushing to origin...\n\n  Released v1.0.1!\n\n  GitHub Release will be created automatically by CI.\n  Users will see the update notification on next run.\n\n"
        ));
        assert_eq!(
            sh(remote.path(), &["log", "--format=%s", "-1", "main"]),
            "release: v1.0.1\n"
        );
        assert_eq!(sh(remote.path(), &["tag", "-l"]), "1.0.1\n");
    }
}
```

Run: `cargo test cli::release 2>&1 | grep -E '^error' | sort | uniq -c | sort -rn | head -2`
Expected:

```text
   6 error[E0433]: cannot find type `Bump` in this scope
   5 error[E0425]: cannot find function `set_crate_version` in this scope
```

- [x] **Step 3: Write the implementation**

Insert into `src/cli/release.rs`, between `use crate::style::Style;` and the `#[cfg(test)]` fixtures:

```rust

const USAGE: &str = "  Usage: agentsync release [major|minor|patch] [--no-push]";
const CRATE_FILES: [&str; 2] = ["Cargo.toml", "Cargo.lock"];
const NAME_LINE: &str = "name = \"agentsync\"";

/// What `release` takes from the process.
pub struct Env<'a> {
    /// The working directory, tried first as the checkout.
    pub cwd: String,
    /// `resolve_install_dir`: `AGENTSYNC_HOME` when it holds a `.git`.
    pub install_dir: Option<String>,
    /// The next line of stdin without its newline; `None` at end of input.
    pub read_line: &'a mut dyn FnMut() -> Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bump {
    Major,
    Minor,
    Patch,
}

impl Bump {
    fn parse(word: &str) -> Option<Self> {
        match word {
            "major" => Some(Self::Major),
            "minor" => Some(Self::Minor),
            "patch" => Some(Self::Patch),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Patch => "patch",
        }
    }

    fn apply(self, (major, minor, patch): (u64, u64, u64)) -> (u64, u64, u64) {
        match self {
            Self::Major => (major + 1, 0, 0),
            Self::Minor => (major, minor + 1, 0),
            Self::Patch => (major, minor, patch + 1),
        }
    }
}

/// `read -r current_version < VERSION`: the first line, IFS whitespace trimmed.
fn current_version(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim_matches([' ', '\t'])
}

/// `IFS='.' read -r major minor patch`, the three parts read as decimal
/// integers (design spec, "Accepted deviations", Phase 5b).
fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let mut next = || parts.next().and_then(|part| part.parse::<u64>().ok());
    let parsed = (next()?, next()?, next()?);
    parts.next().is_none().then_some(parsed)
}

/// The records awk reads: every line, a final one without a newline included.
fn records(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .collect()
}

/// `_release_crate_version`: the first `version = "…"` line after
/// `name = "agentsync"`, which names the crate in Cargo.toml and its entry in
/// Cargo.lock alike.
pub fn crate_version(text: &str) -> Option<String> {
    let mut hit = false;
    for line in records(text) {
        if line == NAME_LINE {
            hit = true;
            continue;
        }
        if hit && line.starts_with("version = \"") {
            let value = &line["version = \"".len()..];
            return Some(value.strip_suffix('"').unwrap_or(value).to_string());
        }
    }
    None
}

/// `_release_set_crate_version`: that line rewritten, every record printed
/// with a newline as awk prints it.
pub fn set_crate_version(text: &str, new_version: &str) -> String {
    let mut hit = false;
    let mut out = String::with_capacity(text.len());
    for line in records(text) {
        if line == NAME_LINE {
            hit = true;
        }
        if hit && line.starts_with("version = \"") {
            out.push_str(&format!("version = \"{new_version}\"\n"));
            hit = false;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The `## <version>` section of `CHANGELOG.md`: the lines after the heading
/// up to the next `## `, trailing newlines dropped as `$(...)` drops them.
pub fn changelog_section(changelog: &str, version: &str) -> String {
    let heading = format!("## {version}");
    let mut found = false;
    let mut body = String::new();
    for line in records(changelog) {
        if line == heading {
            found = true;
            continue;
        }
        if !found {
            continue;
        }
        if line.starts_with("## ") {
            break;
        }
        body.push_str(line);
        body.push('\n');
    }
    body.trim_end_matches('\n').to_string()
}

/// `printf 'v%s\n\n%s'`.
fn tag_message(version: &str, body: &str) -> String {
    format!("v{version}\n\n{body}")
}

/// The status the shell reports for a child: its exit code, or 128 plus the
/// signal that ended it.
fn shell_status(status: ExitStatus) -> u8 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128u8.wrapping_add(signal as u8);
        }
    }
    status.code().unwrap_or(1) as u8
}

/// `[[ -n "$(git status --porcelain 2>/dev/null)" ]]`.
fn tree_is_dirty(repo: &Path) -> Result<bool, Error> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["status", "--porcelain"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| Error::io("git", e))?;
    Ok(output.stdout.iter().any(|b| *b != b'\n'))
}

/// `git <args>` in `repo` on the process's streams, as the shell ran it, once
/// everything printed so far has reached them; `message` goes to its stdin.
fn git(
    repo: &Path,
    args: &[&str],
    message: Option<&str>,
    out: &mut dyn Write,
) -> Result<ExitStatus, Error> {
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    let mut command = Command::new("git");
    command.current_dir(repo).args(args);
    let Some(message) = message else {
        command.stdin(Stdio::null());
        return command.status().map_err(|e| Error::io("git", e));
    };
    command.stdin(Stdio::piped());
    let mut child = command.spawn().map_err(|e| Error::io("git", e))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(message.as_bytes())
            .map_err(|e| Error::io("git", e))?;
    }
    child.wait().map_err(|e| Error::io("git", e))
}

fn refuse(err: &mut dyn Write, style: &Style, message: &str) -> Result<u8, Error> {
    put(
        err,
        format!("{}: {message}\n", style.red("Error")).as_bytes(),
    )?;
    Ok(1)
}

/// `cmd_release`.
pub fn release(
    args: &[String],
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut bump = Bump::Patch;
    let mut skip_push = false;
    for arg in args {
        match (arg.as_str(), Bump::parse(arg)) {
            ("--no-push", _) => skip_push = true,
            (_, Some(named)) => bump = named,
            (other, None) => {
                return refuse(err, style, &format!("Unknown bump type: {other}\n{USAGE}"));
            }
        }
    }
    let cwd = Path::new(&env.cwd);
    let install_dir = env
        .install_dir
        .as_deref()
        .filter(|dir| Path::new(dir).join("VERSION").is_file());
    let repo: PathBuf = if cwd.join("VERSION").is_file() && cwd.join("bin/agentsync.sh").is_file() {
        cwd.to_path_buf()
    } else if let Some(dir) = install_dir {
        PathBuf::from(dir)
    } else {
        return refuse(err, style, "Must be run from the AgentSync repository.");
    };
    if tree_is_dirty(&repo)? {
        return refuse(
            err,
            style,
            "Working tree is not clean. Commit or stash changes first.",
        );
    }
    let version_file = repo.join("VERSION");
    let version_text =
        std::fs::read_to_string(&version_file).map_err(|e| Error::io(&version_file, e))?;
    let current = current_version(&version_text);
    let Some(parts) = parse_version(current) else {
        return refuse(err, style, &format!("Cannot parse VERSION: {current}"));
    };
    let mut crate_texts = Vec::new();
    for name in CRATE_FILES {
        let text = std::fs::read_to_string(repo.join(name))
            .ok()
            .filter(|text| crate_version(text).is_some());
        let Some(text) = text else {
            return refuse(
                err,
                style,
                &format!("Cannot find the agentsync crate version in {name}"),
            );
        };
        crate_texts.push((name, text));
    }
    let (major, minor, patch) = bump.apply(parts);
    let new_version = format!("{major}.{minor}.{patch}");
    put(
        out,
        format!(
            "\n{}\n\n  {} → {} ({})\n\n  {} Continue? [Y/n]: ",
            style.bold("  AgentSync Release"),
            style.dim(current),
            style.green(&new_version),
            bump.name(),
            style.green("▸")
        )
        .as_bytes(),
    )?;
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    // `read -r confirm` fails at end of input and errexit ends the run there
    // (design spec, "Known quirks", item 54).
    let Some(answer) = (env.read_line)() else {
        return Ok(1);
    };
    if answer.trim_matches([' ', '\t']).starts_with(['n', 'N']) {
        put(out, b"  Cancelled.\n")?;
        return Ok(0);
    }
    std::fs::write(&version_file, format!("{new_version}\n"))
        .map_err(|e| Error::io(&version_file, e))?;
    put(
        out,
        format!("  Updated {} → {new_version}\n", style.cyan("VERSION")).as_bytes(),
    )?;
    for (name, text) in &crate_texts {
        let path = repo.join(name);
        std::fs::write(&path, set_crate_version(text, &new_version))
            .map_err(|e| Error::io(&path, e))?;
        put(
            out,
            format!("  Updated {} → {new_version}\n", style.cyan(name)).as_bytes(),
        )?;
    }
    let subject = format!("release: v{new_version}");
    for step in [
        &["add", "VERSION", "Cargo.toml", "Cargo.lock"][..],
        &["commit", "-m", &subject, "--quiet"][..],
    ] {
        let status = git(&repo, step, None, out)?;
        if !status.success() {
            return Ok(shell_status(status));
        }
    }
    put(
        out,
        format!("  Created commit: {}\n", style.dim(&subject)).as_bytes(),
    )?;
    let changelog_file = repo.join("CHANGELOG.md");
    let changelog =
        std::fs::read_to_string(&changelog_file).map_err(|e| Error::io(&changelog_file, e))?;
    let message = tag_message(&new_version, &changelog_section(&changelog, &new_version));
    let status = git(
        &repo,
        &["tag", "-a", &new_version, "-F", "-"],
        Some(&message),
        out,
    )?;
    if !status.success() {
        return Ok(shell_status(status));
    }
    put(
        out,
        format!("  Created tag: {}\n", style.cyan(&new_version)).as_bytes(),
    )?;
    let released = style.green(&format!("Released v{new_version}!"));
    if skip_push {
        put(
            out,
            format!(
                "\n  {released} (local only, --no-push)\n\n  Push manually:\n    git push origin main && git push origin {new_version}\n\n"
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    put(out, b"\n  Pushing to origin...\n")?;
    for refname in ["main", new_version.as_str()] {
        let status = git(&repo, &["push", "--quiet", "origin", refname], None, out)?;
        if !status.success() {
            return Ok(shell_status(status));
        }
    }
    put(
        out,
        format!(
            "\n  {released}\n\n  GitHub Release will be created automatically by CI.\n  Users will see the update notification on next run.\n\n"
        )
        .as_bytes(),
    )?;
    Ok(0)
}
```

In `src/main.rs`, right after the `setup-hooks` block (the `}` on line 153) and before `if let Some(command @ ("export" | "import"))`:

```rust
    if args.first().and_then(|a| a.to_str()) == Some("release") {
        let rest: Vec<String> = args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        let mut read_line = || {
            let mut line = String::new();
            match std::io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => None,
                Ok(_) => Some(line.trim_end_matches('\n').to_string()),
            }
        };
        let mut env = cli::release::Env {
            cwd: paths::logical_root(None, &cwd, var("PWD").as_deref()),
            install_dir: var("AGENTSYNC_HOME").filter(|home| Path::new(home).join(".git").is_dir()),
            read_line: &mut read_line,
        };
        return cli::release::release(
            &rest,
            &Style::for_stdout(),
            &mut env,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }
```

In `docs/specs/2026-09-12-rust-migration-design.md`, after quirk 53 (which ends `prints the unknown-option error, not the help.`) add:

```markdown
54. `release` exits 1 with nothing after its `Continue? [Y/n]:` prompt when
    stdin ends there: `read -r confirm` fails and errexit ends the run.
```

and after the last accepted deviation (`- Phase 4m: `shell-init`'s refusals are log lines coloured from stdout, as` / `  `_use_colors` decided, through `Log::capturing`.`) add:

```markdown
- Phase 5b: `release` requires the three `VERSION` components to be decimal
  integers and refuses others with `Cannot parse VERSION`; Bash evaluated them
  as shell arithmetic, so `1.a.0` bumped to `1.a.1` and `1.2.3.4` died with
  the shell's syntax error.
- Phase 5b: `release` reports the Rust I/O error when `git` cannot be started
  or `CHANGELOG.md` cannot be read, where Bash printed the shell's `command not
  found` (status 127) or awk's message (status 2); the tag is missing in both.
- Phase 5b: outside a checkout, `release` falls back to `AGENTSYNC_HOME` alone,
  when it holds a `.git`; Bash also tried the dispatcher's own checkout, which
  the binary has no counterpart for.
```

- [x] **Step 4: Run the tests, confirm green**

```bash
cargo fmt --all
cargo test 2>&1 | grep 'test result' | head -4
cargo build --release
printf 'release bash=%s native=%s\n' \
    "$(AGENTSYNC_NATIVE=0 bats --tap tests/release.bats | grep -c '^not ok')" \
    "$(AGENTSYNC_NATIVE=1 bats --tap tests/release.bats | grep -c '^not ok')"
bats --tap -f 'parity: release' tests/native_parity.bats 2>&1 | grep -E '^(ok|not ok)'
grep -c '^@test' tests/native_parity.bats tests/release.bats
```

Expected: `298 passed`, `0 passed`, `11 passed`, `1 passed`; `release bash=0 native=0`; `ok 1 parity: release bumps, commits, tags, and pushes like Bash`, `ok 2 parity: release cancels, refuses, and stops where Bash does`; `70` and `18` cases. The two git-spawning test modules run on unix alone (`#[cfg(all(test, unix))]`), as `setup_hooks.rs`'s do.

- [x] **Step 5: Prove the fixture bites, run the references, lint, commit**

Change `"  Updated {} → {new_version}\n", style.cyan("VERSION")` to `"  Updated {}: {new_version}\n", style.cyan("VERSION")` in `src/cli/release.rs`, `cargo build --release`, rerun `bats --tap -f 'parity: release bumps' tests/native_parity.bats`: `not ok 1` with `# output differs for [release patch --no-push]`; revert and rebuild.

Recreate the harnesses when the session scratchpad no longer holds `phase5b/`. The reference and pty scripts take `<engine 0|1> <repo root> <out file>` and run through `bin/agentsync.sh` under `AGENTSYNC_NATIVE=<engine>` from a fresh fixture checkout each time; `AGENTSYNC_HOME` names an empty directory, so `resolve_install_dir` never leaves the fixture. The pty script needs `script`, which the agent sandbox refuses (`openpty: Operation not permitted`).

`phase5b/mkfixture.sh`:

```bash
#!/usr/bin/env bash
# Usage: mkfixture.sh <dir> [<version>] [remote|noremote] [git|nogit]
# A checkout `agentsync release` accepts: VERSION, bin/agentsync.sh, Cargo.toml,
# Cargo.lock, CHANGELOG.md with a 1.0.1 section, committed on main, with a bare
# origin beside it unless `noremote`.
set -euo pipefail
dir="$1"; version="${2:-1.0.0}"; remote="${3:-remote}"; git="${4:-git}"
rm -rf "${dir:?}" "${dir:?}.remote.git"
mkdir -p "$dir/bin"
: > "$dir/bin/agentsync.sh"
printf '%s\n' "$version" > "$dir/VERSION"
printf '[package]\nname = "agentsync"\n# The crate version follows VERSION.\nversion = "%s"\nedition = "2024"\n\n[dependencies]\nclap = { version = "4.6", features = ["derive"] }\n' "$version" > "$dir/Cargo.toml"
printf '# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = "agentsync"\nversion = "%s"\ndependencies = [\n "clap",\n]\n\n[[package]]\nname = "clap"\nversion = "4.6.0"\n' "$version" > "$dir/Cargo.lock"
printf '# Changelog\n\n## 1.0.1\n\nA patch.\n\n- one fix\n\n## 1.0.0\n\nFirst.\n' > "$dir/CHANGELOG.md"
[[ "$git" == "nogit" ]] && exit 0
git -C "$dir" init --quiet
git -C "$dir" symbolic-ref HEAD refs/heads/main
git -C "$dir" config user.email "test@test.com"
git -C "$dir" config user.name "Test"
git -C "$dir" config commit.gpgsign false
git -C "$dir" config tag.gpgsign false
git -C "$dir" add -A
git -C "$dir" commit -m "seed" --quiet
if [[ "$remote" == "remote" ]]; then
    git init --bare --quiet "$dir.remote.git"
    git -C "$dir" remote add origin "$dir.remote.git"
fi
```

`phase5b/release_reference.sh`:

```bash
#!/usr/bin/env bash
# Usage: release_reference.sh <engine 0|1> <repo root> <out file>
# Runs every release situation against a fresh fixture checkout through
# bin/agentsync.sh under the given engine and records status, stdout, stderr,
# and the checkout's state apart. The fixture's origin is a bare repo beside it.
set -uo pipefail
MODE="$1"; REPO="$2"; OUT="$3"
S="$(cd "$(dirname "$0")" && pwd)"
WORK="$S/ref_$MODE"
rm -rf "${WORK:?}"
mkdir -p "$WORK/nohome" "$WORK/elsewhere"
: > "$OUT"
unset AGENTSYNC_REPO_ROOT AGENTSYNC_CONFIG_PATH AGENTSYNC_ENGINE_VERSION NO_COLOR
FIX="$WORK/fixture"

engine() {
    AGENTSYNC_NATIVE="$MODE" AGENTSYNC_HOME="${HOME_DIR:-$WORK/nohome}" bash "$REPO/bin/agentsync.sh" "$@"
}

state() {
    local dir="$1"
    printf -- '--- state\n'
    for f in VERSION Cargo.toml Cargo.lock; do
        printf '%s: ' "$f"; cat "$dir/$f" 2>/dev/null | tr '\n' '|'; printf '\n'
    done
    printf 'log: '; git -C "$dir" log --format=%s -3 2>/dev/null | tr '\n' '|'; printf '\n'
    printf 'tags: '; git -C "$dir" tag -l -n99 2>/dev/null | tr '\n' '|'; printf '\n'
    printf 'porcelain: '; git -C "$dir" status --porcelain 2>/dev/null | tr '\n' '|'; printf '\n'
    if [[ -d "$dir.remote.git" ]]; then
        printf 'remote log: '; git -C "$dir.remote.git" log --format=%s -1 2>/dev/null | tr '\n' '|'; printf '\n'
        printf 'remote tags: '; git -C "$dir.remote.git" tag -l 2>/dev/null | tr '\n' '|'; printf '\n'
    fi
}

# case_ <name> <cwd> <stdin text or -> <args...>
case_() {
    local name="$1" cwd="$2" answer="$3"; shift 3
    local rc=0
    if [[ "$answer" == "-" ]]; then
        (cd "$cwd" && engine release "$@" > "$WORK/out" 2> "$WORK/err" < /dev/null) || rc=$?
    else
        (cd "$cwd" && printf '%s\n' "$answer" | engine release "$@" > "$WORK/out" 2> "$WORK/err") || rc=$?
    fi
    {
        printf '### %s: agentsync release' "$name"
        printf ' [%s]' "$@"
        printf ' <<< %q\nrc=%s\n--- stdout\n' "$answer" "$rc"
        cat "$WORK/out"
        printf -- '--- stderr\n'
        cat "$WORK/err"
        state "${STATE_DIR:-$cwd}"
    } >> "$OUT"
}

fresh() { bash "$S/mkfixture.sh" "$FIX" "$@"; }

fresh;                                  case_ patch_y        "$FIX" y   patch --no-push
fresh;                                  case_ minor_y        "$FIX" y   minor --no-push
fresh;                                  case_ major_enter    "$FIX" ""  major --no-push
fresh;                                  case_ default_n      "$FIX" n   --no-push
fresh;                                  case_ default_sp_n   "$FIX" " nope" --no-push
fresh;                                  case_ default_yes    "$FIX" yes --no-push
fresh;                                  case_ banana         "$FIX" -   banana
fresh;                                  case_ extra_arg      "$FIX" -   patch extra
fresh; echo x > "$FIX/dirty.txt";       case_ dirty          "$FIX" y   patch --no-push
fresh 1.2;                              case_ two_parts      "$FIX" y   patch --no-push
fresh 1.a.0;                            case_ alpha_part     "$FIX" y   patch --no-push
fresh 1.2.3.4;                          case_ four_parts     "$FIX" y   patch --no-push
fresh;                                  case_ eof            "$FIX" -   patch --no-push
fresh;                                  case_ push_ok        "$FIX" y   patch
fresh 1.0.0 noremote;                   case_ push_noremote  "$FIX" y   patch
fresh 1.0.0 remote nogit;               case_ nogit          "$FIX" y   patch --no-push
fresh; printf 'version = 4\n' > "$FIX/Cargo.lock"; git -C "$FIX" commit -qam lock
                                        case_ no_lock_entry  "$FIX" y   patch --no-push
fresh; rm -f "$FIX/Cargo.toml"; git -C "$FIX" commit -qam toml
                                        case_ no_toml        "$FIX" y   patch --no-push
fresh; HOME_DIR="$FIX" STATE_DIR="$FIX" case_ home_fallback  "$WORK/elsewhere" y patch --no-push
fresh; rm -f "$FIX/VERSION"; git -C "$FIX" commit -qam noversion
       HOME_DIR="$FIX" STATE_DIR="$FIX" case_ home_noversion "$WORK/elsewhere" y patch --no-push
```

`phase5b/release_tty.sh`:

```bash
#!/usr/bin/env bash
# Usage: release_tty.sh <engine 0|1> <repo root> <out file>
# Runs release on a pseudo-terminal through `script`, the answer piped in so
# stdin is not the terminal, and keeps the escape codes for a byte comparison.
set -uo pipefail
MODE="$1"; REPO="$2"; OUT="$3"
S="$(cd "$(dirname "$0")" && pwd)"
WORK="$S/tty_$MODE"
rm -rf "${WORK:?}"
mkdir -p "$WORK/nohome"
: > "$OUT"
unset AGENTSYNC_REPO_ROOT NO_COLOR
export AGENTSYNC_NO_UPDATE_CHECK=1
FIX="$WORK/fixture"
CMD="AGENTSYNC_NATIVE=$MODE AGENTSYNC_HOME=$WORK/nohome bash $REPO/bin/agentsync.sh"
_dirty() { echo x > dirty.txt; }
# scenario <name> <answer> <args> [<function run inside the fixture first>]
scenario() {
    local name="$1" answer="$2" args="$3" prepare="${4:-}"
    local transcript="$WORK/$name.txt"
    bash "$S/mkfixture.sh" "$FIX"
    if [[ -n "$prepare" ]]; then (cd "$FIX" && "$prepare"); fi
    (cd "$FIX" && script -q "$transcript" bash -c "printf '%s\n' '$answer' | $CMD release $args; echo \"rc=\$?\"" < /dev/null > /dev/null 2> "$WORK/$name.err")
    {
        echo "### $name :: $args <<< $answer"
        cat "$WORK/$name.err"
        tr -d '\r' < "$transcript" | od -c | head -200
    } >> "$OUT"
}
scenario "cancelled" "n" "--no-push"
scenario "released" "y" "patch --no-push"
scenario "banana" "y" "banana"
scenario "dirty" "y" "patch --no-push" _dirty
```

```bash
bash phase5b/release_reference.sh 0 "$PWD" phase5b/ref_bash.out && bash phase5b/release_reference.sh 1 "$PWD" phase5b/ref_native.out
wc -l < phase5b/ref_bash.out
grep -c '^### ' phase5b/ref_bash.out
diff phase5b/ref_bash.out phase5b/ref_native.out | grep -c '^[<>]'
diff phase5b/ref_bash.out phase5b/ref_native.out | grep -c 'Cannot parse VERSION'
bash phase5b/release_tty.sh 0 "$PWD" phase5b/tty_bash.out && bash phase5b/release_tty.sh 1 "$PWD" phase5b/tty_native.out
wc -l < phase5b/tty_native.out
grep -c '033' phase5b/tty_native.out
grep -c '^script' phase5b/tty_native.out
diff phase5b/tty_bash.out phase5b/tty_native.out | grep -c '^[<>]'
```

Expected: `423` lines over `20` situations with `31` differing lines, all under `### alpha_part` and `### four_parts` (the decimal-components deviation: Bash released `1.a.1` and died in arithmetic on `1.2.3.4`, the binary refuses both), `2` of them `Error: Cannot parse VERSION` lines; `56` dump lines, `25` of them holding an escape, `0` `script` errors, and `0` differing lines over the four pty scenarios (the cancelled prompt, a release, the unknown bump type, the dirty tree).

```bash
shellcheck -x -S warning -e SC1091 bin/agentsync.sh
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/cli/release.rs src/cli/mod.rs src/main.rs bin/agentsync.sh tests/native_parity.bats docs/specs/2026-09-12-rust-migration-design.md docs/plans/2026-09-18-rust-migration-phase-5b-release.md
git commit -m "feat(native): port release"
```

---

### Task 4: Verify and Module Map

**Files:**
- Modify: `.ai/src/skills/native-port/references/module-map.md`, `docs/specs/2026-09-12-rust-migration-design.md:117,304-305`, `.ai/.sync-manifest`

- [ ] **Step 1: Module map, spec, and outputs**

In the module map's "Engine modules" block, replace

```text
lib/helpers/release.sh           → src/cli/release.rs      Phase 5, bumps VERSION and Cargo.toml
```

with

```text
lib/helpers/release.sh           → src/cli/release.rs      Phase 5b, ported; git through the executable, the tag message on its stdin
```

In "Command closure and ownership", replace the `release` row's last column `VERSION, tag, push` with `VERSION, Cargo.toml, Cargo.lock, tag, push`. In "bats ownership", set `tests/native_parity.bats 68` to `70`, remove the row `tests/release.bats 10         release`, and add after `tests/guard.bats 18           guard output, adopt, doctor, init, profile`:

```text
tests/release.bats 18         release
```

In the spec, replace line 117

```text
Cargo.toml                 # crate `agentsync`, version stays 0.0.0 (VERSION file rules)
```

with

```text
Cargo.toml                 # crate `agentsync`, version equal to VERSION (release bumps both)
```

and the Phase 5 bullet

```markdown
- `release` bumps `VERSION` and `Cargo.toml` together; the auto-tag workflow
  triggers the release build.
```

with

```markdown
- `release` bumps `VERSION`, `Cargo.toml`, and `Cargo.lock` together; the
  auto-tag workflow triggers the release build.
```

Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force > /dev/null` (outside the sandbox if it refuses a write) and read `git status --short`: ` M .ai/.sync-manifest`, ` M .ai/src/skills/native-port/references/module-map.md`, ` M docs/specs/2026-09-12-rust-migration-design.md`, and the plan.

- [ ] **Step 2: Verify (outside the agent sandbox)**

Recreate `phase5b/native_suite.sh` when the scratchpad no longer holds it:

```bash
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
```

```bash
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
bash phase5b/native_suite.sh "$PWD" both phase5b/suite_both.out && tail -1 phase5b/suite_both.out
```

Expected: `298 passed`, `0`, `11`, `1`; lint exit 0; `TOTAL bash=0 native=0` over the 50 bats files, each run one at a time under both engines. `sync` and `check` do not change in this slice, so no timings are due.

- [ ] **Step 3: Commit**

```bash
git add .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/specs/2026-09-12-rust-migration-design.md docs/plans/2026-09-18-rust-migration-phase-5b-release.md
git commit -m "docs(native): map the phase 5b module"
```

---

## Completion

The plan is closed when every box is ticked, every bats file is green under both engines, the two parity fixtures pass, the crate version equals `VERSION` with `src/lib.rs` guarding it, and a `## Completion receipt` records the fresh verification. `sync` and `check` do not change, so no timings are due. Phase 5 stays open until the plans for 5c, 5d, and 5e are closed as well.

## Run log

### 2026-09-18 — Phase 5b planned
- Commits: this plan.
- Verified: the whole slice was drafted in the tree and parked in the session scratchpad (`phase5b/draft/`), then the tree was restored to HEAD. Against the draft: `cargo test` 298/0/11/1, fmt and clippy exit 0, ShellCheck exit 0 on `bin/agentsync.sh` and `lib/helpers/release.sh`; `release.bats` 18 cases at `bash=0 native=0`; the two parity fixtures `ok`, `not ok` with `output differs for [release patch --no-push]` on the Step 5 mutation, and `not ok` with `bash=0 native=1` (16 of 18 native `release.bats` cases failing) against a binary without the port; `release_reference.sh` 423 lines over 20 situations with 31 differing lines, all under `alpha_part` and `four_parts`; `release_tty.sh` 56 dump lines, 25 with escapes, 0 differing, run outside the sandbox because `openpty` is refused inside it. The `lib.rs` guard fails against `version = "0.0.0"` with the assertion quoted in Task 1 Step 1; Task 2's tests fail 4 of 18 against the committed `release.sh`; the tests-first `release.rs` fails to compile with the two errors quoted in Task 3 Step 2. The Bash probe found end of input at the prompt exiting 1 silently (quirk 54), `1.a.0` bumping to `1.a.1`, `1.2.3.4` dying in arithmetic, and a missing Cargo entry ignored; the first is reproduced, the next two are accepted deviations, the last is what Task 2 fixes. `target/release/agentsync` still carries the draft build until Task 0 rebuilds it.
- Plan amended: none.
- Next: Task 0 Step 1, after the review.
- Blocker: none.

### 2026-09-18 — Task 0 and Task 1 done
- Commits: this commit, feat(native): carry VERSION in Cargo.toml and Cargo.lock.
- Verified: baseline at `569dadb`: `cargo test` 289/0/11/1; `cargo build --release` rebuilt the binary from HEAD, replacing the draft build; `release`, `native_dispatch`, and `native_parity` at bash=0, the last outside the sandbox; 10 and 68 cases; `version = "0.0.0"` on line 5 of `Cargo.toml` and line 7 of `Cargo.lock`. Task 1: the `src/lib.rs` guard failed with left `"0.0.0"`, right `"0.36.0"`; after the bump `cargo test` 290/0/11/1, `Cargo.lock | 2 +-`, its line 7 `version = "0.36.0"`; `sync --dry-run` 0 warnings; `sync --force` refused inside the sandbox (the backup `tar` cannot create under `.claude/commands/`, `.claude/agents/`, and `.mcp.json`, `Operation not permitted`, and the transaction changed nothing) and run outside it; `.claude/rules/native-engine.md` `same` as its source; status exactly the five files; `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` exit 0.
- Plan amended: none.
- Next: Task 2 Step 1.
- Blocker: none.

### 2026-09-18 — Task 2 done
- Commits: this commit, feat(release): bump Cargo.toml and Cargo.lock with VERSION.
- Verified: the 18-case `tests/release.bats` against the committed `release.sh` failed exactly cases 4, 8, 12, and 13; against the new `release.sh` 0 failures over 18 cases, and 0 under `AGENTSYNC_NATIVE=1` as well, since `release` is not in `_NATIVE_COMMANDS` yet and both engines run Bash; `shellcheck -x -S warning -e SC1091 lib/helpers/release.sh bin/agentsync.sh` exit 0.
- Plan amended: none.
- Next: Task 3 Step 1.
- Blocker: none.

### 2026-09-18 — Task 3 done
- Commits: this commit, feat(native): port release.
- Verified: Step 1 against the binary without the port: both parity cases `not ok` with `exit status differs for [release patch --no-push]: bash=0 native=1` and `[release --no-push]: bash=0 native=1`, 16 of 18 native `release.bats` cases failing; Step 2: `cargo test cli::release` failed with 6 × E0433 `Bump` and 5 × E0425 `set_crate_version`; Step 4: `cargo test` 298/0/11/1, `release.bats` bash=0 native=0, both parity cases `ok`, 70 and 18 cases; Step 5: the `→` to `:` mutation gave `not ok 1` with `output differs for [release patch --no-push]`, reverted and rebuilt; `release_reference.sh` 423 lines over 20 situations, 31 differing lines all inside `### alpha_part` (213–241) and `### four_parts` (242–255), 2 of them `Cannot parse VERSION`; `release_tty.sh` outside the sandbox: 56 dump lines, 25 with escapes, 0 `script` errors, 0 differing; `shellcheck -x -S warning -e SC1091 bin/agentsync.sh` exit 0, `cargo fmt --all --check` exit 0, `cargo clippy --all-targets -- -D warnings` exit 0. The harness scripts were recreated in this session's scratchpad (`phase5b/`) from Task 3 Step 5.
- Plan amended: none.
- Next: Task 4 Step 1.
- Blocker: none.
