use shacs_api::ChatCompletionAdapter;
use shacs_cli::AgentLoopChatCompletionAdapter;
use shacs_config::{Config, ConfigBundle, ConfigContext, McpServerConfig};
use shacs_core::runtime::PermissionMode;
use shacs_projection::{ProcessAdapterKind, ProcessAdapterSupport, ProcessTerminalOutcome};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

#[test]
fn production_startup_projects_configured_and_failed_mcp_transport_outcomes(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let data_dir = root.path().join("data");
    fs::create_dir_all(&workspace)?;
    let server = root.path().join("mcp_server.py");
    fs::write(&server, MCP_SERVER)?;
    let mut config = Config::default();
    config.agents.defaults.provider = "custom".to_owned();
    config.agents.defaults.model = "test-model".to_owned();
    config.agents.defaults.workspace = workspace.to_string_lossy().into_owned();
    config.permissions.mode = PermissionMode::BypassPermissions;
    let mut connected = mcp_config("python3", vec!["*".to_owned()]);
    connected.args = vec![server.to_string_lossy().into_owned()];
    let mut failed = mcp_config("unused", vec!["*".to_owned()]);
    failed.r#type = Some("unsupported".to_owned());
    config.tools.mcp_servers = BTreeMap::from([
        ("configured".to_owned(), mcp_config("unused", Vec::new())),
        ("connected".to_owned(), connected),
        ("failed".to_owned(), failed),
    ]);
    let adapter = AgentLoopChatCompletionAdapter::from_bundle(
        ConfigBundle {
            config,
            context: ConfigContext {
                config_path: data_dir.join("config.json"),
                data_dir,
                workspace,
            },
            migrations: Vec::new(),
        },
        false,
    )?;

    let projection = adapter.trusted_runtime_projection();

    let mcp = projection
        .process_adapters()
        .iter()
        .find(|adapter| adapter.adapter == ProcessAdapterKind::Mcp)
        .ok_or("MCP adapter missing")?;
    assert_eq!(mcp.support, ProcessAdapterSupport::Supported);
    assert_eq!(
        mcp.recent_outcomes
            .iter()
            .map(|outcome| outcome.outcome)
            .collect::<Vec<_>>(),
        [
            ProcessTerminalOutcome::Unsupported,
            ProcessTerminalOutcome::Failed,
            ProcessTerminalOutcome::Failed,
        ]
    );
    Ok(())
}

fn mcp_config(command: &str, enabled_tools: Vec<String>) -> McpServerConfig {
    McpServerConfig {
        r#type: Some("stdio".to_owned()),
        command: command.to_owned(),
        args: Vec::new(),
        env: BTreeMap::new(),
        url: String::new(),
        headers: BTreeMap::new(),
        tool_timeout: 2,
        enabled_tools,
    }
}

const MCP_SERVER: &str = r#"import json, sys
while True:
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            raise SystemExit(0)
        if line in (b'\r\n', b'\n'):
            break
        if line.lower().startswith(b'content-length:'):
            length = int(line.split(b':', 1)[1].strip())
    request = json.loads(sys.stdin.buffer.read(length))
    if 'id' not in request:
        continue
    method = request['method']
    if method == 'initialize':
        result = {'protocolVersion': '2024-11-05', 'capabilities': {}, 'serverInfo': {'name': 'fixture', 'version': '1'}}
    elif method == 'tools/list':
        result = {'tools': [{'name': 'ping', 'inputSchema': {'type': 'object', 'properties': {}}}]}
    elif method == 'resources/list':
        result = {'resources': []}
    else:
        result = {'prompts': []}
    body = json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': result}).encode()
    sys.stdout.buffer.write(f'Content-Length: {len(body)}\r\n\r\n'.encode() + body)
    sys.stdout.buffer.flush()
"#;
