//! The scoped-CSS renderer — a faithful port of the official `svelte@5.56.3`
//! `phases/3-transform/css/index.js` (`render_stylesheet` + its zimmerframe
//! visitors), producing the scoped stylesheet text (the official `css.code`)
//! by SOURCE-POSITION edits over the ORIGINAL component source.
//!
//! Two fidelity anchors:
//!
//! - Every mutation is a span-addressed edit (insert / update / remove at
//!   byte offsets carried by the parsed CSS AST) applied through the SHARED
//!   [`CodeTransform`]'s checked (`try_*`) operations, whose
//!   insertion-affinity chunk model carries the `magic-string` semantics the
//!   official renderer edits through — the semantics that carry output
//!   meaning: `try_append_left` content precedes `try_prepend_right` content
//!   at the same position, the content-only `try_update` preserves boundary
//!   insertions on the replaced range's first chunk (the official "closing
//!   unused comment" case), and `try_remove` clears interior insertions. No
//!   string surgery on rendered output, no reserialization — and because the
//!   render edits the one shared transform, the on-demand css SOURCE MAP is
//!   generated from the SAME chunk list that built the code
//!   (CodeTransform-SSOT).
//! - The visitor walk mirrors the official visitor set, order, and early
//!   returns: `Atrule` (keyframes rename, then stop), `Declaration`
//!   (`animation`/`animation-name` token rewrite), `Rule` (empty / unused
//!   comment-wrap, `:global { … }` block wrap), `SelectorList` (per-selector
//!   prune toggle + the per-rule specificity reset), `ComplexSelector`
//!   (`:global` removal + scope-class application), and the
//!   `PseudoClassSelector` recursion rule (argument lists are visited for
//!   `:is`/`:where`/`:has`/`:not` only).
//!
//! Verter's runtime codegen refuses dev output, so the renderer implements
//! the official non-dev branches in BOTH minify families: the EXTERNAL
//! `css.code` artifact (comment-wrapped prunes, whitespace preserved) and the
//! MINIFIED injected `$$css` payload (`state.minify = inject_styles && !dev`
//! — outright removals, per-declaration whitespace collapse, custom-property
//! values preserved). Both outputs are byte-parity with the official
//! compiler for the same input.

use oxc_allocator::Allocator;
use verter_span::Span;

use super::analyze::{is_keyframes_node, keyframes_name_token_span, remove_css_prefix};
use super::types::{
    Atrule, Block, BlockChild, Combinator, ComplexSelector, Declaration, KeyframeName, Rule,
    SelectorList, SimpleSelector, StyleChild, StyleSheet,
};
use crate::code_transform::{CodeTransform, CodeTransformError, SourceMapOptions};

/// A fail-closed render refusal: the AST or spans handed to the renderer were
/// malformed — an out-of-range or mid-character span, an inconsistent
/// metadata/node shape, an edit the chunk model cannot express. The caller
/// treats it exactly like an analysis failure (the style stays refused); a
/// partial or unscoped stylesheet is never produced, and the renderer never
/// panics the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RenderError {
    /// The span (or point offset) of the offending construct.
    pub span: Span,
}

/// The scoped render's output pair — the official `css.code` bytes plus the
/// on-demand css source map (the official `css.map`), generated from the
/// SAME shared transform whose edits produced the code, so the two can never
/// desync (CodeTransform-SSOT).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopedCssRender {
    /// The rendered scoped stylesheet (the official `css.code`).
    pub(crate) code: String,
    /// The css source-map JSON — `Some` ONLY when the render was asked for
    /// it (`want_source_map`). Its mappings point rendered css positions
    /// back to the ORIGINAL component source; `file` and `sources[0]` carry
    /// the component filename's BASENAME (the official magic-string
    /// `generateMap` naming) or `"(unknown)"` when no filename was given
    /// (svelte's validated-options default), and `sourcesContent` embeds the
    /// component source either way.
    pub(crate) source_map: Option<String>,
}

/// Render the scoped stylesheet (the official `css.code`) from the analyzed +
/// matcher-verdict-bearing AST: scope classes applied per the `scoped`
/// compound verdicts, `:global(...)` unwrapped, unused/empty rules
/// comment-pruned per the `used` selector verdicts, and local `@keyframes`
/// renamed to `<hash>-<name>` (with `animation`/`animation-name` references
/// rewritten). `keyframes` is the LOCAL rename list (the official
/// `analysis.css.keyframes`).
///
/// `minify` selects the official `state.minify = inject_styles && !dev`
/// branch family (Verter's runtime codegen refuses dev output, so the flag is
/// exactly the css output mode): the INJECTED `$$css` payload strips
/// inter-rule/declaration whitespace and REMOVES unused/empty rules and the
/// `:global {}` wrapper tokens outright, where the external artifact
/// comment-wraps them.
///
/// A malformed input fails closed with [`RenderError`] instead of panicking;
/// CSS that parsed and analyzed against the same source never takes that
/// path, so the faithful output is unchanged for every valid stylesheet.
///
/// `filename` names the component source in the on-demand css source map
/// (the official `generateMap({ source, file })` inputs — emitted as its
/// BASENAME, or `"(unknown)"` when absent); `want_source_map` is the map
/// demand — `source_map` stays `None` without it, and the rendered `code`
/// bytes are identical either way.
pub(crate) fn render_stylesheet(
    source: &str,
    stylesheet: &StyleSheet,
    hash: &str,
    keyframes: &[KeyframeName],
    minify: bool,
    filename: Option<&str>,
    want_source_map: bool,
) -> Result<ScopedCssRender, RenderError> {
    // The render-local arena the shared transform bump-allocates inserted
    // content into (the transform itself borrows, never owns).
    let allocator = Allocator::default();
    let mut renderer = Renderer {
        code: CodeTransform::new(source, &allocator),
        source,
        hash,
        selector: format!(".{hash}"),
        keyframes: keyframes.iter().map(|k| k.name.as_str()).collect(),
        rule_stack: Vec::new(),
        minify,
        failure: None,
        edit_failure: None,
    };
    if want_source_map {
        renderer.register_stylesheet_locations(stylesheet);
    }
    renderer.visit_style_children(&stylesheet.children);

    // The official minify final trim: collapse the whitespace run preceding
    // the body end. Verter applies this trim BEFORE the outside-content
    // removals below; the official compiler applies it AFTER them
    // (3-transform/css/index.js). The two edit ranges are disjoint for a valid
    // stylesheet body, so the order commutes and the emitted bytes are identical.
    if minify {
        renderer.remove_preceding_whitespace(stylesheet.span.end);
    }

    // The official final trim: everything before and after the css body.
    renderer.remove(0, stylesheet.span.start);
    renderer.remove(stylesheet.span.end, source.len() as u32);

    let Renderer {
        code,
        failure,
        edit_failure,
        ..
    } = renderer;
    if let Some(span) = failure {
        return Err(RenderError { span });
    }
    if let Some(error) = edit_failure {
        let offset = edit_failure_offset(error);
        return Err(RenderError {
            span: Span::new(offset, offset),
        });
    }
    Ok(ScopedCssRender {
        code: code.build_string(),
        source_map: want_source_map.then(|| {
            // The official map naming, mirrored exactly: svelte validates a
            // missing `options.filename` to `"(unknown)"`, then magic-string's
            // `generateMap({ source: filename, file: filename })` emits the
            // BASENAME (`file.split(/[/\\]/).pop()`) for `file` and resolves
            // `sources[0]` relative to `file`'s directory — with source ===
            // file that is exactly the basename. So BOTH fields carry
            // `"(unknown)"` without a filename and the basename with one
            // (`src/Foo.svelte` → `Foo.svelte`), while the mappings still
            // point the rendered css back to the ORIGINAL component source.
            let map_name = filename.map_or("(unknown)", map_basename);
            code.generate_map_json(SourceMapOptions {
                source: Some(map_name),
                file: Some(map_name),
                include_content: true,
            })
        }),
    })
}

/// magic-string's `file.split(/[/\\]/).pop()` — the final path segment under
/// BOTH separators (the official css map basenames `src/Foo.svelte` and
/// `src\win\Foo.svelte` to `Foo.svelte` alike).
fn map_basename(filename: &str) -> &str {
    filename.rsplit(['/', '\\']).next().unwrap_or(filename)
}

/// The offending byte offset a checked-transform refusal names — the point
/// span the fail-closed [`RenderError`] carries.
fn edit_failure_offset(error: CodeTransformError) -> u32 {
    match error {
        CodeTransformError::OutOfRange { offset, .. }
        | CodeTransformError::MidChar { offset }
        | CodeTransformError::ZeroLengthRange { offset }
        | CodeTransformError::ReplacedContentSplit { offset } => offset,
        CodeTransformError::ReversedRange { start, .. } => start,
    }
}

// ─── the visitor walk ────────────────────────────────────────────────────────

/// The parent of a visited [`SelectorList`] — the official `path.at(-1)`
/// discrimination between a rule PRELUDE and a pseudo-class ARGUMENT list.
enum ListParent<'a> {
    /// The list is the prelude of this rule.
    Rule(&'a Rule),
    /// The list is the argument of an `:is`/`:where`/`:has`/`:not`.
    PseudoArgs,
}

struct Renderer<'a> {
    /// The SHARED span-edit transform (CodeTransform-SSOT) over the ORIGINAL
    /// component source. Every mutation routes through the checked (`try_*`)
    /// operation wrappers below — never the unchecked positional API.
    code: CodeTransform<'a>,
    source: &'a str,
    hash: &'a str,
    /// `state.selector` — `.` + hash.
    selector: String,
    /// `state.keyframes` — the LOCAL keyframe names.
    keyframes: Vec<&'a str>,
    /// The ancestor-rule chain, innermost last — the facts the official
    /// visitors read through `context.path` / `metadata.parent_rule` (pushed
    /// before a rule's prelude + block descent).
    rule_stack: Vec<&'a Rule>,
    /// `state.minify` — the official `inject_styles && !dev` branch family.
    minify: bool,
    /// The first malformed-input condition the walk hit — fail-closed: the
    /// whole render refuses instead of panicking (never tripped by CSS that
    /// parsed and analyzed against the same source).
    failure: Option<Span>,
    /// The first typed refusal a checked edit returned — the transform-side
    /// fail-closed poison: once set, every subsequent edit is a no-op and
    /// `render_stylesheet` refuses the whole render (never a torn or partial
    /// stylesheet; never tripped by CSS that parsed and analyzed against the
    /// same source).
    edit_failure: Option<CodeTransformError>,
}

impl<'a> Renderer<'a> {
    /// Record the FIRST malformed-input condition; `render_stylesheet`
    /// refuses the whole render once any is set.
    fn fail(&mut self, span: Span) {
        if self.failure.is_none() {
            self.failure = Some(span);
        }
    }

    // ─── the checked-edit wrappers ───────────────────────────────────────
    //
    // Each wrapper is one `magic-string` operation of the official renderer,
    // routed through the shared transform's fail-atomic checked op. The
    // FIRST `Err` poisons the render: subsequent edits no-op and the whole
    // result is refused — matching the official chunk model's fail-closed
    // behavior on malformed offsets, without ever panicking the host.

    /// `appendLeft(index, content)` — LEFT affinity, stacking in call order.
    /// Register both boundaries of every authored CSS AST node. MagicString's
    /// boundary-resolution mode is node-driven rather than character-dense:
    /// these locations add exact tokens only at semantic boundaries and never
    /// change generated CSS bytes.
    fn register_stylesheet_locations(&mut self, stylesheet: &StyleSheet) {
        self.register_span(stylesheet.span);
        for child in &stylesheet.children {
            self.register_style_child_locations(child);
        }
    }

    fn register_style_child_locations(&mut self, child: &StyleChild) {
        match child {
            StyleChild::Rule(rule) => self.register_rule_locations(rule),
            StyleChild::Atrule(atrule) => self.register_atrule_locations(atrule),
        }
    }

    fn register_atrule_locations(&mut self, atrule: &Atrule) {
        self.register_span(atrule.span);
        self.register_span(atrule.name_span);
        self.register_span(atrule.prelude_span);
        if let Some(block) = &atrule.block {
            self.register_block_locations(block);
        }
    }

    fn register_rule_locations(&mut self, rule: &Rule) {
        self.register_span(rule.span);
        self.register_selector_list_locations(&rule.prelude);
        self.register_block_locations(&rule.block);
    }

    fn register_block_locations(&mut self, block: &Block) {
        self.register_span(block.span);
        for child in &block.children {
            match child {
                BlockChild::Declaration(declaration) => self.register_span(declaration.span),
                BlockChild::Rule(rule) => self.register_rule_locations(rule),
                BlockChild::Atrule(atrule) => self.register_atrule_locations(atrule),
            }
        }
    }

    fn register_selector_list_locations(&mut self, list: &SelectorList) {
        self.register_span(list.span);
        for complex in &list.children {
            self.register_span(complex.span);
            for relative in &complex.children {
                self.register_span(relative.span);
                if let Some(combinator) = &relative.combinator {
                    self.register_span(combinator.span);
                }
                for selector in &relative.selectors {
                    self.register_span(selector.span());
                    if let SimpleSelector::PseudoClass {
                        args: Some(arguments),
                        ..
                    } = selector
                    {
                        self.register_selector_list_locations(arguments);
                    }
                }
            }
        }
    }

    fn register_span(&mut self, span: Span) {
        if self.edit_failure.is_some() {
            return;
        }
        for offset in [span.start, span.end] {
            if let Err(error) = self.code.try_add_sourcemap_location(offset) {
                self.edit_failure = Some(error);
                return;
            }
        }
    }

    fn append_left(&mut self, index: u32, content: &str) {
        if self.edit_failure.is_some() {
            return;
        }
        if let Err(error) = self.code.try_append_left(index, content) {
            self.edit_failure = Some(error);
        }
    }

    /// `prependRight(index, content)` — RIGHT affinity, stacking in reverse
    /// call order.
    fn prepend_right(&mut self, index: u32, content: &str) {
        if self.edit_failure.is_some() {
            return;
        }
        if let Err(error) = self.code.try_prepend_right(index, content) {
            self.edit_failure = Some(error);
        }
    }

    /// `appendRight(index, content)` — RIGHT affinity, stacking in call
    /// order.
    fn append_right(&mut self, index: u32, content: &str) {
        if self.edit_failure.is_some() {
            return;
        }
        if let Err(error) = self.code.try_append_right(index, content) {
            self.edit_failure = Some(error);
        }
    }

    /// `update(start, end, content)` — content-only: the range's first chunk
    /// keeps its boundary insertions.
    fn update(&mut self, start: u32, end: u32, content: &str) {
        if self.edit_failure.is_some() {
            return;
        }
        if let Err(error) = self.code.try_update(start, end, content) {
            self.edit_failure = Some(error);
        }
    }

    /// `overwrite(start, end, content)` — clears the range's first-chunk
    /// boundary insertions.
    fn overwrite(&mut self, start: u32, end: u32, content: &str) {
        if self.edit_failure.is_some() {
            return;
        }
        if let Err(error) = self.code.try_overwrite(start, end, content) {
            self.edit_failure = Some(error);
        }
    }

    /// `remove(start, end)` — clears content AND boundary insertions of
    /// every chunk starting within the range (a zero-length range is a
    /// no-op).
    fn remove(&mut self, start: u32, end: u32) {
        if self.edit_failure.is_some() {
            return;
        }
        if let Err(error) = self.code.try_remove(start, end) {
            self.edit_failure = Some(error);
        }
    }

    /// The official minify `remove_preceding_whitespace(end)`: remove the JS
    /// `\s` whitespace run ending at `end`. An `end` past the source (a
    /// mis-anchored span) or off a char boundary fails closed — the backward
    /// scan cannot anchor.
    fn remove_preceding_whitespace(&mut self, end: u32) {
        if end as usize > self.source.len() || !self.source.is_char_boundary(end as usize) {
            self.fail(Span::new(end, end));
            return;
        }
        let mut start = end;
        while let Some(character) = self.source[..start as usize].chars().next_back() {
            if !is_js_whitespace(character) {
                break;
            }
            start -= character.len_utf8() as u32;
        }
        if start < end {
            self.remove(start, end);
        }
    }

    /// The official `is_in_global_block(path)` over the current ancestors.
    fn in_global_block(&self) -> bool {
        self.rule_stack
            .iter()
            .any(|rule| rule.metadata.is_global_block)
    }

    fn visit_style_children(&mut self, children: &'a [StyleChild]) {
        for child in children {
            match child {
                StyleChild::Rule(rule) => self.visit_rule(rule),
                StyleChild::Atrule(atrule) => self.visit_atrule(atrule),
            }
        }
    }

    fn visit_block(&mut self, block: &'a Block) {
        for child in &block.children {
            match child {
                BlockChild::Declaration(declaration) => self.visit_declaration(declaration),
                BlockChild::Rule(rule) => self.visit_rule(rule),
                BlockChild::Atrule(atrule) => self.visit_atrule(atrule),
            }
        }
    }

    /// The official `Atrule` visitor: rename a local `@keyframes` (or strip
    /// its `-global-` prefix) and do NOT transform anything within; every
    /// other at-rule recurses into its block.
    fn visit_atrule(&mut self, node: &'a Atrule) {
        if is_keyframes_node(node) {
            // The name-token anchor below is a BYTE offset off the raw at-rule
            // name span; the official anchor is `node.start + node.name.length
            // + 1` in UTF-16 units over the DECODED keyword. They agree only
            // when the `keyframes` keyword is the literal ASCII form — a
            // CSS-escaped `@\6b eyframes` (or a non-ASCII keyword) desyncs the
            // anchor, so fail closed rather than splice at a wrong offset. This
            // guards ONLY the at-rule keyword span, so a non-ASCII/escaped
            // keyframe NAME stays supported.
            let keyword_is_literal_ascii = self
                .source
                .get(node.name_span.start as usize..node.name_span.end as usize)
                .is_some_and(|keyword| keyword.is_ascii() && !keyword.contains('\\'));
            if !keyword_is_literal_ascii {
                self.fail(node.span);
                return;
            }
            // `start = node.start + node.name.length + 1`, then skip spaces —
            // the shared name-token scan (anchored on the parsed name span).
            let start = keyframes_name_token_span(self.source, node).start;
            if node.prelude.starts_with("-global-") {
                self.remove(start, start + "-global-".len() as u32);
            } else if !self.in_global_block() {
                self.prepend_right(start, &format!("{}-", self.hash));
            }
            return; // don't transform anything within
        }

        if let Some(block) = &node.block {
            self.visit_block(block);
        }
    }

    /// The official `node.start + node.property.length + 1` scan anchor: the
    /// byte position right after the property text (positionally exact — the
    /// property is the raw source token) plus ONE source char. Upstream's
    /// arithmetic is UTF-16 units, and the char after a parsed property is
    /// always its separator (the `:` or the single JS-`\s` whitespace the
    /// property scan stopped at — both BMP, one unit), so skipping one CHAR
    /// is exact; a byte `+1` would land mid-character on a Unicode space
    /// (`animation\u{a0}: spin`). `None` when the property span is malformed
    /// (out of range / mid-char) — the caller fails closed.
    fn post_property_anchor(&self, node: &Declaration) -> Option<usize> {
        let property_end = (node.span.start as usize).checked_add(node.property.len())?;
        if property_end > self.source.len() || !self.source.is_char_boundary(property_end) {
            return None;
        }
        Some(match self.source[property_end..].chars().next() {
            Some(character) => property_end + character.len_utf8(),
            // A property ending at EOF has no separator to skip (the
            // official anchor lands past the template; no scan runs).
            None => property_end,
        })
    }

    /// The official `Declaration` visitor: rewrite local-keyframe name tokens
    /// in `animation` / `animation-name` values (char-boundary scan over the
    /// ORIGINAL text — never a regex over the value); under minify, a
    /// NON-animation declaration strips its preceding whitespace and collapses
    /// the run after `property:` (custom `--` properties keep their value
    /// whitespace — the official Chromium `--foo: ;` caveat).
    fn visit_declaration(&mut self, node: &Declaration) {
        let lowered = node.property.to_lowercase();
        let property = remove_css_prefix(&lowered);
        if property != "animation" && property != "animation-name" {
            if self.minify {
                self.remove_preceding_whitespace(node.span.start);
                if !node.property.starts_with("--") {
                    let Some(start) = self.post_property_anchor(node) else {
                        // A mis-anchored declaration span cannot anchor the
                        // post-colon whitespace collapse.
                        self.fail(node.span);
                        return;
                    };
                    let mut end = start;
                    while let Some(character) = self.source[end..].chars().next() {
                        if !is_js_whitespace(character) {
                            break;
                        }
                        end += character.len_utf8();
                    }
                    if end > start {
                        self.remove(start as u32, end as u32);
                    }
                }
            }
            return;
        }

        // A scan anchor that cannot reach the declaration's value tokens
        // fails closed: with a non-empty value the loop below would silently
        // not execute and the render would return a PARTIAL stylesheet
        // (keyframes renamed, animation references unrewritten). An anchor
        // exactly AT the end with an EMPTY value has no tokens to rewrite
        // (nothing is lost); a malformed property span is refused outright.
        let Some(mut pos) = self.post_property_anchor(node) else {
            self.fail(node.span);
            return;
        };
        if pos == self.source.len() && !node.value.is_empty() {
            self.fail(node.span);
            return;
        }
        let mut name_start = pos;
        while pos < self.source.len() {
            let character = match self.source[pos..].chars().next() {
                Some(character) => character,
                None => break,
            };
            if is_css_name_boundary(character) {
                let name = &self.source[name_start..pos];
                if self.keyframes.contains(&name) {
                    self.prepend_right(name_start as u32, &format!("{}-", self.hash));
                }
                if character == ';' || character == '}' {
                    break;
                }
                name_start = pos + character.len_utf8();
            }
            pos += character.len_utf8();
        }
    }

    /// The official `Rule` visitor: empty wrap, unused wrap, `:global { … }`
    /// block wrap (body-only recursion), else full recursion. Under minify the
    /// wraps become outright REMOVALS and the rule strips its preceding
    /// whitespace (plus the run before the closing brace).
    fn visit_rule(&mut self, node: &'a Rule) {
        let in_global_block = self.in_global_block();

        if self.minify {
            if node.block.span.end == 0 {
                // A zero block end cannot carry the closing-brace arithmetic.
                self.fail(node.block.span);
                return;
            }
            self.remove_preceding_whitespace(node.span.start);
            self.remove_preceding_whitespace(node.block.span.end - 1);
        }

        // Verter's runtime codegen is non-dev: empty rules always comment out
        // (minify removes them outright).
        if is_empty(node, in_global_block) {
            if self.minify {
                self.remove(node.span.start, node.span.end);
            } else {
                self.prepend_right(node.span.start, "/* (empty) ");
                self.append_left(node.span.end, "*/");
                self.escape_comment_close(node.span);
            }
            return;
        }

        if !is_used(node) && !in_global_block {
            if self.minify {
                self.remove(node.span.start, node.span.end);
            } else {
                self.prepend_right(node.span.start, "/* (unused) ");
                self.append_left(node.span.end, "*/");
                self.escape_comment_close(node.span);
            }
            return;
        }

        if node.metadata.is_global_block {
            if let [selector] = node.prelude.children.as_slice() {
                if selector.children.len() == 1 && selector.children[0].selectors.len() == 1 {
                    // `:global {...}` — comment-wrap the wrapper (minify:
                    // remove the wrapper tokens), keep the body.
                    let block = &node.block;
                    if block.span.end == 0 {
                        // A zero end offset cannot carry the closing-brace
                        // wrap arithmetic — malformed, fail closed.
                        self.fail(block.span);
                        return;
                    }
                    let Some(after_open) = block.span.start.checked_add(1) else {
                        // A saturated open-brace offset cannot carry the wrap.
                        self.fail(block.span);
                        return;
                    };
                    if self.minify {
                        self.remove(node.span.start, after_open);
                        self.remove(block.span.end - 1, node.span.end);
                    } else {
                        self.prepend_right(node.span.start, "/* ");
                        self.append_left(after_open, "*/");
                        self.prepend_right(block.span.end - 1, "/*");
                        self.append_left(block.span.end, "*/");
                    }

                    // Don't recurse into the prelude, but visit the body.
                    self.rule_stack.push(node);
                    self.visit_block(block);
                    self.rule_stack.pop();
                    return;
                }
            }
        }

        self.rule_stack.push(node);
        self.visit_selector_list(&node.prelude, ListParent::Rule(node), &mut false, false);
        self.visit_block(&node.block);
        self.rule_stack.pop();
    }

    /// The official `SelectorList` visitor: the per-selector prune toggle
    /// (comment out contiguous unused runs) and the per-rule specificity
    /// reset. `bumped` is the enclosing specificity flag (shared with sibling
    /// argument lists); `ancestor_unused` is the official
    /// `path.find(n => n.type === 'ComplexSelector' && !n.metadata.used)`.
    fn visit_selector_list(
        &mut self,
        node: &'a SelectorList,
        parent: ListParent<'a>,
        bumped: &mut bool,
        ancestor_unused: bool,
    ) {
        // Only add comments if we're not inside a complex selector that
        // itself is unused or a global block (the global-block rule's own
        // multi-selector prelude stays eligible).
        let parent_is_global_block_rule =
            matches!(parent, ListParent::Rule(rule) if rule.metadata.is_global_block);
        if (!self.in_global_block() || (node.children.len() > 1 && parent_is_global_block_rule))
            && !ancestor_unused
        {
            let children = &node.children;
            let Some(first_child) = children.first() else {
                // A selector list with no selectors is not a parseable shape.
                self.fail(node.span);
                return;
            };
            let mut pruning = false;
            let mut prune_start = first_child.span.start;
            let mut last = prune_start;
            let mut has_previous_used = false;

            for (i, selector) in children.iter().enumerate() {
                if selector.metadata.used == pruning {
                    if pruning {
                        // Scan back to the separating comma; a span outside
                        // the source, or with no comma before it, is
                        // malformed — fail closed, never panic.
                        let bytes = self.source.as_bytes();
                        let mut comma = selector.span.start;
                        if comma as usize >= bytes.len() {
                            self.fail(selector.span);
                            return;
                        }
                        while comma > 0 && bytes[comma as usize] != b',' {
                            comma -= 1;
                        }
                        if bytes[comma as usize] != b',' {
                            self.fail(selector.span);
                            return;
                        }
                        let boundary = if has_previous_used { comma } else { comma + 1 };
                        if self.minify {
                            self.remove(prune_start, boundary);
                        } else {
                            self.append_right(boundary, "*/");
                        }
                    } else if i == 0 {
                        if self.minify {
                            prune_start = selector.span.start;
                        } else {
                            self.prepend_right(selector.span.start, "/* (unused) ");
                        }
                    } else if self.minify {
                        prune_start = last;
                    } else {
                        self.overwrite(last, selector.span.start, " /* (unused) ");
                    }

                    pruning = !pruning;
                }

                if !pruning && selector.metadata.used {
                    has_previous_used = true;
                }

                last = selector.span.end;
            }

            if pruning {
                if self.minify {
                    self.remove(prune_start, last);
                } else {
                    self.append_left(last, "*/");
                }
            }
        }

        match parent {
            // A rule-level list requires a fresh specificity bump unless an
            // ancestor rule already has local selectors (the official
            // `metadata.parent_rule` chain walk).
            ListParent::Rule(_) => {
                let mut local_bumped = self.rule_stack[..self.rule_stack.len() - 1]
                    .iter()
                    .any(|rule| rule.metadata.has_local_selectors);
                for child in &node.children {
                    self.visit_complex_selector(child, &mut local_bumped, ancestor_unused);
                }
            }
            // A pseudo-argument list keeps the enclosing bump state.
            ListParent::PseudoArgs => {
                for child in &node.children {
                    self.visit_complex_selector(child, bumped, ancestor_unused);
                }
            }
        }
    }

    /// The official `ComplexSelector` visitor: `:global` removal (with the
    /// nested `&`-prefix), mid-compound `:global(...)` stripping, and the
    /// scope-class application (first scoped compound `.hash`, later ones
    /// `:where(.hash)`, applied right-to-left within the compound).
    fn visit_complex_selector(
        &mut self,
        node: &'a ComplexSelector,
        bumped: &mut bool,
        ancestor_unused: bool,
    ) {
        let before_bumped = *bumped;

        for (index, relative_selector) in node.children.iter().enumerate() {
            if relative_selector.metadata.is_global {
                let Some(SimpleSelector::PseudoClass {
                    span: global_span,
                    args,
                    ..
                }) = relative_selector.selectors.first()
                else {
                    // `is_global` is set only when the compound leads with a
                    // `:global` pseudo-class — anything else is a malformed
                    // input; fail closed rather than panic.
                    self.fail(relative_selector.span);
                    continue;
                };
                self.remove_global_pseudo_class(
                    *global_span,
                    args.is_some(),
                    relative_selector.combinator.as_ref(),
                );

                // `node.metadata.rule?.metadata.parent_rule` — the rule owning
                // this complex selector is the innermost stack entry; its
                // parent is one level up.
                let parent_rule = (self.rule_stack.len() >= 2)
                    .then(|| self.rule_stack[self.rule_stack.len() - 2]);
                if let Some(parent_rule) = parent_rule {
                    if args.is_none() {
                        if relative_selector.combinator.is_none() {
                            // div { :global.x { ... } } becomes div { &.x { ... } }
                            self.prepend_right(global_span.start, "&");
                        }

                        // The official comma cleanup for multiple `:global`s
                        // in a selector list — its index arithmetic
                        // (`children.length === findIndex(...) - 1`) can never
                        // hold for a member index; ported as written.
                        if parent_rule.prelude.children.len() > 1
                            && node.children.len() as isize == index as isize - 1
                        {
                            let next_selector = parent_rule
                                .prelude
                                .children
                                .iter()
                                .find(|s| s.span.start > global_span.end);
                            if let Some(next_selector) = next_selector {
                                if next_selector.metadata.used {
                                    self.update(global_span.end, next_selector.span.start, "");
                                }
                            }
                        }
                    }
                }
                continue;
            }

            // Strip any `:global(...)` at the middle of the compound.
            for selector in &relative_selector.selectors {
                if let SimpleSelector::PseudoClass { span, name, args } = selector {
                    if name == "global" {
                        self.remove_global_pseudo_class(*span, args.is_some(), None);
                    }
                }
            }

            if relative_selector.metadata.scoped {
                // Skip standalone `:is`/`:where` selectors…
                if let [SimpleSelector::PseudoClass { name, .. }] =
                    relative_selector.selectors.as_slice()
                {
                    if name == "is" || name == "where" {
                        continue;
                    }
                }
                // …and any compound carrying a nesting selector.
                if relative_selector
                    .selectors
                    .iter()
                    .any(|selector| matches!(selector, SimpleSelector::Nesting { .. }))
                {
                    continue;
                }

                // For the first occurrence, a classname selector (+0-1-0
                // specificity bump); thereafter a `:where` (specificity
                // neutral).
                let modifier = if *bumped {
                    format!(":where({})", self.selector)
                } else {
                    self.selector.clone()
                };
                *bumped = true;

                let mut i = relative_selector.selectors.len();
                while i > 0 {
                    i -= 1;
                    let selector = &relative_selector.selectors[i];

                    match selector {
                        SimpleSelector::PseudoElement { name, span }
                        | SimpleSelector::PseudoClass { name, span, .. } => {
                            if name != "root" && name != "host" && i == 0 {
                                self.prepend_right(span.start, &modifier);
                            }
                            continue;
                        }
                        SimpleSelector::Type { name, span } if name == "*" => {
                            self.update(span.start, span.end, &modifier);
                        }
                        other => {
                            self.append_left(other.span().end, &modifier);
                        }
                    }

                    break;
                }
            }
        }

        // `context.next()` — the only simple selectors with visited children
        // are the recursing pseudo-classes (`:is`/`:where`/`:has`/`:not`
        // argument lists; every other pseudo-class keeps its args unvisited).
        let child_ancestor_unused = ancestor_unused || !node.metadata.used;
        for relative_selector in &node.children {
            for selector in &relative_selector.selectors {
                if let SimpleSelector::PseudoClass {
                    name,
                    args: Some(args),
                    ..
                } = selector
                {
                    if matches!(name.as_str(), "is" | "where" | "has" | "not") {
                        self.visit_selector_list(
                            args,
                            ListParent::PseudoArgs,
                            bumped,
                            child_ancestor_unused,
                        );
                    }
                }
            }
        }

        *bumped = before_bumped;
    }

    /// The official `remove_global_pseudo_class`: the argument-less form
    /// updates `:global` away (eating the preceding whitespace run when the
    /// combinator is the descendant space, so `div :global.x` becomes
    /// `div.x`); the argument form removes `:global(` and `)`.
    fn remove_global_pseudo_class(
        &mut self,
        span: Span,
        has_args: bool,
        combinator: Option<&Combinator>,
    ) {
        // The endpoint arithmetic below adds the BYTE length of the literal
        // `:global` / `:global(`; the official offsets are UTF-16 units over
        // the DECODED token, so they agree ONLY when the source carries the
        // literal ASCII keyword. A CSS-escaped `:\67 lobal` (or any non-literal
        // form) desyncs the byte anchor — fail closed rather than splice at a
        // wrong offset.
        let literal: &str = if has_args { ":global(" } else { ":global" };
        let starts_with_literal = self
            .source
            .get(span.start as usize..)
            .is_some_and(|rest| rest.starts_with(literal));
        if !starts_with_literal {
            self.fail(span);
            return;
        }
        if !has_args {
            let mut start = span.start;
            if combinator.is_some_and(|combinator| combinator.name == " ") {
                if start as usize > self.source.len()
                    || !self.source.is_char_boundary(start as usize)
                {
                    // A malformed span cannot anchor the whitespace scan.
                    self.fail(span);
                    return;
                }
                while let Some(character) = self.source[..start as usize].chars().next_back() {
                    if !is_js_whitespace(character) {
                        break;
                    }
                    start -= character.len_utf8() as u32;
                }
            }

            // update(...), not remove(...) — a closing unused comment may sit
            // at a boundary inside the range and must survive.
            self.update(start, span.start + ":global".len() as u32, "");
        } else {
            if span.end == 0 {
                // A zero end offset cannot carry the `)` removal arithmetic.
                self.fail(span);
                return;
            }
            self.remove(span.start, span.start + ":global(".len() as u32);
            self.remove(span.end - 1, span.end);
        }
    }

    /// The official `escape_comment_close`: escape every `*/` inside an
    /// existing comment within the wrapped rule, so the wrapping comment
    /// survives (byte scan with the official escape/comment state machine).
    fn escape_comment_close(&mut self, span: Span) {
        let bytes = self.source.as_bytes();
        if span.end as usize > bytes.len() {
            // The escape scan cannot walk past the source end.
            self.fail(span);
            return;
        }
        let mut escaped = false;
        let mut in_comment = false;

        let mut i = span.start as usize;
        while i < span.end as usize {
            if escaped {
                escaped = false;
            } else {
                let character = bytes[i];
                if in_comment {
                    if character == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        i += 1;
                        self.prepend_right(i as u32, "\\");
                        in_comment = false;
                    }
                } else if character == b'\\' {
                    escaped = true;
                } else if character == b'/' {
                    // The official scan consumes the next char unconditionally.
                    i += 1;
                    if bytes.get(i) == Some(&b'*') {
                        in_comment = true;
                    }
                }
            }
            i += 1;
        }
    }
}

/// The official `is_empty(rule, is_in_global_block)`.
fn is_empty(rule: &Rule, is_in_global_block: bool) -> bool {
    if rule.metadata.is_global_block {
        return rule.block.children.is_empty();
    }

    for child in &rule.block.children {
        match child {
            BlockChild::Declaration(_) => return false,
            BlockChild::Rule(child) => {
                if (is_used(child) || is_in_global_block) && !is_empty(child, is_in_global_block) {
                    return false;
                }
            }
            BlockChild::Atrule(child) => match &child.block {
                None => return false,
                Some(block) if !block.children.is_empty() => return false,
                Some(_) => {}
            },
        }
    }

    true
}

/// The official `is_used(rule)`.
fn is_used(rule: &Rule) -> bool {
    rule.prelude
        .children
        .iter()
        .any(|selector| selector.metadata.used)
}

/// The official `regex_css_name_boundary` (`/^[\s,;}]$/`).
fn is_css_name_boundary(character: char) -> bool {
    matches!(character, ',' | ';' | '}') || is_js_whitespace(character)
}

/// The JS `\s` character class (the official boundary regex and the
/// whitespace-eating scans run under JS regex semantics).
fn is_js_whitespace(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\u{B}' | '\u{C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}
