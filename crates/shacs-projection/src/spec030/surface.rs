use super::*;
use std::fmt::Debug;

pub trait Spec030ProjectionProvider {
    fn projection(&self) -> Spec030RuntimeProjection;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableSpec030ProjectionProvider;

impl Spec030ProjectionProvider for UnavailableSpec030ProjectionProvider {
    fn projection(&self) -> Spec030RuntimeProjection {
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing)
    }
}

pub fn serialize_spec030_runtime(
    projection: &Spec030RuntimeProjection,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(projection)
}

pub fn render_spec030_runtime(projection: &Spec030RuntimeProjection) -> String {
    let mut lines = vec![
        format!(
            "Trusted runtime: availability={} status={}",
            wire_label(projection.availability()),
            wire_label(projection.status())
        ),
        "Warning: trusted runtime execution uses Current OS user authority; lifecycle boundaries are not security isolation.".to_owned(),
        format!(
            "profile: availability={} authority={} workspaceTrust={}",
            wire_label(projection.profile().availability),
            wire_label(projection.profile().execution_authority),
            wire_label(projection.profile().workspace_trust),
        ),
        format!(
            "profile policy: remediation={} containment={} optionalSandbox={}",
            projection
                .profile()
                .workspace_trust_remediation
                .map_or_else(|| "none".to_owned(), wire_label),
            wire_label(projection.profile().default_containment),
            wire_label(projection.profile().optional_sandbox),
        ),
    ];
    if projection.availability() == Spec030Availability::Unavailable {
        lines.push(format!(
            "Unavailable: reason={}",
            projection
                .unavailable_reason()
                .map_or_else(|| "ownerUnavailable".to_owned(), wire_label)
        ));
    }
    if projection.lifecycle_boundaries().is_empty() {
        lines.push("lifecycle: Unavailable (owner facts missing)".to_owned());
    }
    lines.extend(projection.lifecycle_boundaries().iter().map(|boundary| {
        format!(
            "lifecycle: kind={} status={} isolation={}",
            wire_label(boundary.kind),
            wire_label(boundary.status),
            wire_label(boundary.isolation)
        )
    }));
    lines.push(format!(
        "{} availability={} status={} registeredHandlers={}",
        section_label("hooks", projection.hooks().availability),
        wire_label(projection.hooks().availability),
        wire_label(projection.hooks().status),
        projection.hooks().registered_handlers
    ));
    lines.extend(projection.hooks().diagnostics.iter().map(|diagnostic| {
        format!(
            "hook diagnostic: ref={} kind={} behavior={}",
            diagnostic.hook_ref,
            wire_label(diagnostic.kind),
            wire_label(diagnostic.behavior)
        )
    }));
    lines.extend(projection.hooks().recent_denials.iter().map(|denial| {
        format!(
            "hook denial: ref={} call={} reason={}",
            denial.hook_ref,
            denial.call_ref,
            wire_label(denial.reason)
        )
    }));
    if projection.process_adapters().is_empty() {
        lines.push("process: Unavailable (owner facts missing)".to_owned());
    }
    lines.extend(projection.process_adapters().iter().map(|adapter| {
        format!(
            "process: adapter={} support={} controlScope={} reason={} timeout={} abort={} cwd={} env={} boundedOutput={} descendantCleanup={}",
            wire_label(adapter.adapter),
            wire_label(adapter.support),
            wire_label(adapter.control_scope),
            wire_label(adapter.reason),
            adapter.capabilities.timeout,
            adapter.capabilities.abort,
            adapter.capabilities.cwd,
            adapter.capabilities.env,
            adapter.capabilities.bounded_output,
            adapter.capabilities.descendant_cleanup,
        )
    }));
    lines.push(format!(
        "{} availability={} status={} source={}",
        section_label("credential", projection.credential().availability),
        wire_label(projection.credential().availability),
        wire_label(projection.credential().status),
        projection
            .credential()
            .source
            .map_or_else(|| "none".to_owned(), wire_label),
    ));
    lines.push(format!(
        "credential detail: fingerprint={} refreshSerialization={}",
        wire_label(projection.credential().fingerprint),
        wire_label(projection.credential().refresh_serialization)
    ));
    lines.push(format!(
        "{} availability={} status={} fallback={}",
        section_label("sandbox", projection.sandbox().availability),
        wire_label(projection.sandbox().availability),
        wire_label(projection.sandbox().status),
        wire_label(projection.sandbox().fallback),
    ));
    lines.push(format!(
        "sandbox policy: adapters={} filesystem={} network={}",
        projection
            .sandbox()
            .applied_adapters
            .iter()
            .copied()
            .map(wire_label)
            .collect::<Vec<_>>()
            .join(","),
        wire_label(projection.sandbox().filesystem_policy),
        wire_label(projection.sandbox().network_policy)
    ));
    if projection.resources().is_empty() {
        lines.push("resource: Unavailable (owner facts missing)".to_owned());
    }
    lines.extend(projection.resources().iter().map(|resource| {
        format!(
            "resource: ref={} kind={} source={} precedence={} path={} collision={} load={} activation={} trustedCode={}",
            resource.resource_ref,
            wire_label(resource.kind),
            wire_label(resource.source),
            wire_label(resource.precedence),
            resource.canonical_path,
            wire_label(resource.collision),
            wire_label(resource.load_status),
            wire_label(resource.activation),
            wire_label(resource.trusted_code_disclosure)
        )
    }));
    lines.extend(projection.resources().iter().map(|resource| {
        format!(
            "resource digest: ref={} sha256={}",
            resource.resource_ref,
            resource.content_sha256.as_deref().unwrap_or("unavailable")
        )
    }));
    lines.extend(projection.resources().iter().flat_map(|resource| {
        resource.diagnostics.iter().map(|diagnostic| {
            format!(
                "resource diagnostic: ref={} code={} path={} reason={}",
                resource.resource_ref,
                diagnostic.code,
                diagnostic.path.as_deref().unwrap_or("none"),
                diagnostic.reason
            )
        })
    }));
    lines.push(format!(
        "disclosure: rawContentPossible={} surfaces={}",
        projection.disclosure().raw_content_possible,
        projection
            .disclosure()
            .surfaces
            .iter()
            .copied()
            .map(wire_label)
            .collect::<Vec<_>>()
            .join(",")
    ));
    lines.push(format!(
        "trace: status={} preview={}",
        wire_label(projection.disclosure().trace.status),
        projection.disclosure().trace.preview.as_ref().map_or_else(
            || "none".to_owned(),
            |preview| format!(
                "records:{} bytes:{} destination:{} exporter:{} endpoint:{}",
                preview.record_count,
                preview.approximate_bytes,
                wire_label(preview.destination),
                preview.exporter.as_deref().unwrap_or("none"),
                preview.endpoint_summary.as_deref().unwrap_or("none")
            )
        )
    ));
    lines.join("\n")
}

fn wire_label(value: impl Debug) -> String {
    let mut label = format!("{value:?}");
    if let Some(first) = label.get_mut(0..1) {
        first.make_ascii_lowercase();
    }
    label
}

fn section_label(name: &str, availability: Spec030Availability) -> String {
    match availability {
        Spec030Availability::Unavailable => format!("{name}: Unavailable"),
        Spec030Availability::Available
        | Spec030Availability::Degraded
        | Spec030Availability::Unknown => format!("{name}:"),
    }
}
