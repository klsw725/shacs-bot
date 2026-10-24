use super::image_support::{generate, image_runtime, image_tools, ok_images};
use super::*;

/// Long enough, and asking for a review, to be admitted as an automatic
/// workflow if it were not an image request.
const LONG_REVIEW: &str = "Generate an image of a red square, 64x48 png, then review it so the colors suit the quarterly slide deck";

#[test]
fn a_long_review_style_image_request_stays_on_the_checked_image_path() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let claim = || text("The blue cat is ready.");
    let provider = Provider::new(vec![generate("64x48"), claim(), claim(), claim(), claim()]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    // When
    let prompt = output(agent.process_message(inbound(LONG_REVIEW))?);
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(prompt.contains("Permission approval required"), "{prompt}");
    assert!(
        answer.starts_with("Image saved and checked on disk (see below)."),
        "{answer}"
    );
    assert!(answer.contains("Method: image_generation_review (automatic, revision 1)"));
    let history = saved(&agent)["messages"].to_string();
    for shown in [&prompt, &answer, &history] {
        assert!(!shown.contains("blue cat"), "{shown}");
        assert!(!shown.contains("Workflow completed"), "{shown}");
    }
    Ok(())
}
