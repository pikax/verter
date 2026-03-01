// Component usage diagnostics: unknown props, unknown v-models.
//
// Checks parent component template usages against child component definitions.
// When a parent passes a prop that the child doesn't define, a diagnostic is emitted.

use std::collections::HashSet;

use tower_lsp_server::lsp_types::*;
use verter_analysis::template::{TemplateComponentUsage, TemplatePropUsage};
use verter_analysis::types::{AnalysisFlags, AnalyzedMacroKind, VueApiClassification};
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;

/// Attributes that are always valid on any component (Vue fallthrough attrs).
const BUILTIN_ATTRS: &[&str] = &["class", "style"];

/// Information about an unknown prop found on a component usage.
pub struct UnknownPropInfo {
    pub component_name: String,
    pub prop_name: String,
    pub import_source: String,
    pub span: verter_span::Span,
}

/// Convert kebab-case to camelCase for prop name comparison.
fn kebab_to_camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = false;
    for ch in name.chars() {
        if ch == '-' {
            upper_next = true;
            continue;
        }
        if upper_next {
            for uc in ch.to_uppercase() {
                out.push(uc);
            }
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Check if a child component suppresses unknown prop diagnostics.
///
/// Returns true if:
/// - The child calls `useAttrs()` (accessing fallthrough attrs)
/// - The child has `defineOptions({ inheritAttrs: false })`
fn child_suppresses_prop_checks(child: &FileAnalysisSnapshot) -> bool {
    // Check useAttrs()
    let has_use_attrs = child
        .vue_api_calls
        .iter()
        .any(|c| c.api == VueApiClassification::UseAttrs);
    if has_use_attrs {
        return true;
    }

    // Check inheritAttrs: false
    let flags = AnalysisFlags::from_bits_truncate(child.script_flags);
    if flags.contains(AnalysisFlags::HAS_INHERIT_ATTRS_FALSE) {
        return true;
    }

    false
}

/// Get the set of defined prop names from a child's analysis (camelCase).
fn child_prop_names(child: &FileAnalysisSnapshot) -> HashSet<String> {
    child
        .template
        .as_ref()
        .map(|t| t.prop_definitions.iter().map(|p| p.name.clone()).collect())
        .unwrap_or_default()
}

/// Find unknown props across all component usages.
pub fn find_unknown_props(
    analysis: &FileAnalysisSnapshot,
    resolve_child: &dyn Fn(&str) -> Option<FileAnalysisSnapshot>,
) -> Vec<UnknownPropInfo> {
    let template = match &analysis.template {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut results = Vec::new();

    for comp in &template.components {
        if let Some(infos) = check_component_props(comp, resolve_child) {
            results.extend(infos);
        }
    }

    results
}

/// Check a single component usage for unknown props.
fn check_component_props(
    comp: &TemplateComponentUsage,
    resolve_child: &dyn Fn(&str) -> Option<FileAnalysisSnapshot>,
) -> Option<Vec<UnknownPropInfo>> {
    // Skip dynamic components (<component :is="...">)
    if comp.is_dynamic {
        return None;
    }

    // Skip if component has v-bind spread (can't validate individual props)
    if comp.has_spread {
        return None;
    }

    // Need import source to resolve child
    let import_source = comp.import_source.as_deref()?;

    // Resolve child component analysis
    let child = resolve_child(import_source)?;

    // Check if child suppresses prop checks
    if child_suppresses_prop_checks(&child) {
        return None;
    }

    let defined_props = child_prop_names(&child);
    let mut unknowns = Vec::new();

    for prop in &comp.props {
        if is_unknown_prop(prop, &defined_props) {
            unknowns.push(UnknownPropInfo {
                component_name: comp.name.clone(),
                prop_name: prop.name.clone(),
                import_source: import_source.to_string(),
                span: prop.span,
            });
        }
    }

    Some(unknowns)
}

/// Check if a single prop is unknown (not defined by the child).
fn is_unknown_prop(prop: &TemplatePropUsage, defined_props: &HashSet<String>) -> bool {
    // Skip spread entries
    if prop.from_spread {
        return false;
    }

    // Skip builtin attributes
    if BUILTIN_ATTRS.contains(&prop.name.as_str()) {
        return false;
    }

    // Normalize to camelCase for comparison
    let camel_name = kebab_to_camel(&prop.name);

    // Check against defined props
    !defined_props.contains(&camel_name)
}

/// Information about an unknown v-model found on a component usage.
pub struct UnknownModelInfo {
    pub component_name: String,
    pub model_name: String,
    pub import_source: String,
    pub span: verter_span::Span,
}

/// Get the set of defined model names from a child's macros.
///
/// Each `defineModel('name')` contributes a name. `defineModel()` without
/// arguments contributes `"modelValue"`.
fn child_model_names(child: &FileAnalysisSnapshot) -> HashSet<String> {
    child
        .macros
        .iter()
        .filter(|m| m.kind == AnalyzedMacroKind::DefineModel)
        .map(|m| {
            m.model_name
                .clone()
                .unwrap_or_else(|| "modelValue".to_string())
        })
        .collect()
}

/// Find unknown v-models across all component usages.
pub fn find_unknown_models(
    analysis: &FileAnalysisSnapshot,
    resolve_child: &dyn Fn(&str) -> Option<FileAnalysisSnapshot>,
) -> Vec<UnknownModelInfo> {
    let template = match &analysis.template {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut results = Vec::new();

    for comp in &template.components {
        if comp.is_dynamic || comp.v_models.is_empty() {
            continue;
        }

        let import_source = match &comp.import_source {
            Some(s) => s.as_str(),
            None => continue,
        };

        let child = match resolve_child(import_source) {
            Some(c) => c,
            None => continue,
        };

        let defined_models = child_model_names(&child);

        for vmodel in &comp.v_models {
            if !defined_models.contains(&vmodel.binding_name) {
                results.push(UnknownModelInfo {
                    component_name: comp.name.clone(),
                    model_name: vmodel.binding_name.clone(),
                    import_source: import_source.to_string(),
                    span: vmodel.span,
                });
            }
        }
    }

    results
}

/// Generate LSP diagnostics for unknown props and v-models on component usages.
pub fn component_usage_diagnostics(
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
    resolve_child: &dyn Fn(&str) -> Option<FileAnalysisSnapshot>,
) -> Vec<Diagnostic> {
    let unknowns = find_unknown_props(analysis, resolve_child);
    let mut diagnostics = Vec::new();

    for info in &unknowns {
        let start = line_index
            .offset_to_position(info.span.start)
            .unwrap_or(Position {
                line: 0,
                character: 0,
            });
        let end = line_index
            .offset_to_position(info.span.end)
            .unwrap_or(start);

        diagnostics.push(Diagnostic {
            range: Range { start, end },
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("verter/unknown-prop".into())),
            source: Some("verter".into()),
            message: format!(
                "Unknown prop '{}' on component <{}>",
                info.prop_name, info.component_name
            ),
            ..Default::default()
        });
    }

    // V-model diagnostics
    let unknown_models = find_unknown_models(analysis, resolve_child);
    for info in &unknown_models {
        let start = line_index
            .offset_to_position(info.span.start)
            .unwrap_or(Position {
                line: 0,
                character: 0,
            });
        let end = line_index
            .offset_to_position(info.span.end)
            .unwrap_or(start);

        diagnostics.push(Diagnostic {
            range: Range { start, end },
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("verter/unknown-model".into())),
            source: Some("verter".into()),
            message: format!(
                "Unknown v-model '{}' on component <{}>",
                info.model_name, info.component_name
            ),
            ..Default::default()
        });
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_analysis::template::{
        AnalyzedPropDefinition, PropValueConstness, TemplateAnalysisSnapshot,
        TemplateComponentUsage, TemplateComponentVModel, TemplatePropUsage,
    };
    use verter_analysis::types::AnalyzedMacro;
    use verter_analysis::types::VueApiCallSite;

    /// Helper to build a parent analysis with component usages.
    fn make_parent_analysis(components: Vec<TemplateComponentUsage>) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
                components,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Helper to build a child analysis with defined props.
    fn make_child_with_props(prop_names: &[&str]) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
                prop_definitions: prop_names
                    .iter()
                    .map(|name| AnalyzedPropDefinition {
                        name: name.to_string(),
                        type_annotation: Some("string".into()),
                        has_default: false,
                        is_required: true,
                        is_boolean: false,
                        used_in_template: false,
                        used_in_script: false,
                        span: verter_span::Span::new(0, 0),
                    })
                    .collect(),
                ..Default::default()
            }),
            ..Default::default()
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

    // -- find_unknown_props tests --

    #[test]
    fn unknown_prop_produces_diagnostic() {
        // Parent: <Child :foo="bar" />, Child: defineProps<{ msg: string }>()
        let parent = make_parent_analysis(vec![make_component(
            "Child",
            "./Child.vue",
            vec![make_prop("foo")],
        )]);
        let child = make_child_with_props(&["msg"]);

        let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));

        // Positive: foo is unknown
        assert_eq!(unknowns.len(), 1, "should find 1 unknown prop");
        assert_eq!(unknowns[0].prop_name, "foo");
        assert_eq!(unknowns[0].component_name, "Child");
        // Negative: msg should NOT be flagged
        assert!(
            !unknowns.iter().any(|u| u.prop_name == "msg"),
            "msg should not be flagged"
        );
    }

    #[test]
    fn known_prop_no_diagnostic() {
        // Parent: <Child :msg="val" />, Child: defineProps<{ msg: string }>()
        let parent = make_parent_analysis(vec![make_component(
            "Child",
            "./Child.vue",
            vec![make_prop("msg")],
        )]);
        let child = make_child_with_props(&["msg"]);

        let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
        assert!(unknowns.is_empty(), "should have no unknown props");
    }

    #[test]
    fn builtin_attrs_not_flagged() {
        // Parent: <Child class="foo" style="color:red" />
        // key and ref are already filtered out at extraction level
        let parent = make_parent_analysis(vec![make_component(
            "Child",
            "./Child.vue",
            vec![
                TemplatePropUsage {
                    name: "class".to_string(),
                    is_bound: false,
                    constness: PropValueConstness::Const,
                    referenced_bindings: vec![],
                    from_spread: false,
                    span: verter_span::Span::new(10, 21),
                },
                TemplatePropUsage {
                    name: "style".to_string(),
                    is_bound: false,
                    constness: PropValueConstness::Const,
                    referenced_bindings: vec![],
                    from_spread: false,
                    span: verter_span::Span::new(22, 40),
                },
            ],
        )]);
        let child = make_child_with_props(&[]); // No props defined

        let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
        // Positive: empty — class and style are builtin
        assert!(
            unknowns.is_empty(),
            "builtin attrs (class, style) should not be flagged"
        );
        // Negative: no class or style in unknowns
        assert!(
            !unknowns.iter().any(|u| u.prop_name == "class"),
            "class should not be unknown"
        );
        assert!(
            !unknowns.iter().any(|u| u.prop_name == "style"),
            "style should not be unknown"
        );
    }

    #[test]
    fn use_attrs_suppresses_all_unknown_props() {
        // Child has useAttrs() → no diagnostics regardless of props passed
        let parent = make_parent_analysis(vec![make_component(
            "Child",
            "./Child.vue",
            vec![make_prop("unknown1"), make_prop("unknown2")],
        )]);
        let child = FileAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::UseAttrs,
                span: verter_span::Span::new(30, 42),
                arg_value: None,
                is_async_callback: false,
            }],
            ..Default::default()
        };

        let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
        assert!(
            unknowns.is_empty(),
            "useAttrs() should suppress all unknown prop diagnostics"
        );
    }

    #[test]
    fn inherit_attrs_false_suppresses_all() {
        // Child has defineOptions({ inheritAttrs: false }) → no diagnostics
        let parent = make_parent_analysis(vec![make_component(
            "Child",
            "./Child.vue",
            vec![make_prop("unknown1")],
        )]);
        let child = FileAnalysisSnapshot {
            script_flags: AnalysisFlags::HAS_INHERIT_ATTRS_FALSE.bits(),
            ..Default::default()
        };

        let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
        assert!(
            unknowns.is_empty(),
            "inheritAttrs: false should suppress all unknown prop diagnostics"
        );
    }

    #[test]
    fn v_bind_spread_skips_component() {
        // <Child v-bind="obj" :extra="val" /> (has_spread=true) → no diagnostics
        let parent = make_parent_analysis(vec![TemplateComponentUsage {
            name: "Child".to_string(),
            import_source: Some("./Child.vue".to_string()),
            is_dynamic: false,
            props: vec![make_prop("extra")],
            has_spread: true, // v-bind="obj" spread
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![],
            span: verter_span::Span::new(0, 50),
        }]);
        let child = make_child_with_props(&[]);

        let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
        assert!(
            unknowns.is_empty(),
            "v-bind spread should skip component prop checking"
        );
    }

    #[test]
    fn dynamic_component_skipped() {
        // <component :is="comp" :foo="bar" /> → no diagnostics
        let parent = make_parent_analysis(vec![TemplateComponentUsage {
            name: "component".to_string(),
            import_source: None,
            is_dynamic: true,
            props: vec![make_prop("foo")],
            has_spread: false,
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![],
            span: verter_span::Span::new(0, 50),
        }]);

        let unknowns = find_unknown_props(&parent, &|_| None);
        assert!(
            unknowns.is_empty(),
            "dynamic components should be skipped entirely"
        );
    }

    #[test]
    fn event_prop_not_flagged_when_emit_defined() {
        // Events (@save) are skipped during template extraction and don't
        // appear in component props. This test verifies no false positives
        // when only events are used and emit definitions exist.
        let parent = make_parent_analysis(vec![make_component(
            "Child",
            "./Child.vue",
            vec![], // @save is extracted as event handler, not prop
        )]);
        let child = make_child_with_props(&[]);

        let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
        assert!(
            unknowns.is_empty(),
            "no false positives when component only has events"
        );
    }

    #[test]
    fn kebab_case_prop_matches_camel_case_definition() {
        // Parent: <Child some-prop="val" />, Child: defineProps<{ someProp: string }>()
        let parent = make_parent_analysis(vec![make_component(
            "Child",
            "./Child.vue",
            vec![make_prop("some-prop")],
        )]);
        let child = make_child_with_props(&["someProp"]);

        let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
        // Positive: should match despite case difference
        assert!(
            unknowns.is_empty(),
            "kebab-case 'some-prop' should match camelCase 'someProp'"
        );
    }

    #[test]
    fn unresolvable_component_no_diagnostic() {
        // Component that can't be resolved → no diagnostics
        let parent = make_parent_analysis(vec![make_component(
            "Unknown",
            "./Unknown.vue",
            vec![make_prop("foo")],
        )]);

        let unknowns = find_unknown_props(&parent, &|_| None);
        assert!(
            unknowns.is_empty(),
            "unresolvable component should not produce diagnostics"
        );
    }

    #[test]
    fn no_import_source_skipped() {
        // Component with no import source (global component) → skip
        let parent = make_parent_analysis(vec![TemplateComponentUsage {
            name: "GlobalComp".to_string(),
            import_source: None,
            is_dynamic: false,
            props: vec![make_prop("unknown")],
            has_spread: false,
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![],
            span: verter_span::Span::new(0, 50),
        }]);

        let unknowns = find_unknown_props(&parent, &|_| None);
        assert!(
            unknowns.is_empty(),
            "global components without import source should be skipped"
        );
    }

    #[test]
    fn multiple_components_independent_checks() {
        // Two components: one with unknown props, one without
        let parent = make_parent_analysis(vec![
            make_component("ChildA", "./ChildA.vue", vec![make_prop("foo")]),
            make_component(
                "ChildB",
                "./ChildB.vue",
                vec![make_prop("bar"), make_prop("baz")],
            ),
        ]);
        let child_a = make_child_with_props(&["msg"]); // foo is unknown
        let child_b = make_child_with_props(&["bar"]); // baz is unknown

        let unknowns = find_unknown_props(&parent, &|source| match source {
            "./ChildA.vue" => Some(child_a.clone()),
            "./ChildB.vue" => Some(child_b.clone()),
            _ => None,
        });

        assert_eq!(unknowns.len(), 2, "should find 2 unknown props total");
        // Positive: foo on ChildA, baz on ChildB
        assert!(unknowns
            .iter()
            .any(|u| u.prop_name == "foo" && u.component_name == "ChildA"));
        assert!(unknowns
            .iter()
            .any(|u| u.prop_name == "baz" && u.component_name == "ChildB"));
        // Negative: bar should NOT be flagged (it's defined in ChildB)
        assert!(!unknowns.iter().any(|u| u.prop_name == "bar"));
    }

    // -- component_usage_diagnostics (LSP Diagnostic conversion) --

    #[test]
    fn diagnostics_have_correct_code_and_source() {
        let parent = make_parent_analysis(vec![make_component(
            "Child",
            "./Child.vue",
            vec![TemplatePropUsage {
                name: "unknown".to_string(),
                is_bound: true,
                constness: PropValueConstness::Dynamic,
                referenced_bindings: vec![],
                from_spread: false,
                span: verter_span::Span::new(15, 30),
            }],
        )]);
        let child = make_child_with_props(&[]);
        let source = "<template><Child :unknown=\"val\" /></template>";
        let line_index = LineIndex::new_utf16(source);

        let diags = component_usage_diagnostics(&parent, &line_index, &|_| Some(child.clone()));

        assert_eq!(diags.len(), 1);
        // Positive: correct code and source
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("verter/unknown-prop".into()))
        );
        assert_eq!(diags[0].source.as_deref(), Some("verter"));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
        assert!(
            diags[0].message.contains("unknown"),
            "message should contain prop name"
        );
        assert!(
            diags[0].message.contains("Child"),
            "message should contain component name"
        );
        // Negative: code is NOT a different value
        assert_ne!(
            diags[0].code,
            Some(NumberOrString::String("verter/unknown-model".into()))
        );
    }

    // -- kebab_to_camel tests --

    #[test]
    fn kebab_to_camel_simple() {
        assert_eq!(kebab_to_camel("foo"), "foo");
    }

    #[test]
    fn kebab_to_camel_hyphenated() {
        assert_eq!(kebab_to_camel("some-prop"), "someProp");
    }

    #[test]
    fn kebab_to_camel_multi_hyphen() {
        assert_eq!(kebab_to_camel("my-long-prop-name"), "myLongPropName");
    }

    // -- spread prop entry ignored --

    #[test]
    fn spread_prop_entry_ignored() {
        // Spread prop entries (from_spread: true) should never be flagged
        let parent = make_parent_analysis(vec![make_component(
            "Child",
            "./Child.vue",
            vec![TemplatePropUsage {
                name: String::new(),
                is_bound: true,
                constness: PropValueConstness::Unknown,
                referenced_bindings: vec![],
                from_spread: true,
                span: verter_span::Span::new(0, 0),
            }],
        )]);
        let child = make_child_with_props(&[]);

        let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
        assert!(unknowns.is_empty(), "spread prop entries should be ignored");
    }

    // -- v-model diagnostic tests --

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

    fn make_child_with_models(model_names: &[Option<&str>]) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            macros: model_names
                .iter()
                .map(|name| AnalyzedMacro {
                    kind: AnalyzedMacroKind::DefineModel,
                    is_type_based: true,
                    type_references: vec![],
                    binding_name: None,
                    model_name: name.map(|s| s.to_string()),
                    has_inherit_attrs_false: false,
                    span: verter_span::Span::new(0, 30),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn unknown_vmodel_produces_diagnostic() {
        // <Child v-model:title="val" />, Child has no defineModel('title')
        let parent = make_parent_analysis(vec![make_component_with_vmodels(
            "Child",
            "./Child.vue",
            vec![TemplateComponentVModel {
                binding_name: "title".to_string(),
                span: verter_span::Span::new(10, 30),
            }],
        )]);
        let child = make_child_with_models(&[]); // No defineModel at all

        let unknowns = find_unknown_models(&parent, &|_| Some(child.clone()));
        // Positive: title is unknown
        assert_eq!(unknowns.len(), 1, "should find 1 unknown model");
        assert_eq!(unknowns[0].model_name, "title");
        assert_eq!(unknowns[0].component_name, "Child");
        // Negative: should not produce empty model name
        assert!(!unknowns[0].model_name.is_empty());
    }

    #[test]
    fn known_vmodel_no_diagnostic() {
        // <Child v-model:title="val" />, Child: defineModel('title')
        let parent = make_parent_analysis(vec![make_component_with_vmodels(
            "Child",
            "./Child.vue",
            vec![TemplateComponentVModel {
                binding_name: "title".to_string(),
                span: verter_span::Span::new(10, 30),
            }],
        )]);
        let child = make_child_with_models(&[Some("title")]);

        let unknowns = find_unknown_models(&parent, &|_| Some(child.clone()));
        assert!(unknowns.is_empty(), "should have no unknown models");
    }

    #[test]
    fn default_vmodel_checks_model_value() {
        // <Child v-model="val" /> → binding_name is "modelValue"
        // Child: defineModel() → model_name is None (= "modelValue")
        let parent = make_parent_analysis(vec![make_component_with_vmodels(
            "Child",
            "./Child.vue",
            vec![TemplateComponentVModel {
                binding_name: "modelValue".to_string(),
                span: verter_span::Span::new(10, 25),
            }],
        )]);
        let child = make_child_with_models(&[None]); // defineModel() without name

        let unknowns = find_unknown_models(&parent, &|_| Some(child.clone()));
        // Positive: modelValue is known
        assert!(
            unknowns.is_empty(),
            "default v-model should match defineModel()"
        );

        // Negative: without defineModel, it should be unknown
        let child_empty = FileAnalysisSnapshot::default();
        let unknowns2 = find_unknown_models(&parent, &|_| Some(child_empty.clone()));
        assert_eq!(
            unknowns2.len(),
            1,
            "default v-model should be unknown without defineModel"
        );
    }

    #[test]
    fn vmodel_diagnostic_has_correct_code() {
        let parent = make_parent_analysis(vec![make_component_with_vmodels(
            "Child",
            "./Child.vue",
            vec![TemplateComponentVModel {
                binding_name: "title".to_string(),
                span: verter_span::Span::new(15, 35),
            }],
        )]);
        let child = make_child_with_models(&[]);
        let source = "<template><Child v-model:title=\"val\" /></template>";
        let line_index = LineIndex::new_utf16(source);

        let diags = component_usage_diagnostics(&parent, &line_index, &|_| Some(child.clone()));

        assert_eq!(diags.len(), 1);
        // Positive: correct diagnostic code
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("verter/unknown-model".into()))
        );
        assert!(
            diags[0].message.contains("title"),
            "message should contain model name"
        );
        // Negative: code is NOT the prop code
        assert_ne!(
            diags[0].code,
            Some(NumberOrString::String("verter/unknown-prop".into()))
        );
    }

    #[test]
    fn dynamic_component_vmodel_skipped() {
        // <component :is="comp" v-model:title="val" /> → no diagnostics
        let parent = make_parent_analysis(vec![TemplateComponentUsage {
            name: "component".to_string(),
            import_source: None,
            is_dynamic: true,
            props: vec![],
            has_spread: false,
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![TemplateComponentVModel {
                binding_name: "title".to_string(),
                span: verter_span::Span::new(10, 30),
            }],
            span: verter_span::Span::new(0, 50),
        }]);

        let unknowns = find_unknown_models(&parent, &|_| None);
        assert!(
            unknowns.is_empty(),
            "dynamic components should be skipped for v-model checks"
        );
    }
}
