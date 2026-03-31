// Macro code actions: generate/augment defineSlots and defineEmits from template usage.

use tower_lsp_server::ls_types::*;
use verter_analysis::types::{AnalysisFlags, AnalyzedBinding, AnalyzedMacroKind};
use verter_session::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;
use crate::features::action_utils::{find_script_insert_offset, make_insert_action, needs_quoting};

/// Context for resolving slot binding types.
///
/// Tier 1: look up expression in `bindings` by name, use `type_annotation`.
/// Tier 2 (future): fall back to TypeProvider hover for complex expressions.
pub struct SlotTypeContext<'a> {
    /// Script bindings from analysis (Tier 1).
    pub bindings: &'a [AnalyzedBinding],
    /// TypeProvider + mapping context (Tier 2 fallback, optional).
    pub type_provider: Option<()>, // placeholder for TypeProviderRef
}

/// Resolve the TypeScript type for a slot binding expression.
///
/// Tier 1: look up `expression` in `bindings` by name, return `type_annotation`.
/// Falls back to `"unknown"` if no match or no annotation.
fn resolve_binding_type(ctx: Option<&SlotTypeContext<'_>>, expression: &str) -> String {
    if let Some(ctx) = ctx {
        // Tier 1: direct binding lookup
        if let Some(binding) = ctx.bindings.iter().find(|b| b.name == expression) {
            if let Some(ref ann) = binding.type_annotation {
                return ann.clone();
            }
        }
        // Tier 2: TypeProvider fallback (future)
        // if let Some(ref tp) = ctx.type_provider { ... }
    }
    "unknown".to_string()
}

/// Build the slot member text for a single slot in the defineSlots type literal.
fn build_slot_member(
    slot: &verter_analysis::template::DefinedSlot,
    type_ctx: Option<&SlotTypeContext<'_>>,
) -> String {
    let mut member = String::new();
    member.push_str("    ");
    // Quote the slot name if it contains special characters
    let name = if needs_quoting(&slot.name) {
        format!("'{}'", slot.name)
    } else {
        slot.name.clone()
    };
    member.push_str(&name);
    member.push_str("(props: {");
    if slot.has_bindings && !slot.binding_names.is_empty() {
        for (i, binding) in slot.binding_names.iter().enumerate() {
            if i > 0 {
                member.push(',');
            }
            member.push(' ');
            member.push_str(binding);
            member.push_str(": ");
            // Resolve type from binding expression
            let expr = slot
                .binding_expressions
                .get(i)
                .map(|s| s.as_str())
                .unwrap_or(binding.as_str());
            member.push_str(&resolve_binding_type(type_ctx, expr));
        }
        member.push(' ');
    }
    member.push_str("}): any\n");
    member
}

/// Produce macro code actions based on template analysis vs script macros.
///
/// Returns actions for:
/// - B1: Generate `defineSlots` from template `<slot>` tags
/// - B2: Generate `defineEmits` from undeclared emit usage
/// - B3: Add missing slots to existing `defineSlots`
/// - B4: Add missing emits to existing `defineEmits`
///
/// When `cursor_offset` is `Some`, only actions relevant to the cursor position
/// are returned. Slot actions appear when the cursor is on a `<slot>` element or
/// on the `defineSlots` macro. Emit actions appear when the cursor is on an undeclared
/// emit usage or on the `defineEmits` macro. When `None`, all actions are returned.
pub fn macro_code_actions(
    source: &str,
    analysis: Option<&FileAnalysisSnapshot>,
    blocks: &[SfcBlock],
    line_index: &LineIndex,
    cursor_offset: Option<u32>,
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

    // Build type context from analysis bindings
    let type_ctx = SlotTypeContext {
        bindings: &analysis.bindings,
        type_provider: None,
    };

    // Cursor-based filtering: determine which action categories are relevant.
    let (show_slot_actions, show_emit_actions) = match cursor_offset {
        None => (true, true), // No cursor → show all
        Some(offset) => {
            let on_slot_element = template
                .defined_slots
                .iter()
                .any(|s| offset >= s.span.start && offset <= s.span.end);
            let on_define_slots = analysis.macros.iter().any(|m| {
                m.kind == AnalyzedMacroKind::DefineSlots
                    && offset >= m.span.start
                    && offset <= m.span.end
            });
            let on_emit_usage = template
                .emit_definitions
                .iter()
                .filter(|e| !e.is_declared)
                .any(|e| offset >= e.span.start && offset <= e.span.end);
            let on_define_emits = analysis.macros.iter().any(|m| {
                m.kind == AnalyzedMacroKind::DefineEmits
                    && offset >= m.span.start
                    && offset <= m.span.end
            });
            (
                on_slot_element || on_define_slots,
                on_emit_usage || on_define_emits,
            )
        }
    };

    let mut actions = Vec::new();

    // B1: Generate defineSlots (no existing defineSlots, template has <slot> tags)
    if show_slot_actions
        && !flags.contains(AnalysisFlags::HAS_DEFINE_SLOTS)
        && !template.defined_slots.is_empty()
    {
        if let Some(action) = generate_define_slots_action(
            source,
            analysis,
            setup_block,
            template,
            line_index,
            &type_ctx,
        ) {
            actions.push(action);
        }
    }

    // B2: Generate defineEmits (no existing defineEmits, template has undeclared emits)
    if show_emit_actions && !flags.contains(AnalysisFlags::HAS_DEFINE_EMITS) {
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
    if show_slot_actions && flags.contains(AnalysisFlags::HAS_DEFINE_SLOTS) {
        if let Some(action) =
            add_missing_slots_action(source, analysis, template, line_index, &type_ctx)
        {
            actions.push(action);
        }
    }

    // B5: Prop mismatch detection (missing props in defineSlots)
    if show_slot_actions && flags.contains(AnalysisFlags::HAS_DEFINE_SLOTS) {
        actions.extend(prop_mismatch_actions(
            source, analysis, template, line_index, &type_ctx,
        ));
    }

    // B4: Add missing emits to existing defineEmits
    if show_emit_actions && flags.contains(AnalysisFlags::HAS_DEFINE_EMITS) {
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
    type_ctx: &SlotTypeContext<'_>,
) -> Option<CodeActionOrCommand> {
    let slots = &template.defined_slots;
    if slots.is_empty() {
        return None;
    }

    // Build the type literal for defineSlots
    let mut type_members = String::new();
    for slot in slots {
        type_members.push_str(&build_slot_member(slot, Some(type_ctx)));
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
    source: &str,
    analysis: &FileAnalysisSnapshot,
    template: &verter_analysis::template::TemplateAnalysisSnapshot,
    line_index: &LineIndex,
    type_ctx: &SlotTypeContext<'_>,
) -> Option<CodeActionOrCommand> {
    // Find the defineSlots macro
    let slots_macro = analysis
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)?;

    // Parse existing slot names from the defineSlots source text
    let macro_text = &source[slots_macro.span.start as usize..slots_macro.span.end as usize];
    let existing_names =
        crate::features::action_utils::extract_slot_names_from_type_literal(macro_text);

    let missing: Vec<_> = template
        .defined_slots
        .iter()
        .filter(|s| !existing_names.iter().any(|n| n == &s.name))
        .collect();

    if missing.is_empty() {
        return None;
    }

    // Build the new members to insert
    let mut new_members = String::new();
    for slot in &missing {
        new_members.push_str(&build_slot_member(slot, Some(type_ctx)));
    }

    // Find insertion offset: scan backwards from span end to find the `}` before `>()`
    let insert_offset = find_type_literal_close(macro_text)
        .map(|rel| slots_macro.span.start + rel as u32)
        .unwrap_or_else(|| {
            // Fallback: span_end - 4 heuristic
            if slots_macro.span.end >= 4 {
                slots_macro.span.end - 4
            } else {
                slots_macro.span.end
            }
        });
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

/// Find the offset of the closing `}` before `>()` in a defineSlots macro text.
/// Returns the byte offset relative to the start of `macro_text`.
fn find_type_literal_close(macro_text: &str) -> Option<usize> {
    // Look for `}>()` and return the position of `}`
    macro_text.rfind("}>()").or_else(|| macro_text.rfind("}>"))
}

// ── B5: Prop mismatch detection ──────────────────────────────────────────

fn prop_mismatch_actions(
    source: &str,
    analysis: &FileAnalysisSnapshot,
    template: &verter_analysis::template::TemplateAnalysisSnapshot,
    line_index: &LineIndex,
    type_ctx: &SlotTypeContext<'_>,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    let slots_macro = match analysis
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)
    {
        Some(m) => m,
        None => return actions,
    };

    let macro_text = &source[slots_macro.span.start as usize..slots_macro.span.end as usize];
    let parsed_slots =
        crate::features::action_utils::extract_slots_with_props_from_type_literal(macro_text);

    // For each template slot that also exists in defineSlots, compare props
    for template_slot in &template.defined_slots {
        let parsed = match parsed_slots.iter().find(|p| p.name == template_slot.name) {
            Some(p) => p,
            None => continue, // slot not in defineSlots — handled by B3
        };

        // Missing props: template has `:prop` but defineSlots doesn't
        let missing_props: Vec<&str> = template_slot
            .binding_names
            .iter()
            .filter(|name| !parsed.prop_names.iter().any(|p| p == *name))
            .map(|s| s.as_str())
            .collect();

        if !missing_props.is_empty() {
            // Build insertion text for missing props
            let mut new_props = String::new();
            for prop_name in &missing_props {
                if !new_props.is_empty() {
                    new_props.push_str(", ");
                }
                new_props.push_str(prop_name);
                new_props.push_str(": ");
                // Resolve type from binding expression
                let expr = template_slot
                    .binding_expressions
                    .iter()
                    .zip(template_slot.binding_names.iter())
                    .find(|(_, name)| name.as_str() == *prop_name)
                    .map(|(expr, _)| expr.as_str())
                    .unwrap_or(prop_name);
                new_props.push_str(&resolve_binding_type(Some(type_ctx), expr));
            }

            // Find the props `{ ... }` for this slot in the macro text
            // We need to insert before the closing `}` of the props object
            if let Some(insert_offset) = find_slot_props_close(macro_text, &template_slot.name) {
                let abs_offset = slots_macro.span.start + insert_offset as u32;
                if let Some(position) = line_index.offset_to_position(abs_offset) {
                    let insert_text = if parsed.prop_names.is_empty() {
                        format!(" {} ", new_props)
                    } else {
                        format!(", {}", new_props)
                    };

                    let title = if missing_props.len() == 1 {
                        format!(
                            "Add prop '{}' to slot '{}' in defineSlots",
                            missing_props[0], template_slot.name
                        )
                    } else {
                        format!(
                            "Add {} missing props to slot '{}' in defineSlots",
                            missing_props.len(),
                            template_slot.name
                        )
                    };

                    actions.push(make_insert_action(
                        &title,
                        CodeActionKind::QUICKFIX,
                        &insert_text,
                        position,
                    ));
                }
            }
        }
    }

    actions
}

/// Find the byte offset of the closing `}` of a specific slot's `props: { ... }` in the macro text.
fn find_slot_props_close(macro_text: &str, slot_name: &str) -> Option<usize> {
    // Find the slot name in the macro text
    let name_pattern = if needs_quoting(slot_name) {
        format!("'{}'", slot_name)
    } else {
        slot_name.to_string()
    };

    let name_pos = macro_text.find(&name_pattern)?;
    // Find the `(` after the name
    let after_name = &macro_text[name_pos + name_pattern.len()..];
    let paren_pos = after_name.find('(')?;
    let after_paren = &macro_text[name_pos + name_pattern.len() + paren_pos + 1..];

    // Find the `{` opening the props object
    let brace_pos = after_paren.find('{')?;
    let props_start = name_pos + name_pattern.len() + paren_pos + 1 + brace_pos + 1;

    // Find the matching closing `}`
    let rest = &macro_text[props_start..];
    let mut depth = 1;
    for (i, ch) in rest.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(props_start + i);
                }
            }
            _ => {}
        }
    }
    None
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
