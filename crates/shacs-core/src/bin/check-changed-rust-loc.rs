use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[path = "check_changed_rust_loc/marker_policy.rs"]
mod marker_policy;

use marker_policy::{marker_is_valid, MarkerPolicyInput};

const MARKER: &str = "// allow: SIZE_OK — ";

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) | Err(_) => ExitCode::FAILURE,
    }
}

fn run() -> Result<bool, String> {
    let base = env::args()
        .nth(1)
        .ok_or_else(|| "usage: check-changed-rust-loc <base-revision>".to_owned())?;
    git(&["rev-parse", "--verify", &format!("{base}^{{commit}}")])?;
    let files = changed_rust_files(&base)?;
    let mut valid = true;

    for path in files {
        let source =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let pure_loc = count_pure_loc(&source);
        let markers = source
            .lines()
            .filter_map(|line| line.trim().strip_prefix(MARKER))
            .collect::<Vec<_>>();
        let existed_at_base =
            git_success(&["cat-file", "-e", &format!("{base}:{}", path.display())]);
        let semantic_additions = added_pure_loc(&base, &path)?;
        let marker_valid = marker_is_valid(MarkerPolicyInput {
            path: &path,
            markers: &markers,
            existed_at_base,
            semantic_additions,
            pure_loc,
        });
        let status = if marker_valid { "PASS" } else { "FAIL" };
        println!(
            "{status} pure_loc={pure_loc} semantic_additions={semantic_additions} marker_count={} path={}",
            markers.len(),
            path.display()
        );
        valid &= marker_valid;
    }
    Ok(valid)
}

fn changed_rust_files(base: &str) -> Result<BTreeSet<PathBuf>, String> {
    let mut files = BTreeSet::new();
    for path in nul_paths(&git(&["diff", "--name-only", "-z", base, "--", "*.rs"])?) {
        if path.is_file() {
            files.insert(path);
        }
    }
    for path in nul_paths(&git(&[
        "ls-files",
        "--others",
        "--exclude-standard",
        "-z",
        "--",
        "*.rs",
    ])?) {
        if path.is_file() {
            files.insert(path);
        }
    }
    Ok(files)
}

fn nul_paths(output: &[u8]) -> impl Iterator<Item = PathBuf> + '_ {
    output
        .split(|byte| *byte == 0)
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| PathBuf::from(String::from_utf8_lossy(bytes).into_owned()))
}

fn added_pure_loc(base: &str, path: &Path) -> Result<usize, String> {
    let diff = git(&[
        "diff",
        "--unified=0",
        "--no-ext-diff",
        base,
        "--",
        path.to_string_lossy().as_ref(),
    ])?;
    let text = String::from_utf8(diff).map_err(|error| error.to_string())?;
    let additions = text
        .lines()
        .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
        .map(|line| &line[1..])
        .collect::<Vec<_>>()
        .join("\n");
    Ok(count_pure_loc(&additions))
}

fn count_pure_loc(source: &str) -> usize {
    let mut in_block_comment = false;
    source
        .lines()
        .filter(|line| line_has_code(line, &mut in_block_comment))
        .count()
}

fn line_has_code(line: &str, in_block_comment: &mut bool) -> bool {
    if !*in_block_comment && line.trim_start().starts_with('#') {
        return false;
    }
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut has_code = false;
    while index < bytes.len() {
        if *in_block_comment {
            if bytes[index..].starts_with(b"*/") {
                *in_block_comment = false;
                index += 2;
            } else {
                index += 1;
            }
        } else if bytes[index..].starts_with(b"//") {
            break;
        } else if bytes[index..].starts_with(b"/*") {
            *in_block_comment = true;
            index += 2;
        } else {
            has_code |= !bytes[index].is_ascii_whitespace();
            index += 1;
        }
    }
    has_code
}

fn git(args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn git_success(args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}
