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
