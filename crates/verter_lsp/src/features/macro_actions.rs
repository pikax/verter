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
mod tests {
    use super::*;
    use verter_analysis::template::{
        AnalyzedEmitDefinition, DefinedSlot, TemplateAnalysisSnapshot,
    };
    use verter_analysis::types::{AnalysisFlags, AnalyzedMacro, AnalyzedMacroKind};
    use verter_analysis::AnalyzedImport;

    fn make_analysis_with_slots(
        slots: Vec<DefinedSlot>,
        flags: AnalysisFlags,
        imports: Vec<AnalyzedImport>,
        macros: Vec<AnalyzedMacro>,
    ) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            imports,
            macros,
            script_flags: flags.bits(),
            template: Some(TemplateAnalysisSnapshot {
                defined_slots: slots,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn make_analysis_with_emits(
        emits: Vec<AnalyzedEmitDefinition>,
        flags: AnalysisFlags,
        imports: Vec<AnalyzedImport>,
        macros: Vec<AnalyzedMacro>,
    ) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            imports,
            macros,
            script_flags: flags.bits(),
            template: Some(TemplateAnalysisSnapshot {
                emit_definitions: emits,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    // ── B1 tests ─────────────────────────────────────────────────────────

    #[test]
    fn b1_generate_define_slots_from_template() {
        let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n<template><slot name=\"header\" /><slot /></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = make_analysis_with_slots(
            vec![
                DefinedSlot {
                    name: "header".into(),
                    has_bindings: false,
                    binding_names: vec![],
                    span: verter_span::Span::new(80, 110),
                },
                DefinedSlot {
                    name: "default".into(),
                    has_bindings: false,
                    binding_names: vec![],
                    span: verter_span::Span::new(110, 120),
                },
            ],
            AnalysisFlags::empty(),
            vec![AnalyzedImport {
                source: "vue".into(),
                is_type_only: false,
                bindings: vec![],
                span: verter_span::Span::new(24, 49),
                resolved_canonical_id: None,
            }],
            vec![],
        );

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        assert!(!actions.is_empty(), "should generate defineSlots action");

        let action = match &actions[0] {
            CodeActionOrCommand::CodeAction(ca) => ca,
            _ => panic!("expected CodeAction"),
        };
        assert_eq!(action.title, "Generate defineSlots from template");

        // Verify the edit contains both slot names
        let edit = action.edit.as_ref().unwrap();
        if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
            let text = &edits[0].edits[0];
            if let OneOf::Left(te) = text {
                assert!(te.new_text.contains("header"), "should contain header slot");
                assert!(
                    te.new_text.contains("default"),
                    "should contain default slot"
                );
                assert!(
                    te.new_text.contains("defineSlots"),
                    "should contain defineSlots"
                );
                // Negative: should not contain defineEmits
                assert!(
                    !te.new_text.contains("defineEmits"),
                    "should not contain defineEmits"
                );
            }
        }
    }

    #[test]
    fn b1_no_action_when_define_slots_exists() {
        let source = "<script setup lang=\"ts\">\ndefineSlots<{}>()\n</script>\n<template><slot /></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = make_analysis_with_slots(
            vec![DefinedSlot {
                name: "default".into(),
                has_bindings: false,
                binding_names: vec![],
                span: verter_span::Span::new(60, 70),
            }],
            AnalysisFlags::HAS_DEFINE_SLOTS,
            vec![],
            vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineSlots,
                is_type_based: true,
                type_references: vec!["default".into()],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                span: verter_span::Span::new(24, 42),
            }],
        );

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        // Should not have B1 (generate defineSlots) — only possibly B3
        let has_generate = actions.iter().any(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title == "Generate defineSlots from template",
            _ => false,
        });
        assert!(
            !has_generate,
            "should not generate defineSlots when it already exists"
        );
    }

    #[test]
    fn b1_no_action_without_script_setup() {
        let source = "<script>\nexport default {}\n</script>\n<template><slot /></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = make_analysis_with_slots(
            vec![DefinedSlot {
                name: "default".into(),
                has_bindings: false,
                binding_names: vec![],
                span: verter_span::Span::new(50, 60),
            }],
            AnalysisFlags::empty(),
            vec![],
            vec![],
        );

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        assert!(
            actions.is_empty(),
            "should not offer action without <script setup>"
        );
    }

    // ── B2 tests ─────────────────────────────────────────────────────────

    #[test]
    fn b2_generate_define_emits_from_undeclared() {
        let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\n</script>\n<template><button @click=\"$emit('save')\">Save</button></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = make_analysis_with_emits(
            vec![AnalyzedEmitDefinition {
                event_name: "save".into(),
                has_validator: false,
                is_declared: false,
                emit_locations: vec![],
                span: verter_span::Span::new(80, 100),
            }],
            AnalysisFlags::empty(),
            vec![AnalyzedImport {
                source: "vue".into(),
                is_type_only: false,
                bindings: vec![],
                span: verter_span::Span::new(24, 49),
                resolved_canonical_id: None,
            }],
            vec![],
        );

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        assert!(!actions.is_empty(), "should generate defineEmits action");

        let action = match &actions[0] {
            CodeActionOrCommand::CodeAction(ca) => ca,
            _ => panic!("expected CodeAction"),
        };
        assert_eq!(action.title, "Generate defineEmits from template usage");

        let edit = action.edit.as_ref().unwrap();
        if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
            let text = &edits[0].edits[0];
            if let OneOf::Left(te) = text {
                assert!(te.new_text.contains("'save'"), "should contain save event");
                assert!(
                    te.new_text.contains("defineEmits"),
                    "should contain defineEmits"
                );
                // Negative: no defineSlots
                assert!(
                    !te.new_text.contains("defineSlots"),
                    "should not contain defineSlots"
                );
            }
        }
    }

    #[test]
    fn b2_no_action_when_all_emits_declared() {
        let source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ (e: 'save'): void }>()\n</script>\n<template><button @click=\"emit('save')\">Save</button></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = make_analysis_with_emits(
            vec![AnalyzedEmitDefinition {
                event_name: "save".into(),
                has_validator: false,
                is_declared: true,
                emit_locations: vec![],
                span: verter_span::Span::new(30, 50),
            }],
            AnalysisFlags::HAS_DEFINE_EMITS,
            vec![],
            vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineEmits,
                is_type_based: true,
                type_references: vec![],
                binding_name: Some("emit".into()),
                model_name: None,
                has_inherit_attrs_false: false,
                span: verter_span::Span::new(24, 72),
            }],
        );

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        // No B2 (generate), no B4 (add missing) since all are declared
        let has_emit_action = actions.iter().any(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.contains("defineEmits"),
            _ => false,
        });
        assert!(
            !has_emit_action,
            "should not offer emit actions when all declared"
        );
    }

    // ── B4 tests ─────────────────────────────────────────────────────────

    #[test]
    fn b4_add_missing_emit_to_existing_type_based() {
        let source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ (e: 'save'): void }>()\n</script>\n<template><button @click=\"emit('delete')\">Del</button></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = make_analysis_with_emits(
            vec![
                AnalyzedEmitDefinition {
                    event_name: "save".into(),
                    has_validator: false,
                    is_declared: true,
                    emit_locations: vec![],
                    span: verter_span::Span::new(30, 50),
                },
                AnalyzedEmitDefinition {
                    event_name: "delete".into(),
                    has_validator: false,
                    is_declared: false,
                    emit_locations: vec![],
                    span: verter_span::Span::new(100, 120),
                },
            ],
            AnalysisFlags::HAS_DEFINE_EMITS,
            vec![],
            vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineEmits,
                is_type_based: true,
                type_references: vec![],
                binding_name: Some("emit".into()),
                model_name: None,
                has_inherit_attrs_false: false,
                span: verter_span::Span::new(24, 72),
            }],
        );

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        let add_action = actions.iter().find(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.contains("Add emit"),
            _ => false,
        });
        assert!(
            add_action.is_some(),
            "should offer action to add missing emit"
        );

        if let Some(CodeActionOrCommand::CodeAction(ca)) = add_action {
            assert!(
                ca.title.contains("delete"),
                "title should mention the missing emit"
            );
            let edit = ca.edit.as_ref().unwrap();
            if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
                let text = &edits[0].edits[0];
                if let OneOf::Left(te) = text {
                    assert!(te.new_text.contains("'delete'"), "should add delete emit");
                    // Negative: should not re-add save
                    assert!(
                        !te.new_text.contains("'save'"),
                        "should not re-add existing save emit"
                    );
                }
            }
        }
    }

    #[test]
    fn b4_add_missing_emit_to_runtime_array() {
        let source = "<script setup lang=\"ts\">\nconst emit = defineEmits(['save'])\n</script>\n<template><button @click=\"emit('delete')\">Del</button></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = make_analysis_with_emits(
            vec![
                AnalyzedEmitDefinition {
                    event_name: "save".into(),
                    has_validator: false,
                    is_declared: true,
                    emit_locations: vec![],
                    span: verter_span::Span::new(30, 50),
                },
                AnalyzedEmitDefinition {
                    event_name: "delete".into(),
                    has_validator: false,
                    is_declared: false,
                    emit_locations: vec![],
                    span: verter_span::Span::new(100, 120),
                },
            ],
            AnalysisFlags::HAS_DEFINE_EMITS,
            vec![],
            vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineEmits,
                is_type_based: false,
                type_references: vec![],
                binding_name: Some("emit".into()),
                model_name: None,
                has_inherit_attrs_false: false,
                span: verter_span::Span::new(24, 57),
            }],
        );

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        let add_action = actions.iter().find(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.contains("Add emit"),
            _ => false,
        });
        assert!(
            add_action.is_some(),
            "should offer action to add missing emit to array"
        );

        if let Some(CodeActionOrCommand::CodeAction(ca)) = add_action {
            let edit = ca.edit.as_ref().unwrap();
            if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
                let text = &edits[0].edits[0];
                if let OneOf::Left(te) = text {
                    assert!(te.new_text.contains("'delete'"), "should add delete emit");
                    // Runtime array form uses comma-separated strings
                    assert!(
                        te.new_text.starts_with(", "),
                        "should start with comma separator"
                    );
                }
            }
        }
    }

    #[test]
    fn no_actions_without_analysis() {
        let source = "<script setup>\n</script>\n<template></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let actions = macro_code_actions(source, None, &blocks, &line_index);
        assert!(actions.is_empty(), "should return empty without analysis");
    }

    #[test]
    fn b1_slots_with_scoped_bindings() {
        let source = "<script setup lang=\"ts\">\n</script>\n<template><slot name=\"row\" :item=\"item\" :index=\"i\" /></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = make_analysis_with_slots(
            vec![DefinedSlot {
                name: "row".into(),
                has_bindings: true,
                binding_names: vec!["item".into(), "index".into()],
                span: verter_span::Span::new(35, 90),
            }],
            AnalysisFlags::empty(),
            vec![],
            vec![],
        );

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        assert!(
            !actions.is_empty(),
            "should generate defineSlots for scoped slot"
        );

        if let Some(CodeActionOrCommand::CodeAction(ca)) = actions.first() {
            let edit = ca.edit.as_ref().unwrap();
            if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
                let text = &edits[0].edits[0];
                if let OneOf::Left(te) = text {
                    assert!(
                        te.new_text.contains("item: unknown"),
                        "should have item binding"
                    );
                    assert!(
                        te.new_text.contains("index: unknown"),
                        "should have index binding"
                    );
                    assert!(
                        te.new_text.contains("row(props:"),
                        "should have row slot name"
                    );
                }
            }
        }
    }

    /// @ai-generated - Slot names with hyphens are quoted
    #[test]
    fn b1_slot_names_with_hyphens_are_quoted() {
        let source =
            "<script setup lang=\"ts\">\n</script>\n<template><slot name=\"nav-bar\" /></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = make_analysis_with_slots(
            vec![DefinedSlot {
                name: "nav-bar".into(),
                has_bindings: false,
                binding_names: vec![],
                span: verter_span::Span::new(35, 60),
            }],
            AnalysisFlags::empty(),
            vec![],
            vec![],
        );

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        assert!(!actions.is_empty());

        if let Some(CodeActionOrCommand::CodeAction(ca)) = actions.first() {
            let edit = ca.edit.as_ref().unwrap();
            if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
                let text = &edits[0].edits[0];
                if let OneOf::Left(te) = text {
                    assert!(
                        te.new_text.contains("'nav-bar'"),
                        "hyphenated slot name should be quoted, got: {}",
                        te.new_text
                    );
                    // Negative: should NOT have unquoted nav-bar
                    assert!(
                        !te.new_text.contains(" nav-bar("),
                        "unquoted hyphenated name should not appear"
                    );
                }
            }
        }
    }

    /// @ai-generated - Insert offset prefers position after last import
    #[test]
    fn b1_insert_after_last_import() {
        let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nimport { computed } from 'vue'\nconst x = ref(0)\n</script>\n<template><slot /></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = make_analysis_with_slots(
            vec![DefinedSlot {
                name: "default".into(),
                has_bindings: false,
                binding_names: vec![],
                span: verter_span::Span::new(100, 110),
            }],
            AnalysisFlags::empty(),
            vec![
                AnalyzedImport {
                    source: "vue".into(),
                    is_type_only: false,
                    bindings: vec![],
                    span: verter_span::Span::new(24, 49),
                    resolved_canonical_id: None,
                },
                AnalyzedImport {
                    source: "vue".into(),
                    is_type_only: false,
                    bindings: vec![],
                    span: verter_span::Span::new(50, 80),
                    resolved_canonical_id: None,
                },
            ],
            vec![],
        );

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        assert!(!actions.is_empty());

        if let Some(CodeActionOrCommand::CodeAction(ca)) = actions.first() {
            let edit = ca.edit.as_ref().unwrap();
            if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
                let text = &edits[0].edits[0];
                if let OneOf::Left(te) = text {
                    // The insert position should be after the second import, not at the start
                    // Line 2 = "import { computed } from 'vue'" (0-indexed)
                    assert!(
                        te.range.start.line >= 2,
                        "should insert after last import (line >=2), got line {}",
                        te.range.start.line
                    );
                }
            }
        }
    }

    /// @ai-generated - Multiple undeclared emits generate multi-entry defineEmits
    #[test]
    fn b2_multiple_undeclared_emits() {
        let source = "<script setup lang=\"ts\">\n</script>\n<template><button @click=\"$emit('save')\">Save</button><button @click=\"$emit('cancel')\">Cancel</button></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = make_analysis_with_emits(
            vec![
                AnalyzedEmitDefinition {
                    event_name: "save".into(),
                    has_validator: false,
                    is_declared: false,
                    emit_locations: vec![],
                    span: verter_span::Span::new(40, 60),
                },
                AnalyzedEmitDefinition {
                    event_name: "cancel".into(),
                    has_validator: false,
                    is_declared: false,
                    emit_locations: vec![],
                    span: verter_span::Span::new(80, 100),
                },
            ],
            AnalysisFlags::empty(),
            vec![],
            vec![],
        );

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        assert!(!actions.is_empty());

        if let Some(CodeActionOrCommand::CodeAction(ca)) = actions.first() {
            let edit = ca.edit.as_ref().unwrap();
            if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
                let text = &edits[0].edits[0];
                if let OneOf::Left(te) = text {
                    assert!(te.new_text.contains("'save'"), "should contain save");
                    assert!(te.new_text.contains("'cancel'"), "should contain cancel");
                    // Should be a single defineEmits call with both entries
                    let define_count = te.new_text.matches("defineEmits").count();
                    assert_eq!(define_count, 1, "should have exactly one defineEmits call");
                }
            }
        }
    }

    /// @ai-generated - No actions when template has no slots or emits
    #[test]
    fn no_actions_with_empty_template() {
        let source = "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div>Hello</div></template>";
        let line_index = LineIndex::new_utf16(source);
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

        let analysis = FileAnalysisSnapshot {
            script_flags: AnalysisFlags::empty().bits(),
            template: Some(TemplateAnalysisSnapshot::default()),
            ..Default::default()
        };

        let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index);
        assert!(
            actions.is_empty(),
            "should not offer any actions for template without slots/emits"
        );
    }
}
