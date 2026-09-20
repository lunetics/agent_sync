# Core Rules

The binary runs on macOS, Linux, and Windows, and `cargo test` gates all three. Failures surface, and changes stay inside the requested scope.

## Shell Script Quality

Applies to the shell CI gates: `install.sh` and `lib/templates/guard/claude.sh`.

- Keep strict mode on: `install.sh` is bash with `set -euo pipefail`; the guard hook is POSIX `sh`, where `set -u` is the whole of it — `pipefail` is not POSIX.
- Quote expansions unless intentional splitting or glob expansion is part of the contract.
- Declare function-scoped variables with `local`.
- In `install.sh`, reach for `[[ ]]` over `[ ]`, and `$(command)` over backticks. Stay compatible with Bash 3.2: no associative arrays, namerefs, or `mapfile`.

## Portability

- Engine paths are `/`-separated strings handled by `src/paths.rs`. A disk path enters through `from_disk`/`DiskText`; a `PathBuf` is never formatted into engine output.
- The guard hook is POSIX `sh`: `[ ]` rather than `[[ ]]`, no `local`, no arrays, `printf` rather than `echo -e`.
- In either script, POSIX flags only — `sed`, `grep`, and `awk` ship in different flavours. `sed -i` has no portable spelling: write a temp file and `mv` it over the original.
- `install.sh` probes for the tool it needs (`sha256sum` then `shasum`, `curl`, `git`, `tar`) instead of assuming a platform has it.
- Run `cargo test` locally, and ShellCheck on a changed script; CI confirms Linux, macOS, and Windows.

## Scope of Changes

- Touch only what the task requires. Adjacent code stays as-is until asked.
- Three similar lines beat a premature abstraction — let real duplication drive helpers.
- Delete dead code outright; git keeps the history.
- Keep configuration within the scalar, nested-key, and supported list shapes implemented in `src/yaml_subset.rs`.

## Error Handling

- Match the surrounding command's output layer: the engine logs through `src/log.rs`; command modules use `src/style.rs` and `src/prompts.rs`.
- `src/error.rs` is the single error type; `main` maps each variant to its message and exit code. Guard expected optional inputs explicitly. Let unexpected filesystem, parser, and subprocess failures propagate as `Error`.
- Preserve meaningful non-zero exit codes and actionable stderr messages at CLI boundaries.
- In shell, let `set -euo pipefail` stay active throughout the run — fix the failing command rather than disabling strict mode for it.
