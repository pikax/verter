//! Regression coverage for the `defineExpose` binding-admission invariant,
//! reproduced end-to-end against real `.vue` fixtures through the public
//! [`verter_session`] API (no internal `crate::` access):
//!
//! - **`defineExpose` binding admission**: every demanded, lexically visible
//!   local value binding — a plain value, a `ref`/`computed` call-initializer
//!   (typed or untyped), a function declaration, an enum, a class, or a
//!   binding whose type comes from an imported position — must reach a
//!   genuine typed source through `component_meta_binding_type_entries`
//!   (`verter_session/src/host_manage/eval_env.rs`) and
//!   `resolve_exposed_type` (`verter_semantic/src/analysis/component_meta.rs`),
//!   never a silent `Unknown`/`Absent` substitution. A binding that reaches
//!   admission but whose preparation genuinely fails must surface that
//!   failure explicitly, never collapse into the same result an unoffered
//!   binding produces.
//!
//! Each test is independently discriminating: it asserts the exact admitted
//! shape, not just presence, so a regression in the invariant fails it.

use std::sync::Arc;

use verter_session::meta::MetaProject;
use verter_session::{AnalysisLevel, HostConfig, VerterHost};
use verter_type_expr::facts::{SemanticTypeSource, SourcePosition};
use verter_type_expr::locators::AuthoredBodyLocator;

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        // Single-threaded scheduler: avoids CPU oversubscription across the
        // many parallel test-binary threads each spinning up their own pool
        // (the same rationale `verter_session::meta_tests` documents).
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    );
    MetaProject::new(host)
}

/// A loose presence check: `Present`, any source shape. This alone does NOT
/// discriminate a genuinely admitted binding from a fallback
/// `Closed(Leaf(Primitive(Unknown)))` carrier — every Finding-B admission
/// assertion below pairs this with [`is_authored_present`], which pins the
/// EXACT shape (an authored decl-body locator, never the `Unknown` fallback).
fn is_genuinely_present(position: &SourcePosition) -> bool {
    position.present().is_some()
}

/// The EXACT shape every admitted `defineExpose` VALUE-BINDING field (a
/// `FieldKind::Binding` demand, no macro payload) reaches at the
/// component_meta-analysis level: a `Present` `AuthoredBodyLocator::DeclBody`
/// locator with an EMPTY path (`authored_field_source`,
/// `host_manage/eval_env.rs`, always emits this exact shape for a binding
/// demand) — never the `Closed(Leaf(Primitive(Unknown)))` fallback shape a
/// hidden admission failure could still render through `unknown_source()`,
/// and never a DIFFERENT `Authored` variant (`AugmentationBody`/
/// `JsdocTypedefBody`/`MacroPayload`), which would mean the binding-demand
/// path was bypassed entirely. This is the discriminating check
/// `is_genuinely_present` alone cannot make: a
/// `Present(Closed(Leaf(Primitive(Unknown))))` passes `is_genuinely_present`
/// but fails this.
fn is_authored_present(position: &SourcePosition) -> bool {
    matches!(
        position,
        SourcePosition::Present(SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(
            slot
        ))) if slot.path.is_empty()
    )
}

/// The symbol name an admitted binding's authored `DeclBody` locator anchors
/// to, when `position` is the exact shape [`is_authored_present`] checks.
/// Lets a caller pin WHICH local binding was actually resolved — e.g. that
/// `defineExpose({ public: local })` resolved the `local` declaration, not a
/// (nonexistent) declaration literally named `public`.
fn authored_binding_symbol(position: &SourcePosition) -> Option<&str> {
    match position {
        SourcePosition::Present(SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(
            slot,
        ))) if slot.path.is_empty() => Some(slot.anchor.symbol.as_ref()),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// defineExpose binding admission
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn expose_admits_plain_function_declaration_binding() {
    let project = make_project();
    project
        .upsert_base(
            "/FnExpose.vue",
            r#"<script setup lang="ts">
function increment() {
  return 1
}
defineExpose({ increment })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/FnExpose.vue")
        .expect("component meta resolves");
    let increment = meta
        .exposed
        .iter()
        .find(|e| e.name == "increment")
        .expect("increment must be exposed");
    assert!(
        is_genuinely_present(&increment.type_source),
        "a plain function declaration's defineExpose binding must not \
         silently degrade to Absent/Unknown, got {:?}",
        increment.type_source
    );
    assert!(
        is_authored_present(&increment.type_source),
        "a plain function declaration's defineExpose binding must reach the \
         exact authored decl-body shape, not the Unknown fallback, got {:?}",
        increment.type_source
    );
}

/// An UNTYPED call-initializer (`const count = ref(0)`) exposed by name —
/// the ref/computed axis. `ref`/`computed` calls carry no authored
/// annotation; their type lives on the inferred expression-source fact.
#[test]
fn expose_admits_untyped_ref_and_computed_bindings() {
    let project = make_project();
    project
        .upsert_base(
            "/RefExpose.vue",
            r#"<script setup lang="ts">
import { ref, computed } from 'vue'

const count = ref(0)
const doubled = computed(() => count.value * 2)
defineExpose({ count, doubled })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/RefExpose.vue")
        .expect("component meta resolves");
    for name in ["count", "doubled"] {
        let field = meta
            .exposed
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} must be exposed"));
        assert!(
            is_genuinely_present(&field.type_source),
            "untyped call-initializer binding {name} must not silently \
             degrade to Absent/Unknown, got {:?}",
            field.type_source
        );
        assert!(
            is_authored_present(&field.type_source),
            "untyped call-initializer binding {name} must reach the exact \
             authored decl-body shape, not the Unknown fallback, got {:?}",
            field.type_source
        );
    }
}

/// An EXPLICITLY ANNOTATED call-initializer (`const count: Ref<number> =
/// ref(0)`) — the "call-initializer residual" case. Its annotation should
/// already classify `Direct`
/// (`value_type_annotation_fact` / `fact_projection.rs`), so this pins that
/// the admission gate (and whatever handoff sits between the annotation fact
/// and the gate's read of it) actually admits it end-to-end.
#[test]
fn expose_admits_explicitly_typed_call_initializer_binding() {
    let project = make_project();
    project
        .upsert_base(
            "/TypedRefExpose.vue",
            r#"<script setup lang="ts">
import { ref, type Ref } from 'vue'

const count: Ref<number> = ref(0)
defineExpose({ count })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/TypedRefExpose.vue")
        .expect("component meta resolves");
    let count = meta
        .exposed
        .iter()
        .find(|e| e.name == "count")
        .expect("count must be exposed");
    assert!(
        is_genuinely_present(&count.type_source),
        "an EXPLICITLY annotated call-initializer must not silently \
         degrade to Absent/Unknown, got {:?}",
        count.type_source
    );
    assert!(
        is_authored_present(&count.type_source),
        "an EXPLICITLY annotated call-initializer must reach the exact \
         authored decl-body shape, not the Unknown fallback, got {:?}",
        count.type_source
    );
}

/// The "multiple" / "mixed full-API" shape: plain values, refs, computed,
/// and methods all exposed together in one `defineExpose` call — every axis
/// value must be admitted in the SAME macro, not just in isolation.
#[test]
fn expose_admits_mixed_full_api_bindings() {
    let project = make_project();
    project
        .upsert_base(
            "/MixedExpose.vue",
            r#"<script setup lang="ts">
import { ref, computed } from 'vue'

const label: string = 'hello'
const count = ref(0)
const doubled = computed(() => count.value * 2)
function reset() {
  count.value = 0
}

defineExpose({ label, count, doubled, reset })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/MixedExpose.vue")
        .expect("component meta resolves");
    for name in ["label", "count", "doubled", "reset"] {
        let field = meta
            .exposed
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} must be exposed"));
        assert!(
            is_genuinely_present(&field.type_source),
            "mixed full-API binding {name} must not silently degrade to \
             Absent/Unknown, got {:?}",
            field.type_source
        );
        assert!(
            is_authored_present(&field.type_source),
            "mixed full-API binding {name} must reach the exact authored \
             decl-body shape, not the Unknown fallback, got {:?}",
            field.type_source
        );
    }
}

/// Project-aware / imported type position: the exposed binding's TYPE is
/// declared in a separate file. Exercises the "type position: imported"
/// acceptance-matrix axis alongside the admission repair — the admission
/// gate fix must not depend on the type being source-local.
#[test]
fn expose_admits_binding_with_imported_type_position() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    );
    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Api {
  open(): void
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Imported.vue",
            r#"<script setup lang="ts">
import type { Api } from './types'

const api: Api = { open() {} }
defineExpose({ api })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/Imported.vue")
        .expect("component meta resolves");
    let api = meta
        .exposed
        .iter()
        .find(|e| e.name == "api")
        .expect("api must be exposed");
    assert!(
        is_genuinely_present(&api.type_source),
        "an exposed binding whose declared type is imported from another \
         file must not silently degrade to Absent/Unknown, got {:?}",
        api.type_source
    );
    assert!(
        is_authored_present(&api.type_source),
        "an exposed binding whose declared type is imported from another \
         file must reach the exact authored decl-body shape, not the \
         Unknown fallback, got {:?}",
        api.type_source
    );
}

/// An ENUM binding exposed by name: `enum Status { ... }; defineExpose({
/// Status })`. An enum value declaration carries no annotation, no object
/// shape, and no signature — its type lives entirely on
/// `PreparedValueDecl.enum_members`. Exercises BOTH the admission gate
/// (`enum_members.is_some()`) AND the dereference that must actually
/// materialize the enum-object surface from that same fact — the admission
/// gate alone is not sufficient (see
/// `expose_enum_binding_materializes_object_type` below for the
/// materialization half).
#[test]
fn expose_admits_enum_binding() {
    let project = make_project();
    project
        .upsert_base(
            "/EnumExpose.vue",
            r#"<script setup lang="ts">
enum Status {
  Active,
  Inactive,
}
defineExpose({ Status })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/EnumExpose.vue")
        .expect("component meta resolves");
    let status = meta
        .exposed
        .iter()
        .find(|e| e.name == "Status")
        .expect("Status must be exposed");
    assert!(
        matches!(
            status.type_source,
            SourcePosition::Present(SemanticTypeSource::Authored(_))
        ),
        "an enum defineExpose binding must reach a Present authored source, \
         not silently degrade to Absent/Unknown, got {:?}",
        status.type_source
    );
}

/// The materialization half of the enum binding: the admitted `Authored`
/// locator from [`expose_admits_enum_binding`] above must actually
/// DEREFERENCE to the enum-object surface, not fail the strict raise with
/// `UnraisableSource`. This is independently discriminating from the
/// admission-gate fix alone — an admission-only fix (`enum_members.is_some()`
/// with no matching dereference fallback) reaches `Present` here and then
/// fails materialization outright.
#[test]
fn expose_enum_binding_materializes_object_type() {
    let project = make_project();
    project
        .upsert_base(
            "/EnumOutput.vue",
            r#"<script setup lang="ts">
enum Status {
  Active,
  Inactive,
}
defineExpose({ Status })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/EnumOutput.vue")
        .expect(
            "output materialization must not fail with UnraisableSource for an enum \
             defineExpose binding",
        )
        .expect("component resolves");
    let (analysis, _resolution, types) = output.into_parts();
    let index = analysis
        .exposed
        .iter()
        .position(|e| e.name == "Status")
        .expect("Status must be exposed");
    let lanes = types.into_lanes();
    let materialized = &lanes.exposed[index];
    let verter_type_expr::TypeExpr::Object(object) = materialized else {
        panic!(
            "an enum defineExpose binding must materialize as the enum-object \
             type, got {materialized:?}"
        );
    };
    let member_names: Vec<&str> = object
        .properties
        .iter()
        .filter_map(|member| match member {
            verter_type_expr::ObjectMember::Property(prop) => match &prop.key {
                verter_type_expr::TypeAuthoredPropertyKey::String(name) => Some(name.as_ref()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        member_names,
        vec!["Active", "Inactive"],
        "the materialized enum-object type must carry the enum's ACTUAL \
         member names in declaration order, not just an empty/opaque object \
         shape, got {materialized:?}"
    );
}

/// An exposed field whose KEY differs from its aliased local binding
/// (`defineExpose({ public: local })`) must resolve the LOCAL BINDING's
/// type, not look up a binding named after the exposed property key (which
/// does not exist as a local declaration at all). A pre-fix admission gate
/// that keys off `field.name` looks for a binding literally named `public`,
/// finds nothing, and silently degrades to Absent/Unknown even though
/// `local` is a perfectly resolvable binding right there in scope.
#[test]
fn expose_admits_aliased_binding_by_referenced_identifier_not_property_key() {
    let project = make_project();
    project
        .upsert_base(
            "/AliasExpose.vue",
            r#"<script setup lang="ts">
const local: string = 'hello'
defineExpose({ public: local })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/AliasExpose.vue")
        .expect("component meta resolves");
    let public = meta
        .exposed
        .iter()
        .find(|e| e.name == "public")
        .expect("public must be exposed under its declared property key");
    assert!(
        is_genuinely_present(&public.type_source),
        "an aliased defineExpose binding must resolve the referenced LOCAL \
         binding's type, not silently degrade to Absent/Unknown because no \
         binding is named after the property key, got {:?}",
        public.type_source
    );
    assert!(
        is_authored_present(&public.type_source),
        "an aliased defineExpose binding must reach the exact authored \
         decl-body shape of the REFERENCED local binding, not the Unknown \
         fallback, got {:?}",
        public.type_source
    );
    assert_eq!(
        authored_binding_symbol(&public.type_source),
        Some("local"),
        "the admitted binding must anchor to the REFERENCED local \
         declaration `local`, never the exposed property key `public` \
         (which is not itself a local declaration), got {:?}",
        public.type_source
    );
}

/// A NON-IDENTIFIER exposed value (`defineExpose({ public: local.foo })`)
/// carries NO `referenced_binding` at all (a member expression has no
/// single referenced binding) — `resolved_binding_key()` must NOT fall
/// back to the exposed property key `public`, even when `public` happens
/// to ALSO be an unrelated in-scope binding name. A pre-fix fallback
/// resolves that unrelated top-level `public` binding and publishes its
/// (wrong) type; the fix must leave the field genuinely unresolvable
/// (unannotated), never silently substitute an unrelated binding's type.
#[test]
fn expose_non_identifier_value_never_falls_back_to_colliding_property_key() {
    let project = make_project();
    project
        .upsert_base(
            "/NonIdentifierExpose.vue",
            r#"<script setup lang="ts">
const public: number = 1
const local: { foo: string } = { foo: 'hi' }
defineExpose({ public: local.foo })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/NonIdentifierExpose.vue")
        .expect("component meta resolves");
    let public = meta
        .exposed
        .iter()
        .find(|e| e.name == "public")
        .expect("public must be exposed under its declared property key");
    assert_eq!(
        public.type_source,
        SourcePosition::unannotated(),
        "a non-identifier exposed value must never fall back to an \
         unrelated in-scope binding that happens to share the exposed \
         property key's name — it must stay the exact unannotated \
         absence (never the colliding `public: number` binding's type), \
         got {:?}",
        public.type_source
    );
}

// ─────────────────────────────────────────────────────────────────────────
// `Present -> UnraisableSource` materialization
// ─────────────────────────────────────────────────────────────────────────
//
// Distinct from the `Absent -> Unknown` silent degrade above: this section
// covers a position that DOES reach `SourcePosition::Present` but whose
// strict raise (`raise_semantic_type_source_to_hot_strict`) then fails to
// produce a graph handle for it — `ComponentMetaOutputFailure::UnraisableSource`
// (`crates/verter_session/src/meta_resolve/output.rs:562-564`).
//
// The defineExpose macro-argument-type expander's `FieldKind::Binding` field
// producer (`authored_field_source` closure, `host_manage/eval_env.rs`)
// emits an `AuthoredBodyLocator::DeclBody` locator with an EMPTY path for
// every admitted binding, unconditionally — the SAME locator shape whether
// the target declaration carries an authored annotation, an object shape,
// or (a `function` declaration) ONLY a signature. The locator's dereference
// (`navigate_value_parts`, `decl_body_memo/locator_deref.rs`) must read the
// annotation, object-shape, AND signature facts at that position — a
// function-declaration `defineExpose` binding reaches a genuine `Present`
// source, and its dereference must materialize the signature's function
// type rather than failing with `UnraisableSource`.
//
// This is independently discriminating relative to the admission-gate
// coverage above: a tree with ADMISSION but WITHOUT the
// `navigate_value_parts` signature fallback below fails
// `expose_function_binding_materializes_a_function_type` with exactly this
// `UnraisableSource` error.

/// A plain function-declaration `defineExpose` binding must materialize a
/// REAL function type through the strict output-materialization raise, not
/// fail with `UnraisableSource` and not silently render `unknown`.
#[test]
fn expose_function_binding_materializes_a_function_type() {
    let project = make_project();
    project
        .upsert_base(
            "/FnOutput.vue",
            r#"<script setup lang="ts">
function increment() {
  return 1
}
defineExpose({ increment })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/FnOutput.vue")
        .expect(
            "output materialization must not fail with UnraisableSource for a plain \
             function-declaration defineExpose binding",
        )
        .expect("component resolves");
    let (analysis, _resolution, types) = output.into_parts();
    let index = analysis
        .exposed
        .iter()
        .position(|e| e.name == "increment")
        .expect("increment must be exposed");
    let lanes = types.into_lanes();
    let materialized = &lanes.exposed[index];
    let verter_type_expr::TypeExpr::Function(function) = materialized else {
        panic!(
            "a plain function declaration's defineExpose type must materialize \
             as a function type, got {materialized:?}"
        );
    };
    assert!(
        function.parameters.is_empty(),
        "increment() takes no parameters, got {:?}",
        function.parameters
    );
    assert_eq!(
        function.return_type.as_deref(),
        Some(&verter_type_expr::TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Number
        )),
        "increment's inferred return type (from `return 1`) must materialize \
         as the actual number return type, not an opaque/untyped function, \
         got {materialized:?}"
    );
}

/// The SAME reproduction through `get_component_meta_output`'s Result
/// contract, with each of the three outcomes separated instead of collapsed
/// into one `.expect(...)`.
///
/// The producer defect this fixture characterises
/// (`decl_body_memo/locator_deref.rs`'s `navigate_value_parts`: an
/// empty-path value position over a declaration that carries ONLY a
/// signature) surfaced as one specific typed error —
/// [`ComponentMetaOutputFailure::UnraisableSource`] on the
/// [`ComponentMetaOutputLane::Exposed`] lane.
///
/// **Where the discrimination lives.** The correct answer for this input is
/// SUCCESS, so every `Err` is red whatever it carries. No comparison of the
/// failure's lane or variant could change that verdict, so this test makes
/// none: the `Err` arm reports the observed error in full and names the
/// owning producer, and that is all it claims to do. It is not an oracle and
/// nothing here should describe it as one.
///
/// What discriminates is the `Ok(Some(_))` arm's pin on the exposed member's
/// own materialized shape — a real function type with the authored parameter
/// count — which a silent per-field degrade behind an overall-successful
/// component resolve would fail. That pin is the verdict rail and is
/// plant-proven.
///
/// A verdict-bearing assertion on an exact `(lane, failure)` pair does exist,
/// on inputs where a typed failure is the CORRECT answer:
/// `runtime_constructor_matrix.rs::fail_closed_constructor_positions_surface_the_exact_typed_failure`.
///
/// The three arms:
///
/// - `Err(_)` — a regression. Report the observed error in full and name the
///   owning producer.
/// - `Ok(None)` — the forbidden swallow: a failure demoted to absence,
///   indistinguishable from a missing canonical.
/// - `Ok(Some(_))` — the repaired tree; the field's own materialized shape
///   is pinned.
#[test]
fn expose_function_binding_output_materializes_never_absent_never_unraisable() {
    let project = make_project();
    project
        .upsert_base(
            "/FnOutput2.vue",
            r#"<script setup lang="ts">
function reset() {}
defineExpose({ reset })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = match project.host().get_component_meta_output("/FnOutput2.vue") {
        Err(error) => panic!(
            "REGRESSION: the `defineExpose`-bound function declaration failed to \
             materialize. The known pre-repair failure of this producer was \
             `Exposed` / `UnraisableSource` — the empty-path value deref finding no \
             annotation, no object shape, and not recovering the lone function \
             signature. Owning producer: the whole-signature recovery in \
             `navigate_value_parts` \
             (`crates/verter_session/src/decl_body_memo/locator_deref.rs`). \
             Observed in full: {error:?}"
        ),
        Ok(None) => panic!(
            "the component resolved to ABSENCE. A materialization failure demoted to \
             `Ok(None)` is the forbidden swallow: the typed failure must survive as \
             `Err`, never collapse into the same result a missing canonical produces"
        ),
        Ok(Some(output)) => output,
    };
    let (analysis, _resolution, types) = output.into_parts();
    let index = analysis
        .exposed
        .iter()
        .position(|e| e.name == "reset")
        .expect("reset must be exposed");
    let lanes = types.into_lanes();
    let materialized = &lanes.exposed[index];
    // `Ok(Some(component))` alone is not proof the exposed member itself
    // resolved genuinely. Pin the recovered SIGNATURE, not merely the fact
    // that something function-shaped came back: `reset()` takes no
    // parameters, and the whole point of the repair is that the lone
    // signature is recovered rather than substituted for.
    let verter_type_expr::TypeExpr::Function(signature) = materialized else {
        panic!(
            "reset's defineExpose type must materialize as a real function type, \
             not silently degrade to an opaque/unknown substitute behind an \
             overall `Some` component resolve, got {materialized:?}"
        );
    };
    assert!(
        signature.parameters.is_empty(),
        "reset is authored with no parameters; the recovered signature must be \
         the authored one, not a fabricated or wrongly-arity stand-in, got \
         {signature:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Invocation-shape coverage: warm, concurrent, batch, overlay/session
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn expose_admission_is_stable_cold_and_warm() {
    let project = make_project();
    project
        .upsert_base(
            "/Warm.vue",
            r#"<script setup lang="ts">
import { ref } from 'vue'

const count = ref(0)
defineExpose({ count })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let cold = project
        .host()
        .get_component_meta("/Warm.vue")
        .expect("cold resolve");
    let warm = project
        .host()
        .get_component_meta("/Warm.vue")
        .expect("warm resolve");

    let cold_count = &cold
        .exposed
        .iter()
        .find(|e| e.name == "count")
        .unwrap()
        .type_source;
    let warm_count = &warm
        .exposed
        .iter()
        .find(|e| e.name == "count")
        .unwrap()
        .type_source;
    assert!(is_genuinely_present(cold_count), "cold: {cold_count:?}");
    assert!(is_genuinely_present(warm_count), "warm: {warm_count:?}");
    assert!(is_authored_present(cold_count), "cold: {cold_count:?}");
    assert!(is_authored_present(warm_count), "warm: {warm_count:?}");
    assert_eq!(
        cold_count, warm_count,
        "cold and warm resolutions of the same file must agree exactly"
    );
}

/// `Promise.all`-equivalent concurrent invocation: resolve `defineExpose`
/// admission for distinct files from multiple threads against one shared
/// host — the admission gate fix must not be a single-threaded-only fix.
#[test]
fn expose_admission_is_stable_under_concurrent_resolution() {
    let project = make_project();
    project
        .upsert_base(
            "/Concurrent1.vue",
            r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
defineExpose({ count })
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Concurrent2.vue",
            r#"<script setup lang="ts">
function increment() {
  return 1
}
defineExpose({ increment })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    std::thread::scope(|scope| {
        let h1 = scope.spawn(|| {
            let meta = project
                .host()
                .get_component_meta("/Concurrent1.vue")
                .expect("resolves");
            let count = &meta
                .exposed
                .iter()
                .find(|e| e.name == "count")
                .unwrap()
                .type_source;
            assert!(is_genuinely_present(count));
            assert!(is_authored_present(count));
        });
        let h2 = scope.spawn(|| {
            let meta = project
                .host()
                .get_component_meta("/Concurrent2.vue")
                .expect("resolves");
            let increment = &meta
                .exposed
                .iter()
                .find(|e| e.name == "increment")
                .unwrap()
                .type_source;
            assert!(is_genuinely_present(increment));
            assert!(is_authored_present(increment));
        });
        h1.join().unwrap();
        h2.join().unwrap();
    });
}

/// Batch invocation (`get_component_meta_output_batch`) must agree with the
/// scalar `get_component_meta_output` on `defineExpose` admission.
#[test]
fn expose_admission_agrees_scalar_and_batch() {
    let project = make_project();
    project
        .upsert_base(
            "/Batch1.vue",
            r#"<script setup lang="ts">
function reset() {
  return 0
}
defineExpose({ reset })
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Batch2.vue",
            r#"<script setup lang="ts">
function increment() {
  return 1
}
defineExpose({ increment })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().expect("batch session opens");

    let scalar1 = session
        .get_component_meta_output("/Batch1.vue")
        .expect("output materializes")
        .expect("component resolves");
    let scalar2 = session
        .get_component_meta_output("/Batch2.vue")
        .expect("output materializes")
        .expect("component resolves");

    let mut batch = session
        .get_component_meta_output_batch(&["/Batch1.vue".to_string(), "/Batch2.vue".to_string()])
        .expect("batch call succeeds");
    assert_eq!(batch.len(), 2);
    let batch2 = batch
        .pop()
        .unwrap()
        .expect("Batch2.vue batch job succeeds")
        .expect("Batch2.vue resolves in batch");
    let batch1 = batch
        .pop()
        .unwrap()
        .expect("Batch1.vue batch job succeeds")
        .expect("Batch1.vue resolves in batch");

    let (scalar1_analysis, ..) = scalar1.into_parts();
    let (batch1_analysis, ..) = batch1.into_parts();
    let scalar1_reset = &scalar1_analysis
        .exposed
        .iter()
        .find(|e| e.name == "reset")
        .unwrap()
        .type_source;
    let batch1_reset = &batch1_analysis
        .exposed
        .iter()
        .find(|e| e.name == "reset")
        .unwrap()
        .type_source;
    assert!(is_genuinely_present(scalar1_reset));
    assert!(is_authored_present(scalar1_reset));
    assert_eq!(
        scalar1_reset, batch1_reset,
        "scalar and batch defineExpose admission must agree"
    );

    let (scalar2_analysis, ..) = scalar2.into_parts();
    let (batch2_analysis, ..) = batch2.into_parts();
    let scalar2_count = &scalar2_analysis
        .exposed
        .iter()
        .find(|e| e.name == "increment")
        .unwrap()
        .type_source;
    let batch2_count = &batch2_analysis
        .exposed
        .iter()
        .find(|e| e.name == "increment")
        .unwrap()
        .type_source;
    assert!(is_genuinely_present(scalar2_count));
    assert!(is_authored_present(scalar2_count));
    assert_eq!(scalar2_count, batch2_count);
}

/// Overlay / request-view scope: a session overlay resolving `defineExpose`
/// admission must admit it exactly like the base session — the admission
/// gate fix must not be a base-view-only fix.
#[test]
fn expose_admission_holds_in_overlay_session() {
    let project = make_project();
    project
        .upsert_base(
            "/Overlay.vue",
            r#"<script setup lang="ts">
const label: string = 'hello'
defineExpose({ label })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Base session: sanity baseline.
    let base_meta = project
        .host()
        .get_component_meta("/Overlay.vue")
        .expect("base resolves");
    let base_label = &base_meta
        .exposed
        .iter()
        .find(|e| e.name == "label")
        .unwrap()
        .type_source;
    assert!(is_genuinely_present(base_label));
    assert!(is_authored_present(base_label));

    // Overlay session: an edit that ADDS a second defineExpose ref binding,
    // visible only in the session view.
    let session = project.open_session().expect("session opens");
    session
        .upsert(
            "/Overlay.vue",
            r#"<script setup lang="ts">
import { ref } from 'vue'
const label: string = 'hello'
const enabled = ref(true)
defineExpose({ label, enabled })
</script>
<template><div /></template>"#
                .to_string(),
        )
        .expect("overlay upsert");

    let overlay_meta = session
        .get_component_meta("/Overlay.vue")
        .expect("overlay resolves")
        .expect("component resolves in overlay");
    let enabled = &overlay_meta
        .exposed
        .iter()
        .find(|e| e.name == "enabled")
        .unwrap()
        .type_source;
    assert!(
        is_genuinely_present(enabled),
        "the admission-gate fix must hold inside an overlay/request-view session"
    );
    assert!(
        is_authored_present(enabled),
        "the admission-gate fix must reach the exact authored decl-body \
         shape inside an overlay/request-view session, not the Unknown \
         fallback, got {enabled:?}"
    );

    // Base session view is unaffected by the overlay (isolation sanity).
    let base_meta_after = project
        .host()
        .get_component_meta("/Overlay.vue")
        .expect("base still resolves");
    assert!(
        base_meta_after.exposed.iter().all(|e| e.name != "enabled"),
        "the overlay edit must not leak into the base session view"
    );
}
