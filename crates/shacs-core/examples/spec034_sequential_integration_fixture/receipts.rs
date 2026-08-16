use super::receipt_inputs::ReceiptInputs;
use super::receipt_model::{all_observed, ObservedReceipt, ReceiptDraft};
use super::receipts_expanded as expanded;
use serde_json::json;
use shacs_projection::Spec034PrimaryPrd;
use std::error::Error;

macro_rules! observed {
    ($rows:ident, $id:literal, $name:literal, $prd:ident, $source:literal, $value:expr, $condition:expr) => {
        $rows.push(ObservedReceipt::from_observation(ReceiptDraft {
            requirement_id: $id,
            name: $name,
            primary_prd: Spec034PrimaryPrd::$prd,
            production_source: $source,
            observable: $value,
            observed: $condition,
        })?);
    };
}

macro_rules! expanded_observed {
    ($rows:ident, $input:ident, $id:literal, $name:literal, $prd:ident, $source:literal, $builder:ident) => {{
        let observations = expanded::$builder(&$input);
        observed!(
            $rows,
            $id,
            $name,
            $prd,
            $source,
            json!({"sub_observations": &observations}),
            all_observed(&observations)
        );
    }};
}

pub fn build(input: ReceiptInputs<'_>) -> Result<Vec<ObservedReceipt>, Box<dyn Error>> {
    let mut rows = Vec::with_capacity(22);
    observed!(
        rows,
        "034-MH001",
        "codex_event_to_artifact",
        Prd000,
        "shacs_providers::parse_codex_media_stream -> ArtifactStore::persist",
        json!({"candidate_count": input.lifecycle.final_candidate_count, "artifact_id": input.artifact.artifact_id.as_str()}),
        input.lifecycle.final_candidate_count == 1 && input.artifact_record_exists
    );
    expanded_observed!(
        rows,
        input,
        "034-MH002",
        "provider_neutral_image_operations",
        Prd001,
        "ImageOperationService::execute",
        mh002
    );
    expanded_observed!(
        rows,
        input,
        "034-MH003",
        "source_replacement_revalidation",
        Prd001,
        "ImageOperationService::admit -> execute_admitted",
        mh003
    );
    observed!(
        rows,
        "034-MH004",
        "five_state_media_lifecycle",
        Prd001,
        "parse_codex_media_stream + ImageOperationLifecycle::apply",
        json!({"states": input.lifecycle.states, "failed_error_redacted": input.lifecycle.failed_error_redacted}),
        input.lifecycle.states == ["started", "partial", "final", "failed", "cancelled"]
            && input.lifecycle.failed_error_redacted
    );
    observed!(
        rows,
        "034-MH005",
        "remote_three_way_policy",
        Prd002,
        "RemoteOutputPolicy::evaluate -> ArtifactPublisher::publish_remote",
        json!({"outcomes": input.remote.outcomes}),
        input.remote.outcomes == ["persisted", "reference", "rejected"]
    );
    expanded_observed!(
        rows,
        input,
        "034-MH006",
        "committed_artifact_metadata",
        Prd002,
        "ArtifactStore::persist -> record.json",
        mh006
    );
    observed!(
        rows,
        "034-MH007",
        "diagnostics_disclosure",
        Prd002,
        "project_media_evidence_diagnostics",
        json!({"raw_content_possible": input.diagnostics.disclosure.raw_content_possible, "scan_matches": input.scan.matches}),
        input.diagnostics.disclosure.raw_content_possible && input.scan.matches.is_empty()
    );
    expanded_observed!(
        rows,
        input,
        "034-MH008",
        "video_state_vocabulary",
        Prd003,
        "project_video_analyzer",
        mh008
    );
    expanded_observed!(
        rows,
        input,
        "034-MH009",
        "injected_analyzer_owner_facts",
        Prd003,
        "project_video_analyzer -> VideoAnalyzerOwnerFactsProjection",
        mh009
    );
    observed!(
        rows,
        "034-MH010",
        "recorded_only_replay",
        Prd002,
        "replay_recorded_media_evidence",
        json!({"probe": input.replay.probe_counts, "replay": input.replay.replay_counts}),
        input.replay.probe_counts == [1, 1, 1, 1] && input.replay.replay_counts == [0, 0, 0, 0]
    );

    observed!(
        rows,
        "034-AC001",
        "media_not_text_response",
        Prd000,
        "parse_codex_media_stream ProviderEvent routing",
        json!({"text_deltas": input.lifecycle.text_delta_count, "artifact_candidates": input.lifecycle.final_candidate_count}),
        input.lifecycle.text_delta_count == 0 && input.lifecycle.final_candidate_count == 1
    );
    expanded_observed!(
        rows,
        input,
        "034-AC002",
        "operation_lineage_admission",
        Prd001,
        "ImageOperationService mask/edit/variation admission",
        ac002
    );
    observed!(
        rows,
        "034-AC003",
        "partial_final_distinction",
        Prd001,
        "parse_codex_media_stream lifecycle callback",
        json!({"ordered_states": input.lifecycle.states}),
        input.lifecycle.states[1] == "partial" && input.lifecycle.states[2] == "final"
    );
    expanded_observed!(
        rows,
        input,
        "034-AC004",
        "remote_guard_enforcement",
        Prd002,
        "NetworkGuard + RemoteOutputPolicy",
        ac004
    );
    expanded_observed!(
        rows,
        input,
        "034-AC005",
        "persistence_digest_source_chain",
        Prd002,
        "ArtifactStore::read_payload + edit output record",
        ac005
    );
    observed!(
        rows,
        "034-AC006",
        "computed_forbidden_material_scan",
        Prd002,
        "serialized diagnostics/artifacts/surfaces scan",
        json!({"inputs": input.scan.inputs, "forbidden_classes": input.scan.forbidden_classes, "matches": input.scan.matches}),
        input.scan.inputs.len() >= 8 && input.scan.matches.is_empty()
    );
    expanded_observed!(
        rows,
        input,
        "034-AC007",
        "canonical_state_surface_parity",
        Prd003,
        "project_video_analyzer + actual CLI/API/WebSocket/channel adapters",
        ac007
    );
    expanded_observed!(
        rows,
        input,
        "034-AC008",
        "codec_and_duration_bounds",
        Prd003,
        "project_video_analyzer unsupported + duration cap",
        ac008
    );
    observed!(
        rows,
        "034-AC009",
        "replay_uses_recorded_digest",
        Prd002,
        "replay_recorded_media_evidence + persisted remote artifact",
        json!({"source": input.replay.source, "artifact_count": input.replay.artifact_count, "remote_hash": input.remote.persisted_hash_consistent}),
        input.replay.source == "recorded_metadata"
            && input.replay.artifact_count >= 2
            && input.remote.persisted_hash_consistent
    );
    expanded_observed!(
        rows,
        input,
        "034-AC010",
        "typed_non_guarantee_disclosure",
        Prd004,
        "documentation-policy.json + docs/specs/README.md",
        ac010
    );
    observed!(
        rows,
        "034-AC011",
        "owner_fact_surface_field_parity",
        Prd003,
        "actual CLI/API/WebSocket/channel semantic recursive diff",
        json!({"cli": input.surfaces.cli_diff, "http": input.surfaces.http_diff, "websocket": input.surfaces.websocket_diff, "channel": input.surfaces.channel_diff}),
        input.surfaces.semantic_parity && !input.surfaces.http_websocket_raw_equal
    );
    observed!(
        rows,
        "034-AC012",
        "snapshot_replay_without_live_resolution",
        Prd003,
        "recorded snapshot replay with reset callable spies",
        json!({"snapshot_id": input.replay.snapshot_id, "live_calls": input.replay.replay_counts, "crash_recovered": input.crash.after_rename_recovered}),
        !input.replay.snapshot_id.is_empty()
            && input.replay.replay_counts == [0, 0, 0, 0]
            && input.crash.before_rename_hidden_and_clean
            && input.crash.after_rename_recovered
    );
    Ok(rows)
}
