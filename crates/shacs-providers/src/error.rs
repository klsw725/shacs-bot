use std::collections::BTreeMap;
use std::fmt::{self, Display};

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderError {
    ProviderNotFound {
        provider_id: String,
        suggestions: Vec<String>,
    },
    ModelNotFound {
        provider_id: String,
        model_id: String,
        suggestions: Vec<String>,
    },
    AuthRequired {
        provider_id: String,
    },
    Api {
        status: Option<u16>,
        message: String,
        retryable: bool,
        headers: BTreeMap<String, String>,
        body: Option<String>,
    },
}

impl Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderNotFound { provider_id, .. } => {
                write!(formatter, "provider not found: {provider_id}")
            }
            Self::ModelNotFound {
                provider_id,
                model_id,
                ..
            } => write!(formatter, "model not found: {provider_id}/{model_id}"),
            Self::AuthRequired { provider_id } => write!(formatter, "auth required: {provider_id}"),
            Self::Api { message, .. } => write!(formatter, "provider API error: {message}"),
        }
    }
}

impl std::error::Error for ProviderError {}
