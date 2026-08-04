use super::*;
use verter_semantic::analysis::template::{
    AnalyzedEmitDefinition, DefinedSlot, TemplateAnalysisSnapshot,
};
use verter_semantic::analysis::types::{
    AnalysisFlags, AnalyzedBinding, AnalyzedBindingKind, MacroAnchorUnsupported, ReactivityKind,
};

// ── B1 tests ─────────────────────────────────────────────────────────

#[test]
fn b1_generate_define_slots_from_template() {
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n<template><slot name=\"header\" /><slot /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![
            DefinedSlot {
                name: "header".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                has_fallback_content: false,
                span: verter_span::Span::new(80, 110),
            },
            DefinedSlot {
                name: "default".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                has_fallback_content: false,
                span: verter_span::Span::new(110, 120),
            },
        ],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "default".into(),
            has_bindings: false,
            binding_names: vec![],
            binding_expressions: vec![],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(60, 70),
        }],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "default".into(),
            has_bindings: false,
            binding_names: vec![],
            binding_expressions: vec![],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(50, 60),
        }],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![],
        vec![AnalyzedEmitDefinition {
            event_name: "save".into(),
            has_validator: false,
            is_declared: false,
            emit_locations: vec![],
            span: verter_span::Span::new(80, 100),
        }],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![],
        vec![AnalyzedEmitDefinition {
            event_name: "save".into(),
            has_validator: false,
            is_declared: true,
            emit_locations: vec![],
            span: verter_span::Span::new(30, 50),
        }],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![],
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
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![],
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
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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

/// B4's EMPTY-array separator branch (review-B P2-3): the first entry into
/// `defineEmits([])` must carry NO leading comma — an inverted `is_empty`
/// condition would emit `[, 'save']`, which this pins in both directions.
#[test]
fn b4_first_emit_into_an_empty_runtime_array_has_no_leading_separator() {
    let source = "<script setup lang=\"ts\">\nconst emit = defineEmits([])\n</script>\n<template><button @click=\"emit('save')\">S</button></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![],
        vec![AnalyzedEmitDefinition {
            event_name: "save".into(),
            has_validator: false,
            is_declared: false,
            emit_locations: vec![],
            span: verter_span::Span::new(80, 100),
        }],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    let add_action = actions.iter().find(|a| match a {
        CodeActionOrCommand::CodeAction(ca) => ca.title.contains("Add emit"),
        _ => false,
    });
    let Some(CodeActionOrCommand::CodeAction(ca)) = add_action else {
        panic!("should offer action to add the first emit to an empty array");
    };
    let edit = ca.edit.as_ref().unwrap();
    let Some(DocumentChanges::Edits(edits)) = &edit.document_changes else {
        panic!("edit must carry document changes");
    };
    let OneOf::Left(te) = &edits[0].edits[0] else {
        panic!("edit must be a text edit");
    };
    assert_eq!(
        te.new_text, "'save'",
        "the FIRST entry into an empty array carries no separator"
    );
    assert!(
        !te.new_text.starts_with(','),
        "an inverted empty-anchor branch would emit a leading comma"
    );
}

/// B5's EMPTY-object separator branch: the first prop into a slot declared
/// with an empty props object must not carry a leading comma either.
#[test]
fn b5_first_prop_into_an_empty_slot_object_has_no_leading_separator() {
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: {}): any\n}>()\n</script>\n<template><slot name=\"header\" :item=\"x\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(
        source,
        vec![slot("header", &["item"], verter_span::Span::new(0, 0))],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    let add_action = actions.iter().find(|a| match a {
        CodeActionOrCommand::CodeAction(ca) => ca.title.contains("Add prop 'item'"),
        _ => false,
    });
    let Some(CodeActionOrCommand::CodeAction(ca)) = add_action else {
        panic!("should offer action to add the first prop to the empty slot object");
    };
    let edit = ca.edit.as_ref().unwrap();
    let Some(DocumentChanges::Edits(edits)) = &edit.document_changes else {
        panic!("edit must carry document changes");
    };
    let OneOf::Left(te) = &edits[0].edits[0] else {
        panic!("edit must be a text edit");
    };
    assert!(
        !te.new_text.trim_start().starts_with(','),
        "the FIRST prop into an empty object must not carry a leading comma, got {:?}",
        te.new_text
    );
    assert!(
        te.new_text.contains("item"),
        "the prop itself must be inserted, got {:?}",
        te.new_text
    );
}

#[test]
fn no_actions_without_analysis() {
    let source = "<script setup>\n</script>\n<template></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        None,
        &blocks,
        &line_index,
        None,
    );
    assert!(actions.is_empty(), "should return empty without analysis");
}

#[test]
fn b1_slots_with_scoped_bindings() {
    let source = "<script setup lang=\"ts\">\n</script>\n<template><slot name=\"row\" :item=\"item\" :index=\"i\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "row".into(),
            has_bindings: true,
            binding_names: vec!["item".into(), "index".into()],
            binding_expressions: vec!["item".into(), "i".into()],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(35, 90),
        }],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "nav-bar".into(),
            has_bindings: false,
            binding_names: vec![],
            binding_expressions: vec![],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(35, 60),
        }],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "default".into(),
            has_bindings: false,
            binding_names: vec![],
            binding_expressions: vec![],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(100, 110),
        }],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![],
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
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = FileAnalysisSnapshot {
        script_flags: AnalysisFlags::empty().bits(),
        template: Some((TemplateAnalysisSnapshot::default()).into()),
        ..Default::default()
    };

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    assert!(
        actions.is_empty(),
        "should not offer any actions for template without slots/emits"
    );
}

// ── Type resolution tests ───────────────────────────────

fn make_binding(name: &str, type_annotation: Option<&str>) -> AnalyzedBinding {
    AnalyzedBinding {
        name: name.into(),
        kind: AnalyzedBindingKind::Const,
        is_reactive: false,
        reactivity_kind: ReactivityKind::None,
        type_annotation: type_annotation.map(|s| s.into()),
        initializer: None,
        span: verter_span::Span::new(0, 0),
        used_in_script: false,
        used_in_style: false,
    }
}

#[test]
fn b1_resolves_type_from_analysis_bindings() {
    let source = "<script setup lang=\"ts\">\nconst title: string = 'hello'\n</script>\n<template><slot :title=\"title\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let mut analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "default".into(),
            has_bindings: true,
            binding_names: vec!["title".into()],
            binding_expressions: vec!["title".into()],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(70, 100),
        }],
        vec![],
    );
    analysis.bindings = vec![make_binding("title", Some("string"))];

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let mut analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "default".into(),
            has_bindings: true,
            binding_names: vec!["items".into()],
            binding_expressions: vec!["items".into()],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(60, 95),
        }],
        vec![],
    );
    // Binding exists but has no type_annotation
    analysis.bindings = vec![make_binding("items", None)];

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let mut analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "header".into(),
            has_bindings: true,
            binding_names: vec!["title".into(), "count".into()],
            binding_expressions: vec!["title".into(), "count".into()],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(80, 140),
        }],
        vec![],
    );
    analysis.bindings = vec![
        make_binding("title", Some("string")),
        make_binding("count", Some("number")),
    ];

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![
            DefinedSlot {
                name: "header".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                has_fallback_content: false,
                span: verter_span::Span::new(90, 110),
            },
            DefinedSlot {
                name: "footer".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                has_fallback_content: false,
                span: verter_span::Span::new(110, 135),
            },
        ],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![
            DefinedSlot {
                name: "header".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                has_fallback_content: false,
                span: verter_span::Span::new(120, 140),
            },
            DefinedSlot {
                name: "default".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                has_fallback_content: false,
                span: verter_span::Span::new(140, 155),
            },
        ],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "header".into(),
            has_bindings: true,
            binding_names: vec!["title".into(), "subtitle".into()],
            binding_expressions: vec!["t".into(), "s".into()],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(100, 150),
        }],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "header".into(),
            has_bindings: true,
            binding_names: vec!["title".into()],
            binding_expressions: vec!["t".into()],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(100, 140),
        }],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    // <slot name="header" /> starts at offset 80 (approx)
    let slot_start = source.find("<slot").unwrap() as u32;
    let slot_end = source[slot_start as usize..].find("/>").unwrap() as u32 + slot_start + 2;

    let analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "header".into(),
            has_bindings: false,
            binding_names: vec![],
            binding_expressions: vec![],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(slot_start, slot_end),
        }],
        vec![],
    );

    // Cursor inside the <slot> element → should show slot actions
    let cursor_in_slot = Some(slot_start + 2); // inside "<sl|ot"
    let actions = macro_code_actions(
        source,
        verter_session::AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        cursor_in_slot,
    );
    assert!(
        !actions.is_empty(),
        "cursor inside <slot> should show slot actions"
    );

    // Cursor on the <div> → should NOT show slot actions
    let div_start = source.find("<div").unwrap() as u32;
    let cursor_on_div = Some(div_start + 2);
    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        cursor_on_div,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let macro_start = source.find("defineSlots").unwrap() as u32;

    let slot1_start = source.find("<slot name=\"header\"").unwrap() as u32;
    let slot1_end = source[slot1_start as usize..].find("/>").unwrap() as u32 + slot1_start + 2;
    let slot2_start = source.find("<slot name=\"footer\"").unwrap() as u32;
    let slot2_end = source[slot2_start as usize..].find("/>").unwrap() as u32 + slot2_start + 2;

    let analysis = producer_backed_snapshot(
        source,
        vec![
            DefinedSlot {
                name: "header".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                has_fallback_content: false,
                span: verter_span::Span::new(slot1_start, slot1_end),
            },
            DefinedSlot {
                name: "footer".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                has_fallback_content: false,
                span: verter_span::Span::new(slot2_start, slot2_end),
            },
        ],
        vec![],
    );

    // Cursor on defineSlots macro → should show B3 actions
    let cursor_on_macro = Some(macro_start + 5);
    let actions = macro_code_actions(
        source,
        verter_session::AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        cursor_on_macro,
    );
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
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);

    let slot_start = source.find("<slot").unwrap() as u32;
    let slot_end = source[slot_start as usize..].find("/>").unwrap() as u32 + slot_start + 2;

    let analysis = producer_backed_snapshot(
        source,
        vec![DefinedSlot {
            name: "header".into(),
            has_bindings: false,
            binding_names: vec![],
            binding_expressions: vec![],
            binding_value_spans: vec![],
            has_fallback_content: false,
            span: verter_span::Span::new(slot_start, slot_end),
        }],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    assert!(
        !actions.is_empty(),
        "cursor_offset=None should return all actions"
    );
}

// ── T-A7: analysis-owned membership + revision-stamped edit anchors ──
//
// Every case below pairs a hand-authored TEMPLATE inventory with a script
// analysis minted by the REAL analyzer (`macro_fixture::analyze_sfc_script`),
// so macro membership rows and edit anchors are producer-backed: a fixture
// cannot hand-forge an anchor, and a mint bug therefore fails the test.

use crate::features::macro_fixture::analyze_sfc_script;

/// Build a snapshot whose script side (macros, `slot_fields`, anchors, flags,
/// bindings, imports) comes from the real analyzer over `source`, paired with
/// the given template inventory.
fn producer_backed_snapshot(
    source: &str,
    slots: Vec<DefinedSlot>,
    emits: Vec<AnalyzedEmitDefinition>,
) -> FileAnalysisSnapshot {
    let script = analyze_sfc_script(source);
    FileAnalysisSnapshot {
        imports: script.imports.clone(),
        bindings: script.bindings.clone(),
        macros: script.macros.clone().into(),
        script_flags: script.flags.bits(),
        template: Some(
            (TemplateAnalysisSnapshot {
                defined_slots: slots,
                emit_definitions: emits,
                ..Default::default()
            })
            .into(),
        ),
        // These are the exact bytes the analyzer above observed.
        anchor_revision: AnalysisSourceRevision::of_source(source),
        ..Default::default()
    }
}

fn slot(name: &str, bindings: &[&str], span: verter_span::Span) -> DefinedSlot {
    DefinedSlot {
        name: name.into(),
        has_bindings: !bindings.is_empty(),
        binding_names: bindings.iter().map(|b| (*b).to_string()).collect(),
        binding_expressions: bindings.iter().map(|b| (*b).to_string()).collect(),
        binding_value_spans: vec![],
        has_fallback_content: false,
        span,
    }
}

fn undeclared_emit(name: &str) -> AnalyzedEmitDefinition {
    AnalyzedEmitDefinition {
        event_name: name.into(),
        has_validator: false,
        is_declared: false,
        emit_locations: vec![],
        span: verter_span::Span::new(0, 0),
    }
}

fn action_titles(actions: &[CodeActionOrCommand]) -> Vec<String> {
    actions
        .iter()
        .map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
            _ => "command".to_string(),
        })
        .collect()
}

/// Apply the single insertion edit an action carries to `source`.
fn apply_insert(source: &str, line_index: &LineIndex, action: &CodeActionOrCommand) -> String {
    let CodeActionOrCommand::CodeAction(ca) = action else {
        panic!("expected a CodeAction");
    };
    let Some(DocumentChanges::Edits(doc_edits)) = &ca.edit.as_ref().expect("edit").document_changes
    else {
        panic!("expected DocumentChanges::Edits");
    };
    let OneOf::Left(te) = &doc_edits[0].edits[0] else {
        panic!("expected a TextEdit");
    };
    let offset = line_index
        .position_to_offset(&te.range.start)
        .expect("edit position must map back to a byte offset") as usize;
    let mut out = String::with_capacity(source.len() + te.new_text.len());
    out.push_str(&source[..offset]);
    out.push_str(&te.new_text);
    out.push_str(&source[offset..]);
    out
}

// ── A7-01: membership comes from analysis rows, never macro text ──

/// The macro TEXT advertises `subtitle` inside a comment; the analyzer's rows
/// do not. Membership must follow the rows, so `subtitle` is still missing.
#[test]
fn b5_slot_prop_membership_reads_analysis_rows_not_macro_text() {
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: { /* subtitle: string */ title: string }): any\n}>()\n</script>\n<template><slot name=\"header\" :title=\"t\" :subtitle=\"s\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(
        source,
        vec![slot(
            "header",
            &["title", "subtitle"],
            verter_span::Span::new(0, 0),
        )],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    let titles = action_titles(&actions);

    assert!(
        titles.iter().any(|t| t.contains("subtitle")),
        "commented-out `subtitle` is NOT an analysis row, so it is missing; titles: {titles:?}"
    );
    assert!(
        !titles.iter().any(|t| t.contains("'title'")),
        "`title` IS an analysis row and must not be offered; titles: {titles:?}"
    );
}

// ── A7-01b: arrow-form and `Pick<…>` surfaces participate ──

/// An arrow-form slot (`default: (props: {…}) => any`) IS a declared member.
/// The deleted scanner required `name(` and therefore offered a duplicate.
#[test]
fn b3_offers_no_duplicate_for_arrow_form_declared_slot() {
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    default: (props: { item: string }) => any\n}>()\n</script>\n<template><slot :item=\"i\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(
        source,
        vec![slot("default", &["item"], verter_span::Span::new(0, 0))],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    let titles = action_titles(&actions);

    assert!(
        !titles.iter().any(|t| t.contains("Add slot")),
        "arrow-form `default` already exists — no duplicate member; titles: {titles:?}"
    );
}

/// An arrow-form slot's props object is anchorable, so B5 fires for it.
#[test]
fn b5_arrow_form_slot_props_participate() {
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    default: (props: { item: string }) => any\n}>()\n</script>\n<template><slot :item=\"i\" :index=\"n\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(
        source,
        vec![slot(
            "default",
            &["item", "index"],
            verter_span::Span::new(0, 0),
        )],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    let add_index = actions
        .iter()
        .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("index")))
        .unwrap_or_else(|| {
            panic!(
                "arrow-form slot props must participate in B5; titles: {:?}",
                action_titles(&actions)
            )
        });

    assert_eq!(
        apply_insert(source, &line_index, add_index),
        "<script setup lang=\"ts\">\ndefineSlots<{\n    default: (props: { item: string , index: unknown}) => any\n}>()\n</script>\n<template><slot :item=\"i\" :index=\"n\" /></template>",
        "the prop must land inside the arrow-form slot's own props object, immediately \
         before its closing delimiter — the anchor owns the placement authority, and \
         the insertion column matches what a reader of the authored source expects"
    );
}

/// A `Pick<…>` binding surface's keys ARE members: `id` is declared, so no
/// prop action may be offered for it — and nothing may be inserted into a
/// LATER slot's props object (the deleted scanner's misplacement).
#[test]
fn b3_pick_binding_surface_slots_are_members() {
    let source = "<script setup lang=\"ts\">\ntype Row = { id: string, note: string }\ndefineSlots<{\n    row(props: Pick<Row, 'id'>): any\n    footer(props: { note: string }): any\n}>()\n</script>\n<template><slot name=\"row\" :id=\"r\" /><slot name=\"footer\" :note=\"n\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(
        source,
        vec![
            slot("row", &["id"], verter_span::Span::new(0, 0)),
            slot("footer", &["note"], verter_span::Span::new(0, 0)),
        ],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    let titles = action_titles(&actions);

    assert!(
        !titles.iter().any(|t| t.contains("Add slot")),
        "`row` and `footer` are both declared members; titles: {titles:?}"
    );
    assert!(
        !titles.iter().any(|t| t.contains("slot 'row'")),
        "`id` is declared through the Pick surface — no prop action for `row`; titles: {titles:?}"
    );
    assert!(
        actions.is_empty(),
        "nothing is missing, so no action at all may be produced; titles: {titles:?}"
    );
}

// ── A7-02: every insertion position comes from an exact anchor ──

/// `rfind("}>")` lands inside a NESTED generic's `}>`; the anchor does not.
#[test]
fn b3_insert_offset_comes_from_type_literal_anchor() {
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: { m: Map<string, { x: number }> }): any\n} /* keep */>()\n</script>\n<template><slot name=\"header\" /><slot name=\"footer\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(
        source,
        vec![
            slot("header", &[], verter_span::Span::new(0, 0)),
            slot("footer", &[], verter_span::Span::new(0, 0)),
        ],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    let add_footer = actions
        .iter()
        .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("footer")))
        .unwrap_or_else(|| {
            panic!(
                "missing `footer` slot must be offered; titles: {:?}",
                action_titles(&actions)
            )
        });

    assert_eq!(
        apply_insert(source, &line_index, add_footer),
        "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: { m: Map<string, { x: number }> }): any\n    footer(props: {}): any\n} /* keep */>()\n</script>\n<template><slot name=\"header\" /><slot name=\"footer\" /></template>",
        "the member must land before the OUTER type literal close, never inside `Map<...>`"
    );
}

/// `find_slot_props_close` matched the slot name inside a COMMENT and inserted
/// the prop there, silently swallowing it. The anchor is the real OXC literal.
#[test]
fn b5_insert_offset_comes_from_slot_props_anchor() {
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    // header(props: { legacy: string }): any\n    header(props: { title: string }): any\n}>()\n</script>\n<template><slot name=\"header\" :title=\"t\" :subtitle=\"s\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(
        source,
        vec![slot(
            "header",
            &["title", "subtitle"],
            verter_span::Span::new(0, 0),
        )],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    let titles = action_titles(&actions);
    assert!(
        !titles.iter().any(|t| t.contains("'title'")),
        "`title` IS declared on the live member — only `subtitle` is missing; titles: {titles:?}"
    );
    let add_subtitle = actions
        .iter()
        .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("subtitle")))
        .unwrap_or_else(|| panic!("`subtitle` must be offered; titles: {titles:?}"));

    assert_eq!(
        apply_insert(source, &line_index, add_subtitle),
        "<script setup lang=\"ts\">\ndefineSlots<{\n    // header(props: { legacy: string }): any\n    header(props: { title: string , subtitle: unknown}): any\n}>()\n</script>\n<template><slot name=\"header\" :title=\"t\" :subtitle=\"s\" /></template>",
        "the prop must land in the real props object, never inside the comment"
    );
}

/// Type-based `defineEmits`: `span.end - 4` assumes `}>()` and misses a
/// trailing comment.
#[test]
fn b4_emit_insert_offset_comes_from_anchor() {
    let source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{\n    (e: 'a'): void\n} /* keep */>()\n</script>\n<template><button @click=\"emit('save')\">x</button></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(source, vec![], vec![undeclared_emit("save")]);

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    let add_emit = actions
        .iter()
        .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("save")))
        .unwrap_or_else(|| {
            panic!(
                "missing emit must be offered; titles: {:?}",
                action_titles(&actions)
            )
        });

    assert_eq!(
        apply_insert(source, &line_index, add_emit),
        "<script setup lang=\"ts\">\nconst emit = defineEmits<{\n    (e: 'a'): void\n    (e: 'save', ...args: any[]): void\n} /* keep */>()\n</script>\n<template><button @click=\"emit('save')\">x</button></template>",
        "the call signature must land before the type literal close"
    );
}

/// Runtime ARRAY `defineEmits`: `span.end - 2` assumes `])` and inserts the
/// new entry as a SECOND ARGUMENT when the call has inner spacing.
#[test]
fn b4_emit_runtime_array_insert_offset_comes_from_anchor() {
    let source = "<script setup lang=\"ts\">\nconst emit = defineEmits([ 'a' ] )\n</script>\n<template><button @click=\"emit('save')\">x</button></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(source, vec![], vec![undeclared_emit("save")]);

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    let add_emit = actions
        .iter()
        .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.contains("save")))
        .unwrap_or_else(|| {
            panic!(
                "missing emit must be offered; titles: {:?}",
                action_titles(&actions)
            )
        });

    assert_eq!(
        apply_insert(source, &line_index, add_emit),
        "<script setup lang=\"ts\">\nconst emit = defineEmits([ 'a' , 'save'] )\n</script>\n<template><button @click=\"emit('save')\">x</button></template>",
        "the entry must land inside the array, never after its close bracket \
         (pre-change `span.end - 2` made it a SECOND ARGUMENT)"
    );
}

/// Runtime OBJECT `defineEmits({ … })` has no array element list: the
/// `span.end - 2` heuristic produced `{ custom: null, 'save' }` — invalid JS.
#[test]
fn b4_emit_runtime_object_form_yields_no_action() {
    let source = "<script setup lang=\"ts\">\nconst emit = defineEmits({ custom: null })\n</script>\n<template><button @click=\"emit('save')\">x</button></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(source, vec![], vec![undeclared_emit("save")]);

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    assert!(
        actions.is_empty(),
        "no array element list to append to ⇒ zero actions; titles: {:?}",
        action_titles(&actions)
    );
    let anchors = analysis.macros[0].edit_anchors;
    assert_eq!(
        anchors.runtime_array.unsupported_reason(),
        Some(MacroAnchorUnsupported::NoMemberList),
        "a runtime OBJECT argument carries no array element list"
    );
    assert_eq!(
        anchors.type_literal.unsupported_reason(),
        Some(MacroAnchorUnsupported::NotTypeBased),
        "and the type-argument position reason stays distinct from it"
    );
}

// ── A7-04: malformed / missing anchor ⇒ typed unsupported, no action ──

/// `defineSlots<S>()`: `span.end - 4` inserted the member INSIDE the
/// identifier `S`. A bare type reference is fail-closed.
#[test]
fn named_type_argument_macro_yields_unsupported_anchor_and_no_action() {
    let source = "<script setup lang=\"ts\">\ntype S = { header(props: {}): any }\ndefineSlots<S>()\n</script>\n<template><slot name=\"header\" /><slot name=\"footer\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(
        source,
        vec![
            slot("header", &[], verter_span::Span::new(0, 0)),
            slot("footer", &[], verter_span::Span::new(0, 0)),
        ],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    assert!(
        actions.is_empty(),
        "a bare type reference has no anchorable member list ⇒ zero actions; titles: {:?}",
        action_titles(&actions)
    );
    // The reason is asserted per case: `NamedTypeArgument`, not the
    // `NoTypeArgument` default and not the `NoMemberList` catch-all.
    assert_eq!(
        analysis.macros[0]
            .edit_anchors
            .type_literal
            .unsupported_reason(),
        Some(MacroAnchorUnsupported::NamedTypeArgument)
    );
}

/// An intersection type argument has no single member-list close position.
#[test]
fn intersection_type_argument_yields_unsupported_anchor_and_no_action() {
    let source = "<script setup lang=\"ts\">\ntype A = { header(props: {}): any }\ndefineSlots<A & { nav(props: {}): any }>()\n</script>\n<template><slot name=\"header\" /><slot name=\"footer\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(
        source,
        vec![
            slot("header", &[], verter_span::Span::new(0, 0)),
            slot("footer", &[], verter_span::Span::new(0, 0)),
        ],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    assert!(
        actions.is_empty(),
        "an intersection type argument is fail-closed; titles: {:?}",
        action_titles(&actions)
    );
    assert_eq!(
        analysis.macros[0]
            .edit_anchors
            .type_literal
            .unsupported_reason(),
        Some(MacroAnchorUnsupported::IntersectionTypeArgument),
        "the intersection reason must not collapse into the named-reference one"
    );
}

/// A `Pick<…>` props surface has no member list to append to, so B5 must
/// offer nothing for that slot rather than edit a neighbour's object.
#[test]
fn pick_props_surface_yields_unsupported_slot_props_anchor() {
    let source = "<script setup lang=\"ts\">\ntype Row = { id: string, note: string }\ndefineSlots<{\n    row(props: Pick<Row, 'id'>): any\n    footer(props: { note: string }): any\n}>()\n</script>\n<template><slot name=\"row\" :id=\"r\" :extra=\"e\" /><slot name=\"footer\" :note=\"n\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let analysis = producer_backed_snapshot(
        source,
        vec![
            slot("row", &["id", "extra"], verter_span::Span::new(0, 0)),
            slot("footer", &["note"], verter_span::Span::new(0, 0)),
        ],
        vec![],
    );

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    assert!(
        actions.is_empty(),
        "the Pick surface is not appendable and `footer` is complete ⇒ zero actions; titles: {:?}",
        action_titles(&actions)
    );
    // Discriminator: `extra` genuinely IS missing from `row`'s declared
    // bindings, so the zero-action result is the fail-closed anchor, not a
    // "nothing was missing" no-op.
    let script = analyze_sfc_script(source);
    let row = script.macros[0]
        .slot_fields
        .iter()
        .find(|f| f.name == "row")
        .expect("`row` is a declared slot member");
    assert!(
        !row.bindings.iter().any(|b| b.name == "extra"),
        "`extra` is not declared on the Pick surface, so B5 had a real gap to fail closed on"
    );
    assert!(
        row.props_anchor.available().is_none(),
        "a Pick props surface publishes no appendable anchor"
    );
}

/// The panic path: anchors and spans minted from a LONGER source, applied to a
/// shorter live buffer. Pre-change this PANICKED inside `&source[mac.span]`
/// (`end byte index 70 is out of bounds for string of length 34`).
///
/// The fixture deliberately stamps the LIVE buffer's revision onto an analysis
/// whose anchors came from other bytes, so the revision gate PASSES and the
/// bounds/char-boundary check is the thing under test. The gate closes the
/// silent-miscarry class; this closes the panic class, and both are owed.
#[test]
fn anchor_out_of_bounds_for_live_source_is_unsupported() {
    let analyzed = "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: {}): any\n}>()\nconst padding_that_makes_this_much_longer = 1\n</script>\n<template><slot name=\"header\" /><slot name=\"footer\" /></template>";
    let live = "<script setup lang=\"ts\">\n</script>";
    let line_index = LineIndex::new_utf16(live);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(live);
    let mut analysis = producer_backed_snapshot(
        analyzed,
        vec![
            slot("header", &[], verter_span::Span::new(0, 0)),
            slot("footer", &[], verter_span::Span::new(0, 0)),
        ],
        vec![],
    );
    // Sanity: the anchor really does address bytes the live buffer does not have.
    let anchor_offset = analysis.macros[0]
        .edit_anchors
        .type_literal
        .available()
        .expect("the fixture mints an available anchor")
        .insert_offset() as usize;
    assert!(
        anchor_offset > live.len(),
        "fixture must produce an out-of-range anchor ({anchor_offset} vs {})",
        live.len()
    );
    analysis.anchor_revision = AnalysisSourceRevision::of_source(live);

    let actions = macro_code_actions(
        live,
        AnalysisSourceRevision::of_source(live),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    assert!(
        actions.is_empty(),
        "an out-of-range anchor is a typed miss ⇒ zero actions and no panic; titles: {:?}",
        action_titles(&actions)
    );
}

// ── A7-03: an analysis from another source revision is rejected ──

/// Two failure shapes, plus the control that proves the rejection is the gate
/// and not "the action was never offered".
#[test]
fn macro_actions_reject_analysis_from_a_different_source_revision() {
    let analyzed = "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: {}): any\n}>()\n</script>\n<template><slot name=\"header\" /><slot name=\"footer\" /></template>";
    let slots = || {
        vec![
            slot("header", &[], verter_span::Span::new(0, 0)),
            slot("footer", &[], verter_span::Span::new(0, 0)),
        ]
    };
    let analysis = producer_backed_snapshot(analyzed, slots(), vec![]);

    // Control: same bytes ⇒ the action IS offered, so the fixture is live.
    let line_index = LineIndex::new_utf16(analyzed);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(analyzed);
    let control = macro_code_actions(
        analyzed,
        AnalysisSourceRevision::of_source(analyzed),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    assert!(
        action_titles(&control).iter().any(|t| t.contains("footer")),
        "control: a matching revision must still serve the action; titles: {:?}",
        action_titles(&control)
    );

    // (i) Shorter live source — the pre-change panic case.
    let shorter = "<script setup lang=\"ts\">\ndefineSlots<{}>()\n</script>\n<template><slot name=\"footer\" /></template>";
    let shorter_index = LineIndex::new_utf16(shorter);
    let shorter_blocks = crate::documents::carrier_structure::test_carrier_blocks(shorter);
    let actions = macro_code_actions(
        shorter,
        AnalysisSourceRevision::of_source(shorter),
        Some(&analysis),
        &shorter_blocks,
        &shorter_index,
        None,
    );
    assert!(
        actions.is_empty(),
        "a shorter live buffer is another revision ⇒ zero actions and no panic; titles: {:?}",
        action_titles(&actions)
    );

    // (ii) Same LENGTH, different content — the silent-miscarry case that a
    // bounds check alone cannot catch.
    let same_length = analyzed.replace("header(props", "heaAAr(props");
    assert_eq!(
        same_length.len(),
        analyzed.len(),
        "fixture must match length"
    );
    assert_ne!(same_length, analyzed, "fixture must differ in content");
    let same_index = LineIndex::new_utf16(&same_length);
    let same_blocks = crate::documents::carrier_structure::test_carrier_blocks(&same_length);
    let actions = macro_code_actions(
        &same_length,
        AnalysisSourceRevision::of_source(&same_length),
        Some(&analysis),
        &same_blocks,
        &same_index,
        None,
    );
    assert!(
        actions.is_empty(),
        "an in-bounds offset from another revision must still be refused; titles: {:?}",
        action_titles(&actions)
    );
}

/// An UNSTAMPED analysis (a producer that recorded no source identity, or a
/// `Default`-constructed snapshot) fails the gate rather than editing from
/// unpaired geometry.
#[test]
fn macro_actions_reject_analysis_with_unstamped_revision() {
    let source = "<script setup lang=\"ts\">\ndefineSlots<{\n    header(props: {}): any\n}>()\n</script>\n<template><slot name=\"header\" /><slot name=\"footer\" /></template>";
    let line_index = LineIndex::new_utf16(source);
    let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
    let mut analysis = producer_backed_snapshot(
        source,
        vec![
            slot("header", &[], verter_span::Span::new(0, 0)),
            slot("footer", &[], verter_span::Span::new(0, 0)),
        ],
        vec![],
    );
    analysis.anchor_revision = AnalysisSourceRevision::default();
    assert!(analysis.anchor_revision.is_unstamped());

    let actions = macro_code_actions(
        source,
        AnalysisSourceRevision::of_source(source),
        Some(&analysis),
        &blocks,
        &line_index,
        None,
    );
    assert!(
        actions.is_empty(),
        "an unstamped analysis fails closed; titles: {:?}",
        action_titles(&actions)
    );
}
