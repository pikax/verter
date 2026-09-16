//! Vue main-module composition owned by the Vue runtime compiler.
//!
//! One Vue request has one assembly authority: this module. It owns script/
//! template contribution order, host-identifier topology (style/custom
//! virtual imports, HMR, `__file`, SSR-manifest wiring), declared
//! imports/exports, dialect, qualified maps, and compile-artifact-set
//! [`super::publish::CompileArtifactSet`] emission. Callers supply only
//! host-owned identifiers through [`VueMainDecoration`]; they do not infer
//! framework module topology or reconstruct it from block fields.

#[cfg(any(test, feature = "test-support"))]
use std::cell::Cell;
use std::ops::Range;
use std::sync::Arc;

use oxc_sourcemap::SourceMap;

use super::compose::{assemble_sequence, ComposeRefusal};
use super::custom_block::{
    CustomBlockContent, CustomBlockDescriptor, CustomBlockDescriptorRequest, CustomBlockLifecycle,
};
use super::fragment::{
    ArtifactContent, ArtifactProvenance, ArtifactRelation, ArtifactRelationKind, CompileArtifact,
    DeclaredImport, DeclaredImportKind, Fragment, FragmentDialect, FragmentRefusal,
    FrameworkDomain, PlacementSlot, SfcExportPlacement, SyntacticContract, ValidatedFragment,
};
use super::map_compose::to_source_map;
use super::map_input::{
    agree_source_root, validate_and_decode, AssembleMapFailure, DecodedFragmentMap, MapFragment,
};
use super::plan::{PlannedArtifact, ProductPlan};
use super::publish::{
    publish, ArtifactContribution, ArtifactSet, AssemblyRefusal, CompileArtifactSet,
};
use super::source_space::{
    ArtifactMapFamily, ArtifactMapSegment, QualifiedArtifactMap, SourceSpaceKind,
};
use super::source_unit::{
    source_unit_id, ArtifactSourceUnit, ContentId, SourceId, SourceRevision, SourceUnit,
};
use crate::code_transform::CodeTransform;
use crate::compile::format_import_specifier;
use crate::compile_request::{ProductKind, RuntimeHmrStrategy};
use crate::framework_common::{RuntimeCompileOutput, TemplateRenderExport};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ResultContractId};

/// Every `binding_ranges` entry's own bytes must equal this literal — the
/// identifier every runtime-emission site writes before host assembly
/// renames it (see [`crate::script::SFC_BINDING`], mirrored here as an
/// independent constant rather than a cross-module `pub` surface for one
/// literal).
const SFC_BINDING: &str = "__sfc__";
/// Every declared binding reference is renamed to this.
const SFC_MAIN_BINDING: &str = "_sfc_main";
/// The exact bytes a declared `export_statement_range` must contain — the
/// terminal statement removed once the assembled module re-exports the
/// composed result under its own name.
const EXPORT_STATEMENT_TEXT: &str = "export default __sfc__;\n";

/// Why the `__sfc__` → `_sfc_main` rewrite refused a declared
/// [`SfcExportPlacement`] fact. Every variant is a producer defect — a
/// script whose own bytes disagree with what its producer claims about them
/// — never a condition [`rewrite_script`] recovers from by falling back to
/// scanning. A MISSING fact (`None`) is deliberately not one of these: it is
/// indistinguishable, without scanning, from a genuinely empty declared fact
/// (a script with nothing to rewrite), so `rewrite_script` treats the two
/// identically rather than refusing one of two equally-untestable-without-
/// a-scan possibilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SfcRewriteRefusal {
    /// A declared range's `end` exceeds the script's own byte length, or its
    /// `start`/`end` do not land on a UTF-8 character boundary.
    OutOfBounds { start: u32, end: u32 },
    /// A declared binding range's bytes are not literally `__sfc__`.
    InconsistentBindingRange { start: u32, end: u32 },
    /// The declared export-statement range's bytes are not literally
    /// `export default __sfc__;\n`.
    InconsistentExportStatement { start: u32, end: u32 },
    /// A declared binding range partially overlaps the declared
    /// export-statement range — neither fully inside it (where it would be
    /// removed as part of the whole statement) nor fully outside it.
    BindingRangeOverlapsExportStatement { start: u32, end: u32 },
    /// Chaining the rewrite's own transform onto the caller-supplied input
    /// map failed — the input map names a generated position the rewrite's
    /// transform does not tile (out of bounds, or an unsupported chunk
    /// shape). The rewrite's own transform is always overwrite-only over
    /// positions already validated against `code`; a chain failure means the
    /// INPUT map disagrees with the script it claims to describe.
    ChainFailed(crate::code_transform::SourceMapChainError),
}

fn checked_slice(code: &str, start: u32, end: u32) -> Result<&str, SfcRewriteRefusal> {
    let (s, e) = (start as usize, end as usize);
    if start > end || e > code.len() || !code.is_char_boundary(s) || !code.is_char_boundary(e) {
        return Err(SfcRewriteRefusal::OutOfBounds { start, end });
    }
    Ok(&code[s..e])
}

/// Apply the ONE authorized rewrite — every `binding_ranges` entry renamed
/// to `_sfc_main`, and the declared `export_statement_range` (if any)
/// removed — driven entirely by `fact`'s declared ranges. Never scans `code`
/// for the landmark strings: an out-of-bounds or inconsistent fact is a
/// typed [`SfcRewriteRefusal`], not a rescan. `fact: None` is treated
/// identically to a declared-but-empty fact (nothing to rewrite) — see
/// [`SfcRewriteRefusal`]'s own doc for why a missing fact is not itself a
/// refusal.
///
/// `map` is an ALREADY-DECODED [`SourceMap`] — this function performs no
/// JSON validation of its own. Each caller decodes under whatever trust
/// regime fits its own inputs: `verter_session`'s host composer decodes
/// under its hardened multi-fragment validator (host-authored/cross-tool
/// maps need it); the direct one-shot core decodes its own just-produced
/// map with the same trusted `SourceMap::from_json_string` every other
/// same-crate fragment map already goes through in
/// [`super::compose::assemble_sequence`].
///
/// Runs whether or not a map was requested: the rewrite determines the
/// module's bytes regardless of `map`.
pub(crate) fn rewrite_script(
    code: &str,
    fact: Option<&SfcExportPlacement>,
    map: Option<&SourceMap<'_>>,
) -> Result<(String, Option<String>), SfcRewriteRefusal> {
    let empty = SfcExportPlacement::default();
    let fact = fact.unwrap_or(&empty);

    // Validate every declared range to completion BEFORE any edit is
    // queued — a refusal must never leave a partially-rewritten transform.
    for range in &fact.binding_ranges {
        let slice = checked_slice(code, range.start, range.end)?;
        if slice != SFC_BINDING {
            return Err(SfcRewriteRefusal::InconsistentBindingRange {
                start: range.start,
                end: range.end,
            });
        }
    }
    if let Some(export) = &fact.export_statement_range {
        let slice = checked_slice(code, export.start, export.end)?;
        if slice != EXPORT_STATEMENT_TEXT {
            return Err(SfcRewriteRefusal::InconsistentExportStatement {
                start: export.start,
                end: export.end,
            });
        }
        for range in &fact.binding_ranges {
            let inside = range.start >= export.start && range.end <= export.end;
            let outside = range.end <= export.start || range.start >= export.end;
            if !inside && !outside {
                return Err(SfcRewriteRefusal::BindingRangeOverlapsExportStatement {
                    start: range.start,
                    end: range.end,
                });
            }
        }
    }

    // A binding fully inside the export statement is removed wholesale with
    // it — renaming it separately would be a redundant, overlapping edit
    // over the same bytes the export-statement overwrite already covers.
    let inside_export = |range: &Range<u32>| {
        fact.export_statement_range
            .as_ref()
            .is_some_and(|export| range.start >= export.start && range.end <= export.end)
    };

    let allocator = oxc_allocator::Allocator::default();
    let mut ct = CodeTransform::new(code, &allocator);
    for range in &fact.binding_ranges {
        if inside_export(range) {
            continue;
        }
        ct.overwrite(range.start, range.end, SFC_MAIN_BINDING);
    }
    if let Some(export) = &fact.export_statement_range {
        ct.overwrite(export.start, export.end, "");
    }
    let rewritten = ct.build_string();

    // The rewrite is an overwrite-only transform over positions this
    // function already validated against `code`, so a chain failure here is
    // unexpected — but `chain_source_map` genuinely returns failures for an
    // out-of-bounds or malformed INPUT map (`map` came from the caller, not
    // from this function's own transform), so it is reported typed rather
    // than unwound.
    let chained = map
        .map(|map| {
            ct.chain_source_map(map)
                .map(|chained| chained.to_json_string())
        })
        .transpose()
        .map_err(SfcRewriteRefusal::ChainFailed)?;

    Ok((rewritten, chained))
}

/// One compiler-owned decoration piece contributed to the composed
/// module's prelude (ahead of the script) or trailer (before the terminal
/// `export default`). Built only from [`VueMainDecoration`]; callers never
/// mint these to reconstruct topology.
#[derive(Debug, Clone, Default)]
pub struct ExtraFragment {
    pub role: &'static str,
    pub code: String,
    pub imports: Vec<DeclaredImport>,
}

/// Host-owned identifiers and request axes the Vue assembler consumes.
/// Topology (when to emit style/custom imports, HMR, `__file`, SSR
/// registration, and in what order) is compiler-owned; this struct carries
/// only the identifiers and knobs the host is allowed to supply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueMainDecoration {
    /// Dev-server tooling flavour gating `__file` and hot-accept.
    pub hmr: RuntimeHmrStrategy,
    /// Production build: no `__file`, no HMR acceptance.
    pub is_production: bool,
    /// Emit the Vite SSR-manifest module registration on an SSR assembly.
    /// Defaults true — official `transformMain` emits it on every SSR
    /// compile.
    pub emit_ssr_module_registration: bool,
    /// Manifest key form the SSR registration records; `canonical_id` is
    /// the fallback.
    pub ssr_module_id: Option<String>,
    /// Host-rendered style virtual-file specifiers, in inventory order.
    pub style_specifiers: Vec<String>,
    /// Host-rendered custom-block virtual-file specifiers, in inventory
    /// order.
    pub custom_specifiers: Vec<String>,
}

impl Default for VueMainDecoration {
    fn default() -> Self {
        Self {
            hmr: RuntimeHmrStrategy::None,
            is_production: false,
            emit_ssr_module_registration: true,
            ssr_module_id: None,
            style_specifiers: Vec::new(),
            custom_specifiers: Vec::new(),
        }
    }
}

impl VueMainDecoration {
    fn prelude(&self, compiled: &RuntimeCompileOutput) -> ExtraFragment {
        use std::fmt::Write;
        let mut prelude = String::new();
        let mut imports: Vec<DeclaredImport> = Vec::new();
        for id in self.style_specifiers.iter().take(compiled.styles.len()) {
            let _ = writeln!(prelude, "import \"{id}\"");
            imports.push(DeclaredImport {
                specifier: id.clone(),
                kind: DeclaredImportKind::SideEffect,
            });
        }
        for (idx, id) in self
            .custom_specifiers
            .iter()
            .take(compiled.custom_blocks.len())
            .enumerate()
        {
            let block_name = format!("block{idx}");
            let _ = writeln!(prelude, "import {block_name} from \"{id}\"");
            imports.push(DeclaredImport {
                specifier: id.clone(),
                kind: DeclaredImportKind::Default(block_name),
            });
        }
        if !prelude.is_empty() {
            prelude.push('\n');
        }
        ExtraFragment {
            role: "prelude",
            code: prelude,
            imports,
        }
    }

    fn trailer(
        &self,
        compiled: &RuntimeCompileOutput,
        canonical_id: &str,
        runtime: &str,
        ssr: bool,
    ) -> ExtraFragment {
        use std::fmt::Write;
        let mut trailer = String::new();
        let custom_count = self
            .custom_specifiers
            .len()
            .min(compiled.custom_blocks.len());
        for idx in 0..custom_count {
            let _ = writeln!(
                trailer,
                "if (typeof block{idx} === 'function') block{idx}(_sfc_main)"
            );
        }

        if !self.is_production && self.hmr != RuntimeHmrStrategy::None {
            let _ = writeln!(trailer, "_sfc_main.__file = {:?}", canonical_id);
        }

        if !self.is_production && !ssr {
            match self.hmr {
                RuntimeHmrStrategy::Vite => {
                    trailer.push_str("/* HMR(vite) */\n");
                    trailer.push_str("if (import.meta.hot) { import.meta.hot.accept(() => {}) }\n");
                }
                RuntimeHmrStrategy::Webpack => {
                    trailer.push_str("/* HMR(webpack) */\n");
                    trailer.push_str("if (module.hot) { module.hot.accept(() => {}) }\n");
                }
                RuntimeHmrStrategy::None => {}
            }
        }

        let mut imports: Vec<DeclaredImport> = Vec::new();
        if ssr && self.emit_ssr_module_registration {
            let _ = writeln!(
                trailer,
                "import {{ useSSRContext as __vite_useSSRContext }} from \"{runtime}\""
            );
            imports.push(DeclaredImport {
                specifier: runtime.to_string(),
                kind: DeclaredImportKind::Named(vec!["__vite_useSSRContext".to_string()]),
            });
            trailer.push_str("const _sfc_setup = _sfc_main.setup\n");
            trailer.push_str("_sfc_main.setup = (props, ctx) => {\n");
            trailer.push_str("  const ssrContext = __vite_useSSRContext()\n");
            let registered_id = self.ssr_module_id.as_deref().unwrap_or(canonical_id);
            let _ = writeln!(
                trailer,
                "  ;(ssrContext.modules || (ssrContext.modules = new Set())).add({:?})",
                registered_id
            );
            trailer.push_str("  return _sfc_setup ? _sfc_setup(props, ctx) : undefined\n");
            trailer.push_str("}\n");
        }
        ExtraFragment {
            role: "trailer",
            code: trailer,
            imports,
        }
    }
}

/// Everything [`compose_main_module`] needs: the compiled blocks, the
/// resolved dialect/product kind/runtime specifier, whether a map was
/// requested, every ALREADY-DECODED contributing map, and host-owned
/// identifiers. Never raw source or a
/// [`crate::compile_request::CompileRequest`] — a caller builds this from
/// its own already-produced [`RuntimeCompileOutput`] plus its own validated
/// maps.
pub struct VueMainModuleRequest<'a> {
    pub canonical_id: &'a str,
    pub compiled: &'a RuntimeCompileOutput,
    pub dialect: FragmentDialect,
    /// `RuntimeClient` or `RuntimeServer` — the one artifact this composer
    /// publishes.
    pub planned_kind: ProductKind,
    /// The runtime module specifier every emitted `import ... from` line
    /// resolves against (official default `"vue"`).
    pub runtime: &'a str,
    pub want_maps: bool,
    pub source_root: Option<&'a str>,
    /// The script's own pre-rewrite map, already decoded by the caller.
    pub script_map: Option<&'a SourceMap<'a>>,
    /// The template's map, already re-encoded by the caller through the
    /// canonical single-spelling encoder its own decode regime uses (or
    /// passed through verbatim when the caller's own template map is
    /// already canonical, as for a same-crate direct compile).
    pub template_map_json: Option<String>,
    /// Host-owned identifiers and request axes. Topology is compiler-owned.
    pub decoration: VueMainDecoration,
}

/// Compiler-owned Vue main-module assembly request. Session code supplies
/// host identifiers and authorship metadata; this crate owns dialect,
/// map validation, composition, and compile-artifact-set emission.
pub struct VueRuntimeMainRequest<'a> {
    pub canonical_id: &'a str,
    pub compiled: &'a RuntimeCompileOutput,
    /// Authored `<script>` lang, `None` when no script block exists.
    pub script_lang: Option<&'a str>,
    /// Authored-fragment inventory: a synthesized script is not authored.
    pub has_script: bool,
    /// Authored-fragment inventory: a synthesized template is not authored.
    pub has_template: bool,
    pub force_js: bool,
    pub source_map: bool,
    pub runtime: &'a str,
    pub ssr: bool,
    pub decoration: VueMainDecoration,
    /// Authored SFC bytes. Identity for the custom-block source unit uses
    /// this space directly; the assembler does not re-parse or re-emit it.
    pub source: &'a str,
    /// This compile's own retained custom-block facts (region/source_order/
    /// lang/src/attrs), in document order. Empty ⇒ zero custom-block work.
    pub custom_blocks: &'a [crate::compile::VerterCustomBlock],
}

/// Assembled Vue `_sfc_main` plus its compile-artifact-set relations.
///
/// `code` is the same allocation stored on the main artifact's
/// [`ArtifactContent::Available`] payload.
#[derive(Debug)]
pub struct VueRuntimeMainAssembled {
    pub code: Arc<str>,
    pub source_map: Option<String>,
    pub lang: String,
    pub artifacts: CompileArtifactSet,
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static VUE_MAIN_ASSEMBLY_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// Assemblies observed on this thread. STYLE-only demand must stay at zero.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn vue_main_assembly_count() -> usize {
    VUE_MAIN_ASSEMBLY_COUNT.with(Cell::get)
}

/// Reset this thread's assembly counter. Test-only observation of unrequested
/// Main work.
#[cfg(any(test, feature = "test-support"))]
pub fn reset_vue_main_assembly_count() {
    VUE_MAIN_ASSEMBLY_COUNT.with(|count| count.set(0));
}

fn record_vue_main_assembly() {
    #[cfg(any(test, feature = "test-support"))]
    VUE_MAIN_ASSEMBLY_COUNT.with(|count| count.set(count.get() + 1));
}

/// Every way [`compose_fragments`] can fail to compose a Main module's
/// fragments: the script's own declared `__sfc__` fact was invalid
/// ([`SfcRewriteRefusal`]), a scaffold/content fragment failed its own
/// declared grammar ([`FragmentRefusal`]), or sequential composition failed
/// ([`ComposeRefusal`]). Never a panic: every producer-supplied byte this
/// function rewrites or sequences can genuinely be malformed, so every
/// failure mode is typed. Does NOT cover publication — see
/// [`VueMainAssemblyFailure`] for the caller that also publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueMainCompositionFailure {
    InvalidSfcExportPlacement(SfcRewriteRefusal),
    FragmentValidation(FragmentRefusal),
    Composition(ComposeRefusal),
}

impl From<SfcRewriteRefusal> for VueMainCompositionFailure {
    fn from(failure: SfcRewriteRefusal) -> Self {
        Self::InvalidSfcExportPlacement(failure)
    }
}

impl From<FragmentRefusal> for VueMainCompositionFailure {
    fn from(failure: FragmentRefusal) -> Self {
        Self::FragmentValidation(failure)
    }
}

impl From<ComposeRefusal> for VueMainCompositionFailure {
    fn from(failure: ComposeRefusal) -> Self {
        Self::Composition(failure)
    }
}

impl std::fmt::Display for VueMainCompositionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSfcExportPlacement(e) => {
                write!(
                    f,
                    "the script's declared __sfc__ export-placement fact is invalid: {e:?}"
                )
            }
            Self::FragmentValidation(e) => {
                write!(
                    f,
                    "a Main-module fragment failed its declared grammar: {e:?}"
                )
            }
            Self::Composition(e) => write!(f, "Main-module fragment composition failed: {e:?}"),
        }
    }
}

impl std::error::Error for VueMainCompositionFailure {}

/// Every way [`compose_main_module`] can fail to publish a Main module:
/// fragment composition failed ([`VueMainCompositionFailure`]), or the
/// final atomic-publication boundary refused the composed artifact
/// ([`AssemblyRefusal`]). Never a panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueMainAssemblyFailure {
    InputMap(AssembleMapFailure),
    Composition(VueMainCompositionFailure),
    Publication(AssemblyRefusal),
}

impl From<AssembleMapFailure> for VueMainAssemblyFailure {
    fn from(failure: AssembleMapFailure) -> Self {
        Self::InputMap(failure)
    }
}

impl From<VueMainCompositionFailure> for VueMainAssemblyFailure {
    fn from(failure: VueMainCompositionFailure) -> Self {
        Self::Composition(failure)
    }
}

impl From<AssemblyRefusal> for VueMainAssemblyFailure {
    fn from(failure: AssemblyRefusal) -> Self {
        Self::Publication(failure)
    }
}

impl std::fmt::Display for VueMainAssemblyFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputMap(e) => write!(f, "{e}"),
            Self::Composition(e) => write!(f, "{e}"),
            Self::Publication(e) => write!(f, "Main-module publication failed: {e:?}"),
        }
    }
}

impl std::error::Error for VueMainAssemblyFailure {}

/// One scaffold/content piece, minted, validated, and pushed onto
/// `fragments` in one step — every piece of the Main module goes through
/// this, so `assemble_sequence`/`publish` always compose the SAME
/// collection that was actually validated, never a raw `{code, map}` pair
/// reconstructed on the side.
#[allow(clippy::too_many_arguments)]
fn mint_and_validate(
    fragments: &mut Vec<ValidatedFragment>,
    canonical_id: &str,
    role: &str,
    planned_kind: ProductKind,
    placement: PlacementSlot,
    dialect: FragmentDialect,
    code: String,
    source_map: Option<String>,
    imports: Vec<DeclaredImport>,
) -> Result<(), VueMainCompositionFailure> {
    let fragment = Fragment {
        domain: FrameworkDomain::Vue,
        product: planned_kind,
        source_unit: source_unit_id(canonical_id, role),
        source_space: SourceSpaceKind::GeneratedFragment,
        placement,
        contract: SyntacticContract::CompleteModule,
        dialect,
        code,
        source_map,
        imports,
        exports: Vec::new(),
        helpers: Vec::new(),
        dependencies: Vec::new(),
    };
    let validated = fragment.validate()?;
    fragments.push(validated);
    Ok(())
}

/// One Main module's composed fragments plus the sequenced output they
/// produce — owned, so a caller that needs to combine this artifact's
/// contribution with OTHER contributions before one shared [`publish`] call
/// (the direct one-shot core, publishing a whole multi-product
/// [`crate::compile_request::CompileRequest`] atomically) can keep the
/// fragments alive across that combination. [`compose_main_module`] is the
/// single-artifact convenience that composes AND publishes in one call for a
/// caller that only ever publishes this one artifact.
pub(crate) struct ComposedFragments {
    pub fragments: Vec<ValidatedFragment>,
    pub code: String,
    pub source_map: String,
    pub emitted_imports: Vec<DeclaredImport>,
}

/// Compose the Vue `_sfc_main` runtime module's fragments from a
/// framework-neutral [`RuntimeCompileOutput`] plus caller-owned decoration —
/// EVERYTHING [`compose_main_module`] does except the final [`publish`] call.
/// Shared by the session identifier/axes transport (`assemble_vue_main_module`
/// → [`assemble_vue_runtime_main`]) and the direct one-shot core (through
/// this function directly, so it can publish this artifact atomically
/// alongside sibling contributions from the SAME
/// [`crate::compile_request::CompileRequest`]). Decoration topology is
/// compiler-owned ([`VueMainDecoration`]); the session adapter does not
/// compose.
///
/// Script rewrites (`__sfc__` → `_sfc_main`, then strip
/// `export default _sfc_main;\n`) go through [`rewrite_script`] so the same
/// chunk list produces bytes and map. The template is written verbatim.
/// Every scaffold/content piece is a real, VALIDATED
/// [`crate::assembly::fragment::Fragment`] — sequenced through
/// [`assemble_sequence`] — never a raw `{code, source_map}` pair.
///
/// # Errors
///
/// [`VueMainCompositionFailure`] on any failure — an invalid `__sfc__` fact,
/// a fragment that fails its own declared grammar, or a composition defect.
/// Never a panic.
pub(crate) fn compose_fragments(
    request: VueMainModuleRequest<'_>,
) -> Result<ComposedFragments, VueMainCompositionFailure> {
    use std::fmt::Write;

    let VueMainModuleRequest {
        canonical_id,
        compiled,
        dialect,
        planned_kind,
        runtime,
        want_maps,
        source_root,
        script_map,
        template_map_json,
        decoration,
    } = request;
    let ssr = planned_kind == ProductKind::RuntimeServer;
    let prelude_extra = vec![decoration.prelude(compiled)];
    let trailer_extra = vec![decoration.trailer(compiled, canonical_id, runtime, ssr)];

    let rewritten_script = compiled
        .script
        .as_ref()
        .map(|script| {
            rewrite_script(
                &script.code,
                script.sfc_export_placement.as_ref(),
                script_map,
            )
        })
        .transpose()?;

    let mut fragments: Vec<ValidatedFragment> = Vec::new();

    // ── prelude: caller-owned decoration, ahead of the script ──────────
    for extra in &prelude_extra {
        mint_and_validate(
            &mut fragments,
            canonical_id,
            extra.role,
            planned_kind,
            PlacementSlot::ModulePrelude,
            dialect,
            extra.code.clone(),
            None,
            extra.imports.clone(),
        )?;
    }

    // ── script (including its imports) — precedes the template's runtime
    // helper imports, official `@vitejs/plugin-vue` / `@vue/compiler-sfc`
    // order. ESM hoists imports either way; the order is conformance. ────
    let mut script_scaffold = String::new();
    let (script_code, script_source_map, script_imports): (
        String,
        Option<String>,
        Vec<DeclaredImport>,
    ) = match &rewritten_script {
        Some((code, map)) => {
            // The script's own runtime-helper imports are ALREADY embedded
            // in `code` (written by the SAME `CodeTransform` that produced
            // it) — this fragment declares them as a fact about bytes it
            // already contains, never a second import line this assembler
            // writes itself.
            let script_imports = compiled
                .script
                .as_ref()
                .map(|s| &s.runtime_imports)
                .filter(|names| !names.is_empty())
                .map(|names| {
                    vec![DeclaredImport {
                        specifier: runtime.to_string(),
                        kind: DeclaredImportKind::Named(names.clone()),
                    }]
                })
                .unwrap_or_default();
            (
                code.clone(),
                map.as_ref().filter(|_| want_maps).cloned(),
                script_imports,
            )
        }
        None => {
            script_scaffold.push_str("const _sfc_main = {}\n");
            if !compiled.scope_id.is_empty() {
                let _ = writeln!(
                    script_scaffold,
                    "_sfc_main.__scopeId = \"{}\"",
                    compiled.scope_id
                );
            }
            (script_scaffold.clone(), None, Vec::new())
        }
    };
    let script_ends_with_newline = script_code.ends_with('\n');
    mint_and_validate(
        &mut fragments,
        canonical_id,
        "script",
        planned_kind,
        PlacementSlot::ModuleBody,
        dialect,
        script_code,
        script_source_map,
        script_imports,
    )?;

    let mut post_script = String::new();
    if !script_ends_with_newline {
        post_script.push('\n');
    }
    mint_and_validate(
        &mut fragments,
        canonical_id,
        "post_script",
        planned_kind,
        PlacementSlot::ModuleBody,
        dialect,
        post_script,
        None,
        Vec::new(),
    )?;

    // ── template ─────────────────────────────────────────────────────
    if let Some(template) = &compiled.template {
        let mut template_prelude = String::new();
        let mut template_prelude_imports: Vec<DeclaredImport> = Vec::new();
        if !template.imports.is_empty() {
            let _ = write!(template_prelude, "import {{ ");
            for (i, name) in template.imports.iter().enumerate() {
                if i > 0 {
                    template_prelude.push_str(", ");
                }
                template_prelude.push_str(&format_import_specifier(name));
            }
            let _ = writeln!(template_prelude, " }} from \"{}\"", runtime);
            template_prelude_imports.push(DeclaredImport {
                specifier: runtime.to_string(),
                kind: DeclaredImportKind::Named(template.imports.clone()),
            });
        }
        // SSR helpers are imported from "vue/server-renderer"
        if !template.ssr_imports.is_empty() {
            let _ = write!(template_prelude, "import {{ ");
            for (i, name) in template.ssr_imports.iter().enumerate() {
                if i > 0 {
                    template_prelude.push_str(", ");
                }
                template_prelude.push_str(&format_import_specifier(name));
            }
            let _ = writeln!(template_prelude, " }} from \"vue/server-renderer\"");
            template_prelude_imports.push(DeclaredImport {
                specifier: "vue/server-renderer".to_string(),
                kind: DeclaredImportKind::Named(template.ssr_imports.clone()),
            });
        }
        template_prelude.push('\n');
        mint_and_validate(
            &mut fragments,
            canonical_id,
            "template_prelude",
            planned_kind,
            PlacementSlot::ModulePrelude,
            dialect,
            template_prelude,
            None,
            template_prelude_imports,
        )?;

        let template_ends_with_newline = template.code.ends_with('\n');
        mint_and_validate(
            &mut fragments,
            canonical_id,
            "template",
            planned_kind,
            PlacementSlot::ModuleBody,
            dialect,
            template.code.clone(),
            template_map_json.clone(),
            Vec::new(),
        )?;

        let mut post_template = String::new();
        if !template_ends_with_newline {
            post_template.push('\n');
        }
        match template.render_export {
            TemplateRenderExport::SsrRender => {
                post_template.push_str("_sfc_main.ssrRender = ssrRender\n");
            }
            TemplateRenderExport::Render => {
                post_template.push_str("_sfc_main.render = render\n");
            }
        }
        mint_and_validate(
            &mut fragments,
            canonical_id,
            "post_template",
            planned_kind,
            PlacementSlot::ModuleBody,
            dialect,
            post_template,
            None,
            Vec::new(),
        )?;
    }

    // ── trailer: caller-owned decoration, then the terminal
    // `export default` ──────────────────────────────────────────────────
    let mut trailer = String::new();
    let mut trailer_imports: Vec<DeclaredImport> = Vec::new();
    for extra in &trailer_extra {
        trailer.push_str(&extra.code);
        trailer_imports.extend(extra.imports.iter().cloned());
    }
    trailer.push_str("export default _sfc_main");
    mint_and_validate(
        &mut fragments,
        canonical_id,
        "trailer",
        planned_kind,
        PlacementSlot::ModuleBody,
        dialect,
        trailer,
        None,
        trailer_imports,
    )?;

    let emitted_imports: Vec<DeclaredImport> = fragments
        .iter()
        .flat_map(|f| f.fragment().imports.iter().cloned())
        .collect();
    let sequenced = {
        let fragment_refs: Vec<&ValidatedFragment> = fragments.iter().collect();
        assemble_sequence(&fragment_refs, source_root)?
    };

    Ok(ComposedFragments {
        fragments,
        code: sequenced.code,
        source_map: sequenced.source_map.to_json_string(),
        emitted_imports,
    })
}

/// Compose AND publish a Main module in one call — the single-artifact
/// convenience for a caller that only ever publishes the one artifact it
/// composes. A caller publishing this artifact atomically alongside sibling
/// contributions from the same request (the direct one-shot core) uses
/// [`compose_fragments`] directly instead, so it can call [`publish`]
/// exactly once over the FULL contribution set.
///
/// # Errors
///
/// [`VueMainAssemblyFailure`] on any failure — a composition defect (see
/// [`compose_fragments`]) or a publication refusal. Never a panic.
pub fn compose_main_module(
    request: VueMainModuleRequest<'_>,
) -> Result<ArtifactSet, VueMainAssemblyFailure> {
    let planned_kind = request.planned_kind;
    let dialect = request.dialect;
    let want_maps = request.want_maps;

    let composed = compose_fragments(request)?;
    let fragment_refs: Vec<&ValidatedFragment> = composed.fragments.iter().collect();

    let plan = ProductPlan::single(PlannedArtifact {
        kind: planned_kind,
        requires_source_projection_map: false,
        requires_runtime_source_map: want_maps,
    });
    let contribution = ArtifactContribution {
        kind: planned_kind,
        fragments: fragment_refs,
        code: composed.code,
        emitted_imports: composed.emitted_imports,
        dialect,
        source_projection_map: None,
        runtime_source_map: want_maps.then_some(composed.source_map),
    };
    Ok(publish(&plan, vec![contribution])?)
}

/// Sole semantic Vue main-module assembler: map validation, dialect,
/// composition, and compile-artifact-set emission. Session adapters transport
/// identifiers only.
pub fn assemble_vue_runtime_main(
    request: VueRuntimeMainRequest<'_>,
) -> Result<VueRuntimeMainAssembled, VueMainAssemblyFailure> {
    let planned_kind = if request.ssr {
        ProductKind::RuntimeServer
    } else {
        ProductKind::RuntimeClient
    };
    let dialect = resolve_vue_main_dialect(request.script_lang, request.force_js);
    let validated = if request.source_map {
        Some(validate_vue_main_maps(
            request.compiled,
            request.has_script,
            request.has_template,
        )?)
    } else {
        None
    };
    let want_maps = validated.is_some();
    let source_root = validated
        .as_ref()
        .and_then(|inputs| inputs.source_root.clone());
    let script_map = validated
        .as_ref()
        .and_then(|inputs| inputs.script.as_ref())
        .map(to_source_map);
    let template_map_json: Option<String> = validated
        .as_ref()
        .and_then(|inputs| inputs.template.as_ref())
        .map(|map| to_source_map(map).to_json_string());

    record_vue_main_assembly();

    let composed = compose_fragments(VueMainModuleRequest {
        canonical_id: request.canonical_id,
        compiled: request.compiled,
        dialect,
        planned_kind,
        runtime: request.runtime,
        want_maps,
        source_root: source_root.as_deref(),
        script_map: script_map.as_ref(),
        template_map_json,
        decoration: request.decoration.clone(),
    })?;
    let fragment_refs: Vec<&ValidatedFragment> = composed.fragments.iter().collect();
    let plan = ProductPlan::single(PlannedArtifact {
        kind: planned_kind,
        requires_source_projection_map: false,
        requires_runtime_source_map: want_maps,
    });
    let source_map = want_maps.then(|| composed.source_map.clone());
    let contribution = ArtifactContribution {
        kind: planned_kind,
        fragments: fragment_refs,
        code: composed.code.clone(),
        emitted_imports: composed.emitted_imports,
        dialect,
        source_projection_map: None,
        runtime_source_map: source_map.clone(),
    };
    publish(&plan, vec![contribution]).map_err(VueMainAssemblyFailure::from)?;
    let code: Arc<str> = Arc::from(composed.code);
    let artifacts = vue_main_compile_artifacts(
        request.canonical_id,
        request.compiled,
        planned_kind,
        dialect,
        Arc::clone(&code),
        source_map.as_deref(),
        &request.decoration,
        want_maps,
        request.source,
        request.custom_blocks,
    )
    .map_err(|err| {
        VueMainAssemblyFailure::Publication(AssemblyRefusal::FinalParseFailed {
            kind: planned_kind,
            reason: err.to_string(),
        })
    })?;
    Ok(VueRuntimeMainAssembled {
        code,
        source_map,
        lang: dialect.lang_id().to_string(),
        artifacts,
    })
}

fn resolve_vue_main_dialect(script_lang: Option<&str>, force_js: bool) -> FragmentDialect {
    let raw = script_lang.unwrap_or("js");
    let is_tsx = raw.eq_ignore_ascii_case("tsx");
    let is_jsx = is_tsx || raw.eq_ignore_ascii_case("jsx");
    let is_ts = is_tsx || raw.eq_ignore_ascii_case("ts");
    if force_js {
        if is_jsx {
            FragmentDialect::Jsx
        } else {
            FragmentDialect::JavaScript
        }
    } else if is_tsx {
        FragmentDialect::Tsx
    } else if is_jsx {
        FragmentDialect::Jsx
    } else if is_ts {
        FragmentDialect::TypeScript
    } else {
        FragmentDialect::JavaScript
    }
}

struct ValidatedVueMainMaps {
    script: Option<DecodedFragmentMap>,
    template: Option<DecodedFragmentMap>,
    source_root: Option<String>,
}

fn validate_vue_main_maps(
    compiled: &RuntimeCompileOutput,
    has_script: bool,
    has_template: bool,
) -> Result<ValidatedVueMainMaps, AssembleMapFailure> {
    let script_required = has_script && compiled.script.is_some();
    let template_required = has_template && compiled.template.is_some();

    if script_required
        && compiled
            .script
            .as_ref()
            .is_some_and(|script| script.source_map.is_empty())
    {
        return Err(AssembleMapFailure::MissingRequiredInputMap {
            fragment: MapFragment::Script,
        });
    }
    if template_required
        && compiled
            .template
            .as_ref()
            .is_some_and(|template| template.source_map.is_empty())
    {
        return Err(AssembleMapFailure::MissingRequiredInputMap {
            fragment: MapFragment::Template,
        });
    }

    let script = match &compiled.script {
        Some(script) if !script.source_map.is_empty() => Some(
            validate_and_decode(&script.source_map, &script.code).map_err(|code| {
                AssembleMapFailure::UncomposableInputMap {
                    fragment: MapFragment::Script,
                    code,
                }
            })?,
        ),
        _ => None,
    };
    let template = match &compiled.template {
        Some(template) if !template.source_map.is_empty() => Some(
            validate_and_decode(&template.source_map, &template.code).map_err(|code| {
                AssembleMapFailure::UncomposableInputMap {
                    fragment: MapFragment::Template,
                    code,
                }
            })?,
        ),
        _ => None,
    };

    let source_root = agree_source_root(
        script
            .iter()
            .map(|map| (MapFragment::Script, map))
            .chain(template.iter().map(|map| (MapFragment::Template, map))),
    )?;

    Ok(ValidatedVueMainMaps {
        script,
        template,
        source_root,
    })
}

pub(crate) struct VueAssemblyTag<'a>(pub(crate) &'a str);
impl CanonicalEncode for VueAssemblyTag<'_> {
    const DOMAIN_TAG: &'static str = "verter.compiler.vue.main.assembly.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.0);
    }
}

fn content_digest(bytes: &[u8]) -> [u8; 32] {
    *ContentId::from_content_bytes(bytes).digest().as_bytes()
}

struct VueMainInputBasis<'a> {
    canonical_id: &'a str,
    hmr: &'a str,
    is_production: bool,
    emit_ssr: bool,
    ssr_module_id: &'a str,
    want_maps: bool,
    kind: &'a str,
    order: &'a str,
    script_digest: [u8; 32],
    template_digest: [u8; 32],
    main_digest: [u8; 32],
    map_digest: [u8; 32],
}
impl CanonicalEncode for VueMainInputBasis<'_> {
    const DOMAIN_TAG: &'static str = "verter.compiler.vue.main.input_basis.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.canonical_id);
        e.field_str(2, self.hmr);
        e.field_bool(3, self.is_production);
        e.field_bool(4, self.emit_ssr);
        e.field_str(5, self.ssr_module_id);
        e.field_bool(6, self.want_maps);
        e.field_str(7, self.kind);
        e.field_str(8, self.order);
        e.field_bytes(9, &self.script_digest);
        e.field_bytes(10, &self.template_digest);
        e.field_bytes(11, &self.main_digest);
        e.field_bytes(12, &self.map_digest);
    }
}

/// Compile-artifact schema for one assembled Vue main artifact and its
/// contributing script/template units. Relations are typed; maps are
/// qualified by family. Construction performs no compile work.
///
/// `custom_blocks` (this compile's own retained parse facts) mint one
/// source-backed [`CustomBlockDescriptor`] per block, sharing this call's
/// own `source_id`/revision and attached to an `"sfc"` unit/artifact added
/// to the SAME set Main publishes — never a second, unrelated lineage.
/// Empty `custom_blocks` is zero extra work.
#[allow(clippy::too_many_arguments)]
pub fn vue_main_compile_artifacts(
    canonical_id: &str,
    compiled: &RuntimeCompileOutput,
    kind: ProductKind,
    dialect: FragmentDialect,
    code: impl Into<Arc<str>>,
    runtime_source_map: Option<&str>,
    decoration: &VueMainDecoration,
    want_maps: bool,
    source: &str,
    custom_blocks: &[crate::compile::VerterCustomBlock],
) -> Result<CompileArtifactSet, super::publish::ArtifactSchemaError> {
    let code = code.into();
    use std::collections::BTreeSet;

    let script_bytes = compiled
        .script
        .as_ref()
        .map(|s| s.code.as_bytes())
        .unwrap_or(&[]);
    let template_bytes = compiled
        .template
        .as_ref()
        .map(|t| t.code.as_bytes())
        .unwrap_or(&[]);
    let mut order = String::new();
    if compiled.script.is_some() {
        order.push_str("script");
    }
    if compiled.template.is_some() {
        if !order.is_empty() {
            order.push(',');
        }
        order.push_str("template");
    }
    let map_json = runtime_source_map.unwrap_or("");
    let ssr_module_id = decoration.ssr_module_id.as_deref().unwrap_or(canonical_id);
    let basis = VueMainInputBasis {
        canonical_id,
        hmr: decoration.hmr.wire_name(),
        is_production: decoration.is_production,
        emit_ssr: decoration.emit_ssr_module_registration,
        ssr_module_id,
        want_maps,
        kind: kind.wire_tag(),
        order: &order,
        script_digest: content_digest(script_bytes),
        template_digest: content_digest(template_bytes),
        main_digest: content_digest(code.as_bytes()),
        map_digest: content_digest(map_json.as_bytes()),
    };
    let source_id = SourceId::from_canonical(&VueAssemblyTag(canonical_id));
    let revision = SourceRevision::from_canonical(&basis);
    let mut source_units = Vec::new();
    let mut inputs = BTreeSet::new();

    let mut push_unit = |role: &str, bytes: &[u8], span: verter_span::Span| {
        let unit = SourceUnit::mint(
            source_id.clone(),
            revision.clone(),
            role,
            ContentId::from_content_bytes(bytes),
        );
        inputs.insert(unit.id().clone());
        source_units.push(ArtifactSourceUnit {
            source_span: span,
            unit,
        });
    };
    push_unit(
        "main",
        code.as_bytes(),
        verter_span::Span::new(0, code.len() as u32),
    );

    let authored = authored_contribution_units(compiled, runtime_source_map);
    for unit in &authored {
        push_unit(&unit.role, unit.content.as_bytes(), unit.span);
    }

    let producer = ResultContractId::from_canonical(&VueAssemblyTag("vue-runtime-main"));
    let input_basis = InputBasisId::from_canonical(&basis);
    let mut artifacts = Vec::new();
    let mut main = CompileArtifact::new(
        source_units[0].unit.id().clone(),
        kind,
        dialect.schema_language(),
        "main",
        ArtifactProvenance {
            input_basis: input_basis.clone(),
            producer,
            inputs: inputs.clone(),
        },
        ArtifactContent::Available(Arc::clone(&code)),
    );
    for role in ["script", "template"] {
        let Some(unit) = source_units.iter().find(|u| u.unit.logical_role() == role) else {
            continue;
        };
        let fragment = CompileArtifact::new(
            unit.unit.id().clone(),
            kind,
            dialect.schema_language(),
            role,
            ArtifactProvenance {
                input_basis: input_basis.clone(),
                producer: ResultContractId::from_canonical(&VueAssemblyTag(if role == "script" {
                    "vue-runtime-script"
                } else {
                    "vue-runtime-template"
                })),
                inputs: BTreeSet::from([unit.unit.id().clone()]),
            },
            ArtifactContent::Unavailable(super::fragment::ArtifactUnavailableReason::NotProduced),
        );
        main.relations.insert(ArtifactRelation {
            kind: ArtifactRelationKind::DependsOn,
            target: fragment.id().clone(),
        });
        artifacts.push(fragment);
    }
    if let Some(map_json) = runtime_source_map {
        let authored_ids: BTreeSet<_> = source_units
            .iter()
            .filter(|unit| unit.unit.logical_role() != "main")
            .map(|unit| unit.unit.id().clone())
            .collect();
        if !authored_ids.is_empty() {
            main.maps.push(QualifiedArtifactMap {
                family: ArtifactMapFamily::RuntimeSourceMap,
                generated: main.id().clone(),
                generated_content: ContentId::from_content_bytes(code.as_bytes()),
                input_basis: input_basis.clone(),
                sources: authored_ids,
                segments: runtime_map_segments(map_json, code.as_ref(), &source_units, &authored),
            });
        }
    }
    artifacts.insert(0, main);

    // The custom-block producer cell: an `"sfc"` unit spanning the full
    // authored SFC bytes, sharing this call's own `source_id`/`revision` —
    // the same lineage Main publishes under, never a second private set.
    // Absent blocks are zero-work (no unit, no artifact, no descriptor).
    let sfc = (!custom_blocks.is_empty()).then(|| {
        let sfc_unit = SourceUnit::mint(
            source_id.clone(),
            revision.clone(),
            "sfc",
            ContentId::from_content_bytes(source.as_bytes()),
        );
        let sfc_artifact = CompileArtifact::new(
            sfc_unit.id().clone(),
            ProductKind::Analysis,
            verter_language::LanguageId::new("vue"),
            "sfc",
            ArtifactProvenance {
                input_basis: input_basis.clone(),
                producer: ResultContractId::from_canonical(&VueAssemblyTag(
                    "vue-runtime-custom-blocks",
                )),
                inputs: BTreeSet::from([sfc_unit.id().clone()]),
            },
            ArtifactContent::Unavailable(super::fragment::ArtifactUnavailableReason::NotProduced),
        );
        (sfc_unit, sfc_artifact)
    });
    if let Some((sfc_unit, sfc_artifact)) = &sfc {
        source_units.push(ArtifactSourceUnit {
            unit: sfc_unit.clone(),
            source_span: verter_span::Span::new(0, source.len() as u32),
        });
        artifacts.push(sfc_artifact.clone());
    }

    let set = CompileArtifactSet::new(source_units, artifacts)?;
    let Some((sfc_unit, sfc_artifact)) = sfc else {
        return Ok(set);
    };
    let source_content = ContentId::from_content_bytes(source.as_bytes());
    let block_producer =
        ResultContractId::from_canonical(&VueAssemblyTag("vue-runtime-custom-blocks"));
    let requests: Vec<CustomBlockDescriptorRequest> = custom_blocks
        .iter()
        .map(|block| {
            let content = if block.src.is_some() {
                CustomBlockContent::SrcBacked
            } else if block.content.is_empty() {
                CustomBlockContent::Empty
            } else {
                CustomBlockContent::Local {
                    content: ContentId::from_content_bytes(block.content.as_bytes()),
                    text: block.content.clone(),
                }
            };
            CustomBlockDescriptorRequest {
                source_unit: sfc_unit.id().clone(),
                source_id: source_id.clone(),
                revision: revision.clone(),
                source_content: source_content.clone(),
                role: block.block_type.clone(),
                lang: block.lang.clone(),
                src: block.src.clone(),
                attributes: block.attrs.clone(),
                source_order: block.source_order,
                region: block.region,
                content,
                provenance: ArtifactProvenance {
                    input_basis: input_basis.clone(),
                    producer: block_producer.clone(),
                    inputs: BTreeSet::from([sfc_unit.id().clone()]),
                },
                attached_to: sfc_artifact.id().clone(),
                lifecycle: CustomBlockLifecycle::Complete,
            }
        })
        .collect();
    let descriptors = requests
        .into_iter()
        .map(CustomBlockDescriptor::try_new)
        .collect::<Result<Vec<_>, _>>()
        .map_err(super::publish::ArtifactSchemaError::CustomBlockInvalid)?;
    set.attach_custom_blocks(descriptors)
        .map_err(super::publish::ArtifactSchemaError::CustomBlockInvalid)
}

struct AuthoredContribution {
    role: String,
    content: String,
    span: verter_span::Span,
    /// Composed-map source-row index for this contribution. Script and
    /// template maps can share a source name and overlapping local spans;
    /// tokens bind through this identity, not the name or span alone.
    composed_source_id: u32,
}

fn fragment_source_row_count(map_json: &str) -> u32 {
    if map_json.is_empty() {
        return 0;
    }
    SourceMap::from_json_string(map_json)
        .map(|map| map.get_sources().count() as u32)
        .unwrap_or(0)
}

fn authored_contribution_units(
    compiled: &RuntimeCompileOutput,
    runtime_source_map: Option<&str>,
) -> Vec<AuthoredContribution> {
    let mut units = Vec::new();
    let mut source_base = 0u32;
    let mut push_fragment = |role: &str, map_json: &str| {
        let count = fragment_source_row_count(map_json);
        if let Some(mut unit) = authored_unit_from_map(role, map_json) {
            unit.composed_source_id = source_base.saturating_add(unit.composed_source_id);
            units.push(unit);
        }
        source_base = source_base.saturating_add(count);
    };
    if let Some(script) = &compiled.script {
        push_fragment("script", &script.source_map);
    }
    if let Some(template) = &compiled.template {
        push_fragment("template", &template.source_map);
    }
    if units.is_empty() {
        if let Some(map_json) = runtime_source_map {
            if let Some(unit) = authored_unit_from_map("sfc", map_json) {
                units.push(unit);
            }
        }
    }
    units
}

fn authored_unit_from_map(role: &str, map_json: &str) -> Option<AuthoredContribution> {
    if map_json.is_empty() {
        return None;
    }
    let map = SourceMap::from_json_string(map_json).ok()?;
    let (source_id, content) = map.get_sources().enumerate().find_map(|(index, _)| {
        let id = index as u32;
        map.get_source_content(id)
            .filter(|bytes| !bytes.is_empty())
            .map(|bytes| (id, bytes.to_string()))
    })?;
    let line_starts = generated_line_starts(&content);
    let mut start = u32::MAX;
    let mut end = 0u32;
    for token in map.get_tokens() {
        if token.get_source_id() != Some(source_id) {
            continue;
        }
        let Some(offset) = utf16_offset_to_bytes(
            &content,
            &line_starts,
            token.get_src_line(),
            token.get_src_col(),
        ) else {
            continue;
        };
        start = start.min(offset);
        end = end.max(next_char_end(&content, offset));
    }
    if start == u32::MAX || start > end {
        start = 0;
        end = content.len() as u32;
    }
    let end = end.min(content.len() as u32);
    Some(AuthoredContribution {
        role: role.to_string(),
        content,
        span: verter_span::Span::new(start, end),
        composed_source_id: source_id,
    })
}

fn runtime_map_segments(
    map_json: &str,
    generated: &str,
    source_units: &[ArtifactSourceUnit],
    authored: &[AuthoredContribution],
) -> Vec<ArtifactMapSegment> {
    let Ok(map) = SourceMap::from_json_string(map_json) else {
        return Vec::new();
    };
    if authored.is_empty() {
        return Vec::new();
    }
    let source_contents: Vec<Option<String>> = map
        .get_source_contents()
        .map(|content| content.map(str::to_string))
        .collect();
    let source_line_starts: Vec<Option<Vec<u32>>> = source_contents
        .iter()
        .map(|content| content.as_deref().map(generated_line_starts))
        .collect();
    let gen_starts = generated_line_starts(generated);
    let gen_len = generated.len() as u32;
    let mut segments: Vec<ArtifactMapSegment> = map
        .get_tokens()
        .filter_map(|token| {
            let source_id = token.get_source_id()?;
            let content = source_contents.get(source_id as usize)?.as_deref()?;
            let line_starts = source_line_starts.get(source_id as usize)?.as_ref()?;
            let source_start = utf16_offset_to_bytes(
                content,
                line_starts,
                token.get_src_line(),
                token.get_src_col(),
            )?;
            let source_end = next_char_end(content, source_start);
            if source_start > source_end {
                return None;
            }
            let contribution = authored.iter().find(|unit| {
                token.get_source_id() == Some(unit.composed_source_id)
                    && source_start >= unit.span.start
                    && source_end <= unit.span.end
            })?;
            let unit = source_units
                .iter()
                .find(|candidate| candidate.unit.logical_role() == contribution.role)?;
            let start = utf16_offset_to_bytes(
                generated,
                &gen_starts,
                token.get_dst_line(),
                token.get_dst_col(),
            )?;
            if start >= gen_len {
                return None;
            }
            let end = next_char_end(generated, start).min(gen_len);
            generated.get(start as usize..end as usize)?;
            Some(ArtifactMapSegment {
                generated: start..end,
                source_unit: unit.unit.id().clone(),
                source_span: verter_span::Span::new(source_start, source_end),
            })
        })
        .collect();
    segments.sort_by_key(|segment| (segment.generated.start, segment.generated.end));
    segments.dedup_by(|a, b| a.generated.start == b.generated.start);
    let mut kept = Vec::with_capacity(segments.len());
    for segment in segments {
        if kept
            .last()
            .is_none_or(|prev: &ArtifactMapSegment| prev.generated.end <= segment.generated.start)
        {
            kept.push(segment);
        }
    }
    kept
}

fn generated_line_starts(code: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    for (index, byte) in code.bytes().enumerate() {
        if byte == b'\n' {
            starts.push((index + 1) as u32);
        }
    }
    starts
}

fn utf16_offset_to_bytes(code: &str, line_starts: &[u32], line: u32, column: u32) -> Option<u32> {
    let start = *line_starts.get(line as usize)? as usize;
    let end = line_starts
        .get(line as usize + 1)
        .map(|next| *next as usize)
        .unwrap_or(code.len());
    let line_bytes = code.get(start..end)?;
    let mut utf16 = 0u32;
    for (byte_off, ch) in line_bytes.char_indices() {
        if utf16 == column {
            return Some((start + byte_off) as u32);
        }
        utf16 = utf16.saturating_add(ch.len_utf16() as u32);
        if utf16 > column {
            return None;
        }
    }
    if utf16 == column {
        Some((start + line_bytes.len()) as u32)
    } else {
        None
    }
}

fn next_char_end(code: &str, start: u32) -> u32 {
    let start = start as usize;
    let Some(rest) = code.get(start..) else {
        return start as u32;
    };
    match rest.chars().next() {
        Some(ch) => (start + ch.len_utf8()) as u32,
        None => start as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `rewrite_script` applies ONLY the ranges a producer-declared
    /// `SfcExportPlacement` fact names — never a scan for the `__sfc__` /
    /// `export default` landmark strings. Every declared binding is renamed;
    /// the declared export statement (including its own internal binding) is
    /// removed wholesale, not separately renamed then removed.
    /// `authored_text_matching_the_landmarks_is_left_untouched` below is the
    /// companion collision proof: an UNDECLARED occurrence of either landmark
    /// string is left untouched.
    #[test]
    fn rewrite_applies_only_the_declared_ranges() {
        // A binding NOT part of the export statement, plus the export
        // statement's own internal binding — the general two-binding shape a
        // real producer declares.
        let code = "const __sfc__ = {}\n__sfc__.__scopeId = \"x\";\nexport default __sfc__;\n";
        let first_start = code.find("__sfc__").unwrap() as u32;
        let first = first_start..first_start + 7;
        let second_start = code[first.end as usize..].find("__sfc__").unwrap() as u32 + first.end;
        let second = second_start..second_start + 7;
        let export_start = code.find("export default").unwrap() as u32;
        let export = export_start..code.len() as u32;
        let export_binding = export_start + "export default ".len() as u32;
        let export_binding = export_binding..export_binding + 7;
        let fact = SfcExportPlacement {
            binding_ranges: vec![first, second, export_binding],
            export_statement_range: Some(export),
        };
        let (rewritten, _) = rewrite_script(code, Some(&fact), None)
            .expect("a fact whose declared ranges match the script's own bytes is accepted");
        assert_eq!(
            rewritten, "const _sfc_main = {}\n_sfc_main.__scopeId = \"x\";\n",
            "every declared binding is renamed, and the declared export statement \
             (including its own internal binding) is removed wholesale, not \
             separately renamed then removed"
        );
    }

    /// Authored source text containing the literal strings `__sfc__` or
    /// `export default _sfc_main` is left untouched when no fact declares it as
    /// a rename/removal target — `rewrite_script` acts only on declared ranges,
    /// never on an incidental text match.
    #[test]
    fn authored_text_matching_the_landmarks_is_left_untouched() {
        let code = "const __sfc__ = {}\n\
                     const decoy = \"__sfc__\";\n\
                     const other = \"export default _sfc_main;\";\n\
                     export default __sfc__;\n";
        let binding = 6..13; // the ONLY declared binding: the real one.
        let export_start = code.rfind("export default __sfc__;\n").unwrap() as u32;
        let export = export_start..export_start + "export default __sfc__;\n".len() as u32;
        let export_binding_start = export_start + "export default ".len() as u32;
        let export_binding = export_binding_start..export_binding_start + 7;
        let fact = SfcExportPlacement {
            binding_ranges: vec![binding, export_binding],
            export_statement_range: Some(export),
        };
        let (rewritten, _) = rewrite_script(code, Some(&fact), None)
            .expect("a fact whose declared ranges match the script's own bytes is accepted");
        assert!(
            rewritten.contains("const decoy = \"__sfc__\";"),
            "an UNDECLARED `__sfc__` occurrence inside authored text must survive \
             verbatim, got:\n{rewritten}"
        );
        assert!(
            rewritten.contains("const other = \"export default _sfc_main;\";"),
            "an UNDECLARED `export default _sfc_main` occurrence inside authored \
             text must survive verbatim, got:\n{rewritten}"
        );
        assert!(
            rewritten.contains("const _sfc_main = {}"),
            "the DECLARED binding is still renamed, got:\n{rewritten}"
        );
        assert!(
            !rewritten.contains("export default _sfc_main;\n\n") && rewritten.ends_with('\n'),
            "the DECLARED export statement is still removed, got:\n{rewritten}"
        );
    }

    /// A declared range whose bytes do not match the fact's claim (a producer
    /// defect) is a typed refusal — never silently rescanned or half-applied.
    #[test]
    fn inconsistent_declared_range_is_a_typed_refusal() {
        let code = "const __sfc__ = {}\n";
        let wrong_range = 0..7; // "const _" — not "__sfc__"
        let fact = SfcExportPlacement {
            binding_ranges: vec![wrong_range],
            export_statement_range: None,
        };
        let err = rewrite_script(code, Some(&fact), None).unwrap_err();
        assert!(
            matches!(
                err,
                SfcRewriteRefusal::InconsistentBindingRange { start: 0, end: 7 }
            ),
            "got {err:?}"
        );
    }

    /// A missing fact (`None`) is indistinguishable, without scanning, from a
    /// genuinely empty declared fact — `rewrite_script` treats it the same:
    /// zero edits, `code` returned verbatim. Never a refusal (a script with no
    /// `__sfc__` at all is legitimate — e.g. a fixture built purely to exercise
    /// unrelated map-composition mechanics) and never a scan to find out which
    /// case it is.
    #[test]
    fn missing_fact_is_treated_as_an_empty_fact() {
        let code = "const x = 1\n";
        let (rewritten, _) =
            rewrite_script(code, None, None).expect("a missing fact is never itself a refusal");
        assert_eq!(rewritten, code);
    }

    /// `rewrite_script` chains its own overwrite-only transform onto the
    /// caller-supplied script map (`CodeTransform::chain_source_map`), which
    /// genuinely returns failures for a map whose declared generated position
    /// does not exist in the transform's own text. `rewrite_script`'s own
    /// signature does not, and must not, assume its `map` argument was
    /// already validated against `code` — this proves the typed refusal fires
    /// (not a panic) when that invariant is broken directly.
    #[test]
    fn chain_source_map_failure_is_a_typed_refusal_not_a_panic() {
        let code = "const x = 1\n"; // one line, plus the trailing-newline empty line
                                    // `code` has only lines 0 and 1 (the trailing empty line); line 99
                                    // names a generated position `chain_source_map`'s own text tiling
                                    // cannot resolve.
        let token = oxc_sourcemap::Token::new(99, 0, 0, 0, None, None);
        let map = SourceMap::new(
            None,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Box::new([token]),
            None,
        );
        match rewrite_script(code, None, Some(&map)) {
            Err(SfcRewriteRefusal::ChainFailed(_)) => {}
            other => panic!("expected a typed ChainFailed refusal, got: {other:?}"),
        }
    }

    /// The chained script map carries BOTH passes, composed in sequence.
    ///
    /// The two authorized rewrites must be applied SEQUENTIALLY — pass two on
    /// pass one's output coordinate space. The failure mode that requirement
    /// exists to exclude is a chain whose second pass is applied to the ORIGINAL
    /// fragment map instead of pass one's output: the code still comes out right,
    /// because the code is built by the transforms themselves, but the map silently
    /// loses pass one's contribution.
    ///
    /// The fixture makes both contributions observable in one sequence, at
    /// coordinates that differ under the defect:
    ///
    /// - the rename is two bytes longer than what it replaces, so an authored
    ///   position after it on line 0 lands at generated column 16; chained over the
    ///   original map it would land at 14, its unshifted column;
    /// - the removal deletes line 1 entirely, so the authored position on line 2
    ///   lands on generated line 1; a chain that never ran pass two would leave it
    ///   on line 2.
    ///
    /// Asserting both in one result is what pins the composition rather than either
    /// pass alone.
    #[test]
    fn the_chained_script_map_carries_both_rewrite_passes_in_sequence() {
        let code = "const __sfc__ = {}\nexport default __sfc__;\nconst z = 2\n";
        // Column 14 is the `=` of `const __sfc__ = {}`, i.e. the first authored
        // position AFTER the identifier pass one rewrites.
        let tokens = [
            oxc_sourcemap::Token::new(0, 0, 1, 0, Some(0), None),
            oxc_sourcemap::Token::new(0, 14, 1, 14, Some(0), None),
            oxc_sourcemap::Token::new(2, 0, 3, 0, Some(0), None),
        ];
        let map = SourceMap::new(
            None,
            Vec::new(),
            None,
            vec!["Comp.vue".into()],
            Vec::new(),
            Box::new(tokens),
            None,
        );
        let export_start = code.find("export default __sfc__;\n").unwrap() as u32;
        let fact = SfcExportPlacement {
            binding_ranges: vec![
                6..13,
                export_start + "export default ".len() as u32
                    ..export_start + "export default ".len() as u32 + 7,
            ],
            export_statement_range: Some(
                export_start..export_start + "export default __sfc__;\n".len() as u32,
            ),
        };

        let (rewritten, chained) =
            rewrite_script(code, Some(&fact), Some(&map)).expect("the fact matches the fixture");
        let chained = chained.expect("a contributing map produces a chained sequence");
        let chained = SourceMap::from_json_string(&chained)
            .expect("rewrite_script's own re-encoded chain is composable");

        assert_eq!(
            rewritten, "const _sfc_main = {}\nconst z = 2\n",
            "both passes must have run on the bytes"
        );

        let at = |line: u32, column: u32| {
            chained
                .get_tokens()
                .any(|token| token.get_dst_line() == line && token.get_dst_col() == column)
        };

        assert!(
            at(0, 16),
            "pass one's rename must be carried into the chained map: the authored \
             position at original column 14 belongs at generated column 16. Chained \
             over the ORIGINAL fragment map it would sit at 14."
        );
        assert!(
            !at(0, 14),
            "generated column 14 is pass one's INPUT column; its presence means the \
             second pass was chained over the original map rather than over pass \
             one's output."
        );
        assert!(
            at(1, 0),
            "pass two's removal must be carried too: the authored position on \
             original line 2 belongs on generated line 1."
        );
        assert!(
            chained.get_tokens().all(|token| token.get_dst_line() < 2),
            "the rewritten script has two lines, so no chained segment may remain on \
             line 2 — one that does means the removal never reached the map."
        );
    }

    fn assembled(
        compiled: &RuntimeCompileOutput,
        decoration: VueMainDecoration,
        kind: ProductKind,
    ) -> String {
        let set = compose_main_module(VueMainModuleRequest {
            canonical_id: "Comp.vue",
            compiled,
            dialect: FragmentDialect::JavaScript,
            planned_kind: kind,
            runtime: "vue",
            want_maps: false,
            source_root: None,
            script_map: None,
            template_map_json: None,
            decoration,
        })
        .expect("assembly with maps disabled cannot fail");
        set.artifact(kind)
            .expect("publish returns the planned artifact")
            .code()
            .to_string()
    }

    fn empty_bundle() -> RuntimeCompileOutput {
        RuntimeCompileOutput::default()
    }

    /// Host identifiers drive style/custom imports; the compiler owns the
    /// import shape. Empty identifiers emit no host topology.
    #[test]
    fn decoration_emits_host_identifiers_and_skips_them_when_absent() {
        use crate::framework_common::{
            RuntimeCustomBlock, RuntimeOutputDescriptor, RuntimeStyleBlock, SourceMapFidelity,
        };
        let style = RuntimeStyleBlock {
            code: ".x{}".to_string(),
            source_map: None,
            lang: Some("css".to_string()),
            scope_hash: None,
            has_global: false,
            output_descriptor: RuntimeOutputDescriptor::generated(
                ".x{}",
                None,
                &[("test:space", "test:artifact")],
                SourceMapFidelity::Approximate,
            ),
        };
        let compiled = RuntimeCompileOutput {
            styles: vec![style],
            custom_blocks: vec![RuntimeCustomBlock {
                block_type: "i18n".to_string(),
                content: "{}".to_string(),
            }],
            ..empty_bundle()
        };
        let bare = assembled(
            &compiled,
            VueMainDecoration::default(),
            ProductKind::RuntimeClient,
        );
        assert!(
            !bare.contains("import \""),
            "empty host identifiers must not invent style/custom imports:\n{bare}"
        );
        let decorated = assembled(
            &compiled,
            VueMainDecoration {
                style_specifiers: vec!["Comp.vue?vue&type=style&index=0&lang.css".to_string()],
                custom_specifiers: vec!["Comp.vue?vue&type=i18n&index=0".to_string()],
                ..VueMainDecoration::default()
            },
            ProductKind::RuntimeClient,
        );
        assert!(
            decorated.contains("import \"Comp.vue?vue&type=style&index=0&lang.css\""),
            "style specifier must appear as a side-effect import:\n{decorated}"
        );
        assert!(
            decorated.contains("import block0 from \"Comp.vue?vue&type=i18n&index=0\""),
            "custom specifier must appear as a default import:\n{decorated}"
        );
        assert!(
            decorated.contains("if (typeof block0 === 'function') block0(_sfc_main)"),
            "custom invocation is compiler-owned topology:\n{decorated}"
        );
    }

    /// HMR / `__file` / SSR registration are compiler-owned, driven by
    /// decoration axes — never reconstructed by scanning assembled bytes.
    #[test]
    fn decoration_owns_hmr_file_and_ssr_registration() {
        let compiled = empty_bundle();
        let vite = assembled(
            &compiled,
            VueMainDecoration {
                hmr: RuntimeHmrStrategy::Vite,
                ..VueMainDecoration::default()
            },
            ProductKind::RuntimeClient,
        );
        assert!(vite.contains("_sfc_main.__file = \"Comp.vue\""));
        assert!(vite.contains("import.meta.hot"));
        let ssr = assembled(
            &compiled,
            VueMainDecoration {
                ssr_module_id: Some("src/Comp.vue".to_string()),
                ..VueMainDecoration::default()
            },
            ProductKind::RuntimeServer,
        );
        assert!(ssr.contains("useSSRContext as __vite_useSSRContext"));
        assert!(ssr.contains(".add(\"src/Comp.vue\")"));
        assert!(!ssr.contains("import.meta.hot"));
        let artifacts = vue_main_compile_artifacts(
            "Comp.vue",
            &compiled,
            ProductKind::RuntimeClient,
            FragmentDialect::JavaScript,
            vite.as_str(),
            None,
            &VueMainDecoration {
                hmr: RuntimeHmrStrategy::Vite,
                ..VueMainDecoration::default()
            },
            false,
            "",
            &[],
        )
        .expect("schema accepts the assembled main");
        assert_eq!(artifacts.artifacts().len(), 1);
        assert!(artifacts.artifacts().iter().any(|a| a.name() == "main"));
    }

    #[test]
    fn main_artifact_identity_binds_revision_options_and_restores_on_revert() {
        let compiled = empty_bundle();
        let vite = VueMainDecoration {
            hmr: RuntimeHmrStrategy::Vite,
            ..VueMainDecoration::default()
        };
        let none = VueMainDecoration::default();
        let code = assembled(&compiled, vite.clone(), ProductKind::RuntimeClient);
        let first = vue_main_compile_artifacts(
            "Comp.vue",
            &compiled,
            ProductKind::RuntimeClient,
            FragmentDialect::JavaScript,
            code.as_str(),
            None,
            &vite,
            false,
            "",
            &[],
        )
        .expect("schema accepts");
        let option_changed = vue_main_compile_artifacts(
            "Comp.vue",
            &compiled,
            ProductKind::RuntimeClient,
            FragmentDialect::JavaScript,
            code.as_str(),
            None,
            &none,
            false,
            "",
            &[],
        )
        .expect("schema accepts");
        let edited = format!("{code}\n");
        let after_edit = vue_main_compile_artifacts(
            "Comp.vue",
            &compiled,
            ProductKind::RuntimeClient,
            FragmentDialect::JavaScript,
            edited.as_str(),
            None,
            &vite,
            false,
            "",
            &[],
        )
        .expect("schema accepts");
        let reverted = vue_main_compile_artifacts(
            "Comp.vue",
            &compiled,
            ProductKind::RuntimeClient,
            FragmentDialect::JavaScript,
            code.as_str(),
            None,
            &vite,
            false,
            "",
            &[],
        )
        .expect("schema accepts");
        let first_main = first
            .artifacts()
            .iter()
            .find(|a| a.name() == "main")
            .expect("main");
        let option_main = option_changed
            .artifacts()
            .iter()
            .find(|a| a.name() == "main")
            .expect("main");
        let edited_main = after_edit
            .artifacts()
            .iter()
            .find(|a| a.name() == "main")
            .expect("main");
        let reverted_main = reverted
            .artifacts()
            .iter()
            .find(|a| a.name() == "main")
            .expect("main");
        assert_ne!(
            first_main.provenance.input_basis, option_main.provenance.input_basis,
            "option change must change input basis"
        );
        let first_rev = first
            .source_units()
            .find(|u| u.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .revision()
            .clone();
        let edited_rev = after_edit
            .source_units()
            .find(|u| u.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .revision()
            .clone();
        let reverted_rev = reverted
            .source_units()
            .find(|u| u.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .revision()
            .clone();
        let first_content = first
            .source_units()
            .find(|u| u.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .content()
            .clone();
        assert_ne!(first_rev, edited_rev, "edit must change source revision");
        assert_eq!(
            first_main.provenance.input_basis, reverted_main.provenance.input_basis,
            "revert must restore input basis"
        );
        assert_eq!(first_rev, reverted_rev, "revert must restore revision");
        assert_eq!(
            first_content,
            ContentId::from_content_bytes(code.as_bytes()),
            "main unit content is the assembled bytes"
        );
        let _ = edited_main;
    }

    #[test]
    fn qualified_runtime_map_binds_authored_units_not_generated_main() {
        use crate::framework_common::{
            RuntimeOutputDescriptor, RuntimeScriptBlock, SourceMapFidelity,
        };
        let script_map = r#"{"version":3,"file":"Comp.vue","sources":["Comp.vue"],"sourcesContent":["const n = 1\n"],"names":[],"mappings":"AAAA"}"#;
        let compiled = RuntimeCompileOutput {
            script: Some(RuntimeScriptBlock {
                code: "const n = 1\n".to_string(),
                source_map: script_map.to_string(),
                setup: false,
                output_descriptor: RuntimeOutputDescriptor::generated(
                    "const n = 1\n",
                    Some(script_map),
                    &[("test:space", "test:artifact")],
                    SourceMapFidelity::Approximate,
                ),
                generated_template_hole: None,
                runtime_imports: Vec::new(),
                sfc_export_placement: None,
            }),
            ..empty_bundle()
        };
        let generated = "const n = 1\nexport default n;\n";
        let artifacts = vue_main_compile_artifacts(
            "Comp.vue",
            &compiled,
            ProductKind::RuntimeClient,
            FragmentDialect::JavaScript,
            generated,
            Some(script_map),
            &VueMainDecoration::default(),
            true,
            "",
            &[],
        )
        .expect("schema accepts authored map geometry");
        let main = artifacts
            .artifacts()
            .iter()
            .find(|artifact| artifact.name() == "main")
            .expect("main");
        let map = main
            .maps
            .iter()
            .find(|map| map.family == ArtifactMapFamily::RuntimeSourceMap)
            .expect("runtime map");
        let main_unit = artifacts
            .source_units()
            .find(|unit| unit.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .id()
            .clone();
        let script_unit = artifacts
            .source_units()
            .find(|unit| unit.unit.logical_role() == "script")
            .expect("script unit");
        assert!(
            !map.segments.is_empty(),
            "authored tokens must produce segments"
        );
        assert!(
            map.segments
                .iter()
                .all(|segment| segment.source_unit != main_unit),
            "segments must not use the generated main unit as authored source"
        );
        assert!(
            map.segments
                .iter()
                .any(|segment| segment.source_unit == *script_unit.unit.id()),
            "a generated token must map onto the authored script unit"
        );
        assert!(
            map.segments.iter().all(|segment| {
                segment.source_span.start >= script_unit.source_span.start
                    && segment.source_span.end <= script_unit.source_span.end
            }),
            "authored spans must stay inside the script unit extent"
        );
        assert!(
            main.relations.iter().any(|relation| {
                relation.kind == ArtifactRelationKind::DependsOn
                    && artifacts.artifacts().iter().any(|artifact| {
                        artifact.id() == &relation.target && artifact.name() == "script"
                    })
            }),
            "main must retain a typed script dependency"
        );
    }

    #[test]
    fn overlapping_local_spans_bind_by_composed_source_row() {
        use crate::framework_common::{
            RuntimeOutputDescriptor, RuntimeScriptBlock, RuntimeTemplateBlock, SourceMapFidelity,
            TemplateRenderExport,
        };
        fn map_json(source: &str, content: &str) -> String {
            let token = oxc_sourcemap::Token::new(0, 0, 0, 0, Some(0), None);
            SourceMap::new(
                None,
                Vec::new(),
                None,
                vec![source.into()],
                vec![Some(content.into())],
                Box::new([token]),
                None,
            )
            .to_json_string()
        }
        let script_content = "const n = 1\n";
        let template_content = "<div>{{ n }}</div>\n";
        let script_map = map_json("Comp.vue", script_content);
        let template_map = map_json("Comp.vue", template_content);
        let composed = SourceMap::new(
            None,
            Vec::new(),
            None,
            vec!["Comp.vue".into(), "Comp.vue".into()],
            vec![Some(script_content.into()), Some(template_content.into())],
            Box::new([
                oxc_sourcemap::Token::new(0, 0, 0, 0, Some(0), None),
                oxc_sourcemap::Token::new(1, 0, 0, 0, Some(1), None),
            ]),
            None,
        )
        .to_json_string();
        let compiled = RuntimeCompileOutput {
            script: Some(RuntimeScriptBlock {
                code: script_content.to_string(),
                source_map: script_map,
                setup: false,
                output_descriptor: RuntimeOutputDescriptor::generated(
                    script_content,
                    None,
                    &[("test:space", "test:artifact")],
                    SourceMapFidelity::Approximate,
                ),
                generated_template_hole: None,
                runtime_imports: Vec::new(),
                sfc_export_placement: None,
            }),
            template: Some(RuntimeTemplateBlock {
                code: "function render() {}\n".to_string(),
                source_map: template_map,
                imports: Vec::new(),
                ssr_imports: Vec::new(),
                render_export: TemplateRenderExport::Render,
                output_descriptor: RuntimeOutputDescriptor::generated(
                    "function render() {}\n",
                    None,
                    &[("test:space", "test:artifact")],
                    SourceMapFidelity::Approximate,
                ),
            }),
            ..empty_bundle()
        };
        let generated = "const n = 1\nfunction render() {}\n";
        let artifacts = vue_main_compile_artifacts(
            "Comp.vue",
            &compiled,
            ProductKind::RuntimeClient,
            FragmentDialect::JavaScript,
            generated,
            Some(&composed),
            &VueMainDecoration::default(),
            true,
            "",
            &[],
        )
        .expect("schema accepts overlapping local spans");
        let script_unit = artifacts
            .source_units()
            .find(|unit| unit.unit.logical_role() == "script")
            .expect("script unit");
        let template_unit = artifacts
            .source_units()
            .find(|unit| unit.unit.logical_role() == "template")
            .expect("template unit");
        assert_eq!(
            script_unit.source_span.start,
            template_unit.source_span.start
        );
        let map = artifacts
            .artifacts()
            .iter()
            .find(|artifact| artifact.name() == "main")
            .expect("main")
            .maps
            .iter()
            .find(|map| map.family == ArtifactMapFamily::RuntimeSourceMap)
            .expect("runtime map");
        let script_id = script_unit.unit.id().clone();
        let template_id = template_unit.unit.id().clone();
        let script_segments: Vec<_> = map
            .segments
            .iter()
            .filter(|segment| segment.source_unit == script_id)
            .collect();
        let template_segments: Vec<_> = map
            .segments
            .iter()
            .filter(|segment| segment.source_unit == template_id)
            .collect();
        assert!(
            !script_segments.is_empty(),
            "script tokens must bind to the script unit"
        );
        assert!(
            !template_segments.is_empty(),
            "template tokens must bind to the template unit, not the overlapping script span"
        );
        assert!(
            template_segments
                .iter()
                .all(|segment| segment.source_unit != script_id),
            "duplicate source names must not collapse template tokens onto script"
        );
    }

    #[test]
    fn input_basis_retains_content_digests_not_raw_bytes() {
        let compiled = empty_bundle();
        let huge = format!("export default {{ n: '{}' }}\n", "a".repeat(32 * 1024));
        let artifacts = vue_main_compile_artifacts(
            "Comp.vue",
            &compiled,
            ProductKind::RuntimeClient,
            FragmentDialect::JavaScript,
            huge.as_str(),
            None,
            &VueMainDecoration::default(),
            false,
            "",
            &[],
        )
        .expect("schema accepts");
        let revision = artifacts
            .source_units()
            .find(|unit| unit.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .revision()
            .clone();
        let main = artifacts
            .artifacts()
            .iter()
            .find(|artifact| artifact.name() == "main")
            .expect("main");
        assert!(
            revision.canonical_bytes().len() < huge.len() / 4,
            "source revision must not retain the assembled payload, got {} vs {}",
            revision.canonical_bytes().len(),
            huge.len()
        );
        assert!(
            main.provenance.input_basis.canonical_bytes().len() < huge.len() / 4,
            "input basis must not retain the assembled payload"
        );
    }

    #[test]
    fn demanded_main_shares_one_payload_allocation() {
        let compiled = empty_bundle();
        let assembled = assemble_vue_runtime_main(VueRuntimeMainRequest {
            canonical_id: "Comp.vue",
            compiled: &compiled,
            script_lang: Some("js"),
            has_script: false,
            has_template: false,
            force_js: false,
            source_map: false,
            runtime: "vue",
            ssr: false,
            decoration: VueMainDecoration::default(),
            source: "",
            custom_blocks: &[],
        })
        .expect("template-less assembly still emits Main");
        let main = assembled
            .artifacts
            .artifacts()
            .iter()
            .find(|artifact| artifact.name() == "main")
            .expect("main");
        let ArtifactContent::Available(content) = &main.content else {
            panic!(
                "produced Main must keep available content, got {:?}",
                main.content
            );
        };
        assert!(
            Arc::ptr_eq(&assembled.code, content),
            "body payload and typed artifact must share one allocation"
        );
        assert!(
            assembled
                .artifacts
                .artifacts()
                .iter()
                .filter(|artifact| artifact.name() != "main")
                .all(|artifact| matches!(artifact.content, ArtifactContent::Unavailable(_))),
            "script/template artifacts must not copy fragment bytes"
        );
    }

    #[test]
    fn no_script_dialect_is_javascript() {
        assert_eq!(
            resolve_vue_main_dialect(None, false),
            FragmentDialect::JavaScript
        );
        assert_eq!(
            resolve_vue_main_dialect(Some("ts"), false),
            FragmentDialect::TypeScript
        );
    }

    #[test]
    fn missing_authored_script_map_refuses() {
        use crate::framework_common::{
            RuntimeOutputDescriptor, RuntimeScriptBlock, SourceMapFidelity,
        };
        let compiled = RuntimeCompileOutput {
            script: Some(RuntimeScriptBlock {
                code: "const _sfc_main = {}\nexport default _sfc_main\n".to_string(),
                source_map: String::new(),
                setup: false,
                output_descriptor: RuntimeOutputDescriptor::generated(
                    "const _sfc_main = {}\n",
                    None,
                    &[("test:space", "test:artifact")],
                    SourceMapFidelity::Approximate,
                ),
                generated_template_hole: None,
                runtime_imports: Vec::new(),
                sfc_export_placement: None,
            }),
            ..empty_bundle()
        };
        let err = assemble_vue_runtime_main(VueRuntimeMainRequest {
            canonical_id: "Comp.vue",
            compiled: &compiled,
            script_lang: Some("js"),
            has_script: true,
            has_template: false,
            force_js: false,
            source_map: true,
            runtime: "vue",
            ssr: false,
            decoration: VueMainDecoration::default(),
            source: "",
            custom_blocks: &[],
        })
        .expect_err("empty authored script map must refuse");
        assert!(matches!(
            err,
            VueMainAssemblyFailure::InputMap(AssembleMapFailure::MissingRequiredInputMap {
                fragment: MapFragment::Script,
            })
        ));
    }
}
