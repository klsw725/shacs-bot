use super::web_watch_number::{
    adjacent_unit, infer_format, is_unit_char, numeric_runs, parse_number, NumberFormat,
    ALL_SEPARATORS,
};
use super::{
    WebCheckFailure, WebExtraction, WebObservedValue, WebPreviewTarget, WebValueKind,
    WEB_TEXT_VERSION,
};

const MAX_REGION_CHARS: usize = 120;
const LABEL_WINDOW_CHARS: usize = 80;
const MAX_PREFIX_CHARS: usize = 32;
const MAX_SUFFIX_CHARS: usize = 24;
const MAX_CANDIDATES: usize = 10;
/// Dash and tilde forms that join the two ends of a range.
const RANGE_MARKS: [char; 7] = [
    '~', '\u{ff5e}', '\u{301c}', '-', '\u{2212}', '\u{2013}', '\u{2014}',
];
/// Words that join the two ends of a range written apart, as in `$10 to $12`.
const RANGE_WORDS: [&str; 3] = ["to", "부터", "에서"];
/// v1 reads unsigned single numbers only; a number right after one of these (a sign, a
/// leading separator or a range mark) is not offered.
pub(super) const UNSUPPORTED_NUMBER_LEAD: [char; 12] = [
    '-', '\u{2212}', '+', '\u{ff0b}', '\u{b1}', '.', ',', '~', '\u{ff5e}', '\u{301c}', '\u{2013}',
    '\u{2014}',
];

pub(super) struct Candidate {
    pub shown_text: String,
    pub value: WebObservedValue,
    pub extraction: WebExtraction,
}

/// Reads the confirmed region. Never picks another location when the anchors do not match.
pub(super) fn extract(
    text: &str,
    extraction: &WebExtraction,
) -> Result<WebObservedValue, WebCheckFailure> {
    extract_shown(text, extraction).map(|(value, _)| value)
}

/// [`extract`], with the region's text as the page shows it.
pub(super) fn extract_shown<'a>(
    text: &'a str,
    extraction: &WebExtraction,
) -> Result<(WebObservedValue, &'a str), WebCheckFailure> {
    let matches = region_matches(text, &extraction.prefix, &extraction.suffix);
    if matches.is_empty() {
        return Err(WebCheckFailure::StaticContentUnavailable);
    }
    if u32::try_from(matches.len()).ok() != Some(extraction.match_count) {
        return Err(WebCheckFailure::AmbiguousValue);
    }
    let (start, end) = usize::try_from(extraction.occurrence)
        .ok()
        .and_then(|occurrence| occurrence.checked_sub(1))
        .and_then(|index| matches.get(index).copied())
        .ok_or(WebCheckFailure::AmbiguousValue)?;
    let region = text[start..end].trim();
    let value = match &extraction.value {
        WebValueKind::Text { phrase } => WebObservedValue::Text {
            present: region.contains(phrase.as_str()),
        },
        WebValueKind::Number {
            decimal_separator,
            thousands_separator,
            unit_before,
            unit_after,
        } => {
            if continues_number(&text[end..])
                || repeats_slot(&text[end + extraction.suffix.len()..], &extraction.suffix)
            {
                return Err(WebCheckFailure::AmbiguousValue);
            }
            let format = NumberFormat {
                decimal: *decimal_separator,
                thousands: *thousands_separator,
            };
            let (run_start, run_end) = match numeric_runs(region, &format.separators()).as_slice() {
                [] => return Err(WebCheckFailure::FormatMismatch),
                [run] => *run,
                [first, .., last] => {
                    let joined = region[first.0..last.1]
                        .chars()
                        .all(|c| c.is_ascii_digit() || ALL_SEPARATORS.contains(&c));
                    return Err(if joined {
                        WebCheckFailure::FormatMismatch
                    } else {
                        WebCheckFailure::AmbiguousValue
                    });
                }
            };
            if region[..run_start] != *unit_before.as_deref().unwrap_or("")
                || region[run_end..] != *unit_after.as_deref().unwrap_or("")
            {
                return Err(WebCheckFailure::FormatMismatch);
            }
            parse_number(&region[run_start..run_end], format)
                .map(|value| WebObservedValue::Number { value })
                .ok_or(WebCheckFailure::FormatMismatch)?
        }
    };
    Ok((value, region))
}

/// Deterministic preview candidates; page text only becomes anchors and values.
pub(super) fn candidates(text: &str, target: &WebPreviewTarget) -> Vec<Candidate> {
    let mut found = Vec::new();
    match target {
        WebPreviewTarget::Number {
            label,
            decimal_comma,
        } => {
            for (label_start, _) in text.match_indices(label.as_str()) {
                let window_start = label_start + label.len();
                let window_end = advance(text, window_start, LABEL_WINDOW_CHARS);
                let Some((start, end, kind, value)) =
                    first_number(text, window_start, window_end, *decimal_comma)
                else {
                    continue;
                };
                let Some(suffix) =
                    suffix_after(text, end).filter(|_| !continues_number(&text[end..]))
                else {
                    continue;
                };
                push(
                    &mut found,
                    text,
                    (&text[label_start..start], suffix),
                    (start, &text[start..end]),
                    kind,
                    value,
                );
            }
        }
        WebPreviewTarget::Text { phrase, area: None } => {
            for (start, _) in text.match_indices(phrase.as_str()) {
                let (Some(prefix), Some(suffix)) = (
                    prefix_before(text, start),
                    suffix_after(text, start + phrase.len()),
                ) else {
                    continue;
                };
                push(
                    &mut found,
                    text,
                    (prefix, suffix),
                    (start, phrase),
                    WebValueKind::Text {
                        phrase: phrase.clone(),
                    },
                    WebObservedValue::Text { present: true },
                );
            }
        }
        WebPreviewTarget::Text {
            phrase,
            area: Some(area),
        } => {
            for (start, end) in region_matches(text, &area.label, &area.until) {
                let region = text[start..end].trim();
                push(
                    &mut found,
                    text,
                    (&area.label, &area.until),
                    (start, region),
                    WebValueKind::Text {
                        phrase: phrase.clone(),
                    },
                    WebObservedValue::Text {
                        present: region.contains(phrase.as_str()),
                    },
                );
            }
        }
    }
    found.truncate(MAX_CANDIDATES);
    found
}

fn push(
    found: &mut Vec<Candidate>,
    text: &str,
    (prefix, suffix): (&str, &str),
    (start, shown): (usize, &str),
    kind: WebValueKind,
    value: WebObservedValue,
) {
    let matches = region_matches(text, prefix, suffix);
    let Some(index) = matches
        .iter()
        .position(|(region_start, _)| *region_start == start)
    else {
        return;
    };
    let (Ok(occurrence), Ok(match_count)) =
        (u32::try_from(index + 1), u32::try_from(matches.len()))
    else {
        return;
    };
    let extraction = WebExtraction {
        version: WEB_TEXT_VERSION.to_owned(),
        prefix: prefix.to_owned(),
        suffix: suffix.to_owned(),
        occurrence,
        match_count,
        value: kind,
    };
    if extract(text, &extraction).as_ref() != Ok(&value)
        || found
            .iter()
            .any(|candidate| candidate.extraction == extraction)
    {
        return;
    }
    found.push(Candidate {
        shown_text: shown.to_owned(),
        value,
        extraction,
    });
}

/// The first number in `text[from..to]`, as absolute byte positions. When v1 cannot read
/// that number, there is no candidate; a later number is never offered in its place.
fn first_number(
    text: &str,
    from: usize,
    to: usize,
    decimal_comma: bool,
) -> Option<(usize, usize, WebValueKind, WebObservedValue)> {
    let window = &text[from..to];
    let (run_start, run_end) = numeric_runs(window, &ALL_SEPARATORS).first().copied()?;
    let mut parts = vec![(run_start, run_end)];
    if window[run_start..run_end].contains(' ') {
        parts.extend(
            numeric_runs(&window[..run_end], &['.', ',', '\''])
                .into_iter()
                .find(|(start, _)| *start >= run_start),
        );
    }
    for (start, end) in parts {
        let Some(format) = infer_format(&window[start..end], decimal_comma) else {
            continue;
        };
        let value = parse_number(&window[start..end], format)?;
        let unit_before = adjacent_unit(&window[..start], true);
        let unit_after = adjacent_unit(&window[end..], false);
        let span_start = from + start - unit_before.len();
        if text[..span_start].ends_with(UNSUPPORTED_NUMBER_LEAD) {
            continue;
        }
        let kind = WebValueKind::Number {
            decimal_separator: format.decimal,
            thousands_separator: format.thousands,
            unit_before: (!unit_before.is_empty()).then(|| unit_before.to_owned()),
            unit_after: (!unit_after.is_empty()).then(|| unit_after.to_owned()),
        };
        let span_end = from + end + unit_after.len();
        return Some((
            span_start,
            span_end,
            kind,
            WebObservedValue::Number { value },
        ));
    }
    None
}

/// Regions between `prefix` and the next `suffix`, scanning left to right without overlap.
fn region_matches(text: &str, prefix: &str, suffix: &str) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    if prefix.is_empty() || suffix.is_empty() {
        return regions;
    }
    let mut from = 0;
    while let Some(offset) = text[from..].find(prefix) {
        let at = from + offset;
        let start = at + prefix.len();
        let end = text[start..]
            .find(suffix)
            .map(|offset| start + offset)
            .filter(|end| text[start..*end].chars().count() <= MAX_REGION_CHARS);
        match end {
            Some(end) => {
                regions.push((start, end));
                from = end + suffix.len();
            }
            None => from = at + text[at..].chars().next().map_or(1, char::len_utf8),
        }
    }
    regions
}

/// Whether the text after a number continues it: a digit in the next word (`~12,000`,
/// `₩15,000`), or a range mark or word and another number, maybe after a separated unit
/// word (`~ -₩12,000`, `원 - 12,000`, `to $12`). Such a number is not offered, so a range
/// never becomes one value.
pub(super) fn continues_number(after: &str) -> bool {
    let rest = after.trim_start();
    let next_word = rest.split_whitespace().next().unwrap_or_default();
    if next_word.contains(|character: char| character.is_ascii_digit()) {
        return true;
    }
    let rest = match rest.strip_prefix(next_word) {
        Some(after_unit)
            if next_word.chars().all(is_unit_char) && range_word(next_word).is_none() =>
        {
            after_unit.trim_start()
        }
        _ => rest,
    };
    rest.strip_prefix(RANGE_MARKS)
        .or_else(|| range_word(rest).map(|word| &rest[word.len()..]))
        .is_some_and(|other_end| starts_number(other_end.trim_start()))
}

/// Whether another number with the same suffix follows the suffix, as in `10,000 원 13,000 원`
/// or `13,000 원 (10,000 원)`: a second value in the same slot. A number after a different
/// word (`배송비 3,000원`) is another field and does not count.
fn repeats_slot(after_suffix: &str, suffix: &str) -> bool {
    let rest = after_suffix.trim_start_matches(|character: char| {
        !character.is_ascii_digit() && !character.is_alphabetic()
    });
    rest.starts_with(|character: char| character.is_ascii_digit())
        && rest
            .trim_start_matches(|character: char| {
                character.is_ascii_digit() || ALL_SEPARATORS.contains(&character)
            })
            .starts_with(suffix)
}

/// The range word `text` starts with, as a whole word.
fn range_word(text: &str) -> Option<&'static str> {
    RANGE_WORDS.into_iter().find(|word| {
        text.get(..word.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(word))
            && text[word.len()..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
    })
}

/// Whether `text` starts with a number, perhaps behind a currency, unit, sign or separator.
fn starts_number(text: &str) -> bool {
    text.trim_start_matches(|character: char| {
        is_unit_char(character) || UNSUPPORTED_NUMBER_LEAD.contains(&character)
    })
    .starts_with(|character: char| character.is_ascii_digit())
}

/// The next word after `end` without the space before it, for example `"배송비"`. Only the
/// prefix keeps the space at the region edge, so a region whose value is deleted still matches.
fn suffix_after(text: &str, end: usize) -> Option<&str> {
    let rest = text[end..].trim_start();
    let word = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let suffix = &rest[..advance(rest, 0, MAX_SUFFIX_CHARS).min(word)];
    (!suffix.is_empty()).then_some(suffix)
}

/// The previous word before `start` with its trailing space, for example `"재고: "`.
fn prefix_before(text: &str, start: usize) -> Option<&str> {
    let before = &text[..start];
    let word_end = before.trim_end().len();
    let word_start = before[..word_end]
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1);
    let first = before.len()
        - before[word_start..]
            .chars()
            .rev()
            .take(MAX_PREFIX_CHARS)
            .map(char::len_utf8)
            .sum::<usize>();
    let prefix = &before[first..];
    (!prefix.trim().is_empty()).then_some(prefix)
}

fn advance(text: &str, from: usize, chars: usize) -> usize {
    text[from..]
        .char_indices()
        .nth(chars)
        .map_or(text.len(), |(offset, _)| from + offset)
}

#[cfg(test)]
mod tests {
    use super::{continues_number, repeats_slot};

    #[test]
    fn only_a_following_number_or_the_other_end_of_a_range_continues_a_value() {
        let cases = [
            ("~12,000 배송비", true),
            (" \u{20a9}15,000 정가", true),
            (" ~ \u{20a9}12,000 배송비", true),
            (" ~ -12,000 배송비", true),
            (" ~ +.5", true),
            (" 원 - 12,000 원", true),
            (" to $12 배송비", true),
            (" 원 부터 12,000 원", true),
            (" 배송비 무료", false),
            ("/월 배송비", false),
            (" 배송비 - 무료", false),
            (" to 무료", false),
            (" per month", false),
            (" today 12", false),
        ];

        for (after, continues) in cases {
            assert_eq!(continues_number(after), continues, "{after}");
        }
    }

    #[test]
    fn only_the_same_suffix_after_another_number_repeats_a_slot() {
        let cases = [
            (" 13,000 원 배송", "원", true),
            (" (13,000 원)", "원", true),
            (", 13,000원", "원", true),
            (" 3,000원 무료", "배송비", false),
            (" 배송비 3,000 원", "원", false),
            (" 3개 구매 시 30,000 원", "원", false),
        ];

        for (after_suffix, suffix, repeats) in cases {
            assert_eq!(
                repeats_slot(after_suffix, suffix),
                repeats,
                "{after_suffix}"
            );
        }
    }
}
