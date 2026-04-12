/// Batch runner for snapshot tests.
///
/// Loads all .snap files from the match-these-snaps directory and runs
/// the optimizer on each, comparing output.
use std::path::Path;

use super::snapshot_parser::{parse_snapshot, SnapshotTestCase};

/// Load all snapshot test cases from a directory
pub fn load_snapshots(dir: &Path) -> Vec<SnapshotTestCase> {
    let mut cases = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "snap") {
                let name = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if let Ok(content) = std::fs::read_to_string(&path) {
                    cases.push(parse_snapshot(&name, &content));
                }
            }
        }
    }

    cases.sort_by(|a, b| a.name.cmp(&b.name));
    cases
}
