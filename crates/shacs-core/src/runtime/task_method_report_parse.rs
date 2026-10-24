use super::task_method_calc::bounded;
use super::task_method_report::{
    digest, Citation, CitationStatus, OutputKind, ReportClaim, ReportOutput, CLAIM_LIMIT,
    LOCATOR_LIMIT, TEXT_LIMIT,
};

pub(crate) const INPUT_LIMIT: usize = 65_536;
/// Citation key of the calculation part of a composed request.
pub(crate) const RESULT_CALCULATION: &str = "result:calculation";

#[cfg(test)]
#[path = "task_method_report_parse_tests.rs"]
mod tests;

pub(crate) fn output(kind: OutputKind, locator: &str, text: &str) -> ReportOutput {
    // Text, lines and citations beyond the limits are counted as unchecked.
    let mut unchecked = usize::from(text.chars().count() > INPUT_LIMIT);
    let text: String = text.chars().take(INPUT_LIMIT).collect();
    let mut claims = Vec::new();
    let mut uncited_lines = 0;
    let mut uncited = Vec::new();
    for line in text.lines() {
        let (plain, markers) = split_markers(line);
        if layout(&plain) {
            // Markers without a statement back nothing.
            continue;
        }
        let claim = (!markers.is_empty()).then(|| claim(&plain, &markers));
        match claim {
            Some((mut claim, overflow))
                if !claim.citations.is_empty()
                    || claim.unresolved
                    || !claim.conflict.is_empty() =>
            {
                // A cited heading, lead-in or pointer is checked but is not content.
                claim.statement = substantive(&plain);
                unchecked += usize::from(overflow);
                if claims.len() < CLAIM_LIMIT {
                    claims.push(claim);
                } else {
                    unchecked += 1;
                }
            }
            // No marker, or only empty ones: the statement is not source-checked.
            _ if substantive(&plain) => {
                uncited_lines += 1;
                if uncited.len() < CLAIM_LIMIT {
                    uncited.push(bounded(plain.trim(), TEXT_LIMIT));
                }
            }
            _ => {}
        }
    }
    ReportOutput {
        kind,
        locator: bounded(locator, LOCATOR_LIMIT),
        key: locator_key(locator),
        selection_revision: 0,
        digest: digest(text.as_bytes()),
        claims,
        uncited_lines,
        uncited,
        unchecked,
    }
}

/// The claim on one marked line, and whether its citations overflowed.
pub(crate) fn claim(plain: &str, markers: &[String]) -> (ReportClaim, bool) {
    let mut claim = ReportClaim {
        text: bounded(plain.trim(), TEXT_LIMIT),
        citations: Vec::new(),
        unresolved: false,
        conflict: Vec::new(),
        statement: true,
        quoted: false,
    };
    for marker in markers {
        let (key, value) = marker.split_once(':').unwrap_or((marker.as_str(), ""));
        match key.trim().to_ascii_lowercase().as_str() {
            "source" | "sources" => {
                claim
                    .citations
                    .extend(locators(value).into_iter().map(|(locator, key)| Citation {
                        locator,
                        key,
                        status: CitationStatus::Unread,
                        source: None,
                    }))
            }
            "calculation" => claim
                .citations
                .push(result("calculation result", RESULT_CALCULATION)),
            "unresolved" => claim.unresolved = true,
            "conflict" => claim
                .conflict
                .extend(locators(value).into_iter().map(|(locator, _)| locator)),
            _ => {}
        }
    }
    let overflow = claim.citations.len() > CLAIM_LIMIT;
    claim.citations.truncate(CLAIM_LIMIT);
    claim.conflict.truncate(CLAIM_LIMIT);
    (claim, overflow)
}

/// Empty text or markdown layout only (rules, table separators, fences).
pub(crate) fn layout(line: &str) -> bool {
    line.trim().chars().all(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                '-' | '*' | '_' | '=' | '|' | ':' | '+' | '>' | '#' | '`' | '~'
            )
    })
}

/// A line that states something. Only headings, lead-ins ending with ':',
/// markdown layout lines and the file pointer "Saved: <file>" are skipped.
pub(crate) fn substantive(line: &str) -> bool {
    let line = line.trim();
    let layout = layout(line);
    let pointer = line.split_once(':').is_some_and(|(label, file)| {
        let file = file.trim();
        label.trim().eq_ignore_ascii_case("saved")
            && !file.is_empty()
            && !file.contains(char::is_whitespace)
    });
    let fence = line.starts_with("```") || line.starts_with("~~~");
    !(layout
        || pointer
        || fence
        || line.starts_with('#')
        || line.ends_with(':')
        || line.ends_with('：'))
}

pub(crate) fn split_markers(line: &str) -> (String, Vec<String>) {
    let mut plain = String::new();
    let mut markers = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('[') {
        let Some(length) = rest[start..].find(']') else {
            break;
        };
        let inner = &rest[start + 1..start + length];
        let key = inner
            .split(':')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "source" | "sources" | "unresolved" | "conflict" | "calculation" | "runtime"
        ) {
            plain.push_str(&rest[..start]);
            markers.push(inner.to_owned());
        } else {
            plain.push_str(&rest[..start + length + 1]);
        }
        rest = &rest[start + length + 1..];
    }
    plain.push_str(rest);
    (plain, markers)
}

fn result(locator: &str, key: &str) -> Citation {
    Citation {
        locator: locator.to_owned(),
        key: key.to_owned(),
        status: CitationStatus::Unread,
        source: None,
    }
}

/// Cited locators as (bounded display text, exact match key).
fn locators(value: &str) -> Vec<(String, String)> {
    value
        .split(';')
        .map(normalize)
        .filter(|locator| !locator.is_empty())
        .map(|locator| (bounded(&locator, LOCATOR_LIMIT), locator_key(&locator)))
        .collect()
}

pub(crate) fn normalize(locator: &str) -> String {
    let trimmed = locator
        .trim()
        .trim_matches(|ch| matches!(ch, '`' | '"' | '\'' | '<' | '>'))
        .trim();
    let trimmed = trimmed.strip_prefix("./").unwrap_or(trimmed);
    trimmed.trim_end_matches('/').to_owned()
}

/// Exact-match key of a whole normalized locator, so long locators that share
/// a displayed prefix never collide.
pub(crate) fn locator_key(locator: &str) -> String {
    digest(normalize(locator).as_bytes())
}
