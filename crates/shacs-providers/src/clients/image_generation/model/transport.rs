use super::{
    api_error, ImageGenerationHttpResponse, ImageGenerationHttpTransport,
    ImageGenerationRequestParts, ImageMultipartRequestParts, UreqImageGenerationHttpTransport,
    DEFAULT_IMAGE_GENERATION_TIMEOUT, IMAGE_GENERATION_RESPONSE_BODY_TOO_LARGE_CODE,
    IMAGE_GENERATION_RESPONSE_MAX_BYTES, IMAGE_GENERATION_RESPONSE_READ_LIMIT,
};
use crate::error::ProviderError;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

impl UreqImageGenerationHttpTransport {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_timeout(base_url, DEFAULT_IMAGE_GENERATION_TIMEOUT)
    }

    pub fn with_timeout(base_url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            base_url: base_url.into(),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(timeout))
                .http_status_as_error(false)
                .build()
                .new_agent(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl ImageGenerationHttpTransport for UreqImageGenerationHttpTransport {
    fn post_json(
        &self,
        request: ImageGenerationRequestParts,
    ) -> Result<ImageGenerationHttpResponse, ProviderError> {
        let url = join_base_and_path(&self.base_url, &request.path)?;
        let mut http_request = self
            .agent
            .post(&url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json");
        for (key, value) in &request.headers {
            http_request = http_request.header(key, value);
        }
        let body = serde_json::to_string(&request.body)
            .map_err(|error| api_error(None, error, false, BTreeMap::new(), None))?;
        let mut response = http_request.send(body).map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        let headers = response_headers(response.headers());
        let body_text = read_response_body(&mut response, status)?;
        Ok(ImageGenerationHttpResponse {
            status,
            headers,
            body: parse_http_body(body_text),
        })
    }

    fn post_multipart(
        &self,
        request: ImageMultipartRequestParts,
    ) -> Result<ImageGenerationHttpResponse, ProviderError> {
        let url = join_base_and_path(&self.base_url, &request.path)?;
        let mut http_request = self
            .agent
            .post(&url)
            .header("Accept", "application/json")
            .header("Content-Type", &request.content_type);
        for (key, value) in &request.headers {
            http_request = http_request.header(key, value);
        }
        let mut response = http_request.send(request.body).map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        let headers = response_headers(response.headers());
        let body_text = read_response_body(&mut response, status)?;
        Ok(ImageGenerationHttpResponse {
            status,
            headers,
            body: parse_http_body(body_text),
        })
    }
}

fn join_base_and_path(base_url: &str, path: &str) -> Result<String, ProviderError> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        return Err(api_error(
            None,
            "missing OpenAI image generation base URL",
            false,
            BTreeMap::new(),
            None,
        ));
    }
    Ok(format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    ))
}

fn response_headers(headers: &ureq::http::HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter(|(key, _)| key.as_str().eq_ignore_ascii_case("x-request-id"))
        .filter_map(|(key, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (key.as_str().to_owned(), value.to_owned()))
        })
        .collect()
}

fn read_response_body(
    response: &mut ureq::http::Response<ureq::Body>,
    status: u16,
) -> Result<String, ProviderError> {
    if response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > IMAGE_GENERATION_RESPONSE_MAX_BYTES)
    {
        return Err(api_error(
            Some(status),
            IMAGE_GENERATION_RESPONSE_BODY_TOO_LARGE_CODE,
            false,
            BTreeMap::new(),
            None,
        ));
    }
    let mut body = String::new();
    response
        .body_mut()
        .with_config()
        .limit(IMAGE_GENERATION_RESPONSE_READ_LIMIT)
        .reader()
        .take(IMAGE_GENERATION_RESPONSE_READ_LIMIT)
        .read_to_string(&mut body)
        .map_err(|_| {
            api_error(
                Some(status),
                "image_generation_response_read_failed",
                false,
                BTreeMap::new(),
                None,
            )
        })?;
    if body.len() > IMAGE_GENERATION_RESPONSE_MAX_BYTES {
        return Err(api_error(
            Some(status),
            IMAGE_GENERATION_RESPONSE_BODY_TOO_LARGE_CODE,
            false,
            BTreeMap::new(),
            None,
        ));
    }
    Ok(body)
}

fn parse_http_body(body: String) -> Value {
    if body.trim().is_empty() {
        return Value::Null;
    }
    serde_json::from_str(&body).unwrap_or(Value::String(body))
}

fn map_ureq_error(error: ureq::Error) -> ProviderError {
    let retryable = matches!(
        error,
        ureq::Error::Timeout(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
            | ureq::Error::Io(_)
    );
    let status = match error {
        ureq::Error::StatusCode(status) => Some(status),
        _ => None,
    };
    api_error(
        status,
        "image_generation_transport_error",
        retryable,
        BTreeMap::new(),
        None,
    )
}
