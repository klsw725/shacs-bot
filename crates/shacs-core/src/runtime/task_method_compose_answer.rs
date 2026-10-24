use super::task_method::{MethodId, MethodRequest};
use super::task_method_apply::part_label;
use super::task_method_calc_render::calc_line;
use super::task_method_compose::{is_calc_line, is_image_line, part_states, unavailable_parts};
use super::task_method_image_answer::{image_headline, image_line, withheld};
use super::task_method_report::{Citation, ReportVerdict};
use super::task_method_report_parse::{claim, layout, split_markers, substantive, INPUT_LIMIT};

const WITHHELD: &str =
    "[runtime: line withheld — not a word-for-word quote of a file read in this request]";

/// The answer for a composed request. With a report part, the model's report
/// is shown with the runtime's check on every line it cannot back, and each
/// `[image]` or `[calculation]` line replaced by the runtime's own line; with an
/// image part, a line is shown only if it quotes a file read in this request
/// word for word.
/// Without a report part, nothing checks the reply line by line, so (as for an
/// image request) the runtime's account replaces it (`true` = withheld).
pub(super) fn composed_answer(
    request: &MethodRequest,
    reply: &str,
    stop_reason: &str,
) -> Option<(String, bool)> {
    if request.selection.method != MethodId::Composed {
        return None;
    }
    let parts = request.selection.parts();
    let image = parts.contains(&MethodId::ImageGenerationReview);
    let report = parts.contains(&MethodId::SourceGroundedReport)
        && !unavailable_parts(&request.selection).contains(&MethodId::SourceGroundedReport);
    let states = part_states(request);
    if image && !report {
        let all: Vec<String> = states
            .iter()
            .map(|(part, label, _)| format!("{} — {label}", part_label(*part)))
            .collect();
        let text = format!(
            "Parts (each checked on its own): {}. {}",
            all.join("; "),
            image_headline(request)
        );
        return Some((withheld(text, stop_reason), true));
    }
    let open: Vec<String> = states
        .iter()
        .filter(|(_, _, confirmed)| !confirmed)
        .map(|(part, label, _)| format!("{} — {label}", part_label(*part)))
        .collect();
    let mut answer = String::new();
    if !open.is_empty() {
        answer.push_str(&format!(
            "Not every part of this request is confirmed ({}). Lines marked [runtime: …] are not backed by a confirmed part; see the parts summary below.\n\n",
            open.join("; ")
        ));
    }
    let completed = stop_reason == "completed";
    if !completed {
        answer.push_str(&format!(
            "The turn ended early ({stop_reason}), so its reply was not checked.\n\n"
        ));
    }
    if matches!(stop_reason, "error" | "tool_error") {
        // The runtime's text for the error that ended the turn (`quiet_run_error`), not a reply.
        answer.push_str(reply);
        return Some((answer, false));
    }
    let verdict =
        request.report.verdict.as_ref().filter(|verdict| {
            completed && verdict.selection_revision == request.selection.revision
        });
    let (body, withheld_lines) = checked_lines(request, verdict, reply, image);
    if withheld_lines {
        answer.push_str("This request includes an image, so the report shows only lines that quote a file read in this request word for word (a file not changed since the request began), with their sources; the others are withheld, since nothing checks what they say about the image.\n\n");
    }
    answer.push_str(&body);
    Some((answer, false))
}

/// The reply's lines as shown, and whether any was withheld.
fn checked_lines(
    request: &MethodRequest,
    verdict: Option<&ReportVerdict>,
    reply: &str,
    image: bool,
) -> (String, bool) {
    let mut seen = 0;
    let mut lines = Vec::new();
    let mut withheld_any = false;
    for line in reply.lines() {
        // Placeholders the runtime fills in, which are not report text either.
        if image && is_image_line(line) {
            lines.push(image_line(request));
            continue;
        }
        if is_calc_line(line) {
            lines.push(calc_line(request));
            continue;
        }
        let start = seen;
        seen += line.chars().count() + 1;
        // Only the runtime marks lines: a `[runtime …]` mark the model wrote is dropped.
        let line = &without_runtime_marks(line);
        let (plain, markers) = split_markers(line);
        // With an image part, the words of a marker are never shown either.
        if layout(&plain) && !(image && !markers.is_empty()) {
            lines.push(line.to_owned());
            continue;
        }
        let (parsed, _) = claim(&plain, &markers);
        let found = verdict.and_then(|verdict| {
            verdict.claims.iter().find(|checked| {
                !parsed.citations.is_empty()
                    && checked.text == parsed.text
                    && keys(&checked.citations) == keys(&parsed.citations)
            })
        });
        let note = if parsed.citations.is_empty() {
            let open = parsed.unresolved || !parsed.conflict.is_empty();
            (!open && substantive(&plain)).then(|| "no source marker, not checked".to_owned())
        } else if start + line.chars().count() > INPUT_LIMIT {
            Some("beyond the checking limit, not checked".to_owned())
        } else {
            match found {
                None => Some("not checked".to_owned()),
                Some(found) => {
                    let broken: Vec<String> = found
                        .citations
                        .iter()
                        .filter(|citation| !citation.status.linked())
                        .map(|citation| {
                            format!("{} — {}", citation.locator, citation.status.label())
                        })
                        .collect();
                    (!broken.is_empty()).then(|| format!("not backed: {}", broken.join("; ")))
                }
            }
        };
        if image {
            // Rebuilt from the checked quote and its sources' numbers: the paths
            // the model wrote, and other marks (such as `[unresolved: …]`), hold
            // words nothing checked.
            let whole = parsed.text == plain.trim();
            if let Some(found) = found.filter(|found| note.is_none() && whole && found.quoted) {
                let sources: Vec<String> = found
                    .citations
                    .iter()
                    .filter_map(|citation| citation.source)
                    .map(|seq| format!("#{seq}"))
                    .collect();
                lines.push(format!("{} [source {}]", parsed.text, sources.join("; ")));
                continue;
            }
            if lines.last().map(String::as_str) != Some(WITHHELD) {
                lines.push(WITHHELD.to_owned());
            }
            withheld_any = true;
            continue;
        }
        lines.push(match note {
            Some(note) => format!("{line} [runtime: {note}]"),
            None => line.to_owned(),
        });
    }
    (lines.join("\n"), withheld_any)
}

fn keys(citations: &[Citation]) -> Vec<&str> {
    citations
        .iter()
        .map(|citation| citation.key.as_str())
        .collect()
}

/// The line without `[runtime: …]` marks, which only the runtime may add (the
/// report parser drops them the same way).
fn without_runtime_marks(line: &str) -> String {
    let mut kept = String::new();
    let mut rest = line;
    while let Some(start) = rest.find('[') {
        let Some(length) = rest[start..].find(']') else {
            break;
        };
        let inner = &rest[start + 1..start + length];
        kept.push_str(&rest[..start]);
        let mark = inner
            .split(':')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("runtime");
        if !mark {
            kept.push_str(&rest[start..=start + length]);
        }
        rest = &rest[start + length + 1..];
        if mark && kept.ends_with(' ') {
            rest = rest.strip_prefix(' ').unwrap_or(rest);
        }
    }
    kept.push_str(rest);
    kept.trim_end().to_owned()
}
