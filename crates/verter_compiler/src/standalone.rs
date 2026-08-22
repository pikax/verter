//! The internal compiler's sole raw-source direct-compile boundary ("R1",
//! the borrowed one-shot direct route): every caller supplies a canonical
//! [`crate::compile_request::CompileRequest`] (built through
//! [`CompileRequest::new`](CompileRequest::new), which enforces every
//! construction-time fail-closed rule) plus the framework-tagged
//! [`DirectExecutionInputs`] carrier for resolved facts excluded from
//! request identity, and gets back exactly one atomic
//! [`crate::assembly::ArtifactSet`] — the SAME B4 publication boundary
//! every host-backed route publishes through, never a second one — plus a
//! [`DirectCompileOutput`] sibling for the two facts B4's sealed
//! `ProductKind`/`ArtifactContribution`/`publish()` model has no carrier
//! for at all (style/CSS content, and non-fatal compile diagnostics): both
//! are HOST-side siblings in every registered route (a virtual style file,
//! a diagnostics snapshot) for BOTH frameworks, so a one-shot compile with
//! no host/virtual-file system needs its own explicit sibling to avoid
//! silently discarding real computed output.
//!
//! [`StandaloneCompiler::compile`] dispatches solely on
//! `request.framework()`, builds the [`crate::compile_request::CompileRequest`]'s
//! own [`crate::assembly::ProductPlan`] once, compiles or composes only the
//! requested products, and calls [`crate::assembly::publish`] exactly once
//! over the full contribution set — including BOTH runtime products when a
//! request legitimately plans `RuntimeClient` AND `RuntimeServer` together
//! (independent, co-requestable products per
//! [`crate::compile_request::CompileRequest`]'s own doc). Vue's
//! runtime-module composition (the `__sfc__` rewrite, script/template/
//! import fragment minting, sequencing) is the SAME
//! [`crate::assembly::vue_module`] machinery `verter_session`'s
//! host-decorated `assemble_vue_main_module` shares — this route just
//! supplies empty host-decoration extras (no host state exists for a
//! one-shot compile). Svelte's client compile
//! ([`crate::svelte::runtime::compile_client`]) is likewise the SAME
//! algorithm the host carrier drives.

use oxc_allocator::Allocator;

use crate::assembly::fragment::{
    DeclaredImport, Fragment, FragmentDialect, FragmentRefusal, FrameworkDomain, PlacementSlot,
    SyntacticContract,
};
use crate::assembly::plan::ProductPlan;
use crate::assembly::publish::{publish, ArtifactContribution};
use crate::assembly::source_space::SourceSpaceKind;
use crate::assembly::source_unit::SourceUnitId;
use crate::assembly::vue_module::{
    compose_fragments, ComposedFragments, VueMainCompositionFailure, VueMainModuleRequest,
};
use crate::assembly::{ArtifactSet, AssemblyRefusal};
use crate::compile::types::{CompileDiagnostic, VueExecutionInputs};
use crate::compile::VueMacroSemanticInput;
use crate::compile_request::{
    CompileProduct, CompileRequest, CompileRequestError, FrameworkCompileRequest, ProductKind,
};
use crate::framework_common::{RuntimeCompileOutput, RuntimeOutputDescriptor, RuntimeStyleBlock};
use crate::parser::types::{sfc_script_dialect, ParsedSfc, SfcScriptDialect};
use crate::svelte::runtime::{
    compile_client, ClientCompileError, SvelteFragments, SvelteNamespace, SvelteRuntimeOptions,
};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};

/// Ephemeral, non-identity execution inputs for a Svelte compile — resolved
/// framework facts threaded alongside a canonical
/// [`crate::compile_request::CompileRequest`] but EXCLUDED from its
/// identity, mirroring [`VueExecutionInputs`]'s role for Vue. NOT a second
/// option authority: `css_hash_override` is the same session/host-resolved
/// fact [`SvelteRuntimeOptions::css_hash_override`] already carries — the
/// official user `cssHash` callback's already-computed result, preserved
/// byte-exact. Genuine Svelte semantic options (`runes`, `namespace`,
/// `fragments`, …) live on [`crate::compile_request::SvelteCompileRequest`]
/// per B3 and are never duplicated here.
#[derive(Debug, Clone, Default)]
pub struct SvelteExecutionInputs {
    pub css_hash_override: Option<String>,
}

/// The framework-tagged borrowed execution-input carrier
/// [`StandaloneCompiler::compile`] takes alongside a canonical
/// [`CompileRequest`]. The request's own declared framework
/// ([`CompileRequest::framework`]) and this carrier's variant must agree —
/// disagreement is a typed [`DirectCompileError::FrameworkMismatch`], never
/// a panic.
pub enum DirectExecutionInputs<'a> {
    Vue {
        execution: &'a VueExecutionInputs,
        macros: &'a VueMacroSemanticInput,
    },
    Svelte {
        execution: &'a SvelteExecutionInputs,
    },
}

/// [`StandaloneCompiler::compile`]'s successful result: the atomic
/// [`ArtifactSet`] every planned product publishes into, plus two siblings
/// B4's sealed publication model carries no slot for.
///
/// `styles` is the style/CSS content a compiled `RuntimeClient`/
/// `RuntimeServer` product's own `<style>` block(s) produce — in every
/// registered host route this rides as a SEPARATE virtual file the
/// `RuntimeClient`/`RuntimeServer` artifact only `import`s by reference
/// (Vue) or as an external scoped-css artifact beside the client module
/// (Svelte); a one-shot compile with no virtual-file system has nowhere
/// else to put it, so it rides here instead. Empty (never missing) when the
/// component has no style output.
///
/// `diagnostics` is the compile's own non-fatal diagnostic channel — Vue's
/// `VerterCompileResult::errors`; always empty for Svelte, whose
/// `compile_client` is refuse-by-default (a diagnostic-worthy defect is
/// always a hard [`DirectCompileError::Svelte`] refusal there, never a
/// soft, coexisting-with-success diagnostic).
#[derive(Debug)]
pub struct DirectCompileOutput {
    pub artifacts: ArtifactSet,
    pub styles: Vec<RuntimeStyleBlock>,
    pub diagnostics: Vec<CompileDiagnostic>,
}

/// Every way [`StandaloneCompiler::compile`] can fail. No variant carries a
/// partial [`DirectCompileOutput`] — a refusal here means nothing was
/// published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectCompileError {
    /// `request.framework()` and the supplied [`DirectExecutionInputs`]
    /// variant disagree.
    FrameworkMismatch {
        expected: &'static str,
        actual: &'static str,
    },
    /// A Vue construction/resolution-time refusal (the two `SSR x Vapor` /
    /// `inline x Vapor` cases construction could not see, or a genuine
    /// compile diagnostic path — see
    /// [`crate::compile_request::CompileRequestError`]).
    Vue(CompileRequestError),
    /// Vue main-module fragment composition failed (an invalid `__sfc__`
    /// fact, a fragment grammar violation, or a sequencing defect) — see
    /// [`VueMainCompositionFailure`].
    VueComposition(VueMainCompositionFailure),
    /// The Svelte client backend refused the component (an official
    /// compile-error parity reject, an unsupported surface, or an internal
    /// codegen-invariant failure) — see [`ClientCompileError`]. SSR requests
    /// always land here today (the server backend fails closed until it
    /// lands) — this route never reinterprets that as anything else.
    Svelte(ClientCompileError),
    /// A Svelte-produced module failed its own declared fragment grammar —
    /// an internal codegen-invariant failure (the same class
    /// [`ClientCompileError::GeneratedModuleInvalid`] guards downstream of
    /// `compile_client` itself), reported here because it surfaces at this
    /// route's own fragment-validation step instead.
    SvelteFragment(FragmentRefusal),
    /// The request's `namespace` selection
    /// ([`crate::compile_request::svelte::SvelteNamespaceRequest::Foreign`])
    /// has no representation on the compiler-internal
    /// [`SvelteNamespace`] this route resolves into — neither this route
    /// nor the host route (which never round-trips this specific enum at
    /// all, see the B5 fix-round evidence record) has ever had to answer
    /// what it means, so it fails closed rather than silently defaulting to
    /// HTML.
    UnsupportedSvelteNamespace,
    /// The final atomic-publication boundary refused the composed
    /// contribution set (exact-cardinality, required-map,
    /// undeclared-helper, or final-parse checks).
    Publish(AssemblyRefusal),
    /// The request planned a product this direct route does not (yet)
    /// produce. Never a silent skip — every planned artifact this route
    /// cannot supply is a typed refusal, matching `publish`'s own
    /// `MissingPlannedArtifact` philosophy at this route's own boundary
    /// (before a plan even reaches `publish`).
    UnsupportedProduct(ProductKind),
}

impl From<AssemblyRefusal> for DirectCompileError {
    fn from(failure: AssemblyRefusal) -> Self {
        Self::Publish(failure)
    }
}

/// Stateless compiler for callers that do not participate in a registered
/// host.
#[derive(Debug, Default, Clone, Copy)]
pub struct StandaloneCompiler;

impl StandaloneCompiler {
    /// Compile borrowed standalone source into exactly the
    /// [`DirectCompileOutput`] `request` plans — the internal compiler's
    /// sole raw-source parser boundary; registered hosts consume their
    /// elected artifact through their own host-backed routes instead.
    ///
    /// Dispatches solely on `request.framework()`; `inputs`'s variant must
    /// agree (a mismatch is a typed refusal, never a panic). Every planned
    /// product not yet produced by this route is a typed
    /// [`DirectCompileError::UnsupportedProduct`] — never silently skipped
    /// and never a partial [`ArtifactSet`]. A request legitimately planning
    /// BOTH `RuntimeClient` and `RuntimeServer` (independent,
    /// co-requestable products) publishes both, in the SAME atomic
    /// `publish()` call.
    pub fn compile<'a>(
        &self,
        source: &'a str,
        request: &CompileRequest,
        inputs: DirectExecutionInputs<'a>,
    ) -> Result<DirectCompileOutput, DirectCompileError> {
        match (request.framework(), inputs) {
            (FrameworkCompileRequest::Vue(_), DirectExecutionInputs::Vue { execution, macros }) => {
                self.compile_vue(source, request, execution, macros)
            }
            (FrameworkCompileRequest::Svelte(_), DirectExecutionInputs::Svelte { execution }) => {
                self.compile_svelte(source, request, execution)
            }
            (FrameworkCompileRequest::Vue(_), DirectExecutionInputs::Svelte { .. }) => {
                Err(DirectCompileError::FrameworkMismatch {
                    expected: "Vue",
                    actual: "Svelte",
                })
            }
            (FrameworkCompileRequest::Svelte(_), DirectExecutionInputs::Vue { .. }) => {
                Err(DirectCompileError::FrameworkMismatch {
                    expected: "Svelte",
                    actual: "Vue",
                })
            }
        }
    }

    fn compile_vue(
        &self,
        source: &str,
        request: &CompileRequest,
        execution_inputs: &VueExecutionInputs,
        macro_semantics: &VueMacroSemanticInput,
    ) -> Result<DirectCompileOutput, DirectCompileError> {
        let allocator = Allocator::new();
        let (parsed, mut result) = crate::compile::compile_with_parsed(
            source,
            request,
            execution_inputs,
            macro_semantics,
            &allocator,
        )
        .map_err(DirectCompileError::Vue)?;

        let plan = ProductPlan::from_request(request);

        // Taken out BEFORE the framework-neutral runtime-bundle conversion
        // below (which consumes `result`) — `RuntimeCompileOutput` carries
        // no `.tsc` slot, its own `.tsx` is redundant with the one already
        // sitting here, and its diagnostics are re-derived from
        // `result.errors` rather than read back out of the bundle.
        let tsx = result.tsx.take();
        let tsc = result.tsc.take();
        let diagnostics = std::mem::take(&mut result.errors);

        let mut contributions: Vec<ArtifactContribution<'_>> = Vec::new();
        let mut styles: Vec<RuntimeStyleBlock> = Vec::new();

        if plan.wants(ProductKind::IdeCompanion) {
            let tsx = tsx.ok_or(DirectCompileError::UnsupportedProduct(
                ProductKind::IdeCompanion,
            ))?;
            contributions.push(ArtifactContribution {
                kind: ProductKind::IdeCompanion,
                fragments: Vec::new(),
                code: tsx.code,
                emitted_imports: Vec::new(),
                dialect: if tsx.is_jsx {
                    FragmentDialect::Jsx
                } else {
                    FragmentDialect::Tsx
                },
                // An IDE companion's projection map is NEVER optional
                // (`PlannedArtifact::requires_source_projection_map` is
                // always `true` for `IdeCompanion`) — always `Some`, even
                // when the map string happens to be empty.
                source_projection_map: Some(tsx.source_map),
                runtime_source_map: None,
            });
        }
        if plan.wants(ProductKind::Declarations) {
            let tsc = tsc.ok_or(DirectCompileError::UnsupportedProduct(
                ProductKind::Declarations,
            ))?;
            contributions.push(ArtifactContribution {
                kind: ProductKind::Declarations,
                fragments: Vec::new(),
                code: tsc.code,
                emitted_imports: Vec::new(),
                dialect: FragmentDialect::Declaration,
                // Neither mapping product is planned for `Declarations`
                // (`plan::ProductPlan::from_request`) — never attach one
                // unrequested.
                source_projection_map: None,
                runtime_source_map: None,
            });
        }

        let wants_client = plan.wants(ProductKind::RuntimeClient);
        let wants_server = plan.wants(ProductKind::RuntimeServer);
        let mut runtime_composed: Vec<(ComposedFragments, ProductKind, FragmentDialect, bool)> =
            Vec::new();

        if wants_client || wants_server {
            // `compile_with_parsed`'s own `ssr` derivation
            // (`derive_legacy_vue_options`) is `ANY RuntimeServer present`,
            // so the compile already performed above already matches
            // whichever kind this picks as PRIMARY — never re-derived
            // independently of that assumption.
            let primary_kind = if wants_server {
                ProductKind::RuntimeServer
            } else {
                ProductKind::RuntimeClient
            };
            let vue_request = request.vue().expect("dispatch already matched Vue");
            let dialect = direct_vue_dialect(&parsed, request.force_js());
            let want_maps = runtime_source_map_wanted(request, primary_kind);

            let bundle = crate::framework_common::vue_bridge::vue_result_to_runtime_bundle(
                source, &parsed, result,
            );
            // Style content is ssr-mode-independent — taken from this
            // (primary) bundle only, never duplicated from a secondary
            // compile below.
            styles.extend(bundle.styles.iter().cloned());

            let composed = compose_vue_runtime(
                source,
                vue_request,
                request.filename(),
                dialect,
                primary_kind,
                &bundle,
                want_maps,
            )?;
            runtime_composed.push((composed, primary_kind, dialect, want_maps));

            // Both `RuntimeClient` and `RuntimeServer` were planned
            // together — independent, co-requestable products
            // (`compile_request/mod.rs`'s own doc). `compile_inner`
            // resolves exactly ONE `ssr` mode per call
            // (`derive_legacy_vue_options`'s `ssr = ANY RuntimeServer
            // present`), so the SECOND kind needs its OWN compile, driven
            // by a narrowed single-product sub-request that forces the
            // opposite `ssr` derivation.
            if wants_client && wants_server {
                let secondary_kind = ProductKind::RuntimeClient;
                let secondary_request = single_runtime_product_request(request, secondary_kind)?;
                let secondary_allocator = Allocator::new();
                let (secondary_parsed, secondary_result) = crate::compile::compile_with_parsed(
                    source,
                    &secondary_request,
                    execution_inputs,
                    macro_semantics,
                    &secondary_allocator,
                )
                .map_err(DirectCompileError::Vue)?;
                let secondary_dialect =
                    direct_vue_dialect(&secondary_parsed, secondary_request.force_js());
                let secondary_want_maps =
                    runtime_source_map_wanted(&secondary_request, secondary_kind);
                let secondary_vue_request =
                    secondary_request.vue().expect("secondary request is Vue");
                let secondary_bundle =
                    crate::framework_common::vue_bridge::vue_result_to_runtime_bundle(
                        source,
                        &secondary_parsed,
                        secondary_result,
                    );
                let secondary_composed = compose_vue_runtime(
                    source,
                    secondary_vue_request,
                    secondary_request.filename(),
                    secondary_dialect,
                    secondary_kind,
                    &secondary_bundle,
                    secondary_want_maps,
                )?;
                runtime_composed.push((
                    secondary_composed,
                    secondary_kind,
                    secondary_dialect,
                    secondary_want_maps,
                ));
            }
        }
        for (composed, kind, dialect, want_maps) in &runtime_composed {
            let fragment_refs: Vec<_> = composed.fragments.iter().collect();
            contributions.push(ArtifactContribution {
                kind: *kind,
                fragments: fragment_refs,
                code: composed.code.clone(),
                emitted_imports: composed.emitted_imports.clone(),
                dialect: *dialect,
                source_projection_map: None,
                runtime_source_map: want_maps.then(|| composed.source_map.clone()),
            });
        }

        for planned in plan.artifacts() {
            if !contributions.iter().any(|c| c.kind == planned.kind) {
                return Err(DirectCompileError::UnsupportedProduct(planned.kind));
            }
        }

        let artifacts = publish(&plan, contributions)?;
        Ok(DirectCompileOutput {
            artifacts,
            styles,
            diagnostics,
        })
    }

    fn compile_svelte(
        &self,
        source: &str,
        request: &CompileRequest,
        execution_inputs: &SvelteExecutionInputs,
    ) -> Result<DirectCompileOutput, DirectCompileError> {
        let plan = ProductPlan::from_request(request);
        for planned in plan.artifacts() {
            if !matches!(
                planned.kind,
                ProductKind::RuntimeClient | ProductKind::RuntimeServer
            ) {
                return Err(DirectCompileError::UnsupportedProduct(planned.kind));
            }
        }

        let svelte_request = request.svelte().expect("dispatch already matched Svelte");
        let opts = direct_svelte_runtime_options(request, svelte_request, execution_inputs)?;

        let allocator = Allocator::default();
        let parsed = crate::svelte::parse_svelte(source);

        struct PendingRuntime {
            kind: ProductKind,
            code: String,
            emitted_imports: Vec<DeclaredImport>,
            runtime_source_map: Option<String>,
        }

        let mut validated_fragments = Vec::new();
        let mut pending: Vec<PendingRuntime> = Vec::new();
        let mut styles: Vec<RuntimeStyleBlock> = Vec::new();

        // Server checked first: when both kinds are planned together and
        // SSR is requested, `compile_client(ssr: true)` fails closed
        // immediately (the server backend has not landed) — failing fast
        // avoids compiling the client half for nothing.
        for kind in [ProductKind::RuntimeServer, ProductKind::RuntimeClient] {
            if !plan.wants(kind) {
                continue;
            }
            let ssr = kind == ProductKind::RuntimeServer;
            let want_maps = runtime_source_map_wanted(request, kind);
            // SSR always fails closed here (`compile_client`'s own `ssr`
            // gate) — this route never reinterprets that refusal.
            let module = compile_client(source, &parsed, &opts, &allocator, ssr, want_maps)
                .map_err(DirectCompileError::Svelte)?;

            // The EXTERNAL scoped-css artifact — the Svelte analogue of
            // Vue's own `<style>` blocks — mirrors the production host
            // route's identical conversion
            // (`svelte::carrier::VueCarrierCompiler::compile_bundle`'s
            // `RuntimeStyleBlock` population). Style content does not vary
            // between client/server compiles of the SAME source, so it is
            // taken from whichever kind's compile produces it first.
            if styles.is_empty() {
                if let Some(css) = &module.css {
                    let (space, artifact) = RuntimeOutputDescriptor::carrier_source(source);
                    let output_descriptor = RuntimeOutputDescriptor::generated(
                        &css.code,
                        css.source_map.as_deref(),
                        &[(space.as_str(), artifact.as_str())],
                        crate::framework_common::SourceMapFidelity::Approximate,
                    );
                    styles.push(RuntimeStyleBlock {
                        code: css.code.clone(),
                        source_map: css.source_map.clone(),
                        lang: None,
                        scope_hash: Some(css.hash.clone()),
                        has_global: css.has_global,
                        output_descriptor,
                    });
                }
            }

            let dialect = FragmentDialect::JavaScript;
            let fragment = Fragment {
                domain: FrameworkDomain::Svelte,
                product: kind,
                source_unit: SourceUnitId::from_canonical(&DirectSvelteFragmentTag {
                    canonical_id: request.filename().unwrap_or(""),
                    role: svelte_fragment_role(kind),
                }),
                source_space: SourceSpaceKind::GeneratedFragment,
                placement: PlacementSlot::ModuleBody,
                contract: SyntacticContract::CompleteModule,
                dialect,
                code: module.code.clone(),
                source_map: module.source_map.clone(),
                imports: module.declared_imports.clone(),
                exports: Vec::new(),
                helpers: Vec::new(),
                dependencies: Vec::new(),
            };
            let validated = fragment
                .validate()
                .map_err(DirectCompileError::SvelteFragment)?;
            validated_fragments.push(validated);
            pending.push(PendingRuntime {
                kind,
                code: module.code,
                emitted_imports: module.declared_imports,
                runtime_source_map: module.source_map,
            });
        }

        let mut contributions = Vec::new();
        for (validated, p) in validated_fragments.iter().zip(pending) {
            contributions.push(ArtifactContribution {
                kind: p.kind,
                fragments: vec![validated],
                code: p.code,
                emitted_imports: p.emitted_imports,
                dialect: FragmentDialect::JavaScript,
                source_projection_map: None,
                runtime_source_map: p.runtime_source_map,
            });
        }
        let artifacts = publish(&plan, contributions)?;
        Ok(DirectCompileOutput {
            artifacts,
            styles,
            diagnostics: Vec::new(),
        })
    }
}

/// This kind's own `RuntimeProductRequest.runtime_source_map` flag, read
/// directly off `request.products()` rather than through
/// [`CompileRequest::wants_runtime_source_map`] — that accessor reads
/// whichever runtime product it finds FIRST, which is ambiguous the moment
/// a request plans BOTH `RuntimeClient` and `RuntimeServer` together with
/// DIFFERENT map demands.
fn runtime_source_map_wanted(request: &CompileRequest, kind: ProductKind) -> bool {
    request
        .products()
        .iter()
        .find_map(|p| match (p, kind) {
            (CompileProduct::RuntimeClient(r), ProductKind::RuntimeClient) => {
                Some(r.runtime_source_map)
            }
            (CompileProduct::RuntimeServer(r), ProductKind::RuntimeServer) => {
                Some(r.runtime_source_map)
            }
            _ => None,
        })
        .unwrap_or(false)
}

/// A narrowed [`CompileRequest`] planning ONLY `kind`'s own product,
/// carrying over its exact [`crate::compile_request::RuntimeProductRequest`]
/// from `request`, plus every other framework-neutral field unchanged. Used
/// to force `compile_with_parsed`'s single-`ssr`-mode-per-call derivation
/// (`derive_legacy_vue_options`) onto the specific kind a caller needs when
/// `request` itself planned both runtime kinds together.
fn single_runtime_product_request(
    request: &CompileRequest,
    kind: ProductKind,
) -> Result<CompileRequest, DirectCompileError> {
    let product = request
        .products()
        .iter()
        .find(|p| p.kind() == kind)
        .cloned()
        .expect("caller only asks for a product kind present in the original request");
    CompileRequest::new(
        vec![product],
        request.framework().clone(),
        request.semantic_profile().cloned(),
        request.filename().map(str::to_string),
        request.component_id().map(str::to_string),
        request.is_production(),
        request.force_js(),
    )
    .map_err(DirectCompileError::Vue)
}

/// Compose one Vue runtime artifact (`RuntimeClient` or `RuntimeServer`)
/// from an already-produced [`RuntimeCompileOutput`] through the SAME
/// shared [`compose_fragments`] machinery `verter_session`'s host composer
/// uses — no host decoration (empty prelude/trailer extras).
fn compose_vue_runtime(
    source: &str,
    vue_request: &crate::compile_request::VueCompileRequest,
    filename: Option<&str>,
    dialect: FragmentDialect,
    planned_kind: ProductKind,
    bundle: &RuntimeCompileOutput,
    want_maps: bool,
) -> Result<ComposedFragments, DirectCompileError> {
    let runtime = if planned_kind == ProductKind::RuntimeServer {
        vue_request
            .ssr_runtime_module_name
            .as_deref()
            .or(vue_request.runtime_module_name.as_deref())
            .unwrap_or("vue")
    } else {
        vue_request.runtime_module_name.as_deref().unwrap_or("vue")
    };

    // Decoded under the TRUSTED same-crate regime
    // (`oxc_sourcemap::SourceMap::from_json_string`) — this map was
    // produced by THIS SAME compile a moment ago, not received from a
    // host/cross-tool source, so the hardened multi-fragment validator
    // `verter_session` needs for host-authored input has no work to do
    // here.
    let script_map = bundle
        .script
        .as_ref()
        .map(|s| &s.source_map)
        .filter(|map| !map.is_empty())
        .map(|map| oxc_sourcemap::SourceMap::from_json_string(map))
        .transpose()
        .map_err(|_| {
            DirectCompileError::VueComposition(VueMainCompositionFailure::Composition(
                crate::assembly::ComposeRefusal::UncomposableMap,
            ))
        })?;
    let template_map_json = bundle
        .template
        .as_ref()
        .map(|t| t.source_map.clone())
        .filter(|map| !map.is_empty());

    let compose_request = VueMainModuleRequest {
        canonical_id: filename.unwrap_or(""),
        compiled: bundle,
        dialect,
        planned_kind,
        runtime,
        want_maps,
        source_root: None,
        script_map: script_map.as_ref(),
        template_map_json,
        prelude_extra: Vec::new(),
        trailer_extra: Vec::new(),
    };
    let _ = source;
    compose_fragments(compose_request).map_err(DirectCompileError::VueComposition)
}

/// The dialect a direct Vue compile's runtime-module fragments/final
/// artifact are validated/parsed under — the SFC's own authored script
/// dialect ([`sfc_script_dialect`]), collapsed to its JS-only sibling under
/// `force_js`. Mirrors `verter_session`'s `resolve_main_dialect` (which
/// reads the same classification pre-computed onto its host-owned
/// `FileMeta.script_lang`); this route reads it directly off the parse this
/// same compile just produced instead.
fn direct_vue_dialect(parsed: &ParsedSfc, force_js: bool) -> FragmentDialect {
    let dialect = sfc_script_dialect(parsed.script_setup(), parsed.script());
    if force_js {
        if dialect.is_jsx() {
            FragmentDialect::Jsx
        } else {
            FragmentDialect::JavaScript
        }
    } else {
        match dialect {
            SfcScriptDialect::JavaScript => FragmentDialect::JavaScript,
            SfcScriptDialect::Jsx => FragmentDialect::Jsx,
            SfcScriptDialect::TypeScript => FragmentDialect::TypeScript,
            SfcScriptDialect::Tsx => FragmentDialect::Tsx,
        }
    }
}

/// Resolve [`SvelteRuntimeOptions`] from the canonical
/// [`crate::compile_request::SvelteCompileRequest`] plus this route's own
/// [`SvelteExecutionInputs`] — the SAME resolution
/// `crate::svelte::carrier`'s host-backed `compile_bundle` performs from its
/// own (legacy, string-typed) `RuntimeCompileOptions` bridge, entered here
/// directly from the canonical request's typed enums instead. Never a
/// second option authority: every field not representable on the canonical
/// request (`accessors`/`immutable`/`hmr`/`compatibility_component_api`) has
/// no canonical-request slot at all — structurally always `None`, exactly as
/// the host bridge already sets them.
///
/// `custom_element_descriptor` is NOT consumed here — a verified,
/// PRE-EXISTING gap this route matches byte-for-byte rather than silently
/// diverging from: the host route (`svelte/carrier.rs`) never reads it
/// either (confirmed by inspection — zero references), and the runtime
/// lowering's own `resolve_custom_element`
/// (`svelte/runtime/custom_element.rs`) takes only a bare
/// `custom_element_option: bool`, never a descriptor, when no inline
/// `<svelte:options customElement>` exists. See the B5 fix-round evidence
/// record for the full citation trail.
///
/// # Errors
///
/// [`DirectCompileError::UnsupportedSvelteNamespace`] when the request's
/// `namespace` is
/// [`crate::compile_request::svelte::SvelteNamespaceRequest::Foreign`] — the
/// compiler-internal [`SvelteNamespace`] this resolves into has no
/// representation for it, and neither this route nor the host route (which
/// never round-trips this specific enum) has ever defined what it should
/// mean, so it fails closed rather than silently defaulting to HTML.
fn direct_svelte_runtime_options(
    request: &CompileRequest,
    svelte_request: &crate::compile_request::SvelteCompileRequest,
    execution_inputs: &SvelteExecutionInputs,
) -> Result<SvelteRuntimeOptions, DirectCompileError> {
    use crate::compile_request::svelte::{
        SvelteFragmentsRequest, SvelteNamespaceRequest, SvelteRunesRequest,
    };

    let namespace = match svelte_request.namespace {
        None => None,
        Some(SvelteNamespaceRequest::Html) => Some(SvelteNamespace::Html),
        Some(SvelteNamespaceRequest::Svg) => Some(SvelteNamespace::Svg),
        Some(SvelteNamespaceRequest::MathMl) => Some(SvelteNamespace::Mathml),
        Some(SvelteNamespaceRequest::Foreign) => {
            return Err(DirectCompileError::UnsupportedSvelteNamespace)
        }
    };

    Ok(SvelteRuntimeOptions {
        filename: request.filename().map(str::to_string),
        name: None,
        runes: svelte_request.runes.and_then(|runes| match runes {
            SvelteRunesRequest::True => Some(true),
            SvelteRunesRequest::False => Some(false),
            SvelteRunesRequest::Infer => None,
        }),
        is_production: request.is_production(),
        dev_codegen: svelte_request.dev.unwrap_or(false),
        custom_element: svelte_request.custom_element.unwrap_or(false),
        css_hash_override: execution_inputs.css_hash_override.clone(),
        namespace,
        fragments: svelte_request.fragments.map(|fragments| match fragments {
            SvelteFragmentsRequest::Html => SvelteFragments::Html,
            SvelteFragmentsRequest::Tree => SvelteFragments::Tree,
        }),
        preserve_whitespace: svelte_request.preserve_whitespace,
        preserve_comments: svelte_request.preserve_comments,
        disclose_version: svelte_request.disclose_version,
        // Structurally unrepresentable on the canonical request — see this
        // function's own doc.
        accessors: None,
        immutable: None,
        hmr: None,
        compatibility_component_api: None,
    })
}

fn svelte_fragment_role(kind: ProductKind) -> &'static str {
    match kind {
        ProductKind::RuntimeServer => "server",
        _ => "client",
    }
}

/// Deterministic per-request, per-runtime-kind [`SourceUnitId`] for the
/// direct core's Svelte client-module fragment(s) — `role` keeps a
/// dual-`RuntimeClient`+`RuntimeServer` request's two fragments distinct
/// identities.
struct DirectSvelteFragmentTag<'a> {
    canonical_id: &'a str,
    role: &'static str,
}

impl CanonicalEncode for DirectSvelteFragmentTag<'_> {
    const DOMAIN_TAG: &'static str = "verter.compiler.standalone.svelte_client_fragment.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.canonical_id);
        e.field_str(2, self.role);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile_request::{
        AnalysisProductRequest, CompileProduct, DeclarationProductRequest, IdeProductRequest,
        RuntimeProductRequest, SvelteCompileRequest, VueCompileRequest,
    };

    const VUE_SOURCE: &str =
        "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>\n";
    const VUE_STYLED_SOURCE: &str = "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>\n<style>\n.foo { color: red; }\n</style>\n";
    const SVELTE_SOURCE: &str = "<script>\n  let count = $state(0);\n</script>\n<button onclick={() => count++}>{count}</button>\n";
    const SVELTE_STYLED_SOURCE: &str = "<script>\n  let count = $state(0);\n</script>\n<button onclick={() => count++}>{count}</button>\n<style>\n  button { color: red; }\n</style>\n";

    fn vue_request(products: Vec<CompileProduct>) -> CompileRequest {
        CompileRequest::new(
            products,
            FrameworkCompileRequest::Vue(VueCompileRequest::default()),
            None,
            Some("Comp.vue".to_string()),
            None,
            false,
            false,
        )
        .expect("test request constructs")
    }

    fn svelte_request(products: Vec<CompileProduct>) -> CompileRequest {
        CompileRequest::new(
            products,
            FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
            None,
            Some("Comp.svelte".to_string()),
            None,
            false,
            false,
        )
        .expect("test request constructs")
    }

    fn vue_inputs() -> DirectExecutionInputs<'static> {
        DirectExecutionInputs::Vue {
            execution: LEAKED_VUE_EXECUTION_INPUTS,
            macros: LEAKED_VUE_MACROS,
        }
    }

    // `VueExecutionInputs`/`VueMacroSemanticInput` are borrowed by
    // `DirectExecutionInputs<'a>`; leaking a `Default`/`Unavailable` instance
    // once keeps every test's call site a plain expression instead of
    // threading a local through each one.
    static LEAKED_VUE_EXECUTION_INPUTS: &VueExecutionInputs = &VueExecutionInputs {
        macro_runtime: None,
        prop_constness_overrides: None,
        style_v_bind_vars: Vec::new(),
        style_v_bind_usage_complete: None,
        template_binding_metadata: None,
        template_used_vars: None,
        runtime_template_hole: false,
        runtime_inline_template_chunk: false,
    };
    static LEAKED_VUE_MACROS: &VueMacroSemanticInput = &VueMacroSemanticInput::Unavailable;

    fn svelte_inputs() -> DirectExecutionInputs<'static> {
        DirectExecutionInputs::Svelte {
            execution: LEAKED_SVELTE_EXECUTION_INPUTS,
        }
    }

    static LEAKED_SVELTE_EXECUTION_INPUTS: &SvelteExecutionInputs = &SvelteExecutionInputs {
        css_hash_override: None,
    };

    #[test]
    fn vue_ide_companion_one_shot_publishes_exactly_that_artifact() {
        let request = vue_request(vec![CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map: true,
            ..Default::default()
        })]);
        let output = StandaloneCompiler
            .compile(VUE_SOURCE, &request, vue_inputs())
            .expect("a plain IdeCompanion compile must not be refused");
        let set = output.artifacts;
        assert_eq!(
            set.artifacts().len(),
            1,
            "must publish exactly one artifact"
        );
        let artifact = set
            .artifact(ProductKind::IdeCompanion)
            .expect("the requested IdeCompanion artifact must be present");
        assert!(set.artifact(ProductKind::RuntimeClient).is_none());
        assert!(set.artifact(ProductKind::Declarations).is_none());
        assert!(
            artifact.code().contains("msg"),
            "generated TSX must reflect the authored binding, got:\n{}",
            artifact.code()
        );
        assert!(
            artifact.source_projection_map().is_some(),
            "an IdeCompanion artifact's projection map is never optional"
        );
    }

    #[test]
    fn vue_runtime_client_one_shot_publishes_a_composed_module() {
        let request = vue_request(vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )]);
        let output = StandaloneCompiler
            .compile(VUE_SOURCE, &request, vue_inputs())
            .expect("a plain RuntimeClient compile must not be refused");
        let set = output.artifacts;
        assert_eq!(set.artifacts().len(), 1);
        let artifact = set
            .artifact(ProductKind::RuntimeClient)
            .expect("the requested RuntimeClient artifact must be present");
        let code = artifact.code();
        assert!(
            code.contains("export default _sfc_main"),
            "the composed module must terminate with the real assembly trailer, got:\n{code}"
        );
        assert!(
            !code.contains("__sfc__"),
            "the __sfc__ binding must have been renamed by the shared Vue composer, got:\n{code}"
        );
        assert!(
            code.contains("msg"),
            "the composed module must contain the real script content, got:\n{code}"
        );
        assert!(
            output.styles.is_empty(),
            "a style-less component must publish an EMPTY styles list, not a missing one"
        );
    }

    #[test]
    fn vue_runtime_client_with_requested_map_publishes_one() {
        let request = vue_request(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
            runtime_source_map: true,
            ..Default::default()
        })]);
        let output = StandaloneCompiler
            .compile(VUE_SOURCE, &request, vue_inputs())
            .expect("compile must not be refused");
        let artifact = output
            .artifacts
            .artifact(ProductKind::RuntimeClient)
            .unwrap();
        assert!(
            artifact.runtime_source_map().is_some(),
            "a requested runtime source map must be produced"
        );
    }

    #[test]
    fn vue_runtime_client_without_requested_map_publishes_none() {
        let request = vue_request(vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )]);
        let output = StandaloneCompiler
            .compile(VUE_SOURCE, &request, vue_inputs())
            .expect("compile must not be refused");
        let artifact = output
            .artifacts
            .artifact(ProductKind::RuntimeClient)
            .unwrap();
        assert!(
            artifact.runtime_source_map().is_none(),
            "an unrequested runtime map must be a true None"
        );
    }

    #[test]
    fn vue_multi_product_request_publishes_both_atomically() {
        let request = vue_request(vec![
            CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
            CompileProduct::IdeCompanion(IdeProductRequest::default()),
        ]);
        let output = StandaloneCompiler
            .compile(VUE_SOURCE, &request, vue_inputs())
            .expect("a multi-product compile must not be refused");
        let set = output.artifacts;
        assert_eq!(set.artifacts().len(), 2);
        assert!(set.artifact(ProductKind::RuntimeClient).is_some());
        assert!(set.artifact(ProductKind::IdeCompanion).is_some());
    }

    #[test]
    fn vue_dual_runtime_client_and_server_request_publishes_both_atomically() {
        // `RuntimeClient`/`RuntimeServer` are independent, co-requestable
        // products (`compile_request/mod.rs`'s own doc) — a request naming
        // BOTH must publish BOTH, not silently collapse to one.
        let request = vue_request(vec![
            CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
            CompileProduct::RuntimeServer(RuntimeProductRequest::default()),
        ]);
        let output = StandaloneCompiler
            .compile(VUE_SOURCE, &request, vue_inputs())
            .expect("a dual-runtime compile must not be refused");
        let set = output.artifacts;
        assert_eq!(set.artifacts().len(), 2, "both runtime kinds must publish");
        let client = set
            .artifact(ProductKind::RuntimeClient)
            .expect("the client artifact must be present");
        let server = set
            .artifact(ProductKind::RuntimeServer)
            .expect("the server artifact must be present");
        assert!(
            client.code().contains("_sfc_main.render = render"),
            "the client half must bind the CLIENT render function, got:\n{}",
            client.code()
        );
        assert!(
            server.code().contains("_sfc_main.ssrRender = ssrRender"),
            "the server half must bind the SSR render function, got:\n{}",
            server.code()
        );
        assert_ne!(
            client.code(),
            server.code(),
            "the two halves must be genuinely distinct compiles, not one artifact republished twice"
        );
    }

    #[test]
    fn vue_declarations_one_shot_publishes_exactly_that_artifact() {
        let request = vue_request(vec![CompileProduct::Declarations(
            DeclarationProductRequest::default(),
        )]);
        let output = StandaloneCompiler
            .compile(VUE_SOURCE, &request, vue_inputs())
            .expect("a plain Declarations compile must not be refused");
        let set = output.artifacts;
        assert_eq!(set.artifacts().len(), 1);
        assert!(set.artifact(ProductKind::Declarations).is_some());
    }

    #[test]
    fn vue_unsupported_product_is_refused_before_publish() {
        let request = vue_request(vec![CompileProduct::Analysis(AnalysisProductRequest {
            want_script_bindings: true,
            want_template_data: false,
        })]);
        let error = StandaloneCompiler
            .compile(VUE_SOURCE, &request, vue_inputs())
            .expect_err("this route does not yet produce an Analysis artifact");
        assert_eq!(
            error,
            DirectCompileError::UnsupportedProduct(ProductKind::Analysis)
        );
    }

    #[test]
    fn vue_styled_component_publishes_non_empty_styles() {
        let request = vue_request(vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )]);
        let output = StandaloneCompiler
            .compile(VUE_STYLED_SOURCE, &request, vue_inputs())
            .expect("a styled RuntimeClient compile must not be refused");
        assert_eq!(output.styles.len(), 1, "the one <style> block must publish");
        assert!(
            output.styles[0].code.contains("color: red"),
            "got:\n{}",
            output.styles[0].code
        );
    }

    #[test]
    fn svelte_request_with_vue_inputs_is_refused_not_a_panic() {
        let request = svelte_request(vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )]);
        let error = StandaloneCompiler
            .compile(SVELTE_SOURCE, &request, vue_inputs())
            .expect_err("a Svelte request must not reach the Vue-only driver");
        assert_eq!(
            error,
            DirectCompileError::FrameworkMismatch {
                expected: "Svelte",
                actual: "Vue",
            }
        );
    }

    #[test]
    fn vue_request_with_svelte_inputs_is_refused_not_a_panic() {
        let request = vue_request(vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )]);
        let error = StandaloneCompiler
            .compile(VUE_SOURCE, &request, svelte_inputs())
            .expect_err("a Vue request must not reach the Svelte-only driver");
        assert_eq!(
            error,
            DirectCompileError::FrameworkMismatch {
                expected: "Vue",
                actual: "Svelte",
            }
        );
    }

    #[test]
    fn svelte_runtime_client_one_shot_publishes_a_composed_module() {
        let request = svelte_request(vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )]);
        let output = StandaloneCompiler
            .compile(SVELTE_SOURCE, &request, svelte_inputs())
            .expect("a plain Svelte RuntimeClient compile must not be refused");
        let set = output.artifacts;
        assert_eq!(set.artifacts().len(), 1);
        let artifact = set
            .artifact(ProductKind::RuntimeClient)
            .expect("the requested RuntimeClient artifact must be present");
        let code = artifact.code();
        assert!(
            code.contains("svelte/internal/client"),
            "the composed module must import the real Svelte client runtime, got:\n{code}"
        );
        assert!(
            code.contains("count"),
            "the composed module must contain the real script content, got:\n{code}"
        );
        assert!(
            output.styles.is_empty(),
            "a style-less component must publish an EMPTY styles list, not a missing one"
        );
    }

    #[test]
    fn svelte_styled_component_publishes_non_empty_styles() {
        let request = svelte_request(vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )]);
        let output = StandaloneCompiler
            .compile(SVELTE_STYLED_SOURCE, &request, svelte_inputs())
            .expect("a styled Svelte RuntimeClient compile must not be refused");
        assert_eq!(output.styles.len(), 1, "the one <style> block must publish");
        assert!(
            output.styles[0].code.contains("color: red"),
            "got:\n{}",
            output.styles[0].code
        );
    }

    #[test]
    fn svelte_runtime_server_request_fails_closed_not_reinterpreted() {
        // SSR always fails closed at `compile_client` today (the server
        // backend has not landed) — this route must propagate that typed
        // refusal, never silently fall back to a client build.
        let request = svelte_request(vec![CompileProduct::RuntimeServer(
            RuntimeProductRequest::default(),
        )]);
        let error = StandaloneCompiler
            .compile(SVELTE_SOURCE, &request, svelte_inputs())
            .expect_err("Svelte SSR is not yet implemented and must fail closed");
        assert!(
            matches!(error, DirectCompileError::Svelte(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn svelte_dual_runtime_client_and_server_request_fails_closed_with_no_partial_output() {
        // Both kinds requested together: the server half fails closed
        // (SSR unsupported) — the WHOLE compile must refuse, never publish
        // just the client half.
        let request = svelte_request(vec![
            CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
            CompileProduct::RuntimeServer(RuntimeProductRequest::default()),
        ]);
        let error = StandaloneCompiler
            .compile(SVELTE_SOURCE, &request, svelte_inputs())
            .expect_err("the SSR half must refuse the whole compile");
        assert!(
            matches!(error, DirectCompileError::Svelte(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn svelte_unsupported_product_is_refused_before_publish() {
        let request = svelte_request(vec![CompileProduct::IdeCompanion(
            IdeProductRequest::default(),
        )]);
        let error = StandaloneCompiler
            .compile(SVELTE_SOURCE, &request, svelte_inputs())
            .expect_err("this route does not produce a Svelte IdeCompanion artifact");
        assert_eq!(
            error,
            DirectCompileError::UnsupportedProduct(ProductKind::IdeCompanion)
        );
    }

    #[test]
    fn svelte_foreign_namespace_is_refused_not_silently_defaulted() {
        let request = CompileRequest::new(
            vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )],
            FrameworkCompileRequest::Svelte(SvelteCompileRequest {
                namespace: Some(crate::compile_request::svelte::SvelteNamespaceRequest::Foreign),
                ..Default::default()
            }),
            None,
            Some("Comp.svelte".to_string()),
            None,
            false,
            false,
        )
        .expect("a Foreign namespace constructs fine at the canonical-request layer");
        let error = StandaloneCompiler
            .compile(SVELTE_SOURCE, &request, svelte_inputs())
            .expect_err("a Foreign namespace has no compiler-internal representation");
        assert_eq!(error, DirectCompileError::UnsupportedSvelteNamespace);
    }

    #[test]
    fn a_malformed_vue_request_refuses_with_no_partial_artifact() {
        // `SSR x Vapor` is refused at construction, before this route ever
        // runs — proves the request-construction refusal propagates rather
        // than being silently bypassed by the direct route.
        let request = CompileRequest::new(
            vec![CompileProduct::RuntimeServer(
                RuntimeProductRequest::default(),
            )],
            FrameworkCompileRequest::Vue(VueCompileRequest {
                backend: crate::compile_request::VueBackendRequest::Vapor,
                ..Default::default()
            }),
            None,
            None,
            None,
            false,
            false,
        );
        assert_eq!(
            request.unwrap_err(),
            CompileRequestError::SsrVaporBackendUnsupported
        );
    }
}
