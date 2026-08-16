use crate::support::CapturingTransport;
use serde_json::json;
use shacs_providers::{
    ImageEditRequest, ImageFileInput, ImageGenerationClient, ImageMaskRequest,
    ImageOperationContractError, ImageOperationRequest, ImageOperationResult,
    OpenAiImageGenerationClient, ProviderError, MAX_IMAGE_OPERATION_INPUT_BYTES,
};
use std::error::Error;

#[test]
fn openai_edit_uses_multipart_source_seam() -> Result<(), Box<dyn Error>> {
    let transport = CapturingTransport::success();
    let client = openai_client(transport.clone());
    let source = ImageFileInput::new("source.png", "image/png", b"source-png".to_vec())?;

    let result = client.execute_image_operation(ImageOperationRequest::Edit(
        ImageEditRequest::new("add a hat", source),
    ))?;

    if !matches!(result, ImageOperationResult::Edit(_)) {
        return Err(format!("edit returned wrong result variant: {result:?}").into());
    }
    let requests = transport.captured()?;
    assert_openai_multipart(&requests, false)
}

#[test]
fn openai_mask_uses_multipart_source_and_mask_seam() -> Result<(), Box<dyn Error>> {
    let transport = CapturingTransport::success();
    let client = openai_client(transport.clone());
    let source = ImageFileInput::new("source.png", "image/png", b"source-png".to_vec())?;
    let mask = ImageFileInput::new("mask.png", "image/png", b"mask-png".to_vec())?;

    let result = client.execute_image_operation(ImageOperationRequest::Mask(
        ImageMaskRequest::new("replace the sky", source, mask),
    ))?;

    if !matches!(result, ImageOperationResult::Mask(_)) {
        return Err(format!("mask returned wrong result variant: {result:?}").into());
    }
    let requests = transport.captured()?;
    assert_openai_multipart(&requests, true)
}

#[test]
fn missing_source_and_mask_are_typed_contract_errors() -> Result<(), Box<dyn Error>> {
    match ImageEditRequest::try_new("edit", None) {
        Err(ImageOperationContractError::MissingSource) => {}
        other => return Err(format!("missing source was accepted: {other:?}").into()),
    }
    let source = ImageFileInput::new("source.png", "image/png", vec![1])?;
    match ImageMaskRequest::try_new("mask", Some(source), None) {
        Err(ImageOperationContractError::MissingMask) => Ok(()),
        other => Err(format!("missing mask was accepted: {other:?}").into()),
    }
}

#[test]
fn malformed_parts_and_oversized_payload_are_rejected() -> Result<(), Box<dyn Error>> {
    match ImageFileInput::new("bad\r\nname.png", "image/png", vec![1]) {
        Err(ImageOperationContractError::MalformedPart) => {}
        other => return Err(format!("malformed filename was accepted: {other:?}").into()),
    }
    match ImageFileInput::new(
        "source.png",
        "image/png\r\nx-injected: yes",
        vec![0; MAX_IMAGE_OPERATION_INPUT_BYTES + 1],
    ) {
        Err(ImageOperationContractError::PayloadTooLarge { .. }) => Ok(()),
        other => Err(format!("oversized payload was accepted: {other:?}").into()),
    }
}

#[test]
fn malformed_provider_option_cannot_inject_multipart_header() -> Result<(), Box<dyn Error>> {
    let transport = CapturingTransport::success();
    let client = openai_client(transport.clone());
    let source = ImageFileInput::new("source.png", "image/png", vec![1])?;
    let mut request = ImageEditRequest::new("edit", source);
    request
        .options
        .provider_options
        .insert("bad\r\nfield".to_owned(), json!("injected"));

    match client.execute_image_operation(ImageOperationRequest::Edit(request)) {
        Err(ProviderError::Api {
            status: None,
            retryable: false,
            ..
        }) if transport.captured()?.is_empty() => Ok(()),
        other => Err(format!("malformed provider option reached transport: {other:?}").into()),
    }
}

#[test]
fn misleading_success_without_image_is_rejected() -> Result<(), Box<dyn Error>> {
    let transport = CapturingTransport::with_body(json!({"data": []}));
    let client = openai_client(transport);
    let source = ImageFileInput::new("source.png", "image/png", vec![1])?;

    let error = match client.execute_image_operation(ImageOperationRequest::Edit(
        ImageEditRequest::new("edit", source),
    )) {
        Ok(result) => return Err(format!("empty success accepted: {result:?}").into()),
        Err(error) => error,
    };
    match error {
        ProviderError::Api {
            status: Some(200),
            message,
            retryable: false,
            ..
        } if message.contains("empty") => Ok(()),
        other => Err(format!("unexpected misleading success error: {other:?}").into()),
    }
}

fn openai_client(transport: CapturingTransport) -> OpenAiImageGenerationClient<CapturingTransport> {
    OpenAiImageGenerationClient::new(
        "not-a-secret",
        "https://api.openai.com/v1",
        "gpt-image-2",
        transport,
    )
}

fn assert_openai_multipart(
    requests: &[shacs_providers::ImageMultipartRequestParts],
    expects_mask: bool,
) -> Result<(), Box<dyn Error>> {
    let request = requests.first().ok_or("multipart request missing")?;
    let body = String::from_utf8_lossy(&request.body);
    if request.path != "/images/edits"
        || !request
            .content_type
            .starts_with("multipart/form-data; boundary=")
        || !body.contains("name=\"model\"\r\n\r\ngpt-image-2")
        || !body.contains("name=\"prompt\"")
        || !body.contains("name=\"image\"; filename=\"source.png\"")
        || body.contains("not-a-secret")
        || body.contains("name=\"mask\"") != expects_mask
    {
        return Err(format!("multipart wire shape drifted: {request:?}").into());
    }
    Ok(())
}
