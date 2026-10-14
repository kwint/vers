//! `vers new`: create a fragment.
//!
//! Spec: `docs/reference/cli.md#vers-new`,
//! `docs/how-to/link-issues-and-merge-requests.md`.

use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::cli::NewArgs;
use crate::config::{self, Config, RefKind, Selection};
use crate::context::Context;
use crate::error::{Error, Result};
use crate::fragment;

#[derive(Serialize)]
struct JsonOutput {
    path: String,
    #[serde(rename = "type")]
    type_name: String,
    name: String,
    refs: Vec<String>,
    breaking: bool,
}

pub fn run(ctx: &Context, args: &NewArgs) -> Result<()> {
    let selection = match &args.package {
        Some(name) => Selection::Named(vec![name.clone()]),
        None => Selection::Current,
    };
    let config = config::select_packages(&ctx.cwd, &selection, ctx.config_override.as_deref())?
        .into_iter()
        .next()
        .expect("select_packages returns at least one package");

    let type_name = match &args.r#type {
        Some(t) => resolve_type(&config, t)?,
        None if std::io::stdin().is_terminal() => prompt_type(&config)?,
        None => {
            return Err(Error::Usage(format!("missing change type; pass one of: {}", type_list(&config))));
        }
    };

    if let Some(name) = &args.name {
        validate_name(name, &config.refs)?;
    }

    let mut extra_refs = Vec::new();
    for raw in &args.refs {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(Error::Usage("--ref must not be empty".to_string()));
        }
        if fragment::resolve_ref(raw, &config.refs).is_none() && fragment::looks_like_ref(raw) {
            ctx.warn(format!(
                "reference `{raw}` matches no reference kind, so it will be shown as plain text without a link"
            ));
        }
        extra_refs.push(raw.to_string());
    }

    let message = match &args.message {
        Some(m) => m.clone(),
        None => read_message()?,
    };
    let message = message.trim();
    if message.is_empty() {
        return Err(Error::Usage("the fragment message is empty; describe the change".to_string()));
    }

    let name = match &args.name {
        Some(name) => name.clone(),
        None => fragment::slug_from_message(message),
    };

    let dir = &config.fragments.directory;
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let content = fragment::render_file(message, &extra_refs, args.breaking);
    let path = write_fragment(&config, &name, &type_name, &content)?;

    let loaded = match fragment::load(&path, &config) {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
    };

    let shown = super::init::show_path(ctx, &path);
    if ctx.json_output() {
        ctx.json(&JsonOutput {
            path: shown,
            type_name: loaded.type_name,
            name: loaded.name,
            refs: loaded.refs.into_iter().map(|r| r.raw).collect(),
            breaking: loaded.breaking,
        });
    } else {
        ctx.line(format!("Created {shown}"));
    }
    Ok(())
}

/// Create the fragment file, adding a counter if the name is taken (also if
/// another process creates it at the same moment).
fn write_fragment(config: &Config, name: &str, type_name: &str, content: &str) -> Result<PathBuf> {
    let mut previous: Option<PathBuf> = None;
    loop {
        let path = fragment::new_fragment_path(config, name, type_name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(content.as_bytes()) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(Error::io(&path, e));
                }
                return Ok(path);
            }
            // Taken since `new_fragment_path` looked: try the next counter. If
            // the same path comes back, something `exists()` can't see (such
            // as a dangling symlink) is in the way.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && previous.as_ref() != Some(&path) => {
                previous = Some(path);
            }
            Err(e) => return Err(Error::io(&path, e)),
        }
    }
}

fn type_list(config: &Config) -> String {
    config.types.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")
}

fn resolve_type(config: &Config, name: &str) -> Result<String> {
    config.find_type(name.trim()).map(|t| t.name.clone()).ok_or_else(|| {
        Error::Usage(format!(
            "unknown change type `{name}`; expected one of: {} (or one of their aliases)",
            type_list(config)
        ))
    })
}

/// Ask for the change type on stderr and read the answer from stdin.
fn prompt_type(config: &Config) -> Result<String> {
    let mut stderr = std::io::stderr();
    let mut menu = String::from("Change type:\n");
    for (i, t) in config.types.iter().enumerate() {
        menu.push_str(&format!("  {}) {}\n", i + 1, t.name));
    }
    menu.push_str(&format!("Choose [1-{}]: ", config.types.len()));
    let _ = stderr.write_all(menu.as_bytes());
    let _ = stderr.flush();

    let mut answer = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|e| Error::Usage(format!("can't read the change type: {e}")))?;
    let answer = answer.trim();
    if answer.is_empty() {
        return Err(Error::Usage(format!("no change type chosen; pass one of: {}", type_list(config))));
    }
    if let Ok(n) = answer.parse::<usize>() {
        return match config.types.get(n.wrapping_sub(1)) {
            Some(t) => Ok(t.name.clone()),
            None => Err(Error::Usage(format!("no change type number {n}; choose 1 to {}", config.types.len()))),
        };
    }
    resolve_type(config, answer)
}

fn validate_name(name: &str, kinds: &[RefKind]) -> Result<()> {
    if !fragment::is_valid_name(name) {
        return Err(Error::Usage(format!(
            "invalid fragment name `{name}`: use only letters, digits, `-` and `_` (e.g. `123`, `mr-45` or `oauth-login`)"
        )));
    }
    if fragment::resolve_ref(name, kinds).is_none() && fragment::looks_like_ref(name) {
        let configured = if kinds.is_empty() {
            "no reference kinds are configured".to_string()
        } else {
            let names: Vec<String> = kinds.iter().map(|k| format!("`{}` (/{}/)", k.name, k.matcher.as_str())).collect();
            format!("configured kinds: {}", names.join(", "))
        };
        return Err(Error::Usage(format!(
            "`{name}` looks like a reference but matches no reference kind ({configured}); \
             fix the typo, add a `[refs.<kind>]` table to the config, or use a descriptive slug instead"
        )));
    }
    Ok(())
}

/// The message from `$VISUAL`/`$EDITOR`, or from stdin when no editor is set
/// and stdin isn't a terminal.
fn read_message() -> Result<String> {
    let editor = ["VISUAL", "EDITOR"].iter().filter_map(|var| std::env::var(var).ok()).find(|v| !v.trim().is_empty());
    if let Some(editor) = editor {
        return edit_message(&editor);
    }
    if std::io::stdin().is_terminal() {
        return Err(Error::Usage(
            "no message given; pass -m <MESSAGE>, set $VISUAL or $EDITOR, or pipe the message to stdin".to_string(),
        ));
    }
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| Error::Usage(format!("can't read the message from stdin: {e}")))?;
    Ok(text)
}

/// Open `editor` on a temporary file and return what was written to it.
fn edit_message(editor: &str) -> Result<String> {
    let path = create_temp_file()?;
    let result = run_editor(editor, &path);
    let text = result.and_then(|()| crate::error::read_to_string(&path));
    let _ = std::fs::remove_file(&path);
    text
}

fn run_editor(editor: &str, path: &Path) -> Result<()> {
    let mut words = split_command(editor);
    if words.is_empty() {
        return Err(Error::Usage("the editor command is empty; check $VISUAL and $EDITOR".to_string()));
    }
    let program = resolve_program(&words.remove(0));
    let status = Command::new(&program)
        .args(&words)
        .arg(path)
        .status()
        .map_err(|e| Error::Failed(format!("can't start the editor `{editor}`: {e}")))?;
    if !status.success() {
        let how = match status.code() {
            Some(code) => format!("exited with code {code}"),
            None => "was terminated".to_string(),
        };
        return Err(Error::Failed(format!("the editor `{editor}` {how}; no fragment created")));
    }
    Ok(())
}

/// The program to run for `name`. On Windows, `Command` only finds `.exe`
/// files on `PATH`, but editors are often `.cmd` shims (`code`), so a bare
/// name is looked up with each `PATHEXT` extension.
fn resolve_program(name: &str) -> PathBuf {
    if cfg!(windows) && Path::new(name).extension().is_none() && !name.contains(['/', '\\']) {
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        if let Some(path) = std::env::var_os("PATH").and_then(|p| find_in_path(name, &p, &exts)) {
            return path;
        }
    }
    PathBuf::from(name)
}

fn find_in_path(name: &str, path_var: &std::ffi::OsStr, exts: &str) -> Option<PathBuf> {
    std::env::split_paths(path_var).find_map(|dir| {
        exts.split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| dir.join(format!("{name}{}", ext.to_ascii_lowercase())))
            .find(|candidate| candidate.is_file())
    })
}

/// Split an editor command into words, honouring simple single and double quotes.
fn split_command(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for c in command.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                in_word = true;
            }
            None if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            None => {
                current.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

/// Create a new, empty temporary file for the editor. `create_new` makes sure
/// an existing file (or a symlink planted in a shared temp directory) is never
/// reused.
fn create_temp_file() -> Result<PathBuf> {
    let dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    for attempt in 0u32.. {
        let path = dir.join(format!("vers-fragment-{}-{nanos}-{attempt}.md", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 100 => {}
            Err(e) => return Err(Error::io(&path, e)),
        }
    }
    unreachable!("the loop returns")
}

#[cfg(test)]
mod tests {
    use super::{find_in_path, split_command};

    #[test]
    fn program_lookup_with_extensions() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(b.path().join("code.cmd"), "").unwrap();
        let path = std::env::join_paths([a.path(), b.path()]).unwrap();
        assert_eq!(find_in_path("code", &path, ".EXE;.CMD"), Some(b.path().join("code.cmd")));
        assert_eq!(find_in_path("vim", &path, ".EXE;.CMD"), None);
    }

    #[test]
    fn editor_commands() {
        assert_eq!(split_command("vim"), ["vim"]);
        assert_eq!(split_command("code --wait"), ["code", "--wait"]);
        assert_eq!(split_command(r#""C:\Program Files\Editor\ed.exe" -w"#), [r"C:\Program Files\Editor\ed.exe", "-w"]);
        assert_eq!(split_command("  "), Vec::<String>::new());
    }
}
