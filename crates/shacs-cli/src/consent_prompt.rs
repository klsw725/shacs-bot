//! The question before `permissions auto-mode reset` and `setup` write the
//! config (Spec 048 PRD 050 scopes 5 and 7): only `y` or `yes` writes, and
//! input that is not a terminal never does.

use std::io::{self, BufRead, IsTerminal, Write};

pub(crate) trait Prompt {
    /// Shows what the command would write, before the question.
    fn show(&mut self, text: &str);
    /// The answer, or `None` when standard input is not a terminal.
    fn answer(&mut self, question: &str) -> Option<String>;
}

pub(crate) struct StdPrompt;

impl Prompt for StdPrompt {
    fn show(&mut self, text: &str) {
        println!("{text}");
    }

    fn answer(&mut self, question: &str) -> Option<String> {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            return None;
        }
        print!("{question} ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        stdin.lock().read_line(&mut line).ok()?;
        Some(line)
    }
}

#[cfg(test)]
pub(crate) struct ScriptedPrompt {
    pub(crate) shown: Vec<String>,
    pub(crate) answer: Option<String>,
    pub(crate) before_answer: Option<Box<dyn FnOnce()>>,
}

#[cfg(test)]
impl ScriptedPrompt {
    pub(crate) fn answering(answer: Option<&str>) -> Self {
        Self {
            shown: Vec::new(),
            answer: answer.map(str::to_owned),
            before_answer: None,
        }
    }
}

#[cfg(test)]
impl Prompt for ScriptedPrompt {
    fn show(&mut self, text: &str) {
        self.shown.push(text.to_owned());
    }

    fn answer(&mut self, _question: &str) -> Option<String> {
        if let Some(hook) = self.before_answer.take() {
            hook();
        }
        self.answer.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Consent {
    Given,
    Refused,
    NoTerminal,
}

impl Consent {
    pub(crate) fn asked(prompt: &mut dyn Prompt, question: &str) -> Self {
        match prompt.answer(question) {
            None => Self::NoTerminal,
            Some(answer) if matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") => {
                Self::Given
            }
            Some(_) => Self::Refused,
        }
    }

    /// Why nothing was written, or `None` when the user agreed.
    pub(crate) fn unwritten(self) -> Option<&'static str> {
        match self {
            Self::Given => None,
            Self::Refused => Some("Nothing was written."),
            Self::NoTerminal => Some(
                "Standard input is not a terminal, so nothing was written. Run it again with --yes to write.",
            ),
        }
    }
}
