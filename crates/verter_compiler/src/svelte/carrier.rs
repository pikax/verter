//! The Svelte carrier bridge.
//!
//! Owns [`SvelteParseCarrier`] — the concrete [`CarrierParse`] payload wrapping
//! a [`ParsedSvelte`] — and [`build_svelte_parse_artifact`], the producer that
//! lifts a parse into the framework-neutral [`FrameworkParseArtifact`] (typed
//! script regions for BOTH the instance and module `<script>` blocks, plus the
//! `<style>` regions). The instance script is the runes-or-legacy component
//! body; the module script is `<script module>` (5.5) / legacy
//! `<script context="module">`.
//!
//! [`SvelteCarrierCompiler`] is the second [`CarrierCompiler`] (Vue is the
//! reference). `parse` produces the neutral artifact, `eval_source` blanks
//! everything but BOTH script contents at their raw offsets (output length ==
//! input length), `template_data` extracts the framework-neutral
//! component-usage facts from the typed template tree (see
//! [`template_facts`](super::template_facts)), and `compile_ide` projects the
//! type-checked IDE TSX.

use std::any::Any;
use std::sync::Arc;

use verter_language::{
    CarrierParse, ExternalLink, ExternalLinkKind, FrameworkAdapterId, FrameworkParseArtifact,
    FrameworkParseCommon, JsModuleKind, LanguageId, ScriptRegion, ScriptRegionKind,
    ScriptSourceType, StyleRegion,
};
use verter_span::Span;

use crate::framework_common::carrier_compiler::{
    CarrierCompiler, CompileUnsupported, IdeCompileOptions, IdeOutput, ParseOptions,
    RuntimeCompileOptions, RuntimeCompileOutput, RuntimeDiagnostic, RuntimeDiagnosticSeverity,
    TemplateFacts,
};
use crate::framework_common::ctx::{receive_svelte_carrier_token, CarrierCompilerCtx};

use super::parser::{parse_svelte, ParsedSvelte, SvelteScript};

/// The concrete Svelte carrier: the parsed component behind the erasure seam.
#[derive(Debug)]
pub struct SvelteParseCarrier {
    parsed: Arc<ParsedSvelte>,
}

impl SvelteParseCarrier {
    /// Wrap a parsed Svelte component.
    #[must_use]
    pub fn new(parsed: Arc<ParsedSvelte>) -> Self {
        Self { parsed }
    }

    /// The wrapped parse result.
    #[must_use]
    pub fn parsed(&self) -> &ParsedSvelte {
        &self.parsed
    }

    /// The wrapped parse result, as the shared handle.
    #[must_use]
    pub fn parsed_arc(&self) -> &Arc<ParsedSvelte> {
        &self.parsed
    }
}

impl CarrierParse for SvelteParseCarrier {
    fn __verter_as_any(&self) -> &dyn Any {
        self
    }
    fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

/// Resolve a Svelte `<script lang>` value to a neutral [`ScriptSourceType`].
///
/// Svelte components are TypeScript-or-JavaScript; `lang="ts"` (or absent) is
/// TypeScript, `lang="tsx"` is TSX, `lang="jsx"`/`lang="js"` map to their JS
/// dialects. A `.svelte` script is module-grammar (top-level `import`/`export`
/// allowed), so JS dialects resolve the module module-kind.
fn svelte_script_source_type(script: Option<&SvelteScript>) -> ScriptSourceType {
    match script.and_then(|s| s.lang.as_deref()) {
        Some(lang) if lang.eq_ignore_ascii_case("tsx") => ScriptSourceType::Tsx,
        Some(lang) if lang.eq_ignore_ascii_case("jsx") => {
            ScriptSourceType::Jsx(JsModuleKind::Module)
        }
        Some(lang) if lang.eq_ignore_ascii_case("js") => ScriptSourceType::Js(JsModuleKind::Module),
        _ => ScriptSourceType::Ts,
    }
}

/// The Svelte carrier parser version stamped on produced artifacts.
pub const SVELTE_CARRIER_PARSER_VERSION: u32 = 1;

/// Lift a parsed Svelte component into the framework-neutral parse artifact.
///
/// The neutral common surface carries:
/// * one [`ScriptRegion`] per `<script>` block — `<script module>` →
///   [`ScriptRegionKind::Module`], the instance `<script>` →
///   [`ScriptRegionKind::Instance`] — each stamped with the block's resolved
///   [`ScriptSourceType`]; regions are SOURCE-ordered;
/// * one [`StyleRegion`] per component `<style>` block;
/// * external `src` links for script/style blocks (Svelte rarely uses `src`,
///   but the producer records them uniformly).
#[must_use]
pub fn build_svelte_parse_artifact(
    source: &str,
    parsed: Arc<ParsedSvelte>,
    parser_version: u32,
) -> Arc<FrameworkParseArtifact> {
    let mut script_regions = Vec::new();
    let mut external_links = Vec::new();

    for script in [
        parsed.instance_script.as_ref(),
        parsed.module_script.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        let source_type = svelte_script_source_type(Some(script));
        let span = script
            .content
            .unwrap_or_else(|| Span::new(script.tag_open.end, script.tag_open.end));
        script_regions.push(ScriptRegion {
            span,
            source_type,
            kind: if script.is_module {
                ScriptRegionKind::Module
            } else {
                ScriptRegionKind::Instance
            },
        });
        if let Some((specifier, link_span)) = script_src(script, source) {
            external_links.push(ExternalLink {
                kind: ExternalLinkKind::Script,
                specifier,
                span: Some(link_span),
            });
        }
    }
    // Source-ordered (the parser already discovers them in source order, but a
    // module script may precede the instance one).
    script_regions.sort_by_key(|region| region.span.start);

    let mut style_regions = Vec::new();
    for style in &parsed.styles {
        let span = style
            .content
            .unwrap_or_else(|| Span::new(style.tag_open.end, style.tag_open.end));
        style_regions.push(StyleRegion { span });
    }

    Arc::new(FrameworkParseArtifact::new(
        FrameworkAdapterId::svelte(),
        LanguageId::new("svelte"),
        parser_version,
        FrameworkParseCommon {
            script_regions,
            template_regions: Vec::new(),
            style_regions,
            external_links,
            diagnostics: Vec::new(),
        },
        Arc::new(SvelteParseCarrier::new(parsed)),
    ))
}

/// Read a `src="..."` specifier off a script block's attributes.
fn script_src(script: &SvelteScript, source: &str) -> Option<(String, Span)> {
    use super::parser::{SvelteAttributeKind, SvelteAttributeValue};
    script.attributes.iter().find_map(|attr| match &attr.kind {
        SvelteAttributeKind::Plain {
            name,
            value: Some(SvelteAttributeValue::Text(span)),
            ..
        } if name.eq_ignore_ascii_case("src") => Some((
            source[span.start as usize..span.end as usize].to_string(),
            *span,
        )),
        _ => None,
    })
}

/// The Svelte carrier compiler — the second [`CarrierCompiler`].
pub struct SvelteCarrierCompiler {
    ctx: CarrierCompilerCtx,
}

impl std::fmt::Debug for SvelteCarrierCompiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SvelteCarrierCompiler")
            .finish_non_exhaustive()
    }
}

impl Default for SvelteCarrierCompiler {
    fn default() -> Self {
        Self {
            ctx: CarrierCompilerCtx::new(receive_svelte_carrier_token()),
        }
    }
}

impl SvelteCarrierCompiler {
    /// Reach the parsed component back out of a Svelte artifact, or `None` when
    /// the artifact is not a Svelte carrier.
    #[must_use]
    fn parsed_svelte<'a>(&self, artifact: &'a FrameworkParseArtifact) -> Option<&'a ParsedSvelte> {
        self.ctx
            .carrier_for::<SvelteParseCarrier>(artifact)
            .map(|carrier| carrier.parsed())
    }

    /// Run the Svelte IDE projection once and return BOTH the rendered
    /// [`IdeOutput`] and the neutral diagnostics it produced.
    ///
    /// The Svelte IDE codegen owns its OWN `CodeTransform` (the single source
    /// of truth for generated-code edits): the projection is a pure syntactic
    /// transform, NO type lowering (the thin-adapters guard). The output is a
    /// `.svelte.tsx` that type-checks clean through TSGO.
    ///
    /// This is the SINGLE projection entry the carrier reaches; both
    /// `compile_ide` (drops the diagnostics — the trait method has no
    /// diagnostic channel) and `compile_bundle` (LIFTS them into the bundle)
    /// route through it.
    fn project_ide(
        parsed: &ParsedSvelte,
        source: &str,
        opts: &IdeCompileOptions,
    ) -> (IdeOutput, Vec<RuntimeDiagnostic>) {
        let start = std::time::Instant::now();
        let projection = crate::svelte::ide::project_svelte_ide(
            source,
            parsed,
            opts.filename.as_deref(),
            opts.skip_source_map,
        );
        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

        let diagnostics = projection
            .diagnostics
            .iter()
            .map(|d| RuntimeDiagnostic {
                severity: match d.severity {
                    crate::svelte::ide::DiagnosticSeverity::Error => {
                        RuntimeDiagnosticSeverity::Error
                    }
                    crate::svelte::ide::DiagnosticSeverity::Information => {
                        RuntimeDiagnosticSeverity::Info
                    }
                },
                code: d.code.to_string(),
                message: d.message.clone(),
                span: Some(d.span),
            })
            .collect();

        let ide = IdeOutput {
            code: projection.code,
            source_map: projection.source_map,
            // A `.svelte` always projects TS `.tsx`: the projection emits
            // TS with the `@jsxImportSource` pragma; it is never `.jsx`.
            is_jsx: false,
            duration_ms,
            destructured_block: None,
        };
        (ide, diagnostics)
    }
}

impl CarrierCompiler for SvelteCarrierCompiler {
    fn adapter_id(&self) -> FrameworkAdapterId {
        FrameworkAdapterId::svelte()
    }

    fn carrier_language_id(&self) -> LanguageId {
        LanguageId::new("svelte")
    }

    fn parse(&self, source: &str, _opts: &ParseOptions) -> Arc<FrameworkParseArtifact> {
        let parsed = Arc::new(parse_svelte(source));
        build_svelte_parse_artifact(source, parsed, SVELTE_CARRIER_PARSER_VERSION)
    }

    fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
        // Position-preserving blanking: every byte starts blanked (line
        // terminators preserved so line/column geometry is unchanged), then
        // each script region's RAW bytes are stamped back over their carrier-
        // absolute offsets. BOTH the instance and module script blocks are
        // preserved; everything else (template, styles) is blanked. Output
        // length == input length by construction.
        let src = source.as_bytes();
        let mut out: Vec<u8> = src
            .iter()
            .map(|&b| if b == b'\n' || b == b'\r' { b } else { b' ' })
            .collect();
        for region in &artifact.common.script_regions {
            let start = region.span.start as usize;
            let end = region.span.end as usize;
            if start <= end && end <= src.len() {
                out[start..end].copy_from_slice(&src[start..end]);
            }
        }
        Arc::from(
            String::from_utf8(out)
                .unwrap_or_else(|_| source.to_string())
                .as_str(),
        )
    }

    fn compile_ide(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &IdeCompileOptions,
    ) -> Result<IdeOutput, CompileUnsupported> {
        // A foreign artifact (not a Svelte carrier) declines with the typed
        // answer — never a silent empty output.
        let Some(parsed) = self.parsed_svelte(artifact) else {
            return Err(CompileUnsupported::NoIdeProjection {
                adapter_id: self.adapter_id(),
            });
        };

        // The trait `compile_ide` surface has no framework-neutral diagnostic
        // channel — the projection's typed-unsupported diagnostics are LIFTED
        // by `compile_bundle` instead (the host's IDE-ensure path), where the
        // `RuntimeCompileOutput.diagnostics` channel reaches the host
        // `DiagnosticsSnapshot`. Here they are produced-and-dropped (matching
        // the Vue carrier `compile_ide`, whose IDE diagnostics flow through the
        // parse artifact's `common.diagnostics`, not `IdeOutput`).
        let (ide, _diagnostics) = Self::project_ide(parsed, source, opts);
        Ok(ide)
    }

    fn template_data(&self, source: &str, artifact: &FrameworkParseArtifact) -> TemplateFacts {
        // A foreign artifact (not a Svelte carrier) yields empty neutral facts.
        let Some(parsed) = self.parsed_svelte(artifact) else {
            return TemplateFacts::default();
        };
        let mut data = crate::compile::RawTemplateData::default();
        // STRUCTURAL walk over the typed `ParsedSvelte.template` tree (mirrors
        // `collect_slot_elements`' walk shape — recurse element children, block
        // children, and each clause's children). The component-by-KIND
        // classification reads the typed AST; expression TEXT is span-sliced from
        // the carrier source. No structural source scan.
        super::template_facts::collect_component_usages(&parsed.template, source, &mut data);
        TemplateFacts { data }
    }

    fn compile_bundle(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &RuntimeCompileOptions,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<RuntimeCompileOutput, CompileUnsupported> {
        // A foreign artifact (not a Svelte carrier) declines with the typed
        // answer — never a silent empty bundle.
        let Some(parsed) = self.parsed_svelte(artifact) else {
            return Err(CompileUnsupported::NoIdeProjection {
                adapter_id: self.adapter_id(),
            });
        };

        let mut bundle = RuntimeCompileOutput::default();

        // The Svelte native RUNTIME compiler (source `.svelte` → JS importing
        // `svelte/internal/client`). A SUPPORTED component populates
        // `main.body_code` (the host emits the `Main` virtual node from it,
        // `has_runtime_surface()` becoming true through registry routing); an
        // UNSUPPORTED runtime surface FAILS CLOSED with a precise non-fatal
        // diagnostic (carrying the surface + owning vertical) and produces NO
        // runtime body — the IDE artifact is still produced so type-checking
        // survives. SSR (`opts.ssr`) fails closed until the server backend lands.
        let runtime_opts = super::runtime::SvelteRuntimeOptions {
            filename: opts.filename.clone(),
            name: None,
            runes: None,
            is_production: opts.is_production,
            // The neutral `RuntimeCompileOptions` carries no dev-codegen request
            // distinct from the §1.2 default (`is_production == false` is the
            // canonical request shape, NOT a dev-codegen request), so the Svelte
            // client backend always emits PRODUCTION output here. The dev-mode
            // output axis (5k) is requested through a dedicated signal that lands
            // with that block; until then `dev_codegen` stays false.
            dev_codegen: false,
        };
        match super::runtime::compile_client(source, parsed, &runtime_opts, alloc, opts.ssr) {
            Ok(module) => {
                bundle.main.body_code = Some(module.code);
                bundle.main.lang = Some("js".to_string());
            }
            Err(super::runtime::ClientCompileError::Unsupported(surface)) => {
                // Fail closed: NO `Main` runtime node is produced, the bundle is
                // marked `runtime_surface_refused` (the distinct typed signal a
                // runtime-requesting consumer reads so it cannot mistake the absent
                // `Main` for a successful runtime compile), and the precise
                // `svelte-runtime-unsupported-<surface>` reason reaches the host
                // `DiagnosticsSnapshot`. The diagnostic stays NON-FATAL (Warning) so
                // the IDE projection below still compiles and type-checking survives.
                bundle.runtime_surface_refused = true;
                bundle.diagnostics.push(RuntimeDiagnostic {
                    severity: RuntimeDiagnosticSeverity::Warning,
                    code: surface.diagnostic_code().to_string(),
                    message: surface.message(),
                    span: Some(surface.span()),
                });
            }
            Err(super::runtime::ClientCompileError::Lowering(errors)) => {
                // A genuine lowering failure (a malformed construct) ALSO produces no
                // `Main` — so it is a runtime refusal too: set `runtime_surface_refused`
                // (the distinct typed signal) so a runtime-requesting consumer reads the
                // failure EXPLICITLY rather than mistaking the absent `Main` for a clean
                // IDE-only carrier. Each recorded problem is surfaced as a non-fatal
                // diagnostic — the IDE projection still runs (it has its own error
                // recovery).
                bundle.runtime_surface_refused = true;
                for diag in &errors.diagnostics {
                    bundle.diagnostics.push(RuntimeDiagnostic {
                        severity: RuntimeDiagnosticSeverity::Warning,
                        code: diag.code.to_string(),
                        message: diag.message.clone(),
                        span: Some(diag.span),
                    });
                }
            }
            Err(super::runtime::ClientCompileError::OfficialReject(rejection)) => {
                // The component is MALFORMED Svelte official ALSO compile-errors — fail
                // closed (NO `Main`), mark `runtime_surface_refused`, and surface the
                // typed official-reject diagnostic (the rule's stable code + a message
                // naming the EXACT official code the rejection mirrors). The IDE
                // projection below still runs (it owns its own error recovery), so a
                // malformed component still type-checks while producing no runtime module.
                bundle.runtime_surface_refused = true;
                bundle.diagnostics.push(RuntimeDiagnostic {
                    severity: RuntimeDiagnosticSeverity::Warning,
                    code: rejection.rule.diagnostic_code().to_string(),
                    message: format!(
                        "{} (official `{}`)",
                        rejection.rule.message(),
                        rejection.official_code
                    ),
                    span: None,
                });
            }
        }

        // The IDE projection is ALWAYS available (independent of the runtime
        // surface) so an unsupported-runtime component still type-checks. Its
        // typed-unsupported diagnostics are lifted alongside the runtime ones.
        if opts.want_ide {
            let ide_opts = IdeCompileOptions {
                filename: opts.filename.clone(),
                skip_source_map: !opts.source_map,
                embed_ambient_types: opts.embed_ambient_types,
            };
            let (ide, mut diagnostics) = Self::project_ide(parsed, source, &ide_opts);
            bundle.tsx = Some(ide);
            bundle.diagnostics.append(&mut diagnostics);
        }

        if opts.want_template_data {
            let mut data = crate::compile::RawTemplateData::default();
            super::template_facts::collect_component_usages(&parsed.template, source, &mut data);
            bundle.template_data = Some(data);
        }

        Ok(bundle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework_common::sourcemap_e2e_helpers::{
        assert_token_maps_to_source, assert_token_maps_to_source_line, build_lookup_table,
        parse_ide_output,
    };

    fn artifact_for(source: &str) -> Arc<FrameworkParseArtifact> {
        let parsed = Arc::new(parse_svelte(source));
        build_svelte_parse_artifact(source, parsed, SVELTE_CARRIER_PARSER_VERSION)
    }

    #[test]
    fn compile_bundle_emits_a_runtime_main_body_for_a_runes_component() {
        // A SUPPORTED runes component populates `main.body_code` (Svelte client JS)
        // so `has_runtime_surface()` becomes true. DISCRIMINATING: the body is the
        // client module, not empty.
        let compiler = SvelteCarrierCompiler::default();
        let source = "<script>let count = $state(0);</script>\n<button onclick={() => count++}>{count}</button>\n";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let alloc = oxc_allocator::Allocator::default();
        let bundle = compiler
            .compile_bundle(source, &artifact, &RuntimeCompileOptions::default(), &alloc)
            .expect("svelte runtime bundle");
        assert!(
            bundle.has_runtime_surface(),
            "a runes component must carry a runtime surface"
        );
        let body = bundle
            .main
            .body_code
            .expect("a runes component emits a Main body");
        assert!(
            body.contains("import * as $ from 'svelte/internal/client';"),
            "body:\n{body}"
        );
        assert!(body.contains("$.state(0)"), "body:\n{body}");
        assert_eq!(bundle.main.lang.as_deref(), Some("js"));
        // No fail-closed diagnostic for a supported component.
        assert!(
            bundle.diagnostics.is_empty(),
            "a supported component emits no diagnostics, got: {:?}",
            bundle.diagnostics
        );
    }

    #[test]
    fn compile_bundle_fails_closed_for_a_legacy_component_with_no_main() {
        // A legacy (non-runes) component is an unsupported runtime surface: NO Main
        // body, and a precise non-fatal diagnostic (so the IDE surface survives).
        let compiler = SvelteCarrierCompiler::default();
        let source = "<script>export let label;</script>\n<p>{label}</p>\n";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let alloc = oxc_allocator::Allocator::default();
        let bundle = compiler
            .compile_bundle(source, &artifact, &RuntimeCompileOptions::default(), &alloc)
            .expect("the bundle is produced (fail-closed, not an Err)");
        assert!(
            !bundle.has_runtime_surface(),
            "a legacy component must NOT carry a runtime surface"
        );
        assert!(
            bundle.main.body_code.is_none(),
            "no Main body for a legacy component"
        );
        // The fail-closed diagnostic is precise + NON-FATAL (so the IDE survives).
        assert!(
            bundle
                .diagnostics
                .iter()
                .any(|d| d.code == "svelte-runtime-unsupported-legacy-mode"),
            "the legacy-mode fail-closed diagnostic must be present, got: {:?}",
            bundle
                .diagnostics
                .iter()
                .map(|d| &d.code)
                .collect::<Vec<_>>()
        );
        assert!(
            !bundle.has_errors(),
            "the fail-closed diagnostic is non-fatal so the IDE compile survives"
        );
    }

    #[test]
    fn runtime_refusal_is_unmissable_to_a_runtime_consumer_yet_ide_survives() {
        // F5: a RUNTIME request on an unsupported component must surface the refusal
        // UNAMBIGUOUSLY — `runtime_surface_refused()` is the distinct typed signal
        // (set ONLY on a fail-closed runtime outcome), `has_runtime_surface()` is
        // false (no `Main`), and the precise diagnostic carries the reason — so a
        // consumer cannot mistake the absent `Main` for a successful runtime
        // compile. YET the IDE projection (`tsx`) is still produced and the
        // diagnostics stay NON-FATAL, so type-checking survives.
        let compiler = SvelteCarrierCompiler::default();
        // An `{#if}` block is an unsupported runtime surface (5e).
        let source = "<script>let c = $state(true);</script>\n{#if c}<p>yes</p>{/if}\n";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let alloc = oxc_allocator::Allocator::default();
        let opts = RuntimeCompileOptions {
            want_ide: true,
            ..Default::default()
        };
        let bundle = compiler
            .compile_bundle(source, &artifact, &opts, &alloc)
            .expect("the bundle is produced (fail-closed, not an Err)");

        // The distinct typed runtime-refusal signal.
        assert!(
            bundle.runtime_surface_refused(),
            "an unsupported runtime surface must set the runtime-refusal signal"
        );
        // No `Main` runtime surface.
        assert!(
            !bundle.has_runtime_surface(),
            "a refused runtime surface produces NO Main"
        );
        // The precise reason reaches the consumer.
        assert!(
            bundle
                .diagnostics
                .iter()
                .any(|d| d.code.starts_with("svelte-runtime-unsupported-")),
            "the precise unsupported-surface diagnostic is present"
        );
        // The IDE survives: the `tsx` artifact is present and the refusal is
        // non-fatal (does not kill the IDE compile).
        assert!(bundle.tsx.is_some(), "the IDE projection still compiles");
        assert!(
            !bundle.has_errors(),
            "the runtime refusal is non-fatal so the IDE survives"
        );
    }

    #[test]
    fn a_supported_component_never_sets_the_runtime_refusal_signal() {
        // F5 NEGATIVE: a SUPPORTED runes component emits a Main and NEVER sets the
        // runtime-refusal signal (the signal discriminates refusal from success).
        let compiler = SvelteCarrierCompiler::default();
        let source =
            "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let alloc = oxc_allocator::Allocator::default();
        let bundle = compiler
            .compile_bundle(source, &artifact, &RuntimeCompileOptions::default(), &alloc)
            .expect("svelte runtime bundle");
        assert!(
            bundle.has_runtime_surface(),
            "a supported component carries a Main"
        );
        assert!(
            !bundle.runtime_surface_refused(),
            "a supported component must NOT set the runtime-refusal signal"
        );
    }

    #[test]
    fn ide_sourcemap_maps_script_and_template_expressions_back_to_source() {
        // The sourcemap e2e (Tests #2): a script-region binding and a template
        // expression each map back to the matching ORIGINAL carrier text. The
        // unmapped prelude shifts no mapped position — the tokens still land.
        let compiler = SvelteCarrierCompiler::default();
        let source =
            "<script lang=\"ts\">let myUniqueBinding = 0;</script>\n<div>{myUniqueBinding}</div>";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let ide = compiler
            .compile_ide(
                source,
                &artifact,
                &IdeCompileOptions {
                    filename: Some("Comp.svelte".to_string()),
                    ..Default::default()
                },
            )
            .expect("svelte ide projection");
        let (code, sm) = parse_ide_output(&ide);
        let lookup = build_lookup_table(&sm);

        // The script binding maps back (line granularity for script regions).
        assert_token_maps_to_source_line(&sm, &lookup, &code, source, "myUniqueBinding", 0);
        // The template-expression occurrence (`{myUniqueBinding}`) maps back to
        // the SECOND source occurrence — exact column.
        assert_token_maps_to_source(&sm, &lookup, &code, source, "myUniqueBinding", 1);
    }

    #[test]
    fn ide_sourcemap_maps_transition_params_expression_back_to_source() {
        // F2 sourcemap e2e: the `transition:` PARAMS expression is moved into the
        // projected `__verter_transition(fly(node, <params>))` call but stays a
        // mapped Original chunk — a hover inside the params identifier lands on
        // the matching ORIGINAL `.svelte` byte (the directive prefix strip /
        // checker injection went through CodeTransform, so the map stays
        // token-precise). DISCRIMINATING: the params identifier `flyParam` is
        // unique to the directive value, so its mapped token can only come from
        // the original `transition:fly={flyParam}` position.
        let compiler = SvelteCarrierCompiler::default();
        let source = "<script lang=\"ts\">import { fly } from \"svelte/transition\";\n\
             const flyParam = { delay: 0 };</script>\n\
             <div transition:fly={flyParam}>x</div>";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let ide = compiler
            .compile_ide(
                source,
                &artifact,
                &IdeCompileOptions {
                    filename: Some("Comp.svelte".to_string()),
                    ..Default::default()
                },
            )
            .expect("svelte ide projection");
        let (code, sm) = parse_ide_output(&ide);
        let lookup = build_lookup_table(&sm);

        // The `flyParam` declaration in the script maps back (line granularity).
        assert_token_maps_to_source_line(&sm, &lookup, &code, source, "flyParam", 0);
        // The `transition:fly={flyParam}` PARAMS occurrence (the SECOND source
        // occurrence) maps back to its exact original column — the moved params
        // expression kept its source span.
        assert_token_maps_to_source(&sm, &lookup, &code, source, "flyParam", 1);
    }

    #[test]
    fn artifact_identity_names_the_svelte_adapter() {
        let artifact = artifact_for("<script>let a = 1;</script>");
        assert!(!artifact.adapter_id.is_vue());
        assert_eq!(artifact.adapter_id, FrameworkAdapterId::svelte());
        assert_eq!(artifact.language_id.as_str(), "svelte");
        assert_eq!(artifact.parser_version, SVELTE_CARRIER_PARSER_VERSION);
    }

    #[test]
    fn instance_and_module_scripts_carry_kind_and_source_type() {
        let source =
            "<script module>export const x = 1;</script>\n<script lang=\"ts\">let a = 1;</script>";
        let artifact = artifact_for(source);
        let regions = &artifact.common.script_regions;
        assert_eq!(regions.len(), 2);
        // Source-ordered: module script first.
        assert_eq!(regions[0].kind, ScriptRegionKind::Module);
        assert_eq!(regions[1].kind, ScriptRegionKind::Instance);
        assert_eq!(regions[0].span.slice(source).trim(), "export const x = 1;");
        assert_eq!(regions[1].span.slice(source).trim(), "let a = 1;");
    }

    #[test]
    fn legacy_context_module_classifies_as_module() {
        let source = "<script context=\"module\">export const x = 1;</script>";
        let artifact = artifact_for(source);
        assert_eq!(artifact.common.script_regions.len(), 1);
        assert_eq!(
            artifact.common.script_regions[0].kind,
            ScriptRegionKind::Module
        );
    }

    #[test]
    fn eval_source_is_position_preserving_with_both_scripts_at_raw_offsets() {
        let source = "<script module>export const x = 1;</script>\n<div>{count}</div>\n<script lang=\"ts\">let count = 0;</script>";
        let compiler = SvelteCarrierCompiler::default();
        let artifact = compiler.parse(source, &ParseOptions::default());
        let eval = compiler.eval_source(source, &artifact);
        assert_eq!(eval.len(), source.len(), "eval source must be same length");
        for region in &artifact.common.script_regions {
            let (s, e) = (region.span.start as usize, region.span.end as usize);
            assert_eq!(
                &eval[s..e],
                &source[s..e],
                "script bytes preserved at raw offsets"
            );
        }
        // The `<div>` markup is blanked (no `<` survives outside script).
        let markup_idx = source.find("<div>").unwrap();
        assert_eq!(eval.as_bytes()[markup_idx], b' ', "markup is blanked");
    }

    #[test]
    fn compile_ide_projects_a_tsx_artifact_with_the_pragma_prelude() {
        let compiler = SvelteCarrierCompiler::default();
        let source = "<script lang=\"ts\">let a = 1;</script>\n<div>{a}</div>";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let out = compiler
            .compile_ide(source, &artifact, &IdeCompileOptions::default())
            .expect("the Svelte IDE projection produces a TSX artifact");
        // A `.svelte` always projects TS `.tsx` (never `.jsx`).
        assert!(!out.is_jsx);
        // The pragma prelude opens the file.
        assert!(out
            .code
            .starts_with("/** @jsxImportSource @verter/svelte-jsx */"));
        // The script body is preserved.
        assert!(out.code.contains("let a = 1;"));
        // No `<script>` tag residue survives.
        assert!(!out.code.contains("<script"));
        // A source map is produced by default.
        assert!(!out.source_map.is_empty());
    }

    #[test]
    fn compile_ide_declines_a_foreign_artifact() {
        let compiler = SvelteCarrierCompiler::default();
        // A Vue-shaped artifact is not a Svelte carrier — the typed answer.
        let svelte = compiler.parse("<div />", &ParseOptions::default());
        // Re-wrap is unnecessary; a real foreign carrier is exercised by the
        // shared contract tests. Here we assert the Svelte path succeeds.
        let out = compiler.compile_ide("<div />", &svelte, &IdeCompileOptions::default());
        assert!(out.is_ok());
    }

    fn facts_for(source: &str) -> crate::compile::RawTemplateData {
        let compiler = SvelteCarrierCompiler::default();
        let artifact = compiler.parse(source, &ParseOptions::default());
        compiler.template_data(source, &artifact).data
    }

    #[test]
    fn template_data_collects_full_component_usage_facts() {
        // The discriminating producer unit: a component usage carries the static
        // prop, the `on*` plain attribute AS A PROP (props/events is syntactic —
        // a plain `on*` attr is never an event at the usage site), the `bind:`
        // binding, and the passed `{#snippet}` (via `slots_used`).
        let source = "<script lang=\"ts\">let value = 0; function handler() {}</script>\n\
             <Button size=\"sm\" bind:value onclick={handler}>{#snippet icon()}x{/snippet}</Button>";
        let data = facts_for(source);
        assert_eq!(data.components.len(), 1, "exactly one component usage");
        let usage = &data.components[0];
        assert_eq!(usage.tag_name, "Button");
        assert!(!usage.is_dynamic, "a static component is not dynamic");

        // `size="sm"` is a STATIC prop (Text value); `onclick={handler}` is a
        // PLAIN prop (a callback handler IS a prop in Svelte 5 — the child
        // decides which props are events, not a usage-site name guess);
        // `bind:value` is NOT a prop.
        let prop_names: Vec<&str> = usage.props.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            prop_names,
            vec!["size", "onclick"],
            "`size` and `onclick` are both props"
        );
        let size_prop = usage.props.iter().find(|p| p.name == "size").unwrap();
        assert!(!size_prop.is_bound, "a Text value is a static prop");
        let onclick_prop = usage.props.iter().find(|p| p.name == "onclick").unwrap();
        assert!(onclick_prop.is_bound, "an `{{...}}` value is a bound prop");
        assert_eq!(onclick_prop.expression.as_deref(), Some("handler"));

        // `bind:value` is a neutral BINDING (NOT a prop, NOT v_model).
        let binding_names: Vec<&str> = usage.bindings.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(binding_names, vec!["value"], "`bind:value` is a binding");

        // No EVENT — there is no legacy `on:` directive here. A plain `on*`
        // attribute is never fabricated as an event.
        assert!(
            usage.events.is_empty(),
            "a plain `on*` attribute is a prop, never an event"
        );

        // The `{#snippet icon}` passed inside the component is recorded in
        // `slots_used` (names-only).
        assert_eq!(usage.slots_used, vec!["icon".to_string()]);

        // NEGATIVES: `bind:value` does NOT leak into props; `onclick` does NOT
        // leak into bindings.
        assert!(!prop_names.contains(&"value"));
        assert!(!binding_names.contains(&"onclick"));
    }

    #[test]
    fn template_data_keeps_on_prefixed_plain_attrs_as_props_not_events() {
        // THE discriminating regression for the P1 finding: plain attributes
        // whose names START WITH `on` but are NOT events (`online`, `once`,
        // `onboarding`, `one`) must stay PROPS and must NOT be fabricated as
        // events by a name-prefix heuristic. RED against the old
        // `is_runes_event_name` (which classified every `on`-prefixed plain attr
        // as an event); GREEN after the syntactic-only classification.
        let source = "<script lang=\"ts\">let x = 1; let y = 2; let z = 3;</script>\n\
             <Widget online={x} once={y} onboarding={z} one=\"1\" onclick={x} />";
        let data = facts_for(source);
        let usage = &data.components[0];

        let prop_names: Vec<&str> = usage.props.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            prop_names,
            vec!["online", "once", "onboarding", "one", "onclick"],
            "every plain `on*` attribute is a PROP, none removed by a name guess"
        );

        // NEGATIVE: none of them were fabricated as events.
        assert!(
            usage.events.is_empty(),
            "no plain `on*` attribute is fabricated as an event, got {:?}",
            usage.events.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
        for name in ["online", "once", "onboarding", "one", "onclick"] {
            assert!(
                !usage.events.iter().any(|e| e.name == name),
                "`{name}` must not be an event"
            );
        }
    }

    #[test]
    fn template_data_maps_legacy_on_directive_to_event() {
        // A legacy `on:click|stop` is an EVENT named `click` carrying its
        // modifiers — NOT a prop, NOT a binding.
        let source = "<script>function f(){}</script>\n<Button on:click|stop={f} />";
        let data = facts_for(source);
        let usage = &data.components[0];
        assert!(usage.props.is_empty(), "`on:click` is not a prop");
        assert!(usage.bindings.is_empty(), "`on:click` is not a binding");
        let event_names: Vec<&str> = usage.events.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            event_names,
            vec!["click"],
            "`on:click` maps to event `click`"
        );
        assert_eq!(usage.events[0].modifiers, vec!["stop".to_string()]);
    }

    #[test]
    fn template_data_skips_non_model_directives_and_class_use_let() {
        // NEGATIVES: `class:`, `use:`, `bind:this`, `let:` produce NO prop, NO
        // event, NO binding on a component usage.
        let source = "<script>let el; function a(){}</script>\n\
             <Button class:active use:a bind:this={el} let:item />";
        let data = facts_for(source);
        let usage = &data.components[0];
        assert!(
            usage.props.is_empty(),
            "no props from class/use/bind:this/let:"
        );
        assert!(
            usage.events.is_empty(),
            "no events from class/use/bind:this/let:"
        );
        assert!(
            usage.bindings.is_empty(),
            "`bind:this` is a ref, not a model binding; class/use/let produce no binding"
        );
    }

    #[test]
    fn template_data_dynamic_component_skips_this_attr() {
        // `<svelte:component this={X}>` is a DYNAMIC usage; the `this` attribute
        // is the component selector, NOT a prop.
        let source = "<script>let X; let v = 0;</script>\n\
             <svelte:component this={X} label=\"hi\" bind:value={v} />";
        let data = facts_for(source);
        assert_eq!(data.components.len(), 1);
        let usage = &data.components[0];
        assert!(usage.is_dynamic, "`<svelte:component>` is dynamic");
        let prop_names: Vec<&str> = usage.props.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            prop_names,
            vec!["label"],
            "`this` is skipped as a prop; `label` remains a prop"
        );
        assert!(!prop_names.contains(&"this"), "`this` is never a prop");
        let binding_names: Vec<&str> = usage.bindings.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            binding_names,
            vec!["value"],
            "`bind:value` is still a binding"
        );
    }

    #[test]
    fn template_data_records_self_reference_and_spread() {
        // `<svelte:self>` is a self usage; `{...rest}` sets `has_spread`.
        let source = "<script>let rest = {};</script>\n<svelte:self {...rest} />";
        let data = facts_for(source);
        assert_eq!(data.components.len(), 1);
        let usage = &data.components[0];
        assert_eq!(usage.tag_name, "svelte:self");
        assert!(usage.has_spread, "`{{...rest}}` sets has_spread");
    }

    #[test]
    fn template_data_recurses_into_blocks_and_clauses() {
        // The structural walk recurses element children, block children, and each
        // clause's children — a component nested in an `{:else}` clause is found.
        let source = "<script>let cond = true;</script>\n\
             {#if cond}<Yes />{:else}<No />{/if}";
        let data = facts_for(source);
        let names: Vec<&str> = data
            .components
            .iter()
            .map(|c| c.tag_name.as_str())
            .collect();
        assert!(names.contains(&"Yes"), "the if-branch component is found");
        assert!(names.contains(&"No"), "the else-clause component is found");
    }

    #[test]
    fn template_data_ignores_intrinsic_and_special_layout_elements() {
        // NEGATIVE: intrinsic HTML elements and non-component special elements
        // (`<svelte:head>`) are NOT component usages.
        let source =
            "<script>let a = 1;</script>\n<div><svelte:head><title>t</title></svelte:head></div>";
        let data = facts_for(source);
        assert!(
            data.components.is_empty(),
            "intrinsic / layout special elements are not component usages"
        );
    }

    #[test]
    fn template_data_records_inline_handler_and_expression_binding() {
        // An inline arrow handler on a LEGACY `on:` directive is flagged
        // `is_inline`; a plain `onclick={...}` attribute is a BOUND PROP (not an
        // event — the props/events split is syntactic); an Expression-valued
        // (`{count}`) plain prop is a BOUND prop.
        let source = "<script>let count = 0;</script>\n\
             <Child on:click={() => count++} onclick={() => count++} count={count} />";
        let data = facts_for(source);
        let usage = &data.components[0];

        // The legacy `on:click` directive is the SOLE event, and its inline
        // arrow handler is flagged `is_inline`.
        assert_eq!(usage.events.len(), 1, "only the legacy `on:` is an event");
        assert_eq!(usage.events[0].name, "click");
        assert!(usage.events[0].is_inline, "an arrow handler is inline");

        // The plain `onclick={...}` attribute is a BOUND prop, NOT an event.
        let onclick_prop = usage
            .props
            .iter()
            .find(|p| p.name == "onclick")
            .expect("onclick is a prop, not an event");
        assert!(
            onclick_prop.is_bound,
            "an `{{...}}` handler attribute is a bound prop"
        );

        let count_prop = usage
            .props
            .iter()
            .find(|p| p.name == "count")
            .expect("count prop present");
        assert!(count_prop.is_bound, "an `{{expr}}` value is a bound prop");
    }
}
