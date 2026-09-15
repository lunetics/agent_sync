# MCP library v1 (read-only)

AgentSync can read an explicitly selected local MCP catalog without running a
server, accessing the network, or changing a project or catalog file. This is
separate from `agentsync add mcp`, which continues to manage `.ai/src/mcp.json`.

## Select a catalog

Use one of these forms:

```sh
agentsync mcp list --library catalog/mcp
agentsync mcp show example --library catalog/mcp
agentsync mcp validate --library catalog/mcp
agentsync mcp validate example --library /absolute/catalog/mcp
```

`--library` is explicit and may be absolute or relative to the selected
AgentSync root (`AGENTSYNC_REPO_ROOT`, otherwise the current directory). Without
the option, the selected `agent_sync.yaml` must contain:

```yaml
library:
  mcp:
    path: "catalog/mcp"
```

That configured path must stay under the selected root after symlinks resolve.
No directory is searched implicitly. Catalog entries use this layout:

```text
catalog/mcp/<id>/manifest.json
```

Entry IDs are 1–64 ASCII characters matching
`[A-Za-z0-9][A-Za-z0-9_-]*`, and must match both the directory name and manifest
`id`. Entry-directory symlinks and manifests resolving outside the selected
catalog are rejected.

## Commands and output

`list` prints `id<TAB>title`, sorted by ID. ASCII controls and UTF-8 C1 controls
in titles are rendered as `\t`, `\n`, `\r`, or `\u00XX` so list output cannot
inject terminal control lines. `show <id>` validates the manifest
then writes its original JSON bytes to standard output. `validate [id]` checks a
single selected entry or the whole catalog; full-catalog validation also detects
duplicate manifest IDs. Diagnostics go to standard error and return non-zero.

All three commands are offline and read-only. They do not execute `stdio`
commands, contact HTTP endpoints, read credentials, write native client config,
or run the normal CLI update check.

## Manifest schema v1

The root object permits only these fields:

```json
{
  "schema_version": 1,
  "id": "example",
  "title": "Example MCP",
  "description": "Optional display text.",
  "provenance": {
    "homepage": "https://example.invalid",
    "repository": null,
    "artifact": null
  },
  "connection": {
    "type": "stdio",
    "command": "example-mcp",
    "args": ["--safe"]
  },
  "requirements": {
    "binaries": ["example-mcp"],
    "inputs": []
  },
  "extensions": {
    "example.invalid": {"opaque": true}
  }
}
```

Required fields are `schema_version` (exactly `1`), `id`, `title`,
`connection`, and `requirements`. `description`, `provenance`, and `extensions`
are optional. Provenance permits only `homepage`, `repository`, and `artifact`,
each a string or `null`. Extension keys must be namespaced (for example,
`example.invalid`); their values are opaque JSON.

`connection.type` is exactly `stdio` or `http`. Stdio requires a non-empty
`command` and an `args` string array. HTTP requires a non-empty `url` and does
not permit command or args fields. Requirements requires `binaries` and `inputs`
string arrays. In this first read-only API, `inputs` must be empty: structured
input binding is explicitly unsupported.

The parser rejects malformed JSON, invalid UTF-8, unknown fields, unknown schema
versions, duplicate JSON object keys at any depth (including equivalent escaped
Unicode keys), duplicate catalog IDs, and unpaired surrogate escapes. It accepts valid control escapes as
JSON data. `show` does not decode and re-encode strings, so valid Unicode,
arguments, backslashes, quotes, and escaped control values retain their source
spelling.

To keep validation bounded, a manifest is limited to 131,072 bytes (with or
without a final newline), JSON nesting to 16 levels, and a catalog to 256
entries. These are intentional format limits, not general JSON Schema support.

## Development validation

The production path uses Bash and awk, with no new Python, Node, or jq runtime
dependency. The independent development probe uses Python's standard JSON
decoder with duplicate-key rejection:

```sh
python3 tests/mcp_library_probe.py
# Optional comparison, not a replacement for the strict oracle:
python3 tests/mcp_library_probe.py --compare-jq
```

The probe checks valid and malformed JSON, equivalent escaped keys, nested
extensions, UTF-8, and byte-exact `show`. It is not a complete JSON conformance
suite or proof of safe execution of a described server. The CLI does not check
whether binaries exist, endpoints are reachable, or provenance is trustworthy.

Local development validation uses a non-root user in a network-disabled,
mount-free Linux container. CI runs ShellCheck, Bats on Linux/macOS/Windows,
and the reference probe with GNU awk and mawk on Linux and native awk/Bash on
macOS. Local Linux results do not establish macOS or Windows compatibility.

Import, export, release management, input binding, and project assignment are
not implemented by these commands. The older `add mcp` workflow is unchanged.
