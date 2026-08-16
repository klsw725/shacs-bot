use shacs_channels::project_spec035_media_for_channel;
use shacs_projection::Spec035MediaProjection;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: spec034_surface_projection_probe <projection-json>")?;
    let projection = Spec035MediaProjection::parse_json(&std::fs::read_to_string(path)?)?;
    let channel = project_spec035_media_for_channel(projection);
    println!("{}", serde_json::to_string(channel.media_capability())?);
    Ok(())
}
