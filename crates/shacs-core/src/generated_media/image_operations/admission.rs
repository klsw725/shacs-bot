use super::{
    ArtifactImageOperationRequest, ImageOperationAdmissionError, ValidatedLocalImage,
    MAX_IMAGE_OPERATION_SOURCE_BYTES,
};
use crate::generated_media::{
    ArtifactId, CommittedArtifact, GeneratedArtifactRef, GeneratedMediaKind, GenerationOperation,
};
use shacs_providers::{
    ImageEditRequest, ImageFileInput, ImageGenerationResult, ImageMaskRequest,
    ImageOperationRequest, ImageOperationResult, ImageVariationRequest,
};

pub(super) enum ExpectedOperation {
    Edit,
    Mask,
    Variation,
}

pub(super) fn source_refs(
    request: &ArtifactImageOperationRequest,
) -> Result<Vec<&GeneratedArtifactRef>, ImageOperationAdmissionError> {
    match request {
        ArtifactImageOperationRequest::Edit { source, .. }
        | ArtifactImageOperationRequest::Variation { source, .. } => Ok(vec![source]),
        ArtifactImageOperationRequest::Mask {
            source,
            mask: Some(mask),
            ..
        } => Ok(vec![source, mask]),
        ArtifactImageOperationRequest::Mask { mask: None, .. } => {
            Err(ImageOperationAdmissionError::MissingMask)
        }
    }
}

pub(super) fn validate_source(
    committed: &CommittedArtifact,
    bytes: &[u8],
) -> Result<(), ImageOperationAdmissionError> {
    if committed.kind != GeneratedMediaKind::Image {
        return Err(ImageOperationAdmissionError::InvalidSourceKind);
    }
    if !matches!(
        committed.mime_type.as_str(),
        "image/png" | "image/jpeg" | "image/webp"
    ) {
        return Err(ImageOperationAdmissionError::InvalidSourceMime);
    }
    if bytes.len() > MAX_IMAGE_OPERATION_SOURCE_BYTES {
        return Err(ImageOperationAdmissionError::SourceTooLarge);
    }
    if !mime_matches(&committed.mime_type, bytes) {
        return Err(ImageOperationAdmissionError::MimeMismatch);
    }
    Ok(())
}

pub(super) fn prepare_provider_request(
    request: ArtifactImageOperationRequest,
    inputs: Vec<ImageFileInput>,
) -> Result<
    (
        ExpectedOperation,
        GenerationOperation,
        ImageOperationRequest,
    ),
    ImageOperationAdmissionError,
> {
    let mut inputs = inputs.into_iter();
    match request {
        ArtifactImageOperationRequest::Edit {
            prompt, options, ..
        } => {
            let source = next_input(&mut inputs)?;
            let mut request = ImageEditRequest::new(prompt, source);
            request.options = options;
            Ok((
                ExpectedOperation::Edit,
                GenerationOperation::Edit,
                ImageOperationRequest::Edit(request),
            ))
        }
        ArtifactImageOperationRequest::Mask {
            prompt, options, ..
        } => {
            let source = next_input(&mut inputs)?;
            let mask = next_input(&mut inputs)?;
            let mut request = ImageMaskRequest::new(prompt, source, mask);
            request.options = options;
            Ok((
                ExpectedOperation::Mask,
                GenerationOperation::Edit,
                ImageOperationRequest::Mask(request),
            ))
        }
        ArtifactImageOperationRequest::Variation { options, .. } => {
            let source = next_input(&mut inputs)?;
            let mut request = ImageVariationRequest::new(source);
            request.options = options;
            Ok((
                ExpectedOperation::Variation,
                GenerationOperation::Variation,
                ImageOperationRequest::Variation(request),
            ))
        }
    }
}

pub(super) fn expected_result(
    expected: ExpectedOperation,
    result: ImageOperationResult,
) -> Result<ImageGenerationResult, ImageOperationAdmissionError> {
    match (expected, result) {
        (ExpectedOperation::Edit, ImageOperationResult::Edit(result))
        | (ExpectedOperation::Mask, ImageOperationResult::Mask(result))
        | (ExpectedOperation::Variation, ImageOperationResult::Variation(result)) => Ok(result),
        (ExpectedOperation::Edit, _)
        | (ExpectedOperation::Mask, _)
        | (ExpectedOperation::Variation, _) => {
            Err(ImageOperationAdmissionError::InvalidProviderResult)
        }
    }
}

pub(super) fn validate_provider_result(
    result: ImageGenerationResult,
) -> Result<ValidatedLocalImage, ImageOperationAdmissionError> {
    if result.images.len() != 1 || !result.remote_images.is_empty() {
        return Err(ImageOperationAdmissionError::InvalidProviderResult);
    }
    let image = &result.images[0];
    if image.bytes.is_empty()
        || image.byte_len != image.bytes.len()
        || image.bytes.len() > MAX_IMAGE_OPERATION_SOURCE_BYTES
        || !mime_matches(image.mime_type.as_str(), &image.bytes)
    {
        return Err(ImageOperationAdmissionError::InvalidProviderResult);
    }
    Ok(ValidatedLocalImage { result })
}

pub(super) fn source_file_name(artifact_id: &ArtifactId, mime_type: &str) -> String {
    let extension = match mime_type {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    };
    format!("{}.{}", artifact_id.as_str(), extension)
}

fn next_input(
    inputs: &mut impl Iterator<Item = ImageFileInput>,
) -> Result<ImageFileInput, ImageOperationAdmissionError> {
    inputs
        .next()
        .ok_or(ImageOperationAdmissionError::InvalidSourceKind)
}

fn mime_matches(mime_type: &str, bytes: &[u8]) -> bool {
    match mime_type {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}
