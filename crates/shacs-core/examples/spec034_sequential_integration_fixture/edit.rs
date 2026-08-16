#[path = "../../tests/spec034_image_edit/support.rs"]
mod fixture;

use serde::Serialize;
use serde_json::json;
use shacs_core::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactImageOperationRequest, ArtifactStore,
    ArtifactWriteRequest, GeneratedArtifactDefinition, GeneratedArtifactMetadata,
    GeneratedArtifactRecord, GeneratedArtifactRef, GeneratedMediaKind, GenerationOperation,
    ImageOperationService, ProjectionDisclosure, ProviderMediaBytes, ProviderMediaCandidate,
    ProviderMediaCandidateId, ProviderMediaOrigin, RetentionPolicy,
    MAX_IMAGE_OPERATION_SOURCE_BYTES,
};
use shacs_providers::ImageOperationOptions;
use std::error::Error;

#[derive(Debug, Serialize)]
pub struct EditReport {
    pub operations: Vec<&'static str>,
    pub transport_calls: usize,
    pub source_lineage: Vec<String>,
    pub replacement_revalidated: bool,
    pub replacement_transport_calls: usize,
    pub misleading_success_rejected: bool,
    pub raw_options_bounded: bool,
    pub admission: AdmissionMatrix,
    #[serde(skip)]
    pub record: Option<GeneratedArtifactRecord>,
}

#[derive(Debug, Serialize)]
pub struct AdmissionMatrix {
    pub path_traversal: bool,
    pub source_mime: bool,
    pub source_size: bool,
    pub source_provenance: bool,
    pub mask_mime: bool,
    pub mask_size: bool,
}

pub fn run(store: &ArtifactStore, root: &std::path::Path) -> Result<EditReport, Box<dyn Error>> {
    let source = fixture::persist_image(store, "edit-source", fixture::PNG)?;
    let mask = fixture::persist_image(store, "edit-mask", fixture::PNG)?;
    let client = fixture::CountingClient::new();
    let service = ImageOperationService::new(store, &client);
    let edit = service.execute(ArtifactImageOperationRequest::edit(
        "replace sky",
        source.clone(),
    ))?;
    let masked = service.execute(ArtifactImageOperationRequest::mask(
        "replace sky",
        source.clone(),
        Some(mask),
    ))?;
    let variation = service.execute(ArtifactImageOperationRequest::variation(source.clone()))?;
    if edit.operation() != GenerationOperation::Edit
        || masked.source_artifact_ids().len() != 2
        || variation.operation() != GenerationOperation::Variation
    {
        return Err("image operation variants lost typed identity".into());
    }

    let source_lineage = masked
        .source_artifact_ids()
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect::<Vec<_>>();
    let record = publish_mask_candidate(store, masked)?;
    let (replacement_revalidated, replacement_transport_calls) = replacement_probe(store, root)?;
    let count_before_misleading = fixture::artifact_count(root)?;
    let misleading = fixture::CountingClient::misleading_success();
    let misleading_result = ImageOperationService::new(store, &misleading)
        .execute(ArtifactImageOperationRequest::variation(source));
    let misleading_success_rejected = misleading_result.is_err()
        && misleading.calls() == 1
        && fixture::artifact_count(root)? == count_before_misleading;
    if !replacement_revalidated || !misleading_success_rejected {
        return Err("image operation revalidation probes failed".into());
    }
    let mut options = ImageOperationOptions::default();
    options
        .provider_options
        .insert("raw_secret".to_owned(), json!("must-not-render"));
    let options_debug = format!("{options:?}");
    let raw_options_bounded = options_debug.contains("[REDACTED]")
        && !options_debug.contains("must-not-render")
        && !options_debug.contains("raw_secret");
    let admission = admission_matrix()?;
    Ok(EditReport {
        operations: vec!["edit", "mask", "variation"],
        transport_calls: client.calls(),
        source_lineage,
        replacement_revalidated,
        replacement_transport_calls,
        misleading_success_rejected,
        raw_options_bounded,
        admission,
        record: Some(record),
    })
}

fn admission_matrix() -> Result<AdmissionMatrix, Box<dyn Error>> {
    let path_traversal = [
        "/tmp/source.png",
        "../source.png",
        "artifacts/../source.png",
    ]
    .into_iter()
    .all(|path| {
        serde_json::from_value::<GeneratedArtifactRef>(json!({
            "artifactId": "source",
            "mediaRootRelativePath": path,
            "sha256": "0".repeat(64),
        }))
        .is_err()
    });
    Ok(AdmissionMatrix {
        path_traversal,
        source_mime: rejects_source("audio/mpeg", fixture::PNG, GeneratedMediaKind::Audio)?,
        source_size: rejects_source(
            "image/png",
            &vec![0; MAX_IMAGE_OPERATION_SOURCE_BYTES + 1],
            GeneratedMediaKind::Image,
        )?,
        source_provenance: rejects_inbound_source()?,
        mask_mime: rejects_mask("audio/mpeg", fixture::PNG, GeneratedMediaKind::Audio)?,
        mask_size: rejects_mask(
            "image/png",
            &vec![0; MAX_IMAGE_OPERATION_SOURCE_BYTES + 1],
            GeneratedMediaKind::Image,
        )?,
    })
}

fn rejects_source(
    mime: &str,
    bytes: &[u8],
    kind: GeneratedMediaKind,
) -> Result<bool, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = fixture::persist_media(&store, "matrix-source", mime, bytes, kind)?;
    let client = fixture::CountingClient::new();
    let rejected = ImageOperationService::new(&store, &client)
        .execute(ArtifactImageOperationRequest::edit("edit", source))
        .is_err();
    Ok(rejected && client.calls() == 0)
}

fn rejects_mask(
    mime: &str,
    bytes: &[u8],
    kind: GeneratedMediaKind,
) -> Result<bool, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = fixture::persist_image(&store, "matrix-source", fixture::PNG)?;
    let mask = fixture::persist_media(&store, "matrix-mask", mime, bytes, kind)?;
    let client = fixture::CountingClient::new();
    let rejected = ImageOperationService::new(&store, &client)
        .execute(ArtifactImageOperationRequest::mask(
            "mask",
            source,
            Some(mask),
        ))
        .is_err();
    Ok(rejected && client.calls() == 0)
}

fn rejects_inbound_source() -> Result<bool, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = fixture::persist_image(&store, "matrix-source", fixture::PNG)?;
    let record_path = root.path().join("artifacts/matrix-source/record.json");
    let mut record: serde_json::Value = serde_json::from_slice(&std::fs::read(&record_path)?)?;
    record["provenance"] = json!({
        "kind": "inbound_attachment",
        "attachmentId": "attachment-1",
        "channel": "local"
    });
    std::fs::write(record_path, serde_json::to_vec_pretty(&record)?)?;
    let client = fixture::CountingClient::new();
    let rejected = ImageOperationService::new(&store, &client)
        .execute(ArtifactImageOperationRequest::edit("edit", source))
        .is_err();
    Ok(rejected && client.calls() == 0)
}

fn replacement_probe(
    store: &ArtifactStore,
    root: &std::path::Path,
) -> Result<(bool, usize), Box<dyn Error>> {
    let source = fixture::persist_image(store, "replacement-source", fixture::PNG)?;
    let client = fixture::CountingClient::new();
    let service = ImageOperationService::new(store, &client);
    let admitted = service.admit(ArtifactImageOperationRequest::edit("edit", source))?;
    std::fs::write(
        root.join("artifacts/replacement-source/payload.png"),
        b"replaced-after-admission",
    )?;
    let rejected = service.execute_admitted(admitted).is_err();
    Ok((rejected && client.calls() == 0, client.calls()))
}

fn publish_mask_candidate(
    store: &ArtifactStore,
    candidate: shacs_core::generated_media::ValidatedImageOperationCandidate,
) -> Result<GeneratedArtifactRecord, Box<dyn Error>> {
    let (operation, result, source_ids) = candidate.into_parts();
    let image = result
        .images
        .into_iter()
        .next()
        .ok_or("mask operation returned no image")?;
    let media = ProviderMediaCandidate::bytes(
        ProviderMediaCandidateId::new("candidate-edit-output")?,
        ProviderMediaOrigin::new(result.provider_id, result.model),
        ProviderMediaBytes::new(image.mime_type, image.bytes),
    );
    let metadata = GeneratedArtifactMetadata::new(
        ArtifactId::new("edit-output")?,
        GeneratedArtifactDefinition::new(
            GeneratedMediaKind::Image,
            operation,
            ArtifactHandlingPolicy::new(
                RetentionPolicy::UserManaged,
                ProjectionDisclosure::RawContentPossibleElsewhere,
            ),
        ),
        "2026-08-15T00:00:00Z",
    )
    .with_sources(source_ids);
    Ok(store
        .persist(ArtifactWriteRequest::new(media, metadata))?
        .record()
        .clone())
}
