use super::analyzer::AnalyzerReport;
use super::crash::CrashReport;
use super::docs_policy::DocumentationPolicyReport;
use super::edit::EditReport;
use super::lifecycle::LifecycleReport;
use super::remote::RemoteResult;
use super::replay::ReplayReport;
use super::secret_scan::SecretScanReport;
use super::surfaces::SurfaceReport;
use shacs_core::generated_media::GeneratedArtifactRecord;
use shacs_core::runtime::MediaEvidenceDiagnostics;

pub struct ReceiptInputs<'a> {
    pub lifecycle: &'a LifecycleReport,
    pub artifact: &'a GeneratedArtifactRecord,
    pub artifact_record_exists: bool,
    pub artifact_hash_consistent: bool,
    pub edit: &'a EditReport,
    pub remote: &'a RemoteResult,
    pub analyzer: &'a AnalyzerReport,
    pub diagnostics: &'a MediaEvidenceDiagnostics,
    pub replay: &'a ReplayReport,
    pub surfaces: &'a SurfaceReport,
    pub scan: &'a SecretScanReport,
    pub crash: &'a CrashReport,
    pub documentation_policy: &'a DocumentationPolicyReport,
}
