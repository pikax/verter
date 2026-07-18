//! Structural directive → JSX constructs for TSX template codegen.
//!
//! Converts Vue structural directives to JSX-compatible constructs:
//! - `v-if="cond"` → `{()=>{if(cond){...}}}` (IIFE for TypeScript control flow narrowing)
//! - `v-else-if="cond"` → `}else if(cond){...}` (chained in same IIFE)
//! - `v-else` → `}else{...}}}` (final branch + IIFE close)
//! - `v-for="item in items"` → `{items.map((item) => (...))}`
//! - `v-show="expr"` → `style={{display: expr ? undefined : 'none'}}`

use oxc_allocator::Allocator;

use verter_span::{RelativeSpan, SourceByteOffset, SourceByteRange};

use crate::ast::types::{ElementNode, ElementNodeConditionKind};
use crate::ide::template::emit::{
    emit_op, emit_relocated_value, trim_span, EmitOp, EmitText, ExprOptions,
};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::expression::{
    build_prefixed_expr_segments, resolve_simple_expr_segments,
};
use crate::template::code_gen::types::{CodeGenOutput, MappedGeneratedText};
use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};
use crate::utils::oxc::{Binding, BindingExtractionResult, Dynamism};

/// Emit the opening of a v-if/v-else-if/v-else IIFE block.
///
/// - `v-if="cond"` → `{()=>{if(cond){`
/// - `v-else-if="cond"` → `}else if(cond){`
/// - `v-else` → `}else{`
///
/// The IIFE pattern `{()=>{if(cond){...}}}` enables TypeScript control flow
/// narrowing within the if-block, unlike ternaries which don't narrow.
///
/// The condition expression is emitted with per-identifier source mapping
/// so that hovering over e.g. `__props.leftArrow` in the generated TSX maps
/// back to the original `leftArrow` in the template source.
pub fn emit_v_if_open<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
    parent_condition_scopes: &[crate::ide::condition::ConditionScope],
) {
    let condition = match &el.v_condition {
        Some(c) => c,
        None => return,
    };

    match condition.kind {
        ElementNodeConditionKind::If => {
            // v-if="cond" → {()=>{if(cond){
            if let (Some(vs), Some(ve)) = (condition.prop.value_start, condition.prop.value_end) {
                // For nested v-if, emit block guard from parent scopes
                let parent_guard =
                    crate::ide::condition::generate_condition_text(parent_condition_scopes)
                        .map(|text| crate::ide::condition::build_block_guard(&text))
                        .unwrap_or_default();

                let prefix = format!("{{()=>{{{}if(", parent_guard);
                emit_mapped_condition_expr(
                    out,
                    el.tag_open.start,
                    &prefix,
                    "){\n",
                    vs,
                    ve,
                    source,
                    oxc_el,
                    resolver,
                );
            }
        }
        ElementNodeConditionKind::ElseIf => {
            // v-else-if="cond" → else if(cond){
            // Note: the preceding if/else-if block's `}` is emitted by emit_v_if_close,
            // so we do NOT prefix with `}` here (that would double-close).
            if let (Some(vs), Some(ve)) = (condition.prop.value_start, condition.prop.value_end) {
                emit_mapped_condition_expr(
                    out,
                    el.tag_open.start,
                    "else if(",
                    "){\n",
                    vs,
                    ve,
                    source,
                    oxc_el,
                    resolver,
                );
            }
        }
        ElementNodeConditionKind::Else => {
            // v-else → else{
            // Note: the preceding if/else-if block's `}` is emitted by emit_v_if_close.
            out.prepend_alloc(el.tag_open.start, "else{\n");
        }
    }
}

/// Emit the closing of a v-if/v-else-if/v-else IIFE block.
///
/// - v-if / v-else-if: close the if-block with `}`
///   (the IIFE closure `}}` is handled by the parent walk loop)
/// - v-else: close the else block + IIFE: `}}}`
pub fn emit_v_if_close(el: &ElementNode, _source: &str, out: &mut CodeGenOutput<'_>) {
    let condition = match &el.v_condition {
        Some(c) => c,
        None => return,
    };

    let el_end = el
        .tag_close
        .as_ref()
        .map(|tc| tc.end)
        .unwrap_or(el.tag_open.end);

    match condition.kind {
        ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
            // Close the if/else-if block; parent loop handles IIFE closure
            out.prepend_alloc(el_end, "\n}");
        }
        ElementNodeConditionKind::Else => {
            // Close else block + arrow body + JSX expression: }}}
            out.prepend_alloc(el_end, "\n}}}");
        }
    }
}

/// Emit the opening of a v-for map expression with source mapping.
///
/// `v-for="item in items"` → `{items.map((item) => (`
///
/// Uses mapped prepends so that TSGO can resolve types for:
/// - The iterable expression (per-identifier mapping, like v-if)
/// - The iteration parameter (mapped to its position in the v-for value)
#[allow(clippy::too_many_arguments)]
pub fn emit_v_for_open<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
    is_jsx: bool,
    bare: bool,
) {
    let v_for_prop = match &el.v_for {
        Some(p) => p,
        None => return,
    };

    if let (Some(vs), Some(ve)) = (v_for_prop.value_start, v_for_prop.value_end) {
        let v_for_expr = &source[vs as usize..ve as usize];

        // Parse "item in items" or "(item, index) in items"
        if let Some((params, source_expr)) = parse_v_for_expr(v_for_expr) {
            let target_pos = el.tag_open.start;

            // Emit iterable with per-identifier source mapping
            emit_mapped_v_for_iterable(
                out,
                target_pos,
                source_expr.trim(),
                vs,
                v_for_expr,
                oxc_el,
                source,
                bare,
                resolver,
            );

            // Emit ".map((" — unmapped bridge
            // Use mapped_with_offset with offset = content.len() to stay in mapped_prepends vec
            // for correct ordering (regular prepends merge before mapped at same position).
            let map_open = ".map((";
            out.prepend_alloc_mapped_with_offset(target_pos, 0, map_open.len() as u32, map_open);

            // Emit params with source mapping to their position in the v-for value.
            // Compute the byte offset of the trimmed params within the v-for expression.
            let params_trimmed = params.trim();
            let params_offset = v_for_expr
                .find(params_trimmed)
                .map(|off| vs + off as u32)
                .unwrap_or(vs);
            out.prepend_alloc_mapped(target_pos, params_offset, params_trimmed);

            // Add type annotation for v-for parameter to preserve named types in hover.
            // Only for TSX (not JSX — TS annotations are invalid in plain JS), simple
            // identifier iterables, and single/destructured params (no comma).
            let iterable_trimmed = source_expr.trim();
            let has_comma = params_trimmed.contains(',');
            let is_simple_ident = !iterable_trimmed.is_empty()
                && iterable_trimmed
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
                && iterable_trimmed
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$');
            if !is_jsx && !has_comma && is_simple_ident {
                let annotation = format!(": (typeof {})[number]", iterable_trimmed);
                out.prepend_alloc_mapped_with_offset(
                    target_pos,
                    0,
                    annotation.len() as u32,
                    &annotation,
                );
            }

            // Emit ") => { return (" — unmapped tail (statement-body for nested IIFEs)
            let map_close = ") => { return (";
            out.prepend_alloc_mapped_with_offset(target_pos, 0, map_close.len() as u32, map_close);
        }
    }
}

/// Emit the iterable part of v-for as one source-mapped expression plan.
///
/// The iterable lowering builds a single [`MappedGeneratedText`] plan through the
/// shared expression producer: the JS wrappers (`{(` / `Array.from({length: …`),
/// the numeric scaffolding, and the resolver-injected binding prefixes/suffixes
/// (`__props.`, `.value`, keyword brackets) are synthetic and carry no source-map
/// token, while the authored iterable identifiers carry their source offsets. The
/// whole plan lowers through [`CodeGenOutput::prepend_mapped_generated_text`].
#[allow(clippy::too_many_arguments)]
fn emit_mapped_v_for_iterable<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    target_pos: u32,
    iterable: &str,
    v_for_value_start: u32,
    v_for_full_expr: &str,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &str,
    bare: bool,
    resolver: &BindingResolver<'alloc>,
) {
    // Absolute byte offset of the iterable within the source.
    let iterable_offset_in_expr = v_for_full_expr.find(iterable).unwrap_or(0);
    let iterable_start = v_for_value_start + iterable_offset_in_expr as u32;

    // Resolve the iterable through the shared segmented producer.
    let resolved = resolve_iterable_segments(iterable, iterable_start, oxc_el, source, resolver);

    // Wrap with the synthetic JS scaffolding around the resolved iterable.
    // Numeric literals (`v-for="i in 12"`) can't call `.map()` directly — `12.map(…)`
    // is invalid JS — so they wrap in `Array.from({length: N}, …)`. Other iterables
    // are parenthesised so a trailing numeric literal (`endIndex - startIndex + 1`)
    // can't bind into the following `.map(`.
    let (prefix, suffix): (&str, &str) = if is_numeric_v_for_iterable(iterable) {
        if bare {
            ("Array.from({length: ", "}, (_, __i) => __i + 1)")
        } else {
            ("{Array.from({length: ", "}, (_, __i) => __i + 1)")
        }
    } else if bare {
        ("(", ")")
    } else {
        ("{(", ")")
    };

    let wrapped = resolved.wrapped(prefix, suffix);
    out.prepend_mapped_generated_text(target_pos, &wrapped);
}

/// Resolve the v-for iterable into a [`MappedGeneratedText`] through the shared
/// expression producer.
///
/// The OXC v-for parse is the authority for which iterable identifiers are
/// resolvable references, so the routing tracks its three shapes exactly:
///
/// - **No OXC v-for** (no element, or no v-for on it) → the resolver-only
///   [`resolve_simple_expr_segments`] path, which injects the binding
///   prefix/suffix for a bare identifier.
/// - **OXC v-for present with in-range references** → adapt each reference into
///   the producer's binding inventory and route through
///   [`build_prefixed_expr_segments`].
/// - **OXC v-for present with zero in-range references** (an iterable built only
///   from loop locals, e.g. `v-for="item in item"`) → emit the iterable verbatim;
///   there is no binding to prefix, so prefixing it through the resolver-only path
///   would corrupt the generated bytes.
///
/// This adapter performs NO prefix/suffix logic of its own — every accessor
/// decision is the producer's.
fn resolve_iterable_segments(
    iterable: &str,
    iterable_start: u32,
    oxc_el: Option<&OxcParsedElement<'_>>,
    source: &str,
    resolver: &BindingResolver<'_>,
) -> MappedGeneratedText {
    // No OXC v-for → resolver-only simple-identifier (or pass-through) resolution.
    let Some(v_for) = oxc_el.and_then(|el| el.v_for.as_ref()) else {
        return resolve_simple_expr_segments(resolver, iterable, iterable_start);
    };

    let iterable_end = iterable_start + iterable.len() as u32;

    // External references within the iterable span, in source order. A v-for loop
    // local (e.g. the `item` in `v-for="item in item"`) is filtered out of the
    // reference set, so an iterable built only from locals yields zero references.
    let mut refs: Vec<_> = v_for
        .parsed
        .references
        .iter()
        .filter(|s| s.start >= iterable_start && s.end <= iterable_end)
        .collect();
    refs.sort_by_key(|s| s.start);

    if refs.is_empty() {
        // OXC v-for present with no in-range reference → no binding to prefix. Emit
        // the iterable verbatim as one source-mapped segment with no resolver
        // prefix/suffix; routing it through the resolver-only path would wrongly
        // prefix the bare identifier.
        return MappedGeneratedText::source(iterable, iterable_start);
    }

    // Adapt each external reference into a non-ignored, non-shorthand binding at
    // its source offset, then delegate to the single compound producer.
    let bindings = refs
        .iter()
        .map(|s| Binding {
            name: &source[s.start as usize..s.end as usize],
            span: RelativeSpan::new(
                s.start.saturating_sub(iterable_start),
                s.end.saturating_sub(iterable_start),
            ),
            pos: s.start,
            ignore: false,
            is_shorthand: false,
        })
        .collect();
    let parsed = OxcParsedExpression {
        offset: iterable_start,
        expression: None,
        errors: None,
        bindings: Some(BindingExtractionResult {
            bindings,
            ..Default::default()
        }),
        dynamism: Dynamism::Dynamic,
    };
    build_prefixed_expr_segments(iterable, iterable_start, &parsed, resolver, &[])
}

/// Emit the closing of a v-for map expression.
///
/// Normal mode: `) })}` — closes statement-body, map call, JSX expression.
/// Bare mode (lifted chain branch): `) })` — no outer JSX `}` (parent ternary owns braces).
pub fn emit_v_for_close(el: &ElementNode, _source: &str, out: &mut CodeGenOutput<'_>, bare: bool) {
    let el_end = el
        .tag_close
        .as_ref()
        .map(|tc| tc.end)
        .unwrap_or(el.tag_open.end);

    if bare {
        out.prepend_alloc(el_end, ") })");
    } else {
        out.prepend_alloc(el_end, ") })}");
    }
}

/// Emit v-show as a style attribute.
///
/// Emit unmapped synthetic text at `at` (an `Inserted` chunk → maps to `None`),
/// order-preserving so it interleaves with adjacent mapped value emissions.
#[inline]
fn emit_unmapped<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    at: SourceByteOffset,
    text: &'static str,
) {
    emit_op(
        out,
        &EmitOp::InsertUnmapped {
            at,
            text: EmitText::Static(text),
        },
    );
}

/// `v-show="expr"` → appends `style={{display: expr ? undefined : 'none'}}`
pub fn emit_v_show<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
) {
    // Find v-show prop
    let show_prop = el.props.iter().enumerate().find(|(_, p)| {
        p.is_directive && {
            let name = &source[p.start as usize..p.name_end as usize];
            name == "v-show"
        }
    });

    let (show_idx, show) = match show_prop {
        Some((idx, p)) => (idx, p),
        None => return,
    };

    if let (Some(vs), Some(ve)) = (show.value_start, show.value_end) {
        let prop_end = super::props::get_prop_end(show);

        // The v-show condition is a navigable user expression relocated into a
        // synthetic `style={{ display: … }}` attribute; capture its trimmed source
        // span + bindings so it can be emitted mapped through the typed substrate.
        let (show_tvs, show_tve) = trim_span(source, vs, ve);
        let show_range =
            SourceByteRange::new(SourceByteOffset(show_tvs), SourceByteOffset(show_tve));
        let show_bindings = oxc_el
            .and_then(|e| e.prop(show_idx))
            .and_then(|p| p.exp.as_ref())
            .and_then(|exp| exp.bindings.as_ref())
            .map(|b| b.bindings.as_slice());

        // Check if the element already has a :style binding. If so, merge the
        // v-show display condition into it to avoid duplicate `style` attributes.
        // The parser stores `:style` as directive name `:` (or `v-bind`) with
        // argument `style` in arg_start..arg_end.
        let existing_style = el.props.iter().enumerate().find(|(i, p)| {
            *i != show_idx && p.is_directive && {
                let dir_name = &source[p.start as usize..p.name_end as usize];
                (dir_name == ":" || dir_name == "v-bind")
                    && p.arg_start
                        .zip(p.arg_end)
                        .map(|(a, b)| &source[a as usize..b as usize] == "style")
                        .unwrap_or(false)
            }
        });

        if let Some((style_idx, style_prop)) = existing_style {
            // Merge: remove v-show, transform :style to include display condition.
            // :style="itemStyle" → style={{...(itemStyle), display: expr ? undefined : 'none'}}
            // Both `itemStyle` and the v-show condition stay mapped; the synthetic
            // `style={{...(`, `), display: `, ` ? undefined : 'none'}}` is unmapped.
            out.overwrite(show.start, prop_end, "");

            if let (Some(svs), Some(sve)) = (style_prop.value_start, style_prop.value_end) {
                let (style_tvs, style_tve) = trim_span(source, svs, sve);
                let style_range =
                    SourceByteRange::new(SourceByteOffset(style_tvs), SourceByteOffset(style_tve));
                let style_bindings = oxc_el
                    .and_then(|e| e.prop(style_idx))
                    .and_then(|p| p.exp.as_ref())
                    .and_then(|exp| exp.bindings.as_ref())
                    .map(|b| b.bindings.as_slice());

                let style_end = super::props::get_prop_end(style_prop);
                let at = SourceByteOffset(style_prop.start);
                out.overwrite(style_prop.start, style_end, "");
                emit_unmapped(out, at, "style={{...(");
                emit_relocated_value(
                    out,
                    at,
                    source,
                    style_range,
                    style_bindings,
                    resolver,
                    ExprOptions::default(),
                );
                emit_unmapped(out, at, "), display: ");
                emit_relocated_value(
                    out,
                    at,
                    source,
                    show_range,
                    show_bindings,
                    resolver,
                    ExprOptions::default(),
                );
                emit_unmapped(out, at, " ? undefined : 'none'}}");
            }
        } else {
            // No existing :style — replace v-show with style attribute directly.
            // `style={{display: ` / ` ? undefined : 'none'}}` is unmapped synthetic
            // scaffolding; the v-show condition stays mapped to its source span.
            let at = SourceByteOffset(show.start);
            out.overwrite(show.start, prop_end, "");
            emit_unmapped(out, at, "style={{display: ");
            emit_relocated_value(
                out,
                at,
                source,
                show_range,
                show_bindings,
                resolver,
                ExprOptions::default(),
            );
            emit_unmapped(out, at, " ? undefined : 'none'}}");
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────

/// Emit the v-if/v-else-if condition expression with per-identifier source mapping.
///
/// Instead of emitting the entire IIFE opening as a single unmapped string,
/// decomposes the condition into segments where each binding identifier gets
/// its own `InsertedMapped` chunk. This ensures the source map correctly maps
/// generated identifiers (e.g., `__props.leftArrow`) back to their original
/// template positions (e.g., `leftArrow` in `v-if="leftArrow || leftText"`).
///
/// All segments use `mapped_prepends` (even unmapped gaps) to guarantee correct
/// ordering — `apply_to` merges regular prepends before mapped prepends at the
/// same position, which would break the interleaved order.
#[allow(clippy::too_many_arguments)]
pub fn emit_mapped_condition_expr<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    target_pos: u32,
    prefix: &str,
    suffix: &str,
    vs: u32,
    ve: u32,
    source: &'alloc str,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    resolver: &BindingResolver<'alloc>,
) {
    // The condition resolves through the shared expression producer
    // (`code_gen::expression`) for both the OXC-binding (compound) and
    // resolver-only (simple) cases. Every authored identifier and verbatim run
    // maps to its source span; resolver-injected scaffolding — binding prefixes
    // (`__props.`/`_ctx.`/`$setup.`), the `.value` suffix, keyword brackets,
    // shorthand keys, and the surrounding IIFE `prefix`/`suffix` wrappers —
    // carries no source-map token. The plan keeps each suffix as its own
    // unmapped segment, so a `.value` can never fold into the identifier token.
    // Generated bytes equal the flat `resolve_condition_expr`; only the source
    // map gains per-identifier precision.
    let raw = &source[vs as usize..ve as usize];
    let wrapped =
        resolve_condition_expr_segments(raw, vs, oxc_el, resolver).wrapped(prefix, suffix);
    out.prepend_mapped_generated_text(target_pos, &wrapped);
}

/// Build a fully resolved condition expression for v-if/v-else-if.
/// Public wrapper for use by the condition scope builder.
pub fn resolve_condition_expr_pub(
    raw_expr: &str,
    expr_start: u32,
    oxc_el: Option<&OxcParsedElement<'_>>,
    resolver: &BindingResolver<'_>,
) -> String {
    resolve_condition_expr(raw_expr, expr_start, oxc_el, resolver)
}

/// Build a fully resolved condition expression for v-if/v-else-if.
/// Uses `build_prefixed_expr` to inject binding prefixes into the expression string,
/// instead of positional patches that would conflict with attribute removal.
fn resolve_condition_expr(
    raw_expr: &str,
    expr_start: u32,
    oxc_el: Option<&OxcParsedElement<'_>>,
    resolver: &BindingResolver<'_>,
) -> String {
    if let Some(oxc_el) = oxc_el {
        if let Some(ref cond) = oxc_el.condition {
            return build_prefixed_expr(raw_expr, expr_start, cond, resolver, &[]);
        }
    }
    resolver.resolve_simple_expr(raw_expr)
}

/// Segmented analogue of [`resolve_condition_expr`]: the resolved condition as a
/// [`MappedGeneratedText`] plan, so the no-OXC-binding emission path maps the
/// authored identifier while leaving any injected `__props.` / `.value` /
/// bracket scaffolding unmapped. `.text` is byte-identical to
/// `resolve_condition_expr`.
fn resolve_condition_expr_segments(
    raw_expr: &str,
    expr_start: u32,
    oxc_el: Option<&OxcParsedElement<'_>>,
    resolver: &BindingResolver<'_>,
) -> MappedGeneratedText {
    if let Some(oxc_el) = oxc_el {
        if let Some(ref cond) = oxc_el.condition {
            return build_prefixed_expr_segments(raw_expr, expr_start, cond, resolver, &[]);
        }
    }
    resolve_simple_expr_segments(resolver, raw_expr, expr_start)
}

/// Check if a v-for iterable expression is a pure numeric literal (e.g., "12", "100").
/// Vue supports `v-for="i in 12"` to iterate 1..12, but `12.map(...)` is invalid JS.
fn is_numeric_v_for_iterable(iterable: &str) -> bool {
    let trimmed = iterable.trim();
    !trimmed.is_empty() && trimmed.bytes().all(|b| b.is_ascii_digit())
}

/// Parse a v-for expression into (params, source).
///
/// Handles:
/// - `item in items` → ("item", "items")
/// - `(item, index) in items` → ("item, index", "items")
/// - `item of items` → ("item", "items")
/// - `(value, key, index) in obj` → ("value, key, index", "obj")
fn parse_v_for_expr(expr: &str) -> Option<(&str, &str)> {
    // Find " in " or " of " separator
    let sep_pos = expr
        .find(" in ")
        .map(|pos| (pos, 4)) // " in " is 4 chars
        .or_else(|| expr.find(" of ").map(|pos| (pos, 4))); // " of " is 4 chars

    let (sep_start, sep_len) = sep_pos?;

    let params = expr[..sep_start].trim();
    let source = expr[sep_start + sep_len..].trim();

    // Strip parentheses from params if present
    let params = params
        .strip_prefix('(')
        .and_then(|p| p.strip_suffix(')'))
        .unwrap_or(params);

    Some((params, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_v_for_basic() {
        let (params, source) = parse_v_for_expr("item in items").unwrap();
        assert_eq!(params, "item");
        assert_eq!(source, "items");
    }

    #[test]
    fn parse_v_for_with_index() {
        let (params, source) = parse_v_for_expr("(item, index) in items").unwrap();
        assert_eq!(params, "item, index");
        assert_eq!(source, "items");
    }

    #[test]
    fn parse_v_for_object() {
        let (params, source) = parse_v_for_expr("(value, key, index) in obj").unwrap();
        assert_eq!(params, "value, key, index");
        assert_eq!(source, "obj");
    }

    #[test]
    fn parse_v_for_of() {
        let (params, source) = parse_v_for_expr("item of items").unwrap();
        assert_eq!(params, "item");
        assert_eq!(source, "items");
    }

    #[test]
    fn parse_v_for_numeric() {
        let (params, source) = parse_v_for_expr("n in 10").unwrap();
        assert_eq!(params, "n");
        assert_eq!(source, "10");
    }

    /// Discriminating — `emit_mapped_condition_expr` with no resolvable bindings
    /// segments the synthetic prefix, the source-derived condition, and the
    /// synthetic suffix so the suffix stays UNMAPPED. A single concatenated mapped
    /// op would bleed the condition's `vs` mapping across the synthetic suffix —
    /// the unmapped-token-at-suffix-start assertion fails on that bleed and passes
    /// only for the per-segment emission.
    #[test]
    fn emit_mapped_condition_expr_no_bindings_leaves_suffix_unmapped() {
        let alloc = Allocator::default();
        // Source: condition keyword `true` lives at byte 3.
        let source = "xx true yy";
        let resolver = BindingResolver::new(rustc_hash::FxHashMap::default(), false);

        let mut out = CodeGenOutput::new(&alloc);
        // target_pos 0, prefix `if(`, suffix `){`, vs 3, ve 7 (`true`), no oxc element.
        emit_mapped_condition_expr(&mut out, 0, "if(", "){", 3, 7, source, None, &resolver);

        let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
        out.apply_to(&mut ct);

        // `true` is left unchanged by the empty resolver; bytes are unchanged.
        assert_eq!(ct.build_string(), "if(true){xx true yy");

        let map =
            ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();
        let dump: Vec<_> = tokens
            .iter()
            .map(|t| {
                (
                    t.get_dst_col(),
                    t.get_src_col(),
                    t.get_source_id().is_some(),
                )
            })
            .collect();

        // Condition body: one source token at gen col 3 (after `if(`) → src col 3.
        let body = tokens
            .iter()
            .find(|t| t.get_dst_col() == 3 && t.get_source_id().is_some());
        assert!(body.is_some(), "condition body must map; tokens: {dump:?}");
        assert_eq!(
            body.unwrap().get_src_col(),
            3,
            "`true` must map to src col 3"
        );

        // Discriminating: the synthetic `){` suffix begins at gen col 7 and must be
        // an UNMAPPED segment (its own token, source_id none). A single
        // concatenated mapped op emits no token here.
        assert!(
            tokens
                .iter()
                .any(|t| t.get_dst_col() == 7 && t.get_source_id().is_none()),
            "synthetic suffix must start an unmapped segment; tokens: {dump:?}"
        );
    }

    /// Discriminating — `emit_mapped_condition_expr` with NO OXC bindings but a
    /// resolver-injected prefix (`title` → `__props.title`) maps ONLY the `title`
    /// identifier; the synthetic `__props.` prefix (and the IIFE suffix) stay
    /// unmapped. Mapping the whole resolved string to `vs` would land the source
    /// token on `__props.` instead of `title` — the `[3, 11)`-region negative
    /// assertion fails on that bleed.
    #[test]
    fn emit_mapped_condition_expr_no_bindings_prefixed_prop_maps_identifier_not_prefix() {
        let alloc = Allocator::default();
        // Source: prop `title` lives at byte 3.
        let source = "xx title yy";
        let mut map = rustc_hash::FxHashMap::default();
        map.insert(
            "title" as &str,
            crate::template::code_gen::binding::BindingType::Props,
        );
        // Inline mode → props resolve with the `__props.` prefix.
        let resolver = BindingResolver::new(map, true);

        let mut out = CodeGenOutput::new(&alloc);
        // prefix `if(`, suffix `){`, vs 3, ve 8 (`title`), no oxc element.
        emit_mapped_condition_expr(&mut out, 0, "if(", "){", 3, 8, source, None, &resolver);

        let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
        out.apply_to(&mut ct);

        // Resolver injects the synthetic `__props.` prefix.
        assert_eq!(ct.build_string(), "if(__props.title){xx title yy");

        let map =
            ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();
        let dump: Vec<_> = tokens
            .iter()
            .map(|t| {
                (
                    t.get_dst_col(),
                    t.get_src_col(),
                    t.get_source_id().is_some(),
                )
            })
            .collect();

        // `title` maps at gen col 11 (`if(` = 3 + `__props.` = 8) → src col 3.
        let title = tokens
            .iter()
            .find(|t| t.get_dst_col() == 11 && t.get_source_id().is_some());
        assert!(title.is_some(), "`title` must map; tokens: {dump:?}");
        assert_eq!(
            title.unwrap().get_src_col(),
            3,
            "`title` must map to src col 3, not the synthetic `__props.`"
        );

        // The synthetic `__props.` prefix region [3, 11) carries NO source token.
        assert!(
            !tokens.iter().any(|t| t.get_dst_col() >= 3
                && t.get_dst_col() < 11
                && t.get_source_id().is_some()),
            "synthetic `__props.` prefix must be unmapped; tokens: {dump:?}"
        );
    }

    /// Build a minimal `OxcParsedElement` carrying a single-identifier v-if
    /// condition binding, so `emit_mapped_condition_expr` takes the OXC-binding
    /// (compound-walk) path rather than the resolver-only fallback.
    fn single_ident_cond_element(
        name: &'static str,
        pos: u32,
    ) -> crate::template::oxc::types::OxcParsedElement<'static> {
        use crate::common::RelativeSpan;
        use crate::template::oxc::types::{ExpressionFlag, OxcParsedElement, OxcParsedExpression};
        use crate::utils::oxc::{Binding, BindingExtractionResult, Dynamism};

        let condition = OxcParsedExpression {
            offset: pos,
            expression: None,
            errors: None,
            bindings: Some(BindingExtractionResult {
                bindings: vec![Binding {
                    name,
                    span: RelativeSpan::new(0, name.len() as u32),
                    pos,
                    ignore: false,
                    is_shorthand: false,
                }],
                functions: vec![],
                literals: vec![],
                has_errors: false,
                dynamism: Dynamism::Dynamic,
            }),
            dynamism: Dynamism::Dynamic,
        };
        OxcParsedElement {
            condition: Some(condition),
            v_for: None,
            v_slot: None,
            props: vec![],
            prop_lookup: vec![],
            provided_locals: None,
            expression_flag: ExpressionFlag::empty(),
        }
    }

    /// Discriminating — `emit_mapped_condition_expr` with an OXC condition binding
    /// present (`v-if="count"`, an inline `SetupRef`) maps `count` to its source
    /// span while the resolver-injected `.value` suffix stays UNMAPPED. Folding
    /// `count` + `.value` into one `prepend_alloc_mapped_with_offset` chunk would
    /// leave its mapped token covering the whole `count.value` run, with no
    /// unmapped token at the `.value` boundary — the col-8 assertion fails on that
    /// fold and passes only for the per-segment emission routed through the shared
    /// producer.
    #[test]
    fn emit_mapped_condition_expr_oxc_setup_ref_keeps_value_unmapped() {
        let alloc = Allocator::default();
        // Source: setup ref `count` lives at byte 3.
        let source = "xx count yy";
        let mut map = rustc_hash::FxHashMap::default();
        map.insert(
            "count" as &str,
            crate::template::code_gen::binding::BindingType::SetupRef,
        );
        // Inline mode → a setup ref resolves with the `.value` suffix.
        let resolver = BindingResolver::new(map, true);

        let el = single_ident_cond_element("count", 3);

        let mut out = CodeGenOutput::new(&alloc);
        // prefix `if(`, suffix `){`, vs 3, ve 8 (`count`), with the OXC element.
        emit_mapped_condition_expr(&mut out, 0, "if(", "){", 3, 8, source, Some(&el), &resolver);

        let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
        out.apply_to(&mut ct);

        // Resolver injects the synthetic `.value`; the source bytes are unchanged.
        assert_eq!(ct.build_string(), "if(count.value){xx count yy");

        let map =
            ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();
        let dump: Vec<_> = tokens
            .iter()
            .map(|t| {
                (
                    t.get_dst_col(),
                    t.get_src_col(),
                    t.get_source_id().is_some(),
                )
            })
            .collect();

        // `count` maps at gen col 3 (after `if(`) → src col 3.
        let body = tokens
            .iter()
            .find(|t| t.get_dst_col() == 3 && t.get_source_id().is_some());
        assert!(body.is_some(), "`count` must map; tokens: {dump:?}");
        assert_eq!(
            body.unwrap().get_src_col(),
            3,
            "`count` must map to src col 3, not the synthetic `.value`"
        );

        // Discriminating: the synthetic `.value` begins at gen col 8
        // (`if(` = 3 + `count` = 5) and must START its own UNMAPPED segment. The
        // folded single-chunk emission placed no token at col 8.
        assert!(
            tokens
                .iter()
                .any(|t| t.get_dst_col() == 8 && t.get_source_id().is_none()),
            "synthetic `.value` must start an unmapped segment at col 8; tokens: {dump:?}"
        );

        // No source token anywhere inside the `.value` region [8, 14).
        assert!(
            !tokens.iter().any(|t| t.get_dst_col() >= 8
                && t.get_dst_col() < 14
                && t.get_source_id().is_some()),
            "`.value` region [8, 14) must carry no source token; tokens: {dump:?}"
        );
    }

    /// Build a minimal `OxcParsedElement` carrying a v-for whose iterable holds a
    /// single external reference span, so `emit_mapped_v_for_iterable` takes the
    /// reference-driven (compound) producer path rather than the resolver-only
    /// fallback. Only `v_for.parsed.references` is read by the emitter; the parse
    /// `result` is a minimal placeholder.
    fn single_ref_v_for_element(
        ref_start: u32,
        ref_end: u32,
    ) -> crate::template::oxc::types::OxcParsedElement<'static> {
        use crate::template::oxc::types::{ExpressionFlag, OxcParsedElement, OxcParsedVFor};
        use crate::utils::oxc::vue::{VForParseResult, VForWithBindings};
        use verter_span::Span;

        OxcParsedElement {
            condition: None,
            v_for: Some(OxcParsedVFor {
                parsed: VForWithBindings {
                    result: VForParseResult {
                        left: None,
                        right: None,
                        is_of: false,
                        left_offset: 0,
                        right_offset: 0,
                        left_errors: Vec::new(),
                        right_errors: Vec::new(),
                    },
                    locals: Vec::new(),
                    references: vec![Span::new(ref_start, ref_end)],
                    liveness_reference_names: Vec::new(),
                    scope_local_reference_names: Vec::new(),
                },
            }),
            v_slot: None,
            props: Vec::new(),
            prop_lookup: Vec::new(),
            provided_locals: None,
            expression_flag: ExpressionFlag::empty(),
        }
    }

    /// Discriminating — `emit_mapped_v_for_iterable` with NO OXC v-for prop but a
    /// resolver-injected prefix (`items` → `__props.items`) maps ONLY the `items`
    /// identifier; the synthetic `{(` wrapper and `__props.` prefix stay unmapped.
    /// The old flat branch mapped the whole resolved string at the prefix start —
    /// the `[2, 10)`-region negative assertion fails on that fold.
    #[test]
    fn emit_mapped_v_for_iterable_no_oxc_prop_maps_identifier_not_prefix() {
        let alloc = Allocator::default();
        // Source: iterable `items` lives at byte 11 (`xx item in items yy`).
        let source = "xx item in items yy";
        let mut map = rustc_hash::FxHashMap::default();
        map.insert(
            "items" as &str,
            crate::template::code_gen::binding::BindingType::Props,
        );
        // Inline mode → props resolve with the `__props.` prefix.
        let resolver = BindingResolver::new(map, true);

        let mut out = CodeGenOutput::new(&alloc);
        // v-for value `item in items` spans [3, 16); iterable `items` is at 11.
        emit_mapped_v_for_iterable(
            &mut out,
            0,
            "items",
            3,
            "item in items",
            None,
            source,
            false,
            &resolver,
        );

        let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
        out.apply_to(&mut ct);

        // Iterable head bytes unchanged: `{(__props.items)`.
        assert!(
            ct.build_string().starts_with("{(__props.items)"),
            "got: {}",
            ct.build_string()
        );

        let map =
            ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();
        let dump: Vec<_> = tokens
            .iter()
            .map(|t| {
                (
                    t.get_dst_col(),
                    t.get_src_col(),
                    t.get_source_id().is_some(),
                )
            })
            .collect();

        // `items` maps at gen col 10 (`{(` = 2 + `__props.` = 8) → src col 11.
        let items = tokens
            .iter()
            .find(|t| t.get_dst_col() == 10 && t.get_source_id().is_some());
        assert!(items.is_some(), "`items` must map; tokens: {dump:?}");
        assert_eq!(
            items.unwrap().get_src_col(),
            11,
            "`items` must map to src col 11, not the synthetic `__props.`"
        );

        // The synthetic `{(__props.` region [2, 10) carries NO source token.
        assert!(
            !tokens.iter().any(|t| t.get_dst_col() >= 2
                && t.get_dst_col() < 10
                && t.get_source_id().is_some()),
            "synthetic `{{(__props.` prefix must be unmapped; tokens: {dump:?}"
        );
    }

    /// Build a minimal `OxcParsedElement` carrying a v-for whose iterable holds NO
    /// external references — the shape OXC produces when the iterable's only
    /// identifier is also the v-for loop local (`v-for="item in item"`), which is
    /// filtered out of the reference set. The emitter sees an OXC v-for present
    /// with zero in-range references.
    fn no_ref_v_for_element() -> crate::template::oxc::types::OxcParsedElement<'static> {
        use crate::template::oxc::types::{ExpressionFlag, OxcParsedElement, OxcParsedVFor};
        use crate::utils::oxc::vue::{VForParseResult, VForWithBindings};

        OxcParsedElement {
            condition: None,
            v_for: Some(OxcParsedVFor {
                parsed: VForWithBindings {
                    result: VForParseResult {
                        left: None,
                        right: None,
                        is_of: false,
                        left_offset: 0,
                        right_offset: 0,
                        left_errors: Vec::new(),
                        right_errors: Vec::new(),
                    },
                    locals: Vec::new(),
                    references: Vec::new(),
                    liveness_reference_names: Vec::new(),
                    scope_local_reference_names: Vec::new(),
                },
            }),
            v_slot: None,
            props: Vec::new(),
            prop_lookup: Vec::new(),
            provided_locals: None,
            expression_flag: ExpressionFlag::empty(),
        }
    }

    /// Discriminating — `emit_mapped_v_for_iterable` with an OXC v-for PRESENT but
    /// ZERO in-range references must emit the iterable VERBATIM, never routing it
    /// through the resolver-only path. Here `items` is a registered prop, so the
    /// resolver-only path would prefix it to `__props.items`; the zero-reference
    /// v-for sub-case must instead keep the bare `{(items)` (matching a verbatim
    /// patch over an empty reference set) and map the identifier to its source
    /// offset with no synthetic prefix.
    #[test]
    fn emit_mapped_v_for_iterable_oxc_present_zero_refs_emits_verbatim_not_prefixed() {
        let alloc = Allocator::default();
        // Source: iterable `items` lives at byte 11 (`xx item in items yy`).
        let source = "xx item in items yy";
        let mut map = rustc_hash::FxHashMap::default();
        map.insert(
            "items" as &str,
            crate::template::code_gen::binding::BindingType::Props,
        );
        // Inline mode → a prop would resolve with the `__props.` prefix IF routed
        // through the resolver-only path.
        let resolver = BindingResolver::new(map, true);

        // OXC v-for present but carrying zero references (loop-local-shadowing shape).
        let el = no_ref_v_for_element();

        let mut out = CodeGenOutput::new(&alloc);
        emit_mapped_v_for_iterable(
            &mut out,
            0,
            "items",
            3,
            "item in items",
            Some(&el),
            source,
            false,
            &resolver,
        );

        let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
        out.apply_to(&mut ct);

        // Iterable head bytes must be VERBATIM `{(items)` — no `__props.` prefix.
        let built = ct.build_string();
        assert!(
            built.starts_with("{(items)"),
            "zero-ref v-for iterable must stay verbatim `{{(items)`, got: {built}"
        );
        assert!(
            !built.contains("__props."),
            "zero-ref v-for iterable must carry no resolver prefix, got: {built}"
        );

        let map =
            ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();
        let dump: Vec<_> = tokens
            .iter()
            .map(|t| {
                (
                    t.get_dst_col(),
                    t.get_src_col(),
                    t.get_source_id().is_some(),
                )
            })
            .collect();

        // `items` maps at gen col 2 (after `{(`) → src col 11 (the authored offset).
        let body = tokens
            .iter()
            .find(|t| t.get_dst_col() == 2 && t.get_source_id().is_some());
        assert!(body.is_some(), "`items` must map; tokens: {dump:?}");
        assert_eq!(
            body.unwrap().get_src_col(),
            11,
            "`items` must map to src col 11"
        );
    }

    /// Discriminating — `emit_mapped_v_for_iterable` with an OXC v-for reference
    /// over a setup ref (`todos`, inline → `.value`) maps `todos` to its source
    /// span while the resolver-injected `.value` suffix stays UNMAPPED. The old
    /// per-identifier fold concatenated `todos` + `.value` into one mapped chunk,
    /// leaving no unmapped token at the `.value` boundary — the col-7 assertion
    /// fails on that fold and passes only for the per-segment producer emission.
    #[test]
    fn emit_mapped_v_for_iterable_oxc_setup_ref_keeps_value_unmapped() {
        let alloc = Allocator::default();
        // Source: iterable `todos` (a setup ref) lives at byte 11.
        let source = "xx item in todos yy";
        let mut map = rustc_hash::FxHashMap::default();
        map.insert(
            "todos" as &str,
            crate::template::code_gen::binding::BindingType::SetupRef,
        );
        // Inline mode → a setup ref resolves with the `.value` suffix.
        let resolver = BindingResolver::new(map, true);

        // v-for OXC element carrying one external reference span over `todos` [11, 16).
        let el = single_ref_v_for_element(11, 16);

        let mut out = CodeGenOutput::new(&alloc);
        emit_mapped_v_for_iterable(
            &mut out,
            0,
            "todos",
            3,
            "item in todos",
            Some(&el),
            source,
            false,
            &resolver,
        );

        let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
        out.apply_to(&mut ct);

        // `.value` injected; iterable head bytes: `{(todos.value)`.
        assert!(
            ct.build_string().starts_with("{(todos.value)"),
            "got: {}",
            ct.build_string()
        );

        let map =
            ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();
        let dump: Vec<_> = tokens
            .iter()
            .map(|t| {
                (
                    t.get_dst_col(),
                    t.get_src_col(),
                    t.get_source_id().is_some(),
                )
            })
            .collect();

        // `todos` maps at gen col 2 (after `{(`) → src col 11.
        let body = tokens
            .iter()
            .find(|t| t.get_dst_col() == 2 && t.get_source_id().is_some());
        assert!(body.is_some(), "`todos` must map; tokens: {dump:?}");
        assert_eq!(
            body.unwrap().get_src_col(),
            11,
            "`todos` must map to src col 11, not the synthetic `.value`"
        );

        // Discriminating: the synthetic `.value` begins at gen col 7
        // (`{(` = 2 + `todos` = 5) and must START its own UNMAPPED segment.
        assert!(
            tokens
                .iter()
                .any(|t| t.get_dst_col() == 7 && t.get_source_id().is_none()),
            "synthetic `.value` must start an unmapped segment at col 7; tokens: {dump:?}"
        );

        // No source token anywhere inside the `.value` region [7, 13).
        assert!(
            !tokens.iter().any(|t| t.get_dst_col() >= 7
                && t.get_dst_col() < 13
                && t.get_source_id().is_some()),
            "`.value` region [7, 13) must carry no source token; tokens: {dump:?}"
        );
    }
}
