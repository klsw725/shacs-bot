use serde_json::{json, Value};
use shacs_providers::{
    ImageGenerationHttpResponse, ImageGenerationHttpTransport, ImageGenerationRequestParts,
    ImageMultipartRequestParts, ProviderError,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct CapturingTransport {
    multipart: Arc<Mutex<Vec<ImageMultipartRequestParts>>>,
    response: ImageGenerationHttpResponse,
}

impl CapturingTransport {
    pub fn success() -> Self {
        Self::with_body(json!({"data": [{"b64_json": "aW1hZ2U="}]}))
    }

    pub fn with_body(body: Value) -> Self {
        Self {
            multipart: Arc::new(Mutex::new(Vec::new())),
            response: ImageGenerationHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body,
            },
        }
    }

    pub fn captured(&self) -> Result<Vec<ImageMultipartRequestParts>, ProviderError> {
        self.multipart
            .lock()
            .map(|requests| requests.clone())
            .map_err(lock_error)
    }
}

impl ImageGenerationHttpTransport for CapturingTransport {
    fn post_json(
        &self,
        _request: ImageGenerationRequestParts,
    ) -> Result<ImageGenerationHttpResponse, ProviderError> {
        Ok(self.response.clone())
    }

    fn post_multipart(
        &self,
        request: ImageMultipartRequestParts,
    ) -> Result<ImageGenerationHttpResponse, ProviderError> {
        self.multipart.lock().map_err(lock_error)?.push(request);
        Ok(self.response.clone())
    }
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: error.to_string(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
