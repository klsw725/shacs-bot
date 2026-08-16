use serde_json::Value;
use std::error::Error;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

pub(super) struct CargoCommand<'a> {
    pub repo: &'a Path,
    pub package: &'a str,
    pub example: Option<&'a str>,
    pub arguments: &'a [String],
}

pub(super) struct ApiServer {
    pub child: Child,
    pub address: SocketAddr,
}

impl Drop for ApiServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) fn write_cli_fixture(root: &Path, projection: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let workspace = root.join("workspace");
    std::fs::create_dir(&workspace)?;
    let mut sessions = shacs_session::SessionManager::new(&workspace)?;
    let mut session = shacs_session::Session::new("cli:direct");
    session.add_message("user", "inspect media", serde_json::Map::new());
    sessions.save(&session)?;
    let mut config = shacs_config::Config::default();
    config.agents.defaults.workspace = path_text(&workspace);
    let config_path = root.join("config.json");
    std::fs::write(&config_path, serde_json::to_vec_pretty(&config)?)?;
    let projection = shacs_projection::Spec035MediaProjection::parse_json(
        &std::fs::read_to_string(projection)?,
    )?;
    shacs_core::runtime::Spec035MediaProjectionStore::new(root).publish(&projection)?;
    Ok(config_path)
}

pub(super) fn start_api(repo: &Path, root: &Path) -> Result<ApiServer, Box<dyn Error>> {
    let mut child = Command::new(cargo_binary())
        .args([
            "run",
            "--quiet",
            "--manifest-path",
            &path_text(&repo.join("crates/Cargo.toml")),
            "--locked",
            "-p",
            "shacs-api",
            "--example",
            "spec034_media_api_fixture",
            "--",
            "127.0.0.1:0",
            &path_text(root),
        ])
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or("API fixture stdout unavailable")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(ApiServer {
        child,
        address: line.trim().parse()?,
    })
}

pub(super) fn cargo_output(spec: CargoCommand<'_>) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(cargo_binary());
    command.args([
        "run",
        "--quiet",
        "--manifest-path",
        &path_text(&spec.repo.join("crates/Cargo.toml")),
        "--locked",
        "-p",
        spec.package,
    ]);
    if let Some(example) = spec.example {
        command.args(["--example", example]);
    }
    command
        .arg("--")
        .args(spec.arguments)
        .current_dir(spec.repo);
    Ok(command.output()?)
}

pub(super) fn output_text(output: Output) -> Result<String, Box<dyn Error>> {
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

pub(super) fn http_get(address: SocketAddr) -> Result<String, Box<dyn Error>> {
    let mut stream = TcpStream::connect(address)?;
    write!(
        stream,
        "GET /v1/media/diagnostics HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

pub(super) fn semantic_diff(expected: &Value, actual: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    collect_diff("$", expected, actual, &mut paths);
    paths
}

fn collect_diff(path: &str, expected: &Value, actual: &Value, paths: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(left), Value::Object(right)) => {
            let keys = left
                .keys()
                .chain(right.keys())
                .collect::<std::collections::BTreeSet<_>>();
            for key in keys {
                collect_diff(
                    &format!("{path}.{key}"),
                    left.get(key).unwrap_or(&Value::Null),
                    right.get(key).unwrap_or(&Value::Null),
                    paths,
                );
            }
        }
        (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                collect_diff(&format!("{path}[{index}]"), left, right, paths);
            }
        }
        _ if expected == actual => {}
        _ => paths.push(path.to_owned()),
    }
}

pub(super) fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn cargo_binary() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
}
