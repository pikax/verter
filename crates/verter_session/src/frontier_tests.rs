//! Behavioral tests for the semantic type resolution overhaul.
//!
//! Invariants under test:
//! - Each canonical imported file is loaded/parsed at most once per request.
//! - Barrel wildcard chains respect declared order and avoid repeated sibling rescans.
//! - Same-file symbol closure does not enqueue same-file symbols as external work.
//! - Cross-file resolution deduplicates `(canonical_id, exported_name)` pairs.
//! - Cycle handling is deterministic and does not reopen traversal.
//! - Builder lookup does not perform file I/O on cache hits.
//! - Shared-host reuse: once a file is ready, different entrypoints reuse state.

use std::sync::Arc;

use crate::{CompileErrorPolicy, FileLanguage, HostConfig, UpsertRequest, VerterHost};
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn strict_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_non_sfc(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}
/// CountingWorkspace â€" thin wrapper over MemoryWorkspace that counts reads.
struct CountingWorkspace {
    inner: Arc<verter_workspace::MemoryWorkspace>,
    read_counts: parking_lot::Mutex<rustc_hash::FxHashMap<String, u64>>,
}

impl CountingWorkspace {
    fn new() -> Self {
        Self {
            inner: Arc::new(verter_workspace::MemoryWorkspace::new(
                verter_workspace::MemoryOptions::default(),
            )),
            read_counts: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
        }
    }

    fn inject_file(&self, path: &str, source: &str) {
        self.inner
            .inject_file(path.to_string(), Arc::<str>::from(source));
    }

    fn reset_reads(&self) {
        self.read_counts.lock().clear();
    }

    fn read_count(&self, path: &str) -> u64 {
        self.read_counts.lock().get(path).copied().unwrap_or(0)
    }
}

impl verter_workspace::WorkspaceRead for CountingWorkspace {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        *self
            .read_counts
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner.read_file(canonical_id)
    }

    fn take_last_read_file_trace_detail(&self, canonical_id: &str) -> Option<String> {
        self.inner.take_last_read_file_trace_detail(canonical_id)
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.inner.file_exists(canonical_id)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.inner.realpath(canonical_id)
    }

    fn read_package_manifest(
        &self,
        canonical_id: &str,
    ) -> Option<verter_workspace::PackageManifest> {
        self.inner.read_package_manifest(canonical_id)
    }

    fn classify_file(&self, canonical_id: &str) -> verter_language::FileLanguage {
        self.inner.classify_file(canonical_id)
    }

    fn resolve_import(
        &self,
        importer_id: &str,
        specifier: &str,
        ctx: verter_workspace::ResolutionContext,
    ) -> Option<verter_workspace::ResolveResult> {
        self.inner.resolve_import(importer_id, specifier, ctx)
    }

    fn content_generation(&self) -> u64 {
        self.inner.content_generation()
    }

    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.reverse_deps_for(canonical_id)
    }

    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.forward_deps_for(canonical_id)
    }

    fn dependency_snapshot(
        &self,
        canonical_id: &str,
    ) -> Option<verter_workspace::DependencySnapshotView> {
        self.inner.dependency_snapshot(canonical_id)
    }

    fn read_dir(
        &self,
        dir: &str,
    ) -> Result<Vec<verter_workspace::DirEntry>, verter_workspace::VfsError> {
        self.inner.read_dir(dir)
    }

    fn walk(
        &self,
        root: &str,
        filter_dir: &dyn Fn(&str) -> bool,
        filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, verter_workspace::VfsError> {
        self.inner.walk(root, filter_dir, filter_file)
    }
}

impl verter_workspace::WorkspaceAccess for CountingWorkspace {
    fn record_parsed_edges(&self, canonical_id: &str, edges: &[verter_workspace::ParsedEdge]) {
        self.inner.record_parsed_edges(canonical_id, edges);
    }

    fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        self.inner.set_exact_resolutions(canonical_id, resolutions)
    }
    fn record_parsed_edges_with_exact_resolutions(
        &self,
        canonical_id: &str,
        edges: &[verter_workspace::ParsedEdge],
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        self.inner
            .record_parsed_edges_with_exact_resolutions(canonical_id, edges, resolutions)
    }

    // ── R6/R7: forwarding wrapper for new reverse-graph methods ──
    // The compile-time enforcement ensures CountingWorkspace cannot drop
    // edges silently; every workspace authority method is forwarded to
    // the inner workspace.
    fn replace_semantic_transitive(
        &self,
        canonical_id: &str,
        deps: std::collections::BTreeSet<String>,
    ) {
        self.inner.replace_semantic_transitive(canonical_id, deps);
    }

    fn set_default_resolve_extensions(&self, host_extensions: Vec<String>) {
        self.inner.set_default_resolve_extensions(host_extensions);
    }

    fn record_ambient_dependency(&self, consumer: &str, virtual_id: &str) {
        self.inner.record_ambient_dependency(consumer, virtual_id);
    }

    fn notify_upsert(&self, canonical_id: &str, source: Arc<str>) {
        self.inner.notify_upsert(canonical_id, source);
    }

    fn notify_close(&self, canonical_id: &str) {
        self.inner.notify_close(canonical_id);
    }

    fn notify_delete(&self, canonical_id: &str) {
        self.inner.notify_delete(canonical_id);
    }

    fn configure_resolver(&self, projects: Vec<verter_workspace::resolver::IdeProjectConfig>) {
        self.inner.configure_resolver(projects);
    }
}

fn make_host_with_workspace(ws: Arc<CountingWorkspace>) -> VerterHost {
    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    host
}

fn set_dep(host: &VerterHost, from: &str, specifier: &str, to: &str) {
    host.set_import_dependencies(
        from,
        vec![crate::DependencyResolution {
            specifier: specifier.to_string(),
            resolved_canonical_id: Some(to.to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
}

fn set_deps(host: &VerterHost, from: &str, deps: Vec<(&str, &str)>) {
    host.set_import_dependencies(
        from,
        deps.into_iter()
            .map(|(spec, to)| crate::DependencyResolution {
                specifier: spec.to_string(),
                resolved_canonical_id: Some(to.to_string()),
                possible_canonical_ids: Vec::new(),
            })
            .collect(),
    );
}
/// CHARACTERIZATION of the LAYER-ORDERED named-export walk
/// (`resolve_named_type_export_route_uncached`): a name reachable BOTH via a
/// SHALLOW same-layer wildcard sibling AND via a DEEPER branch behind an
/// EARLIER-DECLARED sibling resolves to the SAME-LAYER declaration, and the
/// deeper branch's file is never loaded. Driven at the route level
/// (`build_named_type_export_route_entry`) so the winning CANONICAL is pinned,
/// not just resolvability.
///
/// Discriminating against the prior declared-order DFS: a depth-first walk
/// descends the earlier-declared `./deep-first` wildcard fully before probing
/// its same-layer sibling, so it (a) resolves `Target` to
/// `/workspace/src/deep-leaf.ts` — failing the canonical assertion — and
/// (b) reads `deep-leaf.ts` — failing the zero-read assertion.
#[test]
fn named_export_route_layer_order_shallow_sibling_wins_over_earlier_deep_branch() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/entry.ts",
        "export * from './deep-first'\nexport * from './same-layer'\n",
    );
    ws.inject_file(
        "/workspace/src/deep-first.ts",
        "export * from './deep-leaf'\n",
    );
    ws.inject_file(
        "/workspace/src/deep-leaf.ts",
        "export interface Target { viaDeep: true }\n",
    );
    ws.inject_file(
        "/workspace/src/same-layer.ts",
        "export interface Target { viaShallow: true }\n",
    );

    let host = make_host_with_workspace(ws.clone());
    set_deps(
        &host,
        "/workspace/src/entry.ts",
        vec![
            ("./deep-first", "/workspace/src/deep-first.ts"),
            ("./same-layer", "/workspace/src/same-layer.ts"),
        ],
    );
    set_dep(
        &host,
        "/workspace/src/deep-first.ts",
        "./deep-leaf",
        "/workspace/src/deep-leaf.ts",
    );

    ws.reset_reads();
    let (route, _facts) = host
        .build_named_type_export_route_entry("/workspace/src/entry.ts", "Target")
        .expect("the layered walk must produce a route entry");

    // The SHALLOW same-layer re-export WINS: the resolved canonical is the
    // same-layer sibling's declaration, never the deeper declaration behind
    // the earlier-declared sibling.
    match &route {
        crate::resolver_core::RouteResult::Resolved {
            defining_canonical,
            defining_owner,
            defining_symbol,
        } => {
            assert_eq!(
                defining_canonical.as_str(),
                "/workspace/src/same-layer.ts",
                "the nearest same-layer re-export must win over the deeper \
                 earlier-declared branch",
            );
            assert_eq!(
                *defining_owner,
                verter_type_expr::TopLevelOwnerId::ordinary_file()
            );
            assert_eq!(defining_symbol.as_str(), "Target");
        }
        other => panic!("Target must resolve through the barrel; got {other:?}"),
    }

    // The deeper branch behind the EARLIER-declared sibling is never loaded:
    // the same-layer direct-export probe decides the route before ANY
    // node's wildcard children are descended.
    assert_eq!(
        ws.read_count("/workspace/src/deep-leaf.ts"),
        0,
        "the deeper branch must not be loaded when a same-layer sibling \
         exports the name",
    );
    // Positive witness: the winning same-layer sibling WAS read — the zero
    // above is a chosen shallow match, not a stalled walk.
    assert!(
        ws.read_count("/workspace/src/same-layer.ts") > 0,
        "the same-layer sibling must have been probed for its direct export",
    );
}
// ===========================================================================
// V6: Imported alias preparation should not eagerly materialize bodies
// ===========================================================================

/// `ProjectionMode::Identity` and `ProjectionMode::Expanded` should both resolve, with `ProjectionMode::Expanded`
/// providing evaluated type shapes.
#[test]
fn type_and_expanded_modes_both_resolve() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string; count: number; active: boolean }\n",
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./types", "/src/types.ts");

    // `ProjectionMode::Identity`: should resolve identity without full expansion
    let meta_type =
        host.resolve_component_meta("/src/Consumer.vue", crate::types::ProjectionMode::Identity);
    assert!(
        meta_type.is_some(),
        "Type-mode component-meta should resolve"
    );
    let type_result = meta_type.unwrap();
    assert!(
        type_result.evaluated_types.is_none(),
        "`ProjectionMode::Identity` should not produce evaluated types"
    );

    // `ProjectionMode::Expanded`: full materialization
    let meta_expanded =
        host.resolve_component_meta("/src/Consumer.vue", crate::types::ProjectionMode::Expanded);
    assert!(
        meta_expanded.is_some(),
        "Expanded-mode component-meta should resolve"
    );
    let expanded_result = meta_expanded.unwrap();
    assert!(
        expanded_result.evaluated_types.is_some(),
        "`ProjectionMode::Expanded` should produce evaluated types"
    );
}
#[test]
fn narrow_route_required_imports_follow_local_export_alias_and_cache_by_route() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"
import type { Alpha } from './alpha'
import type { Beta } from './beta'

interface InternalProps {
  a: Alpha,
  b: Beta
}

export { InternalProps as PublicProps }
"#,
    );
    upsert_non_sfc(
        &host,
        "/src/alpha.ts",
        "export interface Alpha { value: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/beta.ts",
        "export interface Beta { value: number }\n",
    );

    let _ = host.shallow_file_state("/src/types.ts");

    let member_route = crate::resolver_core::RouteDemand::member_path(vec!["a".to_string()]);
    let member_required = host.required_import_names_for_exported_route(
        "/src/types.ts",
        "PublicProps",
        &member_route,
    );
    let whole_required = host.required_import_names_for_exported_route(
        "/src/types.ts",
        "PublicProps",
        &crate::resolver_core::RouteDemand::Whole,
    );

    assert!(
        member_required.contains("Alpha"),
        "narrow route should follow the aliased local symbol"
    );
    assert!(
        !member_required.contains("Beta"),
        "narrow route should not widen to sibling imports"
    );
    assert!(
        !whole_required.contains("Alpha"),
        "whole-route closure should keep direct imported object props symbolic through a local export alias",
    );
    assert!(
        !whole_required.contains("Beta"),
        "whole-route closure should not widen to sibling imported object props through a local export alias",
    );
}
/// Test 4b: Scale test for component-meta with cross-file deps.
/// A large lib file with 200 types, where the root type has a dep
/// pointing to another large file.
#[test]
fn large_cross_file_deps_component_meta_does_not_hang() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });

    // Large "runtime" file with 200 types
    let mut runtime_source = String::new();
    runtime_source.push_str("export interface FunctionalComponent<P = {}> { (props: P): any }\n");
    runtime_source.push_str("export type DefineComponent<P = any> = { new (): { $props: P } }\n");
    runtime_source
        .push_str("export type Component = FunctionalComponent | DefineComponent | string\n");
    for i in 0..200 {
        runtime_source.push_str(&format!(
            "export interface RuntimeType{} {{ value{}: string }}\n",
            i, i,
        ));
    }
    upsert_non_sfc(&host, "/node_modules/runtime/types.d.ts", &runtime_source);

    // Library file that imports from runtime
    let mut lib_source = String::new();
    lib_source.push_str("import type { Component } from 'runtime'\n");
    lib_source.push_str("export interface PrimitiveProps { asChild?: boolean; as?: Component }\n");
    for i in 0..50 {
        lib_source.push_str(&format!(
            "export interface LibType{} extends PrimitiveProps {{ extra{}: number }}\n",
            i, i,
        ));
    }
    lib_source.push_str(
        "export interface AccordionRootProps extends PrimitiveProps { collapsible?: boolean }\n",
    );
    lib_source
        .push_str("export interface AccordionRootEmits { 'update:modelValue': [value: string] }\n");
    upsert_non_sfc(&host, "/node_modules/lib/types.d.ts", &lib_source);

    upsert_vue(
        &host,
        "/src/Accordion.vue",
        r#"<script setup lang="ts">
import type { AccordionRootProps, AccordionRootEmits } from 'lib'
defineProps<AccordionRootProps>()
defineEmits<AccordionRootEmits>()
</script>
<template><div /></template>"#,
    );

    set_dep(
        &host,
        "/src/Accordion.vue",
        "lib",
        "/node_modules/lib/types.d.ts",
    );
    set_dep(
        &host,
        "/node_modules/lib/types.d.ts",
        "runtime",
        "/node_modules/runtime/types.d.ts",
    );

    let meta =
        host.resolve_component_meta("/src/Accordion.vue", crate::types::ProjectionMode::Expanded);
    assert!(
        meta.is_some(),
        "large cross-file component-meta should resolve"
    );
}

/// Test 4c: Deeply nested generics simulating Vue's runtime-core types.
/// `CreateComponentPublicInstanceWithMixins` has 25+ type parameters.
#[test]
fn deeply_nested_generics_component_meta_does_not_hang() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });

    // Simulate Vue's deeply generic runtime-core.d.ts types
    upsert_non_sfc(
        &host,
        "/node_modules/vue-runtime/types.d.ts",
        r#"
type Data = Record<string, unknown>
type ComputedOptions = Record<string, () => any>
type MethodOptions = Record<string, (...args: any[]) => any>
type EmitsOptions = Record<string, ((...args: any[]) => any) | null>
interface ComponentOptionsMixin {}
type IntersectionMixin<T> = T extends ComponentOptionsMixin ? T : never
type UnwrapMixinsType<T, Type extends string> = T extends Record<Type, any> ? T[Type] : {}
type EnsureNonVoid<T> = T extends void ? {} : T
interface ComponentInjectOptions {}
interface SlotsType {}
type Prettify<T> = { [K in keyof T]: T[K] } & {}
type ExtractPropTypes<T> = { [K in keyof T]: T[K] extends { type: infer V } ? V : T[K] }

interface ComponentOptionsBase<
    P, B, D, C extends ComputedOptions, M extends MethodOptions,
    Mixin extends ComponentOptionsMixin, Extends extends ComponentOptionsMixin,
    E extends EmitsOptions, S = string, Defaults = {}
> {
    data?: () => D
    computed?: C
    methods?: M
    emits?: E
}

export type ComponentPublicInstance<
    P = {}, B = {}, D = {}, C extends ComputedOptions = {},
    M extends MethodOptions = {}, E extends EmitsOptions = {},
    PublicProps = P, Defaults = {}, MakeDefaultsOptional extends boolean = false,
    Options = ComponentOptionsBase<P, B, D, C, M, ComponentOptionsMixin, ComponentOptionsMixin, E>
> = {
    $props: Prettify<PublicProps>
    $data: D
    $options: Options
} & P & B & D

export type CreateComponentPublicInstanceWithMixins<
    P = {}, B = {}, D = {}, C extends ComputedOptions = {},
    M extends MethodOptions = {}, Mixin extends ComponentOptionsMixin = ComponentOptionsMixin,
    Extends extends ComponentOptionsMixin = ComponentOptionsMixin,
    E extends EmitsOptions = {}, PublicProps = P, Defaults = {},
    MakeDefaultsOptional extends boolean = false,
    I extends ComponentInjectOptions = {}, S extends SlotsType = {},
    PublicMixin = IntersectionMixin<Mixin> & IntersectionMixin<Extends>,
    PublicP = UnwrapMixinsType<PublicMixin, 'P'> & EnsureNonVoid<P>,
    PublicB = UnwrapMixinsType<PublicMixin, 'B'> & EnsureNonVoid<B>,
    PublicD = UnwrapMixinsType<PublicMixin, 'D'> & EnsureNonVoid<D>,
    PublicC extends ComputedOptions = UnwrapMixinsType<PublicMixin, 'C'> & EnsureNonVoid<C>,
    PublicM extends MethodOptions = UnwrapMixinsType<PublicMixin, 'M'> & EnsureNonVoid<M>,
    PublicDefaults = UnwrapMixinsType<PublicMixin, 'Defaults'> & EnsureNonVoid<Defaults>
> = ComponentPublicInstance<PublicP, PublicB, PublicD, PublicC, PublicM, E, PublicProps, PublicDefaults, MakeDefaultsOptional>

export type DefineComponent<
    PropsOrPropOptions = {}, RawBindings = {}, D = {}, C extends ComputedOptions = {},
    M extends MethodOptions = {}, Mixin extends ComponentOptionsMixin = ComponentOptionsMixin,
    Extends extends ComponentOptionsMixin = ComponentOptionsMixin,
    E extends EmitsOptions = {}, EE extends string = string
> = {
    new (...args: any[]): CreateComponentPublicInstanceWithMixins<
        Readonly<PropsOrPropOptions>, RawBindings, D, C, M, Mixin, Extends, E
    >
}

export interface FunctionalComponent<P = {}> {
    (props: P): any
    displayName?: string
}

export type ConcreteComponent<P = Data> = ComponentOptionsBase<P, any, any, any, any, any, any, any> | FunctionalComponent<P>
export type Component<P = any> = ConcreteComponent<P> | DefineComponent<P> | string
"#,
    );

    // Library types importing Component
    upsert_non_sfc(
        &host,
        "/node_modules/lib/types.d.ts",
        r#"
import type { Component } from 'vue-runtime'
export interface PrimitiveProps { asChild?: boolean; as?: Component }
export interface AccordionRootProps extends PrimitiveProps {
    collapsible?: boolean
    disabled?: boolean
}
"#,
    );

    upsert_vue(
        &host,
        "/src/Accordion.vue",
        r#"<script setup lang="ts">
import type { AccordionRootProps } from 'lib'
defineProps<AccordionRootProps>()
</script>
<template><div /></template>"#,
    );

    set_dep(
        &host,
        "/src/Accordion.vue",
        "lib",
        "/node_modules/lib/types.d.ts",
    );
    set_dep(
        &host,
        "/node_modules/lib/types.d.ts",
        "vue-runtime",
        "/node_modules/vue-runtime/types.d.ts",
    );

    let meta =
        host.resolve_component_meta("/src/Accordion.vue", crate::types::ProjectionMode::Expanded);
    assert!(meta.is_some(), "deeply generic Accordion should resolve");
}

/// Test 4: Component-meta resolution (`ProjectionMode::Expanded`) for the Accordion
/// pattern. This is the full pipeline that was hanging.
#[test]
fn accordion_pattern_component_meta_expanded_does_not_hang() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });

    upsert_non_sfc(
        &host,
        "/node_modules/@vue/runtime-core.d.ts",
        r#"
export interface FunctionalComponent<P = {}> { (props: P): any }
export interface ComponentOptions<D = any> { data?: () => D }
export type DefineComponent<P = any> = { new (): { $props: P } }
export type ConcreteComponent = FunctionalComponent | ComponentOptions
export type Component = ConcreteComponent | DefineComponent | string
"#,
    );

    upsert_non_sfc(
        &host,
        "/node_modules/reka-ui/types.d.ts",
        r#"
import type { Component } from '@vue/runtime-core'
export interface PrimitiveProps { asChild?: boolean; as?: Component }
export interface AccordionRootProps extends PrimitiveProps {
    collapsible?: boolean
    disabled?: boolean
}
export interface AccordionRootEmits { 'update:modelValue': [value: string | string[]] }
"#,
    );

    upsert_vue(
        &host,
        "/src/Accordion.vue",
        r#"<script setup lang="ts">
import type { AccordionRootProps, AccordionRootEmits } from 'reka-ui'
defineProps<AccordionRootProps>()
defineEmits<AccordionRootEmits>()
</script>
<template><div /></template>"#,
    );

    set_dep(
        &host,
        "/src/Accordion.vue",
        "reka-ui",
        "/node_modules/reka-ui/types.d.ts",
    );
    set_dep(
        &host,
        "/node_modules/reka-ui/types.d.ts",
        "@vue/runtime-core",
        "/node_modules/@vue/runtime-core.d.ts",
    );

    // Full component-meta resolution -- the primary contract is that this
    // does NOT hang. Whether evaluated_types are fully populated depends on
    // cross-file import resolution which may be incomplete without a live
    // workspace resolver.
    let meta =
        host.resolve_component_meta("/src/Accordion.vue", crate::types::ProjectionMode::Expanded);
    assert!(
        meta.is_some(),
        "Accordion component-meta should resolve without hanging"
    );
    let reka_entry = host
        .ensure_indexed_ready("/node_modules/reka-ui/types.d.ts")
        .expect("Accordion resolution should keep the imported reka-ui entry cached");
    assert!(
        reka_entry.shallow_state.has_resolvable_surface(),
        "cached reka-ui module facts should have populated shallow state",
    );
}
