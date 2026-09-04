use super::super::model::{Spec031ReleaseArtifactError, Spec031ReleaseCommandRecord};

pub(super) fn find_command<'a>(
    commands: &'a [Spec031ReleaseCommandRecord],
    id: &str,
) -> Result<&'a Spec031ReleaseCommandRecord, Spec031ReleaseArtifactError> {
    commands
        .iter()
        .find(|command| command.id == id)
        .ok_or(Spec031ReleaseArtifactError::UnmappedCoverageRequirement)
}
