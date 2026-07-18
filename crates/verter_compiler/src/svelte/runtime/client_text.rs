//! Text-run reconstruction and reactive text emission for the client backend.

use super::client::ClientEmitter;
use super::client_effect::Memoizer;
use super::client_module_frame::escape_template_literal;
use super::client_plan::{ClientNode, ClientRuntimeOp};
use super::entity_decode::decode_text_entities;
use super::ir::{IrNode, NodeId};
use super::whitespace::{cleaned_text_run_parts, CleanContext, CleanItem, RunTextPart};

impl ClientEmitter<'_> {
    /// Whether a cleaned DOM position is a TEXT node that is a PURE single
    /// interpolation (`<p>{count}</p>` → `$.child(p, true)`), the official
    /// `is_text` flag condition. A run is pure iff it is exactly ONE interpolation
    /// with NO literal text (the SAME predicate the `?? ''` text-effect decision
    /// uses); a mixed run (`x {count}` / `{a}{b}` / `{count}!`) is NOT pure. A
    /// non-text node is never `is_text`.
    pub(super) fn item_is_pure_interp_text(&self, item: &CleanItem) -> bool {
        let CleanItem::TextRun { interps, .. } = item else {
            return false;
        };
        let [interp] = interps.as_slice() else {
            return false;
        };
        // SHAPE-only query: the owning run is pure iff it is exactly one
        // interpolation with no literal text.
        matches!(
            self.owning_text_run(*interp).as_slice(),
            [RunPart::Interp(_)]
        )
    }

    /// The emitted DOM-variable name reaching a node (from the walk's `node_var` map),
    /// or the `node` fallback (unreachable on the accept path).
    pub(super) fn dom_var(&self, target: NodeId) -> String {
        self.node_var
            .get(&target)
            .cloned()
            .unwrap_or_else(|| "node".to_string())
    }

    /// Return the cooked value of an all-static interpolation text run. Source
    /// literal parts are decoded because `textContent`/`nodeValue` consume DOM
    /// text rather than HTML source. A run containing any live interpolation is
    /// not a static initialization.
    pub(super) fn static_text_run(&self, item: &CleanItem) -> Option<String> {
        let CleanItem::TextRun { interps, .. } = item else {
            return None;
        };
        let first = *interps.first()?;
        if interps
            .iter()
            .any(|node| !matches!(self.client_node(*node), ClientNode::StaticText { .. }))
        {
            return None;
        }
        let run = self.owning_text_run(first);
        if run.iter().any(|part| matches!(part, RunPart::Interp(_))) {
            return None;
        }
        Some(
            run.into_iter()
                .map(|part| match part {
                    RunPart::Literal(text) => text,
                    RunPart::Interp(_) => unreachable!(),
                })
                .collect(),
        )
    }

    /// Emit the `$.set_text(...)` call body for the reactive-text node `target`.
    ///
    /// The official `?? ''` rule is CONTENT-driven, not per-op: a text DOM node
    /// whose content is a PURE single interpolation emits `$.set_text(var, EXPR)`;
    /// a text node mixing static text with interpolation(s) emits the
    /// `` `..${EXPR ?? ''}..` `` template literal. The text node's content is
    /// recovered from the owning element's cleaned child run.
    pub(super) fn emit_set_text(
        &self,
        target: NodeId,
        memoizer: &mut Memoizer,
    ) -> super::output::MappedCode {
        let var = self
            .interp_var
            .get(&target)
            .cloned()
            .unwrap_or_else(|| "text".to_string());
        // The owning text run drives the pure-vs-mixed shape (the interp the op
        // targets is found by node id within the run).
        let interp = self.interp_node_for_text(target);
        match self.text_node_template(interp, memoizer) {
            // A pure single interpolation → the (possibly memoized) value.
            TextNodeShape::PureInterp => {
                let value = self.memoized_interp(interp, memoizer);
                value.wrapped(&format!("$.set_text({var}, "), ")")
            }
            // A mixed text run → a template literal with `?? ''` on each interp.
            TextNodeShape::Mixed(template) => template.wrapped(&format!("$.set_text({var}, "), ")"),
        }
    }

    /// The interpolation node whose text node is `target`. The reactive-text op's
    /// `target` IS the interpolation node id (see `SupportedClientIr::build_ops`),
    /// so this is the identity — kept as a named seam for the run partition.
    fn interp_node_for_text(&self, target: NodeId) -> NodeId {
        target
    }

    /// Route one interpolation through the MEMOIZER, consuming the PREPARED op
    /// carrier: a `has_call` chunk is hoisted into a `$N` placeholder (its prepared
    /// expression becomes a `() => <expr>` dep on the shared memoizer); a bare read
    /// stays inline. The rewrite, the facts, AND the legacy value wrap were
    /// computed at BUILD time (the fallible planning stage already ran), so this is
    /// a pure serialization — a non-memoized wrapped value embeds as the
    /// parenthesized sequence.
    fn memoized_interp(
        &self,
        interp: NodeId,
        memoizer: &mut Memoizer,
    ) -> super::output::MappedCode {
        let value = self.reactive_text_for(interp);
        memoizer.add_mapped(value.effect_mapped_value(), value.has_call())
    }

    /// The PREPARED reactive-text carrier for the interpolation node, from the
    /// narrow plan ops (the op's `target` is the interp node id). TOTAL over
    /// the accept path: planning prepares every interpolation through the sole
    /// authored-value entry (one `ReactiveText` op per interpolation node), so
    /// the returned value is always a plan-prepared carrier — the emitter has
    /// no way to fabricate one. A missing op is an internal routing defect and
    /// fails CLOSED with context; there is no raw-source or empty fallback.
    fn reactive_text_for(
        &self,
        interp: NodeId,
    ) -> &super::client_legacy_value::PreparedTemplateValue {
        self.plan
            .all_ops()
            .find_map(|op| match op {
                ClientRuntimeOp::ReactiveText { target, value } if NodeId(target.0) == interp => {
                    Some(value)
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                unreachable!(
                    "routing defect: interpolation node {interp:?} reached emission without \
                     a prepared ReactiveText op — planning prepares every reactive \
                     interpolation (fail closed)"
                )
            })
    }

    /// Determine the text-node template shape for an interpolation's DOM text node.
    ///
    /// Recovers the owning text RUN (the maximal text/interpolation sibling run the
    /// interpolation belongs to) and decides whether the run is a PURE single
    /// interpolation or a MIXED literal/interpolation run, building the mixed
    /// template literal (with `?? ''` per interp) from the run parts. A `has_call`
    /// interp in a mixed run is MEMOIZED into a `$N` placeholder substituted into
    /// the template literal (`${$N ?? ''}`), matching the official memoizer.
    fn text_node_template(&self, interp: NodeId, memoizer: &mut Memoizer) -> TextNodeShape {
        let run = self.owning_text_run(interp);
        // A run that is exactly one interpolation with no literal text → pure.
        if let [RunPart::Interp(_)] = run.as_slice() {
            return TextNodeShape::PureInterp;
        }
        // Mixed: build the `` `lit${expr ?? ''}lit` `` template literal.
        let mut tmpl = super::output::MappedCode::unmapped("`");
        for part in &run {
            match part {
                RunPart::Literal(text) => tmpl.push_unmapped(&escape_template_literal(text)),
                RunPart::Interp(interp_node) => {
                    let value = self.memoized_interp(*interp_node, memoizer);
                    tmpl.push_unmapped("${");
                    match self.client_node(*interp_node) {
                        ClientNode::ReactiveText {
                            coalesce: super::reactive_fold::NullishCoalesce::None,
                            ..
                        } => tmpl.push_mapped(&value),
                        ClientNode::ReactiveText {
                            coalesce: super::reactive_fold::NullishCoalesce::Parenthesized,
                            ..
                        } => {
                            tmpl.push_unmapped("(");
                            tmpl.push_mapped(&value);
                            tmpl.push_unmapped(") ?? ''");
                        }
                        ClientNode::ReactiveText { .. } => {
                            tmpl.push_mapped(&value);
                            tmpl.push_unmapped(" ?? ''");
                        }
                        _ => unreachable!("only live interpolation parts reach memoization"),
                    }
                    tmpl.push_unmapped("}");
                }
            }
        }
        tmpl.push_unmapped("`");
        TextNodeShape::Mixed(tmpl)
    }

    /// Recover the maximal text/interpolation run (in source order) that the given
    /// interpolation belongs to, as an ordered list of literal-text + interpolation
    /// NODE parts. The run is found among the interpolation's PARENT element's
    /// children (or the root scope's roots when at the top level).
    ///
    /// The whitespace + drop rules are the SHARED `clean_nodes` authority
    /// ([`cleaned_text_run_parts`]) — the SAME cleaner the skeleton/DOM walk key on,
    /// so the run reconstruction cannot disagree with the skeleton. A dropped node
    /// (comment / non-rendering / non-body special) never breaks the run; a REAL
    /// element does. Interior whitespace WITHIN a text node is preserved verbatim;
    /// the run's OUTER boundary (the leading whitespace of the first text and the
    /// trailing whitespace of the last text in the cleaned sequence) is stripped; a
    /// space ADJACENT to an interpolation is preserved (so `Hello {name}!` keeps the
    /// space after `Hello`); the boundary whitespace between two texts made adjacent
    /// by a dropped comment is collapsed by the cleaner's neighbor-aware rule.
    fn owning_text_run(&self, interp: NodeId) -> Vec<RunPart> {
        let (siblings, ctx) = self.owning_siblings_and_ctx(interp);
        // Reconstruct the run through the SHARED `clean_nodes` whitespace + drop-set
        // authority. Each literal is the cleaned text of ONE source text node (not
        // entity-decoded by the cleaner — the skeleton needs raw HTML); `set_text`
        // writes `textContent`, so each literal is decoded HERE, per text node, BEFORE
        // the template builder concatenates them (a `&amp` reference split across a
        // dropped comment decodes the two text nodes independently, never merging into
        // one `&amp;`). (Pre-fix this reconstructed from RAW children with a SECOND
        // whitespace path that treated a `<!--x-->` comment as a run break, truncating
        // the run after the interpolation.)
        if let Some(parts) = cleaned_text_run_parts(self.ir(), &siblings, ctx, interp) {
            return parts
                .into_iter()
                .map(|p| match p {
                    RunTextPart::Literal(text) => RunPart::Literal(decode_text_entities(&text)),
                    RunTextPart::Interp(node) => match self.client_node(node) {
                        ClientNode::StaticText { cooked, .. } => RunPart::Literal(cooked.clone()),
                        ClientNode::ReactiveText { .. } => RunPart::Interp(node),
                        _ => unreachable!("text-run interpolation must project to a text node"),
                    },
                })
                .collect();
        }
        // Fallback: the interpolation alone.
        if matches!(self.ir().node(interp), IrNode::Interpolation { .. }) {
            return vec![RunPart::Interp(interp)];
        }
        Vec::new()
    }

    /// The sibling node sequence (the parent element's children, or a template
    /// region's roots) that directly contains `interp`, paired with the
    /// [`CleanContext`] those siblings are cleaned in — the ancestor-folded child
    /// context of the parent element, or [`CleanContext::region_root`] at the region
    /// level. The context drives the SHARED whitespace cleaner over the run.
    fn owning_siblings_and_ctx(&self, interp: NodeId) -> (Vec<NodeId>, CleanContext<'_>) {
        // Search every element's children + every template region's roots for the
        // sequence that lists `interp` directly.
        for (idx, node) in self.ir().nodes.iter().enumerate() {
            if let IrNode::Element(el) = node {
                if el.children.contains(&interp) {
                    let ctx = self.clean_ctx_for_children_of(NodeId(idx as u32));
                    return (el.children.clone(), ctx);
                }
            }
        }
        for scope in &self.ir().template_scopes {
            if scope.roots.contains(&interp) {
                return (scope.roots.clone(), super::html::region_ctx(self.ir()));
            }
        }
        (vec![interp], super::html::region_ctx(self.ir()))
    }

    /// The [`CleanContext`] for the CHILDREN of element `parent` — the region-root
    /// context folded through every ANCESTOR element's `for_children_of` (root →
    /// parent), so inherited namespace / `<pre>` whitespace-preservation / SVG-`<text>`
    /// significance are threaded exactly as the skeleton walk threads them. The IR is
    /// a flat node list, so the parent chain is recovered by scanning for the element
    /// whose `children` list each node (the runtime IR is small).
    fn clean_ctx_for_children_of(&self, parent: NodeId) -> CleanContext<'_> {
        // Walk up to collect the ancestor ELEMENT tags (innermost-first), starting at
        // `parent` itself. Only intrinsic elements thread `for_children_of` (component
        // / special / block ancestors do not change the namespace/pre context here).
        let mut tags_inner_first: Vec<&str> = Vec::new();
        let mut current = Some(parent);
        while let Some(node_id) = current {
            if let IrNode::Element(el) = self.ir().node(node_id) {
                tags_inner_first.push(el.tag.as_str());
            }
            current = self.dom_parent_of(node_id);
        }
        // Fold root → parent so each element's `for_children_of` is applied in order.
        // The BASE carries the resolved `preserveComments` flag so a folded child
        // context drops/retains comments identically to the skeleton.
        let mut ctx = super::html::region_ctx(self.ir());
        for tag in tags_inner_first.iter().rev() {
            ctx = ctx.for_children_of(tag);
        }
        ctx
    }

    /// The DOM parent of `node` — the element / component / special whose `children`
    /// list contains it. `None` at a region root. (The flat IR carries no parent
    /// pointers; the runtime IR is small enough to scan.)
    fn dom_parent_of(&self, node: NodeId) -> Option<NodeId> {
        for (idx, candidate) in self.ir().nodes.iter().enumerate() {
            let children = match candidate {
                IrNode::Element(el) => &el.children,
                IrNode::Component(c) => &c.children,
                IrNode::Special(s) => &s.children,
                _ => continue,
            };
            if children.contains(&node) {
                return Some(NodeId(idx as u32));
            }
        }
        None
    }
}

/// The text-node template shape for an interpolation.
enum TextNodeShape {
    /// A pure single interpolation — `$.set_text(var, EXPR)`.
    PureInterp,
    /// A mixed literal/interpolation run — `$.set_text(var, `..${EXPR ?? ''}..`)`.
    Mixed(super::output::MappedCode),
}

/// One part of a text run: a literal text chunk or an interpolation NODE.
enum RunPart {
    /// A literal text chunk (whitespace-collapsed).
    Literal(String),
    /// An interpolation NODE (the reactive-text op's target node id).
    Interp(NodeId),
}
