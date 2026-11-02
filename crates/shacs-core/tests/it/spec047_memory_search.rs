#[path = "spec047_memory_search/evaluation.rs"]
mod evaluation;
#[path = "spec047_memory_search/failures.rs"]
mod failures;
#[path = "spec047_memory_search/limits.rs"]
mod limits;
#[path = "spec047_memory_search/ranking.rs"]
mod ranking;
#[path = "spec047_memory_search/support.rs"]
mod support;

#[test]
#[ignore = "needs the default model installed; PRD 010 decision 12 gives the command"]
fn real_model() -> support::TestResult {
    evaluation::real_model()
}
