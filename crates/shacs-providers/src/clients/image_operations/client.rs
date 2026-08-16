use super::{build_openai_multipart_request, ImageOperationRequest, ImageOperationResult};
use crate::clients::image_generation::{
    parse_openai_image_generation_response, ImageGenerationHttpTransport,
};
use crate::error::ProviderError;

pub(crate) fn execute_openai_image_operation<T>(
    transport: &T,
    api_key: &str,
    default_model: &str,
    mut request: ImageOperationRequest,
) -> Result<ImageOperationResult, ProviderError>
where
    T: ImageGenerationHttpTransport,
{
    request.apply_default_model(default_model);
    match request {
        ImageOperationRequest::Generate(_) => Err(ProviderError::UnsupportedCapability {
            provider_id: "openai".to_owned(),
            capability: "internal_generate_dispatch".to_owned(),
        }),
        request @ ImageOperationRequest::Edit(_) => {
            let model = operation_model(&request, default_model);
            let response = transport
                .post_multipart(build_openai_multipart_request(api_key, &request, model)?)?;
            parse_openai_image_generation_response(response, model).map(ImageOperationResult::Edit)
        }
        request @ ImageOperationRequest::Mask(_) => {
            let model = operation_model(&request, default_model);
            let response = transport
                .post_multipart(build_openai_multipart_request(api_key, &request, model)?)?;
            parse_openai_image_generation_response(response, model).map(ImageOperationResult::Mask)
        }
        ImageOperationRequest::Variation(_) => Err(ProviderError::UnsupportedCapability {
            provider_id: "openai".to_owned(),
            capability: "image_variation".to_owned(),
        }),
    }
}

fn operation_model<'a>(request: &'a ImageOperationRequest, default_model: &'a str) -> &'a str {
    match request {
        ImageOperationRequest::Edit(request) => request.parts().2.model.as_deref(),
        ImageOperationRequest::Mask(request) => request.parts().3.model.as_deref(),
        ImageOperationRequest::Generate(_) | ImageOperationRequest::Variation(_) => None,
    }
    .unwrap_or(default_model)
}
