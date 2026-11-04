//! Splits a shell command into words and operators without running a shell
//! (Spec 048 PRD 000 scope 6). Syntax this reader does not follow is reported
//! as unanalyzable instead of guessed.

pub(crate) const MAX_COMMAND_CHARS: usize = 10_000;

/// One shell word after quote and escape removal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ShellWord {
    /// The word as the shell would pass it, with parameters left as written.
    pub(crate) text: String,
    /// The word with every parameter expansion replaced by nothing.
    pub(crate) without_parameters: String,
    pub(crate) quoted: bool,
    pub(crate) unquoted_glob: bool,
    pub(crate) has_parameter: bool,
    pub(crate) has_positional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Separator {
    And,
    Or,
    Semicolon,
    Pipe,
    Background,
    Newline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RedirectKind {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShellToken {
    Word(ShellWord),
    Separator(Separator),
    /// A redirection to a file. Descriptor duplication such as `2>&1` has no target.
    Redirect {
        kind: RedirectKind,
        target: Option<ShellWord>,
    },
    HereDoc {
        body: String,
    },
    HereString {
        word: ShellWord,
    },
}

/// The command uses shell syntax this reader does not follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unanalyzable;

pub(crate) fn tokenize(command: &str) -> Result<Vec<ShellToken>, Unanalyzable> {
    if command.chars().count() > MAX_COMMAND_CHARS {
        return Err(Unanalyzable);
    }
    let mut reader = Reader {
        chars: command.chars().collect(),
        index: 0,
        tokens: Vec::new(),
        pending: Vec::new(),
    };
    reader.run()?;
    check_separators(&reader.tokens)?;
    Ok(reader.tokens)
}

struct PendingHereDoc {
    delimiter: String,
    strip_tabs: bool,
    expands: bool,
    token_index: usize,
}

struct Reader {
    chars: Vec<char>,
    index: usize,
    tokens: Vec<ShellToken>,
    pending: Vec<PendingHereDoc>,
}

impl Reader {
    fn run(&mut self) -> Result<(), Unanalyzable> {
        while let Some(character) = self.peek(0) {
            match character {
                ' ' | '\t' | '\r' => self.index += 1,
                '\n' => {
                    self.index += 1;
                    self.tokens.push(ShellToken::Separator(Separator::Newline));
                    self.read_here_doc_bodies()?;
                }
                '#' => self.skip_comment(),
                '&' | '|' | ';' => self.read_control()?,
                '(' | ')' => return Err(Unanalyzable),
                '<' | '>' => self.read_redirect()?,
                digit if digit.is_ascii_digit() && self.is_descriptor_redirect() => {
                    while self.peek(0).is_some_and(|next| next.is_ascii_digit()) {
                        self.index += 1;
                    }
                    self.read_redirect()?;
                }
                _ => {
                    let word = self.read_word()?;
                    if !word.quoted && matches!(word.text.as_str(), "{" | "}") {
                        return Err(Unanalyzable);
                    }
                    if word.quoted || !word.text.is_empty() {
                        self.tokens.push(ShellToken::Word(word));
                    }
                }
            }
        }
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err(Unanalyzable)
        }
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.index + offset).copied()
    }

    fn next_char(&mut self) -> Option<char> {
        let character = self.peek(0)?;
        self.index += 1;
        Some(character)
    }

    fn skip_comment(&mut self) {
        while self.peek(0).is_some_and(|character| character != '\n') {
            self.index += 1;
        }
    }

    fn is_descriptor_redirect(&self) -> bool {
        let mut offset = 0;
        while self
            .peek(offset)
            .is_some_and(|character| character.is_ascii_digit())
        {
            offset += 1;
        }
        matches!(self.peek(offset), Some('<' | '>'))
    }

    fn read_control(&mut self) -> Result<(), Unanalyzable> {
        let separator = match (self.peek(0), self.peek(1)) {
            (Some('&'), Some('&')) => (Separator::And, 2),
            (Some('&'), Some('>')) => {
                self.index += if self.peek(2) == Some('>') { 3 } else { 2 };
                let target = self.read_target_word()?;
                self.push_redirect(RedirectKind::Output, Some(target));
                return Ok(());
            }
            (Some('&'), _) => (Separator::Background, 1),
            (Some('|'), Some('|')) => (Separator::Or, 2),
            (Some('|'), Some('&')) => (Separator::Pipe, 2),
            (Some('|'), _) => (Separator::Pipe, 1),
            (Some(';'), Some(';' | '&')) => return Err(Unanalyzable),
            _ => (Separator::Semicolon, 1),
        };
        self.index += separator.1;
        self.tokens.push(ShellToken::Separator(separator.0));
        Ok(())
    }

    fn read_redirect(&mut self) -> Result<(), Unanalyzable> {
        let operator = self.next_char();
        let following = self.peek(0);
        match (operator, following) {
            (_, Some('(')) => Err(Unanalyzable),
            (Some('<'), Some('<')) if self.peek(1) == Some('<') => {
                self.index += 2;
                let word = self.read_target_word()?;
                self.tokens.push(ShellToken::HereString { word });
                Ok(())
            }
            (Some('<'), Some('<')) => {
                self.index += 1;
                let strip_tabs = self.peek(0) == Some('-');
                if strip_tabs {
                    self.index += 1;
                }
                let delimiter = self.read_target_word()?;
                self.pending.push(PendingHereDoc {
                    delimiter: delimiter.text,
                    strip_tabs,
                    expands: !delimiter.quoted,
                    token_index: self.tokens.len(),
                });
                self.tokens.push(ShellToken::HereDoc {
                    body: String::new(),
                });
                Ok(())
            }
            (Some(direction), Some('&')) => {
                self.index += 1;
                let target = self.read_target_word()?;
                let duplicate =
                    target.text == "-" || target.text.chars().all(|c| c.is_ascii_digit());
                self.push_redirect(redirect_kind(direction), (!duplicate).then_some(target));
                Ok(())
            }
            (Some('<'), Some('>')) | (Some('>'), Some('>' | '|')) => {
                self.index += 1;
                let target = self.read_target_word()?;
                self.push_redirect(RedirectKind::Output, Some(target));
                Ok(())
            }
            (Some(direction), _) => {
                let target = self.read_target_word()?;
                self.push_redirect(redirect_kind(direction), Some(target));
                Ok(())
            }
            (None, _) => Err(Unanalyzable),
        }
    }

    fn push_redirect(&mut self, kind: RedirectKind, target: Option<ShellWord>) {
        self.tokens.push(ShellToken::Redirect { kind, target });
    }

    fn read_target_word(&mut self) -> Result<ShellWord, Unanalyzable> {
        while matches!(self.peek(0), Some(' ' | '\t')) {
            self.index += 1;
        }
        match self.peek(0) {
            None | Some('\n' | '&' | '|' | ';' | '<' | '>' | '(' | ')') => Err(Unanalyzable),
            Some(_) => self.read_word(),
        }
    }

    fn read_word(&mut self) -> Result<ShellWord, Unanalyzable> {
        let mut word = ShellWord::default();
        while let Some(character) = self.peek(0) {
            match character {
                ' ' | '\t' | '\r' | '\n' | '&' | '|' | ';' | '<' | '>' => break,
                '(' | ')' | '`' => return Err(Unanalyzable),
                '\'' => {
                    self.index += 1;
                    word.quoted = true;
                    loop {
                        match self.next_char() {
                            None => return Err(Unanalyzable),
                            Some('\'') => break,
                            Some(literal) => push_literal(&mut word, literal),
                        }
                    }
                }
                '"' => {
                    self.index += 1;
                    word.quoted = true;
                    self.read_double_quoted(&mut word)?;
                }
                '\\' => {
                    self.index += 1;
                    match self.next_char() {
                        None => return Err(Unanalyzable),
                        Some('\n') => {}
                        Some(literal) => push_literal(&mut word, literal),
                    }
                }
                '$' => self.read_parameter(&mut word)?,
                '*' | '?' | '[' | '{' => {
                    self.index += 1;
                    word.unquoted_glob = true;
                    push_literal(&mut word, character);
                }
                literal => {
                    self.index += 1;
                    push_literal(&mut word, literal);
                }
            }
        }
        Ok(word)
    }

    fn read_double_quoted(&mut self, word: &mut ShellWord) -> Result<(), Unanalyzable> {
        loop {
            match self.peek(0) {
                None | Some('`') => return Err(Unanalyzable),
                Some('"') => {
                    self.index += 1;
                    return Ok(());
                }
                Some('\\') => {
                    self.index += 1;
                    match self.peek(0) {
                        Some(escaped @ ('$' | '`' | '"' | '\\')) => {
                            self.index += 1;
                            push_literal(word, escaped);
                        }
                        Some('\n') => self.index += 1,
                        _ => push_literal(word, '\\'),
                    }
                }
                Some('$') => self.read_parameter(word)?,
                Some(literal) => {
                    self.index += 1;
                    push_literal(word, literal);
                }
            }
        }
    }

    fn read_parameter(&mut self, word: &mut ShellWord) -> Result<(), Unanalyzable> {
        let start = self.index;
        let (length, positional) = match self.peek(1) {
            Some('(' | '\'' | '"') => return Err(Unanalyzable),
            Some('{') => {
                let close = (self.index + 2..self.chars.len())
                    .find(|position| self.chars[*position] == '}')
                    .ok_or(Unanalyzable)?;
                let first = self.chars.get(self.index + 2).copied();
                (close + 1 - start, first.is_some_and(is_positional_start))
            }
            Some(first) if first.is_ascii_alphabetic() || first == '_' => {
                let name_length = self.chars[start + 1..]
                    .iter()
                    .take_while(|character| character.is_ascii_alphanumeric() || **character == '_')
                    .count();
                (name_length + 1, false)
            }
            Some(first) if first.is_ascii_digit() || matches!(first, '@' | '*') => {
                (2, is_positional_start(first))
            }
            Some('#' | '?' | '$' | '!' | '-') => (2, false),
            _ => {
                self.index += 1;
                push_literal(word, '$');
                return Ok(());
            }
        };
        word.text.extend(&self.chars[start..start + length]);
        word.has_parameter = true;
        word.has_positional |= positional;
        self.index = start + length;
        Ok(())
    }

    fn read_here_doc_bodies(&mut self) -> Result<(), Unanalyzable> {
        for pending in std::mem::take(&mut self.pending) {
            let mut body = String::new();
            loop {
                if self.index >= self.chars.len() {
                    return Err(Unanalyzable);
                }
                let line_end = (self.index..self.chars.len())
                    .find(|position| self.chars[*position] == '\n')
                    .unwrap_or(self.chars.len());
                let line = self.chars[self.index..line_end].iter().collect::<String>();
                self.index = (line_end + 1).min(self.chars.len());
                let compared = if pending.strip_tabs {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if compared == pending.delimiter {
                    break;
                }
                body.push_str(&line);
                body.push('\n');
            }
            if pending.expands && (body.contains("$(") || body.contains('`')) {
                return Err(Unanalyzable);
            }
            self.tokens[pending.token_index] = ShellToken::HereDoc { body };
        }
        Ok(())
    }
}

fn push_literal(word: &mut ShellWord, character: char) {
    word.text.push(character);
    word.without_parameters.push(character);
}

fn is_positional_start(character: char) -> bool {
    matches!(character, '1'..='9' | '@' | '*')
}

fn redirect_kind(direction: char) -> RedirectKind {
    if direction == '<' {
        RedirectKind::Input
    } else {
        RedirectKind::Output
    }
}

/// Rejects an operator with no command on one of its sides, such as a
/// trailing `&&` or `; ;`. A line break after `&&`, `||` or `|` continues the
/// command as in a shell.
fn check_separators(tokens: &[ShellToken]) -> Result<(), Unanalyzable> {
    let mut expecting_command = false;
    let mut segment_empty = true;
    for token in tokens {
        match token {
            ShellToken::Separator(Separator::Newline) => {
                if !expecting_command {
                    segment_empty = true;
                }
            }
            ShellToken::Separator(Separator::And | Separator::Or | Separator::Pipe) => {
                if expecting_command || segment_empty {
                    return Err(Unanalyzable);
                }
                expecting_command = true;
            }
            ShellToken::Separator(Separator::Semicolon | Separator::Background) => {
                if expecting_command || segment_empty {
                    return Err(Unanalyzable);
                }
                segment_empty = true;
            }
            ShellToken::Word(_)
            | ShellToken::Redirect { .. }
            | ShellToken::HereDoc { .. }
            | ShellToken::HereString { .. } => {
                expecting_command = false;
                segment_empty = false;
            }
        }
    }
    if expecting_command {
        Err(Unanalyzable)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "shell_token_tests.rs"]
mod tests;
