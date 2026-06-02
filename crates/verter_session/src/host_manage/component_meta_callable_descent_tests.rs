//! Discriminating tests for the Pick member-route callable-descent
//! predicates (`type_expr_contains_callable_surface_impl` and
//! `pick_member_route_should_skip_callable_descent_impl`).
//!
//! These pin that a bare `TypeExpr::ConstructorType` (`new (...) => R`),
//! which reaches the predicates un-collapsed on RAW prepared-decl bodies,
//! is treated EXACTLY like a `TypeExpr::Function` — both carry the same
//! `FunctionExpr` payload.
//!
//! Pre-fix the predicates matched only `TypeExpr::Function` and absorbed a
//! `ConstructorType` through the `_ => false` wildcard, so a raw
//! constructor-valued member silently (a) was not recognised as a callable
//! surface, and (b) never had its package-backed parameter roots checked —
//! defeating the suppression that keeps package-backed callable params
//! symbolic.
//!
//! Each test below FAILS against the pre-fix tree and PASSES against the
//! post-fix tree (Function/ConstructorType parity).

use std::sync::Arc;

use verter_type_expr::{FunctionExpr, FunctionParam, PrimitiveName, TypeExpr};

use super::{
    pick_member_route_should_skip_callable_descent_impl, type_expr_contains_callable_surface_impl,
};
use crate::meta::MetaProject;
use crate::resolver_core::with_bare_host_ctx_for_test;
use crate::types::HostConfig;
use crate::VerterHost;
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

fn build_hermetic_project(files: &[(&str, &str)]) -> Arc<MetaProject> {
    let scheduler_config = verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    };
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws_access,
        scheduler_config,
    );
    MetaProject::new(host)
}

/// `(arg: ArgRef) => void` — a FUNCTION-typed callable taking a single
/// reference-typed parameter.
fn function_taking(arg: TypeExpr) -> TypeExpr {
    TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("arg".to_string()),
            arg,
            false,
            false,
        )],
        Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Void))),
        Vec::new(),
    )))
}

/// `new (arg: ArgRef) => void` — a CONSTRUCTOR-typed callable taking a
/// single reference-typed parameter. Carries the SAME `FunctionExpr`
/// payload as [`function_taking`]; only the variant tag differs.
fn constructor_taking(arg: TypeExpr) -> TypeExpr {
    TypeExpr::ConstructorType(Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("arg".to_string()),
            arg,
            false,
            false,
        )],
        Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Void))),
        Vec::new(),
    )))
}

fn ref_named(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::new().as_slice()),
    }
}

/// `(<callable>)[]` — wrap a callable in an array, the exact shape a
/// picked `actions?: (...)[]` member's raw leaf takes.
fn array_of(element: TypeExpr) -> TypeExpr {
    TypeExpr::Array {
        element: Arc::new(element),
        readonly: false,
    }
}

// ───────────────────────────────────────────────────────────────────────
// type_expr_contains_callable_surface_impl — ConstructorType is callable
// ───────────────────────────────────────────────────────────────────────

/// A bare constructor type is a callable surface, identically to a
/// function type. Discriminator: pre-fix `type_expr_contains_callable_surface`
/// matched only `TypeExpr::Function` and fell through `_ => false` for a
/// `ConstructorType`, returning `false`.
#[test]
fn constructor_type_is_detected_as_callable_surface() {
    let ctor = constructor_taking(ref_named("UIMessage"));
    assert!(
        type_expr_contains_callable_surface_impl(&ctor),
        "a bare constructor type must be recognised as a callable surface"
    );
    // Parity: the function-typed equivalent is recognised identically.
    let func = function_taking(ref_named("UIMessage"));
    assert_eq!(
        type_expr_contains_callable_surface_impl(&ctor),
        type_expr_contains_callable_surface_impl(&func),
        "ConstructorType and Function callables must be classified identically"
    );
}

/// The constructor surface is detected through the same nested structure
/// (Array element) the Pick member-route leaf actually uses.
#[test]
fn constructor_type_callable_detected_through_array_element() {
    let ctor_array = array_of(constructor_taking(ref_named("UIMessage")));
    assert!(
        type_expr_contains_callable_surface_impl(&ctor_array),
        "a constructor type nested in an array element must be detected as callable"
    );
    let func_array = array_of(function_taking(ref_named("UIMessage")));
    assert_eq!(
        type_expr_contains_callable_surface_impl(&ctor_array),
        type_expr_contains_callable_surface_impl(&func_array),
        "array-nested ConstructorType / Function callables must be classified identically"
    );
}

// ───────────────────────────────────────────────────────────────────────
// pick_member_route_should_skip_callable_descent_impl — package-backed
// constructor param surfaced identically to a function param
// ───────────────────────────────────────────────────────────────────────

const AI_INDEX_DTS: &str = r#"export interface UIMessage {
  role: string;
  content: string;
}
"#;

const LOCAL_TS: &str = r#"export interface LocalShape {
  payload: string;
}
"#;

// A `.ts` scope that imports BOTH a package-backed type (`UIMessage` from
// `ai`, which resides under `node_modules`) and a workspace-local type
// (`LocalShape`). The predicate resolves the raw leaf's parameter `Ref`s
// in this scope.
const SCOPE_TS: &str = r#"import type { UIMessage } from 'ai'
import type { LocalShape } from './local'

export interface Carrier {
  fn?: (m: UIMessage) => void
}
"#;

const SCOPE_VUE: &str = r#"<script setup lang="ts">
import type { Carrier } from './carrier'
defineProps<{ user?: Carrier }>();
</script>
<template><div /></template>
"#;

fn callable_descent_project() -> Arc<MetaProject> {
    let project = build_hermetic_project(&[
        (
            "/workspace/node_modules/ai/package.json",
            r#"{ "name": "ai", "types": "./index.d.ts" }"#,
        ),
        ("/workspace/node_modules/ai/index.d.ts", AI_INDEX_DTS),
        ("/workspace/src/local.ts", LOCAL_TS),
        ("/workspace/src/carrier.ts", SCOPE_TS),
        ("/workspace/src/Carrier.vue", SCOPE_VUE),
    ]);
    // Establish host state so dependency resolution + indexing is warm.
    let session = project.open_session_batch().expect("session");
    let _ = session.get_component_meta("/workspace/src/Carrier.vue");
    project
}

/// A raw leaf `(new (m: UIMessage) => void)[]` whose constructor parameter
/// root (`UIMessage`) is PACKAGE-BACKED must trip the suppression predicate
/// — exactly as the function-typed equivalent `(m: UIMessage) => void`
/// does. Discriminator: pre-fix `any_callable_param_is_package_backed`
/// matched only `TypeExpr::Function`, so the constructor's package-backed
/// parameter root was never checked and the predicate returned `false`,
/// letting the package-backed internals be descended into.
#[test]
fn constructor_param_package_backed_trips_suppression_like_function() {
    let project = callable_descent_project();
    let scope = "/workspace/src/carrier.ts";

    let ctor_leaf = array_of(constructor_taking(ref_named("UIMessage")));
    let func_leaf = array_of(function_taking(ref_named("UIMessage")));

    with_bare_host_ctx_for_test(project.host(), |ctx| {
        let ctor_skip = pick_member_route_should_skip_callable_descent_impl(&ctor_leaf, ctx, scope);
        let func_skip = pick_member_route_should_skip_callable_descent_impl(&func_leaf, ctx, scope);
        assert!(
            func_skip,
            "sanity: the function-typed package-backed leaf must trip suppression"
        );
        assert!(
            ctor_skip,
            "a constructor-typed callable with a package-backed parameter root \
             must trip the suppression predicate identically to the function-typed \
             equivalent (pre-fix the ConstructorType arm was missing and this was false)"
        );
        assert_eq!(
            ctor_skip, func_skip,
            "ConstructorType / Function package-backed suppression must be identical"
        );
    });
}

/// Counterpart: a raw leaf `(new (l: LocalShape) => void)[]` whose
/// constructor parameter root is WORKSPACE-LOCAL must NOT trip the
/// suppression predicate (parity with the function-typed equivalent).
/// This guards against the ConstructorType arm over-firing: it must mirror
/// `Function` for non-package-backed params too (suppression stays off).
#[test]
fn constructor_param_workspace_local_does_not_trip_suppression_like_function() {
    let project = callable_descent_project();
    let scope = "/workspace/src/carrier.ts";

    let ctor_leaf = array_of(constructor_taking(ref_named("LocalShape")));
    let func_leaf = array_of(function_taking(ref_named("LocalShape")));

    with_bare_host_ctx_for_test(project.host(), |ctx| {
        let ctor_skip = pick_member_route_should_skip_callable_descent_impl(&ctor_leaf, ctx, scope);
        let func_skip = pick_member_route_should_skip_callable_descent_impl(&func_leaf, ctx, scope);
        assert!(
            !func_skip,
            "sanity: the function-typed workspace-local leaf must NOT trip suppression"
        );
        assert!(
            !ctor_skip,
            "a constructor-typed callable with a workspace-local parameter root must \
             NOT trip suppression (parity with the function-typed equivalent)"
        );
        assert_eq!(
            ctor_skip, func_skip,
            "ConstructorType / Function workspace-local suppression must be identical"
        );
    });
}
