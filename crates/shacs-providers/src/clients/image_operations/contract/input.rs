use super::ImageOperationContractError;
use std::fmt;

pub const MAX_IMAGE_OPERATION_INPUT_BYTES: usize = 25 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct ImageFileInput {
    file_name: String,
    mime_type: String,
    bytes: Vec<u8>,
}

impl ImageFileInput {
    pub fn new(
        file_name: impl Into<String>,
        mime_type: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<Self, ImageOperationContractError> {
        if bytes.len() > MAX_IMAGE_OPERATION_INPUT_BYTES {
            return Err(ImageOperationContractError::PayloadTooLarge {
                byte_len: bytes.len(),
                max_bytes: MAX_IMAGE_OPERATION_INPUT_BYTES,
            });
        }
        let file_name = file_name.into();
        let mime_type = mime_type.into();
        if bytes.is_empty() {
            return Err(ImageOperationContractError::EmptyPayload);
        }
        if !valid_part_value(&file_name)
            || !valid_part_value(&mime_type)
            || !matches!(
                mime_type.as_str(),
                "image/png" | "image/jpeg" | "image/webp"
            )
        {
            return Err(ImageOperationContractError::MalformedPart);
        }
        Ok(Self {
            file_name,
            mime_type,
            bytes,
        })
    }

    pub(crate) fn parts(&self) -> (&str, &str, &[u8]) {
        (&self.file_name, &self.mime_type, &self.bytes)
    }
}

impl fmt::Debug for ImageFileInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageFileInput")
            .field("file_name", &self.file_name)
            .field("mime_type", &self.mime_type)
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

fn valid_part_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b'"' | b'\\'))
}
