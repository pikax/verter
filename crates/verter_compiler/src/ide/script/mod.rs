//! TSX script generation.
//!
//! Generates the script portion of TSX output from `<script setup>` and `<script>` blocks.
//! Unlike the normal script codegen (which transforms macros into runtime code), this
//! preserves TypeScript types and macro call syntax for IDE type checking.
//!
//! ## Error Recovery
//!
//! OXC parses the original `<script setup>` content exactly ONCE. When it parses
//! cleanly, the full codegen path runs. When it has a genuine syntax error
//! (common while typing — e.g. an incomplete `a.`), there is NO reparse: a single
//! token scan of the REAL source ([`script_recover::ScriptTokenScanner::recover_plan`])
//! produces a [`script_recover::ScriptSetupRecoveryPlan`] whose original-span
//! import / macro / variable / function facts feed hoisting and binding
//! registration, and whose OUTPUT-ONLY member / expression holes and scope closers
//! keep the user's body valid TSX while it stays inside the `TemplateBindingFN`
//! wrapper. Synthetic recovery chunks are never treated as bindings, macros,
//! imports, or any other source fact.
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

use crate::ide::{IdeScriptOptions, TemplateComponentBindings};

use crate::compile::types::DestructuredBlockMeta;

mod comp_emit;
mod detectors;
pub(crate) mod event_inference;
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
pub(crate) use ts_assertions::rewrite_ts_type_assertions;
use type_constructs::{
    build_binding_source_info, collect_builtin_components, emit_attrs_type_aliases,
    emit_helper_imports, emit_helper_imports_with_define_component, emit_type_constructs,
};
use wrapper::{
    collect_global_component_fallbacks, directive_accessor_declaration,
    emit_global_component_fallbacks, emit_minimal_wrapper, instance_declaration,
    instance_declaration_ambient, instance_probe_line, public_facade_reexport,
    should_infer_function_types, to_pascal_case, PREFIX,
};

pub use type_constructs::{
    VERTER_TYPES_AMBIENT_MODULE, VERTER_TYPES_STANDALONE_DTS, VUE_JSX_RUNTIME_AUGMENTATION,
};

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
    /// Inventory of GlobalComponents fallback consts emitted into the templateBindingFN,
    /// shared with template event typing so a globally-registered component's `@event`
    /// payload resolves through the same in-scope const that was emitted for it.
    pub template_component_bindings: TemplateComponentBindings,
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
    // GlobalComponents fallback consts emitted into the templateBindingFN. Only the
    // `<script setup>` arm emits them; the options-API and no-script arms emit none.
    let mut global_component_fallbacks: Vec<String> = Vec::new();

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
                &mut global_component_fallbacks,
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

    // Emit the component's PUBLIC-FACADE re-export as a top-level statement on
    // the IDE carrier. A consumer importing the component must see its PUBLIC
    // `export default`, so the IDE carrier re-exports the public default from the
    // API carrier (`.verter.ts`, where the public default is synthesised). A bare
    // in-project `import Comp from "./Comp.vue"` resolves natively to the
    // `.d.vue.ts` declaration carrier (the IDE carrier itself is the
    // self-diagnostics surface).
    //
    // EXCEPTION — the Options-API script-only arm `(Some(script), None)` already
    // emits its OWN public `export default __sfc__` (the `defineComponent`-shape
    // component value), so it is already public-clean; appending a second
    // `export default` re-export there would be a DUPLICATE default export
    // (invalid TS/JS). Only the `<script setup>` arm and the no-script arm lack
    // an own default (the setup path REMOVES any companion `export default`), so
    // the facade re-export is needed exactly there.
    //
    // The facade is ADDITIVE — every template internal stays local
    // (non-exported). Emitted through `CodeTransform::append` (output-only,
    // unmapped) so the facade does not perturb any mapped source span, keeping
    // CodeTransform the single source of truth for the carrier text.
    let emits_own_public_default = matches!((script, script_setup), (Some(_), None));
    if !emits_own_public_default {
        ct.append(&public_facade_reexport(options.filename));
    }

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
        template_component_bindings: TemplateComponentBindings::new(global_component_fallbacks),
    }
}

#[cfg(test)]
#[path = "../script_partial_tests.rs"]
mod script_partial_tests;

#[cfg(test)]
mod tests;
