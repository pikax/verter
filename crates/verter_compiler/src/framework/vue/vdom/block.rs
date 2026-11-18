//! `<template v-if>` and `<template v-for>` rendered as a Fragment.

use crate::ast::types::ElementNode;

use super::state::VdomOut;
use super::{children, VdomState};
use crate::template::code_gen::shared::helpers::VdomHelper;

impl<'c> VdomState<'c> {
    /// Handle `<template v-if>` / `<template v-for>` as Fragment.
    ///
    /// Renders the children inside a Fragment instead of a literal `<template>`
    /// element. The open/close tags are replaced with Fragment VNode calls.
    ///
    /// Output: `(_openBlock(), _createElementBlock(_Fragment, null, [children], 64))`
    pub(super) fn leave_template_fragment(
        &mut self,
        el: &ElementNode,
        source: &'c str,
        out: &mut VdomOut<'c>,
        injected_key: Option<u32>,
    ) {
        let el_children = el
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);

        let children = self.build_child_records(el_children, source);

        // Imports
        out.add_vdom_import(VdomHelper::OpenBlock);
        let block = self.options.element_block();
        out.add_vdom_import(block);
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
            prefix.push_str("(_openBlock(), ");
            prefix.push_str(block.name());
            prefix.push_str("(_Fragment, { key: ");
            prefix.push_str(&k.to_string());
            prefix.push_str(" }, [\n");
        } else {
            prefix.push_str("(_openBlock(), ");
            prefix.push_str(block.name());
            prefix.push_str("(_Fragment, null, [\n");
        }
        // Build close tag replacement: ], 64 /* STABLE_FRAGMENT */))
        let suffix = if self.options.is_production {
            "\n], 64))"
        } else {
            "\n], 64 /* STABLE_FRAGMENT */))"
        };
        // A self-closing `<template />` has no close tag to carry the suffix:
        // the single open-tag overwrite emits the whole empty fragment.
        if el.tag_close.is_none() {
            prefix.push_str(suffix);
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
            self.prepend_scope_close(out, record_end, &scope_close);
        }
    }
}
