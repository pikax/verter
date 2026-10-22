//! A wide `defineProps` surface addresses every prop payload from ONE replay
//! of the file's macro assembly per file version. Each prop's authored type
//! is lowered on demand from the retained parse; the field ADDRESSES those
//! demands read are built once and shared, so a component with `N` props
//! does linear address work, not `N` assemblies of `N` props each.
//!
//! Reads the optional decl-lowering counters: runs in the instrumentation
//! lane.

use std::sync::Arc;

use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact, SemanticTypeSource};
use verter_type_expr::PrimitiveName;

use crate::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

/// Close the host even when an assertion unwinds.
struct ClosingHost(VerterHost);

impl std::ops::Deref for ClosingHost {
    type Target = VerterHost;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for ClosingHost {
    fn drop(&mut self) {
        self.0.close();
    }
}

const COMPONENT: &str = "/workspace/Wide.vue";

/// The authored primitive of prop `p{index}`: string and number alternate,
/// so a payload read from a neighbouring prop's address changes the answer.
fn primitive_of(index: usize) -> (&'static str, PrimitiveName) {
    if index % 2 == 0 {
        ("string", PrimitiveName::String)
    } else {
        ("number", PrimitiveName::Number)
    }
}

fn wide_component(props: usize, tag: &str) -> String {
    let mut source = format!("<script setup lang=\"ts\">\n// {tag}\ndefineProps<{{\n");
    for index in 0..props {
        source.push_str(&format!("  p{index}: {}\n", primitive_of(index).0));
    }
    source.push_str("}>()\n</script>\n");
    source
}

fn publish(host: &VerterHost, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: COMPONENT.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert the component");
}

/// The decl-lowering macro field-address counters `(builds, work)`.
fn address_counters(host: &VerterHost) -> (u64, u64) {
    let snapshot = host.provenance().snapshot();
    (
        snapshot.macro_field_address_builds,
        snapshot.macro_field_address_work,
    )
}

/// Demand the component's metadata and require every prop, in authored
/// order, complete and typed with its own authored primitive. Returns the
/// `(builds, work)` the demand added.
fn demand_complete(host: &VerterHost, props: usize, phase: &str) -> (u64, u64) {
    let (builds, work) = address_counters(host);
    let delivery = host
        .get_component_meta(COMPONENT)
        .expect("component-meta delivers");
    assert!(delivery.is_complete(), "{phase} {props}: complete answer");
    let published = &delivery.analysis().props;
    assert_eq!(published.len(), props, "{phase} {props}: every prop");
    for (index, prop) in published.iter().enumerate() {
        assert_eq!(prop.name, format!("p{index}"), "{phase} {props}: order");
        let expected = primitive_of(index).1;
        assert!(
            matches!(
                prop.publication.result().selected_source(),
                Some(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(p))))
                    if p == &expected
            ),
            "{phase} {props}: p{index} publishes its own {expected:?}, got {:?}",
            prop.publication.result().selected_source()
        );
    }
    let (builds_after, work_after) = address_counters(host);
    (builds_after - builds, work_after - work)
}

#[test]
fn wide_props_address_every_payload_from_one_macro_assembly() {
    const SIZES: [usize; 3] = [48, 96, 192];
    let mut cold_work = Vec::new();
    let mut assembly_builds = Vec::new();
    for props in SIZES {
        let host = ClosingHost(VerterHost::new_standalone(HostConfig::default()));
        publish(&host, &wide_component(props, "v1"));

        let (builds, work) = demand_complete(&host, props, "cold");
        eprintln!(
            "macro_field_address_builds_{props}={builds} macro_field_address_work_{props}={work}"
        );
        assembly_builds.push((props, "cold", builds));
        cold_work.push(work);

        let (warm_builds, _) = demand_complete(&host, props, "warm");
        assert_eq!(warm_builds, 0, "warm {props}: the cold answer is reused");

        // An edit is a new file version: its own retained parse, its own
        // single assembly.
        publish(
            &host,
            &wide_component(props, "v2 with shifted payload spans"),
        );
        let (edit_builds, edit_work) = demand_complete(&host, props, "edit");
        assembly_builds.push((props, "edit", edit_builds));
        assert_eq!(edit_work, work, "edit {props}: the same address work");
    }
    for (props, phase, builds) in assembly_builds {
        assert_eq!(builds, 1, "{phase} {props}: one assembly per file version");
    }
    for (pair, sizes) in cold_work.windows(2).zip(SIZES.windows(2)) {
        assert!(
            pair[0] >= sizes[0] as u64,
            "every prop is addressed: {cold_work:?} at sizes {SIZES:?}"
        );
        assert!(
            pair[1] <= 2 * pair[0] + 8,
            "address work must at most double when the props double: \
             {cold_work:?} at sizes {SIZES:?}"
        );
    }
}

/// A small component with props, emits and slots answers the same way: one
/// assembly serves every field family.
#[test]
fn a_small_component_addresses_every_field_family_from_one_assembly() {
    let host = ClosingHost(VerterHost::new_standalone(HostConfig::default()));
    publish(
        &host,
        "<script setup lang=\"ts\">\n\
         defineProps<{ a: string; b?: number }>()\n\
         defineEmits<{ (e: 'change', id: number): void; close: [] }>()\n\
         defineSlots<{ default(props: { x: string }): any }>()\n\
         </script>\n",
    );
    let (builds, _) = address_counters(&host);
    let delivery = host
        .get_component_meta(COMPONENT)
        .expect("component-meta delivers");
    assert!(delivery.is_complete());
    let meta = delivery.analysis();
    let names = |it: Vec<&str>| it.into_iter().map(str::to_string).collect::<Vec<_>>();
    assert_eq!(
        meta.props
            .iter()
            .map(|p| p.name.clone())
            .collect::<Vec<_>>(),
        names(vec!["a", "b"])
    );
    assert_eq!(
        meta.events
            .iter()
            .map(|e| e.name.clone())
            .collect::<Vec<_>>(),
        names(vec!["change", "close"])
    );
    assert_eq!(
        meta.slots
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>(),
        names(vec!["default"])
    );
    let (builds_after, _) = address_counters(&host);
    assert_eq!(
        builds_after - builds,
        1,
        "one assembly per file version addresses every family"
    );
}
