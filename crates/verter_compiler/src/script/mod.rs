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

use crate::code_transform::CodeTransform;
use crate::css::types::VBindVar;
use crate::parser::types::RootNodeScript;
use crate::script::prepared::PreparedScript;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::types::CodeGenOutput;

/// Options for script code generation.
#[derive(Debug, Clone, Default)]
pub struct ScriptCodeGenOptions<'a> {
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

/// Shared mutable context for script processing functions.
///
/// Bundles the common state passed through `process_script_setup`,
/// `process_script_only`, and `process_macro_item`.
pub struct ScriptContext<'alloc> {
    pub source: &'alloc str,
    pub out: CodeGenOutput<'alloc>,
    pub bindings: FxHashMap<&'alloc str, BindingType>,
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
    }
}

#[cfg(test)]
mod tests;
