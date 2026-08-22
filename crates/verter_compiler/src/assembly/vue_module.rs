//! Framework-neutral half of Vue main-module composition, shared by every
//! caller that assembles a Vue `_sfc_main` runtime module from a
//! [`RuntimeCompileOutput`] — `verter_session`'s host-decorated
//! `assemble_vue_main_module` (style/custom-block virtual imports, HMR,
//! `__file`, SSR-manifest wiring) and the borrowed one-shot
//! [`super::super::standalone::StandaloneCompiler`] direct core alike. Host
//! decoration rides in as [`ExtraFragment`] prelude/trailer content; this
//! module owns everything else: the `__sfc__` → `_sfc_main` rewrite, script/
//! template/style-import fragment minting, sequencing, and the final
//! [`super::publish`] atomicity boundary. Never duplicated — a caller with no
//! host decoration (the direct core) passes empty extra-fragment sets.

use std::ops::Range;

use oxc_sourcemap::SourceMap;

use super::compose::{assemble_sequence, ComposeRefusal};
use super::fragment::{
    DeclaredImport, DeclaredImportKind, Fragment, FragmentDialect, FragmentRefusal,
    FrameworkDomain, PlacementSlot, SfcExportPlacement, SyntacticContract, ValidatedFragment,
};
use super::plan::{PlannedArtifact, ProductPlan};
use super::publish::{publish, ArtifactContribution, ArtifactSet, AssemblyRefusal};
use super::source_space::SourceSpaceKind;
use super::source_unit::SourceUnitId;
use crate::code_transform::CodeTransform;
use crate::compile::format_import_specifier;
use crate::compile_request::ProductKind;
use crate::framework_common::{RuntimeCompileOutput, TemplateRenderExport};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};

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

/// One host- or caller-owned decoration piece contributed to the composed
/// module's prelude (ahead of the script) or trailer (before the terminal
/// `export default`) — style/custom-block virtual imports, HMR, `__file`,
/// SSR-manifest wiring for `verter_session`'s host composer; empty for the
/// direct one-shot core (no host state exists for a one-shot compile).
#[derive(Debug, Clone, Default)]
pub struct ExtraFragment {
    pub role: &'static str,
    pub code: String,
    pub imports: Vec<DeclaredImport>,
}

/// Everything [`compose_main_module`] needs beyond a caller's own
/// decoration: the compiled blocks, the resolved dialect/product kind/
/// runtime specifier, whether a map was requested, and every ALREADY-
/// DECODED contributing map. Never raw source or a
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
    /// Extra fragments placed in the module prelude, ahead of the script.
    pub prelude_extra: Vec<ExtraFragment>,
    /// Extra raw text appended to the trailer, before the terminal
    /// `export default` statement. Declared imports these lines need ride
    /// along per fragment.
    pub trailer_extra: Vec<ExtraFragment>,
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
    Composition(VueMainCompositionFailure),
    Publication(AssemblyRefusal),
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
            Self::Composition(e) => write!(f, "{e}"),
            Self::Publication(e) => write!(f, "Main-module publication failed: {e:?}"),
        }
    }
}

impl std::error::Error for VueMainAssemblyFailure {}

/// Deterministic role-based [`SourceUnitId`] for one of this function's own
/// scaffold/content fragments — same `canonical_id` + `role` always mints
/// the same id, so the identity is a pure function of the two, never a
/// counter.
struct MainFragmentTag<'a> {
    canonical_id: &'a str,
    role: &'a str,
}

impl CanonicalEncode for MainFragmentTag<'_> {
    const DOMAIN_TAG: &'static str = "verter.compiler.assembly.vue_main_fragment.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.canonical_id);
        e.field_str(2, self.role);
    }
}

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
        source_unit: SourceUnitId::from_canonical(&MainFragmentTag { canonical_id, role }),
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
/// caller (`verter_session`'s host composer) that only ever publishes this
/// one artifact.
pub(crate) struct ComposedFragments {
    pub fragments: Vec<ValidatedFragment>,
    pub code: String,
    pub source_map: String,
    pub emitted_imports: Vec<DeclaredImport>,
}

/// Compose the Vue `_sfc_main` runtime module's fragments from a
/// framework-neutral [`RuntimeCompileOutput`] plus caller-owned decoration —
/// EVERYTHING [`compose_main_module`] does except the final [`publish`] call.
/// Shared by `verter_session`'s host-decorated `assemble_vue_main_module`
/// (through [`compose_main_module`]) and the direct one-shot core (through
/// this function directly, so it can publish this artifact atomically
/// alongside sibling contributions from the SAME
/// [`crate::compile_request::CompileRequest`]) — see this module's own doc
/// for the split.
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
        prelude_extra,
        trailer_extra,
    } = request;

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
        source_map: sequenced.source_map,
        emitted_imports,
    })
}

/// Compose AND publish a Main module in one call — the single-artifact
/// convenience for a caller that only ever publishes the one artifact it
/// composes (`verter_session`'s host-decorated `assemble_vue_main_module`).
/// A caller publishing this artifact atomically alongside sibling
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
}
