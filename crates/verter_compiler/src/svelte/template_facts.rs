//! Svelte template usage-fact extraction.
//!
//! Walks the typed [`ParsedSvelte`](super::parser::ParsedSvelte) template tree
//! and records each child-component USAGE as a framework-neutral
//! [`RawComponentUsage`](crate::compile::RawComponentUsage): the props, the
//! `bind:` bindings, the events (the legacy `on:` directive ONLY), the spread
//! flag, and the passed `{#snippet}` names (in `slots_used`).
//!
//! Props/events discrimination is SYNTACTIC at the usage site. A plain
//! attribute — including any `on`-prefixed one (`onclick`, `oninput`, but also
//! `online`, `once`, `onboarding`) — is a PROP. In Svelte 5 a callback handler
//! IS a prop; whether a passed prop is the child's callback event is decided by
//! the CHILD component's component-meta, which this structural walk does not
//! resolve. Only the legacy `on:` DIRECTIVE (`on:click`) is unambiguously an
//! event at the usage site, so it is the SOLE source of an `events` fact. A
//! plain `on*` attribute is never removed from `props` or fabricated as an
//! event based on its name alone.
//!
//! Typed-IR only: the walk classifies elements BY KIND off the typed AST and
//! recurses the typed children/clauses; expression TEXT is span-sliced from the
//! carrier source. There is NO structural source scan — the typed tree is the
//! authority for what is a component, a prop, a binding, an event.

use crate::compile::{
    RawComponentBindingUsage, RawComponentEventUsage, RawComponentUsage, RawPropData,
    RawTemplateData,
};

use super::parser::template_ast::{
    SvelteAttributeKind, SvelteAttributeValue, SvelteBlock, SvelteBlockKind, SvelteDirectiveKind,
    SvelteElement, SvelteElementKind, SvelteNode, SvelteSpecialKind,
};
use verter_span::Span;

/// Collect every child-component usage from a template node run into `data`.
///
/// Recurses element children, block children, and each block clause's children
/// (the same walk shape as `svelte_exec::collect_slot_elements`). `source` is
/// the carrier component source; expression spans slice it directly.
pub fn collect_component_usages(nodes: &[SvelteNode], source: &str, data: &mut RawTemplateData) {
    for node in nodes {
        match node {
            SvelteNode::Element(element) => {
                if let Some(usage) = component_usage_for(element, source) {
                    data.components.push(usage);
                }
                // Recurse into the element's children regardless of kind — a
                // component may itself contain nested component usages.
                collect_component_usages(&element.children, source, data);
            }
            SvelteNode::Block(block) => {
                collect_component_usages(&block.children, source, data);
                for clause in &block.clauses {
                    collect_component_usages(&clause.children, source, data);
                }
            }
            _ => {}
        }
    }
}

/// Collect every `{#snippet name(params)}` declaration from a template node
/// run into `data` (D5 — powers `{@render |}` callee completion with the
/// component's in-scope snippet names). Recurses element children, block
/// children, and each block clause's children, mirroring
/// [`collect_component_usages`]; expression text is span-sliced from source.
pub fn collect_snippet_definitions(nodes: &[SvelteNode], source: &str, data: &mut RawTemplateData) {
    for node in nodes {
        match node {
            SvelteNode::Element(element) => {
                collect_snippet_definitions(&element.children, source, data);
            }
            SvelteNode::Block(block) => {
                if let SvelteBlockKind::Snippet {
                    name,
                    name_text,
                    params,
                } = &block.kind
                {
                    data.snippet_definitions
                        .push(crate::compile::RawSnippetDef {
                            name: name_text.clone(),
                            name_span: *name,
                            params_text: params.map(|span| {
                                source
                                    .get(span.start as usize..span.end as usize)
                                    .unwrap_or("")
                                    .to_string()
                            }),
                        });
                }
                collect_snippet_definitions(&block.children, source, data);
                for clause in &block.clauses {
                    collect_snippet_definitions(&clause.children, source, data);
                }
            }
            _ => {}
        }
    }
}

/// Whether this element is a child-component usage, and if so, its kind.
enum UsageClass {
    /// A static component (`<Button>`), `is_dynamic = false`.
    Static,
    /// A dynamic component (`<svelte:component this={X}>`), `is_dynamic = true`;
    /// the `this` attribute is the selector and is NOT a prop.
    Dynamic,
    /// A recursive self reference (`<svelte:self>`).
    SelfRef,
}

/// Classify a template element as a component usage, or `None` for an intrinsic
/// element / a non-component special element (`<svelte:head>`, `<svelte:window>`,
/// …) / a nested style.
fn classify(element: &SvelteElement) -> Option<UsageClass> {
    match &element.kind {
        SvelteElementKind::Component => Some(UsageClass::Static),
        SvelteElementKind::Special(SvelteSpecialKind::Component) => Some(UsageClass::Dynamic),
        SvelteElementKind::Special(SvelteSpecialKind::SelfRef) => Some(UsageClass::SelfRef),
        _ => None,
    }
}

/// Build the neutral [`RawComponentUsage`] for a component element, or `None`
/// when the element is not a component usage.
fn component_usage_for(element: &SvelteElement, source: &str) -> Option<RawComponentUsage> {
    let class = classify(element)?;
    let is_dynamic = matches!(class, UsageClass::Dynamic);

    let mut props: Vec<RawPropData> = Vec::new();
    let mut bindings: Vec<RawComponentBindingUsage> = Vec::new();
    let mut events: Vec<RawComponentEventUsage> = Vec::new();
    let mut has_spread = false;

    for attr in &element.attributes {
        match &attr.kind {
            SvelteAttributeKind::Plain {
                name,
                value,
                name_span,
            } => {
                // A dynamic component's `this` attribute is the component
                // selector, never a prop.
                if is_dynamic && name == "this" {
                    continue;
                }
                // Every plain attribute is a PROP — including any `on`-prefixed
                // one. The props/events split is syntactic: only the legacy
                // `on:` DIRECTIVE (handled below) is an event at the usage site.
                // A plain `on*` attribute (`onclick`, but also `online`,
                // `once`) stays a prop; the child component-meta — not a name
                // guess — decides which passed props are callback events.
                props.push(plain_prop(
                    name,
                    *name_span,
                    value.as_ref(),
                    attr.span,
                    source,
                ));
            }
            SvelteAttributeKind::Spread(_) => {
                has_spread = true;
            }
            // An `{@attach expr}` on a component usage is the element-attachment
            // machinery (officially a computed-key `[$.attachment()]` prop) — it has
            // no named prop / binding / event surface at the usage site.
            SvelteAttributeKind::Attach { .. } => {}
            SvelteAttributeKind::Directive(directive) => match directive.kind {
                SvelteDirectiveKind::On => {
                    // Legacy `on:click` → event named by the directive local.
                    events.push(RawComponentEventUsage {
                        name: directive.local.clone(),
                        handler_expression: directive
                            .value
                            .as_ref()
                            .and_then(|v| expression_text(v, source)),
                        is_inline: directive
                            .value
                            .as_ref()
                            .map(|v| is_inline_handler(v, source))
                            .unwrap_or(false),
                        modifiers: directive.modifiers.clone(),
                        span: attr.span,
                    });
                }
                SvelteDirectiveKind::Bind => {
                    // `bind:this` is a ref, NOT a model binding — skip it.
                    if directive.local == "this" {
                        continue;
                    }
                    bindings.push(RawComponentBindingUsage {
                        name: directive.local.clone(),
                        modifiers: directive.modifiers.clone(),
                        span: attr.span,
                    });
                }
                // `let:` is a slot-prop binding, not a prop; class/style/use/
                // transition/in/out/animate are presentational directives — none
                // are component props/bindings/events.
                SvelteDirectiveKind::Let
                | SvelteDirectiveKind::Class
                | SvelteDirectiveKind::Style
                | SvelteDirectiveKind::Use
                | SvelteDirectiveKind::Transition
                | SvelteDirectiveKind::In
                | SvelteDirectiveKind::Out
                | SvelteDirectiveKind::Animate
                | SvelteDirectiveKind::Unknown => {}
            },
        }
    }

    // Snippets passed as direct children (`{#snippet name()}…{/snippet}`) are
    // recorded by name in `slots_used`.
    let mut slots_used: Vec<String> = Vec::new();
    for child in &element.children {
        if let SvelteNode::Block(SvelteBlock {
            kind: SvelteBlockKind::Snippet { name_text, .. },
            ..
        }) = child
        {
            if !slots_used.contains(name_text) {
                slots_used.push(name_text.clone());
            }
        }
    }

    Some(RawComponentUsage {
        tag_name: element.name.clone(),
        is_dynamic,
        props,
        has_spread,
        slots_used,
        static_classes: Vec::new(),
        has_dynamic_class: false,
        dynamic_class_expr: None,
        bindings,
        events,
        span: element.open_span,
    })
}

/// Build a plain prop. A `Text` value is a static prop; an `Expression` /
/// `Mixed` value (or a shorthand `{value}` recorded as `Expression`) is a bound
/// prop. Every plain attribute — including any `on`-prefixed one — flows here;
/// the props/events split is syntactic (only the legacy `on:` directive is an
/// event).
fn plain_prop(
    name: &str,
    name_span: Span,
    value: Option<&SvelteAttributeValue>,
    attr_span: Span,
    source: &str,
) -> RawPropData {
    let is_bound = matches!(
        value,
        Some(SvelteAttributeValue::Expression(_)) | Some(SvelteAttributeValue::Mixed(_))
    );
    RawPropData {
        name: name.to_string(),
        is_bound,
        expression: value.and_then(|v| expression_text(v, source)),
        referenced_bindings: Vec::new(),
        all_bindings_static: None,
        from_spread: false,
        span: attr_span,
        name_span,
        is_same_name_shorthand: false,
    }
}

/// The expression/value text of an attribute value, span-sliced from the
/// carrier source. `None` when the span is out of range.
fn expression_text(value: &SvelteAttributeValue, source: &str) -> Option<String> {
    let span = match value {
        SvelteAttributeValue::Text(span)
        | SvelteAttributeValue::Expression(span)
        | SvelteAttributeValue::Mixed(span) => *span,
    };
    let (s, e) = (span.start as usize, span.end as usize);
    source.get(s..e).map(|slice| slice.trim().to_string())
}

/// Whether a handler value is an inline function expression (arrow / `function`)
/// rather than a bare identifier reference. A `Text`-valued handler is never
/// inline.
fn is_inline_handler(value: &SvelteAttributeValue, source: &str) -> bool {
    match value {
        SvelteAttributeValue::Expression(span) | SvelteAttributeValue::Mixed(span) => {
            let (s, e) = (span.start as usize, span.end as usize);
            let Some(text) = source.get(s..e) else {
                return false;
            };
            let text = text.trim();
            text.contains("=>") || text.starts_with("function")
        }
        SvelteAttributeValue::Text(_) => false,
    }
}
