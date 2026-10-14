//! Finding the exact byte span of the version string inside a version file.
//!
//! Reading and updating both go through [`locate`], so vers always writes
//! exactly the bytes it read. The span covers only the version text: never the
//! quotes, the key or any surrounding whitespace, so replacing it keeps every
//! other byte of the file (comments, key order, quoting style, CRLF line
//! endings) intact.

use std::ops::Range;
use std::path::Path;

use regex::{Regex, RegexBuilder};

use crate::config::{VersionFileSpec, VersionLocator};
use crate::error::{Error, Result};

/// Structured formats a `key` can be looked up in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Toml,
    Json,
    Yaml,
}

impl Format {
    fn detect(path: &Path) -> Option<Format> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "toml" => Some(Format::Toml),
            "json" => Some(Format::Json),
            "yaml" | "yml" => Some(Format::Yaml),
            _ => None,
        }
    }
}

/// The byte range of the version string in `content`.
pub(super) fn locate(spec: &VersionFileSpec, content: &str) -> Result<Range<usize>> {
    let path = spec.path.as_path();
    // A UTF-8 byte order mark is not part of the document for any parser.
    let bom = if content.starts_with('\u{feff}') { '\u{feff}'.len_utf8() } else { 0 };
    let body = &content[bom..];
    let span = match &spec.locator {
        VersionLocator::Whole => whole(path, body)?,
        VersionLocator::Pattern(re) => pattern(path, body, re)?,
        VersionLocator::Key(key) => {
            let segments = split_key(path, key)?;
            match Format::detect(path) {
                Some(Format::Toml) => toml(path, body, key, &segments)?,
                Some(Format::Json) => json(path, body, key, &segments)?,
                Some(Format::Yaml) => yaml(path, body, key, &segments)?,
                None => {
                    return Err(Error::version_file(
                        path,
                        format!(
                            "cannot look up key `{key}`: unknown file format (expected a .toml, .json, .yaml or \
                             .yml file); hint: use `pattern` for other files"
                        ),
                    ));
                }
            }
        }
    };
    Ok(span.start + bom..span.end + bom)
}

fn split_key<'k>(path: &Path, key: &'k str) -> Result<Vec<&'k str>> {
    let segments: Vec<&str> = key.split('.').collect();
    if segments.iter().any(|s| s.is_empty()) {
        return Err(Error::version_file(
            path,
            format!("invalid key `{key}`: expected a dotted path such as `package.version`"),
        ));
    }
    Ok(segments)
}

fn not_found(path: &Path, key: &str) -> Error {
    Error::version_file(path, format!("key `{key}` not found"))
}

// ---------------------------------------------------------------------------
// Whole file

fn whole(path: &Path, body: &str) -> Result<Range<usize>> {
    let trimmed_start = body.trim_start();
    let start = body.len() - trimmed_start.len();
    let end = start + trimmed_start.trim_end().len();
    if start == end {
        return Err(Error::version_file(path, "file is empty; expected it to contain only a version"));
    }
    Ok(start..end)
}

// ---------------------------------------------------------------------------
// Regex pattern

fn pattern(path: &Path, body: &str, re: &Regex) -> Result<Range<usize>> {
    if !re.capture_names().any(|n| n == Some("version")) {
        return Err(Error::version_file(
            path,
            format!("pattern `{}` has no named group `version`; hint: write it as `(?<version>...)`", re.as_str()),
        ));
    }
    // `^` and `$` match at line boundaries (also before `\r\n`), so patterns
    // like `^release = "(?<version>[^"]+)"` work anywhere in the file.
    let re = RegexBuilder::new(re.as_str()).multi_line(true).crlf(true).build().unwrap_or_else(|_| re.clone());
    let spans: Vec<Range<usize>> =
        re.captures_iter(body).filter_map(|c| c.name("version")).map(|m| m.range()).collect();
    match spans.len() {
        1 => Ok(spans.into_iter().next().unwrap()),
        0 => Err(Error::version_file(
            path,
            format!("pattern `{}` did not match; it must match exactly once", re.as_str()),
        )),
        n => Err(Error::version_file(
            path,
            format!(
                "pattern `{}` matched {n} times; it must match exactly once (hint: make the pattern more specific)",
                re.as_str()
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// TOML

fn toml(path: &Path, body: &str, key: &str, segments: &[&str]) -> Result<Range<usize>> {
    let doc = toml_edit::Document::parse(body)
        .map_err(|e| Error::version_file(path, format!("invalid TOML: {}", e.to_string().trim_end())))?;
    let mut item = doc.as_item();
    for seg in segments {
        item = item.get(*seg).ok_or_else(|| not_found(path, key))?;
    }
    let value = match item.as_value() {
        Some(toml_edit::Value::String(s)) => s,
        _ if item.get("workspace").is_some() => {
            return Err(Error::version_file(
                path,
                format!(
                    "key `{key}` is inherited from the workspace (`workspace = true`), not a version string; \
                     hint: point vers at the workspace Cargo.toml with key `workspace.package.version`"
                ),
            ));
        }
        _ => return Err(Error::version_file(path, format!("key `{key}` is not a string"))),
    };
    let span = value.span().ok_or_else(|| not_found(path, key))?;
    let raw = &body[span.clone()];
    let (open, close) = if raw.starts_with("\"\"\"") || raw.starts_with("'''") {
        // Multi-line strings drop a newline directly after the opening quotes.
        let after = &raw[3..];
        let skip = if after.starts_with("\r\n") {
            2
        } else if after.starts_with('\n') {
            1
        } else {
            0
        };
        (3 + skip, 3)
    } else {
        (1, 1)
    };
    let inner = span.start + open..span.end - close;
    if inner.start > inner.end || body[inner.clone()] != *value.value() {
        return Err(Error::version_file(
            path,
            format!("key `{key}` uses escape sequences or line continuations; write it as a plain string"),
        ));
    }
    Ok(inner)
}

// ---------------------------------------------------------------------------
// JSON

fn json(path: &Path, body: &str, key: &str, segments: &[&str]) -> Result<Range<usize>> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| Error::version_file(path, format!("invalid JSON: {e}")))?;
    let mut node = &value;
    for seg in segments {
        node = node.as_object().and_then(|o| o.get(*seg)).ok_or_else(|| not_found(path, key))?;
    }
    let expected = node.as_str().ok_or_else(|| Error::version_file(path, format!("key `{key}` is not a string")))?;

    let mut scanner = JsonScanner { b: body.as_bytes(), i: 0 };
    let span = scanner
        .value(Some(segments))
        .map_err(|e| Error::version_file(path, format!("invalid JSON: {e}")))?
        .ok_or_else(|| not_found(path, key))?;
    if body[span.clone()] != *expected {
        return Err(Error::version_file(
            path,
            format!("key `{key}` uses escape sequences; write it as a plain string"),
        ));
    }
    Ok(span)
}

/// A minimal JSON scanner that walks the document and records the span of the
/// string at a key path. Validation is left to `serde_json`; this only needs
/// to understand the structure.
struct JsonScanner<'a> {
    b: &'a [u8],
    i: usize,
}

impl JsonScanner<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> std::result::Result<(), String> {
        self.ws();
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected `{}` at byte {}", c as char, self.i))
        }
    }

    /// Scan a string starting at `"`. Returns the span of its contents.
    fn string(&mut self) -> std::result::Result<Range<usize>, String> {
        self.expect(b'"')?;
        let start = self.i;
        while let Some(c) = self.peek() {
            match c {
                b'"' => {
                    let end = self.i;
                    self.i += 1;
                    return Ok(start..end);
                }
                b'\\' => self.i += 2,
                _ => self.i += 1,
            }
        }
        Err("unterminated string".to_string())
    }

    /// Scan one value. `target` is the remaining key path to find inside this
    /// value (`Some(&[])` means this value is the target), or `None` when the
    /// target can't be inside it.
    fn value(&mut self, target: Option<&[&str]>) -> std::result::Result<Option<Range<usize>>, String> {
        self.ws();
        let wanted_here = matches!(target, Some([]));
        match self.peek() {
            Some(b'"') => {
                let span = self.string()?;
                Ok(if wanted_here { Some(span) } else { None })
            }
            Some(b'{') => {
                self.i += 1;
                let mut found = None;
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(None);
                }
                loop {
                    self.ws();
                    let key_span = self.string()?;
                    let raw_key = std::str::from_utf8(&self.b[key_span.clone()]).map_err(|e| e.to_string())?;
                    let key: std::borrow::Cow<'_, str> = if raw_key.contains('\\') {
                        let quoted = &self.b[key_span.start - 1..key_span.end + 1];
                        serde_json::from_slice::<String>(quoted).map_err(|e| e.to_string())?.into()
                    } else {
                        raw_key.into()
                    };
                    self.expect(b':')?;
                    let child = match target {
                        Some([first, rest @ ..]) if *first == key => Some(rest),
                        _ => None,
                    };
                    // Like serde_json, the last duplicate key wins.
                    if let Some(span) = self.value(child)? {
                        found = Some(span);
                    }
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(found);
                        }
                        _ => return Err(format!("expected `,` or `}}` at byte {}", self.i)),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(None);
                }
                loop {
                    self.value(None)?;
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(None);
                        }
                        _ => return Err(format!("expected `,` or `]` at byte {}", self.i)),
                    }
                }
            }
            Some(_) => {
                // Number, true, false or null.
                let start = self.i;
                while let Some(c) = self.peek() {
                    if matches!(c, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r') {
                        break;
                    }
                    self.i += 1;
                }
                if self.i == start {
                    return Err(format!("unexpected character at byte {}", self.i));
                }
                Ok(None)
            }
            None => Err("unexpected end of input".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// YAML

fn yaml(path: &Path, body: &str, key: &str, segments: &[&str]) -> Result<Range<usize>> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(body).map_err(|e| Error::version_file(path, format!("invalid YAML: {e}")))?;
    let mut node = &value;
    for seg in segments {
        node = node.as_mapping().and_then(|m| m.get(*seg)).ok_or_else(|| not_found(path, key))?;
    }
    let expected = match node {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Number(n) => n.to_string(),
        _ => return Err(Error::version_file(path, format!("key `{key}` is not a string"))),
    };

    let unsupported = || {
        Error::version_file(
            path,
            format!(
                "cannot locate key `{key}` for editing; vers supports block-style mappings with the version as \
                 a plain or quoted scalar on the same line as its key"
            ),
        )
    };
    let span = yaml_scan(body, segments).ok_or_else(unsupported)?;
    if body[span.clone()] != *expected {
        return Err(unsupported());
    }
    Ok(span)
}

struct YamlLine<'a> {
    /// Byte offset of the line in the document.
    start: usize,
    /// The line without its line ending.
    text: &'a str,
}

fn yaml_lines(body: &str) -> Vec<YamlLine<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for raw in body.split_inclusive('\n') {
        let text = raw.strip_suffix('\n').unwrap_or(raw);
        let text = text.strip_suffix('\r').unwrap_or(text);
        lines.push(YamlLine { start, text });
        start += raw.len();
    }
    lines
}

/// Whether a line carries no mapping content: blank, a comment, or a
/// document marker.
fn yaml_skippable(text: &str) -> bool {
    let t = text.trim();
    t.is_empty() || t.starts_with('#') || t == "---" || t == "..." || t.starts_with("--- ") || t.starts_with("%")
}

/// If `rest` (a line without its indentation) starts with `key:`, return the
/// byte offset just after the colon.
fn yaml_key(rest: &str, key: &str) -> Option<usize> {
    let after = [format!("{key}:"), format!("\"{key}\":"), format!("'{key}':")]
        .into_iter()
        .find_map(|form| rest.strip_prefix(form.as_str()).map(|_| form.len()))?;
    match rest[after..].chars().next() {
        None | Some(' ') | Some('\t') => Some(after),
        _ => None,
    }
}

fn yaml_scan(body: &str, segments: &[&str]) -> Option<Range<usize>> {
    let lines = yaml_lines(body);
    let mut parent_indent: Option<usize> = None;
    let mut from = 0;
    for (n, seg) in segments.iter().enumerate() {
        let mut child_indent = None;
        let mut found = None;
        for (idx, line) in lines.iter().enumerate().skip(from) {
            if yaml_skippable(line.text) {
                continue;
            }
            let indent = line.text.len() - line.text.trim_start_matches(' ').len();
            if parent_indent.is_some_and(|p| indent <= p) {
                break; // end of the parent's block
            }
            let child = *child_indent.get_or_insert(indent);
            if indent != child {
                continue; // nested deeper, not a key of this mapping
            }
            if let Some(after) = yaml_key(&line.text[indent..], seg) {
                found = Some((idx, indent, indent + after));
                break;
            }
        }
        let (idx, indent, after) = found?;
        if n + 1 < segments.len() {
            parent_indent = Some(indent);
            from = idx + 1;
            continue;
        }
        let line = &lines[idx];
        let rest = &line.text[after..];
        let lead = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let value = &rest[lead..];
        let base = line.start + after + lead;
        let span = match value.chars().next()? {
            q @ ('"' | '\'') => {
                let close = value[1..].find(q)?;
                base + 1..base + 1 + close
            }
            '#' | '|' | '>' | '&' | '*' | '!' | '[' | '{' => return None,
            _ => {
                let end = [" #", "\t#"].iter().filter_map(|c| value.find(c)).min().unwrap_or(value.len());
                let scalar = value[..end].trim_end();
                base..base + scalar.len()
            }
        };
        return Some(span);
    }
    None
}
