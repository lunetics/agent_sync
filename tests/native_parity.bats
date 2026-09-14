#!/usr/bin/env bats
# Bash and native answers for a ported command must match byte for byte.
# Skips when no binary is built: `cargo build --release` first.

load test_helper

setup_file() { seed_project; }
teardown_file() { teardown_seed_project; }

setup() {
    clone_seed
    NATIVE_BIN="${AGENTSYNC_NATIVE_BIN:-$REPO_ROOT/target/release/agentsync}"
    if [[ ! -x "$NATIVE_BIN" ]] && [[ -x "$NATIVE_BIN.exe" ]]; then
        NATIVE_BIN="$NATIVE_BIN.exe"
    fi
    [[ -x "$NATIVE_BIN" ]] || skip "no native binary at $NATIVE_BIN"
    export AGENTSYNC_NATIVE_BIN="$NATIVE_BIN"
}

teardown() { teardown_test_project; }

_run_engine() {
    local mode="$1"
    shift
    AGENTSYNC_NATIVE="$mode" AGENTSYNC_HOME="$REPO_ROOT" bash "$AGENTSYNC_BIN" "$@"
}

# Usage: assert_parity <agentsync args...>
# Fails with a diff when stdout+stderr differ, or when the exit status differs.
assert_parity() {
    local bash_out native_out bash_rc=0 native_rc=0
    bash_out=$(_run_engine 0 "$@" 2>&1) || bash_rc=$?
    native_out=$(_run_engine 1 "$@" 2>&1) || native_rc=$?
    if [[ "$bash_rc" -ne "$native_rc" ]]; then
        echo "exit status differs: bash=$bash_rc native=$native_rc" >&2
        return 1
    fi
    if [[ "$bash_out" != "$native_out" ]]; then
        diff <(printf '%s\n' "$bash_out") <(printf '%s\n' "$native_out") >&2 || true
        return 1
    fi
}

@test "parity: version and its flags" {
    assert_parity version
    assert_parity --version
    assert_parity -v
}

@test "parity: list on a fresh project" {
    assert_parity list
    assert_parity ls
}

@test "parity: list with enabled tools" {
    enable_tools claude cursor
    assert_parity list
}

@test "parity: list with a payload override in the per-tool layout" {
    mkdir -p .ai/src/tools/cursor
    echo '{}' > .ai/src/tools/cursor/hooks.json
    assert_parity list
}

@test "parity: list with a legacy flat-layout override" {
    mkdir -p .ai/src/mcp
    echo '{}' > .ai/src/mcp/claude.json
    assert_parity list
}

@test "parity: list with a shared MCP source and one per-tool override" {
    echo '{}' > .ai/src/mcp.json
    mkdir -p .ai/src/tools/kimi
    echo '{}' > .ai/src/tools/kimi/mcp.json
    assert_parity list
}

@test "parity: list with a custom tool enabled the legacy way" {
    mkdir -p .ai/src/tools
    printf 'name: "My Tool"\nenabled: true\n' > .ai/src/tools/mytool.yaml
    assert_parity list
}

@test "parity: list with a profile variant tool" {
    mkdir -p .ai/src/tools
    printf 'base: claude\nprofile_home: ".claude-hub"\n' > .ai/src/tools/claude-hub.yaml
    assert_parity list
}

@test "parity: list without a .ai directory" {
    rm -rf .ai
    assert_parity list
}

# ── check ────────────────────────────────────────────────────────────────────
# Outputs always come from the Bash sync, so a native check that agrees proves
# the render reproduces every managed file byte for byte.

ALL_TOOLS=(amazonq antigravity claude cline codex copilot cursor gemini junie kimi opencode windsurf zed)

_bash_sync() { _run_engine 0 sync "$@" >/dev/null 2>&1; }

# Usage: assert_parity_head <lines> <agentsync args...>
# For an accepted deviation past the first <lines> lines: the exit status and
# those lines must match.
assert_parity_head() {
    local count="$1"
    shift
    local bash_out native_out bash_rc=0 native_rc=0
    bash_out=$(_run_engine 0 "$@" 2>&1) || bash_rc=$?
    native_out=$(_run_engine 1 "$@" 2>&1) || native_rc=$?
    if [[ "$bash_rc" -ne "$native_rc" ]]; then
        echo "exit status differs: bash=$bash_rc native=$native_rc" >&2
        return 1
    fi
    bash_out=$(printf '%s\n' "$bash_out" | head -n "$count")
    native_out=$(printf '%s\n' "$native_out" | head -n "$count")
    if [[ "$bash_out" != "$native_out" ]]; then
        diff <(printf '%s\n' "$bash_out") <(printf '%s\n' "$native_out") >&2 || true
        return 1
    fi
}

_all_tools_fixture() {
    enable_tools "${ALL_TOOLS[@]}"
    printf '%s\n' '---' 'paths:' '  - "**/*.dart"' '---' '' '# Scoped Fixture Rule' '' '- Body.' \
        > .ai/src/rules/scoped-fixture.md
    printf '%s\n' '---' 'description: Explicit-only fixture command' 'disable-model-invocation: true' '---' '' 'Body.' \
        > .ai/src/commands/explicit-only.md
}

@test "parity: check on a project that was never synced" {
    enable_tools claude cursor
    assert_parity check
}

@test "parity: check after a Bash sync of all 13 tools" {
    _all_tools_fixture
    _bash_sync
    assert_parity check
}

@test "parity: check reports edited, deleted, and stale outputs" {
    _all_tools_fixture
    _bash_sync
    echo "edit" >> CLAUDE.md
    rm -f .cursor/rules/core.mdc
    echo "# more" >> .ai/src/rules/core.md
    assert_parity check
}

@test "parity: check after a tool is disabled" {
    enable_tools claude cursor codex
    _bash_sync
    _run_engine 0 disable cursor >/dev/null
    assert_parity check
}

@test "parity: check with shared inheritance and a changed parent" {
    enable_tools claude codex
    mkdir -p 'shared parent/.ai/src/rules' 'shared parent/.ai/src/skills/parent-only' 'shared parent/.ai/src/tools'
    printf 'parent rule\n' > 'shared parent/.ai/src/rules/parent-only.md'
    printf 'parent skill\n' > 'shared parent/.ai/src/skills/parent-only/SKILL.md'
    printf 'targets:\n  agents:\n    dest: "OTHER.md"\n' > 'shared parent/.ai/src/tools/claude.yaml'
    printf '\nshared:\n  path: "shared parent"\n  inherit: rules, skills, tools\n' >> .ai/agent_sync.yaml
    _bash_sync
    assert_parity check
    printf 'changed parent rule\n' > 'shared parent/.ai/src/rules/parent-only.md'
    assert_parity check
}

@test "parity: check with the engine skill layer off and a project copy of it" {
    enable_tools claude kimi
    _bash_sync
    printf '\nbase_skills: false\n' >> .ai/agent_sync.yaml
    assert_parity check
    mkdir -p .ai/src/skills/agentsync
    printf -- '---\nname: agentsync\ndescription: Project copy\n---\n' > .ai/src/skills/agentsync/SKILL.md
    assert_parity check
}

@test "parity: check with an active profile overlay" {
    enable_tools claude
    _run_engine 0 profile add hub --tools claude,codex >/dev/null
    mkdir -p .ai/profiles/hub/src/rules
    printf '# Hub only\n' > .ai/profiles/hub/src/rules/hub.md
    _bash_sync
    assert_parity check
    printf '# Hub changed\n' > .ai/profiles/hub/src/rules/hub.md
    assert_parity check
}

@test "parity: check with composed OpenCode MCP" {
    enable_tools opencode
    mkdir -p .ai/src/tools/opencode
    printf '%s\n' '{"$schema":"https://opencode.ai/config.json","theme":"system"}' > .ai/src/tools/opencode/settings.json
    printf '%s\n' '{"mcpServers":{"github":{"command":"npx","args":["-y","@github/mcp"],"env":{"TOKEN":"${GITHUB_TOKEN}"}},"docs":{"type":"sse","url":"https://example.test/mcp","headers":{"Authorization":"Bearer {env:TOKEN}"},"enabled":false,"timeout":9000,"oauth":false}}}' > .ai/src/mcp.json
    _bash_sync
    assert_parity check
}

@test "parity: check when the OpenCode MCP source is malformed" {
    enable_tools opencode claude
    _bash_sync
    printf '%s\n' '{"mcpServers":[]}' > .ai/src/mcp.json
    assert_parity_head 3 check
}

@test "parity: check with legacy, declared, per-tool, and shared payload sources" {
    enable_tools claude cursor windsurf junie
    mkdir -p .ai/src/mcp .ai/src/tools/cursor config
    echo '{"legacy":true}' > .ai/src/mcp/claude.json
    echo '{"per-tool":true}' > .ai/src/tools/cursor/hooks.json
    echo '{"shared":true}' > .ai/src/mcp.json
    echo '{"declared":true}' > config/windsurf-mcp.json
    printf 'targets:\n  mcp:\n    source: "config/windsurf-mcp.json"\n' > .ai/src/tools/windsurf.yaml
    _bash_sync
    assert_parity check
}

@test "parity: check with skills filters and a custom tool using every rules option" {
    enable_tools claude mytool
    mkdir -p .ai/src/skills/keepme .ai/src/skills/dropme .ai/src/tools
    echo "k" > .ai/src/skills/keepme/SKILL.md
    echo "d" > .ai/src/skills/dropme/SKILL.md
    printf 'targets:\n  skills:\n    exclude:\n      - dropme\n  rules:\n    include: [core.md, git.md]\n' > .ai/src/tools/claude.yaml
    cat > .ai/src/tools/mytool.yaml <<'YAML'
name: "My Tool"
targets:
  agents:
    dest: ".mytool/rules/AGENTS.md"
  rules:
    dest: ".mytool/rules"
    extension: ".txt"
    header: "---\nalways: true\n---"
    append_imports: true
  skills:
    dest: ".mytool/skills"
    include: "keep*"
  commands:
    inline_into_agents: true
  subagents:
    dest: ".mytool/agents"
    format: amazonq_json
YAML
    _bash_sync
    assert_parity check
}

@test "parity: check with a committed version pin mismatch" {
    enable_tools claude
    printf '\noutputs: committed\nagentsync_version: "0.0.1"\n' >> .ai/agent_sync.yaml
    assert_parity check
}

@test "parity: check without AGENTS.md" {
    enable_tools claude
    rm -f .ai/src/AGENTS.md
    assert_parity_head 3 check
}

@test "parity: check without a .ai directory" {
    rm -rf .ai
    assert_parity_head 2 check
}

@test "parity: config selection and version_pin.mode in check and list" {
    enable_tools claude
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_parity check
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_parity list
    mkdir -p config
    cp .ai/agent_sync.yaml config/agentsync.yaml
    AGENTSYNC_CONFIG_PATH=config/agentsync.yaml _bash_sync
    AGENTSYNC_CONFIG_PATH=config/agentsync.yaml assert_parity check
    AGENTSYNC_CONFIG_PATH=config/agentsync.yaml assert_parity list
    printf 'tools:\n  enabled: [claude]\noutputs: local\nagentsync_version: "0.0.1"\nversion_pin:\n  mode: strict\n' > .ai/agent_sync.yaml
    assert_parity check
    printf 'version_pin: nope\n' > .ai/agent_sync.yaml
    assert_parity check
    printf 'gitignore:\n  update: false\nagentsync_version: "0.0.1"\n' > .ai/agent_sync.yaml
    assert_parity check
}

@test "parity: check with this repository's own .ai/src and all 13 tools" {
    rm -rf .ai/src
    cp -R "$REPO_ROOT/.ai/src" .ai/src
    enable_tools "${ALL_TOOLS[@]}"
    _bash_sync
    assert_parity check
}

# ── sync and rollback ────────────────────────────────────────────────────────
# Both commands change the project, so each engine runs in its own copy and
# the copies must end identical. Output differs only where the run names its
# own machinery: the project copy, backup ids, the engine checkout Bash read
# templates from, and the temporary overlay directories.

# Usage: _mask_run_paths <dir> < output
_mask_run_paths() {
    local dir="$1" text from
    local root_mask="<root>" engine_mask="<engine>/lib/templates"
    text=$(cat)
    # Bash 3.2 splits a literal pattern at its first slash, so patterns go
    # through variables.
    from=$(cd -P "$dir" && pwd)
    text=${text//"$from"/$root_mask}
    text=${text//"$dir"/$root_mask}
    for from in "$REPO_ROOT/lib/templates" "~/${REPO_ROOT#"$HOME"/}/lib/templates" "/<agentsync>/lib/templates"; do
        text=${text//"$from"/$engine_mask}
    done
    printf '%s\n' "$text" | sed -E \
        -e 's#[^ ]*/agentsync_shared\.[A-Za-z0-9]+/src#<overlay>/src#g' \
        -e 's#/<agentsync-overlay>/[a-z-]+/src#<overlay>/src#g' \
        -e 's#[0-9]{8}T[0-9]{6}Z-(sync|rollback|init)-[0-9]+(-[0-9]+)?#<backup-id>#g'
}

# Usage: _assert_same_trees <left> <right>
# Every path outside .git and the backup store, and every file's bytes.
_assert_same_trees() {
    local left="$1" right="$2" rel
    (cd "$left" && find . \( -path '*/.git' -o -path '*/.ai/backups' \) -prune -o -print | LC_ALL=C sort) > "$left.tree"
    (cd "$right" && find . \( -path '*/.git' -o -path '*/.ai/backups' \) -prune -o -print | LC_ALL=C sort) > "$right.tree"
    if ! diff "$left.tree" "$right.tree" >&2; then
        return 1
    fi
    while IFS= read -r rel; do
        [[ -f "$left/$rel" ]] || continue
        if ! cmp -s "$left/$rel" "$right/$rel"; then
            echo "content differs: $rel" >&2
            diff "$left/$rel" "$right/$rel" >&2 || true
            return 1
        fi
    done < "$left.tree"
}

# Usage: [PARITY_CWD=<subdir>] assert_tree_parity <agentsync args...>
# Runs the command in a copy of the project per engine, from PARITY_CWD
# inside it, and compares status, masked output, and the resulting trees.
assert_tree_parity() {
    local left="$BATS_TEST_TMPDIR/bash" right="$BATS_TEST_TMPDIR/native"
    rm -rf "$left" "$right"
    cp -pR "$TEST_PROJECT" "$left"
    cp -pR "$TEST_PROJECT" "$right"
    local bash_out native_out bash_rc=0 native_rc=0
    bash_out=$(cd "$left/${PARITY_CWD:-.}" && _run_engine 0 "$@" 2>&1) || bash_rc=$?
    native_out=$(cd "$right/${PARITY_CWD:-.}" && _run_engine 1 "$@" 2>&1) || native_rc=$?
    if [[ "$bash_rc" -ne "$native_rc" ]]; then
        echo "exit status differs: bash=$bash_rc native=$native_rc" >&2
        printf '%s\n' "$native_out" >&2
        return 1
    fi
    printf '%s\n' "$bash_out" | _mask_run_paths "$left" > "$left.out"
    printf '%s\n' "$native_out" | _mask_run_paths "$right" > "$right.out"
    if ! diff "$left.out" "$right.out" >&2; then
        return 1
    fi
    _assert_same_trees "$left" "$right"
}

@test "parity: sync writes a fresh project and its manifest, .gitignore, and backup" {
    enable_tools claude codex cursor
    printf '# Hand-written\n' > AGENTS.md
    printf '\noutputs: local\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
}

@test "parity: sync of every tool and this repository's own .ai/src" {
    rm -rf .ai/src
    cp -R "$REPO_ROOT/.ai/src" .ai/src
    enable_tools "${ALL_TOOLS[@]}"
    assert_tree_parity sync
}

@test "parity: sync dry-run, --only, --skip, and option errors" {
    enable_tools claude codex gemini
    assert_tree_parity sync --dry-run
    assert_tree_parity sync --only codex,gemini --skip gemini
    assert_tree_parity sync --only
    assert_tree_parity sync --bogus
    assert_tree_parity sync --help
}

@test "parity: sync refuses a manual edit, keeps untracked outputs, and prunes what it generated" {
    enable_tools claude cursor
    mkdir -p .ai/src/skills/temp-skill
    printf -- '---\nname: temp-skill\n---\n' > .ai/src/skills/temp-skill/SKILL.md
    printf '# Temp\n' > .ai/src/rules/temp.md
    _bash_sync
    rm -rf .ai/src/skills/temp-skill .ai/src/rules/temp.md
    printf 'mine\n' > .claude/rules/mine.md
    mkdir -p .claude/skills/mine
    printf 'mine\n' > .claude/skills/mine/SKILL.md
    assert_tree_parity sync --dry-run
    assert_tree_parity sync
    printf 'edited\n' >> CLAUDE.md
    assert_tree_parity sync
    assert_tree_parity sync --force
}

@test "parity: sync cleans a disabled tool and follows the outputs mode" {
    enable_tools claude cursor codex
    _bash_sync
    _run_engine 0 disable cursor >/dev/null
    printf '\noutputs: committed\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
    printf '\ngitignore:\n  update: false\n' >> .ai/agent_sync.yaml
    sed 's/^outputs: committed$//' .ai/agent_sync.yaml > .ai/agent_sync.yaml.tmp
    mv .ai/agent_sync.yaml.tmp .ai/agent_sync.yaml
    assert_tree_parity sync
    printf '\noutputs: shared\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
}

@test "parity: sync with profiles, shared inheritance, and a version pin" {
    enable_tools claude
    _run_engine 0 profile add hub --tools claude,codex >/dev/null
    mkdir -p .ai/profiles/hub/src/rules parent/.ai/src/rules parent/.ai/src/skills/parent-skill
    printf '# Hub only\n' > .ai/profiles/hub/src/rules/hub.md
    printf '# Parent\n' > parent/.ai/src/rules/parent.md
    printf 'p\n' > parent/.ai/src/skills/parent-skill/SKILL.md
    printf '\nshared:\n  path: parent\n  inherit: rules, skills, tools\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
    assert_tree_parity sync --profile hub --only claude-hub
    printf '\nagentsync_version: "0.0.1"\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
}

@test "parity: sync --if-stale, post-sync hooks, and a failed run's restore" {
    enable_tools claude opencode
    _bash_sync
    touch -t 203001010000 .ai/.sync-manifest
    assert_tree_parity sync --if-stale
    touch -t 200001010000 .ai/.sync-manifest
    assert_tree_parity sync --if-stale
    mkdir -p .ai/src/tools
    printf 'post_sync: "printf hooked > hooked.txt"\n' > .ai/src/tools/claude.yaml
    assert_tree_parity sync
    AGENTSYNC_ALLOW_POST_SYNC=true assert_tree_parity sync
    printf 'post_sync: "false"\n' > .ai/src/tools/claude.yaml
    AGENTSYNC_ALLOW_POST_SYNC=true assert_tree_parity sync
    printf '%s\n' '{"mcpServers":[]}' > .ai/src/mcp.json
    rm -f .ai/src/tools/claude.yaml
    assert_tree_parity sync
}

@test "parity: sync from inside .ai, into a symlinked dest, and across a workspace" {
    enable_tools claude
    PARITY_CWD=.ai/src assert_tree_parity sync
    mkdir -p "$BATS_TEST_TMPDIR/outside"
    create_test_symlink "$BATS_TEST_TMPDIR/outside" .claude
    assert_tree_parity sync
    rm -f .claude
    mkdir -p leaf
    (cd leaf && git init --quiet && _run_engine 0 init --no-detect --no-sync >/dev/null 2>&1)
    (cd leaf && _run_engine 0 enable cursor --no-scaffold >/dev/null)
    assert_tree_parity sync --workspace --dry-run
    assert_tree_parity sync --workspace --only cursor
}

@test "parity: sync fails closed on config selection and enforces version_pin.mode" {
    enable_tools claude
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_tree_parity sync
    printf 'tools:\n  enabled: [claude]\noutputs: local\nagentsync_version: "0.0.1"\nversion_pin: strict\n' > .ai/agent_sync.yaml
    assert_tree_parity sync
    printf 'tools:\n  enabled: [claude]\nversion_pin:\n  mode: refuse\n' > .ai/agent_sync.yaml
    assert_tree_parity sync
    rm .ai/agent_sync.yaml
    assert_tree_parity sync
    assert_tree_parity sync --dry-run
    mkdir -p .ai/src/tools
    printf 'enabled: true\n' > .ai/src/tools/claude.yaml
    assert_tree_parity sync
}

@test "parity: backup.retention in sync and rollback" {
    enable_tools claude
    mkdir -p .ai/backups/20200101T000000Z-sync-1/files .ai/backups/.tmp.sync.abandoned
    printf 'schema=1\noperation=sync\ncreated_at=20200101T000000Z\n' > .ai/backups/20200101T000000Z-sync-1/metadata
    : > .ai/backups/20200101T000000Z-sync-1/targets.tsv
    : > .ai/backups/20200101T000000Z-sync-1/.complete
    touch -t 202001010000 .ai/backups/.tmp.sync.abandoned
    printf 'backup:\n  retention: typo\n' >> .ai/agent_sync.yaml
    assert_tree_parity sync
    sed 's/retention: typo/retention: preserve/' .ai/agent_sync.yaml > .ai/agent_sync.yaml.tmp
    mv .ai/agent_sync.yaml.tmp .ai/agent_sync.yaml
    AGENTSYNC_BACKUP_LIMIT=1 AGENTSYNC_BACKUP_MAX_AGE_DAYS=1 assert_tree_parity sync
    AGENTSYNC_BACKUP_LIMIT=typo assert_tree_parity sync
    _bash_sync
    AGENTSYNC_BACKUP_LIMIT=1 assert_tree_parity rollback --yes
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_tree_parity rollback --yes
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_tree_parity rollback --list
}

@test "parity: sources outside the project and escaping source links" {
    enable_tools claude
    local outside="$BATS_TEST_TMPDIR/outside"
    mkdir -p "$outside/rules" "$outside/tools/claude"
    printf '# Outside\n' > "$outside/rules/outside.md"
    printf '{"outside":true}\n' > "$outside/tools/claude/settings.json"
    printf '\nsource:\n  rules: "%s/rules"\n  tools: "%s/tools"\n' "$outside" "$outside" >> .ai/agent_sync.yaml
    assert_tree_parity sync
    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$outside" assert_tree_parity sync
    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$outside" _bash_sync
    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$outside" assert_parity check
    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$outside" assert_parity list
    grep -v '^  rules:\|^  tools:\|^source:' .ai/agent_sync.yaml > .ai/agent_sync.yaml.tmp
    mv .ai/agent_sync.yaml.tmp .ai/agent_sync.yaml
    mkdir -p .ai/src/rules
    create_test_symlink "$outside/rules/outside.md" .ai/src/rules/leak.md
    assert_tree_parity sync
    assert_parity check
}

@test "parity: rollback plans, restores, and refuses like Bash" {
    enable_tools claude
    printf 'before-sync\n' > CLAUDE.md
    _bash_sync
    assert_tree_parity rollback --dry-run
    assert_tree_parity rollback --list
    assert_tree_parity rollback
    assert_tree_parity rollback --yes
    assert_tree_parity rollback "../$(cat .ai/backups/.latest)" --yes
    assert_tree_parity rollback nope
    assert_tree_parity rollback --bogus
    assert_tree_parity rollback --help
}

@test "parity: rollback conflicts, --force, and unsealed snapshots" {
    enable_tools claude
    _bash_sync
    printf 'edited\n' >> CLAUDE.md
    assert_tree_parity rollback --yes
    assert_tree_parity rollback --dry-run
    assert_tree_parity rollback --dry-run --force
    assert_tree_parity rollback --force --yes
    rm ".ai/backups/$(cat .ai/backups/.latest)/after.tsv"
    assert_tree_parity rollback --yes
    printf 'post-state-v2\tbad\n' > ".ai/backups/$(cat .ai/backups/.latest)/after.tsv"
    assert_tree_parity rollback --yes
}

@test "parity: a backup the native sync writes is restored by the Bash rollback" {
    enable_tools claude cursor
    printf 'before-sync\n' > CLAUDE.md
    local left="$BATS_TEST_TMPDIR/bash-made" right="$BATS_TEST_TMPDIR/native-made"
    cp -pR "$TEST_PROJECT" "$left"
    cp -pR "$TEST_PROJECT" "$right"
    (cd "$left" && _run_engine 0 sync >/dev/null 2>&1)
    (cd "$right" && _run_engine 1 sync >/dev/null 2>&1)
    [ "$(cat "$right/.ai/backups/$(cat "$right/.ai/backups/.latest")/targets.tsv")" = \
      "$(cat "$left/.ai/backups/$(cat "$left/.ai/backups/.latest")/targets.tsv")" ]
    cmp "$right/.ai/backups/$(cat "$right/.ai/backups/.latest")/after.tsv" \
        "$left/.ai/backups/$(cat "$left/.ai/backups/.latest")/after.tsv"
    (cd "$left" && _run_engine 0 rollback --yes >/dev/null 2>&1)
    (cd "$right" && _run_engine 0 rollback --yes >/dev/null 2>&1)
    _assert_same_trees "$left" "$right"
    [ "$(cat "$right/CLAUDE.md")" = "before-sync" ]
}

# ── enable / disable ─────────────────────────────────────────────────────────

@test "parity: enable scaffolds, reports, and refuses like Bash" {
    assert_tree_parity enable claude windsurf
    assert_tree_parity enable
    assert_tree_parity enable --bogus claude
    assert_tree_parity enable --help
    assert_tree_parity enable claude bogus_tool --no-scaffold
    assert_tree_parity enable -- claude
    mkdir -p .ai/src/settings
    printf '{"legacy": true}\n' > .ai/src/settings/claude.json
    printf '{"mcpServers":{}}\n' > .ai/src/mcp.json
    assert_tree_parity enable claude cursor --scaffold
    _run_engine 0 enable claude >/dev/null
    assert_tree_parity enable claude cursor
}

@test "parity: disable edits block and inline lists and legacy flags like Bash" {
    _run_engine 0 enable claude cursor >/dev/null
    mkdir -p .ai/src/tools
    printf 'enabled: true\n' > .ai/src/tools/kimi.yaml
    assert_tree_parity disable claude kimi bogus_tool
    assert_tree_parity disable
    assert_tree_parity disable zed
    printf 'tools:\n  enabled: [claude, cursor]\n' > .ai/agent_sync.yaml
    assert_tree_parity disable cursor
    printf 'format: 2\ntools:\n  other: x\n' > .ai/agent_sync.yaml
    assert_tree_parity enable claude
    rm .ai/agent_sync.yaml
    assert_tree_parity disable claude
    assert_tree_parity enable claude claude
}

@test "parity: enable and disable with an explicit config and outside source.tools" {
    local outside="$BATS_TEST_TMPDIR/outside"
    mkdir -p "$outside" config
    printf 'enabled: true\n' > "$outside/kimi.yaml"
    printf 'tools:\n  enabled: [claude]\nsource:\n  tools: "%s"\n' "$outside" > config/a.yaml
    AGENTSYNC_CONFIG_PATH=config/a.yaml assert_tree_parity enable cursor --scaffold
    AGENTSYNC_CONFIG_PATH=config/a.yaml assert_tree_parity enable cursor claude
    AGENTSYNC_CONFIG_PATH=config/a.yaml assert_tree_parity disable claude
    AGENTSYNC_CONFIG_PATH=config/a.yaml assert_tree_parity disable kimi
    AGENTSYNC_CONFIG_PATH=missing.yaml assert_tree_parity enable claude
}

# ── customize / show / diff ──────────────────────────────────────────────────

@test "parity: customize scaffolds tools and payloads like Bash" {
    enable_tools cursor
    assert_tree_parity customize
    assert_tree_parity customize claude
    assert_tree_parity customize cursor --full
    assert_tree_parity customize claude nope
    assert_tree_parity customize nope mcp
    assert_tree_parity customize nope --full
    assert_tree_parity customize a b c
    assert_tree_parity customize --bogus
    assert_tree_parity customize --help
    assert_tree_parity customize cursor hooks
    assert_tree_parity customize cursor hooks --yes
    mkdir -p .ai/src/mcp
    printf '{"marker":"USER"}\n' > .ai/src/mcp/claude.json
    assert_tree_parity customize claude mcp
    _run_engine 0 customize claude >/dev/null
    assert_tree_parity customize claude
    printf 'tools:\n  enabled: [cursor]\nsource:\n  tools: "%s/outside"\n' "$BATS_TEST_TMPDIR" > .ai/agent_sync.yaml
    mkdir -p "$BATS_TEST_TMPDIR/outside"
    assert_tree_parity customize codex
}

@test "parity: show prints effective tools and payload sources like Bash" {
    enable_tools cursor
    assert_tree_parity show
    assert_tree_parity show claude
    assert_tree_parity show claude --base
    assert_tree_parity show nope
    assert_tree_parity show nope --base
    assert_tree_parity show claude nope
    assert_tree_parity show a b c
    assert_tree_parity show claude --help
    assert_tree_parity show cursor hooks
    assert_tree_parity show cursor hooks --base
    assert_tree_parity show claude settings
    assert_tree_parity show zed hooks
    _run_engine 0 customize cursor --full >/dev/null
    printf 'targets:\n  rules:\n    dest: ".custom/rules"\n' > .ai/src/tools/claude.yaml
    mkdir -p .ai/src/hooks
    printf '{"legacy":true}\n' > .ai/src/hooks/cursor.json
    assert_tree_parity show claude
    assert_tree_parity show cursor
    assert_tree_parity show cursor hooks
    printf '{"mcpServers":{}}\n' > .ai/src/mcp.json
    assert_tree_parity show claude mcp
}

@test "parity: diff reports overrides and payload hunks like Bash" {
    enable_tools cursor
    assert_tree_parity diff
    assert_tree_parity diff claude
    assert_tree_parity diff claude hooks
    assert_tree_parity diff a b c
    assert_tree_parity diff --bogus
    assert_tree_parity diff claude --help
    _run_engine 0 customize cursor --full >/dev/null
    _run_engine 0 customize claude >/dev/null
    printf 'name: "My Claude"\ntargets:\n  rules:\n    dest: ".custom/rules"\n' >> .ai/src/tools/claude.yaml
    printf 'name: "Mine"\n' > .ai/src/tools/mytool.yaml
    assert_tree_parity diff
    assert_tree_parity diff claude
    assert_tree_parity diff zed
    assert_tree_parity diff claude settings
    assert_tree_parity diff cursor nope
    assert_tree_parity diff nope mcp
    _run_engine 0 customize cursor hooks --yes >/dev/null
    assert_tree_parity diff cursor hooks
    printf '{\n  "version": 2,\n  "hooks": {"afterFileEdit": []}\n}\n' > .ai/src/tools/cursor/hooks.json
    assert_tree_parity diff cursor hooks
    mkdir -p .ai/src/mcp
    printf '{"legacy":true}\n' > .ai/src/mcp/claude.json
    assert_tree_parity diff claude mcp
}
