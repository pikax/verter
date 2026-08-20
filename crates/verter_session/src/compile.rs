//! Main module assembly.

use verter_compiler::compile::format_import_specifier;
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

use map_compose::{FragmentWrite, MapComposer, ModuleWriter, SegmentOrigin};
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
/// # Errors
///
/// [`AssembleMapFailure`] if a required map is missing or uncomposable.
/// No success without a map when one was requested.
pub fn assemble_vue_main_module(
    canonical_id: &str,
    compiled: &RuntimeCompileOutput,
    meta: &FileMeta,
    profile: &CompileProfile,
) -> Result<AssembledVueModule, AssembleMapFailure> {
    use std::fmt::Write;

    // Validation runs to completion BEFORE any composition work begins. When no
    // map was requested it does not run at all, and a fragment's non-empty map
    // string is ignored rather than composed unasked.
    let inputs = if profile.source_map {
        Some(validate_inputs(compiled, meta)?)
    } else {
        None
    };
    let script_map = inputs.as_ref().and_then(|inputs| inputs.script.as_ref());
    let template_map = inputs.as_ref().and_then(|inputs| inputs.template.as_ref());

    // The two script rewrites run whether or not a map was requested: they
    // determine the module's bytes.
    let rewritten_script = compiled
        .script
        .as_ref()
        .map(|script| map_compose::rewrite_script(&script.code, script_map));

    // Estimate capacity: script + template + overhead
    let script_len = compiled.script.as_ref().map_or(20, |s| s.code.len());
    let template_len = compiled.template.as_ref().map_or(0, |t| t.code.len());
    let mut out = ModuleWriter::with_capacity(script_len + template_len + 256);
    let mut composer = MapComposer::default();

    for idx in 0..compiled.styles.len() {
        let (id, _) = render_ids(canonical_id, &VirtualNodeKind::Style { index: idx }, meta);
        let _ = writeln!(out, "import \"{}\"", id);
    }

    for idx in 0..compiled.custom_blocks.len() {
        let (id, _) = render_ids(canonical_id, &VirtualNodeKind::Custom { index: idx }, meta);
        let _ = writeln!(out, "import block{} from \"{}\"", idx, id);
    }

    if !compiled.styles.is_empty() || !compiled.custom_blocks.is_empty() {
        out.push('\n');
    }

    // Script (including its imports) precedes the template's runtime
    // helper imports — official `@vitejs/plugin-vue` / `@vue/compiler-sfc`
    // order. ESM hoists imports either way; the order is conformance.
    if let Some((script_code, chained)) = &rewritten_script {
        // Script is unchanged except the two authorized rewrites.
        // Setup-binding elision is `build_returned_object`, not a text
        // post-pass (`filter_setup_return` keyed on a `return { ... };`
        // shape the compiler no longer emits).
        composer.write_fragment(
            &mut out,
            FragmentWrite {
                code: script_code,
                chained: chained.as_deref(),
                map: script_map,
                origin: SegmentOrigin::Script,
            },
        );
        if !script_code.ends_with('\n') {
            out.push('\n');
        }
    } else {
        out.push_str("const _sfc_main = {}\n");
        if !compiled.scope_id.is_empty() {
            let _ = writeln!(out, "_sfc_main.__scopeId = \"{}\"", compiled.scope_id);
        }
    }

    if let Some(template) = &compiled.template {
        if !template.imports.is_empty() {
            let runtime = profile.runtime_module_name.as_deref().unwrap_or("vue");
            let _ = write!(out, "import {{ ");
            for (i, name) in template.imports.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format_import_specifier(name));
            }
            let _ = writeln!(out, " }} from \"{}\"", runtime);
        }
        // SSR helpers are imported from "vue/server-renderer"
        if !template.ssr_imports.is_empty() {
            let _ = write!(out, "import {{ ");
            for (i, name) in template.ssr_imports.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format_import_specifier(name));
            }
            let _ = writeln!(out, " }} from \"vue/server-renderer\"");
        }
        out.push('\n');
        composer.write_fragment(
            &mut out,
            FragmentWrite {
                code: &template.code,
                // The template is written verbatim and is never rewritten, so
                // its map is placed directly with no chain step.
                chained: template_map.map(|map| map.segments.as_slice()),
                map: template_map,
                origin: SegmentOrigin::Template,
            },
        );
        if !template.code.ends_with('\n') {
            out.push('\n');
        }
        if template.code.contains("function ssrRender(") {
            out.push_str("_sfc_main.ssrRender = ssrRender\n");
        } else if template.code.contains("function render(") {
            out.push_str("_sfc_main.render = render\n");
        }
    }

    for idx in 0..compiled.custom_blocks.len() {
        let _ = writeln!(
            out,
            "if (typeof block{} === 'function') block{}(_sfc_main)",
            idx, idx
        );
    }

    // Official `transformMain` gates `__file` on
    // `devToolsEnabled || (devServer && !isProduction)`. `hmr_strategy:
    // None` means no dev-server tooling, so skip `__file` as well as HMR.
    if !profile.is_production && profile.hmr_strategy != HmrStrategy::None {
        let _ = writeln!(out, "_sfc_main.__file = {:?}", canonical_id);
    }

    if !profile.is_production && !profile.ssr {
        match profile.hmr_strategy {
            HmrStrategy::Vite => {
                out.push_str("/* HMR(vite) */\n");
                out.push_str("if (import.meta.hot) { import.meta.hot.accept(() => {}) }\n");
            }
            HmrStrategy::Webpack => {
                out.push_str("/* HMR(webpack) */\n");
                out.push_str("if (module.hot) { module.hot.accept(() => {}) }\n");
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
    if profile.ssr && profile.emit_ssr_module_registration {
        let runtime = profile.runtime_module_name.as_deref().unwrap_or("vue");
        let _ = writeln!(
            out,
            "import {{ useSSRContext as __vite_useSSRContext }} from \"{}\"",
            runtime
        );
        out.push_str("const _sfc_setup = _sfc_main.setup\n");
        out.push_str("_sfc_main.setup = (props, ctx) => {\n");
        out.push_str("  const ssrContext = __vite_useSSRContext()\n");
        let registered_id = profile.ssr_module_id.as_deref().unwrap_or(canonical_id);
        let _ = writeln!(
            out,
            "  ;(ssrContext.modules || (ssrContext.modules = new Set())).add({:?})",
            registered_id
        );
        out.push_str("  return _sfc_setup ? _sfc_setup(props, ctx) : undefined\n");
        out.push_str("}\n");
    }

    out.push_str("export default _sfc_main");

    let source_map = inputs.map(|inputs| composer.into_artifact(inputs.source_root));

    Ok(AssembledVueModule {
        code: out.into_string(),
        source_map,
    })
}

/// The contributing maps, validated in the specified order.
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
