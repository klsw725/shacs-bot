use super::{ImageFileInput, ImageOperation, ImageOperationContractError};
use crate::clients::image_generation::{ImageGenerationRequest, ImageGenerationResult};
use serde_json::{Map, Value};
use std::fmt;

#[derive(Clone, PartialEq, Default)]
pub struct ImageOperationOptions {
    pub model: Option<String>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub output_format: Option<String>,
    pub background: Option<String>,
    pub count: Option<u32>,
    pub provider_options: Map<String, Value>,
}

impl fmt::Debug for ImageOperationOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageOperationOptions")
            .field("model", &self.model)
            .field("size", &self.size)
            .field("quality", &self.quality)
            .field("output_format", &self.output_format)
            .field("background", &self.background)
            .field("count", &self.count)
            .field("provider_options", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageEditRequest {
    prompt: String,
    source: ImageFileInput,
    pub options: ImageOperationOptions,
}

impl ImageEditRequest {
    pub fn new(prompt: impl Into<String>, source: ImageFileInput) -> Self {
        Self {
            prompt: prompt.into(),
            source,
            options: ImageOperationOptions::default(),
        }
    }

    pub fn try_new(
        prompt: impl Into<String>,
        source: Option<ImageFileInput>,
    ) -> Result<Self, ImageOperationContractError> {
        source
            .map(|source| Self::new(prompt, source))
            .ok_or(ImageOperationContractError::MissingSource)
    }

    pub(crate) fn parts(&self) -> (&str, &ImageFileInput, &ImageOperationOptions) {
        (&self.prompt, &self.source, &self.options)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageMaskRequest {
    prompt: String,
    source: ImageFileInput,
    mask: ImageFileInput,
    pub options: ImageOperationOptions,
}

impl ImageMaskRequest {
    pub fn new(prompt: impl Into<String>, source: ImageFileInput, mask: ImageFileInput) -> Self {
        Self {
            prompt: prompt.into(),
            source,
            mask,
            options: ImageOperationOptions::default(),
        }
    }

    pub fn try_new(
        prompt: impl Into<String>,
        source: Option<ImageFileInput>,
        mask: Option<ImageFileInput>,
    ) -> Result<Self, ImageOperationContractError> {
        let source = source.ok_or(ImageOperationContractError::MissingSource)?;
        let mask = mask.ok_or(ImageOperationContractError::MissingMask)?;
        Ok(Self::new(prompt, source, mask))
    }

    pub(crate) fn parts(
        &self,
    ) -> (
        &str,
        &ImageFileInput,
        &ImageFileInput,
        &ImageOperationOptions,
    ) {
        (&self.prompt, &self.source, &self.mask, &self.options)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageVariationRequest {
    source: ImageFileInput,
    pub options: ImageOperationOptions,
}

impl ImageVariationRequest {
    pub fn new(source: ImageFileInput) -> Self {
        Self {
            source,
            options: ImageOperationOptions::default(),
        }
    }

    pub fn try_new(source: Option<ImageFileInput>) -> Result<Self, ImageOperationContractError> {
        source
            .map(Self::new)
            .ok_or(ImageOperationContractError::MissingSource)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImageOperationRequest {
    Generate(ImageGenerationRequest),
    Edit(ImageEditRequest),
    Mask(ImageMaskRequest),
    Variation(ImageVariationRequest),
}

impl ImageOperationRequest {
    pub const fn operation(&self) -> ImageOperation {
        match self {
            Self::Generate(_) => ImageOperation::Generate,
            Self::Edit(_) => ImageOperation::Edit,
            Self::Mask(_) => ImageOperation::Mask,
            Self::Variation(_) => ImageOperation::Variation,
        }
    }

    pub(crate) fn apply_default_model(&mut self, default_model: &str) {
        let model = match self {
            Self::Generate(request) => &mut request.model,
            Self::Edit(request) => &mut request.options.model,
            Self::Mask(request) => &mut request.options.model,
            Self::Variation(request) => &mut request.options.model,
        };
        if model.as_deref().map(str::trim).map_or(true, str::is_empty) {
            *model = Some(default_model.to_owned());
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImageOperationResult {
    Generate(ImageGenerationResult),
    Edit(ImageGenerationResult),
    Mask(ImageGenerationResult),
    Variation(ImageGenerationResult),
}
