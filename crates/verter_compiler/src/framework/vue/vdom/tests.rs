//! Client-tree render functions written from the shared prerequisites.

use verter_audit::observe::{ObserveExec, Uncaptured};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{ContentId, SourceId, SourceUnitId};

use super::VueVdomExecutor;
use crate::assembly::emit_plan::MapMode;
use crate::assembly::source_space::SourceSpaceKind;
use crate::compile::types::VueMacroSemanticInput;
use crate::compile_request::demand::{close_demand, DemandSeed, TargetKind};
use crate::compile_request::{ProductKind, RuntimeStyleProcessing};
use crate::emission::{QualifiedSource, SourceKey};
use crate::framework::vue::assembly::{with_target_inputs, VueTargetPrerequisiteArgs};
use crate::framework::vue::target::{VueTargetInputs, VueTargetOptions, VueTargetOutput};
use crate::target_plan::TargetExecutor;
use crate::template::topology::VueTemplateNodeKind;
use crate::types::BindingType;
use crate::work_ledger::CompilerWorkLedger;

struct FixtureName;

impl CanonicalEncode for FixtureName {
    const DOMAIN_TAG: &'static str = "verter.compiler.vdom.fixture.v1";
    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, "App.vue");
    }
}

fn qualified(text: &str) -> QualifiedSource<'_> {
    QualifiedSource {
        key: SourceKey {
            unit: SourceUnitId::from_lineage(&SourceId::from_canonical(&FixtureName), "source"),
            content: ContentId::from_content_bytes(text.as_bytes()),
            space: SourceSpaceKind::Original,
        },
        text,
    }
}

/// What one run of the client-tree target wrote: the preamble (hoisted
/// constants) followed by the render function, and the runtime helpers.
struct Rendered {
    code: String,
    helpers: Vec<&'static str>,
}

/// Render a template-only SFC through the client-tree target, with the
/// options the in-place renderer uses for a development build.
fn render(source: &str, map_mode: MapMode) -> Rendered {
    render_with(source, map_mode, false, &[])
}

/// Render through the client-tree target in inline or separate mode, with
/// the script bindings the script frontend reported.
fn render_with(
    source: &str,
    map_mode: MapMode,
    inline: bool,
    bindings: &[(&'static str, BindingType)],
) -> Rendered {
    render_in(
        source,
        map_mode,
        inline,
        bindings,
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
}

/// [`render_with`], counting the work of the whole run on `ledger`.
fn render_in<M: ObserveExec>(
    source: &str,
    map_mode: MapMode,
    inline: bool,
    bindings: &[(&'static str, BindingType)],
    ledger: &mut CompilerWorkLedger<M>,
) -> Rendered {
    with_inputs(
        source,
        map_mode,
        inline,
        bindings,
        ledger,
        |inputs, ledger| {
            let VueTargetOutput {
                render,
                preamble,
                helpers,
                ..
            } = VueVdomExecutor::execute(inputs, ledger).expect("client tree target renders");
            let mut code = String::new();
            if let Some(preamble) = preamble {
                code.push_str(preamble.write(ledger).expect("preamble plan writes").text());
            }
            code.push_str(render.write(ledger).expect("render plan writes").text());
            Rendered {
                code,
                helpers: helpers.runtime,
            }
        },
    )
}

/// Build the shared prerequisites of `source` and hand them to `run`.
fn with_inputs<M: ObserveExec, R>(
    source: &str,
    map_mode: MapMode,
    inline: bool,
    bindings: &[(&'static str, BindingType)],
    ledger: &mut CompilerWorkLedger<M>,
    run: impl for<'c> FnOnce(&'c VueTargetInputs<'c>, &mut CompilerWorkLedger<M>) -> R,
) -> R {
    let parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, None);
    let template = parsed.template_ast().expect("template block");
    let demand = close_demand(&[DemandSeed {
        product: ProductKind::RuntimeClient,
        target: TargetKind::ClientTree,
        runtime_source_map: map_mode == MapMode::Map,
        style_processing: RuntimeStyleProcessing::Complete,
        has_template: true,
        has_script: false,
        style_count: 0,
    }]);
    let bindings: rustc_hash::FxHashMap<_, _> = bindings.iter().copied().collect();
    let args = VueTargetPrerequisiteArgs {
        style_source: source,
        retained: None,
        source: qualified(source),
        template: Some(template),
        styles: parsed.style_nodes(),
        demand: &demand,
        bindings: &bindings,
        macros: &VueMacroSemanticInput::Unavailable,
        runtime_module: "vue",
        component_id: "App.vue",
        scope: None,
        prepared_styles: &[],
        style_source_name: "App.vue",
        style_processing: RuntimeStyleProcessing::Complete,
        options: VueTargetOptions {
            inline,
            is_production: false,
            comments: true,
            force_js: false,
            self_name: "App".to_string(),
            has_script: !bindings.is_empty(),
            hoist_static: true,
            cache_handlers: true,
            const_props: None,
            ref_bindable_imports: Default::default(),
            map_mode,
        },
    };
    with_target_inputs(args, ledger, run).expect("the shared prerequisites build")
}

/// Template-only components covering each construct family.
const CONSTRUCT_FAMILIES: &[(&str, &str)] = &[
    ("static element", "<template><div>hello</div></template>"),
    ("interpolation", "<template><p>{{ msg }}</p></template>"),
    (
        "nested elements and whitespace",
        "<template>\n  <div id=\"a\">\n    <span>one</span>\n    <span>two</span>\n  </div>\n</template>",
    ),
    (
        "multi-root fragment",
        "<template>\n  <h1>a</h1>\n  <p>{{ b }}</p>\n</template>",
    ),
    ("comment", "<template><div><!-- note --><p>x</p></div></template>"),
    (
        "if, else-if and else with a comment between",
        "<template><div><p v-if=\"a\">1</p><!-- gap --><p v-else-if=\"b\">2</p><p v-else>3</p></div></template>",
    ),
    (
        "keyed for",
        "<template><ul><li v-for=\"i in list\" :key=\"i\">{{ i }}</li></ul></template>",
    ),
    (
        "template fragment",
        "<template><div><template v-if=\"ok\">\n  <b>x</b>\n  <i>y</i>\n</template></div></template>",
    ),
    (
        "events with modifiers",
        "<template><button @click.stop=\"go\" @keyup.enter=\"go\">x</button></template>",
    ),
    ("class and style bindings", "<template><div :class=\"c\" :style=\"s\" class=\"k\">x</div></template>"),
    ("native model", "<template><input v-model=\"text\"></template>"),
    ("shown", "<template><p v-show=\"s\">x</p></template>"),
    ("resolved component", "<template><foo-bar :a=\"1\" /></template>"),
    (
        "component with named slots",
        "<template><Comp><template #head=\"{ t }\">{{ t }}</template><p>body</p></Comp></template>",
    ),
    (
        "slot outlet with fallback",
        "<template><div><slot name=\"n\" :p=\"v\">fb</slot></div></template>",
    ),
    ("dynamic component", "<template><component :is=\"which\" /></template>"),
    (
        "static subtree caching",
        "<template><div><p>a</p><p>b</p><span>{{ x }}</span></div></template>",
    ),
];

#[test]
fn a_static_element_is_one_block() {
    let rendered = render("<template><div>hello</div></template>", MapMode::NoMap);
    assert_eq!(
        rendered.code,
        "function render(_ctx, _cache) {\nreturn (_openBlock(), _createElementBlock(\"div\", null, \"hello\"))\n}"
    );
    assert_eq!(rendered.helpers, ["_createElementBlock", "_openBlock"]);
}

#[test]
fn a_text_only_render_writes_the_same_text_as_a_mapped_one() {
    for &(name, source) in CONSTRUCT_FAMILIES {
        assert_eq!(
            render(source, MapMode::NoMap).code,
            render(source, MapMode::Map).code,
            "{name}"
        );
    }
}

#[test]
fn listeners_are_grouped_only_when_two_can_share_a_key() {
    // One handler cannot collide, so the grouping maps are never built.
    super::events::reset_event_merge_full_scan_count();
    let single = render(
        "<template><button @click=\"a\">x</button></template>",
        MapMode::NoMap,
    );
    assert_eq!(
        super::events::event_merge_full_scan_count(),
        0,
        "{}",
        single.code
    );

    super::events::reset_event_merge_full_scan_count();
    let none = render("<template><div id=\"x\">y</div></template>", MapMode::NoMap);
    assert_eq!(
        super::events::event_merge_full_scan_count(),
        0,
        "{}",
        none.code
    );

    // Two handlers of one event can collide: the maps are built and the
    // handlers still merge into one array.
    super::events::reset_event_merge_full_scan_count();
    let dup = render(
        "<template><button @click=\"a\" @click=\"b\">x</button></template>",
        MapMode::NoMap,
    );
    assert!(
        super::events::event_merge_full_scan_count() > 0,
        "{}",
        dup.code
    );
    assert!(
        dup.code.contains("onClick: [_ctx.a, _ctx.b]"),
        "{}",
        dup.code
    );
}

#[test]
fn an_inline_model_on_a_binding_that_may_hold_a_ref_writes_through_is_ref() {
    let source =
        "<template><input v-model=\"a\"><input v-model=\"b\"><input v-model=\"c\"></template>";
    let rendered = render_with(
        source,
        MapMode::NoMap,
        true,
        &[
            ("a", BindingType::SetupLet),
            ("b", BindingType::SetupMaybeRef),
            ("c", BindingType::SetupRef),
        ],
    );
    let code = &rendered.code;
    assert!(
        code.contains("$event => (_isRef(a) ? (a).value = $event : a = $event)"),
        "{code}"
    );
    assert!(
        code.contains("$event => (_isRef(b) ? (b).value = $event : null)"),
        "{code}"
    );
    assert!(code.contains("$event => ((c.value) = $event)"), "{code}");
    assert!(
        rendered.helpers.contains(&"_isRef"),
        "{:?}",
        rendered.helpers
    );
}

/// Templates over `fmt`, `Klass` and `list` (imports Vue marks
/// `setup-maybe-ref`), `count` (a `let`), `r` (a `ref`), each with the
/// expressions Vue writes for it inline and whether Vue imports `unref`.
///
/// The expected text is `@vue/compiler-sfc` 3.6.0-rc.5
/// `compileScript({ inlineTemplate: true })` output for the same template
/// under `<script setup>import { fmt, Klass, list } from './util'; import {
/// ref } from 'vue'; let count = 0; const r = ref(1)</script>`. Layout
/// whitespace is not part of the comparison.
const INLINE_MAYBE_REF_GOLDENS: &[(&str, &str, &[&str], bool)] = &[
    (
        "reads",
        "<template><p>{{ fmt(count) }} {{ r }}</p></template>",
        &["_toDisplayString(_unref(fmt)(_unref(count))) + \" \" + _toDisplayString(r.value)"],
        true,
    ),
    (
        "writes",
        "<template><button @click=\"fmt = 3\">a</button><button @click=\"fmt++\">b</button><button @click=\"[fmt, count] = obj\">c</button></template>",
        &[
            "$event => (fmt.value = 3)",
            "$event => (fmt.value++)",
            "$event => ([fmt.value, count] = _ctx.obj)",
        ],
        false,
    ),
    (
        "constructors",
        "<template><i :title=\"new Klass().x\" :b=\"new fmt.X()\"></i></template>",
        &["title: new (_unref(Klass))().x", "b: new (_unref(fmt)).X()"],
        true,
    ),
    (
        "models",
        "<template><input v-model=\"fmt\"><input v-model=\"fmt.x\"></template>",
        &[
            "$event => (_isRef(fmt) ? (fmt).value = $event : null)",
            "[_vModelText, _unref(fmt)]",
            "$event => ((_unref(fmt).x) = $event)",
            "[_vModelText, _unref(fmt).x]",
        ],
        true,
    ),
    (
        "iterable, dynamic component, dynamic key, objects and condition",
        "<template><div v-for=\"i in list\" :key=\"i\">{{ i }}</div><component :is=\"fmt\" /><span :[fmt]=\"1\" :class=\"{ a: fmt }\" :style=\"{ fmt }\"></span><b v-if=\"fmt\">x</b></template>",
        &[
            "_renderList(_unref(list), (i) => {",
            "_resolveDynamicComponent(_unref(fmt))",
            "[_unref(fmt) || \"\"]: 1",
            "{ a: _unref(fmt) }",
            "{ fmt: _unref(fmt) }",
            "(_unref(fmt)) ? ",
        ],
        true,
    ),
    (
        "component",
        "<template><Klass :x=\"1\" /></template>",
        &["_createBlock(_unref(Klass), { x: 1 })"],
        true,
    ),
];

#[test]
fn inline_bindings_that_may_hold_a_ref_are_read_through_unref_as_vue_does() {
    let bindings = [
        ("fmt", BindingType::SetupMaybeRef),
        ("Klass", BindingType::SetupMaybeRef),
        ("list", BindingType::SetupMaybeRef),
        ("count", BindingType::SetupLet),
        ("r", BindingType::SetupRef),
    ];
    let compact = |text: &str| text.split_whitespace().collect::<String>();
    let mut failures = Vec::new();
    for &(name, source, golden, imports_unref) in INLINE_MAYBE_REF_GOLDENS {
        let rendered = render_with(source, MapMode::NoMap, true, &bindings);
        let code = compact(&rendered.code);
        for expected in golden {
            if !code.contains(&compact(expected)) {
                failures.push(format!("{name}: missing `{expected}`\n{}", rendered.code));
            }
        }
        if rendered.helpers.contains(&"_unref") != imports_unref {
            failures.push(format!(
                "{name}: `_unref` imported is {}, Vue: {imports_unref}; {:?}",
                !imports_unref, rendered.helpers
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_separate_render_function_writes_setup_bindings_through_setup() {
    let rendered = render_with(
        "<template><input v-model=\"a\"></template>",
        MapMode::NoMap,
        false,
        &[("a", BindingType::SetupLet)],
    );
    assert!(
        rendered.code.contains("$event => (($setup.a) = $event)"),
        "{}",
        rendered.code
    );
    assert!(
        !rendered.helpers.contains(&"_isRef"),
        "{:?}",
        rendered.helpers
    );
}

#[test]
fn a_kebab_tag_resolves_a_camel_case_binding() {
    let rendered = render_with(
        "<template><my-header /></template>",
        MapMode::NoMap,
        false,
        &[("myHeader", BindingType::SetupConst)],
    );
    assert!(
        rendered.code.contains("$setup[\"myHeader\"]"),
        "{}",
        rendered.code
    );
    assert!(
        !rendered.code.contains("_resolveComponent"),
        "{}",
        rendered.code
    );
}

#[test]
fn a_member_tag_appends_its_tail_to_the_matched_binding() {
    let rendered = render_with(
        "<template><Swiper.Item /></template>",
        MapMode::NoMap,
        false,
        &[("Swiper", BindingType::SetupConst)],
    );
    assert!(
        rendered.code.contains("$setup[\"Swiper\"].Item"),
        "{}",
        rendered.code
    );
}

#[test]
fn a_built_in_tag_wins_over_a_binding_named_like_it() {
    let rendered = render_with(
        "<template><Suspense><p>x</p></Suspense></template>",
        MapMode::NoMap,
        false,
        &[("Suspense", BindingType::SetupConst)],
    );
    assert!(
        !rendered.code.contains("$setup[\"Suspense\"]"),
        "{}",
        rendered.code
    );
    assert!(rendered.code.contains("_Suspense"), "{}", rendered.code);
}

#[cfg(feature = "semantic-observe")]
#[test]
fn the_client_tree_target_records_no_server_or_fine_grained_work() {
    use crate::work_ledger::WorkKind;
    let mut ledger = CompilerWorkLedger::<verter_audit::observe::Captured>::new();
    let source = "<template><div><p v-if=\"a\" @click=\"n++\">{{ a }}</p><i v-for=\"x in l\">{{ x }}</i></div></template>";
    let rendered = render_in(source, MapMode::Map, false, &[], &mut ledger);
    assert!(rendered.code.contains("_renderList"), "{}", rendered.code);
    let snapshot = ledger.snapshot().expect("capture is compiled in");
    assert_eq!(snapshot.count(WorkKind::PlanningEffectNodes), 0);
    assert_eq!(snapshot.count(WorkKind::PlanningEffectEdges), 0);
}

// Reads optional audit capture: runs in the instrumentation lane.
#[cfg(feature = "semantic-observe")]
#[test]
fn each_prerequisite_runs_once_for_a_separate_render() {
    use crate::work_ledger::WorkKind;
    let mut ledger = CompilerWorkLedger::<verter_audit::observe::Captured>::new();
    let source = "<template><div :id=\"a\"><p v-if=\"b\">{{ c }}</p></div></template>\n<style scoped>.x { color: red }</style>";
    let rendered = render_in(source, MapMode::Map, false, &[], &mut ledger);
    assert!(
        rendered.code.contains("_toDisplayString"),
        "{}",
        rendered.code
    );
    let snapshot = ledger.snapshot().expect("capture is compiled in");
    // One visit of the template builds the topology.
    assert_eq!(snapshot.count(WorkKind::StructureSourceSizedVisits), 1);
    // The template's expressions are parsed in one pass, one attempt each.
    assert_eq!(
        snapshot.count(WorkKind::ParseAuthoritativeExpressionParses),
        1
    );
    assert_eq!(snapshot.count(WorkKind::ParseExpressionAttempts), 3);
    // The style block is planned once.
    assert_eq!(snapshot.count(WorkKind::StyleBlocks), 1);
}

#[derive(Debug)]
struct RenderedSlotContent {
    array: String,
    helpers: Vec<&'static str>,
    resolved_components: Vec<super::component::ResolvedComponent>,
}

/// The content each slot owner of `source` provides, in document order:
/// every `<template v-slot>` and every component.
fn slot_contents(source: &str) -> Vec<RenderedSlotContent> {
    slot_contents_with(source, &[])
}

/// [`slot_contents`] with the script bindings the script frontend reported.
fn slot_contents_with(
    source: &str,
    bindings: &[(&'static str, BindingType)],
) -> Vec<RenderedSlotContent> {
    with_inputs(
        source,
        MapMode::NoMap,
        false,
        bindings,
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            let topology = inputs.topology;
            topology
                .nodes()
                .filter(|&node| {
                    topology.kind(node) == VueTemplateNodeKind::Component
                        || topology.element(node).is_some_and(|el| el.v_slot.is_some())
                })
                .map(|owner| {
                    let content = VueVdomExecutor::render_slot_content::<Uncaptured>(inputs, owner)
                        .expect("slot content renders");
                    let mut plan =
                        crate::assembly::emit_plan::EmitPlanBuilder::new(inputs.options.map_mode);
                    plan.push_fragment(content.array).expect("slot fragment");
                    RenderedSlotContent {
                        array: plan
                            .finish()
                            .unwrap()
                            .write(ledger)
                            .unwrap()
                            .text()
                            .to_string(),
                        helpers: content.helpers,
                        resolved_components: content.resolved_components,
                    }
                })
                .collect()
        },
    )
}

#[test]
fn slot_content_is_written_as_virtual_nodes_without_hoists_or_caches() {
    let contents = slot_contents(
        "<template><Comp><template #head=\"{ t }\"><b>{{ t }}</b></template>\n  <p>a</p>\n  <p>b</p>\n  <foo-bar /></Comp></template>",
    );
    // The component, its named slot and the content-less `<foo-bar />`.
    let [default, head, empty] = &contents[..] else {
        panic!("three slot owners: {contents:?}");
    };
    assert_eq!(empty.array, "[]");
    // The component's own children outside its named slot.
    assert!(
        default.array.starts_with('[') && default.array.ends_with(']'),
        "{default:?}"
    );
    assert!(
        default.array.contains("_createVNode(\"p\", null, \"a\""),
        "{default:?}"
    );
    assert!(default.array.contains("_component_foo_bar"), "{default:?}");
    assert!(
        !default.array.contains("#head") && !default.array.contains("_toDisplayString"),
        "{default:?}"
    );
    assert!(
        !default.array.contains("_hoisted") && !default.array.contains("_cache"),
        "{default:?}"
    );
    assert_eq!(
        default.resolved_components,
        [super::component::ResolvedComponent {
            tag: "foo-bar".to_string(),
            variable: "_component_foo_bar".to_string(),
            self_reference: false
        }]
    );
    // The named slot's children.
    // The slot prop is a template local: read bare, never through `_ctx`.
    assert!(head.array.contains("_toDisplayString(t)"), "{head:?}");
    assert!(head.array.contains("_createVNode(\"b\""), "{head:?}");
    assert!(
        !head.array.contains("<template") && !head.array.contains("<p>"),
        "{head:?}"
    );
    assert!(head.helpers.contains(&"_toDisplayString"), "{head:?}");
}

#[test]
fn a_slot_owner_with_no_content_writes_an_empty_array() {
    let contents = slot_contents("<template><Comp /></template>");
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].array, "[]");
}

/// Vue's server golden for `ssr/slot-handler-statements`: the client branch
/// of the slot creates the button with `_createVNode`, and its uncached
/// statement-list handler is a block body and a dynamic prop.
#[test]
fn slot_content_matches_the_server_golden_for_a_statement_list_handler() {
    let source = include_str!(
        "../../../../../verter_vue_conformance/corpus/cases/ssr/slot-handler-statements.vue"
    );
    let contents = slot_contents_with(
        source,
        &[
            ("a", BindingType::SetupRef),
            ("b", BindingType::SetupRef),
            ("ChildComp", BindingType::SetupConst),
        ],
    );
    let [content] = &contents[..] else {
        panic!("one slot owner: {contents:?}");
    };
    // The golden's client branch, as `ssr/ssr/slot-handler-statements.js`
    // writes it; layout whitespace is not part of the comparison.
    let golden = r#"[
          _createVNode("button", {
            onClick: $event => {$setup.a = ''; $setup.b = ''}
          }, "reset", 8 /* PROPS */, ["onClick"])
        ]"#;
    let compact = |text: &str| text.split_whitespace().collect::<String>();
    assert_eq!(compact(&content.array), compact(golden), "{content:?}");
    assert_eq!(content.helpers, ["_createVNode"], "{content:?}");
}

#[test]
fn a_single_assignment_handler_is_an_expression_body() {
    let contents = slot_contents_with(
        r#"<template><Comp><button @click="a = ''">x</button></Comp></template>"#,
        &[("a", BindingType::SetupRef)],
    );
    let [content] = &contents[..] else {
        panic!("one slot owner: {contents:?}");
    };
    assert!(
        content.array.contains("onClick: $event => ($setup.a = '')"),
        "{content:?}"
    );
}

/// Vue's server golden for `ssr/scoped-slot-branches`: in the client branch
/// of a slot, branch blocks use `_createBlock`, the interpolation is written
/// without its inner spaces, and the array puts its items on their own
/// lines.
#[test]
fn slot_content_matches_the_server_golden_for_branch_blocks() {
    let source = include_str!(
        "../../../../../verter_vue_conformance/corpus/cases/ssr/scoped-slot-branches.vue"
    );
    let contents = slot_contents_with(source, &[("ChildComp", BindingType::SetupConst)]);
    let [default, cell] = &contents[..] else {
        panic!("two slot owners: {contents:?}");
    };
    assert_eq!(default.array, "[]");
    let golden = r#"[
          (row.a)
            ? (_openBlock(), _createBlock("span", { key: 0 }, "a"))
            : (_openBlock(), _createBlock(_Fragment, { key: 1 }, [
                _createTextVNode(_toDisplayString(row.b), 1 /* TEXT */)
              ], 64 /* STABLE_FRAGMENT */))
        ]"#;
    let compact = |text: &str| text.split_whitespace().collect::<String>();
    assert_eq!(compact(&cell.array), compact(golden), "{cell:?}");
    assert!(
        cell.array.starts_with("[\n") && cell.array.ends_with("\n]"),
        "{cell:?}"
    );
    assert!(cell.array.contains("_toDisplayString(row.b)"), "{cell:?}");
    assert!(!cell.helpers.contains(&"_createElementBlock"), "{cell:?}");
}

#[test]
fn a_merged_listener_with_a_bound_member_is_a_dynamic_prop() {
    // Vue 3.5.42 lists a duplicate `onClick` merged from `@click` and a bound
    // `:onClick` in dynamicProps, since the bound value changes between
    // renders; the `@click` member keeps its cache slot.
    let code = render(
        r#"<template><div><div v-for="row in rows" :key="row.id"><button @click="pick(0)" :onClick="() => pick(row)">z</button></div><button @click="pick(0)" :onClick="h">w</button></div></template>"#,
        MapMode::NoMap,
    )
    .code;
    assert!(code.contains(r#" = ["onClick"]"#), "{code}");
    assert!(
        code.contains(
            r#"{ onClick: [_cache[0] || (_cache[0] = $event => (_ctx.pick(0))), () => _ctx.pick(row)] }, "z", 8 /* PROPS */, _hoisted_"#
        ),
        "{code}"
    );
    assert!(
        code.contains(
            r#"{ onClick: [_cache[1] || (_cache[1] = $event => (_ctx.pick(0))), _ctx.h] }, "w", 8 /* PROPS */, _hoisted_"#
        ),
        "{code}"
    );
}
