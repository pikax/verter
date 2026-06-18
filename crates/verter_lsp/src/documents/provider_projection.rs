//! Projection-aware document → TypeProvider position mapping.
//!
//! A document's provider buffer (the bytes the TypeProvider type-checks) is
//! produced one of two ways, and each carries its own source↔provider mapper:
//!
//! - **Carrier IDE projection** — a framework carrier (`.vue` / `.svelte`)
//!   compiles to an IDE TSX file with a real source map; the mapper is the
//!   source-map-backed [`PositionMapper`].
//! - **Self-file projection** — a NON-component rune module (`.svelte.ts` /
//!   `.svelte.js`) serves its own bytes (after import-specifier rewrite) with a
//!   synthetic ambient rune prelude prepended whole-line. The provider path IS
//!   the module's own canonical path; the mapper is the line-only,
//!   rewrite-aware [`SelfFileProviderMapper`].
//!
//! Both expose the SAME three mapping operations the feature layer uses —
//! `carrier_to_tsx`, `tsx_to_carrier`, `tsx_range_to_carrier` — through
//! [`ProviderPositionMapper`], so every LSP feature maps positions uniformly
//! regardless of which projection produced the buffer.

use std::sync::Arc;

use verter_span::{LspPosition, TsPosition};

use super::position_map::{GeneratedMapped, PositionMapper, RunId, SourceMapped};

/// The persisted per-document projection: the mapper plus the discriminant of
/// which provider buffer the document projects into. The `provider_path` is
/// NOT stored here — it is derived from the committed provider-sync state
/// (carrier IDE path) or IS the canonical id (self-file), so the projection
/// carries only the mapper.
#[derive(Clone)]
pub enum DocumentProviderProjection {
    /// A framework carrier projecting into an IDE TSX file via a source map.
    /// The source-map-backed [`PositionMapper`] is held behind an [`Arc`]: it
    /// is the large variant (it owns the `OwnedSourceMap` plus three precomputed
    /// lookup tables), and the hot read path (`get_position_mapper` →
    /// [`DocumentProviderProjection::mapper`]) hands out a CHEAP handle clone per
    /// request instead of deep-copying the whole mapper. Sharing the allocation
    /// also keeps an `Option<DocumentProviderProjection>` field cheap to move.
    CarrierIde {
        /// The shared source-map-backed position mapper.
        mapper: Arc<PositionMapper>,
    },
    /// A non-component rune module projecting into its own-path provider buffer
    /// (`<rune prelude> + <rewritten module bytes>`).
    SelfFile {
        /// The line-only, rewrite-aware self-file mapper.
        mapper: SelfFileProviderMapper,
    },
}

impl DocumentProviderProjection {
    /// Build a carrier-IDE projection from a source-map-backed mapper.
    #[must_use]
    pub fn carrier_ide(mapper: PositionMapper) -> Self {
        DocumentProviderProjection::CarrierIde {
            mapper: Arc::new(mapper),
        }
    }

    /// Whether this projection is a SELF-FILE rune-module own-buffer projection.
    ///
    /// Used to GATE OFF features whose workspace-EDIT positions are not yet
    /// mapped through the [`SelfFileProviderMapper`] (rename, code actions) for a
    /// rune-module own buffer — an unmapped edit would land at the wrong
    /// position (off by `prelude_line_count`, or inside the prelude) and corrupt
    /// the module. These features stay DEFERRED for the self-file projection
    /// until their edit-mapping is implemented; the carrier projection is
    /// unaffected.
    #[must_use]
    pub fn is_self_file(&self) -> bool {
        matches!(self, DocumentProviderProjection::SelfFile { .. })
    }

    /// The unified mapper view for the feature layer.
    #[must_use]
    pub fn mapper(&self) -> ProviderPositionMapper {
        match self {
            DocumentProviderProjection::CarrierIde { mapper } => {
                ProviderPositionMapper::SourceMap(mapper.clone())
            }
            DocumentProviderProjection::SelfFile { mapper } => {
                ProviderPositionMapper::SelfFile(mapper.clone())
            }
        }
    }
}

/// The unified source↔provider mapper handed to every LSP feature.
///
/// Dispatches the three mapping operations to the source-map-backed
/// [`PositionMapper`] (carrier IDE) or the line-only rewrite-aware
/// [`SelfFileProviderMapper`] (self-file rune module). The return shapes match
/// [`PositionMapper`] exactly, so the `type_provider::merge` helpers are projection-
/// agnostic.
#[derive(Clone)]
pub enum ProviderPositionMapper {
    /// Source-map-backed mapping (carrier IDE TSX). The large variant: the
    /// `PositionMapper` is held behind an [`Arc`] so cloning this mapper (the
    /// per-request read path) shares ONE allocation instead of deep-copying the
    /// source map and its precomputed lookup tables.
    SourceMap(Arc<PositionMapper>),
    /// Line-only rewrite-aware mapping (self-file rune module).
    SelfFile(SelfFileProviderMapper),
}

impl ProviderPositionMapper {
    /// Wrap a source-map-backed [`PositionMapper`] as a provider mapper.
    #[must_use]
    pub fn source_map(mapper: PositionMapper) -> Self {
        ProviderPositionMapper::SourceMap(Arc::new(mapper))
    }

    /// Map a generated provider-buffer position back to the user-source
    /// position. `None` when the provider position has no user-source
    /// correlation (synthetic region / prelude / inside a rewritten specifier).
    #[must_use]
    pub fn tsx_to_carrier(&self, pos: TsPosition) -> Option<SourceMapped> {
        match self {
            ProviderPositionMapper::SourceMap(m) => m.tsx_to_carrier(pos),
            ProviderPositionMapper::SelfFile(m) => m.tsx_to_carrier(pos),
        }
    }

    /// Map a user-source position to the generated provider-buffer position.
    /// `None` when the source position has no provider correlation (inside a
    /// rewritten specifier / unmapped region).
    #[must_use]
    pub fn carrier_to_tsx(&self, pos: LspPosition) -> Option<GeneratedMapped> {
        match self {
            ProviderPositionMapper::SourceMap(m) => m.carrier_to_tsx(pos),
            ProviderPositionMapper::SelfFile(m) => m.carrier_to_tsx(pos),
        }
    }

    /// Map a generated provider-buffer range back to a user-source range. Maps
    /// only when BOTH endpoints have user-source correlation in the same
    /// projection component.
    #[must_use]
    pub fn tsx_range_to_carrier(
        &self,
        start: TsPosition,
        end: TsPosition,
    ) -> Option<(LspPosition, LspPosition)> {
        match self {
            ProviderPositionMapper::SourceMap(m) => m.tsx_range_to_carrier(start, end),
            ProviderPositionMapper::SelfFile(m) => m.tsx_range_to_carrier(start, end),
        }
    }

    /// Find the mapped run whose carrier-source extent ends exactly at `(line, col)` and
    /// return its provider-side endpoint, for the completion-only incomplete-member-access
    /// (`obj.` / `obj?.`) boundary anchor.
    ///
    /// This is a source-map-run concept: only the `SourceMap` variant has mapped runs. A
    /// `SelfFile` rune-module projection has no synthetic JSX runs — its member boundary is
    /// the strict line-only mapper's normal path — so it returns `None` (fail-closed), and the
    /// completion handler stays mapper-agnostic.
    #[must_use]
    pub(crate) fn mapped_run_ending_at_src(&self, line: u32, col: u32) -> Option<TsPosition> {
        match self {
            ProviderPositionMapper::SourceMap(m) => m.mapped_run_ending_at_src(line, col),
            ProviderPositionMapper::SelfFile(_) => None,
        }
    }

    /// The generated provider-buffer position immediately after the last emitted synthetic
    /// helper import, the authoritative gate for classifying a TypeProvider auto-import
    /// insertion (see [`crate::type_provider::auto_import`]).
    ///
    /// Only the carrier-IDE `SourceMap` projection emits a synthetic helper-import preamble; a
    /// `SelfFile` rune-module projection has none, so it returns `None` (the auto-import
    /// translator then rejects a non-round-tripping edit rather than re-anchoring on a guess).
    #[must_use]
    pub(crate) fn helper_preamble_end(&self) -> Option<TsPosition> {
        match self {
            ProviderPositionMapper::SourceMap(m) => m.helper_preamble_end(),
            ProviderPositionMapper::SelfFile(_) => None,
        }
    }
}

/// One contiguous import-specifier rewrite on a single user-source line.
///
/// `rewrite_non_carrier_source_with_resolver` replaces an import specifier's
/// byte span (e.g. `'./store.svelte'` → `'./store.svelte.ts'`) in place BEFORE
/// the rune prelude is prepended. The replacement inserts NO newline, so the
/// LINE offset of every position is exact; but it CHANGES the byte/char length
/// of the specifier, shifting every column after the rewrite point on that
/// line. The mapper records each rewrite as a per-line column segment so it can
/// (a) shift columns past the rewrite point and (b) DROP positions that land
/// INSIDE a rewritten specifier (the rewritten text has no faithful per-char
/// user-source correlation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RewriteSegment {
    /// User-source line the rewrite occurred on (0-based, after prelude offset
    /// is NOT yet applied — this is the user-source line).
    line: u32,
    /// User-source column where the rewritten specifier starts (inclusive).
    src_start: u32,
    /// User-source column where the rewritten specifier ends (exclusive).
    src_end: u32,
    /// Provider column where the rewritten specifier starts (inclusive). Equals
    /// `src_start` (nothing before the rewrite point shifts).
    provider_start: u32,
    /// Provider column where the rewritten specifier ends (exclusive).
    provider_end: u32,
}

/// A line-only, rewrite-aware source↔provider mapper for a self-file provider
/// buffer (`<rune prelude> + <rewritten module bytes>`).
///
/// Mapping rules (the source↔provider contract):
/// - **source → provider**: `line + prelude_line_count`; column shifted by the
///   cumulative rewrite delta of every rewrite earlier on the same line; DROP
///   if the source position falls INSIDE a rewritten specifier span.
/// - **provider → source**: DROP if `line < prelude_line_count` (the prelude
///   region has no user-source correlation — never clamp); else
///   `line - prelude_line_count`; column shifted back by the rewrite delta;
///   DROP if the provider position falls INSIDE a rewritten specifier span.
/// - **ranges**: map ONLY when BOTH endpoints map (both in the user-source
///   region, neither inside a rewritten specifier); otherwise drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfFileProviderMapper {
    prelude_line_count: u32,
    /// Rewrite segments, grouped/sorted by `(line, src_start)`.
    rewrites: Vec<RewriteSegment>,
}

impl SelfFileProviderMapper {
    /// Build a self-file mapper from the prelude line count and the import-
    /// specifier rewrite replacements (`(byte_start, byte_end, replacement)`)
    /// computed against the pre-rewrite, pre-prelude user-source.
    ///
    /// `line_index` must be built from that SAME user-source — the byte spans
    /// index into it and are translated into per-line (line, column) segments
    /// in the negotiated encoding.
    #[must_use]
    pub fn new(
        prelude_line_count: u32,
        replacements: &[(usize, usize, String)],
        line_index: &super::line_index::LineIndex,
    ) -> Self {
        let mut rewrites: Vec<RewriteSegment> = Vec::with_capacity(replacements.len());
        for (byte_start, byte_end, replacement) in replacements {
            let Some(start_pos) = line_index.offset_to_position(*byte_start as u32) else {
                continue;
            };
            let Some(end_pos) = line_index.offset_to_position(*byte_end as u32) else {
                continue;
            };
            // A specifier rewrite never spans a newline (it replaces a quoted
            // string literal in place). Skip anything that does — it has no
            // single-line column model and must not corrupt the per-line delta.
            if start_pos.line != end_pos.line {
                continue;
            }
            // The replacement length in the negotiated encoding (UTF-16 code
            // units in the default config) — the provider-side specifier width.
            let provider_width = encoded_len(replacement, &line_index.encoding());
            let src_width = end_pos.character.saturating_sub(start_pos.character);
            rewrites.push(RewriteSegment {
                line: start_pos.line,
                src_start: start_pos.character,
                src_end: start_pos.character + src_width,
                // Provisional provider bounds keyed off the SOURCE column. Every
                // segment after the first on the same line is shifted below by
                // the cumulative provider-vs-source delta of EARLIER same-line
                // rewrites — the provider buffer applies them left-to-right.
                provider_start: start_pos.character,
                provider_end: start_pos.character + provider_width,
            });
        }
        rewrites.sort_by_key(|r| (r.line, r.src_start));
        // Shift each segment's provider-side bounds by the cumulative column
        // delta of every EARLIER rewrite on the SAME line. A specifier rewrite
        // changes the specifier's width, so the provider column of every later
        // same-line specifier is offset by the sum of earlier deltas. The
        // source-side bounds stay keyed off the (unshifted) user-source column;
        // only the provider side accumulates. Without this, `provider_col_to_
        // source` compares a provider column against the UNSHIFTED bounds of the
        // 2nd+ same-line segment and mismaps positions inside/after it.
        let mut current_line = u32::MAX;
        let mut line_delta: i64 = 0;
        for r in &mut rewrites {
            if r.line != current_line {
                current_line = r.line;
                line_delta = 0;
            }
            if line_delta != 0 {
                let shift = |c: u32| u32::try_from(i64::from(c) + line_delta).unwrap_or(c);
                r.provider_start = shift(r.provider_start);
                r.provider_end = shift(r.provider_end);
            }
            // Accumulate THIS rewrite's width delta for the next same-line segment.
            line_delta +=
                i64::from(r.provider_end - r.provider_start) - i64::from(r.src_end - r.src_start);
        }
        Self {
            prelude_line_count,
            rewrites,
        }
    }

    /// The number of whole prelude lines this mapper offsets by.
    #[must_use]
    pub fn prelude_line_count(&self) -> u32 {
        self.prelude_line_count
    }

    /// Map a user-source column on `line` to the provider column, applying the
    /// cumulative rewrite delta of earlier rewrites on the line. Returns `None`
    /// when the column falls INSIDE a rewritten specifier span (no faithful
    /// per-char provider correlation).
    fn source_col_to_provider(&self, line: u32, col: u32) -> Option<u32> {
        let mut delta: i64 = 0;
        for r in self.rewrites.iter().filter(|r| r.line == line) {
            if col < r.src_start {
                break;
            }
            if col < r.src_end {
                // Inside the rewritten specifier — drop.
                return None;
            }
            // Past this rewrite — accumulate its width delta.
            delta +=
                i64::from(r.provider_end - r.provider_start) - i64::from(r.src_end - r.src_start);
        }
        let provider = i64::from(col) + delta;
        u32::try_from(provider).ok()
    }

    /// Map a provider column on the user-source line `line` back to the user-
    /// source column, undoing the rewrite delta. Returns `None` when the
    /// provider column falls INSIDE a rewritten specifier span.
    fn provider_col_to_source(&self, line: u32, col: u32) -> Option<u32> {
        let mut delta: i64 = 0;
        for r in self.rewrites.iter().filter(|r| r.line == line) {
            if col < r.provider_start {
                break;
            }
            if col < r.provider_end {
                // Inside the rewritten specifier — drop.
                return None;
            }
            delta +=
                i64::from(r.provider_end - r.provider_start) - i64::from(r.src_end - r.src_start);
        }
        let src = i64::from(col) - delta;
        u32::try_from(src).ok()
    }

    /// source → provider (`carrier_to_tsx`): shift line down by the prelude,
    /// apply the per-line rewrite delta, drop inside a rewrite span.
    fn map_source_to_provider(&self, pos: LspPosition) -> Option<TsPosition> {
        let col = self.source_col_to_provider(pos.line, pos.character)?;
        Some(TsPosition {
            line: pos.line + self.prelude_line_count,
            character: col,
        })
    }

    /// provider → source (`tsx_to_carrier`): drop the prelude region, shift
    /// line up by the prelude, undo the per-line rewrite delta, drop inside a
    /// rewrite span.
    fn map_provider_to_source(&self, pos: TsPosition) -> Option<LspPosition> {
        if pos.line < self.prelude_line_count {
            // Prelude region — no user-source correlation. Never clamp.
            return None;
        }
        let src_line = pos.line - self.prelude_line_count;
        let col = self.provider_col_to_source(src_line, pos.character)?;
        Some(LspPosition {
            line: src_line,
            character: col,
        })
    }

    /// Map a provider position back to user-source (the `tsx_to_carrier`
    /// surface). The `RunId` is a self-file sentinel; self-file ranges are
    /// composed by [`Self::tsx_range_to_carrier`] directly, never by run
    /// compatibility.
    #[must_use]
    pub fn tsx_to_carrier(&self, pos: TsPosition) -> Option<SourceMapped> {
        let mapped = self.map_provider_to_source(pos)?;
        Some(SourceMapped {
            pos: mapped,
            run: RunId::self_file_sentinel(),
        })
    }

    /// Map a user-source position to the provider buffer (the `carrier_to_tsx`
    /// surface).
    #[must_use]
    pub fn carrier_to_tsx(&self, pos: LspPosition) -> Option<GeneratedMapped> {
        let mapped = self.map_source_to_provider(pos)?;
        Some(GeneratedMapped {
            pos: mapped,
            run: RunId::self_file_sentinel(),
        })
    }

    /// Map a provider range back to a user-source range. Both endpoints must
    /// map (both in the user-source region, neither inside a rewrite span).
    #[must_use]
    pub fn tsx_range_to_carrier(
        &self,
        start: TsPosition,
        end: TsPosition,
    ) -> Option<(LspPosition, LspPosition)> {
        let start_src = self.map_provider_to_source(start)?;
        // Half-open end: the exclusive end column may sit at the provider end
        // of a rewrite span (`col == provider_end`), which `map_provider_to_
        // source` accepts (only `< provider_end` is dropped). A pure column
        // shift on the same line is the faithful end.
        let end_src = self.map_provider_to_source(end)?;
        Some((start_src, end_src))
    }
}

/// Length of `s` in the negotiated position encoding's code units.
fn encoded_len(s: &str, encoding: &tower_lsp_server::ls_types::PositionEncodingKind) -> u32 {
    use tower_lsp_server::ls_types::PositionEncodingKind;
    if *encoding == PositionEncodingKind::UTF16 {
        s.encode_utf16().count() as u32
    } else if *encoding == PositionEncodingKind::UTF32 {
        s.chars().count() as u32
    } else {
        s.len() as u32
    }
}

#[cfg(test)]
#[path = "provider_projection_tests.rs"]
mod provider_projection_tests;
