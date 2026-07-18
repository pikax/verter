//! AST-to-OXC template expression parsing pass.
//!
//! Converts the arena-based [`TemplateAst`] nodes into OXC-parsed equivalents
//! in a single forward pass over the nodes vec. Each node gets a corresponding
//! [`OxcNodeData`] entry with parsed expressions, extracted bindings, and
//! dynamism classification.

pub(crate) mod slot_summary;
pub mod types;

use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::common::Span;
use crate::utils::oxc::{
    extract_bindings_from_expression, vue::adjust_diagnostics_spans, BindingContext,
};

use self::types::*;

/// Parse a single expression from a source span.
///
/// Returns an [`OxcParsedExpression`] with:
/// - Substring-relative AST spans (not adjusted to file positions)
/// - File-relative binding positions (via `BindingContext::base_offset`)
/// - File-relative diagnostic spans (adjusted for error reporting)
/// - [`Dynamism`] classification based on binding analysis
fn parse_expression<'alloc>(
    span: Span,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ignored: &[&'alloc str],
    ide_completion: bool,
) -> OxcParsedExpression<'alloc> {
    let source_slice = &input[span.start as usize..span.end as usize];
    // A whitespace-only span (`{{ }}`, `{{   }}`) is an EMPTY interpolation, not a
    // parse error: it references nothing and must NOT mark template liveness
    // incomplete. Handle it exactly like a zero-width span so `errors` stays `None`
    // (the gate stays closed and a genuinely-unused binding is still demoted),
    // instead of feeding OXC a blank slice it rejects as a syntax error.
    if span.start >= span.end || source_slice.trim().is_empty() {
        return OxcParsedExpression {
            offset: span.start,
            expression: None,
            errors: None,
            bindings: None,
            dynamism: Dynamism::Static,
        };
    }

    let parser = oxc_parser::Parser::new(alloc, source_slice, source_type);

    match parser.parse_expression() {
        Ok(expr) => {
            // Don't adjust expression AST spans — keep substring-relative.
            // Bindings get file-relative positions via base_offset.
            // Dynamism is computed incrementally during extraction.
            let binding_ctx = BindingContext::with_ignored(span.start, ignored.iter().copied())
                .completion_aware(ide_completion);
            let bindings = extract_bindings_from_expression(&expr, source_slice, binding_ctx);

            OxcParsedExpression {
                offset: span.start,
                expression: Some(expr),
                errors: None,
                dynamism: bindings.dynamism,
                bindings: Some(bindings),
            }
        }
        Err(mut errors) => {
            adjust_diagnostics_spans(&mut errors, span.start);
            OxcParsedExpression {
                offset: span.start,
                expression: None,
                errors: Some(errors),
                bindings: None,
                dynamism: Dynamism::Static,
            }
        }
    }
}

use crate::ast::types::{
    AstNodeKind, ChildrenFlags, ElementNode, ElementNodeConditionKind, TemplateAst,
};
use crate::utils::oxc::vue::{parse_vfor_with_bindings_sliced, parse_vslot_with_bindings_sliced};

/// Parse all expressions on a single element node.
///
/// Processes structural directives (v-if/v-for/v-slot) and regular props
/// in Vue priority order. Accumulates provided locals from v-for/v-slot
/// for children. Computes [`ExpressionFlag`] for codegen optimization.
///
/// Returns an [`OxcParsedElement`] with parsed expressions, provided locals,
/// and expression flags.
fn parse_element<'alloc>(
    element: &ElementNode,
    parent_ignored: &[&'alloc str],
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ide_completion: bool,
) -> OxcParsedElement<'alloc> {
    // Fast path: plain element with no directives → empty result.
    // Plain elements carry only static attributes, so no prop produces a parsed
    // expression and the dense lookup is empty — `OxcParsedElement::prop` returns
    // `None` for every index regardless.
    if element.is_plain() {
        return OxcParsedElement {
            condition: None,
            v_for: None,
            v_slot: None,
            props: Vec::new(),
            prop_lookup: Vec::new(),
            provided_locals: None,
            expression_flag: ExpressionFlag::empty(),
        };
    }

    let mut expression_flag = ExpressionFlag::empty();
    let has_scoping_directives = element.v_for.is_some() || element.v_slot.is_some();

    // Only clone parent_ignored when we need a mutable Vec to push v-for/v-slot
    // locals into. Most elements have neither, so this avoids the Vec allocation.
    let mut owned_locals: Option<Vec<&'alloc str>> = if has_scoping_directives {
        Some(parent_ignored.to_vec())
    } else {
        None
    };

    // Active locals: either the owned mutable Vec or the parent slice.
    // Use a macro to avoid borrow-checker issues with conditional references.
    macro_rules! active_locals {
        () => {
            match &owned_locals {
                Some(v) => v.as_slice(),
                None => parent_ignored,
            }
        };
    }

    // ── 1. v-if / v-else-if condition ───────────────────────────
    let condition = match &element.v_condition {
        Some(cond) if !matches!(cond.kind, ElementNodeConditionKind::Else) => {
            if let (Some(vs), Some(ve)) = (cond.prop.value_start, cond.prop.value_end) {
                let parsed = parse_expression(
                    Span::new(vs, ve),
                    input,
                    alloc,
                    source_type,
                    active_locals!(),
                    ide_completion,
                );
                if parsed.dynamism == Dynamism::Static {
                    expression_flag = expression_flag.add(ExpressionFlags::StaticCondition);
                }
                Some(parsed)
            } else {
                None // v-if/v-else-if with no value (malformed)
            }
        }
        _ => None, // v-else (no expression) or no condition
    };

    // ── 2. v-for ────────────────────────────────────────────────
    let v_for = match &element.v_for {
        Some(prop) => {
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let parsed = parse_vfor_with_bindings_sliced(
                    alloc,
                    Span::new(vs, ve),
                    input,
                    source_type,
                    active_locals!(),
                );
                // Add v-for locals to owned_locals for subsequent parsing
                let locals = owned_locals.as_mut().unwrap();
                for local_span in &parsed.locals {
                    locals.push(local_span.slice(input));
                }
                Some(OxcParsedVFor { parsed })
            } else {
                None
            }
        }
        None => None,
    };

    // ── 3. v-slot ───────────────────────────────────────────────
    let v_slot = match &element.v_slot {
        Some(prop) => {
            let slot_span = match (prop.value_start, prop.value_end) {
                (Some(vs), Some(ve)) => Some(Span::new(vs, ve)),
                _ => None,
            };
            // Dynamic slot NAME (`#[expr]`): parse the inner expression in
            // the scope OUTSIDE the slot — enclosing v-for aliases apply
            // (already in active locals), the slot's own params do NOT (the
            // name computes before they bind), so this parse runs BEFORE
            // the params push locals below.
            let dynamic_name = if prop.is_dynamic == Some(true) {
                match (prop.arg_start, prop.arg_end) {
                    (Some(as_), Some(ae)) if ae > as_ => {
                        let raw = &input[as_ as usize..ae as usize];
                        let (start, end) = if raw.starts_with('[') && raw.ends_with(']') {
                            (as_ + 1, ae - 1)
                        } else {
                            (as_, ae)
                        };
                        (end > start).then(|| {
                            parse_expression(
                                Span::new(start, end),
                                input,
                                alloc,
                                source_type,
                                active_locals!(),
                                ide_completion,
                            )
                        })
                    }
                    _ => None,
                }
            } else {
                None
            };
            let parsed = parse_vslot_with_bindings_sliced(
                alloc,
                slot_span,
                input,
                source_type,
                active_locals!(),
            );
            // Add v-slot locals to owned_locals
            let locals = owned_locals.as_mut().unwrap();
            for local_span in &parsed.locals {
                locals.push(local_span.slice(input));
            }
            Some(OxcParsedVSlot {
                parsed,
                dynamic_name,
            })
        }
        None => None,
    };

    // ── 4. Regular props ────────────────────────────────────────
    // `oxc_props` is sparse (only directives with parsed expressions); `prop_lookup`
    // is dense over the FULL `ElementNode.props`, mapping each prop index to its slot
    // in `oxc_props` (or `None` for static attrs / value-less directives). This is the
    // O(1) correlation table consumers query via `OxcParsedElement::prop`.
    let mut oxc_props: Vec<OxcParsedProp<'alloc>> = Vec::with_capacity(element.props.len());
    let mut prop_lookup: Vec<Option<u32>> = vec![None; element.props.len()];

    for (i, prop) in element.props.iter().enumerate() {
        if !prop.is_directive {
            // Static attribute — no OXC parsing needed.
            continue;
        }

        // Parse directive value expression
        let exp = match (prop.value_start, prop.value_end) {
            (Some(vs), Some(ve)) => {
                let parsed = parse_expression(
                    Span::new(vs, ve),
                    input,
                    alloc,
                    source_type,
                    active_locals!(),
                    ide_completion,
                );

                // Check for expression flag based on arg name
                if parsed.dynamism == Dynamism::Static {
                    if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                        let arg_name = &input[as_ as usize..ae as usize];
                        match arg_name {
                            "class" => {
                                expression_flag =
                                    expression_flag.add(ExpressionFlags::StaticClassExpr);
                            }
                            "style" => {
                                expression_flag =
                                    expression_flag.add(ExpressionFlags::StaticStyleExpr);
                            }
                            "key" => {
                                expression_flag =
                                    expression_flag.add(ExpressionFlags::StaticKeyExpr);
                            }
                            _ => {}
                        }
                    }
                }

                Some(parsed)
            }
            _ => None,
        };

        // Parse dynamic arg expression (:[key]="value")
        let arg = match (prop.is_dynamic, prop.arg_start, prop.arg_end) {
            (Some(true), Some(as_), Some(ae)) => Some(parse_expression(
                Span::new(as_, ae),
                input,
                alloc,
                source_type,
                active_locals!(),
                ide_completion,
            )),
            _ => None,
        };

        // Only include props that have something parsed
        if exp.is_some() || arg.is_some() {
            prop_lookup[i] = Some(oxc_props.len() as u32);
            oxc_props.push(OxcParsedProp {
                prop_index: i,
                arg,
                exp,
            });
        }
    }

    OxcParsedElement {
        condition,
        v_for,
        v_slot,
        props: oxc_props,
        prop_lookup,
        provided_locals: owned_locals,
        expression_flag,
    }
}

/// Parse all template expressions in a single forward pass over the AST nodes vec.
///
/// Produces a parallel `OxcParsedAst` where `data[node_id.0]` contains the
/// OXC-parsed data for `ast.nodes[node_id.0]`. Parents are always at lower
/// indices than their children (allocated at `open_element`), so a forward
/// scan guarantees parent data is available when processing children.
///
/// Scope cascade: v-for/v-slot locals from a parent element are propagated
/// to children via `provided_locals`. Sibling elements do NOT share scopes.
///
/// `AllInterpolationsStatic` is set optimistically on elements with
/// interpolation children, then removed if any interpolation is non-Static.
///
/// `ide_completion` enables completion-prefix matching for v-for / v-slot scope
/// locals (see [`BindingContext`]). IDE/TSX codegen passes `true`; runtime
/// (VDOM / Vapor) codegen passes `false` so partial identifiers stay real
/// references and the per-binding prefix scan is skipped.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn parse_template_expressions<'alloc>(
    ast: &TemplateAst,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ide_completion: bool,
) -> OxcParsedAst<'alloc> {
    #[cfg(test)]
    {
        PARSE_TEMPLATE_EXPRESSIONS_CALLS.with(|c| c.set(c.get() + 1));
        // Record the EXACT SourceType discriminant OXC parses with on this
        // call, so tests can prove a JS SFC's TSX lane parses with `jsx()`
        // (is_typescript = false) while its runtime lane uses `tsx()`.
        PARSE_TEMPLATE_EXPRESSIONS_SOURCE_TYPES.with(|v| {
            v.borrow_mut()
                .push((source_type.is_typescript(), source_type.is_jsx()))
        });
    }

    let mut data: Vec<OxcNodeData<'alloc>> = Vec::with_capacity(ast.nodes.len());

    for node in &ast.nodes {
        // Get nearest ancestor's provided_locals for scope cascade.
        // Parents always have lower indices, so data[pid.0] is already populated.
        // Walk up past OxcNodeData::None entries (plain elements that skipped
        // expression parsing) to find the nearest ancestor with scope info.
        let parent_locals: &[&'alloc str] = {
            let mut ancestor = node.parent;
            loop {
                match ancestor {
                    Some(pid) => match &data[pid.0] {
                        // Only stop at elements that added v-for/v-slot locals.
                        // Elements with `None` have no locals of their own — walk through.
                        OxcNodeData::Element(el) => match &el.provided_locals {
                            Some(locals) => break locals.as_slice(),
                            None => ancestor = ast.nodes[pid.0].parent,
                        },
                        _ => ancestor = ast.nodes[pid.0].parent,
                    },
                    None => break &[],
                }
            }
        };

        match &node.kind {
            AstNodeKind::Element(el) => {
                // Fast path: element with only static attributes (no directives,
                // no v-if/v-for/v-slot/v-once). Skip OXC parsing entirely —
                // no Box<OxcParsedElement> allocation needed.
                //
                // `v-memo="[deps]"` carries a dependency expression that must be
                // resolved (binding-prefixed) at codegen — its directive sets no
                // prop flag, so include it explicitly here.
                let has_memo = el.props.iter().any(|p| {
                    p.is_directive && &input[p.start as usize..p.name_end as usize] == "v-memo"
                });
                if !el.needs_expression_parsing() && !has_memo {
                    data.push(OxcNodeData::None);
                    continue;
                }

                let mut parsed =
                    parse_element(el, parent_locals, input, alloc, source_type, ide_completion);

                // Optimistically set AllInterpolationsStatic if element has
                // interpolation children (from pre-computed children_flag).
                if el.children_flag.has(ChildrenFlags::HasInterpolation) {
                    parsed.expression_flag = parsed
                        .expression_flag
                        .add(ExpressionFlags::AllInterpolationsStatic);
                }

                data.push(OxcNodeData::Element(Box::new(parsed)));
            }
            AstNodeKind::Interpolation(interp) => {
                let expr = parse_expression(
                    Span::new(interp.inner_start, interp.inner_end),
                    input,
                    alloc,
                    source_type,
                    parent_locals,
                    ide_completion,
                );

                // If non-static, remove AllInterpolationsStatic from parent.
                if expr.dynamism != Dynamism::Static {
                    if let Some(pid) = node.parent {
                        if let OxcNodeData::Element(parent_el) = &mut data[pid.0] {
                            parent_el.expression_flag = parent_el
                                .expression_flag
                                .remove(ExpressionFlags::AllInterpolationsStatic);
                        }
                    }
                }

                data.push(OxcNodeData::Interpolation(expr));
            }
            AstNodeKind::Text(_) | AstNodeKind::Comment(_) => {
                data.push(OxcNodeData::None);
            }
        }
    }

    OxcParsedAst::new(data)
}

#[cfg(test)]
thread_local! {
    /// Counts invocations of [`parse_template_expressions`] on the current
    /// thread. Lets tests assert that a combined TS-SFC compile parses its
    /// template expressions exactly once (shared overlay) instead of twice.
    static PARSE_TEMPLATE_EXPRESSIONS_CALLS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };

    /// Records the `(is_typescript, is_jsx)` discriminant of the [`SourceType`]
    /// each [`parse_template_expressions`] call parses with, in call order.
    /// Lets tests prove a JS SFC's TSX lane parses with `jsx()` (not `tsx()`).
    static PARSE_TEMPLATE_EXPRESSIONS_SOURCE_TYPES: std::cell::RefCell<Vec<(bool, bool)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Reset the per-thread `parse_template_expressions` invocation counter and the
/// recorded source-type log.
#[cfg(test)]
pub(crate) fn reset_parse_template_expressions_calls() {
    PARSE_TEMPLATE_EXPRESSIONS_CALLS.with(|c| c.set(0));
    PARSE_TEMPLATE_EXPRESSIONS_SOURCE_TYPES.with(|v| v.borrow_mut().clear());
}

/// Read the per-thread `parse_template_expressions` invocation counter.
#[cfg(test)]
pub(crate) fn parse_template_expressions_call_count() -> usize {
    PARSE_TEMPLATE_EXPRESSIONS_CALLS.with(|c| c.get())
}

/// Read the per-thread log of `(is_typescript, is_jsx)` source-type
/// discriminants, one entry per [`parse_template_expressions`] call, in call
/// order. `tsx()` records `(true, true)`; `jsx()` records `(false, true)`.
#[cfg(test)]
pub(crate) fn parse_template_expressions_source_types() -> Vec<(bool, bool)> {
    PARSE_TEMPLATE_EXPRESSIONS_SOURCE_TYPES.with(|v| v.borrow().clone())
}

// Re-export the slot-summary build/read counters so tests can assert the
// compute-once-consume-twice invariant without naming the private submodule path.
#[cfg(test)]
pub(crate) use slot_summary::{
    reset_slot_summary_counts, slot_summary_build_count, slot_summary_read_count,
};

#[cfg(test)]
mod tests;
