use super::*;

pub(super) fn normalize_owner_facts(
    input: Spec035MediaOwnerFactsInput,
) -> (Spec035MediaDisclosure, Spec035MediaOwnerFacts) {
    let mut unavailable_reasons = input.unavailable_reasons;
    unavailable_reasons.sort();
    let mut output = Spec035MediaOwnerFacts {
        unavailable_reasons,
        analyzer_source: None,
        sandbox: None,
        credential: None,
        snapshot: None,
    };
    let mut disclosure = Spec035MediaDisclosure::Unavailable;
    for fact in input.facts {
        match fact {
            Spec035MediaOwnerFactInput::AnalyzerSource {
                analyzer_ref,
                source,
                activation,
                trust,
                trusted_code_disclosure,
            } => {
                output.analyzer_source = Some(Spec035MediaAnalyzerSourceFact {
                    analyzer_ref,
                    source,
                    activation,
                    trust,
                    trusted_code_disclosure,
                });
            }
            Spec035MediaOwnerFactInput::Sandbox(mut value) => {
                value.applied_adapters.sort_by_key(|adapter| match adapter {
                    crate::ProcessAdapterKind::Bash => 0,
                    crate::ProcessAdapterKind::GenericExec => 1,
                    crate::ProcessAdapterKind::CredentialCommand => 2,
                    crate::ProcessAdapterKind::PackageOperation => 3,
                    crate::ProcessAdapterKind::PythonKernel => 4,
                    crate::ProcessAdapterKind::DaemonWorker => 5,
                    crate::ProcessAdapterKind::Mcp => 6,
                });
                output.sandbox = Some(value);
            }
            Spec035MediaOwnerFactInput::Credential(value) => output.credential = Some(value),
            Spec035MediaOwnerFactInput::Disclosure(mut value) => {
                value.surfaces.sort_by_key(|surface| match surface {
                    crate::DataSurface::Session => 0,
                    crate::DataSurface::Log => 1,
                    crate::DataSurface::Trace => 2,
                    crate::DataSurface::ToolOutput => 3,
                    crate::DataSurface::ExtensionData => 4,
                });
                disclosure = Spec035MediaDisclosure::Recorded(value);
            }
            Spec035MediaOwnerFactInput::Snapshot {
                snapshot_ref,
                provenance_digest,
            } => {
                output.snapshot = Some(Spec035MediaSnapshotFact {
                    snapshot_ref,
                    provenance_digest,
                });
            }
        }
    }
    (disclosure, output)
}
