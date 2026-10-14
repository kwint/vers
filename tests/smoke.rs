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
