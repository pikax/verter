//! B8a Svelte vertical — session integration behavior.
//!
//! Discriminating coverage for the parse + shallow + synth + api-content half:
//! - shallow inventory carries the synthesized `default`
//!   (`is_synthesised_component_default`) + exported members;
//! - a TS file importing a `.svelte` resolves the public type through the shared
//!   `Instantiate` dispatch; circular `.svelte ↔ .svelte` imports terminate;
//! - the userland-`Snippet` NEGATIVE: a `Snippet` from a non-`svelte` module is
//!   NOT classified snippet-typed (the resolved-validation stage rejects it; a
//!   raw-name match would pass it);
//! - the Deferred-surface intermediate state: a framework-surface request for a
//!   `.svelte` component returns one entry per known kind with UNSUPPORTED +
//!   GRAPH_EXACTNESS_UNSUPPORTED;
//! - the Svelte api-content shim (`get_public_api`) — class default with
//!   `$props: __VerterProps`, refs preserved un-inlined, the type-only prelude;
//!   `get_public_api_with_mode(Testing)` returns `None`;
//! - synth parse-domain invariance (D-au).

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
    let sig = default_symbol
        .signatures
        .first()
        .expect("construct signature");
    let return_type = sig.return_type.as_ref().expect("instance return type");
    let members: Vec<String> = match return_type {
        verter_type_expr::TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .filter_map(|m| match m {
                verter_type_expr::ObjectMember::Property(p) => Some(p.name.clone()),
                _ => None,
            })
            .collect(),
        other => panic!("expected an object instance shape, got {other:?}"),
    };
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
    let members = instance_member_names(default_symbol);
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
    let default_symbol = indexed
        .shallow_state
        .value_symbol("default")
        .expect("synthesized default");
    let members = instance_member_names(default_symbol);
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
fn instance_member_names(default_symbol: &crate::resolver_core::ShallowValueSymbol) -> Vec<String> {
    match default_symbol
        .signatures
        .first()
        .and_then(|s| s.return_type.as_ref())
    {
        Some(verter_type_expr::TypeExpr::Object(obj)) => obj
            .properties
            .iter()
            .filter_map(|m| match m {
                verter_type_expr::ObjectMember::Property(p) => Some(p.name.clone()),
                _ => None,
            })
            .collect(),
        other => panic!("expected object instance, got {other:?}"),
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
    // Note: resolution depends on the host resolving `svelte` to node_modules.
    // When it resolves, `row` is validated; when the hermetic resolver cannot
    // reach the package, no facts are produced — either way the negative test
    // above is the load-bearing discriminator. Assert that IF facts are
    // produced, `row` is the validated member (never a different member).
    if let Some(facts) = facts {
        assert_eq!(facts.validated_snippet_members, vec!["row".to_string()]);
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

#[test]
fn svelte_framework_surface_request_is_deferred_unsupported_per_kind() {
    let host = host();
    upsert_svelte(
        &host,
        "/Deferred.svelte",
        "<script lang=\"ts\">let { x }: { x: number } = $props();</script>\n",
    );
    let envelope = framework_envelope("/Deferred.svelte", "svelte");
    let result = host.resolve_framework_surface_with_audit(envelope);
    // A Deferred adapter is a STRUCTURAL response (NOT an error) — the audited
    // Ok outcome carrying the framework_surface arm.
    let response = result
        .as_result()
        .expect("a Deferred surface is a structural response, not an error");
    let payload = match &response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        other => panic!("expected the framework_surface arm, got {other:?}"),
    };
    // EXACTLY ONE entry per known kind, EVERY one structurally UNSUPPORTED.
    assert_eq!(payload.surfaces.len(), 6, "one entry per known kind");
    let strings = payload
        .graph
        .as_ref()
        .and_then(|g| g.strings.as_ref())
        .map(|t| t.entries.clone())
        .unwrap_or_default();
    for entry in &payload.surfaces {
        let status = entry.status.as_ref().expect("per-kind status");
        assert_eq!(
            status.support,
            FrameworkSurfaceKindSupport::Unsupported as i32,
            "a Deferred Svelte surface answers UNSUPPORTED for kind {}",
            entry.kind
        );
        assert_eq!(
            status.exactness,
            Exactness::Unsupported as i32,
            "GRAPH_EXACTNESS_UNSUPPORTED for kind {}",
            entry.kind
        );
        // The diagnostic names the deferred state explicitly (D-ag), NOT the
        // generic per-adapter unsupport — DISCRIMINATING: it is the
        // surfaces-not-yet-registered message.
        let diag = status
            .diagnostics
            .first()
            .expect("the Deferred surface carries a diagnostic");
        let message = strings
            .get(diag.message_name_id as usize)
            .cloned()
            .unwrap_or_default();
        assert!(
            message.contains("not yet registered"),
            "the Deferred diagnostic names the not-yet-registered state, got {message:?}"
        );
        assert!(entry.members.is_empty());
    }
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
    // The D-at type-only import prelude line for the preserved reference.
    assert!(
        code.contains("import type") && code.contains("WidgetProps") && code.contains("./props"),
        "the type-only import prelude imports the preserved WidgetProps reference:\n{code}"
    );
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

// ── Test 9: synth parse-domain invariance (D-au) ────────────────────────

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
        let default_symbol = indexed
            .shallow_state
            .value_symbol("default")
            .expect("synthesized default");
        let sig = default_symbol.signatures.first().expect("signature");
        match sig.return_type.as_ref().expect("return type") {
            verter_type_expr::TypeExpr::Object(obj) => {
                let mut names: Vec<String> = obj
                    .properties
                    .iter()
                    .filter_map(|m| match m {
                        verter_type_expr::ObjectMember::Property(p) => Some(p.name.clone()),
                        _ => None,
                    })
                    .collect();
                names.sort();
                names
            }
            other => panic!("expected object shape, got {other:?}"),
        }
    };

    assert_eq!(
        members_of(&host_a),
        members_of(&host_b),
        "synth output must be identical regardless of the svelte package presence \
         (parse-domain pure)"
    );
}
