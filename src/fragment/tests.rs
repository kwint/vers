use std::collections::HashMap;
use std::path::{Path, PathBuf};

use regex::Regex;

use super::*;
use crate::config::{Config, RefKind, RepoKind};
use crate::error::Error;

fn fname(name: &str, type_name: &str, counter: Option<u32>) -> Option<FileName> {
    Some(FileName { name: name.to_string(), type_name: type_name.to_string(), counter })
}

fn jira() -> RefKind {
    RefKind {
        name: "jira".to_string(),
        matcher: Regex::new(r"^(?<id>[A-Z][A-Z0-9]+-\d+)$").unwrap(),
        display: "{id}".to_string(),
        url: Some("https://example.atlassian.net/browse/{id}".to_string()),
    }
}

/// A temp dir with a gitlab config (issue, mr) plus jira, and a `changes/` dir.
struct Env {
    _tmp: tempfile::TempDir,
    config: Config,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let mut config = Config::default_for(&dir, "pkg").with_repo(RepoKind::Gitlab, "https://gitlab.example.com/g/p");
        config.refs.push(jira());
        std::fs::create_dir_all(&config.fragments.directory).unwrap();
        Env { _tmp: tmp, config }
    }

    fn plain() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let config = Config::default_for(&dir, "pkg");
        std::fs::create_dir_all(&config.fragments.directory).unwrap();
        Env { _tmp: tmp, config }
    }

    fn dir(&self) -> &Path {
        &self.config.fragments.directory
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.dir().join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn load(&self, name: &str, content: &str) -> Result<Fragment> {
        let path = self.write(name, content);
        load(&path, &self.config)
    }

    fn ok(&self, name: &str, content: &str) -> Fragment {
        self.load(name, content).unwrap_or_else(|e| panic!("{name} should load: {e}"))
    }

    fn err(&self, name: &str, content: &str) -> String {
        match self.load(name, content) {
            Ok(f) => panic!("{name} should fail, got {f:?}"),
            Err(e @ Error::Fragment { .. }) => e.to_string(),
            Err(e) => panic!("expected a fragment error, got {e:?}"),
        }
    }
}

// ---------------------------------------------------------------- filenames

#[test]
fn parses_name_and_type() {
    assert_eq!(parse_file_name("123.fixed.md"), fname("123", "fixed", None));
    assert_eq!(parse_file_name("mr-45.added.md"), fname("mr-45", "added", None));
    assert_eq!(parse_file_name("PROJ-42.changed.md"), fname("PROJ-42", "changed", None));
    assert_eq!(parse_file_name("oauth_login-2.Feat.md"), fname("oauth_login-2", "Feat", None));
}

#[test]
fn parses_counter() {
    assert_eq!(parse_file_name("123.fixed.2.md"), fname("123", "fixed", Some(2)));
    assert_eq!(parse_file_name("123.fixed.1.md"), fname("123", "fixed", Some(1)));
    assert_eq!(parse_file_name("123.fixed.10.md"), fname("123", "fixed", Some(10)));
}

#[test]
fn rejects_bad_counters() {
    assert_eq!(parse_file_name("123.fixed.0.md"), None);
    assert_eq!(parse_file_name("123.fixed.-1.md"), None);
    assert_eq!(parse_file_name("123.fixed.two.md"), None);
    assert_eq!(parse_file_name("123.fixed.+2.md"), None);
    assert_eq!(parse_file_name("123.fixed.99999999999.md"), None);
}

#[test]
fn rejects_bad_shapes() {
    for name in [
        "README.md",
        "fixed.md",
        ".md",
        "a.b.c.d.md",
        ".fixed.md",
        "123..md",
        "123.fixed..md",
        "my change.fixed.md",
        "#123.fixed.md",
        "!45.added.md",
        "café.fixed.md",
    ] {
        assert_eq!(parse_file_name(name), None, "{name}");
    }
}

#[test]
fn non_markdown_files_are_not_fragments() {
    for name in [".gitkeep", "123.fixed.txt", "123.fixed", "123.fixed.md.swp", "123.fixed.MD"] {
        assert_eq!(parse_file_name(name), None, "{name}");
    }
}

// --------------------------------------------------------------- references

#[test]
fn looks_like_ref_rules() {
    for yes in ["123", "0", "mr-45", "ABC-12", "pr-1", "Proj-7"] {
        assert!(looks_like_ref(yes), "{yes}");
    }
    for no in ["oauth-login", "fix-login-123", "mr-", "-45", "abc", "a1-2", "mr-45a", "", "mr_45", "x-1-2"] {
        assert!(!looks_like_ref(no), "{no}");
    }
}

#[test]
fn resolve_ref_first_match_wins() {
    let catch_all = RefKind {
        name: "any".to_string(),
        matcher: Regex::new(r"^(?<id>.+)$").unwrap(),
        display: "any {id}".to_string(),
        url: None,
    };
    let env = Env::new();
    let mut kinds = env.config.refs.clone();
    kinds.push(catch_all.clone());

    let r = resolve_ref("123", &kinds).unwrap();
    assert_eq!(r.kind.as_deref(), Some("issue"));
    assert_eq!(r.display, "#123");
    assert_eq!(r.url.as_deref(), Some("https://gitlab.example.com/g/p/-/issues/123"));
    assert_eq!(r.raw, "123");

    let r = resolve_ref("mr-45", &kinds).unwrap();
    assert_eq!(r.kind.as_deref(), Some("mr"));
    assert_eq!(r.display, "!45");
    assert_eq!(r.url.as_deref(), Some("https://gitlab.example.com/g/p/-/merge_requests/45"));
    assert_eq!(r.raw, "mr-45");

    let r = resolve_ref("PROJ-42", &kinds).unwrap();
    assert_eq!(r.kind.as_deref(), Some("jira"));
    assert_eq!(r.display, "PROJ-42");

    let r = resolve_ref("whatever", &kinds).unwrap();
    assert_eq!(r.kind.as_deref(), Some("any"));
    assert_eq!(r.display, "any whatever");
    assert_eq!(r.url, None);

    // Order matters: the catch-all first shadows everything.
    let shadowed = vec![catch_all, jira()];
    assert_eq!(resolve_ref("PROJ-42", &shadowed).unwrap().kind.as_deref(), Some("any"));
}

#[test]
fn resolve_ref_without_match_or_url() {
    assert_eq!(resolve_ref("123", &[]), None);
    let env = Env::new();
    assert_eq!(resolve_ref("oauth-login", &env.config.refs), None);
    assert_eq!(resolve_ref("pr-45", &env.config.refs), None);

    let no_url = crate::config::preset_ref_kinds(RepoKind::Github, None);
    let r = resolve_ref("pr-9", &no_url).unwrap();
    assert_eq!(r.display, "#9");
    assert_eq!(r.url, None);
}

#[test]
fn resolve_ref_without_id_group_uses_whole_match() {
    let kind = RefKind {
        name: "t".to_string(),
        matcher: Regex::new(r"^T\d+$").unwrap(),
        display: "ticket {id}".to_string(),
        url: Some("https://t/{id}".to_string()),
    };
    let r = resolve_ref("T7", &[kind]).unwrap();
    assert_eq!(r.display, "ticket T7");
    assert_eq!(r.url.as_deref(), Some("https://t/T7"));
}

#[test]
fn filename_reference_is_linked() {
    let env = Env::new();
    let f = env.ok("mr-45.added.md", "Add dark mode.\n");
    assert_eq!(f.name, "mr-45");
    assert_eq!(f.type_name, "added");
    assert_eq!(f.counter, None);
    assert_eq!(
        f.refs,
        vec![Ref {
            raw: "mr-45".to_string(),
            kind: Some("mr".to_string()),
            display: "!45".to_string(),
            url: Some("https://gitlab.example.com/g/p/-/merge_requests/45".to_string()),
        }]
    );
    assert_eq!(f.path, env.dir().join("mr-45.added.md"));
}

#[test]
fn slug_has_no_reference() {
    let env = Env::new();
    let f = env.ok("oauth-login.added.md", "OAuth login.");
    assert!(f.refs.is_empty());
    assert_eq!(f.name, "oauth-login");
}

#[test]
fn unmatched_reference_in_filename_is_an_error() {
    let env = Env::new();
    let msg = env.err("pr-45.added.md", "Text.");
    assert!(msg.contains("`pr-45` looks like a reference"), "{msg}");
    assert!(msg.contains("[refs.<kind>]"), "{msg}");
    assert!(msg.contains("`mr`"), "{msg}");
    assert!(msg.contains("pr-45.added.md"), "{msg}");

    let env = Env::plain();
    let msg = env.err("123.fixed.md", "Text.");
    assert!(msg.contains("no reference kinds are configured"), "{msg}");
    // Without kinds, a non-ref-looking name is still a fine slug.
    assert!(env.ok("fix-123-crash.fixed.md", "Text.").refs.is_empty());
}

#[test]
fn frontmatter_refs_follow_filename_ref() {
    let env = Env::new();
    let f = env.ok("123.fixed.md", "---\nrefs: [\"mr-130\", \"PROJ-7\"]\n---\nFix it.\n");
    let raws: Vec<_> = f.refs.iter().map(|r| r.raw.as_str()).collect();
    assert_eq!(raws, ["123", "mr-130", "PROJ-7"]);
    let displays: Vec<_> = f.refs.iter().map(|r| r.display.as_str()).collect();
    assert_eq!(displays, ["#123", "!130", "PROJ-7"]);
}

#[test]
fn unmatched_frontmatter_refs_are_plain_text() {
    let env = Env::new();
    let f = env.ok("slug.fixed.md", "---\nrefs: [\"pr-9\", \"see the forum\", 123]\n---\nFix it.\n");
    assert_eq!(f.refs[0], Ref { raw: "pr-9".to_string(), kind: None, display: "pr-9".to_string(), url: None });
    assert_eq!(f.refs[1].kind, None);
    assert_eq!(f.refs[1].display, "see the forum");
    // Integers are accepted and resolved like strings.
    assert_eq!(f.refs[2].kind.as_deref(), Some("issue"));
    assert_eq!(f.refs[2].display, "#123");
}

#[test]
fn empty_frontmatter_ref_is_an_error() {
    let env = Env::new();
    let msg = env.err("slug.fixed.md", "---\nrefs: [\"\"]\n---\nFix it.\n");
    assert!(msg.contains("empty entry"), "{msg}");
}

// -------------------------------------------------------------------- types

#[test]
fn types_are_case_insensitive_and_aliases_resolve() {
    let env = Env::new();
    assert_eq!(env.ok("a.FIXED.md", "x").type_name, "fixed");
    assert_eq!(env.ok("b.feat.md", "x").type_name, "added");
    assert_eq!(env.ok("c.Bugfix.md", "x").type_name, "fixed");
    assert_eq!(env.ok("d.removal.2.md", "x").type_name, "removed");
    assert_eq!(env.ok("d.removal.2.md", "x").counter, Some(2));
}

#[test]
fn unknown_type_is_an_error() {
    let env = Env::new();
    let msg = env.err("123.fxed.md", "x");
    assert!(msg.contains("unknown change type `fxed`"), "{msg}");
    assert!(msg.contains("added, changed, deprecated, removed, fixed, security"), "{msg}");
}

#[test]
fn bad_filename_is_an_error_in_load() {
    let env = Env::new();
    let msg = env.err("README.md", "x");
    assert!(msg.contains("invalid fragment filename `README.md`"), "{msg}");
    assert!(msg.contains("<name>.<type>[.<counter>].md"), "{msg}");
}

// -------------------------------------------------------------- frontmatter

#[test]
fn yaml_frontmatter() {
    let env = Env::new();
    let f = env.ok("x.changed.md", "---\nbreaking: true\nrefs: [\"mr-130\", \"PROJ-7\"]\n---\nBody.\n");
    assert!(f.breaking);
    assert_eq!(f.refs.len(), 2);
    assert_eq!(f.body, "Body.");

    let f = env.ok("y.changed.md", "---\nrefs:\n  - mr-1\n  - '2'\nbreaking: false\n---\nBody.\n");
    assert!(!f.breaking);
    assert_eq!(f.refs.iter().map(|r| r.display.as_str()).collect::<Vec<_>>(), ["!1", "#2"]);
}

#[test]
fn toml_frontmatter() {
    let env = Env::new();
    let f = env.ok("x.changed.md", "+++\nbreaking = true\nrefs = [\"mr-130\", 7]\n+++\nBody.\n");
    assert!(f.breaking);
    assert_eq!(f.refs.iter().map(|r| r.display.as_str()).collect::<Vec<_>>(), ["!130", "#7"]);
    assert_eq!(f.body, "Body.");
}

#[test]
fn empty_frontmatter_is_allowed() {
    let env = Env::new();
    let f = env.ok("x.changed.md", "---\n---\nBody.\n");
    assert!(!f.breaking);
    assert!(f.refs.is_empty());
    let f = env.ok("y.changed.md", "+++\n\n+++\nBody.\n");
    assert_eq!(f.body, "Body.");
}

#[test]
fn no_frontmatter_defaults() {
    let env = Env::new();
    let f = env.ok("x.changed.md", "Body.\n---\nMore.\n");
    assert!(!f.breaking);
    assert_eq!(f.body, "Body.\n---\nMore.");
}

#[test]
fn frontmatter_must_be_at_the_very_start() {
    let env = Env::new();
    // A blank line before `---` means it's body text, not frontmatter.
    let f = env.ok("x.changed.md", "\n---\nbreaking: true\n---\nBody.\n");
    assert!(!f.breaking);
    assert!(f.body.starts_with("---"));
}

#[test]
fn unknown_frontmatter_keys_are_errors() {
    let env = Env::new();
    let msg = env.err("x.changed.md", "---\nbraeking: true\n---\nBody.\n");
    assert!(msg.contains("invalid YAML frontmatter"), "{msg}");
    assert!(msg.contains("braeking"), "{msg}");

    let msg = env.err("y.changed.md", "+++\nbraeking = true\n+++\nBody.\n");
    assert!(msg.contains("invalid TOML frontmatter"), "{msg}");
    assert!(msg.contains("braeking"), "{msg}");
}

#[test]
fn malformed_frontmatter_is_an_error() {
    let env = Env::new();
    for (name, content, want) in [
        ("a.fixed.md", "---\nbreaking: [\n---\nBody.\n", "invalid YAML frontmatter"),
        ("b.fixed.md", "---\nbreaking: \"yes please\"\n---\nBody.\n", "invalid YAML frontmatter"),
        ("c.fixed.md", "---\nrefs: mr-1\n---\nBody.\n", "invalid YAML frontmatter"),
        ("d.fixed.md", "---\n- a\n---\nBody.\n", "invalid YAML frontmatter"),
        ("e.fixed.md", "+++\nbreaking = \n+++\nBody.\n", "invalid TOML frontmatter"),
        ("f.fixed.md", "+++\nrefs = [true]\n+++\nBody.\n", "invalid TOML frontmatter"),
        ("g.fixed.md", "---\nbreaking: true\nBody.\n", "unterminated YAML frontmatter"),
        ("h.fixed.md", "+++\nbreaking = true\n---\nBody.\n", "unterminated TOML frontmatter"),
    ] {
        let msg = env.err(name, content);
        assert!(msg.contains(want), "{name}: {msg}");
    }
}

#[test]
fn crlf_and_bom_are_handled() {
    let env = Env::new();
    let f = env.ok(
        "x.changed.md",
        "\u{feff}---\r\nbreaking: true\r\nrefs: [\"mr-1\"]\r\n---\r\n- First line.\r\n\r\nSecond.  \r\n",
    );
    assert!(f.breaking);
    assert_eq!(f.refs[0].display, "!1");
    assert_eq!(f.body, "First line.\n\nSecond.");

    let f = env.ok("y.changed.md", "+++\r\nbreaking = true\r\n+++\r\nBody.\r\n");
    assert!(f.breaking);
    assert_eq!(f.body, "Body.");
}

// --------------------------------------------------------------------- body

#[test]
fn body_is_normalised() {
    let env = Env::new();
    let cases = [
        ("Fix it.", "Fix it."),
        ("\n\n  \nFix it.\n\n\n", "Fix it."),
        ("- Fix it.\n", "Fix it."),
        ("* Fix it.\n", "Fix it."),
        ("- - Nested.\n", "- Nested."),
        ("   - Indented bullet.\n", "Indented bullet."),
        ("-Not a bullet.\n", "-Not a bullet."),
        ("Line one.   \n\n  Indented line two.\t\n", "Line one.\n\n  Indented line two."),
        ("First.\n- a list\n- inside\n", "First.\n- a list\n- inside"),
        ("**Bold** start.", "**Bold** start."),
    ];
    for (i, (input, want)) in cases.iter().enumerate() {
        let f = env.ok(&format!("b{i}.fixed.md"), input);
        assert_eq!(f.body, *want, "input {input:?}");
    }
}

#[test]
fn empty_body_is_an_error() {
    let env = Env::new();
    for (i, content) in
        ["", "\n\n", "   \n\t\n", "---\nbreaking: true\n---\n", "---\nbreaking: true\n---\n\n  \n"].iter().enumerate()
    {
        let msg = env.err(&format!("e{i}.fixed.md"), content);
        assert!(msg.contains("body is empty"), "{content:?}: {msg}");
    }
}

#[test]
fn unreadable_fragment_is_io_error() {
    let env = Env::new();
    let err = load(&env.dir().join("missing.fixed.md"), &env.config).unwrap_err();
    assert!(matches!(err, Error::Io { .. }), "{err:?}");
}

// ----------------------------------------------------------------- load_all

#[test]
fn load_all_missing_directory_is_empty() {
    let env = Env::new();
    std::fs::remove_dir(env.dir()).unwrap();
    assert!(load_all(&env.config).unwrap().is_empty());
}

#[test]
fn load_all_reads_fragments_in_filename_order() {
    let env = Env::new();
    env.write("b.fixed.md", "B.");
    env.write("a.added.md", "A.");
    env.write("123.fixed.2.md", "Two.");
    env.write("123.fixed.md", "One.");
    env.write(".gitkeep", "");
    env.write("notes.txt", "not a fragment");
    std::fs::create_dir(env.dir().join("sub")).unwrap();
    std::fs::write(env.dir().join("sub").join("nested.fixed.md"), "Nested.").unwrap();
    // A directory named like a fragment is not a fragment either.
    std::fs::create_dir(env.dir().join("dir.fixed.md")).unwrap();

    let all = load_all(&env.config).unwrap();
    let names: Vec<_> = all.iter().map(|f| f.path.file_name().unwrap().to_str().unwrap()).collect();
    assert_eq!(names, ["123.fixed.2.md", "123.fixed.md", "a.added.md", "b.fixed.md"]);
}

#[test]
fn load_all_reports_bad_markdown_names() {
    let env = Env::new();
    env.write("a.added.md", "A.");
    env.write("README.md", "About this directory.");
    let err = load_all(&env.config).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("invalid fragment filename `README.md`"), "{msg}");
}

#[test]
fn load_all_reports_invalid_fragments() {
    let env = Env::new();
    env.write("a.added.md", "A.");
    env.write("b.nope.md", "B.");
    let msg = load_all(&env.config).unwrap_err().to_string();
    assert!(msg.contains("unknown change type `nope`"), "{msg}");
}

// --------------------------------------------------------------------- sort

fn frag(path: PathBuf) -> Fragment {
    let name = path.file_name().unwrap().to_str().unwrap().to_string();
    let parsed = parse_file_name(&name).unwrap();
    Fragment {
        path,
        name: parsed.name,
        type_name: "fixed".to_string(),
        counter: parsed.counter,
        refs: Vec::new(),
        breaking: false,
        body: name,
    }
}

fn file_names(fragments: &[Fragment]) -> Vec<String> {
    fragments.iter().map(|f| f.path.file_name().unwrap().to_str().unwrap().to_string()).collect()
}

#[test]
fn sort_by_git_time_then_filename() {
    let dir = PathBuf::from("/nonexistent-vers-test/changes");
    let mut fs = vec![
        frag(dir.join("a.fixed.md")),
        frag(dir.join("b.fixed.md")),
        frag(dir.join("c.fixed.md")),
        frag(dir.join("d.fixed.md")),
    ];
    let times: HashMap<PathBuf, i64> =
        [(dir.join("a.fixed.md"), 300), (dir.join("b.fixed.md"), 100), (dir.join("c.fixed.md"), 100)]
            .into_iter()
            .collect();
    sort(&mut fs, &times);
    // b and c tie on time → filename; d has no time at all → last.
    assert_eq!(file_names(&fs), ["b.fixed.md", "c.fixed.md", "a.fixed.md", "d.fixed.md"]);
}

#[test]
fn sort_without_any_times_is_alphabetical() {
    let dir = PathBuf::from("/nonexistent-vers-test/changes");
    let mut fs = vec![frag(dir.join("b.fixed.md")), frag(dir.join("10.fixed.md")), frag(dir.join("a.fixed.md"))];
    sort(&mut fs, &HashMap::new());
    assert_eq!(file_names(&fs), ["10.fixed.md", "a.fixed.md", "b.fixed.md"]);
}

#[test]
fn sort_uses_creation_time_for_files_git_does_not_know() {
    let env = Env::new();
    let older = env.write("zzz.fixed.md", "Z.");
    std::thread::sleep(std::time::Duration::from_millis(30));
    let newer = env.write("aaa.fixed.md", "A.");
    let committed = env.dir().join("mmm.fixed.md");
    std::fs::write(&committed, "M.").unwrap();
    let missing = env.dir().join("000.fixed.md");

    let created = |p: &Path| std::fs::metadata(p).and_then(|m| m.created()).ok();
    let mut fs = vec![frag(newer.clone()), frag(missing), frag(committed.clone()), frag(older.clone())];
    // Committed long ago; the git time wins over its (recent) creation time.
    let times: HashMap<PathBuf, i64> = [(committed, 1_000)].into_iter().collect();
    sort(&mut fs, &times);

    match (created(&older), created(&newer)) {
        (Some(a), Some(b)) if a < b => {
            assert_eq!(file_names(&fs), ["mmm.fixed.md", "zzz.fixed.md", "aaa.fixed.md", "000.fixed.md"]);
        }
        (Some(_), Some(_)) => {} // Coarse timestamps: nothing useful to assert about these two.
        _ => {
            // No btime support: only the git time is known; the rest is alphabetical.
            assert_eq!(file_names(&fs), ["mmm.fixed.md", "000.fixed.md", "aaa.fixed.md", "zzz.fixed.md"]);
        }
    }
}

#[test]
fn sort_matches_git_times_by_canonical_path() {
    let env = Env::new();
    let a = env.write("a.fixed.md", "A.");
    let b = env.write("b.fixed.md", "B.");
    // Fragment paths written in a non-canonical form.
    let indirect = |p: &Path| env.dir().join("..").join("changes").join(p.file_name().unwrap());
    let mut fs = vec![frag(indirect(&a)), frag(indirect(&b))];
    let times: HashMap<PathBuf, i64> = [(a, 200), (b, 100)].into_iter().collect();
    sort(&mut fs, &times);
    assert_eq!(file_names(&fs), ["b.fixed.md", "a.fixed.md"]);
}

// --------------------------------------------------------------------- slug

#[test]
fn slug_examples_from_spec() {
    assert_eq!(slug_from_message("Return 404 instead of 400 for unknown orders."), "return-404-instead-of-400-for");
    assert_eq!(slug_from_message("Don't crash on an empty name."), "don-t-crash-on-an-empty");
    assert_eq!(slug_from_message("Greet people by name."), "greet-people-by-name");
}

#[test]
fn slug_edge_cases() {
    assert_eq!(slug_from_message(""), "change");
    assert_eq!(slug_from_message("  ... !!! "), "change");
    assert_eq!(slug_from_message("--Fix   the__thing--"), "fix-the-thing");
    assert_eq!(slug_from_message("Multi\nline\r\nmessage here"), "multi-line-message-here");
    assert_eq!(slug_from_message("Café crème brûlée für Straße"), "cafe-creme-brulee-fur-strasse");
    assert_eq!(slug_from_message("Ünïcödé"), "unicode");
    assert_eq!(slug_from_message("日本語 support"), "support");
    assert_eq!(slug_from_message("日本語"), "change");
    // Results that would look like a reference are prefixed.
    assert_eq!(slug_from_message("2024"), "change-2024");
    assert_eq!(slug_from_message("ABC-12"), "change-abc-12");
    assert_eq!(slug_from_message("Fix ABC-12"), "fix-abc-12");
}

#[test]
fn slugs_are_valid_fragment_names() {
    for msg in ["Return 404.", "Don't", "Ünïcödé ✓ 🎉 ok", "x", "___", "a.b.c", "Ωμέγα"] {
        let slug = slug_from_message(msg);
        assert!(is_valid_name(&slug), "{msg:?} → {slug:?}");
        assert!(!looks_like_ref(&slug), "{msg:?} → {slug:?}");
        assert!(parse_file_name(&format!("{slug}.fixed.md")).is_some(), "{slug}");
    }
}

// --------------------------------------------------------- new_fragment_path

#[test]
fn new_fragment_path_adds_counter() {
    let env = Env::new();
    let p1 = new_fragment_path(&env.config, "123", "fixed");
    assert_eq!(p1, env.dir().join("123.fixed.md"));
    std::fs::write(&p1, "x").unwrap();

    let p2 = new_fragment_path(&env.config, "123", "fixed");
    assert_eq!(p2, env.dir().join("123.fixed.2.md"));
    std::fs::write(&p2, "x").unwrap();

    assert_eq!(new_fragment_path(&env.config, "123", "fixed"), env.dir().join("123.fixed.3.md"));
    // Other types and names are independent.
    assert_eq!(new_fragment_path(&env.config, "123", "added"), env.dir().join("123.added.md"));
    assert_eq!(new_fragment_path(&env.config, "124", "fixed"), env.dir().join("124.fixed.md"));
}

#[test]
fn new_fragment_path_in_missing_directory() {
    let env = Env::new();
    std::fs::remove_dir(env.dir()).unwrap();
    assert_eq!(new_fragment_path(&env.config, "slug", "added"), env.dir().join("slug.added.md"));
}

// --------------------------------------------------------------- render_file

#[test]
fn render_plain_body() {
    assert_eq!(render_file("Fix the login redirect.", &[], false), "Fix the login redirect.\n");
    assert_eq!(render_file("Fix it.\n\n\n", &[], false), "Fix it.\n");
    assert_eq!(render_file("Line one.\n\nLine two.", &[], false), "Line one.\n\nLine two.\n");
}

#[test]
fn render_frontmatter() {
    assert_eq!(
        render_file("Fix the login redirect.", &["mr-130".to_string()], false),
        "---\nrefs: [\"mr-130\"]\n---\nFix the login redirect.\n"
    );
    assert_eq!(render_file("Body.", &[], true), "---\nbreaking: true\n---\nBody.\n");
    assert_eq!(
        render_file("Body.", &["mr-130".to_string(), "PROJ-7".to_string()], true),
        "---\nbreaking: true\nrefs: [\"mr-130\", \"PROJ-7\"]\n---\nBody.\n"
    );
}

#[test]
fn rendered_files_load_back() {
    let env = Env::new();
    let refs = vec!["mr-130".to_string(), "with \"quotes\" and: colon".to_string()];
    let content = render_file("- Body text.\n\nMore.", &refs, true);
    let f = env.ok("123.fixed.md", &content);
    assert!(f.breaking);
    assert_eq!(
        f.refs.iter().map(|r| r.raw.as_str()).collect::<Vec<_>>(),
        ["123", "mr-130", "with \"quotes\" and: colon"]
    );
    assert_eq!(f.body, "Body text.\n\nMore.");
}

#[test]
fn render_protects_body_that_looks_like_frontmatter() {
    let env = Env::new();
    let content = render_file("---\nbreaking: true\n---\nBody.", &[], false);
    assert_eq!(content, "---\n---\n---\nbreaking: true\n---\nBody.\n");
    let f = env.ok("x.fixed.md", &content);
    assert!(!f.breaking);
    assert_eq!(f.body, "---\nbreaking: true\n---\nBody.");
}
