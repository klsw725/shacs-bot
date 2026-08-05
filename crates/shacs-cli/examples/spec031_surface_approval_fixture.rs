use shacs_cli::spec031_surface_approval_fixture::FixtureRuntime;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let options = Options::parse(std::env::args().skip(1))?;
    let mut runtime = FixtureRuntime::start(options.config_path, options.workspace)?;
    write_json(&options.ready_file, &runtime.snapshot()?)?;
    loop {
        if options.stop_file.exists() {
            write_json(&options.state_file, &runtime.snapshot()?)?;
            runtime.stop()?;
            return Ok(());
        }
        if options.new_approval_file.exists() {
            fs::remove_file(&options.new_approval_file)?;
            let lineage = runtime.create_pending()?;
            write_json(
                &options.ack_file,
                &serde_json::json!({"new_lineage": lineage}),
            )?;
        }
        if options.replace_owner_file.exists() {
            fs::remove_file(&options.replace_owner_file)?;
            let owner_id = runtime.replace_owner_generation()?;
            write_json(
                &options.ack_file,
                &serde_json::json!({"owner_id": owner_id}),
            )?;
        }
        runtime.renew_owner()?;
        write_json(&options.state_file, &runtime.snapshot()?)?;
        thread::sleep(Duration::from_millis(50));
    }
}

fn write_json(path: &PathBuf, value: &serde_json::Value) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

struct Options {
    config_path: PathBuf,
    workspace: PathBuf,
    ready_file: PathBuf,
    state_file: PathBuf,
    ack_file: PathBuf,
    new_approval_file: PathBuf,
    replace_owner_file: PathBuf,
    stop_file: PathBuf,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, Box<dyn Error>> {
        let mut config_path = None;
        let mut workspace = None;
        let mut ready_file = None;
        let mut state_file = None;
        let mut ack_file = None;
        let mut new_approval_file = None;
        let mut replace_owner_file = None;
        let mut stop_file = None;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| format!("{arg} requires a value"))?;
            match arg.as_str() {
                "--config" => config_path = Some(PathBuf::from(value)),
                "--workspace" => workspace = Some(PathBuf::from(value)),
                "--ready-file" => ready_file = Some(PathBuf::from(value)),
                "--state-file" => state_file = Some(PathBuf::from(value)),
                "--ack-file" => ack_file = Some(PathBuf::from(value)),
                "--new-approval-file" => new_approval_file = Some(PathBuf::from(value)),
                "--replace-owner-file" => replace_owner_file = Some(PathBuf::from(value)),
                "--stop-file" => stop_file = Some(PathBuf::from(value)),
                other => return Err(format!("unknown option {other}").into()),
            }
        }
        Ok(Self {
            config_path: config_path.ok_or("missing --config")?,
            workspace: workspace.ok_or("missing --workspace")?,
            ready_file: ready_file.ok_or("missing --ready-file")?,
            state_file: state_file.ok_or("missing --state-file")?,
            ack_file: ack_file.ok_or("missing --ack-file")?,
            new_approval_file: new_approval_file.ok_or("missing --new-approval-file")?,
            replace_owner_file: replace_owner_file.ok_or("missing --replace-owner-file")?,
            stop_file: stop_file.ok_or("missing --stop-file")?,
        })
    }
}
