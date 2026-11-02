use regex::Regex;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use super::memory_snapshot;

pub const MAX_QUERY_TERMS: usize = 32;
/// The trigram index cannot match a shorter term, so those are compared directly.
pub(super) const FTS_MIN_TERM_CHARS: usize = 3;

/// Unicode general categories Z*, P*, S* and Cc.
static TERM_BOUNDARY: OnceLock<Option<Regex>> = OnceLock::new();

/// The terms of `query` in its search text (NFC, then lowercase), split at every space,
/// punctuation, symbol and control character: the first [`MAX_QUERY_TERMS`] distinct ones, in
/// the order they first appear.
pub fn query_terms(query: &str) -> Vec<String> {
    let search_text = memory_snapshot::search_text(query);
    let Some(boundary) = TERM_BOUNDARY
        .get_or_init(|| Regex::new(r"[\p{Z}\p{P}\p{S}\p{Cc}]").ok())
        .as_ref()
    else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    boundary
        .split(&search_text)
        .filter(|term| !term.is_empty() && seen.insert(*term))
        .take(MAX_QUERY_TERMS)
        .map(str::to_owned)
        .collect()
}

/// An FTS5 query that matches any term long enough for the trigram index, each as a quoted
/// phrase with its quotes doubled.
pub(super) fn fts_query(terms: &[String]) -> Option<String> {
    let phrases: Vec<String> = terms
        .iter()
        .filter(|term| term.chars().count() >= FTS_MIN_TERM_CHARS)
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect();
    (!phrases.is_empty()).then(|| phrases.join(" OR "))
}

#[cfg(test)]
mod tests {
    use super::fts_query;

    #[test]
    fn fts_query_quotes_each_long_term_and_doubles_quotes_inside() {
        // Given
        let terms = ["ab", "say\"hi\"", "회의록"].map(str::to_owned);

        // When
        let query = fts_query(&terms);

        // Then
        assert_eq!(query.as_deref(), Some("\"say\"\"hi\"\"\" OR \"회의록\""));
        assert_eq!(fts_query(&["ab".to_owned(), "치과".to_owned()]), None);
    }
}
