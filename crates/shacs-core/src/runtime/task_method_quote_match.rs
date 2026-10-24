//! Whether a report line copies a file word for word. Only whole sentences or
//! whole lines count: a fragment can drop the words that change its meaning
//! ("blue in the photo, not in the image"), or a digit or a sign ("10" in
//! "100%", "10" in "-10").

#[cfg(test)]
#[path = "task_method_quote_match_tests.rs"]
mod tests;

const OPENERS: &[char] = &['"', '\'', '“', '‘', '(', '[', '«', '「', '『'];
const CLOSERS: &[char] = &['"', '\'', '”', '’', ')', ']', '»', '」', '』'];
const ENDS: &[char] = &['.', '?', '!', '。', '？', '！'];

/// A file's words with single spaces, and the byte offsets where a block (a
/// paragraph, list item, heading, quote or table row) starts.
pub(crate) struct Folded {
    text: String,
    blocks: Vec<usize>,
}

impl Folded {
    pub(crate) fn new(file: &str) -> Self {
        let mut text = String::new();
        let mut blocks = Vec::new();
        let mut boundary = true;
        for line in file.lines() {
            let words: Vec<&str> = line.split_whitespace().collect();
            let Some(first) = words.first() else {
                boundary = true;
                continue;
            };
            if !text.is_empty() {
                text.push(' ');
            }
            if boundary || marker(first) || first.starts_with('|') {
                blocks.push(text.len());
            }
            text.push_str(&words.join(" "));
            // A heading or a table row is a block of one line.
            boundary = first.starts_with('#') || first.starts_with('|');
        }
        Self { text, blocks }
    }

    /// Whether `line` (a report line, see [`claimed`]) is whole sentences or
    /// lines of this file, word for word with runs of white space as one space:
    /// it starts where a block or a sentence starts and ends where a sentence or
    /// a block ends. Only a final `.` may be left out, not a `?` or `!`.
    pub(crate) fn quotes(&self, line: &str) -> bool {
        let quote = claimed(line)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        !quote.is_empty()
            && self
                .text
                .match_indices(&quote)
                .any(|(start, _)| self.begins(start) && self.ends(start + quote.len(), &quote))
    }

    fn begins(&self, start: usize) -> bool {
        let at = self.text[..start].trim_end_matches(OPENERS).len();
        let block = self
            .blocks
            .iter()
            .rev()
            .find(|block| **block <= at)
            .map_or(0, |block| *block);
        let lead = &self.text[block..at];
        if lead.is_empty() || (lead.ends_with(' ') && lead.split_whitespace().all(marker)) {
            return true;
        }
        // After a sentence end, unless a lower-case word follows (as after "U.S.").
        self.text[..at].strip_suffix(' ').is_some_and(|before| {
            before.trim_end_matches(CLOSERS).ends_with(ENDS)
                && !self.text[start..].starts_with(char::is_lowercase)
        })
    }

    fn ends(&self, end: usize, quote: &str) -> bool {
        let rest = self.text[end..].trim_start_matches(CLOSERS);
        // A sentence ends at its mark unless a lower-case word follows (as in
        // "U.S. revenue"), where the mark ends only an abbreviation.
        let finished = |rest: &str| {
            self.block_ends(rest)
                || rest
                    .strip_prefix(' ')
                    .is_some_and(|next| !next.starts_with(char::is_lowercase))
        };
        if quote.ends_with(ENDS) {
            return finished(rest);
        }
        if let Some(after) = rest.strip_prefix(['.', '。']) {
            if finished(after.trim_start_matches(CLOSERS)) {
                return true;
            }
        }
        self.block_ends(rest)
    }

    /// Whether a block ends where `rest` (a suffix of the text) begins.
    fn block_ends(&self, rest: &str) -> bool {
        let at = self.text.len() - rest.len();
        rest.is_empty() || (rest.starts_with(' ') && self.blocks.binary_search(&(at + 1)).is_ok())
    }
}

/// The words a report line states, without the list mark, quotation marks
/// and trailing `,`, `;` or `:` around them: `- "Revenue grew."` states
/// `Revenue grew.`.
fn claimed(line: &str) -> &str {
    let mut rest = line.trim();
    while let Some((head, tail)) = rest.split_once(char::is_whitespace) {
        if !marker(head) {
            break;
        }
        rest = tail.trim_start();
    }
    loop {
        let trimmed = rest
            .trim()
            .trim_matches(['"', '\'', '“', '”', '‘', '’'])
            .trim_end_matches([',', ';', ':']);
        if trimmed.len() == rest.len() {
            return trimmed;
        }
        rest = trimmed;
    }
}

/// A list, quote or heading mark at the start of a line.
fn marker(token: &str) -> bool {
    matches!(token, "-" | "*" | "+" | ">")
        || token.chars().all(|ch| ch == '#')
        || token.strip_suffix(['.', ')']).is_some_and(|digits| {
            !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit())
        })
}
