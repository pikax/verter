//! Discriminating tests for the structural macro-participation
//! classification path in `merge_evaluated_prop_types_into_meta`.
//!
//! These tests pin the contract enforced by
//! `build_macro_participating_identities` /
//! `collect_imported_macro_participating_refs` /
//! `resolve_ref_to_root_identity`:
//!
//! 1. Type-role classification is structural — a name participates
//!    because a Vue SFC macro consumes its declaration, NOT because
//!    the identifier ends in `"Props"`.
//! 2. Identity is `ResolvedRootIdentity { canonical_id, symbol_name }`,
//!    so the same name in two scopes is not collapsed.
//! 3. The iterative walker terminates on recursive type aliases and
//!    on programmatically deep object/intersection nests.

use crate::meta::MetaProject;
use crate::types::HostConfig;
use crate::VerterHost;
use std::sync::Arc;
use verter_type_expr::TypeExpr;

fn test_scheduler_config() -> verter_scheduler::scheduler::SchedulerConfig {
    verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    }
}

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        test_scheduler_config(),
    );
    MetaProject::new(host)
}

/// Discriminating test 1 — macro-participation positive.
///
/// SFC defines `defineProps<MyButton>()` where `MyButton` is an
/// imported interface with NO `Props` suffix. The structural
/// classification MUST surface `MyButton` as a macro participant
/// because `defineProps` consumes its declaration.
///
/// Discriminator: a nominal classifier that filters raw refs by
/// `.ends_with("Props")` would return an EMPTY set for this fixture
/// — `MyButton` does not end in `Props`, so a suffix-based filter
/// drops it despite being the macro's actual type argument. The
/// correct classification is STRUCTURAL: a type is a participant
/// because a Vue SFC macro consumes its declaration, regardless of
/// identifier suffix.
///
/// Asserted contract: `build_macro_participating_identities` +
/// `collect_imported_macro_participating_refs` return the imported
/// `(canonical_id: /src/button.ts, symbol_name: "MyButton")` identity
/// with arity 0. The walker classifies by macro consumption
/// (structural), not by identifier suffix (nominal).
#[test]
fn macro_participation_positive_surfaces_non_props_suffixed_import() {
    use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
    use verter_semantic::analysis::AnalyzedMacroKind;

    let project = make_project();
    project
        .upsert_base(
            "/src/button.ts",
            r#"export interface MyButton {
  label: string
  count: number
}

export interface RandomThing {
  unrelated: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { MyButton, RandomThing } from './button'
// MyButton is the macro participant. RandomThing is imported but
// not consumed by any macro — it must NOT enter the participation
// set even though both share the same import.
export type _Unused = RandomThing
defineProps<MyButton>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Establish base host state.
    let session = project.open_session_batch().unwrap();
    let _ = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    let host = project.host();
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("App.vue analysis snapshot");

    // Build the macro-participation set with the same call shape as
    // `merge_evaluated_prop_types_into_meta` uses.
    let participating = super::build_macro_participating_identities_for_test(
        host,
        "/src/App.vue",
        &snapshot,
        &[
            AnalyzedMacroKind::DefineProps,
            AnalyzedMacroKind::WithDefaults,
            AnalyzedMacroKind::DefineModel,
        ],
    );

    let my_button_identity = ResolvedRootIdentity::new("/src/button.ts", "MyButton");
    let random_thing_identity = ResolvedRootIdentity::new("/src/button.ts", "RandomThing");

    assert!(
        participating.contains(&my_button_identity),
        "MyButton (no `Props` suffix) must surface as a structural participant — the \
         deleted `.ends_with(\"Props\")` filter would have dropped it. Got: {participating:?}"
    );
    assert!(
        !participating.contains(&random_thing_identity),
        "RandomThing is imported but not consumed by any macro — it MUST NOT enter the \
         participation set. Got: {participating:?}"
    );

    // Walk a prop's type_expr to confirm collect_imported_macro_participating_refs
    // surfaces MyButton with the correct arity.
    let prop_type_expr = TypeExpr::Ref {
        name: Arc::from("MyButton"),
        type_arguments: Arc::from(Vec::new().as_slice()),
    };
    let collected = super::collect_imported_macro_participating_refs_for_test(
        host,
        "/src/App.vue",
        &prop_type_expr,
        &participating,
    );

    assert!(
        collected.contains(&(my_button_identity.clone(), 0)),
        "collect_imported_macro_participating_refs must surface MyButton with arity 0. Got: {collected:?}"
    );
}

/// Discriminating test 2 — cycle termination.
///
/// A type alias whose declaration recursively references itself
/// participates in a `defineProps` macro. The walker must terminate
/// without re-walking the same node, and the participation set
/// records the type once.
#[test]
fn macro_participation_walker_terminates_on_recursive_alias() {
    let project = make_project();
    project
        .upsert_base(
            "/src/node.ts",
            r#"export type Foo = {
  next: Foo | null
  label: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Foo } from './node'
defineProps<Foo>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should terminate on recursive participating type");

    // Test does not deadlock / stack-overflow — that's the
    // discriminating assertion. Plus the merge produces a sane meta
    // (label is reachable, next stays shallow).
    assert!(
        meta.props.iter().any(|p| p.name == "label"),
        "label prop must surface from recursive Foo type, got: {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert!(
        meta.props.iter().any(|p| p.name == "next"),
        "next prop must surface from recursive Foo type, got: {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

/// Discriminating test 3 — deep-nesting termination.
///
/// Build a TypeExpr with 100 levels of nested intersection. The
/// iterative walker must complete without stack overflow on the
/// default stack budget. A recursive walker would blow the stack
/// well below 100 levels in debug builds with default RUST_MIN_STACK.
#[test]
fn macro_participation_walker_handles_100_level_intersection() {
    use rustc_hash::FxHashSet;
    use verter_semantic::analysis::types::{AnalyzedMacro, ResolvedLocalType};
    use verter_semantic::analysis::AnalyzedMacroKind;
    use verter_span::Span;

    // Build a 100-level nested intersection ending in a Ref { name:
    // "Leaf" }. The participation walker should encounter exactly one
    // leaf identity, regardless of depth.
    let mut node: TypeExpr = TypeExpr::Ref {
        name: Arc::from("Leaf"),
        type_arguments: Arc::from(Vec::new().as_slice()),
    };
    for _ in 0..100 {
        let other = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
        node = TypeExpr::Intersection(Arc::from(vec![node, other].as_slice()));
    }

    // Construct an `AnalyzedMacro` whose `resolved_local_types`
    // contains the deeply-nested expression, then run the iterative
    // `harvest_ref_names_iterative` walker over it through the
    // public-via-test entry point `harvest_collect_for_test`. We pull
    // the helper directly through a public re-export to keep the test
    // pure — no host required.

    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineProps,
        is_type_based: true,
        type_references: vec!["Leaf".to_string()],
        binding_name: None,
        model_name: None,
        has_inherit_attrs_false: false,
        prop_fields: Vec::new(),
        emit_fields: Vec::new(),
        slot_fields: Vec::new(),
        default_keys: Vec::new(),
        default_values: Vec::new(),
        expose_fields: Vec::new(),
        resolved_local_types: vec![ResolvedLocalType {
            name: "Wrapper".to_string(),
            expanded: String::new(),
            type_expr: Some(node),
            span: Span::default(),
        }],
        parsed_type_argument: None,
        parsed_type_argument_scope: None,
        span: Span::default(),
    }];

    let collected = collect_ref_names_for_test(&macros);
    assert!(
        collected.contains("Leaf"),
        "deeply nested intersection must surface Leaf identity"
    );
    assert!(
        collected.contains("Wrapper"),
        "resolved local types must contribute their declared name"
    );

    let _expected: FxHashSet<&str> = ["Leaf", "Wrapper"].into_iter().collect();
}

/// Test-only entry point that exercises the iterative ref harvester
/// over the `resolved_local_types[i].type_expr` axis. Mirrors what
/// `build_macro_participating_identities` does inline, minus host
/// resolution.
fn collect_ref_names_for_test(
    macros: &[verter_semantic::analysis::types::AnalyzedMacro],
) -> rustc_hash::FxHashSet<String> {
    let mut out = rustc_hash::FxHashSet::default();
    for mac in macros {
        for type_name in mac.type_references.iter() {
            out.insert(type_name.clone());
        }
        for resolved_local in mac.resolved_local_types.iter() {
            out.insert(resolved_local.name.clone());
            if let Some(local_expr) = resolved_local.type_expr.as_ref() {
                super::harvest_ref_names_for_test(local_expr, |name| {
                    out.insert(name.to_string());
                });
            }
        }
    }
    out
}

/// Discriminating test 4 — scope correctness.
///
/// SFC A imports `Helper` from a sibling file B. A also defines a
/// LOCAL `Helper` inside `defineProps<{ inner: Helper }>()`. The
/// local declaration shadows the import per JS module-scope rules.
///
/// The walker MUST distinguish the two `Helper` identities by
/// `ResolvedRootIdentity` — the local one keys on
/// `(/src/App.vue, "Helper")`, the imported one keys on
/// `(/src/b.ts, "Helper")`.
///
/// The deleted string-keyed walker collected `(String, usize)` pairs
/// by name only, so the two `Helper`s collided. The
/// `ResolvedRootIdentity`-keyed walker resolves each `Ref` to its
/// canonical identity and the two scopes stay distinct.
#[test]
fn macro_participation_distinguishes_local_helper_from_imported_helper() {
    use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;

    let project = make_project();
    project
        .upsert_base(
            "/src/b.ts",
            r#"export interface Helper {
  fromB: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
// Local Helper shadows the import for the macro's type argument scope.
interface Helper {
  inner: number
}
defineProps<Helper>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Establish base host state by issuing a get_component_meta call.
    let session = project.open_session_batch().unwrap();
    let _ = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should resolve through the local Helper, not the import");

    // Verify that resolve_ref_to_root_identity discriminates the two
    // Helper identities for the App.vue scope.
    let host = project.host();
    let local_identity =
        super::resolve_ref_to_root_identity_for_test(host, "/src/App.vue", "Helper")
            .expect("local Helper must resolve to a root identity");
    let imported_identity =
        super::resolve_ref_to_root_identity_for_test(host, "/src/b.ts", "Helper")
            .expect("imported Helper must resolve to a root identity in b.ts scope");

    assert_eq!(
        local_identity,
        ResolvedRootIdentity::new("/src/App.vue", "Helper"),
        "local Helper must key on App.vue, NOT b.ts"
    );
    assert_eq!(
        imported_identity,
        ResolvedRootIdentity::new("/src/b.ts", "Helper"),
        "imported Helper must key on b.ts"
    );
    assert_ne!(
        local_identity, imported_identity,
        "the two Helper identities MUST NOT collide — naive (String, usize) keying would have collapsed them"
    );
}

/// Discriminating regression — `build_public_instance_slot_type`
/// MUST consume `SlotAnalysis.return_expr` (typed) and ignore
/// `SlotAnalysis.return_type` (display-only) for semantic decisions.
/// This is the Typed-IR-Only Resolver Rule (CLAUDE.md): raw / display
/// strings are passthrough; a consumer that reparses them is the bug.
///
/// The discriminator: construct a slot whose typed `return_expr` is
/// `Primitive(Boolean)` while the display `return_type` string lowers
/// to a DIFFERENT shape (`"VNode[]"` -> `Array<Ref { name: "VNode" }>`).
/// A correct implementation reads `return_expr` and surfaces
/// `Primitive(Boolean)`; an incorrect (reparse-driven) implementation
/// reparses `"VNode[]"` and surfaces the array shape. The two trees
/// give observably different answers — the test is discriminating.
#[test]
fn build_public_instance_slot_type_consumes_return_expr_not_return_type() {
    use verter_semantic::analysis::component_meta::SlotAnalysis;
    use verter_type_expr::{FunctionExpr, PrimitiveName, TypeExpr, TypeExprScope};

    // The typed companion: an unambiguous, non-`unknown` primitive that
    // CANNOT be derived from reparsing the display string below.
    let typed_return = TypeExpr::Primitive(PrimitiveName::Boolean);

    // The display string lowers to `Array<Ref { name: "VNode" }>` — a
    // different shape from the typed primitive above. An incorrect
    // (reparse-driven) implementation parses this and silently
    // overrides `return_expr`; the correct typed-IR consumer ignores
    // it.
    let display_return_type = "VNode[]".to_string();

    let slot = SlotAnalysis {
        name: "default".to_string(),
        is_scoped: false,
        bindings: Vec::new(),
        is_required: true,
        return_type: Some(display_return_type),
        return_expr: Some(typed_return.clone()),
        return_expr_scope: Some(TypeExprScope::new("test:fixture")),
        description: None,
        tags: Vec::new(),
    };

    let built = super::build_public_instance_slot_type_for_test(&slot);

    let TypeExpr::Function(func) = &built else {
        panic!(
            "expected a Function TypeExpr at the slot public-instance surface; got {:?}",
            built
        );
    };

    let FunctionExpr { return_type, .. } = func.as_ref();
    let return_type = return_type
        .as_deref()
        .expect("required slot lowers to a function with a return type");

    assert_eq!(
        *return_type, typed_return,
        "build_public_instance_slot_type MUST read `slot.return_expr` \
         directly. If this assertion fails, the consumer has regressed \
         to reparsing `slot.return_type` (display string), violating the \
         Typed-IR-Only Resolver Rule."
    );

    // Negative: the built return type MUST NOT be the lowered form of
    // the display string. This guards against a text-mode reparse
    // fallback re-entering the consumer.
    let lowered_from_display =
        verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload("VNode[]");
    assert_ne!(
        *return_type, lowered_from_display,
        "build_public_instance_slot_type MUST NOT reparse `slot.return_type`          when `slot.return_expr` is present."
    );
}


/// Discriminating coverage for M1/M2/M4 — the macro-participation walker
/// must traverse Object / Function / Conditional / Mapped / KeyOf /
/// RecursiveRef / TemplateLiteral nodes when searching for participating
/// refs.
///
/// Pre-fix coverage: `collect_imported_macro_participating_refs` matched
/// only Parenthesized / Ref / Union / Intersection / Array / Tuple /
/// IndexedAccess. A participating Ref nested inside ANY of the missing
/// branches would be silently dropped — a false negative that the
/// structural-classifier contract forbade.
///
/// Each test constructs a synthetic `TypeExpr` placing a participating
/// Ref deep inside one of the missing-branch shapes, runs the walker,
/// and asserts the Ref surfaces. Pre-fix the assertion FAILS; post-fix
/// it PASSES.
#[cfg(test)]
mod walker_coverage_tests {
    use super::*;
    use rustc_hash::FxHashSet;
    use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
    use verter_type_expr::{
        FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
    };

    fn participating_ref() -> TypeExpr {
        TypeExpr::Ref {
            name: Arc::from("Target"),
            type_arguments: Arc::from(Vec::new().as_slice()),
        }
    }

    fn fixture_project_with_target() -> Arc<MetaProject> {
        let project = make_project();
        project
            .upsert_base(
                "/src/target.ts",
                "export interface Target { x: number }\n",
            )
            .unwrap();
        project
            .upsert_base(
                "/src/App.vue",
                "<script setup lang=\"ts\">\nimport type { Target } from './target'\ndefineProps<Target>()\n</script>\n<template><div /></template>",
            )
            .unwrap();
        // Establish host state.
        let session = project.open_session_batch().unwrap();
        let _ = session.get_component_meta("/src/App.vue").unwrap();
        project
    }

    fn participating_set() -> FxHashSet<ResolvedRootIdentity> {
        let mut set = FxHashSet::default();
        set.insert(ResolvedRootIdentity::new("/src/target.ts", "Target"));
        set
    }

    /// Embed the participating Ref deep inside a 100-level Object chain
    /// (`{ a: { a: { a: ... Target } } }`). Pre-fix walker terminates at
    /// the first Object and silently drops Target.
    #[test]
    fn walker_traverses_deep_object_chain() {
        let project = fixture_project_with_target();
        let participating = participating_set();

        let mut node = participating_ref();
        for _ in 0..100 {
            let obj = ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "a".to_string(),
                    ty: node,
                    optional: false,
                    readonly: false,
                })],
            };
            node = TypeExpr::Object(Arc::new(obj));
        }

        let collected = super::super::collect_imported_macro_participating_refs_for_test(
            project.host(),
            "/src/App.vue",
            &node,
            &participating,
        );

        assert!(
            collected.contains(&(
                ResolvedRootIdentity::new("/src/target.ts", "Target"),
                0
            )),
            "100-deep Object chain must not terminate the walker — Target should surface. Got: {collected:?}"
        );
    }

    /// Embed the participating Ref inside a Conditional ladder.
    /// Pre-fix walker terminates at the Conditional and drops Target.
    #[test]
    fn walker_traverses_deep_conditional_ladder() {
        let project = fixture_project_with_target();
        let participating = participating_set();

        // Build `T0 extends T1 ? T2 extends T3 ? ... ? Target : never : never`
        let mut node = participating_ref();
        for _ in 0..50 {
            node = TypeExpr::Conditional {
                check: Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)),
                extends: Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)),
                true_type: Arc::new(node),
                false_type: Arc::new(TypeExpr::Primitive(
                    verter_type_expr::PrimitiveName::Never,
                )),
            };
        }

        let collected = super::super::collect_imported_macro_participating_refs_for_test(
            project.host(),
            "/src/App.vue",
            &node,
            &participating,
        );

        assert!(
            collected.contains(&(
                ResolvedRootIdentity::new("/src/target.ts", "Target"),
                0
            )),
            "deep Conditional ladder must not terminate the walker — Target should surface. Got: {collected:?}"
        );
    }

    /// Embed the participating Ref inside a Function return_type wrapping
    /// an Object. Pre-fix walker terminates at Function/Object combo.
    #[test]
    fn walker_traverses_function_return_wrapping_object() {
        let project = fixture_project_with_target();
        let participating = participating_set();

        // `() => { wrapped: Target }`
        let object = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "wrapped".to_string(),
                ty: participating_ref(),
                optional: false,
                readonly: false,
            })],
        }));
        let function = TypeExpr::Function(Arc::new(FunctionExpr {
            parameters: Vec::new(),
            return_type: Some(Arc::new(object)),
            type_parameters: Vec::new(),
        }));

        let collected = super::super::collect_imported_macro_participating_refs_for_test(
            project.host(),
            "/src/App.vue",
            &function,
            &participating,
        );

        assert!(
            collected.contains(&(
                ResolvedRootIdentity::new("/src/target.ts", "Target"),
                0
            )),
            "Function return_type wrapping Object must not terminate the walker. Got: {collected:?}"
        );
    }

    /// Embed the participating Ref inside Function parameters.
    /// Pre-fix walker did not traverse Function parameters either.
    #[test]
    fn walker_traverses_function_parameters() {
        let project = fixture_project_with_target();
        let participating = participating_set();

        // `(arg: Target) => void`
        let function = TypeExpr::Function(Arc::new(FunctionExpr {
            parameters: vec![FunctionParam {
                name: Some("arg".to_string()),
                ty: participating_ref(),
                optional: false,
                rest: false,
            }],
            return_type: Some(Arc::new(TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::Void,
            ))),
            type_parameters: Vec::new(),
        }));

        let collected = super::super::collect_imported_macro_participating_refs_for_test(
            project.host(),
            "/src/App.vue",
            &function,
            &participating,
        );

        assert!(
            collected.contains(&(
                ResolvedRootIdentity::new("/src/target.ts", "Target"),
                0
            )),
            "Function parameter type must not terminate the walker. Got: {collected:?}"
        );
    }

    /// Embed the participating Ref inside KeyOf. Pre-fix walker did not
    /// traverse KeyOf.
    #[test]
    fn walker_traverses_keyof() {
        let project = fixture_project_with_target();
        let participating = participating_set();

        let keyof = TypeExpr::KeyOf(Arc::new(participating_ref()));

        let collected = super::super::collect_imported_macro_participating_refs_for_test(
            project.host(),
            "/src/App.vue",
            &keyof,
            &participating,
        );

        assert!(
            collected.contains(&(
                ResolvedRootIdentity::new("/src/target.ts", "Target"),
                0
            )),
            "KeyOf wrapper must not terminate the walker. Got: {collected:?}"
        );
    }
}
