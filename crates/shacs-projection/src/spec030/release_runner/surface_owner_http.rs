use super::model::{Spec030ReleaseArtifactError, Spec030ReleaseRunnerConfig};
use super::surface_runner::CONTROLLED_EXEC_CALL_ID;
use crate::{
    CredentialSource, CredentialStatus, DataSurface, HookRuntimeStatus, ProcessAdapterKind,
    ProcessAdapterSupport, ProcessTerminalOutcome, ResourceTrust, SandboxFallback,
    SandboxFilesystemPolicy, SandboxNetworkPolicy, SandboxStatus, Spec030RuntimeProjection,
    TraceStatus, WorkspaceTrust,
};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub(super) fn status(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<u16, Spec030ReleaseArtifactError> {
    request(port, method, path, body, timeout).map(|response| response.status)
}

pub(super) fn exercise(
    port: u16,
    config: &Spec030ReleaseRunnerConfig,
    timeout: Duration,
) -> Result<(), Spec030ReleaseArtifactError> {
    let body =
        br#"{"model":"gpt-4o","messages":[{"role":"user","content":"exercise trusted runtime"}]}"#;
    if request(port, "POST", "/v1/chat/completions", Some(body), timeout)?.status != 200 {
        return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    }
    if !has_exec_receipt(config)? {
        return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    }
    let response = request(
        port,
        "GET",
        "/v1/trusted-runtime?schema_version=1",
        None,
        timeout,
    )?;
    let projection = std::str::from_utf8(&response.body)
        .ok()
        .and_then(|body| Spec030RuntimeProjection::parse_json(body).ok())
        .ok_or(Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    if response.status != 200 || !has_expected_facts(&projection) {
        return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    }
    Ok(())
}

fn has_exec_receipt(
    config: &Spec030ReleaseRunnerConfig,
) -> Result<bool, Spec030ReleaseArtifactError> {
    let workspace = config.evidence_root.join("surface/workspace");
    let expected_cwd = workspace.display().to_string();
    let sessions = std::fs::read_dir(workspace.join("sessions"))
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    for entry in sessions {
        let path = entry
            .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?
            .path();
        let content = std::fs::read_to_string(path)
            .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
        if content.lines().any(|line| {
            serde_json::from_str::<SessionToolReceipt>(line)
                .ok()
                .is_some_and(|receipt| {
                    receipt.role.as_deref() == Some("tool")
                        && receipt.tool_call_id.as_deref() == Some(CONTROLLED_EXEC_CALL_ID)
                        && receipt.content.as_deref().is_some_and(|content| {
                            content.contains(&expected_cwd) && content.contains("Exit code: 0")
                        })
                })
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(serde::Deserialize)]
struct SessionToolReceipt {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

fn has_expected_facts(projection: &Spec030RuntimeProjection) -> bool {
    let profile = projection.profile();
    let bash = projection
        .process_adapters()
        .iter()
        .find(|adapter| adapter.adapter == ProcessAdapterKind::Bash);
    let sandbox = projection.sandbox();
    let credential = projection.credential();
    profile.workspace_trust == WorkspaceTrust::UserAsserted
        && profile.resource_trust == ResourceTrust::ExplicitOrTrustedWorkspace
        && projection.hooks().status == HookRuntimeStatus::Active
        && projection.hooks().registered_handlers > 0
        && bash.is_some_and(|adapter| {
            adapter.support == ProcessAdapterSupport::Supported
                && adapter.capabilities.timeout
                && adapter.capabilities.abort
                && adapter.capabilities.cwd
                && adapter.capabilities.bounded_output
                && adapter.capabilities.descendant_cleanup
                && adapter.recent_outcomes.iter().any(|outcome| {
                    outcome.outcome == ProcessTerminalOutcome::Succeeded
                        && !outcome.output_truncated
                        && outcome.duration_ms.is_some()
                })
        })
        && sandbox.status == SandboxStatus::Active
        && sandbox.fallback == SandboxFallback::NotApplicable
        && sandbox.applied_adapters.contains(&ProcessAdapterKind::Bash)
        && sandbox.filesystem_policy == SandboxFilesystemPolicy::Applied
        && sandbox.network_policy == SandboxNetworkPolicy::Applied
        && credential.status == CredentialStatus::Resolved
        && credential.source == Some(CredentialSource::Environment)
        && !projection.resources().is_empty()
        && projection
            .disclosure()
            .surfaces
            .contains(&DataSurface::Trace)
        && projection.disclosure().trace.status == TraceStatus::Enabled
}

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn request(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<HttpResponse, Spec030ReleaseArtifactError> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    let payload = body.unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.write_all(payload))
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    let status = std::str::from_utf8(&response[..header_end])
        .ok()
        .and_then(|headers| headers.lines().next())
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
    Ok(HttpResponse {
        status,
        body: response[header_end + 4..].to_vec(),
    })
}
