use std::path::{Path, PathBuf};

use regex::Regex;
use semver::Version;

use super::*;
use crate::config::{Config, VersionFileSpec, VersionLocator, VersionSettings, default_types};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn frag(type_name: &str, breaking: bool) -> Fragment {
    Fragment {
        path: PathBuf::from(format!("changes/x.{type_name}.md")),
        name: "x".to_string(),
        type_name: type_name.to_string(),
        counter: None,
        refs: Vec::new(),
        breaking,
        body: "Something.".to_string(),
    }
}

fn settings() -> VersionSettings {
    Config::default_for(Path::new("/tmp/pkg"), "pkg").version
}

// ---------------------------------------------------------------------------
// infer_bump

#[test]
fn infer_no_fragments_is_none() {
    assert_eq!(infer_bump(&[], &default_types()), Bump::None);
}

#[test]
fn infer_uses_type_levels_and_max() {
    let types = default_types();
    let b = |fs: &[Fragment]| infer_bump(fs, &types);
    assert_eq!(b(&[frag("fixed", false)]), Bump::Patch);
    assert_eq!(b(&[frag("changed", false)]), Bump::Patch);
    assert_eq!(b(&[frag("security", false)]), Bump::Patch);
    assert_eq!(b(&[frag("fixed", false), frag("added", false)]), Bump::Minor);
    assert_eq!(b(&[frag("deprecated", false)]), Bump::Minor);
    assert_eq!(b(&[frag("added", false), frag("removed", false)]), Bump::Major);
    assert_eq!(b(&[frag("changed", true)]), Bump::Major);
    assert_eq!(b(&[frag("fixed", false), frag("fixed", true)]), Bump::Major);
}

#[test]
fn infer_ignores_unknown_types() {
    let types = default_types();
    assert_eq!(infer_bump(&[frag("mystery", true)], &types), Bump::None);
    assert_eq!(infer_bump(&[frag("mystery", true), frag("fixed", false)], &types), Bump::Patch);
}

#[test]
fn infer_respects_custom_type_bump() {
    let mut types = default_types();
    types.iter_mut().find(|t| t.name == "changed").unwrap().bump = Bump::Major;
    types.iter_mut().find(|t| t.name == "fixed").unwrap().bump = Bump::None;
    assert_eq!(infer_bump(&[frag("changed", false)], &types), Bump::Major);
    assert_eq!(infer_bump(&[frag("fixed", false)], &types), Bump::None);
}

/// The table in docs/concepts/versioning.md.
#[test]
fn inference_table_from_docs() {
    let types = default_types();
    let rows: Vec<(Vec<Fragment>, &str, &str)> = vec![
        (vec![frag("fixed", false)], "1.4.3", "0.4.3"),
        (vec![frag("fixed", false), frag("added", false)], "1.5.0", "0.4.3"),
        (vec![frag("added", false), frag("removed", false)], "2.0.0", "0.5.0"),
        (vec![frag("changed", true)], "2.0.0", "0.5.0"),
    ];
    for (fragments, one, zero) in rows {
        let bump = infer_bump(&fragments, &types);
        assert_eq!(apply_bump(&v("1.4.2"), bump, true), v(one), "{fragments:?}");
        assert_eq!(apply_bump(&v("0.4.2"), bump, true), v(zero), "{fragments:?}");
    }
}

// ---------------------------------------------------------------------------
// apply_bump

#[test]
fn apply_bump_plain() {
    assert_eq!(apply_bump(&v("1.4.2"), Bump::Major, true), v("2.0.0"));
    assert_eq!(apply_bump(&v("1.4.2"), Bump::Minor, true), v("1.5.0"));
    assert_eq!(apply_bump(&v("1.4.2"), Bump::Patch, true), v("1.4.3"));
    assert_eq!(apply_bump(&v("1.4.2"), Bump::None, true), v("1.4.2"));
}

#[test]
fn apply_bump_zero_major_shift() {
    assert_eq!(apply_bump(&v("0.4.2"), Bump::Major, true), v("0.5.0"));
    assert_eq!(apply_bump(&v("0.4.2"), Bump::Minor, true), v("0.4.3"));
    assert_eq!(apply_bump(&v("0.4.2"), Bump::Patch, true), v("0.4.3"));
    assert_eq!(apply_bump(&v("0.4.2"), Bump::None, true), v("0.4.2"));
}

#[test]
fn apply_bump_zero_major_shift_off() {
    assert_eq!(apply_bump(&v("0.4.2"), Bump::Major, false), v("1.0.0"));
    assert_eq!(apply_bump(&v("0.4.2"), Bump::Minor, false), v("0.5.0"));
    assert_eq!(apply_bump(&v("0.4.2"), Bump::Patch, false), v("0.4.3"));
}

#[test]
fn apply_bump_prerelease() {
    assert_eq!(apply_bump(&v("1.3.0-rc.2"), Bump::Minor, true), v("1.3.0"));
    assert_eq!(apply_bump(&v("1.3.0-rc.2"), Bump::Patch, true), v("1.3.0"));
    assert_eq!(apply_bump(&v("1.3.0-rc.2"), Bump::Major, true), v("2.0.0"));
    assert_eq!(apply_bump(&v("2.0.0-rc.1"), Bump::Major, true), v("2.0.0"));
    assert_eq!(apply_bump(&v("2.0.0-rc.1"), Bump::Minor, true), v("2.0.0"));
    assert_eq!(apply_bump(&v("1.3.1-rc.1"), Bump::Minor, true), v("1.4.0"));
    assert_eq!(apply_bump(&v("1.3.1-rc.1"), Bump::Patch, true), v("1.3.1"));
    assert_eq!(apply_bump(&v("1.3.0-rc.1"), Bump::None, true), v("1.3.0-rc.1"));
    // 0.x pre-releases shift first: major acts as minor.
    assert_eq!(apply_bump(&v("0.5.0-rc.1"), Bump::Major, true), v("0.5.0"));
    assert_eq!(apply_bump(&v("0.5.1-rc.1"), Bump::Major, true), v("0.6.0"));
}

#[test]
fn apply_bump_drops_build_metadata() {
    assert_eq!(apply_bump(&v("1.2.3+build.5"), Bump::Patch, true), v("1.2.4"));
    assert_eq!(apply_bump(&v("1.2.3-rc.1+abc"), Bump::Patch, true), v("1.2.3"));
}

#[test]
fn bump_parse_and_display() {
    for b in [Bump::None, Bump::Patch, Bump::Minor, Bump::Major] {
        assert_eq!(b.to_string().parse::<Bump>().unwrap(), b);
    }
    assert!("huge".parse::<Bump>().unwrap_err().contains("huge"));
    assert!(Bump::None < Bump::Patch && Bump::Patch < Bump::Minor && Bump::Minor < Bump::Major);
}

// ---------------------------------------------------------------------------
// choose

struct In {
    current: Option<&'static str>,
    files: Option<&'static str>,
    cli_version: Option<&'static str>,
    cli_bump: Option<Bump>,
    pre: Option<&'static str>,
    bump: Bump,
}

impl Default for In {
    fn default() -> Self {
        In { current: None, files: None, cli_version: None, cli_bump: None, pre: None, bump: Bump::Patch }
    }
}

fn run(i: In) -> Result<VersionPlan> {
    run_with(i, &settings())
}

fn run_with(i: In, s: &VersionSettings) -> Result<VersionPlan> {
    choose(&ChooseInput {
        current: i.current.map(v),
        files_version: i.files.map(|f| (v(f), PathBuf::from("package.json"))),
        cli_version: i.cli_version.map(v),
        cli_bump: i.cli_bump,
        pre: i.pre.map(str::to_string),
        fragments_bump: i.bump,
        settings: s,
    })
}

fn next(i: In) -> (String, VersionSource) {
    let p = run(i).unwrap();
    (p.next.to_string(), p.source)
}

#[test]
fn choose_infers_from_fragments() {
    let (n, s) = next(In { current: Some("1.4.2"), bump: Bump::Minor, ..Default::default() });
    assert_eq!(n, "1.5.0");
    assert_eq!(s, VersionSource::Inferred(Bump::Minor));
    let (n, _) = next(In { current: Some("0.4.2"), bump: Bump::Major, ..Default::default() });
    assert_eq!(n, "0.5.0");
}

#[test]
fn choose_honours_zero_major_shift_setting() {
    let mut s = settings();
    s.zero_major_shift = false;
    let p = run_with(In { current: Some("0.4.2"), bump: Bump::Major, ..Default::default() }, &s).unwrap();
    assert_eq!(p.next, v("1.0.0"));
}

#[test]
fn choose_plan_carries_current() {
    let p = run(In { current: Some("1.0.0"), ..Default::default() }).unwrap();
    assert_eq!(p.current, Some(v("1.0.0")));
    assert!(p.warnings.is_empty());
}

#[test]
fn choose_initial_without_current() {
    let (n, s) = next(In { bump: Bump::Major, ..Default::default() });
    assert_eq!(n, "0.1.0");
    assert_eq!(s, VersionSource::Inferred(Bump::Major));
    let mut st = settings();
    st.initial = v("1.0.0");
    let p = run_with(In { bump: Bump::Patch, ..Default::default() }, &st).unwrap();
    assert_eq!(p.next, v("1.0.0"));
}

#[test]
fn choose_initial_with_pre() {
    let (n, _) = next(In { pre: Some("rc"), ..Default::default() });
    assert_eq!(n, "0.1.0-rc.1");
}

#[test]
fn choose_cli_version_wins() {
    let (n, s) = next(In {
        current: Some("0.4.2"),
        files: Some("0.9.0"),
        cli_version: Some("1.0.0"),
        cli_bump: Some(Bump::Patch),
        bump: Bump::Major,
        ..Default::default()
    });
    assert_eq!(n, "1.0.0");
    assert_eq!(s, VersionSource::Cli);
}

#[test]
fn choose_cli_version_ignores_pre() {
    let (n, _) = next(In { current: Some("1.2.0"), cli_version: Some("2.0.0"), pre: Some("rc"), ..Default::default() });
    assert_eq!(n, "2.0.0");
}

#[test]
fn choose_cli_version_must_be_newer() {
    let err = run(In { current: Some("1.2.0"), cli_version: Some("1.2.0"), ..Default::default() }).unwrap_err();
    assert!(matches!(err, Error::Failed(_)));
    let msg = err.to_string();
    assert!(msg.contains("1.2.0 is not newer than the current version 1.2.0"), "{msg}");
    assert!(run(In { current: Some("1.2.0"), cli_version: Some("1.1.9"), ..Default::default() }).is_err());
    // Build metadata doesn't make a version newer.
    assert!(run(In { current: Some("1.2.0"), cli_version: Some("1.2.0+b1"), ..Default::default() }).is_err());
}

#[test]
fn choose_cli_version_without_current() {
    let (n, s) = next(In { cli_version: Some("3.0.0"), ..Default::default() });
    assert_eq!(n, "3.0.0");
    assert_eq!(s, VersionSource::Cli);
}

#[test]
fn choose_cli_bump_beats_files_and_inference() {
    let (n, s) = next(In {
        current: Some("1.4.2"),
        files: Some("1.4.3"),
        cli_bump: Some(Bump::Major),
        bump: Bump::Patch,
        ..Default::default()
    });
    assert_eq!(n, "2.0.0");
    assert_eq!(s, VersionSource::Cli);
}

#[test]
fn choose_cli_bump_shifts_on_zero_major() {
    let (n, _) = next(In { current: Some("0.4.2"), cli_bump: Some(Bump::Major), ..Default::default() });
    assert_eq!(n, "0.5.0");
}

#[test]
fn choose_cli_bump_with_pre() {
    let (n, _) =
        next(In { current: Some("1.2.0"), cli_bump: Some(Bump::Minor), pre: Some("beta"), ..Default::default() });
    assert_eq!(n, "1.3.0-beta.1");
}

#[test]
fn choose_cli_bump_none_is_not_newer() {
    let err = run(In { current: Some("1.2.0"), cli_bump: Some(Bump::None), ..Default::default() }).unwrap_err();
    assert!(err.to_string().contains("not newer"));
}

#[test]
fn choose_cli_bump_without_current_uses_initial() {
    let (n, s) = next(In { cli_bump: Some(Bump::Major), ..Default::default() });
    assert_eq!(n, "0.1.0");
    assert_eq!(s, VersionSource::Cli);
}

#[test]
fn choose_files_when_newer() {
    let (n, s) = next(In { current: Some("0.3.1"), files: Some("0.4.0"), bump: Bump::Patch, ..Default::default() });
    assert_eq!(n, "0.4.0");
    assert_eq!(s, VersionSource::Files(PathBuf::from("package.json")));
}

#[test]
fn choose_files_without_current() {
    let (n, s) = next(In { files: Some("2.3.4"), ..Default::default() });
    assert_eq!(n, "2.3.4");
    assert_eq!(s, VersionSource::Files(PathBuf::from("package.json")));
}

#[test]
fn choose_files_equal_to_current_falls_back_to_inference() {
    let (n, s) = next(In { current: Some("1.4.2"), files: Some("1.4.2"), bump: Bump::Minor, ..Default::default() });
    assert_eq!(n, "1.5.0");
    assert_eq!(s, VersionSource::Inferred(Bump::Minor));
}

#[test]
fn choose_files_older_than_current_falls_back_to_inference() {
    // agreed_version normally rejects this; with --force choose still works.
    let (n, _) = next(In { current: Some("1.4.2"), files: Some("1.0.0"), bump: Bump::Patch, ..Default::default() });
    assert_eq!(n, "1.4.3");
}

#[test]
fn choose_warns_when_fragments_suggest_more() {
    let p = run(In { current: Some("1.4.2"), files: Some("1.4.3"), bump: Bump::Minor, ..Default::default() }).unwrap();
    assert_eq!(p.next, v("1.4.3"));
    assert_eq!(p.warnings.len(), 1);
    let w = &p.warnings[0];
    assert!(w.contains("minor bump") && w.contains("1.5.0") && w.contains("1.4.3"), "{w}");
    assert!(w.contains("package.json"), "{w}");
}

#[test]
fn choose_no_warning_when_files_match_or_exceed_fragments() {
    let p = run(In { current: Some("1.4.2"), files: Some("1.5.0"), bump: Bump::Minor, ..Default::default() }).unwrap();
    assert!(p.warnings.is_empty());
    let p = run(In { current: Some("1.4.2"), files: Some("2.0.0"), bump: Bump::Patch, ..Default::default() }).unwrap();
    assert!(p.warnings.is_empty());
}

#[test]
fn choose_files_with_pre() {
    let (n, _) = next(In { current: Some("1.2.0"), files: Some("1.3.0"), pre: Some("rc"), ..Default::default() });
    assert_eq!(n, "1.3.0-rc.1");
    // A pre-release already in the files is used as is.
    let (n, _) =
        next(In { current: Some("1.2.0"), files: Some("1.3.0-beta.4"), pre: Some("rc"), ..Default::default() });
    assert_eq!(n, "1.3.0-beta.4");
}

#[test]
fn choose_prerelease_sequence_from_docs() {
    // 1.2.0 → 1.3.0-rc.1 → 1.3.0-rc.2 → 1.3.0
    let (n, _) = next(In { current: Some("1.2.0"), pre: Some("rc"), bump: Bump::Minor, ..Default::default() });
    assert_eq!(n, "1.3.0-rc.1");
    let (n, s) = next(In { current: Some("1.3.0-rc.1"), pre: Some("rc"), bump: Bump::Patch, ..Default::default() });
    assert_eq!(n, "1.3.0-rc.2");
    assert_eq!(s, VersionSource::Inferred(Bump::Patch));
    let (n, s) = next(In { current: Some("1.3.0-rc.2"), bump: Bump::None, ..Default::default() });
    assert_eq!(n, "1.3.0");
    assert_eq!(s, VersionSource::FinalisePrerelease);
}

#[test]
fn choose_pre_without_new_fragments_increments() {
    let (n, _) = next(In { current: Some("1.3.0-rc.2"), pre: Some("rc"), bump: Bump::None, ..Default::default() });
    assert_eq!(n, "1.3.0-rc.3");
}

#[test]
fn choose_final_after_prerelease_with_fragments() {
    let (n, s) = next(In { current: Some("1.3.0-rc.2"), bump: Bump::Minor, ..Default::default() });
    assert_eq!(n, "1.3.0");
    assert_eq!(s, VersionSource::Inferred(Bump::Minor));
    let (n, _) = next(In { current: Some("1.3.0-rc.2"), bump: Bump::Major, ..Default::default() });
    assert_eq!(n, "2.0.0");
}

#[test]
fn choose_pre_with_new_identifier_restarts() {
    let (n, _) = next(In { current: Some("1.3.0-beta.3"), pre: Some("rc"), bump: Bump::Patch, ..Default::default() });
    assert_eq!(n, "1.3.0-rc.1");
}

#[test]
fn choose_pre_with_bigger_bump_moves_base() {
    let (n, _) = next(In { current: Some("1.3.0-rc.2"), pre: Some("rc"), bump: Bump::Major, ..Default::default() });
    assert_eq!(n, "2.0.0-rc.1");
}

#[test]
fn choose_pre_going_backwards_is_refused() {
    let err = run(In { current: Some("1.3.0-rc.1"), pre: Some("alpha"), bump: Bump::Patch, ..Default::default() })
        .unwrap_err();
    assert!(err.to_string().contains("1.3.0-alpha.1 is not newer than the current version 1.3.0-rc.1"), "{err}");
}

#[test]
fn choose_invalid_pre_identifier() {
    for bad in ["", "r c", "rc.1", "ü"] {
        let err = run(In { current: Some("1.0.0"), pre: Some(bad), ..Default::default() }).unwrap_err();
        assert!(matches!(err, Error::Usage(_)), "{bad}: {err}");
    }
}

#[test]
fn choose_nothing_to_bump_is_refused() {
    let err = run(In { current: Some("1.0.0"), bump: Bump::None, ..Default::default() }).unwrap_err();
    assert!(matches!(err, Error::Failed(_)));
}

// ---------------------------------------------------------------------------
// Version files

struct Dir(tempfile::TempDir);

impl Dir {
    fn new() -> Dir {
        Dir(tempfile::tempdir().unwrap())
    }

    fn file(&self, name: &str, content: &str) -> PathBuf {
        let p = self.0.path().join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    fn key(&self, name: &str, content: &str, key: &str) -> VersionFileSpec {
        VersionFileSpec { path: self.file(name, content), locator: VersionLocator::Key(key.to_string()) }
    }

    fn pattern(&self, name: &str, content: &str, re: &str) -> VersionFileSpec {
        VersionFileSpec { path: self.file(name, content), locator: VersionLocator::Pattern(Regex::new(re).unwrap()) }
    }

    fn whole(&self, name: &str, content: &str) -> VersionFileSpec {
        VersionFileSpec { path: self.file(name, content), locator: VersionLocator::Whole }
    }
}

/// Read the version, then update to 9.8.7 and check that the file is
/// identical except for one occurrence of the version text.
fn roundtrip(spec: &VersionFileSpec, expected: &str) -> String {
    assert_eq!(read_file_version(spec).unwrap(), v(expected));
    let before = std::fs::read_to_string(&spec.path).unwrap();
    let after = update_file_version(spec, &v("9.8.7")).unwrap();
    let positions: Vec<_> = before.match_indices(expected).map(|(i, _)| i).collect();
    assert!(
        positions.iter().any(|&i| format!("{}9.8.7{}", &before[..i], &before[i + expected.len()..]) == after),
        "unexpected edit:\n--- before\n{before}\n--- after\n{after}"
    );
    // The new file reads back as the new version.
    std::fs::write(&spec.path, &after).unwrap();
    assert_eq!(read_file_version(spec).unwrap(), v("9.8.7"));
    after
}

const CARGO_TOML: &str = r#"# The vers crate
[package]
name = "demo"
version    =   "1.2.3"   # bumped by vers
edition = "2024"

[dependencies]
serde = { version = "1.0.0", features = ["derive"] }

[package.metadata.release]
version = "0.0.1"
"#;

#[test]
fn cargo_toml_package_version() {
    let d = Dir::new();
    let spec = d.key("Cargo.toml", CARGO_TOML, "package.version");
    let after = roundtrip(&spec, "1.2.3");
    assert!(after.contains("version    =   \"9.8.7\"   # bumped by vers\n"));
    assert!(after.contains("serde = { version = \"1.0.0\""));
}

#[test]
fn cargo_toml_workspace_package_version() {
    let d = Dir::new();
    let content =
        "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = '0.7.0'\nedition = \"2024\"\n";
    let spec = d.key("Cargo.toml", content, "workspace.package.version");
    let after = roundtrip(&spec, "0.7.0");
    assert!(after.contains("version = '9.8.7'\n"), "literal string quotes kept: {after}");
}

#[test]
fn cargo_toml_inherited_version_has_hint() {
    let d = Dir::new();
    let spec = d.key("Cargo.toml", "[package]\nname = \"a\"\nversion.workspace = true\n", "package.version");
    let err = read_file_version(&spec).unwrap_err().to_string();
    assert!(err.contains("workspace.package.version"), "{err}");
}

#[test]
fn pyproject_toml() {
    let d = Dir::new();
    let content = "[build-system]\nrequires = [\"hatchling\"]\n\n[project]\nname = \"demo\"\nversion = \"0.3.1\"\n\n\
                   [tool.ruff]\nline-length = 100\n";
    roundtrip(&d.key("pyproject.toml", content, "project.version"), "0.3.1");
}

#[test]
fn toml_dotted_and_inline_keys() {
    let d = Dir::new();
    roundtrip(&d.key("a.toml", "project.version = \"1.0.0\"\n", "project.version"), "1.0.0");
    roundtrip(&d.key("b.toml", "project = { name = \"x\", version = \"1.0.0\" }\n", "project.version"), "1.0.0");
}

#[test]
fn toml_multiline_string() {
    let d = Dir::new();
    roundtrip(&d.key("a.toml", "version = \"\"\"\n1.0.0\"\"\"\n", "version"), "1.0.0");
}

#[test]
fn toml_escaped_string_is_rejected() {
    let d = Dir::new();
    let spec = d.key("a.toml", "version = \"1.0.\\u0030\"\n", "version");
    let err = read_file_version(&spec).unwrap_err().to_string();
    assert!(err.contains("escape"), "{err}");
}

#[test]
fn toml_crlf() {
    let d = Dir::new();
    let content = "[package]\r\nname = \"demo\"\r\nversion = \"1.2.3\"\r\n";
    let after = roundtrip(&d.key("Cargo.toml", content, "package.version"), "1.2.3");
    assert_eq!(after, "[package]\r\nname = \"demo\"\r\nversion = \"9.8.7\"\r\n");
}

#[test]
fn toml_missing_key_and_wrong_type() {
    let d = Dir::new();
    let err = read_file_version(&d.key("a.toml", "[package]\nname = \"x\"\n", "package.version")).unwrap_err();
    assert!(err.to_string().contains("key `package.version` not found"), "{err}");
    let err = read_file_version(&d.key("b.toml", "version = 1\n", "version")).unwrap_err();
    assert!(err.to_string().contains("not a string"), "{err}");
    let err = read_file_version(&d.key("c.toml", "version = [\n", "version")).unwrap_err();
    assert!(err.to_string().contains("invalid TOML"), "{err}");
}

const PACKAGE_JSON: &str = r#"{
  "name": "web",
  "dependencies": {
    "version": "1.2.3",
    "left-pad": "^1.2.3"
  },
  "nested": { "meta": { "version": "1.2.3" } },
  "list": [ { "version": "1.2.3" }, "1.2.3", 1, true, null, -1.5e3 ],
  "version": "1.2.3",
  "scripts": { "build": "tsc \"src\" --version" }
}
"#;

#[test]
fn package_json_top_level_only_touches_key_path() {
    let d = Dir::new();
    let spec = d.key("package.json", PACKAGE_JSON, "version");
    let after = roundtrip(&spec, "1.2.3");
    let expected =
        PACKAGE_JSON.replace("  \"version\": \"1.2.3\",\n  \"scripts\"", "  \"version\": \"9.8.7\",\n  \"scripts\"");
    assert_eq!(after, expected);
}

#[test]
fn package_json_nested_key() {
    let d = Dir::new();
    let spec = d.key("package.json", PACKAGE_JSON, "nested.meta.version");
    let after = roundtrip(&spec, "1.2.3");
    assert!(after.contains("\"nested\": { \"meta\": { \"version\": \"9.8.7\" } }"));
    assert!(after.contains("\"dependencies\": {\n    \"version\": \"1.2.3\""));
}

#[test]
fn json_last_duplicate_key_wins() {
    let d = Dir::new();
    let content = "{\"version\": \"0.0.1\", \"a\": {\"version\": \"0.0.1\"}, \"version\": \"2.0.0\"}";
    let after = roundtrip(&d.key("x.json", content, "version"), "2.0.0");
    assert_eq!(after, "{\"version\": \"0.0.1\", \"a\": {\"version\": \"0.0.1\"}, \"version\": \"9.8.7\"}");
}

#[test]
fn json_escaped_keys_and_strings() {
    let d = Dir::new();
    let content = "{\"x\\\"y\": \"a\\\"b\\\\\", \"ver\\u0073ion\": \"3.0.0\"}";
    roundtrip(&d.key("x.json", content, "version"), "3.0.0");
}

#[test]
fn json_crlf_and_bom() {
    let d = Dir::new();
    let content = "\u{feff}{\r\n  \"version\": \"1.0.0\"\r\n}\r\n";
    let spec = d.key("package.json", content, "version");
    // serde_json itself rejects a BOM, so vers strips it before parsing.
    let after = roundtrip(&spec, "1.0.0");
    assert_eq!(after, "\u{feff}{\r\n  \"version\": \"9.8.7\"\r\n}\r\n");
}

#[test]
fn json_errors() {
    let d = Dir::new();
    let err = read_file_version(&d.key("a.json", "{\"name\": \"x\"}", "version")).unwrap_err();
    assert!(err.to_string().contains("key `version` not found"), "{err}");
    let err = read_file_version(&d.key("b.json", "{\"version\": 1}", "version")).unwrap_err();
    assert!(err.to_string().contains("not a string"), "{err}");
    let err = read_file_version(&d.key("c.json", "{\"version\": ", "version")).unwrap_err();
    assert!(err.to_string().contains("invalid JSON"), "{err}");
    let err = read_file_version(&d.key("d.json", "{\"a\": [\"version\"]}", "a.version")).unwrap_err();
    assert!(err.to_string().contains("not found"), "{err}");
}

const CHART_YAML: &str = "\
# Helm chart
apiVersion: v2
name: demo
dependencies:
  - name: redis
    version: 1.2.3
annotations:
  version: \"1.2.3\"
version: 1.2.3   # chart version
appVersion: \"1.2.3\"
";

#[test]
fn chart_yaml_version() {
    let d = Dir::new();
    let after = roundtrip(&d.key("Chart.yaml", CHART_YAML, "version"), "1.2.3");
    assert!(after.contains("\nversion: 9.8.7   # chart version\n"), "{after}");
    assert!(after.contains("    version: 1.2.3\n"));
    assert!(after.contains("appVersion: \"1.2.3\"\n"));
}

#[test]
fn chart_yaml_quoted_and_nested() {
    let d = Dir::new();
    let after = roundtrip(&d.key("Chart.yml", CHART_YAML, "appVersion"), "1.2.3");
    assert!(after.contains("appVersion: \"9.8.7\"\n"));
    let after = roundtrip(&d.key("chart/Chart.yaml", CHART_YAML, "annotations.version"), "1.2.3");
    assert!(after.contains("annotations:\n  version: \"9.8.7\"\n"));
}

#[test]
fn yaml_nested_deeper_blocks_are_skipped() {
    let d = Dir::new();
    let content = "---\na:\n  b:\n    version: 0.0.1\n  # comment\n\n  version: '4.5.6'\nversion: 0.0.2\n";
    let after = roundtrip(&d.key("x.yaml", content, "a.version"), "4.5.6");
    assert!(after.contains("  version: '9.8.7'\n"));
    assert!(after.contains("version: 0.0.1") && after.contains("version: 0.0.2"));
}

#[test]
fn yaml_crlf() {
    let d = Dir::new();
    let content = "name: demo\r\nversion: 1.0.0\r\nappVersion: x\r\n";
    let after = roundtrip(&d.key("Chart.yaml", content, "version"), "1.0.0");
    assert_eq!(after, "name: demo\r\nversion: 9.8.7\r\nappVersion: x\r\n");
}

#[test]
fn yaml_errors() {
    let d = Dir::new();
    let err = read_file_version(&d.key("a.yaml", "name: x\n", "version")).unwrap_err();
    assert!(err.to_string().contains("not found"), "{err}");
    let err = read_file_version(&d.key("b.yaml", "meta: {version: 1.0.0}\n", "meta.version")).unwrap_err();
    assert!(err.to_string().contains("cannot locate"), "{err}");
    let err = read_file_version(&d.key("c.yaml", "version:\n  - 1\n", "version")).unwrap_err();
    assert!(err.to_string().contains("not a string"), "{err}");
    let err = read_file_version(&d.key("d.yaml", "version: [\n", "version")).unwrap_err();
    assert!(err.to_string().contains("invalid YAML"), "{err}");
}

#[test]
fn key_on_unknown_format_is_an_error() {
    let d = Dir::new();
    let err = read_file_version(&d.key("setup.cfg", "version = 1.0.0\n", "version")).unwrap_err();
    assert!(err.to_string().contains("unknown file format"), "{err}");
    assert!(err.to_string().contains("pattern"), "{err}");
}

#[test]
fn extension_is_case_insensitive() {
    let d = Dir::new();
    roundtrip(&d.key("PACKAGE.JSON", "{\"version\": \"1.0.0\"}", "version"), "1.0.0");
}

#[test]
fn invalid_key_path() {
    let d = Dir::new();
    let err = read_file_version(&d.key("a.toml", "version = \"1.0.0\"\n", "package..version")).unwrap_err();
    assert!(err.to_string().contains("invalid key"), "{err}");
}

#[test]
fn c_header_pattern() {
    let d = Dir::new();
    let content =
        "#ifndef VERSION_H\n#define VERSION_H\n#define APP_VERSION \"2.1.0\"\n#define APP_NAME \"demo\"\n#endif\n";
    let spec = d.pattern("src/version.h", content, r#"#define APP_VERSION "(?<version>[^"]+)""#);
    let after = roundtrip(&spec, "2.1.0");
    assert!(after.contains("#define APP_VERSION \"9.8.7\"\n"));
}

#[test]
fn pattern_anchors_match_lines() {
    let d = Dir::new();
    let content = "project = \"demo\"\r\nrelease = \"0.9.0\"\r\nversion = \"0.9\"\r\n";
    let spec = d.pattern("docs/conf.py", content, r#"^release = "(?<version>[^"]+)"$"#);
    let after = roundtrip(&spec, "0.9.0");
    assert_eq!(after, "project = \"demo\"\r\nrelease = \"9.8.7\"\r\nversion = \"0.9\"\r\n");
}

#[test]
fn pattern_must_match_exactly_once() {
    let d = Dir::new();
    let spec = d.pattern("a.txt", "v=1.0.0\nv=1.0.0\n", r"v=(?<version>\S+)");
    let err = read_file_version(&spec).unwrap_err().to_string();
    assert!(err.contains("matched 2 times"), "{err}");
    assert!(update_file_version(&spec, &v("2.0.0")).is_err());
    let spec = d.pattern("b.txt", "nothing here\n", r"v=(?<version>\S+)");
    let err = read_file_version(&spec).unwrap_err().to_string();
    assert!(err.contains("did not match"), "{err}");
}

#[test]
fn pattern_without_version_group() {
    let d = Dir::new();
    let err = read_file_version(&d.pattern("a.txt", "v=1.0.0\n", r"v=(\S+)")).unwrap_err();
    assert!(err.to_string().contains("named group `version`"), "{err}");
}

#[test]
fn version_file_whole() {
    let d = Dir::new();
    let after = roundtrip(&d.whole("VERSION", "1.4.2\n"), "1.4.2");
    assert_eq!(after, "9.8.7\n");
    let after = roundtrip(&d.whole("VERSION2", "  1.4.2  \r\n\r\n"), "1.4.2");
    assert_eq!(after, "  9.8.7  \r\n\r\n");
    let after = roundtrip(&d.whole("VERSION3", "1.4.2"), "1.4.2");
    assert_eq!(after, "9.8.7");
}

#[test]
fn version_file_whole_empty() {
    let d = Dir::new();
    let err = read_file_version(&d.whole("VERSION", " \n")).unwrap_err();
    assert!(err.to_string().contains("empty"), "{err}");
}

#[test]
fn invalid_semver_names_file_and_value() {
    let d = Dir::new();
    let spec = d.whole("VERSION", "1.2\n");
    let err = read_file_version(&spec).unwrap_err();
    assert!(matches!(err, Error::VersionFile { .. }));
    let msg = err.to_string();
    assert!(msg.contains("VERSION") && msg.contains("`1.2`"), "{msg}");

    let spec = d.key("package.json", "{\"version\": \"v1.2.3\"}", "version");
    let msg = read_file_version(&spec).unwrap_err().to_string();
    assert!(msg.contains("package.json") && msg.contains("`v1.2.3`") && msg.contains("leading `v`"), "{msg}");
}

#[test]
fn missing_file_is_io_error() {
    let d = Dir::new();
    let spec = VersionFileSpec { path: d.0.path().join("VERSION"), locator: VersionLocator::Whole };
    assert!(matches!(read_file_version(&spec).unwrap_err(), Error::Io { .. }));
}

#[test]
fn update_does_not_write() {
    let d = Dir::new();
    let spec = d.whole("VERSION", "1.0.0\n");
    update_file_version(&spec, &v("2.0.0")).unwrap();
    assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), "1.0.0\n");
}

#[test]
fn update_with_prerelease_and_build() {
    let d = Dir::new();
    let spec = d.key("package.json", "{\"version\": \"1.0.0\"}", "version");
    assert_eq!(update_file_version(&spec, &v("1.1.0-rc.1+b5")).unwrap(), "{\"version\": \"1.1.0-rc.1+b5\"}");
}

// ---------------------------------------------------------------------------
// agreed_version

#[test]
fn agreed_no_files() {
    assert_eq!(agreed_version(&[], Some(&v("1.0.0")), false).unwrap(), None);
}

#[test]
fn agreed_all_same() {
    let d = Dir::new();
    let a = d.whole("VERSION", "1.2.0\n");
    let b = d.key("package.json", "{\"version\": \"1.2.0\"}", "version");
    let (ver, path) = agreed_version(&[a.clone(), b], Some(&v("1.1.0")), false).unwrap().unwrap();
    assert_eq!(ver, v("1.2.0"));
    assert_eq!(path, a.path);
    // At the current version, or with no current version, is fine too.
    assert!(agreed_version(std::slice::from_ref(&a), Some(&v("1.2.0")), false).is_ok());
    assert!(agreed_version(std::slice::from_ref(&a), None, false).is_ok());
}

#[test]
fn agreed_disagreement_lists_files() {
    let d = Dir::new();
    let a = d.whole("VERSION", "1.2.0\n");
    let b = d.key("package.json", "{\"version\": \"1.3.0\"}", "version");
    let err = agreed_version(&[a, b], None, false).unwrap_err();
    assert!(matches!(err, Error::VersionFile { .. }));
    let msg = err.to_string();
    assert!(msg.contains("disagree"), "{msg}");
    assert!(msg.contains("VERSION: 1.2.0") && msg.contains("package.json: 1.3.0"), "{msg}");
}

#[test]
fn agreed_older_than_current() {
    let d = Dir::new();
    let a = d.key("Cargo.toml", "[package]\nversion = \"1.0.0\"\n", "package.version");
    let err = agreed_version(std::slice::from_ref(&a), Some(&v("1.1.0")), false).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Cargo.toml") && msg.contains("older than the current version 1.1.0"), "{msg}");
}

#[test]
fn agreed_force_takes_highest() {
    let d = Dir::new();
    let a = d.whole("VERSION", "1.2.0\n");
    let b = d.key("package.json", "{\"version\": \"1.3.0\"}", "version");
    let c = d.whole("OTHER", "1.0.0\n");
    let (ver, path) = agreed_version(&[a, b.clone(), c], Some(&v("2.0.0")), true).unwrap().unwrap();
    assert_eq!(ver, v("1.3.0"));
    assert_eq!(path, b.path);
}

#[test]
fn agreed_propagates_read_errors_even_with_force() {
    let d = Dir::new();
    let a = d.whole("VERSION", "garbage\n");
    assert!(agreed_version(&[a], None, true).is_err());
}
