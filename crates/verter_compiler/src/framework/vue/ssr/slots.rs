//! Slots: slot functions of components, `<template v-slot>`, `<slot>` outlets.

use std::fmt::Write;

use crate::ast::types::{ElementNode, ElementNodeConditionKind, TagType};
use crate::framework::vue::structure::VueWalkControl;
use crate::template::code_gen::shared::helpers::{self, SsrHelper, VdomHelper};
use crate::template::oxc::types::OxcParsedElement;
use crate::template::topology::{VueTemplateNodeId, VueTemplateNodeKind};
use crate::template::vue_syntax::{camelize, needs_quoted_key};
use crate::types::NodeProp;

use super::escape::escape_js_string;
use super::state::{ElemCtx, SlotArray, SsrEmitter, SsrOut, Visit};

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// Build a slot object key from a `v-slot` directive prop.
    ///
    /// - Static: `#header` / `#header.id` → `header` / `"header.id"`
    /// - Dynamic: `#[name]` → `[_ctx.name]` (computed property key). Emitting the
    ///   brackets as a quoted string (`"[name]"`) makes a literal slot named
    ///   `"[name]"` instead of reading the binding — drop-in SSR fails.
    ///
    /// The dynamic name resolves through the OXC-parsed expression
    /// (`OxcParsedVSlot::dynamic_name`) so compound expressions
    /// (`#[foo.bar]`) resolve their root binding instead of passing through
    /// raw (a free identifier → ReferenceError), and template-scope locals
    /// (`#[name]` under `v-for="name in tabs"`) stay bare instead of
    /// mis-prefixing as `_ctx.name`.
    pub(super) fn build_slot_name(
        &self,
        v_slot: &NodeProp,
        source: &str,
        oxc_el: Option<&OxcParsedElement<'c>>,
    ) -> String {
        if let (Some(as_), Some(ae)) = (v_slot.arg_start, v_slot.arg_end) {
            let raw = &source[as_ as usize..ae as usize];
            if v_slot.is_dynamic == Some(true) {
                let trimmed = raw.trim();
                let (inner, inner_offset) = if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    (&trimmed[1..trimmed.len() - 1], as_ + 1)
                } else {
                    (trimmed, as_)
                };
                let oxc_name = oxc_el
                    .and_then(|e| e.v_slot.as_ref())
                    .and_then(|vs| vs.dynamic_name.as_ref());
                let resolved = self.resolve_expr(inner, inner_offset, oxc_name);
                format!("[{}]", resolved)
            } else if needs_quoted_key(raw) {
                format!("\"{}\"", raw)
            } else {
                raw.to_string()
            }
        } else {
            "default".to_string()
        }
    }

    /// Open an implicit `default: _withCtx(...)` wrapper at the given position.
    /// Called when the first non-template child of a ComponentWithSlots is encountered.
    pub(super) fn open_default_slot(&mut self, pos: u32, out: &mut SsrOut<'c>) {
        self.close_push(pos, out);
        out.add_vdom_import(VdomHelper::WithCtx);
        self.buf.clear();
        // After the slot array opened, the default slot is an array entry.
        let in_array = match self.slot_array_mut() {
            Some(list) if list.open => {
                list.default_in_array = true;
                true
            }
            _ => false,
        };
        let key = if in_array {
            "\n{\nname: \"default\",\nfn"
        } else {
            "\ndefault"
        };
        let _ = write!(
            self.buf,
            "{key}: _withCtx((_, _push, _parent, _scopeId) => {{\nif (_push) {{\n"
        );
        out.prepend_alloc(pos, &self.buf);
        self.default_slot_open = true;
        self.in_push = false;
        // Record start for potential reordering (default before named slots).
        // Use comp_children_start (component tag_open.end) to ensure Inserted
        // chunks are captured even when whitespace overwrites leave position gaps.
        self.default_slot_move_start = self.comp_children_start;
    }

    /// Extract slot name from a `<slot>` outlet element.
    /// Returns `("default", false)` if no `name` attribute is found.
    /// For dynamic `:name="expr"`, returns `(resolved_expr, true)`.
    pub(super) fn extract_slot_outlet_name(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'c>>,
        source: &str,
    ) -> (String, bool) {
        for (i, prop) in el.props.iter().enumerate() {
            let prop_name = &source[prop.start as usize..prop.name_end as usize];
            // Static name="header"
            if !prop.is_directive && prop_name == "name" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    return (source[vs as usize..ve as usize].to_string(), false);
                }
            }
            // Dynamic :name="expr" or v-bind:name="expr"
            if prop.is_directive
                && (prop_name.starts_with(':')
                    || (prop_name == "v-bind" && prop.arg_start.is_some()))
            {
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let attr = &source[as_ as usize..ae as usize];
                    if attr == "name" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| o.prop(i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            let resolved = self.resolve_expr(expr, vs, oxc_expr);
                            return (resolved, true);
                        }
                    }
                }
            }
        }
        ("default".to_string(), false)
    }

    /// Build slot outlet props (non-name static/dynamic attrs).
    /// Build the props argument for `_ssrRenderSlot`.
    /// Returns `(props_string, needs_merge_props)`.
    pub(super) fn build_slot_outlet_props(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'c>>,
        source: &str,
    ) -> (String, bool) {
        let mut parts: Vec<String> = Vec::new();
        let mut spreads: Vec<String> = Vec::new();
        for (i, prop) in el.props.iter().enumerate() {
            let prop_name = &source[prop.start as usize..prop.name_end as usize];
            if !prop.is_directive {
                if prop_name == "name" {
                    continue;
                }
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let value = &source[vs as usize..ve as usize];
                    // Slot outlet props are always camelized (like component props)
                    let key = camelize(prop_name);
                    parts.push(format!("{}: \"{}\"", key, escape_js_string(value)));
                }
            } else if prop_name == "v-bind" && prop.arg_start.is_none() {
                // v-bind="obj" spread (no argument)
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let oxc_prop = oxc.and_then(|o| o.prop(i));
                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                    let resolved = self.resolve_expr(expr, vs, oxc_expr);
                    spreads.push(resolved);
                }
            } else if prop_name.starts_with(':')
                || (prop_name == "v-bind" && prop.arg_start.is_some())
            {
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let attr = &source[as_ as usize..ae as usize];
                    // Skip :name — handled by extract_slot_outlet_name
                    if attr == "name" {
                        continue;
                    }
                    let key = camelize(attr);
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_prop = oxc.and_then(|o| o.prop(i));
                        let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                        let resolved = self.resolve_expr(expr, vs, oxc_expr);
                        parts.push(format!("{}: {}", key, resolved));
                    } else {
                        // Same-name shorthand: `:items` ≡ `:items="items"`
                        // Camelize the attr for binding lookup — hyphenated
                        // names like `heading-value` must resolve to
                        // `headingValue`, not `heading - value` (subtraction).
                        let prefix = self.bindings.resolve_prefix(&key);
                        let resolved = if prefix.is_empty() {
                            format!("_ctx.{}", key)
                        } else {
                            format!("{}{}", prefix, key)
                        };
                        parts.push(format!("{}: {}", key, resolved));
                    }
                }
            }
        }

        if spreads.is_empty() {
            // No v-bind spread — same as before
            if parts.is_empty() {
                ("{}".to_string(), false)
            } else {
                (format!("{{ {} }}", parts.join(", ")), false)
            }
        } else if parts.is_empty() {
            // Only v-bind spread(s), no individual props
            if spreads.len() == 1 {
                // Single spread — use directly, no _mergeProps needed
                (spreads.remove(0), false)
            } else {
                (format!("_mergeProps({})", spreads.join(", ")), true)
            }
        } else {
            // v-bind spread(s) + individual props → _mergeProps(spread, { props })
            let obj = format!("{{ {} }}", parts.join(", "));
            let mut args: Vec<String> = spreads;
            args.push(obj);
            (format!("_mergeProps({})", args.join(", ")), true)
        }
    }

    pub(super) fn open_implicit_default_slot(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let el = v.el;
        // When inside a ComponentWithSlots with named slots, non-template
        // children need to be wrapped in `default: _withCtx(...)`.
        if matches!(
            self.elem_ctx.last(),
            Some(&ElemCtx::ComponentWithSlots) | Some(&ElemCtx::DynamicComponentWithSlots)
        ) && !(el.tag_type == TagType::Template && el.v_slot.is_some())
            && !self.default_slot_open
        {
            self.open_default_slot(el.tag_open.start, out);
        }
    }

    pub(super) fn enter_slot_template(
        &mut self,
        v: &Visit<'c>,
        out: &mut SsrOut<'c>,
    ) -> VueWalkControl {
        let (tid, el, oxc) = (v.tid, v.el, v.oxc);
        let source = self.source;
        // Check if we're inside a Suspense FIRST — Suspense slots use simple
        // arrow functions, not _withCtx with VDOM fallback. This must happen
        // before the default_slot_open check, because when Suspense is nested
        // inside a ComponentWithSlots, `default_slot_open` refers to the OUTER
        // component's slot, not the Suspense's implicit default.
        let in_suspense = self
            .elem_ctx
            .iter()
            .rev()
            .any(|ctx| matches!(ctx, ElemCtx::SuspenseSlots));

        // If a default slot was opened for preceding non-template children,
        // close it before starting the named slot.
        // Skip this when inside Suspense — the outer component's default slot
        // is NOT being closed here; the Suspense implicit default is handled below.
        if !in_suspense && self.default_slot_open {
            let push_close = self.take_push_close();
            // Walk up to parent component to generate VDOM fallback for default children
            let parent = self
                .inputs
                .topology
                .parent(tid)
                .filter(|&p| self.is_element_like(p));
            let vdom_fallback = match parent {
                Some(parent) => self.slot_content(parent, out),
                None => None,
            };
            self.buf.clear();
            self.buf
                .push_str(&std::mem::take(&mut self.default_slot_edge_closing));
            if let Some(close) = push_close {
                self.buf.push_str(close);
            }
            self.buf.push_str("} else {\nreturn ");
            let hole = self.buf.len() as u32;
            self.buf.push_str("[]\n}\n}),");
            let in_array = self.slot_arrays.last().is_some_and(Option::is_some);
            if let Some(list) = self.slot_array_mut() {
                if std::mem::take(&mut list.default_in_array) {
                    self.buf.push_str("\n},");
                }
            }
            let closing = self.slot_closing(vdom_fallback, hole, out);
            self.default_slot_open = false;
            if self.default_slot_move_start.is_some() && !in_array {
                // The default slot moves after the named ones when the
                // component closes; its closing text travels with it.
                self.default_slot_move_end = Some(el.tag_open.start);
                self.default_slot_close = Some(closing);
            } else {
                out.prepend_allocated(el.tag_open.start, closing);
            }
        }

        let slot_name = if let Some(ref v_slot) = el.v_slot {
            self.build_slot_name(v_slot, source, oxc)
        } else {
            "default".to_string()
        };

        let params = if let Some(ref v_slot) = el.v_slot {
            if let (Some(vs), Some(ve)) = (v_slot.value_start, v_slot.value_end) {
                let p = source[vs as usize..ve as usize].trim();
                if p.is_empty() {
                    "_"
                } else {
                    p
                }
            } else {
                "_"
            }
        } else {
            "_"
        };

        self.buf.clear();
        if in_suspense {
            // If there's an open implicit default slot (mixed content Suspense),
            // close it before emitting the named slot entry.
            if self.suspense_implicit_default_open {
                self.close_push(el.tag_open.start, out);
                self.buf.push_str("},\n");
                self.suspense_implicit_default_open = false;
            }
            let _ = write!(self.buf, "\n{}: () => {{\n", slot_name);
            out.overwrite_segmented(el.tag_open.start, el.tag_open.end, &self.buf, &[]);
            self.in_push = false;
            self.elem_ctx.push(ElemCtx::SuspenseSlotTemplate);
        } else {
            let tail = self.open_slot_array_entry(v, &slot_name, out);
            let key_sep = if tail.is_some() {
                "fn"
            } else {
                slot_name.as_str()
            };
            let _ = write!(
                self.buf,
                "\n{}: _withCtx(({}, _push, _parent, _scopeId) => {{\nif (_push) {{\n",
                key_sep, params
            );
            self.slot_entry_tails.push(tail);
            out.overwrite_segmented(el.tag_open.start, el.tag_open.end, &self.buf, &[]);
            self.in_push = false;
            // Track scoped slot depth for child component DYNAMIC flag
            let is_scoped = params != "_";
            if is_scoped {
                self.v_slot_scope_depth += 1;
            }
            self.scoped_slot_entered.push(is_scoped);
            self.elem_ctx.push(ElemCtx::SlotTemplate);
            self.depth += 1;
        }
        VueWalkControl::Continue
    }

    pub(super) fn enter_slot_outlet(
        &mut self,
        v: &Visit<'c>,
        out: &mut SsrOut<'c>,
    ) -> VueWalkControl {
        let (el, oxc) = (v.el, v.oxc);
        let source = self.source;
        self.close_push(el.tag_open.start, out);
        out.add_ssr_import(SsrHelper::RenderSlot);

        let (slot_name, is_dynamic_name) = self.extract_slot_outlet_name(el, oxc, source);
        let (props, needs_merge_props) = self.build_slot_outlet_props(el, oxc, source);
        if needs_merge_props {
            out.add_vdom_import(VdomHelper::MergeProps);
        }

        let has_children = el.content.as_ref().is_some_and(|c| !c.children.is_empty());

        // Format slot name: quoted for static, expression for dynamic
        let name_arg = if is_dynamic_name {
            slot_name.clone()
        } else {
            format!("\"{}\"", slot_name)
        };

        let sid_arg = self.slot_outlet_scope_arg();
        self.buf.clear();
        if has_children {
            let _ = writeln!(
                self.buf,
                "_ssrRenderSlot(_ctx.$slots, {}, {}, () => {{",
                name_arg, props
            );
            out.overwrite_segmented(el.tag_open.start, el.tag_open.end, &self.buf, &[]);
            self.in_push = false;
            self.elem_ctx.push(ElemCtx::SlotOutletFallback);
            self.depth += 1;
        } else {
            let _ = writeln!(
                self.buf,
                "_ssrRenderSlot(_ctx.$slots, {}, {}, null, _push, _parent{})",
                name_arg, props, sid_arg
            );
            let el_end = self.el_end(el);
            out.overwrite_segmented(el.tag_open.start, el_end, &self.buf, &[]);
            self.elem_ctx.push(ElemCtx::Complete);
        }
        VueWalkControl::Continue
    }

    pub(super) fn leave_component_with_slots(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let sid_arg = self.scope_id_arg();
        self.close_component_slots(v, &format!(", _parent{sid_arg}))\n"), out);
    }

    pub(super) fn leave_dynamic_component_with_slots(
        &mut self,
        v: &Visit<'c>,
        out: &mut SsrOut<'c>,
    ) {
        let sid_arg = self.scope_id_arg();
        self.close_component_slots(v, &format!("), _parent{sid_arg})\n"), out);
    }

    /// Close a component's slots, then its render call with `call_close`.
    ///
    /// Stability: `_: 2 /* DYNAMIC */` when slots are conditional, repeated
    /// or dynamically named, or inside a scoped slot (the server render
    /// re-runs with it); `_: 3 /* FORWARDED */` when the content forwards
    /// slots through `<slot>`; `_: 1 /* STABLE */` otherwise.
    fn close_component_slots(&mut self, v: &Visit<'c>, call_close: &str, out: &mut SsrOut<'c>) {
        let (tid, el) = (v.tid, v.el);
        self.in_component_slots -= 1;
        let was_scoped = self.scoped_slot_entered.pop() == Some(true);
        if was_scoped {
            self.v_slot_scope_depth -= 1;
        }
        let has_named_slots = self.has_template_slot_children(tid);
        let slot_flag = if self.has_dynamic_slots(tid) || self.v_slot_scope_depth > 0 {
            "_: 2 /* DYNAMIC */"
        } else if self.has_slot_outlet_in_descendants(tid) {
            "_: 3 /* FORWARDED */"
        } else {
            "_: 1 /* STABLE */"
        };

        // The literal ends before the closing code that replaces the tag.
        let close_pos = el
            .tag_close
            .as_ref()
            .map(|tc| tc.start)
            .unwrap_or(self.el_end(el));
        self.close_push(close_pos, out);

        let array = self.slot_arrays.pop().flatten();
        // A slot array replaces the object's stability key.
        let (object_close, array_default) = match &array {
            Some(list) if list.open => (
                "\n])".to_string(),
                if list.default_in_array { "\n}," } else { "" },
            ),
            Some(_) => (format!("\n{}\n}})", slot_flag), ""),
            None => (format!("\n{}\n}}", slot_flag), ""),
        };
        self.buf.clear();
        let fallback = if !has_named_slots {
            self.slot_content(tid, out)
        } else if self.default_slot_open {
            self.default_slot_open = false;
            self.slot_content(tid, out)
        } else {
            None
        };
        let closing = if let Some(fill) = fallback {
            self.buf.push_str("} else {\nreturn ");
            let hole = self.buf.len() as u32;
            let _ = write!(
                self.buf,
                "[]\n}}\n}}),{}{}{}",
                array_default, object_close, call_close
            );
            self.slot_closing(Some(fill), hole, out)
        } else {
            let _ = write!(self.buf, "{}{}", object_close, call_close);
            out.alloc_str(&self.buf)
        };
        if let Some(ref tc) = el.tag_close {
            out.overwrite_unmapped_alloc(tc.start, tc.end, closing);
        } else {
            let el_end = self.el_end(el);
            out.prepend_allocated(el_end, closing);
        }
        // A default slot opened before the named ones moves after them,
        // carrying the text that closed it; in a slot array it stays an
        // object entry where it is.
        let moved = (
            self.default_slot_move_start.take(),
            self.default_slot_move_end.take(),
            self.default_slot_close.take(),
        );
        if let (Some(move_start), Some(move_end), Some(closing)) = moved {
            out.relocate_allocated(move_start, move_end, close_pos, "", closing);
        }
        self.default_slot_open = self.saved_default_slot_open.pop().unwrap_or(false);
        if let Some((ms, me, mc)) = self.saved_default_slot_move.pop() {
            self.default_slot_move_start = ms;
            self.default_slot_move_end = me;
            self.default_slot_close = mc;
        }
    }

    pub(super) fn leave_slot_template(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let (tid, el) = (v.tid, v.el);
        // Close the named slot's _withCtx wrapper
        self.depth -= 1;
        // Restore scoped slot depth for SSR push path.
        let was_scoped = self.scoped_slot_entered.pop() == Some(true);
        if was_scoped {
            self.v_slot_scope_depth -= 1;
        }
        let close_pos = el
            .tag_close
            .as_ref()
            .map(|tc| tc.start)
            .unwrap_or(self.el_end(el));
        self.close_push(close_pos, out);

        let vdom_fallback = self.slot_content(tid, out);
        self.buf.clear();
        self.buf.push_str("} else {\nreturn ");
        let hole = self.buf.len() as u32;
        self.buf.push_str("[]\n}\n})");
        match self.slot_entry_tails.pop().flatten() {
            Some(tail) => self.buf.push_str(&tail),
            None => self.buf.push(','),
        }

        let closing = self.slot_closing(vdom_fallback, hole, out);
        if let Some(ref tc) = el.tag_close {
            out.overwrite_unmapped_alloc(tc.start, tc.end, closing);
        } else {
            let el_end = self.el_end(el);
            out.prepend_allocated(el_end, closing);
        }
    }

    pub(super) fn leave_suspense_slot_template(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let el = v.el;
        // Close a Suspense slot's simple arrow function: `},`
        let close_pos = el
            .tag_close
            .as_ref()
            .map(|tc| tc.start)
            .unwrap_or(self.el_end(el));
        self.close_push(close_pos, out);

        self.buf.clear();
        self.buf.push_str("},");

        if let Some(ref tc) = el.tag_close {
            out.overwrite_segmented(tc.start, tc.end, &self.buf, &[]);
        } else {
            let el_end = self.el_end(el);
            out.prepend_alloc(el_end, &self.buf);
        }
    }

    pub(super) fn leave_slot_outlet_fallback(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let el = v.el;
        // Close the fallback function of _ssrRenderSlot
        self.depth -= 1;
        let close_pos = el
            .tag_close
            .as_ref()
            .map(|tc| tc.start)
            .unwrap_or(self.el_end(el));
        self.close_push(close_pos, out);

        let sid_arg = self.slot_outlet_scope_arg();
        self.buf.clear();
        let _ = writeln!(self.buf, "}}, _push, _parent{})", sid_arg);

        if let Some(ref tc) = el.tag_close {
            out.overwrite_segmented(tc.start, tc.end, &self.buf, &[]);
        } else {
            let el_end = self.el_end(el);
            out.prepend_alloc(el_end, &self.buf);
        }
    }

    pub(super) fn leave_suspense_slots(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let (tid, el) = (v.tid, v.el);
        // Close the _ssrRenderSuspense slots object
        self.in_component_slots -= 1;
        let close_pos = el
            .tag_close
            .as_ref()
            .map(|tc| tc.start)
            .unwrap_or(self.el_end(el));
        self.close_push(close_pos, out);

        let has_named_slots = self.has_template_slot_children(tid);
        self.buf.clear();
        if has_named_slots {
            // Named slots already closed by SuspenseSlotTemplate leave handlers
            self.buf.push_str("\n_: 1 /* STABLE */\n})\n");
        } else {
            // Implicit default: close the arrow function body + object + call
            self.buf.push_str("},\n_: 1 /* STABLE */\n})\n");
        }

        if let Some(ref tc) = el.tag_close {
            out.overwrite_segmented(tc.start, tc.end, &self.buf, &[]);
        } else {
            let el_end = self.el_end(el);
            out.prepend_alloc(el_end, &self.buf);
        }
    }
}

/// What a component's or `<template>`'s direct children contain.
impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    fn children_of(&self, tid: VueTemplateNodeId) -> impl Iterator<Item = VueTemplateNodeId> + '_ {
        self.inputs.topology.children(tid)
    }

    fn is_slot_template(&self, tid: VueTemplateNodeId) -> bool {
        self.kind(tid) == VueTemplateNodeKind::Template && self.element(tid).v_slot.is_some()
    }

    fn is_blank_text(&self, tid: VueTemplateNodeId) -> bool {
        self.kind(tid) == VueTemplateNodeKind::Text && self.text(tid).trim().is_empty()
    }

    /// A child is a `<template v-slot>`.
    pub(super) fn has_template_slot_children(&self, tid: VueTemplateNodeId) -> bool {
        self.children_of(tid)
            .any(|child| self.is_slot_template(child))
    }

    /// A child outside every `<template v-slot>` renders content: an
    /// element, an interpolation or non-blank text (comments do not).
    pub(super) fn has_bare_children(&self, tid: VueTemplateNodeId) -> bool {
        self.children_of(tid).any(|child| match self.kind(child) {
            VueTemplateNodeKind::Comment | VueTemplateNodeKind::Root => false,
            VueTemplateNodeKind::Text => !self.is_blank_text(child),
            VueTemplateNodeKind::Interpolation => true,
            _ => !self.is_slot_template(child),
        })
    }

    /// The slots are dynamic: inside a `v-for`, or a slot template is
    /// conditional, repeated or dynamically named.
    pub(super) fn has_dynamic_slots(&self, tid: VueTemplateNodeId) -> bool {
        if self.v_for_depth.get() > 0 {
            return true;
        }
        self.children_of(tid).any(|child| {
            self.is_slot_template(child) && {
                let el = self.element(child);
                el.v_condition.is_some()
                    || el.v_for.is_some()
                    || el
                        .v_slot
                        .as_ref()
                        .is_some_and(|slot| slot.is_dynamic == Some(true))
            }
        })
    }

    /// Exactly one element child (one per `v-if` chain) and no text or
    /// interpolation beside it.
    pub(super) fn has_single_element_child(&self, tid: VueTemplateNodeId) -> bool {
        let mut element_count = 0;
        for child in self.children_of(tid) {
            match self.kind(child) {
                VueTemplateNodeKind::Comment | VueTemplateNodeKind::Root => {}
                VueTemplateNodeKind::Interpolation => return false,
                VueTemplateNodeKind::Text => {
                    if !self.is_blank_text(child) {
                        return false;
                    }
                }
                _ => {
                    if !self.continues_chain(child) {
                        element_count += 1;
                    }
                }
            }
        }
        element_count == 1
    }

    /// Any child other than blank text.
    pub(super) fn has_effective_children(&self, tid: VueTemplateNodeId) -> bool {
        self.children_of(tid).any(|child| {
            self.kind(child) != VueTemplateNodeKind::Text || !self.is_blank_text(child)
        })
    }

    /// A `<slot>` outlet among the descendants: the slot content forwards
    /// the parent's slots.
    pub(super) fn has_slot_outlet_in_descendants(&self, tid: VueTemplateNodeId) -> bool {
        self.inputs
            .topology
            .subtree(tid)
            .skip(1)
            .any(|node| self.kind(node) == VueTemplateNodeKind::SlotOutlet)
    }
}

/// Conditional and repeated slot templates: `_createSlots`'s array.
impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// A slot template child carries `v-if`/`v-else-if`/`v-else` or `v-for`.
    pub(super) fn has_conditional_slot_templates(&self, tid: VueTemplateNodeId) -> bool {
        self.children_of(tid).any(|child| {
            self.is_slot_template(child) && {
                let el = self.element(child);
                el.v_condition.is_some() || el.v_for.is_some()
            }
        })
    }

    /// The slot array of the innermost component with slot children.
    pub(super) fn slot_array_mut(&mut self) -> Option<&mut SlotArray> {
        self.slot_arrays.last_mut().and_then(Option::as_mut)
    }

    /// The element is a slot template of a component whose slots form an
    /// array; its `v-if` and `v-for` are written as array expressions, not
    /// as statements.
    pub(super) fn is_slot_array_template(&self, el: &ElementNode) -> bool {
        el.tag_type == TagType::Template
            && el.v_slot.is_some()
            && self.in_component_slots > 0
            && matches!(self.slot_arrays.last(), Some(Some(_)))
    }

    /// Write the start of a slot template's entry into `buf`, up to its
    /// `fn` key, when it belongs in the slot array, and return the text that
    /// closes the entry after its `_withCtx(…)`. A static slot before the
    /// first conditional one stays in the object (`None`).
    fn open_slot_array_entry(
        &mut self,
        v: &Visit<'c>,
        slot_name: &str,
        out: &mut SsrOut<'c>,
    ) -> Option<String> {
        let (tid, el, oxc) = (v.tid, v.el, v.oxc);
        let source = self.source;
        let dynamic = el.v_condition.is_some() || el.v_for.is_some();
        let list = self.slot_array_mut()?;
        if !list.open && !dynamic {
            return None;
        }
        let mut head = String::new();
        if !list.open {
            list.open = true;
            head.push_str("\n_: 2 /* DYNAMIC */\n}, [");
        }
        let key = list.next_key;
        // An object key (`a`, `"a.b"`, `[expr]`) as a name expression.
        let name = match slot_name
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
        {
            Some(expr) => expr.to_string(),
            None if slot_name.starts_with('"') => slot_name.to_string(),
            None => format!("\"{slot_name}\""),
        };

        let tail = if let Some(cond) = &el.v_condition {
            self.slot_array_mut()
                .expect("slot array checked above")
                .next_key += 1;
            let test = || {
                let oxc_cond = oxc.and_then(|o| o.condition.as_ref());
                match (cond.prop.value_start, cond.prop.value_end) {
                    (Some(vs), Some(ve)) => {
                        self.resolve_expr(&source[vs as usize..ve as usize], vs, oxc_cond)
                    }
                    _ => "true".to_string(),
                }
            };
            match cond.kind {
                ElementNodeConditionKind::If => {
                    let _ = write!(head, "\n({})\n? {{", test());
                }
                ElementNodeConditionKind::ElseIf => {
                    let _ = write!(head, "\n: ({})\n? {{", test());
                }
                ElementNodeConditionKind::Else => head.push_str("\n: {"),
            }
            let mut tail = format!(",\nkey: \"{key}\"\n}}");
            if cond.kind == ElementNodeConditionKind::Else {
                tail.push(',');
            } else if !self.has_condition_continuation(tid) {
                tail.push_str("\n: undefined,");
            }
            tail
        } else if let Some(v_for) = &el.v_for {
            out.add_vdom_import(VdomHelper::RenderList);
            let full_expr = helpers::extract_directive_value(v_for, source);
            let (params, iterable) = helpers::parse_v_for_expression(full_expr);
            let iterable = match oxc.and_then(|o| o.v_for.as_ref()) {
                Some(vfor) => self.resolve_for_iterable(iterable, v_for, vfor),
                None => self.bindings.resolve_simple_expr(iterable),
            };
            let _ = write!(
                head,
                "\n_renderList({iterable}, ({params}) => {{\nreturn {{"
            );
            "\n}\n}),".to_string()
        } else {
            head.push_str("\n{");
            "\n},".to_string()
        };
        let _ = write!(head, "\nname: {name},");
        self.buf.push_str(&head);
        Some(tail)
    }
}

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// The scope argument of `_ssrRenderSlot`: a scoped component passes its
    /// own `-s` slot scope, followed inside a slot function by the inherited
    /// `_scopeId`; an unscoped one passes only the inherited `_scopeId`.
    fn slot_outlet_scope_arg(&self) -> String {
        match (&self.scope_id, self.in_slot_function()) {
            (Some(id), true) => format!(", \"{id}-s\" + _scopeId"),
            (Some(id), false) => format!(", \"{id}-s\""),
            (None, true) => ", _scopeId".to_string(),
            (None, false) => String::new(),
        }
    }

    /// Whether the current emission inherits a slot function's scope ID.
    pub(super) fn in_slot_function(&self) -> bool {
        // `_scopeId` is a parameter of `_withCtx` slot functions; a
        // `<Teleport>` or `<Suspense>` callback declares only `_push`, and
        // sees `_scopeId` only when a slot function encloses it.
        self.elem_ctx.iter().any(|ctx| {
            matches!(
                ctx,
                ElemCtx::ComponentWithSlots
                    | ElemCtx::DynamicComponentWithSlots
                    | ElemCtx::SlotTemplate
            )
        })
    }
}
