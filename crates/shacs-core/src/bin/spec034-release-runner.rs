use shacs_core::runtime::{run_spec034_release_runner, Spec034ReleaseConfig, Spec034ReleaseMode};
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    if std::env::args()
        .skip(1)
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        println!("Usage: spec034-release-runner --run-id <id> --repo-root <path> --evidence-root <path> --mode <success-fixture|current-worktree>");
        return;
    }
    match parse().and_then(|config| run_spec034_release_runner(&config).map_err(|error| error.to_string())) {
        Ok(manifest) => println!(
            "spec034 runner evidence published: requirements={} blockers={} dirty={} closure_eligible={}",
            manifest.requirement_count,
            manifest.blocker_count,
            manifest.source.worktree_dirty,
            manifest.closure_eligible
        ),
        Err(error) => {
            eprintln!("spec034 release runner failed: {error}");
            std::process::exit(1);
        }
    }
}

fn parse() -> Result<Spec034ReleaseConfig, String> {
    let mut run_id = None;
    let mut repo_root = None;
    let mut evidence_root = None;
    let mut mode = None;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--run-id" => run_id = args.next(),
            "--repo-root" => repo_root = args.next().map(PathBuf::from),
            "--evidence-root" => evidence_root = args.next().map(PathBuf::from),
            "--mode" => mode = args.next(),
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(Spec034ReleaseConfig {
        run_id: run_id.ok_or_else(|| "missing --run-id".to_owned())?,
        repo_root: repo_root.ok_or_else(|| "missing --repo-root".to_owned())?,
        evidence_root: evidence_root.ok_or_else(|| "missing --evidence-root".to_owned())?,
        mode: match mode.as_deref() {
            Some("success-fixture") => Spec034ReleaseMode::SuccessFixture,
            Some("current-worktree") => Spec034ReleaseMode::CurrentWorktree,
            Some(value) => return Err(format!("unknown mode: {value}")),
            None => return Err("missing --mode".to_owned()),
        },
        command_timeout: Duration::from_secs(7_200),
    })
}
