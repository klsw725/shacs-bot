use shacs_core::runtime::{run_spec033_release_runner, Spec033ReleaseConfig, Spec033ReleaseMode};
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    if std::env::args()
        .skip(1)
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        println!("Usage: spec033-release-runner --run-id <id> --repo-root <path> --evidence-root <path> --trajectory-root <path> --data-dir <path> --trajectory-id <id> --mode <current-worktree|fixture>");
        return;
    }
    match parse()
        .and_then(|config| run_spec033_release_runner(&config).map_err(|error| error.to_string()))
    {
        Ok(manifest) => println!(
            "spec033 release evidence published: checks={} coverage={} artifacts={}",
            manifest.commands.len(),
            manifest.coverage.len(),
            manifest.artifact_digests.len()
        ),
        Err(error) => {
            eprintln!("spec033 release runner failed: {error}");
            std::process::exit(1);
        }
    }
}

fn parse() -> Result<Spec033ReleaseConfig, String> {
    parse_args(std::env::args().skip(1))
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Spec033ReleaseConfig, String> {
    let mut run_id = None;
    let mut repo_root = None;
    let mut evidence_root = None;
    let mut trajectory_root = None;
    let mut data_dir = None;
    let mut trajectory_id = None;
    let mut mode = None;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--run-id" => run_id = args.next(),
            "--repo-root" => repo_root = args.next().map(PathBuf::from),
            "--evidence-root" => evidence_root = args.next().map(PathBuf::from),
            "--trajectory-root" => trajectory_root = args.next().map(PathBuf::from),
            "--data-dir" => data_dir = args.next().map(PathBuf::from),
            "--trajectory-id" => trajectory_id = args.next(),
            "--mode" => mode = args.next(),
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(Spec033ReleaseConfig {
        run_id: run_id.ok_or_else(|| "missing --run-id".to_owned())?,
        repo_root: repo_root.ok_or_else(|| "missing --repo-root".to_owned())?,
        evidence_root: evidence_root.ok_or_else(|| "missing --evidence-root".to_owned())?,
        trajectory_root: trajectory_root.ok_or_else(|| "missing --trajectory-root".to_owned())?,
        data_dir: data_dir.ok_or_else(|| "missing --data-dir".to_owned())?,
        trajectory_id: trajectory_id.ok_or_else(|| "missing --trajectory-id".to_owned())?,
        mode: match mode.as_deref() {
            Some("current-worktree") => Spec033ReleaseMode::CurrentWorktree,
            Some("fixture") => Spec033ReleaseMode::Fixture,
            Some(value) => return Err(format!("unknown mode: {value}")),
            None => return Err("missing --mode".to_owned()),
        },
        command_timeout: Duration::from_secs(7_200),
    })
}

#[cfg(test)]
mod tests {
    use super::parse_args;
    use std::path::Path;

    fn required_args() -> Vec<String> {
        [
            "--run-id",
            "release-1",
            "--repo-root",
            "/repo",
            "--evidence-root",
            "/evidence",
            "--trajectory-root",
            "/trajectories",
            "--data-dir",
            "/runtime-data",
            "--trajectory-id",
            "trajectory-1",
            "--mode",
            "fixture",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[test]
    fn parse_requires_explicit_runtime_data_dir() {
        let args = required_args()
            .into_iter()
            .filter(|argument| argument != "--data-dir" && argument != "/runtime-data");

        let result = parse_args(args);

        assert_eq!(result.unwrap_err(), "missing --data-dir");
    }

    #[test]
    fn parse_keeps_runtime_data_dir_distinct_from_trajectory_root() {
        let config = parse_args(required_args()).expect("valid release arguments");

        assert_eq!(config.trajectory_root, Path::new("/trajectories"));
        assert_eq!(config.data_dir, Path::new("/runtime-data"));
    }
}
