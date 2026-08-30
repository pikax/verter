//! The internal compiler's sole raw-source direct-compile boundary — the
//! borrowed one-shot direct route: every caller supplies a canonical
//! [`crate::compile_request::CompileRequest`] (built through
//! [`CompileRequest::new`](CompileRequest::new), which enforces every
//! construction-time fail-closed rule) plus the framework-tagged
//! [`DirectExecutionInputs`] carrier for resolved facts excluded from
//! request identity, and gets back exactly one atomic
//! [`crate::assembly::ArtifactSet`] — the SAME publication boundary
//! every host-backed route publishes through, never a second one — plus a
//! [`DirectCompileOutput`] sibling for the two facts the sealed
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
use crate::assembly::source_unit::source_unit_id;
use crate::assembly::vue_module::{
    compose_fragments, ComposedFragments, VueMainCompositionFailure, VueMainModuleRequest,
};
use crate::assembly::{ArtifactSet, AssemblyRefusal};
use crate::compile::types::{
    CompileDiagnostic, CompileTarget, VueExecutionInputs, VueMacroSemanticInput,
};
use crate::compile::{
    compile_from_parsed, compile_from_parsed_legacy, derive_legacy_vue_options,
    parse_template_block, template_unit_used_vars,
};
use crate::compile_request::{
    CompileProduct, CompileRequest, CompileRequestError, FrameworkCompileRequest, ProductKind,
};
use crate::framework_common::{
    IdeOutput, RuntimeBlockContentInput, RuntimeBlockContentInputs, RuntimeCompileOutput,
    RuntimeOutputDescriptor, RuntimeStyleBlock,
};
use crate::parser::types::{sfc_script_dialect, ParsedSfc, SfcScriptDialect};
use crate::svelte::carrier::render_admitted_svelte_ide;
use crate::svelte::ide::SvelteIdeUnsupportedDiagnostic;
#[cfg(test)]
use crate::svelte::runtime::UnsupportedSvelteRuntimeSurface;
use crate::svelte::runtime::{
    compile_client, refuse_unproducible_runtime_surface, ClientCompileError, SvelteFragments,
    SvelteNamespace, SvelteRuntimeOptions,
};
use crate::svelte::ParsedSvelte;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

/// Ephemeral, non-identity execution inputs for a Svelte compile — resolved
/// framework facts threaded alongside a canonical
/// [`crate::compile_request::CompileRequest`] but EXCLUDED from its
/// identity, mirroring [`VueExecutionInputs`]'s role for Vue. NOT a second
/// option authority: `css_hash_override` is the same session/host-resolved
/// fact [`SvelteRuntimeOptions::css_hash_override`] already carries — the
/// official user `cssHash` callback's already-computed result, preserved
/// byte-exact. Genuine Svelte semantic options (`runes`, `namespace`,
/// `fragments`, …) live on [`crate::compile_request::SvelteCompileRequest`]
/// by the request layer and are never duplicated here.
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
///
/// `Clone`/`Copy`: every field is a borrowed reference, so duplicating a
/// value is free — [`StandaloneCompiler::compile_batch`] needs to read one
/// item's `inputs` out of a borrowed `&[BatchCompileItem]` slice entry.
#[derive(Clone, Copy)]
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
/// the sealed publication model carries no slot for.
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
    /// all) has ever had to answer
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
    /// [`StandaloneCompiler::compile_prepared`] was called with a
    /// [`PreparedCarrier`] whose recorded digest(s) no longer match the
    /// caller's `source`/`request` — never silently reused against stale
    /// input.
    StalePreparedInput { reason: StalePreparedReason },
}

/// Why [`DirectCompileError::StalePreparedInput`] was raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StalePreparedReason {
    /// The caller's `source` no longer matches the digest
    /// [`StandaloneCompiler::prepare`] recorded.
    SourceChanged,
    /// The Vue parse-affecting options (`delimiters`, `is_custom_element`)
    /// recorded at prepare time no longer match `request.vue()`. Svelte
    /// never raises this variant — [`crate::svelte::parse_svelte`] takes no
    /// parse-affecting options.
    ParseOptionsChanged,
}

impl From<AssemblyRefusal> for DirectCompileError {
    fn from(failure: AssemblyRefusal) -> Self {
        Self::Publish(failure)
    }
}

/// Zero-sized marker making a prepared carrier a single-owner value: it
/// derives neither `Clone` nor `Copy`, so embedding it as a field makes
/// `#[derive(Clone)]` on the embedding type a compile error.
///
/// The marker alone does not stop a HAND-WRITTEN `impl Clone`, which could
/// simply construct a fresh `SingleOwner`. The `assert_not_impl_any!` lines
/// below close that, and they are library code rather than `#[cfg(test)]`
/// so both halves hold in every build. Together they make "not `Clone`" a
/// structural fact rather than a doc-comment claim.
#[derive(Debug)]
struct SingleOwner;

/// The Vue half of [`PreparedCarrier`] — a parsed SFC plus the digests
/// [`StandaloneCompiler::compile_prepared`] revalidates on every reuse.
///
/// Not `Clone`: duplicating a retained parse is an explicit retention
/// decision ([`StandaloneCompiler::prepare`] / [`StandaloneCompiler::prepare_owned`]),
/// never a silent `clone()`. Enforced structurally by [`SingleOwner`].
#[derive(Debug)]
pub struct VuePreparedCarrier {
    parsed: ParsedSfc,
    source_digest: [u8; 32],
    parse_identity_digest: [u8; 32],
    /// Present only when this carrier was built by
    /// [`StandaloneCompiler::prepare_owned`] — the caller-chosen owned
    /// source, counted in [`PreparedCarrier::retained_weight`].
    owned_source: Option<String>,
    _single_owner: SingleOwner,
}

/// The Svelte half of [`PreparedCarrier`]. [`crate::svelte::parse_svelte`]
/// takes no parse-affecting options, so there is no second identity digest
/// to track — only `source_digest`.
///
/// Not `Clone`: see [`VuePreparedCarrier`].
#[derive(Debug)]
pub struct SveltePreparedCarrier {
    parsed: ParsedSvelte,
    source_digest: [u8; 32],
    owned_source: Option<String>,
    _single_owner: SingleOwner,
}

/// A single already-parsed source, produced by [`StandaloneCompiler::prepare`]
/// and replayable through any number of [`StandaloneCompiler::compile_prepared`]
/// calls — [`StandaloneCompiler::compile_batch`] uses this internally to
/// share one parse across items with an identical `(framework, source,
/// Vue parse-options)` group key. Framework-tagged so a mismatched
/// [`CompileRequest`]/[`DirectExecutionInputs`] pairing is caught the same
/// way [`StandaloneCompiler::compile`] already catches one — a typed
/// [`DirectCompileError::FrameworkMismatch`], never a panic. Carries no
/// product/request state of its own — no product selection, compatibility
/// flag, helper choice, diagnostic policy, or mapping/publication
/// preference. It DOES retain `source_digest` and (Vue only)
/// `parse_identity_digest` — blake3 digests over the parse-affecting
/// subset of the request ([`vue_parse_identity_digest`]'s own two fields)
/// used ONLY for stale-input revalidation at [`StandaloneCompiler::compile_prepared`]
/// time, never read for any semantic/product decision: prepared state may
/// not change request defaults, compatibility, products, helpers,
/// diagnostics, mappings, or publication meaning.
///
/// Not `Clone`: the retained parse is a single-owner value. Borrowed-source
/// preparation is [`StandaloneCompiler::prepare`]; owned-source preparation
/// is [`StandaloneCompiler::prepare_owned`]. Inspect retained bytes via
/// [`Self::retained_weight`]; dropping the value releases them.
#[derive(Debug)]
pub enum PreparedCarrier {
    Vue(VuePreparedCarrier),
    Svelte(SveltePreparedCarrier),
}

// Single-owner reuse is a type-system fact, not prose: the embedded
// `SingleOwner` marker already makes `#[derive(Clone)]` on either variant
// type a compile error; these asserts also close a hand-written `impl
// Clone` (which does not need its fields to be `Clone`) on any of the
// three carrier types, and run in LIBRARY code (not `#[cfg(test)]`) so a
// manual impl fails `cargo check -p verter_compiler`, not only `cargo
// test`.
static_assertions::assert_not_impl_any!(VuePreparedCarrier: Clone, Copy);
static_assertions::assert_not_impl_any!(SveltePreparedCarrier: Clone, Copy);
static_assertions::assert_not_impl_any!(PreparedCarrier: Clone, Copy);

impl PreparedCarrier {
    /// Bytes this carrier retains independently of any borrowed `&str` the
    /// caller still holds: parsed inventory + revalidation digests +, when
    /// built by [`StandaloneCompiler::prepare_owned`], the owned source.
    /// Not a process-RSS measurement — that cell belongs to the separate
    /// route-overhead lock, not this observability surface.
    pub fn retained_weight(&self) -> usize {
        match self {
            Self::Vue(carrier) => vue_parsed_retained_bytes(&carrier.parsed)
                .saturating_add(64)
                .saturating_add(
                    carrier
                        .owned_source
                        .as_ref()
                        .map(String::capacity)
                        .unwrap_or(0),
                ),
            Self::Svelte(carrier) => svelte_parsed_retained_bytes(&carrier.parsed)
                .saturating_add(32)
                .saturating_add(
                    carrier
                        .owned_source
                        .as_ref()
                        .map(String::capacity)
                        .unwrap_or(0),
                ),
        }
    }

    /// The owned source [`StandaloneCompiler::prepare_owned`] retained, if
    /// any. Borrowed [`StandaloneCompiler::prepare`] never stores source.
    pub fn retained_source(&self) -> Option<&str> {
        match self {
            Self::Vue(carrier) => carrier.owned_source.as_deref(),
            Self::Svelte(carrier) => carrier.owned_source.as_deref(),
        }
    }
}

/// One item of a [`StandaloneCompiler::compile_batch`] call — the exact
/// borrowed triple [`StandaloneCompiler::compile`] takes, batched.
pub struct BatchCompileItem<'a> {
    pub source: &'a str,
    pub request: &'a CompileRequest,
    pub inputs: DirectExecutionInputs<'a>,
}

/// Refuses a request whose framework and whose [`DirectExecutionInputs`]
/// variant disagree, before the item is grouped or its carrier prepared.
///
/// Only [`StandaloneCompiler::compile_batch`] calls this;
/// [`StandaloneCompiler::compile`] reaches the same conclusion from the match
/// it already performs on the pair. The product/capability preflight is a
/// different function, [`refuse_unproducible_plan`].
fn refuse_inputs_mismatch(
    request: &CompileRequest,
    inputs: DirectExecutionInputs<'_>,
) -> Result<(), DirectCompileError> {
    match (request.framework(), inputs) {
        (FrameworkCompileRequest::Vue(_), DirectExecutionInputs::Vue { .. })
        | (FrameworkCompileRequest::Svelte(_), DirectExecutionInputs::Svelte { .. }) => Ok(()),
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

/// The Vue product kinds the direct core can produce. This list is the
/// DECLARATION; the completeness check at the end of `compile_vue_from_parsed`
/// is the ENFORCEMENT — a kind named here with no branch that contributes an
/// artifact still fails there, so the list cannot silently over-promise.
const VUE_PRODUCIBLE_KINDS: &[ProductKind] = &[
    ProductKind::RuntimeClient,
    ProductKind::RuntimeServer,
    ProductKind::IdeCompanion,
    ProductKind::Declarations,
];

/// The Svelte runtime kinds the direct core admits, DERIVED from the runtime's
/// own capability answer rather than restated beside it. When the runtime
/// gains a server backend, `refuse_unproducible_runtime_surface` starts
/// returning `Ok` and this list, the plan preflight, and the compile loop all
/// follow from that one change — the loop below iterates this same function.
fn svelte_producible_kinds() -> Vec<ProductKind> {
    [ProductKind::RuntimeClient, ProductKind::RuntimeServer]
        .into_iter()
        .filter(|kind| {
            refuse_unproducible_runtime_surface(*kind == ProductKind::RuntimeServer).is_ok()
        })
        .collect()
}

/// Refuse, before any parse, a plan the direct core cannot produce.
///
/// This preflight owns WHEN the question is asked, never the answer: the
/// answer comes from the per-framework capability sources above, and an
/// unproducible Svelte server surface carries the runtime's OWN typed
/// refusal rather than a generic one, so the early refusal is the same value
/// a late one would have been.
fn refuse_unproducible_plan(request: &CompileRequest) -> Result<(), DirectCompileError> {
    match request.framework() {
        FrameworkCompileRequest::Vue(_) => {
            for product in request.products() {
                if !VUE_PRODUCIBLE_KINDS.contains(&product.kind()) {
                    return Err(DirectCompileError::UnsupportedProduct(product.kind()));
                }
            }
        }
        FrameworkCompileRequest::Svelte(_) => {
            let producible = svelte_producible_kinds();
            for product in request.products() {
                if producible.contains(&product.kind()) {
                    continue;
                }
                // A kind the RUNTIME refuses reports the runtime's own typed
                // error; anything else was never a Svelte runtime kind at all.
                if product.kind() == ProductKind::RuntimeServer {
                    refuse_unproducible_runtime_surface(true)
                        .map_err(DirectCompileError::Svelte)?;
                }
                return Err(DirectCompileError::UnsupportedProduct(product.kind()));
            }
            if let Some(svelte) = request.svelte() {
                // Same single-owner rule for namespace support: the mapping
                // that `direct_svelte_runtime_options` uses to build the
                // runtime options is the mapping consulted here, so refusing
                // early cannot drift from refusing late.
                resolve_svelte_namespace(svelte.namespace)?;
            }
        }
    }
    Ok(())
}

/// The single mapping from a requested Svelte namespace to the runtime
/// namespace, and so the single place `Foreign` is refused. The plan
/// preflight consults it to refuse before parsing;
/// [`direct_svelte_runtime_options`] consults it to build the options.
fn resolve_svelte_namespace(
    requested: Option<crate::compile_request::svelte::SvelteNamespaceRequest>,
) -> Result<Option<SvelteNamespace>, DirectCompileError> {
    use crate::compile_request::svelte::SvelteNamespaceRequest;
    match requested {
        None => Ok(None),
        Some(SvelteNamespaceRequest::Html) => Ok(Some(SvelteNamespace::Html)),
        Some(SvelteNamespaceRequest::Svg) => Ok(Some(SvelteNamespace::Svg)),
        Some(SvelteNamespaceRequest::MathMl) => Ok(Some(SvelteNamespace::Mathml)),
        Some(SvelteNamespaceRequest::Foreign) => {
            Err(DirectCompileError::UnsupportedSvelteNamespace)
        }
    }
}

fn vue_parsed_retained_bytes(parsed: &ParsedSfc) -> usize {
    parsed.retained_bytes()
}

fn svelte_parsed_retained_bytes(parsed: &ParsedSvelte) -> usize {
    parsed.retained_bytes()
}

/// [`StandaloneCompiler::compile_batch`]'s own accounting: `cold_build_count`
/// is the number of [`StandaloneCompiler::prepare`] calls the batch actually
/// performed (one per distinct group); `reuse_count` is the number of
/// [`StandaloneCompiler::compile_prepared`] calls it performed. Items
/// refused by the product/capability preflight never prepare and never
/// compile, so they increment neither count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompileBatchReport {
    pub cold_build_count: usize,
    pub reuse_count: usize,
}

/// [`StandaloneCompiler::compile_batch`]'s result: one outcome per input
/// item, in input order — a batch never reorders, and one item's `Err`
/// never affects any other item's entry.
pub struct BatchCompileOutput {
    pub results: Vec<Result<DirectCompileOutput, DirectCompileError>>,
    pub report: CompileBatchReport,
}

/// Parsed Vue lowering: carrier compile result plus the selected-template
/// diagnostic batch. Selected diagnostics stay a separate batch and are
/// never merged into `result.errors`.
pub(crate) struct VueParsedLowering {
    pub result: crate::compile::VerterCompileResult,
    pub selected_diagnostics: Vec<CompileDiagnostic>,
}

/// Parsed Svelte IDE lowering: companion bytes plus the projector diagnostics
/// retained from the admitted parse. Diagnostics stay on this lowering and
/// are never rewritten onto [`CompileDiagnostic`].
pub(crate) struct SvelteParsedLowering {
    pub ide: IdeOutput,
    pub diagnostics: Vec<SvelteIdeUnsupportedDiagnostic>,
}

/// Selected-template IDE lowering: parse the extra template space, compile
/// the carrier shell with a generated hole, compile the selected chunk with
/// transferred script bindings, and return both on one TSX result.
///
/// Script bindings and chunk boundaries are internal prerequisites of
/// IDE + selected template. They are never added as request products.
fn lower_selected_template_ide(
    source: &str,
    parsed: &ParsedSfc,
    request: &CompileRequest,
    execution_inputs: &VueExecutionInputs,
    macro_semantics: &VueMacroSemanticInput,
    selected: &RuntimeBlockContentInput,
) -> Result<VueParsedLowering, DirectCompileError> {
    let vue = request.vue().ok_or(DirectCompileError::FrameworkMismatch {
        expected: "Vue",
        actual: "Svelte",
    })?;
    let delimiters = vue
        .delimiters
        .as_ref()
        .map(|(open, close)| (open.as_str(), close.as_str()));
    let custom_elements = if vue.is_custom_element.is_empty() {
        None
    } else {
        Some(vue.is_custom_element.as_slice())
    };
    let parsed_template = parse_template_block(&selected.code, delimiters, custom_elements);
    let allocator = Allocator::new();
    let used_vars = template_unit_used_vars(
        &selected.code,
        &parsed_template,
        vue.delimiters.clone(),
        if vue.is_custom_element.is_empty() {
            None
        } else {
            Some(vue.is_custom_element.clone())
        },
        &allocator,
    );

    let mut carrier_view = parsed.clone();
    carrier_view.template_ast = None;
    let mut carrier_execution = execution_inputs.clone();
    carrier_execution.template_used_vars = Some(used_vars);

    let resolved_backend = request
        .resolve_vue_backend(parsed.is_vapor())
        .map_err(DirectCompileError::Vue)?;
    let (mut carrier_options, carrier_verter) =
        derive_legacy_vue_options(request, resolved_backend, &carrier_execution);
    carrier_options.ide_chunk_boundaries = true;
    carrier_options.target |= CompileTarget::SCRIPT;

    let mut carrier_result = compile_from_parsed_legacy(
        source,
        &carrier_view,
        &carrier_options,
        &carrier_verter,
        macro_semantics,
        &allocator,
    )
    .map_err(DirectCompileError::Vue)?;

    let selected_execution = VueExecutionInputs {
        prop_constness_overrides: carrier_result.template_binding_metadata.const_props.clone(),
        template_binding_metadata: Some(carrier_result.template_binding_metadata.clone()),
        ..VueExecutionInputs::default()
    };
    let (mut chunk_options, chunk_verter) =
        derive_legacy_vue_options(request, resolved_backend, &selected_execution);
    chunk_options.ide_chunk_boundaries = true;

    let mut selected_result = compile_from_parsed_legacy(
        &selected.code,
        &parsed_template,
        &chunk_options,
        &chunk_verter,
        &VueMacroSemanticInput::default(),
        &allocator,
    )
    .map_err(DirectCompileError::Vue)?;

    let Some(shell) = carrier_result.tsx.as_mut() else {
        return Err(DirectCompileError::UnsupportedProduct(
            ProductKind::IdeCompanion,
        ));
    };
    let Some(fragment) = selected_result.tsx.as_mut() else {
        return Err(DirectCompileError::UnsupportedProduct(
            ProductKind::IdeCompanion,
        ));
    };
    shell.generated_template_chunk = fragment.generated_template_chunk.take();
    Ok(VueParsedLowering {
        result: carrier_result,
        selected_diagnostics: selected_result.errors,
    })
}

/// Stateless compiler for callers that do not participate in a registered
/// host.
#[derive(Debug, Default, Clone, Copy)]
pub struct StandaloneCompiler;

impl StandaloneCompiler {
    /// Compile borrowed standalone source into exactly the
    /// [`DirectCompileOutput`] `request` plans — the sole `StandaloneCompiler`
    /// entry that parses AND compiles in one call ([`Self::prepare`] also
    /// takes raw source, but only parses; [`Self::compile_prepared`] takes it
    /// as well, hashing it to revalidate the carrier and handing it to
    /// codegen, but never re-parsing it); registered hosts consume their
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
        refuse_unproducible_plan(request)?;
        let vue = request.vue().expect("dispatch already matched Vue");
        let parsed = crate::compile::parse_sfc(
            source,
            vue.delimiters
                .as_ref()
                .map(|(o, c)| (o.as_str(), c.as_str())),
            Some(vue.is_custom_element.as_slice()),
        );
        self.compile_vue_from_parsed(source, &parsed, request, execution_inputs, macro_semantics)
    }

    /// Lower an already-parsed Vue SFC through the same codegen
    /// [`Self::compile_vue_from_parsed`] uses, without publishing artifacts.
    /// Callers that already hold a [`ParsedSfc`] must use this instead of
    /// re-parsing.
    ///
    /// `block_content` is execution input excluded from request identity.
    /// A selected template is an internal IDE prerequisite: script bindings
    /// and the generated template chunk are computed here and never added
    /// as request products.
    pub(crate) fn lower_vue_from_parsed(
        &self,
        source: &str,
        parsed: &ParsedSfc,
        request: &CompileRequest,
        execution_inputs: &VueExecutionInputs,
        macro_semantics: &VueMacroSemanticInput,
        block_content: &RuntimeBlockContentInputs,
    ) -> Result<VueParsedLowering, DirectCompileError> {
        refuse_unproducible_plan(request)?;
        match block_content.template.as_ref() {
            Some(selected) => lower_selected_template_ide(
                source,
                parsed,
                request,
                execution_inputs,
                macro_semantics,
                selected,
            ),
            None => {
                let allocator = Allocator::new();
                compile_from_parsed(
                    source,
                    parsed,
                    request,
                    execution_inputs,
                    macro_semantics,
                    &allocator,
                )
                .map(|result| VueParsedLowering {
                    result,
                    selected_diagnostics: Vec::new(),
                })
                .map_err(DirectCompileError::Vue)
            }
        }
    }

    /// Lower an already-parsed Svelte component through the IDE projector,
    /// without publishing artifacts. Callers that already hold a
    /// [`ParsedSvelte`] must use this instead of re-parsing.
    ///
    /// This is parsed-core IDE lowering, not `compile()` publication. It does
    /// not run the compile-route product/namespace preflight: that gate is
    /// runtime `compile()`'s, and IDE projection is gated by
    /// `require_ide_only`.
    pub(crate) fn lower_svelte_from_parsed(
        &self,
        source: &str,
        parsed: &ParsedSvelte,
        request: &CompileRequest,
    ) -> Result<SvelteParsedLowering, DirectCompileError> {
        let (ide, diagnostics) = render_admitted_svelte_ide(
            source,
            parsed,
            request.filename(),
            !request.wants_ide_source_map(),
        );
        Ok(SvelteParsedLowering { ide, diagnostics })
    }

    /// The parsed-input core [`Self::compile_vue`] delegates to once it has
    /// a [`ParsedSfc`] in hand — also [`Self::compile_prepared`]'s Vue
    /// dispatch target, so a direct, prepared-first, prepared-repeat, or
    /// batch compile of the same `(source, request, execution_inputs,
    /// macro_semantics)` runs the IDENTICAL codegen from this point on;
    /// only where/how often the parse itself happened differs between
    /// routes.
    fn compile_vue_from_parsed(
        &self,
        source: &str,
        parsed: &ParsedSfc,
        request: &CompileRequest,
        execution_inputs: &VueExecutionInputs,
        macro_semantics: &VueMacroSemanticInput,
    ) -> Result<DirectCompileOutput, DirectCompileError> {
        let mut result = self
            .lower_vue_from_parsed(
                source,
                parsed,
                request,
                execution_inputs,
                macro_semantics,
                &RuntimeBlockContentInputs::default(),
            )?
            .result;

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
            let dialect = direct_vue_dialect(parsed, request.force_js());
            let want_maps = runtime_source_map_wanted(request, primary_kind);

            let bundle = crate::framework_common::vue_bridge::vue_result_to_runtime_bundle(
                source, parsed, result,
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
                // Reuses the SAME `parsed` as the primary compile — proven
                // behavior-preserving (see this module's own doc): both
                // sub-requests parse the identical source under identical
                // options, and `compile_inner` never re-parses internally.
                let secondary_result = crate::compile::compile_from_parsed(
                    source,
                    parsed,
                    &secondary_request,
                    execution_inputs,
                    macro_semantics,
                    &secondary_allocator,
                )
                .map_err(DirectCompileError::Vue)?;
                let secondary_dialect = direct_vue_dialect(parsed, secondary_request.force_js());
                let secondary_want_maps =
                    runtime_source_map_wanted(&secondary_request, secondary_kind);
                let secondary_vue_request =
                    secondary_request.vue().expect("secondary request is Vue");
                let secondary_bundle =
                    crate::framework_common::vue_bridge::vue_result_to_runtime_bundle(
                        source,
                        parsed,
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
        refuse_unproducible_plan(request)?;
        let parsed = crate::svelte::parse_svelte(source);
        self.compile_svelte_from_parsed(source, &parsed, request, execution_inputs)
    }

    /// The parsed-input core [`Self::compile_svelte`] delegates to once it
    /// has a [`ParsedSvelte`] in hand — also [`Self::compile_prepared`]'s
    /// Svelte dispatch target. Direct and batch entry points refuse
    /// unproducible products/capabilities BEFORE this parse; an explicit
    /// [`Self::prepare`] call may still parse, because preparation was then
    /// the requested operation. This core still re-runs the same preflight
    /// so a `compile_prepared` of an unproducible request never compiles.
    fn compile_svelte_from_parsed(
        &self,
        source: &str,
        parsed: &ParsedSvelte,
        request: &CompileRequest,
        execution_inputs: &SvelteExecutionInputs,
    ) -> Result<DirectCompileOutput, DirectCompileError> {
        refuse_unproducible_plan(request)?;
        let plan = ProductPlan::from_request(request);

        let svelte_request = request.svelte().expect("dispatch already matched Svelte");
        let opts = direct_svelte_runtime_options(request, svelte_request, execution_inputs)?;

        let allocator = Allocator::default();

        struct PendingRuntime {
            kind: ProductKind,
            code: String,
            emitted_imports: Vec<DeclaredImport>,
            runtime_source_map: Option<String>,
        }

        let mut validated_fragments = Vec::new();
        let mut pending: Vec<PendingRuntime> = Vec::new();
        let mut styles: Vec<RuntimeStyleBlock> = Vec::new();

        // Iterate the SAME capability answer the plan preflight used, so a
        // kind this loop can build and a kind the preflight admits cannot
        // drift apart. A kind the runtime cannot produce never reaches here:
        // the preflight already returned the runtime's own typed refusal.
        for kind in svelte_producible_kinds() {
            if !plan.wants(kind) {
                continue;
            }
            let ssr = kind == ProductKind::RuntimeServer;
            let want_maps = runtime_source_map_wanted(request, kind);
            let module = compile_client(source, parsed, &opts, &allocator, ssr, want_maps)
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
                source_unit: source_unit_id(
                    request.filename().unwrap_or(""),
                    svelte_fragment_role(kind),
                ),
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

    /// Parse `source` once under `request`'s framework and (Vue-only)
    /// parse-affecting options, without compiling any product. The
    /// returned [`PreparedCarrier`] can be replayed through
    /// [`Self::compile_prepared`] any number of times —
    /// [`Self::compile_batch`] uses this internally to share one parse
    /// across items with an identical group key.
    ///
    /// Infallible: both [`crate::compile::parse_sfc`] and
    /// [`crate::svelte::parse_svelte`] are infallible parsers — a malformed
    /// source still parses, with its own diagnostics carried on the
    /// returned [`ParsedSfc`]/[`ParsedSvelte`]; refusal only happens later,
    /// at [`Self::compile_prepared`] time, exactly like the direct route.
    pub fn prepare(&self, source: &str, request: &CompileRequest) -> PreparedCarrier {
        match request.framework() {
            FrameworkCompileRequest::Vue(_) => {
                let vue = request.vue().expect("dispatch already matched Vue");
                let parsed = crate::compile::parse_sfc(
                    source,
                    vue.delimiters
                        .as_ref()
                        .map(|(o, c)| (o.as_str(), c.as_str())),
                    Some(vue.is_custom_element.as_slice()),
                );
                PreparedCarrier::Vue(VuePreparedCarrier {
                    parsed,
                    source_digest: source_digest(source),
                    parse_identity_digest: vue_parse_identity_digest(vue),
                    owned_source: None,
                    _single_owner: SingleOwner,
                })
            }
            FrameworkCompileRequest::Svelte(_) => {
                let parsed = crate::svelte::parse_svelte(source);
                PreparedCarrier::Svelte(SveltePreparedCarrier {
                    parsed,
                    source_digest: source_digest(source),
                    owned_source: None,
                    _single_owner: SingleOwner,
                })
            }
        }
    }

    /// Owned-source preparation: the caller has already taken ownership of
    /// the source bytes (FFI, or a caller that wants source lifetime to
    /// follow the carrier). Parses the same way [`Self::prepare`] does,
    /// then retains the `String` so [`PreparedCarrier::retained_weight`]
    /// includes it and [`PreparedCarrier::retained_source`] can hand it
    /// back. Does not copy a borrowed `&str` the way a `to_string()` inside
    /// [`Self::prepare`] would.
    pub fn prepare_owned(&self, source: String, request: &CompileRequest) -> PreparedCarrier {
        let mut prepared = self.prepare(&source, request);
        match &mut prepared {
            PreparedCarrier::Vue(carrier) => carrier.owned_source = Some(source),
            PreparedCarrier::Svelte(carrier) => carrier.owned_source = Some(source),
        }
        prepared
    }

    /// Compile `source` from an already-[`Self::prepare`]d carrier instead
    /// of re-parsing. Three-way framework agreement is enforced —
    /// `request.framework()`, `prepared`'s variant, and `inputs`'s variant
    /// must all name the same framework, mirroring exactly the two-way
    /// check [`Self::compile`] already performs (a `PreparedCarrier`
    /// disagreeing with the pair is the SAME class of error as `inputs`
    /// disagreeing with `request`, so it maps to the identical
    /// [`DirectCompileError::FrameworkMismatch`] variant, never a new one).
    ///
    /// `source`/`request` are revalidated against the carrier's recorded
    /// digests on every call — a stale carrier (a different `source`, or
    /// different Vue `delimiters`/`is_custom_element`) is a typed
    /// [`DirectCompileError::StalePreparedInput`], never a silently-wrong
    /// compiled result. The carrier's retained parse is reused; the
    /// request and inputs are always the caller's fresh values, exactly
    /// like the direct route.
    pub fn compile_prepared<'a>(
        &self,
        source: &'a str,
        prepared: &PreparedCarrier,
        request: &CompileRequest,
        inputs: DirectExecutionInputs<'a>,
    ) -> Result<DirectCompileOutput, DirectCompileError> {
        match (request.framework(), inputs) {
            (FrameworkCompileRequest::Vue(_), DirectExecutionInputs::Vue { execution, macros }) => {
                let PreparedCarrier::Vue(carrier) = prepared else {
                    return Err(DirectCompileError::FrameworkMismatch {
                        expected: "Vue",
                        actual: "Svelte",
                    });
                };
                if carrier.source_digest != source_digest(source) {
                    return Err(DirectCompileError::StalePreparedInput {
                        reason: StalePreparedReason::SourceChanged,
                    });
                }
                let vue = request.vue().expect("dispatch already matched Vue");
                if carrier.parse_identity_digest != vue_parse_identity_digest(vue) {
                    return Err(DirectCompileError::StalePreparedInput {
                        reason: StalePreparedReason::ParseOptionsChanged,
                    });
                }
                self.compile_vue_from_parsed(source, &carrier.parsed, request, execution, macros)
            }
            (FrameworkCompileRequest::Svelte(_), DirectExecutionInputs::Svelte { execution }) => {
                let PreparedCarrier::Svelte(carrier) = prepared else {
                    return Err(DirectCompileError::FrameworkMismatch {
                        expected: "Svelte",
                        actual: "Vue",
                    });
                };
                if carrier.source_digest != source_digest(source) {
                    return Err(DirectCompileError::StalePreparedInput {
                        reason: StalePreparedReason::SourceChanged,
                    });
                }
                self.compile_svelte_from_parsed(source, &carrier.parsed, request, execution)
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

    /// Compile every item in `items`, sharing one [`PreparedCarrier`]
    /// across items whose `(framework, source, Vue parse-options)` group
    /// key matches — `report.cold_build_count` is the number of
    /// [`Self::prepare`] calls actually performed (one per distinct
    /// group), `report.reuse_count` is the number of
    /// [`Self::compile_prepared`] calls. An item refused by the
    /// product/capability preflight is recorded as `Err` in its original
    /// slot and never prepared, so it increments neither count. Never
    /// reorders: `results[i]` is always item `i`'s own outcome, and
    /// one item's `Err` never affects any other item's entry — each
    /// iteration only reads the shared, immutable group carrier and writes
    /// its own `results` slot.
    ///
    /// An empty `items` returns immediately with zero counts and no
    /// allocator/parse work at all. No persistent cache: this call's
    /// grouping is local to the call — a second `compile_batch` call never
    /// reuses a carrier a prior call built.
    pub fn compile_batch(&self, items: &[BatchCompileItem<'_>]) -> BatchCompileOutput {
        if items.is_empty() {
            return BatchCompileOutput {
                results: Vec::new(),
                report: CompileBatchReport {
                    cold_build_count: 0,
                    reuse_count: 0,
                },
            };
        }

        // `carriers` alone owns group order — first appearance in `items`,
        // which is also the order `prepare` runs in. `group_index` is a pure
        // lookup: it turns the per-item search from a scan of every existing
        // group (O(N x G), quadratic for a batch of all-distinct sources)
        // into one hash probe, and never decides an order.
        let mut carriers: Vec<PreparedCarrier> = Vec::new();
        let mut group_index: FxHashMap<BatchGroupKey, usize> = FxHashMap::default();
        let mut report = CompileBatchReport {
            cold_build_count: 0,
            reuse_count: 0,
        };
        let mut results = Vec::with_capacity(items.len());

        for item in items {
            if let Err(error) = refuse_inputs_mismatch(item.request, item.inputs) {
                results.push(Err(error));
                continue;
            }
            if let Err(error) = refuse_unproducible_plan(item.request) {
                results.push(Err(error));
                continue;
            }
            let key = batch_group_key(item.source, item.request);
            let idx = match group_index.entry(key) {
                Entry::Occupied(slot) => *slot.get(),
                Entry::Vacant(slot) => {
                    let carrier = self.prepare(item.source, item.request);
                    carriers.push(carrier);
                    report.cold_build_count += 1;
                    *slot.insert(carriers.len() - 1)
                }
            };
            let carrier = &carriers[idx];
            let result = self.compile_prepared(item.source, carrier, item.request, item.inputs);
            report.reuse_count += 1;
            results.push(result);
        }

        BatchCompileOutput { results, report }
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
/// to force the compile core's single-`ssr`-mode-per-call derivation
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
/// `<svelte:options customElement>` exists.
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
    use crate::compile_request::svelte::{SvelteFragmentsRequest, SvelteRunesRequest};

    let namespace = resolve_svelte_namespace(svelte_request.namespace)?;

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
        prepared_styles: Vec::new(),
    })
}

fn svelte_fragment_role(kind: ProductKind) -> &'static str {
    match kind {
        ProductKind::RuntimeServer => "server",
        _ => "client",
    }
}

// ── Prepared/batch digests ─────────────────────────────────────────

fn hash_len_prefixed_bytes(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn hash_len_prefixed_str(hasher: &mut blake3::Hasher, s: &str) {
    hash_len_prefixed_bytes(hasher, s.as_bytes());
}

fn hash_len_prefixed_opt_str(hasher: &mut blake3::Hasher, s: Option<&str>) {
    match s {
        Some(s) => {
            hasher.update(&[1u8]);
            hash_len_prefixed_str(hasher, s);
        }
        None => {
            hasher.update(&[0u8]);
        }
    }
}

fn hash_usize(hasher: &mut blake3::Hasher, n: usize) {
    hasher.update(&(n as u64).to_le_bytes());
}

/// `blake3::hash(source.as_bytes())`, as a plain byte digest — the identity
/// [`StandaloneCompiler::prepare`]/[`StandaloneCompiler::compile_prepared`]
/// revalidate `source` against, and the first component of a
/// [`BatchGroupKey`]. A raw `blake3::hash` call is enough here — this is a
/// plain byte-digest binding check, not the canonical multi-field encoding
/// [`verter_identity::encoding::CanonicalEncoder`] exists for.
fn source_digest(source: &str) -> [u8; 32] {
    *blake3::hash(source.as_bytes()).as_bytes()
}

/// Digest over exactly the two fields [`crate::compile::parse_sfc`] reads
/// from a [`crate::compile_request::VueCompileRequest`] — `delimiters` and
/// `is_custom_element` — the identity [`StandaloneCompiler::compile_prepared`]
/// revalidates a Vue carrier's recorded parse options against. Every field
/// is length-prefixed so no two distinct `(delimiters, is_custom_element)`
/// pairs can hash to the same byte stream (no `Debug`-formatting
/// ambiguity). `None` delimiters hash a length-8 sentinel that no `Some`
/// pair can ever produce (every `Some` pair contributes at least two
/// 8-byte length prefixes, i.e. at least 16 bytes, before this field ends).
fn vue_parse_identity_digest(vue: &crate::compile_request::VueCompileRequest) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    match &vue.delimiters {
        Some((open, close)) => {
            hash_len_prefixed_str(&mut hasher, open);
            hash_len_prefixed_str(&mut hasher, close);
        }
        None => {
            hasher.update(&u64::MAX.to_le_bytes());
        }
    }
    hash_usize(&mut hasher, vue.is_custom_element.len());
    for prefix in &vue.is_custom_element {
        hash_len_prefixed_str(&mut hasher, prefix);
    }
    *hasher.finalize().as_bytes()
}

/// [`StandaloneCompiler::compile_batch`]'s grouping key — items sharing a
/// key share one [`PreparedCarrier`]. The framework tag lives in the enum
/// discriminant itself (a Vue item and a Svelte item never share a key even
/// if their `source_digest`s happened to collide).
///
/// `Hash` and `Eq` are both derived over the same discriminant and the same
/// whole byte-digest fields, so they cannot disagree — every field is a
/// plain `[u8; 32]` with no normalization, no interior mutability, and no
/// hand-written comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BatchGroupKey {
    Vue {
        source_digest: [u8; 32],
        parse_identity_digest: [u8; 32],
    },
    Svelte {
        source_digest: [u8; 32],
    },
}

fn batch_group_key(source: &str, request: &CompileRequest) -> BatchGroupKey {
    match request.framework() {
        FrameworkCompileRequest::Vue(_) => {
            let vue = request.vue().expect("dispatch already matched Vue");
            BatchGroupKey::Vue {
                source_digest: source_digest(source),
                parse_identity_digest: vue_parse_identity_digest(vue),
            }
        }
        FrameworkCompileRequest::Svelte(_) => BatchGroupKey::Svelte {
            source_digest: source_digest(source),
        },
    }
}

/// Fixed, `Debug`-independent rank for [`ProductKind`] — a stable numeric
/// identity to hash in [`direct_compile_output_digest`], so the digest does
/// not depend on `Debug` output. It does NOT order the artifact walk: that
/// walk follows publication order, which is observable. Never used to
/// compare kinds for equality (that stays `==`).
fn product_kind_rank(kind: ProductKind) -> u8 {
    match kind {
        ProductKind::RuntimeClient => 0,
        ProductKind::RuntimeServer => 1,
        ProductKind::IdeCompanion => 2,
        ProductKind::PublicApi => 3,
        ProductKind::Declarations => 4,
        ProductKind::Analysis => 5,
    }
}

/// Fixed, `Debug`-independent rank for [`FragmentDialect`] — folded into
/// [`direct_compile_output_digest`]'s per-artifact hash input.
fn fragment_dialect_rank(dialect: FragmentDialect) -> u8 {
    match dialect {
        FragmentDialect::JavaScript => 0,
        FragmentDialect::Jsx => 1,
        FragmentDialect::TypeScript => 2,
        FragmentDialect::Tsx => 3,
        FragmentDialect::Declaration => 4,
    }
}

/// Hashes a [`RuntimeOutputDescriptor`]'s own structured fields — never its
/// `Debug` formatting, per this module's own no-`Debug`-ambiguity
/// convention (see [`vue_parse_identity_digest`]). Every field is content
/// derived (`descriptor_hash` over `code`/the raw map/declared-source
/// identity tokens), so this is defense-in-depth over what `code`/
/// `source_map`/`scope_hash` already cover in the caller's digest, not a
/// distinct observation — but it is hashed explicitly rather than assumed
/// redundant, since a caller changing declared-source identity (a
/// different `filename`) with byte-identical code/map would otherwise be
/// invisible to the digest.
fn hash_output_descriptor(hasher: &mut blake3::Hasher, descriptor: &RuntimeOutputDescriptor) {
    hash_len_prefixed_str(hasher, &descriptor.source_space.token);
    hasher.update(&[descriptor.source_space.kind as u8]);
    hash_len_prefixed_str(hasher, &descriptor.source_space.source_token);
    hash_len_prefixed_str(hasher, &descriptor.source_space.content_hash);
    hash_usize(hasher, descriptor.source_space.utf8_byte_len as usize);
    hash_len_prefixed_str(hasher, &descriptor.content_artifact.token);
    hash_len_prefixed_str(hasher, &descriptor.content_artifact.source_space_token);
    hash_len_prefixed_str(hasher, &descriptor.content_artifact.content_hash);
    hash_usize(hasher, descriptor.content_artifact.utf8_byte_len as usize);
    hash_len_prefixed_str(hasher, &descriptor.source_map.map_hash);
    hash_len_prefixed_str(hasher, &descriptor.source_map.destination_space_token);
    hash_usize(hasher, descriptor.source_map.declared_space_tokens.len());
    for token in &descriptor.source_map.declared_space_tokens {
        hash_len_prefixed_str(hasher, token);
    }
    hash_len_prefixed_opt_str(hasher, descriptor.source_map.raw_map.as_deref());
    hasher.update(&[descriptor.source_map.fidelity as u8]);
}

/// A canonical, length-prefixed blake3 digest over every field
/// [`AssembledArtifact`]/[`RuntimeStyleBlock`]/[`CompileDiagnostic`] actually
/// expose: per-artifact `kind`/`code`/`dialect`/both source-map slots, hashed
/// in the order [`crate::assembly::ArtifactSet::artifacts`] exposes them —
/// publication order is part of the observable result, so a route that
/// reordered artifacts must produce a different digest, not the same one, then
/// `styles`' `code`/`source_map`/`lang`/`scope_hash`/`has_global`/
/// `output_descriptor` (via [`hash_output_descriptor`] — itself derived
/// purely from `code`/the raw map/declared-source identity, but hashed
/// explicitly rather than assumed redundant), then `diagnostics`'
/// `severity`/`code`/`message`/`span`. Lets a result-identity comparison
/// across routes (direct / prepared-first / prepared-repeat / batch) report
/// ONE short mismatching digest per fixture/route instead of a giant string
/// diff. Shared verbatim by this module's own tests and the
/// `compiler_route_overhead` bench harness — do not write a second copy of this
/// logic anywhere else.
pub fn direct_compile_output_digest(output: &DirectCompileOutput) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();

    let artifacts = output.artifacts.artifacts();
    hash_usize(&mut hasher, artifacts.len());
    for artifact in artifacts {
        hash_usize(&mut hasher, product_kind_rank(artifact.kind()) as usize);
        hash_len_prefixed_str(&mut hasher, artifact.code());
        hash_usize(
            &mut hasher,
            fragment_dialect_rank(artifact.dialect()) as usize,
        );
        hash_len_prefixed_opt_str(&mut hasher, artifact.source_projection_map());
        hash_len_prefixed_opt_str(&mut hasher, artifact.runtime_source_map());
    }

    hash_usize(&mut hasher, output.styles.len());
    for style in &output.styles {
        hash_len_prefixed_str(&mut hasher, &style.code);
        hash_len_prefixed_opt_str(&mut hasher, style.source_map.as_deref());
        hash_len_prefixed_opt_str(&mut hasher, style.lang.as_deref());
        hash_len_prefixed_opt_str(&mut hasher, style.scope_hash.as_deref());
        hasher.update(&[style.has_global as u8]);
        hash_output_descriptor(&mut hasher, &style.output_descriptor);
    }

    hash_usize(&mut hasher, output.diagnostics.len());
    for diagnostic in &output.diagnostics {
        hash_usize(&mut hasher, diagnostic.severity as usize);
        hash_len_prefixed_str(&mut hasher, &diagnostic.code);
        hash_len_prefixed_str(&mut hasher, &diagnostic.message);
        match diagnostic.span {
            Some(span) => {
                hasher.update(&[1u8]);
                hasher.update(&span.start.to_le_bytes());
                hasher.update(&span.end.to_le_bytes());
            }
            None => {
                hasher.update(&[0u8]);
            }
        }
    }

    *hasher.finalize().as_bytes()
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
        prepared_styles: Vec::new(),
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
            matches!(
                error,
                DirectCompileError::Svelte(ClientCompileError::Unsupported(
                    UnsupportedSvelteRuntimeSurface::ServerGenerate { .. }
                ))
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn svelte_dual_runtime_client_and_server_request_fails_closed_with_no_partial_output() {
        // Both kinds requested together: the server half is unproducible —
        // the WHOLE compile must refuse before parse, never publish just
        // the client half.
        let request = svelte_request(vec![
            CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
            CompileProduct::RuntimeServer(RuntimeProductRequest::default()),
        ]);
        let error = StandaloneCompiler
            .compile(SVELTE_SOURCE, &request, svelte_inputs())
            .expect_err("the SSR half must refuse the whole compile");
        assert!(
            matches!(
                error,
                DirectCompileError::Svelte(ClientCompileError::Unsupported(
                    UnsupportedSvelteRuntimeSurface::ServerGenerate { .. }
                ))
            ),
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

    /// `compile_batch`'s `group_index` map invokes `Eq` only on a hash
    /// collision, so a `BatchGroupKey` whose `Eq` (or `Hash`) stopped
    /// seeing a field would merge distinct groups without any batch test
    /// noticing. Pin both halves on `parse_identity_digest`: two keys
    /// differing only there must be unequal AND hash apart under the
    /// exact hasher the map probes with.
    #[test]
    fn batch_group_key_eq_and_hash_both_see_parse_identity_digest() {
        use std::hash::BuildHasher;

        let a = BatchGroupKey::Vue {
            source_digest: [7u8; 32],
            parse_identity_digest: [1u8; 32],
        };
        let b = BatchGroupKey::Vue {
            source_digest: [7u8; 32],
            parse_identity_digest: [2u8; 32],
        };
        assert_ne!(a, b, "Eq must discriminate on parse_identity_digest");

        let group_index: FxHashMap<BatchGroupKey, usize> = FxHashMap::default();
        assert_ne!(
            group_index.hasher().hash_one(a),
            group_index.hasher().hash_one(b),
            "Hash must discriminate on parse_identity_digest"
        );
    }
}

#[cfg(test)]
#[path = "standalone_prepared_tests.rs"]
mod prepared_tests;
