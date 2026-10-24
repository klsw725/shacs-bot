use super::*;
use crate::runtime::task_method_store::MethodStore;
use crate::tools::ToolRegistry;

#[test]
fn projection_stays_within_its_limit_including_the_calculation_protocol() -> std::io::Result<()> {
    // Given
    let mut store = MethodStore::default();
    store.start("Calculate the floor area", &ToolRegistry::new())?;
    let request = store.active.as_mut().ok_or(std::io::ErrorKind::NotFound)?;
    for index in 0..128 {
        request
            .remaining_questions
            .push(format!("question {index}: {}", "x".repeat(400)));
    }
    // When
    let text = projection(request);
    // Then
    assert!(text.chars().count() <= 8192, "{}", text.chars().count());
    assert!(text.ends_with("</task-method>"));
    assert!(text.contains("Calculation protocol"));
    Ok(())
}
