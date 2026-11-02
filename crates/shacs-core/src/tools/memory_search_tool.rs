use serde_json::Value;
use std::sync::Arc;

use crate::memory_search::{MemorySearch, DEFAULT_SEARCH_RESULTS, MAX_SEARCH_RESULTS};
use crate::tools::{
    IntegerSchema, JsonMap, SchemaFragment, StringSchema, Tool, ToolParameters, ToolResult,
};

pub const MEMORY_SEARCH_TOOL_NAME: &str = "memory_search";

pub struct MemorySearchTool {
    search: Arc<MemorySearch>,
}

impl MemorySearchTool {
    pub fn new(search: Arc<MemorySearch>) -> Self {
        Self { search }
    }
}

impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        MEMORY_SEARCH_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Search the user's memory (memory/MEMORY.md and memory/history.jsonl) by the words and \
         the meaning of a query, with a local model; nothing leaves this machine. Returns the best \
         items with their exact text, their source (MEMORY.md lines, or history cursor and \
         timestamp) and whether the words, the meaning or both found them, plus the semantic \
         search and index status. Use grep on memory/history.jsonl for an exact string or time."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property(
                "query",
                StringSchema::new("What to look for, in natural language or keywords"),
            )
            .property(
                "limit",
                IntegerSchema::new("How many items to return (default 5)")
                    .minimum(1)
                    .maximum(MAX_SEARCH_RESULTS as i64),
            )
            .required(["query"])
            .to_json_schema()
    }

    fn read_only(&self) -> bool {
        true
    }

    fn concurrency_safe(&self) -> bool {
        false
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|limit| usize::try_from(limit).ok())
            .unwrap_or(DEFAULT_SEARCH_RESULTS);
        ToolResult::Text(self.search.search(query, limit))
    }
}
