use super::Spec034ReleaseArtifactError;

pub(in crate::runtime::generated_media_release) fn reject_behavior_config(
    bytes: &[u8],
) -> Result<(), Spec034ReleaseArtifactError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?
        .to_ascii_lowercase();
    let forbidden = [
        "[include",
        "worktree =",
        "hookspath",
        "[alias]",
        "[filter ",
        "[diff ",
        "[merge ",
        "textconv",
        "external",
        "fsmonitor",
        "credential",
        "pager",
    ];
    (!forbidden.iter().any(|value| text.contains(value)))
        .then_some(())
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)
}
