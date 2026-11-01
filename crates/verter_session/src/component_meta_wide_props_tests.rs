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
    if index.is_multiple_of(2) {
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

/// Where a component's props are authored. Each shape reaches its payloads
/// through a different chain of member lists.
#[derive(Clone, Copy, Debug)]
enum Shape {
    Literal,
    InlineIntersection,
    Alias,
    AliasIntersection,
    Interface,
}

impl Shape {
    const ALL: [Self; 5] = [
        Self::Literal,
        Self::InlineIntersection,
        Self::Alias,
        Self::AliasIntersection,
        Self::Interface,
    ];

    /// Address units one prop costs: its member, its field and the hop from
    /// the field's address to its member list, plus one for the list and one
    /// for the arm hop where every prop has its own intersection arm.
    fn work_per_prop(self) -> u64 {
        match self {
            Self::Literal | Self::Alias | Self::Interface => 3,
            Self::InlineIntersection | Self::AliasIntersection => 5,
        }
    }

    fn component(self, props: usize, tag: &str) -> String {
        let member = |index: usize| format!("p{index}: {}", primitive_of(index).0);
        let members = || (0..props).map(member).collect::<Vec<_>>().join("; ");
        let arms = || {
            (0..props)
                .map(|index| format!("{{ {} }}", member(index)))
                .collect::<Vec<_>>()
                .join(" & ")
        };
        let script = match self {
            Self::Literal => format!("defineProps<{{ {} }}>()", members()),
            Self::InlineIntersection => format!("defineProps<{}>()", arms()),
            Self::Alias => format!("type Props = {{ {} }}\ndefineProps<Props>()", members()),
            Self::AliasIntersection => {
                format!("type Props = {}\ndefineProps<Props>()", arms())
            }
            Self::Interface => {
                format!("interface Props {{ {} }}\ndefineProps<Props>()", members())
            }
        };
        format!("<script setup lang=\"ts\">\n// {tag}\n{script}\n</script>\n")
    }
}

/// Every prop shape pays a constant, exact address cost per prop, whether it
/// is read through one member list or one list per prop. Re-walking a
/// macro's lists, or the file's declarations, for each field grows the cost
/// with the prop count and fails the exact increment.
#[test]
fn every_prop_shape_pays_constant_address_work_per_prop() {
    const SIZES: [usize; 3] = [16, 32, 64];
    for shape in Shape::ALL {
        let mut cold_work = Vec::new();
        for props in SIZES {
            let host = ClosingHost(VerterHost::new_standalone(HostConfig::default()));
            publish(&host, &shape.component(props, "v1"));

            let (builds, work) = demand_complete(&host, props, "cold");
            assert_eq!(builds, 1, "{shape:?} cold {props}: one assembly");
            cold_work.push(work);

            let (warm_builds, warm_work) = demand_complete(&host, props, "warm");
            assert_eq!(
                (warm_builds, warm_work),
                (0, 0),
                "{shape:?} warm {props}: the cold answer is reused"
            );

            publish(&host, &shape.component(props, "v2 with shifted spans"));
            let (edit_builds, edit_work) = demand_complete(&host, props, "edit");
            assert_eq!(edit_builds, 1, "{shape:?} edit {props}: one assembly");
            assert_eq!(edit_work, work, "{shape:?} edit {props}: the same work");
        }
        let expected = shape.work_per_prop();
        for (pair, sizes) in cold_work.windows(2).zip(SIZES.windows(2)) {
            assert_eq!(
                pair[1] - pair[0],
                expected * (sizes[1] - sizes[0]) as u64,
                "{shape:?}: {expected} units per prop: {cold_work:?} at sizes {SIZES:?}"
            );
        }
    }
}

/// A field its macro's own arguments author costs the same beside a huge
/// unrelated interface as beside a tiny one: declarations no field resolves
/// into are never indexed.
#[test]
fn unrelated_declaration_members_cost_no_address_work() {
    for macro_call in [
        "defineProps<{ value: string }>()",
        "defineProps<{ value: string } & { other: number }>()",
        "defineProps({ value: { type: String as PropType<string> } })",
    ] {
        let work_beside = |members: usize| {
            let unused = (0..members)
                .map(|index| format!("u{index}: string"))
                .collect::<Vec<_>>()
                .join("; ");
            let host = ClosingHost(VerterHost::new_standalone(HostConfig::default()));
            publish(
                &host,
                &format!(
                    "<script setup lang=\"ts\">\n\
                     import type {{ PropType }} from 'vue'\n\
                     interface Unused {{ {unused} }}\n\
                     {macro_call}\n\
                     </script>\n"
                ),
            );
            let (_, work) = address_counters(&host);
            let delivery = host
                .get_component_meta(COMPONENT)
                .expect("component-meta delivers");
            assert_eq!(delivery.analysis().props[0].name, "value", "{macro_call}");
            let (_, work_after) = address_counters(&host);
            work_after - work
        };
        assert_eq!(
            work_beside(4),
            work_beside(256),
            "{macro_call}: the unused interface's members are never visited"
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
