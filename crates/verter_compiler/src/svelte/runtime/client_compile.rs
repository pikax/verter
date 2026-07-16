//! The carrier-facing client compile ENTRY of the Svelte runtime backend.
//!
//! Extracted from the runtime module root (the file-size guard boundary):
//! [`compile_client`] — the REFUSE-BY-DEFAULT pipeline driver (official-reject
//! gate → parse-domain gate → runtime lowering → default-deny classification →
//! narrow-plan projection → static-template + topology planning → emission) —
//! and its typed [`ClientCompileError`] outcome.

use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::svelte::parser::ParsedSvelte;
use verter_span::Span;

use super::client;
use super::client_plan;
use super::client_surface;
use super::official_reject;
use super::official_rule::OfficialRejection;
use super::parse_refusal::parse_domain_gate;
use super::{
    lower_parsed_svelte_to_ir, plan_client_topology, resolve_svelte_compile_options,
    RuntimeLoweringErrors, SvelteRuntimeOptions, UnsupportedSvelteRuntimeSurface,
};

/// The exact invariant that prevented a generated client source map from being
/// published. These are backend defects, not authored-source diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedSourceMapInvalidReason {
    EmptyGeneratedRange,
    GeneratedRangeOutOfBounds,
    GeneratedRangeNotUtf8Boundary,
    OverlappingGeneratedRanges,
    SourceOffsetOutOfBounds,
    SourceOffsetNotUtf8Boundary,
    GeneratedCodeChanged,
}

/// The outcome of [`compile_client`] when the client module cannot be emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCompileError {
    /// The runtime lowering itself failed (a malformed construct) — carries the
    /// collected lowering diagnostics.
    Lowering(RuntimeLoweringErrors),
    /// The component uses a runtime surface this backend does not yet emit — fails
    /// closed with the typed reason (never a silent empty module).
    Unsupported(UnsupportedSvelteRuntimeSurface),
    /// The component is MALFORMED Svelte the official `svelte@5.56.3` compiler also
    /// COMPILE-ERRORS (a duplicate declaration, a `$`-prefixed binding, a duplicate /
    /// mis-`context`-ed `<script>`, an invalid HTML placement, a global `$foo`
    /// reference). Accepting it would change the observable contract from "compile
    /// error, no module" to "module exists", so it fails closed with the typed
    /// official-reject rejection (the rule class + its exact official code) — never a
    /// `Main`. The official-reject parity quadrant.
    OfficialReject(OfficialRejection),
    /// The backend produced syntactically invalid JavaScript. This is an
    /// internal codegen invariant failure and therefore fails closed before the
    /// module can reach a host or cache.
    GeneratedModuleInvalid { diagnostic_count: usize },
    /// The backend accumulated an invalid authored-to-generated mapping. This is
    /// an internal codegen invariant failure; the module is refused instead of
    /// silently publishing incomplete or corrupt provenance.
    GeneratedSourceMapInvalid {
        reason: GeneratedSourceMapInvalidReason,
    },
}

impl From<UnsupportedSvelteRuntimeSurface> for ClientCompileError {
    /// Convert a REWRITER-CHANNEL refusal to the client compile error. The fallible
    /// expression rewriter reports through `UnsupportedSvelteRuntimeSurface`, but a
    /// [`UnsupportedSvelteRuntimeSurface::OfficialReject`] carrier is a mid-rewrite
    /// OFFICIAL compile-error (a `$$`-member of a `$props()` rest / whole-object
    /// binding → `props_illegal_name`), so it becomes a real
    /// [`ClientCompileError::OfficialReject`] carrying the exact official code — NOT
    /// the generic unsupported quadrant. Every other surface is a genuine
    /// unsupported feature.
    fn from(surface: UnsupportedSvelteRuntimeSurface) -> Self {
        match surface {
            UnsupportedSvelteRuntimeSurface::OfficialReject { rejection, .. } => {
                ClientCompileError::OfficialReject(rejection)
            }
            other => ClientCompileError::Unsupported(other),
        }
    }
}

/// Compile a parsed Svelte component into the `svelte/internal/client` JS module
/// (the carrier-facing entry).
///
/// Runs the full pipeline — runtime lowering → static-template planning →
/// client-topology planning → [`emit_client_module`] — and returns the emitted
/// module, or a typed [`ClientCompileError`] (a lowering failure, or an
/// unsupported surface that fails closed). `ssr` requests the server backend
/// (fails closed until the server backend lands). `want_source_map` is the
/// css source-map demand (the carrier's `RuntimeCompileOptions::source_map`
/// output axis, NOT a lowering option): the scoped render generates
/// `css.map` from the same shared transform that produced `css.code` and the
/// external artifact carries it.
pub fn compile_client<'a>(
    source: &'a str,
    parsed: &ParsedSvelte,
    opts: &SvelteRuntimeOptions,
    alloc: &'a Allocator,
    ssr: bool,
    want_source_map: bool,
) -> Result<client::ClientModule, ClientCompileError> {
    // The REFUSE-BY-DEFAULT pipeline. Each stage is a choke point: an unsupported
    // surface fails closed BEFORE the next stage, so the narrow plan the emitter
    // consumes can ONLY describe a fully-supported component — emit-by-default is
    // structurally impossible.
    //
    // (0) SSR requests the server backend (fails closed until it lands).
    if ssr {
        return Err(ClientCompileError::Unsupported(
            UnsupportedSvelteRuntimeSurface::ServerGenerate {
                span: Span::new(0, 0),
            },
        ));
    }
    // (1) `official_reject_gate` — the OFFICIAL-REJECT parity gate. Refuse the
    // MALFORMED-input classes official ALSO compile-errors (a duplicate / mis-context
    // `<script>`, a `$`-prefixed binding, a duplicate accepted declaration, a global
    // `$foo` / `$$foo` reference, an invalid HTML placement) FIRST, so a genuinely
    // malformed component is rejected for being malformed — not later mis-attributed
    // to an unsupported feature, and never accepted as a divergent `Main`.
    if let Some(rejection) =
        official_reject::official_reject_gate_with_runes(source, parsed, opts.runes)
    {
        return Err(ClientCompileError::OfficialReject(rejection));
    }
    // (1b) Resolve the compile options ONCE — the SOLE fold point of the compile-option
    // side with the inline `<svelte:options>` attributes (INLINE WINS per admitted key).
    // It runs AFTER the official-reject gate, so it only ever folds an already-ACCEPTED
    // options element (every malformed form was rejected upstream with its exact code),
    // and it fails closed (NO module) on an unsupported feature option
    // (`compatibility.componentApi: 4` / `hmr` / `accessors` / `immutable`). The resolved
    // object threads into the per-option codegen consumers via the IR's `root_options`.
    let resolved = resolve_svelte_compile_options(source, parsed, opts)
        .map_err(ClientCompileError::Unsupported)?;
    // (2) `parse_domain_gate` — refuse the PARSE-DOMAIN surfaces the runtime IR
    // does not carry (a `<style>` with an unprovable css output mode or a failed
    // css analysis — css analysis runs before template lowering, so a css
    // diagnostic is reported ahead of a template-lowering failure — a
    // `<svelte:options>` axis beyond `runes` / `customElement` / `css="injected"`,
    // or a dev-mode codegen request) BEFORE lowering, so a lossy lowering cannot
    // hide them. A VALID `customElement` value passes
    // this gate and is LOWERED by the native client path (the parser-retained
    // descriptor); only its invalid forms remain rejected — as exact-code
    // official rejects by the gate above, never here. An ACCEPTED `<style>`
    // hands its parsed + analyzed body forward as the pre-lowering style stage.
    let prepared_style =
        parse_domain_gate(source, parsed, opts).map_err(ClientCompileError::Unsupported)?;
    // (3) Lower to the BROAD runtime IR (the shared substrate). The broad IR may
    // exist; it just never reaches emission.
    let mut ir = lower_parsed_svelte_to_ir(source, parsed, opts, alloc)
        .map_err(ClientCompileError::Lowering)?;
    // Thread the resolved compile-options into the IR the codegen consumers read
    // (the fragments strategy, the whitespace + comment retention, the disclose-version
    // import toggle). The namespace is html-only (a non-`html` namespace was refused at
    // the resolver), so no namespace value is threaded. The component name
    // (`derive_component_name`) and the `cssHash` override route through lowering / the
    // style plan respectively, so they are not carried here.
    ir.root_options = super::ir::RootCompileOptions {
        fragments: resolved.fragments,
        preserve_whitespace: resolved.preserve_whitespace,
        preserve_comments: resolved.preserve_comments,
        disclose_version: resolved.disclose_version,
    };
    // (3b) Complete the per-`<style>` scope plan over the REAL runtime IR: the
    // selector-to-template matcher marks the used/scoped verdicts and the scoped
    // render produces the official `css.code`. The plan is PROVEN BY
    // CONSTRUCTION — every failure is the typed `StylePlanFailure`, mapped by
    // its class onto the precise refusal surface with the failure's own
    // code + span (+ construct) threaded unchanged: a matcher refusal fails
    // closed on the selector surface (never a guessed scope); an analysis or
    // render failure fails closed on the css-analysis surface.
    let style_plan = match prepared_style {
        Some(prepared) => Some(
            super::css::complete_style_scope_plan(
                source,
                prepared.analyzed,
                opts.filename.as_deref(),
                opts.css_hash_override.as_deref(),
                prepared.mode,
                &ir,
                want_source_map,
            )
            .map_err(|failure| {
                ClientCompileError::Unsupported(match failure.class {
                    super::css::StylePlanFailureClass::SelectorUnprovable => {
                        UnsupportedSvelteRuntimeSurface::StyleSelectorUnsupported {
                            code: failure.code,
                            span: failure.span,
                            construct: failure.construct,
                        }
                    }
                    super::css::StylePlanFailureClass::ParseAnalysis
                    | super::css::StylePlanFailureClass::RenderInvariant => {
                        UnsupportedSvelteRuntimeSurface::StyleCssAnalysis {
                            code: failure.code,
                            span: failure.span,
                        }
                    }
                })
            })?,
        ),
        None => None,
    };
    // The ONE scope-injection fact pair (hash + scoped NodeIds) EVERY injection
    // site reads — the static skeleton bake, the `$.set_class` threading, and
    // the spread `$.attribute_effect` hash argument all derive from this single
    // value, so the sites cannot disagree. `Option` ONLY because the whole
    // style block may be absent — a present plan always has proven facts.
    let scope_facts = style_plan
        .as_ref()
        .map(super::css::types::ProvenStyleScopePlan::scope_facts);
    // (4) `ClientSyntaxSurface::classify` — the DEFAULT-DENY classifier. It accepts
    // ONLY when every node / attr / script-item is in the supported allowlist; the
    // first unsupported surface fails closed (no wildcard accept arm). The
    // discriminating `From` conversion routes a classifier-detected OFFICIAL
    // reject (the `OfficialReject` carrier — e.g. a runes-mode `export let` /
    // `$:` detected against the FINAL inferred mode) to
    // `ClientCompileError::OfficialReject`, everything else to `Unsupported`.
    let classified =
        client_surface::ClientSyntaxSurface::classify(&ir).map_err(ClientCompileError::from)?;
    // (5) `SupportedClientIr::build` — the semantic projection. It decides which
    // interpolations are ACTUALLY reactive (a non-reactive one fails closed),
    // validates lvalues, and rewrites every script item + op through the FALLIBLE
    // rewriter into the NARROW `ClientModulePlan`.
    // The rewriter reports through `UnsupportedSvelteRuntimeSurface`; the
    // discriminating `From` conversion routes a mid-rewrite official-reject carrier
    // (a `$$`-member of a rest / whole-object binding) to
    // `ClientCompileError::OfficialReject`, everything else to `Unsupported`.
    let plan = client_plan::SupportedClientIr::build(&classified, &ir, scope_facts.clone())
        .map_err(ClientCompileError::from)?;
    // (6) Plan the static templates + topology, then emit from the NARROW plan
    // only. The scoped css routes per the plan's output mode (the official
    // `inject_styles` routing): EXTERNAL css becomes the module's separate css
    // artifact (the carrier publishes it); INJECTED css (options
    // `css="injected"` / custom element) inlines into the module as the hoisted
    // `$$css` + `$.append_styles` prelude and produces NO external artifact.
    let html_plan =
        super::html::plan_static_templates_for_client(&ir, scope_facts.as_ref(), &plan.nodes);
    let topology = plan_client_topology(&ir, &html_plan, scope_facts.as_ref());
    let (external_css, injected_css) = match &style_plan {
        Some(style) => {
            let artifact = client::ScopedCssArtifact {
                hash: style.hash.clone(),
                code: style.css_code.clone(),
                source_map: style.source_map.clone(),
                has_global: style.has_global,
            };
            match style.mode {
                // An EXTERNAL artifact publishes whenever the style block
                // EXISTS — official `compiled.css` is NON-null even for an
                // EMPTY rendered `css.code` (`<style></style>` compiles to
                // `{ code: '', hasGlobal: false, map }`). Only the ABSENCE of
                // a `<style>` block publishes none (the `None`-plan arm
                // below — official `compiled.css === null`).
                super::css::types::CssMode::External => (Some(artifact), None),
                // The INJECTED `$$css` emits whenever the style EXISTS — the
                // official transform hoists it even with an empty minified
                // payload (`analysis.css.ast !== null && inject_styles`).
                super::css::types::CssMode::Injected => (None, Some(artifact)),
            }
        }
        None => (None, None),
    };
    let mut module = client::emit_client_module(
        source,
        &plan,
        &html_plan,
        &topology,
        client::ClientEmitOptions {
            injected_css: injected_css.as_ref(),
            filename: opts.filename.as_deref(),
            want_source_map,
        },
        alloc,
    )?;
    validate_generated_client_module(&module.code, alloc)?;
    module.css = external_css;
    Ok(module)
}

/// Parse the final generated client module under the exact module-JavaScript
/// grammar consumers execute. Every successful native compile passes through
/// this gate; malformed output is never published as a successful module.
pub(super) fn validate_generated_client_module(
    code: &str,
    alloc: &Allocator,
) -> Result<(), ClientCompileError> {
    let parsed = oxc_parser::Parser::new(alloc, code, SourceType::mjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(ClientCompileError::GeneratedModuleInvalid {
            diagnostic_count: parsed.errors.len().max(usize::from(parsed.panicked)),
        });
    }
    Ok(())
}
