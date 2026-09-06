use super::*;

#[test]
fn verified_self_image_copy_ignores_sibling_preseed() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("runner");
    let sibling = root.path().join("spec034-linker-wrapper");
    let target = root.path().join("run-local-wrapper");
    std::fs::write(&source, b"verified-running-image")?;
    std::fs::write(&sibling, b"malicious-sibling")?;

    copy_verified_self_image(&source, &target)?;

    assert_eq!(std::fs::read(target)?, b"verified-running-image");
    Ok(())
}
