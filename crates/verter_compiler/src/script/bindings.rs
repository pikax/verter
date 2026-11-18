//! The script lane's bindings: every name the template can reach in the
//! component's scripts, with the class the script frontend gives it.
//!
//! [`script_lane_bindings`] is the one producer. Script codegen seeds its
//! context from it, and the template facts read it, so the two can never see
//! different bindings for one component.

use oxc_allocator::Allocator;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_macro_dto::MacroRuntimeBundle;

use super::macros::{companion_binding_names, macro_binding_names};
use super::prepared::PreparedScript;
use super::process::is_ref_bindable_import;
use crate::parser::types::RootNodeScript;
use crate::template::binding::BindingType;
use crate::utils::oxc::vue::ScriptItem;

/// The bindings of a component's scripts.
#[derive(Debug, Default)]
pub struct ScriptLaneBindings<'alloc> {
    /// Name to class, for the template.
    pub bindings: FxHashMap<&'alloc str, BindingType>,
    /// `bindings`' setup-scope keys in first-seen declaration order: Vue's
    /// non-inline `__returned__` (`genSetupReturn`) keeps `allBindings`'
    /// insertion order, which a hash map cannot recover.
    pub binding_order: Vec<&'alloc str>,
    /// Named and default user imports Vue marks `setup-maybe-ref`: an inline
    /// template `ref="name"` to one binds `ref_key`/`ref: name`. Anything but
    /// a namespace import, a default `.vue` import or a `vue` import.
    pub ref_bindable_imports: FxHashSet<&'alloc str>,
}

/// The bindings of a component whose `<script setup>` block is
/// `script_setup` and whose script blocks were parsed into `prepared`.
/// `macro_runtime` is the validated runtime macro semantics, when supplied:
/// a type-based macro binds its prop names only through it.
///
/// - With a `<script setup>` that has content: the companion's runtime
///   imports are ref-bindable first, then the setup imports'; each macro's
///   prop names are props; the setup parse's bindings (but its `Props`, whose
///   spans mix coordinate systems, so the macro names above stand for them)
///   then take their own classes; and the companion's imports and top-level
///   declarations are `SetupImport`, unless setup declares the same name.
///   Imports in the companion are reachable from the template at run time
///   because the component factory merges both blocks.
/// - With only a `<script>` (Options API): its data, props, computed,
///   methods and injections, so the template uses `$data.`, `$props.` or
///   `_ctx.`.
/// - With an empty `<script setup>`, or no script: nothing.
pub fn script_lane_bindings<'alloc>(
    script_setup: Option<&RootNodeScript>,
    prepared: &PreparedScript<'alloc>,
    macro_runtime: Option<&MacroRuntimeBundle>,
    alloc: &'alloc Allocator,
) -> ScriptLaneBindings<'alloc> {
    let mut lane = ScriptLaneBindings::default();
    if script_setup.is_none() {
        if let Some(companion) = prepared.companion() {
            let content_str = companion.content_str();
            for (span, bt) in &companion.parse_result().bindings {
                lane.bindings
                    .insert(&content_str[span.start as usize..span.end as usize], *bt);
            }
        }
        return lane;
    }
    let Some(setup) = prepared.setup() else {
        return lane;
    };

    let (companion_names, companion_ref_bindable) = prepared
        .companion()
        .map(companion_binding_names)
        .unwrap_or_default();
    lane.ref_bindable_imports.extend(companion_ref_bindable);

    let parse_result = setup.parse_result();
    let mut macro_index = 0_u32;
    for item in &parse_result.items {
        match item {
            ScriptItem::Import(imp) if !imp.is_type_only => {
                lane.ref_bindable_imports.extend(
                    imp.bindings
                        .iter()
                        .filter(|binding| {
                            !binding.is_type_only
                                && is_ref_bindable_import(imp.source, binding.import_kind)
                        })
                        .map(|binding| binding.name),
                );
            }
            ScriptItem::Macro(mac) => {
                macro_binding_names(mac, macro_index, macro_runtime, &mut |name| {
                    lane.bindings
                        .insert(alloc.alloc_str(name), BindingType::Props);
                });
                macro_index = macro_index.saturating_add(1);
            }
            _ => {}
        }
    }

    let content_str = setup.content_str();
    for (span, bt) in &parse_result.bindings {
        if *bt == BindingType::Props {
            continue;
        }
        let name = &content_str[span.start as usize..span.end as usize];
        if lane.bindings.insert(name, *bt).is_none() {
            lane.binding_order.push(name);
        }
    }

    for name in companion_names {
        if let std::collections::hash_map::Entry::Vacant(entry) = lane.bindings.entry(name) {
            entry.insert(BindingType::SetupImport);
            lane.binding_order.push(name);
        }
    }
    lane
}
