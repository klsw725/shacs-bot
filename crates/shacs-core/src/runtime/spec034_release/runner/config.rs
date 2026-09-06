use super::{Spec034ReleaseArtifactError, Spec034ReleaseConfig};

const MAX_RUN_ID_LEN: usize = 80;
const MAX_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(7_200);

pub(super) fn validate(
    config: &Spec034ReleaseConfig,
) -> Result<(), Spec034ReleaseArtifactError> {
    (valid_run_id(&config.run_id)
        && !config.command_timeout.is_zero()
        && config.command_timeout <= MAX_COMMAND_TIMEOUT)
        .then_some(())
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)
}

pub(super) fn valid_run_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RUN_ID_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
