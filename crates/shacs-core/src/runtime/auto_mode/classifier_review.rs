//! Connects the classifier to a run (Spec 048 PRD 000 scope 10): the agent
//! loop prepares it once per turn, and the runner calls it only for actions
//! that reach row 16.

use super::classifier_request::{self, ClassifierContextFiles};
use super::classifier_slots::AutoModeSlots;
use super::classifier_stage::{self, ClassifierAttempt, ClassifierCall};
use super::permission_environment::PermissionEnvironment;
use super::secret_values::SecretValues;
use crate::runtime::{ContextBuilder, ContextProviderHandoff, PermissionedAction};
use crate::tools::ToolRegistry;
use serde_json::Value;
use shacs_providers::{ProviderClient, ProviderInvocation};
use std::collections::BTreeSet;
use std::path::Path;

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
    let workspace =
        std::fs::canonicalize(input.workspace).unwrap_or_else(|_| input.workspace.to_path_buf());
    let slots = environment
        .slots
        .filled(&workspace.display().to_string(), &input.conversation);
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
        rule_labels: &classifier.rule_labels,
        now_unix_ms,
    });
    ClassifierReview { attempt, payload }
}
