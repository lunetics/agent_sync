# Bash → Rust module map

Tier order is the porting order: a module ports only after everything it depends on. The Rust path is the planned home; check `src/` for what already exists.

## Engine modules

```
Tier 0 — no dependencies
lib/helpers/yaml.sh              → src/yaml_subset.rs      value(), list(); mirrors the reader, no YAML crate
lib/helpers/filters.sh           → src/filters.rs          matches_filter; exclude wins, empty include = all
lib/helpers/cli_colors.sh        → src/style.rs            bold/green/cyan/yellow/red/dim, pad_right
lib/helpers/logging.sh           → src/log.rs              plain and coloured tags, [DONE], step, separator, tail, streaming sink; display_path is in src/paths.rs
lib/helpers/tmp.sh               → src/staging.rs          tmp_sibling + mv as write_beside; no run directory, overlays are virtual
lib/helpers/resolve.sh           → (none)                  engine dir lookup; templates are embedded
(bash builtins)                  → src/text.rs             read loops, [[:space:]], printf '%b', $(...) newlines, JSON/TOML escapes
(check.sh tar copy)              → src/workspace.rs        in memory for check; on disk for sync, with /<agentsync> and /<agentsync-overlay> kept virtual
(trap INT TERM HUP)              → src/interrupt.rs        signal-hook flags; restore at the next step, then re-raise

Tier 1
lib/helpers/version.sh           → src/version.rs          version_pin mode, mismatch error and hint; engine_version stays in src/lib.rs
lib/helpers/project_config.sh    → src/project_config.rs   project_config_path_r over an is_file probe; shared by sync, check, list
lib/helpers/format.sh            → src/format_rev.rs       engine and project revision (Phase 4g), read by doctor (4j); the terminal notice stays in the dispatcher
lib/helpers/paths.sh             → src/paths.rs            normalise, containment (lexical for check, through the disk for sync), repo-relative, ai_dir_enclosing_root, find_workspace_ai_dirs, find_parent_ai_src; explicit source roots trusted through AGENTSYNC_EXTERNAL_SOURCE_ROOTS, escaping source-link scan
lib/helpers/tool_resolver.sh     → src/tool.rs, src/catalog.rs, src/payload.rs; source.tools as Session::tools_dir
lib/helpers/profiles.sh          → src/profiles.rs         names, overlay dir, tools, active, rewrite_dest

Tier 2
lib/helpers/manifest.sh          → src/session.rs (record_write, was_touched, record_tree, may_prune), src/manifest.rs (load, drift, write, update_entry (Phase 4f), entries (4j))
lib/helpers/file_ops.sh          → src/file_ops.rs         ensure_dir, cleanup_path, copy_file, sync_dir, prune-vs-preserve

Tier 3
lib/helpers/rule_operations.sh   → src/rules.rs            headers, frontmatter merge, append_imports, merge_to_file, inliners, commands as skills
lib/helpers/format_conversion.sh → src/convert.rs          frontmatter and per-file converters; directory loops live in src/rules.rs
lib/helpers/opencode.sh          → src/opencode_json.rs    awk composer, exit codes 20-26; settings_has_mcp (4j)
lib/helpers/shared.sh            → src/overlay.rs          shared, base-src, and profile overlays; shared and base-src mirror the resolved sources; shared parent merge for check
lib/helpers/gitignore.sh         → src/gitignore.rs        managed block between START/END markers
lib/helpers/backup.sh            → src/backup.rs           same on-disk layout; create, restore, latest, list, prune; backup.retention (configure, Retention), validated before sync and rollback write
lib/helpers/backup_state.sh      → src/witness.rs          after.tsv post-state-v2: print, seal, preflight, first difference
lib/helpers/yaml_edit.sh         → src/yaml_edit.rs        set_scalar, list_append, list_remove, find_key_line (Phase 4a), remove_key (Phase 4c); rename_key waits for a caller
lib/helpers/template_manifest.sh → src/template_manifest.rs   hash (4e); load, lookup, remove, write (4g); record and heal (4h)
lib/helpers/snapshot.sh          → src/snapshot.rs         read_pending_pairs, clear_pending (Phase 4c); save, diff, conflicts wait for update
lib/helpers/prompts.sh           → src/prompts.rs          confirm on /dev/tty (Phase 3); multiselect through stty (4i)
lib/helpers/edit_paths.sh        → src/edit_paths.rs       block for enable (Phase 4a); checklist for doctor (4j)

Commands
lib/helpers/list.sh              → src/cli/list.rs         Phase 1
lib/check.sh                     → src/cli/check.rs        render + compare, no tar; Phase 2, ported
lib/sync.sh                      → src/render.rs (stages) + src/cli/sync.rs (transaction); Phase 3, ported
bin/agentsync.sh workspace fan-out → src/cli/workspace.rs  Phase 3, ported
lib/helpers/backup.sh (rollback) → src/cli/rollback.rs     Phase 3, ported; preflight, --force, sealed safety snapshot
lib/helpers/enable.sh            → src/cli/enable.rs       Phase 4a, ported
lib/helpers/customize.sh         → src/cli/{customize,show,diff}.rs   Phase 4b, ported; diff -u spawned for payload hunks
lib/helpers/simplify.sh          → src/cli/simplify.rs     Phase 4c, ported
lib/helpers/resolve_cmd.sh       → src/cli/resolve.rs      Phase 4c, ported
lib/helpers/profile.sh           → src/cli/profile.rs      Phase 4d, ported
lib/helpers/init.sh              → src/cli/{init,upgrade_config}.rs   Phase 4d (upgrade_config) and 4i (init), ported
lib/helpers/refresh.sh           → src/cli/refresh.rs      Phase 4h, ported
lib/helpers/dedupe.sh            → src/cli/dedupe.rs       Phase 4e, ported
lib/helpers/migrate.sh           → src/cli/migrate.rs      Phase 4g, ported
lib/helpers/adopt.sh             → src/cli/adopt.rs        Phase 4f, ported; Resolver serves init's adoption (4i)
lib/helpers/doctor.sh            → src/cli/doctor.rs       Phase 4j, ported; exit 0/1/2 = clean/warnings/errors
lib/helpers/add.sh               → src/cli/add.rs          Phase 4k, ported; the awk merge as merge(), content templates through catalog::content_template
lib/helpers/export.sh            → src/cli/bundle.rs       Phase 4l, ported; tar through the executable
lib/helpers/import.sh            → src/cli/bundle.rs       Phase 4l, ported; curl and tar through the executables
lib/helpers/generate.sh          → src/cli/generate.rs     Phase 4m, ported; prompt embedded
lib/helpers/shell_init.sh        → src/cli/shell_init.rs   Phase 4m, ported; stdout carries the snippet alone
lib/setup_hooks.sh               → src/cli/setup_hooks.rs  Phase 4m, ported; git through the executable
bin/agentsync.sh print_usage, the --help interception, *) → src/cli/usage.rs   Phase 5a, ported
lib/helpers/update.sh            → src/cli/update.rs       Phase 5, binary self-replace
lib/helpers/release.sh           → src/cli/release.rs      Phase 5b, ported; git through the executable, the tag message on its stdin
.github/workflows/auto-tag.yaml  → .github/workflows/release.yml  Phase 5c; dist 0.32.0 generates it from dist-workspace.toml (workflow_dispatch), auto-tag dispatches it on the tag
```

## Command closure and ownership

`bin/agentsync.sh:328-408` is the dispatch. `_need` sources `lib/helpers/<mod>.sh` in-process; `cmd_engine` runs `lib/<script>.sh` as a subprocess. Always loaded: `cli_colors`, `resolve`, `update`, `tmp`.

```
command        _need / sourced modules                                              tty  external          writes
init           prompts yaml logging tool_resolver template_manifest paths filters   yes  sed awk, sync.sh  .ai/, config, workflows
               file_ops manifest tmp backup adopt format init
sync           sync.sh sources logging tmp yaml version paths filters file_ops      no   sed find sha256   outputs, manifest, .gitignore, backups
               backup rule_operations format_conversion gitignore tool_resolver
               manifest shared profiles opencode
check          check.sh sources tmp yaml version; re-runs sync.sh                   no   tar cmp tail      temp only
rollback       prompts paths backup                                                 yes  date mktemp tar   restores targets
list / ls      yaml tool_resolver customize list                                    no   none              none
enable         prompts yaml yaml_edit tool_resolver edit_paths enable               yes  awk               agent_sync.yaml, overrides
disable        yaml yaml_edit tool_resolver enable                                  no   awk               agent_sync.yaml
add            add                                                                  no   awk sed           .ai/src/*
customize      yaml yaml_edit tool_resolver customize                               yes  diff sed          .ai/src/tools/*
simplify       yaml yaml_edit tool_resolver customize simplify                      yes  none              only with --apply
migrate        prompts yaml yaml_edit tool_resolver template_manifest format migrate yes clipboard find    only with --apply
show / diff    yaml yaml_edit tool_resolver snapshot customize                      no   sed / diff        none
resolve        yaml yaml_edit tool_resolver snapshot customize resolve_cmd          yes  sed               override YAML
doctor         yaml tool_resolver project_config edit_paths opencode format doctor  no   python3|node find none
dedupe         yaml yaml_edit prompts paths template_manifest dedupe (+ sourced)    yes  diff find         deletes duplicates
adopt          yaml tool_resolver paths logging filters file_ops prompts manifest   yes  diff sed find     .ai/src/*, manifest
profile        yaml yaml_edit tool_resolver profiles paths logging prompts profile  yes  awk               variants, overlay, config
generate       prompts generate                                                     yes  clipboard         none
setup-hooks    setup_hooks.sh sources yaml                                          no   git diff date     .git hooks dir
shell-init     logging shell_init                                                   no   command -v        none
export         yaml export                                                          no   tar find          archive
import         yaml export import                                                   yes  curl tar find     .ai/src/
refresh        yaml export prompts template_manifest refresh                        yes  diff find         .ai/src/*, template manifest
update         yaml snapshot (+ update)                                             no   git curl          install dir
upgrade-config prompts yaml tool_resolver init                                      no   awk sed           agent_sync.yaml
release        release                                                              yes  git awk           VERSION, Cargo.toml, Cargo.lock, tag, push
version        none                                                                 no   none              none
help           none                                                                 no   none              none
```

Known gaps in the `_need` lists: `doctor` and `dedupe` omit `logging` although the helpers they source call `log_*`. Treat `logging` as an unconditional dependency of `paths`, `shared`, `manifest`, and `file_ops`.

## bats ownership

```
tests/native_parity.bats 70   Bash vs native per ported command (Phase 1 on)
tests/sync.bats 55            sync end to end
tests/refresh.bats 38         refresh
tests/init.bats 37            init, add, customize, enable, sync
tests/doctor.bats 37          doctor (+ init add customize dedupe enable list upgrade-config as fixtures)
tests/files.bats 36           unit: cli_colors logging filters file_ops rule_operations format_conversion
tests/add.bats 36             add, sync
tests/rollback_preflight.bats 30  rollback refuses foreign changes, even with --yes
tests/adopt.bats 29           adopt, sync
tests/source_overrides.bats 28  source.* layouts outside .ai/src, external roots, doctor
tests/drift.bats 28           sync drift, --force, --dry-run, doctor
tests/paths.bats 27           unit: paths.sh
tests/migrate.bats 22         migrate, check, doctor
tests/profiles.bats 20        profile, sync --profile, check, enable
tests/update_snapshot.bats 20 snapshot.sh
tests/opencode.bats 19        sync OpenCode composition
tests/resource_resolver.bats 19  resolution order via sync, customize, init
tests/backup.bats 18          unit: paths.sh + backup.sh
tests/guard.bats 18           guard output, adopt, doctor, init, profile
tests/release.bats 18         release
tests/backup_retention.bats 17  backup pruning by count and age through real commands
tests/sync_options.bats 17    --only/--skip, check, disable
tests/bundle.bats 16          export, import (archive, directory, curl stand-in)
tests/simplify.bats 16        simplify, customize
tests/hooks.bats 16           setup-hooks, init, sync
tests/enable.bats 15          enable, disable, add, customize
tests/init_flow.bats 15       init first sync, check
tests/shell_init.bats 15      shell-init, sync
tests/tmp.bats 15             unit: tmp.sh
tests/changelog_render.bats 13  update changelog renderer
tests/customize.bats 13       customize, show, diff, enable
tests/shared.bats 13          sync with shared:, init, enable
tests/version_pin.bats 13     pin across sync, check, init, update, upgrade-config
tests/base_skills.bats 12     engine-owned skill layer: sync, check, init
tests/dedupe.bats 12          dedupe
tests/baseline.bats 11        first-sync replacement warning, adopt, rollback
tests/format_migration.bats 11  migrate, init, doctor, sync, upgrade-config
tests/check.bats 10           check
tests/outputs_mode.bats 10    init --outputs, sync, profile
tests/native_dispatch.bats 9  dispatcher gating (Phase 1)
tests/cli.bats 8              help, version, unknown command, rollback --help
tests/list.bats 8             list, ls, enable
tests/team_workflow.bats 8    committed vs local outputs
tests/config_safety.bats 7    config selection fails closed before a write sync; doctor
tests/generate.bats 7         generate
tests/gitignore.bats 7        unit: gitignore.sh
tests/workspace.bats 7        sync --workspace
tests/install.bats 6          install.sh, update <version>
tests/rollback.bats 6         rollback
tests/update.bats 6           update
```

Counts are `@test` cases at the time of writing; recount with `grep -c '^@test' tests/<file>` when it matters.
