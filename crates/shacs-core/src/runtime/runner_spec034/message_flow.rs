use super::media_result::has_generated_artifacts;
use crate::runtime::AgentRunSpec;
use serde_json::Value;
use std::panic::{catch_unwind, AssertUnwindSafe};

const MAX_INJECTION_CYCLES: usize = 5;

pub(crate) fn append_mid_turn_injections(
    spec: &AgentRunSpec<'_>,
    messages: &mut Vec<Value>,
    had_injections: &mut bool,
    injection_cycles: &mut usize,
) -> bool {
    if has_generated_artifacts() || *injection_cycles >= MAX_INJECTION_CYCLES {
        return false;
    }
    let injections = spec
        .mid_turn_injection_callback
        .as_ref()
        .map(|callback| {
            catch_unwind(AssertUnwindSafe(|| callback()))
                .ok()
                .unwrap_or_default()
        })
        .unwrap_or_default();
    if injections.is_empty() {
        return false;
    }
    *injection_cycles += 1;
    *had_injections = true;
    messages.extend(injections);
    true
}
