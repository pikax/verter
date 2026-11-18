//! Independent revision channels and complete-result cache retention.

use std::sync::Arc;

use verter_compiler::framework_common::vue_projection_backend::CheckingDialect;

use crate::sfc_projection_cache::*;
use crate::sfc_projection_mapper::*;

fn project(name: &str) -> MapperProjectHandle {
    MapperProjectHandle(Arc::from(name))
}

fn read_set(carrier: &str) -> ProjectionReadSet {
    ProjectionReadSet {
        carrier: Arc::from(carrier),
        external_sources: Vec::new(),
        settings_identity: [1; 32],
    }
}

fn result(text: &str) -> Arc<MapperTransformResult> {
    Arc::new(MapperTransformResult {
        virtual_file: Arc::from("/ws/App.vue.ts"),
        dialect: CheckingDialect::Ts,
        text: Arc::from(text),
        mappings: Vec::new(),
        diagnostics: Vec::new(),
        module: None,
        plane: None,
        complete: true,
    })
}

fn transform(source: &str) -> MapperTransformResult {
    let adapter = ContentMapperProjectionAdapter::new();
    let handle = project("project");
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
    adapter.transform(&handle, "/ws/App.vue", source).unwrap()
}

#[test]
fn a_comment_moves_correspondence_without_changing_checking_text_and_undo_restores_revisions() {
    let source = "<script setup lang=\"ts\">import { ref } from 'vue'; const value = ref(1);</script><template>{{ value }}</template>";
    let edited = source.replace("<template>", "<!-- comment --><template>");
    let reads = read_set("/ws/App.vue");
    let original = transform(source);
    let changed = transform(&edited);
    let before = revisions_of(source, &original, None, &reads);
    let after = revisions_of(&edited, &changed, None, &reads);
    assert_ne!(before.authored_text, after.authored_text);
    assert_eq!(before.checking_text, after.checking_text);
    assert_ne!(before.correspondence, after.correspondence);
    let authored = edited.find("value }}").unwrap() as u32;
    let token = changed
        .mappings
        .iter()
        .find(|mapping| mapping.original_start == authored && mapping.virtual_length > 0)
        .expect("the moved template read has authored correspondence");
    assert_eq!(
        token.original_length,
        "value".len() as u32,
        "a template read is an authored copy"
    );
    assert_eq!(
        &changed.text
            [token.virtual_start as usize..(token.virtual_start + token.virtual_length) as usize],
        "value"
    );
    assert_eq!(
        changed
            .plane
            .as_ref()
            .unwrap()
            .to_carrier(verter_compiler::code_transform::MappingSpan {
                start: token.virtual_start,
                end: token.virtual_start + token.virtual_length,
            }),
        crate::content_mapper::CarrierAnswer::Span(verter_compiler::code_transform::MappingSpan {
            start: authored,
            end: authored + "value".len() as u32,
        })
    );
    assert_eq!(
        before,
        revisions_of(source, &transform(source), None, &reads)
    );
}

#[test]
fn diagnostic_observations_change_correspondence_even_when_wire_mappings_are_unchanged() {
    use oxc_allocator::Allocator;
    use verter_compiler::code_transform::{CodeTransform, MappingSpan};
    use verter_compiler::framework_common::projection_plan::origin::{
        ObservationRole, ProjectionEmission, ProjectionOrigin, RoleQualifiedObservation,
    };

    let source = "<script setup lang=\"ts\">const value = 1;</script>";
    let base = transform(source);
    let allocator = Allocator::default();
    let operation = CodeTransform::new(&base.text, &allocator);
    let with_observations = |observations| {
        let mut result = base.clone();
        let mut module = (**result.module.as_ref().unwrap()).clone();
        module.emission = ProjectionEmission::from_operation(&operation, observations).unwrap();
        result.mappings = lower_wire_mappings(&module);
        result.module = Some(Arc::new(module));
        result
    };
    let first = with_observations(Vec::new());
    let second = with_observations(vec![RoleQualifiedObservation::new(
        MappingSpan { start: 0, end: 1 },
        ObservationRole::Diagnostic,
        ProjectionOrigin::empty(),
    )]);
    assert_eq!(first.text, second.text);
    assert_eq!(first.mappings, second.mappings);
    let reads = read_set("/ws/App.vue");
    let before = revisions_of(source, &first, None, &reads);
    let after = revisions_of(source, &second, None, &reads);
    assert_eq!(before.checking_text, after.checking_text);
    assert_ne!(before.correspondence, after.correspondence);
}

#[test]
fn public_source_and_dependency_changes_affect_only_their_channels() {
    let text = result("export {};");
    let reads = read_set("/ws/App.vue");
    let original = revisions_of(
        "source",
        &text,
        Some("declare const props: { value: string };"),
        &reads,
    );
    let changed = revisions_of(
        "source",
        &text,
        Some("declare const props: { value: number };"),
        &reads,
    );
    assert_ne!(original.public_signature, changed.public_signature);
    assert_eq!(original.authored_text, changed.authored_text);
    assert_eq!(original.checking_text, changed.checking_text);
    assert_eq!(original.correspondence, changed.correspondence);
    assert_eq!(original.dependencies, changed.dependencies);
    assert_ne!(
        revisions_of("source", &text, None, &reads).public_signature,
        revisions_of("source", &text, Some(""), &reads).public_signature
    );
    let mut external = reads.clone();
    external.external_sources = vec![
        Arc::from("C:\\ws\\logic.ts"),
        Arc::from("/ws/template.html"),
    ];
    let dependencies = revisions_of("source", &text, None, &external).dependencies;
    assert_ne!(
        dependencies,
        revisions_of("source", &text, None, &reads).dependencies
    );
    external.external_sources.reverse();
    external.external_sources.push(Arc::from("c:/ws/logic.ts"));
    assert_eq!(
        dependencies,
        revisions_of("source", &text, None, &external).dependencies
    );
}

fn admit(
    cache: &ProjectionFragmentCache,
    handle: &MapperProjectHandle,
    reads: ProjectionReadSet,
    source: &str,
) {
    let text = result(source);
    let revisions = revisions_of(source, &text, None, &reads);
    assert!(cache.admit(handle, reads, revisions, text));
}

#[test]
fn a_thousand_edits_keep_each_project_within_capacity_and_hits_refresh_recency() {
    let cache = ProjectionFragmentCache::new(3);
    let handles = [project("first"), project("second")];
    let reads = read_set("/ws/App.vue");
    for edit in 0..1000 {
        for handle in &handles {
            admit(&cache, handle, reads.clone(), &edit.to_string());
        }
        assert_eq!(cache.retained(), 2 * (edit + 1).min(3));
    }
    assert!(cache
        .get(
            &handles[0],
            &reads.carrier,
            authored_revision("996"),
            reads.settings_identity
        )
        .is_none());
    assert!(cache
        .get(
            &handles[0],
            &reads.carrier,
            authored_revision("997"),
            reads.settings_identity
        )
        .is_some());
    admit(&cache, &handles[0], reads.clone(), "1000");
    assert!(cache
        .get(
            &handles[0],
            &reads.carrier,
            authored_revision("998"),
            reads.settings_identity
        )
        .is_none());
    assert!(cache
        .get(
            &handles[0],
            &reads.carrier,
            authored_revision("997"),
            reads.settings_identity
        )
        .is_some());
    cache.close_project(&handles[0]);
    assert_eq!(cache.retained(), 3);
    assert!(cache
        .get(
            &handles[1],
            &reads.carrier,
            authored_revision("998"),
            reads.settings_identity
        )
        .is_some());
}

#[test]
fn incomplete_results_and_zero_capacity_never_retain_entries() {
    let handle = project("project");
    let reads = read_set("/ws/App.vue");
    let mut incomplete = (*result("text")).clone();
    incomplete.complete = false;
    let revisions = revisions_of("source", &incomplete, None, &reads);
    let cache = ProjectionFragmentCache::new(2);
    assert!(!cache.admit(&handle, reads.clone(), revisions, Arc::new(incomplete)));
    assert_eq!(cache.retained(), 0);
    let disabled = ProjectionFragmentCache::new(0);
    assert!(!disabled.admit(&handle, reads, revisions, result("text")));
    assert_eq!(disabled.retained(), 0);
}

#[test]
fn changed_inputs_miss_and_invalidation_removes_only_reading_entries() {
    let cache = ProjectionFragmentCache::new(8);
    let handle = project("project");
    let mut first = read_set("C:\\ws\\App.vue");
    first.external_sources.push(Arc::from("C:\\ws\\logic.ts"));
    let mut second = read_set("c:/ws/Other.vue");
    second.external_sources.push(Arc::from("c:/ws/logic.ts"));
    let third = read_set("c:/ws/Unrelated.vue");
    for reads in [&first, &second, &third] {
        admit(&cache, &handle, reads.clone(), "source");
    }
    assert!(cache
        .get(
            &handle,
            "c:/ws/App.vue",
            authored_revision("source"),
            first.settings_identity
        )
        .is_some());
    assert!(cache
        .get(
            &handle,
            "c:/ws/App.vue",
            authored_revision("edited"),
            first.settings_identity
        )
        .is_none());
    assert!(cache
        .get(
            &handle,
            "c:/ws/App.vue",
            authored_revision("source"),
            [2; 32]
        )
        .is_none());
    assert!(cache
        .get(
            &project("other"),
            "c:/ws/App.vue",
            authored_revision("source"),
            first.settings_identity
        )
        .is_none());
    assert_eq!(cache.invalidate_path("c:/ws/missing.ts"), 0);
    assert_eq!(cache.invalidate_path("c:/ws/logic.ts"), 2);
    assert_eq!(cache.retained(), 1);
    assert!(cache
        .get(
            &handle,
            &third.carrier,
            authored_revision("source"),
            third.settings_identity
        )
        .is_some());
    assert_eq!(cache.invalidate_path("C:\\ws\\Unrelated.vue"), 1);
    assert_eq!(cache.retained(), 0);
}

#[test]
fn cached_results_match_fresh_compositions_for_script_fixtures() {
    let cache = ProjectionFragmentCache::new(4);
    let handle = project("project");
    let reads = read_set("/ws/App.vue");
    for source in [
        "<script setup lang=\"ts\">const value = 1;</script>",
        "<script setup lang=\"ts\">defineProps<{ value: string }>();</script>",
        "<script lang=\"ts\">export default { name: 'Example' };</script>",
    ] {
        let fresh = Arc::new(transform(source));
        assert!(
            fresh.complete,
            "script-only checking is complete: {:?}",
            fresh.diagnostics
        );
        let revisions = revisions_of(source, &fresh, None, &reads);
        assert!(cache.admit(&handle, reads.clone(), revisions, Arc::clone(&fresh)));
        let cached = cache
            .get(
                &handle,
                &reads.carrier,
                revisions.authored_text,
                reads.settings_identity,
            )
            .unwrap();
        assert!(Arc::ptr_eq(&fresh, &cached));
        let recomposed = transform(source);
        assert_eq!(cached.text, recomposed.text);
        assert_eq!(cached.mappings, recomposed.mappings);
        assert_eq!(cached.diagnostics, recomposed.diagnostics);
        assert_eq!(cached.virtual_file, recomposed.virtual_file);
        assert_eq!(cached.dialect, recomposed.dialect);
        assert_eq!(cached.complete, recomposed.complete);
        assert_eq!(revisions, revisions_of(source, &recomposed, None, &reads));
    }
}
