# Rust Migration Phase 5c: The Release Build

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Make a pushed `VERSION` produce a GitHub release with a binary for the five targets: cargo-dist 0.32.0 configured in `dist-workspace.toml` and `Cargo.toml`, the release workflow it generates, the shell and PowerShell installers, sha256 sums, and artifact attestations, with the auto-tag workflow dispatching the build for the tag it creates. Nothing is released by this slice; the first release is the 5e cutover. This is the third of the five Phase 5 slices; 5d's `update` downloads what this build publishes, 5e links the binary the installers place.

**Architecture:** `dist init` wrote the config; the plan carries it whole. `dist-workspace.toml` names the targets, the installers, `~/.agentsync/bin` as the install path, `checksum = "sha256"`, `github-attestations = true`, and `dispatch-releases = true`, so `.github/workflows/release.yml` runs on `workflow_dispatch` with a `tag` input and never on a tag push. `Cargo.toml` opts the crate into dist (`publish = false` hides it otherwise), names the real repository, and adds `[profile.dist]` inheriting `release`. `auto-tag.yaml` keeps creating the annotated tag from `VERSION` and gains a step that dispatches `release.yml` on the tag ref with `gh workflow run`, the one event a `GITHUB_TOKEN` push can start. The seam is `dist` itself: `dist plan --tag 0.36.0` proves the config, `dist generate` proves the workflow (its sha256 is recorded), a host `dist build` proves the archive, its checksum, and the installers, and a hand edit of the workflow proves `dist plan` refuses stale generated files, so the release workflow's own `plan` job, which runs on pull requests too, is the freshness check.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new crate. New developer tool: cargo-dist 0.32.0 (`dist`), installed by the maintainer on 2026-09-18 with `cargo install cargo-dist --locked`. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 5". Previous plan: `docs/plans/2026-09-18-rust-migration-phase-5b-release.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. This slice writes `dist-workspace.toml`, `Cargo.toml`, `.github/workflows/release.yml` (generated, never hand-edited), `.github/workflows/auto-tag.yaml`, the module map, the spec, and `.ai/.sync-manifest`; no other file.
- No binary ships to users until 5e; without a binary every command runs in Bash. No Bash changes: `bin/agentsync.sh`, `install.sh`, and `lib/**/*.sh` are untouched and stay clean under `shellcheck -x -S warning -e SC1091`.
- `.github/workflows/release.yml` is exactly what `dist generate` writes from `dist-workspace.toml` with cargo-dist 0.32.0: sha256 `3350d9e2c86f3aaf096fa03e2b96e6d84261a207132a975e90d1c4cdfda5cb9e`, 308 lines. `dist plan` exits 255 on any hand edit, and the workflow's `plan` job runs `dist` on every pull request, so the check needs no `ci.yaml` step.
- The five targets and nothing else: `aarch64-apple-darwin`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`, `x86_64-pc-windows-msvc`. `dist plan` places each on a native runner (`macos-14`, `ubuntu-22.04-arm`, `macos-15-intel`, `ubuntu-22.04`, `windows-2022`); nothing cross-compiles.
- Tags stay bare (`0.36.0`), as `auto-tag.yaml`, `release`, and `install.sh`'s `AGENTSYNC_VERSION` pin use them. `dist` parses a bare tag (announced as `v0.36.0`); the installers' download URL carries the tag `dist build --tag` is given, so CI, which always passes `--tag`, points at `releases/download/0.36.0`; a local `dist build` without `--tag` points at `v0.36.0` and is not shipped.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency; `Cargo.lock` unchanged. `Cargo.toml` and `Cargo.lock` keep carrying `VERSION` (`src/lib.rs` tests it), and `dist host --steps=create --tag=<tag>` in CI refuses a tag that is not the crate version.
- `release.yml` keeps its `pull_request` trigger (`dist plan` only, `pr-run-mode` default), and `permissions` are those `dist` generates: `contents: write` for the workflow, `attestations: write`, `contents: read`, `id-token: write` for the build jobs. `auto-tag.yaml` adds `actions: write` for `gh workflow run` and nothing else.
- `auto-tag.yaml` dispatches only when `gh release view <version>` fails, on the ref `<version>`, with `-f tag=<version>`, so a rerun after a published release is a no-op and the build checks out the tagged commit.
- No release, push, or tag is made by this plan. Every CI-side behaviour (the dispatch, the attestation, the GitHub release) is exercised for the first time by the 5e cutover release and is listed as deferred in the receipt.
- Every expected value was captured on 2026-09-18 from cargo-dist 0.32.0 on macOS 26.5 arm64: the `dist plan` listing, the sha256 of `release.yml` and `auto-tag.yaml`, the host build, and the `dist plan` refusal of a hand edit.
- Commits follow Conventional Commits, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

1. **`dispatch-releases = true` over a tag-push trigger.** A tag `auto-tag.yaml` pushes with `GITHUB_TOKEN` starts no workflow, so the build must be dispatched either way. In dispatch mode `dist` generates the `workflow_dispatch` trigger itself and `dist plan` keeps guarding the file; the alternative, `allow-dirty = ["ci"]` with a hand-added `workflow_dispatch:` block, disables that guard and is overwritten by every `dist generate`. In dispatch mode the workflow does not create the tag (the diff between the two generated files touches only the trigger, the `plan` outputs, and one `if`), so the annotated tag `auto-tag.yaml` makes, with the changelog section as its message, stays the release tag and `gh release create` attaches to it. Cost: a tag pushed by hand or by `agentsync release` starts nothing until `auto-tag.yaml` runs on the `VERSION` push and dispatches; the pre-existing race between `release`'s tag push and `auto-tag.yaml`'s tag creation is unchanged.
2. **`install-path = "~/.agentsync/bin"`.** The installers place the binary where the clone install already lives, honour `AGENTSYNC_INSTALL_DIR` as the root (the same variable `install.sh` reads), and add the directory to the shell profile. Alternative: the default `CARGO_HOME`, which puts a non-Cargo tool in `~/.cargo/bin`; rejected. Open for 5e: the spec names `<engine>/bin/agentsync-native[.exe]` for a binary next to the dispatcher; the installers name it `agentsync`. 5e decides which name the dispatcher looks for; nothing here depends on it.
3. **`repository = "https://github.com/yelmuratoff/agent_sync"` in `Cargo.toml`.** `dist` builds the installers' download URL from it; `yelmuratoff/agent` redirects, but a redirect on every install is a dependency on a redirect. `install.sh` and the README still say `agent` and change in 5e.
4. **`[profile.dist]` inherits `release` and nothing else.** `dist init` proposed `lto = "thin"`; the shipped binary should be the `cargo build --release` binary (`lto = true`, `strip = true`, one codegen unit).
5. **No `ci.yaml` change.** `dist plan` refuses a stale `release.yml` (exit 255, verified), and `release.yml`'s `plan` job runs `dist plan` on pull requests; a `dist generate --check` step in `ci.yaml` would install `dist` a second time for the same check.
6. **Checksums as `checksum = "sha256"`, attestations as `github-attestations = true`.** Both are `dist` options; the build job gets the `actions/attest@v4` step and the `id-token`/`attestations` permissions from `dist generate`, and `sha256.sum` lists every archive in CI (locally it lists what was built on the host).
7. **Task order:** the dist configuration and the generated workflow (Task 1), the auto-tag dispatch (Task 2), the docs (Task 3). **Recommended:** as listed.

## Module closure

```text
Cargo.toml                          9-10     repository (changes), publish = false (the reason for [package.metadata.dist])
                                    27-30    [profile.release], which [profile.dist] inherits
dist-workspace.toml                          new, whole file in Task 1
.github/workflows/release.yml                generated by dist 0.32.0 in Task 1, 308 lines, never hand-edited
.github/workflows/auto-tag.yaml     1-48     the tag job; Task 2 adds actions: write and the dispatch step
.github/workflows/ci.yaml                    untouched
install.sh, bin/agentsync.sh, lib/           untouched
src/lib.rs                          42-53    engine_version and the crate-version test, unchanged
.ai/src/skills/native-port/references/module-map.md   70   the Phase 5b row; Task 3 adds the 5c row after it
docs/specs/2026-09-12-rust-migration-design.md        297-299   the cargo-dist bullet Task 3 rewrites
```

No Bash module is ported. Nothing in `src/` changes.

---

### Task 0: Baseline

**Files:** none changed.

- [ ] **Step 1: Record the baseline**

```bash
git log --oneline -1
dist --version
cargo test 2>&1 | grep 'test result' | head -4
shasum -a 256 .github/workflows/auto-tag.yaml
ls dist-workspace.toml .github/workflows/release.yml 2>&1 | grep -c 'No such file'
grep -c 'Phase 5c' .ai/src/skills/native-port/references/module-map.md
sed -n '9p' Cargo.toml
```

Expected: the plan's latest commit; `cargo-dist 0.32.0` (when `dist` is missing, stop: the maintainer installs it with `cargo install cargo-dist --locked`); `298 passed`, `0 passed`, `11 passed`, `1 passed`; `c4515c07a646b952c06e484010e2aeab609aa29358c515c241048685f33b5251`; `2`; `0`; `repository = "https://github.com/yelmuratoff/agent"`.

---

### Task 1: The dist configuration and the generated release workflow

**Files:**
- Create: `dist-workspace.toml`, `.github/workflows/release.yml` (by `dist generate`)
- Modify: `Cargo.toml:9`, `Cargo.toml:10` (a block after it), `Cargo.toml:30` (a block after it)

**Interfaces:**
- Consumes: `Cargo.toml` `version = "0.36.0"` equal to `VERSION` (Phase 5b).
- Produces: `release.yml` with `on: workflow_dispatch: inputs: tag` (required, default `dry-run`), which Task 2 dispatches with `gh workflow run release.yml --ref <tag> -f tag=<tag>`; the release assets 5d's `update` downloads: `agentsync-<target>.tar.xz` (`.zip` on Windows) holding `agentsync[.exe]`, `CHANGELOG.md`, `LICENSE`, `README.md`, each with a `.sha256` beside it, `sha256.sum`, `agentsync-installer.sh`, `agentsync-installer.ps1`, `source.tar.gz`, all under `https://github.com/yelmuratoff/agent_sync/releases/download/<tag>/`.

- [ ] **Step 1: Write `dist-workspace.toml`**

Create `dist-workspace.toml` at the repository root:

```toml
[workspace]
members = ["cargo:."]

# Config for 'dist'
[dist]
# The preferred dist version to use in CI (Cargo.toml SemVer syntax)
cargo-dist-version = "0.32.0"
# CI backends to support
ci = "github"
# The installers to generate for each app
installers = ["shell", "powershell"]
# Target platforms to build apps for (Rust target-triple syntax)
targets = ["aarch64-apple-darwin", "aarch64-unknown-linux-musl", "x86_64-apple-darwin", "x86_64-unknown-linux-musl", "x86_64-pc-windows-msvc"]
# Path that installers should place binaries in
install-path = "~/.agentsync/bin"
# Where to host releases
hosting = "github"
# Whether to install an updater program
install-updater = false
# Checksums for every artifact and a GitHub artifact attestation per release
checksum = "sha256"
github-attestations = true
dispatch-releases = true
```

Run: `shasum -a 256 dist-workspace.toml`
Expected: `f74b4fb9ada6ffa122b7f34b1b47bef5ad1111f3301d231ecc49ce594d072b76  dist-workspace.toml`

- [ ] **Step 2: Opt the crate into dist**

In `Cargo.toml`, replace line 9

```toml
repository = "https://github.com/yelmuratoff/agent"
```

with

```toml
repository = "https://github.com/yelmuratoff/agent_sync"
```

After line 10 (`publish = false`) insert

```toml

# publish = false hides the crate from dist; this opts it back in.
[package.metadata.dist]
dist = true
```

and append to the end of the file, after `strip = true`:

```toml

[profile.dist]
inherits = "release"
```

Run: `git diff --stat Cargo.toml Cargo.lock | cat; sed -n '9p;12,14p;36,37p' Cargo.toml`
Expected: `Cargo.toml | 9 ++++++++-` alone (`Cargo.lock` absent from the stat); the six lines `repository = "https://github.com/yelmuratoff/agent_sync"`, `# publish = false hides the crate from dist; this opts it back in.`, `[package.metadata.dist]`, `dist = true`, `[profile.dist]`, `inherits = "release"`.

- [ ] **Step 3: Generate the workflow and plan the release**

```bash
dist generate 2>&1 | tail -1
shasum -a 256 .github/workflows/release.yml; wc -l < .github/workflows/release.yml
sed -n '42,49p' .github/workflows/release.yml
dist plan --tag 0.36.0
```

Expected: `generated Github CI to <repo>/.github/workflows/release.yml`; `3350d9e2c86f3aaf096fa03e2b96e6d84261a207132a975e90d1c4cdfda5cb9e` and `308`; the trigger block

```yaml
  pull_request:
  workflow_dispatch:
    inputs:
      tag:
        description: Release Tag
        required: true
        default: dry-run
        type: string
```

and the plan, exit 0:

```text
announcing v0.36.0
  agentsync 0.36.0
    source.tar.gz
      [checksum] source.tar.gz.sha256
    agentsync-installer.sh
    agentsync-installer.ps1
    sha256.sum
    agentsync-aarch64-apple-darwin.tar.xz
      [bin] agentsync
      [misc] CHANGELOG.md, LICENSE, README.md
      [checksum] agentsync-aarch64-apple-darwin.tar.xz.sha256
    agentsync-aarch64-unknown-linux-musl.tar.xz
      [bin] agentsync
      [misc] CHANGELOG.md, LICENSE, README.md
      [checksum] agentsync-aarch64-unknown-linux-musl.tar.xz.sha256
    agentsync-x86_64-apple-darwin.tar.xz
      [bin] agentsync
      [misc] CHANGELOG.md, LICENSE, README.md
      [checksum] agentsync-x86_64-apple-darwin.tar.xz.sha256
    agentsync-x86_64-pc-windows-msvc.zip
      [bin] agentsync.exe
      [misc] CHANGELOG.md, LICENSE, README.md
      [checksum] agentsync-x86_64-pc-windows-msvc.zip.sha256
    agentsync-x86_64-unknown-linux-musl.tar.xz
      [bin] agentsync
      [misc] CHANGELOG.md, LICENSE, README.md
      [checksum] agentsync-x86_64-unknown-linux-musl.tar.xz.sha256
```

Then the runner matrix:

```bash
dist plan --tag 0.36.0 --output-format=json 2>/dev/null | jq -c '.ci.github.artifacts_matrix.include[] | [.runner, .targets[0], (.container.image // "none")]'
```

Expected, five lines:

```text
["macos-14","aarch64-apple-darwin","none"]
["ubuntu-22.04-arm","aarch64-unknown-linux-musl","none"]
["macos-15-intel","x86_64-apple-darwin","none"]
["windows-2022","x86_64-pc-windows-msvc","none"]
["ubuntu-22.04","x86_64-unknown-linux-musl","none"]
```

- [ ] **Step 4: Prove the freshness guard**

```bash
cp .github/workflows/release.yml "$TMPDIR/release.yml.bak"
printf '\n# hand edit\n' >> .github/workflows/release.yml
dist plan --tag 0.36.0 > /dev/null 2> "$TMPDIR/plan.err"; echo "exit=$?"
grep -c 'has out of date contents and' "$TMPDIR/plan.err"
mv "$TMPDIR/release.yml.bak" .github/workflows/release.yml
dist plan --tag 0.36.0 > /dev/null 2>&1; echo "exit=$?"
shasum -a 256 .github/workflows/release.yml
```

Expected: `exit=255`, `1`, `exit=0`, and the sha256 `3350d9e2c86f3aaf096fa03e2b96e6d84261a207132a975e90d1c4cdfda5cb9e` again.

- [ ] **Step 5: Build the host target and the global artifacts**

```bash
host=$(rustc -vV | sed -n 's/^host: //p'); echo "$host"
dist build --artifacts=local --target="$host" > /dev/null 2>&1; echo "local exit=$?"
dist build --artifacts=global --tag 0.36.0 > /dev/null 2>&1; echo "global exit=$?"
ls target/distrib/
tar -tJf "target/distrib/agentsync-$host.tar.xz"
shasum -a 256 "target/distrib/agentsync-$host.tar.xz" | cut -d' ' -f1
cut -d' ' -f1 "target/distrib/agentsync-$host.tar.xz.sha256"
grep -n 'releases/download' target/distrib/agentsync-installer.sh | head -1
grep -n 'releases/download' target/distrib/agentsync-installer.ps1 | head -1
grep -c '\$HOME/.agentsync/bin' target/distrib/agentsync-installer.sh
"target/distrib/agentsync-$host/agentsync" version
```

Expected (`host` is `aarch64-apple-darwin` on the 2026-09-18 machine; on another host the same names with that triple): `local exit=0`, `global exit=0`; the listing `agentsync-<host>/`, `agentsync-<host>.tar.xz`, `agentsync-<host>.tar.xz.sha256`, `agentsync-installer.ps1`, `agentsync-installer.sh`, `sha256.sum`, `source.tar.gz`, `source.tar.gz.sha256`; the archive holding `agentsync-<host>/`, `agentsync-<host>/agentsync`, `agentsync-<host>/README.md`, `agentsync-<host>/CHANGELOG.md`, `agentsync-<host>/LICENSE`; the two sha256 lines equal; `34:    ARTIFACT_DOWNLOAD_URLS="${INSTALLER_BASE_URL}/yelmuratoff/agent_sync/releases/download/0.36.0"`; `14:https://github.com/yelmuratoff/agent_sync/releases/download/0.36.0`; `2`; `agentsync v0.36.0`. `target/` is ignored, so `git status --short` shows only the three files of this task.

- [ ] **Step 6: The Rust gates**

```bash
cargo fmt --all --check; echo "fmt=$?"
cargo clippy --all-targets -- -D warnings > /dev/null 2>&1; echo "clippy=$?"
cargo test 2>&1 | grep 'test result' | head -4
git status --short
```

Expected: `fmt=0`, `clippy=0`, `298 passed`, `0 passed`, `11 passed`, `1 passed`; status ` M Cargo.toml`, `?? .github/workflows/release.yml`, `?? dist-workspace.toml`, and the plan.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml dist-workspace.toml .github/workflows/release.yml
git commit -m "feat(release): add the cargo-dist release build"
```

---

### Task 2: The auto-tag workflow dispatches the release build

**Files:**
- Modify: `.github/workflows/auto-tag.yaml:12-13` (permissions), append one step after line 48

**Interfaces:**
- Consumes: `release.yml`'s `workflow_dispatch` `tag` input (Task 1).
- Produces: on a `VERSION` push to `main`, the annotated tag `<version>` as before, then `gh workflow run release.yml --ref <version> -f tag=<version>` unless `gh release view <version>` succeeds.

- [ ] **Step 1: Write the workflow**

Replace `.github/workflows/auto-tag.yaml` whole with:

```yaml
name: Auto Tag

on:
  push:
    branches: [main]
    paths:
      - VERSION

env:
  FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: true

permissions:
  contents: write
  actions: write

jobs:
  tag:
    name: Create tag from VERSION
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Read version
        id: version
        run: echo "version=$(cat VERSION)" >> "$GITHUB_OUTPUT"

      - name: Check if tag exists
        id: tag_exists
        run: |
          if git rev-parse "refs/tags/${{ steps.version.outputs.version }}" > /dev/null 2>&1; then
            echo "exists=true" >> "$GITHUB_OUTPUT"
          else
            echo "exists=false" >> "$GITHUB_OUTPUT"
          fi

      - name: Create and push tag
        if: steps.tag_exists.outputs.exists == 'false'
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          version="${{ steps.version.outputs.version }}"
          tag_body=$(awk -v ver="## ${version}" \
            '$0 == ver {found=1; next} found && /^## /{exit} found' CHANGELOG.md)
          printf 'v%s\n\n%s' "$version" "$tag_body" > /tmp/tag_message.txt
          git tag -a "$version" -F /tmp/tag_message.txt
          git push origin "$version"

      # A tag pushed with GITHUB_TOKEN starts no workflow, and the release
      # workflow runs on workflow_dispatch only (dist-workspace.toml,
      # dispatch-releases), so the build is dispatched here, on the tag ref,
      # unless the release already exists.
      - name: Dispatch the release build
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          version="${{ steps.version.outputs.version }}"
          if gh release view "$version" > /dev/null 2>&1; then
            echo "release $version exists; nothing to dispatch"
            exit 0
          fi
          gh workflow run release.yml --ref "$version" -f "tag=$version"
```

Run: `shasum -a 256 .github/workflows/auto-tag.yaml; git diff --stat .github/workflows/auto-tag.yaml | cat`
Expected: `f3066db38851d82fde0208100d98e297f060a73a017dd700661e3ff1ab76feae`; `.github/workflows/auto-tag.yaml | 16 ++++++++++++++++`.

- [ ] **Step 2: Parse both workflows**

Write `$TMPDIR/yaml_check.rb` (Ruby ships with macOS and the Linux runners; it is a verification tool, not a runtime dependency):

```ruby
#!/usr/bin/env ruby
# Usage: yaml_check.rb <workflow.yaml>...
# Parses each workflow and prints its job names and, per job, the step names.
require "yaml"
ARGV.each do |path|
  doc = YAML.safe_load(File.read(path), permitted_classes: [], aliases: true)
  jobs = doc.fetch("jobs")
  puts "#{path}: #{jobs.size} job(s), permissions=#{doc['permissions'].inspect}"
  jobs.each do |name, job|
    steps = job.fetch("steps").map { |s| s["name"] || s["uses"] || s["id"] || "?" }
    puts "  #{name}: #{steps.join(' | ')}"
  end
end
```

Run: `ruby "$TMPDIR/yaml_check.rb" .github/workflows/auto-tag.yaml .github/workflows/release.yml; echo "exit=$?"`
Expected:

```text
.github/workflows/auto-tag.yaml: 1 job(s), permissions={"contents"=>"write", "actions"=>"write"}
  tag: actions/checkout@v4 | Read version | Check if tag exists | Create and push tag | Dispatch the release build
.github/workflows/release.yml: 5 job(s), permissions={"contents"=>"write"}
  plan: actions/checkout@v6 | Install dist | Cache dist | plan | Upload dist-manifest.json
  build-local-artifacts: enable windows longpaths | actions/checkout@v6 | Install Rust non-interactively if not already installed | Install dist | Fetch local artifacts | Install dependencies | Build artifacts | Attest | Post-build | Upload artifacts
  build-global-artifacts: actions/checkout@v6 | Install cached dist | ? | Fetch local artifacts | cargo-dist | Upload artifacts
  host: actions/checkout@v6 | Install cached dist | ? | Fetch artifacts | host | Upload dist-manifest.json | Download GitHub Artifacts | Cleanup | Create GitHub Release
  announce: actions/checkout@v6
exit=0
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/auto-tag.yaml
git commit -m "feat(release): dispatch the release build from auto-tag"
```

---

### Task 3: Verify and Module Map

**Files:**
- Modify: `.ai/src/skills/native-port/references/module-map.md:70`, `docs/specs/2026-09-12-rust-migration-design.md:297-299`, `.ai/.sync-manifest` (regenerated)

- [ ] **Step 1: Module map, spec, and outputs**

In the module map's "Engine modules" block, after

```text
lib/helpers/release.sh           → src/cli/release.rs      Phase 5b, ported; git through the executable, the tag message on its stdin
```

add

```text
.github/workflows/auto-tag.yaml  → .github/workflows/release.yml  Phase 5c; dist 0.32.0 generates it from dist-workspace.toml (workflow_dispatch), auto-tag dispatches it on the tag
```

In the spec, replace the Phase 5 bullet

```markdown
- cargo-dist: Linux x86_64 and aarch64 (musl), macOS x86_64 and aarch64,
  Windows x86_64; release workflow, `curl | sh` installer, PowerShell
  installer, sha256 sums, artifact attestations.
```

with

```markdown
- cargo-dist 0.32.0 (`dist-workspace.toml`, `[package.metadata.dist]`): Linux x86_64
  and aarch64 (musl), macOS x86_64 and aarch64, Windows x86_64; the generated
  release workflow runs on `workflow_dispatch` with the tag, `curl | sh` and
  PowerShell installers into `~/.agentsync/bin`, sha256 sums, artifact
  attestations.
```

Run: `git diff --stat .ai/src docs/specs | cat`
Expected: `.ai/src/skills/native-port/references/module-map.md | 1 +` and `docs/specs/2026-09-12-rust-migration-design.md | 8 +++++---`.

Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force > /dev/null` (outside the sandbox if it refuses a write) and read `git status --short`: ` M .ai/.sync-manifest`, ` M .ai/src/skills/native-port/references/module-map.md`, ` M docs/specs/2026-09-12-rust-migration-design.md`, and the plan.

- [ ] **Step 2: Verify (outside the agent sandbox where a file says so)**

Set `S="$TMPDIR/phase5c"; mkdir -p "$S"` and write `$S/native_suite.sh` when it does not exist:

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
cargo build --release 2>&1 | tail -1
cargo test 2>&1 | grep 'test result' | head -4
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
dist plan --tag 0.36.0 > /dev/null; echo "plan=$?"
bash "$S/native_suite.sh" "$PWD" both "$S/suite_both.out" && tail -1 "$S/suite_both.out"
```

Expected: `Finished` (no source changed, so the release binary is up to date); `298 passed`, `0`, `11`, `1`; lint exit 0; `plan=0`; `TOTAL bash=0 native=0` over the 50 bats files, each run one at a time under both engines, with `native_parity` run outside the sandbox, which refuses the `diff -` stdin operand `src/cli/diff.rs` uses. `sync` and `check` do not change in this slice, so no timings are due.

- [ ] **Step 3: Commit**

```bash
git add .ai/.sync-manifest .ai/src/skills/native-port/references/module-map.md docs/specs/2026-09-12-rust-migration-design.md
git commit -m "docs(native): map the phase 5c release build"
```

---

## Completion

The plan is closed when every box is ticked, `dist plan --tag 0.36.0` exits 0 with the listing in Task 1 Step 3, `release.yml` carries the recorded sha256, the host build in Task 1 Step 5 verifies its own checksum, every bats file is green under both engines, and a `## Completion receipt` records the fresh verification. The receipt lists as deferred everything only a real release exercises: the dispatch from `auto-tag.yaml`, the five runners, the attestation, the GitHub release, and the installers against a published asset; the 5e cutover release is their first run. Phase 5 stays open until the plans for 5d and 5e are closed as well.

## Run log

### 2026-09-18 — Phase 5c planned
- Commits: this plan.
- Verified: the whole slice was drafted in the tree and parked in the session scratchpad (`phase5c/draft/`), then the tree was restored to HEAD. Against the draft: `dist init --yes` proposed seven targets (the two gnu ones added) and could not release a `publish = false` crate (`This workspace doesn't have anything for dist to Release!`), which `[package.metadata.dist] dist = true` fixes; with the plan's config `dist generate` wrote `release.yml` at sha256 `3350d9e2…` (308 lines); `dist plan --tag 0.36.0` exit 0 with the listing in Task 1 Step 3 and the five native runners; a bare tag parses (`announcing v0.36.0`); generating with and without `dispatch-releases` differs only in the trigger, three `plan` outputs, the `plan` command, and one `if`, and neither creates the tag; a hand edit of `release.yml` made `dist plan` exit 255 with `has out of date contents and needs to be regenerated`; `dist build --artifacts=local --target=aarch64-apple-darwin` and `dist build --artifacts=global --tag 0.36.0` exit 0, the archive's sha256 equal to its `.sha256`, the installers pointing at `yelmuratoff/agent_sync/releases/download/0.36.0` (without `--tag` they point at `v0.36.0`), the shell installer 1468 lines naming `$HOME/.agentsync/bin`; `cargo test` 298/0/11/1, fmt and clippy exit 0 with the `Cargo.toml` change and `Cargo.lock` untouched; both workflows parse with `yaml_check.rb` and print the job and step names in Task 2 Step 2; `sync --dry-run` 0 warnings with the docs edits. `cargo install cargo-dist --locked` was run by the maintainer on 2026-09-18 after the previous run stopped on it.
- Plan amended: none.
- Next: Task 0 Step 1, after the review.
- Blocker: none.
