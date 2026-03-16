use super::*;
use verter_span::Span;

fn module_reference(
    analyzability: crate::ModuleReferenceAnalyzability,
    literal_specifier: Option<&str>,
    finite_specifiers: &[&str],
) -> crate::AnalyzedModuleReference {
    crate::AnalyzedModuleReference {
        syntax: crate::ModuleReferenceSyntax::DynamicImport,
        semantics: crate::ModuleReferenceSemantics::Import,
        is_type_only: false,
        span: Span::new(0, 1),
        expr_span: Span::new(0, 1),
        raw_text: String::new(),
        literal_specifier: literal_specifier.map(str::to_string),
        finite_specifiers: finite_specifiers
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        static_prefix: None,
        analyzability,
    }
}

#[test]
fn collect_resolvable_module_reference_specifiers_preserves_order_and_skips_unknown_dynamic() {
    let module_references = vec![
        module_reference(
            crate::ModuleReferenceAnalyzability::Exact,
            Some("./exact"),
            &[],
        ),
        module_reference(
            crate::ModuleReferenceAnalyzability::FiniteSet,
            None,
            &["./components/Foo.vue", "./utils", "./exact"],
        ),
        module_reference(
            crate::ModuleReferenceAnalyzability::UnknownDynamic,
            None,
            &[],
        ),
    ];

    assert_eq!(
        collect_resolvable_module_reference_specifiers(&module_references),
        vec![
            "./exact".to_string(),
            "./components/Foo.vue".to_string(),
            "./utils".to_string(),
        ]
    );
}

#[test]
fn resolve_known_module_reference_dependencies_uses_only_provided_known_ids() {
    let module_references = vec![
        module_reference(
            crate::ModuleReferenceAnalyzability::Exact,
            Some("./exact"),
            &[],
        ),
        module_reference(
            crate::ModuleReferenceAnalyzability::FiniteSet,
            None,
            &["./components/Foo.vue", "./utils", "./missing"],
        ),
        module_reference(
            crate::ModuleReferenceAnalyzability::UnknownDynamic,
            None,
            &[],
        ),
    ];
    let known_ids = vec![
        "src/App.vue".to_string(),
        "src/exact.ts".to_string(),
        "src/components/Foo.vue".to_string(),
        "src/utils/index.ts".to_string(),
    ];

    assert_eq!(
        resolve_known_module_reference_dependencies(
            "src/App.vue",
            &module_references,
            &known_ids,
            &["".to_string(), ".ts".to_string(), ".vue".to_string()],
        ),
        vec![
            "src/exact.ts".to_string(),
            "src/components/Foo.vue".to_string(),
            "src/utils/index.ts".to_string(),
        ]
    );
}

#[test]
fn resolve_known_module_reference_dependencies_respects_caller_supplied_extension_order() {
    let module_references = vec![module_reference(
        crate::ModuleReferenceAnalyzability::Exact,
        Some("./widget"),
        &[],
    )];
    let known_ids = vec!["src/widget.ts".to_string(), "src/widget.vue".to_string()];

    assert_eq!(
        resolve_known_module_reference_dependencies(
            "src/App.vue",
            &module_references,
            &known_ids,
            &[".vue".to_string(), ".ts".to_string()],
        ),
        vec!["src/widget.vue".to_string()]
    );
    assert_eq!(
        resolve_known_module_reference_dependencies(
            "src/App.vue",
            &module_references,
            &known_ids,
            &[".ts".to_string(), ".vue".to_string()],
        ),
        vec!["src/widget.ts".to_string()]
    );
}
