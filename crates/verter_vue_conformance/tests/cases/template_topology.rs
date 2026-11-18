//! The template topology over the whole conformance corpus: every parsed
//! node gets one topology node, spans nest, every non-root region has an
//! owner, and the rendering is stable across builds.

use verter_audit::observe::Uncaptured;
use verter_compiler::topology::{
    build_template_topology, dump_topology, TopologyRecoveryFacts, VueTemplateTopology,
};
use verter_compiler::work_ledger::CompilerWorkLedger;
use verter_parser::ast::types::TemplateAst;
use verter_parser::diagnostics::{SyntaxPluginContext, SyntaxPluginOptions};
use verter_parser::parser::Syntax;
use verter_parser::tokenizer::byte::tokenize_sfc;
use verter_vue_conformance::{corpus_root, Manifest};

use crate::common::case_sfc_source;

fn parse_template(source: &str) -> Option<TemplateAst> {
    let bytes = source.as_bytes();
    let options = SyntaxPluginOptions::default();
    let context = SyntaxPluginContext {
        input: source,
        bytes,
        options: &options,
        diagnostics: Vec::new(),
    };
    let mut syntax = Syntax::new(false);
    tokenize_sfc(bytes, |event| syntax.handle(&event, &context));
    syntax.take_template_ast()
}

fn build<'p>(source: &'p str, ast: &'p TemplateAst) -> VueTemplateTopology<'p> {
    build_template_topology(
        source,
        ast,
        TopologyRecoveryFacts::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .expect("topology builds")
}

#[test]
fn corpus_topologies_are_consistent_and_stable() {
    let manifest = Manifest::load(&corpus_root()).expect("load manifest");
    let mut with_template = 0;
    for case in &manifest.cases {
        let source = case_sfc_source(&case.id);
        let Some(ast) = parse_template(&source) else {
            continue;
        };
        with_template += 1;
        let topology = build(&source, &ast);
        assert_eq!(topology.nodes().count(), ast.nodes.len() + 1, "{}", case.id);
        for node in topology.nodes() {
            if let Some(parent) = topology.parent(node) {
                let (inner, outer) = (topology.span(node), topology.span(parent));
                assert!(
                    outer.start <= inner.start && inner.end <= outer.end,
                    "{}: node {} escapes its parent",
                    case.id,
                    node.index()
                );
            }
        }
        for region in topology.regions() {
            if region != topology.root_region() {
                assert!(
                    topology.region_owner(region).is_some(),
                    "{}: region {} has no owner",
                    case.id,
                    region.index()
                );
            }
        }
        assert_eq!(
            dump_topology(&topology),
            dump_topology(&build(&source, &ast)),
            "{}",
            case.id
        );
    }
    assert!(with_template > 0, "the corpus has templates");
}

#[test]
fn dump_of_a_branch_pair_is_pinned() {
    let source = r#"<template><div v-if="a"><b/></div><p v-else/></template>"#;
    let ast = parse_template(source).expect("template");
    let expected = "\
Root [10..45) r0
  Element [10..34) r1 if:0.0
    Element [24..28) r1
  Element [34..45) r2 if:0.1
r0 Root parent=- owner=- Complete
r1 IfBranch parent=r0 owner=0 Complete
r2 IfBranch parent=r0 owner=0 Complete
";
    assert_eq!(dump_topology(&build(source, &ast)), expected);
}
