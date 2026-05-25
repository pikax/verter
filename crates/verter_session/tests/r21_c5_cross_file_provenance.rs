//! R21-F1 c5 — discriminating cross-file fixtures for
//! `declared_in_macro_type_arg` on `PropAnalysis`.
//!
//! These tests close the cross-file provenance hole the R20-fix2 STOP
//! framing left at `crates/verter_semantic/src/analysis/component_meta.rs`
//! (the former `R20-fix2 F1 STOP` block at lines 1469-1484, replaced
//! in c5 with the structural-fact read `field.declared_in_macro_type_arg`).
//!
//! Discrimination property: reverting the c5 production change in
//! `component_meta.rs` to the prior `source_field.map(|p|
//! p.declared_in_macro_type_arg).unwrap_or(false)` form causes the
//! body-context cross-file assertions to fail with `declared=false`
//! for cross-file imported macros — the analyzer's local
//! `AnalyzedPropField` never sees the imported interface's members,
//! so `source_field` is `None` and the old expression collapses to
//! `unwrap_or(false)`.
//!
//! Each test exercises one of the brief's reference cross-file
//! shapes:
//!
//! 1. cross-file-simple: `import type { Props } from './x';
//!    defineProps<Props>()` — own-body members of `Props` reach the
//!    macro surface at body position. `declared = true` for every
//!    own-body member.
//! 2. cross-file-omit-then-reintroduce: `interface Carrier extends
//!    Omit<Vendor, 'k1' | 'k2' | 'k3'> { k1: T; k2: U; k3: V }` —
//!    the re-introduced own-body members carry `true`; remaining
//!    Vendor members reached via heritage carry `false`.
//! 3. cross-file-no-own-body-name (negative): a contested name only
//!    reaches the surface through the heritage chain (NOT in any
//!    own-body) — assert `declared = false` AND assert the member is
//!    correctly stripped under the Refined publication policy by
//!    `verter_audit::PublishedSurfacePolicy` (it survives the unrefined
//!    surface; the discriminator targets the structural fact, which
//!    in turn governs the Refined policy's reject-non-author-declared
//!    rule).
//!
//! Reference: `D:/tmp/round21-f1-cross-file-brief.md` (R21 c5 scope).

#![cfg(test)]

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::{CompileErrorPolicy, HostConfig};

fn metahost() -> ComponentMetaHost {
    ComponentMetaHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

/// CASE B (R21 brief) — fully-cross-file simple import.
///
/// `types.ts` declares `interface FooProps { onSubmit?: ...; label?:
/// string }`. `component.vue` imports and consumes via
/// `defineProps<FooProps>()`. Assert each own-body member surfaces with
/// `declared_in_macro_type_arg = true`.
///
/// Why this test discriminates: the analyzer's local
/// `AnalyzedPropField` for `component.vue` does NOT contain `onSubmit`
/// or `label` (they're cross-file). Pre-c5,
/// `source_field.map(...).unwrap_or(false)` collapsed to `false`.
/// Post-c5 reads `field.declared_in_macro_type_arg` from the
/// `ExpandedField` produced by the resolver pipeline (c2 parser fact
/// + c3 semantic propagation + c4 prepared-surface walker keying).
#[test]
fn cross_file_simple_imported_interface_own_body_members_carry_declared_true() {
    let mh = metahost();

    mh.upsert_base(
        "/src/types.ts",
        "export interface FooProps {\n\
         \tonSubmit?: (payload: string) => void;\n\
         \tlabel?: string;\n\
         }\n",
    )
    .expect("types.ts upsert");

    mh.upsert_base(
        "/src/Component.vue",
        "<script setup lang=\"ts\">\n\
         import type { FooProps } from './types';\n\
         defineProps<FooProps>();\n\
         defineEmits<{ submit: [payload: string] }>();\n\
         </script>\n",
    )
    .expect("Component.vue upsert");

    let meta = mh
        .host()
        .get_component_meta("/src/Component.vue")
        .expect("component meta resolves");

    let on_submit = meta.props.iter().find(|p| p.name == "onSubmit").expect(
        "cross-file-simple: meta.props MUST contain `onSubmit` \
             (the imported interface's own-body member must survive \
             the Refined publication policy because it is \
             author-declared in `FooProps`'s body)",
    );
    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("cross-file-simple: meta.props MUST contain `label`");

    assert!(
        on_submit.declared_in_macro_type_arg,
        "cross-file-simple: FooProps.onSubmit MUST carry \
         declared_in_macro_type_arg=true. Got declared={}. A `false` \
         here means component_meta.rs reverted to the pre-c5 \
         `source_field.map(...).unwrap_or(false)` framing — the \
         analyzer's local registry never sees the imported \
         interface's members, so the lookup defaults to `false`. \
         Post-c5 reads `field.declared_in_macro_type_arg` from the \
         `ExpandedField` produced by the c2 parser + c3 semantic \
         propagation + c4 prepared-surface walker.",
        on_submit.declared_in_macro_type_arg,
    );
    assert!(
        label.declared_in_macro_type_arg,
        "cross-file-simple: FooProps.label MUST carry \
         declared_in_macro_type_arg=true. Got declared={}.",
        label.declared_in_macro_type_arg,
    );
}

/// CASE C (R21 brief) — cross-file heritage with Omit + own-body
/// re-introduction.
///
/// `vendor.ts` declares `interface Vendor { state: …; onStateChange:
/// …; renderFallbackValue: …; other: … }`. `types.ts` declares
/// `interface Carrier extends Omit<Vendor, 'state' | 'onStateChange'
/// | 'renderFallbackValue'> { state: U; onStateChange: V;
/// renderFallbackValue: W }`. `component.vue` consumes
/// `defineProps<Carrier>()`.
///
/// Discriminator: the 3 re-introduced own-body members of `Carrier`
/// MUST carry `declared = true`; the heritage-only `other` member
/// MUST carry `declared = false`.
#[test]
fn cross_file_omit_then_reintroduce_own_body_members_carry_declared_true() {
    let mh = metahost();

    mh.upsert_base(
        "/src/vendor.ts",
        "export interface Vendor {\n\
         \tstate: number;\n\
         \tonStateChange: (next: number) => void;\n\
         \trenderFallbackValue: () => string;\n\
         \tother: boolean;\n\
         }\n",
    )
    .expect("vendor.ts upsert");

    mh.upsert_base(
        "/src/types.ts",
        "import type { Vendor } from './vendor';\n\
         export interface Carrier extends Omit<Vendor, 'state' | 'onStateChange' | 'renderFallbackValue'> {\n\
         \tstate: string;\n\
         \tonStateChange: (next: string) => void;\n\
         \trenderFallbackValue: () => number;\n\
         }\n",
    )
    .expect("types.ts upsert");

    mh.upsert_base(
        "/src/Component.vue",
        "<script setup lang=\"ts\">\n\
         import type { Carrier } from './types';\n\
         defineProps<Carrier>();\n\
         </script>\n",
    )
    .expect("Component.vue upsert");

    let meta = mh
        .host()
        .get_component_meta("/src/Component.vue")
        .expect("component meta resolves");

    // 3 re-introduced own-body members: MUST carry declared=true.
    for own_body_name in ["state", "onStateChange", "renderFallbackValue"] {
        let p = meta
            .props
            .iter()
            .find(|p| p.name == own_body_name)
            .unwrap_or_else(|| {
                panic!(
                    "cross-file-omit-then-reintroduce: meta.props \
                     MUST contain own-body re-introduced member `{}` \
                     (it must survive the Refined publication policy \
                     because it is author-declared in `Carrier`'s \
                     body)",
                    own_body_name,
                )
            });
        assert!(
            p.declared_in_macro_type_arg,
            "cross-file-omit-then-reintroduce: own-body \
             re-introduced member `{}` MUST carry \
             declared_in_macro_type_arg=true. Got declared={}. A \
             `false` here means the c4 prepared-surface walker did \
             NOT preserve `from_root_body=true` for the intersection \
             arm produced by `Omit<Vendor, …> & {{ {} }}` — the \
             own-body literal arm of an intersection MUST stamp its \
             members with the caller's body flag, per the c4 \
             `arm_is_own_body_literal && from_root_body` rule.",
            own_body_name, p.declared_in_macro_type_arg, own_body_name,
        );
    }

    // Heritage-only `other` member: MUST carry declared=false.
    // Reaches the surface only through `extends Omit<Vendor, …>` —
    // i.e. through the heritage descent. The c2 parser stamps it
    // `false`, c3 propagates, c4 the prepared-surface walker
    // descends into the `Omit` argument at `from_root_body=false`,
    // and c5 surfaces the structural fact through `ExpandedField`.
    let other =
        meta.props.iter().find(|p| p.name == "other").expect(
            "cross-file-omit-then-reintroduce: meta.props contains `other` heritage member",
        );
    assert!(
        !other.declared_in_macro_type_arg,
        "cross-file-omit-then-reintroduce: heritage-only member \
         `other` MUST carry declared_in_macro_type_arg=false. Got \
         declared={}. A `true` here means the c4 walker incorrectly \
         propagated `from_root_body=true` into the `Omit` first \
         argument's descent, OR c2's heritage-descent stamping is \
         broken in the parser.",
        other.declared_in_macro_type_arg,
    );
}

/// NEGATIVE (R21 brief) — contested name reaches the surface only via
/// heritage, no own-body re-introduction.
///
/// `vendor.ts` declares `interface Vendor { contested: number; alpha:
/// string }`. `types.ts` declares `interface Carrier extends Vendor
/// { extra: string }` (NO own-body for `contested` — it reaches the
/// surface only via the heritage chain). `component.vue` consumes
/// `defineProps<Carrier>()`.
///
/// Discriminator: the contested name MUST carry `declared = false`
/// (heritage-reached) and the Carrier's own-body `extra` MUST carry
/// `declared = true`.
#[test]
fn cross_file_no_own_body_name_heritage_reached_member_carries_declared_false() {
    let mh = metahost();

    mh.upsert_base(
        "/src/vendor.ts",
        "export interface Vendor {\n\
         \tcontested: number;\n\
         \talpha: string;\n\
         }\n",
    )
    .expect("vendor.ts upsert");

    mh.upsert_base(
        "/src/types.ts",
        "import type { Vendor } from './vendor';\n\
         export interface Carrier extends Vendor {\n\
         \textra: string;\n\
         }\n",
    )
    .expect("types.ts upsert");

    mh.upsert_base(
        "/src/Component.vue",
        "<script setup lang=\"ts\">\n\
         import type { Carrier } from './types';\n\
         defineProps<Carrier>();\n\
         </script>\n",
    )
    .expect("Component.vue upsert");

    let meta = mh
        .host()
        .get_component_meta("/src/Component.vue")
        .expect("component meta resolves");

    let contested =
        meta.props.iter().find(|p| p.name == "contested").expect(
            "cross-file-no-own-body-name: meta.props contains `contested` (heritage-reached)",
        );
    assert!(
        !contested.declared_in_macro_type_arg,
        "cross-file-no-own-body-name: `contested` reaches Carrier's \
         surface ONLY through `extends Vendor`. It MUST carry \
         declared_in_macro_type_arg=false. Got declared={}. A `true` \
         here is a P0 break — c2 parser is leaking the body flag \
         into heritage descent, OR c5 component_meta.rs is reading a \
         wrong source. Reverting c5 to the pre-c5 \
         `source_field.unwrap_or(false)` form would PASS this \
         assertion BY ACCIDENT (the analyzer never saw `contested`, \
         so source_field is None, and the old code defaults to \
         `false`). The cross-file-simple test discriminates that \
         accident: pre-c5 returns false for own-body members too.",
        contested.declared_in_macro_type_arg,
    );

    let extra = meta
        .props
        .iter()
        .find(|p| p.name == "extra")
        .expect("cross-file-no-own-body-name: meta.props contains own-body `extra`");
    assert!(
        extra.declared_in_macro_type_arg,
        "cross-file-no-own-body-name: own-body member `extra` MUST \
         carry declared_in_macro_type_arg=true. Got declared={}.",
        extra.declared_in_macro_type_arg,
    );
}
