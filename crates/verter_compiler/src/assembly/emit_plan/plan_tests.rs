use verter_audit::observe::Uncaptured;
use verter_span::Span;

use super::super::{
    EmissionBuilder, EmissionSegment, EmitPlan, EmitPlanBuilder, EmitPlanError, EmittedArtifact,
    MapMode, MapSource, MarkerId, Piece, PieceText, Provenance,
};
use crate::assembly::fragment::{
    final_module_parse_errors, test_witness, FragmentDialect, SyntacticContract,
};
use crate::code_transform::{
    code_transform_build_string_call_count, reset_code_transform_build_string_call_count,
    CodeTransform, EditRecorder, MappingProduct, ProjectedClass, SourceMapOptions,
};
use crate::emission::{map_capture_events, reset_map_capture_events, test_source};
use crate::work_ledger::CompilerWorkLedger;

fn write(plan: EmitPlan<'_>) -> EmittedArtifact {
    plan.write(&mut CompilerWorkLedger::<Uncaptured>::new())
        .unwrap()
}

#[test]
fn sequential_plan_keeps_order() {
    let source = test_source("App.vue", "abcdef");
    let mut builder = EmitPlanBuilder::new(MapMode::NoMap);
    builder.push_source(&source, Span::new(0, 2)).unwrap();
    builder.push_unmapped("X");
    builder.push_source(&source, Span::new(4, 6)).unwrap();
    let plan = builder.finish().unwrap();
    assert_eq!(plan.len(), 5);
    assert_eq!(plan.segments().len(), 1);
    assert!(matches!(plan.segments()[0], EmissionSegment::Fragment(_)));
    assert_eq!(write(plan).text(), "abXef");
}

#[test]
fn a_slice_outside_its_source_is_refused() {
    let source = test_source("App.vue", "abc");
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    assert_eq!(
        builder.push_source(&source, Span::new(1, 4)),
        Err(EmitPlanError::InvalidSourceSlice {
            span: Span::new(1, 4)
        })
    );
    assert_eq!(builder.finish().unwrap().len(), 0);
}

#[test]
fn a_slice_splitting_a_code_point_is_refused() {
    // "é" is two bytes: offset 2 is inside it.
    let source = test_source("App.vue", "aéb");
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    assert_eq!(
        builder.push_source(&source, Span::new(0, 2)),
        Err(EmitPlanError::InvalidSourceSlice {
            span: Span::new(0, 2)
        })
    );
    assert_eq!(builder.push_source(&source, Span::new(0, 3)), Ok(()));
}

#[test]
fn equal_keys_merge_and_equal_bytes_with_other_keys_do_not() {
    let text = String::from("same");
    let copy = String::from("same");
    let first = test_source("App.vue", &text);
    // The same key over another allocation of the same bytes.
    let again = test_source("App.vue", &copy);
    // Equal bytes from another unit.
    let other = test_source("Other.vue", &text);
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder.push_source(&first, Span::new(0, 1)).unwrap();
    builder.push_source(&again, Span::new(1, 2)).unwrap();
    builder.push_source(&other, Span::new(2, 3)).unwrap();
    let plan = builder.finish().unwrap();
    assert_eq!(plan.sources().len(), 2);
    let artifact = write(plan);
    assert_eq!(artifact.text(), "sam");
    let map = artifact.map().unwrap();
    assert_eq!(map.lookup(0), Some((0, 0)));
    assert_eq!(map.lookup(1), Some((0, 1)));
    assert_eq!(map.lookup(2), Some((1, 2)));
}

#[test]
fn a_fragment_keeps_its_own_source_identity() {
    let template = test_source("App.vue#template", "<div>{{ msg }}</div>");
    let script = test_source("App.vue#script", "const msg = 1");

    // The script fragment is built over its own source.
    let mut script_fragment = EmissionBuilder::new(MapMode::Map).with_source(script.clone());
    script_fragment.push_source(Span::new(0, 13)).unwrap();
    let script_fragment = script_fragment.finish().unwrap();

    // The template is registered first; the script fragment follows.
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder.push_anchored("_ctx.msg", &template, 8);
    builder.push_unmapped(";\n");
    builder.push_fragment(script_fragment).unwrap();
    let plan = builder.finish().unwrap();
    assert_eq!(plan.sources().len(), 2);
    let artifact = write(plan);
    assert_eq!(artifact.text(), "_ctx.msg;\nconst msg = 1");
    assert!(artifact.sources()[0].same(&template.key));
    assert!(artifact.sources()[1].same(&script.key));

    let map = artifact.map().unwrap();
    assert_eq!(map.lookup(0), Some((0, 8)));
    // Every byte of the script fragment names the script, at its own offset.
    for generated in 10..23 {
        assert_eq!(
            map.lookup(generated),
            Some((1, generated - 10)),
            "{generated}"
        );
    }
}

#[test]
fn a_handle_from_another_plan_is_refused() {
    let mut first = EmitPlanBuilder::new(MapMode::NoMap);
    let mut second = EmitPlanBuilder::new(MapMode::NoMap);
    let handle = first.reserve_slot(SyntacticContract::Expression);
    assert_eq!(second.fill_empty(handle), Err(EmitPlanError::ForeignHandle));
}

#[test]
fn a_fragment_in_the_wrong_grammar_context_is_refused() {
    let mut builder = EmitPlanBuilder::new(MapMode::NoMap);
    let handle = builder.reserve_slot(SyntacticContract::Expression);
    let mut fragment = EmissionBuilder::new(MapMode::NoMap);
    fragment.push_unmapped("let x = 1;");
    assert_eq!(
        builder.fill_fragment(
            handle,
            fragment.finish().unwrap(),
            test_witness(
                SyntacticContract::StatementList,
                FragmentDialect::JavaScript
            )
        ),
        Err(EmitPlanError::SlotContractMismatch {
            slot: SyntacticContract::Expression,
            fragment: SyntacticContract::StatementList,
        })
    );
    // A nested plan declares its grammar context too.
    let handle = builder.reserve_slot(SyntacticContract::Expression);
    let mut nested = EmitPlanBuilder::new(MapMode::NoMap);
    nested.push_unmapped("let y = 1;");
    assert_eq!(
        builder.fill_plan(
            handle,
            nested.finish().unwrap(),
            SyntacticContract::StatementList
        ),
        Err(EmitPlanError::SlotContractMismatch {
            slot: SyntacticContract::Expression,
            fragment: SyntacticContract::StatementList,
        })
    );
    // The slots stay unfilled.
    assert_eq!(builder.finish().err(), Some(EmitPlanError::UnfilledSlot));
}

#[test]
fn two_revisions_of_one_source_are_refused() {
    let before = test_source("App.vue", "let a");
    let after = test_source("App.vue", "let b");
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder.push_source(&before, Span::new(0, 3)).unwrap();
    let mut fragment = EmissionBuilder::new(MapMode::Map).with_source(after.clone());
    fragment.push_source(Span::new(4, 5)).unwrap();
    assert_eq!(
        builder.push_fragment(fragment.finish().unwrap()),
        Err(EmitPlanError::SourceRevisionConflict { key: after.key })
    );
}

#[test]
fn a_fragment_under_the_other_map_policy_is_refused() {
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    let text_only = EmissionBuilder::new(MapMode::NoMap).finish().unwrap();
    assert_eq!(
        builder.push_fragment(text_only),
        Err(EmitPlanError::ModeMismatch)
    );
    let handle = builder.reserve_slot(SyntacticContract::StatementList);
    assert_eq!(
        builder.fill_plan(
            handle,
            EmitPlanBuilder::new(MapMode::NoMap).finish().unwrap(),
            SyntacticContract::StatementList
        ),
        Err(EmitPlanError::ModeMismatch)
    );
}

#[test]
fn an_unfilled_slot_fails_finish() {
    let mut builder = EmitPlanBuilder::new(MapMode::NoMap);
    builder.push_unmapped("a");
    let _handle = builder.reserve_slot(SyntacticContract::Expression);
    assert_eq!(builder.finish().err(), Some(EmitPlanError::UnfilledSlot));
}

#[test]
fn an_empty_slot_emits_nothing() {
    let mut builder = EmitPlanBuilder::new(MapMode::NoMap);
    builder.push_unmapped("a");
    let handle = builder.reserve_slot(SyntacticContract::Expression);
    builder.push_unmapped("b");
    builder.fill_empty(handle).unwrap();
    let plan = builder.finish().unwrap();
    assert_eq!(plan.len(), 2);
    assert_eq!(write(plan).text(), "ab");
}

#[test]
fn plan_length_is_cached_and_exact() {
    let source = test_source("App.vue", "abcdef");
    let mut inner = EmitPlanBuilder::new(MapMode::Map);
    inner.push_source(&source, Span::new(1, 4)).unwrap();
    inner.push_unmapped_fmt(format_args!("{}", 12345));
    let inner = inner.finish().unwrap();

    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    let handle = builder.reserve_slot(SyntacticContract::StatementList);
    for index in 0..1_000u32 {
        if index % 2 == 0 {
            builder.push_unmapped("é;");
        } else {
            builder.push_anchored("x", &source, index % 6);
        }
    }
    builder
        .fill_plan(handle, inner, SyntacticContract::StatementList)
        .unwrap();
    let plan = builder.finish().unwrap();
    let len = plan.len();
    assert_eq!(len, 3 + 5 + 500 * 3 + 500);
    let (text, _) = write(plan).into_parts();
    assert_eq!(text.len(), len as usize);
    assert_eq!(text.capacity(), len as usize);
}

#[test]
fn a_marked_slot_is_replaced_before_writing() {
    let source = test_source("main.js", "const __sfc__ = {};\nexport default __sfc__;\n");
    let binding = MarkerId::fresh();
    let export = MarkerId::fresh();
    let mut script = EmissionBuilder::new(MapMode::Map).with_source(source.clone());
    script.push_source(Span::new(0, 6)).unwrap();
    script
        .push_marked(
            Piece::new(
                PieceText::Source(Span::new(6, 13)),
                Provenance::Exact { source_start: 6 },
            ),
            binding,
        )
        .unwrap();
    script.push_source(Span::new(13, 20)).unwrap();
    script
        .push_marked(
            Piece::new(
                PieceText::Source(Span::new(20, 44)),
                Provenance::Exact { source_start: 20 },
            ),
            export,
        )
        .unwrap();
    let script = script.finish().unwrap();
    let binding_slot = script.slot(binding).unwrap();
    let export_slot = script.slot(export).unwrap();
    let duplicate = script.slot(binding).unwrap();

    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder.push_fragment(script).unwrap();
    let mut name = EmissionBuilder::new(MapMode::Map);
    name.push_unmapped("_sfc_main");
    builder
        .replace_marked(binding_slot, name.finish().unwrap())
        .unwrap();
    builder
        .replace_marked(
            export_slot,
            EmissionBuilder::new(MapMode::Map).finish().unwrap(),
        )
        .unwrap();
    // A slot is filled once.
    assert_eq!(
        builder.replace_marked(
            duplicate,
            EmissionBuilder::new(MapMode::Map).finish().unwrap()
        ),
        Err(EmitPlanError::UnknownSlot)
    );
    let plan = builder.finish().unwrap();
    let len = plan.len();
    let artifact = write(plan);
    assert_eq!(artifact.text(), "const _sfc_main = {};\n");
    assert_eq!(artifact.text().len(), len as usize);
    let map = artifact.map().unwrap();
    // Authored bytes after the replaced binding keep their own offsets.
    assert_eq!(map.lookup(15), Some((0, 13)));
    // The replacement is synthetic.
    assert_eq!(map.lookup(6), None);
}

/// One transform over `source` with an exact run, a rewritten replacement,
/// a synthetic overwrite and an unmapped insertion.
fn edited<'a>(
    source: &'a str,
    allocator: &'a oxc_allocator::Allocator,
    mode: MapMode,
) -> CodeTransform<'a> {
    let mut transform = CodeTransform::with_mode_in(source, allocator, mode);
    transform.overwrite(4, 5, "Bee");
    transform.overwrite_unmapped(8, 9, "__sfc__");
    transform.prepend_left(10, "\n/* tail */");
    transform
}

const SCRIPT: &str = "let a = b;";

#[test]
fn a_frozen_transform_writes_what_build_string_builds() {
    let allocator = oxc_allocator::Allocator::default();
    let reference = edited(SCRIPT, &allocator, MapMode::Map);
    let expected_text = reference.build_string();
    let expected = reference
        .generate_map(SourceMapOptions::new().with_source("App.vue"))
        .unwrap();

    let source = test_source("App.vue", SCRIPT);
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder
        .push_transform(edited(SCRIPT, &allocator, MapMode::Map), source)
        .unwrap();
    let artifact = write(builder.finish().unwrap());
    assert_eq!(artifact.text(), expected_text);

    let written = artifact.map().unwrap().to_source_map(
        artifact.text(),
        &[MapSource {
            name: "App.vue",
            content: Some(SCRIPT),
        }],
        None,
    );
    // One wire policy: the transform's own map and the writer place the
    // same tokens, sourceless ones included.
    let tokens = |map: &oxc_sourcemap::SourceMap<'_>| {
        map.get_tokens()
            .map(|token| {
                (
                    token.get_dst_line(),
                    token.get_dst_col(),
                    token
                        .get_source_id()
                        .map(|source| (source, token.get_src_line(), token.get_src_col())),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(tokens(&written), tokens(&expected));
    assert!(tokens(&written).iter().any(|token| token.2.is_none()));
}

#[test]
fn a_synthetic_overwrite_is_never_an_authored_anchor() {
    let allocator = oxc_allocator::Allocator::default();
    let reference = edited(SCRIPT, &allocator, MapMode::Map);
    let product = MappingProduct::of(&reference).unwrap();

    let source = test_source("App.vue", SCRIPT);
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder
        .push_transform(edited(SCRIPT, &allocator, MapMode::Map), source)
        .unwrap();
    let artifact = write(builder.finish().unwrap());
    // "let Bee = __sfc__;": the synthetic `__sfc__` is bytes 10..17.
    let synthetic = artifact.text().find("__sfc__").unwrap() as u32;
    let map = artifact.map().unwrap();
    for generated in synthetic..synthetic + 7 {
        assert_eq!(map.lookup(generated), None, "{generated}");
        assert_eq!(
            product.projected_at(generated).map(|region| region.class),
            Some(ProjectedClass::Synthesized),
            "{generated}"
        );
    }
    // The rewritten replacement stays mapped to the span it stands for.
    assert_eq!(map.lookup(5), Some((0, 4)));
}

#[test]
fn a_text_only_transform_hand_off_captures_nothing() {
    let allocator = oxc_allocator::Allocator::default();
    let source = test_source("App.vue", SCRIPT);
    let transform = edited(SCRIPT, &allocator, MapMode::NoMap);
    reset_map_capture_events();
    reset_code_transform_build_string_call_count();
    let mut builder = EmitPlanBuilder::new(MapMode::NoMap);
    builder.push_transform(transform, source.clone()).unwrap();
    let artifact = write(builder.finish().unwrap());
    assert_eq!(artifact.text(), "let Bee = __sfc__;\n/* tail */");
    assert_eq!(artifact.map(), None);
    assert_eq!(map_capture_events(), 0);
    // The hand-off builds no intermediate string.
    assert_eq!(code_transform_build_string_call_count(), 0);

    // Mapped control: the same edits carry a map.
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder
        .push_transform(edited(SCRIPT, &allocator, MapMode::Map), source)
        .unwrap();
    assert!(!write(builder.finish().unwrap())
        .map()
        .unwrap()
        .segments()
        .is_empty());
}

#[test]
fn a_transform_under_the_other_policy_or_source_is_refused() {
    let allocator = oxc_allocator::Allocator::default();
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    assert_eq!(
        builder.push_transform(
            edited(SCRIPT, &allocator, MapMode::NoMap),
            test_source("App.vue", SCRIPT)
        ),
        Err(EmitPlanError::ModeMismatch)
    );
    let copy = String::from(SCRIPT);
    assert!(matches!(
        builder.push_transform(
            edited(SCRIPT, &allocator, MapMode::Map),
            test_source("App.vue", &copy)
        ),
        Err(EmitPlanError::Edit(_))
    ));
}

#[test]
fn a_written_module_parses() {
    let allocator = oxc_allocator::Allocator::default();
    let module = "import { ref } from \"vue\";\nconst count = ref(0);\n";
    let mut transform: CodeTransform<'_> =
        CodeTransform::with_mode_in(module, &allocator, MapMode::Map);
    transform.overwrite(33, 38, "_sfc_count");
    let source = test_source("App.vue", module);
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder.push_unmapped("/* generated */\n");
    builder.push_transform(transform, source).unwrap();
    builder.push_unmapped("export default {};\n");
    let artifact = write(builder.finish().unwrap());
    assert!(artifact.text().contains("const _sfc_count = ref(0);"));
    assert_eq!(
        final_module_parse_errors(artifact.text(), FragmentDialect::JavaScript),
        None
    );
}

#[cfg(feature = "semantic-observe")]
#[test]
fn a_hand_off_copies_each_byte_once() {
    use crate::work_ledger::WorkKind;
    use verter_audit::observe::Captured;

    let allocator = oxc_allocator::Allocator::default();
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder
        .push_transform(
            edited(SCRIPT, &allocator, MapMode::Map),
            test_source("App.vue", SCRIPT),
        )
        .unwrap();
    let mut ledger = CompilerWorkLedger::<Captured>::new();
    let artifact = builder.finish().unwrap().write(&mut ledger).unwrap();
    let counts = ledger.snapshot().expect("a captured ledger exports counts");
    assert_eq!(
        counts.count(WorkKind::EmissionCopyBytes),
        artifact.text().len() as u64
    );
    assert_eq!(counts.count(WorkKind::EmissionAllocations), 1);
}

/// One SFC-shaped script through recorder, transform, freeze, plan and write
/// under `mode`.
fn compile_through_emission<M: verter_audit::observe::ObserveExec>(
    mode: MapMode,
    ledger: &mut CompilerWorkLedger<M>,
) -> EmittedArtifact {
    const SFC: &str = "<script setup>\nconst msg = 1\n</script>\n<template>{{ msg }}</template>\n";
    let allocator = oxc_allocator::Allocator::default();
    let mut edits = EditRecorder::with_mode(&allocator, mode);
    edits.overwrite(0, 15, "");
    edits.prepend_alloc_mapped(21, 21, "$setup.");
    edits.overwrite(29, 70, "\nexport default {}\n");
    let mut transform: CodeTransform<'_> = CodeTransform::with_mode_in(SFC, &allocator, mode);
    edits.apply(&mut transform).unwrap();
    let mut builder = EmitPlanBuilder::new(mode);
    builder
        .push_transform(transform, test_source("App.vue", SFC))
        .unwrap();
    builder.finish().unwrap().write(ledger).unwrap()
}

#[test]
fn no_map_compile_captures_nothing() {
    reset_map_capture_events();
    let artifact =
        compile_through_emission(MapMode::NoMap, &mut CompilerWorkLedger::<Uncaptured>::new());
    assert_eq!(
        artifact.text(),
        "const $setup.msg = 1\n\nexport default {}\n"
    );
    assert_eq!(artifact.map(), None);
    assert_eq!(map_capture_events(), 0);

    // Mapped control over the same input: captures happen and a map exists.
    reset_map_capture_events();
    let artifact =
        compile_through_emission(MapMode::Map, &mut CompilerWorkLedger::<Uncaptured>::new());
    assert!(map_capture_events() > 0);
    assert!(!artifact.map().unwrap().segments().is_empty());

    #[cfg(feature = "semantic-observe")]
    {
        use crate::work_ledger::WorkKind;
        use verter_audit::observe::Captured;
        let segments = |mode| {
            let mut ledger = CompilerWorkLedger::<Captured>::new();
            compile_through_emission(mode, &mut ledger);
            ledger
                .snapshot()
                .expect("a captured ledger exports counts")
                .count(WorkKind::EmissionMapSegments)
        };
        assert_eq!(segments(MapMode::NoMap), 0);
        assert!(segments(MapMode::Map) > 0);
    }
}

#[test]
fn a_plan_of_one_fragment_hands_it_over_whole() {
    let source = test_source("App.vue", "abcdef");
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder.push_unmapped("return ");
    builder.push_source(&source, Span::new(0, 3)).unwrap();
    let fragment = builder.finish().unwrap().into_single_fragment().unwrap();
    assert_eq!(fragment.len(), 10);
    assert!(fragment
        .source()
        .is_some_and(|own| own.key.same(&source.key)));

    // Anything more than one placed fragment is not one fragment.
    let mut slotted = EmitPlanBuilder::new(MapMode::Map);
    slotted.push_unmapped("a");
    let slot = slotted.reserve_slot(SyntacticContract::Expression);
    slotted.fill_empty(slot).unwrap();
    assert_eq!(
        slotted
            .finish()
            .unwrap()
            .into_single_fragment()
            .unwrap_err(),
        EmitPlanError::NotOneFragment
    );
    assert_eq!(
        EmitPlanBuilder::new(MapMode::Map)
            .finish()
            .unwrap()
            .into_single_fragment()
            .unwrap_err(),
        EmitPlanError::NotOneFragment
    );
}

#[test]
fn a_fragment_in_a_witnessed_slot_takes_witnessed_fills() {
    let source = test_source("App.vue", "<x>");
    let allocator = oxc_allocator::Allocator::default();
    let mut ct: CodeTransform<'_> =
        CodeTransform::with_mode_in(source.text, &allocator, MapMode::Map);
    let content = ct.alloc_str("const __sfc__ = 1\n");
    ct.overwrite_unmapped_alloc(0, 3, content);
    let marker = ct.mark_generated_sub_range(content, 6, 13).unwrap();
    let id = MarkerId::fresh();
    let fragment = ct.freeze_marked(source.clone(), &[(marker, id)]).unwrap();
    let marked = fragment.slot(id).unwrap();
    let witness = test_witness(
        SyntacticContract::CompleteModule,
        FragmentDialect::JavaScript,
    );

    let mut plan = EmitPlanBuilder::witnessed(MapMode::Map, witness);
    let slot = plan.reserve_slot(SyntacticContract::CompleteModule);
    plan.fill_fragment(slot, fragment, witness).unwrap();
    let mut name = EmissionBuilder::new(MapMode::Map);
    name.push_borrowed("_sfc_main");
    plan.replace_marked_witnessed(marked, name.finish().unwrap(), witness)
        .unwrap();
    let plan = plan.finish().unwrap();
    assert_eq!(plan.last_byte(), Some(b'\n'));
    let written = write(plan);
    assert_eq!(written.text(), "const _sfc_main = 1\n");
    assert_eq!(written.grammar(), Some(witness), "every part was witnessed");
}
