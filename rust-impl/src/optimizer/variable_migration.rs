/// Decides whether module-level declarations should be:
/// - moved into segment
/// - re-exported as _auto_VARNAME
/// - kept at root
///
/// Analyzes dependency graph between root declarations and segments.

#[derive(Debug, Clone, PartialEq)]
pub enum MigrationDecision {
    /// Keep at module root
    Keep,
    /// Move into the segment that uses it
    MoveToSegment(String),
    /// Re-export as _auto_VARNAME for segment to import
    ReExport(String),
}
