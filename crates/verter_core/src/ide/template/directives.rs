//! Structural directive → JSX constructs for TSX template codegen.
//!
//! Converts Vue structural directives to JSX-compatible constructs:
//! - `v-if="cond"` → `{()=>{if(cond){...}}}` (IIFE for TypeScript control flow narrowing)
//! - `v-else-if="cond"` → `}else if(cond){...}` (chained in same IIFE)
//! - `v-else` → `}else{...}}}` (final branch + IIFE close)
//! - `v-for="item in items"` → `{items.map((item) => (...))}`
//! - `v-show="expr"` → `style={{display: expr ? undefined : 'none'}}`

use oxc_allocator::Allocator;

use crate::ast::types::{ElementNode, ElementNodeConditionKind};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
use crate::template::oxc::types::OxcParsedElement;

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
pub fn emit_v_for_open<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
    is_jsx: bool,
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

            // Emit ") => (" — unmapped tail
            let map_close = ") => (";
            out.prepend_alloc_mapped_with_offset(target_pos, 0, map_close.len() as u32, map_close);
        }
    }
}

/// Emit the iterable part of v-for with per-identifier source mapping.
///
/// Follows the same pattern as `emit_mapped_condition_expr` for v-if:
/// each binding identifier in the iterable gets its own mapped chunk so
/// the source map correctly links back to the original template positions.
#[allow(clippy::too_many_arguments)]
fn emit_mapped_v_for_iterable<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    target_pos: u32,
    iterable: &str,
    v_for_value_start: u32,
    v_for_full_expr: &str,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &str,
    resolver: &BindingResolver<'alloc>,
) {
    // Calculate absolute position of the iterable in the source
    let iterable_offset_in_expr = v_for_full_expr.find(iterable).unwrap_or(0);
    let iterable_start = v_for_value_start + iterable_offset_in_expr as u32;
    let iterable_end = iterable_start + iterable.len() as u32;

    // Get OXC references for per-identifier mapping
    let references = oxc_el.and_then(|el| el.v_for.as_ref()).map(|vf| {
        let mut refs: Vec<_> = vf
            .parsed
            .references
            .iter()
            .filter(|s| s.start >= iterable_start && s.end <= iterable_end)
            .collect();
        refs.sort_by_key(|s| s.start);
        refs
    });

    let has_refs = references.as_ref().is_some_and(|r| !r.is_empty());

    if !has_refs {
        // No resolvable bindings — emit with single mapping point at iterable start
        let resolved = resolve_v_for_iterable(
            iterable,
            v_for_value_start,
            v_for_full_expr,
            oxc_el,
            source,
            resolver,
        );
        // Numeric literals (e.g., `v-for="i in 12"`) can't have `.map()` called
        // directly — `12.map(...)` is invalid JS. Wrap in Array.from() to produce
        // a valid iterable with correct number[] type.
        // Non-numeric expressions are wrapped in parens to prevent parse errors
        // when the expression ends with a number (e.g., `endIndex - startIndex + 1`
        // would become `+ 1.map(...)` without parens).
        let content = if is_numeric_v_for_iterable(iterable) {
            format!(
                "{{Array.from({{length: {}}}, (_, __i) => __i + 1)",
                resolved
            )
        } else {
            format!("{{({}", resolved)
        };
        // Map to the iterable start position (offset 2 skips the `{(` prefix)
        let offset = if is_numeric_v_for_iterable(iterable) {
            1
        } else {
            2
        };
        out.prepend_alloc_mapped_with_offset(target_pos, iterable_start, offset, &content);
        // Emit closing paren as unmapped synthetic text.
        // Use mapped_with_offset (offset=len) to stay in the mapped_prepends vec,
        // preserving call-order at the same position (regular prepends merge before mapped).
        if !is_numeric_v_for_iterable(iterable) {
            out.prepend_alloc_mapped_with_offset(target_pos, 0, 1, ")");
        }
        return;
    }

    let refs = references.unwrap();

    // Build per-identifier mapped segments (same pattern as emit_mapped_condition_expr)
    let mut cursor = iterable_start;
    let mut first = true;

    for span in &refs {
        let gap = if span.start > cursor {
            &source[cursor as usize..span.start as usize]
        } else {
            ""
        };

        let name = &source[span.start as usize..span.end as usize];
        let bind_prefix = resolver.resolve_prefix(name);
        let bind_suffix = resolver.resolve_suffix(name);

        // Wrap in parens to prevent parse errors with trailing numeric literals
        // e.g., `endIndex - startIndex + 1` → `(endIndex - startIndex + 1).map(...)`
        let prefix = if first { "{(" } else { "" };
        first = false;

        let content = format!("{}{}{}{}{}", prefix, gap, bind_prefix, name, bind_suffix);
        let content_offset = (prefix.len() + gap.len() + bind_prefix.len()) as u32;

        out.prepend_alloc_mapped_with_offset(target_pos, span.start, content_offset, &content);
        cursor = span.end;
    }

    // Remaining iterable text after last reference.
    // Map the start to `cursor` so positions within the tail have correct source mapping.
    if cursor < iterable_end {
        let remaining = &source[cursor as usize..iterable_end as usize];
        out.prepend_alloc_mapped_with_offset(target_pos, cursor, 0, remaining);
    }
    // Emit closing paren as unmapped synthetic text.
    // Use mapped_with_offset (offset=len) to stay in mapped_prepends vec for correct ordering.
    out.prepend_alloc_mapped_with_offset(target_pos, 0, 1, ")");
}

/// Emit the closing of a v-for map expression.
pub fn emit_v_for_close(el: &ElementNode, _source: &str, out: &mut CodeGenOutput<'_>) {
    let el_end = el
        .tag_close
        .as_ref()
        .map(|tc| tc.end)
        .unwrap_or(el.tag_open.end);

    out.prepend_alloc(el_end, "))}");
}

/// Emit v-if as a ternary expression (for use inside v-for expression body where IIFE is invalid).
///
/// `v-if="cond"` → `cond ?\n`
///
/// When v-if and v-for coexist on the same element, the v-for emits `items.map((item) => (...))`.
/// An IIFE `{()=>{if(cond){...}}}` inside the parenthesized expression body is parsed as an
/// object literal → invalid JSX. A ternary is valid in that context.
pub fn emit_v_if_ternary_open<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
) {
    let condition = match &el.v_condition {
        Some(c) => c,
        None => return,
    };

    if condition.kind != ElementNodeConditionKind::If {
        return;
    }

    if let (Some(vs), Some(ve)) = (condition.prop.value_start, condition.prop.value_end) {
        emit_mapped_condition_expr(
            out,
            el.tag_open.start,
            "",
            " ?\n",
            vs,
            ve,
            source,
            oxc_el,
            resolver,
        );
    }
}

/// Close the v-if ternary: `\n: null`
pub fn emit_v_if_ternary_close(el: &ElementNode, out: &mut CodeGenOutput<'_>) {
    let el_end = el
        .tag_close
        .as_ref()
        .map(|tc| tc.end)
        .unwrap_or(el.tag_open.end);

    out.prepend_alloc(el_end, "\n: null");
}

/// Emit v-show as a style attribute.
///
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
        let expr = &source[vs as usize..ve as usize];
        let prop_end = super::props::get_prop_end(show);

        // Build fully resolved expression BEFORE overwrite to avoid orphaning
        // binding patches (same pattern as v-if/v-for).
        let resolved_expr = if let Some(oxc_el) = oxc_el {
            if let Some(oxc_prop) = oxc_el.props.iter().find(|p| p.prop_index == show_idx) {
                if let Some(ref exp) = oxc_prop.exp {
                    build_prefixed_expr(expr, vs, exp, resolver, &[])
                } else {
                    resolver.resolve_simple_expr(expr)
                }
            } else {
                resolver.resolve_simple_expr(expr)
            }
        } else {
            resolver.resolve_simple_expr(expr)
        };

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
            // :style="itemStyle" → style={{...itemStyle, display: expr ? undefined : 'none'}}
            out.overwrite(show.start, prop_end, "");

            if let (Some(svs), Some(sve)) = (style_prop.value_start, style_prop.value_end) {
                let style_expr = &source[svs as usize..sve as usize];
                let resolved_style = if let Some(oxc_el) = oxc_el {
                    if let Some(oxc_prop) = oxc_el.props.iter().find(|p| p.prop_index == style_idx)
                    {
                        if let Some(ref exp) = oxc_prop.exp {
                            build_prefixed_expr(style_expr, svs, exp, resolver, &[])
                        } else {
                            resolver.resolve_simple_expr(style_expr)
                        }
                    } else {
                        resolver.resolve_simple_expr(style_expr)
                    }
                } else {
                    resolver.resolve_simple_expr(style_expr)
                };

                let style_end = super::props::get_prop_end(style_prop);
                out.overwrite(
                    style_prop.start,
                    style_end,
                    &format!(
                        "style={{{{...({resolved_style}), display: {resolved_expr} ? undefined : 'none'}}}}",
                    ),
                );
            }
        } else {
            // No existing :style — replace v-show with style attribute directly.
            out.overwrite(
                show.start,
                prop_end,
                &format!(
                    "style={{{{display: {} ? undefined : 'none'}}}}",
                    resolved_expr
                ),
            );
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
fn emit_mapped_condition_expr<'alloc>(
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
    // Get OXC bindings for the condition expression
    let bindings = oxc_el
        .and_then(|el| el.condition.as_ref())
        .and_then(|cond| cond.bindings.as_ref());

    let cond_bindings = bindings.map(|b| {
        let mut sorted: Vec<_> = b.bindings.iter().filter(|b| !b.ignore).collect();
        sorted.sort_by_key(|b| b.pos);
        sorted
    });

    let has_bindings = cond_bindings
        .as_ref()
        .is_some_and(|bindings| !bindings.is_empty());

    if !has_bindings {
        // No resolvable bindings — still use mapped emission so the expression
        // has source map tokens for hover/diagnostics position mapping.
        let raw = &source[vs as usize..ve as usize];
        let resolved = resolve_condition_expr(raw, vs, oxc_el, resolver);
        let content = format!("{}{}{}", prefix, resolved, suffix);
        let content_offset = prefix.len() as u32;
        out.prepend_alloc_mapped_with_offset(target_pos, vs, content_offset, &content);
        return;
    }

    let sorted = cond_bindings.unwrap();

    // Build per-identifier mapped segments.
    // Each segment = [gap_text][binding_prefix][binding_name][binding_suffix]
    // The first segment also includes the IIFE prefix in the gap.
    let mut cursor = vs;
    let mut first = true;

    for binding in &sorted {
        // Gap text from the original source between the previous binding end and this one
        let gap = if binding.pos > cursor {
            &source[cursor as usize..binding.pos as usize]
        } else {
            ""
        };

        let bind_prefix = resolver.resolve_prefix(binding.name);
        let bind_suffix = resolver.resolve_suffix(binding.name);

        // Build content: [iife_prefix][gap][bind_prefix][name][bind_suffix]
        let iife_prefix = if first { prefix } else { "" };
        first = false;

        let content = format!(
            "{}{}{}{}{}",
            iife_prefix, gap, bind_prefix, binding.name, bind_suffix
        );

        // content_offset points to the start of `name` within content
        let content_offset = (iife_prefix.len() + gap.len() + bind_prefix.len()) as u32;

        out.prepend_alloc_mapped_with_offset(target_pos, binding.pos, content_offset, &content);

        cursor = binding.pos + binding.name.len() as u32;
    }

    // Remaining source text after last binding — mapped to `cursor` so that
    // positions within the tail (e.g., `===` in ` && 1 ===2`) have a source map
    // token for interpolation back to the Vue source.
    let remaining = if cursor < ve {
        &source[cursor as usize..ve as usize]
    } else {
        ""
    };

    if !remaining.is_empty() {
        // Map remaining source text: content_offset = 0 → source position `cursor`
        out.prepend_alloc_mapped_with_offset(target_pos, cursor, 0, remaining);
    }

    // Suffix (e.g., "){\n") is synthetic — emit unmapped to avoid false source
    // positions past the expression end.
    let suffix_len = suffix.len() as u32;
    out.prepend_alloc_mapped_with_offset(target_pos, 0, suffix_len, suffix);
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

/// Build a fully resolved iterable expression for v-for.
/// Resolves binding prefixes for all identifier references in the iterable part.
fn resolve_v_for_iterable(
    iterable: &str,
    v_for_value_start: u32,
    v_for_full_expr: &str,
    oxc_el: Option<&OxcParsedElement<'_>>,
    source: &str,
    resolver: &BindingResolver<'_>,
) -> String {
    if let Some(oxc_el) = oxc_el {
        if let Some(ref v_for) = oxc_el.v_for {
            // Calculate the byte offset where the iterable starts within the source
            let iterable_offset_in_expr = v_for_full_expr.find(iterable).unwrap_or(0);
            let iterable_start = v_for_value_start + iterable_offset_in_expr as u32;
            let iterable_end = iterable_start + iterable.len() as u32;

            // Build resolved string by applying prefixes to references within iterable range
            let mut result = iterable.to_string();
            // Collect patches sorted by position (reverse order for safe insertion)
            let mut patches: Vec<(usize, &str, &str)> = Vec::new(); // (offset_in_iterable, prefix, suffix)
            for span in &v_for.parsed.references {
                if span.start >= iterable_start && span.end <= iterable_end {
                    let name = &source[span.start as usize..span.end as usize];
                    let prefix = resolver.resolve_prefix(name);
                    let suffix = resolver.resolve_suffix(name);
                    if !prefix.is_empty() || !suffix.is_empty() {
                        let offset = (span.start - iterable_start) as usize;
                        patches.push((offset, prefix, suffix));
                    }
                }
            }
            // Apply in reverse order to maintain correct positions
            patches.sort_by(|a, b| b.0.cmp(&a.0));
            for (offset, prefix, suffix) in patches {
                let name_len = {
                    result[offset..]
                        .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
                        .unwrap_or(result.len() - offset)
                };
                if !suffix.is_empty() {
                    result.insert_str(offset + name_len, suffix);
                }
                if !prefix.is_empty() {
                    result.insert_str(offset, prefix);
                }
            }
            return result;
        }
    }
    resolver.resolve_simple_expr(iterable)
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
}
