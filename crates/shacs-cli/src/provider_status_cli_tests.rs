//! Spec 052 PRD 000 acceptance 5: official-container evidence turns the
//! advice into Compose `shacs-cli` commands. PRD 040 acceptance 5: a
//! credential saved under another declaration is restored, or saved again
//! when the declaration lets the auth store supply it.

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

fn saved_under_another_declaration(
    provider_id: &str,
    family: CredentialFamily,
    environment: Option<&str>,
    local_auth_allowed: bool,
) -> ProviderCredentialPlan {
    let mut plan = plan(provider_id, family, None);
    if let Some(provider) = plan.selected.as_mut() {
        provider.declared_environment = environment.map(str::to_owned);
        provider.local_auth_allowed = local_auth_allowed;
        provider.local_auth_skip = local_auth_allowed.then_some(LocalAuthSkip::FingerprintStale);
        provider.fingerprint_differs = true;
    }
    plan
}

#[test]
fn a_credential_saved_under_another_declaration_is_restored_or_saved_again() {
    // Given
    let restore = |id: &str| {
        format!(
            "restore providers.{id}.credentialSource and apiKeyRef to what they were when it was saved, or "
        )
    };
    let cases = [
        (
            saved_under_another_declaration("openai_codex", CredentialFamily::OAuth, None, true),
            CommandForm::Host,
            format!(
                "{}save it again: shacs-bot provider codex login",
                restore("openai_codex")
            ),
        ),
        (
            saved_under_another_declaration("openai_codex", CredentialFamily::OAuth, None, true),
            CommandForm::OfficialContainer,
            format!(
                "{}save it again: docker compose run --rm shacs-cli provider codex login --headless",
                restore("openai_codex")
            ),
        ),
        (
            saved_under_another_declaration("github_copilot", CredentialFamily::OAuth, None, true),
            CommandForm::Host,
            format!(
                "{}save it again: shacs-bot provider copilot import-token --token-stdin",
                restore("github_copilot")
            ),
        ),
        (
            saved_under_another_declaration(
                "anthropic",
                CredentialFamily::ApiKey,
                Some("ANTHROPIC_API_KEY"),
                true,
            ),
            CommandForm::Host,
            format!(
                "{}save it again: shacs-bot provider anthropic import-key --token-stdin, or set \
                 ANTHROPIC_API_KEY",
                restore("anthropic")
            ),
        ),
        (
            saved_under_another_declaration(
                "anthropic",
                CredentialFamily::ApiKey,
                Some("ANTHROPIC_API_KEY"),
                true,
            ),
            CommandForm::OfficialContainer,
            format!(
                "{}save it again: docker compose run --rm shacs-cli provider anthropic import-key \
                 --token-stdin",
                restore("anthropic")
            ),
        ),
        (
            saved_under_another_declaration(
                "anthropic",
                CredentialFamily::ApiKey,
                Some("ANTHROPIC_API_KEY"),
                false,
            ),
            CommandForm::Host,
            format!(
                "{}set ANTHROPIC_API_KEY or providers.anthropic.apiKey",
                restore("anthropic")
            ),
        ),
        (
            saved_under_another_declaration("anthropic", CredentialFamily::ApiKey, None, false),
            CommandForm::OfficialContainer,
            format!(
                "{}set providers.anthropic.apiKey or a credential command",
                restore("anthropic")
            ),
        ),
    ];
    for (plan, form, expected) in cases {
        // When
        let advice = next(&plan, form);

        // Then
        assert_eq!(advice, expected);
    }
}
