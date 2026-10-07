use super::*;
use crate::resolver_core::ResolverStore;
use crate::types::HostConfig;
use crate::VerterHost;
use std::collections::BTreeSet;
use std::sync::Arc;
use verter_session_query::analysis::type_expand::ExpandedComponentTypes;
use verter_session_query::facts::store_view::StoreView;
use verter_type_expr::{LiteralValue, ObjectMember, PrimitiveName, TypeExpr, UnknownValue};

fn published_type(
    row: &crate::meta_resolve::MaterializedTypePublication,
) -> &verter_type_expr::TypeExpr {
    row.materialized_type()
        .expect("publication must carry a materialized type")
}

fn materialized_event_types(
    lanes: &crate::meta_resolve::MaterializedComponentMetaTypeLanes,
) -> Vec<TypeExpr> {
    lanes
        .events
        .iter()
        .map(|occurrence| published_type(&occurrence.payload).clone())
        .collect()
}

fn event_occurrence_publications(
    lanes: &crate::meta_resolve::MaterializedComponentMetaTypeLanes,
) -> Vec<crate::meta_resolve::MaterializedTypePublication> {
    lanes
        .events
        .iter()
        .map(|occurrence| occurrence.payload.clone())
        .collect()
}

fn replace_event_payload(
    event: &mut verter_session_query::analysis::component_meta::EventAnalysis,
    payload: verter_type_expr::facts::SourcePosition,
) {
    event.payload = payload;
    event.publication = verter_type_expr::TypePublication::from_source_position(
        &event.payload,
        verter_type_expr::ResolutionExactness::ExactConcrete,
        verter_type_expr::ResolutionProvenance::FrameworkSurface,
        Arc::from([]),
        None,
        &verter_type_expr::PublicationPolicy::exact_only(),
    );
}

fn prop_terminal_display(project: &MetaProject, owner: &str, prop_name: &str) -> Option<String> {
    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output(owner)
        .expect("component-meta output should materialize")
        .expect("component should resolve")
        .into_parts();
    let index = analysis
        .props
        .iter()
        .position(|prop| prop.name == prop_name)
        .unwrap_or_else(|| panic!("missing prop {prop_name}"));
    types.into_lanes().props[index]
        .terminal_display()
        .text()
        .map(str::to_string)
}

fn slot_binding_terminal_display(
    project: &MetaProject,
    owner: &str,
    slot_name: &str,
    binding_name: &str,
) -> Option<String> {
    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output(owner)
        .expect("component-meta output should materialize")
        .expect("component should resolve")
        .into_parts();
    let slot_index = analysis
        .slots
        .iter()
        .position(|slot| slot.name == slot_name)
        .unwrap_or_else(|| panic!("missing slot {slot_name}"));
    let binding_index = analysis.slots[slot_index]
        .bindings
        .iter()
        .position(|binding| binding.name == binding_name)
        .unwrap_or_else(|| panic!("missing slot binding {slot_name}.{binding_name}"));
    types.into_lanes().slot_bindings[slot_index][binding_index]
        .terminal_display()
        .text()
        .map(str::to_string)
}

fn make_project() -> Arc<MetaProject> {
    make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    })
}

/// Test hosts construct schedulers with `cpu_threads = 1` to avoid
/// CPU oversubscription when many parallel test threads each spin
/// up their own Rayon pools. This is the architecture that replaced
/// the prior `HEAVY_COMPONENT_META_TEST_MUTEX` serialisation strategy.
fn test_scheduler_config() -> verter_scheduler::scheduler::SchedulerConfig {
    verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    }
}

fn make_project_with_config(config: HostConfig) -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..config
        },
        test_scheduler_config(),
    );
    MetaProject::new(host)
}

fn make_workspace_project(ws: Arc<verter_workspace::MemoryWorkspace>) -> Arc<MetaProject> {
    let host = VerterHost::new_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
        test_scheduler_config(),
    );
    MetaProject::new(host)
}

fn sfc(props: &str) -> String {
    format!(
        r#"<script setup lang="ts">
defineProps<{{ {props} }}>()
</script>
<template><div>hello</div></template>"#
    )
}

/// Extract prop field names from a FileAnalysisSnapshot's macros.
fn prop_names(
    snapshot: &verter_session_query::analysis::file_analysis::FileAnalysisSnapshot,
) -> Vec<String> {
    snapshot
        .macros
        .iter()
        .filter(|m| m.kind == verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps)
        .flat_map(|m| m.prop_fields.iter())
        .map(|f| f.name.clone())
        .collect()
}

/// Resolved-macro prop names sourced from the SOLE typeinfo macro-surface
/// authority (`vue_macro_dtos`, FullMetadata), keyed on each DefineProps macro
/// index (deduped, mirroring the production producer). The resolved-state
/// `ResolvedMacroMeta` no longer carries the published props/emits/slots/
/// exposed surface; it supplies only the macro index + kind for provenance.
fn resolved_macro_prop_names(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Vec<String> {
    let mut seen = rustc_hash::FxHashSet::default();
    state
        .resolved_macros
        .iter()
        .filter(|m| {
            m.macro_kind == verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps
        })
        .filter(|m| seen.insert(m.macro_index))
        .flat_map(|m| {
            host.vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
                owner_canonical: std::sync::Arc::from(owner),
                macro_index: m.macro_index,
                macro_kind: m.macro_kind,
                root_identity: host.current_or_read_whole_hash(owner).unwrap_or([0u8; 16]),
                level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
            })
            .expect("the Vue adapter is admitted")
            .prop_fields()
            .iter()
            .map(|p| p.analysis.name.clone())
            .collect::<Vec<_>>()
        })
        .collect()
}

fn evaluated_prop_type(
    project: &MetaProject,
    owner_canonical: &str,
    types: &ExpandedComponentTypes,
    name: &str,
) -> TypeExpr {
    let field = types
        .props
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("missing evaluated prop {name}"));
    crate::test_only::semantic_source_probe::demand_type_expr(
        project.host(),
        owner_canonical,
        field
            .authority
            .source_position()
            .present()
            .expect("present source"),
    )
    .unwrap_or_else(|| panic!("evaluated prop {name}'s published source must demand-materialize"))
}

/// Shallow companion of [`evaluated_prop_type`]: shell-materializes the
/// published source WITHOUT a resolution demand, so shallow published
/// carriers (`Ref { .. }`, utility carriers) survive for assertions that
/// pin the published shape itself.
fn evaluated_prop_shallow_type(
    project: &MetaProject,
    owner_canonical: &str,
    types: &ExpandedComponentTypes,
    name: &str,
) -> TypeExpr {
    let field = types
        .props
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("missing evaluated prop {name}"));
    crate::test_only::semantic_source_probe::shallow_type_expr(
        project.host(),
        owner_canonical,
        field
            .authority
            .source_position()
            .present()
            .expect("present source"),
    )
    .unwrap_or_else(|| panic!("evaluated prop {name}'s published source must shell-materialize"))
}

fn evaluated_define_props_type(
    project: &MetaProject,
    owner_canonical: &str,
    types: &ExpandedComponentTypes,
    name: &str,
) -> TypeExpr {
    let prop = types
        .define_props
        .iter()
        .flat_map(|entry| entry.result.value.properties.iter())
        .find(|prop| prop.name == name)
        .unwrap_or_else(|| panic!("missing defineProps property {name}"));
    crate::test_only::semantic_source_probe::demand_type_expr(
        project.host(),
        owner_canonical,
        prop.ty.present().expect("present source"),
    )
    .unwrap_or_else(|| {
        panic!(
            "defineProps property {name}'s published source must demand-materialize: {:?}",
            prop.ty
        )
    })
}

/// Shallow companion of [`evaluated_define_props_type`]: shell-materializes
/// the published source WITHOUT a resolution demand, so shallow published
/// carriers (`Ref { .. }`, symbolic indexed accesses) survive for assertions
/// that pin the published shape itself.
fn evaluated_define_props_shallow_type(
    project: &MetaProject,
    owner_canonical: &str,
    types: &ExpandedComponentTypes,
    name: &str,
) -> TypeExpr {
    let prop = types
        .define_props
        .iter()
        .flat_map(|entry| entry.result.value.properties.iter())
        .find(|prop| prop.name == name)
        .unwrap_or_else(|| panic!("missing defineProps property {name}"));
    crate::test_only::semantic_source_probe::shallow_type_expr(
        project.host(),
        owner_canonical,
        prop.ty.present().expect("present source"),
    )
    .unwrap_or_else(|| {
        panic!(
            "defineProps property {name}'s published source must shell-materialize: {:?}",
            prop.ty
        )
    })
}

/// Demand-materializes a published `SemanticTypeSource` through the shared
/// dispatch. Panics loudly when no typed source was published or the demand
/// yields nothing — a runtime `None` must fail the test, never soften it.
fn demand_published_type(
    host: &VerterHost,
    owner_canonical: &str,
    source: Option<&verter_type_expr::facts::SemanticTypeSource>,
    what: &str,
) -> TypeExpr {
    let source = source.unwrap_or_else(|| panic!("{what} must publish a typed source"));
    crate::test_only::semantic_source_probe::demand_type_expr(host, owner_canonical, source)
        .unwrap_or_else(|| panic!("{what}'s published source must demand-materialize: {source:?}"))
}

/// Shallow companion of [`demand_published_type`]: shell-materializes the
/// published source WITHOUT a resolution demand, so shallow published
/// carriers (`Ref`, utility carriers, symbolic indexed accesses) survive
/// for assertions that pin the published shape itself.
fn shallow_published_type(
    host: &VerterHost,
    owner_canonical: &str,
    source: Option<&verter_type_expr::facts::SemanticTypeSource>,
    what: &str,
) -> TypeExpr {
    let source = source.unwrap_or_else(|| panic!("{what} must publish a typed source"));
    crate::test_only::semantic_source_probe::shallow_type_expr(host, owner_canonical, source)
        .unwrap_or_else(|| panic!("{what}'s published source must shell-materialize: {source:?}"))
}

fn assert_union_string_literals(expr: &TypeExpr, expected: &[&str]) {
    let mut actual = BTreeSet::new();
    match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => {
            actual.insert(value.as_str());
        }
        TypeExpr::Union(types) => {
            for ty in types.iter() {
                match ty {
                    TypeExpr::Literal(LiteralValue::String(value)) => {
                        actual.insert(value.as_str());
                    }
                    TypeExpr::Primitive(PrimitiveName::Undefined) => {}
                    other => panic!(
                        "expected only string literal members (plus optional undefined), got {other:?}"
                    ),
                }
            }
        }
        other => panic!("expected string literal union, got {other:?}"),
    }

    assert_eq!(
        actual,
        BTreeSet::from_iter(expected.iter().copied()),
        "unexpected literal union members for {expr:?}"
    );
}

fn cached_resolved_state(
    project: &MetaProject,
    canonical: &str,
    mode: verter_type_engine::semantic_query::ProjectionMode,
) -> Option<Arc<crate::meta_resolve::ResolvedComponentMetaState>> {
    // The slot key is `(mode, view_fingerprint)`. Test fixtures
    // exercise a single session at a time; the helper prefers the
    // overlay-bearing slot (view_fingerprint != 0) if present and
    // falls back to the base slot (view_fingerprint == 0) so the
    // historical "give me the latest cached state for this mode"
    // semantics are preserved across the view-aware migration.
    fn pick<'a>(
        entries: impl Iterator<
            Item = (
                &'a (verter_type_engine::semantic_query::ProjectionMode, u64),
                &'a crate::types::ResolvedComponentMetaCacheEntry,
            ),
        >,
        mode: verter_type_engine::semantic_query::ProjectionMode,
    ) -> Option<Arc<crate::meta_resolve::ResolvedComponentMetaState>> {
        let mut base = None;
        let mut overlay = None;
        for ((slot_mode, view_fp), cached) in entries {
            if slot_mode != &mode {
                continue;
            }
            if *view_fp == 0 {
                base = Some(Arc::clone(&cached.state));
            } else if overlay.is_none() {
                overlay = Some(Arc::clone(&cached.state));
            }
        }
        overlay.or(base)
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        // cached_resolved_meta lives on DerivedRawState (D48 split).
        project
            .host()
            .derived_raw_cache()
            .get(canonical)
            .and_then(|entry| pick(entry.cached_resolved_meta.iter(), mode))
    }

    #[cfg(target_arch = "wasm32")]
    {
        let files = crate::shared::read_lock(&project.host().files);
        files
            .get(canonical)
            .and_then(|entry| pick(entry.cached_resolved_meta.iter(), mode))
    }
}

fn clear_legacy_cached_resolved_state(
    project: &MetaProject,
    canonical: &str,
    mode: verter_type_engine::semantic_query::ProjectionMode,
) {
    // Drops every slot matching `mode` regardless of view fingerprint
    // so a test fixture exercising base / overlay isolation can reset
    // the cache to a known-empty state for `mode`.
    #[cfg(not(target_arch = "wasm32"))]
    {
        // cached_resolved_meta lives on DerivedRawState (D48 split).
        if let Some(mut entry) = project.host().derived_raw_cache().get_mut(canonical) {
            entry
                .cached_resolved_meta
                .retain(|(slot_mode, _view_fp), _| slot_mode != &mode);
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut files = crate::shared::write_lock(&project.host().files);
        if let Some(entry) = files.get_mut(canonical) {
            entry
                .cached_resolved_meta
                .retain(|(slot_mode, _view_fp), _| slot_mode != &mode);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn cached_fallthrough_state(
    project: &MetaProject,
    canonical: &str,
) -> Option<Arc<crate::types::FallthroughResolution>> {
    // cached_fallthrough lives on DerivedRawState (D48 split).
    project
        .host()
        .derived_raw_cache()
        .get(canonical)
        .and_then(|entry| {
            entry
                .cached_fallthrough
                .as_ref()
                .map(|cached| Arc::clone(&cached.resolution))
        })
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_legacy_cached_fallthrough_state(project: &MetaProject, canonical: &str) {
    // cached_fallthrough lives on DerivedRawState (D48 split).
    if let Some(mut entry) = project.host().derived_raw_cache().get_mut(canonical) {
        entry.cached_fallthrough = None;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_runtime_top_level_fallthrough_node(project: &MetaProject, canonical: &str) {
    let key = crate::resolver_core::fallthrough_cache_key(
        canonical,
        project.host().config.generic_root_propagation,
        None,
    );
    project
        .host()
        .resolver_runtime()
        .fallthrough
        .remove_node_for_test(&key);
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_runtime_root_follow_node(project: &MetaProject, canonical: &str) {
    let key = crate::resolver_core::fallthrough_resolver::root_follow_key(
        canonical,
        crate::resolver_core::FallthroughOverrideIdentity::NoOverrides,
        project.host().config.generic_root_propagation,
    );
    project
        .host()
        .resolver_runtime()
        .fallthrough
        .remove_node_for_test(&key);
}

#[cfg(not(target_arch = "wasm32"))]
fn cached_fallthrough_entry(
    project: &MetaProject,
    canonical: &str,
) -> Option<crate::types::CachedFallthroughEntry> {
    // cached_fallthrough lives on DerivedRawState (D48 split).
    project
        .host()
        .derived_raw_cache()
        .get(canonical)
        .and_then(|entry| entry.cached_fallthrough.clone())
}

/// A no-macro owner whose ONLY budget consumer is the fallthrough spread walker:
/// resolve does zero projection ops (no `defineProps` to instantiate), so the
/// low projection budget is tripped exclusively MID-FALLTHROUGH. The spread is a
/// `v-bind` of a union of distinct-keyed objects on a native root, so the walker
/// descends every union arm and trips the cap.
fn upsert_fallthrough_spread_owner(project: &Arc<MetaProject>) {
    project
        .upsert_base(
            "/src/obj.ts",
            r#"export declare const obj: { a: string } | { b: string } | { c: string } | { d: string };"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { obj } from './obj'
</script>
<template><div v-bind="obj" /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./obj".to_string(),
            resolved_canonical_id: Some("/src/obj.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
}

/// The emitted runtime option value (`props: { … }` / `emits: [ … ]`),
/// bracket-matched out of a rendered module.
///
/// The assertion target has to be this object and nothing else. A rendered
/// `<script setup>` module splices the AUTHORED script verbatim into
/// `setup(__props)`, so `code.contains("label")` is satisfied by the helper
/// source whatever the props block says — a props check written that way
/// passes against `props: {}`.
#[cfg(not(target_arch = "wasm32"))]
fn emitted_runtime_option(code: &str, option_key: &str) -> String {
    let Some(key) = code.find(option_key) else {
        panic!("the rendered module declares no `{option_key}` option:\n{code}");
    };
    let open = key + option_key.len();
    // The option key is the LAST thing in the module only if the render
    // truncated: index rather than slice-index so a truncated render is a
    // named failure instead of a bounds panic pointing at this helper.
    let (opener, closer) = match code.as_bytes().get(open) {
        Some(b'{') => (b'{', b'}'),
        Some(b'[') => (b'[', b']'),
        Some(other) => {
            panic!("`{option_key}` is neither an object nor an array (got {other:?}):\n{code}")
        }
        None => panic!("the rendered module ends at its `{option_key}` option:\n{code}"),
    };
    let mut depth = 0usize;
    for (offset, byte) in code.as_bytes()[open..].iter().enumerate() {
        if *byte == opener {
            depth += 1;
        } else if *byte == closer {
            depth -= 1;
            if depth == 0 {
                return code[open..=open + offset].to_owned();
            }
        }
    }
    panic!("unbalanced `{option_key}` option:\n{code}");
}

/// What the RUNTIME lane produced for one `<script setup>` body.
#[cfg(not(target_arch = "wasm32"))]
enum RenderedRuntime {
    /// The module compiled; the payload is its `props` option object.
    Props(String),
    /// The lane refused with `XUnavailableMacroSemanticResult`.
    Refused,
}

/// Render `script` as a `<script setup lang="ts">` body whose props type is
/// `ReturnType<typeof makeProps>`, on the RUNTIME (bundler) lane.
#[cfg(not(target_arch = "wasm32"))]
fn render_runtime_props(canonical: &str, script: &str) -> RenderedRuntime {
    render_runtime_macro(
        canonical,
        script,
        "defineProps<ReturnType<typeof makeProps>>()",
        "props: ",
    )
}

/// [`render_runtime_props`] for `defineEmits<ReturnType<typeof makeEmits>>()`.
/// The payload is the emitted `emits: [...]` option array.
#[cfg(not(target_arch = "wasm32"))]
fn render_runtime_emits(canonical: &str, script: &str) -> RenderedRuntime {
    render_runtime_macro(
        canonical,
        script,
        "defineEmits<ReturnType<typeof makeEmits>>()",
        "emits: ",
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn render_runtime_macro(
    canonical: &str,
    script: &str,
    macro_call: &str,
    option_key: &str,
) -> RenderedRuntime {
    use crate::CompileTarget;

    let project = make_project();
    project
        .upsert_base(
            canonical,
            &format!(
                "<script setup lang=\"ts\">\n{script}\n{macro_call}\n</script>\n<template><div /></template>"
            ),
        )
        .unwrap();
    let response = project.host().get_virtual_file(crate::types::VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_owned()),
        node_kind: Some(crate::types::VirtualNodeKind::Main),
        compile_profile: crate::types::CompileProfile {
            target: CompileTarget::BUNDLER,
            ..crate::types::CompileProfile::default()
        },
    });
    match response {
        Ok(response) => {
            let macro_errors: Vec<_> = response
                .diagnostics
                .diagnostics
                .iter()
                .filter(|d| d.code == "XUnavailableMacroSemanticResult")
                .collect();
            assert!(
                macro_errors.is_empty(),
                "{canonical}: a module that emits bytes must not ALSO carry the \
                 macro-semantic refusal; got {macro_errors:?}"
            );
            RenderedRuntime::Props(emitted_runtime_option(&response.code, option_key))
        }
        Err(crate::types::HostError::CompileError(failure)) => {
            assert!(
                failure
                    .diagnostics
                    .diagnostics
                    .iter()
                    .any(|d| d.code == "XUnavailableMacroSemanticResult"),
                "{canonical}: the refusal must be the macro-semantic diagnostic, not an \
                 unrelated compile failure; got {:?}",
                failure.diagnostics.diagnostics
            );
            RenderedRuntime::Refused
        }
        Err(other) => panic!("{canonical}: unexpected host failure {other:?}"),
    }
}

/// Owner that charges projection ops in BOTH phases over DISJOINT source
/// types: the `defineProps<Partial<PropsBase>>()` macro's Expanded resolve
/// instantiates the mapped utility (resolve-phase ops), and the
/// `v-bind="obj"` spread's fallthrough walk resolves a 4-arm union
/// (fallthrough-phase ops). Disjoint types ⇒ the two phases' op costs are
/// additive (no shared sub-resolution that would warm one phase from the
/// other).
fn upsert_mixed_budget_owner(project: &Arc<MetaProject>) {
    project
        .upsert_base(
            "/src/propsbase.ts",
            r#"export interface PropsBase { a: string; b: number; c: boolean; d: string; e: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/obj.ts",
            r#"export declare const obj: { p: string } | { q: string } | { r: string } | { s: string };"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { PropsBase } from './propsbase'
import { obj } from './obj'
defineProps<Partial<PropsBase>>()
</script>
<template><div v-bind="obj" /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./propsbase".to_string(),
                resolved_canonical_id: Some("/src/propsbase.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./obj".to_string(),
                resolved_canonical_id: Some("/src/obj.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
}

/// The mixed owner with the `v-bind` spread REMOVED — same props macro, no
/// fallthrough spread — so the fallthrough phase charges ~0 ops. Isolates
/// the RESOLVE-phase op cost `R`.
fn upsert_mixed_budget_resolve_only_owner(project: &Arc<MetaProject>) {
    project
        .upsert_base(
            "/src/propsbase.ts",
            r#"export interface PropsBase { a: string; b: number; c: boolean; d: string; e: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { PropsBase } from './propsbase'
defineProps<Partial<PropsBase>>()
</script>
<template><div>no spread</div></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./propsbase".to_string(),
            resolved_canonical_id: Some("/src/propsbase.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
}

/// Owner whose `defineProps<>` macro-DTO COLD resolution trips the projection-op
/// budget: a 32-arm `Partial<S..>` intersection over an imported helper. NO
/// fallthrough spread (`<div />`) — so the budget trip is PURELY in the
/// PRE-CHOKE macro-DTO extraction and the fallthrough choke charges ~0 ops and
/// cannot independently bound the work. The 32-arm intersection trips a tight
/// budget mid-materialisation; the DTO is returned partial and REFUSED
/// `vue_surface_store` admission (`vue_exec` admits ONLY a Complete bundle).
fn upsert_macro_dto_budget_owner(project: &Arc<MetaProject>) {
    use std::fmt::Write as _;
    let mut helper = String::new();
    for n in 1..=32u32 {
        let _ = writeln!(
            helper,
            "export interface S{n:02} {{ a{n:02}: string; b{n:02}: number; c{n:02}: boolean }}"
        );
    }
    project.upsert_base("/src/dto_helper.ts", &helper).unwrap();

    let mut names = String::new();
    let mut arms = String::new();
    for n in 1..=32u32 {
        if n > 1 {
            names.push_str(", ");
            arms.push_str(" & ");
        }
        let _ = write!(names, "S{n:02}");
        let _ = write!(arms, "Partial<S{n:02}>");
    }
    let source = format!(
        "<script setup lang=\"ts\">\n\
         import type {{ {names} }} from './dto_helper'\n\
         defineProps<{arms}>();\n\
         </script>\n\
         <template><div /></template>\n"
    );
    project.upsert_base("/src/App.vue", &source).unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./dto_helper".to_string(),
            resolved_canonical_id: Some("/src/dto_helper.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
}

// ===========================================================================
// Phase 1: Provenance counters and enriched-analysis caching
// ===========================================================================

/// Helper to read the provenance counters from a MetaProject's host.
fn provenance(project: &MetaProject) -> crate::meta_provenance::MetaProvenanceSnapshot {
    project.host().provenance().snapshot()
}

/// Assert `ty` is the CONCRETE `typeof theme` object surface used by
/// the owner-local registry fixtures:
/// `{ variants: { color: { primary: ''; secondary: '' } } }`, pinned
/// all the way to the `as const` string-literal leaves. `label`
/// identifies the asserting site in panic messages. A DIFFERENT
/// in-scope const (different member names / values) cannot satisfy
/// this — the assertion discriminates a wrong-but-consistent
/// substitution that bound the type parameter to another same-file
/// type.
fn assert_concrete_theme_surface(ty: &TypeExpr, label: &str) {
    let find_prop = |members: &[ObjectMember], prop_name: &str| -> Option<TypeExpr> {
        members.iter().find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == prop_name =>
            {
                Some(property.ty.clone())
            }
            _ => None,
        })
    };
    let TypeExpr::Object(obj) = ty else {
        panic!("{label} must be the concrete `typeof theme` object surface, got {ty:?}");
    };
    let variants = find_prop(&obj.properties, "variants").unwrap_or_else(|| {
        panic!("{label} (`typeof theme`) must expose a `variants` member, got {obj:?}")
    });
    let TypeExpr::Object(variants_obj) = &variants else {
        panic!("{label} `typeof theme`.variants must be an object, got {variants:?}");
    };
    let color = find_prop(&variants_obj.properties, "color").unwrap_or_else(|| {
        panic!("{label} `typeof theme`.variants must expose a `color` member, got {variants_obj:?}")
    });
    let TypeExpr::Object(color_obj) = &color else {
        panic!("{label} `typeof theme`.variants.color must be an object, got {color:?}");
    };
    let mut color_keys: Vec<&str> = color_obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => {
                Some(property.string_name().expect("string-key fixture"))
            }
            _ => None,
        })
        .collect();
    color_keys.sort_unstable();
    assert_eq!(
        color_keys,
        ["primary", "secondary"],
        "{label} `typeof theme`.variants.color must expose exactly the fixture's \
         primary/secondary keys, got {color_obj:?}",
    );
    assert_eq!(
        find_prop(&color_obj.properties, "primary").as_ref(),
        Some(&TypeExpr::string_literal("")),
        "{label} `typeof theme`.variants.color.primary must be the `''` const literal, \
         got {color_obj:?}",
    );
    assert_eq!(
        find_prop(&color_obj.properties, "secondary").as_ref(),
        Some(&TypeExpr::string_literal("")),
        "{label} `typeof theme`.variants.color.secondary must be the `''` const literal, \
         got {color_obj:?}",
    );
}

// ===========================================================================
// Phase 3: Fallthrough inheritance resolver
// ===========================================================================

use verter_session_query::analysis::component_meta::{
    AcceptedEventKind, AcceptedPropKind, AcceptedSurfaceCompleteness, BranchStatus,
    FallthroughSurface, MemberAvailability, MemberProvenance, PartialBranchReason,
    ResolvedRootStep, UnresolvedBranchReason,
};

/// Helper: get the component meta for a file (through session).
/// The typed masking probe over a published prop surface: every present
/// prop source reads NOT-degraded on the raise-time sidecar (fold +
/// `from_parts` choke point — independent of `synthesis_should_suppress`).
fn assert_no_degraded_props(
    host: &VerterHost,
    canonical: &str,
    meta: &verter_session_query::analysis::component_meta::ComponentMetaAnalysis,
) {
    for p in &meta.props {
        let Some(source) = p.publication.result().selected_source() else {
            continue;
        };
        assert_eq!(
            crate::test_only::semantic_source_probe::shallow_is_degraded(host, canonical, source),
            Some(false),
            "prop `{}` must carry NO typed degradation on its raise-time sidecar (masking case)",
            p.name,
        );
    }
}

fn get_meta(
    project: &Arc<MetaProject>,
    canonical_id: &str,
) -> verter_session_query::analysis::component_meta::ComponentMetaAnalysis {
    let session = project.open_session_batch().unwrap();
    session
        .get_component_meta(canonical_id)
        .unwrap()
        .expect("get_component_meta should return metadata")
}

/// Every root-chain step reachable from a resolved fallthrough surface.
fn root_chain_steps(
    meta: &verter_session_query::analysis::component_meta::ComponentMetaAnalysis,
) -> Vec<ResolvedRootStep> {
    match &meta.fallthrough_surface {
        FallthroughSurface::Branches { branches } => branches
            .iter()
            .flat_map(|branch| branch.root_chain.iter().cloned())
            .collect(),
        FallthroughSurface::None { .. } => Vec::new(),
    }
}

/// True when `arm` is a `(payload: <name>) => …` handler whose sole parameter's
/// type references `event_name` (by `Ref` name or by the `source: '<discriminant>'`
/// literal of its resolved object body).
fn payload_param_references(arm: &TypeExpr, event_name: &str) -> bool {
    match arm {
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => function
            .parameters
            .first()
            .is_some_and(|param| payload_ty_references(&param.ty, event_name)),
        TypeExpr::Parenthesized(inner) => payload_param_references(inner, event_name),
        _ => false,
    }
}

/// True when `ty` references `event_name` — either as a bare `Ref { name }` or as
/// the resolved object body carrying the matching `source: '<discriminant>'`
/// literal (`FallbackClickEvent` ⇒ `'fallback'`, `ProjectClickEvent` ⇒ `'project'`).
fn payload_ty_references(ty: &TypeExpr, event_name: &str) -> bool {
    let discriminant = match event_name {
        "FallbackClickEvent" => "fallback",
        "ProjectClickEvent" => "project",
        _ => return false,
    };
    match ty {
        TypeExpr::Ref { name, .. } => name.as_ref() == event_name,
        TypeExpr::Object(shape) => shape.properties.iter().any(|member| {
            matches!(
                member,
                ObjectMember::Property(property)
                    if property.string_name().expect("string-key fixture") == "source"
                        && matches!(
                            &property.ty,
                            TypeExpr::Literal(verter_type_expr::LiteralValue::String(value))
                                if value == discriminant
                        )
            )
        }),
        TypeExpr::Parenthesized(inner) => payload_ty_references(inner, event_name),
        _ => false,
    }
}

// ===========================================================================
// Payload cache tests
// ===========================================================================

/// A simple encode function for tests: deterministic bytes from analysis+resolved.
/// Uses the prop count + file path to produce reproducible output.
fn test_encode_fn(output: crate::meta_resolve::ComponentMetaOutput) -> Vec<u8> {
    let (analysis, _resolution, _types) = output.into_parts();
    // Produce deterministic bytes based on the analysis content.
    let marker = format!(
        "payload:{}:props={}:events={}",
        analysis.file_path,
        analysis.props.len(),
        analysis.events.len(),
    );
    marker.into_bytes()
}

/// Collect every call ident (free/assoc-fn last path segment + method-call name)
/// syntactically reachable inside the fn named `target` (a nested `fn` or an impl
/// method), with depth tracking so sibling fns never leak.
#[cfg(test)]
fn output_sink_calls_in(src: &str, target: &str) -> std::collections::BTreeSet<String> {
    use std::collections::BTreeSet;
    use syn::visit::Visit;

    #[derive(Default)]
    struct CallCollector {
        target: String,
        depth: usize,
        found: bool,
        calls: BTreeSet<String>,
    }
    impl<'ast> Visit<'ast> for CallCollector {
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            let hit = f.sig.ident == self.target;
            if hit {
                self.found = true;
                self.depth += 1;
            }
            syn::visit::visit_item_fn(self, f);
            if hit {
                self.depth -= 1;
            }
        }
        fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
            let hit = f.sig.ident == self.target;
            if hit {
                self.found = true;
                self.depth += 1;
            }
            syn::visit::visit_impl_item_fn(self, f);
            if hit {
                self.depth -= 1;
            }
        }
        fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
            if self.depth > 0 {
                if let syn::Expr::Path(p) = c.func.as_ref() {
                    if let Some(seg) = p.path.segments.last() {
                        self.calls.insert(seg.ident.to_string());
                    }
                }
            }
            syn::visit::visit_expr_call(self, c);
        }
        fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
            if self.depth > 0 {
                self.calls.insert(m.method.to_string());
            }
            syn::visit::visit_expr_method_call(self, m);
        }
    }

    let file = syn::parse_file(src).expect("output_sink.rs must parse");
    let mut collector = CallCollector {
        target: target.to_string(),
        ..Default::default()
    };
    collector.visit_file(&file);
    assert!(
        collector.found,
        "target fn `{target}` not found in output_sink.rs (renamed/removed?) — the \
         characterization must not vacuously pass"
    );
    collector.calls
}

// ===========================================================================
// Events-payload output materialization (the session output envelope)
// ===========================================================================

/// Expected-shape helper for the output-envelope tests: a labeled,
/// non-optional, non-rest tuple.
fn labeled_tuple(elements: &[(&str, TypeExpr)]) -> TypeExpr {
    TypeExpr::Tuple {
        elements: elements
            .iter()
            .map(|(label, ty)| verter_type_expr::TupleElement {
                label: Some((*label).to_string()),
                ty: ty.clone(),
                optional: false,
                rest: false,
            })
            .collect::<Vec<_>>()
            .into(),
        readonly: false,
    }
}

// ===========================================================================
// Full output-boundary conversion: all 13 wire lanes, view-fence equivalence,
// cache rails, request-local dedupe, cross-owner effective scope
// ===========================================================================

/// A blank analysis carrier for the synthetic lane tests: every lane empty,
/// no template, `Exact` accepted surface.
fn blank_output_analysis() -> verter_session_query::analysis::component_meta::ComponentMetaAnalysis
{
    use verter_session_query::analysis::component_meta as cm;
    cm::ComponentMetaAnalysis {
        props: Vec::new(),
        events: Vec::new(),
        slots: Vec::new(),
        models: Vec::new(),
        exposed: Vec::new(),
        public_instance: None,
        ordered_sfc_structure: None,
        type_registry: Vec::new(),
        components: Vec::new(),
        template_refs: Vec::new(),
        imports: Vec::new(),
        bindings: Vec::new(),
        vue_api_calls: Vec::new(),
        styles: Vec::new(),
        flags: cm::ComponentMetaFlags::default(),
        root_reachability: cm::RootReachability::NoFallthrough {
            reason: cm::NoFallthroughReason::NoTemplate,
        },
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness: cm::AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface: cm::FallthroughSurface::None {
            reason: cm::NoFallthroughReason::NoTemplate,
        },
        macro_expansion_diagnostics: Vec::new(),
        options_api: false,
        file_path: "/App.vue".to_string(),
    }
}

/// A closed leaf-`Ref` source (the shallow name-as-written carrier).
fn closed_ref_source(name: &str) -> verter_type_expr::facts::SemanticTypeSource {
    verter_type_expr::facts::SemanticTypeSource::Closed(
        verter_type_expr::facts::ClosedTypeFact::Leaf(verter_type_expr::facts::LeafTypeFact::Ref(
            name.to_string(),
        )),
    )
}

/// An authored decl-body source anchored at `canonical`/`symbol` — raisable
/// iff that declaration exists under the live view.
fn authored_decl_body_source(
    canonical: &str,
    symbol: &str,
) -> verter_type_expr::facts::SemanticTypeSource {
    verter_type_expr::facts::SemanticTypeSource::Authored(
        verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
            verter_type_expr::locators::TypeBodySlot {
                anchor: verter_type_expr::locators::AuthoredAnchor {
                    canonical_id: Arc::from(canonical),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol: Arc::from(symbol),
                    space: verter_type_expr::locators::LocatorSymbolSpace::Type,
                },
                path: Arc::from(Vec::new().into_boxed_slice()),
            },
        ),
    )
}

/// Arm the test-only pre-resolve hook on the SHARED cold bodies: `hook`
/// runs ONCE, inside the next cold component-meta entry, in exactly the
/// capture→resolve window (after the entry captured its fixed store view,
/// before the pinned resolve dispatches).
fn arm_cold_body_pre_resolve_hook(hook: impl FnOnce() + 'static) {
    crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
}

/// The FIX-1 torn-result fixture: `/dep.ts` publishes the whole props
/// payload; a SIBLING owner pre-materializes the dep's artifacts under the
/// ORIGINAL content so the pinned cold compute can read the captured view's
/// candidates; the owner under test stays COLD.
fn seed_pinned_view_fixture(project: &Arc<MetaProject>, owner: &str) {
    project
        .upsert_base("/dep.ts", "export interface DepProps { value: number }\n")
        .unwrap();
    let sfc = r#"<script setup lang="ts">
import type { DepProps } from './dep'
defineProps<DepProps>()
</script>
<template><div /></template>"#;
    project.upsert_base("/PinSibling.vue", sfc).unwrap();
    project.upsert_base(owner, sfc).unwrap();
    let sibling = project
        .host()
        .get_component_meta("/PinSibling.vue")
        .expect("sibling owner resolves");
    assert_eq!(
        sibling.props.len(),
        1,
        "fixture premise: the original dep publishes exactly one prop"
    );
}

/// The dep mutation the hook lands inside the capture→resolve window: the
/// NEW content grows a second prop and flips the first prop's type, so a
/// resolve that opens its own fresh store view is unmistakable (2 props /
/// string) versus the captured view's world (1 prop / number).
fn mutate_pinned_view_dep(project: &Arc<MetaProject>) {
    project
        .upsert_base(
            "/dep.ts",
            "export interface DepProps { value: string; extra: boolean }\n",
        )
        .unwrap();
}

/// A `TypeBodySlot` locator anchored at a declaration that does not exist —
/// the failed REQUIRED interior dereference every FIX-3 composite fixture
/// embeds.
fn missing_interior_slot() -> verter_type_expr::locators::TypeBodySlot {
    verter_type_expr::locators::TypeBodySlot {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from("/definitely-missing.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("NoSuchType"),
            space: verter_type_expr::locators::LocatorSymbolSpace::Type,
        },
        path: Arc::from(Vec::new().into_boxed_slice()),
    }
}

/// Wrap a composite source into a one-prop analysis and build the output.
fn build_output_with_prop_source(
    host: &VerterHost,
    source: verter_type_expr::facts::SemanticTypeSource,
) -> Result<crate::meta_resolve::ComponentMetaOutput, crate::meta_resolve::ComponentMetaOutputError>
{
    let mut analysis = blank_output_analysis();
    analysis.props.push(
        verter_session_query::analysis::component_meta::PropAnalysis {
            name: "p".to_string(),
            callable_role: verter_type_expr::PropCallableRole::default(),
            publication: crate::test_only::type_publication_fixture(
                verter_type_expr::facts::SourcePosition::Present(source),
                verter_type_expr::ResolutionExactness::ExactConcrete,
                None,
                None,
            ),
            type_expansion: None,
            required: true,
            has_default: false,
            default_value: None,
            description: None,
            tags: Vec::new(),
            declared_in_macro_type_arg: false,
        },
    );

    let fixture_dispatch_18 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_18,
        "/App.vue",
        analysis,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
}

/// Shared body for same-name intersection cases containing a stable unresolved
/// reference. `MissingType` is not an operational failure: it stays an
/// explicit carrier beside the concrete `string` contributor. Neither arm may
/// mask or erase the other, and the result remains complete.
fn assert_same_name_intersection_prop_preserves_unresolved_carrier(type_argument: &str) {
    let project = make_project();
    // `/bad.ts` compiles as a file but `MissingType` does not exist anywhere:
    // `Bad.x` therefore carries a stable unresolved authored reference.
    project
        .upsert_base("/bad.ts", "export interface Bad { x: MissingType }\n")
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            &format!(
                r#"<script setup lang="ts">
import type {{ Bad }} from './bad'
defineProps<{type_argument}>()
</script>
<template><div /></template>"#
            ),
        )
        .unwrap();

    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("a stable unresolved carrier materializes without an output failure")
        .expect("the SFC resolves")
        .into_parts();
    let index = analysis
        .props
        .iter()
        .position(|prop| prop.name == "x")
        .expect("the merged prop publishes");
    let lanes = types.into_lanes();
    let TypeExpr::Intersection(arms) = lanes.props[index]
        .materialized_type()
        .expect("published type")
    else {
        panic!(
            "the merged member must preserve both contributors as an intersection; got {:?}",
            lanes.props[index]
        );
    };
    assert!(
        arms.iter()
            .any(|arm| matches!(arm, TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "MissingType" && type_arguments.is_empty())),
        "the unresolved contributor remains an explicit Ref carrier; got {arms:?}"
    );
    assert!(
        arms.iter()
            .any(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::String))),
        "the local contributor remains present beside the carrier; got {arms:?}"
    );

    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("the analysis itself still assembles");
    assert!(
        !state.completeness.is_partial(),
        "a stable unresolved contributor is Complete; got {:?}",
        state.completeness
    );
    assert!(
        !state.synthesis_should_suppress,
        "stable unresolved carriers do not suppress warm admission"
    );
}

/// Leaf union arms of `expr`, walking through nested `Union` layers (an
/// authored `(A | B) | undefined` publishes the distributed inner union
/// as a nested node — arm MEMBERSHIP is the semantic contract asserted
/// here, not the nesting shape).
fn flatten_union_arms(expr: &TypeExpr) -> Vec<&TypeExpr> {
    match expr {
        TypeExpr::Union(members) => members.iter().flat_map(flatten_union_arms).collect(),
        other => vec![other],
    }
}

/// Render `script` as a `<script setup lang="ts">` body under an ARBITRARY
/// macro call, on the RUNTIME (bundler) lane.
///
/// [`render_runtime_props`] pins the single-argument
/// `defineProps<ReturnType<typeof makeProps>>()` shape. A surface assembled
/// from SEVERAL producers — an intersection arm, a heritage clause, a
/// `withDefaults` wrapper — needs the macro call spelled per row, because
/// the defect those rows exist to catch is invisible when the degraded
/// producer is the only one.
#[cfg(not(target_arch = "wasm32"))]
fn render_runtime_composed(
    canonical: &str,
    script: &str,
    macro_call: &str,
    option_key: &str,
) -> RenderedRuntime {
    render_runtime_macro(canonical, script, macro_call, option_key)
}

/// Build the fixture whose partiality is reachable ONLY through the extract
/// phase: a WIDE child (24 inherited prop interfaces) whose surface the parent
/// touches solely through its fallthrough compute, and a parent whose own
/// macro surface is a single inline literal.
///
/// With a projection budget large enough for the parent's own resolve but too
/// small for the fallthrough walk over the child's surface,
/// `resolved.completeness` stays Complete while the extract scope trips —
/// exactly the shape the resolve-phase term cannot see. A generous budget is
/// the control: nothing trips, the result is Complete, and it warms.
#[cfg(not(target_arch = "wasm32"))]
fn extract_only_partial_project(projection_op_budget: usize) -> Arc<MetaProject> {
    use std::fmt::Write as _;
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget,
        ..HostConfig::default()
    });
    let mut helper = String::new();
    for n in 1..=24u32 {
        let _ = writeln!(helper, "export interface P{n:02} {{ a{n:02}: string }}");
    }
    let _ = writeln!(
        helper,
        "export interface ChildProps extends {} {{ label: string }}",
        (1..=24u32)
            .map(|n| format!("P{n:02}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    project.upsert_base("/src/child_props.ts", &helper).unwrap();
    project
        .upsert_base(
            "/src/WideChild.vue",
            "<script setup lang=\"ts\">\nimport type { ChildProps } from './child_props'\ndefineProps<ChildProps>()\n</script>\n<template><div /></template>",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/WideParent.vue",
            "<script setup lang=\"ts\">\nimport Child from '/src/WideChild.vue'\ndefineProps<{ title: string }>()\n</script>\n<template><Child label=\"x\" /></template>",
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/WideChild.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./child_props".to_string(),
            resolved_canonical_id: Some("/src/child_props.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/WideParent.vue",
        vec![crate::types::DependencyResolution {
            specifier: "/src/WideChild.vue".to_string(),
            resolved_canonical_id: Some("/src/WideChild.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project
}

/// The budget at which the parent's OWN resolve completes but the fallthrough
/// extract trips. Measured on this fixture: the window is 4..=100 (below 4 the
/// resolve itself is partial, so the resolve term would already carry it; at
/// 150 and above nothing trips at all).
#[cfg(not(target_arch = "wasm32"))]
const EXTRACT_ONLY_PARTIAL_BUDGET: usize = 20;

mod caching;
mod events;
mod expose;
mod fallthrough;
mod framework;
mod general;
mod generics;
mod models;
mod props;
mod publication;
mod resolution;
mod slots;
