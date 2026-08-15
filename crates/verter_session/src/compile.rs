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

/// The assembled Vue runtime main module: the code, and the source map it was
/// generated from, as ONE result.
///
/// `source_map` is `None` when no map was requested — positively absent, not an
/// empty map and not a map with empty `mappings`. When a map WAS requested one
/// is always produced, even for a module none of whose present fragments
/// contributes a mapping: an empty artifact is the truthful description of such
/// a module, and degrading it to "no map" would make a map-enabled compile
/// indistinguishable from a map-disabled one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledVueModule {
    pub code: String,
    pub source_map: Option<String>,
}

/// Assemble the Vue `_sfc_main` runtime module from the framework-neutral
/// [`RuntimeCompileOutput`] the Vue carrier produced.
///
/// This is the host's VUE main-module assembly (its virtual-file concern:
/// style / custom-block virtual imports via [`render_ids`], plus HMR). It
/// consumes the NEUTRAL bundle — never the Vue-shaped `VerterCompileResult` —
/// so the carrier owns producing the blocks and the host owns wiring them into
/// the `_sfc_main` object. The output is byte-identical for the same blocks
/// regardless of how the carrier produced them.
///
/// The script fragment passes through two authorized rewrites — a global
/// `__sfc__` → `_sfc_main` rename, then a global removal of
/// `export default _sfc_main;\n` over the rename's output — both driven through
/// real [`CodeTransform`](verter_compiler::code_transform::CodeTransform)
/// transforms so the same chunk list produces the bytes AND the map. The
/// template fragment is written verbatim and is not rewritten.
///
/// Assembly-owned bytes — the virtual imports, render attachment, custom-block
/// invocation, `__file`, HMR, the SSR-context wrapper, and the export
/// scaffolding — carry no mapping, because no authored source justifies one.
///
/// Public so conformance/test harnesses (`verter_vue_conformance`) compare
/// against the GENUINE shipped runtime Main rather than a hand copy.
///
/// # Errors
///
/// [`AssembleMapFailure`] when a required input map is missing or a present one
/// is structurally uncomposable. Either way there is NO successful result — not
/// code without a map, not code with an empty map. With `profile.source_map`
/// disabled no map is required and no failure is possible.
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

    // Template runtime imports must come before script code (ESM requirement)
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
    }

    if let Some((script_code, chained)) = &rewritten_script {
        // The compiler-emitted script passes through UNCHANGED apart from the
        // two authorized rewrites: setup-binding elision (type-only imports,
        // unused setup imports) is owned by the compiler's
        // `build_returned_object` (template_used_vars-driven), not by a
        // text-level post-pass here — the old `filter_setup_return` was
        // removed: it keyed on a `return { ... };` shape the compiler has not
        // emitted since `__returned__` was introduced, so it was dead code on
        // the real production output (proven by canary).
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

    if !profile.is_production {
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
    // canonical.
    if profile.ssr {
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
