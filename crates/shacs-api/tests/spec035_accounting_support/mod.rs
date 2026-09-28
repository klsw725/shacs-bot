use std::io::Write;
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;

pub fn capture_payload(
    socket: &WebSocketStream<TcpStream>,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(root) = std::env::var_os("SPEC035_ACCOUNTING_ARTIFACT_DIR") else {
        return Ok(());
    };
    std::fs::create_dir_all(&root)?;
    let name = format!(
        "wire-{}-{}.ndjson",
        socket.get_ref().peer_addr()?.port(),
        socket.get_ref().local_addr()?.port()
    );
    let mut output = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::path::PathBuf::from(root).join(name))?;
    output.write_all(text.as_bytes())?;
    output.write_all(b"\n")?;
    Ok(())
}
