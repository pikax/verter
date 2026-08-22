//! `vue_macro_codegen` typeinfo tests — runtime-object `defineExpose`
//! per-member `TypeOf` resolution.
//!
//! Split from `vue_macro_codegen.rs` (same test module, sibling file) to keep
//! each production `.rs` under the module-size budget. Runs as a child module
//! of the parent test module and reaches its shared fixtures/helpers
//! (`upsert`, `produce`, the imported DTO types) through `use super::*`.
//!
//! These tests prove the SESSION-SIDE half of the mechanism: that a real
//! `VerterHost` resolves a runtime-object `defineExpose({ ... })` member's
//! REFERENCED BINDING through the shared `TypeOf` dispatch and produces a
//! `MacroTscProjection::Expose` row carrying the real resolved type (or a
//! typed `Unavailable` degradation). The COMPILER-SIDE half — that
//! `apply_tsc_bundle`/`render_instance_shape_body` correctly CONSUME that row
//! — is proven separately in `verter_compiler::tsc::tests` via hand-built
//! bundles.

use super::*;

fn expose_projection(
    output: &crate::typeinfo::vue_macro_codegen::VueMacroCodegenOutput,
) -> &verter_macro_dto::TscExposeProjection {
    let bundle = output.tsc.as_ref().expect("TSC bundle");
    assert_eq!(bundle.entries.len(), 1, "one entry per defineExpose call");
    let MacroTscOutcome::Complete(MacroTscProjection::Expose(expose)) = &bundle.entries[0].outcome
    else {
        panic!("complete expose projection expected: {bundle:?}");
    };
    expose
}

fn member<'a>(
    expose: &'a verter_macro_dto::TscExposeProjection,
    name: &str,
) -> &'a verter_macro_dto::TscExposeMemberType {
    &expose
        .members
        .iter()
        .find(|member| member.name == name)
        .unwrap_or_else(|| panic!("member `{name}` must be published, got: {expose:?}"))
        .member_type
}

#[test]
fn runtime_object_expose_call_initialized_const_resolves_real_type() {
    // The ruling's canonical shape: a call-initialized `const` with NO
    // authored type annotation. Its real type is the RESULT of inference —
    // exactly what the shared `TypeOf` dispatch computes.
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/Expose.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
function seed(): number { return 42 }
const count = seed()
defineExpose({ count })
</script>"#,
    );

    let output = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    let expose = expose_projection(&output);

    let verter_macro_dto::TscExposeMemberType::Resolved(text) = member(expose, "count") else {
        panic!("`count` must resolve, got: {expose:?}");
    };
    assert_eq!(
        text.as_str().trim(),
        "number",
        "the real inferred type must be resolved, not a fallback shape"
    );
}

#[test]
fn runtime_object_expose_unannotated_function_resolves_real_signature() {
    // An unannotated-RETURN `function` declaration resolves its REAL
    // inferred return type via `TypeOf` — better than the authored-syntax
    // callable shape (`(step: any) => any`), which never recovers a return
    // type at all.
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/Expose.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
function bump(step: number) {
  return step
}
defineExpose({ bump })
</script>"#,
    );

    let output = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    let expose = expose_projection(&output);

    let verter_macro_dto::TscExposeMemberType::Resolved(text) = member(expose, "bump") else {
        panic!("`bump` must resolve, got: {expose:?}");
    };
    // Exact match, not a loose substring check: an earlier version of this
    // test asserted only `contains("step: number") && contains("number")`,
    // which is satisfied trivially by the PARAMETER text alone
    // (`"step: number"` already contains `"number"`) — it would have kept
    // passing even if the return type were the leaked
    // `unmodeledPosition` sentinel instead of a real inferred type. Pin the
    // full string so a regression back to that leaked shape fails loudly.
    assert_eq!(
        text.as_str().trim(),
        "(step: number) => number",
        "the real inferred signature (real param AND return type) must be resolved, got: {text:?}"
    );
}

#[test]
fn runtime_object_expose_function_arithmetic_return_position_reports_typed_unavailable_not_leaked_sentinel(
) {
    // Regression: a function whose return position is a binary arithmetic
    // expression over a typed parameter (`step + 1`) is a position the
    // shared flow-return substrate cannot currently model
    // (`QueryError::UnmodeledPosition`). Before the leaked-sentinel guard,
    // `render_tsc_node` returned `Ok("(step: number) => unmodeledPosition")`
    // — a loose `contains("number")` assertion could not tell that apart
    // from a real resolved return type (the param slice `"step: number"`
    // already contains the substring). The producer must degrade to the
    // honest typed `Unavailable` outcome instead of publishing that leaked
    // sentinel — this still-unresolved flow-inference gap is a pre-existing,
    // separately-scoped limitation, not something this producer papers over.
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/Expose.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
function bump(step: number) {
  return step + 1
}
defineExpose({ bump })
</script>"#,
    );

    let output = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    let expose = expose_projection(&output);

    match member(expose, "bump") {
        verter_macro_dto::TscExposeMemberType::Unavailable(_) => {
            // Honest degradation — the expected outcome today.
        }
        verter_macro_dto::TscExposeMemberType::Resolved(text) => {
            assert!(
                !text.as_str().contains("unmodeledPosition"),
                "resolved member text must never leak the internal compat sentinel, got: {text:?}"
            );
        }
    }
}

#[test]
fn runtime_object_expose_method_shorthand_reports_typed_unavailable() {
    // A method shorthand property (`bump() {}`) has NO referenced binding to
    // typeof — nothing structurally recoverable — and must report a typed
    // `Unavailable` degradation, never a fabricated success.
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/Expose.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
defineExpose({
  bump() {
    return 1
  },
})
</script>"#,
    );

    let output = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    let expose = expose_projection(&output);

    assert_eq!(
        member(expose, "bump"),
        &verter_macro_dto::TscExposeMemberType::Unavailable(
            TscDeclarationFailureReason::Unsupported(UnsupportedReason::SemanticConstruct)
        ),
        "a method shorthand has no capturable referenced binding"
    );
}

#[test]
fn runtime_object_expose_non_identifier_value_reports_typed_unavailable() {
    // A non-identifier value expression (`{ sum: a + b }`) — nothing
    // structurally recoverable either, same typed degradation as a method.
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/Expose.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
const a = 1
const b = 2
defineExpose({ sum: a + b })
</script>"#,
    );

    let output = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    let expose = expose_projection(&output);

    assert_eq!(
        member(expose, "sum"),
        &verter_macro_dto::TscExposeMemberType::Unavailable(
            TscDeclarationFailureReason::Unsupported(UnsupportedReason::SemanticConstruct)
        ),
        "a non-identifier value expression has no capturable referenced binding"
    );
}

#[test]
fn runtime_object_expose_resolved_type_expands_a_referenced_local_interface() {
    // `Expanded` mode (matching Props/Emits/Model's own choice for TSC
    // output — the consumer needs the full resolved structural shape, not a
    // shallow reference) fully materializes a local interface reference
    // inline rather than emitting a bare name — so a local interface member
    // needs NO scope retention at all: there is no `Box` identifier left in
    // the rendered text to keep in scope.
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/Expose.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
interface Box { value: number }
function makeBox(v: number): Box {
  return { value: v }
}
const count = makeBox(0)
defineExpose({ count })
</script>"#,
    );

    let output = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    let expose = expose_projection(&output);

    let verter_macro_dto::TscExposeMemberType::Resolved(text) = member(expose, "count") else {
        panic!("`count` must resolve, got: {expose:?}");
    };
    assert!(
        text.as_str().contains("value") && text.as_str().contains("number"),
        "the resolved type must carry the interface's real structural shape, got: {text:?}"
    );
    assert!(
        expose.scope.dependency_declarations.is_empty(),
        "an inlined structural shape needs no retained local declaration, got: {:?}",
        expose.scope
    );
}

#[test]
fn runtime_object_expose_shorthand_and_explicit_identifier_both_resolve() {
    // Both member forms — shorthand (`{ count }`) and explicit-identifier
    // (`{ total: count }`) — carry a structurally captured referenced
    // binding and resolve identically.
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/Expose.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
function seed(): number { return 42 }
const count = seed()
defineExpose({ count, total: count })
</script>"#,
    );

    let output = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    let expose = expose_projection(&output);

    for name in ["count", "total"] {
        let verter_macro_dto::TscExposeMemberType::Resolved(text) = member(expose, name) else {
            panic!("`{name}` must resolve, got: {expose:?}");
        };
        assert_eq!(
            text.as_str().trim(),
            "number",
            "member `{name}` resolves the real type"
        );
    }
}

#[test]
fn runtime_object_expose_generic_closure_inferred_member_never_leaks_compat_sentinel() {
    // Regression: a NESTED `QueryError::Miss` inside a structurally-
    // materialized instantiated type — a generic function's type parameter
    // inferred from a closure argument's return type, exactly the shape
    // `computed(() => ...)` produces — must never surface as the leaked
    // internal compat-projection sentinel spelling
    // (`semantic_query::compat_spelling::SEMANTIC_MISS`) baked into
    // published declaration text. This reproduces the root cause with NO
    // Vue types involved at all: a purely local generic-closure pattern is
    // enough to trigger the same nested-miss materialization gap.
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/Expose.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
interface Box<T> { readonly value: T }
function wrap<T>(getter: () => T): Box<T> {
  return { value: getter() }
}
const doubled = wrap(() => 1 + 2)
defineExpose({ doubled })
</script>"#,
    );

    let output = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    let expose = expose_projection(&output);

    match member(expose, "doubled") {
        verter_macro_dto::TscExposeMemberType::Resolved(text) => {
            assert!(
                !text.as_str().contains("semanticMiss"),
                "resolved member text must never leak the internal compat sentinel, got: {text:?}"
            );
        }
        verter_macro_dto::TscExposeMemberType::Unavailable(_) => {
            // An honest degradation to `Unavailable` is an acceptable
            // outcome for this still-unresolved generic-inference gap —
            // publishing the leaked sentinel as if it were a real type is
            // never acceptable.
        }
    }
}

#[test]
fn runtime_object_expose_member_typed_via_a_user_type_named_exactly_semantic_miss_still_resolves() {
    // Discriminates the typed degradation carrier from a textual heuristic.
    // The user's OWN recursive type alias is spelled exactly `semanticMiss`
    // — the same standalone-token spelling `compat_spelling::SEMANTIC_MISS`
    // reserves for the terminal compat-projection sentinel — and its
    // self-reference renders as the bare identifier text `"semanticMiss"`
    // nested inside an otherwise fully-materialized, non-degraded shape (a
    // `RecursiveRef` is a legitimate, deliberately-materialised placeholder,
    // never a resolver miss). A text-scanning screen over the rendered
    // declaration cannot tell that apart from a genuinely leaked sentinel
    // and downgrades the whole member to `Unavailable`; the typed
    // `has_degradation` carrier this producer now reads instead never
    // looks at rendered text, so the real, non-degraded resolution
    // survives.
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/Expose.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
interface semanticMiss { value: number; self: semanticMiss }
declare const seed: semanticMiss
const thing = seed
defineExpose({ thing })
</script>"#,
    );

    let output = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    let expose = expose_projection(&output);

    let verter_macro_dto::TscExposeMemberType::Resolved(text) = member(expose, "thing") else {
        panic!(
            "a genuinely-resolved member typed via a recursive user alias named \
             `semanticMiss` must survive as Resolved, got: {expose:?}"
        );
    };
    assert!(
        text.as_str().contains("value"),
        "member `thing` resolves its real user-authored shape, not a degraded Unavailable: {text:?}"
    );
}

#[test]
fn runtime_object_expose_duplicate_authored_names_receive_distinct_anchors() {
    // Two `defineExpose` members share the authored name `dup` at distinct
    // source positions. Each row's anchor must encode its OWN authored
    // position — a name-based re-scan resolves both occurrences to the
    // position of the FIRST match, collapsing two distinct members onto one
    // anchor.
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/Expose.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
const a = 1
const b = "two"
defineExpose({ dup: a, dup: b })
</script>"#,
    );

    let output = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    let expose = expose_projection(&output);
    let dups: Vec<_> = expose.members.iter().filter(|m| m.name == "dup").collect();
    assert_eq!(
        dups.len(),
        2,
        "both duplicate-named members must be published as distinct rows, got: {expose:?}"
    );
    assert_ne!(
        dups[0].anchor, dups[1].anchor,
        "duplicate authored names must not collapse onto the same anchor: {expose:?}"
    );
}
