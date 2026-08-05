use super::{
    OnboardWizardExternalOwnerFact, OnboardWizardReport, OnboardWizardResumeState,
    OnboardWizardStatus,
};

pub(crate) fn prompt(state: &OnboardWizardResumeState, resumed: bool) -> String {
    format!(
        "Onboard wizard: refs={} resumed={}. Commands: provider <provider-id> env <ENV_VAR>, finish, cancel, restart, help.",
        state.provider_secret_refs.len(),
        crate::yes_no_label(resumed)
    )
}

pub(crate) fn report(
    status: OnboardWizardStatus,
    resumed: bool,
    state: OnboardWizardResumeState,
    external_owner_facts: Vec<OnboardWizardExternalOwnerFact>,
    readiness_lines: Vec<String>,
) -> OnboardWizardReport {
    OnboardWizardReport {
        status,
        resumed,
        provider_secret_refs: state.provider_secret_refs,
        external_owner_facts,
        readiness_lines,
    }
}
