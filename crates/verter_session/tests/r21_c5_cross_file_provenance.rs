//! Cross-file `declared_in_macro_type_arg` provenance characterizations.
//!
//! Each test exercises one of the reference cross-file shapes:
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
//!    own-body) — assert `declared = false`. The
//!    `cross_file_provenance_fixtures_tests` module adds the
//!    downstream `PublishedSurfacePolicy::Refined` strip assertion
//!    on a sibling fixture so the published-surface consequence is
//!    discriminating end-to-end.
//!
//! See `cross_file_provenance_fixtures_tests` for the fixture-driven
//! variants that exercise on-disk inputs and the Refined-policy
//! projection.

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

/// Positive characterization for the cross-file-simple shape.
///
/// `types.ts` declares `interface FooProps { onSubmit?: ...; label?:
/// string }`. `component.vue` imports and consumes via
/// `defineProps<FooProps>()`. Assert each own-body member surfaces
/// with `declared_in_macro_type_arg = true`.
///
/// Discrimination note (honest): this test exercises the resolver
/// stack end-to-end for the imported-own-body shape, but it does
/// NOT uniquely discriminate any single commit in the
/// `declared_in_macro_type_arg` chain — the upstream pipeline
/// (parser stamping, semantic propagation, prepared-surface walker)
/// already populates `field.declared_in_macro_type_arg` for own-body
/// imported members. A genuinely-discriminating test for the
/// cross-file consumer read at `component_meta.rs` lives in
/// `cross_file_provenance_fixtures_tests.rs`
/// (`fixture_cross_file_simple_own_body_members_survive_refined_projection`)
/// — that test asserts BOTH the structural fact AND the Refined
/// policy's downstream consequence (keeping `onSubmit` on the
/// published surface), which makes the consumer-read regression
/// observable through the audit projection.
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
         declared_in_macro_type_arg=true. Got declared={}. The \
         analyzer's local registry never sees the imported \
         interface's members, so the resolver pipeline must \
         supply `field.declared_in_macro_type_arg` from the \
         `ExpandedField` published by the parser + semantic \
         propagation + prepared-surface walker.",
        on_submit.declared_in_macro_type_arg,
    );
    assert!(
        label.declared_in_macro_type_arg,
        "cross-file-simple: FooProps.label MUST carry \
         declared_in_macro_type_arg=true. Got declared={}.",
        label.declared_in_macro_type_arg,
    );
}

/// Cross-file heritage with Omit + own-body re-introduction.
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

/// NEGATIVE — contested name reaches the surface only via heritage,
/// no own-body re-introduction.
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
