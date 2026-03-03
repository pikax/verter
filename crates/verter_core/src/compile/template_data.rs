//! Raw template data extracted during compilation.
//!
//! These are core-native types with no dependency on `verter_analysis`.
//! `verter_host` converts them into `verter_analysis::TemplateAnalysisSnapshot`.

use crate::ast::types::{AstNodeKind, ElementNode, TemplateAst};
use crate::common::Span;
use crate::template::code_gen::binding::BindingType;
use crate::template::oxc::types::{OxcNodeData, OxcParsedAst};
use crate::types::NodeId;
use rustc_hash::FxHashMap;

/// Raw template data extracted during compilation.
/// `verter_host` converts this to `verter_analysis::TemplateAnalysisSnapshot`.
#[derive(Debug, Default)]
pub struct RawTemplateData {
    pub components: Vec<RawComponentUsage>,
    pub binding_occurrences: Vec<RawBindingOccurrence>,
    pub elements: Vec<RawElementData>,
    pub slot_definitions: Vec<RawSlotDef>,
    pub template_refs: Vec<RawTemplateRef>,
    pub event_handlers: Vec<RawEventHandler>,
    pub v_for_directives: Vec<RawVForData>,
    pub v_model_directives: Vec<RawVModelData>,
    pub if_chains: Vec<RawIfChain>,
    pub comment_directives: Vec<RawCommentDirective>,
    pub max_nesting_depth: u16,
}

/// A component usage in the template.
#[derive(Debug, Clone)]
pub struct RawComponentUsage {
    pub tag_name: String,
    pub is_dynamic: bool,
    pub props: Vec<RawPropData>,
    pub has_spread: bool,
    pub slots_used: Vec<String>,
    /// Static class names from `class="foo bar"`.
    pub static_classes: Vec<String>,
    /// Whether `:class="..."` is present.
    pub has_dynamic_class: bool,
    /// Raw `:class` expression text (for dynamic class name extraction by verter_host).
    pub dynamic_class_expr: Option<String>,
    pub span: Span,
}

/// A single prop passed to a component or element.
#[derive(Debug, Clone)]
pub struct RawPropData {
    pub name: String,
    pub is_bound: bool,
    pub expression: Option<String>,
    pub referenced_bindings: Vec<String>,
    /// Whether all referenced bindings resolve to static (const) types.
    /// `None` = unknown (parse error, complex expression).
    pub all_bindings_static: Option<bool>,
    pub from_spread: bool,
    /// Byte span of the prop attribute in the SFC source.
    pub span: Span,
}

/// A script binding referenced at a specific position in the template.
#[derive(Debug, Clone)]
pub struct RawBindingOccurrence {
    pub name: String,
    pub span: Span,
    /// Whether this binding exists in the script bindings map.
    pub is_in_bindings_map: bool,
    /// Usage kind: 0=interpolation, 1=directive, 2=event, 3=component, 4=ref, 5=iterator
    pub usage_kind: u8,
}

/// Raw element data for linter/analysis.
#[derive(Debug, Clone)]
pub struct RawElementData {
    pub tag: String,
    pub is_component: bool,
    pub is_self_closing: bool,
    pub has_v_if: bool,
    pub has_v_else: bool,
    pub has_v_else_if: bool,
    pub has_v_show: bool,
    pub has_v_html: bool,
    pub has_v_text: bool,
    /// Whether this element has non-whitespace text or interpolation children.
    pub has_text_content: bool,
    /// Whether this element has direct child elements (non-text, non-comment children).
    pub has_element_children: bool,
    pub nesting_depth: u16,
    pub parent_tag: Option<String>,
    /// Index of the parent element in `RawTemplateData::elements`. `None` for root elements.
    pub parent_index: Option<u32>,
    pub span: Span,
    /// Byte offset end of the opening tag only (past the `>`).
    pub tag_span_end: u32,
    pub attributes: Vec<RawAttributeData>,
    pub directives: Vec<RawDirectiveData>,
    /// Index into `RawTemplateData::v_for_directives` if this element has v-for.
    pub v_for_idx: Option<usize>,
    /// Index into `RawTemplateData::v_model_directives` if this element has v-model.
    pub v_model_idx: Option<usize>,
}

/// A static or dynamic attribute on an element (non-directive).
#[derive(Debug, Clone)]
pub struct RawAttributeData {
    pub name: String,
    pub value: Option<String>,
    pub is_dynamic: bool,
    pub span: Span,
}

/// A directive on an element.
#[derive(Debug, Clone)]
pub struct RawDirectiveData {
    /// Normalized directive name: `"on"`, `"bind"`, `"if"`, `"for"`, `"model"`, etc.
    pub name: String,
    /// Raw directive as written in source: `"@click"`, `":class"`, `"v-if"`, etc.
    pub raw_name: String,
    pub argument: Option<String>,
    pub modifiers: Vec<String>,
    pub expression: Option<String>,
    pub span: Span,
}

/// A slot defined in this component's template.
#[derive(Debug, Clone)]
pub struct RawSlotDef {
    pub name: String,
    pub has_bindings: bool,
    /// Prop names from `:prop` bindings on the `<slot>` element.
    pub binding_names: Vec<String>,
    pub span: Span,
}

/// A template ref attribute.
#[derive(Debug, Clone)]
pub struct RawTemplateRef {
    pub name: String,
    pub is_dynamic: bool,
    pub target_tag: String,
    pub span: Span,
}

/// An event handler in the template.
#[derive(Debug, Clone)]
pub struct RawEventHandler {
    pub event_name: String,
    pub handler_expression: Option<String>,
    pub is_inline: bool,
    /// The tag name of the element this handler is on.
    pub target_tag: String,
    pub span: Span,
}

/// v-for directive data.
#[derive(Debug, Clone)]
pub struct RawVForData {
    pub variable: String,
    pub index: Option<String>,
    pub iterable: String,
    pub has_key: bool,
    pub key_expression: Option<String>,
    pub key_uses_index: bool,
    pub span: Span,
}

/// v-model directive data.
#[derive(Debug, Clone)]
pub struct RawVModelData {
    pub binding_name: String,
    pub modifiers: Vec<String>,
    pub target_is_component: bool,
    pub target_tag: String,
    pub span: Span,
}

/// A v-if/v-else-if chain.
#[derive(Debug, Clone)]
pub struct RawIfChain {
    pub conditions: Vec<(String, u32, u32)>,
}

/// A comment directive (e.g., `<!-- @verter:disable no-v-html -->`).
#[derive(Debug, Clone)]
pub struct RawCommentDirective {
    pub kind: u8, // 0=disable, 1=disable-next-line, 2=enable, 3=todo, 4=fixme, 5=deprecated, 6=ignore-start, 7=ignore-end, 8=level
    pub rule_or_message: Option<String>,
    pub span: Span,
    pub affects_next_line: bool,
}

// ── Extraction ────────────────────────────────────────────────────

/// Immutable context for template data extraction.
struct ExtractCtx<'a> {
    ast: &'a TemplateAst,
    oxc_ast: &'a OxcParsedAst<'a>,
    source: &'a str,
    bindings: &'a FxHashMap<&'a str, BindingType>,
}

/// Extract raw template data from a compiled template AST.
///
/// Called after script codegen (bindings available) and after OXC expression
/// parsing, but independent of the template codegen backend (VDOM/Vapor).
pub fn extract_raw_template_data(
    ast: &TemplateAst,
    oxc_ast: &OxcParsedAst<'_>,
    source: &str,
    bindings: &FxHashMap<&str, BindingType>,
) -> RawTemplateData {
    let mut data = RawTemplateData::default();
    let mut max_depth: u16 = 0;
    let ctx = ExtractCtx {
        ast,
        oxc_ast,
        source,
        bindings,
    };

    // Track v-if chains: when we see v-if, start a new chain.
    // v-else-if/v-else extend the current chain.
    let mut current_if_chain: Option<RawIfChain> = None;

    // Walk the root children
    if let Some(ref content) = ast.root.content {
        for &child_id in &content.children {
            walk_node_for_extraction(
                &ctx,
                child_id,
                0, // depth
                None,
                None, // parent_element_index
                &mut data,
                &mut max_depth,
                &mut current_if_chain,
            );
        }
    }

    // Flush any pending if-chain
    if let Some(chain) = current_if_chain.take() {
        if chain.conditions.len() > 1 {
            data.if_chains.push(chain);
        }
    }

    data.max_nesting_depth = max_depth;
    data
}

#[allow(clippy::too_many_arguments)]
fn walk_node_for_extraction(
    ctx: &ExtractCtx<'_>,
    node_id: NodeId,
    depth: u16,
    parent_tag: Option<&str>,
    parent_element_index: Option<u32>,
    data: &mut RawTemplateData,
    max_depth: &mut u16,
    current_if_chain: &mut Option<RawIfChain>,
) {
    let node = &ctx.ast.nodes[node_id.0];
    let oxc_data = &ctx.oxc_ast.data[node_id.0];

    match &node.kind {
        AstNodeKind::Element(el) => {
            let current_depth = depth + 1;
            if current_depth > *max_depth {
                *max_depth = current_depth;
            }

            let tag_name = extract_tag_name(el, ctx.source);
            let span_start = el.tag_open.start;
            let span_end = el
                .tag_close
                .as_ref()
                .map(|tc| tc.end)
                .unwrap_or(el.tag_open.end);
            let tag_span_end = el.tag_open.end;

            let this_element_index = data.elements.len() as u32;
            extract_element_data(
                el,
                &tag_name,
                depth,
                parent_tag,
                parent_element_index,
                span_start,
                span_end,
                tag_span_end,
                ctx.source,
                data,
            );
            handle_if_chain(el, ctx.source, span_start, span_end, current_if_chain, data);

            if el.tag_type.is_component() {
                extract_component_usage(ctx, el, oxc_data, &tag_name, span_start, span_end, data);
            }

            if el.tag_type.is_slot_outlet() {
                extract_slot_def(el, ctx.source, span_start, span_end, data);
            }

            // Static ref is in v_ref, dynamic :ref is in props as v-bind
            if el.v_ref.is_some()
                || el.prop_flag.has(crate::ast::types::PropFlags::HasRef)
                || el
                    .prop_flag
                    .has(crate::ast::types::PropFlags::HasDynamicBinding)
            {
                extract_template_ref(el, ctx.source, &tag_name, span_start, span_end, data);
            }

            if el
                .prop_flag
                .has(crate::ast::types::PropFlags::HasEventListener)
            {
                extract_event_handlers(el, ctx.source, &tag_name, span_start, span_end, data);
            }

            if let Some(ref v_for_prop) = el.v_for {
                extract_v_for(
                    el, v_for_prop, oxc_data, ctx.source, span_start, span_end, data,
                );
                if let Some(elem) = data.elements.last_mut() {
                    elem.v_for_idx = Some(data.v_for_directives.len() - 1);
                }
            }

            if el.prop_flag.has(crate::ast::types::PropFlags::HasModel) {
                extract_v_model(el, ctx.source, &tag_name, span_start, span_end, data);
                if let Some(elem) = data.elements.last_mut() {
                    elem.v_model_idx = Some(data.v_model_directives.len() - 1);
                }
            }

            extract_binding_occurrences(oxc_data, ctx.bindings, data);

            // Recurse into children
            if let Some(ref content) = el.content {
                let mut child_if_chain: Option<RawIfChain> = None;
                for &child_id in &content.children {
                    walk_node_for_extraction(
                        ctx,
                        child_id,
                        current_depth,
                        Some(&tag_name),
                        Some(this_element_index),
                        data,
                        max_depth,
                        &mut child_if_chain,
                    );
                }
                if let Some(chain) = child_if_chain.take() {
                    if chain.conditions.len() > 1 {
                        data.if_chains.push(chain);
                    }
                }
            }
        }
        AstNodeKind::Interpolation(_interp) => {
            flush_if_chain(current_if_chain, data);

            if let OxcNodeData::Interpolation(ref oxc_expr) = oxc_data {
                if let Some(ref result) = oxc_expr.bindings {
                    for binding in &result.bindings {
                        if !binding.ignore {
                            data.binding_occurrences.push(RawBindingOccurrence {
                                name: binding.name.to_string(),
                                span: Span::new(
                                    binding.pos,
                                    binding.pos + binding.name.len() as u32,
                                ),
                                is_in_bindings_map: ctx.bindings.contains_key(binding.name),
                                usage_kind: 0, // interpolation
                            });
                        }
                    }
                }
            }
        }
        AstNodeKind::Comment(comment) => {
            let content_str =
                &ctx.source[comment.content_start as usize..comment.content_end as usize];
            let trimmed = content_str.trim();
            if let Some(directive) = parse_comment_directive(trimmed, comment.start, comment.end) {
                data.comment_directives.push(directive);
            }
        }
        AstNodeKind::Text(_) => {
            flush_if_chain(current_if_chain, data);
        }
    }
}

fn extract_tag_name(el: &ElementNode, source: &str) -> String {
    let start = el.tag_open.start as usize + 1; // skip '<'
    let end = el.tag_open.name_end as usize;
    source[start..end].to_string()
}

#[allow(clippy::too_many_arguments)]
fn extract_element_data(
    el: &ElementNode,
    tag_name: &str,
    depth: u16,
    parent_tag: Option<&str>,
    parent_element_index: Option<u32>,
    span_start: u32,
    span_end: u32,
    tag_span_end: u32,
    source: &str,
    data: &mut RawTemplateData,
) {
    use crate::ast::types::ElementNodeConditionKind;

    let mut attributes = Vec::new();
    let mut directives = Vec::new();

    // Extract from el.props (non-cached directives and static attributes)
    for prop in &el.props {
        let prop_end = prop_span_end(prop, source);
        if prop.is_directive {
            let raw_name = &source[prop.start as usize..prop.name_end as usize];
            let (name, normalized_raw) = normalize_directive_name(raw_name);
            let argument = prop
                .arg_start
                .zip(prop.arg_end)
                .map(|(s, e)| source[s as usize..e as usize].to_string());
            let modifiers: Vec<String> = prop
                .modifiers
                .iter()
                .map(|m| m.slice(source).to_string())
                .collect();
            let expression = prop
                .value_start
                .zip(prop.value_end)
                .map(|(s, e)| source[s as usize..e as usize].to_string());
            directives.push(RawDirectiveData {
                name,
                raw_name: normalized_raw,
                argument,
                modifiers,
                expression,
                span: Span::new(prop.start, prop_end),
            });
        } else {
            let name = source[prop.start as usize..prop.name_end as usize].to_string();
            let value = prop
                .value_start
                .zip(prop.value_end)
                .map(|(s, e)| source[s as usize..e as usize].to_string());
            attributes.push(RawAttributeData {
                name,
                value,
                is_dynamic: false,
                span: Span::new(prop.start, prop_end),
            });
        }
    }

    // Extract cached directives (not in el.props)
    if let Some(ref cond) = el.v_condition {
        let expression = cond
            .prop
            .value_start
            .zip(cond.prop.value_end)
            .map(|(s, e)| source[s as usize..e as usize].to_string());
        let (name, raw_name) = match cond.kind {
            ElementNodeConditionKind::If => ("if".to_string(), "v-if".to_string()),
            ElementNodeConditionKind::ElseIf => ("else-if".to_string(), "v-else-if".to_string()),
            ElementNodeConditionKind::Else => ("else".to_string(), "v-else".to_string()),
        };
        directives.push(RawDirectiveData {
            name,
            raw_name,
            argument: None,
            modifiers: Vec::new(),
            expression,
            span: Span::new(cond.prop.start, prop_span_end(&cond.prop, source)),
        });
    }

    if let Some(ref v_for) = el.v_for {
        let expression = v_for
            .value_start
            .zip(v_for.value_end)
            .map(|(s, e)| source[s as usize..e as usize].to_string());
        directives.push(RawDirectiveData {
            name: "for".to_string(),
            raw_name: "v-for".to_string(),
            argument: None,
            modifiers: Vec::new(),
            expression,
            span: Span::new(v_for.start, prop_span_end(v_for, source)),
        });
    }

    if let Some(ref v_slot) = el.v_slot {
        let raw_name_str = &source[v_slot.start as usize..v_slot.name_end as usize];
        let argument = v_slot
            .arg_start
            .zip(v_slot.arg_end)
            .map(|(s, e)| source[s as usize..e as usize].to_string());
        let expression = v_slot
            .value_start
            .zip(v_slot.value_end)
            .map(|(s, e)| source[s as usize..e as usize].to_string());
        directives.push(RawDirectiveData {
            name: "slot".to_string(),
            raw_name: raw_name_str.to_string(),
            argument,
            modifiers: Vec::new(),
            expression,
            span: Span::new(v_slot.start, prop_span_end(v_slot, source)),
        });
    }

    if let Some(ref v_once) = el.v_once {
        directives.push(RawDirectiveData {
            name: "once".to_string(),
            raw_name: "v-once".to_string(),
            argument: None,
            modifiers: Vec::new(),
            expression: None,
            span: Span::new(v_once.start, prop_span_end(v_once, source)),
        });
    }

    if let Some(ref v_ref) = el.v_ref {
        // Static ref="x" — treated as attribute
        let value = v_ref
            .value_start
            .zip(v_ref.value_end)
            .map(|(s, e)| source[s as usize..e as usize].to_string());
        attributes.push(RawAttributeData {
            name: "ref".to_string(),
            value,
            is_dynamic: false,
            span: Span::new(v_ref.start, prop_span_end(v_ref, source)),
        });
    }

    data.elements.push(RawElementData {
        tag: tag_name.to_string(),
        is_component: el.tag_type.is_component(),
        is_self_closing: el.is_self_closing,
        has_v_if: el
            .v_condition
            .as_ref()
            .is_some_and(|c| c.kind == ElementNodeConditionKind::If),
        has_v_else: el
            .v_condition
            .as_ref()
            .is_some_and(|c| c.kind == ElementNodeConditionKind::Else),
        has_v_else_if: el
            .v_condition
            .as_ref()
            .is_some_and(|c| c.kind == ElementNodeConditionKind::ElseIf),
        has_v_show: el.prop_flag.has(crate::ast::types::PropFlags::HasShow),
        has_v_html: el.prop_flag.has(crate::ast::types::PropFlags::HasVHtml),
        has_v_text: el.prop_flag.has(crate::ast::types::PropFlags::HasVText),
        has_text_content: el
            .children_flag
            .has(crate::ast::types::ChildrenFlags::HasText)
            || el
                .children_flag
                .has(crate::ast::types::ChildrenFlags::HasInterpolation),
        has_element_children: el
            .children_flag
            .has(crate::ast::types::ChildrenFlags::HasElement),
        nesting_depth: depth,
        parent_tag: parent_tag.map(|s| s.to_string()),
        parent_index: parent_element_index,
        span: Span::new(span_start, span_end),
        tag_span_end,
        attributes,
        directives,
        v_for_idx: None,
        v_model_idx: None,
    });
}

/// Compute the end position of a prop span.
fn prop_span_end(prop: &crate::types::NodeProp, source: &str) -> u32 {
    // value_end is "before the closing quote", so +1 for the quote itself
    if let Some(ve) = prop.value_end {
        // Check if there's a quote character after value_end
        let ve_usize = ve as usize;
        if ve_usize < source.len() {
            let next = source.as_bytes()[ve_usize];
            if next == b'"' || next == b'\'' {
                return ve + 1;
            }
        }
        return ve;
    }
    let mut end = prop.name_end;
    if let Some(ae) = prop.arg_end {
        end = end.max(ae);
    }
    for m in &prop.modifiers {
        end = end.max(m.end);
    }
    end
}

/// Normalize a directive raw name to (canonical_name, raw_name_for_display).
fn normalize_directive_name(raw: &str) -> (String, String) {
    match raw {
        "@" => ("on".to_string(), "@".to_string()),
        ":" => ("bind".to_string(), ":".to_string()),
        "#" => ("slot".to_string(), "#".to_string()),
        _ if raw.starts_with("v-") => {
            let name = raw[2..].to_string();
            (name, raw.to_string())
        }
        _ => (raw.to_string(), raw.to_string()),
    }
}

fn handle_if_chain(
    el: &ElementNode,
    source: &str,
    span_start: u32,
    span_end: u32,
    current_if_chain: &mut Option<RawIfChain>,
    data: &mut RawTemplateData,
) {
    use crate::ast::types::ElementNodeConditionKind;

    match &el.v_condition {
        Some(cond) => match cond.kind {
            ElementNodeConditionKind::If => {
                // Flush previous chain
                flush_if_chain(current_if_chain, data);
                // Start new chain
                let expr = cond
                    .prop
                    .value_start
                    .zip(cond.prop.value_end)
                    .map(|(s, e)| source[s as usize..e as usize].to_string())
                    .unwrap_or_default();
                *current_if_chain = Some(RawIfChain {
                    conditions: vec![(expr, span_start, span_end)],
                });
            }
            ElementNodeConditionKind::ElseIf => {
                let expr = cond
                    .prop
                    .value_start
                    .zip(cond.prop.value_end)
                    .map(|(s, e)| source[s as usize..e as usize].to_string())
                    .unwrap_or_default();
                if let Some(ref mut chain) = current_if_chain {
                    chain.conditions.push((expr, span_start, span_end));
                }
            }
            ElementNodeConditionKind::Else => {
                if let Some(ref mut chain) = current_if_chain {
                    chain
                        .conditions
                        .push(("".to_string(), span_start, span_end));
                }
                // v-else ends the chain
                flush_if_chain(current_if_chain, data);
            }
        },
        None => {
            // Non-conditional elements break the if-chain
            flush_if_chain(current_if_chain, data);
        }
    }
}

fn flush_if_chain(current_if_chain: &mut Option<RawIfChain>, data: &mut RawTemplateData) {
    if let Some(chain) = current_if_chain.take() {
        if chain.conditions.len() > 1 {
            data.if_chains.push(chain);
        }
    }
}

fn extract_component_usage(
    ctx: &ExtractCtx<'_>,
    el: &ElementNode,
    oxc_data: &OxcNodeData<'_>,
    tag_name: &str,
    span_start: u32,
    span_end: u32,
    data: &mut RawTemplateData,
) {
    let source = ctx.source;
    let bindings = ctx.bindings;
    let is_dynamic = tag_name == "component";
    let has_spread = el.has_spread();

    let mut props = Vec::new();
    let mut slots_used = Vec::new();

    let oxc_el = match oxc_data {
        OxcNodeData::Element(ref el) => Some(el.as_ref()),
        _ => None,
    };

    for (i, prop) in el.props.iter().enumerate() {
        let base = &source[prop.start as usize..prop.name_end as usize];
        let arg = prop
            .arg_start
            .zip(prop.arg_end)
            .map(|(s, e)| &source[s as usize..e as usize]);

        if prop.is_directive {
            // Skip structural directives (v-if, v-else, v-else-if, v-for are cached, not in props,
            // but v-slot/# may still be here)
            match base {
                "v-if" | "v-else-if" | "v-else" | "v-for" => continue,
                "v-slot" | "#" => {
                    let slot_name = arg.unwrap_or("default");
                    let slot_name = if slot_name.is_empty() {
                        "default".to_string()
                    } else {
                        slot_name.to_string()
                    };
                    if !slots_used.contains(&slot_name) {
                        slots_used.push(slot_name);
                    }
                    continue;
                }
                "@" | "v-on" => continue, // Event handlers — skip
                ":" | "v-bind" => {
                    // Check for key (skip), ref (skip), spread (skip)
                    match arg {
                        Some("key") => continue,
                        None => continue, // v-bind spread — already captured by has_spread
                        _ => {}           // Regular bound prop — fall through
                    }
                }
                _ => continue, // Other directives (v-model, v-show, etc.) — skip
            }
        } else {
            // Non-directive attribute — skip "key" (static key)
            if base == "key" {
                continue;
            }
        }

        // At this point we have either:
        // - A non-directive attribute (base = prop name)
        // - A v-bind/:  directive with an arg (the prop name is in arg)
        let is_bound = prop.is_directive;
        let actual_name = if is_bound {
            arg.unwrap_or(base).to_string()
        } else {
            base.to_string()
        };

        let expression = prop
            .value_start
            .zip(prop.value_end)
            .map(|(s, e)| source[s as usize..e as usize].to_string());

        // Extract referenced bindings from OXC data
        let mut referenced = Vec::new();
        let mut all_static = if is_bound { Some(true) } else { None };

        if is_bound {
            if let Some(oxc_el) = oxc_el {
                for oxc_prop in &oxc_el.props {
                    if oxc_prop.prop_index == i {
                        if let Some(ref exp) = oxc_prop.exp {
                            if let Some(ref result) = exp.bindings {
                                for b in &result.bindings {
                                    if !b.ignore {
                                        referenced.push(b.name.to_string());
                                        if let Some(ref mut is_static) = all_static {
                                            let bt = bindings.get(b.name);
                                            let binding_is_static = bt.is_some_and(|bt| {
                                                matches!(
                                                    bt.reactivity_level(),
                                                    crate::template::code_gen::binding::ReactivityLevel::Static
                                                )
                                            });
                                            if !binding_is_static {
                                                *is_static = false;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }

        // Compute the full span of this prop attribute.
        // span_start = prop.start (the beginning of the attribute name or directive prefix)
        // span_end = past the closing quote of the value, or name_end if no value
        let prop_span_end = prop
            .value_end
            .map(|ve| ve + 1) // +1 to skip past the closing quote
            .unwrap_or(prop.name_end);

        props.push(RawPropData {
            name: actual_name,
            is_bound,
            expression,
            referenced_bindings: referenced,
            all_bindings_static: all_static,
            from_spread: false,
            span: Span::new(prop.start, prop_span_end),
        });
    }

    // Add a spread prop entry if v-bind spread exists
    if has_spread {
        props.push(RawPropData {
            name: String::new(),
            is_bound: true,
            expression: None,
            referenced_bindings: Vec::new(),
            all_bindings_static: None,
            from_spread: true,
            span: Span::new(0, 0),
        });
    }

    // Extract static classes, dynamic class flag, and dynamic class expression from attributes
    let mut static_classes = Vec::new();
    let mut has_dynamic_class = false;
    let mut dynamic_class_expr: Option<String> = None;
    for prop in el.props.iter() {
        let base = &source[prop.start as usize..prop.name_end as usize];
        let arg = prop
            .arg_start
            .zip(prop.arg_end)
            .map(|(s, e)| &source[s as usize..e as usize]);
        if !prop.is_directive && base == "class" {
            // Static class attribute: class="foo bar"
            if let Some((vs, ve)) = prop.value_start.zip(prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                for cls in value.split_whitespace() {
                    if !cls.is_empty() {
                        static_classes.push(cls.to_string());
                    }
                }
            }
        } else if prop.is_directive {
            // Check for :class or v-bind:class
            let is_class_binding = if base == ":" || base == "v-bind" {
                arg == Some("class")
            } else {
                base == ":class"
            };
            if is_class_binding {
                has_dynamic_class = true;
                // Store the expression value for dynamic class name extraction
                if let Some((vs, ve)) = prop.value_start.zip(prop.value_end) {
                    dynamic_class_expr = Some(source[vs as usize..ve as usize].to_string());
                }
            }
        }
    }

    data.components.push(RawComponentUsage {
        tag_name: tag_name.to_string(),
        is_dynamic,
        props,
        has_spread,
        slots_used,
        static_classes,
        has_dynamic_class,
        dynamic_class_expr,
        span: Span::new(span_start, span_end),
    });
}

fn extract_slot_def(
    el: &ElementNode,
    source: &str,
    span_start: u32,
    span_end: u32,
    data: &mut RawTemplateData,
) {
    // Find the "name" attribute on the <slot> element
    let mut name = "default".to_string();
    let mut has_bindings = false;
    let mut binding_names = Vec::new();

    for prop in &el.props {
        if prop.is_directive {
            let base = &source[prop.start as usize..prop.name_end as usize];
            // Slot bindings: v-bind / : with an arg (scoped slots pass data via v-bind)
            if base == ":" || base == "v-bind" {
                let arg = prop
                    .arg_start
                    .zip(prop.arg_end)
                    .map(|(s, e)| &source[s as usize..e as usize]);
                if let Some(arg_name) = arg {
                    has_bindings = true;
                    binding_names.push(arg_name.to_string());
                }
            }
        } else {
            let attr_name = &source[prop.start as usize..prop.name_end as usize];
            if attr_name == "name" {
                if let Some((s, e)) = prop.value_start.zip(prop.value_end) {
                    name = source[s as usize..e as usize].to_string();
                }
            }
        }
    }

    data.slot_definitions.push(RawSlotDef {
        name,
        has_bindings,
        binding_names,
        span: Span::new(span_start, span_end),
    });
}

fn extract_template_ref(
    el: &ElementNode,
    source: &str,
    tag_name: &str,
    span_start: u32,
    span_end: u32,
    data: &mut RawTemplateData,
) {
    // Static ref="foo" is cached in el.v_ref (taken out of props by the parser).
    if let Some(ref v_ref) = el.v_ref {
        let name = v_ref
            .value_start
            .zip(v_ref.value_end)
            .map(|(s, e)| source[s as usize..e as usize].to_string())
            .unwrap_or_default();
        data.template_refs.push(RawTemplateRef {
            name,
            is_dynamic: false,
            target_tag: tag_name.to_string(),
            span: Span::new(span_start, span_end),
        });
        return;
    }

    // Dynamic :ref="expr" or v-bind:ref="expr" — stored as a directive prop
    // where the base name is ":" or "v-bind" and the arg is "ref".
    for prop in &el.props {
        if !prop.is_directive {
            continue;
        }
        let base = &source[prop.start as usize..prop.name_end as usize];
        if base != ":" && base != "v-bind" {
            continue;
        }
        if let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) {
            let arg = &source[arg_s as usize..arg_e as usize];
            if arg == "ref" {
                let name = prop
                    .value_start
                    .zip(prop.value_end)
                    .map(|(s, e)| source[s as usize..e as usize].to_string())
                    .unwrap_or_default();
                data.template_refs.push(RawTemplateRef {
                    name,
                    is_dynamic: true,
                    target_tag: tag_name.to_string(),
                    span: Span::new(span_start, span_end),
                });
                return;
            }
        }
    }
}

fn extract_event_handlers(
    el: &ElementNode,
    source: &str,
    tag_name: &str,
    span_start: u32,
    span_end: u32,
    data: &mut RawTemplateData,
) {
    for prop in &el.props {
        if !prop.is_directive {
            continue;
        }
        let base = &source[prop.start as usize..prop.name_end as usize];
        // Event handlers are directives with base "@" or "v-on" and an arg.
        if base != "@" && base != "v-on" {
            continue;
        }
        // The event name is in arg_start..arg_end (e.g., "click" for @click).
        let event_name = if let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) {
            &source[arg_s as usize..arg_e as usize]
        } else {
            continue; // v-on without arg is a spread, not an event handler
        };

        let handler_expr = prop
            .value_start
            .zip(prop.value_end)
            .map(|(s, e)| source[s as usize..e as usize].to_string());

        let is_inline = handler_expr.as_ref().is_some_and(|e| !is_simple_handler(e));

        data.event_handlers.push(RawEventHandler {
            event_name: event_name.to_string(),
            handler_expression: handler_expr,
            is_inline,
            target_tag: tag_name.to_string(),
            span: Span::new(span_start, span_end),
        });
    }
}

/// Check if a handler expression is a simple reference (not inline).
fn is_simple_handler(expr: &str) -> bool {
    let trimmed = expr.trim();
    // Simple: single identifier, or member expression (foo.bar, foo?.bar)
    trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'$' || b == b'?')
        && !trimmed.is_empty()
}

fn extract_v_for(
    el: &ElementNode,
    v_for_prop: &crate::types::NodeProp,
    oxc_data: &OxcNodeData<'_>,
    source: &str,
    span_start: u32,
    span_end: u32,
    data: &mut RawTemplateData,
) {
    let oxc_el = match oxc_data {
        OxcNodeData::Element(ref el) => Some(el.as_ref()),
        _ => None,
    };

    // Parse the v-for expression
    let mut variable = String::new();
    let mut index = None;
    let mut iterable = String::new();

    if let Some(oxc_el) = oxc_el {
        if let Some(ref vfor) = oxc_el.v_for {
            // Extract locals (variable, index)
            for (i, local) in vfor.parsed.locals.iter().enumerate() {
                let local_str = local.slice(source);
                if i == 0 {
                    variable = local_str.to_string();
                } else if i == 1 {
                    index = Some(local_str.to_string());
                }
            }
            // First reference is typically the iterable
            if let Some(reference) = vfor.parsed.references.first() {
                iterable = reference.slice(source).to_string();
            }
            // Fallback: literal iterables (arrays, objects, `as const`) have no
            // identifier references. Extract the full iterable text from source.
            if iterable.is_empty() {
                if let Some(ve) = v_for_prop.value_end {
                    let right_start = vfor.parsed.result.right_offset as usize;
                    let right_end = ve as usize;
                    if right_start < right_end && right_end <= source.len() {
                        iterable = source[right_start..right_end].trim().to_string();
                    }
                }
            }
        }
    }

    // Fallback: parse from raw value
    if variable.is_empty() {
        if let Some((s, e)) = v_for_prop.value_start.zip(v_for_prop.value_end) {
            let raw = &source[s as usize..e as usize];
            // Simple parse: "item in items" or "(item, index) in items"
            if let Some(in_pos) = raw.find(" in ").or_else(|| raw.find(" of ")) {
                let left = raw[..in_pos].trim();
                iterable = raw[in_pos + 4..].trim().to_string();
                let left = left.trim_start_matches('(').trim_end_matches(')');
                let mut parts = left.split(',');
                if let Some(v) = parts.next() {
                    variable = v.trim().to_string();
                }
                if let Some(i) = parts.next() {
                    index = Some(i.trim().to_string());
                }
            }
        }
    }

    // Check for :key
    let has_key = el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasDynamicKey);
    let mut key_expression = None;
    let mut key_uses_index = false;

    if has_key {
        for prop in &el.props {
            if !prop.is_directive {
                continue;
            }
            let base = &source[prop.start as usize..prop.name_end as usize];
            if base != ":" && base != "v-bind" {
                continue;
            }
            let arg = prop
                .arg_start
                .zip(prop.arg_end)
                .map(|(s, e)| &source[s as usize..e as usize]);
            if arg == Some("key") {
                if let Some((s, e)) = prop.value_start.zip(prop.value_end) {
                    let kexpr = source[s as usize..e as usize].to_string();
                    if let Some(ref idx) = index {
                        key_uses_index = kexpr.contains(idx.as_str());
                    }
                    key_expression = Some(kexpr);
                }
                break;
            }
        }
    }

    data.v_for_directives.push(RawVForData {
        variable,
        index,
        iterable,
        has_key,
        key_expression,
        key_uses_index,
        span: Span::new(span_start, span_end),
    });
}

fn extract_v_model(
    el: &ElementNode,
    source: &str,
    tag_name: &str,
    span_start: u32,
    span_end: u32,
    data: &mut RawTemplateData,
) {
    for prop in &el.props {
        if !prop.is_directive {
            continue;
        }
        let base = &source[prop.start as usize..prop.name_end as usize];
        if base != "v-model" {
            continue;
        }
        // Custom model name is in the arg (e.g., v-model:title → arg="title")
        let binding_name = prop
            .arg_start
            .zip(prop.arg_end)
            .map(|(s, e)| source[s as usize..e as usize].to_string())
            .unwrap_or_else(|| "modelValue".to_string());

        let modifiers: Vec<String> = prop
            .modifiers
            .iter()
            .map(|m| m.slice(source).to_string())
            .collect();

        data.v_model_directives.push(RawVModelData {
            binding_name,
            modifiers,
            target_is_component: el.tag_type.is_component(),
            target_tag: tag_name.to_string(),
            span: Span::new(span_start, span_end),
        });
        break; // Only one v-model per element
    }
}

fn extract_binding_occurrences(
    oxc_data: &OxcNodeData<'_>,
    bindings: &FxHashMap<&str, BindingType>,
    data: &mut RawTemplateData,
) {
    let oxc_el = match oxc_data {
        OxcNodeData::Element(ref el) => el.as_ref(),
        _ => return,
    };

    // Extract from directive value expressions
    for oxc_prop in &oxc_el.props {
        if let Some(ref exp) = oxc_prop.exp {
            if let Some(ref result) = exp.bindings {
                for b in &result.bindings {
                    if !b.ignore {
                        data.binding_occurrences.push(RawBindingOccurrence {
                            name: b.name.to_string(),
                            span: Span::new(b.pos, b.pos + b.name.len() as u32),
                            is_in_bindings_map: bindings.contains_key(b.name),
                            usage_kind: 1, // directive value
                        });
                    }
                }
            }
        }
    }

    // Extract from v-if condition
    if let Some(ref cond) = oxc_el.condition {
        if let Some(ref result) = cond.bindings {
            for b in &result.bindings {
                if !b.ignore {
                    data.binding_occurrences.push(RawBindingOccurrence {
                        name: b.name.to_string(),
                        span: Span::new(b.pos, b.pos + b.name.len() as u32),
                        is_in_bindings_map: bindings.contains_key(b.name),
                        usage_kind: 1, // directive value
                    });
                }
            }
        }
    }
}

fn parse_comment_directive(content: &str, start: u32, end: u32) -> Option<RawCommentDirective> {
    let trimmed = content.trim();

    let (prefix, rest) = if let Some(r) = trimmed.strip_prefix("@verter:") {
        ("@verter:", r)
    } else {
        return None;
    };
    let _ = prefix;

    let rest = rest.trim();
    let (kind, affects_next_line, rule_or_message) =
        if let Some(r) = rest.strip_prefix("disable-next-line") {
            (1, true, Some(r.trim().to_string()))
        } else if let Some(r) = rest.strip_prefix("disable") {
            (0, false, Some(r.trim().to_string()))
        } else if let Some(r) = rest.strip_prefix("enable") {
            (2, false, Some(r.trim().to_string()))
        } else if let Some(r) = rest.strip_prefix("todo") {
            (3, false, Some(r.trim().to_string()))
        } else if let Some(r) = rest.strip_prefix("fixme") {
            (4, false, Some(r.trim().to_string()))
        } else if let Some(r) = rest.strip_prefix("deprecated") {
            (5, false, Some(r.trim().to_string()))
        } else if rest.starts_with("ignore-start") {
            (6, false, None)
        } else if rest.starts_with("ignore-end") {
            (7, false, None)
        } else if let Some(r) = rest.strip_prefix("level") {
            // @verter:level(warn) or @verter:level(error) or @verter:level(off)
            // Extract the argument inside parens, e.g. "level(warn)" → "warn"
            let arg = r.trim();
            let arg = arg.strip_prefix('(').and_then(|s| s.strip_suffix(')'));
            let msg = arg.map(str::trim).map(|s| s.to_string());
            (8, false, msg)
        } else {
            return None;
        };

    let rule_or_message = rule_or_message.and_then(|s| {
        let s = s.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    });

    Some(RawCommentDirective {
        kind,
        rule_or_message,
        span: Span::new(start, end),
        affects_next_line,
    })
}

#[cfg(test)]
#[path = "template_data_tests.rs"]
mod template_data_tests;
