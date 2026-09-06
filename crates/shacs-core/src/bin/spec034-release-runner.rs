use shacs_core::runtime::{run_spec034_release_runner, Spec034ReleaseConfig, Spec034ReleaseMode};
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    if std::env::var("SHACS_SPEC034_LINKER_MODE").ok().as_deref() == Some("self-image-v1") {
        if shacs_core::runtime::run_spec034_linker_wrapper().is_err() {
            std::process::exit(1);
        }
        return;
    }
    if std::env::args()
        .skip(1)
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        println!("Usage: spec034-release-runner --run-id <id> --repo-root <path> --evidence-root <path> --mode <success-fixture|current-worktree>");
        return;
    }
    match parse().and_then(|config| {
        run_spec034_release_runner(&config)
            .map(|result| result.identity.content_digest)
            .map_err(|error| error.to_string())
    }) {
        Ok(content_digest) => match approved_stdout(&content_digest) {
            Ok(line) => println!("{line}"),
            Err(error) => {
                eprintln!("spec034 release runner failed: {error}");
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("spec034 release runner failed: {error}");
            std::process::exit(1);
        }
    }
}

fn approved_stdout(content_digest: &str) -> Result<&str, String> {
    let hex = content_digest
        .strip_prefix("sha256:")
        .ok_or_else(|| "invalid publication digest".to_owned())?;
    (hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
    .then_some(content_digest)
    .ok_or_else(|| "invalid publication digest".to_owned())
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
        cache_root: std::env::var_os("SHACS_SPEC034_RELEASE_CACHE_ROOT").map(PathBuf::from),
        mode: match mode.as_deref() {
            Some("success-fixture") => Spec034ReleaseMode::SuccessFixture,
            Some("current-worktree") => Spec034ReleaseMode::CurrentWorktree,
            Some(value) => return Err(format!("unknown mode: {value}")),
            None => return Err("missing --mode".to_owned()),
        },
        command_timeout: Duration::from_secs(600),
    })
}

#[cfg(test)]
mod tests {
    use super::approved_stdout;

    #[test]
    fn success_stdout_is_exact_lowercase_sha256_marker() {
        let digest = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(approved_stdout(digest), Ok(digest));
        assert!(approved_stdout("sha256:ABCDEF").is_err());
    }
}
