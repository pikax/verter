use super::*;
use verter_analysis::template::{AnalyzedEmitDefinition, DefinedSlot, TemplateAnalysisSnapshot};
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
    let source =
        "<script setup lang=\"ts\">\ndefineSlots<{}>()\n</script>\n<template><slot /></template>";
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

#[test]
fn no_actions_with_empty_template() {
    let source =
        "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div>Hello</div></template>";
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
