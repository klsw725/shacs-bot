mod configured;
mod discovered;
mod trace;

use super::{Spec030FactStore, Spec030FactStoreError};
use crate::controlled_child::ControlledChildAbort;
use crate::runtime::trusted_resources::{inspect_resources, WorkspaceResourceTrust};
use shacs_config::ConfigBundle;

pub fn populate(
    facts: &Spec030FactStore,
    bundle: &ConfigBundle,
) -> Result<(), Spec030FactStoreError> {
    let mut candidates = discovered::candidates(bundle);
    candidates.extend(configured::candidates(bundle));
    let trust = if bundle
        .config
        .plugins
        .trusts_workspace(&bundle.context.workspace)
    {
        WorkspaceResourceTrust::Trusted
    } else {
        WorkspaceResourceTrust::Untrusted
    };
    let inspection = inspect_resources(candidates, trust, &ControlledChildAbort::new());
    facts.record_resource_inspection(&inspection)?;
    facts.update_trace(trace::disclosure(bundle))
}
