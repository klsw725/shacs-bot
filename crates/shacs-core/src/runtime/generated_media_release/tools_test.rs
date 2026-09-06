use super::*;

const CHILD: &str = "SPEC034_TOOL_ENV_CHILD";

#[test]
fn release_temp_directories_are_identifiable() -> Result<(), Box<dyn std::error::Error>> {
    let toolchain = ResolvedToolchain::resolve()?;
    let root = toolchain.home.parent().ok_or("controlled root")?;
    assert!(root
        .file_name()
        .ok_or("controlled root name")?
        .to_string_lossy()
        .starts_with(".shacs-spec034-tools-"));
    assert_eq!(toolchain.home.file_name().and_then(|name| name.to_str()), Some("home"));
    assert_eq!(
        toolchain.cargo_home.file_name().and_then(|name| name.to_str()),
        Some("cargo-home")
    );
    assert_eq!(toolchain.target.file_name().and_then(|name| name.to_str()), Some("target"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn malicious_cargo_home_parent_a_b_a_breaks_controlled_root_seal(
) -> Result<(), Box<dyn std::error::Error>> {
    let toolchain = ResolvedToolchain::resolve()?;
    let cargo_home = toolchain.cargo_home_path().to_path_buf();
    let controlled = cargo_home.parent().ok_or("controlled root")?.to_path_buf();
    let parent = controlled.parent().ok_or("controlled parent")?;
    let displaced = parent.join("controlled-a");
    let replacement = parent.join("controlled-b");

    std::fs::rename(&controlled, &displaced)?;
    std::fs::create_dir_all(replacement.join("cargo-home"))?;
    std::fs::write(
        replacement.join("cargo-home/config.toml"),
        b"[build]\nrustc-wrapper='/malicious'\n",
    )?;
    std::fs::rename(&replacement, &controlled)?;
    std::fs::remove_dir_all(&controlled)?;
    std::fs::rename(&displaced, &controlled)?;

    assert!(matches!(
        toolchain.verify(),
        Err(Spec034ReleaseArtifactError::DigestMismatch)
    ));
    Ok(())
}

#[test]
fn fake_path_cannot_replace_resolved_cargo() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let fake = root.path().join("cargo");
    std::fs::write(&fake, b"fake cargo")?;
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("runtime::generated_media_release::tools::tests::fake_path_child")
        .arg("--nocapture")
        .env(CHILD, "fake-path")
        .env("PATH", root.path())
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn fake_path_child() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var(CHILD).as_deref() != Ok("fake-path") {
        return Ok(());
    }
    let resolved = ResolvedTool::cargo()?;
    assert_eq!(resolved.identity.name, "cargo");
    assert!(resolved.identity.executable_digest.starts_with("sha256:"));
    assert!(!resolved.path.starts_with(std::env::var_os("PATH").ok_or("PATH")?));
    Ok(())
}

#[test]
fn git_environment_injection_is_not_inherited() -> Result<(), Box<dyn std::error::Error>> {
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("runtime::generated_media_release::tools::tests::git_environment_child")
        .arg("--nocapture")
        .env(CHILD, "git-env")
        .env("GIT_DIR", "/nonexistent/forged-git-dir")
        .env("GIT_INDEX_FILE", "/nonexistent/forged-index")
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn git_environment_child() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var(CHILD).as_deref() != Ok("git-env") {
        return Ok(());
    }
    let git = ResolvedTool::git()?;
    let output = git.command(Path::new(".")).arg("--version").output()?;
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)?.starts_with("git version"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn ambient_cargo_home_wrapper_cannot_run() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    let cargo_home = root.path().join("cargo-home");
    let project = root.path().join("project");
    let marker = root.path().join("wrapper-ran");
    std::fs::create_dir_all(&cargo_home)?;
    std::fs::create_dir_all(project.join("src"))?;
    std::fs::write(project.join("Cargo.toml"), b"[package]\nname='probe'\nversion='0.1.0'\n")?;
    std::fs::write(project.join("src/lib.rs"), b"pub fn probe() {}\n")?;
    let wrapper = root.path().join("wrapper.sh");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\ntouch '{}'\nexec \"$@\"\n", marker.display()),
    )?;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))?;
    std::fs::write(
        cargo_home.join("config.toml"),
        format!("[build]\nrustc-wrapper = '{}'\n", wrapper.display()),
    )?;

    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("runtime::generated_media_release::tools::tests::ambient_cargo_home_wrapper_child")
        .arg("--nocapture")
        .env(CHILD, "cargo-home")
        .env("CARGO_HOME", &cargo_home)
        .env("SPEC034_TOOL_PROJECT", &project)
        .env("SPEC034_TOOL_MARKER", &marker)
        .status()?;
    assert!(status.success());
    assert!(!marker.exists());
    Ok(())
}

#[test]
fn ambient_cargo_home_wrapper_child() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var(CHILD).as_deref() != Ok("cargo-home") {
        return Ok(());
    }
    let project = std::env::var_os("SPEC034_TOOL_PROJECT").ok_or("project")?;
    let marker = std::env::var_os("SPEC034_TOOL_MARKER").ok_or("marker")?;
    let status = ResolvedToolchain::resolve()?
        .command(&Path::new(&project).join("Cargo.toml"))?
        .args(["check", "--quiet", "--manifest-path"])
        .arg(Path::new(&project).join("Cargo.toml"))
        .status()?;
    assert!(status.success());
    assert!(!Path::new(&marker).exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn snapshot_parent_cargo_config_is_ignored() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let project = root.join("snapshot/project");
    let marker = root.join("runner-ran");
    write_test_project(&project)?;
    write_runner_config(&root, &marker)?;
    let status = ResolvedToolchain::resolve()?
        .command(&project.join("Cargo.toml"))?
        .args(["test", "--quiet", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .status()?;

    assert!(status.success());
    assert!(!marker.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn tmpdir_cargo_config_is_ignored() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let project = root.join("project");
    let marker = root.join("runner-ran");
    write_test_project(&project)?;
    write_runner_config(&root, &marker)?;
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("runtime::generated_media_release::tools::tests::tmpdir_cargo_config_child")
        .arg("--nocapture")
        .env(CHILD, "tmpdir-config")
        .env("TMPDIR", &root)
        .env("SPEC034_TOOL_PROJECT", &project)
        .env("SPEC034_TOOL_MARKER", &marker)
        .status()?;
    assert!(status.success());
    assert!(!marker.exists());
    Ok(())
}

#[test]
fn tmpdir_cargo_config_child() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var(CHILD).as_deref() != Ok("tmpdir-config") {
        return Ok(());
    }
    let project = PathBuf::from(std::env::var_os("SPEC034_TOOL_PROJECT").ok_or("project")?);
    let marker = PathBuf::from(std::env::var_os("SPEC034_TOOL_MARKER").ok_or("marker")?);
    let status = ResolvedToolchain::resolve()?
        .command(&project.join("Cargo.toml"))?
        .args(["test", "--quiet", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .status()?;
    assert!(status.success());
    assert!(!marker.exists());
    Ok(())
}

#[cfg(unix)]
fn write_runner_config(root: &Path, marker: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    let cargo = root.join(".cargo");
    let runner = root.join("fake-runner.sh");
    std::fs::create_dir(&cargo)?;
    std::fs::write(&runner, format!("#!/bin/sh\ntouch '{}'\nexec \"$@\"\n", marker.display()))?;
    std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o700))?;
    std::fs::write(
        cargo.join("config.toml"),
        format!("[target.'cfg(all())']\nrunner = '{}'\n", runner.display()),
    )?;
    Ok(())
}

fn write_test_project(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(project.join("src"))?;
    std::fs::write(
        project.join("Cargo.toml"),
        b"[package]\nname='probe'\nversion='0.1.0'\nedition='2021'\n",
    )?;
    std::fs::write(project.join("src/lib.rs"), b"#[test]\nfn probe() {}\n")?;
    Ok(())
}

#[test]
fn filesystem_root_cargo_config_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::create_dir(root.path().join(".cargo"))?;
    std::fs::write(root.path().join(".cargo/config.toml"), b"[build]\n")?;

    assert!(matches!(
        reject_root_cargo_config(root.path()),
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    ));
    Ok(())
}
