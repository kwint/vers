//! Integration tests for `vers release`.

mod common;

use common::TestRepo;
use predicates::prelude::*;

const HEADER: &str = "\
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
";

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

/// A changelog with the standard header and the given released versions
/// (newest first), each with one "Fixed" entry.
fn changelog_with(versions: &[&str]) -> String {
    let mut text = HEADER.to_string();
    for v in versions {
        text.push_str(&format!("\n## [{v}] - 2026-01-01\n\n### Fixed\n\n- Something in {v}.\n"));
    }
    text
}

/// A single-package repo without a remote (no links), committed.
fn single(name: &str, changelog_versions: &[&str], version_file: Option<&str>) -> TestRepo {
    let repo = TestRepo::new();
    let mut config = format!("[package]\nname = \"{name}\"\n");
    if let Some(v) = version_file {
        config.push_str("\n[[version.files]]\npath = \"VERSION\"\n");
        repo.write("VERSION", &format!("{v}\n"));
    }
    repo.write("vers.toml", &config);
    repo.write("CHANGELOG.md", &changelog_with(changelog_versions));
    repo.write("changes/.gitkeep", "");
    repo.commit("Set up vers", Some("2026-01-01T00:00:00Z"));
    repo
}

/// The tutorial's monorepo: shared root config, `api` (Cargo.toml 1.4.0) and
/// `web` (package.json 0.3.0), with a GitLab origin.
fn monorepo() -> TestRepo {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "https://gitlab.com/example/shop.git"]);
    repo.write(
        "vers.toml",
        "[changelog]\nrepo = \"gitlab\"\nrepo_url = \"https://gitlab.com/example/shop\"\n\n[version]\ntag_format = \"{name}@v{version}\"\n",
    );
    repo.write("packages/api/Cargo.toml", "[package]\nname = \"api\"\nversion = \"1.4.0\"\n");
    repo.write(
        "packages/api/vers.toml",
        "extend = \"../../vers.toml\"\n\n[package]\nname = \"api\"\n\n[[version.files]]\npath = \"Cargo.toml\"\nkey = \"package.version\"\n",
    );
    repo.write("packages/web/package.json", "{ \"name\": \"web\", \"version\": \"0.3.0\" }\n");
    repo.write(
        "packages/web/vers.toml",
        "extend = \"../../vers.toml\"\n\n[package]\nname = \"web\"\n\n[[version.files]]\npath = \"package.json\"\nkey = \"version\"\n",
    );
    for pkg in ["api", "web"] {
        repo.write(&format!("packages/{pkg}/CHANGELOG.md"), HEADER);
        repo.write(&format!("packages/{pkg}/changes/.gitkeep"), "");
    }
    repo.commit("Set up vers", Some("2026-01-01T00:00:00Z"));
    repo
}

// ---------------------------------------------------------------------------
// The tutorials and README
// ---------------------------------------------------------------------------

#[test]
fn first_release_tutorial() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "https://github.com/example/greeter.git"]);
    repo.write("VERSION", "0.1.0\n");
    repo.write("vers.toml", "[package]\nname = \"greeter\"\n\n[[version.files]]\npath = \"VERSION\"\n");
    repo.write("CHANGELOG.md", HEADER);
    repo.write("changes/.gitkeep", "");
    repo.commit("Set up vers", Some("2026-01-01T00:00:00Z"));
    repo.write("changes/greet-people-by-name.added.md", "Greet people by name.\n");
    repo.write("changes/7.fixed.md", "Don't crash on an empty name.\n");

    repo.vers()
        .arg("release")
        .assert()
        .success()
        .stderr("")
        .stdout("Releasing greeter 0.1.0 (from VERSION)\n  Updated CHANGELOG.md\n  Removed 2 fragments\n");
    let expected = format!(
        "{HEADER}
## [0.1.0] - 2026-10-14

### Added

- Greet people by name.

### Fixed

- Don't crash on an empty name. ([#7](https://github.com/example/greeter/issues/7))

[Unreleased]: https://github.com/example/greeter/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/example/greeter/releases/tag/v0.1.0
"
    );
    assert_eq!(repo.read("CHANGELOG.md"), expected);
    assert!(!repo.exists("changes/7.fixed.md"));
    assert!(!repo.exists("changes/greet-people-by-name.added.md"));
    assert!(repo.exists("changes/.gitkeep"));
    assert_eq!(repo.read("VERSION"), "0.1.0\n");
    repo.commit("Release 0.1.0", None);

    repo.write("changes/9.fixed.md", "Trim whitespace around names.\n");
    repo.vers().arg("release").assert().success().stderr("").stdout(
        "Releasing greeter 0.1.1 (inferred: patch)\n  Updated CHANGELOG.md\n  Updated VERSION (0.1.0 → 0.1.1)\n  Removed 1 fragment\n",
    );
    assert_eq!(repo.read("VERSION"), "0.1.1\n");
    let changelog = repo.read("CHANGELOG.md");
    assert!(changelog.contains(
        "## [Unreleased]\n\n## [0.1.1] - 2026-10-14\n\n### Fixed\n\n- Trim whitespace around names. ([#9](https://github.com/example/greeter/issues/9))\n\n## [0.1.0] - 2026-10-14\n"
    ));
    assert!(changelog.ends_with(
        "[Unreleased]: https://github.com/example/greeter/compare/v0.1.1...HEAD\n\
         [0.1.1]: https://github.com/example/greeter/compare/v0.1.0...v0.1.1\n\
         [0.1.0]: https://github.com/example/greeter/releases/tag/v0.1.0\n"
    ));
}

#[test]
fn monorepo_tutorial_release_current_then_all() {
    let repo = monorepo();
    repo.write("packages/api/changes/mr-12.added.md", "Add the /orders endpoint.\n");
    repo.write("packages/web/changes/30.fixed.md", "Fix the cart total rounding.\n");
    repo.write("packages/api/changes/return-404-instead-of-400-for.changed.md", "Return 404.\n");

    repo.vers_in("packages/web").arg("release").assert().success().stderr("").stdout(
        "Releasing web 0.3.1 (inferred: patch)\n  Updated CHANGELOG.md\n  Updated package.json (0.3.0 → 0.3.1)\n  Removed 1 fragment\n",
    );
    assert_eq!(repo.read("packages/web/package.json"), "{ \"name\": \"web\", \"version\": \"0.3.1\" }\n");
    assert!(repo.read("packages/web/CHANGELOG.md").contains(
        "[Unreleased]: https://gitlab.com/example/shop/-/compare/web@v0.3.1...HEAD\n[0.3.1]: https://gitlab.com/example/shop/-/tags/web@v0.3.1\n"
    ));

    repo.vers().args(["release", "--all"]).assert().success().stderr("").stdout(
        "Releasing api 1.5.0 (inferred: minor)\n  Updated packages/api/CHANGELOG.md\n  Updated packages/api/Cargo.toml (1.4.0 → 1.5.0)\n  Removed 2 fragments\nSkipped web (no fragments)\n",
    );
    assert_eq!(repo.read("packages/api/Cargo.toml"), "[package]\nname = \"api\"\nversion = \"1.5.0\"\n");
    assert!(repo.read("packages/api/CHANGELOG.md").contains(
        "## [1.5.0] - 2026-10-14\n\n### Added\n\n- Add the /orders endpoint. ([!12](https://gitlab.com/example/shop/-/merge_requests/12))\n\n### Changed\n\n- Return 404.\n"
    ));
}

#[test]
fn release_named_package_from_root() {
    let repo = monorepo();
    repo.write("packages/web/changes/30.fixed.md", "Fix the cart total rounding.\n");
    repo.write("packages/api/changes/1.fixed.md", "Fix api.\n");
    repo.vers().args(["release", "web"]).assert().success().stdout(
        "Releasing web 0.3.1 (inferred: patch)\n  Updated packages/web/CHANGELOG.md\n  Updated packages/web/package.json (0.3.0 → 0.3.1)\n  Removed 1 fragment\n",
    );
    // api is untouched.
    assert!(repo.exists("packages/api/changes/1.fixed.md"));
    assert_eq!(repo.read("packages/api/CHANGELOG.md"), HEADER);
}

#[test]
fn readme_example() {
    let repo = TestRepo::new();
    repo.write(
        "vers.toml",
        "[package]\nname = \"app\"\n\n[changelog]\nrepo = \"gitlab\"\nrepo_url = \"https://gitlab.com/acme/app\"\n\n[[version.files]]\npath = \"Cargo.toml\"\nkey = \"package.version\"\n",
    );
    repo.write("Cargo.toml", "[package]\nname = \"app\"\nversion = \"1.2.0\"\n");
    repo.write(
        "CHANGELOG.md",
        &format!("{HEADER}\n## [1.2.0] - 2026-09-30\n\n### Added\n\n- First.\n\n[Unreleased]: https://gitlab.com/acme/app/-/compare/v1.2.0...HEAD\n[1.2.0]: https://gitlab.com/acme/app/-/tags/v1.2.0\n"),
    );
    repo.commit("init", Some("2026-01-01T00:00:00Z"));
    repo.write("changes/mr-45.added.md", "Log in with OAuth.\n");
    repo.write("changes/123.fixed.md", "Don't crash when the config file is empty.\n");
    repo.vers().arg("release").assert().success().stderr("").stdout(
        "Releasing app 1.3.0 (inferred: minor)\n  Updated CHANGELOG.md\n  Updated Cargo.toml (1.2.0 → 1.3.0)\n  Removed 2 fragments\n",
    );
    assert_eq!(
        repo.read("CHANGELOG.md"),
        format!(
            "{HEADER}
## [1.3.0] - 2026-10-14

### Added

- Log in with OAuth. ([!45](https://gitlab.com/acme/app/-/merge_requests/45))

### Fixed

- Don't crash when the config file is empty. ([#123](https://gitlab.com/acme/app/-/issues/123))

## [1.2.0] - 2026-09-30

### Added

- First.

[Unreleased]: https://gitlab.com/acme/app/-/compare/v1.3.0...HEAD
[1.3.0]: https://gitlab.com/acme/app/-/compare/v1.2.0...v1.3.0
[1.2.0]: https://gitlab.com/acme/app/-/tags/v1.2.0
"
        )
    );
}

// ---------------------------------------------------------------------------
// Choosing the version
// ---------------------------------------------------------------------------

fn release_version(changelog: &[&str], fragments: &[(&str, &str)], extra_args: &[&str]) -> (String, String) {
    let repo = single("app", changelog, None);
    for (name, body) in fragments {
        repo.write(&format!("changes/{name}"), body);
    }
    let out = repo.vers().arg("release").args(extra_args).output().unwrap();
    assert!(out.status.success(), "release failed: {}", stderr(&out));
    let first = stdout(&out).lines().next().unwrap().to_string();
    (first, repo.read("CHANGELOG.md"))
}

/// Current version, fragments (file name, body), expected first output line.
type BumpCase<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);

#[test]
fn inferred_bumps() {
    let cases: &[BumpCase] = &[
        ("1.4.2", &[("a.fixed.md", "Fix.\n")], "Releasing app 1.4.3 (inferred: patch)"),
        ("1.4.2", &[("a.fixed.md", "Fix.\n"), ("b.added.md", "Add.\n")], "Releasing app 1.5.0 (inferred: minor)"),
        ("1.4.2", &[("a.added.md", "Add.\n"), ("b.removed.md", "Rm.\n")], "Releasing app 2.0.0 (inferred: major)"),
        ("1.4.2", &[("a.changed.md", "---\nbreaking: true\n---\nBreak.\n")], "Releasing app 2.0.0 (inferred: major)"),
        // 0.x: each bump moves down one level.
        ("0.4.2", &[("a.fixed.md", "Fix.\n")], "Releasing app 0.4.3 (inferred: patch)"),
        ("0.4.2", &[("a.fixed.md", "Fix.\n"), ("b.added.md", "Add.\n")], "Releasing app 0.4.3 (inferred: minor)"),
        ("0.4.2", &[("a.added.md", "Add.\n"), ("b.removed.md", "Rm.\n")], "Releasing app 0.5.0 (inferred: major)"),
        ("0.4.2", &[("a.changed.md", "---\nbreaking: true\n---\nBreak.\n")], "Releasing app 0.5.0 (inferred: major)"),
    ];
    for (current, fragments, expected) in cases {
        let (line, _) = release_version(&[current], fragments, &[]);
        assert_eq!(&line, expected, "from {current} with {fragments:?}");
    }
}

#[test]
fn breaking_entry_is_prefixed() {
    let (_, changelog) = release_version(&["1.0.0"], &[("a.changed.md", "---\nbreaking: true\n---\nBreak.\n")], &[]);
    assert!(changelog.contains("### Changed\n\n- **Breaking:** Break.\n"));
}

#[test]
fn initial_version_without_current_or_files() {
    let (line, changelog) = release_version(&[], &[("a.added.md", "Add.\n")], &[]);
    assert_eq!(line, "Releasing app 0.1.0 (initial)");
    assert!(changelog.contains("## [0.1.0] - 2026-10-14\n\n### Added\n\n- Add.\n"));
}

#[test]
fn bump_and_version_from_command_line() {
    let (line, _) = release_version(&["1.4.2"], &[("a.fixed.md", "Fix.\n")], &["--bump", "major"]);
    assert_eq!(line, "Releasing app 2.0.0 (from command line)");
    let (line, _) = release_version(&["0.4.2"], &[("a.fixed.md", "Fix.\n")], &["--version", "1.0.0"]);
    assert_eq!(line, "Releasing app 1.0.0 (from command line)");
}

#[test]
fn version_must_be_newer() {
    let repo = single("app", &["1.4.2"], None);
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers()
        .args(["release", "--version", "1.4.2"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("is not newer than the current version 1.4.2"));
    assert_eq!(repo.read("CHANGELOG.md"), changelog_with(&["1.4.2"]));
    assert!(repo.exists("changes/a.fixed.md"));
}

#[test]
fn version_file_newer_than_changelog_is_released_as_is() {
    let repo = single("app", &["1.2.0"], Some("1.3.0"));
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers()
        .arg("release")
        .assert()
        .success()
        .stderr("")
        .stdout("Releasing app 1.3.0 (from VERSION)\n  Updated CHANGELOG.md\n  Removed 1 fragment\n");
    assert_eq!(repo.read("VERSION"), "1.3.0\n");
}

#[test]
fn version_file_smaller_than_fragments_warns() {
    let repo = single("app", &["1.2.0"], Some("1.2.1"));
    repo.write("changes/a.added.md", "Add.\n");
    repo.vers()
        .arg("release")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("Releasing app 1.2.1 (from VERSION)\n"))
        .stderr(predicate::str::starts_with("warning: ").and(predicate::str::contains("minor bump")));
}

#[test]
fn version_files_disagree_writes_nothing() {
    let repo = TestRepo::new();
    repo.write(
        "vers.toml",
        "[package]\nname = \"app\"\n\n[[version.files]]\npath = \"VERSION\"\n\n[[version.files]]\npath = \"package.json\"\nkey = \"version\"\n",
    );
    repo.write("VERSION", "1.3.0\n");
    repo.write("package.json", "{ \"version\": \"1.2.0\" }\n");
    repo.write("CHANGELOG.md", &changelog_with(&["1.2.0"]));
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers().arg("release").assert().code(1).stdout("").stderr(predicate::str::contains("version files disagree"));
    assert_eq!(repo.read("CHANGELOG.md"), changelog_with(&["1.2.0"]));
    assert_eq!(repo.read("VERSION"), "1.3.0\n");
    assert!(repo.exists("changes/a.fixed.md"));

    // --force uses the highest and rewrites the file that differs.
    repo.vers().args(["release", "--force"]).assert().success().stdout(
        "Releasing app 1.3.0 (from VERSION)\n  Updated CHANGELOG.md\n  Updated package.json (1.2.0 → 1.3.0)\n  Removed 1 fragment\n",
    );
    assert_eq!(repo.read("package.json"), "{ \"version\": \"1.3.0\" }\n");
}

#[test]
fn version_file_older_than_changelog_is_an_error() {
    let repo = single("app", &["1.2.0"], Some("1.1.0"));
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers().arg("release").assert().code(1).stderr(predicate::str::contains("older than the current version"));
    assert_eq!(repo.read("CHANGELOG.md"), changelog_with(&["1.2.0"]));
}

#[test]
fn prereleases_then_final_folds() {
    let repo = single("app", &["1.2.0"], Some("1.2.0"));
    repo.write("changes/a.added.md", "Add a.\n");
    repo.vers().args(["release", "--pre", "rc"]).assert().success().stdout(
        "Releasing app 1.3.0-rc.1 (inferred: minor)\n  Updated CHANGELOG.md\n  Updated VERSION (1.2.0 → 1.3.0-rc.1)\n  Removed 1 fragment\n",
    );
    repo.write("changes/b.fixed.md", "Fix b.\n");
    repo.vers().args(["release", "--pre", "rc"]).assert().success().stdout(
        "Releasing app 1.3.0-rc.2 (inferred: patch)\n  Updated CHANGELOG.md\n  Updated VERSION (1.3.0-rc.1 → 1.3.0-rc.2)\n  Removed 1 fragment\n",
    );
    assert!(
        repo.read("CHANGELOG.md").contains("## [1.3.0-rc.2] - 2026-10-14\n\n### Fixed\n\n- Fix b.\n\n## [1.3.0-rc.1]")
    );

    // The final release needs no new fragments.
    repo.vers().arg("release").assert().success().stdout(
        "Releasing app 1.3.0 (finalising pre-release)\n  Updated CHANGELOG.md\n  Updated VERSION (1.3.0-rc.2 → 1.3.0)\n",
    );
    let changelog = repo.read("CHANGELOG.md");
    assert!(
        changelog.contains(
            "## [Unreleased]\n\n## [1.3.0] - 2026-10-14\n\n### Added\n\n- Add a.\n\n### Fixed\n\n- Fix b.\n\n## [1.2.0]"
        ),
        "{changelog}"
    );
    assert!(!changelog.contains("rc."));
    assert_eq!(repo.read("VERSION"), "1.3.0\n");

    // Nothing left to release.
    repo.vers().arg("release").assert().code(1).stderr("error: nothing to release for `app`: no pending fragments\n");
}

#[test]
fn final_release_with_new_fragments_after_prerelease() {
    let repo = single("app", &["1.2.0"], None);
    repo.write("changes/a.added.md", "Add a.\n");
    repo.vers().args(["release", "--pre", "rc"]).assert().success();
    repo.write("changes/c.fixed.md", "Fix c.\n");
    repo.write("changes/d.added.md", "Add d.\n");
    repo.vers().arg("release").assert().success();
    let changelog = repo.read("CHANGELOG.md");
    assert!(
        changelog.contains(
            "## [1.3.0] - 2026-10-14\n\n### Added\n\n- Add a.\n- Add d.\n\n### Fixed\n\n- Fix c.\n\n## [1.2.0]"
        ),
        "{changelog}"
    );
}

#[test]
fn prereleases_kept_without_folding() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"app\"\n\n[version]\nfold_prereleases = false\n");
    repo.write("CHANGELOG.md", &changelog_with(&["1.2.0"]));
    repo.write("changes/a.added.md", "Add a.\n");
    repo.vers().args(["release", "--pre", "rc"]).assert().success();
    repo.write("changes/b.fixed.md", "Fix b.\n");
    repo.vers().arg("release").assert().success().stdout(predicate::str::starts_with("Releasing app 1.3.0 "));
    let changelog = repo.read("CHANGELOG.md");
    assert!(changelog.contains("## [1.3.0] - 2026-10-14\n\n### Fixed\n\n- Fix b.\n\n## [1.3.0-rc.1]"), "{changelog}");
}

// ---------------------------------------------------------------------------
// Selection and nothing to release
// ---------------------------------------------------------------------------

#[test]
fn nothing_to_release_is_an_error() {
    let repo = single("app", &["1.0.0"], None);
    repo.vers()
        .arg("release")
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: nothing to release for `app`: no pending fragments\n");

    let repo = monorepo();
    repo.vers()
        .args(["release", "web"])
        .assert()
        .code(1)
        .stderr("error: nothing to release for `web`: no pending fragments\n");
}

#[test]
fn hidden_fragments_alone_are_nothing_to_release_but_are_removed_with_others() {
    let repo = TestRepo::new();
    repo.write(
        "vers.toml",
        "[package]\nname = \"app\"\n\n[[types]]\nname = \"fixed\"\nheading = \"Fixed\"\nbump = \"patch\"\n\n[[types]]\nname = \"internal\"\nheading = \"Internal\"\nbump = \"none\"\nhidden = true\n",
    );
    repo.write("CHANGELOG.md", &changelog_with(&["1.0.0"]));
    repo.write("changes/refactor.internal.md", "Refactor.\n");
    repo.vers().arg("release").assert().code(1).stderr(predicate::str::contains("nothing to release"));
    assert!(repo.exists("changes/refactor.internal.md"));

    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers()
        .arg("release")
        .assert()
        .success()
        .stdout("Releasing app 1.0.1 (inferred: patch)\n  Updated CHANGELOG.md\n  Removed 2 fragments\n");
    assert!(!repo.exists("changes/refactor.internal.md"));
    assert!(!repo.read("CHANGELOG.md").contains("Refactor"));
}

#[test]
fn all_with_nothing_pending_skips_everything() {
    let repo = monorepo();
    repo.vers()
        .args(["release", "--all"])
        .assert()
        .success()
        .stdout("Skipped api (no fragments)\nSkipped web (no fragments)\n");
}

#[test]
fn version_option_usage_errors() {
    let repo = monorepo();
    repo.write("packages/api/changes/a.fixed.md", "Fix.\n");
    repo.write("packages/web/changes/a.fixed.md", "Fix.\n");
    repo.vers()
        .args(["release", "--all", "--version", "2.0.0"])
        .assert()
        .code(2)
        .stderr(predicate::str::starts_with("error: --version can't be used with --all or with more than one package"));
    repo.vers()
        .args(["release", "api", "web", "--version", "2.0.0"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("more than one package"));
    repo.vers().args(["release", "api", "--version", "2.0.0", "--bump", "major"]).assert().code(2);
    repo.vers().args(["release", "api", "--all"]).assert().code(2);
    repo.vers().args(["release", "nope"]).assert().code(2).stderr(predicate::str::contains("unknown package `nope`"));
    // --bump with several packages is fine.
    repo.vers().args(["release", "api", "web", "--bump", "major"]).assert().success().stdout(
        "Releasing api 2.0.0 (from command line)\n  Updated packages/api/CHANGELOG.md\n  Updated packages/api/Cargo.toml (1.4.0 → 2.0.0)\n  Removed 1 fragment\n\
         Releasing web 0.4.0 (from command line)\n  Updated packages/web/CHANGELOG.md\n  Updated packages/web/package.json (0.3.0 → 0.4.0)\n  Removed 1 fragment\n",
    );
    assert_eq!(repo.read("packages/api/Cargo.toml"), "[package]\nname = \"api\"\nversion = \"2.0.0\"\n",);
}

#[test]
fn a_failing_package_aborts_all_writes() {
    let repo = monorepo();
    repo.write("packages/api/changes/a.added.md", "Add.\n");
    repo.write("packages/web/changes/b.fixed.md", "Fix.\n");
    // web's version file is older than its changelog.
    repo.write("packages/web/CHANGELOG.md", &changelog_with(&["0.5.0"]));
    repo.vers()
        .args(["release", "--all"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("older than the current version 0.5.0"));
    assert_eq!(repo.read("packages/api/CHANGELOG.md"), HEADER);
    assert_eq!(repo.read("packages/api/Cargo.toml"), "[package]\nname = \"api\"\nversion = \"1.4.0\"\n");
    assert!(repo.exists("packages/api/changes/a.added.md"));
    assert!(repo.exists("packages/web/changes/b.fixed.md"));
}

#[test]
fn invalid_fragment_aborts_release() {
    let repo = single("app", &["1.0.0"], None);
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.write("changes/b.bogus.md", "What.\n");
    repo.vers().arg("release").assert().code(1).stdout("");
    assert!(repo.exists("changes/a.fixed.md"));
    assert_eq!(repo.read("CHANGELOG.md"), changelog_with(&["1.0.0"]));
}

// ---------------------------------------------------------------------------
// The changelog file
// ---------------------------------------------------------------------------

#[test]
fn missing_changelog_is_created() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"app\"\n");
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers()
        .arg("release")
        .assert()
        .success()
        .stdout("Releasing app 0.1.0 (initial)\n  Created CHANGELOG.md\n  Removed 1 fragment\n");
    assert_eq!(repo.read("CHANGELOG.md"), format!("{HEADER}\n## [0.1.0] - 2026-10-14\n\n### Fixed\n\n- Fix.\n"));
}

#[test]
fn missing_unreleased_heading_is_added_and_reported() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"app\"\n");
    repo.write("CHANGELOG.md", "# Changelog\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n");
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers().arg("release").assert().success().stdout(
        "Releasing app 1.0.1 (inferred: patch)\n  Added ## [Unreleased] to CHANGELOG.md\n  Updated CHANGELOG.md\n  Removed 1 fragment\n",
    );
    assert_eq!(
        repo.read("CHANGELOG.md"),
        "# Changelog\n\n## [Unreleased]\n\n## [1.0.1] - 2026-10-14\n\n### Fixed\n\n- Fix.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n"
    );
}

// ---------------------------------------------------------------------------
// Fragments
// ---------------------------------------------------------------------------

#[test]
fn tracked_fragments_are_removed_with_git_rm() {
    let repo = single("app", &["1.0.0"], None);
    repo.write("changes/a.fixed.md", "Fix a.\n");
    repo.write("changes/b.fixed.md", "Fix b.\n");
    repo.commit("Add a and b", Some("2026-02-01T00:00:00Z"));
    // Staged but not committed, and edited after staging.
    repo.write("changes/c.fixed.md", "Fix c.\n");
    repo.git(&["add", "changes/c.fixed.md"]);
    repo.write("changes/c.fixed.md", "Fix c, edited.\n");
    // Untracked.
    repo.write("changes/d.fixed.md", "Fix d.\n");

    repo.vers()
        .arg("release")
        .assert()
        .success()
        .stdout("Releasing app 1.0.1 (inferred: patch)\n  Updated CHANGELOG.md\n  Removed 4 fragments\n");
    for f in ["a", "b", "c", "d"] {
        assert!(!repo.exists(&format!("changes/{f}.fixed.md")));
    }
    assert!(repo.exists("changes/.gitkeep"));
    let status = repo.git(&["status", "--porcelain"]);
    assert!(status.contains("D  changes/a.fixed.md"), "{status}");
    assert!(status.contains("D  changes/b.fixed.md"), "{status}");
    assert!(!status.contains("c.fixed.md"), "{status}");
    assert!(!status.contains("d.fixed.md"), "{status}");
    // Committed fragments come first, in add order.
    assert!(repo.read("CHANGELOG.md").contains("- Fix a.\n- Fix b.\n"));
}

// ---------------------------------------------------------------------------
// Dry run
// ---------------------------------------------------------------------------

#[test]
fn dry_run_prints_a_diff_and_writes_nothing() {
    let repo = single("app", &[], Some("1.0.0"));
    repo.write("CHANGELOG.md", &changelog_with(&["1.0.0"]));
    repo.write("changes/a.fixed.md", "Fix.\n");
    let expected = [
        "--- a/CHANGELOG.md",
        "+++ b/CHANGELOG.md",
        "@@ -7,6 +7,12 @@",
        " ",
        " ## [Unreleased]",
        " ",
        "+## [1.0.1] - 2026-10-14",
        "+",
        "+### Fixed",
        "+",
        "+- Fix.",
        "+",
        " ## [1.0.0] - 2026-01-01",
        " ",
        " ### Fixed",
        "--- a/VERSION",
        "+++ b/VERSION",
        "@@ -1 +1 @@",
        "-1.0.0",
        "+1.0.1",
        "delete changes/a.fixed.md",
        "",
    ]
    .join("\n");
    repo.vers().args(["release", "--dry-run"]).assert().success().stderr("").stdout(expected);
    assert_eq!(repo.read("CHANGELOG.md"), changelog_with(&["1.0.0"]));
    assert_eq!(repo.read("VERSION"), "1.0.0\n");
    assert!(repo.exists("changes/a.fixed.md"));
}

#[test]
fn dry_run_of_a_new_changelog_and_with_tag_does_nothing() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"app\"\n\n[version]\ntag = true\n");
    repo.write("changes/a.fixed.md", "Fix.\n");
    let out = repo.vers().args(["release", "--dry-run"]).output().unwrap();
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.starts_with("--- /dev/null\n+++ b/CHANGELOG.md\n"), "{text}");
    assert!(text.ends_with("+- Fix.\ndelete changes/a.fixed.md\n"), "{text}");
    assert!(!repo.exists("CHANGELOG.md"));
    assert!(repo.git(&["tag", "--list"]).is_empty());
}

#[test]
fn dry_run_in_a_subdirectory_shows_paths_relative_to_it() {
    let repo = monorepo();
    repo.write("packages/api/changes/a.fixed.md", "Fix.\n");
    let out = repo.vers_in("packages").args(["release", "--all", "--dry-run"]).output().unwrap();
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("--- a/api/CHANGELOG.md\n+++ b/api/CHANGELOG.md\n"), "{text}");
    assert!(text.contains("--- a/api/Cargo.toml\n+++ b/api/Cargo.toml\n"), "{text}");
    assert!(text.ends_with("delete api/changes/a.fixed.md\n"), "{text}");
    assert!(repo.exists("packages/api/changes/a.fixed.md"));
}

// ---------------------------------------------------------------------------
// Commit and tag
// ---------------------------------------------------------------------------

#[test]
fn tag_commits_only_release_files() {
    let repo = single("app", &["1.0.0"], Some("1.0.0"));
    repo.write(
        "vers.toml",
        "[package]\nname = \"app\"\n\n[version]\ntag = true\n\n[[version.files]]\npath = \"VERSION\"\n",
    );
    repo.write("changes/a.fixed.md", "Fix a.\n");
    repo.commit("Add a", Some("2026-02-01T00:00:00Z"));
    repo.write("changes/b.added.md", "Add b.\n");
    // Unrelated staged and unstaged changes must stay out of the commit.
    repo.write("other.txt", "staged\n");
    repo.git(&["add", "other.txt"]);
    repo.write("unstaged.txt", "x\n");

    let out = repo.vers().arg("release").output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let sha = repo.git(&["rev-parse", "--short", "HEAD"]).trim().to_string();
    assert_eq!(
        stdout(&out),
        format!(
            "Releasing app 1.1.0 (inferred: minor)\n  Updated CHANGELOG.md\n  Updated VERSION (1.0.0 → 1.1.0)\n  Removed 2 fragments\n  Committed {sha}\n  Tagged v1.1.0\n"
        )
    );
    assert_eq!(repo.git(&["log", "-1", "--format=%B"]).trim(), "Release app 1.1.0");
    let files = repo.git(&["show", "--name-status", "--format=", "HEAD"]);
    assert_eq!(files, "M\tCHANGELOG.md\nM\tVERSION\nD\tchanges/a.fixed.md\n");
    assert_eq!(repo.git(&["status", "--porcelain"]), "A  other.txt\n?? unstaged.txt\n");
    assert_eq!(repo.git(&["tag", "-n1", "--list"]).trim(), "v1.1.0          Release app 1.1.0");
    assert_eq!(repo.git(&["cat-file", "-t", "v1.1.0"]).trim(), "tag");
    assert_eq!(repo.git(&["rev-parse", "v1.1.0^{commit}"]), repo.git(&["rev-parse", "HEAD"]));
}

#[test]
fn tag_flags_override_config() {
    let repo = single("app", &["1.0.0"], None);
    repo.write(
        "vers.toml",
        "[package]\nname = \"app\"\n\n[version]\ntag = true\ncommit_message = \"chore(release): {name} {version}\"\n",
    );
    repo.commit("config", None);
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers()
        .args(["release", "--no-tag"])
        .assert()
        .success()
        .stdout("Releasing app 1.0.1 (inferred: patch)\n  Updated CHANGELOG.md\n  Removed 1 fragment\n");
    assert!(repo.git(&["tag", "--list"]).is_empty());
    assert_eq!(repo.git(&["log", "-1", "--format=%s"]).trim(), "config");
    repo.commit("release", None);

    repo.write(
        "vers.toml",
        "[package]\nname = \"app\"\n\n[version]\ncommit_message = \"chore(release): {name} {version}\"\n",
    );
    repo.commit("config 2", None);
    repo.write("changes/b.fixed.md", "Fix.\n");
    repo.vers()
        .args(["release", "--no-tag", "--tag"])
        .assert()
        .success()
        .stdout(predicate::str::ends_with("  Tagged v1.0.2\n"));
    assert_eq!(repo.git(&["log", "-1", "--format=%s"]).trim(), "chore(release): app 1.0.2");
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
}

#[test]
fn all_packages_share_one_commit_with_a_tag_each() {
    let repo = monorepo();
    repo.write("packages/api/changes/a.added.md", "Add.\n");
    repo.write("packages/web/changes/b.fixed.md", "Fix.\n");
    let out = repo.vers().args(["release", "--all", "--tag"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let sha = repo.git(&["rev-parse", "--short", "HEAD"]).trim().to_string();
    assert_eq!(
        stdout(&out),
        format!(
            "Releasing api 1.5.0 (inferred: minor)\n  Updated packages/api/CHANGELOG.md\n  Updated packages/api/Cargo.toml (1.4.0 → 1.5.0)\n  Removed 1 fragment\n  Committed {sha}\n  Tagged api@v1.5.0\n\
             Releasing web 0.3.1 (inferred: patch)\n  Updated packages/web/CHANGELOG.md\n  Updated packages/web/package.json (0.3.0 → 0.3.1)\n  Removed 1 fragment\n  Committed {sha}\n  Tagged web@v0.3.1\n"
        )
    );
    assert_eq!(
        repo.git(&["log", "-1", "--format=%B"]).trim(),
        "Release 2 packages\n\nRelease api 1.5.0\nRelease web 0.3.1"
    );
    let mut tags: Vec<String> = repo.git(&["tag", "--list"]).lines().map(String::from).collect();
    tags.sort();
    assert_eq!(tags, ["api@v1.5.0", "web@v0.3.1"]);
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
}

#[test]
fn existing_tag_aborts_before_writing() {
    let repo = single("app", &["1.0.0"], None);
    repo.git(&["tag", "v1.0.1"]);
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers()
        .args(["release", "--tag"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("tag `v1.0.1` already exists"));
    assert_eq!(repo.read("CHANGELOG.md"), changelog_with(&["1.0.0"]));
    assert!(repo.exists("changes/a.fixed.md"));
}

#[test]
fn tagging_without_git_is_a_usage_error() {
    let repo = TestRepo::without_git();
    repo.write("vers.toml", "[package]\nname = \"app\"\n");
    repo.write("CHANGELOG.md", &changelog_with(&["1.0.0"]));
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers().args(["release", "--tag"]).assert().code(2).stderr(predicate::str::contains("needs a git repository"));
    assert!(repo.exists("changes/a.fixed.md"));
    // Without tagging, git isn't needed.
    repo.vers()
        .arg("release")
        .assert()
        .success()
        .stdout("Releasing app 1.0.1 (inferred: patch)\n  Updated CHANGELOG.md\n  Removed 1 fragment\n");
    assert!(!repo.exists("changes/a.fixed.md"));
}

// ---------------------------------------------------------------------------
// JSON and quiet output
// ---------------------------------------------------------------------------

#[test]
fn json_output() {
    let repo = monorepo();
    repo.write("packages/api/changes/a.added.md", "Add.\n");
    let out = repo.vers().args(["--format", "json", "release", "--all"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "released": [{
                "name": "api",
                "version": "1.5.0",
                "source": "inferred: minor",
                "files": ["packages/api/CHANGELOG.md", "packages/api/Cargo.toml"],
                "removed": ["packages/api/changes/a.added.md"],
                "tag": null,
            }],
            "skipped": ["web"],
            "dry_run": false,
        })
    );
}

#[test]
fn json_dry_run_and_tag() {
    let repo = single("app", &["1.0.0"], None);
    repo.write("changes/a.fixed.md", "Fix.\n");
    let out = repo.vers().args(["--format", "json", "release", "--dry-run", "--tag"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["released"][0]["tag"], "v1.0.1");
    assert!(repo.exists("changes/a.fixed.md"));
}

#[test]
fn quiet_prints_nothing() {
    let repo = single("app", &["1.0.0"], None);
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers().args(["-q", "release"]).assert().success().stdout("").stderr("");
    assert!(!repo.exists("changes/a.fixed.md"));
}

#[test]
fn pre_on_top_of_a_newer_files_version_rewrites_the_files() {
    let repo = single("app", &["1.2.0"], Some("1.3.0"));
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers().args(["release", "--pre", "rc"]).assert().success().stdout(
        "Releasing app 1.3.0-rc.1 (from VERSION)\n  Updated CHANGELOG.md\n  Updated VERSION (1.3.0 → 1.3.0-rc.1)\n  Removed 1 fragment\n",
    );
    assert_eq!(repo.read("VERSION"), "1.3.0-rc.1\n");
}

#[test]
fn files_above_initial_without_a_release_are_the_current_version() {
    // No changelog release and no tags: VERSION at 1.4.0 means 1.4.0 was
    // released before vers, so a fix gives 1.4.1.
    let repo = single("app", &[], Some("1.4.0"));
    repo.write("changes/a.fixed.md", "Fix.\n");
    repo.vers().arg("release").assert().success().stdout(
        "Releasing app 1.4.1 (inferred: patch)\n  Updated CHANGELOG.md\n  Updated VERSION (1.4.0 → 1.4.1)\n  Removed 1 fragment\n",
    );
    // --version can't go backwards from it.
    repo.write("changes/b.fixed.md", "Fix.\n");
    repo.vers().args(["release", "--version", "1.0.0"]).assert().code(1);
}
