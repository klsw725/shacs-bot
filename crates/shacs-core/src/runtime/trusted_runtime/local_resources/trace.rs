use super::super::TraceDisclosureUpdate;
use shacs_config::{ConfigBundle, TrustedTraceDestination};
use shacs_projection::{
    DataSurface, TraceDestination, TraceDisclosureProjection, TracePreviewProjection, TraceStatus,
};
use std::path::PathBuf;

pub fn disclosure(bundle: &ConfigBundle) -> TraceDisclosureUpdate {
    let config = &bundle.config.trusted_runtime.trace;
    if !config.enabled {
        return TraceDisclosureUpdate {
            raw_content_possible: true,
            surfaces: vec![
                DataSurface::Session,
                DataSurface::Log,
                DataSurface::ToolOutput,
                DataSurface::ExtensionData,
            ],
            trace: TraceDisclosureProjection {
                status: TraceStatus::Disabled,
                preview: None,
            },
        };
    }
    let path = config.path.as_ref().map(|path| {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            bundle.context.data_dir.join(path)
        }
    });
    let bytes = path
        .as_ref()
        .and_then(|path| std::fs::read(path).ok())
        .unwrap_or_default();
    let destination = match config.destination {
        TrustedTraceDestination::LocalOnly => TraceDestination::LocalOnly,
        TrustedTraceDestination::ConfiguredRemote => TraceDestination::ConfiguredRemote,
    };
    let exporter = config
        .exporter
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    let endpoint_summary = config
        .endpoint_summary
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    let status = match destination {
        TraceDestination::LocalOnly => TraceStatus::Enabled,
        TraceDestination::ConfiguredRemote if exporter.is_some() && endpoint_summary.is_some() => {
            TraceStatus::Enabled
        }
        TraceDestination::ConfiguredRemote => TraceStatus::Preview,
    };
    TraceDisclosureUpdate {
        raw_content_possible: true,
        surfaces: vec![
            DataSurface::Session,
            DataSurface::Log,
            DataSurface::Trace,
            DataSurface::ToolOutput,
            DataSurface::ExtensionData,
        ],
        trace: TraceDisclosureProjection {
            status,
            preview: Some(TracePreviewProjection {
                record_count: u64::try_from(String::from_utf8_lossy(&bytes).lines().count())
                    .unwrap_or(u64::MAX),
                approximate_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                destination,
                exporter,
                endpoint_summary,
            }),
        },
    }
}
