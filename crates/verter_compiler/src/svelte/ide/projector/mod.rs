//! The in-place Svelte IDE TSX projector.
//!
//! Drives ONE [`CodeTransform`] over the original `.svelte` source. Structural
//! connective tissue (block keywords, tag syntax, directive prefixes) is
//! overwritten in place; expression interiors (`{expr}`, block conditions,
//! script bodies) stay as Original chunks so they keep their source spans and
//! map back token-precisely. The ambient prelude is the module INTRO (always
//! the leading bytes — the `@jsxImportSource` pragma must lead; unmapped). The
//! whole template is wrapped in a `__verter_render` scope function, with
//! snippet declarators and declaration-tag bindings hoisted to the top of that
//! scope in source order (D-ap) via CodeTransform MOVE operations.

use oxc_allocator::Allocator;

use verter_span::Span;

use crate::code_transform::{CodeTransform, SourceMapOptions};

use super::await_scan::scan_await_positions;
use super::bind_contract::{lookup_bind_contract, BindContract, BindDirection};
use super::emit::{is_css_custom_property, UnsupportedKind};
use super::prelude::render_prelude;
use crate::svelte::parser::{
    ParsedSvelte, SvelteAttribute, SvelteAttributeKind, SvelteAttributeValue, SvelteBlock,
    SvelteBlockKind, SvelteClauseKind, SvelteDirectiveKind, SvelteElement, SvelteElementKind,
    SvelteNode, SvelteScript, SvelteSpecialKind, SvelteTag, SvelteTagKind,
};

/// The `bind:` directive projection (F4/F5) — a continuation of the
/// `TemplateProjector` impl, extracted for file size.
mod bind;

/// A typed-unsupported diagnostic the projector emitted for an OUT-OF-SCOPE
/// matrix construct (the construct was still void-checked, never dropped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvelteIdeUnsupportedDiagnostic {
    /// The machine-stable code (e.g. `svelte-await-experimental`).
    pub code: &'static str,
    /// A human-readable message.
    pub message: String,
    /// The offending span in the ORIGINAL source.
    pub span: Span,
}

/// The rendered Svelte IDE projection.
#[derive(Debug, Clone)]
pub struct SvelteIdeProjection {
    /// The generated TSX.
    pub code: String,
    /// The JSON source map (empty when source maps are skipped).
    pub source_map: String,
    /// Typed-unsupported diagnostics for OUT-OF-SCOPE constructs.
    pub diagnostics: Vec<SvelteIdeUnsupportedDiagnostic>,
}

/// Project a parsed Svelte component into the IDE TSX artifact.
///
/// `filename` identifies the source for the map; `skip_source_map` produces an
/// empty `source_map`.
#[must_use]
pub fn project_svelte_ide(
    source: &str,
    parsed: &ParsedSvelte,
    filename: Option<&str>,
    skip_source_map: bool,
) -> SvelteIdeProjection {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new(source, &allocator);
    let mut diagnostics = Vec::new();

    // The first template-markup byte: the render fragment opens here.
    let first_template = parsed
        .template
        .iter()
        .filter_map(node_span)
        .map(|s| s.start)
        .min();

    // 1) Strip the `<script>` tags. A script BEFORE the first markup byte keeps
    //    its body in place (top-level, mapped). A script that falls AT/AFTER the
    //    first markup byte (interleaved or trailing) would land INSIDE the render
    //    fragment, so its body is MOVED above the render fn (still mapped). All
    //    `<style>` blocks are opaque and removed wholesale.
    for script in [
        parsed.module_script.as_ref(),
        parsed.instance_script.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        strip_script_tags(&mut ct, Some(script), first_template);
    }
    for style in &parsed.styles {
        remove_span(&mut ct, style_full_span(style));
    }

    // 2) The unmapped prelude as the module INTRO — always emitted FIRST in
    //    output (the `@jsxImportSource` pragma MUST be the leading bytes). Using
    //    the intro (not `prepend_left(0)`) keeps the pragma ahead of any content
    //    a trailing-script MOVE relocates to the top.
    let prelude = render_prelude();
    ct.prepend(&prelude);

    // 3) The render scope function wrapping the template. With trailing/
    //    interleaved scripts MOVED above the render fn, the markup is contiguous
    //    from the first template byte to the source end, so the render fragment
    //    wraps the WHOLE markup (including element close tags the AST does not
    //    span individually).
    let region = first_template.map(|first| (first, source.len() as u32));
    let mut projector = TemplateProjector {
        ct: &mut ct,
        source,
        diagnostics: &mut diagnostics,
        snippet_moves: Vec::new(),
        decl_moves: Vec::new(),
    };
    projector.project_template(&parsed.template, region);

    // Await-experimental (D-bg) — instance/module SCRIPT positions. The script
    // body is hoisted/kept verbatim (its inner type errors + hover survive), but
    // an `await` at instance-script top level OR inside a `$derived(...)` /
    // `$derived.by(...)` arg in the script is out-of-scope v1 — record one typed
    // diagnostic per await-bearing position (the markup positions are handled in
    // the projector walk).
    for content in [parsed.module_content(), parsed.instance_content()]
        .into_iter()
        .flatten()
    {
        let text = &source[content.start as usize..content.end as usize];
        for at in scan_await_positions(text) {
            let span = Span::new(
                content.start + at,
                content.start + at + "await".len() as u32,
            );
            diagnostics.push(SvelteIdeUnsupportedDiagnostic {
                code: UnsupportedKind::AwaitExperimental.code(),
                message: UnsupportedKind::AwaitExperimental.message().to_string(),
                span,
            });
        }
    }

    let code = ct.build_string();
    let source_map = if skip_source_map {
        String::new()
    } else {
        let opts = SourceMapOptions {
            source: filename,
            file: None,
            include_content: true,
        };
        ct.generate_map_json(opts)
    };

    SvelteIdeProjection {
        code,
        source_map,
        diagnostics,
    }
}

/// The full span of a `<script>` block (open tag through `</script>`).
fn script_full_span(script: &SvelteScript) -> Span {
    // The parser records the open-tag span and the content span; the close tag
    // follows the content. We reconstruct the full removable range as
    // [tag_open.start, content.end + len("</script>")] when content exists,
    // else just the open tag (self-closed / empty).
    let start = script.tag_open.start;
    let end = match script.content {
        Some(content) => content.end,
        None => script.tag_open.end,
    };
    Span::new(start, end)
}

/// The full span of a `<style>` block — open tag through `</style>` close.
///
/// Component `<style>` blocks are opaque (CSS domain) and stripped wholesale,
/// so the removal MUST cover the trailing `</style>` (the parser's content
/// span excludes it) — otherwise a raw `</style>` leaks into the projected
/// module.
fn style_full_span(style: &crate::svelte::parser::SvelteStyle) -> Span {
    let start = style.tag_open.start;
    let end = match style.content {
        Some(content) => content.end + "</style>".len() as u32,
        None => style.tag_open.end,
    };
    Span::new(start, end)
}

/// Remove a span (overwrite with whitespace-equivalent nothing).
fn remove_span(ct: &mut CodeTransform, span: Span) {
    if span.start < span.end {
        ct.remove(span.start, span.end);
    }
}

/// Strip a script block's open + close tags.
///
/// `<script ...>BODY</script>` → the open tag and the close tag are removed.
/// When the script sits BEFORE the first markup byte (`first_template`), the
/// `BODY` stays in place (top-level, mapped). When it sits AT/AFTER the first
/// markup byte (interleaved or trailing), the `BODY` would land inside the
/// render fragment, so it is MOVED to offset 0 (above the render fn; still
/// mapped via `move_*`) — keeping the projected module a valid top-level
/// script + one render function.
fn strip_script_tags(
    ct: &mut CodeTransform,
    script: Option<&SvelteScript>,
    first_template: Option<u32>,
) {
    let Some(script) = script else { return };
    match script.content {
        Some(content) => {
            // Remove the open tag (`<script ...>`) up to the content start.
            if script.tag_open.start < content.start {
                ct.remove(script.tag_open.start, content.start);
            }
            // Remove the close tag `</script>` after the content.
            let close_end = content.end + "</script>".len() as u32;
            ct.remove(content.end, close_end);

            // If the script body falls AT/AFTER the first markup byte, move it
            // to the render-fragment open point (`first_template`) so it lands
            // ABOVE the render fn header (the same hoist anchor the snippet /
            // declaration moves use) rather than inside the JSX fragment — and
            // crucially AFTER the prelude pragma, never before it.
            if let Some(first) = first_template {
                if script.tag_open.start >= first && content.start < content.end {
                    ct.move_wrapped(content.start, content.end, first, "\n", "\n;");
                }
            }
        }
        None => {
            // Empty/self-closed script — remove the whole open tag.
            remove_span(ct, script_full_span(script));
        }
    }
}

/// The recursive template projector.
struct TemplateProjector<'ct, 'a> {
    ct: &'ct mut CodeTransform<'a>,
    source: &'a str,
    diagnostics: &'ct mut Vec<SvelteIdeUnsupportedDiagnostic>,
    /// Snippet declarator MOVE requests collected during the walk, applied
    /// after the body so they hoist to the TOP of the scope (D-ap order).
    snippet_moves: Vec<SnippetMove>,
    /// Declaration-tag (`{const}`/`{let}`/`{@const}`) MOVE requests, hoisted to
    /// the render scope top so the declared binding is a real statement VISIBLE
    /// to sibling references (D-ap sibling-run scope) — an in-place IIFE would
    /// scope the binding locally and a following sibling could not see it.
    decl_moves: Vec<DeclMove>,
}

/// A snippet declarator to hoist to the top of its scope function.
struct SnippetMove {
    /// The full `{#snippet}` block span.
    block_span: Span,
    /// The snippet name.
    name: String,
    /// The params span (excludes parens), if any.
    params: Option<Span>,
    /// The body span (between the snippet head and `{/snippet}`).
    body_span: Span,
}

/// A declaration-tag binding to hoist to the render scope top.
struct DeclMove {
    /// `true` for `let`, `false` for `const`.
    is_let: bool,
    /// The inner `x = e` declaration span (kept mapped when moved).
    inner_span: Span,
}

impl TemplateProjector<'_, '_> {
    /// Project the whole template into the render scope function.
    ///
    /// `region` is the markup byte range `[start, end)` the render fragment
    /// wraps — every byte outside the script/style blocks.
    fn project_template(&mut self, nodes: &[SvelteNode], region: Option<(u32, u32)>) {
        let Some((first, last)) = region else {
            // No template — emit an empty render function so the file is still
            // a valid module and the prelude's checkers are referenced.
            self.ct
                .append("\n;function __verter_render() { return (<></>); }\nexport {};\n");
            return;
        };

        // Snippet declarators are hoisted to MODULE scope, ABOVE the render
        // fragment's `return` (Svelte snippets are visible to preceding
        // siblings; an in-place `const` would TDZ-error a preceding `{@render}`
        // under the clean-type-check gate, D-ap). Module-scope `const`
        // declarations are visible inside the render fn with no TDZ. The
        // declarator MOVEs land before the render-header insertion at `first`
        // (verified by the ordering test).
        for node in nodes {
            self.project_node(node);
        }

        // Hoist snippet declarators to MODULE scope (before the render fn).
        // Module-scope `const`s are visible inside the render fn with no TDZ,
        // and a preceding `{@render}` sibling references a later-declared
        // snippet cleanly (D-ap). We move each declarator to `first` FIRST,
        // then prepend the render header at `first` with `prepend_left` — which
        // inserts at the chunk boundary BEFORE the already-moved declarator
        // chunks, landing the header below the declarators (module-scope
        // declarators, then `;function __verter_render()`).
        let snippet_moves = std::mem::take(&mut self.snippet_moves);
        for snip in &snippet_moves {
            self.emit_snippet_declarator(first, snip);
        }
        // Hoist declaration-tag bindings to MODULE scope too (before the render
        // fn). They reference script-/module-level symbols, so module scope is
        // valid, and as real statements every sibling reference resolves them
        // (D-ap sibling-run scope) — an in-place IIFE would not.
        let decl_moves = std::mem::take(&mut self.decl_moves);
        for decl in &decl_moves {
            let kw = if decl.is_let { "let " } else { "const " };
            self.ct.move_wrapped(
                decl.inner_span.start,
                decl.inner_span.end,
                first,
                &format!("\n{kw}"),
                ";\n",
            );
        }
        self.ct.append_left(last, "\n</>);\n}\nexport {};\n");
        self.ct
            .prepend_left(first, "\n;function __verter_render() {\nreturn (<>\n");
    }

    /// Emit one hoisted snippet declarator at the scope top.
    ///
    /// The declarator is moved to BEFORE the `;function __verter_render()`
    /// header we prepended at `scope_anchor` — i.e. it relocates the mapped
    /// snippet body to the scope top while branding it through
    /// `__verter_snippet`.
    fn emit_snippet_declarator(&mut self, scope_anchor: u32, snip: &SnippetMove) {
        let params = snip
            .params
            .map(|p| self.slice(p).to_string())
            .unwrap_or_default();
        let header = format!(
            "const {} = __verter_snippet(({}) => (<>\n",
            snip.name, params
        );
        // Move the snippet body to the scope anchor, wrapped so it becomes the
        // branded declarator. The body keeps its mapped span.
        self.ct.move_wrapped(
            snip.body_span.start,
            snip.body_span.end,
            scope_anchor,
            &header,
            "\n</>));\n",
        );
        let _ = snip.block_span;
    }

    fn slice(&self, span: Span) -> &str {
        &self.source[span.start as usize..span.end as usize]
    }

    /// Project one template node.
    fn project_node(&mut self, node: &SvelteNode) {
        match node {
            SvelteNode::Text(_) => { /* literal text — valid JSX text, kept */ }
            SvelteNode::Comment(span) => {
                // `<!-- ... -->` is valid JSX only inside `{/* */}`. Remove it
                // to keep the projection clean (comments carry no types).
                remove_span(self.ct, *span);
            }
            SvelteNode::Interpolation(span) => {
                // `{expr}` is valid JSX child syntax. We re-emit the enclosing
                // braces as overwrites so the EXPRESSION INTERIOR begins its
                // own Original chunk — the source map then emits a token at the
                // expression's start, giving per-expression (hover-precise)
                // mapping rather than line-granular. The `{` is at span.start-1
                // and the `}` at span.end.
                if span.start > 0 {
                    self.ct.overwrite(span.start - 1, span.start, "{");
                }
                self.ct.overwrite(span.end, span.end + 1, "}");
                // An await-EXPRESSION inside the interpolation is out of scope
                // v1 (D-bg): record one typed diagnostic per await-bearing
                // position. The scan is word-boundary + string/comment-aware, so
                // it catches a leading `{await x}`, a NESTED `{foo(await bar())}`,
                // and an `await` inside `$derived(await …)` in markup (all three
                // markup forms). The expression still type-checks; the value
                // remains a checked position.
                let body = self.slice(*span);
                for at in scan_await_positions(body) {
                    let kw = Span::new(span.start + at, span.start + at + "await".len() as u32);
                    self.push_diag(kw, UnsupportedKind::AwaitExperimental);
                }
            }
            SvelteNode::Element(el) => self.project_element(el),
            SvelteNode::Block(block) => self.project_block(block),
            SvelteNode::Tag(tag) => self.project_tag(tag),
        }
    }

    /// Project an element / component / special element.
    fn project_element(&mut self, el: &SvelteElement) {
        match &el.kind {
            SvelteElementKind::NestedStyle => {
                // Nested `<style>` — opaque, stripped from projection (D-ap).
                remove_span(self.ct, el.open_span);
                for child in &el.children {
                    if let SvelteNode::Text(span) = child {
                        remove_span(self.ct, *span);
                    }
                }
                return;
            }
            SvelteElementKind::Special(kind) => {
                self.project_special_element(el, *kind);
                return;
            }
            _ => {}
        }
        // Intrinsic or component element: project attributes, then children.
        for attr in &el.attributes {
            self.project_attribute(el, attr);
        }
        for child in &el.children {
            self.project_node(child);
        }
    }

    /// Project a `<svelte:*>` special element.
    fn project_special_element(&mut self, el: &SvelteElement, kind: SvelteSpecialKind) {
        match kind {
            SvelteSpecialKind::Component | SvelteSpecialKind::SelfRef => {
                self.push_diag(el.open_span, UnsupportedKind::DeprecatedSpecialElement);
                // Project as a void-checked fragment so the file stays valid.
                self.neutralize_element(el);
            }
            SvelteSpecialKind::Fragment => {
                self.push_diag(el.open_span, UnsupportedKind::LegacyFragment);
                self.neutralize_element(el);
            }
            SvelteSpecialKind::Unknown => {
                self.push_diag(el.open_span, UnsupportedKind::Unknown);
                self.neutralize_element(el);
            }
            // Head / Window / Document / Body / Element / Boundary / Options:
            // rewrite the `<svelte:foo>` tag name to a lowercase intrinsic so
            // the JSX intrinsic table types it conservatively, keep attributes.
            _ => {
                self.rewrite_special_to_intrinsic(el);
                for attr in &el.attributes {
                    self.project_attribute(el, attr);
                }
                for child in &el.children {
                    self.project_node(child);
                }
            }
        }
    }

    /// Rewrite a `<svelte:foo ...>` open + close to a lowercase `<div>`
    /// intrinsic carrier (conservative typing) keeping the attribute run.
    fn rewrite_special_to_intrinsic(&mut self, el: &SvelteElement) {
        // Overwrite `svelte:foo` name with `div` so the element types through
        // the intrinsic table, on BOTH the open AND the matching close tag —
        // an `</svelte:window>` residue would be invalid TSX.
        self.ct
            .overwrite(el.name_span.start, el.name_span.end, "div");
        self.rewrite_close_tag_name(el, "div");
    }

    /// Project an element to an empty void-checked fragment (its expressions
    /// are preserved in children but the element wrapper is neutralized).
    fn neutralize_element(&mut self, el: &SvelteElement) {
        // Rewrite the tag name to a fragment-safe `div` and keep children, on
        // BOTH the open AND close tag.
        if el.name_span.start < el.name_span.end {
            self.ct
                .overwrite(el.name_span.start, el.name_span.end, "div");
        }
        self.rewrite_close_tag_name(el, "div");
        for child in &el.children {
            self.project_node(child);
        }
    }

    /// Rewrite the MATCHING `</original-name>` close tag's NAME to `replacement`
    /// (no-op for a self-closing element). DEPTH-AWARE: scans from the open
    /// tag's end, counting `<name`/`</name` opens and closes so a NESTED
    /// same-name element (e.g. nested `<svelte:boundary>`) does not steal the
    /// match — the close that brings depth back to zero is this element's.
    fn rewrite_close_tag_name(&mut self, el: &SvelteElement, replacement: &str) {
        if el.self_closing {
            return;
        }
        let open_needle = format!("<{}", el.name);
        let close_needle = format!("</{}", el.name);
        let mut pos = el.open_span.end as usize;
        let mut depth: i32 = 1; // the open tag itself
        let bytes = self.source.as_bytes();
        while pos < bytes.len() {
            // Find the next open or close of this name, whichever is first.
            let next_close = self.source[pos..].find(&close_needle).map(|i| pos + i);
            let next_open = self.source[pos..].find(&open_needle).map(|i| pos + i);
            match (next_close, next_open) {
                (None, _) => return, // unterminated — leave as-is
                (Some(c), Some(o)) if o < c => {
                    // A nested same-name OPEN — but `</name` also matches
                    // `<name` as a prefix; ensure this `o` is NOT the `c`'s `</`.
                    if o + 1 < bytes.len() && bytes[o + 1] == b'/' {
                        // It's actually a close tag (`</name`), handle as close.
                        depth -= 1;
                        if depth == 0 {
                            self.rewrite_close_at(c, el.name.len(), replacement);
                            return;
                        }
                        pos = c + close_needle.len();
                    } else {
                        depth += 1;
                        pos = o + open_needle.len();
                    }
                }
                (Some(c), _) => {
                    depth -= 1;
                    if depth == 0 {
                        self.rewrite_close_at(c, el.name.len(), replacement);
                        return;
                    }
                    pos = c + close_needle.len();
                }
            }
        }
    }

    /// Overwrite the name run of a `</name` close tag starting at `close_start`.
    fn rewrite_close_at(&mut self, close_start: usize, name_len: usize, replacement: &str) {
        let name_start = (close_start + 2) as u32; // after `</`
        let name_end = name_start + name_len as u32;
        self.ct.overwrite(name_start, name_end, replacement);
    }

    /// Project the attributes of an element (events verbatim-lowercase, class
    /// object/array via the checker, CSS custom props stripped, bindings,
    /// directives).
    fn project_attribute(&mut self, el: &SvelteElement, attr: &SvelteAttribute) {
        match &attr.kind {
            SvelteAttributeKind::Plain {
                name,
                name_span,
                value,
            } => {
                // An empty-name plain attribute carrying an inline-tag inner
                // (`{@attach expr}` / a brace comment used in attribute
                // position) — dispatch on the leading sigil.
                if name.is_empty() {
                    if let Some(SvelteAttributeValue::Expression(inner)) = value {
                        self.project_inline_tag_attribute(attr, *inner);
                        return;
                    }
                }
                if is_css_custom_property(name) {
                    // CSS custom property `--x={expr}`: strip the attribute,
                    // void-check the value (D-ap).
                    self.strip_custom_property_attr(attr, value.as_ref());
                    return;
                }
                // Attribute-value shorthand `<input {value} />`: the parser
                // sets name == the inner expression text and the attribute span
                // opens with `{`. A bare `{value}` is INVALID in a JSX opening
                // tag — rewrite it to `value={value}` by inserting `name=`
                // before the `{` (the `{value}` expression stays mapped).
                if self.source.as_bytes().get(attr.span.start as usize) == Some(&b'{') {
                    self.ct.prepend_left(attr.span.start, &format!("{name}="));
                    let _ = (name_span, value);
                    return;
                }
                // Plain attribute / lowercase event attribute: kept verbatim.
                // `onclick={fn}` stays `onclick` typed by SvelteHTMLElements.
                let _ = (name_span, value);
            }
            SvelteAttributeKind::Spread(_) => {
                // `{...rest}` is valid JSX spread — kept.
            }
            SvelteAttributeKind::Directive(dir) => {
                self.project_directive(el, attr, dir);
            }
        }
    }

    /// Project an inline-tag attribute (`{@attach expr}` used in an element
    /// open tag). The `{@attach}` form is element-attachment machinery: it has
    /// NO published prop (D-bg). We project it to a JSX spread that void-checks
    /// the attachment expression through `__verter_attach` while contributing
    /// no props: `{...(__verter_attach(expr), {})}`.
    fn project_inline_tag_attribute(&mut self, attr: &SvelteAttribute, inner: Span) {
        let body = self.slice(inner).trim_start();
        if let Some(rest) = body.strip_prefix("@attach") {
            // Replace the whole `{@attach EXPR}` attribute with the spread,
            // keeping EXPR mapped: overwrite the `{@attach ` prefix with the
            // spread opener and the trailing `}` with the spread closer.
            // Find EXPR start: inner.start + offset of rest within body.
            let rest_trimmed = rest.trim_start();
            let expr_offset = body.len() - rest_trimmed.len();
            let expr_start = inner.start + expr_offset as u32;
            // `{@attach ` → `{...(__verter_attach(`
            self.ct
                .overwrite(attr.span.start, expr_start, "{...(__verter_attach(");
            // closing `}` → `), {})}`
            self.ct.overwrite(inner.end, attr.span.end, "), {})}");
            return;
        }
        // A brace comment or unrecognised inline tag in attribute position —
        // strip it (no type surface).
        remove_span(self.ct, attr.span);
    }

    /// Strip a CSS custom-property attribute (`--x={expr}`) from the JSX
    /// position and void-check its value (D-ap). A `--`-prefixed name is not a
    /// valid JSX attribute identifier, so the WHOLE `--name=` attribute name is
    /// removed; the `{expr}` value is rewritten into a JSX spread that
    /// void-checks the expression while contributing NO props:
    /// `{...(__verter_void(expr), {})}`. The expression bytes stay mapped.
    fn strip_custom_property_attr(
        &mut self,
        attr: &SvelteAttribute,
        value: Option<&SvelteAttributeValue>,
    ) {
        if let Some(SvelteAttributeValue::Expression(expr)) = value {
            // `--name={` → `{...(__verter_void(` ; the trailing `}` → `), {})}`.
            // The whole prefix (attribute start through the expression start)
            // becomes the spread opener — no `--` residue survives.
            self.ct
                .overwrite(attr.span.start, expr.start, "{...(__verter_void(");
            self.ct.overwrite(expr.end, attr.span.end, "), {})}");
            return;
        }
        // Static or no value — remove the attribute entirely (no type surface).
        remove_span(self.ct, attr.span);
    }

    /// Project a directive attribute.
    fn project_directive(
        &mut self,
        el: &SvelteElement,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        match dir.kind {
            SvelteDirectiveKind::Bind => {
                self.project_bind(el, attr, dir);
            }
            SvelteDirectiveKind::On => {
                // Legacy `on:click={h}` → `onclick={h}` verbatim namespaced
                // lowercase (D-ae(b)) — rewrite `on:click` to `onclick`.
                self.rewrite_legacy_on(attr, dir);
            }
            SvelteDirectiveKind::Class => {
                // `class:active={cond}` → keep as a checkable boolean attribute
                // by rewriting to `data-class-active={cond}` (SUPPORTED legacy
                // coverage — the condition expression stays void-checked).
                self.rewrite_class_directive_to_data(attr, dir);
            }
            SvelteDirectiveKind::Style => {
                // `style:color={c}` / `style:color|important` (F1) — SUPPORTED.
                // A `style:`-prefixed name is not a valid JSX attribute
                // identifier, so the directive is STRIPPED from the JSX position
                // (mirroring the CSS-custom-property pass-through) and its value
                // is void-checked. The `|important` modifier is presentational.
                // The shorthand `style:color` (no value) projects the implied
                // `color` binding only when the name is a valid binding identifier.
                self.rewrite_style_directive(attr, dir);
            }
            SvelteDirectiveKind::Use => {
                // `use:action` (+ parameter) — SUPPORTED (basic action parameter
                // checking, D-u). Strip from the JSX position but void-check the
                // parameter so its type errors / hover survive.
                self.strip_directive_void_checking_value(attr, dir);
            }
            SvelteDirectiveKind::Transition
            | SvelteDirectiveKind::In
            | SvelteDirectiveKind::Out => {
                // `transition:fn={p}` / `in:fn={p}` / `out:fn={p}` (+`|local`/
                // `|global`) (F2) — SUPPORTED. Stripped from the JSX position and
                // spread-merged into a `__verter_transition(node_hint, fn, p)`
                // check (like `__verter_attach`): the transition function `fn`
                // (the directive local) and the params `p` are checked against the
                // host element's instance type. The `|local`/`|global` modifiers
                // are presentational.
                self.rewrite_transition_directive(el, attr, dir);
            }
            SvelteDirectiveKind::Animate => {
                // `animate:fn={p}` (F3) — SUPPORTED. Stripped + spread-merged into
                // `__verter_animate(fn(NODE_HINT, DIRECTIONS, p))`.
                self.rewrite_animate_directive(el, attr, dir);
            }
            SvelteDirectiveKind::Unknown => {
                remove_span(self.ct, attr.span);
            }
        }
    }

    /// Strip an out-of-scope directive from the JSX attribute position while
    /// VOID-CHECKING its value expression — the value stays mapped + checkable
    /// so inner type errors and hover survive, but contributes NO prop:
    /// `{...(__verter_void(EXPR), {})}`. A directive with no expression value
    /// (or a static/quoted value) is removed outright (no type surface).
    fn strip_directive_void_checking_value(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        if let Some(SvelteAttributeValue::Expression(expr)) = dir.value {
            self.ct
                .overwrite(attr.span.start, expr.start, "{...(__verter_void(");
            self.ct.overwrite(expr.end, attr.span.end, "), {})}");
            return;
        }
        remove_span(self.ct, attr.span);
    }

    /// Project a `style:` directive (F1).
    ///
    /// `style:color={c}` / `style:color|important` — a `style:`-prefixed name is
    /// not a valid JSX attribute identifier, so the directive is STRIPPED from
    /// the JSX position (mirroring the CSS-custom-property pass-through) and its
    /// value is void-checked: `{...(__verter_void(c), {})}` (the value stays
    /// mapped/checkable, contributes no prop). The `|important` modifier is
    /// presentational. The valueless SHORTHAND `style:color` projects the implied
    /// `color` binding identifier (`{...(__verter_void(color), {})}`) ONLY when
    /// `color` is a valid JS binding identifier; otherwise the attribute is
    /// removed outright (no type surface, no invalid identifier residue).
    fn rewrite_style_directive(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        if let Some(SvelteAttributeValue::Expression(expr)) = dir.value {
            // `style:NAME[|mods]={` → `{...(__verter_void(` ; trailing `}` →
            // `), {})}`. The whole directive name+modifiers prefix becomes the
            // spread opener — no `style:` residue survives, the value is mapped.
            self.ct
                .overwrite(attr.span.start, expr.start, "{...(__verter_void(");
            self.ct.overwrite(expr.end, attr.span.end, "), {})}");
            return;
        }
        // Shorthand `style:color` (no `={…}`): project the implied `color`
        // binding when it is a valid identifier so its type errors / hover
        // survive — `{...(__verter_void(color), {})}`.
        if is_valid_binding_identifier(&dir.local) {
            self.ct.overwrite(
                attr.span.start,
                attr.span.end,
                &format!("{{...(__verter_void({}), {{}})}}", dir.local),
            );
            return;
        }
        remove_span(self.ct, attr.span);
    }

    /// Project a `transition:` / `in:` / `out:` directive (F2).
    ///
    /// `transition:fn={p}` (+`|local`/`|global`) → a spread-merged
    /// `{...(__verter_transition(fn(NODE_HINT, p)), {})}` — the directive is
    /// stripped from the JSX position and the transition function `fn` (the
    /// directive local, an imported function identifier) is CALLED on the host
    /// element instance (`NODE_HINT`, a typed `null!` cast keyed off the host
    /// tag) with the params `p`. A real call site is the soundest projection:
    /// TSGO checks the host-node type, the params type, the arg count (a
    /// non-function `fn` is not callable, a missing required `params` is an
    /// arg-count error, a wrong `params` is a type error), and the result is
    /// asserted to be a `TransitionConfig` through `__verter_transition` (a thin
    /// result-shape checker). The `|local`/`|global` modifiers are
    /// presentational. A valueless `transition:fn` (no params) calls
    /// `fn(NODE_HINT)`. A non-identifier local emits no call (the attribute is
    /// removed — no invalid identifier residue).
    fn rewrite_transition_directive(
        &mut self,
        el: &SvelteElement,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        if !is_valid_binding_identifier(&dir.local) {
            remove_span(self.ct, attr.span);
            return;
        }
        let node_hint = self.host_element_hint(el);
        let fn_name = dir.local.clone();
        if let Some(SvelteAttributeValue::Expression(expr)) = dir.value {
            // `transition:fn[|mods]={` →
            // `{...(__verter_transition({fn}({hint}, ` ; trailing `}` →
            // `)), {})}`. The params expression `p` stays mapped (its inner type
            // errors + hover survive). A Svelte transition function is invoked at
            // RUNTIME as `fn(node, params, { direction })`, but the PUBLIC TYPES of
            // the built-in transitions (`fly`/`fade`/`slide`/…) and idiomatic
            // userland transitions declare only `(node, params?)` — the trailing
            // `options` is always optional/absent in the type surface. Passing a
            // third arg would therefore break every two-param built-in, so the
            // projected call is `fn(node, params)` (host node + params): a custom
            // transition that DECLARES an `options` param keeps it optional (the
            // `custom_transition_with_optional_options_…` gate fixture pins this).
            self.ct.overwrite(
                attr.span.start,
                expr.start,
                &format!("{{...(__verter_transition({fn_name}({node_hint}, "),
            );
            self.ct.overwrite(expr.end, attr.span.end, ")), {})}");
            return;
        }
        // No params: call `fn(NODE_HINT)`.
        self.ct.overwrite(
            attr.span.start,
            attr.span.end,
            &format!("{{...(__verter_transition({fn_name}({node_hint})), {{}})}}"),
        );
    }

    /// Project an `animate:` directive (F3).
    ///
    /// `animate:fn={p}` →
    /// `{...(__verter_animate(fn(NODE_HINT, DIRECTIONS, p)), {})}` — the directive
    /// is stripped and the animate function `fn` is CALLED on the host element
    /// with a synthetic from/to-rect `DIRECTIONS` descriptor and the params `p`.
    /// As for transitions, the real call site is the soundest check (host node +
    /// params + arity + non-function), and the result is asserted to be an
    /// `AnimationConfig` through `__verter_animate`. A valueless `animate:fn`
    /// calls `fn(NODE_HINT, DIRECTIONS)`; a non-identifier local emits no call.
    fn rewrite_animate_directive(
        &mut self,
        el: &SvelteElement,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        if !is_valid_binding_identifier(&dir.local) {
            remove_span(self.ct, attr.span);
            return;
        }
        let node_hint = self.host_element_hint(el);
        let fn_name = dir.local.clone();
        let directions = "(null! as { from: DOMRect; to: DOMRect })";
        if let Some(SvelteAttributeValue::Expression(expr)) = dir.value {
            self.ct.overwrite(
                attr.span.start,
                expr.start,
                &format!("{{...(__verter_animate({fn_name}({node_hint}, {directions}, "),
            );
            self.ct.overwrite(expr.end, attr.span.end, ")), {})}");
            return;
        }
        self.ct.overwrite(
            attr.span.start,
            attr.span.end,
            &format!("{{...(__verter_animate({fn_name}({node_hint}, {directions})), {{}})}}"),
        );
    }

    /// The typed node-hint expression for a `transition:`/`animate:`-host element.
    ///
    /// For an INTRINSIC element the hint resolves the precise DOM instance type
    /// via the prelude's `__VerterHostEl<Tag>` (known HTML/SVG tag → its element
    /// type, unknown/custom → `Element`). For a component / `<svelte:*>` /
    /// dynamic host the host element type is unknown, so the hint falls back to
    /// the `Element` bound.
    fn host_element_hint(&self, el: &SvelteElement) -> String {
        match el.kind {
            SvelteElementKind::Intrinsic => {
                // NIT-1: `el.name` is interpolated raw into a `__VerterHostEl<"…">`
                // string literal. The parser only classifies a bare tag identifier
                // as `Intrinsic`, so this is safe today; guard it so a future
                // producer change that admits a `"`/newline into the name fails
                // loudly here instead of emitting a broken type literal.
                debug_assert!(
                    is_bare_tag_identifier(&el.name),
                    "intrinsic host tag must be a bare identifier with no quote/newline: {:?}",
                    el.name
                );
                format!("(null! as __VerterHostEl<\"{}\">)", el.name)
            }
            _ => "(null! as Element)".to_string(),
        }
    }

    /// Whether a `bind:` directive value is a function binding
    /// (`bind:x={get, set}` — a top-level comma in the value expression).
    fn is_function_binding(&self, dir: &crate::svelte::parser::SvelteDirective) -> bool {
        let Some(SvelteAttributeValue::Expression(span)) = dir.value else {
            return false;
        };
        let body = self.slice(span);
        // A top-level comma (depth 0, outside strings) marks the `get, set`
        // function-binding form.
        let mut depth = 0i32;
        for ch in body.chars() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => return true,
                _ => {}
            }
        }
        false
    }

    /// Project a `bind:` directive to a checkable JSX attribute pair (the
    /// component `$bindable`-prop path + `bind:value`/`bind:checked`).
    ///
    /// `bind:value={v}` → `value={v}` (strip the `bind:` prefix, keep the
    /// `={v}` value mapped). The valueless SHORTHAND `bind:value` (no `={…}`)
    /// binds the same-named local, so it becomes `value={value}` — the whole
    /// `bind:local` run is overwritten with `local={local}` (a bare `value`
    /// attribute would be a boolean `true`, not the bound value).
    fn rewrite_bind_to_attribute(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        if dir.value.is_some() {
            // Strip `bind:` (prefix + colon), keeping `local={value}`.
            let prefix_len = "bind:".len() as u32;
            self.ct
                .overwrite(attr.span.start, attr.span.start + prefix_len, "");
        } else {
            // Valueless shorthand `bind:local` → `local={local}`.
            let local = &dir.local;
            self.ct.overwrite(
                attr.span.start,
                attr.span.end,
                &format!("{local}={{{local}}}"),
            );
        }
    }

    /// Rewrite a legacy `on:event` to `onevent` (verbatim lowercase).
    fn rewrite_legacy_on(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        // `on:click` → `onclick`. Overwrite `on:` with `on` (drop the colon),
        // keeping the event local + value.
        let start = attr.span.start;
        let prefix_len = "on:".len() as u32;
        self.ct.overwrite(start, start + prefix_len, "on");
        let _ = dir;
    }

    /// Rewrite a `class:` directive to a `data-class-*` attribute keeping the
    /// condition value mapped.
    fn rewrite_class_directive_to_data(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        // Replace `class:active` (name part) with `data-class-active`. We
        // overwrite from the attribute start to the end of the local name.
        let name_end = attr.span.start + ("class:".len() + dir.local.len()) as u32;
        let replacement = format!("data-class-{}", dir.local);
        self.ct.overwrite(attr.span.start, name_end, &replacement);
    }

    /// Project a block construct.
    fn project_block(&mut self, block: &SvelteBlock) {
        match &block.kind {
            SvelteBlockKind::If => self.project_if(block),
            SvelteBlockKind::Each { item, index, key } => {
                self.project_each(block, *item, *index, *key)
            }
            SvelteBlockKind::Await {
                then_binding,
                catch_binding,
            } => self.project_await(block, *then_binding, *catch_binding),
            SvelteBlockKind::Key => self.project_key(block),
            SvelteBlockKind::Snippet {
                name_text, params, ..
            } => self.project_snippet(block, name_text, *params),
        }
    }

    /// `{#if c}A{:else if d}B{:else}C{/if}` → `{c ? (<>A</>) : d ? (<>B</>) : (<>C</>)}`.
    fn project_if(&mut self, block: &SvelteBlock) {
        let Some(head) = block.head_expr else { return };
        // Overwrite `{#if ` (from block start to head start) with `{`.
        self.ct.overwrite(block.span.start, head.start, "{");
        // After the condition: `}` (the close of `{#if c}`) becomes ` ? (<>`.
        // The body run follows. We overwrite the single `}` after head.
        let after_head = head.end;
        // Find the `}` closing the if-open.
        let close = self.find_char_after(after_head, '}');
        if let Some(close_idx) = close {
            self.ct.overwrite(after_head, close_idx + 1, " ? (<>");
        }
        // Project children (the true branch body).
        for child in &block.children {
            self.project_node(child);
        }
        // Handle clauses.
        self.project_if_clauses(block);
        // Close the whole block: overwrite `{/if}` with `</>)}`.
        let end_tag_start = self.find_str_before(block.span.end, "{/if}");
        if let Some(s) = end_tag_start {
            self.ct.overwrite(s, block.span.end, "</>)}");
        }
    }

    fn project_if_clauses(&mut self, block: &SvelteBlock) {
        for clause in &block.clauses {
            match clause.kind {
                SvelteClauseKind::ElseIf => {
                    // `{:else if d}` → `</>) : d ? (<>`. The clause-tag head span
                    // (`{:else if ` through the condition start) is rewritten
                    // from the parser-provided `tag_span`, and the closing `}`
                    // (tag_span.end-1..end) re-opens the body fragment.
                    if let Some(expr) = clause.expr {
                        self.ct
                            .overwrite(clause.tag_span.start, expr.start, "</>) : ");
                        self.ct.overwrite(expr.end, clause.tag_span.end, " ? (<>");
                    } else {
                        // A malformed `{:else if}` with no condition — rewrite the
                        // whole tag to a falsy ternary arm so no raw `{:…}` leaks.
                        self.ct.overwrite(
                            clause.tag_span.start,
                            clause.tag_span.end,
                            "</>) : false ? (<>",
                        );
                    }
                }
                SvelteClauseKind::Else => {
                    // `{:else}` → `</>) : (<>` — overwrite the WHOLE clause-tag
                    // span (braces included). An empty `{:else}` (no expr, no
                    // children) is still rewritten — the `tag_span` is always
                    // present, so no raw `{:else}` leaks (P1-1).
                    self.ct
                        .overwrite(clause.tag_span.start, clause.tag_span.end, "</>) : (<>");
                }
                _ => {}
            }
            for child in &clause.children {
                self.project_node(child);
            }
        }
    }

    /// `{#each xs as x, i (key)}BODY{/each}` → `{xs.map((x, i) => (<>BODY</>))}`.
    fn project_each(
        &mut self,
        block: &SvelteBlock,
        item: Option<Span>,
        index: Option<Span>,
        _key: Option<Span>,
    ) {
        let Some(head) = block.head_expr else { return };
        // `{#each ` → `{`
        self.ct.overwrite(block.span.start, head.start, "{");
        // After the list expression, build `.map((item, index) => (<>`.
        // The original ` as x, i (key)}` run (from head.end to the open `}`)
        // is overwritten.
        let open_close = self.find_char_after(head.end, '}');
        if let Some(close_idx) = open_close {
            let params = match (item, index) {
                (Some(it), Some(ix)) => format!("{}, {}", self.slice(it), self.slice(ix)),
                (Some(it), None) => self.slice(it).to_string(),
                (None, _) => "__verter_item".to_string(),
            };
            self.ct
                .overwrite(head.end, close_idx + 1, &format!(".map(({params}) => (<>"));
        }
        for child in &block.children {
            self.project_node(child);
        }
        // `{:else}` (each-else): close the `.map(...)` items expression and open
        // a SEPARATE sibling `{false && (<>ELSE</>)}` — the else body's
        // expressions stay type-checked (and mapped) but render nothing. This
        // is valid TSX (two sibling JSX expressions), unlike a patched `.map`
        // close.
        let has_else = block
            .clauses
            .iter()
            .any(|c| c.kind == SvelteClauseKind::Else);
        for clause in &block.clauses {
            if clause.kind == SvelteClauseKind::Else {
                // Overwrite the WHOLE `{:else}` clause-tag span (braces
                // included) — an empty each-else still rewrites cleanly (P1-1).
                self.ct.overwrite(
                    clause.tag_span.start,
                    clause.tag_span.end,
                    "</>))}\n{false && (<>",
                );
                for child in &clause.children {
                    self.project_node(child);
                }
            }
        }
        // `{/each}` → close the (items map) OR the (else sibling fragment).
        if let Some(s) = self.find_str_before(block.span.end, "{/each}") {
            if has_else {
                // The else sibling fragment closes with `</>)}`.
                self.ct.overwrite(s, block.span.end, "</>)}");
            } else {
                self.ct.overwrite(s, block.span.end, "</>))}");
            }
        }
    }

    /// `{#await p}P{:then v}T{:catch e}C{/await}` → ternary over a synthetic
    /// promise-state holder. v1: await-expressions are out of scope (D-bg) only
    /// for the EXPRESSION position; the `{#await}` BLOCK itself projects.
    fn project_await(
        &mut self,
        block: &SvelteBlock,
        then_binding: Option<Span>,
        catch_binding: Option<Span>,
    ) {
        let Some(head) = block.head_expr else { return };
        // Synthetic holder: `{((__verter_await) => __verter_await.pending ? (<>P</>) : __verter_await.error ? (<>C</>) : (<>T</>))(__verter_state(PROMISE))}`
        // For a tractable, type-clean projection: resolve the promise value
        // type via `Awaited<typeof PROMISE>` and bind it.
        self.ct.overwrite(
            block.span.start,
            head.start,
            "{((__verter_p) => { type __VA = Awaited<typeof __verter_p>; ",
        );
        // Bind then/catch and project branches as void-checked fragments.
        let _ = (then_binding, catch_binding);
        let open_close = self.find_char_after(head.end, '}');
        if let Some(c) = open_close {
            self.ct.overwrite(head.end, c + 1, "; return (<>");
        }
        for child in &block.children {
            self.project_node(child);
        }
        // Project clauses (`:then`, `:catch`) as fragment continuations.
        for clause in &block.clauses {
            match clause.kind {
                SvelteClauseKind::Then => {
                    // Bind the value as a const of the awaited type. Overwrite the
                    // WHOLE `{:then v}` clause-tag span — an empty `{:then}` (no
                    // binding) still rewrites cleanly with a synthetic name (P1-1).
                    let binding = clause
                        .expr
                        .map(|sp| self.slice(sp).to_string())
                        .unwrap_or_else(|| "__verter_v".to_string());
                    self.ct.overwrite(
                        clause.tag_span.start,
                        clause.tag_span.end,
                        &format!("</>); const {binding}: __VA = (null as any); return (<>"),
                    );
                    for child in &clause.children {
                        self.project_node(child);
                    }
                }
                SvelteClauseKind::Catch => {
                    // Declare the catch binding (`{:catch e}` → a typed
                    // `const e: unknown`) so the catch body's `{e}` resolves.
                    // Overwrite the WHOLE `{:catch e}` clause-tag span — an empty
                    // `{:catch}` still rewrites cleanly (P1-1).
                    let binding = clause
                        .expr
                        .map(|sp| self.slice(sp).to_string())
                        .unwrap_or_else(|| "__verter_e".to_string());
                    self.ct.overwrite(
                        clause.tag_span.start,
                        clause.tag_span.end,
                        &format!("</>); const {binding}: unknown = (null as any); return (<>"),
                    );
                    for child in &clause.children {
                        self.project_node(child);
                    }
                }
                _ => {}
            }
        }
        if let Some(s) = self.find_str_before(block.span.end, "{/await}") {
            self.ct
                .overwrite(s, block.span.end, "</>); })(null as any)}");
        }
    }

    /// `{#key e}BODY{/key}` → `{(__verter_void(e), <>BODY</>)}` — the key
    /// expression `e` stays mapped and type-checked (the comma operator's left
    /// operand), the body renders. Valid TSX, no IIFE arity mismatch.
    fn project_key(&mut self, block: &SvelteBlock) {
        let Some(head) = block.head_expr else { return };
        // `{#key ` → `{(__verter_void(` — the head `e` stays in place, mapped.
        self.ct
            .overwrite(block.span.start, head.start, "{(__verter_void(");
        // The `}` closing `{#key e}` → `), <>` (close the void call, comma, open
        // the body fragment).
        if let Some(c) = self.find_char_after(head.end, '}') {
            self.ct.overwrite(head.end, c + 1, "), <>");
        }
        for child in &block.children {
            self.project_node(child);
        }
        // `{/key}` → `</>)}`
        if let Some(s) = self.find_str_before(block.span.end, "{/key}") {
            self.ct.overwrite(s, block.span.end, "</>)}");
        }
    }

    /// `{#snippet name(params)}BODY{/snippet}` → hoisted branded declarator.
    fn project_snippet(&mut self, block: &SvelteBlock, name: &str, params: Option<Span>) {
        // Compute the body span: from the end of the `{#snippet ...}` head to
        // the start of `{/snippet}`.
        let head_close = self
            .find_char_after(block.span.start, '}')
            .map(|c| c + 1)
            .unwrap_or(block.span.start);
        let end_tag = self
            .find_str_before(block.span.end, "{/snippet}")
            .unwrap_or(block.span.end);
        let body_span = Span::new(head_close, end_tag);
        // Remove the original `{#snippet ...}` head and `{/snippet}` tail in
        // place (the body is MOVED out, so its source bytes are relocated).
        self.ct.remove(block.span.start, head_close);
        self.ct.remove(end_tag, block.span.end);
        // Project the body's children before moving (transforms apply to the
        // moved bytes).
        for child in &block.children {
            self.project_node(child);
        }
        self.snippet_moves.push(SnippetMove {
            block_span: block.span,
            name: name.to_string(),
            params,
            body_span,
        });
    }

    /// Remove a declaration tag (`{const ...}`/`{let ...}`/`{@const ...}`) in
    /// place and queue its inner `x = e` for hoisting to the render scope top.
    fn hoist_declaration_tag(&mut self, tag: &SvelteTag, is_let: bool) {
        // Remove the whole tag from the JSX position (`{const ` … `}`) — the
        // inner declaration is moved out, so its bytes are relocated.
        self.ct.remove(tag.span.start, tag.inner.start);
        self.ct.remove(tag.inner.end, tag.span.end);
        self.decl_moves.push(DeclMove {
            is_let,
            inner_span: tag.inner,
        });
    }

    /// Project a standalone tag.
    fn project_tag(&mut self, tag: &SvelteTag) {
        match tag.kind {
            SvelteTagKind::Render => {
                // `{@render snippet(args)}` → `{snippet(args)}` — checks through
                // Snippet's call signature. Overwrite `{@render ` → `{`.
                self.ct.overwrite(tag.span.start, tag.inner.start, "{");
                // Close `}` stays.
                self.rewrite_tag_close(tag, "}");
            }
            SvelteTagKind::Html => {
                // `{@html e}` → `{__verter_html_check(e)}` is overkill; a string
                // position checks `e` — overwrite `{@html ` → `{(`, close → `)}`.
                self.ct.overwrite(tag.span.start, tag.inner.start, "{(");
                self.rewrite_tag_close(tag, ") as unknown as string}");
            }
            SvelteTagKind::Const | SvelteTagKind::LegacyConst => {
                // `{const x = e}` / `{@const x = e}` → a `const x = e;` HOISTED
                // to the render scope top (a real statement, visible to sibling
                // references — D-ap sibling-run scope). The inner `x = e` is
                // moved (kept mapped); the original tag is removed in place.
                self.hoist_declaration_tag(tag, false);
            }
            SvelteTagKind::Let => {
                self.hoist_declaration_tag(tag, true);
            }
            SvelteTagKind::Debug => {
                // `{@debug a, b}` → `{__verter_void([a, b])}` void reference.
                self.ct
                    .overwrite(tag.span.start, tag.inner.start, "{__verter_void([");
                self.rewrite_tag_close(tag, "])}");
            }
            SvelteTagKind::Attach => {
                // `{@attach e}` → `{__verter_attach(e)}` checker argument.
                self.ct
                    .overwrite(tag.span.start, tag.inner.start, "{__verter_attach(");
                self.rewrite_tag_close(tag, ")}");
            }
            SvelteTagKind::Unknown => {
                self.push_diag(tag.span, UnsupportedKind::Unknown);
                self.ct
                    .overwrite(tag.span.start, tag.inner.start, "{__verter_void(");
                self.rewrite_tag_close(tag, ")}");
            }
        }
    }

    /// Rewrite a tag's closing `}` (the last `}` before tag.span.end).
    fn rewrite_tag_close(&mut self, tag: &SvelteTag, replacement: &str) {
        // The tag ends with `}`. Overwrite the final `}` (tag.span.end-1..end).
        if tag.span.end > tag.inner.end {
            self.ct.overwrite(tag.inner.end, tag.span.end, replacement);
        }
    }

    fn push_diag(&mut self, span: Span, kind: UnsupportedKind) {
        self.diagnostics.push(SvelteIdeUnsupportedDiagnostic {
            code: kind.code(),
            message: kind.message().to_string(),
            span,
        });
    }

    /// Find the byte index of the first `needle` char at or after `from`.
    fn find_char_after(&self, from: u32, needle: char) -> Option<u32> {
        let bytes = self.source.as_bytes();
        let mut i = from as usize;
        while i < bytes.len() {
            if bytes[i] == needle as u8 {
                return Some(i as u32);
            }
            i += 1;
        }
        None
    }

    /// Find the start index of the last `needle` substring before `before`.
    fn find_str_before(&self, before: u32, needle: &str) -> Option<u32> {
        let hay = &self.source[..(before as usize).min(self.source.len())];
        hay.rfind(needle).map(|i| i as u32)
    }
}

fn node_span(node: &SvelteNode) -> Option<Span> {
    Some(match node {
        SvelteNode::Text(s) | SvelteNode::Comment(s) | SvelteNode::Interpolation(s) => *s,
        SvelteNode::Element(el) => el.open_span,
        SvelteNode::Block(b) => b.span,
        SvelteNode::Tag(t) => t.span,
    })
}

/// Whether `name` is a valid JS binding identifier — used to decide whether a
/// shorthand `style:color` / a `transition:`/`animate:` local can be projected
/// as a bare identifier reference. Conservative ASCII rule: a leading
/// `A-Za-z_$`, then `A-Za-z0-9_$`. A name failing this (empty, hyphenated, …)
/// is NOT emitted as an identifier (the directive is removed — no invalid
/// identifier residue in the projected TSX).
fn is_valid_binding_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Whether `name` is a bare tag identifier safe to interpolate raw into a
/// `__VerterHostEl<"…">` string literal (no `"`, no newline, no backslash).
/// Used as a defensive guard at the `host_element_hint` interpolation site
/// (NIT-1) — the parser only classifies a bare tag as `Intrinsic`, so this holds
/// today; the guard hardens against a future producer change.
fn is_bare_tag_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c != '"' && c != '\\' && c != '\n' && c != '\r' && c != '<' && c != '>')
}

/// Whether `name` is a valid component reference identifier to interpolate into
/// `InstanceType<typeof Name>` / `InstanceType<typeof Name>["$props"][…]`. A
/// component tag is PascalCase OR a dotted/namespaced member access
/// (`ns.Widget`) — both are valid `typeof` operands. A name with any other
/// character (a quote, a `<`, whitespace) is NOT emitted (the host falls back to
/// `Element`).
fn is_valid_component_reference(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    // Each dotted segment must be a valid identifier.
    name.split('.').all(is_valid_binding_identifier)
}

/// Whether `expr` is a TYPE-QUERY-SAFE lvalue — a bare identifier or a dotted
/// member chain (`el`, `refs.first`) — so `typeof expr` is a valid TS type
/// query. An element-access (`refs[i]`), a call, or any other expression is NOT
/// safe (`typeof refs[i]` parses `i` as a type), and the `bind:this` projection
/// routes those through the read-bearing invariant form instead. Whitespace is
/// trimmed first (a `{ el }`-style padded expression slice).
fn is_type_query_safe_lvalue(expr: &str) -> bool {
    let trimmed = expr.trim();
    !trimmed.is_empty() && trimmed.split('.').all(is_valid_binding_identifier)
}
