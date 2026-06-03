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
    ///
    /// The unified planner now expresses in-place preservation as plan-level
    /// `Verbatim`/`Ident` NO-OPS rather than a standalone `PreserveOriginal` op, so
    /// this variant is no longer constructed in production. It is retained as part
    /// of the typed `EmitOp` substrate (the `emit_op_has_no_mapped_overwrite_variant`
    /// guard enumerates it) and as the explicit "preserve one span" affordance.
    #[allow(dead_code)]
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

/// Trim a source span of leading/trailing ASCII whitespace, returning the trimmed
/// `[start, end)` offsets. Mirrors the `value_expr.trim()` offset arithmetic used
/// throughout the IDE prop emitters.
pub fn trim_span(source: &str, start: u32, end: u32) -> (u32, u32) {
    let raw = &source[start as usize..end as usize];
    let lead = (raw.len() - raw.trim_start().len()) as u32;
    let trail = (raw.len() - raw.trim_end().len()) as u32;
    (start + lead, end - trail)
}

// ─────────────────────────────────────────────────────────────────────────────
// Unified user-expression emission planner + placement sinks.
//
// THE single owner of template-binding emission semantics. Every IDE user
// expression — v-html / v-text / dynamic-key `:[key]` / native + dynamic
// v-model / v-on (object-spread, dynamic-event, duplicate, inline, `$event`,
// non-object) / v-show / no-value `:foo` / `.foo` / `:ref` — builds an
// [`ExprPlan`] via [`plan_user_expr`] and emits it through [`emit_expr_plan`]
// with a [`Placement`]. The object-literal rewriting layer ([`plan_object_literal`])
// sits ABOVE the expression planner and handles static keys / computed keys /
// spreads / shorthand exactly once.
//
// The planner owns the binding semantics ONCE (replacing the hand-rolled
// `emit_relocated_value` / `emit_v_on_object_spread` / `build_prefixed_expr`
// re-derivations):
//   - resolved binding → synthetic prefix, MAPPED identifier, synthetic suffix
//   - ignored / local binding → MAPPED bare identifier, NO prefix/suffix
//   - verbatim punctuation / operators / literals → unmapped
//   - shorthand → controlled by the EXPLICIT [`ShorthandMode`], NOT inferred
//     blindly from `binding.is_shorthand`
//
// The IN-PLACE sink emits over the same plan: in-place mapped identifiers are
// NO-OPS (the original bytes stay an `Original`, 1:1-mapped chunk); only the
// prefix / suffix / shorthand-expansion inserts are emitted (the exact analogue
// of `collect_binding_patches`). The RELOCATED sink emits each surviving
// identifier as an `InsertMapped` at the target anchor and every verbatim /
// synthetic slice as an `InsertUnmapped`.
// ─────────────────────────────────────────────────────────────────────────────

/// Where an [`ExprPlan`] is emitted.
#[derive(Debug, Clone, Copy)]
pub enum Placement {
    /// The user expression bytes stay verbatim in the source; only prefix /
    /// suffix / shorthand-expansion inserts are emitted (mapped identifiers are
    /// already `Original` chunks). Synthetic boundaries are the caller's separate
    /// [`EmitOp::OverwriteSyntheticBoundary`] ops AROUND the plan.
    InPlace,
    /// The prop span is deleted and the expression re-emitted at `at`; every
    /// surviving identifier becomes its own mapped insertion.
    Relocated { at: SourceByteOffset },
}

/// How the planner treats a shorthand-property value identifier (`{ foo }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShorthandMode {
    /// Default: a shorthand binding that gets an accessor prefix/suffix is
    /// expanded to `name: <prefixed>` (otherwise `{ _ctx.foo }` is invalid JS).
    /// This mirrors `build_prefixed_expr` / `collect_binding_patches`.
    Auto,
    /// The enclosing object-literal layer has ALREADY emitted the property key
    /// (`onClick: `), so the value must be emitted WITHOUT a key expansion —
    /// otherwise `onClick: ` + auto-expand `click: ` would double the key
    /// (`onClick: click: __props.click`). Used by [`plan_object_literal`].
    ValueOnly,
}

/// Options driving [`plan_user_expr`].
#[derive(Debug, Clone, Copy)]
pub struct ExprOptions {
    /// Shorthand-expansion policy (see [`ShorthandMode`]).
    pub shorthand: ShorthandMode,
    /// How to plan an expression with NO extracted bindings (parse failure / an
    /// opaque value). When `true` (RELOCATED sites: the value moves into synthetic
    /// scaffolding and must carry its own accessor prefix), the resolver's
    /// simple-expression split is applied (`resolve_simple_expr`). When `false`
    /// (IN-PLACE sites: the value's bytes stay authoritative as authored), the span
    /// is preserved verbatim with NO resolution — matching the legacy in-place
    /// behaviour where `collect_binding_patches` only ran when bindings existed.
    pub resolve_unbound: bool,
}

impl Default for ExprOptions {
    fn default() -> Self {
        // The default is a RELOCATED value (the common case for the planner's
        // explicit callers): resolve an unbound expression so it carries its prefix.
        Self {
            shorthand: ShorthandMode::Auto,
            resolve_unbound: true,
        }
    }
}

impl ExprOptions {
    /// Options for an IN-PLACE value: never resolve an unbound expression (its bytes
    /// stay authoritative as authored).
    pub fn in_place() -> Self {
        Self {
            shorthand: ShorthandMode::Auto,
            resolve_unbound: false,
        }
    }
}

/// One piece of a planned user expression. The planner is the SOLE producer; the
/// two placement sinks are the sole consumers.
#[derive(Debug, Clone)]
enum ExprPiece<'a> {
    /// Synthetic scaffolding text that is NOT a source slice — object braces /
    /// separators / rewritten event keys produced by [`plan_object_literal`].
    /// Always unmapped. Only valid in a RELOCATED plan (object-literal rewriting
    /// deletes the prop span); in-place emission of one is a caller bug.
    Synthetic { text: EmitText<'a> },
    /// Verbatim source slice between identifiers (punctuation, operators,
    /// literals, whitespace). `range` is the source span. In-place: stays an
    /// `Original` chunk (no-op). Relocated: emitted as an unmapped insert (a
    /// relocated verbatim slice has no navigation meaning).
    Verbatim { range: SourceByteRange },
    /// A navigable identifier that survives in the output. In-place: the
    /// identifier stays an `Original` chunk; the `prefix` / `suffix` /
    /// `shorthand_key` are emitted as in-place prepends. Relocated: prefix +
    /// shorthand-key + MAPPED identifier + suffix at the anchor.
    Ident {
        source_start: SourceByteOffset,
        name: &'a str,
        prefix: &'static str,
        suffix: &'static str,
        /// `Some("name: ")` when an `Auto`-shorthand binding needs key expansion.
        shorthand_key: Option<String>,
    },
    /// A scoped local (v-for / v-slot) — MAPPED bare identifier, NO prefix/suffix.
    /// In-place: stays an `Original` chunk (no-op). Relocated: a bare
    /// `InsertMapped`.
    IgnoredIdent {
        source_start: SourceByteOffset,
        name: &'a str,
    },
    /// A synthetic value identifier whose mapped core is NOT a verbatim slice of
    /// the source (the no-value `:foo` / `.foo` shorthands derive `fooBar` from the
    /// `foo-bar` arg token). `core` maps to `core_source_start`; the surrounding
    /// `prefix` / `suffix` are unmapped. Only ever emitted RELOCATED (the value text
    /// does not exist in the source).
    SynthesizedCore {
        core: String,
        core_source_start: SourceByteOffset,
        prefix: String,
        suffix: String,
    },
}

/// A planned user expression: an ordered piece list owning the binding semantics.
#[derive(Debug)]
pub struct ExprPlan<'a> {
    pieces: Vec<ExprPiece<'a>>,
}

/// Plan a user value expression `[span.start, span.end)` (already trimmed) into an
/// ordered [`ExprPlan`]. `all_bindings` is the enclosing expression's extracted
/// bindings (a binding outside `span` is filtered out). When no bindings are
/// available (parse failure / single bare identifier), the resolver's
/// simple-expression accessor split drives a single-identifier plan.
///
/// This is the ONE place binding semantics are derived for IDE emission; both
/// sinks consume the result.
pub fn plan_user_expr<'a>(
    source: &'a str,
    span: SourceByteRange,
    all_bindings: Option<&[crate::utils::oxc::Binding<'a>]>,
    resolver: &BindingResolver<'_>,
    opts: ExprOptions,
) -> ExprPlan<'a> {
    let start = span.start.0;
    let end = span.end.0;
    let expr = &source[start as usize..end as usize];

    // Filter bindings to those inside the span (a larger enclosing expression's
    // bindings may include identifiers before/after this sub-span).
    let in_span: Vec<&crate::utils::oxc::Binding<'a>> = match all_bindings {
        Some(bs) => bs
            .iter()
            .filter(|b| b.pos >= start && b.pos < end)
            .collect(),
        None => Vec::new(),
    };

    let mut pieces: Vec<ExprPiece<'a>> = Vec::new();

    if in_span.is_empty() {
        // No extracted bindings (parse failure / opaque value).
        if !opts.resolve_unbound {
            // IN-PLACE: the bytes stay authoritative as authored — preserve the
            // whole span verbatim (1:1 mapped, no resolution). This matches the
            // legacy in-place behaviour (`collect_binding_patches` only ran when
            // bindings existed; otherwise the original bytes were preserved).
            if end > start {
                pieces.push(ExprPiece::Verbatim {
                    range: SourceByteRange::new(SourceByteOffset(start), SourceByteOffset(end)),
                });
            }
            return ExprPlan { pieces };
        }
        // RELOCATED: split the resolved simple expression into a mapped identifier
        // core plus unmapped prefix/suffix (the resolver's simple-expression
        // accessor decision). If the resolved form does not contain the trimmed
        // expression verbatim (keyword bracket-notation rewrite, etc.), emit a
        // synthesized core so the identifier still maps.
        let trimmed = expr.trim();
        let resolved = resolver.resolve_simple_expr(trimmed);
        if resolved == trimmed {
            // Unchanged — the whole span is one verbatim/identifier run that maps
            // 1:1 in place and as one mapped run relocated.
            pieces.push(ExprPiece::Ident {
                source_start: SourceByteOffset(start),
                name: expr,
                prefix: "",
                suffix: "",
                shorthand_key: None,
            });
        } else if let Some(idx) = resolved.find(trimmed) {
            // `prefix + ident + suffix`: the core maps; prefix/suffix unmapped.
            pieces.push(ExprPiece::SynthesizedCore {
                core: trimmed.to_string(),
                core_source_start: SourceByteOffset(start),
                prefix: resolved[..idx].to_string(),
                suffix: resolved[idx + trimmed.len()..].to_string(),
            });
        } else {
            // Resolver rewrote the expression entirely (e.g. `$props["class"]`):
            // the core text is not a verbatim slice. Emit a synthesized core with
            // no extra prefix/suffix so the whole resolved form is one mapped run.
            pieces.push(ExprPiece::SynthesizedCore {
                core: resolved,
                core_source_start: SourceByteOffset(start),
                prefix: String::new(),
                suffix: String::new(),
            });
        }
        return ExprPlan { pieces };
    }

    // Bindings present: walk them in source order, emitting verbatim slices
    // between identifiers and a typed identifier piece per binding. This is the
    // unified analogue of `build_prefixed_expr` / `collect_binding_patches`.
    let mut cursor = start;
    for binding in in_span {
        let pos = binding.pos;
        if pos < cursor || pos > end {
            continue;
        }
        // Verbatim text before this identifier.
        if pos > cursor {
            pieces.push(ExprPiece::Verbatim {
                range: SourceByteRange::new(SourceByteOffset(cursor), SourceByteOffset(pos)),
            });
        }
        let name_end = pos + binding.name.len() as u32;
        if binding.ignore {
            pieces.push(ExprPiece::IgnoredIdent {
                source_start: SourceByteOffset(pos),
                name: binding.name,
            });
            cursor = name_end;
            continue;
        }
        let prefix = resolver.resolve_prefix(binding.name);
        let suffix = resolver.resolve_suffix(binding.name);
        // Shorthand key expansion is controlled by the EXPLICIT mode: only `Auto`
        // expands. `ValueOnly` (object-literal layer already emitted the key)
        // never does — this is the fix for the doubled `onClick: click:` bug.
        let shorthand_key = if opts.shorthand == ShorthandMode::Auto
            && binding.is_shorthand
            && (!prefix.is_empty() || !suffix.is_empty())
        {
            Some(format!("{}: ", binding.name))
        } else {
            None
        };
        pieces.push(ExprPiece::Ident {
            source_start: SourceByteOffset(pos),
            name: binding.name,
            prefix,
            suffix,
            shorthand_key,
        });
        cursor = name_end;
    }
    // Trailing verbatim text.
    if cursor < end {
        pieces.push(ExprPiece::Verbatim {
            range: SourceByteRange::new(SourceByteOffset(cursor), SourceByteOffset(end)),
        });
    }

    ExprPlan { pieces }
}

/// Emit an [`ExprPlan`] at a [`Placement`]. The two sinks share the plan but
/// differ in how each piece lowers (see [`ExprPiece`]).
pub fn emit_expr_plan<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    plan: &ExprPlan<'_>,
    placement: Placement,
    source: &'alloc str,
) {
    match placement {
        Placement::InPlace => emit_plan_in_place(out, plan),
        Placement::Relocated { at } => emit_plan_relocated(out, plan, at, source),
    }
}

/// IN-PLACE sink: the original expression bytes stay verbatim (`Original` chunks);
/// only prefix / suffix / shorthand-key inserts are emitted at source positions.
/// This is the exact analogue of `collect_binding_patches` — verbatim slices,
/// surviving identifiers, and ignored locals are all NO-OPS.
fn emit_plan_in_place<'alloc>(out: &mut CodeGenOutput<'alloc>, plan: &ExprPlan<'_>) {
    for piece in &plan.pieces {
        match piece {
            // Verbatim / surviving identifier / ignored local: already in the
            // source as an `Original` (1:1-mapped) chunk → NO-OP.
            ExprPiece::Verbatim { .. } | ExprPiece::IgnoredIdent { .. } => {}
            // Synthetic scaffolding is only produced by the object-literal layer,
            // which always relocates. An in-place synthetic piece is a caller bug.
            ExprPiece::Synthetic { .. } => {
                debug_assert!(
                    false,
                    "Synthetic piece (object-literal scaffolding) cannot be emitted IN-PLACE"
                );
            }
            ExprPiece::Ident {
                source_start,
                name,
                prefix,
                suffix,
                shorthand_key,
            } => {
                // Shorthand expansion + accessor prefix at the identifier start;
                // accessor suffix right after it. Stable-sorted same-position
                // prepends preserve `<shorthand-key><prefix><ident>` order, exactly
                // matching `collect_binding_patches`.
                if let Some(key) = shorthand_key {
                    out.prepend_alloc(source_start.0, key);
                }
                if !prefix.is_empty() {
                    out.prepend_static(source_start.0, prefix);
                }
                if !suffix.is_empty() {
                    out.prepend_static(source_start.0 + name.len() as u32, suffix);
                }
            }
            // A synthesized core cannot stay in place — its mapped text is not a
            // verbatim source slice. It is only ever planned for the relocated
            // shorthands; in-place emission of one is a planner/caller bug.
            ExprPiece::SynthesizedCore { .. } => {
                debug_assert!(
                    false,
                    "SynthesizedCore piece cannot be emitted IN-PLACE (its text is not a source slice)"
                );
            }
        }
    }
}

/// RELOCATED sink: the prop span is deleted and the expression re-emitted at `at`.
/// Each surviving identifier is an `InsertMapped` pointing at its source start;
/// verbatim slices, accessor prefixes/suffixes, and shorthand keys are unmapped
/// inserts. Ignored locals are MAPPED bare identifiers.
fn emit_plan_relocated<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    plan: &ExprPlan<'_>,
    at: SourceByteOffset,
    source: &'alloc str,
) {
    let unmapped = |out: &mut CodeGenOutput<'alloc>, text: EmitText<'_>| {
        emit_op(out, &EmitOp::InsertUnmapped { at, text });
    };
    let mapped =
        |out: &mut CodeGenOutput<'alloc>, text: EmitText<'_>, source_start: SourceByteOffset| {
            emit_op(
                out,
                &EmitOp::InsertMapped {
                    at,
                    text,
                    source_start,
                    content_offset: GeneratedByteLen(0),
                },
            );
        };

    for piece in &plan.pieces {
        match piece {
            ExprPiece::Synthetic { text } => {
                if !text.is_empty() {
                    unmapped(out, EmitText::Borrowed(text.as_str()));
                }
            }
            ExprPiece::Verbatim { range } => {
                let slice = &source[range.start.0 as usize..range.end.0 as usize];
                if !slice.is_empty() {
                    unmapped(out, EmitText::Borrowed(slice));
                }
            }
            ExprPiece::IgnoredIdent { source_start, name } => {
                // Scoped local: MAPPED (so ctrl+click lands on the local) but bare.
                mapped(out, EmitText::Borrowed(name), *source_start);
            }
            ExprPiece::Ident {
                source_start,
                name,
                prefix,
                suffix,
                shorthand_key,
            } => {
                if let Some(key) = shorthand_key {
                    unmapped(out, EmitText::Borrowed(key));
                }
                if !prefix.is_empty() {
                    unmapped(out, EmitText::Borrowed(prefix));
                }
                mapped(out, EmitText::Borrowed(name), *source_start);
                if !suffix.is_empty() {
                    unmapped(out, EmitText::Borrowed(suffix));
                }
            }
            ExprPiece::SynthesizedCore {
                core,
                core_source_start,
                prefix,
                suffix,
            } => {
                if !prefix.is_empty() {
                    unmapped(out, EmitText::Borrowed(prefix.as_str()));
                }
                if !core.is_empty() {
                    mapped(out, EmitText::Borrowed(core.as_str()), *core_source_start);
                }
                if !suffix.is_empty() {
                    unmapped(out, EmitText::Borrowed(suffix.as_str()));
                }
            }
        }
    }
}

/// Emit a single user value expression RELOCATED at `at`. Convenience wrapper that
/// plans `span` (filtered to `all_bindings`) and emits it through the relocated
/// sink. `opts` carries the shorthand policy (object-literal callers pass
/// [`ShorthandMode::ValueOnly`]).
pub fn emit_relocated_value<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    at: SourceByteOffset,
    source: &'alloc str,
    span: SourceByteRange,
    all_bindings: Option<&[crate::utils::oxc::Binding<'alloc>]>,
    resolver: &BindingResolver<'alloc>,
    opts: ExprOptions,
) {
    let plan = plan_user_expr(source, span, all_bindings, resolver, opts);
    emit_expr_plan(out, &plan, Placement::Relocated { at }, source);
}

/// Key-rewriting policy for [`plan_object_literal`]. A property-aware layer above
/// the expression planner; today only `v-on`'s event-object rewriting exists.
#[derive(Debug, Clone, Copy)]
pub enum KeyRewritePolicy {
    /// `v-on="{ … }"`: a static key is an event name rewritten via
    /// `event_to_jsx_name` (`click` → `onClick`, quoted when not a JSX identifier);
    /// a computed key stays a navigable expression; shorthand values are emitted in
    /// [`ShorthandMode::ValueOnly`] because the rewritten key is emitted separately.
    VOnEventObject,
}

/// Plan a `{ … }` object literal as a mapped spread `{...{ … }}`, RELOCATED.
///
/// The property-aware layer that sits ABOVE [`plan_user_expr`]: it handles static
/// key rewriting, computed keys, spread properties, and shorthand EXACTLY ONCE.
/// Each property VALUE is planned through the shared [`plan_user_expr`] (so its
/// identifiers map to source); the rewritten key, braces, and separators are
/// synthetic. For a shorthand `{ click }` it emits the synthetic key `onClick: `
/// and then the value in [`ShorthandMode::ValueOnly`] → `onClick: __props.click`
/// (never the doubled `onClick: click: __props.click`).
///
/// Walks the SOURCE object AST — never a reparsed flat string. Returns `None` when
/// the object cannot be rewritten structurally (an unsupported static-key shape),
/// so the caller can fall back to a flat unmapped spread of the whole expression.
pub fn plan_object_literal<'a>(
    source: &'a str,
    obj: &oxc_ast::ast::ObjectExpression<'a>,
    base: u32,
    bindings: Option<&[crate::utils::oxc::Binding<'a>]>,
    resolver: &BindingResolver<'_>,
    policy: KeyRewritePolicy,
) -> Option<ExprPlan<'a>> {
    use oxc_ast::ast::ObjectPropertyKind;
    use oxc_span::GetSpan;

    let KeyRewritePolicy::VOnEventObject = policy;

    let mut pieces: Vec<ExprPiece<'a>> = Vec::new();
    pieces.push(ExprPiece::Synthetic {
        text: EmitText::Static("{...{"),
    });

    let mut first = true;
    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::SpreadProperty(spread) => {
                let span = spread.argument.span();
                if span.end <= span.start {
                    continue;
                }
                if !first {
                    pieces.push(ExprPiece::Synthetic {
                        text: EmitText::Static(", "),
                    });
                }
                first = false;
                pieces.push(ExprPiece::Synthetic {
                    text: EmitText::Static("..."),
                });
                let (s, e) = trim_span(source, base + span.start, base + span.end);
                extend_plan_with_value(
                    &mut pieces,
                    source,
                    SourceByteRange::new(SourceByteOffset(s), SourceByteOffset(e)),
                    bindings,
                    resolver,
                    ShorthandMode::Auto,
                );
            }
            ObjectPropertyKind::ObjectProperty(p) => {
                let key_span = p.key.span();
                let value_span = p.value.span();
                if key_span.end <= key_span.start || value_span.end <= value_span.start {
                    continue;
                }
                let (vs, ve) = trim_span(source, base + value_span.start, base + value_span.end);
                let value_range = SourceByteRange::new(SourceByteOffset(vs), SourceByteOffset(ve));

                if p.computed {
                    // Computed key `[expr]: value` — both navigable.
                    if !first {
                        pieces.push(ExprPiece::Synthetic {
                            text: EmitText::Static(", "),
                        });
                    }
                    first = false;
                    let (ks, ke) = trim_span(source, base + key_span.start, base + key_span.end);
                    pieces.push(ExprPiece::Synthetic {
                        text: EmitText::Static("["),
                    });
                    extend_plan_with_value(
                        &mut pieces,
                        source,
                        SourceByteRange::new(SourceByteOffset(ks), SourceByteOffset(ke)),
                        bindings,
                        resolver,
                        ShorthandMode::Auto,
                    );
                    pieces.push(ExprPiece::Synthetic {
                        text: EmitText::Static("]: "),
                    });
                    extend_plan_with_value(
                        &mut pieces,
                        source,
                        value_range,
                        bindings,
                        resolver,
                        ShorthandMode::Auto,
                    );
                } else {
                    // Static event key (`click`, `"my-event"`) → JSX event name.
                    let raw_key =
                        &source[(base + key_span.start) as usize..(base + key_span.end) as usize];
                    let event_key = parse_static_event_key(raw_key.trim())?;
                    let mapped_name = crate::ide::event_to_jsx_name(event_key);
                    let key = if crate::template::code_gen::binding::is_simple_ident(&mapped_name) {
                        mapped_name
                    } else {
                        format!("\"{}\"", mapped_name)
                    };
                    if !first {
                        pieces.push(ExprPiece::Synthetic {
                            text: EmitText::Static(", "),
                        });
                    }
                    first = false;
                    // The rewritten event key is synthetic → unmapped. The value is
                    // emitted in ValueOnly mode (the key is already here), so a
                    // shorthand `{ click }` does NOT re-expand its key.
                    pieces.push(ExprPiece::Synthetic {
                        text: EmitText::Owned(format!("{}: ", key)),
                    });
                    extend_plan_with_value(
                        &mut pieces,
                        source,
                        value_range,
                        bindings,
                        resolver,
                        ShorthandMode::ValueOnly,
                    );
                }
            }
        }
    }
    pieces.push(ExprPiece::Synthetic {
        text: EmitText::Static("}}"),
    });
    Some(ExprPlan { pieces })
}

/// Plan one object-property VALUE through the shared [`plan_user_expr`] and append
/// its pieces to `pieces`. Centralises the per-value planning so the object-literal
/// layer never re-derives binding semantics.
fn extend_plan_with_value<'a>(
    pieces: &mut Vec<ExprPiece<'a>>,
    source: &'a str,
    span: SourceByteRange,
    bindings: Option<&[crate::utils::oxc::Binding<'a>]>,
    resolver: &BindingResolver<'_>,
    shorthand: ShorthandMode,
) {
    // Object-literal values are RELOCATED (the prop span is deleted), so an unbound
    // value must carry its accessor prefix → `resolve_unbound: true`.
    let plan = plan_user_expr(
        source,
        span,
        bindings,
        resolver,
        ExprOptions {
            shorthand,
            resolve_unbound: true,
        },
    );
    pieces.extend(plan.pieces);
}

/// Parse a static object-property event key: an unquoted simple identifier or a
/// quoted string literal. Returns the inner event name, or `None` for an
/// unsupported key shape.
fn parse_static_event_key(raw_key: &str) -> Option<&str> {
    let trimmed = raw_key.trim();
    if let Some(stripped) = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })
    {
        return Some(stripped.trim());
    }
    if crate::template::code_gen::binding::is_simple_ident(trimmed) {
        return Some(trimmed);
    }
    None
}

/// Plan + emit a SYNTHESIZED no-value shorthand value RELOCATED at `at`. `<div
/// :foo/>` ≡ `:foo="foo"` and `<div .foo/>` ≡ `.foo="foo"`; Vue derives the value
/// identifier from the arg/key name (kebab→camel), so the generated value text is a
/// TRANSFORM of the source token. `resolved` is the resolver output for `core`; the
/// `core` identifier within it maps to `core_source_start` (the arg/key source
/// token). The surrounding accessor prefix/suffix is unmapped. When `core` is not a
/// substring of `resolved` (the resolver rewrote it entirely), the whole `resolved`
/// is emitted as one mapped run pointing at the source token.
pub fn emit_synthesized_shorthand_value<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    at: SourceByteOffset,
    resolved: &str,
    core: &str,
    core_source_start: SourceByteOffset,
) {
    let piece = match resolved.find(core) {
        Some(idx) if !core.is_empty() => ExprPiece::SynthesizedCore {
            core: resolved[idx..idx + core.len()].to_string(),
            core_source_start,
            prefix: resolved[..idx].to_string(),
            suffix: resolved[idx + core.len()..].to_string(),
        },
        // Resolver rewrote the expression entirely (or empty core): the whole
        // resolved text maps to the source token as one run.
        _ => ExprPiece::SynthesizedCore {
            core: resolved.to_string(),
            core_source_start,
            prefix: String::new(),
            suffix: String::new(),
        },
    };
    let plan = ExprPlan {
        pieces: vec![piece],
    };
    emit_expr_plan(out, &plan, Placement::Relocated { at }, "");
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
