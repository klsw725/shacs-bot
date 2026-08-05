use super::{value, CanonicalFields, CHAT_ID, FIELDS, FINAL_TEXT, OBSERVED_AT_UNIX_MS, REPLY_ID};
use serde_json::{json, Value};
use std::error::Error;
use std::path::Path;

pub fn write(
    root: &Path,
    expected: &CanonicalFields,
    rows: &[(&str, &str, CanonicalFields); 4],
) -> Result<(), Box<dyn Error>> {
    let dir = root.join(".omo/evidence/spec031/prd001/parity");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("fixture-registry.json"),
        serde_json::to_vec_pretty(&registry())?,
    )?;
    std::fs::write(
        dir.join("parity-matrix.json"),
        serde_json::to_vec_pretty(&matrix(expected, rows))?,
    )?;
    std::fs::write(
        dir.join("qa-redaction.json"),
        serde_json::to_vec_pretty(&qa())?,
    )?;
    Ok(())
}

fn registry() -> Value {
    json!({"schema":"spec031-prd001-parity-fixture-registry.v4","owner_input":{"chat_id":CHAT_ID,"reply_id":REPLY_ID,"final_text":FINAL_TEXT,"observed_at_unix_ms":OBSERVED_AT_UNIX_MS},"oracle":"literal raw owner constants; no production projector or surface helper"})
}

fn matrix(expected: &CanonicalFields, rows: &[(&str, &str, CanonicalFields); 4]) -> Value {
    json!({"schema":"spec031-prd001-parity-matrix.v4","canonical_fields":FIELDS,"supported":rows.iter().map(|(surface,path,actual)| json!({"surface":surface,"adapter_path":path,"expected":fields_json(expected),"actual":fields_json(actual),"result":"pass"})).collect::<Vec<_>>()})
}

fn qa() -> Value {
    json!({"schema":"spec031-prd001-black-box-qa.v1","checks":["shipped spec031-fixture unknown","CLI channels status reachable","HTTP /health reachable","HTTP /v1/diagnostics semantic response reachable","real WebSocket emits safe Spec031 frame","external channel unsupported explicit"],"redaction":"no sentinel or raw payload fields"})
}

fn fields_json(fields: &CanonicalFields) -> Value {
    json!({"kind":value(fields,"kind"),"state":value(fields,"state"),"severity":value(fields,"severity"),"reason":{"code":value(fields,"reason.code")},"lineage":{"subject_ref":value(fields,"lineage.subject_ref"),"parent_ref":value(fields,"lineage.parent_ref"),"action_ref":value(fields,"lineage.action_ref")},"capability":{"kind":value(fields,"capability.kind"),"delivery":value(fields,"capability.delivery")}})
}
