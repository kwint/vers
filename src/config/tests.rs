use std::path::{Path, PathBuf};

use super::*;
use crate::error::Error;

/// A temporary directory that git won't treat as (part of) a repository: a
/// bogus `.git` file makes `git rev-parse` fail, so `repo = "auto"` never
/// picks up an `origin` from whatever repository the temp dir is inside.
struct Tmp {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Tmp {
    fn new() -> Tmp {
        let dir = tempfile::tempdir().unwrap();
        let root = resolve::absolute(&dir.path().canonicalize().unwrap()).join("ws");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".git"), "gitdir: does-not-exist\n").unwrap();
        Tmp { _dir: dir, root }
    }

    fn path(&self, rel: &str) -> PathBuf {
        if rel.is_empty() { self.root.clone() } else { resolve::normalize(&self.root.join(rel)) }
    }

    fn write(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    fn mkdir(&self, rel: &str) -> PathBuf {
        let path = self.path(rel);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn load(&self, rel: &str, content: &str) -> Result<Config> {
        load(&self.write(rel, content))
    }
}

fn config_err<T: std::fmt::Debug>(result: Result<T>) -> (PathBuf, String) {
    match result {
        Err(Error::Config { path, message }) => (path, message),
        Err(other) => panic!("expected a config error, got {other:?}"),
        Ok(c) => panic!("expected a config error, got {c:?}"),
    }
}

fn err_message<T: std::fmt::Debug>(result: Result<T>) -> String {
    result.unwrap_err().to_string()
}

fn names(configs: &[Config]) -> Vec<&str> {
    configs.iter().map(Config::name).collect()
}

// ---------------------------------------------------------------------------
// find_config_for
// ---------------------------------------------------------------------------

#[test]
fn find_config_walks_up_to_the_nearest_config() {
    let t = Tmp::new();
    let root = t.write("vers.toml", "");
    let api = t.write("packages/api/vers.toml", "[package]\n");
    t.mkdir("packages/api/src/deep");
    t.mkdir("tools");

    assert_eq!(find_config_for(&t.path("packages/api/src/deep")).unwrap(), Some(api.clone()));
    assert_eq!(find_config_for(&t.path("packages/api")).unwrap(), Some(api.clone()));
    assert_eq!(find_config_for(&t.path("tools")).unwrap(), Some(root.clone()));
    assert_eq!(find_config_for(&t.path("packages")).unwrap(), Some(root));
}

#[test]
fn find_config_for_a_file_starts_at_its_directory() {
    let t = Tmp::new();
    let api = t.write("api/vers.toml", "[package]\n");
    let fragment = t.write("api/changes/1.fixed.md", "Fix.\n");
    assert_eq!(find_config_for(&fragment).unwrap(), Some(api.clone()));
    // A deleted file (as in a diff) still maps to its package.
    assert_eq!(find_config_for(&t.path("api/src/gone.rs")).unwrap(), Some(api));
}

#[test]
fn dot_vers_toml_beats_vers_toml() {
    let t = Tmp::new();
    t.write("vers.toml", "[package]\nname = \"plain\"\n");
    let hidden = t.write(".vers.toml", "[package]\nname = \"hidden\"\n");
    assert_eq!(find_config_for(&t.root).unwrap(), Some(hidden));
    assert_eq!(load_for(&t.root).unwrap().name(), "hidden");
}

#[test]
fn find_config_returns_none_without_config() {
    let t = Tmp::new();
    t.mkdir("a/b");
    // There may be a config somewhere above the temp dir on a developer's
    // machine, but never one inside it.
    if let Some(found) = find_config_for(&t.path("a/b")).unwrap() {
        assert!(!found.starts_with(&t.root), "{}", found.display());
    }
}

#[test]
fn find_config_accepts_relative_paths() {
    let t = Tmp::new();
    let config = t.write("vers.toml", "");
    let relative = t.path("vers.toml").strip_prefix(&t.root).unwrap().to_path_buf();
    // Relative to the process cwd, so just check it doesn't panic and that an
    // absolute path works.
    let _ = find_config_for(&relative).unwrap();
    assert_eq!(find_config_for(&config).unwrap(), Some(config));
}

#[test]
fn load_for_without_config_suggests_init() {
    let t = Tmp::new();
    let dir = t.mkdir("nothing/here");
    // Only meaningful when nothing above the temp dir has a config.
    if find_config_for(&dir).unwrap().is_none() {
        let (path, message) = config_err(load_for(&dir));
        assert_eq!(path, dir);
        assert!(message.contains("vers init"), "{message}");
    }
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

#[test]
fn empty_package_table_gives_defaults() {
    let t = Tmp::new();
    let c = t.load("my-pkg/vers.toml", "[package]\n").unwrap();
    let dir = t.path("my-pkg");
    assert_eq!(c.path, dir.join("vers.toml"));
    assert_eq!(c.dir, dir);
    assert_eq!(c.name(), "my-pkg");
    assert!(c.required_version.is_none());
    assert_eq!(c.changelog.path, dir.join("CHANGELOG.md"));
    assert_eq!(c.changelog.repo, RepoKind::None);
    assert_eq!(c.changelog.repo_url, None);
    assert_eq!(c.changelog.compare_url, None);
    assert_eq!(c.changelog.release_url, None);
    assert_eq!(c.changelog.ref_position, RefPosition::End);
    assert_eq!(c.changelog.breaking_prefix, "**Breaking:** ");
    assert_eq!(c.fragments.directory, dir.join("changes"));
    assert_eq!(c.types, default_types());
    assert!(c.refs.is_empty());
    assert_eq!(c.version.initial, Version::new(0, 1, 0));
    assert!(c.version.zero_major_shift);
    assert!(c.version.fold_prereleases);
    assert!(!c.version.tag);
    assert_eq!(c.version.tag_format, "v{version}");
    assert_eq!(c.version.commit_message, "Release {name} {version}");
    assert!(c.version.files.is_empty());
    assert!(c.check.require_fragment);
    assert!(c.check.ignore.is_empty());
}

#[test]
fn defaults_match_default_for() {
    let t = Tmp::new();
    let c = t.load("pkg/vers.toml", "[package]\n").unwrap();
    let d = Config::default_for(&t.path("pkg"), "pkg");
    assert_eq!(c.path, d.path);
    assert_eq!(c.changelog.path, d.changelog.path);
    assert_eq!(c.changelog.breaking_prefix, d.changelog.breaking_prefix);
    assert_eq!(c.fragments.directory, d.fragments.directory);
    assert_eq!(c.types, d.types);
    assert_eq!(c.version.initial, d.version.initial);
    assert_eq!(c.version.tag_format, d.version.tag_format);
    assert_eq!(c.version.commit_message, d.version.commit_message);
}

#[test]
fn no_package_table_is_a_shared_config() {
    let t = Tmp::new();
    let c = t.load("vers.toml", "[version]\ntag = true\n").unwrap();
    assert!(!c.is_package());
    assert!(c.package.is_none());
    assert!(c.version.tag);
    let c = t.load("empty/vers.toml", "").unwrap();
    assert!(!c.is_package());
}

#[test]
fn explicit_package_name() {
    let t = Tmp::new();
    let c = t.load("dir/vers.toml", "[package]\nname = \"api\"\n").unwrap();
    assert_eq!(c.name(), "api");
}

#[test]
fn package_name_must_not_be_empty_or_contain_spaces() {
    let t = Tmp::new();
    let (_, m) = config_err(t.load("a/vers.toml", "[package]\nname = \"\"\n"));
    assert!(m.contains("must not be empty"), "{m}");
    let (_, m) = config_err(t.load("b/vers.toml", "[package]\nname = \"my api\"\n"));
    assert!(m.contains("\"my api\"") && m.contains("whitespace"), "{m}");
    // The default comes from the directory name, so the error says to set it.
    let (_, m) = config_err(t.load("my dir/vers.toml", "[package]\n"));
    assert!(m.contains("\"my dir\"") && m.contains("set `name`"), "{m}");
}

#[test]
fn all_keys_are_read() {
    let t = Tmp::new();
    let c = t
        .load(
            "pkg/vers.toml",
            r#"
required_version = ">=0.1"

[package]
name = "api"

[changelog]
path = "docs/CHANGES.md"
repo = "github"
repo_url = "https://github.com/org/repo/"
compare_url = "{repo_url}/compare/{from}..{to}"
release_url = "https://example.com/{tag}"
ref_position = "start"
breaking_prefix = "BREAKING: "

[fragments]
directory = "news"

[version]
initial = "1.0.0"
zero_major_shift = false
fold_prereleases = false
tag = true
tag_format = "{name}@v{version}"
commit_message = "chore: release {name} {version}"

[[version.files]]
path = "VERSION"

[check]
require_fragment = false
ignore = ["docs/**", "*.md"]
"#,
        )
        .unwrap();
    let dir = t.path("pkg");
    assert_eq!(c.required_version, Some(VersionReq::parse(">=0.1").unwrap()));
    assert_eq!(c.changelog.path, dir.join("docs").join("CHANGES.md"));
    assert_eq!(c.changelog.repo, RepoKind::Github);
    assert_eq!(c.changelog.repo_url.as_deref(), Some("https://github.com/org/repo"));
    assert_eq!(c.changelog.compare_url.as_deref(), Some("https://github.com/org/repo/compare/{from}..{to}"));
    assert_eq!(c.changelog.release_url.as_deref(), Some("https://example.com/{tag}"));
    assert_eq!(c.changelog.ref_position, RefPosition::Start);
    assert_eq!(c.changelog.breaking_prefix, "BREAKING: ");
    assert_eq!(c.fragments.directory, dir.join("news"));
    assert_eq!(c.version.initial, Version::new(1, 0, 0));
    assert!(!c.version.zero_major_shift);
    assert!(!c.version.fold_prereleases);
    assert!(c.version.tag);
    assert_eq!(c.version.tag_format, "{name}@v{version}");
    assert_eq!(c.version.commit_message, "chore: release {name} {version}");
    assert_eq!(c.version.files.len(), 1);
    assert_eq!(c.version.files[0].path, dir.join("VERSION"));
    assert!(matches!(c.version.files[0].locator, VersionLocator::Whole));
    assert!(!c.check.require_fragment);
    assert_eq!(c.check.ignore, vec!["docs/**", "*.md"]);
}

#[test]
fn paths_are_normalised() {
    let t = Tmp::new();
    let c = t
        .load(
            "packages/api/vers.toml",
            "[package]\n[changelog]\npath = \"./CHANGELOG.md\"\n[[version.files]]\npath = \"../../Cargo.toml\"\nkey = \"package.version\"\n",
        )
        .unwrap();
    assert_eq!(c.changelog.path, t.path("packages/api/CHANGELOG.md"));
    assert_eq!(c.version.files[0].path, t.path("Cargo.toml"));
}

#[test]
fn absolute_paths_are_kept() {
    let t = Tmp::new();
    let elsewhere = t.path("elsewhere/CHANGELOG.md");
    let toml = format!("[package]\n[changelog]\npath = '{}'\n", elsewhere.display());
    let c = t.load("pkg/vers.toml", &toml).unwrap();
    assert_eq!(c.changelog.path, elsewhere);
}

#[test]
fn config_path_may_be_relative_or_unnormalised() {
    let t = Tmp::new();
    t.write("pkg/vers.toml", "[package]\n");
    let c = load(&t.root.join("pkg/./sub/../vers.toml")).unwrap();
    assert_eq!(c.path, t.path("pkg/vers.toml"));
    assert_eq!(c.dir, t.path("pkg"));
}

#[test]
fn crlf_and_bom_are_accepted() {
    let t = Tmp::new();
    let c = t.load("pkg/vers.toml", "\u{feff}[package]\r\nname = \"api\"\r\n\r\n[version]\r\ntag = true\r\n").unwrap();
    assert_eq!(c.name(), "api");
    assert!(c.version.tag);
}

#[test]
fn missing_config_file_is_a_config_error() {
    let t = Tmp::new();
    let missing = t.path("nope/vers.toml");
    let (path, m) = config_err(load(&missing));
    assert_eq!(path, missing);
    assert!(m.contains("can't read"), "{m}");
}

// ---------------------------------------------------------------------------
// Parse errors
// ---------------------------------------------------------------------------

#[test]
fn syntax_errors_name_the_line() {
    let t = Tmp::new();
    let file = t.write("vers.toml", "[package]\nname = \"api\n");
    let (path, m) = config_err(load(&file));
    assert_eq!(path, file);
    assert!(m.contains("line 2"), "{m}");
}

#[test]
fn unknown_keys_are_errors_everywhere() {
    let cases = [
        ("extnd = \"../vers.toml\"\n", "extnd"),
        ("[package]\nnmae = \"api\"\n", "nmae"),
        ("[changelog]\nrepo_ulr = \"x\"\n", "repo_ulr"),
        ("[fragments]\ndir = \"x\"\n", "dir"),
        ("[[types]]\nname = \"a\"\nheading = \"A\"\nbumps = \"minor\"\n", "bumps"),
        ("[refs.jira]\nmatch = '(?<id>x)'\nlink = \"x\"\n", "link"),
        ("[version]\ntags = true\n", "tags"),
        ("[[version.files]]\npath = \"VERSION\"\nkeys = \"a\"\n", "keys"),
        ("[check]\nignored = []\n", "ignored"),
        ("[tool]\nx = 1\n", "tool"),
    ];
    for (i, (toml, key)) in cases.iter().enumerate() {
        let t = Tmp::new();
        let (_, m) = config_err(t.load(&format!("c{i}/vers.toml"), toml));
        assert!(m.contains(&format!("unknown field `{key}`")), "{toml}: {m}");
        assert!(m.contains("line "), "{toml}: {m}");
    }
}

#[test]
fn wrong_value_types_are_errors() {
    let t = Tmp::new();
    let (_, m) = config_err(t.load("a/vers.toml", "[version]\ntag = \"yes\"\n"));
    assert!(m.contains("line 2"), "{m}");
    let (_, m) = config_err(t.load("b/vers.toml", "[changelog]\nrepo = \"bitbucket\"\n"));
    assert!(m.contains("bitbucket") || m.contains("unknown variant"), "{m}");
    let (_, m) = config_err(t.load("c/vers.toml", "[changelog]\nref_position = \"middle\"\n"));
    assert!(m.contains("middle") || m.contains("unknown variant"), "{m}");
}

// ---------------------------------------------------------------------------
// required_version
// ---------------------------------------------------------------------------

#[test]
fn required_version_that_matches() {
    let t = Tmp::new();
    let req = format!("={}", env!("CARGO_PKG_VERSION"));
    let c = t.load("vers.toml", &format!("required_version = \"{req}\"\n")).unwrap();
    assert_eq!(c.required_version, Some(VersionReq::parse(&req).unwrap()));
}

#[test]
fn required_version_mismatch_names_both_versions() {
    let t = Tmp::new();
    let file = t.write("vers.toml", "required_version = \">=999.0\"\n");
    let (path, m) = config_err(load(&file));
    assert_eq!(path, file);
    assert!(m.contains(">=999.0"), "{m}");
    assert!(m.contains(env!("CARGO_PKG_VERSION")), "{m}");
    assert_eq!(load(&file).unwrap_err().exit_code(), 2);
}

#[test]
fn required_version_is_checked_before_unknown_keys() {
    let t = Tmp::new();
    let (_, m) = config_err(t.load("vers.toml", "required_version = \">=999\"\nfuture_key = 1\n"));
    assert!(m.contains(">=999"), "{m}");
}

#[test]
fn invalid_required_version() {
    let t = Tmp::new();
    let (_, m) = config_err(t.load("vers.toml", "required_version = \"not a version\"\n"));
    assert!(m.contains("invalid `required_version`") && m.contains("not a version"), "{m}");
}

#[test]
fn required_version_in_an_extended_config_is_checked() {
    let t = Tmp::new();
    let base = t.write("vers.toml", "required_version = \">=999\"\n");
    let (path, m) = config_err(t.load("pkg/vers.toml", "extend = \"../vers.toml\"\n[package]\n"));
    assert_eq!(path, base);
    assert!(m.contains(">=999"), "{m}");
}

// ---------------------------------------------------------------------------
// extend
// ---------------------------------------------------------------------------

#[test]
fn no_extend_inherits_nothing() {
    let t = Tmp::new();
    t.write(
        "vers.toml",
        "[changelog]\nrepo = \"github\"\nrepo_url = \"https://github.com/o/r\"\n[version]\ntag = true\n",
    );
    let c = t.load("packages/api/vers.toml", "[package]\n").unwrap();
    assert_eq!(c.changelog.repo, RepoKind::None);
    assert_eq!(c.changelog.repo_url, None);
    assert!(!c.version.tag);
}

#[test]
fn extend_inherits_and_overrides() {
    let t = Tmp::new();
    t.write(
        "vers.toml",
        r#"
[changelog]
repo = "gitlab"
repo_url = "https://gitlab.com/shop/shop"
breaking_prefix = "BREAKING "

[version]
tag = true
tag_format = "{name}@v{version}"
"#,
    );
    let c = t
        .load(
            "packages/api/vers.toml",
            "extend = \"../../vers.toml\"\n[package]\nname = \"api\"\n[version]\ntag = false\n",
        )
        .unwrap();
    assert_eq!(c.name(), "api");
    assert_eq!(c.changelog.repo, RepoKind::Gitlab);
    assert_eq!(c.changelog.repo_url.as_deref(), Some("https://gitlab.com/shop/shop"));
    assert_eq!(c.changelog.breaking_prefix, "BREAKING ");
    // Tables merge key by key: `tag` is overridden, `tag_format` inherited.
    assert!(!c.version.tag);
    assert_eq!(c.version.tag_format, "{name}@v{version}");
    // The package's own paths are relative to the package.
    assert_eq!(c.path, t.path("packages/api/vers.toml"));
    assert_eq!(c.changelog.path, t.path("packages/api/CHANGELOG.md"));
    assert_eq!(c.fragments.directory, t.path("packages/api/changes"));
}

#[test]
fn package_table_is_inherited_but_name_defaults_to_the_extending_dir() {
    let t = Tmp::new();
    t.write("shared/vers.toml", "[package]\n");
    let c = t.load("web/vers.toml", "extend = \"../shared/vers.toml\"\n").unwrap();
    assert_eq!(c.name(), "web");
}

#[test]
fn inherited_paths_resolve_against_the_file_that_wrote_them() {
    let t = Tmp::new();
    t.write(
        "vers.toml",
        r#"
[changelog]
path = "docs/CHANGELOG.md"

[fragments]
directory = "news"

[[version.files]]
path = "VERSION"
"#,
    );
    let c = t.load("packages/api/vers.toml", "extend = \"../../vers.toml\"\n[package]\n").unwrap();
    assert_eq!(c.changelog.path, t.path("docs/CHANGELOG.md"));
    assert_eq!(c.fragments.directory, t.path("news"));
    assert_eq!(c.version.files[0].path, t.path("VERSION"));
}

#[test]
fn arrays_are_replaced_not_concatenated() {
    let t = Tmp::new();
    t.write(
        "vers.toml",
        r#"
[[types]]
name = "added"
heading = "Added"

[[types]]
name = "fixed"
heading = "Fixed"

[[version.files]]
path = "VERSION"

[[version.files]]
path = "Cargo.toml"
key = "package.version"

[check]
ignore = ["docs/**", "*.md"]
require_fragment = false
"#,
    );
    let c = t
        .load(
            "pkg/vers.toml",
            r#"
extend = "../vers.toml"
[package]

[[types]]
name = "internal"
heading = "Internal"

[[version.files]]
path = "package.json"
key = "version"

[check]
ignore = ["*.txt"]
"#,
        )
        .unwrap();
    assert_eq!(c.types.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["internal"]);
    assert_eq!(c.version.files.len(), 1);
    assert_eq!(c.version.files[0].path, t.path("pkg/package.json"));
    assert_eq!(c.check.ignore, vec!["*.txt"]);
    assert!(!c.check.require_fragment, "check table merges key by key");
}

#[test]
fn inherited_arrays_are_kept_when_not_overridden() {
    let t = Tmp::new();
    t.write("vers.toml", "[[types]]\nname = \"misc\"\nheading = \"Misc\"\n");
    let c = t.load("pkg/vers.toml", "extend = \"../vers.toml\"\n[package]\n").unwrap();
    assert_eq!(c.types.len(), 1);
    assert_eq!(c.types[0].name, "misc");
}

#[test]
fn extend_chains() {
    let t = Tmp::new();
    t.write(
        "vers.toml",
        "[changelog]\nbreaking_prefix = \"root \"\n[version]\ntag_format = \"root-{version}\"\ntag = true\n",
    );
    t.write("packages/vers.toml", "extend = \"../vers.toml\"\n[version]\ntag_format = \"mid-{version}\"\n");
    let c = t.load("packages/api/vers.toml", "extend = \"../vers.toml\"\n[package]\n[version]\ntag = false\n").unwrap();
    assert_eq!(c.changelog.breaking_prefix, "root ");
    assert_eq!(c.version.tag_format, "mid-{version}");
    assert!(!c.version.tag);
}

#[test]
fn extend_paths_are_relative_to_each_file_in_the_chain() {
    let t = Tmp::new();
    t.write("config/base.toml", "[fragments]\ndirectory = \"fragments\"\n");
    // `extend` in config/mid.toml is relative to config/, not to the package.
    t.write("config/mid.toml", "extend = \"base.toml\"\n");
    let c = t.load("pkg/vers.toml", "extend = \"../config/mid.toml\"\n[package]\n").unwrap();
    assert_eq!(c.fragments.directory, t.path("config/fragments"));
}

#[test]
fn extend_cycles_are_errors() {
    let t = Tmp::new();
    let a = t.write("a/vers.toml", "extend = \"../b/vers.toml\"\n");
    t.write("b/vers.toml", "extend = \"../a/vers.toml\"\n");
    let (path, m) = config_err(load(&a));
    assert_eq!(path, a);
    assert!(m.contains("cycle"), "{m}");
    assert!(m.contains("a/vers.toml") || m.contains("a\\vers.toml"), "{m}");

    let s = t.write("self/vers.toml", "extend = \"vers.toml\"\n");
    let (_, m) = config_err(load(&s));
    assert!(m.contains("cycle"), "{m}");
}

#[test]
fn extend_to_a_missing_file_names_the_extending_file() {
    let t = Tmp::new();
    let file = t.write("pkg/vers.toml", "extend = \"../nope.toml\"\n[package]\n");
    let (path, m) = config_err(load(&file));
    assert_eq!(path, file);
    assert!(m.contains("../nope.toml") && m.contains("doesn't exist"), "{m}");

    let file = t.write("pkg2/vers.toml", "extend = \"..\"\n");
    let (_, m) = config_err(load(&file));
    assert!(m.contains("directory"), "{m}");

    let file = t.write("pkg3/vers.toml", "extend = \"\"\n");
    let (_, m) = config_err(load(&file));
    assert!(m.contains("empty"), "{m}");
}

#[test]
fn errors_in_an_extended_file_name_that_file() {
    let t = Tmp::new();
    let base = t.write("vers.toml", "[version]\ntagg = true\n");
    let (path, m) = config_err(t.load("pkg/vers.toml", "extend = \"../vers.toml\"\n[package]\n"));
    assert_eq!(path, base);
    assert!(m.contains("tagg"), "{m}");

    let base = t.write("other/vers.toml", "[refs.x]\nmatch = '(\\d+)'\n");
    let (path, m) = config_err(t.load("pkg2/vers.toml", "extend = \"../other/vers.toml\"\n"));
    assert_eq!(path, base);
    assert!(m.contains("named group `id`"), "{m}");
}

// ---------------------------------------------------------------------------
// Repository presets and links
// ---------------------------------------------------------------------------

#[test]
fn github_preset() {
    let t = Tmp::new();
    let c = t
        .load("vers.toml", "[package]\n[changelog]\nrepo = \"github\"\nrepo_url = \"https://github.com/o/r\"\n")
        .unwrap();
    assert_eq!(c.changelog.repo, RepoKind::Github);
    assert_eq!(c.changelog.compare_url.as_deref(), Some("https://github.com/o/r/compare/{from}...{to}"));
    assert_eq!(c.changelog.release_url.as_deref(), Some("https://github.com/o/r/releases/tag/{tag}"));
    let refs: Vec<_> = c.refs.iter().map(|r| (r.name.as_str(), r.display.as_str(), r.url.as_deref())).collect();
    assert_eq!(
        refs,
        vec![
            ("issue", "#{id}", Some("https://github.com/o/r/issues/{id}")),
            ("pr", "#{id}", Some("https://github.com/o/r/pull/{id}")),
        ]
    );
}

#[test]
fn gitlab_preset() {
    let t = Tmp::new();
    let c = t
        .load(
            "vers.toml",
            "[package]\n[changelog]\nrepo = \"gitlab\"\nrepo_url = \"https://gitlab.example.com/team/p/\"\n",
        )
        .unwrap();
    assert_eq!(c.changelog.repo, RepoKind::Gitlab);
    assert_eq!(c.changelog.repo_url.as_deref(), Some("https://gitlab.example.com/team/p"));
    assert_eq!(c.changelog.compare_url.as_deref(), Some("https://gitlab.example.com/team/p/-/compare/{from}...{to}"));
    assert_eq!(c.changelog.release_url.as_deref(), Some("https://gitlab.example.com/team/p/-/tags/{tag}"));
    assert_eq!(c.refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["issue", "mr"]);
    assert_eq!(c.refs[1].display, "!{id}");
    assert_eq!(c.refs[1].url.as_deref(), Some("https://gitlab.example.com/team/p/-/merge_requests/{id}"));
    assert!(c.refs[1].matcher.is_match("mr-45"));
}

#[test]
fn auto_detects_from_explicit_repo_url() {
    let t = Tmp::new();
    let c = t.load("a/vers.toml", "[changelog]\nrepo_url = \"https://gitlab.com/a/b\"\n").unwrap();
    assert_eq!(c.changelog.repo, RepoKind::Gitlab);
    let c = t.load("b/vers.toml", "[changelog]\nrepo = \"auto\"\nrepo_url = \"https://github.com/a/b\"\n").unwrap();
    assert_eq!(c.changelog.repo, RepoKind::Github);
    let c = t.load("c/vers.toml", "[changelog]\nrepo_url = \"https://git.example.com/a/b\"\n").unwrap();
    assert_eq!(c.changelog.repo, RepoKind::None);
    assert_eq!(c.changelog.repo_url.as_deref(), Some("https://git.example.com/a/b"));
    assert_eq!(c.changelog.compare_url, None);
    assert!(c.refs.is_empty());
}

#[test]
fn auto_without_git_is_none() {
    let t = Tmp::new();
    let c = t.load("vers.toml", "[package]\n").unwrap();
    assert_eq!(c.changelog.repo, RepoKind::None);
    assert_eq!(c.changelog.repo_url, None);
}

#[test]
fn explicit_repo_kind_without_url_keeps_ref_kinds_without_links() {
    let t = Tmp::new();
    let c = t.load("vers.toml", "[package]\n[changelog]\nrepo = \"gitlab\"\n").unwrap();
    assert_eq!(c.changelog.repo, RepoKind::Gitlab);
    assert_eq!(c.changelog.repo_url, None);
    assert_eq!(c.changelog.compare_url, None);
    assert_eq!(c.changelog.release_url, None);
    assert_eq!(c.refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["issue", "mr"]);
    assert!(c.refs.iter().all(|r| r.url.is_none()));
}

#[test]
fn repo_none_has_no_refs_or_links_even_with_url() {
    let t = Tmp::new();
    let c = t.load("vers.toml", "[changelog]\nrepo = \"none\"\nrepo_url = \"https://github.com/a/b\"\n").unwrap();
    assert_eq!(c.changelog.repo, RepoKind::None);
    assert_eq!(c.changelog.repo_url.as_deref(), Some("https://github.com/a/b"));
    assert_eq!(c.changelog.compare_url, None);
    assert_eq!(c.changelog.release_url, None);
    assert!(c.refs.is_empty());
}

#[test]
fn link_templates_override_the_preset() {
    let t = Tmp::new();
    let c = t
        .load(
            "a/vers.toml",
            r#"
[changelog]
repo = "none"
repo_url = "https://git.example.com/a/b"
compare_url = "{repo_url}/diff/{from}..{to}"
release_url = "{repo_url}/tag/{tag}"
"#,
        )
        .unwrap();
    assert_eq!(c.changelog.compare_url.as_deref(), Some("https://git.example.com/a/b/diff/{from}..{to}"));
    assert_eq!(c.changelog.release_url.as_deref(), Some("https://git.example.com/a/b/tag/{tag}"));

    // A template that needs an unknown repo_url gives no links...
    let c =
        t.load("b/vers.toml", "[changelog]\nrepo = \"none\"\ncompare_url = \"{repo_url}/c/{from}/{to}\"\n").unwrap();
    assert_eq!(c.changelog.compare_url, None);
    // ...but one that doesn't need it works without.
    let c = t.load("c/vers.toml", "[changelog]\ncompare_url = \"https://x.test/{from}/{to}\"\n").unwrap();
    assert_eq!(c.changelog.compare_url.as_deref(), Some("https://x.test/{from}/{to}"));
    // An empty template turns links off.
    let c = t
        .load(
            "d/vers.toml",
            "[changelog]\nrepo = \"github\"\nrepo_url = \"https://github.com/a/b\"\ncompare_url = \"\"\n",
        )
        .unwrap();
    assert_eq!(c.changelog.compare_url, None);
    assert!(c.changelog.release_url.is_some());
}

// ---------------------------------------------------------------------------
// Reference kinds
// ---------------------------------------------------------------------------

#[test]
fn configured_ref_kinds_come_after_preset_kinds_in_file_order() {
    let t = Tmp::new();
    let c = t
        .load(
            "vers.toml",
            r#"
[changelog]
repo = "gitlab"
repo_url = "https://gitlab.com/a/b"

[refs.zeta]
match = '^(?<id>Z-\d+)$'
display = "{id}"
url = "https://z.test/{id}"

[refs.alpha]
match = '^(?<id>A-\d+)$'
url = "{repo_url}/alpha/{id}"

[refs.jira]
match = '^(?<id>[A-Z][A-Z0-9]+-\d+)$'
"#,
        )
        .unwrap();
    let kinds: Vec<_> = c.refs.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(kinds, vec!["issue", "mr", "zeta", "alpha", "jira"]);
    assert_eq!(c.refs[2].url.as_deref(), Some("https://z.test/{id}"));
    assert_eq!(c.refs[3].url.as_deref(), Some("https://gitlab.com/a/b/alpha/{id}"));
    assert_eq!(c.refs[3].display, "{id}", "display defaults to {{id}}");
    assert_eq!(c.refs[4].url, None, "url is optional");
}

#[test]
fn configured_kind_replaces_preset_kind_in_place() {
    let t = Tmp::new();
    let c = t
        .load(
            "vers.toml",
            r#"
[changelog]
repo = "github"
repo_url = "https://github.com/a/b"

[refs.issue]
match = '^gh-(?<id>\d+)$'
display = "GH-{id}"
url = "https://issues.test/{id}"
"#,
        )
        .unwrap();
    assert_eq!(c.refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["issue", "pr"]);
    assert!(c.refs[0].matcher.is_match("gh-12"));
    assert!(!c.refs[0].matcher.is_match("12"));
    assert_eq!(c.refs[0].display, "GH-{id}");
    assert_eq!(c.refs[0].url.as_deref(), Some("https://issues.test/{id}"));
}

#[test]
fn partial_override_of_preset_kind_keeps_other_preset_fields() {
    let t = Tmp::new();
    let c = t
        .load(
            "vers.toml",
            "[changelog]\nrepo = \"gitlab\"\nrepo_url = \"https://gitlab.com/a/b\"\n[refs.mr]\ndisplay = \"MR {id}\"\n",
        )
        .unwrap();
    assert_eq!(c.refs[1].name, "mr");
    assert_eq!(c.refs[1].display, "MR {id}");
    assert!(c.refs[1].matcher.is_match("mr-3"));
    assert_eq!(c.refs[1].url.as_deref(), Some("https://gitlab.com/a/b/-/merge_requests/{id}"));
}

#[test]
fn ref_kinds_merge_across_extend() {
    let t = Tmp::new();
    t.write("vers.toml", "[refs.jira]\nmatch = '^(?<id>J-\\d+)$'\nurl = \"https://jira.test/{id}\"\n");
    let c = t
        .load(
            "pkg/vers.toml",
            "extend = \"../vers.toml\"\n[package]\n[refs.jira]\ndisplay = \"[{id}]\"\n[refs.other]\nmatch = '^(?<id>O\\d)$'\n",
        )
        .unwrap();
    assert_eq!(c.refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["jira", "other"]);
    assert_eq!(c.refs[0].display, "[{id}]");
    assert_eq!(c.refs[0].url.as_deref(), Some("https://jira.test/{id}"));
    assert!(c.refs[0].matcher.is_match("J-1"));
}

#[test]
fn ref_matchers_must_match_the_whole_name() {
    let t = Tmp::new();
    let c = t.load("vers.toml", "[refs.jira]\nmatch = '(?<id>PROJ-\\d+)'\n").unwrap();
    let m = &c.refs[0].matcher;
    assert!(m.is_match("PROJ-42"));
    assert!(!m.is_match("xPROJ-42"));
    assert!(!m.is_match("PROJ-42-fix"));
    assert_eq!(&m.captures("PROJ-42").unwrap()["id"], "PROJ-42");
    // Alternations are anchored as a whole.
    let c = t.load("b/vers.toml", "[refs.x]\nmatch = 'a(?<id>\\d)|b'\n").unwrap();
    assert!(!c.refs[0].matcher.is_match("a1b"));
    assert!(!c.refs[0].matcher.is_match("xb"));
}

#[test]
fn ref_kind_validation() {
    let t = Tmp::new();
    let (_, m) = config_err(t.load("a/vers.toml", "[refs.jira]\nmatch = '(?<id>[A-Z'\n"));
    assert!(m.contains("invalid `match` regex") && m.contains("jira"), "{m}");
    let (_, m) = config_err(t.load("b/vers.toml", "[refs.jira]\nmatch = '(?<key>\\d+)'\n"));
    assert!(m.contains("named group `id`") && m.contains("jira"), "{m}");
    let (_, m) = config_err(t.load("c/vers.toml", "[refs.jira]\ndisplay = \"{id}\"\n"));
    assert!(m.contains("needs a `match`") && m.contains("jira"), "{m}");
    // A preset name with repo = "none" has no preset to fall back on.
    let (_, m) = config_err(t.load("d/vers.toml", "[changelog]\nrepo = \"none\"\n[refs.issue]\ndisplay = \"#{id}\"\n"));
    assert!(m.contains("needs a `match`"), "{m}");
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[test]
fn custom_types_replace_defaults_and_keep_order() {
    let t = Tmp::new();
    let c = t
        .load(
            "vers.toml",
            r#"
[[types]]
name = "performance"
heading = "Performance"
aliases = ["perf"]
bump = "minor"

[[types]]
name = "internal"
heading = "Internal"
bump = "none"
hidden = true

[[types]]
name = "fixed"
heading = "Fixed"

[[types]]
name = "breaking"
heading = "Breaking"
bump = "major"
"#,
        )
        .unwrap();
    assert_eq!(
        c.types,
        vec![
            ChangeType {
                name: "performance".into(),
                heading: "Performance".into(),
                aliases: vec!["perf".into()],
                bump: Bump::Minor,
                hidden: false
            },
            ChangeType {
                name: "internal".into(),
                heading: "Internal".into(),
                aliases: vec![],
                bump: Bump::None,
                hidden: true
            },
            ChangeType {
                name: "fixed".into(),
                heading: "Fixed".into(),
                aliases: vec![],
                bump: Bump::Patch,
                hidden: false
            },
            ChangeType {
                name: "breaking".into(),
                heading: "Breaking".into(),
                aliases: vec![],
                bump: Bump::Major,
                hidden: false
            },
        ]
    );
    assert_eq!(c.find_type("PERF").unwrap().name, "performance");
    assert!(c.find_type("added").is_none());
}

#[test]
fn type_validation() {
    let cases = [
        ("[[types]]\nname = \"a\"\nheading = \"A\"\nbump = \"huge\"\n", "huge"),
        ("[[types]]\nname = \"a\"\n", "heading"),
        ("[[types]]\nheading = \"A\"\n", "name"),
        ("[[types]]\nname = \"a\"\nheading = \" \"\n", "empty `heading`"),
        ("[[types]]\nname = \"\"\nheading = \"A\"\n", "must not be empty"),
        ("[[types]]\nname = \"a.b\"\nheading = \"A\"\n", "\"a.b\""),
        ("[[types]]\nname = \"a b\"\nheading = \"A\"\n", "\"a b\""),
        ("[[types]]\nname = \"a\"\nheading = \"A\"\naliases = [\"x/y\"]\n", "\"x/y\""),
        ("types = []\n", "at least one"),
        (
            "[[types]]\nname = \"fixed\"\nheading = \"Fixed\"\n[[types]]\nname = \"fixed\"\nheading = \"Fixed again\"\n",
            "`fixed` is used more than once",
        ),
        (
            "[[types]]\nname = \"fixed\"\nheading = \"F\"\naliases = [\"fix\"]\n[[types]]\nname = \"bug\"\nheading = \"B\"\naliases = [\"Fix\"]\n",
            "`Fix` is used more than once (by types `fixed` and `bug`)",
        ),
        (
            "[[types]]\nname = \"added\"\nheading = \"A\"\n[[types]]\nname = \"new\"\nheading = \"N\"\naliases = [\"added\"]\n",
            "`added` is used more than once",
        ),
        ("[[types]]\nname = \"fix\"\nheading = \"F\"\naliases = [\"fix\"]\n", "(by type `fix`)"),
    ];
    for (i, (toml, needle)) in cases.iter().enumerate() {
        let t = Tmp::new();
        let (_, m) = config_err(t.load(&format!("c{i}/vers.toml"), toml));
        assert!(m.contains(needle), "{toml}\n→ {m}");
    }
}

// ---------------------------------------------------------------------------
// Version files
// ---------------------------------------------------------------------------

#[test]
fn version_file_locators() {
    let t = Tmp::new();
    let c = t
        .load(
            "pkg/vers.toml",
            r#"
[[version.files]]
path = "Cargo.toml"
key = "package.version"

[[version.files]]
path = "src/version.h"
pattern = '#define VERSION "(?<version>[^"]+)"'

[[version.files]]
path = "VERSION"
"#,
        )
        .unwrap();
    let files = &c.version.files;
    assert_eq!(files.len(), 3);
    assert_eq!(files[0].path, t.path("pkg/Cargo.toml"));
    assert!(matches!(&files[0].locator, VersionLocator::Key(k) if k == "package.version"));
    assert_eq!(files[1].path, t.path("pkg/src/version.h"));
    match &files[1].locator {
        VersionLocator::Pattern(re) => {
            assert_eq!(&re.captures("#define VERSION \"1.2.3\"").unwrap()["version"], "1.2.3")
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(files[2].locator, VersionLocator::Whole));
}

#[test]
fn version_file_validation() {
    let cases = [
        ("[[version.files]]\npath = \"a.toml\"\nkey = \"v\"\npattern = '(?<version>.*)'\n", "both `key` and `pattern`"),
        ("[[version.files]]\npath = \"a.h\"\npattern = '\"(.*)\"'\n", "named group `version`"),
        ("[[version.files]]\npath = \"a.h\"\npattern = '(?<version>[a'\n", "invalid `pattern` regex"),
        ("[[version.files]]\nkey = \"version\"\n", "path"),
        ("[[version.files]]\npath = \"\"\n", "empty `path`"),
        ("[[version.files]]\npath = \"a.toml\"\nkey = \"package..version\"\n", "invalid `key`"),
        ("[version]\ninitial = \"1.0\"\n", "invalid `version.initial` \"1.0\""),
    ];
    for (i, (toml, needle)) in cases.iter().enumerate() {
        let t = Tmp::new();
        let (_, m) = config_err(t.load(&format!("c{i}/vers.toml"), toml));
        assert!(m.contains(needle), "{toml}\n→ {m}");
    }
}

#[test]
fn version_initial_prerelease() {
    let t = Tmp::new();
    let c = t.load("vers.toml", "[version]\ninitial = \"1.0.0-alpha.1\"\n").unwrap();
    assert_eq!(c.version.initial, Version::parse("1.0.0-alpha.1").unwrap());
}

// ---------------------------------------------------------------------------
// check.ignore
// ---------------------------------------------------------------------------

#[test]
fn invalid_ignore_glob() {
    let t = Tmp::new();
    let (_, m) = config_err(t.load("vers.toml", "[check]\nignore = [\"docs/[a\"]\n"));
    assert!(m.contains("docs/[a") && m.contains("check.ignore"), "{m}");
}

#[test]
fn inherited_ignore_globs_are_rebased_to_the_package() {
    let t = Tmp::new();
    t.write(
        "vers.toml",
        "[check]\nignore = [\"*.md\", \"**/*.snap\", \"packages/api/docs/**\", \"packages/*/bench/**\", \"docs/**\"]\n",
    );
    let c = t.load("packages/api/vers.toml", "extend = \"../../vers.toml\"\n[package]\n").unwrap();
    assert_eq!(c.check.ignore, vec!["*.md", "**/*.snap", "docs/**", "bench/**", "../../docs/**"]);
}

#[test]
fn rebase_glob_rules() {
    let root = resolve::absolute(Path::new("/r"));
    let api = root.join("packages").join("api");
    let shared = root.join("shared");
    assert_eq!(resolve::rebase_glob("docs/**", &api, &api), "docs/**");
    assert_eq!(resolve::rebase_glob("*.md", &root, &api), "*.md");
    assert_eq!(resolve::rebase_glob("packages/api/x/*.rs", &root, &api), "x/*.rs");
    assert_eq!(resolve::rebase_glob("packages/**/*.lock", &root, &api), "**/*.lock");
    assert_eq!(resolve::rebase_glob("**/fixtures/**", &root, &api), "**/fixtures/**");
    assert_eq!(resolve::rebase_glob("packages/web/**", &root, &api), "../../packages/web/**");
    assert_eq!(resolve::rebase_glob("x/**", &shared, &api), "../../shared/x/**");
    assert_eq!(resolve::rebase_glob("*.md", &shared, &api), "../../shared/*.md");
}

#[test]
fn normalize_handles_dot_and_dotdot() {
    let base = resolve::absolute(Path::new("/a"));
    assert_eq!(resolve::normalize(&base.join("b/./c/../d")), base.join("b").join("d"));
    let root = base.parent().unwrap().to_path_buf();
    assert_eq!(resolve::normalize(&root.join("../../x")), root.join("x"));
}

// ---------------------------------------------------------------------------
// discover_packages
// ---------------------------------------------------------------------------

#[test]
fn discovers_packages_sorted_and_skips_shared_configs() {
    let t = Tmp::new();
    t.write("vers.toml", "");
    t.write("packages/web/vers.toml", "extend = \"../../vers.toml\"\n[package]\n");
    t.write("packages/api/vers.toml", "extend = \"../../vers.toml\"\n[package]\n");
    t.write("tools/vers.toml", "# shared\n");
    t.write("packages/api/nested/lib/vers.toml", "[package]\nname = \"lib\"\n");
    let pkgs = discover_packages(&t.root).unwrap();
    assert_eq!(names(&pkgs), vec!["api", "lib", "web"]);
    assert_eq!(pkgs[0].dir, t.path("packages/api"));

    let pkgs = discover_packages(&t.path("packages/web")).unwrap();
    assert_eq!(names(&pkgs), vec!["web"]);
    let pkgs = discover_packages(&t.path("tools")).unwrap();
    assert!(pkgs.is_empty());
}

#[test]
fn discovery_includes_the_root_package() {
    let t = Tmp::new();
    t.write("vers.toml", "[package]\nname = \"root\"\n");
    t.write("sub/vers.toml", "[package]\n");
    assert_eq!(names(&discover_packages(&t.root).unwrap()), vec!["root", "sub"]);
}

#[test]
fn discovery_respects_gitignore_outside_git() {
    let t = Tmp::new();
    t.write(".gitignore", "node_modules/\nbuild\n");
    t.write("a/vers.toml", "[package]\n");
    t.write("node_modules/dep/vers.toml", "[package]\n");
    t.write("a/build/vers.toml", "[package]\nname = \"built\"\n");
    assert_eq!(names(&discover_packages(&t.root).unwrap()), vec!["a"]);
}

#[test]
fn discovery_finds_hidden_files_and_dirs_but_skips_git() {
    let t = Tmp::new();
    t.write("a/.vers.toml", "[package]\n");
    t.write(".hidden/b/vers.toml", "[package]\n");
    let git_dir = t.path("repo");
    std::fs::create_dir_all(git_dir.join(".git").join("x")).unwrap();
    t.write("repo/.git/x/vers.toml", "[package]\nname = \"inside-git\"\n");
    // Sorted by path: ".hidden/b" sorts before "a".
    assert_eq!(names(&discover_packages(&t.root).unwrap()), vec!["b", "a"]);
}

#[test]
fn discovery_uses_dot_vers_toml_precedence() {
    let t = Tmp::new();
    t.write("p/vers.toml", "[package]\nname = \"plain\"\n");
    t.write("p/.vers.toml", "[package]\nname = \"dot\"\n");
    assert_eq!(names(&discover_packages(&t.root).unwrap()), vec!["dot"]);
}

#[test]
fn discovery_rejects_duplicate_names() {
    let t = Tmp::new();
    t.write("a/vers.toml", "[package]\nname = \"same\"\n");
    t.write("b/vers.toml", "[package]\nname = \"same\"\n");
    let m = err_message(discover_packages(&t.root));
    assert!(m.contains("`same`"), "{m}");
    let a = t.path("a/vers.toml").display().to_string();
    let b = t.path("b/vers.toml").display().to_string();
    assert!(m.contains(&a) && m.contains(&b), "{m}");
}

#[test]
fn discovery_reports_invalid_configs() {
    let t = Tmp::new();
    t.write("a/vers.toml", "[package]\nbogus = 1\n");
    let m = err_message(discover_packages(&t.root));
    assert!(m.contains("bogus"), "{m}");
}

// ---------------------------------------------------------------------------
// select_packages
// ---------------------------------------------------------------------------

fn monorepo() -> Tmp {
    let t = Tmp::new();
    t.write("vers.toml", "[version]\ntag_format = \"{name}@v{version}\"\n");
    t.write("packages/api/vers.toml", "extend = \"../../vers.toml\"\n[package]\n");
    t.write("packages/web/vers.toml", "extend = \"../../vers.toml\"\n[package]\n");
    t.mkdir("packages/api/src");
    t.mkdir("tools");
    t
}

#[test]
fn select_current_package() {
    let t = monorepo();
    let pkgs = select_packages(&t.path("packages/api"), &Selection::Current, None).unwrap();
    assert_eq!(names(&pkgs), vec!["api"]);
    assert_eq!(pkgs[0].version.tag_format, "{name}@v{version}");
    let pkgs = select_packages(&t.path("packages/api/src"), &Selection::Current, None).unwrap();
    assert_eq!(names(&pkgs), vec!["api"]);
}

#[test]
fn select_current_under_shared_config_is_an_error() {
    let t = monorepo();
    for dir in ["", "tools", "packages"] {
        let (path, m) = config_err(select_packages(&t.path(dir), &Selection::Current, None));
        assert_eq!(path, t.path("vers.toml"));
        assert!(m.contains("shared config") && m.contains("[package]"), "{m}");
    }
}

#[test]
fn select_without_any_config_suggests_init() {
    let t = Tmp::new();
    let dir = t.mkdir("empty");
    if find_config_for(&dir).unwrap().is_none() {
        for selection in [Selection::Current, Selection::All, Selection::Named(vec!["x".into()])] {
            let m = err_message(select_packages(&dir, &selection, None));
            assert!(m.contains("vers init"), "{selection:?}: {m}");
        }
    }
}

#[test]
fn select_all() {
    let t = monorepo();
    assert_eq!(names(&select_packages(&t.root, &Selection::All, None).unwrap()), vec!["api", "web"]);
    assert_eq!(names(&select_packages(&t.path("packages"), &Selection::All, None).unwrap()), vec!["api", "web"]);
    assert_eq!(names(&select_packages(&t.path("packages/web"), &Selection::All, None).unwrap()), vec!["web"]);
    // Inside a package's subdirectory, that package is the one "at" cwd.
    assert_eq!(names(&select_packages(&t.path("packages/api/src"), &Selection::All, None).unwrap()), vec!["api"]);
}

#[test]
fn select_all_without_packages_is_an_error() {
    let t = monorepo();
    let (_, m) = config_err(select_packages(&t.path("tools"), &Selection::All, None));
    assert!(m.contains("no packages found"), "{m}");
}

#[test]
fn select_named() {
    let t = monorepo();
    let sel = Selection::Named(vec!["web".into(), "api".into(), "web".into()]);
    assert_eq!(names(&select_packages(&t.root, &sel, None).unwrap()), vec!["web", "api"]);
}

#[test]
fn select_named_only_searches_below_cwd() {
    let t = monorepo();
    let sel = Selection::Named(vec!["api".into()]);
    match select_packages(&t.path("packages/web"), &sel, None) {
        Err(Error::Usage(m)) => assert!(m.contains("`api`") && m.contains("available packages: web"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn select_unknown_names_lists_available_packages() {
    let t = monorepo();
    let sel = Selection::Named(vec!["api".into(), "nope".into(), "gone".into()]);
    let err = select_packages(&t.root, &sel, None).unwrap_err();
    assert_eq!(err.exit_code(), 2);
    match err {
        Error::Usage(m) => {
            assert!(m.contains("unknown packages `nope`, `gone`"), "{m}");
            assert!(m.contains("available packages: api, web"), "{m}");
        }
        other => panic!("{other:?}"),
    }
    let sel = Selection::Named(vec!["x".into()]);
    match select_packages(&t.path("tools"), &sel, None) {
        Err(Error::Usage(m)) => assert!(m.contains("unknown package `x`") && m.contains("no packages"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn select_with_config_override() {
    let t = monorepo();
    let custom = t.write("ci/release.toml", "[package]\nname = \"special\"\n");
    // The override applies wherever we are, even under another package.
    for cwd in ["", "packages/api", "tools"] {
        let cwd = t.path(cwd);
        assert_eq!(names(&select_packages(&cwd, &Selection::Current, Some(&custom)).unwrap()), vec!["special"]);
        assert_eq!(names(&select_packages(&cwd, &Selection::All, Some(&custom)).unwrap()), vec!["special"]);
        let sel = Selection::Named(vec!["special".into()]);
        assert_eq!(names(&select_packages(&cwd, &sel, Some(&custom)).unwrap()), vec!["special"]);
    }
    let pkgs = select_packages(&t.root, &Selection::Current, Some(&custom)).unwrap();
    assert_eq!(pkgs[0].dir, t.path("ci"));

    let sel = Selection::Named(vec!["api".into()]);
    match select_packages(&t.root, &sel, Some(&custom)) {
        Err(Error::Usage(m)) => assert!(m.contains("`api`") && m.contains("available packages: special"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn select_with_shared_config_override() {
    let t = monorepo();
    let shared = t.path("vers.toml");
    for selection in [Selection::Current, Selection::All] {
        match select_packages(&t.path("packages/api"), &selection, Some(&shared)) {
            Err(Error::Usage(m)) => assert!(m.contains("--config") && m.contains("[package]"), "{m}"),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn select_with_relative_config_override() {
    let t = monorepo();
    t.write("ci/release.toml", "[package]\nname = \"special\"\n");
    let pkgs = select_packages(&t.root, &Selection::Current, Some(Path::new("ci/release.toml"))).unwrap();
    assert_eq!(pkgs[0].path, t.path("ci/release.toml"));
}

#[test]
fn select_reports_duplicate_between_owner_and_nested() {
    let t = Tmp::new();
    t.write("vers.toml", "[package]\nname = \"x\"\n");
    t.write("sub/inner/vers.toml", "[package]\nname = \"x\"\n");
    let m = err_message(select_packages(&t.path("sub"), &Selection::All, None));
    assert!(m.contains("`x`"), "{m}");
}

#[test]
fn preset_templates_match_preset_ref_kinds() {
    for kind in [RepoKind::Github, RepoKind::Gitlab, RepoKind::None] {
        let kinds = preset_ref_kinds(kind, Some("https://h/x/"));
        let templates = preset_ref_templates(kind);
        assert_eq!(kinds.len(), templates.len());
        for (k, t) in kinds.iter().zip(templates) {
            assert_eq!(k.name, t.name);
            assert_eq!(k.url.as_deref(), Some(t.url.replace("{repo_url}", "https://h/x").as_str()));
        }
        assert!(preset_ref_kinds(kind, None).iter().all(|k| k.url.is_none()));
    }
}
