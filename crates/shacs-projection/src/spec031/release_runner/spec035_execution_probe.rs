use std::fs;
use std::process::Command;
use std::sync::OnceLock;

pub(super) struct Probe {
    pub(super) stdout: String,
    pub(super) manifest: String,
    pub(super) source: String,
    pub(super) lock: Vec<u8>,
}

pub(super) fn probe() -> &'static Probe {
    static PROBE: OnceLock<Probe> = OnceLock::new();
    PROBE.get_or_init(|| {
        let root = tempfile::tempdir().expect("probe project");
        let manifest = "[package]\nname=\"probe\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n[lib]\npath=\"probe.rs\"\n";
        let source = "#[test]\nfn evidence_probe() { assert_eq!(2 + 2, 4); }\n";
        fs::write(root.path().join("Cargo.toml"),manifest).expect("probe manifest");
        fs::write(root.path().join("probe.rs"),source).expect("probe test");
        let output = Command::new("cargo").args(["test","--manifest-path"]).arg(root.path().join("Cargo.toml")).args(["--lib","evidence_probe","--","--exact"]).output().expect("focused Cargo probe");
        assert!(output.status.success());
        Probe {stdout:String::from_utf8(output.stdout).expect("Cargo stdout"),manifest:manifest.to_owned(),source:source.to_owned(),lock:fs::read(root.path().join("Cargo.lock")).expect("Cargo-generated lock")}
    })
}
