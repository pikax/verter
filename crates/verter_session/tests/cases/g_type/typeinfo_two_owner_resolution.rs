//! Which declaration resolve-symbol answers when a carrier's script blocks
//! both declare the requested type name.
//!
//! A Vue `<script setup>` and a Svelte instance script are each the lexical
//! child of the module script: the engine's bare-name lookup sees the inner
//! block's own names first, so its declaration shadows the module one.
//! Resolve-symbol takes no owner, and it answers with that same visible
//! declaration, never the module declaration by default. A value does not
//! shadow a type of the same name, so only type declarations decide.

use std::sync::Arc;

use verter_protocol::typeinfo::graph::{
    self as wire, Operation, ProjectionMode, TYPEINFO_GRAPH_SCHEMA_VERSION,
};
use verter_protocol::typeinfo::TypeInfoRequest;
use verter_protocol::verter::v1::{
    graph_closure_policy, graph_property_key, graph_type_node, type_info_graph_request,
    type_info_graph_response,
};
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

fn host_with(canonical: &str, source: &str, file_language: FileLanguage) -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("{canonical}: upsert {error:?}"));
    host
}

fn resolve_envelope(canonical: &str, name: &str) -> TypeInfoRequest {
    TypeInfoRequest {
        schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: Operation::ResolveSymbol as i32,
        payload: Some(type_info_graph_request::Payload::ResolveSymbol(
            wire::ResolveSymbolGraphRequest {
                canonical_id: canonical.to_string(),
                name: name.to_string(),
                context: Some(wire::ProjectionReductionContext {
                    mode: ProjectionMode::Expanded as i32,
                    demand: wire::ReductionDemand::Published as i32,
                }),
                closure: Some(wire::ClosurePolicy {
                    kind: Some(graph_closure_policy::Kind::OneLevel(
                        wire::ClosureOneLevel {},
                    )),
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
                include_degraded: false,
            },
        )),
    }
}

/// The member names of the object `Foo` resolves to in `canonical`.
fn resolved_member_names(host: &VerterHost, canonical: &str) -> Vec<String> {
    let (response, _record) = host
        .type_info_with_audit(resolve_envelope(canonical, "Foo"))
        .into_parts();
    let response = response.expect("a well-formed resolve answers with the graph arm");
    let type_info_graph_response::Kind::Graph(graph) =
        response.kind.expect("the graph arm is present")
    else {
        panic!("{canonical}: resolve-symbol answers with the graph arm");
    };
    let root_id = graph.root_ids.first().copied().expect("one root");
    let root = &graph.nodes[root_id as usize];
    let Some(graph_type_node::Kind::Object(object)) = root.kind.as_ref() else {
        panic!(
            "{canonical}: Foo resolves to an object, got {:?}",
            root.kind
        );
    };
    let entries = graph
        .strings
        .as_ref()
        .map(|table| table.entries.as_slice())
        .unwrap_or(&[] as &[String]);
    object
        .members
        .iter()
        .map(|member| {
            let Some(graph_property_key::Key::StringId(id)) = member
                .property_key
                .as_ref()
                .and_then(|key| key.key.as_ref())
            else {
                panic!("{canonical}: every Foo member has a string key");
            };
            entries[*id as usize].clone()
        })
        .collect()
}

#[test]
fn a_name_declared_in_both_vue_script_blocks_resolves_to_the_setup_declaration() {
    let source = "<script lang=\"ts\">\ninterface Foo { a: string }\n</script>\n\
                  <script setup lang=\"ts\">\ninterface Foo { b: number }\n</script>\n\
                  <template><div /></template>\n";
    let host = host_with("/Both.vue", source, FileLanguage::vue());
    assert_eq!(resolved_member_names(&host, "/Both.vue"), ["b"]);
}

#[test]
fn a_name_declared_in_both_svelte_scripts_resolves_to_the_instance_declaration() {
    let source = "<script module lang=\"ts\">\ninterface Foo { a: string }\n</script>\n\
                  <script lang=\"ts\">\ninterface Foo { b: number }\n</script>\n\
                  <p>both</p>\n";
    let host = host_with("/Both.svelte", source, FileLanguage::svelte());
    assert_eq!(resolved_member_names(&host, "/Both.svelte"), ["b"]);
}

#[test]
fn a_name_declared_in_one_block_resolves_there() {
    let rows: [(&str, &str, &str); 2] = [
        (
            "/ModuleOnly.vue",
            "<script lang=\"ts\">\ninterface Foo { a: string }\n</script>\n\
             <script setup lang=\"ts\">\nconst unrelated = 1\n</script>\n\
             <template><div /></template>\n",
            "a",
        ),
        (
            "/SetupOnly.vue",
            "<script lang=\"ts\">\nconst unrelated = 1\n</script>\n\
             <script setup lang=\"ts\">\ninterface Foo { b: number }\n</script>\n\
             <template><div /></template>\n",
            "b",
        ),
    ];
    for (canonical, source, member) in rows {
        let host = host_with(canonical, source, FileLanguage::vue());
        assert_eq!(
            resolved_member_names(&host, canonical),
            [member],
            "{canonical}"
        );
    }
}

#[test]
fn a_type_and_a_same_named_value_in_the_other_block_resolve_the_type() {
    let source = "<script lang=\"ts\">\ninterface Foo { a: string }\n</script>\n\
                  <script setup lang=\"ts\">\nconst Foo = 1\n</script>\n\
                  <template><div /></template>\n";
    let host = host_with("/TypeAndValue.vue", source, FileLanguage::vue());
    assert_eq!(resolved_member_names(&host, "/TypeAndValue.vue"), ["a"]);
}
