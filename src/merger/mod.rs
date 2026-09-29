//! Merge execution: ffmpeg invocation, retry/timeout/interrupt handling,
//! output validation, source deletion, and the merge report.

mod delete;
mod merge;
mod runner;
mod summary;

use crate::cli::OutputFormat;
use crate::scanner::safe_stem;
use std::path::{Path, PathBuf};

pub use delete::delete_source_files;
pub use merge::{merge_pair, MergeOptions};
pub use summary::{MergeResult, MergeSummary};

/// File name of the merged output for `stem`: its composite key flattened into a
/// single filename, carrying the format's extension.
///
/// Single source of truth for the output-name rule, shared by the merger and by
/// the previews that show the name before the file exists.
pub fn output_file_name(stem: &str, format: OutputFormat) -> String {
    format!("{}.{}", safe_stem(stem), format.extension())
}

/// Path of the merged output for `stem` inside `output_dir`.
///
/// Used by the merger and the workflow (resume anchors, interrupt cleanup,
/// `--once` error paths).
pub fn output_path(output_dir: &Path, stem: &str, format: OutputFormat) -> PathBuf {
    output_dir.join(output_file_name(stem, format))
}
