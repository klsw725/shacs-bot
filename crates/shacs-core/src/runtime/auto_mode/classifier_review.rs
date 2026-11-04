//! Connects the classifier to a run (Spec 048 PRD 000 scope 10): the agent
//! loop prepares it once per turn, and the runner calls it only for actions
//! that reach row 16.

use super::classifier_request::{self, ClassifierContextFiles};
use super::classifier_slots::AutoModeSlots;
use super::classifier_stage::{self, ClassifierAttempt, ClassifierCall};
use super::permission_environment::PermissionEnvironment;
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
}

impl<'a> AutoModeClassifier<'a> {
    pub fn new(
        client: &'a dyn ProviderClient,
        invocation: ProviderInvocation,
        model: impl Into<String>,
        slots: &AutoModeSlots,
        files: &ClassifierContextFiles,
    ) -> Self {
        Self {
            client,
            invocation,
            model: model.into(),
            system: classifier_request::system_message(slots, files),
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

pub(crate) fn turn_classifier<'c>(
    input: TurnClassifierInput<'_, 'c>,
) -> Option<AutoModeClassifier<'c>> {
    let environment = input.environment?;
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
    let payload = classifier_request::payload(messages, pending_call_id, action, registry);
    let attempt = classifier_stage::classify(&ClassifierCall {
        client: classifier.client,
        invocation: &classifier.invocation,
        model: &classifier.model,
        system: &classifier.system,
        payload: &payload,
        now_unix_ms,
    });
    ClassifierReview { attempt, payload }
}
