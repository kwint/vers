//! Prompts for interactive `vers new`.
//!
//! The questions go through [`Prompter`] so the flow in `new.rs` can be tested
//! with scripted answers; [`TerminalPrompter`] asks them on stderr.

use inquire::ui::RenderConfig;
use inquire::validator::Validation;
use inquire::{Confirm, InquireError, Select, Text};

use crate::error::{Error, Result};

/// Checks a text answer. `Err` holds the message shown before asking again.
pub type Validator<'a> = &'a dyn Fn(&str) -> std::result::Result<(), String>;

/// Asks the user questions. Every method returns [`cancelled`] when the user
/// cancels (Ctrl-C or Esc).
pub trait Prompter {
    /// Choose one of `options` and return its index.
    fn select(&mut self, prompt: &str, options: &[String], help: &str) -> Result<usize>;

    /// Ask for a line of text until `validate` accepts it.
    fn text(&mut self, prompt: &str, placeholder: &str, help: &str, validate: Validator<'_>) -> Result<String>;

    /// Ask a yes/no question.
    fn confirm(&mut self, prompt: &str, help: &str, default: bool) -> Result<bool>;

    /// Open `editor` on a file containing `template` and return the file's
    /// contents once the editor exits.
    fn edit(&mut self, editor: &str, template: &str) -> Result<String>;
}

/// The error for a cancelled prompt.
pub fn cancelled() -> Error {
    Error::Failed("cancelled; no fragment created".to_string())
}

/// Asks on the terminal (stderr) and reads keys from it.
pub struct TerminalPrompter {
    render: RenderConfig<'static>,
}

impl TerminalPrompter {
    pub fn new(color: bool) -> TerminalPrompter {
        let render = if color { RenderConfig::default_colored() } else { RenderConfig::empty() };
        TerminalPrompter { render }
    }
}

impl Prompter for TerminalPrompter {
    fn select(&mut self, prompt: &str, options: &[String], help: &str) -> Result<usize> {
        Select::new(prompt, options.to_vec())
            .with_help_message(help)
            .with_page_size(options.len().max(1))
            .with_render_config(self.render)
            .raw_prompt()
            .map(|chosen| chosen.index)
            .map_err(prompt_error)
    }

    fn text(&mut self, prompt: &str, placeholder: &str, help: &str, validate: Validator<'_>) -> Result<String> {
        let validator = move |input: &str| {
            Ok(match validate(input) {
                Ok(()) => Validation::Valid,
                Err(message) => Validation::Invalid(message.into()),
            })
        };
        let mut text = Text::new(prompt).with_validator(validator).with_render_config(self.render);
        if !placeholder.is_empty() {
            text = text.with_placeholder(placeholder);
        }
        if !help.is_empty() {
            text = text.with_help_message(help);
        }
        text.prompt().map_err(prompt_error)
    }

    fn confirm(&mut self, prompt: &str, help: &str, default: bool) -> Result<bool> {
        let mut confirm = Confirm::new(prompt).with_default(default).with_render_config(self.render);
        if !help.is_empty() {
            confirm = confirm.with_help_message(help);
        }
        confirm.prompt().map_err(prompt_error)
    }

    fn edit(&mut self, editor: &str, template: &str) -> Result<String> {
        super::edit_file(editor, template)
    }
}

fn prompt_error(e: InquireError) -> Error {
    match e {
        InquireError::OperationCanceled | InquireError::OperationInterrupted => cancelled(),
        InquireError::NotTTY => Error::Usage(
            "can't ask for the missing arguments: the terminal isn't interactive; pass TYPE and -m <MESSAGE>"
                .to_string(),
        ),
        other => Error::Failed(format!("can't read the answer from the terminal: {other}")),
    }
}

/// Answers given in advance, for tests. Each answer is consumed by the next
/// question of its kind; a question without an answer fails the test.
#[cfg(test)]
#[derive(Default)]
pub struct ScriptedPrompter {
    /// Answers in order. `None` cancels at that question.
    pub answers: std::collections::VecDeque<Option<Answer>>,
    /// Every question asked, as `kind: prompt`, plus validation errors shown.
    pub log: Vec<String>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub enum Answer {
    /// The index of the chosen option.
    Select(usize),
    Text(String),
    Confirm(bool),
    /// What the user leaves in the editor's file.
    Edit(String),
}

#[cfg(test)]
impl ScriptedPrompter {
    pub fn new(answers: impl IntoIterator<Item = Option<Answer>>) -> ScriptedPrompter {
        ScriptedPrompter { answers: answers.into_iter().collect(), log: Vec::new() }
    }

    fn next(&mut self, kind: &str, prompt: &str) -> Result<Answer> {
        self.log.push(format!("{kind}: {prompt}"));
        match self.answers.pop_front() {
            Some(Some(answer)) => Ok(answer),
            Some(None) => Err(cancelled()),
            None => panic!("no scripted answer for {kind} `{prompt}`; asked so far: {:?}", self.log),
        }
    }
}

#[cfg(test)]
impl Prompter for ScriptedPrompter {
    fn select(&mut self, prompt: &str, options: &[String], _help: &str) -> Result<usize> {
        self.log.push(format!("options: {}", options.join(" | ")));
        match self.next("select", prompt)? {
            Answer::Select(i) if i < options.len() => Ok(i),
            other => panic!("expected a select answer for `{prompt}`, got {other:?}"),
        }
    }

    fn text(&mut self, prompt: &str, placeholder: &str, help: &str, validate: Validator<'_>) -> Result<String> {
        self.log.push(format!("placeholder: {placeholder}"));
        self.log.push(format!("help: {help}"));
        // Like the terminal, ask again until the answer is valid.
        loop {
            match self.next("text", prompt)? {
                Answer::Text(text) => match validate(&text) {
                    Ok(()) => return Ok(text),
                    Err(message) => self.log.push(format!("invalid: {message}")),
                },
                other => panic!("expected a text answer for `{prompt}`, got {other:?}"),
            }
        }
    }

    fn confirm(&mut self, prompt: &str, _help: &str, default: bool) -> Result<bool> {
        match self.next("confirm", &format!("{prompt} (default {default})"))? {
            Answer::Confirm(yes) => Ok(yes),
            other => panic!("expected a confirm answer for `{prompt}`, got {other:?}"),
        }
    }

    fn edit(&mut self, editor: &str, template: &str) -> Result<String> {
        match self.next("edit", editor)? {
            // The scripted text replaces the empty first line of the template,
            // keeping the instructions below it, as a user typing would.
            Answer::Edit(text) => Ok(format!("{text}{template}")),
            other => panic!("expected an edit answer for `{editor}`, got {other:?}"),
        }
    }
}
