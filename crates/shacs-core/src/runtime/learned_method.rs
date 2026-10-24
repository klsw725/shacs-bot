//! Spec038 learned methods: reusable working procedures proposed from one
//! conversation's evidence. A stored method keeps digests and source
//! references, never the raw request, tool output, or transcript.

use serde::{Deserialize, Serialize};

pub const LEARNED_METHOD_SCHEMA: &str = "shacs.learned-methods.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearnedMethod {
    pub method_id: String,
    pub state: LearnedMethodState,
    /// Increments on every change of this method.
    pub revision: u64,
    pub current_version: u32,
    pub created_at: String,
    pub versions: Vec<LearnedMethodVersion>,
    /// What the user and later tasks decided about the method, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<MethodDecision>,
}

impl LearnedMethod {
    pub fn current(&self) -> Option<&LearnedMethodVersion> {
        self.versions
            .iter()
            .find(|version| version.version == self.current_version)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnedMethodState {
    /// Proposed only: never used by default and grants no permission.
    Candidate,
    /// Confirmed by the user, or redone in two different tasks: may be
    /// offered as the default for the tasks it applies to. Grants no
    /// permission.
    Confirmed,
    /// Rejected by the user: never offered.
    Rejected,
    /// Confirmed, then contradicted by the user's feedback or put out of
    /// use by the user: not offered until the user confirms it again.
    Inactive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodDecision {
    pub kind: DecisionKind,
    /// The version decided on.
    pub version: u32,
    pub at: String,
    /// Digest of the key of the session the decision was made in.
    pub session: String,
    /// The code of the review a confirmation was made from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
    /// What a check against a later task found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<TaskCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Confirmed,
    Rejected,
    /// A later, different task of the kind the method applies to redid
    /// every supported step.
    Verified,
    /// The user's feedback, in any task checked, contradicted a step.
    Conflicted,
    /// A rerun of a task already recorded (its request or material again,
    /// or for a method untrusted material influenced, its source again):
    /// kept only as evidence of which steps stayed the same; never counts.
    Repeated,
    /// The user's edit made this version from the version before it; the
    /// new version is current and starts as a candidate.
    Edited,
    /// The user put a confirmed version out of use.
    Deactivated,
    /// The user made this earlier confirmed version current again.
    Restored,
}

/// A later task checked against a method, identified by digests only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCheck {
    pub task: TaskIdentity,
    /// Which evidence redid which step (or, for a conflict, contradicted it).
    pub steps: Vec<StepCheck>,
    pub evidence: Vec<MethodEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepCheck {
    /// The step's number, from 1.
    pub step: usize,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearnedMethodVersion {
    pub version: u32,
    pub created_at: String,
    pub origin: VersionOrigin,
    pub purpose: CitedText,
    pub applies_to: CitedText,
    pub premises: Vec<CitedText>,
    pub steps: Vec<MethodStep>,
    pub unverified: Vec<String>,
    pub excluded: Vec<ExcludedEvidence>,
    pub evidence: Vec<MethodEvidence>,
}

/// The model's wording with the evidence it cites, or the user's edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CitedText {
    pub text: String,
    pub evidence: Vec<String>,
    /// The version whose edit by the user wrote this text; it cites nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_in: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionOrigin {
    pub kind: OriginKind,
    /// Digest of the session key; the key itself may carry identifiers.
    pub session: String,
    /// Evidence items left out of the bounded window.
    pub omitted_evidence: usize,
    /// The task the version came from: the whole window, not only the
    /// evidence kept.
    #[serde(default)]
    pub task: TaskIdentity,
    /// For a version the user's edit made, the version it was made from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_from: Option<u32>,
}

/// What tells one task from another, by digests only: a task with any of
/// them in common with another is the same task.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskIdentity {
    /// The user's requests, by text: the same request asked again.
    pub requests: Vec<String>,
    /// Every message of the user, by text and time: the same conversation,
    /// however its messages were sorted.
    pub messages: Vec<String>,
    /// What the task's tool calls and attachments brought in.
    pub materials: Vec<String>,
    /// The arguments of the tool calls that brought material in.
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginKind {
    ToolLessGeneration,
    /// The user's edit of an earlier version; its evidence is that version's.
    UserEdit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodStep {
    /// The user's own words for `UserStated`, a runtime label of the cited
    /// runs for `ObservedRun`, otherwise the model's wording.
    pub text: String,
    pub evidence: Vec<String>,
    /// Computed from the cited evidence by the runtime, not by the model.
    pub support: StepSupport,
    /// The model's description of an observed run; never checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_note: Option<String>,
    /// For `UserEdited`, the version whose edit wrote the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_in: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepSupport {
    /// A cited tool call ran without a reported error; this says nothing
    /// about whether the step caused a good result.
    ObservedRun,
    /// The step repeats, word for word, a message in which the user asked
    /// for, described, or judged the work.
    UserStated,
    /// The user wrote the step with `/learn edit`; it cites nothing.
    UserEdited,
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludedEvidence {
    pub evidence: String,
    pub kind: UserStatement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodEvidence {
    pub id: String,
    pub kind: EvidenceKind,
    pub source_ref: String,
    /// When the source message was written; with the digest it tells the
    /// original message apart from a later one with the same text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    pub trust: SourceTrust,
    /// Digest of the message text or of the tool call arguments.
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classified_as: Option<UserStatement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<RunOutcome>,
    /// Digest of what the call or message brought in (tool output or
    /// attachments); that material is always untrusted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    UserMessage,
    ToolCall,
    Answer,
}

/// Trust of the evidence content, following the Spec024 quarantine split:
/// external material and model output produced after reading it are
/// untrusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTrust {
    Owner,
    Model,
    Untrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserStatement {
    Request,
    Feedback,
    Procedure,
    Preference,
    TaskCondition,
    /// A preference or one-task condition mixed with anything else.
    Mixed,
    #[serde(other)]
    Other,
}

impl UserStatement {
    /// Kept out of the method and never shown to the step generator.
    pub fn excluded(self) -> bool {
        matches!(self, Self::Preference | Self::TaskCondition | Self::Mixed)
    }

    /// The only messages whose text the step generator sees.
    pub fn shown(self) -> bool {
        matches!(self, Self::Request | Self::Feedback | Self::Procedure)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Ok,
    Error,
    Incomplete,
}
