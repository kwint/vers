//! State and output helpers shared by every command.
//!
//! Output conventions:
//! - Normal output goes to stdout through [`Context::line`] (suppressed by `--quiet`).
//! - Warnings go to stderr as `warning: …` through [`Context::warn`].
//! - Errors are returned as [`crate::Error`]; `main` prints them as `error: …`.
//! - With `--format json`, a command prints exactly one JSON document through
//!   [`Context::json`] and nothing else on stdout.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cli::{ColorChoice, Format, GlobalArgs};

#[derive(Debug, Clone)]
pub struct Context {
    pub cwd: PathBuf,
    pub config_override: Option<PathBuf>,
    pub format: Format,
    pub color: ColorChoice,
    pub quiet: bool,
    pub verbose: u8,
}

impl Context {
    pub fn new(cwd: PathBuf, global: &GlobalArgs) -> Context {
        Context {
            config_override: global.config.as_ref().map(|p| cwd.join(p)),
            cwd,
            format: global.format,
            color: global.color,
            quiet: global.quiet,
            verbose: global.verbose,
        }
    }

    pub fn json_output(&self) -> bool {
        self.format == Format::Json
    }

    /// Print a line of normal text output.
    pub fn line(&self, text: impl AsRef<str>) {
        if !self.quiet && !self.json_output() {
            println!("{}", text.as_ref());
        }
    }

    /// Print a warning to stderr.
    pub fn warn(&self, text: impl AsRef<str>) {
        if !self.quiet {
            eprintln!("warning: {}", relativise_message(text.as_ref(), &self.cwd));
        }
    }

    /// Print detail shown only with `-v`.
    pub fn verbose(&self, text: impl AsRef<str>) {
        if self.verbose > 0 {
            eprintln!("{}", text.as_ref());
        }
    }

    /// Print the command's JSON document.
    pub fn json(&self, value: &impl Serialize) {
        println!("{}", serde_json::to_string_pretty(value).expect("output is serialisable"));
    }

    /// `path` relative to the current directory (see [`display_relative`]), for display.
    pub fn display_path(&self, path: &Path) -> String {
        display_relative(path, &self.cwd)
    }
}

/// `path` relative to `base` with `/` separators, using `..` when `path` isn't
/// below `base`, and an empty string when they're equal. Falls back to the full path when they share no root (e.g.
/// different drives on Windows).
pub fn display_relative(path: &Path, base: &Path) -> String {
    use std::path::Component;
    let path_parts: Vec<Component> = path.components().collect();
    let base_parts: Vec<Component> = base.components().collect();
    let common = path_parts.iter().zip(&base_parts).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return path.to_string_lossy().replace('\\', "/");
    }
    let mut parts: Vec<String> = (common..base_parts.len()).map(|_| "..".to_string()).collect();
    parts.extend(path_parts[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    parts.join("/")
}

/// `message` with absolute paths below `base` shown relative to it. Core
/// modules format paths with `Path::display`; this keeps user-facing errors
/// and warnings short.
pub fn relativise_message(message: &str, base: &Path) -> String {
    let mut prefix = base.display().to_string();
    if !prefix.ends_with(std::path::MAIN_SEPARATOR) {
        prefix.push(std::path::MAIN_SEPARATOR);
    }
    message.replace(&prefix, "")
}

/// Today's date as `YYYY-MM-DD` in local time. Honours `SOURCE_DATE_EPOCH`
/// for reproducible output (used by the docs tests).
pub fn release_date() -> String {
    if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH")
        && let Ok(secs) = epoch.trim().parse::<i64>()
        && let Ok(ts) = jiff::Timestamp::from_second(secs)
    {
        return ts.to_zoned(jiff::tz::TimeZone::UTC).date().to_string();
    }
    jiff::Zoned::now().date().to_string()
}
