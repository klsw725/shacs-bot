use crate::tools::filesystem::{
    raw_candidate_path, reject_existing_symlink_components, resolve_creatable_path, resolve_path,
    PathContext,
};
use crate::tools::SchemaFragment;
use crate::tools::{
    FileState, IntegerSchema, JsonMap, StringSchema, Tool, ToolParameters, ToolResult,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const VALID_CELL_TYPES: &[&str] = &["code", "markdown"];
const VALID_EDIT_MODES: &[&str] = &["replace", "insert", "delete"];

#[derive(Clone)]
pub struct NotebookEditTool {
    context: PathContext,
    file_state: Arc<Mutex<FileState>>,
}

impl NotebookEditTool {
    pub fn new(context: PathContext) -> Self {
        Self {
            context,
            file_state: Arc::new(Mutex::new(FileState::new())),
        }
    }

    pub fn with_file_state(context: PathContext, file_state: Arc<Mutex<FileState>>) -> Self {
        Self {
            context,
            file_state,
        }
    }
}

impl Tool for NotebookEditTool {
    fn name(&self) -> &str {
        "notebook_edit"
    }

    fn description(&self) -> &str {
        "Edit a Jupyter notebook (.ipynb) cell. Modes: replace, insert, delete. cell_index is 0-based."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property(
                "path",
                StringSchema::new("Path to the .ipynb notebook file"),
            )
            .property(
                "cell_index",
                IntegerSchema::new("0-based index of the cell to edit").minimum(0),
            )
            .property(
                "new_source",
                StringSchema::new("New source content for the cell"),
            )
            .raw_property(
                "cell_type",
                json!({
                    "type": "string",
                    "enum": ["code", "markdown"],
                    "description": "Cell type: code or markdown (default: code)"
                }),
            )
            .raw_property(
                "edit_mode",
                json!({
                    "type": "string",
                    "enum": ["replace", "insert", "delete"],
                    "description": "Mode: replace (default), insert (after target), or delete"
                }),
            )
            .required(["path", "cell_index"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let cell_index = params
            .get("cell_index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        let new_source = params
            .get("new_source")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let cell_type = params
            .get("cell_type")
            .and_then(Value::as_str)
            .unwrap_or("code");
        let edit_mode = params
            .get("edit_mode")
            .and_then(Value::as_str)
            .unwrap_or("replace");

        match self.edit_notebook(path, cell_index, new_source, cell_type, edit_mode) {
            Ok(message) => message.into(),
            Err(error) => format!("Error editing notebook: {error}").into(),
        }
    }
}

impl NotebookEditTool {
    fn edit_notebook(
        &self,
        path: &str,
        cell_index: usize,
        new_source: &str,
        cell_type: &str,
        edit_mode: &str,
    ) -> Result<String, String> {
        if path.is_empty() {
            return Ok("Error: path is required".to_owned());
        }
        if !path.ends_with(".ipynb") {
            return Ok(
                "Error: notebook_edit only works on .ipynb files. Use edit_file for other files."
                    .to_owned(),
            );
        }
        if !VALID_EDIT_MODES.contains(&edit_mode) {
            return Ok(format!(
                "Error: Invalid edit_mode '{edit_mode}'. Use one of: replace, insert, delete."
            ));
        }
        if !VALID_CELL_TYPES.contains(&cell_type) {
            return Ok(format!(
                "Error: Invalid cell_type '{cell_type}'. Use one of: code, markdown."
            ));
        }

        let raw_candidate = raw_candidate_path(path, &self.context);
        reject_existing_symlink_components(&raw_candidate)?;
        let fp = if raw_candidate.exists() {
            resolve_path(path, &self.context)?
        } else if edit_mode == "insert" {
            resolve_creatable_path(path, &self.context)?
        } else {
            return Ok(format!("Error: File not found: {path}"));
        };

        if !fp.exists() {
            let mut notebook = make_empty_notebook();
            let cell = new_cell(new_source, cell_type, true, &HashSet::new());
            notebook["cells"] = Value::Array(vec![cell]);
            write_notebook(&fp, &notebook)?;
            self.record_write(&fp)?;
            return Ok(format!("Successfully created {} with 1 cell", fp.display()));
        }

        if !fp.is_file() {
            return Ok(format!("Error: Not a file: {path}"));
        }

        let text = fs::read_to_string(&fp)
            .map_err(|error| format!("Failed to parse notebook: {error}"))?;
        let mut notebook: Value = serde_json::from_str(&text)
            .map_err(|error| format!("Failed to parse notebook: {error}"))?;
        let generate_id = should_generate_id(&notebook);
        ensure_cells_array(&mut notebook)?;
        let cells = notebook
            .get_mut("cells")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| "Notebook cells must be an array".to_owned())?;
        let existing_ids = collect_cell_ids(cells);

        match edit_mode {
            "delete" => {
                if cell_index >= cells.len() {
                    return Ok(format!(
                        "Error: cell_index {cell_index} out of range (notebook has {} cells)",
                        cells.len()
                    ));
                }
                cells.remove(cell_index);
                write_notebook(&fp, &notebook)?;
                self.record_write(&fp)?;
                Ok(format!(
                    "Successfully deleted cell {cell_index} from {}",
                    fp.display()
                ))
            }
            "insert" => {
                let insert_at = cell_index.saturating_add(1).min(cells.len());
                cells.insert(
                    insert_at,
                    new_cell(new_source, cell_type, generate_id, &existing_ids),
                );
                write_notebook(&fp, &notebook)?;
                self.record_write(&fp)?;
                Ok(format!(
                    "Successfully inserted cell at index {insert_at} in {}",
                    fp.display()
                ))
            }
            "replace" => {
                if cell_index >= cells.len() {
                    return Ok(format!(
                        "Error: cell_index {cell_index} out of range (notebook has {} cells)",
                        cells.len()
                    ));
                }
                replace_cell(&mut cells[cell_index], new_source, cell_type)?;
                write_notebook(&fp, &notebook)?;
                self.record_write(&fp)?;
                Ok(format!(
                    "Successfully edited cell {cell_index} in {}",
                    fp.display()
                ))
            }
            _ => Ok(format!(
                "Error: Invalid edit_mode '{edit_mode}'. Use one of: replace, insert, delete."
            )),
        }
    }

    fn record_write(&self, path: &Path) -> Result<(), String> {
        self.file_state
            .lock()
            .map_err(|_| "file state lock poisoned".to_owned())?
            .record_write(path);
        Ok(())
    }
}

fn new_cell(
    source: &str,
    cell_type: &str,
    generate_id: bool,
    existing_ids: &HashSet<String>,
) -> Value {
    let mut cell = Map::new();
    cell.insert("cell_type".to_owned(), Value::String(cell_type.to_owned()));
    cell.insert("source".to_owned(), Value::String(source.to_owned()));
    cell.insert("metadata".to_owned(), Value::Object(Map::new()));
    if cell_type == "code" {
        cell.insert("outputs".to_owned(), Value::Array(Vec::new()));
        cell.insert("execution_count".to_owned(), Value::Null);
    }
    if generate_id {
        cell.insert(
            "id".to_owned(),
            Value::String(generate_cell_id(source, cell_type, existing_ids)),
        );
    }
    Value::Object(cell)
}

fn make_empty_notebook() -> Value {
    json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": {
            "kernelspec": {"display_name": "Python 3", "language": "python", "name": "python3"},
            "language_info": {"name": "python"}
        },
        "cells": []
    })
}

fn should_generate_id(notebook: &Value) -> bool {
    let nbformat = notebook
        .get("nbformat")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let minor = notebook
        .get("nbformat_minor")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    nbformat >= 4 && minor >= 5
}

fn ensure_cells_array(notebook: &mut Value) -> Result<(), String> {
    let Some(object) = notebook.as_object_mut() else {
        return Err("Notebook root must be an object".to_owned());
    };
    match object.get("cells") {
        Some(Value::Array(_)) => Ok(()),
        Some(_) => Err("Notebook cells must be an array".to_owned()),
        None => {
            object.insert("cells".to_owned(), Value::Array(Vec::new()));
            Ok(())
        }
    }
}

fn replace_cell(cell: &mut Value, source: &str, cell_type: &str) -> Result<(), String> {
    let Some(object) = cell.as_object_mut() else {
        return Err("Notebook cell must be an object".to_owned());
    };
    object.insert("source".to_owned(), Value::String(source.to_owned()));
    let previous_type = object
        .get("cell_type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if previous_type != cell_type {
        object.insert("cell_type".to_owned(), Value::String(cell_type.to_owned()));
        if cell_type == "code" {
            object
                .entry("outputs".to_owned())
                .or_insert_with(|| Value::Array(Vec::new()));
            object
                .entry("execution_count".to_owned())
                .or_insert(Value::Null);
        } else {
            object.remove("outputs");
            object.remove("execution_count");
        }
    }
    Ok(())
}

fn collect_cell_ids(cells: &[Value]) -> HashSet<String> {
    cells
        .iter()
        .filter_map(|cell| cell.get("id"))
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn write_notebook(path: &Path, notebook: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(notebook).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())
}

fn generate_cell_id(source: &str, cell_type: &str, existing_ids: &HashSet<String>) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    for attempt in 0u64..=u64::MAX {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        hasher.update(cell_type.as_bytes());
        hasher.update(nanos.to_string().as_bytes());
        hasher.update(attempt.to_string().as_bytes());
        let digest = format!("{:x}", hasher.finalize());
        let candidate = digest[..8].to_owned();
        if !existing_ids.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("u64 attempt space exhausted while generating notebook cell id")
}
