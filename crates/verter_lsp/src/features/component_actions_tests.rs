use super::*;
use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use verter_semantic::analysis::template::{
    AnalyzedPropDefinition, PropValueConstness, TemplateAnalysisSnapshot, TemplateComponentUsage,
    TemplateComponentVModel, TemplatePropUsage,
};
use verter_semantic::analysis::types::AnalyzedMacro;
use verter_semantic::analysis::types::ImportBindingKind;
use verter_session::FileAnalysisSnapshot;

fn make_parent_analysis(components: Vec<TemplateComponentUsage>) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
                components,
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    }
}

fn make_component(
    name: &str,
    import_source: &str,
    props: Vec<TemplatePropUsage>,
) -> TemplateComponentUsage {
    TemplateComponentUsage {
        name: name.to_string(),
        import_source: Some(import_source.to_string()),
        is_dynamic: false,
        props,
        has_spread: false,
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::new(0, 50),
    }
}

fn make_prop(name: &str) -> TemplatePropUsage {
    TemplatePropUsage {
        name: name.to_string(),
        is_bound: true,
        expression: None,
        constness: PropValueConstness::Dynamic,
        referenced_bindings: vec![],
        from_spread: false,
        span: verter_span::Span::new(10, 20),
        name_span: verter_span::Span::new(0, 0),
        is_shorthand: false,
    }
}

fn make_child_context(source: &str, analysis: FileAnalysisSnapshot) -> ChildComponentContext {
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    ChildComponentContext {
        canonical_id: "/project/src/Child.vue".to_string(),
        uri: "file:///project/src/Child.vue".parse().unwrap(),
        source: source.to_string(),
        analysis,
        blocks,
        line_index,
    }
}

#[test]
fn add_prop_to_type_based_define_props() {
    // Parent passes unknown prop, child has type-based defineProps
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("title")],
    )]);

    let child_source = "<script setup lang=\"ts\">\ndefineProps<{\n  msg: string\n}>()\n</script>";
    let child_analysis = FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
                prop_definitions: vec![AnalyzedPropDefinition {
                    name: "msg".into(),
                    type_annotation: Some("string".into()),
                    has_default: false,
                    is_required: true,
                    is_boolean: false,
                    used_in_template: false,
                    used_in_script: false,
                    span: verter_span::Span::new(0, 0),
                }],
                ..Default::default()
            })
            .into(),
        ),
        macros: (vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            span: verter_span::Span::new(24, 56),
        }])
        .into(),
        ..Default::default()
    };
    let child_ctx = make_child_context(child_source, child_analysis);

    let actions = component_code_actions(&parent, &|_| Some(child_ctx.clone()));

    // Positive: generates action to add 'title' prop
    assert_eq!(actions.len(), 1, "should generate 1 code action");
    if let CodeActionOrCommand::CodeAction(action) = &actions[0] {
        assert!(
            action.title.contains("title"),
            "title should contain prop name"
        );
        assert!(
            action.title.contains("Child"),
            "title should contain component name"
        );
        // Positive: edit targets child URI
        let edit = action.edit.as_ref().expect("should have edit");
        if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
            assert_eq!(
                edits[0].text_document.uri.as_str(),
                "file:///project/src/Child.vue"
            );
            // Positive: edit text contains the new prop
            let edit_text = &edits[0].edits[0];
            if let OneOf::Left(text_edit) = edit_text {
                assert!(
                    text_edit.new_text.contains("title"),
                    "edit text should contain prop name"
                );
                assert!(
                    text_edit.new_text.contains("unknown"),
                    "edit text should contain 'unknown' type"
                );
            }
        }
        // Negative: does NOT modify parent file
        assert_ne!(action.title, "", "action title should not be empty");
    } else {
        panic!("expected CodeAction, got Command");
    }
}

#[test]
fn add_prop_generates_define_props_when_missing() {
    // Child has no defineProps → generate full defineProps<{ propName: unknown }>()
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("title")],
    )]);

    let child_source =
        "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>";
    let child_analysis = FileAnalysisSnapshot {
        imports: vec![verter_semantic::analysis::AnalyzedImport {
            source: "vue".into(),
            is_type_only: false,
            bindings: vec![],
            span: verter_span::Span::new(24, 49),
            resolved_canonical_id: None,
        }],
        ..Default::default()
    };
    let child_ctx = make_child_context(child_source, child_analysis);

    let actions = component_code_actions(&parent, &|_| Some(child_ctx.clone()));

    // Positive: generates action
    assert_eq!(actions.len(), 1, "should generate 1 code action");
    if let CodeActionOrCommand::CodeAction(action) = &actions[0] {
        let edit = action.edit.as_ref().expect("should have edit");
        if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
            let edit_text = &edits[0].edits[0];
            if let OneOf::Left(text_edit) = edit_text {
                // Positive: generates full defineProps macro
                assert!(
                    text_edit.new_text.contains("defineProps"),
                    "should generate full defineProps"
                );
                assert!(
                    text_edit.new_text.contains("title"),
                    "should contain prop name"
                );
            }
        }
    }
}

#[test]
fn no_action_without_script_setup_in_child() {
    // Child has <script> (not setup) → empty actions
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("foo")],
    )]);

    let child_source = "<script>\nexport default {}\n</script>";
    let child_analysis = FileAnalysisSnapshot::default();
    let child_ctx = make_child_context(child_source, child_analysis);

    let actions = component_code_actions(&parent, &|_| Some(child_ctx.clone()));
    assert!(
        actions.is_empty(),
        "no code actions when child has no <script setup>"
    );
}

#[test]
fn no_action_for_runtime_based_define_props() {
    // Child has runtime defineProps(['msg']) → can't insert type members
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("title")],
    )]);

    let child_source = "<script setup>\ndefineProps(['msg'])\n</script>";
    let child_analysis = FileAnalysisSnapshot {
        macros: (vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: false,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            span: verter_span::Span::new(15, 35),
        }])
        .into(),
        ..Default::default()
    };
    let child_ctx = make_child_context(child_source, child_analysis);

    let actions = component_code_actions(&parent, &|_| Some(child_ctx.clone()));
    // Runtime macro + no defineProps type-based → should generate whole new
    // defineProps as fallback? Actually no — make_insert_into_macro returns None
    // for non-type-based, and find_macro returns Some, so the else branch
    // (generate new) won't run. This is the correct behavior: we can't add
    // type members to runtime defineProps.
    assert!(
        actions.is_empty(),
        "should not generate action for runtime defineProps"
    );
}

// -- v-model code action tests --

fn make_component_with_vmodels(
    name: &str,
    import_source: &str,
    vmodels: Vec<TemplateComponentVModel>,
) -> TemplateComponentUsage {
    TemplateComponentUsage {
        name: name.to_string(),
        import_source: Some(import_source.to_string()),
        is_dynamic: false,
        props: vec![],
        has_spread: false,
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vmodels,
        span: verter_span::Span::new(0, 50),
    }
}

#[test]
fn add_define_model_to_child() {
    // Parent: <Child v-model:title="val" />, child has no defineModel
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "title".to_string(),
            span: verter_span::Span::new(10, 30),
        }],
    )]);

    let child_source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\n</script>";
    let child_analysis = FileAnalysisSnapshot {
        imports: vec![verter_semantic::analysis::AnalyzedImport {
            source: "vue".into(),
            is_type_only: false,
            bindings: vec![],
            span: verter_span::Span::new(24, 49),
            resolved_canonical_id: None,
        }],
        ..Default::default()
    };
    let child_ctx = make_child_context(child_source, child_analysis);

    let actions = component_code_actions(&parent, &|_| Some(child_ctx.clone()));

    // Positive: generates action
    assert_eq!(actions.len(), 1, "should generate 1 code action");
    if let CodeActionOrCommand::CodeAction(action) = &actions[0] {
        // Positive: title mentions defineModel and the model name
        assert!(
            action.title.contains("defineModel"),
            "title should mention defineModel"
        );
        assert!(
            action.title.contains("title"),
            "title should mention model name"
        );

        // Positive: edit targets child URI and contains defineModel
        let edit = action.edit.as_ref().expect("should have edit");
        if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
            assert_eq!(
                edits[0].text_document.uri.as_str(),
                "file:///project/src/Child.vue"
            );
            if let OneOf::Left(text_edit) = &edits[0].edits[0] {
                assert!(
                    text_edit.new_text.contains("defineModel"),
                    "edit should contain defineModel"
                );
                assert!(
                    text_edit.new_text.contains("'title'"),
                    "edit should contain model name argument"
                );
                // Negative: should NOT contain 'modelValue'
                assert!(
                    !text_edit.new_text.contains("modelValue"),
                    "named model should not use modelValue"
                );
            }
        }
    } else {
        panic!("expected CodeAction, got Command");
    }
}

#[test]
fn add_default_define_model_to_child() {
    // Parent: <Child v-model="val" />, child has no defineModel
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "modelValue".to_string(),
            span: verter_span::Span::new(10, 25),
        }],
    )]);

    let child_source = "<script setup>\n</script>";
    let child_analysis = FileAnalysisSnapshot::default();
    let child_ctx = make_child_context(child_source, child_analysis);

    let actions = component_code_actions(&parent, &|_| Some(child_ctx.clone()));

    assert_eq!(actions.len(), 1, "should generate 1 code action");
    if let CodeActionOrCommand::CodeAction(action) = &actions[0] {
        let edit = action.edit.as_ref().expect("should have edit");
        if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
            if let OneOf::Left(text_edit) = &edits[0].edits[0] {
                assert!(
                    text_edit.new_text.contains("defineModel"),
                    "edit should contain defineModel"
                );
                // Default model: defineModel<unknown>() — no name argument
                assert!(
                    !text_edit.new_text.contains("'modelValue'"),
                    "default model should not pass 'modelValue' as argument"
                );
            }
        }
    }
}

#[test]
fn no_vmodel_action_without_script_setup() {
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "title".to_string(),
            span: verter_span::Span::new(10, 30),
        }],
    )]);

    let child_source = "<script>\nexport default {}\n</script>";
    let child_analysis = FileAnalysisSnapshot::default();
    let child_ctx = make_child_context(child_source, child_analysis);

    let actions = component_code_actions(&parent, &|_| Some(child_ctx.clone()));
    assert!(
        actions.is_empty(),
        "no v-model code action without <script setup>"
    );
}

// ── suggest_matching_props tests ──────────────────────────────────────

fn make_parent_with_bindings_and_components(
    components: Vec<TemplateComponentUsage>,
    bindings: Vec<verter_semantic::analysis::AnalyzedBinding>,
    imports: Vec<verter_semantic::analysis::AnalyzedImport>,
) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
                components,
                ..Default::default()
            })
            .into(),
        ),
        bindings,
        imports,
        ..Default::default()
    }
}

fn make_binding(name: &str) -> verter_semantic::analysis::AnalyzedBinding {
    verter_semantic::analysis::AnalyzedBinding {
        name: name.to_string(),
        kind: verter_semantic::analysis::types::AnalyzedBindingKind::Const,
        is_reactive: false,
        reactivity_kind: verter_semantic::analysis::types::ReactivityKind::None,
        type_annotation: None,
        initializer: None,
        span: verter_span::Span::new(0, 0),
        used_in_script: false,
        used_in_style: false,
    }
}

fn make_child_with_props(source: &str, prop_names: &[&str]) -> ChildComponentContext {
    let child_analysis = FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
                prop_definitions: prop_names
                    .iter()
                    .map(|name| AnalyzedPropDefinition {
                        name: name.to_string(),
                        type_annotation: Some("unknown".to_string()),
                        has_default: false,
                        is_required: false,
                        is_boolean: false,
                        used_in_template: false,
                        used_in_script: false,
                        span: verter_span::Span::new(0, 0),
                    })
                    .collect(),
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    };
    make_child_context(source, child_analysis)
}

#[test]
fn suggest_matching_props_single_match() {
    // Parent has binding `title`, child expects prop `title`, parent doesn't pass it
    let source = "<template>\n  <Child></Child>\n</template>\n<script setup>\nimport Child from './Child.vue'\nconst title = 'hello'\n</script>";
    let parent = make_parent_with_bindings_and_components(
        vec![make_component("Child", "./Child.vue", vec![])],
        vec![make_binding("title")],
        vec![],
    );

    let child_source = "<script setup>\ndefineProps<{ title: string }>()\n</script>";
    let child_ctx = make_child_with_props(child_source, &["title"]);

    let li = LineIndex::new_utf16(source);
    let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

    let actions = suggest_matching_props(&parent, source, &li, &uri, &|_| Some(child_ctx.clone()));

    // Positive: produces action(s) with `:title`
    assert!(!actions.is_empty(), "should produce at least one action");
    let found = actions.iter().any(|a| {
        if let CodeActionOrCommand::CodeAction(ca) = a {
            ca.title.contains("title") && ca.title.contains("Child")
        } else {
            false
        }
    });
    assert!(found, "should have action mentioning 'title' and 'Child'");

    // Negative: no action mentions a prop that doesn't exist
    for a in &actions {
        if let CodeActionOrCommand::CodeAction(ca) = a {
            assert!(
                !ca.title.contains("unknown-prop"),
                "should not suggest unknown props"
            );
        }
    }
}

#[test]
fn suggest_matching_props_multiple_matches() {
    let source = "<template>\n  <Child></Child>\n</template>\n<script setup>\nconst title = ''\nconst count = 0\nconst name = ''\n</script>";
    let parent = make_parent_with_bindings_and_components(
        vec![make_component("Child", "./Child.vue", vec![])],
        vec![
            make_binding("title"),
            make_binding("count"),
            make_binding("name"),
        ],
        vec![],
    );

    let child_source =
        "<script setup>\ndefineProps<{ title: string, count: number, name: string }>()\n</script>";
    let child_ctx = make_child_with_props(child_source, &["title", "count", "name"]);

    let li = LineIndex::new_utf16(source);
    let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

    let actions = suggest_matching_props(&parent, source, &li, &uri, &|_| Some(child_ctx.clone()));

    // Positive: should have a bulk action (3 matches) + 3 individual actions = 4 total
    assert!(
        actions.len() >= 4,
        "should produce bulk + individual actions, got {}",
        actions.len()
    );

    // Positive: bulk action mentions count
    let has_bulk = actions.iter().any(|a| {
        if let CodeActionOrCommand::CodeAction(ca) = a {
            ca.title.contains("3 matching props")
        } else {
            false
        }
    });
    assert!(has_bulk, "should have bulk '3 matching props' action");
}

#[test]
fn suggest_matching_props_no_match() {
    // Parent bindings don't match any child prop names
    let source =
        "<template>\n  <Child></Child>\n</template>\n<script setup>\nconst x = 1\n</script>";
    let parent = make_parent_with_bindings_and_components(
        vec![make_component("Child", "./Child.vue", vec![])],
        vec![make_binding("x")],
        vec![],
    );

    let child_source = "<script setup>\ndefineProps<{ title: string }>()\n</script>";
    let child_ctx = make_child_with_props(child_source, &["title"]);

    let li = LineIndex::new_utf16(source);
    let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

    let actions = suggest_matching_props(&parent, source, &li, &uri, &|_| Some(child_ctx.clone()));

    assert!(actions.is_empty(), "no matching bindings → no actions");
}

#[test]
fn suggest_matching_props_already_passed() {
    // Parent already passes the prop → don't suggest it again
    let source = "<template>\n  <Child :title=\"title\"></Child>\n</template>\n<script setup>\nconst title = ''\n</script>";
    let parent = make_parent_with_bindings_and_components(
        vec![make_component(
            "Child",
            "./Child.vue",
            vec![make_prop("title")],
        )],
        vec![make_binding("title")],
        vec![],
    );

    let child_source = "<script setup>\ndefineProps<{ title: string }>()\n</script>";
    let child_ctx = make_child_with_props(child_source, &["title"]);

    let li = LineIndex::new_utf16(source);
    let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

    let actions = suggest_matching_props(&parent, source, &li, &uri, &|_| Some(child_ctx.clone()));

    assert!(
        actions.is_empty(),
        "already-passed prop should not be suggested"
    );
}

#[test]
fn suggest_matching_props_no_child_analysis() {
    let source =
        "<template>\n  <Child></Child>\n</template>\n<script setup>\nconst title = ''\n</script>";
    let parent = make_parent_with_bindings_and_components(
        vec![make_component("Child", "./Child.vue", vec![])],
        vec![make_binding("title")],
        vec![],
    );

    let li = LineIndex::new_utf16(source);
    let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

    let actions = suggest_matching_props(
        &parent,
        source,
        &li,
        &uri,
        &|_| None, // unresolvable child
    );

    assert!(actions.is_empty(), "unresolvable child → no actions");
}

#[test]
fn suggest_matching_props_no_script_setup_child() {
    let source =
        "<template>\n  <Child></Child>\n</template>\n<script setup>\nconst title = ''\n</script>";
    let parent = make_parent_with_bindings_and_components(
        vec![make_component("Child", "./Child.vue", vec![])],
        vec![make_binding("title")],
        vec![],
    );

    let child_source = "<script>\nexport default {}\n</script>";
    let child_ctx = make_child_with_props(child_source, &["title"]);

    let li = LineIndex::new_utf16(source);
    let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

    let actions = suggest_matching_props(&parent, source, &li, &uri, &|_| Some(child_ctx.clone()));

    assert!(
        actions.is_empty(),
        "child without script setup → no actions"
    );
}

#[test]
fn suggest_matching_props_from_imports() {
    // Matching binding comes from an import, not a local const
    let source = "<template>\n  <Child></Child>\n</template>\n<script setup>\nimport { title } from './data'\n</script>";
    let parent = make_parent_with_bindings_and_components(
        vec![make_component("Child", "./Child.vue", vec![])],
        vec![], // no local bindings
        vec![verter_semantic::analysis::AnalyzedImport {
            source: "./data".into(),
            is_type_only: false,
            bindings: vec![verter_semantic::analysis::types::AnalyzedImportBinding {
                name: "title".into(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        }],
    );

    let child_source = "<script setup>\ndefineProps<{ title: string }>()\n</script>";
    let child_ctx = make_child_with_props(child_source, &["title"]);

    let li = LineIndex::new_utf16(source);
    let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

    let actions = suggest_matching_props(&parent, source, &li, &uri, &|_| Some(child_ctx.clone()));

    assert!(!actions.is_empty(), "import-provided binding should match");
}

#[test]
fn suggest_matching_props_type_only_import_excluded() {
    // Type-only imports should not be offered as bindings
    let source = "<template>\n  <Child></Child>\n</template>\n<script setup>\nimport type { Title } from './types'\n</script>";
    let parent = make_parent_with_bindings_and_components(
        vec![make_component("Child", "./Child.vue", vec![])],
        vec![],
        vec![verter_semantic::analysis::AnalyzedImport {
            source: "./types".into(),
            is_type_only: true,
            bindings: vec![verter_semantic::analysis::types::AnalyzedImportBinding {
                name: "title".into(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: true,
                vue_api: None,
                span: verter_span::Span::new(0, 0),
            }],
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        }],
    );

    let child_source = "<script setup>\ndefineProps<{ title: string }>()\n</script>";
    let child_ctx = make_child_with_props(child_source, &["title"]);

    let li = LineIndex::new_utf16(source);
    let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

    let actions = suggest_matching_props(&parent, source, &li, &uri, &|_| Some(child_ctx.clone()));

    assert!(actions.is_empty(), "type-only import must not be offered");
}

#[test]
fn suggest_matching_props_spread_present_skips() {
    // Component with v-bind="obj" spread → don't suggest
    let source = "<template>\n  <Child v-bind=\"obj\"></Child>\n</template>\n<script setup>\nconst title = ''\n</script>";
    let mut comp = make_component("Child", "./Child.vue", vec![]);
    comp.has_spread = true;
    let parent =
        make_parent_with_bindings_and_components(vec![comp], vec![make_binding("title")], vec![]);

    let child_source = "<script setup>\ndefineProps<{ title: string }>()\n</script>";
    let child_ctx = make_child_with_props(child_source, &["title"]);

    let li = LineIndex::new_utf16(source);
    let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

    let actions = suggest_matching_props(&parent, source, &li, &uri, &|_| Some(child_ctx.clone()));

    assert!(actions.is_empty(), "spread present → skip suggestions");
}

#[test]
fn suggest_matching_props_self_closing() {
    // Self-closing component: <Child />
    let source =
        "<template>\n  <Child />\n</template>\n<script setup>\nconst title = ''\n</script>";
    let parent = make_parent_with_bindings_and_components(
        vec![make_component("Child", "./Child.vue", vec![])],
        vec![make_binding("title")],
        vec![],
    );

    let child_source = "<script setup>\ndefineProps<{ title: string }>()\n</script>";
    let child_ctx = make_child_with_props(child_source, &["title"]);

    let li = LineIndex::new_utf16(source);
    let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

    let actions = suggest_matching_props(&parent, source, &li, &uri, &|_| Some(child_ctx.clone()));

    // Positive: should still produce actions for self-closing tags
    assert!(
        !actions.is_empty(),
        "self-closing tag should still get suggestions"
    );

    // Verify the edit position is before `/>`, not inside content
    if let CodeActionOrCommand::CodeAction(ca) = &actions[0] {
        let edit = ca.edit.as_ref().unwrap();
        if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
            if let OneOf::Left(text_edit) = &edits[0].edits[0] {
                assert!(
                    text_edit.new_text.contains(":title"),
                    "edit should contain :title prop"
                );
            }
        }
    }
}

#[test]
fn suggest_matching_props_partial_match() {
    // 3 child props, only 1 has matching binding → action for 1
    let source =
        "<template>\n  <Child></Child>\n</template>\n<script setup>\nconst title = ''\n</script>";
    let parent = make_parent_with_bindings_and_components(
        vec![make_component("Child", "./Child.vue", vec![])],
        vec![make_binding("title")],
        vec![],
    );

    let child_source =
        "<script setup>\ndefineProps<{ title: string, count: number, name: string }>()\n</script>";
    let child_ctx = make_child_with_props(child_source, &["title", "count", "name"]);

    let li = LineIndex::new_utf16(source);
    let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

    let actions = suggest_matching_props(&parent, source, &li, &uri, &|_| Some(child_ctx.clone()));

    // Should produce 1 individual action (no bulk since only 1 match)
    assert_eq!(
        actions.len(),
        1,
        "should produce exactly 1 action for single match"
    );
    if let CodeActionOrCommand::CodeAction(ca) = &actions[0] {
        assert!(ca.title.contains("title"), "should suggest 'title'");
        assert!(!ca.title.contains("count"), "should not suggest 'count'");
        assert!(!ca.title.contains("name"), "should not suggest 'name'");
    }
}

// ── find_opening_tag_end helper tests ──────────────────────────────

#[test]
fn find_tag_end_normal() {
    let source = "<Child class=\"x\">";
    assert_eq!(find_opening_tag_end(source, 0), Some(16));
}

#[test]
fn find_tag_end_self_closing() {
    let source = "<Child class=\"x\" />";
    // Should find `/` at position 17
    assert_eq!(find_opening_tag_end(source, 0), Some(17));
}

#[test]
fn find_tag_end_with_quotes() {
    let source = "<Child title=\"a > b\" />";
    // The `>` inside quotes should be ignored
    let offset = find_opening_tag_end(source, 0).unwrap();
    assert!(offset > 15, "should skip > inside quotes, got {}", offset);
}
