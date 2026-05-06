use std::path::{Path, PathBuf};

pub fn wrap_command(
    sandbox: &str,
    command: &str,
    workspace: &Path,
    cwd: &Path,
    media_dir: Option<&Path>,
) -> Result<String, String> {
    match sandbox {
        "bwrap" => Ok(wrap_bwrap(command, workspace, cwd, media_dir)),
        other => Err(format!(
            "Unknown sandbox backend {other:?}. Available: [\"bwrap\"]"
        )),
    }
}

fn wrap_bwrap(command: &str, workspace: &Path, cwd: &Path, media_dir: Option<&Path>) -> String {
    let workspace = canonical_or_self(workspace);
    let cwd = canonical_or_self(cwd);
    let sandbox_cwd = cwd
        .strip_prefix(&workspace)
        .map(|relative| workspace.join(relative))
        .unwrap_or_else(|_| workspace.clone());
    let media = media_dir.map(canonical_or_self);

    let mut args = vec![
        "bwrap".to_owned(),
        "--new-session".to_owned(),
        "--die-with-parent".to_owned(),
    ];
    args.extend(["--ro-bind".to_owned(), "/usr".to_owned(), "/usr".to_owned()]);
    for path in [
        "/bin",
        "/lib",
        "/lib64",
        "/etc/alternatives",
        "/etc/ssl/certs",
        "/etc/resolv.conf",
        "/etc/ld.so.cache",
    ] {
        args.extend(["--ro-bind-try".to_owned(), path.to_owned(), path.to_owned()]);
    }
    args.extend([
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
        "--tmpfs".to_owned(),
        workspace
            .parent()
            .unwrap_or(workspace.as_path())
            .to_string_lossy()
            .into_owned(),
        "--dir".to_owned(),
        workspace.to_string_lossy().into_owned(),
        "--bind".to_owned(),
        workspace.to_string_lossy().into_owned(),
        workspace.to_string_lossy().into_owned(),
    ]);
    if let Some(media) = media {
        args.extend([
            "--ro-bind-try".to_owned(),
            media.to_string_lossy().into_owned(),
            media.to_string_lossy().into_owned(),
        ]);
    }
    args.extend([
        "--chdir".to_owned(),
        sandbox_cwd.to_string_lossy().into_owned(),
        "--".to_owned(),
        "sh".to_owned(),
        "-c".to_owned(),
        command.to_owned(),
    ]);
    shell_join(&args)
}

fn canonical_or_self(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':' | b'=')
    }) {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
