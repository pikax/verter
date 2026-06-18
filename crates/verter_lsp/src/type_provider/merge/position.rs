//! Position and range mapping between carrier sources and generated TSX.
//!
//! Carries the external-IDE context types and resolver aliases the merge
//! functions share, plus the carrier↔TSX offset/range mappers (including the
//! completion-only member-boundary anchor and the strict range mapper).

use std::sync::Arc;

use tower_lsp_server::ls_types::*;
use verter_span::{LspPosition, TsPosition};

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;

/// External IDE context for resolving positions in a foreign carrier IDE TSX
/// file (`Comp.vue.tsx`, `Comp.svelte.tsx`, …).
///
/// For cross-file navigation (e.g., CTRL+CLICK navigates to another carrier
/// file), the merge functions need the target file's TSX line index, position
/// mapper, and carrier-source line index. This struct carries those, and the
/// resolver closure produces it.
pub struct ExternalIdeContext {
    pub tsx_line_index: LineIndex,
    pub mapper: ProviderPositionMapper,
    pub carrier_line_index: LineIndex,
}

/// Resolver for looking up IDE context by IDE path (e.g., `/path/to/Comp.vue.tsx`).
///
/// Returns `None` if the file isn't tracked or hasn't been compiled yet.
pub type ExternalIdeResolver<'a> = &'a dyn Fn(&str) -> Option<ExternalIdeContext>;

/// Resolver for following a type-provider location through barrel re-exports.
///
/// The input is the raw provider path plus its byte-offset range in that file.
/// Returns a fully resolved LSP location when the file/range matches a known
/// re-export signature; otherwise returns `None` and merge logic keeps the
/// original provider location unchanged.
pub type BarrelResolver<'a> = &'a dyn Fn(&str, u32, u32) -> Option<Location>;

/// Reader for a definition/type-definition target's OWN source, routed through the host's
/// workspace (VFS) layer instead of direct disk I/O.
///
/// [`super::resolve_external_target_range`] converts the provider's byte offsets to line:col by
/// reading the same source those offsets index; the read goes through this closure so the
/// merge layer never touches `std::fs` directly — the workspace/VFS is the single source-read
/// authority (host cache → snapshot → disk, an open editor's overlay winning over stale disk
/// content). Returns `None` when the file cannot be read, and the caller then fails closed.
pub type ExternalSourceReader<'a> = &'a dyn Fn(&str) -> Option<Arc<str>>;

/// Map an LSP `Position` (in the carrier source file) to a byte offset in the
/// generated TSX.
///
/// Steps: LSP Position → byte offset via LineIndex → line/col → PositionMapper → TSX line/col → TSX byte offset via TSX LineIndex.
///
/// Returns `None` if any mapping step fails.
pub fn carrier_position_to_tsx_offset(
    position: &Position,
    _carrier_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    tsx_line_index: &LineIndex,
) -> Option<u32> {
    let tsx_pos = mapper
        .carrier_to_tsx(LspPosition::new(position.line, position.character))?
        .pos;
    tsx_line_index.position_to_offset(&Position {
        line: tsx_pos.line,
        character: tsx_pos.character,
    })
}

/// Map a carrier-source position to a TSX byte offset, with round-trip validation.
///
/// After mapping carrier→TSX, verifies the TSX offset maps back to the same
/// carrier-source line. Returns `None` if the round-trip fails (indicating the
/// TSX offset is in a synthetic region like generated JSX for HTML elements,
/// where TSGO queries would crash).
pub fn carrier_position_to_tsx_offset_validated(
    position: &Position,
    carrier_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    tsx_line_index: &LineIndex,
) -> Option<u32> {
    let tsx_offset =
        carrier_position_to_tsx_offset(position, carrier_line_index, mapper, tsx_line_index)?;
    if let Some(exact_offset) =
        find_exact_roundtrip_offset(position, tsx_offset, mapper, tsx_line_index)
    {
        return Some(exact_offset);
    }

    // Round-trip: TSX offset → TSX position → Vue position
    let tsx_pos = tsx_line_index.offset_to_position(tsx_offset)?;
    let carrier_roundtrip = mapper
        .tsx_to_carrier(TsPosition::new(tsx_pos.line, tsx_pos.character))?
        .pos;

    // The round-trip Vue position should be on the same line as the original.
    // If not, the TSX offset is in a synthetic region with no valid source correlation.
    if carrier_roundtrip.line == position.line {
        Some(tsx_offset)
    } else {
        None
    }
}

fn find_exact_roundtrip_offset(
    position: &Position,
    initial_offset: u32,
    mapper: &ProviderPositionMapper,
    tsx_line_index: &LineIndex,
) -> Option<u32> {
    const SEARCH_WINDOW: u32 = 256;

    let initial_pos = tsx_line_index.offset_to_position(initial_offset)?;

    let roundtrips_exact = |offset: u32| -> Option<bool> {
        let tsx_pos = tsx_line_index.offset_to_position(offset)?;
        if tsx_pos.line != initial_pos.line {
            return Some(false);
        }
        let carrier_pos = mapper
            .tsx_to_carrier(TsPosition::new(tsx_pos.line, tsx_pos.character))?
            .pos;
        Some(carrier_pos.line == position.line && carrier_pos.character == position.character)
    };

    if roundtrips_exact(initial_offset)? {
        return Some(initial_offset);
    }

    for delta in 1..=SEARCH_WINDOW {
        if initial_offset >= delta {
            let candidate = initial_offset - delta;
            if roundtrips_exact(candidate)? {
                return Some(candidate);
            }
        }

        let candidate = initial_offset + delta;
        if roundtrips_exact(candidate)? {
            return Some(candidate);
        }
    }

    None
}

/// Completion-ONLY member-boundary mapping for an incomplete member access (`obj.` / `obj?.`).
///
/// This is NOT a relaxation of the strict mappers and NOT a "fall back to the raw offset on any
/// `.` trigger". It is a precisely-guarded path used ONLY by the completion handler, AFTER
/// [`carrier_position_to_tsx_offset_validated`] has returned `None`. The strict mappers
/// ([`carrier_position_to_tsx_offset`], [`carrier_position_to_tsx_offset_validated`],
/// [`ProviderPositionMapper::carrier_to_tsx`], [`ProviderPositionMapper::tsx_to_carrier`]) keep
/// their strict in-run semantics; this helper never feeds any other feature path.
///
/// The cursor after `obj.` is a zero-width member-access boundary that sits OUTSIDE any mapped
/// run, so the strict path legitimately maps nothing. This helper anchors on a mapped run whose
/// SOURCE extent ends exactly at one of TWO same-line endpoints — the cursor itself, or the
/// position just before the operator — and accepts only when the generated TSX carries the
/// matching `.`/`?.` operator at that run's generated endpoint. Every guard is mandatory;
/// failing both anchor arms returns `None`.
///
/// Guard chain (the completion-boundary rule):
/// 1. **Validated-first / completion-only** — enforced by the caller: this runs only from
///    `handle_completion`, only when the validated strict mapper returned `None`.
/// 2. **Source PROVES incomplete member access** — the Vue source immediately before the cursor
///    must end EXACTLY with `?.` (checked first) or `.` (not merely a `.` trigger character,
///    and not a `..`/`...` suffix — a `.` preceded by another `.` rejects).
/// 3. **At-cursor anchor** — [`ProviderPositionMapper::mapped_run_ending_at_src`] at the cursor column
///    (converted to UTF-16 code units — mapped-run columns are always UTF-16, while the LSP
///    `position.character` is in the client-negotiated encoding): the run includes the trailing
///    operator as its last source content (position-preserving emission), so its source extent
///    ends AT the cursor. Accepted only when the generated text ending at the run's generated
///    endpoint ENDS WITH EXACTLY the same operator (a source `.` does not accept a generated
///    `?.`); the result is that endpoint's byte offset (immediately after the generated
///    operator).
/// 4. **Before-operator anchor** — otherwise, [`ProviderPositionMapper::mapped_run_ending_at_src`] at
///    `cursor - operator length`: the receiver run EXCLUDES the operator (relocated/planned
///    expression shapes emit the `.`/`?.` as generated content immediately after the mapped
///    endpoint). Accepted only when the generated text immediately AFTER the run's generated
///    endpoint STARTS WITH the same operator; the result is the endpoint's byte offset PLUS the
///    operator length.
/// 5. **No other lookup** — no cross-line anchor, no generated-containment lookup, no
///    nearest-preceding-run snap, no raw fallback. Both arms failing returns `None`.
///
/// Both arms demand an exact same-line source-extent-endpoint match plus source/generated
/// operator agreement. On success the returned offset is the generated byte offset immediately
/// AFTER the matched generated operator (before any trailing synthetic `}`), so a TSGO query
/// there resolves `obj`'s members.
pub(crate) fn carrier_completion_member_boundary_offset(
    position: &Position,
    carrier_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    tsx_line_index: &LineIndex,
    tsx_code: &str,
    carrier_source: &str,
) -> Option<u32> {
    // Guard 2: source PROVES incomplete member access. Inspect the Vue source bytes
    // immediately before the cursor; the suffix must be EXACTLY `?.` or `.` — a `..`/`...`
    // suffix is not a member-access boundary and rejects.
    let cursor_byte = carrier_line_index.position_to_offset(position)? as usize;
    let before = carrier_source.get(..cursor_byte)?;
    let op_str = if before.ends_with("?.") {
        "?."
    } else if let Some(stripped) = before.strip_suffix('.') {
        if stripped.ends_with('.') {
            return None;
        }
        "."
    } else {
        return None;
    };
    // The operator is ASCII, so its byte length equals its UTF-16 column width.
    let op_len = op_str.len() as u32;

    // Generated-operator agreement for the matched anchor: the generated text must carry
    // EXACTLY the source operator. For `.` that excludes a generated `?.` (a bare
    // `ends_with(".")` would also accept it); `?.` already excludes a bare `.`.
    let generated_operator_matches = |prefix: &str| -> bool {
        match op_str {
            "." => prefix.ends_with('.') && !prefix.ends_with("?."),
            _ => prefix.ends_with("?."),
        }
    };

    // Mapped-run columns are ALWAYS UTF-16 code units (the source-map column space), while
    // `position.character` is in the client-negotiated encoding (UTF-8-first). Convert the
    // cursor's source column to UTF-16 units via the byte offset guard 2 already computed:
    // the UTF-16 column is the UTF-16 length of the line's text up to the cursor byte.
    let line_start = carrier_line_index.line_start(position.line as usize)? as usize;
    let cursor_col_utf16: u32 = carrier_source
        .get(line_start..cursor_byte)?
        .chars()
        .map(|c| c.len_utf16() as u32)
        .sum();

    // The anchor returned by `mapped_run_ending_at_src` is ALSO in UTF-16 columns (the
    // generated side of the same source-map space), while `tsx_line_index` interprets
    // `Position.character` in the negotiated encoding — so converting the anchor through
    // `position_to_offset` would mis-land when non-ASCII text precedes it on the generated
    // line. Convert the UTF-16 column to a byte offset directly against the generated line
    // text (encoding-independent; a column inside a surrogate pair or past EOL rejects).
    let anchor_byte_offset = |anchor: &TsPosition| -> Option<u32> {
        let line_start = tsx_line_index.line_start(anchor.line as usize)?;
        let line_end = tsx_line_index.line_end(anchor.line as usize)?;
        let line_text = tsx_code.get(line_start as usize..line_end as usize)?;
        let mut utf16_remaining = anchor.character;
        let mut byte_col = 0u32;
        for c in line_text.chars() {
            if utf16_remaining == 0 {
                break;
            }
            let units = c.len_utf16() as u32;
            if units > utf16_remaining {
                return None;
            }
            utf16_remaining -= units;
            byte_col += c.len_utf8() as u32;
        }
        if utf16_remaining > 0 {
            return None;
        }
        Some(line_start + byte_col)
    };

    // Guard 3: at-cursor anchor. The run whose source extent ends exactly AT the cursor column
    // includes the member operator as its last source content; its generated endpoint is the
    // boundary just past the generated operator. Accept only on generated-suffix agreement.
    if let Some(anchor) = mapper.mapped_run_ending_at_src(position.line, cursor_col_utf16) {
        if let Some(anchor_offset) = anchor_byte_offset(&anchor) {
            if tsx_code
                .get(..anchor_offset as usize)
                .is_some_and(generated_operator_matches)
            {
                return Some(anchor_offset);
            }
        }
    }

    // Guard 4: before-operator anchor. The receiver run's source extent ends exactly at the
    // column just BEFORE the operator (the operator is not part of the mapped run); the
    // generated operator must sit immediately AFTER the run's generated endpoint. Accept only
    // on generated-prefix agreement, returning the endpoint plus the operator length.
    let receiver_col = cursor_col_utf16.checked_sub(op_len)?;
    let anchor = mapper.mapped_run_ending_at_src(position.line, receiver_col)?;
    let anchor_offset = anchor_byte_offset(&anchor)?;
    let generated_after = tsx_code.get(anchor_offset as usize..)?;
    if !generated_after.starts_with(op_str) {
        return None;
    }
    Some(anchor_offset + op_len)
}

/// Map a TSX byte offset range back to an LSP `Range` in the Vue source.
///
/// Routes through the mapper's strict [`PositionMapper::tsx_range_to_carrier`], which enforces
/// the half-open endpoint-compatibility rule: the range maps ONLY when both endpoints resolve
/// inside compatible mapped runs (the same run, or genuinely-contiguous runs with no
/// synthetic/unmapped content between them). A range whose endpoints fall in two runs
/// separated by synthetic content — even though each endpoint individually maps — is dropped.
///
/// Returns `None` if any mapping step fails or the endpoints are incompatible.
pub fn tsx_range_to_carrier_range(
    tsx_start: u32,
    tsx_end: u32,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
) -> Option<Range> {
    let start_pos = tsx_line_index.offset_to_position(tsx_start)?;
    let end_pos = tsx_line_index.offset_to_position(tsx_end)?;

    let (carrier_start, carrier_end) = mapper.tsx_range_to_carrier(
        TsPosition::new(start_pos.line, start_pos.character),
        TsPosition::new(end_pos.line, end_pos.character),
    )?;

    // Validate the mapped positions produce valid byte offsets
    let start_lsp = Position {
        line: carrier_start.line,
        character: carrier_start.character,
    };
    let end_lsp = Position {
        line: carrier_end.line,
        character: carrier_end.character,
    };
    carrier_line_index.position_to_offset(&start_lsp)?;
    carrier_line_index.position_to_offset(&end_lsp)?;

    Some(Range {
        start: start_lsp,
        end: end_lsp,
    })
}
