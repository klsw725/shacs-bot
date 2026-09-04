use super::artifacts::digest_bytes;
use super::model::{PortableToolIdentity, Spec034ReleaseArtifactError};
use super::path_chain::PathChainSeal;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) mod control;
use control::controlled_temp_root;
use control::ControlLease;
mod cache_key;
use cache_key::rust_tool_candidates;
pub(super) use cache_key::tool_cache_key;
mod cache;
mod execution_context;
use execution_context::ToolchainExecutionContext;
mod monitor;
use monitor::ExecutionLedger;
pub(super) mod linker;
mod binding;
pub(super) use binding::RetiredToolchain;
mod support;
use support::{minimal_command, monitor_paths, reject_root_cargo_config, set_read_only_closure};
#[cfg(test)]
pub(super) use support::release_tempdir;
pub(super) mod spawn;
#[cfg(not(test))]
mod vendor;
mod dependencies;
mod runtime_libraries;
#[path = "tools/resolved.rs"]
mod resolved;

const MAX_TOOL_BYTES: u64 = 128 * 1024 * 1024;

pub struct ResolvedTool {
    path: PathBuf,
    identity: PortableToolIdentity,
    seal: PathChainSeal,
    runtime_seals: Vec<PathChainSeal>,
    runtime_inventory: Vec<PathBuf>,
    _root: Option<tempfile::TempDir>,
    _control: Option<ControlLease>,
}

pub struct ResolvedToolchain {
    cargo: ResolvedTool,
    rustc: ResolvedTool,
    rustdoc: ResolvedTool,
    home: PathBuf,
    cargo_home: PathBuf,
    target: PathBuf,
    execution: ToolchainExecutionContext,
    cache_binding: Option<cache::CacheBinding>,
    #[cfg(not(test))]
    vendor_binding: Option<vendor::VendorBinding>,
    ledger: ExecutionLedger,
    linker_receipts: linker::LinkerReceipts,
    linker_path: PathBuf,
    linker_seals: Vec<PathChainSeal>,
    _root: Option<tempfile::TempDir>,
    _control: Option<ControlLease>,
}

impl ResolvedToolchain {
    pub fn resolve() -> Result<Self, Spec034ReleaseArtifactError> {
        let (control, root) = controlled_temp_root()?;
        let home = root.path().join("home");
        let cargo_home = root.path().join("cargo-home");
        let target = root.path().join("target");
        for path in [&home, &cargo_home, &target] {
            std::fs::create_dir(path).map_err(Spec034ReleaseArtifactError::Io)?;
        }
        let tools = root.path().join("toolchain/tools");
        let cache_tools = root.path().join("cache/toolchain/tools");
        let mut toolchain =
            Self::resolve_at(home, cargo_home, target, tools, cache_tools, None, None)?;
        toolchain._root = Some(root);
        toolchain._control = Some(control);
        Ok(toolchain)
    }

    pub(super) fn resolve_at(
        home: PathBuf,
        cargo_home: PathBuf,
        target: PathBuf,
        tools: PathBuf,
        cache_tools: PathBuf,
        manifest: Option<&Path>,
        linker_image: Option<&Path>,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let (mut cargo, mut rustc, mut rustdoc, cache_binding) =
            cache::resolve_into(&cache_tools, &tools)?;
        let (linker_path, _, linker_identity) = linker::prepare_wrapper(&tools, linker_image)?;
        let fixed_linker = linker::fixed_linker()?;
        set_read_only_closure(
            tools
                .parent()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?,
        )?;
        let linker_seals = vec![
            PathChainSeal::capture_digest_leaf(&linker_path)?,
            PathChainSeal::capture_digest_leaf(&fixed_linker)?,
        ];
        cargo.reseal()?;
        rustc.reseal()?;
        rustdoc.reseal()?;
        #[cfg(not(test))]
        let temporary_ledger = ExecutionLedger::arm(&monitor_paths(
            tools
                .parent()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?,
        )?)?;
        #[cfg(not(test))]
        let vendor = manifest
            .map(|manifest| vendor::prepare(&cargo.path, manifest, &home, &temporary_ledger))
            .transpose()?;
        #[cfg(test)]
        let vendor: Option<(PathBuf, ())> = {
            let _ = manifest;
            None
        };
        let mut monitored = monitor_paths(
            tools
                .parent()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?,
        )?;
        monitored.push(fixed_linker);
        if let Some((path, _)) = &vendor {
            monitored.extend(monitor_paths(path)?);
        }
        let ledger = ExecutionLedger::arm(&monitored)?;
        let mut toolchain =
            Self::from_resolved(
                (home, cargo_home, target),
                (cargo, rustc, rustdoc),
                ledger,
                vendor.as_ref().map(|(path, _)| path.as_path()),
                linker_path,
                linker_seals,
                linker_identity,
            )?;
        toolchain.cache_binding = Some(cache_binding);
        #[cfg(not(test))]
        {
            toolchain.vendor_binding = vendor.map(|(_, binding)| binding);
        }
        Ok(toolchain)
    }

    fn from_resolved(
        paths: (PathBuf, PathBuf, PathBuf),
        tools: (ResolvedTool, ResolvedTool, ResolvedTool),
        ledger: ExecutionLedger,
        vendor: Option<&Path>,
        linker_path: PathBuf,
        linker_seals: Vec<PathChainSeal>,
        linker_identity: spawn::ProcessIdentity,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let (home, cargo_home, target) = paths;
        let (cargo, rustc, rustdoc) = tools;
        let execution = ToolchainExecutionContext::prepare(&home, &cargo_home, &target, vendor)?;
        #[cfg(not(test))]
        let compiler_identity = spawn::capture_static_identity(&rustc.path)?;
        #[cfg(test)]
        let compiler_identity = spawn::capture_process_identity(
            i32::try_from(std::process::id())
                .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
        )?;
        let linker_receipts =
            linker::LinkerReceipts::prepare(&target, linker_identity, compiler_identity)?;
        Ok(Self {
            cargo,
            rustc,
            rustdoc,
            home,
            cargo_home,
            target,
            execution,
            cache_binding: None,
            #[cfg(not(test))]
            vendor_binding: None,
            ledger,
            linker_receipts,
            linker_path,
            linker_seals,
            _root: None,
            _control: None,
        })
    }

    pub fn command(
        &self,
        manifest: &Path,
    ) -> Result<Command, Spec034ReleaseArtifactError> {
        self.verify()?;
        let manifest = manifest
            .canonicalize()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let root = manifest
            .ancestors()
            .last()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        reject_root_cargo_config(root)?;
        #[cfg(test)]
        let _ = &self.linker_path;
        let mut command = minimal_command(&self.cargo.path, root);
        command
            .env("HOME", &self.home)
            .env("CARGO_HOME", &self.cargo_home)
            .env("CARGO_INCREMENTAL", "0")
            .env("CARGO_TARGET_DIR", &self.target)
            .env("RUSTC", &self.rustc.path)
            .env("RUSTDOC", &self.rustdoc.path)
            .env("RUSTC_WRAPPER", "")
            .env("RUSTC_WORKSPACE_WRAPPER", "")
            .env("CARGO_BUILD_RUSTC_WRAPPER", "")
            .env("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER", "");
        #[cfg(all(not(test), target_vendor = "apple", target_arch = "aarch64"))]
        command.env("CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER", &self.linker_path);
        #[cfg(all(not(test), target_vendor = "apple", target_arch = "x86_64"))]
        command.env("CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER", &self.linker_path);
        #[cfg(not(test))]
        {
            command.env("SHACS_SPEC034_TARGET", &self.target);
            self.linker_receipts.configure(&mut command);
        }
        Ok(command)
    }

    #[cfg(not(test))]
    pub(super) fn spawn_cargo(
        &self,
        command: &Command,
        stdout: &File,
        stderr: &File,
    ) -> Result<spawn::ExecutionChild, Spec034ReleaseArtifactError> {
        self.verify()?;
        spawn::spawn_verified(command, stdout, stderr, &self.ledger)
    }

}

#[cfg(test)]
#[path = "tools_test.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "tools_path_chain_test.rs"]
mod path_chain_tests;

#[cfg(all(test, unix))]
#[path = "tools_config_test.rs"]
mod config_tests;

#[cfg(test)]
#[path = "tools/test_support.rs"]
mod test_support;

#[cfg(all(test, target_vendor = "apple"))]
#[path = "tools/monitor_test.rs"]
mod monitor_tests;
