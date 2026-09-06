mod invocation;
mod staging;
mod supervisor;

pub use invocation::{AnalyzerInvocation, AnalyzerMediaProvenance};
pub(crate) use supervisor::{
    run_supervised_video_analyzer, SupervisedVideoAnalyzer, SupervisedVideoAnalyzerOutcome,
};
