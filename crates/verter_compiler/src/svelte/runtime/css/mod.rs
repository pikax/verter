//! The Svelte-OWNED CSS substrate: span-bearing body parse, scope-hash
//! derivation, and scoping analysis for component `<style>` blocks.
//!
//! This module owns the CSS DOMAIN of the Svelte runtime pipeline. It is
//! fully separate from the Vue style pipeline (`crate::css`) — the Svelte
//! scoping semantics are a faithful port of the official `svelte@5.56.3`
//! compiler (`phases/1-parse/read/style.js`, `phases/2-analyze/css/*`,
//! `phases/css.js`), operating on byte spans of the ORIGINAL component
//! source so downstream source-position edits map exactly.
//!
//! Pipeline: [`parse::parse_style_body`] builds the span-bearing AST →
//! [`analyze::analyze_stylesheet`] populates selector metadata + collects
//! keyframes/global facts → [`matcher::match_stylesheet`] runs the
//! selector-to-template matcher (the `css-prune.js` port) over the runtime
//! IR, marking the used/scoped selector verdicts and producing the
//! per-element scope facts → [`hash::css_scope_hash`] derives the
//! `svelte-<djb2>` scope hash → [`render::render_stylesheet`] produces the
//! scoped stylesheet text (the official `css.code`) by source-position edits
//! over the ORIGINAL component source → the facts assemble into the
//! per-`<style>` [`ProvenStyleScopePlan`](types::ProvenStyleScopePlan) side
//! table (the ONE shared fact both scope-class injection sites and the css
//! emitter read). Every failure mode — a css parse/analysis failure, an
//! unprovable selector⇄template relation, a render refusal — is the typed
//! [`StylePlanFailure`]: a plan value exists ONLY for a fully-proven style.

pub mod analyze;
pub mod hash;
#[path = "match.rs"]
pub mod matcher;
pub mod parse;
pub mod render;
pub mod types;

use verter_span::Span;

use super::ir::SvelteRuntimeIr;
use types::{CssMode, ProvenStyleScopePlan, StyleSheet};

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
    /// The scoped render refused (a malformed span/AST shape the renderer
    /// fails closed on instead of panicking).
    RenderInvariant,
}

/// The parsed + analyzed css body — the CSS-DOMAIN half of the plan build,
/// produced by [`analyze_style_body`] BEFORE the runtime IR exists. Carrying
/// it forward into [`complete_style_scope_plan`] keeps the body parsed once
/// while letting the css-analysis diagnostic surface FIRST (a css failure is
/// reported before any template-lowering failure — the css-first diagnostic
/// order). The two halves are one pipeline, not alternative paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedStyleBody {
    /// The span-bearing CSS AST with the ANALYZER metadata populated (the
    /// matcher verdicts land later, in the completion stage).
    ast: StyleSheet,
    /// The analyzer facts (keyframes / global collection).
    analysis: analyze::CssAnalysis,
}

/// The CSS-DOMAIN half of the plan build: parse the css body at `content`
/// (absolute offsets into `source`) and run the scoping analysis. Runs BEFORE
/// template lowering, so a css parse/analysis failure is the FIRST diagnostic
/// a style component reports.
pub fn analyze_style_body(
    source: &str,
    content: Span,
) -> Result<AnalyzedStyleBody, StylePlanFailure> {
    let mut ast = parse::parse_style_body(source, content).map_err(|err| StylePlanFailure {
        class: StylePlanFailureClass::ParseAnalysis,
        code: err.code,
        span: err.span,
        construct: None,
    })?;
    let analysis =
        analyze::analyze_stylesheet(source, &mut ast).map_err(|err| StylePlanFailure {
            class: StylePlanFailureClass::ParseAnalysis,
            code: err.code,
            span: err.span,
            construct: None,
        })?;
    Ok(AnalyzedStyleBody { ast, analysis })
}

/// The TEMPLATE-DOMAIN half of the plan build: run the selector-to-template
/// matcher over the component's runtime IR (populating the used/scoped
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
    mode: CssMode,
    ir: &SvelteRuntimeIr<'_>,
    want_source_map: bool,
) -> Result<ProvenStyleScopePlan, StylePlanFailure> {
    let AnalyzedStyleBody { mut ast, analysis } = analyzed;
    let content = ast.span;
    let facts = matcher::match_stylesheet(&mut ast, ir).map_err(|refusal| StylePlanFailure {
        class: StylePlanFailureClass::SelectorUnprovable,
        code: "svelte-runtime-unsupported-style-selector",
        span: refusal.span,
        construct: Some(refusal.construct),
    })?;
    let css_text = source
        .get(content.start as usize..content.end as usize)
        .unwrap_or("");
    let hash = hash::css_scope_hash(filename, css_text);
    // The scoped render consumes the matcher's PROVEN used/scoped verdicts on
    // the AST metadata and produces the official `css.code`. The render is
    // MODE-FAITHFUL: the injected `$$css` payload renders the official
    // minified form (`state.minify = inject_styles && !dev`; Verter refuses
    // dev codegen, so the flag is exactly the mode), the external artifact
    // the non-minified form. A render refusal (a malformed span/AST shape)
    // surfaces as the typed RenderInvariant failure — never a panic, never a
    // partial stylesheet.
    let render = render::render_stylesheet(
        source,
        &ast,
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
        ast,
        facts,
    })
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
    complete_style_scope_plan(source, analyzed, filename, mode, ir, want_source_map)
}

#[cfg(test)]
mod render_tests;

#[cfg(test)]
mod tests {
    use super::types::{CssMode, ProvenStyleScopePlan};
    use super::{build_style_scope_plan, StylePlanFailure, StylePlanFailureClass};
    use crate::svelte::parser::parse_svelte;
    use crate::svelte::runtime::{lower_parsed_svelte_to_ir, SvelteRuntimeOptions};
    use oxc_allocator::Allocator;
    use verter_span::Span;

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
        assert_eq!(plan.ast.children.len(), 2);
        // The matcher ran: `.card` matched the `<div>` — one scoped element,
        // and the selector is marked used on the AST metadata. A constructed
        // plan carries its PROVEN facts directly (no outcome state).
        assert_eq!(plan.facts.scoped.len(), 1);
        let scope = plan.scope_facts();
        assert_eq!(scope.hash, plan.hash);
        assert_eq!(scope.scoped, plan.facts.scoped);
        let super::types::StyleChild::Rule(rule) = &plan.ast.children[1] else {
            panic!("the second child is the `.card` rule");
        };
        assert!(rule.prelude.children[0].metadata.used);
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
        let source = "<slot></slot>\n<style>.card { color: red; }</style>";
        let err = plan_for(source, None).expect_err("a `<slot>` template cannot prove");
        assert_eq!(err.class, StylePlanFailureClass::SelectorUnprovable);
        assert_eq!(err.code, "svelte-runtime-unsupported-style-selector");
        let slot = source.find("<slot>").unwrap() as u32;
        assert_eq!(err.span, Span::new(slot, slot + "<slot>".len() as u32));
        assert!(
            err.construct
                .expect("a matcher refusal names its construct")
                .contains("<slot>"),
            "the construct description names the unprovable construct"
        );
    }

    #[test]
    fn render_refusal_fails_the_plan_with_a_render_invariant_failure() {
        // The `RenderError → StylePlanFailure` mapping: a render refusal (a
        // malformed selector span the renderer refuses) surfaces as the typed
        // `RenderInvariant` failure carrying `css_render_failed` + the
        // offending span — never a panic, never a partial plan.
        let source = "<div class=\"b\">x</div>\n<style>.a,.b { color: red; }</style>";
        let alloc = Allocator::default();
        let parsed = parse_svelte(source);
        let opts = SvelteRuntimeOptions::default();
        let ir =
            lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc).expect("lowering succeeds");
        let mut analyzed =
            super::analyze_style_body(source, body_span(source)).expect("the body analyzes");
        // Corrupt the `.b` complex selector's span out of range: the matcher
        // still proves (`.b` matches the div; `.a` stays unused), then the
        // unused-run prune back-scan hits the malformed span and refuses.
        {
            let super::types::StyleChild::Rule(rule) = &mut analyzed.ast.children[0] else {
                panic!("the sheet's child is a rule");
            };
            rule.prelude.children[1].span = Span::new(10_000, 10_002);
        }
        let err =
            super::complete_style_scope_plan(source, analyzed, None, CssMode::External, &ir, false)
                .expect_err("a render refusal fails the plan");
        assert_eq!(
            err,
            StylePlanFailure {
                class: StylePlanFailureClass::RenderInvariant,
                code: "css_render_failed",
                span: Span::new(10_000, 10_002),
                construct: None,
            }
        );
    }
}
