use serde::Serialize;
use shacs_core::generated_media::Sha256Digest;

#[derive(Debug, Serialize)]
pub struct ScanInput {
    pub name: String,
    pub byte_len: usize,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct SecretScanReport {
    pub inputs: Vec<ScanInput>,
    pub forbidden_classes: Vec<String>,
    pub matches: Vec<String>,
}

pub fn run(outputs: &[(&str, &str)], forbidden: &[(&str, &str)]) -> SecretScanReport {
    let inputs = outputs
        .iter()
        .map(|(name, value)| ScanInput {
            name: (*name).to_owned(),
            byte_len: value.len(),
            sha256: Sha256Digest::from_bytes(value.as_bytes())
                .as_str()
                .to_owned(),
        })
        .collect();
    let forbidden_classes = forbidden
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    let mut matches = Vec::new();
    for (output_name, output) in outputs {
        for (class, needle) in forbidden {
            if output.contains(needle) {
                matches.push(format!("{output_name}:{class}"));
            }
        }
    }
    SecretScanReport {
        inputs,
        forbidden_classes,
        matches,
    }
}
