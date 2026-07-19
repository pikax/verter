//! TSX template generation: converts Vue template AST to valid JSX.
//!
//! Walks the [`TemplateAst`] directly (not using the shared `TemplateCodeGen` trait
//! or walker) and converts Vue template syntax to JSX using `CodeTransform` mutations.
//!
//! ## Conversion rules
//!
//! | Vue syntax | JSX output |
//! |---|---|
//! | `{{ expr }}` | `{expr}` |
//! | `<!-- comment -->` | `{/* comment */}` |
//! | `:prop="expr"` | `prop={expr}` |
//! | `@event="handler"` | `onEvent={handler}` |
//! | `v-if="cond"` | `{cond ? (...) : null}` |
//! | `v-for="item in items"` | `{items.map((item) => (...))}` |
//! | `v-show="expr"` | `style={{display: expr ? undefined : 'none'}}` |
//! | `v-model="val"` | `modelValue={val} onUpdate:modelValue={...}` |
//! | `v-bind="obj"` | `{...obj}` |
//! | `v-on="obj"` | `{...obj}` |

pub mod directives;
pub mod emit;
pub mod props;
pub mod vmodel;
pub mod von;

use oxc_allocator::Allocator;
use rustc_hash::{FxHashMap, FxHashSet};

use verter_span::{SourceByteOffset, SourceByteRange};

use crate::ast::types::{
    AstNodeKind, CommentNode, ConditionalChain, ElementNode, ElementNodeConditionKind,
    InterpolationNode, TagType, TextNode,
};
use crate::ide::condition::{self, ConditionScope};
use crate::ide::template::emit::{emit_op, emit_synthesized_shorthand_value, EmitOp, EmitText};
use crate::template::code_gen::binding::{BindingResolver, BindingType};
use crate::template::code_gen::expression::{
    build_prefixed_expr_segments, resolve_simple_expr_segments,
};
use crate::template::code_gen::types::{CodeGenOutput, MappedGeneratedText};
use crate::template::oxc::types::{
    ComponentSlotSummary, OxcNodeData, OxcParsedAst, OxcParsedElement, SlotChildFact, SlotChildKind,
};
use crate::types::NodeId;

use super::{IdeTemplateOptions, TemplateComponentBindings};

/// How this element is being emitted relative to a v-if chain.
///
/// This parameter flows down from the chain walk loop to `walk_element`
/// but does NOT propagate to nested children (those always get `Normal`).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ChainMode {
    /// Normal emission (JSX children context). Default for all non-chain elements.
    Normal,
    /// Lifted chain branch that has v-for: condition is outside,
    /// v-for emits bare (no `{`/`}`), element is in expression context.
    LiftedBranch,
    /// Lifted chain branch without v-for: condition is outside,
    /// element emits plain JSX in expression context.
    LiftedPlain,
}

/// Expression context for brace ownership.
///
/// Replaces the boolean `has_v_for` checks on brace emission.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum EmitContext {
    /// Inside JSX children: expressions need `{...}` wrapping.
    JsxChildren,
    /// Inside a JS expression (`.map()` callback body or ternary branch):
    /// expressions are already in JS context — no `{...}` wrapping needed.
    Expression,
}

/// A collected strict slot entry for `strictRenderSlot` emission.
struct StrictSlotEntry {
    /// SFC-absolute offset of parent component's tag_open.start (for Comp function reference).
    parent_comp_offset: u32,
    /// Slot name ("default", "header", etc.).
    slot_name: String,
    /// Child type references with their source positions.
    children: Vec<StrictSlotChild>,
}

/// A single child type reference for strict slot checking.
///
/// `source_pos` fields store SFC-absolute byte offsets used for sourcemap
/// mapping (child constructor names → original template positions).
enum StrictSlotChild {
    /// Component: constructor name + tag name SFC-absolute position.
    Component { name: String, source_pos: u32 },
    /// HTML element: tag name + SFC-absolute position.
    HtmlElement { tag: String, source_pos: u32 },
    /// Text node: mapped to text start position.
    Text { source_pos: u32 },
    /// Interpolation: mapped to interpolation start position.
    Interpolation { source_pos: u32 },
}

/// A collected entry for `checkRequiredSlots` emission.
struct RequiredSlotsCheck {
    /// SFC-absolute offset of the component's tag_open.start (for Comp function reference).
    comp_offset: u32,
    /// Slot names provided by the parent.
    provided_slot_names: Vec<String>,
    /// SFC-absolute position of the component tag (for sourcemapping).
    source_pos: u32,
}

/// Shared context for TSX template walker functions.
///
/// Groups the 7 parameters that are threaded identically through
/// `walk_node`, `walk_element`, and `walk_children_with_iife_tracking`.
struct IdeTemplateCtx<'a, 'alloc> {
    ast: &'a crate::ast::types::TemplateAst,
    oxc_ast: &'a OxcParsedAst<'alloc>,
    source: &'alloc str,
    out: &'a mut CodeGenOutput<'alloc>,
    alloc: &'alloc Allocator,
    resolver: &'a BindingResolver<'alloc>,
    options: &'a IdeTemplateOptions<'a>,
    /// Inventory of GlobalComponents fallback consts in scope in the templateBindingFN,
    /// so a global component's `@event` spread payload resolves through its emitted const.
    components: &'a TemplateComponentBindings,
    /// TS directive comments to inject inside `<component :is>` IIFE (before `return`).
    ts_directives_for_component_is: Vec<String>,
    /// Collected strict slot entries for `strictRenderSlot` emission.
    strict_slot_entries: Vec<StrictSlotEntry>,
    /// Collected required slots checks for `checkRequiredSlots` emission.
    required_slot_checks: Vec<RequiredSlotsCheck>,
}

/// Generate TSX template (JSX) from the template AST.
///
/// Walks the AST and produces JSX output by overwriting Vue-specific syntax
/// with JSX equivalents. Uses `CodeGenOutput` for deferred batch operations.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn generate_ide_template<'alloc>(
    ast: &crate::ast::types::TemplateAst,
    oxc_ast: &OxcParsedAst<'alloc>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    alloc: &'alloc Allocator,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    options: &IdeTemplateOptions<'_>,
    components: &TemplateComponentBindings,
) {
    let mut resolver = BindingResolver::new(bindings.clone(), true);
    resolver.set_tsx(true);

    let root = &ast.root;
    let content = match &root.content {
        Some(c) => c,
        None => return, // No template content
    };

    // Overwrite <template> tags
    // Replace <template> open tag with empty (we just want the content)
    out.overwrite(root.tag_open.start, root.tag_open.end, "");

    // Replace </template> close tag with empty
    if let Some(tag_close) = &root.tag_close {
        out.overwrite(tag_close.start, tag_close.end, "");
    }

    // Walk root children
    let children = &content.children;

    // Empty template: emit an empty fragment so TypeScript sees JSX usage
    if children.is_empty() {
        out.prepend_alloc(content.start, "<></>");
        return;
    }

    // If multiple root children, wrap in fragment
    let needs_fragment = children.len() > 1;
    if needs_fragment {
        out.prepend_alloc(content.start, "<>");
    }

    let mut ctx = IdeTemplateCtx {
        ast,
        oxc_ast,
        source,
        out,
        alloc,
        resolver: &resolver,
        options,
        components,
        ts_directives_for_component_is: Vec::new(),
        strict_slot_entries: Vec::new(),
        required_slot_checks: Vec::new(),
    };
    walk_children_with_iife_tracking(children, &content.v_if_chains, &mut ctx, &[]);

    if needs_fragment {
        ctx.out.prepend_alloc(content.end, "</>");
    }

    // Emit strict slot checks after template content
    if !ctx.strict_slot_entries.is_empty() || !ctx.required_slot_checks.is_empty() {
        emit_strict_slot_checks(&mut ctx, content.end);
    }
}

/// Walk a single AST node and generate JSX output.
fn walk_node<'a, 'alloc>(
    id: NodeId,
    ctx: &mut IdeTemplateCtx<'a, 'alloc>,
    condition_scopes: &[ConditionScope],
    chain_mode: ChainMode,
) {
    let node = &ctx.ast.nodes[id.0];
    let oxc_data = &ctx.oxc_ast.data[id.0];

    match &node.kind {
        AstNodeKind::Element(el) => {
            let oxc_el = match oxc_data {
                OxcNodeData::Element(el) => Some(el.as_ref()),
                _ => None,
            };
            walk_element(id, el, oxc_el, ctx, condition_scopes, chain_mode);
        }
        AstNodeKind::Text(text) => {
            visit_text(text, ctx.source, ctx.out);
        }
        AstNodeKind::Interpolation(interp) => {
            let oxc_expr = match oxc_data {
                OxcNodeData::Interpolation(expr) => Some(expr),
                _ => None,
            };
            visit_interpolation(interp, oxc_expr, ctx.source, ctx.out, ctx.resolver);
        }
        AstNodeKind::Comment(comment) => {
            visit_comment(comment, ctx.out, ctx.options);
        }
    }
}

/// Walk an element node: handle directives, props, children.
fn walk_element<'a, 'alloc>(
    id: NodeId,
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    ctx: &mut IdeTemplateCtx<'a, 'alloc>,
    parent_condition_scopes: &[ConditionScope],
    chain_mode: ChainMode,
) {
    // Handle structural directives first
    let has_v_if = el.v_condition.is_some();
    let has_v_for = el.v_for.is_some();
    // <template v-if v-slot> — v-if is handled by slot codegen, skip IIFE wrapping
    let is_slot_template = el.tag_type == TagType::Template && has_v_if && el.v_slot.is_some();

    // Lifted chain members skip v-if emission (the condition is emitted by the parent walk loop).
    let is_lifted = matches!(chain_mode, ChainMode::LiftedBranch | ChainMode::LiftedPlain);

    let emit_iife = has_v_if && !is_slot_template && !is_lifted;

    // Compute emit context for brace ownership
    let emit_ctx = match chain_mode {
        ChainMode::Normal if has_v_for => EmitContext::Expression,
        ChainMode::Normal => EmitContext::JsxChildren,
        ChainMode::LiftedBranch | ChainMode::LiftedPlain => EmitContext::Expression,
    };

    // v-for wrapping
    if has_v_for {
        directives::emit_v_for_open(
            el,
            oxc_el,
            ctx.source,
            ctx.out,
            ctx.alloc,
            ctx.resolver,
            ctx.options.is_jsx,
            chain_mode == ChainMode::LiftedBranch,
        );
    }

    // Build condition scope for this element (for type narrowing guards).
    // This computes the current element's scope and the full accumulated scopes.
    let own_scope = if has_v_if {
        build_condition_scope(el, oxc_el, ctx.source, ctx.resolver, ctx.ast, id)
    } else {
        None
    };
    let full_scopes: Vec<ConditionScope> = if let Some(ref scope) = own_scope {
        let mut s = parent_condition_scopes.to_vec();
        s.push(scope.clone());
        s
    } else {
        parent_condition_scopes.to_vec()
    };

    // Generate guard text for prop narrowing (full accumulated scopes)
    let guard_text = condition::generate_condition_text(&full_scopes);

    // v-if/v-else-if/v-else IIFE wrapping (skip for <template v-if v-slot>)
    if emit_iife {
        directives::emit_v_if_open(
            el,
            oxc_el,
            ctx.source,
            ctx.out,
            ctx.alloc,
            ctx.resolver,
            parent_condition_scopes,
        );
    }

    // Remove cached structural directive attributes from source.
    // These are NOT in el.props (the parser extracts them via prop.take()),
    // but their byte ranges are still in the original source. Without explicit
    // removal they leak into the JSX output as invalid attributes.
    // We also consume leading whitespace so `<div v-once>` → `<div>`, not `<div >`.
    if let Some(ref condition) = el.v_condition {
        let start = eat_leading_whitespace(ctx.source, condition.prop.start);
        let prop_end = props::get_prop_end(&condition.prop);
        ctx.out.overwrite(start, prop_end, "");
    }
    if let Some(ref v_for) = el.v_for {
        let start = eat_leading_whitespace(ctx.source, v_for.start);
        let prop_end = props::get_prop_end(v_for);
        ctx.out.overwrite(start, prop_end, "");
    }
    // Skip v_slot removal for <template> — handled in the TagType::Template branch
    // which preserves the slot name at its original position for sourcemaps.
    if el.tag_type != TagType::Template {
        if let Some(ref v_slot) = el.v_slot {
            let start = eat_leading_whitespace(ctx.source, v_slot.start);
            let prop_end = props::get_prop_end(v_slot);
            ctx.out.overwrite(start, prop_end, "");
        }
    }
    if let Some(ref v_once) = el.v_once {
        let start = eat_leading_whitespace(ctx.source, v_once.start);
        let prop_end = props::get_prop_end(v_once);
        ctx.out.overwrite(start, prop_end, "");
    }
    // Convert a cached STATIC `ref` attribute to JSX expression syntax:
    //   `ref="myRef"` → `ref={"myRef"}`
    // Only a NON-directive `ref="…"` is cached on `el.v_ref` (the parser routes a
    // dynamic `:ref="expr"` / `v-bind:ref="expr"` through `el.props` →
    // `process_v_bind`, which already emits `ref={expr}` IN PLACE with the value
    // navigable). The static ref value is a string literal, NOT a navigable binding,
    // so it is emitted as an UNMAPPED synthetic replacement (delete the original span
    // + unmapped insert) — never a mapped `out.overwrite(v_ref.start, prop_end,
    // &format!("ref={{\"{}\"}}", value))` (which would map the synthetic `ref={"…"}`
    // back to the prop start).
    if let Some(ref v_ref) = el.v_ref {
        if let (Some(vs), Some(ve)) = (v_ref.value_start, v_ref.value_end) {
            let prop_end = props::get_prop_end(v_ref);
            let value = &ctx.source[vs as usize..ve as usize];
            ctx.out.overwrite(v_ref.start, prop_end, "");
            ctx.out
                .prepend_alloc(v_ref.start, &format!("ref={{\"{}\"}}", value));
        }
    }

    // Handle the element tag itself
    let tag_name = &ctx.source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];

    // Track the actual JSX tag name. Dynamic/static `<component :is>` rewrites
    // both authored tag names, so the empty typed pair emitted for Vue slot
    // isolation must close the rewritten name rather than literal `component`.
    let mut emitted_tag_name = tag_name;

    // Track whether dynamic <component :is> needs IIFE closing after element
    let mut needs_component_is_iife_close = false;

    // Track whether a kebab component tag was rewritten to its PascalCase
    // binding (open AND close), so the later close-tag case-mismatch fix does
    // not double-overwrite the already-rewritten close name.
    let mut kebab_tag_rewritten = false;

    // Convert tag for components
    match el.tag_type {
        TagType::Component => {
            // Handle `<component is="...">` / `<component :is="...">`.
            // Dynamic `:is` wraps the element in an IIFE — needs closing after element end.
            if tag_name == "component" {
                if let Some(rewrite) = rewrite_component_is(
                    el,
                    oxc_el,
                    ctx.source,
                    ctx.out,
                    ctx.resolver,
                    ctx.components,
                    &ctx.ts_directives_for_component_is,
                    emit_ctx,
                ) {
                    emitted_tag_name = rewrite.tag_name;
                    needs_component_is_iife_close = rewrite.needs_iife_close;
                }
            } else if tag_name.contains('-') {
                // Rewrite a resolvable kebab component tag to its in-scope
                // PascalCase binding (a local script binding or a
                // GlobalComponents fallback const). A lowercase JSX identifier
                // is an INTRINSIC-element lookup that never consults the
                // emitted const — the rewrite makes the tag reference it, with
                // the generated name mapped onto the authored kebab spans (a
                // mapped CodeTransform overwrite) so hover/definition/rename
                // keep working from the source tag. An unresolvable kebab tag
                // stays as-authored (fail-closed intrinsic diagnostic).
                if let Some(binding) = ctx
                    .components
                    .resolve(tag_name, el.tag_type, |n| ctx.resolver.get(n).is_some())
                {
                    if binding != tag_name {
                        let pascal = ctx.out.alloc_str(&binding);
                        // Mapped overwrite of the open tag-name span.
                        ctx.out
                            .overwrite(el.tag_open.start + 1, el.tag_open.name_end, pascal);
                        // Rewrite the authored close name only when the body
                        // will NOT be isolated: `isolate_vue_slot_body` deletes
                        // the whole authored close span and emits the mapped
                        // `</Pascal>` pair-close from `emitted_tag_name`, so a
                        // second overwrite inside that span would conflict.
                        let will_isolate = el
                            .content
                            .as_ref()
                            .is_some_and(|content| !content.children.is_empty())
                            && el.tag_close.is_some();
                        if !will_isolate {
                            if let Some(tag_close) = &el.tag_close {
                                ctx.out
                                    .overwrite(tag_close.start + 2, tag_close.name_end, pascal);
                            }
                        }
                        emitted_tag_name = pascal;
                        kebab_tag_rewritten = true;
                    }
                }
            }
        }
        TagType::Template => {
            // Named slot: preserve slot name at original position for intellisense.
            // <template #header> → <>{"header"}
            // <template v-slot:header> → <>{"header"}
            let is_self_closing = el.tag_close.is_none();
            // Self-closing templates need <></> since there's no closing tag to rewrite
            let frag_suffix = if is_self_closing { "</>" } else { "" };
            if let Some(ref v_slot) = el.v_slot {
                if let (Some(arg_start), Some(arg_end)) = (v_slot.arg_start, v_slot.arg_end) {
                    // Overwrite everything before slot name → <>{"
                    ctx.out.overwrite(el.tag_open.start, arg_start, "<>{\"");
                    // Slot name stays at [arg_start, arg_end) — sourcemap preserves position
                    // Overwrite everything after slot name through close of open tag → "}
                    ctx.out
                        .overwrite(arg_end, el.tag_open.end, &format!("\"}}{frag_suffix}"));
                } else {
                    // Default slot (no name): <template v-slot> → <>
                    ctx.out.overwrite(
                        el.tag_open.start,
                        el.tag_open.end,
                        &format!("<>{frag_suffix}"),
                    );
                }
            } else {
                // Plain <template> wrapper → <>
                ctx.out.overwrite(
                    el.tag_open.start,
                    el.tag_open.end,
                    &format!("<>{frag_suffix}"),
                );
            }
        }
        TagType::SlotOutlet => {
            // <slot name="x" :prop="val">fallback</slot>
            // → {___VERTER___instance.$slots.x?.({ prop: val }) ?? <>fallback</>}
            //
            // Uses fine-grained overwrites (not overwrite-all + prepends) so that
            // vue_to_tsx interpolation stays bounded. Each overwrite creates a source
            // map boundary, preventing positions within the tag from interpolating
            // past `$slots.name` into the `?.()` call site (which caused `() any` hover).
            let has_children = el.content.as_ref().is_some_and(|c| !c.children.is_empty());

            // Extract slot name + source positions from props
            let slot_info = extract_slot_name(el, ctx.source);

            // Collect slot props (non-name, non-structural attributes)
            let slot_props = collect_slot_props(el, oxc_el, ctx.source, ctx.resolver);

            // Build the call suffix: `?.()` or `?.({ props })`, with `}` or `?? <>`
            // In expression context, omit the closing `}` since we don't emit the opening `{`.
            let jsx_close = if emit_ctx == EmitContext::Expression {
                ""
            } else {
                "}"
            };
            let call_suffix = if slot_props.is_empty() {
                if has_children {
                    "?.() ?? <>".to_string()
                } else {
                    format!("?.(){jsx_close}")
                }
            } else if has_children {
                format!("?.({{ {} }}) ?? <>", slot_props)
            } else {
                format!("?.({{ {} }}){jsx_close}", slot_props)
            };

            // Fine-grained overwrites for source map accuracy:
            // 1. `<` → `{___VERTER___instance.` (or no `{` inside v-for to avoid
            //    `=> ({...})` being parsed as parenthesized object literal)
            let slot_prefix = if emit_ctx == EmitContext::Expression {
                "___VERTER___instance."
            } else {
                "{___VERTER___instance."
            };
            ctx.out
                .overwrite(el.tag_open.start, el.tag_open.start + 1, slot_prefix);
            // 2. `slot` → `$slots`
            ctx.out
                .overwrite(el.tag_open.start + 1, el.tag_open.name_end, "$slots");

            if let Some(ref info) = slot_info {
                // Static name: overwrite gap between tag name and value to `.`,
                // keep the name value in place, overwrite rest to call suffix.
                if is_valid_js_ident(info.name) {
                    // Dot notation: ` name="` → `.`
                    ctx.out
                        .overwrite(el.tag_open.name_end, info.value_start, ".");
                    // Keep slot name value (source mapped)
                    // `" />` or `" >` → call suffix
                    ctx.out
                        .overwrite(info.value_end, el.tag_open.end, &call_suffix);
                } else {
                    // Bracket notation for non-ident names (e.g., `overlay-content`):
                    // ` name="` → `['`
                    ctx.out
                        .overwrite(el.tag_open.name_end, info.value_start, "['");
                    // Keep slot name value (source mapped)
                    // `" />` → `']` + call suffix
                    ctx.out.overwrite(
                        info.value_end,
                        el.tag_open.end,
                        &format!("']{}", call_suffix),
                    );
                }
            } else {
                // No static name (default slot or dynamic :name):
                // overwrite everything after tag name to `.default` + call suffix
                ctx.out.overwrite(
                    el.tag_open.name_end,
                    el.tag_open.end,
                    &format!(".default{}", call_suffix),
                );
            }

            // Close tag
            if let Some(tag_close) = &el.tag_close {
                if has_children {
                    let close_suffix = if emit_ctx == EmitContext::Expression {
                        "</>"
                    } else {
                        "</>}"
                    };
                    ctx.out
                        .overwrite(tag_close.start, tag_close.end, close_suffix);
                } else {
                    ctx.out.overwrite(tag_close.start, tag_close.end, "");
                }
            }

            // Skip normal prop processing and child walking for slot outlets —
            // we've already handled everything above. Process children below
            // only if there's fallback content.
            if has_children {
                if let Some(content) = &el.content {
                    walk_children_with_iife_tracking(
                        &content.children,
                        &content.v_if_chains,
                        ctx,
                        &full_scopes,
                    );
                }
            }

            // Close v-if/v-for if present
            if emit_iife {
                directives::emit_v_if_close(el, ctx.source, ctx.out);
            }
            if has_v_for {
                directives::emit_v_for_close(
                    el,
                    ctx.source,
                    ctx.out,
                    chain_mode == ChainMode::LiftedBranch,
                );
            }
            return; // Early return — skip normal element processing below
        }
        _ => {
            // Native HTML elements — pass through
        }
    }

    // Process props/attributes → JSX (pass guard for type narrowing in arrow functions)
    let collected_directives = props::process_element_props(
        el,
        oxc_el,
        ctx.source,
        ctx.out,
        ctx.alloc,
        ctx.resolver,
        ctx.components,
        guard_text.as_deref(),
        ctx.options.is_jsx,
    );

    // Emit v-directive callback prop for collected custom directives (TS mode only)
    if !collected_directives.is_empty() {
        use std::fmt::Write;
        let mut directive_prop = String::from(
            " v-directive={(___VERTER___slotInstance)=>{const ___VERTER___directiveElement={} as ___VERTER___ExtractLeafElement<typeof ___VERTER___slotInstance>;",
        );
        for d in &collected_directives {
            write!(
                directive_prop,
                "___VERTER___runCustomDirective(___VERTER___directiveElement,___VERTER___directiveAccessor[\"{name}\"])(___VERTER___directiveElement,{value},{arg},{mods});",
                name = d.camel_name,
                value = d.value,
                arg = d.arg,
                mods = d.modifiers,
            )
            .expect("write to String is infallible");
        }
        directive_prop.push_str("}}");

        // Insert just before the tag close: before `/>` for self-closing, before `>` otherwise
        let is_self_closing =
            ctx.source.as_bytes().get(el.tag_open.end as usize - 2) == Some(&b'/');
        let insert_pos = if is_self_closing {
            el.tag_open.end - 2
        } else {
            el.tag_open.end - 1
        };
        ctx.out.prepend_alloc(insert_pos, &directive_prop);
    }

    // Process v-show
    directives::emit_v_show(el, oxc_el, ctx.source, ctx.out, ctx.alloc, ctx.resolver);

    // Void HTML elements (<br>, <input>, <img>, <hr>, etc.) need self-closing in JSX.
    // The parser sets is_self_closing for void tags, but the source may lack the `/`.
    // Check if the source `>` at tag_open.end-1 is preceded by `/` — if not, add it.
    if el.tag_close.is_none() && el.content.is_none() {
        let end_byte = el.tag_open.end as usize;
        if end_byte >= 2
            && ctx.source.as_bytes().get(end_byte - 1) == Some(&b'>')
            && ctx.source.as_bytes().get(end_byte - 2) != Some(&b'/')
        {
            ctx.out
                .overwrite(el.tag_open.end - 1, el.tag_open.end, " />");
        }
    }

    // Vue element bodies are slot/template content, not a React-style
    // `children` prop. Keep an empty, fully typed JSX pair for tag/prop checking
    // and navigation, then place the authored body beside it in a fragment. This
    // prevents an ambient React `ElementChildrenAttribute` from injecting a
    // synthetic `children` attribute into Vue's JSX contract under
    // `jsx: preserve` while retaining every authored child expression.
    let has_isolated_slot_body = isolate_vue_slot_body(el, emitted_tag_name, ctx.out);

    // ── v-slot scoped parameter IIFE wrapping ────────────────────────
    // When v-slot has parameters (e.g., `v-slot="{ slotItem }"`), wrap children
    // in an IIFE that types the slot params via extractArgumentsFromRenderSlot.
    //
    // Component: <MyComp v-slot="{ slotItem }">children</MyComp>
    //   → <MyComp>{(({ slotItem }) => (<>children</>))(CALL)}</MyComp>
    //
    // Template: <template #header="{ title }">children</template>
    //   → <>{"header"}{(({ title }) => (<>children</>))(CALL)}</>
    let slot_iife_info = build_slot_iife_info(id, el, ctx.source, ctx.ast);
    if let Some(ref slot_info) = slot_iife_info {
        // Emit slot IIFE opening in three ordered parts through the mapped
        // prepend channel (insertion order is preserved within one anchor):
        //   unmapped `{(() => { const ` + MAPPED authored pattern + unmapped
        //   ` = extractArgumentsFromRenderSlot(...); return (<>`.
        // The pattern bytes are verbatim-authored, so they carry a source-map
        // token — pattern-position hover maps into the generated destructure
        // and the provider answers with the typed binding quickinfo (D4).
        let anchor = if has_isolated_slot_body {
            // The generated mapped `</Tag>` sits at the same anchor. Keep all
            // emissions in the ordered prepend channel so the empty pair
            // closes before the slot IIFE begins.
            el.tag_open.end - 1
        } else {
            el.tag_open.end
        };
        ctx.out
            .prepend_ordered_unmapped(anchor, &slot_info.open_prefix);
        ctx.out.prepend_alloc_mapped(
            anchor,
            slot_info.params_start,
            &ctx.source[slot_info.params_start as usize..slot_info.params_end as usize],
        );
        ctx.out
            .prepend_ordered_unmapped(anchor, &slot_info.open_suffix);
    }

    // Walk children — children inherit the condition scopes from this element
    if let Some(content) = &el.content {
        walk_children_with_iife_tracking(
            &content.children,
            &content.v_if_chains,
            ctx,
            &full_scopes,
        );
    }

    // ── Strict slot children collection ────────────────────────────
    // Both checks read the SAME per-component slot summary from the shared
    // overlay (built once on first demand for this component, then memoized)
    // instead of each re-scanning the component's children. `None` covers
    // dynamic `<component :is>` (no checks).
    if ctx.options.strict_slots && !ctx.options.is_jsx && el.tag_type == TagType::Component {
        let oxc_ast = ctx.oxc_ast;
        let ast = ctx.ast;
        let source = ctx.source;
        if let Some(summary) = oxc_ast.slot_summary(id, ast, source) {
            collect_strict_slot_children(summary, el, ctx);
            collect_required_slots_check(summary, el, ctx);
        }
    }

    // Close slot IIFE: </>)(extractArgumentsFromRenderSlot(...))}
    if let Some(ref slot_info) = slot_iife_info {
        let close_pos = el
            .tag_close
            .as_ref()
            .map(|tc| tc.start)
            .unwrap_or(el.tag_open.end);
        ctx.out.prepend_alloc(close_pos, &slot_info.close_text);
    }

    if has_isolated_slot_body {
        let tag_close = el
            .tag_close
            .as_ref()
            .expect("isolated Vue slot bodies always have an authored closing tag");
        emit_op(
            ctx.out,
            &EmitOp::InsertUnmapped {
                at: SourceByteOffset(tag_close.start),
                text: EmitText::Static("</>"),
            },
        );
    }

    // Fix closing tag case mismatch: Vue is case-insensitive for closing tags
    // (e.g., <Button>...</button>) but JSX requires exact case match. Rewrite the
    // closing tag name to match the opening tag when they differ. Skipped when a
    // kebab tag was rewritten to its PascalCase binding — that path already
    // overwrote (or isolated) the close name.
    if !kebab_tag_rewritten {
        if let Some(tag_close) = &el.tag_close {
            let open_name =
                &ctx.source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
            let close_name = &ctx.source[tag_close.start as usize + 2..tag_close.name_end as usize];
            if open_name != close_name && open_name.eq_ignore_ascii_case(close_name) {
                ctx.out
                    .overwrite(tag_close.start + 2, tag_close.name_end, open_name);
            }
        }
    }

    // Handle close tag for <template> → </>
    if el.tag_type == TagType::Template {
        if let Some(tag_close) = &el.tag_close {
            ctx.out.overwrite(tag_close.start, tag_close.end, "</>");
        }
    }

    // Close dynamic <component :is> IIFE wrapper — must be before v-if/v-for close
    // so the IIFE is innermost: `{(() => { ...; return <comp/>; })()}`
    if needs_component_is_iife_close {
        let el_end = el
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(el.tag_open.end);
        let iife_close = if emit_ctx == EmitContext::Expression {
            "; })()"
        } else {
            "; })()}"
        };
        ctx.out.prepend_alloc(el_end, iife_close);
    }

    // Close v-if IIFE (skip for <template v-if v-slot>)
    if emit_iife {
        directives::emit_v_if_close(el, ctx.source, ctx.out);
    }

    // Close v-for
    if has_v_for {
        directives::emit_v_for_close(
            el,
            ctx.source,
            ctx.out,
            chain_mode == ChainMode::LiftedBranch,
        );
    }
}

/// Isolate an authored Vue element body from JSX's framework-specific
/// `ElementChildrenAttribute` contract.
///
/// A paired element with body content becomes one JSX fragment whose first
/// child is an empty typed pair and whose remaining children are the original
/// Vue body:
///
/// ```text
/// <Panel prop="x">body</Panel>
/// <><Panel prop="x"></Panel>body</>
/// ```
///
/// The opening name remains original source. The generated empty-pair closing
/// name is mapped to the authored closing-name span so hover/definition/rename
/// continue to work from either tag. All fragment punctuation is unmapped.
fn isolate_vue_slot_body(
    el: &ElementNode,
    emitted_tag_name: &str,
    out: &mut CodeGenOutput<'_>,
) -> bool {
    if matches!(el.tag_type, TagType::Template | TagType::SlotOutlet)
        || el
            .content
            .as_ref()
            .is_none_or(|content| content.children.is_empty())
    {
        return false;
    }

    let Some(tag_close) = &el.tag_close else {
        // An incomplete/unclosed element already carries a parser diagnostic.
        // Do not invent a closing-source anchor that does not exist.
        return false;
    };

    // Delete the authored `<` and put `<><` immediately before the preserved
    // opening name. Anchoring at `start + 1` keeps existing v-if/v-for/dynamic
    // component wrappers (inserted at `start`) outside the new fragment.
    emit_op(
        out,
        &EmitOp::OverwriteSyntheticBoundary {
            source: SourceByteRange::new(
                SourceByteOffset(el.tag_open.start),
                SourceByteOffset(el.tag_open.start + 1),
            ),
            text: EmitText::Static(""),
            anchor: None,
        },
    );
    emit_op(
        out,
        &EmitOp::InsertUnmapped {
            at: SourceByteOffset(el.tag_open.start + 1),
            text: EmitText::Static("<><"),
        },
    );

    // Replace the authored opening `>` with `></Tag>`, closing the typed element
    // immediately. Anchoring before the original `>` avoids the boundary shared
    // with a following text/interpolation overwrite. Only the generated tag name
    // is mapped; JSX punctuation is synthetic. Use the actual emitted name for
    // `<component :is>` rewrites while mapping it to the authored close.
    let close_name_start = tag_close.start + 2;
    emit_op(
        out,
        &EmitOp::OverwriteSyntheticBoundary {
            source: SourceByteRange::new(
                SourceByteOffset(el.tag_open.end - 1),
                SourceByteOffset(el.tag_open.end),
            ),
            text: EmitText::Static(""),
            anchor: None,
        },
    );
    let mut empty_pair_close = MappedGeneratedText::synthetic("></");
    empty_pair_close.push(emitted_tag_name, Some(close_name_start));
    empty_pair_close.push(">", None);
    out.prepend_mapped_generated_text(el.tag_open.end - 1, &empty_pair_close);

    // Delete the original close. `walk_element` emits the synthetic fragment
    // close at `tag_close.start` only after children and any slot-IIFE close have
    // been recorded, preserving their nesting order.
    emit_op(
        out,
        &EmitOp::OverwriteSyntheticBoundary {
            source: SourceByteRange::new(
                SourceByteOffset(tag_close.start),
                SourceByteOffset(tag_close.end),
            ),
            text: EmitText::Static(""),
            anchor: None,
        },
    );
    true
}

/// Info for generating a v-slot scoped parameter IIFE wrapper.
struct SlotIifeInfo {
    /// Unmapped open prefix prepended after the open tag: `{(() => { const `
    open_prefix: String,
    /// Authored byte range of the destructure pattern — emitted between
    /// `open_prefix` and `open_suffix` as a SOURCE-MAPPED prepend so IDE
    /// features (hover on the destructured bindings) resolve the provider's
    /// typed quickinfo at the authored pattern positions instead of landing
    /// in an unmapped synthetic region (D4).
    params_start: u32,
    params_end: u32,
    /// Unmapped open suffix prepended after the pattern:
    /// ` = ___VERTER___extractArgumentsFromRenderSlot(...); return (<>`
    open_suffix: String,
    /// Text to prepend before the close tag: `</>)(___VERTER___extractArgumentsFromRenderSlot(...))}`
    close_text: String,
}

/// Build slot IIFE info for an element with v-slot parameters.
///
/// Returns `Some(SlotIifeInfo)` when the element's v-slot has parameters (value_start/end),
/// indicating that children should be wrapped in a typed IIFE.
///
/// For components: `<MyComp v-slot="{ item }">` → uses MyComp as the tag
/// For templates: `<template #header="{ title }">` → looks up parent component tag
fn build_slot_iife_info(
    id: NodeId,
    el: &ElementNode,
    source: &str,
    ast: &crate::ast::types::TemplateAst,
) -> Option<SlotIifeInfo> {
    let v_slot = el.v_slot.as_ref()?;
    let (vs, ve) = match (v_slot.value_start, v_slot.value_end) {
        (Some(s), Some(e)) if s < e => (s, e),
        _ => return None, // No params
    };

    // Determine the slot name
    let slot_name = if let (Some(arg_start), Some(arg_end)) = (v_slot.arg_start, v_slot.arg_end) {
        &source[arg_start as usize..arg_end as usize]
    } else {
        "default"
    };

    // Determine the component tag name for instantiateComponent
    let comp_tag = if el.tag_type == TagType::Template {
        // For <template v-slot>, look up the parent component
        find_parent_component_tag(id, source, ast)?
    } else {
        // For component v-slot (e.g., <MyComp v-slot="...">)
        source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize].to_string()
    };

    let open_prefix = "{(() => { const ".to_string();
    let open_suffix = format!(
        " = ___VERTER___extractArgumentsFromRenderSlot(___VERTER___instantiateComponent({comp_tag}, {{}}), \"{slot_name}\"); return (<>"
    );
    let close_text = "</>); })()}".to_string();

    Some(SlotIifeInfo {
        open_prefix,
        params_start: vs,
        params_end: ve,
        open_suffix,
        close_text,
    })
}

/// Find the parent component tag name for a <template v-slot> element.
fn find_parent_component_tag(
    id: NodeId,
    source: &str,
    ast: &crate::ast::types::TemplateAst,
) -> Option<String> {
    let node = &ast.nodes[id.0];
    let parent_id = node.parent?;
    let parent_node = &ast.nodes[parent_id.0];
    if let AstNodeKind::Element(ref parent_el) = parent_node.kind {
        if parent_el.tag_type == TagType::Component {
            let tag = &source
                [(parent_el.tag_open.start + 1) as usize..parent_el.tag_open.name_end as usize];
            return Some(tag.to_string());
        }
    }
    None
}

/// Walk a list of child nodes, using chain metadata for v-if/v-else chains.
///
/// Chains are pre-computed by the AST builder. Each chain is classified as:
/// - **Lifted** (any member has v-for): emits `{cond ? branch : branch : null}`
/// - **IIFE** (no member has v-for): emits `{()=>{if(cond){ ... }else{ ... }}}` (existing)
///
/// Non-chain children (including solo v-if without siblings) pass through normally.
fn walk_children_with_iife_tracking<'a, 'alloc>(
    children: &[NodeId],
    chains: &[ConditionalChain],
    ctx: &mut IdeTemplateCtx<'a, 'alloc>,
    parent_condition_scopes: &[ConditionScope],
) {
    // ── Build per-index plan from chain metadata ──

    // Classify each chain: Lifted (any member has v-for) vs Iife
    #[derive(Copy, Clone, PartialEq)]
    enum ChainShape {
        Lifted,
        Iife,
    }

    // Per-member plan entry
    struct MemberPlan {
        shape: ChainShape,
        is_first: bool,
        is_last: bool,
        has_else: bool, // true if this member is v-else (only relevant when is_last)
        member_has_v_for: bool,
    }

    let mut member_plan: FxHashMap<usize, MemberPlan> = FxHashMap::default();
    let mut suppressed_indices: FxHashSet<usize> = FxHashSet::default();

    for chain in chains {
        let indices = &chain.member_indices;
        if indices.is_empty() {
            continue;
        }

        // Classify: lifted if any member has v-for
        let any_v_for = indices.iter().any(|&idx| {
            matches!(
                &ctx.ast.nodes[children[idx].0].kind,
                AstNodeKind::Element(el) if el.v_for.is_some()
            )
        });
        let shape = if any_v_for {
            ChainShape::Lifted
        } else {
            ChainShape::Iife
        };

        let last_idx = indices.len() - 1;
        for (pos, &child_idx) in indices.iter().enumerate() {
            let el = match &ctx.ast.nodes[children[child_idx].0].kind {
                AstNodeKind::Element(el) => el,
                _ => continue,
            };
            let has_else = el
                .v_condition
                .as_ref()
                .is_some_and(|c| c.kind == ElementNodeConditionKind::Else);
            member_plan.insert(
                child_idx,
                MemberPlan {
                    shape,
                    is_first: pos == 0,
                    is_last: pos == last_idx,
                    has_else,
                    member_has_v_for: el.v_for.is_some(),
                },
            );
        }

        // Suppress text/comment nodes between chain members
        if indices.len() >= 2 {
            let first = indices[0];
            let last = indices[indices.len() - 1];
            for idx in (first + 1)..last {
                if !member_plan.contains_key(&idx) {
                    suppressed_indices.insert(idx);
                }
            }
        }
    }

    // ── Comment repositioning (unchanged logic) ──

    let analysis = if ctx.options.comments {
        analyze_child_comments(children, ctx.ast, ctx.source)
    } else {
        ChildCommentAnalysis {
            v_if_repositioned: FxHashSet::default(),
            v_for_repositioned: FxHashMap::default(),
            component_is_comments: FxHashMap::default(),
        }
    };

    let mut all_repositioned = analysis.v_if_repositioned.clone();
    for indices in analysis.v_for_repositioned.values() {
        for &idx in indices {
            all_repositioned.insert(idx);
        }
    }
    // Also add suppressed chain-interval indices
    all_repositioned.extend(&suppressed_indices);

    // ── IIFE tracking state (for Iife-shape chains) ──
    let mut pending_iife_close_pos: Option<u32> = None;

    // ── Walk ──

    for (idx, &child_id) in children.iter().enumerate() {
        let child_node = &ctx.ast.nodes[child_id.0];

        // Skip repositioned/suppressed comments and whitespace between chain members
        if all_repositioned.contains(&idx) {
            match &child_node.kind {
                AstNodeKind::Comment(c) => {
                    ctx.out.overwrite(c.start, c.end, "");
                }
                AstNodeKind::Text(t) if suppressed_indices.contains(&idx) => {
                    ctx.out.overwrite(t.start, t.end, "");
                }
                _ => {}
            }
            continue;
        }

        // ── Chain-driven emission ──

        if let Some(plan) = member_plan.get(&idx) {
            let AstNodeKind::Element(child_el) = &child_node.kind else {
                continue;
            };

            match plan.shape {
                ChainShape::Lifted => {
                    // Flush any pending IIFE from a previous Iife chain
                    if let Some(pos) = pending_iife_close_pos.take() {
                        ctx.out.prepend_alloc(pos, "}}");
                    }

                    let oxc_el = match &ctx.oxc_ast.data[child_id.0] {
                        OxcNodeData::Element(el) => Some(el.as_ref()),
                        _ => None,
                    };

                    // Emit ternary structure
                    if plan.is_first {
                        // `{` opens the JSX expression
                        ctx.out.prepend_alloc(child_el.tag_open.start, "{");
                    }

                    // Emit condition
                    if let Some(ref cond) = child_el.v_condition {
                        match cond.kind {
                            ElementNodeConditionKind::If => {
                                // Emit condition expression + ` ?\n`
                                if let (Some(vs), Some(ve)) =
                                    (cond.prop.value_start, cond.prop.value_end)
                                {
                                    directives::emit_mapped_condition_expr(
                                        ctx.out,
                                        child_el.tag_open.start,
                                        "",
                                        " ?\n",
                                        vs,
                                        ve,
                                        ctx.source,
                                        oxc_el,
                                        ctx.resolver,
                                    );
                                }
                            }
                            ElementNodeConditionKind::ElseIf => {
                                // ` : ` + condition + ` ?\n`
                                if let (Some(vs), Some(ve)) =
                                    (cond.prop.value_start, cond.prop.value_end)
                                {
                                    directives::emit_mapped_condition_expr(
                                        ctx.out,
                                        child_el.tag_open.start,
                                        " : ",
                                        " ?\n",
                                        vs,
                                        ve,
                                        ctx.source,
                                        oxc_el,
                                        ctx.resolver,
                                    );
                                }
                            }
                            ElementNodeConditionKind::Else => {
                                ctx.out.prepend_alloc(child_el.tag_open.start, " : ");
                            }
                        }
                    }

                    // Walk the element with appropriate chain mode
                    let mode = if plan.member_has_v_for {
                        ChainMode::LiftedBranch
                    } else {
                        ChainMode::LiftedPlain
                    };

                    // Set up component :is TS directives
                    if let Some(comments) = analysis.component_is_comments.get(&idx) {
                        ctx.ts_directives_for_component_is = comments.clone();
                    }
                    walk_node(child_id, ctx, parent_condition_scopes, mode);
                    ctx.ts_directives_for_component_is.clear();

                    // Inject repositioned comments
                    if let AstNodeKind::Element(child_el) = &ctx.ast.nodes[child_id.0].kind {
                        if let Some(comment_indices) = analysis.v_for_repositioned.get(&idx) {
                            inject_ts_directive_comments_for_v_for(
                                comment_indices,
                                children,
                                ctx.ast,
                                ctx.source,
                                ctx.out,
                                child_el,
                            );
                        }
                    }

                    // Emit trailing closure for lifted chain
                    if plan.is_last {
                        let el_end = child_el
                            .tag_close
                            .as_ref()
                            .map(|tc| tc.end)
                            .unwrap_or(child_el.tag_open.end);
                        if !plan.has_else {
                            // Last member is v-else-if or solo v-if: add ` : null}`
                            ctx.out.prepend_alloc(el_end, "\n : null}");
                        } else {
                            // Last member is v-else: just close `}`
                            ctx.out.prepend_alloc(el_end, "\n}");
                        }
                    }
                }
                ChainShape::Iife => {
                    // ── IIFE chain logic (same as before but chain-driven) ──

                    let is_slot_template = child_el.tag_type == TagType::Template
                        && child_el.v_condition.is_some()
                        && child_el.v_slot.is_some();

                    if !is_slot_template {
                        if let Some(ref cond) = child_el.v_condition {
                            match cond.kind {
                                ElementNodeConditionKind::If => {
                                    // Flush pending from previous chain
                                    if let Some(pos) = pending_iife_close_pos.take() {
                                        ctx.out.prepend_alloc(pos, "}}");
                                    }
                                }
                                ElementNodeConditionKind::ElseIf
                                | ElementNodeConditionKind::Else => {
                                    // Continue existing chain
                                }
                            }
                        }
                    }

                    // Set up component :is TS directives
                    if let Some(comments) = analysis.component_is_comments.get(&idx) {
                        ctx.ts_directives_for_component_is = comments.clone();
                    }
                    walk_node(child_id, ctx, parent_condition_scopes, ChainMode::Normal);
                    ctx.ts_directives_for_component_is.clear();

                    // Inject repositioned comments
                    if let AstNodeKind::Element(child_el) = &ctx.ast.nodes[child_id.0].kind {
                        let has_dynamic_is = is_dynamic_component_is(child_el, ctx.source);
                        if matches!(
                            child_el.v_condition.as_ref().map(|c| &c.kind),
                            Some(ElementNodeConditionKind::If)
                        ) && !analysis.v_if_repositioned.is_empty()
                            && !has_dynamic_is
                        {
                            inject_repositioned_comments(
                                idx,
                                children,
                                &analysis.v_if_repositioned,
                                ctx.ast,
                                ctx.source,
                                ctx.out,
                                child_el,
                                ctx.alloc,
                            );
                        }
                        if let Some(comment_indices) = analysis.v_for_repositioned.get(&idx) {
                            inject_ts_directive_comments_for_v_for(
                                comment_indices,
                                children,
                                ctx.ast,
                                ctx.source,
                                ctx.out,
                                child_el,
                            );
                        }
                    }

                    // Track IIFE close position
                    if !is_slot_template {
                        if let AstNodeKind::Element(child_el) = &ctx.ast.nodes[child_id.0].kind {
                            if let Some(ref cond) = child_el.v_condition {
                                let el_end = child_el
                                    .tag_close
                                    .as_ref()
                                    .map(|tc| tc.end)
                                    .unwrap_or(child_el.tag_open.end);
                                match cond.kind {
                                    ElementNodeConditionKind::If
                                    | ElementNodeConditionKind::ElseIf => {
                                        pending_iife_close_pos = Some(el_end);
                                    }
                                    ElementNodeConditionKind::Else => {
                                        pending_iife_close_pos = None;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // ── Non-chain child (normal processing) ──

            // Flush any pending IIFE close
            if let Some(pos) = pending_iife_close_pos.take() {
                ctx.out.prepend_alloc(pos, "}}");
            }

            // Set up component :is TS directives
            if let Some(comments) = analysis.component_is_comments.get(&idx) {
                ctx.ts_directives_for_component_is = comments.clone();
            }
            walk_node(child_id, ctx, parent_condition_scopes, ChainMode::Normal);
            ctx.ts_directives_for_component_is.clear();

            // Inject repositioned comments
            if let AstNodeKind::Element(child_el) = &ctx.ast.nodes[child_id.0].kind {
                let has_dynamic_is = is_dynamic_component_is(child_el, ctx.source);
                if matches!(
                    child_el.v_condition.as_ref().map(|c| &c.kind),
                    Some(ElementNodeConditionKind::If)
                ) && !analysis.v_if_repositioned.is_empty()
                    && !has_dynamic_is
                {
                    inject_repositioned_comments(
                        idx,
                        children,
                        &analysis.v_if_repositioned,
                        ctx.ast,
                        ctx.source,
                        ctx.out,
                        child_el,
                        ctx.alloc,
                    );
                }
                if let Some(comment_indices) = analysis.v_for_repositioned.get(&idx) {
                    inject_ts_directive_comments_for_v_for(
                        comment_indices,
                        children,
                        ctx.ast,
                        ctx.source,
                        ctx.out,
                        child_el,
                    );
                }

                // Solo v-if elements NOT in any chain still use IIFE tracking
                if let Some(ref cond) = child_el.v_condition {
                    let is_slot_template = child_el.tag_type == TagType::Template
                        && child_el.v_condition.is_some()
                        && child_el.v_slot.is_some();
                    if !is_slot_template {
                        let el_end = child_el
                            .tag_close
                            .as_ref()
                            .map(|tc| tc.end)
                            .unwrap_or(child_el.tag_open.end);
                        match cond.kind {
                            ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
                                pending_iife_close_pos = Some(el_end);
                            }
                            ElementNodeConditionKind::Else => {
                                pending_iife_close_pos = None;
                            }
                        }
                    }
                }
            }
        }
    }

    // After all children: flush any remaining pending IIFE close
    if let Some(pos) = pending_iife_close_pos.take() {
        ctx.out.prepend_alloc(pos, "}}");
    }
}

/// Check if a comment node contains a TypeScript directive (`@ts-expect-error`, `@ts-ignore`, `@ts-nocheck`).
fn is_ts_directive_comment(source: &str, comment: &CommentNode) -> bool {
    let content = source[comment.content_start as usize..comment.content_end as usize].trim();
    content.starts_with("@ts-expect-error")
        || content.starts_with("@ts-ignore")
        || content.starts_with("@ts-nocheck")
}

/// Result of pre-scanning children for comments that need repositioning.
struct ChildCommentAnalysis {
    /// Comment indices to reposition inside v-if IIFEs (ALL comments, existing behavior).
    v_if_repositioned: FxHashSet<usize>,
    /// TS directive comment indices to reposition inside v-for `.map()` callbacks.
    /// Key: element child index, Value: comment child indices (forward order).
    v_for_repositioned: FxHashMap<usize, Vec<usize>>,
    /// TS directive comment text to inject inside `<component :is>` IIFEs.
    /// Key: element child index, Value: trimmed comment content strings.
    component_is_comments: FxHashMap<usize, Vec<String>>,
}

/// Check if an element is a dynamic `<component :is="...">`.
fn is_dynamic_component_is(el: &ElementNode, source: &str) -> bool {
    el.tag_type == TagType::Component
        && &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize] == "component"
        && el.props.iter().any(|p| {
            p.is_directive
                && directive_name(p, source) == "bind"
                && p.arg_start
                    .zip(p.arg_end)
                    .map(|(a, b)| &source[a as usize..b as usize] == "is")
                    .unwrap_or(false)
        })
}

/// Pre-scan children to categorize comments that need repositioning.
///
/// - **v-if elements** (no dynamic `:is`): ALL preceding comments → `v_if_repositioned`
/// - **v-if + `<component :is>`**: TS directive comments → `component_is_comments`
/// - **v-for elements** (no v-if): TS directive comments → `v_for_repositioned`
/// - **v-for + v-if**: TS directive comments → `v_for_repositioned` (v-for is outermost)
/// - **`<component :is>` (no v-if, no v-for)**: TS directive comments → `component_is_comments`
fn analyze_child_comments(
    children: &[NodeId],
    ast: &crate::ast::types::TemplateAst,
    source: &str,
) -> ChildCommentAnalysis {
    let mut analysis = ChildCommentAnalysis {
        v_if_repositioned: FxHashSet::default(),
        v_for_repositioned: FxHashMap::default(),
        component_is_comments: FxHashMap::default(),
    };

    for (i, &child_id) in children.iter().enumerate() {
        let node = &ast.nodes[child_id.0];
        let AstNodeKind::Element(el) = &node.kind else {
            continue;
        };

        let has_v_if = el
            .v_condition
            .as_ref()
            .is_some_and(|c| c.kind == ElementNodeConditionKind::If);
        let has_v_for = el.v_for.is_some();
        let has_dynamic_is = is_dynamic_component_is(el, source);

        if has_v_if && !has_dynamic_is && !has_v_for {
            // Pure v-if (no :is, no v-for): reposition ALL comments (existing behavior)
            collect_preceding_comments(i, children, ast, source, |j, _| {
                analysis.v_if_repositioned.insert(j);
            });
        } else if has_v_for {
            // v-for (with or without v-if): reposition only TS directive comments
            collect_preceding_comments(i, children, ast, source, |j, comment| {
                if is_ts_directive_comment(source, comment) {
                    analysis.v_for_repositioned.entry(i).or_default().push(j);
                }
            });
            // Ensure forward order (collect walks backward)
            if let Some(v) = analysis.v_for_repositioned.get_mut(&i) {
                v.sort_unstable();
            }
        } else if has_dynamic_is {
            // <component :is> (with or without v-if): reposition TS directive comments
            collect_preceding_comments(i, children, ast, source, |j, comment| {
                if is_ts_directive_comment(source, comment) {
                    let text = source[comment.content_start as usize..comment.content_end as usize]
                        .trim()
                        .to_string();
                    analysis
                        .component_is_comments
                        .entry(i)
                        .or_default()
                        .push(text);
                    // Also mark for removal from original position
                    analysis.v_if_repositioned.insert(j);
                }
            });
        }
    }

    analysis
}

/// Walk backward from `element_idx` collecting consecutive preceding comments.
/// Calls `callback(child_index, comment_node)` for each comment found.
fn collect_preceding_comments(
    element_idx: usize,
    children: &[NodeId],
    ast: &crate::ast::types::TemplateAst,
    source: &str,
    mut callback: impl FnMut(usize, &CommentNode),
) {
    let mut j = element_idx;
    while j > 0 {
        j -= 1;
        let prev = &ast.nodes[children[j].0];
        match &prev.kind {
            AstNodeKind::Comment(c) => {
                callback(j, c);
            }
            AstNodeKind::Text(t) => {
                let text = &source[t.start as usize..t.end as usize];
                if text.trim().is_empty() {
                    continue;
                }
                break;
            }
            _ => break,
        }
    }
}

/// After walking a v-if element, emit repositioned comments inside the IIFE.
/// Comments are emitted in forward order at the element's tag_open position,
/// which places them after the IIFE open (`{()=>{if(cond){\n`} already prepended there.
#[allow(clippy::too_many_arguments)]
fn inject_repositioned_comments<'alloc>(
    v_if_idx: usize,
    children: &[NodeId],
    reposition_set: &rustc_hash::FxHashSet<usize>,
    ast: &crate::ast::types::TemplateAst,
    source: &str,
    out: &mut CodeGenOutput<'alloc>,
    el: &ElementNode,
    _alloc: &'alloc Allocator,
) {
    // Find the first repositioned comment before this v-if element
    let mut first_comment = v_if_idx;
    for j in (0..v_if_idx).rev() {
        if reposition_set.contains(&j) {
            first_comment = j;
        } else {
            let prev = &ast.nodes[children[j].0];
            if let AstNodeKind::Text(t) = &prev.kind {
                if source[t.start as usize..t.end as usize].trim().is_empty() {
                    continue; // Skip whitespace text between comments
                }
            }
            break;
        }
    }

    // Emit comments in forward order.
    // Use mapped_prepend (with offset = content.len() → effectively unmapped) so that
    // these comments stay in the mapped_prepends vec and maintain correct insertion order
    // relative to the IIFE opening which is also emitted via mapped_prepends.
    for (j, &child_id) in children
        .iter()
        .enumerate()
        .take(v_if_idx)
        .skip(first_comment)
    {
        if !reposition_set.contains(&j) {
            continue;
        }
        let prev = &ast.nodes[child_id.0];
        if let AstNodeKind::Comment(c) = &prev.kind {
            let text = &source[c.content_start as usize..c.content_end as usize];
            let jsx_comment = format!("{{/*{}*/}}\n", text);
            let len = jsx_comment.len() as u32;
            out.prepend_alloc_mapped_with_offset(el.tag_open.start, 0, len, &jsx_comment);
        }
    }
}

/// After walking a v-for element, inject TS directive comments inside the `.map()` callback.
///
/// Comments are emitted at `el.tag_open.start` via `prepend_alloc_mapped_with_offset`.
/// Because the v-for `.map()` open is also emitted at this position via earlier prepends,
/// and `CodeGenOutput` uses stable sort order (later prepends appear after earlier ones at
/// the same position), these comments land inside the callback body.
fn inject_ts_directive_comments_for_v_for(
    comment_indices: &[usize],
    children: &[NodeId],
    ast: &crate::ast::types::TemplateAst,
    source: &str,
    out: &mut CodeGenOutput<'_>,
    el: &ElementNode,
) {
    for &j in comment_indices {
        let prev = &ast.nodes[children[j].0];
        if let AstNodeKind::Comment(c) = &prev.kind {
            let text = source[c.content_start as usize..c.content_end as usize].trim();
            let jsx_comment = format!("{{/*{}*/}}\n", text);
            let len = jsx_comment.len() as u32;
            out.prepend_alloc_mapped_with_offset(el.tag_open.start, 0, len, &jsx_comment);
        }
    }
}

/// Build a [`ConditionScope`] for a v-if/v-else-if/v-else element.
///
/// Walks backward through siblings to collect sibling negation conditions,
/// and resolves the element's own condition with binding prefixes.
fn build_condition_scope<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &str,
    resolver: &BindingResolver<'alloc>,
    ast: &crate::ast::types::TemplateAst,
    node_id: NodeId,
) -> Option<ConditionScope> {
    let condition = el.v_condition.as_ref()?;

    // Resolve own condition expression (positive)
    let positive = match condition.kind {
        ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
            let (Some(vs), Some(ve)) = (condition.prop.value_start, condition.prop.value_end)
            else {
                return None;
            };
            let raw_expr = &source[vs as usize..ve as usize];
            Some(directives::resolve_condition_expr_pub(
                raw_expr, vs, oxc_el, resolver,
            ))
        }
        ElementNodeConditionKind::Else => None,
    };

    // Collect sibling negations by walking backward
    let sibling_negations = match condition.kind {
        ElementNodeConditionKind::If => vec![],
        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else => {
            collect_sibling_negations(ast, node_id, source, resolver)
        }
    };

    Some(ConditionScope {
        positive,
        sibling_negations,
    })
}

/// Walk backward through siblings of a v-else-if/v-else element to collect
/// the resolved condition expressions of preceding v-if and v-else-if elements.
fn collect_sibling_negations<'alloc>(
    ast: &crate::ast::types::TemplateAst,
    node_id: NodeId,
    source: &str,
    resolver: &BindingResolver<'alloc>,
) -> Vec<String> {
    let mut negations = Vec::new();
    let mut current = node_id;

    while let Some(prev) = ast.prev_sibling(current) {
        let prev_node = &ast.nodes[prev.0];
        match &prev_node.kind {
            AstNodeKind::Element(prev_el) => {
                if let Some(ref cond) = prev_el.v_condition {
                    // Resolve the sibling's condition expression
                    if let (Some(vs), Some(ve)) = (cond.prop.value_start, cond.prop.value_end) {
                        let raw_expr = &source[vs as usize..ve as usize];
                        let resolved = resolver.resolve_simple_expr(raw_expr);
                        negations.push(resolved);
                    }

                    // If we hit a v-if, that's the start of the chain — stop
                    if matches!(cond.kind, ElementNodeConditionKind::If) {
                        break;
                    }
                } else {
                    // Non-conditional element — stop (not part of the chain)
                    break;
                }
            }
            AstNodeKind::Text(text) => {
                // Skip whitespace-only text nodes
                let t = &source[text.start as usize..text.end as usize];
                if t.trim().is_empty() {
                    current = prev;
                    continue;
                }
                break; // Non-whitespace text — stop
            }
            AstNodeKind::Comment(_) => {
                // Skip comments
                current = prev;
                continue;
            }
            _ => break,
        }
        current = prev;
    }

    // Reverse so they're in chain order (v-if first, then v-else-if's)
    negations.reverse();
    negations
}

/// Result of rewriting Vue's polymorphic `<component :is>` tag.
///
/// `tag_name` is the name present in generated JSX. The slot-body isolation
/// pair must close this exact name while mapping it to the authored closing tag.
struct ComponentIsRewrite<'a> {
    tag_name: &'a str,
    needs_iife_close: bool,
}

/// Rewrite `<component :is="expr">` to use `extractRenderComponent`.
/// Returns the emitted JSX tag identity when a static or dynamic `is` rewrite
/// occurred; dynamic rewrites also require an IIFE close after the element.
///
/// `ts_directives`: TS directive comment texts (e.g., `"@ts-expect-error"`) to inject
/// inside the IIFE before `return`, so they suppress errors on the resolved component.
#[allow(clippy::too_many_arguments)]
fn rewrite_component_is<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
    components: &TemplateComponentBindings,
    ts_directives: &[String],
    emit_ctx: EmitContext,
) -> Option<ComponentIsRewrite<'alloc>> {
    let static_is_prop = el.props.iter().find(|prop| {
        if prop.is_directive {
            return false;
        }
        &source[prop.start as usize..prop.name_end as usize] == "is"
    });

    // 1) Static `is="div"`
    if let Some(is_prop) = static_is_prop {
        let (Some(value_start), Some(value_end)) = (is_prop.value_start, is_prop.value_end) else {
            return None;
        };
        if value_end <= value_start {
            return None;
        }

        let target_tag = source[value_start as usize..value_end as usize].trim();
        if target_tag.is_empty() {
            return None;
        }

        // Resolve the target through the shared component inventory first: a
        // kebab or PascalCase COMPONENT name (local binding or GlobalComponents
        // fallback const) rewrites to its in-scope binding so the JSX resolves
        // the const rather than an intrinsic. A native tag (`is="div"`) or an
        // unresolvable name keeps the verbatim target (fail-closed).
        let resolved_target = components
            .resolve(target_tag, TagType::Component, |n| {
                resolver.get(n).is_some()
            })
            .filter(|resolved| resolved != target_tag)
            .map(|resolved| out.alloc_str(&resolved) as &str);
        let emit_tag = resolved_target.unwrap_or(target_tag);

        rewrite_component_tag_name(el, emit_tag, out);

        // Remove `is="..."`
        let is_prop_end = props::get_prop_end(is_prop);
        out.overwrite(is_prop.start, is_prop_end, "");
        return Some(ComponentIsRewrite {
            tag_name: emit_tag,
            needs_iife_close: false,
        });
    }

    // 2) Dynamic `:is="expr"` / `v-bind:is="expr"`
    let bind_is_result = el.props.iter().enumerate().find(|(_, prop)| {
        if !prop.is_directive || prop.is_dynamic == Some(true) {
            return false;
        }
        if directive_name(prop, source) != "bind" {
            return false;
        }
        let (Some(arg_start), Some(arg_end)) = (prop.arg_start, prop.arg_end) else {
            return false;
        };
        source[arg_start as usize..arg_end as usize].trim() == "is"
    });

    let (bind_is_index, bind_is_prop) = bind_is_result?;

    let (Some(value_start), Some(value_end)) = (bind_is_prop.value_start, bind_is_prop.value_end)
    else {
        return None;
    };
    if value_end <= value_start {
        return None;
    }

    let value_expr = source[value_start as usize..value_end as usize].trim();
    if value_expr.is_empty() {
        return None;
    }

    // All dynamic :is expressions use the extractRenderComponent wrapper. The
    // raw `:is` expression resolves through the shared segmented producer so the
    // authored identifiers carry their source offsets while the resolver
    // scaffolding (`_ctx.` / `__props.` prefixes, `.value` suffixes) stays
    // unmapped.
    let oxc_prop = oxc_el.and_then(|el| el.prop(bind_is_index));
    let resolved = if let Some(oxc_p) = oxc_prop {
        if let Some(ref exp) = oxc_p.exp {
            build_prefixed_expr_segments(value_expr, value_start, exp, resolver, &[])
        } else {
            resolve_simple_expr_segments(resolver, value_expr, value_start)
        }
    } else {
        resolve_simple_expr_segments(resolver, value_expr, value_start)
    };

    // Wrap in IIFE so the `const` declaration is valid in any context.
    // In JSX children: {(() => { const comp = ...; return <comp/>; })()}
    // In v-for body:    (() => { const comp = ...; return <comp/>; })()
    // The outer {} is JSX expression syntax — needed in JSX but causes
    // a parse error in v-for's `=> (...)` expression context (parsed as object literal).
    let temp_name = "___VERTER___component_render";
    let iife_prefix = if emit_ctx == EmitContext::Expression {
        format!(
            "(() => {{ const {}=___VERTER___extractRenderComponent(",
            temp_name
        )
    } else {
        format!(
            "{{(() => {{ const {}=___VERTER___extractRenderComponent(",
            temp_name
        )
    };
    // Insert TS directive comments (e.g., `/* @ts-expect-error */`) between `);` and `return`
    // so they suppress errors on the resolved component element.
    let ts_comment_text = if ts_directives.is_empty() {
        String::new()
    } else {
        let mut buf = String::new();
        for d in ts_directives {
            buf.push_str(&format!(" /* {} */\n", d));
        }
        buf
    };
    // Assemble the IIFE scaffolding around the mapped expression into one plan:
    // `iife_prefix` opens the wrapper call, the suffix closes it (`);`), emits any
    // TS directive comments, then ` return `. Both wrappers are synthetic, so only
    // the authored `:is` identifiers carry source-map tokens — TSGO maps hover
    // positions back to the Vue template through them.
    let suffix = format!(");{} return ", ts_comment_text);
    let plan = resolved.wrapped(&iife_prefix, &suffix);
    out.prepend_mapped_generated_text(el.tag_open.start, &plan);
    rewrite_component_tag_name(el, temp_name, out);

    // Remove `:is="..."`
    let prop_end = props::get_prop_end(bind_is_prop);
    out.overwrite(bind_is_prop.start, prop_end, "");
    Some(ComponentIsRewrite {
        tag_name: temp_name,
        needs_iife_close: true,
    })
}

fn rewrite_component_tag_name(el: &ElementNode, target_tag: &str, out: &mut CodeGenOutput<'_>) {
    // Rewrite opening `<component` to `<targetTag`.
    out.overwrite(el.tag_open.start + 1, el.tag_open.name_end, target_tag);

    // Rewrite closing `</component>` (or `</component :is="as">`) if present.
    // Use `end - 1` instead of `name_end` to strip any trailing attributes on
    // the closing tag (e.g., `</component :is="as">` is technically valid HTML
    // but produces invalid JSX if the attributes are preserved).
    if let Some(tag_close) = &el.tag_close {
        out.overwrite(tag_close.start + 2, tag_close.end - 1, target_tag);
    }
}

/// Walk backwards from `pos` to consume preceding ASCII whitespace (spaces/tabs).
/// Returns the earliest position that is still whitespace, so the overwrite range
/// `[eat_leading_whitespace(source, prop.start) .. prop_end]` removes the attribute
/// AND the space before it (e.g., `<div v-once>` → `<div>`).
fn eat_leading_whitespace(source: &str, pos: u32) -> u32 {
    let bytes = source.as_bytes();
    let mut i = pos as usize;
    while i > 0 && matches!(bytes[i - 1], b' ' | b'\t') {
        i -= 1;
    }
    i as u32
}

fn directive_name<'a>(prop: &crate::types::NodeProp, source: &'a str) -> &'a str {
    let name = &source[prop.start as usize..prop.name_end as usize];
    if name.starts_with(':') || name.starts_with('.') {
        return "bind";
    }
    if name.starts_with('@') {
        return "on";
    }
    if name.starts_with('#') {
        return "slot";
    }
    name.strip_prefix("v-").unwrap_or(name)
}

/// Check if a string is a valid JS identifier (can be used as a bare property name).
fn is_valid_js_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Extracted slot name info for fine-grained source map overwrites.
struct SlotNameInfo<'a> {
    /// The slot name string (e.g., "header")
    name: &'a str,
    /// Source position of the name value start (inside quotes)
    value_start: u32,
    /// Source position of the name value end (before closing quote)
    value_end: u32,
}

/// Extract the slot name from a `<slot>` element's attributes.
/// Returns the name string and its source positions for sourcemap mapping.
///
/// - `<slot>` → `None` (will use "default")
/// - `<slot name="header">` → `Some(SlotNameInfo { name: "header", value_start, value_end })`
/// - `<slot :name="dynamicName">` → `None` (dynamic, falls back to "default")
fn extract_slot_name<'a>(el: &ElementNode, source: &'a str) -> Option<SlotNameInfo<'a>> {
    for prop in &el.props {
        if !prop.is_directive {
            let attr_name = &source[prop.start as usize..prop.name_end as usize];
            if attr_name == "name" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let name = source[vs as usize..ve as usize].trim();
                    if !name.is_empty() {
                        return Some(SlotNameInfo {
                            name,
                            value_start: vs,
                            value_end: ve,
                        });
                    }
                }
            }
        } else {
            let dir_name = directive_name(prop, source);
            if dir_name == "bind" {
                if let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) {
                    let arg = &source[arg_s as usize..arg_e as usize];
                    if arg == "name" {
                        // Dynamic :name — can't resolve statically
                        return None;
                    }
                }
            }
        }
    }
    None
}

/// Collect slot outlet props as a comma-separated string of `key: value` pairs.
///
/// Excludes the `name` attribute (used for slot identification, not passed as props).
fn collect_slot_props(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'_>>,
    source: &str,
    resolver: &BindingResolver<'_>,
) -> String {
    use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;

    let mut parts = Vec::new();

    for (i, prop) in el.props.iter().enumerate() {
        if !prop.is_directive {
            let attr_name = &source[prop.start as usize..prop.name_end as usize];
            if attr_name == "name" {
                continue; // Skip name attribute
            }
            let key = quote_prop_key_if_needed(attr_name);
            // Static attribute: name="value"
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                parts.push(format!("{}: \"{}\"", key, value));
            } else {
                parts.push(format!("{}: true", key));
            }
        } else {
            let dir_name = directive_name(prop, source);
            match dir_name {
                "bind" => {
                    if let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) {
                        let arg = &source[arg_s as usize..arg_e as usize];
                        if arg == "name" {
                            continue; // Skip :name
                        }
                        let key = quote_prop_key_if_needed(arg);
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let raw = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc_el.and_then(|e| e.prop(i));
                            let resolved = if let Some(oxc_p) = oxc_prop {
                                if let Some(ref exp) = oxc_p.exp {
                                    build_prefixed_expr(raw, vs, exp, resolver, &[])
                                } else {
                                    resolver.resolve_simple_expr(raw)
                                }
                            } else {
                                resolver.resolve_simple_expr(raw)
                            };
                            parts.push(format!("{}: {}", key, resolved));
                        }
                    }
                }
                _ => {
                    // Skip other directives on slot outlets (v-if etc handled separately)
                }
            }
        }
    }

    parts.join(", ")
}

/// Quote a property key if it's not a valid JS identifier (e.g., contains hyphens).
/// `item-class` → `"item-class"`, `itemClass` → `itemClass` (unchanged).
fn quote_prop_key_if_needed(key: &str) -> String {
    if is_valid_js_ident(key) {
        key.to_string()
    } else {
        format!("\"{}\"", key)
    }
}

/// Visit a text node.
///
/// To keep TSX valid for content like `2 < 1`, non-empty trimmed text is wrapped
/// as a string expression (`{"..."}`), matching v5/process text-plugin semantics.
/// Whitespace-only text remains unchanged.
fn visit_text(text: &TextNode, source: &str, out: &mut CodeGenOutput<'_>) {
    if text.end <= text.start {
        return;
    }

    let raw_text = &source[text.start as usize..text.end as usize];
    let trimmed = raw_text.trim();
    if trimmed.is_empty() || trimmed == "<" {
        return;
    }

    let Some(rel_start) = raw_text.find(trimmed) else {
        return;
    };

    let content_start = text.start + rel_start as u32;
    let content_end = content_start + trimmed.len() as u32;

    // Escape characters that are invalid inside a `"..."` JS string literal:
    // - double quotes → \"
    // - newlines → \n (multi-line strings are illegal in JS)
    // - carriage returns → \r
    // - backslashes → \\ (must escape first to avoid double-escaping)
    for (i, b) in trimmed.as_bytes().iter().enumerate() {
        let pos = content_start + i as u32;
        match *b {
            b'\\' => out.overwrite(pos, pos + 1, "\\\\"),
            b'"' => out.overwrite(pos, pos + 1, "\\\""),
            b'\n' => out.overwrite(pos, pos + 1, "\\n"),
            b'\r' => out.overwrite(pos, pos + 1, "\\r"),
            _ => {}
        }
    }

    out.prepend_alloc(content_start, "{\"");
    out.prepend_alloc(content_end, "\"}");
}

/// Visit an interpolation node: `{{ expr }}` → `{expr}`.
fn visit_interpolation<'alloc>(
    interp: &InterpolationNode,
    oxc_expr: Option<&crate::template::oxc::types::OxcParsedExpression<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
) {
    // Replace `{{` with `{`
    out.overwrite(interp.start, interp.inner_start, "{");

    if let Some(expr) = oxc_expr {
        if expr.expression.is_none() && expr.errors.is_some() {
            let raw_expr = &source[interp.inner_start as usize..interp.inner_end as usize];
            recover_broken_interpolation_expr(raw_expr, interp.inner_start, out, resolver);
        } else if let Some(ref bindings) = expr.bindings {
            // Apply binding prefixes to expression identifiers for valid expressions.
            resolver.collect_binding_patches(bindings, out);
        }
    }

    // Replace `}}` with `}`
    out.overwrite(interp.inner_end, interp.end, "}");
}

struct RecoveredInterpolationIdent {
    start: usize,
    end: usize,
    patch: RecoveredInterpolationPatch,
}

enum RecoveredInterpolationPatch {
    PrefixSuffix {
        ident: String,
        prefix: &'static str,
        suffix: &'static str,
    },
    /// The resolved form is NOT a plain `prefix + ident + suffix` (e.g. a keyword
    /// rewritten to bracket notation `$props["class"]`). The recovered identifier
    /// `ident` still occurs inside `resolved`, so it is emitted via the unified
    /// synthesized-shorthand path: the `ident` core maps to its source token, the
    /// surrounding accessor scaffolding is unmapped — never baked into one mapped
    /// overwrite of the identifier span.
    SynthesizedResolved { ident: String, resolved: String },
}

fn recover_broken_interpolation_expr<'alloc>(
    raw_expr: &str,
    expr_start: u32,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
) {
    let bytes = raw_expr.as_bytes();
    let mut recovered = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let byte = bytes[i];
        if is_ident_start(byte)
            && (i == 0 || !(is_ident_continue(bytes[i - 1]) || bytes[i - 1] == b'.'))
        {
            let start = i;
            i += 1;
            while i < bytes.len() && is_ident_continue(bytes[i]) {
                i += 1;
            }
            let ident = &raw_expr[start..i];
            let resolved = resolver.resolve_simple_expr(ident);
            let is_passthrough_keyword_or_global = resolved == ident
                && (crate::utils::oxc::bindings::keywords::is_keyword(ident.as_bytes())
                    || crate::utils::oxc::bindings::keywords::is_global(ident.as_bytes()));
            if is_passthrough_keyword_or_global {
                continue;
            }

            let prefix = resolver.resolve_prefix(ident);
            let suffix = resolver.resolve_suffix(ident);
            let simple_resolved = format!("{prefix}{ident}{suffix}");
            let patch = if resolved == simple_resolved {
                RecoveredInterpolationPatch::PrefixSuffix {
                    ident: ident.to_string(),
                    prefix,
                    suffix,
                }
            } else {
                RecoveredInterpolationPatch::SynthesizedResolved {
                    ident: ident.to_string(),
                    resolved,
                }
            };
            recovered.push(RecoveredInterpolationIdent {
                start,
                end: i,
                patch,
            });
            continue;
        }
        i += 1;
    }

    if recovered.is_empty() {
        out.overwrite(expr_start, expr_start + raw_expr.len() as u32, "undefined");
        return;
    }

    let mut sanitized = vec![b' '; raw_expr.len()];
    for recovered_ident in &recovered {
        sanitized[recovered_ident.start..recovered_ident.end]
            .copy_from_slice(&bytes[recovered_ident.start..recovered_ident.end]);
    }

    for pair in recovered.windows(2) {
        let gap_start = pair[0].end;
        let gap_end = pair[1].start;
        if gap_start >= gap_end {
            continue;
        }
        let separator_offset = bytes[gap_start..gap_end]
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .unwrap_or(0);
        sanitized[gap_start + separator_offset] = b',';
    }

    let mut start = 0usize;
    while start < bytes.len() {
        if bytes[start] == sanitized[start] {
            start += 1;
            continue;
        }

        let mut end = start + 1;
        while end < bytes.len() && bytes[end] != sanitized[end] {
            end += 1;
        }

        let replacement =
            std::str::from_utf8(&sanitized[start..end]).expect("recovered interpolation is ASCII");
        out.overwrite(
            expr_start + start as u32,
            expr_start + end as u32,
            replacement,
        );
        start = end;
    }

    for recovered_ident in recovered {
        match recovered_ident.patch {
            RecoveredInterpolationPatch::PrefixSuffix {
                ident,
                prefix,
                suffix,
            } => {
                if !prefix.is_empty() {
                    out.prepend_static(expr_start + recovered_ident.start as u32, prefix);
                }
                if !suffix.is_empty() {
                    out.prepend_static(
                        expr_start + recovered_ident.start as u32 + ident.len() as u32,
                        suffix,
                    );
                }
            }
            RecoveredInterpolationPatch::SynthesizedResolved { ident, resolved } => {
                // Delete the recovered identifier span (replace with "" — unmapped)
                // and re-emit the resolved form through the unified
                // synthesized-shorthand path: the `ident` core maps back to its
                // source token, the accessor scaffolding (`$props["` / `"]`) is
                // unmapped. The deletion writes an empty string, so it is never a
                // mapped bake of the resolved expression onto the identifier span.
                let ident_start = expr_start + recovered_ident.start as u32;
                out.overwrite(ident_start, expr_start + recovered_ident.end as u32, "");
                emit_synthesized_shorthand_value(
                    out,
                    SourceByteOffset(ident_start),
                    &resolved,
                    &ident,
                    SourceByteOffset(ident_start),
                );
            }
        }
    }
}

#[inline]
fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

#[inline]
fn is_ident_continue(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

/// Visit a comment node: `<!-- text -->` → `{/* text */}`.
fn visit_comment(
    comment: &CommentNode,
    out: &mut CodeGenOutput<'_>,
    options: &IdeTemplateOptions<'_>,
) {
    if !options.comments {
        // Strip comment entirely
        out.overwrite(comment.start, comment.end, "");
        return;
    }

    // Convert HTML comment to JSX comment
    // <!-- → {/*  and  --> → */}
    // Keep original comment-inner spacing untouched.
    out.overwrite(comment.start, comment.content_start, "{/*");
    out.overwrite(comment.content_end, comment.end, "*/}");
}

// ── Strict slot children ────────────────────────────────────────

/// Emit `strictRenderSlot` entries for a component from its slot summary.
///
/// Reads the shared overlay's per-component [`ComponentSlotSummary`] (built once
/// for this component, then memoized) instead of re-scanning the component's
/// children. Each non-empty slot group becomes one [`StrictSlotEntry`]; the
/// per-child source spans are resolved from each fact's AST node so the emitted
/// bytes match a direct scan.
fn collect_strict_slot_children(
    summary: &ComponentSlotSummary,
    el: &ElementNode,
    ctx: &mut IdeTemplateCtx<'_, '_>,
) {
    #[cfg(test)]
    crate::template::oxc::slot_summary::record_slot_summary_read();

    let parent_offset = el.tag_open.start;
    for group in &summary.groups {
        let children = group
            .children
            .iter()
            .map(|fact| strict_child_from_fact(fact, ctx.ast, ctx.source))
            .collect();
        ctx.strict_slot_entries.push(StrictSlotEntry {
            parent_comp_offset: parent_offset,
            slot_name: group.name.clone(),
            children,
        });
    }
}

/// Resolve a recorded [`SlotChildFact`] into the emit-time strict-slot child
/// reference, reading the tag-name / text source span straight from the fact's
/// AST node. The slot scan records each fact's kind from the node it came from,
/// so the fact kind always matches its node kind here.
fn strict_child_from_fact(
    fact: &SlotChildFact,
    ast: &crate::ast::types::TemplateAst,
    source: &str,
) -> StrictSlotChild {
    match (fact.kind, &ast.nodes[fact.node_id.0].kind) {
        (SlotChildKind::Component, AstNodeKind::Element(el)) => {
            let name_start = el.tag_open.start + 1;
            StrictSlotChild::Component {
                name: source[name_start as usize..el.tag_open.name_end as usize].to_string(),
                source_pos: name_start,
            }
        }
        (SlotChildKind::HtmlElement, AstNodeKind::Element(el)) => {
            let name_start = el.tag_open.start + 1;
            StrictSlotChild::HtmlElement {
                tag: source[name_start as usize..el.tag_open.name_end as usize].to_string(),
                source_pos: name_start,
            }
        }
        (SlotChildKind::Text, AstNodeKind::Text(text)) => StrictSlotChild::Text {
            source_pos: text.start,
        },
        (SlotChildKind::Interpolation, AstNodeKind::Interpolation(interp)) => {
            StrictSlotChild::Interpolation {
                source_pos: interp.start,
            }
        }
        _ => unreachable!("slot child fact kind does not match its AST node kind"),
    }
}

/// Emit `strictRenderSlot` calls for all collected strict slot entries.
///
/// Each call is split into unmapped and mapped segments. Child constructor
/// names are emitted as `InsertedMapped` chunks so that TypeScript errors
/// on mismatched children point to the exact child position in the template.
fn emit_strict_slot_checks(ctx: &mut IdeTemplateCtx<'_, '_>, emit_pos: u32) {
    let entries = std::mem::take(&mut ctx.strict_slot_entries);
    let prefix = "___VERTER___";

    for entry in &entries {
        // 1. Unmapped prefix: call site + slot type extraction
        let call_prefix = format!(
            "\n{prefix}strictRenderSlot({{}} as NonNullable<ReturnType<typeof {prefix}Comp{offset}>['$slots']['{slot}']>, [",
            prefix = prefix,
            offset = entry.parent_comp_offset,
            slot = entry.slot_name,
        );
        ctx.out.prepend_alloc(emit_pos, &call_prefix);

        // 2. Per-child: mapped reference with sourcemap token
        for (i, child) in entry.children.iter().enumerate() {
            if i > 0 {
                ctx.out.prepend_alloc(emit_pos, ", ");
            }
            match child {
                StrictSlotChild::Component {
                    name, source_pos, ..
                } => {
                    // Component constructor: mapped to tag name position
                    ctx.out.prepend_alloc_mapped(emit_pos, *source_pos, name);
                }
                StrictSlotChild::HtmlElement {
                    tag, source_pos, ..
                } => {
                    // HTML element: `{} as HTMLElementTagNameMap["input"]`
                    // Map the tag name inside the string to its template position
                    let content = format!("{{}} as HTMLElementTagNameMap[\"{}\"]", tag);
                    // content_offset points to the tag name inside the quotes
                    let content_offset = "{} as HTMLElementTagNameMap[\"".len() as u32;
                    ctx.out.prepend_alloc_mapped_with_offset(
                        emit_pos,
                        *source_pos,
                        content_offset,
                        &content,
                    );
                }
                StrictSlotChild::Text { source_pos } => {
                    // Text: `"" as string`, mapped to text start
                    ctx.out
                        .prepend_alloc_mapped(emit_pos, *source_pos, "\"\" as string");
                }
                StrictSlotChild::Interpolation { source_pos } => {
                    // Interpolation: `"" as string`, mapped to interpolation start
                    ctx.out
                        .prepend_alloc_mapped(emit_pos, *source_pos, "\"\" as string");
                }
            }
        }

        // 3. Unmapped suffix
        ctx.out.prepend_alloc(emit_pos, "]);");
    }

    // Emit required slot checks after strict slot checks
    let required_checks = std::mem::take(&mut ctx.required_slot_checks);
    for check in &required_checks {
        let mut provided = String::from("{ ");
        for (i, name) in check.provided_slot_names.iter().enumerate() {
            if i > 0 {
                provided.push_str(", ");
            }
            // Quote names containing hyphens or other special chars
            if name.contains('-') || name.contains(' ') {
                provided.push_str(&format!("'{}': true", name));
            } else {
                provided.push_str(&format!("{}: true", name));
            }
        }
        provided.push_str(" }");

        let call = format!(
            "\n{prefix}checkRequiredSlots({{}} as NonNullable<ReturnType<typeof {prefix}Comp{offset}>['$slots']>, {provided});",
            prefix = prefix,
            offset = check.comp_offset,
            provided = provided,
        );
        ctx.out
            .prepend_alloc_mapped(emit_pos, check.source_pos, &call);
    }
}

/// Emit a `checkRequiredSlots` entry for a component from its slot summary.
///
/// Reads the shared overlay's `provided_slot_names` (the same per-component
/// summary that feeds [`collect_strict_slot_children`], built once then
/// memoized) instead of re-scanning the component's children. Every component
/// usage records a check — a self-closing component yields an empty
/// provided-names list.
fn collect_required_slots_check(
    summary: &ComponentSlotSummary,
    el: &ElementNode,
    ctx: &mut IdeTemplateCtx<'_, '_>,
) {
    #[cfg(test)]
    crate::template::oxc::slot_summary::record_slot_summary_read();

    let comp_offset = el.tag_open.start;
    ctx.required_slot_checks.push(RequiredSlotsCheck {
        comp_offset,
        provided_slot_names: summary.provided_slot_names.clone(),
        source_pos: comp_offset,
    });
}

#[cfg(test)]
mod tests;
