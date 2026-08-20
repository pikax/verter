//! Main module assembly.

use verter_compiler::assembly::{
    assemble_sequence, publish, ArtifactContribution, DeclaredImport, DeclaredImportKind, Fragment,
    FragmentDialect, FrameworkDomain, PlacementSlot, PlannedArtifact, ProductPlan, SourceSpaceKind,
    SourceUnitId, SyntacticContract, ValidatedFragment,
};
use verter_compiler::compile::format_import_specifier;
use verter_compiler::compile_request::ProductKind;
use verter_compiler::framework_common::RuntimeCompileOutput;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};

use crate::id::render_ids;
use crate::types::{CompileProfile, FileMeta, HmrStrategy, VirtualNodeKind};

mod map_compose;
mod map_input;
mod map_json;

#[cfg(test)]
mod compile_tests;
#[cfg(test)]
mod map_equality_tests;
#[cfg(test)]
mod map_tests;

pub use map_compose::SfcRewriteRefusal;
pub use map_input::{AssembleMapFailure, MapFragment, UncomposableCode, UncomposableFamily};

use map_input::{agree_source_root, validate_and_decode, DecodedFragmentMap};

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

impl From<verter_compiler::assembly::FragmentRefusal> for VueMainAssemblyFailure {
    fn from(failure: verter_compiler::assembly::FragmentRefusal) -> Self {
        Self::FragmentValidation(failure)
    }
}

impl From<verter_compiler::assembly::ComposeRefusal> for VueMainAssemblyFailure {
    fn from(failure: verter_compiler::assembly::ComposeRefusal) -> Self {
        Self::Composition(failure)
    }
}

impl From<verter_compiler::assembly::AssemblyRefusal> for VueMainAssemblyFailure {
    fn from(failure: verter_compiler::assembly::AssemblyRefusal) -> Self {
        Self::Publication(failure)
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

/// Deterministic role-based [`SourceUnitId`] for one of this function's own
/// scaffold/content fragments — same `canonical_id` + `role` always mints
/// the same id, so the identity is a pure function of the two, never a
/// counter.
struct MainFragmentTag<'a> {
    canonical_id: &'a str,
    role: &'a str,
}

impl CanonicalEncode for MainFragmentTag<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.compile.vue_main_fragment.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.canonical_id);
        e.field_str(2, self.role);
    }
}

/// The exact language a Main module's fragments and final artifact are
/// validated/parsed under — derived ONCE from the SAME inputs
/// `virtual_file_pipeline.rs` used to independently (and redundantly)
/// re-derive `main_lang` for both its Main-node paths, so this is the
/// single authority both now read from [`AssembledVueModule::lang`]
/// instead.
fn resolve_main_dialect(meta: &FileMeta, profile: &CompileProfile) -> FragmentDialect {
    let raw = meta.script_lang.as_deref().unwrap_or("js");
    let is_tsx = raw.eq_ignore_ascii_case("tsx");
    let is_jsx = is_tsx || raw.eq_ignore_ascii_case("jsx");
    let is_ts = is_tsx || raw.eq_ignore_ascii_case("ts");
    if profile.force_js {
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

fn dialect_lang_str(dialect: FragmentDialect) -> &'static str {
    match dialect {
        FragmentDialect::JavaScript => "js",
        FragmentDialect::Jsx => "jsx",
        FragmentDialect::TypeScript => "ts",
        FragmentDialect::Tsx => "tsx",
        FragmentDialect::Declaration => "dts",
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
) -> Result<(), VueMainAssemblyFailure> {
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

/// Assemble the Vue `_sfc_main` runtime module from the carrier's
/// framework-neutral [`RuntimeCompileOutput`].
///
/// Host owns virtual-file wiring (style / custom-block imports via
/// [`render_ids`], HMR); the carrier owns the blocks. Same blocks
/// produce byte-identical output.
///
/// Script rewrites (`__sfc__` → `_sfc_main`, then strip
/// `export default _sfc_main;\n`) go through
/// [`CodeTransform`](verter_compiler::code_transform::CodeTransform)
/// so the same chunk list produces bytes and map. Template is verbatim.
/// Assembly-owned bytes (imports, `__file`, HMR, SSR wrapper, export)
/// carry no mapping.
///
/// Every scaffold/content piece is a real, VALIDATED
/// [`verter_compiler::assembly::Fragment`] — sequenced through
/// [`assemble_sequence`] and published through [`publish`] — never a raw
/// `{code, source_map}` pair. The SAME validated collection is what
/// `publish`'s atomicity checks (declared-import/undeclared-helper,
/// final-parse) run against.
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
    use std::fmt::Write;

    // Validation runs to completion BEFORE any composition work begins. When no
    // map was requested it does not run at all, and a fragment's non-empty map
    // string is ignored rather than composed unasked.
    let inputs = if profile.source_map {
        Some(validate_inputs(compiled, meta)?)
    } else {
        None
    };
    let want_maps = inputs.is_some();
    let source_root = inputs
        .as_ref()
        .and_then(|inputs| inputs.source_root.clone());
    let script_map = inputs.as_ref().and_then(|inputs| inputs.script.as_ref());
    // Re-encoded through this crate's own canonical-form encoder — never
    // the template's raw as-authored map string — so a legitimate
    // dual-spelling ignore list never crosses into `oxc_sourcemap`'s
    // stricter decoder (see `ValidatedInputs`'s own doc).
    let template_map_json: Option<String> = inputs
        .as_ref()
        .and_then(|inputs| inputs.template.as_ref())
        .map(|map| map_compose::to_source_map(map).to_json_string());

    // The rewrite runs whether or not a map was requested: it determines
    // the module's bytes regardless of `profile.source_map`. Applies ONLY
    // the script's own declared `SfcExportPlacement` fact — never scans
    // generated text for the `__sfc__`/`export default` landmarks.
    let rewritten_script = compiled
        .script
        .as_ref()
        .map(|script| {
            map_compose::rewrite_script(
                &script.code,
                script.sfc_export_placement.as_ref(),
                script_map,
            )
        })
        .transpose()
        .map_err(|reason| AssembleMapFailure::InvalidSfcExportPlacement { reason })?;

    // Derived ONCE, reused for every fragment's own dialect, the final
    // artifact's dialect, and the returned `lang` — never a fixed
    // permissive default and never re-derived a second time downstream.
    let dialect = resolve_main_dialect(meta, profile);
    let runtime = profile.runtime_module_name.as_deref().unwrap_or("vue");

    let planned_kind = if profile.ssr {
        ProductKind::RuntimeServer
    } else {
        ProductKind::RuntimeClient
    };

    let mut fragments: Vec<ValidatedFragment> = Vec::new();

    // ── prelude: style + custom-block virtual imports ──────────────────
    let mut prelude = String::new();
    let mut prelude_imports: Vec<DeclaredImport> = Vec::new();
    for idx in 0..compiled.styles.len() {
        let (id, _) = render_ids(canonical_id, &VirtualNodeKind::Style { index: idx }, meta);
        let _ = writeln!(prelude, "import \"{}\"", id);
        prelude_imports.push(DeclaredImport {
            specifier: id,
            kind: DeclaredImportKind::SideEffect,
        });
    }
    for idx in 0..compiled.custom_blocks.len() {
        let (id, _) = render_ids(canonical_id, &VirtualNodeKind::Custom { index: idx }, meta);
        let block_name = format!("block{}", idx);
        let _ = writeln!(prelude, "import {} from \"{}\"", block_name, id);
        prelude_imports.push(DeclaredImport {
            specifier: id,
            kind: DeclaredImportKind::Default(block_name),
        });
    }
    if !compiled.styles.is_empty() || !compiled.custom_blocks.is_empty() {
        prelude.push('\n');
    }
    mint_and_validate(
        &mut fragments,
        canonical_id,
        "prelude",
        planned_kind,
        PlacementSlot::ModulePrelude,
        dialect,
        prelude,
        None,
        prelude_imports,
    )?;

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

        // The template is written verbatim and is never rewritten, so its
        // map (already the template codegen's own encoded output,
        // re-encoded through the canonical single-spelling encoder above)
        // is sequenced directly with no chain step.
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
            verter_compiler::framework_common::TemplateRenderExport::SsrRender => {
                post_template.push_str("_sfc_main.ssrRender = ssrRender\n");
            }
            verter_compiler::framework_common::TemplateRenderExport::Render => {
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

    // ── trailer: custom-block invocations, __file, HMR, SSR registration,
    // the terminal `export default` ──────────────────────────────────
    let mut trailer = String::new();
    for idx in 0..compiled.custom_blocks.len() {
        let _ = writeln!(
            trailer,
            "if (typeof block{} === 'function') block{}(_sfc_main)",
            idx, idx
        );
    }

    // Official `transformMain` gates `__file` on
    // `devToolsEnabled || (devServer && !isProduction)`. `hmr_strategy:
    // None` means no dev-server tooling, so skip `__file` as well as HMR.
    if !profile.is_production && profile.hmr_strategy != HmrStrategy::None {
        let _ = writeln!(trailer, "_sfc_main.__file = {:?}", canonical_id);
    }

    if !profile.is_production && !profile.ssr {
        match profile.hmr_strategy {
            HmrStrategy::Vite => {
                trailer.push_str("/* HMR(vite) */\n");
                trailer.push_str("if (import.meta.hot) { import.meta.hot.accept(() => {}) }\n");
            }
            HmrStrategy::Webpack => {
                trailer.push_str("/* HMR(webpack) */\n");
                trailer.push_str("if (module.hot) { module.hot.accept(() => {}) }\n");
            }
            HmrStrategy::None => {}
        }
    }

    // Vite SSR asset collection: register this module id on the request's
    // `ssrContext.modules` set (same shape as @vitejs/plugin-vue). Without
    // this, Vite cannot collect CSS/JS deps for the SSR render tree. The
    // registered id must match the ssr-manifest KEY FORM — root-relative
    // under Vite — so the bundler-supplied `ssr_module_id` wins; the
    // canonical id is only a fallback for callers whose manifest keys are
    // canonical. Real `@vitejs/plugin-vue` `transformMain` emits this
    // unconditionally on `ssr` (confirmed directly against its source —
    // no dev-server gate, dev AND production both get it), so
    // `emit_ssr_module_registration` defaults `true` and this is NOT
    // gated on `hmr_strategy`/production the way `__file`/HMR are above —
    // see the field's own doc comment for the one narrow exception.
    let mut trailer_imports: Vec<DeclaredImport> = Vec::new();
    if profile.ssr && profile.emit_ssr_module_registration {
        let _ = writeln!(
            trailer,
            "import {{ useSSRContext as __vite_useSSRContext }} from \"{}\"",
            runtime
        );
        trailer_imports.push(DeclaredImport {
            specifier: runtime.to_string(),
            kind: DeclaredImportKind::Named(vec!["__vite_useSSRContext".to_string()]),
        });
        trailer.push_str("const _sfc_setup = _sfc_main.setup\n");
        trailer.push_str("_sfc_main.setup = (props, ctx) => {\n");
        trailer.push_str("  const ssrContext = __vite_useSSRContext()\n");
        let registered_id = profile.ssr_module_id.as_deref().unwrap_or(canonical_id);
        let _ = writeln!(
            trailer,
            "  ;(ssrContext.modules || (ssrContext.modules = new Set())).add({:?})",
            registered_id
        );
        trailer.push_str("  return _sfc_setup ? _sfc_setup(props, ctx) : undefined\n");
        trailer.push_str("}\n");
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

    let fragment_refs: Vec<&ValidatedFragment> = fragments.iter().collect();
    let sequenced = assemble_sequence(&fragment_refs, source_root.as_deref())?;

    // Publish through the shared atomic boundary: exact-cardinality,
    // required-map, undeclared-helper, and final-parse checks over the
    // fully composed module — this host composer never went through a
    // `CompileRequest`/`ProductPlan`, so it declares the one artifact it
    // itself composes. `fragments`/`emitted_imports` are the SAME
    // collection just validated and sequenced, not a second copy.
    let plan = ProductPlan::single(PlannedArtifact {
        kind: planned_kind,
        requires_source_projection_map: false,
        requires_runtime_source_map: want_maps,
    });
    let emitted_imports: Vec<DeclaredImport> = fragments
        .iter()
        .flat_map(|f| f.fragment().imports.iter().cloned())
        .collect();
    let contribution = ArtifactContribution {
        kind: planned_kind,
        fragments: fragment_refs,
        code: sequenced.code,
        emitted_imports,
        dialect,
        source_projection_map: None,
        runtime_source_map: want_maps.then_some(sequenced.source_map),
    };
    let set = publish(&plan, vec![contribution])?;
    let artifact = set
        .artifact(planned_kind)
        .expect("publish returns exactly the one planned artifact kind");

    Ok(AssembledVueModule {
        code: artifact.code().to_string(),
        source_map: artifact.runtime_source_map().map(|s| s.to_string()),
        lang: dialect_lang_str(dialect).to_string(),
    })
}

/// The contributing maps, validated in the specified order. The template's
/// own decoded map is validated here (decodability, index bounds, and its
/// contribution to `source_root` agreement) and IS retained: assembly
/// re-encodes it through this crate's OWN canonical-form encoder
/// (`map_compose::to_source_map`) before sequencing, rather than passing
/// the template's raw as-authored map string — `oxc_sourcemap`'s decoder
/// rejects an otherwise-valid map declaring both accepted ignore-list
/// spellings (a "duplicate field", stricter than `validate_and_decode`'s
/// "both spellings, must agree" rule), so only the single-spelling
/// canonical re-encoding may safely cross into `assemble_sequence`.
struct ValidatedInputs {
    script: Option<DecodedFragmentMap>,
    template: Option<DecodedFragmentMap>,
    source_root: Option<String>,
}

/// A fragment's map is REQUIRED iff the fragment is both AUTHORED and PRESENT.
///
/// Authorship comes from the pre-assembly authored-fragment inventory, never
/// from the presence of a compiled block: a template-only cell whose compiler
/// synthesised a script block is not missing a required map, it is synthetic
/// sourceless code. Presence participates too, because the alternative would
/// demand a map for a fragment that emits no bytes — the inline topology, where
/// a template is authored but the render closure lives inside `setup()` and no
/// template block exists.
fn validate_inputs(
    compiled: &RuntimeCompileOutput,
    meta: &FileMeta,
) -> Result<ValidatedInputs, AssembleMapFailure> {
    let script_required = meta.has_script && compiled.script.is_some();
    let template_required = meta.has_template && compiled.template.is_some();

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

    // The per-map checks run to completion for the SCRIPT map first, then for
    // the template: a malformed script map and a dangling-index template map
    // report the script's outcome.
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

    // The cross-map agreement runs over the contributing set at ANY
    // cardinality, including exactly one and zero — it is not conditional on
    // both fragments carrying maps, which is how a single-fragment compile
    // would otherwise skip it.
    let source_root = agree_source_root(
        script
            .iter()
            .map(|map| (MapFragment::Script, map))
            .chain(template.iter().map(|map| (MapFragment::Template, map))),
    )?;

    Ok(ValidatedInputs {
        script,
        template,
        source_root,
    })
}
