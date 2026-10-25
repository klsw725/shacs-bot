use super::docs_policy::DocumentationPolicyReport;
use super::edit::EditReport;
use super::lifecycle::LifecycleReport;
use super::remote::RemoteResult;
use super::secret_scan::SecretScanReport;
use shacs_core::generated_media::GeneratedArtifactRecord;

pub struct ReceiptInputs<'a> {
    pub lifecycle: &'a LifecycleReport,
    pub artifact: &'a GeneratedArtifactRecord,
    pub artifact_record_exists: bool,
    pub artifact_hash_consistent: bool,
    pub edit: &'a EditReport,
    pub remote: &'a RemoteResult,
    pub scan: &'a SecretScanReport,
    pub documentation_policy: &'a DocumentationPolicyReport,
}
