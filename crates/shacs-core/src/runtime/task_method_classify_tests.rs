use super::*;

#[test]
fn explicit_korean_and_english_intents_select_their_method() {
    // Given
    let cases = [
        (
            "Calculate 20 percent of 80",
            MethodId::ReproducibleCalculation,
        ),
        (
            "80의 20퍼센트를 계산해 줘",
            MethodId::ReproducibleCalculation,
        ),
        (
            "Write a report with citations",
            MethodId::SourceGroundedReport,
        ),
        (
            "자료에 근거한 보고서를 작성해",
            MethodId::SourceGroundedReport,
        ),
        (
            "Generate an image of a tree",
            MethodId::ImageGenerationReview,
        ),
        ("나무 이미지를 생성해", MethodId::ImageGenerationReview),
    ];
    for (input, expected) in cases {
        // When
        let (method, _) = classify(input);
        // Then
        assert_eq!(method, expected);
    }
}

#[test]
fn ambiguous_requests_remain_general() {
    // Given
    for input in [
        "hello",
        "calculation or report?",
        "계산 방법을 설명해",
        "이미지 관련 질문",
        "report a bug",
    ] {
        // When
        let result = classify(input);
        // Then
        assert_eq!(result, (MethodId::General, Reason::NoSpecializedIntent));
    }
}

struct Named(&'static str);
impl crate::tools::Tool for Named {
    fn name(&self) -> &str {
        self.0
    }
    fn description(&self) -> &str {
        "Registered tool fixture"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    fn execute(&self, _: crate::tools::JsonMap) -> crate::tools::ToolResult {
        "ok".into()
    }
}

#[test]
fn several_specialized_intents_compose_and_block_only_when_no_part_is_available() {
    // Given
    let request = "Calculate 10 percent and write a report with sources";
    let none = ToolRegistry::new();
    let mut reader = ToolRegistry::new();
    reader.register(Named("read_file"));
    // When
    let (method, reason) = classify(request);
    let parts = intents(request);
    let blocked = composed_availability(&parts, &none);
    let partial = composed_availability(&parts, &reader);
    // Then
    assert_eq!(
        (method, reason),
        (MethodId::Composed, Reason::ComposedIntent)
    );
    assert_eq!(
        parts,
        [
            MethodId::ReproducibleCalculation,
            MethodId::SourceGroundedReport
        ]
    );
    assert_eq!(blocked.support, Support::Supported);
    assert!(blocked.blocked());
    assert!(!partial.blocked());
    assert_eq!(partial.readiness, Readiness::Available);
    assert_eq!(partial.missing_required_tools, ["exec"]);
    assert!(availability(MethodId::Composed, &reader).blocked());
}

#[test]
fn image_and_calculation_are_unavailable_without_their_registered_tools() {
    // Given
    let tools = ToolRegistry::new();
    for (method, missing) in [
        (MethodId::ReproducibleCalculation, "exec"),
        (MethodId::ImageGenerationReview, "image_generate"),
    ] {
        // When
        let available = availability(method, &tools);
        // Then
        assert_eq!(available.readiness, Readiness::Unavailable);
        assert_eq!(available.missing_required_tools, vec![missing.to_owned()]);
    }
}
