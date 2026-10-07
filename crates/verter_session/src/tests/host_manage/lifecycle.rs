use super::*;

#[test]
fn template_class_facts_exact_domains_aliases_and_fail_closed_unions() {
    let host = make_host();
    let cases = [
        (
            "/Direct.vue",
            "type Variant = 'primary' | 'secondary'; const variant: Variant = 'primary';",
            "variant",
            vec!["primary", "secondary"],
        ),
        (
            "/Alias.vue",
            "type Base = 'primary' | 'secondary'; type Variant = Base; const variant: Variant = 'primary';",
            "variant",
            vec!["primary", "secondary"],
        ),
        (
            "/Mixed.vue",
            "type Variant = 'primary' | string; const variant: Variant = 'primary';",
            "variant",
            vec![],
        ),
        (
            "/Nullable.vue",
            "type Variant = 'primary' | null; const variant: Variant = 'primary';",
            "variant",
            vec![],
        ),
        (
            "/Formatting.vue",
            "const variant:\n  'primary'\n  |\n  'secondary'\n  = 'primary';",
            "variant",
            vec!["primary", "secondary"],
        ),
    ];

    for (canonical, script, expression, expected) in cases {
        upsert_vue(
            &host,
            canonical,
            &format!(
                "<script setup lang=\"ts\">{script}</script><template><div :class=\"{expression}\" /></template>"
            ),
        );
        let analysis = host.get_analysis(canonical).expect("analysis");
        let classes = analysis
            .template
            .expect("template")
            .elements
            .iter()
            .flat_map(|element| element.dynamic_classes.iter().cloned())
            .collect::<Vec<_>>();
        assert_eq!(classes, expected, "wrong exact domain for {canonical}");
    }
}

/// The RULED fail-closed negative of A5-03b: an IMPORTED props type publishes no
/// classes and claims no route, and the boundary is LOCALITY — not a broken
/// fixture.
///
/// An imported `Props` yields NO analyzer prop field at all (`mac.prop_fields`
/// is written only by the analyzer's LOCAL type-registry resolution), so
/// `join_prop_field` returns `Unresolved` and `classify_prop` is never reached.
/// Serving it positively needs cross-file macro prop-field materialisation or a
/// new subject/locator vocabulary — a distinct capability, REJECTED for this
/// campaign by ruling (`T-A7-scope-ruling-RESULT.md` Q2), not deferred.
///
/// The CONTROL is what makes this discriminating: the byte-identical props body,
/// declared LOCALLY in the same test, DOES peel. Without it the negative could
/// pass because the fixture never resolved anything at all.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn template_class_imported_props_type_argument_fails_closed_with_local_control() {
    const PROPS_BODY: &str = "{ variant: Ref<'primary' | 'secondary'> }";
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/src/imported-props.ts",
        &format!("import type {{ Ref }} from 'vue'\nexport type Props = {PROPS_BODY}\n"),
    );
    host.set_import_dependencies(
        "/workspace/src/imported-props.ts",
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );

    // ── The NEGATIVE: `Props` is imported.
    let imported = "/workspace/src/ImportedProps.vue";
    upsert_vue(
        &host,
        imported,
        r#"<script setup lang="ts">
import type { Props } from './imported-props'
const props = defineProps<Props>()
</script><template><div :class="props.variant" /></template>"#,
    );
    host.set_import_dependencies(
        imported,
        vec![
            exact_dependency("vue", "/workspace/node_modules/vue/index.d.ts"),
            exact_dependency("./imported-props", "/workspace/src/imported-props.ts"),
        ],
    );

    let imported_template = host
        .get_analysis(imported)
        .expect("lazy analysis")
        .template
        .expect("template");
    assert!(
        imported_template.elements[0].dynamic_classes.is_empty(),
        "an imported props type must publish no closed subset, got {:?}",
        imported_template.elements[0].dynamic_classes
    );
    let imported_row = template_class_facts_for(&host, imported)
        .rows()
        .iter()
        .find(|row| row.subject.label() == "variant")
        .cloned()
        .expect("the requested subject must produce a row, not vanish");
    assert!(
        imported_row.wrapper.import_provenance.is_none(),
        "an imported props member must claim NO import provenance"
    );
    assert_ne!(
        imported_row.wrapper.role,
        verter_type_expr::ReactiveWrapperRole::Ref,
        "an imported props member must not be granted the `Ref` wrapper role"
    );
    assert!(
        !matches!(
            imported_row.domain,
            verter_type_expr::ClosedLiteralDomain::Strings(_)
        ),
        "the subject domain must not be a closed string set, got {:?}",
        imported_row.domain
    );
    assert!(
        !matches!(
            imported_row.wrapper.inner_domain,
            verter_type_expr::ClosedLiteralDomain::Strings(_)
        ),
        "the wrapper inner domain must not be a closed string set, got {:?}",
        imported_row.wrapper.inner_domain
    );

    // ── The CONTROL: the SAME props body, declared locally, DOES peel. This is
    // what proves the negative above discriminates locality rather than passing
    // vacuously on a broken fixture.
    let local = "/workspace/src/LocalPropsControl.vue";
    upsert_vue(
        &host,
        local,
        &format!(
            r#"<script setup lang="ts">
import type {{ Ref }} from 'vue'
type Props = {PROPS_BODY}
const props = defineProps<Props>()
</script><template><div :class="props.variant" /></template>"#
        ),
    );
    host.set_import_dependencies(
        local,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    let local_template = host
        .get_analysis(local)
        .expect("lazy analysis")
        .template
        .expect("template");
    assert_eq!(
        local_template.elements[0].dynamic_classes,
        ["primary", "secondary"],
        "CONTROL: the byte-identical props body declared LOCALLY must peel — \
         otherwise the imported negative above proves nothing"
    );
    let local_row = template_class_facts_for(&host, local)
        .rows()
        .iter()
        .find(|row| row.subject.label() == "variant")
        .cloned()
        .expect("the control subject must join");
    assert_eq!(
        local_row.wrapper.role,
        verter_type_expr::ReactiveWrapperRole::Ref,
        "CONTROL: the local arm must be granted the exact `Ref` role"
    );
    assert_eq!(
        local_row
            .wrapper
            .import_provenance
            .as_ref()
            .expect("CONTROL: the local arm must publish an exact route")
            .terminal_import_source
            .as_ref(),
        "vue"
    );
}

/// The FAIL-CLOSED boundary of the vocabulary gate: a package wrapper authored
/// as a TRANSPARENT ALIAS is not recognized by its own name.
///
/// The shared authored-route walk follows a forwarding declaration to its
/// terminal, so `type Reactive<T> = Unwrapped<T>` in the package routes past
/// `Reactive` and terminates at `Unwrapped`, which is outside the closed
/// vocabulary. This is landed shared-route-walk semantics — the template-class
/// path behaves identically — and it fails CLOSED: no role is guessed from the
/// authored spelling `Reactive`, and no provenance is published. This test
/// exists so the boundary is visible instead of being hidden by the fixture
/// shape; a future change to where the route walk stops inverts it.
#[test]
fn return_wrapper_role_fails_closed_for_a_transparent_alias_wrapper() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        "export interface Unwrapped<T> { __u: T }\nexport type Reactive<T> = Unwrapped<T>\n",
    );
    let canonical = "/workspace/src/transparent.ts";
    upsert_ts(
        &host,
        canonical,
        "import type { Reactive } from 'vue'\n\
         export function getValue(): Reactive<number> { return null as never; }\n",
    );
    host.set_import_dependencies(
        canonical,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    let (role, provenance) = return_wrapper_role_for(&host, canonical, "getValue");
    assert_eq!(
        role,
        verter_type_expr::ReactiveWrapperRole::None,
        "the route terminates outside the closed vocabulary, so the wrapper is \
         proven absent rather than claimed from the authored name"
    );
    assert!(provenance.is_none());
}

/// An explicit overlay whose publication identity is incomplete is a real
/// request-bound refusal. Every sibling adapter must preserve that refusal
/// even when a complete base artifact for the same canonical is available.
#[test]
fn explicit_overlay_materialization_refusal_fails_closed_at_request_boundaries() {
    use crate::resolver_core::{ComponentMetaRequestHost, SessionResolverContext};
    use crate::session_view::SessionView;
    use verter_session_query::{QueryHostError, QueryHostPort};
    use verter_type_expr::locators::{
        AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
    };

    let canonical = "/workspace/refusal.ts";
    let host = Arc::new(make_host());
    upsert_non_sfc(
        &host,
        canonical,
        "export interface Ready { value: string }\n",
    );
    assert!(host.ensure_indexed_ready_serve(canonical).is_some());
    let base_store = host.resolver_store_view_read().into_owned_view();
    assert!(host
        .capture_component_meta_inputs(canonical, &base_store)
        .is_some());
    assert!(host.routed_shallow_state(canonical).is_some());

    let base = crate::session_view::HostView::new(Arc::clone(&host));
    let hash = base
        .content_hash_for(canonical)
        .expect("base fixture has current content");
    let view = DecliningOverlayView {
        base,
        canonical: canonical.to_string(),
        hash,
    };
    let store_view = host
        .resolver_store_view_read()
        .into_cold_seed_view()
        .with_session_overlay(&host, &view);
    let completion = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = SessionResolverContext::from_cold_seed(&host, &view, &store_view, completion);

    assert!(ctx.ensure_indexed_ready_serve(canonical).is_none());
    assert!(host
        .capture_component_meta_inputs_with_view(canonical, &view)
        .is_none());
    assert!(host
        .routed_shallow_state_with_view(canonical, Some(&view))
        .is_none());

    let locator = AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("Ready"),
            space: LocatorSymbolSpace::Type,
        },
        path: Arc::from([]),
    });
    let port = crate::query_host_port::SessionQueryHostPort::new(&ctx);
    let answer = port.lower_authored_body(&locator);
    assert!(matches!(answer.outcome, Err(QueryHostError::UnknownFile)));

    assert!(host.ensure_indexed_ready_serve(canonical).is_some());
    assert!(host
        .capture_component_meta_inputs(canonical, &base_store)
        .is_some());
    assert!(host.routed_shallow_state(canonical).is_some());

    // Mutation controls: replace any of the three explicit-overlay branches
    // with a `.or_else(...)` base fallback; the corresponding `is_none`
    // assertion above becomes RED while the base preconditions remain green.
}

/// A component's fallthrough nodes are keyed by the component, so they can
/// never be read again once it is closed or deleted: closing and deleting
/// every component returns the fallthrough node cache to where it started.
///
/// Discriminating: the cache used to keep every component's nodes for the
/// life of the host, one set per component ever resolved, so the count grew
/// with the number of components the session had seen.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn closing_or_deleting_components_releases_their_fallthrough_nodes() {
    const OWNERS: usize = 32;

    fn source(n: usize) -> String {
        format!(
            r#"<script setup lang="ts">
defineProps<{{ label?: string }}>()
const n = {n}
</script>
<template><div :title="label">{{{{ n }}}}</div></template>"#
        )
    }
    let nodes = |host: &VerterHost| host.retention_snapshot().fallthrough_nodes;

    let host = make_host();
    // The project's intrinsic `div` surface is shared by every component and
    // stays; warm it once so the baseline includes it.
    upsert_vue(&host, "/src/Warm.vue", &source(0));
    assert!(host.resolve_fallthrough_surface("/src/Warm.vue").is_some());
    host.evict("/src/Warm.vue");
    let baseline = nodes(&host);

    let owners: Vec<String> = (0..OWNERS).map(|n| format!("/src/C{n}.vue")).collect();
    for (n, owner) in owners.iter().enumerate() {
        upsert_vue(&host, owner, &source(n + 1));
        assert!(host.resolve_fallthrough_surface(owner).is_some());
    }
    let resolved = nodes(&host);
    assert!(
        resolved >= baseline + OWNERS,
        "fixture: every component warms its own nodes ({baseline} -> {resolved})"
    );

    for (n, owner) in owners.iter().enumerate() {
        if n % 2 == 0 {
            host.evict(owner);
        } else {
            let _ = host.remove(owner);
        }
    }
    assert_eq!(
        nodes(&host),
        baseline,
        "closed and deleted components keep no fallthrough nodes"
    );
}

/// `VerterHost::evict` (the `did_close` path) releases every semantic node
/// scoped to the closed document, and the DISK reload path stays intact:
/// `ensure_loaded` brings the file back from the workspace, and a consumer
/// edit that forces re-resolution re-lowers it under fresh ids, yielding
/// the same component meta as before the close. (The reopen-by-upsert
/// variant lives in `component_meta_caches_tests`.)
///
/// On the old code `evict` never reached the semantic graph: every node
/// scoped to `/src/types/icon.ts` stayed live after the close, so the
/// `== 0` assertion failed and `node_count` did not drop.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_evict_releases_the_closed_documents_nodes_and_the_disk_reload_reinterns() {
    use verter_type_engine::semantic_query::SemanticNodeId;

    const CONSUMER_V1: &str = r#"<script setup lang="ts">
import type { IconProps } from './types/icon'
defineProps<IconProps>()
</script>
<template><div /></template>"#;
    const CONSUMER_V2: &str = r#"<script setup lang="ts">
import type { IconProps } from './types/icon'
// edited after the dependency was closed
defineProps<IconProps>()
</script>
<template><div /></template>"#;

    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/Consumer.vue", CONSUMER_V1);
    ws.inject_file(
        "/src/types/icon.ts",
        "export interface IconProps { name: string; size: number }\n",
    );
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(host.ensure_loaded("/src/Consumer.vue"));
    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types/icon", "/src/types/icon.ts")],
    );
    let meta_before = host
        .get_component_meta("/src/Consumer.vue")
        .expect("component meta before the close");
    assert!(
        meta_before.props.iter().any(|prop| prop.name == "name"),
        "fixture: the consumer's props resolve through icon.ts, got {:?}",
        meta_before.props
    );

    let graph = host.project_type_store().semantic_graph();
    let live_scoped_to = |canonical: &str| {
        (0..graph.node_slot_count() as u64)
            .filter(|ordinal| {
                graph
                    .node_scope(SemanticNodeId(*ordinal))
                    .and_then(|scope| scope.canonical_file())
                    .is_some_and(|file| file.as_ref() == canonical)
            })
            .count()
    };
    assert!(
        live_scoped_to("/src/types/icon.ts") > 0,
        "fixture: resolving the consumer lowers declaration nodes scoped to icon.ts"
    );
    let live_before = graph.node_count();

    host.evict("/src/types/icon.ts");

    assert_eq!(
        live_scoped_to("/src/types/icon.ts"),
        0,
        "the close releases every node scoped to the closed document"
    );
    assert!(
        graph.node_count() < live_before,
        "the live node count drops on close ({} -> {})",
        live_before,
        graph.node_count()
    );

    // The disk reload: the closed document comes back from the workspace.
    assert!(
        host.ensure_loaded("/src/types/icon.ts"),
        "the closed document reloads from the workspace"
    );
    // A consumer edit forces re-resolution through the reloaded file.
    upsert_vue(&host, "/src/Consumer.vue", CONSUMER_V2);
    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types/icon", "/src/types/icon.ts")],
    );
    let meta_after = host
        .get_component_meta("/src/Consumer.vue")
        .expect("component meta after the close + reload");
    // The stable semantic projection: prop identity plus the RESOLVED type
    // source. The consumer's own provenance (content hash, source
    // generation, artifact token) legitimately changes with its edit and
    // is outside the comparison.
    let published_sources =
        |meta: &verter_session_query::analysis::component_meta::ComponentMetaAnalysis| {
            meta.props
                .iter()
                .map(|prop| {
                    (
                        prop.name.clone(),
                        prop.required,
                        format!("{:?}", prop.callable_role),
                        format!("{:?}", prop.publication.authority().source()),
                    )
                })
                .collect::<Vec<_>>()
        };
    assert_eq!(
        published_sources(&meta_after),
        published_sources(&meta_before),
        "the re-lowered dependency yields the same resolved prop sources"
    );
    assert!(
        live_scoped_to("/src/types/icon.ts") > 0,
        "the reload re-interned the closed document's declarations under fresh ids"
    );
}

/// The consumer itself is closed and reopened UNCHANGED: the byte-identical
/// reload serves the same indexed artifact, whose macro mirror lowered the
/// `defineProps<IconProps>()` carrier before the close released it. When the
/// imported dependency then changes, the cold component-meta demand re-reads
/// that mirror and must get a live carrier: the props resolve through the
/// dependency's new content exactly as they did before the close.
///
/// Discriminating: the write-once mirror cell handed the cold demand the
/// released carrier, which reads as an unresolved value, so the props the
/// dependency declares were lost.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_evict_of_an_unchanged_consumer_keeps_its_macro_props_resolving_after_a_dependency_edit() {
    const CONSUMER: &str = r#"<script setup lang="ts">
import type { IconProps } from './types/icon'
defineProps<IconProps>()
</script>
<template><div /></template>"#;

    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/Consumer.vue", CONSUMER);
    ws.inject_file(
        "/src/types/icon.ts",
        "export interface IconProps { name: string; size: number }\n",
    );
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    let prop_names = |host: &VerterHost| {
        let mut names = host
            .get_component_meta("/src/Consumer.vue")
            .expect("component meta")
            .props
            .iter()
            .map(|prop| prop.name.clone())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    assert!(host.ensure_loaded("/src/Consumer.vue"));
    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types/icon", "/src/types/icon.ts")],
    );
    assert_eq!(
        prop_names(&host),
        vec!["name".to_string(), "size".to_string()],
        "fixture: the consumer's props resolve through icon.ts"
    );

    // Close the consumer and reload the same bytes from disk.
    host.evict("/src/Consumer.vue");
    assert!(host.ensure_loaded("/src/Consumer.vue"));
    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types/icon", "/src/types/icon.ts")],
    );

    // The dependency changes, so the consumer's component meta recomputes.
    upsert_ts(
        &host,
        "/src/types/icon.ts",
        "export interface IconProps { name: string; size: number; color: string }\n",
    );
    assert_eq!(
        prop_names(&host),
        vec!["color".to_string(), "name".to_string(), "size".to_string()],
        "the reopened consumer's props resolve through the dependency's new content"
    );
}

/// The LSP churn lane, in-process: the dependency `/src/types/icon.ts` is
/// opened with UNIQUE content each cycle, the still-open consumer
/// resolves its exported surface through the real component-meta query
/// path (the shape a hover on the consumer drives), and the dependency is
/// closed (`VerterHost::evict`). After the first cycle the live substrate
/// at the "resolved" point — memo entries, live nodes, shape entries —
/// must be exactly the first cycle's: every entry a cycle mints for the
/// closed content must be released with it. On failure the message names
/// the memo families that grew.
///
/// On the old code (release without the content-bound drain) the
/// consumer-side entries produced against the dependency's content
/// survived every close — `memo_entry_count` grew by a fixed amount per
/// cycle — so the flat assertion failed on cycle 1 and named the family.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_evict_churn_keeps_memo_entries_flat_across_unique_content_cycles() {
    const CONSUMER: &str = r#"<script setup lang="ts">
import type { IconProps } from './types/icon'
defineProps<IconProps>()
</script>
<template><div /></template>"#;
    let icon_source = |cycle: usize| {
        format!("export interface IconProps {{ name: string; size: number; v{cycle}: boolean }}\n")
    };

    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/Consumer.vue", CONSUMER);
    ws.inject_file("/src/types/icon.ts", &icon_source(0));
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(host.ensure_loaded("/src/Consumer.vue"));
    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types/icon", "/src/types/icon.ts")],
    );
    let store = host.project_type_store();
    let graph = store.semantic_graph();

    let mut first: Option<ChurnCycleCounters> = None;
    // The live nodes that exist before any cycle (the consumer's own
    // substrate); every other live node at a later cycle's start was
    // minted by an earlier cycle and survived that cycle's close.
    let persistent: std::collections::HashSet<u64> = (0..graph.node_slot_count() as u64)
        .filter(|ordinal| {
            graph.node_is_live(verter_type_engine::semantic_query::SemanticNodeId(*ordinal))
        })
        .collect();
    for cycle in 0..20usize {
        let slots_at_cycle_start = graph.node_slot_count();
        // Open the dependency with unique content (the editor buffer).
        upsert_ts(&host, "/src/types/icon.ts", &icon_source(cycle));
        // The consumer resolves the dependency's exported surface.
        let meta = host
            .get_component_meta("/src/Consumer.vue")
            .expect("component meta resolves each cycle");
        assert!(
            meta.props
                .iter()
                .any(|prop| prop.name == format!("v{cycle}")),
            "cycle {cycle}: the consumer sees THIS cycle's content, got {:?}",
            meta.props.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
        let resolved = (
            graph.memo_entry_count(),
            graph.node_count(),
            store.shape_cache_db().live_count(),
            graph.memo_entry_counts_by_family(),
            graph.node_slot_count(),
        );
        // Cycle 0 runs against a world that never closed the dependency;
        // from cycle 1 on every cycle resolves through the reload path a
        // close leaves behind (the consumer's declaration placeholder for
        // the reloaded file is minted once more). Steady state is cycle 1,
        // and every later cycle must equal it exactly.
        match &first {
            None => {
                if cycle >= 1 {
                    first = Some(resolved);
                }
            }
            Some(first) => {
                assert_eq!(
                    resolved.0, first.0,
                    "cycle {cycle}: memo entries grew — by family, first {:?} vs now {:?}",
                    first.3, resolved.3
                );
                // Name every live node minted after the first cycle's id space
                // that survived a close: `(id, scope, payload)`.
                let survivors: Vec<String> = (0..slots_at_cycle_start as u64)
                    .filter(|ordinal| !persistent.contains(ordinal))
                    .map(verter_type_engine::semantic_query::SemanticNodeId)
                    .filter(|id| graph.node_is_live(*id))
                    .map(|id| {
                        format!(
                            "{id:?} scope={:?} payload={:?}",
                            graph.node_scope(id).map(|scope| scope.canonical_file()),
                            graph.node_data(id)
                        )
                    })
                    .collect();
                let minted_now: Vec<String> = (slots_at_cycle_start as u64
                    ..graph.node_slot_count() as u64)
                    .map(verter_type_engine::semantic_query::SemanticNodeId)
                    .filter(|id| graph.node_is_live(*id))
                    .map(|id| {
                        format!(
                            "{id:?} scope={:?} payload={:?}",
                            graph.node_scope(id).map(|scope| scope.canonical_file()),
                            graph.node_data(id)
                        )
                    })
                    .collect();
                assert_eq!(
                    resolved.1,
                    first.1,
                    "cycle {cycle}: live nodes grew (slots {} -> {}); nodes minted in EARLIER \
                     cycles that survived a close: {survivors:#?}; minted THIS cycle: \
                     {minted_now:#?}",
                    first.4,
                    graph.node_slot_count()
                );
                assert_eq!(resolved.2, first.2, "cycle {cycle}: shape entries grew");
            }
        }
        // Close the dependency; the consumer stays open.
        host.evict("/src/types/icon.ts");
    }
}

/// The dx-harness churn lane, in-process, on the lane's own carrier shape:
/// the churned document is a Vue SFC (`defineProps<ChurnProps>()`, derived
/// script bindings, a template using them) whose headline line is made
/// UNIQUE each cycle; the still-open consumer imports it as a child
/// component. Per cycle: open (upsert) the unique content, resolve the
/// churned document's own component meta (what a change-triggered
/// diagnostic / hover computes) and the consumer's, then close
/// (`VerterHost::evict`). After the first cycle the live substrate at the
/// resolved point must be exactly the first cycle's; on failure the
/// message names the memo families that grew and the nodes that survived.
///
/// On the old code (release without the content-bound drain) the entries
/// minted against the closed document's content survived every close and
/// `memo_entry_count` grew by a fixed amount per cycle, so the flat
/// assertion failed on cycle 1 and named the family.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_evict_churn_vue_carrier_keeps_memo_entries_flat_across_unique_content_cycles() {
    const CONSUMER: &str = r#"<script setup lang="ts">
import Churn from './Churn.vue'
const consumerLabel = "consumer";
const consumerLength = consumerLabel.length;
</script>
<template>
  <main :data-len="consumerLength">
    <Churn :churn-label="consumerLabel" />
  </main>
</template>"#;
    let churn_source = |cycle: usize| {
        format!(
            r#"<script setup lang="ts">
interface ChurnProps {{
  churnLabel: string;
  churnField0?: string;
  churnField1?: string;
}}
const props = defineProps<ChurnProps>();
const emit = defineEmits<{{ (e: "churn", value: string): void }}>();
const churnLocal0 = `c0:${{props.churnLabel}}:${{props.churnField0 ?? ""}}`;
const churnLength0 = churnLocal0.length;
const churnHeadline = props.churnLabel.toUpperCase() + "{cycle}";
function fireChurn() {{
  emit("churn", churnHeadline);
}}
</script>

<template>
  <section>
    <h1 :title="churnHeadline">{{{{ churnHeadline }}}}</h1>
    <span :title="churnLocal0" :data-len="churnLength0">{{{{ churnLocal0 }}}}</span>
    <button @click="fireChurn">go</button>
  </section>
</template>
"#
        )
    };

    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/ChurnConsumer.vue", CONSUMER);
    ws.inject_file("/src/Churn.vue", &churn_source(0));
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(host.ensure_loaded("/src/ChurnConsumer.vue"));
    host.set_import_dependencies(
        "/src/ChurnConsumer.vue",
        vec![exact_dependency("./Churn.vue", "/src/Churn.vue")],
    );
    let store = host.project_type_store();
    let graph = store.semantic_graph();

    let mut first: Option<ChurnCycleCounters> = None;
    for cycle in 0..20usize {
        let slots_at_cycle_start = graph.node_slot_count();
        upsert_vue(&host, "/src/Churn.vue", &churn_source(cycle));
        let churn_meta = host
            .get_component_meta("/src/Churn.vue")
            .expect("the churned document's meta resolves each cycle");
        assert!(
            churn_meta
                .props
                .iter()
                .any(|prop| prop.name == "churnLabel"),
            "cycle {cycle}: the churned document publishes its props"
        );
        // The other semantic entry points the language server drives for an
        // open document on change / hover / diagnostics.
        let _ = host.get_public_api("/src/Churn.vue");
        let _ = host.get_script_ingress("/src/Churn.vue");
        let _consumer_meta = host
            .get_component_meta("/src/ChurnConsumer.vue")
            .expect("the consumer's meta resolves each cycle");
        let _ = host.get_public_api("/src/ChurnConsumer.vue");
        let _ = host.get_script_ingress("/src/ChurnConsumer.vue");
        let resolved = (
            graph.memo_entry_count(),
            graph.node_count(),
            store.shape_cache_db().live_count(),
            graph.memo_entry_counts_by_family(),
            graph.node_slot_count(),
        );
        match &first {
            None => first = Some(resolved),
            Some(first) => {
                assert_eq!(
                    resolved.0, first.0,
                    "cycle {cycle}: memo entries grew — by family, first {:?} vs now {:?}",
                    first.3, resolved.3
                );
                let survivors: Vec<String> = (first.4 as u64..slots_at_cycle_start as u64)
                    .map(verter_type_engine::semantic_query::SemanticNodeId)
                    .filter(|id| graph.node_is_live(*id))
                    .map(|id| {
                        format!(
                            "{id:?} scope={:?} payload={:?}",
                            graph.node_scope(id),
                            graph.node_data(id)
                        )
                    })
                    .collect();
                assert_eq!(
                    resolved.1,
                    first.1,
                    "cycle {cycle}: live nodes grew (slots {} -> {}); nodes minted in EARLIER \
                     cycles that survived a close: {survivors:#?}",
                    first.4,
                    graph.node_slot_count()
                );
                assert_eq!(resolved.2, first.2, "cycle {cycle}: shape entries grew");
            }
        }
        host.evict("/src/Churn.vue");
    }
}

/// A close that lands while a computation is in flight must not change any
/// node that computation can read: the release is queued, every node of the
/// closed document keeps its payload (the SAME payload on every read), and
/// the release is applied the moment the last computation ends.
///
/// Discriminating: with the release applied inline at `evict` (the previous
/// design), the closed document's nodes read as the released placeholder
/// while the guard is still held — the exact switch that made a carrier
/// normaliser, midway through two reads of one node, panic on a worker
/// thread and stall the scheduler in the WSP6 churn lane.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_close_during_a_computation_defers_the_node_release_until_it_ends() {
    let (_ws, host) = activity_gate_fixture();
    let graph = host.project_type_store().semantic_graph();
    let ids = live_node_ids_scoped_to(&host, "/src/types/icon.ts");
    assert!(!ids.is_empty(), "fixture: icon.ts has live nodes");
    let before: Vec<_> = ids
        .iter()
        .map(|id| graph.node_data(*id).expect("live node"))
        .collect();

    let computation = host.semantic_activity();
    host.evict("/src/types/icon.ts");

    assert_eq!(
        host.project_type_store().deferred_release_count(),
        1,
        "the close is queued while a computation is in flight"
    );
    for (id, payload) in ids.iter().zip(&before) {
        assert!(
            graph.node_is_live(*id),
            "node {id:?} stays live until the computation ends"
        );
        let now = graph.node_data(*id).expect("still readable");
        assert!(
            Arc::ptr_eq(&now, payload),
            "node {id:?} reads the same payload it read before the close"
        );
    }

    drop(computation);

    assert_eq!(
        host.project_type_store().deferred_release_count(),
        0,
        "the last computation to end applies the queued release"
    );
    assert!(
        live_node_ids_scoped_to(&host, "/src/types/icon.ts").is_empty(),
        "every node of the closed document is released once nothing is in flight"
    );
}

/// With no computation in flight (a batch host, a test), the close applies
/// the release inline, exactly as before.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_close_with_nothing_in_flight_releases_inline() {
    let (_ws, host) = activity_gate_fixture();
    assert!(!live_node_ids_scoped_to(&host, "/src/types/icon.ts").is_empty());
    host.evict("/src/types/icon.ts");
    assert_eq!(host.project_type_store().deferred_release_count(), 0);
    assert!(live_node_ids_scoped_to(&host, "/src/types/icon.ts").is_empty());
}

/// The queued release takes only what the CLOSED content interned: nodes the
/// reload interned for the same document while the release was waiting
/// (ids at or past the watermark taken at the close) stay live, and the
/// consumer still resolves through them.
///
/// Discriminating: a release that re-scanned by canonical at apply time (no
/// watermark) would also take the reloaded document's fresh nodes, leaving
/// the live consumer pointing at released ids.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_deferred_release_keeps_nodes_interned_after_the_close() {
    const CONSUMER_V2: &str = r#"<script setup lang="ts">
import type { IconProps } from './types/icon'
// edited after the dependency was closed
defineProps<IconProps>()
</script>
<template><div /></template>"#;
    let (_ws, host) = activity_gate_fixture();
    let closed = live_node_ids_scoped_to(&host, "/src/types/icon.ts");
    assert!(!closed.is_empty());

    let computation = host.semantic_activity();
    host.evict("/src/types/icon.ts");
    // The reload and a consumer edit re-lower icon.ts while the release waits.
    assert!(host.ensure_loaded("/src/types/icon.ts"));
    upsert_vue(&host, "/src/Consumer.vue", CONSUMER_V2);
    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types/icon", "/src/types/icon.ts")],
    );
    let meta = host
        .get_component_meta("/src/Consumer.vue")
        .expect("component meta after the reload");
    assert!(meta.props.iter().any(|prop| prop.name == "name"));
    let reloaded: Vec<_> = live_node_ids_scoped_to(&host, "/src/types/icon.ts")
        .into_iter()
        .filter(|id| !closed.contains(id))
        .collect();
    assert!(
        !reloaded.is_empty(),
        "fixture: the reload interned fresh icon.ts nodes while the release waited"
    );

    drop(computation);

    let graph = host.project_type_store().semantic_graph();
    for id in &closed {
        assert!(
            !graph.node_is_live(*id),
            "closed-content node {id:?} is released"
        );
    }
    for id in &reloaded {
        assert!(
            graph.node_is_live(*id),
            "node {id:?} interned after the close survives the deferred release"
        );
    }
}

/// The caches a document's own content keys (resolved-import facts, the
/// resolver's component-meta states, the overlay source registration) follow
/// the current content across edits and are released by the close: the
/// retention snapshot's counts hold flat over edits and drop at the close.
/// Discriminating: each of the three gained one key per edit before (the
/// WSP6 churn lane's heap profile attributed the residual per-cycle growth
/// to them).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn edits_keep_the_content_keyed_caches_flat_and_a_close_releases_them() {
    let (_ws, host) = activity_gate_fixture();
    let edited = |edit: usize| {
        format!(
            "<script setup lang=\"ts\">
import type {{ IconProps }} from './types/icon'
             // edit {edit}
defineProps<IconProps>()
</script>
<template><div /></template>"
        )
    };
    let query = |host: &VerterHost| {
        host.set_import_dependencies(
            "/src/Consumer.vue",
            vec![exact_dependency("./types/icon", "/src/types/icon.ts")],
        );
        let meta = host
            .get_component_meta("/src/Consumer.vue")
            .expect("component meta");
        assert!(meta.props.iter().any(|prop| prop.name == "name"));
    };
    // Two edits fill the window of two contents each cache keeps; the
    // counts must not move from there on.
    upsert_vue(&host, "/src/Consumer.vue", &edited(0));
    query(&host);
    upsert_vue(&host, "/src/Consumer.vue", &edited(1));
    query(&host);
    let baseline = host.retention_snapshot();
    for edit in 2..=5 {
        upsert_vue(&host, "/src/Consumer.vue", &edited(edit));
        query(&host);
        let after = host.retention_snapshot();
        assert_eq!(
            after.resolved_import_facts, baseline.resolved_import_facts,
            "edit {edit}: resolved-import facts follow the current content"
        );
        assert_eq!(
            after.component_meta_states, baseline.component_meta_states,
            "edit {edit}: component-meta states follow the current view"
        );
        assert_eq!(
            after.registered_sources, baseline.registered_sources,
            "edit {edit}: a superseded overlay registration is retracted"
        );
    }
    host.evict("/src/Consumer.vue");
    let closed = host.retention_snapshot();
    assert!(
        closed.resolved_import_facts < baseline.resolved_import_facts,
        "the close releases the document's resolved-import facts ({} -> {})",
        baseline.resolved_import_facts,
        closed.resolved_import_facts
    );
    assert!(
        closed.component_meta_states < baseline.component_meta_states,
        "the close releases the document's component-meta states ({} -> {})",
        baseline.component_meta_states,
        closed.component_meta_states
    );
    assert!(
        closed.registered_sources <= baseline.registered_sources,
        "the close retracts the document's overlay registration ({} -> {})",
        baseline.registered_sources,
        closed.registered_sources
    );
}

/// A whole-host reset — `close()`, or `set_workspace()` onto another
/// workspace — drops the component-meta view bookkeeping together with the
/// states it tracks.
///
/// Discriminating: the reset reached `UnifiedResolverRuntime::clear_caches`,
/// which cleared the states but kept every document's fingerprint queue, so a
/// host reused across workspaces kept the old canonicals for its life.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_whole_host_reset_drops_the_component_meta_view_bookkeeping() {
    let (_ws, host) = activity_gate_fixture();
    assert!(
        host.resolver_runtime()
            .component_meta_view_bookkeeping_len()
            > 0,
        "fixture: the cold publication noted its view"
    );
    host.close();
    assert_eq!(host.retention_snapshot().component_meta_states, 0);
    assert_eq!(
        host.resolver_runtime()
            .component_meta_view_bookkeeping_len(),
        0,
        "close() forgets the view bookkeeping"
    );

    let (_ws, host) = activity_gate_fixture();
    assert!(
        host.resolver_runtime()
            .component_meta_view_bookkeeping_len()
            > 0
    );
    host.set_workspace(Arc::new(CountingWorkspace::new()));
    assert_eq!(
        host.resolver_runtime()
            .component_meta_view_bookkeeping_len(),
        0,
        "set_workspace() forgets the view bookkeeping"
    );
}
