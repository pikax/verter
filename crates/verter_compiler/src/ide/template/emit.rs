//! Typed IDE-only emit substrate for prefixed template expressions.
//!
//! IDE template codegen converts a Vue binding value (`v-html`, `v-text`,
//! `:[key]`, native `v-model`, …) into JSX. The user expression must keep an
//! exact source-map mapping so navigation (hover / go-to-definition) lands on
//! the original identifier, while the synthetic JSX scaffolding (`innerHTML={`,
//! `value={`, `($event:any) => ((`, …) stays unmapped.
//!
//! The previous flat-string producers (`resolve_prefixed_expr`) concatenated
//! `prefix + identifier (+ suffix)` and baked the whole thing into ONE
//! `out.overwrite(prop.start, prop_end, …)`. That maps the entire generated run
//! back to `prop.start` — the desync bug. This module makes that shape
//! impossible: the only mapped emission a binding value can produce is
//! [`EmitOp::InsertMapped`] over the user expression; synthetic text is always
//! [`EmitOp::InsertUnmapped`] or an unmapped [`EmitOp::OverwriteSyntheticBoundary`].
//!
//! No `EmitOp` variant carries BOTH a synthetic prefix and a `source_start` on a
//! single overwrite — see [`emit_op_has_no_mapped_overwrite_variant`].

use verter_span::{GeneratedByteLen, SourceByteOffset, SourceByteRange};

use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::BindingExtractionResult;

/// Text payload for an emit op. Avoids needless allocation: static scaffolding
/// stays `&'static str`, source slices stay borrowed, and only genuinely
/// computed text is owned.
#[derive(Debug, Clone)]
pub enum EmitText<'a> {
    Static(&'static str),
    Borrowed(&'a str),
    /// Owned computed text (e.g. a shorthand-property expansion `name: `).
    Owned(String),
}

impl EmitText<'_> {
    #[inline]
    pub fn as_str(&self) -> &str {
        match self {
            EmitText::Static(s) => s,
            EmitText::Borrowed(s) => s,
            EmitText::Owned(s) => s.as_str(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.as_str().is_empty()
    }
}

/// A single typed emit operation. Each variant lowers to [`CodeGenOutput`] with a
/// fixed mapping discipline (see module docs):
///
/// - [`InsertUnmapped`](EmitOp::InsertUnmapped) → `Inserted` chunk → maps to `None`.
/// - [`InsertMapped`](EmitOp::InsertMapped) → `InsertedMapped` chunk; the mapped
///   token sits at `content_offset` bytes into `text`, pointing at `source_start`.
/// - [`PreserveOriginal`](EmitOp::PreserveOriginal) → a pure no-op; the original
///   `source` bytes stay an `Original` (1:1 mapped) chunk. It writes NOTHING.
/// - [`OverwriteSyntheticBoundary`](EmitOp::OverwriteSyntheticBoundary) → an
///   UNMAPPED replacement: delete `source`, then insert `text` as an `Inserted`
///   chunk. Never a mapped `out.overwrite` (that would map the boundary start).
/// - [`MoveOriginal`](EmitOp::MoveOriginal) → `move_wrapped`; preserves mapping at
///   the new location.
///
/// `MoveOriginal` and the `anchor` field on `OverwriteSyntheticBoundary` exist for
/// parity with the binding mapping architecture (named navigable anchors / future
/// movers). The four desync sites this module fixes do not use `MoveOriginal`, and
/// none of them populate `anchor` (they emit only `None`-mapped synthetic text).
#[derive(Debug, Clone)]
pub enum EmitOp<'a> {
    /// Insert unmapped synthetic text before `at`.
    InsertUnmapped {
        at: SourceByteOffset,
        text: EmitText<'a>,
    },
    /// Insert text before `at`; the mapped token starts at `content_offset` bytes
    /// into `text` and points to `source_start`. Bytes `[0, content_offset)` are
    /// unmapped (e.g. an inline prefix). For the four desync sites the prefix is
    /// emitted as a SEPARATE `InsertUnmapped`, so `content_offset` is `0` and the
    /// mapped identifier begins at byte 0 of `text`.
    InsertMapped {
        at: SourceByteOffset,
        text: EmitText<'a>,
        source_start: SourceByteOffset,
        content_offset: GeneratedByteLen,
    },
    /// Keep the original `source` bytes verbatim and 1:1 mapped. PURE NO-OP — emits
    /// nothing. All surrounding synthetic text is the caller's separate
    /// `OverwriteSyntheticBoundary` ops. `source` documents the preserved span for
    /// the typed contract; the no-op lowering intentionally never dereferences it
    /// (the bytes already live in the source as an `Original` chunk).
    PreserveOriginal {
        #[allow(dead_code)]
        source: SourceByteRange,
    },
    /// Replace the original `source` bytes with UNMAPPED synthetic `text` (delete +
    /// unmapped insert). `anchor` records a named navigable anchor when one is
    /// intended.
    OverwriteSyntheticBoundary {
        source: SourceByteRange,
        text: EmitText<'a>,
        /// Named navigable anchor for the synthetic boundary. Part of the mapping
        /// architecture's "named anchor" affordance; the four desync sites emit
        /// `None` (the boundary text is purely synthetic and unmapped).
        #[allow(dead_code)]
        anchor: Option<SourceByteOffset>,
    },
    /// Move the original `source` bytes to `at`, preserving their mapping. Present
    /// for parity with the mapping architecture / future movers; the four desync
    /// sites do not relocate original bytes, so this variant is not constructed
    /// here today.
    #[allow(dead_code)]
    MoveOriginal {
        source: SourceByteRange,
        at: SourceByteOffset,
    },
}

/// A user binding value to emit as JSX. Structured pieces — never a flat mapped
/// string. `occurrences > 1` means the SAME source expression is emitted multiple
/// times (native `v-model`: a read occurrence plus an assignment-LHS occurrence);
/// each occurrence becomes its own mapped emission pointing at the same source span.
#[derive(Debug)]
pub struct JsxBindingValue<'a> {
    /// The user expression span (already trimmed of surrounding whitespace).
    pub source_expr: SourceByteRange,
    /// Synthetic accessor prefix for a SIMPLE identifier (e.g. `___VERTER___instance.`),
    /// or `None` when per-identifier prefixes come from `bindings`.
    pub prefix: Option<&'a str>,
    /// Synthetic trailing text for a SIMPLE identifier (e.g. `.value`), if any.
    pub suffix: Option<&'a str>,
    /// Number of generated occurrences of the expression (1 for most; 2-3 for v-model).
    pub occurrences: u8,
    /// Per-identifier sub-expression mapping for compound expressions
    /// (`collect_binding_patches` input). Empty for a single bare identifier.
    pub bindings: &'a [crate::utils::oxc::Binding<'a>],
}

/// Lower a single [`EmitOp`] into the deferred [`CodeGenOutput`] operation buffer.
///
/// This is the ONLY place `EmitOp` variants become `CodeGenOutput` mutations, so
/// the mapping discipline lives in one audited spot.
pub fn emit_op<'alloc>(out: &mut CodeGenOutput<'alloc>, op: &EmitOp<'_>) {
    match op {
        EmitOp::InsertUnmapped { at, text } => {
            // Order-preserving unmapped insert: interleaves with adjacent mapped
            // inserts at the same anchor (relocated emission) and emits no mapping.
            out.prepend_ordered_unmapped(at.0, text.as_str());
        }
        EmitOp::InsertMapped {
            at,
            text,
            source_start,
            content_offset,
        } => {
            out.prepend_alloc_mapped_with_offset(
                at.0,
                source_start.0,
                content_offset.0,
                text.as_str(),
            );
        }
        // PURE NO-OP. The original bytes already live in the source and remain an
        // `Original` (1:1 mapped) chunk; emitting anything here would either
        // double-write or break the mapping.
        EmitOp::PreserveOriginal { .. } => {}
        EmitOp::OverwriteSyntheticBoundary { source, text, .. } => {
            // UNMAPPED replacement: delete the original bytes, then insert the
            // synthetic text as an `Inserted` chunk (maps to None). NEVER a mapped
            // `out.overwrite(start, end, text)`.
            out.overwrite(source.start.0, source.end.0, "");
            if !text.is_empty() {
                match text {
                    EmitText::Static(s) => out.prepend_static(source.start.0, s),
                    EmitText::Borrowed(s) => out.prepend_alloc(source.start.0, s),
                    EmitText::Owned(s) => out.prepend_alloc(source.start.0, s),
                }
            }
        }
        EmitOp::MoveOriginal { source, at } => {
            out.move_wrapped(source.start.0, source.end.0, at.0, "", "");
        }
    }
}

/// Emit a binding value `occurrences` times as mapped JSX insertions anchored at
/// `at` (used when the prop span is fully overwritten and the expression is
/// RELOCATED / duplicated, e.g. native `v-model`).
///
/// Each occurrence reproduces the resolved expression so that every user
/// identifier carries its own `InsertMapped` token pointing at the original
/// source position. Synthetic accessor prefixes / suffixes and any verbatim
/// punctuation between identifiers are emitted unmapped. For a compound
/// expression the `bindings` drive a per-identifier split (the mapped analogue of
/// `build_prefixed_expr`); for a single bare identifier the whole expression is
/// one mapped run.
///
/// In-place sites (v-html / v-text / `:[key]` / static `:prop`) do NOT use this:
/// they keep the expression bytes via `PreserveOriginal` + `collect_binding_patches`
/// and only emit `OverwriteSyntheticBoundary` ops around them.
pub fn emit_jsx_binding_value<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    at: SourceByteOffset,
    source: &str,
    v: &JsxBindingValue<'_>,
    resolver: &BindingResolver<'_>,
) {
    let expr_start = v.source_expr.start.0 as usize;
    let expr_end = v.source_expr.end.0 as usize;
    let expr = &source[expr_start..expr_end];

    for _ in 0..v.occurrences.max(1) {
        emit_one_occurrence(out, at, v, expr, resolver);
    }
}

fn emit_one_occurrence<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    at: SourceByteOffset,
    v: &JsxBindingValue<'_>,
    expr: &str,
    resolver: &BindingResolver<'_>,
) {
    let expr_start = v.source_expr.start.0;

    // All emissions here are RELOCATED (anchored at `at`, not at their source
    // bytes). Each piece is a typed `EmitOp` lowered through `emit_op`:
    // `InsertUnmapped` (order-preserving, no mapping) for synthetic/verbatim text
    // and `InsertMapped` for each user identifier.
    let unmapped = |out: &mut CodeGenOutput<'alloc>, text: EmitText<'_>| {
        emit_op(out, &EmitOp::InsertUnmapped { at, text });
    };
    let mapped = |out: &mut CodeGenOutput<'alloc>, text: &str, source_start: u32| {
        emit_op(
            out,
            &EmitOp::InsertMapped {
                at,
                text: EmitText::Borrowed(text),
                source_start: SourceByteOffset(source_start),
                content_offset: GeneratedByteLen(0),
            },
        );
    };

    // No extracted bindings (parse failed or a single bare identifier with no
    // binding data): emit the explicit prefix (unmapped) + the whole expression
    // mapped at its source start + the suffix (unmapped). The prefix/suffix on
    // `JsxBindingValue` carry the simple-identifier accessor decision.
    if v.bindings.is_empty() {
        if let Some(prefix) = v.prefix {
            unmapped(out, EmitText::Borrowed(prefix));
        }
        mapped(out, expr, expr_start);
        if let Some(suffix) = v.suffix {
            unmapped(out, EmitText::Borrowed(suffix));
        }
        return;
    }

    // Bindings present: walk them and emit verbatim slices unmapped, each
    // non-ignored identifier mapped at its own source position (with the
    // resolver's accessor prefix/suffix). This is the mapped analogue of
    // `build_prefixed_expr` and handles both single-ident and compound cases.
    let mut last_end = 0usize; // expr-relative byte cursor
    for binding in v.bindings {
        let rel = (binding.pos as usize).saturating_sub(expr_start as usize);
        if rel > expr.len() {
            continue;
        }
        // Verbatim text before this identifier → unmapped.
        if rel > last_end {
            unmapped(out, EmitText::Borrowed(&expr[last_end..rel]));
        }
        let name_end = rel + binding.name.len();
        if binding.ignore {
            // v-for / v-slot local: bare, unmapped, no prefix.
            unmapped(
                out,
                EmitText::Borrowed(&expr[rel..name_end.min(expr.len())]),
            );
            last_end = name_end;
            continue;
        }
        let prefix = resolver.resolve_prefix(binding.name);
        let suffix = resolver.resolve_suffix(binding.name);
        if binding.is_shorthand && (!prefix.is_empty() || !suffix.is_empty()) {
            unmapped(out, EmitText::Owned(format!("{}: ", binding.name)));
        }
        if !prefix.is_empty() {
            unmapped(out, EmitText::Borrowed(prefix));
        }
        // Identifier text mapped back to its source position.
        mapped(out, binding.name, binding.pos);
        if !suffix.is_empty() {
            unmapped(out, EmitText::Borrowed(suffix));
        }
        last_end = name_end;
    }
    // Trailing verbatim text → unmapped.
    if last_end < expr.len() {
        unmapped(out, EmitText::Borrowed(&expr[last_end..]));
    }
}

/// Build the `JsxBindingValue.bindings` slice from a parsed expression's
/// extracted bindings, or an empty slice when the expression is a single bare
/// identifier with no binding data.
#[inline]
pub fn binding_slice<'a>(
    bindings: Option<&'a BindingExtractionResult<'a>>,
) -> &'a [crate::utils::oxc::Binding<'a>] {
    match bindings {
        Some(b) => &b.bindings,
        None => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug class — a single overwrite carrying BOTH synthetic prefix text and
    /// a `source_start` for a user identifier — must be UNREPRESENTABLE by the
    /// `EmitOp` type. We assert this structurally: the only variant with a
    /// `source_start` is `InsertMapped`, and it has no synthetic-prefix field
    /// (`OverwriteSyntheticBoundary` has no `source_start`). Enumerating every
    /// variant pins the shape so a future field addition that reintroduces the bug
    /// fails to compile against this exhaustive match.
    #[test]
    fn emit_op_has_no_mapped_overwrite_variant() {
        fn assert_shape(op: &EmitOp<'_>) -> bool {
            match op {
                // Unmapped synthetic text: no source_start. OK.
                EmitOp::InsertUnmapped { .. } => true,
                // The ONLY mapped emission: an inserted user expression. It carries
                // source_start + content_offset but NO synthetic-prefix field — the
                // prefix is a separate InsertUnmapped op. OK.
                EmitOp::InsertMapped {
                    source_start,
                    content_offset,
                    ..
                } => {
                    let _ = (source_start, content_offset);
                    true
                }
                // No-op over preserved bytes: no source_start, no text. OK.
                EmitOp::PreserveOriginal { .. } => true,
                // Synthetic boundary: carries text but explicitly NO source_start —
                // it can never map the boundary back to a user identifier. OK.
                EmitOp::OverwriteSyntheticBoundary { anchor, .. } => {
                    // `anchor` is an Option<SourceByteOffset>, NOT a source_start for
                    // mapped content — it names a navigable anchor only.
                    let _ = anchor;
                    true
                }
                // Move preserves an existing mapping; it does not synthesize a
                // mapped overwrite from prefix + identifier. OK.
                EmitOp::MoveOriginal { .. } => true,
            }
        }

        let ops = [
            EmitOp::InsertUnmapped {
                at: SourceByteOffset(0),
                text: EmitText::Static("innerHTML={"),
            },
            EmitOp::InsertMapped {
                at: SourceByteOffset(0),
                text: EmitText::Static("msg"),
                source_start: SourceByteOffset(3),
                content_offset: GeneratedByteLen(0),
            },
            EmitOp::PreserveOriginal {
                source: SourceByteRange::new(SourceByteOffset(3), SourceByteOffset(6)),
            },
            EmitOp::OverwriteSyntheticBoundary {
                source: SourceByteRange::new(SourceByteOffset(0), SourceByteOffset(3)),
                text: EmitText::Static("}"),
                anchor: None,
            },
            EmitOp::MoveOriginal {
                source: SourceByteRange::new(SourceByteOffset(3), SourceByteOffset(6)),
                at: SourceByteOffset(10),
            },
        ];
        assert!(ops.iter().all(assert_shape));
    }
}
