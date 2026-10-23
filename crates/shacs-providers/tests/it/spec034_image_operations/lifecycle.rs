use shacs_providers::{
    GeneratedImage, ImageGenerationResult, ImageLifecycleError, ImageOperationLifecycle,
    ImageOperationLifecycleState, ImageOperationResult, ProviderMediaCandidateId,
    ProviderMediaLifecycleObservation,
};
use std::error::Error;

#[test]
fn lifecycle_accepts_started_partial_final_sequence() -> Result<(), Box<dyn Error>> {
    let candidate_id = ProviderMediaCandidateId::new("image-1")?;
    let mut lifecycle = ImageOperationLifecycle::new();

    lifecycle.apply(&ProviderMediaLifecycleObservation::started(
        candidate_id.clone(),
    ))?;
    lifecycle.apply(&ProviderMediaLifecycleObservation::partial(
        candidate_id.clone(),
        1,
    ))?;
    lifecycle.apply(&ProviderMediaLifecycleObservation::final_candidate(
        candidate_id,
        2,
    ))?;

    if lifecycle.state() != ImageOperationLifecycleState::Final {
        return Err(format!("unexpected lifecycle state: {:?}", lifecycle.state()).into());
    }
    lifecycle.finalize(final_result())?;
    Ok(())
}

#[test]
fn partial_cannot_finalize_without_final_event() -> Result<(), Box<dyn Error>> {
    let candidate_id = ProviderMediaCandidateId::new("image-1")?;
    let mut lifecycle = ImageOperationLifecycle::new();
    lifecycle.apply(&ProviderMediaLifecycleObservation::started(
        candidate_id.clone(),
    ))?;
    lifecycle.apply(&ProviderMediaLifecycleObservation::partial(candidate_id, 1))?;

    match lifecycle.finalize(final_result()) {
        Err(ImageLifecycleError::FinalEventRequired) => Ok(()),
        other => Err(format!("partial lifecycle finalized: {other:?}").into()),
    }
}

#[test]
fn lifecycle_rejects_final_before_start_and_post_cancel_transition() -> Result<(), Box<dyn Error>> {
    let candidate_id = ProviderMediaCandidateId::new("image-1")?;
    let mut lifecycle = ImageOperationLifecycle::new();
    match lifecycle.apply(&ProviderMediaLifecycleObservation::final_candidate(
        candidate_id.clone(),
        1,
    )) {
        Err(ImageLifecycleError::InvalidTransition { .. }) => {}
        other => return Err(format!("final-before-start accepted: {other:?}").into()),
    }

    lifecycle.apply(&ProviderMediaLifecycleObservation::started(
        candidate_id.clone(),
    ))?;
    lifecycle.apply(&ProviderMediaLifecycleObservation::cancelled(
        candidate_id.clone(),
        None,
    ))?;
    if lifecycle.state() != ImageOperationLifecycleState::Cancelled {
        return Err("cancel did not become terminal".into());
    }
    match lifecycle.apply(&ProviderMediaLifecycleObservation::partial(candidate_id, 2)) {
        Err(ImageLifecycleError::InvalidTransition { .. }) => Ok(()),
        other => Err(format!("post-cancel partial accepted: {other:?}").into()),
    }
}

#[test]
fn lifecycle_accepts_failed_terminal_transition() -> Result<(), Box<dyn Error>> {
    let candidate_id = ProviderMediaCandidateId::new("image-1")?;
    let mut lifecycle = ImageOperationLifecycle::new();
    lifecycle.apply(&ProviderMediaLifecycleObservation::started(
        candidate_id.clone(),
    ))?;
    lifecycle.apply(&ProviderMediaLifecycleObservation::failed(
        candidate_id,
        Some(1),
    ))?;
    if lifecycle.state() != ImageOperationLifecycleState::Failed {
        return Err("failed event did not become terminal".into());
    }
    Ok(())
}

#[test]
fn lifecycle_accepts_cancellation_before_provider_start() -> Result<(), Box<dyn Error>> {
    let candidate_id = ProviderMediaCandidateId::new("image-cancelled")?;
    let mut lifecycle = ImageOperationLifecycle::new();
    lifecycle.apply(&ProviderMediaLifecycleObservation::cancelled(
        candidate_id,
        None,
    ))?;
    if lifecycle.state() != ImageOperationLifecycleState::Cancelled {
        return Err("pre-start cancellation did not become terminal".into());
    }
    Ok(())
}

#[test]
fn lifecycle_transition_matrix_covers_all_nonterminal_states() -> Result<(), Box<dyn Error>> {
    for (prefix_partial, terminal, expected) in [
        (false, "partial", ImageOperationLifecycleState::Partial),
        (false, "final", ImageOperationLifecycleState::Final),
        (false, "failed", ImageOperationLifecycleState::Failed),
        (false, "cancelled", ImageOperationLifecycleState::Cancelled),
        (true, "partial", ImageOperationLifecycleState::Partial),
        (true, "final", ImageOperationLifecycleState::Final),
        (true, "failed", ImageOperationLifecycleState::Failed),
        (true, "cancelled", ImageOperationLifecycleState::Cancelled),
    ] {
        let candidate_id = ProviderMediaCandidateId::new("image-matrix")?;
        let mut lifecycle = ImageOperationLifecycle::new();
        lifecycle.apply(&ProviderMediaLifecycleObservation::started(
            candidate_id.clone(),
        ))?;
        if prefix_partial {
            lifecycle.apply(&ProviderMediaLifecycleObservation::partial(
                candidate_id.clone(),
                1,
            ))?;
        }
        let sequence = u32::from(prefix_partial) + 1;
        let event = match terminal {
            "partial" => ProviderMediaLifecycleObservation::partial(candidate_id, sequence),
            "final" => ProviderMediaLifecycleObservation::final_candidate(candidate_id, sequence),
            "failed" => ProviderMediaLifecycleObservation::failed(candidate_id, Some(sequence)),
            "cancelled" => {
                ProviderMediaLifecycleObservation::cancelled(candidate_id, Some(sequence))
            }
            other => return Err(format!("unknown matrix event: {other}").into()),
        };
        lifecycle.apply(&event)?;
        if lifecycle.state() != expected {
            return Err(format!(
                "transition matrix drifted: partial={prefix_partial} event={terminal} state={:?}",
                lifecycle.state()
            )
            .into());
        }
    }
    Ok(())
}

fn final_result() -> ImageOperationResult {
    ImageOperationResult::Edit(ImageGenerationResult {
        provider_id: "openai".to_owned(),
        model: "gpt-image-2".to_owned(),
        images: vec![GeneratedImage {
            index: 0,
            mime_type: shacs_providers::ImageMimeType::Png,
            bytes: vec![1],
            byte_len: 1,
            revised_prompt: None,
            provider_item_id: None,
        }],
        remote_images: Vec::new(),
        usage: None,
        request_id: None,
    })
}
