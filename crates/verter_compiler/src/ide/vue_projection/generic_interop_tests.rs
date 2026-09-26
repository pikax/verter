use super::attribute_operations::project_attribute_operations;
use super::component_use::{project_component_uses, USE_CONSTRUCTOR, USE_PRELUDE, WITNESS_PREFIX};
use super::generic_interop::*;
use super::public_constructor::{project_public_constructor, public_binder};
use super::script_setup::{ScriptBlockInput, SetupProjectionRefusal};
use crate::cursor::ScriptLanguage;
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput};

const PICKER_GENERIC: &str =
    "const T extends { id: number }, K extends keyof T = keyof T, Extra extends unknown[] = []";

const PICKER_SETUP: &str = r#"import type { VNode } from "vue";
defineProps<{ items: readonly T[]; field: K; format: <V>(value: V) => string; extra?: Extra }>();
defineEmits<{ pick: [item: T, key: K] }>();
defineSlots<{ default(props: { item: T; value: T[K]; map: <R>(project: (item: T) => R) => R[] }): VNode[] }>();
"#;

/// The parent binder every probe parent declares.
const PARENT_GENERIC: &str = "T extends { id: number; label: string }";

/// Positive parent: a forwarding generic use with a higher-rank slot,
/// explicit aliases through a re-export and a namespace, Options API,
/// generic setup-function, functional and generic functional components,
/// two uses of a six-overload constructor, an erased and an untyped
/// component, and non-last overloads of a constructor ending in an open
/// catch-all and of an overloaded functional component.
const POSITIVE_TEMPLATE: &str = concat!(
    "  <Picker :items=\"props.items\" field=\"label\" :format=\"(value) => String(value)\" @pick=\"(item, key) => onPick(item, key)\">\n",
    "    <template #default=\"{ item, map }\">{{ item.label }}{{ map((entry) => entry.id) }}</template>\n",
    "  </Picker>\n",
    "  <BarrelPicker :items=\"rows\" field=\"id\" :format=\"describe\" @pick=\"(row) => log(row.label)\" />\n",
    "  <pickers.RowPicker :items=\"rows\" field=\"id\" :format=\"<V,>(value: V) => JSON.stringify(value)\" />\n",
    "  <Counter :count=\"total\" @bump=\"(by) => (total += by)\" />\n",
    "  <Choice value=\"b\" :options=\"choices\" @change=\"(value) => choose(value)\" />\n",
    "  <Badge :level=\"2\" @dismiss=\"(level) => dismiss(level)\">\n",
    "    <template #default=\"{ level }\">{{ level }}</template>\n",
    "  </Badge>\n",
    "  <Cell :value=\"3\" :render=\"(value) => value.toFixed(1)\" />\n",
    "  <Shape kind=\"circle\" :radius=\"1\" />\n",
    "  <Shape kind=\"polygon\" :sides=\"5\" />\n",
    "  <ErasedList :items=\"rows\" />\n",
    "  <Untyped :anything=\"rows\" />\n",
    "  <Gauge unit=\"celsius\" :value=\"21\" />\n",
    "  <Gauge unit=\"percent\" :ratio=\"0.5\" />\n",
    "  <Toggle mode=\"on\" :level=\"2\" />\n",
    "  <Toggle mode=\"off\" reason=\"idle\" />\n",
    "  <Menu :items=\"['a']\" :flag=\"true\" />\n",
    "  <Select value=\"a\" :options=\"['a']\" />\n",
    "  <Mix kind=\"g\" value=\"v\" />"
);

/// Setup statements the positive probe supplies to the scope, including the
/// hoisted `observe` function that reads each use's witness.
const POSITIVE_SETUP: &str = r#"const props = defineProps<{ items: T[] }>();
const rows: Row[] = [];
const describe = (value: unknown) => String(value);
function onPick(item: T, key: keyof T) {
  return [item, key];
}
function log(text: string) {
  return text;
}
let total = 0;
const choices: ("a" | "b")[] = ["a", "b"];
function choose(value: "a" | "b") {
  return value;
}
function dismiss(level: 1 | 2 | 3) {
  return level;
}
function observe() {
  const pickerSlot = {} as __VerterUseSlotProps<typeof __VerterUse_7d12abeb7579a548, "default">;
  const forwarded: T = pickerSlot.item;
  const forwardedLabel: T["label"] = pickerSlot.value;
  const stp19HoverTarget = pickerSlot.map((entry) => entry.id);
  const labels: string[] = pickerSlot.map((entry) => entry.label);
  const format = {} as __VerterUseProp<typeof __VerterUse_0b8d07d8215f8d9f, "format">;
  const formatted: [string, string] = [format(1), format({ nested: true })];
  const barrelSlot = {} as __VerterUseSlotProps<typeof __VerterUse_de28af435ed10d58, "default">;
  const explicitItem: Row = barrelSlot.item;
  const explicitValue: number = barrelSlot.value;
  const explicitListener: __VerterUseListener<typeof __VerterUse_de28af435ed10d58, "onPick"> = (item, key) => [item.label, key] as const;
  const namespaceInstance: InstanceType<typeof pickers.RowPicker> = __VerterUse_0b8d07d8215f8d9f;
  const bump: __VerterUseListener<typeof __VerterUse_c00ddc48b3efc0e3, "onBump"> = (by) => by.toFixed();
  const counted: number = __VerterUse_c00ddc48b3efc0e3.$props.count;
  const chosen: "a" | "b" = __VerterUse_73fa95a4c37e26b8.$props.value;
  const badgeSlot = {} as __VerterUseSlotProps<typeof __VerterUse_38e3f1df3d80f389, "default">;
  const level: 1 | 2 | 3 = badgeSlot.level;
  const cellValue: number = __VerterUse_d9fde548ef6c26b1.$props.value;
  const circle: "circle" = __VerterUse_69386ace753053c3.kind;
  const polygon: "polygon" = __VerterUse_be4d6146599d76b8.kind;
  const erasedSlot = {} as __VerterUseSlotProps<typeof __VerterUse_fe63724fd35964d9, "default">;
  const erasedIsUnknown: IsExactlyUnknown<typeof erasedSlot.item> = true;
  const untypedIsAny: IsAny<typeof __VerterUse_88698eb00ab7e0aa> = true;
  const celsius: "celsius" = __VerterUse_0f68882e583e0974.unit;
  const percent: "percent" = __VerterUse_d206613c79de26f0.unit;
  const toggledOn: number = __VerterUse_b5063f18761348d2.$props.level;
  const toggledOff: string = __VerterUse_254f2d410a2b7939.$props.reason;
  const flagged: boolean = __VerterUse_8465246605fe46b7.$props.flag;
  const selected: string = __VerterUse_593dab2aba76bd47.value;
  const mixed: "g" = __VerterUse_9b18a6441c8960f4.$props.kind;
  return [forwarded, forwardedLabel, stp19HoverTarget, labels, formatted, explicitItem, explicitValue, explicitListener, namespaceInstance, bump, counted, chosen, level, cellValue, circle, polygon, erasedIsUnknown, untypedIsAny, celsius, percent, toggledOn, toggledOff, flagged, selected, mixed];
}
"#;

/// Negative parent: functional props and collected listeners that contradict
/// the published contract (TS2322).
const NEGATIVE_TEMPLATE: &str = concat!(
    "  <Badge :level=\"4\" />\n",
    "  <Badge :level=\"1\" @dismiss=\"dismiss\" v-on:dismiss=\"(level: string) => level\" />\n",
    "  <BarrelPicker :items=\"rows\" field=\"id\" :format=\"describe\" @pick=\"(row) => row.id\" v-on:pick=\"count\" />\n",
    "  <Picker :items=\"props.items\" field=\"id\" :format=\"String\" />\n",
    "  <ErasedList :items=\"rows\" />"
);

const NEGATIVE_SETUP: &str = r#"const props = defineProps<{ items: T[] }>();
const rows: Row[] = [];
const describe = (value: unknown) => String(value);
function dismiss(level: 1 | 2 | 3) {
  return level;
}
function count(total: number) {
  return total;
}
function observe() {
  const pickerSlot = {} as __VerterUseSlotProps<typeof __VerterUse_8c747e13a84a8163, "default">;
  const labels: string[] = pickerSlot.map((entry) => entry.id);
  const erasedSlot = {} as __VerterUseSlotProps<typeof __VerterUse_fe63724fd35964d9, "default">;
  const erasedItem: number = erasedSlot.item;
  return [labels, erasedItem];
}
"#;

/// Construction-negative parent: each construction violates both the exact
/// contract and the tolerant fallback (TS2769).
const CONSTRUCTION_TEMPLATE: &str = concat!(
    "  <Picker :items=\"props.items\" field=\"missing\" :format=\"String\" />\n",
    "  <BarrelPicker :items=\"rows\" field=\"label\" :format=\"describe\" />\n",
    "  <Counter :count=\"'1'\" />\n",
    "  <Shape kind=\"polygon\" :sides=\"'five'\" />\n",
    "  <Select :label=\"1\" />"
);

const CONSTRUCTION_SETUP: &str = r#"const props = defineProps<{ items: T[] }>();
const rows: Row[] = [];
const describe = (value: unknown) => String(value);
"#;

fn sfc(generic: Option<&str>, template: &str) -> String {
    let generic = generic
        .map(|g| format!(" generic=\"{g}\""))
        .unwrap_or_default();
    format!(
        "<script setup lang=\"ts\"{generic}>\nimport Picker from './components/Picker.vue';\n</script>\n<template>\n{template}\n</template>\n"
    )
}

fn project_with(generic: Option<&str>, template: &str) -> AdvancedGenericUseProjection {
    let source = sfc(generic, template);
    let parsed = crate::compile::parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///Parent.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
    });
    let attributes = project_attribute_operations(&plan, &parsed, &source);
    let uses = project_component_uses(&plan, &attributes);
    let binder = public_binder(generic).expect("binder parses");
    project_advanced_generic_uses(&plan, uses, binder)
}

fn project(template: &str) -> AdvancedGenericUseProjection {
    project_with(Some(PARENT_GENERIC), template)
}

/// The declared result of every adapter declaration: a type alias's
/// right-hand side, a function's return type.
fn declared_results() -> Vec<&'static str> {
    ForeignComponentContractAdapter::DECLARATIONS
        .lines()
        .map(|line| match line.strip_prefix("declare function ") {
            Some(function) => function.rsplit_once("): ").map_or("", |(_, ret)| ret),
            None => line.split_once(" = ").map_or("", |(_, rhs)| rhs),
        })
        .collect()
}

/// Every use applies the adapter: the component's exact contract first,
/// then its attribute-tolerant signature, each over the component
/// expression itself — never a derived single signature, which keeps only
/// one construct overload.
#[test]
fn generic_use_construction_applies_the_exact_contract_first() {
    let projection = project(POSITIVE_TEMPLATE);
    assert!(projection.complete);
    let components: Vec<&str> = projection
        .witnesses
        .witnesses
        .iter()
        .map(|w| w.component.as_str())
        .collect();
    assert_eq!(
        components,
        vec![
            "Picker",
            "BarrelPicker",
            "pickers.RowPicker",
            "Counter",
            "Choice",
            "Badge",
            "Cell",
            "Shape",
            "Shape",
            "ErasedList",
            "Untyped",
            "Gauge",
            "Gauge",
            "Toggle",
            "Toggle",
            "Menu",
            "Select",
            "Mix",
        ]
    );
    for witness in &projection.witnesses.witnesses {
        let c = &witness.component;
        assert!(
            witness.render().starts_with(&format!(
                "const {} = new ({USE_COMPONENT}({c}, {USE_CONSTRUCTOR}({c})))({{ ",
                witness.binding
            )),
            "{}",
            witness.render()
        );
    }
    let rendered = projection.render(POSITIVE_SETUP);
    assert!(!rendered.contains(&format!("new ({USE_CONSTRUCTOR}(")));
    let dynamic =
        project("  <component :is=\"pick ? Shape : Badge\" kind=\"point\" :level=\"1\" />");
    assert_eq!(
        ForeignComponentContractAdapter
            .construction_callee(&dynamic.witnesses.witnesses[0].component),
        format!(
            "{USE_COMPONENT}((pick ? Shape : Badge), {USE_CONSTRUCTOR}((pick ? Shape : Badge)))"
        )
    );
}

/// The adapter declarations keep every published component shape: a typed
/// constructor passes through whole, a constructor ending in an
/// open-argument (`...args: any[]`) signature is rebuilt signature by
/// signature with each open signature taking the `$props` its instance
/// publishes, an overloaded callable is rebuilt signature by signature, a
/// single call signature reaches the construction through higher-order
/// inference, and no declared result introduces `any`.
#[test]
fn foreign_contract_declarations_keep_published_shapes() {
    let declarations = ForeignComponentContractAdapter::DECLARATIONS;
    assert!(USE_PRELUDE.starts_with(declarations));
    assert_eq!(USE_PRELUDE.matches(declarations).count(), 1);
    let contract = declarations
        .lines()
        .find(|line| line.starts_with(&format!("type {USE_CONTRACT}<C> = ")))
        .expect("contract declaration");
    assert!(contract.contains(&format!(
        "__VerterUseTupleRest<C> extends true ? C : C extends abstract new (...args: infer A) => unknown ? ({USE_OPEN_ARGS}<A> extends true ? {USE_CONSTRUCTS}<C, unknown, never, []> : C) : {USE_CALLS}<C, unknown, never, []>;"
    )));
    assert!(declarations.contains(
        "type __VerterUseTupleRest<C> = C extends new <T extends any[]>(...args: T) => { readonly args: T } ? true : C extends new <T extends readonly any[]>(...args: T) => { readonly args: T } ? true : false;"
    ));
    assert!(declarations.contains(&format!(
        "type {USE_OPEN_ARGS}<A> = __VerterUseSame<A, any[]>;"
    )));
    assert!(declarations.contains(
        "(I extends { readonly $props: infer P } ? new (props: P) => I : new (props: Record<string, never>) => I)"
    ));
    let tolerant = declarations
        .lines()
        .find(|line| line.starts_with(&format!("type {USE_TOLERANT}<P, I> = ")))
        .expect("tolerant declaration");
    assert!(tolerant
        .contains("(0 extends 1 & P ? (I extends { readonly $props: infer Q } ? Q : (0 extends 1 & I ? P : {})) : P)"));
    assert!(declarations.contains(&format!(
        "declare function {USE_CONSTRUCTOR}<P, X, R>(component: (props: P, ctx: X) => R): new (props: P & Record<string, unknown>) => {USE_FUNCTIONAL}<P, X>;"
    )));
    assert!(declarations.contains(&format!(
        "declare function {USE_CONSTRUCTOR}<C extends new <T extends any[]>(...args: T) => {{ readonly args: T }}>(component: C): C;"
    )));
    assert!(declarations.contains(&format!(
        "declare function {USE_CONSTRUCTOR}<P, I>(component: abstract new (props: P) => I): new (props: {USE_TOLERANT}<P, I>) => I;"
    )));
    assert!(declarations.contains(&format!(
        "declare function {USE_COMPONENT}<C, A>(component: C, tolerant: A): {USE_CONTRACT}<C> & A;"
    )));
    let results = declared_results();
    assert_eq!(results.len(), declarations.lines().count());
    for result in results {
        assert!(!result.is_empty());
        let without_any_probe = result
            .replace("readonly any[]", "")
            .replace("any[]", "")
            .replace("0 extends 1 & ", "");
        assert!(
            !without_any_probe.contains("any"),
            "adapter result introduces any: {result}"
        );
    }
}

/// Every use sits in one scope over the parent's authored binder — `const`,
/// constraints, dependent and defaulted parameters verbatim — after the
/// supplied setup statements, so a forwarded parameter stays itself.
#[test]
fn generic_use_scope_carries_the_authored_binder() {
    let generic =
        "const T extends readonly unknown[], U extends keyof T = keyof T, Rest extends unknown[] = []";
    let projection = project_with(
        Some(generic),
        "  <Picker :items=\"items\" field=\"length\" />",
    );
    assert_eq!(projection.scope_binder(), format!("<{generic}>"));
    let witness = &projection.witnesses.witnesses[0];
    let rendered = projection.render("const items = [] as unknown as T;");
    assert_eq!(
        rendered,
        format!(
            "{USE_PRELUDE}function {USE_SCOPE}<{generic}>() {{\nconst items = [] as unknown as T;\n{}}}\n",
            witness.render()
        )
    );
    let plain = project_with(None, "  <Picker :items=\"items\" />");
    assert!(plain.binder.is_empty());
    assert_eq!(
        plain.render(""),
        format!(
            "{USE_PRELUDE}function {USE_SCOPE}() {{\n{}}}\n",
            plain.witnesses.witnesses[0].render()
        )
    );
}

/// A use whose component cannot be named has no witness and none is
/// fabricated: it is recorded unavailable and the product stays
/// incomplete, while its sibling keeps its own witness.
#[test]
fn generic_use_availability_keeps_unavailable_uses_unwitnessed() {
    let projection =
        project("  <Picker :items=\"rows\" />\n  <component :is=\"\" :items=\"rows\" />");
    assert!(!projection.complete);
    assert_eq!(projection.uses.len(), 2);
    assert_eq!(projection.witnesses.witnesses.len(), 1);
    let witness = &projection.witnesses.witnesses[0];
    assert_eq!(
        projection.uses[0].availability,
        UseContractAvailability::Witnessed {
            binding: witness.binding.clone()
        }
    );
    assert_eq!(
        projection.uses[1].availability,
        UseContractAvailability::Unavailable
    );
    assert_eq!(
        projection.use_record(&projection.uses[1].use_id),
        Some(&projection.uses[1])
    );
    let rendered = projection.render("");
    assert_eq!(rendered.matches(" = new (").count(), 1);
    // The construction names the witness, and its direct-key check names it again.
    assert_eq!(rendered.matches(WITNESS_PREFIX).count(), 2);
}

/// Every authored expression a witness carries — including the type
/// argument tokens of a generic arrow — keeps its admitted origin: the
/// plan's occurrence slices the SFC source to exactly the spelling the
/// witness renders, so the mapping layer anchors each generic argument
/// token at its authored bytes instead of at generated text.
#[test]
fn generic_use_witness_values_keep_their_authored_origin() {
    let source = sfc(Some(PARENT_GENERIC), POSITIVE_TEMPLATE);
    let parsed = crate::compile::parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///Parent.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
    });
    let attributes = project_attribute_operations(&plan, &parsed, &source);
    let uses = project_component_uses(&plan, &attributes);
    let binder = public_binder(Some(PARENT_GENERIC)).expect("binder parses");
    let projection = project_advanced_generic_uses(&plan, uses, binder);
    let rendered = projection.render(POSITIVE_SETUP);
    let mut generic_argument_origins = 0;
    for witness in &projection.witnesses.witnesses {
        let component = plan
            .expression(&witness.component_expression)
            .expect("component expression is admitted");
        assert_eq!(
            &source[component.start as usize..component.end as usize],
            witness.component
        );
        for member in &witness.transaction.members {
            let value = match member {
                super::component_use::TransactionMember::Property { value, .. }
                | super::component_use::TransactionMember::Spread { value, .. } => value,
            };
            let (Some(id), super::component_use::MemberValue::Expression { spelling, .. }) =
                (value.expression(), value)
            else {
                continue;
            };
            let occurrence = plan.expression(id).expect("member expression is admitted");
            let authored = &source[occurrence.start as usize..occurrence.end as usize];
            assert_eq!(authored, spelling);
            assert!(rendered.contains(&format!("({authored})")), "{authored}");
            if authored.starts_with("<V,>") {
                generic_argument_origins += 1;
            }
        }
    }
    assert_eq!(generic_argument_origins, 1);
}

/// Explicit instantiation aliases — reached through a renamed re-export and
/// a namespace — are constructed as the alias itself, and a generic arrow
/// passed to a higher-rank prop keeps its own binder verbatim.
#[test]
fn generic_use_explicit_aliases_construct_the_alias_itself() {
    let projection = project(POSITIVE_TEMPLATE);
    let rendered = projection.render(POSITIVE_SETUP);
    for alias in ["BarrelPicker", "pickers.RowPicker"] {
        assert!(rendered.contains(&format!(
            "new ({USE_COMPONENT}({alias}, {USE_CONSTRUCTOR}({alias})))({{ \"items\": (rows), \"field\": \"id\", "
        )));
    }
    assert!(rendered.contains("\"format\": (<V,>(value: V) => JSON.stringify(value)) });"));
}

/// Products are deterministic and a binder that does not parse is refused.
#[test]
fn generic_use_products_are_deterministic_and_invalid_binders_refuse() {
    assert_eq!(project(POSITIVE_TEMPLATE), project(POSITIVE_TEMPLATE));
    assert_eq!(
        public_binder(Some("T extends")),
        Err(SetupProjectionRefusal::InvalidGeneric)
    );
}

#[test]
fn picker_probe_fixture_is_the_rendered_declaration() {
    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/sfc-projection/STP19/probes/components/Picker.vue.ts"
    ));
    let contract = project_public_constructor(
        None,
        Some(ScriptBlockInput {
            content: PICKER_SETUP,
            content_start: 0,
            lang: Some(ScriptLanguage::TypeScript),
        }),
        Some(PICKER_GENERIC),
    )
    .expect("projects");
    let rendered = contract.declaration().expect("setup renders a constructor");
    assert!(
        FIXTURE.replace("\r\n", "\n").ends_with(&rendered),
        "the probe component must end with the rendered declaration:\n{rendered}"
    );
}

/// The tsc probes carry the product's own rendering: each parent probe
/// contains its template's rendered projection over its setup statements
/// byte for byte.
#[test]
fn probe_fixtures_are_the_rendered_products() {
    const POSITIVE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/sfc-projection/STP19/probes/positive.ts"
    ));
    const NEGATIVE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/sfc-projection/STP19/probes/negative.ts"
    ));
    const CONSTRUCTION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/sfc-projection/STP19/probes/negative-construction.ts"
    ));
    for (fixture, template, setup) in [
        (POSITIVE, POSITIVE_TEMPLATE, POSITIVE_SETUP),
        (NEGATIVE, NEGATIVE_TEMPLATE, NEGATIVE_SETUP),
        (CONSTRUCTION, CONSTRUCTION_TEMPLATE, CONSTRUCTION_SETUP),
    ] {
        let rendered = project(template).render(setup);
        assert!(
            fixture.replace("\r\n", "\n").contains(&rendered),
            "probe must contain the rendered projection:\n{rendered}"
        );
    }
}
