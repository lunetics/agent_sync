#!/usr/bin/env bats
# Recovery policy through real commands, including failure/rollback paths.
load test_helper

setup() {
    setup_test_project
    unset AGENTSYNC_BACKUP_LIMIT AGENTSYNC_BACKUP_MAX_AGE_DAYS AGENTSYNC_CONFIG_PATH
    source "$REPO_ROOT/lib/helpers/paths.sh"
    source "$REPO_ROOT/lib/helpers/backup.sh"
    RETENTION_EVIDENCE="$(mktemp -d "${TMPDIR:-/tmp}/agentsync_retention_evidence.XXXXXX")"
}

teardown() {
    # Keep the complete before/restore evidence outside the disposable project.
    teardown_test_project
}

init_project() {
    run_agentsync init --tools claude --yes --no-sync >/dev/null
    printf 'before-sync\n' > CLAUDE.md
}

set_retention() {
    printf '\nbackup:\n  retention: %s\n' "$1" >> .ai/agent_sync.yaml
}

seed_recovery() {
    local id
    for id in 20200101T000000Z-sync-old 20990101T000000Z-sync-count; do
        mkdir -p ".ai/backups/$id/files"
        printf 'schema=1\noperation=sync\ncreated_at=%s\n' "${id%%-*}" > ".ai/backups/$id/metadata"
        printf 'present\tCLAUDE.md\n' > ".ai/backups/$id/targets.tsv"
        printf 'recovered-output\n' > ".ai/backups/$id/files/CLAUDE.md"
        : > ".ai/backups/$id/.complete"
    done
    mkdir -p .ai/backups/.tmp.sync.old/files .ai/backups/.tmp.sync.fresh/files .ai/backups/unrelated
    printf 'partial recovery\n' > .ai/backups/.tmp.sync.old/files/sentinel
    printf 'old pointer\n' > .ai/backups/.latest.tmp.old
    printf 'old ignore\n' > .ai/backups/.gitignore.tmp.old
    printf 'foreign content\n' > .ai/backups/unrelated/sentinel
    touch -t 202001010000 .ai/backups/.tmp.sync.old .ai/backups/.latest.tmp.old .ai/backups/.gitignore.tmp.old
}

save_and_verify_project() {
    local label="$1"
    tar -cpf "$RETENTION_EVIDENCE/$label.tar" .
    mkdir "$RETENTION_EVIDENCE/$label-restore"
    tar -xpf "$RETENTION_EVIDENCE/$label.tar" -C "$RETENTION_EVIDENCE/$label-restore"
    diff -qr . "$RETENTION_EVIDENCE/$label-restore"
}

assert_recovery_preserved() {
    local entry
    for entry in 20200101T000000Z-sync-old 20990101T000000Z-sync-count .tmp.sync.old .tmp.sync.fresh .latest.tmp.old .gitignore.tmp.old unrelated; do
        diff -r "$RETENTION_EVIDENCE/before-restore/.ai/backups/$entry" ".ai/backups/$entry"
    done
}

@test "retention default sync still sweeps staging when both snapshot limits are zero" {
    init_project
    seed_recovery
    save_and_verify_project before
    AGENTSYNC_BACKUP_LIMIT=0 AGENTSYNC_BACKUP_MAX_AGE_DAYS=0 run run_agentsync sync --only claude
    [ "$status" -eq 0 ]
    [ ! -e .ai/backups/.tmp.sync.old ]
    [ ! -e .ai/backups/.latest.tmp.old ]
    [ ! -e .ai/backups/.gitignore.tmp.old ]
    [ -d .ai/backups/.tmp.sync.fresh ]
    [ -f .ai/backups/unrelated/sentinel ]
    [ -d .ai/backups/20200101T000000Z-sync-old ]
}

@test "retention bounded sync applies age and count limits and sweeps old staging" {
    init_project
    set_retention bounded
    seed_recovery
    save_and_verify_project before
    AGENTSYNC_BACKUP_LIMIT=1 AGENTSYNC_BACKUP_MAX_AGE_DAYS=30 run run_agentsync sync --only claude
    [ "$status" -eq 0 ]
    [ ! -d .ai/backups/20200101T000000Z-sync-old ]
    [ ! -d .ai/backups/20990101T000000Z-sync-count ]
    [ ! -e .ai/backups/.tmp.sync.old ]
    [ ! -e .ai/backups/.latest.tmp.old ]
    [ ! -e .ai/backups/.gitignore.tmp.old ]
    [ -d .ai/backups/.tmp.sync.fresh ]
    [ -f .ai/backups/unrelated/sentinel ]
}

@test "retention preserve sync keeps snapshots and every staging kind despite low limits" {
    init_project
    set_retention preserve
    seed_recovery
    save_and_verify_project before
    AGENTSYNC_BACKUP_LIMIT=1 AGENTSYNC_BACKUP_MAX_AGE_DAYS=1 run run_agentsync sync --only claude
    [ "$status" -eq 0 ]
    assert_recovery_preserved
    [ "$(cat CLAUDE.md)" != "before-sync" ]
    local snapshot
    snapshot="$(cat .ai/backups/.latest)"
    [ "$(cat ".ai/backups/$snapshot/files/CLAUDE.md")" = before-sync ]
    # Verify AgentSync's own restore, in addition to the complete tar restore.
    save_and_verify_project before-engine-restore
    backup_restore "$TEST_PROJECT" "$snapshot"
    [ "$(cat CLAUDE.md)" = before-sync ]
}

@test "retention preserve sync with zero limits also keeps abandoned staging" {
    init_project
    set_retention preserve
    seed_recovery
    save_and_verify_project before
    AGENTSYNC_BACKUP_LIMIT=0 AGENTSYNC_BACKUP_MAX_AGE_DAYS=0 run run_agentsync sync --only claude
    [ "$status" -eq 0 ]
    assert_recovery_preserved
}

@test "retention invalid or empty values reject sync before any project mutation" {
    init_project
    seed_recovery
    cp .ai/agent_sync.yaml "$RETENTION_EVIDENCE/config-original"
    local value
    for value in typo false 0 null '[]' '{}' '""' ''; do
        cp "$RETENTION_EVIDENCE/config-original" .ai/agent_sync.yaml
        set_retention "$value"
        local evidence
        evidence="$(mktemp -d "$RETENTION_EVIDENCE/invalid.XXXXXX")"
        tar -cpf "$evidence/before.tar" .
        mkdir "$evidence/restore"
        tar -xpf "$evidence/before.tar" -C "$evidence/restore"
        diff -qr . "$evidence/restore"
        run run_agentsync sync --only claude
        [ "$status" -ne 0 ]
        [[ "$output" == *"backup.retention"* ]]
        diff -qr . "$evidence/restore"
    done
}

@test "retention malformed backup section rejects sync before changes" {
    init_project
    printf '\nbackup: preserve\n' >> .ai/agent_sync.yaml
    seed_recovery
    save_and_verify_project before
    run run_agentsync sync --only claude
    [ "$status" -ne 0 ]
    [[ "$output" == *"backup.retention"* ]]
    diff -qr . "$RETENTION_EVIDENCE/before-restore"
}

@test "retention invalid init config fails before scaffolding or recovery changes" {
    mkdir -p .ai
    printf 'backup:\n  retention: typo\n' > .ai/agent_sync.yaml
    seed_recovery
    save_and_verify_project before
    run run_agentsync init --tools claude --yes --no-sync
    [ "$status" -ne 0 ]
    [[ "$output" == *"backup.retention"* ]]
    diff -qr . "$RETENTION_EVIDENCE/before-restore"
}

@test "retention preserve init keeps existing snapshots and staging" {
    mkdir -p .ai
    printf 'backup:\n  retention: preserve\n' > .ai/agent_sync.yaml
    seed_recovery
    save_and_verify_project before
    AGENTSYNC_BACKUP_LIMIT=1 AGENTSYNC_BACKUP_MAX_AGE_DAYS=1 run run_agentsync init --tools claude --yes --no-sync
    [ "$status" -eq 0 ]
    assert_recovery_preserved
    [ -f .ai/src/AGENTS.md ]
}

@test "retention preserve failed sync restores outputs without pruning recovery" {
    init_project
    set_retention preserve
    mkdir -p .ai/src/tools
    printf 'post_sync: "false"\n' > .ai/src/tools/claude.yaml
    seed_recovery
    save_and_verify_project before
    AGENTSYNC_ALLOW_POST_SYNC=true AGENTSYNC_BACKUP_LIMIT=1 AGENTSYNC_BACKUP_MAX_AGE_DAYS=1 run run_agentsync sync --only claude
    [ "$status" -ne 0 ]
    [[ "$output" == *"Restored pre-sync state"* ]]
    [ "$(cat CLAUDE.md)" = before-sync ]
    [ ! -e .ai/.sync-manifest ]
    assert_recovery_preserved
}

@test "retention preserve rollback retains its policy when restoring a bounded config" {
    init_project
    printf '\nbackup:\n  retention: bounded\n' >> .ai/agent_sync.yaml
    local snapshot
    snapshot="$(backup_create "$TEST_PROJECT" sync .ai/agent_sync.yaml CLAUDE.md)"
    # This intentional fixture edit is preceded by a full, verified backup.
    save_and_verify_project before-config-change
    sed -i 's/retention: bounded/retention: preserve/' .ai/agent_sync.yaml
    printf 'current-output\n' > CLAUDE.md
    seed_recovery
    save_and_verify_project before
    AGENTSYNC_BACKUP_LIMIT=1 AGENTSYNC_BACKUP_MAX_AGE_DAYS=1 run run_agentsync rollback "$(basename "$snapshot")" --yes
    [ "$status" -eq 0 ]
    [ "$(cat CLAUDE.md)" = before-sync ]
    grep -q 'retention: bounded' .ai/agent_sync.yaml
    assert_recovery_preserved
    [ -d "$snapshot" ]
}

@test "retention invalid rollback config fails before safety snapshot or restore" {
    init_project
    set_retention typo
    seed_recovery
    save_and_verify_project before
    run run_agentsync rollback 20200101T000000Z-sync-old --yes
    [ "$status" -ne 0 ]
    [[ "$output" == *"backup.retention"* ]]
    diff -qr . "$RETENTION_EVIDENCE/before-restore"
}

@test "retention uses the explicit config instead of a conflicting local policy" {
    init_project
    set_retention bounded
    seed_recovery
    printf 'tools:\n  enabled: [claude]\nbackup:\n  retention: "preserve" # keep recovery\n' > selected.yaml
    save_and_verify_project before
    AGENTSYNC_CONFIG_PATH=selected.yaml AGENTSYNC_BACKUP_LIMIT=1 run run_agentsync sync --only claude
    [ "$status" -eq 0 ]
    assert_recovery_preserved
}

@test "retention invalid numeric bounds reject sync before changes" {
    init_project
    seed_recovery
    save_and_verify_project before
    AGENTSYNC_BACKUP_LIMIT=typo run run_agentsync sync --only claude
    [ "$status" -ne 0 ]
    [[ "$output" == *"Backup limit must be"* ]]
    diff -qr . "$RETENTION_EVIDENCE/before-restore"
    AGENTSYNC_BACKUP_MAX_AGE_DAYS=-1 run run_agentsync sync --only claude
    [ "$status" -ne 0 ]
    [[ "$output" == *"Backup max age must be"* ]]
    diff -qr . "$RETENTION_EVIDENCE/before-restore"
}

@test "retention configured helper preserves staging and resets to bounded for the next project" {
    init_project
    set_retention preserve
    seed_recovery
    save_and_verify_project before
    backup_configure "$TEST_PROJECT"
    _backup_sweep_stale_staging "$TEST_PROJECT/.ai/backups"
    backup_prune "$TEST_PROJECT" 1 1
    assert_recovery_preserved
    mkdir "$RETENTION_EVIDENCE/empty-project"
    backup_configure "$RETENTION_EVIDENCE/empty-project"
    [ "$BACKUP_RETENTION_MODE" = bounded ]
}

@test "retention invalid explicit config rejects init and rollback without fallback" {
    init_project
    seed_recovery
    save_and_verify_project before
    AGENTSYNC_CONFIG_PATH=missing.yaml run run_agentsync init --tools claude --yes --no-sync
    [ "$status" -ne 0 ]
    [[ "$output" == *"AGENTSYNC_CONFIG_PATH is set but file not found"* ]]
    diff -qr . "$RETENTION_EVIDENCE/before-restore"
    AGENTSYNC_CONFIG_PATH=missing.yaml run run_agentsync rollback 20200101T000000Z-sync-old --yes
    [ "$status" -ne 0 ]
    [[ "$output" == *"AGENTSYNC_CONFIG_PATH is set but file not found"* ]]
    diff -qr . "$RETENTION_EVIDENCE/before-restore"
}
