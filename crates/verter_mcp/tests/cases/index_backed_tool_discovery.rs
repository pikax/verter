//! The project-wide MCP tools answer the same populations whatever host
//! history produced them: parity on a fixed workspace, and an incrementally
//! edited host answering exactly like a fresh host loaded with the same files.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;
use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use serde_json::Value;
use verter_diagnostics::Linter;
use verter_mcp::config::McpServerConfig;
use verter_mcp::index_tools::{self, DiscoveryWork};
use verter_mcp::server::{
    ComponentGraphParams, FilePathParams, OptionalPathParams, TraceEventFlowParams,
    TraceStoreFlowParams,
};
use verter_mcp::tools::diagnostics::make_lint_config;
use verter_mcp::VerterMcpServer;
use verter_session::workspace_index::QueryBudgets;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const PARENT: &str = r#"<script setup lang="ts">
import { provide } from 'vue'
import { RouterLink } from 'vue-router'
import Child from './Child.vue'
import { useCounterStore } from '@/stores/counter'
defineProps<{ title: string; theme: string }>()
const counter = useCounterStore()
provide('theme', 'dark')
function onSave() {}
</script>
<template>
  <div class="card">
    <Child :title="title" :extra="1" @save="onSave" />
    <RouterLink to="/about">About</RouterLink>
  </div>
</template>
<style>
.card { color: red; }
.shared { color: blue; }
</style>
"#;

const CHILD: &str = r#"<script setup lang="ts">
import { inject } from 'vue'
defineProps<{ title: string; size: number }>()
defineEmits(['save'])
const theme = inject('theme')
const missing = inject('missing')
</script>
<template>
  <section class="card inner">{{ title }}</section>
</template>
<style>
.shared { color: green; }
</style>
<style scoped>
.inner { color: black; }
</style>
"#;

const OPTIONS: &str = r#"<script>
import { defineComponent, onMounted } from 'vue'
const Options = defineComponent({
  data() { return { a: 1 } },
  setup() { onMounted(() => {}) },
})
export default Options
</script>
<template><div>{{ a }}</div></template>
"#;

const STORE_HOST: &str = r#"<script lang="ts">
import { defineStore } from 'pinia'
export const useCounterStore = defineStore('counter', {
  state: () => ({ count: 0 }),
  actions: { inc() {} },
})
</script>
<template><div /></template>
"#;

const WIDGET: &str = "<script>\nlet a = 1;\n</script>\n<div class=\"shared\">{a}</div>\n";

const UTIL: &str = "export const one = 1;\n";

fn language(id: &str) -> FileLanguage {
    if id.ends_with(".vue") {
        FileLanguage::vue()
    } else if id.ends_with(".svelte") {
        FileLanguage::svelte()
    } else {
        FileLanguage::script_ts()
    }
}

fn upsert(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(id.to_string()),
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: language(id),
            aliases: Vec::new(),
        })
        .expect("upsert");
}

fn fixture() -> BTreeMap<&'static str, String> {
    [
        ("/app/src/Parent.vue", PARENT),
        ("/app/src/Child.vue", CHILD),
        ("/app/src/Options.vue", OPTIONS),
        ("/app/src/StoreHost.vue", STORE_HOST),
        ("/app/src/Widget.svelte", WIDGET),
        ("/app/src/util.ts", UTIL),
    ]
    .into_iter()
    .map(|(id, source)| (id, source.to_string()))
    .collect()
}

fn server_over(host: Arc<VerterHost>) -> VerterMcpServer {
    let linter = Arc::new(Linter::new(make_lint_config("recommended")));
    VerterMcpServer::new(
        host,
        linter,
        McpServerConfig {
            project_root: Some(std::path::PathBuf::from("/app")),
            ..McpServerConfig::default()
        },
    )
}

fn host() -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

/// A server over a fresh host loaded in reverse order: no answer may depend
/// on load order.
fn fresh_server(files: &BTreeMap<&str, String>) -> VerterMcpServer {
    let host = host();
    for (id, source) in files.iter().rev() {
        upsert(&host, id, source);
    }
    server_over(host)
}

fn body(result: Result<CallToolResult, rmcp::model::ErrorData>) -> Value {
    let result = result.expect("tool call");
    let text = result
        .content
        .iter()
        .find_map(|content| content.as_text().map(|text| text.text.clone()))
        .expect("text content");
    serde_json::from_str(&text).expect("json body")
}

/// Arrays sorted by their serialized form, so hash-map iteration order in an
/// answer never decides equality.
fn canonical(value: Value) -> Value {
    match value {
        Value::Array(items) => {
            let mut items: Vec<Value> = items.into_iter().map(canonical).collect();
            items.sort_by_key(|item| item.to_string());
            Value::Array(items)
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect(),
        ),
        other => other,
    }
}

fn none() -> Parameters<OptionalPathParams> {
    Parameters(OptionalPathParams { path: None })
}

/// Every tool this suite covers, by name, with its answer.
async fn answers(server: &VerterMcpServer) -> BTreeMap<&'static str, Value> {
    let mut out = BTreeMap::new();
    out.insert(
        "get_component_graph",
        body(
            server
                .get_component_graph(Parameters(ComponentGraphParams {
                    root: None,
                    max_depth: None,
                }))
                .await,
        ),
    );
    out.insert(
        "check_component_props",
        body(server.check_component_props(none()).await),
    );
    out.insert(
        "detect_prop_drilling",
        body(server.detect_prop_drilling(none()).await),
    );
    out.insert(
        "trace_event_flow",
        body(
            server
                .trace_event_flow(Parameters(TraceEventFlowParams {
                    event_name: "save".to_string(),
                }))
                .await,
        ),
    );
    out.insert(
        "validate_provide_inject",
        body(server.validate_provide_inject().await),
    );
    out.insert(
        "get_store_usage",
        body(server.get_store_usage(none()).await),
    );
    out.insert("get_store_graph", body(server.get_store_graph().await));
    out.insert(
        "trace_store_flow",
        body(
            server
                .trace_store_flow(Parameters(TraceStoreFlowParams {
                    store_id: "useCounterStore".to_string(),
                }))
                .await,
        ),
    );
    out.insert(
        "detect_css_bleed",
        body(server.detect_css_bleed(none()).await),
    );
    out.insert("get_project_stats", body(server.get_project_stats().await));
    out.insert(
        "detect_migration_targets",
        body(server.detect_migration_targets(none()).await),
    );
    out.insert(
        "ssr_project_report",
        body(server.ssr_project_report().await),
    );
    out.insert(
        "analyze_route_health",
        body(server.analyze_route_health().await),
    );
    out.insert("get_route_tree", body(server.get_route_tree().await));
    out.insert(
        "get_navigation_map",
        body(server.get_navigation_map(none()).await),
    );
    out.insert(
        "get_router_views",
        body(server.get_router_views(none()).await),
    );
    out.insert(
        "get_route_for_component",
        body(
            server
                .get_route_for_component(Parameters(FilePathParams {
                    path: "/app/src/Child.vue".to_string(),
                }))
                .await,
        ),
    );
    out.into_iter()
        .map(|(name, value)| (name, canonical(value)))
        .collect()
}

fn paths(value: &Value) -> Vec<String> {
    let mut out: Vec<String> = value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("path").to_string())
        .collect();
    out.sort();
    out
}

#[tokio::test]
async fn relation_tools_answer_the_fixture_populations() {
    let server = fresh_server(&fixture());
    let answers = answers(&server).await;

    let events = &answers["trace_event_flow"];
    assert_eq!(paths(&events["emitters"]), ["/app/src/Child.vue"]);
    assert_eq!(paths(&events["listeners"]), ["/app/src/Parent.vue"]);

    let provide = &answers["validate_provide_inject"];
    assert_eq!(provide["total_provide_keys"], 1);
    assert_eq!(provide["total_inject_keys"], 2);
    let missing = provide["missing_providers"].as_array().expect("missing");
    assert_eq!(missing.len(), 1, "{provide}");
    assert_eq!(missing[0]["key"], "missing");
    assert_eq!(paths(&missing[0]["inject_files"]), ["/app/src/Child.vue"]);
    assert_eq!(provide["unused_provides"], Value::Array(Vec::new()));

    let graph = answers["get_component_graph"]
        .as_object()
        .expect("graph object");
    assert_eq!(
        graph.keys().cloned().collect::<Vec<_>>(),
        ["/app/src/Parent.vue"],
        "only the file that uses components"
    );
    let uses: Vec<&str> = graph["/app/src/Parent.vue"]
        .as_array()
        .expect("uses")
        .iter()
        .filter_map(|usage| usage["name"].as_str())
        .collect();
    assert!(uses.contains(&"Child"), "{uses:?}");

    let props = &answers["check_component_props"];
    let issues = props["issues"].as_array().expect("issues");
    assert!(
        issues.iter().any(|issue| issue["kind"] == "unknown_prop"
            && issue["file"] == "/app/src/Parent.vue"
            && issue["component"] == "Child"
            && issue["prop"] == "extra"),
        "{props}"
    );
    assert!(
        !issues.iter().any(|issue| issue["prop"] == "title"),
        "a declared prop is known: {props}"
    );

    let drilling = &answers["detect_prop_drilling"];
    let drilled = drilling["drilled_props"].as_array().expect("drilled");
    assert!(
        drilled.iter().any(|entry| entry["parent"] == "Parent"
            && entry["child"] == "Child"
            && entry["drilled_props"] == serde_json::json!(["title"])),
        "{drilling}"
    );

    let bleed = &answers["detect_css_bleed"];
    let bleeds = bleed["bleeds"].as_array().expect("bleeds");
    assert!(
        bleeds
            .iter()
            .any(|entry| entry["kind"] == "global_class_bleed"
                && entry["class"] == "card"
                && entry["source_file"] == "/app/src/Parent.vue"
                && entry["affected_file"] == "/app/src/Child.vue"),
        "{bleed}"
    );
    assert!(
        bleeds
            .iter()
            .any(|entry| entry["kind"] == "shared_class_collision" && entry["class"] == "shared"),
        "{bleed}"
    );
    assert!(
        !bleeds.iter().any(|entry| entry["class"] == "inner"),
        "a scoped class never bleeds: {bleed}"
    );
}

#[tokio::test]
async fn store_tools_answer_the_fixture_populations() {
    let server = fresh_server(&fixture());
    let answers = answers(&server).await;

    let graph = &answers["get_store_graph"];
    assert_eq!(graph["total_store_definitions"], 1, "{graph}");
    assert_eq!(graph["all_store_ids"], serde_json::json!(["counter"]));
    let users = graph["store_usages_by_file"].as_array().expect("users");
    assert!(
        users
            .iter()
            .any(|entry| entry["file"] == "/app/src/Parent.vue"),
        "{graph}"
    );

    let usage = &answers["get_store_usage"];
    let files: Vec<&str> = usage["results"]
        .as_array()
        .expect("results")
        .iter()
        .filter_map(|entry| entry["file"].as_str())
        .collect();
    assert!(files.contains(&"/app/src/Parent.vue"), "{usage}");
    assert!(files.contains(&"/app/src/StoreHost.vue"), "{usage}");
    assert!(!files.contains(&"/app/src/Child.vue"), "{usage}");

    let flow = &answers["trace_store_flow"];
    assert_eq!(flow["definition_file"], "/app/src/StoreHost.vue", "{flow}");
    assert_eq!(flow["consumer_count"], 1, "{flow}");
}

#[tokio::test]
async fn population_tools_answer_the_fixture_populations() {
    let server = fresh_server(&fixture());
    let answers = answers(&server).await;

    let stats = &answers["get_project_stats"];
    assert_eq!(stats["overview"]["total_files"], 6, "{stats}");
    assert_eq!(stats["overview"]["componentFiles"], 5, "{stats}");
    assert_eq!(stats["overview"]["script_deps"], 1, "{stats}");
    assert_eq!(
        stats["component_stats"]["worst_components"]
            .as_array()
            .expect("worst")
            .len(),
        5
    );

    let ssr = &answers["ssr_project_report"];
    assert_eq!(ssr["total_components"], 4, "Vue components only: {ssr}");

    let migration = &answers["detect_migration_targets"];
    let targets: Vec<&str> = migration["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .filter_map(|target| target["path"].as_str())
        .collect();
    assert_eq!(targets, ["/app/src/Options.vue"], "{migration}");

    let health = &answers["analyze_route_health"];
    assert!(health.is_object(), "{health}");
}

async fn check(incremental: &VerterMcpServer, files: &BTreeMap<&str, String>, step: &str) {
    let live = answers(incremental).await;
    let clean = answers(&fresh_server(files)).await;
    for (tool, answer) in &live {
        assert_eq!(answer, &clean[tool], "{step}: {tool}");
    }
}

#[tokio::test]
async fn an_edited_host_answers_like_a_fresh_host_after_edit_add_and_delete() {
    let mut files = fixture();
    let host = host();
    for (id, source) in &files {
        upsert(&host, id, source);
    }
    let incremental = server_over(Arc::clone(&host));

    check(&incremental, &files, "initial").await;

    // Edit: the child stops injecting the missing key, emits another event,
    // and its global class moves.
    let edited = CHILD
        .replace("const missing = inject('missing')\n", "")
        .replace("defineEmits(['save'])", "defineEmits(['save', 'close'])")
        .replace(".shared { color: green; }", ".card { color: green; }");
    files.insert("/app/src/Child.vue", edited.clone());
    upsert(&host, "/app/src/Child.vue", &edited);
    check(&incremental, &files, "edit").await;

    // Add: a second consumer of the store and a listener.
    let added = "<script setup lang=\"ts\">\nimport { inject } from 'vue'\nimport Child from './Child.vue'\nimport { useCounterStore } from '@/stores/counter'\nconst store = useCounterStore()\nconst missing = inject('missing')\n</script>\n<template><Child title=\"x\" @close=\"() => {}\" class=\"shared\" /></template>\n";
    files.insert("/app/src/Other.vue", added.to_string());
    upsert(&host, "/app/src/Other.vue", added);
    check(&incremental, &files, "add").await;

    // Delete: the store host and the parent go away.
    files.remove("/app/src/StoreHost.vue");
    let _ = host.remove("/app/src/StoreHost.vue");
    files.remove("/app/src/Parent.vue");
    let _ = host.remove("/app/src/Parent.vue");
    check(&incremental, &files, "delete").await;
}

#[tokio::test]
async fn a_complete_population_adds_nothing_to_the_envelope() {
    let answers = answers(&fresh_server(&fixture())).await;
    for (tool, answer) in &answers {
        assert!(answer.get("enumeration").is_none(), "{tool}: {answer}");
    }
}

#[tokio::test]
async fn a_budget_cut_population_is_reported_partial_and_warms_nothing() {
    let files = fixture();
    let host = host();
    for (id, source) in &files {
        upsert(&host, id, source);
    }
    let linter = Arc::new(Linter::new(make_lint_config("recommended")));
    let tight = VerterMcpServer::new(
        Arc::clone(&host),
        Arc::clone(&linter),
        McpServerConfig {
            project_root: Some(std::path::PathBuf::from("/app")),
            index_budgets: QueryBudgets {
                postings: 1,
                records: 1,
            },
            ..McpServerConfig::default()
        },
    );
    let cut = answers(&tight).await;
    // Every tool whose population holds more than one posting.
    for tool in [
        "get_component_graph",
        "check_component_props",
        "trace_event_flow",
        "validate_provide_inject",
        "get_store_usage",
        "get_store_graph",
        "trace_store_flow",
        "detect_css_bleed",
        "get_project_stats",
        "detect_migration_targets",
        "ssr_project_report",
        "analyze_route_health",
    ] {
        let note = &cut[tool]["enumeration"];
        assert_eq!(note["status"], "partial", "{tool}: {}", cut[tool]);
        assert!(
            note["causes"]
                .as_array()
                .is_some_and(|causes| causes.iter().any(|cause| cause == "budget_exhausted")),
            "{tool}: {note}"
        );
    }

    // The partial pages kept nothing: the same host then answers in full,
    // exactly like a fresh host.
    let wide = server_over(Arc::clone(&host));
    let live = answers(&wide).await;
    let clean = answers(&fresh_server(&files)).await;
    for (tool, answer) in &live {
        assert!(answer.get("enumeration").is_none(), "{tool}: {answer}");
        assert_eq!(answer, &clean[tool], "{tool}");
    }
}

/// The counted work of every tool on `server`.
fn work(server: &VerterMcpServer) -> BTreeMap<&'static str, DiscoveryWork> {
    let none = OptionalPathParams { path: None };
    let root = std::path::Path::new("/app");
    let mut out = BTreeMap::new();
    out.insert(
        "get_component_graph",
        index_tools::get_component_graph(
            server,
            &ComponentGraphParams {
                root: None,
                max_depth: None,
            },
        )
        .work,
    );
    out.insert(
        "check_component_props",
        index_tools::check_component_props(server, &none).work,
    );
    out.insert(
        "detect_prop_drilling",
        index_tools::detect_prop_drilling(server, &none).work,
    );
    out.insert(
        "trace_event_flow",
        index_tools::trace_event_flow(
            server,
            &TraceEventFlowParams {
                event_name: "save".to_string(),
            },
        )
        .work,
    );
    out.insert(
        "validate_provide_inject",
        index_tools::validate_provide_inject(server).work,
    );
    out.insert(
        "get_store_usage",
        index_tools::get_store_usage(server, &none).work,
    );
    out.insert("get_store_graph", index_tools::get_store_graph(server).work);
    out.insert(
        "trace_store_flow",
        index_tools::trace_store_flow(
            server,
            &TraceStoreFlowParams {
                store_id: "useCounterStore".to_string(),
            },
        )
        .work,
    );
    out.insert(
        "detect_css_bleed",
        index_tools::detect_css_bleed(server, &none).work,
    );
    out.insert(
        "get_project_stats",
        index_tools::get_project_stats(server).work,
    );
    out.insert(
        "detect_migration_targets",
        index_tools::detect_migration_targets(server, &none).work,
    );
    out.insert(
        "ssr_project_report",
        index_tools::ssr_project_report(server).work,
    );
    out.insert(
        "analyze_route_health",
        index_tools::analyze_route_health(server, root)
            .expect("route health")
            .work,
    );
    out
}

#[test]
fn relation_lookups_are_keyed_and_bounded() {
    let server = fresh_server(&fixture());
    let work = work(&server);
    let events = work["trace_event_flow"];
    assert_eq!(events.index_queries, 1);
    assert_eq!(events.key_lookups, 1, "one event key");
    assert_eq!(events.posting_visits, 2, "the emit and the listener only");
    assert_eq!(events.owner_reads, 0, "rows answer without reading a file");
    let provide = work["validate_provide_inject"];
    assert_eq!(provide.posting_visits, 3, "one provide and two injects");
    assert_eq!(provide.owner_reads, 0);
    for tool in ["get_store_usage", "get_store_graph", "trace_store_flow"] {
        assert_eq!(work[tool].owner_reads, 2, "{tool}: the store files only");
    }
    assert_eq!(
        work["get_component_graph"].owner_reads, 1,
        "the one file that uses components"
    );
}

#[test]
fn unrelated_files_add_no_relation_visits_and_population_reads_grow_with_the_population() {
    let host = host();
    for (id, source) in &fixture() {
        upsert(&host, id, source);
    }
    let server = server_over(Arc::clone(&host));
    let before = work(&server);

    // Scripts with no component, style, registration or route facts.
    for index in 0..6 {
        upsert(
            &host,
            &format!("/app/src/lib/helper{index}.ts"),
            &format!("export const helper{index} = {index};\n"),
        );
    }
    let with_scripts = work(&server);
    for (tool, counted) in &before {
        if *tool == "get_project_stats" || *tool == "analyze_route_health" {
            // These walk the whole file population: visits grow by one per
            // file, owner reads do not.
            assert_eq!(
                with_scripts[tool].owner_reads, counted.owner_reads,
                "{tool}"
            );
            assert_eq!(
                with_scripts[tool].posting_visits,
                counted.posting_visits + 6,
                "{tool}"
            );
        } else {
            assert_eq!(&with_scripts[tool], counted, "{tool}");
        }
    }

    // Components with no relation to anything: relation answers stay put,
    // and only the component populations read the new components.
    for index in 0..3 {
        upsert(
            &host,
            &format!("/app/src/plain/Plain{index}.vue"),
            "<script setup>\nconst a = 1\n</script>\n<template><p>{{ a }}</p></template>\n",
        );
    }
    let with_components = work(&server);
    for tool in [
        "get_component_graph",
        "detect_prop_drilling",
        "trace_event_flow",
        "validate_provide_inject",
        "get_store_usage",
        "get_store_graph",
        "trace_store_flow",
        "detect_css_bleed",
    ] {
        assert_eq!(with_components[tool], with_scripts[tool], "{tool}");
    }
    assert_eq!(
        with_components["check_component_props"].owner_reads,
        with_scripts["check_component_props"].owner_reads,
        "no new stem matches a used tag"
    );
    for tool in [
        "get_project_stats",
        "detect_migration_targets",
        "ssr_project_report",
    ] {
        assert_eq!(
            with_components[tool].owner_reads,
            with_scripts[tool].owner_reads + 3,
            "{tool}: one read per new component"
        );
    }
}

#[tokio::test]
async fn a_component_without_a_template_block_leaves_every_answer_complete() {
    let mut files = fixture();
    files.insert(
        "/app/src/ScriptOnly.vue",
        "<script setup lang=\"ts\">\nconst a = 1\n</script>\n".to_string(),
    );
    let answers = answers(&fresh_server(&files)).await;
    for (tool, answer) in &answers {
        assert!(answer.get("enumeration").is_none(), "{tool}: {answer}");
    }
    assert_eq!(answers["ssr_project_report"]["total_components"], 5);
}
