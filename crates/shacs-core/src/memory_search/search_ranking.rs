use serde::Serialize;
use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::memory_snapshot::ItemSource;
use super::query_terms::FTS_MIN_TERM_CHARS;

/// Items each of the two rankings keeps for merging.
pub const RANKED_LIST_LIMIT: usize = 50;
const RRF_K: u64 = 60;

pub(super) struct SearchItem {
    pub key: String,
    pub source: ItemSource,
    pub text: String,
    pub search_text: String,
    pub vector: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MatchedBy {
    Text,
    Semantic,
    Both,
}

/// The order T of items whose scores are equal: `MEMORY.md` items by first line, then history
/// items by cursor from the newest and by position within an entry, then by key.
pub fn tie_order(first: (&ItemSource, &str), second: (&ItemSource, &str)) -> Ordering {
    let by_source = match (first.0, second.0) {
        (
            ItemSource::Memory { first_line, .. },
            ItemSource::Memory {
                first_line: other, ..
            },
        ) => first_line.cmp(other),
        (ItemSource::Memory { .. }, ItemSource::History { .. }) => Ordering::Less,
        (ItemSource::History { .. }, ItemSource::Memory { .. }) => Ordering::Greater,
        (
            ItemSource::History {
                cursor, ordinal, ..
            },
            ItemSource::History {
                cursor: other_cursor,
                ordinal: other_ordinal,
                ..
            },
        ) => other_cursor.cmp(cursor).then(ordinal.cmp(other_ordinal)),
    };
    by_source.then_with(|| first.1.cmp(second.1))
}

fn item_order(first: &SearchItem, second: &SearchItem) -> Ordering {
    tie_order((&first.source, &first.key), (&second.source, &second.key))
}

/// Items holding the most distinct terms first, then by bm25, then in order T. With
/// `text_scores` (bm25 by key, from the index) long terms are found through them; without, every
/// term is compared directly and bm25 is skipped.
pub(super) fn rank_text<'a>(
    items: &'a [SearchItem],
    terms: &[String],
    text_scores: Option<&BTreeMap<String, f64>>,
) -> Vec<&'a SearchItem> {
    let mut found: Vec<(&SearchItem, usize, Option<f64>)> = items
        .iter()
        .filter_map(|item| {
            let held = |term: &&String| item.search_text.contains(term.as_str());
            let distinct = terms.iter().filter(held).count();
            let score = text_scores.and_then(|scores| scores.get(&item.key).copied());
            let matched = match text_scores {
                Some(_) => {
                    score.is_some()
                        || terms
                            .iter()
                            .filter(|term| term.chars().count() < FTS_MIN_TERM_CHARS)
                            .any(|term| held(&term))
                }
                None => distinct > 0,
            };
            matched.then_some((item, distinct, score))
        })
        .collect();
    found.sort_by(|first, second| {
        second
            .1
            .cmp(&first.1)
            .then_with(|| bm25_order(first.2, second.2))
            .then_with(|| item_order(first.0, second.0))
    });
    found.truncate(RANKED_LIST_LIMIT);
    found.into_iter().map(|(item, ..)| item).collect()
}

fn bm25_order(first: Option<f64>, second: Option<f64>) -> Ordering {
    match (first, second) {
        (Some(first), Some(second)) => first.total_cmp(&second),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Items with a vector by cosine with `query`, highest first, then in order T.
pub(super) fn rank_semantic<'a>(items: &'a [SearchItem], query: &[f32]) -> Vec<&'a SearchItem> {
    let query_norm = dot(query, query).sqrt();
    let mut scored: Vec<(&SearchItem, f64)> = items
        .iter()
        .filter_map(|item| {
            let vector = item.vector.as_deref()?;
            Some((
                item,
                dot(query, vector) / (query_norm * dot(vector, vector).sqrt()),
            ))
        })
        .collect();
    scored.sort_by(|first, second| {
        second
            .1
            .total_cmp(&first.1)
            .then_with(|| item_order(first.0, second.0))
    });
    scored.truncate(RANKED_LIST_LIMIT);
    scored.into_iter().map(|(item, _)| item).collect()
}

/// Summed from +0.0 in a fixed order, so equal vectors give equal values and never -0.0.
fn dot(first: &[f32], second: &[f32]) -> f64 {
    first.iter().zip(second).fold(0.0, |sum, (left, right)| {
        sum + f64::from(*left) * f64::from(*right)
    })
}

/// The `limit` items with the highest reciprocal rank fusion score Σ 1/(60 + rank) over the two
/// lists (ranks from 1), then in order T.
pub(super) fn merge<'a>(
    text: &[&'a SearchItem],
    semantic: &[&'a SearchItem],
    limit: usize,
) -> Vec<(&'a SearchItem, MatchedBy)> {
    let mut ranks: BTreeMap<&str, (&SearchItem, Option<usize>, Option<usize>)> = BTreeMap::new();
    for (index, item) in text.iter().enumerate() {
        ranks.entry(&item.key).or_insert((item, None, None)).1 = Some(index + 1);
    }
    for (index, item) in semantic.iter().enumerate() {
        ranks.entry(&item.key).or_insert((item, None, None)).2 = Some(index + 1);
    }
    let mut merged: Vec<_> = ranks.into_values().collect();
    merged.sort_by(|first, second| {
        rrf_order(rrf_score(second.1, second.2), rrf_score(first.1, first.2))
            .then_with(|| item_order(first.0, second.0))
    });
    merged
        .into_iter()
        .take(limit)
        .map(|(item, text_rank, semantic_rank)| {
            let matched_by = match (text_rank, semantic_rank) {
                (Some(_), Some(_)) => MatchedBy::Both,
                (Some(_), None) => MatchedBy::Text,
                _ => MatchedBy::Semantic,
            };
            (item, matched_by)
        })
        .collect()
}

/// The score as an exact fraction (numerator, denominator), so that equal sums compare equal.
fn rrf_score(text_rank: Option<usize>, semantic_rank: Option<usize>) -> (u64, u64) {
    [text_rank, semantic_rank]
        .into_iter()
        .flatten()
        .map(|rank| RRF_K + rank as u64)
        .fold((0, 1), |(numerator, denominator), term| {
            (numerator * term + denominator, denominator * term)
        })
}

fn rrf_order(first: (u64, u64), second: (u64, u64)) -> Ordering {
    (first.0 * second.1).cmp(&(second.0 * first.1))
}
