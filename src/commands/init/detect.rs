//! Detecting version files in a package directory for `vers init`.

use std::path::Path;

use regex::Regex;
use semver::Version;

use crate::config::{VersionFileSpec, VersionLocator};
use crate::version::read_file_version;

/// How a detected file's version is located, as it will be written to `vers.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locator {
    Whole,
    Key(String),
    Pattern(String),
}

/// A version file found in the package directory.
#[derive(Debug, Clone)]
pub struct Detected {
    /// Path relative to the package directory, with forward slashes.
    pub path: String,
    pub locator: Locator,
    pub version: Version,
}

/// Candidate locators per file, tried in order; the first that reads a valid
/// SemVer version wins.
enum Candidates {
    Keys(&'static [&'static str]),
    Patterns(&'static [&'static str]),
    Whole,
}

const SETUP_CFG: &[&str] = &[r"^version\s*=\s*(?<version>[^\s#;]+)\s*$"];
const MIX_EXS: &[&str] = &[r#"\bversion:\s*"(?<version>[^"]+)""#, r#"@version\s+"(?<version>[^"]+)""#];
const GEMSPEC: &[&str] = &[r#"\.version\s*=\s*["'](?<version>[^"']+)["']"#];
const GRADLE: &[&str] = &[r#"^\s*version\s*=?\s*["'](?<version>[^"']+)["']"#];
const POM: &[&str] = &[r"<version>\s*(?<version>[^<\s]+)\s*</version>"];

/// The fixed-name files, in the order they're reported.
fn fixed_files() -> Vec<(&'static str, Candidates)> {
    vec![
        ("Cargo.toml", Candidates::Keys(&["package.version", "workspace.package.version"])),
        ("package.json", Candidates::Keys(&["version"])),
        ("pyproject.toml", Candidates::Keys(&["project.version", "tool.poetry.version"])),
        ("setup.cfg", Candidates::Patterns(SETUP_CFG)),
        ("Chart.yaml", Candidates::Keys(&["version"])),
        ("pubspec.yaml", Candidates::Keys(&["version"])),
        ("mix.exs", Candidates::Patterns(MIX_EXS)),
    ]
}

/// Every version file in `dir` whose version reads as valid SemVer.
pub fn version_files(dir: &Path) -> Vec<Detected> {
    let mut files: Vec<(String, Candidates)> =
        fixed_files().into_iter().map(|(name, c)| (name.to_string(), c)).collect();
    let mut gemspecs = files_with_suffix(dir, ".gemspec");
    gemspecs.sort();
    files.extend(gemspecs.into_iter().map(|name| (name, Candidates::Patterns(GEMSPEC))));
    files.push(("build.gradle".to_string(), Candidates::Patterns(GRADLE)));
    files.push(("build.gradle.kts".to_string(), Candidates::Patterns(GRADLE)));
    files.push(("pom.xml".to_string(), Candidates::Patterns(POM)));
    files.push(("VERSION".to_string(), Candidates::Whole));

    files.into_iter().filter_map(|(name, candidates)| detect_one(dir, &name, &candidates)).collect()
}

fn files_with_suffix(dir: &Path, suffix: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| n.ends_with(suffix) && n.len() > suffix.len())
        .collect()
}

fn detect_one(dir: &Path, name: &str, candidates: &Candidates) -> Option<Detected> {
    let path = dir.join(name);
    // Exact-case match only: on case-insensitive filesystems `version` would
    // otherwise be reported as `VERSION`.
    if !path.is_file() || !name_matches_exactly(dir, name) {
        return None;
    }
    let locators: Vec<Locator> = match candidates {
        Candidates::Keys(keys) => keys.iter().map(|k| Locator::Key(k.to_string())).collect(),
        Candidates::Patterns(patterns) => patterns.iter().map(|p| Locator::Pattern(p.to_string())).collect(),
        Candidates::Whole => vec![Locator::Whole],
    };
    locators.into_iter().find_map(|locator| {
        let spec_locator = match &locator {
            Locator::Whole => VersionLocator::Whole,
            Locator::Key(k) => VersionLocator::Key(k.clone()),
            Locator::Pattern(p) => VersionLocator::Pattern(Regex::new(p).ok()?),
        };
        let spec = VersionFileSpec { path: path.clone(), locator: spec_locator };
        let version = read_file_version(&spec).ok()?;
        Some(Detected { path: name.to_string(), locator, version })
    })
}

fn name_matches_exactly(dir: &Path, name: &str) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| entries.flatten().any(|e| e.file_name().to_str() == Some(name)))
}
