//! Git integration. vers shells out to `git` so the user's git config (signing,
//! hooks, credentials) applies.
//!
//! Spec: `DESIGN.md` §1.4 (ordering), §6.1 (release), §6.2 (check),
//! `docs/how-to/require-fragments-in-ci.md`.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

use crate::config::RepoKind;
use crate::error::{Error, Result};

/// Marks the start of a commit's record in `git log` output (see
/// [`Git::added_times`]). A control character can't plausibly start a path.
const COMMIT_MARKER: u8 = 0x01;

/// A git working tree.
#[derive(Debug, Clone)]
pub struct Git {
    root: PathBuf,
    /// Extra environment for every git invocation: `Some` sets a variable,
    /// `None` removes it. Empty outside tests.
    env: Vec<(OsString, Option<OsString>)>,
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
                let root = root.canonicalize().map(simplify_verbatim).unwrap_or(root);
                Ok(Some(Git { root, env: Vec::new() }))
            }
            _ => Ok(None),
        }
    }

    /// Absolute, canonical path of the working tree root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A `git` command running in the working tree root.
    fn command(&self) -> Command {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.root);
        for (key, value) in &self.env {
            match value {
                Some(value) => cmd.env(key, value),
                None => cmd.env_remove(key),
            };
        }
        cmd
    }

    /// Run git with `args` and return its output, whatever the exit status.
    fn output(&self, args: &[OsString]) -> Result<Output> {
        self.command()
            .args(args)
            .output()
            .map_err(|e| Error::Git(format!("failed to run git: {e} (is git installed and on the PATH?)")))
    }

    /// The error for a git command that exited unsuccessfully.
    fn failure(args: &[OsString], out: &Output) -> Error {
        let cmd = args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ");
        Error::Git(format!("`git {cmd}` failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }

    /// Run git with `args` and return its raw stdout, failing on a non-zero exit.
    fn run_bytes(&self, args: &[OsString]) -> Result<Vec<u8>> {
        let out = self.output(args)?;
        if !out.status.success() {
            return Err(Self::failure(args, &out));
        }
        Ok(out.stdout)
    }

    /// Run git in the working tree root and return trimmed stdout.
    pub(crate) fn run<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let args: Vec<OsString> = args.into_iter().map(|a| a.as_ref().to_os_string()).collect();
        let stdout = self.run_bytes(&args)?;
        Ok(String::from_utf8_lossy(&stdout).trim_end().to_string())
    }

    /// URL of the remote `name`, or `None` if there is no such remote.
    pub fn remote_url(&self, name: &str) -> Result<Option<String>> {
        match self.run(["remote", "get-url", name]) {
            Ok(url) if !url.is_empty() => Ok(Some(url)),
            _ => Ok(None),
        }
    }

    /// Whether the repository is a shallow clone, so history (and with it
    /// add-times and merge bases) may be incomplete.
    pub fn is_shallow(&self) -> Result<bool> {
        Ok(self.run(["rev-parse", "--is-shallow-repository"])?.trim() == "true")
    }

    /// Author time (seconds since the epoch) of the commit that first added
    /// each file under `dirs`, keyed by absolute path. Uses a single
    /// `git log --diff-filter=A` call. Files never committed are absent.
    ///
    /// Relative `dirs` are relative to the working tree root. Directories that
    /// don't exist or lie outside the working tree are fine, and a repository
    /// without commits gives an empty map.
    ///
    /// Each key is the directory it was found under, as passed in, joined with
    /// the rest of the path. A caller that lists a directory and looks its
    /// files up here gets matching paths even when the directory was reached
    /// through a symlink (macOS `/var`) or written in another form (Windows
    /// `\\?\` or 8.3 short names).
    pub fn added_times(&self, dirs: &[PathBuf]) -> Result<HashMap<PathBuf, i64>> {
        let mut times = HashMap::new();
        // (root-relative pathspec, the directory as the caller wrote it)
        let bases: Vec<(OsString, PathBuf)> = dirs
            .iter()
            .filter_map(|dir| {
                let rel = self.relative(dir)?;
                if rel.components().next() == Some(Component::ParentDir) {
                    return None;
                }
                Some((slash_path(&rel), self.resolve(dir)))
            })
            .collect();
        if bases.is_empty() {
            return Ok(times);
        }
        let mut args = git_args(&[
            "-c",
            "core.quotepath=off",
            "-c",
            "log.showSignature=false",
            "-c",
            "log.follow=false",
            "--literal-pathspecs",
            "log",
            "--no-renames",
            "--diff-filter=A",
            "--name-only",
            "--no-color",
            "-z",
            "--format=%x01%at",
            "--",
        ]);
        args.extend(bases.iter().map(|(spec, _)| spec.clone()));
        let out = self.output(&args)?;
        if !out.status.success() {
            if !self.has_commits()? {
                return Ok(times);
            }
            return Err(Self::failure(&args, &out));
        }

        // Per commit: `\x01<time>`, then the paths it added; everything is
        // separated by NULs and newlines. git lists the newest commit first, so
        // a later record for the same path is an older add, which wins.
        let mut current: Option<i64> = None;
        for token in out.stdout.split(|&b| b == 0) {
            let mut token = trim_leading_newlines(token);
            if let Some(rest) = token.strip_prefix(&[COMMIT_MARKER]) {
                let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
                let time = std::str::from_utf8(&rest[..end]).ok().and_then(|t| t.trim().parse::<i64>().ok());
                let Some(time) = time else {
                    return Err(Error::Git(format!(
                        "unexpected `git log` output: bad commit time '{}'",
                        String::from_utf8_lossy(&rest[..end])
                    )));
                };
                current = Some(time);
                token = trim_leading_newlines(&rest[end..]);
            }
            if token.is_empty() {
                continue;
            }
            let Some(time) = current else {
                return Err(Error::Git(format!(
                    "unexpected `git log` output: path '{}' before any commit",
                    String::from_utf8_lossy(token)
                )));
            };
            times.insert(self.key_under(token, &bases), time);
        }
        Ok(times)
    }

    /// The absolute path for `git_path`, built on the longest of `bases` that
    /// contains it (see [`Git::added_times`]), or on the root.
    fn key_under(&self, git_path: &[u8], bases: &[(OsString, PathBuf)]) -> PathBuf {
        let best = bases
            .iter()
            .filter_map(|(spec, given)| {
                let spec = os_to_bytes(spec);
                let rest = if spec == b"." {
                    Some(git_path)
                } else {
                    git_path.strip_prefix(spec.as_slice()).and_then(|rest| rest.strip_prefix(b"/"))
                };
                rest.map(|rest| (spec.len(), given, rest))
            })
            .max_by_key(|(len, _, _)| *len);
        match best {
            Some((_, given, rest)) => join_slash(given.clone(), rest),
            None => self.absolute(git_path),
        }
    }

    /// Files (absolute paths) changed between the merge base of `base` and
    /// `HEAD`, and `HEAD` (`git diff --name-only base...HEAD`). Deleted files
    /// are included.
    pub fn changed_files(&self, base: &str) -> Result<Vec<PathBuf>> {
        self.diff_names(base, None)
    }

    /// Files (absolute paths) added or modified between the merge base of
    /// `base` and `HEAD`.
    pub fn added_or_modified_files(&self, base: &str) -> Result<Vec<PathBuf>> {
        self.diff_names(base, Some("--diff-filter=AM"))
    }

    fn diff_names(&self, base: &str, filter: Option<&str>) -> Result<Vec<PathBuf>> {
        self.verify_base(base)?;
        let mut args = git_args(&[
            "-c",
            "core.quotepath=off",
            "diff",
            "--name-only",
            "--no-renames",
            "--no-ext-diff",
            "--no-color",
            "-z",
        ]);
        if let Some(filter) = filter {
            args.push(filter.into());
        }
        args.push(format!("{base}...HEAD").into());
        args.push("--".into());
        let out = self.output(&args)?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let message = if stderr.contains("merge base") {
                format!(
                    "no common ancestor between '{base}' and HEAD; if this is a shallow clone, fetch the full \
                     history (`git fetch --unshallow`, or `fetch-depth: 0` / `GIT_DEPTH: 0` in CI)"
                )
            } else {
                format!("cannot compare HEAD with base '{base}': {}", stderr.trim())
            };
            return Err(Error::Git(message));
        }
        Ok(out.stdout.split(|&b| b == 0).filter(|p| !p.is_empty()).map(|p| self.absolute(p)).collect())
    }

    /// Check that `base` names a commit, with a hint for the usual CI causes.
    fn verify_base(&self, base: &str) -> Result<()> {
        if base.trim().is_empty() {
            return Err(Error::Git("empty base ref; pass a branch, tag or commit with --base".to_string()));
        }
        let commit = format!("{base}^{{commit}}");
        let args = git_args(&["rev-parse", "--verify", "--quiet", "--end-of-options", &commit]);
        if self.output(&args)?.status.success() {
            return Ok(());
        }
        let fetch = match base.strip_prefix("origin/") {
            Some(branch) if branch != "HEAD" => format!("`git fetch origin {branch}`"),
            _ => "`git fetch origin`".to_string(),
        };
        let mut message = format!(
            "cannot resolve base ref '{base}'; fetch the base branch first ({fetch}), use a full clone in CI \
             (`fetch-depth: 0` / `GIT_DEPTH: 0`), or pass another ref with --base"
        );
        if base == "origin/HEAD" {
            message.push_str("; if origin/HEAD is missing, `git remote set-head origin --auto` creates it");
        }
        Err(Error::Git(message))
    }

    /// Whether `path` is tracked (in the index). A path outside the working
    /// tree is not.
    pub fn is_tracked(&self, path: &Path) -> Result<bool> {
        match self.relative(path) {
            Some(rel) if rel.components().next() != Some(Component::ParentDir) => {}
            _ => return Ok(false),
        }
        let mut args = git_args(&["--literal-pathspecs", "ls-files", "--error-unmatch", "--"]);
        args.push(self.pathspec(path));
        let out = self.output(&args)?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(Self::failure(&args, &out)),
        }
    }

    /// `git rm -q` the given paths.
    pub fn rm(&self, paths: &[PathBuf]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut args = git_args(&["--literal-pathspecs", "rm", "-q", "--"]);
        args.extend(paths.iter().map(|p| self.pathspec(p)));
        self.run_bytes(&args)?;
        Ok(())
    }

    /// Stage exactly `paths` (including deletions) and commit only them with
    /// `message`, leaving any other staged or unstaged changes alone.
    pub fn commit_paths(&self, paths: &[PathBuf], message: &str) -> Result<()> {
        if paths.is_empty() {
            return Err(Error::Git("nothing to commit: no files were changed".to_string()));
        }
        let specs: Vec<OsString> = paths.iter().map(|p| self.pathspec(p)).collect();

        // `git add` refuses a path that is neither on disk nor in the index (a
        // file already removed with `git rm`); `commit --only` takes those
        // deletions from HEAD by itself.
        let (on_disk, missing): (Vec<_>, Vec<_>) =
            paths.iter().zip(&specs).partition(|(path, _)| self.resolve(path).symlink_metadata().is_ok());
        let mut to_add: Vec<OsString> = on_disk.into_iter().map(|(_, spec)| spec.clone()).collect();
        if !missing.is_empty() {
            let mut args = git_args(&["--literal-pathspecs", "ls-files", "-z", "--"]);
            args.extend(missing.iter().map(|(_, spec)| (*spec).clone()));
            let out = self.run_bytes(&args)?;
            let indexed: HashSet<&[u8]> = out.split(|&b| b == 0).filter(|p| !p.is_empty()).collect();
            to_add.extend(
                missing
                    .iter()
                    .filter(|(_, spec)| indexed.contains(spec.to_string_lossy().as_bytes()))
                    .map(|(_, spec)| (*spec).clone()),
            );
        }
        if !to_add.is_empty() {
            let mut args = git_args(&["--literal-pathspecs", "add", "-A", "--"]);
            args.extend(to_add);
            self.run_bytes(&args)?;
        }

        let mut args = git_args(&["--literal-pathspecs", "commit", "-q", "--only", "-m", message, "--"]);
        args.extend(specs);
        self.run_bytes(&args)?;
        Ok(())
    }

    /// Create an annotated tag `name` at `HEAD` with `message`.
    pub fn tag(&self, name: &str, message: &str) -> Result<()> {
        if self.tag_exists(name)? {
            return Err(Error::Git(format!(
                "tag '{name}' already exists; delete it with `git tag -d {name}` or release a different version"
            )));
        }
        self.run(["tag", "-a", name, "-m", message])?;
        Ok(())
    }

    fn tag_exists(&self, name: &str) -> Result<bool> {
        let tag_ref = format!("refs/tags/{name}");
        let args = git_args(&["rev-parse", "--verify", "--quiet", "--end-of-options", &tag_ref]);
        Ok(self.output(&args)?.status.success())
    }

    /// All tag names.
    pub fn tags(&self) -> Result<Vec<String>> {
        // `--no-column`: with `column.ui = always`, git puts several names on a line.
        let out = self.run(["tag", "--list", "--no-column"])?;
        Ok(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
    }

    /// Whether `HEAD` points at a commit.
    fn has_commits(&self) -> Result<bool> {
        let args = git_args(&["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]);
        Ok(self.output(&args)?.status.success())
    }

    /// `path` as an absolute path: relative paths are taken relative to the root.
    fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() { path.to_path_buf() } else { self.root.join(path) }
    }

    /// `path` as a pathspec relative to the root, with `/` separators. Paths
    /// outside the root are passed as given, and git reports them.
    fn pathspec(&self, path: &Path) -> OsString {
        match self.relative(path) {
            Some(rel) => slash_path(&rel),
            None => path.as_os_str().to_os_string(),
        }
    }

    /// `path` relative to the root, if it's inside it. Tries the path as given
    /// first, then canonicalized (resolving symlinks such as macOS's
    /// `/var` → `/private/var`), then with only its parent canonicalized (for
    /// files that no longer exist).
    fn relative(&self, path: &Path) -> Option<PathBuf> {
        if path.is_relative() {
            return Some(normalize(path));
        }
        if let Ok(rel) = path.strip_prefix(&self.root) {
            return Some(normalize(rel));
        }
        if let Ok(canonical) = path.canonicalize().map(simplify_verbatim)
            && let Ok(rel) = canonical.strip_prefix(&self.root)
        {
            return Some(rel.to_path_buf());
        }
        let parent = simplify_verbatim(path.parent()?.canonicalize().ok()?);
        let rel = parent.strip_prefix(&self.root).ok()?;
        Some(normalize(&rel.join(path.file_name()?)))
    }

    /// A root-relative path as git prints it (`/`-separated) → an absolute path
    /// with native separators.
    fn absolute(&self, git_path: &[u8]) -> PathBuf {
        join_slash(self.root.clone(), git_path)
    }
}

/// `base` joined with a `/`-separated relative path, using native separators.
fn join_slash(mut base: PathBuf, rel: &[u8]) -> PathBuf {
    for part in rel.split(|&b| b == b'/').filter(|p| !p.is_empty()) {
        base.push(bytes_to_os(part));
    }
    base
}

/// Drop the `\\?\` prefix that `canonicalize` adds on Windows when the path
/// can be written without it, so the root compares equal to paths built from
/// `current_dir()` and config files. Other platforms are unchanged.
fn simplify_verbatim(path: PathBuf) -> PathBuf {
    if cfg!(windows)
        && let Some(simple) = path.to_str().and_then(strip_verbatim)
    {
        return PathBuf::from(simple);
    }
    path
}

/// `\\?\C:\x` → `C:\x` and `\\?\UNC\server\share` → `\\server\share`; `None`
/// for anything else.
fn strip_verbatim(path: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return Some(format!(r"\\{rest}"));
    }
    let rest = path.strip_prefix(r"\\?\")?;
    match rest.as_bytes() {
        [drive, b':'] | [drive, b':', b'\\', ..] if drive.is_ascii_alphabetic() => Some(rest.to_string()),
        _ => None,
    }
}

fn git_args(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn trim_leading_newlines(mut bytes: &[u8]) -> &[u8] {
    while let [b'\n' | b'\r', rest @ ..] = bytes {
        bytes = rest;
    }
    bytes
}

/// Drop `.` components and resolve `..` lexically.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Join a relative path's components with `/`, as git expects on every
/// platform. An empty path is the root itself: `.`.
fn slash_path(rel: &Path) -> OsString {
    let mut out = OsString::new();
    for (i, component) in rel.components().enumerate() {
        if i > 0 {
            out.push("/");
        }
        out.push(component.as_os_str());
    }
    if out.is_empty() {
        out.push(".");
    }
    out
}

#[cfg(unix)]
fn bytes_to_os(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(bytes).to_os_string()
}

#[cfg(not(unix))]
fn bytes_to_os(bytes: &[u8]) -> OsString {
    // git for Windows prints paths as UTF-8.
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(unix)]
fn os_to_bytes(os: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    os.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn os_to_bytes(os: &std::ffi::OsStr) -> Vec<u8> {
    os.to_string_lossy().into_owned().into_bytes()
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
mod repo_tests;

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

    #[test]
    fn slash_paths() {
        assert_eq!(slash_path(Path::new("")), OsString::from("."));
        assert_eq!(slash_path(&Path::new("a").join("b").join("c.md")), OsString::from("a/b/c.md"));
        assert_eq!(normalize(Path::new("./a/../b/./c")), PathBuf::from("b").join("c"));
        assert_eq!(trim_leading_newlines(b"\n\r\nab\n"), b"ab\n");
        assert_eq!(join_slash(PathBuf::from("base"), b"a//b/"), PathBuf::from("base").join("a").join("b"));
    }

    #[test]
    fn verbatim_prefixes() {
        assert_eq!(strip_verbatim(r"\\?\C:\repo\x").as_deref(), Some(r"C:\repo\x"));
        assert_eq!(strip_verbatim(r"\\?\d:").as_deref(), Some("d:"));
        assert_eq!(strip_verbatim(r"\\?\UNC\server\share\x").as_deref(), Some(r"\\server\share\x"));
        assert_eq!(strip_verbatim(r"\\?\Volume{abc}\x"), None);
        assert_eq!(strip_verbatim(r"C:\repo"), None);
        assert_eq!(strip_verbatim("/home/x"), None);
        // Only Windows rewrites paths.
        let verbatim = PathBuf::from(r"\\?\C:\repo");
        let expected = if cfg!(windows) { PathBuf::from(r"C:\repo") } else { verbatim.clone() };
        assert_eq!(simplify_verbatim(verbatim), expected);
    }
}
