use super::command_runner::{execute, Invocation};
use super::model::{
    Spec030ReleaseArtifactError, Spec030ReleaseRunnerConfig, Spec030SurfaceOwnerEvidence,
};
use super::semantic::{parse_spec030_surface_assertions, Spec030SurfaceAssertions};
use crate::release_evidence::EvidenceWriter;
use crate::{Spec031ReleaseCommandRecord, Spec031ReleaseGateKind};

pub(super) const OWNER_CREDENTIAL_ENV: &str = "SPEC030_OWNER_API_KEY";
pub(super) const OWNER_CREDENTIAL_VALUE: &str = "spec030-owner-fixture-credential";
pub(super) const CONTROLLED_EXEC_CALL_ID: &str = "spec030-controlled-bwrap-v1";

pub(super) fn collect_surfaces(
    config: &Spec030ReleaseRunnerConfig,
    writer: &EvidenceWriter,
) -> Result<
    (
        Vec<Spec031ReleaseCommandRecord>,
        Spec030SurfaceAssertions,
        Spec030SurfaceOwnerEvidence,
    ),
    Spec030ReleaseArtifactError,
> {
    let port = super::surface_owner::ephemeral_port()?;
    prepare(config, writer, port)?;
    let mut owner = super::surface_owner::ProductionOwner::start(config, port)?;
    owner.wait_until_ready(config.command_timeout)?;
    let tui_no_session = capture_tui_no_session(config, writer)?;
    owner.exercise_runtime(config, config.command_timeout)?;
    let cli_json = execute(config, writer, cli(config, "surface-cli-json", "json"))?;
    copy(config, writer, &cli_json.stdout_path, "surface/cli.json")?;
    let cli_human = execute(config, writer, cli(config, "surface-cli-human", "human"))?;
    copy(config, writer, &cli_human.stdout_path, "surface/cli.txt")?;
    let tui_runtime = capture_tui_runtime(config, writer)?;
    let api = execute(config, writer, api(config, port))?;
    copy(config, writer, &api.stdout_path, "surface/api.json")?;
    let assertions = parse_spec030_surface_assertions(&config.evidence_root.join("surface"))?;
    let owner = owner.stop(config, writer)?;
    Ok((
        vec![tui_no_session, cli_json, cli_human, tui_runtime, api],
        assertions,
        owner,
    ))
}

pub(super) fn capture_tui_no_session(
    config: &Spec030ReleaseRunnerConfig,
    writer: &EvidenceWriter,
) -> Result<Spec031ReleaseCommandRecord, Spec030ReleaseArtifactError> {
    let record = execute(config, writer, tui(config, "surface-tui-no-session"))?;
    copy(
        config,
        writer,
        &record.stdout_path,
        "surface/tui-no-session.txt",
    )?;
    Ok(record)
}

pub(super) fn capture_tui_runtime(
    config: &Spec030ReleaseRunnerConfig,
    writer: &EvidenceWriter,
) -> Result<Spec031ReleaseCommandRecord, Spec030ReleaseArtifactError> {
    let record = execute(config, writer, tui(config, "surface-tui-runtime"))?;
    copy(
        config,
        writer,
        &record.stdout_path,
        "surface/tui-runtime.txt",
    )?;
    Ok(record)
}

fn cli(config: &Spec030ReleaseRunnerConfig, id: &'static str, format: &str) -> Invocation {
    Invocation {
        id,
        package: Some("shacs-cli"),
        gate: Spec031ReleaseGateKind::SurfaceSmoke,
        cwd: config.repo_root.clone(),
        argv: strings(&[
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            "crates/Cargo.toml",
            "-p",
            "shacs-cli",
            "--bin",
            "shacs-bot",
            "--",
            "runtime",
            "trusted-runtime",
            "--config",
        ])
        .into_iter()
        .chain([surface(config, "config.json"), "--workspace".to_owned()])
        .chain([
            surface(config, "workspace"),
            "--format".to_owned(),
            format.to_owned(),
        ])
        .collect(),
        filter: None,
    }
}

fn tui(config: &Spec030ReleaseRunnerConfig, id: &'static str) -> Invocation {
    Invocation {
        id,
        package: Some("shacs-tui"),
        gate: Spec031ReleaseGateKind::SurfaceSmoke,
        cwd: config.repo_root.clone(),
        argv: strings(&[
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            "crates/Cargo.toml",
            "-p",
            "shacs-tui",
            "--bin",
            "shacs-tui",
            "--",
            "--config",
        ])
        .into_iter()
        .chain([surface(config, "config.json"), "--workspace".to_owned()])
        .chain([surface(config, "workspace"), "--once".to_owned()])
        .collect(),
        filter: None,
    }
}

fn api(config: &Spec030ReleaseRunnerConfig, port: u16) -> Invocation {
    Invocation {
        id: "surface-api-schema",
        package: Some("shacs-api"),
        gate: Spec031ReleaseGateKind::SurfaceSmoke,
        cwd: config.repo_root.clone(),
        argv: strings(&[
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            "crates/Cargo.toml",
            "-p",
            "shacs-api",
            "--bin",
            "spec030-api-probe",
            "--",
            "--address",
        ])
        .into_iter()
        .chain([format!("127.0.0.1:{port}")])
        .collect(),
        filter: None,
    }
}

pub(super) fn prepare(
    config: &Spec030ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    port: u16,
) -> Result<(), Spec030ReleaseArtifactError> {
    writer
        .create_dir_all("surface/workspace")
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    let workspace = surface(config, "workspace");
    let config_json = serde_json::to_vec_pretty(&serde_json::json!({
        "agents": {"defaults": {
            "provider": "openai",
            "model": "gpt-4o",
            "maxToolIterations": 2,
            "workspace": workspace
        }},
        "api": {"host": "127.0.0.1", "port": port, "timeout": 10.0},
        "providers": {"openai": {"credentialSource": {
            "schemaVersion": 1,
            "environment": OWNER_CREDENTIAL_ENV,
            "localAuth": false
        }}},
        "permissions": {
            "mode": "auto",
            "autoApproval": {
                "enabled": true,
                "requireDockerContainmentForExec": false,
                "allowWorkspaceEdits": true,
                "allowProcExecVerification": true
            }
        },
        "plugins": {
            "enabled": ["release-owner"],
            "trustedWorkspaces": [workspace]
        },
        "trustedRuntime": {"trace": {
            "enabled": true,
            "destination": "localOnly",
            "path": surface(config, "trace.jsonl")
        }},
        "tools": {"exec": {"sandbox": "bwrap", "sandboxPolicy": {
            "fallback": "sandboxRequired",
            "network": "deny",
            "allowWrite": [surface(config, "workspace")]
        }}}
    }))
    .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new("surface/config.json", &config_json)
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new(
            "surface/owner-address",
            format!("127.0.0.1:{port}").as_bytes(),
        )
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    let provider_responses = serde_json::to_vec(&serde_json::json!([
        {"finish_reason":"tool_calls","tool_calls":[{
            "id":CONTROLLED_EXEC_CALL_ID,
            "name":"exec",
            "arguments":{
                "command":"pwd",
                "working_dir":workspace,
                "timeout":5
            }
        }]},
        {"content":"{\"verdict\":\"allow_candidate\",\"confidence\":\"high\",\"scope_match\":\"requested\",\"risk_summary\":\"release owner pwd verification\",\"evidence_refs\":[\"spec030:release-owner\"],\"evaluator_ref\":\"spec030:release-owner\"}","finish_reason":"stop"},
        {"content":"done","finish_reason":"stop"}
    ]))
    .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new("surface/provider-responses.json", &provider_responses)
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .create_dir_all("surface/plugins/release-owner")
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new(
            "surface/plugins/release-owner/plugin.json",
            br#"{"schemaVersion":1,"name":"release-owner","version":"0.1.0","surfaces":{"hooks":["tool:before"]},"entrypoints":{"trustedHooks":{"tool:before":"hook.js"}}}"#,
        )
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new(
            "surface/plugins/release-owner/hook.js",
            b"function toolBefore() { return {allow: true}; }",
        )
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    let session_root = config.evidence_root.join("surface/workspace/sessions");
    if session_root.exists() {
        return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    }
    Ok(())
}

fn copy(
    config: &Spec030ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    source: &str,
    target: &str,
) -> Result<(), Spec030ReleaseArtifactError> {
    let bytes = std::fs::read(config.evidence_root.join(source))
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new(target, &bytes)
        .map_err(|_| Spec030ReleaseArtifactError::Io)
}

fn surface(config: &Spec030ReleaseRunnerConfig, name: &str) -> String {
    config
        .evidence_root
        .join("surface")
        .join(name)
        .display()
        .to_string()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
