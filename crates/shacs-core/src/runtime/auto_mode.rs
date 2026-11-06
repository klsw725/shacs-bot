#[path = "auto_mode/action_materials.rs"]
pub(crate) mod action_materials;
#[path = "auto_mode/action_secrets.rs"]
pub(crate) mod action_secrets;
#[path = "auto_mode/approval_why.rs"]
pub(crate) mod approval_why;
#[path = "auto_mode/auto_decision.rs"]
pub(crate) mod auto_decision;
#[path = "auto_mode/block_counts.rs"]
pub(crate) mod block_counts;
#[path = "auto_mode/blocked_call.rs"]
pub(crate) mod blocked_call;
#[path = "auto_mode/classifier_request.rs"]
mod classifier_request;
#[path = "auto_mode/classifier_review.rs"]
pub(crate) mod classifier_review;
#[path = "auto_mode/classifier_slots.rs"]
mod classifier_slots;
#[path = "auto_mode/classifier_stage.rs"]
mod classifier_stage;
#[path = "auto_mode/critical_path.rs"]
mod critical_path;
#[path = "auto_mode/exec_pattern.rs"]
mod exec_pattern;
#[path = "auto_mode/host_pattern.rs"]
mod host_pattern;
#[path = "auto_mode/path_pattern.rs"]
mod path_pattern;
#[path = "auto_mode/permission_environment.rs"]
mod permission_environment;
#[path = "auto_mode/permission_materials.rs"]
mod permission_materials;
#[path = "auto_mode/permission_rule.rs"]
mod permission_rule;
#[path = "auto_mode/protected_path.rs"]
mod protected_path;
#[path = "auto_mode/real_path.rs"]
mod real_path;
#[path = "auto_mode/report_review.rs"]
pub(crate) mod report_review;
#[path = "auto_mode/result_check.rs"]
pub(crate) mod result_check;
#[path = "auto_mode/retry_approval.rs"]
pub(crate) mod retry_approval;
#[path = "auto_mode/rule_match.rs"]
mod rule_match;
#[path = "auto_mode/secret_file.rs"]
mod secret_file;
#[path = "auto_mode/secret_values.rs"]
mod secret_values;
#[path = "auto_mode/shell_command.rs"]
mod shell_command;
#[path = "auto_mode/shell_paths.rs"]
mod shell_paths;
#[path = "auto_mode/shell_token.rs"]
mod shell_token;

pub use approval_why::ApprovalExplanation;
pub use block_counts::{AutoModeBlockCounts, AutoModeBlocks, AutoModeEscalation};
pub use classifier_request::ClassifierContextFiles;
pub use classifier_review::AutoModeClassifier;
pub use classifier_slots::{AutoModeSlots, SlotOverrides, DEFAULTS_MARKER};
pub use permission_environment::{PermissionEnvironment, PermissionEnvironmentInput};
pub use permission_materials::{AutoAllow, PermissionMaterials};
pub use permission_rule::{
    RawRuleLists, RuleDiagnostic, RuleEffect, RuleProblem, UserPermissionRules,
};
pub use secret_values::SecretValues;
