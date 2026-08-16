use shacs_api::{serve_api_listener, ApiError, ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_providers::types::text_response;
use shacs_providers::LlmResponse;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;

struct FixtureAdapter {
    workspace: PathBuf,
    data_dir: PathBuf,
}

impl ChatCompletionAdapter for FixtureAdapter {
    fn configured_model(&self) -> &str {
        "fixture-model"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(text_response("unused"))
    }

    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.workspace.clone())
    }

    fn runtime_data_dir(&self) -> Option<PathBuf> {
        Some(self.data_dir.clone())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:0".to_owned());
    let data_dir = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let workspace = data_dir.join("workspace");
    let listener = TcpListener::bind(&bind).await?;
    println!("{}", listener.local_addr()?);
    serve_api_listener(
        listener,
        Arc::new(FixtureAdapter {
            workspace,
            data_dir,
        }),
        std::future::pending(),
    )
    .await?;
    Ok(())
}
