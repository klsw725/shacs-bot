use super::runner_spec034::observable_call_id;
use super::task_method::{EffectOutcome, EvidenceFact, ExternalEffectFact};
use super::task_method_calc::{run, CalculationRun};
use super::task_method_image::ImageAttempt;
use super::task_method_image_inspect::{image_call, ImageCall};
use super::task_method_observe_report::{
    record_result, record_unregistered_call, report_call, ReportCall,
};
use super::task_method_report::{ReportOutput, ReportSource};
use super::task_method_store::preview;
use crate::runtime::{RuntimeToolCall, RuntimeToolMessage, ToolEvent, ToolStatus};
use crate::tools::ToolRegistry;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

#[path = "task_method_observe_hook.rs"]
mod hook;
#[cfg(test)]
#[path = "task_method_observe_tests.rs"]
mod tests;

pub(crate) use hook::attach;

#[derive(Debug, Default)]
pub struct MethodObservation {
    pub(crate) effects: Vec<ExternalEffectFact>,
    pub(crate) evidence: Vec<EvidenceFact>,
    pub(crate) calculation_runs: Vec<CalculationRun>,
    pub(crate) report_sources: Vec<ReportSource>,
    pub(crate) report_outputs: Vec<ReportOutput>,
    pub(crate) image_attempts: Vec<ImageAttempt>,
    pub(crate) media_roots: Vec<PathBuf>,
}

impl MethodObservation {
    /// An observation that checks saved images under these media roots.
    pub fn with_media_roots(media_roots: Vec<PathBuf>) -> Self {
        Self {
            media_roots,
            ..Self::default()
        }
    }
}

struct Candidate {
    name: String,
    digest: String,
    written_path: Option<String>,
    command: Option<String>,
    report: Option<ReportCall>,
    image: Option<ImageCall>,
    effect: bool,
}

pub(crate) fn exec_command(name: &str, arguments: &Value) -> Option<String> {
    (name == "exec")
        .then(|| arguments.get("command").and_then(Value::as_str))
        .flatten()
        .map(str::to_owned)
}

const INVALID_EXEC: &str = "<exec call rejected before it ran>";
const SKIPPED_EXEC: &str = "<exec call skipped before it ran>";

struct Capture {
    tools: ToolRegistry,
    calls: Mutex<BTreeMap<String, Candidate>>,
    observed: Arc<Mutex<MethodObservation>>,
    turn_id: String,
    workspace: Option<PathBuf>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("lock poisoned by a panicking thread")
}

impl Capture {
    fn record_calls(&self, calls: &[RuntimeToolCall]) {
        let mut pending = lock(&self.calls);
        for call in calls {
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
            pending.insert(
                observable_call_id(&call.id),
                Candidate {
                    name: preview(name),
                    digest: digest(arguments),
                    written_path: matches!(name, "write_file" | "edit_file")
                        .then(|| arguments.get("path").and_then(Value::as_str).map(preview))
                        .flatten(),
                    command: exec_command(name, arguments),
                    report: report_call(name, arguments),
                    image: image_call(name, arguments),
                    effect: self.tools.get(name).is_none_or(|tool| !tool.read_only()),
                },
            );
        }
    }

    fn record_blocked(&self, blocked: &[RuntimeToolMessage]) {
        for message in blocked {
            let id = observable_call_id(&message.tool_call_id);
            let Some(candidate) = lock(&self.calls).remove(&id) else {
                continue;
            };
            let mut observed = lock(&self.observed);
            if let Some(command) = &candidate.command {
                observed
                    .calculation_runs
                    .push(run(&id, command, &message.content));
            }
            record_result(
                &mut observed,
                candidate.report.as_ref(),
                candidate.image.as_ref(),
                &id,
                &message.content,
                self.workspace.as_deref(),
                Some("blocked before it ran"),
            );
        }
    }

    fn event(&self, event: &ToolEvent) {
        let outcome = match event.status {
            ToolStatus::Waiting => return,
            ToolStatus::Skipped => EffectOutcome::Cancelled,
            ToolStatus::Ok => EffectOutcome::Completed,
            ToolStatus::Error => failure_outcome(&event.detail),
        };
        let output = match &event.result {
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
            None => event.detail.clone(),
        };
        let Some(candidate) = event
            .call_id
            .as_ref()
            .and_then(|id| lock(&self.calls).remove(id))
        else {
            let sentinel = match event.status {
                ToolStatus::Error => INVALID_EXEC,
                ToolStatus::Skipped => SKIPPED_EXEC,
                ToolStatus::Ok | ToolStatus::Waiting => return,
            };
            if let (Some(id), "exec") = (&event.call_id, event.name.as_str()) {
                lock(&self.observed)
                    .calculation_runs
                    .push(run(id, sentinel, &output));
            }
            if let Some(id) = &event.call_id {
                record_unregistered_call(
                    &mut lock(&self.observed),
                    (&event.name, event.arguments.as_ref()),
                    id,
                    &output,
                    event.status == ToolStatus::Skipped,
                    self.workspace.as_deref(),
                );
            }
            return;
        };
        let mut observed = lock(&self.observed);
        if let (Some(command), Some(id)) = (&candidate.command, &event.call_id) {
            observed.calculation_runs.push(run(id, command, &output));
        }
        if let Some(id) = &event.call_id {
            let not_completed =
                (event.status != ToolStatus::Ok).then_some("the tool reported an error");
            record_result(
                &mut observed,
                candidate.report.as_ref(),
                candidate.image.as_ref(),
                id,
                &output,
                self.workspace.as_deref(),
                not_completed,
            );
        }
        if candidate.effect {
            observed.effects.push(ExternalEffectFact {
                tool_name: candidate.name,
                outcome,
                argument_digest: candidate.digest,
                selection_revision: 0,
                turn_id: preview(&self.turn_id),
                observed_at: chrono::Utc::now().to_rfc3339(),
            });
        }
        if outcome == EffectOutcome::Completed {
            if let Some(path) = candidate.written_path {
                observed.evidence.push(EvidenceFact::WrittenFile { path });
            }
        }
    }
}

pub(crate) fn digest(arguments: &Value) -> String {
    let mut canonical = arguments.clone();
    canonical.sort_all_objects();
    format!(
        "sha256:{:x}",
        Sha256::digest(canonical.to_string().as_bytes())
    )
}

pub(crate) fn failure_outcome(detail: &str) -> EffectOutcome {
    let detail = detail.to_ascii_lowercase();
    if detail.contains("cancelled") || detail.contains("canceled") {
        EffectOutcome::Cancelled
    } else if detail.contains("denied") && !detail.contains("(os error") {
        EffectOutcome::Denied
    } else {
        EffectOutcome::Failed
    }
}

pub(crate) fn record_artifacts(
    observation: &Arc<Mutex<MethodObservation>>,
    result: &crate::runtime::AgentRunResult,
) {
    for artifact in &result.generated_artifacts {
        lock(observation)
            .evidence
            .push(EvidenceFact::GeneratedArtifact {
                reference: preview(artifact.media_root_relative_path.as_str()),
            });
    }
}
