use super::image_support::{generate, image_runtime, image_tools, ok_images, Images};
use super::*;

const ASK: &str = "Generate an image of a red square, 64x48 png";

#[test]
fn revising_an_image_keeps_the_goal_and_earlier_image_and_never_claims_an_edit(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let provider = Provider::new(vec![
        generate("64x48"),
        text("Here is your image."),
        generate("32x32"),
        text("Here is the smaller one."),
    ]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound(ASK))?;
    agent.process_message(inbound("approve"))?;
    // When
    let pending = output(agent.process_message(inbound("/method revise"))?);
    agent.process_message(inbound("Edit that image so it is 32x32"))?;
    let revised = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        pending.contains("Your next message revises the image from method-request-1 as version 2"),
        "{pending}"
    );
    assert!(revised.contains("Image: generated"), "{revised}");
    assert!(revised.contains("size 32x32 — met (measured 32x32); format png — met (measured png)"));
    assert!(revised.contains(
        "Version 2 revising method-request-1: earlier image media/image-generation/img-"
    ));
    assert!(revised.contains("This version is a new generation from your revised request; image edit, mask and variation are unsupported here, so an earlier image is never edited."));
    let state = methods(&agent);
    let second = &state["closed"][1];
    assert_eq!(second["selection"]["reason"], "image_revision");
    assert_eq!(second["image"]["version"], 2);
    assert_eq!(second["image"]["purpose"], ASK);
    let first_image = &state["closed"][0]["image"]["attempts"][0]["artifacts"][0]["reference"];
    assert_eq!(&second["image"]["previous"][0]["reference"], first_image);
    let claims = &second["selection"]["validation_claims"];
    assert_eq!(claims[2]["check"], "user revision");
    assert_eq!(claims[2]["state"], "not_verified");
    Ok(())
}

#[test]
fn artifact_store_commits_are_checked_through_their_media_root_reference(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let images = Images {
        provider_id: "openai_codex",
        ..ok_images()
    };
    let tools = image_tools(&media, images);
    let provider = Provider::new(vec![generate("64x48"), text("Here is your image.")]);
    let mut agent = image_runtime(dir.path(), vec![media.clone()], &tools, &provider)?;
    agent.process_message(inbound(ASK))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.contains("Image: generated — 1 image saved and checked on disk"),
        "{answer}"
    );
    let state = methods(&agent);
    let artifact = &state["closed"][0]["image"]["attempts"][0]["artifacts"][0];
    assert_eq!(artifact["committed"], true);
    let reference = artifact["reference"].as_str().unwrap_or_default();
    assert!(
        !reference.starts_with('/') && media.join(reference).is_file(),
        "{reference}"
    );
    assert!(answer.contains(&format!("Images: {reference} (png, 64x48, ")));
    Ok(())
}

#[test]
fn revise_targets_the_latest_finished_image_or_report() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let provider = Provider::new(vec![generate("64x48")]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound(ASK))?;
    agent.process_message(inbound("deny"))?;
    // When
    let pending = output(agent.process_message(inbound("/method revise"))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        pending.contains("revises the image from method-request-1 as version 2"),
        "{pending}"
    );
    assert!(
        status.contains("Next-request override: revision of the image from method-request-1"),
        "{status}"
    );
    Ok(())
}

#[test]
fn revise_promises_nothing_before_the_new_image_exists() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let provider = Provider::new(vec![generate("64x48")]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound(ASK))?;
    agent.process_message(inbound("deny"))?;
    // When
    let pending = output(agent.process_message(inbound("/method revise"))?);
    // Then
    assert!(!pending.contains("A new image is generated"), "{pending}");
    assert!(pending.contains("Nothing is generated yet"), "{pending}");
    assert!(
        pending.contains("method-request-1 has no saved image to link"),
        "{pending}"
    );
    Ok(())
}
