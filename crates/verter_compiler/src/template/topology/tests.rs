use verter_audit::observe::Uncaptured;
use verter_span::Span;

use super::*;
use crate::ast::types::TemplateAst;
use crate::work_ledger::CompilerWorkLedger;

struct Fixture {
    source: String,
    ast: TemplateAst,
}

/// The wrapper the fixture puts around a template body.
const WRAPPER_OPEN: &str = "<template>";

impl Fixture {
    fn new(template: &str) -> Self {
        let source = format!("{WRAPPER_OPEN}{template}</template>");
        let ast = crate::framework_common::vue_bridge::parse_sfc(&source, None, None)
            .template_ast
            .expect("template block");
        Self { source, ast }
    }

    fn build(&self, errors: &[Span]) -> VueTemplateTopology<'_> {
        build_template_topology(
            &self.source,
            &self.ast,
            TopologyRecoveryFacts {
                error_spans: errors,
            },
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
        .expect("topology builds")
    }

    /// The node whose authored span starts at the first occurrence of `text`.
    fn at(&self, topology: &VueTemplateTopology<'_>, text: &str) -> VueTemplateNodeId {
        let offset = self.source[WRAPPER_OPEN.len()..]
            .find(text)
            .expect("text in template");
        let start = (WRAPPER_OPEN.len() + offset) as u32;
        topology
            .nodes()
            .find(|&n| {
                topology.kind(n) != VueTemplateNodeKind::Root && topology.span(n).start == start
            })
            .unwrap_or_else(|| panic!("no node starts at {text:?}"))
    }
}

fn kinds(topology: &VueTemplateTopology<'_>) -> Vec<VueTemplateNodeKind> {
    topology.nodes().map(|n| topology.kind(n)).collect()
}

fn region_kinds(topology: &VueTemplateTopology<'_>) -> Vec<VueRegionKind> {
    topology
        .regions()
        .map(|r| topology.region_kind(r))
        .collect()
}

#[test]
fn tree_tables_follow_the_parse() {
    use VueTemplateNodeKind::*;
    let f = Fixture::new("<div><span>a</span>{{ b }}<!--c--></div>");
    let t = f.build(&[]);
    assert_eq!(
        kinds(&t),
        [Root, Element, Element, Text, Interpolation, Comment]
    );
    let div = f.at(&t, "<div");
    let span = f.at(&t, "<span");
    let interpolation = f.at(&t, "{{");
    let comment = f.at(&t, "<!--");
    assert_eq!(t.parent(span), Some(div));
    assert_eq!(t.parent(div), Some(t.root()));
    assert_eq!(t.parent(t.root()), None);
    assert_eq!(
        t.children(div).collect::<Vec<_>>(),
        [span, interpolation, comment]
    );
    assert_eq!(t.next_sibling(span), Some(interpolation));
    assert_eq!(t.next_sibling(comment), None);
    assert_eq!(t.subtree(div).count(), 5);
    assert_eq!(t.subtree(t.root()).count(), 6);
    for node in t.nodes().filter(|&n| n != t.root()) {
        let syntax = t.syntax(node).expect("parser node");
        assert_eq!(t.node_for_syntax(syntax), Some(node));
    }
    assert_eq!(t.syntax(t.root()), None);
    assert!(t.element(div).is_some());
    assert!(t.element(interpolation).is_none());
}

#[test]
fn spans_are_the_authored_extents() {
    let f = Fixture::new("<p>a</p>{{ b }}");
    let t = f.build(&[]);
    let slice = |text: &str| {
        let span = t.span(f.at(&t, text));
        &f.source[span.start as usize..span.end as usize]
    };
    assert_eq!(slice("<p"), "<p>a</p>");
    assert_eq!(slice("{{"), "{{ b }}");
    let root = t.span(t.root());
    assert_eq!(
        &f.source[root.start as usize..root.end as usize],
        "<p>a</p>{{ b }}"
    );
}

#[test]
fn tag_kinds_map_to_node_kinds() {
    let f = Fixture::new("<Comp/><slot/><template/>");
    let t = f.build(&[]);
    assert_eq!(
        kinds(&t),
        [
            VueTemplateNodeKind::Root,
            VueTemplateNodeKind::Component,
            VueTemplateNodeKind::SlotOutlet,
            VueTemplateNodeKind::Template,
        ]
    );
}

#[test]
fn empty_template_is_the_root_alone() {
    let t_empty = Fixture::new("");
    let t = t_empty.build(&[]);
    assert_eq!(t.len(), 1);
    assert_eq!(t.regions().count(), 1);
    assert_eq!(t.completeness(), RegionCompleteness::Complete);
}

#[test]
fn if_chain_keeps_authored_order() {
    let f = Fixture::new(r#"<p v-if="a"/><p v-else-if="b"/><p v-else/>"#);
    let t = f.build(&[]);
    assert_eq!(t.if_chains().count(), 1);
    let chain = t.if_chains().next().unwrap();
    let branches = t.branches(chain);
    assert_eq!(
        branches.iter().map(|b| b.role).collect::<Vec<_>>(),
        [
            VueBranchRole::If,
            VueBranchRole::ElseIf,
            VueBranchRole::Else
        ]
    );
    assert_eq!(t.if_chain(chain).owner, t.root());
    let nodes: Vec<_> = t.children(t.root()).collect();
    assert_eq!(branches.iter().map(|b| b.node).collect::<Vec<_>>(), nodes);
    for (index, branch) in branches.iter().enumerate() {
        assert_eq!(t.region_kind(branch.region), VueRegionKind::IfBranch);
        assert_eq!(t.region_owner(branch.region), Some(t.root()));
        assert_eq!(t.region_of(branch.node), branch.region);
        assert_eq!(t.chain_of(branch.node), Some((chain, index as u32)));
    }
}

#[test]
fn comments_and_whitespace_keep_a_chain_but_content_ends_it() {
    let kept = Fixture::new("<p v-if=\"a\"/>\n  <!-- c -->\n<p v-else/>");
    assert_eq!(kept.build(&[]).if_chains().count(), 1);

    let split = Fixture::new("<p v-if=\"a\"/>text<p v-else/>");
    let t = split.build(&[]);
    // The orphaned `v-else` is its own chain and cannot be trusted.
    assert_eq!(t.if_chains().count(), 2);
    let orphan = split.at(&t, "<p v-else");
    assert_eq!(
        t.region(t.region_of(orphan)).completeness,
        RegionCompleteness::Recovered
    );
    assert_eq!(t.completeness(), RegionCompleteness::Recovered);

    let separate = Fixture::new("<p v-if=\"a\"/><p v-if=\"b\"/>");
    assert_eq!(separate.build(&[]).if_chains().count(), 2);
}

#[test]
fn v_for_nests_inside_v_if_on_one_element() {
    let f = Fixture::new(r#"<li v-for="i in l" v-if="i"/>"#);
    let t = f.build(&[]);
    let li = f.at(&t, "<li");
    let body = t.region_of(li);
    assert_eq!(t.region_kind(body), VueRegionKind::ForBody);
    let branch = t.region_parent(body).unwrap();
    assert_eq!(t.region_kind(branch), VueRegionKind::IfBranch);
    assert_eq!(t.region_parent(branch), Some(t.root_region()));
    assert_eq!(
        t.for_region(li),
        Some(VueForRegion {
            region: body,
            node: li
        })
    );
    assert_eq!(t.for_regions().len(), 1);
    assert_eq!(t.region_owner(body), Some(t.root()));
    assert_eq!(
        t.enclosing_regions(li).collect::<Vec<_>>(),
        [body, branch, t.root_region()]
    );
}

#[test]
fn slot_templates_and_loose_children_get_their_own_regions() {
    let f = Fixture::new("<C><template #a>x</template>y</C>");
    let t = f.build(&[]);
    let c = f.at(&t, "<C");
    let template = f.at(&t, "<template");
    let x = f.at(&t, "x</template>");
    let y = f.at(&t, "y</C>");
    let children = t.region_of(template);
    assert_eq!(t.region_kind(children), VueRegionKind::ComponentChildren);
    assert_eq!(t.region_owner(children), Some(c));
    let slots = t.slot_regions(c);
    assert_eq!(slots.len(), 2);
    let named = slots.iter().find(|s| s.template == Some(template)).unwrap();
    let default = slots.iter().find(|s| s.implicit_default).unwrap();
    assert_eq!(t.region_of(x), named.region);
    assert_eq!(t.region_owner(named.region), Some(template));
    assert_eq!(t.region_of(y), default.region);
    assert_eq!(t.region_owner(default.region), Some(c));
    assert_eq!(default.template, None);
    // Source order: the named slot comes first.
    assert_eq!(slots[0], *named);
}

#[test]
fn conditional_slot_template_sits_in_a_branch_of_the_component_children() {
    let f = Fixture::new(r#"<C><template v-if="s" #a>x</template></C>"#);
    let t = f.build(&[]);
    let c = f.at(&t, "<C");
    let template = f.at(&t, "<template");
    let branch = t.region_of(template);
    assert_eq!(t.region_kind(branch), VueRegionKind::IfBranch);
    let parent = t.region_parent(branch).unwrap();
    assert_eq!(t.region_kind(parent), VueRegionKind::ComponentChildren);
    assert_eq!(t.region_owner(parent), Some(c));
    let slots = t.slot_regions(c);
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].template, Some(template));
    assert_eq!(t.region_parent(slots[0].region), Some(branch));
}

#[test]
fn whitespace_between_slot_templates_is_not_default_slot_content() {
    let f = Fixture::new("<C>\n  <template #a>x</template>\n  <template #b>y</template>\n</C>");
    let t = f.build(&[]);
    let c = f.at(&t, "<C");
    assert!(t.slot_regions(c).iter().all(|s| !s.implicit_default));
    assert_eq!(t.slot_regions(c).len(), 2);
}

#[test]
fn slot_declared_on_the_component_tag_has_no_template() {
    let f = Fixture::new(r#"<C v-slot="p">x</C>"#);
    let t = f.build(&[]);
    let c = f.at(&t, "<C");
    let slots = t.slot_regions(c);
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].template, None);
    assert!(!slots[0].implicit_default);
    assert_eq!(t.region_of(f.at(&t, "x</C>")), slots[0].region);
    assert!(!region_kinds(&t).contains(&VueRegionKind::ComponentChildren));
}

#[test]
fn nested_regions_report_the_innermost_first() {
    let f = Fixture::new(r#"<div v-if="a"><p v-for="i in l"><b/></p></div>"#);
    let t = f.build(&[]);
    let b = f.at(&t, "<b");
    let kinds: Vec<_> = t.enclosing_regions(b).map(|r| t.region_kind(r)).collect();
    assert_eq!(
        kinds,
        [
            VueRegionKind::ForBody,
            VueRegionKind::IfBranch,
            VueRegionKind::Root
        ]
    );
}

#[test]
fn complete_template_is_complete_everywhere() {
    let f = Fixture::new(r#"<div v-if="a"><br><input/></div><p v-else>t</p>"#);
    let t = f.build(&[]);
    assert!(t
        .regions()
        .all(|r| t.region(r).completeness == RegionCompleteness::Complete));
    assert_eq!(t.completeness(), RegionCompleteness::Complete);
}

#[test]
fn unclosed_element_marks_its_region_incomplete() {
    let f = Fixture::new("<div><span></div>");
    let t = f.build(&[]);
    let span = f.at(&t, "<span");
    assert_eq!(
        t.region(t.region_of(span)).completeness,
        RegionCompleteness::Incomplete
    );
    assert_eq!(t.completeness(), RegionCompleteness::Incomplete);
}

#[test]
fn unclosed_element_inside_a_branch_spreads_outward_only() {
    let f = Fixture::new(r#"<div v-if="a"><span></div><div v-else/>"#);
    let t = f.build(&[]);
    let chain = t.if_chains().next().unwrap();
    let branches = t.branches(chain);
    assert_eq!(
        t.region(branches[0].region).completeness,
        RegionCompleteness::Incomplete
    );
    assert_eq!(
        t.region(branches[1].region).completeness,
        RegionCompleteness::Complete
    );
    assert_eq!(
        t.region(t.root_region()).completeness,
        RegionCompleteness::Incomplete
    );
}

#[test]
fn error_span_marks_only_the_branch_it_touches() {
    let f = Fixture::new(r#"<p v-if="a">one</p><p v-else>two</p>"#);
    let at = f.source.find("one").unwrap() as u32;
    let t = f.build(&[Span::new(at, at + 1)]);
    let chain = t.if_chains().next().unwrap();
    let branches = t.branches(chain);
    assert_eq!(
        t.region(branches[0].region).completeness,
        RegionCompleteness::Recovered
    );
    assert_eq!(
        t.region(branches[1].region).completeness,
        RegionCompleteness::Complete
    );
    assert_eq!(
        t.region(t.root_region()).completeness,
        RegionCompleteness::Recovered
    );
    assert_eq!(t.completeness(), RegionCompleteness::Recovered);
}

#[test]
fn error_span_outside_the_template_marks_nothing() {
    let f = Fixture::new("<p>a</p>");
    let t = f.build(&[Span::new(f.source.len() as u32, f.source.len() as u32)]);
    assert_eq!(t.completeness(), RegionCompleteness::Complete);
}

#[test]
fn identity_and_preorder_are_deterministic() {
    let template =
        r#"<C><template v-if="s" #a><i v-for="x in y"/></template>t</C><p v-if="a"/><p v-else/>"#;
    let f = Fixture::new(template);
    assert_eq!(dump_topology(&f.build(&[])), dump_topology(&f.build(&[])));
    // A topology rebuilt from an edited snapshot equals a fresh one.
    let edited = Fixture::new(template);
    assert_eq!(
        dump_topology(&f.build(&[])),
        dump_topology(&edited.build(&[]))
    );
}

#[test]
fn dump_renders_nodes_then_regions() {
    let f = Fixture::new(r#"<div v-if="a"><b/></div><p v-else/>"#);
    let t = f.build(&[]);
    let expected = "\
Root [10..45) r0
  Element [10..34) r1 if:0.0
    Element [24..28) r1
  Element [34..45) r2 if:0.1
r0 Root parent=- owner=- Complete
r1 IfBranch parent=r0 owner=0 Complete
r2 IfBranch parent=r0 owner=0 Complete
";
    assert_eq!(dump_topology(&t), expected);
}

#[test]
fn records_are_compact() {
    assert_eq!(std::mem::size_of::<VueTemplateNode>(), 8);
    assert_eq!(std::mem::size_of::<VueRegion>(), 8);
    assert_eq!(std::mem::size_of::<VueTemplateNodeId>(), 4);
}

#[test]
fn every_region_indexes_its_own_table() {
    let f = Fixture::new(
        r#"<C><template v-if="s" #a><i v-for="x in y"/></template>t</C><p v-if="a"/><p v-else/><li v-for="z in w"/>"#,
    );
    let t = f.build(&[]);
    let mut seen: Vec<VueRegionKind> = Vec::new();
    for region in t.regions() {
        let data = t.region(region);
        if !seen.contains(&data.kind) {
            seen.push(data.kind);
        }
        match data.kind {
            VueRegionKind::Root | VueRegionKind::ComponentChildren => {
                assert_eq!(data.detail, u32::MAX)
            }
            VueRegionKind::IfBranch => {
                let chain = t.if_chains().nth(data.detail as usize).expect("chain");
                assert!(t.branches(chain).iter().any(|b| b.region == region));
            }
            VueRegionKind::ForBody => {
                assert_eq!(t.for_regions()[data.detail as usize].region, region);
            }
            VueRegionKind::SlotBody => {
                let slots: Vec<_> = t
                    .nodes()
                    .flat_map(|n| t.slot_regions(n).iter().copied())
                    .collect();
                assert_eq!(slots[data.detail as usize].region, region);
            }
        }
    }
    assert_eq!(seen.len(), 5, "fixture covers every region kind");
}

#[test]
fn self_closing_slot_template_still_declares_its_slot() {
    let f = Fixture::new("<C><template #header/></C>");
    let t = f.build(&[]);
    let c = f.at(&t, "<C");
    let template = f.at(&t, "<template");
    let slots = t.slot_regions(c);
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].template, Some(template));
    assert_eq!(t.region_owner(slots[0].region), Some(template));
}

#[test]
fn discarded_content_in_a_component_slot_body_is_recovered() {
    let f = Fixture::new("<C v-slot></oops></C>");
    let start = WRAPPER_OPEN.len() + "<C v-slot>".len();
    let end = start + "</oops>".len();
    let t = f.build(&[Span::new(start as u32, end as u32)]);
    let c = f.at(&t, "<C");
    let slots = t.slot_regions(c);
    assert_eq!(slots.len(), 1);
    assert_eq!(
        t.region(slots[0].region).completeness,
        RegionCompleteness::Recovered
    );
}
