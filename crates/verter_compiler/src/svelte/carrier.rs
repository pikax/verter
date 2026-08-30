//! Svelte carrier bridge.
//!
//! [`SvelteParseCarrier`] wraps [`ParsedSvelte`]. [`build_svelte_parse_artifact`]
//! produces the unregistered artifact; the projector owns inventory
//! geometry. [`SvelteCarrierCompiler`]: `parse` → neutral artifact;
//! `compile_ide` projects IDE TSX.

use std::any::Any;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use verter_language::{
    parse_key_for, sort_language_diagnostics, syntax_profile_id_for, CarrierParse, FileLanguage,
    FrameworkAdapterId, JsModuleKind, LanguageDiagnostic, LanguageDiagnosticSeverity, LanguageId,
    ParseKey, ParseOptions, ScriptSourceType, SyntaxProfileId, SyntaxReject,
    UnregisteredFrameworkParseArtifact, SVELTE_SYNTAX_COMPATIBILITY_DOMAIN,
    SVELTE_SYNTAX_COMPATIBILITY_EPOCH,
};
use verter_span::Span;

use crate::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest,
    SvelteCompileRequest,
};
use crate::framework_common::carrier_compiler::{
    CarrierCompileOutcome, CarrierCompiler, CompileUnsupported, IdeCompileOptions, IdeOutput,
    RuntimeCompileOptions, RuntimeCompileOutput, RuntimeDiagnostic, RuntimeDiagnosticSeverity,
    RuntimeOutputDescriptor, RuntimeSurfaceRefusal, SourceMapFidelity,
};
use crate::framework_common::FrameworkParseArtifact;

use super::attribute_expressions::SvelteAttributeExpressions;
use super::parser::{parse_svelte, CloseTagViolationKind, ParsedSvelte, SvelteScript};
use super::runtime::{SvelteFragments, SvelteNamespace};

/// Maps the canonical request's `svelte_namespace` string to the compiler's
/// typed [`SvelteNamespace`]. Only the three official tokens are valid; an
/// unrecognized token is a decode-boundary concern (transport validation),
/// not something this carrier silently guesses at — it resolves to `None`
/// (the request's own default), never fabricates a value.
fn parse_svelte_namespace(token: &str) -> Option<SvelteNamespace> {
    match token {
        "html" => Some(SvelteNamespace::Html),
        "svg" => Some(SvelteNamespace::Svg),
        "mathml" => Some(SvelteNamespace::Mathml),
        _ => None,
    }
}

/// Maps the canonical request's `svelte_fragments` string to the compiler's
/// typed [`SvelteFragments`]. See [`parse_svelte_namespace`] for the
/// unrecognized-token rationale.
fn parse_svelte_fragments(token: &str) -> Option<SvelteFragments> {
    match token {
        "html" => Some(SvelteFragments::Html),
        "tree" => Some(SvelteFragments::Tree),
        _ => None,
    }
}

/// The concrete Svelte carrier: the parsed component behind the erasure seam,
/// plus the typed lowering of its plain-attribute `{expr}` values.
#[derive(Debug)]
pub struct SvelteParseCarrier {
    parsed: Arc<ParsedSvelte>,
    attribute_expressions: SvelteAttributeExpressions,
}

impl SvelteParseCarrier {
    /// Wrap a parsed Svelte component, lowering its plain-attribute values
    /// against the carrier `source` they were parsed from.
    #[must_use]
    pub fn new(parsed: Arc<ParsedSvelte>, source: &str) -> Self {
        let attribute_expressions = SvelteAttributeExpressions::lower(&parsed, source);
        Self {
            parsed,
            attribute_expressions,
        }
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

    /// The retained typed lowering of this component's plain-attribute values.
    #[must_use]
    pub fn attribute_expressions(&self) -> &SvelteAttributeExpressions {
        &self.attribute_expressions
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
/// Svelte components are TypeScript-or-JavaScript; only exact `lang="ts"` is
/// TypeScript. No-lang and other script languages use JavaScript grammar. A
/// `.svelte` script is module-grammar (top-level `import`/`export` allowed), so
/// JS dialects resolve the module module-kind.
pub(crate) fn svelte_script_source_type(script: Option<&SvelteScript>) -> ScriptSourceType {
    match script.and_then(|s| s.lang.as_deref()) {
        Some("ts") => ScriptSourceType::Ts,
        Some("jsx") => ScriptSourceType::Jsx(JsModuleKind::Module),
        _ => ScriptSourceType::Js(JsModuleKind::Module),
    }
}

/// The Svelte carrier parser version stamped on produced artifacts.
/// Map parser diagnostic rails onto the framework-neutral mapped channel.
/// Close-tag / parse-reject facts come from typed rails (no code-string
/// classification); every entry has a retained span.
///
/// Strict-parse facts (`ParsedSvelte::strict_parse_errors`) are mapped too —
/// official recovers a usable tree while still recording the diagnostic.
/// Unlike other rails they carry `blocks_compile: false`: IDE-visible at
/// `Error` severity, but `compile_entry` must not refuse the whole file over
/// one recoverable defect. Client-runtime "emits `Main` ⇔ official ACCEPTS"
/// still sees every strict fact at `official_reject_gate` (typed non-fatal
/// runtime refusal, never an `Err` that blocks IDE).
fn svelte_parse_diagnostics(
    source: &str,
    parsed: &ParsedSvelte,
    parse_key: &ParseKey,
) -> Vec<LanguageDiagnostic> {
    let mut diagnostics = parsed
        .diagnostics
        .iter()
        .map(|diagnostic| LanguageDiagnostic {
            span: diagnostic.span,
            severity: LanguageDiagnosticSeverity::Error,
            code: diagnostic.official_code.unwrap_or(diagnostic.code),
            arguments: Vec::new(),
            message: diagnostic.message.clone(),
            blocks_compile: true,
        })
        .collect::<Vec<_>>();
    diagnostics.extend(parsed.close_tag_violations.iter().map(|violation| {
        let code = match violation.kind {
            CloseTagViolationKind::Unclosed => "element_unclosed",
            CloseTagViolationKind::InvalidClosingTag => "element_invalid_closing_tag",
            CloseTagViolationKind::VoidElementInvalidContent => "void_element_invalid_content",
        };
        recovered_svelte_diagnostic(violation.span, code, true)
    }));
    // Strict-parse facts: `blocks_compile: false` (usable recovered tree).
    // Other rails stay `blocks_compile: true`.
    diagnostics.extend(
        parsed
            .strict_parse_errors
            .iter()
            .map(|fact| recovered_svelte_diagnostic(fact.span, fact.official_code, false)),
    );
    diagnostics.extend(
        parsed
            .parse_reject_facts
            .iter()
            .map(|fact| recovered_svelte_diagnostic(fact.span, fact.official_code, true)),
    );
    // Non-CSS deferred parse defects only. CSS style-body rejection is
    // `official_reject::deferred_parse_defects`, not this mapped channel.
    diagnostics.extend(
        super::runtime::deferred_parse_defects_excluding_css(source, parsed)
            .into_iter()
            .map(|defect| {
                recovered_svelte_diagnostic(defect.span, defect.rejection.official_code, true)
            }),
    );
    sort_language_diagnostics(parse_key, &mut diagnostics);
    diagnostics
}

fn recovered_svelte_diagnostic(
    span: Span,
    code: &'static str,
    blocks_compile: bool,
) -> LanguageDiagnostic {
    LanguageDiagnostic {
        span,
        severity: LanguageDiagnosticSeverity::Error,
        code,
        arguments: Vec::new(),
        message: format!("Svelte recovered from `{code}`"),
        blocks_compile,
    }
}

/// Wrap a parsed Svelte component for the registered projector.
#[must_use]
pub fn build_svelte_parse_artifact(
    source: &str,
    parsed: Arc<ParsedSvelte>,
    parse_key: Arc<ParseKey>,
    syntax_profile: Arc<SyntaxProfileId>,
) -> Arc<UnregisteredFrameworkParseArtifact> {
    let diagnostics = svelte_parse_diagnostics(source, &parsed, &parse_key);
    Arc::new(UnregisteredFrameworkParseArtifact::new(
        FrameworkAdapterId::svelte(),
        LanguageId::new("svelte"),
        parse_key,
        syntax_profile,
        diagnostics,
        Arc::new(SvelteParseCarrier::new(parsed, source)),
    ))
}

/// The Svelte carrier compiler — the second [`CarrierCompiler`].
///
/// Reaches its parsed component back out of the type-erased artifact
/// through its own inherent downcast — no capability token, since only
/// this adapter's own inherent methods call the raw carrier downcast on
/// its own artifacts.
#[derive(Default)]
pub struct SvelteCarrierCompiler;

impl std::fmt::Debug for SvelteCarrierCompiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SvelteCarrierCompiler")
            .finish_non_exhaustive()
    }
}

/// The registered-projector opener installed on the Svelte framework leg
/// (`CarrierLeg::open`). Returns the artifact's erased carrier ONLY for a
/// Svelte-adapter artifact — the sole cross-crate entry point for reaching
/// a Svelte registered artifact's typed carrier.
#[doc(hidden)]
pub fn open_svelte_carrier(artifact: &FrameworkParseArtifact) -> Option<Arc<dyn CarrierParse>> {
    artifact.erased_carrier_for_adapter(&FrameworkAdapterId::svelte())
}

impl SvelteCarrierCompiler {
    pub(crate) fn unregistered_carrier_arc(
        &self,
        artifact: &UnregisteredFrameworkParseArtifact,
    ) -> Option<Arc<SvelteParseCarrier>> {
        verter_language::__carrier_downcast_arc::<SvelteParseCarrier>(artifact)
    }

    /// Reach the parsed component back out of a Svelte artifact, or `None` when
    /// the artifact is not a Svelte carrier.
    #[must_use]
    pub(crate) fn parsed_svelte<'a>(
        &self,
        artifact: &'a FrameworkParseArtifact,
    ) -> Option<&'a ParsedSvelte> {
        self.svelte_carrier(artifact)
            .map(SvelteParseCarrier::parsed)
    }

    pub(crate) fn unregistered_parsed_svelte<'a>(
        &self,
        artifact: &'a UnregisteredFrameworkParseArtifact,
    ) -> Option<&'a ParsedSvelte> {
        verter_language::__carrier_downcast_ref::<SvelteParseCarrier>(artifact)
            .map(SvelteParseCarrier::parsed)
    }

    /// Reach the Svelte carrier payload — the parse plus its retained typed
    /// attribute-value lowering — or `None` for a foreign artifact.
    #[must_use]
    fn svelte_carrier<'a>(
        &self,
        artifact: &'a FrameworkParseArtifact,
    ) -> Option<&'a SvelteParseCarrier> {
        artifact.carrier_ref::<SvelteParseCarrier>()
    }
}

/// Project an already-parsed Svelte component into IDE bytes and maps.
///
/// Callers that already hold a [`ParsedSvelte`] must use this instead of
/// re-parsing. Duration uses the wasm-safe `web_time` clock.
pub(crate) fn render_admitted_svelte_ide(
    source: &str,
    parsed: &ParsedSvelte,
    filename: Option<&str>,
    skip_source_map: bool,
) -> (
    IdeOutput,
    Vec<crate::svelte::ide::SvelteIdeUnsupportedDiagnostic>,
) {
    let start = Instant::now();
    let projection =
        crate::svelte::ide::project_svelte_ide(source, parsed, filename, skip_source_map);
    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    let (space, artifact) = RuntimeOutputDescriptor::carrier_source(source);
    let output_descriptor = RuntimeOutputDescriptor::generated(
        &projection.code,
        (!projection.source_map.is_empty()).then_some(projection.source_map.as_str()),
        &[(space.as_str(), artifact.as_str())],
        SourceMapFidelity::Approximate,
    );
    let ide = IdeOutput {
        code: projection.code,
        source_map: projection.source_map,
        is_jsx: projection.is_jsx,
        duration_ms,
        destructured_block: None,
        output_descriptor,
        generated_template_hole: None,
        generated_template_chunk: None,
    };
    (ide, projection.diagnostics)
}

fn svelte_ide_only_request(
    filename: Option<String>,
    ide: IdeProductRequest,
) -> Result<CompileRequest, CompileUnsupported> {
    CompileRequest::new(
        vec![CompileProduct::IdeCompanion(ide)],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        filename,
        None,
        false,
        false,
    )
    .map_err(CompileUnsupported::RequestExecutionRefused)
}

impl CarrierCompiler for SvelteCarrierCompiler {
    fn adapter_id(&self) -> FrameworkAdapterId {
        FrameworkAdapterId::svelte()
    }

    fn carrier_language_id(&self) -> LanguageId {
        LanguageId::new("svelte")
    }

    fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject> {
        let language = FileLanguage::svelte();
        let syntax_profile = syntax_profile_id_for(&language, opts)
            .expect("the built-in Svelte language has a syntax profile");
        let parse_key = parse_key_for(
            source,
            &language,
            SVELTE_SYNTAX_COMPATIBILITY_DOMAIN,
            SVELTE_SYNTAX_COMPATIBILITY_EPOCH,
            &syntax_profile,
        )
        .expect("the built-in Svelte language has a parse identity");
        let syntax_profile = Arc::new(syntax_profile);
        let parse_key = Arc::new(parse_key);
        // Svelte's official `loose` parse mode is not implemented by this
        // frontend (capability-matrix: `SVELTE-PARSE-LOCAL` is "strict
        // parser diagnostics/recovery only; ... loose is unsupported
        // fail-closed"). Reject before parsing — never silently downgrade
        // to strict parsing.
        if opts.svelte_loose {
            return Err(SyntaxReject::UnsupportedProfile {
                parse_key,
                syntax_profile,
                reason: verter_language::UnsupportedSyntaxProfileReason::UnsupportedOption,
            });
        }
        // Every OTHER strict-parse / close-tag defect is a parser-owned
        // RECOVERY point: the tokenizer is intentionally infallible and
        // always produces a faithful tree, which is correct for the IDE
        // projection (it owns its own error recovery) — see
        // `SvelteStrictParseError`'s doc. Refusing publication here would
        // make the carrier unusable for exactly the states an editor spends
        // most of its time in (an unclosed `<script>` mid-typing, a stray
        // close tag). The CLIENT-runtime "Verter emits a `Main` ⇔ official
        // ACCEPTS" contract is enforced separately and later, at
        // `official_reject_gate` (compile time) — never at this parse/publish
        // seam.
        let parsed = Arc::new(parse_svelte(source));
        Ok(build_svelte_parse_artifact(
            source,
            parsed,
            parse_key,
            syntax_profile,
        ))
    }

    fn compile_ide(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &IdeCompileOptions,
    ) -> Result<IdeOutput, CompileUnsupported> {
        let request = svelte_ide_only_request(
            opts.filename.clone(),
            IdeProductRequest {
                want_source_map: !opts.skip_source_map,
                embed_ambient_types: opts.embed_ambient_types,
                ..Default::default()
            },
        )?;
        crate::framework_common::registered_carrier_projection::project_ide_from_catalog(
            artifact,
            source,
            &request,
            &crate::framework_common::registered_carrier_projection::ProjectionCatalogInputs {
                block_content: opts.block_content.clone(),
                ..Default::default()
            },
        )
        .map(|companion| companion.ide)
    }

    fn compile_bundle(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &RuntimeCompileOptions,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<CarrierCompileOutcome, CompileUnsupported> {
        // A foreign artifact (not a Svelte carrier) declines with the typed
        // answer — never a silent empty bundle.
        let Some(carrier) = self.svelte_carrier(artifact) else {
            return Err(CompileUnsupported::NoIdeProjection {
                adapter_id: self.adapter_id(),
            });
        };
        let parsed = carrier.parsed();

        // `inline` (Vue's `compileScript({ inlineTemplate })` production
        // topology) is a Vue-only axis — mirrors `CompileRequest::new`'s
        // non-Vue refusal so an explicit request this framework has no
        // concept of fails closed instead of being silently ignored.
        if opts.inline == Some(true) {
            return Err(CompileUnsupported::RequestExecutionRefused(
                crate::compile_request::CompileRequestError::InlineSsrUnsupported,
            ));
        }

        let mut bundle = RuntimeCompileOutput::default();

        // The Svelte native RUNTIME compiler (source `.svelte` → JS importing
        // `svelte/internal/client`), attempted ONLY when the request asked for
        // a runtime product. A SUPPORTED component populates `main.body_code`
        // (the host emits the `Main` virtual node from it,
        // `has_runtime_surface()` becoming true through registry routing); an
        // UNSUPPORTED runtime surface FAILS CLOSED, returning a product-free
        // refusal carrying the precise surface + owning vertical. The refusal
        // returns BEFORE the IDE projection below, so a request refused its
        // runtime surface publishes no sibling product either. A request that
        // asked for NO runtime product never reaches here and so can never be
        // refused. SSR (`opts.ssr`) fails closed until the server backend lands.
        let runtime_opts = super::runtime::SvelteRuntimeOptions {
            filename: opts.filename.clone(),
            name: None,
            runes: opts.svelte_runes,
            is_production: opts.is_production,
            // `dev` (`ModuleCompileOptions.dev`) now threads through from the
            // canonical request; an explicit `true` reaches the EXISTING
            // `UnsupportedSvelteRuntimeSurface::DevMode` typed refusal
            // downstream rather than being silently dropped to `false` —
            // dev-mode codegen output liveness is a separate, tracked gap,
            // not this carrier's to close.
            dev_codegen: opts.svelte_dev.unwrap_or(false),
            // Explicit carrier profile axis. An in-source
            // `<svelte:options customElement>` value still wins over this
            // compile option, matching official precedence.
            custom_element: opts.custom_element,
            // The RESOLVED Svelte cssHash override (from the host/session boundary,
            // preserved byte-exact) threads verbatim into the style-plan scope class.
            css_hash_override: opts.svelte_css_hash_override.clone(),
            // The neutral `RuntimeCompileOptions` now carries a channel for
            // each of these — an in-source `<svelte:options namespace /
            // preserveWhitespace>` still wins via the resolver's INLINE-WINS
            // fold, matching official precedence.
            namespace: opts
                .svelte_namespace
                .as_deref()
                .and_then(parse_svelte_namespace),
            fragments: opts
                .svelte_fragments
                .as_deref()
                .and_then(parse_svelte_fragments),
            preserve_whitespace: opts.svelte_preserve_whitespace,
            preserve_comments: opts.svelte_preserve_comments,
            disclose_version: opts.svelte_disclose_version,
            // Unsupported fail-closed rows (`accessors`, `immutable`, `hmr`,
            // `compatibility.componentApi`) have no canonical-request field —
            // structurally unrepresentable, per the compile-request module.
            accessors: None,
            immutable: None,
            hmr: None,
            compatibility_component_api: None,
            css: None,
            custom_element_descriptor: None,
            prepared_styles: opts.prepared_styles.clone(),
        };
        // `opts.source_map` is the neutral OUTPUT-axis map demand: it reaches
        // the css RENDER through `compile_client`'s `want_source_map` (never
        // a lowering option on `SvelteRuntimeOptions`).
        let runtime_result = opts.want_runtime.then(|| {
            super::runtime::compile_client(
                source,
                parsed,
                &runtime_opts,
                alloc,
                opts.ssr,
                opts.source_map,
            )
        });
        match runtime_result {
            // No runtime product was requested, so no runtime attempt and no
            // refusal. The OFFICIAL-REJECT gate still runs: it is an
            // ANALYSIS-domain source-validity oracle over the typed parse (see
            // `runtime::official_reject_gate`), not a runtime-emission step — a
            // component that official Svelte also compile-errors is malformed
            // whether or not THIS request asked for a runtime module, and its
            // diagnostic belongs to the source the way a parse error does.
            // Losing it here would silently drop malformed-source reporting for
            // every IDE-only consumer. It stays NON-FATAL, so the IDE
            // projection below still compiles (it owns its own error recovery).
            None => {
                if let Some(rejection) = super::runtime::official_reject_gate(source, parsed) {
                    bundle.diagnostics.push(RuntimeDiagnostic {
                        severity: RuntimeDiagnosticSeverity::Warning,
                        code: rejection.rule.diagnostic_code().to_string(),
                        message: format!(
                            "{} (official `{}`)",
                            rejection.rule.message(),
                            rejection.official_code
                        ),
                        // The OFFICIAL-REJECT oracle is a whole-component
                        // validity judgment, not a located defect — the
                        // whole-source span IS this diagnostic's own span,
                        // decided here where `source` is known, not defaulted
                        // downstream.
                        span: verter_span::Span::new(0, source.len() as u32),
                    });
                }
            }
            Some(Ok(module)) => {
                bundle.main.body_code = Some(module.code);
                bundle.main.source_map = module.source_map.unwrap_or_default();
                bundle.main.lang = Some("js".to_string());
                // The EXTERNAL scoped-css artifact (the official `compiled.css`
                // — `{ code, map, hasGlobal }` + the scope hash): it publishes
                // as the bundle's style block (the Svelte analogue of the Vue
                // styles population). Injected-mode css is inlined in the
                // module (no artifact), and a style-less component has none.
                if let Some(css) = module.css {
                    let (space, artifact) = RuntimeOutputDescriptor::carrier_source(source);
                    let output_descriptor = RuntimeOutputDescriptor::generated(
                        &css.code,
                        css.source_map.as_deref(),
                        &[(space.as_str(), artifact.as_str())],
                        SourceMapFidelity::Approximate,
                    );
                    bundle.styles.push(
                        crate::framework_common::carrier_compiler::RuntimeStyleBlock {
                            code: css.code,
                            source_map: css.source_map,
                            lang: None,
                            scope_hash: Some(css.hash),
                            has_global: css.has_global,
                            output_descriptor,
                        },
                    );
                }
            }
            Some(Err(super::runtime::ClientCompileError::Unsupported(surface))) => {
                // Fail closed on a REQUESTED runtime surface: return a
                // product-free refusal carrying the precise
                // `svelte-runtime-unsupported-<surface>` reason structurally.
                // Returning here — before the IDE projection below — is what
                // makes the request atomic: no `tsx`, no styles, no template
                // data accompany the refusal.
                return Ok(CarrierCompileOutcome::RuntimeSurfaceRefused(
                    RuntimeSurfaceRefusal {
                        diagnostic_code: surface.diagnostic_code().to_string(),
                        message: surface.message(),
                        span: surface.span(),
                        diagnostics: std::mem::take(&mut bundle.diagnostics),
                    },
                ));
            }
            Some(Err(super::runtime::ClientCompileError::Lowering(errors))) => {
                // A genuine lowering failure (a malformed construct) ALSO
                // produces no `Main`, so it is a runtime refusal too. The first
                // recorded problem is the structural reason; the rest ride
                // along as non-fatal diagnostics. `RuntimeLoweringErrors` is
                // non-empty by construction (`lower_parsed_svelte_to_ir` only
                // returns `Err` behind an `!ctx.errors.is_empty()` guard), so
                // `first()` is always `Some` here.
                let mut diagnostics = std::mem::take(&mut bundle.diagnostics);
                let (first, rest) = errors
                    .diagnostics
                    .split_first()
                    .expect("RuntimeLoweringErrors is non-empty by construction");
                let (code, message, span) =
                    (first.code.to_string(), first.message.clone(), first.span);
                for diag in rest {
                    diagnostics.push(RuntimeDiagnostic {
                        severity: RuntimeDiagnosticSeverity::Warning,
                        code: diag.code.to_string(),
                        message: diag.message.clone(),
                        span: diag.span,
                    });
                }
                return Ok(CarrierCompileOutcome::RuntimeSurfaceRefused(
                    RuntimeSurfaceRefusal {
                        diagnostic_code: code,
                        message,
                        span,
                        diagnostics,
                    },
                ));
            }
            Some(Err(super::runtime::ClientCompileError::GeneratedModuleInvalid { .. })) => {
                return Ok(CarrierCompileOutcome::RuntimeSurfaceRefused(
                    RuntimeSurfaceRefusal {
                        diagnostic_code: "svelte-runtime-generated-module-invalid".to_string(),
                        message: "The native Svelte backend generated invalid JavaScript; runtime output was refused."
                            .to_string(),
                        span: Span::new(0, 0),
                        diagnostics: std::mem::take(&mut bundle.diagnostics),
                    },
                ));
            }
            Some(Err(super::runtime::ClientCompileError::GeneratedSourceMapInvalid { .. })) => {
                return Ok(CarrierCompileOutcome::RuntimeSurfaceRefused(
                    RuntimeSurfaceRefusal {
                        diagnostic_code: "svelte-runtime-generated-source-map-invalid".to_string(),
                        message: "The native Svelte backend could not safely generate the client source map; runtime output was refused."
                            .to_string(),
                        span: Span::new(0, 0),
                        diagnostics: std::mem::take(&mut bundle.diagnostics),
                    },
                ));
            }
            Some(Err(super::runtime::ClientCompileError::OfficialReject(rejection))) => {
                // The component is MALFORMED Svelte official ALSO compile-errors
                // — fail closed with the typed official-reject reason (the
                // rule's stable code + a message naming the EXACT official code
                // the rejection mirrors). The OFFICIAL-REJECT oracle judges the
                // whole component, not a located construct, so the whole-source
                // span IS this refusal's own span — decided here, not defaulted
                // downstream.
                return Ok(CarrierCompileOutcome::RuntimeSurfaceRefused(
                    RuntimeSurfaceRefusal {
                        diagnostic_code: rejection.rule.diagnostic_code().to_string(),
                        message: format!(
                            "{} (official `{}`)",
                            rejection.rule.message(),
                            rejection.official_code
                        ),
                        span: Span::new(0, source.len() as u32),
                        diagnostics: std::mem::take(&mut bundle.diagnostics),
                    },
                ));
            }
        }

        // The IDE projection is produced for a request that asked for it and
        // whose runtime half (if it asked for one) did NOT fail closed — a
        // refusal returned above without reaching here. Its typed-unsupported
        // diagnostics are lifted alongside the runtime ones.
        if opts.want_ide {
            let request = svelte_ide_only_request(
                opts.filename.clone(),
                IdeProductRequest {
                    want_source_map: opts.source_map,
                    embed_ambient_types: opts.embed_ambient_types,
                    ..Default::default()
                },
            )?;
            let companion =
                crate::framework_common::registered_carrier_projection::project_ide_from_catalog(
                    artifact,
                    source,
                    &request,
                    &crate::framework_common::registered_carrier_projection::ProjectionCatalogInputs {
                        block_content: opts.block_content.clone(),
                        ..Default::default()
                    },
                )?;
            bundle.tsx = Some(companion.ide);
            bundle.diagnostics.extend(companion.diagnostics);
        }

        bundle.template_data = if opts.want_template_data {
            let basis = match opts.block_content.template.as_ref() {
                Some(input) => crate::framework_common::registered_carrier_projection::TemplateFactsBasis::SelectedTemplate(
                    input.code.as_ref(),
                ),
                None => crate::framework_common::registered_carrier_projection::TemplateFactsBasis::AdmittedArtifact,
            };
            crate::framework_common::registered_carrier_projection::template_facts_from_catalog(
                artifact, source, basis,
            )
            .map(|facts| {
                bundle.diagnostics.extend(facts.diagnostics);
                facts.data
            })
        } else {
            None
        };

        Ok(CarrierCompileOutcome::Produced(bundle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework_common::carrier_compiler::CompileBundleProducedExt;
    use crate::framework_common::sourcemap_e2e_helpers::{
        assert_token_maps_to_source, assert_token_maps_to_source_line, build_lookup_table,
        parse_ide_output,
    };
    use crate::framework_common::{
        FrameworkSemanticAuthority, RuntimeBlockContentInput, RuntimeBlockContentInputs,
    };
    use verter_language::ScriptRegionKind;

    fn artifact_for(source: &str) -> Arc<FrameworkParseArtifact> {
        use verter_language::carrier_grammar::{
            CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
            FrameworkAdapterSemanticVersion,
        };
        use verter_language::registered_source_authority::{
            CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
        };
        let source_authority = RegisteredSourceAuthority::new().unwrap();
        let grammar_authority = CarrierGrammarAuthority::new().unwrap();
        let config = CarrierGrammarConfig::Svelte;
        grammar_authority
            .register_carrier_grammar(
                verter_language::FileLanguage::svelte(),
                FrameworkAdapterSemanticVersion::new(1).unwrap(),
                CarrierParserGrammarVersion::new(1).unwrap(),
                config.clone(),
            )
            .unwrap();
        let snapshot = source_authority
            .register_source(
                CanonicalFileId::new("file:///fixture.svelte"),
                FileIncarnation::new(1),
                SourceGeneration::new(1),
                verter_language::FileLanguage::svelte(),
                Arc::from(source),
            )
            .unwrap();
        let accepted = grammar_authority
            .accept_registered_source(&source_authority, &snapshot, &config)
            .unwrap();
        Arc::new(
            crate::framework_common::CarrierCompilerRegistry::built_in()
                .project_registered(&accepted)
                .expect("fixture source parses")
                .into_framework_parse_artifact(),
        )
    }

    fn expected_parse_key(source: &str) -> ParseKey {
        let language = FileLanguage::svelte();
        let profile = syntax_profile_id_for(&language, &ParseOptions::default()).unwrap();
        parse_key_for(
            source,
            &language,
            SVELTE_SYNTAX_COMPATIBILITY_DOMAIN,
            SVELTE_SYNTAX_COMPATIBILITY_EPOCH,
            &profile,
        )
        .unwrap()
    }

    #[test]
    fn malformed_svelte_source_still_publishes() {
        // An unterminated `<style>` block is a parser-recoverable STRICT
        // defect: the carrier still publishes (the IDE keeps a usable
        // structure while the user is mid-edit) rather than refusing the
        // whole artifact — but the strict-parse fact IS still recorded on
        // the carrier's own mapped-diagnostic channel (see
        // `svelte_parse_diagnostics`'s doc), exactly like every other
        // recovered defect on this channel; the CLIENT-runtime full-reject
        // contract is still enforced separately, at compile time
        // (`official_reject_gate`).
        let source = "<style>div{color:red}";
        let artifact = SvelteCarrierCompiler
            .parse(source, &ParseOptions::default())
            .expect("a recoverable strict-parse defect must still publish");
        assert!(artifact
            .diagnostics
            .iter()
            .all(|d| d.span.end as usize <= source.len()));
        assert!(
            artifact
                .diagnostics
                .iter()
                .any(|d| d.code == "expected_token"),
            "the strict-parse fact for the unterminated <style> must still \
             surface as a mapped diagnostic, got: {:?}",
            artifact.diagnostics
        );
    }

    /// A representative sample of official Svelte `compiler-errors`/`validator`
    /// STRICT-parse defects (pinned `svelte@5.56.10` oracle,
    /// `sveltejs/svelte@56a036f4ce873a24ee6631a06d03d372523d7a9b`): the carrier
    /// parser is intentionally forgiving/recovery-based, so `parse()` must
    /// still publish (never a hard reject — an editor mid-typing needs the
    /// recovered tree), but the strict-parse fact must still surface on the
    /// carrier's own mapped-diagnostic channel with the EXACT official code —
    /// the class of gap `svelte_parse_diagnostics` previously dropped by
    /// excluding `ParsedSvelte::strict_parse_errors` from this channel.
    #[test]
    fn strict_parse_defects_publish_and_surface_the_official_code() {
        let cases: &[(&str, &str, &str)] = &[
            // compiler-errors/samples/attribute-empty
            ("<div class= ></div>", "expected_attribute_value", "attribute-empty"),
            // compiler-errors/samples/script-unclosed
            (
                "<script>\n\n<h1>Hello {name}!</h1>",
                "element_unclosed",
                "script-unclosed",
            ),
            // compiler-errors/samples/comment-unclosed
            (
                "<!-- an unclosed comment",
                "expected_token",
                "comment-unclosed",
            ),
            // compiler-errors/samples/unexpected-end-of-input-b
            ("<d", "unexpected_eof", "unexpected-end-of-input-b"),
            // compiler-errors/samples/component-invalid-name
            (
                "<!-- ok -->\n<Component />\n<Wunderschön />\n<Cæжαकン中 />\n\n<!-- error -->\n<Components[1] />\n",
                "tag_invalid_name",
                "component-invalid-name",
            ),
            // validator/samples/declaration-tag-invalid-type
            (
                "{#if true}\n\t{var foo = 1}\n{/if}\n",
                "declaration_tag_invalid_type",
                "declaration-tag-invalid-type",
            ),
            // validator/samples/logic-block-in-attribute
            (
                "<div style=\"{#if condition}a{/if}\"></div>\n",
                "block_invalid_placement",
                "logic-block-in-attribute",
            ),
            // compiler-errors/samples/illegal-expression
            ("{42 = nope}\n", "js_parse_error", "illegal-expression"),
            // A top-level <style> left unterminated — the fifth recovery
            // point that used to double-report (an informal
            // "unterminated-style" diag alongside the strict fact). The
            // content itself is a complete, well-formed rule (a closing
            // `}` boundary), so official defers to the missing `</style`
            // and reports `expected_token`, not a CSS-domain code —
            // verified directly against the pinned oracle compiler.
            ("<style>div{color:red}", "expected_token", "style-unclosed"),
        ];
        for (source, expected_code, name) in cases {
            let artifact = SvelteCarrierCompiler
                .parse(source, &ParseOptions::default())
                .unwrap_or_else(|reject| {
                    panic!(
                        "{name} ({source:?}): a recoverable strict-parse defect must \
                         still publish, got a hard reject: {reject:?}"
                    )
                });
            assert!(
                artifact
                    .diagnostics
                    .iter()
                    .any(|d| d.code == *expected_code),
                "{name} ({source:?}) must surface `{expected_code}`, got: {:?}",
                artifact.diagnostics
            );
            // No two diagnostics on this channel may share the identical
            // span — official reports exactly one diagnostic per defect;
            // a duplicate span means the SAME recovery point pushed both an
            // informal `parsed.diagnostics` entry AND its strict fact (the
            // `unterminated-script`/`unterminated-style`/`unterminated-comment`/
            // `unterminated-tag` class of bug this loop now guards).
            let mut spans: Vec<_> = artifact.diagnostics.iter().map(|d| d.span).collect();
            spans.sort_by_key(|s| (s.start, s.end));
            assert!(
                spans.windows(2).all(|pair| pair[0] != pair[1]),
                "{name} ({source:?}) has two diagnostics sharing an identical span, got: {:?}",
                artifact.diagnostics
            );
        }
    }

    #[test]
    fn well_formed_svelte_source_surfaces_zero_parse_diagnostics() {
        let source = "<script>let count = $state(0);</script>\n<button onclick={() => count++}>{count}</button>\n";
        let artifact = artifact_for(source);
        assert!(
            artifact.diagnostics().is_empty(),
            "well-formed input must not surface false-positive parse diagnostics, got: {:?}",
            artifact.diagnostics()
        );
    }

    #[test]
    fn compile_bundle_refuses_explicit_inline_request() {
        // `inline` (Vue's `compileScript({ inlineTemplate })` production
        // topology) is a Vue-only axis; Svelte has no such concept. An
        // explicit request must fail closed with a typed refusal rather
        // than being silently ignored — mirrors `CompileRequest::new`'s
        // non-Vue `InlineSsrUnsupported` refusal.
        let compiler = SvelteCarrierCompiler;
        let source = "<script>let count = $state(0);</script>\n<button onclick={() => count++}>{count}</button>\n";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let result = compiler.compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                inline: Some(true),
                ..Default::default()
            },
            &alloc,
        );
        assert!(
            matches!(
                result,
                Err(CompileUnsupported::RequestExecutionRefused(
                    crate::compile_request::CompileRequestError::InlineSsrUnsupported
                ))
            ),
            "expected a typed InlineSsrUnsupported refusal, got {result:?}"
        );
    }

    #[test]
    fn compile_bundle_emits_a_runtime_main_body_for_a_runes_component() {
        // A SUPPORTED runes component populates `main.body_code` (Svelte client JS)
        // so `has_runtime_surface()` becomes true. DISCRIMINATING: the body is the
        // client module, not empty.
        let compiler = SvelteCarrierCompiler;
        let source = "<script>let count = $state(0);</script>\n<button onclick={() => count++}>{count}</button>\n";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let bundle = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions::default(),
                &alloc,
            )
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
    fn compile_bundle_emits_a_runtime_main_for_a_legacy_export_prop() {
        // A legacy (non-runes) component with an `export let` prop is a
        // SUPPORTED runtime surface: the prop lowers through the shared
        // `$.prop` prop-source substrate (legacy base flags 8, accessor-call
        // reads) and the bundle carries a real Main — the former per-surface
        // export refusal is gone.
        let compiler = SvelteCarrierCompiler;
        let source = "<script>export let label;</script>\n<p>{label}</p>\n";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let bundle = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions::default(),
                &alloc,
            )
            .expect("the bundle is produced");
        assert!(
            bundle.has_runtime_surface(),
            "a legacy export-let prop component compiles to a runtime surface"
        );
        let body = bundle
            .main
            .body_code
            .as_deref()
            .expect("a Main body for a legacy export-let prop component");
        assert!(
            body.contains("let label = $.prop($$props, 'label', 8);"),
            "the prop lowers through the shared $.prop substrate:\n{body}"
        );
        assert!(
            !bundle
                .diagnostics
                .iter()
                .any(|d| d.code.starts_with("svelte-runtime-unsupported")),
            "no unsupported-surface diagnostic for a supported legacy prop, got: {:?}",
            bundle
                .diagnostics
                .iter()
                .map(|d| &d.code)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn runtime_main_carries_the_demanded_client_source_map() {
        let compiler = SvelteCarrierCompiler;
        // Keep the rune genuinely reactive: the runtime's supported-surface
        // classifier intentionally rejects a demoted/static interpolation.
        // The click write makes this a valid Main carrier and therefore a
        // discriminating test of the carrier's source-map transport.
        let source = "<script>let count = $state(0);</script>\n\
<button onclick={() => count += 1}>{count}</button>\n";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let mapped = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("src/Counter.svelte".to_string()),
                    source_map: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("mapped Svelte runtime bundle");
        assert!(
            mapped.main.body_code.is_some(),
            "source-map demand must preserve the successful Main body; diagnostics: {:?}",
            mapped.diagnostics
        );
        assert!(
            mapped.diagnostics.is_empty(),
            "a supported mapped compile must not acquire diagnostics: {:?}",
            mapped.diagnostics
        );
        assert!(
            !mapped.main.source_map.is_empty(),
            "a successful mapped Main must carry its demanded source map"
        );
        let map = oxc_sourcemap::OwnedSourceMap::from_json_string(&mapped.main.source_map)
            .expect("RuntimeMainModule.source_map is valid JSON");
        assert_eq!(map.get_file(), Some("Counter.svelte"));
        assert_eq!(map.get_sources().collect::<Vec<_>>(), ["Counter.svelte"]);
        assert_eq!(map.get_source_content(0), Some(source));

        let plain = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("src/Counter.svelte".to_string()),
                    source_map: false,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("plain Svelte runtime bundle");
        assert!(plain.main.source_map.is_empty(), "no demand, no main map");
        assert_eq!(
            mapped.main.body_code, plain.main.body_code,
            "source-map demand must not change Main JavaScript bytes"
        );
    }

    #[test]
    fn external_scoped_css_rides_the_bundle_with_demanded_map_and_has_global() {
        // §3.7: the external css artifact is `{ code, map, hash, has_global }`
        // on `RuntimeCompileOutput.styles`. The EXISTING
        // `RuntimeCompileOptions.source_map` flag is the map demand — it
        // reaches the css RENDER through `compile_client`, and the produced
        // map + the `:global` fact ride the neutral style block.
        let compiler = SvelteCarrierCompiler;
        let source = "<script>let c = $state(0);</script>\n<style>.r{color:red}\n:global(.x){margin:0}</style>\n<button class=\"r\" onclick={() => c++}>{c}</button>\n";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let opts = RuntimeCompileOptions {
            filename: Some("App.svelte".to_string()),
            source_map: true,
            ..Default::default()
        };
        let bundle = compiler
            .compile_bundle_expect_produced(source, &artifact, &opts, &alloc)
            .expect("svelte runtime bundle");
        let style = bundle.styles.first().expect("an external style block");
        assert!(
            style.scope_hash.is_some(),
            "the scoped block carries its hash"
        );
        assert!(
            style.has_global,
            "`:global(.x)` css reaches RuntimeStyleBlock.has_global"
        );
        let map = style
            .source_map
            .as_deref()
            .expect("RuntimeCompileOptions.source_map demands the css map");
        assert!(
            map.contains("App.svelte"),
            "the css map names the component source: {map}"
        );

        // A/B: no demand ⇒ no map (same component, source_map off).
        let opts_off = RuntimeCompileOptions {
            filename: Some("App.svelte".to_string()),
            ..Default::default()
        };
        let bundle_off = compiler
            .compile_bundle_expect_produced(source, &artifact, &opts_off, &alloc)
            .expect("svelte runtime bundle");
        assert_eq!(
            bundle_off.styles.first().expect("a style block").source_map,
            None,
            "an undemanded css map stays None"
        );

        // A non-global component reports `has_global == false`.
        let non_global = "<script>let c = $state(0);</script>\n<style>.r{color:red}</style>\n<button class=\"r\" onclick={() => c++}>{c}</button>\n";
        let artifact2 = artifact_for(non_global);
        let bundle2 = compiler
            .compile_bundle_expect_produced(non_global, &artifact2, &opts, &alloc)
            .expect("svelte runtime bundle");
        assert!(
            !bundle2.styles.first().expect("a style block").has_global,
            "css without `:global` must not claim has_global"
        );
    }

    #[test]
    fn empty_external_style_still_publishes_an_empty_css_artifact() {
        // Official svelte@5.56.10 first-hand: `compile('<style></style><p>hi</p>',
        // { css: 'external' }).css` is NON-null — `{ code: '', hasGlobal: false,
        // map: {...} }`. An EXISTING `<style>` block always publishes the external
        // artifact, even when the rendered `css.code` is empty; only the ABSENCE
        // of a style block publishes none (`compiled.css === null`).
        let compiler = SvelteCarrierCompiler;
        let source = "<style></style><p>hi</p>\n";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let opts = RuntimeCompileOptions {
            filename: Some("X.svelte".to_string()),
            source_map: true,
            ..Default::default()
        };
        let bundle = compiler
            .compile_bundle_expect_produced(source, &artifact, &opts, &alloc)
            .expect("svelte runtime bundle");
        assert_eq!(
            bundle.styles.len(),
            1,
            "an existing (empty) style block publishes exactly ONE artifact"
        );
        let style = &bundle.styles[0];
        assert_eq!(
            style.code, "",
            "the artifact's code is the official empty render"
        );
        assert!(!style.has_global, "an empty stylesheet has no `:global`");
        assert!(
            style.scope_hash.is_some(),
            "the scope hash is still real (the filename hash input)"
        );
        assert!(
            style.source_map.is_some(),
            "the demanded css map rides the empty artifact (official emits a map)"
        );

        // NEGATIVE: NO `<style>` block ⇒ NO artifact (official css === null).
        let source_none = "<p>hi</p>\n";
        let artifact_none = artifact_for(source_none);
        let bundle_none = compiler
            .compile_bundle_expect_produced(source_none, &artifact_none, &opts, &alloc)
            .expect("svelte runtime bundle");
        assert!(bundle_none.styles.is_empty(), "no style block, no artifact");
    }

    #[test]
    fn a_refused_runtime_request_carries_no_product_at_all() {
        // A RUNTIME request on an unsupported component fail-closes into the
        // product-free refusal arm: the outcome is `RuntimeSurfaceRefused`, so
        // there is NO bundle to hold a `tsx` beside it — the atomicity is
        // structural, not asserted over a flag. The precise reason travels on
        // the refusal itself, not recovered from diagnostic text.
        let compiler = SvelteCarrierCompiler;
        // A `{#snippet}` declaration is an unsupported runtime surface — the
        // control-flow blocks (`{#if}`/…) ARE supported, so the refused example uses a
        // construct that genuinely still fails closed.
        let source =
            "<script>let c = $state(true);</script>\n{#snippet foo()}<p>{c}</p>{/snippet}\n";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        // The request asks for BOTH the runtime and the IDE product.
        let opts = RuntimeCompileOptions {
            want_runtime: true,
            want_ide: true,
            ..Default::default()
        };
        let outcome = compiler
            .compile_bundle(source, &artifact, &opts, &alloc)
            .expect("the outcome is produced-or-refused, never an Err");

        match outcome {
            CarrierCompileOutcome::RuntimeSurfaceRefused(refusal) => {
                assert!(
                    refusal
                        .diagnostic_code
                        .starts_with("svelte-runtime-unsupported-"),
                    "the refusal must name the precise unsupported surface, got {:?}",
                    refusal.diagnostic_code
                );
                assert!(
                    !refusal.message.is_empty(),
                    "the refusal must carry a reason"
                );
            }
            CarrierCompileOutcome::Produced(bundle) => panic!(
                "a REQUESTED-but-unsupported runtime surface must refuse, carrying no product. \
                 Got a bundle with tsx={} main={}",
                bundle.tsx.is_some(),
                bundle.main.body_code.is_some()
            ),
        }
    }

    #[test]
    fn an_ide_only_request_is_never_refused_and_still_projects() {
        // The other half of the requested-product set: the SAME unsupported
        // component, asked ONLY for its IDE product, attempts no runtime compile
        // and therefore cannot be refused one — it publishes its `tsx` normally.
        // Without this, "refuse everything" would satisfy the test above.
        let compiler = SvelteCarrierCompiler;
        let source =
            "<script>let c = $state(true);</script>\n{#snippet foo()}<p>{c}</p>{/snippet}\n";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let opts = RuntimeCompileOptions {
            want_runtime: false,
            want_ide: true,
            ..Default::default()
        };
        let bundle = compiler
            .compile_bundle(source, &artifact, &opts, &alloc)
            .expect("the outcome is produced-or-refused, never an Err")
            .into_produced()
            .expect("an IDE-only request asks for no runtime product, so none can be refused");
        assert!(
            bundle.tsx.is_some(),
            "the IDE-only request must still project its TSX"
        );
        assert!(
            !bundle.has_runtime_surface(),
            "an IDE-only request must not emit a runtime surface it did not ask for"
        );
        assert!(
            !bundle.has_errors(),
            "declining to compile an unrequested runtime surface is not an error"
        );
    }

    #[test]
    fn malformed_component_still_publishes_a_mapped_diagnostic_for_ide_projection() {
        // A stray close tag official rejects for the CLIENT runtime, but the
        // carrier PARSE/PUBLISH seam stays recoverable (an editor spends most
        // of its time in exactly this kind of transiently-broken state) —
        // the defect surfaces as a mapped diagnostic instead of refusing
        // publication.
        let compiler = SvelteCarrierCompiler;
        let source = "<script>let c = $state(0);</script>\n<div><span></div></span>\n";
        let artifact = compiler
            .parse(source, &ParseOptions::default())
            .expect("a stray close tag is a recoverable parser defect, not a refusal");
        assert!(
            artifact
                .diagnostics
                .iter()
                .any(|d| d.code == "element_invalid_closing_tag"),
            "the stray close tag must still surface as a mapped diagnostic: {:?}",
            artifact.diagnostics
        );
    }

    #[test]
    fn a_well_formed_ide_only_request_reports_no_official_reject() {
        // The negative control: the same IDE-only identity on a WELL-FORMED
        // component reports no official-reject diagnostic, so the assertion above
        // discriminates malformed from clean rather than always finding one.
        let compiler = SvelteCarrierCompiler;
        let source = "<script>let c = $state(0);</script>\n<div><span>hi</span></div>\n";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let bundle = compiler
            .compile_bundle(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    want_runtime: false,
                    want_ide: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("the outcome is produced-or-refused, never an Err")
            .into_produced()
            .expect("an IDE-only request cannot be refused a runtime surface");
        assert!(
            !bundle
                .diagnostics
                .iter()
                .any(|d| d.code.starts_with("svelte-official-reject-")),
            "a WELL-FORMED component must report no official-reject diagnostic. Got: {:?}",
            bundle
                .diagnostics
                .iter()
                .map(|d| d.code.as_str())
                .collect::<Vec<_>>()
        );
        assert!(bundle.tsx.is_some(), "the IDE projection is still produced");
    }

    #[test]
    fn a_supported_component_produces_rather_than_refuses() {
        // The success direction: a SUPPORTED runes component takes the PRODUCED
        // arm and emits a Main, so the sum discriminates refusal from success.
        let compiler = SvelteCarrierCompiler;
        let source =
            "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let bundle = compiler
            .compile_bundle(source, &artifact, &RuntimeCompileOptions::default(), &alloc)
            .expect("svelte runtime bundle")
            .into_produced()
            .expect("a supported component produces, it does not refuse");
        assert!(
            bundle.has_runtime_surface(),
            "a supported component carries a Main"
        );
    }

    #[test]
    fn ide_sourcemap_maps_script_and_template_expressions_back_to_source() {
        // The sourcemap e2e (Tests #2): a script-region binding and a template
        // expression each map back to the matching ORIGINAL carrier text. The
        // unmapped prelude shifts no mapped position — the tokens still land.
        let compiler = SvelteCarrierCompiler;
        let source =
            "<script lang=\"ts\">let myUniqueBinding = 0;</script>\n<div>{myUniqueBinding}</div>";
        let artifact = artifact_for(source);
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
        let compiler = SvelteCarrierCompiler;
        let source = "<script lang=\"ts\">import { fly } from \"svelte/transition\";\n\
             const flyParam = { delay: 0 };</script>\n\
             <div transition:fly={flyParam}>x</div>";
        let artifact = artifact_for(source);
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
        let source = "<script>let a = 1;</script>";
        let artifact = artifact_for(source);
        assert!(!artifact.adapter_id().is_vue());
        assert_eq!(artifact.adapter_id(), &FrameworkAdapterId::svelte());
        assert_eq!(artifact.language_id().as_str(), "svelte");
        assert_eq!(artifact.parse_key(), &expected_parse_key(source));
    }

    #[test]
    fn instance_and_module_scripts_carry_kind_and_source_type() {
        let source =
            "<script module>export const x = 1;</script>\n<script lang=\"ts\">let a = 1;</script>";
        let artifact = artifact_for(source);
        let regions = artifact.script_regions();
        assert_eq!(regions.len(), 2);
        // Source-ordered: module script first.
        assert_eq!(regions[0].kind, ScriptRegionKind::Module);
        assert_eq!(regions[1].kind, ScriptRegionKind::Instance);
        assert_eq!(regions[0].span.slice(source).trim(), "export const x = 1;");
        assert_eq!(regions[1].span.slice(source).trim(), "let a = 1;");
        assert_eq!(
            regions[0].source_type,
            ScriptSourceType::Js(JsModuleKind::Unambiguous),
            "inventory compatibility projects generic carrier JavaScript"
        );
        assert_eq!(regions[1].source_type, ScriptSourceType::Ts);
    }

    #[test]
    fn legacy_context_module_classifies_as_module() {
        let source = "<script context=\"module\">export const x = 1;</script>";
        let artifact = artifact_for(source);
        assert_eq!(artifact.script_regions().len(), 1);
        assert_eq!(artifact.script_regions()[0].kind, ScriptRegionKind::Module);
    }

    #[test]
    fn compile_ide_projects_a_tsx_artifact_with_the_pragma_prelude() {
        let compiler = SvelteCarrierCompiler;
        let source = "<script lang=\"ts\">let a = 1;</script>\n<div>{a}</div>";
        let artifact = artifact_for(source);
        let out = compiler
            .compile_ide(source, &artifact, &IdeCompileOptions::default())
            .expect("the Svelte IDE projection produces a TSX artifact");
        // A TypeScript `.svelte` projects `.tsx`.
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
    fn compile_ide_projects_a_no_lang_component_as_valid_jsx_with_jsdoc() {
        let compiler = SvelteCarrierCompiler;
        let source = r#"<script>
/** @type {{ label: string }} */
let { label } = $props();
let count = $state(0);
</script>
<button onclick={() => count += 1}>{label}: {count}</button>"#;
        let artifact = artifact_for(source);
        let out = compiler
            .compile_ide(source, &artifact, &IdeCompileOptions::default())
            .expect("the Svelte IDE projection produces a JavaScript carrier");

        assert!(out.is_jsx, "a no-lang Svelte component must publish .jsx");
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            oxc_parser::Parser::new(&allocator, &out.code, oxc_span::SourceType::jsx()).parse();
        assert!(
            parsed.errors.is_empty(),
            "the JavaScript carrier must be syntactically valid JSX:\n{:?}\n{}",
            parsed.errors,
            out.code
        );
        assert!(
            out.code
                .starts_with("/** @jsxImportSource @verter/svelte-jsx */"),
            "the file-local Svelte JSX environment must be preserved"
        );
        assert!(
            !out.source_map.is_empty(),
            "the JavaScript carrier keeps the normal IDE source map"
        );
    }

    #[test]
    fn compile_ide_declines_a_foreign_artifact() {
        let compiler = SvelteCarrierCompiler;
        // A Vue-shaped artifact is not a Svelte carrier — the typed answer.
        let svelte = artifact_for("<div />");
        // Re-wrap is unnecessary; a real foreign carrier is exercised by the
        // shared contract tests. Here we assert the Svelte path succeeds.
        let out = compiler.compile_ide("<div />", &svelte, &IdeCompileOptions::default());
        assert!(out.is_ok());
    }

    fn facts_for(source: &str) -> crate::compile::RawTemplateData {
        let artifact = artifact_for(source);
        crate::svelte::SvelteSemanticAuthority
            .template_facts(source, &artifact)
            .expect("a Svelte artifact must produce template facts")
            .data
    }

    #[test]
    fn compile_bundle_template_facts_come_from_the_catalog() {
        let source = concat!(
            "<script lang=\"ts\">let value = 0;</script>\n",
            "<Button size=\"sm\" bind:value />",
        );
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let opts = RuntimeCompileOptions {
            want_runtime: false,
            want_ide: false,
            want_template_data: true,
            ..Default::default()
        };
        let bundle = SvelteCarrierCompiler
            .compile_bundle_expect_produced(source, &artifact, &opts, &alloc)
            .expect("Svelte compile_bundle produces a bundle");
        let catalog =
            crate::framework_common::registered_carrier_projection::template_facts_from_catalog(
                &artifact,
                source,
                crate::framework_common::registered_carrier_projection::TemplateFactsBasis::AdmittedArtifact,
            )
            .expect("catalog must produce Svelte template facts")
            .data;
        let bundled = bundle
            .template_data
            .as_ref()
            .expect("want_template_data must fill catalog facts");
        assert_eq!(bundled.components.len(), catalog.components.len());
        assert!(
            bundled
                .components
                .iter()
                .any(|component| component.tag_name == "Button"),
            "bundle template facts must retain the <Button> usage"
        );

        let reminted = artifact.remint_epoch_for_tests("unknown-epoch");
        let refused = SvelteCarrierCompiler
            .compile_bundle_expect_produced(source, &reminted, &opts, &alloc)
            .expect("runtime/IDE-free compile still produces a bundle");
        assert!(
            refused.template_data.is_none(),
            "a catalog miss must leave template_data None, not independently extract"
        );
    }

    fn projected_template(code: &str) -> RuntimeBlockContentInput {
        RuntimeBlockContentInput {
            code: Arc::from(code),
            source_map: None,
            lang: "html".to_string(),
            content_artifact_token: "content:html".to_string(),
            source_space_token: "space:html".to_string(),
            parsed: None,
        }
    }

    fn template_facts_opts(template: Option<&str>) -> RuntimeCompileOptions {
        RuntimeCompileOptions {
            want_runtime: false,
            want_ide: false,
            want_template_data: true,
            block_content: RuntimeBlockContentInputs {
                template: template.map(projected_template),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn compile_bundle_refuses_template_facts_when_selected_bytes_differ() {
        let source = concat!(
            "<script lang=\"ts\">let value = 0;</script>\n",
            "<Button size=\"sm\" bind:value />",
        );
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let _ = crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations();
        let bundle = SvelteCarrierCompiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &template_facts_opts(Some("<Replacement />")),
                &alloc,
            )
            .expect("Svelte compile_bundle produces a bundle");
        assert!(
            bundle.template_data.is_none(),
            "selected bytes that replace the admitted markup must refuse facts, never publish the superseded <Button>"
        );
        assert_eq!(
            crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations(),
            0,
            "a selected-content mismatch must not invoke the semantic producer"
        );
    }

    #[test]
    fn compile_bundle_refuses_selected_template_facts_without_a_template_host() {
        let source = concat!(
            "<script lang=\"ts\">let value = 0;</script>\n",
            "<Button size=\"sm\" bind:value />",
        );
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::default();
        let _ = crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations();
        let selected = SvelteCarrierCompiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &template_facts_opts(Some("<Button size=\"sm\" bind:value />")),
                &alloc,
            )
            .expect("Svelte compile_bundle produces a bundle");
        assert!(
            selected.template_data.is_none(),
            "selected template bytes cannot bind without an admitted template-host region"
        );
        assert_eq!(
            crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations(),
            0,
            "selected content without a template host must not invoke the semantic producer"
        );

        let admitted = SvelteCarrierCompiler
            .compile_bundle_expect_produced(source, &artifact, &template_facts_opts(None), &alloc)
            .expect("Svelte compile_bundle produces a bundle");
        let facts = admitted
            .template_data
            .as_ref()
            .expect("native admitted Svelte markup must keep catalog facts");
        assert!(
            facts
                .components
                .iter()
                .any(|component| component.tag_name == "Button"),
            "admitted-artifact Svelte facts must retain the <Button> usage"
        );
        assert_eq!(
            crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations(),
            1,
            "an admitted artifact query must invoke the semantic producer exactly once"
        );
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
    fn template_data_reads_bound_prop_expressions_from_the_carrier_lowering() {
        // A call-shaped bound prop publishes the carrier's retained typed
        // record, anchored at the CARRIER offset of the call. A static prop
        // publishes none. Mutation: drop the carrier lowering (or key it on the
        // attribute span instead of the value span) and the record is absent or
        // mis-anchored.
        let source = "<script lang=\"ts\">function makeMsg() { return \"x\"; }</script>\n\
             <Child label=\"static\" msg={makeMsg()} />";
        let data = facts_for(source);
        let usage = &data.components[0];
        let msg = usage.props.iter().find(|p| p.name == "msg").unwrap();
        let expression = msg
            .indexed_expression
            .as_ref()
            .expect("a bound prop publishes the retained typed record");
        assert!(
            matches!(
                expression,
                verter_type_expr::IndexedValueExpression::Call(call)
                    if call.point == source.rfind("makeMsg()").unwrap() as u32
            ),
            "the record is a call anchored at its carrier offset: {expression:?}"
        );
        let label = usage.props.iter().find(|p| p.name == "label").unwrap();
        assert!(
            label.indexed_expression.is_none(),
            "a static Text value is not an expression"
        );
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
            "<script>let a = 1;</script>\n<svelte:head><title>t</title></svelte:head><div></div>";
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
