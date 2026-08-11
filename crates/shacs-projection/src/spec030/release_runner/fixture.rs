use super::model::Spec030ReleaseArtifactError;
use crate::release_evidence::EvidenceWriter;

pub(super) fn prepare(writer: &EvidenceWriter) -> Result<(), Spec030ReleaseArtifactError> {
    writer
        .create_dir_all("fixtures/success/src")
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .create_dir_all("fixtures/success/tests")
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .create_dir_all("fixtures/success/crates/src")
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    for target in super::target_catalog::spec030_integration_targets() {
        writer
            .write_new(
                format!("fixtures/success/tests/{}.rs", target.target),
                b"#[test]\nfn spec030_fixture_target_runs() { assert!(true); }\n",
            )
            .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    }
    writer
        .write_new(
            "fixtures/success/tests/spec030_fixture.rs",
            b"#[test]\nfn spec030_fixture_gate_runs() { assert!(true); }\n",
        )
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new(
            "fixtures/success/crates/Cargo.toml",
            b"[package]\nname = \"shacs-projection\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new(
            "fixtures/success/crates/src/lib.rs",
            b"#[cfg(test)]\nmod tests { #[test] fn linux_bwrap_owner_lifecycle() { assert!(true); } }\n",
        )
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new(
            "fixtures/success/Cargo.toml",
            b"[package]\nname = \"spec030-success-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new(
            "fixtures/success/src/lib.rs",
            b"#[cfg(test)]\nmod tests { #[test] fn spec030_release_runner_success_fixture_passes() { assert!(true); } }\n",
        )
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .create_dir_all("surface")
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    let projection = serde_json::json!({
        "schemaVersion":1, "availability":"available", "status":"active",
        "profile":{"availability":"available","status":"active","profile":"trustedLocalAgent","executionAuthority":"currentOsUser","workspaceTrust":"userAsserted","resourceTrust":"explicitOrTrustedWorkspace","defaultContainment":"none","optionalSandbox":"adapterScoped"},
        "hooks":{"availability":"available","status":"active","registeredHandlers":1},
        "processAdapters":[
            {"support":"supported","capabilities":{"timeout":true,"abort":true,"cwd":true,"boundedOutput":true,"descendantCleanup":true,"startupReadiness":false}},
            {"support":"supported","capabilities":{"timeout":false,"abort":false,"cwd":false,"boundedOutput":false,"descendantCleanup":false,"startupReadiness":true}}
        ],
        "credential":{"availability":"available","status":"resolved","source":"environment","refreshSerialization":"inactive"},
        "sandbox":{"availability":"available","status":"active","fallback":"notApplicable"},
        "resources":[{"status":"loaded"}],
        "disclosure":{"rawContentPossible":true,"surfaces":["session","log","trace","toolOutput","extensionData"],"trace":{"status":"disabled"}}
    });
    let projection_json =
        serde_json::to_string(&projection).map_err(|_| Spec030ReleaseArtifactError::Io)?;
    let api_json = serde_json::to_string(&serde_json::json!({
        "schema1":{"status":200,"body":projection},
        "schema2":{"status":400,"body":{"error":{"type":"invalid_request_error"}}}
    }))
    .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    for (path, content) in [
        ("surface/cli.json", projection_json.as_str()),
        (
            "surface/cli.txt",
            "Trusted runtime: active\nprofile: status=active\ncredential: availability=available status=resolved source=environment\nsandbox: availability=available status=active fallback=notApplicable\ndisclosure: rawContentPossible=true surfaces=session,log,trace,toolOutput,extensionData\ntrace: status=disabled",
        ),
        (
            "surface/tui-no-session.txt",
            "Trusted runtime: active\ncredential: availability=unavailable status=unavailable source=unavailable\nsandbox: availability=unavailable status=unknown fallback=unknown\nstatus: no sessions",
        ),
        (
            "surface/tui-runtime.txt",
            "Trusted runtime: active\nprofile: status=active\ncredential: availability=available status=resolved source=environment\nsandbox: availability=available status=active fallback=notApplicable\ndisclosure: rawContentPossible=true surfaces=session,log,trace,toolOutput,extensionData\ntrace: status=disabled\nprocess: adapter=bash support=supported controlScope=controlledChild",
        ),
        ("surface/api.json", api_json.as_str()),
    ] {
        writer
            .write_new(path, content.as_bytes())
            .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    }
    Ok(())
}
