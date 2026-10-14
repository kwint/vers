//! The changelog file: parsing, rendering a release section, inserting it,
//! maintaining link references and folding pre-releases.
//!
//! Spec: `docs/reference/changelog.md`, `docs/reference/fragments.md#rendering`.
//!
//! The changelog is only parsed at the level of `## ` headings and the trailing
//! link reference block. Internally the text is handled as a list of lines that
//! keep their own line endings, so everything vers doesn't touch is preserved
//! byte-for-byte (including mixed LF/CRLF files).

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use semver::Version;

use crate::config::{ChangeType, Config};
use crate::error::{Error, Result};
use crate::fragment::{Fragment, Ref};

/// The header written to a new changelog. Ends with the empty Unreleased section.
pub const NEW_CHANGELOG: &str = "\
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
";

/// A release to be written: a version, a date and its sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// ISO 8601 date, `YYYY-MM-DD`.
    pub date: String,
    /// Sections in changelog type order; never empty.
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Type heading, e.g. `Added`.
    pub heading: String,
    /// Rendered entries: Markdown list items *including* the leading `- `,
    /// continuation lines indented by two spaces, no trailing newline.
    pub entries: Vec<String>,
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Normalise a fragment body: LF line endings, whitespace-only lines become
/// empty, and leading/trailing blank lines are removed.
fn normalise_body(body: &str) -> String {
    let lines: Vec<&str> = body
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .map(|l| if l.trim().is_empty() { "" } else { l })
        .collect();
    let first = lines.iter().position(|l| !l.is_empty());
    let last = lines.iter().rposition(|l| !l.is_empty());
    match (first, last) {
        (Some(first), Some(last)) => lines[first..=last].join("\n"),
        _ => String::new(),
    }
}

/// The key used to decide whether two fragments have the same body. Trailing
/// whitespace on lines is not significant.
fn body_key(body: &str) -> String {
    normalise_body(body).lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
}

fn render_ref(r: &Ref) -> String {
    match &r.url {
        Some(url) => format!("[{}]({})", r.display, url),
        None => r.display.clone(),
    }
}

/// A code fence line (opening or closing), allowing for list indentation.
fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn render_refs(refs: &[Ref]) -> String {
    refs.iter().map(render_ref).collect::<Vec<_>>().join(", ")
}

fn render_item(body: &str, refs: &[Ref], breaking: bool, config: &Config) -> String {
    let body = normalise_body(body);
    let prefix = if breaking { config.changelog.breaking_prefix.as_str() } else { "" };
    let refs = render_refs(refs);
    let mut lines: Vec<String> = body.split('\n').map(str::to_string).collect();
    if !refs.is_empty() {
        match config.changelog.ref_position {
            crate::config::RefPosition::End => {
                // The last paragraph line: trailing link reference definitions
                // (`[docs]: https://…`) are skipped, because text appended to
                // them would become part of the URL's title.
                let target = lines.iter().rposition(|l| !l.trim().is_empty() && link_label(l).is_none());
                match target {
                    // Text after a closing code fence would turn it into an
                    // opening fence and swallow the rest of the changelog,
                    // so the refs get their own paragraph after the code.
                    Some(i) if is_fence(&lines[i]) => {
                        lines.splice(i + 1..i + 1, [String::new(), format!("({refs})")]);
                    }
                    Some(i) => lines[i].push_str(&format!(" ({refs})")),
                    None => {
                        let last = lines.last_mut().expect("split yields at least one line");
                        if last.is_empty() {
                            *last = format!("({refs})");
                        } else {
                            lines.extend([String::new(), format!("({refs})")]);
                        }
                    }
                }
            }
            crate::config::RefPosition::Start => {
                if is_fence(&lines[0]) {
                    // `- [#1](…) ```rust` would no longer open a code block.
                    lines.splice(0..0, [refs, String::new()]);
                } else {
                    let first = &mut lines[0];
                    *first = if first.is_empty() { refs } else { format!("{refs} {first}") };
                }
            }
        }
    }
    let mut out = format!("- {prefix}{}", lines[0]);
    for line in &lines[1..] {
        out.push('\n');
        if !line.is_empty() {
            out.push_str("  ");
            out.push_str(line);
        }
    }
    out
}

/// Render one fragment as a list item: breaking prefix, body, references (at
/// the end or start, per `config.changelog.ref_position`).
pub fn render_entry(fragment: &Fragment, config: &Config) -> String {
    render_item(&fragment.body, &fragment.refs, fragment.breaking, config)
}

/// Build a release from `fragments` (already in release order): group by type
/// in `types` order, skip hidden types, merge fragments of the same type with
/// identical bodies (combining refs), drop empty sections.
pub fn build_release(
    version: Version,
    date: String,
    fragments: &[Fragment],
    types: &[ChangeType],
    config: &Config,
) -> Release {
    struct Merged<'a> {
        key: String,
        body: &'a str,
        refs: Vec<Ref>,
        breaking: bool,
    }

    let mut sections = Vec::new();
    for ty in types.iter().filter(|t| !t.hidden) {
        let mut merged: Vec<Merged> = Vec::new();
        for fragment in fragments.iter().filter(|f| f.type_name.eq_ignore_ascii_case(&ty.name)) {
            let key = body_key(&fragment.body);
            let item = match merged.iter_mut().position(|m| m.key == key) {
                Some(i) => &mut merged[i],
                None => {
                    merged.push(Merged { key, body: &fragment.body, refs: Vec::new(), breaking: false });
                    merged.last_mut().expect("just pushed")
                }
            };
            item.breaking |= fragment.breaking;
            for r in &fragment.refs {
                if !item.refs.iter().any(|x| x.display == r.display && x.url == r.url) {
                    item.refs.push(r.clone());
                }
            }
        }
        if !merged.is_empty() {
            sections.push(Section {
                heading: ty.heading.clone(),
                entries: merged.iter().map(|m| render_item(m.body, &m.refs, m.breaking, config)).collect(),
            });
        }
    }
    Release { version, date, sections }
}

/// Position of `heading` in `types` order; unknown headings sort last.
fn type_rank(heading: &str, types: &[ChangeType]) -> usize {
    types.iter().position(|t| t.heading.eq_ignore_ascii_case(heading)).unwrap_or(usize::MAX)
}

impl Release {
    /// Merge `earlier` sections (e.g. from folded pre-releases) into this
    /// release: their entries come before this release's entries under the
    /// same heading, and section order follows `types`.
    pub fn merge_earlier(&mut self, earlier: Vec<Section>, types: &[ChangeType]) {
        let mut merged: Vec<Section> = Vec::new();
        for section in earlier.into_iter().chain(std::mem::take(&mut self.sections)) {
            match merged.iter_mut().find(|s| s.heading.eq_ignore_ascii_case(&section.heading)) {
                Some(existing) => existing.entries.extend(section.entries),
                None => merged.push(section),
            }
        }
        merged.retain(|s| !s.entries.is_empty());
        // Stable: unknown headings keep their relative order at the end.
        merged.sort_by_key(|s| type_rank(&s.heading, types));
        self.sections = merged;
    }
}

/// Render a release section as Markdown, starting with its `## [x.y.z] - date`
/// heading. This is what `vers draft` prints.
pub fn render_release(release: &Release) -> String {
    let mut out = format!("## [{}] - {}\n", release.version, release.date);
    for section in &release.sections {
        out.push_str(&format!("\n### {}\n\n", section.heading));
        for entry in &section.entries {
            out.push_str(entry);
            out.push('\n');
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

static RELEASE_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^## \[(?<version>[^\]\s]+)\] - (?<date>\d{4}-\d{2}-\d{2})(?: \[YANKED\])?[ \t]*$")
        .expect("valid regex")
});
static UNRELEASED_HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^##[ \t]+\[unreleased\][ \t]*$").expect("valid regex"));
// Only unindented definitions count: indented ones belong to a list item (e.g.
// a reference definition at the end of a multi-line entry), not to the
// changelog's link block.
static LINK_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[(?<label>[^\]]+)\]:[ \t]*\S").expect("valid regex"));

const BOM: &str = "\u{feff}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Eol {
    Lf,
    CrLf,
    None,
}

impl Eol {
    fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
            Eol::None => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    content: String,
    eol: Eol,
}

impl Line {
    fn is_blank(&self) -> bool {
        self.content.trim().is_empty()
    }
}

fn split_lines(text: &str) -> Vec<Line> {
    text.split_inclusive('\n')
        .map(|raw| {
            if let Some(c) = raw.strip_suffix("\r\n") {
                Line { content: c.to_string(), eol: Eol::CrLf }
            } else if let Some(c) = raw.strip_suffix('\n') {
                Line { content: c.to_string(), eol: Eol::Lf }
            } else {
                Line { content: raw.to_string(), eol: Eol::None }
            }
        })
        .collect()
}

fn join_lines(lines: &[Line]) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(&line.content);
        out.push_str(line.eol.as_str());
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HeadingKind {
    Unreleased,
    Release(Version),
    /// Any other level 1 or 2 heading.
    Other,
}

#[derive(Debug, Clone)]
struct Heading {
    line: usize,
    kind: HeadingKind,
}

/// The structure vers cares about: level 1/2 headings and the link block.
#[derive(Debug)]
struct Scan {
    headings: Vec<Heading>,
    /// Index of the first line of the trailing link reference block.
    link_start: Option<usize>,
}

impl Scan {
    fn new(lines: &[Line]) -> Scan {
        let link_start = find_link_block(lines);
        let limit = link_start.unwrap_or(lines.len());
        let mut headings = Vec::new();
        let mut fence: Option<char> = None;
        for (i, line) in lines[..limit].iter().enumerate() {
            let content = line.content.as_str();
            let trimmed = content.trim_start_matches(' ');
            if content.len() - trimmed.len() <= 3 {
                if let Some(c) = fence {
                    if trimmed.starts_with(&c.to_string().repeat(3)) {
                        fence = None;
                    }
                    continue;
                } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                    fence = trimmed.chars().next();
                    continue;
                }
            } else if fence.is_some() {
                continue;
            }
            let kind = if UNRELEASED_HEADING.is_match(content) {
                HeadingKind::Unreleased
            } else if let Some(c) = RELEASE_HEADING.captures(content) {
                match Version::parse(&c["version"]) {
                    Ok(v) => HeadingKind::Release(v),
                    Err(_) => HeadingKind::Other,
                }
            } else if is_heading(content, 1) || is_heading(content, 2) {
                HeadingKind::Other
            } else {
                continue;
            };
            headings.push(Heading { line: i, kind });
        }
        Scan { headings, link_start }
    }

    fn unreleased(&self) -> Option<usize> {
        self.headings.iter().find(|h| h.kind == HeadingKind::Unreleased).map(|h| h.line)
    }

    fn releases(&self) -> impl Iterator<Item = (usize, &Version)> {
        self.headings.iter().filter_map(|h| match &h.kind {
            HeadingKind::Release(v) => Some((h.line, v)),
            _ => None,
        })
    }

    /// The end (exclusive) of the section whose heading is at `line`: the next
    /// level 1/2 heading, the link block or the end of the file.
    fn section_end(&self, line: usize, len: usize) -> usize {
        self.headings
            .iter()
            .map(|h| h.line)
            .find(|&l| l > line)
            .or(self.link_start.filter(|&l| l > line))
            .unwrap_or(len)
    }

    /// Where body content ends: the link block or the end of the file.
    fn content_end(&self, len: usize) -> usize {
        self.link_start.unwrap_or(len)
    }
}

/// `# Title` / `## Title` (exact level, ATX style).
fn is_heading(content: &str, level: usize) -> bool {
    let hashes = "#".repeat(level);
    match content.strip_prefix(&hashes) {
        Some(rest) => rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t'),
        None => false,
    }
}

fn link_label(content: &str) -> Option<&str> {
    LINK_REF.captures(content).map(|c| c.name("label").expect("label group").as_str())
}

/// The trailing run of link reference lines (blank lines allowed between and
/// after them). Returns the index of the first link line.
fn find_link_block(lines: &[Line]) -> Option<usize> {
    let mut start = None;
    for (i, line) in lines.iter().enumerate().rev() {
        if line.is_blank() {
            continue;
        }
        if link_label(&line.content).is_some() {
            start = Some(i);
        } else {
            break;
        }
    }
    start
}

/// Parse the entries under `### ` headings in `lines`. Content before the
/// first `### ` heading is not part of any section and is ignored.
fn parse_sections(lines: &[Line]) -> Vec<Section> {
    fn finish(entry: &mut Option<Vec<String>>, section: Option<&mut Section>) {
        if let Some(mut e) = entry.take() {
            while e.last().is_some_and(|l| l.trim().is_empty()) {
                e.pop();
            }
            if let Some(section) = section
                && !e.is_empty()
            {
                section.entries.push(e.join("\n"));
            }
        }
    }

    let mut sections: Vec<Section> = Vec::new();
    let mut entry: Option<Vec<String>> = None;
    let mut in_fence = false;
    for line in lines {
        let content = line.content.as_str();
        // Inside a code block, `- ` and `### ` lines are code, not structure.
        if in_fence || is_fence(content) {
            if is_fence(content) {
                in_fence = !in_fence;
            }
            if !sections.is_empty() {
                entry.get_or_insert_with(Vec::new).push(content.to_string());
            }
            continue;
        }
        if is_heading(content, 3) {
            finish(&mut entry, sections.last_mut());
            sections.push(Section { heading: content[3..].trim().to_string(), entries: Vec::new() });
            continue;
        }
        if sections.is_empty() {
            continue;
        }
        let bullet = ["- ", "* ", "+ "].iter().find_map(|m| content.strip_prefix(m));
        if let Some(rest) = bullet {
            finish(&mut entry, sections.last_mut());
            entry = Some(vec![format!("- {rest}")]);
        } else if let Some(e) = entry.as_mut() {
            e.push(if line.is_blank() { String::new() } else { content.to_string() });
        } else if !line.is_blank() {
            entry = Some(vec![content.to_string()]);
        }
    }
    finish(&mut entry, sections.last_mut());
    sections
}

/// A parsed changelog. Keeps the original text so that everything vers doesn't
/// own is preserved byte-for-byte.
#[derive(Debug, Clone)]
pub struct Changelog {
    text: String,
    /// Used in error messages.
    path: PathBuf,
}

impl Changelog {
    pub fn parse(text: &str) -> Result<Changelog> {
        Ok(Changelog { text: text.to_string(), path: PathBuf::from("CHANGELOG.md") })
    }

    /// Load a changelog; `Ok(None)` if the file doesn't exist.
    pub fn load(path: &Path) -> Result<Option<Changelog>> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::io(path, e)),
        };
        let text = String::from_utf8(bytes).map_err(|e| {
            Error::changelog(
                path,
                format!(
                    "the changelog is not valid UTF-8 (invalid byte at offset {}); convert it to UTF-8",
                    e.utf8_error().valid_up_to()
                ),
            )
        })?;
        let mut changelog = Changelog::parse(&text)?;
        changelog.path = path.to_path_buf();
        Ok(Some(changelog))
    }

    /// The lines of the text, without a UTF-8 byte order mark (which
    /// [`Changelog::set_lines`] puts back at the very start).
    fn lines(&self) -> Vec<Line> {
        split_lines(self.text.strip_prefix(BOM).unwrap_or(&self.text))
    }

    fn set_lines(&mut self, lines: &[Line]) {
        let bom = if self.text.starts_with(BOM) { BOM } else { "" };
        self.text = format!("{bom}{}", join_lines(lines));
    }

    /// The line ending used for new lines: the file's first line ending.
    fn eol(&self) -> Eol {
        self.lines().iter().map(|l| l.eol).find(|&e| e != Eol::None).unwrap_or(Eol::Lf)
    }

    fn new_lines(&self, text: &str) -> Vec<Line> {
        let eol = self.eol();
        text.strip_suffix('\n').unwrap_or(text).split('\n').map(|l| Line { content: l.to_string(), eol }).collect()
    }

    /// The highest SemVer version among release headings.
    pub fn latest_version(&self) -> Option<Version> {
        self.versions().into_iter().max()
    }

    /// Every version with a release heading, in file order.
    pub fn versions(&self) -> Vec<Version> {
        let lines = self.lines();
        Scan::new(&lines).releases().map(|(_, v)| v.clone()).collect()
    }

    /// Insert `## [Unreleased]` if missing (above the first release heading,
    /// or at the end of the preamble). Returns whether it was added.
    pub fn ensure_unreleased(&mut self) -> bool {
        let mut lines = self.lines();
        let scan = Scan::new(&lines);
        if scan.unreleased().is_some() {
            return false;
        }
        let at = scan
            .releases()
            .map(|(line, _)| line)
            .next()
            .or_else(|| {
                // No releases: the preamble ends at the first `## ` heading
                // (the `# ` title belongs to the preamble).
                scan.headings.iter().map(|h| h.line).find(|&l| is_heading(&lines[l].content, 2))
            })
            .unwrap_or_else(|| scan.content_end(lines.len()));
        let block = self.new_lines("## [Unreleased]");
        insert_block(&mut lines, at, block, self.eol());
        finish_end(&mut lines, self.eol());
        self.set_lines(&lines);
        true
    }

    /// Entries of the pre-release sections of `version` (e.g. every
    /// `1.3.0-*` for `1.3.0`), merged per heading, without modifying the
    /// changelog. Used to fold pre-releases into a final release.
    pub fn prerelease_sections(&self, version: &Version) -> Vec<Section> {
        let lines = self.lines();
        let scan = Scan::new(&lines);
        let mut pre: Vec<(usize, &Version)> = scan.releases().filter(|(_, v)| is_prerelease_of(v, version)).collect();
        // Older pre-releases first.
        pre.sort_by(|a, b| a.1.cmp(b.1));
        let mut merged: Vec<Section> = Vec::new();
        for (line, _) in pre {
            let end = scan.section_end(line, lines.len());
            for section in parse_sections(&lines[line + 1..end]) {
                match merged.iter_mut().find(|s| s.heading.eq_ignore_ascii_case(&section.heading)) {
                    Some(existing) => existing.entries.extend(section.entries),
                    None => merged.push(section),
                }
            }
        }
        merged.retain(|s| !s.entries.is_empty());
        merged
    }

    /// Insert `release` below `## [Unreleased]` (adding that heading if
    /// missing), update link references using `config` (tag names from
    /// `tag_format`, URLs from `compare_url`/`release_url`), and, if
    /// `config.version.fold_prereleases` and `release` is a final version,
    /// remove its pre-release sections and their link references. The caller
    /// is responsible for having merged [`Changelog::prerelease_sections`]
    /// into `release`.
    pub fn insert_release(&mut self, release: &Release, config: &Config) -> Result<()> {
        let version = &release.version;
        // Build metadata doesn't count: `1.0.0+build.2` is the same release as `1.0.0`.
        if let Some(existing) = self.versions().into_iter().find(|v| v.cmp_precedence(version).is_eq()) {
            return Err(Error::changelog(
                &self.path,
                format!(
                    "version {version} is already in the changelog (as `## [{existing}]`); \
                     choose a newer version with --version or --bump"
                ),
            ));
        }
        let eol = self.eol();

        // 1. Fold pre-releases.
        if config.version.fold_prereleases && version.pre.is_empty() {
            self.remove_prereleases(version);
        }

        // 2. Unreleased heading.
        self.ensure_unreleased();

        // 3. The new section, after the Unreleased heading and its content.
        let mut lines = self.lines();
        let scan = Scan::new(&lines);
        let unreleased = scan.unreleased().expect("ensure_unreleased added it");
        let at = scan.section_end(unreleased, lines.len());
        let block = self.new_lines(&render_release(release));
        insert_block(&mut lines, at, block, eol);
        finish_end(&mut lines, eol);
        self.set_lines(&lines);

        // 4. Link references.
        if config.changelog.compare_url.is_some() {
            self.update_links(version, config);
        }
        Ok(())
    }

    /// Remove every `<version>-*` section and its link reference.
    fn remove_prereleases(&mut self, version: &Version) {
        let mut lines = self.lines();
        let scan = Scan::new(&lines);
        let mut ranges: Vec<(usize, usize)> = scan
            .releases()
            .filter(|(_, v)| is_prerelease_of(v, version))
            .map(|(line, _)| (line, scan.section_end(line, lines.len())))
            .collect();
        if let Some(link_start) = scan.link_start {
            for (i, line) in lines.iter().enumerate().skip(link_start) {
                if let Some(label) = link_label(&line.content)
                    && let Ok(v) = Version::parse(label)
                    && is_prerelease_of(&v, version)
                {
                    ranges.push((i, i + 1));
                }
            }
        }
        if ranges.is_empty() {
            return;
        }
        ranges.sort();
        for (start, end) in ranges.into_iter().rev() {
            lines.drain(start..end);
        }
        finish_end(&mut lines, self.eol());
        self.set_lines(&lines);
    }

    fn update_links(&mut self, version: &Version, config: &Config) {
        let Some(compare_url) = config.changelog.compare_url.as_deref() else { return };
        let package = config.package.as_ref().map(|p| p.name.as_str()).unwrap_or("");
        let tag = |v: &Version| tag_name(&config.version.tag_format, package, v);
        let compare = |from: &Version, to: &str| compare_url.replace("{from}", &tag(from)).replace("{to}", to);

        let versions = self.versions();
        let newest = versions.iter().max().unwrap_or(version);
        let unreleased_url = compare(newest, "HEAD");
        let previous = versions.iter().filter(|v| *v < version).max();
        let version_url = match previous {
            Some(prev) => Some(compare(prev, &tag(version))),
            None => config.changelog.release_url.as_deref().map(|u| u.replace("{tag}", &tag(version))),
        };

        let eol = self.eol();
        let mut lines = self.lines();
        let label = version.to_string();
        let new_line = |content: String| Line { content, eol };

        match Scan::new(&lines).link_start {
            Some(start) => {
                // Drop a stale link for this version, if any.
                if let Some(i) = (start..lines.len()).find(|&i| link_label(&lines[i].content) == Some(label.as_str())) {
                    lines.remove(i);
                }
                let unreleased = (start..lines.len())
                    .find(|&i| link_label(&lines[i].content).is_some_and(|l| l.eq_ignore_ascii_case("unreleased")));
                let after = match unreleased {
                    Some(i) => {
                        let existing = link_label(&lines[i].content).expect("is a link").to_string();
                        lines[i].content = format!("[{existing}]: {unreleased_url}");
                        i + 1
                    }
                    None => {
                        let at = start.min(lines.len());
                        lines.insert(at, new_line(format!("[Unreleased]: {unreleased_url}")));
                        at + 1
                    }
                };
                if let Some(url) = version_url {
                    lines.insert(after, new_line(format!("[{label}]: {url}")));
                }
                // The last line may not have had a line ending.
                for line in lines.iter_mut() {
                    if line.eol == Eol::None {
                        line.eol = eol;
                    }
                }
            }
            None => {
                let mut block = vec![new_line(format!("[Unreleased]: {unreleased_url}"))];
                if let Some(url) = version_url {
                    block.push(new_line(format!("[{label}]: {url}")));
                }
                let at = lines.len();
                insert_block(&mut lines, at, block, eol);
            }
        }
        finish_end(&mut lines, eol);
        self.set_lines(&lines);
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

fn is_prerelease_of(candidate: &Version, version: &Version) -> bool {
    version.pre.is_empty()
        && !candidate.pre.is_empty()
        && candidate.major == version.major
        && candidate.minor == version.minor
        && candidate.patch == version.patch
}

/// Insert `block` before line `at`, with exactly one blank line between it and
/// the surrounding content (none at the start or end of the file).
fn insert_block(lines: &mut Vec<Line>, at: usize, block: Vec<Line>, eol: Eol) {
    let blank = || Line { content: String::new(), eol };
    let mut before: Vec<Line> = lines[..at].to_vec();
    let mut after: Vec<Line> = lines[at..].to_vec();
    while before.last().is_some_and(Line::is_blank) {
        before.pop();
    }
    let leading = after.iter().take_while(|l| l.is_blank()).count();
    after.drain(..leading);
    if let Some(last) = before.last_mut() {
        if last.eol == Eol::None {
            last.eol = eol;
        }
        before.push(blank());
    }
    before.extend(block);
    if !after.is_empty() {
        before.push(blank());
    }
    before.extend(after);
    *lines = before;
}

/// The file ends with exactly one line ending (and no trailing blank lines).
fn finish_end(lines: &mut Vec<Line>, eol: Eol) {
    while lines.last().is_some_and(Line::is_blank) {
        lines.pop();
    }
    if let Some(last) = lines.last_mut()
        && last.eol == Eol::None
    {
        last.eol = eol;
    }
}

/// Render a tag name from `tag_format`.
pub fn tag_name(tag_format: &str, package_name: &str, version: &Version) -> String {
    tag_format.replace("{name}", package_name).replace("{version}", &version.to_string())
}

#[cfg(test)]
mod tests;
