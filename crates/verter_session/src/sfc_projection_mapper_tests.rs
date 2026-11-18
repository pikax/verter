//! Unit tests for lowering a checking module onto the content-mapper plane.

use std::sync::Arc;

use verter_compiler::code_transform::{MappingSpan, ProjectedClass};
use verter_compiler::framework_common::vue_projection_backend::{
    CheckingModule, CheckingModuleRefusal, SetupProjectionRefusal,
};
use verter_language::carrier_grammar::CarrierGrammarAuthority;

use crate::content_mapper::{CarrierAnswer, Refusal};
use crate::sfc_projection_mapper::*;

const CARRIER: &str = "/ws/src/Comp.vue";

const COUNTER: &str = concat!(
    "<template>\n  <p>{{ count }}</p>\n</template>\n",
    "<script setup lang=\"ts\">\n",
    "import { ref } from 'vue'\n",
    "const count = ref(1)\n",
    "</script>\n",
);

/// Script, a branch, a loop, a component, a handler and interpolations in
/// one carrier: every region kind the composer emits today.
const KITCHEN_SINK: &str = concat!(
    "<script lang=\"ts\">\n",
    "export interface Item { id: number; label: string }\n",
    "</script>\n",
    "<script setup lang=\"ts\">\n",
    "import { ref, computed } from 'vue'\n",
    "import Child from './Child.vue'\n",
    "const items = ref<Item[]>([])\n",
    "const selected = ref<Item | null>(null)\n",
    "const total = computed(() => items.value.length)\n",
    "function pick(item: Item) { selected.value = item }\n",
    "</script>\n",
    "<template>\n",
    "  <section>\n",
    "    <h1>{{ total }} items</h1>\n",
    "    <p v-if=\"selected\">{{ selected.label }}</p>\n",
    "    <p v-else>none</p>\n",
    "    <ul>\n",
    "      <li v-for=\"item in items\" :key=\"item.id\" @click=\"pick(item)\">{{ item.label }}</li>\n",
    "    </ul>\n",
    "    <Child :count=\"total\" />\n",
    "  </section>\n",
    "</template>\n",
);

fn settings(custom_elements: &[&str]) -> MapperProjectSettings {
    MapperProjectSettings::new(
        Arc::from("/ws/tsconfig.json"),
        custom_elements
            .iter()
            .map(|prefix| prefix.to_string())
            .collect(),
        Vec::new(),
        Vec::new(),
    )
}

fn open(adapter: &ContentMapperProjectionAdapter, name: &str) -> MapperProjectHandle {
    let handle = MapperProjectHandle(Arc::from(name));
    adapter
        .open_project(handle.clone(), settings(&[]))
        .expect("a new project opens");
    handle
}

fn transformed(source: &str) -> MapperTransformResult {
    let adapter = ContentMapperProjectionAdapter::new();
    let handle = open(&adapter, "project");
    adapter
        .transform(&handle, CARRIER, source)
        .expect("the project is open")
}

#[test]
fn editing_one_of_ten_carriers_composes_only_that_carrier_and_close_drains_its_cache() {
    let adapter = ContentMapperProjectionAdapter::new();
    let handle = open(&adapter, "project");
    let other = open(&adapter, "other");
    let source = "<script setup lang=\"ts\">const value = 1;</script>";
    let edited = source.replace("1", "2");
    for index in 0..10 {
        let path = std::path::Path::new("/ws").join(format!("Comp{index}.vue"));
        let path = path.to_str().unwrap();
        let fresh = adapter.transform(&handle, path, source).unwrap();
        assert!(
            fresh.complete,
            "script checking is complete: {:?}",
            fresh.diagnostics
        );
    }
    assert_eq!(adapter.composition_count(), 10);
    assert_eq!(adapter.retained_projections(), 10);
    let before = adapter.composition_count();
    for index in 0..10 {
        let path = std::path::Path::new("/ws").join(format!("Comp{index}.vue"));
        let path = path.to_str().unwrap();
        let content = if index == 4 { &edited } else { source };
        let cached = adapter.transform(&handle, path, content).unwrap();
        let fresh = transformed(content);
        assert_eq!(cached.text, fresh.text);
        assert_eq!(cached.mappings, fresh.mappings);
        assert_eq!(cached.diagnostics, fresh.diagnostics);
        assert_eq!(cached.dialect, fresh.dialect);
        assert_eq!(cached.complete, fresh.complete);
    }
    assert_eq!(adapter.composition_count() - before, 1);
    adapter.transform(&other, CARRIER, source).unwrap();
    assert_eq!(adapter.retained_projections(), 12);
    adapter.close_project(&handle).unwrap();
    assert_eq!(adapter.retained_projections(), 1);
    adapter.transform(&other, CARRIER, source).unwrap();
    assert_eq!(adapter.composition_count(), 12);
    adapter.open_project(handle.clone(), settings(&[])).unwrap();
    adapter.transform(&handle, "/ws/Comp4.vue", source).unwrap();
    assert_eq!(adapter.composition_count(), 13);
}

#[test]
fn unchanged_incomplete_carriers_are_composed_again_instead_of_warming_the_cache() {
    let adapter = ContentMapperProjectionAdapter::new();
    let handle = open(&adapter, "project");
    for _ in 0..2 {
        let result = adapter
            .transform(
                &handle,
                CARRIER,
                "<script setup lang=\"ts\">const value = </script>",
            )
            .unwrap();
        assert!(!result.complete);
    }
    assert_eq!(adapter.composition_count(), 2);
    assert_eq!(adapter.retained_projections(), 0);
}

fn module_of(result: &MapperTransformResult) -> &CheckingModule {
    result.module.as_deref().unwrap_or_else(|| {
        panic!(
            "the carrier composes; diagnostics: {:?}",
            result.diagnostics
        )
    })
}

fn virtual_slice<'t>(result: &'t MapperTransformResult, mapping: &WireSpanMapping) -> &'t str {
    &result.text
        [mapping.virtual_start as usize..(mapping.virtual_start + mapping.virtual_length) as usize]
}

fn original_slice<'s>(source: &'s str, mapping: &WireSpanMapping) -> &'s str {
    &source[mapping.original_start as usize
        ..(mapping.original_start + mapping.original_length) as usize]
}

#[test]
fn authored_script_bytes_and_template_reads_lower_to_verbatim_and_rewrites_to_atoms() {
    let result = transformed(COUNTER);
    let module = module_of(&result);

    let declaration = "const count = ref(1)";
    let verbatim = result
        .mappings
        .iter()
        .find(|mapping| {
            mapping.kind == WireSpanKind::Verbatim
                && virtual_slice(&result, mapping).contains(declaration)
        })
        .expect("the setup statement is projected verbatim");
    assert_eq!(verbatim.virtual_length, verbatim.original_length);
    assert_eq!(
        virtual_slice(&result, verbatim),
        original_slice(COUNTER, verbatim),
        "a verbatim mapping names the same bytes on both sides"
    );

    let authored = COUNTER.find("count }}").expect("interpolation") as u32;
    let template_read = result
        .mappings
        .iter()
        .find(|mapping| mapping.original_start == authored && mapping.virtual_length > 0)
        .expect("the template read of `count` is mapped");
    // The template read is an authored copy, so it maps byte for byte.
    assert_eq!(template_read.kind, WireSpanKind::Verbatim);
    assert_eq!(virtual_slice(&result, template_read), "count");
    assert_eq!(original_slice(COUNTER, template_read), "count");

    // Every rewritten region of the product, and only those, is an atom.
    let rewritten = module
        .emission
        .mapping()
        .projected()
        .iter()
        .filter(|region| region.class == ProjectedClass::Rewritten)
        .count();
    let atoms = result
        .mappings
        .iter()
        .filter(|mapping| mapping.kind == WireSpanKind::Atom && mapping.virtual_length > 0)
        .count();
    assert_eq!(atoms, rewritten);
}

#[test]
fn synthesized_text_has_no_mapping() {
    let result = transformed(KITCHEN_SINK);
    let module = module_of(&result);
    let synthesized: Vec<MappingSpan> = module
        .emission
        .mapping()
        .projected()
        .iter()
        .filter(|region| region.class == ProjectedClass::Synthesized)
        .map(|region| region.generated)
        .collect();
    assert!(!synthesized.is_empty(), "the module has scaffolding");
    for mapping in &result.mappings {
        let start = mapping.virtual_start;
        let end = start + mapping.virtual_length;
        for region in &synthesized {
            assert!(
                end <= region.start || region.end <= start,
                "mapping {mapping:?} covers synthesized bytes {region:?}"
            );
        }
    }
}

#[test]
fn mappings_are_sorted_and_do_not_overlap() {
    let result = transformed(KITCHEN_SINK);
    module_of(&result);
    for pair in result.mappings.windows(2) {
        let (first, second) = (&pair[0], &pair[1]);
        assert!(
            first.virtual_start + first.virtual_length <= second.virtual_start,
            "{first:?} overlaps or follows {second:?}"
        );
    }
}

#[test]
fn the_end_of_the_module_is_answered_from_an_anchor_not_clamped() {
    let result = transformed(COUNTER);
    let plane = result
        .plane
        .as_ref()
        .expect("a composed module has a plane");
    let end = result.text.len() as u32;
    let last = result.mappings.last().expect("mappings");
    assert_eq!(
        (
            last.virtual_start,
            last.virtual_length,
            last.original_start,
            last.original_length
        ),
        (end, 0, COUNTER.len() as u32, 0),
        "the end of the module stands at the end of the carrier"
    );
    // The plane itself declares no such anchor: the end is refused there,
    // so the wire answer is the declared anchor, not a clamped neighbour.
    assert_eq!(
        plane.to_carrier(MappingSpan { start: end, end }),
        CarrierAnswer::Refused(Refusal::OutOfRange)
    );
}

#[test]
fn every_mapping_round_trips_through_the_content_mapper() {
    let result = transformed(KITCHEN_SINK);
    let plane = result
        .plane
        .as_ref()
        .expect("a composed module has a plane");
    let mut checked = 0;
    for mapping in result
        .mappings
        .iter()
        .filter(|mapping| mapping.virtual_length > 0)
    {
        let projected = MappingSpan {
            start: mapping.virtual_start,
            end: mapping.virtual_start + mapping.virtual_length,
        };
        let expected = if mapping.original_length == 0 {
            CarrierAnswer::Point(mapping.original_start)
        } else {
            CarrierAnswer::Span(MappingSpan {
                start: mapping.original_start,
                end: mapping.original_start + mapping.original_length,
            })
        };
        assert_eq!(plane.to_carrier(projected), expected, "{mapping:?}");
        checked += 1;
    }
    assert!(checked > 5, "the kitchen sink maps script and template");
}

#[test]
fn a_refused_carrier_is_an_empty_module_with_its_reason() {
    let conflicting = concat!(
        "<script lang=\"ts\">export const a = 1</script>\n",
        "<script setup lang=\"tsx\">const b = 2</script>\n",
    );
    let result = transformed(conflicting);
    assert!(result.module.is_none());
    assert!(result.plane.is_none());
    assert!(!result.complete);
    assert_eq!(&*result.text, "export {};");
    assert!(result.mappings.is_empty());
    assert_eq!(result.virtual_file.as_ref(), "/ws/src/Comp.vue.ts");
    // The carrier parser's own error is the reason. The composer refuses the
    // same conflict, which is not a second diagnostic.
    let [parsed] = result.diagnostics.as_slice() else {
        panic!("one diagnostic: {:?}", result.diagnostics);
    };
    assert_eq!(parsed.code, MapperDiagnosticCode::ScriptLangConflict);
    // The wire still carries the parser's error as a syntax error.
    assert_eq!(
        parsed.code.wire_code(),
        MapperDiagnosticCode::Syntax.wire_code()
    );
    // The composer's refusal alone keeps the conflict's one code.
    let refused = refusal_diagnostic(
        &CheckingModuleRefusal::Script(SetupProjectionRefusal::ScriptLangConflict),
        conflicting.len() as u32,
    );
    assert_eq!(refused.code, MapperDiagnosticCode::ScriptLangConflict);
    assert_eq!((refused.start, refused.end), (0, conflicting.len() as u32));
}

#[test]
fn a_carrier_of_another_framework_is_not_composed() {
    let adapter = ContentMapperProjectionAdapter::new();
    let handle = open(&adapter, "project");
    let svelte = "/ws/src/Widget.svelte";
    assert!(adapter.owns(CARRIER));
    assert!(!adapter.owns(svelte));
    assert!(!adapter.owns("/ws/src/main.ts"));

    let source = "<script lang=\"ts\">let count: number = 1</script>\n<p>{count}</p>\n";
    let result = adapter
        .transform(&handle, svelte, source)
        .expect("the project is open");
    assert_eq!(
        adapter.composition_count(),
        0,
        "a foreign carrier is never parsed"
    );
    assert!(result.module.is_none());
    assert!(!result.complete);
    assert_eq!(&*result.text, "export {};");
    let [refused] = result.diagnostics.as_slice() else {
        panic!("one refusal: {:?}", result.diagnostics);
    };
    assert_eq!(refused.code, MapperDiagnosticCode::ProjectionRefused);
    assert_eq!((refused.start, refused.end), (0, source.len() as u32));

    // The adapter's own carriers still compose.
    module_of(&adapter.transform(&handle, CARRIER, COUNTER).expect("open"));
    assert_eq!(adapter.composition_count(), 1);
}

#[test]
fn the_virtual_file_is_named_by_the_descriptor_column() {
    let result = transformed(COUNTER);
    assert_eq!(result.virtual_file.as_ref(), "/ws/src/Comp.vue.ts");
    let adapter = ContentMapperProjectionAdapter::new();
    assert_eq!(adapter.virtual_extension(result.dialect), Some(".ts"));
}

#[test]
fn projects_with_different_custom_elements_never_share_a_result() {
    let carrier = concat!(
        "<template>\n  <my-widget :value=\"count\" />\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "const count = 1\n",
        "</script>\n",
    );
    let adapter = ContentMapperProjectionAdapter::new();
    let plain = MapperProjectHandle(Arc::from("plain"));
    let custom = MapperProjectHandle(Arc::from("custom"));
    adapter
        .open_project(plain.clone(), settings(&[]))
        .expect("opens");
    adapter
        .open_project(custom.clone(), settings(&["my-"]))
        .expect("opens");

    let as_component = adapter.transform(&plain, CARRIER, carrier).expect("open");
    let as_element = adapter.transform(&custom, CARRIER, carrier).expect("open");
    assert_ne!(
        as_component.text, as_element.text,
        "a custom-element prefix changes how the tag is checked"
    );
    // Neither project's result leaks into the other's next transform.
    let again = adapter.transform(&plain, CARRIER, carrier).expect("open");
    assert_eq!(again.text, as_component.text);
    assert_eq!(
        adapter.open_project(plain.clone(), settings(&[])),
        Err(MapperError::ProjectAlreadyOpen(plain))
    );
}

#[test]
fn a_closed_project_is_unknown_and_retains_nothing() {
    let adapter = ContentMapperProjectionAdapter::new();
    let handle = open(&adapter, "project");
    assert_eq!(adapter.open_projects(), 1);
    adapter
        .close_project(&handle)
        .expect("an open project closes");
    assert_eq!(adapter.open_projects(), 0);
    assert_eq!(
        adapter.transform(&handle, CARRIER, COUNTER).err(),
        Some(MapperError::UnknownProject(handle.clone()))
    );
    assert_eq!(
        adapter.close_project(&handle),
        Err(MapperError::UnknownProject(handle))
    );
}

#[test]
fn a_transform_reads_only_the_content_it_is_given() {
    let adapter = ContentMapperProjectionAdapter::new();
    let handle = open(&adapter, "project");
    // No file exists at this path; the content alone is composed.
    let missing = "/no/such/directory/Comp.vue";
    let first = adapter.transform(&handle, missing, COUNTER).expect("open");
    module_of(&first);
    let edited = COUNTER.replace("ref(1)", "ref('one')");
    let second = adapter.transform(&handle, missing, &edited).expect("open");
    assert!(second.text.contains("ref('one')"));
    assert!(!first.text.contains("ref('one')"));
}

#[test]
fn a_changed_declared_option_changes_the_project_identity() {
    let with = |options: &[&str]| {
        MapperProjectSettings::new(
            Arc::from("/ws/tsconfig.json"),
            Vec::new(),
            options.iter().map(|option| option.to_string()).collect(),
            Vec::new(),
        )
    };
    let strict = with(&["strict=true", "noUnusedLocals=true"]);
    assert_eq!(
        strict.declared_option_dependencies,
        vec!["noUnusedLocals=true".to_string(), "strict=true".to_string()],
        "dependencies are sorted"
    );
    assert_eq!(
        strict.identity,
        with(&["noUnusedLocals=true", "strict=true"]).identity
    );
    assert_ne!(
        strict.identity,
        with(&["strict=false", "noUnusedLocals=true"]).identity
    );
    assert_ne!(strict.identity, settings(&[]).identity);
}

fn panicking_composer(
    grammars: &CarrierGrammarAuthority,
    canonical: &str,
    content: &str,
    custom_elements: &[String],
    declarations: bool,
) -> Composition {
    if content.contains("panic-here") {
        panic!("composition defect under test");
    }
    compose_carrier(grammars, canonical, content, custom_elements, declarations)
}

#[test]
fn a_panic_in_composition_is_an_internal_diagnostic_and_the_next_transform_works() {
    let adapter = ContentMapperProjectionAdapter::with_composer(panicking_composer);
    let handle = open(&adapter, "project");
    let failed = adapter
        .transform(&handle, CARRIER, "<template>panic-here</template>")
        .expect("a panic never fails the project");
    assert!(failed.module.is_none());
    let [diagnostic] = failed.diagnostics.as_slice() else {
        panic!("one diagnostic: {:?}", failed.diagnostics);
    };
    assert_eq!(diagnostic.code, MapperDiagnosticCode::ProjectionInternal);
    assert!(diagnostic.message.contains("composition defect under test"));

    let next = adapter.transform(&handle, CARRIER, COUNTER).expect("open");
    module_of(&next);
    assert_eq!(adapter.open_projects(), 1);
}

#[test]
fn the_carrier_parsers_own_errors_are_reported() {
    let result = transformed("<style>p { color: red }</style>\n");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == MapperDiagnosticCode::Syntax),
        "a carrier with no entry block is not reported clean: {:?}",
        result.diagnostics
    );
    assert!(!result.complete);
}

#[test]
fn an_external_script_is_refused_at_its_src_value_not_dropped() {
    let source = "<script lang=\"ts\" src=\"./logic.ts\"></script>\n";
    let result = transformed(source);
    assert!(result.module.is_none());
    let start = source.find("./logic.ts").expect("specifier") as u32;
    let refused: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == MapperDiagnosticCode::ProjectionRefused)
        .collect();
    let [diagnostic] = refused.as_slice() else {
        panic!("one refusal: {:?}", result.diagnostics);
    };
    assert!(
        diagnostic.start <= start && start + "./logic.ts".len() as u32 <= diagnostic.end,
        "the refusal covers the `src` value: {diagnostic:?}"
    );
    assert!(diagnostic.end - diagnostic.start <= "\"./logic.ts\"".len() as u32);
}

#[test]
fn the_wire_result_carries_the_tuples_and_numeric_codes() {
    let mapping = WireSpanMapping {
        virtual_start: 3,
        virtual_length: 5,
        original_start: 7,
        original_length: 5,
        kind: WireSpanKind::Verbatim,
        features: wire_features::HOVER,
    };
    assert_eq!(mapping.to_wire(), [3, 5, 7, 5, 0, 1]);
    let result = transformed("<script setup lang=\"ts\">const a = </script>\n");
    let wire = result.to_wire(".ts");
    assert_eq!(wire.extension, ".ts");
    assert_eq!(wire.diagnostics.len(), result.diagnostics.len());
    for (wire, diagnostic) in wire.diagnostics.iter().zip(&result.diagnostics) {
        assert_eq!(wire.code, diagnostic.code.wire_code());
        assert_eq!(wire.start + wire.length, diagnostic.end);
    }
}
