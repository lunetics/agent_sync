#!/usr/bin/env bats
# Foreign changes must block the whole rollback, even with --yes.
load test_helper

setup() {
    setup_test_project
    source "$REPO_ROOT/lib/helpers/paths.sh"
    source "$REPO_ROOT/lib/helpers/yaml.sh"
    source "$REPO_ROOT/lib/helpers/project_config.sh"
    source "$REPO_ROOT/lib/helpers/manifest.sh"
    source "$REPO_ROOT/lib/helpers/backup.sh"
    source "$REPO_ROOT/lib/helpers/backup_state.sh"
    PROOF_DIR="$(host_path "$(mktemp -d "${TMPDIR:-/tmp}/agentsync_rollback_proof.XXXXXX")")"
    run_agentsync init --tools claude,codex --yes --no-sync >/dev/null
    checkpoint initialized
    cat > .ai/agent_sync.yaml <<'YAML'
outputs: local
base_skills: false
defaults:
  cleanup: false
tools:
  enabled: [claude, codex]
backup:
  retention: preserve
YAML
    local tool
    for tool in claude codex; do
        cat > ".ai/src/tools/$tool.yaml" <<'YAML'
targets:
  agents:
    enabled: false
  rules:
    enabled: false
  commands:
    enabled: false
  subagents:
    enabled: false
  settings:
    enabled: false
  mcp:
    enabled: false
  hooks:
    enabled: false
  guard:
    enabled: false
YAML
    done
}

teardown() {
    # The complete archives and separate restores stay outside the test project.
    teardown_test_project
}

# Independent oracle: lstat/link text and bytes, never linked target contents.
assert_tree_equal() {
    python3 - "$1" "$2" <<'PY'
import hashlib, os, stat, sys

def tree(root):
    result = {}
    def visit(path, rel):
        info = os.lstat(path)
        kind = stat.S_IFMT(info.st_mode)
        if stat.S_ISLNK(info.st_mode):
            value = os.readlink(path)
        elif stat.S_ISREG(info.st_mode):
            with open(path, "rb") as handle:
                value = hashlib.sha256(handle.read()).hexdigest()
        else:
            value = None
        result[rel] = (kind, stat.S_IMODE(info.st_mode), value)
        if stat.S_ISDIR(info.st_mode):
            with os.scandir(path) as entries:
                for entry in sorted(entries, key=lambda e: e.name):
                    visit(entry.path, rel + "/" + entry.name)
    visit(root, ".")
    return result

left, right = map(tree, sys.argv[1:])
changed = [p for p in sorted(set(left) | set(right)) if left.get(p) != right.get(p)]
if changed:
    print("Tree mismatch:", repr(changed))
    sys.exit(1)
PY
}

skip_on_windows() {
    case "$(uname -s)" in
        MINGW*|MSYS*) skip "$1" ;;
    esac
}

checkpoint() {
    local label="$1"
    tar -cpf "$PROOF_DIR/$label.tar" .
    mkdir "$PROOF_DIR/$label"
    tar -xpf "$PROOF_DIR/$label.tar" -C "$PROOF_DIR/$label"
    assert_tree_equal . "$PROOF_DIR/$label"
}

sync_once() {
    checkpoint before-sync
    run_agentsync sync >/dev/null
    SYNC_ID="$(cat .ai/backups/.latest)"
    checkpoint after-sync
}

assert_refused_unchanged() {
    checkpoint before-refusal
    run run_agentsync rollback "$SYNC_ID" --yes
    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "Error: Rollback conflict: "
    assert_tree_equal . "$PROOF_DIR/before-refusal"
}

@test "rollback preflight blocks later native settings even when settings are disabled" {
    sync_once
    mkdir -p .codex .claude
    printf 'foreign-codex\n' > .codex/config.toml
    printf '{"foreign":"claude"}\n' > .claude/settings.json
    assert_refused_unchanged
    [ "$(cat .codex/config.toml)" = foreign-codex ]
    [ "$(cat .claude/settings.json)" = '{"foreign":"claude"}' ]
}

@test "rollback preflight blocks a changed existing native file" {
    mkdir -p .codex
    printf 'native-before\n' > .codex/config.toml
    sync_once
    printf 'native-after\n' > .codex/config.toml
    assert_refused_unchanged
}

@test "rollback preflight blocks deletion of an existing target" {
    mkdir -p .claude
    printf 'native-before\n' > .claude/settings.json
    sync_once
    rm .claude/settings.json
    assert_refused_unchanged
}

@test "rollback preflight blocks foreign children in managed directories before any writes" {
    sync_once
    mkdir -p .claude/skills/foreign
    printf 'unrelated\n' > .claude/skills/foreign/NOTE.md
    assert_refused_unchanged
}

@test "rollback preflight blocks a directory replaced by a file" {
    sync_once
    mv .claude/skills "$PROOF_DIR/skills-before-type-change"
    printf 'foreign file\n' > .claude/skills
    assert_refused_unchanged
}

@test "rollback preflight blocks a file replaced by a directory" {
    mkdir -p .codex
    printf 'native-before\n' > .codex/config.toml
    sync_once
    rm .codex/config.toml
    mkdir .codex/config.toml
    printf 'foreign child\n' > .codex/config.toml/child
    assert_refused_unchanged
}

@test "rollback preflight blocks symlink replacement without touching either link target" {
    skip_on_windows "the tar checkpoint cannot recreate Windows symlinks"
    mkdir -p private-knowledge other-knowledge .claude/skills/manual
    printf 'original\n' > private-knowledge/data
    printf 'other\n' > other-knowledge/data
    create_test_symlink "$TEST_PROJECT/private-knowledge" .claude/skills/manual/knowledge
    sync_once
    rm .claude/skills/manual/knowledge
    create_test_symlink "$TEST_PROJECT/other-knowledge" .claude/skills/manual/knowledge
    assert_refused_unchanged
    [ "$(cat private-knowledge/data)" = original ]
    [ "$(cat other-knowledge/data)" = other ]
}

@test "rollback preflight blocks a dangling link added at an absent target" {
    skip_on_windows "the tar checkpoint cannot recreate Windows symlinks"
    sync_once
    mkdir -p .codex
    create_test_symlink "$TEST_PROJECT/does-not-exist" .codex/config.toml
    assert_refused_unchanged
    [ -L .codex/config.toml ]
}

@test "rollback preflight ignores mutable knowledge contents and normal undo is still safe" {
    skip_on_windows "the tar checkpoint cannot recreate Windows symlinks"
    mkdir -p private-knowledge .claude/skills/manual
    printf 'before\n' > private-knowledge/data
    create_test_symlink "$TEST_PROJECT/private-knowledge" .claude/skills/manual/knowledge
    sync_once
    printf 'legitimate new knowledge\n' > private-knowledge/data
    printf 'new entry\n' > private-knowledge/new
    checkpoint before-rollback
    run run_agentsync rollback "$SYNC_ID" --yes
    [ "$status" -eq 0 ]
    [ "$(cat private-knowledge/data)" = "legitimate new knowledge" ]
    [ "$(cat private-knowledge/new)" = "new entry" ]
    [ -L .claude/skills/manual/knowledge ]
    local undo
    undo="$(cat .ai/backups/.latest)"
    checkpoint before-undo
    run run_agentsync rollback "$undo" --yes
    [ "$status" -eq 0 ]
    [ "$(cat private-knowledge/data)" = "legitimate new knowledge" ]
    [ "$(cat private-knowledge/new)" = "new entry" ]
    [ -L .claude/skills/manual/knowledge ]
    assert_tree_equal .claude/skills "$PROOF_DIR/after-sync/.claude/skills"
    [ -d ".ai/backups/$SYNC_ID" ]
    [ -d ".ai/backups/$undo" ]
}

@test "rollback preflight compares the actual post-sync state not the pre-sync backup" {
    mkdir -p .claude/skills/manual
    printf 'old user skill\n' > .claude/skills/manual/SKILL.md
    sync_once
    checkpoint before-rollback
    run run_agentsync rollback "$SYNC_ID" --yes
    [ "$status" -eq 0 ]
    assert_tree_equal .claude/skills "$PROOF_DIR/before-sync/.claude/skills"
    local undo
    undo="$(cat .ai/backups/.latest)"
    checkpoint before-undo
    run run_agentsync rollback "$undo" --yes
    [ "$status" -eq 0 ]
    assert_tree_equal .claude/skills "$PROOF_DIR/after-sync/.claude/skills"
}

@test "rollback preflight restores an unsealed historical snapshot with a warning" {
    mkdir -p .codex
    printf 'before\n' > .codex/config.toml
    local historical
    historical="$(backup_create "$TEST_PROJECT" sync .codex/config.toml)"
    printf 'after\n' > .codex/config.toml
    run run_agentsync rollback "$(basename "$historical")" --yes
    [ "$status" -eq 0 ]
    printf '%s' "$output" | grep -qF -- "Warning: Backup $(basename "$historical") has no post-operation record; changes made after that operation cannot be detected."
    [ "$(cat .codex/config.toml)" = before ]
}

@test "rollback preflight dry-run shows the plan and the conflict and exits 1" {
    sync_once
    mkdir -p .codex
    printf 'foreign\n' > .codex/config.toml
    checkpoint before-refusal
    run run_agentsync rollback "$SYNC_ID" --dry-run
    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "Rollback plan:"
    printf '%s' "$output" | grep -qF -- "Error: Rollback conflict: .codex/config.toml changed after the operation recorded in backup $SYNC_ID; no files were changed."
    printf '%s' "$output" | grep -qF -- "Dry run — nothing was written."
    run run_agentsync rollback "$SYNC_ID" --dry-run --force
    [ "$status" -eq 0 ]
    printf '%s' "$output" | grep -qF -- "Warning: Rollback conflict: .codex/config.toml changed after the operation recorded in backup $SYNC_ID; --force will overwrite it."
    assert_tree_equal . "$PROOF_DIR/before-refusal"
}

@test "rollback conflict names the first differing path inside a directory" {
    sync_once
    printf 'finder\n' > .claude/skills/.DS_Store
    run run_agentsync rollback --yes
    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "Error: Rollback conflict: .claude/skills/.DS_Store changed after the operation recorded in backup $SYNC_ID; no files were changed."
    printf '%s' "$output" | grep -qF -- "Re-run with --force to restore the backup anyway and discard that change."
    [ "$(cat .ai/backups/.latest)" = "$SYNC_ID" ]
}

@test "rollback --force restores over a conflict and keeps it undoable" {
    sync_once
    mkdir -p .claude/skills/foreign
    printf 'unrelated\n' > .claude/skills/foreign/NOTE.md
    run run_agentsync rollback "$SYNC_ID" --yes --force
    [ "$status" -eq 0 ]
    [ ! -e .claude/skills/foreign ]
    local undo
    undo="$(cat .ai/backups/.latest)"
    [ "$undo" != "$SYNC_ID" ]
    run run_agentsync rollback "$undo" --yes
    [ "$status" -eq 0 ]
    [ "$(cat .claude/skills/foreign/NOTE.md)" = unrelated ]
}

@test "rolling back an older backup after a newer sync points at the newer backups" {
    sync_once
    local first="$SYNC_ID"
    mkdir -p .ai/src/skills/added
    printf -- '---\nname: added\ndescription: Added after the first sync.\n---\n\nBody.\n' > .ai/src/skills/added/SKILL.md
    run_agentsync sync >/dev/null
    local second
    second="$(cat .ai/backups/.latest)"
    [ -e .claude/skills/added/SKILL.md ]
    run run_agentsync rollback "$first" --yes
    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "Error: Rollback conflict: .claude/skills/added changed after the operation recorded in backup $first; no files were changed."
    printf '%s' "$output" | grep -qF -- "Newer AgentSync operations may have changed this target. Roll back the newer backups first, or re-run with --force to restore anyway."
    run run_agentsync rollback "$second" --yes
    [ "$status" -eq 0 ]
    run run_agentsync rollback "$first" --yes
    [ "$status" -eq 0 ]
    [ ! -e .claude/skills ]
    [ ! -e "$PROOF_DIR/before-sync/.claude/skills" ]
}

@test "rollback preflight detects mode changes on regular files" {
    skip_on_windows "Windows does not model POSIX chmod modes"
    mkdir -p .codex
    printf 'before\n' > .codex/config.toml
    chmod 644 .codex/config.toml
    sync_once
    chmod 755 .codex/config.toml
    assert_refused_unchanged
}

@test "rollback preflight refuses changed ancestor links even when contents match" {
    skip_on_windows "the tar checkpoint cannot recreate Windows symlinks"
    sync_once
    mv .claude "$PROOF_DIR/claude-original"
    create_test_symlink "$PROOF_DIR/claude-original" .claude
    checkpoint before-refusal
    run run_agentsync rollback "$SYNC_ID" --yes
    [ "$status" -ne 0 ]
    assert_tree_equal . "$PROOF_DIR/before-refusal"
    assert_tree_equal "$PROOF_DIR/claude-original" "$PROOF_DIR/after-sync/.claude"
}

@test "rollback preflight treats a malformed post-operation record as unsealed" {
    sync_once
    mkdir -p .claude/skills/foreign
    printf 'broken\n' > ".ai/backups/$SYNC_ID/after.tsv"
    run run_agentsync rollback "$SYNC_ID" --dry-run
    [ "$status" -eq 0 ]
    printf '%s' "$output" | grep -qF -- "Warning: Backup $SYNC_ID has a malformed post-operation record; changes made after that operation cannot be detected."
    printf 'post-state-v2\t%064d\nfile\tnot-a-hash\t.claude/skills\n' 0 > ".ai/backups/$SYNC_ID/after.tsv"
    run run_agentsync rollback "$SYNC_ID" --dry-run
    [ "$status" -eq 0 ]
    printf '%s' "$output" | grep -qF -- "has a post-operation record that does not match its target list"
    : > ".ai/backups/$SYNC_ID/after.tsv"
    run run_agentsync rollback "$SYNC_ID" --yes
    [ "$status" -eq 0 ]
    printf '%s' "$output" | grep -qF -- "Warning: Backup $SYNC_ID has a malformed post-operation record"
    [ ! -e .claude/skills/foreign ]
}

@test "rollback preflight treats a record with a malformed body as unsealed" {
    sync_once
    printf 'foreign\n' > .claude/skills/NOTE.md
    local record=".ai/backups/$SYNC_ID/after.tsv"
    { head -1 "$record"; printf 'file\tnot-a-hash\t.claude/skills\n'; } > "$PROOF_DIR/after.tsv"
    cat "$PROOF_DIR/after.tsv" > "$record"
    run run_agentsync rollback "$SYNC_ID" --yes
    [ "$status" -eq 0 ]
    printf '%s' "$output" | grep -qF -- "Warning: Backup $SYNC_ID has a malformed post-operation record"
    [ ! -e .claude/skills/NOTE.md ]
}

@test "rollback preflight supports unusual child names without ignoring their changes" {
    [[ "$OSTYPE" != msys* ]] || skip "Windows does not support control characters in filenames"
    mkdir -p .claude/skills/manual
    printf 'before\n' > $'.claude/skills/manual/tab\tand\nnewline'
    sync_once
    printf 'after\n' > $'.claude/skills/manual/tab\tand\nnewline'
    assert_refused_unchanged
}

@test "rollback preflight supports sealed init snapshots and their undo" {
    local fresh="$TEST_PROJECT/fresh"
    mkdir "$fresh"
    cd "$fresh"
    git init --quiet
    checkpoint before-fresh-init
    run_agentsync init --tools claude --yes --no-sync >/dev/null
    local init_id undo_id
    init_id="$(cat .ai/backups/.latest)"
    checkpoint before-init-rollback
    run run_agentsync rollback "$init_id" --yes
    [ "$status" -eq 0 ]
    [ ! -e .ai/src ]
    undo_id="$(cat .ai/backups/.latest)"
    checkpoint before-init-undo
    run run_agentsync rollback "$undo_id" --yes
    [ "$status" -eq 0 ]
    assert_tree_equal .ai/src "$PROOF_DIR/before-init-rollback/.ai/src"
}

@test "rollback preflight catches edits during safety backup without arming destructive recovery" {
    skip_on_windows "the race is staged through the Bash backup helper"
    sync_once
    checkpoint before-race
    eval "$(declare -f backup_create | sed '1s/backup_create/real_backup_create/')"
    backup_create() {
        real_backup_create "$@" || return 1
        mkdir -p "$TEST_PROJECT/.codex"
        printf 'concurrent foreign change\n' > "$TEST_PROJECT/.codex/config.toml"
    }
    AGENTSYNC_REPO_ROOT="$TEST_PROJECT" run cmd_rollback "$SYNC_ID" --yes
    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "Error: Rollback conflict: .codex/config.toml changed after the operation recorded in backup $SYNC_ID"
    [ "$(cat .codex/config.toml)" = "concurrent foreign change" ]
    assert_tree_equal .claude/skills "$PROOF_DIR/after-sync/.claude/skills"
    cmp .ai/.sync-manifest "$PROOF_DIR/after-sync/.ai/.sync-manifest"
    [ "$(cat .ai/backups/.latest)" = "$SYNC_ID" ]
    [ "$(find .ai/backups -maxdepth 1 -name '*-rollback-*' | wc -l | tr -d ' ')" = 0 ]
}

@test "snapshot post-state refuses trailing-slash targets before traversing a link" {
    skip_on_windows "the tar checkpoint cannot recreate Windows symlinks"
    mkdir private-knowledge
    printf 'mutable\n' > private-knowledge/data
    checkpoint before-snapshot
    local snapshot sealed=true
    snapshot="$(backup_create "$TEST_PROJECT" sync managed/)"
    checkpoint before-link
    create_test_symlink "$TEST_PROJECT/private-knowledge" managed
    checkpoint before-seal
    backup_seal "$TEST_PROJECT" "$snapshot" || sealed=false
    [ "$sealed" = false ]
    [ "$BACKUP_SEAL_REASON" = "the snapshot target list is invalid" ]
    [ ! -e "$snapshot/after.tsv" ]
    assert_tree_equal . "$PROOF_DIR/before-seal"
}

@test "snapshot post-state records one line per path with batched hashes" {
    mkdir -p .codex/sub
    printf 'one\n' > .codex/config.toml
    printf '#!/bin/sh\n' > .codex/sub/run.sh
    chmod +x .codex/sub/run.sh
    create_test_symlink "config.toml" .codex/link
    local snapshot
    snapshot="$(backup_create "$TEST_PROJECT" sync .codex absent.md)"
    backup_seal "$TEST_PROJECT" "$snapshot"
    local tab=$'\t' expected
    expected="post-state-v2${tab}$(file_sha256 "$snapshot/targets.tsv")
dir${tab}-${tab}.codex
file${tab}$(file_sha256 .codex/config.toml)${tab}.codex/config.toml
link${tab}config.toml${tab}.codex/link
dir${tab}-${tab}.codex/sub
exec${tab}$(file_sha256 .codex/sub/run.sh)${tab}.codex/sub/run.sh
missing${tab}-${tab}absent.md"
    [[ "$OSTYPE" != msys* ]] || skip "Windows derives the executable bit from file contents"
    [ "$(cat "$snapshot/after.tsv")" = "$expected" ]
}

@test "snapshot post-state hashes names that hash tools escape" {
    skip_on_windows "Windows does not allow these characters in filenames"
    mkdir -p .codex
    printf 'slash\n' > '.codex/back\slash'
    printf 'newline\n' > $'.codex/new\nline'
    printf 'percent\n' > '.codex/100%'
    local snapshot tab=$'\t'
    snapshot="$(backup_create "$TEST_PROJECT" sync .codex)"
    backup_seal "$TEST_PROJECT" "$snapshot"
    grep -qxF -- "file${tab}$(file_sha256 /dev/stdin < '.codex/back\slash')${tab}.codex/back\\slash" "$snapshot/after.tsv"
    grep -qxF -- "file${tab}$(printf 'newline\n' | file_sha256 /dev/stdin)${tab}.codex/new%0Aline" "$snapshot/after.tsv"
    grep -qxF -- "file${tab}$(file_sha256 /dev/stdin < '.codex/100%')${tab}.codex/100%25" "$snapshot/after.tsv"
}

@test "an unreadable file is the reported conflict, not the files hashed after it" {
    sync_once
    local victim
    victim="$(find .claude/skills -type f | LC_ALL=C sort | head -1)"
    [ -n "$victim" ]
    chmod 000 "$victim"
    if [ -r "$victim" ]; then
        chmod 644 "$victim"
        skip "file permissions do not restrict this user"
    fi
    run run_agentsync rollback "$SYNC_ID" --dry-run
    chmod 644 "$victim"
    [ "$status" -eq 1 ]
    printf '%s' "$output" | grep -qF -- "Error: Rollback conflict: $victim changed after"
}

@test "sync through a .claude symlink inside the project succeeds and seals" {
    mkdir -p tooling/claude
    create_test_symlink tooling/claude .claude
    run run_agentsync sync
    [ "$status" -eq 0 ]
    [ "$(printf '%s' "$output" | grep -cF -- "restoring pre-sync state")" = 0 ]
    SYNC_ID="$(cat .ai/backups/.latest)"
    grep -qF -- "$(printf 'dir\t-\t.claude/skills')" ".ai/backups/$SYNC_ID/after.tsv"
    [ -L .claude ]
    run run_agentsync rollback "$SYNC_ID" --yes
    [ "$status" -eq 0 ]
}

@test "sync with a FIFO under a target succeeds and records it by type" {
    skip_on_windows "Windows has no FIFOs"
    command -v mkfifo >/dev/null 2>&1 || skip "mkfifo is unavailable"
    mkdir -p .claude/skills
    mkfifo .claude/skills/pipe || skip "the filesystem does not support FIFOs"
    run run_agentsync sync
    [ "$status" -eq 0 ]
    [ "$(printf '%s' "$output" | grep -cF -- "restoring pre-sync state")" = 0 ]
    SYNC_ID="$(cat .ai/backups/.latest)"
    grep -qF -- "$(printf 'other\t-\t.claude/skills/pipe')" ".ai/backups/$SYNC_ID/after.tsv"
}
