//! Rewriting towncrier release headings to Keep a Changelog headings.
//!
//! towncrier's Markdown template writes `## <name> <version> (<date>)`. Older
//! setups use other heading levels, or setext underlines (`===`/`---`). Each
//! recognised heading becomes `## [<version>] - <date>`; every other byte of
//! the file is kept.

use std::sync::LazyLock;

use regex::Regex;

static TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:.*?\s)?v?(?P<version>\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)\s+\((?P<date>\d{4}-\d{2}-\d{2})\)$",
    )
    .expect("valid regex")
});

/// The Keep a Changelog heading for a towncrier title such as
/// `project 1.2.0 (2026-01-31)`, or `None` if `title` isn't one.
fn convert_title(title: &str) -> Option<String> {
    let caps = TITLE.captures(title.trim())?;
    let version = semver::Version::parse(&caps["version"]).ok()?;
    Some(format!("## [{version}] - {}", &caps["date"]))
}

/// The text of an ATX heading line (`#` to `######`), without the markers.
fn atx_text(line: &str) -> Option<&str> {
    let trimmed = line.strip_prefix("   ").or_else(|| line.strip_prefix("  ")).or_else(|| line.strip_prefix(' '));
    let line = trimmed.unwrap_or(line);
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    // A closing sequence of `#`s is not part of the text.
    let text = rest.trim();
    let text = text.trim_end_matches('#');
    Some(text.trim_end())
}

fn is_setext_underline(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && (t.bytes().all(|b| b == b'=') || t.bytes().all(|b| b == b'-'))
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// `text` with every towncrier release heading rewritten. Line endings are
/// kept as they were.
pub fn convert(text: &str) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        let content = raw.trim_end_matches(['\n', '\r']);
        let eol = &raw[content.len()..];
        if is_fence(content) {
            in_fence = !in_fence;
        }
        if !in_fence {
            if let Some(heading) = atx_text(content).and_then(convert_title) {
                out.push_str(&heading);
                out.push_str(eol);
                i += 1;
                continue;
            }
            let previous_blank = i == 0 || lines[i - 1].trim().is_empty();
            if previous_blank
                && let Some(next) = lines.get(i + 1)
                && is_setext_underline(next)
                && let Some(heading) = convert_title(content)
            {
                let next_content = next.trim_end_matches(['\n', '\r']);
                out.push_str(&heading);
                out.push_str(&next[next_content.len()..]);
                i += 2;
                continue;
            }
        }
        out.push_str(raw);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::convert;

    #[test]
    fn atx_headings() {
        assert_eq!(convert("## project 1.2.0 (2026-01-31)\n"), "## [1.2.0] - 2026-01-31\n");
        assert_eq!(convert("# 1.2.0 (2026-01-31)\r\nx\r\n"), "## [1.2.0] - 2026-01-31\r\nx\r\n");
        assert_eq!(convert("### My Project v2.0.0-rc.1 (2026-01-31) ###"), "## [2.0.0-rc.1] - 2026-01-31");
    }

    #[test]
    fn setext_headings() {
        assert_eq!(
            convert("# Changelog\n\nproject 1.2.0 (2026-01-31)\n==========================\n\n- x\n"),
            "# Changelog\n\n## [1.2.0] - 2026-01-31\n\n- x\n"
        );
    }

    #[test]
    fn leaves_other_lines_alone() {
        let text =
            "# Changelog\n\n## [1.0.0] - 2026-01-01\n\n```\n## p 1.2.0 (2026-01-31)\n```\nsee 1.2.0 (2026-01-31)\n";
        assert_eq!(convert(text), text);
        assert_eq!(convert("## p 1.2 (2026-01-31)\n"), "## p 1.2 (2026-01-31)\n");
    }
}
