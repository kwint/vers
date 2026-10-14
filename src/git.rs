//! Git integration. vers shells out to `git` so the user's git config (signing,
//! hooks, credentials) applies.
//!
//! Spec: `DESIGN.md` §1.4 (ordering), §6.1 (release), §6.2 (check),
//! `docs/how-to/require-fragments-in-ci.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::RepoKind;
use crate::error::{Error, Result};

/// A git working tree.
#[derive(Debug, Clone)]
pub struct Git {
    root: PathBuf,
}

impl Git {
    /// The working tree containing `path`, or `None` if `path` isn't inside one
    /// (or git isn't installed).
    pub fn discover(path: &Path) -> Result<Option<Git>> {
        let output = Command::new("git").arg("-C").arg(path).args(["rev-parse", "--show-toplevel"]).output();
        match output {
            Ok(out) if out.status.success() => {
                let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let root = PathBuf::from(root);
                let root = root.canonicalize().unwrap_or(root);
                Ok(Some(Git { root }))
            }
            _ => Ok(None),
        }
    }

    /// Absolute, canonical path of the working tree root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Run git in the working tree root and return trimmed stdout.
    pub(crate) fn run<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let args: Vec<_> = args.into_iter().map(|a| a.as_ref().to_os_string()).collect();
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(&args)
            .output()
            .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;
        if !out.status.success() {
            let cmd = args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ");
            return Err(Error::Git(format!("`git {cmd}` failed: {}", String::from_utf8_lossy(&out.stderr).trim())));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    }

    /// URL of the remote `name`, or `None` if there is no such remote.
    pub fn remote_url(&self, name: &str) -> Result<Option<String>> {
        match self.run(["remote", "get-url", name]) {
            Ok(url) if !url.is_empty() => Ok(Some(url)),
            _ => Ok(None),
        }
    }

    pub fn is_shallow(&self) -> Result<bool> {
        let _ = self;
        todo!("phase 1: git")
    }

    /// Author time (seconds since the epoch) of the commit that first added
    /// each file under `dirs`, keyed by absolute path. Uses a single
    /// `git log --diff-filter=A` call. Files never committed are absent.
    pub fn added_times(&self, dirs: &[PathBuf]) -> Result<HashMap<PathBuf, i64>> {
        let _ = dirs;
        todo!("phase 1: git")
    }

    /// Files (absolute paths) changed between the merge base of `base` and
    /// `HEAD`, and `HEAD` (`git diff --name-only base...HEAD`). Deleted files
    /// are included.
    pub fn changed_files(&self, base: &str) -> Result<Vec<PathBuf>> {
        let _ = base;
        todo!("phase 1: git")
    }

    /// Files (absolute paths) added or modified between the merge base of
    /// `base` and `HEAD`.
    pub fn added_or_modified_files(&self, base: &str) -> Result<Vec<PathBuf>> {
        let _ = base;
        todo!("phase 1: git")
    }

    /// Whether `path` is tracked (in the index).
    pub fn is_tracked(&self, path: &Path) -> Result<bool> {
        let _ = path;
        todo!("phase 1: git")
    }

    /// `git rm -q` the given paths.
    pub fn rm(&self, paths: &[PathBuf]) -> Result<()> {
        let _ = paths;
        todo!("phase 1: git")
    }

    /// Stage exactly `paths` (including deletions) and commit only them with
    /// `message`, leaving any other staged or unstaged changes alone.
    pub fn commit_paths(&self, paths: &[PathBuf], message: &str) -> Result<()> {
        let _ = (paths, message);
        todo!("phase 1: git")
    }

    /// Create an annotated tag `name` at `HEAD` with `message`.
    pub fn tag(&self, name: &str, message: &str) -> Result<()> {
        let _ = (name, message);
        todo!("phase 1: git")
    }

    /// All tag names.
    pub fn tags(&self) -> Result<Vec<String>> {
        todo!("phase 1: git")
    }
}

/// Default base ref for `vers check` when `--base` isn't given:
/// `GITHUB_BASE_REF` (as `origin/<ref>`), then `CI_MERGE_REQUEST_DIFF_BASE_SHA`,
/// then `origin/HEAD`.
pub fn default_check_base() -> String {
    if let Ok(r) = std::env::var("GITHUB_BASE_REF")
        && !r.is_empty()
    {
        return format!("origin/{r}");
    }
    if let Ok(sha) = std::env::var("CI_MERGE_REQUEST_DIFF_BASE_SHA")
        && !sha.is_empty()
    {
        return sha;
    }
    "origin/HEAD".to_string()
}

/// Convert a git remote URL to the repository's web URL:
/// `git@host:a/b.git` and `ssh://git@host/a/b.git` → `https://host/a/b`;
/// `https://host/a/b.git` → `https://host/a/b`. Returns `None` for local paths.
pub fn web_url_from_remote(remote: &str) -> Option<String> {
    let remote = remote.trim();
    let (host, path) = if let Some(rest) = remote.strip_prefix("https://").or_else(|| remote.strip_prefix("http://")) {
        let rest = rest.split_once('@').map_or(rest, |(_, r)| r);
        rest.split_once('/')?
    } else if let Some(rest) = remote.strip_prefix("ssh://") {
        let rest = rest.split_once('@').map_or(rest, |(_, r)| r);
        let (host, path) = rest.split_once('/')?;
        (host.split_once(':').map_or(host, |(h, _)| h), path)
    } else if let Some((user_host, path)) = remote.split_once(':') {
        if user_host.contains('/') || path.starts_with("//") {
            return None;
        }
        (user_host.split_once('@').map_or(user_host, |(_, h)| h), path)
    } else {
        return None;
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!("https://{host}/{path}"))
}

/// Detect the repository kind from a web URL's host: github.com → GitHub,
/// gitlab.com → GitLab, anything else → None.
pub fn detect_repo_kind(web_url: &str) -> RepoKind {
    let host = web_url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or("");
    match host.to_ascii_lowercase().as_str() {
        "github.com" | "www.github.com" => RepoKind::Github,
        "gitlab.com" | "www.gitlab.com" => RepoKind::Gitlab,
        _ => RepoKind::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_urls() {
        let cases = [
            ("git@github.com:kwint/vers.git", Some("https://github.com/kwint/vers")),
            ("https://github.com/kwint/vers.git", Some("https://github.com/kwint/vers")),
            ("https://token@gitlab.com/a/b/c", Some("https://gitlab.com/a/b/c")),
            ("ssh://git@gitlab.example.com:2222/team/proj.git", Some("https://gitlab.example.com/team/proj")),
            ("/srv/git/repo.git", None),
            ("../other", None),
        ];
        for (remote, expected) in cases {
            assert_eq!(web_url_from_remote(remote).as_deref(), expected, "{remote}");
        }
    }

    #[test]
    fn repo_kinds() {
        assert_eq!(detect_repo_kind("https://github.com/a/b"), RepoKind::Github);
        assert_eq!(detect_repo_kind("https://gitlab.com/a/b"), RepoKind::Gitlab);
        assert_eq!(detect_repo_kind("https://gitlab.example.com/a/b"), RepoKind::None);
    }
}
