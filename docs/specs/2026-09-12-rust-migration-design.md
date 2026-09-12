# Rust Engine Migration

Date: 2026-09-12
Status: In progress since 2026-09-12. Phase 1 is planned in
`docs/plans/2026-09-12-rust-migration-phase-1-native-list.md`.

## Objective

Replace the Bash engine with one statically linked Rust binary, command by
command, so that a user never sees a behaviour change until the cutover release
and never needs Git Bash on Windows after it.

The result must preserve AgentSync's existing properties:

- `.ai/src/` remains the source of truth.
- Generated outputs are byte-identical to what 0.35.x writes for the same
  source, config, and enabled tools.
- Drift protection, the transactional backup and rollback, the
  `agentsync_version` pin, and the committed/local outputs modes keep their
  semantics and their on-disk formats.
- The guard hook stays plain POSIX `sh`: a teammate without the CLI must still
  be protected.
- The 725-test bats suite stays the behavioural contract and runs against the
  binary, until Phase 7 rewrites it as Rust integration tests.

## Why

Measured on this repository's own `.ai/src/` (392 source files, 98 skills),
engine at 0.35.2, macOS:

| Command | Wall time | user / sys |
| --- | --- | --- |
| `sync`, 13 tools, 272 outputs | 6.2–6.7 s | 2.8 s / 3.5 s |
| `check`, 13 tools | 7.0 s | 2.9 s / 4.0 s |
| `sync`, 2 tools | 2.0 s | 1.0 s / 0.9 s |
| `sync --if-stale`, no-op | 0.15 s | |

System time exceeds user time: the engine is fork-bound. 0.35.1 removed
subshells from the hot paths and gained 23%; the remaining ~1,600 command
substitutions are the floor. `check` at 7 s runs in CI gates and pre-commit
hooks.

Reliability: 0.35.2 fixed five platform-specific failures of one class
(`dirname` on BSD and MSYS, `shasum` absent on Git Bash, `$TMPDIR` with a
trailing slash), one intermittent macOS failure stays undiagnosed, and Windows
CI shards bats twelve ways because Git Bash cannot run it in parallel. A typed
language with `Result`-based I/O and a standard path API removes the class, not
the instance.

## Product Boundary

Changes:

- The engine implementation, its distribution (prebuilt binaries instead of a
  git clone), and the `update` mechanism.
- Windows support without Git Bash at cutover.

Does not change:

- The CLI surface: commands, flags, exit codes, stdout and stderr text.
- File formats: `agent_sync.yaml`, tool YAML, `.ai/.sync-manifest`,
  `.ai/.template-manifest`, `.ai/backups/<id>/` layout, the `.gitignore`
  managed block.
- Shipped templates and skills under `lib/templates/`; they are embedded in the
  binary unchanged.
- `lib/templates/guard/claude.sh` and the hook snippets `setup-hooks` and
  `shell-init` emit: they run in the user's shell, not in the engine.

## Considered Approaches

### Keep Bash, keep optimising

Rejected. 0.35.1 already took the cheap wins. What remains is the fork cost of
the language itself and a bug class that grows with every platform.

### Big-bang rewrite in a separate repository

Rejected. 416 commits in seven months and a feature release (0.35) three weeks
old: freezing that for the months a rewrite takes is how rewrites die. Nothing
would be verifiable until the end.

### Strangler: Bash dispatcher, commands move one at a time

Chosen. `bin/agentsync.sh` stays the entry point and delegates each command to
the binary once that command is ported. Releases keep shipping throughout, a
feature lands in whichever implementation owns the command, and every step is
verified by the existing suite.

### Language

Rust, for the reasons recorded in the discussion that produced this document:
`Result` forces every filesystem failure to be handled, `Path` is typed, the
workload (files in, strings transformed, files out; no concurrency) is the easy
part of the language, and cargo-dist produces the `curl | sh` installer,
checksums, and release workflow. Go is the fallback if, at the end of Phase 1,
the maintainer's velocity in Rust is not acceptable: the dispatcher, the parity
suite, and the CI shape are language-agnostic, and only the crate would be
replaced.

## Architecture

### Layout

```text
Cargo.toml                 # crate `agentsync`, version stays 0.0.0 (VERSION file rules)
src/main.rs                # args → run(); exit codes; the only process-aware file
src/lib.rs                 # module tree; engine_version()
src/cli/<command>.rs       # one file per command: args → core calls → text
src/<module>.rs            # core: yaml_subset, project, catalog, tool, payload, style, …
lib/templates/             # unchanged; embedded via include_dir!
bin/agentsync.sh           # dispatcher until Phase 6
tests/*.bats               # the contract; unchanged files run against either engine
tests/native_dispatch.bats # dispatcher gating
tests/native_parity.bats   # Bash vs native diff per ported command
tests/*.rs                 # cargo integration tests
```

### Dispatcher

`bin/agentsync.sh` gains `_native_try`, called after `check_for_updates` and
the `--help` interception, before the command `case`:

- `AGENTSYNC_NATIVE=0` — always Bash.
- `AGENTSYNC_NATIVE=1` — require a binary; fail loudly without one.
- unset — use a binary when one is found.
- `AGENTSYNC_NATIVE_BIN` — an explicit binary; otherwise
  `<engine>/target/release/agentsync[.exe]` (developer build) or
  `<engine>/bin/agentsync-native[.exe]` (Phase 5 installer).

The dispatcher passes `AGENTSYNC_ENGINE_VERSION=$VERSION`; a binary whose
embedded `VERSION` differs refuses to run, so a stale developer build can never
answer for a newer engine.

`_NATIVE_COMMANDS` is the single list of ported commands. A command is ported
when its bats file passes with `AGENTSYNC_NATIVE=1` and its parity tests pass.

### Conformance

Three layers, one seam:

1. **bats with `AGENTSYNC_NATIVE=1`** — the existing suite, unchanged, run
   against the binary for ported commands. `tests/test_helper.bash` defaults
   `AGENTSYNC_NATIVE` to `0` so a stray developer build never changes what the
   suite exercises.
2. **Parity tests** — `tests/native_parity.bats` runs each ported command
   through both engines on the same fixture and diffs stdout+stderr and the
   exit status. Fixtures are added per command as it is ported.
3. **Golden outputs** — from Phase 2, the Bash engine generates the outputs for
   this repository's own `.ai/src/` with all 13 tools enabled; the native
   engine must reproduce all 272 files byte for byte. 0.35.1 used the same
   technique.

Unit tests live inside the crate for pure modules.

### Config reading

The Bash engine never parsed YAML. `lib/helpers/yaml.sh` is a line-oriented
reader with its own rules: the first duplicate key wins, an unquoted value ends
at the first `#`, `\n` inside quotes stays literal until `printf '%b'` at write
time, an empty block list keeps scanning and picks up the next dash list in the
file. Every shipped and user config was written against those rules, and the
suite asserts on them.

`src/yaml_subset.rs` therefore ports that reader line for line rather than
adopting a YAML crate. This keeps parity provable and keeps the port small.
Replacing it with a real parser plus `doctor` validation is a post-cutover
decision, taken with the quirk list below in hand.

### Errors and output

- Core modules return `Result<_, agentsync::Error>` (`thiserror`). `main.rs`
  maps errors to the same messages and exit codes the Bash command used.
- Two output voices exist today and both are kept: `style` mirrors
  `cli_colors.sh` (bold/green/cyan/yellow/red/dim, decided once from stdout
  being a TTY and `NO_COLOR`), and `log` (Phase 2) mirrors `logging.sh`
  (`[INFO]`, `[WARNING]`, the `═` separator, emoji only when coloured).
- Prompts read the terminal directly (`/dev/tty`, `CONIN$`), as
  `prompts.sh` does, so captured output never breaks interaction.
- A broken pipe on stdout exits 0 silently, matching a shell pipeline.

## Phases

Each phase ends with the full suite green in both modes and a release from
`main`. Phase 1 has an executable plan; each later phase gets its own plan when
its turn comes, written against what the previous phase revealed.

### Phase 1 — Foundation and `list`

Crate scaffold, CI on three platforms, dispatcher and its tests, `yaml_subset`,
project config and the layered tool resolver, payload discovery, `version` and
`list` served natively, parity suite. No user-visible change apart from one
prefactor the parity fixtures exposed before a line of Rust existed: `list`
exits 1 silently when the last `.ai/src/tools/*.yaml` override lacks
`enabled: true`, because `list_legacy_enabled_tools` returns the status of its
final `[[ ]]`; it is fixed with a regression test first, so the Bash reference
is correct. Exit: Phase 1 plan's completion receipt; decision gate on language
velocity.

### Phase 2 — Render core and `check`

Port everything `sync` needs to compute outputs without writing them:
`paths` (normalisation, containment, existing-ancestor canonicalisation),
`filters`, full tool resolution (`get_tool_filter`, `resolve_payload_source`
with the legacy warning), `profiles`, `shared` overlays and the engine-owned
skill layer, `rule_operations` (headers, frontmatter merge, `append_imports`,
`merge_to_file`, the three inliners, commands as skills, guard),
`format_conversion` (TOML, Amazon Q JSON, OpenCode MD), OpenCode JSON
composition. The renderer produces an in-memory map of relative path to bytes.

`check` becomes render + compare against the manifest paths, with the same
messages and exit codes as `lib/check.sh`, and no `tar` or temp workspace.
Exit: `tests/check.bats`, `sync.bats`-derived golden outputs, `base_skills`,
`shared`, `profiles`, `opencode`, `resource_resolver` fixtures byte-identical;
`check` on the 13-tool fixture measured and recorded.

### Phase 3 — `sync` transaction and `rollback`

Manifest load/drift/write, backup create/restore/prune in the existing
`.ai/backups/` format (so a Bash `rollback` on an older install still reads
it), the `.gitignore` block, `post_sync` with the install-dir `config.yaml`
gate, `--dry-run`, `--force`, `--only`, `--skip`, `--profile`, `--if-stale`,
`--workspace`, the version-pin gate, the baseline-replacement warning, and the
signal-safe restore. Exit: `sync`, `check`, `rollback` bats files and
`drift`, `outputs_mode`, `team_workflow`, `workspace`, `version_pin` green
natively; `sync` on the 13-tool fixture measured and recorded.

### Phase 4 — Remaining commands

Grouped by the module they share, each group its own plan:

- `yaml_edit` family: `enable`, `disable`, `customize`, `simplify`, `show`,
  `diff`, `resolve`, `profile`, `upgrade-config`.
- `template_manifest` family: `init`, `refresh`, `dedupe`, `migrate`, `adopt`.
- Standalone: `doctor` (keeps its tri-state exit code), `add`, `export`,
  `import`, `generate`, `shell-init`, `setup-hooks`.

`update` and `release` are rewritten in Phase 5 because their mechanics change.
Exit: `_NATIVE_COMMANDS` lists every command; the whole suite passes with
`AGENTSYNC_NATIVE=1`.

### Phase 5 — Distribution and cutover

- cargo-dist: Linux x86_64 and aarch64 (musl), macOS x86_64 and aarch64,
  Windows x86_64; release workflow, `curl | sh` installer, PowerShell
  installer, sha256 sums, artifact attestations.
- `install.sh` becomes the generated installer (or downloads the binary and
  verifies its checksum); `AGENTSYNC_VERSION=<tag>` still pins.
- `update` replaces the binary from GitHub Releases and keeps `update <version>`
  pinning; the `agentsync_version` gate is unchanged.
- `release` bumps `VERSION` and `Cargo.toml` together; the auto-tag workflow
  triggers the release build.
- `bin/agentsync.sh` becomes a one-line shim, then the symlink points at the
  binary.
- Windows: the binary is the entry point; the bats suite runs against it
  directly, without sharding. Terminal colour on legacy consoles is enabled
  with the `anstream` crate if needed.

Exit: first binary release; README and `.ai/src/AGENTS.md` updated; the
"pure Bash" claim replaced by "single static binary".

### Phase 6 — Retire Bash

Delete `lib/*.sh` and `bin/agentsync.sh`; port the Bash-unit bats files
(`files`, `paths`, `backup`, `tmp`, `gitignore`, `update_snapshot`) to Rust
unit tests; keep the CLI-level bats files as the conformance suite until
Phase 7 retires them; remove `_native_try`, `AGENTSYNC_NATIVE`, and the
Windows shard matrix; ShellCheck covers only the guard and installer scripts
that remain.

### Phase 7 — Retire bats

Port the CLI-level conformance suite (43 `.bats` files, 733 tests at the start
of Phase 1) to Rust integration tests on `assert_cmd`, the shape
`tests/cli.rs` already uses: one commit per bats file, Rust test names copied
from the bats test names so a reviewer can map them one to one. Delete
`tests/test_helper.bash`, drop bats and GNU parallel from CI, and remove the
Windows sharding scaffolding.

This cannot move earlier. The suite is the only proof of parity while both
engines exist: the same test grades Bash under `AGENTSYNC_NATIVE=0` and the
binary under `=1`. Rewriting it before Phase 6 would replace the contract with
its own reimplementation. Once Bash is gone there is no second engine to grade,
the argument expires, and bats becomes a dependency that costs a serial
~58-minute Windows run (see the sharding comment in `.github/workflows/ci.yaml`).

Exit: `cargo test` is the whole suite; no `.bats` file remains.

### The shell floor

Two shell scripts survive every phase, because a binary cannot do their job:

- `lib/templates/guard/claude.sh` (50 lines of POSIX `sh`) — it runs in a
  teammate's checkout where the CLI is not installed, which is the reason the
  hook exists. The alternative is committing a per-platform binary into the
  user's repository.
- The installer — bootstrap: something must detect the platform and fetch the
  binary before a binary exists. From Phase 5 cargo-dist generates it, so it
  stops being hand-maintained code. `rustup` ships the same way.

The text `shell-init` and `setup-hooks` emit stays shell because the shell
`eval`s it and Git runs it as a hook; from Phase 4 the logic that produces that
text is Rust, and the emitted snippet is a wrapper that calls the binary.

## Distribution

Users today run `curl | bash`, which clones the repository into
`~/.agentsync` and symlinks `bin/agentsync.sh`. After Phase 5 the same command
downloads a platform binary, verifies its sha256, and links it; `agentsync
update` swaps the binary. The `agentsync_version` pin, `update <version>`, and
`AGENTSYNC_VERSION=<tag>` in the installer keep working, which keeps the
committed-outputs team workflow intact.

Until Phase 5, no user has a binary. Native code path exposure is limited to
developers who run `cargo build --release`, and the dispatcher's default falls
back to Bash when no binary exists.

## Known quirks to reproduce now and fix after cutover

Recorded so the parity work reproduces them knowingly and the post-cutover
cleanup has a list:

1. `parse_yaml_list` on an empty block key keeps scanning and returns the next
   dash list anywhere later in the file (`yaml.sh:180-197`).
2. An unquoted scalar is cut at the first `#`, even without a preceding space.
3. `\n` in a quoted header stays literal until `printf '%b'` at write time.
4. `get_tool_value` cannot override a base value with an empty string, and
   never consults `base:` when a shipped file exists for the slug.
5. `defaults.enabled` in `agent_sync.yaml` and the `defaults:` block in
   `lib/config.yaml` are never read.
6. Bash `printf '%-Ns'` pads styled strings including their escape bytes, so
   coloured `list` columns drift; the native `pad_right` reproduces it.
7. `outputs` absent means `local`, except when `gitignore.update: false`, which
   means `committed`; the rule is duplicated in three files.
8. Tool listings are sorted with locale `sort`; the native engine uses byte
   order (accepted deviation, see below).

## Accepted deviations

Appended one line at a time as they are found, with the phase:

- Phase 1: clap rejects unrecognised arguments to a ported command with exit 2;
  Bash ignored them.
- Phase 1: tool slugs sort in byte order; Bash used the locale's `sort`.
- Phase 1: `\r\n` line endings in config files read as `\n`; Bash kept a
  trailing `\r` that normalisation then trimmed, so values agree.

## Risks

- **Windows during the strangler.** Under Git Bash, POSIX paths in
  `AGENTSYNC_REPO_ROOT` and `TMPDIR` do not reach a native executable. Native
  bats runs on Windows wait until Phase 5, when the binary is the entry point;
  `cargo test` covers the Rust side on Windows from Phase 1.
- **Symlinked project roots.** Bash keeps the logical `$PWD`; Rust's
  `current_dir()` is physical. Phase 2's `paths` module honours `$PWD` when it
  names the same directory, so manifest and display paths do not change.
- **Interactive prompts.** Bash reads `/dev/tty`; the port must do the same or
  captured-output flows (`check_for_updates`, hooks) change behaviour.
- **Feature work during migration.** A feature on an unported command lands in
  Bash; after Phase 3 a feature on `sync` lands in Rust only. The dispatcher's
  list makes the owner explicit.
- **Maintainer velocity.** The decision gate at the end of Phase 1 exists for
  this; Go is the fallback with the same architecture.

## Definition of Done

- `bin/agentsync.sh` and `lib/*.sh` are gone; `agentsync` is one binary on five
  targets, installed and updated from GitHub Releases.
- The bats suite passes against the binary on Linux, macOS, and Windows without
  sharding.
- Golden outputs for this repository's `.ai/src/` with 13 tools are
  byte-identical to 0.35.2's.
- `install.sh`, `update`, the version pin, and the guard hook behave as
  documented in the README, which no longer says "pure Bash".
