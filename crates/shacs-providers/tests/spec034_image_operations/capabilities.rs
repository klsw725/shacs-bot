use crate::support::CapturingTransport;
use shacs_providers::{
    image_operation_capabilities, CodexClient, CodexHttpStreamResponse, CodexRequestParts,
    ImageEditRequest, ImageFileInput, ImageGenerationClient, ImageMaskRequest, ImageOperation,
    ImageOperationRequest, ImageVariationRequest, OpenRouterImageGenerationClient, ProviderConfig,
    ProviderError,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn capability_matrix_distinguishes_each_operation() -> Result<(), Box<dyn Error>> {
    let openai = image_operation_capabilities("openai");
    let openrouter = image_operation_capabilities("openrouter");
    let codex = image_operation_capabilities("openai_codex");

    if !openai.supports(ImageOperation::Generate)
        || !openai.supports(ImageOperation::Edit)
        || !openai.supports(ImageOperation::Mask)
        || openai.supports(ImageOperation::Variation)
        || !openrouter.supports(ImageOperation::Generate)
        || openrouter.supports(ImageOperation::Edit)
        || !codex.supports(ImageOperation::Generate)
        || codex.supports(ImageOperation::Mask)
    {
        return Err(format!("image operation capability matrix drifted: {openai:?}").into());
    }
    Ok(())
}

#[test]
fn unsupported_openrouter_edit_is_typed_without_transport() -> Result<(), Box<dyn Error>> {
    let transport = CapturingTransport::success();
    let client = OpenRouterImageGenerationClient::new(
        "not-a-secret",
        "https://openrouter.ai/api/v1",
        "openai/gpt-image",
        transport.clone(),
    );
    let source = ImageFileInput::new("source.png", "image/png", b"source".to_vec())?;
    let request = ImageOperationRequest::Edit(ImageEditRequest::new("add a hat", source));

    let error = match client.execute_image_operation(request) {
        Ok(result) => return Err(format!("unsupported edit succeeded: {result:?}").into()),
        Err(error) => error,
    };
    match error {
        ProviderError::UnsupportedCapability {
            provider_id,
            capability,
        } if provider_id == "openrouter" && capability == "image_edit" => {}
        other => return Err(format!("unexpected unsupported error: {other:?}").into()),
    }
    if !transport.captured()?.is_empty() {
        return Err("unsupported edit reached transport".into());
    }
    Ok(())
}

#[test]
fn unsupported_codex_operations_preserve_provider_identity() -> Result<(), Box<dyn Error>> {
    let transport_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&transport_calls);
    let client = CodexClient::new(
        ProviderConfig::default(),
        move |_request: CodexRequestParts| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(CodexHttpStreamResponse {
                status: 500,
                headers: BTreeMap::new(),
                body: String::new(),
            })
        },
    );
    let operations = [
        (
            "image_edit",
            ImageOperationRequest::Edit(ImageEditRequest::new("edit", image_input()?)),
        ),
        (
            "image_mask",
            ImageOperationRequest::Mask(ImageMaskRequest::new(
                "mask",
                image_input()?,
                image_input()?,
            )),
        ),
        (
            "image_variation",
            ImageOperationRequest::Variation(ImageVariationRequest::new(image_input()?)),
        ),
    ];

    for (expected_capability, request) in operations {
        match client.execute_image_operation(request) {
            Err(ProviderError::UnsupportedCapability {
                provider_id,
                capability,
            }) if provider_id == "openai_codex" && capability == expected_capability => {}
            other => {
                return Err(format!(
                    "Codex unsupported identity drifted for {expected_capability}: {other:?}"
                )
                .into())
            }
        }
    }
    if transport_calls.load(Ordering::SeqCst) != 0 {
        return Err("unsupported Codex operation reached transport".into());
    }
    Ok(())
}

fn image_input() -> Result<ImageFileInput, Box<dyn Error>> {
    ImageFileInput::new("source.png", "image/png", vec![1]).map_err(Into::into)
}
