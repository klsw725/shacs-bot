#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaProjectionView {
    lines: Vec<String>,
}

impl MediaProjectionView {
    pub fn unavailable() -> Self {
        Self {
            lines: vec![
                "media: state=unavailable reason=unavailable freshness=unavailable".to_owned(),
                "media lineage: unavailable".to_owned(),
                "media disclosure: unavailable".to_owned(),
            ],
        }
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }
}
