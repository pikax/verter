//! The Svelte `clean_nodes` whitespace + run-partition core, namespace-aware.
//!
//! A faithful port of `svelte@5.56.3`'s `clean_nodes`
//! (`phases/3-transform/utils.js`) plus the `process_children` / `flush_sequence`
//! run partition (`phases/3-transform/client/visitors/shared/fragment.js`). It is
//! the SINGLE whitespace + run-partition authority the runtime HTML serializer and
//! the node-path walk both key on (the official compiler runs the same
//! `clean_nodes` everywhere).
//!
//! It carries the NAMESPACE + parent context the official cleaner threads, so the
//! `can_remove_entirely` whitespace rule (a whitespace-only interior text node is
//! removed ENTIRELY — not collapsed to `" "` — inside `select` / `tr` / `table` /
//! `tbody` / `thead` / `tfoot` / `colgroup` / `datalist`, OR in an SVG context
//! outside a `<text>` element) and the `<pre>` first-newline discard are applied.

use super::ir::{IrNode, NodeId, SvelteRuntimeIr};

/// The DOM namespace a node's children render in — the official `Namespace`
/// (`'html' | 'svg' | 'mathml'`). This is the namespace dimension the
/// `can_remove_entirely` whitespace rule reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Namespace {
    /// The default HTML namespace.
    Html,
    /// The SVG namespace (inside an `<svg>` subtree, outside a `foreignObject`).
    Svg,
    /// The MathML namespace (inside a `<math>` subtree).
    Mathml,
}

/// The cleaning context threaded into [`clean_nodes`]: the namespace the cleaned
/// nodes live in, the PARENT node's tag (for the table-family / `<pre>` /
/// `<text>` parent checks), and the inherited whitespace-preservation flag.
///
/// This mirrors the official `clean_nodes(parent, nodes, path, namespace, …,
/// preserve_whitespace, …)` parameters the runtime cleaner needs: the `parent` +
/// `path` table-family / svg-`<text>` discriminants collapse to `parent_tag` +
/// `in_svg_text` here (the runtime walks top-down, so the "any ancestor is a
/// `<text>` element" path check is the inherited `in_svg_text` flag).
#[derive(Debug, Clone, Copy)]
pub(super) struct CleanContext<'a> {
    /// The namespace the cleaned nodes render in.
    pub(super) namespace: Namespace,
    /// The parent element's tag, or `None` at the fragment / region root (the
    /// official `parent.type !== 'RegularElement'`).
    pub(super) parent_tag: Option<&'a str>,
    /// The inherited whitespace-preservation flag (`<pre>` / `<textarea>`
    /// descendant). When set, whitespace cleaning is SKIPPED.
    pub(super) preserve_ws: bool,
    /// Whether ANY ancestor (including the parent) is an SVG `<text>` element — the
    /// official `path.some((n) => n.type === 'RegularElement' && n.name === 'text')`.
    /// Inside an SVG `<text>`, whitespace is SIGNIFICANT, so `can_remove_entirely`
    /// is NOT triggered by the SVG arm.
    pub(super) in_svg_text: bool,
}

impl<'a> CleanContext<'a> {
    /// The root cleaning context for a TEMPLATE REGION's roots: HTML namespace,
    /// no parent element, no whitespace preservation, not inside an SVG `<text>`.
    /// (A region's roots are at the fragment level — never inside a `<pre>`.)
    pub(super) fn region_root() -> Self {
        Self {
            namespace: Namespace::Html,
            parent_tag: None,
            preserve_ws: false,
            in_svg_text: false,
        }
    }

    /// The cleaning context for the CHILDREN of element `tag` (whose own children
    /// are being cleaned). Computes the child namespace
    /// (`determine_namespace_for_children`), inherits/sets the
    /// whitespace-preservation flag (a `<pre>` / `<textarea>` turns it on), and
    /// tracks whether we are now inside an SVG `<text>` element.
    pub(super) fn for_children_of(self, tag: &'a str) -> Self {
        let namespace = determine_namespace_for_children(self.namespace, tag);
        Self {
            namespace,
            parent_tag: Some(tag),
            preserve_ws: self.preserve_ws || preserves_whitespace(tag),
            // Inside an SVG `<text>` element, whitespace is significant. Once set,
            // it stays set for the whole `<text>` subtree (the official `path.some`).
            in_svg_text: self.in_svg_text || (namespace == Namespace::Svg && tag == "text"),
        }
    }

    /// Whether a whitespace-only text node reduced to `" "` is removed ENTIRELY in
    /// this context (the official `can_remove_entirely`):
    ///
    /// - the SVG arm: the namespace is SVG, the PARENT is not a `<text>` element,
    ///   and no ancestor is a `<text>` element; OR
    /// - the table-family arm: the parent is one of `select` / `tr` / `table` /
    ///   `tbody` / `thead` / `tfoot` / `colgroup` / `datalist`.
    fn can_remove_entirely(&self) -> bool {
        let svg_arm = self.namespace == Namespace::Svg
            && self.parent_tag != Some("text")
            && !self.in_svg_text;
        let table_arm = matches!(
            self.parent_tag,
            Some("select" | "tr" | "table" | "tbody" | "thead" | "tfoot" | "colgroup" | "datalist")
        );
        svg_arm || table_arm
    }
}

/// Determine the namespace the children of element `tag` render in (the official
/// `determine_namespace_for_children`): a `foreignObject` resets to HTML; an SVG
/// element's children are SVG; a MathML element's children are MathML; otherwise
/// the namespace is HTML.
///
/// NOTE: the official `metadata.svg` also marks an `<a>` / `<title>` nested under
/// an SVG ancestor as SVG. The current namespace is ALREADY SVG when cleaning such
/// a child's siblings (we are inside the SVG subtree), so the `can_remove_entirely`
/// SVG arm still fires for whitespace between `<a>`/`<title>` siblings — the
/// element-name lookup here only needs to KEEP the namespace SVG for an SVG-named
/// child, which the propagation already does for a non-resetting element. An `<a>`
/// inside `<svg>` does not appear in `SVG_ELEMENTS`, so its OWN children reset to
/// HTML here — matching the browser (an SVG `<a>`'s content is SVG, but Svelte's
/// `determine_namespace_for_children` returns HTML for a non-SVG-named element, and
/// the whitespace rule keys on the namespace of the SIBLING sequence, which is the
/// parent's namespace, not the child's).
pub(super) fn determine_namespace_for_children(current: Namespace, tag: &str) -> Namespace {
    if tag == "foreignObject" {
        return Namespace::Html;
    }
    if is_svg_element(tag) {
        return Namespace::Svg;
    }
    if is_mathml_element(tag) {
        return Namespace::Mathml;
    }
    // A non-namespaced element inside an SVG / MathML subtree keeps that namespace
    // for its sibling-sequence whitespace decision is the PARENT's job; for the
    // element's OWN children the official returns HTML. We mirror that: a plain
    // element resets to HTML for its children.
    let _ = current;
    Namespace::Html
}

/// Whether an element preserves its children's whitespace (`<pre>` / `<textarea>`,
/// per official `RegularElement.js` `name === 'pre' || name === 'textarea'`).
pub(super) fn preserves_whitespace(tag: &str) -> bool {
    tag == "pre" || tag == "textarea"
}

/// The ASCII HTML-whitespace set the official `clean_nodes` treats as
/// insignificant: space, tab, CR, LF (the official `regex_not_whitespace =
/// /[^ \t\r\n]/`). Deliberately NOT `char::is_whitespace` / `str::trim`, which
/// would also fold a literal NBSP (`\u{00a0}`) and other Unicode whitespace that
/// the browser (and Svelte) treat as SIGNIFICANT content. This is the SINGLE
/// HTML-whitespace predicate used for every HTML-significance decision.
pub(super) fn is_html_ws(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\r' | '\n')
}

/// One DOM position in a CLEANED node sequence — the output of [`clean_nodes`].
///
/// Mirrors the official `process_children` / `flush_sequence` partition: a maximal
/// run of `(Text | Interpolation)` siblings becomes ONE text DOM node (a
/// [`CleanItem::TextRun`]), and every other rendered node (element / component /
/// renderable special / block / `{@html}` / …) is its own DOM node (a
/// [`CleanItem::Node`]). The index of an item IS its sibling offset for the DOM
/// walk, so a wrong run partition can no longer shift a sibling offset undetectably.
#[derive(Debug, Clone)]
pub(super) enum CleanItem {
    /// A merged text/interpolation run occupying ONE text DOM node. `text` is the
    /// SKELETON bytes: the whitespace-cleaned RAW text for a pure-text run, or a
    /// single ` ` placeholder for a run containing any interpolation (the official
    /// `flush_sequence` `push_text([{ data: ' ' }])`). `interps` are the
    /// interpolation node ids that share this text node.
    TextRun {
        /// The skeleton text bytes for this run's single text DOM node.
        text: String,
        /// The interpolation node ids merged into this run (empty for pure text).
        interps: Vec<NodeId>,
    },
    /// A non-text rendered node (element / component / renderable special / block /
    /// `{@html}` / comment) occupying its own DOM node.
    Node(NodeId),
}

/// Apply the official `clean_nodes` + `process_children`/`flush_sequence` partition
/// to a SIBLING sequence, returning the cleaned DOM-position sequence.
///
/// This is faithful to `svelte@5.56.3` (`clean_nodes`, `process_children`,
/// `flush_sequence`):
///
/// 1. Drop non-rendering / hoisted nodes (`{@const}` / `{@debug}` / `{#snippet}`
///    declaration / non-body special / …) — they never occupy a DOM position.
/// 2. Whitespace (`preserve_whitespace = false`): drop leading/trailing
///    whitespace-only text siblings; strip the leading run of the first remaining
///    text and the trailing run of the last; for each interior text, collapse its
///    leading run to `''` (if the previous sibling is a text ending in whitespace)
///    or a single ` ` (else), UNLESS the previous sibling is an interpolation, and
///    collapse its trailing run to ` ` UNLESS the next sibling is an interpolation;
///    INTERIOR whitespace is preserved verbatim. A text reduced to empty is dropped.
///    A text reduced to exactly `" "` is dropped ENTIRELY when
///    [`CleanContext::can_remove_entirely`] holds (the table-family / SVG rule).
/// 3. `<pre>` first-newline discard: if the parent is `<pre>` and the FIRST
///    remaining text node is exactly `\n` / `\r\n`, drop it (the browser would, so
///    keeping it breaks hydration). `<textarea>` does NOT do this.
/// 4. Partition the cleaned regular sequence into MAXIMAL `(Text | Interpolation)`
///    runs (one `TextRun` DOM node each) and standalone `Node`s.
///
/// When `ctx.preserve_ws` is set (inside a `<pre>` / `<textarea>` — INHERITED by
/// all descendants), the whitespace-cleaning step (2) is SKIPPED: text is kept
/// verbatim and only the run partition applies. (The `<pre>` first-newline discard
/// is a SEPARATE rule that still applies — see step 3.)
/// Whether a node is DROPPED from a cleaned sibling sequence (it never occupies a
/// DOM position): a COMMENT (the default `preserve_comments = false`), a hoisted
/// non-rendering construct (`{@const}` / `{#snippet}` declaration / `{@debug}` /
/// `{@attach}`), or a non-body special (`<svelte:head>` / `<svelte:options>` /
/// window / document / body — they render in their own region).
///
/// This is the SINGLE drop-set authority both [`clean_nodes`] (the skeleton + DOM
/// walk) and the reactive-text run reconstruction key on, so a comment cannot
/// break a text run in one path while being dropped in the other. Mirrors the
/// `svelte@5.56.3` `clean_nodes` step-1 filter.
pub(super) fn is_dropped_from_clean_sequence(node: &IrNode) -> bool {
    matches!(node, IrNode::Comment { .. })
        || super::html::is_non_body_special(node)
        || super::html::is_non_rendering_node(node)
}

pub(super) fn clean_nodes(
    ir: &SvelteRuntimeIr,
    children: &[NodeId],
    ctx: CleanContext,
) -> Vec<CleanItem> {
    clean_nodes_indexed(ir, children, ctx).0
}

/// [`clean_nodes`] plus, per emitted [`CleanItem`], the LAST original-child index it
/// covers (a `Node` item → that node's index; a `TextRun` → the index of its last
/// contributing text/interp child). The indices are MONOTONIC in item order, so a
/// document-order construct dropped from the clean sequence (a `{@debug}`) maps to its
/// emission gap by counting the items whose last index precedes it. The `items` output is
/// byte-identical to [`clean_nodes`].
pub(super) fn clean_nodes_indexed(
    ir: &SvelteRuntimeIr,
    children: &[NodeId],
    ctx: CleanContext,
) -> (Vec<CleanItem>, Vec<usize>) {
    // (1) The "regular" sequence: rendered nodes only (paired with their ORIGINAL child
    // index). The dropped set (comments, hoisted non-rendering constructs, non-body
    // specials) is the shared [`is_dropped_from_clean_sequence`] authority — mirroring
    // official `clean_nodes`.
    let mut regular: Vec<NodeId> = Vec::new();
    let mut regular_orig: Vec<usize> = Vec::new();
    for (orig, &id) in children.iter().enumerate() {
        if !is_dropped_from_clean_sequence(ir.node(id)) {
            regular.push(id);
            regular_orig.push(orig);
        }
    }

    // (2) The whitespace-cleaned text for each regular node, aligned to `regular`
    // (`None` for a non-text node OR a fully-dropped whitespace-only text). Inside a
    // `<pre>` / `<textarea>` (`preserve_ws`), whitespace cleaning is SKIPPED.
    let cleaned_text: Vec<Option<String>> = if ctx.preserve_ws {
        regular
            .iter()
            .map(|&id| match ir.node(id) {
                IrNode::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    } else {
        clean_regular_texts(ir, &regular, ctx)
    };

    // (3) `<pre>` first-newline discard: applied to the cleaned alignment, BEFORE
    // the run partition. The official discards the FIRST trimmed child when the
    // parent is `<pre>` and that child's text is exactly `\n` / `\r\n`. We locate
    // the first NON-dropped node; if it is a text whose cleaned data (which equals
    // the raw text under `preserve_ws`) is exactly `\n` / `\r\n`, drop it.
    let mut dropped_pre_newline: Option<usize> = None;
    if ctx.parent_tag == Some("pre") {
        for (idx, &id) in regular.iter().enumerate() {
            // The first node that contributes a DOM position (a non-dropped text, or
            // any non-text node) is the official `trimmed[0]`.
            match ir.node(id) {
                IrNode::Text { .. } => {
                    if let Some(t) = &cleaned_text[idx] {
                        if t == "\n" || t == "\r\n" {
                            dropped_pre_newline = Some(idx);
                        }
                        break; // the first contributing text node decides
                    }
                    // a fully-dropped whitespace text never existed — keep scanning
                }
                _ => break, // the first contributing node is not a text → no discard
            }
        }
    }

    // (4) Partition into maximal (Text | Interpolation) runs.
    let mut items: Vec<CleanItem> = Vec::new();
    let mut last_indices: Vec<usize> = Vec::new();
    let mut run_text = String::new();
    let mut run_interps: Vec<NodeId> = Vec::new();
    let mut run_active = false;
    // The last ORIGINAL-child index contributing to the in-progress run.
    let mut run_last_orig = 0usize;

    let flush = |items: &mut Vec<CleanItem>,
                 last_indices: &mut Vec<usize>,
                 run_text: &mut String,
                 run_interps: &mut Vec<NodeId>,
                 run_active: &mut bool,
                 run_last_orig: usize| {
        if !*run_active {
            return;
        }
        let item_text = if run_interps.is_empty() {
            std::mem::take(run_text)
        } else {
            // A run with any interpolation → a single ` ` placeholder; the static
            // text inside the run is dropped (the runtime materialises the node).
            run_text.clear();
            " ".to_string()
        };
        items.push(CleanItem::TextRun {
            text: item_text,
            interps: std::mem::take(run_interps),
        });
        last_indices.push(run_last_orig);
        *run_active = false;
    };

    for (idx, &id) in regular.iter().enumerate() {
        if dropped_pre_newline == Some(idx) {
            continue; // the discarded `<pre>` leading newline
        }
        match ir.node(id) {
            IrNode::Text { .. } => {
                if let Some(t) = &cleaned_text[idx] {
                    run_text.push_str(t);
                    run_active = true;
                    run_last_orig = regular_orig[idx];
                }
                // A text node that cleaned to nothing contributes no bytes but does
                // NOT break the run (it never existed as a DOM node).
            }
            IrNode::Interpolation { .. } => {
                run_interps.push(id);
                run_active = true;
                run_last_orig = regular_orig[idx];
            }
            // Any other rendered node breaks the current run and is its own DOM node.
            _ => {
                flush(
                    &mut items,
                    &mut last_indices,
                    &mut run_text,
                    &mut run_interps,
                    &mut run_active,
                    run_last_orig,
                );
                items.push(CleanItem::Node(id));
                last_indices.push(regular_orig[idx]);
            }
        }
    }
    flush(
        &mut items,
        &mut last_indices,
        &mut run_text,
        &mut run_interps,
        &mut run_active,
        run_last_orig,
    );
    (items, last_indices)
}

/// One ordered part of a reactive text RUN: ONE text node's cleaned literal text,
/// or an interpolation node. The literal text is whitespace-cleaned (the SAME
/// neighbor-aware `clean_regular_texts` the skeleton uses, so a dropped comment's
/// adjacent texts have their boundary whitespace collapsed) but NOT entity-decoded.
/// Entity decoding is the reactive-text caller's concern (`set_text` writes
/// `textContent`), and is applied PER text node BEFORE the parts are concatenated —
/// a `&amp` reference split across a dropped comment (`&amp<!--x-->;`) therefore
/// decodes the two text nodes independently (`&` + `; …`), never merging into one
/// `&amp;` reference, matching the official per-text-node decode.
#[derive(Debug, Clone)]
pub(super) enum RunTextPart {
    /// One text node's cleaned literal text (collapsed + boundary-aware; not decoded).
    Literal(String),
    /// An interpolation node id in the run.
    Interp(NodeId),
}

/// Reconstruct the ordered cleaned text-run parts (one literal per text node +
/// interp nodes) for the maximal `(Text | Interpolation)` run that CONTAINS `target`
/// among `children`, driving from the SAME drop-set + `clean_regular_texts`
/// whitespace authority [`clean_nodes`] uses. A dropped node (comment / non-rendering
/// / non-body special) never breaks the run (it is filtered out before the
/// partition), so a comment between an interpolation and trailing static text keeps
/// them in one run — matching the skeleton, which drops the same node. A REAL
/// rendered node (element / component / block / renderable tag) still breaks the run.
/// Boundary whitespace BETWEEN two texts made adjacent by a dropped comment is
/// collapsed by `clean_regular_texts`'s neighbor-aware rule (the previous text ending
/// in whitespace replaces the next text's leading whitespace with `""`).
///
/// Returns `None` if `target` is not an interpolation inside any run of `children`
/// (the caller falls back to the lone-interpolation form).
pub(super) fn cleaned_text_run_parts(
    ir: &SvelteRuntimeIr,
    children: &[NodeId],
    ctx: CleanContext,
    target: NodeId,
) -> Option<Vec<RunTextPart>> {
    // (1) The shared drop-set filter — comments / non-rendering / non-body specials
    // never occupy a DOM position, so they never break a run.
    let regular: Vec<NodeId> = children
        .iter()
        .copied()
        .filter(|&id| !is_dropped_from_clean_sequence(ir.node(id)))
        .collect();

    // (2) The same whitespace-cleaned per-node text the skeleton uses (`None` for a
    // non-text node OR a fully-dropped whitespace-only text).
    let cleaned_text: Vec<Option<String>> = if ctx.preserve_ws {
        regular
            .iter()
            .map(|&id| match ir.node(id) {
                IrNode::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    } else {
        clean_regular_texts(ir, &regular, ctx)
    };

    // (3) Partition into runs (mirroring `clean_nodes` step 4), accumulating ordered
    // parts (one literal PER text node so the caller decodes each independently)
    // instead of collapsing the run to a ` ` placeholder.
    let mut parts: Vec<RunTextPart> = Vec::new();
    let mut contains_target = false;

    for (idx, &id) in regular.iter().enumerate() {
        match ir.node(id) {
            IrNode::Text { .. } => {
                if let Some(t) = &cleaned_text[idx] {
                    parts.push(RunTextPart::Literal(t.clone()));
                }
                // A text that cleaned to nothing contributes no bytes but does NOT
                // break the run (it never existed as a DOM node).
            }
            IrNode::Interpolation { .. } => {
                parts.push(RunTextPart::Interp(id));
                if id == target {
                    contains_target = true;
                }
            }
            // A REAL rendered node breaks the current run. If the target was in THIS
            // run, return it now; otherwise reset and keep scanning.
            _ => {
                if contains_target {
                    return Some(std::mem::take(&mut parts));
                }
                parts.clear();
            }
        }
    }
    if contains_target {
        Some(parts)
    } else {
        None
    }
}

/// The whitespace-cleaned text for each node in a REGULAR (already hoisted-filtered)
/// sibling sequence, aligned to `regular` (`None` for a non-text node OR a
/// fully-dropped whitespace-only text node). A faithful port of the `clean_nodes`
/// whitespace rules (`svelte@5.56.3`, `preserve_whitespace = false`). An
/// interpolation is the official `ExpressionTag`, so whitespace adjacent to it is
/// preserved (not collapsed). A text reduced to exactly `" "` is dropped ENTIRELY
/// when [`CleanContext::can_remove_entirely`] holds (the table-family / SVG rule).
fn clean_regular_texts(
    ir: &SvelteRuntimeIr,
    regular: &[NodeId],
    ctx: CleanContext,
) -> Vec<Option<String>> {
    let is_ws_only_text = |id: NodeId| matches!(ir.node(id), IrNode::Text { text, .. } if text.chars().all(is_html_ws));
    let node_is_interp = |id: NodeId| matches!(ir.node(id), IrNode::Interpolation { .. });

    // The first/last index that is NOT a leading/trailing whitespace-only text run
    // (mirroring the `regular.shift()` / `regular.pop()` loops).
    let mut start = 0usize;
    while start < regular.len() && is_ws_only_text(regular[start]) {
        start += 1;
    }
    let mut end = regular.len();
    while end > start && is_ws_only_text(regular[end - 1]) {
        end -= 1;
    }

    let can_remove_entirely = ctx.can_remove_entirely();

    let mut out: Vec<Option<String>> = vec![None; regular.len()];
    for idx in start..end {
        let child = regular[idx];
        let IrNode::Text { text, .. } = ir.node(child) else {
            continue;
        };
        let mut s = text.clone();

        let prev = if idx > start {
            Some(regular[idx - 1])
        } else {
            None
        };
        let next = if idx + 1 < end {
            Some(regular[idx + 1])
        } else {
            None
        };

        // Leading-run handling.
        if idx == start {
            s = strip_leading_ws(&s);
        } else if !prev.is_some_and(node_is_interp) {
            let prev_text_ends_ws = prev.is_some_and(|p| {
                matches!(ir.node(p), IrNode::Text { text, .. } if text.chars().next_back().is_some_and(is_html_ws))
            });
            s = replace_leading_ws(&s, if prev_text_ends_ws { "" } else { " " });
        }

        // Trailing-run handling.
        if idx == end - 1 {
            s = strip_trailing_ws(&s);
        } else if !next.is_some_and(node_is_interp) {
            s = replace_trailing_ws(&s, " ");
        }

        // The official `node.data && (node.data !== ' ' || !can_remove_entirely)`:
        // an empty text is always dropped; a text reduced to exactly `" "` is
        // dropped ENTIRELY in a table-family / SVG context.
        out[idx] = if s.is_empty() || (s == " " && can_remove_entirely) {
            None
        } else {
            Some(s)
        };
    }
    out
}

/// Strip the leading HTML-whitespace run (`^[ \t\r\n]+`).
fn strip_leading_ws(s: &str) -> String {
    s.trim_start_matches(is_html_ws).to_string()
}

/// Strip the trailing HTML-whitespace run (`[ \t\r\n]+$`).
fn strip_trailing_ws(s: &str) -> String {
    s.trim_end_matches(is_html_ws).to_string()
}

/// Replace the leading HTML-whitespace run with `repl`.
fn replace_leading_ws(s: &str, repl: &str) -> String {
    let rest = s.trim_start_matches(is_html_ws);
    if rest.len() == s.len() {
        s.to_string()
    } else {
        format!("{repl}{rest}")
    }
}

/// Replace the trailing HTML-whitespace run with `repl`.
fn replace_trailing_ws(s: &str, repl: &str) -> String {
    let rest = s.trim_end_matches(is_html_ws);
    if rest.len() == s.len() {
        s.to_string()
    } else {
        format!("{rest}{repl}")
    }
}

/// Whether `tag` is an SVG element name (the vendored `svelte@5.56.3` `SVG_ELEMENTS`
/// set). Used by [`determine_namespace_for_children`] so the namespace propagates
/// into an `<svg>` subtree for the `can_remove_entirely` whitespace rule.
fn is_svg_element(tag: &str) -> bool {
    SVG_ELEMENTS.binary_search(&tag).is_ok()
}

/// Whether `tag` is a MathML element name (the vendored `svelte@5.56.3`
/// `MATHML_ELEMENTS` set).
fn is_mathml_element(tag: &str) -> bool {
    MATHML_ELEMENTS.binary_search(&tag).is_ok()
}

/// The vendored `svelte@5.56.3` `SVG_ELEMENTS` set, SORTED for binary search.
/// (`scripts/generate-svelte-entities.mjs` is the entity table's generator; this
/// element set is small + stable, vendored inline as a sorted literal.)
const SVG_ELEMENTS: &[&str] = &[
    "altGlyph",
    "altGlyphDef",
    "altGlyphItem",
    "animate",
    "animateColor",
    "animateMotion",
    "animateTransform",
    "circle",
    "clipPath",
    "color-profile",
    "cursor",
    "defs",
    "desc",
    "discard",
    "ellipse",
    "feBlend",
    "feColorMatrix",
    "feComponentTransfer",
    "feComposite",
    "feConvolveMatrix",
    "feDiffuseLighting",
    "feDisplacementMap",
    "feDistantLight",
    "feDropShadow",
    "feFlood",
    "feFuncA",
    "feFuncB",
    "feFuncG",
    "feFuncR",
    "feGaussianBlur",
    "feImage",
    "feMerge",
    "feMergeNode",
    "feMorphology",
    "feOffset",
    "fePointLight",
    "feSpecularLighting",
    "feSpotLight",
    "feTile",
    "feTurbulence",
    "filter",
    "font",
    "font-face",
    "font-face-format",
    "font-face-name",
    "font-face-src",
    "font-face-uri",
    "foreignObject",
    "g",
    "glyph",
    "glyphRef",
    "hatch",
    "hatchpath",
    "hkern",
    "image",
    "line",
    "linearGradient",
    "marker",
    "mask",
    "mesh",
    "meshgradient",
    "meshpatch",
    "meshrow",
    "metadata",
    "missing-glyph",
    "mpath",
    "path",
    "pattern",
    "polygon",
    "polyline",
    "radialGradient",
    "rect",
    "set",
    "solidcolor",
    "stop",
    "svg",
    "switch",
    "symbol",
    "text",
    "textPath",
    "tref",
    "tspan",
    "unknown",
    "use",
    "view",
    "vkern",
];

/// The vendored `svelte@5.56.3` `MATHML_ELEMENTS` set, SORTED for binary search.
const MATHML_ELEMENTS: &[&str] = &[
    "annotation",
    "annotation-xml",
    "maction",
    "math",
    "merror",
    "mfrac",
    "mi",
    "mmultiscripts",
    "mn",
    "mo",
    "mover",
    "mpadded",
    "mphantom",
    "mprescripts",
    "mroot",
    "mrow",
    "ms",
    "mspace",
    "msqrt",
    "mstyle",
    "msub",
    "msubsup",
    "msup",
    "mtable",
    "mtd",
    "mtext",
    "mtr",
    "munder",
    "munderover",
    "semantics",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_and_mathml_element_sets_are_sorted_for_binary_search() {
        // The vendored sets MUST stay sorted (binary_search correctness). A
        // mis-sorted entry would silently miss the namespace classification.
        assert!(
            SVG_ELEMENTS.windows(2).all(|w| w[0] < w[1]),
            "SVG_ELEMENTS must be strictly sorted"
        );
        assert!(
            MATHML_ELEMENTS.windows(2).all(|w| w[0] < w[1]),
            "MATHML_ELEMENTS must be strictly sorted"
        );
    }

    #[test]
    fn namespace_for_children_matches_official() {
        // `determine_namespace_for_children`: foreignObject → html, svg → svg,
        // math → mathml, div → html.
        assert_eq!(
            determine_namespace_for_children(Namespace::Html, "svg"),
            Namespace::Svg
        );
        assert_eq!(
            determine_namespace_for_children(Namespace::Svg, "foreignObject"),
            Namespace::Html
        );
        assert_eq!(
            determine_namespace_for_children(Namespace::Html, "math"),
            Namespace::Mathml
        );
        assert_eq!(
            determine_namespace_for_children(Namespace::Svg, "div"),
            Namespace::Html
        );
        // A nested svg-named element keeps svg.
        assert_eq!(
            determine_namespace_for_children(Namespace::Svg, "g"),
            Namespace::Svg
        );
    }

    #[test]
    fn can_remove_entirely_table_family_and_svg_arms() {
        // Table-family arm: parent in the set.
        for parent in [
            "select", "tr", "table", "tbody", "thead", "tfoot", "colgroup", "datalist",
        ] {
            let ctx = CleanContext {
                namespace: Namespace::Html,
                parent_tag: Some(parent),
                preserve_ws: false,
                in_svg_text: false,
            };
            assert!(
                ctx.can_remove_entirely(),
                "<{parent}> is a whitespace-removing parent"
            );
        }
        // A plain <div> parent does NOT remove entirely.
        let div_ctx = CleanContext {
            namespace: Namespace::Html,
            parent_tag: Some("div"),
            preserve_ws: false,
            in_svg_text: false,
        };
        assert!(!div_ctx.can_remove_entirely(), "<div> keeps a single space");
        // SVG arm: svg namespace, parent not <text>, not inside <text>.
        let svg_ctx = CleanContext {
            namespace: Namespace::Svg,
            parent_tag: Some("svg"),
            preserve_ws: false,
            in_svg_text: false,
        };
        assert!(
            svg_ctx.can_remove_entirely(),
            "svg namespace removes entirely"
        );
        // SVG <text> parent: whitespace is significant → NOT removed.
        let svg_text_ctx = CleanContext {
            namespace: Namespace::Svg,
            parent_tag: Some("text"),
            preserve_ws: false,
            in_svg_text: false,
        };
        assert!(
            !svg_text_ctx.can_remove_entirely(),
            "an SVG <text> parent keeps whitespace"
        );
        // Inside an SVG <text> subtree (in_svg_text): NOT removed even if the
        // immediate parent is a <tspan>.
        let inside_text_ctx = CleanContext {
            namespace: Namespace::Svg,
            parent_tag: Some("tspan"),
            preserve_ws: false,
            in_svg_text: true,
        };
        assert!(
            !inside_text_ctx.can_remove_entirely(),
            "inside an SVG <text> subtree keeps whitespace"
        );
    }
}
