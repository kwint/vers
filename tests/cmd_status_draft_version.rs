//! `vers status`, `vers draft` and `vers version`.

mod common;

use common::TestRepo;
use serde_json::{Value, json};

const HEADER: &str = "\
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
";

fn changelog(releases: &str) -> String {
    format!("{HEADER}{releases}")
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn json_stdout(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap()
}

/// The project from the first-release tutorial, after `vers init` and two
/// `vers new` (fragments committed in order: added, then fixed).
fn greeter() -> TestRepo {
    let repo = TestRepo::new();
    repo.write(
        "vers.toml",
        "[package]\nname = \"greeter\"\n\n[changelog]\nrepo = \"github\"\nrepo_url = \"https://github.com/example/greeter\"\n\n[[version.files]]\npath = \"VERSION\"\n",
    )
    .write("VERSION", "0.1.0\n")
    .write("CHANGELOG.md", HEADER)
    .write("changes/.gitkeep", "");
    repo.commit("Set up vers", Some("2026-01-01T00:00:00Z"));
    repo.write("changes/greet-people-by-name.added.md", "Greet people by name.\n");
    repo.commit("Add", Some("2026-01-02T00:00:00Z"));
    repo.write("changes/7.fixed.md", "Don't crash on an empty name.\n");
    repo.commit("Fix", Some("2026-01-03T00:00:00Z"));
    repo
}

/// The monorepo from the monorepo tutorial at step 5.
fn shop() -> TestRepo {
    let repo = TestRepo::new();
    repo.write(
        "vers.toml",
        "[changelog]\nrepo = \"gitlab\"\nrepo_url = \"https://gitlab.com/example/shop\"\n\n[version]\ntag_format = \"{name}@v{version}\"\n",
    );
    repo.write("packages/api/Cargo.toml", "[package]\nname = \"api\"\nversion = \"1.4.0\"\n")
        .write(
            "packages/api/vers.toml",
            "extend = \"../../vers.toml\"\n\n[package]\nname = \"api\"\n\n[[version.files]]\npath = \"Cargo.toml\"\nkey = \"package.version\"\n",
        )
        .write("packages/api/CHANGELOG.md", &changelog("\n## [1.4.0] - 2026-01-01\n\n### Added\n\n- Orders.\n"))
        .write("packages/api/changes/mr-12.added.md", "Add the /orders endpoint.\n")
        .write(
            "packages/api/changes/return-404-instead-of-400-for.changed.md",
            "Return 404 instead of 400 for unknown orders.\n",
        );
    repo.write("packages/web/package.json", "{ \"name\": \"web\", \"version\": \"0.3.0\" }\n")
        .write(
            "packages/web/vers.toml",
            "extend = \"../../vers.toml\"\n\n[package]\nname = \"web\"\n\n[[version.files]]\npath = \"package.json\"\nkey = \"version\"\n",
        )
        .write("packages/web/CHANGELOG.md", &changelog("\n## [0.3.0] - 2026-01-01\n\n### Added\n\n- Cart.\n"))
        .write("packages/web/changes/30.fixed.md", "Fix the cart total rounding.\n");
    repo.commit("Set up", Some("2026-01-01T00:00:00Z"));
    repo
}

// ---------------------------------------------------------------------------
// vers status
// ---------------------------------------------------------------------------

#[test]
fn status_first_release_from_version_file() {
    let repo = greeter();
    repo.vers()
        .arg("status")
        .assert()
        .success()
        .stdout("greeter  (unreleased) → 0.1.0 (from VERSION)  2 fragments: 1 added, 1 fixed\n")
        .stderr("");
}

#[test]
fn status_second_release_inferred() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"greeter\"\n\n[[version.files]]\npath = \"VERSION\"\n")
        .write("VERSION", "0.1.0\n")
        .write("CHANGELOG.md", &changelog("\n## [0.1.0] - 2026-10-14\n\n### Added\n\n- Greet.\n"))
        .write("changes/trim.fixed.md", "Trim whitespace around names.\n");
    repo.vers()
        .arg("status")
        .assert()
        .success()
        .stdout("greeter  0.1.0 → 0.1.1 (inferred: patch)  1 fragment: 1 fixed\n");
}

#[test]
fn status_initial_version_without_files() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n").write("changes/x.added.md", "X.\n");
    repo.vers().arg("status").assert().success().stdout("tool  (unreleased) → 0.1.0 (initial)  1 fragment: 1 added\n");
    let out = repo.vers().args(["--format", "json", "status"]).output().unwrap();
    let pkg = &json_stdout(&out)["packages"][0];
    assert_eq!(pkg["version_source"], "initial");
    assert_eq!(pkg["bump"], Value::Null);
    assert_eq!(pkg["current_version"], Value::Null);
    assert_eq!(pkg["next_version"], "0.1.0");
}

#[test]
fn status_initial_honours_config() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n\n[version]\ninitial = \"1.0.0\"\n")
        .write("changes/x.fixed.md", "X.\n");
    repo.vers().arg("status").assert().success().stdout("tool  (unreleased) → 1.0.0 (initial)  1 fragment: 1 fixed\n");
}

#[test]
fn status_no_fragments_shows_current_only() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"cli\"\n")
        .write("CHANGELOG.md", &changelog("\n## [2.0.0] - 2026-01-01\n\n### Added\n\n- X.\n"));
    repo.vers().arg("status").assert().success().stdout("cli  2.0.0  no fragments\n");
}

#[test]
fn status_unreleased_without_fragments() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"new\"\n");
    repo.vers().arg("status").assert().success().stdout("new  (unreleased)  no fragments\n");
    let out = repo.vers().args(["--format", "json", "status"]).output().unwrap();
    assert_eq!(
        json_stdout(&out),
        json!({"packages": [{
            "name": "new", "path": ".", "current_version": null, "next_version": null,
            "version_source": null, "bump": null, "fragments": []
        }]})
    );
}

#[test]
fn status_current_version_from_tags() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n");
    repo.commit("init", None);
    repo.git(&["tag", "v1.2.0"]);
    repo.git(&["tag", "v1.10.0"]);
    repo.git(&["tag", "unrelated"]);
    repo.write("changes/x.fixed.md", "X.\n");
    repo.vers()
        .arg("status")
        .assert()
        .success()
        .stdout("tool  1.10.0 → 1.10.1 (inferred: patch)  1 fragment: 1 fixed\n");
}

#[test]
fn status_monorepo_from_root() {
    let repo = shop();
    repo.vers()
        .arg("status")
        .assert()
        .success()
        .stdout(
            "api  1.4.0 → 1.5.0 (inferred: minor)  2 fragments: 1 added, 1 changed\n\
             web  0.3.0 → 0.3.1 (inferred: patch)  1 fragment: 1 fixed\n",
        )
        .stderr("");
}

#[test]
fn status_pads_columns_to_widest_value() {
    let repo = shop();
    std::fs::remove_file(repo.path("packages/web/changes/30.fixed.md")).unwrap();
    repo.write("packages/web/package.json", "{ \"name\": \"web\", \"version\": \"0.4.0\" }\n");
    repo.write("packages/web/changes/1.fixed.md", "A.\n").write("packages/web/changes/2.fixed.md", "B.\n");
    repo.write("packages/cli/vers.toml", "[package]\nname = \"cli\"\n")
        .write("packages/cli/CHANGELOG.md", &changelog("\n## [2.0.0] - 2026-01-01\n\n### Added\n\n- X.\n"));
    repo.vers().arg("status").assert().success().stdout(
        "api  1.4.0 → 1.5.0 (inferred: minor)    2 fragments: 1 added, 1 changed\n\
             cli  2.0.0                              no fragments\n\
             web  0.3.0 → 0.4.0 (from package.json)  2 fragments: 2 fixed\n",
    );
}

#[test]
fn status_named_packages() {
    let repo = shop();
    repo.vers()
        .args(["status", "web"])
        .assert()
        .success()
        .stdout("web  0.3.0 → 0.3.1 (inferred: patch)  1 fragment: 1 fixed\n");
}

#[test]
fn status_unknown_package_is_a_usage_error() {
    let repo = shop();
    repo.vers()
        .args(["status", "nope"])
        .assert()
        .code(2)
        .stdout("")
        .stderr("error: unknown package `nope`; available packages: api, web\n");
}

#[test]
fn status_in_package_directory_shows_only_that_package() {
    let repo = shop();
    repo.vers_in("packages/api")
        .arg("status")
        .assert()
        .success()
        .stdout("api  1.4.0 → 1.5.0 (inferred: minor)  2 fragments: 1 added, 1 changed\n");
    // Below the package's own directory, the owning package is still shown.
    repo.vers_in("packages/api/changes")
        .arg("status")
        .assert()
        .success()
        .stdout("api  1.4.0 → 1.5.0 (inferred: minor)  2 fragments: 1 added, 1 changed\n");
}

#[test]
fn status_without_any_config_fails_with_config_error() {
    let repo = TestRepo::new();
    let out = repo.vers().arg("status").assert().code(2).stdout("").get_output().clone();
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("error: "));
}

#[test]
fn status_with_only_shared_config_fails() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[version]\ntag = false\n");
    repo.vers().arg("status").assert().code(2).stdout("");
}

#[test]
fn status_counts_types_in_configured_order() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n")
        .write("CHANGELOG.md", &changelog("\n## [1.0.0] - 2026-01-01\n\n### Added\n\n- X.\n"))
        .write("changes/a.security.md", "A.\n")
        .write("changes/b.fix.md", "B.\n")
        .write("changes/c.fixed.md", "C.\n")
        .write("changes/d.feat.md", "D.\n");
    repo.vers()
        .arg("status")
        .assert()
        .success()
        .stdout("tool  1.0.0 → 1.1.0 (inferred: minor)  4 fragments: 1 added, 2 fixed, 1 security\n");
}

#[test]
fn status_breaking_fragment_is_major() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n")
        .write("CHANGELOG.md", &changelog("\n## [1.4.2] - 2026-01-01\n\n### Added\n\n- X.\n"))
        .write("changes/x.changed.md", "---\nbreaking: true\n---\nX.\n");
    repo.vers()
        .arg("status")
        .assert()
        .success()
        .stdout("tool  1.4.2 → 2.0.0 (inferred: major)  1 fragment: 1 changed\n");
}

#[test]
fn status_hidden_fragments_are_counted_but_not_released() {
    let repo = TestRepo::new();
    repo.write(
        "vers.toml",
        "[package]\nname = \"tool\"\n\n[[types]]\nname = \"fixed\"\nheading = \"Fixed\"\n\n[[types]]\nname = \"internal\"\nheading = \"Internal\"\nhidden = true\n",
    )
    .write("CHANGELOG.md", &changelog("\n## [1.0.0] - 2026-01-01\n\n### Fixed\n\n- X.\n"))
    .write("changes/x.internal.md", "X.\n");
    repo.vers().arg("status").assert().success().stdout("tool  1.0.0  1 fragment: 1 internal\n");
    repo.vers()
        .arg("draft")
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: nothing to release for `tool`: no pending fragments\n");
}

#[test]
fn status_finalising_prerelease() {
    let repo = prerelease_repo();
    repo.vers()
        .arg("status")
        .assert()
        .success()
        .stdout("app  1.3.0-rc.2 → 1.3.0 (finalising pre-release)  no fragments\n");
    let out = repo.vers().args(["--format", "json", "status"]).output().unwrap();
    let pkg = &json_stdout(&out)["packages"][0];
    assert_eq!(pkg["version_source"], "finalise");
    assert_eq!(pkg["next_version"], "1.3.0");
    assert_eq!(pkg["bump"], Value::Null);
}

#[test]
fn status_warns_when_files_are_behind_fragments() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n\n[[version.files]]\npath = \"VERSION\"\n")
        .write("VERSION", "1.4.3\n")
        .write("CHANGELOG.md", &changelog("\n## [1.4.2] - 2026-01-01\n\n### Added\n\n- X.\n"))
        .write("changes/x.added.md", "X.\n");
    let out = repo
        .vers()
        .arg("status")
        .assert()
        .success()
        .stdout("tool  1.4.2 → 1.4.3 (from VERSION)  1 fragment: 1 added\n")
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("warning: "), "{stderr}");
    assert!(stderr.contains("minor"), "{stderr}");
}

#[test]
fn status_version_files_disagreeing_is_an_error() {
    let repo = TestRepo::new();
    repo.write(
        "vers.toml",
        "[package]\nname = \"tool\"\n\n[[version.files]]\npath = \"VERSION\"\n\n[[version.files]]\npath = \"OTHER\"\n",
    )
    .write("VERSION", "1.0.0\n")
    .write("OTHER", "1.1.0\n");
    repo.vers().arg("status").assert().code(1).stdout("");
}

#[test]
fn status_json_matches_reference_example() {
    let repo = shop();
    let out = repo.vers().args(["status", "--format", "json"]).output().unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let value = json_stdout(&out);
    assert_eq!(
        value,
        json!({"packages": [
            {
                "name": "api",
                "path": "packages/api",
                "current_version": "1.4.0",
                "next_version": "1.5.0",
                "version_source": "inferred",
                "bump": "minor",
                "fragments": [
                    { "path": "packages/api/changes/mr-12.added.md", "type": "added", "refs": ["mr-12"], "breaking": false },
                    { "path": "packages/api/changes/return-404-instead-of-400-for.changed.md", "type": "changed", "refs": [], "breaking": false }
                ]
            },
            {
                "name": "web",
                "path": "packages/web",
                "current_version": "0.3.0",
                "next_version": "0.3.1",
                "version_source": "inferred",
                "bump": "patch",
                "fragments": [
                    { "path": "packages/web/changes/30.fixed.md", "type": "fixed", "refs": ["30"], "breaking": false }
                ]
            }
        ]})
    );
}

#[test]
fn status_json_files_source_and_relative_paths() {
    let repo = greeter();
    repo.write("changes/x.changed.md", "---\nrefs: [PROJ-1]\nbreaking: true\n---\nX.\n");
    let out = repo.vers_in("changes").args(["--format", "json", "status"]).output().unwrap();
    assert!(out.status.success());
    let pkg = &json_stdout(&out)["packages"][0];
    assert_eq!(pkg["path"], "..");
    assert_eq!(pkg["version_source"], "files");
    assert_eq!(pkg["bump"], Value::Null);
    let fragments = pkg["fragments"].as_array().unwrap();
    assert_eq!(fragments[0]["path"], "greet-people-by-name.added.md");
    assert_eq!(fragments[2], json!({"path": "x.changed.md", "type": "changed", "refs": ["PROJ-1"], "breaking": true}));
    // The plain-text reference warning goes to stderr, not into the JSON.
    assert!(String::from_utf8_lossy(&out.stderr).contains("warning: "));
}

#[test]
fn status_quiet_prints_nothing() {
    let repo = greeter();
    repo.vers().args(["-q", "status"]).assert().success().stdout("").stderr("");
}

// ---------------------------------------------------------------------------
// vers draft
// ---------------------------------------------------------------------------

#[test]
fn draft_first_release() {
    let repo = greeter();
    repo.vers().arg("draft").assert().success().stderr("").stdout(
        "## [0.1.0] - 2026-10-14\n\
         \n\
         ### Added\n\
         \n\
         - Greet people by name.\n\
         \n\
         ### Fixed\n\
         \n\
         - Don't crash on an empty name. ([#7](https://github.com/example/greeter/issues/7))\n",
    );
    // Nothing was changed.
    assert_eq!(repo.read("CHANGELOG.md"), HEADER);
    assert!(repo.exists("changes/7.fixed.md"));
}

#[test]
fn draft_json() {
    let repo = greeter();
    let out = repo.vers().args(["--format", "json", "draft"]).output().unwrap();
    assert!(out.status.success());
    let value = json_stdout(&out);
    assert_eq!(value["package"], "greeter");
    assert_eq!(value["version"], "0.1.0");
    assert!(value["markdown"].as_str().unwrap().starts_with("## [0.1.0] - 2026-10-14\n\n### Added\n"));
    assert!(value["markdown"].as_str().unwrap().ends_with("issues/7))\n"));
    assert_eq!(value.as_object().unwrap().len(), 3);
}

#[test]
fn draft_version_and_bump_options() {
    let repo = greeter();
    let first = |out: std::process::Output| stdout(&out).lines().next().unwrap().to_string();
    assert_eq!(first(repo.vers().args(["draft", "--version", "2.0.0"]).output().unwrap()), "## [2.0.0] - 2026-10-14");
    assert_eq!(first(repo.vers().args(["draft", "--bump", "major"]).output().unwrap()), "## [0.2.0] - 2026-10-14");
    assert_eq!(first(repo.vers().args(["draft", "--pre", "rc"]).output().unwrap()), "## [0.1.0-rc.1] - 2026-10-14");
}

#[test]
fn draft_bump_on_existing_release() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n")
        .write("CHANGELOG.md", &changelog("\n## [1.2.0] - 2026-01-01\n\n### Added\n\n- X.\n"))
        .write("changes/x.fixed.md", "Fix.\n");
    repo.vers()
        .args(["draft", "--bump", "major"])
        .assert()
        .success()
        .stdout("## [2.0.0] - 2026-10-14\n\n### Fixed\n\n- Fix.\n");
    repo.vers()
        .args(["draft", "--pre", "rc"])
        .assert()
        .success()
        .stdout("## [1.2.1-rc.1] - 2026-10-14\n\n### Fixed\n\n- Fix.\n");
}

#[test]
fn draft_version_not_newer_fails() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n")
        .write("CHANGELOG.md", &changelog("\n## [1.2.0] - 2026-01-01\n\n### Added\n\n- X.\n"))
        .write("changes/x.fixed.md", "Fix.\n");
    repo.vers().args(["draft", "--version", "1.2.0"]).assert().code(1).stdout("");
}

#[test]
fn draft_conflicting_options_is_usage_error() {
    let repo = greeter();
    repo.vers().args(["draft", "--version", "1.0.0", "--bump", "major"]).assert().code(2);
}

#[test]
fn draft_nothing_to_release() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n");
    repo.vers()
        .arg("draft")
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: nothing to release for `tool`: no pending fragments\n");
}

#[test]
fn draft_named_package_and_current_package() {
    let repo = shop();
    let web = "## [0.3.1] - 2026-10-14\n\n### Fixed\n\n- Fix the cart total rounding. ([#30](https://gitlab.com/example/shop/-/issues/30))\n";
    repo.vers().args(["draft", "web"]).assert().success().stdout(web);
    repo.vers_in("packages/web").arg("draft").assert().success().stdout(web);
    repo.vers_in("packages/web/changes").arg("draft").assert().success().stdout(web);
    // From the root there is no current package.
    repo.vers().arg("draft").assert().code(2).stdout("");
}

#[test]
fn draft_groups_by_type_and_merges_identical_entries() {
    let repo = TestRepo::new();
    repo.write(
        "vers.toml",
        "[package]\nname = \"tool\"\n\n[changelog]\nrepo = \"github\"\nrepo_url = \"https://github.com/o/r\"\n",
    )
    .write("CHANGELOG.md", &changelog("\n## [1.0.0] - 2026-01-01\n\n### Added\n\n- X.\n"));
    repo.commit("init", Some("2026-01-01T00:00:00Z"));
    repo.write("changes/2.fixed.md", "Same fix.\n");
    repo.commit("a", Some("2026-01-02T00:00:00Z"));
    repo.write("changes/b.added.md", "Feature.\n");
    repo.commit("b", Some("2026-01-03T00:00:00Z"));
    repo.write("changes/1.fixed.md", "Same fix.\n");
    repo.commit("c", Some("2026-01-04T00:00:00Z"));
    repo.vers().arg("draft").assert().success().stdout(
        "## [1.1.0] - 2026-10-14\n\n### Added\n\n- Feature.\n\n### Fixed\n\n\
         - Same fix. ([#2](https://github.com/o/r/issues/2), [#1](https://github.com/o/r/issues/1))\n",
    );
}

fn prerelease_repo() -> TestRepo {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"app\"\n").write(
        "CHANGELOG.md",
        &changelog(
            "\n## [1.3.0-rc.2] - 2026-03-01\n\n### Fixed\n\n- Second fix.\n\
             \n## [1.3.0-rc.1] - 2026-02-01\n\n### Added\n\n- Feature.\n\n### Fixed\n\n- First fix.\n\
             \n## [1.2.0] - 2026-01-01\n\n### Added\n\n- Old.\n",
        ),
    );
    repo
}

#[test]
fn draft_finalising_prerelease_folds_sections() {
    let repo = prerelease_repo();
    repo.vers()
        .arg("draft")
        .assert()
        .success()
        .stdout("## [1.3.0] - 2026-10-14\n\n### Added\n\n- Feature.\n\n### Fixed\n\n- First fix.\n- Second fix.\n");
}

#[test]
fn draft_final_release_with_new_fragments_folds_earlier_entries_first() {
    let repo = prerelease_repo();
    repo.write("changes/x.fixed.md", "Third fix.\n").write("changes/y.security.md", "Patch it.\n");
    repo.vers().arg("draft").assert().success().stdout(
        "## [1.3.0] - 2026-10-14\n\n### Added\n\n- Feature.\n\n### Fixed\n\n- First fix.\n- Second fix.\n- Third fix.\n\n\
         ### Security\n\n- Patch it.\n",
    );
}

#[test]
fn draft_next_prerelease_does_not_fold() {
    let repo = prerelease_repo();
    repo.write("changes/x.fixed.md", "Third fix.\n");
    repo.vers()
        .args(["draft", "--pre", "rc"])
        .assert()
        .success()
        .stdout("## [1.3.0-rc.3] - 2026-10-14\n\n### Fixed\n\n- Third fix.\n");
}

#[test]
fn draft_without_folding_keeps_only_new_entries() {
    let repo = prerelease_repo();
    repo.write("vers.toml", "[package]\nname = \"app\"\n\n[version]\nfold_prereleases = false\n")
        .write("changes/x.fixed.md", "Third fix.\n");
    repo.vers().arg("draft").assert().success().stdout("## [1.3.0] - 2026-10-14\n\n### Fixed\n\n- Third fix.\n");
}

// ---------------------------------------------------------------------------
// vers version
// ---------------------------------------------------------------------------

#[test]
fn version_current_and_next() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n\n[[version.files]]\npath = \"VERSION\"\n")
        .write("VERSION", "1.4.2\n")
        .write("CHANGELOG.md", &changelog("\n## [1.4.2] - 2026-01-01\n\n### Added\n\n- X.\n"))
        .write("changes/x.added.md", "X.\n");
    repo.vers().arg("version").assert().success().stdout("1.4.2\n").stderr("");
    repo.vers().args(["version", "--next"]).assert().success().stdout("1.5.0\n");
    let out = repo.vers().args(["version", "--format", "json"]).output().unwrap();
    assert_eq!(json_stdout(&out), json!({"package": "tool", "current": "1.4.2", "next": "1.5.0"}));
}

#[test]
fn version_no_release_yet() {
    let repo = greeter();
    repo.vers().arg("version").assert().code(1).stdout("").stderr("error: `greeter` has no released version yet\n");
    repo.vers().args(["version", "--next"]).assert().success().stdout("0.1.0\n");
    let out = repo.vers().args(["--format", "json", "version", "--next"]).output().unwrap();
    assert_eq!(json_stdout(&out), json!({"package": "greeter", "current": null, "next": "0.1.0"}));
}

#[test]
fn version_next_with_nothing_to_release() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n")
        .write("CHANGELOG.md", &changelog("\n## [1.0.0] - 2026-01-01\n\n### Added\n\n- X.\n"));
    repo.vers()
        .args(["version", "--next"])
        .assert()
        .code(1)
        .stdout("")
        .stderr("error: nothing to release for `tool`: no pending fragments\n");
    let out = repo.vers().args(["--format", "json", "version"]).output().unwrap();
    assert_eq!(json_stdout(&out), json!({"package": "tool", "current": "1.0.0", "next": null}));
}

#[test]
fn version_next_finalises_prerelease() {
    let repo = prerelease_repo();
    repo.vers().arg("version").assert().success().stdout("1.3.0-rc.2\n");
    repo.vers().args(["version", "--next"]).assert().success().stdout("1.3.0\n");
}

#[test]
fn version_named_package() {
    let repo = shop();
    repo.vers().args(["version", "api"]).assert().success().stdout("1.4.0\n");
    repo.vers().args(["version", "web", "--next"]).assert().success().stdout("0.3.1\n");
    repo.vers().args(["version", "nope"]).assert().code(2);
}

#[test]
fn version_files_disagreeing_is_an_error() {
    let repo = TestRepo::new();
    repo.write(
        "vers.toml",
        "[package]\nname = \"tool\"\n\n[[version.files]]\npath = \"VERSION\"\n\n[[version.files]]\npath = \"OTHER\"\n",
    )
    .write("VERSION", "1.0.0\n")
    .write("OTHER", "1.1.0\n")
    .write("CHANGELOG.md", &changelog("\n## [1.0.0] - 2026-01-01\n\n### Added\n\n- X.\n"));
    let out = repo.vers().arg("version").assert().code(1).stdout("").get_output().clone();
    assert!(String::from_utf8_lossy(&out.stderr).contains("version files disagree"));
}

#[test]
fn version_files_older_than_changelog_is_an_error() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n\n[[version.files]]\npath = \"VERSION\"\n")
        .write("VERSION", "0.9.0\n")
        .write("CHANGELOG.md", &changelog("\n## [1.0.0] - 2026-01-01\n\n### Added\n\n- X.\n"));
    let out = repo.vers().arg("version").assert().code(1).stdout("").get_output().clone();
    assert!(String::from_utf8_lossy(&out.stderr).contains("older than the current version 1.0.0"));
}

// ---------------------------------------------------------------------------
// Review fixes
// ---------------------------------------------------------------------------

#[test]
fn status_version_file_outside_package_dir_is_shown_relative_to_it() {
    let repo = TestRepo::new();
    repo.write("VERSION", "2.1.0\n")
        .write("packages/api/vers.toml", "[package]\nname = \"api\"\n\n[[version.files]]\npath = \"../../VERSION\"\n")
        .write("packages/api/CHANGELOG.md", &changelog("\n## [2.0.0] - 2026-01-01\n\n### Added\n\n- X.\n"))
        .write("packages/api/changes/x.fixed.md", "X.\n");
    repo.vers()
        .arg("status")
        .assert()
        .success()
        .stdout("api  2.0.0 → 2.1.0 (from ../../VERSION)  1 fragment: 1 fixed\n")
        .stderr("");
}

#[test]
fn status_inferred_label_shows_bump_after_zero_major_shift() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"tool\"\n")
        .write("CHANGELOG.md", &changelog("\n## [0.4.2] - 2026-01-01\n\n### Added\n\n- X.\n"))
        .write("changes/a.added.md", "A.\n");
    repo.vers().arg("status").assert().success().stdout("tool  0.4.2 → 0.4.3 (inferred: patch)  1 fragment: 1 added\n");
    let out = repo.vers().args(["--format", "json", "status"]).output().unwrap();
    assert_eq!(json_stdout(&out)["packages"][0]["bump"], "patch");

    repo.write("changes/b.changed.md", "---\nbreaking: true\n---\nB.\n");
    repo.vers()
        .arg("status")
        .assert()
        .success()
        .stdout("tool  0.4.2 → 0.5.0 (inferred: minor)  2 fragments: 1 added, 1 changed\n");

    repo.write("vers.toml", "[package]\nname = \"tool\"\n\n[version]\nzero_major_shift = false\n");
    repo.vers()
        .arg("status")
        .assert()
        .success()
        .stdout("tool  0.4.2 → 1.0.0 (inferred: major)  2 fragments: 1 added, 1 changed\n");
}

#[test]
fn warnings_show_paths_relative_to_cwd() {
    let repo = shop();
    repo.write("packages/web/changes/x.fixed.md", "---\nrefs: [\"bogus ref\"]\n---\nHi.\n");
    let warning = "reference `bogus ref` matches no reference kind and is shown as plain text\n";
    repo.vers().arg("status").assert().success().stderr(format!("warning: packages/web/changes/x.fixed.md: {warning}"));
    repo.vers_in("packages/web")
        .arg("draft")
        .assert()
        .success()
        .stderr(format!("warning: changes/x.fixed.md: {warning}"));
    repo.vers_in("packages/web/changes")
        .args(["version", "--next"])
        .assert()
        .success()
        .stdout("0.3.1\n")
        .stderr(format!("warning: x.fixed.md: {warning}"));
}

#[test]
fn version_file_warning_is_relative_to_cwd() {
    let repo = TestRepo::new();
    repo.write("packages/tool/vers.toml", "[package]\nname = \"tool\"\n\n[[version.files]]\npath = \"VERSION\"\n")
        .write("packages/tool/VERSION", "1.4.3\n")
        .write("packages/tool/CHANGELOG.md", &changelog("\n## [1.4.2] - 2026-01-01\n\n### Added\n\n- X.\n"))
        .write("packages/tool/changes/x.added.md", "X.\n");
    repo.vers().arg("status").assert().success().stderr(
        "warning: packages/tool/VERSION sets version 1.4.3, but the pending fragments suggest a minor bump (1.5.0); \
         releasing 1.4.3\n",
    );
}

#[test]
fn version_file_errors_are_relative_to_cwd() {
    let repo = shop();
    repo.write("packages/web/package.json", "{ \"name\": \"web\", \"version\": \"0.2.0\" }\n");
    repo.vers().arg("status").assert().code(1).stdout("").stderr(
        "error: packages/web/package.json: version file is older than the current version 0.3.0:\n  \
         packages/web/package.json: 0.2.0\n\
         hint: a release was probably made without vers; set the files to 0.3.0 or newer, or pass --force\n",
    );
    repo.vers_in("packages/web").arg("version").assert().code(1).stdout("").stderr(
        "error: package.json: version file is older than the current version 0.3.0:\n  package.json: 0.2.0\n\
         hint: a release was probably made without vers; set the files to 0.3.0 or newer, or pass --force\n",
    );
}

#[test]
fn draft_finalising_without_folding_uses_prerelease_entries() {
    let repo = prerelease_repo();
    repo.write(
        "vers.toml",
        "[package]\nname = \"app\"\n\n[version]\nfold_prereleases = false\n\n\
         [[types]]\nname = \"added\"\nheading = \"Added\"\n\n\
         [[types]]\nname = \"fixed\"\nheading = \"Fixed\"\n\n\
         [[types]]\nname = \"internal\"\nheading = \"Internal\"\nhidden = true\n",
    );
    let expected = "## [1.3.0] - 2026-10-14\n\n### Added\n\n- Feature.\n\n### Fixed\n\n- First fix.\n- Second fix.\n";
    repo.vers().arg("draft").assert().success().stdout(expected);
    // Only hidden fragments pending: still the pre-release entries, not
    // "nothing to release" while `version --next` says 1.3.0.
    repo.write("changes/x.internal.md", "Refactor.\n");
    repo.vers().args(["version", "--next"]).assert().success().stdout("1.3.0\n");
    repo.vers().arg("draft").assert().success().stdout(expected);
}
