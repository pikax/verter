use super::*;
use verter_analysis::template::{AnalyzedEmitDefinition, DefinedSlot, TemplateAnalysisSnapshot};
use verter_analysis::types::{
    AnalysisFlags, AnalyzedBinding, AnalyzedBindingKind, AnalyzedMacro, AnalyzedMacroKind,
    ReactivityKind,
};
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
                binding_expressions: vec![],
                binding_value_spans: vec![],
                span: verter_span::Span::new(80, 110),
            },
            DefinedSlot {
                name: "default".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
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

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
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
            binding_expressions: vec![],
            binding_value_spans: vec![],
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
            prop_fields: vec![],
            span: verter_span::Span::new(24, 42),
        }],
    );

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
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
            binding_expressions: vec![],
            binding_value_spans: vec![],
            span: verter_span::Span::new(50, 60),
        }],
        AnalysisFlags::empty(),
        vec![],
        vec![],
    );

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
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

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
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
            prop_fields: vec![],
            span: verter_span::Span::new(24, 72),
        }],
    );

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
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
            prop_fields: vec![],
            span: verter_span::Span::new(24, 72),
        }],
    );

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
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
            prop_fields: vec![],
            span: verter_span::Span::new(24, 57),
        }],
    );

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
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

    let actions = macro_code_actions(source, None, &blocks, &line_index, None);
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
            binding_expressions: vec!["item".into(), "i".into()],
            binding_value_spans: vec![],
            span: verter_span::Span::new(35, 90),
        }],
        AnalysisFlags::empty(),
        vec![],
        vec![],
    );

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
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
            binding_expressions: vec![],
            binding_value_spans: vec![],
            span: verter_span::Span::new(35, 60),
        }],
        AnalysisFlags::empty(),
        vec![],
        vec![],
    );

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
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
            binding_expressions: vec![],
            binding_value_spans: vec![],
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

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
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

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
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

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
    assert!(
        actions.is_empty(),
        "should not offer any actions for template without slots/emits"
    );
}

// ── Type resolution tests (Phase 2-3) ───────────────────────────────

fn make_binding(name: &str, type_annotation: Option<&str>) -> AnalyzedBinding {
    AnalyzedBinding {
        name: name.into(),
        kind: AnalyzedBindingKind::Const,
        is_reactive: false,
        reactivity_kind: ReactivityKind::None,
        type_annotation: type_annotation.map(|s| s.into()),
        initializer: None,
        span: verter_span::Span::new(0, 0),
    }
}

#[test]
fn b1_resolves_type_from_analysis_bindings() {
    let source = "<script setup lang=\"ts\">\nconst title: string = 'hello'\n</script>\n<template><slot :title=\"title\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

    let mut analysis = make_analysis_with_slots(
        vec![DefinedSlot {
            name: "default".into(),
            has_bindings: true,
            binding_names: vec!["title".into()],
            binding_expressions: vec!["title".into()],
            binding_value_spans: vec![],
            span: verter_span::Span::new(70, 100),
        }],
        AnalysisFlags::empty(),
        vec![],
        vec![],
    );
    analysis.bindings = vec![make_binding("title", Some("string"))];

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
    assert!(!actions.is_empty(), "should generate defineSlots action");

    if let Some(CodeActionOrCommand::CodeAction(ca)) = actions.first() {
        let edit = ca.edit.as_ref().unwrap();
        if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
            let text = &edits[0].edits[0];
            if let OneOf::Left(te) = text {
                assert!(
                    te.new_text.contains("title: string"),
                    "should resolve type from analysis binding, got: {}",
                    te.new_text
                );
                // Negative: should NOT have "unknown" for title
                assert!(
                    !te.new_text.contains("title: unknown"),
                    "should not use unknown when type is available"
                );
            }
        }
    }
}

#[test]
fn b1_falls_back_to_unknown_without_type_annotation() {
    let source = "<script setup lang=\"ts\">\nconst items = ref([])\n</script>\n<template><slot :items=\"items\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

    let mut analysis = make_analysis_with_slots(
        vec![DefinedSlot {
            name: "default".into(),
            has_bindings: true,
            binding_names: vec!["items".into()],
            binding_expressions: vec!["items".into()],
            binding_value_spans: vec![],
            span: verter_span::Span::new(60, 95),
        }],
        AnalysisFlags::empty(),
        vec![],
        vec![],
    );
    // Binding exists but has no type_annotation
    analysis.bindings = vec![make_binding("items", None)];

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
    assert!(!actions.is_empty());

    if let Some(CodeActionOrCommand::CodeAction(ca)) = actions.first() {
        let edit = ca.edit.as_ref().unwrap();
        if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
            let text = &edits[0].edits[0];
            if let OneOf::Left(te) = text {
                assert!(
                    te.new_text.contains("items: unknown"),
                    "should fall back to unknown, got: {}",
                    te.new_text
                );
            }
        }
    }
}

#[test]
fn b1_resolves_multiple_binding_types() {
    let source = "<script setup lang=\"ts\">\nconst title: string = ''\nconst count: number = 0\n</script>\n<template><slot name=\"header\" :title=\"title\" :count=\"count\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

    let mut analysis = make_analysis_with_slots(
        vec![DefinedSlot {
            name: "header".into(),
            has_bindings: true,
            binding_names: vec!["title".into(), "count".into()],
            binding_expressions: vec!["title".into(), "count".into()],
            binding_value_spans: vec![],
            span: verter_span::Span::new(80, 140),
        }],
        AnalysisFlags::empty(),
        vec![],
        vec![],
    );
    analysis.bindings = vec![
        make_binding("title", Some("string")),
        make_binding("count", Some("number")),
    ];

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
    assert!(!actions.is_empty());

    if let Some(CodeActionOrCommand::CodeAction(ca)) = actions.first() {
        let edit = ca.edit.as_ref().unwrap();
        if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
            let text = &edits[0].edits[0];
            if let OneOf::Left(te) = text {
                assert!(
                    te.new_text.contains("title: string"),
                    "should resolve title type, got: {}",
                    te.new_text
                );
                assert!(
                    te.new_text.contains("count: number"),
                    "should resolve count type, got: {}",
                    te.new_text
                );
            }
        }
    }
}

// ── B3 tests (add missing slots to existing defineSlots) ────────────

#[test]
fn b3_detects_missing_slot() {
    // defineSlots has only "header", but template also has "footer"
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: {}): any\n}>()\n</script>\n<template><slot name=\"header\" /><slot name=\"footer\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

    let analysis = make_analysis_with_slots(
        vec![
            DefinedSlot {
                name: "header".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                span: verter_span::Span::new(90, 110),
            },
            DefinedSlot {
                name: "footer".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                span: verter_span::Span::new(110, 135),
            },
        ],
        AnalysisFlags::HAS_DEFINE_SLOTS,
        vec![],
        vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineSlots,
            is_type_based: true,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            span: verter_span::Span::new(24, 68),
        }],
    );

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
    let add_action = actions.iter().find(|a| match a {
        CodeActionOrCommand::CodeAction(ca) => ca.title.contains("footer"),
        _ => false,
    });
    assert!(
        add_action.is_some(),
        "should offer action to add missing footer slot, actions: {:?}",
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                _ => "command".into(),
            })
            .collect::<Vec<_>>()
    );

    if let Some(CodeActionOrCommand::CodeAction(ca)) = add_action {
        let edit = ca.edit.as_ref().unwrap();
        if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
            let text = &edits[0].edits[0];
            if let OneOf::Left(te) = text {
                assert!(te.new_text.contains("footer"), "should contain footer slot");
                // Negative: should NOT re-add header
                assert!(
                    !te.new_text.contains("header"),
                    "should not re-add existing header slot"
                );
            }
        }
    }
}

#[test]
fn b3_no_action_when_all_present() {
    // defineSlots already has both "header" and "default"
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: {}): any\n    default(props: {}): any\n}>()\n</script>\n<template><slot name=\"header\" /><slot /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

    let analysis = make_analysis_with_slots(
        vec![
            DefinedSlot {
                name: "header".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                span: verter_span::Span::new(120, 140),
            },
            DefinedSlot {
                name: "default".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                span: verter_span::Span::new(140, 155),
            },
        ],
        AnalysisFlags::HAS_DEFINE_SLOTS,
        vec![],
        vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineSlots,
            is_type_based: true,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            span: verter_span::Span::new(24, 92),
        }],
    );

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
    let has_add_slot = actions.iter().any(|a| match a {
        CodeActionOrCommand::CodeAction(ca) => {
            ca.title.contains("Add slot") || ca.title.contains("missing slot")
        }
        _ => false,
    });
    assert!(
        !has_add_slot,
        "should not offer action when all slots are present"
    );
}

// ── B5 tests (prop mismatch detection) ──────────────────────────────

#[test]
fn b5_missing_prop_detected() {
    // defineSlots has header with "title" prop, but template also passes "subtitle"
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: { title: string }): any\n}>()\n</script>\n<template><slot name=\"header\" :title=\"t\" :subtitle=\"s\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

    let analysis = make_analysis_with_slots(
        vec![DefinedSlot {
            name: "header".into(),
            has_bindings: true,
            binding_names: vec!["title".into(), "subtitle".into()],
            binding_expressions: vec!["t".into(), "s".into()],
            binding_value_spans: vec![],
            span: verter_span::Span::new(100, 150),
        }],
        AnalysisFlags::HAS_DEFINE_SLOTS,
        vec![],
        vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineSlots,
            is_type_based: true,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            span: verter_span::Span::new(24, 80),
        }],
    );

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
    let add_prop_action = actions.iter().find(|a| match a {
        CodeActionOrCommand::CodeAction(ca) => ca.title.contains("subtitle"),
        _ => false,
    });
    assert!(
        add_prop_action.is_some(),
        "should offer action to add missing prop 'subtitle', actions: {:?}",
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                _ => "command".into(),
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn b5_no_action_when_props_match() {
    // defineSlots and template have the same props
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: { title: string }): any\n}>()\n</script>\n<template><slot name=\"header\" :title=\"t\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

    let analysis = make_analysis_with_slots(
        vec![DefinedSlot {
            name: "header".into(),
            has_bindings: true,
            binding_names: vec!["title".into()],
            binding_expressions: vec!["t".into()],
            binding_value_spans: vec![],
            span: verter_span::Span::new(100, 140),
        }],
        AnalysisFlags::HAS_DEFINE_SLOTS,
        vec![],
        vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineSlots,
            is_type_based: true,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            span: verter_span::Span::new(24, 80),
        }],
    );

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
    let has_prop_action = actions.iter().any(|a| match a {
        CodeActionOrCommand::CodeAction(ca) => {
            ca.title.contains("prop") && ca.title.contains("slot")
        }
        _ => false,
    });
    assert!(
        !has_prop_action,
        "should not offer prop action when all props match"
    );
}

// ── Cursor range filtering tests ─────────────────────────────────────

#[test]
fn cursor_on_slot_element_shows_slot_actions() {
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\n</script>\n<template><div>hello</div><slot name=\"header\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

    // <slot name="header" /> starts at offset 80 (approx)
    let slot_start = source.find("<slot").unwrap() as u32;
    let slot_end = source[slot_start as usize..].find("/>").unwrap() as u32 + slot_start + 2;

    let analysis = make_analysis_with_slots(
        vec![DefinedSlot {
            name: "header".into(),
            has_bindings: false,
            binding_names: vec![],
            binding_expressions: vec![],
            binding_value_spans: vec![],
            span: verter_span::Span::new(slot_start, slot_end),
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

    // Cursor inside the <slot> element → should show slot actions
    let cursor_in_slot = Some(slot_start + 2); // inside "<sl|ot"
    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, cursor_in_slot);
    assert!(
        !actions.is_empty(),
        "cursor inside <slot> should show slot actions"
    );

    // Cursor on the <div> → should NOT show slot actions
    let div_start = source.find("<div").unwrap() as u32;
    let cursor_on_div = Some(div_start + 2);
    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, cursor_on_div);
    assert!(
        actions.is_empty(),
        "cursor on <div> should NOT show slot actions"
    );
}

#[test]
fn cursor_on_define_slots_macro_shows_augmentation_actions() {
    // defineSlots has only "header", but template also has "footer"
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: {}): any\n}>()\n</script>\n<template><slot name=\"header\" /><slot name=\"footer\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

    let macro_start = source.find("defineSlots").unwrap() as u32;
    let macro_end = source[macro_start as usize..].find("()").unwrap() as u32 + macro_start + 2;

    let slot1_start = source.find("<slot name=\"header\"").unwrap() as u32;
    let slot1_end = source[slot1_start as usize..].find("/>").unwrap() as u32 + slot1_start + 2;
    let slot2_start = source.find("<slot name=\"footer\"").unwrap() as u32;
    let slot2_end = source[slot2_start as usize..].find("/>").unwrap() as u32 + slot2_start + 2;

    let analysis = make_analysis_with_slots(
        vec![
            DefinedSlot {
                name: "header".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                span: verter_span::Span::new(slot1_start, slot1_end),
            },
            DefinedSlot {
                name: "footer".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                span: verter_span::Span::new(slot2_start, slot2_end),
            },
        ],
        AnalysisFlags::HAS_DEFINE_SLOTS,
        vec![],
        vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineSlots,
            is_type_based: true,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            span: verter_span::Span::new(macro_start, macro_end),
        }],
    );

    // Cursor on defineSlots macro → should show B3 actions
    let cursor_on_macro = Some(macro_start + 5);
    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, cursor_on_macro);
    let has_add_footer = actions.iter().any(|a| match a {
        CodeActionOrCommand::CodeAction(ca) => ca.title.contains("footer"),
        _ => false,
    });
    assert!(
        has_add_footer,
        "cursor on defineSlots should show 'add footer' action"
    );
}

#[test]
fn cursor_none_shows_all_actions() {
    // When cursor_offset is None, all actions should be returned (backward compat)
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\n</script>\n<template><slot name=\"header\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);

    let slot_start = source.find("<slot").unwrap() as u32;
    let slot_end = source[slot_start as usize..].find("/>").unwrap() as u32 + slot_start + 2;

    let analysis = make_analysis_with_slots(
        vec![DefinedSlot {
            name: "header".into(),
            has_bindings: false,
            binding_names: vec![],
            binding_expressions: vec![],
            binding_value_spans: vec![],
            span: verter_span::Span::new(slot_start, slot_end),
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

    let actions = macro_code_actions(source, Some(&analysis), &blocks, &line_index, None);
    assert!(
        !actions.is_empty(),
        "cursor_offset=None should return all actions"
    );
}
