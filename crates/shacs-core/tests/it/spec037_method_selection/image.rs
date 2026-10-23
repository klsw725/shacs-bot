use super::image_support::{generate, image_runtime, image_tools, ok_images, Images};
use super::*;

#[test]
fn a_generated_image_is_saved_measured_and_left_for_the_user_to_review(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let provider = Provider::new(vec![generate("64x48"), text("The blue cat is ready.")]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    let prompt =
        output(agent.process_message(inbound("Generate an image of a red square, 64x48 png"))?);
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(prompt.contains("Permission approval required"), "{prompt}");
    assert!(
        answer.starts_with("Image saved and checked on disk (see below). Content and look are not checked automatically; please review the image yourself. The model's own reply is not shown"),
        "{answer}"
    );
    assert!(!answer.contains("blue cat") && !status.contains("blue cat"));
    let history = saved(&agent)["messages"].to_string();
    assert!(!history.contains("blue cat"), "{history}");
    assert!(history.contains("Image saved and checked on disk"));
    assert!(
        answer.contains("Image: generated — 1 image saved and checked on disk"),
        "{answer}"
    );
    assert!(answer.contains("Images: media/image-generation/img-"));
    assert!(answer.contains("(png, 64x48, "));
    assert!(answer.contains("from openai/test-image-model)"));
    assert!(answer.contains("Requirements: size 64x48 — met (measured 64x48); format png — met (measured png); content and look — not checked automatically; please review the image"));
    assert!(status.contains("prompt: a red square"), "{status}");
    assert!(status.contains("saved file verified"));
    let closed = &methods(&agent)["closed"][0];
    assert_eq!(closed["image"]["withheld_reply"], "The blue cat is ready.");
    let claims = &closed["selection"]["validation_claims"];
    assert_eq!(claims[0]["state"], "verified");
    assert_eq!(claims[1]["state"], "not_verified");
    Ok(())
}

#[test]
fn a_measured_mismatch_fails_the_stated_requirement() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let images = Images {
        fixed: Some((64, 48)),
        ..ok_images()
    };
    let tools = image_tools(&media, images);
    let provider = Provider::new(vec![generate("1024x1024"), text("Done.")]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound("Generate an image of a red square, 1024x1024 png"))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.starts_with("Image saved, but not every stated requirement is met (see below)."),
        "{answer}"
    );
    assert!(!answer.contains("Done."));
    assert!(
        answer.contains("size 1024x1024 — not met (measured 64x48)"),
        "{answer}"
    );
    let claims = &methods(&agent)["closed"][0]["selection"]["validation_claims"];
    assert_eq!(claims[1]["state"], "failed");
    Ok(())
}

#[test]
fn unavailable_failed_unsaved_and_unconfirmed_images_are_never_shown_as_generated(
) -> Result<(), Box<dyn Error>> {
    let auth = || ProviderError::AuthRequired {
        provider_id: "openai".to_owned(),
    };
    let api = || ProviderError::Api {
        status: Some(500),
        message: "boom".to_owned(),
        retryable: false,
        headers: Default::default(),
        body: None,
    };
    for (error, unwritable, roots, expected) in [
        (Some(auth as fn() -> ProviderError), false, true, "Image: unavailable — image_generate did not save an image (provider or credentials unavailable)"),
        (Some(api as fn() -> ProviderError), false, true, "Image: failed — image_generate did not save an image (generation failed)"),
        (None, true, true, "Image: failed — image_generate did not save an image (image could not be saved)"),
        (None, false, false, "Image: save not confirmed"),
    ] {
        // Given
        let dir = tempfile::tempdir()?;
        let media = dir.path().join("media");
        if unwritable {
            std::fs::create_dir_all(&media)?;
            std::fs::write(media.join("image-generation"), "not a directory")?;
        }
        let tools = image_tools(&media, Images { error, ..ok_images() });
        let provider = Provider::new(vec![generate("64x48"), text("Here is your image.")]);
        let roots = if roots { vec![media] } else { Vec::new() };
        let mut agent = image_runtime(dir.path(), roots, &tools, &provider)?;
        agent.process_message(inbound("Generate an image of a red square, 64x48 png"))?;
        // When
        let answer = output(agent.process_message(inbound("approve"))?);
        // Then
        assert!(answer.contains(expected), "{answer}");
        assert!(!answer.contains("Here is your image."), "{answer}");
        assert!(answer.starts_with("No new image was saved for this request"));
        assert!(!answer.contains("Images:") && !answer.contains("Image: generated"));
        assert!(answer.contains("Requirements: not checked, because no saved image was produced"));
        let claims = &methods(&agent)["closed"][0]["selection"]["validation_claims"];
        assert_ne!(claims[0]["state"], "verified");
    }
    Ok(())
}

#[test]
fn a_denied_image_call_is_reported_as_not_generated() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let provider = Provider::new(vec![generate("64x48")]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound("Generate an image of a red square"))?;
    // When
    let denied = output(agent.process_message(inbound("deny"))?);
    // Then
    assert!(
        denied.contains("Image: not generated — image_generate did not save an image (denied): Permission denied by user."),
        "{denied}"
    );
    assert_eq!(
        methods(&agent)["closed"][0]["image"]["attempts"][0]["status"],
        "denied"
    );
    Ok(())
}

#[test]
fn a_reply_without_any_image_call_is_withheld() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let provider = Provider::new(vec![text("Here is your image.")]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound("Generate an image of a red square"))?);
    // Then
    assert!(
        answer.starts_with("No image was generated for this request (image: not generated): no image_generate call ran."),
        "{answer}"
    );
    assert!(!answer.contains("Here is your image."), "{answer}");
    assert!(answer.contains("Image: not generated — no image_generate call ran"));
    let history = saved(&agent)["messages"].to_string();
    assert!(!history.contains("Here is your image."), "{history}");
    Ok(())
}

#[test]
fn a_reply_ending_in_a_provider_error_is_withheld_too() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let failed = LlmResponse {
        content: Some("The blue cat is ready.".to_owned()),
        finish_reason: "error".to_owned(),
        ..LlmResponse::default()
    };
    let provider = Provider::new(vec![generate("64x48"), failed]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound("Generate an image of a red square, 64x48 png"))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.starts_with("Image saved and checked on disk"),
        "{answer}"
    );
    assert!(answer.contains("The turn ended early (error). The model's own reply is not shown"));
    assert!(!answer.contains("blue cat"), "{answer}");
    let history = saved(&agent)["messages"].to_string();
    assert!(!history.contains("blue cat"), "{history}");
    Ok(())
}
