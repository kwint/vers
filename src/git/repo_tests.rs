//! Tests against real temporary repositories, with git isolated from the
//! user's configuration.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::Git;

const DEFAULT_DATE: &str = "2026-01-01T00:00:00Z";

fn ts(date: &str) -> i64 {
    date.parse::<jiff::Timestamp>().unwrap().as_second()
}

struct Repo {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    git: Git,
}

fn isolated_env(home: &Path) -> Vec<(OsString, Option<OsString>)> {
    let set = |k: &str, v: &str| (OsString::from(k), Some(OsString::from(v)));
    let mut env = vec![
        (OsString::from("HOME"), Some(home.as_os_str().to_os_string())),
        set("GIT_CONFIG_NOSYSTEM", "1"),
        set("GIT_CONFIG_GLOBAL", "/dev/null"),
        set("GIT_AUTHOR_NAME", "Test"),
        set("GIT_AUTHOR_EMAIL", "test@example.com"),
        set("GIT_COMMITTER_NAME", "Test"),
        set("GIT_COMMITTER_EMAIL", "test@example.com"),
        set("GIT_AUTHOR_DATE", DEFAULT_DATE),
        set("GIT_COMMITTER_DATE", DEFAULT_DATE),
    ];
    for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_CEILING_DIRECTORIES"] {
        env.push((OsString::from(var), None));
    }
    env
}

impl Repo {
    /// A new repository on branch `main` with no commits.
    fn new() -> Repo {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let root = dir.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(dir.join("home")).unwrap();
        let git = Git { root, env: isolated_env(&dir.join("home")) };
        let repo = Repo { _tmp: tmp, dir, git };
        repo.sh(&["init", "-q", "-b", "main"]);
        repo
    }

    fn root(&self) -> &Path {
        self.git.root()
    }

    fn path(&self, rel: &str) -> PathBuf {
        let mut path = self.root().to_path_buf();
        path.extend(rel.split('/'));
        path
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn remove(&self, rel: &str) {
        std::fs::remove_file(self.path(rel)).unwrap();
    }

    /// Run git with the isolated environment (and optional fixed date);
    /// returns stdout and panics on failure.
    fn sh_at(&self, args: &[&str], date: Option<&str>) -> String {
        let mut cmd = self.git.command();
        cmd.args(args);
        if let Some(date) = date {
            cmd.env("GIT_AUTHOR_DATE", date).env("GIT_COMMITTER_DATE", date);
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn sh(&self, args: &[&str]) -> String {
        self.sh_at(args, None)
    }

    /// Stage everything and commit at `date`.
    fn commit_all(&self, message: &str, date: &str) {
        self.sh(&["add", "-A"]);
        self.sh_at(&["commit", "-q", "--allow-empty", "-m", message], Some(date));
    }

    /// `git status --porcelain` lines, sorted.
    fn status(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .sh(&["-c", "core.quotepath=off", "status", "--porcelain", "--untracked-files=all"])
            .lines()
            .map(String::from)
            .collect();
        lines.sort();
        lines
    }

    /// Files changed by the HEAD commit, as `<status>\t<path>` lines, sorted.
    fn head_changes(&self) -> Vec<String> {
        let out = self.sh(&["-c", "core.quotepath=off", "show", "--no-renames", "--name-status", "--format=", "HEAD"]);
        let mut lines: Vec<String> = out.lines().filter(|l| !l.is_empty()).map(String::from).collect();
        lines.sort();
        lines
    }
}

fn sorted(mut v: Vec<PathBuf>) -> Vec<PathBuf> {
    v.sort();
    v
}

// --- is_shallow -------------------------------------------------------------

#[test]
fn normal_repo_is_not_shallow() {
    let repo = Repo::new();
    repo.write("a.txt", "a");
    repo.commit_all("one", DEFAULT_DATE);
    assert!(!repo.git.is_shallow().unwrap());
}

#[test]
fn depth_one_clone_is_shallow() {
    let repo = Repo::new();
    repo.write("a.txt", "a");
    repo.commit_all("one", "2026-01-01T00:00:00Z");
    repo.write("b.txt", "b");
    repo.commit_all("two", "2026-01-02T00:00:00Z");

    let src = repo.root().to_string_lossy().replace('\\', "/");
    let src = src.trim_start_matches("//?/");
    let url = if src.starts_with('/') { format!("file://{src}") } else { format!("file:///{src}") };
    let clone_dir = repo.dir.join("clone");
    repo.sh(&["clone", "-q", "--depth", "1", &url, clone_dir.to_str().unwrap()]);
    let clone = Git { root: clone_dir.canonicalize().unwrap(), env: repo.git.env.clone() };
    assert!(clone.is_shallow().unwrap());

    // History is cut off: a.txt appears to be added by the only commit there is.
    let times = clone.added_times(&[PathBuf::from(".")]).unwrap();
    assert_eq!(times[&clone.root().join("a.txt")], ts("2026-01-02T00:00:00Z"));
}

// --- added_times --------------------------------------------------------------

#[test]
fn added_times_keep_first_add_across_commits() {
    let repo = Repo::new();
    repo.write("README.md", "readme");
    repo.write("changes/1.added.md", "First.");
    repo.commit_all("one", "2026-01-01T10:00:00Z");

    repo.write("changes/2.fixed.md", "Second.");
    repo.write("changes/1.added.md", "First, edited.");
    repo.commit_all("two", "2026-02-01T10:00:00Z");

    repo.write("changes/3.removed.md", "Third.");
    repo.write("changes/2.fixed.md", "Second, edited.");
    repo.commit_all("three", "2026-03-01T10:00:00Z");

    // Staged but not committed, and untracked: both absent.
    repo.write("changes/staged.md", "Staged.");
    repo.sh(&["add", "changes/staged.md"]);
    repo.write("changes/untracked.md", "Untracked.");

    let times = repo.git.added_times(&[repo.path("changes")]).unwrap();
    let expected: HashMap<PathBuf, i64> = [
        (repo.path("changes/1.added.md"), ts("2026-01-01T10:00:00Z")),
        (repo.path("changes/2.fixed.md"), ts("2026-02-01T10:00:00Z")),
        (repo.path("changes/3.removed.md"), ts("2026-03-01T10:00:00Z")),
    ]
    .into_iter()
    .collect();
    assert_eq!(times, expected);
}

#[test]
fn added_times_use_author_time_not_committer_time() {
    let repo = Repo::new();
    repo.write("changes/a.md", "A.");
    repo.sh(&["add", "-A"]);
    let mut cmd = repo.git.command();
    cmd.args(["commit", "-q", "-m", "one"])
        .env("GIT_AUTHOR_DATE", "2025-05-05T05:05:05Z")
        .env("GIT_COMMITTER_DATE", "2026-06-06T06:06:06Z");
    assert!(cmd.output().unwrap().status.success());
    let times = repo.git.added_times(&[repo.path("changes")]).unwrap();
    assert_eq!(times[&repo.path("changes/a.md")], ts("2025-05-05T05:05:05Z"));
}

#[test]
fn added_times_cover_subdirs_several_dirs_and_odd_names() {
    let repo = Repo::new();
    repo.write("api/changes/nested/deep.added.md", "Deep.");
    repo.write("api/changes/héllo wörld ✨.fixed.md", "Unicode.");
    repo.write("api/changes/[glob]*.md", "Glob chars.");
    repo.write("web/changes/1.added.md", "Web.");
    repo.write("other/changes/ignored.md", "Not asked for.");
    repo.write("api/src/lib.rs", "code");
    repo.commit_all("one", "2026-04-01T00:00:00Z");

    let dirs = [
        repo.path("api/changes"),
        PathBuf::from("web").join("changes"), // relative to the root, native separators
        repo.path("missing/changes"),         // doesn't exist
    ];
    let times = repo.git.added_times(&dirs).unwrap();
    let t = ts("2026-04-01T00:00:00Z");
    let expected: HashMap<PathBuf, i64> = [
        (repo.path("api/changes/nested/deep.added.md"), t),
        (repo.path("api/changes/héllo wörld ✨.fixed.md"), t),
        (repo.path("api/changes/[glob]*.md"), t),
        (repo.path("web/changes/1.added.md"), t),
    ]
    .into_iter()
    .collect();
    assert_eq!(times, expected);
    for path in times.keys() {
        assert!(path.is_absolute() && path.exists(), "{}", path.display());
    }
}

#[test]
fn added_times_see_renamed_files_as_new_adds() {
    let repo = Repo::new();
    repo.write("changes/old.md", "Same content for rename detection.");
    repo.commit_all("one", "2026-01-01T00:00:00Z");
    repo.sh(&["mv", "changes/old.md", "changes/new.md"]);
    repo.commit_all("rename", "2026-02-01T00:00:00Z");

    let times = repo.git.added_times(&[repo.path("changes")]).unwrap();
    // Deleted paths may appear too; callers only look up files that exist.
    assert_eq!(times[&repo.path("changes/old.md")], ts("2026-01-01T00:00:00Z"));
    assert_eq!(times[&repo.path("changes/new.md")], ts("2026-02-01T00:00:00Z"));
}

#[test]
fn added_times_keep_oldest_add_of_a_readded_file() {
    let repo = Repo::new();
    repo.write("changes/a.md", "A.");
    repo.commit_all("add", "2026-01-01T00:00:00Z");
    repo.remove("changes/a.md");
    repo.commit_all("delete", "2026-02-01T00:00:00Z");
    repo.write("changes/a.md", "A again.");
    repo.commit_all("re-add", "2026-03-01T00:00:00Z");

    let times = repo.git.added_times(&[repo.path("changes")]).unwrap();
    assert_eq!(times[&repo.path("changes/a.md")], ts("2026-01-01T00:00:00Z"));
}

#[test]
fn added_times_include_files_from_merged_branches() {
    let repo = Repo::new();
    repo.write("README.md", "r");
    repo.commit_all("init", "2026-01-01T00:00:00Z");
    repo.sh(&["switch", "-q", "-c", "feature"]);
    repo.write("changes/feature.md", "Feature.");
    repo.commit_all("feature", "2026-01-05T00:00:00Z");
    repo.sh(&["switch", "-q", "main"]);
    repo.write("changes/main.md", "Main.");
    repo.commit_all("main", "2026-01-10T00:00:00Z");
    repo.sh_at(&["merge", "-q", "--no-ff", "-m", "merge", "feature"], Some("2026-01-20T00:00:00Z"));

    let times = repo.git.added_times(&[repo.path("changes")]).unwrap();
    assert_eq!(times.len(), 2);
    assert_eq!(times[&repo.path("changes/feature.md")], ts("2026-01-05T00:00:00Z"));
    assert_eq!(times[&repo.path("changes/main.md")], ts("2026-01-10T00:00:00Z"));
}

#[test]
fn added_times_for_the_whole_repo_and_no_dirs() {
    let repo = Repo::new();
    repo.write("a.md", "a");
    repo.write("sub/b.md", "b");
    repo.commit_all("one", "2026-01-01T00:00:00Z");
    assert!(repo.git.added_times(&[]).unwrap().is_empty());
    let times = repo.git.added_times(&[repo.root().to_path_buf()]).unwrap();
    assert_eq!(times.len(), 2);
    assert!(times.contains_key(&repo.path("sub/b.md")));
}

#[test]
fn added_times_in_repo_without_commits_is_empty() {
    let repo = Repo::new();
    repo.write("changes/a.md", "A.");
    repo.sh(&["add", "-A"]);
    assert!(repo.git.added_times(&[repo.path("changes")]).unwrap().is_empty());
}

// --- changed_files / added_or_modified_files ---------------------------------

/// main: a, b, c. feature (from there): modifies a, adds d, deletes c,
/// renames b → e. main then adds main-only.md, which must not show up.
fn branched_repo() -> Repo {
    let repo = Repo::new();
    repo.write("a.txt", "a");
    repo.write("b.txt", "b content long enough to be detected as a rename");
    repo.write("pkg/c.txt", "c");
    repo.commit_all("base", "2026-01-01T00:00:00Z");
    repo.sh(&["switch", "-q", "-c", "feature"]);
    repo.write("a.txt", "a changed");
    repo.write("pkg/changes/d ünï.md", "d");
    repo.remove("pkg/c.txt");
    repo.sh(&["mv", "b.txt", "e.txt"]);
    repo.commit_all("feature", "2026-01-02T00:00:00Z");
    repo.sh(&["switch", "-q", "main"]);
    repo.write("main-only.md", "m");
    repo.commit_all("main moves on", "2026-01-03T00:00:00Z");
    repo.sh(&["switch", "-q", "feature"]);
    repo
}

#[test]
fn changed_files_since_merge_base() {
    let repo = branched_repo();
    let changed = sorted(repo.git.changed_files("main").unwrap());
    let expected = sorted(vec![repo.path("a.txt"), repo.path("b.txt"), repo.path("e.txt"), repo.path("pkg/c.txt"), {
        repo.path("pkg/changes/d ünï.md")
    }]);
    assert_eq!(changed, expected);
}

#[test]
fn added_or_modified_files_exclude_deletions() {
    let repo = branched_repo();
    let changed = sorted(repo.git.added_or_modified_files("main").unwrap());
    let expected = sorted(vec![repo.path("a.txt"), repo.path("e.txt"), repo.path("pkg/changes/d ünï.md")]);
    assert_eq!(changed, expected);
}

#[test]
fn changed_files_against_a_commit_sha_and_no_changes() {
    let repo = branched_repo();
    let head = repo.sh(&["rev-parse", "HEAD"]);
    assert!(repo.git.changed_files(head.trim()).unwrap().is_empty());
    let base = repo.sh(&["rev-parse", "main~1"]);
    assert_eq!(repo.git.changed_files(base.trim()).unwrap().len(), 5);
}

#[test]
fn changed_files_ignore_uncommitted_changes() {
    let repo = branched_repo();
    repo.write("uncommitted.txt", "x");
    repo.sh(&["add", "-A"]);
    assert!(!repo.git.changed_files("main").unwrap().contains(&repo.path("uncommitted.txt")));
}

#[test]
fn unresolvable_base_errors_with_hint() {
    let repo = branched_repo();
    let err = repo.git.changed_files("origin/main").unwrap_err().to_string();
    assert!(err.contains("'origin/main'"), "{err}");
    assert!(err.contains("git fetch origin main"), "{err}");
    assert!(err.contains("fetch-depth: 0"), "{err}");

    let err = repo.git.added_or_modified_files("origin/HEAD").unwrap_err().to_string();
    assert!(err.contains("'origin/HEAD'") && err.contains("set-head"), "{err}");

    let err = repo.git.changed_files("").unwrap_err().to_string();
    assert!(err.contains("--base"), "{err}");

    // An option-looking ref is not taken as an option.
    assert!(repo.git.changed_files("--all").is_err());
}

#[test]
fn unrelated_histories_error_mentions_common_ancestor() {
    let repo = branched_repo();
    repo.sh(&["switch", "-q", "--orphan", "lonely"]);
    repo.write("x.txt", "x");
    repo.commit_all("orphan", "2026-01-04T00:00:00Z");
    let err = repo.git.changed_files("main").unwrap_err().to_string();
    assert!(err.contains("main"), "{err}");
}

// --- is_tracked / rm ------------------------------------------------------------

#[test]
fn tracked_status() {
    let repo = Repo::new();
    repo.write(".gitignore", "*.log\n");
    repo.write("committed.md", "c");
    repo.write("dir/nested.md", "n");
    repo.commit_all("one", DEFAULT_DATE);
    repo.write("staged.md", "s");
    repo.sh(&["add", "staged.md"]);
    repo.write("untracked.md", "u");
    repo.write("ignored.log", "i");
    repo.write("[x].md", "glob");

    assert!(repo.git.is_tracked(&repo.path("committed.md")).unwrap());
    assert!(repo.git.is_tracked(&repo.path("dir/nested.md")).unwrap());
    assert!(repo.git.is_tracked(&PathBuf::from("dir").join("nested.md")).unwrap());
    assert!(repo.git.is_tracked(&repo.path("staged.md")).unwrap());
    assert!(!repo.git.is_tracked(&repo.path("untracked.md")).unwrap());
    assert!(!repo.git.is_tracked(&repo.path("ignored.log")).unwrap());
    assert!(!repo.git.is_tracked(&repo.path("does-not-exist.md")).unwrap());
    // `[x].md` is untracked; as a glob it would match nothing else either, but
    // literal pathspecs must not match `x.md`-style names.
    repo.write("x.md", "x");
    repo.sh(&["add", "x.md"]);
    assert!(!repo.git.is_tracked(&repo.path("[x].md")).unwrap());
}

#[test]
fn rm_deletes_and_stages() {
    let repo = Repo::new();
    repo.write("changes/1.added.md", "One.");
    repo.write("changes/sub/2 two.fixed.md", "Two.");
    repo.write("keep.md", "k");
    repo.commit_all("one", DEFAULT_DATE);

    repo.git.rm(&[repo.path("changes/1.added.md"), repo.path("changes/sub/2 two.fixed.md")]).unwrap();
    assert!(!repo.path("changes/1.added.md").exists());
    assert!(!repo.path("changes/sub/2 two.fixed.md").exists());
    assert_eq!(repo.status(), vec!["D  \"changes/sub/2 two.fixed.md\"", "D  changes/1.added.md"]);
    repo.git.rm(&[]).unwrap();

    let err = repo.git.rm(&[repo.path("never-existed.md")]).unwrap_err().to_string();
    assert!(err.contains("never-existed.md"), "{err}");
}

// --- commit_paths ----------------------------------------------------------------

#[test]
fn commit_paths_commits_only_the_given_paths() {
    let repo = Repo::new();
    repo.write("CHANGELOG.md", "# Changelog\n");
    repo.write("Cargo.toml", "version = \"1.0.0\"\n");
    repo.write("changes/1.added.md", "One.");
    repo.write("changes/2.fixed.md", "Two.");
    repo.write("src/staged.rs", "old");
    repo.write("src/unstaged.rs", "old");
    repo.commit_all("init", DEFAULT_DATE);

    // Unrelated work in progress: staged, unstaged and untracked.
    repo.write("src/staged.rs", "staged change");
    repo.sh(&["add", "src/staged.rs"]);
    repo.write("src/unstaged.rs", "unstaged change");
    repo.write("notes.txt", "untracked");
    // A path to commit that also has unrelated staged content gets its
    // working tree version.
    repo.write("Cargo.toml", "version = \"1.0.0\" # staged\n");
    repo.sh(&["add", "Cargo.toml"]);

    // What a release does: changelog and version file, one fragment removed
    // with `git rm`, one deleted only from the working tree, and a new file.
    repo.write("CHANGELOG.md", "# Changelog\n\n## 1.1.0\n");
    repo.write("Cargo.toml", "version = \"1.1.0\"\n");
    repo.git.rm(&[repo.path("changes/1.added.md")]).unwrap();
    repo.remove("changes/2.fixed.md");
    repo.write("pkg/VERSION", "1.1.0\n");

    let paths = vec![
        repo.path("CHANGELOG.md"),
        repo.path("Cargo.toml"),
        repo.path("changes/1.added.md"),
        repo.path("changes/2.fixed.md"),
        PathBuf::from("pkg").join("VERSION"),
    ];
    repo.git.commit_paths(&paths, "Release pkg 1.1.0").unwrap();

    assert_eq!(repo.sh(&["log", "-1", "--format=%s"]).trim(), "Release pkg 1.1.0");
    assert_eq!(
        repo.head_changes(),
        vec!["A\tpkg/VERSION", "D\tchanges/1.added.md", "D\tchanges/2.fixed.md", "M\tCHANGELOG.md", "M\tCargo.toml"]
    );
    assert_eq!(repo.sh(&["show", "HEAD:Cargo.toml"]), "version = \"1.1.0\"\n");
    // Everything else is exactly as it was.
    assert_eq!(repo.status(), vec![" M src/unstaged.rs", "?? notes.txt", "M  src/staged.rs"]);
    assert_eq!(std::fs::read_to_string(repo.path("src/unstaged.rs")).unwrap(), "unstaged change");
}

#[test]
fn commit_paths_with_odd_names() {
    let repo = Repo::new();
    repo.write("README.md", "r");
    repo.commit_all("init", DEFAULT_DATE);
    repo.write("ünïcödé dir/[v]*.md", "glob");
    repo.write("v.md", "must stay uncommitted");
    repo.git.commit_paths(&[repo.path("ünïcödé dir/[v]*.md")], "odd").unwrap();
    assert_eq!(repo.head_changes(), vec!["A\tünïcödé dir/[v]*.md"]);
    assert_eq!(repo.status(), vec!["?? v.md"]);
}

#[test]
fn commit_paths_as_first_commit() {
    let repo = Repo::new();
    repo.write("CHANGELOG.md", "# Changelog\n");
    repo.write("other.md", "o");
    repo.git.commit_paths(&[repo.path("CHANGELOG.md")], "first").unwrap();
    assert_eq!(repo.head_changes(), vec!["A\tCHANGELOG.md"]);
    assert_eq!(repo.status(), vec!["?? other.md"]);
}

#[test]
fn commit_paths_errors() {
    let repo = Repo::new();
    repo.write("a.md", "a");
    repo.commit_all("init", DEFAULT_DATE);
    assert!(repo.git.commit_paths(&[], "empty").unwrap_err().to_string().contains("nothing to commit"));
    // A path git has never heard of.
    let err = repo.git.commit_paths(&[repo.path("ghost.md")], "ghost").unwrap_err().to_string();
    assert!(err.contains("ghost.md"), "{err}");
}

// --- tags --------------------------------------------------------------------------

#[test]
fn annotated_tags() {
    let repo = Repo::new();
    assert!(repo.git.tags().unwrap().is_empty());
    repo.write("a.md", "a");
    repo.commit_all("init", DEFAULT_DATE);

    repo.git.tag("v1.0.0", "Release 1.0.0").unwrap();
    repo.git.tag("api@v0.2.0", "Release api 0.2.0").unwrap();
    assert_eq!(repo.sh(&["cat-file", "-t", "refs/tags/v1.0.0"]).trim(), "tag");
    assert_eq!(repo.sh(&["tag", "-l", "--format=%(contents:subject)", "v1.0.0"]).trim(), "Release 1.0.0");
    assert_eq!(repo.sh(&["rev-parse", "v1.0.0^{commit}"]), repo.sh(&["rev-parse", "HEAD"]));

    let mut tags = repo.git.tags().unwrap();
    tags.sort();
    assert_eq!(tags, vec!["api@v0.2.0", "v1.0.0"]);

    let err = repo.git.tag("v1.0.0", "again").unwrap_err().to_string();
    assert!(err.contains("'v1.0.0' already exists"), "{err}");
    // Lightweight tags count too.
    repo.sh(&["tag", "light"]);
    assert!(repo.git.tag("light", "x").is_err());
    // Invalid names are reported by git.
    assert!(repo.git.tag("bad name", "x").is_err());
}

#[test]
fn tag_without_commits_fails() {
    let repo = Repo::new();
    assert!(repo.git.tag("v1.0.0", "x").is_err());
}

// --- paths -------------------------------------------------------------------------

#[test]
fn pathspecs_are_root_relative_with_slashes() {
    let repo = Repo::new();
    let root = repo.root();
    assert_eq!(repo.git.pathspec(&root.join("a").join("b.md")), OsString::from("a/b.md"));
    assert_eq!(repo.git.pathspec(root), OsString::from("."));
    assert_eq!(repo.git.pathspec(&PathBuf::from("a").join(".").join("b.md")), OsString::from("a/b.md"));
    // Not yet existing file in an existing dir, reached through a non-canonical path.
    std::fs::create_dir_all(root.join("dir")).unwrap();
    let odd = root.join("dir").join("..").join("dir").join("new.md");
    assert_eq!(repo.git.pathspec(&odd), OsString::from("dir/new.md"));
    assert_eq!(repo.git.absolute(b"dir/new.md"), root.join("dir").join("new.md"));
}

#[test]
fn discover_finds_root_from_subdir() {
    let repo = Repo::new();
    repo.write("sub/dir/file.md", "x");
    let found = Git::discover(&repo.path("sub/dir")).unwrap().unwrap();
    assert_eq!(found.root(), repo.root());
    assert!(Git::discover(&repo.dir.join("home")).unwrap().is_none_or(|g| g.root() != repo.root()));
}

#[test]
fn run_reports_failing_command() {
    let repo = Repo::new();
    let err = repo.git.run(["rev-parse", "--verify", "nope"]).unwrap_err().to_string();
    assert!(err.starts_with("git: `git rev-parse --verify nope` failed"), "{err}");
}
