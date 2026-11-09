//! Spec 052 PRD 000 acceptance 5: official-container evidence turns the
//! advice into Compose `shacs-cli` commands.

use super::super::{runtime_containment_classify, RuntimeContainmentEvidence};
use super::{judge, CommandForm, ProviderReadiness};
use shacs_config::{Config, CredentialFamily};
use shacs_core::runtime::{CredentialSlot, LocalAuthSkip, PlannedProvider, ProviderCredentialPlan};

fn form(sandbox: Option<&str>, official_package: bool, container_markers: &[&str]) -> CommandForm {
    CommandForm::of(&runtime_containment_classify(
        RuntimeContainmentEvidence::from_parts(sandbox, official_package, container_markers, &[]),
    ))
}

fn plan(
    provider_id: &str,
    family: CredentialFamily,
    slot: Option<CredentialSlot>,
) -> ProviderCredentialPlan {
    ProviderCredentialPlan {
        unknown_provider: false,
        selected: Some(PlannedProvider {
            provider_id: provider_id.to_owned(),
            model: "model".to_owned(),
            family,
            declared_environment: None,
            local_auth_allowed: true,
            slot,
            local_auth_skip: Some(LocalAuthSkip::NoEntry),
            fingerprint_differs: false,
        }),
        auth_store_unreadable: false,
    }
}

fn next(plan: &ProviderCredentialPlan, form: CommandForm) -> String {
    match judge(plan, &Config::default().agents.defaults, form) {
        ProviderReadiness::Missing { next, .. } => next,
        ProviderReadiness::Ready { credential, .. } => format!("ready: {credential}"),
    }
}

#[test]
fn official_container_evidence_selects_the_compose_command_form() {
    assert_eq!(
        form(None, true, &["dockerenv"]),
        CommandForm::OfficialContainer
    );
    assert_eq!(
        form(Some("bwrap"), true, &["dockerenv"]),
        CommandForm::OfficialContainer
    );
    assert_eq!(form(None, false, &["dockerenv"]), CommandForm::Host);
    assert_eq!(form(None, true, &[]), CommandForm::Host);
    assert_eq!(form(None, false, &[]), CommandForm::Host);
}

#[test]
fn advice_follows_the_command_form() {
    let codex = plan("openai_codex", CredentialFamily::OAuth, None);
    let empty_environment = plan(
        "anthropic",
        CredentialFamily::ApiKey,
        Some(CredentialSlot::Environment {
            name: "ANTHROPIC_API_KEY".to_owned(),
            empty: true,
        }),
    );

    assert_eq!(
        next(&codex, CommandForm::Host),
        "shacs-bot provider codex login"
    );
    assert_eq!(
        next(&codex, CommandForm::OfficialContainer),
        "docker compose run --rm shacs-cli provider codex login --headless"
    );
    assert_eq!(
        next(&empty_environment, CommandForm::Host),
        "set ANTHROPIC_API_KEY to the key, or unset it"
    );
    let container = next(&empty_environment, CommandForm::OfficialContainer);
    assert!(
        container.contains("ANTHROPIC_API_KEY=<key> in the .env file next to docker-compose.yml"),
        "{container}"
    );
}
