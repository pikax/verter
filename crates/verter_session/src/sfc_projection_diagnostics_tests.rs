//! Engine diagnostics mapped through real carrier projection results.

use std::collections::HashMap;
use std::sync::Arc;
use verter_compiler::framework_common::vue_projection_backend::{ObligationFamily, UseHost};

use crate::sfc_projection_diagnostics::*;
use crate::sfc_projection_mapper::{
    ContentMapperProjectionAdapter, MapperProjectHandle, MapperProjectSettings,
    MapperTransformResult,
};

#[derive(Default)]
struct Lookup(HashMap<Arc<str>, Arc<MapperTransformResult>>);

impl TransformLookup for Lookup {
    fn result_for_virtual(&self, file: &str) -> Option<Arc<MapperTransformResult>> {
        self.0.get(file).cloned()
    }
}

impl Lookup {
    fn transform(&mut self, carrier: &str, source: &str) -> Arc<MapperTransformResult> {
        let adapter = ContentMapperProjectionAdapter::new();
        let handle = MapperProjectHandle(Arc::from("project"));
        adapter
            .open_project(
                handle.clone(),
                MapperProjectSettings::new(
                    Arc::from("/ws/tsconfig.json"),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
            )
            .unwrap();
        let result = Arc::new(adapter.transform(&handle, carrier, source).unwrap());
        assert!(result.module.is_some(), "{:?}", result.diagnostics);
        self.0.insert(result.virtual_file.clone(), result.clone());
        result
    }
}

fn diagnostic(result: &MapperTransformResult, text: &str, message: String) -> EngineDiagnostic {
    EngineDiagnostic {
        file: result.virtual_file.clone(),
        start: result.text.find(text).unwrap() as u32,
        length: text.len() as u32,
        code: 2345,
        category: DiagnosticCategory::Error,
        message,
        related: Vec::new(),
    }
}

#[test]
fn explicit_authored_provenance_preserves_companion_like_paths_and_related_anchors() {
    struct Authored;
    impl TransformLookup for Authored {
        fn result_for_virtual(&self, _: &str) -> Option<Arc<MapperTransformResult>> {
            None
        }
        fn is_authored(&self, file: &str) -> bool {
            matches!(file, "/ws/my components/Foo.vue.ts" | "/ws/Other.svelte.ts")
        }
    }
    let raw = EngineDiagnostic {
        file: Arc::from("/ws/my components/Foo.vue.ts"),
        start: 4,
        length: 3,
        code: 2322,
        category: DiagnosticCategory::Error,
        message: "See '/ws/my components/Foo.vue.ts' and /ws/Other.svelte.ts.".into(),
        related: vec![EngineRelated {
            file: Arc::from("/ws/Other.svelte.ts"),
            start: 2,
            length: 1,
            message: "Declared in '/ws/Other.svelte.ts'".into(),
        }],
    };
    let attributed = attribute_diagnostics(&[raw.clone(), raw.clone()], &Authored);
    assert_eq!(attributed.len(), 1);
    let d = &attributed[0];
    assert_eq!(d.carrier, raw.file);
    assert_eq!(d.anchor, DiagnosticAnchor::Authored { start: 4, end: 7 });
    assert_eq!(d.message, raw.message);
    assert_eq!(d.related[0].file, raw.related[0].file);
    assert_eq!(
        d.related[0].anchor,
        DiagnosticAnchor::Authored { start: 2, end: 3 }
    );
    assert_eq!(d.related[0].message, raw.related[0].message);
    assert_eq!(d.dedup.carrier, raw.file);
}

#[test]
fn missing_required_prop_maps_to_the_component_tag_and_its_obligation() {
    let source = "<script setup lang=\"ts\">import Child from './Child.vue';</script><template><Child /></template>";
    let mut lookup = Lookup::default();
    let result = lookup.transform("/ws/Parent.vue", source);
    let mut raw = diagnostic(
        &result,
        "Child, __VerterUseConstructor",
        "Property 'name' is missing".into(),
    );
    raw.length = "Child".len() as u32;
    let attributed = attribute_diagnostics(&[raw], &lookup);
    assert_eq!(attributed.len(), 1);
    let error = &attributed[0];
    let tag = source.find("Child />").unwrap() as u32;
    assert_eq!(error.carrier.as_ref(), "/ws/Parent.vue");
    assert_eq!(
        error.anchor,
        DiagnosticAnchor::Authored {
            start: tag,
            end: tag
        }
    );
    assert_eq!(error.family, Some(ObligationFamily::ComponentUse));
    assert!(matches!(error.host, Some(UseHost::Component(_))));
    assert!(error.message.contains("'name'"));
}

#[test]
fn related_information_uses_its_own_carrier_and_plain_scripts_pass_through() {
    let mut lookup = Lookup::default();
    let parent = lookup.transform(
        "/ws/Parent.vue",
        "<script setup lang=\"ts\">const value = 1;</script>",
    );
    let child_source = "<script setup lang=\"ts\">const label: string = 1;</script>";
    let child = lookup.transform("/ws/Child.vue", child_source);
    let mut error = diagnostic(
        &parent,
        "const value",
        format!("See '{}' and '/ws/plain.ts'", child.virtual_file),
    );
    error.related.push(EngineRelated {
        file: child.virtual_file.clone(),
        start: child.text.find("label: string").unwrap() as u32,
        length: 5,
        message: format!("Declared in '{}'", parent.virtual_file),
    });
    let plain = EngineDiagnostic {
        file: Arc::from("/ws/plain.ts"),
        start: 7,
        length: 3,
        code: 2322,
        category: DiagnosticCategory::Warning,
        message: "plain failure".into(),
        related: Vec::new(),
    };
    let raw = vec![error, plain.clone()];
    let original = raw.clone();
    let attributed = attribute_diagnostics(&raw, &lookup);
    assert_eq!(raw, original, "attribution preserves the raw engine list");
    assert_eq!(attributed.len(), raw.len());
    assert_eq!(
        attributed[0].message,
        "See '/ws/Child.vue' and '/ws/plain.ts'"
    );
    let related = &attributed[0].related[0];
    let label = child_source.find("label").unwrap() as u32;
    assert_eq!(related.file.as_ref(), "/ws/Child.vue");
    assert_eq!(
        related.anchor,
        DiagnosticAnchor::Authored {
            start: label,
            end: label + 5
        }
    );
    assert_eq!(related.message, "Declared in '/ws/Parent.vue'");
    assert_eq!(attributed[1].carrier, plain.file);
    assert_eq!(
        attributed[1].anchor,
        DiagnosticAnchor::Authored { start: 7, end: 10 }
    );
    assert_eq!(attributed[1].message, plain.message);
    assert_eq!(attributed[1].category, plain.category);
    assert_eq!(attributed[1].family, None);
}

#[test]
fn template_reads_map_to_their_authored_text_and_scaffolding_alone_stays_generated() {
    let source = "<script setup lang=\"ts\">const value = 1; const other = 2;</script><template>{{ value + other }}</template>";
    let mut lookup = Lookup::default();
    let result = lookup.transform("/ws/Parent.vue", source);
    // `__VerterCtx.value` is the context read of the authored `value`.
    let qualified = diagnostic(
        &result,
        "__VerterCtx.value",
        "Operand of the authored read".into(),
    );
    // The whole binary expression, with the second read's qualifier inside.
    let mut whole = qualified.clone();
    whole.length = "__VerterCtx.value + __VerterCtx.other".len() as u32;
    whole.message = "Operator of the authored expression".into();
    let mut generated = qualified.clone();
    generated.length = "__VerterCtx.".len() as u32;
    generated.message = "Cannot find generated context".into();
    let raw = vec![qualified, whole, generated];
    let errors = attribute_diagnostics(&raw, &lookup);
    assert_eq!(errors.len(), 3);
    assert!(errors
        .iter()
        .all(|error| error.carrier.as_ref() == "/ws/Parent.vue"));
    let value = source.find("value + other").unwrap() as u32;
    assert_eq!(
        errors[0].anchor,
        DiagnosticAnchor::Authored {
            start: value,
            end: value + "value".len() as u32
        }
    );
    assert_eq!(
        errors[1].anchor,
        DiagnosticAnchor::Authored {
            start: value,
            end: value + "value + other".len() as u32
        }
    );
    assert_eq!(
        errors[2].anchor,
        DiagnosticAnchor::Generated {
            snippet: "__VerterCtx.".to_string(),
        }
    );
    assert_eq!(errors[2].message, raw[2].message);
}

#[test]
fn generated_failures_from_different_virtual_files_do_not_collapse() {
    let first = EngineDiagnostic {
        file: Arc::from("/ws/Foo.vue.ts"),
        start: 0,
        length: 1,
        code: 2304,
        category: DiagnosticCategory::Error,
        message: "Cannot find generated binding".into(),
        related: Vec::new(),
    };
    let mut declaration = first.clone();
    declaration.file = Arc::from("/ws/Foo.d.vue.ts");
    let errors = attribute_diagnostics(&[first.clone(), declaration, first], &Lookup::default());
    assert_eq!(
        errors.len(),
        2,
        "each virtual file's generated failure survives, with only its own duplicate removed"
    );
    assert!(errors
        .iter()
        .all(|error| error.carrier.as_ref() == "/ws/Foo.vue"));
    assert!(errors.iter().all(|error| error.anchor
        == DiagnosticAnchor::Generated {
            snippet: String::new()
        }));
}

#[test]
fn replay_duplicates_collapse_but_different_errors_at_the_same_anchor_survive() {
    let source =
        "<script setup lang=\"ts\">const value = 1;</script><template>{{ value }}</template>";
    let mut lookup = Lookup::default();
    let result = lookup.transform("/ws/Parent.vue", source);
    let first = diagnostic(&result, "const value", "First error".into());
    let mut other = first.clone();
    other.message = "Second error".into();
    let mut other_code = first.clone();
    other_code.code += 1;
    let mut warning = first.clone();
    warning.category = DiagnosticCategory::Warning;
    let raw = vec![first.clone(), other, first.clone(), other_code, warning];
    let unchanged = raw.clone();
    let errors = attribute_diagnostics(&raw, &lookup);
    assert_eq!(raw, unchanged);
    assert_eq!(errors.len(), 4);
    assert_eq!(errors[0].message, "First error");
    assert_eq!(errors[1].message, "Second error");
    assert_ne!(
        errors[0].dedup.message_digest,
        errors[1].dedup.message_digest
    );
    assert_ne!(errors[0].code, errors[2].code);
    assert_eq!(errors[3].category, DiagnosticCategory::Warning);
}

#[test]
fn an_expression_and_its_guard_replay_share_one_authored_anchor() {
    let source = "<script setup lang=\"ts\">import Child from './Child.vue'; const value = 1; const handler = () => {};</script><template><Child v-if=\"value\" @click=\"handler()\" /></template>";
    let mut lookup = Lookup::default();
    let result = lookup.transform("/ws/Parent.vue", source);
    let occurrences: Vec<_> = result.text.match_indices("__VerterCtx.value").collect();
    assert!(
        occurrences.len() >= 2,
        "a handler replays its dominating condition: {}",
        result.text
    );
    let authored = source.find("v-if=\"value").unwrap() as u32 + "v-if=\"".len() as u32;
    // The engine reports the read's name in every copy. Each copy maps the
    // name to the same authored bytes, so the repeats are one diagnostic.
    let raw: Vec<_> = occurrences
        .iter()
        .map(|(at, _)| EngineDiagnostic {
            file: result.virtual_file.clone(),
            start: (at + "__VerterCtx.".len()) as u32,
            length: "value".len() as u32,
            code: 2339,
            category: DiagnosticCategory::Error,
            message: "Property 'value' does not exist".into(),
            related: Vec::new(),
        })
        .collect();
    let errors = attribute_diagnostics(&raw, &lookup);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(
        errors[0].anchor,
        DiagnosticAnchor::Authored {
            start: authored,
            end: authored + "value".len() as u32
        }
    );
}

#[test]
fn messages_rewrite_only_descriptor_classified_paths_including_quoted_spaces() {
    let plain = EngineDiagnostic {
        file: Arc::from("/ws/plain.ts"), start: 0, length: 1, code: 2322,
        category: DiagnosticCategory::Error,
        message: "See '/ws/my components/Child.vue.ts' and import(\"/ws/Other.vue.tsx\"); /ws/App.vue.js /ws/plain.ts".into(),
        related: Vec::new(),
    };
    let errors = attribute_diagnostics(&[plain], &Lookup::default());
    assert_eq!(
        errors[0].message,
        "See '/ws/my components/Child.vue' and import(\"/ws/Other.vue\"); /ws/App.vue /ws/plain.ts"
    );
}

#[test]
fn listener_payload_failure_keeps_its_family_and_maps_related_declared_payload() {
    let mut lookup = Lookup::default();
    let parent = lookup.transform("/ws/Parent.vue", "<script setup lang=\"ts\">import Child from './Child.vue'; const handlers = {};</script><template><Child v-on=\"handlers\" /></template>");
    let child_source =
        "<script setup lang=\"ts\">const payload: number = 1; defineEmits<{ save: [value: number] }>();</script>";
    let child = lookup.transform("/ws/Child.vue", child_source);
    let start = parent.text.rfind("__VerterListeners(").unwrap() as u32;
    let raw = EngineDiagnostic {
        file: parent.virtual_file.clone(),
        start,
        length: "__VerterListeners".len() as u32,
        code: 2345,
        category: DiagnosticCategory::Error,
        message: "Invalid save listener".into(),
        related: vec![EngineRelated {
            file: child.virtual_file.clone(),
            start: child.text.find("const payload").unwrap() as u32 + 6,
            length: "payload".len() as u32,
            message: "Declared payload".into(),
        }],
    };
    let errors = attribute_diagnostics(&[raw], &lookup);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].family, Some(ObligationFamily::Listener));
    assert!(matches!(errors[0].host, Some(UseHost::Component(_))));
    assert!(matches!(
        errors[0].anchor,
        DiagnosticAnchor::Generated { .. }
    ));
    let payload = child_source.find("payload").unwrap() as u32;
    assert_eq!(errors[0].related[0].file.as_ref(), "/ws/Child.vue");
    assert_eq!(
        errors[0].related[0].anchor,
        DiagnosticAnchor::Authored {
            start: payload,
            end: payload + 7
        }
    );
}

#[test]
fn a_whole_expression_maps_in_a_setup_block_running_to_the_end() {
    // No closing tag: the template scope is inserted, each authored piece
    // standing at its authored point.
    let source = "<template>{{ value + other }}</template><script setup lang=\"ts\">const value = true; const other = 1;";
    let mut lookup = Lookup::default();
    let result = lookup.transform("/ws/Parent.vue", source);
    let whole = diagnostic(
        &result,
        "__VerterCtx.value + __VerterCtx.other",
        "Operator of the authored expression".into(),
    );
    let errors = attribute_diagnostics(&[whole], &lookup);
    assert_eq!(errors.len(), 1);
    let value = source.find("value + other").unwrap() as u32;
    let other = source.find("other }}").unwrap() as u32;
    match errors[0].anchor {
        DiagnosticAnchor::Authored { start, end } => {
            assert_eq!(start, value, "{:?}", errors[0]);
            assert!(end >= other, "{:?}", errors[0]);
        }
        DiagnosticAnchor::Generated { .. } => panic!("positionless: {:?}", errors[0]),
    }
}
