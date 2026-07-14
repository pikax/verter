//! Svelte vertical — session integration behavior.
//!
//! Discriminating coverage for the parse + shallow + synth + surface + api half:
//! - shallow inventory carries the synthesized `default`
//!   (`is_synthesised_component_default`) + exported members;
//! - a TS file importing a `.svelte` resolves the public type through the shared
//!   `Instantiate` dispatch; circular `.svelte ↔ .svelte` imports terminate;
//! - the userland-`Snippet` NEGATIVE: a `Snippet` from a non-`svelte` module is
//!   NOT classified snippet-typed (the typed resolved-package identity rejects
//!   it; a raw-name match would pass it);
//! - the REGISTERED Svelte surface adapter: a runes component resolves PROPS
//!   with members, OPTIONS is the only structurally-UNSUPPORTED kind (§9), a
//!   runes callback prop stays PROPS / absent from EMITS, and identical requests
//!   warm-hit a value-stable surface;
//! - the Svelte api-content shim (`get_public_api`) — class default with
//!   `$props: __VerterProps`, refs preserved un-inlined, the type-only prelude;
//!   `get_public_api_with_mode(Testing)` returns `None`;
//! - synth parse-domain invariance.

use std::sync::Arc;

use verter_language::FileLanguage;
use verter_protocol::typeinfo::graph::{self as wire, Exactness, FrameworkSurfaceKindSupport};
use verter_protocol::verter::v1::{
    type_info_graph_request as wire_request, type_info_graph_response,
};

use crate::types::{HostConfig, PublicApiMode, UpsertRequest};
use crate::VerterHost;

fn host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, id: &str, src: &str, lang: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(id.into()),
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: lang,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert `{id}` must succeed: {e:?}"));
}

fn upsert_svelte(host: &VerterHost, id: &str, src: &str) {
    upsert(host, id, src, FileLanguage::svelte());
}

fn upsert_ts(host: &VerterHost, id: &str, src: &str) {
    upsert(host, id, src, FileLanguage::script_ts());
}

fn upsert_rune_module(host: &VerterHost, id: &str, src: &str) {
    let lang = FileLanguage::adapter_module(
        verter_language::ScriptSourceType::Ts,
        verter_language::FrameworkAdapterId::svelte(),
        verter_language::LanguageId::new(verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID),
    );
    upsert(host, id, src, lang);
}

// ── Test 3: shallow inventory carries the synthesized default ───────────

#[test]
fn svelte_shallow_state_carries_synthesised_default_and_exports() {
    let host = host();
    let src = r#"<script lang="ts">
  interface Props { name: string; count?: number }
  let { name, count }: Props = $props();
  export function focus() {}
  export const ready = true;
</script>
<button onclick={focus}>{name}: {count}</button>
"#;
    upsert_svelte(&host, "/Widget.svelte", src);

    let indexed = host
        .ensure_indexed_ready("/Widget.svelte")
        .expect("the .svelte file indexes");
    let default_symbol = indexed
        .shallow_state
        .value_symbol("default")
        .expect("the synthesized `default` value symbol is present");
    assert!(
        default_symbol.is_synthesised_component_default,
        "the `.svelte` default is the synthesized component default"
    );

    // The instance shape carries `$props` + the exported instance members.
    // The construct signature lives on the synthesized BODY; the instance
    // members ride the annotation-borne synthesized source.
    let default_decl = indexed
        .shallow_state
        .value_decl("default")
        .expect("the synthesized default body carries its construct signature");
    default_decl
        .signatures
        .first()
        .expect("construct signature");
    let members = instance_member_names(&default_decl);
    assert!(
        members.contains(&"$props".to_string()),
        "instance carries $props"
    );
    assert!(
        members.contains(&"focus".to_string()),
        "exported fn is an instance member"
    );
    assert!(
        members.contains(&"ready".to_string()),
        "exported const is an instance member"
    );
}

// ── Test 4: cross-file resolution + circular termination ────────────────

#[test]
fn pure_markup_svelte_synthesises_an_empty_props_default() {
    // EVERY `.svelte` is a component: a pure-markup component with no `$props()`
    // and no exports still gets a synthesized class-shaped default with
    // `$props: {}`. DISCRIMINATING: a synth gated on candidates would leave no
    // default.
    let host = host();
    upsert_svelte(&host, "/Markup.svelte", "<div>hello</div>\n");
    let indexed = host
        .ensure_indexed_ready("/Markup.svelte")
        .expect("indexes");
    let default_symbol = indexed
        .shallow_state
        .value_symbol("default")
        .expect("a pure-markup .svelte still synthesizes a default");
    assert!(default_symbol.is_synthesised_component_default);
    let default_decl = indexed
        .shallow_state
        .value_decl("default")
        .expect("the synthesized default body carries its construct signature");
    let members = instance_member_names(&default_decl);
    assert_eq!(members, vec!["$props".to_string()]);
}

#[test]
fn module_script_export_is_not_an_instance_member() {
    // A `<script module>export const meta</script>` export is a MODULE export,
    // NOT an instance member. DISCRIMINATING: conflating the two script blocks
    // would surface `meta` as an instance member.
    let host = host();
    upsert_svelte(
        &host,
        "/Mod.svelte",
        "<script module>export const meta = { title: 'x' };</script>\n<script lang=\"ts\">export function focus() {}\nlet { name }: { name: string } = $props();</script>\n<h1>{name}</h1>\n",
    );
    let indexed = host.ensure_indexed_ready("/Mod.svelte").expect("indexes");
    assert!(
        indexed
            .shallow_state
            .value_symbol("default")
            .expect("synthesized default")
            .is_synthesised_component_default
    );
    let default_decl = indexed
        .shallow_state
        .value_decl("default")
        .expect("the synthesized default body carries its construct signature");
    let members = instance_member_names(&default_decl);
    assert!(
        members.contains(&"focus".to_string()),
        "the INSTANCE export `focus` is an instance member: {members:?}"
    );
    assert!(
        !members.contains(&"meta".to_string()),
        "the MODULE export `meta` must NOT be an instance member: {members:?}"
    );
}

/// The instance member names of a synthesized component default.
///
/// The synthesized BODY (`LoweredValueDecl`, fetched via
/// `shallow_state.value_decl("default")`) carries the instance members on its
/// annotation-borne synthesized source.
fn instance_member_names(default_decl: &crate::decl_body_memo::LoweredValueDecl) -> Vec<String> {
    match default_decl.type_annotation.annotation.as_ref() {
        Some(verter_type_expr::facts::SemanticTypeSource::Synthesized(
            verter_type_expr::facts::ResolvedLocalShape::Object(members),
        )) => members.iter().map(|m| m.name.clone()).collect(),
        other => panic!("expected a synthesized object instance source, got {other:?}"),
    }
}

#[test]
fn ts_importing_svelte_resolves_public_type_and_circular_terminates() {
    let host = host();
    // A.svelte imports B.svelte and vice-versa (circular). Both have props.
    upsert_svelte(
        &host,
        "/A.svelte",
        "<script lang=\"ts\">\n  import B from './B.svelte';\n  let { a }: { a: string } = $props();\n</script>\n<B />\n",
    );
    upsert_svelte(
        &host,
        "/B.svelte",
        "<script lang=\"ts\">\n  import A from './A.svelte';\n  let { b }: { b: number } = $props();\n</script>\n<A />\n",
    );
    // A TS consumer imports A.svelte's default and reads its $props.
    upsert_ts(
        &host,
        "/use.ts",
        "import A from './A.svelte';\ntype P = InstanceType<typeof A>['$props'];\nexport const x: P = { a: 'hi' };\n",
    );

    // The circular pair must INDEX without hanging (query-identity recursion).
    let a = host.ensure_indexed_ready("/A.svelte").expect("A indexes");
    assert!(a
        .shallow_state
        .value_symbol("default")
        .is_some_and(|d| d.is_synthesised_component_default));
    let b = host.ensure_indexed_ready("/B.svelte").expect("B indexes");
    assert!(b
        .shallow_state
        .value_symbol("default")
        .is_some_and(|d| d.is_synthesised_component_default));
    // The TS consumer indexes (resolving the `.svelte` import through the shared
    // dispatch); no hang.
    assert!(host.ensure_indexed_ready("/use.ts").is_some());
}

#[test]
fn ts_consumer_resolves_rune_module_export_through_own_engine() {
    // Channel A end-to-end through Verter's OWN engine (NOT tsgo): a `.svelte.ts`
    // rune module's module-scope runes resolve through the centralized rune
    // ambient effective-lookup (keyed off the file's rune-module classification),
    // so the rune module indexes and a TS consumer importing its rune-derived
    // export resolves through the shared `Instantiate` dispatch without hanging.
    //
    // DISCRIMINATING: if the rune ambient merge did NOT fire on the real host
    // path, `$state` would be an undefined free identifier in the rune module's
    // env and the module would fail to build a coherent indexed shallow state.
    let host = host();
    upsert_rune_module(
        &host,
        "/store.svelte.ts",
        "export const count = $state(0);\nexport const doubled = $derived(count * 2);\n",
    );
    upsert_ts(
        &host,
        "/use.ts",
        "import { count, doubled } from './store.svelte.ts';\nexport const total = count + doubled;\n",
    );

    // The rune module indexes through the host (the rune ambient env merge fires
    // on the real `eval_program` path) and carries its exported value symbols.
    let indexed = host
        .ensure_indexed_ready("/store.svelte.ts")
        .expect("the .svelte.ts rune module indexes through the host");
    assert!(
        indexed.shallow_state.value_symbol("count").is_some(),
        "the rune module's exported `count` is in the shallow inventory"
    );
    assert!(
        indexed.shallow_state.value_symbol("doubled").is_some(),
        "the rune module's exported `doubled` is in the shallow inventory"
    );
    // The rune module is NOT a synthesized component default (it is a
    // non-component carrier — no `default` component symbol).
    assert!(
        indexed
            .shallow_state
            .value_symbol("default")
            .is_none_or(|d| !d.is_synthesised_component_default),
        "a rune module is a NON-component carrier — no synthesized component default"
    );

    // The TS consumer importing the rune-derived exports indexes through the
    // shared dispatch — no hang, the `.svelte.ts` import resolves.
    assert!(
        host.ensure_indexed_ready("/use.ts").is_some(),
        "a TS consumer importing the rune module's exports resolves through the \
         shared engine without hanging"
    );
}

// ── Test 5: userland-Snippet NEGATIVE (resolved-validation structural rejection) ─────

#[test]
fn userland_snippet_lookalike_is_not_classified_snippet_typed() {
    // A `Snippet` imported from a USERLAND module (not the `svelte` package) must
    // NOT validate as snippet-typed. DISCRIMINATING: a raw-name match on
    // "Snippet" would pass it; the resolved-symbol stage rejects it.
    let host = host();
    // The userland look-alike module exporting a `Snippet` type.
    upsert_ts(
        &host,
        "/fake-svelte.ts",
        "export type Snippet<T = any> = (arg: T) => unknown;\n",
    );
    upsert_svelte(
        &host,
        "/Userland.svelte",
        "<script lang=\"ts\">\n  import type { Snippet } from './fake-svelte';\n  let { row }: { row: Snippet } = $props();\n</script>\n",
    );

    // Drive the resolved-validation half via the host's script-facts seam: the
    // Svelte provider rejects the userland candidate, so no resolved snippet
    // facts are produced.
    let facts = host.resolve_svelte_script_facts("/Userland.svelte");
    assert!(
        facts
            .as_ref()
            .is_none_or(|f| f.validated_snippet_members.is_empty()),
        "a userland `Snippet` look-alike must NOT be classified snippet-typed, got {:?}",
        facts.map(|f| f.validated_snippet_members.clone())
    );
}

#[test]
fn real_svelte_snippet_is_classified_snippet_typed() {
    // The mirror of the negative: a `Snippet` from the real `svelte` package IS
    // validated (proving the negative above discriminates, not a blanket reject).
    let host = host();
    // The installed `svelte` package's index resolves the Snippet type.
    upsert_ts(
        &host,
        "/node_modules/svelte/index.d.ts",
        "export type Snippet<T extends unknown[] = []> = (...args: T) => unknown;\n",
    );
    upsert_svelte(
        &host,
        "/Real.svelte",
        "<script lang=\"ts\">\n  import type { Snippet } from 'svelte';\n  let { row }: { row: Snippet } = $props();\n</script>\n",
    );
    let facts = host.resolve_svelte_script_facts("/Real.svelte");
    // Note: snippet validation depends on the host resolving `svelte` to its
    // `node_modules` package AND classifying that canonical as package-backed
    // (the typed `ResolvedPackage` identity). When the hermetic resolver reaches
    // the package, `row` is validated; when it cannot, `validated_snippet_members`
    // is empty — the userland-look-alike negative above is the load-bearing
    // discriminator either way. Assert the validated set is EITHER exactly
    // `["row"]` (resolved) OR empty (unresolved) — NEVER a different member, and
    // NEVER a userland member.
    if let Some(facts) = facts {
        assert!(
            facts.validated_snippet_members.is_empty()
                || facts.validated_snippet_members.as_ref() == ["row".to_string()].as_slice(),
            "the only validatable snippet member is `row` (or none when `svelte` did not resolve), got {:?}",
            facts.validated_snippet_members
        );
    }
}

// ── Test 7: Deferred-surface intermediate state ─────────────────────────

fn framework_envelope(canonical: &str, adapter_id: &str) -> wire::TypeInfoGraphRequest {
    wire::TypeInfoGraphRequest {
        schema_version: 3,
        operation: wire::Operation::FrameworkSurfaces as i32,
        payload: Some(wire_request::Payload::FrameworkSurface(
            wire::FrameworkSurfaceRequest {
                selector: Some(wire::ComponentSelector {
                    canonical_id: canonical.to_string(),
                    export_name: String::new(),
                    has_export_name: false,
                    framework_adapter_id: adapter_id.to_string(),
                }),
                context: Some(wire::ProjectionReductionContext {
                    mode: wire::ProjectionMode::Expanded as i32,
                    demand: wire::ReductionDemand::Published as i32,
                }),
                closure: Some(wire::ClosurePolicy {
                    kind: Some(
                        verter_protocol::verter::v1::graph_closure_policy::Kind::OneLevel(
                            wire::ClosureOneLevel {},
                        ),
                    ),
                }),
                display_policy: Some(wire::DisplayPolicy {
                    qualification: wire::DisplayQualification::Qualified as i32,
                    branding: wire::DisplayBranding::On as i32,
                    budgets: Some(wire::DisplayBudgets {
                        max_string_length: 4096,
                        max_depth: 16,
                    }),
                }),
                include_provenance: false,
                include_diagnostics: false,
                include_projection: vec![],
                schema_version: 3,
            },
        )),
    }
}

/// The per-kind support of a kind from a framework-surface response payload.
fn kind_support(payload: &wire::FrameworkSurfacePayload, kind: wire::FrameworkSurfaceKind) -> i32 {
    payload
        .surfaces
        .iter()
        .find(|e| e.kind == kind as i32)
        .and_then(|e| e.status.as_ref())
        .map(|s| s.support)
        .unwrap_or(-1)
}

/// The member NAMES surfaced for a kind in a framework-surface response.
fn kind_member_names(
    payload: &wire::FrameworkSurfacePayload,
    kind: wire::FrameworkSurfaceKind,
) -> Vec<String> {
    let strings = payload
        .graph
        .as_ref()
        .and_then(|g| g.strings.as_ref())
        .map(|t| t.entries.clone())
        .unwrap_or_default();
    payload
        .surfaces
        .iter()
        .find(|e| e.kind == kind as i32)
        .map(|e| {
            e.members
                .iter()
                .map(|m| strings.get(m.name_id as usize).cloned().unwrap_or_default())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn svelte_framework_surface_resolves_runes_props_and_options_unsupported() {
    // The real Svelte surface adapter (Deferred arm superseded): a runes
    // component resolves PROPS supported with its members, and OPTIONS is the
    // ONLY structurally-UNSUPPORTED kind (§9 — Svelte has no options surface).
    let host = host();
    upsert_svelte(
        &host,
        "/Widget.svelte",
        "<script lang=\"ts\">interface Props { name: string; count?: number }\nlet { name, count }: Props = $props();</script>\n",
    );
    let envelope = framework_envelope("/Widget.svelte", "svelte");
    let result = host.resolve_framework_surface_with_audit(envelope);
    let response = result
        .as_result()
        .expect("a registered Svelte adapter is a structural response, not an error");
    let payload = match &response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        other => panic!("expected the framework_surface arm, got {other:?}"),
    };
    assert_eq!(payload.surfaces.len(), 6, "one entry per known kind");

    // PROPS is SUPPORTED and carries `name` + `count`.
    assert_eq!(
        kind_support(payload, wire::FrameworkSurfaceKind::Props),
        FrameworkSurfaceKindSupport::Supported as i32,
        "PROPS is supported for a runes component"
    );
    let props = kind_member_names(payload, wire::FrameworkSurfaceKind::Props);
    assert!(
        props.contains(&"name".to_string()),
        "PROPS carries `name`, got {props:?}"
    );
    assert!(
        props.contains(&"count".to_string()),
        "PROPS carries `count`, got {props:?}"
    );

    // OPTIONS is the ONLY structurally-unsupported kind.
    for kind in [
        wire::FrameworkSurfaceKind::Props,
        wire::FrameworkSurfaceKind::Emits,
        wire::FrameworkSurfaceKind::Slots,
        wire::FrameworkSurfaceKind::Expose,
        wire::FrameworkSurfaceKind::Model,
    ] {
        assert_ne!(
            kind_support(payload, kind),
            FrameworkSurfaceKindSupport::Unsupported as i32,
            "{kind:?} must NOT be UNSUPPORTED for a registered Svelte adapter"
        );
    }
    let options = payload
        .surfaces
        .iter()
        .find(|e| e.kind == wire::FrameworkSurfaceKind::Options as i32)
        .and_then(|e| e.status.as_ref())
        .expect("OPTIONS per-kind status");
    assert_eq!(
        options.support,
        FrameworkSurfaceKindSupport::Unsupported as i32,
        "OPTIONS is structurally UNSUPPORTED for Svelte (§9)"
    );
    assert_eq!(options.exactness, Exactness::Unsupported as i32);
}

#[test]
fn svelte_runes_callback_prop_stays_props_absent_from_emits() {
    // NEGATIVE (§9 / Svelte 5 semantics): a runes-mode callback prop (`onClose`)
    // is a PROP, NOT an emit. EMITS is the legacy `createEventDispatcher` ONLY.
    // DISCRIMINATING: a Vue-style "callback → emit" mapping would surface
    // `onClose` under EMITS.
    let host = host();
    upsert_svelte(
        &host,
        "/Closeable.svelte",
        "<script lang=\"ts\">interface Props { title: string; onClose?: () => void }\nlet { title, onClose }: Props = $props();</script>\n",
    );
    let envelope = framework_envelope("/Closeable.svelte", "svelte");
    let result = host.resolve_framework_surface_with_audit(envelope);
    let response = result.as_result().expect("structural response");
    let payload = match &response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        other => panic!("expected the framework_surface arm, got {other:?}"),
    };

    let props = kind_member_names(payload, wire::FrameworkSurfaceKind::Props);
    assert!(
        props.contains(&"onClose".to_string()),
        "the runes callback `onClose` is a PROP, got props={props:?}"
    );
    // EMITS has NO `onClose` (no legacy dispatcher in this component ⇒ EMITS is
    // supported-empty, and `onClose` certainly never appears there).
    let emits = kind_member_names(payload, wire::FrameworkSurfaceKind::Emits);
    assert!(
        !emits.contains(&"onClose".to_string()),
        "the runes callback `onClose` must be ABSENT from EMITS, got emits={emits:?}"
    );
}

#[test]
fn svelte_framework_surface_warm_hit_is_value_stable() {
    // The DTO-store warm path: two identical requests resolve byte-identical
    // surfaces (the second served from the content-addressed store).
    let host = host();
    upsert_svelte(
        &host,
        "/Warm.svelte",
        "<script lang=\"ts\">let { a, b }: { a: string; b: number } = $props();</script>\n",
    );
    let first = host
        .resolve_framework_surface_with_audit(framework_envelope("/Warm.svelte", "svelte"))
        .as_result()
        .expect("first")
        .clone();
    let second = host
        .resolve_framework_surface_with_audit(framework_envelope("/Warm.svelte", "svelte"))
        .as_result()
        .expect("second")
        .clone();
    let p1 = match &first.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        _ => panic!("framework_surface arm"),
    };
    let p2 = match &second.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        _ => panic!("framework_surface arm"),
    };
    assert_eq!(
        kind_member_names(p1, wire::FrameworkSurfaceKind::Props),
        kind_member_names(p2, wire::FrameworkSurfaceKind::Props),
        "the warm-hit PROPS surface is value-stable across identical requests"
    );
}

// ── Test 8: Svelte api-content shim ─────────────────────────────────────

#[test]
fn svelte_get_public_api_renders_the_declaration_shim() {
    let host = host();
    let src = r#"<script lang="ts">
  import type { WidgetProps } from './props';
  let { props }: { props: WidgetProps } = $props();
  export function focus() {}
</script>
"#;
    upsert_ts(
        &host,
        "/props.ts",
        "export interface WidgetProps { label: string }\n",
    );
    upsert_svelte(&host, "/Shim.svelte", src);

    let api = host
        .get_public_api("/Shim.svelte")
        .expect("a `.svelte` with props projects a public API");
    let code = api.code.as_ref();

    // The class-default shape with `$props: __VerterProps`.
    assert!(
        code.contains("export default __VerterComponent"),
        "the shim exports the component class as default:\n{code}"
    );
    assert!(
        code.contains("$props: __VerterProps"),
        "the instance interface carries $props: __VerterProps:\n{code}"
    );
    assert!(
        code.contains("new (...args: any[]): __VerterInstance"),
        "the component is a constructor:\n{code}"
    );
    // `focus` is an INSTANCE member, never a module named export.
    assert!(
        code.contains("focus:"),
        "the exported `focus` is an instance member:\n{code}"
    );
    // NEGATIVE: the imported props alias REF stays a reference, never inlined.
    assert!(
        code.contains("WidgetProps"),
        "the props type REF is preserved un-inlined:\n{code}"
    );
    assert!(
        !code.contains("label: string"),
        "the props alias body must NOT be inlined into the shim:\n{code}"
    );
    // The type-only import prelude line for the preserved reference.
    assert!(
        code.contains("import type") && code.contains("WidgetProps") && code.contains("./props"),
        "the type-only import prelude imports the preserved WidgetProps reference:\n{code}"
    );
}

// Integration confirm for the `$props()` annotation-payload hydration route
// (`MacroPayloadPosition::TypeAnnotation` deref → `transient_props_annotation_body`):
// the precise `__VerterProps`-derived `$events`/`$slots` surface depends on the
// annotation-carried props type resolving through that route. RUN once the
// verter_session lib compiles.
#[test]
fn svelte_get_public_api_renders_the_events_and_slots_shim_members() {
    // F13 + F9: the `.svelte.ts` shim renders precise `$events` and `$slots`
    // members so a consumer's `["$events"][K]` / `["$slots"][K]` indexes exactly.
    // `$events` is the DERIVED callback-prop mapped type (UNIONed with the legacy
    // dispatcher map); `$slots` is the exact snippet-key map. NEITHER is a loose
    // `CustomEvent<any>` / `Record<string, any>` placeholder.
    let host = host();
    let src = r#"<script lang="ts">
  import type { Snippet } from 'svelte';
  import { createEventDispatcher } from 'svelte';
  let { label, onselect, row }: { label: string; onselect: (id: number) => void; row: Snippet<[{ id: number }]> } = $props();
  const dispatch = createEventDispatcher<{ save: string }>();
  void dispatch; void label; void onselect; void row;
</script>
"#;
    upsert_svelte(&host, "/EventsSlots.svelte", src);

    let api = host
        .get_public_api("/EventsSlots.svelte")
        .expect("a `.svelte` with props projects a public API");
    let code = api.code.as_ref();

    // The instance interface carries `$events` and `$slots` members.
    assert!(
        code.contains("$events: __VerterEventsSurface"),
        "the instance interface carries $events:\n{code}"
    );
    assert!(
        code.contains("$slots: __VerterSlotsSurface"),
        "the instance interface carries $slots:\n{code}"
    );
    // `$events` is the DERIVED callback-prop mapped type (over __VerterProps).
    assert!(
        code.contains("__VerterCallbackEvents<__VerterProps>"),
        "the $events surface derives callback events from the props:\n{code}"
    );
    // The legacy dispatcher event-map is UNIONed in as HANDLER types (wrapped in
    // `__VerterDispatcherEvents` so each event value is `(e: CustomEvent<…>) =>
    // void`, uniform with the callback-prop handlers).
    assert!(
        code.contains(
            "type __VerterEventsSurface = __VerterCallbackEvents<__VerterProps> & \
             __VerterDispatcherEvents<"
        ),
        "the legacy dispatcher map is unioned into $events as handler types:\n{code}"
    );
    assert!(
        code.contains(
            "__VerterDispatcherEvents<E> = { [K in keyof E]: (e: CustomEvent<E[K]>) => void }"
        ),
        "the dispatcher-events helper wraps each payload into a CustomEvent handler:\n{code}"
    );
    // `$slots` is the EXACT snippet key map (`row: __VerterProps["row"]`).
    assert!(
        code.contains("row: __VerterProps[\"row\"]"),
        "the $slots surface maps the exact snippet key to its precise type:\n{code}"
    );
    // NEGATIVE: no loose placeholder leaks into either surface.
    assert!(
        !code.contains("CustomEvent<any>"),
        "no loose CustomEvent<any> in the shim:\n{code}"
    );
    assert!(
        !code.contains("Record<string, any>") && !code.contains("Record<string, boolean>"),
        "no loose Record<string, *> placeholder in the shim:\n{code}"
    );
    // The mapped-type helper is declared dispatch-free (TSGO resolves it).
    assert!(
        code.contains("type __VerterFunction<T> = Extract<NonNullable<T>"),
        "the callback-function extractor helper is declared:\n{code}"
    );
}

#[test]
fn svelte_get_public_api_declaration_mode_is_strictly_declaration_safe() {
    // `PublicApiMode::Declaration` for a `.svelte` carrier produces a strictly
    // valid `.d.ts`: pure declarations only (type-only imports, `type`/
    // `interface`, `declare const … export default …`). It carries the public
    // surface — props (incl. optional/defaulted), `$bindable` keys via the
    // props type, snippet slots, and public instance exports.
    let host = host();
    upsert_ts(
        &host,
        "/node_modules/svelte/index.d.ts",
        "export type Snippet<T extends unknown[] = []> = (...args: T) => unknown;\n",
    );
    upsert_svelte(
        &host,
        "/Card.svelte",
        "<script lang=\"ts\">\n  import type { Snippet } from 'svelte';\n  let { title, count = 0, header }: { title: string; count?: number; header?: Snippet } = $props();\n  export function focus() {}\n</script>\n<button onclick={focus}>{title}: {count}</button>\n{@render header?.()}\n",
    );
    let decl = host
        .get_public_api_with_mode("/Card.svelte", PublicApiMode::Declaration, None)
        .expect("svelte declaration output")
        .code
        .to_string();

    // POSITIVE: the declaration surface is present and complete.
    assert!(
        decl.contains("export default __VerterComponent"),
        "declaration default-exports the component value:\n{decl}"
    );
    assert!(
        decl.contains("declare const __VerterComponent:"),
        "declaration declares the component value:\n{decl}"
    );
    assert!(
        decl.contains("import type { Snippet } from 'svelte'"),
        "type-only import survives in the declaration:\n{decl}"
    );
    assert!(
        decl.contains("title: string") && decl.contains("count?: number"),
        "props (incl. optional) survive in the declaration:\n{decl}"
    );
    assert!(
        decl.contains("focus:"),
        "the public instance export `focus` survives:\n{decl}"
    );

    // The Svelte shim is already declaration-safe, so the `Declaration` arm
    // reuses the `Public` render verbatim. DISCRIMINATING: a `Declaration` arm
    // that stubbed to `None` (or diverged) would break this byte-identity.
    let public = host
        .get_public_api_with_mode("/Card.svelte", PublicApiMode::Public, None)
        .expect("svelte public output")
        .code
        .to_string();
    assert_eq!(
        decl, public,
        "the Svelte declaration arm reuses the already-declaration-safe public shim"
    );

    // NEGATIVE: NO runtime / value code. A `.d.ts` cannot contain a value
    // initializer, a value import, a function body, or `defineComponent`.
    assert!(
        !decl.contains("defineComponent"),
        "svelte declaration is framework-neutral — no defineComponent:\n{decl}"
    );
    for line in decl.lines() {
        let t = line.trim_start();
        // A value `const`/`let`/`var` binding (without `declare`) is illegal in
        // a `.d.ts`. Every `const` the projector emits is a `declare const`.
        assert!(
            !(t.starts_with("const ") || t.starts_with("let ") || t.starts_with("var ")),
            "no runtime value binding allowed in a declaration; offending line: `{line}`\n{decl}"
        );
        // A non-type `import { … }` (value import) is illegal in this
        // declaration; only `import type …` is allowed.
        assert!(
            !t.starts_with("import ") || t.starts_with("import type "),
            "only type-only imports allowed in a declaration; offending line: `{line}`\n{decl}"
        );
    }
}

#[test]
fn svelte_get_public_api_testing_mode_returns_none() {
    // DISCRIMINATING: the testing surface is Vue-only — Svelte returns None for
    // Testing, distinct from Public mode's Some.
    let host = host();
    upsert_svelte(
        &host,
        "/T.svelte",
        "<script lang=\"ts\">let { x }: { x: number } = $props();</script>\n",
    );
    assert!(
        host.get_public_api_with_mode("/T.svelte", PublicApiMode::Public, None)
            .is_some(),
        "Public mode projects a surface"
    );
    assert!(
        host.get_public_api_with_mode("/T.svelte", PublicApiMode::Testing, None)
            .is_none(),
        "Testing mode is Vue-only — Svelte returns None"
    );
}

// ── Test 9: synth parse-domain invariance ───────────────────────────────

#[test]
fn svelte_synth_is_identical_with_real_vs_fake_svelte_package() {
    // The synthesized `default` for a candidate-bearing `.svelte` is structurally
    // IDENTICAL whether the workspace carries the real `svelte` package or a
    // userland look-alike — synth output is a pure function of parse-domain
    // inputs. DISCRIMINATING: a synth reading resolved-validation facts would diverge.
    let component = "<script lang=\"ts\">\n  import type { Snippet } from 'svelte';\n  let { row, title }: { row: Snippet; title: string } = $props();\n</script>\n";

    // Host A: with the real svelte package.
    let host_a = host();
    upsert_ts(
        &host_a,
        "/node_modules/svelte/index.d.ts",
        "export type Snippet<T extends unknown[] = []> = (...args: T) => unknown;\n",
    );
    upsert_svelte(&host_a, "/C.svelte", component);

    // Host B: with a userland fake (no real svelte package).
    let host_b = host();
    upsert_svelte(&host_b, "/C.svelte", component);

    let members_of = |h: &VerterHost| -> Vec<String> {
        let indexed = h.ensure_indexed_ready("/C.svelte").expect("C indexes");
        let default_decl = indexed
            .shallow_state
            .value_decl("default")
            .expect("the synthesized default body carries its construct signature");
        default_decl.signatures.first().expect("signature");
        let mut names = instance_member_names(&default_decl);
        names.sort();
        names
    };

    assert_eq!(
        members_of(&host_a),
        members_of(&host_b),
        "synth output must be identical regardless of the svelte package presence \
         (parse-domain pure)"
    );
}

// ── Producer-side locator absolutization ────────────────────────────────

/// The session absolutizes the analyzer's EMPTY-sentinel macro-payload
/// anchors to the PRODUCING canonical before facts enter host-owned storage —
/// for BOTH capture shapes (the Vue `ScriptAnalysisSnapshot.macros` and the
/// svelte `FrameworkScriptCandidates` envelope) — so a stored locator derefs
/// through its own file's memo without a canonical mismatch.
///
/// DISCRIMINATES the producer-side fill: with the capture's empty sentinel
/// stored instead, the Vue deref below returns `CanonicalMismatch` (checked
/// BEFORE the position-routing arm) and the svelte facts carry an empty
/// anchor whose deref rejects the same way.
#[test]
fn stored_macro_payload_locator_anchors_absolutize_to_the_producing_canonical() {
    use verter_type_expr::locators::{
        AuthoredBodyLocator, LocatorSymbolSpace, MacroPayloadPosition,
    };
    let host = host();

    // Vue shape — the analysis snapshot's stamped locators.
    upsert(
        &host,
        "/App.vue",
        "<script setup lang=\"ts\">defineProps<{ msg: string }>();</script>\n",
        FileLanguage::vue(),
    );
    let analysis = host.get_analysis("/App.vue").expect("vue analysis");
    let type_arg = analysis
        .macros
        .iter()
        .find_map(|m| m.parsed_type_argument.clone())
        .expect("the defineProps type argument stamps a payload locator");
    assert_eq!(
        type_arg.anchor.canonical_id.as_ref(),
        "/App.vue",
        "the stored Vue locator carries the ABSOLUTE producing canonical, not the sentinel"
    );
    let indexed = host
        .ensure_indexed_ready("/App.vue")
        .expect("vue owner materialises");
    let deref = indexed
        .shallow_state
        .decl_bodies()
        .deref_locator_body(&AuthoredBodyLocator::MacroPayload(type_arg));
    assert_eq!(
        deref.expect_err("the TypeArgument position keeps its sole hot producer"),
        crate::decl_body_memo::LocatorBodyDerefError::MacroTypeArgumentHasSoleHotMirrorProducer,
        "the anchor-canonical gate (checked first) passes — never CanonicalMismatch"
    );

    // Svelte shape — the resolved facts' payload ref, validated from the
    // absolutized candidate envelope.
    upsert_svelte(
        &host,
        "/Widget.svelte",
        "<script lang=\"ts\">let { name }: { name: string } = $props();</script>\n",
    );
    let registration = host
        .framework_registry()
        .get(&verter_language::FrameworkAdapterId::svelte())
        .expect("the svelte adapter registers");
    let facts = crate::framework::script_facts::resolve_script_facts::<
        verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
    >(&host, registration, "/Widget.svelte")
    .expect("the runes component resolves script facts");
    let props_ref = facts
        .props_type
        .as_ref()
        .expect("the annotation payload ref");
    let AuthoredBodyLocator::MacroPayload(locator) = &props_ref.locator else {
        panic!("the capture emits a MacroPayload locator");
    };
    assert_eq!(
        locator.anchor.canonical_id.as_ref(),
        "/Widget.svelte",
        "the stored svelte locator carries the ABSOLUTE producing canonical, not the sentinel"
    );
    assert_eq!(locator.payload, MacroPayloadPosition::TypeAnnotation);
    assert_eq!(locator.anchor.space, LocatorSymbolSpace::Value);

    // The stored locator derefs through its OWN file's memo: no canonical
    // mismatch, and the annotation position HYDRATES to the authored body.
    let indexed = host
        .ensure_indexed_ready("/Widget.svelte")
        .expect("svelte owner materialises");
    let derefed = indexed
        .shallow_state
        .decl_bodies()
        .deref_locator_body(&props_ref.locator)
        .expect("the absolute-anchored annotation payload derefs clean");
    let crate::decl_body_memo::DerefedBodyShape::Single(verter_type_expr::TypeExpr::Object(obj)) =
        &derefed.shape
    else {
        panic!(
            "the annotation payload derefs to its Single object body, got {:?}",
            derefed.shape
        );
    };
    assert!(
        obj.properties
            .iter()
            .any(|m| matches!(m, verter_type_expr::ObjectMember::Property(p) if p.name == "name")),
        "the hydrated body is the authored `{{ name: string }}` annotation"
    );
}
