use shacs_config::{
    CommandCredentialInput, CommandCredentialOutcome, CredentialFamily, CredentialFingerprint,
    CredentialResolutionInput, CredentialSource, CredentialSourceDeclaration, ProviderConfig,
    RawCredential,
};

fn declaration(family: CredentialFamily) -> CredentialSourceDeclaration {
    CredentialSourceDeclaration {
        family,
        environment: Some("SHACS_TEST_KEY".to_owned()),
        local_auth: true,
        command: Some("credential-helper".to_owned()),
    }
}

#[test]
fn spec030_auth_runtime_override_wins_with_distinct_api_key_values() {
    let provider = ProviderConfig {
        api_key: Some("literal-api".to_owned()),
        ..ProviderConfig::default()
    };
    let input = CredentialResolutionInput {
        runtime_override: Some(RawCredential::api_key("runtime-api")),
        environment: Some(RawCredential::api_key("environment-api")),
        local_auth: Some(RawCredential::api_key("local-api")),
        local_auth_fingerprint: None,
        command: CommandCredentialInput::succeeded("command-api"),
    };

    let resolved = declaration(CredentialFamily::ApiKey)
        .resolve(&provider, input)
        .expect("runtime override resolves");

    assert_eq!(resolved.source(), CredentialSource::RuntimeOverride);
    assert_eq!(resolved.transport().value(), "runtime-api");
}

#[test]
fn spec030_auth_precedence_is_deterministic_for_oauth_family() {
    let provider = ProviderConfig {
        api_key: Some("literal-bearer".to_owned()),
        ..ProviderConfig::default()
    };
    let input = CredentialResolutionInput {
        runtime_override: None,
        environment: Some(RawCredential::oauth("environment-access", None, None)),
        local_auth: Some(RawCredential::oauth("local-access", None, None)),
        local_auth_fingerprint: None,
        command: CommandCredentialInput::succeeded("command-access"),
    };

    let resolved = declaration(CredentialFamily::OAuth)
        .resolve(&provider, input)
        .expect("environment resolves");

    assert_eq!(resolved.source(), CredentialSource::Environment);
    assert_eq!(resolved.transport().value(), "environment-access");
}

#[test]
fn spec030_auth_stale_local_source_transitions_to_command() {
    let declaration = declaration(CredentialFamily::OAuth);
    let input = CredentialResolutionInput {
        runtime_override: None,
        environment: None,
        local_auth: Some(RawCredential::oauth("stale-access", None, None)),
        local_auth_fingerprint: Some(CredentialFingerprint::from_descriptor("old-source")),
        command: CommandCredentialInput::succeeded("command-access"),
    };

    let resolved = declaration
        .resolve(&ProviderConfig::default(), input)
        .expect("command resolves after stale local source");

    assert_eq!(resolved.source(), CredentialSource::Command);
    assert_eq!(resolved.transport().value(), "command-access");
}

#[test]
fn spec030_auth_local_store_wins_over_command_and_literal() {
    let provider = ProviderConfig {
        api_key: Some("literal-api".to_owned()),
        ..ProviderConfig::default()
    };
    let input = CredentialResolutionInput {
        local_auth: Some(RawCredential::api_key("local-api")),
        command: CommandCredentialInput::succeeded("command-api"),
        ..CredentialResolutionInput::default()
    };

    let resolved = declaration(CredentialFamily::ApiKey)
        .resolve(&provider, input)
        .expect("local auth resolves");

    assert_eq!(resolved.source(), CredentialSource::LocalAuthStore);
    assert_eq!(resolved.transport().value(), "local-api");
}

#[test]
fn spec030_auth_command_wins_over_oauth_literal() {
    let provider = ProviderConfig {
        api_key: Some("literal-bearer".to_owned()),
        ..ProviderConfig::default()
    };

    let resolved = declaration(CredentialFamily::OAuth)
        .resolve(
            &provider,
            CredentialResolutionInput {
                command: CommandCredentialInput::succeeded("command-access"),
                ..CredentialResolutionInput::default()
            },
        )
        .expect("command resolves");

    assert_eq!(resolved.source(), CredentialSource::Command);
    assert_eq!(resolved.transport().value(), "command-access");
}

#[test]
fn spec030_auth_command_nonzero_and_empty_block_literal_fallback() {
    let provider = ProviderConfig {
        api_key: Some("literal-api".to_owned()),
        ..ProviderConfig::default()
    };
    for command in [
        CommandCredentialInput::result(CommandCredentialOutcome::NonZero, "ignored"),
        CommandCredentialInput::succeeded("  \n"),
    ] {
        let error = declaration(CredentialFamily::ApiKey)
            .resolve(
                &provider,
                CredentialResolutionInput {
                    command,
                    ..CredentialResolutionInput::default()
                },
            )
            .expect_err("failed command blocks provider literal");
        assert_eq!(
            error.status().status,
            shacs_config::CredentialStatus::Missing
        );
        assert_eq!(error.status().source, Some(CredentialSource::Command));
    }
}

#[test]
fn spec030_auth_command_cache_input_is_consumed_without_execution() {
    let cached = RawCredential::api_key("cached-api");
    let resolved = declaration(CredentialFamily::ApiKey)
        .resolve(
            &ProviderConfig::default(),
            CredentialResolutionInput {
                command: CommandCredentialInput::cached(cached),
                ..CredentialResolutionInput::default()
            },
        )
        .expect("cached command value resolves");

    assert_eq!(resolved.source(), CredentialSource::Command);
    assert_eq!(resolved.transport().value(), "cached-api");
}

#[test]
fn spec030_auth_raw_holder_debug_and_status_serialization_exclude_canaries() {
    let credential =
        RawCredential::oauth("ACCESS_CANARY", Some("REFRESH_CANARY".to_owned()), Some(10));
    let resolved = declaration(CredentialFamily::OAuth)
        .resolve(
            &ProviderConfig::default(),
            CredentialResolutionInput {
                runtime_override: Some(credential),
                ..CredentialResolutionInput::default()
            },
        )
        .expect("OAuth override resolves");

    let debug = format!("{resolved:?}");
    let status = serde_json::to_string(&resolved.status()).expect("status serializes");
    for canary in ["ACCESS_CANARY", "REFRESH_CANARY"] {
        assert!(!debug.contains(canary));
        assert!(!status.contains(canary));
    }
    let api = declaration(CredentialFamily::ApiKey)
        .resolve(
            &ProviderConfig::default(),
            CredentialResolutionInput {
                runtime_override: Some(RawCredential::api_key("API_CANARY")),
                ..CredentialResolutionInput::default()
            },
        )
        .expect("API key override resolves");
    assert!(!format!("{api:?}").contains("API_CANARY"));
    assert!(!serde_json::to_string(&api.status())
        .expect("API key status serializes")
        .contains("API_CANARY"));
}
