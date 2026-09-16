#!/usr/bin/env bats
# Tests for agentsync export and import: a bundle round trip, a directory
# source, and a GitHub archive served by a curl stand-in on PATH.

load test_helper

setup_file() { seed_project; }
teardown_file() { teardown_seed_project; }
setup() { clone_seed; }
teardown() { teardown_test_project; }

# Put a `curl` on PATH that serves $TEST_PROJECT/github/<owner>_<repo>-<branch>.tar.gz
# for the archive URL import builds, and fails like `curl -f` otherwise.
github_stub() {
    mkdir -p stub github
    cat > stub/curl <<'EOF'
#!/usr/bin/env bash
out=""; url=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        --max-time) shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
    esac
done
name="${url##*/archive/refs/heads/}"
repo="${url#https://github.com/}"; repo="${repo%%/archive/*}"
file="$FAKE_GITHUB_DIR/${repo//\//_}-$name"
[ -f "$file" ] || exit 22
cp "$file" "$out"
EOF
    chmod +x stub/curl
    export FAKE_GITHUB_DIR="$TEST_PROJECT/github"
    export PATH="$TEST_PROJECT/stub:$PATH"
}

# An archive as GitHub serves one: the repository under <repo>-<branch>/.
github_archive() {
    local owner_repo="$1" branch="$2" top
    top="${owner_repo#*/}-$branch"
    mkdir -p "gh/$top/.ai/src/rules"
    printf '# From %s\n' "$branch" > "gh/$top/.ai/src/AGENTS.md"
    printf '# GH rule\n' > "gh/$top/.ai/src/rules/gh.md"
    (cd gh && tar -czf "$FAKE_GITHUB_DIR/${owner_repo//\//_}-$branch.tar.gz" "$top")
}

@test "export writes the bundle and lists its contents" {
    run run_agentsync export
    [ "$status" -eq 0 ]
    grep -qF -- "AGENTS.md" <<<"$output"
    grep -qF -- "rules/ (" <<<"$output"
    grep -qF -- "Exported!" <<<"$output"
    [ -f agentsync-bundle.tar.gz ]
    tar -tzf agentsync-bundle.tar.gz | grep -q '^.ai/src/AGENTS.md$'
    tar -tzf agentsync-bundle.tar.gz | grep -q '^.ai/agent_sync.yaml$'
}

@test "export --dry-run writes nothing" {
    run run_agentsync export --dry-run
    [ "$status" -eq 0 ]
    grep -qF -- "Dry run" <<<"$output"
    [ ! -e agentsync-bundle.tar.gz ]
}

@test "export sizes a relative archive from the project root" {
    mkdir -p sub
    run bash -c "cd sub && AGENTSYNC_REPO_ROOT='$TEST_PROJECT' AGENTSYNC_HOME='$REPO_ROOT' bash '$AGENTSYNC_BIN' export -o rel.tgz"
    [ "$status" -eq 0 ]
    [ -f rel.tgz ]
    grep -qF -- "rel.tgz (" <<<"$output"
    ! grep -qF -- "(? B)" <<<"$output"
}

@test "export fails without .ai" {
    rm -rf .ai
    run run_agentsync export
    [ "$status" -eq 1 ]
    grep -qF -- "No .ai/ directory found" <<<"$output"
}

@test "import copies a bundle into a fresh project" {
    run_agentsync export -o bundle.tgz >/dev/null
    mkdir -p fresh
    mv bundle.tgz fresh/
    run bash -c "cd fresh && AGENTSYNC_HOME='$REPO_ROOT' bash '$AGENTSYNC_BIN' import bundle.tgz"
    [ "$status" -eq 0 ]
    grep -qF -- "Imported!" <<<"$output"
    [ -f fresh/.ai/src/AGENTS.md ]
    [ -f fresh/.ai/agent_sync.yaml ]
    cmp -s .ai/src/rules/core.md fresh/.ai/src/rules/core.md
}

@test "import reports an up-to-date project" {
    run_agentsync export -o bundle.tgz >/dev/null
    run run_agentsync import bundle.tgz
    [ "$status" -eq 0 ]
    grep -qF -- "Already up to date!" <<<"$output"
}

@test "import --dry-run previews without writing" {
    run_agentsync export -o bundle.tgz >/dev/null
    mkdir -p fresh
    mv bundle.tgz fresh/
    run bash -c "cd fresh && AGENTSYNC_HOME='$REPO_ROOT' bash '$AGENTSYNC_BIN' import bundle.tgz --dry-run"
    [ "$status" -eq 0 ]
    grep -qF -- "Dry run" <<<"$output"
    [ ! -e fresh/.ai ]
}

@test "import --only limits the targets" {
    mkdir -p other/.ai/src/rules other/.ai/src/skills/new
    printf '# Other rule\n' > other/.ai/src/rules/other.md
    printf '# New skill\n' > other/.ai/src/skills/new/SKILL.md
    run run_agentsync import other --only rules
    [ "$status" -eq 0 ]
    [ -f .ai/src/rules/other.md ]
    [ ! -e .ai/src/skills/new ]
}

@test "import --only that matches nothing reports an up-to-date project" {
    run_agentsync export -o bundle.tgz >/dev/null
    run run_agentsync import bundle.tgz --only bogus
    [ "$status" -eq 0 ]
    grep -qF -- "Already up to date!" <<<"$output"
}

@test "import --only previews a config-only change" {
    mkdir -p other/.ai/src/skills/new
    printf '# New skill\n' > other/.ai/src/skills/new/SKILL.md
    printf 'outputs: committed\n' > other/.ai/agent_sync.yaml
    run run_agentsync import other --only rules --dry-run
    [ "$status" -eq 0 ]
    grep -qF -- "~ agent_sync.yaml (update)" <<<"$output"
    grep -qF -- "Summary: 0 new, 1 updated, 0 unchanged" <<<"$output"
    ! grep -qF -- "unbound variable" <<<"$output"
}

@test "import from a directory updates changed files" {
    mkdir -p other/.ai/src/rules
    printf '# Replaced core\n' > other/.ai/src/rules/core.md
    run run_agentsync import other --force
    [ "$status" -eq 0 ]
    grep -qF -- "1 updated" <<<"$output"
    grep -q '^# Replaced core$' .ai/src/rules/core.md
}

@test "import refuses a source without .ai" {
    mkdir -p plain/docs
    printf 'x\n' > plain/docs/readme.md
    run run_agentsync import plain
    [ "$status" -eq 1 ]
    grep -qF -- "No .ai/src/ (or .ai/) directory found in source." <<<"$output"
}

@test "import rejects an unrecognized source" {
    run run_agentsync import nothing.txt
    [ "$status" -eq 1 ]
    grep -qF -- "Cannot recognize source" <<<"$output"
}

@test "import downloads a GitHub archive through curl" {
    github_stub
    github_archive user/repo main
    run run_agentsync import https://github.com/user/repo --force
    [ "$status" -eq 0 ]
    grep -qF -- "Downloading user/repo (branch: main)" <<<"$output"
    grep -qF -- "Downloaded." <<<"$output"
    [ -f .ai/src/rules/gh.md ]
}

@test "import falls back to master when main is missing" {
    github_stub
    github_archive user/repo2 master
    run run_agentsync import https://github.com/user/repo2 --force
    [ "$status" -eq 0 ]
    grep -qF -- "trying 'master'" <<<"$output"
    grep -q '^# From master$' .ai/src/AGENTS.md
}

@test "import reports a branch that cannot be downloaded" {
    github_stub
    run run_agentsync import https://github.com/user/repo --branch nope
    [ "$status" -eq 1 ]
    grep -qF -- "Failed to download branch 'nope'." <<<"$output"
}
