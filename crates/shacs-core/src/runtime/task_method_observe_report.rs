use super::task_method_calc::bounded;
use super::task_method_image_inspect::{attempt, image_call, ImageCall};
use super::task_method_observe::MethodObservation;
use super::task_method_report::{
    failure, file_state, read_source, OutputKind, SourceKind, TEXT_LIMIT,
};
use super::task_method_report_parse::output;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone)]
pub(crate) enum ReportCall {
    Read(SourceKind, String),
    Write(String, String),
    Edit(String),
}

pub(crate) fn report_call(name: &str, arguments: &Value) -> Option<ReportCall> {
    let text = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    match name {
        "read_file" => text("path").map(|path| ReportCall::Read(SourceKind::File, path)),
        "web_fetch" => text("url").map(|url| ReportCall::Read(SourceKind::Web, url)),
        "write_file" => Some(ReportCall::Write(text("path")?, text("content")?)),
        "edit_file" => text("path").map(ReportCall::Edit),
        _ => None,
    }
}

/// Records one report-relevant call. `not_completed` is the reason to use
/// when the runtime already knows the call did not complete (blocked, or an
/// error event) and its result text does not say so itself.
pub(crate) fn record_report_call(
    observed: &mut MethodObservation,
    call: &ReportCall,
    call_id: &str,
    result: &str,
    workspace: Option<&Path>,
    not_completed: Option<&str>,
) {
    let error = failure(result).or_else(|| not_completed.map(str::to_owned));
    match call {
        ReportCall::Read(kind, locator) => {
            let file = (*kind == SourceKind::File && error.is_none())
                .then(|| file_state(workspace, locator))
                .flatten();
            observed
                .report_sources
                .push(read_source(call_id, *kind, locator, result, file, error));
        }
        ReportCall::Write(path, content) if error.is_none() => {
            observed
                .report_outputs
                .push(output(OutputKind::File, path, content));
        }
        ReportCall::Edit(path) if error.is_none() => {
            let content = file_state(workspace, path)
                .and_then(|file| std::fs::read_to_string(file.path).ok());
            if let Some(content) = content {
                observed
                    .report_outputs
                    .push(output(OutputKind::File, path, &content));
            }
        }
        ReportCall::Write(..) | ReportCall::Edit(_) => {}
    }
}

/// Records the report and image parts of one finished or blocked call.
pub(crate) fn record_result(
    observed: &mut MethodObservation,
    report: Option<&ReportCall>,
    image: Option<&ImageCall>,
    call_id: &str,
    result: &str,
    workspace: Option<&Path>,
    not_completed: Option<&str>,
) {
    if let Some(call) = report {
        record_report_call(observed, call, call_id, result, workspace, not_completed);
    }
    if let Some(call) = image {
        let recorded = attempt(call_id, call, result, not_completed, &observed.media_roots);
        observed.image_attempts.push(recorded);
    }
}

/// Records a read or image call that stopped before the observer registered it
/// (cancelled before preparation, or rejected by preparation). The requested
/// locator or options are recovered from the event's arguments when available.
pub(crate) fn record_unregistered_call(
    observed: &mut MethodObservation,
    (name, arguments): (&str, Option<&Value>),
    call_id: &str,
    result: &str,
    skipped: bool,
    workspace: Option<&Path>,
) {
    let reason = if skipped {
        "skipped before it ran"
    } else {
        "rejected before it ran"
    };
    if name == "image_generate" {
        let call = arguments
            .and_then(|arguments| image_call(name, arguments))
            .unwrap_or(ImageCall {
                options: Vec::new(),
                prompt: None,
            });
        let recorded = attempt(call_id, &call, result, Some(reason), &[]);
        observed.image_attempts.push(recorded);
        return;
    }
    if let Some(call) = arguments.and_then(|arguments| report_call(name, arguments)) {
        if matches!(call, ReportCall::Read(..)) {
            record_report_call(observed, &call, call_id, result, workspace, Some(reason));
        }
        return;
    }
    let kind = match name {
        "read_file" => SourceKind::File,
        "web_fetch" => SourceKind::Web,
        _ => return,
    };
    let locator = if skipped {
        "<read call skipped before it ran>"
    } else {
        "<read call rejected before it ran>"
    };
    let error = failure(result).unwrap_or_else(|| bounded(result.trim(), TEXT_LIMIT));
    observed.report_sources.push(read_source(
        call_id,
        kind,
        locator,
        result,
        None,
        Some(error),
    ));
}
