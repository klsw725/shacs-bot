#[path = "spec034_sequential_integration_fixture/scenario.rs"]
mod scenario;

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let report = scenario::run()?;
    if !report.is_complete() {
        return Err("Spec034 sequential fixture was incomplete".into());
    }
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
