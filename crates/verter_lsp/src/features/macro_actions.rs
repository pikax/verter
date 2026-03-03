// Macro code actions: generate/augment defineSlots and defineEmits from template usage.

use tower_lsp_server::lsp_types::*;
use verter_analysis::types::{AnalysisFlags, AnalyzedMacroKind};
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;
use crate::features::action_utils::{find_script_insert_offset, make_insert_action, needs_quoting};

/// Produce macro code actions based on template analysis vs script macros.
///
/// Returns actions for:
/// - B1: Generate `defineSlots` from template `<slot>` tags
/// - B2: Generate `defineEmits` from undeclared emit usage
/// - B3: Add missing slots to existing `defineSlots`
/// - B4: Add missing emits to existing `defineEmits`
pub fn macro_code_actions(
    source: &str,
    analysis: Option<&FileAnalysisSnapshot>,
    blocks: &[SfcBlock],
    line_index: &LineIndex,
) -> Vec<CodeActionOrCommand> {
    let analysis = match analysis {
        Some(a) => a,
        None => return vec![],
    };

    // Must have a <script setup> block
    let setup_block = match blocks.iter().find(|b| b.is_setup()) {
        Some(b) => b,
        None => return vec![],
    };

    let flags = AnalysisFlags::from_bits_truncate(analysis.script_flags);
    let template = match &analysis.template {
        Some(t) => t,
        None => return vec![],
    };

    let mut actions = Vec::new();

    // B1: Generate defineSlots (no existing defineSlots, template has <slot> tags)
    if !flags.contains(AnalysisFlags::HAS_DEFINE_SLOTS) && !template.defined_slots.is_empty() {
        if let Some(action) =
            generate_define_slots_action(source, analysis, setup_block, template, line_index)
        {
            actions.push(action);
        }
    }

    // B2: Generate defineEmits (no existing defineEmits, template has undeclared emits)
    if !flags.contains(AnalysisFlags::HAS_DEFINE_EMITS) {
        let undeclared: Vec<&str> = template
            .emit_definitions
            .iter()
            .filter(|e| !e.is_declared)
            .map(|e| e.event_name.as_str())
            .collect();
        if !undeclared.is_empty() {
            if let Some(action) =
                generate_define_emits_action(source, analysis, setup_block, &undeclared, line_index)
            {
                actions.push(action);
            }
        }
    }

    // B3: Add missing slots to existing defineSlots
    if flags.contains(AnalysisFlags::HAS_DEFINE_SLOTS) {
        if let Some(action) = add_missing_slots_action(source, analysis, template, line_index) {
            actions.push(action);
        }
    }

    // B4: Add missing emits to existing defineEmits
    if flags.contains(AnalysisFlags::HAS_DEFINE_EMITS) {
        if let Some(action) = add_missing_emits_action(source, analysis, template, line_index) {
            actions.push(action);
        }
    }

    actions
}

// ── B1: Generate defineSlots ─────────────────────────────────────────────

fn generate_define_slots_action(
    source: &str,
    analysis: &FileAnalysisSnapshot,
    setup_block: &SfcBlock,
    template: &verter_analysis::template::TemplateAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<CodeActionOrCommand> {
    let slots = &template.defined_slots;
    if slots.is_empty() {
        return None;
    }

    // Build the type literal for defineSlots
    let mut type_members = String::new();
    for slot in slots {
        type_members.push_str("    ");
        // Quote the slot name if it contains special characters
        let name = if needs_quoting(&slot.name) {
            format!("'{}'", slot.name)
        } else {
            slot.name.clone()
        };
        type_members.push_str(&name);
        type_members.push_str("(props: {");
        if slot.has_bindings && !slot.binding_names.is_empty() {
            for (i, binding) in slot.binding_names.iter().enumerate() {
                if i > 0 {
                    type_members.push_str(", ");
                }
                type_members.push(' ');
                type_members.push_str(binding);
                type_members.push_str(": unknown");
            }
            type_members.push(' ');
        }
        type_members.push_str("}): any\n");
    }

    let insert_text = format!("defineSlots<{{\n{}}}>()\n", type_members);
    let insert_offset = find_script_insert_offset(source, analysis, setup_block);
    let position = line_index.offset_to_position(insert_offset)?;

    Some(make_insert_action(
        "Generate defineSlots from template",
        CodeActionKind::QUICKFIX,
        &insert_text,
        position,
    ))
}

// ── B2: Generate defineEmits ─────────────────────────────────────────────

fn generate_define_emits_action(
    source: &str,
    analysis: &FileAnalysisSnapshot,
    setup_block: &SfcBlock,
    undeclared: &[&str],
    line_index: &LineIndex,
) -> Option<CodeActionOrCommand> {
    if undeclared.is_empty() {
        return None;
    }

    // Build type-based defineEmits
    let mut type_members = String::new();
    for event in undeclared {
        type_members.push_str("    (e: '");
        type_members.push_str(event);
        type_members.push_str("', ...args: any[]): void\n");
    }

    let insert_text = format!("const emit = defineEmits<{{\n{}}}>()\n", type_members);
    let insert_offset = find_script_insert_offset(source, analysis, setup_block);
    let position = line_index.offset_to_position(insert_offset)?;

    Some(make_insert_action(
        "Generate defineEmits from template usage",
        CodeActionKind::QUICKFIX,
        &insert_text,
        position,
    ))
}

// ── B3: Add missing slots to existing defineSlots ────────────────────────

fn add_missing_slots_action(
    _source: &str,
    analysis: &FileAnalysisSnapshot,
    template: &verter_analysis::template::TemplateAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<CodeActionOrCommand> {
    // Find the defineSlots macro
    let slots_macro = analysis
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)?;

    // Find which template slots are missing from the macro's slot definitions
    let existing_slot_names: Vec<&str> = template
        .defined_slots
        .iter()
        .filter(|s| s.span.start == 0 && s.span.end == 0)
        .map(|s| s.name.as_str())
        .collect();

    // Collect slots that exist in template but might not be in the macro
    // We compare template.defined_slots against macro emits
    let macro_slots = analysis
        .macros
        .iter()
        .filter(|m| m.kind == AnalyzedMacroKind::DefineSlots)
        .flat_map(|m| {
            // The macro's type_references contain the slot names if extracted
            m.type_references.iter().map(|s| s.as_str())
        })
        .collect::<std::collections::HashSet<_>>();

    let missing: Vec<_> = template
        .defined_slots
        .iter()
        .filter(|s| !macro_slots.contains(s.name.as_str()))
        .collect();

    // Can't reliably detect existing slots without full type analysis of the macro body.
    // Only if we have macro emits in the analysis can we check.
    // For now, use the simpler heuristic: if the template defines slots and there's a
    // defineSlots macro, check if template has more slots than we can find declared.
    if missing.is_empty() || existing_slot_names.len() == template.defined_slots.len() {
        return None;
    }

    // Build the new members to insert before the closing `}>`
    let mut new_members = String::new();
    for slot in &missing {
        new_members.push_str("    ");
        let name = if needs_quoting(&slot.name) {
            format!("'{}'", slot.name)
        } else {
            slot.name.clone()
        };
        new_members.push_str(&name);
        new_members.push_str("(props: {");
        if slot.has_bindings && !slot.binding_names.is_empty() {
            for (i, binding) in slot.binding_names.iter().enumerate() {
                if i > 0 {
                    new_members.push_str(", ");
                }
                new_members.push(' ');
                new_members.push_str(binding);
                new_members.push_str(": unknown");
            }
            new_members.push(' ');
        }
        new_members.push_str("}): any\n");
    }

    // Insert just before the macro's span_end (the closing `)`)
    // We want to insert before `}>()` — approximate by inserting at span_end - 4
    let insert_offset = if slots_macro.span.end >= 4 {
        slots_macro.span.end - 4
    } else {
        slots_macro.span.end
    };
    let position = line_index.offset_to_position(insert_offset)?;

    let title = if missing.len() == 1 {
        format!("Add slot '{}' to defineSlots", missing[0].name)
    } else {
        format!("Add {} missing slots to defineSlots", missing.len())
    };

    Some(make_insert_action(
        &title,
        CodeActionKind::QUICKFIX,
        &new_members,
        position,
    ))
}

// ── B4: Add missing emits to existing defineEmits ────────────────────────

fn add_missing_emits_action(
    _source: &str,
    analysis: &FileAnalysisSnapshot,
    template: &verter_analysis::template::TemplateAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<CodeActionOrCommand> {
    // Find the defineEmits macro
    let emits_macro = analysis
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineEmits)?;

    // Find undeclared emits
    let undeclared: Vec<&str> = template
        .emit_definitions
        .iter()
        .filter(|e| !e.is_declared)
        .map(|e| e.event_name.as_str())
        .collect();

    if undeclared.is_empty() {
        return None;
    }

    if emits_macro.is_type_based {
        // Type-based: insert new call signatures before `}>`
        let mut new_members = String::new();
        for event in &undeclared {
            new_members.push_str("    (e: '");
            new_members.push_str(event);
            new_members.push_str("', ...args: any[]): void\n");
        }

        let insert_offset = if emits_macro.span.end >= 4 {
            emits_macro.span.end - 4
        } else {
            emits_macro.span.end
        };
        let position = line_index.offset_to_position(insert_offset)?;

        let title = if undeclared.len() == 1 {
            format!("Add emit '{}' to defineEmits", undeclared[0])
        } else {
            format!("Add {} missing emits to defineEmits", undeclared.len())
        };

        Some(make_insert_action(
            &title,
            CodeActionKind::QUICKFIX,
            &new_members,
            position,
        ))
    } else {
        // Runtime array form: defineEmits(['existing', ...])
        // Insert new strings before the closing `]`
        let new_entries: String = undeclared.iter().map(|e| format!(", '{}'", e)).collect();

        // Insert before the `]` in the array — approximate at span_end - 2
        let insert_offset = if emits_macro.span.end >= 2 {
            emits_macro.span.end - 2
        } else {
            emits_macro.span.end
        };
        let position = line_index.offset_to_position(insert_offset)?;

        let title = if undeclared.len() == 1 {
            format!("Add emit '{}' to defineEmits", undeclared[0])
        } else {
            format!("Add {} missing emits to defineEmits", undeclared.len())
        };

        Some(make_insert_action(
            &title,
            CodeActionKind::QUICKFIX,
            &new_entries,
            position,
        ))
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────
// find_insert_offset, needs_quoting, make_insert_action are now in action_utils.rs

#[cfg(test)]
#[path = "macro_actions_tests.rs"]
mod macro_actions_tests;
