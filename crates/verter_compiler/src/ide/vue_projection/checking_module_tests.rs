//! Unit tests for the checking module module.

use std::sync::Arc;

use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};

use super::checking_module::*;
use crate::code_transform::{MappingSpan, ProjectedClass};
use crate::framework_common::projection_plan::ExpressionKind;
use crate::framework_common::registered_carrier_projection::project_registered_accepted;
use crate::framework_common::vue_projection_backend::VueProjectionBackend;
use crate::framework_common::FrameworkParseArtifact;
use crate::ide::projection_recovery::{ProjectionRegion, RegionState};
use crate::ide::vue_projection::checking_fragment::{CheckingFragment, ObligationFamily, UseHost};
use crate::ide::vue_projection::external_sources::{
    BoundExternalBlock, ExternalBlockKind, ExternalBlockProjectionBinding, ExternalLegality,
    ProcessedSourceChain,
};
use crate::ide::vue_projection::script_setup::{SetupProjectionRefusal, SourceRange};

const CANONICAL: &str = "file:///src/Comp.vue";

#[test]
fn nested_ref_objects_check_the_receiving_value_type() {
    let source = concat!(
        "<script setup lang=\"ts\">import Child from './Child.vue'; ",
        "import { shallowRef } from 'vue'; ",
        "const targets = { child: shallowRef<InstanceType<typeof Child> | null>(null) };</script>",
        "<template><Child :ref=\"targets.child\" /></template>",
    );
    let module = composed(source);
    let output = module.checking_text();
    assert!(!module
        .withheld
        .iter()
        .any(|product| product.family == ObligationFamily::TemplateRef));
    let at = offset_of(output, "__VerterDynamicRef<typeof __VerterUse_");
    assert!(
        output[at..].contains("| null>()(__VerterCtx.targets.child)"),
        "{output}"
    );
    // ShallowUnwrapRef keeps targets.child as Ref: the generated assignability
    // conditional must compare the nullable instance to that ref's value.
    assert!(
        output.contains("[V] extends [T extends import(\"vue\").Ref<infer R> ? R : T]"),
        "{output}"
    );
    assert_eq!(
        module.obligation_at(at as u32).unwrap().family,
        ObligationFamily::TemplateRef
    );
    assert_checking_syntax(&module);
}

#[test]
fn correlated_component_refs_omit_absent_witnesses_and_keep_native_checks() {
    let source = concat!(
        "<script setup lang=\"ts\">import Child from './Child.vue'; ",
        "import { useTemplateRef } from 'vue'; ",
        "const panel = useTemplateRef<InstanceType<typeof Child>>('panel'); ",
        "const input = useTemplateRef<HTMLInputElement>('input'); ",
        "const choice = { component: Child, props: {} };</script>",
        "<template><component :is=\"choice.component\" v-bind=\"choice.props\" ref=\"panel\" />",
        "<input ref=\"input\" /></template>",
    );
    let module = composed(source);
    let output = module.checking_text();
    assert!(module.withheld.iter().any(|product| {
        product.family == ObligationFamily::TemplateRef
            && product.reason == WithheldReason::Incomplete
    }));
    assert!(!output.contains("typeof __VerterUse_"), "{output}");
    assert!(
        output.contains("null! as (__VerterNativeElement<\"input\", \"html\">) | null"),
        "{output}"
    );
    assert!(!module.obligations.iter().any(|obligation| {
        obligation.family == ObligationFamily::TemplateRef
            && matches!(obligation.host, Some(UseHost::Component(_)))
    }));
    assert_checking_syntax(&module);
}

#[test]
fn component_bound_refs_keep_primary_identifier_correspondence() {
    for expression in ["el => el?.missing()", "targets.missing"] {
        let source = format!(
            "<script setup lang=\"ts\">import Child from './Child.vue'; const targets = {{}};</script><template><Child :ref=\"{expression}\" /></template>"
        );
        let module = composed(&source);
        let output = module.checking_text();
        let generated = offset_of(output, "missing") as u32;
        let authored = offset_of(&source, "missing") as u32;
        for column in 0.."missing".len() as u32 {
            let mapped = module
                .emission
                .mapping()
                .projected_at(generated + column)
                .unwrap();
            assert!(
                matches!(
                    mapped.class,
                    ProjectedClass::Identity | ProjectedClass::Relocated
                ),
                "the authored identifier is copied byte for byte: {mapped:?}"
            );
            let carrier = mapped.carrier.unwrap();
            assert_eq!(
                carrier.start + generated + column - mapped.generated.start,
                authored + column
            );
        }
        assert_checking_syntax(&module);
    }
}

#[test]
fn template_ref_checks_follow_the_specialized_use_and_map_to_the_parser_key() {
    let source = concat!(
        "<script setup lang=\"ts\">import Child from './Child.vue'; ",
        "import { useTemplateRef as templateRef } from 'vue'; ",
        "const target = templateRef<InstanceType<typeof Child>>('panel'); const visible = true;</script>",
        "<template><Child v-if=\"visible\" ref=\"panel\" /></template>",
    );
    let module = composed(source);
    assert!(!module
        .withheld
        .iter()
        .any(|product| product.family == ObligationFamily::TemplateRef));
    let output = module.checking_text();
    let at = offset_of(output, "__VerterTemplateRef(__VerterRef_0.value,");
    assert!(offset_of(output, "if (__VerterCtx.visible)") < at);
    assert!(offset_of(output, "= new (") < at, "{output}");
    let target = at + "__VerterTemplateRef(".len();
    let mapped = module
        .emission
        .mapping()
        .projected_at(target as u32)
        .unwrap()
        .carrier
        .unwrap();
    assert_eq!(mapped.start, offset_of(source, "'panel'") as u32);
    let value = at + output[at..].find("null! as").unwrap();
    assert_eq!(
        module
            .emission
            .mapping()
            .projected_at(value as u32)
            .unwrap()
            .carrier
            .unwrap()
            .start,
        offset_of(source, "'panel'") as u32
    );
    assert_eq!(
        module.obligation_at(at as u32).unwrap().family,
        ObligationFamily::TemplateRef
    );
    let alloc = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, output, oxc_span::SourceType::ts()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{output}",
        parsed.diagnostics
    );
}

#[test]
fn native_models_check_context_writes_with_source_anchors_and_guard_reentry() {
    let source = concat!(
        "<script setup lang=\"ts\">import { queryField } from './query'; import { ref, computed } from 'vue'; ",
        "const search = queryField('q', ''); const user = ref<{ text: string } | null>(null); const total = computed(() => 1);</script>",
        "<template><input v-model.number.trim.lazy=\"search\" />",
        "<textarea v-if=\"user\" v-model=\"user.text\" /><input v-model=\"total\" /></template>"
    );
    let module = composed(source);
    assert!(!module
        .withheld
        .iter()
        .any(|p| p.family == ObligationFamily::NativeModel));
    let output = module.checking_text();
    assert!(output.contains("__VerterCtx.search = null! as"), "{output}");
    assert!(
        output.contains("if (!(__VerterCtx.user)) throw undefined;"),
        "{output}"
    );
    assert!(
        output.contains("__VerterModelWrite(__VerterCtx"),
        "{output}"
    );
    assert!(module
        .obligations
        .iter()
        .any(|o| o.family == ObligationFamily::NativeModel));
    for (generated, authored) in [
        (
            "__VerterCtx.search = null!",
            "v-model.number.trim.lazy=\"search",
        ),
        ("__VerterCtx.user.text = null!", "v-model=\"user.text"),
        ("__VerterCtx.total = null!", "v-model=\"total"),
    ] {
        // The context qualifier is scaffolding; the write target's authored
        // name maps byte for byte.
        let at = (offset_of(output, generated) + "__VerterCtx.".len()) as u32;
        let expected = (offset_of(source, authored) + authored.rfind('"').unwrap() + 1) as u32;
        let mapped = module.emission.mapping().projected_at(at).unwrap();
        let carrier = mapped.carrier.expect("the authored write target is mapped");
        assert_eq!(carrier.start + (at - mapped.generated.start), expected);
    }
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, output, oxc_span::SourceType::ts())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{output}",
        parsed.diagnostics
    );
}

#[test]
fn directives_check_builtin_values_in_their_existing_scope() {
    let source = concat!(
        "<script setup lang=\"ts\">import Child from './Child.vue'; const state = { visible: true, text: 'hello', html: '<b/>', componentHtml: '<i/>', componentText: 'child', memo: [1], dep: 1 };</script>",
        "<template><div v-show=\"state.visible\" v-text=\"state.text\" />",
        "<div v-html=\"state.html\" v-memo=\"state.memo\">{{ state.dep + 1 }}</div>",
        "<Child v-html=\"state.componentHtml\" /><Child v-text=\"state.componentText\" />",
        "<div v-pre>{{ missing.literal }}</div><i v-once v-cloak /></template>",
    );
    let module = composed(source);
    assert!(
        !module
            .withheld
            .iter()
            .any(|product| product.family == ObligationFamily::Directive),
        "built-in directives are checked"
    );
    for expression in [
        "state.visible",
        "state.text",
        "state.html",
        "state.memo",
        "state.componentHtml",
        "state.componentText",
    ] {
        assert_expression_pieces_at_source(&module, source, expression);
    }
    let text = module.checking_text();
    assert!(!text.contains("missing.literal"));
    assert!(text.contains("(__VerterCtx.state.dep + 1)"), "{text}");
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, text, oxc_span::SourceType::ts()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{text}",
        parsed.diagnostics
    );
}

#[test]
fn directives_resolve_local_and_global_bindings_and_preserve_arguments() {
    let source = concat!(
        "<script setup lang=\"ts\">",
        "import type { Directive } from 'vue';",
        "import { shallowRef } from 'vue';",
        "import Child from './Child.vue';",
        "const vPaint = null! as Directive<SVGCircleElement, string, 'lazy', 'fg'>;",
        "const vRefPaint = shallowRef<Directive<HTMLDivElement, string>>(null!);",
        "const color = 'red'; const arg = 'fg';",
        "</script><template>",
        "<svg><circle v-paint:fg.lazy=\"color\" /><discard v-focus /></svg>",
        "<math><mi v-global:[arg].lazy=\"color + '!'\" /></math>",
        "<input v-focus />",
        "<Child v-paint:fg=\"color + '?'\" />",
        "<div v-ref-paint=\"'red'\" />",
        "</template>",
    );
    let module = composed(source);
    assert!(!module
        .withheld
        .iter()
        .any(|p| p.family == ObligationFamily::Directive));
    let text = module.checking_text();
    for host in [
        "(SVGElementTagNameMap & Record<string, SVGElement>)[\"circle\"]",
        "(SVGElementTagNameMap & Record<string, SVGElement>)[\"discard\"]",
        "(MathMLElementTagNameMap & Record<string, MathMLElement>)[\"mi\"]",
        "(HTMLElementTagNameMap & Record<string, HTMLElement>)[\"input\"]",
    ] {
        assert!(text.contains(host), "{text}");
    }
    assert!(
        text.contains("___VERTER___shallowUnwrapRef({ vPaint }).vPaint"),
        "{text}"
    );
    assert!(
        text.contains("___VERTER___runCustomDirective(__VerterUse_"),
        "{text}"
    );
    assert!(
        text.contains("___VERTER___shallowUnwrapRef({ vRefPaint }).vRefPaint"),
        "{text}"
    );
    assert!(
        text.contains("___VERTER___retrieveSetupDirectives({})[\"vGlobal\"]"),
        "{text}"
    );
    assert_expression_pieces_at_source(&module, source, "color + '!'");
    assert_eq!(
        mapped_point_after(&module, "(__VerterCtx.color), ", "\"fg\""),
        (offset_of(source, ":fg.lazy") + 1) as u32,
    );
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, text, oxc_span::SourceType::ts()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{text}",
        parsed.diagnostics
    );
}

#[test]
fn directives_omit_withheld_component_hosts_and_keep_native_checks() {
    for native in ["", "<div v-paint=\"'native'\" />"] {
        let source = format!(
            "<script setup lang=\"ts\">import Child from './Child.vue'; const choice = {{ component: Child, props: {{}} }}; const vPaint = () => {{}};</script><template><component :is=\"choice.component\" v-bind=\"choice.props\" v-paint=\"'component'\" />{native}</template>"
        );
        let module = composed(&source);
        assert!(module.withheld.iter().any(|product| {
            product.family == ObligationFamily::Directive
                && product.reason == WithheldReason::Incomplete
        }));
        let text = module.checking_text();
        assert!(!text.contains("runCustomDirective(__VerterUse_"), "{text}");
        assert_eq!(
            text.contains("___VERTER___runCustomDirective("),
            !native.is_empty(),
            "{text}"
        );
        assert_eq!(
            text.contains("declare function ___VERTER___runCustomDirective<"),
            !native.is_empty(),
            "{text}"
        );
        assert!(!text.contains("@verter/types"), "{text}");
        if !native.is_empty() {
            assert_expression_pieces_at_source(&module, &source, "'native'");
        }
    }
}

/// Template expressions are authored copies: every byte maps to its authored
/// byte, so a diagnostic inside one lands where it was written. Context
/// qualification has no correspondence.
fn assert_expression_pieces_at_source(module: &CheckingModule, source: &str, expression: &str) {
    assert_maps_verbatim(module, source, expression);
}

fn artifact(source: &str) -> FrameworkParseArtifact {
    let language = verter_language::FileLanguage::vue();
    let sources = RegisteredSourceAuthority::new().expect("source authority");
    let snapshot = sources
        .register_source(
            CanonicalFileId::new(CANONICAL),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            language.clone(),
            Arc::from(source),
        )
        .expect("registered source");
    let grammars = CarrierGrammarAuthority::new().expect("grammar authority");
    let config =
        crate::framework_common::vue_grammar_config("{{", "}}", std::iter::empty::<&str>())
            .expect("vue");
    grammars
        .register_carrier_grammar(
            language,
            FrameworkAdapterSemanticVersion::new(1).expect("adapter version"),
            CarrierParserGrammarVersion::new(1).expect("grammar version"),
            config.clone(),
        )
        .expect("grammar registration");
    let accepted = grammars
        .accept_registered_source(&sources, &snapshot, &config)
        .expect("accepted source");
    project_registered_accepted(&accepted)
        .expect("registered projection")
        .into_framework_parse_artifact()
}

fn compose(source: &str) -> Result<CheckingModule, CheckingModuleRefusal> {
    compose_with(source, false)
}

fn compose_with(source: &str, declarations: bool) -> Result<CheckingModule, CheckingModuleRefusal> {
    let artifact = artifact(source);
    compose_checking_module(&CheckingModuleInput {
        source,
        artifact: &artifact,
        canonical_id: CANONICAL,
        custom_elements: None,
        external: None,
        declarations,
    })
}

fn composed(source: &str) -> CheckingModule {
    compose(source).expect("the carrier composes")
}

/// The composition with its declaration product.
fn composed_declaring(source: &str) -> CheckingModule {
    compose_with(source, true).expect("the carrier composes")
}

#[test]
fn global_component_fallbacks_carry_the_shared_ambient_helper_contracts() {
    for script in [
        "",
        "<script lang=\"ts\">export default {};</script>",
        "<script setup lang=\"ts\">const label = 'value';</script>",
    ] {
        let source = format!("{script}<template><GlobalChild label=\"value\"/><global-child label=\"value\"/></template>");
        let module = composed(&source);
        let text = module.checking_text();
        assert_eq!(
            text.matches("declare function ___VERTER___globalComponentsNav()")
                .count(),
            1,
            "{text}"
        );
        assert!(
            text.contains("declare type ___VERTER___GlobalComponentType<N>"),
            "{text}"
        );
        assert!(
            text.contains("declare type ___VERTER___GlobalComponentKebabType<N, K extends string>"),
            "{text}"
        );
        assert!(
            text.contains(
                "const GlobalChild = {} as ___VERTER___GlobalComponentType<'GlobalChild'>"
            ),
            "{text}"
        );
        assert!(
            text.contains("void ___VERTER___globalComponentsNav().GlobalChild"),
            "{text}"
        );
        assert_checking_syntax(&module);
    }
    let local = composed("<script setup lang=\"ts\">import Child from './Child.vue';</script><template><Child /></template>");
    assert!(!local
        .checking_text()
        .contains("___VERTER___GlobalComponent"));
}

#[test]
fn builtins_use_vue_contracts_and_preserve_nested_slot_scope() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "import Child from './Child.vue';\n",
        "const target = 42; const label = 'inside'; const child = null;\n",
        "</script>\n<template>",
        "<Teleport :to=\"target\">{{ label.toUpperCase() }}</Teleport>",
        "<Transition @enter=\"el => el.getBoundingClientRect()\"><Child /></Transition>",
        "<Suspense><template #default>{{ label.length }}</template>",
        "<template #fallback>{{ label.slice(0) }}</template></Suspense>",
        "<KeepAlive><Child ref=\"child\" :value=\"label\" /></KeepAlive>",
        "</template>",
    );
    let module = composed(source);
    let text = module.checking_text();
    for name in ["Teleport", "Transition", "Suspense", "KeepAlive"] {
        assert!(
            text.contains(&format!("typeof import(\"vue\").{name}")),
            "{text}"
        );
        assert!(
            !text.contains(&format!("GlobalComponentType<'{name}'>")),
            "{text}"
        );
    }
    for expression in ["label.toUpperCase()", "label.length", "label.slice(0)"] {
        assert_maps_expression(&module, source, expression);
    }
    assert!(text.contains("__VerterCtx.target"), "{text}");
    assert!(text.contains("el => el.getBoundingClientRect()"), "{text}");
    assert!(text.contains("__VerterSlotName("), "{text}");
    for prelude in [
        crate::ide::vue_projection::generic_interop::ForeignComponentContractAdapter::DECLARATIONS,
        crate::ide::vue_projection::provided_slots::PROVIDED_SLOT_PRELUDE.text,
    ] {
        assert_eq!(text.matches(prelude).count(), 1, "{text}");
    }
    assert!(!module
        .withheld
        .iter()
        .any(|p| p.family == ObligationFamily::Builtin));
    assert_checking_syntax(&module);
}

fn assert_checking_syntax(module: &CheckingModule) {
    let _ = checking_default_count(module);
}

fn checking_default_count(module: &CheckingModule) -> usize {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = verter_parser::oxc_parse::Parser::new(
        &allocator,
        module.checking_text(),
        oxc_span::SourceType::tsx(),
    )
    .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{}",
        parsed.diagnostics,
        module.checking_text()
    );
    parsed
        .program
        .body
        .iter()
        .map(|statement| {
            if matches!(
                statement,
                oxc_ast::ast::Statement::ExportDefaultDeclaration(_)
            ) {
                return 1;
            }
            if let oxc_ast::ast::Statement::ExportAllDeclaration(export) = statement {
                return usize::from(
                    !export.export_kind.is_type()
                        && export
                            .exported
                            .as_ref()
                            .is_some_and(|name| name.name() == "default"),
                );
            }
            verter_parser::utils::oxc::script::export_parts::NamedExportParts::of_statement(
                statement,
            )
            .filter(|export| !export.export_kind.is_type())
            .map_or(0, |export| {
                export
                    .specifiers
                    .iter()
                    .filter(|specifier| {
                        !specifier.export_kind.is_type() && specifier.exported.name() == "default"
                    })
                    .count()
            })
        })
        .sum()
}

#[test]
fn carrier_shapes_keep_a_default_constructor_and_check_their_templates() {
    for source in [
        "",
        "   <!-- no blocks -->\n",
        "<style>.card { color: red }</style>",
        "<template></template>",
        "<template>{{ missing }}</template>",
        "<script lang=\"ts\">export const named = 1;</script><template>{{ named.toFixed() }}</template>",
        "<script lang=\"ts\">export default { data() { return { count: 1 }; }, methods: { increment() { this.count++; } } };</script><template>{{ count.toFixed() }}</template>",
        "<script lang=\"ts\">const options = { data() { return { count: 1 }; } }; export default options;</script><template>{{ count.toFixed() }}</template>",
        "<script lang=\"ts\">import { defineComponent as dc } from 'vue'; export default dc({ data() { return { count: 1 }; } });</script><template>{{ count.toFixed() }}</template>",
        "<script lang=\"ts\">export default { data() { return { count: 1 }; } };</script><script setup lang=\"ts\">const local = 'setup';</script><template>{{ count.toFixed() }}{{ local.toUpperCase() }}</template>",
        "<script setup lang=\"ts\">const local = 1;</script><template>{{ local.toFixed() }}</template>",
    ] {
        let module = composed(source);
        let text = module.checking_text();
        assert_eq!(checking_default_count(&module), 1, "{source}\n{text}");
        if source.contains("const options = ") {
            assert!(text.contains("import(\"vue\").ComponentInstance<"), "{source}\n{text}");
            assert!(text.contains("const __VerterAuthoredDefault = options;"), "{source}\n{text}");
            assert!(text.contains("const __VerterOptionsComponent = __VerterPublicDefault(__VerterAuthoredDefault);"), "{source}\n{text}");
        }
        assert!(text.contains("__VerterPublicInstance"), "{source}\n{text}");
        assert!(!module.withheld.iter().any(|p| p.family == ObligationFamily::Scaffold), "{source}\n{:?}", module.withheld);
        for expression in ["missing", "named.toFixed()", "count.toFixed()", "local.toUpperCase()", "local.toFixed()"] {
            if source.contains(&format!("{{{{ {expression} }}}}")) {
                assert_maps_expression(&module, source, expression);
                assert!(text.contains(&format!("__VerterCtx.{expression}")), "{source}\n{text}");
            }
        }
        if source.contains("data()") {
            assert_maps_verbatim(&module, source, "data() { return { count: 1 }; }");
            assert!(text.contains("InstanceType<typeof __VerterOptionsConstructor>"), "{text}");
        }
    }
}

#[test]
fn normal_default_specifiers_keep_one_public_default_and_their_sibling_exports() {
    for source in [
        "<script lang=\"ts\">import { defineComponent } from 'vue'; export { C as default }; const C = defineComponent({});</script>",
        "<script lang=\"ts\">import { defineComponent } from 'vue'; const C = defineComponent({}); const value = 1; type Item = number; export { type Item, C as default, value as named };</script>",
        "<script lang=\"ts\">import { defineComponent } from 'vue'; const C = defineComponent({}); const value = 1; type Item = number; export { C as default, type Item, value as named };</script>",
        "<script lang=\"ts\">import { defineComponent } from 'vue'; const C = defineComponent({}); const value = 1; type Item = number; export { type Item, value as named, C as default };</script>",
        "<script lang=\"ts\">export { default } from './Base.vue';</script>",
        "<script lang=\"ts\">export { type Item, default as default, value as named } from './Base.vue';</script>",
    ] {
        let module = composed(source);
        let text = module.checking_text();
        assert_eq!(checking_default_count(&module), 1, "{source}\n{text}");
        if source.contains("type Item,") {
            assert_maps_verbatim(&module, source, "type Item,");
            assert_maps_verbatim(&module, source, "value as named");
        }
        if source.contains("C as default") {
            assert!(offset_of(text, "const C = defineComponent({});") < offset_of(text, "const __VerterAuthoredDefault = C;"), "forward declaration is initialized before capture: {text}");
            assert_eq!(mapped_point_after(&module, "const __VerterAuthoredDefault = ", "C"), offset_of(source, "C as default") as u32);
        } else if !source.contains("type Item,") {
            assert_maps_expression(&module, source, "./Base.vue");
        }
    }
}

#[test]
fn default_function_overloads_stay_together_before_the_public_capture() {
    let source = concat!(
        "<script lang=\"ts\">\n",
        "export default function View(props: { value: number }): null;\n",
        "export default function View(props: { value: string }): null;\n",
        "export default function View(props: { value: number | string }) { return null; }\n",
        "export const same = View;\n",
        "</script>",
    );
    let module = composed(source);
    let text = module.checking_text();
    assert_eq!(checking_default_count(&module), 1, "{text}");
    let implementation = "function View(props: { value: number | string }) { return null; }";
    assert!(
        offset_of(text, "const __VerterAuthoredDefault = View;")
            >= offset_of(text, implementation) + implementation.len(),
        "the public capture follows the complete overload group: {text}",
    );
    for declaration in [
        "function View(props: { value: number }): null;",
        "function View(props: { value: string }): null;",
        implementation,
        "export const same = View;",
    ] {
        assert_maps_verbatim(&module, source, declaration);
    }
}

#[test]
fn anonymous_default_overloads_keep_one_binding_at_parameter_anchors() {
    for (name, implementation, asynchronous, generator, returns) in [
        ("", "function(props: { value: number | string }) { return null; }", false, false, "null"),
        ("", "async function /* implementation */ (props: { value: number | string }): Promise<null> { return null; }", true, false, "Promise<null>"),
        ("", "function* /* implementation */ (props: { value: number | string }): IterableIterator<number | string> { yield props.value; }", false, true, "IterableIterator<number | string>"),
        (" View", "function View(props: { value: number | string }) { return null; }", false, false, "null"),
    ] {
        let first = format!("export default function /* signature */ (props: {{ value: number }}): {returns};");
        let second = format!("export default function{name}(props: {{ value: string }}): {returns};");
        let implementation = format!("export default {implementation}");
        let first_suffix = format!("(props: {{ value: number }}): {returns};");
        let second_suffix = format!("(props: {{ value: string }}): {returns};");
        assert_anonymous_overload_group(
            &[&first, &second, &implementation],
            if name.is_empty() { "__VerterAuthoredDefault" } else { "View" },
            &[
                &first_suffix,
                &second_suffix,
                if generator { "(props: { value: number | string }): IterableIterator<number | string> { yield props.value; }" }
                    else if asynchronous { "(props: { value: number | string }): Promise<null> { return null; }" }
                    else { "(props: { value: number | string }) { return null; }" },
            ],
            asynchronous,
            generator,
        );
    }
    let single = "<script lang=\"ts\">export default function(props: { value: number }) { return null; }</script>";
    let module = composed(single);
    assert_eq!(checking_default_count(&module), 1);
    assert!(module
        .checking_text()
        .contains("const __VerterAuthoredDefault = function("));
    assert_maps_verbatim(
        &module,
        single,
        "function(props: { value: number }) { return null; }",
    );
}

#[test]
fn anonymous_default_overloads_keep_generic_binders_at_type_parameter_anchors() {
    for name in ["", " View"] {
        let second =
            format!("export default function{name}<T extends string>(props: {{ value: T }}): T;");
        let implementation = format!("export default function{name}<T extends number | string>(props: {{ value: T }}) {{ return props.value; }}");
        assert_anonymous_overload_group(
            &[
                "export default function /* signature */ <T extends number>(props: { value: T }): T;",
                &second,
                &implementation,
            ],
            if name.is_empty() { "__VerterAuthoredDefault" } else { "View" },
            &[
                "<T extends number>(props: { value: T }): T;",
                "<T extends string>(props: { value: T }): T;",
                "<T extends number | string>(props: { value: T }) { return props.value; }",
            ],
            false,
            false,
        );
    }
}

fn assert_anonymous_overload_group(
    declarations: &[&str],
    binding: &str,
    suffixes: &[&str],
    asynchronous: bool,
    generator: bool,
) {
    let source = format!(
        "<script lang=\"ts\">\n{}\n</script>",
        declarations.join("\n")
    );
    let module = composed(&source);
    let text = module.checking_text();
    assert_eq!(checking_default_count(&module), 1, "{text}");
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, text, oxc_span::SourceType::tsx())
            .parse();
    let functions: Vec<_> = parsed
        .program
        .body
        .iter()
        .filter_map(|statement| {
            let oxc_ast::ast::Statement::FunctionDeclaration(function) = statement else {
                return None;
            };
            function
                .id
                .as_ref()
                .is_some_and(|id| id.name == binding)
                .then_some(&**function)
        })
        .collect();
    assert_eq!(
        functions.len(),
        declarations.len(),
        "one binding owns every overload: {text}"
    );
    assert!(
        functions[..functions.len() - 1]
            .iter()
            .all(|function| function.body.is_none()),
        "{text}"
    );
    let implementation = functions.last().expect("implementation");
    assert!(implementation.body.is_some(), "{text}");
    assert_eq!(implementation.r#async, asynchronous, "{text}");
    assert_eq!(implementation.generator, generator, "{text}");
    assert!(
        offset_of(
            text,
            "const __VerterOptionsComponent = __VerterPublicDefault(__VerterAuthoredDefault);"
        ) >= implementation.span.end as usize,
        "adaptation follows the implementation: {text}"
    );
    if binding == "__VerterAuthoredDefault" {
        assert!(
            !text.contains("const __VerterAuthoredDefault ="),
            "the shared function binding needs no capture alias: {text}"
        );
    } else {
        assert!(
            offset_of(text, &format!("const __VerterAuthoredDefault = {binding};"))
                >= implementation.span.end as usize,
            "{text}"
        );
    }
    for suffix in suffixes {
        assert_maps_verbatim(&module, &source, suffix);
    }
}

#[test]
fn named_default_reexports_retain_import_attributes_and_sibling_mappings() {
    for keyword in ["with", "assert"] {
        for exported in ["default", "\"default\""] {
            for siblings in [false, true] {
                assert_reexport_attributes(false, siblings, keyword, exported);
            }
        }
    }
}

#[test]
fn namespace_default_reexports_retain_import_attributes() {
    for keyword in ["with", "assert"] {
        for exported in ["default", "\"default\""] {
            assert_reexport_attributes(true, false, keyword, exported);
        }
    }
}

fn assert_reexport_attributes(namespace: bool, siblings: bool, keyword: &str, exported: &str) {
    let clause = format!(" /* module */ {keyword} /* attributes */ {{ type: 'json' }}");
    let specifier = if siblings {
        "./options"
    } else {
        "./options.json"
    };
    let export = if namespace {
        format!("export * as {exported} from '{specifier}'{clause};")
    } else if siblings {
        format!("export {{ type Item, default as {exported}, value as named }} from '{specifier}'{clause};")
    } else {
        format!("export {{ default as {exported} }} from '{specifier}'{clause};")
    };
    let source = format!("<script lang=\"ts\">{export}</script>");
    let module = composed(&source);
    let text = module.checking_text();
    assert_eq!(checking_default_count(&module), 1, "{text}");
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, text, oxc_span::SourceType::tsx())
            .parse();
    let imports: Vec<_> = parsed
        .program
        .body
        .iter()
        .filter_map(|statement| {
            let oxc_ast::ast::Statement::ImportDeclaration(import) = statement else {
                return None;
            };
            (import.source.value == specifier).then_some(&**import)
        })
        .collect();
    assert_eq!(imports.len(), 1, "{text}");
    let import = imports[0];
    let attributes = import
        .with_clause
        .as_ref()
        .expect("the rewritten import retains its attributes");
    let start = import.source.span.end;
    let end = attributes.span.end;
    assert_eq!(
        &text[start as usize..end as usize],
        clause,
        "keyword and trivia remain authored"
    );
    // A surviving sibling export intentionally repeats this text. Inspect the
    // import's parsed range rather than asserting global substring uniqueness.
    let authored = offset_of(&source, &clause) as u32;
    let mut at = start;
    while at < end {
        let region = module
            .emission
            .mapping()
            .projected_at(at)
            .expect("attribute bytes are projected");
        assert_eq!(region.class, ProjectedClass::Rewritten);
        let carrier = region.carrier.expect("mapped attribute origin");
        assert_eq!(
            carrier.start + at - region.generated.start,
            authored + at - start
        );
        at = region.generated.end.min(end);
    }
    if siblings {
        assert_maps_verbatim(&module, &source, "type Item");
        assert_maps_verbatim(&module, &source, "value as named");
        let export = parsed
            .program
            .body
            .iter()
            .find_map(|statement| {
                if let oxc_ast::ast::Statement::ExportFromDeclaration(export) = statement {
                    Some(export)
                } else {
                    None
                }
            })
            .expect("sibling export remains");
        let attributes = export
            .with_clause
            .as_ref()
            .expect("sibling attributes remain");
        let generated = export.source.span.end;
        assert_eq!(
            &text[generated as usize..attributes.span.end as usize],
            clause
        );
        let region = module
            .emission
            .mapping()
            .projected_at(generated)
            .expect("sibling clause is projected");
        assert_eq!(region.class, ProjectedClass::Identity);
        assert_eq!(
            region.carrier.expect("sibling origin").start + generated - region.generated.start,
            authored
        );
    }
    module
        .emission
        .roundtrip_verbatim(&source)
        .expect("retained bytes roundtrip");
}

#[test]
fn namespace_defaults_use_the_namespace_value_and_preserve_other_exports() {
    let source = "<script lang=\"ts\">export * as default from './options';</script>";
    let module = composed(source);
    let text = module.checking_text();
    assert_eq!(checking_default_count(&module), 1, "{text}");
    assert!(
        text.contains("import * as __VerterAuthoredDefault from "),
        "{text}"
    );
    assert!(
        text.contains(
            "const __VerterOptionsComponent = __VerterPublicDefault(__VerterAuthoredDefault);"
        ),
        "{text}"
    );
    assert_maps_expression(&module, source, "./options");

    for statement in [
        "export * from './options';",
        "export * as named from './options';",
        "export type * from './options';",
        "export type * as named from './options';",
    ] {
        let source = format!("<script lang=\"ts\">{statement}</script>");
        let module = composed(&source);
        assert_eq!(
            checking_default_count(&module),
            1,
            "{}",
            module.checking_text()
        );
        assert_maps_verbatim(&module, &source, statement);
    }
}

#[test]
fn callable_defaults_keep_their_public_value_and_use_the_functional_instance_adapter() {
    let source = "<script lang=\"ts\">import { h } from 'vue'; export default (props: { label: string }) => h('div', props.label);</script>";
    let module = composed(source);
    let text = module.checking_text();
    assert!(
        !text.contains("InstanceType<typeof __VerterOptionsComponent>"),
        "{text}"
    );
    assert!(
        text.contains("__VerterUseConstructor(__VerterOptionsComponent)"),
        "{text}"
    );
    assert!(
        text.contains("const __VerterPublicComponent = __VerterOptionsComponent"),
        "{text}"
    );
    assert_maps_verbatim(
        &module,
        source,
        "(props: { label: string }) => h('div', props.label)",
    );
    assert_checking_syntax(&module);
    let named = "<script lang=\"ts\">import { h } from 'vue'; export default function View(props: { label: string }) { return h('div', props.label); } export const same = View;</script>";
    let module = composed(named);
    assert!(
        module
            .checking_text()
            .contains("const __VerterAuthoredDefault = View;"),
        "{}",
        module.checking_text()
    );
    assert_maps_verbatim(
        &module,
        named,
        "function View(props: { label: string }) { return h('div', props.label); }",
    );
    assert_maps_verbatim(&module, named, "export const same = View;");
    assert_checking_syntax(&module);
}

#[test]
fn options_type_aliases_do_not_shadow_instance_values_in_the_template_context() {
    let source = "<script lang=\"ts\">type count = number; export default { data() { return { count: 1 }; } };</script><template>{{ count.toFixed() }}</template>";
    let module = composed(source);
    let text = module.checking_text();
    assert!(text.contains("__VerterTemplateContext({},"), "{text}");
    assert_maps_expression(&module, source, "count.toFixed()");
    assert_checking_syntax(&module);
}

#[test]
fn template_feature_inventory_has_a_proof_for_every_family() {
    use ObligationFamily::*;
    // Function references keep the inventory tied to compiled behavioral proofs.
    let proofs: &[(ObligationFamily, fn())] = &[
        (
            Interpolation,
            an_interpolation_reads_its_setup_binding_through_the_template_context,
        ),
        (
            Branch,
            a_v_if_chain_is_real_branches_with_each_condition_once,
        ),
        (
            Iteration,
            template_feature_inventory_has_a_proof_for_every_family,
        ),
        (ComponentUse, a_component_use_is_constructed_in_its_block),
        (
            Listener,
            a_listener_object_is_checked_against_the_use_after_its_construction,
        ),
        (
            Model,
            template_feature_inventory_has_a_proof_for_every_family,
        ),
        (
            ProvidedSlot,
            builtins_use_vue_contracts_and_preserve_nested_slot_scope,
        ),
        (
            SlotOutlet,
            slot_outlets_payloads_are_checked_and_mapped_inside_their_branch,
        ),
        (
            NativeElement,
            native_elements_check_values_in_their_branch_and_keep_authored_mappings,
        ),
        (
            NativeModel,
            native_models_check_context_writes_with_source_anchors_and_guard_reentry,
        ),
        (
            Directive,
            directives_check_builtin_values_in_their_existing_scope,
        ),
        (
            Builtin,
            builtins_use_vue_contracts_and_preserve_nested_slot_scope,
        ),
        (Fallthrough, fallthrough_declarations_follow_runtime_roots),
        (
            TemplateRef,
            template_ref_checks_follow_the_specialized_use_and_map_to_the_parser_key,
        ),
        (
            Scaffold,
            carrier_shapes_keep_a_default_constructor_and_check_their_templates,
        ),
    ];
    let source = concat!(
        "<script setup lang=\"ts\">import Child from './Child.vue'; const value = 1; const rows = [1];</script>",
        "<template>{{ value }}<Teleport v-if=\"value\" to=\"#app\"><Child v-for=\"row in rows\" v-model=\"value\"><template #default>{{ row }}</template></Child></Teleport></template>",
    );
    let module = composed(source);
    for family in [Iteration, Model, ProvidedSlot] {
        assert!(
            module.obligations.iter().any(|o| o.family == family),
            "{family:?} is checked by this fixture: {}",
            module.checking_text()
        );
    }
    let mut families = std::collections::HashSet::new();
    families.extend(module.obligations.iter().map(|o| o.family));
    families.extend(module.withheld.iter().map(|p| p.family));
    for family in families {
        // An added family requires an explicit proof row even if its product
        // has not yet emitted a statement in this fixture.
        match family {
            Interpolation | Branch | Iteration | ComponentUse | Listener | Model | ProvidedSlot
            | SlotOutlet | NativeElement | NativeModel | Directive | Builtin | Fallthrough
            | TemplateRef | Scaffold => {}
        }
        assert_eq!(
            proofs
                .iter()
                .filter(|(covered, _)| *covered == family)
                .count(),
            1,
            "missing or duplicated proof for {family:?}"
        );
    }
}

fn offset_of(haystack: &str, needle: &str) -> usize {
    assert_eq!(
        haystack.matches(needle).count(),
        1,
        "`{needle}` appears exactly once in:\n{haystack}"
    );
    haystack.find(needle).expect("present")
}

/// Every byte of `needle` in the checking text maps to the same bytes of the
/// authored carrier, verbatim.
fn assert_maps_verbatim(module: &CheckingModule, source: &str, needle: &str) {
    assert_maps_as(
        module,
        source,
        needle,
        &[ProjectedClass::Identity, ProjectedClass::Relocated],
    );
}

/// The existing expression emitter records inserted authored pieces as rewritten
/// correspondence. Verify their bytes and exact authored offsets independently.
fn assert_maps_expression(module: &CheckingModule, source: &str, needle: &str) {
    assert_maps_as(
        module,
        source,
        needle,
        &[
            ProjectedClass::Identity,
            ProjectedClass::Relocated,
            ProjectedClass::Rewritten,
        ],
    );
}

fn assert_maps_as(module: &CheckingModule, source: &str, needle: &str, classes: &[ProjectedClass]) {
    let generated = offset_of(module.checking_text(), needle) as u32;
    let authored = offset_of(source, needle) as u32;
    let mapping = module.emission.mapping();
    let mut at = generated;
    while at < generated + needle.len() as u32 {
        let region = mapping
            .projected_at(at)
            .unwrap_or_else(|| panic!("generated byte {at} of `{needle}` is projected"));
        assert!(
            classes.contains(&region.class),
            "`{needle}` is verbatim, not {:?}",
            region.class
        );
        let carrier = region
            .carrier
            .expect("a verbatim region has a carrier span");
        assert_eq!(
            carrier.start + (at - region.generated.start),
            authored + (at - generated),
            "generated byte {at} of `{needle}` maps to its authored byte"
        );
        at = region.generated.end;
    }
    module
        .emission
        .roundtrip_verbatim(source)
        .expect("verbatim regions equal the carrier");
}

const GENERIC_SETUP: &str = concat!(
    "<template>\n  <div>{{ label }}</div>\n</template>\n",
    "<script setup lang=\"ts\" generic=\"T extends string\">\n",
    "import { ref } from 'vue'\n",
    "import { computed } from 'vue'\n",
    "interface Row { id: number }\n",
    "const items = ref<T[]>([]);\n",
    "type Rows = Row[];\n",
    "type Pair = [T, T];\n",
    "type Local = typeof items;\n",
    "defineProps<{ rows: Rows; pair?: Pair }>();\n",
    "const label = computed(() => items.value.join(', '));\n",
    "function first(): T | undefined { return items.value[0]; }\n",
    "</script>\n",
);

#[test]
fn setup_statements_appear_once_under_the_authored_binder_and_map_verbatim() {
    let module = composed(GENERIC_SETUP);
    let text = module.checking_text();
    let header = offset_of(text, "function __VerterSetupScope<T extends string>() {");
    for statement in [
        "const items = ref<T[]>([]);",
        "type Pair = [T, T];",
        "type Local = typeof items;",
        "const label = computed(() => items.value.join(', '));",
        "function first(): T | undefined { return items.value[0]; }",
    ] {
        assert!(
            offset_of(text, statement) > header,
            "`{statement}` sits inside the setup scope:\n{text}"
        );
        assert_maps_verbatim(&module, GENERIC_SETUP, statement);
    }
    assert!(
        !text.contains("function __VerterExpose"),
        "no second copy of the setup body:\n{text}"
    );
    assert!(
        !text.contains("<template>") && !text.contains("<script") && !text.contains("</script>"),
        "carrier markup is not checking text:\n{text}"
    );
}

#[test]
fn hoisted_imports_and_public_types_keep_source_order_and_line_ends() {
    let module = composed(GENERIC_SETUP);
    let text = module.checking_text();
    let hoisted = offset_of(
        text,
        concat!(
            "import { ref } from 'vue'\n",
            "import { computed } from 'vue'\n",
            "interface Row { id: number }\n",
            "type Rows = Row[];\n",
        ),
    );
    assert!(
        hoisted < offset_of(text, "declare const __VerterPublicComponent"),
        "hoisted statements precede the public declaration:\n{text}"
    );
    for statement in [
        "import { ref } from 'vue'",
        "import { computed } from 'vue'",
        "interface Row { id: number }",
        "type Rows = Row[];",
    ] {
        assert_maps_verbatim(&module, GENERIC_SETUP, statement);
    }
}

#[test]
fn an_await_in_class_heritage_makes_the_scope_async() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "class Base {}\n",
        "class Child extends (await Promise.resolve(Base)) {}\n",
        "defineExpose({ Child });\n",
        "</script>\n",
    );
    let text = composed(source).checking_text().to_string();
    offset_of(&text, "async function __VerterSetupScope() {");
    offset_of(
        &text,
        "ShallowUnwrapRef<Awaited<ReturnType<typeof __VerterSetupScope>>>",
    );
}

#[test]
fn a_wrapped_or_declared_expose_call_stays_valid() {
    let wrapped = concat!(
        "<script setup lang=\"ts\">\n",
        "const shown = 1;\n",
        "(defineExpose({ shown }));\n",
        "</script>\n",
    );
    let text = composed(wrapped).checking_text().to_string();
    offset_of(
        &text,
        "const __VerterExposedValue = (__VerterExposed({ shown }));",
    );
    offset_of(&text, "return __VerterExposedValue;");
    let declared = concat!(
        "<script setup lang=\"ts\">\n",
        "const shown = 1;\n",
        "const handle = defineExpose({ shown });\n",
        "</script>\n",
    );
    let text = composed(declared).checking_text().to_string();
    offset_of(&text, "const handle = __VerterExposed({ shown });");
    offset_of(&text, "return handle;");
}

#[test]
fn top_level_await_makes_the_scope_async_and_the_exposed_type_awaited() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const data = await Promise.resolve(1);\n",
        "defineExpose({ data });\n",
        "</script>\n",
    );
    let text = composed(source).checking_text().to_string();
    offset_of(&text, "async function __VerterSetupScope() {");
    offset_of(
        &text,
        "ShallowUnwrapRef<Awaited<ReturnType<typeof __VerterSetupScope>>>",
    );
}

#[test]
fn a_type_assertion_in_typescript_setup_survives() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "declare const value: unknown;\n",
        "const n = <number>value;\n",
        "</script>\n",
    );
    let module = composed(source);
    assert_eq!(module.dialect, CheckingDialect::Ts);
    assert_maps_verbatim(&module, source, "const n = <number>value;");
}

#[test]
fn expose_binds_the_value_the_setup_scope_returns() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const shown = 1;\n",
        "const hidden = 2;\n",
        "defineExpose({ shown });\n",
        "</script>\n",
    );
    let text = composed(source).checking_text().to_string();
    let bound = offset_of(
        &text,
        "const __VerterExposedValue = __VerterExposed({ shown });",
    );
    let returned = offset_of(&text, "return __VerterExposedValue;");
    assert!(
        bound < returned,
        "the scope returns the bound value:\n{text}"
    );
    offset_of(&text, "declare function __VerterExposed<T>(exposed: T): T;");
    offset_of(
        &text,
        "ShallowUnwrapRef<ReturnType<typeof __VerterSetupScope>>",
    );
    assert!(
        !text.contains("defineExpose("),
        "the callee is bound:\n{text}"
    );
}

#[test]
fn a_local_function_named_like_a_macro_is_an_ordinary_call() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "function defineExpose(value: unknown) { return value; }\n",
        "defineExpose({ local: 1 });\n",
        "</script>\n",
    );
    let module = composed(source);
    let text = module.checking_text();
    assert!(
        !text.contains("__VerterExposed"),
        "a shadowed name is not the macro:\n{text}"
    );
    assert_maps_verbatim(&module, source, "defineExpose({ local: 1 });");
}

#[test]
fn the_normal_script_stays_at_module_scope_verbatim() {
    let source = concat!(
        "<script lang=\"ts\">\n",
        "export const shared = 1;\n",
        "</script>\n",
        "<script setup lang=\"ts\">\n",
        "const local = shared + 1;\n",
        "</script>\n",
    );
    let module = composed(source);
    let text = module.checking_text();
    assert!(
        offset_of(text, "export const shared = 1;")
            < offset_of(text, "function __VerterSetupScope() {"),
        "the normal script precedes the setup scope:\n{text}"
    );
    assert_maps_verbatim(&module, source, "export const shared = 1;");
    assert_maps_verbatim(&module, source, "const local = shared + 1;");
}

#[test]
fn a_tsx_setup_selects_the_tsx_dialect() {
    let module = composed("<script setup lang=\"tsx\">\nconst a = <div />;\n</script>\n");
    assert_eq!(module.dialect, CheckingDialect::Tsx);
    assert_eq!(module.dialect.extension(), "tsx");
}

/// A JavaScript module carries no TypeScript syntax: it parses as the
/// JavaScript (with JSX) TypeScript reads in a `.js` file.
fn assert_javascript_syntax(module: &CheckingModule) {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = verter_parser::oxc_parse::Parser::new(
        &allocator,
        module.checking_text(),
        oxc_span::SourceType::jsx().with_module(true),
    )
    .parse();
    assert!(
        !parsed.fatal_error && parsed.diagnostics.is_empty(),
        "{:?}\n{}",
        parsed.diagnostics,
        module.checking_text()
    );
}

#[test]
fn a_javascript_carrier_is_checked_as_javascript() {
    let setup = concat!(
        "<script setup>\n",
        "import { ref, computed } from 'vue'\n",
        "const props = defineProps({ label: { type: String, required: true } })\n",
        "const emit = defineEmits(['change'])\n",
        "const count = ref(0)\n",
        "const doubled = computed(() => count.value * 2)\n",
        "function bump(step) {\n",
        "  count.value += step\n",
        "  emit('change', count.value)\n",
        "}\n",
        "function describe(item) {\n",
        "  return `${props.label}: ${item.name}`\n",
        "}\n",
        "</script>\n",
        "<template>\n",
        "  <button @click=\"bump(1)\">{{ label }} {{ doubled }}</button>\n",
        "  <span>{{ describe({ name: 'x' }) }}</span>\n",
        "</template>\n",
    );
    let options = concat!(
        "<script>\n",
        "export default {\n",
        "  props: { label: { type: String, required: true } },\n",
        "  data() {\n",
        "    return { count: 0 };\n",
        "  },\n",
        "  methods: {\n",
        "    bump(step) {\n",
        "      this.count += step;\n",
        "    },\n",
        "  },\n",
        "};\n",
        "</script>\n",
        "<template>\n",
        "  <button @click=\"bump(1)\">{{ label }} {{ count }}</button>\n",
        "</template>\n",
    );
    let exposed = concat!(
        "<script setup>\n",
        "import { ref } from 'vue'\n",
        "const count = ref(0)\n",
        "function bump(step) {\n",
        "  count.value += step\n",
        "}\n",
        "defineExpose({\n",
        "  bump,\n",
        "  focus(target) {\n",
        "    return target\n",
        "  },\n",
        "})\n",
        "</script>\n",
        "<template>\n",
        "  <button @click=\"bump(1)\">{{ count }}</button>\n",
        "</template>\n",
    );
    let jsdoc = concat!(
        "<script setup lang=\"js\">\n",
        "/** @param {number} n */\n",
        "function timesTwo(n) {\n",
        "  return n * 2\n",
        "}\n",
        "const bad = timesTwo('nope')\n",
        "</script>\n",
        "<template><div>{{ bad }}</div></template>\n",
    );
    let jsx = concat!(
        "<script lang=\"jsx\">\n",
        "export default {\n",
        "  props: { label: { type: String, required: true } },\n",
        "  methods: {\n",
        "    renderBadge() {\n",
        "      return <span class=\"badge\">{this.label}</span>;\n",
        "    },\n",
        "  },\n",
        "};\n",
        "</script>\n",
        "<template>\n",
        "  <div>{{ label }}</div>\n",
        "</template>\n",
    );
    // The script's first byte is rewritten too: the helpers still come first.
    let inline = "<script>export default { props: { a: String } };</script>\n<template><p>{{ a }}</p></template>\n";
    for (source, dialect, authored, generated) in [
        (
            inline,
            CheckingDialect::Js,
            "props: { a: String }",
            "__VerterDefineComponent({",
        ),
        (
            setup,
            CheckingDialect::Js,
            "function bump(step) {",
            "/** @type {",
        ),
        (
            options,
            CheckingDialect::Js,
            "this.count += step;",
            "__VerterDefineComponent({",
        ),
        (
            exposed,
            CheckingDialect::Js,
            "focus(target) {",
            "__VerterExposed({",
        ),
        (
            jsdoc,
            CheckingDialect::Js,
            "/** @param {number} n */",
            "@typedef {",
        ),
        (
            jsx,
            CheckingDialect::Jsx,
            "return <span class=\"badge\">{this.label}</span>;",
            "__VerterDefineComponent({",
        ),
    ] {
        let module = composed(source);
        assert_eq!(module.dialect, dialect, "{source}");
        assert_javascript_syntax(&module);
        // The authored script is checked as it is, under JavaScript rules.
        assert_maps_verbatim(&module, source, authored);
        assert!(
            module.checking_text().contains(generated),
            "{generated}\n{}",
            module.checking_text()
        );
        // The template is checked too: its statements have a JavaScript form.
        assert!(
            !module
                .withheld
                .iter()
                .any(|product| product.family == ObligationFamily::Scaffold),
            "{:?}\n{}",
            module.withheld,
            module.checking_text()
        );
    }
}

#[test]
fn a_javascript_carriers_file_pragma_leads_the_module() {
    // TypeScript reads `// @ts-check` / `// @ts-nocheck` only from a file's
    // leading comments, the last one winning, and vue-tsc honors them in
    // either script block. The module opens with the one that applies, closed
    // by an empty statement so no authored comment after it leads the module.
    // Authored comments stay where they are, so JSDoc stays on its
    // declaration.
    for (source, pragma, kept) in [
        (
            "<script setup>\n// @ts-check\nimport { ref } from 'vue'\nconst n = ref(0)\n</script>\n<template><p>{{ n }}</p></template>\n",
            "// @ts-check\n;\n",
            "\nimport { ref } from 'vue'\n",
        ),
        (
            "<script>\n// @ts-nocheck\nexport default { methods: { f(x) { return x } } }\n</script>\n<template><p /></template>\n",
            "// @ts-nocheck\n;\n",
            // The authored comment stays before the rewritten default export.
            "// @ts-nocheck\nimport {",
        ),
        (
            // The setup block comes later, so its pragma wins.
            "<script>\n// @ts-nocheck\nexport const shared = 1;\n</script>\n<script setup>\n// @ts-check\nshared.toUpperCase();\n</script>\n",
            "// @ts-check\n;\n",
            "// @ts-nocheck\nexport const shared = 1;",
        ),
        (
            // The pragma is the comment's first token, not a later mention.
            "<script setup>\n// @ts-check -- do not add @ts-nocheck\nconst a = 1;\n</script>\n",
            "// @ts-check\n;\n",
            "const a = 1;",
        ),
        (
            // TypeScript reads these pragmas from line comments only.
            "<script setup>\n/* @ts-nocheck */\nconst a = 1;\n</script>\n",
            "",
            "/* @ts-nocheck */\nconst a = 1;",
        ),
        (
            "<script setup>\n/** @param {number} n */\nfunction timesTwo(n) {\n  return n * 2\n}\n</script>\n<template><p /></template>\n",
            "",
            "/** @param {number} n */\nfunction timesTwo(n) {",
        ),
    ] {
        let module = composed(source);
        let text = module.checking_text();
        assert_javascript_syntax(&module);
        assert!(text.trim_start().starts_with(pragma), "{pragma}\n{text}");
        assert!(text.contains(kept), "{kept}\n{text}");
        if pragma.is_empty() {
            assert!(!text.trim_start().starts_with("// @ts-"), "{text}");
        }
    }
}

#[test]
fn a_javascript_pair_whose_lang_values_differ_is_refused() {
    // Vue compares the decoded `lang` values: `js` beside `javascript` is a
    // conflict although both name JavaScript.
    assert_eq!(
        compose(concat!(
            "<script lang=\"js\">\nexport const a = 1;\n</script>\n",
            "<script setup lang=\"javascript\">\nconst b = a;\n</script>\n",
        ))
        .err(),
        Some(CheckingModuleRefusal::Script(
            SetupProjectionRefusal::ScriptLangConflict
        ))
    );
}

#[test]
fn a_javascript_carrier_declares_no_generic_binder() {
    assert_eq!(
        compose("<script setup generic=\"T\">\nconst a = 1;\n</script>\n").err(),
        Some(CheckingModuleRefusal::Script(
            SetupProjectionRefusal::InvalidGeneric
        ))
    );
}

#[test]
fn a_setup_type_shadows_a_normal_script_type_of_the_same_name() {
    let source = concat!(
        "<script lang=\"ts\">\n",
        "type A = 1;\n",
        "</script>\n",
        "<script setup lang=\"ts\">\n",
        "type A = 2;\n",
        "defineProps<{ value: A }>();\n",
        "const a: A = 2;\n",
        "</script>\n",
    );
    let module = composed(source);
    let text = module.checking_text();
    let header = offset_of(text, "function __VerterSetupScope() {");
    assert!(
        offset_of(text, "type A = 1;") < header,
        "the normal-script type stays at module scope:\n{text}"
    );
    assert!(
        offset_of(text, "type A = 2;") > header,
        "the setup type is declared in the setup scope:\n{text}"
    );
    assert_maps_verbatim(&module, source, "type A = 2;");
}

#[test]
fn exported_setup_types_are_module_exports_with_the_local_types_they_name() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "interface Cell { text: string }\n",
        "export interface Row { id: string; cell: Cell }\n",
        "export type Rows = Row[];\n",
        "export const limit = 1;\n",
        "defineProps<{ row: Row }>();\n",
        "</script>\n",
    );
    let module = composed_declaring(source);
    let text = module.checking_text();
    let header = offset_of(text, "function __VerterSetupScope() {");
    // `Rows` is named by no macro, and `Cell` only through the export.
    for statement in [
        "interface Cell { text: string }",
        "export interface Row { id: string; cell: Cell }",
        "export type Rows = Row[];",
    ] {
        assert!(
            offset_of(text, statement) < header,
            "`{statement}` stands at module scope:\n{text}"
        );
        assert_maps_verbatim(&module, source, statement);
    }
    assert!(
        offset_of(text, "export const limit = 1;") > header,
        "a value export stays in the setup scope:\n{text}"
    );

    let product = module
        .declarations
        .as_ref()
        .expect("a clean carrier carries its declaration product");
    assert!(product.is_publishable(), "{:?}", product.diagnostics);
    let declaration = &product.declaration_source;
    for exported in [
        "export interface Row { id: string; cell: Cell }",
        "export type Rows = Row[];",
    ] {
        assert!(declaration.contains(exported), "{exported}:\n{declaration}");
    }
    assert!(
        declaration.contains("interface Cell { text: string }")
            && !declaration.contains("export interface Cell"),
        "{declaration}"
    );
    assert!(!declaration.contains("limit = 1"), "{declaration}");
}

#[test]
fn a_public_setup_type_that_reads_a_setup_value_stays_in_the_setup_scope() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const count = 1;\n",
        "type Counted = { count: typeof count };\n",
        "defineProps<{ counted: Counted }>();\n",
        "</script>\n",
    );
    let module = composed(source);
    let text = module.checking_text();
    let header = offset_of(text, "function __VerterSetupScope() {");
    assert!(
        offset_of(text, "type Counted = { count: typeof count };") > header,
        "the type reads `count`, which only the setup scope binds:\n{text}"
    );
}

/// The carrier offset the generated `needle` right after `after` stands for.
/// An authored copy maps byte for byte, so it is the offset of the needle's
/// first byte; a generated spelling is one insertion at a carrier point.
fn mapped_point_after(module: &CheckingModule, after: &str, needle: &str) -> u32 {
    let text = module.checking_text();
    let generated = (offset_of(text, after) + after.len()) as u32;
    let region = module
        .emission
        .mapping()
        .projected_at(generated)
        .unwrap_or_else(|| panic!("generated byte {generated} is projected"));
    let carrier = region
        .carrier
        .unwrap_or_else(|| panic!("`{needle}` is mapped, not {:?}", region.class));
    match region.class {
        ProjectedClass::Identity | ProjectedClass::Relocated => {
            assert!(
                generated + needle.len() as u32 <= region.generated.end,
                "`{needle}` is one authored copy"
            );
            carrier.start + (generated - region.generated.start)
        }
        _ => {
            assert_eq!(
                &text[region.generated.start as usize..region.generated.end as usize],
                needle,
                "`{needle}` is one mapped insertion"
            );
            assert_eq!(
                carrier.start, carrier.end,
                "a mapped insertion stands at a point"
            );
            carrier.start
        }
    }
}

const COUNTER: &str = concat!(
    "<template>\n  <p>{{ count }}</p>\n</template>\n",
    "<script setup lang=\"ts\">\n",
    "import { ref } from 'vue'\n",
    "const count = ref(1);\n",
    "const unused = 2;\n",
    "</script>\n",
);

const CONTEXT_INSTANCE: &str = "null! as __VerterPublicInstance & __VerterPublicProps";

#[test]
fn an_interpolation_reads_its_setup_binding_through_the_template_context() {
    let module = composed(COUNTER);
    let text = module.checking_text();
    let header = offset_of(text, "function __VerterSetupScope() {");
    let context = offset_of(
        text,
        &format!("const __VerterCtx = __VerterTemplateContext({{ count }}, {CONTEXT_INSTANCE});\n"),
    );
    let read = offset_of(text, "(__VerterCtx.count);\n");
    assert!(
        header < offset_of(text, "const count = ref(1);") && context > header && read > context,
        "the context and the template read follow the setup statements:\n{text}"
    );
    offset_of(text, "declare function __VerterTemplateContext<B, I>(");
    let authored = offset_of(COUNTER, "{{ count }}") as u32 + 3;
    assert_eq!(
        mapped_point_after(&module, "(__VerterCtx.", "count"),
        authored,
        "the read maps to the authored identifier"
    );
}

#[test]
fn only_names_the_template_references_enter_the_context() {
    let text = composed(COUNTER).checking_text().to_string();
    assert!(
        !text.contains("unused }") && !text.contains("{ unused"),
        "an unreferenced setup binding stays out of the context:\n{text}"
    );
}

#[test]
fn a_let_binding_is_read_through_an_accessor_pair() {
    let source = concat!(
        "<template>\n  <p>{{ camera.zoom }}</p>\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "interface Camera { zoom: number }\n",
        "let camera: Camera;\n",
        "camera = { zoom: 1 };\n",
        "</script>\n",
    );
    let text = composed(source).checking_text().to_string();
    offset_of(
        &text,
        "{ get camera() { return camera; }, set camera(v) { camera = v; } }",
    );
    offset_of(&text, "(__VerterCtx.camera.zoom);");
}

#[test]
fn a_shorthand_property_keeps_its_key_and_reads_a_prop_from_the_instance() {
    let source = concat!(
        "<template>\n  <p>{{ t('k', { login }) }}</p>\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "defineProps<{ login: string }>();\n",
        "const t = (key: string, values: object) => key;\n",
        "</script>\n",
    );
    let module = composed(source);
    let text = module.checking_text();
    offset_of(text, "__VerterTemplateContext({ t }, ");
    offset_of(text, "(__VerterCtx.t('k', { login: __VerterCtx.login }));");
    let authored = offset_of(source, "login }") as u32;
    assert_eq!(
        mapped_point_after(&module, "{ login: __VerterCtx.", "login"),
        authored,
        "the expanded value maps to the authored shorthand"
    );
}

#[test]
fn supported_templates_are_complete_and_keep_checked_obligations() {
    let module = composed(GENERIC_SETUP);
    assert!(module.withheld.is_empty(), "{:?}", module.withheld);
    assert!(module.is_complete());
    offset_of(module.checking_text(), "(__VerterCtx.label);");
    let script_only = composed("<script setup lang=\"ts\">\nconst a = 1;\n</script>\n");
    assert!(script_only.withheld.is_empty());
    assert!(script_only.is_complete());
}

#[test]
fn statements_carry_their_obligation_family() {
    let module = composed(COUNTER);
    let text = module.checking_text();
    let read = offset_of(text, "(__VerterCtx.count);") as u32;
    let context = offset_of(text, "const __VerterCtx") as u32;
    let at = |offset: u32| module.obligation_at(offset).map(|span| span.family);
    assert_eq!(at(read + 2), Some(ObligationFamily::Interpolation));
    assert_eq!(at(context + 2), Some(ObligationFamily::Scaffold));
    assert_eq!(at(0), None, "the module scope holds no template obligation");
}

#[test]
fn checking_ranges_select_the_innermost_containing_obligation() {
    let source = "<script setup lang=\"ts\">import Child from './Child.vue';</script><template><Child /></template>";
    let mut module = composed(source);
    let construction = offset_of(
        module.checking_text(),
        "= new (__VerterUseComponent(Child, ",
    ) as u32;
    let outer = module.obligation_at(construction).unwrap().clone();
    let query = MappingSpan {
        start: construction,
        end: construction + 4,
    };
    let found = module.obligation_for_span(query).unwrap();
    assert_eq!(found.family, ObligationFamily::ComponentUse);
    assert_eq!(found.host, outer.host);
    let mut inner = outer.clone();
    inner.generated = SourceRange {
        start: construction,
        end: construction + 6,
    };
    inner.family = ObligationFamily::Listener;
    module.obligations.push(inner);
    module.obligations.sort_by_key(|span| span.generated.start);
    assert_eq!(
        module.obligation_for_span(query).unwrap().family,
        ObligationFamily::Listener
    );
    assert_eq!(
        module
            .obligation_for_span(MappingSpan {
                start: construction,
                end: construction + 7
            })
            .unwrap()
            .family,
        ObligationFamily::ComponentUse
    );
    assert!(module
        .obligation_for_span(MappingSpan {
            start: outer.generated.end - 1,
            end: outer.generated.end + 1
        })
        .is_none());
    assert!(module
        .obligation_for_span(MappingSpan { start: 1, end: 0 })
        .is_none());
}

#[test]
fn an_expression_emitted_twice_as_mapped_is_refused() {
    let artifact = artifact(COUNTER);
    let plan = VueProjectionBackend.projection_plan(COUNTER, &artifact, CANONICAL);
    let id = plan
        .expressions()
        .iter()
        .find(|occurrence| occurrence.kind == ExpressionKind::Interpolation)
        .expect("the interpolation is admitted")
        .id
        .clone();
    let statement = |replay: bool| {
        let mut fragment = CheckingFragment::synth("(");
        fragment.push_expression(id.clone()).push_synth(");\n(");
        if replay {
            fragment.push_replay(id.clone());
        } else {
            fragment.push_expression(id.clone());
        }
        fragment.push_synth(");\n");
        ScopeStatement {
            family: ObligationFamily::Interpolation,
            host: None,
            fragment,
        }
    };
    assert!(matches!(
        lower_scope(COUNTER, &plan, &[statement(false)]),
        Err(CheckingModuleRefusal::FragmentInvariant {
            family: ObligationFamily::Interpolation,
            ..
        })
    ));
    assert!(
        lower_scope(COUNTER, &plan, &[statement(true)]).is_ok(),
        "a replay is a further occurrence, not a second mapping"
    );
}

const DANGLING_MEMBER: &str = concat!(
    "<template>\n  <p>{{ count }}</p>\n</template>\n",
    "<script setup lang=\"ts\">\n",
    "const count = ref(1);\n",
    "const next = count.\n",
    "</script>\n",
);

const UNCLOSED_ELEMENT: &str = concat!(
    "<template>\n  <div>{{ count }}\n</template>\n",
    "<script setup lang=\"ts\">\n",
    "const count = 1;\n",
    "</script>\n",
);

#[test]
fn a_script_syntax_error_is_recovered_not_refused() {
    let module = composed(DANGLING_MEMBER);
    assert!(!module.is_complete());
    assert!(!module.recovery.syntax_diagnostics().is_empty());
    assert_eq!(
        module
            .recovery
            .region(ProjectionRegion::InstanceScope)
            .map(|region| region.state),
        Some(RegionState::Recovered)
    );
    assert!(
        module.checking_text().contains("count.valueOf"),
        "the dangling member keeps its receiver: {}",
        module.checking_text()
    );
    assert!(module
        .withheld
        .iter()
        .any(|product| product.family == ObligationFamily::Scaffold));
}

#[test]
fn a_recovered_composition_leaves_nothing_behind_for_the_next_one() {
    let valid = DANGLING_MEMBER.replace("count.\n", "count.value;\n");
    let _ = composed(DANGLING_MEMBER);
    let again = composed(&valid);
    let fresh = composed(&valid);
    assert_eq!(again.checking_text(), fresh.checking_text());
    assert!(again.recovery.is_complete());
}

#[test]
fn a_template_region_the_topology_does_not_trust_is_listed() {
    let module = composed(UNCLOSED_ELEMENT);
    let marked = module
        .recovery
        .region(ProjectionRegion::Markup)
        .expect("the unclosed element's region is listed");
    assert_ne!(marked.state, RegionState::Complete);
    assert!(!module.is_complete());
    if marked.state == RegionState::Omitted {
        assert!(!module.checking_text().contains("count);"));
    }
}

fn external_binding(
    source: &str,
    kind: ExternalBlockKind,
    specifier: &str,
    legality: ExternalLegality,
) -> ExternalBlockProjectionBinding {
    let start = offset_of(source, specifier) as u32;
    ExternalBlockProjectionBinding {
        blocks: vec![BoundExternalBlock {
            kind,
            specifier: specifier.to_string(),
            specifier_range: SourceRange {
                start,
                end: start + specifier.len() as u32,
            },
            source_identity: format!("/src/{}", specifier.trim_start_matches("./")),
            legality,
            chain: ProcessedSourceChain::default(),
        }],
        ..Default::default()
    }
}

fn compose_external(
    source: &str,
    external: &ExternalBlockProjectionBinding,
) -> Result<CheckingModule, CheckingModuleRefusal> {
    let artifact = artifact(source);
    compose_checking_module(&CheckingModuleInput {
        source,
        artifact: &artifact,
        canonical_id: CANONICAL,
        custom_elements: None,
        external: Some(external),
        // An external instance declares nothing even when asked.
        declarations: true,
    })
}

#[test]
fn external_script_is_re_exported_from_its_src_value() {
    let source = "<script lang=\"ts\" src=\"./logic.ts\"></script>\n";
    let binding = external_binding(
        source,
        ExternalBlockKind::Script,
        "./logic.ts",
        ExternalLegality::Admitted,
    );
    let module = compose_external(source, &binding).expect("an admitted external script composes");

    assert_eq!(
        module
            .checking_text()
            .matches("export * from \"./logic.ts\";")
            .count(),
        1,
        "the external file is named once, not inlined:\n{}",
        module.checking_text()
    );
    assert!(
        module
            .checking_text()
            .contains("export { default } from \"./logic.ts\";"),
        "a carrier without setup forwards the external default"
    );
    let generated = module.checking_text().find("./logic.ts").expect("present") as u32;
    let region = module
        .emission
        .mapping()
        .projected_at(generated)
        .expect("the specifier is projected");
    assert_eq!(region.class, ProjectedClass::Rewritten);
    assert_eq!(
        region.carrier.expect("mapped to the carrier").start,
        offset_of(source, "./logic.ts") as u32,
        "a failure to resolve the import lands on the src value"
    );
}

#[test]
fn external_script_specifier_that_cannot_stand_in_a_literal_is_escaped_and_unmapped() {
    let source = "<script lang=\"ts\" src='./a\"b.ts'></script>\n";
    let binding = external_binding(
        source,
        ExternalBlockKind::Script,
        "./a\"b.ts",
        ExternalLegality::Admitted,
    );
    let module = compose_external(source, &binding).expect("composes");

    assert!(
        module
            .checking_text()
            .contains("export * from \"./a\\\"b.ts\";"),
        "{}",
        module.checking_text()
    );
}

#[test]
fn a_carrier_without_an_external_binding_has_no_external_import() {
    let module = composed("<script lang=\"ts\">export const a = 1;</script>\n");
    assert!(!module.checking_text().contains("export * from"));
}

#[test]
fn external_legality_refusals_stop_the_composition() {
    for (source, legality) in [
        (
            "<script setup lang=\"ts\" src=\"./setup.ts\"></script>\n",
            ExternalLegality::ScriptSetupSrc,
        ),
        (
            "<script lang=\"ts\" src=\"./setup.ts\"></script>\n",
            ExternalLegality::DialectMismatch,
        ),
        (
            "<script lang=\"ts\" src=\"./setup.ts\"></script>\n",
            ExternalLegality::UnsupportedLanguage("coffee".to_string()),
        ),
        (
            "<script lang=\"ts\" src=\"./setup.ts\"></script>\n",
            ExternalLegality::Unreadable,
        ),
    ] {
        let binding = external_binding(
            source,
            ExternalBlockKind::Script,
            "./setup.ts",
            legality.clone(),
        );
        assert_eq!(
            compose_external(source, &binding).unwrap_err(),
            CheckingModuleRefusal::External(legality)
        );
    }
}

#[test]
fn external_template_is_withheld_because_its_text_is_not_in_the_carrier() {
    let source =
        "<script setup lang=\"ts\">let a = 1</script>\n<template src=\"./view.html\"></template>\n";
    let binding = external_binding(
        source,
        ExternalBlockKind::Template,
        "./view.html",
        ExternalLegality::Admitted,
    );
    let scaffold = |module: &CheckingModule| {
        module
            .withheld
            .iter()
            .any(|product| product.family == ObligationFamily::Scaffold)
    };
    assert!(!scaffold(&composed(source)));

    let module = compose_external(source, &binding).expect("composes");
    assert!(scaffold(&module), "{:?}", module.withheld);
    assert!(!module.is_complete());
}

#[test]
fn a_v_if_chain_is_real_branches_with_each_condition_once() {
    let source = concat!(
        "<template>\n",
        "  <div v-if=\"user\">\n",
        "    <span v-if=\"user.name\">{{ user.name.length }}</span>\n",
        "  </div>\n",
        "  <p v-else-if=\"kind === 'b'\">{{ kind }}</p>\n",
        "  <p v-else>{{ other }}</p>\n",
        "</template>\n",
        "<script setup lang=\"ts\">\n",
        "const user = null as { name: string | null } | null;\n",
        "const kind = 'a' as string;\n",
        "const other = 1;\n",
        "</script>\n",
    );
    let module = composed(source);
    let text = module.checking_text();
    let body = concat!(
        "{\n",
        "if (__VerterCtx.user) {\n",
        "if (__VerterCtx.user.name) {\n",
        "(__VerterCtx.user.name.length);\n",
        "}\n",
        "}\n",
        "else if (__VerterCtx.kind === 'b') {\n",
        "(__VerterCtx.kind);\n",
        "}\n",
        "else {\n",
        "(__VerterCtx.other);\n",
        "}\n",
        "}\n",
    );
    let at = offset_of(text, body);
    assert!(
        at > offset_of(text, "const __VerterCtx = "),
        "the blocks read the context:\n{text}"
    );
    assert!(!module
        .withheld
        .iter()
        .any(|product| product.family == ObligationFamily::Branch));
    let head = offset_of(text, "else if (__VerterCtx.kind === 'b') {") as u32;
    assert_eq!(
        module.obligation_at(head + 1).map(|span| span.family),
        Some(ObligationFamily::Branch)
    );
    let authored = offset_of(source, "kind === 'b'") as u32;
    assert_eq!(
        mapped_point_after(&module, "else if (__VerterCtx.", "kind"),
        authored,
        "the condition maps to the authored condition"
    );
}

#[test]
fn an_authored_type_predicate_guards_its_arm() {
    let source = concat!(
        "<template>\n  <p v-if=\"isText(value)\">{{ value.length }}</p>\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "const value = 1 as string | number;\n",
        "const isText = (v: unknown): v is string => typeof v === 'string';\n",
        "</script>\n",
    );
    let text = composed(source).checking_text().to_string();
    offset_of(
        &text,
        "if (__VerterCtx.isText(__VerterCtx.value)) {\n(__VerterCtx.value.length);\n}\n",
    );
}

#[test]
fn the_composer_takes_no_native_flow_input_and_is_deterministic() {
    let first = composed(GENERIC_SETUP);
    let second = composed(GENERIC_SETUP);
    assert_eq!(first.checking_text(), second.checking_text());
    assert_eq!(first.obligations, second.obligations);
}

/// Minimised carriers that once broke the template scope; each composes.
const BRANCH_REPROS: [(&str, &str); 6] = [
    (
        "comment between if and else-if",
        concat!(
            "<script setup lang=\"ts\">\nconst kind = 'a' as string;\n</script>\n",
            "<template>\n  <article>\n    <p v-if=\"kind === 'a'\">a</p>\n",
            "    <!-- A project its red main holds (x): why its starts wait. -->\n",
            "    <p v-else-if=\"kind === 'b'\">b</p>\n  </article>\n</template>\n",
        ),
    ),
    (
        "else-if with no if",
        concat!(
            "<script setup lang=\"ts\">\nconst item = { kind: 'a' };\n</script>\n",
            "<template>\n  <article>\n\n",
            "    <MainRedDetail v-else-if=\"item.kind === 'main-red'\" :item=\"item\" />\n",
            "  </article>\n</template>\n",
        ),
    ),
    (
        "comment between if and else",
        concat!(
            "<script setup lang=\"ts\">\nconst kind = 'a' as string;\n</script>\n",
            "<template>\n  <div>\n    <p v-if=\"kind === 'a'\">a</p>\n    <!-- note -->\n",
            "    <p v-else>b</p>\n  </div>\n</template>\n",
        ),
    ),
    (
        "same key in both arms",
        concat!(
            "<script setup lang=\"ts\">\nimport { ref } from 'vue';\n",
            "const kind = ref('a'), id = ref('1');\n</script>\n",
            "<template><div><p v-if=\"kind === 'a'\" :key=\"`k:${id}`\">a</p>",
            "<span v-else :key=\"`k:${id}`\">b</span></div></template>\n",
        ),
    ),
    (
        "same key in sibling ifs",
        concat!(
            "<script setup lang=\"ts\">\nimport { ref } from 'vue';\n",
            "const sel = ref<{ id: string } | null>({ id: 'a' });\n</script>\n",
            "<template><div><p v-if=\"sel\" :key=\"sel.id\">a</p>",
            "<span v-if=\"sel\" :key=\"sel.id\">b</span></div></template>\n",
        ),
    ),
    (
        "comment inside a component chain",
        concat!(
            "<script setup lang=\"ts\">\n",
            "import QuestionDetail from './QuestionDetail.vue';\n",
            "import MainRedDetail from './MainRedDetail.vue';\n",
            "const props = defineProps<{ tenant: string; item: { kind: string; id: string } }>();\n",
            "</script>\n",
            "<template>\n  <article   >\n",
            "    <QuestionDetail v-if=\"item.kind === 'question'\"  :key=\"`${tenant}:${item.id}`\" />\n",
            "    <!-- A project its red main holds (#618): why its starts wait. -->\n",
            "    <MainRedDetail v-else-if=\"item.kind === 'main-red'\" :key=\"`${tenant}:${item.id}`\" />\n",
            "  </article>\n</template>\n",
        ),
    ),
];

#[test]
fn minimised_branch_carriers_compose_without_refusal() {
    for (case, source) in BRANCH_REPROS {
        let module = compose(source).unwrap_or_else(|refusal| panic!("{case}: {refusal:?}"));
        let text = module.checking_text();
        assert!(
            text.contains("if (__VerterCtx."),
            "{case}: the chain is emitted:\n{text}"
        );
        // An else-if with no if is invalid Vue; its chain is still checked.
        if case != "else-if with no if" {
            assert!(
                !module
                    .withheld
                    .iter()
                    .any(|product| product.family == ObligationFamily::Branch),
                "{case}: no branch is withheld"
            );
        }
    }
}

const USES: &str = concat!(
    "<template>\n",
    "  <Child v-if=\"user\" :name=\"user.name\" />\n",
    "  <Child v-else name=\"anonymous\" />\n",
    "</template>\n",
    "<script setup lang=\"ts\">\n",
    "import Child from './Child.vue'\n",
    "const user = null as { name: string } | null;\n",
    "</script>\n",
);

#[test]
fn a_component_use_is_constructed_in_its_block() {
    let module = composed(USES);
    let text = module.checking_text();
    offset_of(text, "declare function __VerterUseDirect<");
    let arm = offset_of(text, "if (__VerterCtx.user) {\nconst __VerterUse_");
    let construction = offset_of(
        text,
        "= new (__VerterUseComponent(Child, __VerterUseConstructor(Child)))({ \"name\": (__VerterCtx.user.name) });\n",
    );
    assert!(arm < construction, "the use is built in its arm:\n{text}");
    let else_arm = offset_of(text, "else {\nconst __VerterUse_");
    assert!(
        offset_of(text, "({ \"name\": \"anonymous\" });\n") > else_arm,
        "the static attribute is a string member:\n{text}"
    );
    let direct = text
        .matches("__VerterUseDirect(Child, __VerterUse_")
        .count();
    assert_eq!(direct, 2, "one direct-key check per use:\n{text}");
    assert!(!module
        .withheld
        .iter()
        .any(|product| product.family == ObligationFamily::ComponentUse));
    let span = module
        .obligation_at(construction as u32 + 4)
        .expect("the construction has an obligation");
    assert_eq!(span.family, ObligationFamily::ComponentUse);
    assert!(matches!(span.host, Some(UseHost::Component(_))));
    let authored = offset_of(USES, "user.name\"") as u32;
    assert_eq!(
        mapped_point_after(&module, "{ \"name\": (__VerterCtx.", "user"),
        authored,
        "the prop value maps to the authored attribute value"
    );
}

#[test]
fn a_dynamic_component_reads_its_value_in_the_template_context() {
    let source = concat!(
        "<template>\n  <component :is=\"current\" :title=\"title\" />\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "import Child from './Child.vue'\n",
        "const current = Child;\n",
        "const title = 'x';\n",
        "</script>\n",
    );
    let text = composed(source).checking_text().to_string();
    offset_of(
        &text,
        "new (__VerterUseComponent((__VerterCtx.current), __VerterUseConstructor((__VerterCtx.current))))({ \"title\": (__VerterCtx.title) });\n",
    );
}

#[test]
fn the_template_scope_starts_a_new_statement_after_the_last_setup_token() {
    let source = concat!(
        "<script setup lang=\"ts\">const count = 1 // last</script>",
        "<template>{{ count }}</template>\n",
    );
    let text = composed(source).checking_text().to_string();
    offset_of(&text, "const count = 1 // last\n");
    offset_of(&text, "(__VerterCtx.count);");
}

#[test]
fn a_value_less_kebab_binding_reads_its_camelized_name() {
    let source = concat!(
        "<template>\n  <Child :foo-bar />\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "import Child from './Child.vue'\n",
        "const fooBar = 'ok';\n",
        "</script>\n",
    );
    let module = composed(source);
    let text = module.checking_text();
    offset_of(text, "__VerterTemplateContext({ fooBar }, ");
    offset_of(text, "(__VerterCtx.fooBar)");
    assert!(!text.contains("foo-bar)"), "no kebab read:\n{text}");
    let authored = offset_of(source, "foo-bar") as u32;
    assert_eq!(
        mapped_point_after(&module, "(__VerterCtx.", "fooBar"),
        authored
    );
}

#[test]
fn a_chain_with_an_unadmitted_condition_contributes_nothing() {
    let source = concat!(
        "<template>\n  <p v-if=\"a +\" />\n  <p v-else>{{ b }}</p>\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "const a = 1;\n",
        "const b = 2;\n",
        "</script>\n",
    );
    let module = composed(source);
    let text = module.checking_text();
    assert!(
        !text.contains("else") && !text.contains("__VerterCtx.b"),
        "no arm of the chain is emitted:\n{text}"
    );
    assert!(module.withheld.contains(&WithheldProduct {
        family: ObligationFamily::Branch,
        reason: WithheldReason::Incomplete,
    }));
}

#[test]
fn a_handler_ending_in_a_semicolon_keeps_its_returned_value() {
    let source = concat!(
        "<template>\n  <Child @validate=\"check();\" />\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "import Child from './Child.vue'\n",
        "const check = () => true;\n",
        "</script>\n",
    );
    let text = composed(source).checking_text().to_string();
    offset_of(&text, "($event) => (void $event, __VerterCtx.check())");
}

#[test]
fn every_binding_a_use_declares_is_read() {
    let source = concat!(
        "<template>\n",
        "  <Child @change=\"log\" v-bind=\"attrs\" :onChange=\"count\" v-on:change=\"(value) => log(value)\" />\n",
        "</template>\n",
        "<script setup lang=\"ts\">\n",
        "import Child from './Child.vue'\n",
        "const log = (value: unknown) => value;\n",
        "const attrs = {};\n",
        "const count = 1;\n",
        "</script>\n",
    );
    let text = composed(source).checking_text().to_string();
    assert!(
        text.contains("_check0: "),
        "the use has a validation:\n{text}"
    );
    assert!(
        text.contains("_check0; ") && text.contains("_check1;\n"),
        "every validation is read:\n{text}"
    );
}

#[test]
fn a_use_with_no_authored_value_still_carries_its_obligation() {
    let source = concat!(
        "<template>\n  <Child />\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "import Child from './Child.vue'\n",
        "</script>\n",
    );
    let module = composed(source);
    let text = module.checking_text();
    let construction = offset_of(text, "= new (__VerterUseComponent(Child, ") as u32;
    let span = module
        .obligation_at(construction)
        .expect("the construction has an obligation");
    assert_eq!(span.family, ObligationFamily::ComponentUse);
    assert!(matches!(span.host, Some(UseHost::Component(_))));
    let authored = offset_of(source, "Child />") as u32;
    assert_eq!(
        mapped_point_after(&module, "new (__VerterUseComponent(", "Child"),
        authored,
        "the component maps to its tag"
    );
}

#[test]
fn native_elements_check_values_in_their_branch_and_keep_authored_mappings() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const user = {} as { disabled: boolean; accept(event: InputEvent): void } | null;\n",
        "</script>\n<template><input v-if=\"user\" :disabled=\"user.disabled\" ",
        "@input=\"user.accept($event)\" /></template>\n",
    );
    let module = composed(source);
    let text = module.checking_text();
    assert_eq!(text.matches("declare function __VerterNative<").count(), 1);
    let branch = offset_of(text, "if (__VerterCtx.user) {");
    let construction = offset_of(text, "= __VerterNative(\"input\", \"html\")");
    assert!(construction > branch, "{text}");
    offset_of(text, "if (!(__VerterCtx.user)) throw undefined;");
    assert_eq!(
        mapped_point_after(&module, "\"disabled\": (__VerterCtx.", "user"),
        offset_of(source, "user.disabled") as u32,
    );
    assert_eq!(
        mapped_point_after(&module, "\"disabled\": (__VerterCtx.user", ".disabled"),
        offset_of(source, "user.disabled") as u32 + 4,
    );
    assert_eq!(
        mapped_point_after(&module, "return (__VerterCtx.", "user"),
        offset_of(source, "user.accept($event)") as u32,
    );
    assert_eq!(
        mapped_point_after(&module, "return (__VerterCtx.user", ".accept($event)"),
        offset_of(source, "user.accept($event)") as u32 + 4,
    );
    let key = offset_of(text, "\"disabled\":") as u32;
    let key_origin = module
        .emission
        .mapping()
        .projected_at(key)
        .unwrap()
        .carrier
        .unwrap();
    assert_eq!(key_origin.start, offset_of(source, "user.disabled") as u32);
    let obligation = module.obligation_at(construction as u32).unwrap();
    assert_eq!(obligation.family, ObligationFamily::NativeElement);
    assert!(matches!(obligation.host, Some(UseHost::Element(_))));
    assert!(!module
        .withheld
        .iter()
        .any(|product| product.family == ObligationFamily::NativeElement));
    let alloc = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, text, oxc_span::SourceType::ts()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{text}",
        parsed.diagnostics
    );
}

#[test]
fn native_svg_refs_and_camelized_keys_compose_with_context_references() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const view = '0 0 10 10';\n",
        "function accept(el: SVGCircleElement | null) {}\n",
        "</script>\n<template><svg :view-box.camel=\"view\">",
        "<a ref=\"link\" /><circle :ref=\"el => accept(el)\" />",
        "</svg></template>\n",
    );
    let module = composed(source);
    let text = module.checking_text();
    offset_of(text, "__VerterNative(\"a\", \"svg\")");
    offset_of(text, "__VerterNative(\"circle\", \"svg\")");
    offset_of(text, "\"viewBox\": (__VerterCtx.view)");
    offset_of(text, "\"ref\": (el => __VerterCtx.accept(el))");
    assert_eq!(
        mapped_point_after(&module, "\"ref\": (el => __VerterCtx.", "accept"),
        offset_of(source, "accept(el)") as u32,
    );
    assert!(!module
        .withheld
        .iter()
        .any(|product| product.family == ObligationFamily::NativeElement));
    let alloc = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, text, oxc_span::SourceType::ts()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{text}",
        parsed.diagnostics
    );
}

#[test]
fn slot_outlets_payloads_are_checked_and_mapped_inside_their_branch() {
    let source = r#"<script setup lang="ts" generic="T">
defineProps<{ item: T; visible: boolean }>();
defineSlots<{ default(props: { item: T }): unknown }>();
</script><template><slot v-if="visible" :item="item" >{{ item }}</slot></template>"#;
    let module = composed(source);
    assert!(!module
        .withheld
        .iter()
        .any(|p| p.family == ObligationFamily::SlotOutlet));
    assert!(
        module.checking_text().contains("__VerterOutlet("),
        "{}",
        module.checking_text()
    );
    let payload = module
        .obligations
        .iter()
        .find(|o| o.family == ObligationFamily::SlotOutlet)
        .expect("outlet obligation");
    assert!(matches!(payload.host, Some(UseHost::Element(_))));
    let text = module.checking_text();
    assert!(
        offset_of(text, "if (__VerterCtx.visible)") < offset_of(text, "__VerterOutlet(null! as")
    );
    let authored = source.find(":item=\"item").unwrap() as u32 + 7;
    assert_eq!(
        mapped_point_after(&module, "\"item\": (__VerterCtx.", "item"),
        authored
    );
    let key = offset_of(text, "\"item\": (__VerterCtx.item)") as u32;
    assert_eq!(
        module
            .emission
            .mapping()
            .projected_at(key)
            .expect("payload key anchor")
            .carrier
            .expect("authored value")
            .start,
        authored
    );
    let alloc = oxc_allocator::Allocator::default();
    let parsed = verter_parser::oxc_parse::Parser::new(
        &alloc,
        module.checking_text(),
        oxc_span::SourceType::ts(),
    )
    .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{}",
        parsed.diagnostics,
        module.checking_text()
    );

    let source = r#"<script setup lang="ts" generic="T">
import Child from './Child.vue';
const items = [] as T[];
defineSlots<{ default(props: { item: T }): unknown }>();
</script><template><Child :items="items"><template #default="{ item }"><slot :item="item" /></template></Child></template>"#;
    let module = composed(source);
    let text = module.checking_text();
    assert!(!module
        .withheld
        .iter()
        .any(|p| p.family == ObligationFamily::SlotOutlet));
    assert!(text.contains("declare function __VerterSlotName"));
    assert!(text.contains("\"item\": (item)"), "{text}");
    assert!(offset_of(text, "const { item } =") < offset_of(text, "__VerterOutlet(null! as"));
    let alloc = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, text, oxc_span::SourceType::ts()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{text}",
        parsed.diagnostics
    );
}

#[test]
fn fallthrough_declarations_follow_runtime_roots() {
    for (template, expected) in [
        (
            "<div><span /></div>",
            Some("__VerterNativeAttributes<\"div\", \"html\">"),
        ),
        ("<div/><span/>", None),
        ("<div/>significant text", None),
        (
            "<!-- note --><a/>",
            Some("__VerterNativeAttributes<\"a\", \"html\">"),
        ),
    ] {
        let source = format!("<script setup lang=\"ts\">defineProps<{{ title?: number }}>();</script><template>{template}</template>");
        let module = composed(&source);
        assert!(
            !module
                .withheld
                .iter()
                .any(|p| p.family == ObligationFamily::Fallthrough),
            "{template}: {:?}",
            module.withheld
        );
        let output = module.checking_text();
        let props = output
            .lines()
            .find(|line| line.starts_with("type __VerterPublicProps ="))
            .expect("public props");
        match expected {
            Some(root) => assert!(props.contains(root), "{template}: {props}"),
            None => assert!(
                !props.contains("__VerterNativeAttributes"),
                "{template}: {props}"
            ),
        }
        assert!(output.contains("readonly $attrs:"), "{output}");
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            verter_parser::oxc_parse::Parser::new(&allocator, output, oxc_span::SourceType::ts())
                .parse();
        assert!(
            parsed.diagnostics.is_empty(),
            "{:?}\n{output}",
            parsed.diagnostics
        );
    }
}

#[test]
fn fallthrough_component_roots_withhold_extras_without_widening_numeric_props() {
    let module = composed(
        r#"<script setup lang="ts">
import Child from './Child.vue';
defineProps<{ value: number }>();
</script><template><Child :value="value"/></template>"#,
    );
    let text = module.checking_text();
    assert!(
        module
            .withheld
            .iter()
            .any(|product| product.family == ObligationFamily::Fallthrough),
        "{:?}",
        module.withheld
    );
    let props = text
        .lines()
        .find(|line| line.starts_with("type __VerterPublicProps ="))
        .unwrap();
    assert!(props.contains("({ value: number })"), "{props}");
    assert!(!props.contains("__VerterFallthrough"), "{props}");
    assert!(
        !text.contains("format: (value: unknown) => string"),
        "{text}"
    );
    assert!(
        !text.contains("__VerterUseContract<typeof Child>"),
        "{text}"
    );
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, text, oxc_span::SourceType::ts()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{text}",
        parsed.diagnostics
    );
}

#[test]
fn fallthrough_native_declaration_omits_model_writes() {
    let module = composed(
        r#"<script setup lang="ts">
import Child from './Child.vue';
let title = 'hello';
</script><template><div v-model:title.trim="title"/></template>"#,
    );
    let text = module.checking_text();
    assert!(
        !module
            .withheld
            .iter()
            .any(|product| product.family == ObligationFamily::Fallthrough),
        "{:?}",
        module.withheld
    );
    let props = text
        .lines()
        .find(|line| line.starts_with("type __VerterPublicProps ="))
        .unwrap();
    assert!(
        props.contains("__VerterNativeKey<\"title\"> | __VerterNativeKey<\"onUpdate:title\"> | __VerterNativeKey<\"titleModifiers\">"),
        "{props}"
    );
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, text, oxc_span::SourceType::ts()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{text}",
        parsed.diagnostics
    );
}

const LISTENER_OBJECT: &str = concat!(
    "<template>\n  <Child v-on=\"handlers\" />\n</template>\n",
    "<script setup lang=\"ts\">\n",
    "import Child from './Child.vue';\n",
    "const handlers = { save: (id: number) => id };\n",
    "</script>\n",
);

#[test]
fn a_listener_object_is_checked_against_the_use_after_its_construction() {
    let module = composed(LISTENER_OBJECT);
    let text = module.checking_text();
    let declaration = offset_of(text, "declare function __VerterListeners<");
    let construction = offset_of(text, "const __VerterUse_");
    let check = offset_of(text, "__VerterListeners(__VerterUse_");
    assert!(
        declaration < construction && construction < check,
        "the declaration, the construction, then the check:\n{text}"
    );
    offset_of(text, ", (__VerterCtx.handlers));\n");
    assert!(
        module
            .withheld
            .iter()
            .all(|product| product.family != ObligationFamily::Listener),
        "the listener product is composed, not withheld"
    );
    let authored = offset_of(LISTENER_OBJECT, "handlers\" />") as u32;
    assert_eq!(
        mapped_point_after(&module, ", (__VerterCtx.", "handlers"),
        authored,
        "the object maps to the authored value"
    );
}

#[test]
fn a_listener_object_on_a_withheld_use_is_not_checked_against_a_missing_witness() {
    let source = concat!(
        "<template>\n  <component :is=\"choice.component\" v-bind=\"choice.props\" v-on=\"handlers\" />\n</template>\n",
        "<script setup lang=\"ts\">\n",
        "import Child from './Child.vue';\n",
        "const choice = { component: Child, props: {} };\n",
        "const handlers = {};\n",
        "</script>\n",
    );
    let module = composed(source);
    assert!(!module.checking_text().contains("__VerterListeners("));
    assert!(module
        .withheld
        .iter()
        .any(|product| product.family == ObligationFamily::Listener));
}

#[test]
fn the_composition_carries_the_declaration_product_of_its_public_contract() {
    let clean = concat!(
        "<script setup lang=\"ts\">\n",
        "import { ref, type Ref } from 'vue';\n",
        "const count: Ref<number> = ref(0);\n",
        "const secret = count.value * 2;\n",
        "defineProps<{ label: string }>();\n",
        "defineExpose({ count });\n",
        "</script>\n",
    );
    // Only a composition that asks for it builds the product.
    assert!(composed(clean).declarations.is_none());
    let module = composed_declaring(clean);
    let product = module
        .declarations
        .as_ref()
        .expect("a clean carrier carries its declaration product");
    assert!(product.is_publishable(), "{:?}", product.diagnostics);
    assert_eq!(product.module.canonical_id, CANONICAL);
    let declaration = &product.declaration_source;
    assert!(declaration.contains("export default"), "{declaration}");
    // The setup body and its scope stay in the checking text.
    assert!(module.checking_text().contains(SETUP_SCOPE));
    for private in [SETUP_SCOPE, "secret", "ref(0)"] {
        assert!(!declaration.contains(private), "{private}:\n{declaration}");
    }

    // The template's declaration extras reach the product: the root's
    // fallthrough types the published `$attrs`.
    let templated = concat!(
        "<script setup lang=\"ts\">defineProps<{ label: string }>();</script>",
        "<template><a :title=\"label\"/></template>",
    );
    let product = composed_declaring(templated)
        .declarations
        .expect("a templated carrier carries its declaration product");
    assert!(
        product.declaration_source.contains("readonly $attrs: "),
        "{}",
        product.declaration_source
    );

    // A carrier with syntax errors publishes nothing.
    let recovered = composed_declaring(DANGLING_MEMBER);
    assert!(!recovered.recovery.is_complete());
    assert!(recovered.declarations.is_none());

    // An external script's instance is its own module's.
    let source = "<script lang=\"ts\" src=\"./logic.ts\"></script>\n";
    let binding = external_binding(
        source,
        ExternalBlockKind::Script,
        "./logic.ts",
        ExternalLegality::Admitted,
    );
    let module = compose_external(source, &binding).expect("an admitted external script composes");
    assert!(module.declarations.is_none());
}

/// Every cleanly composed conformance carrier carries its declaration
/// product. Also reports what building it adds to one composition.
#[test]
fn every_cleanly_composed_corpus_carrier_carries_its_declaration_product() {
    use std::time::{Duration, Instant};

    let carriers = corpus_carriers();
    let (mut composed_clean, mut publishable) = (0usize, 0usize);
    let (mut checking, mut declaring) = (Duration::ZERO, Duration::ZERO);
    for (canonical, source) in &carriers {
        let (canonical, source) = (canonical.as_str(), source.as_str());
        let artifact = artifact(source);
        let input = CheckingModuleInput {
            source,
            artifact: &artifact,
            canonical_id: canonical,
            custom_elements: None,
            external: None,
            declarations: true,
        };
        let checking_only = CheckingModuleInput {
            declarations: false,
            ..input
        };
        let Ok(module) = compose_checking_module(&input) else {
            continue;
        };
        if !module.recovery.is_complete() {
            assert!(module.declarations.is_none(), "{canonical}");
            continue;
        }
        let product = module
            .declarations
            .as_ref()
            .unwrap_or_else(|| panic!("{canonical} composed without its declaration product"));
        composed_clean += 1;
        publishable += usize::from(product.is_publishable());
        let unasked = compose_checking_module(&checking_only).expect("composes again");
        assert!(unasked.declarations.is_none(), "{canonical}");
        assert_eq!(
            unasked.checking_text(),
            module.checking_text(),
            "{canonical}"
        );
        for _ in 0..2 {
            let started = Instant::now();
            let _ = compose_checking_module(&checking_only);
            checking += started.elapsed();
            let started = Instant::now();
            let _ = compose_checking_module(&input);
            declaring += started.elapsed();
        }
    }
    assert!(composed_clean > 0, "the corpus composes no carrier");
    eprintln!(
        "declaration product cost: {} carriers, {composed_clean} composed cleanly, \
         {publishable} publishable; checking-only {checking:?}, with declarations \
         {declaring:?} ({:+.2}%)",
        carriers.len(),
        (declaring.as_secs_f64() / checking.as_secs_f64().max(f64::EPSILON) - 1.0) * 100.0,
    );
}

/// The conformance corpus carriers, sorted, as `(canonical id, source)` with
/// line ends normalized.
fn corpus_carriers() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../verter_vue_conformance/corpus/cases");
    let mut pending = vec![root.clone()];
    let mut paths = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("corpus directory") {
            let path = entry.expect("corpus entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "vue") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let canonical = format!(
                "/corpus/{}",
                path.strip_prefix(&root)
                    .expect("under the corpus")
                    .to_string_lossy()
                    .replace('\\', "/")
            );
            let source = std::fs::read_to_string(path)
                .expect("corpus carrier")
                .replace("\r\n", "\n");
            (canonical, source)
        })
        .collect()
}

/// Carriers that reach the projection's own references to Vue: type, short
/// and runtime emits, models, slots, expose, a generic binder, built-in
/// components, custom directives, class and style merging, a dynamic
/// component and a global component fallback.
const VUE_SURFACE_CARRIERS: &[(&str, &str)] = &[
    (
        "typed-generic",
        concat!(
            "<script setup lang=\"ts\" generic=\"T extends string\">\n",
            "const props = withDefaults(defineProps<{ item: T; size?: number }>(), { size: 1 });\n",
            "const emit = defineEmits<{ (e: 'title', value: T): void; (e: 'close'): void }>();\n",
            "const open = defineModel<boolean>('open');\n",
            "defineSlots<{ default(props: { item: T }): any }>();\n",
            "function reset() { emit('close'); }\n",
            "defineExpose({ reset });\n",
            "</script>\n",
            "<template><div :class=\"['a', { b: open }]\" :style=\"{ color: 'red' }\"><slot :item=\"props.item\" /></div></template>\n",
        ),
    ),
    (
        "short-emits-and-builtins",
        concat!(
            "<script setup lang=\"ts\">\n",
            "import { ref } from 'vue';\n",
            "import Child from './Child.vue';\n",
            "const emit = defineEmits<{ save: [id: number]; cancel: [] }>();\n",
            "const vFocus = { mounted(el: HTMLElement) { el.focus(); } };\n",
            "const shown = ref(true);\n",
            "const box = ref<HTMLElement | null>(null);\n",
            "</script>\n",
            "<template>\n",
            "<Transition><p v-if=\"shown\" v-focus ref=\"box\" @click=\"emit('save', 1)\">x</p></Transition>\n",
            "<TransitionGroup tag=\"ul\"><li key=\"a\">a</li></TransitionGroup>\n",
            "<KeepAlive><Child /></KeepAlive>\n",
            "<Teleport to=\"body\"><span /></Teleport>\n",
            "<Suspense><Child /></Suspense>\n",
            "<component :is=\"Child\" class=\"c\" :style=\"{ top: 0 }\" />\n",
            "<GlobalThing label=\"x\" />\n",
            "</template>\n",
        ),
    ),
    (
        "runtime-options",
        concat!(
            "<script lang=\"ts\">\nexport default { name: 'Named', inheritAttrs: false };\n</script>\n",
            "<script setup lang=\"ts\">\n",
            "defineProps({ count: { type: Number, required: true } });\n",
            "defineEmits(['change']);\n",
            "</script>\n",
            "<template><span class=\"a\" :class=\"{ b: count }\">{{ count }}</span></template>\n",
        ),
    ),
];

/// Every member of `vue` the text names through an import type or a type
/// query: `import("vue").Name` or `import("vue")["name"]`, in either quote.
/// A whole-module reference names no member.
fn vue_members_named_in(text: &str, names: &mut std::collections::BTreeSet<String>) {
    for quote in ['"', '\''] {
        let opener = format!("import({quote}vue{quote})");
        let mut rest = text;
        while let Some(at) = rest.find(&opener) {
            rest = &rest[at + opener.len()..];
            let member = rest
                .strip_prefix('.')
                .or_else(|| rest.strip_prefix("[\"").or_else(|| rest.strip_prefix("['")));
            let Some(member) = member else { continue };
            let name: String = member
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            assert!(!name.is_empty(), "unnamed vue member before {member:.40}");
            names.insert(name);
        }
    }
}

/// The checking module and the declaration product name only what Vue 3.5
/// exports. A name Vue declares without exporting is TS2694 in every
/// project on that version, and the type that reads it collapses.
#[test]
fn projected_vue_references_are_exported_by_vue_3_5() {
    let exported: std::collections::BTreeSet<&str> =
        include_str!("fixtures/vue-3.5.42-exports.txt")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
    let mut carriers: Vec<(String, String)> = VUE_SURFACE_CARRIERS
        .iter()
        .map(|(name, source)| (format!("/surface/{name}.vue"), (*source).to_owned()))
        .collect();
    let surface_carriers = carriers.len();
    carriers.extend(corpus_carriers());
    let mut missing = Vec::new();
    let mut named = std::collections::BTreeSet::new();
    for (index, (canonical, source)) in carriers.iter().enumerate() {
        let artifact = artifact(source);
        let module = compose_checking_module(&CheckingModuleInput {
            source,
            artifact: &artifact,
            canonical_id: canonical,
            custom_elements: None,
            external: None,
            declarations: true,
        });
        let module = match module {
            Ok(module) => module,
            Err(refusal) => {
                assert!(
                    index >= surface_carriers,
                    "{canonical} does not compose: {refusal:?}"
                );
                continue;
            }
        };
        let mut names = std::collections::BTreeSet::new();
        vue_members_named_in(module.checking_text(), &mut names);
        if let Some(product) = &module.declarations {
            vue_members_named_in(&product.declaration_source, &mut names);
        }
        for name in &names {
            if !exported.contains(name.as_str()) {
                missing.push(format!("{canonical}: {name}"));
            }
        }
        named.extend(names);
    }
    // The carriers reach the emits surfaces, so the scan reads real output.
    for reached in ["EmitFn", "EmitsToProps", "ComponentPublicInstance"] {
        assert!(
            named.contains(reached),
            "no carrier reaches {reached}: {named:?}"
        );
    }
    assert!(missing.is_empty(), "not exported by vue 3.5: {missing:#?}");
}

#[test]
fn a_component_prop_key_stands_at_its_authored_attribute() {
    let source = concat!(
        "<script setup lang=\"ts\">import Child from './Child.vue';</script>",
        "<template><Child declaredProp=\"x\" :count=\"1\" /></template>",
    );
    let module = composed(source);
    let text = module.checking_text();
    // TypeScript reports a member that fails the child's contract on its key.
    for (key, attribute) in [
        ("\"declaredProp\": ", "declaredProp="),
        ("\"count\": ", "count="),
    ] {
        let generated = offset_of(text, key) as u32;
        let region = module
            .emission
            .mapping()
            .projected_at(generated)
            .unwrap_or_else(|| panic!("`{key}` is projected"));
        let authored = offset_of(source, attribute) as u32;
        assert_eq!(region.class, ProjectedClass::Rewritten, "{key}\n{text}");
        assert_eq!(
            region.carrier,
            Some(MappingSpan {
                start: authored,
                end: authored
            }),
            "{key}\n{text}"
        );
    }
    assert_checking_syntax(&module);
}

#[test]
fn a_setup_block_running_to_the_end_still_checks_the_template() {
    // No closing tag: the setup contents run to the end, so no carrier bytes
    // are removed where the template scope goes.
    let source = "<template>{{ missing }}</template><script setup lang=\"ts\">const ok = 1;";
    let module = composed(source);
    let text = module.checking_text();
    let generated = offset_of(text, "missing") as u32;
    let region = module
        .emission
        .mapping()
        .projected_at(generated)
        .expect("the template read is projected");
    let carrier = region.carrier.expect("the template read is mapped");
    assert_eq!(
        carrier.start + (generated - region.generated.start),
        offset_of(source, "missing") as u32,
        "{text}"
    );
    assert!(
        module
            .obligations
            .iter()
            .any(|obligation| obligation.family == ObligationFamily::Interpolation),
        "{text}"
    );
}

/// Carriers whose templates hold no component use, or hold components only
/// among several roots, compose with every product: no listener reaches a
/// component, and a fragment's fallthrough contract never names a child.
/// The carriers are the type checker's own fixtures.
#[test]
fn carriers_without_listeners_or_component_roots_are_fully_checked() {
    macro_rules! fixture {
        ($path:literal) => {
            (
                $path,
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../verter_tsc/tests/cases/fixtures/",
                    $path
                )),
            )
        };
    }
    for (path, source) in [
        fixture!("fallthrough/src/ChildDiv.vue"),
        fixture!("fallthrough/src/ChildAnchorRoot.vue"),
        fixture!("fallthrough/src/ChildMultiRoot.vue"),
        fixture!("fallthrough/src/ChildNoInherit.vue"),
        fixture!("fallthrough/src/ChildOptionalProp.vue"),
        fixture!("fallthrough/src/ChildTyped.vue"),
        fixture!("fallthrough/src/Grandchild.vue"),
        fixture!("fallthrough/src/OptionsApiAnchorRoot.vue"),
        fixture!("fallthrough/src/ScriptlessAnchorRoot.vue"),
        fixture!("diagnostics/src/EmitErrors.vue"),
        fixture!("diagnostics/src/GenericInstanceErrors.vue"),
        fixture!("diagnostics/src/OptionsApiConsumer.vue"),
        fixture!("js_carrier/src/ImportsRefused.vue"),
    ] {
        let source = source.replace("\r\n", "\n");
        let module = composed(&source);
        assert_eq!(module.withheld, Vec::new(), "{path}");
        assert!(module.is_complete(), "{path}");
    }
}

#[test]
fn an_asserted_model_target_maps_its_parentheses_to_the_authored_target() {
    let source = concat!(
        "<script setup lang=\"ts\">import { ref } from 'vue'; ",
        "const text = ref<string | number>('');</script>",
        "<template><input v-model=\"text as string\" /></template>"
    );
    let module = composed(source);
    let output = module.checking_text();
    let open = offset_of(output, "(__VerterCtx.text as string) = null!") as u32;
    let close = open + "(__VerterCtx.text as string".len() as u32;
    let authored = offset_of(source, "text as string") as u32;
    let mapping = module.emission.mapping();
    for at in [open, close] {
        let region = mapping.projected_at(at).expect("mapped");
        assert_eq!(region.carrier.map(|c| c.start), Some(authored), "{output}");
    }
}
