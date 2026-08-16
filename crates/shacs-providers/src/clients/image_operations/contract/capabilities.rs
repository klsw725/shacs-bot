#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOperation {
    Generate,
    Edit,
    Mask,
    Variation,
}

impl ImageOperation {
    pub const fn capability_name(self) -> &'static str {
        match self {
            Self::Generate => "image_generation",
            Self::Edit => "image_edit",
            Self::Mask => "image_mask",
            Self::Variation => "image_variation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageOperationCapabilities {
    pub provider_id: String,
    pub generate: bool,
    pub edit: bool,
    pub mask: bool,
    pub variation: bool,
}

impl ImageOperationCapabilities {
    pub const fn supports(&self, operation: ImageOperation) -> bool {
        match operation {
            ImageOperation::Generate => self.generate,
            ImageOperation::Edit => self.edit,
            ImageOperation::Mask => self.mask,
            ImageOperation::Variation => self.variation,
        }
    }
}

pub fn image_operation_capabilities(provider_id: &str) -> ImageOperationCapabilities {
    let (generate, edit, mask, variation) = match provider_id {
        "openai" => (true, true, true, false),
        "openrouter" | "openai_codex" => (true, false, false, false),
        _ => (false, false, false, false),
    };
    ImageOperationCapabilities {
        provider_id: provider_id.to_owned(),
        generate,
        edit,
        mask,
        variation,
    }
}
