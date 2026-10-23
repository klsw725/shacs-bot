use serde_json::{json, Value};
use shacs_api::{ApiError, ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_projection::Spec035MediaProjection;
use shacs_providers::types::text_response;
use shacs_providers::LlmResponse;
use std::error::Error;
use std::path::PathBuf;

pub(crate) struct MediaAdapter {
    pub(crate) projection: Option<Spec035MediaProjection>,
}

pub(crate) struct StoreMediaAdapter {
    pub(crate) data_dir: PathBuf,
}

impl ChatCompletionAdapter for MediaAdapter {
    fn configured_model(&self) -> &str {
        "fixture-model"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(text_response("unused"))
    }

    fn media_projection(&self) -> Option<Spec035MediaProjection> {
        self.projection.clone()
    }
}

impl ChatCompletionAdapter for StoreMediaAdapter {
    fn configured_model(&self) -> &str {
        "store-model"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(text_response("unused"))
    }

    fn runtime_data_dir(&self) -> Option<PathBuf> {
        Some(self.data_dir.clone())
    }
}

fn included_json() -> Value {
    json!({
        "schema_version": 1,
        "kind": "media_capability",
        "state": "included",
        "reason": {
            "code": "included",
            "safe_summary": "bounded analyzer evidence included"
        },
        "lineage": {
            "artifact_ref": "spec034://media/artifact/api-fixture",
            "analyzer_ref": "spec034://media/analyzer/api-fixture",
            "snapshot_ref": "snapshot:034:api-fixture",
            "evidence_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        },
        "freshness": "current",
        "disclosure": {
            "status": "recorded",
            "raw_content_possible": true,
            "surfaces": ["session", "trace"],
            "trace_status": "enabled"
        },
        "owner_facts": {
            "unavailable_reasons": [],
            "analyzer_source": {
                "analyzer_ref": "spec034://media/analyzer/api-fixture",
                "source": "explicit",
                "activation": "explicit",
                "trust": "explicitOrTrustedWorkspace",
                "trusted_code_disclosure": "shown"
            },
            "sandbox": {
                "availability": "available",
                "status": "active",
                "fallback": "notApplicable",
                "appliedAdapters": ["genericExec"],
                "filesystemPolicy": "applied",
                "networkPolicy": "applied"
            },
            "credential": {
                "availability": "available",
                "status": "resolved",
                "source": "environment",
                "fingerprint": "current",
                "refreshSerialization": "active"
            },
            "snapshot": {
                "snapshot_ref": "snapshot:034:api-fixture",
                "provenance_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
        }
    })
}

pub(crate) fn projection_for(state: &str) -> Result<Spec035MediaProjection, Box<dyn Error>> {
    let mut value = included_json();
    value["state"] = json!(state);
    value["reason"]["code"] = json!(state);
    value["reason"]["safe_summary"] = json!(format!("media state is {state}"));
    match state {
        "included" | "truncated" => {}
        "unsupported" | "extraction_failed" => {
            value["lineage"]
                .as_object_mut()
                .ok_or("lineage must be an object")?
                .remove("evidence_digest");
        }
        "analyzer_missing" => {
            make_unavailable(&mut value, "unavailable", "missing_analyzer_owner_ref")?
        }
        "unavailable" => make_unavailable(&mut value, "stale", "stale_owner_facts")?,
        _ => return Err(format!("unsupported fixture state: {state}").into()),
    }
    Ok(Spec035MediaProjection::from_json_value(value)?)
}

fn make_unavailable(
    value: &mut Value,
    freshness: &str,
    unavailable_reason: &str,
) -> Result<(), Box<dyn Error>> {
    let lineage = value["lineage"]
        .as_object_mut()
        .ok_or("lineage must be an object")?;
    lineage.remove("analyzer_ref");
    lineage.remove("snapshot_ref");
    lineage.remove("evidence_digest");
    value["freshness"] = json!(freshness);
    value["disclosure"] = json!({"status": "unavailable"});
    value["owner_facts"] = json!({"unavailable_reasons": [unavailable_reason]});
    Ok(())
}
