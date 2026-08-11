use shacs_projection::{
    run_spec030_release_runner, Spec030ReleaseRunId, Spec030ReleaseRunnerConfig,
    Spec030ReleaseRunnerMode, Spec030ReleaseVerdict,
};
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    if std::env::args()
        .skip(1)
        .any(|argument| matches!(argument.as_str(), "--help" | "-h"))
    {
        print_usage();
        return;
    }
    match parse_config()
        .and_then(|config| run_spec030_release_runner(&config).map_err(|error| error.to_string()))
    {
        Ok(artifacts) if artifacts.verdict == Spec030ReleaseVerdict::Pass => {
            println!("PASS {}", artifacts.evidence_root);
        }
        Ok(artifacts) => {
            eprintln!("BLOCKED {}", artifacts.evidence_root);
            for blocker in artifacts.blockers {
                eprintln!("{}: {}", blocker.code, blocker.detail);
            }
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("spec030 release runner failed: {error}");
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    println!(
        "Usage: spec030-release-runner --run-id <id> --evidence-root <path> --repo-root <path> --mode <success-fixture|current-worktree> [--manual-record <path>]... [--bwrap-record <path>]"
    );
}

fn parse_config() -> Result<Spec030ReleaseRunnerConfig, String> {
    let mut run_id = None;
    let mut evidence_root = None;
    let mut repo_root = None;
    let mut mode = None;
    let mut manual_records = Vec::new();
    let mut bwrap_record = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--run-id" => run_id = arguments.next(),
            "--evidence-root" => evidence_root = arguments.next().map(PathBuf::from),
            "--repo-root" => repo_root = arguments.next().map(PathBuf::from),
            "--mode" => mode = arguments.next().map(parse_mode).transpose()?,
            "--manual-record" => manual_records.push(PathBuf::from(
                arguments
                    .next()
                    .ok_or_else(|| "missing --manual-record value".to_owned())?,
            )),
            "--bwrap-record" => {
                bwrap_record = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "missing --bwrap-record value".to_owned())?,
                ));
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(Spec030ReleaseRunnerConfig {
        run_id: Spec030ReleaseRunId::try_new(
            run_id
                .as_deref()
                .ok_or_else(|| "missing --run-id".to_owned())?,
        )
        .map_err(|_| "invalid --run-id".to_owned())?,
        evidence_root: evidence_root.ok_or_else(|| "missing --evidence-root".to_owned())?,
        repo_root: repo_root.ok_or_else(|| "missing --repo-root".to_owned())?,
        mode: mode.ok_or_else(|| "missing --mode".to_owned())?,
        command_timeout: Duration::from_secs(7_200),
        manual_records,
        bwrap_record,
    })
}

fn parse_mode(value: String) -> Result<Spec030ReleaseRunnerMode, String> {
    match value.as_str() {
        "success-fixture" => Ok(Spec030ReleaseRunnerMode::SuccessFixture),
        "current-worktree" => Ok(Spec030ReleaseRunnerMode::CurrentWorktree),
        _ => Err(format!("unknown --mode: {value}")),
    }
}
