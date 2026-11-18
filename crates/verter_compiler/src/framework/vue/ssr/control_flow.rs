//! `v-if` chains, `v-for` and `<template>` fragments.

use std::fmt::Write;

use crate::ast::types::ElementNodeConditionKind;
use crate::framework::vue::structure::VueWalkControl;
use crate::template::code_gen::shared::helpers::{self, SsrHelper};
use crate::template::topology::VueTemplateNodeId;

use super::state::{ElemCtx, SsrEmitter, SsrOut, Visit};

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// Open the statements of a `v-if` branch and of a `v-for` before the
    /// element's own output.
    pub(super) fn enter_control_flow(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let (el, oxc) = (v.el, v.oxc);
        let source = self.source;
        if self.is_slot_array_template(el) {
            // The slot template writes its condition or loop as an array entry.
            return;
        }

        if let Some(ref cond) = el.v_condition {
            self.close_push(el.tag_open.start, out);
            match cond.kind {
                ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
                    let oxc_cond = oxc.and_then(|o| o.condition.as_ref());
                    let expr_str = if let (Some(vs), Some(ve)) =
                        (cond.prop.value_start, cond.prop.value_end)
                    {
                        let raw = &source[vs as usize..ve as usize];
                        self.resolve_expr(raw, vs, oxc_cond)
                    } else {
                        "true".to_string()
                    };
                    self.buf.clear();
                    if cond.kind == ElementNodeConditionKind::If {
                        let _ = writeln!(self.buf, "if ({}) {{", expr_str);
                    } else {
                        let _ = writeln!(self.buf, "}} else if ({}) {{", expr_str);
                    }
                    out.prepend_alloc(el.tag_open.start, &self.buf);
                }
                ElementNodeConditionKind::Else => {
                    out.prepend_alloc(el.tag_open.start, "} else {\n");
                }
            }
        }

        if let Some(ref v_for) = el.v_for {
            self.v_for_depth.set(self.v_for_depth.get() + 1);

            // The fragment open marker joins the open push, or pushes alone.
            if self.in_push {
                out.prepend_alloc(el.tag_open.start, "<!--[-->");
                self.close_push(el.tag_open.start, out);
            } else {
                out.prepend_alloc(el.tag_open.start, "_push(`<!--[-->`)\n");
            }

            out.add_ssr_import(SsrHelper::RenderList);

            let full_expr = helpers::extract_directive_value(v_for, source);
            let (params, iterable) = helpers::parse_v_for_expression(full_expr);

            let resolved_iterable = match oxc.and_then(|o| o.v_for.as_ref()) {
                Some(vfor_data) => self.resolve_for_iterable(iterable, v_for, vfor_data),
                None => self.bindings.resolve_simple_expr(iterable),
            };

            self.buf.clear();
            let _ = writeln!(
                self.buf,
                "_ssrRenderList({}, ({}) => {{",
                resolved_iterable, params
            );
            out.prepend_alloc(el.tag_open.start, &self.buf);
        }
    }

    /// A `<template>` without `v-slot` renders its children in place, inside
    /// fragment markers unless it wraps exactly one element and loops not.
    pub(super) fn enter_template_fragment(
        &mut self,
        v: &Visit<'c>,
        out: &mut SsrOut<'c>,
    ) -> VueWalkControl {
        let el = v.el;
        let skip_frag = el.v_for.is_none() && self.has_single_element_child(v.tid);
        self.ensure_push(el.tag_open.start, out);
        let open = if skip_frag { "" } else { "<!--[-->" };
        out.overwrite_segmented(el.tag_open.start, el.tag_open.end, open, &[]);
        self.depth += 1;
        self.elem_ctx.push(ElemCtx::InParentPush);
        VueWalkControl::Continue
    }

    /// Close the element's `v-for` and `v-if` statements. A chain without a
    /// final `v-else` renders an empty comment in its implicit else.
    pub(super) fn leave_control_flow(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let el = v.el;
        if self.is_slot_array_template(el) {
            return;
        }
        let end = self.el_end(el);
        let mut closing = String::new();
        if el.v_for.is_some() {
            self.v_for_depth.set(self.v_for_depth.get() - 1);
            if let Some(close) = self.take_push_close() {
                closing.push_str(close);
            }
            closing.push_str("})\n_push(`<!--]-->");
            self.in_push = true;
        }
        if let Some(cond) = &el.v_condition {
            if let Some(close) = self.take_push_close() {
                closing.push_str(close);
            }
            closing.push_str(match cond.kind {
                ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf
                    if self.has_condition_continuation(v.tid) =>
                {
                    "\n"
                }
                ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
                    "} else {\n_push(`<!---->`)\n}\n"
                }
                ElementNodeConditionKind::Else => "}\n",
            });
        }
        if closing.is_empty() {
            return;
        }
        let topology = self.inputs.topology;
        let closes_moving_default = self.default_slot_open
            && self.default_slot_move_start.is_some()
            && topology.next_sibling(v.tid).is_some_and(|next| {
                topology.span(next).start == end
                    && topology.element(next).is_some_and(|el| {
                        el.tag_type == crate::ast::types::TagType::Template && el.v_slot.is_some()
                    })
            });
        if closes_moving_default {
            // The source range excludes this boundary. Its branch closers
            // belong to the default slot and travel in that slot's suffix.
            self.default_slot_edge_closing.push_str(&closing);
        } else {
            out.prepend_alloc(end, &closing);
        }
    }

    /// A later branch of the node's chain follows it.
    pub(super) fn has_condition_continuation(&self, tid: VueTemplateNodeId) -> bool {
        let topology = self.inputs.topology;
        topology
            .chain_of(tid)
            .is_some_and(|(chain, index)| (index as usize) + 1 < topology.branches(chain).len())
    }
}
