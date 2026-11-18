//! Plain elements: open and close tags inside the push buffer.

use std::fmt::Write;

use crate::ast::types::TagType;
use crate::code_transform::{AnchorKind, SegmentAnchor};
use crate::framework::vue::structure::VueWalkControl;
use crate::template::code_gen::shared::helpers::SsrHelper;
use crate::utils::vue::tag::is_void_tag;

use super::state::{ElemCtx, SsrEmitter, SsrOut, Visit};

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    pub(super) fn enter_plain_element(
        &mut self,
        v: &Visit<'c>,
        out: &mut SsrOut<'c>,
    ) -> VueWalkControl {
        let (el, oxc, is_root) = (v.el, v.oxc, v.is_root);
        let tag_name = v.tag_name;
        let source = self.source;
        let is_void = is_void_tag(tag_name.as_bytes());
        let attrs = self.build_attrs_string(el, oxc, source, out, is_root);
        let (attrs_str, attrs_anchor) = (attrs.text.as_str(), attrs.anchor);

        let sid = self.scope_id_suffix();

        // v-html: entire element is one _push
        if let Some(html_expr) = self.get_v_html_expr(el, oxc, source) {
            self.ensure_push(el.tag_open.start, out);
            self.buf.clear();
            let _ = write!(self.buf, "<{}{}{}>", tag_name, attrs_str, sid);
            let _ = write!(self.buf, "${{({}) ?? ''}}", html_expr);
            if !is_void {
                let _ = write!(self.buf, "</{}>", tag_name);
            }
            let el_end = self.el_end(el);
            out.overwrite_segmented(el.tag_open.start, el_end, &self.buf, &[]);
            self.elem_ctx.push(ElemCtx::Complete);
            return VueWalkControl::Continue;
        }

        // v-text: entire element is one _push
        if let Some(text_expr) = self.get_v_text_expr(el, oxc, source) {
            out.add_ssr_import(SsrHelper::Interpolate);
            self.ensure_push(el.tag_open.start, out);
            self.buf.clear();
            let _ = write!(self.buf, "<{}{}{}>", tag_name, attrs_str, sid);
            let _ = write!(self.buf, "${{_ssrInterpolate({})}}", text_expr);
            if !is_void {
                let _ = write!(self.buf, "</{}>", tag_name);
            }
            let el_end = self.el_end(el);
            out.overwrite_segmented(el.tag_open.start, el_end, &self.buf, &[]);
            self.elem_ctx.push(ElemCtx::Complete);
            return VueWalkControl::Continue;
        }

        // Textarea with v-model or :value: content handling.
        if tag_name == "textarea" {
            // Check if the element has a real v-model (not just :value)
            let has_real_v_model = el.props.iter().any(|p| {
                if !p.is_directive {
                    return false;
                }
                let name = &source[p.start as usize..p.name_end as usize];
                name.starts_with("v-model")
            });

            if let Some(model_expr) = self.get_v_model_expr(el, oxc, source) {
                if has_real_v_model {
                    // v-model on textarea: always render content via _ssrInterpolate
                    // (both root and non-root — value is NOT in attrs).
                    self.ensure_push(el.tag_open.start, out);
                    self.buf.clear();
                    let _ = write!(self.buf, "<{}{}{}>", tag_name, attrs_str, sid);
                    out.add_ssr_import(SsrHelper::Interpolate);
                    let _ = write!(self.buf, "${{_ssrInterpolate({})}}", model_expr);
                    let _ = write!(self.buf, "</{}>", tag_name);
                    let el_end = self.el_end(el);
                    out.overwrite_segmented(el.tag_open.start, el_end, &self.buf, &[]);
                    self.elem_ctx.push(ElemCtx::Complete);
                    return VueWalkControl::Continue;
                } else {
                    // :value on textarea: for root path, value is in attrs (via _ssrRenderAttrs).
                    // For non-root, content is interpolated.
                    let value_in_attrs = attrs.through_render_attrs;
                    self.ensure_push(el.tag_open.start, out);
                    self.buf.clear();
                    let _ = write!(self.buf, "<{}{}{}>", tag_name, attrs_str, sid);
                    if !value_in_attrs {
                        out.add_ssr_import(SsrHelper::Interpolate);
                        let _ = write!(self.buf, "${{_ssrInterpolate({})}}", model_expr);
                    }
                    let _ = write!(self.buf, "</{}>", tag_name);
                    let el_end = self.el_end(el);
                    out.overwrite_segmented(el.tag_open.start, el_end, &self.buf, &[]);
                    self.elem_ctx.push(ElemCtx::Complete);
                    return VueWalkControl::Continue;
                }
            }
        }

        // Void element
        if is_void {
            self.ensure_push(el.tag_open.start, out);
            self.buf.clear();
            let _ = write!(self.buf, "<{}{}{}>", tag_name, attrs_str, sid);
            let el_end = self.el_end(el);
            out.overwrite_segmented(el.tag_open.start, el_end, &self.buf, &[]);
            self.elem_ctx.push(ElemCtx::Complete);
            return VueWalkControl::Continue;
        }

        // Non-void element with children.
        self.buf.clear();
        let mut anchors: [SegmentAnchor; 1] = [SegmentAnchor {
            content_offset: 0,
            length: 0,
            source_pos: 0,
            kind: AnchorKind::Point,
        }];
        let anchor_count = if !self.in_push {
            // Not inside a push — open a new push (for root or standalone)
            self.buf.push_str("_push(`<");
            self.buf.push_str(tag_name);
            let attrs_offset = self.buf.len() as u32;
            self.buf.push_str(attrs_str);
            self.buf.push_str(&sid);
            self.buf.push('>');
            self.in_push = true;
            self.elem_ctx.push(ElemCtx::OwnPush);
            if let Some(anchor) = attrs_anchor {
                anchors[0] = SegmentAnchor {
                    content_offset: attrs_offset + anchor.content_offset,
                    ..anchor
                };
                1
            } else {
                0
            }
        } else {
            // Already in a push (nested element, or root after preceding comment).
            // attrs_str still has _ssrRenderAttrs(_attrs) when is_root.
            self.buf.push('<');
            self.buf.push_str(tag_name);
            let attrs_offset = self.buf.len() as u32;
            self.buf.push_str(attrs_str);
            self.buf.push_str(&sid);
            self.buf.push('>');
            self.elem_ctx.push(ElemCtx::InParentPush);
            if let Some(anchor) = attrs_anchor {
                anchors[0] = SegmentAnchor {
                    content_offset: attrs_offset + anchor.content_offset,
                    ..anchor
                };
                1
            } else {
                0
            }
        };
        out.overwrite_segmented(
            el.tag_open.start,
            el.tag_open.end,
            &self.buf,
            &anchors[..anchor_count],
        );
        self.depth += 1;
        VueWalkControl::Continue
    }

    pub(super) fn leave_in_parent_push(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let (tid, el) = (v.tid, v.el);
        let source = self.source;
        self.depth -= 1;
        let tag_name = self.tag_name(el, source);
        let close_pos = el
            .tag_close
            .as_ref()
            .map(|tc| tc.start)
            .unwrap_or(el.tag_open.end);
        self.ensure_push(close_pos, out);

        self.buf.clear();
        if el.tag_type == TagType::Template {
            // <template> → fragment close marker unless single-element child (no v-for)
            let skip_frag = el.v_for.is_none() && self.has_single_element_child(tid);
            if !skip_frag {
                self.buf.push_str("<!--]-->");
            }
        } else {
            let _ = write!(self.buf, "</{}>", tag_name);
        }
        if let Some(ref tc) = el.tag_close {
            out.overwrite_segmented(tc.start, tc.end, &self.buf, &[]);
        } else {
            out.prepend_alloc(el.tag_open.end, &self.buf);
        }
    }

    pub(super) fn leave_own_push(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let el = v.el;
        let source = self.source;
        self.depth -= 1;
        let tag_name = self.tag_name(el, source);
        let is_now_root = self.depth == 0 && self.in_component_slots == 0;

        if self.in_push {
            self.buf.clear();
            let _ = write!(self.buf, "</{}>", tag_name);

            if is_now_root && !self.is_multi_root {
                // Single-root: close the push after the closing tag.
                // Multi-root: leave the push open so leave_template
                // can merge the fragment close marker <!--]--> into it.
                self.buf.push_str("`)\n");
                self.in_push = false;
            }

            if let Some(ref tc) = el.tag_close {
                out.overwrite_segmented(tc.start, tc.end, &self.buf, &[]);
            } else {
                out.prepend_alloc(el.tag_open.end, &self.buf);
            }
        } else {
            // Push was closed by a child (e.g., v-if/v-for).
            // For non-root elements, re-open the push for the closing tag.
            if is_now_root && !self.is_multi_root {
                self.buf.clear();
                let _ = write!(self.buf, "_push(`</{}>", tag_name);
                self.buf.push_str("`)\n");
            } else if is_now_root && self.is_multi_root {
                // Multi-root: re-open push for the closing tag but
                // leave it open so leave_template merges <!--]-->.
                let close_pos = el
                    .tag_close
                    .as_ref()
                    .map(|tc| tc.start)
                    .unwrap_or(el.tag_open.end);
                self.ensure_push(close_pos, out);
                self.buf.clear();
                let _ = write!(self.buf, "</{}>", tag_name);
            } else {
                let close_pos = el
                    .tag_close
                    .as_ref()
                    .map(|tc| tc.start)
                    .unwrap_or(el.tag_open.end);
                self.ensure_push(close_pos, out);
                self.buf.clear();
                let _ = write!(self.buf, "</{}>", tag_name);
            }

            if let Some(ref tc) = el.tag_close {
                out.overwrite_segmented(tc.start, tc.end, &self.buf, &[]);
            } else {
                out.prepend_alloc(el.tag_open.end, &self.buf);
            }
        }
    }
}
