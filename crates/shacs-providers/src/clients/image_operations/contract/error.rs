use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageOperationContractError {
    MissingSource,
    MissingMask,
    EmptyPayload,
    MalformedPart,
    PayloadTooLarge { byte_len: usize, max_bytes: usize },
}

impl fmt::Display for ImageOperationContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSource => formatter.write_str("image operation source is required"),
            Self::MissingMask => formatter.write_str("image mask is required"),
            Self::EmptyPayload => formatter.write_str("image operation payload is empty"),
            Self::MalformedPart => formatter.write_str("image multipart part is malformed"),
            Self::PayloadTooLarge {
                byte_len,
                max_bytes,
            } => write!(
                formatter,
                "image operation payload is too large: {byte_len} > {max_bytes}"
            ),
        }
    }
}

impl std::error::Error for ImageOperationContractError {}
