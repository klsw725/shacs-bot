//! Spec 048 PRD 000 acceptance: the auto mode decision order, the rules every
//! mode gains, the process gate parity and the transcript classifier. PRD
//! 010 acceptance: the `.env.json` secret files and value masking. PRD 020
//! acceptance: blocks returned as tool results, block counts, escalation and
//! approvals from the recent list.

#[path = "spec048_auto_mode/blocks.rs"]
mod blocks;
#[path = "spec048_auto_mode/classifier.rs"]
mod classifier;
#[path = "spec048_auto_mode/decision_table.rs"]
mod decision_table;
#[path = "spec048_auto_mode/env_masking.rs"]
mod env_masking;
#[path = "spec048_auto_mode/env_secret_files.rs"]
mod env_secret_files;
#[path = "spec048_auto_mode/harness.rs"]
mod harness;
#[path = "spec048_auto_mode/loop_blocks.rs"]
mod loop_blocks;
#[path = "spec048_auto_mode/other_modes.rs"]
mod other_modes;
#[path = "spec048_auto_mode/paths.rs"]
mod paths;
#[path = "spec048_auto_mode/secret_files.rs"]
mod secret_files;
#[path = "spec048_auto_mode/support.rs"]
mod support;
