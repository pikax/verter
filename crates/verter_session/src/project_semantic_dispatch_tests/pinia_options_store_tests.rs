//! A Pinia options store — `defineStore(id, { state, getters, actions })`
//! over Pinia's options-store declarations and Vue's `UnwrapRef` — answers
//! every member of the store it returns as the checker does: state read
//! through `UnwrapRef`, getters typed by their contextual `state`
//! parameter, actions typed by the `A & ThisType<…>` receiver, the store
//! id, and `$state` with its custom-state intersection.
//!
//! The scenario is `fixtures/pinia_options_store.ts`, a reconstruction (see
//! its header). Each answer is the benchmark's named demand: a probe alias
//! resolved by name in a project of the benchmark library and the scenario,
//! projected to its wire answer. Every expected answer is TypeScript
//! 7.0.2's, read through its native API (`getTypeAtPosition` on the probe
//! alias's name, then `typeToString` with `NoTruncation | InTypeAlias`)
//! over the same files under `noLib`, in all four `strictNullChecks` ×
//! `noImplicitAny` settings, which answer alike.
//!
//! The contextual-this marker is the GLOBAL `ThisType`, identified by the
//! declaration a reference resolves to, never by its spelling: a library's
//! declaration and an authored `declare global` one are the marker; a
//! module-scope namesake is an ordinary interface, in its own module even
//! where that module also contributes the global declaration, which every
//! other module still reads as the marker; and with no declaration at all
//! the checker reports the name and recovers without the marker. Where no
//! marker types the actions, the named demand refuses the store with a
//! typed cause — the absent member an action reads off its own object
//! literal's `this`, read off the shared flow result of the store's
//! initializer — that every member then propagates; an unrelated refusal of
//! the same call, though its members print the same unknown leaf, is told
//! apart, a marked store's missing dependency included.
//!
//! The rules the store reaches are pinned on their smallest shapes too:
//! an object relating to a `Record` over index keys, inference through
//! `Pick` / `Omit` of an object or an interface, the keys of an interface
//! applied to an open type parameter, a callback typed by the one arm of a
//! union of signatures its arity admits, a statement call through an
//! object literal's contextual `this` beside one through a contextual
//! signature's explicit `this`, and a member read keeping an intersection's
//! generic-interface contributor.

use std::sync::Arc;

use verter_type_engine::semantic_query::{FlowReturnDegradation, QueryError, SemanticNodeData};
use verter_type_expr::{ObjectMember, PrimitiveName, TypeExpr};

use super::differential_harness_tests::{canonical_text, ALL};

/// The library both arms of the semantic benchmark read.
const BENCHMARK_LIBRARY: &str =
    include_str!("../../../../scripts/benchmark/semantic-perf/lib/bench-globals.d.ts");

/// The options-store scenario: Pinia's and Vue's declarations, the store,
/// and `St`, the type of the store `useCounterStore()` returns.
const STORE: &str = include_str!("fixtures/pinia_options_store.ts");

/// The global marker as TypeScript's `lib.es5.d.ts` declares it.
const LIBRARY_MARKER: &str = "interface ThisType<T> {}\n";

/// The library files of a store typed through the library's marker.
const MARKED: &[(&str, &str)] = &[("lib.marker.d.ts", LIBRARY_MARKER)];

/// A module contributing the global marker beside a module-scope namesake.
const CONTRIBUTING_MODULE: &str =
    "export {};\ninterface ThisType<T> {}\ndeclare global { interface ThisType<T> {} }\n";

/// The whole store, one member per element.
const WHOLE_STORE: &str = "[St['count'], St['items'], St['double'], St['first'], \
     ReturnType<St['increment']>, ReturnType<St['add']>, St['$id'], St['$state']]";

/// The store's state as `UnwrapRef` reads it, as TypeScript 7.0.2 prints it.
const STATE: &str = "{ count: number; items: { id: number; }[]; name: string; }";

/// `$state` as TypeScript 7.0.2 prints it: the unwrapped state and the
/// (empty) custom-state interface applied to the declared state.
fn custom_state() -> String {
    format!("{STATE} & PiniaCustomStateProperties<{STATE}>")
}

/// The utility declarations the minimal shapes read beside the benchmark
/// library.
const UTILITIES: &str = "type Exclude<T, U> = T extends U ? never : T;\n\
     type Pick<T, K extends keyof T> = { [P in K]: T[P] };\n\
     type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>;\n\
     type Record<K extends keyof any, T> = { [P in K]: T };\n";

/// The project a scenario is checked in: `noLib`, the benchmark library,
/// the `.d.ts` files `declarations` (each `(file name, source)`), and the
/// scenario module.
struct Project<'a> {
    declarations: &'a [(&'a str, &'a str)],
    scenario: String,
}

impl<'a> Project<'a> {
    /// The store module, preceded by `prelude`, beside `declarations`.
    fn store(prelude: &str, declarations: &'a [(&'a str, &'a str)]) -> Self {
        Self {
            declarations,
            scenario: format!("{prelude}{STORE}"),
        }
    }

    /// A module of `source` beside the marker library.
    fn module(source: &str) -> Self {
        Self {
            declarations: MARKED,
            scenario: format!("{UTILITIES}{source}\nexport {{}};\n"),
        }
    }

    /// Each probe resolved by name under `options` — the alias `__P{i}`
    /// beside the scenario in one project — as its wire answer.
    fn answers(&self, options: &str, probes: &[&str]) -> Vec<TypeExpr> {
        let host = self.host(options, probes);
        (0..probes.len())
            .map(|index| wire_answer(&host, &named_answer(&host, index)))
            .collect()
    }

    /// The host checking the scenario under `options`, each probe the
    /// alias `__P{i}` beside it.
    fn host(&self, options: &str, probes: &[&str]) -> crate::VerterHost {
        let mut options: serde_json::Value =
            serde_json::from_str(options).expect("the setting's options are JSON");
        options["noLib"] = serde_json::Value::Bool(true);
        let files: Vec<&str> = std::iter::once("lib.bench.d.ts")
            .chain(self.declarations.iter().map(|(name, _)| *name))
            .chain(std::iter::once("scenario.ts"))
            .collect();
        let tsconfig =
            serde_json::json!({ "compilerOptions": options, "files": files }).to_string();
        let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
            crate::HostConfig::default(),
            &[("/pinia", tsconfig.as_str())],
        );
        let library = |name: &str| format!("/pinia/{name}");
        for (canonical, text) in std::iter::once((library("lib.bench.d.ts"), BENCHMARK_LIBRARY))
            .chain(
                self.declarations
                    .iter()
                    .map(|(name, text)| (library(name), *text)),
            )
        {
            let language = crate::LanguageRegistry::global()
                .classify_static(&canonical)
                .static_resolution();
            upsert(&host, &canonical, text.to_owned(), language);
        }
        let aliases: String = probes
            .iter()
            .enumerate()
            .map(|(index, probe)| format!("type __P{index} = {probe};\n"))
            .collect();
        upsert(
            &host,
            SCENARIO,
            format!("type __BenchInit = 0;\n{}\n{aliases}", self.scenario),
            crate::FileLanguage::script_ts(),
        );
        host
    }

    /// Every `(probe, checker print)` row whose named answer, under any of
    /// the four settings, does not print as the checker's (member
    /// separators and quote style aside, which the wire rendering does not
    /// carry) or holds a degraded leaf, an `any` the checker does not
    /// print, or an operator left unreduced.
    fn mismatches(&self, rows: &[(&str, &str)]) -> Vec<String> {
        let probes: Vec<&str> = rows.iter().map(|(probe, _)| *probe).collect();
        let mut failures = Vec::new();
        for setting in ALL {
            let answers = self.answers(setting.options, &probes);
            for ((probe, checker), answer) in rows.iter().zip(&answers) {
                failures.extend(
                    answer_mismatch(answer, checker)
                        .map(|failure| format!("{}: `{probe}`: {failure}", setting.label)),
                );
            }
        }
        failures
    }
}

/// Why `answer` is not the checker's `checker`, if it is not. The wire
/// rendering spells an array `Array<T>` where the checker prints `T[]`, the
/// one array type either way; member separators and quote style it does
/// not carry.
fn answer_mismatch(answer: &TypeExpr, checker: &str) -> Option<String> {
    let spelling = |text: &str| canonical_text(&text.replace(';', "").replace('\'', "\""));
    let rendered = checker.replace("{ id: number; }[]", "Array<{ id: number; }>");
    let printed = display(answer);
    if spelling(&printed) != spelling(&rendered) {
        return Some(format!(
            "the checker answers `{checker}`, the named demand `{printed}`"
        ));
    }
    unresolved_leaf(answer, !checker.contains("any"), &[])
        .map(|leaf| format!("holds `{}`: `{printed}`", display(leaf)))
}

/// The scenario module's path.
const SCENARIO: &str = "/pinia/scenario.ts";

/// The named demand of the probe alias `__P{index}`.
fn named_answer(
    host: &crate::VerterHost,
    index: usize,
) -> verter_type_engine::semantic_query_memo::node_roots::NodeRoot {
    let (outcome, _) = host
        .resolve_named_symbol_with_audit(SCENARIO, &format!("__P{index}"), None)
        .into_parts();
    outcome
        .expect("the named demand completes")
        .expect("the probe exists")
}

/// `node` projected to its wire answer.
fn wire_answer(
    host: &crate::VerterHost,
    node: &verter_type_engine::semantic_query_memo::node_roots::NodeRoot,
) -> TypeExpr {
    let wire = host
        .project_node_to_type_expr_json_bytes(node)
        .expect("the answer projects");
    let value: serde_json::Value = serde_json::from_slice(&wire).expect("the wire answer is JSON");
    verter_type_expr::type_expr_from_json(&value).expect("a wire TypeExpr")
}

/// Why the leased node is not the store's propagated refusal, if it is not: the
/// typed semantic miss, or an access or a tuple composed only over it or
/// deferred on the store type `St` (whose own answer is that miss).
fn lease_mismatch(
    lease: &verter_type_engine::semantic_query_memo::node_data_lease::NodeDataLease,
) -> Option<String> {
    let child = |id| {
        let child = lease
            .child(id)
            .and_then(|child| child.read())
            .expect("a retained child is live");
        lease_mismatch(&child)
    };
    match &**lease {
        SemanticNodeData::Opaque(QueryError::Miss) => None,
        SemanticNodeData::Opaque(other) => Some(format!("the refusal {other:?}")),
        SemanticNodeData::IndexedAccess { object, .. } => child(*object),
        SemanticNodeData::DeclRef { identity }
            if identity.canonical_id.as_ref() == SCENARIO
                && identity.decl_name.as_ref() == "St" =>
        {
            None
        }
        SemanticNodeData::Tuple { elements, .. } => {
            elements.iter().find_map(|element| child(element.value))
        }
        _ => Some("a value".to_owned()),
    }
}

/// The typed refusal `node` is, if it is one.
fn refusal(
    node: &verter_type_engine::semantic_query_memo::node_roots::NodeRoot,
) -> Option<QueryError> {
    let data = node
        .node_ref()
        .and_then(|node| node.read())
        .expect("the delivered answer is live");
    match &*data {
        SemanticNodeData::Opaque(error) => Some(error.clone()),
        _ => None,
    }
}

/// The typed degradation of the flow result of the store's initializer,
/// `defineStore("counter", { … })` — the shared result every member of
/// the store reads — or why it has no value.
fn store_degradation(host: &crate::VerterHost) -> Result<Option<FlowReturnDegradation>, String> {
    flow_degradation(
        host,
        "useCounterStore",
        verter_type_expr::facts::FunctionPartIdentity::Initializer,
    )
}

/// The typed degradation of the shared flow result of the scenario's
/// function `symbol` at `part`, or why it has no value.
fn flow_degradation(
    host: &crate::VerterHost,
    symbol: &str,
    part: verter_type_expr::facts::FunctionPartIdentity,
) -> Result<Option<FlowReturnDegradation>, String> {
    let initializer = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: verter_type_expr::OwnedStr::from(SCENARIO),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: verter_type_expr::OwnedStr::from(symbol),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: part,
        overload_ordinal: 0,
    };
    let (result, _) = host
        .get_flow_return_type_with_audit(
            &initializer,
            verter_type_engine::semantic_query::ReturnProjectionDemand::whole_return(),
        )
        .into_parts();
    result
        .map(|result| result.degradation())
        .map_err(|error| format!("{error:?}"))
}

fn upsert(
    host: &crate::VerterHost,
    canonical: &str,
    source: String,
    file_language: crate::FileLanguage,
) {
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(canonical.into()),
            input_id: canonical.into(),
            source: Arc::from(source),
            file_language,
            aliases: vec![],
        })
        .expect("the file is admitted");
}

fn display(expr: &TypeExpr) -> String {
    verter_type_expr::render_type_expr_display(expr)
        .expect("the answer renders")
        .text
}

/// The first leaf of `expr` no complete answer of this scenario holds: a
/// degraded leaf, an operator the checker reduces (a conditional, an
/// indexed access, a `keyof`, a mapped type, an `infer`, a `typeof`), a
/// type parameter no enclosing signature in `declared` declares, and, when
/// `forbid_any`, an `any`.
fn unresolved_leaf<'e>(
    expr: &'e TypeExpr,
    forbid_any: bool,
    declared: &[&str],
) -> Option<&'e TypeExpr> {
    let mut scope: Vec<&str> = declared.to_vec();
    let children: Vec<&TypeExpr> = match expr {
        TypeExpr::TypeParameter(param) if declared.contains(&param.name.as_str()) => Vec::new(),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() && declared.contains(&name.as_ref()) => Vec::new(),
        TypeExpr::Unknown(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::KeyOf(_)
        | TypeExpr::Mapped { .. }
        | TypeExpr::Infer { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_) => return Some(expr),
        TypeExpr::Primitive(PrimitiveName::Any) if forbid_any => return Some(expr),
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => members.iter().collect(),
        TypeExpr::Array { element, .. } => vec![element],
        TypeExpr::Tuple { elements, .. } => elements.iter().map(|element| &element.ty).collect(),
        TypeExpr::Object(object) => object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(&property.ty),
                _ => None,
            })
            .collect(),
        TypeExpr::Function(function) => {
            scope.extend(
                function
                    .type_parameters
                    .iter()
                    .map(|param| param.name.as_str()),
            );
            function
                .return_type
                .as_deref()
                .into_iter()
                .chain(function.parameters.iter().map(|parameter| &parameter.ty))
                .collect()
        }
        TypeExpr::Ref { type_arguments, .. } => type_arguments.iter().collect(),
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => vec![inner],
        _ => Vec::new(),
    };
    children
        .into_iter()
        .find_map(|child| unresolved_leaf(child, forbid_any, &scope))
}

/// The marker-typed store's members but `$state`, each as the checker
/// prints it.
const MEMBERS: [(&str, &str); 7] = [
    ("St['count']", "number"),
    ("St['items']", "{ id: number; }[]"),
    ("St['double']", "number"),
    ("St['first']", "{ id: number; }"),
    ("ReturnType<St['increment']>", "number"),
    ("ReturnType<St['add']>", "{ id: number; }"),
    ("St['$id']", "\"counter\""),
];

/// `$state` and the checker's `$state` are each assignable to the other:
/// the probe that answers `1` exactly when they are.
fn state_equivalence(state: &str) -> String {
    let checker = custom_state();
    format!("{state} extends {checker} ? {checker} extends {state} ? 1 : 0 : 0")
}

/// Every failure of a marker-typed store: each member prints as the
/// checker's, `$state` retains its generic-interface contributor and full
/// denotation, and the whole store answers element by element alike.
fn marked_store_mismatches(project: &Project<'_>) -> Vec<String> {
    let equivalence = state_equivalence("St['$state']");
    let whole_equivalence = state_equivalence(&format!("({WHOLE_STORE})[7]"));
    let probes: Vec<&str> = MEMBERS
        .iter()
        .map(|(probe, _)| *probe)
        .chain([
            "St['$state']",
            equivalence.as_str(),
            WHOLE_STORE,
            whole_equivalence.as_str(),
        ])
        .collect();
    let mut failures = Vec::new();
    for setting in ALL {
        let host = project.host(setting.options, &probes);
        let answers: Vec<TypeExpr> = (0..probes.len())
            .map(|index| wire_answer(&host, &named_answer(&host, index)))
            .collect();
        let store = store_degradation(&host);
        if store != Ok(None) {
            failures.push(format!(
                "{}: the store's initializer is {store:?}, not a complete value",
                setting.label
            ));
        }
        let mut report = |probe: &str, failure: Option<String>| {
            failures
                .extend(failure.map(|failure| format!("{}: `{probe}`: {failure}", setting.label)));
        };
        for ((probe, checker), answer) in MEMBERS.iter().zip(&answers) {
            report(probe, answer_mismatch(answer, checker));
        }
        let state = &answers[MEMBERS.len()];
        report("St['$state']", state_mismatch(state));
        report(
            &equivalence,
            answer_mismatch(&answers[MEMBERS.len() + 1], "1"),
        );
        let whole = &answers[MEMBERS.len() + 2];
        let TypeExpr::Tuple { elements, .. } = whole else {
            report(WHOLE_STORE, Some(format!("is `{}`", display(whole))));
            continue;
        };
        if elements.len() != MEMBERS.len() + 1 {
            report(WHOLE_STORE, Some(format!("is `{}`", display(whole))));
            continue;
        }
        for (element, (_, checker)) in elements.iter().zip(&MEMBERS) {
            report(WHOLE_STORE, answer_mismatch(&element.ty, checker));
        }
        report(WHOLE_STORE, state_mismatch(&elements[MEMBERS.len()].ty));
        report(
            &whole_equivalence,
            answer_mismatch(&answers[MEMBERS.len() + 3], "1"),
        );
    }
    failures
}

/// Check display independently of denotation: dropping the empty generic
/// interface preserves assignability but loses the checker's identity.
fn state_mismatch(state: &TypeExpr) -> Option<String> {
    answer_mismatch(state, &custom_state())
}

/// Measured on TypeScript 7.0.2 with the marker declared by the library,
/// all four settings alike and free of diagnostics: the whole store is
/// `[number, { id: number; }[], number, { id: number; }, number, { id:
/// number; }, "counter", S & PiniaCustomStateProperties<S>]` with `S` the
/// state `{ count: number; items: { id: number; }[]; name: string; }`, and
/// each member alone answers its element. `first`, `state => state.items[0]`,
/// reads the state its contextual `state` parameter takes, never `any`;
/// `add` and `increment` read the store through the actions' `this`.
#[test]
fn an_options_store_answers_every_member_as_the_checker_does() {
    let failures = marked_store_mismatches(&Project::store("", MARKED));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An authored `declare global` marker types the store as the library's
/// does. Measured on TypeScript 7.0.2, all four settings, free of
/// diagnostics.
#[test]
fn an_authored_global_marker_types_the_store() {
    let failures = marked_store_mismatches(&Project::store(
        "declare global { interface ThisType<T> {} }\n",
        &[],
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The global marker a module contributes beside its own module-scope
/// `ThisType` types the store of another module, which reads the global
/// declaration. Measured on TypeScript 7.0.2, all four settings, free of
/// diagnostics.
#[test]
fn a_global_marker_contributed_beside_a_namesake_types_another_modules_store() {
    let failures = marked_store_mismatches(&Project::store(
        "",
        &[("contrib.d.ts", CONTRIBUTING_MODULE)],
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every failure of a store no marker types to show the named demand's
/// permitted recovery refusal, under each of `settings`: an action reads
/// `this.count` off its own object literal, which declares no `count` —
/// the checker's TS2339 recovery, the typed
/// [`FlowReturnDegradation::AbsentReceiverMember`] the store initializer's
/// shared flow result names as its first cause — so the store's
/// `defineStore` call cannot infer its actions, and every member of the
/// whole store, each element of `WHOLE_STORE` beside the checker's print
/// of it in `checker`, is the typed semantic miss that refusal propagates:
/// the miss itself, or the member's access deferred on `St`, whose own
/// answer is that miss. Never a value. With the marker `this` declares
/// `count` and the same call completes, so this cause is also the proof
/// the marker typed nothing. Any other cause — the call's refusal alone,
/// an inapplicable or undecided call, a budget, a missing dependency, an
/// unmodelled position, a checker recovery — fails, whatever wire leaf
/// its members print.
///
/// A refused answer is never kept, so every demand recomputes the store:
/// the members are read as the whole store's elements, one demand per
/// setting, and `St` under the first setting alone.
fn recovery_mismatches(
    project: &Project<'_>,
    settings: &[super::differential_harness_tests::Setting],
    checker: &[&str; 8],
) -> Vec<String> {
    let members: Vec<&str> = MEMBERS
        .iter()
        .map(|(probe, _)| *probe)
        .chain(["St['$state']"])
        .collect();
    let mut failures = Vec::new();
    for (ordinal, setting) in settings.iter().enumerate() {
        let probes: &[&str] = if ordinal == 0 {
            &[WHOLE_STORE, "St"]
        } else {
            &[WHOLE_STORE]
        };
        let host = project.host(setting.options, probes);
        let store = store_degradation(&host);
        if store != Ok(Some(FlowReturnDegradation::AbsentReceiverMember)) {
            failures.push(format!(
                "{}: the store's initializer is {store:?}, not refused for an \
                 action's absent receiver member",
                setting.label
            ));
        }
        if ordinal == 0 {
            let store_type = refusal(&named_answer(&host, 1));
            if store_type != Some(QueryError::Miss) {
                failures.push(format!(
                    "{}: `St` is {store_type:?}, not the typed semantic miss",
                    setting.label
                ));
            }
        }
        let whole = named_answer(&host, 0);
        match element_mismatches(&whole) {
            Some(elements) if elements.len() == members.len() => {
                for ((member, checker), found) in members.iter().zip(checker).zip(elements) {
                    failures.extend(found.map(|found| {
                        format!(
                            "{}: `{member}` (the checker's `{checker}`) holds {found}, not the \
                             store's propagated miss",
                            setting.label
                        )
                    }));
                }
            }
            _ => failures.push(format!(
                "{}: the whole store is `{}`, not a tuple of its members",
                setting.label,
                display(&wire_answer(&host, &whole))
            )),
        }
    }
    failures
}

/// For a tuple answer, why each element is not the store's propagated
/// refusal, if it is not.
fn element_mismatches(
    node: &verter_type_engine::semantic_query_memo::node_roots::NodeRoot,
) -> Option<Vec<Option<String>>> {
    let lease = node
        .node_ref()
        .and_then(|node| node.read())
        .expect("the delivered answer is live");
    let SemanticNodeData::Tuple { elements, .. } = &*lease else {
        return None;
    };
    Some(
        elements
            .iter()
            .map(|element| {
                let child = lease
                    .child(element.value)
                    .and_then(|child| child.read())
                    .expect("a retained element is live");
                lease_mismatch(&child)
            })
            .collect(),
    )
}

/// The checker's store over a module-scope `ThisType` (TypeScript 7.0.2,
/// all four settings alike), member by member as [`WHOLE_STORE`] lists
/// them: every member as the marker-typed store's but `increment`, which
/// reads `this.count` off its own object literal and recovers as `any` —
/// TS2339 at each `this.count` and `this.items` read of the actions in
/// every setting, and TS7023 on `increment` under `noImplicitAny`. No
/// other diagnostic.
const NAMESAKE_STORE: [&str; 8] = [
    "number",
    "{ id: number; }[]",
    "number",
    "{ id: number; }",
    "any",
    "{ id: number; }",
    "\"counter\"",
    "{ count: number; items: { id: number; }[]; name: string; } & \
     PiniaCustomStateProperties<{ count: number; items: { id: number; }[]; name: string; }>",
];

/// A module-scope `ThisType` is an ordinary interface: the store's actions
/// read `this` as their own object literal, so `increment`, which returns
/// `this.count`, is not the marker-typed `number` — the checker answers
/// `any` ([`NAMESAKE_STORE`]). The named demand does not model that
/// error recovery: it refuses the store's call at its context-sensitive
/// inference and every member propagates the refusal, `double`, `first`,
/// `add`, the state members, `$id` and `$state` included, which the
/// checker answers independently. That propagated refusal is the recorded
/// recovery divergence the error-recovery successor owns, upgrading these
/// members to the checker's answers.
#[test]
fn a_module_scope_namesake_is_not_the_marker() {
    let failures = recovery_mismatches(
        &Project::store("interface ThisType<T> {}\n", &[]),
        &ALL,
        &NAMESAKE_STORE,
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A module that also contributes the global marker still reads its own
/// module-scope `ThisType` there: the namesake shadows the global
/// declaration in its own module, so the store it declares answers as with
/// a namesake alone — the checker's [`NAMESAKE_STORE`] with the same
/// diagnostics (TypeScript 7.0.2, all four settings), and the named
/// demand's same refusal.
#[test]
fn a_namesake_shadows_its_own_modules_global_contribution() {
    let failures = recovery_mismatches(
        &Project::store(
            "interface ThisType<T> {}\ndeclare global { interface ThisType<T> {} }\n",
            &[],
        ),
        &ALL,
        &NAMESAKE_STORE,
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// With no `ThisType` declared, the checker reports the name (TS2304 at
/// both `ThisType` references of the options declaration, and TS7006 on
/// each getter's `state` under `noImplicitAny`) and recovers with
/// `getters` and `actions` inferring nothing: the state members, `$id`
/// and `$state` answer as the marker-typed store's, and every getter and
/// action is absent from the store (TS2339 at each probe reading one), the
/// error type `any`, never the marker-typed member. Measured on
/// TypeScript 7.0.2, all four settings. The named demand refuses the
/// store's call, as with a namesake, for the same absent receiver member.
#[test]
fn an_undeclared_marker_recovers_without_inferring_through_it() {
    let failures = recovery_mismatches(
        &Project::store("", &[]),
        &ALL,
        &[
            "number",
            "{ id: number; }[]",
            "any",
            "any",
            "any",
            "any",
            "\"counter\"",
            NAMESAKE_STORE[7],
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The refusal evidence discriminates: a store built by a callee that is
/// not callable — an unrelated refusal, with the global marker declared —
/// has members holding the same propagated miss as a namesake store's,
/// and the evidence rejects it on the store's own typed cause alone.
#[test]
fn an_unrelated_store_refusal_is_not_the_recovery_refusal() {
    let project = Project {
        declarations: MARKED,
        scenario: format!(
            "declare const notAStore: {{ id: 1 }};\n{}",
            STORE.replacen("defineStore(\"counter\"", "notAStore(\"counter\"", 1)
        ),
    };
    let failures = recovery_mismatches(&project, &ALL[..1], &NAMESAKE_STORE);
    assert!(
        matches!(&failures[..], [store] if store.contains("not refused for an action's absent receiver member")),
        "the unrelated refusal's members hold the propagated miss and only the store's \
         typed cause tells it apart: {failures:?}"
    );
}

/// A marked store whose `first` getter reads a name nothing declares,
/// `(state) => missingDependency` — TS2304 alone in TypeScript 7.0.2, every
/// other member answering as the marker-typed store's — refuses its
/// `defineStore` call too, every member propagating the same miss. Its cause
/// is that context-sensitive call's own refusal, never an absent receiver
/// member, so the recovery evidence rejects it on the typed cause alone.
#[test]
fn a_marked_stores_missing_dependency_is_not_the_recovery_refusal() {
    let project = Project {
        declarations: MARKED,
        scenario: STORE.replacen(
            "first: (state) => state.items[0]",
            "first: (state) => missingDependency",
            1,
        ),
    };
    assert_ne!(project.scenario, STORE, "the getter is replaced");
    let failures = recovery_mismatches(&project, &ALL[..1], &NAMESAKE_STORE);
    assert!(
        matches!(&failures[..], [store] if store.contains("Some(ContextSensitiveInference)")),
        "the missing dependency's members hold the propagated miss and only the store's \
         typed cause tells it apart: {failures:?}"
    );
}

/// An object type relates to a `Record` over index keys as to the object
/// the mapped type resolves to: each index key is an index signature only
/// the members it applies to relate to (`{ count: number }` takes
/// `Record<symbol, any>`, `{ [k: number]: string }` and an `a${string}`
/// pattern, none of whose keys it declares), and an alias key domain
/// (`PropertyKey`) reads as the union it names — Pinia's `StateTree`, the
/// constraint of a store's `S`. Measured on TypeScript 7.0.2, all four
/// settings.
#[test]
fn a_record_over_index_keys_relates_through_its_index_signatures() {
    let failures = Project::module(
        "type StateTree = Record<PropertyKey, any>;\n\
         declare function tree<S extends StateTree>(s: S): S;\n\
         export const treed = tree({ count: 0 });\n",
    )
    .mismatches(&[
        ("{ count: number } extends Record<symbol, any> ? 1 : 0", "1"),
        (
            "{ count: number } extends { [k: number]: string } ? 1 : 0",
            "1",
        ),
        (
            "{ count: number } extends { [k: `a${string}`]: number } ? 1 : 0",
            "1",
        ),
        (
            "{ count: number } extends Record<string | number, any> ? 1 : 0",
            "1",
        ),
        (
            "{ count: number } extends Record<PropertyKey, any> ? 1 : 0",
            "1",
        ),
        (
            "{ count: number } extends Record<\"count\", string> ? 1 : 0",
            "0",
        ),
        (
            "{ count: number } extends Record<string, string> ? 1 : 0",
            "0",
        ),
        ("{ 1: number } extends { [k: number]: string } ? 1 : 0", "0"),
        ("{ abc: 1 } extends { [k: `a${string}`]: 2 } ? 1 : 0", "0"),
        ("typeof treed", "{ count: number; }"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A call infers through `Pick` and `Omit` as through the object their
/// closed keys resolve to, the template still naming the inferred
/// parameter: over an object literal type, over an interface applied to
/// the parameter (whose keys are its declared members, `S` open), and over
/// one extending a generic base (whose `Pick` reads the modifiers it
/// declares). Measured on TypeScript 7.0.2, all four settings.
#[test]
fn a_call_infers_through_pick_and_omit_of_its_parameter() {
    let failures = Project::module(
        "declare function g1<S>(o: Pick<{ id: 1; state: () => S }, \"state\">): S;\n\
         export const p1 = g1({ state: () => ({ count: 0 }) });\n\
         declare function g3<S>(o: Omit<{ id: 1; state?: () => S }, \"id\">): S;\n\
         export const p3 = g3({ state: () => ({ count: 0 }) });\n\
         interface O<S> { id: 1; state?: () => S }\n\
         declare function g4<S>(o: Omit<O<S>, \"id\">): S;\n\
         export const p4 = g4({ state: () => ({ count: 0 }) });\n\
         interface B<X> { base?: X }\n\
         interface OB<S> extends B<S> { id: 1; state?: () => S }\n\
         declare function g5<S>(o: Pick<OB<S>, \"state\">): S;\n\
         export const p5 = g5({ state: () => ({ count: 0 }) });\n\
         declare function g6<S>(o: Omit<OB<S>, \"id\">): S;\n\
         export const p6 = g6({ state: () => ({ count: 0 }) });\n\
         declare function k4<S>(x: S): { k: Exclude<keyof O<S>, \"id\"> };\n",
    )
    .mismatches(&[
        ("typeof p1", "{ count: number; }"),
        ("typeof p3", "{ count: number; }"),
        ("typeof p4", "{ count: number; }"),
        ("typeof p5", "{ count: number; }"),
        ("typeof p6", "{ count: number; }"),
        ("typeof k4", "<S>(x: S) => { k: \"state\"; }"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A callback typed by a union of signatures takes the arms its own arity
/// admits (`isAritySmaller`): `(s) => s.n` against `((s: { n: number }) =>
/// any) | (() => any)` is typed by the first arm, as a Pinia getter is by
/// `_GettersTree`, while a callback with a defaulted parameter requires
/// none and reads no contextual signature from the two differing arms.
/// Measured on TypeScript 7.0.2, all four settings.
#[test]
fn a_callback_takes_the_union_arm_its_arity_admits() {
    let failures = Project::module(
        "declare function f1<G extends Record<string, ((s: { n: number }) => any) | (() => any)>>(g: G): G;\n\
         export const c1 = f1({ a: (s) => s.n });\n\
         declare function f2<S, G>(o: { state: () => S; getters: G & Record<string, ((s: S) => any) | (() => any)> }): G;\n\
         export const c2 = f2({ state: () => ({ n: 1 }), getters: { a: (s) => s.n, b: (s) => s.n } });\n\
         export const c3 = f1({ a: (s = 1) => s });\n\
         export const c4 = f1({ a: () => 2 });\n",
    )
    .mismatches(&[
        ("typeof c1", "{ a: (s: { n: number; }) => number; }"),
        (
            "typeof c2",
            "{ a: (s: { n: number; }) => number; b: (s: { n: number; }) => number; }",
        ),
        ("typeof c3", "{ a: (s?: number) => number; }"),
        ("typeof c4", "{ a: () => 2; }"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A statement call through an object literal's `this`, which has no
/// explicit type (`getExplicitThisType`), neither asserts nor ends the
/// path: the method's return is complete, so `ReturnType` reads it off
/// the inferred object. Measured on TypeScript 7.0.2, all four settings.
#[test]
fn a_call_through_a_contextual_this_has_no_effects() {
    let failures = Project::module(
        "declare function mk<T>(o: T & ThisType<{ items: number[] }>): T;\n\
         export const mo = mk({ add(x: number) { this.items.push(x); return x; } });\n\
         type MO = typeof mo;\n",
    )
    .mismatches(&[("ReturnType<MO['add']>", "number")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A contextual signature's `this` parameter gives an object literal
/// method's `this` an explicit type, so a statement call through it reads
/// that type's effects signature: `this.check(x)` against `check(x:
/// unknown): asserts x is string` narrows `x`, and TypeScript 7.0.2
/// answers `string` for the method's return, free of diagnostics, all four
/// settings. The named demand does not read that assertion: it keeps the
/// typed guard-narrowing gap on the function's shared flow result, never
/// publishing the un-narrowed `unknown` as complete.
#[test]
fn a_call_through_a_contextual_signatures_explicit_this_keeps_its_effects() {
    let project = Project::module(
        "function f() {\n\
           return { m(x: unknown) { this.check(x); return x; } } satisfies \
             { m(this: { check(x: unknown): asserts x is string }, x: unknown): unknown };\n\
         }\n",
    );
    let probe = "ReturnType<ReturnType<typeof f>['m']>";
    for setting in ALL {
        let host = project.host(setting.options, &[probe]);
        let answer = wire_answer(&host, &named_answer(&host, 0));
        let degradation = flow_degradation(
            &host,
            "f",
            verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        );
        assert_eq!(
            degradation,
            Ok(Some(FlowReturnDegradation::FlowGap(
                verter_session_query::flow::policy::FlowGap::GuardNarrowing
            ))),
            "{}: `f` keeps the asserting call's typed gap; `{probe}` is `{}`",
            setting.label,
            display(&answer)
        );
        assert_ne!(
            answer,
            TypeExpr::Primitive(PrimitiveName::Unknown),
            "{}: `{probe}` is not the complete un-narrowed `unknown`",
            setting.label
        );
    }
}

/// A closed access nested in an answer reads what the member resolves to
/// when the member applies an alias the checker prints as what it resolves
/// to: `[Box<{ a: 1 }>['v']]` over `v: Unwrap<T>`, a conditional alias, is
/// `[{ a: 1; }]` as `Box<{ a: 1 }>['v']` alone is, never the application
/// kept as a name. An interface the member applies keeps its name. Measured
/// on TypeScript 7.0.2, all four settings.
#[test]
fn a_nested_access_reads_through_a_transparent_alias_member() {
    let failures = Project::module(
        "type Unwrap<T> = T extends { inner: infer I } ? I : T;\n\
         interface Named<T> { named: T }\n\
         interface Box<T> { v: Unwrap<T>; n: Named<T> }\n",
    )
    .mismatches(&[
        ("[Box<{ a: 1 }>['v']]", "[{ a: 1; }]"),
        ("{ x: Box<{ inner: 2 }>['v'] }", "{ x: 2; }"),
        ("Box<{ a: 1 }>['v']", "{ a: 1; }"),
        ("[Box<{ a: 1 }>['n']]", "[Named<{ a: 1; }>]"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A generic-interface application contributing to an intersection keeps
/// its name through the access that reads the intersection: `C2['m']` over
/// `m: { a: 1 } & E<{ a: 1 }>` answers `{ a: 1 } & E<{ a: 1 }>` as the
/// bare alias does — never the application's structural surface alone,
/// which assignability could not tell apart — and a member demand still
/// reads through the intersection. Measured on TypeScript 7.0.2, all four
/// settings.
#[test]
fn a_nested_access_keeps_an_intersections_generic_interface_contributor() {
    let failures = Project::module(
        "interface E<T> {}\n\
         type A = { a: 1 } & E<{ a: 1 }>;\n\
         interface C2 { m: { a: 1 } & E<{ a: 1 }> }\n\
         type B2 = C2['m'];\n",
    )
    .mismatches(&[
        ("A", "{ a: 1; } & E<{ a: 1; }>"),
        ("B2", "{ a: 1; } & E<{ a: 1; }>"),
        ("B2['a']", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
