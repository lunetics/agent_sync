---
paths:
  - "src/**"
---

# CLI Output Rules

What the binary prints, and why. No standards body governs terminal output, so
each rule below says whether it follows a formal standard, a published informal
one, or a decision this project made.

## Streams

- Diagnostics go to stderr, the command's own output to stdout. POSIX reserves
  stderr for diagnostics; `clig.dev` goes further and puts progress and status
  there too, so stdout stays a clean pipe. AgentSync currently streams the sync
  log to stdout, which predates that guidance. Do not move it without a release
  that says so: a script reading `agentsync sync` today reads stdout.
- A checker's report is its output. `check` printing drift on stdout is
  correct; its exit status carries the verdict.

## Exit codes

- Zero for success, non-zero otherwise, which is the only formal rule (POSIX).
- `check` follows grep's shape: zero when synced, one when out of sync. Keep
  errors distinguishable from a clean negative answer.
- Stay below 125: the shell reserves 126, 127 and everything above 128.
- `sysexits.h` is deprecated by its own manual page. Never adopt its numbers.

## Colour

- Colour when stdout is a terminal and `NO_COLOR` is unset or empty, as
  `no-color.org` specifies. That is an informal standard with wide adoption,
  not a formal one.
- Colour is emphasis, never information. A line must read the same in a pipe.
- The GNU Coding Standards argue against looking at the terminal at all. Every
  modern tool ignores that, and so do we, deliberately.

## Glyphs

**No emoji.** Not a standard: `clig.dev` permits them. It is this project's
decision, on three grounds. Of the tools worth imitating — cargo, git, ripgrep,
kubectl, docker, npm, terraform, gh — not one prints emoji by default, and
cargo's status lines are pure ASCII. Emoji with the presentation selector are
Wide under Unicode Annex #11, which warns that the property "is not intended
for use by modern terminal emulators without appropriate tailoring", so one
glyph misaligns every column to its right. And a screen reader has no text
alternative for a bare glyph, the principle behind WCAG technique H86.

- The level tag carries the level: `[INFO]`, `[WARNING]`, `[ERROR]`,
  `[SUCCESS]`, `[DONE]`. Colour decorates the tag; the words survive a pipe.
- Report markers are the narrow set `✓`, `✗`, `!`, `·`. They are not emoji and
  need no presentation selector.
- Never let a glyph be the only carrier of meaning. `✓ passed`, not a bare `✓`.
- `→` separates a source from a destination and is the one arrow in use.

## Messages

- An error names what failed and what to do next. The GNU shape is
  `program: message`, lower case, no full stop; a hint belongs on its own line,
  the way rustc writes `error:` then `help:`.
- Say the command to run, not the concept: `run agentsync sync`, never
  "re-synchronise the project".
- Never print an engine-internal path. The virtual overlay roots
  (`/<agentsync-overlay>/…`) mean nothing to a reader and currently leak into
  the sync log; a source is named by what the user wrote, or by its category.

## Quiet and machine-readable output

- There is no `--quiet` and no `--json` today. Adding them is the documented
  way to serve scripts; parsing the human log is not, and the human log may
  change between releases.
