use std::sync::Arc;

use verter_audit::observe::Uncaptured;
use verter_css_syntax::{parse_style_ir, CssDialect, CssParseMode, CssSource, StyleStage};

use super::cascade::{transform_vue_style, CascadeInput};
use super::css_vars::generate_var_name;
use super::{plan_vue_styles, ScopeId, VueStyleBlockInput, VueStyleOutcome, VueStylePlan};
use crate::assembly::emit_plan::MapMode;
use crate::compile::{compute_scope_id, get_hash};
use crate::compile_request::demand::InputDemand;
use crate::style_planner::{
    parse_plain_css_for_verification, AuthoredStyleInput, StyleRewriteStage, VerifiedPlainCss,
};
use crate::work_ledger::CompilerWorkLedger;

const COMPONENT: &str = "src/Card.vue";

fn css_block<'a>(
    index: u32,
    code: &'a str,
    scoped: bool,
    module: Option<&'a str>,
) -> VueStyleBlockInput<'a> {
    VueStyleBlockInput {
        index,
        scoped,
        module,
        authored: AuthoredStyleInput::new_css(code, "probe.css", "space:probe", "artifact:probe"),
        prepared: None,
        stage: StyleStage::Authored,
    }
}

fn plan(blocks: &[VueStyleBlockInput<'_>], rewrite: bool, map_mode: MapMode) -> VueStylePlan {
    plan_vue_styles(
        COMPONENT,
        blocks,
        None,
        rewrite,
        map_mode,
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
}

#[test]
fn scope_id_matches_the_legacy_hash() {
    for id in [
        COMPONENT,
        "Card",
        "",
        "src/components/Deep/Nested.vue",
        "unicode-\u{fc}.vue",
    ] {
        let scope = ScopeId::for_component(id);
        assert_eq!(scope.hash().as_bytes(), compute_scope_id(id), "{id}");
        assert_eq!(scope.hash(), &get_hash(id)[..8], "{id}");
    }
}

#[test]
fn scope_attributes_carry_the_hash() {
    let scope = ScopeId::for_component(COMPONENT);
    assert_eq!(scope.attribute(), format!("data-v-{}", scope.hash()));
    assert_eq!(
        scope.slotted_attribute(),
        format!("data-v-{}-s", scope.hash())
    );
}

#[test]
fn a_completed_style_contributes_scope_facts_without_reparsing() {
    use super::VueComponentScopePlan;
    use crate::style_planner::{parse_ir_invocation_count, reset_parse_ir_invocation_count};
    let css = ".x :slotted(span) { color: v-bind(color) }";
    let block = css_block(0, css, true, Some(""));
    let expected = plan(&[css_block(0, css, true, Some(""))], true, MapMode::NoMap);
    let scope_id = ScopeId::for_component(COMPONENT);
    let outcome =
        super::cascade::run_vue_style_cascade(block.authored, scope_id.hash(), true, true, false);
    reset_parse_ir_invocation_count();
    let mut scope = VueComponentScopePlan::new(scope_id);
    scope.record_block(
        &block,
        0,
        true,
        &outcome.facts,
        &outcome.stage_failures,
        Some(&outcome.facts.module_classes),
    );
    assert_eq!(scope, expected.scope);
    assert_eq!(parse_ir_invocation_count(), 0);
}

#[test]
fn no_style_blocks_plan_an_empty_scope() {
    let plan = plan(&[], false, MapMode::NoMap);
    assert!(!plan.scope.has_scoped);
    assert!(!plan.scope.has_slotted);
    assert!(plan.scope.css_vars.is_empty());
    assert!(plan.scope.modules.is_empty());
    assert!(plan.scope.css_vars_complete);
    assert!(plan.blocks.is_empty());
}

#[test]
fn scoped_and_module_blocks_are_reported() {
    let blocks = [
        css_block(0, ".a { color: red; }", true, None),
        css_block(1, ".b { color: blue; }", false, Some("m")),
    ];
    let plan = plan(&blocks, false, MapMode::NoMap);
    assert!(plan.scope.has_scoped);
    assert_eq!(plan.scope.modules.len(), 1);
    assert_eq!(plan.scope.modules[0].binding_name, "m");
    assert_eq!(plan.scope.modules[0].style_index, 1);
    assert_eq!(plan.scope.modules[0].classes.len(), 1);
    assert_eq!(plan.scope.modules[0].classes[0].0, "b");
}

#[test]
fn a_bare_module_attribute_binds_style() {
    let blocks = [css_block(0, ".b { color: blue; }", false, Some(""))];
    let plan = plan(&blocks, false, MapMode::NoMap);
    assert_eq!(plan.scope.modules[0].binding_name, "$style");
}

#[test]
fn a_slotted_selector_marks_the_scope() {
    let blocks = [css_block(0, ":slotted(.a) { color: red; }", true, None)];
    let plan = plan(&blocks, false, MapMode::NoMap);
    assert!(plan.scope.has_slotted);
}

#[test]
fn a_v_bind_becomes_a_css_var_over_its_source_span() {
    let sfc = "<style>\n.a { color: v-bind(color); }\n</style>";
    let content_start = "<style>\n".len() as u32;
    let content = &sfc[content_start as usize..sfc.len() - "</style>".len()];
    let ir = parse_style_ir(
        CssSource::new(Arc::from(content), content_start).expect("source fits"),
        CssDialect::Css,
        CssParseMode::Recover,
    )
    .expect("css parses");
    let mut block = css_block(0, content, false, None);
    block.authored = block.authored.with_prepared(&ir);
    let plan = plan(&[block], false, MapMode::NoMap);
    let scope = ScopeId::for_component(COMPONENT);
    assert_eq!(plan.scope.css_vars.len(), 1);
    let var = &plan.scope.css_vars[0];
    assert_eq!(var.var_name, generate_var_name(scope.hash(), "color"));
    assert_eq!(var.expression, "color");
    assert_eq!(
        &sfc[var.expression_span.start as usize..var.expression_span.end as usize],
        "color"
    );
    assert!(plan.scope.css_vars_complete);
}

#[test]
fn an_unreadable_block_makes_the_css_vars_incomplete() {
    let blocks = [css_block(0, ".a { color: v-bind(tone)", false, None)];
    let plan = plan(&blocks, false, MapMode::NoMap);
    assert!(!plan.scope.css_vars_complete);
}

#[test]
fn plain_scoped_css_is_rewritten() {
    let code = ".a { color: red; }";
    let blocks = [css_block(0, code, true, None)];
    let plan = plan(&blocks, true, MapMode::Map);
    let scope = ScopeId::for_component(COMPONENT);
    let ir = parse_plain_css_for_verification(code, StyleRewriteStage::PostPreprocessScoping)
        .expect("plain css parses");
    let expected = transform_vue_style(
        VerifiedPlainCss::from_parsed_native_css(&ir).expect("native css"),
        CascadeInput::Authored,
        "probe.css",
        "space:probe",
        "artifact:probe",
        scope.hash(),
        false,
        true,
        true,
    );
    let VueStyleOutcome::Rewritten { code, map, .. } = &plan.blocks[0].outcome else {
        panic!("expected a rewrite, got {:?}", plan.blocks[0].outcome);
    };
    assert_eq!(code, expected.code());
    assert!(code.contains(&format!("[{}]", scope.attribute())));
    assert!(map.as_ref().is_some_and(|map| !map.segments().is_empty()));
    assert!(plan.needed_inputs().is_empty());
}

#[test]
fn scoped_scss_asks_for_the_preprocessed_stage() {
    let scss = ".a { .b { color: red; } }";
    let mut block = css_block(0, scss, true, None);
    block.authored = AuthoredStyleInput::new(
        scss,
        CssDialect::Scss,
        "probe.scss",
        "space:probe",
        "artifact:probe",
    );
    let plan = plan(&[block], true, MapMode::NoMap);
    assert_eq!(
        plan.needed_inputs(),
        vec![InputDemand::ExternalStyleStage {
            style_index: 0,
            stage: StyleStage::Preprocessed,
        }]
    );
}

#[test]
fn unscoped_plain_css_is_unchanged() {
    let blocks = [css_block(0, ".a { color: red; }", false, None)];
    let plan = plan(&blocks, true, MapMode::NoMap);
    assert!(matches!(plan.blocks[0].outcome, VueStyleOutcome::Unchanged));
}

#[test]
fn a_refused_block_does_not_hide_its_siblings() {
    let blocks = [
        css_block(0, ".a { color: v-bind(tone)", false, None),
        css_block(1, ".b { color: red; }", true, None),
    ];
    let plan = plan(&blocks, true, MapMode::NoMap);
    assert!(matches!(
        plan.blocks[0].outcome,
        VueStyleOutcome::Refused(_)
    ));
    assert!(matches!(
        plan.blocks[1].outcome,
        VueStyleOutcome::Rewritten { .. }
    ));
}

#[test]
fn no_map_mode_decodes_no_map() {
    let blocks = [css_block(0, ".a { color: red; }", true, None)];
    let plan = plan(&blocks, true, MapMode::NoMap);
    let VueStyleOutcome::Rewritten { map, .. } = &plan.blocks[0].outcome else {
        panic!("expected a rewrite");
    };
    assert!(map.is_none());
}
