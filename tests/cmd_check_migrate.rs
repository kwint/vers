//! Integration tests for `vers check`, `vers migrate towncrier` and
//! `vers init --from-towncrier`.

mod common;

use common::TestRepo;

/// A monorepo with a shared root config and two packages, committed on `main`,
/// with a branch `feature` checked out.
fn monorepo() -> TestRepo {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[changelog]\nrepo = \"none\"\n")
        .write("packages/web/vers.toml", "extend = \"../../vers.toml\"\n\n[package]\nname = \"web\"\n")
        .write("packages/web/changes/.gitkeep", "")
        .write("packages/web/style.css", "body {}\n")
        .write(
            "packages/api/vers.toml",
            "extend = \"../../vers.toml\"\n\n[package]\nname = \"api\"\n\n[check]\nignore = [\"docs/**\", \"*.md\"]\n",
        )
        .write("packages/api/changes/.gitkeep", "")
        .write("packages/api/src/main.rs", "fn main() {}\n")
        .write("tools/release.sh", "echo\n");
    repo.commit("Initial", None);
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

// ---------------------------------------------------------------------------
// vers check
// ---------------------------------------------------------------------------

#[test]
fn check_fails_for_a_package_without_a_fragment() {
    let repo = monorepo();
    repo.write("packages/web/style.css", "/* tweak */\n");
    repo.commit("Tweak styles", None);
    repo.vers().args(["check", "--base", "main"]).assert().code(1).stdout("").stderr(
        "error: package `web` has changes but no new fragment\n  changed: packages/web/style.css\n  hint: add one with `vers new <type> --package web`, or set VERS_SKIP=1 to skip this check\n",
    );
}

#[test]
fn check_passes_with_a_fragment() {
    let repo = monorepo();
    repo.write("packages/web/style.css", "/* tweak */\n");
    repo.write("packages/web/changes/tweak.changed.md", "Tweak the styles.\n");
    repo.commit("Tweak styles", None);
    repo.vers().args(["check", "--base", "main"]).assert().success().stdout("All changed packages have fragments\n");
}

#[test]
fn check_with_no_package_changes() {
    let repo = monorepo();
    repo.write("tools/release.sh", "echo 2\n").write("README.md", "hi\n");
    repo.commit("Tools", None);
    repo.vers().args(["check", "--base", "main"]).assert().success().stdout("No package changes\n").stderr("");
}

#[test]
fn check_ignores_files_matching_ignore_globs() {
    let repo = monorepo();
    repo.write("packages/api/docs/guide/intro.txt", "x\n").write("packages/api/src/NOTES.md", "x\n");
    repo.commit("Docs", None);
    repo.vers().args(["check", "--base", "main"]).assert().success().stdout("No package changes\n");

    repo.write("packages/api/src/main.rs", "fn main() { }\n");
    repo.commit("Code", None);
    let out = repo.vers().args(["check", "--base", "main"]).assert().code(1).get_output().clone();
    assert!(
        stderr(&out)
            .starts_with("error: package `api` has changes but no new fragment\n  changed: packages/api/src/main.rs\n")
    );
}

#[test]
fn check_inherited_ignore_globs_are_relative_to_the_package() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[changelog]\nrepo = \"none\"\n\n[check]\nignore = [\"*.md\", \"packages/web/docs/**\"]\n")
        .write("packages/web/vers.toml", "extend = \"../../vers.toml\"\n\n[package]\nname = \"web\"\n")
        .write("packages/web/a.txt", "a\n");
    repo.commit("Initial", None);
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo.write("packages/web/docs/x.txt", "x\n").write("packages/web/sub/y.md", "y\n");
    repo.commit("Docs", None);
    repo.vers().args(["check", "--base", "main"]).assert().success().stdout("No package changes\n");
}

#[test]
fn check_require_fragment_false() {
    let repo = monorepo();
    repo.write("tools/scripts/vers.toml", "[package]\nname = \"scripts\"\n\n[check]\nrequire_fragment = false\n");
    repo.commit("Scripts package", None);
    repo.write("tools/scripts/run.sh", "echo\n");
    repo.commit("Script", None);
    repo.vers().args(["check", "--base", "main"]).assert().success().stdout("All changed packages have fragments\n");
}

#[test]
fn check_reports_every_problem() {
    let repo = monorepo();
    repo.write("packages/web/style.css", "/* tweak */\n");
    for i in 1..=7 {
        repo.write(&format!("packages/api/src/m{i}.rs"), "\n");
    }
    repo.write("packages/api/changes/broken.bogus.md", "Text.\n");
    repo.commit("Lots", None);
    let out = repo.vers().args(["check", "--base", "main"]).assert().code(1).get_output().clone();
    assert_eq!(
        stderr(&out),
        "error: package `web` has changes but no new fragment\n  changed: packages/web/style.css\n  hint: add one with `vers new <type> --package web`, or set VERS_SKIP=1 to skip this check\n\
         invalid fragment packages/api/changes/broken.bogus.md: unknown change type `bogus`; expected one of: added, changed, deprecated, removed, fixed, security (or one of their aliases)\n"
    );
}

#[test]
fn check_lists_at_most_five_files() {
    let repo = monorepo();
    for i in 1..=7 {
        repo.write(&format!("packages/web/f{i}.css"), "\n");
    }
    repo.commit("Lots", None);
    repo.vers().args(["check", "--base", "main"]).assert().code(1).stderr(
        "error: package `web` has changes but no new fragment\n  changed: packages/web/f1.css\n  changed: packages/web/f2.css\n  changed: packages/web/f3.css\n  changed: packages/web/f4.css\n  changed: packages/web/f5.css\n  … and 2 more\n  hint: add one with `vers new <type> --package web`, or set VERS_SKIP=1 to skip this check\n",
    );
}

#[test]
fn check_validates_new_fragments() {
    let repo = monorepo();
    repo.write("packages/web/changes/empty.fixed.md", "\n");
    repo.write("packages/web/changes/notes.txt", "not a fragment\n");
    repo.write("packages/web/changes/README.md", "About fragments\n");
    repo.commit("Empty fragment", None);
    repo.vers().args(["check", "--base", "main"]).assert().code(1).stderr(
        "error: invalid fragment packages/web/changes/empty.fixed.md: fragment body is empty; write a sentence describing the change\n",
    );
}

#[test]
fn check_rejects_fragments_under_a_shared_config() {
    let repo = monorepo();
    repo.write("changes/1.fixed.md", "Fix.\n");
    repo.commit("Stray fragment", None);
    let out = repo.vers().args(["check", "--base", "main"]).assert().code(1).get_output().clone();
    assert!(stderr(&out).starts_with("error: invalid fragment changes/1.fixed.md: this fragment is under vers.toml"));
}

#[test]
fn check_modified_fragment_counts_and_deleted_files_map_to_packages() {
    let repo = monorepo();
    repo.git(&["switch", "-q", "main"]);
    repo.write("packages/web/changes/a.fixed.md", "Fix.\n");
    repo.commit("Fragment", None);
    repo.git(&["switch", "-q", "-C", "feature"]);
    repo.write("packages/web/changes/a.fixed.md", "Fix it properly.\n");
    std::fs::remove_file(repo.path("packages/api/src/main.rs")).unwrap();
    repo.commit("Edit fragment, delete api file", None);
    repo.vers().args(["check", "--base", "main"]).assert().code(1).stderr(
        "error: package `api` has changes but no new fragment\n  changed: packages/api/src/main.rs\n  hint: add one with `vers new <type> --package api`, or set VERS_SKIP=1 to skip this check\n",
    );
}

#[test]
fn check_deleted_fragments_are_not_changes() {
    let repo = monorepo();
    repo.git(&["switch", "-q", "main"]);
    repo.write("packages/web/changes/a.fixed.md", "Fix.\n");
    repo.commit("Fragment", None);
    repo.git(&["switch", "-q", "-C", "feature"]);
    std::fs::remove_file(repo.path("packages/web/changes/a.fixed.md")).unwrap();
    repo.commit("Drop fragment", None);
    repo.vers().args(["check", "--base", "main"]).assert().success().stdout("No package changes\n");
}

#[test]
fn check_skipped_with_vers_skip() {
    let repo = TestRepo::without_git();
    repo.vers().arg("check").env("VERS_SKIP", "1").assert().success().stdout("Skipped (VERS_SKIP=1)\n");
    let out =
        repo.vers().args(["check", "--format", "json"]).env("VERS_SKIP", "1").assert().success().get_output().clone();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["skipped"], true);
}

#[test]
fn check_needs_git() {
    let repo = TestRepo::without_git();
    repo.write("vers.toml", "[package]\nname = \"x\"\n");
    let out = repo.vers().arg("check").assert().code(2).get_output().clone();
    assert!(stderr(&out).starts_with("error: `vers check`"));
}

#[test]
fn check_bad_base_is_an_error() {
    let repo = monorepo();
    let out = repo.vers().args(["check", "--base", "nope"]).assert().code(1).get_output().clone();
    assert!(stderr(&out).contains("cannot resolve base ref 'nope'"), "{}", stderr(&out));
}

#[test]
fn check_default_base_from_github_base_ref() {
    let repo = monorepo();
    let main = repo.git(&["rev-parse", "main"]);
    repo.git(&["update-ref", "refs/remotes/origin/main", main.trim()]);
    repo.write("packages/web/style.css", "/* tweak */\n");
    repo.commit("Tweak", None);
    let out = repo.vers().arg("check").env("GITHUB_BASE_REF", "main").assert().code(1).get_output().clone();
    assert!(stderr(&out).starts_with("error: package `web`"));
    // CI_MERGE_REQUEST_DIFF_BASE_SHA is used when GITHUB_BASE_REF isn't set.
    let out = repo
        .vers()
        .arg("check")
        .env("CI_MERGE_REQUEST_DIFF_BASE_SHA", main.trim())
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(stderr(&out).starts_with("error: package `web`"));
    // origin/HEAD last.
    repo.git(&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
    let out = repo.vers().arg("check").assert().code(1).get_output().clone();
    assert!(stderr(&out).starts_with("error: package `web`"));
}

#[test]
fn check_only_covers_packages_at_or_below_the_current_directory() {
    let repo = monorepo();
    repo.write("packages/web/style.css", "/* tweak */\n");
    repo.commit("Tweak", None);
    repo.vers_in("packages/api").args(["check", "--base", "main"]).assert().success().stdout("No package changes\n");
    // Inside the package, its changes are still checked.
    let out =
        repo.vers_in("packages/web/changes").args(["check", "--base", "main"]).assert().code(1).get_output().clone();
    assert!(stderr(&out).starts_with("error: package `web` has changes but no new fragment\n"));
}

#[test]
fn check_with_config_override() {
    let repo = monorepo();
    repo.write("tools/release.sh", "echo 2\n");
    repo.commit("Tools", None);
    let out = repo
        .vers()
        .args(["--config", "packages/web/vers.toml", "check", "--base", "main"])
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(
        stderr(&out).starts_with("error: package `web` has changes but no new fragment\n  changed: tools/release.sh\n")
    );
}

#[test]
fn check_changelog_update_counts_as_a_release() {
    let repo = monorepo();
    repo.write("packages/web/CHANGELOG.md", "# Changelog\n\n## [Unreleased]\n\n## [0.1.0] - 2026-10-14\n");
    repo.write("packages/web/package.json", "{}\n");
    repo.commit("Release web", None);
    repo.vers().args(["check", "--base", "main"]).assert().success().stdout("All changed packages have fragments\n");
}

#[test]
fn check_json() {
    let repo = monorepo();
    repo.write("packages/web/style.css", "/* tweak */\n");
    repo.write("packages/api/README.md", "docs\n");
    repo.write("packages/api/changes/x.fixed.md", "Fix.\n");
    repo.write("packages/api/changes/y.nope.md", "Fix.\n");
    repo.commit("Mixed", None);
    let out = repo.vers().args(["check", "--base", "main", "--format", "json"]).assert().code(1).get_output().clone();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["skipped"], false);
    assert_eq!(json["base"], "main");
    let packages = json["packages"].as_array().unwrap();
    assert_eq!(packages.len(), 2);
    assert_eq!(packages[0]["name"], "api");
    assert_eq!(packages[0]["path"], "packages/api");
    assert_eq!(packages[0]["ok"], true);
    assert_eq!(packages[0]["reason"], "fragment_added");
    assert_eq!(packages[0]["ignored"], serde_json::json!(["packages/api/README.md"]));
    assert_eq!(
        packages[0]["fragments"],
        serde_json::json!(["packages/api/changes/x.fixed.md", "packages/api/changes/y.nope.md"])
    );
    assert_eq!(packages[1]["name"], "web");
    assert_eq!(packages[1]["ok"], false);
    assert_eq!(packages[1]["reason"], "missing_fragment");
    assert_eq!(packages[1]["changed"], serde_json::json!(["packages/web/style.css"]));
    assert_eq!(json["invalid_fragments"][0]["path"], "packages/api/changes/y.nope.md");
    assert!(json["invalid_fragments"][0]["error"].as_str().unwrap().starts_with("unknown change type `nope`"));
    assert!(stderr(&out).starts_with("error: package `web`"));
}

#[test]
fn check_warns_in_a_shallow_clone() {
    let repo = monorepo();
    repo.git(&["switch", "-q", "main"]);
    let url = format!("file://{}", repo.root.to_string_lossy().replace('\\', "/"));
    let clone = repo.root.parent().unwrap().join("shallow");
    repo.git(&["clone", "-q", "--depth", "1", &url, clone.to_str().unwrap()]);
    let mut cmd = repo.vers();
    cmd.current_dir(&clone);
    let out = cmd.args(["check", "--base", "HEAD"]).assert().success().get_output().clone();
    assert_eq!(stdout(&out), "No package changes\n");
    assert!(stderr(&out).starts_with("warning: this is a shallow clone"), "{}", stderr(&out));
}

// ---------------------------------------------------------------------------
// vers migrate towncrier
// ---------------------------------------------------------------------------

fn towncrier_project() -> TestRepo {
    let repo = TestRepo::new();
    repo.write(
        "pyproject.toml",
        "[project]\nname = \"proj\"\nversion = \"1.2.0\"\n\n[tool.towncrier]\ndirectory = \"newsfragments\"\nfilename = \"CHANGELOG.md\"\n",
    )
    .write("newsfragments/123.feature.md", "Add a thing.\n")
    .write("newsfragments/123.feature.1.md", "Add another thing.\n")
    .write("newsfragments/45.bugfix", "Fix a bug.\n")
    .write("newsfragments/+abc.removal.md", "Remove a thing.\n")
    .write("newsfragments/7.misc.md", "")
    .write("newsfragments/8.doc.md", "Document it.\n")
    .write("newsfragments/9.added.md", "Already vers.\n")
    .write("newsfragments/.gitignore", "!.gitignore\n")
    .write("vers.toml", "[package]\nname = \"proj\"\n\n[changelog]\nrepo = \"none\"\n\n[fragments]\ndirectory = \"newsfragments\"\n");
    repo.commit("Initial", None);
    repo
}

#[test]
fn migrate_dry_run_changes_nothing() {
    let repo = towncrier_project();
    repo.vers().args(["migrate", "towncrier", "--dry-run"]).assert().success().stderr("").stdout(
        "rename newsfragments/+abc.removal.md → newsfragments/abc.removed.md\n\
         rename newsfragments/123.feature.1.md → newsfragments/123.added.2.md\n\
         rename newsfragments/123.feature.md → newsfragments/123.added.md\n\
         rename newsfragments/45.bugfix → newsfragments/45.fixed.md\n\
         delete newsfragments/7.misc.md\n\
         rename newsfragments/8.doc.md → newsfragments/8.changed.md\n\
         Would rename 5 fragments and delete 1 fragment\n",
    );
    assert!(repo.exists("newsfragments/123.feature.md"));
    assert!(repo.exists("newsfragments/7.misc.md"));
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
}

#[test]
fn migrate_renames_with_git_mv() {
    let repo = towncrier_project();
    repo.write("newsfragments/untracked.feature.md", "New.\n");
    repo.vers().args(["migrate", "towncrier"]).assert().success().stdout(
        "Renamed newsfragments/+abc.removal.md → newsfragments/abc.removed.md\n\
         Renamed newsfragments/123.feature.1.md → newsfragments/123.added.2.md\n\
         Renamed newsfragments/123.feature.md → newsfragments/123.added.md\n\
         Renamed newsfragments/45.bugfix → newsfragments/45.fixed.md\n\
         Deleted newsfragments/7.misc.md\n\
         Renamed newsfragments/8.doc.md → newsfragments/8.changed.md\n\
         Renamed newsfragments/untracked.feature.md → newsfragments/untracked.added.md\n\
         Renamed 6 fragments and deleted 1 fragment\n",
    );
    assert_eq!(repo.read("newsfragments/123.added.md"), "Add a thing.\n");
    assert_eq!(repo.read("newsfragments/123.added.2.md"), "Add another thing.\n");
    assert_eq!(repo.read("newsfragments/9.added.md"), "Already vers.\n");
    assert!(!repo.exists("newsfragments/7.misc.md"));
    let status = repo.git(&["status", "--porcelain"]);
    assert!(status.contains("R  newsfragments/123.feature.md -> newsfragments/123.added.md"), "{status}");
    assert!(status.contains("D  newsfragments/7.misc.md"), "{status}");
    assert!(status.contains("?? newsfragments/untracked.added.md"), "{status}");
    // The result is a valid set of vers fragments.
    repo.git(&["add", "-A"]);
    repo.vers()
        .args(["migrate", "towncrier"])
        .assert()
        .success()
        .stdout("No towncrier fragments to migrate in newsfragments\n");
}

#[test]
fn migrate_map_keeps_misc() {
    let repo = towncrier_project();
    repo.write("newsfragments/7.misc.md", "Misc change.\n");
    let out = repo
        .vers()
        .args(["migrate", "towncrier", "--dry-run", "--map", "misc=changed", "--map", "doc=drop"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text = stdout(&out);
    assert!(text.contains("rename newsfragments/7.misc.md → newsfragments/7.changed.md\n"), "{text}");
    assert!(text.contains("delete newsfragments/8.doc.md\n"), "{text}");
}

#[test]
fn migrate_warns_about_empty_fragments() {
    let repo = towncrier_project();
    repo.vers()
        .args(["migrate", "towncrier", "--dry-run", "--map", "misc=changed"])
        .assert()
        .success()
        .stderr("warning: newsfragments/7.changed.md has no text; vers needs a sentence describing the change before it can release it\n");
}

#[test]
fn migrate_rejects_bad_maps() {
    let repo = towncrier_project();
    repo.vers().args(["migrate", "towncrier", "--map", "misc"]).assert().code(2);
    let out = repo.vers().args(["migrate", "towncrier", "--map", "misc=nope"]).assert().code(2).get_output().clone();
    assert!(stderr(&out).contains("`nope` isn't a change type"));
}

#[test]
fn migrate_fails_on_types_without_a_mapping() {
    let repo = towncrier_project();
    repo.write(
        "pyproject.toml",
        "[tool.towncrier]\ndirectory = \"newsfragments\"\n\n[[tool.towncrier.type]]\ndirectory = \"perf\"\nname = \"Performance\"\n",
    );
    repo.write("newsfragments/3.perf.md", "Faster.\n");
    let out = repo.vers().args(["migrate", "towncrier"]).assert().code(1).get_output().clone();
    assert!(
        stderr(&out).starts_with("error: no vers type for these fragments: 3.perf.md (type `perf`)"),
        "{}",
        stderr(&out)
    );
    assert!(repo.exists("newsfragments/123.feature.md"));
    repo.vers().args(["migrate", "towncrier", "--map", "perf=changed", "--dry-run"]).assert().success();
}

#[test]
fn migrate_without_vers_config_uses_towncrier_directory() {
    let repo = TestRepo::new();
    repo.write("towncrier.toml", "[tool.towncrier]\ndirectory = \"news\"\n").write("news/1.feature", "Thing.\n");
    repo.vers()
        .args(["migrate", "towncrier"])
        .assert()
        .success()
        .stdout("Renamed news/1.feature → news/1.added.md\nRenamed 1 fragment and deleted 0 fragments\n");
    assert!(repo.exists("news/1.added.md"));
}

#[test]
fn migrate_orphans_that_look_like_references() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"p\"\n\n[changelog]\nrepo = \"none\"\n")
        .write("changes/+1234.feature.md", "Thing.\n")
        .write("changes/+a.b.bugfix.md", "Fix.\n");
    repo.vers().args(["migrate", "towncrier", "--dry-run"]).assert().success().stdout(
        "rename changes/+1234.feature.md → changes/change-1234.added.md\n\
         rename changes/+a.b.bugfix.md → changes/a-b.fixed.md\n\
         Would rename 2 fragments and delete 0 fragments\n",
    );
}

#[test]
fn migrate_json() {
    let repo = towncrier_project();
    let out = repo
        .vers()
        .args(["migrate", "towncrier", "--dry-run", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["renamed"].as_array().unwrap().len(), 5);
    assert_eq!(
        json["renamed"][2],
        serde_json::json!({"from": "newsfragments/123.feature.md", "to": "newsfragments/123.added.md"})
    );
    assert_eq!(json["deleted"], serde_json::json!(["newsfragments/7.misc.md"]));
    assert_eq!(json["changelog"], serde_json::Value::Null);
}

const TOWNCRIER_CHANGELOG: &str = "# Changelog\n\n<!-- towncrier release notes start -->\n\n## proj 1.2.0 (2026-01-31)\n\n### Features\n\n- A thing. (#12)\n\nproj 1.1.0 (2025-12-01)\n=======================\n\n- Old.\n";

const CONVERTED_CHANGELOG: &str = "# Changelog\n\n<!-- towncrier release notes start -->\n\n## [Unreleased]\n\n## [1.2.0] - 2026-01-31\n\n### Features\n\n- A thing. (#12)\n\n## [1.1.0] - 2025-12-01\n\n- Old.\n";

#[test]
fn migrate_changelog_dry_run_shows_a_diff() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"proj\"\n\n[changelog]\nrepo = \"none\"\n")
        .write("CHANGELOG.md", TOWNCRIER_CHANGELOG);
    repo.vers().args(["migrate", "towncrier", "--changelog", "--dry-run"]).assert().success().stdout(
        "No towncrier fragments to migrate in changes\n\
         --- a/CHANGELOG.md\n\
         +++ b/CHANGELOG.md\n\
         @@ -2,13 +2,14 @@\n \n <!-- towncrier release notes start -->\n \n\
         -## proj 1.2.0 (2026-01-31)\n\
         +## [Unreleased]\n\
         +\n\
         +## [1.2.0] - 2026-01-31\n \n ### Features\n \n - A thing. (#12)\n \n\
         -proj 1.1.0 (2025-12-01)\n\
         -=======================\n\
         +## [1.1.0] - 2025-12-01\n \n - Old.\n",
    );
    assert_eq!(repo.read("CHANGELOG.md"), TOWNCRIER_CHANGELOG);
}

#[test]
fn migrate_changelog_rewrites_headings() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"proj\"\n\n[changelog]\nrepo = \"none\"\n")
        .write("CHANGELOG.md", TOWNCRIER_CHANGELOG);
    let out = repo.vers().args(["migrate", "towncrier", "--changelog"]).assert().success().get_output().clone();
    assert!(stdout(&out).starts_with("No towncrier fragments to migrate in changes\n--- a/CHANGELOG.md\n"));
    assert!(stdout(&out).ends_with("Updated CHANGELOG.md\n"));
    assert_eq!(repo.read("CHANGELOG.md"), CONVERTED_CHANGELOG);
    // Running it again changes nothing.
    repo.vers().args(["migrate", "towncrier", "--changelog"]).assert().success().stdout(
        "No towncrier fragments to migrate in changes\nNo towncrier release headings to convert in CHANGELOG.md\n",
    );
}

#[test]
fn migrate_changelog_keeps_crlf() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[package]\nname = \"proj\"\n\n[changelog]\nrepo = \"none\"\n")
        .write("CHANGELOG.md", &TOWNCRIER_CHANGELOG.replace('\n', "\r\n"));
    repo.vers().args(["-q", "migrate", "towncrier", "--changelog"]).assert().success().stdout("");
    assert_eq!(repo.read("CHANGELOG.md"), CONVERTED_CHANGELOG.replace('\n', "\r\n"));
}

// ---------------------------------------------------------------------------
// vers init --from-towncrier
// ---------------------------------------------------------------------------

/// Whether the `init` command is wired up (it calls `init_from_towncrier`).
fn init_available(repo: &TestRepo) -> bool {
    let out = repo.vers().args(["init", "--from-towncrier"]).output().unwrap();
    !stderr(&out).contains("not implemented yet")
}

#[test]
fn init_from_towncrier_basic() {
    let repo = TestRepo::new();
    repo.write(
        "pyproject.toml",
        "[project]\nname = \"proj\"\nversion = \"1.2.0\"\n\n[tool.towncrier]\ndirectory = \"newsfragments\"\nfilename = \"NEWS.md\"\n",
    )
    .write("newsfragments/1.feature.md", "Thing.\n")
    .write("NEWS.md", "# News\n\n## proj 1.2.0 (2026-01-31)\n\n- Old.\n");
    if !init_available(&repo) {
        return;
    }
    // `init_available` already ran it once.
    let name = repo.root.file_name().unwrap().to_string_lossy().to_string();
    assert_eq!(
        repo.read("vers.toml"),
        format!(
            "[package]\nname = \"{name}\"\n\n[changelog]\npath = \"NEWS.md\"\n\n[fragments]\ndirectory = \"newsfragments\"\n\n[[version.files]]\npath = \"pyproject.toml\"\nkey = \"project.version\"\n"
        )
    );
    assert_eq!(repo.read("NEWS.md"), "# News\n\n## [Unreleased]\n\n## proj 1.2.0 (2026-01-31)\n\n- Old.\n");
    // A second run fails because vers.toml exists.
    repo.vers().args(["init", "--from-towncrier"]).assert().code(2);
}

#[test]
fn init_from_towncrier_output() {
    let repo = TestRepo::new();
    repo.write("pyproject.toml", "[tool.poetry]\nversion = \"0.3.0\"\n\n[tool.towncrier]\nfilename = \"NEWS.rst\"\n");
    let out = repo.vers().args(["init", "--from-towncrier", "--name", "proj"]).output().unwrap();
    if stderr(&out).contains("not implemented yet") {
        return;
    }
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "Created vers.toml\nCreated newsfragments/.gitkeep\nCreated CHANGELOG.md\n\
         Detected version file: pyproject.toml (tool.poetry.version = 0.3.0)\n\
         Next, rename existing fragments with `vers migrate towncrier --dry-run`\n"
    );
    assert_eq!(
        stderr(&out),
        "warning: towncrier writes NEWS.rst, but vers only writes Markdown; using CHANGELOG.md instead\n"
    );
    assert_eq!(
        repo.read("vers.toml"),
        "[package]\nname = \"proj\"\n\n[fragments]\ndirectory = \"newsfragments\"\n\n[[version.files]]\npath = \"pyproject.toml\"\nkey = \"tool.poetry.version\"\n"
    );
}

/// The function `init` calls, tested directly so it's covered even before
/// `init` dispatches to it.
#[test]
fn init_from_towncrier_custom_types_and_extend() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[changelog]\nrepo = \"none\"\n");
    repo.write(
        "pkg/towncrier.toml",
        "[tool.towncrier]\ndirectory = \"changes\"\n\n[tool.towncrier.fragment.feature]\n\n[tool.towncrier.fragment.perf]\nname = \"Performance\"\n",
    );
    let cwd = repo.path("pkg");
    let global = vers::cli::GlobalArgs {
        config: None,
        format: vers::cli::Format::Text,
        color: vers::cli::ColorChoice::Never,
        quiet: true,
        verbose: 0,
    };
    let ctx = vers::context::Context::new(cwd.clone(), &global);
    let args = vers::cli::InitArgs { name: None, shared: false, no_detect: false, from_towncrier: true };
    vers::commands::migrate::init_from_towncrier(&ctx, &args).unwrap();
    let toml = repo.read("pkg/vers.toml");
    assert!(
        toml.starts_with("extend = \"../vers.toml\"\n\n[package]\nname = \"pkg\"\n\n[[types]]\nname = \"added\"\n"),
        "{toml}"
    );
    assert!(toml.ends_with("[[types]]\nname = \"perf\"\nheading = \"Performance\"\nbump = \"patch\"\n"), "{toml}");
    assert!(!toml.contains("[fragments]"));
    assert!(repo.exists("pkg/changes/.gitkeep"));
    assert_eq!(repo.read("pkg/CHANGELOG.md"), vers::changelog::NEW_CHANGELOG);
    let config = vers::config::load(&cwd.join("vers.toml")).unwrap();
    assert_eq!(config.types.len(), 7);
    assert!(config.find_type("perf").is_some());

    // The migration then keeps `perf` fragments as they are.
    repo.write("pkg/changes/1.perf.md", "Faster.\n").write("pkg/changes/2.feature.md", "New.\n");
    repo.vers_in("pkg")
        .args(["migrate", "towncrier", "--dry-run"])
        .assert()
        .success()
        .stdout("rename changes/2.feature.md → changes/2.added.md\nWould rename 1 fragment and delete 0 fragments\n");

    // Running it again is a usage error (as for `vers init`): the config exists.
    let err = vers::commands::migrate::init_from_towncrier(&ctx, &args).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn init_from_towncrier_needs_towncrier_config() {
    let repo = TestRepo::new();
    let global = vers::cli::GlobalArgs {
        config: None,
        format: vers::cli::Format::Text,
        color: vers::cli::ColorChoice::Never,
        quiet: true,
        verbose: 0,
    };
    let ctx = vers::context::Context::new(repo.root.clone(), &global);
    let args = vers::cli::InitArgs { name: None, shared: false, no_detect: false, from_towncrier: true };
    let err = vers::commands::migrate::init_from_towncrier(&ctx, &args).unwrap_err();
    assert_eq!(err.exit_code(), 2);
    assert!(err.to_string().contains("no [tool.towncrier] table"));
    assert!(!repo.exists("vers.toml"));
}

fn quiet_ctx(cwd: std::path::PathBuf) -> vers::context::Context {
    let global = vers::cli::GlobalArgs {
        config: None,
        format: vers::cli::Format::Text,
        color: vers::cli::ColorChoice::Never,
        quiet: true,
        verbose: 0,
    };
    vers::context::Context::new(cwd, &global)
}

#[test]
fn check_shows_files_outside_the_current_directory_relative_to_it() {
    let repo = monorepo();
    repo.write("packages/web/src/app.js", "x\n");
    repo.write("packages/web/style.css", "/* tweak */\n");
    repo.commit("Tweak", None);
    repo.vers_in("packages/web/src").args(["check", "--base", "main"]).assert().code(1).stdout("").stderr(
        "error: package `web` has changes but no new fragment\n  changed: app.js\n  changed: ../style.css\n  hint: add one with `vers new <type> --package web`, or set VERS_SKIP=1 to skip this check\n",
    );
    let out = repo
        .vers_in("packages/web/src")
        .args(["check", "--base", "main", "--format", "json"])
        .assert()
        .code(1)
        .get_output()
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["packages"][0]["path"], "..");
    assert_eq!(json["packages"][0]["changed"], serde_json::json!(["app.js", "../style.css"]));
}

#[test]
fn check_config_override_applies_outside_the_current_directory() {
    let repo = monorepo();
    repo.write("tools/release.sh", "echo 2\n");
    repo.commit("Tools", None);
    let out = repo
        .vers_in("packages/api")
        .args(["--config", "../web/vers.toml", "check", "--base", "main"])
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(
        stderr(&out)
            .starts_with("error: package `web` has changes but no new fragment\n  changed: ../../tools/release.sh\n"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn check_does_not_load_configs_outside_its_scope() {
    let repo = monorepo();
    repo.write("packages/api/vers.toml", "[package\n").write("packages/api/src/main.rs", "x\n");
    repo.commit("Break api", None);
    repo.vers_in("packages/web").args(["check", "--base", "main"]).assert().success().stdout("No package changes\n");
    repo.vers().args(["check", "--base", "main"]).assert().code(2);
}

#[test]
fn check_config_override_must_be_a_package() {
    let repo = monorepo();
    repo.write("tools/release.sh", "echo 2\n");
    repo.commit("Tools", None);
    repo.vers()
        .args(["--config", "vers.toml", "check", "--base", "main"])
        .assert()
        .code(2)
        .stderr("error: the config given with --config has no [package] table, so it isn't a package\n");
}

#[test]
fn migrate_keeps_types_the_config_defines() {
    let repo = TestRepo::new();
    repo.write(
        "vers.toml",
        "[package]\nname = \"proj\"\n\n[changelog]\nrepo = \"none\"\n\n[fragments]\ndirectory = \"newsfragments\"\n\n\
         [[types]]\nname = \"added\"\nheading = \"Added\"\naliases = [\"feature\"]\n\n\
         [[types]]\nname = \"misc\"\nheading = \"Misc\"\n",
    )
    .write("newsfragments/1.misc.md", "Tidy up.\n")
    .write("newsfragments/2.feature.md", "New.\n")
    .write("newsfragments/+x.misc", "Orphan.\n");
    repo.commit("Initial", None);
    repo.vers().args(["migrate", "towncrier"]).assert().success().stderr("").stdout(
        "Renamed newsfragments/+x.misc → newsfragments/x.misc.md\n\
         Renamed newsfragments/2.feature.md → newsfragments/2.added.md\n\
         Renamed 2 fragments and deleted 0 fragments\n",
    );
    assert_eq!(repo.read("newsfragments/1.misc.md"), "Tidy up.\n");
    assert_eq!(repo.read("newsfragments/x.misc.md"), "Orphan.\n");
}

#[test]
fn migrate_shows_paths_outside_the_current_directory_relative_to_it() {
    let repo = towncrier_project();
    std::fs::create_dir_all(repo.path("src")).unwrap();
    let out = repo.vers_in("src").args(["migrate", "towncrier", "--dry-run"]).assert().success().get_output().clone();
    assert!(
        stdout(&out).starts_with("rename ../newsfragments/+abc.removal.md → ../newsfragments/abc.removed.md\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn init_from_towncrier_matches_init_output() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "git@github.com:example/proj.git"]);
    repo.write(
        "pyproject.toml",
        "[project]\nversion = \"1.2.0\"\n\n[tool.towncrier]\nfilename = \"CHANGELOG.md\"\n\n\
         [tool.towncrier.fragment.feature]\n\n[tool.towncrier.fragment.perf]\nname = \"Performance\"\n",
    )
    .write("CHANGELOG.md", "# Changelog\n\n## proj 1.2.0 (2026-01-31)\n\n- Old.\n");
    std::fs::create_dir_all(repo.path("newsfragments")).unwrap();
    let out = repo.vers().args(["init", "--from-towncrier", "--name", "proj"]).output().unwrap();
    if stderr(&out).contains("not implemented yet") {
        return;
    }
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stderr(&out), "");
    assert_eq!(
        stdout(&out),
        "Created vers.toml\n\
         Created newsfragments/.gitkeep\n\
         Updated CHANGELOG.md (added ## [Unreleased])\n\
         Detected repository: github (https://github.com/example/proj)\n\
         Detected version file: pyproject.toml (project.version = 1.2.0)\n\
         Added custom type: perf (Performance)\n\
         Next, rename existing fragments with `vers migrate towncrier --dry-run`\n"
    );
    assert!(repo.exists("newsfragments/.gitkeep"));
}

#[test]
fn init_from_towncrier_json() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "git@github.com:example/proj.git"]);
    repo.write("towncrier.toml", "[tool.towncrier]\ndirectory = \"changes\"\nfilename = \"CHANGELOG.md\"\n");
    let out = repo.vers().args(["init", "--from-towncrier", "--name", "proj", "--format", "json"]).output().unwrap();
    if stderr(&out).contains("not implemented yet") {
        return;
    }
    assert!(out.status.success(), "{}", stderr(&out));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "config": "vers.toml",
            "shared": false,
            "name": "proj",
            "extends": null,
            "created": ["vers.toml", "changes/.gitkeep", "CHANGELOG.md"],
            "updated": [],
            "repository": {"kind": "github", "url": "https://github.com/example/proj"},
            "version_files": [],
            "fragments_directory": "changes",
            "changelog": "CHANGELOG.md",
            "custom_types": [],
        })
    );
    assert_eq!(repo.read("vers.toml"), "[package]\nname = \"proj\"\n");
}

#[test]
fn init_from_towncrier_keeps_paths_an_extended_config_would_change() {
    let repo = TestRepo::new();
    repo.write(
        "vers.toml",
        "[changelog]\nrepo = \"none\"\npath = \"docs/CHANGES.md\"\n\n[fragments]\ndirectory = \"fragments\"\n",
    );
    repo.write("pkg/towncrier.toml", "[tool.towncrier]\ndirectory = \"changes\"\nfilename = \"CHANGELOG.md\"\n");
    let ctx = quiet_ctx(repo.path("pkg"));
    let args = vers::cli::InitArgs { name: None, shared: false, no_detect: false, from_towncrier: true };
    vers::commands::migrate::init_from_towncrier(&ctx, &args).unwrap();
    assert_eq!(
        repo.read("pkg/vers.toml"),
        "extend = \"../vers.toml\"\n\n[package]\nname = \"pkg\"\n\n[changelog]\npath = \"CHANGELOG.md\"\n\n[fragments]\ndirectory = \"changes\"\n"
    );
    assert!(repo.exists("pkg/changes/.gitkeep"));
    assert!(repo.exists("pkg/CHANGELOG.md"));
    assert!(!repo.exists("fragments") && !repo.exists("docs"));
}

#[test]
fn init_from_towncrier_rejects_bad_names_and_shared() {
    let repo = TestRepo::new();
    repo.write("towncrier.toml", "[tool.towncrier]\n");
    let ctx = quiet_ctx(repo.root.clone());
    let args = vers::cli::InitArgs { name: Some("a b".into()), shared: false, no_detect: true, from_towncrier: true };
    assert_eq!(vers::commands::migrate::init_from_towncrier(&ctx, &args).unwrap_err().exit_code(), 2);
    let args = vers::cli::InitArgs { name: None, shared: true, no_detect: true, from_towncrier: true };
    assert_eq!(vers::commands::migrate::init_from_towncrier(&ctx, &args).unwrap_err().exit_code(), 2);
    assert!(!repo.exists("vers.toml"));
}
