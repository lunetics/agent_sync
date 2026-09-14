# RFC: Skill Cards — requirements, runtime fit, and evidence

Status: **Proposed; no runtime implementation or new CLI commands yet.**
Date: 2026-09-14.
Base: AgentSync `0.36.0`, `b0a962437f1512b5d0a7b7774a4ece51ede4b433`.
Branch: `codex/skill-cards-rfc`.

## 1. Outcome

Give every discovered skill a compact, explainable card answering:

1. What does it do, when should I use it, and who maintains it?
2. Can my selected CLI, IDE, custom agent, or standalone environment use it?
3. What tools, permissions, data access, and dependencies does it require?
4. Is that compatibility merely declared, statically assessed, or actually tested?
5. Where is the authoritative source, and which revision was checked?

Cards are generated views of existing skills, **not another skill format, an
installer, a permissions grant, a registry service, or a test runner**.
Existing skills without extra metadata still get a card, with unknown fields.

## 2. Design principle: three separate inputs

```text
SKILL.md + optional namespaced metadata       author-declared requirements
selected runtime/environment profile         available capabilities and policy
trusted test records for exact revisions     evidence of observed behavior
                         |
                         v
                 one normalized card
                         |
             terminal / JSON / Markdown
```

A distribution adapter answers where AgentSync writes a skill. It does not prove
that the receiving runtime discovers, activates, or successfully executes it.
Likewise, a skill saying it needs shell access neither grants shell access nor
proves that shell access is available. Keep these distinctions in the UI.

## 3. What standalone means

| Mode | Meaning | What it does not imply |
| --- | --- | --- |
| Agent | An agent interprets the instructions; it may be a CLI, IDE, hosted product, or custom agent. | A CLI brand or specific model is required. |
| Manual | The author supplies a documented procedure a person can follow. | Arbitrary agent prompts are automatically good human procedures. |
| Program | A documented entrypoint can run without an LLM interpreting SKILL.md. | Having a scripts directory makes the whole skill runnable. |

“Standalone agent” is an **agent runtime family**, not the same as program mode.
Record deployment separately: local, container, or remote. A custom agent may
support file reads and MCP while lacking an interactive shell; it can still be a
valid target for a skill whose requirements match.

Assess each declared mode independently. Dependencies of a helper script must
not be mistaken for the complete prerequisites of the agent workflow.

## 4. Card layout

Illustrative rendering, **not an assessment of an existing package**:

```text
PDF Summary                                      v1.2 • example/pdf-summary
Summarize local PDFs with citations to page numbers.

Selected target: custom agent / Linux / container
Mode: Agent        Fit: BLOCKED          Evidence: AUTHOR-DECLARED
Reason: required shell.exec is denied by the selected profile.

Modes       Agent; Program (scripts/extract.sh)
Requires    Agent: filesystem.read, shell.exec; pdftotext
Access      Reads input documents; writes an output file; network not required
Support     Claude and Codex named by the author; no trusted test record
Source      repository + skill-relative path + content digest
Details     requirements • target comparison • evidence • source
```

The list view shows name, short description, selected-target fit, evidence level,
and the first actionable reason. The detail view exposes the complete comparison,
source precedence, shadowed copies, requirements, and dated test evidence.
Use text labels as well as colors. Escape terminal control characters, Markdown,
and JSON correctly; metadata is untrusted display data.

## 5. Metadata: one source, standard-compatible extension

Reuse `name`, `description`, `license`, and the human-readable `compatibility`
field. Keep additional values as **strings** inside `metadata`; prefix the new
keys with `agentsync-`. Do not introduce custom top-level fields or put arrays
and nested objects inside the standard metadata map.

Illustrative source fragment; it describes a fictional skill and claims no tests:

```yaml
---
name: pdf-summary
description: Summarize local PDF documents with page references.
license: MIT
compatibility: Agent mode needs file access and a shell; program mode needs Bash and pdftotext.
metadata:
  author: example
  version: "1.2.0"
  agentsync-card-version: "1"
  agentsync-modes: "agent,program"
  agentsync-agent-requires: "filesystem.read,filesystem.write,shell.exec"
  agentsync-agent-tools: "pdftotext"
  agentsync-agent-clients: "claude,codex"
  agentsync-program-entrypoint: "scripts/extract.sh"
  agentsync-program-requires: "filesystem.read,filesystem.write"
  agentsync-program-tools: "bash,pdftotext"
  agentsync-platforms: "linux,macos"
  agentsync-network: "none"
  agentsync-tags: "documents,pdf,summarization"
---
```

The namespace and vocabulary above are an AgentSync proposal, **not additions
already accepted by the Agent Skills standard**. `version` is a display label;
revision identity comes from source and content, not a self-reported version.
`description` remains the activation description; cards do not replace or shorten
the description sent to an agent. Rich details load only on request.

### Initial field contract

| Fields | Interpretation |
| --- | --- |
| `agentsync-card-version` | Exact extension schema version; initially `1`. Unknown versions retain a basic card and an unsupported-schema diagnostic. |
| `agentsync-modes` | Set drawn from `agent,manual,program`. Absent means the extended mode declaration is unknown. |
| `agentsync-<mode>-requires` | Required capability IDs for that mode. Absent is unknown; explicit `none` is an author declaration of no extra capability requirements. |
| `agentsync-<mode>-tools` | Required executable names, descriptive in the first release; no version constraints or automatic invocation. |
| `agentsync-agent-clients` | Author-declared intended client IDs, not an exclusive allowlist or test result. An unnamed client is unknown, not unsupported. |
| `agentsync-program-entrypoint` | Package-relative regular file; display only. Program mode without it is incomplete. |
| `agentsync-manual-guide` | Package-relative documentation; required for a declared manual mode. |
| `agentsync-platforms` | Declared set of OS IDs, or explicit `any`; missing remains unknown. |
| `agentsync-network` | `none`, `required`, or `optional`, applying to every declared mode. Different per-mode network policies require a later schema revision. |
| `agentsync-tags` | Optional descriptive labels; never selection or authorization policy. |

Token sets use comma-separated lowercase IDs, trim surrounding spaces, disallow
empty items and duplicates, and never use shell splitting/globbing. `none` and
`any` are reserved where listed, not ordinary IDs. Values cannot express commands.
Initially supported capability IDs: `filesystem.read`, `filesystem.write`,
`shell.exec`, `network.http`, `browser.interact`, `mcp.client`, `git.read`,
`git.write`, and `user.prompt`. Unknown IDs produce unresolved requirements.
`mcp.client` alone does **not** prove a particular server or tool is available.
Specific MCP services, credentials, dependency versions, and optional features
remain explicit unresolved detail until their structured vocabulary is designed.

Allow only a bounded metadata syntax in the first implementation: an indented
`metadata:` mapping of simple scalar strings, no YAML aliases, tags, merge keys,
flow objects, or executable interpolation. Standard descriptions may use literal
and folded block scalars. An unsupported representation must produce a diagnostic,
never silently remove requirements. Preserve original files verbatim.

## 6. Compatibility is a result, not a badge written by the author

Use independent axes in the normalized output:

- **Declaration:** what the author says, including intended clients and modes.
- **Fit:** `compatible`, `conditional`, `blocked`, `unknown`, or `not-applicable`.
- **Evidence:** `declared`, `static`, or `tested`; plus `current`/`stale` for test relevance.

Examples: `compatible / static` is not `tested`. A passing file-distribution test
is not a passing native-discovery test. A skill tested yesterday may be blocked
today by a changed permission profile.

Decision order for a selected mode and target:

1. Invalid/unreadable metadata makes the assessment unknown with diagnostics.
2. An explicitly excluded mode is not-applicable.
3. A known missing mandatory capability or denied required access is blocked.
4. An unresolved requirement, unknown dependency version, or unknown runtime
   property is unknown. Missing metadata must never turn green by default.
5. Conditional applies only to explicitly modeled, user-actionable conditions,
   such as a required permission awaiting approval; list every condition.
6. Otherwise the declared requirements statically match: compatible/static.
7. Overlay trusted relevant test evidence without overwriting the current fit.

Conflict rules: current explicit denials outrank old passing tests. Conflicting
records for the same tuple display conflict/unknown until reconciled; do not pick
the most favorable result. Absence of a runtime profile is unknown, not a failed
runtime test. No semver compatibility is inferred from a neighboring version.

### Target profiles and evidence

Runtime profiles are separate from skills and from AgentSync output templates.
They identify runtime family (`cli`, `ide`, `hosted-agent`, `custom-agent`,
`manual`, `program`), client, exact version when known, OS/architecture,
deployment, capabilities, dependency observations, and permission policy.
Profiles supplied as test fixtures are not assertions about a user's live machine.

Keep owner-maintained profiles and accepted test evidence under a proposed
`.ai/skill-cards/` tree, outside the copied skill payloads. Profiles use their own
versioned schema. Runtime capability declarations from upstream documentation are
not interchangeable with observed local permissions. Name aliases require an
explicit mapping; model providers are not runtime clients.

A trusted assessment binds: skill source identity and digest, mode, runtime ID
and exact version, profile digest, OS/architecture, test stage, result, date,
reviewer/runner, and evidence reference. Stages: parse, distribute, discover,
activate, execute. “Tested” always names the stage and environment; only an execute
test supports an execution claim. Changing any binding retires the matching badge.
Skill-supplied evidence is untrusted author evidence until accepted separately.
Never auto-run referenced tests, fetch evidence URLs, or treat signatures as proof
of functional correctness.

## 7. Integration with AgentSync

Start with read-only commands in a new namespace, provisionally:

```text
agentsync skills list
agentsync skills show pdf-summary
agentsync skills show pdf-summary --target my-codex-profile --mode agent
agentsync skills list --format json
agentsync skills list --format markdown
agentsync skills validate
```

These commands are **proposed**. Existing `agentsync list` lists tools and
`agentsync show <tool>` shows configuration; do not repurpose either command.
Target selection consumes an explicitly chosen profile; it must not launch the
named CLI, scan global credentials, or make an API/model call to guess features.

Resolve source, layers, profiles, per-tool overrides, and include/exclude rules
through the same effective-source logic as sync. Distinguish “available source”
from “selected for this target.” Show winner and shadowed candidates. Give
same-name skills an identity based on source origin + relative path + digest;
never use the display name as a globally unique key. Include provenance for
engine-owned and generated command-skills without pretending they are ordinary
authored skill packages. Generated command-skills can be marked unevaluated in v1.

Use the 0.36.0 external-source trust checks before reading any external metadata.
An entrypoint, guide, evidence path, or symlink must not widen that trust. Traverse
boundedly; guard link cycles, oversized files and terminal escapes. Do not
dereference mutable data links to hash entire knowledge stores. Use separate
package-file, symlink-text, and dependency identities; external data is out of
the package digest, visibly declared rather than silently treated as validated.

List/show write only to stdout. Export destinations are caller-controlled shell
redirection or a later explicitly authorized export operation. No background
scan, global skill crawling, remote catalog, telemetry, or permission changes.
Validation is read-only. Existing sync/check/rollback outputs remain byte-identical
when cards are not explicitly involved. Metadata cannot silently exclude skills.

## 8. Implementation constraints and alternatives

AgentSync 0.36.0 promises no Node/Python/jq/yq runtime dependency. Preserve that
contract for cards. Its existing `read_frontmatter_field` handles top-level scalar
fields, not nested metadata; `parse_yaml_value_r` is a limited config parser, not
a complete frontmatter parser. Do not run it over a Markdown body and accidentally
parse an example as metadata. A bounded dedicated reader needs its own fixtures.

Namespaced hyphenated keys fit the existing parser's identifier vocabulary better
than dotted keys, but reuse only after tests prove quoting, blocks, duplicate-key
handling, leading delimiters, and boundary behavior. JSON output needs complete
control-character escaping; existing `_json_escape` is not a license to skip tests.

Rejected for the first iteration:

- A separate hand-maintained card file duplicating title/description: introduces drift.
- A giant nested compatibility matrix in SKILL.md: conflicts with string metadata
  and couples each author to every CLI release.
- A general-purpose Bash YAML/semver engine: too large for the first feature.
- Automatically labeling all copied skills compatible: conflates distribution and execution.
- Running dependencies to detect support: can start paid agent calls or execute untrusted code.
- A2A Agent Card reuse as the on-disk format: that describes a service/agent,
  whereas this proposal describes a portable instruction package. An export
  bridge might be useful later; the two objects are not interchangeable.

## 9. Small upstream PRs

| PR | Scope | Acceptance boundary |
| --- | --- | --- |
| 1 — Cards and metadata | Spec, bounded parser, list/show, basic human/JSON/Markdown views. | Every discovered skill has a card; absent metadata is unknown; no sync changes. |
| 2 — Runtime fit | Owner-selected profiles, capability vocabulary, reasoned matching, fixture profiles. | All decision-table cases pass; no CLI invocation, secret reads, or invented version support. |
| 3 — Evidence and standalone | Trusted staged assessments, exact revision binding, program/manual details. | Stale/conflicting evidence visible; entrypoints remain display-only; no implicit execution. |
| 4 — Optional policy integration | Explicit opt-in reporting/filtering after separate design. | Preserve legacy defaults; include migration and negative tests before any filtering. |

Start with PR 1. Do not turn the proposal into one large PR spanning parsers,
runtime inventory, a public registry, synchronization policy and a graphical app.

## 10. Acceptance cases

1. A normal name/description-only skill renders; compatibility remains unknown.
2. Folded descriptions, CRLF, quotes, comments, Unicode, EOF, duplicate keys and
   unsupported YAML have deterministic outcomes; body examples are never parsed.
3. A fictional read-only skill matches an explicitly supplied read-capable profile.
4. A required shell capability denied by policy yields blocked, even with an older pass.
5. A custom agent with matching capabilities is assessed without a known CLI brand.
6. Program mode with no safe regular-file entrypoint is incomplete, never executable.
7. Manual mode requires its guide; program and agent dependencies remain separate.
8. A digest/runtime/profile change makes old execute evidence stale.
9. An author-provided “tested” string cannot impersonate accepted evidence.
10. External roots, escaping links, cycles, special files, hostile display strings,
    giant metadata and untrusted test commands cause no execution or boundary escape.
11. Names collide across layers: show selected provenance and shadowing, not one merged card.
12. Golden sync/manifest fixtures stay unchanged; list/show/validate create no backups,
    caches, config changes, generated instructions or installed skills.

Use synthetic fixtures for these cases. Follow with separately authorized native
discovery tests against exact Claude/Codex versions; record results, not promises.

## 11. Questions for review

- Is “standalone” primarily a custom agent, a direct program, or both? The model
  accommodates both; the first UI can emphasize the relevant one.
- Is the first user a person browsing skills or an orchestrator selecting them?
  Recommendation: both consume one schema; ship terminal plus JSON first.
- Which facts justify a “tested” badge? Recommendation: always expose test stage,
  exact environment and date, never a timeless green client logo.
- Keep declarative metadata advisory initially? Recommendation: yes; enforcement
  changes synchronization semantics and deserves an independent decision.

## 12. Sources and scope

Primary sources checked 2026-09-14:

- [Agent Skills specification](https://agentskills.io/specification): existing
  compatibility field, string-valued metadata, optional package resources.
- [Client integration guide](https://github.com/agentskills/agentskills/blob/main/docs/client-implementation/adding-skills-support.mdx):
  different local/cloud discovery and skill-loading mechanisms.
- [A2A agent-card tutorial, v0.3.0](https://a2a-protocol.org/v0.3.0/tutorials/python/3-agent-skills-and-card/):
  comparison only, not a claim of implementing A2A or tracking its latest version.
- [AgentSync 0.36.0 source](https://github.com/yelmuratoff/agent_sync/tree/b0a962437f1512b5d0a7b7774a4ece51ede4b433):
  README runtime contract; `bin/agentsync.sh`, `lib/helpers/list.sh`,
  `customize.sh`, `format_conversion.sh`, `yaml.sh`, `paths.sh`, `lib/sync.sh`.

All new metadata keys, modes, profiles, commands and matching rules in this RFC
are design proposals. None of the example client labels is a new compatibility
certification. Existing runtime settings and distributed skill packages are unchanged.
