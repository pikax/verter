//! Per-component slot summary for the IDE strict-slot lane.
//!
//! The IDE TSX codegen path emits two slot-checking constructs per component:
//! `strictRenderSlot` (one call per non-empty slot group) and
//! `checkRequiredSlots` (one call listing the provided slot names). Both answer
//! the SAME structural question — "what slots does this component's children
//! provide?" — so the answer is computed ONCE per component, from a single scan
//! of its direct children, and cached on the shared [`OxcParsedAst`] overlay
//! keyed by [`NodeId`]. Each IDE collector then reads the one cached
//! [`ComponentSlotSummary`] instead of independently re-scanning.
//!
//! The summary is built lazily, on the first strict-slot demand for a given
//! component (see [`OxcParsedAst::slot_summary`]). The walk visits each
//! component exactly once, so only components the IDE lane actually reaches are
//! scanned — non-component nodes are never touched, and the runtime VDOM/Vapor
//! and SSR lanes demand no summaries at all.
//!
//! The two emitted constructs follow subtly different rules, so the single scan
//! records both `groups` (strict-slot, non-empty groups only) and
//! `provided_slot_names` (required-slot, broader: counts empty named templates
//! and adds `"default"` for any non-template content including whitespace).
//!
//! [`OxcParsedAst`]: super::types::OxcParsedAst
//! [`OxcParsedAst::slot_summary`]: super::types::OxcParsedAst::slot_summary

use crate::ast::types::{AstNodeKind, ElementNode, TagType, TemplateAst};
use crate::types::NodeId;

use super::types::{ComponentSlotSummary, SlotChildFact, SlotChildKind, SlotGroup};

/// Build the slot summary for the node at `id`, or `None` when the node is not a
/// slot-checkable component.
///
/// Returns `Some` for a static component eligible for slot checking (its summary
/// built from a single scan of its direct children) and `None` for any other
/// node — a non-element, a native/`<template>`/`<slot>` element, or a dynamic
/// `<component :is>` whose concrete type is unknown. Called once per component
/// via the overlay's lazily-memoized [`slot_summary`] lookup.
///
/// [`slot_summary`]: super::types::OxcParsedAst::slot_summary
pub(crate) fn build_slot_summary(
    id: NodeId,
    ast: &TemplateAst,
    source: &str,
) -> Option<ComponentSlotSummary> {
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        return None;
    };
    if el.tag_type != TagType::Component {
        return None;
    }
    // Skip dynamic `<component :is>` — its concrete type is unknown, so the IDE
    // path emits no slot checks for it.
    if tag_name(el, source) == "component" {
        return None;
    }

    #[cfg(test)]
    record_build();

    Some(build_component_summary(ast, source, el))
}

/// Build one component's slot summary from a single scan of its direct children.
fn build_component_summary(
    ast: &TemplateAst,
    source: &str,
    el: &ElementNode,
) -> ComponentSlotSummary {
    let mut groups: Vec<SlotGroup> = Vec::new();
    let mut provided_slot_names: Vec<String> = Vec::new();
    // Tracks whether any non-template content (element, text — including
    // whitespace — or interpolation) is a direct child; such content provides
    // the `"default"` slot for `checkRequiredSlots`.
    let mut has_default_content = false;

    if let Some(content) = &el.content {
        for &child_id in &content.children {
            match &ast.nodes[child_id.0].kind {
                AstNodeKind::Element(child_el) => {
                    if is_named_slot_template(child_el) {
                        let name = template_slot_name(child_el, source);
                        // strict-slot: a named template contributes its inner
                        // classified children, but only when non-empty.
                        let inner = classify_direct_children(ast, source, child_el);
                        if !inner.is_empty() {
                            push_group(&mut groups, &name, inner);
                        }
                        // required-slot: the name counts even when the template
                        // body is empty.
                        if !provided_slot_names.contains(&name) {
                            provided_slot_names.push(name);
                        }
                    } else {
                        // strict-slot: a direct non-template element joins the
                        // default group (transparent `<template>`/`<slot>`
                        // wrappers classify to nothing).
                        if let Some(child) = classify_element(child_id, child_el) {
                            push_group(&mut groups, "default", vec![child]);
                        }
                        // required-slot: any non-template element is default
                        // content, even one that classifies to nothing.
                        has_default_content = true;
                    }
                }
                AstNodeKind::Text(text) => {
                    // strict-slot: only non-whitespace text joins the default
                    // group.
                    if !source[text.start as usize..text.end as usize]
                        .trim()
                        .is_empty()
                    {
                        push_group(
                            &mut groups,
                            "default",
                            vec![SlotChildFact {
                                node_id: child_id,
                                kind: SlotChildKind::Text,
                            }],
                        );
                    }
                    // required-slot: ANY text (including whitespace) is default
                    // content.
                    has_default_content = true;
                }
                AstNodeKind::Interpolation(_) => {
                    push_group(
                        &mut groups,
                        "default",
                        vec![SlotChildFact {
                            node_id: child_id,
                            kind: SlotChildKind::Interpolation,
                        }],
                    );
                    has_default_content = true;
                }
                AstNodeKind::Comment(_) => {}
            }
        }
    }

    if has_default_content && !provided_slot_names.iter().any(|n| n == "default") {
        provided_slot_names.push("default".to_string());
    }

    ComponentSlotSummary {
        groups,
        provided_slot_names,
    }
}

/// Classify the direct children of a `<template>` wrapper for strict-slot use.
///
/// Mirrors the per-child rules of [`build_component_summary`]'s default-content
/// branch: elements classify by tag kind, non-whitespace text and
/// interpolations contribute, comments are ignored. Nested `<template>` /
/// `<slot>` children classify to nothing.
fn classify_direct_children(
    ast: &TemplateAst,
    source: &str,
    el: &ElementNode,
) -> Vec<SlotChildFact> {
    let mut out = Vec::new();
    let Some(content) = &el.content else {
        return out;
    };
    for &child_id in &content.children {
        match &ast.nodes[child_id.0].kind {
            AstNodeKind::Element(child_el) => {
                if let Some(child) = classify_element(child_id, child_el) {
                    out.push(child);
                }
            }
            AstNodeKind::Text(text) => {
                if !source[text.start as usize..text.end as usize]
                    .trim()
                    .is_empty()
                {
                    out.push(SlotChildFact {
                        node_id: child_id,
                        kind: SlotChildKind::Text,
                    });
                }
            }
            AstNodeKind::Interpolation(_) => {
                out.push(SlotChildFact {
                    node_id: child_id,
                    kind: SlotChildKind::Interpolation,
                });
            }
            AstNodeKind::Comment(_) => {}
        }
    }
    out
}

/// Classify one element child, or `None` for transparent wrappers.
///
/// `child_id` is the element's own [`NodeId`], threaded in by the caller so the
/// recorded fact resolves back to the exact AST node when the IDE adapter emits.
fn classify_element(child_id: NodeId, el: &ElementNode) -> Option<SlotChildFact> {
    let kind = match el.tag_type {
        TagType::Component => SlotChildKind::Component,
        TagType::Element => SlotChildKind::HtmlElement,
        // `<template>` wrappers without v-slot and `<slot>` outlets are
        // transparent for strict-slot child typing.
        TagType::Template | TagType::SlotOutlet => return None,
    };
    Some(SlotChildFact {
        node_id: child_id,
        kind,
    })
}

/// Push `children` into the named group, merging into an existing same-name
/// group (preserving first-seen group order) or appending a new one.
fn push_group(groups: &mut Vec<SlotGroup>, name: &str, children: Vec<SlotChildFact>) {
    if let Some(group) = groups.iter_mut().find(|g| g.name == name) {
        group.children.extend(children);
    } else {
        groups.push(SlotGroup {
            name: name.to_string(),
            children,
        });
    }
}

/// Slot name of a `<template #name>` / `<template v-slot:name>` element, or
/// `"default"` when the directive carries no argument.
fn template_slot_name(el: &ElementNode, source: &str) -> String {
    if let Some(v_slot) = &el.v_slot {
        if let (Some(arg_start), Some(arg_end)) = (v_slot.arg_start, v_slot.arg_end) {
            return source[arg_start as usize..arg_end as usize].to_string();
        }
    }
    "default".to_string()
}

/// Whether `el` is a `<template>` carrying a `v-slot` (a named slot template).
#[inline]
fn is_named_slot_template(el: &ElementNode) -> bool {
    el.tag_type == TagType::Template && el.v_slot.is_some()
}

/// The element's tag name slice (`source[start+1..name_end]`).
#[inline]
fn tag_name<'s>(el: &ElementNode, source: &'s str) -> &'s str {
    &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize]
}

// ── Test-only instrumentation ───────────────────────────────────────────────
//
// Counts how many times a component slot summary is BUILT (the lazy per-component
// scan) and READ (each IDE collector consumption). The build-once invariant is
// that a summary consumed by both collectors is built exactly once: builds ==
// component count, reads == 2 × component count. Compiled out of production builds.

#[cfg(test)]
thread_local! {
    static SLOT_SUMMARY_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SLOT_SUMMARY_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn record_build() {
    SLOT_SUMMARY_BUILDS.with(|c| c.set(c.get() + 1));
}

/// Record one IDE-collector consumption of a memoized summary.
#[cfg(test)]
pub(crate) fn record_slot_summary_read() {
    SLOT_SUMMARY_READS.with(|c| c.set(c.get() + 1));
}

/// Reset both per-thread slot-summary counters.
#[cfg(test)]
pub(crate) fn reset_slot_summary_counts() {
    SLOT_SUMMARY_BUILDS.with(|c| c.set(0));
    SLOT_SUMMARY_READS.with(|c| c.set(0));
}

/// Number of component summaries built on this thread since the last reset.
#[cfg(test)]
pub(crate) fn slot_summary_build_count() -> usize {
    SLOT_SUMMARY_BUILDS.with(|c| c.get())
}

/// Number of IDE-collector summary reads on this thread since the last reset.
#[cfg(test)]
pub(crate) fn slot_summary_read_count() -> usize {
    SLOT_SUMMARY_READS.with(|c| c.get())
}
