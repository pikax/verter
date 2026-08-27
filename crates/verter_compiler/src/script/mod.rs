//! Script codegen for the AST-based pipeline.
//!
//! Processes `<script>` and `<script setup>` blocks using
//! [`CodeGenOutput`] for all transformations. Returns binding
//! metadata for the template codegen's [`BindingResolver`].

pub mod css_vars;
pub mod macros;
pub mod prepared;
pub mod process;

#[cfg(test)]
mod ported_tests;

use oxc_allocator::Allocator;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::code_transform::{CodeTransform, GeneratedContentMarker};
use crate::parser::types::RootNodeScript;
use crate::script::prepared::PreparedScript;
use crate::style_planner::VBindVar;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::types::CodeGenOutput;

/// Options for script code generation.
#[derive(Debug, Clone, Default)]
pub struct ScriptCodeGenOptions<'a> {
    /// Authoritative runtime macro semantics for type-based codegen.
    pub macro_runtime: Option<&'a verter_macro_dto::MacroRuntimeBundle>,
    /// Vue production policy for runtime prop/model declarations.
    pub is_production: bool,
    /// Vue custom-element runtime-prop policy. Independent of template
    /// custom-element tag matching.
    pub custom_element: bool,
    /// Component name (used in `__name` property).
    pub component_name: &'a str,
    /// Scoped style ID (e.g., `"data-v-abc123"`).
    pub scope_id: &'a str,
    /// When true, keep TypeScript syntax (interfaces, type aliases, enums)
    /// by hoisting them to file top instead of stripping.
    pub keep_ts_types: bool,
    /// Inline template mode — template is inlined inside `setup()`.
    pub inline_template: bool,
    /// Vapor mode output.
    pub is_vapor: bool,
    /// SSR mode. Non-inline path attaches `ssrRender` separately and returns
    /// plain setup bindings (no `__isScriptSetup` / `__ssrInlineRender`) so the
    /// instance proxy exposes them to `_ctx.*` in `ssrRender`.
    pub ssr: bool,
    /// Whether any `<style scoped>` block exists.
    pub has_scoped_style: bool,
    /// CSS v-bind vars from style codegen (for `_useCssVars` injection).
    pub css_v_binds: &'a [VBindVar],
    /// Set of identifiers used in the template (from AST-based expression
    /// binding extraction + component tag names). `SetupImport` bindings are
    /// only included in `__returned__` when their name appears in this set.
    /// `None` means no template — all imports are included.
    pub template_used_vars: Option<rustc_hash::FxHashSet<String>>,
}

/// The literal internal binding name every runtime-emission site writes at
/// least once before host assembly renames it to `_sfc_main`.
pub(crate) const SFC_BINDING: &str = "__sfc__";

/// Write [`SFC_BINDING`] into `out` and return its own local byte range
/// within `out` — the declared fact
/// [`crate::template::code_gen::types::CodeGenOutput::record_sfc_export_fact`]
/// needs, recorded at the point of writing rather than rediscovered later
/// by scanning generated text for the literal string.
pub(crate) fn push_sfc_binding(out: &mut String) -> std::ops::Range<u32> {
    let start = out.len() as u32;
    out.push_str(SFC_BINDING);
    start..out.len() as u32
}

/// Write the terminal `export default __sfc__;\n` statement into `out` and
/// return `(binding_local_range, statement_local_range)` — the binding
/// reference within the statement, and the statement's own full range
/// (removed entirely once host assembly re-exports the composed module).
pub(crate) fn push_default_export_statement(
    out: &mut String,
) -> (std::ops::Range<u32>, std::ops::Range<u32>) {
    let stmt_start = out.len() as u32;
    out.push_str("export default ");
    let binding_range = push_sfc_binding(out);
    out.push_str(";\n");
    (binding_range, stmt_start..out.len() as u32)
}

/// Visit every public binding name supplied by one authoritative runtime macro
/// shape. Runtime and IDE codegen share this projection so binding ownership
/// cannot diverge between their independent output lanes.
pub(crate) fn visit_runtime_macro_binding_names(
    shape: &verter_macro_dto::MacroRuntimeShape,
    mut visit: impl FnMut(&str),
) {
    match shape {
        verter_macro_dto::MacroRuntimeShape::Props(props) => {
            for prop in &props.props {
                visit(&prop.name);
            }
        }
        verter_macro_dto::MacroRuntimeShape::Model(model) => visit(&model.prop.name),
        verter_macro_dto::MacroRuntimeShape::Emits(_) => {}
    }
}

/// Shared mutable context for script processing functions.
///
/// Bundles the common state passed through `process_script_setup`,
/// `process_script_only`, and `process_macro_item`.
pub struct ScriptContext<'alloc> {
    pub source: &'alloc str,
    pub out: CodeGenOutput<'alloc>,
    pub bindings: FxHashMap<&'alloc str, BindingType>,
    /// `bindings`' keys, in FIRST-SEEN declaration order (parse-order,
    /// deduplicated) — official's non-inline `__returned__` (`genSetupReturn`)
    /// preserves `allBindings`' insertion order (JS object key order), not an
    /// alphabetical sort; `bindings` itself is an `FxHashMap` and cannot
    /// recover that order on its own. See `build_returned_object`.
    pub binding_order: Vec<&'alloc str>,
    pub imports: Vec<&'static str>,
    pub inline_inject_pos: Option<u32>,
    pub alloc: &'alloc Allocator,
    /// Named/default user imports that official marks `setup-maybe-ref` —
    /// ref-bindable as inline template refs (`ref_key`/`ref: name`). The
    /// official rule: anything except namespace imports, default imports
    /// from `.vue` sources, and `vue`-source imports (those are
    /// `setup-const` and stay string refs).
    pub ref_bindable_imports: FxHashSet<&'alloc str>,
}

/// Result of script code generation.
#[allow(dead_code)] // Fields read by tests and downstream consumers
pub struct ScriptCodeGenResult<'alloc> {
    /// Binding metadata for template codegen.
    /// Maps identifier name → BindingType for `BindingResolver`.
    pub bindings: FxHashMap<&'alloc str, BindingType>,
    /// Position to inject inline template (`script.tag_close.start`).
    /// `None` if not inline mode. The orchestrator uses this with `move_slice`.
    pub inline_inject_pos: Option<u32>,
    /// Runtime imports needed by script (e.g., `"_defineComponent"`, `"_useCssVars"`).
    pub imports: Vec<&'static str>,
    /// Named/default user imports official marks `setup-maybe-ref` — inline
    /// template refs to these names bind `ref_key`/`ref: name` (see
    /// [`ScriptContext::ref_bindable_imports`]).
    pub ref_bindable_imports: FxHashSet<&'alloc str>,
    /// Every declared `__sfc__`→`_sfc_main` rename target, as UNRESOLVED
    /// markers — the caller resolves these with
    /// `ct.generated_content_range(marker)` only after every later edit
    /// (import hoisting, the inline-template `move_slice`) has been
    /// applied to `ct` and `ct.build_string()` has run. Never rediscovered
    /// by scanning the built script for the landmark text.
    pub(crate) sfc_binding_markers: Vec<GeneratedContentMarker<'alloc>>,
    /// The declared terminal default-export statement's own range, as an
    /// unresolved marker. `None` for a script with no default export to
    /// remove (a synthetic/no-script cell never reaches this producer at
    /// all — see `compile/mod.rs` and `compile/helpers.rs` for those).
    pub(crate) sfc_export_statement_marker: Option<GeneratedContentMarker<'alloc>>,
}

/// Process `<script>` and/or `<script setup>` blocks.
///
/// All transformations are applied via [`CodeGenOutput`] (batch overwrite/prepend).
/// The accumulated operations are applied to the [`CodeTransform`] in a single pass.
///
/// Returns binding metadata for template codegen and runtime imports.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn generate_script<'alloc>(
    script: Option<&RootNodeScript>,
    script_setup: Option<&RootNodeScript>,
    prepared: &PreparedScript<'alloc>,
    source: &'alloc str,
    ct: &mut CodeTransform<'alloc>,
    alloc: &'alloc Allocator,
    options: &ScriptCodeGenOptions<'_>,
) -> ScriptCodeGenResult<'alloc> {
    let mut ctx = ScriptContext {
        source,
        out: CodeGenOutput::new(alloc),
        bindings: FxHashMap::default(),
        binding_order: Vec::new(),
        imports: Vec::new(),
        inline_inject_pos: None,
        alloc,
        ref_bindable_imports: FxHashSet::default(),
    };

    // Official `@vue/compiler-sfc` wrapper gate: `isTS` when either script
    // block declares `lang="ts"` or `lang="tsx"` — TS components keep the
    // `_defineComponent` wrapper; JS components emit plain object literals
    // (or the `Object.assign` merge path when options exist). This affects
    // only the runtime (SFC→JS) output; the IDE/TSX lane has its own codegen.
    let is_ts = [script, script_setup].into_iter().flatten().any(|s| {
        matches!(
            s.lang,
            Some(crate::cursor::ScriptLanguage::TypeScript)
                | Some(crate::cursor::ScriptLanguage::TSX)
        )
    });

    match (script, script_setup) {
        (_, Some(setup)) => {
            // <script setup> present — this is the primary block
            process::process_script_setup(setup, prepared, &mut ctx, options, is_ts);
        }
        (Some(normal), None) => {
            // Only <script> (no setup) — Options API
            process::process_script_only(normal, prepared.companion(), &mut ctx, options);
        }
        (None, None) => {
            // No script blocks — nothing to do
        }
    }

    // Apply all accumulated operations to CodeTransform
    let codegen_imports = ctx.out.apply_to(ct);
    ctx.imports.extend(codegen_imports.vue);

    // Deduplicate imports (multiple defineModel/useSlots calls can push the same import)
    ctx.imports.sort_unstable();
    ctx.imports.dedup();

    ScriptCodeGenResult {
        bindings: ctx.bindings,
        inline_inject_pos: ctx.inline_inject_pos,
        imports: ctx.imports,
        ref_bindable_imports: ctx.ref_bindable_imports,
        sfc_binding_markers: codegen_imports.sfc_binding_markers,
        sfc_export_statement_marker: codegen_imports.sfc_export_statement_marker,
    }
}

#[cfg(test)]
mod tests;
