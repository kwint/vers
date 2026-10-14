//! Helpers for integration tests: a temporary git repository with an isolated
//! git config, and a way to run the `vers` binary inside it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin_cmd;

/// 2026-10-14T12:00:00Z, so release dates in tests are stable.
pub const SOURCE_DATE_EPOCH: &str = "1791979200";

pub struct TestRepo {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    pub root: PathBuf,
}

impl TestRepo {
    /// A new, empty git repository on branch `main`.
    pub fn new() -> TestRepo {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo").canonicalize().unwrap_or_else(|_| {
            std::fs::create_dir_all(tmp.path().join("repo")).unwrap();
            tmp.path().join("repo").canonicalize().unwrap()
        });
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let repo = TestRepo { _tmp: tmp, home, root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo
    }

    /// A temporary directory that is *not* a git repository.
    pub fn without_git() -> TestRepo {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("repo")).unwrap();
        let root = tmp.path().join("repo").canonicalize().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        TestRepo { _tmp: tmp, home, root }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Write a file (creating parent directories), relative to the repo root.
    pub fn write(&self, rel: &str, content: &str) -> &Self {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        self
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap()
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    fn isolate(&self, cmd: &mut Command) {
        cmd.env("HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("NO_COLOR", "1")
            .env("SOURCE_DATE_EPOCH", SOURCE_DATE_EPOCH)
            .env_remove("GITHUB_BASE_REF")
            .env_remove("CI_MERGE_REQUEST_DIFF_BASE_SHA")
            .env_remove("VERS_SKIP")
            .env_remove("VISUAL")
            .env_remove("EDITOR");
    }

    /// Run git in the repo root and return stdout. Panics on failure.
    pub fn git(&self, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(&self.root);
        self.isolate(&mut cmd);
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Stage everything and commit with `message`. `date` sets both author and
    /// committer date (e.g. `"2026-01-01T00:00:00Z"`) so add-times are controlled.
    pub fn commit(&self, message: &str, date: Option<&str>) {
        self.git(&["add", "-A"]);
        let mut cmd = Command::new("git");
        cmd.args(["commit", "-q", "--allow-empty", "-m", message]).current_dir(&self.root);
        self.isolate(&mut cmd);
        if let Some(date) = date {
            cmd.env("GIT_AUTHOR_DATE", date).env("GIT_COMMITTER_DATE", date);
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "git commit failed: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// A `vers` command running in `rel` (relative to the repo root).
    pub fn vers_in(&self, rel: &str) -> assert_cmd::Command {
        let mut std_cmd = Command::new(cargo_bin_cmd!("vers").get_program());
        std_cmd.current_dir(self.root.join(rel));
        self.isolate(&mut std_cmd);
        assert_cmd::Command::from_std(std_cmd)
    }

    /// A `vers` command running in the repo root.
    pub fn vers(&self) -> assert_cmd::Command {
        self.vers_in("")
    }
}

pub fn assert_path_eq(a: &Path, b: &Path) {
    assert_eq!(a.canonicalize().unwrap(), b.canonicalize().unwrap());
}
