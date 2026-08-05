use shacs_projection::{
    run_spec031_release_runner, Spec031ReleaseRunId, Spec031ReleaseRunnerConfig,
    Spec031ReleaseRunnerMode,
};
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    if std::env::args()
        .skip(1)
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        print_usage();
        return;
    }
    match parse_config().and_then(|config| {
        run_spec031_release_runner(&config)
            .map(|_| ())
            .map_err(|error| format!("{error:?}"))
    }) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("spec031 release runner failed: {error:?}");
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    println!(
        "Usage: spec031-release-runner --run-id <id> --evidence-root <path> --repo-root <path> --mode <success-fixture|current-worktree>"
    );
}

fn parse_config() -> Result<Spec031ReleaseRunnerConfig, String> {
    let mut run_id = None;
    let mut evidence_root = None;
    let mut repo_root = None;
    let mut mode = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--run-id" => run_id = args.next(),
            "--evidence-root" => evidence_root = args.next().map(PathBuf::from),
            "--repo-root" => repo_root = args.next().map(PathBuf::from),
            "--mode" => mode = args.next().map(parse_mode).transpose()?,
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    Ok(Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new(
            run_id
                .as_deref()
                .ok_or_else(|| "missing --run-id".to_owned())?,
        )
        .map_err(|_| "invalid --run-id".to_owned())?,
        evidence_root: evidence_root.ok_or_else(|| "missing --evidence-root".to_owned())?,
        repo_root: repo_root.ok_or_else(|| "missing --repo-root".to_owned())?,
        mode: mode.ok_or_else(|| "missing --mode".to_owned())?,
        command_timeout: Duration::from_secs(7_200),
    })
}

fn parse_mode(value: String) -> Result<Spec031ReleaseRunnerMode, String> {
    match value.as_str() {
        "success-fixture" => Ok(Spec031ReleaseRunnerMode::SuccessFixture),
        "current-worktree" => Ok(Spec031ReleaseRunnerMode::CurrentWorktree),
        _ => Err(format!("unknown --mode: {value}")),
    }
}
