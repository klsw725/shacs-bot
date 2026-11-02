use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::{fmt, fs, io};
use unicode_normalization::UnicodeNormalization;

use crate::runtime::MemoryHistoryEntry;

/// Version of the rules that split the memory files into items; a change rebuilds every index.
pub const CHUNK_RULE_VERSION: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemSource {
    /// Item `ordinal` (from 0) of the history entry at `cursor`.
    History {
        cursor: u64,
        timestamp: String,
        ordinal: usize,
    },
    /// Lines `first_line..=last_line` (from 1) of `MEMORY.md`.
    Memory { first_line: usize, last_line: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryItem {
    pub key: String,
    pub source: ItemSource,
    /// The item as the memory file has it.
    pub text: String,
    /// Lowercase hex SHA-256 of `text`.
    pub digest: String,
}

/// The items of both memory files as read at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemorySnapshot {
    pub items: Vec<MemoryItem>,
    /// History lines left out because they could not be read as an entry.
    pub skipped_history_lines: usize,
}

#[derive(Debug)]
pub struct SnapshotError {
    pub file: PathBuf,
    pub error: io::Error,
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "memory file {} is unreadable: {}",
            self.file.display(),
            self.error
        )
    }
}

impl std::error::Error for SnapshotError {}

/// Reads `memory/history.jsonl` and `memory/MEMORY.md` of `workspace` into items. A missing
/// file has no items; any other failure to read one is an error. Nothing is written.
pub fn read_memory_snapshot(workspace: &Path) -> Result<MemorySnapshot, SnapshotError> {
    let memory_dir = workspace.join("memory");
    let (entries, skipped_history_lines) = read_history(&memory_dir.join("history.jsonl"))?;
    let mut items = Vec::new();
    let mut keys = BTreeSet::new();
    for entry in &entries {
        for (ordinal, block) in split_items(&entry.content).into_iter().enumerate() {
            let digest = text_digest(&block.text);
            let key = format!("h:{}:{ordinal}:{}", entry.cursor, &digest[..16]);
            if keys.insert(key.clone()) {
                items.push(MemoryItem {
                    key,
                    source: ItemSource::History {
                        cursor: entry.cursor,
                        timestamp: entry.timestamp.clone(),
                        ordinal,
                    },
                    text: block.text,
                    digest,
                });
            }
        }
    }
    let memory = read_text(&memory_dir.join("MEMORY.md"))?;
    let mut occurrences: BTreeMap<String, usize> = BTreeMap::new();
    for block in split_items(&memory) {
        let digest = text_digest(&block.text);
        let occurrence = occurrences.entry(digest.clone()).or_insert(0);
        let key = format!("m:{}:{occurrence}", &digest[..16]);
        *occurrence += 1;
        items.push(MemoryItem {
            key,
            source: ItemSource::Memory {
                first_line: block.first_line,
                last_line: block.last_line,
            },
            text: block.text,
            digest,
        });
    }
    Ok(MemorySnapshot {
        items,
        skipped_history_lines,
    })
}

/// The text both searches compare: `text` in Unicode NFC, then lowercased.
pub fn search_text(text: &str) -> String {
    text.nfc().collect::<String>().to_lowercase()
}

fn text_digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

struct Block {
    first_line: usize,
    last_line: usize,
    text: String,
}

/// Splits `text` by lines: a line starting with `- ` is one item, other lines separated by
/// blank lines form one paragraph item, and blank lines and `#` heading lines are dropped.
fn split_items(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph: Option<Block> = None;
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let bullet = line.starts_with("- ");
        if bullet || line.starts_with('#') || line.trim().is_empty() {
            blocks.extend(paragraph.take());
            if bullet {
                blocks.push(Block {
                    first_line: number,
                    last_line: number,
                    text: line.to_owned(),
                });
            }
            continue;
        }
        match &mut paragraph {
            Some(block) => {
                block.last_line = number;
                block.text.push('\n');
                block.text.push_str(line);
            }
            None => {
                paragraph = Some(Block {
                    first_line: number,
                    last_line: number,
                    text: line.to_owned(),
                })
            }
        }
    }
    blocks.extend(paragraph);
    blocks
}

/// Reads the history the way `MemoryStore` does: blank lines are ignored, a line that is not an
/// entry is skipped, and reading stops at the first line that is not UTF-8. Skipped lines,
/// including every non-blank line from such a stop, are counted.
fn read_history(path: &Path) -> Result<(Vec<MemoryHistoryEntry>, usize), SnapshotError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(error) => {
            return Err(SnapshotError {
                file: path.to_path_buf(),
                error,
            })
        }
    };
    let lines: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    let mut entries = Vec::new();
    let mut skipped = 0;
    for (index, raw) in lines.iter().enumerate() {
        let Ok(line) = std::str::from_utf8(raw) else {
            skipped += lines[index..]
                .iter()
                .filter(|rest| !String::from_utf8_lossy(rest).trim().is_empty())
                .count();
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<MemoryHistoryEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => skipped += 1,
        }
    }
    Ok((entries, skipped))
}

fn read_text(path: &Path) -> Result<String, SnapshotError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(SnapshotError {
            file: path.to_path_buf(),
            error,
        }),
    }
}
