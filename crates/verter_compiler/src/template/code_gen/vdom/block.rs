//! Block and condition-chain helpers for the VDOM backend.
//!
//! This module contains functions that deal with structural condition chains
//! (`v-if` / `v-else-if` / `v-else`) and `<template>` Fragment rendering.
//! These are used by `enter_element` / `leave_element` in mod.rs and by
//! the `visit_text` / `visit_comment` interstitial-node checks.

use crate::ast::types::{AstNodeKind, ElementNode, ElementNodeConditionKind};
use crate::types::NodeId;

use super::super::shared::helpers::VdomHelper;
use super::super::types::CodeGenOutput;
use super::{children, directives, element, VdomCodeGen};

impl<'ast, 'alloc> VdomCodeGen<'ast, 'alloc> {
    /// Check whether the next *element* sibling of `id` is a v-else-if or
    /// v-else continuation. Scans forward from `index_in_parent + 1`,
    /// skipping non-element nodes (text, comments, interpolations) that
    /// commonly appear as whitespace between conditional elements.
    ///
    /// Used to downgrade `IfTernary` to `ElseIfTernary` (or upgrade
    /// `ElseIfTernary` to `IfTernary`) so the scope close emits the
    /// correct suffix.
    pub(super) fn has_next_condition_sibling(&self, id: NodeId) -> bool {
        let node = &self.ast.nodes[id.0];

        let children = match node.parent {
            None => self
                .ast
                .root
                .content
                .as_ref()
                .map(|c| c.children.as_slice())
                .unwrap_or(&[]),
            Some(parent_id) => {
                if let AstNodeKind::Element(ref parent_el) = self.ast.nodes[parent_id.0].kind {
                    parent_el
                        .content
                        .as_ref()
                        .map(|c| c.children.as_slice())
                        .unwrap_or(&[])
                } else {
                    return false;
                }
            }
        };

        // Scan forward, skipping non-element siblings (text, comments, etc.)
        #[allow(clippy::needless_range_loop)]
        // Index-based: loop starts at non-zero offset into children slice
        for idx in (node.index_in_parent + 1)..children.len() {
            let next_node = &self.ast.nodes[children[idx].0];
            if let AstNodeKind::Element(ref next_el) = next_node.kind {
                if let Some(cond) = &next_el.v_condition {
                    return matches!(
                        cond.kind,
                        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                    );
                }
                return false; // Next element has no condition -- stop scanning
            }
            // Text/Comment/Interpolation: skip and continue scanning
        }
        false
    }

    /// Check whether a non-element node (comment, text) is between two
    /// condition chain members (v-if/v-else-if/v-else). Vue's compiler
    /// discards such nodes --- they appear in the source between branches
    /// but are not rendered.
    ///
    /// Returns `true` if the previous element sibling has a v-condition
    /// (Start or Continuation) AND the next element sibling is a Continuation.
    pub(super) fn is_interstitial_condition_node(&self, id: NodeId) -> bool {
        let node = &self.ast.nodes[id.0];

        let children = match node.parent {
            None => self
                .ast
                .root
                .content
                .as_ref()
                .map(|c| c.children.as_slice())
                .unwrap_or(&[]),
            Some(parent_id) => {
                if let AstNodeKind::Element(ref parent_el) = self.ast.nodes[parent_id.0].kind {
                    parent_el
                        .content
                        .as_ref()
                        .map(|c| c.children.as_slice())
                        .unwrap_or(&[])
                } else {
                    return false;
                }
            }
        };

        // Check if previous element sibling has a v-condition
        let mut has_prev_condition = false;
        for idx in (0..node.index_in_parent).rev() {
            let prev_node = &self.ast.nodes[children[idx].0];
            if let AstNodeKind::Element(ref prev_el) = prev_node.kind {
                has_prev_condition = prev_el.v_condition.is_some();
                break;
            }
        }
        if !has_prev_condition {
            return false;
        }

        // Check if next element sibling is a Continuation
        #[allow(clippy::needless_range_loop)]
        // Index-based: loop starts at non-zero offset into children slice
        for idx in (node.index_in_parent + 1)..children.len() {
            let next_node = &self.ast.nodes[children[idx].0];
            if let AstNodeKind::Element(ref next_el) = next_node.kind {
                return next_el.v_condition.as_ref().is_some_and(|c| {
                    matches!(
                        c.kind,
                        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                    )
                });
            }
        }
        false
    }

    /// Handle `<template v-if>` / `<template v-for>` as Fragment.
    ///
    /// Renders the children inside a Fragment instead of a literal `<template>`
    /// element. The open/close tags are replaced with Fragment VNode calls.
    ///
    /// Output: `(_openBlock(), _createElementBlock(_Fragment, null, [children], 64))`
    pub(super) fn leave_template_fragment(
        &mut self,
        el: &ElementNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
        injected_key: Option<u32>,
    ) {
        let el_children = el
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);

        let mut children = self.build_child_records(el_children, source);
        element::resolve_whitespace(&mut children, out, true);
        element::strip_interstitial_condition_nodes(&mut children, out, true);

        // Imports
        out.add_vdom_import(VdomHelper::OpenBlock);
        out.add_vdom_import(VdomHelper::CreateElementBlock);
        out.add_vdom_import(VdomHelper::Fragment);

        // Build open tag replacement: (_openBlock(), _createElementBlock(_Fragment, null, [
        let mut prefix = String::with_capacity(128);
        // Pop this element's v_for_prefix entry (pushed by enter_element).
        // IMPORTANT: pop() already consumes the entry -- do NOT pop again in
        // the else branch, or a nested <template v-if> inside a <template v-for>
        // will double-pop and corrupt the stack.
        if let Some((v_for_prefix, _iterable_src)) = self.v_for_prefixes.pop().flatten() {
            prefix.push_str(&v_for_prefix);
        }
        // `<template v-if>` branch identity: official Vue injects `{ key: N }` on
        // the Fragment so it patches distinctly against sibling ternary arms.
        // (When v-for also applies, the key rides the outer `_renderList`
        // Fragment built in enter_element, so `injected_key` is `None` here.)
        if let Some(k) = injected_key {
            prefix.push_str("(_openBlock(), _createElementBlock(_Fragment, { key: ");
            prefix.push_str(&k.to_string());
            prefix.push_str(" }, [\n");
        } else {
            prefix.push_str("(_openBlock(), _createElementBlock(_Fragment, null, [\n");
        }
        out.overwrite(el.tag_open.start, el.tag_open.end, &prefix);

        // Add child separators for array mode
        children::add_children_separators_array(
            &children,
            out,
            &self.options,
            source,
            self.ast,
            el_children,
        );

        // Build close tag replacement: ], 64 /* STABLE_FRAGMENT */))
        let suffix = if self.options.is_production {
            "\n], 64))"
        } else {
            "\n], 64 /* STABLE_FRAGMENT */))"
        };
        if let Some(tag_close) = &el.tag_close {
            out.overwrite(tag_close.start, tag_close.end, suffix);
        }

        // Apply scope close suffix for structural directives
        let record_end = el
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(el.tag_open.end);
        if let Some(scope_close) = self.scope_closes.pop().flatten() {
            let close_suffix =
                directives::format_scope_close(&scope_close, self.options.is_production);
            if !close_suffix.is_empty() {
                out.prepend_static(record_end, close_suffix);
            }
        }
    }
}
