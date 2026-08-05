#[path = "spec031_readiness/cases.rs"]
mod spec031_readiness_cases;
#[path = "spec031_readiness/support.rs"]
mod spec031_readiness_support;

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&spec031_readiness_cases::evidence_json()?)?
    );
    Ok(())
}
