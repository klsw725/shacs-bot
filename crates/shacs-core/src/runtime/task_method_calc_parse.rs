use super::task_method_calc::{
    bounded, CalcResult, RunRole, RunStatus, FIELD_LIMIT, LIST_LIMIT, MARKER,
};
use serde_json::Value;

pub(crate) fn status(output: &str) -> (RunStatus, Option<i32>) {
    let head = output.trim_start();
    if head.starts_with("[Tool call skipped") {
        return (RunStatus::Cancelled, None);
    }
    if head.starts_with("Error") || head.starts_with("Permission denied") {
        let lower = head.to_ascii_lowercase();
        let status = if lower.contains("cancelled") || lower.contains("canceled") {
            RunStatus::Cancelled
        } else if lower.contains("denied") || lower.contains("blocked") {
            RunStatus::Denied
        } else {
            RunStatus::Failed
        };
        return (status, None);
    }
    let code = output.lines().rev().find_map(|line| {
        line.trim()
            .strip_prefix("Exit code:")
            .and_then(|value| value.trim().parse::<i32>().ok())
    });
    match code {
        Some(0) => (RunStatus::Succeeded, Some(0)),
        Some(code) => (RunStatus::NonZeroExit, Some(code)),
        None => (RunStatus::Failed, None),
    }
}

pub(crate) fn marker(output: &str) -> (Option<CalcResult>, Option<String>) {
    let lines: Vec<&str> = output
        .lines()
        .filter_map(|line| line.trim().strip_prefix(MARKER))
        .collect();
    match lines.as_slice() {
        [] => (None, None),
        [line] => match parse(line.trim()) {
            Ok(result) => (Some(result), None),
            Err(error) => (None, Some(error.to_owned())),
        },
        _ => (
            None,
            Some(
                "more than one result line in one run; each run must report one result".to_owned(),
            ),
        ),
    }
}

fn parse(json: &str) -> Result<CalcResult, &'static str> {
    let value: Value = serde_json::from_str(json).map_err(|_| "result line is not valid JSON")?;
    let role = match value.get("role").and_then(Value::as_str) {
        Some("primary") => RunRole::Primary,
        Some("check") => RunRole::Check,
        _ => return Err("result role must be \"primary\" or \"check\""),
    };
    let answer = match value.get("answer") {
        Some(Value::String(text)) if !text.trim().is_empty() => text.trim().to_owned(),
        Some(Value::Number(number)) => number.to_string(),
        _ => return Err("result answer is missing"),
    };
    // The runtime states the checked answer and unit itself, so they must be a
    // value and a unit, never text of the model's own.
    if !(number(&answer)
        || answer
            .split_once('/')
            .is_some_and(|(a, b)| number(a) && number(b)))
    {
        return Err("result answer must be a number, such as 6, -1.5e3, 6,000 or 1/3");
    }
    let unit = value
        .get("unit")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if unit.chars().count() > 24
        || unit
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, '[' | ']'))
    {
        return Err("result unit must be one short word, such as m^2 or km/h");
    }
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(|text| bounded(text.trim(), FIELD_LIMIT))
            .unwrap_or_default()
    };
    let inputs = match value.get("inputs") {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(name, value)| match value {
                Value::String(text) => format!("{name} = {text}"),
                other => format!("{name} = {other}"),
            })
            .collect(),
        Some(Value::Array(items)) => items.iter().map(entry).collect(),
        _ => Vec::new(),
    };
    let assumptions = match value.get("assumptions") {
        Some(Value::Array(items)) => items.iter().map(entry).collect(),
        Some(Value::String(text)) => vec![text.clone()],
        _ => Vec::new(),
    };
    let list = |items: Vec<String>| -> Vec<String> {
        items
            .into_iter()
            .take(LIST_LIMIT)
            .map(|item| bounded(&item, FIELD_LIMIT))
            .collect()
    };
    Ok(CalcResult {
        role,
        answer: bounded(&answer, FIELD_LIMIT),
        unit: text("unit"),
        inputs: list(inputs),
        assumptions: list(assumptions),
        method: text("method"),
    })
}

/// A plain number: an optional sign, digits (`,` or `_` may group them), an
/// optional fraction and an optional exponent.
fn number(text: &str) -> bool {
    let digits = |part: &str| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit());
    let text = text.strip_prefix(['-', '+']).unwrap_or(text);
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map_or((text, None), |(m, e)| (m, Some(e)));
    let (whole, fraction) = mantissa
        .split_once('.')
        .map_or((mantissa, None), |(w, f)| (w, Some(f)));
    whole.starts_with(|ch: char| ch.is_ascii_digit())
        && whole
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, ',' | '_'))
        && fraction.map_or(true, digits)
        && exponent.map_or(true, |e| digits(e.strip_prefix(['-', '+']).unwrap_or(e)))
}

fn entry(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
