use super::owner_fixture;
use super::surface_process::{
    cargo_output, http_get, output_text, path_text, semantic_diff, start_api, write_cli_fixture,
    CargoCommand,
};
use serde::Serialize;
use serde_json::Value;
use shacs_projection::{Spec035MediaProjection, Spec035MediaState};
use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;

const STATES: &[(&str, Spec035MediaState)] = &[
    ("included", Spec035MediaState::Included),
    ("unsupported", Spec035MediaState::Unsupported),
    ("extraction_failed", Spec035MediaState::ExtractionFailed),
    ("analyzer_missing", Spec035MediaState::AnalyzerMissing),
    ("truncated", Spec035MediaState::Truncated),
    ("unavailable", Spec035MediaState::Unavailable),
];

#[derive(Debug, Serialize)]
pub struct SurfaceStateReport {
    pub cli_diff: Vec<String>,
    pub http_diff: Vec<String>,
    pub websocket_diff: Vec<String>,
    pub channel_diff: Vec<String>,
    pub tui_diff: Vec<String>,
}

impl SurfaceStateReport {
    pub fn all_empty(&self) -> bool {
        self.cli_diff.is_empty()
            && self.http_diff.is_empty()
            && self.websocket_diff.is_empty()
            && self.channel_diff.is_empty()
            && self.tui_diff.is_empty()
    }
}

#[derive(Debug, Serialize)]
pub struct SurfaceReport {
    pub semantic_parity: bool,
    pub http_websocket_raw_equal: bool,
    pub cli_diff: Vec<String>,
    pub http_diff: Vec<String>,
    pub websocket_diff: Vec<String>,
    pub channel_diff: Vec<String>,
    pub tui_diff: Vec<String>,
    pub states: BTreeMap<&'static str, SurfaceStateReport>,
    #[serde(skip)]
    pub raw_outputs: Vec<(&'static str, String)>,
}

pub fn run(repo: &Path, root: &Path) -> Result<SurfaceReport, Box<dyn Error>> {
    let mut states = BTreeMap::new();
    let mut raw_outputs = Vec::new();
    let mut raw_equal = true;
    for &(name, state) in STATES {
        let state_root = root.join(format!("surface-{name}"));
        std::fs::create_dir(&state_root)?;
        let canonical = owner_fixture::spec035_projection_for_state(state)?;
        let observed = run_state(repo, &state_root, &canonical)?;
        raw_equal &= observed.http_websocket_raw_equal;
        raw_outputs.extend(observed.raw_outputs);
        states.insert(name, observed.report);
    }
    let semantic_parity = states.values().all(SurfaceStateReport::all_empty);
    Ok(SurfaceReport {
        semantic_parity,
        http_websocket_raw_equal: raw_equal,
        cli_diff: collect_surface_diff(&states, |state| &state.cli_diff),
        http_diff: collect_surface_diff(&states, |state| &state.http_diff),
        websocket_diff: collect_surface_diff(&states, |state| &state.websocket_diff),
        channel_diff: collect_surface_diff(&states, |state| &state.channel_diff),
        tui_diff: collect_surface_diff(&states, |state| &state.tui_diff),
        states,
        raw_outputs,
    })
}

struct StateObservation {
    report: SurfaceStateReport,
    http_websocket_raw_equal: bool,
    raw_outputs: Vec<(&'static str, String)>,
}

fn run_state(
    repo: &Path,
    root: &Path,
    canonical: &Spec035MediaProjection,
) -> Result<StateObservation, Box<dyn Error>> {
    let projection_path = root.join("canonical-media.json");
    let canonical_value = serde_json::to_value(canonical)?;
    std::fs::write(&projection_path, serde_json::to_vec(canonical)?)?;
    let config_path = write_cli_fixture(root, &projection_path)?;
    let cli_text = output_text(cargo_output(CargoCommand {
        repo,
        package: "shacs-cli",
        example: None,
        arguments: &[
            "--config".to_owned(),
            path_text(&config_path),
            "runtime".to_owned(),
            "inspect".to_owned(),
        ],
    })?)?;
    let cli_value: Value = serde_json::from_str(
        cli_text
            .lines()
            .find_map(|line| line.strip_prefix("Spec035 media JSON: "))
            .ok_or("CLI emitted no canonical media JSON")?,
    )?;

    let mut api = start_api(repo, root)?;
    let http_raw = http_get(api.address)?;
    let http_body = http_raw
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .ok_or("HTTP fixture returned no body")?;
    let http_value: Value = serde_json::from_str(http_body)?;
    let websocket_text = output_text(cargo_output(CargoCommand {
        repo,
        package: "shacs-api",
        example: Some("spec034_media_websocket_probe"),
        arguments: &[format!("ws://{}/ws", api.address)],
    })?)?;
    let websocket_value: Value = serde_json::from_str(websocket_text.trim_end())?;
    api.child.kill()?;
    api.child.wait()?;

    let channel_text = output_text(cargo_output(CargoCommand {
        repo,
        package: "shacs-channels",
        example: Some("spec034_surface_projection_probe"),
        arguments: &[path_text(&projection_path)],
    })?)?;
    let channel_value: Value = serde_json::from_str(channel_text.trim_end())?;
    let tui_text = output_text(cargo_output(CargoCommand {
        repo,
        package: "shacs-tui",
        example: None,
        arguments: &[
            "--config".to_owned(),
            path_text(&config_path),
            "--workspace".to_owned(),
            path_text(&root.join("workspace")),
            "--session".to_owned(),
            "cli:direct".to_owned(),
            "--once".to_owned(),
        ],
    })?)?;
    let expected_tui_state = format!(
        "media: state={}",
        canonical_value["state"]
            .as_str()
            .ok_or("canonical state missing")?
    );
    Ok(StateObservation {
        report: SurfaceStateReport {
            cli_diff: semantic_diff(&canonical_value, &cli_value),
            http_diff: semantic_diff(&canonical_value, &http_value),
            websocket_diff: semantic_diff(&canonical_value, &websocket_value),
            channel_diff: semantic_diff(&canonical_value, &channel_value),
            tui_diff: (!tui_text.contains(&expected_tui_state))
                .then(|| "$.state".to_owned())
                .into_iter()
                .collect(),
        },
        http_websocket_raw_equal: http_body.as_bytes() == websocket_text.as_bytes(),
        raw_outputs: vec![
            ("cli", cli_text),
            ("http", http_raw),
            ("websocket", websocket_text),
            ("channel", channel_text),
            ("tui", tui_text),
        ],
    })
}

fn collect_surface_diff(
    states: &BTreeMap<&'static str, SurfaceStateReport>,
    select: impl Fn(&SurfaceStateReport) -> &[String],
) -> Vec<String> {
    states
        .iter()
        .flat_map(|(state, report)| {
            select(report)
                .iter()
                .map(move |path| format!("{state}:{path}"))
        })
        .collect()
}
