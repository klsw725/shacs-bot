use super::receipt_inputs::ReceiptInputs;
use super::receipt_model::{sub_observations, SubObservation};
use shacs_core::generated_media::{
    GeneratedMediaKind, GeneratedProvenanceKind, ProjectionDisclosure, RetentionPolicy,
};

pub use super::receipts_acceptance::{ac002, ac004, ac005, ac007, ac008, ac010};

pub fn mh002(input: &ReceiptInputs<'_>) -> Vec<SubObservation> {
    sub_observations([
        (
            "edit",
            input.edit.operations.contains(&"edit") && input.edit.transport_calls == 3,
        ),
        (
            "mask",
            input.edit.operations.contains(&"mask") && input.edit.source_lineage.len() == 2,
        ),
        ("variation", input.edit.operations.contains(&"variation")),
        ("raw_options_bounded", input.edit.raw_options_bounded),
    ])
}

pub fn mh003(input: &ReceiptInputs<'_>) -> Vec<SubObservation> {
    sub_observations([
        ("path_traversal", input.edit.admission.path_traversal),
        (
            "mime",
            input.edit.admission.source_mime && input.edit.admission.mask_mime,
        ),
        (
            "size",
            input.edit.admission.source_size && input.edit.admission.mask_size,
        ),
        ("provenance", input.edit.admission.source_provenance),
        (
            "replacement",
            input.edit.replacement_revalidated && input.edit.replacement_transport_calls == 0,
        ),
    ])
}

pub fn mh006(input: &ReceiptInputs<'_>) -> Vec<SubObservation> {
    let artifact = input.artifact;
    sub_observations([
        ("identity", !artifact.artifact_id.as_str().is_empty()),
        (
            "media",
            artifact.kind == GeneratedMediaKind::Image
                && artifact.byte_len > 0
                && !artifact.mime_type.is_empty()
                && !artifact.media_root_relative_path.as_path().is_absolute(),
        ),
        (
            "origin",
            artifact.provenance.kind == GeneratedProvenanceKind::Generated
                && !artifact.provenance.provider_id.as_str().is_empty()
                && !artifact.provenance.model_id.as_str().is_empty(),
        ),
        (
            "sources",
            artifact.provenance.source_artifact_ids.is_empty(),
        ),
        (
            "options",
            serde_json::to_value(&artifact.generation_options_summary).is_ok(),
        ),
        (
            "lifecycle",
            input.artifact_record_exists
                && input.artifact_hash_consistent
                && !artifact.created_at.is_empty(),
        ),
        (
            "disclosure",
            artifact.retention == RetentionPolicy::UserManaged
                && artifact.disclosure == ProjectionDisclosure::RawContentPossibleElsewhere,
        ),
    ])
}

pub fn mh008(input: &ReceiptInputs<'_>) -> Vec<SubObservation> {
    let states = [
        "included",
        "unsupported",
        "extraction_failed",
        "analyzer_missing",
        "truncated",
    ]
    .iter()
    .all(|state| input.analyzer.states.contains(state));
    sub_observations([
        ("states", states),
        (
            "stored_provenance",
            input.analyzer.runtime.stored_provenance,
        ),
        (
            "generated_provenance",
            input.analyzer.runtime.generated_provenance,
        ),
        ("minimum_fields", input.analyzer.runtime.minimum_fields),
    ])
}

pub fn mh009(input: &ReceiptInputs<'_>) -> Vec<SubObservation> {
    let owner = input
        .analyzer
        .included
        .as_ref()
        .map(|included| &included.owner_facts);
    sub_observations([
        ("runtime_injection", input.analyzer.runtime.injected),
        (
            "source",
            owner.and_then(|facts| facts.source.as_ref()).is_some()
                && input.analyzer.trusted_source_disclosed,
        ),
        (
            "sandbox",
            owner.and_then(|facts| facts.sandbox.as_ref()).is_some()
                && input.analyzer.sandbox_not_universal,
        ),
        (
            "credential",
            owner.and_then(|facts| facts.credential.as_ref()).is_some()
                && input.analyzer.credential_not_exposed,
        ),
        (
            "disclosure",
            owner.and_then(|facts| facts.disclosure.as_ref()).is_some()
                && input.analyzer.typed_disclosure_recorded,
        ),
        (
            "snapshot",
            owner.and_then(|facts| facts.snapshot.as_ref()).is_some(),
        ),
    ])
}
