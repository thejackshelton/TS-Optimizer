/// Resolves entry field based on strategy type.
use super::types::EntryStrategy;

/// Resolve the entry field for a segment given the strategy.
pub fn resolve_entry_field(
    strategy: &Option<EntryStrategy>,
    _segment_name: &str,
    _ctx_name: &str,
) -> Option<String> {
    match strategy {
        Some(EntryStrategy::Inline) | Some(EntryStrategy::Hoist) => None,
        _ => None, // TODO: implement per-strategy entry resolution
    }
}
