//! Spec 048 PRD 000 acceptance: the auto mode decision order, the rules every
//! mode gains, the process gate parity and the transcript classifier.

#[path = "spec048_auto_mode/classifier.rs"]
mod classifier;
#[path = "spec048_auto_mode/decision_table.rs"]
mod decision_table;
#[path = "spec048_auto_mode/other_modes.rs"]
mod other_modes;
#[path = "spec048_auto_mode/paths.rs"]
mod paths;
#[path = "spec048_auto_mode/secret_files.rs"]
mod secret_files;
#[path = "spec048_auto_mode/support.rs"]
mod support;
