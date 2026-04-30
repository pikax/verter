//! TSX script generation.
//!
//! Generates the script portion of TSX output from `<script setup>` and `<script>` blocks.
//! Unlike the normal script codegen (which transforms macros into runtime code), this
//! preserves TypeScript types and macro call syntax for IDE type checking.
//!
//! ## Error Recovery
//!
//! When OXC encounters parse errors (common during typing), a truncate-and-reparse
//! strategy recovers as much IDE functionality as possible:
//!
//! 1. Find the earliest error offset from OXC diagnostics.
//! 2. Truncate source at the last newline before that offset — the "clean prefix".
//! 3. Re-parse only the clean prefix (which succeeds since the error is removed).
//! 4. Use the clean prefix AST for normal codegen (import hoisting, binding extraction,
//!    macro processing), while the broken tail passes through unchanged.
//!
//! A lightweight token scanner ([`script_recover::ScriptTokenScanner`]) recovers
//! macro binding names from the broken tail so template resolution still works.
//!
//! ## Output structure
//!
//! For `<script setup>`:
//! ```tsx
//! // Hoisted imports
//! import { ref } from 'vue'
//! import type { Props } from './types'
//!
//! // Hoisted type declarations
//! interface Foo { ... }
//!
//! // Temp variable (outside block scope to avoid TDZ)
//! const ___VERTER___unwrapped = ___VERTER___shallowUnwrapRef({
//!     /** My counter */
//!     count: count as unknown as typeof count,
//! });
//!
//! // Exported TemplateBinding wrapper function
//! ;export function ___VERTER___TemplateBindingFN() {
//!   // Setup body (macros boxed, bindings extracted)
//!   ;type ___VERTER___defineProps_Type=___VERTER___Prettify<Props>;
//!   const props = defineProps<___VERTER___defineProps_Type>()
//!   const count = ref(0)
//!
//!   // Block scope: destructure from temp with offset comments, then template JSX
//!   { /* verter-destructured-start */const {
//!     /*45,50*/
//!     count } = ___VERTER___unwrapped; /* verter-destructured-end */
//!     <div>{ count }</div>
//!   } // close block scope
//! } // close templateBindingFN
//! ```

use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;

use crate::ast::types::TemplateAst;
use crate::code_transform::CodeTransform;
use crate::parser::types::RootNodeScript;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::types::CodeGenOutput;

use crate::ide::IdeScriptOptions;

// Re-export from compile::types for internal use (Phase 11d public-surface
// preservation contract: callers may resolve these via
// `crate::ide::script::DestructuredBindingInfo` / `…::DestructuredBlockMeta`).
#[allow(unused_imports)]
pub use crate::compile::types::{DestructuredBindingInfo, DestructuredBlockMeta};

mod comp_emit;
mod detectors;
mod event_inference;
mod macros;
mod options_api;
mod recovery;
mod setup;
mod template_ref;
mod ts_assertions;
mod type_constructs;
mod wrapper;

// Pull every sibling-internal symbol that any sibling resolves via
// `super::<name>` into the script module's namespace. This single
// surface lets each sibling reach the others without spelling
// `crate::ide::script::other_sibling::foo` paths repeatedly.
use comp_emit::{emit_comp_functions_to_string, emit_get_root_component_to_string};
use detectors::{detect_get_current_instance, detect_use_attrs_calls};
use event_inference::{apply_event_handler_param_inference, kebab_to_pascal_case};
use macros::{process_macros, MacroSourceCtx};
use options_api::{process_companion_for_tsx, process_tsx_script_only};
use setup::process_tsx_script_setup;
use template_ref::{
    apply_template_ref_call_inference, callee_identifier_name, collect_binding_names,
};
use ts_assertions::rewrite_ts_type_assertions;
use type_constructs::{
    build_binding_source_info, collect_builtin_components, emit_attrs_type_aliases,
    emit_helper_imports, emit_helper_imports_with_define_component, emit_type_constructs,
};
use wrapper::{
    directive_accessor_declaration, emit_global_component_fallbacks, emit_minimal_wrapper,
    instance_declaration, instance_declaration_ambient, instance_probe_line,
    should_infer_function_types, to_pascal_case, PREFIX,
};

pub use type_constructs::{VERTER_TYPES_AMBIENT_MODULE, VERTER_TYPES_STANDALONE_DTS};

#[cfg(test)]
use comp_emit::resolve_all_prop_refs_in_expr;
#[cfg(test)]
use macros::is_simple_type_reference;

/// Result of TSX script generation (internal, before building string).
pub struct IdeScriptGenResult<'alloc> {
    /// Binding metadata for template TSX generation.
    pub bindings: FxHashMap<&'alloc str, BindingType>,
    /// Type constructs to append after the combined TSX code (no sourcemap).
    /// Concatenated by the caller after source map combination.
    pub type_constructs: String,
    /// Deferred return statement + function close for unified CT mode.
    /// When `template_end` is `Some(...)`, this contains the return+close string
    /// to be applied to the CT AFTER template codegen (to avoid interleaving).
    pub return_close: Option<String>,
    /// Position at which `return_close` should be inserted.
    /// For script-first SFCs this equals `template_end`; for template-first SFCs
    /// this equals `script_close.end` (whichever block ends last in the source).
    pub return_close_pos: Option<u32>,
    /// Structured metadata for the destructured block, if present.
    pub destructured_block: Option<DestructuredBlockMeta>,
}

/// Generate TSX script output from script blocks.
///
/// Returns the generated code, source map, and bindings for template generation.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn generate_ide_script<'alloc>(
    script: Option<&RootNodeScript>,
    script_setup: Option<&RootNodeScript>,
    template_ast: Option<&TemplateAst>,
    source: &'alloc str,
    ct: &mut CodeTransform<'alloc>,
    alloc: &'alloc Allocator,
    options: &IdeScriptOptions<'_>,
    template_end: Option<u32>,
) -> IdeScriptGenResult<'alloc> {
    let mut out = CodeGenOutput::new(alloc);
    let mut bindings = FxHashMap::default();
    let mut type_constructs = String::new();
    let builtin_components = collect_builtin_components(template_ast, source);
    let mut return_close: Option<String> = None;

    let mut destructured_block: Option<DestructuredBlockMeta> = None;

    match (script, script_setup) {
        (_, Some(setup)) => {
            let result = process_tsx_script_setup(
                setup,
                script,
                template_ast,
                source,
                ct,
                &mut out,
                &mut bindings,
                &mut type_constructs,
                alloc,
                options,
                &builtin_components,
                template_end,
            );
            return_close = result.0;
            destructured_block = result.1;
        }
        (Some(normal), None) => {
            process_tsx_script_only(
                normal,
                template_ast,
                source,
                &mut out,
                &mut bindings,
                &mut type_constructs,
                alloc,
                options,
                &builtin_components,
            );
        }
        (None, None) => {
            // No script blocks — emit minimal wrapper + full type constructs.
            // Imports must come BEFORE the function wrapper (TS1232: imports
            // can only appear at the top level of a module).
            emit_helper_imports(&mut out, 0, options, &builtin_components, template_ast);
            return_close = emit_minimal_wrapper(&mut out, options, 0, template_end);
            emit_type_constructs(
                &mut type_constructs,
                &None, // no generics
                &None, // no attrs
                source,
                options,
                false, // no getCurrentInstance
                false, // no Comp functions → skip attributes type
            );
        }
    }

    // Apply accumulated operations
    out.apply_to(ct);

    // Compute the correct insertion position for return_close.
    // Must be after BOTH the template and all script blocks in the source.
    let return_close_pos = if return_close.is_some() {
        let mut pos = template_end.unwrap_or(0);
        if let Some(setup) = script_setup {
            if let Some(tc) = &setup.tag_close {
                pos = pos.max(tc.end);
            }
        }
        if let Some(normal) = script {
            if let Some(tc) = &normal.tag_close {
                pos = pos.max(tc.end);
            }
        }
        Some(pos)
    } else {
        None
    };

    IdeScriptGenResult {
        bindings,
        type_constructs,
        return_close,
        return_close_pos,
        destructured_block,
    }
}

#[cfg(test)]
#[path = "../script_partial_tests.rs"]
mod script_partial_tests;

#[cfg(test)]
mod tests;
