use crate::support::{artifact_count, persist_image, persist_media, CountingClient, PNG};
use serde_json::json;
use shacs_core::generated_media::{
    ArtifactImageOperationRequest, ArtifactStore, GeneratedArtifactRef, GeneratedMediaKind,
    ImageOperationAdmissionError, ImageOperationService, MAX_IMAGE_OPERATION_SOURCE_BYTES,
};
use std::error::Error;

#[test]
fn malformed_absolute_and_traversal_refs_are_rejected_at_parse_boundary(
) -> Result<(), Box<dyn Error>> {
    for path in [
        "/tmp/source.png",
        "../source.png",
        "artifacts/../source.png",
    ] {
        let value = json!({
            "artifactId": "source",
            "mediaRootRelativePath": path,
            "sha256": "0".repeat(64),
        });
        assert!(serde_json::from_value::<GeneratedArtifactRef>(value).is_err());
    }
    Ok(())
}

#[test]
fn caller_path_and_digest_cannot_override_committed_record() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_image(&store, "source", PNG)?;
    let client = CountingClient::new();
    let service = ImageOperationService::new(&store, &client);
    let mut value = serde_json::to_value(&source)?;
    value["sha256"] = json!("0".repeat(64));
    let stale_digest = serde_json::from_value(value)?;
    let mut value = serde_json::to_value(&source)?;
    value["mediaRootRelativePath"] = json!("artifacts/other/payload.png");
    let outside_owner = serde_json::from_value(value)?;

    for artifact_ref in [stale_digest, outside_owner] {
        assert!(matches!(
            service.execute(ArtifactImageOperationRequest::edit("edit", artifact_ref)),
            Err(ImageOperationAdmissionError::ReferenceMismatch)
        ));
    }
    assert_eq!(client.calls(), 0);
    assert_eq!(artifact_count(root.path())?, 1);
    Ok(())
}

#[test]
fn missing_nonregular_tampered_mime_and_oversized_sources_never_reach_transport(
) -> Result<(), Box<dyn Error>> {
    let cases = [
        ("audio", "audio/mpeg", PNG, GeneratedMediaKind::Audio),
        (
            "mismatch",
            "image/png",
            b"not png",
            GeneratedMediaKind::Image,
        ),
    ];
    for (id, mime, bytes, kind) in cases {
        let root = tempfile::tempdir()?;
        let store = ArtifactStore::open(root.path())?;
        let artifact_ref = persist_media(&store, id, mime, bytes, kind)?;
        let client = CountingClient::new();
        let service = ImageOperationService::new(&store, &client);
        assert!(service
            .execute(ArtifactImageOperationRequest::edit("edit", artifact_ref))
            .is_err());
        assert_eq!(client.calls(), 0);
        assert_eq!(artifact_count(root.path())?, 1);
    }

    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let oversized = persist_media(
        &store,
        "oversized",
        "image/png",
        &vec![0; MAX_IMAGE_OPERATION_SOURCE_BYTES + 1],
        GeneratedMediaKind::Image,
    )?;
    let client = CountingClient::new();
    assert!(ImageOperationService::new(&store, &client)
        .execute(ArtifactImageOperationRequest::edit("edit", oversized))
        .is_err());
    assert_eq!(client.calls(), 0);
    Ok(())
}

#[test]
fn missing_nonregular_and_inbound_provenance_records_fail_closed() -> Result<(), Box<dyn Error>> {
    for mutation in ["missing", "nonregular", "inbound"] {
        let root = tempfile::tempdir()?;
        let store = ArtifactStore::open(root.path())?;
        let source = persist_image(&store, "source", PNG)?;
        let artifact_dir = root.path().join("artifacts/source");
        match mutation {
            "missing" => std::fs::remove_dir_all(&artifact_dir)?,
            "nonregular" => {
                let payload = artifact_dir.join("payload.png");
                std::fs::remove_file(&payload)?;
                std::fs::create_dir(&payload)?;
            }
            "inbound" => {
                let record_path = artifact_dir.join("record.json");
                let mut record: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&record_path)?)?;
                record["provenance"] = json!({
                    "kind": "inbound_attachment",
                    "attachmentId": "attachment-1",
                    "channel": "local"
                });
                std::fs::write(record_path, serde_json::to_vec_pretty(&record)?)?;
            }
            _ => return Err("unknown fixture mutation".into()),
        }
        let client = CountingClient::new();
        assert!(ImageOperationService::new(&store, &client)
            .execute(ArtifactImageOperationRequest::edit("edit", source))
            .is_err());
        assert_eq!(client.calls(), 0);
    }
    Ok(())
}

#[test]
fn invalid_mask_mime_uses_the_same_admission_before_transport() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_image(&store, "source", PNG)?;
    let mask = persist_media(&store, "mask", "audio/mpeg", PNG, GeneratedMediaKind::Audio)?;
    let client = CountingClient::new();

    assert!(ImageOperationService::new(&store, &client)
        .execute(ArtifactImageOperationRequest::mask(
            "mask",
            source,
            Some(mask)
        ))
        .is_err());
    assert_eq!(client.calls(), 0);
    assert_eq!(artifact_count(root.path())?, 2);
    Ok(())
}

#[test]
fn oversized_mask_is_rejected_before_transport() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_image(&store, "source", PNG)?;
    let mask = persist_media(
        &store,
        "mask",
        "image/png",
        &vec![0; MAX_IMAGE_OPERATION_SOURCE_BYTES + 1],
        GeneratedMediaKind::Image,
    )?;
    let client = CountingClient::new();

    assert!(ImageOperationService::new(&store, &client)
        .execute(ArtifactImageOperationRequest::mask(
            "mask",
            source,
            Some(mask)
        ))
        .is_err());
    assert_eq!(client.calls(), 0);
    assert_eq!(artifact_count(root.path())?, 2);
    Ok(())
}

#[test]
fn missing_mask_is_rejected_without_transport_or_artifact() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_image(&store, "source", PNG)?;
    let client = CountingClient::new();

    assert!(matches!(
        ImageOperationService::new(&store, &client)
            .execute(ArtifactImageOperationRequest::mask("mask", source, None)),
        Err(ImageOperationAdmissionError::MissingMask)
    ));
    assert_eq!(client.calls(), 0);
    assert_eq!(artifact_count(root.path())?, 1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_root_parent_and_leaf_are_rejected_without_transport() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let parent = tempfile::tempdir()?;
    let target = parent.path().join("target");
    std::fs::create_dir(&target)?;
    let root_link = parent.path().join("root-link");
    symlink(&target, &root_link)?;
    assert!(ArtifactStore::open(root_link).is_err());

    for component in ["parent", "leaf"] {
        let root = tempfile::tempdir()?;
        let store = ArtifactStore::open(root.path())?;
        let source = persist_image(&store, "source", PNG)?;
        let payload = root.path().join("artifacts/source/payload.png");
        let external = root.path().join("external");
        std::fs::write(&external, PNG)?;
        if component == "parent" {
            std::fs::rename(
                root.path().join("artifacts/source"),
                root.path().join("saved"),
            )?;
            symlink(
                root.path().join("saved"),
                root.path().join("artifacts/source"),
            )?;
        } else {
            std::fs::remove_file(&payload)?;
            symlink(&external, &payload)?;
        }
        let client = CountingClient::new();
        assert!(ImageOperationService::new(&store, &client)
            .execute(ArtifactImageOperationRequest::edit("edit", source))
            .is_err());
        assert_eq!(client.calls(), 0);
    }
    Ok(())
}

#[test]
fn replacement_after_initial_admission_is_revalidated_before_transport(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_image(&store, "source", PNG)?;
    let client = CountingClient::new();
    let service = ImageOperationService::new(&store, &client);
    let admitted = service.admit(ArtifactImageOperationRequest::edit("edit", source))?;
    let payload = root.path().join("artifacts/source/payload.png");
    let replacer = std::thread::spawn(move || std::fs::write(payload, b"tampered"));
    replacer
        .join()
        .map_err(|_| "replacement thread panicked")??;

    assert!(matches!(
        service.execute_admitted(admitted),
        Err(ImageOperationAdmissionError::Artifact(_))
    ));
    assert_eq!(client.calls(), 0);
    assert_eq!(artifact_count(root.path())?, 1);
    Ok(())
}

#[test]
fn misleading_provider_success_creates_no_candidate_or_artifact() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_image(&store, "source", PNG)?;
    let client = CountingClient::misleading_success();

    assert!(matches!(
        ImageOperationService::new(&store, &client)
            .execute(ArtifactImageOperationRequest::edit("edit", source)),
        Err(ImageOperationAdmissionError::InvalidProviderResult)
    ));
    assert_eq!(client.calls(), 1);
    assert_eq!(artifact_count(root.path())?, 1);
    Ok(())
}
