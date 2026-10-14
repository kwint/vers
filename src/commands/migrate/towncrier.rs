//! Reading `[tool.towncrier]` and mapping towncrier types to vers types.
//!
//! Spec: `docs/how-to/migrate-from-towncrier.md`.

use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::error::{Error, Result};

/// The towncrier settings vers cares about.
#[derive(Debug, Clone)]
pub struct TowncrierConfig {
    /// The file the settings were read from.
    pub source: PathBuf,
    /// The fragment directory as written (or towncrier's default), `/`-separated
    /// and relative to the directory of [`TowncrierConfig::source`].
    pub directory: String,
    /// The changelog file as written (`filename`, default `NEWS.rst`).
    pub filename: String,
    /// Configured fragment types, in order.
    pub types: Vec<TowncrierType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TowncrierType {
    /// The type as used in fragment filenames.
    pub directory: String,
    /// The section heading.
    pub name: String,
}

/// towncrier's default types.
const DEFAULT_TYPES: [(&str, &str); 5] = [
    ("feature", "Features"),
    ("bugfix", "Bugfixes"),
    ("doc", "Improved Documentation"),
    ("removal", "Deprecations and Removals"),
    ("misc", "Misc"),
];

/// The default mapping from towncrier types to vers types. `None` means the
/// fragments are dropped.
const DEFAULT_MAP: [(&str, Option<&str>); 7] = [
    ("feature", Some("added")),
    ("bugfix", Some("fixed")),
    ("removal", Some("removed")),
    ("deprecation", Some("deprecated")),
    ("security", Some("security")),
    ("doc", Some("changed")),
    ("misc", None),
];

/// What a towncrier type becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Renamed to this canonical vers type.
    Type(String),
    /// Deleted.
    Drop,
}

/// The vers target of a towncrier type from the default mapping, or from the
/// default vers types and their aliases (`fix`, `feat`, `added`, …).
pub fn default_target(tc_type: &str) -> Option<Target> {
    let lower = tc_type.to_ascii_lowercase();
    if let Some((_, target)) = DEFAULT_MAP.iter().find(|(from, _)| *from == lower) {
        return Some(match target {
            Some(t) => Target::Type((*t).to_string()),
            None => Target::Drop,
        });
    }
    config::default_types()
        .iter()
        .find(|t| t.name == lower || t.aliases.contains(&lower))
        .map(|t| Target::Type(t.name.clone()))
}

/// Read `[tool.towncrier]` from `pyproject.toml`, or else from
/// `towncrier.toml`, in `dir`. `None` if neither has it.
pub fn read(dir: &Path) -> Result<Option<TowncrierConfig>> {
    for file in ["pyproject.toml", "towncrier.toml"] {
        let path = dir.join(file);
        let Some(table) = read_toml(&path)? else { continue };
        let Some(towncrier) = table.get("tool").and_then(|t| t.get("towncrier")) else { continue };
        let towncrier = towncrier.as_table().ok_or_else(|| Error::config(&path, "`tool.towncrier` must be a table"))?;
        return parse(&path, towncrier).map(Some);
    }
    Ok(None)
}

/// Parse a TOML file; `None` if it doesn't exist.
pub fn read_toml(path: &Path) -> Result<Option<toml::Table>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::io(path, e)),
    };
    text.parse::<toml::Table>().map(Some).map_err(|e| Error::config(path, format!("invalid TOML: {}", e.message())))
}

fn parse(path: &Path, table: &toml::Table) -> Result<TowncrierConfig> {
    let string = |key: &str| -> Result<Option<String>> {
        match table.get(key) {
            None => Ok(None),
            Some(toml::Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(Error::config(path, format!("`tool.towncrier.{key}` must be a string"))),
        }
    };

    // towncrier's default directory is `newsfragments` inside the Python
    // package when `package` is set, and `newsfragments` otherwise.
    let directory = match string("directory")? {
        Some(dir) => dir,
        None => match string("package")? {
            Some(package) if !package.is_empty() => {
                let package_dir = string("package_dir")?.unwrap_or_else(|| ".".to_string());
                let mut parts: Vec<&str> =
                    package_dir.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect();
                parts.extend(package.split('.'));
                parts.push("newsfragments");
                parts.join("/")
            }
            _ => "newsfragments".to_string(),
        },
    };
    let directory = directory.replace('\\', "/");
    let directory = directory.trim_start_matches("./").trim_end_matches('/').to_string();
    let directory = if directory.is_empty() { ".".to_string() } else { directory };
    let filename = string("filename")?.unwrap_or_else(|| "NEWS.rst".to_string());

    Ok(TowncrierConfig { source: path.to_path_buf(), directory, filename, types: parse_types(path, table)? })
}

/// `[[tool.towncrier.type]]` (older style) or `[tool.towncrier.fragment.<type>]`
/// (newer style); towncrier's defaults if neither is set.
fn parse_types(path: &Path, table: &toml::Table) -> Result<Vec<TowncrierType>> {
    if let Some(types) = table.get("type") {
        let types =
            types.as_array().ok_or_else(|| Error::config(path, "`tool.towncrier.type` must be an array of tables"))?;
        let mut out = Vec::new();
        for t in types {
            let directory = t
                .get("directory")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| Error::config(path, "every `[[tool.towncrier.type]]` needs a `directory` string"))?;
            let name = t.get("name").and_then(toml::Value::as_str).unwrap_or(directory);
            out.push(TowncrierType { directory: directory.to_string(), name: name.to_string() });
        }
        return Ok(out);
    }
    if let Some(fragments) = table.get("fragment") {
        let fragments = fragments
            .as_table()
            .ok_or_else(|| Error::config(path, "`tool.towncrier.fragment` must be a table of tables"))?;
        return Ok(fragments
            .iter()
            .map(|(key, value)| {
                let name = value.get("name").and_then(toml::Value::as_str).unwrap_or(key);
                TowncrierType { directory: key.clone(), name: name.to_string() }
            })
            .collect());
    }
    Ok(DEFAULT_TYPES
        .iter()
        .map(|(directory, name)| TowncrierType { directory: (*directory).to_string(), name: (*name).to_string() })
        .collect())
}

/// `s` with every character outside `[A-Za-z0-9_-]` replaced by `-`, runs of
/// `-` collapsed and leading/trailing `-` trimmed. Empty if nothing is left.
pub fn sanitize(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// The custom vers type a towncrier type needs, when it doesn't map onto a
/// default type: `(name, heading)`.
pub fn custom_type(t: &TowncrierType) -> Option<(String, String)> {
    if default_target(&t.directory).is_some() {
        return None;
    }
    let name = sanitize(&t.directory);
    let name = if name.is_empty() { "other".to_string() } else { name };
    Some((name, t.name.clone()))
}

/// The vers target for a type found in a fragment filename: `--map`
/// overrides, then the default mapping, then the config's own types (which
/// covers custom types written by `vers init --from-towncrier`).
pub fn target_for(tc_type: &str, overrides: &[(String, Target)], config: &Config) -> Option<Target> {
    let lower = tc_type.to_ascii_lowercase();
    if let Some((_, target)) = overrides.iter().find(|(from, _)| *from == lower) {
        return Some(target.clone());
    }
    if let Some(target) = DEFAULT_MAP.iter().find(|(from, _)| *from == lower).map(|(_, t)| match t {
        Some(t) => Target::Type((*t).to_string()),
        None => Target::Drop,
    }) {
        // A default target the config doesn't define (custom types replaced
        // the defaults) falls through to the config's own types.
        match &target {
            Target::Type(t) if config.find_type(t).is_none() => {}
            _ => return Some(target),
        }
    }
    let sanitized = sanitize(tc_type);
    config.find_type(tc_type).or_else(|| config.find_type(&sanitized)).map(|t| Target::Type(t.name.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_targets() {
        assert_eq!(default_target("feature"), Some(Target::Type("added".into())));
        assert_eq!(default_target("misc"), Some(Target::Drop));
        assert_eq!(default_target("fix"), Some(Target::Type("fixed".into())));
        assert_eq!(default_target("perf"), None);
    }

    #[test]
    fn sanitizes() {
        assert_eq!(sanitize("abc.def"), "abc-def");
        assert_eq!(sanitize("+x y!"), "x-y");
        assert_eq!(sanitize("..."), "");
    }

    #[test]
    fn package_default_directory() {
        let table: toml::Table = "package = \"foo.bar\"\npackage_dir = \"src\"".parse().unwrap();
        let config = parse(Path::new("pyproject.toml"), &table).unwrap();
        assert_eq!(config.directory, "src/foo/bar/newsfragments");
        assert_eq!(config.filename, "NEWS.rst");
        assert_eq!(config.types.len(), 5);
    }
}
