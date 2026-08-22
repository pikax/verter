use super::script::{
    generate_tsc_output_with_options, FallthroughPropsProjection, MacroTscInput, TscGenOptions,
    TscMode,
};
use oxc_sourcemap::SourceMap;
use verter_macro_dto::{
    AuthoredMemberOrdinal, MacroAnchor, MacroFailure, MacroInvalidReason, MacroPartialReason,
    MacroTscBundle, MacroTscEntry, MacroTscOutcome, MacroTscProjection, SynthesizedRowKind,
    TscBindingUsage, TscDeclarationFailureReason, TscDependencyDeclaration, TscEmitRow,
    TscEmitsProjection, TscExposeMemberRow, TscExposeMemberType, TscExposeProjection,
    TscInferredClassMember, TscInferredClassTypePosition, TscModelProjection,
    TscOwnerValueDependency, TscPropRow, TscPropsProjection, TscPublicPropsProjection,
    TscRetainedBinding, TscRetainedValueCarrier, TscScopeRequirements, TscScriptOwner,
    TscSemanticInferenceUnavailableReason, TscSpliceText, UnresolvedReason, UnsupportedReason,
};

enum TscFixture<'a> {
    Props {
        syntax_index: u32,
        entry_macro_index: u32,
        rows: Vec<FixturePropRow<'a>>,
        imports: &'a [&'a str],
        declarations: &'a [(&'a str, &'a str)],
    },
    Emits {
        syntax_index: u32,
        entry_macro_index: u32,
        events: Vec<FixtureEmitRow<'a>>,
        imports: &'a [&'a str],
        declarations: &'a [(&'a str, &'a str)],
    },
    Model {
        syntax_index: u32,
        entry_macro_index: u32,
        anchor_macro_index: u32,
        name: &'a str,
        optional: bool,
        value_type: &'a str,
        imports: &'a [&'a str],
        declarations: &'a [(&'a str, &'a str)],
    },
}

#[derive(Clone, Copy)]
struct FixturePropRow<'a> {
    name: &'a str,
    optional: bool,
    type_text: &'a str,
    anchor: MacroAnchor,
}

#[derive(Clone, Copy)]
struct FixtureEmitRow<'a> {
    name: &'a str,
    emit_parameters: &'a str,
    handler_parameters: &'a str,
    anchor: MacroAnchor,
}

const fn authored_prop<'a>(
    name: &'a str,
    optional: bool,
    type_text: &'a str,
    macro_index: u32,
    member_ordinal: u32,
) -> FixturePropRow<'a> {
    FixturePropRow {
        name,
        optional,
        type_text,
        anchor: MacroAnchor::Authored {
            macro_index,
            member_ordinal: AuthoredMemberOrdinal::new(member_ordinal),
        },
    }
}

const fn root_prop<'a>(
    name: &'a str,
    optional: bool,
    type_text: &'a str,
    macro_index: u32,
) -> FixturePropRow<'a> {
    FixturePropRow {
        name,
        optional,
        type_text,
        anchor: MacroAnchor::MacroArgument { macro_index },
    }
}

const fn authored_emit<'a>(
    name: &'a str,
    emit_parameters: &'a str,
    handler_parameters: &'a str,
    macro_index: u32,
    member_ordinal: u32,
) -> FixtureEmitRow<'a> {
    FixtureEmitRow {
        name,
        emit_parameters,
        handler_parameters,
        anchor: MacroAnchor::Authored {
            macro_index,
            member_ordinal: AuthoredMemberOrdinal::new(member_ordinal),
        },
    }
}

const fn root_emit<'a>(
    name: &'a str,
    emit_parameters: &'a str,
    handler_parameters: &'a str,
    macro_index: u32,
) -> FixtureEmitRow<'a> {
    FixtureEmitRow {
        name,
        emit_parameters,
        handler_parameters,
        anchor: MacroAnchor::MacroArgument { macro_index },
    }
}

fn props_fixture<'a>(public_type: &'a str, rows: &[FixturePropRow<'a>]) -> TscFixture<'a> {
    props_fixture_at(0, public_type, rows, &[], &[])
}

fn props_fixture_at<'a>(
    syntax_index: u32,
    _public_type: &'a str,
    rows: &[FixturePropRow<'a>],
    imports: &'a [&'a str],
    declarations: &'a [(&'a str, &'a str)],
) -> TscFixture<'a> {
    props_fixture_identity(syntax_index, syntax_index, rows, imports, declarations)
}

#[allow(clippy::too_many_arguments)]
fn props_fixture_identity<'a>(
    syntax_index: u32,
    entry_macro_index: u32,
    rows: &[FixturePropRow<'a>],
    imports: &'a [&'a str],
    declarations: &'a [(&'a str, &'a str)],
) -> TscFixture<'a> {
    TscFixture::Props {
        syntax_index,
        entry_macro_index,
        rows: rows.to_vec(),
        imports,
        declarations,
    }
}

fn props_root_fixture_at<'a>(
    syntax_index: u32,
    _public_type: &'a str,
    rows: &[FixturePropRow<'a>],
    imports: &'a [&'a str],
    declarations: &'a [(&'a str, &'a str)],
) -> TscFixture<'a> {
    props_fixture_identity(syntax_index, syntax_index, rows, imports, declarations)
}

fn emits_fixture<'a>(events: &[FixtureEmitRow<'a>]) -> TscFixture<'a> {
    emits_fixture_at(0, events, &[], &[])
}

fn emits_fixture_at<'a>(
    syntax_index: u32,
    events: &[FixtureEmitRow<'a>],
    imports: &'a [&'a str],
    declarations: &'a [(&'a str, &'a str)],
) -> TscFixture<'a> {
    emits_fixture_identity(syntax_index, syntax_index, events, imports, declarations)
}

fn emits_fixture_identity<'a>(
    syntax_index: u32,
    entry_macro_index: u32,
    events: &[FixtureEmitRow<'a>],
    imports: &'a [&'a str],
    declarations: &'a [(&'a str, &'a str)],
) -> TscFixture<'a> {
    TscFixture::Emits {
        syntax_index,
        entry_macro_index,
        events: events.to_vec(),
        imports,
        declarations,
    }
}

fn model_fixture<'a>(name: &'a str, value_type: &'a str) -> TscFixture<'a> {
    model_fixture_at(0, name, value_type, &[], &[])
}

fn model_fixture_at<'a>(
    syntax_index: u32,
    name: &'a str,
    value_type: &'a str,
    imports: &'a [&'a str],
    declarations: &'a [(&'a str, &'a str)],
) -> TscFixture<'a> {
    model_fixture_identity(
        syntax_index,
        syntax_index,
        syntax_index,
        name,
        value_type,
        imports,
        declarations,
    )
}

#[allow(clippy::too_many_arguments)]
fn model_fixture_identity<'a>(
    syntax_index: u32,
    entry_macro_index: u32,
    anchor_macro_index: u32,
    name: &'a str,
    value_type: &'a str,
    imports: &'a [&'a str],
    declarations: &'a [(&'a str, &'a str)],
) -> TscFixture<'a> {
    TscFixture::Model {
        syntax_index,
        entry_macro_index,
        anchor_macro_index,
        name,
        optional: true,
        value_type,
        imports,
        declarations,
    }
}

fn fixture_scope(imports: &[&str], declarations: &[(&str, &str)]) -> TscScopeRequirements {
    TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: imports
            .iter()
            .map(|name| TscRetainedBinding {
                owner: TscScriptOwner::Setup,
                local_name: (*name).to_owned(),
                usage: TscBindingUsage::TypePosition,
            })
            .collect(),
        dependency_declarations: declarations
            .iter()
            .enumerate()
            .map(|(index, (name, _declaration))| TscDependencyDeclaration {
                owner: TscScriptOwner::Setup,
                name: (*name).to_owned(),
                contributor_ordinal: declarations[..index]
                    .iter()
                    .filter(|(previous, _)| previous == name)
                    .count() as u32,
                owner_value_dependencies: Vec::new(),
                retained_value_carriers: Vec::new(),
                declaration_failure: None,
                inferred_class_members: Vec::new(),
            })
            .collect(),
    }
}

fn fixture_bundle(fixtures: &[TscFixture<'_>]) -> MacroTscBundle {
    MacroTscBundle {
        entries: fixtures
            .iter()
            .map(|fixture| match fixture {
                TscFixture::Props {
                    syntax_index,
                    entry_macro_index,
                    rows,
                    imports,
                    declarations,
                } => MacroTscEntry {
                    syntax_index: *syntax_index,
                    macro_index: *entry_macro_index,
                    outcome: MacroTscOutcome::Complete(MacroTscProjection::Props(
                        TscPropsProjection {
                            public: TscPublicPropsProjection::AuthoredArgument {
                                anchor: MacroAnchor::MacroArgument {
                                    macro_index: *syntax_index,
                                },
                            },
                            testing_rows: rows
                                .iter()
                                .map(|row| TscPropRow {
                                    name: row.name.to_owned(),
                                    optional: row.optional,
                                    type_text: TscSpliceText::new(row.type_text),
                                    anchor: row.anchor,
                                })
                                .collect(),
                            scope: fixture_scope(imports, declarations),
                        },
                    )),
                },
                TscFixture::Emits {
                    syntax_index,
                    entry_macro_index,
                    events,
                    imports,
                    declarations,
                } => MacroTscEntry {
                    syntax_index: *syntax_index,
                    macro_index: *entry_macro_index,
                    outcome: MacroTscOutcome::Complete(MacroTscProjection::Emits(
                        TscEmitsProjection {
                            events: events
                                .iter()
                                .map(|row| TscEmitRow {
                                    name: row.name.to_owned(),
                                    emit_parameters: TscSpliceText::new(row.emit_parameters),
                                    handler_parameters: TscSpliceText::new(row.handler_parameters),
                                    anchor: row.anchor,
                                })
                                .collect(),
                            scope: fixture_scope(imports, declarations),
                        },
                    )),
                },
                TscFixture::Model {
                    syntax_index,
                    entry_macro_index,
                    anchor_macro_index,
                    name,
                    optional,
                    value_type,
                    imports,
                    declarations,
                } => MacroTscEntry {
                    syntax_index: *syntax_index,
                    macro_index: *entry_macro_index,
                    outcome: MacroTscOutcome::Complete(MacroTscProjection::Model(
                        TscModelProjection {
                            name: (*name).to_owned(),
                            optional: *optional,
                            value_type: TscSpliceText::new(*value_type),
                            anchor: MacroAnchor::Synthesized {
                                macro_index: *anchor_macro_index,
                                row: SynthesizedRowKind::ModelProp,
                            },
                            scope: fixture_scope(imports, declarations),
                        },
                    )),
                },
            })
            .collect(),
    }
}

fn props_bundle_with_scope(scope: TscScopeRequirements) -> MacroTscBundle {
    MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Props(TscPropsProjection {
                public: TscPublicPropsProjection::AuthoredArgument {
                    anchor: MacroAnchor::MacroArgument { macro_index: 0 },
                },
                testing_rows: Vec::new(),
                scope,
            })),
        }],
    }
}

fn gen_tsc_with(sfc: &str, fixtures: &[TscFixture<'_>]) -> String {
    gen_tsc_output_with(sfc, fixtures).code
}

fn gen_tsc_output_with(sfc: &str, fixtures: &[TscFixture<'_>]) -> super::script::TscOutput {
    gen_tsc_mode_with(sfc, TscMode::Public, fixtures)
}

fn gen_tsc_mode_with(
    sfc: &str,
    mode: TscMode,
    fixtures: &[TscFixture<'_>],
) -> super::script::TscOutput {
    let bundle = fixture_bundle(fixtures);
    generate_tsc_output_with_options(
        sfc,
        "TestComp",
        &TscGenOptions {
            filename: Some("/test/TestComp.vue".to_string()),
            mode,
            ..Default::default()
        },
        MacroTscInput::Authoritative(&bundle),
        &FallthroughPropsProjection::none(),
    )
    .expect("explicit TSC fixture must match typed macro syntax")
}

const fn macro_subject(syntax_index: u32) -> super::script::TscFailureSubject {
    super::script::TscFailureSubject::Macro { syntax_index }
}

#[test]
fn direct_tsc_rejects_typed_macro_without_authoritative_semantics() {
    let error = generate_tsc_output_with_options(
        r#"<script setup lang="ts">defineProps<{ value: string }>()</script>"#,
        "TestComp",
        &TscGenOptions::default(),
        MacroTscInput::NotRequired,
        &FallthroughPropsProjection::none(),
    )
    .expect_err("typed macro must reject NotRequired");

    assert_eq!(
        error,
        super::script::TscGenerationError::MissingAuthoritativeSemantics {
            subject: macro_subject(0),
        }
    );
}

fn generate_with_bundle(
    source: &str,
    mode: TscMode,
    bundle: &MacroTscBundle,
) -> Result<super::script::TscOutput, super::script::TscGenerationError> {
    generate_tsc_output_with_options(
        source,
        "TestComp",
        &TscGenOptions {
            filename: Some("/test/TestComp.vue".to_owned()),
            mode,
            ..Default::default()
        },
        MacroTscInput::Authoritative(bundle),
        &FallthroughPropsProjection::none(),
    )
}

fn dependency(
    name: &str,
    inferred_class_members: Vec<TscInferredClassMember>,
) -> TscDependencyDeclaration {
    dependency_for(TscScriptOwner::Setup, name, inferred_class_members)
}

fn dependency_for(
    owner: TscScriptOwner,
    name: &str,
    inferred_class_members: Vec<TscInferredClassMember>,
) -> TscDependencyDeclaration {
    TscDependencyDeclaration {
        owner,
        name: name.to_owned(),
        contributor_ordinal: 0,
        owner_value_dependencies: Vec::new(),
        retained_value_carriers: Vec::new(),
        declaration_failure: None,
        inferred_class_members,
    }
}

fn inferred_class_member(
    name: &str,
    occurrence: u32,
    is_static: bool,
    position: TscInferredClassTypePosition,
    type_text: &str,
) -> TscInferredClassMember {
    TscInferredClassMember {
        name: name.to_owned(),
        occurrence,
        is_static,
        position,
        type_text: TscSpliceText::new(type_text),
    }
}

#[test]
fn authoritative_tsc_bundle_is_a_closed_exact_join() {
    let source = r#"<script setup lang="ts">defineProps<{ value: string }>()</script>"#;
    let valid = fixture_bundle(&[props_fixture("", &[])]);

    assert_eq!(
        generate_with_bundle(source, TscMode::Public, &MacroTscBundle::default()).unwrap_err(),
        super::script::TscGenerationError::MissingEntry {
            subject: macro_subject(0),
        }
    );

    let mut duplicate = valid.clone();
    duplicate.entries.push(duplicate.entries[0].clone());
    assert_eq!(
        generate_with_bundle(source, TscMode::Public, &duplicate).unwrap_err(),
        super::script::TscGenerationError::DuplicateEntry {
            subject: macro_subject(0),
        }
    );

    let wrong_role = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Emits(TscEmitsProjection {
                events: Vec::new(),
                scope: TscScopeRequirements::default(),
            })),
        }],
    };
    assert_eq!(
        generate_with_bundle(source, TscMode::Public, &wrong_role).unwrap_err(),
        super::script::TscGenerationError::RoleMismatch {
            subject: macro_subject(0),
        }
    );

    let unavailable_cases = [
        (
            MacroTscOutcome::Partial(MacroFailure::new(
                MacroPartialReason::IncompleteTraversal,
                Some("partial detail".to_owned()),
            )),
            super::script::TscUnavailableOutcome::Partial(MacroFailure::new(
                MacroPartialReason::IncompleteTraversal,
                Some("partial detail".to_owned()),
            )),
        ),
        (
            MacroTscOutcome::Unresolved(MacroFailure::new(
                UnresolvedReason::AmbiguousReference,
                Some("unresolved detail".to_owned()),
            )),
            super::script::TscUnavailableOutcome::Unresolved(MacroFailure::new(
                UnresolvedReason::AmbiguousReference,
                Some("unresolved detail".to_owned()),
            )),
        ),
        (
            MacroTscOutcome::Unsupported(MacroFailure::new(
                UnsupportedReason::SemanticConstruct,
                Some("unsupported detail".to_owned()),
            )),
            super::script::TscUnavailableOutcome::Unsupported(MacroFailure::new(
                UnsupportedReason::SemanticConstruct,
                Some("unsupported detail".to_owned()),
            )),
        ),
        (
            MacroTscOutcome::Invalid(MacroFailure::new(
                MacroInvalidReason::NonObjectRoot,
                Some("invalid detail".to_owned()),
            )),
            super::script::TscUnavailableOutcome::Invalid(super::script::TscInvalidOutcome::Macro(
                MacroFailure::new(
                    MacroInvalidReason::NonObjectRoot,
                    Some("invalid detail".to_owned()),
                ),
            )),
        ),
    ];
    for (outcome, expected) in unavailable_cases {
        let unavailable = MacroTscBundle {
            entries: vec![MacroTscEntry {
                syntax_index: 0,
                macro_index: 0,
                outcome,
            }],
        };
        assert_eq!(
            generate_with_bundle(source, TscMode::Public, &unavailable).unwrap_err(),
            super::script::TscGenerationError::UnavailableOutcome {
                subject: macro_subject(0),
                outcome: expected,
            }
        );
    }

    let mut extra = valid;
    extra.entries.push(MacroTscEntry {
        syntax_index: 9,
        macro_index: 9,
        outcome: MacroTscOutcome::Unsupported(MacroFailure::new(
            UnsupportedReason::MacroKind,
            None,
        )),
    });
    assert_eq!(
        generate_with_bundle(source, TscMode::Public, &extra).unwrap_err(),
        super::script::TscGenerationError::UnexpectedEntry {
            subject: macro_subject(9),
        }
    );
}

#[test]
fn macro_entries_and_row_anchors_join_parser_owned_identities_and_roles() {
    let with_defaults = r#"<script setup lang="ts">
withDefaults(defineProps<{ value?: string }>(), { value: "x" })
</script>"#;
    let valid = fixture_bundle(&[props_fixture_identity(
        0,
        1,
        &[authored_prop("value", true, "string", 0, 0)],
        &[],
        &[],
    )]);
    generate_with_bundle(with_defaults, TscMode::Testing, &valid)
        .expect("withDefaults payload and effective identities are intentionally distinct");

    let mut wrong_public_payload = fixture_bundle(&[props_fixture_identity(0, 1, &[], &[], &[])]);
    let MacroTscOutcome::Complete(MacroTscProjection::Props(props)) =
        &mut wrong_public_payload.entries[0].outcome
    else {
        unreachable!();
    };
    props.public = TscPublicPropsProjection::AuthoredArgument {
        anchor: MacroAnchor::MacroArgument { macro_index: 1 },
    };
    assert_eq!(
        generate_with_bundle(with_defaults, TscMode::Testing, &wrong_public_payload).unwrap_err(),
        super::script::TscGenerationError::InvalidMacroAnchor {
            subject: macro_subject(0),
        },
        "public authored syntax must retain its exact payload identity even without testing rows"
    );

    let wrong_entry = fixture_bundle(&[props_fixture_identity(
        0,
        0,
        &[authored_prop("value", true, "string", 0, 0)],
        &[],
        &[],
    )]);
    assert_eq!(
        generate_with_bundle(with_defaults, TscMode::Testing, &wrong_entry).unwrap_err(),
        super::script::TscGenerationError::MacroIdentityMismatch {
            subject: macro_subject(0),
        }
    );

    let wrong_payload_anchor = fixture_bundle(&[props_fixture_identity(
        0,
        1,
        &[authored_prop("value", true, "string", 1, 0)],
        &[],
        &[],
    )]);
    assert_eq!(
        generate_with_bundle(with_defaults, TscMode::Testing, &wrong_payload_anchor).unwrap_err(),
        super::script::TscGenerationError::InvalidMacroAnchor {
            subject: macro_subject(0),
        }
    );

    let forged_props_row = FixturePropRow {
        name: "value",
        optional: false,
        type_text: "string",
        anchor: MacroAnchor::Synthesized {
            macro_index: 1,
            row: SynthesizedRowKind::ModelModifiersProp,
        },
    };
    let wrong_kind = fixture_bundle(&[props_fixture_identity(0, 1, &[forged_props_row], &[], &[])]);
    assert_eq!(
        generate_with_bundle(with_defaults, TscMode::Testing, &wrong_kind).unwrap_err(),
        super::script::TscGenerationError::InvalidMacroAnchor {
            subject: macro_subject(0),
        }
    );

    let model_source = r#"<script setup lang="ts">defineModel<string>('title')</script>"#;
    let wrong_model_row =
        fixture_bundle(&[model_fixture_identity(0, 0, 0, "title", "string", &[], &[])]);
    let mut wrong_model_row = wrong_model_row;
    let MacroTscOutcome::Complete(MacroTscProjection::Model(model)) =
        &mut wrong_model_row.entries[0].outcome
    else {
        unreachable!()
    };
    model.anchor = MacroAnchor::Synthesized {
        macro_index: 0,
        row: SynthesizedRowKind::ModelUpdateEvent,
    };
    assert_eq!(
        generate_with_bundle(model_source, TscMode::Public, &wrong_model_row).unwrap_err(),
        super::script::TscGenerationError::InvalidMacroAnchor {
            subject: macro_subject(0),
        }
    );
}

#[test]
fn no_typed_macro_rejects_every_authoritative_extra_on_all_early_paths() {
    let extra = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 7,
            macro_index: 7,
            outcome: MacroTscOutcome::Unsupported(MacroFailure::new(
                UnsupportedReason::MacroKind,
                None,
            )),
        }],
    };
    for source in [
        "<template />",
        "<script setup lang=\"ts\"></script>",
        "<script>export default {}</script>",
    ] {
        assert_eq!(
            generate_with_bundle(source, TscMode::Public, &extra).unwrap_err(),
            super::script::TscGenerationError::UnexpectedEntry {
                subject: macro_subject(7),
            },
            "source: {source}"
        );
    }
}

#[test]
fn local_class_carriers_follow_the_owner_body_mode_matrix() {
    let source = r#"<script setup lang="ts">
class Payload { value = 1 }
defineProps<Payload>()
</script>"#;
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![dependency(
            "Payload",
            vec![inferred_class_member(
                "value",
                0,
                false,
                TscInferredClassTypePosition::Property,
                "number",
            )],
        )],
    });

    let testing = generate_with_bundle(source, TscMode::Testing, &bundle).unwrap();
    assert!(testing.code.contains("class Payload { value = 1 }"));
    assert!(!testing.code.contains("declare class Payload"));

    for mode in [TscMode::Public, TscMode::Declaration] {
        let output = generate_with_bundle(source, mode, &bundle).unwrap();
        assert!(output
            .code
            .contains("declare class Payload { value: number }"));
        assert!(!output.code.contains("value = 1"));
    }

    let exposed = source.replace(
        "defineProps<Payload>()",
        "defineProps<Payload>()\nconst exposed = 1\ndefineExpose({ exposed })",
    );
    let public = generate_with_bundle(&exposed, TscMode::Public, &bundle).unwrap();
    assert!(public.code.contains("class Payload { value = 1 }"));
    assert!(!public.code.contains("declare class Payload"));
}

#[test]
fn companion_class_carrier_is_declaration_safe_in_every_mode() {
    let source = r#"<script lang="ts">class Companion { value = 1 }</script>
<script setup lang="ts">defineProps<Companion>()</script>"#;
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![dependency_for(
            TscScriptOwner::Companion,
            "Companion",
            vec![inferred_class_member(
                "value",
                0,
                false,
                TscInferredClassTypePosition::Property,
                "number",
            )],
        )],
    });

    for mode in [TscMode::Testing, TscMode::Public, TscMode::Declaration] {
        let output = generate_with_bundle(source, mode, &bundle).unwrap();
        assert!(output
            .code
            .contains("declare class Companion { value: number }"));
        assert!(!output.code.contains("value = 1"));
    }
}

#[test]
fn owner_value_dependencies_are_rejected_only_when_the_owner_body_is_omitted() {
    let source = r#"<script setup lang="ts">
const seed = { value: "x" }
type Props = { value: typeof seed }
defineProps<Props>()
</script>"#;
    let mut declaration = dependency("Props", Vec::new());
    declaration.owner_value_dependencies = vec![TscOwnerValueDependency {
        owner: TscScriptOwner::Setup,
        name: "seed".to_owned(),
    }];
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![declaration],
    });

    generate_with_bundle(source, TscMode::Testing, &bundle).unwrap();
    for mode in [TscMode::Public, TscMode::Declaration] {
        assert_eq!(
            generate_with_bundle(source, mode, &bundle).unwrap_err(),
            super::script::TscGenerationError::UnsupportedDeclarationShape {
                subject: macro_subject(0),
                reason: super::script::TscDeclarationShapeReason::OwnerValueDependencyUnavailable,
            }
        );
    }

    let exposed = source.replace(
        "defineProps<Props>()",
        "defineProps<Props>()\nconst exposed = 1\ndefineExpose({ exposed })",
    );
    generate_with_bundle(&exposed, TscMode::Public, &bundle).unwrap();
}

#[test]
fn exact_dual_space_value_carriers_are_validated_across_body_modes_and_owners() {
    let source = r#"<script lang="ts">
class Base {}
</script>
<script setup lang="ts">
class Base {}
enum Kind { Ready }
class Payload { ctor = Base; kind = Kind }
defineProps<Payload>()
</script>"#;
    let mut payload = dependency(
        "Payload",
        vec![
            inferred_class_member(
                "ctor",
                0,
                false,
                TscInferredClassTypePosition::Property,
                "typeof Base",
            ),
            inferred_class_member(
                "kind",
                0,
                false,
                TscInferredClassTypePosition::Property,
                "typeof Kind",
            ),
        ],
    );
    payload.retained_value_carriers = vec![
        TscRetainedValueCarrier {
            owner: TscScriptOwner::Setup,
            name: "Base".to_owned(),
            contributor_ordinal: 0,
        },
        TscRetainedValueCarrier {
            owner: TscScriptOwner::Setup,
            name: "Kind".to_owned(),
            contributor_ordinal: 0,
        },
    ];
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![
            dependency("Base", Vec::new()),
            dependency("Kind", Vec::new()),
            payload.clone(),
        ],
    });

    let testing = generate_with_bundle(source, TscMode::Testing, &bundle).unwrap();
    assert!(testing
        .code
        .contains("class Payload { ctor = Base; kind = Kind }"));
    for mode in [TscMode::Public, TscMode::Declaration] {
        let output = generate_with_bundle(source, mode, &bundle).unwrap();
        assert_generated_tsx_parses(&output.code);
        assert!(
            output.code.contains("declare class Base"),
            "{mode:?}: {}",
            output.code
        );
        assert!(
            output.code.contains("declare enum Kind"),
            "{mode:?}: {}",
            output.code
        );
        assert!(
            output.code.contains("ctor: typeof Base"),
            "{mode:?}: {}",
            output.code
        );
        assert!(
            output.code.contains("kind: typeof Kind"),
            "{mode:?}: {}",
            output.code
        );
    }

    let mut wrong_owner = payload;
    wrong_owner.retained_value_carriers[0].owner = TscScriptOwner::Companion;
    let wrong_owner = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![
            dependency("Base", Vec::new()),
            dependency("Kind", Vec::new()),
            wrong_owner,
        ],
    });
    assert_eq!(
        generate_with_bundle(source, TscMode::Public, &wrong_owner).unwrap_err(),
        super::script::TscGenerationError::MissingScopeDeclaration {
            subject: macro_subject(0),
        }
    );
}

#[test]
fn retained_import_join_is_closed_over_script_owner_and_local_name() {
    let source = r#"<script lang="ts">
import type { Companion as Shared } from './companion'
</script>
<script setup lang="ts">
import type { Setup as Shared } from './setup'
defineProps<Shared>()
</script>"#;

    for (owner, expected) in [
        (
            TscScriptOwner::Setup,
            "import type { Setup as Shared } from './setup'",
        ),
        (
            TscScriptOwner::Companion,
            "import type { Companion as Shared } from './companion'",
        ),
    ] {
        let bundle = props_bundle_with_scope(TscScopeRequirements {
            owner_value_dependencies: Vec::new(),
            retained_bindings: vec![TscRetainedBinding {
                owner,
                local_name: "Shared".to_owned(),
                usage: TscBindingUsage::TypePosition,
            }],
            dependency_declarations: Vec::new(),
        });
        for mode in [TscMode::Public, TscMode::Testing, TscMode::Declaration] {
            let output = generate_with_bundle(source, mode, &bundle).unwrap();
            assert!(
                output.code.contains(expected),
                "mode={mode:?}, owner={owner:?}, output:\n{}",
                output.code
            );
        }
    }
}

#[test]
fn retained_declaration_join_uses_owner_local_contributor_ordinals() {
    let source = r#"<script lang="ts">
interface Props { companionMarker: string }
</script>
<script setup lang="ts">
interface Props { setupMarker: number }
defineProps<Props>()
</script>"#;

    for owner in [TscScriptOwner::Setup, TscScriptOwner::Companion] {
        let bundle = props_bundle_with_scope(TscScopeRequirements {
            owner_value_dependencies: Vec::new(),
            retained_bindings: Vec::new(),
            dependency_declarations: vec![dependency_for(owner, "Props", Vec::new())],
        });
        for mode in [TscMode::Public, TscMode::Testing, TscMode::Declaration] {
            let output = generate_with_bundle(source, mode, &bundle).unwrap();
            let selected = match owner {
                TscScriptOwner::Setup => "setupMarker",
                TscScriptOwner::Companion => "companionMarker",
            };
            assert!(
                output.code.contains(selected),
                "mode={mode:?}, owner={owner:?}, output:\n{}",
                output.code
            );
            if owner == TscScriptOwner::Setup {
                assert!(
                    !output.code.contains("companionMarker"),
                    "owner-local ordinal 0 must not select companion ordinal 0: {}",
                    output.code
                );
            }
        }
    }
}

fn assert_generated_tsx_parses(code: &str) {
    let allocator = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&allocator, code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "generated TSC output must parse as TSX: {:?}\n{code}",
        parsed.errors
    );
}

#[test]
fn enum_carriers_preserve_only_exact_finite_constant_initializers() {
    let source = r#"<script setup lang="ts">
enum Base { Zero, Arithmetic = Zero + 2, Shift = Arithmetic << 1, Text = "x" }
enum Derived { Alias = Base.Shift, Computed = Base["Arithmetic"] + 3 }
const enum ConstKind { Negative = -1, Next = Negative + 3 }
export enum ExportedKind { Copy = Derived.Alias }
enum Merged { First = 1 }
enum Merged { Second = First + 1 }
defineProps<{
  base: Base
  derived: Derived
  constKind: ConstKind
  exported: ExportedKind
  merged: Merged
}>()
</script>"#;
    let mut merged_second = dependency("Merged", Vec::new());
    merged_second.contributor_ordinal = 1;
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![
            dependency("Base", Vec::new()),
            dependency("Derived", Vec::new()),
            dependency("ConstKind", Vec::new()),
            dependency("ExportedKind", Vec::new()),
            dependency("Merged", Vec::new()),
            merged_second,
        ],
    });

    for mode in [TscMode::Public, TscMode::Declaration] {
        let output = generate_with_bundle(source, mode, &bundle).unwrap();
        assert_generated_tsx_parses(&output.code);
        assert!(output.code.contains("declare enum Base"), "{}", output.code);
        assert!(output.code.contains("Arithmetic = 2"), "{}", output.code);
        assert!(output.code.contains("Shift = 4"), "{}", output.code);
        assert!(output.code.contains("Text = \"x\""));
        assert!(
            output.code.contains("declare enum Derived"),
            "{}",
            output.code
        );
        assert!(output.code.contains("Alias = 4"), "{}", output.code);
        assert!(output.code.contains("Computed = 5"), "{}", output.code);
        assert!(
            output.code.contains("declare const enum ConstKind"),
            "{}",
            output.code
        );
        assert!(output.code.contains("Negative = -1"), "{}", output.code);
        assert!(output.code.contains("Next = 2"), "{}", output.code);
        assert!(
            output.code.contains("export declare enum ExportedKind"),
            "{}",
            output.code
        );
        assert!(output.code.contains("Copy = 4"), "{}", output.code);
        assert_eq!(output.code.matches("declare enum Merged").count(), 2);
        assert!(output.code.contains("First = 1"), "{}", output.code);
        assert!(output.code.contains("Second = 2"), "{}", output.code);
    }
    let testing = generate_with_bundle(source, TscMode::Testing, &bundle).unwrap();
    assert_generated_tsx_parses(&testing.code);
    assert!(testing.code.contains("enum Base"));
    assert!(testing.code.contains("Base.Shift"));
    assert!(!testing.code.contains("declare enum Base"));
}

#[test]
fn enum_carriers_reject_unrepresentable_initializers_in_body_omitting_modes() {
    use super::script::{TscDeclarationShapeReason as Reason, TscGenerationError as Error};

    let cases = [
        ("global call", "Math.random()"),
        ("global date call", "Date.now()"),
        ("local call", "seed()"),
        ("imported call", "importedSeed()"),
        ("non-finite NaN", "NaN"),
        ("non-finite Infinity", "Infinity"),
    ];

    for (label, initializer) in cases {
        let source = format!(
            r#"<script setup lang="ts">
import {{ seed as importedSeed }} from "./seed"
const seed = () => 1
enum Payload {{ Value = {initializer} }}
defineProps<Payload>()
</script>"#
        );
        let bundle = props_bundle_with_scope(TscScopeRequirements {
            owner_value_dependencies: Vec::new(),
            retained_bindings: Vec::new(),
            dependency_declarations: vec![dependency("Payload", Vec::new())],
        });

        for mode in [TscMode::Public, TscMode::Declaration] {
            assert_eq!(
                generate_with_bundle(&source, mode, &bundle).unwrap_err(),
                Error::UnsupportedDeclarationShape {
                    subject: macro_subject(0),
                    reason: Reason::UnsupportedEnumShape,
                },
                "case={label}, mode={mode:?}"
            );
        }

        let testing = generate_with_bundle(&source, TscMode::Testing, &bundle).unwrap();
        assert_generated_tsx_parses(&testing.code);
        assert!(
            testing.code.contains(initializer),
            "testing mode must preserve the runtime initializer for {label}: {}",
            testing.code
        );
    }
}

fn declaration_shape_error(class_source: &str) -> super::script::TscGenerationError {
    let source =
        format!("<script setup lang=\"ts\">\n{class_source}\ndefineProps<Payload>()\n</script>");
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![dependency("Payload", Vec::new())],
    });
    generate_with_bundle(&source, TscMode::Declaration, &bundle).unwrap_err()
}

#[test]
fn typeinfo_declaration_failure_codes_are_exact_and_lossless() {
    use super::script::TscDeclarationShapeReason as Reason;

    let cases = [
        (
            TscDeclarationFailureReason::SemanticInferenceUnavailable(
                TscSemanticInferenceUnavailableReason::DepthBudgetExceeded,
            ),
            "semantic-inference-depth-budget-exceeded",
        ),
        (
            TscDeclarationFailureReason::SemanticInferenceUnavailable(
                TscSemanticInferenceUnavailableReason::WorkBudgetExceeded,
            ),
            "semantic-inference-work-budget-exceeded",
        ),
        (
            TscDeclarationFailureReason::Unsupported(UnsupportedReason::MacroKind),
            "semantic-inference-unsupported-macro-kind",
        ),
        (
            TscDeclarationFailureReason::Unsupported(UnsupportedReason::SemanticConstruct),
            "semantic-inference-unsupported-construct",
        ),
        (
            TscDeclarationFailureReason::Unresolved(UnresolvedReason::MissingTypeArgument),
            "semantic-inference-missing-type-argument",
        ),
        (
            TscDeclarationFailureReason::Unresolved(UnresolvedReason::MissingDeclaration),
            "semantic-inference-missing-declaration",
        ),
        (
            TscDeclarationFailureReason::Unresolved(UnresolvedReason::AmbiguousReference),
            "semantic-inference-ambiguous-reference",
        ),
        (
            TscDeclarationFailureReason::Unresolved(UnresolvedReason::MissingDependency),
            "semantic-inference-missing-dependency",
        ),
    ];

    for (failure, expected) in cases {
        let code = Reason::TypeInfoDeclarationFailure(failure).code();
        assert_eq!(code, expected);
        assert_ne!(code, "semantic-inference-unavailable");
    }
}

#[test]
fn typeinfo_declaration_budget_detail_survives_the_compiler_boundary() {
    use super::script::{TscDeclarationShapeReason as Reason, TscGenerationError as Error};

    for detail in [
        TscSemanticInferenceUnavailableReason::DepthBudgetExceeded,
        TscSemanticInferenceUnavailableReason::WorkBudgetExceeded,
    ] {
        let mut declaration = dependency("Payload", Vec::new());
        declaration.declaration_failure = Some(
            TscDeclarationFailureReason::SemanticInferenceUnavailable(detail),
        );
        let bundle = props_bundle_with_scope(TscScopeRequirements {
            owner_value_dependencies: Vec::new(),
            retained_bindings: Vec::new(),
            dependency_declarations: vec![declaration],
        });
        let source = r#"<script setup lang="ts">
class Payload {}
defineProps<Payload>()
</script>"#;

        assert_eq!(
            generate_with_bundle(source, TscMode::Declaration, &bundle).unwrap_err(),
            Error::UnsupportedDeclarationShape {
                subject: macro_subject(0),
                reason: Reason::TypeInfoDeclarationFailure(
                    TscDeclarationFailureReason::SemanticInferenceUnavailable(detail),
                ),
            }
        );
    }
}

#[test]
fn unsupported_class_shapes_have_closed_typed_reasons() {
    use super::script::{TscDeclarationShapeReason as Reason, TscGenerationError as Error};

    let cases = [
        ("@sealed class Payload {}", Reason::ClassDecorator),
        (
            "class Payload extends mixin(Base) {}",
            Reason::ComplexClassHeritage,
        ),
        (
            "class Payload { @dec value = 1 }",
            Reason::DecoratedClassMember,
        ),
        (
            "class Payload { [key]: string }",
            Reason::ComputedClassMember,
        ),
        ("class Payload { #value = 1 }", Reason::PrivateClassMember),
        (
            "class Payload { method(...args: string[]) {} }",
            Reason::RestClassParameter,
        ),
        (
            "class Payload { method({ value }: { value: string }) {} }",
            Reason::DestructuredClassParameter,
        ),
        (
            "class Payload { method(@dec value: string) {} }",
            Reason::DecoratedClassParameter,
        ),
        (
            "class Payload { constructor(value: string); constructor(value: unknown) {} }",
            Reason::ConstructorOverload,
        ),
    ];

    for (source, reason) in cases {
        assert_eq!(
            declaration_shape_error(source),
            Error::UnsupportedDeclarationShape {
                subject: macro_subject(0),
                reason,
            },
            "source: {source}"
        );
    }
}

#[test]
fn supported_class_projection_covers_static_generic_accessors_and_parameter_properties() {
    let source = r#"<script setup lang="ts">
interface Base<T> {}
class Payload<T extends string> implements Base<T> {
  static count: number
  readonly literal = 1
  value = 1
  constructor(public id?: number, protected name = "x") {}
  method(input = 1) { return input }
  get label() { return "x" }
  set label(value: string) {}
}
defineProps<{ payload: Payload<"x"> }>()
</script>"#;
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![
            dependency("Base", Vec::new()),
            dependency(
                "Payload",
                vec![
                    inferred_class_member(
                        "literal",
                        0,
                        false,
                        TscInferredClassTypePosition::Property,
                        "1",
                    ),
                    inferred_class_member(
                        "value",
                        0,
                        false,
                        TscInferredClassTypePosition::Property,
                        "number",
                    ),
                    inferred_class_member(
                        "name",
                        0,
                        false,
                        TscInferredClassTypePosition::Property,
                        "string",
                    ),
                    inferred_class_member(
                        "input",
                        0,
                        false,
                        TscInferredClassTypePosition::Parameter,
                        "number",
                    ),
                    inferred_class_member(
                        "method",
                        0,
                        false,
                        TscInferredClassTypePosition::Return,
                        "number",
                    ),
                    inferred_class_member(
                        "label",
                        0,
                        false,
                        TscInferredClassTypePosition::Return,
                        "string",
                    ),
                ],
            ),
        ],
    });

    let code = generate_with_bundle(source, TscMode::Declaration, &bundle)
        .unwrap()
        .code;
    let compact = code
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    assert!(compact.contains("declareclassPayload<Textendsstring>implementsBase<T>"));
    assert!(compact.contains("staticcount:number"));
    assert!(compact.contains("readonlyliteral:1"));
    assert!(compact.contains("value:number"));
    assert!(compact.contains("id?:number;"));
    assert!(compact.contains("protectedname:string;"));
    assert!(compact.contains("constructor(id?:number,name?:string);"));
    assert!(compact.contains("method(input?:number):number;"));
    assert!(compact.contains("getlabel():string;"));
    assert!(compact.contains("setlabel(value:string);"));
    assert!(!code.contains("return input"));
    assert!(!code.contains("= 1"));
}

#[test]
fn method_overload_implementation_is_removed_without_inference_rows() {
    let source = r#"<script setup lang="ts">
class Payload {
  method(value: string): string
  method(value: number): number
  method(value: string | number) { return value }
}
defineProps<Payload>()
</script>"#;
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![dependency("Payload", Vec::new())],
    });
    let code = generate_with_bundle(source, TscMode::Declaration, &bundle)
        .unwrap()
        .code;
    assert_eq!(code.matches("method(").count(), 2, "{code}");
    assert!(!code.contains("string | number"), "{code}");
    assert!(!code.contains("return value"), "{code}");
}

#[test]
fn ambient_constructor_signatures_are_preserved() {
    let source = r#"<script setup lang="ts">
declare class Payload {
  constructor(value: string)
  constructor(value: number)
}
defineProps<Payload>()
</script>"#;
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![dependency("Payload", Vec::new())],
    });
    let code = generate_with_bundle(source, TscMode::Declaration, &bundle)
        .unwrap()
        .code;
    assert_eq!(code.matches("constructor(").count(), 2, "{code}");
}

#[test]
fn type_and_value_declaration_carriers_satisfy_typeof_value_dependencies() {
    let source = r#"<script setup lang="ts">
class Base {}
type Props = { ctor: typeof Base }
defineProps<Props>()
</script>"#;
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![
            dependency("Base", Vec::new()),
            dependency("Props", Vec::new()),
        ],
    });
    let code = generate_with_bundle(source, TscMode::Declaration, &bundle)
        .unwrap()
        .code;
    assert!(code.contains("declare class Base"));
    assert!(code.contains("typeof Base"));

    let self_source = r#"<script setup lang="ts">
class Payload { peer!: typeof Payload }
defineProps<Payload>()
</script>"#;
    let self_bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![dependency("Payload", Vec::new())],
    });
    let self_code = generate_with_bundle(self_source, TscMode::Declaration, &self_bundle)
        .unwrap()
        .code;
    assert!(self_code.contains("peer: typeof Payload"), "{self_code}");
}

#[test]
fn explicit_props_fixture_drives_testing_rows_and_typed_import_retention() {
    let source = r#"<script setup lang="ts">
import type { External } from './types'
defineProps<External>()
</script>"#;
    let output = gen_tsc_mode_with(
        source,
        TscMode::Testing,
        &[props_root_fixture_at(
            0,
            "External",
            &[root_prop("value", false, "External['value']", 0)],
            &["External"],
            &[],
        )],
    );

    assert!(output
        .code
        .contains("import type { External } from './types'"));
    assert!(output
        .code
        .contains("declare const value: External['value']"));
    assert!(output
        .code
        .contains("$props: import(\"vue\").PublicProps & External"));
}

/// Mutation recipe: synthesize `state.emits_names` from the authoritative TSC
/// event rows. The exact public handler/overload may still pass, but the
/// runtime-`emits` negative assertion must fail because TSC and runtime bundles
/// are independent authorities.
#[test]
fn explicit_emit_fixture_drives_emit_and_handler_parameter_contracts() {
    let source = r#"<script setup lang="ts">defineEmits<{ save: [id: number] }>()</script>"#;
    let bundle = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Emits(TscEmitsProjection {
                events: vec![TscEmitRow {
                    name: "save".to_owned(),
                    emit_parameters: TscSpliceText::new("id: number"),
                    handler_parameters: TscSpliceText::new("payload: number"),
                    anchor: MacroAnchor::MacroArgument { macro_index: 0 },
                }],
                scope: TscScopeRequirements::default(),
            })),
        }],
    };
    let output = generate_tsc_output_with_options(
        source,
        "TestComp",
        &TscGenOptions::default(),
        MacroTscInput::Authoritative(&bundle),
        &FallthroughPropsProjection::none(),
    )
    .expect("explicit emit DTO");

    assert!(output
        .code
        .contains(r#"((event: "save", id: number) => void)"#));
    assert!(output.code.contains("(payload: number) => void"));
    let comp = output.code.split("declare const TestComp").next().unwrap();
    assert!(
        !comp.contains("emits: ["),
        "type-only TSC rows must not synthesize runtime emits: {comp}"
    );
}

fn gen_tsc(sfc: &str) -> String {
    gen_tsc_output(sfc).code
}

fn gen_tsc_props(sfc: &str) -> String {
    gen_tsc_with(sfc, &[props_fixture("", &[])])
}

fn gen_tsc_narrowing_props(sfc: &str, rows: &[FixturePropRow<'_>]) -> String {
    gen_tsc_narrowing_with(sfc, &[props_fixture("", rows)])
}

fn gen_tsc_output(sfc: &str) -> super::script::TscOutput {
    generate_tsc_output_with_options(
        sfc,
        "TestComp",
        &TscGenOptions {
            filename: Some("/test/TestComp.vue".to_string()),
            ..Default::default()
        },
        MacroTscInput::NotRequired,
        &FallthroughPropsProjection::none(),
    )
    .expect("fixture has no typed codegen macro")
}

fn gen_tsc_narrowing_with(sfc: &str, fixtures: &[TscFixture<'_>]) -> String {
    let bundle = fixture_bundle(fixtures);
    generate_tsc_output_with_options(
        sfc,
        "TestComp",
        &TscGenOptions {
            conditional_root_narrowing: true,
            ..Default::default()
        },
        MacroTscInput::Authoritative(&bundle),
        &FallthroughPropsProjection::none(),
    )
    .expect("explicit TSC fixture must match typed macro syntax")
    .code
}

fn gen_tsc_testing(sfc: &str) -> String {
    gen_tsc_output_testing(sfc).code
}

fn gen_tsc_output_testing(sfc: &str) -> super::script::TscOutput {
    generate_tsc_output_with_options(
        sfc,
        "TestComp",
        &TscGenOptions {
            filename: Some("/test/TestComp.vue".to_string()),
            mode: TscMode::Testing,
            ..Default::default()
        },
        MacroTscInput::NotRequired,
        &FallthroughPropsProjection::none(),
    )
    .expect("fixture has no typed codegen macro")
}

fn offset_to_zero_based_line_col(text: &str, offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;
    for ch in text[..offset].chars() {
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

// ── defineProps<ImportedType>() — type-only, no runtime props ────────────────

#[test]
fn tsc_codegen_type_only_props_inlined_in_declare() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script><template><div>hello</div></template>"#,
        &[props_root_fixture_at(0, "Props", &[], &["Props"], &[])],
    );

    assert!(
        r.contains("import type { Props } from './types'"),
        "type import statement emitted"
    );
    assert!(r.contains("defineComponent("), "defineComponent present");
    assert!(!r.contains("props: {"), "no runtime props for type-only");
    assert!(
        r.contains("$props: import(\"vue\").PublicProps & Props"),
        "type name in new()"
    );
    assert!(r.contains("PublicProps"), "PublicProps in constructor");
    assert!(
        !r.contains("import('./types').Props"),
        "should not use inline import() syntax"
    );
    assert!(r.contains("export default"), "export default present");
    assert!(r.contains("sourceMappingURL"), "source map present");
    assert!(!r.contains("___VERTER___"), "no IDE wrapper");
    assert!(!r.contains("setup("), "no setup() in __comp");
    assert!(!r.contains("__verter_"), "no intermediate aliases");
}

// @ai-generated - Testing mode exposes internal script-setup bindings on the instance.
#[test]
fn tsc_testing_mode_exposes_local_script_setup_bindings_on_instance() {
    let public = gen_tsc(
        r#"<script setup lang="ts">
const count = 1
const label = 'hello'
</script><template><div>{{ count }} {{ label }}</div></template>"#,
    );
    let testing = gen_tsc_testing(
        r#"<script setup lang="ts">
const count = 1
const label = 'hello'
</script><template><div>{{ count }} {{ label }}</div></template>"#,
    );

    assert!(
        testing.contains("type __Verter_TestBindings = import(\"vue\").ShallowUnwrapRef<{"),
        "testing mode should emit a debug binding helper: {testing}"
    );
    assert!(
        testing.contains("count: typeof count"),
        "testing mode should expose count on the instance: {testing}"
    );
    assert!(
        testing.contains("label: typeof label"),
        "testing mode should expose label on the instance: {testing}"
    );
    assert!(
        !testing.contains("ref: typeof ref"),
        "value imports must not become instance bindings: {testing}"
    );
    assert!(
        !public.contains("count: typeof count"),
        "public mode must keep script-setup bindings hidden: {public}"
    );
}

// @ai-generated - Testing mode mirrors VTU wrapper.vm shallow ref unwrapping.
#[test]
fn tsc_testing_mode_unwraps_ref_like_bindings_on_instance() {
    let testing = gen_tsc_testing(
        r#"<script setup lang="ts">
import { computed, ref } from 'vue'

const count = ref(1)
const doubled = computed(() => count.value * 2)
</script><template><div>{{ doubled }}</div></template>"#,
    );

    assert!(
        testing.contains("type __Verter_TestBindings = import(\"vue\").ShallowUnwrapRef<{"),
        "testing mode should use ShallowUnwrapRef for instance bindings: {testing}"
    );
    assert!(
        testing.contains("count: typeof count"),
        "ref bindings should be included before unwrapping: {testing}"
    );
    assert!(
        testing.contains("doubled: typeof doubled"),
        "computed bindings should be included before unwrapping: {testing}"
    );
    assert!(
        !testing.contains("ref: typeof ref"),
        "imported helpers must stay out of the instance binding map: {testing}"
    );
}

// @ai-generated - defineExpose must not narrow test-only wrapper.vm bindings.
#[test]
fn tsc_testing_mode_ignores_define_expose_narrowing() {
    let testing = gen_tsc_testing(
        r#"<script setup lang="ts">
import { ref } from 'vue'

const foo = ref(1)
const bar = ref('hidden')

defineExpose({ foo })
</script><template><div>{{ foo }}</div></template>"#,
    );

    assert!(
        testing.contains("foo: typeof foo"),
        "explicitly exposed bindings should still be present: {testing}"
    );
    assert!(
        testing.contains("bar: typeof bar"),
        "non-exposed bindings must remain available in testing mode: {testing}"
    );
    assert!(
        !testing.contains("defineExpose({ foo }) as"),
        "testing mode should not rewrite defineExpose into a narrowing helper: {testing}"
    );
}

// ── defineProps({ ... }) — object syntax, runtime + TS types ─────────────────

#[test]
fn tsc_codegen_props_object_syntax_runtime_and_typed() {
    let r = gen_tsc(
        r#"<script setup>
defineProps({ title: String, count: { type: Number, required: true } })
</script><template/>"#,
    );

    assert!(r.contains("props: {"), "runtime props in __comp");
    assert!(r.contains("title: String"), "runtime String constructor");
    assert!(
        r.contains("{ type: Number, required: true }"),
        "runtime Number required"
    );
    assert!(r.contains("title?: string"), "optional string in declare");
    assert!(r.contains("count: number"), "required number in declare");
    assert!(!r.contains("defineProps"), "macro removed");
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── defineModel<string>('name') — runtime props + emits + TS types ───────────

#[test]
fn tsc_codegen_define_model_runtime_and_typed() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
const title = defineModel<string>('title')
</script><template/>"#,
        &[model_fixture("title", "string")],
    );

    assert!(r.contains("props: {"), "runtime props in __comp");
    assert!(r.contains("title: String"), "runtime model prop");
    assert!(r.contains("emits: ["), "runtime emits in __comp");
    assert!(r.contains("\"update:title\""), "runtime model emit");
    assert!(
        r.contains("\"onUpdate:title\""),
        "model onUpdate prop in declare"
    );
    assert!(
        r.contains(r#"event: "update:title", v: string"#),
        "model emit type in $emit overload"
    );
    assert!(!r.contains("defineModel"), "macro removed");
    assert!(!r.contains("__verter_"), "no intermediate aliases");
    assert!(!r.contains("const title"), "no script body variable");
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── defineEmits(['...']) — array syntax runtime + typed ──────────────────────

#[test]
fn tsc_codegen_emits_array_syntax() {
    let r = gen_tsc(
        r#"<script setup>
defineEmits(['change', 'update:model'])
</script><template/>"#,
    );

    assert!(r.contains("emits: ["), "runtime emits in __comp");
    assert!(r.contains("\"change\""), "runtime emits has change");
    assert!(
        r.contains("\"update:model\""),
        "runtime emits has update:model"
    );
    assert!(
        r.contains(r#"event: "update:model""#),
        "typed emits in $emit overload"
    );
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── defineOptions({ name, inheritAttrs }) — options in __comp ────────────────

#[test]
fn tsc_codegen_define_options_in_comp() {
    let r = gen_tsc(
        r#"<script setup>
defineOptions({ name: 'MyComp', inheritAttrs: false })
</script><template/>"#,
    );

    assert!(
        r.contains("name: 'MyComp' as const"),
        "name as const in __comp"
    );
    assert!(r.contains("inheritAttrs: false"), "inheritAttrs in __comp");
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── Script body + value imports must not appear in output ────────────────────

#[test]
fn tsc_codegen_no_body_no_value_imports() {
    let r = gen_tsc(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
const doubled = computed(() => count.value * 2)
</script><template/>"#,
    );

    assert!(!r.contains("const count"), "no ref variable in output");
    assert!(
        !r.contains("const doubled"),
        "no computed variable in output"
    );
    assert!(!r.contains("import { ref }"), "value import not in output");
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── SFC without <script setup> returns a stub ─────────────────────────────────

#[test]
fn tsc_codegen_no_script_setup_returns_stub() {
    let r = gen_tsc(r#"<template><div>hello</div></template>"#);

    assert!(r.contains("defineComponent"), "stub has defineComponent");
    assert!(r.contains("export default"), "stub has export default");
    assert!(r.contains("sourceMappingURL"), "stub has sourceMappingURL");
}

// ── Options API stub preserves defineComponent props ───────────────────────

#[test]
fn tsc_codegen_options_api_preserves_props() {
    let r = gen_tsc(
        r#"<script lang="ts">
import { defineComponent } from 'vue'
export default defineComponent({
  props: {
    count: { type: Number, required: true },
    label: String
  }
})
</script>
<template><div>{{ count }}</div></template>"#,
    );

    assert!(r.contains("defineComponent"), "stub has defineComponent");
    assert!(r.contains("export default"), "stub has export default");
    // The stub must preserve the actual props so cross-component type checking works
    assert!(
        r.contains("count"),
        "stub must preserve prop 'count' for cross-component type checking:\n{r}"
    );
    assert!(
        r.contains("Number"),
        "stub must preserve prop type 'Number':\n{r}"
    );
    assert!(
        !r.contains("defineComponent({})"),
        "stub must NOT be the empty defineComponent({{}}) placeholder:\n{r}"
    );
}

// ── Options API plain object wrapping with defineComponent ─────────────────

#[test]
fn tsc_options_api_plain_object_gets_define_component_wrap() {
    let r = gen_tsc(
        r#"<script>
export default {
  data() { return { count: 0 } },
  methods: { increment() { this.count++ } }
}
</script>
<template><div>{{ count }}</div></template>"#,
    );

    // Positive: should have defineComponent wrapping
    assert!(
        r.contains("defineComponent("),
        "plain object should be wrapped with defineComponent:\n{r}"
    );
    assert!(
        r.contains("import { defineComponent }"),
        "should add defineComponent import:\n{r}"
    );
    // Positive: original content preserved inside the wrap
    assert!(
        r.contains("data()"),
        "data() must be preserved inside defineComponent wrap:\n{r}"
    );

    // Negative: should not have bare object as default export
    // (the object literal should be inside defineComponent())
    assert!(
        !r.contains("export default {"),
        "plain object should not remain bare — must be wrapped with defineComponent:\n{r}"
    );
}

#[test]
fn tsc_options_api_with_define_component_not_double_wrapped() {
    let r = gen_tsc(
        r#"<script lang="ts">
import { defineComponent } from 'vue'
export default defineComponent({
  data() { return { count: 0 } }
})
</script>
<template><div>{{ count }}</div></template>"#,
    );

    // Positive: defineComponent preserved
    assert!(
        r.contains("defineComponent("),
        "existing defineComponent should be preserved:\n{r}"
    );

    // Negative: should not double-wrap
    let count = r.matches("defineComponent(").count();
    assert_eq!(
        count, 1,
        "should not double-wrap defineComponent, got {count} occurrences in:\n{r}"
    );
}

#[test]
fn tsc_options_api_non_object_export_not_wrapped() {
    let r = gen_tsc(
        r#"<script>
const MyComponent = { data() { return {} } }
export default MyComponent
</script>
<template><div></div></template>"#,
    );

    // Negative: identifier export should NOT get defineComponent wrap
    assert!(
        !r.contains("import { defineComponent }"),
        "identifier export should not get defineComponent import added:\n{r}"
    );
}

// ── PropType<X> extraction ──────────────────────────────────────────────────

#[test]
fn tsc_codegen_proptype_extraction() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'
defineProps({
  items: Array as PropType<string[]>,
  handler: Function as PropType<(e: Event) => void>
})
</script><template/>"#,
    );

    assert!(r.contains("items?: string[]"), "PropType<string[]>");
    assert!(
        r.contains("handler?: (e: Event) => void"),
        "PropType function"
    );
    let comp_section = r.split("declare const").next().unwrap();
    assert!(
        !comp_section.contains("as PropType"),
        "no PropType cast in runtime"
    );
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── Factory function default ──────────────────────────────────────────────────

#[test]
fn tsc_codegen_factory_function_default() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'
defineProps({
  config: { type: Object as PropType<{name: string}>, default: () => ({}) }
})
</script><template/>"#,
    );

    assert!(
        r.contains("config?: {name: string}"),
        "PropType in object form: got {}",
        r
    );
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── Mixed type sources ──────────────────────────────────────────────────────

#[test]
fn tsc_codegen_mixed_type_sources() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'
defineProps({
  title: String,
  count: { type: Number, required: true },
  items: Array as PropType<string[]>,
  config: { type: Object as PropType<{name: string}>, required: true }
})
</script><template/>"#,
    );

    assert!(r.contains("title?: string"), "simple constructor");
    assert!(r.contains("count: number"), "required number");
    assert!(r.contains("items?: string[]"), "PropType annotation");
    assert!(
        r.contains("config: {name: string}"),
        "PropType in object form"
    );
    assert!(!r.contains("setup("), "no setup() in __comp");
}

// ── Complex real-world SFC ──────────────────────────────────────────────────

#[test]
fn tsc_codegen_complex_real_world_sfc() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
import { ref, computed } from 'vue'
import type { PropType } from 'vue'

defineOptions({ name: 'MyForm', inheritAttrs: false })

const emit = defineEmits(['submit', 'cancel'])

const props = defineProps({
  title: String,
  maxLength: { type: Number, required: true },
  items: Array as PropType<string[]>,
  config: { type: Object as PropType<{name: string}>, default: () => ({}) }
})

const name = defineModel<string>('name')

const inputRef = ref(null)
const isValid = computed(() => props.title !== '')
</script>
<template>
  <form @submit.prevent="emit('submit')">
    <input v-model="name" :ref="inputRef" />
  </form>
</template>"#,
        &[model_fixture_at(3, "name", "string", &[], &[])],
    );

    assert!(!r.contains("const inputRef"), "no ref variable");
    assert!(!r.contains("const isValid"), "no computed variable");
    assert!(!r.contains("const props"), "no props variable");
    assert!(!r.contains("const emit"), "no emit variable");
    assert!(!r.contains("import { ref"), "no value imports");
    assert!(!r.contains("setup("), "no setup()");

    let comp_section = r.split("declare const").next().unwrap();
    assert!(
        comp_section.contains("name: 'MyForm' as const"),
        "name option"
    );
    assert!(comp_section.contains("inheritAttrs: false"), "inheritAttrs");
    assert!(comp_section.contains("props: {"), "runtime props");
    assert!(comp_section.contains("title: String"), "runtime String");
    assert!(
        !comp_section.contains("as PropType"),
        "no PropType in runtime"
    );
    assert!(comp_section.contains("emits: ["), "runtime emits");
    assert!(comp_section.contains("\"submit\""), "submit emit");
    assert!(comp_section.contains("\"cancel\""), "cancel emit");
    assert!(comp_section.contains("\"update:name\""), "model emit");

    let declare_section = r.split("declare const").nth(1).unwrap();
    assert!(
        declare_section.contains("title?: string"),
        "optional string"
    );
    assert!(
        declare_section.contains("maxLength: number"),
        "required number"
    );
    assert!(
        declare_section.contains("items?: string[]"),
        "PropType annotation"
    );
    assert!(
        declare_section.contains("config?: {name: string}"),
        "PropType with default"
    );
}

// ── Runtime stripping: as PropType<X> removed from __comp ───────────────────

#[test]
fn tsc_codegen_runtime_stripping_as_proptype() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'
defineProps({ items: Array as PropType<string[]> })
</script><template/>"#,
    );

    let comp_section = r.split("declare const").next().unwrap();
    assert!(
        !comp_section.contains("as PropType"),
        "no PropType cast in runtime section"
    );
    assert!(comp_section.contains("items: Array"), "constructor kept");
}

// ── withDefaults makes props optional ────────────────────────────────────────

#[test]
fn tsc_codegen_with_defaults_makes_props_optional() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
withDefaults(defineProps<{ title: string; count: number }>(), {
  title: 'hello'
})
</script><template/>"#,
        &[props_fixture_identity(
            0,
            1,
            &[
                authored_prop("title", false, "string", 0, 0),
                authored_prop("count", false, "number", 0, 1),
            ],
            &[],
            &[],
        )],
    );

    assert!(r.contains("title?: string"), "title optional with default");
    assert!(
        r.contains("count: number"),
        "count required without default"
    );
    assert!(!r.contains("withDefaults"), "macro removed");
    assert!(!r.contains("setup("), "no setup()");
}

// ── withDefaults with imported type ──────────────────────────────────────────

#[test]
fn tsc_codegen_with_defaults_imported_type() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
import type { Props } from './types'
withDefaults(defineProps<Props>(), { title: 'hello' })
</script><template/>"#,
        &[props_fixture_identity(0, 1, &[], &["Props"], &[])],
    );

    assert!(
        r.contains("import type { Props } from './types'"),
        "import type statement present"
    );
    assert!(
        r.contains(r#"Omit<Props, "title"> & Partial<Pick<Props, "title">>"#),
        "defaulted imported props should be wrapped to make keys optional: {r}"
    );
    assert!(!r.contains("withDefaults"), "macro removed");
}

// @ai-generated - Companion-script resolved prop spans should be consumed as absolute SFC spans.
#[test]
fn tsc_testing_mode_same_sfc_companion_props_use_absolute_spans() {
    let source = r#"<script lang="ts">
export interface Props {
  title: string
  count?: number
}
</script>
<script setup lang="ts">
withDefaults(defineProps<Props>(), {
  title: 'hello',
})
</script><template><div>{{ title }} {{ count }}</div></template>"#;
    let mut bundle = fixture_bundle(&[props_fixture_identity(
        0,
        1,
        &[
            root_prop("title", false, "string", 0),
            root_prop("count", true, "number", 0),
        ],
        &[],
        &[(
            "Props",
            "interface Props {\n  title: string\n  count?: number\n}",
        )],
    )]);
    let MacroTscOutcome::Complete(MacroTscProjection::Props(props)) =
        &mut bundle.entries[0].outcome
    else {
        unreachable!()
    };
    let [declaration] = props.scope.dependency_declarations.as_mut_slice() else {
        unreachable!()
    };
    declaration.owner = TscScriptOwner::Companion;

    let r = generate_with_bundle(source, TscMode::Testing, &bundle)
        .expect("companion-owned fixture must match typed macro syntax")
        .code;

    assert!(
        r.contains("declare const title: string"),
        "defaulted companion-script props should keep their concrete type text: {r}"
    );
    assert!(
        r.contains("declare const count: (number) | undefined"),
        "optional companion-script props should still render optional types: {r}"
    );
}

// ── Object prop with default is optional ─────────────────────────────────────

#[test]
fn tsc_codegen_object_prop_with_default_is_optional() {
    let r = gen_tsc(
        r#"<script setup>
defineProps({ color: { type: String, default: 'red' } })
</script><template/>"#,
    );

    assert!(r.contains("color?: string"), "default makes it optional");
}

// ── PropType with default is optional ────────────────────────────────────────

#[test]
fn tsc_codegen_proptype_with_default_is_optional() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'
defineProps({ config: { type: Object as PropType<{name: string}>, default: () => ({}) } })
</script><template/>"#,
    );

    assert!(
        r.contains("config?: {name: string}"),
        "PropType with default is optional"
    );
}

// ── Union type array: [String, Number, Boolean] ─────────────────────────────

#[test]
fn tsc_codegen_union_type_array_prop() {
    let r = gen_tsc(
        r#"<script setup>
defineProps({
  value: [String, Number, Boolean],
  mixed: { type: [String, Number], required: true }
})
</script><template/>"#,
    );

    assert!(
        r.contains("value?: string | number | boolean"),
        "union type from array"
    );
    assert!(
        r.contains("mixed: string | number"),
        "union type from object array"
    );
    // Verify the $props type has no unknown — the emits fallback may contain unknown
    let props_section = r
        .split("$props:")
        .nth(1)
        .and_then(|s| s.split(',').next())
        .unwrap_or("");
    assert!(
        !props_section.contains("unknown"),
        "no unknown in props types"
    );
    assert!(!r.contains("setup("), "no setup()");
}

// ── defineModel default name (no arg) ────────────────────────────────────────

#[test]
fn tsc_codegen_define_model_default_name() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
const mv = defineModel<number>()
</script><template/>"#,
        &[model_fixture("modelValue", "number")],
    );

    assert!(r.contains("modelValue: Number"), "runtime modelValue prop");
    assert!(
        r.contains("\"update:modelValue\""),
        "runtime update:modelValue emit"
    );
    assert!(
        r.contains("modelValue?: number"),
        "TS modelValue type in $props"
    );
    assert!(r.contains("\"onUpdate:modelValue\""), "TS onUpdate handler");
    assert!(!r.contains("const mv"), "no script body variable");
}

// ── Multiple defineModel calls ───────────────────────────────────────────────

#[test]
fn tsc_codegen_multiple_define_models() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
const first = defineModel<string>('firstName')
const last = defineModel<string>('lastName')
</script><template/>"#,
        &[
            model_fixture_at(0, "firstName", "string", &[], &[]),
            model_fixture_at(1, "lastName", "string", &[], &[]),
        ],
    );

    assert!(r.contains("firstName: String"), "runtime firstName prop");
    assert!(r.contains("lastName: String"), "runtime lastName prop");
    assert!(
        r.contains("\"update:firstName\""),
        "runtime update:firstName"
    );
    assert!(r.contains("\"update:lastName\""), "runtime update:lastName");
    assert!(r.contains("firstName?: string"), "TS firstName type");
    assert!(r.contains("lastName?: string"), "TS lastName type");
}

// ── defineModel with no type parameter ───────────────────────────────────────

#[test]
fn tsc_codegen_define_model_no_type() {
    let r = gen_tsc(
        r#"<script setup>
const val = defineModel('value')
</script><template/>"#,
    );

    assert!(r.contains("value: Object"), "runtime prop with Object ctor");
    assert!(
        r.contains("value?: unknown"),
        "TS type is unknown without type param"
    );
}

// ── defineModel with imported type ──────────────────────────────────────────

#[test]
fn tsc_codegen_define_model_imported_type() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
import type { User } from './types'
const user = defineModel<User>()
</script><template/>"#,
        &[model_fixture_at(0, "modelValue", "User", &["User"], &[])],
    );

    assert!(
        r.contains("import type { User } from './types'"),
        "import type statement for model should be emitted: {r}"
    );
    assert!(r.contains("modelValue?: User"), "TS User type");
    assert!(
        r.contains(r#"event: "update:modelValue", v: User"#),
        "emit type with User in $emit overload"
    );
}

#[test]
fn tsc_codegen_define_model_local_type_dependencies_are_emitted() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
interface Role {
  name: string
}

interface User {
  role: Role
}

const user = defineModel<User>()
</script><template/>"#,
        &[model_fixture_at(
            0,
            "modelValue",
            "User",
            &[],
            &[
                ("Role", "interface Role {\n  name: string\n}"),
                ("User", "interface User {\n  role: Role\n}"),
            ],
        )],
    );

    assert!(
        r.contains("interface User"),
        "User interface should be emitted: {r}"
    );
    assert!(
        r.contains("interface Role"),
        "Role interface should be emitted: {r}"
    );
    assert!(
        r.contains("modelValue?: User"),
        "model should keep the named User type: {r}"
    );
}

// ── defineModel combined with defineProps ────────────────────────────────────

#[test]
fn tsc_codegen_define_model_with_define_props() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineProps({ label: String })
const text = defineModel<string>()
</script><template/>"#,
        &[model_fixture_at(1, "modelValue", "string", &[], &[])],
    );

    assert!(r.contains("label: String"), "runtime label prop");
    assert!(
        r.contains("modelValue: String"),
        "runtime modelValue from model"
    );
    assert!(r.contains("label?: string"), "TS label type");
    assert!(r.contains("modelValue?: string"), "TS modelValue type");
}

// ── Edge cases ──────────────────────────────────────────────────────────────

#[test]
fn tsc_codegen_edge_cases() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'
defineProps({
  cb: Function,
  obj: Object,
  arr: Array,
  sym: Symbol,
  disabled: Boolean,
  items: { type: Array as PropType<string[]>, required: true },
})
</script><template/>"#,
    );

    assert!(
        r.contains("cb?: (...args: unknown[]) => unknown"),
        "Function type"
    );
    assert!(r.contains("obj?: Record<string, unknown>"), "Object type");
    assert!(r.contains("arr?: unknown[]"), "Array type");
    assert!(r.contains("sym?: symbol"), "Symbol type");
    assert!(r.contains("disabled?: boolean"), "Boolean type");
    assert!(r.contains("items: string[]"), "required PropType");
    assert!(!r.contains("setup("), "no setup()");
}

// ── Print output for real-world SFCs ─────────────────────────────────────────

#[test]
fn tsc_codegen_print_real_world_coreui() {
    let r = gen_tsc(
        r##"<script setup>
const props = defineProps({
  href: String,
  tabContentClass: String,
})

const url = `https://coreui.io/vue/docs/${props.href}`
const addClass = props.tabContentClass
</script>
<template>
  <div class="example">
    <CNav variant="underline-border">
      <CNavItem>
        <CNavLink href="#" active>
          <CIcon icon="cil-media-play" class="me-2" />
          Preview
        </CNavLink>
      </CNavItem>
    </CNav>
  </div>
</template>"##,
    );
    eprintln!("\n=== CoreUI DocsExample.vue ===\n{}\n", r);

    assert!(r.contains("href?: string"), "href prop");
    assert!(
        r.contains("tabContentClass?: string"),
        "tabContentClass prop"
    );
    assert!(!r.contains("const url"), "no script body");
    assert!(!r.contains("const addClass"), "no script body");
}

#[test]
fn tsc_codegen_print_real_world_slidev() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineProps<{
  disabled?: boolean
}>()

const value = defineModel<boolean>('modelValue', {
  type: Boolean,
})
</script>
<template>
  <div border="~ main rounded">
    <div i-ri-check-line :class="value ? '' : 'op0'" />
    <input v-model="value" type="checkbox" :disabled="disabled">
  </div>
</template>"#,
        &[
            props_fixture("", &[authored_prop("disabled", true, "boolean", 0, 0)]),
            model_fixture_at(1, "modelValue", "boolean", &[], &[]),
        ],
    );
    eprintln!("\n=== Slidev FormCheckbox.vue ===\n{}\n", r);

    assert!(r.contains("disabled?: boolean"), "disabled prop");
    assert!(r.contains("modelValue"), "modelValue from defineModel");
    assert!(!r.contains("const value"), "no script body");
}

#[test]
fn tsc_codegen_print_real_world_element_plus_watermark() {
    let r = gen_tsc_with(
        r#"<script lang="ts" setup>
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { useMutationObserver } from '@vueuse/core'
import { isArray, isUndefined } from '@element-plus/utils'
import { getPixelRatio, getStyleStr, reRendering } from './utils'
import useClips from './useClips'

import type { WatermarkProps } from './watermark'
import type { CSSProperties } from 'vue'

defineOptions({
  name: 'ElWatermark',
})

const style: CSSProperties = {
  position: 'relative',
}

const props = withDefaults(defineProps<WatermarkProps>(), {
  zIndex: 9,
  rotate: -22,
  content: 'Element Plus',
  gap: () => [100, 100],
})
const fontGap = computed(() => props.font?.fontGap ?? 3)
const color = computed(() => props.font?.color ?? 'rgba(0,0,0,.15)')
const containerRef = shallowRef<HTMLDivElement | null>(null)
const watermarkRef = shallowRef<HTMLDivElement>()
const stopObservation = ref(false)
</script>
<template>
  <div ref="containerRef" :style="[style]">
    <slot />
  </div>
</template>"#,
        &[props_fixture_identity(1, 2, &[], &["WatermarkProps"], &[])],
    );
    eprintln!("\n=== Element Plus watermark.vue ===\n{}\n", r);

    assert!(r.contains("name: 'ElWatermark' as const"), "name option");
    assert!(
        r.contains("import type { WatermarkProps } from './watermark'"),
        "imported type as import statement"
    );
    assert!(
        r.contains(
            r#"$props: import("vue").PublicProps & Omit<WatermarkProps, "zIndex" | "rotate" | "content" | "gap"> & Partial<Pick<WatermarkProps, "zIndex" | "rotate" | "content" | "gap">>"#
        ),
        "defaulted imported props should be optional in $props"
    );
    assert!(!r.contains("const style"), "no script body");
    assert!(!r.contains("const fontGap"), "no computed");
    assert!(!r.contains("import { computed"), "no value imports");
}

#[test]
fn tsc_codegen_print_real_world_complex_type_syntax() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

interface CarouselProps {
  opts?: Record<string, unknown>
  plugins?: unknown[]
  orientation?: 'horizontal' | 'vertical'
  class?: HTMLAttributes['class']
}

const props = withDefaults(defineProps<CarouselProps>(), {
  orientation: 'horizontal',
})

const emits = defineEmits<{
  (e: 'init-api', api: unknown): void
}>()

const dir = ref<'ltr' | 'rtl'>('ltr')
</script>
<template>
  <div :class="props.class" :dir="dir">
    <slot />
  </div>
</template>"#,
        &[
            props_fixture_identity(
                0,
                1,
                &[],
                &["HTMLAttributes"],
                &[(
                    "CarouselProps",
                    "interface CarouselProps {\n  opts?: Record<string, unknown>\n  plugins?: unknown[]\n  orientation?: 'horizontal' | 'vertical'\n  class?: HTMLAttributes['class']\n}",
                )],
            ),
            emits_fixture_identity(
                1,
                2,
                &[authored_emit("init-api", "api: unknown", "api: unknown", 2, 0)],
                &[],
                &[],
            ),
        ],
    );
    eprintln!(
        "\n=== Carousel.vue (type syntax + withDefaults + emits) ===\n{}\n",
        r
    );

    assert!(
        r.contains("orientation?"),
        "orientation optional via default"
    );
    assert!(r.contains("\"init-api\""), "emit name in output");
    assert!(!r.contains("const dir"), "no script body");
}

// ── Union runtime function type must be parenthesized (TS1385) ───────────────

#[test]
fn tsc_codegen_union_runtime_function() {
    let r = gen_tsc(
        r#"<script setup>
defineProps({ msg: [String, Function] })
</script><template/>"#,
    );

    assert!(
        r.contains("msg?: string | ((...args: unknown[]) => unknown)"),
        "Function in union must be parenthesized: got {}",
        r
    );
    // Negative: must NOT have unparenthesized arrow in a union
    assert!(
        !r.contains("string | (...args"),
        "unparenthesized function type in union"
    );
}

#[test]
fn tsc_codegen_single_runtime_function() {
    let r = gen_tsc(
        r#"<script setup>
defineProps({ cb: Function })
</script><template/>"#,
    );

    assert!(
        r.contains("cb?: (...args: unknown[]) => unknown"),
        "single Function: no extra parens needed: got {}",
        r
    );
    // Negative: must NOT have outer parens when not in a union
    assert!(
        !r.contains("cb?: ((...args"),
        "single function should not have outer parens"
    );
}

#[test]
fn tsc_codegen_proptype_union_parens() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'
defineProps({ msg: String as PropType<string | (() => any)> })
</script><template/>"#,
    );

    assert!(
        r.contains("msg?: string | (() => any)"),
        "PropType union with parenthesized function preserved: got {}",
        r
    );
}

#[test]
fn tsc_codegen_type_only_union_function() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts">
defineProps<{ cb: string | (() => void) }>()
</script><template/>"#,
    );

    assert!(
        r.contains("cb: string | (() => void)"),
        "type-only union function preserved from source: got {}",
        r
    );
    // Negative: must NOT lose the parens around the arrow function
    assert!(
        !r.contains("string | () =>"),
        "must not lose parens around arrow function type"
    );
}

// ── Nested object prop with PropType inside — no truncation ──────────────────

#[test]
fn tsc_codegen_nested_object_prop_with_proptype_not_truncated() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'

interface CascaderNode { label: string }

defineProps({
  nodes: {
    type: Array as PropType<CascaderNode[]>,
    required: true,
  },
  index: {
    type: Number,
    required: true,
  },
})
</script><template/>"#,
    );
    eprintln!("\n=== Nested object PropType (cascader-like) ===\n{}\n", r);

    // Runtime section must have valid object syntax (not truncated)
    let comp_section = r.split("declare const").next().unwrap();
    assert!(
        comp_section.contains("nodes: { type: Array, required: true }"),
        "nested object prop reconstructed cleanly: got {}",
        comp_section
    );
    assert!(
        comp_section.contains("index: { type: Number, required: true }"),
        "index prop intact"
    );
    assert!(
        !comp_section.contains("as PropType"),
        "no PropType cast in runtime"
    );

    // Type section
    assert!(r.contains("nodes: CascaderNode[]"), "required prop type");
    assert!(r.contains("index: number"), "required number type");
}

// ══════════════════════════════════════════════════════════════════════════════
// ── Step 2: Type Import Statements ──────────────────────────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsc_codegen_type_import_emits_import_statement() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script><template/>"#,
        &[props_root_fixture_at(0, "Props", &[], &["Props"], &[])],
    );

    // Positive: should emit a proper import type statement
    assert!(
        r.contains("import type { Props } from './types'"),
        "should emit import type statement: got {}",
        r
    );
    // Positive: $props should reference the type name directly
    assert!(
        r.contains("$props: import(\"vue\").PublicProps & Props"),
        "should use type name directly in $props: got {}",
        r
    );
    // Negative: should NOT use inline import() syntax anymore
    assert!(
        !r.contains("import('./types').Props"),
        "should not use inline import() syntax: got {}",
        r
    );
}

#[test]
fn tsc_codegen_type_import_specifier_level() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
import { type MyProps, someValue } from './shared'
defineProps<MyProps>()
</script><template/>"#,
        &[props_root_fixture_at(0, "MyProps", &[], &["MyProps"], &[])],
    );

    // Should emit type import for the type-only specifier
    assert!(
        r.contains("import type { MyProps } from './shared'"),
        "should emit import type for specifier-level type: got {}",
        r
    );
    assert!(
        r.contains("$props: import(\"vue\").PublicProps & MyProps"),
        "should reference type name directly: got {}",
        r
    );
    // Negative: should NOT import the value binding
    assert!(
        !r.contains("someValue"),
        "should not import the value binding: got {}",
        r
    );
}

#[test]
fn tsc_codegen_define_emits_local_type_dependencies_are_emitted() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
interface Payload {
  value: string
}

interface Emits {
  (e: 'submit', payload: Payload): void
}

defineEmits<Emits>()
</script><template/>"#,
        &[emits_fixture_at(
            0,
            &[root_emit(
                "submit",
                "payload: Payload",
                "payload: Payload",
                0,
            )],
            &[],
            &[
                ("Payload", "interface Payload {\n  value: string\n}"),
                (
                    "Emits",
                    "interface Emits {\n  (e: 'submit', payload: Payload): void\n}",
                ),
            ],
        )],
    );

    assert!(
        r.contains("interface Emits"),
        "defineEmits local type should emit the root declaration: {r}"
    );
    assert!(
        r.contains("interface Payload"),
        "defineEmits local type should emit transitive local declarations: {r}"
    );
}

#[test]
fn tsc_codegen_type_import_with_defaults() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
import type { Props } from './types'
withDefaults(defineProps<Props>(), { title: 'hello' })
</script><template/>"#,
        &[props_fixture_identity(0, 1, &[], &["Props"], &[])],
    );

    // Should emit import type statement even through withDefaults
    assert!(
        r.contains("import type { Props } from './types'"),
        "should emit import type with withDefaults: got {}",
        r
    );
    // $props should preserve the named type while optionalizing defaulted keys
    assert!(
        r.contains(
            r#"$props: import("vue").PublicProps & Omit<Props, "title"> & Partial<Pick<Props, "title">>"#
        ),
        "should wrap named props when defaults are present: got {}",
        r
    );
}

#[test]
fn tsc_codegen_no_unused_type_imports() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
import type { UnusedType } from './unused'
import type { Props } from './types'
defineProps<Props>()
</script><template/>"#,
        &[props_root_fixture_at(0, "Props", &[], &["Props"], &[])],
    );

    // Should NOT emit unused type imports
    assert!(
        !r.contains("UnusedType"),
        "should not emit unused type import: got {}",
        r
    );
    assert!(
        !r.contains("'./unused'"),
        "should not reference unused source: got {}",
        r
    );
    // Should emit the used one
    assert!(
        r.contains("import type { Props } from './types'"),
        "should emit used type import: got {}",
        r
    );
}

// ── Step 3: JSDoc Comments on Props ─────────────────────────────────────────

#[test]
fn tsc_codegen_jsdoc_on_props() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts">
defineProps<{
  /** The title of the component */
  title: string
  /** The count value */
  count: number
}>()
</script><template/>"#,
    );

    // Positive: JSDoc comments preserved on props
    assert!(
        r.contains("/** The title of the component */"),
        "should preserve JSDoc on title: got {}",
        r
    );
    assert!(
        r.contains("/** The count value */"),
        "should preserve JSDoc on count: got {}",
        r
    );
    assert!(r.contains("title: string"), "title prop present");
    assert!(r.contains("count: number"), "count prop present");
}

#[test]
fn tsc_codegen_jsdoc_multiline() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts">
defineProps<{
  /**
   * The title of the component.
   * @default 'hello'
   */
  title: string
}>()
</script><template/>"#,
    );

    // Multi-line JSDoc should be preserved
    assert!(
        r.contains("* The title of the component."),
        "should preserve multi-line JSDoc: got {}",
        r
    );
    assert!(
        r.contains("@default 'hello'"),
        "should preserve @default tag: got {}",
        r
    );
}

#[test]
fn tsc_codegen_no_jsdoc_on_type_ref() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script><template/>"#,
        &[props_root_fixture_at(0, "Props", &[], &["Props"], &[])],
    );

    // No JSDoc should be present when using external type reference
    // (the external type carries its own docs)
    assert!(
        !r.contains("/**"),
        "should not have JSDoc on type ref: got {}",
        r
    );
}

// ── Step 4: Slots Support ───────────────────────────────────────────────────

#[test]
fn tsc_codegen_define_slots_inline() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineSlots<{
  default(props: { item: string }): any
  header(): any
}>()
</script><template/>"#,
    );

    // Positive: $slots in output
    assert!(r.contains("$slots:"), "should emit $slots: got {}", r);
    assert!(
        r.contains("default(props: { item: string }): any"),
        "should have default slot type: got {}",
        r
    );
    assert!(
        r.contains("header(): any"),
        "should have header slot type: got {}",
        r
    );
    // Negative: no defineSlots in output
    assert!(
        !r.contains("defineSlots"),
        "defineSlots macro should be removed: got {}",
        r
    );
}

#[test]
fn tsc_codegen_define_slots_imported() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { MySlots } from './slots'
defineSlots<MySlots>()
</script><template/>"#,
    );

    // Should import the type and reference it
    assert!(
        r.contains("import type { MySlots } from './slots'"),
        "should emit import for slot type: got {}",
        r
    );
    assert!(
        r.contains("$slots: MySlots"),
        "should reference slot type by name: got {}",
        r
    );
}

#[test]
fn tsc_codegen_no_slots_when_not_defined() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts">
defineProps<{ title: string }>()
</script><template/>"#,
    );

    // No $slots: field inside the instance when defineSlots not used.
    // Note: "$slots" appears in the Omit<CPI, ...> exclusion list, but
    // the actual `$slots:` assignment must NOT appear in the instance body.
    assert!(
        !r.contains("$slots:"),
        "should not emit $slots: field without defineSlots: got {}",
        r
    );
}

#[test]
fn tsc_codegen_define_slots_local_interface() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
interface MySlots {
  default(props: { msg: string }): any
}
defineSlots<MySlots>()
</script><template/>"#,
    );

    // Should include local type declaration and reference it
    assert!(
        r.contains("interface MySlots"),
        "should include local interface: got {}",
        r
    );
    assert!(
        r.contains("$slots: MySlots"),
        "should reference local slot type: got {}",
        r
    );
}

// ── Step 5: Generic Component Support ───────────────────────────────────────

#[test]
fn tsc_codegen_generic_basic() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts" generic="T">
defineProps<{ items: T[] }>()
</script><template/>"#,
    );

    // Positive: generic on new() with props param
    assert!(
        r.contains("new<T>(props?"),
        "should emit generic on new(props?): got {}",
        r
    );
    assert!(
        r.contains("items: T[]"),
        "should preserve generic type param in props: got {}",
        r
    );
    // Negative: should NOT have plain new() without generic
    assert!(
        !r.contains("  new()"),
        "should not have non-generic new(): got {}",
        r
    );
}

#[test]
fn tsc_codegen_generic_with_constraints() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts" generic="T extends string">
defineProps<{ value: T }>()
</script><template/>"#,
    );

    assert!(
        r.contains("new<T extends string>(props?"),
        "should emit generic with constraint and props: got {}",
        r
    );
}

#[test]
fn tsc_codegen_generic_multiple() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts" generic="K extends string, V">
defineProps<{ key: K; value: V }>()
</script><template/>"#,
    );

    assert!(
        r.contains("new<K extends string, V>(props?"),
        "should emit multiple generic params with props: got {}",
        r
    );
}

#[test]
fn tsc_codegen_no_generic_no_angle_brackets() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts">
defineProps<{ title: string }>()
</script><template/>"#,
    );

    // Without generic, should have plain new(props?: ...) constructor
    assert!(
        r.contains("new(props?: import(\"vue\").PublicProps &"),
        "should have new(props?) with PublicProps: got {}",
        r
    );
    assert!(
        !r.contains("new<"),
        "should not have angle brackets without generic: got {}",
        r
    );
    // Negative: no more Omit<CPI<...>> wrapper
    assert!(
        !r.contains("Omit<import(\"vue\").ComponentPublicInstance<"),
        "should not use Omit<CPI<...>> pattern: got {}",
        r
    );
}

#[test]
fn tsc_codegen_recursive_prop_types_no_excessive_depth() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
export interface Action { label: string; callback?: (a: Action) => void }
defineProps<{ actions: Action[] }>()
</script><template/>"#,
        &[props_fixture_at(
            0,
            "",
            &[],
            &[],
            &[(
                "Action",
                "export interface Action { label: string; callback?: (a: Action) => void }",
            )],
        )],
    );
    // Positive: constructor accepts props param
    assert!(
        r.contains("new(props?:"),
        "constructor accepts props param: got {}",
        r
    );
    // Negative: no ComponentPublicInstance in return type (causes excessive depth)
    assert!(
        !r.contains("ComponentPublicInstance"),
        "no CPI in output — avoids excessive depth: got {}",
        r
    );
    // Negative: no Omit<CPI<...>> wrapping that causes excessive depth
    assert!(
        !r.contains("Omit<import(\"vue\").ComponentPublicInstance<"),
        "no Omit<CPI<...>> pattern: got {}",
        r
    );
}

#[test]
fn tsc_codegen_transitive_local_type_dependencies_are_emitted() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
interface Role {
  name: string
}

interface User {
  role: Role
}

interface Props {
  user: User
}

defineProps<Props>()
</script><template/>"#,
        &[props_fixture_at(
            0,
            "Props",
            &[],
            &[],
            &[
                ("Role", "interface Role {\n  name: string\n}"),
                ("User", "interface User {\n  role: Role\n}"),
                ("Props", "interface Props {\n  user: User\n}"),
            ],
        )],
    );

    assert!(
        r.contains("interface Props"),
        "Props should be emitted: {r}"
    );
    assert!(
        r.contains("interface User"),
        "User should be emitted transitively: {r}"
    );
    assert!(
        r.contains("interface Role"),
        "Role should be emitted transitively: {r}"
    );
}

// ── attrs attribute on <script setup> ────────────────────────────────────────

#[test]
fn tsc_codegen_attrs_explicit_type() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts" attrs="{ class?: string; id?: string }">
defineProps<{ title: string }>()
</script><template/>"#,
    );

    // Positive: $attrs should contain the explicit type
    assert!(
        r.contains("$attrs: { class?: string; id?: string }"),
        "should emit explicit attrs type in $attrs: got {}",
        r
    );
    // Negative: should not be empty
    assert!(
        !r.contains("$attrs: {}"),
        "should not have empty $attrs with explicit attrs: got {}",
        r
    );
}

#[test]
fn tsc_codegen_attrs_default_empty() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts">
defineProps<{ title: string }>()
</script><template/>"#,
    );

    // Positive: $attrs should default to empty
    assert!(
        r.contains("$attrs: {}"),
        "should emit empty $attrs by default: got {}",
        r
    );
}

#[test]
fn tsc_codegen_attrs_alias_attributes() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts" attributes="{ role?: string }">
defineProps<{ title: string }>()
</script><template/>"#,
    );

    // Positive: 'attributes' alias should work
    assert!(
        r.contains("$attrs: { role?: string }"),
        "'attributes' alias should produce typed $attrs: got {}",
        r
    );
}

#[test]
fn tsc_codegen_attrs_with_generic() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts" generic="T" attrs="{ value: T }">
defineProps<{ items: T[] }>()
</script><template/>"#,
    );

    // Positive: $attrs should contain the generic type
    assert!(
        r.contains("$attrs: { value: T }"),
        "should emit generic attrs type in $attrs: got {}",
        r
    );
}

#[test]
fn tsc_codegen_attrs_imported_named_type_emits_import_statement() {
    let r = gen_tsc(
        r#"<script setup lang="ts" attrs="Attrs">
import type { Attrs } from './types'
</script><template/>"#,
    );

    assert!(
        r.contains("import type { Attrs } from './types'"),
        "named attrs type should emit import type statement: {r}"
    );
    assert!(
        r.contains("$attrs: Attrs"),
        "named attrs type should be preserved: {r}"
    );
}

#[test]
fn tsc_type_dependency_inventory_ignores_comment_and_literal_text_and_keeps_unicode_roots() {
    let r = gen_tsc(
        r#"<script setup lang="ts" attrs="Attrs">
import type { Phantom, Réel } from './types'

interface Attrs {
  literal: 'Phantom'
  actual: Réel /* Phantom */
}
</script><template/>"#,
    );

    assert!(r.contains("import type { Réel } from './types'"), "{r}");
    assert!(!r.contains("import type { Phantom }"), "{r}");
    assert!(r.contains("interface Attrs"), "{r}");
}

#[test]
fn tsc_type_dependency_inventory_unions_all_merged_contributors() {
    let r = gen_tsc(
        r#"<script setup lang="ts" attrs="Attrs">
import type { First, Second } from './types'
interface Attrs { first: First }
interface Attrs { second: Second }
</script><template/>"#,
    );

    assert!(r.contains("import type { First } from './types'"), "{r}");
    assert!(r.contains("import type { Second } from './types'"), "{r}");
    assert_eq!(r.matches("interface Attrs").count(), 2, "{r}");
}

// @ai-generated - Guards declaration-carrier dependency closure across all members of a namespace.
#[test]
fn tsc_namespace_carrier_unions_all_member_and_nested_dependencies() {
    let output = gen_tsc_output(
        r#"<script setup lang="ts" attrs="Surface.First & Surface.Nested.Second">
import type { FirstExternal } from './first'
import type { SecondExternal } from './second'

interface FirstDependency { value: FirstExternal }
interface SecondDependency { value: SecondExternal }

namespace Surface {
  export interface First { dependency: FirstDependency }
  export namespace Nested {
    export interface Second { dependency: SecondDependency }
  }
}
</script><template/>"#,
    );
    assert_generated_tsx_parses(&output.code);
    let r = output.code;

    assert!(r.contains("namespace Surface"), "{r}");
    assert!(r.contains("interface FirstDependency"), "{r}");
    assert!(r.contains("interface SecondDependency"), "{r}");
    assert!(
        r.contains("import type { FirstExternal } from './first'"),
        "{r}"
    );
    assert!(
        r.contains("import type { SecondExternal } from './second'"),
        "{r}"
    );
}

// @ai-generated - Proves recovered raw attrs type syntax cannot silently emit invalid TSC output.
#[test]
fn tsc_malformed_raw_attrs_type_fails_closed() {
    let source = r#"<script setup lang="ts" attrs="Attrs.">
import type { Attrs } from './types'
</script><template/>"#;
    let start = source.find("Attrs.").expect("authored attrs payload") as u32;
    let result = generate_tsc_output_with_options(
        source,
        "TestComp",
        &TscGenOptions::default(),
        MacroTscInput::NotRequired,
        &FallthroughPropsProjection::none(),
    );

    let expected = super::script::TscGenerationError::UnavailableOutcome {
        subject: super::script::TscFailureSubject::ScriptSetupAttrs {
            source_range: crate::common::Span::new(start, start + "Attrs.".len() as u32),
        },
        outcome: super::script::TscUnavailableOutcome::Invalid(
            super::script::TscInvalidOutcome::AuthoredTypeSyntax(
                super::script::TscInvalidAuthoredTypeReason::MalformedOrRecoveredTypeSyntax,
            ),
        ),
    };
    assert_eq!(result.unwrap_err(), expected);

    let extracted = extract_tsc_state(source, "TestComp", &TscExtractOptions::default())
        .expect("script setup syntax must still produce a cached extraction");
    assert_eq!(
        generate_tsc_from_state(
            &extracted,
            "TestComp",
            TscMode::Public,
            MacroTscInput::NotRequired,
            &FallthroughPropsProjection::none(),
        )
        .unwrap_err(),
        expected,
    );
}

// @ai-generated - Pins source-carrier parse failures ahead of semantic carrier validation on every generation path.
#[test]
fn tsc_direct_and_cached_generation_share_terminal_failure_precedence() {
    let source = r#"<script setup lang="ts" attrs="Attrs.">
enum Payload { Value = Math.random() }
defineProps<Payload>()
</script><template/>"#;
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![dependency("Payload", Vec::new())],
    });
    let start = source.find("Attrs.").expect("authored attrs payload") as u32;
    let expected = super::script::TscGenerationError::UnavailableOutcome {
        subject: super::script::TscFailureSubject::ScriptSetupAttrs {
            source_range: crate::common::Span::new(start, start + "Attrs.".len() as u32),
        },
        outcome: super::script::TscUnavailableOutcome::Invalid(
            super::script::TscInvalidOutcome::AuthoredTypeSyntax(
                super::script::TscInvalidAuthoredTypeReason::MalformedOrRecoveredTypeSyntax,
            ),
        ),
    };

    let direct = generate_with_bundle(source, TscMode::Public, &bundle)
        .expect_err("malformed source-owned attrs must win direct generation precedence");
    assert_eq!(direct, expected);

    let extracted = extract_tsc_state(source, "TestComp", &TscExtractOptions::default())
        .expect("script setup syntax must still produce a cached extraction");
    let cached = generate_tsc_from_state(
        &extracted,
        "TestComp",
        TscMode::Public,
        MacroTscInput::Authoritative(&bundle),
        &FallthroughPropsProjection::none(),
    )
    .expect_err("malformed source-owned attrs must win cached generation precedence");
    assert_eq!(cached, direct);
}

// @ai-generated - Unplanted control for valid raw attrs parsing and dependency retention.
#[test]
fn tsc_valid_raw_attrs_type_keeps_typed_dependency_paths() {
    let output = gen_tsc_output(
        r#"<script setup lang="ts" attrs="Attrs & ImportedAttrs">
import type { ImportedAttrs } from './types'
interface Attrs { role?: string }
</script><template/>"#,
    );
    assert_generated_tsx_parses(&output.code);
    let r = output.code;

    assert!(r.contains("$attrs: Attrs & ImportedAttrs"), "{r}");
    assert!(r.contains("interface Attrs"), "{r}");
    assert!(
        r.contains("import type { ImportedAttrs } from './types'"),
        "{r}"
    );
}

#[test]
fn tsc_codegen_use_attrs_type_arg_fallback() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs<{ class?: string; id?: string }>()
</script><template/>"#,
    );

    // Positive: useAttrs<T>() type parameter used as $attrs type
    assert!(
        r.contains("$attrs: { class?: string; id?: string }"),
        "should use useAttrs type param as $attrs type, got:\n{}",
        r
    );
    // Negative: should not have empty $attrs
    assert!(
        !r.contains("$attrs: {},"),
        "should not have empty $attrs when useAttrs<T> provides type, got:\n{}",
        r
    );
}

#[test]
fn tsc_codegen_use_attrs_local_type_dependencies_are_emitted() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'

interface Role {
  label: string
}

interface Attrs {
  role: Role
}

const attrs = useAttrs<Attrs>()
</script><template/>"#,
    );

    assert!(
        r.contains("$attrs: Attrs"),
        "named attrs type should be preserved: {r}"
    );
    assert!(
        r.contains("interface Attrs"),
        "Attrs interface should be emitted: {r}"
    );
    assert!(
        r.contains("interface Role"),
        "Role interface should be emitted transitively: {r}"
    );
}

#[test]
fn tsc_codegen_dedupes_shared_named_type_references_across_surfaces() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts" attrs="Shared">
import type { Shared } from './types'
const model = defineModel<Shared>()
</script><template/>"#,
        &[model_fixture_at(
            0,
            "modelValue",
            "Shared",
            &["Shared"],
            &[],
        )],
    );

    let count = r.matches("import type { Shared } from './types'").count();
    assert_eq!(
        count, 1,
        "Shared import should be emitted once across surfaces: {r}"
    );
}

#[test]
fn tsc_codegen_attrs_attribute_priority_over_use_attrs() {
    let r = gen_tsc(
        r#"<script setup lang="ts" attrs="{ role?: string }">
import { useAttrs } from 'vue'
const attrs = useAttrs<{ class?: string }>()
</script><template/>"#,
    );

    // Positive: attrs attribute takes priority
    assert!(
        r.contains("$attrs: { role?: string }"),
        "attrs attribute should take priority over useAttrs<T>, got:\n{}",
        r
    );
    // Negative: useAttrs type should not appear in $attrs
    assert!(
        !r.contains("class?: string"),
        "useAttrs type param should not override attrs attribute, got:\n{}",
        r
    );
}

#[test]
fn tsc_codegen_use_attrs_without_type_no_effect() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs()
</script><template/>"#,
    );

    // Positive: plain useAttrs() → default empty $attrs
    assert!(
        r.contains("$attrs: {},"),
        "useAttrs() without type param should produce empty $attrs, got:\n{}",
        r
    );
}

// ── Root element attrs in external $attrs ────────────────────────────────────

#[test]
fn tsc_root_element_attrs_native_html_root() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
const x = 1
</script><template><div>hello</div></template>"#,
    );
    // Positive: native HTML root should give HTMLAttributes
    assert!(
        r.contains("$attrs: import(\"vue\").HTMLAttributes"),
        "native HTML root should have HTMLAttributes in $attrs, got:\n{}",
        r
    );
    // Negative: should NOT be empty
    assert!(
        !r.contains("$attrs: {},"),
        "$attrs should NOT be empty when native HTML root exists, got:\n{}",
        r
    );
}

#[test]
fn tsc_root_element_attrs_inherit_attrs_false() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
</script><template><div>hello</div></template>"#,
    );
    // Positive: inheritAttrs: false should give empty $attrs
    assert!(
        r.contains("$attrs: {},"),
        "inheritAttrs: false should have empty $attrs, got:\n{}",
        r
    );
    // Negative: should NOT have HTMLAttributes
    assert!(
        !r.contains("HTMLAttributes"),
        "inheritAttrs: false should NOT have HTMLAttributes, got:\n{}",
        r
    );
}

#[test]
fn tsc_root_element_attrs_explicit_takes_precedence() {
    let r = gen_tsc(
        r#"<script setup lang="ts" attrs="{ class?: string }">
const x = 1
</script><template><div>hello</div></template>"#,
    );
    // Positive: explicit attrs should take precedence
    assert!(
        r.contains("$attrs: { class?: string }"),
        "explicit attrs should take precedence over root element, got:\n{}",
        r
    );
    // Negative: should NOT have HTMLAttributes
    assert!(
        !r.contains("HTMLAttributes"),
        "explicit attrs should NOT include HTMLAttributes, got:\n{}",
        r
    );
}

#[test]
fn tsc_root_element_attrs_component_root() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script><template><MyComp /></template>"#,
    );
    // Positive: component root should give empty $attrs (can't resolve type)
    assert!(
        r.contains("$attrs: {},"),
        "component root should have empty $attrs, got:\n{}",
        r
    );
}

#[test]
fn tsc_root_element_attrs_fragment() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
const x = 1
</script><template><div>A</div><span>B</span></template>"#,
    );
    // Positive: fragment should give empty $attrs
    assert!(
        r.contains("$attrs: {},"),
        "fragment root should have empty $attrs, got:\n{}",
        r
    );
}

#[test]
fn tsc_root_element_attrs_no_template() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
const x = 1
</script>"#,
    );
    // Positive: no template should give empty $attrs
    assert!(
        r.contains("$attrs: {},"),
        "no template should have empty $attrs, got:\n{}",
        r
    );
}

// ── Barrel export type preservation: __OmitNew + Omit<CPI> ──────────────────
//
// Barrel re-exports (`export { default as X } from './X.vue'`) degrade
// `typeof __comp`'s construct signature, picking `DefineComponent<{}>`'s
// empty `$props` over our explicit typed one. The fix:
// 1. `__OmitNew<typeof __comp>` strips the construct sig via mapped type
// 2. A single `new()` returns `Omit<CPI, ...> & { $props: T, $emit: E, ... }`
//    so barrel re-exports have exactly one construct signature.

#[test]
fn tsc_codegen_uses_omit_new_for_barrel_safety() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts">
defineProps<{
  zIndex?: number
  duration?: number | string
  show?: boolean
  lockScroll?: boolean
}>()
</script><template><div /></template>"#,
    );

    eprintln!("\n=== barrel type fix output ===\n{}\n", r);

    // Positive: __OmitNew utility type is emitted
    assert!(
        r.contains("type __OmitNew<T> = { [K in keyof T]: T[K] }"),
        "__OmitNew utility type should be present: got:\n{}",
        r
    );
    // Positive: declare uses __OmitNew<typeof __comp>, not raw typeof __comp
    assert!(
        r.contains("__OmitNew<typeof __comp>"),
        "should use __OmitNew<typeof __comp>: got:\n{}",
        r
    );
    // Negative: raw `typeof __comp &` intersection must NOT appear
    assert!(
        !r.contains(": typeof __comp &"),
        "should NOT use raw typeof __comp in intersection: got:\n{}",
        r
    );
    // Positive: CPI is used as plain intersection in constructor return type
    assert!(
        r.contains("new(props?: import(\"vue\").PublicProps &"),
        "constructor should accept props with PublicProps: got:\n{}",
        r
    );
    // Negative: no more Omit<CPI<...>> wrapping
    assert!(
        !r.contains("Omit<import(\"vue\").ComponentPublicInstance<"),
        "should NOT use Omit<CPI<...>> pattern: got:\n{}",
        r
    );
    // Positive: $props includes PublicProps for class/style/key
    assert!(
        r.contains("$props: import(\"vue\").PublicProps &"),
        "$props should include PublicProps intersection: got:\n{}",
        r
    );
    // Positive: explicit $props still has typed fields
    assert!(
        r.contains("zIndex?: number"),
        "$props should have typed zIndex: got:\n{}",
        r
    );
    assert!(
        r.contains("show?: boolean"),
        "$props should have typed show: got:\n{}",
        r
    );
}

#[test]
fn tsc_codegen_generic_uses_omit_new() {
    let r = gen_tsc_props(
        r#"<script setup lang="ts" generic="T">
defineProps<{ items: T[] }>()
</script><template/>"#,
    );

    // Positive: generic on new() with props param
    assert!(
        r.contains("new<T>(props?: import(\"vue\").PublicProps &"),
        "generic new() should accept props with PublicProps: got:\n{}",
        r
    );
    // Negative: no more Omit<CPI<...>> wrapping
    assert!(
        !r.contains("Omit<import(\"vue\").ComponentPublicInstance<"),
        "should NOT use Omit<CPI<...>> pattern: got:\n{}",
        r
    );
    assert!(
        r.contains("__OmitNew<typeof __comp>"),
        "should use __OmitNew: got:\n{}",
        r
    );
}

// ── Conditional root narrowing ──────────────────────────────────────────────

#[test]
fn tsc_narrowing_basic() {
    let r = gen_tsc_narrowing_props(
        r#"<script setup lang="ts">
defineProps<{foo?: boolean}>()
</script>
<template><div v-if="foo">A</div><span v-else>B</span></template>"#,
        &[authored_prop("foo", true, "boolean", 0, 0)],
    );
    // Positive: narrowing generic on new()
    assert!(
        r.contains("T_foo extends boolean = boolean"),
        "should have T_foo generic: {r}"
    );
    // Positive: $props uses generic type
    assert!(
        r.contains("foo?: T_foo"),
        "should substitute generic in $props: {r}"
    );
    // Positive: $root with conditional type
    assert!(
        r.contains("$root: T_foo extends true ? HTMLDivElement : HTMLSpanElement"),
        "$root should have conditional type: {r}"
    );
}

#[test]
fn tsc_narrowing_multi() {
    let r = gen_tsc_narrowing_props(
        r#"<script setup lang="ts">
defineProps<{foo?: boolean, s?: 'foo' | 'bar'}>()
</script>
<template><div v-if="foo">A</div><span v-else-if="s === 'foo'">B</span><canvas v-else-if="s === 'bar'">C</canvas><input v-else /></template>"#,
        &[
            authored_prop("foo", true, "boolean", 0, 0),
            authored_prop("s", true, "'foo' | 'bar'", 0, 1),
        ],
    );
    assert!(
        r.contains("T_foo extends boolean = boolean"),
        "should have T_foo: {r}"
    );
    assert!(
        r.contains("T_s extends 'foo' | 'bar' = 'foo' | 'bar'"),
        "should have T_s: {r}"
    );
    assert!(r.contains("$root:"), "should have $root: {r}");
}

#[test]
fn tsc_narrowing_with_sfc_generics() {
    let r = gen_tsc_narrowing_props(
        r#"<script setup lang="ts" generic="T extends string">
defineProps<{show?: boolean}>()
</script>
<template><div v-if="show">A</div><span v-else>B</span></template>"#,
        &[authored_prop("show", true, "boolean", 0, 0)],
    );
    // Both existing generic and narrowing generic
    assert!(
        r.contains("T extends string, T_show extends boolean = boolean"),
        "should append narrowing to existing generics: {r}"
    );
}

#[test]
fn tsc_narrowing_disabled() {
    // Use default (narrowing disabled)
    let r = gen_tsc_props(
        r#"<script setup lang="ts">
defineProps<{foo?: boolean}>()
</script>
<template><div v-if="foo">A</div><span v-else>B</span></template>"#,
    );
    assert!(
        !r.contains("T_foo"),
        "should NOT have narrowing when disabled: {r}"
    );
    assert!(
        !r.contains("$root"),
        "should NOT have $root when disabled: {r}"
    );
}

#[test]
fn tsc_narrowing_component_roots() {
    let r = gen_tsc_narrowing_props(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import Other from './Other.vue'
defineProps<{v?: 'a' | 'b'}>()
</script>
<template><MyComp v-if="v === 'a'" /><Other v-else /></template>"#,
        &[authored_prop("v", true, "'a' | 'b'", 0, 0)],
    );
    assert!(r.contains("T_v extends"), "should have T_v generic: {r}");
    assert!(
        r.contains("InstanceType<typeof MyComp>"),
        "$root should use InstanceType for components: {r}"
    );
    assert!(
        r.contains("InstanceType<typeof Other>"),
        "$root should use InstanceType for Other: {r}"
    );
}

// ── Emits-to-props: emit events should appear as onEventName in $props ───────

#[test]
fn tsc_codegen_emits_array_to_props() {
    let r = gen_tsc(
        r#"<script setup>
defineEmits(['change', 'clickOverlay'])
</script><template/>"#,
    );

    // Positive: emit events become onEventName props
    assert!(
        r.contains(r#""onChange"?:"#),
        "should have onChange in $props: {r}"
    );
    assert!(
        r.contains(r#""onClickOverlay"?:"#),
        "should have onClickOverlay in $props: {r}"
    );
}

#[test]
fn tsc_codegen_typed_emits_to_props() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'click', event: MouseEvent): void }>()
</script><template/>"#,
        &[emits_fixture(&[authored_emit(
            "click",
            "event: MouseEvent",
            "event: MouseEvent",
            0,
            0,
        )])],
    );

    assert!(
        r.contains(r#""onClick"?: (event: MouseEvent) => void"#),
        "type-based emits should inline handler props with preserved payload types: {r}"
    );
    assert!(
        r.contains(r#"((event: "click", event: MouseEvent) => void)"#),
        "type-based emits should inline $emit overloads: {r}"
    );
}

#[test]
fn tsc_codegen_emits_and_models_props() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'submit', data: string): void }>()
const model = defineModel<string>()
</script><template/>"#,
        &[
            emits_fixture_at(
                0,
                &[authored_emit(
                    "submit",
                    "data: string",
                    "data: string",
                    0,
                    0,
                )],
                &[],
                &[],
            ),
            model_fixture_at(1, "modelValue", "string", &[], &[]),
        ],
    );

    assert!(
        r.contains(r#""onSubmit"?: (data: string) => void"#),
        "type-based emits should contribute inline handler props: {r}"
    );
    assert!(
        r.contains("modelValue?:"),
        "should have modelValue prop: {r}"
    );
    assert!(
        r.contains(r#""onUpdate:modelValue"?:"#),
        "should have onUpdate:modelValue prop: {r}"
    );
}

// ── Kebab-case emit → dual $props keys ──────────────────────────────────────

// Type-based defineEmits: handler types should be inferred from the original type
// via __EmitToProps<OriginalType> rather than manual (...args: unknown[]) => void
#[test]
fn tsc_codegen_kebab_emit_type_based_both_keys_with_correct_handler() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'my-event', value: string): void }>()
</script><template/>"#,
        &[emits_fixture(&[authored_emit(
            "my-event",
            "value: string",
            "value: string",
            0,
            0,
        )])],
    );

    assert!(
        r.contains(r#""onMy-event"?: (value: string) => void"#),
        "kebab emits should generate the kebab handler key inline: {r}"
    );
    assert!(
        r.contains(r#""onMyEvent"?: (value: string) => void"#),
        "kebab emits should also generate the camel handler key inline: {r}"
    );
    assert!(
        !r.contains("__EmitToProps<") && !r.contains("type __Cam<"),
        "inline emits should not rely on helper aliases anymore: {r}"
    );
}

// Type-based defineEmits with multi-segment kebab
#[test]
fn tsc_codegen_multi_segment_kebab_emit_type_based() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'my-custom-event'): void }>()
</script><template/>"#,
        &[emits_fixture(&[authored_emit(
            "my-custom-event",
            "",
            "",
            0,
            0,
        )])],
    );

    assert!(
        r.contains(r#""onMy-custom-event"?: () => void"#),
        "multi-segment kebab emits should keep the kebab alias: {r}"
    );
    assert!(
        r.contains(r#""onMyCustomEvent"?: () => void"#),
        "multi-segment kebab emits should also generate the camel alias: {r}"
    );
}

// Object-syntax defineEmits: handler type inferred from validator params
#[test]
fn tsc_codegen_kebab_emit_object_syntax_both_keys() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits({
  'my-event': (value: string) => true,
  'click': (id: number) => true,
})
</script><template/>"#,
    );

    assert!(
        r.contains(r#""onMy-event"?: (value: string) => void"#),
        "object emits should inline the kebab handler key: {r}"
    );
    assert!(
        r.contains(r#""onMyEvent"?: (value: string) => void"#),
        "object emits should inline the camel handler key: {r}"
    );
    assert!(
        r.contains(r#"((event: "my-event", value: string) => void)"#),
        "object emits should inline $emit overloads from validator params: {r}"
    );
}

// Array-syntax defineEmits
#[test]
fn tsc_codegen_kebab_emit_array_syntax_both_keys() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits(['my-event', 'click'])
</script><template/>"#,
    );

    // kebab event → both keys
    assert!(
        r.contains(r#""onMy-event"?:"#),
        "should have capitalize-only onMy-event prop: {r}"
    );
    assert!(
        r.contains(r#""onMyEvent"?:"#),
        "should have camelized onMyEvent prop: {r}"
    );
    // non-kebab event → single key per props block (appears in both new() param and $props)
    assert!(
        r.contains(r#""onClick"?:"#),
        "should have onClick prop: {r}"
    );
    let count = r.matches(r#""onClick"?:"#).count();
    assert_eq!(
        count, 2,
        "non-kebab emit should produce exactly one key per props block (2 total: new() + $props): {r}"
    );
}

// camelCase emit (type-based) → camel + kebab handler aliases
#[test]
fn tsc_codegen_camel_emit_no_duplicate_prop() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'myEvent'): void }>()
</script><template/>"#,
        &[emits_fixture(&[authored_emit("myEvent", "", "", 0, 0)])],
    );

    assert!(
        r.contains(r#""onMyEvent"?: () => void"#),
        "camel emits should keep the camel handler key: {r}"
    );
    assert!(
        r.contains(r#""onMy-event"?: () => void"#),
        "camel emits should also generate the kebab handler key: {r}"
    );
}

// Simple emit (type-based) → single deduped handler key per props block
#[test]
fn tsc_codegen_simple_emit_no_duplicate_prop() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'click'): void }>()
</script><template/>"#,
        &[emits_fixture(&[authored_emit("click", "", "", 0, 0)])],
    );

    let count = r.matches(r#""onClick"?: () => void"#).count();
    assert_eq!(
        count, 2,
        "simple emits should only produce one deduped handler key in new() and $props: {r}"
    );
}

// update: prefix (type-based) → colon form only
#[test]
fn tsc_codegen_update_prefix_emit_no_camelize() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'update:modelValue'): void }>()
</script><template/>"#,
        &[emits_fixture(&[authored_emit(
            "update:modelValue",
            "",
            "",
            0,
            0,
        )])],
    );

    assert!(
        r.contains(r#""onUpdate:modelValue"?: () => void"#),
        "colon emits should keep the colon handler key: {r}"
    );
    assert!(
        !r.contains("onUpdateModelValue"),
        "colon emits should not generate camelized aliases: {r}"
    );
}

// ── Shorthand type-based defineEmits: $emit + $props typing ──────────────────

#[test]
fn tsc_codegen_shorthand_emits_emit_type_uses_emit_fn() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineEmits<{
  change: [value: string];
  update: [id: number, data: { name: string }];
}>()
</script><template/>"#,
        &[emits_fixture(&[
            authored_emit(
                "change",
                "...args: [value: string]",
                "...args: [value: string]",
                0,
                0,
            ),
            authored_emit(
                "update",
                "...args: [id: number, data: { name: string }]",
                "...args: [id: number, data: { name: string }]",
                0,
                1,
            ),
        ])],
    );

    assert!(
        r.contains(r#"((event: "change", ...args: [value: string]) => void)"#),
        "shorthand emits should inline tuple overloads in $emit: {r}"
    );
    assert!(
        r.contains(r#"((event: "update", ...args: [id: number, data: { name: string }]) => void)"#),
        "shorthand emits should preserve tuple payload text in $emit: {r}"
    );
    assert!(
        !r.contains("__EmitFn<"),
        "helper emit aliases should be gone: {r}"
    );
}

#[test]
fn tsc_codegen_shorthand_emits_props_uses_emit_to_props() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineEmits<{
  change: [value: string];
}>()
</script><template/>"#,
        &[emits_fixture(&[authored_emit(
            "change",
            "...args: [value: string]",
            "...args: [value: string]",
            0,
            0,
        )])],
    );

    assert!(
        r.contains(r#""onChange"?: (...args: [value: string]) => void"#),
        "shorthand emits should inline tuple handler props: {r}"
    );
}

// Function-form type-based: $emit should also inline overloads
#[test]
fn tsc_codegen_function_form_emits_emit_type_uses_emit_fn() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'click', event: MouseEvent): void }>()
</script><template/>"#,
        &[emits_fixture(&[authored_emit(
            "click",
            "event: MouseEvent",
            "event: MouseEvent",
            0,
            0,
        )])],
    );

    assert!(
        r.contains(r#"((event: "click", event: MouseEvent) => void)"#),
        "function-form emits should inline $emit overloads: {r}"
    );
    assert!(
        !r.contains("__EmitFn<"),
        "helper emit aliases should be gone: {r}"
    );
}

// ── Object-arg defineEmits: $emit + $props typing ────────────────────────────

#[test]
fn tsc_codegen_object_arg_emits_uses_type_helpers() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineEmits({
  change: (value: string) => true,
  submit: null,
})
</script><template/>"#,
    );

    assert!(
        r.contains(r#""onChange"?: (value: string) => void"#),
        "object-arg emits should inline handler props: {r}"
    );
    assert!(
        r.contains(r#"((event: "submit", ...args: unknown[]) => void)"#),
        "null validators should fall back to unknown[] in $emit: {r}"
    );
    assert!(
        !r.contains("__EmitToProps<") && !r.contains("__EmitFn<"),
        "object-arg emits should no longer use helper aliases: {r}"
    );
}

#[test]
fn tsc_sourcemap_emit_handler_prop_maps_to_event_name() {
    let sfc = r#"<script setup lang="ts">
defineEmits<{ (e: 'my-event', value: string): void }>()
</script><template/>"#;
    let out = gen_tsc_output_with(
        sfc,
        &[emits_fixture(&[authored_emit(
            "my-event",
            "value: string",
            "value: string",
            0,
            0,
        )])],
    );
    let sourcemap = SourceMap::from_json_string(&out.source_map).expect("valid source map");
    let lookup = sourcemap.generate_lookup_table();
    let generated_offset = out
        .code
        .find(r#""onMyEvent""#)
        .expect("generated handler prop");
    let (generated_line, generated_col) =
        offset_to_zero_based_line_col(&out.code, generated_offset);
    let token = sourcemap
        .lookup_source_view_token(&lookup, generated_line, generated_col)
        .expect("mapped handler prop token");
    let expected_offset = sfc.find("'my-event'").expect("source event literal");
    let (expected_line, expected_col) = offset_to_zero_based_line_col(sfc, expected_offset);

    assert_eq!(token.get_source(), Some("/test/TestComp.vue"));
    assert_eq!(token.get_src_line(), expected_line);
    assert_eq!(token.get_src_col(), expected_col);
}

#[test]
fn tsc_sourcemap_prop_key_maps_to_prop_name() {
    let sfc = r#"<script setup lang="ts">
defineProps<{ title: string; count?: number }>()
</script><template/>"#;
    let out = gen_tsc_output_with(
        sfc,
        &[props_fixture(
            "",
            &[
                authored_prop("title", false, "string", 0, 0),
                authored_prop("count", true, "number", 0, 1),
            ],
        )],
    );
    let sourcemap = SourceMap::from_json_string(&out.source_map).expect("valid source map");
    let lookup = sourcemap.generate_lookup_table();
    let generated_offset = out.code.find("title: string").expect("generated prop");
    let (generated_line, generated_col) =
        offset_to_zero_based_line_col(&out.code, generated_offset);
    let token = sourcemap
        .lookup_source_view_token(&lookup, generated_line, generated_col)
        .expect("mapped prop token");
    let expected_offset = sfc.find("title: string").expect("source prop");
    let (expected_line, expected_col) = offset_to_zero_based_line_col(sfc, expected_offset);

    assert_eq!(token.get_source(), Some("/test/TestComp.vue"));
    assert_eq!(token.get_src_line(), expected_line);
    assert_eq!(token.get_src_col(), expected_col);
}

#[test]
fn tsc_sourcemap_model_members_map_to_model_name() {
    let sfc = r#"<script setup lang="ts">
const title = defineModel<string>('title')
</script><template/>"#;
    let out = gen_tsc_output_with(sfc, &[model_fixture("title", "string")]);
    let sourcemap = SourceMap::from_json_string(&out.source_map).expect("valid source map");
    let lookup = sourcemap.generate_lookup_table();
    let generated_offset = out
        .code
        .find(r#""onUpdate:title""#)
        .expect("generated model handler");
    let (generated_line, generated_col) =
        offset_to_zero_based_line_col(&out.code, generated_offset);
    let token = sourcemap
        .lookup_source_view_token(&lookup, generated_line, generated_col)
        .expect("mapped model token");
    let expected_offset = sfc.find("'title'").expect("source model name");
    let (expected_line, expected_col) = offset_to_zero_based_line_col(sfc, expected_offset);

    assert_eq!(token.get_source(), Some("/test/TestComp.vue"));
    assert_eq!(token.get_src_line(), expected_line);
    assert_eq!(token.get_src_col(), expected_col);
}

#[test]
fn class_declaration_sourcemap_maps_preserved_authored_text_but_not_inferred_replacements() {
    let source = r#"<script setup lang="ts">
class Payload { value = 1 }
defineProps<Payload>()
</script>"#;
    let bundle = props_bundle_with_scope(TscScopeRequirements {
        owner_value_dependencies: Vec::new(),
        retained_bindings: Vec::new(),
        dependency_declarations: vec![dependency(
            "Payload",
            vec![inferred_class_member(
                "value",
                0,
                false,
                TscInferredClassTypePosition::Property,
                "number",
            )],
        )],
    });
    let output = generate_with_bundle(source, TscMode::Declaration, &bundle).unwrap();
    let map = SourceMap::from_json_string(&output.source_map).expect("valid source map");
    let lookup = map.generate_lookup_table();
    let generated_class_start = output
        .code
        .find("class Payload")
        .expect("preserved generated class");
    let source_class_start = source
        .find("class Payload")
        .expect("preserved source class");
    let inferred_offset = generated_class_start
        + output.code[generated_class_start..]
            .find("number")
            .expect("inferred type text");
    let inferred_position = offset_to_zero_based_line_col(&output.code, inferred_offset);

    for needle in ["class Payload", "value", " }"] {
        let generated_offset = generated_class_start
            + output.code[generated_class_start..]
                .find(needle)
                .expect("preserved generated neighbor");
        let source_offset = source_class_start
            + source[source_class_start..]
                .find(needle)
                .expect("preserved source neighbor");
        let generated_position = offset_to_zero_based_line_col(&output.code, generated_offset);
        let source_position = offset_to_zero_based_line_col(source, source_offset);
        let token = map
            .lookup_source_view_token(&lookup, generated_position.0, generated_position.1)
            .expect("preserved neighbor mapping");
        assert_eq!(token.get_source(), Some("/test/TestComp.vue"));
        assert_eq!(
            (token.get_src_line(), token.get_src_col()),
            source_position,
            "{needle:?} must retain its exact authored line/column"
        );
    }

    let inferred_token = map
        .lookup_source_view_token(&lookup, inferred_position.0, inferred_position.1)
        .expect("explicit generated-text reset");
    assert!(inferred_token.get_source_id().is_none());
    assert_eq!(
        (inferred_token.get_dst_line(), inferred_token.get_dst_col()),
        inferred_position,
        "the unmapped replacement reset must begin exactly at the inferred text"
    );
}

// ── Extract + Generate cache equivalence tests ───────────────────────────

use super::script::{extract_tsc_state, generate_tsc_from_state, TscExtractOptions};

#[test]
fn extract_then_generate_matches_direct_for_inline_types() {
    let sfc = r#"<script setup lang="ts">
defineProps<{ x: number; y?: string }>()
defineEmits<{ (e: 'change', val: number): void }>()
</script>
<template><div /></template>"#;

    let extracted = extract_tsc_state(
        sfc,
        "TestComp",
        &TscExtractOptions {
            filename: Some("/test/TestComp.vue".to_string()),
        },
    )
    .expect("should extract from SFC with script setup");

    let bundle = fixture_bundle(&[
        props_fixture(
            "{ x: number; y?: string }",
            &[
                authored_prop("x", false, "number", 0, 0),
                authored_prop("y", true, "string", 0, 1),
            ],
        ),
        emits_fixture_at(
            1,
            &[authored_emit("change", "val: number", "val: number", 1, 0)],
            &[],
            &[],
        ),
    ]);
    let from_cache = generate_tsc_from_state(
        &extracted,
        "TestComp",
        TscMode::Public,
        MacroTscInput::Authoritative(&bundle),
        &FallthroughPropsProjection::none(),
    )
    .expect("explicit semantic DTO");
    let direct = generate_tsc_output_with_options(
        sfc,
        "TestComp",
        &TscGenOptions {
            filename: Some("/test/TestComp.vue".to_string()),
            ..Default::default()
        },
        MacroTscInput::Authoritative(&bundle),
        &FallthroughPropsProjection::none(),
    )
    .expect("explicit semantic DTO");

    assert_eq!(from_cache.code, direct.code);
    assert_eq!(from_cache.source_map, direct.source_map);
}

#[test]
fn extract_then_generate_matches_direct_for_runtime_macros() {
    let sfc = r#"<script setup lang="ts">
defineProps({ x: String, count: { type: Number, default: 0 } })
defineEmits(['click', 'update'])
</script>
<template><div /></template>"#;

    let extracted = extract_tsc_state(
        sfc,
        "TestComp",
        &TscExtractOptions {
            filename: Some("/test/TestComp.vue".to_string()),
        },
    )
    .expect("should extract from SFC with runtime macros");

    let from_cache = generate_tsc_from_state(
        &extracted,
        "TestComp",
        TscMode::Public,
        MacroTscInput::NotRequired,
        &FallthroughPropsProjection::none(),
    )
    .expect("runtime macros require no semantic DTO");
    let direct = gen_tsc(sfc);

    assert_eq!(
        from_cache.code, direct,
        "cached path must match direct for runtime macros"
    );
}

#[test]
fn extract_returns_none_without_script_setup() {
    let sfc = r#"<template><div>hello</div></template>"#;

    let result = extract_tsc_state(sfc, "TestComp", &TscExtractOptions::default());
    assert!(
        result.is_none(),
        "should return None for SFC without script setup"
    );
}

// ── TSC output with dotted component names ──────────────────────────

fn assert_valid_tsc_output(source: &str, name: &str, props: &[FixturePropRow<'_>]) {
    let bundle = fixture_bundle(&[props_fixture("", props)]);
    let tsc_out = generate_tsc_output_with_options(
        source,
        name,
        &TscGenOptions::default(),
        MacroTscInput::Authoritative(&bundle),
        &FallthroughPropsProjection::none(),
    )
    .expect("explicit fixture matches typed macro");
    let code = &tsc_out.code;
    eprintln!("=== TSC {} ===\n{}\n=== END ===", name, code);

    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("[TSC {name}] OXC ERROR: {err}");
    }
    assert!(
        parsed.errors.is_empty(),
        "[TSC {name}] should have no parse errors. Got {} errors. Output:\n{code}",
        parsed.errors.len()
    );
}

#[test]
fn tsc_dotted_component_name_sanitized() {
    // Dotted names like "Drawer.draggable" produce invalid identifiers in
    // `declare const Drawer.draggable:` — dots must be replaced with underscores.
    let source = r#"<script setup lang="ts">
defineProps<{ open: boolean }>()
</script>
<template><div>hello</div></template>"#;
    assert_valid_tsc_output(
        source,
        "Drawer.draggable",
        &[authored_prop("open", false, "boolean", 0, 0)],
    );
}

#[test]
fn tsc_multi_dotted_component_name_sanitized() {
    // Multiple dots: "SwiperCardStyle.story.component"
    let source = r#"<script setup lang="ts">
defineProps<{ count: number }>()
</script>
<template><div>{{ count }}</div></template>"#;
    assert_valid_tsc_output(
        source,
        "SwiperCardStyle.story.component",
        &[authored_prop("count", false, "number", 0, 0)],
    );
}

#[test]
fn tsc_dotted_name_produces_valid_identifiers() {
    let source = r#"<script setup lang="ts">
defineProps<{ value: string }>()
</script>
<template><div>{{ value }}</div></template>"#;
    let bundle = fixture_bundle(&[props_fixture(
        "",
        &[authored_prop("value", false, "string", 0, 0)],
    )]);
    let tsc_out = generate_tsc_output_with_options(
        source,
        "My.Component.Name",
        &TscGenOptions::default(),
        MacroTscInput::Authoritative(&bundle),
        &FallthroughPropsProjection::none(),
    )
    .expect("explicit fixture matches typed macro");
    let code = &tsc_out.code;

    // The output must NOT contain bare `My.Component.Name` as an identifier
    assert!(
        !code.contains("const My.Component.Name"),
        "dotted name must be sanitized in const declaration: {code}"
    );
    assert!(
        !code.contains("default My.Component.Name"),
        "dotted name must be sanitized in export default: {code}"
    );
    // It SHOULD contain the sanitized version
    assert!(
        code.contains("My_Component_Name"),
        "dotted name should be sanitized to underscores: {code}"
    );
}

// ── defineExpose ──────────────────────────────────────────────────────────────

#[test]
fn tsc_codegen_define_expose_object_arg() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const foo = ref(1)
const bar = ref('hello')
function baz() {}
defineExpose({ foo, bar, baz })
</script><template/>"#,
    );

    // Positive: shorthand props use typeof inference via ShallowUnwrapRef
    assert!(
        r.contains("foo: typeof foo"),
        "should have foo: typeof foo in return type: {r}"
    );
    assert!(
        r.contains("bar: typeof bar"),
        "should have bar: typeof bar in return type: {r}"
    );
    assert!(
        r.contains("baz: typeof baz"),
        "should have baz: typeof baz in return type: {r}"
    );
    assert!(
        r.contains("ShallowUnwrapRef"),
        "should use ShallowUnwrapRef wrapper: {r}"
    );
}

#[test]
fn tsc_codegen_define_expose_empty() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineExpose()
</script><template/>"#,
    );

    // Should produce valid output with no extra properties
    assert!(r.contains("new("), "should have constructor in output: {r}");
    assert!(
        !r.contains("defineExpose("),
        "defineExpose call should be removed from output: {r}"
    );
}

#[test]
fn tsc_codegen_define_expose_type_param() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
const foo = ref(1)
const bar = ref('hello')
defineExpose<{ foo: number, bar: string }>({ foo, bar })
</script><template/>"#,
    );

    // Type param wins: intersection with the type text
    assert!(
        r.contains("{ foo: number, bar: string }"),
        "should have type text as intersection on return type: {r}"
    );
    // Should NOT have individual `foo: any` — type param covers it
    assert!(
        !r.contains("foo: any"),
        "should not have individual foo: any when type param present: {r}"
    );
    assert!(
        !r.contains("defineExpose("),
        "defineExpose call should be removed from output: {r}"
    );
}

#[test]
fn tsc_codegen_define_expose_non_object() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
const obj = { foo: 1 }
defineExpose(obj)
</script><template/>"#,
    );

    // Can't extract names from a variable reference — no exposed properties
    assert!(
        !r.contains("foo: any"),
        "should not have foo: any for non-object arg: {r}"
    );
    assert!(r.contains("new("), "should have constructor in output: {r}");
}

#[test]
fn tsc_codegen_define_expose_method() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
function greet(name: string) { return `Hello, ${name}!` }
defineExpose({ greet })
</script><template/>"#,
    );

    // Shorthand property with function identifier — uses typeof
    assert!(
        r.contains("greet: typeof greet"),
        "should have greet: typeof greet in return type: {r}"
    );
}

#[test]
fn tsc_codegen_define_expose_computed_key() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineExpose({ foo, bar: computed(() => 1), baz: 'literal' })
</script><template/>"#,
    );

    // foo is shorthand → typeof, others are complex → any
    assert!(
        r.contains("foo: typeof foo"),
        "should have foo: typeof foo: {r}"
    );
    assert!(r.contains("bar: any"), "should have bar: any: {r}");
    assert!(r.contains("baz: any"), "should have baz: any: {r}");
}

// ── defineExpose with typeof inference ────────────────────────────────────────

#[test]
fn tsc_define_expose_shorthand_uses_typeof() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { ref, computed } from 'vue'
const foo = ref(1)
const bar = computed(() => 'hello')
defineExpose({ foo, bar })
</script><template/>"#,
    );

    // Positive: typeof inference via ShallowUnwrapRef
    assert!(
        r.contains("typeof foo"),
        "should use typeof for shorthand foo: {r}"
    );
    assert!(
        r.contains("typeof bar"),
        "should use typeof for shorthand bar: {r}"
    );
    assert!(
        r.contains("ShallowUnwrapRef"),
        "should use ShallowUnwrapRef wrapper: {r}"
    );
    // Negative: must NOT fall back to `any`
    assert!(
        !r.contains("foo: any"),
        "should NOT use any for shorthand foo: {r}"
    );
    assert!(
        !r.contains("bar: any"),
        "should NOT use any for shorthand bar: {r}"
    );
}

#[test]
fn tsc_define_expose_non_shorthand_ident_uses_typeof() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const val = ref(42)
defineExpose({ myVal: val })
</script><template/>"#,
    );

    assert!(
        r.contains("myVal: typeof val"),
        "should use typeof for identifier value: {r}"
    );
    assert!(
        !r.contains("myVal: any"),
        "should NOT use any for identifier value: {r}"
    );
}

#[test]
fn tsc_define_expose_method_shorthand_falls_back() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineExpose({ focus() {} })
</script><template/>"#,
    );

    assert!(
        r.contains("focus: any"),
        "method shorthand should fall back to any: {r}"
    );
}

#[test]
fn tsc_define_expose_complex_value_falls_back() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { computed } from 'vue'
defineExpose({ bar: computed(() => 1) })
</script><template/>"#,
    );

    assert!(
        r.contains("bar: any"),
        "complex expression value should fall back to any: {r}"
    );
}

#[test]
fn tsc_define_expose_mixed_shorthand_and_complex() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { ref, computed } from 'vue'
const x = ref(1)
defineExpose({ x, y: computed(() => 2) })
</script><template/>"#,
    );

    assert!(r.contains("typeof x"), "shorthand x should use typeof: {r}");
    assert!(
        r.contains("y: any"),
        "complex y should fall back to any: {r}"
    );
}

#[test]
fn tsc_define_expose_type_param_unchanged() {
    // Regression: type-param form must continue to use the intersection type
    let r = gen_tsc(
        r#"<script setup lang="ts">
const foo = ref(1)
defineExpose<{ foo: number }>({ foo })
</script><template/>"#,
    );

    assert!(
        r.contains("{ foo: number }"),
        "type-param form should use the type text directly: {r}"
    );
    assert!(
        !r.contains("ShallowUnwrapRef"),
        "type-param form should NOT use ShallowUnwrapRef: {r}"
    );
}

#[test]
fn tsc_define_expose_empty_unchanged() {
    // Regression: empty defineExpose should not emit ShallowUnwrapRef
    let r = gen_tsc(
        r#"<script setup lang="ts">
defineExpose()
</script><template/>"#,
    );

    assert!(
        !r.contains("ShallowUnwrapRef"),
        "empty expose should NOT use ShallowUnwrapRef: {r}"
    );
}

#[test]
fn tsc_define_expose_includes_setup_content() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const foo = ref(1)
defineExpose({ foo })
</script><template/>"#,
    );

    // Script body must be present so typeof can resolve
    assert!(
        r.contains("const foo = ref(1)"),
        "output should include the script setup body: {r}"
    );
    // Macro stubs must be present so defineExpose doesn't error
    assert!(
        r.contains("declare function defineExpose"),
        "output should include macro stubs: {r}"
    );
}

// ── Dual-script JS SFC (verter-tsc path) ────────────────────────

#[test]
fn tsc_dual_script_js_sfc_basic() {
    let r = gen_tsc(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<script>
export default {
  inheritAttrs: false,
}
</script>
<template><div>{{ count }}</div></template>"#,
    );

    // Should still produce valid TSC output
    assert!(
        r.contains("defineComponent"),
        "should have defineComponent call:\n{r}"
    );
    assert!(
        r.contains("export default"),
        "should have export default:\n{r}"
    );

    // Should NOT contain raw script tags
    assert!(!r.contains("<script"), "script tags must not appear:\n{r}");
    assert!(
        !r.contains("</script>"),
        "close script tags must not appear:\n{r}"
    );

    // Should parse as valid TypeScript
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &r, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC TSC ERROR: {err}");
    }
    assert!(
        parsed.errors.is_empty(),
        "generated TSC output should have no parse errors, got {}:\n{r}",
        parsed.errors.len()
    );
}

#[test]
fn tsc_dual_script_js_vuetify_figure_pattern() {
    let r = gen_tsc(
        r#"<template>
  <figure>
    <figcaption v-if="caption" v-text="caption" />
    <slot v-else />
  </figure>
</template>

<script setup>
  import { computed, useAttrs } from 'vue'

  const attrs = useAttrs()

  defineProps({
    name: String,
  })

  const caption = computed(() => attrs.title === 'null' ? null : attrs.title)
</script>

<script>
  export default {
    inheritAttrs: false,
  }
</script>"#,
    );

    // Should generate valid TSC output with props
    assert!(
        r.contains("defineComponent"),
        "should have defineComponent:\n{r}"
    );
    assert!(r.contains("name:"), "should have name prop:\n{r}");

    // Should NOT contain script tags or companion export default content
    assert!(!r.contains("<script"), "script tags must not appear:\n{r}");

    // Should parse as valid TypeScript
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &r, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC TSC ERROR: {err}");
    }
    assert!(
        parsed.errors.is_empty(),
        "generated TSC output should have no parse errors, got {}:\n{r}",
        parsed.errors.len()
    );
}

#[test]
fn reserved_word_component_name_is_prefixed() {
    let code = generate_tsc_output_with_options(
        "<template><div>hello</div></template>",
        "default",
        &TscGenOptions::default(),
        MacroTscInput::NotRequired,
        &FallthroughPropsProjection::none(),
    )
    .expect("no typed macros")
    .code;
    assert!(
        code.contains("_default"),
        "reserved word 'default' should be prefixed with _, got:\n{}",
        code
    );
    assert!(
        !code.contains("const default"),
        "should not produce `const default` (reserved word), got:\n{}",
        code
    );
}

#[test]
fn digit_prefix_component_name_is_prefixed() {
    let code = generate_tsc_output_with_options(
        "<template><div>not found</div></template>",
        "404",
        &TscGenOptions::default(),
        MacroTscInput::NotRequired,
        &FallthroughPropsProjection::none(),
    )
    .expect("no typed macros")
    .code;
    assert!(
        code.contains("_404"),
        "digit-prefixed name should get _ prefix, got:\n{}",
        code
    );
    assert!(
        !code.contains("const 404"),
        "should not produce `const 404` (invalid identifier), got:\n{}",
        code
    );
}

// ── Declaration mode (`.d.<ext>.ts`) — declaration-SAFE output ───────────────
//
// `TscMode::Declaration` renders the SAME public surface `TscMode::Public`
// computes, but as a strictly valid `.d.ts`: pure declarations only, NO runtime
// / value code. These tests are DISCRIMINATING — the runtime-token assertions
// fail against the `Public` output (which emits `defineComponent` / `const
// __comp` / `typeof __comp`) and pass only for the declaration path.

fn gen_tsc_declaration(sfc: &str) -> String {
    generate_tsc_output_with_options(
        sfc,
        "TestComp",
        &TscGenOptions {
            filename: Some("/test/TestComp.vue".to_string()),
            mode: TscMode::Declaration,
            ..Default::default()
        },
        MacroTscInput::NotRequired,
        &FallthroughPropsProjection::none(),
    )
    .expect("fixture has no typed codegen macro")
    .code
}

fn gen_tsc_declaration_with(sfc: &str, fixtures: &[TscFixture<'_>]) -> String {
    gen_tsc_mode_with(sfc, TscMode::Declaration, fixtures).code
}

#[test]
fn declaration_mode_emits_no_runtime_value_code() {
    let sfc = r#"<script setup lang="ts">
defineProps<{ msg: string; count?: number }>()
defineEmits<{ change: [value: string] }>()
</script>
<template><div>{{ msg }}</div></template>"#;
    let d = gen_tsc_declaration_with(
        sfc,
        &[
            props_fixture("", &[]),
            emits_fixture_at(
                1,
                &[authored_emit(
                    "change",
                    "...args: [value: string]",
                    "...args: [value: string]",
                    1,
                    0,
                )],
                &[],
                &[],
            ),
        ],
    );

    // NEGATIVE: a `.d.ts` MUST NOT contain any runtime / value code. The
    // `Public` mode emits all of these; the declaration path emits none.
    assert!(
        !d.contains("defineComponent("),
        "declaration must not call defineComponent, got:\n{d}"
    );
    assert!(
        !d.contains("= defineComponent"),
        "declaration must not assign defineComponent, got:\n{d}"
    );
    assert!(
        !d.contains("const __comp"),
        "declaration must not create the runtime __comp const, got:\n{d}"
    );
    assert!(
        !d.contains("typeof __comp"),
        "declaration must not reference typeof a runtime value, got:\n{d}"
    );
    assert!(
        !d.contains("import { defineComponent }"),
        "declaration must not value-import defineComponent, got:\n{d}"
    );

    // POSITIVE: it IS a declaration with an exported default value.
    assert!(
        d.contains("declare const TestComp"),
        "declaration declares the component value, got:\n{d}"
    );
    assert!(
        d.contains("export default TestComp"),
        "declaration exports the component as default, got:\n{d}"
    );
}

#[test]
fn declaration_mode_preserves_the_public_props_surface() {
    let sfc = r#"<script setup lang="ts">
defineProps<{ msg: string; count?: number }>()
</script>
<template><div>{{ msg }}</div></template>"#;
    let d = gen_tsc_declaration_with(sfc, &[props_fixture("", &[])]);

    // The declaration must carry the SAME public `$props` surface the Public
    // mode computes (rendered as an explicit declaration). The prop names and
    // optionality must survive into the `new()`/`$props` shape.
    assert!(
        d.contains("$props"),
        "declaration carries $props, got:\n{d}"
    );
    assert!(d.contains("msg"), "required prop `msg` survives, got:\n{d}");
    assert!(
        d.contains("count?"),
        "optional prop `count?` survives, got:\n{d}"
    );
}

#[test]
fn declaration_mode_preserves_type_only_imports_but_drops_value_import() {
    let sfc = r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(
        sfc,
        &[props_root_fixture_at(0, "Props", &[], &["Props"], &[])],
    );

    // Type-only imports are declaration-legal and MUST survive.
    assert!(
        d.contains("import type { Props } from './types'"),
        "type-only import survives in the declaration, got:\n{d}"
    );
    // The vue runtime value import must NOT appear.
    assert!(
        !d.contains("import { defineComponent }"),
        "declaration must not emit the defineComponent value import, got:\n{d}"
    );
}

#[test]
fn declaration_mode_has_no_macro_stubs_or_setup_body_even_with_expose() {
    // `defineExpose` is exactly the case the Public mode emits the setup body +
    // macro stubs (to resolve `typeof` over exposed bindings). The declaration
    // path must render the expose surface as an explicit type WITHOUT any
    // executable body.
    let sfc = r#"<script setup lang="ts">
const internal = 1
defineExpose({ internal })
</script>
<template><div>hi</div></template>"#;
    let d = gen_tsc_declaration(sfc);

    assert!(
        !d.contains("declare function defineProps"),
        "declaration must not emit macro stubs, got:\n{d}"
    );
    assert!(
        !d.contains("const internal = 1"),
        "declaration must not emit the script-setup executable body, got:\n{d}"
    );
    assert!(
        !d.contains("defineExpose("),
        "declaration must not call defineExpose, got:\n{d}"
    );
    assert!(
        d.contains("declare const TestComp"),
        "declaration still declares the component, got:\n{d}"
    );
}

#[test]
fn declaration_mode_empty_sfc_is_declaration_safe() {
    // An SFC with no `<script setup>` and no `<script>` falls to the empty
    // stub. The `Public`/default path emits a RUNTIME stub (`const __comp =
    // defineComponent({})`, `typeof __comp`); the declaration path must NOT.
    let d = gen_tsc_declaration("<template><div>hello</div></template>");
    assert!(
        !d.contains("defineComponent("),
        "empty-SFC declaration must not call defineComponent, got:\n{d}"
    );
    assert!(
        !d.contains("const __comp"),
        "empty-SFC declaration must not create the runtime __comp const, got:\n{d}"
    );
    assert!(
        !d.contains("typeof __comp"),
        "empty-SFC declaration must not reference typeof a runtime value, got:\n{d}"
    );
    assert!(
        d.contains("declare const TestComp") && d.contains("export default TestComp"),
        "empty-SFC declaration still declares + default-exports the component, got:\n{d}"
    );
}

#[test]
fn declaration_mode_options_api_sfc_is_declaration_safe() {
    // An Options-API `<script>` (no `<script setup>`) falls to the
    // options-API stub, which in the runtime path wraps the default export in
    // `defineComponent(...)`. The declaration path must NOT emit that runtime
    // wrapper or value import.
    let sfc = r#"<script>
export default { name: 'Foo', props: { msg: String } }
</script>
<template><div>{{ msg }}</div></template>"#;
    let d = gen_tsc_declaration(sfc);
    assert!(
        !d.contains("defineComponent("),
        "options-API declaration must not wrap in defineComponent, got:\n{d}"
    );
    assert!(
        !d.contains("import { defineComponent }"),
        "options-API declaration must not value-import defineComponent, got:\n{d}"
    );
    assert!(
        d.contains("declare const TestComp") && d.contains("export default TestComp"),
        "options-API declaration still declares + default-exports the component, got:\n{d}"
    );
}

#[test]
fn declaration_mode_options_api_projects_the_full_props_and_emits_surface() {
    // An Options-API `defineComponent({ props, emits })` (no `<script setup>`)
    // must project its FULL public surface into the declaration — NOT the empty
    // `DefineComponent<{}, {}, any>` stub. The runtime-object props/emits are
    // extracted through the same prop/emit normalization the `<script setup>`
    // macros use and rendered declaration-SAFELY (no runtime `defineComponent`
    // call, no `__comp`, no `typeof __comp`).
    let sfc = r#"<script>
import { defineComponent } from 'vue'
export default defineComponent({
  name: 'Foo',
  props: { msg: String },
  emits: ['change'],
})
</script>
<template><div>{{ msg }}</div></template>"#;
    let d = gen_tsc_declaration(sfc);

    // POSITIVE: the prop `msg` and the emit `change` reach the instance surface.
    assert!(
        d.contains("new(") && d.contains("$props"),
        "options-API declaration carries the `new(...)` instance surface with $props, got:\n{d}"
    );
    assert!(
        d.contains("msg"),
        "options-API prop `msg` survives into the declaration surface, got:\n{d}"
    );
    assert!(
        d.contains("\"change\""),
        "options-API emit `change` survives into the declaration emit surface, got:\n{d}"
    );

    // NEGATIVE (discriminating): the empty stub's bare `DefineComponent<{}, {},
    // any>` must NOT be what a props-bearing component renders.
    assert!(
        !d.contains("DefineComponent<{}, {}, any>"),
        "a props/emits-bearing options-API component must NOT collapse to the empty \
         DefineComponent<{{}}, {{}}, any> stub, got:\n{d}"
    );

    // NEGATIVE: still fully declaration-legal — no runtime value code.
    assert!(
        !d.contains("defineComponent("),
        "options-API full-surface declaration must not call defineComponent, got:\n{d}"
    );
    assert!(
        !d.contains("const __comp") && !d.contains("typeof __comp"),
        "options-API full-surface declaration must not create/reference a runtime __comp, got:\n{d}"
    );
    assert!(
        d.contains("declare const TestComp") && d.contains("export default TestComp"),
        "options-API full-surface declaration declares + default-exports the component, got:\n{d}"
    );
}

#[test]
fn declaration_mode_options_api_scriptless_stays_empty_surface() {
    // CONTROL: a genuinely-empty surface (no `<script setup>`, no props/emits)
    // must still render the minimal empty stub — the full-surface projection must
    // not fabricate members where none exist.
    let sfc = r#"<script>
export default { name: 'Bare' }
</script>
<template><div>hi</div></template>"#;
    let d = gen_tsc_declaration(sfc);

    assert!(
        !d.contains("defineComponent("),
        "scriptless options-API declaration must not call defineComponent, got:\n{d}"
    );
    assert!(
        d.contains("declare const TestComp") && d.contains("export default TestComp"),
        "scriptless options-API declaration still declares + default-exports, got:\n{d}"
    );
    // No props were declared, so no prop name should be invented into the surface.
    assert!(
        !d.contains("msg"),
        "scriptless options-API declaration must not invent props, got:\n{d}"
    );
}

#[test]
fn declaration_mode_options_api_preserves_imported_type_only_prop_import() {
    // P2 #5: an Options-API prop typed via an IMPORTED type (`type: Object as
    // PropType<Foo>` where `Foo` is `import type`-ed) must keep `Foo`'s import in
    // the emitted declaration — otherwise the `.d.ts` references `Foo` with no
    // import (an incomplete / illegal declaration). Same class as the setup path's
    // type-import threading; the Options-API path must reuse the SAME machinery
    // (`collect_type_imports` + the `TypeUsageTracker` finalize), not pass empty
    // type-import context.
    //
    // RED-before: `generate_options_api_declaration` passed an EMPTY `type_imports`
    // / `parsed_items`, so the tracker had nothing to emit — `Foo` was referenced
    // in the surface with no `import type { Foo }` line.
    let sfc = r#"<script>
import { defineComponent } from 'vue'
import type { PropType } from 'vue'
import type { Foo } from './types'
export default defineComponent({
  name: 'Foo',
  props: { item: { type: Object as PropType<Foo>, required: true } },
})
</script>
<template><div>{{ item }}</div></template>"#;
    let d = gen_tsc_declaration(sfc);

    // POSITIVE: the prop surface references the imported type `Foo`...
    assert!(
        d.contains("Foo"),
        "options-API prop type references the imported type `Foo`, got:\n{d}"
    );
    // ...and the declaration brings `Foo` into scope via a type-only import.
    assert!(
        d.contains("import type { Foo } from './types'"),
        "options-API declaration emits the type-only import that resolves `Foo`, got:\n{d}"
    );

    // NEGATIVE (discriminating): `Foo` must NOT be referenced WITHOUT its import —
    // the declaration is not declaration-legal if `Foo` is undefined. (Asserted by
    // the positive import check above; this is the explicit no-orphan-type guard.)
    let references_foo_type = d.contains("PropType<Foo>") || d.contains(": Foo");
    let has_foo_import = d.contains("import type { Foo } from './types'");
    assert!(
        !references_foo_type || has_foo_import,
        "options-API declaration must not reference `Foo` without importing it, got:\n{d}"
    );

    // Still fully declaration-legal: no runtime value code, no PropType cast left
    // in a value position.
    assert!(
        !d.contains("defineComponent(") && !d.contains("const __comp"),
        "imported-type options-API declaration stays declaration-legal, got:\n{d}"
    );
}

#[test]
fn declaration_mode_options_api_promotes_value_import_used_in_prop_type() {
    // P2 #5 (value-import promotion): a VALUE import (`import { Foo }`, no `type`
    // modifier) used in an Options-API prop's TYPE position (`PropType<Foo>`) must
    // be PROMOTED to a declaration-legal `import type { Foo }` — exactly as the
    // setup path does. The declaration carries no runtime body, so a bare value
    // import of a type-only symbol risks an unused-value-import.
    let sfc = r#"<script>
import { defineComponent } from 'vue'
import { PropType } from 'vue'
import { Foo } from './types'
export default defineComponent({
  props: { item: { type: Object as PropType<Foo>, required: true } },
})
</script>
<template><div>{{ item }}</div></template>"#;
    let d = gen_tsc_declaration(sfc);

    // POSITIVE: the value import used in a type position is promoted to `import
    // type`.
    assert!(
        d.contains("import type { Foo } from './types'"),
        "options-API declaration promotes the type-position value import to `import type`, got:\n{d}"
    );
    // NEGATIVE: it must NOT emit the bare value import form.
    assert!(
        !d.contains("import { Foo } from './types'"),
        "options-API declaration must NOT emit the bare value import form, got:\n{d}"
    );
}

#[test]
fn declaration_mode_options_api_omits_unused_imported_type() {
    // CONTROL (discriminating): an imported type that is NOT used in any prop/emit
    // type position must stay ABSENT — proving the Options-API import emission is
    // usage-driven (the `TypeUsageTracker` marks only referenced types), not "emit
    // every import in the script".
    let sfc = r#"<script>
import { defineComponent } from 'vue'
import type { PropType } from 'vue'
import type { Unused } from './unused'
import type { Foo } from './types'
export default defineComponent({
  props: { item: { type: Object as PropType<Foo>, required: true } },
})
</script>
<template><div>{{ item }}</div></template>"#;
    let d = gen_tsc_declaration(sfc);

    // The used type is imported...
    assert!(
        d.contains("import type { Foo } from './types'"),
        "the referenced imported type is emitted, got:\n{d}"
    );
    // ...the unused one is not.
    assert!(
        !d.contains("Unused") && !d.contains("from './unused'"),
        "an unused imported type must NOT appear in the options-API declaration, got:\n{d}"
    );
}

#[test]
fn declaration_mode_promotes_value_import_used_in_type_position() {
    // A VALUE import (`import { Props }`, no `type` modifier) used in a TYPE
    // position (`defineProps<Props>()`) must reach the declaration's imports —
    // otherwise `Props` is undefined in the rendered props surface. The Public
    // path gets `Props` from the emitted setup body; the declaration path omits
    // the body, so it must bring `Props` into scope itself, as a declaration-
    // legal `import type` (a bare value import of a type-only symbol risks an
    // unused-value-import in a declaration).
    let sfc = r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(
        sfc,
        &[props_root_fixture_at(0, "Props", &[], &["Props"], &[])],
    );

    // POSITIVE: `Props` is brought into scope via a declaration-legal type
    // import.
    assert!(
        d.contains("import type { Props } from './types'"),
        "declaration promotes the type-position value import to `import type`, got:\n{d}"
    );
    // NEGATIVE: it must NOT emit the bare value import (`import { Props }`
    // without `type`) — that is not declaration-clean.
    assert!(
        !d.contains("import { Props } from './types'"),
        "declaration must NOT emit the bare value import form, got:\n{d}"
    );
    // The props surface references `Props`.
    assert!(
        d.contains("Props"),
        "the props surface references Props, got:\n{d}"
    );
}

#[test]
fn declaration_mode_omits_genuinely_value_only_import() {
    // CONTROL: a value import used ONLY as a runtime value (never in a type
    // position) must stay ABSENT from the declaration output — the declaration
    // carries no runtime code, so a runtime-only import has nothing to bind.
    // This proves the promotion above is type-position-driven, not "emit every
    // import".
    let sfc = r#"<script setup lang="ts">
import { runtimeHelper } from './helper'
import { Props } from './types'
runtimeHelper()
defineProps<Props>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(
        sfc,
        &[props_root_fixture_at(0, "Props", &[], &["Props"], &[])],
    );

    // The type-position import is promoted...
    assert!(
        d.contains("import type { Props } from './types'"),
        "the type-position import is promoted, got:\n{d}"
    );
    // ...but the runtime-only helper import is absent (not used in any type
    // position).
    assert!(
        !d.contains("runtimeHelper"),
        "a runtime-only import must NOT appear in the declaration, got:\n{d}"
    );
    assert!(
        !d.contains("from './helper'"),
        "the runtime-only import's module must NOT appear, got:\n{d}"
    );
}

#[test]
fn declaration_mode_expose_never_references_omitted_setup_binding() {
    // The Public path emits the `<script setup>` body so `typeof internal`
    // resolves against the runtime `const internal`. The declaration path OMITS
    // the setup body, so `typeof internal` would be an UNBOUND value reference —
    // an erroring declaration. The declaration expose surface must therefore be
    // declaration-safe: it must NOT emit `typeof <setup-binding>`.
    let sfc = r#"<script setup lang="ts">
const internal = 1
defineExpose({ internal })
</script>
<template><div>hi</div></template>"#;

    let decl = gen_tsc_declaration(sfc);
    let public = generate_tsc_output_with_options(
        sfc,
        "TestComp",
        &TscGenOptions {
            mode: TscMode::Public,
            ..Default::default()
        },
        MacroTscInput::NotRequired,
        &FallthroughPropsProjection::none(),
    )
    .expect("fixture has no typed codegen macro")
    .code;

    // (a) NEGATIVE: the declaration must NOT reference the omitted setup binding
    // via `typeof internal` (the binding is not in scope in the declaration).
    assert!(
        !decl.contains("typeof internal"),
        "declaration must NOT emit `typeof <setup-binding>` (unbound), got:\n{decl}"
    );

    // (b) POSITIVE: `internal` still surfaces in the expose tail with a legal
    // (declaration-resolvable) type — the member is not silently dropped.
    assert!(
        decl.contains("internal"),
        "declaration still surfaces the exposed `internal` member, got:\n{decl}"
    );

    // (c) DISCRIMINATING control: the Public mode DOES emit the runtime
    // `typeof internal` form (proving the negative above is non-vacuous and the
    // two modes genuinely differ on the expose tail).
    assert!(
        public.contains("typeof internal"),
        "Public mode emits the runtime `typeof internal` form (control), got:\n{public}"
    );
}

#[test]
fn declaration_mode_expose_complex_entries_are_declaration_safe() {
    // A more complex expose surface — an exposed ref and an exposed function —
    // exercises the whole class: NO expose entry may emit a value-position
    // reference (`typeof <ident>`) that only the omitted setup body defines.
    let sfc = r#"<script setup lang="ts">
import { ref } from 'vue'
const counter = ref(0)
function reset() { counter.value = 0 }
defineExpose({ counter, reset })
</script>
<template><div>hi</div></template>"#;

    let decl = gen_tsc_declaration(sfc);

    // NEGATIVE: neither exposed binding may be referenced via `typeof <ident>`
    // (both are omitted-setup-body bindings).
    assert!(
        !decl.contains("typeof counter"),
        "declaration must NOT emit `typeof counter`, got:\n{decl}"
    );
    assert!(
        !decl.contains("typeof reset"),
        "declaration must NOT emit `typeof reset`, got:\n{decl}"
    );
    // NEGATIVE: no setup-body runtime survives.
    assert!(
        !decl.contains("counter.value = 0"),
        "declaration must NOT emit the setup function body, got:\n{decl}"
    );
    // POSITIVE: both members still surface in the declaration's expose tail.
    assert!(
        decl.contains("counter") && decl.contains("reset"),
        "declaration surfaces both exposed members, got:\n{decl}"
    );
}

#[test]
fn declaration_mode_expose_typed_function_recovers_authored_param_and_return_types() {
    // A `function` declaration authored WITH explicit TypeScript parameter and
    // return type annotations must publish those REAL authored types in the
    // declaration-only expose tail, not the untyped `(step: any) => any` shape.
    let sfc = r#"<script setup lang="ts">
function bump(step: number): boolean {
  return step > 0
}
defineExpose({ bump })
</script>
<template><div>hi</div></template>"#;

    let decl = gen_tsc_declaration(sfc);

    assert!(
        decl.contains("bump: (step: number) => boolean"),
        "declaration must recover the authored param/return types, got:\n{decl}"
    );
    // NEGATIVE: the untyped shape-only fallback must not appear for a typed
    // declaration — that would be silently discarding the authored types.
    assert!(
        !decl.contains("bump: (step: any) => any") && !decl.contains("bump: (step?: any) => any"),
        "declaration must NOT fall back to the untyped any-shape for a typed function, got:\n{decl}"
    );
}

#[test]
fn declaration_mode_expose_untyped_function_still_falls_back_to_any_shape() {
    // Regression control: an UNTYPED function declaration has no authored
    // types to recover, so it must keep rendering the existing any-shaped
    // callable fallback unchanged.
    let sfc = r#"<script setup lang="ts">
function bump(step) {
  return step
}
defineExpose({ bump })
</script>
<template><div>hi</div></template>"#;

    let decl = gen_tsc_declaration(sfc);

    assert!(
        decl.contains("bump: (step: any) => any"),
        "untyped function must still render the any-shaped fallback, got:\n{decl}"
    );
}

#[test]
fn declaration_mode_expose_typed_call_initialized_const_recovers_authored_type() {
    // A call-initialized `const` (`ref(0)`) that ALSO carries an explicit
    // authored type annotation (`: Ref<number>`) must publish that REAL
    // authored type in the declaration-only expose tail, not `unknown`.
    let sfc = r#"<script setup lang="ts">
import { ref, Ref } from 'vue'
const counter: Ref<number> = ref(0)
defineExpose({ counter })
</script>
<template><div>hi</div></template>"#;

    let decl = gen_tsc_declaration(sfc);

    assert!(
        decl.contains("counter: Ref<number>"),
        "declaration must recover the authored variable type, got:\n{decl}"
    );
    // NEGATIVE: the untyped fallback must not appear when a real authored
    // type is available.
    assert!(
        !decl.contains("counter: unknown"),
        "declaration must NOT fall back to unknown when the const has an authored type, got:\n{decl}"
    );
}

#[test]
fn declaration_mode_expose_untyped_call_initialized_const_falls_back_to_unknown_without_a_bundle() {
    // Regression control, NARROWED scope: with NO session backing this call
    // (`MacroTscInput::NotRequired` — no TypeInfo producer ever ran, so
    // there is no resolved-type row to consume at all), the untyped
    // call-initialized const has nothing to recover from and must keep
    // rendering `unknown` unchanged. This is DISTINCT from "resolution was
    // attempted and genuinely missed" — see
    // `declaration_mode_expose_runtime_object_unavailable_member_falls_back_to_syntax_derived_type`
    // below for that case, and the `_resolves_authoritative_*` tests for the
    // upgrade path this control does NOT exercise.
    let sfc = r#"<script setup lang="ts">
import { ref } from 'vue'
const counter = ref(0)
defineExpose({ counter })
</script>
<template><div>hi</div></template>"#;

    let decl = gen_tsc_declaration(sfc);

    assert!(
        decl.contains("counter: unknown"),
        "untyped call-initialized const must still fall back to unknown with no bundle, got:\n{decl}"
    );
}

// ---------------------------------------------------------------------------
// Runtime-object `defineExpose` now resolves per-member REAL types through
// the shared TypeInfo `TypeOf` dispatch (`MacroTscProjection::Expose`) when
// an authoritative bundle backs the call, instead of only ever recovering an
// authored-syntax shape. These tests drive `apply_tsc_bundle`'s consumption
// of that DTO row directly via a hand-built bundle — the session-side
// resolution that PRODUCES such a row (real `TypeOf` queries against a live
// `VerterHost`) is proven separately in
// `verter_session::typeinfo::typeinfo_tests::vue_macro_codegen`.
// ---------------------------------------------------------------------------

fn expose_bundle_resolved(name: &str, resolved_text: &str) -> MacroTscBundle {
    expose_bundle_with_scope(
        name,
        TscExposeMemberType::Resolved(TscSpliceText::new(resolved_text)),
        TscScopeRequirements::default(),
    )
}

fn expose_bundle_with_scope(
    name: &str,
    member_type: TscExposeMemberType,
    scope: TscScopeRequirements,
) -> MacroTscBundle {
    MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Expose(TscExposeProjection {
                members: vec![TscExposeMemberRow {
                    name: name.to_owned(),
                    member_type,
                    anchor: MacroAnchor::Authored {
                        macro_index: 0,
                        member_ordinal: AuthoredMemberOrdinal::new(0),
                    },
                }],
                scope,
            })),
        }],
    }
}

#[test]
fn declaration_mode_expose_runtime_object_const_resolves_authoritative_type() {
    // The ruling's canonical example: `const count = ref(0)` (untyped —
    // nothing authored to recover) now renders the REAL resolved type when
    // an authoritative bundle backs the call, not `unknown`.
    let sfc = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
defineExpose({ count })
</script>
<template><div>hi</div></template>"#;
    let bundle = expose_bundle_resolved("count", "Ref<number>");

    let decl = generate_with_bundle(sfc, TscMode::Declaration, &bundle)
        .expect("authoritative expose bundle must apply")
        .code;

    assert!(
        decl.contains("count: Ref<number>"),
        "declaration must splice the authoritative resolved type, got:\n{decl}"
    );
    // NEGATIVE: the old untyped fallback must not survive once a real
    // resolved type is authoritative.
    assert!(
        !decl.contains("count: unknown"),
        "declaration must NOT fall back to unknown once resolution succeeded, got:\n{decl}"
    );
}

#[test]
fn declaration_mode_expose_runtime_object_computed_resolves_authoritative_type() {
    let sfc = r#"<script setup lang="ts">
import { ref, computed } from 'vue'
const count = ref(0)
const doubled = computed(() => count.value * 2)
defineExpose({ doubled })
</script>
<template><div>hi</div></template>"#;
    let bundle = expose_bundle_resolved("doubled", "ComputedRef<number>");

    let decl = generate_with_bundle(sfc, TscMode::Declaration, &bundle)
        .expect("authoritative expose bundle must apply")
        .code;

    assert!(
        decl.contains("doubled: ComputedRef<number>"),
        "declaration must splice the authoritative computed type, got:\n{decl}"
    );
    assert!(
        !decl.contains("doubled: unknown"),
        "declaration must NOT fall back to unknown once resolution succeeded, got:\n{decl}"
    );
}

#[test]
fn declaration_mode_expose_runtime_object_function_resolves_authoritative_signature() {
    // An UNANNOTATED function's syntax-only recovery falls back to the
    // any-shaped callable (`(step: any) => any`). With an authoritative
    // resolved signature it renders the REAL inferred parameter and return
    // types instead.
    let sfc = r#"<script setup lang="ts">
function bump(step) {
  return step + 1
}
defineExpose({ bump })
</script>
<template><div>hi</div></template>"#;
    let bundle = expose_bundle_resolved("bump", "(step: number) => number");

    let decl = generate_with_bundle(sfc, TscMode::Declaration, &bundle)
        .expect("authoritative expose bundle must apply")
        .code;

    assert!(
        decl.contains("bump: (step: number) => number"),
        "declaration must splice the authoritative resolved signature, got:\n{decl}"
    );
    // NEGATIVE: the syntax-only any-shaped fallback must not survive once a
    // real resolved signature is authoritative.
    assert!(
        !decl.contains("bump: (step: any) => any") && !decl.contains("bump: (step?: any) => any"),
        "declaration must NOT fall back to the untyped any-shape once resolution succeeded, got:\n{decl}"
    );
}

#[test]
fn declaration_mode_expose_runtime_object_unavailable_member_falls_back_to_syntax_derived_type() {
    // A per-member typed `Unavailable` degradation (resolution genuinely
    // missed) must NOT masquerade as a fabricated success — the compiler
    // falls back to its own honest syntax-derived answer, exactly the
    // behavior it had before this DTO row existed.
    let sfc = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
defineExpose({ count })
</script>
<template><div>hi</div></template>"#;
    let bundle = expose_bundle_with_scope(
        "count",
        TscExposeMemberType::Unavailable(TscDeclarationFailureReason::Unresolved(
            UnresolvedReason::MissingDeclaration,
        )),
        TscScopeRequirements::default(),
    );

    let decl = generate_with_bundle(sfc, TscMode::Declaration, &bundle)
        .expect("authoritative expose bundle must apply")
        .code;

    assert!(
        decl.contains("count: unknown"),
        "an Unavailable member must fall back to the honest syntax-derived type (unknown here), got:\n{decl}"
    );
}

#[test]
fn declaration_mode_expose_partial_outcome_with_wrong_macro_index_is_rejected() {
    // A degraded (`Partial`) outcome that coincidentally shares this call's
    // `syntax_index` but carries a DIFFERENT call's `macro_index` is not a
    // legitimate "no answer for this call" — it is identity-corrupt bundle
    // data. Before round 5, the degraded-outcome arm (`_ => continue`) never
    // checked `entry.macro_index` at all, so this fell through to the
    // syntax-derived fallback and was silently accepted as if the DTO had
    // genuinely produced no answer. It must hard-error instead, exactly like
    // `apply_tsc_bundle`'s own pre-outcome-match identity check.
    let sfc = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
defineExpose({ count })
</script>
<template><div>hi</div></template>"#;
    let bundle = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            // Differs from the single call's real macro_index (0) — this
            // row belongs to a DIFFERENT call, not this one.
            macro_index: 1,
            outcome: MacroTscOutcome::Partial(MacroFailure::new(
                MacroPartialReason::IncompleteTraversal,
                Some("partial detail".to_owned()),
            )),
        }],
    };

    let error = generate_with_bundle(sfc, TscMode::Declaration, &bundle).expect_err(
        "a Partial outcome whose macro_index mismatches this call's own must hard-error, not silently fall through to the syntax-derived fallback",
    );

    assert_eq!(
        error,
        super::script::TscGenerationError::MacroIdentityMismatch {
            subject: macro_subject(0),
        }
    );
}

#[test]
fn declaration_mode_expose_runtime_object_retains_referenced_local_type_declaration() {
    // The resolved type text can reference a LOCAL declaration (`Box`) — the
    // scope requirements the DTO row carries must retain it so the spliced
    // declaration output stays valid, exactly like the Props/Emits/Model
    // scope-threading this producer already does.
    let sfc = r#"<script setup lang="ts">
interface Box { value: number }
function makeBox(v: number): Box {
  return { value: v }
}
const count = makeBox(0)
defineExpose({ count })
</script>
<template><div>hi</div></template>"#;
    let bundle = expose_bundle_with_scope(
        "count",
        TscExposeMemberType::Resolved(TscSpliceText::new("Box")),
        TscScopeRequirements {
            owner_value_dependencies: Vec::new(),
            retained_bindings: Vec::new(),
            dependency_declarations: vec![dependency("Box", Vec::new())],
        },
    );

    let decl = generate_with_bundle(sfc, TscMode::Declaration, &bundle)
        .expect("authoritative expose bundle must apply")
        .code;

    assert!(
        decl.contains("count: Box"),
        "declaration must splice the authoritative resolved type, got:\n{decl}"
    );
    assert!(
        decl.contains("interface Box"),
        "declaration must retain the referenced local declaration `Box`, got:\n{decl}"
    );
}

#[test]
fn declaration_mode_expose_runtime_object_without_bundle_still_compiles() {
    // Backward compatibility: a caller that asserts `NotRequired` for a file
    // whose ONLY macro is a runtime-object `defineExpose` (no type-based
    // codegen macro at all) must keep compiling exactly as it did before
    // this producer existed — never a new `MissingAuthoritativeSemantics`
    // demand.
    let sfc = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
defineExpose({ count })
</script>
<template><div>hi</div></template>"#;

    let decl = gen_tsc_declaration(sfc);

    assert!(
        decl.contains("count: unknown"),
        "with no bundle at all, the syntax-derived fallback applies unchanged, got:\n{decl}"
    );
}

fn expose_bundle_two_authored_rows(first: (&str, &str), second: (&str, &str)) -> MacroTscBundle {
    MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Expose(TscExposeProjection {
                members: vec![
                    TscExposeMemberRow {
                        name: first.0.to_owned(),
                        member_type: TscExposeMemberType::Resolved(TscSpliceText::new(first.1)),
                        anchor: MacroAnchor::Authored {
                            macro_index: 0,
                            member_ordinal: AuthoredMemberOrdinal::new(0),
                        },
                    },
                    TscExposeMemberRow {
                        name: second.0.to_owned(),
                        member_type: TscExposeMemberType::Resolved(TscSpliceText::new(second.1)),
                        anchor: MacroAnchor::Authored {
                            macro_index: 0,
                            member_ordinal: AuthoredMemberOrdinal::new(1),
                        },
                    },
                ],
                scope: TscScopeRequirements::default(),
            })),
        }],
    }
}

#[test]
fn declaration_mode_expose_duplicate_authored_names_apply_by_position_not_name() {
    // Two members share the authored name `dup` at distinct source
    // positions (`dup: a` then `dup: b`). Each DTO row carries its own
    // `member_ordinal`. Consuming by name collapses both rows onto the
    // FIRST entry sharing that name; consuming by the row's own ordinal
    // must apply each row to its OWN entry instead.
    let sfc = r#"<script setup lang="ts">
const a = 1
const b = "two"
defineExpose({ dup: a, dup: b })
</script>
<template><div>hi</div></template>"#;
    let bundle = expose_bundle_two_authored_rows(("dup", "FirstType"), ("dup", "SecondType"));

    let decl = generate_with_bundle(sfc, TscMode::Declaration, &bundle)
        .expect("authoritative expose bundle must apply")
        .code;

    assert!(
        decl.contains("dup: FirstType") && decl.contains("dup: SecondType"),
        "each duplicate-named member must apply its OWN row by position, got:\n{decl}"
    );
}

#[test]
fn declaration_mode_expose_malformed_anchor_row_is_rejected_not_applied_by_name() {
    // A row anchored to an out-of-bounds authored position claims a
    // `Complete` outcome whose own row identity does not cohere with the
    // syntax it describes — that is corruption in an authoritative answer,
    // not a degraded one. It must hard-error rather than silently falling
    // back to matching by name (or being dropped and letting a corrupt
    // `Complete` DTO pass as valid).
    let sfc = r#"<script setup lang="ts">
function bump(step) {
  return step + 1
}
defineExpose({ bump })
</script>
<template><div>hi</div></template>"#;
    let bundle = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Expose(TscExposeProjection {
                members: vec![TscExposeMemberRow {
                    name: "bump".to_owned(),
                    member_type: TscExposeMemberType::Resolved(TscSpliceText::new(
                        "(step: number) => number",
                    )),
                    anchor: MacroAnchor::Authored {
                        macro_index: 0,
                        member_ordinal: AuthoredMemberOrdinal::new(7),
                    },
                }],
                scope: TscScopeRequirements::default(),
            })),
        }],
    };

    let error = generate_with_bundle(sfc, TscMode::Declaration, &bundle).expect_err(
        "an out-of-bounds authored ordinal must hard-error, not silently apply by name or drop",
    );

    assert_eq!(
        error,
        super::script::TscGenerationError::InvalidAuthoredMemberOrdinal {
            subject: macro_subject(0),
            member_ordinal: 7,
        }
    );
}

#[test]
fn declaration_mode_expose_unavailable_member_with_wrong_macro_index_is_rejected() {
    // A `Complete` outcome whose ONLY member is `Unavailable` still claims to
    // be this call's own authoritative row. If that row's anchor names a
    // DIFFERENT `macro_index` than the call it was returned under, the row's
    // identity does not cohere with the syntax it claims to describe — the
    // same corruption `declaration_mode_expose_malformed_anchor_row_is_rejected_not_applied_by_name`
    // catches for a `Resolved` member. An all-`Unavailable` member set must
    // not let that check be skipped: identity is validated BEFORE branching
    // on whether the member resolved, so this must hard-error rather than
    // silently falling through as "nothing to splice".
    let sfc = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
defineExpose({ count })
</script>
<template><div>hi</div></template>"#;
    let bundle = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Expose(TscExposeProjection {
                members: vec![TscExposeMemberRow {
                    name: "count".to_owned(),
                    member_type: TscExposeMemberType::Unavailable(
                        TscDeclarationFailureReason::Unresolved(
                            UnresolvedReason::MissingDeclaration,
                        ),
                    ),
                    anchor: MacroAnchor::Authored {
                        // Wrong macro_index: the call this entry/correlation
                        // belongs to is macro_index 0, not 1.
                        macro_index: 1,
                        member_ordinal: AuthoredMemberOrdinal::new(0),
                    },
                }],
                scope: TscScopeRequirements::default(),
            })),
        }],
    };

    let error = generate_with_bundle(sfc, TscMode::Declaration, &bundle).expect_err(
        "an Unavailable member with a mismatched macro_index must hard-error, not be silently skipped",
    );

    assert_eq!(
        error,
        super::script::TscGenerationError::InvalidMacroAnchor {
            subject: macro_subject(0),
        }
    );
}

#[test]
fn declaration_mode_expose_anchor_matches_correlation_but_not_entry_is_rejected() {
    // `entry.macro_index` (7) mismatches `correlation.macro_index` (0, the
    // single call's real macro_index) even though the MEMBER row's own
    // anchor happens to match correlation (0). Since round 5, this is caught
    // by the per-call entry-identity check that now runs immediately after
    // resolving the unique bundle entry — BEFORE the outcome is even
    // matched, let alone the member loop reached — so it hard-errors as
    // `MacroIdentityMismatch`, mirroring `apply_tsc_bundle`'s own
    // pre-outcome-match identity check. (Previously, with no such earlier
    // gate, this input reached the member loop's own
    // `macro_index != entry.macro_index || macro_index != correlation.macro_index`
    // check and hard-errored there instead, as `InvalidMacroAnchor`.)
    let sfc = r#"<script setup lang="ts">
const count = 1
defineExpose({ count })
</script>
<template><div>hi</div></template>"#;
    let bundle = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            // Differs from the anchor's macro_index (0) below, independent
            // of correlation.macro_index (also 0).
            macro_index: 7,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Expose(TscExposeProjection {
                members: vec![TscExposeMemberRow {
                    name: "count".to_owned(),
                    member_type: TscExposeMemberType::Resolved(TscSpliceText::new("number")),
                    anchor: MacroAnchor::Authored {
                        macro_index: 0,
                        member_ordinal: AuthoredMemberOrdinal::new(0),
                    },
                }],
                scope: TscScopeRequirements::default(),
            })),
        }],
    };

    let error = generate_with_bundle(sfc, TscMode::Declaration, &bundle).expect_err(
        "a mismatch between entry.macro_index and correlation.macro_index must hard-error",
    );

    assert_eq!(
        error,
        super::script::TscGenerationError::MacroIdentityMismatch {
            subject: macro_subject(0),
        }
    );
}

#[test]
fn declaration_mode_expose_anchor_matches_entry_but_not_correlation_is_rejected() {
    // The member's own anchor matches `entry.macro_index` (5), but
    // `entry.macro_index` itself (5) mismatches `correlation.macro_index`
    // (0, the single call's real macro_index). Since round 5 this is caught
    // by the earlier per-call entry-identity check (same as the previous
    // test) before the outcome — and so the member loop — is ever reached:
    // `entry.macro_index != correlation.macro_index` alone is sufficient,
    // independent of what the member row's own anchor says.
    let sfc = r#"<script setup lang="ts">
const count = 1
defineExpose({ count })
</script>
<template><div>hi</div></template>"#;
    let bundle = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            // Matches the anchor's macro_index (5) below.
            macro_index: 5,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Expose(TscExposeProjection {
                members: vec![TscExposeMemberRow {
                    name: "count".to_owned(),
                    member_type: TscExposeMemberType::Resolved(TscSpliceText::new("number")),
                    anchor: MacroAnchor::Authored {
                        // Differs from correlation.macro_index (0, the
                        // single call's real macro_index).
                        macro_index: 5,
                        member_ordinal: AuthoredMemberOrdinal::new(0),
                    },
                }],
                scope: TscScopeRequirements::default(),
            })),
        }],
    };

    let error = generate_with_bundle(sfc, TscMode::Declaration, &bundle).expect_err(
        "a mismatch between entry.macro_index and correlation.macro_index must hard-error",
    );

    assert_eq!(
        error,
        super::script::TscGenerationError::MacroIdentityMismatch {
            subject: macro_subject(0),
        }
    );
}

#[test]
fn declaration_mode_expose_complete_wrong_projection_is_role_mismatch() {
    // A `Complete` outcome for this call's `syntax_index` that wraps the
    // WRONG projection variant (`Props` instead of `Expose`) is a real
    // complete answer of the wrong shape — corrupt bundle data, not "no
    // answer". It must hard-error the same way `apply_tsc_bundle`'s
    // semantic-slot loop hard-errors on a role mismatch for props/emits/
    // model, not fall through as if the outcome were merely absent.
    let sfc = r#"<script setup lang="ts">
const count = 1
defineExpose({ count })
</script>
<template><div>hi</div></template>"#;
    let bundle = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Props(TscPropsProjection {
                public: TscPublicPropsProjection::AuthoredArgument {
                    anchor: MacroAnchor::MacroArgument { macro_index: 0 },
                },
                testing_rows: Vec::new(),
                scope: TscScopeRequirements::default(),
            })),
        }],
    };

    let error = generate_with_bundle(sfc, TscMode::Declaration, &bundle).expect_err(
        "a Complete outcome wrapping the wrong projection variant must hard-error as a role mismatch",
    );

    assert_eq!(
        error,
        super::script::TscGenerationError::RoleMismatch {
            subject: macro_subject(0),
        }
    );
}

#[test]
fn declaration_mode_expose_duplicate_bundle_entry_for_one_call_is_rejected() {
    // Two bundle entries claim the SAME `syntax_index`. Picking the first
    // (as `.find()` used to) can silently hide a later corrupt/valid entry
    // behind an earlier one. This must hard-error exactly like
    // `apply_tsc_bundle`'s semantic-slot loop does for the equivalent shape.
    let sfc = r#"<script setup lang="ts">
const count = 1
defineExpose({ count })
</script>
<template><div>hi</div></template>"#;
    let one_row = |resolved: &str| {
        MacroTscOutcome::Complete(MacroTscProjection::Expose(TscExposeProjection {
            members: vec![TscExposeMemberRow {
                name: "count".to_owned(),
                member_type: TscExposeMemberType::Resolved(TscSpliceText::new(resolved)),
                anchor: MacroAnchor::Authored {
                    macro_index: 0,
                    member_ordinal: AuthoredMemberOrdinal::new(0),
                },
            }],
            scope: TscScopeRequirements::default(),
        }))
    };
    let bundle = MacroTscBundle {
        entries: vec![
            MacroTscEntry {
                syntax_index: 0,
                macro_index: 0,
                outcome: one_row("FirstType"),
            },
            MacroTscEntry {
                syntax_index: 0,
                macro_index: 0,
                outcome: one_row("SecondType"),
            },
        ],
    };

    let error = generate_with_bundle(sfc, TscMode::Declaration, &bundle).expect_err(
        "two bundle entries for one syntax_index must hard-error, not silently pick the first",
    );

    assert_eq!(
        error,
        super::script::TscGenerationError::DuplicateEntry {
            subject: macro_subject(0),
        }
    );
}

#[test]
fn declaration_mode_expose_complete_row_set_missing_an_ordinal_is_rejected() {
    // `defineExpose({ count, extra })` has TWO authored members, but the
    // `Complete` bundle row set supplies only ordinal 0. The DTO doc
    // contract promises "one row per runtime-object member" — a `Complete`
    // outcome that covers fewer than `member_count` ordinals made an
    // incomplete promise. Ordinal 1 must not silently keep its
    // syntax-derived fallback as if this call had no bundle row at all.
    let sfc = r#"<script setup lang="ts">
const count = 1
const extra = "two"
defineExpose({ count, extra })
</script>
<template><div>hi</div></template>"#;
    let bundle = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Expose(TscExposeProjection {
                members: vec![TscExposeMemberRow {
                    name: "count".to_owned(),
                    member_type: TscExposeMemberType::Resolved(TscSpliceText::new("number")),
                    anchor: MacroAnchor::Authored {
                        macro_index: 0,
                        member_ordinal: AuthoredMemberOrdinal::new(0),
                    },
                }],
                scope: TscScopeRequirements::default(),
            })),
        }],
    };

    let error = generate_with_bundle(sfc, TscMode::Declaration, &bundle)
        .expect_err("a Complete row set covering fewer than member_count ordinals must hard-error");

    assert_eq!(
        error,
        super::script::TscGenerationError::InvalidAuthoredMemberOrdinal {
            subject: macro_subject(0),
            member_ordinal: 1,
        }
    );
}

#[test]
fn declaration_mode_expose_complete_row_set_duplicate_ordinal_is_rejected() {
    // `defineExpose({ count, extra })` has two authored members, but the
    // `Complete` bundle row set carries TWO rows both anchored to ordinal 0
    // — the second would silently overwrite the first's splice, and ordinal
    // 1 is never covered either way. This must hard-error at the point the
    // second (now-identity-proven) row would apply.
    let sfc = r#"<script setup lang="ts">
const count = 1
const extra = "two"
defineExpose({ count, extra })
</script>
<template><div>hi</div></template>"#;
    let bundle = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Expose(TscExposeProjection {
                members: vec![
                    TscExposeMemberRow {
                        name: "count".to_owned(),
                        member_type: TscExposeMemberType::Resolved(TscSpliceText::new("number")),
                        anchor: MacroAnchor::Authored {
                            macro_index: 0,
                            member_ordinal: AuthoredMemberOrdinal::new(0),
                        },
                    },
                    TscExposeMemberRow {
                        name: "count".to_owned(),
                        member_type: TscExposeMemberType::Resolved(TscSpliceText::new("string")),
                        anchor: MacroAnchor::Authored {
                            macro_index: 0,
                            member_ordinal: AuthoredMemberOrdinal::new(0),
                        },
                    },
                ],
                scope: TscScopeRequirements::default(),
            })),
        }],
    };

    let error = generate_with_bundle(sfc, TscMode::Declaration, &bundle).expect_err(
        "two Complete rows anchored to the same ordinal must hard-error, not silently overwrite",
    );

    assert_eq!(
        error,
        super::script::TscGenerationError::InvalidAuthoredMemberOrdinal {
            subject: macro_subject(0),
            member_ordinal: 0,
        }
    );
}

#[test]
fn declaration_mode_expose_two_calls_apply_each_by_its_own_range() {
    // Two `defineExpose` calls in one SFC — nothing in the parser/compiler
    // path forbids it. Each call's `AuthoredMemberOrdinal`s restart at
    // zero, so the second call's row at ordinal 0 must resolve to the
    // SECOND call's first member, not the first call's.
    let sfc = r#"<script setup lang="ts">
const a = 1
const b = "two"
defineExpose({ a })
defineExpose({ b })
</script>
<template><div>hi</div></template>"#;
    let bundle = MacroTscBundle {
        entries: vec![
            MacroTscEntry {
                syntax_index: 0,
                macro_index: 0,
                outcome: MacroTscOutcome::Complete(MacroTscProjection::Expose(
                    TscExposeProjection {
                        members: vec![TscExposeMemberRow {
                            name: "a".to_owned(),
                            member_type: TscExposeMemberType::Resolved(TscSpliceText::new(
                                "number",
                            )),
                            anchor: MacroAnchor::Authored {
                                macro_index: 0,
                                member_ordinal: AuthoredMemberOrdinal::new(0),
                            },
                        }],
                        scope: TscScopeRequirements::default(),
                    },
                )),
            },
            MacroTscEntry {
                syntax_index: 1,
                macro_index: 1,
                outcome: MacroTscOutcome::Complete(MacroTscProjection::Expose(
                    TscExposeProjection {
                        members: vec![TscExposeMemberRow {
                            name: "b".to_owned(),
                            member_type: TscExposeMemberType::Resolved(TscSpliceText::new(
                                "string",
                            )),
                            anchor: MacroAnchor::Authored {
                                macro_index: 1,
                                member_ordinal: AuthoredMemberOrdinal::new(0),
                            },
                        }],
                        scope: TscScopeRequirements::default(),
                    },
                )),
            },
        ],
    };

    let decl = generate_with_bundle(sfc, TscMode::Declaration, &bundle)
        .expect("two runtime-object defineExpose calls must both apply their own bundle row")
        .code;

    assert!(
        decl.contains("a: number"),
        "the first call's `a` must resolve from the first call's own row, got:\n{decl}"
    );
    assert!(
        decl.contains("b: string"),
        "the second call's `b` must resolve from the second call's own row (ordinal 0 within its own range), not the first call's, got:\n{decl}"
    );
    assert!(
        !decl.contains("b: number") && !decl.contains("a: string"),
        "each call's row must never cross-apply to the other call's member, got:\n{decl}"
    );
}

#[test]
fn declaration_mode_carries_the_instance_component_surface() {
    // #5 contract: the declaration `C` is a usable component VALUE — it carries
    // the instance surface (the `new(...)` construct signature exposing
    // `$props`) and default-exports `C`, which is what a consumer importing the
    // component and using it as `<C/>` / `createApp(C)` needs. The non-
    // declaration-legal `__OmitNew<typeof __comp> &` prefix is intentionally
    // absent (it is value-bearing).
    let sfc = r#"<script setup lang="ts">
defineProps<{ msg: string; count?: number }>()
defineEmits<{ change: [value: string] }>()
</script>
<template><div>{{ msg }}</div></template>"#;
    let d = gen_tsc_declaration_with(
        sfc,
        &[
            props_fixture("", &[]),
            emits_fixture_at(
                1,
                &[authored_emit(
                    "change",
                    "...args: [value: string]",
                    "...args: [value: string]",
                    1,
                    0,
                )],
                &[],
                &[],
            ),
        ],
    );

    // The component value is declared and default-exported (so `import C from
    // "..."` and `const x: typeof C` bind).
    assert!(
        d.contains("declare const TestComp"),
        "declares the component value, got:\n{d}"
    );
    assert!(
        d.contains("export default TestComp"),
        "default-exports the component value, got:\n{d}"
    );
    // The instance surface is reachable through the construct signature carrying
    // `$props` (usable as a component) — the load-bearing instance contract.
    assert!(
        d.contains("new(") && d.contains("$props"),
        "carries the `new(...)` instance surface with $props, got:\n{d}"
    );
    assert!(
        d.contains("msg") && d.contains("count?"),
        "the public props surface (msg, count?) is present, got:\n{d}"
    );
    // NEGATIVE: the value-bearing `__OmitNew<typeof __comp>` prefix is NOT
    // declaration-legal and must be absent.
    assert!(
        !d.contains("__OmitNew") && !d.contains("typeof __comp"),
        "the value-bearing __OmitNew<typeof __comp> prefix is absent, got:\n{d}"
    );
}

#[test]
fn declaration_mode_differs_from_public_mode_on_runtime_tokens() {
    // DISCRIMINATING: the same SFC under `Public` DOES contain the runtime
    // tokens, proving the assertions above are not vacuously true.
    let sfc = r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#;
    let fixture = [props_fixture("", &[])];
    let public = gen_tsc_mode_with(sfc, TscMode::Public, &fixture).code;
    let decl = gen_tsc_declaration_with(sfc, &fixture);

    assert!(
        public.contains("const __comp = defineComponent"),
        "Public mode emits the runtime __comp (control), got:\n{public}"
    );
    assert!(
        !decl.contains("const __comp = defineComponent"),
        "Declaration mode drops the runtime __comp, got:\n{decl}"
    );
    assert_ne!(
        public, decl,
        "Declaration output must differ from Public output"
    );
}

#[test]
fn declaration_mode_promotes_namespace_value_import_used_in_type_position() {
    // A NAMESPACE value import (`import * as NS`) used in a type position
    // (`defineProps<NS.Props>()`) references `NS.Props`; the declaration omits
    // the setup body that brought `NS` into scope, so it must bring `NS` into
    // scope itself with the declaration-legal namespace type-only form
    // `import type * as NS from './ns'` — otherwise `NS` is undefined in the
    // rendered props surface. The bare named form (`import type { NS }`) is
    // malformed for a namespace import.
    let sfc = r#"<script setup lang="ts">
import * as NS from './ns'
defineProps<NS.Props>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(
        sfc,
        &[props_root_fixture_at(0, "NS.Props", &[], &["NS"], &[])],
    );

    // POSITIVE: `NS` resolves via the declaration-legal namespace type-only
    // import, so `NS.Props` is bound.
    assert!(
        d.contains("import type * as NS from './ns'"),
        "declaration promotes the type-position namespace import to `import type * as NS`, got:\n{d}"
    );
    // NEGATIVE: it must NOT mis-promote to the malformed bare named form.
    assert!(
        !d.contains("import type { NS }"),
        "a namespace import must NOT be mis-promoted to a bare named import type, got:\n{d}"
    );
    // The props surface references `NS`.
    assert!(
        d.contains("NS.Props") || d.contains("NS"),
        "the props surface references the namespace member NS.Props, got:\n{d}"
    );
}

#[test]
fn declaration_mode_promotes_default_value_import_used_in_type_position() {
    // A DEFAULT value import (`import Props from './types'`) used in a type
    // position (`defineProps<Props>()`) references `Props`; the declaration
    // omits the setup body, so it must bring `Props` into scope itself with the
    // declaration-legal default type-only form `import type Props from
    // './types'` — otherwise `Props` is undefined. The bare named form
    // (`import type { Props }`) is the WRONG shape for a default import.
    let sfc = r#"<script setup lang="ts">
import Props from './types'
defineProps<Props>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(
        sfc,
        &[props_root_fixture_at(0, "Props", &[], &["Props"], &[])],
    );

    // POSITIVE: `Props` resolves via the declaration-legal default type-only
    // import.
    assert!(
        d.contains("import type Props from './types'"),
        "declaration promotes the type-position default import to `import type Props`, got:\n{d}"
    );
    // NEGATIVE: it must NOT mis-promote a default import to the named form.
    assert!(
        !d.contains("import type { Props } from './types'"),
        "a default import must NOT be mis-promoted to a named import type, got:\n{d}"
    );
    // The props surface references `Props`.
    assert!(
        d.contains("Props"),
        "the props surface references Props, got:\n{d}"
    );
}

#[test]
fn declaration_mode_promotes_aliased_named_value_import_with_imported_name() {
    // An ALIASED named value import (`import { Props as P }`) used in a type
    // position (`defineProps<P>()`) references the LOCAL name `P`, but the
    // module exports `Props`, not `P`. The promotion must emit `import type {
    // Props as P }` (the imported name `Props` aliased to the local `P`) —
    // emitting `import type { P }` would not resolve, because `./types` has no
    // export named `P`.
    let sfc = r#"<script setup lang="ts">
import { Props as P } from './types'
defineProps<P>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(sfc, &[props_root_fixture_at(0, "P", &[], &["P"], &[])]);

    // POSITIVE: the imported name `Props` is preserved, aliased to the local
    // `P` — the only form that actually resolves.
    assert!(
        d.contains("import type { Props as P } from './types'"),
        "declaration emits the aliased form `import type {{ Props as P }}`, got:\n{d}"
    );
    // NEGATIVE: the wrong, unresolvable bare-local form must NOT be emitted.
    assert!(
        !d.contains("import type { P } from './types'"),
        "declaration must NOT emit the unresolvable bare-local form `import type {{ P }}`, got:\n{d}"
    );
    // The props surface references the LOCAL name `P`.
    assert!(
        d.contains("P"),
        "the props surface references the local name P, got:\n{d}"
    );
}

#[test]
fn declaration_mode_promotes_non_aliased_named_value_import_control() {
    // CONTROL for the aliased case: a NON-aliased named value import
    // (`import { Props }`, local == imported) must emit the plain
    // `import type { Props }` form — no spurious `Props as Props` self-alias.
    let sfc = r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(
        sfc,
        &[props_root_fixture_at(0, "Props", &[], &["Props"], &[])],
    );

    assert!(
        d.contains("import type { Props } from './types'"),
        "non-aliased import emits the plain `import type {{ Props }}` form, got:\n{d}"
    );
    // No degenerate self-alias.
    assert!(
        !d.contains("Props as Props"),
        "a non-aliased import must NOT emit a self-alias, got:\n{d}"
    );
}

#[test]
fn declaration_mode_type_only_aliased_import_preserves_imported_name() {
    // PRE-EXISTING type-only path audit (same class: reconstructed type-only
    // imports must preserve the imported name). An EXPLICIT type-only aliased
    // import (`import type { Props as P }`) used in a type position must
    // round-trip as `import type { Props as P }`, NOT the unresolvable bare
    // `import type { P }`. This guards the `type_import_stmts` reconstruction,
    // not the value-promotion path.
    let sfc = r#"<script setup lang="ts">
import type { Props as P } from './types'
defineProps<P>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(sfc, &[props_root_fixture_at(0, "P", &[], &["P"], &[])]);

    assert!(
        d.contains("import type { Props as P } from './types'"),
        "type-only aliased import round-trips with the imported name preserved, got:\n{d}"
    );
    assert!(
        !d.contains("import type { P } from './types'"),
        "the type-only path must NOT drop the imported name to the bare-local form, got:\n{d}"
    );
}

#[test]
fn declaration_mode_promotes_mixed_form_default_and_aliased_named_import() {
    // MIXED-FORM import: `import Default, { Named as N } from '...'` with BOTH
    // parts used in type positions. Each part must promote in its own correct
    // declaration-legal form: the default as `import type Default`, the aliased
    // named as `import type { Named as N }`.
    let sfc = r#"<script setup lang="ts">
import Default, { Named as N } from './mixed'
defineProps<Default & N>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(
        sfc,
        &[props_root_fixture_at(
            0,
            "Default & N",
            &[],
            &["Default", "N"],
            &[],
        )],
    );

    // The default part promotes to the default type-only form.
    assert!(
        d.contains("import type Default from './mixed'"),
        "the default part of a mixed import promotes to `import type Default`, got:\n{d}"
    );
    // The aliased named part preserves the imported name.
    assert!(
        d.contains("import type { Named as N } from './mixed'"),
        "the aliased named part preserves the imported name, got:\n{d}"
    );
    // NEGATIVE: no malformed combined/bare-local forms.
    assert!(
        !d.contains("import type { N } from './mixed'"),
        "the named part must NOT drop the imported name, got:\n{d}"
    );
}

#[test]
fn declaration_mode_promotes_string_literal_named_value_import_preserving_quotes() {
    // A STRING-LITERAL named value import (`import { "vue-props" as P }`) used in
    // a type position (`defineProps<P>()`). TypeScript allows an arbitrary-string
    // module export name in a named import; the local MUST be an `as <ident>`
    // alias (a bare `import { "vue-props" }` is a TS error). The promotion must
    // render the imported name as a QUOTED string literal — `import type {
    // "vue-props" as P }` — because `vue-props` is NOT a valid identifier. The
    // bare form `import type { vue-props as P }` is invalid syntax and would fail
    // to type-check.
    let sfc = r#"<script setup lang="ts">
import { "vue-props" as P } from './types'
defineProps<P>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(sfc, &[props_root_fixture_at(0, "P", &[], &["P"], &[])]);

    // POSITIVE: the string-literal imported name keeps its quotes, aliased to the
    // local `P` — the only declaration-legal form that resolves.
    assert!(
        d.contains(r#"import type { "vue-props" as P } from './types'"#),
        "declaration emits the quoted string-literal form `import type {{ \"vue-props\" as P }}`, got:\n{d}"
    );
    // NEGATIVE: the invalid bare-identifier form must NOT be emitted.
    assert!(
        !d.contains("import type { vue-props as P } from './types'"),
        "declaration must NOT emit the invalid bare `import type {{ vue-props as P }}`, got:\n{d}"
    );
    // The props surface references the LOCAL name `P`.
    assert!(
        d.contains("P"),
        "the props surface references the local name P, got:\n{d}"
    );
}

#[test]
fn declaration_mode_type_only_string_literal_aliased_import_preserves_quotes() {
    // PRE-EXISTING type-only path audit for the string-literal class: an EXPLICIT
    // type-only string-literal aliased import (`import type { "vue-props" as P }`)
    // used in a type position must round-trip with the quotes preserved, NOT the
    // invalid bare `import type { vue-props as P }`. Both paths render through the
    // same `render_stmt`, but this asserts the explicit type-only route.
    let sfc = r#"<script setup lang="ts">
import type { "vue-props" as P } from './types'
defineProps<P>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(sfc, &[props_root_fixture_at(0, "P", &[], &["P"], &[])]);

    assert!(
        d.contains(r#"import type { "vue-props" as P } from './types'"#),
        "type-only string-literal aliased import round-trips with quotes preserved, got:\n{d}"
    );
    assert!(
        !d.contains("import type { vue-props as P } from './types'"),
        "the type-only path must NOT drop the quotes to the invalid bare form, got:\n{d}"
    );
}

#[test]
fn declaration_mode_string_literal_export_name_with_newline_escapes_the_line_terminator() {
    // A STRING-LITERAL named value import whose export name CONTAINS a line
    // terminator — the SFC source uses the VALID `\n` escape inside the literal
    // (`import { "line\nprops" as P }`), which OXC cooks into a real LF in the
    // captured export name. Used in a type position (`defineProps<P>()`), it
    // promotes to a declaration-legal `import type`. The reconstruction re-wraps
    // the COOKED name (with a real LF) into a double-quoted TS string literal: a
    // raw newline inside `"…"` is itself invalid, so the line terminator MUST be
    // re-emitted as the two-character escape `\n`, keeping the whole `import
    // type` statement on a single physical line.
    //
    // `\n` in this raw string is the two characters backslash-n (a valid source
    // escape), NOT a Rust line break.
    let sfc = r#"<script setup lang="ts">
import { "line\nprops" as P } from './types'
defineProps<P>()
</script>
<template><div>hello</div></template>"#;
    let d = gen_tsc_declaration_with(sfc, &[props_root_fixture_at(0, "P", &[], &["P"], &[])]);

    // The reconstructed import re-escapes the cooked LF as the two-character
    // sequence backslash-n (`"line\nprops"`), NOT a raw newline.
    let escaped_import = "import type { \"line\\nprops\" as P } from './types'";
    assert!(
        d.contains(escaped_import),
        "declaration must escape the embedded line terminator as `\\n` in the \
         string-literal export name, got:\n{d}"
    );
    // NEGATIVE: the raw-newline form (a line break INSIDE the quotes, splitting
    // the `import type` statement across two physical lines) must NOT appear —
    // that is an invalid TS string literal.
    assert!(
        !d.contains("import type { \"line\nprops\" as P } from './types'"),
        "declaration must NOT emit a raw line terminator inside the string \
         literal (it would split the import across lines), got:\n{d}"
    );
    // The reconstructed `import type` statement is on a SINGLE physical line:
    // locate the emitted `import type { "line` prefix and assert no raw newline
    // precedes its `from './types'` clause.
    let stmt_start = d
        .find("import type { \"line")
        .expect("the reconstructed string-literal import statement is present");
    let after_start = &d[stmt_start..];
    let stmt_end = after_start
        .find("from './types'")
        .expect("the reconstructed import statement reaches its `from` clause");
    assert!(
        !after_start[..stmt_end].contains('\n'),
        "the reconstructed `import type` statement must stay on one physical \
         line (no raw newline between `import type` and `from`), got:\n{d}"
    );
    // The props surface references the LOCAL name `P`.
    assert!(
        d.contains("P"),
        "the props surface references the local name P, got:\n{d}"
    );
}

#[test]
fn quote_module_export_name_is_a_complete_ts_string_literal_encoder() {
    use super::script::quote_module_export_name;

    // A plain printable name (including the hyphen) is wrapped UNCHANGED — no
    // over-escaping of printable ASCII.
    assert_eq!(quote_module_export_name("vue-props"), "\"vue-props\"");
    assert_eq!(quote_module_export_name(""), "\"\"");

    // An embedded double quote is escaped.
    assert_eq!(quote_module_export_name("a\"b"), "\"a\\\"b\"");

    // An embedded backslash is escaped (and escaped FIRST, so it does not
    // double-process the escapes introduced for other characters).
    assert_eq!(quote_module_export_name("a\\b"), "\"a\\\\b\"");
    // `\` immediately followed by `"` stays two independent escapes, not a
    // collapsed `\"`.
    assert_eq!(quote_module_export_name("\\\""), "\"\\\\\\\"\"");

    // Line terminators illegal raw in a string literal become escapes.
    assert_eq!(quote_module_export_name("a\nb"), "\"a\\nb\"");
    assert_eq!(quote_module_export_name("a\rb"), "\"a\\rb\"");
    // U+2028 LINE SEPARATOR / U+2029 PARAGRAPH SEPARATOR are illegal raw in a TS
    // string literal and must be `\u`-escaped.
    assert_eq!(quote_module_export_name("a\u{2028}b"), "\"a\\u2028b\"");
    assert_eq!(quote_module_export_name("a\u{2029}b"), "\"a\\u2029b\"");

    // ASCII control characters: TAB has a short escape; a control char without a
    // short escape (e.g. U+0001) uses the zero-padded 4-hex `\uXXXX` fallback.
    assert_eq!(quote_module_export_name("a\tb"), "\"a\\tb\"");
    assert_eq!(quote_module_export_name("a\u{0001}b"), "\"a\\u0001b\"");

    // NEGATIVE: the encoder NEVER emits a raw control char or line terminator —
    // the output is always a single-line, valid double-quoted literal.
    for input in ["x\ny", "x\ry", "x\tz", "x\u{2028}y", "x\u{0000}y"] {
        let encoded = quote_module_export_name(input);
        assert!(
            encoded.starts_with('"') && encoded.ends_with('"'),
            "encoded value is a double-quoted literal, got: {encoded:?}"
        );
        let inner = &encoded[1..encoded.len() - 1];
        assert!(
            !inner.chars().any(|c| c == '\n'
                || c == '\r'
                || c == '\u{2028}'
                || c == '\u{2029}'
                || (c.is_control())),
            "the encoded literal must not contain a raw control char or line \
             terminator, got: {encoded:?}"
        );
    }
}

// ── Quoted / non-identifier prop keys in the public API carrier ──────────────
//
// A `defineProps` type literal may declare members whose names are NOT valid TS
// identifiers (`"onLate-signal"`, `"data-x"`, `"404"`). The synthesized public
// API carrier must re-quote those keys inside the SINGLE `new(props?: ...)`
// parameter object. Rendering them bare produces INVALID TypeScript
// (`{ onLate-signal?: ... }`), which providers error-recover into a corrupted
// constructor — a phantom second required parameter and an `any` instance —
// silently disabling consumer prop type-checking.

/// Split the parameter list of every `new(...)` construct signature in `code`
/// at paren/brace/bracket/angle depth zero and return the per-signature
/// top-level parameter counts.
///
/// `=>` is a single arrow token, NOT a type-argument close: the trailing `>` of
/// an arrow must not decrement depth, or a function-typed parameter (`(x) =>
/// void`) poisons the walk — depth would fall to zero early, the scan would
/// break at the parameter object's own `}`, and it would report only a prefix
/// (never seeing a second top-level parameter leaked AFTER a function-typed
/// prop). Type strings in this domain contain no comparison operators, so a `>`
/// immediately preceded by `=` is unambiguously an arrow.
fn construct_signature_param_counts(code: &str) -> Vec<usize> {
    let mut counts = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = code[search_from..].find("new(") {
        let open = search_from + rel + "new(".len() - 1;
        let bytes = code.as_bytes();
        let mut depth = 0i32;
        let mut params = 0usize;
        let mut saw_non_ws = false;
        let mut idx = open;
        while idx < bytes.len() {
            match bytes[idx] {
                b'(' | b'{' | b'[' | b'<' => depth += 1,
                // The `>` tail of `=>` is an arrow, not a depth close.
                b'>' if idx > 0 && bytes[idx - 1] == b'=' => saw_non_ws = true,
                b')' | b'}' | b']' | b'>' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                b',' if depth == 1 => params += 1,
                b if !b.is_ascii_whitespace() => saw_non_ws = true,
                _ => {}
            }
            idx += 1;
        }
        if saw_non_ws {
            params += 1;
        }
        counts.push(params);
        search_from = open + 1;
    }
    counts
}

#[test]
fn tsc_codegen_quoted_hyphenated_prop_key_stays_quoted_single_ctor_param() {
    // Mirrors packages/vue-vscode/e2e/fixtures/single-project/src/GlobalEmitComp.vue.
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineProps<{
  onPing?: (payload: { pingCode: string; pingCount: number }) => void;
  "onLate-signal"?: (sig: { sigName: string; sigLevel: number }) => void;
}>()
</script><template><div>global emit comp</div></template>"#,
        &[props_fixture(
            "",
            &[
                authored_prop(
                    "onPing",
                    true,
                    "(payload: { pingCode: string; pingCount: number }) => void",
                    0,
                    0,
                ),
                authored_prop(
                    "onLate-signal",
                    true,
                    "(sig: { sigName: string; sigLevel: number }) => void",
                    0,
                    1,
                ),
            ],
        )],
    );

    // The quoted member name survives INSIDE the props object literal.
    assert!(
        r.contains("\"onLate-signal\"?:"),
        "the quoted hyphenated prop key must be preserved (quoted) in the \
         synthesized props object: got {}",
        r
    );
    // NEGATIVE: the bare (unquoted) member never appears — it is invalid TS and
    // is exactly what corrupts provider parsing.
    assert!(
        !r.contains(" onLate-signal?:") && !r.contains(";onLate-signal?:"),
        "the hyphenated key must never render unquoted (invalid TS member): got {}",
        r
    );
    // STRUCTURE: every construct signature has exactly ONE top-level parameter
    // (`props?`) — the quoted key never leaks out as a second parameter.
    let counts = construct_signature_param_counts(&r);
    assert!(
        !counts.is_empty() && counts.iter().all(|&c| c == 1),
        "every `new(...)` signature must have exactly one `props?` parameter, \
         got counts {:?} in: {}",
        counts,
        r
    );
    // The untouched identifier sibling still renders bare.
    assert!(
        r.contains("onPing?:"),
        "identifier keys stay unquoted: got {}",
        r
    );
}

#[test]
fn tsc_codegen_awkward_prop_keys_requote_data_dash_and_numeric() {
    let r = gen_tsc_with(
        r#"<script setup lang="ts">
defineProps<{
  "data-x"?: string;
  "404": number;
  plain: boolean;
}>()
</script><template/>"#,
        &[props_fixture(
            "",
            &[
                authored_prop("data-x", true, "string", 0, 0),
                authored_prop("404", false, "number", 0, 1),
                authored_prop("plain", false, "boolean", 0, 2),
            ],
        )],
    );

    assert!(
        r.contains("\"data-x\"?:"),
        "`data-x` must stay quoted: got {}",
        r
    );
    assert!(
        r.contains("\"404\":"),
        "numeric-like key must stay quoted: got {}",
        r
    );
    assert!(
        !r.contains(" data-x?:") && !r.contains("; 404:"),
        "awkward keys must never render bare: got {}",
        r
    );
    let counts = construct_signature_param_counts(&r);
    assert!(
        !counts.is_empty() && counts.iter().all(|&c| c == 1),
        "single `props?` parameter regardless of awkward keys, got {:?} in: {}",
        counts,
        r
    );
    assert!(
        r.contains("plain: boolean"),
        "identifier key untouched: {}",
        r
    );
}

#[test]
fn tsc_codegen_testing_mode_quoted_prop_key_stays_quoted() {
    // The Testing-mode carrier renders the same full-props surface and must
    // apply the same re-quoting.
    let r = gen_tsc_mode_with(
        r#"<script setup lang="ts">
defineProps<{
  onPing?: () => void;
  "onLate-signal"?: (sig: { sigName: string }) => void;
}>()
</script><template/>"#,
        TscMode::Testing,
        &[props_fixture(
            "",
            &[
                authored_prop("onPing", true, "() => void", 0, 0),
                authored_prop(
                    "onLate-signal",
                    true,
                    "(sig: { sigName: string }) => void",
                    0,
                    1,
                ),
            ],
        )],
    )
    .code;

    assert!(
        r.contains("\"onLate-signal\"?:"),
        "testing carrier preserves the quoted key: got {}",
        r
    );
    assert!(
        !r.contains(" onLate-signal?:"),
        "testing carrier never renders the key bare: got {}",
        r
    );
    let counts = construct_signature_param_counts(&r);
    assert!(
        !counts.is_empty() && counts.iter().all(|&c| c == 1),
        "testing carrier keeps one `props?` parameter, got {:?} in: {}",
        counts,
        r
    );
}

// ── D9 fix: complete member-key coverage, sound scan, cooked-spelling ─────────

#[test]
fn render_member_key_operates_on_cooked_value_never_guesses_already_quoted() {
    use super::script::render_member_key;

    // A genuinely bare identifier stays bare — no over-quote regression on
    // valid keys.
    assert_eq!(render_member_key("plainKey"), "plainKey");
    assert_eq!(render_member_key("$_ident0"), "$_ident0");

    // A hyphenated / non-identifier cooked key is re-quoted through the shared
    // encoder.
    assert_eq!(render_member_key("data-x"), "\"data-x\"");

    // A numeric-like key (not a valid identifier) is quoted, not emitted bare.
    assert_eq!(render_member_key("404"), "\"404\"");

    // THE cooked/source conflation: the source key `'"x"'` COOKS to the
    // three-character value `"x"` (quote, x, quote). The old `already_quoted`
    // heuristic saw the leading+trailing quote and passed it through as key
    // `x` — silently dropping the real quote characters. Operating on the
    // COOKED value, it must render as the literal whose VALUE is `"x"`, i.e.
    // the escaped `"\"x\""`.
    assert_eq!(render_member_key("\"x\""), "\"\\\"x\\\"\"");

    // A keyword is not a legal bare member-name-as-identifier in this context's
    // conservative classifier, so it is quoted (always-valid TS).
    assert_eq!(render_member_key("class"), "\"class\"");
}

#[test]
fn string_literal_escapers_preserve_quote_style_and_escape_specials() {
    use super::script::{escape_string_literal_body, quote_single_string_literal};

    // Single-quoted context escapes `'` and `\` but leaves `"` raw.
    assert_eq!(quote_single_string_literal("plain"), "'plain'");
    assert_eq!(quote_single_string_literal("a'b"), "'a\\'b'");
    assert_eq!(quote_single_string_literal("a\\b"), "'a\\\\b'");
    assert_eq!(quote_single_string_literal("say \"hi\""), "'say \"hi\"'");

    // The active delimiter is escaped; the opposite quote passes through raw.
    assert_eq!(escape_string_literal_body("a\"b", '\''), "a\"b");
    assert_eq!(escape_string_literal_body("a\"b", '"'), "a\\\"b");
    assert_eq!(escape_string_literal_body("a'b", '\''), "a\\'b");

    // Control chars / line terminators never emitted raw.
    assert_eq!(escape_string_literal_body("a\nb", '\''), "a\\nb");
    assert_eq!(escape_string_literal_body("a\u{0001}b", '"'), "a\\u0001b");
}

#[test]
fn construct_signature_param_counts_counts_across_function_typed_params() {
    // The oracle must survive `=>` in a function-typed parameter. A synthetic
    // two-parameter construct signature whose FIRST parameter is a
    // function-typed object exercises the exact shape the unsound scan
    // (decrementing depth on every `>`, including each `=>`) could not see: it
    // broke at the props object's `}` and reported only ONE parameter, so it
    // could never detect a leaked `, onLate: any` AFTER a function-typed prop.
    let two_param = "class C {\n  \
        new(props?: { onPing?: (p: { c: number }) => void }, onLate: any): Instance\n}";
    assert_eq!(
        construct_signature_param_counts(two_param),
        vec![2],
        "the scan must count BOTH top-level parameters across the function-typed \
         first parameter's `=>`"
    );

    // A single-parameter signature that itself contains multiple arrows still
    // counts as one parameter.
    let one_param = "class C {\n  \
        new(props?: { a?: () => void; b?: (x: number) => string }): Instance\n}";
    assert_eq!(
        construct_signature_param_counts(one_param),
        vec![1],
        "a lone `props?` parameter with inner arrows is one parameter"
    );
}

#[test]
fn tsc_codegen_quoted_expose_key_stays_quoted_public() {
    // defineExpose with a quoted, hyphenated key. Pre-fix the expose member was
    // pushed bare (`data-x: typeof val`) inside `ShallowUnwrapRef<{ … }>` — the
    // same invalid-TS member corruption the props path was fixed for.
    let r = gen_tsc(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const val = ref(42)
defineExpose({ "data-x": val })
</script><template/>"#,
    );

    assert!(
        r.contains("\"data-x\": typeof val"),
        "quoted expose key must stay quoted: got {}",
        r
    );
    assert!(
        !r.contains(" data-x: typeof") && !r.contains("{ data-x:") && !r.contains("; data-x:"),
        "expose key must never render bare (invalid TS member): got {}",
        r
    );
}

#[test]
fn tsc_codegen_quoted_expose_key_stays_quoted_declaration() {
    // The Declaration carrier emits the same instance shape but with a
    // placeholder member type (`unknown`); the key must still be quoted.
    let d = gen_tsc_declaration(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const val = ref(42)
defineExpose({ "data-x": val })
</script><template/>"#,
    );

    assert!(
        d.contains("\"data-x\": unknown"),
        "quoted expose key must stay quoted in the declaration carrier: got {}",
        d
    );
    assert!(
        !d.contains(" data-x: unknown") && !d.contains("{ data-x:"),
        "declaration expose key must never render bare: got {}",
        d
    );
}

#[test]
fn tsc_codegen_quoted_define_options_key_stays_quoted() {
    // A non-identifier defineOptions key flows to `options_extras` and was
    // pushed bare into the runtime `defineComponent({ … })` object literal —
    // invalid JS/TS. It must be re-quoted.
    let sfc = r#"<script setup lang="ts">
defineOptions({ "custom-opt": true })
</script><template/>"#;

    let public = gen_tsc(sfc);
    assert!(
        public.contains("\"custom-opt\": true"),
        "quoted defineOptions key must stay quoted (public): got {}",
        public
    );
    assert!(
        !public.contains(" custom-opt: true"),
        "defineOptions key must never render bare (public): got {}",
        public
    );

    let testing = gen_tsc_testing(sfc);
    assert!(
        testing.contains("\"custom-opt\": true"),
        "quoted defineOptions key must stay quoted (testing): got {}",
        testing
    );
    assert!(
        !testing.contains(" custom-opt: true"),
        "defineOptions key must never render bare (testing): got {}",
        testing
    );
}

// ── Public-API surface dialect (the generated stub's own ScriptKind) ────────

/// The public-API surface reports the DIALECT of the code it CARRIES, so the
/// consumer that writes it to a companion file can label it correctly.
///
/// TypeScript decides both what to TYPECHECK and how to PARSE from a file's
/// extension/ScriptKind: a surface written to a `.ts` file is checked under the
/// project's `strict` settings whatever language its body is actually in, and a
/// `.ts`/`.js` ScriptKind rejects JSX syntax outright. So the classification is
/// FOUR-way, never a JavaScript/TypeScript boolean.
///
/// Fails against the pre-change tree: `TscOutput` carried no dialect at all, so
/// the only companion name a consumer could compose was a hardcoded `.ts`.
#[test]
fn public_api_surface_reports_the_dialect_of_the_code_it_carries() {
    use crate::parser::types::SfcScriptDialect;

    // Positive: an Options-API JavaScript `<script>` passes a JavaScript body
    // through — labelling it TypeScript makes `strict` report TS7006 on every
    // untyped parameter of a file the user never asked to have checked.
    let js_options = gen_tsc_output(
        "<script>\nexport default { methods: { bump(step) { return step + 1 } } }\n</script>\n\
         <template><div/></template>",
    );
    assert_eq!(
        js_options.dialect,
        SfcScriptDialect::JavaScript,
        "an Options-API JavaScript `<script>` body is JavaScript: {}",
        js_options.code
    );

    // Negative: the SAME construct with `lang="ts"` stays TypeScript — the
    // classification must not relabel TypeScript surfaces (that would silently
    // DROP every real TypeScript error in an Options-API component).
    let ts_options = gen_tsc_output(
        "<script lang=\"ts\">\nexport default { methods: { bump(step: number) { return step + 1 } } }\n</script>\n\
         <template><div/></template>",
    );
    assert_eq!(
        ts_options.dialect,
        SfcScriptDialect::TypeScript,
        "an Options-API `lang=\"ts\"` body stays TypeScript: {}",
        ts_options.code
    );

    // The JSX axis, which a JavaScript/TypeScript boolean cannot express. The
    // Options-API stub copies the authored body verbatim, so an authored
    // `<div/>` needs a JSX-capable ScriptKind: `.jsx` (NOT `.js`, where JSX is
    // a syntax error) and `.tsx` (NOT `.ts`, where `<div/>` parses as a type
    // assertion).
    let jsx_options = gen_tsc_output(
        "<script lang=\"jsx\">\nexport default { render() { return <div/> } }\n</script>\n\
         <template><div/></template>",
    );
    assert_eq!(
        jsx_options.dialect,
        SfcScriptDialect::Jsx,
        "a `lang=\"jsx\"` Options-API body needs a JSX ScriptKind: {}",
        jsx_options.code
    );
    assert_eq!(
        jsx_options.dialect.extension(),
        "jsx",
        "the JSX dialect names the `.jsx` extension"
    );
    let tsx_options = gen_tsc_output(
        "<script lang=\"tsx\">\nexport default { render(): unknown { return <div/> } }\n</script>\n\
         <template><div/></template>",
    );
    assert_eq!(
        tsx_options.dialect,
        SfcScriptDialect::Tsx,
        "a `lang=\"tsx\"` Options-API body needs a TSX ScriptKind: {}",
        tsx_options.code
    );
    assert_eq!(
        tsx_options.dialect.extension(),
        "tsx",
        "the TSX dialect names the `.tsx` extension"
    );

    // Negative, and the reason this is NOT simply the SFC's script language:
    // a JavaScript `<script setup>` projects a GENERATED TypeScript declaration
    // surface. Labelling it JavaScript would put `declare`/type-annotation
    // syntax into a `.js` file — a syntax error, not a relaxed check.
    let js_setup = gen_tsc_output(
        "<script setup>\nconst props = defineProps({ label: { type: String } })\n\
         function bump(step) { return step + 1 }\n</script>\n<template><div/></template>",
    );
    assert_eq!(
        js_setup.dialect,
        SfcScriptDialect::TypeScript,
        "a JavaScript `<script setup>` still projects a TypeScript declaration \
         surface: {}",
        js_setup.code
    );
    assert!(
        js_setup.code.contains("declare"),
        "the JavaScript-setup surface really is a declaration surface (the \
         premise of the assertion above): {}",
        js_setup.code
    );

    // Negative: a template-only SFC has no authored script at all and keeps the
    // TypeScript default.
    let template_only = gen_tsc_output("<template><div/></template>");
    assert_eq!(
        template_only.dialect,
        SfcScriptDialect::TypeScript,
        "a template-only SFC keeps the TypeScript default: {}",
        template_only.code
    );
}

/// The parent-facing fallthrough surface of a JavaScript Options-API stub is
/// spelled in JSDoc — never TypeScript-only syntax — because the stub copies
/// the AUTHORED body through and its carrier is a `.js`/`.jsx` file: `type`
/// aliases, `declare module`, and `declare const` there are TS8006/TS8008/
/// TS8009/TS8010 syntax diagnostics on Verter's OWN generated lines, reported
/// whether or not `checkJs` is on.
///
/// Consumers that store the surface in a TypeScript-LABELED file regardless of
/// dialect (the LSP's `.verter.ts` API companion, the tsserver-plugin mirror,
/// the playground carrier store) take the SAME widened surface from
/// [`TscOutput::ts_carrier_code`], rendered in the ordinary TypeScript shape —
/// including the `declare module "vue"` introduce-on-absence augmentation JSDoc
/// cannot express.
///
/// Discriminates in BOTH directions: the JavaScript arm proves the widening IS
/// emitted (in JSDoc) where TypeScript syntax cannot go, and the TypeScript arm
/// proves the ordinary rendering is untouched — a regression that re-gates the
/// JavaScript arm, leaks TS-only syntax into it, or drops the TS-labeled twin
/// fails here rather than passing.
#[test]
fn a_javascript_options_api_stub_widens_in_jsdoc_with_a_typescript_labeled_twin() {
    use super::script::{FallthroughArm, InheritedComponentProps};
    use crate::parser::types::SfcScriptDialect;

    // A resolvable branch with BOTH channels populated — the element arm and
    // the root-component arm — so every construct the widening can emit is in
    // play, not just the cheapest one.
    let projection = FallthroughPropsProjection {
        arms: vec![FallthroughArm {
            root_tag: Some("a".to_string()),
            root_component_props: Some(InheritedComponentProps {
                module_specifier: "./Child.vue".to_string(),
                export_name: "default".to_string(),
                prop_names: vec!["tone".to_string()],
            }),
        }],
    };

    let gen_options_api = |script_open: &str, projection: &FallthroughPropsProjection| {
        generate_tsc_output_with_options(
            &format!(
                "{script_open}\nexport default {{ props: {{ label: {{ type: String }} }} }}\n\
                 </script>\n<template><a>x</a></template>"
            ),
            "TestComp",
            &TscGenOptions {
                filename: Some("/test/TestComp.vue".to_string()),
                ..Default::default()
            },
            MacroTscInput::NotRequired,
            projection,
        )
        .expect("options-api fixture has no typed codegen macro")
    };

    // NEGATIVE: the JavaScript stub carries no TypeScript-only construct.
    let js = gen_options_api("<script>", &projection);
    for forbidden in [
        "type __Verter_RootElementAttrs",
        "type __Verter_DataAttrs",
        "type __OmitNew",
        "declare module",
        "declare const",
    ] {
        assert!(
            !js.code.contains(forbidden),
            "a JavaScript Options-API stub must not carry `{forbidden}` — it is a \
             `.js` carrier and that is a syntax error, not a relaxed check: {}",
            js.code
        );
    }
    // POSITIVE: the widening is present, spelled in JSDoc. Both channels of the
    // arm reach the surface, and the surface reaches BOTH parent-facing sites
    // (the construct-signature parameter and the instance `$props`).
    for required in [
        "@typedef",
        "__Verter_RootElementAttrs<\"a\">",
        "__Verter_DataAttrs",
        "__Verter_FallthroughOverlay",
        "./Child.vue",
        "@type {__OmitNew<typeof __Verter_ComponentOptions>",
        "$props:",
        "export default TestComp",
    ] {
        assert!(
            js.code.contains(required),
            "the JavaScript Options-API stub widens via JSDoc and must carry \
             `{required}`: {}",
            js.code
        );
    }
    // …and it is still a usable stub: the authored body survives, so the
    // assertions above cannot be satisfied by emitting a synthetic surface.
    assert!(
        js.code.contains("props:") && js.code.contains("label"),
        "the JavaScript stub still carries its authored options body: {}",
        js.code
    );
    assert_eq!(
        js.dialect,
        SfcScriptDialect::JavaScript,
        "the widened JavaScript stub's carrier stays a `.js` file — the authored \
         body must still be checked as JavaScript: {}",
        js.code
    );

    // The TS-LABELED twin: the same surface in ordinary TypeScript shape, for
    // consumers whose carrier is a `.ts` file regardless of dialect. JSDoc
    // types are honored ONLY in JavaScript-flavored files, so shipping the
    // JSDoc rendering to a `.verter.ts` companion would silently lose the
    // widening there.
    let twin = js
        .ts_carrier_code
        .as_deref()
        .expect("a widened JavaScript Options-API stub ships a TS-labeled twin");
    for required in [
        "type __Verter_RootElementAttrs",
        "declare module \"vue\"",
        "declare const TestComp",
        "__Verter_RootElementAttrs<\"a\">",
        "./Child.vue",
    ] {
        assert!(
            twin.contains(required),
            "the TS-labeled twin renders the ordinary TypeScript widening and \
             must carry `{required}`: {twin}"
        );
    }
    assert!(
        !twin.contains("@typedef"),
        "the TS-labeled twin is the ordinary TypeScript rendering, not the JSDoc \
         one: {twin}"
    );
    assert!(
        twin.contains("props:") && twin.contains("label"),
        "the TS-labeled twin still carries the authored options body: {twin}"
    );

    // An UN-widened JavaScript stub is byte-stable: no JSDoc helpers, no twin.
    let unwidened = gen_options_api("<script>", &FallthroughPropsProjection::none());
    assert!(
        !unwidened.code.contains("@typedef") && !unwidened.code.contains("@type"),
        "an empty projection leaves the JavaScript stub un-widened: {}",
        unwidened.code
    );
    assert!(
        unwidened.ts_carrier_code.is_none(),
        "an un-widened stub has no distinct TS-labeled rendering"
    );

    // The JSX half of the JavaScript family: a `lang="jsx"` body lands in a
    // `.jsx` carrier, where TypeScript-only syntax is equally illegal and
    // JSDoc is equally honored — same treatment as plain JavaScript.
    let jsx = gen_options_api("<script lang=\"jsx\">", &projection);
    assert_eq!(
        jsx.dialect,
        SfcScriptDialect::Jsx,
        "a `lang=\"jsx\"` Options-API stub keeps its JSX carrier: {}",
        jsx.code
    );
    assert!(
        jsx.code.contains("@typedef") && !jsx.code.contains("declare const"),
        "the `.jsx` carrier widens via JSDoc exactly as `.js` does: {}",
        jsx.code
    );
    assert!(
        jsx.ts_carrier_code.is_some(),
        "the `.jsx` carrier ships the TS-labeled twin exactly as `.js` does"
    );

    // POSITIVE: the identical component with `lang="ts"` widens in TypeScript
    // directly — no JSDoc, no twin (its `code` already IS the TS rendering).
    let ts = gen_options_api("<script lang=\"ts\">", &projection);
    assert!(
        ts.code.contains("type __Verter_RootElementAttrs")
            && ts.code.contains("declare const")
            && ts.code.contains("__Verter_RootElementAttrs<\"a\">"),
        "a TypeScript Options-API stub still projects the widened element arm: {}",
        ts.code
    );
    assert!(
        ts.code.contains("./Child.vue"),
        "a TypeScript Options-API stub still projects the root-component arm: {}",
        ts.code
    );
    assert!(
        !ts.code.contains("@typedef"),
        "the TypeScript stub does not spell its widening in JSDoc: {}",
        ts.code
    );
    assert!(
        ts.ts_carrier_code.is_none(),
        "a TypeScript-dialect stub IS the TS-labeled rendering; a Some here \
         would make consumers diverge on which bytes they publish"
    );
}

/// A JavaScript `<script setup>` whose `defineExpose({ … })` would otherwise
/// have its AUTHORED body copied into the generated TypeScript stub must not
/// carry that body — the stub is a `declare`/`type`-bearing TypeScript root, so
/// an inlined JavaScript body is typechecked under `strict`/`noImplicitAny` and
/// every untyped parameter reports a false TS7006.
///
/// Fails against the pre-change tree: `needs_setup_body` was
/// `!expose_entries.is_empty()` alone, so the JavaScript body below WAS copied
/// into a surface the consumer then named `.vue.ts`.
#[test]
fn javascript_setup_body_never_enters_the_typescript_public_stub() {
    use crate::parser::types::SfcScriptDialect;

    const JS_EXPOSE: &str = "<script setup>\nfunction bump(step) { return step + 1 }\n\
         defineExpose({ bump })\n</script>\n<template><div/></template>";

    let js = gen_tsc_output(JS_EXPOSE);
    assert_eq!(
        js.dialect,
        SfcScriptDialect::TypeScript,
        "the stub is a declaration surface: {}",
        js.code
    );
    assert!(
        !js.code.contains("function bump(step)"),
        "the authored JavaScript body must NOT be copied into a TypeScript \
         root — it is exactly what produced the false TS7006 flood: {}",
        js.code
    );
    assert!(
        js.code.contains("bump: (step: any) => any"),
        "the exposed member keeps a CALLABLE type recovered from the authored \
         syntax — its own parameter name and arity, every parameter `any`, which \
         is what TypeScript gives an unannotated JavaScript function: {}",
        js.code
    );
    assert!(
        !js.code.contains("bump: unknown"),
        "`unknown` would relocate the false diagnostic to every consumer: a \
         parent cannot call an `unknown` member: {}",
        js.code
    );
    assert!(
        !js.code.contains("typeof bump"),
        "`typeof bump` would be an unbound value reference once the body is \
         omitted: {}",
        js.code
    );

    // Negative: a member whose type is the RESULT of inference rather than a
    // syntactic shape stays `unknown`. `ref(0)` is a call, not a function
    // declaration; there is nothing in the syntax to recover, and inventing
    // `any` there would silently widen a member the consumer would then use
    // unchecked. This is the bounded residue, and the authored way to say what
    // it is remains `defineExpose<{ count: Ref<number> }>()`.
    let js_ref = gen_tsc_output(
        "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\n\
         defineExpose({ count })\n</script>\n<template><div/></template>",
    );
    assert!(
        js_ref.code.contains("count: unknown"),
        "a non-function member has no recoverable call shape: {}",
        js_ref.code
    );

    // And a function-valued `const` is recovered exactly as a `function`
    // declaration is — the two spell the same thing.
    let js_arrow = gen_tsc_output(
        "<script setup>\nconst reset = (to = 0, ...rest) => to\n\
         defineExpose({ reset })\n</script>\n<template><div/></template>",
    );
    assert!(
        js_arrow
            .code
            .contains("reset: (to?: any, ...rest: any[]) => any"),
        "an arrow initializer carries its own optionality and rest parameter: {}",
        js_arrow.code
    );

    // A member declared INSIDE the macro — method shorthand, and a
    // function-valued property — carries its authored shape too, from the
    // property's own value rather than a setup declaration it never names.
    //
    // A permissive `(...args: any[]) => any` would be callable and therefore
    // "not `unknown`", but it accepts ANY arity: `child.focus()` and
    // `child.focus(1, 2)` would both type-check against a one-parameter method.
    // Arity is exactly what a consumer needs checked.
    let js_method = gen_tsc_output(
        "<script setup>\ndefineExpose({ focus(target) { return target }, \
         blur: (immediate = false) => immediate })\n</script>\n<template><div/></template>",
    );
    assert!(
        js_method.code.contains("focus: (target: any) => any"),
        "a method shorthand carries its authored name and arity: {}",
        js_method.code
    );
    assert!(
        js_method.code.contains("blur: (immediate?: any) => any"),
        "a function-valued property does too: {}",
        js_method.code
    );
    assert!(
        !js_method.code.contains("...args: any[]"),
        "no member may fall back to a variadic shape that erases its arity: {}",
        js_method.code
    );

    // A DEFAULTED parameter followed by a REQUIRED one. `?` is legal only on a
    // trailing run of parameters — TypeScript rejects a required parameter after
    // an optional one (TS1016) — while JavaScript happily allows the authored
    // form. Where the authored optionality cannot be expressed, the parameter
    // renders REQUIRED: a caller has to pass something positionally anyway, and
    // the alternative is a declaration that does not compile at all.
    let js_mixed_defaults = gen_tsc_output(
        "<script setup>\ndefineExpose({ scrollTo(region = 'top', mode) { return [region, mode] }, \
         resize(width, height = 0) { return width + height } })\n</script>\n\
         <template><div/></template>",
    );
    assert!(
        js_mixed_defaults
            .code
            .contains("scrollTo: (region: any, mode: any) => any"),
        "a default BEFORE a required parameter cannot be spelled `?`, so it \
         renders required rather than emitting TS1016: {}",
        js_mixed_defaults.code
    );
    assert!(
        !js_mixed_defaults.code.contains("region?"),
        "`(region?: any, mode: any)` is the TS1016 shape and must never be \
         emitted: {}",
        js_mixed_defaults.code
    );
    // Negative: this is a PLACEMENT rule, not "defaults are ignored". A
    // trailing default is expressible and stays optional.
    assert!(
        js_mixed_defaults
            .code
            .contains("resize: (width: any, height?: any) => any"),
        "a TRAILING default is expressible and must stay optional: {}",
        js_mixed_defaults.code
    );

    // Negative: the SAME component in TypeScript still inlines its body, so the
    // exposed binding keeps its inferred type. Without this half, "never inline"
    // would pass by inlining nothing at all.
    let ts = gen_tsc_output(
        "<script setup lang=\"ts\">\nfunction bump(step: number) { return step + 1 }\n\
         defineExpose({ bump })\n</script>\n<template><div/></template>",
    );
    assert_eq!(
        ts.dialect,
        SfcScriptDialect::TypeScript,
        "a TypeScript setup body keeps the surface TypeScript: {}",
        ts.code
    );
    assert!(
        ts.code.contains("function bump(step: number)"),
        "a TypeScript body IS still inlined: {}",
        ts.code
    );
    assert!(
        ts.code.contains("bump: typeof bump"),
        "and its exposed member resolves through `typeof`: {}",
        ts.code
    );

    // And a `lang="tsx"` body — inlined, and therefore carrying JSX — moves the
    // surface to the TSX ScriptKind, which is the only one that parses it.
    let tsx = gen_tsc_output(
        "<script setup lang=\"tsx\">\nconst node = <div/>\ndefineExpose({ node })\n</script>\n\
         <template><div/></template>",
    );
    assert_eq!(
        tsx.dialect,
        SfcScriptDialect::Tsx,
        "an inlined TSX body makes the surface TSX: {}",
        tsx.code
    );
    assert!(
        tsx.code.contains("const node = <div/>"),
        "the TSX body really is inlined (the premise of the assertion above): {}",
        tsx.code
    );
}

/// The `<script setup>` body is parsed under the AUTHORED dialect's ScriptKind,
/// on BOTH axes — so TypeScript-only syntax in a `lang="js"`/`lang="jsx"` body
/// is not silently accepted, and JSX in a `lang="jsx"` body is not lost.
///
/// That parse is not a private detail of macro extraction: the same program
/// feeds item/import/binding collection, test bindings, type imports, usage
/// tracking, macro state, attrs-type parsing, and the scope inventories. The
/// observable used here is the testing surface's binding list, which is
/// populated directly from it.
///
/// Fails against a two-way `is_jsx ? tsx() : ts()` mapping: it reads EVERY
/// JavaScript body as TypeScript, so the `lang="js"` case below collects
/// `shown`/`n` from a body that is not valid JavaScript at all, and the
/// `lang="jsx"` case is read as TSX for the same reason. The JavaScript arms
/// are not merely over-accepted either — in TypeScript mode `a < b > (c)` is a
/// generic call and in JavaScript two comparisons, so a JavaScript body read as
/// TypeScript can be MIS-parsed, not just leniently parsed.
#[test]
fn the_setup_body_is_parsed_under_its_authored_script_kind() {
    // TypeScript-only syntax (`interface`, a type annotation) in a body the
    // author declared JavaScript. Under a JavaScript ScriptKind it does not
    // parse, so nothing is collected from it — which is the honest answer: the
    // engine will reject the same body, and Vue's own `compileScript` runs a
    // JavaScript parse over a `lang="js"` block.
    const TS_ONLY_BODY: &str = "interface Local { label: string }\n\
                                const shown: Local = { label: 'x' }\n\
                                const n = 1\n";

    let ts = gen_tsc_testing(&format!(
        "<script setup lang=\"ts\">\n{TS_ONLY_BODY}</script>\n<template><div/></template>"
    ));
    assert!(
        ts.contains("shown: typeof shown") && ts.contains("n: typeof n"),
        "the TypeScript body parses and its bindings are collected — the \
         premise the JavaScript assertion below discriminates against: {ts}"
    );

    let js = gen_tsc_testing(&format!(
        "<script setup lang=\"js\">\n{TS_ONLY_BODY}</script>\n<template><div/></template>"
    ));
    assert!(
        js.contains("type __Verter_TestBindings = {}"),
        "TypeScript-only syntax in a `lang=\"js\"` body must not be silently \
         accepted: {js}"
    );
    assert!(
        !js.contains("shown: typeof shown") && !js.contains("n: typeof n"),
        "no binding may be recovered from a body a JavaScript ScriptKind \
         cannot parse: {js}"
    );

    // Negative, and the half that stops the assertion above from passing for a
    // parser that simply fails on every JavaScript body: a VALID JavaScript
    // body still parses and still yields its bindings.
    let valid_js = gen_tsc_testing(
        "<script setup lang=\"js\">\nconst shown = { label: 'x' }\nconst n = 1\n</script>\n\
         <template><div/></template>",
    );
    assert!(
        valid_js.contains("shown: typeof shown") && valid_js.contains("n: typeof n"),
        "a valid JavaScript body still parses under the JavaScript ScriptKind: \
         {valid_js}"
    );

    // The JSX axis of the same classification, which a JavaScript/TypeScript
    // boolean cannot express: `lang="jsx"` is JavaScript AND JSX-capable.
    const JSX_BODY: &str = "const node = <span class=\"badge\">hi</span>\nconst n = 1\n";
    let jsx = gen_tsc_testing(&format!(
        "<script setup lang=\"jsx\">\n{JSX_BODY}</script>\n<template><div/></template>"
    ));
    assert!(
        jsx.contains("node: typeof node") && jsx.contains("n: typeof n"),
        "a `lang=\"jsx\"` body must reach a JSX-capable ScriptKind — collapsing \
         it onto plain JavaScript loses the whole body to a parse error: {jsx}"
    );

    // And the same JSX body under `lang="js"` does NOT parse — the two
    // JavaScript dialects are genuinely distinct, not one arm wearing two names.
    let jsx_as_js = gen_tsc_testing(&format!(
        "<script setup lang=\"js\">\n{JSX_BODY}</script>\n<template><div/></template>"
    ));
    assert!(
        !jsx_as_js.contains("node: typeof node"),
        "JSX is a syntax error in a non-JSX JavaScript ScriptKind: {jsx_as_js}"
    );
}

/// The TESTING surface reports the dialect of the code it carries, on both axes
/// — the same contract the public-API surface above already satisfies.
///
/// It copies the authored `<script setup>` body VERBATIM (that is its whole
/// purpose: `typeof <binding>` must resolve, so omitting the body is TS2304),
/// which makes it the one surface whose dialect is fully the author's:
///
/// - a `lang="tsx"`/`lang="jsx"` body carries JSX, and a `.ts` ScriptKind parses
///   `<span/>` as a type assertion — a syntax error, not a relaxed check;
/// - a `lang="js"`/`lang="jsx"` body is JavaScript, and a `.ts`/`.tsx` root is
///   ALWAYS checked, so `strict`/`noImplicitAny` reports TS7006 on every untyped
///   parameter of a file the project never asked to have checked.
///
/// **IGNORED: the implementation is not here yet, and this test says so rather
/// than passing vacuously.** Un-ignoring it today FAILS on the first assertion.
/// Two things are missing, and the JSX axis cannot land without the second:
///
/// 1. The emitter. The JSX axis is a label — a `.tsx` root holds the generated
///    `type`/`declare` frame AND an authored JSX body. The JAVASCRIPT axis is
///    not: the frame is `type __OmitNew<T> = …`, `declare function defineProps…`,
///    `declare const C: … & { new(…): … }` — none of it legal in a `.js`/`.jsx`
///    root — so a JavaScript testing carrier needs the frame expressed as JSDoc,
///    or the authored body moved to its own JavaScript module the frame reads
///    through `typeof import(…)`. Neither is a label change.
/// 2. The NAME. `VirtualFileNaming::testing_api_suffix` is a single
///    `&'static str` pinned to `.__verter_test.ts`, mirrored into
///    `packages/language-shared/src/virtual-file-naming.generated.ts` and
///    `client-framework-manifest.generated.ts` and byte-pinned by their
///    freshness guards. Reporting a dialect here changes nothing until that
///    column enumerates its alternatives the way the `ide` column's
///    `JsxConditional` already does, and a production emitter picks between them
///    from the dialect. Until then a dialect on this surface would be an inert
///    field — which is why this test is ignored rather than the field changed.
#[test]
#[ignore = "the four-way testing carrier is not implemented: see the doc comment \
            — the JavaScript frame and the descriptor naming column both have to \
            change, and a dialect reported here alone would be inert"]
fn testing_surface_reports_the_dialect_of_the_code_it_carries() {
    use crate::parser::types::SfcScriptDialect;

    // A `lang="tsx"` body carries JSX into the surface verbatim.
    let tsx = gen_tsc_output_testing(
        "<script setup lang=\"tsx\">\nconst node = <span class=\"badge\">hi</span>\n</script>\n\
         <template><div/></template>",
    );
    assert!(
        tsx.code
            .contains("const node = <span class=\"badge\">hi</span>"),
        "the body really is copied verbatim (the premise of every assertion \
         here): {}",
        tsx.code
    );
    assert_eq!(
        tsx.dialect,
        SfcScriptDialect::Tsx,
        "a copied JSX body needs a JSX-capable ScriptKind: {}",
        tsx.code
    );

    // And a JavaScript body makes the surface JavaScript — the axis that also
    // needs a JSDoc frame, not just a label.
    let js = gen_tsc_output_testing(
        "<script setup lang=\"js\">\nfunction bump(step) { return step + 1 }\n</script>\n\
         <template><div/></template>",
    );
    assert_eq!(
        js.dialect,
        SfcScriptDialect::JavaScript,
        "an untyped JavaScript body must not be labelled a checked TypeScript \
         root: {}",
        js.code
    );
    let jsx = gen_tsc_output_testing(
        "<script setup lang=\"jsx\">\nconst node = <span/>\nfunction bump(step) { return step }\n\
         </script>\n<template><div/></template>",
    );
    assert_eq!(
        jsx.dialect,
        SfcScriptDialect::Jsx,
        "JavaScript AND JSX is its own dialect, not either one alone: {}",
        jsx.code
    );

    // Negative: a TypeScript body keeps the TypeScript label — the four arms are
    // distinct, not one relabelling that moves everything.
    let ts = gen_tsc_output_testing(
        "<script setup lang=\"ts\">\nconst n: number = 1\n</script>\n<template><div/></template>",
    );
    assert_eq!(
        ts.dialect,
        SfcScriptDialect::TypeScript,
        "a TypeScript body stays TypeScript: {}",
        ts.code
    );
}
