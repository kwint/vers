//! Loading one config file: parsing, following `extend`, validation and
//! resolution into the public [`Config`] types.
//!
//! Each file in an `extend` chain is parsed and validated on its own, so an
//! error always names the file that contains the mistake. Relative paths are
//! made absolute against the directory of the file that wrote them *before*
//! the files are merged, so an inherited path keeps pointing where its author
//! meant.

use std::path::{Component, Path, PathBuf};

use indexmap::IndexMap;
use regex::Regex;
use semver::{Version, VersionReq};
use serde::Deserialize;

use super::{
    ChangeType, ChangelogSettings, CheckSettings, Config, FragmentSettings, PackageSettings, Preset, RefKind,
    RefPosition, RepoKind, VersionFileSpec, VersionLocator, VersionSettings, default_types, preset_ref_kinds,
    preset_ref_templates,
};
use crate::error::{Error, Result};
use crate::git::{self, Git};
use crate::version::Bump;

// ---------------------------------------------------------------------------
// Raw (as written) config
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    extend: Option<String>,
    required_version: Option<String>,
    package: Option<RawPackage>,
    changelog: Option<RawChangelog>,
    fragments: Option<RawFragments>,
    types: Option<Vec<RawType>>,
    refs: Option<IndexMap<String, RawRef>>,
    version: Option<RawVersion>,
    check: Option<RawCheck>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPackage {
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChangelog {
    path: Option<String>,
    repo: Option<RawRepo>,
    repo_url: Option<String>,
    compare_url: Option<String>,
    release_url: Option<String>,
    ref_position: Option<RawRefPosition>,
    breaking_prefix: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawRepo {
    Auto,
    Github,
    Gitlab,
    None,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawRefPosition {
    End,
    Start,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFragments {
    directory: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawType {
    name: String,
    heading: String,
    #[serde(default)]
    aliases: Vec<String>,
    bump: Option<RawBump>,
    #[serde(default)]
    hidden: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawBump {
    Major,
    Minor,
    Patch,
    None,
}

impl From<RawBump> for Bump {
    fn from(b: RawBump) -> Bump {
        match b {
            RawBump::Major => Bump::Major,
            RawBump::Minor => Bump::Minor,
            RawBump::Patch => Bump::Patch,
            RawBump::None => Bump::None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRef {
    #[serde(rename = "match")]
    matcher: Option<String>,
    display: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVersion {
    initial: Option<String>,
    zero_major_shift: Option<bool>,
    fold_prereleases: Option<bool>,
    tag: Option<bool>,
    tag_format: Option<String>,
    commit_message: Option<String>,
    files: Option<Vec<RawVersionFile>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVersionFile {
    path: String,
    key: Option<String>,
    pattern: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheck {
    require_fragment: Option<bool>,
    ignore: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// One file of an `extend` chain, with its relative paths already resolved.
struct Layer {
    dir: PathBuf,
    table: toml::Table,
}

/// Load, merge and resolve the config at `path`.
pub(super) fn load_config(path: &Path) -> Result<Config> {
    let path = absolute(path);
    let layers = read_chain(&path)?;

    // Merge from the base of the chain up to `path`, so later files win.
    let mut merged = toml::Table::new();
    let mut ignore_origin: Option<PathBuf> = None;
    for layer in layers.into_iter().rev() {
        if layer.table.get("check").and_then(toml::Value::as_table).is_some_and(|c| c.contains_key("ignore")) {
            ignore_origin = Some(layer.dir.clone());
        }
        deep_merge(&mut merged, layer.table);
    }
    let raw: RawConfig = toml::Value::Table(merged)
        .try_into()
        .map_err(|e: toml::de::Error| Error::config(&path, format!("invalid config: {}", e.message().trim_end())))?;
    resolve(&path, raw, ignore_origin)
}

/// Read `path` and every config it extends, `path` first.
fn read_chain(path: &Path) -> Result<Vec<Layer>> {
    let mut layers = Vec::new();
    let mut chain: Vec<PathBuf> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        let key = current.canonicalize().unwrap_or_else(|_| current.clone());
        if seen.contains(&key) {
            chain.push(current.clone());
            let shown: Vec<String> = chain.iter().map(|p| p.display().to_string()).collect();
            return Err(Error::config(
                &chain[0],
                format!("`extend` forms a cycle: {}; remove one of the `extend` keys", shown.join(" -> ")),
            ));
        }
        seen.push(key);
        chain.push(current.clone());

        let (dir, mut table, extend) = read_layer(&current)?;
        if !layers.is_empty() {
            // Being a package is a property of the file itself, not a setting:
            // inheriting `[package]` would turn a shared config that extends a
            // package into a package, and copy an explicit `name` into a
            // second package.
            table.remove("package");
        }
        layers.push(Layer { dir: dir.clone(), table });
        let Some(extend) = extend else { break };
        if extend.trim().is_empty() {
            return Err(Error::config(&current, "`extend` is empty; set it to the path of another vers.toml"));
        }
        let target = normalize(&dir.join(&extend));
        if !target.is_file() {
            let what = if target.is_dir() { "is a directory, not a config file" } else { "doesn't exist" };
            return Err(Error::config(
                &current,
                format!("`extend = \"{extend}\"` points to {}, which {what}", target.display()),
            ));
        }
        current = target;
    }
    Ok(layers)
}

/// Parse and validate one file. Returns its directory, its table with
/// relative paths resolved and `extend` removed, and its `extend` value.
fn read_layer(path: &Path) -> Result<(PathBuf, toml::Table, Option<String>)> {
    let text =
        std::fs::read_to_string(path).map_err(|e| Error::config(path, format!("can't read config file: {e}")))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);

    let mut table: toml::Table = toml::from_str(text).map_err(|e| toml_error(path, text, &e))?;
    // Check the version first: a config written for a newer vers may use keys
    // this one doesn't know, and the version mismatch is the real problem.
    check_required_version(path, &table)?;
    let raw: RawConfig = toml::from_str(text).map_err(|e| toml_error(path, text, &e))?;
    validate_layer(&raw).map_err(|m| Error::config(path, m))?;

    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let extend = raw.extend;
    table.remove("extend");
    resolve_table_paths(&mut table, &dir);
    Ok((dir, table, extend))
}

fn toml_error(path: &Path, text: &str, e: &toml::de::Error) -> Error {
    let message = e.message().trim_end();
    match e.span() {
        Some(span) => {
            let (line, column) = line_column(text, span.start);
            Error::config(path, format!("invalid config at line {line}, column {column}: {message}"))
        }
        None => Error::config(path, format!("invalid config: {message}")),
    }
}

fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let column = before[line_start..].chars().count() + 1;
    (line, column)
}

fn this_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is valid semver")
}

fn check_required_version(path: &Path, table: &toml::Table) -> Result<()> {
    let Some(toml::Value::String(req)) = table.get("required_version") else {
        return Ok(());
    };
    let parsed = parse_required_version(req).map_err(|m| Error::config(path, m))?;
    let actual = this_version();
    if !parsed.matches(&actual) {
        return Err(Error::config(
            path,
            format!(
                "this project requires vers {}, but this is vers {actual}; install a version of vers that matches `required_version`",
                req.trim()
            ),
        ));
    }
    Ok(())
}

fn parse_required_version(req: &str) -> std::result::Result<VersionReq, String> {
    VersionReq::parse(req)
        .map_err(|e| format!("invalid `required_version` \"{req}\": {e}; expected a requirement such as \">=1.2, <2\""))
}

/// Make the relative paths written in one file absolute against its directory.
/// `check.ignore` globs are handled separately (see [`rebase_glob`]).
fn resolve_table_paths(table: &mut toml::Table, dir: &Path) {
    let resolve = |value: &mut toml::Value| {
        if let toml::Value::String(s) = value {
            *s = normalize(&dir.join(&*s)).to_string_lossy().into_owned();
        }
    };
    if let Some(toml::Value::Table(changelog)) = table.get_mut("changelog")
        && let Some(path) = changelog.get_mut("path")
    {
        resolve(path);
    }
    if let Some(toml::Value::Table(fragments)) = table.get_mut("fragments")
        && let Some(directory) = fragments.get_mut("directory")
    {
        resolve(directory);
    }
    if let Some(toml::Value::Table(version)) = table.get_mut("version")
        && let Some(toml::Value::Array(files)) = version.get_mut("files")
    {
        for file in files {
            if let toml::Value::Table(file) = file
                && let Some(path) = file.get_mut("path")
            {
                resolve(path);
            }
        }
    }
}

/// Merge `over` into `base`: tables merge key by key, everything else
/// (including arrays) is replaced.
fn deep_merge(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        if let toml::Value::Table(over_table) = value {
            if let Some(toml::Value::Table(base_table)) = base.get_mut(&key) {
                deep_merge(base_table, over_table);
                continue;
            }
            base.insert(key, toml::Value::Table(over_table));
        } else {
            base.insert(key, value);
        }
    }
}

// ---------------------------------------------------------------------------
// Validation and resolution
// ---------------------------------------------------------------------------

/// Checks that only need one file, so the error can name that file.
fn validate_layer(raw: &RawConfig) -> std::result::Result<(), String> {
    if let Some(req) = &raw.required_version {
        parse_required_version(req)?;
    }
    if let Some(name) = raw.package.as_ref().and_then(|p| p.name.as_deref()) {
        validate_package_name(name)?;
    }
    if raw.changelog.as_ref().and_then(|c| c.path.as_deref()).is_some_and(|p| p.trim().is_empty()) {
        return Err("`changelog.path` is empty; set it to the changelog file, such as \"CHANGELOG.md\"".to_string());
    }
    if raw.fragments.as_ref().and_then(|f| f.directory.as_deref()).is_some_and(|d| d.trim().is_empty()) {
        return Err("`fragments.directory` is empty; set it to the fragment directory, such as \"changes\"".to_string());
    }
    if let Some(types) = &raw.types {
        build_types(types)?;
    }
    if let Some(refs) = &raw.refs {
        for (name, r) in refs {
            if let Some(pattern) = &r.matcher {
                compile_ref_match(name, pattern)?;
            }
        }
    }
    if let Some(version) = &raw.version {
        if let Some(initial) = &version.initial {
            parse_initial(initial)?;
        }
        for file in version.files.iter().flatten() {
            build_locator(file)?;
        }
    }
    if let Some(globs) = raw.check.as_ref().and_then(|c| c.ignore.as_ref()) {
        for glob in globs {
            validate_glob(glob)?;
        }
    }
    Ok(())
}

fn resolve(path: &Path, raw: RawConfig, ignore_origin: Option<PathBuf>) -> Result<Config> {
    let err = |m: String| Error::config(path, m);
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();

    let required_version = raw.required_version.as_deref().map(parse_required_version).transpose().map_err(err)?;

    let package = match raw.package {
        None => None,
        Some(p) => {
            let name = match p.name {
                Some(name) => name,
                None => dir.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| {
                    err(format!(
                        "can't derive a package name from the directory {}; set `name` in [package]",
                        dir.display()
                    ))
                })?,
            };
            validate_package_name(&name).map_err(|m| {
                err(format!("{m}; the name defaults to the directory name, so set `name` in [package]"))
            })?;
            Some(PackageSettings { name })
        }
    };

    let changelog = raw.changelog.unwrap_or_default();
    let explicit_url =
        changelog.repo_url.as_deref().map(|u| u.trim().trim_end_matches('/').to_string()).filter(|u| !u.is_empty());
    let repo_url = explicit_url.or_else(|| origin_web_url(&dir));
    let repo = match changelog.repo.unwrap_or(RawRepo::Auto) {
        RawRepo::Auto => repo_url.as_deref().map_or(RepoKind::None, git::detect_repo_kind),
        RawRepo::Github => RepoKind::Github,
        RawRepo::Gitlab => RepoKind::Gitlab,
        RawRepo::None => RepoKind::None,
    };
    let preset = Preset::for_kind(repo);
    let compare_url = link_template(changelog.compare_url.as_deref(), preset.compare_url, repo_url.as_deref());
    let release_url = link_template(changelog.release_url.as_deref(), preset.release_url, repo_url.as_deref());
    let refs = build_ref_kinds(repo, repo_url.as_deref(), raw.refs.as_ref()).map_err(err)?;

    let changelog_settings = ChangelogSettings {
        path: changelog.path.map(PathBuf::from).unwrap_or_else(|| dir.join("CHANGELOG.md")),
        repo,
        repo_url,
        compare_url,
        release_url,
        ref_position: match changelog.ref_position {
            Some(RawRefPosition::Start) => RefPosition::Start,
            Some(RawRefPosition::End) | None => RefPosition::End,
        },
        breaking_prefix: changelog.breaking_prefix.unwrap_or_else(|| "**Breaking:** ".to_string()),
    };

    let fragments = FragmentSettings {
        directory: raw.fragments.and_then(|f| f.directory).map(PathBuf::from).unwrap_or_else(|| dir.join("changes")),
    };

    let types = match &raw.types {
        Some(types) => build_types(types).map_err(err)?,
        None => default_types(),
    };

    let version = raw.version.unwrap_or_default();
    let files = version
        .files
        .unwrap_or_default()
        .iter()
        .map(|f| Ok(VersionFileSpec { path: PathBuf::from(&f.path), locator: build_locator(f)? }))
        .collect::<std::result::Result<Vec<_>, String>>()
        .map_err(err)?;
    let version_settings = VersionSettings {
        initial: match &version.initial {
            Some(initial) => parse_initial(initial).map_err(err)?,
            None => Version::new(0, 1, 0),
        },
        zero_major_shift: version.zero_major_shift.unwrap_or(true),
        fold_prereleases: version.fold_prereleases.unwrap_or(true),
        tag: version.tag.unwrap_or(false),
        tag_format: version.tag_format.unwrap_or_else(|| "v{version}".to_string()),
        commit_message: version.commit_message.unwrap_or_else(|| "Release {name} {version}".to_string()),
        files,
    };

    let check = raw.check.unwrap_or_default();
    let ignore_origin = ignore_origin.unwrap_or_else(|| dir.clone());
    let check_settings = CheckSettings {
        require_fragment: check.require_fragment.unwrap_or(true),
        ignore: check
            .ignore
            .unwrap_or_default()
            .iter()
            // `./docs/**` means the same as `docs/**`; the leading `./` would
            // stop the glob from matching any relative path.
            .map(|g| rebase_glob(strip_dot_slash(g), &ignore_origin, &dir))
            .collect(),
    };

    Ok(Config {
        path: path.to_path_buf(),
        dir,
        required_version,
        package,
        changelog: changelog_settings,
        fragments,
        types,
        refs,
        version: version_settings,
        check: check_settings,
    })
}

fn validate_package_name(name: &str) -> std::result::Result<(), String> {
    if name.trim().is_empty() {
        return Err("package `name` must not be empty".to_string());
    }
    if name.chars().any(char::is_whitespace) {
        return Err(format!(
            "invalid package name \"{name}\": it's used in CLI arguments and tags, so it can't contain whitespace"
        ));
    }
    Ok(())
}

/// The web URL of the `origin` remote of the repository containing `dir`.
fn origin_web_url(dir: &Path) -> Option<String> {
    let repo = Git::discover(dir).ok().flatten()?;
    let remote = repo.remote_url("origin").ok().flatten()?;
    git::web_url_from_remote(&remote)
}

/// Substitute `{repo_url}`; `None` if the template needs a URL that isn't known.
fn substitute_repo_url(template: &str, repo_url: Option<&str>) -> Option<String> {
    if template.contains("{repo_url}") {
        repo_url.map(|r| template.replace("{repo_url}", r))
    } else {
        Some(template.to_string())
    }
}

/// A configured link template wins over the preset's; an empty one turns links off.
fn link_template(configured: Option<&str>, preset: Option<&str>, repo_url: Option<&str>) -> Option<String> {
    match configured {
        Some(t) if t.trim().is_empty() => None,
        Some(t) => substitute_repo_url(t, repo_url),
        None => preset.and_then(|t| substitute_repo_url(t, repo_url)),
    }
}

fn build_types(raw: &[RawType]) -> std::result::Result<Vec<ChangeType>, String> {
    if raw.is_empty() {
        return Err(
            "`types` is empty; define at least one [[types]] entry, or remove `types` to use the defaults".to_string()
        );
    }
    let mut seen: Vec<(String, &str)> = Vec::new();
    let mut types = Vec::with_capacity(raw.len());
    for t in raw {
        validate_type_word(&t.name, "type name")?;
        if t.heading.trim().is_empty() {
            return Err(format!("type `{}` has an empty `heading`", t.name));
        }
        for alias in &t.aliases {
            validate_type_word(alias, &format!("alias of type `{}`", t.name))?;
        }
        for word in std::iter::once(&t.name).chain(&t.aliases) {
            let lower = word.to_lowercase();
            if let Some((_, owner)) = seen.iter().find(|(w, _)| *w == lower) {
                let by = if *owner == t.name {
                    format!("type `{owner}`")
                } else {
                    format!("types `{owner}` and `{}`", t.name)
                };
                return Err(format!(
                    "the type name or alias `{word}` is used more than once (by {by}); names and aliases must be unique"
                ));
            }
            seen.push((lower, &t.name));
        }
        types.push(ChangeType {
            name: t.name.clone(),
            heading: t.heading.clone(),
            aliases: t.aliases.clone(),
            bump: t.bump.map_or(Bump::Patch, Bump::from),
            hidden: t.hidden,
        });
    }
    Ok(types)
}

fn validate_type_word(word: &str, what: &str) -> std::result::Result<(), String> {
    if word.is_empty() {
        return Err(format!("{what} must not be empty"));
    }
    if word.contains(['.', '/', '\\']) || word.chars().any(char::is_whitespace) {
        return Err(format!(
            "invalid {what} \"{word}\": it's used in fragment filenames, so it can't contain dots, slashes or whitespace"
        ));
    }
    Ok(())
}

/// Compile a reference kind's `match` regex, anchored so it must match the
/// whole fragment name.
fn compile_ref_match(kind: &str, pattern: &str) -> std::result::Result<Regex, String> {
    let unanchored =
        Regex::new(pattern).map_err(|e| format!("invalid `match` regex for reference kind `{kind}`: {e}"))?;
    if !unanchored.capture_names().flatten().any(|n| n == "id") {
        return Err(format!(
            "the `match` regex for reference kind `{kind}` needs a named group `id`, such as '^(?<id>\\d+)$'"
        ));
    }
    Regex::new(&format!("^(?:{pattern})$"))
        .map_err(|e| format!("invalid `match` regex for reference kind `{kind}`: {e}"))
}

fn build_ref_kinds(
    kind: RepoKind,
    repo_url: Option<&str>,
    configured: Option<&IndexMap<String, RawRef>>,
) -> std::result::Result<Vec<RefKind>, String> {
    let mut kinds = preset_ref_kinds(kind, repo_url);
    let templates = preset_ref_templates(kind);
    for (name, r) in configured.into_iter().flatten() {
        let preset = templates.iter().find(|t| t.name == name);
        let pattern = r.matcher.as_deref().or(preset.map(|p| p.matcher)).ok_or_else(|| {
            format!("reference kind `{name}` needs a `match` regex with a named group `id`, such as '^(?<id>\\d+)$'")
        })?;
        let matcher = compile_ref_match(name, pattern)?;
        let display = r.display.as_deref().or(preset.map(|p| p.display)).unwrap_or("{id}").to_string();
        let url = match r.url.as_deref().or(preset.map(|p| p.url)) {
            None => None,
            Some(t) if t.trim().is_empty() => None,
            Some(t) => substitute_repo_url(t, repo_url),
        };
        let ref_kind = RefKind { name: name.clone(), matcher, display, url };
        match kinds.iter().position(|k| k.name == *name) {
            Some(i) => kinds[i] = ref_kind,
            None => kinds.push(ref_kind),
        }
    }
    Ok(kinds)
}

fn parse_initial(initial: &str) -> std::result::Result<Version, String> {
    Version::parse(initial.trim())
        .map_err(|e| format!("invalid `version.initial` \"{initial}\": {e}; expected a version such as \"0.1.0\""))
}

fn build_locator(file: &RawVersionFile) -> std::result::Result<VersionLocator, String> {
    if file.path.trim().is_empty() {
        return Err("a [[version.files]] entry has an empty `path`".to_string());
    }
    match (&file.key, &file.pattern) {
        (Some(_), Some(_)) => {
            Err(format!("version file \"{}\" sets both `key` and `pattern`; use one of them", file.path))
        }
        (Some(key), None) => {
            if key.split('.').any(|part| part.trim().is_empty()) {
                return Err(format!(
                    "invalid `key` \"{key}\" for version file \"{}\": expected a dotted key path such as \"package.version\"",
                    file.path
                ));
            }
            Ok(VersionLocator::Key(key.clone()))
        }
        (None, Some(pattern)) => {
            let re = Regex::new(pattern)
                .map_err(|e| format!("invalid `pattern` regex for version file \"{}\": {e}", file.path))?;
            if !re.capture_names().flatten().any(|n| n == "version") {
                return Err(format!(
                    "the `pattern` regex for version file \"{}\" needs a named group `version`, such as 'VERSION = \"(?<version>[^\"]+)\"'",
                    file.path
                ));
            }
            Ok(VersionLocator::Pattern(re))
        }
        (None, None) => Ok(VersionLocator::Whole),
    }
}

fn validate_glob(glob: &str) -> std::result::Result<(), String> {
    if strip_dot_slash(glob).trim().is_empty() {
        return Err(format!("`check.ignore` contains an empty glob \"{glob}\""));
    }
    globset::Glob::new(glob).map(drop).map_err(|e| format!("invalid glob \"{glob}\" in `check.ignore`: {e}"))
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// `path` made absolute against the current directory, with `.` and `..`
/// removed lexically. Symlinks are not resolved.
pub(super) fn absolute(path: &Path) -> PathBuf {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    normalize(&abs)
}

/// Remove `.` and `..` components lexically.
pub(super) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            other => out.push(other),
        }
    }
    out
}

fn path_parts(path: &Path) -> Vec<String> {
    path.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect()
}

/// Re-express `glob`, written relative to `from`, relative to `to`
/// ([`CheckSettings::ignore`] is relative to the config's own directory).
///
/// - Leading components that match the path from `from` down to `to` are
///   stripped: written in the root, `packages/api/docs/**` becomes `docs/**`
///   for `packages/api`.
/// - A glob that reaches `**` while stripping applies at any depth and is kept
///   from there.
/// - A glob without `/` (such as `*.md`) matches by file name anywhere below
///   the config that wrote it, so it's kept as is for configs below that one.
/// - Anything else can't match below `to`; it's kept with a `../` prefix so it
///   is still visible but never matches a file inside the package.
pub(super) fn rebase_glob(glob: &str, from: &Path, to: &Path) -> String {
    if from == to {
        return glob.to_string();
    }
    if let Ok(rel) = to.strip_prefix(from) {
        if !glob.contains('/') {
            return glob.to_string();
        }
        let parts = path_parts(rel);
        let mut rest = glob;
        let mut i = 0;
        loop {
            if rest.starts_with("**") || i == parts.len() {
                return rest.to_string();
            }
            match rest.split_once('/') {
                Some((head, tail)) if component_matches(head, &parts[i]) => {
                    rest = tail;
                    i += 1;
                }
                _ => break,
            }
        }
    }
    let prefix = relative_path(to, from);
    if prefix.is_empty() { glob.to_string() } else { format!("{prefix}/{glob}") }
}

fn strip_dot_slash(glob: &str) -> &str {
    let mut glob = glob;
    while let Some(rest) = glob.strip_prefix("./") {
        glob = rest.trim_start_matches('/');
    }
    glob
}

fn component_matches(glob_part: &str, name: &str) -> bool {
    if glob_part == name {
        return true;
    }
    globset::GlobBuilder::new(glob_part)
        .literal_separator(true)
        .build()
        .map(|g| g.compile_matcher().is_match(name))
        .unwrap_or(false)
}

/// The path from directory `base` to `target`, with `/` separators.
fn relative_path(base: &Path, target: &Path) -> String {
    let base = path_parts(base);
    let target = path_parts(target);
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; base.len() - common];
    parts.extend(target[common..].iter().map(String::as_str));
    parts.join("/")
}
