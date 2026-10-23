use shacs_providers::{
    LlmResponse, ProviderMediaBytes, ProviderMediaCandidate, ProviderMediaCandidateId,
    ProviderMediaOrigin,
};
use std::error::Error;

#[test]
fn llm_response_carries_nonserialized_media_candidates() -> Result<(), Box<dyn Error>> {
    // Given
    let candidate = ProviderMediaCandidate::bytes(
        ProviderMediaCandidateId::new("provider-candidate-one")?,
        ProviderMediaOrigin::new("openai", "gpt-image-2"),
        ProviderMediaBytes::new("image/png", b"raw-provider-payload".to_vec()),
    );
    let response = LlmResponse {
        media_candidates: vec![candidate],
        ..LlmResponse::default()
    };

    // When
    let serialized = serde_json::to_string(&response)?;
    let rendered = format!("{response:?}");
    let restored: LlmResponse = serde_json::from_str(&serialized)?;

    // Then
    assert_eq!(response.media_candidates.len(), 1);
    assert!(restored.media_candidates.is_empty());
    assert!(!serialized.contains("media_candidates"));
    assert!(!serialized.contains("raw-provider-payload"));
    assert!(!rendered.contains("raw-provider-payload"));
    Ok(())
}
