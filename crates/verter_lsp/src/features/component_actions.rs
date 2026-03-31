// Component code actions: add missing props and v-models to child component definitions.
//
// When a parent component passes a prop or v-model that the child doesn't define,
// this module generates cross-file code actions to add the definitions.
//
// When a parent has bindings that match child prop names but aren't passed yet,
// this module generates same-file code actions to add the bindings as props.

use std::collections::HashSet;

use tower_lsp_server::ls_types::*;
use verter_analysis::types::AnalyzedMacroKind;

use crate::documents::line_index::LineIndex;
use crate::features::action_utils;
use crate::features::component_diagnostics;
use crate::features::cross_file::ChildComponentContext;
use crate::features::macro_codegen::MacroCodegen;

/// Generate code actions for unknown prop diagnostics.
///
/// For each unknown prop, generates a cross-file edit to add the prop
/// to the child component's `defineProps`.
pub fn component_code_actions(
    analysis: &verter_session::FileAnalysisSnapshot,
    resolve_child_context: &dyn Fn(&str) -> Option<ChildComponentContext>,
) -> Vec<CodeActionOrCommand> {
    let unknowns = component_diagnostics::find_unknown_props(analysis, &|source| {
        resolve_child_context(source).map(|ctx| ctx.analysis.clone())
    });

    let mut actions = Vec::new();

    for info in &unknowns {
        let ctx = match resolve_child_context(&info.import_source) {
            Some(ctx) => ctx,
            None => continue,
        };

        // Skip if child has no <script setup>
        if ctx.script_setup().is_none() {
            continue;
        }

        let prop_name = &info.prop_name;
        let edit = if let Some(mac) = ctx.find_macro(AnalyzedMacroKind::DefineProps) {
            if mac.is_type_based {
                // Insert into existing type-based defineProps<{...}>()
                let member_text = MacroCodegen::define_props()
                    .add_type_member(prop_name, "unknown", false)
                    .build_member_insertion();
                ctx.make_insert_into_macro(AnalyzedMacroKind::DefineProps, &member_text)
            } else {
                // Runtime-based defineProps — can't insert type members
                None
            }
        } else {
            // No defineProps exists — generate a new one
            let macro_text = MacroCodegen::define_props()
                .add_type_member(prop_name, "unknown", false)
                .build();
            ctx.make_insert_at_macros(&macro_text)
        };

        if let Some(edit) = edit {
            let title = format!("Add prop '{}' to <{}>", prop_name, info.component_name);
            actions.push(action_utils::make_code_action(
                title,
                CodeActionKind::QUICKFIX,
                edit,
                false,
                None,
            ));
        }
    }

    // Handle the case where a child has NO defineProps at all (empty prop definitions).
    // `find_unknown_props` skips these to avoid false-positive diagnostics, but for
    // code actions we want to offer "generate defineProps" for each parent-passed prop.
    if let Some(template) = &analysis.template {
        for comp in &template.components {
            if comp.is_dynamic || comp.has_spread || comp.props.is_empty() {
                continue;
            }
            let import_source = match comp.import_source.as_deref() {
                Some(s) => s,
                None => continue,
            };
            let ctx = match resolve_child_context(import_source) {
                Some(ctx) => ctx,
                None => continue,
            };
            if ctx.script_setup().is_none() {
                continue;
            }
            // Only handle the case where child has NO defineProps macro at all
            if ctx.find_macro(AnalyzedMacroKind::DefineProps).is_some() {
                continue;
            }
            // Also skip if child already has prop definitions from other sources
            if ctx
                .analysis
                .template
                .as_ref()
                .is_some_and(|t| !t.prop_definitions.is_empty())
            {
                continue;
            }
            // Generate a defineProps with ALL parent-passed props
            let mut codegen = MacroCodegen::define_props();
            for prop in &comp.props {
                codegen = codegen.add_type_member(&prop.name, "unknown", false);
            }
            let macro_text = codegen.build();
            if let Some(edit) = ctx.make_insert_at_macros(&macro_text) {
                let title = if comp.props.len() == 1 {
                    format!("Add prop '{}' to <{}>", comp.props[0].name, comp.name)
                } else {
                    format!("Add {} props to <{}>", comp.props.len(), comp.name)
                };
                actions.push(action_utils::make_code_action(
                    title,
                    CodeActionKind::QUICKFIX,
                    edit,
                    false,
                    None,
                ));
            }
        }
    }

    // V-model actions: add missing defineModel to child
    let unknown_models = component_diagnostics::find_unknown_models(analysis, &|source| {
        resolve_child_context(source).map(|ctx| ctx.analysis.clone())
    });

    for info in &unknown_models {
        let ctx = match resolve_child_context(&info.import_source) {
            Some(ctx) => ctx,
            None => continue,
        };

        if ctx.script_setup().is_none() {
            continue;
        }

        let model_name = if info.model_name == "modelValue" {
            None
        } else {
            Some(info.model_name.as_str())
        };
        let macro_text = MacroCodegen::define_model(model_name).build();
        if let Some(edit) = ctx.make_insert_at_macros(&macro_text) {
            let title = format!(
                "Add defineModel('{}') to <{}>",
                info.model_name, info.component_name
            );
            actions.push(action_utils::make_code_action(
                title,
                CodeActionKind::QUICKFIX,
                edit,
                false,
                None,
            ));
        }
    }

    actions
}

/// Convert a kebab-case string to camelCase.
fn kebab_to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Find the byte offset just before `>` or `/>` in the opening tag of a
/// component, scanning from `span_start` in the source.
///
/// Returns `None` if no valid insertion point is found.
fn find_opening_tag_end(source: &str, span_start: u32) -> Option<u32> {
    let bytes = source.as_bytes();
    let start = span_start as usize;
    let mut i = start;
    let mut in_quote = false;
    let mut quote_char: u8 = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if in_quote {
            if b == quote_char {
                in_quote = false;
            }
        } else if b == b'"' || b == b'\'' {
            in_quote = true;
            quote_char = b;
        } else if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            // Self-closing `/>` — insert before the `/`
            return Some(i as u32);
        } else if b == b'>' {
            // Normal close `>` — insert before the `>`
            return Some(i as u32);
        }
        i += 1;
    }
    None
}

/// Generate code actions to add matching props from parent bindings to child
/// component tags.
///
/// For each component usage, resolves the child to get its prop definitions.
/// Then finds child props that are:
///   - Not already passed by the parent
///   - Have a matching binding or import in the parent's scope
///
/// Produces insertion edits before the component's `>` or `/>`.
pub fn suggest_matching_props(
    analysis: &verter_session::FileAnalysisSnapshot,
    source: &str,
    line_index: &LineIndex,
    uri: &Uri,
    resolve_child_context: &dyn Fn(&str) -> Option<ChildComponentContext>,
) -> Vec<CodeActionOrCommand> {
    let template = match analysis.template.as_ref() {
        Some(t) => t,
        None => return vec![],
    };

    // Build a set of available binding names (camelCase) from the parent scope.
    // Exclude type-only imports.
    let mut available_bindings: HashSet<String> = HashSet::new();
    for binding in &analysis.bindings {
        available_bindings.insert(binding.name.clone());
    }
    for import in &analysis.imports {
        if import.is_type_only {
            continue;
        }
        for ib in &import.bindings {
            if !ib.is_type_only {
                available_bindings.insert(ib.name.clone());
            }
        }
    }

    if available_bindings.is_empty() {
        return vec![];
    }

    let mut actions = Vec::new();

    for usage in &template.components {
        // Skip dynamic components and those with v-bind spread
        if usage.is_dynamic || usage.has_spread {
            continue;
        }

        let import_source = match usage.import_source.as_deref() {
            Some(s) => s,
            None => continue,
        };

        let child_ctx = match resolve_child_context(import_source) {
            Some(ctx) => ctx,
            None => continue,
        };

        // Skip if child has no <script setup>
        if child_ctx.script_setup().is_none() {
            continue;
        }

        let child_template = match child_ctx.analysis.template.as_ref() {
            Some(t) => t,
            None => continue,
        };

        // Collect already-passed prop names (camelCase normalized)
        let already_passed: HashSet<String> = usage
            .props
            .iter()
            .map(|p| kebab_to_camel(&p.name))
            .collect();

        // Find child props that are missing from parent usage and have matching bindings
        let mut matching: Vec<(&str, &str)> = Vec::new(); // (prop_name, binding_name)

        for prop_def in &child_template.prop_definitions {
            let prop_camel = kebab_to_camel(&prop_def.name);

            if already_passed.contains(&prop_camel) {
                continue;
            }

            // Look for a matching binding in parent scope
            if available_bindings.contains(&prop_camel) {
                matching.push((&prop_def.name, &prop_def.name));
            }
        }

        if matching.is_empty() {
            continue;
        }

        // Find insertion point: before `>` or `/>` of the opening tag
        let insert_offset = match find_opening_tag_end(source, usage.span.start) {
            Some(off) => off,
            None => continue,
        };

        let insert_pos = match line_index.offset_to_position(insert_offset) {
            Some(pos) => pos,
            None => continue,
        };

        // Build insertion text: ` :propName="bindingName"` for each match
        // Use shorthand `:propName` when prop name equals binding name (always
        // true in our case since we match on exact name)
        let mut insert_text = String::new();
        for (prop_name, _binding_name) in &matching {
            insert_text.push_str(&format!(" :{}", prop_name));
        }

        // Build individual actions per prop, plus a bulk "add all" action
        if matching.len() > 1 {
            let title = format!("Add {} matching props to <{}>", matching.len(), usage.name);
            let edit = action_utils::make_insert_edit(uri, insert_pos, insert_text.clone());
            actions.push(action_utils::make_code_action(
                title,
                CodeActionKind::QUICKFIX,
                edit,
                false,
                None,
            ));
        }

        // Individual actions
        for (prop_name, _binding_name) in &matching {
            let single_text = format!(" :{}", prop_name);
            let edit = action_utils::make_insert_edit(uri, insert_pos, single_text);
            let title = format!("Add prop ':{}' to <{}>", prop_name, usage.name);
            actions.push(action_utils::make_code_action(
                title,
                CodeActionKind::QUICKFIX,
                edit,
                matching.len() == 1, // Preferred only when there's a single match
                None,
            ));
        }
    }

    actions
}

#[cfg(test)]
#[path = "component_actions_tests.rs"]
mod component_actions_tests;
