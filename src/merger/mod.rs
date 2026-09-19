//! Merge execution: ffmpeg invocation, retry/timeout/interrupt handling,
//! output validation, source deletion, and the merge report.

mod delete;
mod merge;
mod runner;
mod summary;

pub use delete::delete_source_files;
pub use merge::merge_pair;
pub use summary::{MergeResult, MergeSummary};
