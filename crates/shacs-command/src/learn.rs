use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnCommandArgs {
    Propose,
    List,
    Show(String),
    ShowVersion {
        method_id: String,
        version: u32,
    },
    /// `/learn confirm <id>`: shows what confirming means and the code that
    /// confirms exactly the version shown.
    Review(String),
    Confirm {
        method_id: String,
        version: u32,
        code: String,
    },
    Reject {
        method_id: String,
        version: u32,
    },
    Verify(String),
    /// The request, as typed.
    Match(String),
    /// A new version from `version`, with `part` set to the text as typed,
    /// or removed when there is no text.
    Edit {
        method_id: String,
        version: u32,
        part: MethodPart,
        text: Option<String>,
    },
    Deactivate {
        method_id: String,
        version: u32,
    },
    Restore {
        method_id: String,
        version: u32,
    },
    Invalid,
}

/// A part of a method version the user can edit; premises and steps are
/// numbered from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodPart {
    Purpose,
    AppliesTo,
    Premise(usize),
    Step(usize),
}

pub(super) fn parse_learn_args(args: &str) -> LearnCommandArgs {
    let trimmed = args.trim();
    let (head, rest) = word(trimmed);
    if head.eq_ignore_ascii_case("match") {
        return if rest.is_empty() {
            LearnCommandArgs::Invalid
        } else {
            LearnCommandArgs::Match(rest.to_owned())
        };
    }
    if head.eq_ignore_ascii_case("edit") {
        return parse_edit(rest);
    }
    let normalized = trimmed.to_ascii_lowercase();
    let words: Vec<_> = normalized.split_whitespace().collect();
    let versioned = |id: &str, version: &str, made: fn(String, u32) -> LearnCommandArgs| {
        version_number(version).map_or(LearnCommandArgs::Invalid, |version| {
            made(id.to_owned(), version)
        })
    };
    match words.as_slice() {
        [] => LearnCommandArgs::Propose,
        ["list"] => LearnCommandArgs::List,
        ["show", id] => LearnCommandArgs::Show((*id).to_owned()),
        ["show", id, version] => versioned(id, version, |method_id, version| {
            LearnCommandArgs::ShowVersion { method_id, version }
        }),
        ["confirm", id] => LearnCommandArgs::Review((*id).to_owned()),
        ["confirm", id, version, code] => match version_number(version) {
            Some(version) => LearnCommandArgs::Confirm {
                method_id: (*id).to_owned(),
                version,
                code: (*code).to_owned(),
            },
            None => LearnCommandArgs::Invalid,
        },
        ["reject", id, version] => versioned(id, version, |method_id, version| {
            LearnCommandArgs::Reject { method_id, version }
        }),
        ["deactivate", id, version] => versioned(id, version, |method_id, version| {
            LearnCommandArgs::Deactivate { method_id, version }
        }),
        ["restore", id, version] => versioned(id, version, |method_id, version| {
            LearnCommandArgs::Restore { method_id, version }
        }),
        ["verify", id] => LearnCommandArgs::Verify((*id).to_owned()),
        _ => LearnCommandArgs::Invalid,
    }
}

/// `edit <id> v<N> purpose|applies <text>`, `edit <id> v<N> premise|step <k>
/// <text>`, or `edit <id> v<N> remove premise|step <k>`.
fn parse_edit(args: &str) -> LearnCommandArgs {
    let (id, rest) = word(args);
    let (version, rest) = word(rest);
    let (part, rest) = word(rest);
    let Some(version) = version_number(&version.to_ascii_lowercase()) else {
        return LearnCommandArgs::Invalid;
    };
    let edit = |part: MethodPart, text: Option<&str>| LearnCommandArgs::Edit {
        method_id: id.to_ascii_lowercase(),
        version,
        part,
        text: text.map(str::to_owned),
    };
    let numbered = |kind: &str, rest: &str| -> Option<(MethodPart, String)> {
        let (number, text) = word(rest);
        let number = number.parse().ok().filter(|number| *number > 0)?;
        match kind.to_ascii_lowercase().as_str() {
            "premise" => Some((MethodPart::Premise(number), text.to_owned())),
            "step" => Some((MethodPart::Step(number), text.to_owned())),
            _ => None,
        }
    };
    let set = |part: MethodPart, text: &str| {
        if text.is_empty() {
            LearnCommandArgs::Invalid
        } else {
            edit(part, Some(text))
        }
    };
    match part.to_ascii_lowercase().as_str() {
        "purpose" => set(MethodPart::Purpose, rest),
        "applies" => set(MethodPart::AppliesTo, rest),
        "remove" => {
            let (kind, rest) = word(rest);
            match numbered(kind, rest) {
                Some((part, tail)) if tail.is_empty() => edit(part, None),
                _ => LearnCommandArgs::Invalid,
            }
        }
        kind => {
            numbered(kind, rest).map_or(LearnCommandArgs::Invalid, |(part, text)| set(part, &text))
        }
    }
}

/// The first word of `text` and the rest, both trimmed.
fn word(text: &str) -> (&str, &str) {
    let text = text.trim();
    text.split_once(char::is_whitespace)
        .map_or((text, ""), |(head, rest)| (head, rest.trim()))
}

/// `v1`, `v2`, … as shown next to a method.
fn version_number(word: &str) -> Option<u32> {
    word.strip_prefix('v')
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
}
