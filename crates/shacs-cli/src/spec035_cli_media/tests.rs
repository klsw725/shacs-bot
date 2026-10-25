use crate::{
    format_runtime_inspect, runtime_inspect, ExternalTransportChannelAdapter,
    ExternalTransportRunner, ExternalTransportRuntimeContext, ExternalTransportSpec,
    RuntimeInspectOptions, TelegramRuntimeConfig,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use shacs_channels::{ChannelAdapter, OutboundMessage};
use shacs_core::runtime::MessageBus;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::Duration;

const LEFTOVER_RECORD: &str = r#"{"schema_version":1,"kind":"media_capability","state":"unavailable","reason":{"code":"unavailable","safe_summary":"media evidence unavailable"},"lineage":{"artifact_ref":"spec034://media/artifact/7b1dfd5bb5d3258aa9d9c556171a55c5b2a8dc847bb042faa489277a53f627ee"},"freshness":"unavailable","disclosure":{"status":"unavailable"},"owner_facts":{"unavailable_reasons":["missing_analyzer_owner_ref"]}}"#;
const CORRUPT_RECORD: &str =
    r#"{"schema_version": 1, "kind": "media_capability", "state": "included""#;

#[test]
fn runtime_inspect_and_diagnostics_ignore_leftover_media_records() -> Result<(), Box<dyn Error>> {
    for record in [LEFTOVER_RECORD, CORRUPT_RECORD] {
        // Given
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = shacs_config::Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().into_owned();
        crate::save_config_to_path(&config, &config_path)?;
        write_leftovers(root.path(), &workspace, record)?;
        let before = leftover_digests(root.path(), &workspace)?;

        // When
        let report = runtime_inspect(RuntimeInspectOptions {
            config_path: Some(config_path),
            workspace_override: None,
        })?;
        let diagnostics = crate::diagnostics_snapshot_from_runtime_inspect(&report);
        let output = format_runtime_inspect(report);

        // Then
        assert_eq!(diagnostics.runtime["spec035_media_projections"], json!([]));
        assert!(output
            .contains("Spec035 media projections: unavailable (canonical records unavailable)"));
        assert!(!output.contains("Spec035 media JSON"));
        assert_eq!(leftover_digests(root.path(), &workspace)?, before);
    }
    Ok(())
}

#[test]
fn external_channel_send_ignores_leftover_media_records() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    write_leftovers(root.path(), &workspace, CORRUPT_RECORD)?;
    let before = leftover_digests(root.path(), &workspace)?;
    let mut context = ExternalTransportRuntimeContext::new(root.path().join("metadata"), 1);
    context.configure_durable_inbound(root.path().to_path_buf(), "owner:test".to_owned(), None);
    let (delivered_tx, delivered_rx) = mpsc::channel();
    let runner: ExternalTransportRunner = Arc::new(move |_, _, outbound, _, _| {
        if let Ok(message) = outbound.recv() {
            let _ = delivered_tx.send(message);
        }
    });
    let spec = ExternalTransportSpec::Telegram(TelegramRuntimeConfig {
        token: "telegram-token".to_owned(),
        poll_timeout_seconds: 1,
        poll_limit: 1,
    });
    let mut adapter =
        ExternalTransportChannelAdapter::new(spec, MessageBus::new(), context, runner);
    adapter.start()?;

    // When
    adapter.send(OutboundMessage::new("telegram", "chat-1", "reply"))?;
    let delivered = delivered_rx.recv_timeout(Duration::from_secs(5))?;
    adapter.stop()?;

    // Then
    assert_eq!(delivered.content, "reply");
    assert!(!delivered.metadata.contains_key("media_capability"));
    assert!(!delivered.metadata.contains_key("media_delivery_status"));
    assert_eq!(leftover_digests(root.path(), &workspace)?, before);
    Ok(())
}

fn write_leftovers(data_dir: &Path, workspace: &Path, record: &str) -> Result<(), Box<dyn Error>> {
    let projections = data_dir.join("media").join("projections");
    fs::create_dir_all(&projections)?;
    fs::write(projections.join("current.json"), record)?;
    let staging = workspace
        .join(".shacs-video-analyzer-staging")
        .join("4242-1");
    fs::create_dir_all(&staging)?;
    fs::write(staging.join("partial.bin"), b"leftover staging bytes")?;
    Ok(())
}

fn leftover_digests(
    data_dir: &Path,
    workspace: &Path,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let mut digests = BTreeMap::new();
    for root in [
        data_dir.join("media").join("projections"),
        workspace.join(".shacs-video-analyzer-staging"),
    ] {
        collect_digests(&root, &mut digests)?;
    }
    Ok(digests)
}

fn collect_digests(
    path: &Path,
    digests: &mut BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    if path.is_dir() {
        digests.insert(path.to_string_lossy().into_owned(), "dir".to_owned());
        for entry in fs::read_dir(path)? {
            collect_digests(&entry?.path(), digests)?;
        }
    } else {
        digests.insert(
            path.to_string_lossy().into_owned(),
            format!("{:x}", Sha256::digest(fs::read(path)?)),
        );
    }
    Ok(())
}
