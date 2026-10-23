//! In a composed request with an image part, the model's report lines are
//! shown only where they quote a file read in this request word for word:
//! such a line says what the file says, while nothing checks what any other
//! line says about the image. A file changed since the request began may hold
//! the model's own words (it can write files), so it is never quoted.

use std::collections::BTreeMap;
use std::time::SystemTime;

use super::task_method::{MethodId, MethodRequest};
use super::task_method_quote_match::Folded;
use super::task_method_report::{ReportClaim, ReportSource, TEXT_LIMIT};
use super::task_method_report_source::digest;

#[cfg(test)]
#[path = "task_method_compose_quote_tests.rs"]
mod tests;

/// The largest source file read again to look for a quote.
const QUOTE_FILE_LIMIT: u64 = 8 << 20;

/// A composed request with an image part, where report lines are withheld
/// unless they quote a read file (see the module note).
pub(super) fn withholds(request: &MethodRequest) -> bool {
    request.selection.method == MethodId::Composed
        && request
            .selection
            .parts()
            .contains(&MethodId::ImageGenerationReview)
}

/// Whether a recorded report claim is withheld (see [`withholds`]).
pub(super) fn withheld_claim(request: &MethodRequest, claim: &ReportClaim) -> bool {
    withholds(request) && !claim.quoted
}

/// Marks the current report claims that quote a cited file word for word: the
/// file is read again and must still match the digest read in this request.
pub(crate) fn mark_quotes(request: &mut MethodRequest) {
    if !withholds(request) {
        return;
    }
    let started = request
        .started_at
        .as_deref()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .map(SystemTime::from);
    let sources = request.report.sources.clone();
    let Some(verdict) = request.report.verdict.as_mut() else {
        return;
    };
    let mut files: BTreeMap<u64, Option<Folded>> = BTreeMap::new();
    for claim in &mut verdict.claims {
        let linked = !claim.citations.is_empty()
            && claim
                .citations
                .iter()
                .all(|citation| citation.status.linked());
        // Stored text at the limit may have been cut short, so it is not a whole line.
        let whole = claim.text.chars().count() < TEXT_LIMIT;
        claim.quoted = linked
            && whole
            && claim.citations.iter().any(|citation| {
                citation.source.is_some_and(|seq| {
                    files
                        .entry(seq)
                        .or_insert_with(|| unchanged(&sources, seq, started))
                        .as_ref()
                        .is_some_and(|file| file.quotes(&claim.text))
                })
            });
    }
}

/// Source `seq` as read in this request, if its file is unchanged since the
/// request began and since it was read.
fn unchanged(sources: &[ReportSource], seq: u64, started: Option<SystemTime>) -> Option<Folded> {
    let source = sources.iter().find(|source| source.seq == seq)?;
    let (path, expected, started) = (
        source.resolved_path.as_ref()?,
        source.file_digest.as_ref()?,
        started?,
    );
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > QUOTE_FILE_LIMIT || meta.modified().ok()? >= started {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    (&digest(&bytes) == expected).then(|| Folded::new(&String::from_utf8_lossy(&bytes)))
}
