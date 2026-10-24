use std::collections::HashSet;

/// Shortest run of characters that counts as copied text.
const COPY_CHARS: usize = 16;
const REQUEST_CHARS: usize = 32;
const MIN_QUOTE_CHARS: usize = 6;
const CLAUSE_BREAKS: [char; 8] = ['.', '!', '?', '。', ',', ';', ':', '\n'];

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Whether `quote` repeats `source` word for word, ignoring case, spacing and
/// a closing punctuation mark.
pub(super) fn quotes(quote: &str, source: &str) -> bool {
    let quote = normalize(quote);
    let quote = quote.trim_end_matches(['.', '!', '?', '。']).trim_end();
    quote.chars().count() >= MIN_QUOTE_CHARS && normalize(source).contains(quote)
}

/// How text the draft call was not shown is matched: 16-character runs of
/// any of it, and its clauses as whole words.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kept {
    /// The user's preference, one-task condition or mixed message: every
    /// clause counts, however short.
    Excluded,
    /// Anything else kept out (the user's other messages, messages not from
    /// the user, untrusted model output, tool output): clauses of two words
    /// or more.
    Hidden,
}

/// Text the generator was not shown. A draft that still repeats it copied it
/// from somewhere the runtime kept out, so that draft text is dropped.
pub(super) struct HiddenText {
    windows: HashSet<u64>,
    clauses: Vec<String>,
}

impl HiddenText {
    pub fn new<'a>(
        hidden: impl IntoIterator<Item = (Kept, &'a str)>,
        shown: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let shown: Vec<String> = shown.into_iter().map(normalize).collect();
        let shown_windows: HashSet<u64> = shown.iter().flat_map(|text| windows(text)).collect();
        let mut hidden_text = Self {
            windows: HashSet::new(),
            clauses: Vec::new(),
        };
        for (kept, text) in hidden {
            hidden_text.windows.extend(
                windows(&normalize(text))
                    .into_iter()
                    .filter(|window| !shown_windows.contains(window)),
            );
            for clause in text.split(CLAUSE_BREAKS).map(normalize) {
                let words = clause.split_whitespace().count();
                if (words >= 2 || (kept == Kept::Excluded && words == 1))
                    && !shown.iter().any(|text| contains_words(text, &clause))
                {
                    hidden_text.clauses.push(clause);
                }
            }
        }
        hidden_text
    }

    pub fn copied(&self, text: &str) -> bool {
        let text = normalize(text);
        windows(&text)
            .iter()
            .any(|window| self.windows.contains(window))
            || self
                .clauses
                .iter()
                .any(|clause| contains_words(&text, clause))
    }
}

/// Whether `needle` occurs in `haystack` as whole words.
fn contains_words(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(start, _)| {
        let before = haystack[..start].chars().next_back();
        let after = haystack[start + needle.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// The request tells the drafter what kind of task it was but is identified
/// only by digest, so a draft line may not copy it: 32 characters of a
/// request, any whole clause of it (a one-word or symbols-only request too),
/// or one of its specifics (a path, address, file name, number or
/// capitalised code), unless the user's feedback and procedures also say it.
pub(super) struct RequestText {
    windows: HashSet<u64>,
    clauses: Vec<String>,
    specifics: Vec<String>,
}

impl RequestText {
    pub fn new<'a>(
        requests: impl IntoIterator<Item = &'a str>,
        guidance: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let guidance: Vec<String> = guidance.into_iter().map(normalize).collect();
        let guidance_windows: HashSet<u64> = guidance
            .iter()
            .flat_map(|text| windows_of(text, REQUEST_CHARS))
            .collect();
        let mut request_text = Self {
            windows: HashSet::new(),
            clauses: Vec::new(),
            specifics: Vec::new(),
        };
        for request in requests {
            request_text.windows.extend(
                windows_of(&normalize(request), REQUEST_CHARS)
                    .into_iter()
                    .filter(|window| !guidance_windows.contains(window)),
            );
            request_text.clauses.extend(
                request
                    .split(CLAUSE_BREAKS)
                    .map(normalize)
                    .filter(|clause| !clause.is_empty())
                    .filter(|clause| !guidance.iter().any(|text| contains_words(text, clause))),
            );
            request_text.specifics.extend(
                request
                    .split_whitespace()
                    .map(|token| token.trim_matches(|c: char| ",;:!?()[]{}\"'`".contains(c)))
                    .filter(|token| specific(token))
                    .map(str::to_lowercase)
                    .filter(|token| !guidance.iter().any(|text| contains_words(text, token))),
            );
        }
        request_text
    }

    pub fn copied(&self, text: &str) -> bool {
        let text = normalize(text);
        windows_of(&text, REQUEST_CHARS)
            .iter()
            .any(|window| self.windows.contains(window))
            || self
                .clauses
                .iter()
                .any(|clause| contains_words(&text, clause))
            || self
                .specifics
                .iter()
                .any(|specific| text.contains(specific.as_str()))
    }
}

fn specific(token: &str) -> bool {
    let inner_dot = token
        .trim_end_matches('.')
        .chars()
        .skip(1)
        .any(|c| c == '.');
    token.chars().count() >= 2
        && (token.contains(['/', '\\', '@'])
            || token.chars().any(|c| c.is_ascii_digit())
            || inner_dot
            || (token.chars().count() >= 4
                && token.chars().filter(char::is_ascii_uppercase).count() >= 2))
}

fn windows(text: &str) -> Vec<u64> {
    windows_of(text, COPY_CHARS)
}

/// FNV-1a hashes of every run of `size` characters; a collision only drops a
/// draft line.
fn windows_of(text: &str, size: usize) -> Vec<u64> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .windows(size)
        .map(|window| {
            window
                .iter()
                .fold(0xcbf2_9ce4_8422_2325, |hash, character| {
                    (hash ^ u64::from(u32::from(*character))).wrapping_mul(0x0000_0100_0000_01b3)
                })
        })
        .collect()
}
