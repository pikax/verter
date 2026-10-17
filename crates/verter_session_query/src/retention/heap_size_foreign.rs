//! Heap measurement of identity types owned by crates below the retention
//! layer, for the `#[heap_size(with = "...")]` fields that carry them.

use verter_retention::allocation::{arc_block_bytes, heap_block_bytes};
use verter_retention::heap_size::HeapMeter;

/// A parse identity's retained canonical encoding.
pub fn parse_key<'t>(key: &'t verter_language::ParseKey, _meter: &mut HeapMeter<'_, 't>) -> usize {
    heap_block_bytes(key.canonical_bytes().len())
}

/// A file language row. Adapter and language ids are interned process-wide
/// and charge nothing to a holder; an owning component hint is a raw name
/// of its own.
pub fn file_language<'t>(
    language: &'t verter_language::FileLanguage,
    meter: &mut HeapMeter<'_, 't>,
) -> usize {
    match language {
        verter_language::FileLanguage::Script { .. }
        | verter_language::FileLanguage::Framework { .. } => 0,
        verter_language::FileLanguage::FrameworkTemplate { owner_hint, .. } => {
            owner_hint.as_ref().map_or(0, |hint| {
                if meter.first(hint) {
                    arc_block_bytes(hint.len())
                } else {
                    0
                }
            })
        }
    }
}
