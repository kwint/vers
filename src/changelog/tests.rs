use std::path::{Path, PathBuf};

use semver::Version;

use super::*;
use crate::config::{Config, RefPosition, RepoKind, default_types};
use crate::fragment::{Fragment, Ref};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn config() -> Config {
    Config::default_for(Path::new("/repo"), "app")
}

fn github() -> Config {
    config().with_repo(RepoKind::Github, "https://github.com/example/greeter")
}

fn gitlab() -> Config {
    config().with_repo(RepoKind::Gitlab, "https://gitlab.com/acme/app")
}

fn issue(id: u32) -> Ref {
    Ref {
        raw: id.to_string(),
        kind: Some("issue".into()),
        display: format!("#{id}"),
        url: Some(format!("https://github.com/example/greeter/issues/{id}")),
    }
}

fn plain(raw: &str) -> Ref {
    Ref { raw: raw.into(), kind: None, display: raw.into(), url: None }
}

fn frag(name: &str, type_name: &str, body: &str) -> Fragment {
    Fragment {
        path: PathBuf::from(format!("changes/{name}.{type_name}.md")),
        name: name.into(),
        type_name: type_name.into(),
        counter: None,
        refs: Vec::new(),
        breaking: false,
        body: body.into(),
    }
}

fn with_refs(mut f: Fragment, refs: Vec<Ref>) -> Fragment {
    f.refs = refs;
    f
}

fn breaking(mut f: Fragment) -> Fragment {
    f.breaking = true;
    f
}

fn section(heading: &str, entries: &[&str]) -> Section {
    Section { heading: heading.into(), entries: entries.iter().map(|e| e.to_string()).collect() }
}

fn release(version: &str, date: &str, sections: Vec<Section>) -> Release {
    Release { version: v(version), date: date.into(), sections }
}

// ---------------------------------------------------------------------------
// render_entry
// ---------------------------------------------------------------------------

#[test]
fn entry_plain() {
    assert_eq!(render_entry(&frag("greet", "added", "Greet people by name.\n"), &config()), "- Greet people by name.");
}

#[test]
fn entry_with_ref_at_end() {
    let f = with_refs(frag("7", "fixed", "Don't crash on an empty name."), vec![issue(7)]);
    assert_eq!(
        render_entry(&f, &config()),
        "- Don't crash on an empty name. ([#7](https://github.com/example/greeter/issues/7))"
    );
}

#[test]
fn entry_with_several_refs_and_plain_ref() {
    let f = with_refs(frag("12", "fixed", "Fix crash on startup."), vec![issue(12), issue(15), plain("PROJ-9")]);
    assert_eq!(
        render_entry(&f, &config()),
        "- Fix crash on startup. ([#12](https://github.com/example/greeter/issues/12), \
         [#15](https://github.com/example/greeter/issues/15), PROJ-9)"
    );
}

#[test]
fn entry_with_ref_at_start() {
    let mut c = config();
    c.changelog.ref_position = RefPosition::Start;
    let f = with_refs(frag("123", "fixed", "Fix the login redirect."), vec![issue(123)]);
    assert_eq!(render_entry(&f, &c), "- [#123](https://github.com/example/greeter/issues/123) Fix the login redirect.");
    let f = breaking(f);
    assert_eq!(
        render_entry(&f, &c),
        "- **Breaking:** [#123](https://github.com/example/greeter/issues/123) Fix the login redirect."
    );
}

#[test]
fn entry_start_refs_with_multiline_body_go_on_first_line() {
    let mut c = config();
    c.changelog.ref_position = RefPosition::Start;
    let f = with_refs(frag("1", "fixed", "First.\n\nSecond."), vec![plain("X-1")]);
    assert_eq!(render_entry(&f, &c), "- X-1 First.\n\n  Second.");
}

#[test]
fn entry_breaking_multiline_matches_docs_example() {
    let refs = vec![
        Ref { raw: "88".into(), kind: Some("issue".into()), display: "#88".into(), url: Some("…".into()) },
        Ref { raw: "mr-130".into(), kind: Some("mr".into()), display: "!130".into(), url: Some("…".into()) },
        Ref { raw: "PROJ-7".into(), kind: Some("jira".into()), display: "PROJ-7".into(), url: Some("…".into()) },
    ];
    let body = "The `timeout` setting is now in seconds instead of milliseconds.\n\n\
                Update your config: `timeout = 30000` becomes `timeout = 30`.\n";
    let f = breaking(with_refs(frag("88", "changed", body), refs));
    assert_eq!(
        render_entry(&f, &config()),
        "- **Breaking:** The `timeout` setting is now in seconds instead of milliseconds.\n\
         \n  Update your config: `timeout = 30000` becomes `timeout = 30`. ([#88](…), [!130](…), [PROJ-7](…))"
    );
}

#[test]
fn entry_custom_breaking_prefix() {
    let mut c = config();
    c.changelog.breaking_prefix = "BREAKING: ".into();
    assert_eq!(render_entry(&breaking(frag("x", "removed", "Drop v1 API.")), &c), "- BREAKING: Drop v1 API.");
}

#[test]
fn entry_continuation_lines_indented_blank_lines_empty() {
    let f = frag("x", "added", "\n\nFirst line\nsecond line\n   \n- nested item\n  deeper\n\n");
    assert_eq!(render_entry(&f, &config()), "- First line\n  second line\n\n  - nested item\n    deeper");
}

#[test]
fn entry_crlf_body() {
    let f = frag("x", "added", "One\r\n\r\nTwo\r\n");
    assert_eq!(render_entry(&f, &config()), "- One\n\n  Two");
}

#[test]
fn entry_keeps_markdown_hard_break_spaces() {
    let f = frag("x", "added", "One  \nTwo");
    assert_eq!(render_entry(&f, &config()), "- One  \n  Two");
}

// ---------------------------------------------------------------------------
// build_release / render_release
// ---------------------------------------------------------------------------

#[test]
fn draft_matches_first_release_tutorial() {
    let c = github();
    let fragments = vec![
        frag("greet-people-by-name", "added", "Greet people by name.\n"),
        with_refs(frag("7", "fixed", "Don't crash on an empty name.\n"), vec![issue(7)]),
    ];
    let r = build_release(v("0.1.0"), "2026-10-14".into(), &fragments, &c.types, &c);
    assert_eq!(
        render_release(&r),
        "## [0.1.0] - 2026-10-14

### Added

- Greet people by name.

### Fixed

- Don't crash on an empty name. ([#7](https://github.com/example/greeter/issues/7))
"
    );
}

#[test]
fn render_release_layout() {
    let r = release("1.0.0", "2026-01-02", vec![section("Added", &["- a", "- b"]), section("Fixed", &["- c"])]);
    assert_eq!(render_release(&r), "## [1.0.0] - 2026-01-02\n\n### Added\n\n- a\n- b\n\n### Fixed\n\n- c\n");
}

#[test]
fn build_groups_in_type_order_and_keeps_fragment_order() {
    let c = config();
    let fragments = vec![
        frag("a", "fixed", "Fix A."),
        frag("b", "security", "Sec B."),
        frag("c", "added", "Add C."),
        frag("d", "fixed", "Fix D."),
        frag("e", "added", "Add E."),
    ];
    let r = build_release(v("1.0.0"), "2026-01-01".into(), &fragments, &c.types, &c);
    assert_eq!(
        r.sections,
        vec![
            section("Added", &["- Add C.", "- Add E."]),
            section("Fixed", &["- Fix A.", "- Fix D."]),
            section("Security", &["- Sec B."]),
        ]
    );
}

#[test]
fn build_follows_custom_type_order_and_skips_hidden() {
    let c = config();
    let mut types = default_types();
    types.reverse();
    types.push(crate::config::ChangeType {
        name: "internal".into(),
        heading: "Internal".into(),
        aliases: vec![],
        bump: crate::version::Bump::None,
        hidden: true,
    });
    let fragments = vec![frag("a", "added", "A."), frag("b", "fixed", "B."), frag("c", "internal", "Hidden.")];
    let r = build_release(v("1.0.0"), "2026-01-01".into(), &fragments, &types, &c);
    assert_eq!(r.sections, vec![section("Fixed", &["- B."]), section("Added", &["- A."])]);
}

#[test]
fn build_merges_identical_bodies() {
    let c = config();
    let fragments = vec![
        with_refs(frag("12", "fixed", "Fix crash on startup.\n"), vec![issue(12)]),
        frag("other", "fixed", "Something else."),
        breaking(with_refs(frag("15", "fixed", "\nFix crash on startup.  \r\n"), vec![issue(15), issue(12)])),
        // Same body, different type: not merged.
        with_refs(frag("20", "added", "Fix crash on startup."), vec![issue(20)]),
    ];
    let r = build_release(v("1.0.0"), "2026-01-01".into(), &fragments, &c.types, &c);
    assert_eq!(
        r.sections,
        vec![
            section("Added", &["- Fix crash on startup. ([#20](https://github.com/example/greeter/issues/20))"]),
            section(
                "Fixed",
                &[
                    "- **Breaking:** Fix crash on startup. ([#12](https://github.com/example/greeter/issues/12), \
                     [#15](https://github.com/example/greeter/issues/15))",
                    "- Something else.",
                ]
            ),
        ]
    );
}

#[test]
fn build_drops_empty_sections_and_unknown_types() {
    let c = config();
    let r = build_release(v("1.0.0"), "d".into(), &[frag("a", "nonsense", "A.")], &c.types, &c);
    assert!(r.sections.is_empty());
}

#[test]
fn merge_earlier_puts_earlier_first_and_orders_by_type() {
    let types = default_types();
    let mut r =
        release("1.3.0", "2026-10-14", vec![section("Added", &["- new add"]), section("Fixed", &["- new fix"])]);
    r.merge_earlier(
        vec![section("Security", &["- old sec"]), section("Fixed", &["- old fix 1", "- old fix 2"])],
        &types,
    );
    assert_eq!(
        r.sections,
        vec![
            section("Added", &["- new add"]),
            section("Fixed", &["- old fix 1", "- old fix 2", "- new fix"]),
            section("Security", &["- old sec"]),
        ]
    );
}

#[test]
fn merge_earlier_into_empty_release() {
    let types = default_types();
    let mut r = release("1.3.0", "2026-10-14", vec![]);
    r.merge_earlier(vec![section("Fixed", &["- f"]), section("Notes", &["- n"]), section("Added", &["- a"])], &types);
    assert_eq!(r.sections, vec![section("Added", &["- a"]), section("Fixed", &["- f"]), section("Notes", &["- n"])]);
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

const FIXTURE: &str = "\
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.2.0] - 2026-09-30

### Added

- Export to CSV. ([#40](https://github.com/example/greeter/issues/40))
- A hand-written entry
  that spans two lines.

### Fixed

* An old entry written with an asterisk.

## [1.1.1] - 2026-08-01 [YANKED]

### Security

- Bump a dependency.

## Version 1.0.5 (legacy heading, kept as-is)

Some prose about an ancient release.

```markdown
## [9.9.9] - 2020-01-01
```

## [1.1.0] - 2026-07-15

### Changed

- Hand-edited text   with odd   spacing.

## [1.0.0] - 2026-06-01

- Initial release.

[Unreleased]: https://github.com/example/greeter/compare/v1.2.0...HEAD
[1.2.0]: https://github.com/example/greeter/compare/v1.1.1...v1.2.0

[1.1.1]: https://github.com/example/greeter/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/example/greeter/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/example/greeter/releases/tag/v1.0.0
";

#[test]
fn parse_versions_in_file_order() {
    let c = Changelog::parse(FIXTURE).unwrap();
    assert_eq!(c.versions(), vec![v("1.2.0"), v("1.1.1"), v("1.1.0"), v("1.0.0")]);
    assert_eq!(c.latest_version(), Some(v("1.2.0")));
    assert_eq!(c.as_str(), FIXTURE);
}

#[test]
fn latest_version_is_highest_not_first() {
    let c = Changelog::parse("## [1.0.3] - 2026-01-03\n\n## [2.0.0] - 2026-01-02\n\n## [2.0.0-rc.1] - 2026-01-01\n")
        .unwrap();
    assert_eq!(c.latest_version(), Some(v("2.0.0")));
}

#[test]
fn heading_patterns() {
    let text = "\
## [1.0.0] - 2026-01-01
## [1.0.1]
## 1.0.2 - 2026-01-01
## [1.0.3] - 2026-1-1
## [1.0] - 2026-01-01
## [v1.0.4] - 2026-01-01
## [1.0.5] - 2026-01-01 [YANKED]
## [1.0.6] - 2026-01-01 [yanked]
### [1.0.7] - 2026-01-01
## [1.0.8-beta.1+build.5] - 2026-01-01
";
    let c = Changelog::parse(text).unwrap();
    assert_eq!(c.versions(), vec![v("1.0.0"), v("1.0.5"), v("1.0.8-beta.1+build.5")]);
}

#[test]
fn empty_and_releaseless_changelogs() {
    let c = Changelog::parse("").unwrap();
    assert_eq!(c.versions(), Vec::<Version>::new());
    assert_eq!(c.latest_version(), None);
    assert_eq!(Changelog::parse(NEW_CHANGELOG).unwrap().latest_version(), None);
}

#[test]
fn load_missing_file_is_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Changelog::load(&dir.path().join("CHANGELOG.md")).unwrap().is_none());
}

#[test]
fn load_reads_file_and_uses_path_in_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CHANGES.md");
    std::fs::write(&path, FIXTURE).unwrap();
    let mut c = Changelog::load(&path).unwrap().unwrap();
    assert_eq!(c.as_str(), FIXTURE);
    let err = c.insert_release(&release("1.1.0", "2026-10-14", vec![]), &config()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("CHANGES.md"), "{msg}");
    assert!(msg.contains("1.1.0 is already in the changelog"), "{msg}");
}

#[test]
fn load_rejects_non_utf8() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CHANGELOG.md");
    std::fs::write(&path, b"# Changelog\n\xff\xfe\n").unwrap();
    let err = Changelog::load(&path).unwrap_err();
    assert!(matches!(err, Error::Changelog { .. }));
    assert!(err.to_string().contains("not valid UTF-8"), "{err}");
}

// ---------------------------------------------------------------------------
// ensure_unreleased
// ---------------------------------------------------------------------------

#[test]
fn ensure_unreleased_present_case_insensitive() {
    let text = "# Changelog\n\n## [UNRELEASED]\n\n## [1.0.0] - 2026-01-01\n";
    let mut c = Changelog::parse(text).unwrap();
    assert!(!c.ensure_unreleased());
    assert_eq!(c.as_str(), text);
}

#[test]
fn ensure_unreleased_above_first_release() {
    let mut c = Changelog::parse("# Changelog\n\nIntro.\n\n## [1.0.0] - 2026-01-01\n\n- x\n").unwrap();
    assert!(c.ensure_unreleased());
    assert_eq!(c.as_str(), "# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n- x\n");
}

#[test]
fn ensure_unreleased_no_blank_before_release() {
    let mut c = Changelog::parse("# Changelog\nIntro.\n## [1.0.0] - 2026-01-01\n").unwrap();
    assert!(c.ensure_unreleased());
    assert_eq!(c.as_str(), "# Changelog\nIntro.\n\n## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n");
}

#[test]
fn ensure_unreleased_after_preamble_without_releases() {
    let mut c = Changelog::parse("# Changelog\n\nAll notable changes.\n\n\n").unwrap();
    assert!(c.ensure_unreleased());
    assert_eq!(c.as_str(), "# Changelog\n\nAll notable changes.\n\n## [Unreleased]\n");

    // No trailing newline at all.
    let mut c = Changelog::parse("# Changelog").unwrap();
    assert!(c.ensure_unreleased());
    assert_eq!(c.as_str(), "# Changelog\n\n## [Unreleased]\n");

    // Empty file.
    let mut c = Changelog::parse("").unwrap();
    assert!(c.ensure_unreleased());
    assert_eq!(c.as_str(), "## [Unreleased]\n");
}

#[test]
fn ensure_unreleased_before_legacy_headings_without_releases() {
    let text = "# Changelog\n\nIntro.\n\n## project 1.2.0 (2026-01-31)\n\n- Old.\n";
    let mut c = Changelog::parse(text).unwrap();
    assert!(c.ensure_unreleased());
    assert_eq!(c.as_str(), "# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## project 1.2.0 (2026-01-31)\n\n- Old.\n");
}

#[test]
fn ensure_unreleased_before_link_block_without_releases() {
    let mut c = Changelog::parse("# Changelog\n\n[foo]: https://example.com\n").unwrap();
    assert!(c.ensure_unreleased());
    assert_eq!(c.as_str(), "# Changelog\n\n## [Unreleased]\n\n[foo]: https://example.com\n");
}

// ---------------------------------------------------------------------------
// insert_release
// ---------------------------------------------------------------------------

fn tutorial_release() -> Release {
    release(
        "0.1.0",
        "2026-10-14",
        vec![
            section("Added", &["- Greet people by name."]),
            section("Fixed", &["- Don't crash on an empty name. ([#7](https://github.com/example/greeter/issues/7))"]),
        ],
    )
}

#[test]
fn insert_first_release_into_new_changelog() {
    let mut c = Changelog::parse(NEW_CHANGELOG).unwrap();
    c.insert_release(&tutorial_release(), &github()).unwrap();
    let expected = format!(
        "{NEW_CHANGELOG}
## [0.1.0] - 2026-10-14

### Added

- Greet people by name.

### Fixed

- Don't crash on an empty name. ([#7](https://github.com/example/greeter/issues/7))

[Unreleased]: https://github.com/example/greeter/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/example/greeter/releases/tag/v0.1.0
"
    );
    assert_eq!(c.as_str(), expected);

    // Second release from the tutorial.
    let r = release(
        "0.1.1",
        "2026-10-15",
        vec![section(
            "Fixed",
            &["- Trim whitespace around names. ([#9](https://github.com/example/greeter/issues/9))"],
        )],
    );
    c.insert_release(&r, &github()).unwrap();
    let expected = format!(
        "{NEW_CHANGELOG}
## [0.1.1] - 2026-10-15

### Fixed

- Trim whitespace around names. ([#9](https://github.com/example/greeter/issues/9))

## [0.1.0] - 2026-10-14

### Added

- Greet people by name.

### Fixed

- Don't crash on an empty name. ([#7](https://github.com/example/greeter/issues/7))

[Unreleased]: https://github.com/example/greeter/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/example/greeter/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/example/greeter/releases/tag/v0.1.0
"
    );
    assert_eq!(c.as_str(), expected);
    assert_eq!(c.latest_version(), Some(v("0.1.1")));
}

#[test]
fn insert_into_realistic_changelog() {
    let mut c = Changelog::parse(FIXTURE).unwrap();
    let r = release(
        "1.3.0",
        "2026-10-14",
        vec![section("Added", &["- Log in with OAuth."]), section("Fixed", &["- Empty config crash."])],
    );
    c.insert_release(&r, &github()).unwrap();
    let expected = FIXTURE
        .replace(
            "## [Unreleased]\n\n",
            "## [Unreleased]\n\n## [1.3.0] - 2026-10-14\n\n### Added\n\n- Log in with OAuth.\n\n\
             ### Fixed\n\n- Empty config crash.\n\n",
        )
        .replace(
            "[Unreleased]: https://github.com/example/greeter/compare/v1.2.0...HEAD\n",
            "[Unreleased]: https://github.com/example/greeter/compare/v1.3.0...HEAD\n\
             [1.3.0]: https://github.com/example/greeter/compare/v1.2.0...v1.3.0\n",
        );
    assert_eq!(c.as_str(), expected);
}

#[test]
fn readme_gitlab_example() {
    let text = "# Changelog\n\n## [Unreleased]\n\n## [1.2.0] - 2026-09-30\n\n### Added\n\n- Old.\n";
    let mut c = Changelog::parse(text).unwrap();
    let r = release(
        "1.3.0",
        "2026-10-14",
        vec![
            section("Added", &["- Log in with OAuth. ([!45](https://gitlab.com/acme/app/-/merge_requests/45))"]),
            section(
                "Fixed",
                &["- Don't crash when the config file is empty. ([#123](https://gitlab.com/acme/app/-/issues/123))"],
            ),
        ],
    );
    c.insert_release(&r, &gitlab()).unwrap();
    assert_eq!(
        c.as_str(),
        "# Changelog

## [Unreleased]

## [1.3.0] - 2026-10-14

### Added

- Log in with OAuth. ([!45](https://gitlab.com/acme/app/-/merge_requests/45))

### Fixed

- Don't crash when the config file is empty. ([#123](https://gitlab.com/acme/app/-/issues/123))

## [1.2.0] - 2026-09-30

### Added

- Old.

[Unreleased]: https://gitlab.com/acme/app/-/compare/v1.3.0...HEAD
[1.3.0]: https://gitlab.com/acme/app/-/compare/v1.2.0...v1.3.0
"
    );
}

#[test]
fn insert_keeps_unreleased_content_above_new_section() {
    let text = "# Changelog\n\n## [Unreleased]\nHand-written note.\n\n\n## [1.0.0] - 2026-01-01\n\n- x\n";
    let mut c = Changelog::parse(text).unwrap();
    c.insert_release(&release("1.1.0", "2026-02-02", vec![section("Fixed", &["- y"])]), &config()).unwrap();
    assert_eq!(
        c.as_str(),
        "# Changelog\n\n## [Unreleased]\nHand-written note.\n\n## [1.1.0] - 2026-02-02\n\n### Fixed\n\n- y\n\n\
         ## [1.0.0] - 2026-01-01\n\n- x\n"
    );
}

#[test]
fn insert_without_repo_adds_no_links() {
    let mut c = Changelog::parse("# Changelog\n\n## [Unreleased]\n").unwrap();
    c.insert_release(&release("1.0.0", "2026-01-01", vec![section("Added", &["- a"])]), &config()).unwrap();
    assert_eq!(c.as_str(), "# Changelog\n\n## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n### Added\n\n- a\n");
}

#[test]
fn insert_without_repo_keeps_existing_links_untouched() {
    let text = "## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n- a\n\n[1.0.0]: https://x/1.0.0\n";
    let mut c = Changelog::parse(text).unwrap();
    c.insert_release(&release("1.1.0", "2026-02-01", vec![section("Added", &["- b"])]), &config()).unwrap();
    assert_eq!(
        c.as_str(),
        "## [Unreleased]\n\n## [1.1.0] - 2026-02-01\n\n### Added\n\n- b\n\n## [1.0.0] - 2026-01-01\n\n- a\n\n\
         [1.0.0]: https://x/1.0.0\n"
    );
}

#[test]
fn insert_adds_unreleased_heading_when_missing() {
    let mut c = Changelog::parse("# Changelog\n\n## [1.0.0] - 2026-01-01\n\n- a\n").unwrap();
    c.insert_release(&release("1.1.0", "2026-02-01", vec![section("Added", &["- b"])]), &config()).unwrap();
    assert_eq!(
        c.as_str(),
        "# Changelog\n\n## [Unreleased]\n\n## [1.1.0] - 2026-02-01\n\n### Added\n\n- b\n\n## [1.0.0] - 2026-01-01\n\n- a\n"
    );
}

#[test]
fn insert_into_file_without_trailing_newline_or_with_many() {
    for text in ["# C\n\n## [Unreleased]", "# C\n\n## [Unreleased]\n\n\n\n"] {
        let mut c = Changelog::parse(text).unwrap();
        c.insert_release(&release("1.0.0", "2026-01-01", vec![section("Added", &["- a"])]), &config()).unwrap();
        assert_eq!(c.as_str(), "# C\n\n## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n### Added\n\n- a\n");
    }
}

#[test]
fn insert_existing_version_is_error_and_leaves_text() {
    let mut c = Changelog::parse(FIXTURE).unwrap();
    let err = c.insert_release(&release("1.2.0", "2026-10-14", vec![]), &github()).unwrap_err();
    assert!(matches!(err, Error::Changelog { .. }));
    assert!(err.to_string().starts_with("CHANGELOG.md: version 1.2.0 is already in the changelog"), "{err}");
    assert_eq!(c.as_str(), FIXTURE);
}

#[test]
fn unreleased_link_added_as_first_link_when_missing() {
    let text = "## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n- a\n\n[1.0.0]: https://github.com/example/greeter/releases/tag/v1.0.0\n[other]: https://example.com\n";
    let mut c = Changelog::parse(text).unwrap();
    c.insert_release(&release("1.0.1", "2026-01-02", vec![section("Fixed", &["- b"])]), &github()).unwrap();
    assert!(c.as_str().ends_with(
        "- a\n\n\
         [Unreleased]: https://github.com/example/greeter/compare/v1.0.1...HEAD\n\
         [1.0.1]: https://github.com/example/greeter/compare/v1.0.0...v1.0.1\n\
         [1.0.0]: https://github.com/example/greeter/releases/tag/v1.0.0\n\
         [other]: https://example.com\n"
    ));
}

#[test]
fn unreleased_link_updated_in_place_keeping_label() {
    let text = "## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n- a\n\n[other]: https://example.com\n[unreleased]: https://old/compare\n[1.0.0]: https://old/1.0.0";
    let mut c = Changelog::parse(text).unwrap();
    c.insert_release(&release("1.0.1", "2026-01-02", vec![section("Fixed", &["- b"])]), &github()).unwrap();
    assert!(c.as_str().ends_with(
        "- a\n\n\
         [other]: https://example.com\n\
         [unreleased]: https://github.com/example/greeter/compare/v1.0.1...HEAD\n\
         [1.0.1]: https://github.com/example/greeter/compare/v1.0.0...v1.0.1\n\
         [1.0.0]: https://old/1.0.0\n"
    ));
}

#[test]
fn stale_link_for_new_version_is_replaced() {
    let text = "## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n- a\n\n[Unreleased]: u\n[1.0.0]: x\n[1.0.1]: stale\n";
    let mut c = Changelog::parse(text).unwrap();
    c.insert_release(&release("1.0.1", "2026-01-02", vec![section("Fixed", &["- b"])]), &github()).unwrap();
    assert!(!c.as_str().contains("stale"));
    assert_eq!(c.as_str().matches("[1.0.1]:").count(), 1);
}

#[test]
fn links_use_tag_format_with_package_name() {
    let mut cfg = gitlab();
    cfg.version.tag_format = "{name}@v{version}".into();
    let mut c = Changelog::parse("## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n- a\n").unwrap();
    c.insert_release(&release("1.1.0", "2026-02-01", vec![section("Added", &["- b"])]), &cfg).unwrap();
    assert!(c.as_str().ends_with(
        "- a\n\n\
         [Unreleased]: https://gitlab.com/acme/app/-/compare/app@v1.1.0...HEAD\n\
         [1.1.0]: https://gitlab.com/acme/app/-/compare/app@v1.0.0...app@v1.1.0\n"
    ));
}

#[test]
fn custom_compare_url_without_release_url_skips_first_version_link() {
    let mut cfg = config();
    cfg.changelog.compare_url = Some("https://git.example/c/{from}..{to}".into());
    let mut c = Changelog::parse(NEW_CHANGELOG).unwrap();
    c.insert_release(&release("1.0.0", "2026-01-01", vec![section("Added", &["- a"])]), &cfg).unwrap();
    assert!(c.as_str().ends_with("- a\n\n[Unreleased]: https://git.example/c/v1.0.0..HEAD\n"));
}

#[test]
fn backport_release_compares_with_highest_lower_version() {
    let text = "## [Unreleased]\n\n## [2.0.0] - 2026-03-01\n\n- c\n\n## [1.1.0] - 2026-02-01\n\n- b\n\n## [1.0.0] - 2026-01-01\n\n- a\n\n\
                [Unreleased]: https://github.com/example/greeter/compare/v2.0.0...HEAD\n";
    let mut c = Changelog::parse(text).unwrap();
    c.insert_release(&release("1.1.1", "2026-03-05", vec![section("Fixed", &["- d"])]), &github()).unwrap();
    assert!(c.as_str().ends_with(
        "[Unreleased]: https://github.com/example/greeter/compare/v2.0.0...HEAD\n\
         [1.1.1]: https://github.com/example/greeter/compare/v1.1.0...v1.1.1\n"
    ));
    assert_eq!(c.latest_version(), Some(v("2.0.0")));
}

#[test]
fn crlf_is_preserved() {
    let text = NEW_CHANGELOG.replace('\n', "\r\n");
    let mut c = Changelog::parse(&text).unwrap();
    let r = release("1.0.0", "2026-01-01", vec![section("Added", &["- a\n\n  more"]), section("Fixed", &["- b"])]);
    c.insert_release(&r, &github()).unwrap();
    let out = c.as_str();
    assert!(!out.replace("\r\n", "").contains('\n'), "bare LF in {out:?}");
    assert!(out.starts_with(&text));
    assert!(out.ends_with(
        "## [1.0.0] - 2026-01-01\r\n\r\n### Added\r\n\r\n- a\r\n\r\n  more\r\n\r\n### Fixed\r\n\r\n- b\r\n\r\n\
         [Unreleased]: https://github.com/example/greeter/compare/v1.0.0...HEAD\r\n\
         [1.0.0]: https://github.com/example/greeter/releases/tag/v1.0.0\r\n"
    ));
}

#[test]
fn crlf_ensure_unreleased() {
    let mut c = Changelog::parse("# Changelog\r\n\r\n## [1.0.0] - 2026-01-01\r\n").unwrap();
    assert!(c.ensure_unreleased());
    assert_eq!(c.as_str(), "# Changelog\r\n\r\n## [Unreleased]\r\n\r\n## [1.0.0] - 2026-01-01\r\n");
}

#[test]
fn headings_inside_code_fences_are_ignored() {
    let text = "# Changelog\n\n```\n## [Unreleased]\n## [3.0.0] - 2026-01-01\n```\n\n## [1.0.0] - 2026-01-01\n";
    let mut c = Changelog::parse(text).unwrap();
    assert_eq!(c.versions(), vec![v("1.0.0")]);
    assert!(c.ensure_unreleased());
    assert!(c.as_str().contains("```\n\n## [Unreleased]\n\n## [1.0.0]"));
}

#[test]
fn bom_is_kept_and_first_line_still_parsed() {
    let text = "\u{feff}## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n";
    let mut c = Changelog::parse(text).unwrap();
    assert_eq!(c.versions(), vec![v("1.0.0")]);
    assert!(!c.ensure_unreleased());
}

// ---------------------------------------------------------------------------
// Pre-releases
// ---------------------------------------------------------------------------

const PRE_FIXTURE: &str = "\
# Changelog

## [Unreleased]

## [1.3.0-rc.2] - 2026-10-10

### Added

- Feature from rc2.

### Fixed

- Fix from rc2,
  over two lines.

## [1.3.0-rc.1] - 2026-10-01

### Fixed

- Fix from rc1.

### Added

- Feature from rc1.

## [1.2.0] - 2026-09-30

### Added

- Old feature.

## [1.2.0-beta.1] - 2026-09-01

### Added

- Old beta feature.

[Unreleased]: https://github.com/example/greeter/compare/v1.3.0-rc.2...HEAD
[1.3.0-rc.2]: https://github.com/example/greeter/compare/v1.3.0-rc.1...v1.3.0-rc.2
[1.3.0-rc.1]: https://github.com/example/greeter/compare/v1.2.0...v1.3.0-rc.1
[1.2.0]: https://github.com/example/greeter/compare/v1.2.0-beta.1...v1.2.0
[1.2.0-beta.1]: https://github.com/example/greeter/releases/tag/v1.2.0-beta.1
";

#[test]
fn prerelease_sections_merged_oldest_first() {
    let c = Changelog::parse(PRE_FIXTURE).unwrap();
    assert_eq!(
        c.prerelease_sections(&v("1.3.0")),
        vec![
            section("Fixed", &["- Fix from rc1.", "- Fix from rc2,\n  over two lines."]),
            section("Added", &["- Feature from rc1.", "- Feature from rc2."]),
        ]
    );
    assert_eq!(c.prerelease_sections(&v("1.4.0")), vec![]);
    assert_eq!(c.prerelease_sections(&v("1.3.0-rc.3")), vec![]);
    assert_eq!(c.as_str(), PRE_FIXTURE);
}

#[test]
fn prerelease_sections_parse_bullets_and_continuations() {
    let text = "## [2.0.0-alpha.1] - 2026-01-01\n\nIntro prose is ignored.\n\n### Added\n\n* Star bullet.\n+ Plus bullet.\n- Dash\n\n  paragraph two.\n\n  ```\n  code\n  ```\n#### Sub heading stays\n\n### Empty\n\n### Fixed\n- f\n";
    let c = Changelog::parse(text).unwrap();
    assert_eq!(
        c.prerelease_sections(&v("2.0.0")),
        vec![
            section(
                "Added",
                &[
                    "- Star bullet.",
                    "- Plus bullet.",
                    "- Dash\n\n  paragraph two.\n\n  ```\n  code\n  ```\n#### Sub heading stays"
                ]
            ),
            section("Fixed", &["- f"]),
        ]
    );
}

#[test]
fn fold_prereleases_on_final_release() {
    let cfg = github();
    let mut c = Changelog::parse(PRE_FIXTURE).unwrap();
    let mut r = release("1.3.0", "2026-10-14", vec![section("Fixed", &["- New fix."])]);
    r.merge_earlier(c.prerelease_sections(&v("1.3.0")), &cfg.types);
    c.insert_release(&r, &cfg).unwrap();
    assert_eq!(
        c.as_str(),
        "\
# Changelog

## [Unreleased]

## [1.3.0] - 2026-10-14

### Added

- Feature from rc1.
- Feature from rc2.

### Fixed

- Fix from rc1.
- Fix from rc2,
  over two lines.
- New fix.

## [1.2.0] - 2026-09-30

### Added

- Old feature.

## [1.2.0-beta.1] - 2026-09-01

### Added

- Old beta feature.

[Unreleased]: https://github.com/example/greeter/compare/v1.3.0...HEAD
[1.3.0]: https://github.com/example/greeter/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/example/greeter/compare/v1.2.0-beta.1...v1.2.0
[1.2.0-beta.1]: https://github.com/example/greeter/releases/tag/v1.2.0-beta.1
"
    );
}

#[test]
fn no_folding_when_disabled() {
    let mut cfg = github();
    cfg.version.fold_prereleases = false;
    let mut c = Changelog::parse(PRE_FIXTURE).unwrap();
    c.insert_release(&release("1.3.0", "2026-10-14", vec![section("Fixed", &["- New fix."])]), &cfg).unwrap();
    let out = c.as_str();
    assert!(out.contains("## [1.3.0-rc.2]"));
    assert!(out.contains("## [1.3.0-rc.1]"));
    assert!(out.contains(
        "[Unreleased]: https://github.com/example/greeter/compare/v1.3.0...HEAD\n\
         [1.3.0]: https://github.com/example/greeter/compare/v1.3.0-rc.2...v1.3.0\n\
         [1.3.0-rc.2]:"
    ));
}

#[test]
fn prerelease_insert_does_not_fold() {
    let cfg = github();
    let mut c = Changelog::parse(PRE_FIXTURE).unwrap();
    c.insert_release(&release("1.3.0-rc.3", "2026-10-12", vec![section("Fixed", &["- x"])]), &cfg).unwrap();
    let out = c.as_str();
    assert!(out.contains("## [Unreleased]\n\n## [1.3.0-rc.3] - 2026-10-12\n\n### Fixed\n\n- x\n\n## [1.3.0-rc.2]"));
    assert!(out.contains(
        "[Unreleased]: https://github.com/example/greeter/compare/v1.3.0-rc.3...HEAD\n\
         [1.3.0-rc.3]: https://github.com/example/greeter/compare/v1.3.0-rc.2...v1.3.0-rc.3\n\
         [1.3.0-rc.2]:"
    ));
}

#[test]
fn fold_last_section_before_link_block_keeps_spacing() {
    let text =
        "## [Unreleased]\n\n## [1.0.0-rc.1] - 2026-01-01\n\n### Added\n\n- a\n\n[Unreleased]: x\n[1.0.0-rc.1]: y\n";
    let cfg = github();
    let mut c = Changelog::parse(text).unwrap();
    let mut r = release("1.0.0", "2026-01-05", vec![]);
    r.merge_earlier(c.prerelease_sections(&v("1.0.0")), &cfg.types);
    c.insert_release(&r, &cfg).unwrap();
    assert_eq!(
        c.as_str(),
        "## [Unreleased]\n\n## [1.0.0] - 2026-01-05\n\n### Added\n\n- a\n\n\
         [Unreleased]: https://github.com/example/greeter/compare/v1.0.0...HEAD\n\
         [1.0.0]: https://github.com/example/greeter/releases/tag/v1.0.0\n"
    );
}

#[test]
fn mixed_line_endings_in_untouched_text_are_kept() {
    let text = "# Changelog\r\n\r\nIntro\nmore\r\n\r\n## [Unreleased]\r\n\r\n## [1.0.0] - 2026-01-01\n\n- old\r\n";
    let mut c = Changelog::parse(text).unwrap();
    c.insert_release(&release("1.1.0", "2026-02-01", vec![section("Added", &["- b"])]), &config()).unwrap();
    assert_eq!(
        c.as_str(),
        "# Changelog\r\n\r\nIntro\nmore\r\n\r\n## [Unreleased]\r\n\r\n## [1.1.0] - 2026-02-01\r\n\r\n### Added\r\n\r\n- b\r\n\r\n\
         ## [1.0.0] - 2026-01-01\n\n- old\r\n"
    );
}

#[test]
fn tag_name_format() {
    assert_eq!(tag_name("v{version}", "api", &v("1.2.3")), "v1.2.3");
    assert_eq!(tag_name("{name}@v{version}", "api", &v("1.2.3-rc.1")), "api@v1.2.3-rc.1");
}

// ---------------------------------------------------------------------------
// Review fixes
// ---------------------------------------------------------------------------

#[test]
fn bom_stays_at_start_when_unreleased_is_inserted_on_first_line() {
    let mut c = Changelog::parse("\u{feff}## [1.0.0] - 2026-01-01\n\n- a\n").unwrap();
    assert!(c.ensure_unreleased());
    assert_eq!(c.as_str(), "\u{feff}## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n- a\n");

    let mut c = Changelog::parse("\u{feff}## [1.0.0] - 2026-01-01\r\n").unwrap();
    c.insert_release(&release("1.1.0", "2026-02-01", vec![section("Added", &["- b"])]), &github()).unwrap();
    let out = c.as_str();
    assert!(out.starts_with("\u{feff}## [Unreleased]\r\n\r\n## [1.1.0] - 2026-02-01\r\n"), "{out:?}");
    assert_eq!(out.matches('\u{feff}').count(), 1);
}

#[test]
fn entry_refs_after_trailing_code_fence_get_their_own_paragraph() {
    let f = with_refs(frag("1", "fixed", "Fix it:\n\n```toml\nkey = 1\n```"), vec![issue(1)]);
    assert_eq!(
        render_entry(&f, &config()),
        "- Fix it:\n\n  ```toml\n  key = 1\n  ```\n\n  ([#1](https://github.com/example/greeter/issues/1))"
    );
    let f = with_refs(frag("1", "fixed", "Fix it:\n\n~~~\nx\n~~~"), vec![plain("PROJ-1")]);
    assert_eq!(render_entry(&f, &config()), "- Fix it:\n\n  ~~~\n  x\n  ~~~\n\n  (PROJ-1)");
}

#[test]
fn entry_refs_skip_trailing_link_definitions() {
    let f = with_refs(frag("1", "fixed", "See [the docs].\n\n[the docs]: https://docs.example"), vec![issue(1)]);
    assert_eq!(
        render_entry(&f, &config()),
        "- See [the docs]. ([#1](https://github.com/example/greeter/issues/1))\n\n  [the docs]: https://docs.example"
    );
}

#[test]
fn entry_start_refs_before_leading_code_fence() {
    let mut cfg = config();
    cfg.changelog.ref_position = RefPosition::Start;
    let f = breaking(with_refs(frag("1", "changed", "```\nx\n```\nText."), vec![plain("PROJ-1")]));
    assert_eq!(render_entry(&f, &cfg), "- **Breaking:** PROJ-1\n\n  ```\n  x\n  ```\n  Text.");
}

#[test]
fn indented_link_definition_in_entry_is_not_the_link_block() {
    let mut c = Changelog::parse("# Changelog\n\n## [Unreleased]\n").unwrap();
    let r = build_release(
        v("1.0.0"),
        "2026-01-01".into(),
        &[frag("docs", "fixed", "See [docs].\n\n[docs]: https://d")],
        &default_types(),
        &config(),
    );
    c.insert_release(&r, &config()).unwrap();
    c.insert_release(&release("1.1.0", "2026-01-02", vec![section("Fixed", &["- Other."])]), &github()).unwrap();
    assert_eq!(
        c.as_str(),
        "# Changelog\n\n## [Unreleased]\n\n## [1.1.0] - 2026-01-02\n\n### Fixed\n\n- Other.\n\n\
         ## [1.0.0] - 2026-01-01\n\n### Fixed\n\n- See [docs].\n\n  [docs]: https://d\n\n\
         [Unreleased]: https://github.com/example/greeter/compare/v1.1.0...HEAD\n\
         [1.1.0]: https://github.com/example/greeter/compare/v1.0.0...v1.1.0\n"
    );
}

#[test]
fn prerelease_sections_code_fences_are_not_structure() {
    let text = "## [1.0.0-rc.1] - 2026-01-01\n\n### Added\n\n- Example:\n\n  ```md\n### Not a heading\n- not an entry\n  ```\n- Next.\n";
    let c = Changelog::parse(text).unwrap();
    assert_eq!(
        c.prerelease_sections(&v("1.0.0")),
        vec![section("Added", &["- Example:\n\n  ```md\n### Not a heading\n- not an entry\n  ```", "- Next."])]
    );
}

#[test]
fn insert_version_differing_only_in_build_metadata_is_error() {
    let mut c = Changelog::parse(FIXTURE).unwrap();
    let err = c.insert_release(&release("1.2.0+build.5", "2026-10-14", vec![]), &github()).unwrap_err();
    assert!(err.to_string().contains("version 1.2.0+build.5 is already in the changelog (as `## [1.2.0]`)"), "{err}");
    assert_eq!(c.as_str(), FIXTURE);
}
