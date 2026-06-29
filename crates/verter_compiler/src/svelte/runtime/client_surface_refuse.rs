//! The fail-closed REASON / diagnostic-label builders for the default-deny client
//! syntax classifier, extracted from `client_surface.rs` to keep it under the
//! file-size guard.
//!
//! These are the small typed-IR-driven helpers the node walk (`classify_node`) calls
//! to mint a precise [`UnsupportedSvelteRuntimeSurface`] — or a short diagnostic
//! label — the instant a surface is refused: [`refuse_block`] (the per-block-kind
//! refusal reason: `{#if}` / `{#each}` / `{#await}` / `{#key}` route to `Block`, a
//! `{#snippet}` to `ComponentOrSnippet`), [`refuse_tag`] (the per-standalone-tag
//! refusal reason), [`refuse_unsupported_special_content`] (the special-content host
//! gate for `<textarea>` / `<select>` / `<option>` interiors), and the
//! [`namespace_label`] / [`special_label`] diagnostic-label formatters. Each is driven
//! from the typed `IrNode` / `AttrIr` inventory plus the typed `SupportedHtmlElement`,
//! never a raw-source scan.

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_allowlist::SupportedHtmlElement;
use super::ir::{AttrIr, BlockIr, DeclKind, IrNode, SpecialKind, SvelteRuntimeIr, TagIr};
use super::whitespace::Namespace;
use verter_span::Span;

/// The fail-closed reason for a block construct (5e), or 5f for a snippet.
pub(super) fn refuse_block(block: &BlockIr) -> UnsupportedSvelteRuntimeSurface {
    let construct = match block {
        BlockIr::If { .. } => "if",
        BlockIr::Each { .. } => "each",
        BlockIr::Await { .. } => "await",
        BlockIr::Key { .. } => "key",
        BlockIr::Snippet { .. } => "snippet",
    };
    if matches!(block, BlockIr::Snippet { .. }) {
        UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
            construct,
            span: Span::new(0, 0),
        }
    } else {
        UnsupportedSvelteRuntimeSurface::Block {
            construct,
            span: Span::new(0, 0),
        }
    }
}

/// The fail-closed reason for a standalone tag. A `{@html}` tag is the `$.html` surface
/// and is accepted by the caller before this is reached, so it is not matched here.
pub(super) fn refuse_tag(tag: &TagIr) -> UnsupportedSvelteRuntimeSurface {
    match tag {
        TagIr::Html { .. } => UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
            // Unreachable: `{@html}` is accepted by `classify_node` before `refuse_tag`
            // runs. The arm is retained so the match stays exhaustive over `TagIr`.
            construct: "html",
            span: Span::new(0, 0),
        },
        TagIr::Render { .. } => UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
            construct: "render",
            span: Span::new(0, 0),
        },
        TagIr::Attach { .. } => UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
            construct: "attach",
            span: Span::new(0, 0),
        },
        TagIr::LegacyConst { .. } => UnsupportedSvelteRuntimeSurface::Block {
            construct: "const",
            span: Span::new(0, 0),
        },
        TagIr::Declaration { kind, .. } => UnsupportedSvelteRuntimeSurface::Block {
            construct: match kind {
                DeclKind::Const => "const",
                DeclKind::Let => "let",
            },
            span: Span::new(0, 0),
        },
        TagIr::Debug { .. } => UnsupportedSvelteRuntimeSurface::Block {
            construct: "debug",
            span: Span::new(0, 0),
        },
    }
}

/// Refuse a bindings-breadth special-content host (`<textarea>` / `<select>` /
/// `<option>`) whose INTERIOR content is not the supported `bind:value` host shape.
///
/// 5c emits these elements ONLY as the DOM-bind hosts the pinned `svelte@5.56.3`
/// oracle proves: a `<textarea bind:value>` is cleared EMPTY (`$.remove_textarea_child`
/// strips its content), and a `<select bind:value>` carries STATIC `<option>`
/// children (`<select><option>a</option></select>`). The official compiler gives
/// these elements a SPECIAL content model 5c does not own — a `<textarea>` with
/// text/interpolation content is the raw-text-value surface, and an `<option>` with
/// an INTERPOLATION child is the `option.__value` / `option_value` reactive-tracking
/// surface. Those forms are refused HERE (before the per-attr / child walk) so a
/// divergent module is never emitted.
///
/// The decision is STRUCTURAL over the typed IR children (the node kinds), never a
/// raw-source scan. A non-special element (`element` not in the special set) returns
/// `Ok(())` immediately.
pub(super) fn refuse_unsupported_special_content(
    ir: &SvelteRuntimeIr,
    element: SupportedHtmlElement,
    el: &super::ir::ElementIr,
    el_span: Span,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    let refuse = || {
        Err(UnsupportedSvelteRuntimeSurface::Element {
            tag: el.tag.clone(),
            span: el_span,
        })
    };
    match element {
        // `<textarea>`: the supported shape is the `bind:value` host. With EMPTY content
        // it clears nothing; with a STATIC-TEXT fallback child AND a `bind:value` the
        // existing `$.remove_textarea_child` prelude strips the baked child at runtime, so
        // the static-text fallback is supported (the official `<textarea
        // bind:value>fallback</textarea>` bakes the text into the cloned skeleton, then
        // clears it — the bind is unaffected). A DYNAMIC / interpolation child is the
        // official `$.set_value(textarea, expr)` content channel 5c does NOT own (a
        // distinct surface owned by a later content-model layer — ledger D-22), so it
        // still fails closed. A static-text child WITHOUT a `bind:value` is also out of
        // the supported empty/bind-host shape and fails closed.
        SupportedHtmlElement::Textarea => {
            if el.children.is_empty() {
                return Ok(());
            }
            let has_value_bind = el
                .attrs
                .iter()
                .any(|a| matches!(a, AttrIr::Bind { target, .. } if target == "value"));
            let all_children_static_text = el
                .children
                .iter()
                .all(|&child| matches!(ir.node(child), IrNode::Text { .. }));
            if has_value_bind && all_children_static_text {
                Ok(())
            } else {
                refuse()
            }
        }
        // `<option>`: the supported shape carries STATIC text only (the
        // `<select><option>a</option></select>` form). An INTERPOLATION child is the
        // `option.__value` reactive-tracking surface; a nested element child is a
        // non-core option interior. Only literal text children are accepted.
        SupportedHtmlElement::Option => {
            for &child in &el.children {
                if !matches!(ir.node(child), IrNode::Text { .. }) {
                    return refuse();
                }
            }
            Ok(())
        }
        // `<select>`: the supported shape's children are STATIC `<option>` elements
        // (each itself gated by the `Option` arm when the child walk reaches it).
        // A non-`<option>` child (text other than insignificant whitespace, an
        // interpolation, a block) is not the supported select-host interior.
        SupportedHtmlElement::Select => {
            for &child in &el.children {
                match ir.node(child) {
                    // A child `<option>` element is the supported select interior (it
                    // is itself content-gated when the child walk classifies it).
                    IrNode::Element(child_el) if child_el.tag == "option" => {}
                    // Insignificant whitespace-only text between options is fine (the
                    // whitespace cleaner drops it); significant text / interpolation /
                    // any other node is not a supported select child.
                    IrNode::Text { text, .. } if text.trim().is_empty() => {}
                    _ => return refuse(),
                }
            }
            Ok(())
        }
        // Every other element (including `<audio>` / `<video>` / `<details>`, whose
        // bind-host forms are content-empty in the oracle but whose static interiors
        // are NOT special-content-model surfaces and flow through the ordinary child
        // walk) has no special-content restriction here.
        _ => Ok(()),
    }
}

/// A short namespace label for a fail-closed diagnostic.
pub(super) fn namespace_label(ns: Namespace) -> &'static str {
    match ns {
        Namespace::Html => "html",
        Namespace::Svg => "svg",
        Namespace::Mathml => "mathml",
    }
}

/// A short label for a `<svelte:*>` special kind.
pub(super) fn special_label(kind: SpecialKind) -> &'static str {
    match kind {
        SpecialKind::Head => "svelte:head",
        SpecialKind::Window => "svelte:window",
        SpecialKind::Document => "svelte:document",
        SpecialKind::Body => "svelte:body",
        SpecialKind::Element => "svelte:element",
        SpecialKind::Boundary => "svelte:boundary",
        SpecialKind::Options => "svelte:options",
        SpecialKind::Component => "svelte:component",
        SpecialKind::SelfRef => "svelte:self",
        SpecialKind::Fragment => "svelte:fragment",
    }
}
