//! Git branch / detached HEAD provider.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use gtk4 as gtk;

use super::{
    ContextLine, NullWatcherHandle, WatcherHandle, WorkspaceContext, WorkspaceContextProvider,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeadState {
    Branch(String),
    Detached(String),
    None,
}

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

    fn evaluate(&self, ctx: &WorkspaceContext) -> Option<ContextLine> {
        let start = ctx.cwd.as_deref().or(ctx.folder_path.as_deref())?;
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

    fn install_watchers(
        &self,
        ctx: &WorkspaceContext,
        notify: Rc<dyn Fn()>,
    ) -> Box<dyn WatcherHandle> {
        self.install(ctx, notify)
            .unwrap_or_else(|| Box::new(NullWatcherHandle))
    }
}

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
    fn install(
        &self,
        ctx: &WorkspaceContext,
        notify: Rc<dyn Fn()>,
    ) -> Option<Box<dyn WatcherHandle>> {
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
}
