use crate::support::{artifact_count, persist_image, PNG};
use serde_json::json;
use shacs_core::generated_media::{
    ArtifactImageOperationRequest, ArtifactStore, ImageOperationAdmissionError,
    ImageOperationService, MAX_IMAGE_OPERATION_SOURCE_BYTES,
};
use shacs_providers::{
    parse_openrouter_image_generation_response, GeneratedImage, ImageGenerationClient,
    ImageGenerationHttpResponse, ImageGenerationItemId, ImageGenerationRequest,
    ImageGenerationResult, ImageMimeType, ImageOperationRequest, ImageOperationResult,
    ProviderError,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy)]
enum ProviderResultFixture {
    RemoteOnly,
    MixedLocalAndRemote,
    TwoLocal,
    EmptyBytes,
    ByteLenMismatch,
    Oversize,
    MimeMismatch,
}

struct FixtureClient {
    calls: AtomicUsize,
    fixture: ProviderResultFixture,
}

impl FixtureClient {
    const fn new(fixture: ProviderResultFixture) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            fixture,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ImageGenerationClient for FixtureClient {
    fn generate_image(
        &self,
        _request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        unreachable!("provider result tests execute only image operations")
    }

    fn execute_image_operation(
        &self,
        request: ImageOperationRequest,
    ) -> Result<ImageOperationResult, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = provider_result(self.fixture)?;
        Ok(match request {
            ImageOperationRequest::Edit(_) => ImageOperationResult::Edit(result),
            ImageOperationRequest::Mask(_) => ImageOperationResult::Mask(result),
            ImageOperationRequest::Variation(_) => ImageOperationResult::Variation(result),
            ImageOperationRequest::Generate(_) => ImageOperationResult::Generate(result),
        })
    }
}

#[test]
fn provider_result_shape_is_rejected_before_candidate_creation() -> Result<(), Box<dyn Error>> {
    for fixture in [
        ProviderResultFixture::RemoteOnly,
        ProviderResultFixture::MixedLocalAndRemote,
        ProviderResultFixture::TwoLocal,
        ProviderResultFixture::EmptyBytes,
        ProviderResultFixture::ByteLenMismatch,
        ProviderResultFixture::Oversize,
        ProviderResultFixture::MimeMismatch,
    ] {
        // Given
        let root = tempfile::tempdir()?;
        let store = ArtifactStore::open(root.path())?;
        let source = persist_image(&store, "source", PNG)?;
        let client = FixtureClient::new(fixture);

        // When
        let result = ImageOperationService::new(&store, &client)
            .execute(ArtifactImageOperationRequest::edit("edit", source));

        // Then
        assert!(matches!(
            result,
            Err(ImageOperationAdmissionError::InvalidProviderResult)
        ));
        assert_eq!(client.calls(), 1);
        assert_eq!(artifact_count(root.path())?, 1);
    }
    Ok(())
}

fn provider_result(fixture: ProviderResultFixture) -> Result<ImageGenerationResult, ProviderError> {
    match fixture {
        ProviderResultFixture::RemoteOnly | ProviderResultFixture::MixedLocalAndRemote => {
            remote_result(fixture)
        }
        ProviderResultFixture::TwoLocal
        | ProviderResultFixture::EmptyBytes
        | ProviderResultFixture::ByteLenMismatch
        | ProviderResultFixture::Oversize
        | ProviderResultFixture::MimeMismatch => Ok(local_result(fixture)),
    }
}

fn remote_result(fixture: ProviderResultFixture) -> Result<ImageGenerationResult, ProviderError> {
    let images = match fixture {
        ProviderResultFixture::MixedLocalAndRemote => json!([
            {"image_url": {"url": "data:image/png;base64,iVBORw0KGgpzb3VyY2U="}},
            {"image_url": {"url": "https://cdn.example/final.png"}}
        ]),
        ProviderResultFixture::RemoteOnly => {
            json!([{"image_url": {"url": "https://cdn.example/final.png"}}])
        }
        ProviderResultFixture::TwoLocal
        | ProviderResultFixture::EmptyBytes
        | ProviderResultFixture::ByteLenMismatch
        | ProviderResultFixture::Oversize
        | ProviderResultFixture::MimeMismatch => {
            unreachable!("local fixtures do not create remote results")
        }
    };
    parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({"choices": [{"message": {"images": images}}]}),
        },
        "gpt-image-2",
    )
}

fn local_result(fixture: ProviderResultFixture) -> ImageGenerationResult {
    let mut image = GeneratedImage {
        index: 0,
        mime_type: ImageMimeType::Png,
        bytes: PNG.to_vec(),
        byte_len: PNG.len(),
        revised_prompt: None,
        provider_item_id: Some(ImageGenerationItemId::from_provider("edit-result")),
    };
    match fixture {
        ProviderResultFixture::EmptyBytes => {
            image.bytes.clear();
            image.byte_len = 0;
        }
        ProviderResultFixture::ByteLenMismatch => image.byte_len += 1,
        ProviderResultFixture::Oversize => {
            image.bytes = vec![0; MAX_IMAGE_OPERATION_SOURCE_BYTES + 1];
            image.byte_len = image.bytes.len();
        }
        ProviderResultFixture::MimeMismatch => image.bytes = b"not png".to_vec(),
        ProviderResultFixture::TwoLocal => {}
        ProviderResultFixture::RemoteOnly | ProviderResultFixture::MixedLocalAndRemote => {
            unreachable!("remote fixtures do not create local results")
        }
    }
    let images = match fixture {
        ProviderResultFixture::TwoLocal => vec![image.clone(), image],
        ProviderResultFixture::EmptyBytes
        | ProviderResultFixture::ByteLenMismatch
        | ProviderResultFixture::Oversize
        | ProviderResultFixture::MimeMismatch => vec![image],
        ProviderResultFixture::RemoteOnly | ProviderResultFixture::MixedLocalAndRemote => {
            unreachable!("remote fixtures do not create local results")
        }
    };
    ImageGenerationResult {
        provider_id: "openai".to_owned(),
        model: "gpt-image-2".to_owned(),
        images,
        remote_images: Vec::new(),
        usage: None,
        request_id: None,
    }
}
