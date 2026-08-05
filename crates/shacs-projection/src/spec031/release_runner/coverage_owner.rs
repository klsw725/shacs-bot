use super::coverage::Spec031ExternalOwnerId;
use super::model::Spec031ReleaseArtifactError;

pub(super) fn owner_from_requirement(
    requirement_id: &str,
) -> Result<Spec031ExternalOwnerId, Spec031ReleaseArtifactError> {
    match requirement_id.strip_prefix("spec031:external:") {
        Some("spec029") => Ok(Spec031ExternalOwnerId::Spec029),
        Some("spec030") => Ok(Spec031ExternalOwnerId::Spec030),
        Some("spec032") => Ok(Spec031ExternalOwnerId::Spec032),
        Some("spec033") => Ok(Spec031ExternalOwnerId::Spec033),
        Some("spec034") => Ok(Spec031ExternalOwnerId::Spec034),
        Some("spec035") => Ok(Spec031ExternalOwnerId::Spec035),
        Some(_) | None => Err(Spec031ReleaseArtifactError::UnknownCoverageRequirement),
    }
}
