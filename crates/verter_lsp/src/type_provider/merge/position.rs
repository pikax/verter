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
///
/// ## Encoding contract
///
/// A `CodeTransform` source map indexes its generated and source positions in
/// **UTF-16** code units (the source-map column space), independent of the
/// LSP-negotiated encoding. The mapper ([`ProviderPositionMapper`]) therefore
/// consumes and produces UTF-16 columns. For the source map to be queried
/// correctly, `tsx_line_index` and `carrier_line_index` MUST be built in
/// **UTF-16** (so a provider byte offset converts to a UTF-16 column the map
/// understands, and the mapped carrier UTF-16 column validates against the same
/// space) — NOT in the negotiated encoding.
///
/// `carrier_negotiated_line_index` is the SAME carrier source measured in the
/// **negotiated** LSP encoding. When present, an API-surface mapping re-emits
/// the mapped UTF-16 carrier range into the negotiated encoding (the byte
/// offset is the encoding-neutral bridge) before returning it as an LSP edit
/// range — so a prop after non-ASCII text lands on the correct carrier range
/// under a UTF-8-negotiated session. `None` preserves the legacy IDE path
/// (which is encoding-correct only under the default UTF-16 negotiation).
pub struct ExternalIdeContext {
    pub tsx_line_index: LineIndex,
    pub mapper: ProviderPositionMapper,
    pub carrier_line_index: LineIndex,
    /// The carrier source re-measured in the negotiated LSP encoding, for the
    /// final UTF-16→negotiated edit-range conversion. `None` on the legacy IDE
    /// path; `Some` on the carrier-API rename path.
    pub carrier_negotiated_line_index: Option<LineIndex>,
}

/// Resolver for looking up IDE context by IDE path (e.g., `/path/to/Comp.vue.tsx`).
///
/// Returns `None` if the file isn't tracked or hasn't been compiled yet.
pub type ExternalIdeResolver<'a> = &'a dyn Fn(&str) -> Option<ExternalIdeContext>;

/// The 3-state outcome of resolving a carrier PUBLIC-API surface (`{carrier}.ts`)
/// for the rename merge. This is the fail-closed CLASS distinction a bare
/// `Option<ExternalIdeContext>` could not express: a returned `None` conflated a
/// genuinely-not-virtual path with a known-virtual-but-unmappable one, and the
/// merge then mis-routed the latter into a real same-named file.
///
/// A returned `{carrier}.ts` location's provider offsets index either the synced
/// VIRTUAL surface content OR a real same-named file — never both. The resolver
/// (which alone knows the captured in-flight virtual-surface set and its
/// generations) classifies which, and the merge routes accordingly:
///
/// - [`Vouched`](Self::Vouched): the path IS the currently-pinned virtual API
///   surface and its offsets map through the carried source map → map onto the
///   `.vue` carrier (a vouched-but-range-unmappable hop still drops).
/// - [`VirtualDrop`](Self::VirtualDrop): the path WAS a captured virtual surface
///   but can no longer be mapped (its generation was superseded/retired after
///   capture, or its snapshot carried no source map). Its offsets index VIRTUAL
///   generated content, so the merge FAILS CLOSED (drops) — it must NEVER fall
///   through to the real-file branch and edit a same-named real file with
///   virtual offsets (that is the corruption this variant prevents).
/// - [`NotVirtual`](Self::NotVirtual): the path was NEVER a captured virtual
///   surface. Its offsets index that path's OWN real file (a stale surface, or a
///   hand-written `Child.vue.ts` next to `Child.vue`) → the merge falls through to
///   the real-file branch and edits it in place.
///
/// Not `Debug` — [`ExternalIdeContext`] (held by `Vouched`) is not `Debug`.
pub enum ApiSurfaceResolution {
    /// The currently-pinned virtual API surface; map its offsets onto the carrier.
    Vouched(ExternalIdeContext),
    /// A known virtual surface that can no longer be mapped → fail closed (drop).
    VirtualDrop,
    /// Not a virtual surface; its offsets index its own real file → edit in place.
    NotVirtual,
}

/// Resolver for classifying a carrier PUBLIC-API surface path into the 3-state
/// [`ApiSurfaceResolution`]. The merge consults it (gated by the suffix predicate
/// `is_carrier_api_path`) to decide whether a returned `{carrier}.ts` rename
/// location maps onto the `.vue`, drops, or edits its own real file.
pub type ExternalApiResolver<'a> = &'a dyn Fn(&str) -> ApiSurfaceResolution;

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
    // Guard 3: at-cursor anchor. The run whose source extent ends exactly AT the cursor column
    // includes the member operator as its last source content; its generated endpoint is the
    // boundary just past the generated operator. Accept only on generated-suffix agreement.
    if let Some(anchor) = mapper.mapped_run_ending_at_src(position.line, cursor_col_utf16) {
        if let Some(anchor_offset) =
            ts_position_utf16_to_byte_offset(&anchor, tsx_line_index, tsx_code)
        {
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
    let anchor_offset = ts_position_utf16_to_byte_offset(&anchor, tsx_line_index, tsx_code)?;
    let generated_after = tsx_code.get(anchor_offset as usize..)?;
    if !generated_after.starts_with(op_str) {
        return None;
    }
    Some(anchor_offset + op_len)
}

fn ts_position_utf16_to_byte_offset(
    position: &TsPosition,
    tsx_line_index: &LineIndex,
    tsx_code: &str,
) -> Option<u32> {
    let line_start = tsx_line_index.line_start(position.line as usize)?;
    let line_end = tsx_line_index.line_end(position.line as usize)?;
    let line_text = tsx_code.get(line_start as usize..line_end as usize)?;
    let mut utf16_remaining = position.character;
    let mut byte_col = 0u32;
    for character in line_text.chars() {
        if utf16_remaining == 0 {
            break;
        }
        let units = character.len_utf16() as u32;
        if units > utf16_remaining {
            return None;
        }
        utf16_remaining -= units;
        byte_col += character.len_utf8() as u32;
    }
    if utf16_remaining > 0 {
        return None;
    }
    Some(line_start + byte_col)
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

/// Map a carrier PUBLIC-API surface byte-offset range back to a carrier-source
/// LSP [`Range`] in the NEGOTIATED encoding, with the encoding boundary made
/// explicit and correct.
///
/// The provider's `start`/`end` are byte offsets into the synced `{carrier}.ts`
/// API content. The API surface's `CodeTransform` source map indexes everything
/// in UTF-16 (the source-map column space), so:
///
/// 1. The source-map lookup runs ENTIRELY in UTF-16: `api_utf16_line_index` and
///    `carrier_utf16_line_index` are both built in UTF-16, so the byte offset →
///    UTF-16 column → mapper → UTF-16 carrier column chain stays in one space.
///    This yields the mapped carrier range in UTF-16 columns.
/// 2. The UTF-16 carrier range is then re-emitted in the negotiated encoding:
///    each UTF-16 carrier position converts to a byte offset (via the UTF-16
///    carrier index) and back to a negotiated column (via the negotiated carrier
///    index). The byte offset is the encoding-neutral bridge, so a prop after
///    non-ASCII carrier text maps to the CORRECT range under any negotiated
///    encoding (UTF-8 / UTF-16 / UTF-32).
///
/// Returns `None` (FAIL-CLOSED) when any step fails. A wrong rename edit range
/// would corrupt the user's `.vue`, so on ANY encoding-conversion uncertainty
/// the edit is dropped, never emitted at a guessed range.
///
// TODO(follow-up): the underlying `verter_type_runtime` codec position conversions
// (`position_to_offset` / `offset_to_position`) can fail OPEN — clamping or rounding
// a past-EOL or mid-codepoint column to a valid offset rather than returning `None`.
// This function's fail-closed guarantee therefore relies on the rename data flow never
// constructing such a column (the source map's UTF-16 columns come from real token
// positions). A defensive bounds-check here, or hardening the codec to return `None`
// on an out-of-range/mid-codepoint column, is tracked separately; do NOT change codec
// behavior from here.
pub fn api_surface_range_to_carrier_range(
    api_start: u32,
    api_end: u32,
    api_utf16_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_utf16_line_index: &LineIndex,
    carrier_negotiated_line_index: &LineIndex,
) -> Option<Range> {
    // Step 1: source-map lookup entirely in UTF-16 → UTF-16 carrier range.
    let utf16_range = tsx_range_to_carrier_range(
        api_start,
        api_end,
        api_utf16_line_index,
        mapper,
        carrier_utf16_line_index,
    )?;

    // Step 2: re-emit the UTF-16 carrier range in the negotiated encoding via a
    // byte-offset round-trip over the SAME carrier source.
    let reencode = |utf16_pos: Position| -> Option<Position> {
        let byte_offset = carrier_utf16_line_index.position_to_offset(&utf16_pos)?;
        carrier_negotiated_line_index.offset_to_position(byte_offset)
    };

    Some(Range {
        start: reencode(utf16_range.start)?,
        end: reencode(utf16_range.end)?,
    })
}
