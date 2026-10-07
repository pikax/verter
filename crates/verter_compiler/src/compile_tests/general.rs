use super::*;

#[test]
fn vmrs_boundary_missing_runtime_semantic_bundle_fails_closed() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const props = defineProps<{ title: string }>()
</script>"#,
    );

    assert!(
        result
            .errors
            .iter()
            .any(|diagnostic| diagnostic.code == "XMissingMacroSemanticBundle"),
        "missing semantic input must be explicit: {:?}",
        result.errors
    );
    let script = result.script.expect("script output").code;
    assert!(
        !script.contains("type: String"),
        "compiler-local type inference must fail closed without a runtime bundle: {script}"
    );
}

#[test]
fn vmrs_runtime_null_union_and_production_policy_match_vue() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroAnchor, MacroRuntimeBundle, MacroRuntimeEntry, MacroRuntimeOutcome, MacroRuntimeShape,
        OrderedRuntimeConstructors, PropsDefaultsAssociation, PropsRuntimeShape,
        RuntimeConstructor, RuntimeProp, RuntimePropType,
    };

    let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
        entries: vec![MacroRuntimeEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
                defaults: PropsDefaultsAssociation::None,
                props: vec![
                    RuntimeProp {
                        name: "nullable".to_string(),
                        optional: false,
                        type_shape: RuntimePropType::Resolved {
                            constructors: OrderedRuntimeConstructors::from_ordered([
                                RuntimeConstructor::String,
                                RuntimeConstructor::Null,
                            ]),
                            skip_check: false,
                        },
                        anchor: MacroAnchor::MacroArgument { macro_index: 0 },
                    },
                    RuntimeProp {
                        name: "enabled".to_string(),
                        optional: false,
                        type_shape: RuntimePropType::Resolved {
                            constructors: OrderedRuntimeConstructors::from_ordered([
                                RuntimeConstructor::Boolean,
                            ]),
                            skip_check: false,
                        },
                        anchor: MacroAnchor::MacroArgument { macro_index: 0 },
                    },
                ],
            })),
        }],
    }));
    let source = r#"<script setup lang="ts">defineProps<{ nullable: string | null; enabled: boolean }>()</script>"#;

    let alloc = Allocator::new();
    let dev = compile(
        source,
        &CodegenOptions::default(),
        &VerterCompileOptions {
            force_js: true,
            ..Default::default()
        },
        &semantics,
        &alloc,
    );
    assert!(dev.errors.is_empty(), "{:?}", dev.errors);
    let dev = dev.script.expect("dev script").code;
    assert!(
        dev.contains("nullable: { type: [String, null], required: true }"),
        "ordered null union must retain literal null: {dev}"
    );

    let alloc = Allocator::new();
    let prod = compile(
        source,
        &CodegenOptions {
            is_production: true,
            ..Default::default()
        },
        &VerterCompileOptions {
            force_js: true,
            ..Default::default()
        },
        &semantics,
        &alloc,
    );
    assert!(prod.errors.is_empty(), "{:?}", prod.errors);
    let prod = prod.script.expect("prod script").code;
    assert!(
        prod.contains("nullable: {}"),
        "prod strips non-check types: {prod}"
    );
    assert!(
        prod.contains("enabled: { type: Boolean }"),
        "prod retains Boolean runtime casting semantics: {prod}"
    );
    assert!(
        !prod.contains("required: true"),
        "prod strips required: {prod}"
    );
}

#[test]
fn vmrs_member_degradation_warns_and_renders_null_without_conflating_unknown() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroAnchor, MacroFailure, MacroMemberReason, MacroRuntimeBundle, MacroRuntimeEntry,
        MacroRuntimeOutcome, MacroRuntimeShape, OrderedRuntimeConstructors,
        PropsDefaultsAssociation, PropsRuntimeShape, RuntimeProp, RuntimePropType,
        UnresolvedReason,
    };

    let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
        entries: vec![MacroRuntimeEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
                defaults: PropsDefaultsAssociation::None,
                props: vec![
                    RuntimeProp {
                        name: "knownUnknown".to_string(),
                        optional: true,
                        type_shape: RuntimePropType::Resolved {
                            constructors: OrderedRuntimeConstructors::default(),
                            skip_check: false,
                        },
                        anchor: MacroAnchor::MacroArgument { macro_index: 0 },
                    },
                    RuntimeProp {
                        name: "missingMember".to_string(),
                        optional: false,
                        type_shape: RuntimePropType::Degraded(MacroFailure::new(
                            MacroMemberReason::Unresolved(UnresolvedReason::MissingDependency),
                            None,
                        )),
                        anchor: MacroAnchor::MacroArgument { macro_index: 0 },
                    },
                ],
            })),
        }],
    }));
    let alloc = Allocator::new();
    let result = compile(
        r#"<script setup lang="ts">defineProps<{ knownUnknown?: unknown; missingMember: Missing }>()</script>"#,
        &CodegenOptions::default(),
        &VerterCompileOptions {
            force_js: true,
            ..Default::default()
        },
        &semantics,
        &alloc,
    );

    let warnings = result
        .errors
        .iter()
        .filter(|diagnostic| diagnostic.code == "XUnresolvedImportedMacroType")
        .collect::<Vec<_>>();
    assert_eq!(
        warnings.len(),
        1,
        "only degraded rows warn: {:?}",
        result.errors
    );
    assert!(
        warnings[0].message.contains("missingMember"),
        "warning must retain typed row identity: {:?}",
        warnings[0]
    );
    let code = result.script.expect("script").code;
    assert!(
        code.contains("knownUnknown: { type: null, required: false }"),
        "{code}"
    );
    assert!(
        code.contains("missingMember: { type: null, required: true }"),
        "{code}"
    );
}

#[test]
fn vmrs_dynamic_with_defaults_performs_one_syntax_owned_merge() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroAnchor, MacroRuntimeBundle, MacroRuntimeEntry, MacroRuntimeOutcome, MacroRuntimeShape,
        OrderedRuntimeConstructors, PropsDefaultsAssociation, PropsRuntimeShape,
        RuntimeConstructor, RuntimeProp, RuntimePropType,
    };

    let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
        entries: vec![MacroRuntimeEntry {
            syntax_index: 0,
            macro_index: 1,
            outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
                defaults: PropsDefaultsAssociation::WithDefaults {
                    payload_macro_index: 0,
                    defaults_macro_index: 1,
                },
                props: vec![RuntimeProp {
                    name: "label".to_string(),
                    optional: true,
                    type_shape: RuntimePropType::Resolved {
                        constructors: OrderedRuntimeConstructors::from_ordered([
                            RuntimeConstructor::String,
                        ]),
                        skip_check: false,
                    },
                    anchor: MacroAnchor::MacroArgument { macro_index: 0 },
                }],
            })),
        }],
    }));
    let alloc = Allocator::new();
    let result = compile(
        r#"<script setup lang="ts">
import { defaults } from './defaults'
withDefaults(defineProps<{ label?: string }>(), defaults)
</script>"#,
        &CodegenOptions::default(),
        &VerterCompileOptions {
            force_js: true,
            ..Default::default()
        },
        &semantics,
        &alloc,
    );

    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let code = result.script.expect("script").code;
    assert_eq!(code.matches("_mergeDefaults(").count(), 1, "{code}");
    assert!(code.contains(", defaults)"), "{code}");
}

#[test]
fn vmrs_runtime_failures_preserve_typed_reason_detail_and_absolute_anchor() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroFailure, MacroInvalidReason, MacroPartialReason, MacroRuntimeBundle,
        MacroRuntimeEntry, MacroRuntimeOutcome, UnresolvedReason, UnsupportedReason,
    };

    let source = r#"<script setup lang="ts">
defineProps<{ value: string }>()
</script>"#;
    let type_text = "{ value: string }";
    let type_start = source.find(type_text).expect("type argument") as u32;
    let outcomes = [
        (
            MacroRuntimeOutcome::Partial(MacroFailure::new(
                MacroPartialReason::BudgetExceeded,
                Some("projection work budget exhausted".to_owned()),
            )),
            "XUnavailableMacroSemanticResult",
            "partial",
            "budget-exceeded",
            "projection work budget exhausted",
        ),
        (
            MacroRuntimeOutcome::Unresolved(MacroFailure::new(
                UnresolvedReason::MissingDependency,
                Some("dependency was unavailable".to_owned()),
            )),
            "XUnavailableMacroSemanticResult",
            "unresolved",
            "missing-dependency",
            "dependency was unavailable",
        ),
        (
            MacroRuntimeOutcome::Unsupported(MacroFailure::new(
                UnsupportedReason::SemanticConstruct,
                Some("construct is outside the runtime projection".to_owned()),
            )),
            "XUnavailableMacroSemanticResult",
            "unsupported",
            "semantic-construct",
            "construct is outside the runtime projection",
        ),
        (
            MacroRuntimeOutcome::Invalid(MacroFailure::new(
                MacroInvalidReason::NonObjectRoot,
                Some("resolved root is not object-like".to_owned()),
            )),
            "XInvalidMacroType",
            "invalid",
            "non-object-root",
            "resolved root is not object-like",
        ),
    ];
    for (outcome, code, kind, reason, detail) in outcomes {
        let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
            entries: vec![MacroRuntimeEntry {
                syntax_index: 0,
                macro_index: 0,
                outcome,
            }],
        }));
        let result = compile(
            source,
            &CodegenOptions::default(),
            &VerterCompileOptions {
                force_js: true,
                ..Default::default()
            },
            &semantics,
            &Allocator::new(),
        );
        let diagnostic = result
            .errors
            .iter()
            .find(|diagnostic| diagnostic.code == code)
            .expect("typed unavailable diagnostic");
        assert!(diagnostic.message.contains(kind), "{diagnostic:?}");
        assert!(diagnostic.message.contains(reason), "{diagnostic:?}");
        assert!(diagnostic.message.contains(detail), "{diagnostic:?}");
        assert_eq!(
            diagnostic.span,
            Some(crate::common::Span::new(
                type_start,
                type_start + type_text.len() as u32
            )),
            "runtime diagnostics use SFC-absolute parser geometry"
        );
        assert!(
            !result
                .script
                .expect("script")
                .code
                .contains("value: { type:"),
            "unavailable roots fail closed"
        );
    }
}

#[test]
fn vmrs_degraded_member_uses_honest_authored_anchor_and_exact_payload() {
    use std::sync::Arc;
    use verter_macro_dto::{
        AuthoredMemberOrdinal, MacroAnchor, MacroFailure, MacroMemberReason, MacroRuntimeBundle,
        MacroRuntimeEntry, MacroRuntimeOutcome, MacroRuntimeShape, OrderedRuntimeConstructors,
        PropsDefaultsAssociation, PropsRuntimeShape, RuntimeConstructor, RuntimeProp,
        RuntimePropType, UnresolvedReason,
    };

    let source = r#"<script setup lang="ts">
defineProps<{ first: string; second: Missing }>()
</script>"#;
    let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
        entries: vec![MacroRuntimeEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
                defaults: PropsDefaultsAssociation::None,
                props: vec![
                    RuntimeProp {
                        name: "first".to_owned(),
                        optional: false,
                        type_shape: RuntimePropType::Resolved {
                            constructors: OrderedRuntimeConstructors::from_ordered([
                                RuntimeConstructor::String,
                            ]),
                            skip_check: false,
                        },
                        anchor: MacroAnchor::Authored {
                            macro_index: 0,
                            member_ordinal: AuthoredMemberOrdinal::new(0),
                        },
                    },
                    RuntimeProp {
                        name: "second".to_owned(),
                        optional: false,
                        type_shape: RuntimePropType::Degraded(MacroFailure::new(
                            MacroMemberReason::Unresolved(UnresolvedReason::MissingDependency),
                            Some("dependency ./missing.ts was unavailable".to_owned()),
                        )),
                        anchor: MacroAnchor::Authored {
                            macro_index: 0,
                            member_ordinal: AuthoredMemberOrdinal::new(1),
                        },
                    },
                ],
            })),
        }],
    }));
    let result = compile(
        source,
        &CodegenOptions::default(),
        &VerterCompileOptions {
            force_js: true,
            ..Default::default()
        },
        &semantics,
        &Allocator::new(),
    );
    let diagnostic = result
        .errors
        .iter()
        .find(|diagnostic| diagnostic.code == "XUnresolvedImportedMacroType")
        .expect("degraded member warning");
    assert!(diagnostic.message.contains("unresolved"), "{diagnostic:?}");
    assert!(
        diagnostic.message.contains("missing-dependency"),
        "{diagnostic:?}"
    );
    assert!(
        diagnostic
            .message
            .contains("dependency ./missing.ts was unavailable"),
        "{diagnostic:?}"
    );
    let second_start = source.find("second").expect("second key") as u32;
    assert_eq!(
        diagnostic.span,
        Some(crate::common::Span::new(second_start, second_start + 6))
    );
}

#[test]
fn vmrs_duplicate_entries_and_invalid_anchors_fail_closed() {
    use std::sync::Arc;
    use verter_macro_dto::{
        AuthoredMemberOrdinal, MacroAnchor, MacroRuntimeBundle, MacroRuntimeEntry,
        MacroRuntimeOutcome, MacroRuntimeShape, OrderedRuntimeConstructors,
        PropsDefaultsAssociation, PropsRuntimeShape, RuntimeConstructor, RuntimeProp,
        RuntimePropType,
    };

    let entry = MacroRuntimeEntry {
        syntax_index: 0,
        macro_index: 0,
        outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
            defaults: PropsDefaultsAssociation::None,
            props: vec![RuntimeProp {
                name: "value".to_owned(),
                optional: false,
                type_shape: RuntimePropType::Resolved {
                    constructors: OrderedRuntimeConstructors::from_ordered([
                        RuntimeConstructor::String,
                    ]),
                    skip_check: false,
                },
                anchor: MacroAnchor::Authored {
                    macro_index: 0,
                    member_ordinal: AuthoredMemberOrdinal::new(9),
                },
            }],
        })),
    };
    for entries in [vec![entry.clone()], vec![entry.clone(), entry.clone()]] {
        let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle { entries }));
        let result = compile(
            r#"<script setup lang="ts">defineProps<{ value: string }>()</script>"#,
            &CodegenOptions::default(),
            &VerterCompileOptions {
                force_js: true,
                ..Default::default()
            },
            &semantics,
            &Allocator::new(),
        );
        assert!(
            result.errors.iter().any(|diagnostic| {
                diagnostic.code == "XUnavailableMacroSemanticResult"
                    && (diagnostic
                        .message
                        .contains("invalid-authored-member-ordinal")
                        || diagnostic.message.contains("duplicate-entry"))
            }),
            "invalid authoritative bundle must be explicit: {:?}",
            result.errors
        );
        assert!(
            !result
                .script
                .expect("script")
                .code
                .contains("value: { type: String"),
            "invalid bundle must not drive codegen"
        );
    }
}

#[test]
fn vmrs_escaped_public_names_are_decoded_quoted_and_token_stable() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroAnchor, MacroRuntimeBundle, MacroRuntimeEntry, MacroRuntimeOutcome, MacroRuntimeShape,
        ModelRuntimeShape, OrderedRuntimeConstructors, RuntimeConstructor, RuntimeEmit,
        RuntimeProp, RuntimePropType, SynthesizedRowKind,
    };

    let emitted_name = "line\n\"\\event";
    let model_name = "model\n\"\\name";
    let update_name = format!("update:{model_name}");
    let modifiers_name = format!("{model_name}Modifiers");
    let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
        entries: vec![
            MacroRuntimeEntry {
                syntax_index: 0,
                macro_index: 0,
                outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Emits(vec![
                    RuntimeEmit {
                        name: emitted_name.to_owned(),
                        anchor: MacroAnchor::MacroArgument { macro_index: 0 },
                    },
                ])),
            },
            MacroRuntimeEntry {
                syntax_index: 1,
                macro_index: 1,
                outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Model(
                    ModelRuntimeShape {
                        prop: RuntimeProp {
                            name: model_name.to_owned(),
                            optional: true,
                            type_shape: RuntimePropType::Resolved {
                                constructors: OrderedRuntimeConstructors::from_ordered([
                                    RuntimeConstructor::String,
                                ]),
                                skip_check: false,
                            },
                            anchor: MacroAnchor::Synthesized {
                                macro_index: 1,
                                row: SynthesizedRowKind::ModelProp,
                            },
                        },
                        update_event: RuntimeEmit {
                            name: update_name.clone(),
                            anchor: MacroAnchor::Synthesized {
                                macro_index: 1,
                                row: SynthesizedRowKind::ModelUpdateEvent,
                            },
                        },
                        modifiers_prop: RuntimeProp {
                            name: modifiers_name.clone(),
                            optional: true,
                            type_shape: RuntimePropType::Resolved {
                                constructors: OrderedRuntimeConstructors::default(),
                                skip_check: false,
                            },
                            anchor: MacroAnchor::Synthesized {
                                macro_index: 1,
                                row: SynthesizedRowKind::ModelModifiersProp,
                            },
                        },
                    },
                )),
            },
        ],
    }));
    let source = r#"<script setup lang="ts">
defineEmits<{ (event: 'line\n"\\event'): void }>()
defineModel<string>('model\n"\\name')
</script>"#;
    let result = compile(
        source,
        &CodegenOptions::default(),
        &VerterCompileOptions {
            force_js: true,
            ..Default::default()
        },
        &semantics,
        &Allocator::new(),
    );
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let code = result.script.expect("script").code;
    let alloc = Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &code, oxc_span::SourceType::mjs()).parse();
    assert!(
        !parsed.fatal_error && parsed.diagnostics.is_empty(),
        "escaped public names must produce valid JavaScript: {:?}\n{code}",
        parsed.diagnostics
    );

    use oxc_ast::ast::StringLiteral;
    use oxc_ast_visit::Visit;
    #[derive(Default)]
    struct StringValues(Vec<String>);
    impl<'a> Visit<'a> for StringValues {
        fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
            self.0.push(literal.value.to_string());
        }
    }
    let mut values = StringValues::default();
    values.visit_program(&parsed.program);
    for expected in [
        emitted_name,
        model_name,
        update_name.as_str(),
        modifiers_name.as_str(),
    ] {
        assert!(
            values.0.iter().any(|value| value == expected),
            "decoded public string {expected:?} must survive token normalization: {:?}\n{code}",
            values.0
        );
    }
}

#[test]
fn vmrs_tsc_join_failure_is_reported_as_an_explicit_compiler_diagnostic() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroTscBundle, MacroTscEntry, MacroTscOutcome, MacroTscProjection, TscPropsProjection,
        TscPublicPropsProjection, TscScopeRequirements,
    };

    let entry = MacroTscEntry {
        syntax_index: 0,
        macro_index: 0,
        outcome: MacroTscOutcome::Complete(MacroTscProjection::Props(TscPropsProjection {
            public: TscPublicPropsProjection::AuthoredArgument {
                anchor: verter_macro_dto::MacroAnchor::MacroArgument { macro_index: 0 },
            },
            testing_rows: Vec::new(),
            scope: TscScopeRequirements::default(),
        })),
    };
    let semantics = VueMacroSemanticInput::Tsc(Arc::new(MacroTscBundle {
        entries: vec![entry.clone(), entry],
    }));
    let result = compile(
        r#"<script setup lang="ts">defineProps<{ value: string }>()</script>"#,
        &CodegenOptions {
            target: CompileTarget::TSC,
            ..Default::default()
        },
        &VerterCompileOptions::default(),
        &semantics,
        &Allocator::new(),
    );

    assert!(result.tsc.is_none());
    assert!(
        result.errors.iter().any(|diagnostic| {
            diagnostic.code == "XUnavailableMacroSemanticResult"
                && diagnostic.message.contains("duplicate")
        }),
        "typed TSC join failure must not be silently dropped: {:?}",
        result.errors
    );
}

#[test]
fn vmrs_tsc_unavailable_diagnostics_preserve_exact_outcome_reason_and_detail() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroFailure, MacroInvalidReason, MacroPartialReason, MacroTscBundle, MacroTscEntry,
        MacroTscOutcome, UnresolvedReason, UnsupportedReason,
    };

    let cases = [
        (
            MacroTscOutcome::Partial(MacroFailure::new(
                MacroPartialReason::Recursion,
                Some("partial detail".to_owned()),
            )),
            "partial",
            "recursion",
            "partial detail",
        ),
        (
            MacroTscOutcome::Unresolved(MacroFailure::new(
                UnresolvedReason::AmbiguousReference,
                Some("unresolved detail".to_owned()),
            )),
            "unresolved",
            "ambiguous-reference",
            "unresolved detail",
        ),
        (
            MacroTscOutcome::Unsupported(MacroFailure::new(
                UnsupportedReason::SemanticConstruct,
                Some("unsupported detail".to_owned()),
            )),
            "unsupported",
            "semantic-construct",
            "unsupported detail",
        ),
        (
            MacroTscOutcome::Invalid(MacroFailure::new(
                MacroInvalidReason::NonObjectRoot,
                Some("invalid detail".to_owned()),
            )),
            "invalid",
            "non-object-root",
            "invalid detail",
        ),
    ];

    for (outcome, kind, reason, detail) in cases {
        let semantics = VueMacroSemanticInput::Tsc(Arc::new(MacroTscBundle {
            entries: vec![MacroTscEntry {
                syntax_index: 0,
                macro_index: 0,
                outcome,
            }],
        }));
        let result = compile(
            r#"<script setup lang="ts">defineProps<{ value: string }>()</script>"#,
            &CodegenOptions {
                target: CompileTarget::TSC,
                ..Default::default()
            },
            &VerterCompileOptions::default(),
            &semantics,
            &Allocator::new(),
        );
        assert!(result.tsc.is_none());
        assert!(
            result.errors.iter().any(|diagnostic| {
                diagnostic.message.contains(kind)
                    && diagnostic.message.contains(reason)
                    && diagnostic.message.contains(detail)
            }),
            "kind={kind}, diagnostics={:?}",
            result.errors
        );
    }
}

#[test]
pub(super) fn basic_sfc_compiles() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.script.is_some());
    assert!(result.template.is_some());
}

#[test]
pub(super) fn custom_blocks_extracted() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>

<i18n lang="json">
{ "en": { "hello": "Hello" } }
</i18n>
"#;
    let result = compile_sfc(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.custom_blocks.len(), 1);
    let block = &result.custom_blocks[0];
    assert_eq!(block.block_type, "i18n");
    assert_eq!(block.source_order, 0);
    assert_eq!(block.lang, Some("json".to_string()));
    assert_eq!(block.src, None);
    let region = block.region;
    assert_eq!(
        &block.content[..],
        "\n{ \"en\": { \"hello\": \"Hello\" } }\n"
    );
    assert_eq!(
        &source[region.start as usize..region.end as usize],
        block.content,
        "a locally-authored block's region must span exactly its content bytes"
    );
    assert_eq!(
        block.source_content,
        verter_identity::identity::ContentId::from_content_bytes(source.as_bytes()),
        "the facts carry the digest of the bytes they were parsed from"
    );
}

#[test]
fn custom_blocks_retain_document_order_and_src_facts() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<i18n lang="json">{}</i18n>
<docs src="./docs.md"></docs>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.custom_blocks.len(), 2);
    let i18n = &result.custom_blocks[0];
    let docs = &result.custom_blocks[1];
    assert_eq!(i18n.source_order, 0);
    assert_eq!(docs.source_order, 1);
    assert_eq!(docs.lang, None);
    assert_eq!(docs.src, Some("./docs.md".to_string()));
    assert!(
        i18n.region.end <= docs.region.start,
        "document-order blocks must not carry overlapping or reordered regions"
    );
}

#[test]
fn empty_input_no_panic() {
    let result = compile_sfc("");
    // No template — but no panic, and an empty SFC emits the synthetic
    // empty-component shell (`empty_sfc_compiles_to_empty_component_shell`
    // pins its shape).
    assert!(result.script.is_some());
    assert!(result.template.is_none());
    assert!(result.errors.is_empty());
}

#[test]
fn timing_fields_populated() {
    let result = compile_sfc(
        r#"<script setup>
const x = 1
</script>
<template><div>{{ x }}</div></template>
"#,
    );
    assert!(result.parse_duration_ms >= 0.0);
    assert!(result.total_duration_ms >= 0.0);
    if let Some(ref s) = result.script {
        assert!(s.duration_ms >= 0.0);
    }
    if let Some(ref t) = result.template {
        assert!(t.duration_ms >= 0.0);
    }
}

/// @ai-generated - Regression test using the actual AnalysisPanel.vue template
/// structure that triggered the playground build failure. This complex template
/// produces enough prepends to trigger sort_unstable_by_key reordering.
#[test]
pub(super) fn analysis_panel_regression_valid_js() {
    let source = include_str!("../../../../packages/playground/src/output/AnalysisPanel.vue");
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "store",
            false,
            [verter_macro_dto::RuntimeConstructor::Object],
        )],
    )]);
    let code = compile_and_validate_template_with_runtime(source, runtime);
    assert!(
        !code.contains(",  : _createCommentVNode"),
        "comma should not appear before ternary colon\n{}",
        code
    );
}

// ==================== Text with entities and newlines ====================

#[test]
fn pre_code_with_entities_no_unterminated_string() {
    // Text inside <pre><code> with HTML entities and newlines should produce
    // valid JS. The newlines must be escaped in string literals.
    let code = compile_and_validate_template(
        "<template><pre><code>\n&lt;html dir=\"rtl\"&gt;\n</code></pre></template>",
    );
    assert!(code.contains("function render("));
    // Should not contain raw unescaped newlines inside string literals
    assert!(
        !code.contains("\"\n"),
        "Should not have raw newline after opening quote\n{}",
        code
    );
}

#[test]
fn custom_block_with_html_like_content_no_errors() {
    // A <docs> block containing `Array<string>` should not cause parse
    // errors — the tokenizer enters RCDATA mode for custom SFC blocks.
    let result = compile_sfc(
        r#"<docs>
## Title

Default to `@`, `Array<string>` also supported.

</docs>
<template><div>hello</div></template>
<script setup>
const x = 1
</script>"#,
    );
    assert!(
        result.errors.is_empty(),
        "SFC with <docs> block should not have errors: {:?}",
        result.errors
    );
    assert_eq!(result.custom_blocks.len(), 1);
    assert_eq!(result.custom_blocks[0].block_type, "docs");
    assert!(
        result.custom_blocks[0].content.contains("Array<string>"),
        "Custom block content should be raw text"
    );
}

/// The empty component shell is also valid TSX input for the IDE lane.
#[test]
fn empty_sfc_tsx_output_is_valid() {
    assert_tsx_parses("", "empty SFC");
    assert_tsx_parses("<!-- comment only -->", "comment-only SFC");
}

/// Official Vue rejects a carrier with no `<template>`/`<script>`/`<script setup>`
/// (`parse.spec.ts` #6676). Trivia-only shell exemption
/// ([`empty_sfc_compiles_to_empty_component_shell`]) does not cover style/
/// custom-block-only or arbitrary-text carriers — those still diagnose
/// `MissingSfcEntryBlock`.
#[test]
fn missing_sfc_entry_block_for_non_trivia_block_less_carriers() {
    for src in [
        "import { ref } from 'vue'",
        "<style>.css { color: red; }</style>",
        "<i18n>{ \"en\": { \"hello\": \"Hello\" } }</i18n>",
    ] {
        let result = compile_sfc(src);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.code == "MissingSfcEntryBlock"),
            "{src:?} must diagnose MissingSfcEntryBlock, got: {:?}",
            result.errors
        );
    }
}

// @ai-generated - TDD test: literal boolean/null in v-bind should not get _ctx. prefix
#[test]
pub(super) fn literal_boolean_in_bind_no_ctx_prefix() {
    let code = compile_and_validate_template(
        r#"<template><div><Comp :show="false" :active="true" /></div></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    assert!(
        !code.contains("_ctx.false"),
        "literal false should NOT get _ctx. prefix, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ctx.true"),
        "literal true should NOT get _ctx. prefix, got:\n{}",
        code
    );
    assert!(
        code.contains("show: false") && code.contains("active: true"),
        "literal booleans should appear as-is in props, got:\n{}",
        code
    );
}

// @ai-generated - TDD test: HTML entities in v-bind expressions should be decoded
#[test]
pub(super) fn html_entities_in_bind_value_decoded() {
    let code = compile_and_validate_template(
        r#"<template><div :data="{&quot;key&quot;:&quot;value&quot;}"></div></template>"#,
    );
    assert!(
        !code.contains("&quot;"),
        "HTML entities should be decoded in v-bind expressions, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"{"key":"value"}"#),
        "decoded expression should contain normal quotes, got:\n{}",
        code
    );
}

#[test]
pub(super) fn with_defaults_type_reference() {
    // withDefaults(defineProps<Props>(), { color: 'primary' })
    // where Props is a type alias — type resolution should still work
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "color",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "size",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
type Props = {
  color?: string
  size?: string
}

const props = withDefaults(defineProps<Props>(), {
  color: 'primary',
})
</script>

<template><div>{{ props.color }}</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    // Should have props section with default
    assert!(
        script.code.contains("default: 'primary'"),
        "should merge color default.\nOutput:\n{}",
        script.code
    );
    // Should have props section at all
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
}

/// @ai-generated — withDefaults + unresolvable type without any defaults
/// should emit empty props `{}`
/// A complete semantic result with function-call defaults compiles without a
/// macro-boundary diagnostic.
#[test]
fn with_defaults_authoritative_type_with_function_defaults_has_no_error() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [crate::test_helpers::runtime_prop_at_macro_argument(
            "foo",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
import { getDefaults } from './defaults'

interface Props { foo?: string }

const props = withDefaults(defineProps<Props>(), getDefaults())
</script>
<template><div>{{ props.foo }}</div></template>"#,
        runtime,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let script = result.script.as_ref().expect("script block");
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("getDefaults()"),
        "should reference the defaults function.\nOutput:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("defineProps<Props>()"),
        "defineProps call should be lowered.\nOutput:\n{}",
        script.code
    );
}

/// @ai-generated â€” withDefaults + unresolvable type without any defaults
/// should emit empty props `{}`
#[test]
pub(super) fn with_defaults_unresolvable_type_no_defaults() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    // Should still have props: {} for unresolvable type without defaults
    // (or may omit props section entirely — both are acceptable)
    // But should NOT crash or produce invalid JS
}

/// @ai-generated — withDefaults + resolvable inline type + object defaults
/// should NOT use the IIFE pattern — should resolve types normally
#[test]
pub(super) fn with_defaults_resolvable_type_still_works() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop(
                "color",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop(
                "size",
                true,
                [verter_macro_dto::RuntimeConstructor::Number],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
const props = withDefaults(defineProps<{
  color?: string
  size?: number
}>(), {
  color: 'red',
  size: 42,
})
</script>
<template><div>{{ props.color }}</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("color:"),
        "should have color prop.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default:"),
        "should have defaults.\nOutput:\n{}",
        script.code
    );
    // Should NOT use the IIFE pattern for resolvable types
    assert!(
        !script.code.contains("for(const k in d)"),
        "should NOT use IIFE pattern for resolvable types.\nOutput:\n{}",
        script.code
    );
}

#[test]
fn dynamic_class_with_array_uses_normalize_class() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const cls = ref({})
</script>
<template>
  <div :class="['foo', cls]">hello</div>
</template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_normalizeClass"),
        "Dynamic :class should use _normalizeClass().\nOutput:\n{}",
        tpl.code
    );
}

#[test]
pub(super) fn ts_return_type_annotation_in_computed() {
    // Regression test: vue-vben-admin app.vue panics on type annotation in computed()
    let result = compile_sfc(
        r#"<script lang="ts" setup>
import type { GlobalThemeOverrides } from 'naive-ui';
import { computed } from 'vue';

defineOptions({ name: 'App' });

const themeOverrides = computed((): GlobalThemeOverrides => {
  return {
common: {},
  };
});
</script>

<template>
  <div>{{ themeOverrides }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.script.is_some());
}

#[test]
pub(super) fn ts_return_type_no_strip_mode() {
    // Test compilation with force_js=false (host mode) to match NAPI behavior
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("app.vue".to_string()),
        inline: Some(false),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: false,
        source_map: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script lang="ts" setup>
import type { GlobalThemeOverrides } from 'naive-ui';
import { computed } from 'vue';

import {
  darkTheme,
  dateEnUS,
  dateZhCN,
  enUS,
  lightTheme,
  NConfigProvider,
  NMessageProvider,
  NNotificationProvider,
  zhCN,
} from 'naive-ui';

defineOptions({ name: 'App' });

const { commonTokens } = useNaiveDesignTokens();

const tokenLocale = computed(() =>
  preferences.app.locale === 'zh-CN' ? zhCN : enUS,
);
const tokenDateLocale = computed(() =>
  preferences.app.locale === 'zh-CN' ? dateZhCN : dateEnUS,
);
const tokenTheme = computed(() =>
  preferences.theme.mode === 'dark' ? darkTheme : lightTheme,
);

const themeOverrides = computed((): GlobalThemeOverrides => {
  return {
common: commonTokens,
  };
});
</script>

<template>
  <NConfigProvider
:date-locale="tokenDateLocale"
:locale="tokenLocale"
:theme="tokenTheme"
:theme-overrides="themeOverrides"
class="h-full"
  >
<NNotificationProvider>
  <NMessageProvider>
    <RouterView />
  </NMessageProvider>
</NNotificationProvider>
  </NConfigProvider>
</template>
"#,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.script.is_some());
}

// @ai-generated - TDD test: data-* and aria-* attributes should NOT be camelized in dynamic binds
#[test]
pub(super) fn data_and_aria_attributes_not_camelized() {
    let code = compile_and_validate_template(
        r#"<template><div :data-orientation="orientation" :aria-expanded="expanded" :data-state="state"></div></template>
<script setup>
const orientation = "vertical";
const expanded = true;
const state = "open";
</script>"#,
    );
    assert!(
        code.contains("\"data-orientation\""),
        "data-* bind should preserve hyphenated name with quotes, got:\n{}",
        code
    );
    assert!(
        code.contains("\"aria-expanded\""),
        "aria-* bind should preserve hyphenated name with quotes, got:\n{}",
        code
    );
    assert!(
        code.contains("\"data-state\""),
        "data-state bind should preserve hyphenated name with quotes, got:\n{}",
        code
    );
    assert!(
        !code.contains("dataOrientation")
            && !code.contains("ariaExpanded")
            && !code.contains("dataState"),
        "data-*/aria-* should NOT be camelized, got:\n{}",
        code
    );
}

/// @ai-generated — withDefaults + cross-block type reference from companion <script>
/// should produce correct prop names (not garbled from wrong source offsets).
/// Regression: macros.rs used span extraction into SFC source for external types
/// where key spans reference the companion block, producing corrupted prop names.
#[test]
pub(super) fn with_defaults_cross_block_type_uses_key_name() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "title",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "description",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "color",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script lang="ts">
export interface ExternalProps {
  title?: string
  description?: string
  color?: string
}
</script>
<script setup lang="ts">
const props = withDefaults(defineProps<ExternalProps>(), {
  color: 'primary'
})
</script>
<template><div>{{ props.title }}</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    // The withDefaults path must use key_name (pre-resolved) for cross-block types,
    // not span extraction which indexes into the wrong source region.
    assert!(
        script.code.contains("title:"),
        "title prop should appear with correct name in withDefaults output, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("description:"),
        "description prop should appear with correct name in withDefaults output, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("color:") && script.code.contains("default: 'primary'"),
        "color prop should have default: 'primary' in withDefaults output, got:\n{}",
        script.code
    );
}

// ======================== export type stripping (force_js) ========================

/// @ai-generated - export type inside script setup must be stripped when force_js: true
#[test]
pub(super) fn export_type_stripped_when_force_js() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop_at_macro_argument(
            "visible",
            true,
            [verter_macro_dto::RuntimeConstructor::Boolean],
        )],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
import { computed } from 'vue'

export type NavigatePayload =
  | { type: 'notification'; to: string }
  | { type: 'menu-item'; to: string }

interface SideMenuProps {
  visible?: boolean
}

const props = defineProps<SideMenuProps>()
const isOpen = computed(() => props.visible)
</script>

<template><div>{{ isOpen }}</div></template>"#,
        runtime,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");
    // export type must be completely removed in force_js mode
    assert!(
        !script.code.contains("export type NavigatePayload"),
        "export type should be stripped when force_js: true, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("NavigatePayload"),
        "NavigatePayload should not appear at all in JS output, got:\n{}",
        script.code
    );
}

/// @ai-generated - export interface inside script setup must be stripped when force_js: true
#[test]
pub(super) fn export_interface_stripped_when_force_js() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "title",
                false,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "count",
                false,
                [verter_macro_dto::RuntimeConstructor::Number],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
export interface FooProps {
  title: string
  count: number
}

const props = defineProps<FooProps>()
</script>

<template><div>{{ props.title }}</div></template>"#,
        runtime,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");
    assert!(
        !script.code.contains("export interface FooProps"),
        "export interface should be stripped when force_js: true, got:\n{}",
        script.code
    );
}

/// @ai-generated - bare type and interface (no export) stripped when force_js: true
#[test]
pub(super) fn bare_type_and_interface_stripped_when_force_js() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
type LocalType = { a: string }

interface LocalInterface {
  b: number
}

const x = 1
</script>

<template><div>{{ x }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");
    assert!(
        !script.code.contains("type LocalType"),
        "type alias should be stripped when force_js: true, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("interface LocalInterface"),
        "interface should be stripped when force_js: true, got:\n{}",
        script.code
    );
}

/// @ai-generated - export type is hoisted outside setup wrapper when keeping TS types
#[test]
pub(super) fn export_type_hoisted_when_keep_ts() {
    let result = compile_sfc_keep_ts(
        r#"<script setup lang="ts">
import { computed } from 'vue'

export type NavigatePayload = { type: string; to: string }

const x = computed(() => 1)
</script>

<template><div>{{ x }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");
    // export type should be hoisted BEFORE the setup wrapper (before const __sfc__)
    let type_pos = script
        .code
        .find("export type NavigatePayload")
        .expect("export type should be present when keeping TS");
    let wrapper_pos = script
        .code
        .find("const __sfc__")
        .expect("const __sfc__ should be present");
    assert!(
        type_pos < wrapper_pos,
        "export type should be hoisted before const __sfc__.\ntype_pos={}, wrapper_pos={}\ncode:\n{}",
        type_pos,
        wrapper_pos,
        script.code
    );
    // Must NOT appear inside setup() body
    let setup_start = script.code.find("setup(").expect("setup function");
    assert!(
        type_pos < setup_start,
        "export type should be outside setup function, got:\n{}",
        script.code
    );
}

#[test]
fn title_attr_with_newline_produces_valid_js() {
    // Static attributes with newlines must be properly escaped
    let code =
        compile_and_validate_template("<template><div title=\"line1\nline2\"></div></template>");
    // The output must parse as valid JS (compile_and_validate_template checks this)
    assert!(!code.is_empty());
}

/// @ai-generated — Different option modifiers produce DIFFERENT keys, not merged
#[test]
pub(super) fn different_option_modifiers_produce_different_keys() {
    let result = compile_sfc(
        r#"<template><div @click="a" @click.capture="b"></div></template>
<script setup>const a = () => {}; const b = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onClick") && tpl.code.contains("onClickCapture"),
        "@click and @click.capture should produce two different keys, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Mouse left/right on non-keyboard event → runtime modifiers, same key
#[test]
pub(super) fn mouse_left_right_as_runtime_modifiers_merged() {
    let result = compile_sfc(
        r#"<template><div @click.left="a" @click.right="b"></div></template>
<script setup>const a = () => {}; const b = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onClick: ["),
        "@click.left and @click.right should be merged into array, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onClick:").count(),
        1,
        "@click.left and @click.right should produce one onClick: key, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Handler with both key and runtime modifiers sharing same key
#[test]
pub(super) fn handler_with_mixed_key_and_runtime_modifiers_merged() {
    let result = compile_sfc(
        r#"<template><div @keydown.enter.prevent="a" @keydown.enter.stop="b"></div></template>
<script setup>const a = () => {}; const b = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onKeydown: ["),
        "handlers with mixed modifiers should be merged into array, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onKeydown:").count(),
        1,
        "handlers with mixed modifiers sharing same key should produce one key, got:\n{}",
        tpl.code
    );
}

// ==================== HTML entity decoding ====================

/// @ai-generated - HTML named entity &copy; must be decoded to © in render output.
#[test]
pub(super) fn html_entity_copy_decoded() {
    let code = compile_and_validate_template(
        r#"<template>
  <p>&copy; 2026</p>
</template>"#,
    );
    eprintln!("=== HTML ENTITY OUTPUT ===\n{}", code);
    assert!(
        code.contains("\u{00A9}") || code.contains("©"),
        "&copy; must be decoded to © character. Got:\n{}",
        code
    );
    assert!(
        !code.contains("&copy;"),
        "&copy; must NOT appear as literal string in JS output. Got:\n{}",
        code
    );
}

#[test]
pub(super) fn tsx_basic_sfc() {
    let result = compile_tsx(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(!tsx.code.is_empty(), "TSX code should not be empty");
    assert!(
        tsx.code
            .contains("export function ___VERTER___TemplateBindingFN"),
        "Should contain component wrapper function, got: {}",
        tsx.code
    );
    assert!(
        tsx.code.contains("const msg = 'hello'"),
        "Should preserve setup content, got: {}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<div>"),
        "Should contain template JSX, got: {}",
        tsx.code
    );
}

#[test]
fn tsx_destruct_ref_binding_uses_let() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
const msg = ref('')
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // SetupRef must use `let` in destructuring (allows v-model assignment)
    assert!(
        tsx.code.contains("let {") && tsx.code.contains("msg"),
        "SetupRef binding should use `let` in destructuring, got:\n{}",
        tsx.code
    );
    // Must NOT appear in a `const {` destructuring
    assert!(
        !tsx.code.contains("const { msg }") && !tsx.code.contains("const {\n    msg"),
        "SetupRef binding must NOT appear in const destructuring, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_destruct_plain_const_uses_const() {
    let result = compile_tsx(
        r#"<script setup>
const label = 'hello'
</script>
<template><div>{{ label }}</div></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // LiteralConst/SetupConst must use `const` in destructuring
    assert!(
        tsx.code.contains("const {") && tsx.code.contains("label"),
        "SetupConst/LiteralConst binding should use `const` in destructuring, got:\n{}",
        tsx.code
    );
    // Must NOT appear in a `let {` destructuring
    assert!(
        !tsx.code.contains("let {"),
        "SetupConst/LiteralConst binding must NOT appear in let destructuring, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_destruct_mixed_const_and_ref_split_correctly() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
const label = 'hello'
</script>
<template><div>{{ count }} {{ label }}</div></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Should have both `const {` and `let {` destructuring
    assert!(
        tsx.code.contains("const {"),
        "Should have const destructuring for label, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("let {"),
        "Should have let destructuring for count, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_destruct_reactive_uses_let() {
    let result = compile_tsx(
        r#"<script setup>
import { reactive } from 'vue'
const state = reactive({ x: 0 })
</script>
<template><div>{{ state.x }}</div></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // SetupReactiveConst must use `let` (mutable properties)
    assert!(
        tsx.code.contains("let {") && tsx.code.contains("state"),
        "SetupReactiveConst binding should use `let` in destructuring, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_destruct_let_binding_uses_let() {
    let result = compile_tsx(
        r#"<script setup>
let x = 0
</script>
<template><div>{{ x }}</div></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // SetupLet must use `let`
    assert!(
        tsx.code.contains("let {") && tsx.code.contains(" x"),
        "SetupLet binding should use `let` in destructuring, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("const { x }") && !tsx.code.contains("const {\n    x"),
        "SetupLet binding must NOT appear in const destructuring, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infers_native_click_handler_param_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function handleClick(e) {
  return e
}
</script>
<template>
  <button @click="handleClick">Click</button>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            r#"function handleClick(...[e]: [(GlobalEventHandlersEventMap & { [___VERTER___EventKey: string]: Event })["click"]])"#
        ),
        "Expected inferred click handler parameter type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infers_native_input_handler_multi_params() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function handler(a, b, c) {
  return [a, b, c]
}
</script>
<template>
  <input @input="handler" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            r#"...[a, b, c]: [(GlobalEventHandlersEventMap & { [___VERTER___EventKey: string]: Event })["input"]]"#
        ),
        "Expected inferred tuple rest parameter type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infer_function_does_not_transform_arrow_function_handlers() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const handler = (e) => e?.target
</script>
<template>
  <div @click="handler"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("const handler = (e) => e?.target"),
        "Arrow function should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("...[e]: Parameters<"),
        "Arrow function should not receive inferred tuple-rest typing, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infer_function_does_not_transform_no_param_functions() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function noParams() {
  return 1
}
</script>
<template>
  <div @click="noParams"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function noParams()"),
        "No-parameter function should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("Parameters<"),
        "No-parameter function should not receive inferred tuple-rest typing, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_does_not_infer_unbound_function() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function unused(x) {
  return x
}
</script>
<template>
  <div>No event binding</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function unused(x)"),
        "Function not used in template events should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("IntrinsicElementAttributes"),
        "No inferred event types should be injected for unbound function, got:\n{}",
        tsx.code
    );
}

#[test]
pub(super) fn tsx_binding_v5_process_parity_matrix() {
    let cases: [(&str, &[&str], &[&str]); 8] = [
        (
            r#"<script setup>
const test = 1
</script>
<template>{{ test }}</template>"#,
            &["{ test }"],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const test = 1
const handler = () => {}
</script>
<template><div :test="test" @click="handler"></div></template>"#,
            &["test={test}", "onClick={handler}"],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const items = [1]
</script>
<template><div v-for="item in items">{{ item + items.length }}</div></template>"#,
            &["items).map((item) => { return (", "{ item + items.length }"],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const msg = ''
</script>
<template><div :[msg]="msg" /></template>"#,
            &["{...{[msg]: msg}}"],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const msg = ''
</script>
<template><Comp v-model:[`${msg}ss`]="msg" /></template>"#,
            &[r#"[`${msg}ss`]:msg"#],
            &[r#"v-model:"#, "_ctx."],
        ),
        (
            r#"<script setup>
const test = 1
</script>
<template>{{ { test } }}</template>"#,
            &["{ { test } }"],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const test = 1
</script>
<template>{{ [ test, { test }, [test] ] }}</template>"#,
            &["{ [ test, { test }, [test] ] }"],
            &["_ctx."],
        ),
        (
            r#"<template>{{ (foo:string)=> { foo.toLowerCase(); } }}</template>"#,
            &["(foo:string)=> { foo.toLowerCase(); }"],
            &["_ctx.foo", "___VERTER___instance.foo"],
        ),
    ];

    for (source, required_snippets, forbidden_snippets) in cases {
        let result = compile_tsx(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let tsx = result.tsx.as_ref().expect("tsx block");
        for required in required_snippets {
            assert!(
                tsx.code.contains(required),
                "Expected snippet '{}' in TSX output:\n{}",
                required,
                tsx.code
            );
        }
        for forbidden in forbidden_snippets {
            assert!(
                !tsx.code.contains(forbidden),
                "Unexpected snippet '{}' in TSX output:\n{}",
                forbidden,
                tsx.code
            );
        }
    }
}

#[test]
pub(super) fn tsx_binding_type_assertions_do_not_prefix_type_members() {
    let result = compile_tsx(
        r#"<template>{{
  () => {
    let a = {} as { foo: 1 };
    a;
  }
}}</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.code.contains("_ctx.foo"),
        "Type-annotation members must not be treated as runtime bindings, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("_ctx.as"),
        "Type assertion keyword context must not be prefixed as identifier, got:\n{}",
        tsx.code
    );
}

/// Adapted parity matrix for:
/// - template/plugins/conditional/conditional.spec.ts
/// - template/plugins/conditional/generateConditionText.spec.ts
#[test]
fn tsx_conditional_v5_process_parity_matrix() {
    v_if_only_emits_comment_fallback();
    v_if_v_else_no_comment_fallback();
    v_if_v_else_if_no_v_else_emits_comment_fallback();
    v_if_v_else_if_v_else_complete_chain();
    v_if_after_sibling_has_comma_separator();
    v_if_chain_after_sibling();
    v_if_chain_without_v_else_after_sibling();
    v_if_as_root_single_child();
    v_if_v_else_as_root();
    v_if_in_multi_root_fragment();
    multiple_v_if_chains_in_same_parent();
    v_if_with_whitespace_between_branches();
    v_if_nested_inside_v_for();
    v_if_standalone_emits_comment_vnode();
    v_if_else_chain_with_whitespace_valid_output();
    v_if_inside_v_for_with_whitespace();
    v_if_followed_by_sibling_valid_js();
    nested_v_if_chains_no_overlap();
    v_if_with_comment_between_branches();
    comment_between_v_if_branches_does_not_leak_in_prod();

    template_v_if_renders_as_fragment();
    template_v_for_with_v_if_children_renders_as_fragment();
    tsx_v_for_with_v_if_combination_contains_condition_and_map();
    tsx_parent_v_if_with_child_v_for_contains_outer_condition();
}

/// Adapted parity matrix for:
/// - script/plugins/define-options/defineOptions.spec.ts
#[test]
fn tsx_define_options_v5_process_parity_matrix() {
    ts_return_type_annotation_in_computed();
    ts_return_type_no_strip_mode();
    dual_script_export_default_merged_as_options();

    let result = compile_tsx(
        r#"<script setup lang="ts">
defineOptions({ name: 'App', inheritAttrs: false })
</script>
<template><div /></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // With macro boxing, defineOptions arguments are extracted into a boxed constant.
    // The original arguments should appear in the boxing expression.
    assert!(
        tsx.code.contains("{ name: 'App', inheritAttrs: false }"),
        "defineOptions arguments should be preserved in TSX output (boxed or raw), got:\n{}",
        tsx.code
    );
}

/// Mixed authoring pins Pascal intent: a name authored BOTH as `<GlobalCountComp>`
/// and `<global-count-comp>` shares ONE const, and that const keeps the
/// fail-closed Pascal-authored type (the Pascal authoring is component intent —
/// an unregistered name must keep producing a real diagnostic there).
#[test]
fn tsx_mixed_authoring_shares_one_fail_closed_const() {
    let result = compile_tsx(
        r#"<template>
  <GlobalCountComp :count="1" />
  <global-count-comp :count="2" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert_eq!(
        tsx.code.matches("const GlobalCountComp").count(),
        1,
        "one shared const for both spellings, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(
            "const GlobalCountComp = {} as ___VERTER___GlobalComponentType<'GlobalCountComp'>"
        ),
        "Pascal authoring anywhere keeps the fail-closed const type, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code
            .contains("GlobalComponentKebabType<'GlobalCountComp'"),
        "the mixed-authored name must NOT degrade to the fail-open kebab type, got:\n{}",
        tsx.code
    );
    // Both tags reference the shared const.
    assert!(
        tsx.code.matches("<GlobalCountComp").count() >= 2,
        "both spellings rewrite to the shared const, got:\n{}",
        tsx.code
    );
}

/// Per-segment kebab rewrite emits map tokens at each segment head while the
/// segment bodies stay original (1:1 mapped) — the structural property that
/// keeps the tag TAIL mapped (the LSP-side acceptance lives in
/// `verter_lsp tests/cases/kebab_tag_mapping_full_columns.rs`).
#[test]
fn tsx_kebab_rewrite_keeps_segment_bodies_original_in_map() {
    let source = "<template>\n  <global-count-comp />\n</template>\n";
    let result = compile_tsx(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<GlobalCountComp"),
        "rewritten: {}",
        tsx.code
    );

    let sm = oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map)
        .expect("should parse source map");
    let name_off = source.find("global-count-comp").unwrap();
    let src_line = source[..name_off].matches('\n').count() as u32;
    let src_col = (name_off - source[..name_off].rfind('\n').map(|p| p + 1).unwrap_or(0)) as u32;

    // Tokens must exist mapping the SECOND and THIRD segment bodies (the
    // original `ount`/`omp` resumes after each uppercased head) — the
    // whole-name overwrite emitted exactly ONE token at the name start.
    let seg2_body = src_col + "global-c".len() as u32; // 'o' of "count"
    let seg3_body = src_col + "global-count-c".len() as u32; // 'o' of "comp"
    for expect_col in [seg2_body, seg3_body] {
        assert!(
            sm.get_tokens().any(|t| {
                t.get_source_id().is_some()
                    && t.get_src_line() == src_line
                    && t.get_src_col() == expect_col
            }),
            "per-segment emission must carry a token resuming at src col {expect_col} \
             (segment body original chunk); whole-name overwrite has only the start token"
        );
    }
}

#[test]
fn tsx_not_generated_when_disabled() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    assert!(result.tsx.is_none(), "TSX should be None when disabled");
}

/// @ai-generated - TSX output must be independent from force_js mode.
#[test]
pub(super) fn tsx_force_js_toggle_does_not_change_code() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count: number = 1
const msg: string = 'hello'
</script>
<template>
  <button @click="count++">{{ msg }} {{ count }}</button>
</template>"#;

    let force_js_true = compile_tsx_with_force_js(source, true);
    let force_js_false = compile_tsx_with_force_js(source, false);

    assert!(
        force_js_true.errors.is_empty(),
        "force_js=true compile errors: {:?}",
        force_js_true.errors
    );
    assert!(
        force_js_false.errors.is_empty(),
        "force_js=false compile errors: {:?}",
        force_js_false.errors
    );

    let tsx_true = force_js_true.tsx.expect("tsx block (force_js=true)");
    let tsx_false = force_js_false.tsx.expect("tsx block (force_js=false)");

    assert_eq!(
        tsx_true.code, tsx_false.code,
        "TSX code must be identical regardless of force_js"
    );
}

/// F17: a NESTED `<Teleport>` must force block topology `(_openBlock(),
/// _createBlock(_Teleport, …, [array]))` at any depth, with RAW array children
/// — never a plain `_createVNode` and never a component slot object.
#[test]
fn teleport_nested_forces_block_and_array_children() {
    let code = compile_and_validate_template(
        r#"<template><div><Teleport to="body"><span>x</span></Teleport></div></template>"#,
    );
    assert!(
        code.contains("_createBlock(_Teleport"),
        "nested Teleport must use _createBlock (block topology).\n{code}"
    );
    assert!(
        code.contains("(_openBlock(), _createBlock(_Teleport"),
        "nested Teleport must open its own block.\n{code}"
    );
    // NEGATIVE: never a non-block _createVNode.
    assert!(
        !code.contains("_createVNode(_Teleport"),
        "nested Teleport must NOT be a non-block _createVNode.\n{code}"
    );
    // Raw array children, NOT a component slot object.
    assert!(
        !code.contains("default: _withCtx") && !code.contains("{default:"),
        "Teleport children must be a raw VNode array, not a slot object.\n{code}"
    );
    assert!(
        code.contains("}, ["),
        "Teleport children must open as an array literal after props.\n{code}"
    );
}

#[test]
fn tsx_shallow_unwrap_ref_avoids_tdz() {
    // The shallowUnwrapRef call must use a temp variable outside the block scope
    // to avoid TDZ (Temporal Dead Zone) errors where `const { count } = shallowUnwrapRef({ count: count ... })`
    // would self-reference the uninitialized binding.
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const message = ref('hello')
</script>

<template>
  <div>{{ count }} {{ message }}</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: temp variable pattern
    assert!(
        tsx.code
            .contains("___VERTER___unwrapped = ___VERTER___shallowUnwrapRef("),
        "Should use temp variable for shallowUnwrapRef.\nTSX:\n{}",
        tsx.code
    );

    // Positive: destructuring from temp
    assert!(
        tsx.code.contains("} = ___VERTER___unwrapped;"),
        "Should destructure from ___VERTER___unwrapped temp variable.\nTSX:\n{}",
        tsx.code
    );

    // Negative: old combined pattern must not appear (TDZ-prone)
    assert!(
        !tsx.code.contains("} = ___VERTER___shallowUnwrapRef("),
        "Old combined destructure+call pattern must not appear (causes TDZ).\nTSX:\n{}",
        tsx.code
    );

    // Positive: boundary markers around the destructuring block
    assert!(
        tsx.code.contains("/* verter-destructured-start */"),
        "Destructuring block must be wrapped with start marker.\nTSX:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("/* verter-destructured-end */"),
        "Destructuring block must be wrapped with end marker.\nTSX:\n{}",
        tsx.code
    );

    // The start marker must come before the destructuring, end marker after
    let start_marker_pos = tsx.code.find("/* verter-destructured-start */").unwrap();
    let end_marker_pos = tsx.code.find("/* verter-destructured-end */").unwrap();
    let destruct_pos = tsx.code.find("} = ___VERTER___unwrapped;").unwrap();
    assert!(
        start_marker_pos < destruct_pos && destruct_pos < end_marker_pos,
        "Markers must bracket the destructuring: start={}, destruct={}, end={}\nTSX:\n{}",
        start_marker_pos,
        destruct_pos,
        end_marker_pos,
        tsx.code
    );
}

#[test]
fn tsx_custom_types_module_in_output() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        types_module_name: Some("@my/types".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions::default();
    let source = r#"<script setup>const x = 1</script><template><div/></template>"#;
    let result = compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tsx = result.tsx.expect("tsx block");
    assert!(
        tsx.code.contains(r#"from "@my/types""#),
        "custom types module should be used, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains(r#"from "@verter/types""#),
        "default types module should NOT appear"
    );
}

#[test]
fn tsx_default_types_module_is_verter_types() {
    let result = compile_tsx(r#"<script setup>const x = 1</script><template><div/></template>"#);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(r#"from "@verter/types""#),
        "default should use @verter/types, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains(r#"from "$verter/types$""#),
        "should NOT use $verter/types$"
    );
}

#[test]
fn tsx_options_api_has_type_constructs_at_compile_level() {
    let result = compile_tsx(
        r#"<script lang="ts">
export default { props: ['msg'] }
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    // Helper imports present
    assert!(
        tsx.code.contains(r#"from "@verter/types""#),
        "Options API should have types imports, got:\n{}",
        tsx.code
    );
    // Negative: Instance type should no longer be emitted
    assert!(
        !tsx.code.contains("___VERTER___Instance"),
        "Options API should not have Instance type construct"
    );
    // OXC validation
    let alloc = oxc_allocator::Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Options API TSX must be valid JS: {:?}\n---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tsx.code
    );
}

// ── Error recovery: malformed templates must not panic ─────────────
// @ai-generated — Verifies that broken templates produce diagnostics, not panics.

#[test]
fn error_recovery_orphan_close_tag_does_not_panic() {
    // `</component` inside template — orphan close tag
    let result = compile_sfc("<template></component</template>");
    assert!(
        !result.errors.is_empty(),
        "should report diagnostics for orphan close tag"
    );
}

#[test]
fn error_recovery_incomplete_tag_does_not_panic() {
    let result = compile_sfc("<template><</template>");
    // Should not panic — incomplete `<` treated as text
    let _ = result;
}

#[test]
fn error_recovery_bare_close_tag_no_angle() {
    // `</component` without closing `>` — tokenizer must handle gracefully
    let result = compile_sfc("<template></component");
    // Must not panic
    let _ = result;
}

/// @ai-generated — Nested static elements emit _cache wrapping
#[test]
fn static_hoist_nested_static() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><section><div><span>deep</span></div></section></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "deeply nested static subtree should use _cache wrapping\n--- code ---\n{}",
        code
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "should NOT use _createStaticVNode\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Element with ref is NOT hoisted
#[test]
fn static_hoist_ref_not_hoisted() {
    let code =
        compile_and_validate_hoisted(r#"<template><div><div ref="el">text</div></div></template>"#);
    assert!(
        !code.contains("_createStaticVNode"),
        "element with ref should NOT be hoisted\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Static with dynamic sibling: both present
#[test]
fn static_hoist_mixed_static_and_dynamic() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><span>static</span><span :class="x">dynamic</span></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "static sibling should use _cache wrapping\n--- code ---\n{}",
        code
    );
    assert!(
        code.contains("_createElementVNode"),
        "dynamic sibling should use _createElementVNode\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Consecutive static siblings each get _cache[N] entries
#[test]
fn static_hoist_consecutive_siblings_merge() {
    // All three <p> are direct children of <div> and are ALL individually
    // cacheable — official Vue's `cacheStatic` groups them into ONE cached
    // array and spreads it back (`[...(_cache[0] || (_cache[0] = [a, b,
    // c]))]`), rather than caching each sibling separately. Verified
    // against the vendored `@vue/compiler-sfc`/`@vue/compiler-core` output
    // for this exact shape (also matches the conformance goldens for
    // `elements-text/static-element` and `elements-text/static-class-style`).
    let code =
        compile_and_validate_hoisted(r#"<template><div><p>a</p><p>b</p><p>c</p></div></template>"#);
    assert!(
        code.contains("...(_cache[0] || (_cache[0] = ["),
        "the three static siblings should be ONE grouped cache array, not \
         three individually cached elements\n--- code ---\n{}",
        code
    );
    assert_eq!(
        code.matches("_cache[0]").count(),
        2,
        "exactly one grouped cache slot (referenced twice: `_cache[0] ||` \
         and `_cache[0] =`)\n--- code ---\n{}",
        code
    );
    assert!(
        !code.contains("_cache[1]"),
        "must not be three independently cached elements\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — hoist_static=false disables the optimization
#[test]
fn static_hoist_disabled() {
    let code = compile_and_validate_no_hoist(
        r#"<template><div><div class="card"><h3>Title</h3></div></div></template>"#,
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "hoist_static=false should disable optimization\n--- code ---\n{}",
        code
    );
    assert!(
        code.contains("_createElementVNode"),
        "should use _createElementVNode when hoisting disabled\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — HTML with double quotes produces valid JS (OXC parse check)
#[test]
fn static_hoist_html_quotes_valid_js() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div class="foo" id="bar">text</div></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "should use _cache wrapping for static element with attributes\n--- code ---\n{}",
        code
    );
    // The OXC parse in compile_and_validate_hoisted already validates JS syntax
}

/// @ai-generated — Static parent with one dynamic child is NOT static
#[test]
fn static_hoist_parent_with_dynamic_child() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div class="wrapper"><span :title="t">x</span></div></div></template>"#,
    );
    // The wrapper div has a dynamic child, so it's not fully static
    assert!(
        !code.contains("_createStaticVNode"),
        "parent with dynamic child should NOT be hoisted\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Static element with class (no dynamic binding) is cached
#[test]
fn static_hoist_static_class() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div class="foo">text</div></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "element with static class should use _cache wrapping\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Deep nesting (3+ levels) all static
#[test]
fn static_hoist_deep_nesting() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div><div><div><span>deep</span></div></div></div></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "deeply nested static should use _cache wrapping\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Self-closing static element
#[test]
fn static_hoist_self_closing() {
    let code = compile_and_validate_hoisted(r#"<template><div><br/><hr/></div></template>"#);
    assert!(
        code.contains("_cache["),
        "self-closing static elements should use _cache wrapping\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Template literal escaping: backtick in attribute
#[test]
fn static_hoist_backtick_in_html() {
    let code = compile_and_validate_hoisted(
        "<template><div><div title=\"a`b\">text</div></div></template>",
    );
    assert!(
        code.contains("_cache["),
        "should use _cache wrapping\n--- code ---\n{}",
        code
    );
}

#[test]
fn tsx_export_instance_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { getCurrentInstance } from 'vue'
const instance = getCurrentInstance()
</script>
<template><div>{{ instance?.proxy }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Negative: Instance and CurrentComponentInstance should no longer be exported
    assert!(
        !tsx.code.contains("export type ___VERTER___Instance"),
        "Instance type should no longer be exported, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code
            .contains("export type ___VERTER___CurrentComponentInstance"),
        "CurrentComponentInstance type should no longer be exported, got:\n{}",
        tsx.code
    );
    // Negative: no bare type declarations either
    assert!(
        !tsx.code.contains("\ntype ___VERTER___Instance"),
        "Instance type should NOT have bare 'type' declaration:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_no_get_current_instance_no_declaration() {
    // Issue #11: When getCurrentInstance() is NOT called, the declaration must not appear
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Negative: no getCurrentInstance declaration when it's not used
    assert!(
        !tsx.code.contains("declare function getCurrentInstance"),
        "getCurrentInstance declaration must NOT appear when not used, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("___VERTER___Instance"),
        "Instance type must NOT appear when getCurrentInstance is not used, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_jsdoc_on_shallow_unwrap_ref_entries() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
/** My counter */
const count = ref(0)
const plain = "hello"
</script>
<template><div>{{ count }} {{ plain }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Positive: shallowUnwrapRef should contain JSDoc before count
    assert!(
        tsx.code.contains("/** My counter */\n    count:"),
        "shallowUnwrapRef should have JSDoc before count entry, got:\n{}",
        tsx.code
    );
    // Negative: plain should NOT have JSDoc before it
    let plain_in_unwrap = tsx.code.find("plain: plain as unknown");
    if let Some(pos) = plain_in_unwrap {
        let before = &tsx.code[pos.saturating_sub(20)..pos];
        assert!(
            !before.contains("/**"),
            "plain should not have JSDoc before it in shallowUnwrapRef:\n{}",
            tsx.code
        );
    }
}

#[test]
fn tsx_comp_with_ref_has_void_reference() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
</script>
<template><div ref="el">text</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Positive: void reference to suppress unused warning
    assert!(
        tsx.code.contains("void ___VERTER___Comp"),
        "Comp function should have void reference to suppress unused warning, got:\n{}",
        tsx.code
    );
}

#[test]
fn jsx_compile_lang_js_explicit() {
    assert_jsx_parses(
        r#"<script setup lang="js">
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
        "explicit lang=js",
    );
}

#[test]
fn jsx_compile_options_api() {
    assert_jsx_parses(
        r#"<script>
export default {
  data() { return { count: 0 } }
}
</script>
<template><div>{{ count }}</div></template>"#,
        "options API JS",
    );
}

#[test]
fn jsx_compile_ts_sfc_stays_tsx() {
    // TypeScript SFCs should still produce TSX (is_jsx = false)
    let result = compile_tsx(
        r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    let tsx = result.tsx.as_ref().expect("should have tsx block");
    assert!(
        !tsx.is_jsx,
        "TS SFC should produce TSX (is_jsx = false):\n{}",
        tsx.code
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// ── attrs attribute on <script setup> — IDE codegen ─────────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsx_attrs_explicit_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts" attrs="{ class?: string; id?: string }">
import { ref } from 'vue'
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: explicit attrs type emitted
    assert!(
        tsx.code.contains("___VERTER___attributes"),
        "should emit ___VERTER___attributes type alias, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("{ class?: string; id?: string }"),
        "should contain the attrs type value, got:\n{}",
        tsx.code
    );

    // Negative: should not contain the raw attrs= attribute in TSX output
    assert!(
        !tsx.code.contains(r#"attrs="{ class"#),
        "raw attrs attribute should not appear in output, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_attrs_default_empty() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: default empty attrs type emitted
    assert!(
        tsx.code.contains("type ___VERTER___attributes = {}"),
        "should emit empty ___VERTER___attributes type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_options_api_skips_unreferenced_attributes_alias() {
    let result = compile_tsx(
        r#"<script lang="ts">
import { defineComponent } from "vue"

export default defineComponent({
  props: {
    msg: { type: String, required: true }
  }
})
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    assert!(
        tsx.code
            .contains("declare let ___VERTER___instance: InstanceType<"),
        "Options API template should retain its ambient instance reference, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("___VERTER___attributes"),
        "Options API must not emit the unreferenced attributes alias, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("type ___VERTER___attributes = {};"),
        "Options API must not retain the old empty attributes declaration, got:\n{}",
        tsx.code
    );

    let alloc = oxc_allocator::Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Options API TSX must parse without the alias: {:?}\n---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>(),
        tsx.code
    );
}

#[test]
fn tsx_attrs_alias_attributes() {
    // Also accept 'attributes' as alias for 'attrs'
    let result = compile_tsx(
        r#"<script setup lang="ts" attributes="{ role?: string }">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: attributes alias works the same as attrs
    assert!(
        tsx.code.contains("{ role?: string }"),
        "'attributes' alias should produce the same type, got:\n{}",
        tsx.code
    );
}

#[test]
fn jsx_attrs_explicit_type() {
    let result = compile_tsx_with_force_js(
        r#"<script setup attrs="{ class?: string }">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
        true,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: JSDoc typedef for attrs
    assert!(
        tsx.code.contains("@typedef"),
        "JS mode should use JSDoc @typedef, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("{ class?: string }"),
        "should contain the attrs type in JSDoc, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("___VERTER___attributes"),
        "should reference ___VERTER___attributes, got:\n{}",
        tsx.code
    );
}

#[test]
fn jsx_attrs_default_empty() {
    let result = compile_tsx_with_force_js(
        r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
        true,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: default empty attrs typedef
    assert!(
        tsx.code.contains("@typedef {{}} ___VERTER___attributes"),
        "JS mode should emit empty attrs typedef, got:\n{}",
        tsx.code
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// ── useAttrs<T>() fallback for attrs type — IDE codegen ─────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsx_use_attrs_type_arg_fallback() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs<{ class?: string; id?: string }>()
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: attrs type from useAttrs<T>() used as fallback
    assert!(
        tsx.code.contains("{ class?: string; id?: string }"),
        "should use useAttrs type parameter as attrs type, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("___VERTER___attributes"),
        "should emit ___VERTER___attributes type alias, got:\n{}",
        tsx.code
    );

    // Negative: should not have empty attrs type
    assert!(
        !tsx.code.contains("type ___VERTER___attributes = {};"),
        "should not emit empty attrs when useAttrs<T> provides type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_attrs_attribute_takes_priority_over_use_attrs() {
    let result = compile_tsx(
        r#"<script setup lang="ts" attrs="{ role?: string }">
import { useAttrs } from 'vue'
const attrs = useAttrs<{ class?: string }>()
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: attrs attribute value takes priority
    assert!(
        tsx.code
            .contains("___VERTER___attributes = { role?: string }"),
        "attrs attribute should take priority in type alias, got:\n{}",
        tsx.code
    );

    // Negative: useAttrs type should NOT be in the type alias
    assert!(
        !tsx.code
            .contains("___VERTER___attributes = { class?: string }"),
        "useAttrs type should not be used in type alias when attrs attribute present, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_use_attrs_without_type_arg_no_effect() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs()
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: plain useAttrs() without type param → default empty attrs
    assert!(
        tsx.code.contains("type ___VERTER___attributes = {}"),
        "useAttrs() without type param should produce empty attrs type, got:\n{}",
        tsx.code
    );
}

// ── Instance declaration regression tests ─────────────────────────
//
// These tests verify that the ___VERTER___instance declaration is correctly
// typed based on the script language. TS SFCs use TypeScript declarations;
// JS SFCs use a JSDoc InstanceType bridge to the public API carrier.

#[test]
fn tsx_instance_declaration_ts_sfc_has_instance_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(!tsx.is_jsx, "TS SFC should produce TSX, not JSX");

    // Positive: should have typed InstanceType declaration
    assert!(
        tsx.code.contains("InstanceType<typeof import("),
        "TS SFC instance declaration should use InstanceType<typeof import(...)>, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("let ___VERTER___instance!:"),
        "TS SFC should use definite assignment 'let ... !:', got:\n{}",
        tsx.code
    );

    // Negative: must NOT have JSDoc `any` instance declaration
    assert!(
        !tsx.code
            .contains("/** @type {any} */\nvar ___VERTER___instance"),
        "TS SFC must NOT use JSDoc @type {{any}} for instance, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_instance_declaration_js_sfc_has_jsdoc_public_instance_bridge() {
    let result = compile_tsx(
        r#"<script setup>
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(tsx.is_jsx, "JS SFC (no lang attr) should produce JSX");

    // Positive: JSDoc carries the public constructor instance type.
    assert!(
        tsx.code.contains(
            "/** @type {InstanceType<typeof import(\"./App.vue.verter.js\")['default']>} */"
        ),
        "JS SFC should use the JSDoc public-instance bridge, got:\n{}",
        tsx.code
    );

    // Negative: the JavaScript carrier must not use TypeScript declaration syntax.
    assert!(
        !tsx.code.contains("let ___VERTER___instance!:"),
        "JS SFC must NOT use definite assignment 'let ... !:', got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_instance_declaration_explicit_js_lang_has_jsdoc_public_instance_bridge() {
    let result = compile_tsx(
        r#"<script setup lang="js">
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(tsx.is_jsx, "lang='js' SFC should produce JSX");

    // Positive: JSDoc carries the public constructor instance type.
    assert!(
        tsx.code.contains(
            "/** @type {InstanceType<typeof import(\"./App.vue.verter.js\")['default']>} */"
        ),
        "lang='js' SFC should use the JSDoc public-instance bridge, got:\n{}",
        tsx.code
    );

    // Negative: the JavaScript carrier must not use TypeScript declaration syntax.
    assert!(
        !tsx.code.contains("let ___VERTER___instance!:"),
        "lang='js' SFC must NOT use definite assignment 'let ... !:', got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_instance_declaration_options_api_ts_has_ambient_instance_type() {
    let result = compile_tsx(
        r#"<script lang="ts">
export default {
  data() { return { count: 0 } }
}
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(!tsx.is_jsx, "TS Options API should produce TSX, not JSX");

    // Positive: should have ambient typed instance declaration
    assert!(
        tsx.code.contains("declare let ___VERTER___instance:"),
        "TS Options API should use 'declare let' for instance, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("InstanceType<typeof import("),
        "TS Options API should use InstanceType<typeof import(...)>, got:\n{}",
        tsx.code
    );

    // Negative: must NOT have JSDoc `any`
    assert!(
        !tsx.code
            .contains("/** @type {any} */\nvar ___VERTER___instance"),
        "TS Options API must NOT use JSDoc @type {{any}} for instance, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_instance_declaration_options_api_js_has_jsdoc_typed() {
    let result = compile_tsx(
        r#"<script>
export default {
  data() { return { count: 0 } }
}
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(tsx.is_jsx, "JS Options API should produce JSX");

    // Positive: should have inline defineComponent wrapping for plain object export
    assert!(
        tsx.code.contains("___VERTER___defineComponent)(__sfc__)"),
        "JS Options API should wrap __sfc__ with defineComponent, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("InstanceType<typeof ___VERTER___dc>"),
        "JS Options API should use InstanceType<typeof dc> for instance, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("var ___VERTER___instance"),
        "JS Options API should use var (not declare let) for instance, got:\n{}",
        tsx.code
    );
    // Positive: defineComponent must be imported from vue
    assert!(
        tsx.code
            .contains("defineComponent as ___VERTER___defineComponent"),
        "JS Options API should import defineComponent from vue, got:\n{}",
        tsx.code
    );

    // Negative: must NOT have TS ambient instance syntax
    assert!(
        !tsx.code.contains("declare let ___VERTER___instance:"),
        "JS Options API must NOT use 'declare let' for instance, got:\n{}",
        tsx.code
    );
    // Negative: must NOT have the old untyped @type {any}
    assert!(
        !tsx.code
            .contains("/** @type {any} */\nvar ___VERTER___instance"),
        "JS Options API must NOT use untyped @type {{any}} for instance, got:\n{}",
        tsx.code
    );
    // Negative: must NOT use self-import pattern (TSGO can't resolve virtual .vue.jsx imports)
    assert!(
        !tsx.code.contains("import('./"),
        "JS Options API must NOT use self-import for instance typing, got:\n{}",
        tsx.code
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// ── _attrs parameter on TemplateBindingFN — IDE codegen ─────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsx_attrs_param_inline_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts" attrs="{ class: string }">
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: _attrs parameter in function signature
    assert!(
        tsx.code
            .contains("TemplateBindingFN(_attrs: { class: string })"),
        "should have _attrs param with inline type, got:\n{}",
        tsx.code
    );

    // Negative: should NOT have empty parens
    assert!(
        !tsx.code.contains("TemplateBindingFN()"),
        "should not have empty parens when attrs specified, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_no_attrs_param_without_attrs_attr() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: empty parens when no attrs
    assert!(
        tsx.code.contains("TemplateBindingFN()"),
        "should have empty parens without attrs, got:\n{}",
        tsx.code
    );

    // Negative: no _attrs parameter
    assert!(
        !tsx.code.contains("_attrs"),
        "should not have _attrs param without attrs attribute, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_no_attrs_param_jsx_mode() {
    let result = compile_tsx_with_force_js(
        r#"<script setup attrs="{ class: string }">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
        true,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: JSX mode → empty parens (no TS annotations)
    assert!(
        tsx.code.contains("TemplateBindingFN()"),
        "JSX mode should have empty parens (no TS annotations), got:\n{}",
        tsx.code
    );

    // Negative: no _attrs in JSX mode
    assert!(
        !tsx.code.contains("_attrs:"),
        "JSX mode should not have _attrs TS annotation, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_attrs_priority_over_attributes() {
    let result = compile_tsx(
        r#"<script setup lang="ts" attrs="{ role: string }" attributes="{ id: string }">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: attrs value wins
    assert!(
        tsx.code.contains("{ role: string }"),
        "attrs should take priority over attributes, got:\n{}",
        tsx.code
    );

    // Negative: attributes value should NOT appear
    assert!(
        !tsx.code.contains("{ id: string }"),
        "attributes value should not appear when attrs is present, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_bare_use_attrs_with_explicit_attrs_typeof_cast() {
    let result = compile_tsx(
        r#"<script setup lang="ts" attrs="{ class: string }">
import { useAttrs } from 'vue'
const attrs = useAttrs()
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: bare useAttrs() gets `as typeof _attrs` cast
    assert!(
        tsx.code.contains("useAttrs() as typeof _attrs"),
        "bare useAttrs() should be cast to typeof _attrs when attrs specified, got:\n{}",
        tsx.code
    );

    // Negative: should NOT use the old ___VERTER___Attrs cast
    assert!(
        !tsx.code.contains("as unknown as ___VERTER___Attrs"),
        "should not use ___VERTER___Attrs cast when explicit attrs, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_bare_use_attrs_without_explicit_attrs_keeps_verter_cast() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs()
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: bare useAttrs() still gets ___VERTER___Attrs cast when no explicit attrs
    assert!(
        tsx.code.contains("as unknown as ___VERTER___Attrs"),
        "bare useAttrs() should use ___VERTER___Attrs cast without explicit attrs, got:\n{}",
        tsx.code
    );

    // Negative: should NOT use typeof _attrs
    assert!(
        !tsx.code.contains("as typeof _attrs"),
        "should not use typeof _attrs when no explicit attrs, got:\n{}",
        tsx.code
    );
}

// ==================== Vue 3.4+ same-name shorthand bindings ====================

// @ai-generated - TDD test: Issue 1 — `:disabled` with no value resolves to the binding
#[test]
fn bind_shorthand_resolves_to_binding() {
    // Vue 3.4+ shorthand: `:disabled` is equivalent to `:disabled="disabled"`
    let code = compile_and_validate_template(
        r#"<template><button :disabled>click</button></template>
<script setup>const disabled = true;</script>"#,
    );
    // Should resolve to the setup binding, not emit ""
    assert!(
        code.contains("disabled: $setup.disabled"),
        ":disabled shorthand should resolve to $setup.disabled, got:\n{}",
        code
    );
    assert!(
        !code.contains("disabled: \"\""),
        ":disabled shorthand should NOT emit empty string, got:\n{}",
        code
    );
}

#[test]
fn bind_shorthand_id_resolves_to_binding() {
    let code = compile_and_validate_template(
        r#"<template><div :id>content</div></template>
<script setup>const id = 'my-div';</script>"#,
    );
    assert!(
        code.contains("id: $setup.id"),
        ":id shorthand should resolve to $setup.id, got:\n{}",
        code
    );
}

#[test]
fn bind_shorthand_class_uses_normalize_class() {
    // `:class="myClass"` (explicit value) uses _normalizeClass — verify
    // shorthand doesn't break existing class normalization behavior.
    let code = compile_and_validate_template(
        r#"<template><div :class="myClass">content</div></template>
<script setup>const myClass = 'active';</script>"#,
    );
    assert!(
        code.contains("_normalizeClass("),
        ":class should use _normalizeClass, got:\n{}",
        code
    );
}

/// `withDefaults(defineProps<T>(), { ...Defaults, k: make<X>() })` under force_js
/// must strip TypeScript from the FULL defaults object passed to `_mergeDefaults`
/// — not only the individual resolved-type prop values. The whole second-argument
/// expression is emitted verbatim on the spread path, so nested type args
/// (`make<number>(0)`) would otherwise leak into the JS output.
#[test]
fn force_js_with_defaults_spread_strips_full_defaults_object() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "as",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "n",
                true,
                [verter_macro_dto::RuntimeConstructor::Number],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"
<script setup lang="ts">
import { base } from './base'
import { make } from './make'
interface Props { as?: string; n?: number }
const props = withDefaults(defineProps<Props>(), {
  ...base,
  n: make<number>(0),
})
</script>
<template><div>{{ props.as }}</div></template>
"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(
        code.contains("_mergeDefaults"),
        "spread defaults must use _mergeDefaults, got:\n{code}"
    );
    assert!(
        code.contains("...base"),
        "full defaults spread expression must remain, got:\n{code}"
    );
    assert!(
        !code.contains("make<number>") && !code.contains("<number>"),
        "type args inside the defaults object must be stripped under force_js, got:\n{code}"
    );
    assert!(
        code.contains("make(0)"),
        "runtime call in defaults must remain, got:\n{code}"
    );
}

/// F3: a top-level `await` argument must have its TypeScript type arguments
/// stripped under force_js. The async-context transform wraps the awaited
/// expression; embedding the raw argument would place `<Result>` inside an
/// Overwritten chunk the body strip cannot reach (nested-overwrite no-op).
#[test]
fn force_js_top_level_await_strips_type_args_from_argument() {
    let result = compile_sfc(
        r#"
<script setup lang="ts">
import type { Result } from './api'
import { load } from './api'
const x = await load<Result>()
</script>
<template><div>{{ x }}</div></template>
"#,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(
        code.contains("_withAsyncContext"),
        "top-level await must use _withAsyncContext, got:\n{code}"
    );
    assert!(
        !code.contains("load<Result>") && !code.contains("<Result>"),
        "type args on the awaited call must be stripped under force_js, got:\n{code}"
    );
    assert!(
        code.contains("load()"),
        "the awaited runtime call must remain, got:\n{code}"
    );
}

/// force_js: same-file empty interface chain + withDefaults still emits pure JS.
#[test]
fn force_js_same_file_empty_interface_extends_chain_is_pure_js() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "a",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "b",
                true,
                [verter_macro_dto::RuntimeConstructor::Number],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"
<script setup lang="ts">
interface A { a?: string }
interface B extends A { b?: number }
interface C extends B {}
const props = withDefaults(defineProps<C>(), { a: 'x' })
const n = (1 as number)!
</script>
<template><div>{{ props.a }} {{ n }}</div></template>
"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(code.contains("a:"), "must emit prop a, got: {code}");
    assert!(code.contains("b:"), "must emit prop b, got: {code}");
    assert!(!code.contains("interface "), "got: {code}");
    assert!(!code.contains("as number"), "got: {code}");
    assert!(!code.contains("defineProps<"), "got: {code}");
}

/// force_js: export type / type alias decls do not leak into runtime output.
#[test]
fn force_js_strips_export_type_and_type_alias() {
    let result = compile_sfc(
        r#"
<script setup lang="ts">
export type Payload = { id: number }
type Local = string
const id: Local = '1'
const payload = { id: 1 } as Payload
</script>
<template><div>{{ id }}</div></template>
"#,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(!code.contains("export type"), "got: {code}");
    assert!(!code.contains("type Local"), "got: {code}");
    assert!(!code.contains("as Payload"), "got: {code}");
    assert!(!code.contains(": Local"), "got: {code}");
    assert!(
        code.contains("const id = '1'") || code.contains("const id='1'"),
        "got: {code}"
    );
}

#[test]
fn companion_nonliteral_default_var_ref_preserved() {
    // BLOCKER: JS <script setup> + companion `export default baseOptions`
    // (non-literal). Official rebinds to `const __default__ = baseOptions`
    // and merges via Object.assign — the options are NEVER dropped.
    let code = compile_sfc_script_code(
        r#"<script>
const baseOptions = { inheritAttrs: false }
export default baseOptions
</script>

<script setup>
const msg = 'hi'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("const __default__ = baseOptions"),
        "non-literal companion default must be bound as __default__, got:\n{}",
        code
    );
    assert!(
        code.contains("/*@__PURE__*/Object.assign(__default__, {"),
        "__default__ must be the Object.assign target, got:\n{}",
        code
    );
    assert!(
        code.contains("__name: 'App'"),
        "runtime options merged in, got:\n{}",
        code
    );
    assert_eq!(
        code.matches("export default").count(),
        1,
        "exactly one default export (the component), got:\n{}",
        code
    );
}

#[test]
fn companion_nonliteral_default_call_preserved() {
    // BLOCKER: companion `export default makeOptions()` (call expression).
    let code = compile_sfc_script_code(
        r#"<script>
function makeOptions() { return { inheritAttrs: false } }
export default makeOptions()
</script>

<script setup>
const msg = 'hi'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("const __default__ = makeOptions()"),
        "call-expression companion default must be bound verbatim, got:\n{}",
        code
    );
    assert!(
        code.contains("/*@__PURE__*/Object.assign(__default__, {"),
        "__default__ must be the Object.assign target, got:\n{}",
        code
    );
}

#[test]
fn companion_literal_default_merges_via_default_binding() {
    // Official: literal companion default is ALSO bound as `const __default__`
    // and used as the Object.assign target (not inlined).
    let code = compile_sfc_script_code(
        r#"<script>
export default {
  inheritAttrs: false,
};
</script>

<script setup>
const msg = 'hi'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("const __default__ = {"),
        "literal companion default must be bound as __default__, got:\n{}",
        code
    );
    assert!(
        code.contains("inheritAttrs: false"),
        "companion options preserved, got:\n{}",
        code
    );
    assert!(
        code.contains("/*@__PURE__*/Object.assign(__default__, {"),
        "__default__ must be the Object.assign target, got:\n{}",
        code
    );
    assert_eq!(
        code.matches("export default").count(),
        1,
        "exactly one default export, got:\n{}",
        code
    );
}

#[test]
fn companion_default_and_define_options_merge_both() {
    // HIGH: companion default AND defineOptions — official merges BOTH:
    // Object.assign(__default__, <definedOptions>, { <runtime> }).
    let code = compile_sfc_script_code(
        r#"<script>
export default { name: 'FromScript' }
</script>

<script setup>
defineOptions({ inheritAttrs: false })
const msg = 'hi'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("const __default__ = { name: 'FromScript' }"),
        "companion default bound, got:\n{}",
        code
    );
    assert!(
        code.contains("/*@__PURE__*/Object.assign(__default__, { inheritAttrs: false }, {"),
        "both option sources merge in official order (default, defineOptions, runtime), got:\n{}",
        code
    );
    assert!(
        code.contains("name: 'FromScript'") && code.contains("inheritAttrs: false"),
        "neither option source is dropped, got:\n{}",
        code
    );
}

#[test]
fn ts_companion_nonliteral_default_preserved() {
    // BLOCKER for TS too: non-literal companion default must not be dropped.
    let code = compile_sfc_script_code(
        r#"<script lang="ts">
const baseOptions = { inheritAttrs: false }
export default baseOptions
</script>

<script setup lang="ts">
const msg = 'hi'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("const __default__ = baseOptions"),
        "TS non-literal companion default bound, got:\n{}",
        code
    );
    assert!(
        code.contains("...__default__"),
        "TS spreads __default__ into the wrapper, got:\n{}",
        code
    );
}

#[test]
fn ts_define_options_spread_shape() {
    // TS + defineOptions only: official spreads `...<definedOptions>` inside
    // _defineComponent (not inlined properties).
    let code = compile_sfc_script_code(
        r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
const msg = 'hi'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("/*@__PURE__*/_defineComponent({\n  ...{ inheritAttrs: false },"),
        "TS defineOptions emits the official spread shape, got:\n{}",
        code
    );
}

#[test]
fn ts_companion_and_define_options_merge_both() {
    // TS with BOTH: official emits both spreads in order.
    let code = compile_sfc_script_code(
        r#"<script lang="ts">
export default { name: 'FromScript' }
</script>

<script setup lang="ts">
defineOptions({ inheritAttrs: false })
const msg = 'hi'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("...__default__,"),
        "companion spread present, got:\n{}",
        code
    );
    assert!(
        code.contains("...{ inheritAttrs: false },"),
        "defineOptions spread present, got:\n{}",
        code
    );
    let default_pos = code.find("...__default__").unwrap();
    let options_pos = code.find("...{ inheritAttrs: false }").unwrap();
    assert!(
        default_pos < options_pos,
        "official order: __default__ spread before defineOptions spread, got:\n{}",
        code
    );
}

#[test]
fn define_options_literal_const_reference_stays_valid() {
    // Official exempts literal-const bindings (they are hoistable constants).
    let result = compile_sfc(
        "<script setup>\nconst compName = 'MyComp'\ndefineOptions({ name: compName })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "literal-const reference must stay valid (official exemption), got: {:?}",
        result.errors
    );
}

#[test]
fn inline_static_ref_without_binding_stays_hoisted_string() {
    // No matching setup binding → official keeps `{ ref: "el" }` (hoisted string).
    let result = compile_sfc_inline(
        "<script setup>\nconst x = 1\n</script>\n<template><div ref=\"el\">x</div></template>",
    );
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("{ ref: \"el\" }"),
        "static ref without a setup binding stays a string, got:\n{}",
        code
    );
}

#[test]
fn result_inline_true_when_runtime_inline_happens() {
    let result = compile_sfc_inline(
        "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    assert!(
        result.inline,
        "result.inline true when the runtime inline happened"
    );
    assert!(result.template.is_none());
}

// =========================================================================
// FIX2 — all-literal enum is a literal-const (valid in defineOptions)
// =========================================================================

#[test]
fn define_options_all_literal_enum_stays_valid() {
    // Official isAllLiteral: every member has no initializer or a static
    // (scalar-literal) initializer → literal-const → allowed.
    let code = compile_sfc_script_code(
        "<script setup lang=\"ts\">\nenum E { A, B }\ndefineOptions({ x: E.A })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        code.contains("E.A"),
        "all-literal enum must stay valid (official literal-const), got:\n{}",
        code
    );
}

#[test]
fn define_options_non_literal_enum_member_is_compile_error() {
    let result = compile_sfc(
        "<script setup lang=\"ts\">\nfunction someFn(): number { return 1 }\nenum E { A = someFn() }\ndefineOptions({ x: E.A })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(D1_OFFICIAL_MESSAGE)),
        "enum with a non-literal member must be rejected (official), got: {:?}",
        result.errors
    );
}

// =========================================================================
// R5-FIX1 — `is_static_node` unwraps TS wrappers (official `unwrapTSNode`)
// =========================================================================
//
// Official `isStaticNode` calls `unwrapTSNode(node)` FIRST, stripping
// `as`/`satisfies`/`!`/type-assertion/instantiation wrappers before the
// scalar-literal / static-composition match. So an all-literal enum whose
// members carry those wrappers is still `literal-const` and stays valid in
// `defineOptions`/`defineProps`/etc.

#[test]
fn r5_fix1_enum_ts_wrapped_literal_members_stay_valid() {
    // `1 as const` / `2 satisfies number` unwrap to scalar literals → every
    // member is static → the enum is all-literal (literal-const) → referencing
    // it in defineOptions is valid (no scope-reference error).
    let result = compile_sfc(
        "<script setup lang=\"ts\">\nenum E { A = 1 as const, B = 2 satisfies number }\ndefineOptions({ x: E.A })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "TS-wrapped all-literal enum must stay valid (official unwrapTSNode), got: {:?}",
        result.errors
    );
}

#[test]
fn r5_fix1_enum_nonnull_and_assertion_wrapped_members_stay_valid() {
    // Non-null `!` and angle-bracket type-assertion wrappers over scalar
    // literals also unwrap to statics → all-literal enum stays valid.
    let result = compile_sfc(
        "<script setup lang=\"ts\">\nenum E { A = 1!, B = (2 as const) }\ndefineOptions({ y: E.B })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "non-null / parenthesized TS-wrapped enum members must stay valid, got: {:?}",
        result.errors
    );
}

#[test]
fn r5_fix1_enum_non_static_ts_wrapped_member_still_rejected() {
    // `foo() as const` unwraps to a CALL expression — NOT static — so the enum
    // is setup-const and referencing it in defineOptions is rejected. This is
    // the negative guard proving the unwrap does not over-accept.
    let result = compile_sfc(
        "<script setup lang=\"ts\">\nfunction foo(): number { return 1 }\nenum E { A = foo() as const }\ndefineOptions({ x: E.A })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(D1_OFFICIAL_MESSAGE)),
        "non-static TS-wrapped enum member must keep the enum setup-const (rejected), got: {:?}",
        result.errors
    );
}

// =========================================================================
// R5-FIX2 — `classify_const_init` static-node forms are `LiteralConst`
// =========================================================================
//
// Official `walkDeclaration` marks a const `literal-const` when
// `isStaticNode(unwrapTSNode(init))` — covering static unary/binary/logical
// compositions, expression-free template literals, and TS-unwrapped scalars.
// Object/array literals are NOT static (kept setup-const, `canNeverBeRef`).

#[test]
fn r5_fix2_static_compositions_are_literal_const() {
    for init in [
        "const K = 1 + 2",      // binary of literals
        "const K = -1",         // unary of a literal
        "const K = `x`",        // template literal, no expressions
        "const K = !true",      // unary of a boolean literal
        "const K = 5 as const", // TS-wrapped scalar
        "const K = 1 + 2 * 3",  // nested static binary
    ] {
        let src = format!(
            "<script setup lang=\"ts\">\n{init}\ndefineOptions({{ x: K }})\n</script>\n<template><div>x</div></template>"
        );
        let result = compile_sfc(&src);
        assert!(
            !result
                .errors
                .iter()
                .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
            "static const `{init}` must be literal-const (valid in defineOptions), got: {:?}",
            result.errors
        );
    }
}

#[test]
fn r5_fix2_object_and_array_literals_stay_rejected() {
    // Official isStaticNode is false for object/array expressions → setup-const
    // → rejected in defineOptions. The unwrap/static widening must NOT leak here.
    for init in ["const o = { a: 1 }", "const o = [1, 2]"] {
        let src = format!(
            "<script setup lang=\"ts\">\n{init}\ndefineOptions({{ x: o }})\n</script>\n<template><div>x</div></template>"
        );
        let result = compile_sfc(&src);
        assert!(
            result.errors.iter().any(|d| d.severity
                == crate::compile::CompileDiagnosticSeverity::Error
                && d.message.contains(D1_OFFICIAL_MESSAGE)),
            "object/array const `{init}` must stay setup-const (rejected), got: {:?}",
            result.errors
        );
    }
}

#[test]
fn r5_fix3_nested_validator_function_local_stays_valid() {
    // A nested validator function param (`f`) shadowing the setup binding name
    // is a function-scope local — official does NOT reject it. The reference
    // `f > 0` resolves to the param, not the setup ref.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\ndefineProps({ x: { validator: (f) => f > 0 } })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "a nested validator function param shadowing a setup name must stay valid, got: {:?}",
        result.errors
    );
}

#[test]
fn with_defaults_local_ref_default_is_compile_error() {
    // Type-based defineProps + runtime defaults with a setup-local reference —
    // official reports it under `defineProps()`.
    let result = compile_sfc(
        "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst dft = ref('x')\nwithDefaults(defineProps<{ x?: string }>(), { x: dft.value })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineProps()` in <script setup> cannot reference locally declared variables"
            )),
        "withDefaults defaults referencing a setup-local must be rejected (official), got: {:?}",
        result.errors
    );
}

/// A loop variable the `for` head DECLARES is local and must never be prefixed
/// or recorded as a binding — including in the test, the update, and the body.
/// This is the boundary the expression-init arm must not cross: resolving
/// `for (i = 0; …)` must not start resolving `for (let i = 0; …)`.
#[test]
fn a_declared_for_loop_variable_stays_local() {
    for keyword in ["let", "var", "const"] {
        // `const` cannot be updated, so give it a body-only loop.
        let head = if keyword == "const" {
            "for (const i of arr) log(i)".to_string()
        } else {
            format!("for ({keyword} i = 0; i < n; i++) log(i)")
        };
        let source = format!(
            r#"<script setup>
import {{ ref }} from 'vue'
const n = ref(3)
const arr = ref([1, 2])
const log = (x) => x
</script>
<template><button @click="{head}">x</button></template>"#
        );
        let code = compile_and_validate_template(&source);
        // Only the genuine setup bindings resolve.
        assert!(
            code.contains("$setup.log(i)"),
            "[{keyword}] the setup binding must resolve, got:\n{code}"
        );
        // Negative: the declared loop variable is local in EVERY position.
        for forbidden in ["$setup.i", "_ctx.i", "i.value"] {
            assert!(
                !code.contains(forbidden),
                "[{keyword}] a declared loop variable must stay local, found {forbidden:?} in:\n{code}"
            );
        }
        // Same in inline mode, where prefixing a loop variable would emit a
        // reference to a binding that does not exist.
        let inline = compile_and_validate_inline_script(&source);
        assert!(
            inline.contains("log(i)"),
            "[{keyword}] inline must keep the loop variable bare, got:\n{inline}"
        );
        assert!(
            !inline.contains("i.value"),
            "[{keyword}] a declared loop variable is not a ref, got:\n{inline}"
        );
    }
}

// =========================================================================
// ONE notion of "multi-statement". Every backend's statement-body-vs-paren
// decision reads the PARSE FACT (`OxcParsedExpression::multi_statement`); the
// raw-source `value.contains(';')` probes are gone. A `;` scan disagrees with
// the parse in both directions: a newline-separated list has no `;`, and a `;`
// inside a string literal is not a statement boundary.
// =========================================================================

/// A newline-separated handler is two statements with no `;` anywhere. A `;`
/// scan reads it as one expression and emits `$event => (count++⏎emit(…))`,
/// which does not parse. VDOM and Vapor must agree, and both must parse.
#[test]
fn newline_separated_handler_gets_a_statement_body_in_every_backend() {
    let source = "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\nconst emit = defineEmits(['change'])\n</script>\n<template><button @click=\"count++\nemit('change')\">x</button></template>";
    // Each helper asserts the emitted JS parses.
    let vdom = compile_and_validate_template(source);
    assert!(
        vdom.contains("$event => {$setup.count++"),
        "VDOM must give a newline-separated list a statement body, got:\n{vdom}"
    );
    let vapor = compile_and_validate_vapor_template(source);
    assert!(
        vapor.contains("() => { _ctx.count++"),
        "Vapor must give a newline-separated list a statement body -- and \
         omit the unused $event param (verified against the real compiler), \
         got:\n{vapor}"
    );
    let inline = compile_and_validate_inline_script(source);
    assert!(
        inline.contains("$event => {count.value++"),
        "inline must give a newline-separated list a statement body, got:\n{inline}"
    );
    for (backend, code) in [("vdom", &vdom), ("vapor", &vapor), ("inline", &inline)] {
        assert!(
            !code.contains("$event => ("),
            "{backend} must not put a statement list in an expression container, got:\n{code}"
        );
    }
}

/// A statement list whose text contains a top-level `=>` must not be read as a
/// bare function-expression handler. The text probe finds the arrow and emits
/// the list unwrapped into the props object literal, which does not parse.
#[test]
fn statement_list_containing_an_arrow_is_not_a_bare_function_handler() {
    let source = r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
const b = ref(null)
</script>
<template><button @click="a = 1; b = () => 2">x</button></template>"#;
    // The helper's OXC parse is the discriminator: unwrapped, this emits
    // `onClick: $setup.a = 1; $setup.b = () => 2` inside an object literal.
    let vdom = compile_and_validate_template(source);
    assert!(
        vdom.contains("$event => {$setup.a = 1; $setup.b = () => 2}"),
        "a statement list must keep its statement body, got:\n{vdom}"
    );
    assert!(
        !vdom.contains("onClick: $setup.a = 1"),
        "the statement list must not be emitted unwrapped, got:\n{vdom}"
    );
    let inline = compile_and_validate_inline_script(source);
    assert!(
        inline.contains("$event => {a.value = 1; b.value = () => 2}"),
        "inline must wrap it too, got:\n{inline}"
    );
}

/// A value the STATEMENT grammar cannot read is still not a single expression —
/// that is why it took the statement path. `a = 1; return` is an illegal
/// top-level `return` as a Program, yet is valid as the emitted arrow's BODY,
/// so it must keep the statement container rather than fall into `(…)`.
#[test]
fn handler_the_statement_grammar_rejects_still_gets_a_statement_body() {
    let source = r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
</script>
<template><button @click="a = 1; return">x</button></template>"#;
    let vdom = compile_and_validate_template(source);
    assert!(
        vdom.contains("$event => {a = 1; return}"),
        "VDOM must keep the statement body, got:\n{vdom}"
    );
    let vapor = compile_and_validate_vapor_template(source);
    assert!(
        vapor.contains("() => { a = 1; return }"),
        "Vapor must keep the statement body -- and omit the unused $event \
         param, got:\n{vapor}"
    );
}
