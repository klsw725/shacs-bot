use crate::Session;
use serde::{Deserialize, Serialize};

pub const MAX_PERMISSION_APPROVAL_RECEIPTS: usize = 32;
const METADATA_KEY: &str = "permission_approval_receipts";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionApprovalTerminalState {
    Consumed,
    Denied,
    Expired,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionApprovalReceipt {
    pub approval_request_id: String,
    pub action_digest: String,
    pub snapshot_digest: String,
    pub state: PermissionApprovalTerminalState,
    pub recorded_at_unix_ms: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionApprovalReceipts {
    schema_version: u32,
    session_key: String,
    receipts: Vec<PermissionApprovalReceipt>,
}

impl Session {
    pub fn permission_approval_receipts(&self) -> Vec<PermissionApprovalReceipt> {
        let Some(value) = self.metadata.get(METADATA_KEY) else {
            return Vec::new();
        };
        let Ok(history) = serde_json::from_value::<PermissionApprovalReceipts>(value.clone())
        else {
            return Vec::new();
        };
        if history.schema_version != 1
            || history.session_key != self.key
            || history.receipts.len() > MAX_PERMISSION_APPROVAL_RECEIPTS
            || history.receipts.iter().any(|receipt| !receipt.is_valid())
        {
            return Vec::new();
        }
        history.receipts
    }

    pub fn record_permission_approval_receipt(&mut self, receipt: PermissionApprovalReceipt) {
        if !receipt.is_valid() {
            return;
        }
        let mut receipts = self.permission_approval_receipts();
        if receipts
            .iter()
            .any(|prior| prior.approval_request_id == receipt.approval_request_id)
        {
            return;
        }
        if receipts.len() == MAX_PERMISSION_APPROVAL_RECEIPTS {
            receipts.remove(0);
        }
        receipts.push(receipt);
        self.metadata.insert(
            METADATA_KEY.to_owned(),
            serde_json::json!(PermissionApprovalReceipts {
                schema_version: 1,
                session_key: self.key.clone(),
                receipts,
            }),
        );
    }
}

impl PermissionApprovalReceipt {
    fn is_valid(&self) -> bool {
        !self.approval_request_id.is_empty()
            && self.approval_request_id.len() <= 128
            && self
                .approval_request_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-:.".contains(&byte))
            && [&self.action_digest, &self.snapshot_digest]
                .into_iter()
                .all(|digest| {
                    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
    }
}
