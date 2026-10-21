use std::sync::Arc;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

fn put(host: &VerterHost, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/fixture.vue".into()),
            input_id: "/fixture.vue".into(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert");
}

// Executed unchanged by foreign consumers with support off and on.
fn main() {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn run() {
    use verter_type_engine::semantic_query::{FactStatus, PartialReasonSet};
    let mut deep = String::from("<script setup lang=\"ts\">type P0 = { x: string };\n");
    for level in 1..=30 {
        deep.push_str(&format!("type P{level} = Partial<P{}>;\n", level - 1));
    }
    deep.push_str("defineProps<P30>()</script>");
    for (budget, source, reason) in [
        (
            HostConfig::default().projection_op_budget,
            deep.as_str(),
            PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT,
        ),
        (
            1,
            "<script setup lang=\"ts\">defineProps<{ [K in 'x' | 'y']: string }>()</script>",
            PartialReasonSet::PROJECTION_WORK_LIMIT,
        ),
    ] {
        let host = VerterHost::new_standalone(HostConfig {
            projection_op_budget: budget,
            ..HostConfig::default()
        });
        put(&host, source);
        let first = host.get_component_meta("/fixture.vue").expect("refusal");
        let FactStatus::Unavailable(reasons) = first.status() else {
            panic!("{:?}", first.status());
        };
        assert!(reasons.get().contains(reason));
        assert!(!reasons.get().contains(PartialReasonSet::MISSING_DEPENDENCY));
        let (_, replay) = host
            .get_component_meta_with_resolution("/fixture.vue")
            .expect("refusal replay");
        assert_eq!(replay.fact_status(), first.status());
        assert!(replay.synthesis_should_suppress);
        host.close();
        host.release_completion()
            .wait()
            .expect("refusal releases drain");
    }

    // A caller cancellation is required state in both feature configurations.
    let host = VerterHost::new_standalone(HostConfig::default());
    put(
        &host,
        "<script setup lang=\"ts\">defineProps<{ label: string }>()</script>",
    );
    let context =
        verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
            1,
            Arc::from("/fixture.vue"),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            0,
        );
    context.cancel();
    {
        let _guard = verter_type_engine::request_context::RequestContextGuard::install(context);
        // The published contract is the same typed outcome in both feature
        // configurations: an aborted computation publishes nothing.
        assert!(
            host.get_component_meta("/fixture.vue").is_none(),
            "a cancelled request publishes nothing"
        );
    }
    let recovered = host
        .get_component_meta("/fixture.vue")
        .expect("uncancelled recovery");
    assert!(recovered.is_complete());
    assert_eq!(recovered.props[0].name, "label");
    host.close();
    host.release_completion()
        .wait()
        .expect("cancelled work drains");

    let host = VerterHost::new_standalone(HostConfig::default());
    put(
        &host,
        "<script setup lang=\"ts\">defineProps<{ label: string }>()</script>",
    );
    let first = host.get_component_meta("/fixture.vue").expect("cold");
    assert!(first.is_complete());
    let graph = host.project_type_store().semantic_graph();
    assert!(graph.node_count() > 0);
    assert!(graph.memo_entry_count() > 0);
    assert_eq!(
        first
            .props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["label"]
    );
    let warm = host.get_component_meta("/fixture.vue").expect("warm");
    assert_eq!(warm.status(), first.status());
    assert_eq!(warm.props[0].name, first.props[0].name);
    put(
        &host,
        "<script setup lang=\"ts\">defineProps<{ value: number }>()</script>",
    );
    let changed = host.get_component_meta("/fixture.vue").expect("edited");
    assert!(changed.is_complete());
    assert_eq!(
        changed
            .props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["value"]
    );
    host.close();
    host.release_completion().wait().expect("releases drain");
    assert_eq!(
        graph.memo_entry_count(),
        0,
        "production occupancy remains available after drainage"
    );
    assert_eq!(
        first.props[0].name, "label",
        "closing never revokes a delivered answer"
    );
    assert_eq!(changed.props[0].name, "value");
    assert!(host.get_component_meta("/fixture.vue").is_none());
}
