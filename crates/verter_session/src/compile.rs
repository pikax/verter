//! Main module assembly.

use verter_compiler::assembly::{
    assemble_vue_runtime_main, VueMainCompositionFailure, VueMainDecoration, VueRuntimeMainRequest,
};
use verter_compiler::compile_request::RuntimeHmrStrategy;
use verter_compiler::framework_common::RuntimeCompileOutput;

use crate::id::render_ids;
use crate::types::{CompileProfile, FileMeta, HmrStrategy, VirtualNodeKind};

#[cfg(test)]
pub(crate) use verter_compiler::assembly::{map_compose, map_input};

#[cfg(test)]
mod compile_tests;
#[cfg(test)]
mod map_equality_tests;
#[cfg(test)]
mod map_tests;

pub use verter_compiler::assembly::{
    AssembleMapFailure, MapFragment, SfcRewriteRefusal, UncomposableCode, UncomposableFamily,
};

/// Assembled Vue runtime main module: code and map as one result.
///
/// `source_map` is `None` only when no map was requested — not an empty
/// map. A requested map is always produced, even if no fragment
/// contributes a mapping: an empty artifact is truthful; "no map"
/// would look like a map-disabled compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledVueModule {
    pub code: String,
    pub source_map: Option<String>,
    /// The exact language id (`"js"`/`"jsx"`/`"ts"`/`"tsx"`) this module's
    /// fragments were validated under and [`Self::code`] was final-parsed
    /// under — derived ONCE from `meta`/`profile` and reused for both, so
    /// a caller that also needs the Main virtual node's language (e.g.
    /// `virtual_file_pipeline.rs`'s `compile_entry`) reads it here rather
    /// than independently re-deriving it a second time.
    pub lang: String,
}

/// Every way [`assemble_vue_main_module`] can fail to publish a Main
/// module: input-map validation and the `__sfc__` rewrite
/// ([`AssembleMapFailure`]), per-piece fragment-grammar validation
/// ([`verter_compiler::assembly::FragmentRefusal`]), sequential
/// composition ([`verter_compiler::assembly::ComposeRefusal`]), and the
/// final atomic-publication boundary
/// ([`verter_compiler::assembly::AssemblyRefusal`]). Every branch of
/// `assemble_vue_main_module` propagates through `?` into this ONE typed
/// enum — never a `.expect()`/panic. This function receives and rewrites
/// PRODUCER-SUPPLIED bytes (the real compiled script/template output from
/// `verter_compiler`), so a fragment-grammar, composition, or final-parse
/// refusal here can genuinely reflect a malformed upstream compile, not
/// merely an internal assembly-scaffold bug — the contract is that every
/// assembly failure becomes a typed non-success, never an unwind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueMainAssemblyFailure {
    /// A required input map was missing/uncomposable, or the script's
    /// declared `__sfc__` export-placement fact was invalid.
    InputMap(AssembleMapFailure),
    /// One of this function's own scaffold/content fragments did not
    /// parse under its declared contract and dialect.
    FragmentValidation(verter_compiler::assembly::FragmentRefusal),
    /// Sequential fragment composition failed (an uncomposable per-
    /// fragment map — every fragment's map was already validated or
    /// freshly re-encoded by this point, so this is a defect, not an
    /// expected input, but it is still reported typed rather than
    /// panicking).
    Composition(verter_compiler::assembly::ComposeRefusal),
    /// The final atomic-publication boundary refused the composed
    /// artifact (exact-cardinality, required-map, undeclared-helper, or
    /// final-parse checks).
    Publication(verter_compiler::assembly::AssemblyRefusal),
}

impl From<AssembleMapFailure> for VueMainAssemblyFailure {
    fn from(failure: AssembleMapFailure) -> Self {
        Self::InputMap(failure)
    }
}

/// Lifts the shared [`verter_compiler::assembly`] composer's own failure
/// shape into this crate's public, UNCHANGED four-variant enum — the
/// `__sfc__`-placement half of a composition failure lands in `InputMap`
/// (alongside this crate's own [`AssembleMapFailure::MissingRequiredInputMap`]/
/// `UncomposableInputMap`, since all three answer "why did assembling this
/// map-bearing input fail"); a fragment-grammar or sequencing defect keeps
/// its own variant.
impl From<verter_compiler::assembly::VueMainAssemblyFailure> for VueMainAssemblyFailure {
    fn from(failure: verter_compiler::assembly::VueMainAssemblyFailure) -> Self {
        match failure {
            verter_compiler::assembly::VueMainAssemblyFailure::InputMap(failure) => {
                Self::InputMap(failure)
            }
            verter_compiler::assembly::VueMainAssemblyFailure::Composition(composition) => {
                match composition {
                    VueMainCompositionFailure::InvalidSfcExportPlacement(reason) => {
                        Self::InputMap(AssembleMapFailure::InvalidSfcExportPlacement { reason })
                    }
                    VueMainCompositionFailure::FragmentValidation(reason) => {
                        Self::FragmentValidation(reason)
                    }
                    VueMainCompositionFailure::Composition(reason) => Self::Composition(reason),
                }
            }
            verter_compiler::assembly::VueMainAssemblyFailure::Publication(reason) => {
                Self::Publication(reason)
            }
        }
    }
}

impl std::fmt::Display for VueMainAssemblyFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputMap(e) => write!(f, "{e}"),
            Self::FragmentValidation(e) => {
                write!(
                    f,
                    "a Main-module fragment failed its declared grammar: {e:?}"
                )
            }
            Self::Composition(e) => write!(f, "Main-module fragment composition failed: {e:?}"),
            Self::Publication(e) => write!(f, "Main-module publication failed: {e:?}"),
        }
    }
}

impl std::error::Error for VueMainAssemblyFailure {}

pub(crate) fn hmr_strategy(strategy: HmrStrategy) -> RuntimeHmrStrategy {
    match strategy {
        HmrStrategy::None => RuntimeHmrStrategy::None,
        HmrStrategy::Vite => RuntimeHmrStrategy::Vite,
        HmrStrategy::Webpack => RuntimeHmrStrategy::Webpack,
    }
}

/// Host-owned virtual-file identifiers for Vue main assembly. Topology
/// (when and how they are imported) stays compiler-owned.
pub(crate) fn vue_main_host_identifiers(
    canonical_id: &str,
    style_count: usize,
    custom_count: usize,
    meta: &FileMeta,
) -> (Vec<String>, Vec<String>) {
    let styles = (0..style_count)
        .map(|idx| render_ids(canonical_id, &VirtualNodeKind::Style { index: idx }, meta).0)
        .collect();
    let customs = (0..custom_count)
        .map(|idx| render_ids(canonical_id, &VirtualNodeKind::Custom { index: idx }, meta).0)
        .collect();
    (styles, customs)
}

pub(crate) fn vue_main_decoration_from_axes(
    canonical_id: &str,
    style_count: usize,
    custom_count: usize,
    meta: &FileMeta,
    profile: &VueMainAssemblyAxes,
) -> VueMainDecoration {
    let (style_specifiers, custom_specifiers) =
        vue_main_host_identifiers(canonical_id, style_count, custom_count, meta);
    VueMainDecoration {
        hmr: hmr_strategy(profile.hmr_strategy),
        is_production: profile.is_production,
        emit_ssr_module_registration: profile.emit_ssr_module_registration,
        ssr_module_id: profile.ssr_module_id.clone(),
        style_specifiers,
        custom_specifiers,
    }
}

/// Transport adapter for Vue `_sfc_main` assembly.
///
/// Host supplies identifiers (`render_ids`) and request axes; the Vue
/// compiler owns topology and emits the assembled artifact. Same blocks
/// produce byte-identical output. Hidden host-side reconstruction of
/// imports/HMR/SSR is not a fallback.
///
/// # Errors
///
/// [`VueMainAssemblyFailure`] on any failure — a missing/uncomposable
/// required map, an invalid `__sfc__` fact, a fragment that fails its own
/// declared grammar, a composition defect, or a publication refusal. Never
/// a panic: every producer-supplied byte this function rewrites or
/// sequences can genuinely be malformed, so every failure mode is typed.
pub fn assemble_vue_main_module(
    canonical_id: &str,
    compiled: &RuntimeCompileOutput,
    meta: &FileMeta,
    profile: &CompileProfile,
) -> Result<AssembledVueModule, VueMainAssemblyFailure> {
    assemble_vue_main_module_with_axes(
        canonical_id,
        compiled,
        meta,
        &VueMainAssemblyAxes::from(profile),
    )
}

/// Exactly the axes the compiler-owned `Main` assembly reads, named
/// independently of the vocabulary a caller happens to state them in.
///
/// The assembly itself has one implementation; this carrier is what lets
/// a route holding a canonical compiler request state those axes directly
/// instead of round-tripping them through a compile profile it does not
/// otherwise have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VueMainAssemblyAxes {
    /// Emit JavaScript regardless of the carrier's authored script dialect.
    pub(crate) force_js: bool,
    /// Compose and return the assembled module's source map.
    pub(crate) source_map: bool,
    /// Module specifier the assembled imports resolve the Vue runtime from.
    pub(crate) runtime_module_name: Option<String>,
    /// Assemble the server (SSR) shape.
    pub(crate) ssr: bool,
    /// Production build: no `__file`, no HMR acceptance.
    pub(crate) is_production: bool,
    /// Dev-server tooling flavour, gating `__file` and HMR acceptance.
    pub(crate) hmr_strategy: HmrStrategy,
    /// Emit the Vite SSR-manifest module registration on an SSR assembly.
    pub(crate) emit_ssr_module_registration: bool,
    /// Manifest key form the SSR registration records; the canonical id
    /// is the fallback.
    pub(crate) ssr_module_id: Option<String>,
}

impl From<&CompileProfile> for VueMainAssemblyAxes {
    fn from(profile: &CompileProfile) -> Self {
        Self {
            force_js: profile.force_js,
            source_map: profile.source_map,
            runtime_module_name: profile.runtime_module_name.clone(),
            ssr: profile.ssr,
            is_production: profile.is_production,
            hmr_strategy: profile.hmr_strategy,
            emit_ssr_module_registration: profile.emit_ssr_module_registration,
            ssr_module_id: profile.ssr_module_id.clone(),
        }
    }
}

/// Transport adapter: host identifiers plus authorship metadata. Semantic
/// assembly (dialect, map validation, composition, CCA2A) is compiler-owned.
///
/// # Errors
///
/// See [`assemble_vue_main_module`].
pub(crate) fn assemble_vue_main_module_with_axes(
    canonical_id: &str,
    compiled: &RuntimeCompileOutput,
    meta: &FileMeta,
    profile: &VueMainAssemblyAxes,
) -> Result<AssembledVueModule, VueMainAssemblyFailure> {
    let runtime = profile.runtime_module_name.as_deref().unwrap_or("vue");
    let assembled = assemble_vue_runtime_main(VueRuntimeMainRequest {
        canonical_id,
        compiled,
        script_lang: meta.script_lang.as_deref(),
        has_script: meta.has_script,
        has_template: meta.has_template,
        force_js: profile.force_js,
        source_map: profile.source_map,
        runtime,
        ssr: profile.ssr,
        decoration: vue_main_decoration_from_axes(
            canonical_id,
            compiled.styles.len(),
            compiled.custom_blocks.len(),
            meta,
            profile,
        ),
    })?;
    Ok(AssembledVueModule {
        code: assembled.code,
        source_map: assembled.source_map,
        lang: assembled.lang,
    })
}
