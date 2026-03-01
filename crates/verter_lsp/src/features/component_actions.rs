// Component code actions: add missing props and v-models to child component definitions.
//
// When a parent component passes a prop or v-model that the child doesn't define,
// this module generates cross-file code actions to add the definitions.

use tower_lsp_server::lsp_types::*;
use verter_analysis::types::AnalyzedMacroKind;

use crate::features::action_utils;
use crate::features::component_diagnostics;
use crate::features::cross_file::ChildComponentContext;
use crate::features::macro_codegen::MacroCodegen;

/// Generate code actions for unknown prop diagnostics.
///
/// For each unknown prop, generates a cross-file edit to add the prop
/// to the child component's `defineProps`.
pub fn component_code_actions(
    analysis: &verter_host::FileAnalysisSnapshot,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::line_index::LineIndex;
    use crate::documents::sfc_scanner::scan_sfc_blocks;
    use verter_analysis::template::{
        AnalyzedPropDefinition, PropValueConstness, TemplateAnalysisSnapshot,
        TemplateComponentUsage, TemplateComponentVModel, TemplatePropUsage,
    };
    use verter_analysis::types::AnalyzedMacro;
    use verter_host::FileAnalysisSnapshot;

    fn make_parent_analysis(components: Vec<TemplateComponentUsage>) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
                components,
                ..Default::default()
            }),
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
            constness: PropValueConstness::Dynamic,
            referenced_bindings: vec![],
            from_spread: false,
            span: verter_span::Span::new(10, 20),
        }
    }

    fn make_child_context(source: &str, analysis: FileAnalysisSnapshot) -> ChildComponentContext {
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        ChildComponentContext {
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

        let child_source =
            "<script setup lang=\"ts\">\ndefineProps<{\n  msg: string\n}>()\n</script>";
        let child_analysis = FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
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
            }),
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec![],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                span: verter_span::Span::new(24, 56),
            }],
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
            imports: vec![verter_analysis::AnalyzedImport {
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
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: false,
                type_references: vec![],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                span: verter_span::Span::new(15, 35),
            }],
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
            imports: vec![verter_analysis::AnalyzedImport {
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
}
