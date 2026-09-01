//! Main module assembly.

use verter_compiler::assembly::{
    compose_main_module, DeclaredImport, DeclaredImportKind, ExtraFragment, FragmentDialect,
    VueMainCompositionFailure, VueMainModuleRequest,
};
use verter_compiler::compile_request::ProductKind;
use verter_compiler::framework_common::RuntimeCompileOutput;

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

pub use map_input::{AssembleMapFailure, MapFragment, UncomposableCode, UncomposableFamily};
pub use verter_compiler::assembly::SfcRewriteRefusal;

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

/// The exact language a Main module's fragments and final artifact are
/// validated/parsed under — derived ONCE from the SAME inputs
/// `virtual_file_pipeline.rs` used to independently (and redundantly)
/// re-derive `main_lang` for both its Main-node paths, so this is the
/// single authority both now read from [`AssembledVueModule::lang`]
/// instead.
fn resolve_main_dialect(meta: &FileMeta, profile: &VueMainAssemblyAxes) -> FragmentDialect {
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

/// Assemble the Vue `_sfc_main` runtime module from the carrier's
/// framework-neutral [`RuntimeCompileOutput`].
///
/// Host owns virtual-file wiring (style / custom-block imports via
/// [`render_ids`], HMR); the carrier owns the blocks. Same blocks
/// produce byte-identical output.
///
/// The `__sfc__` → `_sfc_main` rewrite, script/template/import fragment
/// minting, sequencing, and the final atomic-publication boundary are the
/// SAME [`verter_compiler::assembly`] composer
/// ([`compose_main_module`]/[`VueMainModuleRequest`]) the direct one-shot
/// core shares — this function's own remaining job is exactly the HOST
/// decoration: style/custom-block virtual imports, `__file`, HMR, and the
/// Vite SSR-manifest registration, riding in as
/// [`ExtraFragment`] prelude/trailer content, plus this crate's own hardened
/// input-map validation ([`validate_inputs`]).
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

/// Exactly the axes the host-side `Main` assembly reads, named
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

/// The one host-side `Main` assembly. [`assemble_vue_main_module`] is the
/// compile-profile-stated spelling of this same call.
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
    // Decoded under this crate's own hardened multi-fragment validator
    // (`validate_inputs`/`validate_and_decode` above) — host-authored/
    // cross-tool maps need it; lifted into the typed wire form the shared
    // composer's request consumes.
    let script_map = inputs
        .as_ref()
        .and_then(|inputs| inputs.script.as_ref())
        .map(map_compose::to_source_map);
    // Re-encoded through this crate's own canonical-form encoder — never
    // the template's raw as-authored map string — so a legitimate
    // dual-spelling ignore list never crosses into `oxc_sourcemap`'s
    // stricter decoder (see `ValidatedInputs`'s own doc).
    let template_map_json: Option<String> = inputs
        .as_ref()
        .and_then(|inputs| inputs.template.as_ref())
        .map(|map| map_compose::to_source_map(map).to_json_string());

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

    // ── host decoration: style + custom-block virtual imports, ahead of
    // the script ─────────────────────────────────────────────────────────
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
    let prelude_extra = vec![ExtraFragment {
        role: "prelude",
        code: prelude,
        imports: prelude_imports,
    }];

    // ── host decoration: custom-block invocations, __file, HMR, SSR
    // registration — before the shared composer's terminal
    // `export default` ──────────────────────────────────────────────────
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
    let trailer_extra = vec![ExtraFragment {
        role: "trailer",
        code: trailer,
        imports: trailer_imports,
    }];

    let request = VueMainModuleRequest {
        canonical_id,
        compiled,
        dialect,
        planned_kind,
        runtime,
        want_maps,
        source_root: source_root.as_deref(),
        script_map: script_map.as_ref(),
        template_map_json,
        prelude_extra,
        trailer_extra,
    };
    let set = compose_main_module(request)?;
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
