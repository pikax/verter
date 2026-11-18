use verter_audit::observe::Uncaptured;
use verter_span::Span;

use super::tables::build_tables;
use super::*;
use crate::ast::types::TemplateAst;
use crate::framework_common::structure::Arena;
use crate::template::topology::{
    build_template_topology, TopologyRecoveryFacts, VueTemplateNodeKind, VueTemplateTopology,
};
use crate::work_ledger::CompilerWorkLedger;

const WRAPPER_OPEN: &str = "<template>";

struct Fixture {
    source: String,
    ast: TemplateAst,
}

impl Fixture {
    fn new(template: &str) -> Self {
        let source = format!("{WRAPPER_OPEN}{template}</template>");
        let ast = crate::framework_common::vue_bridge::parse_sfc(&source, None, None)
            .template_ast
            .expect("template block");
        Self { source, ast }
    }

    fn topology(&self, errors: &[Span]) -> VueTemplateTopology<'_> {
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

    fn build_with(
        &self,
        topology: &VueTemplateTopology<'_>,
        condense: bool,
        comments: bool,
    ) -> Result<VueCompileStructure, VueStructureRefusal> {
        build_tables(
            &self.source,
            topology,
            condense,
            comments,
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
    }

    fn build(&self) -> (VueTemplateTopology<'_>, VueCompileStructure) {
        let topology = self.topology(&[]);
        let structure = self.build_with(&topology, true, true).expect("structure");
        (topology, structure)
    }
}

/// The compile node whose authored span starts at the first `text`.
fn at(
    f: &Fixture,
    topology: &VueTemplateTopology<'_>,
    structure: &VueCompileStructure,
    text: &str,
) -> VueCompileNodeId {
    let start = (WRAPPER_OPEN.len() + f.source[WRAPPER_OPEN.len()..].find(text).unwrap()) as u32;
    let node = topology
        .nodes()
        .find(|&n| topology.kind(n) != VueTemplateNodeKind::Root && topology.span(n).start == start)
        .unwrap_or_else(|| panic!("no node starts at {text:?}"));
    structure.compile_id(node)
}

fn child_kinds(structure: &VueCompileStructure, node: VueCompileNodeId) -> Vec<VueChildKind> {
    structure.children(node).iter().map(|c| c.kind).collect()
}

fn root_kinds(f: &Fixture, condense: bool, comments: bool) -> Vec<(VueChildKind, VueChainRole)> {
    let topology = f.topology(&[]);
    let structure = f.build_with(&topology, condense, comments).unwrap();
    let root = structure.compile_id(topology.root());
    structure
        .children(root)
        .iter()
        .map(|c| (c.kind, c.chain))
        .collect()
}

// ── nodes and identity ──────────────────────────────────────────

#[test]
fn ids_share_the_topology_index() {
    let f = Fixture::new("<div><span>a</span>{{ b }}</div>");
    let (topology, structure) = f.build();
    assert_eq!(structure.len(), topology.len());
    for node in topology.nodes() {
        let id = structure.compile_id(node);
        assert_eq!(id.index(), node.index());
        assert_eq!(structure.template_id(id), node);
    }
}

#[test]
fn element_flags_come_from_the_parser_and_the_tag_tables() {
    let f = Fixture::new(
        "<br><svg><path/></svg><textarea></textarea><p>x</p><i v-memo=\"[a]\" v-once ref=\"r\" :key=\"k\" v-bind=\"o\" v-focus/><svg><foreignObject><div/></foreignObject></svg>",
    );
    let (t, s) = f.build();
    let flags = |text: &str| s.facts(at(&f, &t, &s, text)).flags;
    assert!(flags("<br").has(VueNodeFlags::VOID_ELEMENT));
    assert!(flags("<svg").has(VueNodeFlags::SVG_NAMESPACE));
    assert!(flags("<path").has(VueNodeFlags::SVG_NAMESPACE));
    assert!(flags("<textarea").has(VueNodeFlags::RAW_TEXT_CHILDREN));
    assert!(flags("<p>").has(VueNodeFlags::FULLY_STATIC));
    assert!(!flags("<br").has(VueNodeFlags::SVG_NAMESPACE));
    let i = flags("<i");
    for bit in [
        VueNodeFlags::HAS_V_MEMO,
        VueNodeFlags::HAS_V_ONCE,
        VueNodeFlags::HAS_REF,
        VueNodeFlags::HAS_KEY,
        VueNodeFlags::HAS_SPREAD,
        VueNodeFlags::HAS_CUSTOM_DIRECTIVE,
    ] {
        assert!(i.has(bit), "flag {bit:#x}");
    }
    assert!(!flags("<div").has(VueNodeFlags::SVG_NAMESPACE));
}

#[test]
fn an_incomplete_topology_is_refused() {
    let f = Fixture::new("<div><span></div>");
    let topology = f.topology(&[]);
    assert_eq!(
        f.build_with(&topology, true, true).err(),
        Some(VueStructureRefusal::IncompleteTopology)
    );
}

#[test]
fn tag_names_are_interned_once() {
    let f = Fixture::new("<div><div/></div>");
    let (t, s) = f.build();
    let outer = at(&f, &t, &s, "<div>");
    let inner = at(&f, &t, &s, "<div/>");
    assert_eq!(s.facts(outer).tag, s.facts(inner).tag);
    assert_eq!(s.tag_name(outer), Some("div"));
    assert_eq!(s.names.len(), 1);
}

// ── attributes ──────────────────────────────────────────────────

#[test]
fn attributes_are_classified_in_source_order() {
    let f = Fixture::new(
        "<a :href=\"u\" @click.stop=\"f\" v-show=\"s\" v-focus class=\"c\" v-bind=\"o\"/>",
    );
    let (t, s) = f.build();
    let a = at(&f, &t, &s, "<a");
    let attrs = s.attrs(a);
    let name = |i: usize| attrs[i].name.map(|n| s.text(n).to_string());
    let kinds: Vec<_> = attrs.iter().map(|x| x.kind).collect();
    assert_eq!(
        kinds,
        [
            VueAttrKind::Bind,
            VueAttrKind::On,
            VueAttrKind::Builtin(VueBuiltinDirective::Show),
            VueAttrKind::Custom,
            VueAttrKind::Static,
            VueAttrKind::Bind,
        ]
    );
    assert_eq!(name(0).as_deref(), Some("href"));
    assert_eq!(name(1).as_deref(), Some("click"));
    assert_eq!(attrs[1].modifier_count, 1);
    assert_eq!(name(3).as_deref(), Some("focus"));
    assert_eq!(name(4).as_deref(), Some("class"));
    assert_eq!(name(5), None);
    let value = attrs[0].value_span.unwrap();
    assert_eq!(&f.source[value.start as usize..value.end as usize], "u");
    assert!(s.find_attr(a, VueAttrKind::Static, "class").is_some());
    assert!(s.find_attr(a, VueAttrKind::Static, "id").is_none());
}

#[test]
fn dynamic_and_slot_arguments_are_read_from_the_parser() {
    let f = Fixture::new("<C :[k]=\"v\"><template #name=\"p\">x</template></C>");
    let (t, s) = f.build();
    let c = at(&f, &t, &s, "<C");
    assert!(s.attrs(c)[0].dynamic_arg);
    assert_eq!(s.attrs(c)[0].name, None);
    let slot = at(&f, &t, &s, "<template");
    let attr = &s.attrs(slot)[0];
    assert_eq!(attr.kind, VueAttrKind::Builtin(VueBuiltinDirective::Slot));
    assert_eq!(attr.name.map(|n| s.text(n)), Some("name"));
}

#[test]
fn directives_the_parser_caches_keep_their_source_position() {
    let f = Fixture::new("<p class=\"c\" v-if=\"a\" id=\"i\" v-for=\"x in l\" ref=\"r\"/>");
    let (t, s) = f.build();
    let p = at(&f, &t, &s, "<p");
    let attrs = s.attrs(p);
    let kinds: Vec<_> = attrs.iter().map(|x| x.kind).collect();
    use VueBuiltinDirective as B;
    assert_eq!(
        kinds,
        [
            VueAttrKind::Static,
            VueAttrKind::Builtin(B::If),
            VueAttrKind::Static,
            VueAttrKind::Builtin(B::For),
            VueAttrKind::Static,
        ]
    );
    assert_eq!(attrs[0].prop_index, 0);
    assert_eq!(attrs[1].prop_index, VueAttr::CACHED_PROP);
    assert_eq!(attrs[2].prop_index, 1);
    assert!(s.find_attr(p, VueAttrKind::Static, "ref").is_some());
}

// ── children ────────────────────────────────────────────────────

#[test]
fn condensing_keeps_inline_spaces_and_drops_newlines_between_elements() {
    use VueChildKind::*;
    let f = Fixture::new("<div> <a/> <b/> </div>");
    let (t, s) = f.build();
    let div = at(&f, &t, &s, "<div");
    assert_eq!(child_kinds(&s, div), [Element, WhitespaceSpace, Element]);
    let f = Fixture::new("<div>\n<a/>\n<b/>\n</div>");
    let (t, s) = f.build();
    let div = at(&f, &t, &s, "<div");
    assert_eq!(child_kinds(&s, div), [Element, Element]);
}

#[test]
fn whitespace_beside_text_or_interpolation_stays() {
    use VueChildKind::*;
    let f = Fixture::new("<div>a\n<b/>\n{{ c }}</div>");
    let (t, s) = f.build();
    let div = at(&f, &t, &s, "<div");
    assert_eq!(
        child_kinds(&s, div),
        [Text, Element, WhitespaceNewline, Interpolation]
    );
}

#[test]
fn comments_and_elements_drop_the_whitespace_between_them() {
    use VueChildKind::*;
    let f = Fixture::new("<div><a/> <!--x--> <!--y--> <b/></div>");
    let (t, s) = f.build();
    let div = at(&f, &t, &s, "<div");
    assert_eq!(child_kinds(&s, div), [Element, Comment, Comment, Element]);
}

#[test]
fn preserving_keeps_interior_whitespace_and_drops_the_edges() {
    use VueChildKind::*;
    let f = Fixture::new("<div>\n<a/>\n<b/>\n</div>");
    let t = f.topology(&[]);
    let s = f.build_with(&t, false, true).unwrap();
    let div = at(&f, &t, &s, "<div");
    assert_eq!(child_kinds(&s, div), [Element, WhitespaceNewline, Element]);
}

#[test]
fn chain_branches_drop_what_sits_between_them() {
    use VueChainRole::*;
    let f = Fixture::new("<p v-if=\"a\"/> <!--x--> <p v-else/>");
    assert_eq!(
        root_kinds(&f, true, true),
        [
            (VueChildKind::Element, Start),
            (VueChildKind::Element, Continuation)
        ]
    );
    let f = Fixture::new("<p v-if=\"a\"/><p v-if=\"b\"/>");
    assert_eq!(
        root_kinds(&f, true, true),
        [
            (VueChildKind::Element, Start),
            (VueChildKind::Element, Start)
        ]
    );
}

#[test]
fn comments_are_dropped_when_not_wanted() {
    let f = Fixture::new("<div><!--x--><a/></div>");
    let t = f.topology(&[]);
    let s = f.build_with(&t, true, false).unwrap();
    let div = at(&f, &t, &s, "<div");
    assert_eq!(child_kinds(&s, div), [VueChildKind::Element]);
}

#[test]
fn a_dropped_comment_merges_the_whitespace_around_it() {
    use VueChildKind::*;
    let f = Fixture::new(
        "<div><a/>
<!--x-->
<b/></div>",
    );
    let t = f.topology(&[]);
    let s = f.build_with(&t, true, false).unwrap();
    let div = at(&f, &t, &s, "<div");
    assert_eq!(child_kinds(&s, div), [Element, Element]);
}

#[test]
fn preformatted_content_keeps_its_whitespace() {
    use VueChildKind::*;
    for (template, tag) in [
        ("<pre>   </pre>", "<pre"),
        ("<textarea>   </textarea>", "<textarea"),
        ("<div v-pre> <a/> </div>", "<div"),
    ] {
        let f = Fixture::new(template);
        let (t, s) = f.build();
        let node = at(&f, &t, &s, tag);
        assert!(!s.children(node).is_empty(), "{template}");
    }
    let f = Fixture::new("<pre><b> </b></pre>");
    let (t, s) = f.build();
    let b = at(&f, &t, &s, "<b");
    assert_eq!(child_kinds(&s, b), [Text]);
}

#[test]
fn a_namespace_reaches_every_descendant_until_an_integration_point() {
    let f = Fixture::new(
        "<svg><a><circle/></a><foreignObject><div/></foreignObject></svg><math><mi/></math>",
    );
    let (t, s) = f.build();
    let flags = |text: &str| s.facts(at(&f, &t, &s, text)).flags;
    assert!(flags("<a>").has(VueNodeFlags::SVG_NAMESPACE));
    assert!(flags("<circle").has(VueNodeFlags::SVG_NAMESPACE));
    assert!(flags("<foreignObject").has(VueNodeFlags::SVG_NAMESPACE));
    assert!(!flags("<div").has(VueNodeFlags::SVG_NAMESPACE));
    assert!(flags("<mi").has(VueNodeFlags::MATHML_NAMESPACE));
}

// ── walk ────────────────────────────────────────────────────────

struct Recorder {
    visits: Vec<String>,
    skip: Option<usize>,
}

impl VueTemplateVisitor for Recorder {
    fn enter_template(&mut self) {
        self.visits.push("enter_template".into());
    }
    fn leave_template(&mut self) {
        self.visits.push("leave_template".into());
    }
    fn enter_element(&mut self, node: VueCompileNodeId, _: VueTemplateNodeKind) -> VueWalkControl {
        self.visits.push(format!("enter_element({})", node.index()));
        if self.skip == Some(node.index()) {
            VueWalkControl::SkipChildren
        } else {
            VueWalkControl::Continue
        }
    }
    fn leave_element(&mut self, node: VueCompileNodeId, _: VueTemplateNodeKind) {
        self.visits.push(format!("leave_element({})", node.index()));
    }
    fn visit_text(&mut self, node: VueCompileNodeId) {
        self.visits.push(format!("visit_text({})", node.index()));
    }
    fn visit_interpolation(&mut self, node: VueCompileNodeId) {
        self.visits
            .push(format!("visit_interpolation({})", node.index()));
    }
    fn visit_comment(&mut self, node: VueCompileNodeId) {
        self.visits.push(format!("visit_comment({})", node.index()));
    }
}

fn walk(template: &str, skip: Option<usize>) -> Vec<String> {
    let f = Fixture::new(template);
    let (t, s) = f.build();
    let mut recorder = Recorder {
        visits: Vec::new(),
        skip,
    };
    walk_template(&t, &s, &mut recorder);
    recorder.visits
}

#[test]
fn empty_template_only_enters_and_leaves() {
    assert_eq!(walk("", None), ["enter_template", "leave_template"]);
}

#[test]
fn single_text_node() {
    assert_eq!(
        walk("hello", None),
        ["enter_template", "visit_text(1)", "leave_template"]
    );
}

#[test]
fn element_with_text_child() {
    assert_eq!(
        walk("<div>hello</div>", None),
        [
            "enter_template",
            "enter_element(1)",
            "visit_text(2)",
            "leave_element(1)",
            "leave_template"
        ]
    );
}

#[test]
fn nested_elements_walk_depth_first() {
    assert_eq!(
        walk("<div><span>text</span></div>", None),
        [
            "enter_template",
            "enter_element(1)",
            "enter_element(2)",
            "visit_text(3)",
            "leave_element(2)",
            "leave_element(1)",
            "leave_template"
        ]
    );
}

#[test]
fn sibling_elements_walk_in_order() {
    assert_eq!(
        walk("<a></a><b></b>", None),
        [
            "enter_template",
            "enter_element(1)",
            "leave_element(1)",
            "enter_element(2)",
            "leave_element(2)",
            "leave_template"
        ]
    );
}

#[test]
fn interpolation_is_visited() {
    assert_eq!(
        walk("<div>{{ foo }}</div>", None),
        [
            "enter_template",
            "enter_element(1)",
            "visit_interpolation(2)",
            "leave_element(1)",
            "leave_template"
        ]
    );
}

#[test]
fn comment_is_visited() {
    assert_eq!(
        walk("<!-- hello -->", None),
        ["enter_template", "visit_comment(1)", "leave_template"]
    );
}

#[test]
fn mixed_children_are_all_visited() {
    assert_eq!(
        walk("<div>text{{ x }}<!-- c --></div>", None),
        [
            "enter_template",
            "enter_element(1)",
            "visit_text(2)",
            "visit_interpolation(3)",
            "visit_comment(4)",
            "leave_element(1)",
            "leave_template"
        ]
    );
}

#[test]
fn skip_children_still_calls_leave() {
    assert_eq!(
        walk("<div><span/></div><p/>", Some(1)),
        [
            "enter_template",
            "enter_element(1)",
            "leave_element(1)",
            "enter_element(3)",
            "leave_element(3)",
            "leave_template"
        ]
    );
}

// ── verification and dump ───────────────────────────────────────

const SAMPLES: &[&str] = &[
    "",
    "<div><span>a</span>{{ b }}<!--c--></div>",
    "<p v-if=\"a\"/><!--x--><p v-else-if=\"b\"/><p v-else/>",
    "<li v-for=\"i in l\" v-if=\"i\">{{ i }}</li>",
    "<C><template #a=\"p\">x</template>y</C>",
    "<ul>\n  <li :k=\"1\" @click.once=\"f\">a</li>\n  <li>b</li>\n</ul>",
    "<svg><g><path d=\"\"/></g></svg><textarea>t</textarea>",
];

#[test]
fn verify_passes_on_representative_templates() {
    for template in SAMPLES {
        let f = Fixture::new(template);
        let (t, s) = f.build();
        verify_structure(&s, &t).unwrap_or_else(|e| panic!("{template}: {e}"));
    }
}

#[test]
fn verify_reports_a_planted_bad_child() {
    let f = Fixture::new("<div><span/></div><p/>");
    let (t, mut s) = f.build();
    let div = at(&f, &t, &s, "<div");
    let p = at(&f, &t, &s, "<p");
    let planted = s
        .children
        .append([VueChildRecord {
            node: p,
            kind: VueChildKind::Element,
            chain: VueChainRole::None,
        }])
        .unwrap();
    let mut nodes = Arena::new();
    for id in s.nodes.ids().collect::<Vec<_>>() {
        let mut facts = s.facts(id);
        if id == div {
            facts.children = planted;
        }
        nodes.push(facts).unwrap();
    }
    s.nodes = nodes;
    let error = verify_structure(&s, &t).unwrap_err();
    assert!(error.contains("is not a child of node"), "{error}");
}

#[test]
fn the_dump_is_stable_and_shows_the_tables() {
    let f = Fixture::new("<div v-if=\"a\"><b/></div><p v-else/>");
    let (t, s) = f.build();
    let first = dump_structure(&s, &t);
    assert_eq!(first, dump_structure(&s, &t));
    assert!(first.contains("[Builtin(If)]"), "{first}");
    assert!(first.contains("Continuation"), "{first}");
}
