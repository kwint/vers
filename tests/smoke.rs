mod common;

use common::TestRepo;

#[test]
fn help_and_version() {
    let repo = TestRepo::new();
    repo.vers().arg("--help").assert().success();
    repo.vers().arg("--version").assert().success().stdout(predicates::str::starts_with("vers "));
}

#[test]
fn completions() {
    let repo = TestRepo::new();
    repo.vers().args(["completions", "bash"]).assert().success();
}

/// Without an install receipt from the standalone installer, `vers self update`
/// points to the package manager instead of touching the binary.
#[test]
fn self_update_without_install_receipt() {
    let repo = TestRepo::new();
    let empty = repo.path("no-receipt");
    std::fs::create_dir_all(&empty).unwrap();
    repo.vers()
        .args(["self", "update"])
        .env("AXOUPDATER_CONFIG_PATH", &empty)
        .env("XDG_CONFIG_HOME", &empty)
        .env("LOCALAPPDATA", &empty)
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicates::str::contains(
            "only available when vers was installed with the standalone installer; \
             update it through your package manager instead",
        ));
}
