# Rust Migration Phase 3b, Family 3: Sources Outside the Project

> Execute with `subagent-driven-development` only when the user explicitly authorizes subagents/delegation; otherwise execute inline. Steps use `- [ ]` for tracking.

**Goal:** Make the native `sync`, `check`, and `list` read explicit `source.*` roots outside the project only when `AGENTSYNC_EXTERNAL_SOURCE_ROOTS` trusts them, read tool YAML and payloads from `source.tools`, build the overlays from the resolved sources, and refuse a source symlink that escapes the project, exactly as release 0.36.0's `lib/helpers/paths.sh`, `lib/helpers/shared.sh`, `lib/helpers/tool_resolver.sh`, and `lib/sync.sh` do.

**Architecture:** `Paths` gains the trusted external roots and the explicit roots a config registers: `classify_explicit_source` is `explicit_source_root_r`, `is_safe_source` admits the registered roots, and `escaping_source_link` is `refuse_escaping_source_links` over the disk. `render::resolve_sources` registers the roots before it checks `AGENTS.md`, as `_resolve_sources` does, and sets `Session::tools_dir`, which tool loading, payload lookup, and `--if-stale` read in place of `.ai/src/tools`. `render::refuse_escaping_source_links` runs between the configless refusal and the pin, in `sync` and in `check`'s render. The shared and engine-skill overlays mirror the resolved `Sources` (`build_source_overlay_tree`) instead of `.ai/src`. The workspace already reads paths outside the project root from the disk, in memory too, so `check` needs no seeding. The seam stays the CLI process boundary: `tests/source_overrides.bats` under `AGENTSYNC_NATIVE=1` plus parity fixtures; disk-touching unit tests are `#[cfg(unix)]`.

**Tech stack:** Rust 2024 edition (MSRV 1.85), clap 4.6, include_dir 0.7, thiserror 2, sha2 0.11, signal-hook 0.4; dev: assert_cmd 2, predicates 3, tempfile 3. No new dependency. Design: `docs/specs/2026-09-12-rust-migration-design.md`, "Phase 3b", family 3. Previous plan: `docs/plans/2026-09-14-rust-migration-phase-3b-backup-retention.md`.

## Global Constraints

- `.ai/src/` remains the source of truth. Outside sources are only read; destinations stay confined to the project root.
- No binary ships to users; this family changes no Bash, and `lib/**/*.sh` stays clean under `shellcheck -x -S warning -e SC1091`.
- Byte-for-byte parity on stdout, stderr, exit status, and the files left behind, except for the accepted deviations.
- Rust: `unsafe_code = "forbid"`; fmt and clippy clean after every task; no new dependency. `main.rs` stays the only file that reads the environment: `AGENTSYNC_EXTERNAL_SOURCE_ROOTS` reaches the engine through `render::Env`.
- Disk-touching unit tests are `#[cfg(unix)]`.
- `AGENTSYNC_INTERNAL_SOURCE_BASE_ROOT` is not ported: only `lib/check.sh` sets it for its isolated Bash sync, and the native `check` renders from the project root itself.
- The Phase 3 accepted deviation stands: a symlink inside a synced tree that the scan admits is copied as the file it points to.
- Commits follow Conventional Commits, scope `native`, subject at most 72 characters, no attribution trailers.
- Run bats one file at a time.

## Decisions for the review

Taken on 2026-09-14 by the maintainer's instruction to proceed without waiting: all three as recommended.

1. **`Session::tools_dir`.** **Recommended:** one field set by `resolve_sources`, read by `render`'s tool loading, `payload::resolve_source`, `payload::describe_source`, and `cli::sync::is_stale`, as `TOOL_RESOLVER_USER_DIR` is one global in Bash. Alternative: thread the path through every function signature.
2. **Scan order.** `refuse_escaping_source_links` reports the first escaping link `find` meets. **Recommended:** walk each root depth-first in `read_dir` order, which is `find`'s order on the same filesystem; `.ai/` entries go non-dot then dot, each byte-sorted, as the two globs order them. Alternative: sort the links, which changes which of several escaping links is named.
3. **The tools dir is normalised.** Bash joins `$REPO_ROOT/$raw` without normalising. **Recommended:** `Paths::absolute`, so an in-memory lookup of `./tools` finds the seeded files; the path is never printed, so output does not change.

## Module closure

```text
lib/helpers/paths.sh              8-104     _external_source_trusted, explicit_source_root_r, register_explicit_source_roots
                                  303-375   is_path_safe_source with explicit roots, resolve_source_path_r
                                  380-446   _source_link_target_r, refuse_escaping_source_links
lib/helpers/shared.sh             86-163    build_source_overlay_tree, _overlay_mirror_source, _overlay_fill_parent
                                  273-330   shared_setup_overlay and base_src_setup_overlay use it
lib/helpers/tool_resolver.sh      27-45     tool_resolver_init_user_dir
                                  445-462   describe_payload_source reads the override dir
lib/sync.sh                       802-818   _refuse_escaping_source_links_or_exit
                                  854-896   _resolve_sources: register, init tools dir, then AGENTS.md
                                  909-938   _sync_is_stale with TOOL_RESOLVER_USER_DIR
                                  1331-1333 main: configless refusal, links, pin
```

---

### Task 0: Baseline

**Files:** none changed.

- [x] **Step 1: Record the baseline**

```bash
git log --oneline -1
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/source_overrides.bats | grep '^not ok'
for f in shared list sync check; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: the family 2 close commit; `171 passed` and `11 passed`; 18 native failures in `source_overrides.bats`, recorded verbatim in the Run log; `0` for `shared`, `list`, `sync`, and `check`.

---

### Task 1: Trusted Roots, Explicit Roots, and the Symlink Scan in `Paths`

**Files:**
- Modify: `src/paths.rs` (`Paths` fields, `new`, `canonicalize_with_existing_ancestor`, `is_safe_source`; new `ExplicitSource`, `trust_external_roots`, `is_trusted_external`, `classify_explicit_source`, `register_explicit_roots`, `escaping_source_link`, private `disk_canonical`, `link_target`, `collect_links`; tests)

**Interfaces:**
- Produces:
  - `pub enum ExplicitSource { Inside, Outside(String), Refused(String), Untrusted(String) }` (`Debug`, `PartialEq`, `Eq`)
  - `pub fn trust_external_roots(&mut self, raw: Option<&str>)`
  - `pub fn is_trusted_external(&self, canonical: &str) -> bool`
  - `pub fn classify_explicit_source(&self, raw: &str) -> ExplicitSource`
  - `pub fn register_explicit_roots(&mut self, roots: Vec<String>)`
  - `pub fn escaping_source_link(&self, roots: &[String]) -> Result<(), String>` — `Err` is the log message without its tag

- [x] **Step 1: Write the failing tests**

Append inside `mod tests` in `src/paths.rs`:

```rust
    #[cfg(unix)]
    fn disk() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let root = format!("{base}/proj");
        std::fs::create_dir_all(format!("{root}/.ai/src/rules")).unwrap();
        std::fs::create_dir_all(format!("{base}/outside/rules")).unwrap();
        std::fs::write(format!("{base}/outside/rules/o.md"), "o\n").unwrap();
        (dir, base, root)
    }

    #[cfg(unix)]
    #[test]
    fn explicit_sources_are_inside_outside_refused_or_untrusted() {
        let (_dir, base, root) = disk();
        let mut p = Paths::new(&root, &root, Some(&base));
        assert_eq!(p.classify_explicit_source(".ai/src/rules"), ExplicitSource::Inside);
        let outside = format!("{base}/outside/rules");
        assert_eq!(
            p.classify_explicit_source(&outside),
            ExplicitSource::Untrusted(outside.clone())
        );
        assert_eq!(
            p.classify_explicit_source("../outside/rules"),
            ExplicitSource::Untrusted(outside.clone())
        );
        p.trust_external_roots(Some(&format!("relative:/nonexistent:{base}/outside")));
        assert_eq!(
            p.classify_explicit_source(&outside),
            ExplicitSource::Outside(outside.clone())
        );
        assert_eq!(p.classify_explicit_source("/"), ExplicitSource::Refused("/".into()));
        assert_eq!(p.classify_explicit_source(".."), ExplicitSource::Refused(base.clone()));
        assert_eq!(p.classify_explicit_source(&base), ExplicitSource::Refused(base.clone()));

        assert!(!p.is_safe_source(&format!("{outside}/o.md")));
        p.register_explicit_roots(vec![outside.clone()]);
        assert!(p.is_safe_source(&format!("{outside}/o.md")));
    }

    #[cfg(unix)]
    #[test]
    fn a_source_link_escaping_the_project_is_named_unless_trusted() {
        use std::os::unix::fs::symlink;
        let (_dir, base, root) = disk();
        let mut p = Paths::new(&root, &root, None);
        std::fs::create_dir_all(format!("{root}/docs")).unwrap();
        std::fs::write(format!("{root}/docs/shared.md"), "s\n").unwrap();
        symlink(format!("{root}/docs/shared.md"), format!("{root}/.ai/src/rules/shared.md")).unwrap();
        let roots = vec![format!("{root}/.ai/src")];
        assert_eq!(p.escaping_source_link(&roots), Ok(()));

        symlink(format!("{base}/outside/rules/o.md"), format!("{root}/.ai/src/rules/leak.md")).unwrap();
        assert_eq!(
            p.escaping_source_link(&roots),
            Err(format!(
                "Source symlink .ai/src/rules/leak.md resolves outside the project: {base}/outside/rules/o.md; add that directory (or a parent) to AGENTSYNC_EXTERNAL_SOURCE_ROOTS to read it"
            ))
        );
        p.trust_external_roots(Some(&format!("{base}/outside")));
        assert_eq!(p.escaping_source_link(&roots), Ok(()));

        let mut p = Paths::new(&root, &root, None);
        std::fs::remove_file(format!("{root}/.ai/src/rules/leak.md")).unwrap();
        std::fs::create_dir_all(format!("{root}/vendor/skill")).unwrap();
        symlink(format!("{base}/outside/rules/o.md"), format!("{root}/vendor/skill/leak.md")).unwrap();
        symlink(format!("{root}/vendor/skill"), format!("{root}/.ai/src/skill")).unwrap();
        let err = p.escaping_source_link(&roots).unwrap_err();
        assert!(err.starts_with("Source symlink vendor/skill/leak.md resolves outside the project: "));
        p.register_explicit_roots(Vec::new());
    }
```

- [x] **Step 2: Run the tests, confirm they fail**

Run: `cargo test paths::tests 2>&1 | grep -E '^error\[' | sort | uniq -c`
Expected: errors for `ExplicitSource` and the four new methods.

- [x] **Step 3: Write the implementation**

Give `Paths` two fields and initialise them in `new`:

```rust
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: String,
    pub root_canonical: String,
    home: Option<String>,
    lexical_below_root: bool,
    external: Vec<String>,
    explicit: Vec<String>,
}
```

```rust
            lexical_below_root: true,
            external: Vec::new(),
            explicit: Vec::new(),
```

Move the disk branch of `canonicalize_with_existing_ancestor` (from `let mut ancestor = abs.to_string();` to its final `Some(normalize(…))`) into `fn disk_canonical(abs: &str) -> Option<String>` at module level, and end `canonicalize_with_existing_ancestor` with `disk_canonical(abs)`.

Extend `is_safe_source`:

```rust
    /// `is_path_safe_source`: the project, the engine, the overlay trees, and
    /// the explicit roots a config registered.
    pub fn is_safe_source(&self, canonical: &str) -> bool {
        is_within(canonical, &self.root_canonical)
            || is_virtual(canonical)
            || self.explicit.iter().any(|root| is_within(canonical, root))
    }
```

Add after `is_safe_source`:

```rust
    /// The directories `AGENTSYNC_EXTERNAL_SOURCE_ROOTS` lists, colon-separated;
    /// a relative or missing entry trusts nothing.
    pub fn trust_external_roots(&mut self, raw: Option<&str>) {
        self.external = raw
            .unwrap_or_default()
            .split(':')
            .filter(|entry| entry.starts_with('/'))
            .filter_map(canonical_dir)
            .collect();
    }

    /// `_external_source_trusted`.
    pub fn is_trusted_external(&self, canonical: &str) -> bool {
        self.external.iter().any(|root| is_within(canonical, root))
    }

    /// `explicit_source_root_r` for a `source.*` value.
    pub fn classify_explicit_source(&self, raw: &str) -> ExplicitSource {
        let abs = self.absolute(raw);
        if abs.starts_with(&format!("{}/", self.root)) {
            return ExplicitSource::Inside;
        }
        let Some(canonical) = self.canonicalize_with_existing_ancestor(&abs) else {
            return ExplicitSource::Inside;
        };
        if canonical.starts_with(&format!("{}/", self.root_canonical)) {
            return ExplicitSource::Inside;
        }
        let home = self.home.as_deref().and_then(canonical_dir);
        if canonical == "/"
            || home.as_deref() == Some(canonical.as_str())
            || canonical == self.root_canonical
            || self.root_canonical.starts_with(&format!("{canonical}/"))
        {
            return ExplicitSource::Refused(canonical);
        }
        if !self.is_trusted_external(&canonical) {
            return ExplicitSource::Untrusted(canonical);
        }
        ExplicitSource::Outside(canonical)
    }

    /// `EXPLICIT_SOURCE_ROOTS`: canonical roots `is_safe_source` admits.
    pub fn register_explicit_roots(&mut self, roots: Vec<String>) {
        self.explicit = roots;
    }

    /// `refuse_escaping_source_links`: every symlink at or below `roots`, and
    /// below each safe directory a link reaches, must resolve to a safe source
    /// or a trusted external root. `Err` is the message to log.
    pub fn escaping_source_link(&self, roots: &[String]) -> Result<(), String> {
        let mut pending: Vec<String> = roots.to_vec();
        let mut visited: Vec<String> = Vec::new();
        let mut index = 0usize;
        while index < pending.len() {
            let root = pending[index].clone();
            index += 1;
            if root.is_empty() {
                continue;
            }
            if index > 256 {
                return Err(
                    "Too many nested symlinks under the source directories to check them safely"
                        .to_string(),
                );
            }
            let mut links = Vec::new();
            if is_symlink(&root) {
                links.push(root);
            } else if Path::new(&root).is_dir() {
                collect_links(&root, &mut links);
            }
            for link in links {
                let shown = link
                    .strip_prefix(&format!("{}/", self.root))
                    .unwrap_or(&link)
                    .to_string();
                let Some(target) = link_target(&link) else {
                    return Err(format!("Cannot resolve source symlink: {shown}"));
                };
                if !self.is_safe_source(&target) && !self.is_trusted_external(&target) {
                    return Err(format!(
                        "Source symlink {shown} resolves outside the project: {target}; add that directory (or a parent) to AGENTSYNC_EXTERNAL_SOURCE_ROOTS to read it"
                    ));
                }
                if Path::new(&target).is_dir() && !visited.contains(&target) {
                    visited.push(target.clone());
                    pending.push(target);
                }
            }
        }
        Ok(())
    }
```

Add at module level:

```rust
/// What `explicit_source_root_r` returns: 0, 1, 2, and 3, with the canonical root.
#[derive(Debug, PartialEq, Eq)]
pub enum ExplicitSource {
    Inside,
    Outside(String),
    Refused(String),
    Untrusted(String),
}

fn is_symlink(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// `_source_link_target_r`: follow a chain of at most 40 links, a relative
/// target resolving from the link's physical directory.
fn link_target(link: &str) -> Option<String> {
    let mut path = link.to_string();
    let mut hops = 0;
    while is_symlink(&path) {
        if hops >= 40 {
            return None;
        }
        hops += 1;
        let target = std::fs::read_link(&path).ok()?;
        let target = target.to_string_lossy().into_owned();
        path = if target.starts_with('/') {
            target
        } else {
            format!("{}/{target}", canonical_dir(&parent(&path))?)
        };
    }
    disk_canonical(&path)
}

/// `find <dir> -type l`: links in walk order, directories entered without
/// following links.
fn collect_links(dir: &str, links: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = format!("{dir}/{}", entry.file_name().to_string_lossy());
        match entry.file_type() {
            Ok(kind) if kind.is_symlink() => links.push(path),
            Ok(kind) if kind.is_dir() => collect_links(&path, links),
            _ => {}
        }
    }
}
```

- [x] **Step 4: Run the tests, confirm green**

Run: `cargo test 2>&1 | grep 'test result' | head -3`
Expected: `174 passed` and `11 passed`.

- [x] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/paths.rs docs/plans/2026-09-14-rust-migration-phase-3b-outside-sources.md
git commit -m "feat(native): classify outside source roots and scan source links"
```

---

### Task 2: `sync` and `check` Register Roots, Read `source.tools`, and Refuse Escaping Links

**Files:**
- Modify: `src/session.rs` (`tools_dir`)
- Modify: `src/render.rs` (`Env`, `prepare`, `resolve_sources`, `render`, `user_tool_slugs`, `load_tool`, new `refuse_escaping_source_links`; tests)
- Modify: `src/payload.rs` (`resolve_source`, `describe_source`; tests)
- Modify: `src/cli/sync.rs` (`sync`, `is_stale`)
- Modify: `src/main.rs` (`sync_env`, `Command::Check`)

**Interfaces:**
- Consumes: Task 1's `Paths` methods.
- Produces:
  - `Session { …, pub tools_dir: String }`, `format!("{root}/.ai/src/tools")` until `resolve_sources` sets it
  - `render::Env { …, pub external_source_roots: Option<String> }`
  - `pub fn refuse_escaping_source_links(s: &mut Session, run: &Run) -> Step`
  - `payload::describe_source(tools_dir: &str, root: &str, path: &str, slug: &str, resource: &str) -> &'static str`

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src/render.rs`:

```rust
    #[test]
    fn source_tools_moves_the_tool_overrides_and_an_untrusted_root_stops() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "source:\n  tools: \"catalog\"\n",
        );
        file(&mut s, "/proj/catalog/claude.yaml", "enabled: true\n");
        assert_eq!(render(&mut s, &Env::default()), Ok(()));
        assert_eq!(s.tools_dir, "/proj/catalog");
        assert!(s.ws.is_file("/proj/CLAUDE.md"));

        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\nsource:\n  rules: \"/\"\n",
        );
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            ["[ERROR] source.rules must not be the filesystem root, the home directory, or the project root or its ancestor: / -> /"]
        );
    }
```

In `src/payload.rs` tests, change the `describe_source("/proj", …)` calls to `describe_source("/proj/.ai/src/tools", "/proj", …)`.

- [ ] **Step 2: Run the tests, confirm they fail**

Run: `cargo test render::tests::source_tools 2>&1 | grep -E '^error\[' | sort | uniq -c`
Expected: `no field `tools_dir` on type `Session`` and the four-argument `describe_source` mismatch.

- [ ] **Step 3: Write the implementation**

`src/session.rs`: add `pub tools_dir: String,` to `Session` and, in `new`, `tools_dir: format!("{}/.ai/src/tools", paths.root),` before `paths,` (move `paths` into the struct last).

`src/render.rs`:

- `Env` gains `pub external_source_roots: Option<String>,` documented as `AGENTSYNC_EXTERNAL_SOURCE_ROOTS`.
- `prepare` calls `resolve_sources(s, env, &mut run)?;` and `resolve_sources` takes `env: &Env`.
- In `resolve_sources`, after the `if let Some(text) = &run.config { … }` override loop and before `let agents_abs = …`:

```rust
    s.paths
        .trust_external_roots(env.external_source_roots.as_deref());
    let mut explicit = Vec::new();
    if let Some(text) = &run.config {
        for key in ["agents", "rules", "skills", "tools", "commands", "subagents"] {
            let raw = yaml_subset::value(text, &format!("source.{key}"));
            if raw.is_empty() {
                continue;
            }
            match s.paths.classify_explicit_source(&raw) {
                paths::ExplicitSource::Inside => {}
                paths::ExplicitSource::Outside(canonical) => explicit.push(canonical),
                paths::ExplicitSource::Refused(canonical) => {
                    s.log.error(&format!(
                        "source.{key} must not be the filesystem root, the home directory, or the project root or its ancestor: {raw} -> {canonical}"
                    ));
                    return Err(Stop(1));
                }
                paths::ExplicitSource::Untrusted(canonical) => {
                    s.log.error(&format!(
                        "source.{key} points outside the project at {canonical}, which AGENTSYNC_EXTERNAL_SOURCE_ROOTS does not list; add that directory (or a parent) to the variable to read from it"
                    ));
                    return Err(Stop(1));
                }
            }
        }
    }
    s.paths.register_explicit_roots(explicit);
    let configured_tools = run
        .config
        .as_deref()
        .map(|text| yaml_subset::value(text, "source.tools"))
        .unwrap_or_default();
    s.tools_dir = if configured_tools.is_empty() {
        format!("{root}/.ai/src/tools")
    } else {
        s.paths.absolute(&configured_tools)
    };
```

- `render` runs `refuse_escaping_source_links(s, &run)?;` between `refuse_configless_cleanup` and `check_version_pin`.
- `user_tool_slugs` and `load_tool` use `s.tools_dir` in place of `format!("{}/.ai/src/tools", s.paths.root)`.
- New step after `refuse_configless_cleanup`:

```rust
/// `_refuse_escaping_source_links_or_exit`: `.ai/` without its backups, then
/// the resolved sources, then the tool overrides.
pub fn refuse_escaping_source_links(s: &mut Session, run: &Run) -> Step {
    let root = s.paths.root.clone();
    let ai = format!("{root}/.ai");
    let mut names: Vec<String> = std::fs::read_dir(&ai)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    let (dotted, plain): (Vec<String>, Vec<String>) =
        names.into_iter().partition(|name| name.starts_with('.'));
    let mut roots: Vec<String> = plain
        .into_iter()
        .chain(dotted.into_iter().filter(|name| !name.starts_with("..")))
        .filter(|name| name != "backups")
        .map(|name| format!("{ai}/{name}"))
        .collect();
    let sources = &run.sources;
    for raw in [
        &sources.agents,
        &sources.rules,
        &sources.skills,
        &sources.commands,
        &sources.subagents,
    ] {
        if !raw.is_empty() {
            roots.push(s.paths.absolute(raw));
        }
    }
    roots.push(s.tools_dir.clone());
    if let Err(message) = s.paths.escaping_source_link(&roots) {
        s.log.error(&message);
        return Err(Stop(1));
    }
    Ok(())
}
```

- The two `payload::describe_source(&root, mcp, &tool.slug, "mcp")` calls become `payload::describe_source(&s.tools_dir, &root, mcp, &tool.slug, "mcp")` (clone `s.tools_dir` first where `s` is borrowed mutably).

`src/payload.rs`:

```rust
    let override_dir = format!("{}/{}", s.tools_dir, tool.slug);
```

```rust
/// `describe_payload_source`.
pub fn describe_source(tools_dir: &str, root: &str, path: &str, slug: &str, resource: &str) -> &'static str {
    if path.is_empty() {
        return "";
    }
    if path.starts_with(&format!("{tools_dir}/{slug}/")) {
        return "override";
    }
```

`src/cli/sync.rs`: `sync` runs `render::refuse_escaping_source_links(s, &run)?;` after `render::refuse_configless_cleanup(s, &run)?;`, and calls `is_stale(&s.paths.root, &run, &s.tools_dir)`; `is_stale` takes `tools_dir: &str` and adds it to the loop over `sources` (`[…, &sources.subagents, &tools_dir.to_string()]`).

`src/main.rs`: `sync_env`'s `render: Env { … }` and `Command::Check`'s `Env { … }` gain `external_source_roots: var("AGENTSYNC_EXTERNAL_SOURCE_ROOTS"),`.

- [ ] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/source_overrides.bats | grep '^not ok'
for f in sync check shared; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `175 passed` and `11 passed`; the remaining `source_overrides.bats` failures are those that need the overlays (Task 3) and `list` (Task 4), recorded in the Run log; `0` for `sync`, `check`, and `shared`.

- [ ] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/session.rs src/render.rs src/payload.rs src/cli/sync.rs src/main.rs docs/plans/2026-09-14-rust-migration-phase-3b-outside-sources.md
git commit -m "feat(native): trust outside sources and refuse escaping source links"
```

---

### Task 3: Overlays From the Resolved Sources

**Files:**
- Modify: `src/overlay.rs` (`build_tree` split; new `build_source_tree`, `mirror_source`, `fill_parent`; `setup_shared`, `setup_base_src`; test)

**Interfaces:**
- Produces: `pub fn build_source_tree(s: &mut Session, name: &str, sources: &Sources, parent_src: &str, categories: &[&str]) -> Result<String, Error>`.

- [ ] **Step 1: Write the failing test**

Append inside the tests module of `src/overlay.rs` (create `#[cfg(test)] mod tests { use super::*; use crate::session::test_session; use crate::workspace::Content; }` if the module has none):

```rust
    #[test]
    fn the_engine_skill_layer_keeps_configured_sources() {
        let mut s = test_session();
        s.ws.insert_file("/proj/.ai/src/AGENTS.md", Content::Bytes(b"# Project\n".to_vec()));
        s.ws.insert_file("/proj/shared-rules/r.md", Content::Bytes(b"r\n".to_vec()));
        let mut sources = Sources {
            agents: ".ai/src/AGENTS.md".into(),
            rules: "shared-rules".into(),
            ..Sources::default()
        };
        setup_base_src(&mut s, None, "/proj/.ai/src", &mut sources).unwrap();
        assert_eq!(sources.rules, "/<agentsync-overlay>/base-src/src/rules");
        assert!(s.ws.is_file("/<agentsync-overlay>/base-src/src/rules/r.md"));
    }
```

- [ ] **Step 2: Run it, confirm it fails**

Run: `cargo test overlay:: 2>&1 | grep -E '^test .*(FAILED|ok)$|^error'`
Expected: `the_engine_skill_layer_keeps_configured_sources ... FAILED` (the overlay mirrored `.ai/src`, which has no `rules/`).

- [ ] **Step 3: Write the implementation**

Split the parent fill out of `build_tree` and add the source-driven builder:

```rust
/// `_overlay_fill_parent`: parent files of the inherited categories the tree
/// lacks, except categories in `skipped`.
fn fill_parent(
    ws: &mut Workspace,
    src: &str,
    parent_src: &str,
    categories: &[&str],
    skipped: &[&str],
) -> Result<(), Error> {
    for category in categories {
        if skipped.contains(category) {
            continue;
        }
        let parent_dir = format!("{parent_src}/{category}");
        if !ws.is_dir(&parent_dir) {
            continue;
        }
        for file in ws.files_under(&parent_dir) {
            let rel = &file[parent_dir.len() + 1..];
            let target = format!("{src}/{category}/{rel}");
            if ws.exists(&target) {
                continue;
            }
            ws.create_dir_all(&paths::parent(&target))?;
            ws.copy(&file, &target)?;
        }
    }
    Ok(())
}

/// `build_source_overlay_tree`: mirror each resolved source, then fill the
/// inherited categories from the parent. A category whose source resolves
/// outside the safe roots is neither mirrored nor filled.
pub fn build_source_tree(
    s: &mut Session,
    name: &str,
    sources: &Sources,
    parent_src: &str,
    categories: &[&str],
) -> Result<String, Error> {
    let dir = format!("{OVERLAY_ROOT}/{name}");
    s.ws.remove(&dir)?;
    let src = format!("{dir}/src");
    s.ws.create_dir_all(&src)?;
    mirror_source(s, true, &sources.agents, &format!("{src}/AGENTS.md"))?;
    let mut refused = Vec::new();
    for (category, raw) in [
        ("rules", &sources.rules),
        ("skills", &sources.skills),
        ("commands", &sources.commands),
        ("agents", &sources.subagents),
    ] {
        if !mirror_source(s, false, raw, &format!("{src}/{category}"))? {
            refused.push(category);
        }
    }
    fill_parent(&mut s.ws, &src, parent_src, categories, &refused)?;
    Ok(dir)
}

/// `_overlay_mirror_source`: `false`, copying nothing, when the source
/// resolves outside the safe roots.
fn mirror_source(s: &mut Session, file: bool, raw: &str, dest: &str) -> Result<bool, Error> {
    if raw.is_empty() {
        return Ok(true);
    }
    let abs = s.paths.absolute(raw);
    let present = if file {
        s.ws.is_file(&abs)
    } else {
        s.ws.is_dir(&abs)
    };
    if !present {
        return Ok(true);
    }
    let Some(canonical) = s.paths.canonicalize_with_existing_ancestor(&abs) else {
        return Ok(false);
    };
    if !s.paths.is_safe_source(&canonical) {
        return Ok(false);
    }
    s.ws.copy(&abs, dest)?;
    Ok(true)
}
```

`build_tree` keeps its mirror loop and ends with `fill_parent(ws, &src, parent_src, categories, &[])?; Ok(dir)`. In `setup_shared`, replace `build_tree(&mut s.ws, "shared", &child_src, &parent_src, &categories)?` with `build_source_tree(s, "shared", &sources.clone(), &parent_src, &categories)?` and drop `child_src`; in `setup_base_src`, replace the `build_tree` call with `build_source_tree(s, "base-src", &sources.clone(), &base_src, &["skills"])?`.

- [ ] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
AGENTSYNC_NATIVE=1 bats --tap tests/source_overrides.bats | grep '^not ok'
for f in shared base_skills profiles sync check; do
    printf '%s native=%s\n' "$f" "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `176 passed` and `11 passed`; only the `list`-driven failures remain in `source_overrides.bats`; `0` elsewhere.

- [ ] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/overlay.rs docs/plans/2026-09-14-rust-migration-phase-3b-outside-sources.md
git commit -m "feat(native): build source overlays from the resolved sources"
```

---

### Task 4: `list` Reads `source.tools`

**Files:**
- Modify: `src/project.rs` (`Project`, `select`, `user_tools_dir`; test)

**Interfaces:**
- Produces: `Project { pub root, pub config_path, tools_dir: PathBuf }`; `user_tools_dir()` returns the configured directory.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn source_tools_moves_the_override_directory() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".ai/agent_sync.yaml", "source:\n  tools: \"catalog\"\n");
        write(dir.path(), "catalog/zed.yaml", "enabled: true\n");
        let project = Project::at(dir.path()).unwrap();
        assert_eq!(project.user_tools_dir(), dir.path().join("catalog"));
        assert_eq!(project.legacy_enabled_tools().unwrap(), ["zed"]);
    }
```

- [ ] **Step 2: Run it, confirm it fails**

Run: `cargo test project::tests::source_tools 2>&1 | grep -E '^test |panicked'`
Expected: `FAILED`, the directory being `.ai/src/tools`.

- [ ] **Step 3: Write the implementation**

Add `tools_dir: PathBuf` to `Project`; in `select`, after `config_path` is known:

```rust
        let configured = match &config_path {
            Some(path) => {
                let text = std::fs::read(path).map_err(|e| Error::io(path, e))?;
                yaml_subset::value(&String::from_utf8_lossy(&text), "source.tools")
            }
            None => String::new(),
        };
        let tools_dir = if configured.is_empty() {
            root.join(".ai").join("src").join("tools")
        } else if configured.starts_with('/') {
            PathBuf::from(configured)
        } else {
            root.join(configured)
        };
        Ok(Self { root, config_path, tools_dir })
```

and `user_tools_dir` returns `self.tools_dir.clone()`.

- [ ] **Step 4: Run the tests, confirm green**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo build --release
printf 'source_overrides native=%s\n' "$(AGENTSYNC_NATIVE=1 bats --tap tests/source_overrides.bats | grep -c '^not ok')"
printf 'list native=%s\n' "$(AGENTSYNC_NATIVE=1 bats --tap tests/list.bats | grep -c '^not ok')"
```

Expected: `177 passed` and `11 passed`; `source_overrides native=0`; `list native=0`.

- [ ] **Step 5: Lint and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
git add src/project.rs docs/plans/2026-09-14-rust-migration-phase-3b-outside-sources.md
git commit -m "feat(native): read tool overrides from source.tools in list"
```

---

### Task 5: Parity Fixtures and Module Map

**Files:**
- Modify: `tests/native_parity.bats`, `.ai/src/skills/native-port/references/module-map.md`, `.ai/.sync-manifest`

- [ ] **Step 1: Write the fixture**

After `parity: backup.retention in sync and rollback`:

```bash
@test "parity: sources outside the project and escaping source links" {
    enable_tools claude
    local outside="$BATS_TEST_TMPDIR/outside"
    mkdir -p "$outside/rules" "$outside/tools/claude"
    printf '# Outside\n' > "$outside/rules/outside.md"
    printf '{"outside":true}\n' > "$outside/tools/claude/settings.json"
    printf '\nsource:\n  rules: "%s/rules"\n  tools: "%s/tools"\n' "$outside" "$outside" >> .ai/agent_sync.yaml
    assert_tree_parity sync
    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$outside" assert_tree_parity sync
    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$outside" _bash_sync
    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$outside" assert_parity check
    AGENTSYNC_EXTERNAL_SOURCE_ROOTS="$outside" assert_parity list
    grep -v '^  rules:\|^  tools:\|^source:' .ai/agent_sync.yaml > .ai/agent_sync.yaml.tmp
    mv .ai/agent_sync.yaml.tmp .ai/agent_sync.yaml
    create_test_symlink "$outside/rules/outside.md" .ai/src/rules/leak.md
    assert_tree_parity sync
    assert_parity check
}
```

- [ ] **Step 2: Run it and prove it bites**

```bash
cargo build --release
bats --tap -f 'sources outside' tests/native_parity.bats
```

Expected: `ok`. Then change `"; add that directory (or a parent) to AGENTSYNC_EXTERNAL_SOURCE_ROOTS to read it"` to end in `it!"` in `src/paths.rs`, rebuild, rerun: `not ok` with that diff; revert and rebuild.

- [ ] **Step 3: Module map and outputs**

In `.ai/src/skills/native-port/references/module-map.md`, extend the `lib/helpers/paths.sh` row with `; explicit source roots trusted through AGENTSYNC_EXTERNAL_SOURCE_ROOTS, escaping source-link scan`, the `lib/helpers/shared.sh` row with `; overlays mirror the resolved sources`, and the `tool_resolver.sh` row with `; source.tools as Session::tools_dir`. Regenerate outputs with `AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$PWD" bash bin/agentsync.sh sync --force`.

- [ ] **Step 4: Verify the family**

```bash
cargo test 2>&1 | grep 'test result' | head -3
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
shellcheck -x -S warning -e SC1091 bin/agentsync.sh install.sh lib/sync.sh lib/check.sh lib/setup_hooks.sh lib/helpers/*.sh
for f in source_overrides shared base_skills profiles list sync check config_safety backup_retention native_parity; do
    printf '%s bash=%s native=%s\n' "$f" \
        "$(AGENTSYNC_NATIVE=0 bats --tap "tests/$f.bats" | grep -c '^not ok')" \
        "$(AGENTSYNC_NATIVE=1 bats --tap "tests/$f.bats" | grep -c '^not ok')"
done
```

Expected: `177 passed` and `11 passed`; lint exit 0; every line `bash=0 native=0` except `native_parity bash=1 native=1` (family 4).

- [ ] **Step 5: Commit**

```bash
git add tests/native_parity.bats .ai/src/skills/native-port/references/module-map.md .ai/.sync-manifest docs/plans/2026-09-14-rust-migration-phase-3b-outside-sources.md
git commit -m "test(native): diff Bash against native outside sources and links"
```

---

## Completion

The family is closed when every box is ticked, `source_overrides.bats` is green under `AGENTSYNC_NATIVE=1`, the parity fixture passes in both modes, and a `## Completion receipt` records the fresh verification. The next family is the rollback witness.

## Run log

### 2026-09-14 — Phase 3b family 3 planned
- Commits: this plan.
- Verified: plan written against `lib/sync.sh` 802-938 and 1331-1333, `lib/helpers/paths.sh`, `lib/helpers/shared.sh`, `lib/helpers/tool_resolver.sh` on the migration branch, and `src/paths.rs`, `src/overlay.rs`, `src/render.rs`, `src/payload.rs`, `src/workspace.rs` (paths outside the root are read from disk in memory too).
- Plan amended: none.
- Next: Task 0 Step 1.
- Blocker: none.

### 2026-09-14 — Tasks 0 and 1 done
- Commits: the Task 1 commit, "feat(native): classify outside source roots and scan source links".
- Verified: Task 0 at `1cfb31c`: `source_overrides` native failures 18 (1, 2, 3, 5, 6, 9, 11, 12, 13, 14, 15, 17, 18, 19, 20, 24, 25, 26), `shared`, `list`, `sync`, `check` 0. Task 1: `cargo test` 174 and 11 passed; fmt and clippy exit 0.
- Plan amended: `trust_external_roots` keeps only directory entries, because `_canon_dir_r` is `cd -P`, which refuses a file; a third test `only_a_directory_entry_is_trusted` pins it, and the link test adds a relative escaping link and a self-looping link, so every later count is one higher. Task 2's untrusted-root message ends in "to read from it", as `register_explicit_source_roots` prints it.
- Next: Task 2 Step 1.
- Blocker: none.
