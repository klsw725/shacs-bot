use super::{now_iso, RuntimeToolCall, Session};
use crate::runtime::task_method::{MethodId, MethodRequest, UserAcceptance};
use crate::runtime::task_method_apply::{part_label, refresh};
use crate::runtime::task_method_calc::run;
use crate::runtime::task_method_calc_render::details as calc_details;
use crate::runtime::task_method_compose::part_states;
use crate::runtime::task_method_image_inspect::{attempt, image_call};
use crate::runtime::task_method_image_render::details as image_details;
use crate::runtime::task_method_observe::exec_command;
use crate::runtime::task_method_observe_report::{report_call, ReportCall};
use crate::runtime::task_method_render::request_text;
use crate::runtime::task_method_report::{failure, read_source};
use crate::runtime::task_method_report_details::details as report_details;
use crate::runtime::task_method_store::MethodStore;
use serde_json::Value;

pub(super) fn accept(store: &mut MethodStore) -> String {
    let Some(request) = store.active.as_mut().or_else(|| store.closed.last_mut()) else {
        return "No method request to accept.".to_owned();
    };
    // Record acceptance against the current state of the sources, not a stale one.
    refresh(request);
    let revision = request.selection.revision;
    let calculation = request
        .calculation
        .verdict
        .as_ref()
        .filter(|verdict| verdict.selection_revision == revision)
        .map(|verdict| verdict.state.label());
    let report = request
        .report
        .verdict
        .as_ref()
        .filter(|verdict| verdict.selection_revision == revision)
        .map(|verdict| verdict.state.label());
    let image = request
        .image
        .verdict
        .as_ref()
        .filter(|verdict| verdict.selection_revision == revision)
        .map(|verdict| verdict.state.label());
    let verification = if request.selection.method == MethodId::Composed {
        let states: Vec<String> = part_states(request)
            .into_iter()
            .map(|(part, label, _)| format!("{} — {label}", part_label(part)))
            .collect();
        states.join("; ")
    } else {
        calculation
            .or(report)
            .or(image)
            .unwrap_or("not evaluated")
            .to_owned()
    };
    let message = format!(
        "Recorded your acceptance of {} (revision {revision}). Verification is unchanged: {verification}.",
        request.request_id
    );
    request.user_acceptance = Some(UserAcceptance {
        selection_revision: revision,
        verification,
        recorded_at: now_iso(),
    });
    message
}

pub(super) fn record_denied_call(
    session: &mut Session,
    call: &RuntimeToolCall,
    content: &str,
) -> std::io::Result<()> {
    let mut store = MethodStore::load(session)?;
    let Some(request) = store.active.as_mut() else {
        return Ok(());
    };
    if record_denied(request, call, content) {
        store.save(session)?;
    }
    Ok(())
}

/// Records a call that never ran (denied, or cancelled with its approval) in
/// every part of `request` it belongs to; `false` when it belongs to none.
pub(super) fn record_denied(
    request: &mut MethodRequest,
    call: &RuntimeToolCall,
    content: &str,
) -> bool {
    let (name, arguments) = if call.name == "tool_call" {
        (
            call.arguments
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&call.name),
            call.arguments.get("arguments").unwrap_or(&call.arguments),
        )
    } else {
        (call.name.as_str(), &call.arguments)
    };
    let command = exec_command(name, arguments);
    let read = match report_call(name, arguments) {
        Some(ReportCall::Read(kind, locator)) => Some((kind, locator)),
        _ => None,
    };
    let image = image_call(name, arguments);
    let revision = request.selection.revision;
    let parts = request.selection.parts();
    // Each part of the request (one, or several when composed) records its own call.
    let part = |method: MethodId| parts.contains(&method);
    match (command, read, image) {
        (Some(command), _, _) if part(MethodId::ReproducibleCalculation) => request
            .calculation
            .push(run(&call.id, &command, content), revision),
        (_, _, Some(image)) if part(MethodId::ImageGenerationReview) => request
            .image
            .push_attempt(attempt(&call.id, &image, content, None, &[]), revision),
        (_, Some((kind, locator)), _) if part(MethodId::SourceGroundedReport) => {
            let error = failure(content).unwrap_or_else(|| "permission denied".to_owned());
            let denied = read_source(&call.id, kind, &locator, content, None, Some(error));
            request.report.push_source(denied, revision);
        }
        _ => return false,
    }
    true
}

pub(super) fn status(store: &mut MethodStore) -> String {
    if let Some(request) = store.active.as_mut().or_else(|| store.closed.last_mut()) {
        refresh(request);
    }
    let current = store
        .active
        .as_ref()
        .or_else(|| store.closed.last())
        .map(|request| {
            format!(
                "{}{}{}{}",
                request_text(request),
                calc_details(request),
                report_details(request),
                image_details(request)
            )
        })
        .unwrap_or_else(|| "No method request recorded.".to_owned());
    let next = match (&store.pending_revision, store.pending_override) {
        (Some(id), _) => {
            let image = store.closed.iter().any(|request| {
                &request.request_id == id
                    && request.selection.method == MethodId::ImageGenerationReview
            });
            let kind = if image { "image" } else { "report" };
            format!("revision of the {kind} from {id}")
        }
        (None, Some(method)) => method.canonical().to_owned(),
        (None, None) => "automatic".to_owned(),
    };
    format!("{current}\nNext-request override: {next}")
}

pub(super) fn revise(store: &mut MethodStore) -> String {
    if store.active.is_some() {
        return "A request is still in progress. Finish it, or use /new, before revising a report or image.".to_owned();
    }
    let Some(request) = store.closed.iter().rev().find(|request| {
        matches!(
            request.selection.method,
            MethodId::SourceGroundedReport | MethodId::ImageGenerationReview
        )
    }) else {
        return "No completed report or image request to revise.".to_owned();
    };
    let id = request.request_id.clone();
    let message = if request.selection.method == MethodId::ImageGenerationReview {
        let saved = request.image.saved_artifacts();
        let link = if saved.is_empty() {
            format!("{id} has no saved image to link")
        } else {
            let refs: Vec<&str> = saved.iter().map(|a| a.reference.as_str()).collect();
            format!(
                "If a new image is saved, it is linked to the earlier image {}",
                refs.join(", ")
            )
        };
        format!(
            "Your next message revises the image from {id} as version {}. Nothing is generated yet: the next turn asks for a new image, and only a saved image counts. {link}. Editing the existing image is unsupported. Earlier checks are not inherited.",
            request.image.version.saturating_add(1)
        )
    } else {
        format!(
            "Your next message revises the report from {id} as version {}. Its read sources are carried over and re-checked; earlier checks are not inherited.",
            request.report.version.saturating_add(1)
        )
    };
    store.pending_override = None;
    store.pending_revision = Some(id);
    message
}
