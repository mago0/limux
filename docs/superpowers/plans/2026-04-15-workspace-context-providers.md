# Workspace Context Providers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a pluggable `workspace_context` framework inside `limux-host-linux` plus a single `git.branch` provider that shows a live branch (or detached short SHA) badge beneath each workspace's sidebar path.

**Architecture:** A sync-only `WorkspaceContextProvider` trait lives in a new `workspace_context/` module. A `ProviderRegistry` is built at startup from `AppConfig.sidebar.providers`. Each `Workspace` owns a `badges_row: gtk::Box` containing one `gtk::Label` per enabled provider, plus the associated `WatcherHandle`s. Providers evaluate on workspace creation, on OSC-7 cwd change, and whenever a `gio::FileMonitor` fires. Everything runs on the GTK main thread.

**Tech Stack:** Rust 1.x (workspace edition), `gtk4` 0.11, `libadwaita` 0.9, `gtk::gio::FileMonitor`, `serde_json` (already vendored), `tempfile` (dev-only). No new Cargo dependencies.

**Reference spec:** `docs/superpowers/specs/2026-04-15-workspace-context-providers-design.md`. Follow that document for all behavioural decisions; this plan is the execution order.

**Quality gate for every task:** every task ends with `./scripts/check.sh` (fmt + clippy -D warnings + workspace tests). Treat clippy findings as required.

## Execution status

Branch: `feat/workspace-context-providers` (off `main`).

Always run cargo through the nix shell: `nix-shell --run '...'`. See `shell.nix` at the repo root. Rustup is pinned to stable 1.94 via `rustup override set 1.94` (per-project, not committed). `libghostty.so` must exist at `ghostty/zig-out/lib/libghostty.so` - if missing, run `nix-shell --run 'cd ghostty && zig build -Dapp-runtime=none -Doptimize=ReleaseFast'` once.

Completed:
- [x] Task 1 - `SidebarConfig` added to `AppConfig`. Commit `cebc21a`.
- [x] Task 2 - `workspace_context` module scaffolded (trait, registry, types, git_branch stub). Commit `699a3d1`.
- [x] Infra - `shell.nix` committed. Commit `1f807a8`.

Pending: Tasks 3-12 below. Resume by dispatching an implementer subagent for Task 3 per `superpowers:subagent-driven-development`.

Known issue out-of-scope for this PR: `cargo clippy -D warnings` fails under rustc 1.95 (current CI stable) with 4 new `collapsible_match` lints in `limux-core`. Pinning local dev to 1.94 sidesteps it; a separate tiny PR should fix the lints.

---

## File Structure

Files created or modified by this plan:

- Modify: `rust/limux-host-linux/src/app_config.rs` - add `SidebarConfig`, extend `AppConfig`, extend `parse_app_config_value`, add tests.
- Create: `rust/limux-host-linux/src/workspace_context/mod.rs` - trait, `WorkspaceContext`, `ContextLine`, `WatcherHandle`, `ProviderRegistry`.
- Create: `rust/limux-host-linux/src/workspace_context/git_branch.rs` - `HeadState`, `detect_head`, `GitBranchProvider`, tests.
- Modify: `rust/limux-host-linux/src/main.rs` - `mod workspace_context;` declaration (alongside existing modules).
- Modify: `rust/limux-host-linux/src/window.rs` - replace `build_sidebar_row` tuple return with `SidebarRowWidgets`, extend `Workspace` struct, wire registry into creation/restore/cwd-change paths, add CSS.

No changes to `limux-core`, `limux-protocol`, `limux-control`, or the control bridge. The design explicitly forbids reverse dependencies.

## Working conventions

- Do NOT use em dashes or en dashes in any file you write. Only hyphens. (Global rule from `~/.claude/CLAUDE.md`.)
- Keep pure logic separate from GTK wiring. `detect_head` is pure and tested against `tempfile::TempDir`; the `FileMonitor` wrapper is thin.
- Use `eprintln!("limux: ...")` for warnings, matching `app_config.rs:243` and `window.rs:734`. Do not introduce `log` or `tracing`.
- Use `gtk::gio` (re-exported from `gtk4`) for `File` and `FileMonitor`. No new Cargo deps needed.
- Frequent, small commits. One task = one commit unless the task explicitly says otherwise.
- Every task ends with a green `./scripts/check.sh` before committing.

---

## Task 1: Add `SidebarConfig` to `AppConfig` with manual JSON parsing and defaults

**Rationale.** Config is read once at startup and cached. A missing `sidebar` key must default to `["git.branch"]` so upgraders get the feature without editing config. Parsing style must match the existing `focus` / `appearance` sections, which use manual extraction in `parse_app_config_value` rather than serde-derive.

**Files:**
- Modify: `rust/limux-host-linux/src/app_config.rs` (struct at lines 40-47, parser at line 130)

- [ ] **Step 1: Write the failing test for default behaviour**

Append at the end of the `#[cfg(test)] mod tests` block in `rust/limux-host-linux/src/app_config.rs`:

```rust
#[test]
fn load_from_path_defaults_sidebar_providers_when_section_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = settings_path_in(tmp.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, r#"{"focus": {"hover_terminal_focus": false}}"#).unwrap();

    let loaded = load_from_path(&path);

    assert!(loaded.warnings.is_empty(), "warnings: {:?}", loaded.warnings);
    assert_eq!(loaded.config.sidebar.providers, vec!["git.branch".to_string()]);
}
```

- [ ] **Step 2: Run the test, expect a compilation failure**

Run: `cargo test -p limux-host-linux --lib load_from_path_defaults_sidebar_providers_when_section_missing`

Expected: compile error - `sidebar` field does not exist on `AppConfig`.

- [ ] **Step 3: Add `SidebarConfig` and extend `AppConfig`**

In `rust/limux-host-linux/src/app_config.rs`, after the `FocusConfig` block (around line 59), add:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarConfig {
    pub providers: Vec<String>,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            providers: vec!["git.branch".to_string()],
        }
    }
}
```

Modify the `AppConfig` struct (line 40) to add the new field:

```rust
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub focus: FocusConfig,
    #[serde(skip)]
    pub appearance: AppearanceConfig,
    #[serde(skip)]
    pub font_size: Option<f32>,
    #[serde(skip)]
    pub sidebar: SidebarConfig,
}
```

`sidebar` is `#[serde(skip)]` because `parse_app_config_value` handles it manually, matching the pattern used for `appearance` and `font_size`.

- [ ] **Step 4: Wire manual parsing in `parse_app_config_value`**

Inside `parse_app_config_value` in `app_config.rs` (starts at line 130), after the existing extraction of `appearance` and `font_size`, compute the sidebar config and include it in the returned `AppConfig { ... }` literal. Add this block before the final struct construction:

```rust
let sidebar = parse_sidebar_config(root);
```

Then include `sidebar` in the returned struct literal (it will be the next field after `font_size`).

Below `parse_app_config_value`, add:

```rust
fn parse_sidebar_config(root: &Value) -> SidebarConfig {
    let Some(sidebar) = root.get("sidebar").and_then(Value::as_object) else {
        return SidebarConfig::default();
    };

    let Some(providers) = sidebar.get("providers").and_then(Value::as_array) else {
        return SidebarConfig::default();
    };

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(providers.len());
    for value in providers {
        if let Some(id) = value.as_str() {
            let id = id.to_string();
            if seen.insert(id.clone()) {
                out.push(id);
            }
        }
    }
    SidebarConfig { providers: out }
}
```

Note: this preserves the explicit-empty case (`providers: []` means no badges) and de-duplicates (first occurrence wins). Unknown-id filtering and WARN logging live in the registry, not the parser, so the config layer stays pure.

- [ ] **Step 5: Run the default test, expect PASS**

Run: `cargo test -p limux-host-linux --lib load_from_path_defaults_sidebar_providers_when_section_missing`

Expected: PASS.

- [ ] **Step 6: Add and verify remaining parser tests**

Append these tests to the same `mod tests`:

```rust
#[test]
fn load_from_path_explicit_empty_sidebar_providers_is_honored() {
    let tmp = tempfile::tempdir().unwrap();
    let path = settings_path_in(tmp.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, r#"{"sidebar": {"providers": []}}"#).unwrap();

    let loaded = load_from_path(&path);

    assert!(loaded.config.sidebar.providers.is_empty());
}

#[test]
fn load_from_path_dedupes_sidebar_providers_preserving_first_occurrence() {
    let tmp = tempfile::tempdir().unwrap();
    let path = settings_path_in(tmp.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"sidebar": {"providers": ["git.branch", "foo.bar", "git.branch"]}}"#,
    )
    .unwrap();

    let loaded = load_from_path(&path);

    assert_eq!(
        loaded.config.sidebar.providers,
        vec!["git.branch".to_string(), "foo.bar".to_string()]
    );
}

#[test]
fn load_from_path_preserves_sidebar_provider_order() {
    let tmp = tempfile::tempdir().unwrap();
    let path = settings_path_in(tmp.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, r#"{"sidebar": {"providers": ["b", "a"]}}"#).unwrap();

    let loaded = load_from_path(&path);

    assert_eq!(
        loaded.config.sidebar.providers,
        vec!["b".to_string(), "a".to_string()]
    );
}
```

Run: `cargo test -p limux-host-linux --lib sidebar`

Expected: all four sidebar tests pass.

- [ ] **Step 7: Run full quality gate**

Run: `./scripts/check.sh`

Expected: PASS. Fix any clippy findings before committing (e.g., `HashSet::with_capacity` hint).

- [ ] **Step 8: Commit**

```bash
git add rust/limux-host-linux/src/app_config.rs
git commit -m "feat(app-config): add SidebarConfig with git.branch default"
```

---

## Task 2: Scaffold `workspace_context` module with trait, types, and empty registry

**Rationale.** Landing the framework without any provider means subsequent tasks can add `GitBranchProvider` in isolation and a later PR can add a second provider without disturbing these files. This task MUST NOT modify `window.rs`. Wiring happens in Tasks 7-9.

**Files:**
- Create: `rust/limux-host-linux/src/workspace_context/mod.rs`
- Modify: `rust/limux-host-linux/src/main.rs` (add module declaration)

- [ ] **Step 1: Locate the existing module declarations**

Open `rust/limux-host-linux/src/main.rs` and find the block of `mod` declarations (near the top of the file). Expected siblings: `mod app_config;`, `mod window;`, `mod terminal;`, etc.

- [ ] **Step 2: Add the module declaration**

Insert `mod workspace_context;` alongside the others, alphabetised if the file sorts them, otherwise appended at the end of the block.

- [ ] **Step 3: Create the module root with trait and types**

Create `rust/limux-host-linux/src/workspace_context/mod.rs` with this content:

```rust
//! Pluggable contextual information displayed beneath each workspace's
//! sidebar path. See docs/superpowers/specs/2026-04-15-workspace-context-providers-design.md.

#![allow(dead_code)] // Individual submodules consume these types; suppress until wired.

use std::path::PathBuf;
use std::rc::Rc;

pub mod git_branch;

/// Inputs a provider receives when evaluating a workspace.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceContext {
    pub workspace_id: String,
    pub cwd: Option<PathBuf>,
    pub folder_path: Option<PathBuf>,
}

/// Data a provider emits; GTK wiring is done by the sidebar renderer, not here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextLine {
    pub icon: Option<&'static str>,
    pub text: String,
    pub tooltip: Option<String>,
    pub css_class: Option<&'static str>,
}

/// Opaque handle. Dropping it must tear down any installed watchers.
pub trait WatcherHandle {}

/// A single contextual-info source for workspace rows.
pub trait WorkspaceContextProvider: 'static {
    fn id(&self) -> &'static str;

    fn evaluate(&self, ctx: &WorkspaceContext) -> Option<ContextLine>;

    /// Install OS-level watchers that call `notify` when cached output may have
    /// changed. The provider owns cleanup through the returned handle's `Drop`.
    fn install_watchers(
        &self,
        ctx: &WorkspaceContext,
        notify: Rc<dyn Fn()>,
    ) -> Box<dyn WatcherHandle>;
}

/// No-op handle used by providers that watch nothing (or as a test double).
pub struct NullWatcherHandle;
impl WatcherHandle for NullWatcherHandle {}

/// Startup-built collection of enabled providers in config order.
pub struct ProviderRegistry {
    providers: Vec<Box<dyn WorkspaceContextProvider>>,
}

impl ProviderRegistry {
    /// Build a registry from the configured ids. Unknown ids are logged and skipped.
    pub fn from_config(enabled_ids: &[String]) -> Self {
        let mut providers: Vec<Box<dyn WorkspaceContextProvider>> = Vec::new();
        for id in enabled_ids {
            match id.as_str() {
                "git.branch" => providers.push(Box::new(git_branch::GitBranchProvider::new())),
                other => eprintln!("limux: sidebar: ignoring unknown provider id: {other:?}"),
            }
        }
        Self { providers }
    }

    pub fn providers(&self) -> &[Box<dyn WorkspaceContextProvider>] {
        &self.providers
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    struct CountingProvider {
        id: &'static str,
        value: &'static str,
        install_count: Rc<Cell<usize>>,
        drop_count: Rc<Cell<usize>>,
    }

    struct CountingHandle {
        drop_count: Rc<Cell<usize>>,
    }
    impl WatcherHandle for CountingHandle {}
    impl Drop for CountingHandle {
        fn drop(&mut self) {
            self.drop_count.set(self.drop_count.get() + 1);
        }
    }

    impl WorkspaceContextProvider for CountingProvider {
        fn id(&self) -> &'static str {
            self.id
        }

        fn evaluate(&self, _ctx: &WorkspaceContext) -> Option<ContextLine> {
            Some(ContextLine {
                icon: None,
                text: self.value.to_string(),
                tooltip: None,
                css_class: None,
            })
        }

        fn install_watchers(
            &self,
            _ctx: &WorkspaceContext,
            _notify: Rc<dyn Fn()>,
        ) -> Box<dyn WatcherHandle> {
            self.install_count.set(self.install_count.get() + 1);
            Box::new(CountingHandle {
                drop_count: self.drop_count.clone(),
            })
        }
    }

    #[test]
    fn registry_from_config_filters_unknown_ids() {
        let registry = ProviderRegistry::from_config(&[
            "git.branch".to_string(),
            "totally.not.a.provider".to_string(),
        ]);
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.providers()[0].id(), "git.branch");
    }

    #[test]
    fn watcher_handle_drop_tears_down_exactly_once() {
        let install_count = Rc::new(Cell::new(0usize));
        let drop_count = Rc::new(Cell::new(0usize));
        let provider = CountingProvider {
            id: "test.counter",
            value: "hello",
            install_count: install_count.clone(),
            drop_count: drop_count.clone(),
        };

        let ctx = WorkspaceContext::default();
        let handle = provider.install_watchers(&ctx, Rc::new(|| {}));
        assert_eq!(install_count.get(), 1);
        assert_eq!(drop_count.get(), 0);
        drop(handle);
        assert_eq!(drop_count.get(), 1);
    }

    #[test]
    fn evaluate_returns_context_line_from_fake_provider() {
        let install_count = Rc::new(Cell::new(0usize));
        let drop_count = Rc::new(Cell::new(0usize));
        let provider = CountingProvider {
            id: "test.counter",
            value: "hello",
            install_count,
            drop_count,
        };
        let line = provider
            .evaluate(&WorkspaceContext::default())
            .expect("some line");
        assert_eq!(line.text, "hello");
    }
}
```

- [ ] **Step 4: Create a placeholder `git_branch.rs` so the module declaration compiles**

Create `rust/limux-host-linux/src/workspace_context/git_branch.rs` with just enough scaffolding to satisfy the registry stub. Task 3 replaces the body of `detect_head`; Task 4 replaces the body of `evaluate`; Task 5 replaces `install_watchers`.

```rust
//! Git branch / detached HEAD provider. Implementation lands across tasks 3-5.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::{ContextLine, NullWatcherHandle, WatcherHandle, WorkspaceContext, WorkspaceContextProvider};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeadState {
    Branch(String),
    Detached(String),
    None,
}

pub fn detect_head(_start: &Path) -> Option<(HeadState, PathBuf)> {
    None // Implemented in Task 3.
}

pub struct GitBranchProvider;

impl GitBranchProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GitBranchProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceContextProvider for GitBranchProvider {
    fn id(&self) -> &'static str {
        "git.branch"
    }

    fn evaluate(&self, _ctx: &WorkspaceContext) -> Option<ContextLine> {
        None // Implemented in Task 4.
    }

    fn install_watchers(
        &self,
        _ctx: &WorkspaceContext,
        _notify: Rc<dyn Fn()>,
    ) -> Box<dyn WatcherHandle> {
        Box::new(NullWatcherHandle) // Replaced in Task 5.
    }
}
```

- [ ] **Step 5: Run the quality gate**

Run: `./scripts/check.sh`

Expected: all tests pass, clippy clean, fmt clean.

- [ ] **Step 6: Commit**

```bash
git add rust/limux-host-linux/src/main.rs \
        rust/limux-host-linux/src/workspace_context/mod.rs \
        rust/limux-host-linux/src/workspace_context/git_branch.rs
git commit -m "feat(workspace-context): scaffold provider trait, registry, and contract tests"
```

---

## Task 3: Implement `detect_head` pure logic with full test matrix

**Rationale.** This is the only git-reading logic we write by hand. Keeping it pure (no GTK, no file monitor) means it gets 10 unit tests for free. Every scenario is a `tempfile::TempDir` with a hand-crafted `.git/` fixture.

**Files:**
- Modify: `rust/limux-host-linux/src/workspace_context/git_branch.rs`

- [ ] **Step 1: Write all pure-logic tests first**

Append at the bottom of `git_branch.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn branch_from_head_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join(".git/HEAD"), "ref: refs/heads/main\n");
        let (state, head) = detect_head(root).expect("detected");
        assert_eq!(state, HeadState::Branch("main".to_string()));
        assert_eq!(head, root.join(".git/HEAD"));
    }

    #[test]
    fn nested_detection_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join(".git/HEAD"), "ref: refs/heads/main\n");
        let deep = root.join("a/b/c");
        fs::create_dir_all(&deep).unwrap();
        let (state, _) = detect_head(&deep).expect("detected");
        assert_eq!(state, HeadState::Branch("main".to_string()));
    }

    #[test]
    fn slash_branch_name_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join(".git/HEAD"), "ref: refs/heads/feature/a/b\n");
        let (state, _) = detect_head(root).expect("detected");
        assert_eq!(state, HeadState::Branch("feature/a/b".to_string()));
    }

    #[test]
    fn detached_head_returns_full_sha() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sha = "0123456789abcdef0123456789abcdef01234567";
        write(&root.join(".git/HEAD"), &format!("{sha}\n"));
        let (state, _) = detect_head(root).expect("detected");
        assert_eq!(state, HeadState::Detached(sha.to_string()));
    }

    #[test]
    fn git_file_resolves_absolute_gitdir() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let gitdir = tmp.path().join("elsewhere/gitdir");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&gitdir).unwrap();
        write(&gitdir.join("HEAD"), "ref: refs/heads/main\n");
        write(
            &repo.join(".git"),
            &format!("gitdir: {}\n", gitdir.display()),
        );
        let (state, head) = detect_head(&repo).expect("detected");
        assert_eq!(state, HeadState::Branch("main".to_string()));
        assert_eq!(head, gitdir.join("HEAD"));
    }

    #[test]
    fn git_file_resolves_relative_gitdir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let gitdir = root.join("repo/actual-gitdir");
        fs::create_dir_all(&gitdir).unwrap();
        write(&gitdir.join("HEAD"), "ref: refs/heads/dev\n");
        write(&repo.join(".git"), "gitdir: ./actual-gitdir\n");
        let (state, _) = detect_head(&repo).expect("detected");
        assert_eq!(state, HeadState::Branch("dev".to_string()));
    }

    #[test]
    fn git_file_with_missing_gitdir_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        write(&repo.join(".git"), "gitdir: /definitely/does/not/exist\n");
        assert!(detect_head(&repo).is_none());
    }

    #[test]
    fn not_a_repo_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(detect_head(tmp.path()).is_none());
    }

    #[test]
    fn malformed_head_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join(".git/HEAD"), "this is not a ref or a sha\n");
        assert!(detect_head(root).is_none());
    }

    #[test]
    fn symlinked_cwd_resolves_to_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        write(&repo.join(".git/HEAD"), "ref: refs/heads/main\n");
        let link = root.join("link-to-repo");
        symlink(&repo, &link).unwrap();
        let (state, _) = detect_head(&link).expect("detected");
        assert_eq!(state, HeadState::Branch("main".to_string()));
    }
}
```

- [ ] **Step 2: Run the tests, expect most to fail**

Run: `cargo test -p limux-host-linux --lib workspace_context::git_branch::tests`

Expected: 10 tests, 10 fail (all scenarios go through the stub that returns `None`).

- [ ] **Step 3: Implement `detect_head`**

Replace the body of `detect_head` in `git_branch.rs`:

```rust
pub fn detect_head(start: &Path) -> Option<(HeadState, PathBuf)> {
    let start = std::fs::canonicalize(start).ok()?;
    let (gitdir, _dot_git_parent) = find_gitdir(&start)?;
    let head_path = gitdir.join("HEAD");
    let head_raw = std::fs::read_to_string(&head_path).ok()?;
    let state = parse_head(head_raw.trim_end_matches(['\r', '\n']))?;
    Some((state, head_path))
}

fn find_gitdir(start: &Path) -> Option<(PathBuf, PathBuf)> {
    for ancestor in start.ancestors() {
        let candidate = ancestor.join(".git");
        let meta = match std::fs::symlink_metadata(&candidate) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_dir() {
            return Some((candidate, ancestor.to_path_buf()));
        }
        if meta.file_type().is_file() {
            let body = std::fs::read_to_string(&candidate).ok()?;
            let gitdir = parse_gitfile(&body)?;
            let resolved = if gitdir.is_absolute() {
                gitdir
            } else {
                ancestor.join(gitdir)
            };
            let canonical = std::fs::canonicalize(&resolved).ok()?;
            return Some((canonical, ancestor.to_path_buf()));
        }
    }
    None
}

fn parse_gitfile(body: &str) -> Option<PathBuf> {
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("gitdir:") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(PathBuf::from(trimmed));
            }
        }
    }
    None
}

fn parse_head(line: &str) -> Option<HeadState> {
    if let Some(rest) = line.strip_prefix("ref: refs/heads/") {
        let name = rest.trim();
        if name.is_empty() {
            return None;
        }
        return Some(HeadState::Branch(name.to_string()));
    }
    if line.len() == 40 && line.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(HeadState::Detached(line.to_string()));
    }
    None
}
```

- [ ] **Step 4: Run the tests, expect PASS**

Run: `cargo test -p limux-host-linux --lib workspace_context::git_branch::tests`

Expected: 10 passed.

- [ ] **Step 5: Full quality gate**

Run: `./scripts/check.sh`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rust/limux-host-linux/src/workspace_context/git_branch.rs
git commit -m "feat(workspace-context): implement detect_head pure logic with test matrix"
```

---

## Task 4: Implement `GitBranchProvider::evaluate` returning `ContextLine`

**Rationale.** Evaluate is the render layer around `detect_head`. Branch mode gets the Nerd Font glyph and full name; detached mode gets italic muted styling and a 7-char SHA prefix.

**Files:**
- Modify: `rust/limux-host-linux/src/workspace_context/git_branch.rs`

- [ ] **Step 1: Add evaluate tests**

Inside the existing `mod tests` in `git_branch.rs`, add:

```rust
#[test]
fn evaluate_branch_produces_nerdfont_context_line() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join(".git/HEAD"), "ref: refs/heads/main\n");
    let provider = GitBranchProvider::new();
    let line = provider
        .evaluate(&WorkspaceContext {
            workspace_id: "ws".to_string(),
            cwd: Some(root.to_path_buf()),
            folder_path: None,
        })
        .expect("line");
    assert_eq!(line.icon, Some("\u{F418}"));
    assert_eq!(line.text, "main");
    assert_eq!(line.tooltip.as_deref(), Some("branch: main"));
    assert_eq!(line.css_class, Some("limux-ctx-git-branch"));
}

#[test]
fn evaluate_detached_produces_short_sha_and_detached_class() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let sha = "0123456789abcdef0123456789abcdef01234567";
    write(&root.join(".git/HEAD"), &format!("{sha}\n"));
    let provider = GitBranchProvider::new();
    let line = provider
        .evaluate(&WorkspaceContext {
            workspace_id: "ws".to_string(),
            cwd: Some(root.to_path_buf()),
            folder_path: None,
        })
        .expect("line");
    assert_eq!(line.text, "0123456");
    assert_eq!(line.css_class, Some("limux-ctx-git-branch-detached"));
    assert_eq!(
        line.tooltip.as_deref(),
        Some("detached HEAD at 0123456789abcdef0123456789abcdef01234567"),
    );
}

#[test]
fn evaluate_falls_back_to_folder_path_when_cwd_is_none() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join(".git/HEAD"), "ref: refs/heads/main\n");
    let provider = GitBranchProvider::new();
    let line = provider
        .evaluate(&WorkspaceContext {
            workspace_id: "ws".to_string(),
            cwd: None,
            folder_path: Some(root.to_path_buf()),
        })
        .expect("line");
    assert_eq!(line.text, "main");
}

#[test]
fn evaluate_returns_none_when_no_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let provider = GitBranchProvider::new();
    let result = provider.evaluate(&WorkspaceContext {
        workspace_id: "ws".to_string(),
        cwd: Some(tmp.path().to_path_buf()),
        folder_path: None,
    });
    assert!(result.is_none());
}
```

- [ ] **Step 2: Run, expect failures**

Run: `cargo test -p limux-host-linux --lib workspace_context::git_branch::tests::evaluate`

Expected: 4 failures (stub returns `None`).

- [ ] **Step 3: Implement `evaluate`**

Replace the existing stub `evaluate` body in `git_branch.rs`:

```rust
fn evaluate(&self, ctx: &WorkspaceContext) -> Option<ContextLine> {
    let start = ctx
        .cwd
        .as_deref()
        .or(ctx.folder_path.as_deref())?;
    let (state, _) = detect_head(start)?;
    match state {
        HeadState::Branch(name) => Some(ContextLine {
            icon: Some("\u{F418}"),
            text: name.clone(),
            tooltip: Some(format!("branch: {name}")),
            css_class: Some("limux-ctx-git-branch"),
        }),
        HeadState::Detached(sha) => {
            let short_len = 7.min(sha.len());
            Some(ContextLine {
                icon: Some("\u{F418}"),
                text: sha[..short_len].to_string(),
                tooltip: Some(format!("detached HEAD at {sha}")),
                css_class: Some("limux-ctx-git-branch-detached"),
            })
        }
        HeadState::None => None,
    }
}
```

- [ ] **Step 4: Run, expect PASS**

Run: `cargo test -p limux-host-linux --lib workspace_context::git_branch::tests`

Expected: all 14 tests pass.

- [ ] **Step 5: Full quality gate**

Run: `./scripts/check.sh`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rust/limux-host-linux/src/workspace_context/git_branch.rs
git commit -m "feat(workspace-context): implement GitBranchProvider evaluate"
```

---

## Task 5: Implement `install_watchers` with `gio::FileMonitor` and a drop-guard handle

**Rationale.** The watcher re-reads HEAD when git mutates it (checkout, commit, rebase). `GFileMonitor` delivers on the GTK main thread so `notify` (non-`Send`) is safe. Cancelling the monitor on drop prevents leaks when a workspace closes or cwd changes.

**Files:**
- Modify: `rust/limux-host-linux/src/workspace_context/git_branch.rs`

- [ ] **Step 1: Implement `install_watchers` and a `Drop` wrapper**

Replace the stub `install_watchers` with:

```rust
use gtk::gio;
use gtk::prelude::*;

struct GitWatcherHandle {
    monitor: gio::FileMonitor,
}

impl WatcherHandle for GitWatcherHandle {}

impl Drop for GitWatcherHandle {
    fn drop(&mut self) {
        self.monitor.cancel();
    }
}

impl GitBranchProvider {
    fn install(&self, ctx: &WorkspaceContext, notify: Rc<dyn Fn()>) -> Option<Box<dyn WatcherHandle>> {
        let start = ctx.cwd.as_deref().or(ctx.folder_path.as_deref())?;
        let (_, head_path) = detect_head(start)?;
        let file = gio::File::for_path(&head_path);
        let monitor = file
            .monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            .ok()?;
        let notify_for_signal = notify.clone();
        monitor.connect_changed(move |_, _, _, _| notify_for_signal());
        Some(Box::new(GitWatcherHandle { monitor }))
    }
}
```

And rewrite the trait method to delegate:

```rust
fn install_watchers(
    &self,
    ctx: &WorkspaceContext,
    notify: Rc<dyn Fn()>,
) -> Box<dyn WatcherHandle> {
    self.install(ctx, notify)
        .unwrap_or_else(|| Box::new(NullWatcherHandle))
}
```

Notes:
- `gio::Cancellable::NONE` is the gtk-rs idiom for "no cancellable".
- The default `set_rate_limit` (~800 ms) is fine; do not configure it.
- When `detect_head` fails (no repo yet), we return a `NullWatcherHandle`. The next cwd change re-evaluates and will install a real monitor if the user `cd`s into a repo.

- [ ] **Step 2: Verify quality gate**

Run: `./scripts/check.sh`

Expected: PASS. Pure-logic tests continue to pass. No new unit test for the monitor path - that is explicitly out of scope per the spec's "Not tested automatically" section.

- [ ] **Step 3: Commit**

```bash
git add rust/limux-host-linux/src/workspace_context/git_branch.rs
git commit -m "feat(workspace-context): install GFileMonitor on .git/HEAD with drop cleanup"
```

---

## Task 6: Refactor `build_sidebar_row` to return a `SidebarRowWidgets` struct

**Rationale.** The tuple already has six elements; adding `badges_row` pushes it past readability. The refactor is mechanical: return a struct, update both call sites in `window.rs` (lines 2494 and 3063). No behaviour change, easy to review, easy to revert.

**Files:**
- Modify: `rust/limux-host-linux/src/window.rs`

- [ ] **Step 1: Introduce `SidebarRowWidgets` and rewrite the return**

Replace the signature and return block of `build_sidebar_row` (currently at `window.rs:1894-1971`). The function becomes:

```rust
pub(super) struct SidebarRowWidgets {
    pub row: gtk::ListBoxRow,
    pub name_label: gtk::Label,
    pub favorite_button: gtk::Button,
    pub notify_dot: gtk::Label,
    pub notify_label: gtk::Label,
    pub path_label: gtk::Label,
}

fn build_sidebar_row(name: &str, folder_path: Option<&str>) -> SidebarRowWidgets {
    // ... existing body unchanged ...
    SidebarRowWidgets {
        row,
        name_label,
        favorite_button,
        notify_dot,
        notify_label,
        path_label,
    }
}
```

Place `SidebarRowWidgets` just above `fn build_sidebar_row`. Keep the existing body byte-for-byte; only the return type and trailing tuple become the struct literal.

- [ ] **Step 2: Update call site 1 (tab-drop workspace creation)**

In `window.rs` around line 2493, replace:

```rust
let (row, name_label, favorite_button, notify_dot, notify_label, path_label) =
    build_sidebar_row(&seed.name, seed.folder_path.as_deref());
```

With:

```rust
let widgets = build_sidebar_row(&seed.name, seed.folder_path.as_deref());
let SidebarRowWidgets {
    row,
    name_label,
    favorite_button,
    notify_dot,
    notify_label,
    path_label,
} = widgets;
```

Leave subsequent uses of `row`, `name_label`, etc. untouched - variable names are identical.

- [ ] **Step 3: Update call site 2 (session restore around line 3063)**

Apply the same destructuring pattern at the second call site. Pull up the current tuple binding and replace with the struct destructure above. Use `Grep` first to confirm there are only two call sites: `grep -n build_sidebar_row rust/limux-host-linux/src/window.rs` should show line 1894 (definition), 2494, and 3063 only.

- [ ] **Step 4: Build and test**

Run: `./scripts/check.sh`

Expected: PASS. This is pure refactor - if any test fails, it means the wrong body was preserved.

- [ ] **Step 5: Commit**

```bash
git add rust/limux-host-linux/src/window.rs
git commit -m "refactor(window): return SidebarRowWidgets from build_sidebar_row"
```

---

## Task 7: Extend `Workspace` with `badges_row`, `badge_labels`, `context_watchers`

**Rationale.** Owning widgets and watchers on the Workspace makes teardown correct by construction: when a Workspace is dropped (or its watchers are `.clear()`'d before widget drop), there is no dangling callback against a freed label.

**Files:**
- Modify: `rust/limux-host-linux/src/window.rs` (struct at line 28-56)
- Modify: `rust/limux-host-linux/src/workspace_context/mod.rs` (re-export needed types)

- [ ] **Step 1: Add new fields to `Workspace`**

In `window.rs`, modify the `Workspace` struct (starts at line 28). Add three fields after `path_label`:

```rust
/// Horizontal container below the path label for provider badges.
badges_row: gtk::Box,
/// Badge labels keyed by provider id (e.g. "git.branch"). Labels are reused
/// across evaluations to avoid widget-tree churn.
badge_labels: std::collections::HashMap<&'static str, gtk::Label>,
/// Watcher handles for installed providers. Dropping a handle cancels its
/// `GFileMonitor`; clearing the vec tears them all down. Drop order matters:
/// watchers must be cleared before the badges_row widget is removed.
context_watchers: Vec<Box<dyn crate::workspace_context::WatcherHandle>>,
```

- [ ] **Step 2: Update `build_sidebar_row` to create the badges row**

In `window.rs`, extend `SidebarRowWidgets` and `build_sidebar_row` to produce the horizontal badges container:

```rust
pub(super) struct SidebarRowWidgets {
    pub row: gtk::ListBoxRow,
    pub name_label: gtk::Label,
    pub favorite_button: gtk::Button,
    pub notify_dot: gtk::Label,
    pub notify_label: gtk::Label,
    pub path_label: gtk::Label,
    pub badges_row: gtk::Box,
}
```

Inside `build_sidebar_row`, after creating `path_label` but before constructing `vbox`, add:

```rust
let badges_row = gtk::Box::builder()
    .orientation(gtk::Orientation::Horizontal)
    .spacing(6)
    .margin_start(8)
    .visible(false)
    .build();
badges_row.add_css_class("limux-ctx-badges");
```

Change the `vbox.append` sequence so `badges_row` sits between `path_label` and `notify_label`:

```rust
vbox.append(&top_row);
vbox.append(&path_label);
vbox.append(&badges_row);
vbox.append(&notify_label);
```

Include `badges_row` in the returned `SidebarRowWidgets` struct literal.

- [ ] **Step 3: Propagate the new field to both workspace creation sites**

In both call sites (around lines 2494 and 3063), add `badges_row` to the destructure pattern. Then, when constructing the `Workspace` (around line 2502 and its sibling around line 3080), add:

```rust
badges_row,
badge_labels: std::collections::HashMap::new(),
context_watchers: Vec::new(),
```

- [ ] **Step 4: Sanity-build**

Run: `cargo build -p limux-host-linux`

Expected: compiles. No new behaviour yet - the `badges_row` is invisible and empty.

- [ ] **Step 5: Run quality gate**

Run: `./scripts/check.sh`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rust/limux-host-linux/src/window.rs
git commit -m "refactor(window): add badges_row and watcher slots to Workspace"
```

---

## Task 8: Build `ProviderRegistry` once at startup and wire it into workspace creation

**Rationale.** The registry is constructed from `AppConfig.sidebar.providers` immediately after `app_config::load()` succeeds. Workspaces borrow `&ProviderRegistry` when they need to evaluate. Storing the registry in `AppState` as `Rc<ProviderRegistry>` keeps borrow patterns identical to the existing `Rc<ResolvedShortcutConfig>`.

**Files:**
- Modify: `rust/limux-host-linux/src/window.rs`

- [ ] **Step 1: Hold the registry in `AppState`**

Add a field on `AppState` (line 58-81 block):

```rust
workspace_context_registry: Rc<crate::workspace_context::ProviderRegistry>,
```

In `build_window` around `window.rs:732-736`, after loading config, construct the registry:

```rust
let workspace_context_registry = Rc::new(
    crate::workspace_context::ProviderRegistry::from_config(
        &loaded_config.config.sidebar.providers,
    ),
);
eprintln!(
    "limux: sidebar: enabled providers: [{}]",
    workspace_context_registry
        .providers()
        .iter()
        .map(|p| p.id())
        .collect::<Vec<_>>()
        .join(", ")
);
```

Thread `workspace_context_registry.clone()` into the `AppState` constructor alongside `shortcuts`.

- [ ] **Step 2: Add a helper that evaluates all providers for a workspace**

Add a free function below `AppState`:

```rust
fn refresh_workspace_context(
    workspace: &mut Workspace,
    registry: &crate::workspace_context::ProviderRegistry,
    notify_cb: std::rc::Rc<dyn Fn()>,
) {
    use crate::workspace_context::WorkspaceContext;
    use gtk::pango::EllipsizeMode;

    // Drop old watchers first so late callbacks don't fire against reused labels.
    workspace.context_watchers.clear();

    let cwd = workspace
        .cwd
        .borrow()
        .clone()
        .map(std::path::PathBuf::from);
    let folder_path = workspace
        .folder_path
        .clone()
        .map(std::path::PathBuf::from);
    let ctx = WorkspaceContext {
        workspace_id: workspace.id.clone(),
        cwd,
        folder_path,
    };

    let mut any_visible = false;
    for provider in registry.providers() {
        let id = provider.id();
        let line = provider.evaluate(&ctx);
        let label = workspace
            .badge_labels
            .entry(id)
            .or_insert_with(|| {
                let l = gtk::Label::builder()
                    .xalign(0.0)
                    .ellipsize(EllipsizeMode::End)
                    .visible(false)
                    .build();
                workspace.badges_row.append(&l);
                l
            });

        if let Some(line) = line {
            let shown = match line.icon {
                Some(icon) => format!("{icon} {}", line.text),
                None => line.text.clone(),
            };
            label.set_label(&shown);
            label.set_tooltip_text(line.tooltip.as_deref());
            // Clear previously applied css classes before re-applying.
            for class in label.css_classes() {
                if class.starts_with("limux-ctx-") {
                    label.remove_css_class(&class);
                }
            }
            if let Some(css) = line.css_class {
                label.add_css_class(css);
            }
            label.set_visible(true);
            any_visible = true;
        } else {
            label.set_visible(false);
        }

        workspace
            .context_watchers
            .push(provider.install_watchers(&ctx, notify_cb.clone()));
    }

    workspace.badges_row.set_visible(any_visible);
}
```

Compiler note: the closure-over-mutable-borrow with `entry().or_insert_with` cannot access `workspace.badges_row` while holding the `HashMap` borrow. Rewrite the label lookup as an explicit two-phase pattern if the borrow checker rejects:

```rust
if !workspace.badge_labels.contains_key(id) {
    let l = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(EllipsizeMode::End)
        .visible(false)
        .build();
    workspace.badges_row.append(&l);
    workspace.badge_labels.insert(id, l);
}
let label = workspace.badge_labels.get(id).expect("inserted above");
```

Use whichever version compiles cleanly.

- [ ] **Step 3: Call `refresh_workspace_context` on creation**

Both workspace-creation paths (around lines 2502 and 3080) construct a `Workspace` and push it into `app_state.workspaces`. After that push, obtain a mutable ref back and refresh. A cwd change notify closure is needed; use a capturing closure that re-runs the refresh. The notify closure must not borrow `AppState` - it must `state.borrow_mut()` on demand.

For the tab-drop creation path, after the `app_state.workspaces.push(Workspace { ... })`:

```rust
let ws_id = new_workspace_id.clone();
let state_for_notify = state.clone();
let notify: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
    trigger_workspace_context_refresh(&state_for_notify, &ws_id);
});
{
    let mut app_state = state.borrow_mut();
    let registry = app_state.workspace_context_registry.clone();
    if let Some(ws) = app_state.workspaces.iter_mut().find(|w| w.id == new_workspace_id) {
        refresh_workspace_context(ws, &registry, notify);
    }
}
```

Add the helper:

```rust
fn trigger_workspace_context_refresh(state: &State, workspace_id: &str) {
    let registry = state.borrow().workspace_context_registry.clone();
    let ws_id = workspace_id.to_string();
    let state_for_closure = state.clone();
    let notify: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
        trigger_workspace_context_refresh(&state_for_closure, &ws_id);
    });
    let mut app_state = state.borrow_mut();
    if let Some(ws) = app_state.workspaces.iter_mut().find(|w| w.id == workspace_id) {
        refresh_workspace_context(ws, &registry, notify);
    }
}
```

Apply the identical pattern to the session-restore creation path (around line 3080). Use the workspace's own id in each closure.

- [ ] **Step 4: Build and test**

Run: `./scripts/check.sh`

Expected: PASS. Launch the app manually to sanity-check that a workspace opened in a git repo shows a branch badge.

- [ ] **Step 5: Commit**

```bash
git add rust/limux-host-linux/src/window.rs
git commit -m "feat(window): evaluate workspace-context providers on workspace creation"
```

---

## Task 9: Re-evaluate providers on OSC-7 cwd change

**Rationale.** The existing `on_pwd_changed` callback at `window.rs:3177-3187` writes `workspace.cwd`. The repo boundary may have moved, so we drop every watcher and reinstall. Cheap, and avoids the "watcher still points at the old repo" failure mode called out in the spec.

**Files:**
- Modify: `rust/limux-host-linux/src/window.rs`

- [ ] **Step 1: Extend the OSC-7 handler**

Locate the `on_pwd_changed` closure near line 3177. Replace its body with:

```rust
on_pwd_changed: Box::new(move |pwd: &str| {
    let state = state_for_pwd.clone();
    let ws_id = ws_id_pwd.clone();
    let pwd = pwd.to_string();
    glib::idle_add_local_once(move || {
        {
            let s = state.borrow();
            if let Some(ws) = s.workspaces.iter().find(|w| w.id == ws_id) {
                *ws.cwd.borrow_mut() = Some(pwd);
            } else {
                return;
            }
        }
        trigger_workspace_context_refresh(&state, &ws_id);
    });
}),
```

The two-phase borrow is intentional: update `cwd` under an immutable `s.borrow()` (the `RefCell<String>` field allows interior mutation), then drop the borrow before `trigger_workspace_context_refresh` re-borrows mutably.

- [ ] **Step 2: Build**

Run: `cargo build -p limux-host-linux`

Expected: compiles.

- [ ] **Step 3: Run quality gate**

Run: `./scripts/check.sh`

Expected: PASS.

- [ ] **Step 4: Manual smoke test**

Launch the app against a git repo:

```bash
(cd ghostty && zig build -Dapp-runtime=none -Doptimize=ReleaseFast) 2>/dev/null  # only if missing
cargo build -p limux-host-linux --release
LD_LIBRARY_PATH=ghostty/zig-out/lib:$LD_LIBRARY_PATH \
    ./target/release/limux
```

Inside the Limux terminal surface: `cd` to a repo with a different branch and confirm the badge updates within ~1 second. Run `git checkout -b feat/foo` and confirm the badge updates. Cannot be automated.

- [ ] **Step 5: Commit**

```bash
git add rust/limux-host-linux/src/window.rs
git commit -m "feat(window): refresh workspace-context providers on OSC-7 cwd change"
```

---

## Task 10: Add CSS for `.limux-ctx-badges`, `.limux-ctx-git-branch`, `.limux-ctx-git-branch-detached`

**Rationale.** The existing CSS block lives in `window.rs` as a const string ending around line 716. Add the three new rules there so they ship in the same stylesheet.

**Files:**
- Modify: `rust/limux-host-linux/src/window.rs`

- [ ] **Step 1: Append CSS rules**

In the CSS const string (the `r#"..."#` block that includes `.limux-ws-path` at line 706), add after the `row:selected .limux-ws-path` rule and before `.limux-content`:

```css
.limux-ctx-badges > label {
    font-size: smaller;
    opacity: 0.7;
}
.limux-ctx-git-branch {
    font-family: "Symbols Nerd Font", "Symbols Nerd Font Mono",
                 "JetBrainsMono Nerd Font", "FiraCode Nerd Font",
                 "Hack Nerd Font", inherit;
}
.limux-ctx-git-branch-detached {
    font-style: italic;
    opacity: 0.55;
}
```

- [ ] **Step 2: Full quality gate**

Run: `./scripts/check.sh`

Expected: PASS.

- [ ] **Step 3: Manual visual verification**

Relaunch the app, confirm:
- Branch badge renders with the Nerd Font glyph (if a Nerd Font is installed) or falls back to the UI font.
- Detached-state badge renders italic and muted.

- [ ] **Step 4: Commit**

```bash
git add rust/limux-host-linux/src/window.rs
git commit -m "feat(window): add CSS for workspace-context badges"
```

---

## Task 11: Walk the manual QA checklist and capture results

**Rationale.** Anything involving GTK rendering and filesystem monitors requires human eyes. This task is the gate before marking the PR ready.

**Files:** no code changes; may modify the spec's checklist if a new case is discovered.

- [ ] **Step 1: Walk every item in the spec's "Manual QA checklist"**

Open `docs/superpowers/specs/2026-04-15-workspace-context-providers-design.md`, find the "Manual QA checklist" section, and reproduce each step against a release build:

```bash
cargo build -p limux-host-linux --release
LD_LIBRARY_PATH=ghostty/zig-out/lib:$LD_LIBRARY_PATH ./target/release/limux
```

Run through each bullet. For each one, record PASS or note a defect.

- [ ] **Step 2: File any regressions found**

If a step fails, open a bug note directly in the plan as a follow-up task or fix inline if small. Do not move on.

- [ ] **Step 3: Confirm config edge cases**

In `$XDG_CONFIG_HOME/limux/settings.json`, temporarily set `"sidebar": {"providers": []}` and relaunch - badges_row should be hidden. Set `"sidebar": {"providers": ["nonexistent.id"]}` and confirm the stderr output contains `limux: sidebar: ignoring unknown provider id: "nonexistent.id"`. Restore config.

- [ ] **Step 4: Close-while-watching smoke test**

Open two workspaces in git repos, `git checkout -b throwaway` in one, close that workspace while the monitor is presumably about to fire. No crash, no leaked file descriptors (`ls -l /proc/$(pidof limux)/fd | grep HEAD` should show zero).

- [ ] **Step 5: No commit**

This task produces only a QA report, not code. Proceed to Task 12 only when every checklist item is green.

---

## Task 12: Final verification and handoff

**Files:** no changes. Final sanity check before PR.

- [ ] **Step 1: Run the canonical quality gate one more time**

Run: `./scripts/check.sh`

Expected: PASS.

- [ ] **Step 2: Confirm no unintended files committed**

Run: `git status` and `git log --oneline origin/main..HEAD`

Expected: only the workspace_context-related commits on the branch. No build artefacts, no stray `scratch/` files.

- [ ] **Step 3: Smoke packaging (optional, recommended before release)**

Run: `./scripts/package.sh` on Ubuntu 24.04 (release builds pin to this distro per `CLAUDE.md`).

Expected: tarball + .deb + .rpm + AppImage build without glibc-baseline complaints. Skip if not building a release.

- [ ] **Step 4: Hand off**

Summarise changes in the PR body: new module, one provider, one config key (default enabled), two refactors (`SidebarRowWidgets`, two call sites updated), CSS additions. Link back to the design spec.

---

## Out-of-scope reminders

Do not add any of the following in this plan's PR. They are deferred by design:

- Dirty indicator (`git.dirty`).
- Rebase/bisect/merge state.
- Surface-count badge.
- Per-workspace provider overrides.
- Hot-reload of config.
- Settings-editor UI for sidebar providers.
- `GIT_DIR` / `GIT_WORK_TREE` env handling.
- Bare repo handling.

If any of these feels necessary while implementing, stop and revisit the spec rather than growing scope silently.
