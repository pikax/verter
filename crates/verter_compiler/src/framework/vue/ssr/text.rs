//! Text, interpolation and comment nodes.

use crate::code_transform::{AnchorKind, SegmentAnchor};
use crate::framework::vue::structure::{VueChildKind, VueChildRecord, VueCompileNodeId};
use crate::template::code_gen::shared::helpers::{has_html_entities, SsrHelper};

use super::escape::{escape_ssr_text, escape_template_literal};
use super::state::{ElemCtx, SsrEmitter, SsrOut};

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// The node's record among its parent's children after whitespace
    /// resolution; `None` when the whitespace policy dropped it.
    fn child_record(&self, node: VueCompileNodeId) -> Option<VueChildRecord> {
        let structure = self.inputs.structure;
        let tid = self.tid(node);
        let parent = self.inputs.topology.parent(tid)?;
        let siblings = structure.children(structure.compile_id(parent));
        // Records are in source order and identities are preorder indices.
        siblings
            .binary_search_by_key(&node.index(), |record| record.node.index())
            .ok()
            .map(|at| siblings[at])
    }

    /// Text and interpolation children of a component are its default slot.
    fn in_slots_without_default(&self) -> bool {
        matches!(
            self.elem_ctx.last(),
            Some(&ElemCtx::ComponentWithSlots) | Some(&ElemCtx::DynamicComponentWithSlots)
        ) && !self.default_slot_open
    }

    pub(super) fn text_node(&mut self, node: VueCompileNodeId, out: &mut SsrOut<'c>) {
        let tid = self.tid(node);
        let span = self.inputs.topology.span(tid);
        let content = self.text(tid);

        if self.in_slots_without_default() && !content.trim().is_empty() {
            self.open_default_slot(span.start, out);
        }

        let record = self.child_record(node);
        match record.map(|record| record.kind) {
            None => out.overwrite_segmented(span.start, span.end, "", &[]),
            Some(VueChildKind::WhitespaceSpace | VueChildKind::WhitespaceNewline) => {
                self.ensure_push(span.start, out);
                out.overwrite_segmented(span.start, span.end, " ", &[]);
            }
            Some(_) => {
                self.ensure_push(span.start, out);
                // Entities decoded, condensed unless preformatted, HTML
                // re-encoded, literal-escaped.
                let preserve = self
                    .inputs
                    .structure
                    .facts(node)
                    .flags
                    .has(crate::framework::vue::structure::VueNodeFlags::PRESERVE_WHITESPACE);
                let escaped = escape_ssr_text(content, preserve);
                if escaped != content {
                    out.overwrite_segmented(span.start, span.end, &escaped, &[]);
                }
            }
        }
    }

    pub(super) fn interpolation_node(&mut self, node: VueCompileNodeId, out: &mut SsrOut<'c>) {
        let tid = self.tid(node);
        let interp = match &self.inputs.topology.ast().nodes[self
            .inputs
            .topology
            .syntax(tid)
            .expect("an interpolation has syntax")
            .0]
            .kind
        {
            crate::ast::types::AstNodeKind::Interpolation(interp) => interp,
            _ => unreachable!("an interpolation node has an interpolation payload"),
        };
        let Some(oxc) = self.oxc_interpolation(tid) else {
            return;
        };

        if self.in_slots_without_default() {
            self.open_default_slot(interp.start, out);
        }

        out.add_ssr_import(SsrHelper::Interpolate);
        let expr = &self.source[interp.inner_start as usize..interp.inner_end as usize];
        self.ensure_push(interp.start, out);

        // Entities are decoded after resolution, which moves byte offsets:
        // such an interpolation is written without source anchors.
        if has_html_entities(expr) {
            let resolved = self.resolve_expr(expr, interp.inner_start, Some(oxc));
            self.buf.clear();
            self.buf.push_str("${_ssrInterpolate(");
            self.buf.push_str(&resolved);
            self.buf.push_str(")}");
            out.overwrite_segmented(interp.start, interp.end, &self.buf, &[]);
            return;
        }

        let segments = self.resolve_expr_segments(expr, interp.inner_start, oxc);
        self.buf.clear();
        self.buf.push_str("${_ssrInterpolate(");
        let wrap_offset = self.buf.len() as u32;
        self.buf.push_str(&segments.text);
        self.buf.push_str(")}");

        let mut anchors = out.anchors();
        for seg in &segments.segments {
            if let Some(source_pos) = seg.source_start {
                anchors.push(SegmentAnchor {
                    content_offset: wrap_offset + seg.generated_start,
                    length: seg.generated_end - seg.generated_start,
                    source_pos,
                    kind: AnchorKind::Exact,
                });
            }
        }
        out.overwrite_segmented(interp.start, interp.end, &self.buf, &anchors);
    }

    pub(super) fn comment_node(&mut self, node: VueCompileNodeId, out: &mut SsrOut<'c>) {
        let tid = self.tid(node);
        let span = self.inputs.topology.span(tid);
        if !self.options.comments {
            // Resolved by `leave_template`, which knows the ranges its
            // function header claims.
            self.pending_disabled_comment_removals
                .push((span.start, span.end));
            return;
        }
        if self.in_slots_without_default() {
            // Between a component's named slots: inside no slot function,
            // so it renders nowhere.
            out.overwrite(span.start, span.end, "");
            return;
        }
        let content = self.text(tid);
        self.ensure_push(span.start, out);
        let escaped = escape_template_literal(content);
        if escaped != content {
            out.overwrite_segmented(span.start, span.end, &escaped, &[]);
        }
    }
}
