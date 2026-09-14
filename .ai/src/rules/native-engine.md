---
paths:
  - "src/**"
  - "Cargo.toml"
  - "tests/*.rs"
  - "tests/native_*.bats"
---

# Native Engine Rules

The Rust crate at the repo root replaces the Bash engine one command at a time behind `_native_try` in `bin/agentsync.sh`. Until cutover (`docs/specs/2026-09-12-rust-migration-design.md`, Phase 5) the Bash implementation is the reference and the bats suite is the contract.

## Toolchain

- Edition 2024, `rust-version = "1.85"`, `unsafe_code = "forbid"`. `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` stay clean.
- `VERSION` is the only version source. The crate reads it with `include_str!`; `Cargo.toml` stays at `0.0.0` until Phase 5 wires cargo-dist.
- Templates embed from `lib/templates/` through `include_dir!`. The binary never looks up an engine directory at runtime.
- Dependencies: clap, include_dir, sha2, signal-hook, thiserror; dev: assert_cmd, predicates, tempfile. Add a crate only for a concrete command need. A YAML parser is never added: `yaml_subset` mirrors `lib/helpers/yaml.sh` by design.

## Structure

- `src/main.rs` is the only process-aware file: arguments, `ExitCode`, the `AGENTSYNC_ENGINE_VERSION` guard. Everything else is a library with a `Result<_, Error>` API.
- `src/cli/<cmd>.rs` owns one command as `render(…) -> Result<String, Error>` plus `run(…, &mut impl Write)`. A command with its own exit status returns it from `run` — `check` through a `Report`, `sync` and `rollback` as a `u8` — and writes only through the writers or the log sink `main` hands it. Core modules never print.
- `src/error.rs` is the single error type. Each variant's `Display` text is the Bash message it replaces, and `main` maps the variant to the Bash exit code.
- Two output voices, kept apart as in Bash: `style` mirrors `cli_colors.sh` for command modules; `log` mirrors `logging.sh` for the engine. Neither leaks into the other's module.
- Bash semantics are mirrored, quirks included; the numbered list in the design spec is the allowed set. Improving behaviour during a port is not allowed; a deliberate change is one line under "Accepted deviations" in the spec.

## Parity

- A command is ported when `_NATIVE_COMMANDS` lists it, its bats file passes with `AGENTSYNC_NATIVE=1`, and `tests/native_parity.bats` covers its situations.
- Output matches Bash byte for byte on stdout, stderr, and exit status when stdout is not a terminal; on a terminal the escape codes are those of `cli_colors.sh`.
- A unit test asserts the value the Bash helper produces, confirmed by running the helper, never derived from reading it.
- A Bash bug found by a fixture is fixed in Bash first, in its own commit with a regression test.
- Tests are hermetic: `tempfile`, no network, no dependency on the developer's `~/.agentsync`.
