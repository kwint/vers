//! Config resolution against real git repositories (`repo = "auto"` and
//! `repo_url` from `origin`).
//!
//! The library shells out to git with the process environment, so every test
//! in this binary first calls [`isolate`], which sets an isolated git
//! environment once, before any test spawns git.

mod common;

use std::sync::Once;

use common::TestRepo;
use vers::config::{self, RepoKind, Selection};

static ISOLATE: Once = Once::new();

fn isolate() {
    ISOLATE.call_once(|| {
        let home = std::env::temp_dir().join("vers-config-tests-home");
        std::fs::create_dir_all(&home).unwrap();
        // SAFETY: every test calls this first and `Once` blocks the others
        // until it returns, so no other thread reads the environment meanwhile.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
            std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
            std::env::set_var("GIT_AUTHOR_NAME", "Test");
            std::env::set_var("GIT_AUTHOR_EMAIL", "test@example.com");
            std::env::set_var("GIT_COMMITTER_NAME", "Test");
            std::env::set_var("GIT_COMMITTER_EMAIL", "test@example.com");
        }
    });
}

fn repo_with_origin(origin: &str) -> TestRepo {
    isolate();
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", origin]);
    repo
}

#[test]
fn auto_detects_github_from_ssh_origin() {
    let repo = repo_with_origin("git@github.com:org/proj.git");
    repo.write("vers.toml", "[package]\n");
    let c = config::load(&repo.path("vers.toml")).unwrap();
    assert_eq!(c.changelog.repo, RepoKind::Github);
    assert_eq!(c.changelog.repo_url.as_deref(), Some("https://github.com/org/proj"));
    assert_eq!(c.changelog.compare_url.as_deref(), Some("https://github.com/org/proj/compare/{from}...{to}"));
    assert_eq!(c.refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["issue", "pr"]);
    assert_eq!(c.refs[0].url.as_deref(), Some("https://github.com/org/proj/issues/{id}"));
}

#[test]
fn auto_detects_gitlab_from_https_origin_in_a_nested_package() {
    let repo = repo_with_origin("https://gitlab.com/example/shop.git");
    repo.write("vers.toml", "[version]\ntag_format = \"{name}@v{version}\"\n");
    repo.write("packages/api/vers.toml", "extend = \"../../vers.toml\"\n[package]\n");
    let c = config::load(&repo.path("packages/api/vers.toml")).unwrap();
    assert_eq!(c.changelog.repo, RepoKind::Gitlab);
    assert_eq!(c.changelog.repo_url.as_deref(), Some("https://gitlab.com/example/shop"));
    assert_eq!(c.changelog.release_url.as_deref(), Some("https://gitlab.com/example/shop/-/tags/{tag}"));
    assert_eq!(c.version.tag_format, "{name}@v{version}");
}

#[test]
fn self_hosted_origin_sets_repo_url_but_not_kind() {
    let repo = repo_with_origin("git@gitlab.example.com:team/project.git");
    repo.write("vers.toml", "[package]\n");
    let c = config::load(&repo.path("vers.toml")).unwrap();
    assert_eq!(c.changelog.repo, RepoKind::None);
    assert_eq!(c.changelog.repo_url.as_deref(), Some("https://gitlab.example.com/team/project"));
    assert_eq!(c.changelog.compare_url, None);
    assert!(c.refs.is_empty());
}

#[test]
fn explicit_kind_uses_repo_url_from_origin() {
    let repo = repo_with_origin("git@gitlab.example.com:team/project.git");
    repo.write("vers.toml", "[package]\n[changelog]\nrepo = \"gitlab\"\n");
    let c = config::load(&repo.path("vers.toml")).unwrap();
    assert_eq!(c.changelog.repo, RepoKind::Gitlab);
    assert_eq!(
        c.changelog.compare_url.as_deref(),
        Some("https://gitlab.example.com/team/project/-/compare/{from}...{to}")
    );
    assert_eq!(c.refs[1].url.as_deref(), Some("https://gitlab.example.com/team/project/-/merge_requests/{id}"));
}

#[test]
fn explicit_repo_url_beats_origin() {
    let repo = repo_with_origin("git@github.com:org/proj.git");
    repo.write("vers.toml", "[package]\n[changelog]\nrepo_url = \"https://gitlab.com/mirror/proj\"\n");
    let c = config::load(&repo.path("vers.toml")).unwrap();
    assert_eq!(c.changelog.repo, RepoKind::Gitlab);
    assert_eq!(c.changelog.repo_url.as_deref(), Some("https://gitlab.com/mirror/proj"));
}

#[test]
fn repo_none_still_gets_repo_url_from_origin() {
    let repo = repo_with_origin("https://github.com/org/proj");
    repo.write("vers.toml", "[package]\n[changelog]\nrepo = \"none\"\n");
    let c = config::load(&repo.path("vers.toml")).unwrap();
    assert_eq!(c.changelog.repo, RepoKind::None);
    assert_eq!(c.changelog.repo_url.as_deref(), Some("https://github.com/org/proj"));
    assert!(c.refs.is_empty());
    assert_eq!(c.changelog.compare_url, None);
}

#[test]
fn no_origin_means_no_repo() {
    isolate();
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "upstream", "git@github.com:org/proj.git"]);
    repo.write("vers.toml", "[package]\n");
    let c = config::load(&repo.path("vers.toml")).unwrap();
    assert_eq!(c.changelog.repo, RepoKind::None);
    assert_eq!(c.changelog.repo_url, None);
}

#[test]
fn local_path_origin_has_no_web_url() {
    let repo = repo_with_origin("/srv/git/proj.git");
    repo.write("vers.toml", "[package]\n");
    let c = config::load(&repo.path("vers.toml")).unwrap();
    assert_eq!(c.changelog.repo, RepoKind::None);
    assert_eq!(c.changelog.repo_url, None);
}

#[test]
fn discovery_respects_gitignore_and_git_info_exclude() {
    isolate();
    let repo = TestRepo::new();
    repo.write(".gitignore", "target/\n");
    repo.write(".git/info/exclude", "scratch/\n");
    repo.write("a/vers.toml", "[package]\n");
    repo.write("a/target/x/vers.toml", "[package]\nname = \"ignored\"\n");
    repo.write("scratch/vers.toml", "[package]\nname = \"excluded\"\n");
    let pkgs = config::discover_packages(&repo.root).unwrap();
    assert_eq!(pkgs.iter().map(|c| c.name()).collect::<Vec<_>>(), vec!["a"]);
}

#[test]
fn monorepo_selection_end_to_end() {
    let repo = repo_with_origin("https://gitlab.com/example/shop.git");
    repo.write("vers.toml", "[changelog]\nrepo = \"gitlab\"\n");
    repo.write("packages/api/vers.toml", "extend = \"../../vers.toml\"\n[package]\nname = \"api\"\n");
    repo.write("packages/web/.vers.toml", "extend = \"../../vers.toml\"\n[package]\nname = \"web\"\n");
    repo.write("packages/web/vers.toml", "[package]\nname = \"ignored-because-dot-wins\"\n");

    let all = config::select_packages(&repo.root, &Selection::All, None).unwrap();
    assert_eq!(all.iter().map(|c| c.name()).collect::<Vec<_>>(), vec!["api", "web"]);
    assert!(all.iter().all(|c| c.changelog.repo_url.as_deref() == Some("https://gitlab.com/example/shop")));

    std::fs::create_dir_all(repo.path("packages/web/src")).unwrap();
    let current = config::select_packages(&repo.path("packages/web/src"), &Selection::Current, None).unwrap();
    assert_eq!(current[0].name(), "web");
    assert_eq!(current[0].changelog.path, repo.path("packages/web/CHANGELOG.md"));
}
