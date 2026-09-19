---
paths:
  - "src/**"
  - "lib/**"
  - ".ai/src/tools/**"
---

# Architecture Rules

The sync engine is config-driven: shipped tool behaviour lives in `lib/templates/tools/*.yaml`, project overrides live in `.ai/src/tools/`, and generic engine modules in `src/` do the work. Extend by adding a YAML option, then handling it in a module — not by branching on a tool name.

## Config-Driven Sync Engine

- Declare shipped behavior in `lib/templates/tools/*.yaml` and repository-specific differences in `.ai/src/tools/*.yaml`. `render::run_passes` reads the resolved `targets.*` values of each `Tool` and routes to the generic copy, conversion, and composition modules.
- Express format differences through YAML target options (`extension`, `header`, `merge_to_file`, `inline_into_agents`, `prepend_agents`, `append_imports`, `as_skills`). Add a new option and a module path rather than an `if slug == "cursor"` branch.
- Resolve source paths through `overlay::Sources`: project `agent_sync.yaml` → global `config.yaml`. Read a tool field through `Tool::value`, which layers base, user, and profile YAML.
- Read defaults (`enabled`, `cleanup`) from `config.yaml` once, then propagate from there.

## Module Map

```
src/main.rs                    → process boundary: arguments, exit codes, environment.
src/cli/<cmd>.rs               → one command each; `usage.rs` owns help and the unknown-command refusal.
src/render.rs / session.rs     → sync and check orchestration: overlays, catalog, tool/profile passes, checkpoint.
src/yaml_subset.rs / yaml_edit.rs → supported YAML scalar, nesting, and list shapes; line-preserving edits.
src/tool.rs / catalog.rs / payload.rs → layered base/user/profile config, embedded templates, payload resolution.
src/file_ops.rs / staging.rs   → safe copying, directory sync, cleanup, write-then-rename.
src/rules.rs / convert.rs / opencode_json.rs → rule headers and merges, target format conversion and composition.
src/paths.rs / filters.rs      → containment, drive-aware `/`-separated paths, include/exclude matching.
src/backup.rs / witness.rs / manifest.rs → transactions, post-operation witnesses, ownership, and drift.
src/overlay.rs / profiles.rs / workspace.rs → source overlays, config-home variants, the virtual file tree.
src/log.rs / style.rs / prompts.rs → engine log voice, command colours, terminal prompts.
src/error.rs                   → the single error type; `main` maps variants to messages and exit codes.
lib/templates/                 → shipped tool/payload bases and init/refresh content, embedded at build time.
```

Business logic lives in the library modules. `main.rs` stays a router, and a `src/cli/<cmd>.rs` module renders through the shared modules so `sync` and `check` stay aligned.

## Hard Constraints

- **Path safety**: `Paths::resolve_dest` rejects paths outside the canonical root. Resolve through `normalize` + `canonicalize_with_existing_ancestor` — these exist so the engine never needs `realpath` semantics on a partly missing path. `Paths::is_safe_source` allows the project root, the embedded engine root, and (when active) the overlay tree that `overlay::setup_shared` builds for `shared:` and tears down at the end of the pass. Engine paths are `/`-separated strings; disk paths enter through `paths::from_disk`.
- **Zero external deps**: the standard library plus the crates in `Cargo.toml`. The binary reaches its goals without `yq`, `jq`, `python`, `node`, `perl`, or `eval` — reads YAML through `yaml_subset`.
- **YAML parser scope**: scalar keys, dot-notation nesting, and the explicitly supported list forms. New YAML shapes need a concrete engine requirement and parser tests.
- **Idempotency**: `agentsync sync` produces identical output on repeated runs. No timestamps, no ordering changes, no platform-dependent sorting. `agentsync check` verifies this.
- **Stateless runs**: read config fresh each invocation. The version comes from the `VERSION` file through `include_str!`, never embedded by hand.
- **Transactional mutation**: `init`, `sync`, and `rollback` snapshot their complete managed write set through `backup::create`. Failures restore the previous state; operations prune completed history through `backup::prune` once no restore is pending.
- **Interrupts**: a transaction arms `interrupt` before its first write. A signal is recorded, the run stops at its next step and restores, then re-raises the signal; a staging file finished with a rename uses `staging` so the rename stays on one filesystem.
- **Document new inline options** (`inline_into_agents`, `prepend_agents`, etc.) in the `agentsync` skill and `_TEMPLATE.yaml` as part of the change that introduces them.

## Data Flow

1. `render::prepare` resolves source overlays and the layered tool catalog; `render::run_passes` dispatches each target to the generic copy, conversion, composition, and manifest modules.
2. Modules operate on resolved inputs rather than tool identity; `render::checkpoint` records the witness, and a failed transaction restores before the command returns.
