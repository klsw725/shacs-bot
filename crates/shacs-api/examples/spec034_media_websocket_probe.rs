use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::error::Error;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let url = std::env::args()
        .nth(1)
        .ok_or("usage: spec034_media_websocket_probe <ws-url>")?;
    let (mut websocket, _) = connect_async(url).await?;
    websocket
        .send(Message::Text(
            json!({"type": "media_projection"}).to_string().into(),
        ))
        .await?;
    let frame = websocket.next().await.ok_or("missing media frame")??;
    let projection: Value = serde_json::from_str(&frame.into_text()?)?;
    println!("{}", serde_json::to_string(&projection)?);
    websocket.close(None).await?;
    Ok(())
}
