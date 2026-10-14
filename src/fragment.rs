//! Fragments: parsing filenames, references, frontmatter and bodies; ordering;
//! creating new fragment files.
//!
//! Spec: `docs/reference/fragments.md`, `docs/how-to/link-issues-and-merge-requests.md`.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::config::{Config, RefKind};
use crate::error::{Error, Result};

/// A parsed, validated fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fragment {
    /// Absolute path of the fragment file.
    pub path: PathBuf,
    /// The `<name>` part of the filename.
    pub name: String,
    /// Canonical change type name (aliases already resolved).
    #[serde(rename = "type")]
    pub type_name: String,
    pub counter: Option<u32>,
    /// References: the filename reference first (if the name is one), then
    /// frontmatter `refs` in the order written.
    pub refs: Vec<Ref>,
    pub breaking: bool,
    /// Normalised Markdown body: trimmed, leading bullet marker removed. Not yet
    /// indented for rendering.
    pub body: String,
}

/// A resolved reference to an issue, merge request or ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ref {
    /// The reference as written, e.g. `mr-45`.
    pub raw: String,
    /// Name of the matching reference kind; `None` for plain-text references
    /// (frontmatter refs that matched no kind).
    pub kind: Option<String>,
    /// Link text, e.g. `!45`. For plain-text references, the raw text.
    pub display: String,
    pub url: Option<String>,
}

/// The parts of a fragment filename, before validation against a config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileName {
    pub name: String,
    /// The type as written (may be an alias, any case).
    pub type_name: String,
    pub counter: Option<u32>,
}

/// The form every fragment filename must have, used in error messages.
const EXPECTED_FORM: &str = "expected `<name>.<type>[.<counter>].md` (e.g. `123.fixed.md` or `oauth-login.added.2.md`), \
     where <name> uses only letters, digits, `-` and `_`";

/// Split `<name>.<type>[.<counter>].md`. Returns `None` for files that aren't
/// fragments (not `.md`, or not enough parts), which callers silently ignore.
///
/// Precisely: `None` is returned for any name that doesn't fit the grammar,
/// including `.md` files with a bad shape. [`load_all`] ignores files that
/// don't end in `.md` and reports every other `None` as an error, so a
/// malformed fragment never goes unnoticed.
///
/// - `name` is one or more of `[A-Za-z0-9_-]`;
/// - `type` is one or more of `[A-Za-z0-9_-]` (validated against the config later);
/// - `counter`, if present, is a positive decimal integer.
pub fn parse_file_name(file_name: &str) -> Option<FileName> {
    let stem = file_name.strip_suffix(".md")?;
    let parts: Vec<&str> = stem.split('.').collect();
    let (name, type_name, counter) = match parts.as_slice() {
        [name, type_name] => (*name, *type_name, None),
        [name, type_name, counter] => (*name, *type_name, Some(parse_counter(counter)?)),
        _ => return None,
    };
    if !is_valid_name(name) || !is_valid_name(type_name) {
        return None;
    }
    Some(FileName { name: name.to_string(), type_name: type_name.to_string(), counter })
}

fn parse_counter(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u32>().ok().filter(|&n| n > 0)
}

/// Whether `s` is a valid fragment `<name>`: one or more of `[A-Za-z0-9_-]`.
pub fn is_valid_name(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Resolve `raw` against `kinds` (first match wins).
///
/// A kind's pattern must match the *whole* of `raw` (as the configuration
/// reference requires), so an unanchored pattern such as `(?<id>\d+)` doesn't
/// turn a slug like `fix-12-crash` into a reference.
///
/// `{id}` in the kind's display and URL templates is replaced by the `id`
/// capture group, or by the whole match if the pattern has no such group.
pub fn resolve_ref(raw: &str, kinds: &[RefKind]) -> Option<Ref> {
    kinds.iter().find_map(|kind| {
        let caps = kind.matcher.captures(raw)?;
        let whole = caps.get(0)?;
        if whole.start() != 0 || whole.end() != raw.len() {
            return None;
        }
        let id = caps.name("id").or_else(|| caps.get(0)).map_or("", |m| m.as_str());
        Some(Ref {
            raw: raw.to_string(),
            kind: Some(kind.name.clone()),
            display: kind.display.replace("{id}", id),
            url: kind.url.as_ref().map(|u| u.replace("{id}", id)),
        })
    })
}

/// Whether an unmatched fragment name looks like a reference (all digits, or
/// `<letters>-<digits>`), which makes it an error rather than a slug.
pub fn looks_like_ref(name: &str) -> bool {
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if all_digits(name) {
        return true;
    }
    match name.split_once('-') {
        Some((letters, digits)) => {
            !letters.is_empty() && letters.bytes().all(|b| b.is_ascii_alphabetic()) && all_digits(digits)
        }
        None => false,
    }
}

/// Parse and validate one fragment file against `config`.
pub fn load(path: &Path, config: &Config) -> Result<Fragment> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::fragment(path, format!("invalid fragment filename; {EXPECTED_FORM}")))?;
    let parsed = parse_file_name(file_name)
        .ok_or_else(|| Error::fragment(path, format!("invalid fragment filename `{file_name}`; {EXPECTED_FORM}")))?;

    let change_type = config.find_type(&parsed.type_name).ok_or_else(|| {
        let known: Vec<&str> = config.types.iter().map(|t| t.name.as_str()).collect();
        Error::fragment(
            path,
            format!(
                "unknown change type `{}`; expected one of: {} (or one of their aliases)",
                parsed.type_name,
                known.join(", ")
            ),
        )
    })?;

    let mut refs = Vec::new();
    match resolve_ref(&parsed.name, &config.refs) {
        Some(r) => refs.push(r),
        None if looks_like_ref(&parsed.name) => {
            return Err(Error::fragment(path, unmatched_ref_message(&parsed.name, &config.refs)));
        }
        None => {}
    }

    let text = crate::error::read_to_string(path)?;
    let (meta, body) = split_frontmatter(&text).map_err(|m| Error::fragment(path, m))?;

    for raw in meta.refs {
        let raw = raw.into_string();
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(Error::fragment(path, "empty entry in frontmatter `refs`"));
        }
        refs.push(resolve_ref(raw, &config.refs).unwrap_or_else(|| Ref {
            raw: raw.to_string(),
            kind: None,
            display: raw.to_string(),
            url: None,
        }));
    }

    let body = normalize_body(body);
    if body.is_empty() {
        return Err(Error::fragment(path, "fragment body is empty; write a sentence describing the change"));
    }

    Ok(Fragment {
        path: path.to_path_buf(),
        name: parsed.name,
        type_name: change_type.name.clone(),
        counter: parsed.counter,
        refs,
        breaking: meta.breaking,
        body,
    })
}

fn unmatched_ref_message(name: &str, kinds: &[RefKind]) -> String {
    let configured = if kinds.is_empty() {
        "no reference kinds are configured".to_string()
    } else {
        let names: Vec<String> = kinds.iter().map(|k| format!("`{}` (/{}/)", k.name, k.matcher.as_str())).collect();
        format!("configured kinds: {}", names.join(", "))
    };
    format!(
        "`{name}` looks like a reference but matches no reference kind ({configured}); \
         fix the typo, add a `[refs.<kind>]` table to the config, or use a descriptive slug instead"
    )
}

/// Load every fragment in `config.fragments.directory` (not recursive). A
/// missing directory means no fragments. Returns fragments in filename order;
/// use [`sort`] for release order.
///
/// Files that don't end in `.md` (such as `.gitkeep`), `README.md` and
/// subdirectories are ignored. A `.md` file whose name doesn't fit the fragment grammar is an
/// error. When several files are invalid, the first in filename order is
/// reported.
pub fn load_all(config: &Config) -> Result<Vec<Fragment>> {
    let dir = &config.fragments.directory;
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(dir, e)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.ends_with(".md") || name.eq_ignore_ascii_case("README.md") {
            continue;
        }
        paths.push(path);
    }
    paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    paths.iter().map(|p| load(p, config)).collect()
}

/// Sort fragments into release order: by git add-time (`added_times`, keyed by
/// absolute path, seconds since the epoch), then filesystem creation time for
/// files missing from `added_times`, then filename. Ties break on filename.
///
/// The exact rule:
///
/// 1. Each fragment gets a timestamp: its entry in `added_times` if there is
///    one (looked up by path, then by canonicalised path); otherwise its
///    filesystem creation time ([`std::fs::Metadata::created`], with sub-second
///    precision); otherwise none.
/// 2. Fragments with a timestamp come first, in ascending timestamp order. Git
///    times and creation times are compared on the same scale, so uncommitted
///    files (created after the last commit) naturally sort after committed ones.
/// 3. Fragments without any timestamp come last.
/// 4. Equal timestamps, and fragments without a timestamp, are ordered by file
///    name, then by full path.
pub fn sort(fragments: &mut [Fragment], added_times: &HashMap<PathBuf, i64>) {
    let mut canonical: Option<HashMap<PathBuf, i64>> = None;
    let mut git_time = |path: &Path| -> Option<i64> {
        if let Some(t) = added_times.get(path) {
            return Some(*t);
        }
        let canonical = canonical.get_or_insert_with(|| {
            added_times.iter().filter_map(|(p, t)| Some((p.canonicalize().ok()?, *t))).collect()
        });
        canonical.get(&path.canonicalize().ok()?).copied()
    };

    let mut keyed: Vec<(Option<(i64, u32)>, Fragment)> = fragments
        .iter()
        .map(|f| {
            let time = git_time(&f.path).map(|t| (t, 0)).or_else(|| created_time(&f.path));
            (time, f.clone())
        })
        .collect();

    keyed.sort_by(|(ta, a), (tb, b)| {
        let by_time = match (ta, tb) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        by_time.then_with(|| a.path.file_name().cmp(&b.path.file_name())).then_with(|| a.path.cmp(&b.path))
    });

    for (slot, (_, f)) in fragments.iter_mut().zip(keyed) {
        *slot = f;
    }
}

/// Filesystem creation time as (seconds, nanoseconds) since the epoch.
fn created_time(path: &Path) -> Option<(i64, u32)> {
    let created = std::fs::metadata(path).ok()?.created().ok()?;
    match created.duration_since(UNIX_EPOCH) {
        Ok(d) => Some((i64::try_from(d.as_secs()).ok()?, d.subsec_nanos())),
        Err(e) => {
            // Before 1970: represent as a negative (seconds, nanos) pair.
            let d = e.duration();
            let secs = -i64::try_from(d.as_secs()).ok()?;
            match d.subsec_nanos() {
                0 => Some((secs, 0)),
                n => Some((secs - 1, 1_000_000_000 - n)),
            }
        }
    }
}

/// Slug made from the first six words of a message: lowercase, runs of
/// non-alphanumeric characters become `-`, trimmed of `-`.
///
/// A "word" is a run of letters and digits, so `Don't` counts as two words.
/// Common accented Latin letters are transliterated (`é` → `e`, `ß` → `ss`);
/// other non-ASCII characters act as separators. The result is always a valid
/// fragment name: `change` if nothing is left, and prefixed with `change-` if
/// it would otherwise look like a reference (`123`, `abc-12`).
pub fn slug_from_message(message: &str) -> String {
    let mut ascii = String::with_capacity(message.len());
    for c in message.chars() {
        if c.is_ascii_alphanumeric() {
            ascii.push(c.to_ascii_lowercase());
        } else if let Some(t) = transliterate(c) {
            ascii.push_str(t);
        } else {
            ascii.push(' ');
        }
    }
    let words: Vec<&str> =
        ascii.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).take(6).collect();
    let slug = words.join("-");
    if slug.is_empty() {
        "change".to_string()
    } else if looks_like_ref(&slug) {
        format!("change-{slug}")
    } else {
        slug
    }
}

fn transliterate(c: char) -> Option<&'static str> {
    let lower: Vec<char> = c.to_lowercase().collect();
    let c = if lower.len() == 1 { lower[0] } else { c };
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' => "t",
        'þ' => "th",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

/// Path for a new fragment named `name` of canonical type `type_name`, adding
/// a counter (`.2`, `.3`, …) if the file already exists.
pub fn new_fragment_path(config: &Config, name: &str, type_name: &str) -> PathBuf {
    let dir = &config.fragments.directory;
    let first = dir.join(format!("{name}.{type_name}.md"));
    if !first.exists() {
        return first;
    }
    (2u32..)
        .map(|n| dir.join(format!("{name}.{type_name}.{n}.md")))
        .find(|p| !p.exists())
        .expect("some counter is free")
}

/// File contents for a new fragment: YAML frontmatter only if there are extra
/// refs or `breaking`, then the body with a trailing newline.
///
/// If the body itself starts with a `---` or `+++` line, an empty frontmatter
/// block is written first so the body isn't mistaken for frontmatter.
pub fn render_file(body: &str, extra_refs: &[String], breaking: bool) -> String {
    let body = body.trim_end();
    let mut out = String::new();
    let body_looks_like_frontmatter = matches!(body.lines().next().map(str::trim_end), Some("---" | "+++"));
    if breaking || !extra_refs.is_empty() || body_looks_like_frontmatter {
        out.push_str("---\n");
        if breaking {
            out.push_str("breaking: true\n");
        }
        if !extra_refs.is_empty() {
            let quoted: Vec<String> =
                extra_refs.iter().map(|r| serde_json::to_string(r).expect("strings serialise")).collect();
            out.push_str(&format!("refs: [{}]\n", quoted.join(", ")));
        }
        out.push_str("---\n");
    }
    out.push_str(body);
    out.push('\n');
    out
}

/// Frontmatter keys. Unknown keys are rejected so typos get caught.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a table of keys (`breaking`, `refs`)")]
struct FrontMatter {
    #[serde(default)]
    breaking: bool,
    #[serde(default)]
    refs: Vec<RefValue>,
}

/// A frontmatter reference. Strings are expected, but bare integers
/// (`refs: [123]`) are accepted too since YAML and TOML users write them
/// naturally.
#[derive(Debug)]
enum RefValue {
    Str(String),
    Int(u64),
}

impl<'de> Deserialize<'de> for RefValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = RefValue;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a reference string such as \"mr-130\"")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<RefValue, E> {
                Ok(RefValue::Str(v.to_string()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<RefValue, E> {
                Ok(RefValue::Int(v))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<RefValue, E> {
                u64::try_from(v)
                    .map(RefValue::Int)
                    .map_err(|_| E::invalid_value(serde::de::Unexpected::Signed(v), &self))
            }
        }
        d.deserialize_any(Visitor)
    }
}

impl RefValue {
    fn into_string(self) -> String {
        match self {
            RefValue::Str(s) => s,
            RefValue::Int(n) => n.to_string(),
        }
    }
}

/// Split file contents into frontmatter and body. Handles a UTF-8 BOM and CRLF
/// line endings. Returns the error message on malformed frontmatter.
fn split_frontmatter(text: &str) -> std::result::Result<(FrontMatter, String), String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n");
    let mut lines = text.split('\n');
    let delim = match lines.next().map(str::trim_end) {
        Some(d @ ("---" | "+++")) => d,
        _ => return Ok((FrontMatter::default(), text)),
    };
    let mut meta_lines = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim_end() == delim {
            closed = true;
            break;
        }
        meta_lines.push(line);
    }
    let format = if delim == "---" { "YAML" } else { "TOML" };
    if !closed {
        return Err(format!(
            "unterminated {format} frontmatter: the opening `{delim}` line has no closing `{delim}` line"
        ));
    }
    let body: Vec<&str> = lines.collect();
    let body = body.join("\n");
    let meta_text = meta_lines.join("\n");

    let meta = if meta_text.trim().is_empty() {
        FrontMatter::default()
    } else if delim == "---" {
        // A leading newline stands in for the opening `---`, so line numbers in
        // YAML errors are line numbers in the file.
        serde_yaml::from_str::<FrontMatter>(&format!("\n{meta_text}")).map_err(|e| {
            format!("invalid YAML frontmatter: {e}; allowed keys are `breaking` (bool) and `refs` (list of strings)")
        })?
    } else {
        toml::from_str::<FrontMatter>(&meta_text).map_err(|e| {
            format!(
                "invalid TOML frontmatter: {}; allowed keys are `breaking` (bool) and `refs` (list of strings)",
                e.message().trim_end()
            )
        })?
    };
    Ok((meta, body))
}

/// Normalise a fragment body: trim trailing whitespace on every line, drop
/// leading and trailing blank lines, and remove leading whitespace and one
/// leading `- ` or `* ` from the first line. A first line that is only a
/// bullet marker (`-` or `*`, its trailing space already trimmed) is dropped,
/// along with any blank lines after it.
fn normalize_body(body: String) -> String {
    let mut lines: Vec<&str> = body.split('\n').map(str::trim_end).collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let Some(start) = lines.iter().position(|l| !l.is_empty()) else {
        return String::new();
    };
    let mut lines = &lines[start..];

    let first = lines[0].trim_start();
    let first = match first {
        "-" | "*" => {
            let rest = &lines[1..];
            let Some(next) = rest.iter().position(|l| !l.is_empty()) else {
                return String::new();
            };
            lines = &rest[next..];
            lines[0].trim_start()
        }
        _ => first.strip_prefix("- ").or_else(|| first.strip_prefix("* ")).unwrap_or(first).trim_start(),
    };
    let mut out = first.to_string();
    for line in &lines[1..] {
        out.push('\n');
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests;
