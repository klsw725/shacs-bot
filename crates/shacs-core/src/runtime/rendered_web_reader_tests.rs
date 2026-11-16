use super::{rendered_web_reader, WebFetchLimits};
use boa_engine::{Context, Source};
use std::error::Error;

#[test]
fn spec051_browser_side_expression_bounds_utf8_before_transfer() -> Result<(), Box<dyn Error>> {
    let limits = WebFetchLimits::PRODUCTION;
    let prefix = "x".repeat(limits.max_bytes - 4);
    let source = format!("{prefix}한😀뒤");
    let mut engine = Context::default();
    let setup = format!(
        "const document={{body:{{innerText:{}}}}};",
        serde_json::to_string(&source)?
    );
    engine
        .eval(Source::from_bytes(&setup))
        .map_err(|error| error.to_string())?;
    let expression = rendered_web_reader::rendered_text_expression(&limits);
    let value = engine
        .eval(Source::from_bytes(&expression))
        .map_err(|error| error.to_string())?
        .to_json(&mut engine)
        .map_err(|error| error.to_string())?
        .ok_or("no value")?;
    assert_eq!(
        value["text"].as_str().ok_or("text")?.len(),
        limits.max_bytes - 1
    );
    assert_eq!(value["text"], format!("{prefix}한"));
    assert_eq!(value["truncated"], true);
    engine
        .eval(Source::from_bytes("document.body=null;"))
        .map_err(|error| error.to_string())?;
    let empty = engine
        .eval(Source::from_bytes(&expression))
        .map_err(|error| error.to_string())?
        .to_json(&mut engine)
        .map_err(|error| error.to_string())?
        .ok_or("no empty value")?;
    assert_eq!(empty["text"], "");
    assert_eq!(empty["truncated"], false);
    Ok(())
}
