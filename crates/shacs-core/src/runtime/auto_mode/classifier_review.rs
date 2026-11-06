//! Connects the classifier to a run (Spec 048 PRD 000 scope 10): the agent
//! loop prepares it once per turn, and the runner calls it only for actions
//! that reach row 16. A subagent run gets its own, which also reviews the
//! finished work (PRD 030 scopes 2 and 4).

use super::classifier_request::{self, ClassifierContextFiles};
use super::classifier_slots::AutoModeSlots;
use super::classifier_stage::{
    self, ClassifierAttempt, ClassifierCall, ACTION_STAGES, REVIEW_STAGES,
};
use super::permission_environment::PermissionEnvironment;
use super::secret_values::SecretValues;
use crate::runtime::context::BOOTSTRAP_FILES;
use crate::runtime::{
    discover_context_files, ContextBuilder, ContextFileDiscoveryOptions, ContextFileReadStatus,
    ContextFileSource, ContextProviderHandoff, PermissionedAction,
};
use crate::tools::ToolRegistry;
use serde_json::Value;
use shacs_providers::{ProviderClient, ProviderInvocation};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const CONTEXT_FILE_LABEL: &str = "context-file:";

#[derive(Clone)]
pub struct AutoModeClassifier<'a> {
    pub client: &'a dyn ProviderClient,
    pub invocation: ProviderInvocation,
    pub model: String,
    pub system: String,
    /// The `.env.json` values masked in requests (Spec 048 PRD 010 scope 10).
    pub secrets: SecretValues,
    /// The rule names an answer may cite (Spec 048 PRD 020 scope 5).
    pub rule_labels: Vec<String>,
}

impl<'a> AutoModeClassifier<'a> {
    pub fn new(
        client: &'a dyn ProviderClient,
        invocation: ProviderInvocation,
        model: impl Into<String>,
        slots: &AutoModeSlots,
        files: &ClassifierContextFiles,
        secrets: &SecretValues,
    ) -> Self {
        Self {
            client,
            invocation,
            model: model.into(),
            system: classifier_request::system_message(slots, files, secrets),
            secrets: secrets.clone(),
            rule_labels: slots.rule_labels(),
        }
    }
}

pub(crate) struct TurnClassifierInput<'i, 'c> {
    pub(crate) environment: Option<&'i PermissionEnvironment>,
    pub(crate) context_builder: &'i ContextBuilder,
    pub(crate) handoff: Option<&'i ContextProviderHandoff>,
    pub(crate) workspace: &'i Path,
    pub(crate) conversation: String,
    pub(crate) client: &'c dyn ProviderClient,
    pub(crate) invocation: &'i ProviderInvocation,
    pub(crate) run_model: &'i str,
}

/// `None` leaves row 16 without a verdict: no permission environment, or
/// `.env.json` could not be read for this turn so its values may not all be
/// masked (Spec 048 PRD 010 scope 10).
pub(crate) fn turn_classifier<'c>(
    input: TurnClassifierInput<'_, 'c>,
) -> Option<AutoModeClassifier<'c>> {
    let environment = input
        .environment
        .filter(|environment| environment.secret_values_current)?;
    let mut instructions = input.context_builder.bootstrap_files();
    instructions.extend(
        input
            .handoff
            .into_iter()
            .flat_map(|handoff| &handoff.blocks)
            .filter_map(|block| {
                let label = block.source_label.strip_prefix(CONTEXT_FILE_LABEL)?;
                Some((label.to_owned(), block.content.clone()))
            }),
    );
    let files = ClassifierContextFiles {
        instructions,
        skills: input.context_builder.build_skills_summary(&BTreeSet::new()),
    };
    let slots = filled_slots(environment, input.workspace, &input.conversation);
    let model = environment
        .classifier_model
        .as_deref()
        .unwrap_or(input.run_model);
    Some(AutoModeClassifier::new(
        input.client,
        input.invocation.clone(),
        model,
        &slots,
        &files,
        &environment.secret_values,
    ))
}

/// What a subagent run's classifier is made from (Spec 048 PRD 030 scope 2).
pub(crate) struct ChildClassifierInput<'i, 'c> {
    pub(crate) environment: Option<&'i PermissionEnvironment>,
    /// The builder of the subagent's own system prompt.
    pub(crate) context_builder: &'i ContextBuilder,
    pub(crate) workspace: &'i Path,
    pub(crate) extra_context_files: &'i [PathBuf],
    pub(crate) conversation: String,
    pub(crate) client: &'c dyn ProviderClient,
    pub(crate) invocation: &'i ProviderInvocation,
    pub(crate) run_model: &'i str,
}

/// A subagent run's classifier for its calls, and the system message that
/// reviews its finished work with the same slots, files and skills.
pub(crate) struct ChildClassifier<'c> {
    pub(crate) classifier: AutoModeClassifier<'c>,
    pub(crate) review_system: String,
}

/// `None` leaves the subagent's row 16 without a verdict, as for a turn.
pub(crate) fn child_classifier<'c>(
    input: ChildClassifierInput<'_, 'c>,
) -> Option<ChildClassifier<'c>> {
    let environment = input
        .environment
        .filter(|environment| environment.secret_values_current)?;
    let mut instructions = input.context_builder.bootstrap_files();
    instructions.extend(discovered_context_files(
        input.workspace,
        input.extra_context_files,
    ));
    let files = ClassifierContextFiles {
        instructions,
        skills: input.context_builder.build_skills_summary(&BTreeSet::new()),
    };
    let slots = filled_slots(environment, input.workspace, &input.conversation);
    let model = environment
        .classifier_model
        .as_deref()
        .unwrap_or(input.run_model);
    Some(ChildClassifier {
        review_system: classifier_request::review_system_message(
            &slots,
            &files,
            &environment.secret_values,
        ),
        classifier: AutoModeClassifier::new(
            input.client,
            input.invocation.clone(),
            model,
            &slots,
            &files,
            &environment.secret_values,
        ),
    })
}

/// The context files a turn adds for this workspace: the current folder's
/// and the configured extra files, without the workspace root's bootstrap
/// files, which come separately.
fn discovered_context_files(workspace: &Path, extra: &[PathBuf]) -> Vec<(String, String)> {
    discover_context_files(
        workspace,
        ContextFileDiscoveryOptions {
            current_dir: std::env::current_dir().ok(),
            extra_context_files: extra.to_vec(),
            ..ContextFileDiscoveryOptions::default()
        },
    )
    .entries
    .into_iter()
    .filter(|entry| {
        matches!(
            entry.status,
            ContextFileReadStatus::Included | ContextFileReadStatus::Truncated
        )
    })
    .filter(|entry| {
        !(entry.source == ContextFileSource::DefaultCandidate
            && entry.source_directory_depth == 0
            && BOOTSTRAP_FILES.contains(&entry.filename.as_str()))
    })
    .filter_map(|entry| Some((entry.path.display().to_string(), entry.content?)))
    .collect()
}

fn filled_slots(
    environment: &PermissionEnvironment,
    workspace: &Path,
    conversation: &str,
) -> AutoModeSlots {
    let workspace = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    environment
        .slots
        .filled(&workspace.display().to_string(), conversation)
}

pub(crate) struct ClassifierReview {
    pub(crate) attempt: ClassifierAttempt,
    pub(crate) payload: Value,
}

pub(crate) fn review(
    classifier: &AutoModeClassifier<'_>,
    messages: &[Value],
    pending_call_id: &str,
    action: &PermissionedAction,
    registry: &ToolRegistry,
    now_unix_ms: &dyn Fn() -> u64,
) -> ClassifierReview {
    let payload = classifier_request::payload(
        messages,
        pending_call_id,
        action,
        registry,
        &classifier.secrets,
    );
    let attempt = classifier_stage::classify(&ClassifierCall {
        client: classifier.client,
        invocation: &classifier.invocation,
        model: &classifier.model,
        system: &classifier.system,
        payload: &payload,
        stages: &ACTION_STAGES,
        rule_labels: &classifier.rule_labels,
        now_unix_ms,
    });
    ClassifierReview { attempt, payload }
}

/// Reviews a subagent's finished work before its report is handed back
/// (Spec 048 PRD 030 scope 4).
pub(crate) fn review_report(
    child: &ChildClassifier<'_>,
    task: &str,
    messages: &[Value],
    report: &str,
    registry: &ToolRegistry,
    now_unix_ms: &dyn Fn() -> u64,
) -> ClassifierReview {
    let classifier = &child.classifier;
    let payload =
        classifier_request::review_payload(task, messages, report, registry, &classifier.secrets);
    let attempt = classifier_stage::classify(&ClassifierCall {
        client: classifier.client,
        invocation: &classifier.invocation,
        model: &classifier.model,
        system: &child.review_system,
        payload: &payload,
        stages: &REVIEW_STAGES,
        rule_labels: &classifier.rule_labels,
        now_unix_ms,
    });
    ClassifierReview { attempt, payload }
}
