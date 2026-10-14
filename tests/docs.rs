//! Runs the `console` blocks in `docs/tutorials/*.md` as tests.
//!
//! Each tutorial runs top to bottom in one fresh temporary directory. A line
//! starting with `$ ` is a shell command; the lines after it, up to the next
//! command or the end of the block, are its expected combined stdout and
//! stderr. The working directory carries over between commands, so `cd`
//! works as it would for a reader.
//!
//! Expected output may use two wildcards:
//! - `[..]` inside a line matches any text on that line
//! - a line containing only `...` matches any number of lines
//!
//! The environment is fixed: `vers` from this build is first on `PATH`, git
//! uses an empty config with `init.defaultBranch = main`, and
//! `SOURCE_DATE_EPOCH` pins the release date to 2026-10-14.

use std::path::{Path, PathBuf};
use std::process::Command;

/// 2026-10-14T12:00:00Z
const SOURCE_DATE_EPOCH: &str = "1791979200";

#[derive(Debug)]
struct Step {
    line: usize,
    command: String,
    expected: Vec<String>,
}

fn parse_steps(markdown: &str) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    let mut in_console = false;
    for (i, line) in markdown.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_console = !in_console && trimmed.trim_start_matches('`').trim() == "console";
            continue;
        }
        if !in_console {
            continue;
        }
        if let Some(command) = line.strip_prefix("$ ") {
            steps.push(Step { line: i + 1, command: command.to_string(), expected: Vec::new() });
        } else if let Some(step) = steps.last_mut() {
            step.expected.push(line.to_string());
        }
    }
    for step in &mut steps {
        while step.expected.last().is_some_and(|l| l.trim().is_empty()) {
            step.expected.pop();
        }
    }
    steps
}

fn line_matches(expected: &str, actual: &str) -> bool {
    let parts: Vec<&str> = expected.split("[..]").collect();
    if parts.len() == 1 {
        return expected == actual;
    }
    let mut rest = actual;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(pos) => rest = &rest[pos + part.len()..],
                None => return false,
            }
        }
    }
    true
}

fn output_matches(expected: &[String], actual: &[String]) -> bool {
    match expected.split_first() {
        None => actual.is_empty(),
        Some((first, rest)) if first == "..." => (0..=actual.len()).any(|skip| output_matches(rest, &actual[skip..])),
        Some((first, rest)) => {
            actual.first().is_some_and(|a| line_matches(first, a)) && output_matches(rest, &actual[1..])
        }
    }
}

fn run_tutorial(doc: &Path) {
    let markdown = std::fs::read_to_string(doc).unwrap();
    let steps = parse_steps(&markdown);
    assert!(!steps.is_empty(), "{} has no console steps", doc.display());

    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let pwd_file = tmp.path().join("pwd");

    let bin_dir = Path::new(env!("CARGO_BIN_EXE_vers")).parent().unwrap().to_path_buf();
    let path = std::env::join_paths(
        std::iter::once(bin_dir).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
    )
    .unwrap();

    let mut cwd: PathBuf = work.clone();
    for step in steps {
        let script = format!("{{ {}\n}} 2>&1\nstatus=$?\npwd > '{}'\nexit $status", step.command, pwd_file.display());
        let output = Command::new("sh")
            .arg("-c")
            .arg(&script)
            .current_dir(&cwd)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("LANG", "C.UTF-8")
            .env("NO_COLOR", "1")
            .env("SOURCE_DATE_EPOCH", SOURCE_DATE_EPOCH)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "init.defaultBranch")
            .env("GIT_CONFIG_VALUE_0", "main")
            .env("GIT_AUTHOR_NAME", "Docs Test")
            .env("GIT_AUTHOR_EMAIL", "docs@example.com")
            .env("GIT_COMMITTER_NAME", "Docs Test")
            .env("GIT_COMMITTER_EMAIL", "docs@example.com")
            .output()
            .unwrap();

        let actual: Vec<String> =
            String::from_utf8_lossy(&output.stdout).lines().map(|l| l.trim_end().to_string()).collect();
        let mut actual = actual;
        while actual.last().is_some_and(|l| l.is_empty()) {
            actual.pop();
        }
        let expected: Vec<String> = step.expected.iter().map(|l| l.trim_end().to_string()).collect();

        if !output_matches(&expected, &actual) {
            panic!(
                "\n{}:{}\n$ {}\n--- expected\n{}\n--- actual\n{}\n",
                doc.display(),
                step.line,
                step.command,
                expected.join("\n"),
                actual.join("\n"),
            );
        }
        if let Ok(new_cwd) = std::fs::read_to_string(&pwd_file) {
            cwd = PathBuf::from(new_cwd.trim());
        }
    }
}

fn tutorial(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/tutorials").join(name)
}

#[test]
#[ignore = "enable once the commands are implemented (phase 2)"]
fn tutorial_first_release() {
    run_tutorial(&tutorial("first-release.md"));
}

#[test]
#[ignore = "enable once the commands are implemented (phase 2)"]
fn tutorial_monorepo() {
    run_tutorial(&tutorial("monorepo.md"));
}

#[test]
fn every_tutorial_has_a_test() {
    let source = std::fs::read_to_string(file!()).unwrap();
    for entry in std::fs::read_dir(tutorial("")).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        assert!(source.contains(&format!("tutorial(\"{name}\")")), "no test runs docs/tutorials/{name}");
    }
}

#[test]
fn wildcards() {
    assert!(line_matches("Created [..]", "Created vers.toml"));
    assert!(line_matches("a [..] c [..] e", "a b c d e"));
    assert!(!line_matches("a [..] c", "a b d"));
    let lines = |s: &[&str]| s.iter().map(|l| l.to_string()).collect::<Vec<_>>();
    assert!(output_matches(&lines(&["a", "...", "d"]), &lines(&["a", "b", "c", "d"])));
    assert!(output_matches(&lines(&["..."]), &lines(&[])));
    assert!(!output_matches(&lines(&["a"]), &lines(&["a", "b"])));
}
