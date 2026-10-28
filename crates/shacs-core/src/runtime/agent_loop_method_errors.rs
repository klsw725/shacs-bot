//! What an image or composed request shows of an error that ends its turn
//! and may quote the model: the output of a call that ran after an approval,
//! or a provider's error (which may name a tool or call id the model wrote).
//! Only digests show (see `quiet_tools`).

use super::{reported, RuntimeExecutionLedger, Session};
use crate::runtime::quiet_tools::quiet_text;
use serde_json::Value;
use shacs_providers::ProviderError;

/// The answer when a call that ran after an approval stops the turn (its
/// output, `error`, ends the turn as it is otherwise): in an image or composed
/// request that output may hold the model's words, so only its digest shows.
pub(in crate::runtime::agent_loop) fn quiet_failure(
    session: &Session,
    error: String,
) -> std::io::Result<String> {
    if reported(session)?.is_none() {
        return Ok(error);
    }
    Ok(format!(
        "The approved tool call stopped this turn; its output is withheld ({}).",
        quiet_text(&error)
    ))
}

/// The answer for a provider call that failed in an image or composed
/// request: a provider's error message may quote what the model wrote (a tool
/// name or call id), so only its digest shows. `None` for other requests and
/// for errors that name only configured providers and models.
pub(in crate::runtime::agent_loop) fn quiet_provider_error(
    session: &Session,
    error: &ProviderError,
) -> std::io::Result<Option<String>> {
    let ProviderError::Api {
        status, message, ..
    } = error
    else {
        return Ok(None);
    };
    if reported(session)?.is_none() {
        return Ok(None);
    }
    Ok(Some(withheld(*status, message)))
}

/// The same for a run that ended on an error it returns as its final content:
/// a provider's error response (the way providers report HTTP and stream
/// errors, also once retries run out), or a tool's output that stopped the
/// turn (as [`quiet_failure`] does for one that ran after an approval).
pub(in crate::runtime::agent_loop) fn quiet_run_error(
    session: &Session,
    result: &mut crate::runtime::AgentRunResult,
) -> std::io::Result<()> {
    let tool = match result.stop_reason.as_str() {
        "error" => false,
        "tool_error" => true,
        _ => return Ok(()),
    };
    if reported(session)?.is_none() {
        return Ok(());
    }
    let error = result.error.as_deref().or(result.final_content.as_deref());
    let error = error.unwrap_or_default();
    let text = if tool {
        format!(
            "A tool call stopped this turn; its output is withheld ({}).",
            quiet_text(error)
        )
    } else {
        withheld(
            result.error_status,
            error.strip_prefix("Error: ").unwrap_or(error),
        )
    };
    result.error = Some(text.clone());
    result.error_message = Some(text.clone());
    result.final_content = Some(text);
    Ok(())
}

fn withheld(status: Option<u16>, message: &str) -> String {
    let status = status
        .map(|status| format!(" (HTTP {status})"))
        .unwrap_or_default();
    format!(
        "Sorry, I encountered an error calling the AI model: provider API error{status}; its message is withheld ({}).",
        quiet_text(message)
    )
}

/// Withholds, the same way, the error messages that turn `turn_id`'s failed
/// provider calls left in the execution ledger.
pub(in crate::runtime::agent_loop) fn quiet_provider_details(
    ledger: &mut RuntimeExecutionLedger,
    turn_id: &str,
) {
    for record in &mut ledger.outcomes {
        let fact = &mut record.fact;
        let provider = matches!(fact.outcome, crate::runtime::ExecutionOutcome::Provider(_));
        if !provider || fact.identity.scope.turn_id != turn_id {
            continue;
        }
        let detail = fact
            .detail
            .as_deref()
            .and_then(|detail| serde_json::from_str(detail).ok());
        if let Some(mut detail @ Value::Object(_)) = detail {
            if let Some(message) = detail
                .get("message")
                .and_then(Value::as_str)
                .map(quiet_text)
            {
                detail["message"] = Value::String(message);
                fact.detail = Some(detail.to_string());
            }
        }
    }
}
