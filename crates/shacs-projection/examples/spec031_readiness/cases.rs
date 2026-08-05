use crate::spec031_readiness_support::*;
use shacs_projection::*;
use std::error::Error;

pub fn evidence_json() -> Result<serde_json::Value, Box<dyn Error>> {
    Ok(serde_json::json!({
        "schema": "spec031_prd003_readiness_task10",
        "truth_table": {
            "required_blocked": "blocked",
            "required_unknown_or_unavailable": "unknown",
            "required_or_optional_degraded": "degraded",
            "optional_blocked": "degraded",
            "optional_unknown_or_unavailable": "ready_when_required_ready",
            "stale_ready": "degraded"
        },
        "cases": case_jsons()?,
        "malformed": duplicate_jsons()?
    }))
}

fn case_jsons() -> Result<Vec<serde_json::Value>, Box<dyn Error>> {
    let cases = [
        scenario("ready", ready_required()?),
        replace("absent_credentials", provider_auth_missing()?)?,
        replace("blocked_migration_storage", storage_blocked()?)?,
        replace("unknown_containment", containment_unknown()?)?,
        replace("failed_channel", channel_blocked()?)?,
        replace("disabled_plugin", plugin_disabled()?)?,
        replace("missing_app_owner", app_owner_missing()?)?,
        replace("queue_admission_block", queue_blocked_zero_missing()?)?,
        replace("stale_ready", provider_stale_ready()?)?,
        with_optional(
            "optional_unknown",
            optional_state(Spec031Availability::Unknown)?,
        )?,
        with_optional(
            "optional_unavailable",
            optional_state(Spec031Availability::Unavailable)?,
        )?,
        with_optional(
            "optional_degraded",
            optional_state(Spec031Availability::Degraded)?,
        )?,
        with_optional(
            "optional_blocked",
            optional_state(Spec031Availability::Blocked)?,
        )?,
        replace("explicit_zero_vs_missing", queue_blocked_zero_missing()?)?,
    ];
    cases.iter().map(scenario_json).collect()
}

fn duplicate_jsons() -> Result<Vec<serde_json::Value>, Box<dyn Error>> {
    Ok(vec![
        duplicate_json(
            "duplicate_required",
            storage_ready("storage ready one")?,
            storage_blocked()?,
        )?,
        duplicate_json(
            "optional_duplicate",
            optional_state(Spec031Availability::Degraded)?,
            optional_state(Spec031Availability::Blocked)?,
        )?,
        duplicate_json(
            "conflicting_requirement_duplicate",
            storage_ready("storage required")?,
            optional_storage_degraded()?,
        )?,
    ])
}

struct Scenario {
    label: &'static str,
    observations: Vec<Spec031ReadinessObservation>,
}

fn scenario(label: &'static str, observations: Vec<Spec031ReadinessObservation>) -> Scenario {
    Scenario {
        label,
        observations,
    }
}

fn replace(
    label: &'static str,
    replacement: Spec031ReadinessObservation,
) -> Result<Scenario, Spec031ConstructionError> {
    let mut observations = ready_required()?;
    observations.retain(|observation| observation.kind != replacement.kind);
    observations.push(replacement);
    Ok(scenario(label, observations))
}

fn with_optional(
    label: &'static str,
    optional: Spec031ReadinessObservation,
) -> Result<Scenario, Spec031ConstructionError> {
    let mut observations = ready_required()?;
    observations.push(optional);
    Ok(scenario(label, observations))
}

fn scenario_json(scenario: &Scenario) -> Result<serde_json::Value, Box<dyn Error>> {
    let report = spec031_aggregate_readiness(&scenario.observations)?;
    let parsed = Spec031Envelope::parse_json(&serde_json::to_string(report.envelope())?)?;
    if parsed != *report.envelope() {
        return Err("readiness envelope roundtrip changed state".into());
    }
    Ok(serde_json::json!({
        "label": scenario.label,
        "state": report.envelope().state(),
        "severity": report.envelope().severity(),
        "reason": report.envelope().reason().code,
        "freshness": report.envelope().source().freshness,
        "component_count": report.components().len(),
        "components": component_jsons(report.components()),
    }))
}

fn component_jsons(components: &[Spec031ReadinessObservation]) -> Vec<serde_json::Value> {
    components
        .iter()
        .map(|component| {
            serde_json::json!({
                "kind": component.kind,
                "requirement": component.requirement,
                "state": component.state,
                "freshness": component.freshness,
                "reason": component.reason_code,
                "queue_depth": component.queue_depth,
                "queue_capacity": component.queue_capacity,
            })
        })
        .collect()
}

fn duplicate_json(
    label: &'static str,
    first: Spec031ReadinessObservation,
    second: Spec031ReadinessObservation,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let error = spec031_aggregate_readiness(&[first, second])
        .err()
        .ok_or("duplicate readiness components unexpectedly aggregated successfully")?;
    let Spec031ReadinessAggregationError::DuplicateComponent { kind } = error else {
        return Err("duplicate readiness error used the wrong variant".into());
    };
    Ok(serde_json::json!({
        "label": label,
        "error": "duplicate_component",
        "kind": kind,
    }))
}
