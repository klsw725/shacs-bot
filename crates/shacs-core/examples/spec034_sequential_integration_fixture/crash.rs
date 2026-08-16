use serde::Serialize;
use shacs_core::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactStore, ArtifactTransactionStage,
    ArtifactWriteRequest, GeneratedArtifactDefinition, GeneratedArtifactMetadata,
    GeneratedMediaKind, GenerationOperation, ProjectionDisclosure, ProviderMediaBytes,
    ProviderMediaCandidate, ProviderMediaCandidateId, ProviderMediaOrigin, RetentionPolicy,
    TransactionDecision,
};
use std::error::Error;
use std::path::Path;

const PAYLOAD: &[u8] = b"transaction-crash-fixture";

#[derive(Debug, Serialize)]
pub struct CrashReport {
    pub before_rename_hidden_and_clean: bool,
    pub after_rename_recovered: bool,
    pub before_interruption_stage: &'static str,
    pub after_interruption_stage: &'static str,
}

pub fn run(root: &Path) -> Result<CrashReport, Box<dyn Error>> {
    let before_root = root.join("crash-before-rename");
    let before = ArtifactStore::open(&before_root)?;
    let interrupted = before.persist_with_observer(request("before")?, |stage| {
        if stage == ArtifactTransactionStage::RecordSynced {
            TransactionDecision::Interrupt
        } else {
            TransactionDecision::Continue
        }
    });
    drop(before);
    let reopened_before = ArtifactStore::open(&before_root)?;
    let before_rename_hidden_and_clean = interrupted.is_err()
        && !before_root.join("artifacts/before").exists()
        && staging_count(&before_root)? == 0;
    drop(reopened_before);

    let after_root = root.join("crash-after-rename");
    let after = ArtifactStore::open(&after_root)?;
    let interrupted = after.persist_with_observer(request("after")?, |stage| {
        if stage == ArtifactTransactionStage::Renamed {
            TransactionDecision::Interrupt
        } else {
            TransactionDecision::Continue
        }
    });
    drop(after);
    let reopened_after = ArtifactStore::open(&after_root)?;
    let recovered = reopened_after.read(&ArtifactId::new("after")?)?;
    let after_rename_recovered = interrupted.is_err()
        && reopened_after.read_payload(&recovered)? == PAYLOAD
        && staging_count(&after_root)? == 0;
    if !before_rename_hidden_and_clean || !after_rename_recovered {
        return Err("artifact transaction crash recovery diverged".into());
    }
    Ok(CrashReport {
        before_rename_hidden_and_clean,
        after_rename_recovered,
        before_interruption_stage: "record_synced",
        after_interruption_stage: "renamed",
    })
}

fn request(id: &str) -> Result<ArtifactWriteRequest, Box<dyn Error>> {
    let candidate = ProviderMediaCandidate::bytes(
        ProviderMediaCandidateId::new(format!("candidate-{id}"))?,
        ProviderMediaOrigin::new("openai", "gpt-image-2"),
        ProviderMediaBytes::new("image/png", PAYLOAD.to_vec()),
    );
    let metadata = GeneratedArtifactMetadata::new(
        ArtifactId::new(id)?,
        GeneratedArtifactDefinition::new(
            GeneratedMediaKind::Image,
            GenerationOperation::Generate,
            ArtifactHandlingPolicy::new(
                RetentionPolicy::UserManaged,
                ProjectionDisclosure::RawContentPossibleElsewhere,
            ),
        ),
        "2026-08-15T00:00:00Z",
    );
    Ok(ArtifactWriteRequest::new(candidate, metadata))
}

fn staging_count(root: &Path) -> Result<usize, Box<dyn Error>> {
    Ok(std::fs::read_dir(root.join("artifacts"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".stage-"))
        .count())
}
