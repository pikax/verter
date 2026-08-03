//! Test-only fixtures that mint macro analysis through the REAL analyzer.
//!
//! Macro edit anchors are minted by `verter_semantic`'s analyzer from live OXC
//! spans and are not constructible outside that crate (`MemberListAnchor`'s
//! offset field is private and its constructor is crate-private). A fixture
//! that hand-wrote an anchor could not discriminate a producer bug, so the
//! anchor-bearing cases run the real analyzer over fixture source instead.

use oxc_allocator::Allocator;
use oxc_span::SourceType;
use verter_semantic::analysis::{
    build_script_analysis_with_scope, AnalysisScope, ScriptAnalysisSnapshot,
};

use crate::documents::carrier_structure::test_carrier_blocks;

/// Blank every byte outside the SFC's `<script>` content ranges, preserving
/// line terminators and total length.
///
/// Mirrors `verter_session`'s position-preserving script projection, so every
/// OXC span the analyzer observes — and therefore every anchor it mints — is
/// SFC-ABSOLUTE, exactly as in production.
fn position_preserving_script_source(source: &str) -> String {
    let src = source.as_bytes();
    let mut out: Vec<u8> = src
        .iter()
        .map(|&b| if b == b'\n' || b == b'\r' { b } else { b' ' })
        .collect();
    for block in test_carrier_blocks(source) {
        if block.tag_name != "script" {
            continue;
        }
        let (start, end) = block.content_range();
        let (start, end) = (start as usize, end as usize);
        if start <= end && end <= src.len() {
            out[start..end].copy_from_slice(&src[start..end]);
        }
    }
    String::from_utf8(out).expect("blanking ASCII bytes preserves UTF-8")
}

/// Run the real `verter_semantic` analyzer over an SFC fixture's script blocks.
///
/// Returns the analyzer's own snapshot — real `macros`, real `slot_fields`,
/// real `edit_anchors`, all with SFC-absolute spans.
pub(crate) fn analyze_sfc_script(source: &str) -> ScriptAnalysisSnapshot {
    let script_source = position_preserving_script_source(source);
    let allocator = Allocator::new();
    build_script_analysis_with_scope(
        &script_source,
        SourceType::ts(),
        &allocator,
        AnalysisScope::all(),
    )
}
