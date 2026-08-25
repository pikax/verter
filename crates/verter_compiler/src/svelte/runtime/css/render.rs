//! The scoped-CSS renderer — matches the official
//! `svelte@5.56.10` `phases/3-transform/css/index.js` (`render_stylesheet` +
//! its zimmerframe visitors), producing the scoped stylesheet text (the
//! official `css.code`) by SOURCE-POSITION edits over the ORIGINAL component
//! source, walking the shared [`StyleSyntaxIr`] tree plus the
//! analyzer/matcher's [`CssAnalysis`](super::analyze::CssAnalysis) side
//! table.
//!
//! Every mutation is a span-addressed [`CodeTransform`] edit — never string
//! surgery on rendered output, never reserialization; because the render
//! edits the one shared transform, the on-demand css SOURCE MAP is generated
//! from the SAME chunk list that built the code (CodeTransform-SSOT). The
//! official `Declaration` visitor's `animation`/`animation-name` value
//! rewrite reads the ALREADY-PARSED
//! [`ComponentValueTree`](verter_css_syntax::ComponentValueTree) token list
//! (an `Ident`-kind [`ComponentToken`](verter_css_syntax::ComponentToken) per
//! candidate name) rather than an independent byte scan over the value text
//! — the parser's own token kinds classify each candidate name. Both compare
//! the RAW (un-decoded) token text against the keyframe name list, matching
//! the official `read_value` convention (escapes RE-ENCODED, not decoded) —
//! `analyze`'s module doc explains why a decoded comparison would silently
//! stop matching an escaped keyframe name.

use oxc_allocator::Allocator;
use verter_css_syntax::{
    ComponentValue, SelectorComponentKind, SelectorPseudo, StyleDeclaration, StyleDirective,
    StyleRule, StyleStatement, StyleSyntaxIr, TokenKind,
};
use verter_span::Span;

use super::analyze::{
    atrule_name_span, component_name, is_keyframes_node, is_pseudo_class_component,
    keyframes_name_token_span, relative_steps, remove_css_prefix, CssAnalysis, KeyframeName,
};
use crate::code_transform::{CodeTransform, CodeTransformError, SourceMapOptions};

/// A fail-closed render refusal: the tree or spans handed to the renderer
/// were malformed — an out-of-range or mid-character span, an inconsistent
/// analysis-fact/tree shape, an edit the chunk model cannot express. The
/// caller treats it exactly like an analysis failure (the style stays
/// refused); a partial or unscoped stylesheet is never produced, and the
/// renderer never panics the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RenderError {
    pub span: Span,
}

/// The scoped render's output pair — the official `css.code` + on-demand css source map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopedCssRender {
    pub(crate) code: String,
    pub(crate) source_map: Option<String>,
}

/// Render the scoped stylesheet from the analyzed + matcher-verdict-bearing
/// [`CssAnalysis`]: scope-class application, `:global(...)` unwrap,
/// unused/empty comment-pruning, local `@keyframes` rename.
// Eight inputs because the shared parse tree is threaded in alongside the
// source it was parsed from: the analysis, the scope hash, the local
// keyframes, and the three output options are all independent caller
// choices. Grouping them into a struct would add a carrier without removing
// a decision, so the count is accepted rather than disguised.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_stylesheet(
    source: &str,
    tree: &StyleSyntaxIr,
    analysis: &CssAnalysis,
    hash: &str,
    keyframes: &[KeyframeName],
    minify: bool,
    filename: Option<&str>,
    want_source_map: bool,
) -> Result<ScopedCssRender, RenderError> {
    let allocator = Allocator::default();
    let body_span = Span::new(tree.source().origin(), tree.source().end());
    let mut renderer = Renderer {
        code: CodeTransform::new(source, &allocator),
        source,
        tree,
        analysis,
        hash,
        selector: format!(".{hash}"),
        keyframes: keyframes.iter().map(|k| k.name.as_str()).collect(),
        rule_stack: Vec::new(),
        minify,
        failure: None,
        edit_failure: None,
    };
    if want_source_map {
        renderer.register_span(body_span);
        renderer.register_statements_locations(tree.statements());
    }
    renderer.visit_statements(tree.statements());

    // The official minify final trim, applied BEFORE the outside-content
    // removals below (the ranges are
    // disjoint for a valid stylesheet body, so the order commutes).
    if minify {
        renderer.remove_preceding_whitespace(body_span.end);
    }

    renderer.remove(0, body_span.start);
    renderer.remove(body_span.end, source.len() as u32);

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
            let map_name = filename.map_or("(unknown)", map_basename);
            code.generate_map_json(SourceMapOptions {
                source: Some(map_name),
                file: Some(map_name),
                include_content: true,
            })
        }),
    })
}

fn map_basename(filename: &str) -> &str {
    filename.rsplit(['/', '\\']).next().unwrap_or(filename)
}

fn edit_failure_offset(error: CodeTransformError) -> u32 {
    match error {
        CodeTransformError::OutOfRange { offset, .. }
        | CodeTransformError::MidChar { offset }
        | CodeTransformError::ZeroLengthRange { offset }
        | CodeTransformError::ReplacedContentSplit { offset } => offset,
        CodeTransformError::ReversedRange { start, .. } => start,
    }
}

/// The parent of a visited selector list — the official `path.at(-1)`
/// discrimination between a rule PRELUDE and a pseudo-class ARGUMENT list.
enum ListParent<'a> {
    Rule(&'a StyleRule),
    PseudoArgs,
}

struct Renderer<'a> {
    code: CodeTransform<'a>,
    source: &'a str,
    tree: &'a StyleSyntaxIr,
    analysis: &'a CssAnalysis,
    hash: &'a str,
    /// `state.selector` — `.` + hash.
    selector: String,
    /// `state.keyframes` — the LOCAL keyframe names (RAW/un-decoded text,
    /// matching the official `read_value` convention).
    keyframes: Vec<&'a str>,
    /// The ancestor-rule chain, innermost last.
    rule_stack: Vec<&'a StyleRule>,
    minify: bool,
    failure: Option<Span>,
    edit_failure: Option<CodeTransformError>,
}

impl<'a> Renderer<'a> {
    fn fail(&mut self, span: Span) {
        if self.failure.is_none() {
            self.failure = Some(span);
        }
    }

    // ─── the checked-edit wrappers ──

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

    fn prepend_right(&mut self, index: u32, content: &str) {
        if self.edit_failure.is_some() {
            return;
        }
        if let Err(error) = self.code.try_prepend_right(index, content) {
            self.edit_failure = Some(error);
        }
    }

    fn append_right(&mut self, index: u32, content: &str) {
        if self.edit_failure.is_some() {
            return;
        }
        if let Err(error) = self.code.try_append_right(index, content) {
            self.edit_failure = Some(error);
        }
    }

    fn update(&mut self, start: u32, end: u32, content: &str) {
        if self.edit_failure.is_some() {
            return;
        }
        if let Err(error) = self.code.try_update(start, end, content) {
            self.edit_failure = Some(error);
        }
    }

    fn overwrite(&mut self, start: u32, end: u32, content: &str) {
        if self.edit_failure.is_some() {
            return;
        }
        if let Err(error) = self.code.try_overwrite(start, end, content) {
            self.edit_failure = Some(error);
        }
    }

    fn remove(&mut self, start: u32, end: u32) {
        if self.edit_failure.is_some() {
            return;
        }
        if let Err(error) = self.code.try_remove(start, end) {
            self.edit_failure = Some(error);
        }
    }

    /// Remove the run of JS-`\s` bytes immediately preceding `end`, bounded
    /// below by the style body's own start (`StyleSyntaxIr::source().origin()`
    /// — a parser fact, never an open-ended scan toward the start of the
    /// file: [`trim_trailing_js_whitespace`] stops at that bound the same
    /// way it stops at any other span's `start`).
    fn remove_preceding_whitespace(&mut self, end: u32) {
        if end as usize > self.source.len() || !self.source.is_char_boundary(end as usize) {
            self.fail(Span::new(end, end));
            return;
        }
        let lower_bound = self.tree.source().origin();
        let start = trim_trailing_js_whitespace(self.source, Span::new(lower_bound, end));
        if start < end {
            self.remove(start, end);
        }
    }

    /// The official `svelte@5.56.10` compiler's own `ComplexSelector.span`
    /// ends at the "pre-whitespace rewind point" (its hand-rolled reader
    /// stops right after the last significant token); the shared grammar's
    /// `ComplexSelector::span()` extends through trailing trivia up to the
    /// next boundary token (the comma or the rule's `{`) instead. Every edit
    /// anchor this module derives from a complex selector's END must rewind
    /// past that trailing whitespace run to land at the SAME byte offset the
    /// official compiler's own edits use — otherwise a comment-wrap boundary
    /// (`/* (unused) .. */`) lands one whitespace run too late.
    fn trimmed_selector_end(&self, span: Span) -> u32 {
        trim_trailing_js_whitespace(self.source, span)
    }

    /// A [`StyleDeclaration::name_span`] under the shared grammar covers a
    /// DIFFERENT property-name boundary than Svelte's own official
    /// `property` read for one lenient shape: the general CSS tokenizer's
    /// identifier grammar treats any codepoint ≥ U+0080 (NBSP, U+00A0,
    /// included) as a valid name-continuation character, so
    /// `animation<NBSP>:` tokenizes as ONE identifier `animation<NBSP>`. The
    /// official Svelte reader's property scan instead stops at the FIRST
    /// JS-`\s` character (which DOES include NBSP), giving the property
    /// `animation` — oracle-confirmed by `render_tests.rs`'s own
    /// `nbsp_between_property_and_colon_still_renames_animation_keyframes`
    /// regression test. Rewinding trailing JS-whitespace off the shared
    /// grammar's own `name_span` recovers the SAME boundary Svelte's reader
    /// stopped at — a bounded, parser-fact-anchored trim, never an
    /// independent property-boundary scan.
    fn svelte_property_span(&self, node: &StyleDeclaration) -> Span {
        let span = node.name_span();
        Span::new(span.start, trim_trailing_js_whitespace(self.source, span))
    }

    /// The official `is_in_global_block(path)` over the current ancestors.
    fn in_global_block(&self) -> bool {
        self.rule_stack
            .iter()
            .any(|rule| self.analysis.rule_facts(rule).is_global_block)
    }

    // ─── source-map location registration ───────────────────────────────

    fn register_statements_locations(&mut self, statements: &'a [StyleStatement]) {
        for statement in statements {
            match statement {
                StyleStatement::Rule(rule) => self.register_rule_locations(rule),
                StyleStatement::AtRule(atrule) => self.register_atrule_locations(atrule),
                StyleStatement::Declaration(decl) => self.register_span(decl.span()),
                StyleStatement::MixinOrFunction(_) | StyleStatement::Unknown(_) => {}
            }
        }
    }

    fn register_atrule_locations(&mut self, atrule: &'a StyleDirective) {
        self.register_span(atrule.span());
        self.register_span(atrule_name_span(atrule));
        self.register_span(atrule.opaque_args().span());
        if let Some(block) = atrule.body() {
            self.register_span(block.span());
            self.register_statements_locations(block.statements());
        }
    }

    fn register_rule_locations(&mut self, rule: &'a StyleRule) {
        self.register_span(rule.span());
        self.register_selector_list_locations(rule.selector_list());
        self.register_span(rule.body().span());
        self.register_statements_locations(rule.body().statements());
    }

    fn register_selector_list_locations(&mut self, list: &verter_css_syntax::SelectorList) {
        self.register_span(list.span());
        for complex in list.selectors() {
            self.register_span(complex.span());
            for (combinator, compound) in relative_steps(complex) {
                if let Some(combinator) = combinator {
                    self.register_span(combinator.span());
                }
                self.register_span(compound.span());
                for component in compound.components() {
                    self.register_span(component.span());
                    if let Some(args) = component.pseudo().and_then(SelectorPseudo::selector_list) {
                        self.register_selector_list_locations(args);
                    }
                }
            }
        }
    }

    // ─── the visitor walk ────────────────────────────────────────────────

    fn visit_statements(&mut self, statements: &'a [StyleStatement]) {
        for statement in statements {
            match statement {
                StyleStatement::Rule(rule) => self.visit_rule(rule),
                StyleStatement::AtRule(atrule) => self.visit_atrule(atrule),
                StyleStatement::Declaration(decl) => self.visit_declaration(decl),
                StyleStatement::MixinOrFunction(_) | StyleStatement::Unknown(_) => {}
            }
        }
    }

    /// The official `Atrule` visitor: rename a local `@keyframes` (or strip
    /// its `-global-` prefix) and do NOT transform anything within; every
    /// other at-rule recurses into its block.
    fn visit_atrule(&mut self, node: &'a StyleDirective) {
        if is_keyframes_node(self.source, node) {
            let name_span = atrule_name_span(node);
            // The byte-offset anchor below adds the literal `-global-`
            // prefix's BYTE length / prepends the hash right at the name
            // token — sound only when the `@keyframes` KEYWORD is literal
            // ASCII; fail closed otherwise.
            let keyword_is_literal_ascii = self
                .source
                .get(name_span.start as usize..name_span.end as usize)
                .is_some_and(|keyword| keyword.is_ascii() && !keyword.contains('\\'));
            if !keyword_is_literal_ascii {
                self.fail(node.span());
                return;
            }
            let start = keyframes_name_token_span(node).start;
            let prelude = node.prelude_text();
            if prelude.starts_with("-global-") {
                self.remove(start, start + "-global-".len() as u32);
            } else if !self.in_global_block() {
                self.prepend_right(start, &format!("{}-", self.hash));
            }
            return; // don't transform anything within
        }

        if let Some(block) = node.body() {
            self.visit_statements(block.statements());
        }
    }

    /// The `post_property_anchor` scan: the byte position right after the
    /// declaration's raw property text plus ONE source char (its separator —
    /// the `:` or a single JS-whitespace char). `None` when the property
    /// span is malformed.
    fn post_property_anchor(&self, node: &StyleDeclaration) -> Option<usize> {
        let property_end = self.svelte_property_span(node).end as usize;
        if property_end > self.source.len() || !self.source.is_char_boundary(property_end) {
            return None;
        }
        Some(match self.source[property_end..].chars().next() {
            Some(character) => property_end + character.len_utf8(),
            None => property_end,
        })
    }

    /// The official `Declaration` visitor: rewrite local-keyframe name
    /// tokens in `animation`/`animation-name` values (over the ALREADY-PARSED
    /// value token list — see the module doc for why this reads typed facts
    /// instead of an independent byte scan); under minify, a
    /// NON-animation declaration strips its preceding whitespace and
    /// collapses the run after `property:` (custom `--` properties keep
    /// their value whitespace).
    fn visit_declaration(&mut self, node: &'a StyleDeclaration) {
        let property_span = self.svelte_property_span(node);
        let raw_property = &self.source[property_span.start as usize..property_span.end as usize];
        let lowered = raw_property.to_lowercase();
        let property = remove_css_prefix(&lowered);
        if property != "animation" && property != "animation-name" {
            if self.minify {
                self.remove_preceding_whitespace(node.span().start);
                if !raw_property.starts_with("--") {
                    let Some(start) = self.post_property_anchor(node) else {
                        self.fail(node.span());
                        return;
                    };
                    // Bounded by the value's own FIRST NON-TRIVIA
                    // [`ComponentValue`]'s span start — `value().span()`
                    // itself starts right where this scan starts (just past
                    // the colon), so it cannot serve as an upper bound, and
                    // neither can the value's own first recorded
                    // [`ComponentValue`]: a leading whitespace run is ITSELF
                    // stored as a trivia `Token` value, so it would clamp
                    // the bound right back to `start`. The first value that
                    // is NOT a trivia token (a real token, a comment,
                    // whatever comes first) is where the value's own real
                    // content begins, and the run being collapsed can never
                    // cross into it. An entirely-trivia/empty value falls
                    // back to the value tree's own end.
                    let upper_bound = node
                        .value()
                        .values()
                        .iter()
                        .find(|value| {
                            !matches!(value, ComponentValue::Token(token) if token.kind().is_trivia())
                        })
                        .map_or(node.value().span().end, |value| value.span().start)
                        as usize;
                    let upper_bound = upper_bound.max(start).min(self.source.len());
                    let mut end = start;
                    while end < upper_bound {
                        let Some(character) = self.source[end..upper_bound].chars().next() else {
                            break;
                        };
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

        // Walk the ALREADY-PARSED value token list; an `Ident` token whose
        // RAW (un-decoded) text matches a local keyframe name gets the hash
        // prefix inserted right before it.
        for value in node.value().values() {
            if let ComponentValue::Token(token) = value {
                if token.kind() == TokenKind::Ident {
                    let span = token.span();
                    let text = &self.source[span.start as usize..span.end as usize];
                    if self.keyframes.contains(&text) {
                        self.prepend_right(span.start, &format!("{}-", self.hash));
                    }
                }
            }
        }
    }

    /// The official `Rule` visitor: empty wrap, unused wrap, `:global { … }`
    /// block wrap (body-only recursion), else full recursion. Under minify
    /// the wraps become outright REMOVALS and the rule strips its preceding
    /// whitespace (plus the run before the closing brace).
    fn visit_rule(&mut self, node: &'a StyleRule) {
        let in_global_block = self.in_global_block();

        if self.minify {
            if node.body().span().end == 0 {
                self.fail(node.body().span());
                return;
            }
            self.remove_preceding_whitespace(node.span().start);
            self.remove_preceding_whitespace(node.body().span().end - 1);
        }

        if self.is_empty(node, in_global_block) {
            if self.minify {
                self.remove(node.span().start, node.span().end);
            } else {
                self.prepend_right(node.span().start, "/* (empty) ");
                self.append_left(node.span().end, "*/");
                self.escape_comment_close(node.span());
            }
            return;
        }

        if !self.is_used(node) && !in_global_block {
            if self.minify {
                self.remove(node.span().start, node.span().end);
            } else {
                self.prepend_right(node.span().start, "/* (unused) ");
                self.append_left(node.span().end, "*/");
                self.escape_comment_close(node.span());
            }
            return;
        }

        if self.analysis.rule_facts(node).is_global_block {
            let selectors = node.selector_list().selectors();
            if let [selector] = selectors {
                let steps = relative_steps(selector);
                if steps.len() == 1 && steps[0].1.components().len() == 1 {
                    // `:global {...}` — comment-wrap the wrapper (minify:
                    // remove the wrapper tokens), keep the body.
                    let block = node.body();
                    if block.span().end == 0 {
                        self.fail(block.span());
                        return;
                    }
                    let Some(after_open) = block.span().start.checked_add(1) else {
                        self.fail(block.span());
                        return;
                    };
                    if self.minify {
                        self.remove(node.span().start, after_open);
                        self.remove(block.span().end - 1, node.span().end);
                    } else {
                        self.prepend_right(node.span().start, "/* ");
                        self.append_left(after_open, "*/");
                        self.prepend_right(block.span().end - 1, "/*");
                        self.append_left(block.span().end, "*/");
                    }

                    // Don't recurse into the prelude, but visit the body.
                    self.rule_stack.push(node);
                    self.visit_statements(block.statements());
                    self.rule_stack.pop();
                    return;
                }
            }
        }

        self.rule_stack.push(node);
        self.visit_selector_list(
            node.selector_list(),
            ListParent::Rule(node),
            &mut false,
            false,
        );
        self.visit_statements(node.body().statements());
        self.rule_stack.pop();
    }

    /// The official `is_empty(rule, is_in_global_block)`.
    fn is_empty(&self, rule: &'a StyleRule, is_in_global_block: bool) -> bool {
        if self.analysis.rule_facts(rule).is_global_block {
            return rule.body().statements().is_empty();
        }
        for statement in rule.body().statements() {
            match statement {
                StyleStatement::Declaration(_) => return false,
                StyleStatement::Rule(child) => {
                    if (self.is_used(child) || is_in_global_block)
                        && !self.is_empty(child, is_in_global_block)
                    {
                        return false;
                    }
                }
                StyleStatement::AtRule(child) => match child.body() {
                    None => return false,
                    Some(block) if !block.statements().is_empty() => return false,
                    Some(_) => {}
                },
                StyleStatement::MixinOrFunction(_) | StyleStatement::Unknown(_) => {}
            }
        }
        true
    }

    /// The official `is_used(rule)`.
    fn is_used(&self, rule: &'a StyleRule) -> bool {
        rule.selector_list()
            .selectors()
            .iter()
            .any(|complex| self.analysis.complex_facts(complex).used)
    }

    /// The official `SelectorList` visitor: the per-selector prune toggle
    /// (comment out contiguous unused runs) and the per-rule specificity
    /// reset.
    fn visit_selector_list(
        &mut self,
        node: &'a verter_css_syntax::SelectorList,
        parent: ListParent<'a>,
        bumped: &mut bool,
        ancestor_unused: bool,
    ) {
        let parent_is_global_block_rule = matches!(parent, ListParent::Rule(rule) if self.analysis.rule_facts(rule).is_global_block);
        let children = node.selectors();
        if (!self.in_global_block() || (children.len() > 1 && parent_is_global_block_rule))
            && !ancestor_unused
        {
            let Some(first_child) = children.first() else {
                self.fail(node.span());
                return;
            };
            let mut pruning = false;
            let mut prune_start = first_child.span().start;
            let mut last = prune_start;
            // The delimiter comma between the PREVIOUS selector and the one
            // about to be visited: the shared grammar's own `ComplexSelector`
            // span is parser-defined to END exactly at the next boundary
            // token (the comma, or the rule's `{`) — see `trimmed_selector_end`'s
            // doc above — so the RAW (untrimmed) span end of the preceding
            // selector IS the delimiter comma's start byte, a parser fact,
            // never a re-scan of the source for it (which could otherwise
            // land on a comma inside an intervening `/* x,y */` comment).
            let mut last_delimiter_comma = prune_start;
            let mut has_previous_used = false;

            for (i, selector) in children.iter().enumerate() {
                let used = self.analysis.complex_facts(selector).used;
                if used == pruning {
                    if pruning {
                        let comma = last_delimiter_comma;
                        let boundary = if has_previous_used { comma } else { comma + 1 };
                        if self.minify {
                            self.remove(prune_start, boundary);
                        } else {
                            self.append_right(boundary, "*/");
                        }
                    } else if i == 0 {
                        if self.minify {
                            prune_start = selector.span().start;
                        } else {
                            self.prepend_right(selector.span().start, "/* (unused) ");
                        }
                    } else if self.minify {
                        prune_start = last;
                    } else {
                        self.overwrite(last, selector.span().start, " /* (unused) ");
                    }

                    pruning = !pruning;
                }

                if !pruning && used {
                    has_previous_used = true;
                }

                last = self.trimmed_selector_end(selector.span());
                last_delimiter_comma = selector.span().end;
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
            ListParent::Rule(_) => {
                let mut local_bumped = self.rule_stack[..self.rule_stack.len() - 1]
                    .iter()
                    .any(|rule| self.analysis.rule_facts(rule).has_local_selectors);
                for child in children {
                    self.visit_complex_selector(child, &mut local_bumped, ancestor_unused);
                }
            }
            ListParent::PseudoArgs => {
                for child in children {
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
        node: &'a verter_css_syntax::ComplexSelector,
        bumped: &mut bool,
        ancestor_unused: bool,
    ) {
        let before_bumped = *bumped;
        let steps = relative_steps(node);

        for (index, (combinator, compound)) in steps.iter().enumerate() {
            let compound_facts = self.analysis.compound_facts(compound);
            let components = compound.components();

            if compound_facts.is_global {
                let Some(first) = components.first() else {
                    self.fail(compound.span());
                    continue;
                };
                if !is_pseudo_class_component(first)
                    || component_name(self.source, first).as_deref() != Some("global")
                {
                    self.fail(compound.span());
                    continue;
                }
                let global_span = first.span();
                let args = first.pseudo().and_then(SelectorPseudo::selector_list);
                self.remove_global_pseudo_class(global_span, args.is_some(), *combinator);

                let parent_rule = (self.rule_stack.len() >= 2)
                    .then(|| self.rule_stack[self.rule_stack.len() - 2]);
                if let Some(parent_rule) = parent_rule {
                    if args.is_none() {
                        if combinator.is_none() {
                            self.prepend_right(global_span.start, "&");
                        }
                        let owner_selectors = parent_rule.selector_list().selectors();
                        if owner_selectors.len() > 1 && steps.len() as isize == index as isize - 1 {
                            let next_selector = owner_selectors
                                .iter()
                                .find(|s| s.span().start > global_span.end);
                            if let Some(next_selector) = next_selector {
                                if self.analysis.complex_facts(next_selector).used {
                                    self.update(global_span.end, next_selector.span().start, "");
                                }
                            }
                        }
                    }
                }
                continue;
            }

            // Strip any `:global(...)` at the middle of the compound.
            for component in components {
                if is_pseudo_class_component(component)
                    && component_name(self.source, component).as_deref() == Some("global")
                {
                    let args = component.pseudo().and_then(SelectorPseudo::selector_list);
                    self.remove_global_pseudo_class(component.span(), args.is_some(), None);
                }
            }

            if compound_facts.scoped {
                // Skip standalone `:is`/`:where` selectors…
                if let [only] = components {
                    if is_pseudo_class_component(only) {
                        let name = component_name(self.source, only);
                        if matches!(name.as_deref(), Some("is") | Some("where")) {
                            continue;
                        }
                    }
                }
                // …and any compound carrying a nesting selector.
                if components
                    .iter()
                    .any(|component| component.kind() == SelectorComponentKind::Nesting)
                {
                    continue;
                }

                let modifier = if *bumped {
                    format!(":where({})", self.selector)
                } else {
                    self.selector.clone()
                };
                *bumped = true;

                let mut i = components.len();
                while i > 0 {
                    i -= 1;
                    let component = &components[i];

                    match component.kind() {
                        SelectorComponentKind::PseudoElement
                        | SelectorComponentKind::PseudoClass
                        | SelectorComponentKind::FunctionalPseudo => {
                            let name = component_name(self.source, component);
                            if !matches!(name.as_deref(), Some("root") | Some("host")) && i == 0 {
                                self.prepend_right(component.span().start, &modifier);
                            }
                            continue;
                        }
                        // The universal selector `*` is a `Type` component
                        // with NO name span (its token is `Delim`, not
                        // `Ident` — see `match::type_selector_name`'s
                        // doc for the same grammar-shape note).
                        SelectorComponentKind::Type if component.name_span().is_none() => {
                            self.update(component.span().start, component.span().end, &modifier);
                        }
                        _ => {
                            self.append_left(component.span().end, &modifier);
                        }
                    }

                    break;
                }
            }
        }

        // `context.next()` — only `:is`/`:where`/`:has`/`:not` argument lists
        // are visited.
        let complex_used = self.analysis.complex_facts(node).used;
        let child_ancestor_unused = ancestor_unused || !complex_used;
        for (_, compound) in &steps {
            for component in compound.components() {
                if is_pseudo_class_component(component) {
                    let name = component_name(self.source, component);
                    if matches!(
                        name.as_deref(),
                        Some("is") | Some("where") | Some("has") | Some("not")
                    ) {
                        if let Some(args) =
                            component.pseudo().and_then(SelectorPseudo::selector_list)
                        {
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
        combinator: Option<&verter_css_syntax::SelectorCombinator>,
    ) {
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
            if let Some(combinator) = combinator {
                if combinator.kind() == verter_css_syntax::CombinatorKind::Descendant {
                    if start as usize > self.source.len()
                        || !self.source.is_char_boundary(start as usize)
                    {
                        self.fail(span);
                        return;
                    }
                    // Bounded by the combinator's OWN span — a Descendant
                    // combinator's frame is exactly the trivia run joining
                    // the two compounds, so the whitespace this strips can
                    // never reach past it into the preceding compound.
                    start = trim_trailing_js_whitespace(
                        self.source,
                        Span::new(combinator.span().start, start),
                    );
                }
            }
            self.update(start, span.start + ":global".len() as u32, "");
        } else {
            if span.end == 0 {
                self.fail(span);
                return;
            }
            self.remove(span.start, span.start + ":global(".len() as u32);
            self.remove(span.end - 1, span.end);
        }
    }

    /// The official `escape_comment_close`: escape every `*/` inside an
    /// existing comment within the wrapped rule, so the wrapping comment
    /// survives.
    fn escape_comment_close(&mut self, span: Span) {
        let bytes = self.source.as_bytes();
        if span.end as usize > bytes.len() {
            self.fail(span);
            return;
        }
        // Walk the parser's own retained comment-span inventory
        // (`StyleSyntaxIr::comment_spans_in`) instead of re-lexing `span`'s
        // bytes for comment/string state: a hand-rolled scan cannot tell a
        // real comment from a string literal that merely CONTAINS `/* … */`
        // bytes, and would wrongly escape a `*/` inside such a string.
        for comment in self.tree.comment_spans_in(span) {
            let end = comment.end as usize;
            if end >= 2 && bytes[end - 2..end] == *b"*/" {
                self.prepend_right(comment.end - 1, "\\");
            }
        }
    }
}

/// Rewind `span.end` past a trailing run of JS-`\s` characters, bounded at
/// `span.start` — the "pre-whitespace rewind point" the official compiler's
/// own hand-rolled reader always stopped at (see
/// [`Renderer::trimmed_selector_end`]'s doc). A malformed (out-of-range /
/// mid-char) `span.end` returns unchanged.
fn trim_trailing_js_whitespace(source: &str, span: Span) -> u32 {
    let mut end = span.end;
    if end as usize > source.len() || !source.is_char_boundary(end as usize) {
        return end;
    }
    while let Some(character) = source[..end as usize].chars().next_back() {
        if !is_js_whitespace(character) || end <= span.start {
            break;
        }
        end -= character.len_utf8() as u32;
    }
    end
}

/// The JS `\s` character class.
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
