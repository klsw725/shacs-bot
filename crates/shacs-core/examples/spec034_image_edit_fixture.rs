use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::json;
use shacs_core::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactImageOperationRequest, ArtifactStore,
    ArtifactWriteRequest, GeneratedArtifactDefinition, GeneratedArtifactMetadata,
    GeneratedArtifactRef, GeneratedMediaKind, GenerationOperation, ImageOperationService,
    ProjectionDisclosure, ProviderMediaBytes, ProviderMediaCandidate, ProviderMediaCandidateId,
    ProviderMediaOrigin, RetentionPolicy,
};
use shacs_providers::{
    ImageGenerationHttpResponse, ImageGenerationHttpTransport, ImageGenerationRequestParts,
    ImageMultipartRequestParts, OpenAiImageGenerationClient, ProviderError,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfixture";

#[derive(Clone)]
struct FixtureTransport {
    calls: Arc<AtomicUsize>,
}

impl ImageGenerationHttpTransport for FixtureTransport {
    fn post_json(
        &self,
        _request: ImageGenerationRequestParts,
    ) -> Result<ImageGenerationHttpResponse, ProviderError> {
        Err(ProviderError::UnsupportedCapability {
            provider_id: "fixture".to_owned(),
            capability: "json".to_owned(),
        })
    }

    fn post_multipart(
        &self,
        request: ImageMultipartRequestParts,
    ) -> Result<ImageGenerationHttpResponse, ProviderError> {
        let body = String::from_utf8_lossy(&request.body);
        if request.path != "/images/edits"
            || !body.contains("name=\"image\"; filename=\"source.png\"")
            || !body.contains("name=\"mask\"; filename=\"mask.png\"")
        {
            return Err(ProviderError::Api {
                status: Some(400),
                message: "fixture multipart mismatch".to_owned(),
                retryable: false,
                headers: BTreeMap::new(),
                body: None,
            });
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({"data": [{"b64_json": STANDARD.encode(PNG)}]}),
        })
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let store = ArtifactStore::open(root.path())?;
    let source = persist_source(&store, "source")?;
    let mask = persist_source(&store, "mask")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let client = OpenAiImageGenerationClient::new(
        "fixture-token",
        "https://fixture.invalid/v1",
        "gpt-image-2",
        FixtureTransport {
            calls: Arc::clone(&calls),
        },
    );

    let candidate = ImageOperationService::new(&store, &client).execute(
        ArtifactImageOperationRequest::mask("replace sky", source, Some(mask)),
    )?;
    let artifact_count = std::fs::read_dir(root.path().join("artifacts"))?.count();
    println!(
        "{}",
        json!({
            "transportCalls": calls.load(Ordering::SeqCst),
            "sourceIds": candidate
                .source_artifact_ids()
                .iter()
                .map(ArtifactId::as_str)
                .collect::<Vec<_>>(),
            "candidateImages": candidate.result().images.len(),
            "publishedOutputArtifacts": artifact_count - 2,
        })
    );
    Ok(())
}

fn persist_source(store: &ArtifactStore, id: &str) -> Result<GeneratedArtifactRef, Box<dyn Error>> {
    let candidate = ProviderMediaCandidate::bytes(
        ProviderMediaCandidateId::new(format!("candidate-{id}"))?,
        ProviderMediaOrigin::new("openai", "gpt-image-2"),
        ProviderMediaBytes::new("image/png", PNG.to_vec()),
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
    Ok(store
        .persist(ArtifactWriteRequest::new(candidate, metadata))?
        .artifact_ref())
}
