//! Git branch / detached HEAD provider. Implementation lands across tasks 3-5.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::{
    ContextLine, NullWatcherHandle, WatcherHandle, WorkspaceContext, WorkspaceContextProvider,
};

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
