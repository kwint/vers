//! `vers new`: create a fragment.
//!
//! Spec: `docs/reference/cli.md#vers-new`,
//! `docs/how-to/link-issues-and-merge-requests.md`.

mod prompt;

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::cli::{ColorChoice, NewArgs};
use crate::config::{self, Config, RefKind, Selection};
use crate::context::Context;
use crate::error::{Error, Result};
use crate::fragment;
use prompt::{Prompter, TerminalPrompter};

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

    let interactive =
        is_interactive(ctx.json_output(), std::io::stdin().is_terminal(), std::io::stderr().is_terminal());

    let type_name = match &args.r#type {
        Some(t) => Some(resolve_type(&config, t)?),
        None if interactive => None,
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

    let answers = if interactive {
        let mut prompter = TerminalPrompter::new(color_enabled(ctx));
        ask(&config, args, type_name, &mut prompter, editor_from_env().as_deref())?
    } else {
        Answers {
            type_name: type_name.expect("a missing type is an error when not interactive"),
            name: args.name.clone(),
            message: match &args.message {
                Some(m) => m.clone(),
                None => read_message()?,
            },
            breaking: args.breaking,
        }
    };

    let message = answers.message.trim();
    if message.is_empty() {
        return Err(Error::Usage("the fragment message is empty; describe the change".to_string()));
    }

    let name = match answers.name {
        Some(name) => name,
        None => fragment::slug_from_message(message),
    };

    let dir = &config.fragments.directory;
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let content = fragment::render_file(message, &extra_refs, answers.breaking);
    let path = write_fragment(&config, &name, &answers.type_name, &content)?;

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

/// Whether to ask for what's missing: only when a person is at the terminal,
/// so never with JSON output and never when stdin or stderr is redirected.
fn is_interactive(json_output: bool, stdin_is_terminal: bool, stderr_is_terminal: bool) -> bool {
    !json_output && stdin_is_terminal && stderr_is_terminal
}

/// What goes into the fragment, from the arguments and the user's answers.
#[derive(Debug, PartialEq, Eq)]
struct Answers {
    type_name: String,
    /// `None`: make a slug from the message.
    name: Option<String>,
    message: String,
    breaking: bool,
}

/// Ask for everything `args` leaves out: the change type, the reference or
/// name, the message (in `editor` if one is set) and whether it's breaking.
fn ask(
    config: &Config,
    args: &NewArgs,
    type_name: Option<String>,
    prompter: &mut dyn Prompter,
    editor: Option<&str>,
) -> Result<Answers> {
    let type_name = match type_name {
        Some(t) => t,
        None => ask_type(config, prompter)?,
    };
    let name = match &args.name {
        Some(name) => Some(name.clone()),
        None => ask_name(&config.refs, prompter)?,
    };
    let message = match &args.message {
        Some(m) => m.clone(),
        None => ask_message(prompter, editor)?,
    };
    let breaking =
        args.breaking || prompter.confirm("Breaking change?", "Does it break anything for existing users?", false)?;
    Ok(Answers { type_name, name, message, breaking })
}

/// Short descriptions of the Keep a Changelog types.
const TYPE_HINTS: [(&str, &str); 6] = [
    ("added", "new features"),
    ("changed", "changes in existing functionality"),
    ("deprecated", "soon-to-be removed features"),
    ("removed", "now removed features"),
    ("fixed", "bug fixes"),
    ("security", "vulnerabilities"),
];

fn ask_type(config: &Config, prompter: &mut dyn Prompter) -> Result<String> {
    if config.types.is_empty() {
        return Err(Error::Usage("no change types are configured".to_string()));
    }
    let options: Vec<String> = config
        .types
        .iter()
        .map(|t| match TYPE_HINTS.iter().find(|(name, _)| *name == t.name) {
            Some((_, hint)) => format!("{} — {hint}", t.heading),
            None => t.heading.clone(),
        })
        .collect();
    let index = prompter.select("Change type:", &options, "↑↓ to move, enter to choose, type to filter")?;
    Ok(config.types[index].name.clone())
}

/// Ask for the fragment name: a reference or a slug. `None` when left empty.
fn ask_name(kinds: &[RefKind], prompter: &mut dyn Prompter) -> Result<Option<String>> {
    let examples = ref_examples(kinds);
    let (prompt, placeholder, help) = if examples.is_empty() {
        (
            "Name (optional):".to_string(),
            "e.g. oauth-login".to_string(),
            "Letters, digits, - and _; empty: a slug from the message".to_string(),
        )
    } else {
        let labels: Vec<&str> = examples.iter().map(|e| e.label.as_str()).collect();
        let names: Vec<&str> = examples.iter().map(|e| e.name.as_str()).collect();
        let links: Vec<String> = examples.iter().map(|e| format!("{} → {}", e.name, e.display)).collect();
        (
            format!("{} (optional):", capitalise(&join_or(&labels))),
            format!("e.g. {}", join_or(&names)),
            format!("{}; other text is a slug; empty: a slug from the message", links.join(", ")),
        )
    };
    let validate = |input: &str| {
        let input = input.trim();
        if input.is_empty() { Ok(()) } else { validate_name(input, kinds).map_err(|e| e.to_string()) }
    };
    let answer = prompter.text(&prompt, &placeholder, &help, &validate)?;
    let answer = answer.trim();
    Ok((!answer.is_empty()).then(|| answer.to_string()))
}

/// An example fragment name for a reference kind.
struct RefExample {
    /// e.g. `mr-45`.
    name: String,
    /// e.g. `merge request`.
    label: String,
    /// The link text, e.g. `!45`.
    display: String,
}

/// An example name for each reference kind that one of a few common shapes
/// resolves to.
fn ref_examples(kinds: &[RefKind]) -> Vec<RefExample> {
    const CANDIDATES: [&str; 7] = ["123", "mr-45", "pr-45", "PROJ-42", "GH-123", "gh-123", "abc-12"];
    kinds
        .iter()
        .filter_map(|kind| {
            CANDIDATES.iter().find_map(|candidate| {
                let r = fragment::resolve_ref(candidate, kinds)?;
                (r.kind.as_deref() == Some(kind.name.as_str())).then(|| RefExample {
                    name: candidate.to_string(),
                    label: match kind.name.as_str() {
                        "mr" => "merge request".to_string(),
                        "pr" => "pull request".to_string(),
                        other => other.to_string(),
                    },
                    display: r.display,
                })
            })
        })
        .collect()
}

/// `a`, `a or b`, `a, b or c`.
fn join_or(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// Instructions put in the editor below the (empty) first line; removed again
/// afterwards.
const EDITOR_HELP: &str = "\n<!--
Describe the change for the changelog. The first line becomes the entry;
more lines are added below it. Markdown works. This comment is removed,
and an empty message creates no fragment.
-->
";

/// `text` without the editor instructions.
fn strip_editor_help(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let help = EDITOR_HELP.trim_start();
    match text.find(help) {
        Some(at) => format!("{}{}", &text[..at], &text[at + help.len()..]),
        None => text,
    }
}

fn ask_message(prompter: &mut dyn Prompter, editor: Option<&str>) -> Result<String> {
    if let Some(editor) = editor {
        loop {
            let message = strip_editor_help(&prompter.edit(editor, EDITOR_HELP)?);
            if !message.trim().is_empty() {
                return Ok(message);
            }
            if !prompter.confirm("The message is empty. Open the editor again?", "", true)? {
                return Err(prompt::cancelled());
            }
        }
    }
    let validate = |input: &str| {
        if input.trim().is_empty() {
            Err("describe the change; the message can't be empty".to_string())
        } else {
            Ok(())
        }
    };
    prompter.text("Message:", "", "One line for the changelog. For more lines, set $EDITOR or pass -m", &validate)
}

fn color_enabled(ctx: &Context) -> bool {
    match ctx.color {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
    }
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

fn validate_name(name: &str, kinds: &[RefKind]) -> Result<()> {
    if !fragment::is_valid_name(name) {
        return Err(Error::Usage(format!(
            "invalid fragment name `{name}`: use only letters, digits, `-` and `_` (e.g. `123`, `mr-45` or `oauth-login`)"
        )));
    }
    if !kinds.is_empty() && fragment::resolve_ref(name, kinds).is_none() && fragment::looks_like_ref(name) {
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

/// `$VISUAL` or `$EDITOR`, whichever is set first.
fn editor_from_env() -> Option<String> {
    ["VISUAL", "EDITOR"].iter().filter_map(|var| std::env::var(var).ok()).find(|v| !v.trim().is_empty())
}

/// The message from `$VISUAL`/`$EDITOR`, or from stdin when no editor is set
/// and stdin isn't a terminal.
fn read_message() -> Result<String> {
    if let Some(editor) = editor_from_env() {
        return edit_file(&editor, EDITOR_HELP).map(|text| strip_editor_help(&text));
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

/// Open `editor` on a temporary file containing `initial` and return what the
/// file contains once the editor exits.
fn edit_file(editor: &str, initial: &str) -> Result<String> {
    let path = create_temp_file()?;
    let result =
        std::fs::write(&path, initial).map_err(|e| Error::io(&path, e)).and_then(|()| run_editor(editor, &path));
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
    use std::path::Path;

    use super::prompt::{Answer, ScriptedPrompter};
    use super::*;
    use crate::config::{ChangeType, RepoKind};
    use crate::version::Bump;

    fn gitlab() -> Config {
        Config::default_for(Path::new("/repo"), "app").with_repo(RepoKind::Gitlab, "https://gitlab.com/acme/app")
    }

    fn args() -> NewArgs {
        NewArgs { r#type: None, name: None, refs: Vec::new(), message: None, breaking: false, package: None }
    }

    fn select(i: usize) -> Option<Answer> {
        Some(Answer::Select(i))
    }

    fn text(s: &str) -> Option<Answer> {
        Some(Answer::Text(s.to_string()))
    }

    fn confirm(yes: bool) -> Option<Answer> {
        Some(Answer::Confirm(yes))
    }

    fn edit(s: &str) -> Option<Answer> {
        Some(Answer::Edit(s.to_string()))
    }

    /// Run the questions with scripted answers; also return what was asked.
    fn run_ask(
        config: &Config,
        args: &NewArgs,
        editor: Option<&str>,
        answers: Vec<Option<Answer>>,
    ) -> (Result<Answers>, Vec<String>) {
        let type_name = args.r#type.as_deref().map(|t| resolve_type(config, t).unwrap());
        let mut prompter = ScriptedPrompter::new(answers);
        let result = ask(config, args, type_name, &mut prompter, editor);
        assert!(prompter.answers.is_empty(), "unused answers: {:?}", prompter.answers);
        (result, prompter.log)
    }

    fn answers(type_name: &str, name: Option<&str>, message: &str, breaking: bool) -> Answers {
        Answers {
            type_name: type_name.to_string(),
            name: name.map(str::to_string),
            message: message.to_string(),
            breaking,
        }
    }

    fn is_cancelled(result: &Result<Answers>) -> bool {
        matches!(result, Err(Error::Failed(m)) if m == "cancelled; no fragment created")
    }

    #[test]
    fn asks_for_everything_in_order() {
        let (result, log) = run_ask(
            &gitlab(),
            &args(),
            None,
            vec![select(4), text("mr-12"), text("Fix the crash on start."), confirm(true)],
        );
        assert_eq!(result.unwrap(), answers("fixed", Some("mr-12"), "Fix the crash on start.", true));
        assert_eq!(
            log,
            [
                "options: Added — new features | Changed — changes in existing functionality | \
                 Deprecated — soon-to-be removed features | Removed — now removed features | \
                 Fixed — bug fixes | Security — vulnerabilities",
                "select: Change type:",
                "placeholder: e.g. 123 or mr-45",
                "help: 123 → #123, mr-45 → !45; other text is a slug; empty: a slug from the message",
                "text: Issue or merge request (optional):",
                "placeholder: ",
                "help: One line for the changelog. For more lines, set $EDITOR or pass -m",
                "text: Message:",
                "confirm: Breaking change? (default false)",
            ]
        );
    }

    #[test]
    fn empty_reference_means_a_slug_from_the_message() {
        let (result, _) =
            run_ask(&gitlab(), &args(), None, vec![select(0), text("  "), text("Add it."), confirm(false)]);
        assert_eq!(result.unwrap(), answers("added", None, "Add it.", false));
    }

    #[test]
    fn invalid_references_are_asked_again() {
        let (result, log) = run_ask(
            &gitlab(),
            &args(),
            None,
            vec![select(0), text("pr-3"), text("bad name!"), text(" 42 "), text("Add it."), confirm(false)],
        );
        assert_eq!(result.unwrap(), answers("added", Some("42"), "Add it.", false));
        let invalid: Vec<&String> = log.iter().filter(|l| l.starts_with("invalid: ")).collect();
        assert_eq!(invalid.len(), 2, "{log:?}");
        assert!(invalid[0].contains("`pr-3` looks like a reference but matches no reference kind"), "{log:?}");
        assert!(invalid[1].contains("invalid fragment name `bad name!`"), "{log:?}");
    }

    #[test]
    fn reference_examples_follow_the_configured_kinds() {
        let github = Config::default_for(Path::new("/repo"), "app")
            .with_repo(RepoKind::Github, "https://github.com/example/app");
        let (_, log) = run_ask(&github, &args(), None, vec![select(0), text(""), text("Add it."), confirm(false)]);
        assert!(log.contains(&"text: Issue or pull request (optional):".to_string()), "{log:?}");
        assert!(log.contains(&"placeholder: e.g. 123 or pr-45".to_string()), "{log:?}");

        // Without reference kinds, any valid name is a slug, even `123`.
        let plain = Config::default_for(Path::new("/repo"), "app");
        let (result, log) =
            run_ask(&plain, &args(), None, vec![select(0), text("no!"), text("123"), text("Add it."), confirm(false)]);
        assert_eq!(result.unwrap(), answers("added", Some("123"), "Add it.", false));
        assert!(log.contains(&"text: Name (optional):".to_string()), "{log:?}");
        assert!(log.contains(&"placeholder: e.g. oauth-login".to_string()), "{log:?}");
    }

    #[test]
    fn custom_types_show_their_heading() {
        let mut config = gitlab();
        config.types.push(ChangeType {
            name: "perf".to_string(),
            heading: "Performance".to_string(),
            aliases: Vec::new(),
            bump: Bump::Patch,
            hidden: false,
        });
        let (result, log) = run_ask(&config, &args(), None, vec![select(6), text(""), text("Faster."), confirm(false)]);
        assert_eq!(result.unwrap().type_name, "perf");
        assert!(log[0].ends_with("| Security — vulnerabilities | Performance"), "{log:?}");
    }

    #[test]
    fn empty_messages_are_asked_again() {
        let (result, log) = run_ask(
            &gitlab(),
            &args(),
            None,
            vec![select(0), text(""), text(""), text("   "), text("Add it."), confirm(false)],
        );
        assert_eq!(result.unwrap(), answers("added", None, "Add it.", false));
        assert_eq!(log.iter().filter(|l| l.starts_with("invalid: describe the change")).count(), 2, "{log:?}");
    }

    #[test]
    fn only_asks_for_what_is_missing() {
        let given = NewArgs {
            r#type: Some("fix".to_string()),
            name: Some("12".to_string()),
            message: Some("Fix it.".to_string()),
            ..args()
        };
        let (result, log) = run_ask(&gitlab(), &given, None, vec![confirm(false)]);
        assert_eq!(result.unwrap(), answers("fixed", Some("12"), "Fix it.", false));
        assert_eq!(log, ["confirm: Breaking change? (default false)"]);

        // With --breaking there is nothing left to ask.
        let (result, log) = run_ask(&gitlab(), &NewArgs { breaking: true, ..given }, Some("vim"), vec![]);
        assert_eq!(result.unwrap(), answers("fixed", Some("12"), "Fix it.", true));
        assert!(log.is_empty(), "{log:?}");
    }

    #[test]
    fn the_editor_is_used_when_set() {
        let (result, log) = run_ask(
            &gitlab(),
            &args(),
            Some("vim"),
            vec![select(0), text(""), edit("Add it.\n\nMore."), confirm(false)],
        );
        assert_eq!(result.unwrap(), answers("added", None, "Add it.\n\nMore.\n", false));
        assert!(log.contains(&"edit: vim".to_string()), "{log:?}");
        assert!(!log.contains(&"text: Message:".to_string()), "{log:?}");
    }

    #[test]
    fn an_empty_editor_message_offers_to_edit_again() {
        let (result, log) = run_ask(
            &gitlab(),
            &args(),
            Some("vim"),
            vec![
                select(0),
                text(""),
                edit(""),
                confirm(true),
                edit(" \n"),
                confirm(true),
                edit("Add it."),
                confirm(false),
            ],
        );
        assert_eq!(result.unwrap(), answers("added", None, "Add it.\n", false));
        assert_eq!(log.iter().filter(|l| l.starts_with("edit: ")).count(), 3, "{log:?}");

        let (result, _) = run_ask(&gitlab(), &args(), Some("vim"), vec![select(0), text(""), edit(""), confirm(false)]);
        assert!(is_cancelled(&result), "{result:?}");
    }

    #[test]
    fn cancelling_any_question_cancels() {
        let scripts = [
            vec![None],
            vec![select(0), None],
            vec![select(0), text("pr-3"), None],
            vec![select(0), text(""), None],
            vec![select(0), text(""), text(""), None],
            vec![select(0), text(""), text("Add it."), None],
        ];
        for script in scripts {
            let (result, log) = run_ask(&gitlab(), &args(), None, script);
            assert!(is_cancelled(&result), "{result:?} {log:?}");
        }
        let (result, _) = run_ask(&gitlab(), &args(), Some("vim"), vec![select(0), text(""), None]);
        assert!(is_cancelled(&result), "{result:?}");
    }

    #[test]
    fn interactive_only_at_a_terminal_without_json() {
        assert!(is_interactive(false, true, true));
        assert!(!is_interactive(true, true, true));
        assert!(!is_interactive(false, false, true));
        assert!(!is_interactive(false, true, false));
    }

    #[test]
    fn editor_instructions_are_removed() {
        assert_eq!(strip_editor_help(&format!("Add it.{EDITOR_HELP}")), "Add it.\n");
        assert_eq!(strip_editor_help(&format!("Add it.\r\n{}", EDITOR_HELP.replace('\n', "\r\n"))), "Add it.\n\n");
        assert_eq!(strip_editor_help(EDITOR_HELP).trim(), "");
        // A changed comment is the user's text and stays.
        assert_eq!(strip_editor_help("Add it.\n<!-- note -->\n"), "Add it.\n<!-- note -->\n");
    }

    #[test]
    fn join_with_or() {
        assert_eq!(join_or(&[]), "");
        assert_eq!(join_or(&["a"]), "a");
        assert_eq!(join_or(&["a", "b"]), "a or b");
        assert_eq!(join_or(&["a", "b", "c"]), "a, b or c");
    }

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
