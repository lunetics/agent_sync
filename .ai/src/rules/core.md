# Core Rules

The binary runs on macOS, Linux, and Windows, and `cargo test` gates all three. Failures surface, and changes stay inside the requested scope.

## Shell Script Quality

Applies to the shell that remains: `install.sh` and `lib/templates/guard/claude.sh`.

- Keep `set -euo pipefail` enabled in executable entry points. Sourced helpers inherit the caller's shell options.
- Quote expansions unless intentional splitting or glob expansion is part of the contract.
- Declare function-scoped variables with `local`.
- Reach for `[[ ]]` over `[ ]`, and `$(command)` over backticks.
- Stay compatible with Bash 3.2: no associative arrays, namerefs, or `mapfile`.

## Portability

POSIX-compatible flags only — `sed`, `grep`, `readlink`, `find` ship in different flavours across platforms. Examples:

```bash
# In-place edits — write to a temp file and move it
- sed -i 's/foo/bar/' "$file"        # GNU-only
- sed -i '' 's/foo/bar/' "$file"     # BSD-only
+ sed 's/foo/bar/' "$file" > "$tmp" && mv "$tmp" "$file"

# Absolute paths — use cd+pwd
- realpath "$path"                   # not on macOS by default
+ (cd "$(dirname "$path")" && pwd)

# Resolve symlinks — manual loop
- readlink -f "$link"                # GNU-only
+ while [[ -L "$target" ]]; do target=$(readlink "$target"); done

# Line-by-line reads — POSIX everywhere
- mapfile -t lines < "$file"         # bash 4+
+ while IFS= read -r line; do ...; done < "$file"
```

In Rust, engine paths are `/`-separated strings (`src/paths.rs`): translate disk paths through `from_disk`/`DiskText`, never format a `PathBuf` into engine output. Run `cargo test` locally, and ShellCheck on a changed script; CI confirms Linux, macOS, and Windows.

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
