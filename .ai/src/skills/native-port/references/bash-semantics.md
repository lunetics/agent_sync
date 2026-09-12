# Bash engine semantics the port reproduces

What the Bash implementation does today, with the line that proves it. Reproduce it. A deliberate change is an accepted deviation, recorded in `docs/specs/2026-09-12-rust-migration-design.md`. The quirks that are reproduced until cutover are numbered there under "Known quirks"; cite the number in a test name.

## YAML reader — lib/helpers/yaml.sh

- A key line is a stripped line matching `^([a-zA-Z0-9_-]+):[[:space:]]*(.*)` (`:71-80`). Anything else, including `- item` and quoted keys, is skipped, not treated as structure.
- Indent is the count of leading whitespace characters; a tab counts 1 (`:67-68`).
- The root key must sit at indent 0. Inside a section, the first line at indent ≤ the section's indent returns empty and the walk ends (`:97-101`).
- The first duplicate key wins.
- Scalar normalisation (`:13-30`): trim; `"…"` or `'…'` unwrapped verbatim with no escape processing; otherwise cut at the first `#` and right-trim. An empty value reads the same as a missing key.
- `parse_yaml_list` (`:133-235`): a single-line `[a, b]` splits on commas; otherwise a block of `- item` lines at the first dash's indent. An empty block key keeps scanning and takes the next dash list anywhere later in the file.
- `parse_yaml_bool`: `true|yes|1|on`, case-insensitive. The strict variant returns 2 for missing and 3 for a non-boolean (`:240-283`).
- `yaml_edit.sh` mutates line by line, never touches comments, writes atomically through `tmp_sibling` + `mv`; `yaml_set_scalar` handles root-depth keys only; `yaml_list_append` collapses an inline list to a block.

## Project config — .ai/agent_sync.yaml

- Discovery: `$AGENTSYNC_CONFIG_PATH` → `.ai/agent_sync.yaml` → `agent_sync.yaml` (`lib/sync.sh:156-181`). `check.sh:32-33` and `format.sh:55-62` repeat the two-path fallback without the env var.
- `agentsync_version` gates `sync` and `check` (`version.sh:15-21`, `sync.sh:798-813`, `check.sh:31-51`); fatal only when `outputs` is `committed`.
- `format` defaults to 1; non-numeric reads as 1 (`format.sh:24-34`).
- Enablement = `tools.enabled` ∪ every `.ai/src/tools/<slug>.yaml` whose root `enabled` is the literal `true` (`tool_resolver.sh:473-501`). `defaults.enabled` and the `defaults:` block in `lib/config.yaml` are never read.
- `defaults.cleanup` defaults to `true` (`sync.sh:762,767`); `cleanup_tool` no-ops unless it is exactly `true`.
- `outputs`: `committed` or `local`; absent means `local` unless `gitignore.update: false`, which means `committed` (`sync.sh:781-795`, duplicated in `check.sh:36-39` and `setup_hooks.sh:112-118`). Any other value is a fatal `log_error`.
- `post_sync.allow` is read only from the install-dir `lib/config.yaml` (`sync.sh:747-755`); a project file cannot enable hook execution.
- `source.<kind>` also accepts the same names as root keys; nested wins (`sync.sh:184-200`). Resolution: global `config.yaml` → autodetect `.ai/src/<kind>` over `.ai/<kind>` → project override (`sync.sh:833-876`).
- `shared.path`, `shared.inherit`, `base_skills` (only the literal `false` disables), `profiles.<name>.{overlay,active,tools}` (`shared.sh`, `profiles.sh`).

## Tool config layering — lib/helpers/tool_resolver.sh

- Per field, never per file: user `.ai/src/tools/<slug>.yaml` wins only when non-empty; the shipped `lib/templates/tools/<slug>.yaml` answers next, even when empty, and blocks the `base:` fallback; `base:` applies only to slugs with no shipped file and never for `base` or `name` (`:67-101`).
- `get_tool_bool` prints `true`, `false`, or empty for garbage; call sites test `== "false"` (`:114-127`).
- `get_tool_filter` accepts a scalar, `[a, b]`, or a block list and returns a space-joined glob string (`:132-191`).
- Payload order (`:330-387`): `.ai/src/tools/<slug>/<res>.*` → `targets.<res>.source` if the file exists → legacy `.ai/src/<res>/<slug>.<ext>` with a once-per-run warning → `.ai/src/mcp.json` for `mcp` only → `lib/templates/<res>/<slug>.*` with a `base:` fallback. `describe_payload_source` classifies the winner as override / declared / legacy / shared / base.
- Target keys: `agents rules skills commands subagents settings mcp hooks guard` (`:19-23`). Every target accepts `dest`, `enabled: false`, `profile_scoped: false`; `source` on agents/rules/settings/mcp/hooks; `extension` on rules/commands/subagents; `header`, `scoped_header`, `append_imports`, `merge_to_file`, `prepend_agents` on rules; `include`/`exclude` on rules/skills/commands; `inline_into_agents` on rules/skills/commands; `as_skills` on commands; `format` on commands (`toml`), subagents (`toml`, `amazonq_json`, `opencode_md`), mcp (`opencode_json`).

## Paths — lib/helpers/paths.sh

- `resolve_dest_path_r` (`:190-214`): normalise lexically against `REPO_ROOT`, canonicalise through the nearest existing ancestor (`cd -P && pwd`, memoised), reject anything outside `REPO_ROOT_CANONICAL`, and return the non-canonical normalised path so symlinked roots keep their spelling. Messages: `<label> is empty`, `Failed to canonicalize <label> path: <raw>`, `<label> resolves outside repository root: <raw> -> <canonical>`.
- `is_path_safe_source` (`:221-250`) allows `REPO_ROOT_CANONICAL`, `DEFAULT_REPO_ROOT`, and the shared, profile, and base-src overlay dirs.
- `to_repo_relative_path_r` (`:416-431`): root → `.`, prefix strip → relative, otherwise `Path is outside repository root: <abs>` and failure.
- `_r`-suffixed helpers return through `$REPLY`; the unsuffixed twins echo. Predicates return status only.

## Manifests

- `.ai/.sync-manifest` (`manifest.sh`): one `<rel>\t<sha256>` line per output, `LC_ALL=C sort -u`, no header. `sha256sum` else `shasum -a 256`; with neither, drift detection silently no-ops. A missing dest is not drift. The written set is old entries whose dest exists and were not touched ∪ fresh hashes of touched paths; an empty set deletes the file.
- `.ai/.template-manifest` (`template_manifest.sh`): same shape, paths relative to `.ai/src/`, read by `init` and `refresh` only.

## check — lib/check.sh

Version pin check → copy `.ai` plus every manifest path into a temp root with `tar` → run `sync.sh --force` there with `AGENTSYNC_SKIP_POST_SYNC=true AGENTSYNC_INTERNAL_SKIP_BACKUP=true` → compare the union of both manifests with `cmp`. Lines: `Files <rel> differ`, `Missing: <rel>`, `No longer generated: <rel>`, at most 20. Clean: `✅ AgentSync configurations are safe and synced.` exit 0. Dirty: `⚠️  AgentSync configurations are out of sync with source.` … `Please run: lib/sync.sh` exit 1. Every failure exits 1. Emoji print unconditionally.

## sync transaction — lib/sync.sh, lib/helpers/backup.sh

- Backup under `.ai/backups/<UTC yyyymmddThhmmssZ>-<op>-<pid>[-n]/` with `metadata` (`schema=1`, `operation`, `created_at`), `targets.tsv` (`present|missing\t<rel>`), `files/`, `.complete`; store has `.latest` and a `.gitignore` containing `*`. Restore removes every recorded target, then copies the present ones back. Prune keeps `AGENTSYNC_BACKUP_LIMIT` (10) and `AGENTSYNC_BACKUP_MAX_AGE_DAYS` (30); the latest snapshot always survives.
- Drift gate without `--force`: `Manual edits detected in N destination file(s) since last sync:` + paths + the three-option hint block, exit 1. With `--force`: `Overwriting N file(s) with manual edits (--force):`.
- `--if-stale` (`sync.sh:884-909`): no manifest → stale; else stale when any file under `.ai/src`, `.ai/profiles`, the config file, or an out-of-tree `SOURCE_*` path is newer than the manifest. Fresh → silent exit 0.
- `.gitignore` block between `# --- AI SYNC GENERATED START ---` and `# --- AI SYNC GENERATED END ---`, sorted and deduped; profile-scoped paths always ignored; in `local` mode the outputs and `.ai/.sync-manifest` are ignored too (`sync.sh:1235-1251`).
- Banner and summary shape: separator, `Starting AgentSync Config Sync...` (`(DRY RUN)` suffix), per-tool `<display> complete`, separator, optional preserved/skipped/backup lines, `Synced M/N tools [(K skipped)] [(dry-run)]`, separator (`sync.sh:997-1006`, `:1255-1276`).

## Output voices

- `cli_colors.sh` (command modules): `_bold` `\033[1m`, `_green` 32, `_cyan` 36, `_yellow` 33, `_red` 31, `_dim` 2, closed by `\033[0m`. Decided once at source time from `[[ -t 1 ]] && [[ -z "$NO_COLOR" ]]`.
- `logging.sh` (engine, re-evaluated per call): coloured `🔵 [INFO] `, `✅ [SUCCESS] `, `⚠️  [WARNING] ` (two spaces), `❌ [ERROR] ` on stderr, `✅ [DONE] `; plain `[INFO] ` and so on. `log_step` is `   📁 <msg>`; `log_separator` is 63 × `═`. `display_path` strips `REPO_ROOT/`, folds `$HOME` to `~/`.
- `doctor.sh`: `    ✓`, `    ⚠`, `    ✗`, `    ·`, separator 60 × `─` with a two-space indent.
- The two families never mix inside one module.

## Exit codes

```
0     success; --help
1     generic failure; check out of sync; unknown command; engine not found
2     usage or precondition: run from inside .ai/, unknown option, profile arguments, adopt targets
3     migrate with AGENTSYNC_NO_CLIPBOARD=1; parse_yaml_bool_strict non-boolean
130   interactive prompt cancelled
128+n signal, re-raised by the handlers
doctor: 0 clean or advisories, 1 warnings, 2 errors
```

## Environment variables

`AGENTSYNC_HOME`, `AGENTSYNC_REPO_ROOT`, `AGENTSYNC_CONFIG_PATH`, `AGENTSYNC_VERSION` (installer), `AGENTSYNC_NO_UPDATE_CHECK`, `AGENTSYNC_NO_CLIPBOARD`, `AGENTSYNC_NO_AUTO_SYNC`, `AGENTSYNC_SKIP_HOOKS`, `AGENTSYNC_SKIP_POST_SYNC`, `AGENTSYNC_ALLOW_POST_SYNC`, `AGENTSYNC_INTERNAL_SKIP_BACKUP`, `AGENTSYNC_BACKUP_LIMIT`, `AGENTSYNC_BACKUP_MAX_AGE_DAYS`, `AGENTSYNC_RUN_TMPDIR`, `AGENTSYNC_RUN_TMPDIR_OWNER`, `AGENTSYNC_LAST_PWD`, `AGENTSYNC_BUSY`, `AGENTSYNC_INSTALL_URL`, `AGENTSYNC_REPO`, and from the migration `AGENTSYNC_NATIVE`, `AGENTSYNC_NATIVE_BIN`, `AGENTSYNC_ENGINE_VERSION`. Plus `NO_COLOR`, `TMPDIR`, `HOME`, `LC_ALL`.
