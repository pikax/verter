//! The Svelte-OWNED CSS substrate: shared-grammar parse, scope-hash
//! derivation, and scoping analysis for component `<style>` blocks.
//!
//! This module owns the CSS DOMAIN of the Svelte runtime pipeline. It is
//! fully separate from the Vue style pipeline (`crate::css`) — the Svelte
//! scoping semantics match the official `svelte@5.56.10` compiler
//! (`phases/2-analyze/css/*`, `phases/3-transform/css/*`,
//! `phases/css.js`), operating on byte spans of the ORIGINAL component
//! source so downstream source-position edits map exactly.
//!
//! Pipeline: [`verter_css_syntax::parse_style_ir`] parses the css body ONCE
//! into the shared [`StyleSyntaxIr`] → [`analyze::analyze_stylesheet`]
//! validates `:global`/nesting placement, builds the `Span`-keyed selector
//! metadata side table ([`analyze::CssAnalysis`]), and collects
//! keyframes/global facts → [`matcher::match_stylesheet`] runs the
//! selector-to-template matcher (the `css-prune.js` port) over the runtime
//! IR, writing the used/scoped selector verdicts into the SAME side table and
//! producing the per-element scope facts → [`hash::css_scope_hash`] derives
//! the `svelte-<djb2>` scope hash → [`render::render_stylesheet`] produces
//! the scoped stylesheet text (the official `css.code`) by source-position
//! edits over the ORIGINAL component source → the facts assemble into the
//! per-`<style>` [`ProvenStyleScopePlan`](types::ProvenStyleScopePlan) side
//! table (the ONE shared fact both scope-class injection sites and the css
//! emitter read). Every failure mode — a css parse/analysis failure, an
//! unprovable selector⇄template relation, a render refusal — is the typed
//! [`StylePlanFailure`]: a plan value exists ONLY for a fully-proven style.

pub mod analyze;
pub mod hash;
mod match_relsel;
#[path = "match.rs"]
pub mod matcher;
pub mod render;
pub mod types;

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_css_syntax::{parse_style_ir, CssDialect, CssParseMode, CssSource, StyleSyntaxIr};
use verter_span::Span;

/// Request-scoped admitted style IRs. Official-reject and analysis share this
/// map so a style body is parsed once per compile, never via thread-local
/// ambient admission.
#[derive(Default)]
pub(super) struct AdmittedStyleIrs {
    irs: FxHashMap<(u32, u32), StyleSyntaxIr>,
}

impl AdmittedStyleIrs {
    pub(super) fn insert_prepared(&mut self, content: Span, ir: StyleSyntaxIr) {
        self.irs.insert((content.start, content.end), ir);
    }

    fn take(&mut self, content: Span) -> Option<StyleSyntaxIr> {
        self.irs.remove(&(content.start, content.end))
    }

    fn get(&self, content: Span) -> Option<&StyleSyntaxIr> {
        self.irs.get(&(content.start, content.end))
    }
}

pub(super) fn seed_admitted_from_prepared(
    source: &str,
    parsed: &crate::svelte::parser::ParsedSvelte,
    prepared_styles: &[Option<crate::style_planner::PreparedStyleIr>],
    admitted: &mut AdmittedStyleIrs,
) {
    for (index, style) in parsed.styles.iter().enumerate() {
        let Some(prepared) = prepared_styles.get(index).and_then(|slot| slot.as_ref()) else {
            continue;
        };
        let Some(content) = style.content else {
            continue;
        };
        let Some(css) = source.get(content.start as usize..content.end as usize) else {
            continue;
        };
        if prepared.ir().source().text() != css || prepared.ir().source().origin() != content.start
        {
            continue;
        }
        admitted.insert_prepared(content, prepared.ir().clone());
    }
}

/// Parse the style body once for the official-reject race. On a Svelte CSS
/// parse code, return it; on a clean body, retain the IR for
/// [`analyze_style_body`].
pub(super) fn admit_style_body(
    source: &str,
    content: Span,
    admitted: &mut AdmittedStyleIrs,
) -> Option<&'static str> {
    if let Some(ir) = admitted.get(content) {
        return verter_css_syntax::svelte_reject_from_ir(ir);
    }
    match verter_css_syntax::parse_style_body(source, content) {
        Ok(ir) => {
            if let Some(code) = verter_css_syntax::svelte_reject_from_ir(&ir) {
                return Some(code);
            }
            admitted.insert_prepared(content, ir);
            None
        }
        Err(_) => Some("css_expected_identifier"),
    }
}
// `ComplexSelectorPart`/`StyleStatement` are read only by the alloc-probe
// bridge below (`reread_cached_css_facts_for_alloc_probe`, itself gated the
// same way).
#[cfg(any(test, feature = "test-support"))]
use verter_css_syntax::{ComplexSelectorPart, StyleStatement};

use super::ir::SvelteRuntimeIr;
use types::{CssMode, ProvenStyleScopePlan};

/// A typed style-plan failure — the ONLY way a `<style>` block does not
/// produce a [`ProvenStyleScopePlan`]. Fail-closed: the caller refuses
/// emission on the style surface (never unscoped output, never a guessed
/// scope), threading [`code`](Self::code) + [`span`](Self::span) unchanged
/// into the diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StylePlanFailure {
    /// The failure class (which pipeline stage refused).
    pub class: StylePlanFailureClass,
    /// The precise diagnostic code: the official css parse / validation code
    /// (`css_expected_identifier` / `css_global_invalid_placement` / …) for a
    /// [`ParseAnalysis`](StylePlanFailureClass::ParseAnalysis) failure, the
    /// fixed selector-refusal id for a
    /// [`SelectorUnprovable`](StylePlanFailureClass::SelectorUnprovable)
    /// refusal, or the Verter fail-closed render refusal `css_render_failed`
    /// for a [`RenderInvariant`](StylePlanFailureClass::RenderInvariant)
    /// refusal.
    pub code: &'static str,
    /// The byte span of the offending construct (absolute in the component
    /// source).
    pub span: Span,
    /// A stable description of the unprovable construct class — `Some` for a
    /// [`SelectorUnprovable`](StylePlanFailureClass::SelectorUnprovable)
    /// matcher refusal, `None` otherwise.
    pub construct: Option<&'static str>,
}

/// The pipeline stage a [`StylePlanFailure`] refused in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StylePlanFailureClass {
    /// The css body failed the PARSE or the scoping ANALYSIS
    /// ([`analyze_style_body`]).
    ParseAnalysis,
    /// The selector-to-template matcher could not PROVE the
    /// selector⇄template relation
    /// ([`match_stylesheet`](matcher::match_stylesheet)).
    SelectorUnprovable,
    /// The scoped render refused (a malformed span/tree shape the renderer
    /// fails closed on instead of panicking).
    RenderInvariant,
}

/// The parsed + analyzed css body — the CSS-DOMAIN half of the plan build,
/// produced by [`analyze_style_body`] BEFORE the runtime IR exists. Carrying
/// it forward into [`complete_style_scope_plan`] keeps the body parsed ONCE
/// while letting the css-analysis diagnostic surface FIRST (a css failure is
/// reported before any template-lowering failure — the css-first diagnostic
/// order). The two halves are one pipeline, not alternative paths.
pub struct AnalyzedStyleBody {
    /// The shared syntax tree — the SOLE parse of the css body.
    tree: StyleSyntaxIr,
    /// The analyzer facts (keyframes/global collection + the `Span`-keyed
    /// selector metadata side table; the matcher verdicts land later, in the
    /// completion stage).
    analysis: analyze::CssAnalysis,
}

/// The CSS-DOMAIN half of the plan build: parse the css body at `content`
/// (absolute offsets into `source`) ONCE and run the scoping analysis. Runs
/// BEFORE template lowering, so a css parse/analysis failure is the FIRST
/// diagnostic a style component reports.
#[cfg(any(test, feature = "test-support"))]
pub fn analyze_style_body(
    source: &str,
    content: Span,
) -> Result<AnalyzedStyleBody, StylePlanFailure> {
    let mut admitted = AdmittedStyleIrs::default();
    analyze_style_body_admitted(source, content, &mut admitted)
}

pub(super) fn analyze_style_body_admitted(
    source: &str,
    content: Span,
    admitted: &mut AdmittedStyleIrs,
) -> Result<AnalyzedStyleBody, StylePlanFailure> {
    verter_audit::attribute_scope!(StyleAnalysis);
    let css = source
        .get(content.start as usize..content.end as usize)
        .ok_or(StylePlanFailure {
            class: StylePlanFailureClass::ParseAnalysis,
            code: "css_expected_identifier",
            span: content,
            construct: None,
        })?;
    let tree = if let Some(admitted) = admitted.take(content) {
        admitted
    } else {
        let syntax_source =
            CssSource::new(Arc::from(css), content.start).map_err(|_| StylePlanFailure {
                class: StylePlanFailureClass::ParseAnalysis,
                code: "css_expected_identifier",
                span: content,
                construct: None,
            })?;
        parse_style_ir(syntax_source, CssDialect::Css, CssParseMode::Recover).map_err(|_| {
            StylePlanFailure {
                class: StylePlanFailureClass::ParseAnalysis,
                code: "css_expected_identifier",
                span: content,
                construct: None,
            }
        })?
    };
    // `analyze_stylesheet` rejects any parse-recovery artifact or
    // dynamic-class/interpolation selector shape inline, at the same visit
    // its single statement walk (`Analyzer::analyze_statements`) analyzes
    // each statement — never a separate shape-validation pre-pass over the
    // SAME tree this function parsed; that rejection surfaces as
    // `svelte-runtime-unsupported-style-selector` and keeps the
    // `SelectorUnprovable` class + `construct` the matcher's own refusal
    // below uses for the same code, distinguishing it from a css-domain
    // parse/placement failure.
    let analysis = analyze::analyze_stylesheet(source, &tree)
        .map_err(|err| {
            if err.code == "svelte-runtime-unsupported-style-selector" {
                StylePlanFailure {
                    class: StylePlanFailureClass::SelectorUnprovable,
                    code: err.code,
                    span: err.span,
                    construct: Some("untrusted-style-syntax-ir"),
                }
            } else {
                StylePlanFailure {
                    class: StylePlanFailureClass::ParseAnalysis,
                    code: err.code,
                    span: err.span,
                    construct: None,
                }
            }
        })?
        .into_analysis();
    Ok(AnalyzedStyleBody { tree, analysis })
}

// ── Allocation probe (test/`test-support`-only) ──
//
// `crates/verter_compiler/tests/allocator_canaries.rs` is a SEPARATE
// integration-test crate: it can only reach `pub` items, and this whole
// `css` module is otherwise private (`mod css;` in `runtime/mod.rs`). These
// two functions — re-exported under the same `test-support` opt-in seam
// `compile_client` uses (`runtime/mod.rs`) — are the narrow bridge that
// binary needs to drive the probe: analyze once, then re-read a compound's
// grammar-gap classification and an at-rule's prelude text through the SAME
// accessors production code (`analyze.rs`/`match_relsel.rs`/`render.rs`)
// uses, asserting the re-read allocates nothing — the executable proof that
// both are parser-minted struct fields, never re-derived from source bytes.

/// Analyze `source`'s `<style>` body at `content` for the allocation probe.
/// Test/guard observability only.
#[cfg(any(test, feature = "test-support"))]
pub fn analyze_style_body_for_alloc_probe(source: &str, content: Span) -> AnalyzedStyleBody {
    analyze_style_body(source, content).expect("a clean body analyzes for the alloc probe")
}

/// Re-read every compound's [`analyze::CompoundTail`] fact and every
/// at-rule's prelude text `analyzed` holds, through the SAME
/// `CssAnalysis::compound_facts` / `StyleDirective::prelude_text` accessors
/// production code uses. `source` is the FULL original component source
/// `analyzed` was built from (the same string passed to
/// [`analyze_style_body_for_alloc_probe`] — `is_keyframes_node` indexes it
/// with the tree's ABSOLUTE spans, not the CSS-body-only substring
/// `analyzed`'s own `StyleSyntaxIr::source()` carries). Test/guard
/// observability only — the caller wraps this call in a counting-allocator
/// measurement window.
#[cfg(any(test, feature = "test-support"))]
pub fn reread_cached_css_facts_for_alloc_probe(source: &str, analyzed: &AnalyzedStyleBody) {
    fn walk(source: &str, analysis: &analyze::CssAnalysis, statements: &[StyleStatement]) {
        for statement in statements {
            match statement {
                StyleStatement::Rule(rule) => {
                    for complex in rule.selector_list().selectors() {
                        for part in complex.parts() {
                            if let ComplexSelectorPart::Compound(compound) = part {
                                std::hint::black_box(analysis.compound_facts(compound));
                            }
                        }
                    }
                    walk(source, analysis, rule.body().statements());
                }
                StyleStatement::AtRule(atrule) => {
                    if analyze::is_keyframes_node(source, atrule) {
                        std::hint::black_box(atrule.prelude_text());
                    }
                    if let Some(block) = atrule.body() {
                        walk(source, analysis, block.statements());
                    }
                }
                StyleStatement::Declaration(_)
                | StyleStatement::MixinOrFunction(_)
                | StyleStatement::Unknown(_) => {}
            }
        }
    }
    walk(source, &analyzed.analysis, analyzed.tree.statements());
}

/// The css body's raw text at `content` (the tree's own recorded source
/// span) sliced out of `source`. `content` is a fact the tree itself
/// carries, so this should always be in bounds — but a caller passing a
/// `source`/`analyzed` pair that does not agree with what was actually
/// parsed (out of bounds, or a span landing off a UTF-8 char boundary) is a
/// malformed-shape condition, not a recoverable default: silently hashing an
/// empty string in its place would produce a wrong-but-plausible-looking
/// scope class instead of surfacing the mismatch. Fails closed with the same
/// `RenderInvariant` class the renderer's own malformed-span refusals use.
fn css_body_text_for(source: &str, content: Span) -> Result<&str, StylePlanFailure> {
    source
        .get(content.start as usize..content.end as usize)
        .ok_or(StylePlanFailure {
            class: StylePlanFailureClass::RenderInvariant,
            code: "css_render_failed",
            span: content,
            construct: None,
        })
}

/// The TEMPLATE-DOMAIN half of the plan build: run the selector-to-template
/// matcher over the component's runtime IR (writing the used/scoped
/// selector metadata + the per-element scope facts), derive the scope hash
/// from the official css-hash input (`filename`, falling back to the raw css
/// text), and render the scoped stylesheet. `mode` is the parse-domain css
/// output mode (recorded on the plan; the emitter selects per mode).
/// `want_source_map` is the css source-map demand: the render generates the
/// map from the SAME shared transform that produced the code and stores it
/// on the plan (`None` without the demand; the rendered bytes are identical
/// either way).
///
/// PROVEN BY CONSTRUCTION: a matcher refusal returns the typed
/// [`SelectorUnprovable`](StylePlanFailureClass::SelectorUnprovable) failure
/// IMMEDIATELY (before any render) — an `Ok` plan can only describe a style
/// whose full selector⇄template relation was proven and rendered.
pub fn complete_style_scope_plan(
    source: &str,
    analyzed: AnalyzedStyleBody,
    filename: Option<&str>,
    resolved_css_hash: Option<&str>,
    mode: CssMode,
    ir: &SvelteRuntimeIr<'_>,
    want_source_map: bool,
) -> Result<ProvenStyleScopePlan, StylePlanFailure> {
    let AnalyzedStyleBody { tree, mut analysis } = analyzed;
    verter_debug_assert_eq!(tree.dialect(), CssDialect::Css);
    let content = Span::new(tree.source().origin(), tree.source().end());
    let facts = matcher::match_stylesheet(source, &tree, &mut analysis, ir).map_err(|refusal| {
        StylePlanFailure {
            class: StylePlanFailureClass::SelectorUnprovable,
            code: "svelte-runtime-unsupported-style-selector",
            span: refusal.span,
            construct: Some(refusal.construct),
        }
    })?;
    let css_text = css_body_text_for(source, content)?;
    // The scope class: a RESOLVED `cssHash` override (the user callback's result,
    // computed OUTSIDE the compiler and preserved byte-exact) REPLACES the default
    // `svelte-<hash>` derivation at this SINGLE construction point; absent, the
    // official default djb2 input rule applies. The override never re-invokes a
    // callback and is never prefixed / re-hashed here.
    let hash = match resolved_css_hash {
        Some(override_class) => override_class.to_string(),
        None => hash::css_scope_hash(filename, css_text),
    };
    // The scoped render consumes the matcher's PROVEN used/scoped verdicts on
    // the analysis side table and produces the official `css.code`. The
    // render is MODE-FAITHFUL: the injected `$$css` payload renders the
    // official minified form (`state.minify = inject_styles && !dev`; Verter
    // refuses dev codegen, so the flag is exactly the mode), the external
    // artifact the non-minified form. A render refusal (a malformed span/tree
    // shape) surfaces as the typed RenderInvariant failure — never a panic,
    // never a partial stylesheet.
    let render = render::render_stylesheet(
        source,
        &tree,
        &analysis,
        &hash,
        &analysis.keyframes,
        matches!(mode, CssMode::Injected),
        filename,
        want_source_map,
    )
    .map_err(|err| StylePlanFailure {
        class: StylePlanFailureClass::RenderInvariant,
        code: "css_render_failed",
        span: err.span,
        construct: None,
    })?;
    Ok(ProvenStyleScopePlan {
        hash,
        css_code: render.code,
        source_map: render.source_map,
        css_body_span: content,
        keyframes: analysis.keyframes,
        global_keyframes: analysis.global_keyframes,
        has_global: analysis.has_global,
        mode,
        facts,
    })
}

/// Test hook: parse + analyze the css body and return the matcher's
/// per-TOP-LEVEL-complex-selector [`MatchCertainty`](matcher::MatchCertainty)
/// rows (prune visit order, `No` rows included) — the tri-state observability
/// behind the production used/scoped projection.
#[cfg(test)]
pub(crate) fn style_selector_certainties_for_test(
    source: &str,
    content: Span,
    ir: &SvelteRuntimeIr<'_>,
) -> Result<Vec<(Span, matcher::MatchCertainty)>, StylePlanFailure> {
    let AnalyzedStyleBody { tree, mut analysis } = analyze_style_body(source, content)?;
    matcher::match_stylesheet_certainties_for_test(source, &tree, &mut analysis, ir).map_err(
        |refusal| StylePlanFailure {
            class: StylePlanFailureClass::SelectorUnprovable,
            code: "svelte-runtime-unsupported-style-selector",
            span: refusal.span,
            construct: Some(refusal.construct),
        },
    )
}

/// Build the per-`<style>` scope plan in one call — [`analyze_style_body`]
/// then [`complete_style_scope_plan`] (the two halves exist so the production
/// pipeline can surface a css-analysis failure BEFORE template lowering; this
/// composition serves the test harnesses that already hold the IR — the
/// production pipeline drives the halves directly).
#[cfg(test)]
pub fn build_style_scope_plan(
    source: &str,
    content: Span,
    filename: Option<&str>,
    mode: CssMode,
    ir: &SvelteRuntimeIr<'_>,
    want_source_map: bool,
) -> Result<ProvenStyleScopePlan, StylePlanFailure> {
    let analyzed = analyze_style_body(source, content)?;
    complete_style_scope_plan(source, analyzed, filename, None, mode, ir, want_source_map)
}

#[cfg(test)]
mod render_tests;

#[cfg(test)]
mod tests {
    use super::types::{CssMode, ProvenStyleScopePlan};
    use super::{
        build_style_scope_plan, css_body_text_for, StylePlanFailure, StylePlanFailureClass,
    };
    use crate::svelte::parser::parse_svelte;
    use crate::svelte::runtime::{lower_parsed_svelte_to_ir, SvelteRuntimeOptions};
    use oxc_allocator::Allocator;
    use verter_span::Span;

    #[test]
    fn css_body_text_for_a_span_within_bounds_slices_cleanly() {
        assert_eq!(css_body_text_for("abcdef", Span::new(1, 4)).unwrap(), "bcd");
    }

    #[test]
    fn css_body_text_for_an_out_of_bounds_span_fails_closed() {
        // A span whose end exceeds `source`'s length — the mismatch this
        // guards against (the tree's own recorded span disagreeing with the
        // `source` string actually passed in).
        let err = css_body_text_for("abc", Span::new(0, 10))
            .expect_err("an out-of-bounds span must fail closed, not silently yield \"\"");
        assert_eq!(err.class, StylePlanFailureClass::RenderInvariant);
        assert_eq!(err.code, "css_render_failed");
        assert_eq!(err.span, Span::new(0, 10));
    }

    #[test]
    fn css_body_text_for_a_non_char_boundary_span_fails_closed() {
        // "é" is 2 bytes (0xC3 0xA9); index 1 lands mid-character.
        let err = css_body_text_for("é", Span::new(1, 2))
            .expect_err("a non-char-boundary span must fail closed, not silently yield \"\"");
        assert_eq!(err.class, StylePlanFailureClass::RenderInvariant);
        assert_eq!(err.code, "css_render_failed");
    }

    fn body_span(source: &str) -> Span {
        let start = source.find("<style>").expect("open tag") + "<style>".len();
        let end = source.rfind("</style>").expect("close tag");
        Span::new(start as u32, end as u32)
    }

    /// Lower the component and build its scope plan (the production wiring:
    /// parse → analyze → match over the runtime IR).
    fn plan_for(
        source: &str,
        filename: Option<&str>,
    ) -> Result<ProvenStyleScopePlan, StylePlanFailure> {
        let alloc = Allocator::default();
        let parsed = parse_svelte(source);
        let opts = SvelteRuntimeOptions {
            filename: filename.map(str::to_string),
            ..Default::default()
        };
        let ir =
            lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc).expect("lowering succeeds");
        build_style_scope_plan(
            source,
            body_span(source),
            filename,
            CssMode::External,
            &ir,
            false,
        )
    }

    #[test]
    fn plan_assembles_hash_span_keyframes_mode_and_matcher_facts() {
        let source = "<div class=\"card\">x</div>\n<style>@keyframes spin { from { opacity: 0 } }\n.card { color: blue; }</style>\n";
        let plan = plan_for(source, Some("css/scoped_styles.svelte")).expect("a clean body plans");
        // The hash is the oracle-pinned filename hash.
        assert_eq!(plan.hash, "svelte-c4vjvh");
        assert_eq!(plan.css_body_span, body_span(source));
        assert_eq!(plan.keyframes.len(), 1);
        assert_eq!(plan.keyframes[0].name, "spin");
        assert!(plan.global_keyframes.is_empty());
        assert!(!plan.has_global);
        assert_eq!(
            plan.source_map, None,
            "an undemanded css map never lands on the plan"
        );
        assert_eq!(plan.mode, CssMode::External);
        // The matcher ran: `.card` matched the `<div>` — one scoped element.
        // A constructed plan carries its PROVEN facts directly (no outcome
        // state).
        assert_eq!(plan.facts.scoped.len(), 1);
        let scope = plan.scope_facts();
        assert_eq!(scope.hash, plan.hash);
        assert_eq!(scope.scoped, plan.facts.scoped);
        assert!(
            plan.css_code.contains(&format!(".card.{}", plan.hash)),
            "the scoped `.card` rule carries the scope class in the rendered output"
        );
    }

    #[test]
    fn plan_hash_falls_back_to_the_css_text_without_a_filename() {
        let source = "<style>.a{color:red}</style>";
        let plan = plan_for(source, None).expect("a clean body plans");
        // hash(css) over the RAW body text — the official `filename ?? css`
        // fallback.
        let expected = format!(
            "svelte-{}",
            crate::svelte::runtime::naming::svelte_hash(".a{color:red}")
        );
        assert_eq!(plan.hash, expected);
        // An empty template: `.a` matches nothing — unused, no scoped
        // elements, still a PROVEN plan (supported-empty, not a failure).
        assert!(plan.facts.scoped.is_empty());
        assert!(plan.scope_facts().scoped.is_empty());
    }

    #[test]
    fn plan_surfaces_parse_and_analysis_failures_as_typed_errors() {
        // A PARSE failure.
        let source = "<style>.1bad { color: red }</style>";
        let err = plan_for(source, None).expect_err("a malformed body fails the plan");
        assert_eq!(
            err,
            StylePlanFailure {
                class: StylePlanFailureClass::ParseAnalysis,
                code: "css_expected_identifier",
                span: Span::new(
                    source.find("1bad").unwrap() as u32,
                    source.find("1bad").unwrap() as u32
                ),
                construct: None,
            }
        );
        // An ANALYSIS failure.
        let source = "<style>.a :global(.x) .b { color: red }</style>";
        let err = plan_for(source, None).expect_err("an invalid :global placement fails the plan");
        assert_eq!(err.class, StylePlanFailureClass::ParseAnalysis);
        assert_eq!(err.code, "css_global_invalid_placement");
    }

    #[test]
    fn unprovable_template_fails_the_plan_with_a_selector_unprovable_failure() {
        // Proven by construction: an unprovable selector⇄template relation
        // can NEVER produce an `Ok` plan — the plan type carries no
        // unprovable state; the refusal is the typed `Err` with the
        // construct's EXACT span and description.
        let source =
            "<svelte:head><title>t</title></svelte:head>\n<style>.card { color: red; }</style>";
        let err = plan_for(source, None).expect_err("a head-`<title>` template cannot prove");
        assert_eq!(err.class, StylePlanFailureClass::SelectorUnprovable);
        assert_eq!(err.code, "svelte-runtime-unsupported-style-selector");
        let head = source.find("<svelte:head>").unwrap() as u32;
        assert_eq!(
            err.span,
            Span::new(head, head + "<svelte:head>".len() as u32)
        );
        assert!(
            err.construct
                .expect("a matcher refusal names its construct")
                .contains("<title>"),
            "the construct description names the unprovable construct"
        );
    }

    #[test]
    fn render_refusal_fails_the_plan_with_a_render_invariant_failure() {
        // The `RenderError → StylePlanFailure` mapping: a render refusal
        // surfaces as the typed `RenderInvariant` failure — never a panic,
        // never a partial plan. A CSS-escaped `:global` keyword
        // (`:\67 lobal(.x)`, decoding to `:global(...)` for parse/analyze/
        // match purposes) is a NATURAL render-stage refusal: the renderer's
        // `:global(...)` removal anchor adds the literal keyword's BYTE
        // length, which desyncs from the escaped spelling — oracle-confirmed
        // against svelte@5.56.10 itself mangling this shape (see
        // `render.rs`'s `remove_global_pseudo_class` doc).
        let source = "<div class=\"x\">y</div>\n<style>:\\67 lobal(.x){color:red}</style>";
        let err = plan_for(source, None).expect_err("an escaped :global keyword fails closed");
        assert_eq!(err.class, StylePlanFailureClass::RenderInvariant);
        assert_eq!(err.code, "css_render_failed");
    }

    // ── single-parse-per-style-block call-count proof ──
    //
    // The counter lives INSIDE `verter_css_syntax::parse_style_ir` itself
    // (`parse_style_ir_thread_invocations`) — the SOLE parse entry point
    // every caller goes through — so it observes every call to that
    // function, not just the one this pipeline's own call site happens to
    // make. `parse_style_ir_thread_invocations_counts_every_call_site`
    // below proves that directly: a second, unrelated `parse_style_ir` call
    // moves the same counter these two tests read.

    #[test]
    fn single_parse_per_style_block_call_count() {
        // A full plan build over ONE `<style>` block performs exactly ONE
        // `parse_style_ir` call.
        let before = verter_css_syntax::parse_style_ir_thread_invocations();
        let source = "<div class=\"card\">x</div>\n<style>.card { color: red; }</style>";
        plan_for(source, None).expect("a clean body plans");
        assert_eq!(
            verter_css_syntax::parse_style_ir_thread_invocations() - before,
            1,
            "exactly one shared-grammar parse per style-block plan build"
        );
    }

    // ── the converged pipeline hides no second parse/reconstruct ──
    //
    // SCOPE NOTE: this is J1-A11e's registered acceptance test (J1.md:311),
    // proving row 5's convergence (Svelte's OWN former grammar —
    // `parse.rs`/`types.rs`/`analyze.rs`/`match.rs`/`hash.rs`/`render.rs` —
    // into `parse_style_ir`) hides no admission-time reparse, over the
    // `analyze_style_body` → `complete_style_scope_plan` pipeline `plan_for`
    // exercises. J1-A16's reject-gate parse is covered by
    // `compile_client_parses_a_style_body_once` below, which drives the
    // production `official_reject_gate` → `analyze_style_body` path.

    #[test]
    fn svelte_convergence_introduces_no_hidden_second_parse_or_reconstruct() {
        // TWO independent observations over a MULTI-CONSTRUCT stylesheet
        // (nested rules, an at-rule, a pseudo-class argument list,
        // `:global`) that touches every analyze/match/render recursion path
        // this convergence introduced.
        //
        // (1) PARSE count: exactly one `parse_style_ir` per plan build.
        //
        // (2) RECONSTRUCTION count: ZERO `CssSource::slice_tokens` calls.
        //     The parse counter alone cannot see the second half of the
        //     claim — "no reconstruct-then-reparse". A pipeline that
        //     materialises the css text back out of the token stream and
        //     then RE-SCANS it (rather than re-feeding it to
        //     `parse_style_ir`) leaves the parse count at exactly 1 while
        //     doing precisely the duplicated work this criterion forbids.
        //     `slice_tokens` is the sole allocating token-stream-to-`String`
        //     materialisation `verter_css_syntax` offers (and the only thing
        //     `LosslessCst::reconstruct` is built on), so requiring a delta
        //     of zero observes the reconstruction directly — and, because
        //     each call is one whole-source `String::with_capacity`, it is
        //     also the allocation this claim is about.
        let parses_before = verter_css_syntax::parse_style_ir_thread_invocations();
        let reconstructions_before = verter_css_syntax::css_source_token_reconstructions();
        let source = "<div class=\"card\"><p class=\"title\">x</p></div>\n<style>\
            @media (min-width: 1px) { .card :is(.title, .other) { color: red; } }\n\
            .card { :global(.x) { color: blue; } }\n\
            @keyframes spin { from { opacity: 0; } }\n\
            .card { animation: spin; }\
            </style>";
        plan_for(source, None).expect("a multi-construct body plans");
        assert_eq!(
            verter_css_syntax::parse_style_ir_thread_invocations() - parses_before,
            1,
            "exactly one shared-grammar parse regardless of construct diversity"
        );
        assert_eq!(
            verter_css_syntax::css_source_token_reconstructions() - reconstructions_before,
            0,
            "the converged pipeline must never rebuild the css text out of the token \
             stream — a reconstruct-then-rescan hides behind a parse count of exactly 1"
        );
    }

    /// Discrimination companion for the reconstruction half, mirroring
    /// `parse_style_ir_thread_invocations_counts_every_call_site`: the
    /// reconstruction counter lives inside `CssSource::slice_tokens` itself,
    /// so an unrelated reconstruction anywhere on this thread moves it. A
    /// zero-delta assertion is therefore evidence, not a counter that never
    /// moves at all.
    #[test]
    fn css_source_token_reconstructions_counts_every_call_site() {
        use std::sync::Arc;
        use verter_css_syntax::{
            parse_with_sink, CssDialect, CssEntryPoint, CssParseMode, CssSource, LosslessCstSink,
        };

        let before = verter_css_syntax::css_source_token_reconstructions();
        let source = "<div class=\"card\">x</div>\n<style>.card { color: red; }</style>";
        plan_for(source, None).expect("a clean body plans");
        assert_eq!(
            verter_css_syntax::css_source_token_reconstructions() - before,
            0,
            "the pipeline itself reconstructs nothing"
        );

        // A direct, unrelated reconstruction — nothing to do with the Svelte
        // pipeline — must move the SAME counter, proving the zero-delta
        // assertions above are capable of failing. Uses the public gateway
        // plus `LosslessCstSink` rather than the crate-private `parse_lossless`
        // convenience, so the witness stays valid after that route is sealed.
        let css = CssSource::new(Arc::from(".x { color: red; }"), 0).unwrap();
        let mut sink = LosslessCstSink::new(css.clone());
        parse_with_sink(
            &css,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
            &mut sink,
        )
        .expect("a clean body parses");
        let cst = sink.finish().expect("cst finishes");
        assert_eq!(cst.reconstruct(), ".x { color: red; }");
        assert_eq!(
            verter_css_syntax::css_source_token_reconstructions() - before,
            1,
            "an unrelated token-stream reconstruction also moves the counter"
        );
    }

    #[test]
    fn compile_client_parses_a_style_body_once() {
        // Production path: official_reject_gate admits the IR, then
        // analyze_style_body reuses it. Two grammars or a second
        // parse_style_ir would make this 2 (or more).
        use crate::svelte::runtime::compile_client;
        let before = verter_css_syntax::parse_style_ir_thread_invocations();
        let source = "<div class=\"x\"></div>\n<style>.x { color: red }</style>";
        let alloc = Allocator::default();
        let parsed = parse_svelte(source);
        compile_client(
            source,
            &parsed,
            &SvelteRuntimeOptions {
                filename: Some("App.svelte".to_string()),
                ..Default::default()
            },
            &alloc,
            false,
            false,
        )
        .expect("a clean component compiles");
        assert_eq!(
            verter_css_syntax::parse_style_ir_thread_invocations() - before,
            1,
            "official-reject + style pipeline share one parse_style_ir"
        );
    }

    // ── discrimination proof: the counter observes EVERY call site ──

    #[test]
    fn parse_style_ir_thread_invocations_counts_every_call_site() {
        // Unlike a counter a caller bumps beside its own call site (which
        // only proves that ONE call site ran once), this counter lives
        // inside `parse_style_ir` itself — so a completely unrelated,
        // direct call to it (bypassing `analyze_style_body`/`plan_for`
        // entirely) must ALSO move it. This is what makes
        // `single_parse_per_style_block_call_count` and
        // `svelte_convergence_introduces_no_hidden_second_parse_or_reconstruct`
        // able to actually catch a hidden second parse anywhere in the
        // pipeline, rather than only re-observing their own one known call.
        use std::sync::Arc;
        use verter_css_syntax::{parse_style_ir, CssDialect, CssParseMode, CssSource};

        let before = verter_css_syntax::parse_style_ir_thread_invocations();
        let source = "<div class=\"card\">x</div>\n<style>.card { color: red; }</style>";
        plan_for(source, None).expect("a clean body plans");
        assert_eq!(
            verter_css_syntax::parse_style_ir_thread_invocations() - before,
            1,
            "the pipeline's own one call moved the counter by exactly one"
        );

        // A direct, unrelated `parse_style_ir` call — nothing to do with
        // `analyze_style_body` — must move the SAME counter.
        let extra_source = CssSource::new(Arc::from(".x { color: red; }"), 0).unwrap();
        parse_style_ir(extra_source, CssDialect::Css, CssParseMode::Recover)
            .expect("a clean body parses");
        assert_eq!(
            verter_css_syntax::parse_style_ir_thread_invocations() - before,
            2,
            "a second, unrelated parse_style_ir call also moves the counter — proving \
             the two tests above would catch a hidden second parse anywhere"
        );
    }
}
