use super::receipt_inputs::ReceiptInputs;
use super::receipt_model::{sub_observations, SubObservation};
use shacs_core::generated_media::{ProjectionDisclosure, RetentionPolicy};

pub fn ac002(input: &ReceiptInputs<'_>) -> Vec<SubObservation> {
    sub_observations([
        ("source_mime", input.edit.admission.source_mime),
        ("source_size", input.edit.admission.source_size),
        ("source_provenance", input.edit.admission.source_provenance),
        ("mask_mime", input.edit.admission.mask_mime),
        ("mask_size", input.edit.admission.mask_size),
        ("path_traversal", input.edit.admission.path_traversal),
    ])
}

pub fn ac004(input: &ReceiptInputs<'_>) -> Vec<SubObservation> {
    let matrix = &input.remote.policy_matrix;
    sub_observations([
        (
            "initial_guard",
            matrix.initial_guard
                && input.remote.private_target_rejected
                && input.remote.guard_absence_rejected,
        ),
        ("redirect_guard", matrix.redirect_guard),
        ("scheme", matrix.scheme),
        ("byte_cap", matrix.byte_cap),
        ("mime_cap", matrix.mime_cap),
        (
            "outcomes",
            input.remote.outcomes == ["persisted", "reference", "rejected"],
        ),
        (
            "credential_omission",
            input.remote.credential_headers_absent,
        ),
    ])
}

pub fn ac005(input: &ReceiptInputs<'_>) -> Vec<SubObservation> {
    let artifact = input.edit.record.as_ref();
    sub_observations([
        (
            "metadata",
            artifact.is_some_and(|record| {
                !record.artifact_id.as_str().is_empty()
                    && record.byte_len > 0
                    && !record.mime_type.is_empty()
                    && !record.media_root_relative_path.as_path().is_absolute()
                    && !record.provenance.provider_id.as_str().is_empty()
                    && !record.provenance.model_id.as_str().is_empty()
                    && !record.created_at.is_empty()
                    && serde_json::to_value(&record.generation_options_summary).is_ok()
            }),
        ),
        (
            "digest",
            input.artifact_hash_consistent
                && artifact.is_some_and(|record| !record.sha256.as_str().is_empty()),
        ),
        (
            "source_chain",
            artifact.is_some_and(|record| record.provenance.source_artifact_ids.len() == 2),
        ),
        (
            "retention",
            artifact.is_some_and(|record| record.retention == RetentionPolicy::UserManaged),
        ),
        (
            "disclosure",
            artifact.is_some_and(|record| {
                record.disclosure == ProjectionDisclosure::RawContentPossibleElsewhere
            }),
        ),
    ])
}

pub fn ac007(input: &ReceiptInputs<'_>) -> Vec<SubObservation> {
    sub_observations([
        state_parity(input, "included"),
        state_parity(input, "unsupported"),
        state_parity(input, "extraction_failed"),
        state_parity(input, "analyzer_missing"),
        state_parity(input, "truncated"),
        state_parity(input, "unavailable"),
    ])
}

pub fn ac008(input: &ReceiptInputs<'_>) -> Vec<SubObservation> {
    sub_observations([
        ("injected", input.analyzer.runtime.injected),
        ("missing", input.analyzer.missing_explicit),
        (
            "codec",
            input.analyzer.codec_unsupported && input.analyzer.codec_reason_recorded,
        ),
        (
            "duration",
            input.analyzer.duration_capped && input.analyzer.duration_reason_recorded,
        ),
    ])
}

pub fn ac010(input: &ReceiptInputs<'_>) -> Vec<SubObservation> {
    input.documentation_policy.sub_observations.clone()
}

fn state_parity(input: &ReceiptInputs<'_>, name: &'static str) -> (&'static str, bool) {
    (
        name,
        input
            .surfaces
            .states
            .get(name)
            .is_some_and(|state| state.all_empty()),
    )
}
