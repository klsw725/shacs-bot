mod admission;
mod types;

pub use types::{
    AdmittedImageOperation, ArtifactImageOperationRequest, ImageOperationAdmissionError,
    ValidatedImageOperationCandidate, ValidatedLocalImage,
};

use super::ArtifactStore;
use admission::{
    expected_result, prepare_provider_request, source_file_name, source_refs,
    validate_provider_result, validate_source,
};
use shacs_providers::{ImageFileInput, ImageGenerationClient, MAX_IMAGE_OPERATION_INPUT_BYTES};

pub const MAX_IMAGE_OPERATION_SOURCE_BYTES: usize = MAX_IMAGE_OPERATION_INPUT_BYTES;

pub struct ImageOperationService<'a> {
    store: &'a ArtifactStore,
    client: &'a dyn ImageGenerationClient,
}

impl<'a> ImageOperationService<'a> {
    pub const fn new(store: &'a ArtifactStore, client: &'a dyn ImageGenerationClient) -> Self {
        Self { store, client }
    }

    pub fn admit(
        &self,
        request: ArtifactImageOperationRequest,
    ) -> Result<AdmittedImageOperation, ImageOperationAdmissionError> {
        let refs = source_refs(&request)?;
        let mut committed_refs = Vec::with_capacity(refs.len());
        for artifact_ref in refs {
            let (committed, bytes) = self.store.read_committed_ref(artifact_ref)?;
            validate_source(&committed, &bytes)?;
            committed_refs.push(committed.artifact_ref());
        }
        Ok(AdmittedImageOperation::new(request, committed_refs))
    }

    pub fn execute(
        &self,
        request: ArtifactImageOperationRequest,
    ) -> Result<ValidatedImageOperationCandidate, ImageOperationAdmissionError> {
        let admitted = self.admit(request)?;
        self.execute_admitted(admitted)
    }

    pub fn execute_admitted(
        &self,
        admitted: AdmittedImageOperation,
    ) -> Result<ValidatedImageOperationCandidate, ImageOperationAdmissionError> {
        let (request, committed_refs) = admitted.into_parts();
        let mut inputs = Vec::with_capacity(committed_refs.len());
        for artifact_ref in &committed_refs {
            let (committed, bytes) = self.store.read_committed_ref(artifact_ref)?;
            validate_source(&committed, &bytes)?;
            inputs.push(ImageFileInput::new(
                source_file_name(&artifact_ref.artifact_id, &committed.mime_type),
                committed.mime_type.clone(),
                bytes,
            )?);
        }
        let (expected, operation, provider_request) = prepare_provider_request(request, inputs)?;
        let result = expected_result(
            expected,
            self.client.execute_image_operation(provider_request)?,
        )?;
        let local_image = validate_provider_result(result)?;
        Ok(ValidatedImageOperationCandidate::new(
            operation,
            local_image,
            committed_refs
                .into_iter()
                .map(|artifact_ref| artifact_ref.artifact_id)
                .collect(),
        ))
    }
}
