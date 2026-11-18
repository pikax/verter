//! A module the compiler's own producers wrote in full is published without
//! being parsed again; anything else still is.

use verter_audit::observe::Uncaptured;
use verter_span::Span;

use super::{publish, ArtifactContribution, AssemblyRefusal};
use crate::assembly::emit_plan::{EmitPlanBuilder, EmittedArtifact, MapMode, MapSource};
use crate::assembly::fragment::{
    take_final_module_parses, test_witness, FragmentDialect, GrammarProducer, SyntacticContract,
};
use crate::assembly::map_compose::take_source_maps_built;
use crate::assembly::plan::ProductPlan;
use crate::assembly::vue_module::VueMainDecoration;
use crate::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, ProductKind, RuntimeProductRequest,
    VueCompileRequest,
};
use crate::emission::{test_source, EmissionBuilder, QualifiedSource};
use crate::work_ledger::CompilerWorkLedger;

const SCRIPT: &str = "const msg = 'hi'\n";
const TEMPLATE: &str = "<div>{{ msg }}</div>";
/// Where `msg` sits inside `{{ msg }}`.
const TEMPLATE_MSG: u32 = 8;

fn client_plan() -> ProductPlan {
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("a client request constructs");
    ProductPlan::from_request(&request)
}

/// How the script statements enter the module.
#[derive(Clone, Copy)]
enum ScriptEntry {
    /// Through a slot, witnessed by its producer.
    Witnessed,
    /// Placed as a finished fragment with no witness.
    Unwitnessed,
}

/// A main module the decoration writes around two slots: the script's
/// statements, and the render function's returned expression.
fn module<'s>(
    mode: MapMode,
    script: &QualifiedSource<'s>,
    template: &QualifiedSource<'s>,
    entry: ScriptEntry,
) -> EmittedArtifact {
    let witness = VueMainDecoration::default().witness(FragmentDialect::JavaScript);
    let mut plan = EmitPlanBuilder::witnessed(mode, witness);
    plan.push_unmapped("import { toDisplayString as _toDisplayString } from 'vue'\n");

    let mut statements = EmissionBuilder::new(mode).with_source(script.clone());
    statements
        .push_source(Span::new(0, SCRIPT.len() as u32))
        .unwrap();
    let statements = statements.finish().unwrap();
    match entry {
        ScriptEntry::Witnessed => {
            let slot = plan.reserve_slot(SyntacticContract::StatementList);
            plan.fill_fragment(
                slot,
                statements,
                test_witness(
                    SyntacticContract::StatementList,
                    FragmentDialect::JavaScript,
                ),
            )
            .unwrap();
        }
        ScriptEntry::Unwitnessed => plan.push_fragment(statements).unwrap(),
    }

    plan.push_unmapped("export function render(_ctx) {\n  return ");
    let slot = plan.reserve_slot(SyntacticContract::Expression);
    let mut render = EmissionBuilder::new(mode).with_source(template.clone());
    render.push_unmapped("_toDisplayString(_ctx.");
    render.push_anchored("msg", TEMPLATE_MSG);
    render.push_unmapped(")");
    plan.fill_fragment(
        slot,
        render.finish().unwrap(),
        test_witness(SyntacticContract::Expression, FragmentDialect::JavaScript),
    )
    .unwrap();
    plan.push_unmapped("\n}\nexport default { render }\n");
    plan.finish()
        .unwrap()
        .write(&mut CompilerWorkLedger::<Uncaptured>::new())
        .unwrap()
}

fn contribution(
    code: &str,
    grammar: Option<crate::assembly::fragment::GrammarWitness>,
) -> ArtifactContribution<'static> {
    ArtifactContribution {
        grammar,
        kind: ProductKind::RuntimeClient,
        fragments: Vec::new(),
        code: code.to_owned(),
        emitted_imports: Vec::new(),
        dialect: FragmentDialect::JavaScript,
        source_projection_map: None,
        runtime_source_map: None,
    }
}

fn sources() -> (QualifiedSource<'static>, QualifiedSource<'static>) {
    (
        test_source("script", SCRIPT),
        test_source("template", TEMPLATE),
    )
}

#[test]
fn a_constructed_module_is_not_reparsed() {
    let (script, template) = sources();
    let written = module(MapMode::NoMap, &script, &template, ScriptEntry::Witnessed);
    let grammar = written.grammar().expect("every part was witnessed");
    assert_eq!(grammar.contract(), SyntacticContract::CompleteModule);
    assert_eq!(grammar.dialect(), FragmentDialect::JavaScript);

    take_final_module_parses();
    let set = publish(
        &client_plan(),
        vec![contribution(written.text(), written.grammar())],
    )
    .unwrap();
    assert_eq!(take_final_module_parses(), 0);
    assert_eq!(
        set.artifact(ProductKind::RuntimeClient).unwrap().code(),
        written.text()
    );
}

#[test]
fn a_supplied_fragment_is_still_parsed() {
    let (script, template) = sources();
    // A finished fragment placed without a witness takes the whole module
    // out of its producer's word.
    let written = module(MapMode::NoMap, &script, &template, ScriptEntry::Unwitnessed);
    assert_eq!(written.grammar(), None);

    take_final_module_parses();
    publish(
        &client_plan(),
        vec![contribution(written.text(), written.grammar())],
    )
    .unwrap();
    assert_eq!(take_final_module_parses(), 1);

    // Supplied bytes that do not parse are refused, whatever produced them.
    let refused = publish(&client_plan(), vec![contribution("const = ;", None)]).unwrap_err();
    assert!(matches!(refused, AssemblyRefusal::FinalParseFailed { .. }));
    assert_eq!(take_final_module_parses(), 1);

    // A witness for another dialect does not stand for this one.
    let typescript = test_witness(
        SyntacticContract::CompleteModule,
        FragmentDialect::TypeScript,
    );
    publish(
        &client_plan(),
        vec![contribution(written.text(), Some(typescript))],
    )
    .unwrap();
    assert_eq!(take_final_module_parses(), 1);
}

#[test]
fn no_map_assembly_builds_no_map() {
    let (script, template) = sources();
    take_source_maps_built();
    let written = module(MapMode::NoMap, &script, &template, ScriptEntry::Witnessed);
    publish(
        &client_plan(),
        vec![contribution(written.text(), written.grammar())],
    )
    .unwrap();
    assert!(written.map().is_none());
    assert_eq!(take_source_maps_built(), 0);

    // The mapped control: the same module asked for its map builds one.
    let mapped = module(MapMode::Map, &script, &template, ScriptEntry::Witnessed);
    let rows = [
        MapSource {
            name: "script",
            content: Some(SCRIPT),
        },
        MapSource {
            name: "template",
            content: Some(TEMPLATE),
        },
    ];
    mapped
        .map()
        .unwrap()
        .to_source_map(mapped.text(), &rows, None);
    assert_eq!(take_source_maps_built(), 1);
}

#[test]
fn a_two_fragment_module_resolves_a_token_in_each_fragment_to_its_own_source() {
    let (script, template) = sources();
    let written = module(MapMode::Map, &script, &template, ScriptEntry::Witnessed);
    let keys = written.sources();
    let script_index = keys.iter().position(|key| key.same(&script.key)).unwrap() as u32;
    let template_index = keys.iter().position(|key| key.same(&template.key)).unwrap() as u32;
    assert_ne!(script_index, template_index);

    let map = written.map().unwrap();
    let in_script = written.text().find("const msg").unwrap() as u32 + 6;
    assert_eq!(map.lookup(in_script), Some((script_index, 6)));
    let in_render = written.text().find("_ctx.msg").unwrap() as u32 + 5;
    assert_eq!(map.lookup(in_render), Some((template_index, TEMPLATE_MSG)));
}
