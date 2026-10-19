use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shacs_session::Session;

use super::{EntrustedWork, EntrustedWorkMetadataError, ENTRUSTED_WORK_METADATA_KEY};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntrustedWorkStore {
    works: Vec<EntrustedWork>,
}

pub(super) fn work_id(goal_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(goal_id.as_bytes());
    let digest = hasher.finalize();
    format!(
        "work-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5]
    )
}

pub(super) fn entrusted_works_from_session(
    session: &Session,
) -> Result<Vec<EntrustedWork>, EntrustedWorkMetadataError> {
    session
        .metadata
        .get(ENTRUSTED_WORK_METADATA_KEY)
        .cloned()
        .map(serde_json::from_value::<EntrustedWorkStore>)
        .transpose()
        .map(|store| store.unwrap_or_default().works)
        .map_err(EntrustedWorkMetadataError::Deserialize)
}

pub(super) fn store_entrusted_works(
    session: &mut Session,
    works: &[EntrustedWork],
) -> Result<(), EntrustedWorkMetadataError> {
    let value = serde_json::to_value(EntrustedWorkStore {
        works: works.to_vec(),
    })
    .map_err(EntrustedWorkMetadataError::Serialize)?;
    session
        .metadata
        .insert(ENTRUSTED_WORK_METADATA_KEY.to_owned(), value);
    Ok(())
}
