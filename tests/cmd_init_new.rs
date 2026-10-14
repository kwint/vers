//! Integration tests for `vers init` and `vers new`.

mod common;

use common::TestRepo;

const GITHUB_REMOTE: &str = "https://github.com/example/greeter.git";

fn new_changelog_ok(text: &str) {
    assert!(text.starts_with("# Changelog\n"), "{text}");
    assert!(text.ends_with("## [Unreleased]\n"), "{text}");
}

fn stdout(out: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&out.get_output().stdout).to_string()
}

// ---------------------------------------------------------------------------
// vers init

#[test]
fn init_first_release_tutorial() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", GITHUB_REMOTE]);
    repo.write("VERSION", "0.1.0\n");

    repo.vers().arg("init").assert().success().stderr("").stdout(
        "Created vers.toml\n\
         Created changes/.gitkeep\n\
         Created CHANGELOG.md\n\
         Detected repository: github (https://github.com/example/greeter)\n\
         Detected version file: VERSION (0.1.0)\n",
    );
    let dir_name = repo.root.file_name().unwrap().to_string_lossy().to_string();
    assert_eq!(
        repo.read("vers.toml"),
        format!("[package]\nname = \"{dir_name}\"\n\n[[version.files]]\npath = \"VERSION\"\n")
    );
    assert_eq!(repo.read("changes/.gitkeep"), "");
    new_changelog_ok(&repo.read("CHANGELOG.md"));

    // The created config is usable straight away.
    repo.vers().args(["new", "fixed", "7", "-m", "Don't crash."]).assert().success();
}

#[test]
fn init_monorepo_tutorial() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", "https://gitlab.com/example/shop.git"]);
    repo.write("packages/api/Cargo.toml", "[package]\nname = \"api\"\nversion = \"1.4.0\"\n");
    repo.write("packages/web/package.json", "{ \"name\": \"web\", \"version\": \"0.3.0\" }\n");

    repo.vers().args(["init", "--shared"]).assert().success().stdout(
        "Created vers.toml (shared config, not a package)\n\
         Detected repository: gitlab (https://gitlab.com/example/shop)\n",
    );
    assert_eq!(
        repo.read("vers.toml"),
        "[changelog]\nrepo = \"gitlab\"\nrepo_url = \"https://gitlab.com/example/shop\"\n\n\
         [version]\ntag_format = \"{name}@v{version}\"\n"
    );
    assert!(!repo.exists("changes"));
    assert!(!repo.exists("CHANGELOG.md"));

    repo.vers_in("packages/api").arg("init").assert().success().stdout(
        "Created vers.toml (extends ../../vers.toml)\n\
         Created changes/.gitkeep\n\
         Created CHANGELOG.md\n\
         Detected version file: Cargo.toml (package.version = 1.4.0)\n",
    );
    assert_eq!(
        repo.read("packages/api/vers.toml"),
        "extend = \"../../vers.toml\"\n\n[package]\nname = \"api\"\n\n\
         [[version.files]]\npath = \"Cargo.toml\"\nkey = \"package.version\"\n"
    );

    repo.vers_in("packages/web").arg("init").assert().success().stdout(
        "Created vers.toml (extends ../../vers.toml)\n\
         Created changes/.gitkeep\n\
         Created CHANGELOG.md\n\
         Detected version file: package.json (version = 0.3.0)\n",
    );
    assert_eq!(
        repo.read("packages/web/vers.toml"),
        "extend = \"../../vers.toml\"\n\n[package]\nname = \"web\"\n\n\
         [[version.files]]\npath = \"package.json\"\nkey = \"version\"\n"
    );

    // Step 4 of the tutorial.
    repo.vers_in("packages/api")
        .args(["new", "added", "mr-12", "-m", "Add the /orders endpoint."])
        .assert()
        .success()
        .stdout("Created changes/mr-12.added.md\n");
    repo.vers_in("packages/web")
        .args(["new", "fixed", "30", "-m", "Fix the cart total rounding."])
        .assert()
        .success()
        .stdout("Created changes/30.fixed.md\n");
    repo.vers()
        .args(["new", "changed", "--package", "api", "-m", "Return 404 instead of 400 for unknown orders."])
        .assert()
        .success()
        .stdout("Created packages/api/changes/return-404-instead-of-400-for.changed.md\n");
}

#[test]
fn init_fails_when_a_config_exists() {
    for name in ["vers.toml", ".vers.toml"] {
        let repo = TestRepo::new();
        repo.write(name, "[package]\nname = \"x\"\n");
        repo.vers().arg("init").assert().code(2).stdout("").stderr(format!(
            "error: {name} already exists in this directory; edit it instead of running `vers init` again\n"
        ));
        assert_eq!(repo.read(name), "[package]\nname = \"x\"\n");
        assert!(!repo.exists("changes"));
    }
}

#[test]
fn init_shared_without_remote() {
    let repo = TestRepo::new();
    repo.vers()
        .args(["init", "--shared"])
        .assert()
        .success()
        .stdout("Created vers.toml (shared config, not a package)\n");
    assert_eq!(repo.read("vers.toml"), "[version]\ntag_format = \"{name}@v{version}\"\n");
}

#[test]
fn init_shared_extending_another_config() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", GITHUB_REMOTE]);
    repo.write("vers.toml", "[version]\ntag_format = \"{name}@v{version}\"\n");
    repo.write("group/.keep", "");
    repo.vers_in("group")
        .args(["init", "--shared"])
        .assert()
        .success()
        .stdout("Created vers.toml (shared config, extends ../vers.toml)\n");
    assert_eq!(
        repo.read("group/vers.toml"),
        "extend = \"../vers.toml\"\n\n[version]\ntag_format = \"{name}@v{version}\"\n"
    );
}

#[test]
fn init_extends_a_hidden_parent_config() {
    let repo = TestRepo::new();
    repo.write(".vers.toml", "[version]\ntag_format = \"{name}@v{version}\"\n");
    repo.write("a/b/.keep", "");
    repo.vers_in("a/b").arg("init").assert().success().stdout(
        "Created vers.toml (extends ../../.vers.toml)\n\
         Created changes/.gitkeep\n\
         Created CHANGELOG.md\n",
    );
    assert!(repo.read("a/b/vers.toml").starts_with("extend = \"../../.vers.toml\"\n\n[package]\nname = \"b\"\n"));
}

#[test]
fn init_no_detect() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", GITHUB_REMOTE]);
    repo.write("VERSION", "0.1.0\n");
    repo.vers().args(["init", "--no-detect", "--name", "app"]).assert().success().stdout(
        "Created vers.toml\n\
         Created changes/.gitkeep\n\
         Created CHANGELOG.md\n",
    );
    assert_eq!(repo.read("vers.toml"), "[package]\nname = \"app\"\n");
}

#[test]
fn init_without_git_or_with_unknown_host() {
    let plain = TestRepo::without_git();
    plain.vers().arg("init").assert().success().stdout(
        "Created vers.toml\n\
         Created changes/.gitkeep\n\
         Created CHANGELOG.md\n",
    );

    let selfhosted = TestRepo::new();
    selfhosted.git(&["remote", "add", "origin", "git@gitlab.example.com:team/project.git"]);
    selfhosted
        .vers()
        .args(["init", "--shared"])
        .assert()
        .success()
        .stdout("Created vers.toml (shared config, not a package)\n");
}

#[test]
fn init_rejects_invalid_package_names() {
    let repo = TestRepo::new();
    repo.vers().args(["init", "--name", "my app"]).assert().code(2).stdout("");
    assert!(!repo.exists("vers.toml"));

    repo.write("my dir/.keep", "");
    repo.vers_in("my dir").arg("init").assert().code(2);
    repo.vers_in("my dir").args(["init", "--name", "mydir"]).assert().success();
    assert!(repo.read("my dir/vers.toml").contains("name = \"mydir\""));
}

#[test]
fn init_keeps_existing_changes_and_changelog() {
    let repo = TestRepo::new();
    repo.write("changes/1.fixed.md", "Fix.\n");
    let changelog = "# Changelog\n\n## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n### Added\n\n- First.\n";
    repo.write("CHANGELOG.md", changelog);
    repo.vers().arg("init").assert().success().stdout("Created vers.toml\n");
    assert!(!repo.exists("changes/.gitkeep"));
    assert_eq!(repo.read("CHANGELOG.md"), changelog);
}

#[test]
fn init_adds_unreleased_to_an_existing_changelog() {
    let repo = TestRepo::new();
    repo.write("changes/.gitkeep", "");
    repo.write("CHANGELOG.md", "# Changelog\n\n## [1.0.0] - 2026-01-01\n\n### Added\n\n- First.\n");
    repo.vers()
        .arg("init")
        .assert()
        .success()
        .stdout("Created vers.toml\nUpdated CHANGELOG.md (added ## [Unreleased])\n");
    assert_eq!(
        repo.read("CHANGELOG.md"),
        "# Changelog\n\n## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n### Added\n\n- First.\n"
    );
}

#[test]
fn init_detects_structured_version_files() {
    let repo = TestRepo::without_git();
    repo.write(
        "Cargo.toml",
        "[package]\nname = \"x\"\nversion.workspace = true\n\n[workspace.package]\nversion = \"2.0.0\"\n",
    );
    repo.write("pyproject.toml", "[tool.poetry]\nname = \"x\"\nversion = \"2.0.0\"\n");
    repo.write("Chart.yaml", "apiVersion: v2\nname: x\nversion: 2.0.0\n");
    repo.write("pubspec.yaml", "name: x\nversion: 2.0.0\n");
    repo.write("package.json", "{ \"name\": \"x\" }\n");
    repo.vers().arg("init").assert().success().stdout(
        "Created vers.toml\n\
         Created changes/.gitkeep\n\
         Created CHANGELOG.md\n\
         Detected version file: Cargo.toml (workspace.package.version = 2.0.0)\n\
         Detected version file: pyproject.toml (tool.poetry.version = 2.0.0)\n\
         Detected version file: Chart.yaml (version = 2.0.0)\n\
         Detected version file: pubspec.yaml (version = 2.0.0)\n",
    );
    let config = repo.read("vers.toml");
    assert!(config.contains("[[version.files]]\npath = \"Cargo.toml\"\nkey = \"workspace.package.version\"\n"));
    assert!(config.contains("[[version.files]]\npath = \"pyproject.toml\"\nkey = \"tool.poetry.version\"\n"));
    assert!(config.contains("[[version.files]]\npath = \"Chart.yaml\"\nkey = \"version\"\n"));
    assert!(!config.contains("package.json"));
    // The config is valid and the versions read back.
    repo.vers().args(["new", "fixed", "-m", "Fix."]).assert().success();
}

#[test]
fn init_detects_files_by_pattern() {
    let repo = TestRepo::without_git();
    repo.write("mix.exs", "def project do\n  [app: :x, version: \"1.2.3\"]\nend\n");
    repo.write("x.gemspec", "Gem::Specification.new do |s|\n  s.version = '1.2.3'\nend\n");
    repo.write("build.gradle.kts", "group = \"com.example\"\nversion = \"1.2.3\"\n");
    repo.write("pom.xml", "<project>\n  <version>1.2.3</version>\n</project>\n");
    repo.write("setup.cfg", "[metadata]\nname = x\nversion = 1.2.3\n");
    repo.vers().arg("init").assert().success().stdout(
        "Created vers.toml\n\
         Created changes/.gitkeep\n\
         Created CHANGELOG.md\n\
         Detected version file: setup.cfg (1.2.3)\n\
         Detected version file: mix.exs (1.2.3)\n\
         Detected version file: x.gemspec (1.2.3)\n\
         Detected version file: build.gradle.kts (1.2.3)\n\
         Detected version file: pom.xml (1.2.3)\n",
    );
    let config = repo.read("vers.toml");
    assert!(config.contains("path = \"mix.exs\"\npattern = '\\bversion:\\s*\"(?<version>[^\"]+)\"'\n"), "{config}");
    // A pattern containing a single quote is written as a basic string.
    assert!(config.contains("path = \"x.gemspec\"\npattern = \""), "{config}");
    repo.vers().args(["new", "fixed", "-m", "Fix."]).assert().success();
}

#[test]
fn init_skips_ambiguous_and_invalid_versions() {
    let repo = TestRepo::without_git();
    repo.write(
        "pom.xml",
        "<project>\n  <version>1.2.3</version>\n  <dependency><version>4.5.6</version></dependency>\n</project>\n",
    );
    repo.write("VERSION", "not a version\n");
    repo.write("package.json", "{ \"version\": \"v1.0\" }\n");
    repo.vers().arg("init").assert().success().stdout(
        "Created vers.toml\n\
         Created changes/.gitkeep\n\
         Created CHANGELOG.md\n",
    );
    assert!(!repo.read("vers.toml").contains("version.files"));
}

#[test]
fn init_warns_when_detected_versions_disagree() {
    let repo = TestRepo::without_git();
    repo.write("package.json", "{ \"version\": \"1.0.0\" }\n");
    repo.write("VERSION", "1.1.0\n");
    repo.vers()
        .arg("init")
        .assert()
        .success()
        .stderr(predicates::str::starts_with("warning: the detected version files disagree"));
}

#[test]
fn init_json() {
    let repo = TestRepo::new();
    repo.git(&["remote", "add", "origin", GITHUB_REMOTE]);
    repo.write("VERSION", "0.1.0\n");
    let out = repo.vers().args(["init", "--format", "json", "--name", "greeter"]).assert().success();
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "config": "vers.toml",
            "shared": false,
            "name": "greeter",
            "extends": null,
            "created": ["vers.toml", "changes/.gitkeep", "CHANGELOG.md"],
            "updated": [],
            "repository": { "kind": "github", "url": "https://github.com/example/greeter" },
            "version_files": [{ "path": "VERSION", "key": null, "pattern": null, "version": "0.1.0" }]
        })
    );
}

// ---------------------------------------------------------------------------
// vers new

fn package(repo: &TestRepo) {
    repo.git(&["remote", "add", "origin", GITHUB_REMOTE]);
    repo.write("vers.toml", "[package]\nname = \"greeter\"\n");
}

#[test]
fn new_from_message_and_reference() {
    let repo = TestRepo::new();
    package(&repo);
    repo.vers()
        .args(["new", "added", "-m", "Greet people by name."])
        .assert()
        .success()
        .stderr("")
        .stdout("Created changes/greet-people-by-name.added.md\n");
    assert_eq!(repo.read("changes/greet-people-by-name.added.md"), "Greet people by name.\n");

    repo.vers()
        .args(["new", "fixed", "7", "-m", "Don't crash on an empty name."])
        .assert()
        .success()
        .stdout("Created changes/7.fixed.md\n");
    assert_eq!(repo.read("changes/7.fixed.md"), "Don't crash on an empty name.\n");
}

#[test]
fn new_resolves_aliases_and_adds_counters() {
    let repo = TestRepo::new();
    package(&repo);
    repo.vers().args(["new", "fix", "123", "-m", "One."]).assert().success().stdout("Created changes/123.fixed.md\n");
    repo.vers()
        .args(["new", "BugFix", "123", "-m", "Two."])
        .assert()
        .success()
        .stdout("Created changes/123.fixed.2.md\n");
    repo.vers()
        .args(["new", "feat", "123", "-m", "Three."])
        .assert()
        .success()
        .stdout("Created changes/123.added.md\n");
}

#[test]
fn new_extra_refs_and_breaking() {
    let repo = TestRepo::new();
    package(&repo);
    repo.vers()
        .args(["new", "fixed", "123", "--ref", "pr-130", "-m", "Fix the login redirect."])
        .assert()
        .success()
        .stdout("Created changes/123.fixed.md\n");
    assert_eq!(repo.read("changes/123.fixed.md"), "---\nrefs: [\"pr-130\"]\n---\nFix the login redirect.\n");

    repo.vers()
        .args(["new", "changed", "--breaking", "-m", "The `timeout` setting is now in seconds."])
        .assert()
        .success()
        .stdout("Created changes/the-timeout-setting-is-now-in.changed.md\n");
    assert_eq!(
        repo.read("changes/the-timeout-setting-is-now-in.changed.md"),
        "---\nbreaking: true\n---\nThe `timeout` setting is now in seconds.\n"
    );
}

#[test]
fn new_warns_about_unmatched_extra_refs() {
    let repo = TestRepo::new();
    package(&repo);
    repo.vers()
        .args(["new", "fixed", "--ref", "PROJ-42", "--ref", "see-docs", "-m", "Fix."])
        .assert()
        .success()
        .stdout("Created changes/fix.fixed.md\n")
        .stderr(
            "warning: reference `PROJ-42` matches no reference kind, so it will be shown as plain text without a link\n",
        );
    assert_eq!(repo.read("changes/fix.fixed.md"), "---\nrefs: [\"PROJ-42\", \"see-docs\"]\n---\nFix.\n");
}

#[test]
fn new_rejects_unknown_types() {
    let repo = TestRepo::new();
    package(&repo);
    repo.vers().args(["new", "bogus", "-m", "X."]).assert().code(2).stdout("").stderr(
        "error: unknown change type `bogus`; expected one of: added, changed, deprecated, removed, fixed, security (or one of their aliases)\n",
    );
    assert!(!repo.exists("changes"));
}

#[test]
fn new_without_type_and_no_terminal() {
    let repo = TestRepo::new();
    package(&repo);
    repo.vers()
        .args(["new", "-m", "X."])
        .assert()
        .code(2)
        .stderr("error: missing change type; pass one of: added, changed, deprecated, removed, fixed, security\n");
}

#[test]
fn new_rejects_bad_names() {
    let repo = TestRepo::new();
    package(&repo);
    repo.vers()
        .args(["new", "fixed", "a.b", "-m", "X."])
        .assert()
        .code(2)
        .stderr(predicates::str::starts_with("error: invalid fragment name `a.b`"));
    // Looks like a reference, but GitHub has no `mr-` kind.
    repo.vers().args(["new", "fixed", "mr-45", "-m", "X."]).assert().code(2).stderr(predicates::str::starts_with(
        "error: `mr-45` looks like a reference but matches no reference kind (configured kinds: `issue`",
    ));
    assert!(!repo.exists("changes"));

    // A slug is fine.
    repo.vers()
        .args(["new", "added", "oauth-login", "-m", "X."])
        .assert()
        .success()
        .stdout("Created changes/oauth-login.added.md\n");
}

#[test]
fn new_message_from_stdin() {
    let repo = TestRepo::new();
    package(&repo);
    repo.vers()
        .args(["new", "fixed"])
        .write_stdin("Fix the thing.\nWith more detail.\n\n")
        .assert()
        .success()
        .stdout("Created changes/fix-the-thing-with-more-detail.fixed.md\n");
    assert_eq!(repo.read("changes/fix-the-thing-with-more-detail.fixed.md"), "Fix the thing.\nWith more detail.\n");
}

#[test]
fn new_empty_message() {
    let repo = TestRepo::new();
    package(&repo);
    repo.vers()
        .args(["new", "fixed"])
        .write_stdin("  \n")
        .assert()
        .code(2)
        .stderr("error: the fragment message is empty; describe the change\n");
    repo.vers().args(["new", "fixed", "-m", ""]).assert().code(2);
    assert!(!repo.exists("changes/change.fixed.md"));
}

#[cfg(unix)]
#[test]
fn new_message_from_editor() {
    use std::os::unix::fs::PermissionsExt;
    let repo = TestRepo::new();
    package(&repo);
    repo.write("editor.sh", "#!/bin/sh\nprintf 'Written in the editor.\\n' > \"$1\"\n");
    let editor = repo.path("editor.sh");
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    repo.vers()
        .args(["new", "added"])
        .env("EDITOR", &editor)
        .assert()
        .success()
        .stdout("Created changes/written-in-the-editor.added.md\n");
    assert_eq!(repo.read("changes/written-in-the-editor.added.md"), "Written in the editor.\n");

    // A failing editor creates nothing.
    repo.vers().args(["new", "added"]).env("VISUAL", "false").assert().code(1);
}

#[test]
fn new_with_package_option() {
    let repo = TestRepo::new();
    repo.write("vers.toml", "[version]\ntag_format = \"{name}@v{version}\"\n");
    repo.write("packages/api/vers.toml", "extend = \"../../vers.toml\"\n\n[package]\nname = \"api\"\n");
    repo.vers()
        .args(["new", "changed", "--package", "api", "-m", "Return 404 instead of 400 for unknown orders."])
        .assert()
        .success()
        .stdout("Created packages/api/changes/return-404-instead-of-400-for.changed.md\n");

    // Outside a package without --package: a config error.
    repo.vers().args(["new", "added", "-m", "X."]).assert().code(2);
    // An unknown package.
    repo.vers()
        .args(["new", "added", "--package", "nope", "-m", "X."])
        .assert()
        .code(2)
        .stderr("error: unknown package `nope`; available packages: api\n");
}

#[test]
fn new_without_config() {
    let repo = TestRepo::new();
    repo.vers().args(["new", "added", "-m", "X."]).assert().code(2);
}

#[test]
fn new_json() {
    let repo = TestRepo::new();
    package(&repo);
    let out = repo
        .vers()
        .args(["--format", "json", "new", "fix", "12", "--ref", "pr-3", "--breaking", "-m", "Fix it."])
        .assert()
        .success();
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "path": "changes/12.fixed.md",
            "type": "fixed",
            "name": "12",
            "refs": ["12", "pr-3"],
            "breaking": true
        })
    );
}

#[test]
fn new_quiet() {
    let repo = TestRepo::new();
    package(&repo);
    repo.vers().args(["-q", "new", "added", "-m", "X."]).assert().success().stdout("");
    assert!(repo.exists("changes/x.added.md"));
}
