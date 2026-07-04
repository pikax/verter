//! The published-surface case adapters — one (or more) per
//! [`super::Stage10SurfaceClass`].
//!
//! Each adapter runs the REAL published-surface query on the given host
//! and returns the full-DTO canonical envelope. Typed anti-vacuity
//! assertions run inside [`PublishedSurfaceCase::run`] on the native DTO
//! (semantic marker values must be present), and
//! [`PublishedSurfaceCase::assert_discriminating`] re-checks the markers
//! on the canonical envelope — so a case where both legs return an empty
//! or degenerate surface FAILS instead of passing vacuously.

use super::envelope_tests::{component_meta_envelope, fallthrough_envelope, OracleEnvelope};
use super::Stage10SurfaceClass;
use crate::VerterHost;

/// One fixture file mounted on each leg's fresh host.
pub(crate) struct FixtureFile {
    pub(crate) path: &'static str,
    pub(crate) source: &'static str,
}

/// One published-surface parity case: fixture files, an entry canonical,
/// a published-surface run producing an [`OracleEnvelope`], and a typed
/// anti-vacuity assertion (so "both legs returned empty" fails loudly
/// instead of passing trivially).
pub(crate) trait PublishedSurfaceCase {
    fn id(&self) -> &'static str;
    fn class(&self) -> Stage10SurfaceClass;
    fn files(&self) -> &'static [FixtureFile];
    /// The canonical id the published-surface query targets.
    fn entry(&self) -> &'static str;
    fn run(&self, host: &VerterHost) -> OracleEnvelope;
    fn assert_discriminating(&self, envelope: &OracleEnvelope);
}

/// All registered parity cases, in class order.
pub(crate) fn all_cases() -> Vec<Box<dyn PublishedSurfaceCase>> {
    vec![
        Box::new(ComponentMetaPayloadCase),
        Box::new(FallthroughRootInheritanceCase),
        Box::new(MacroOwnBodyProvenanceCase),
        Box::new(HeritageShadowingCase),
        Box::new(AuthoredIntersectionCollisionCase),
        Box::new(OpenKeyDomainCarrierStopCase),
        Box::new(ModuleAugmentationSurfaceCase),
        Box::new(GenericSubstitutionCase),
    ]
}

fn assert_json_has(envelope: &OracleEnvelope, needles: &[&str], case: &str) {
    assert_eq!(
        envelope.outcome, "some",
        "{case}: published surface must resolve (anti-vacuity)"
    );
    for needle in needles {
        assert!(
            envelope.canonical_json.contains(needle),
            "{case}: canonical envelope must contain {needle:?} (anti-vacuity); got:\n{}",
            envelope.canonical_json
        );
    }
}

// ─── ComponentMetaPayload ────────────────────────────────────────────────

pub(crate) struct ComponentMetaPayloadCase;

impl PublishedSurfaceCase for ComponentMetaPayloadCase {
    fn id(&self) -> &'static str {
        "component_meta_payload_cross_file_props"
    }
    fn class(&self) -> Stage10SurfaceClass {
        Stage10SurfaceClass::ComponentMetaPayload
    }
    fn files(&self) -> &'static [FixtureFile] {
        &[
            FixtureFile {
                path: "/oracle/types.ts",
                source: "export interface CardProps { msg: string; count?: number }\n",
            },
            FixtureFile {
                path: "/oracle/App.vue",
                source: r#"<script setup lang="ts">
import type { CardProps } from './types'
defineProps<CardProps>()
defineEmits<{ update: [value: string] }>()
</script>
<template><div>x</div></template>
"#,
            },
        ]
    }
    fn entry(&self) -> &'static str {
        "/oracle/App.vue"
    }
    fn run(&self, host: &VerterHost) -> OracleEnvelope {
        let meta = host
            .get_component_meta(self.entry())
            .expect("component meta must resolve (anti-vacuity)");
        // Assert the cross-file member TYPES, not just the names: a resolver
        // that published the right names with the wrong types (a dropped decl
        // body, a swapped member) must fail here. `CardProps { msg: string;
        // count?: number }`.
        let msg = meta
            .props
            .iter()
            .find(|p| p.name == "msg")
            .expect("cross-file prop `msg` must publish");
        assert!(
            matches!(
                msg.type_expr,
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
            ),
            "`msg` must publish `string`; got {:?}",
            msg.type_expr
        );
        let count = meta
            .props
            .iter()
            .find(|p| p.name == "count")
            .expect("cross-file prop `count` must publish");
        assert!(
            matches!(
                count.type_expr,
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
            ),
            "`count` must publish `number`; got {:?}",
            count.type_expr
        );
        assert!(
            !count.required,
            "`count?` must publish as optional (required == false)"
        );
        assert!(
            meta.events.iter().any(|e| e.name == "update"),
            "declared emit `update` must publish"
        );
        component_meta_envelope(Some(&meta))
    }
    fn assert_discriminating(&self, envelope: &OracleEnvelope) {
        assert_json_has(envelope, &["\"msg\"", "\"count\"", "\"update\""], self.id());
    }
}

// ─── FallthroughRootInheritance ──────────────────────────────────────────

pub(crate) struct FallthroughRootInheritanceCase;

impl PublishedSurfaceCase for FallthroughRootInheritanceCase {
    fn id(&self) -> &'static str {
        "fallthrough_single_native_root"
    }
    fn class(&self) -> Stage10SurfaceClass {
        Stage10SurfaceClass::FallthroughRootInheritance
    }
    fn files(&self) -> &'static [FixtureFile] {
        &[
            FixtureFile {
                path: "/oracle/fall-props.ts",
                source: "export interface FallProps { label: string }\n",
            },
            FixtureFile {
                path: "/oracle/Fall.vue",
                source: r#"<script setup lang="ts">
import type { FallProps } from './fall-props'
defineProps<FallProps>()
</script>
<template><button type="button">{{ label }}</button></template>
"#,
            },
        ]
    }
    fn entry(&self) -> &'static str {
        "/oracle/Fall.vue"
    }
    fn run(&self, host: &VerterHost) -> OracleEnvelope {
        let resolution = host.resolve_fallthrough_surface(self.entry());
        let res = resolution
            .as_ref()
            .expect("fallthrough resolution must produce a surface (anti-vacuity)");
        assert!(
            res.accepted_props.iter().any(|p| p.name == "label"),
            "declared prop `label` must be on the accepted surface"
        );
        assert!(
            matches!(
                res.fallthrough_surface,
                verter_semantic::analysis::component_meta::FallthroughSurface::Branches { .. }
            ),
            "a single native button root must produce a branch-structured surface"
        );
        fallthrough_envelope(resolution.as_ref())
    }
    fn assert_discriminating(&self, envelope: &OracleEnvelope) {
        assert_json_has(envelope, &["\"label\"", "\"branches\""], self.id());
    }
}

// ─── MacroOwnBodyProvenance ──────────────────────────────────────────────

pub(crate) struct MacroOwnBodyProvenanceCase;

impl PublishedSurfaceCase for MacroOwnBodyProvenanceCase {
    fn id(&self) -> &'static str {
        "macro_own_body_provenance_intersection"
    }
    fn class(&self) -> Stage10SurfaceClass {
        Stage10SurfaceClass::MacroOwnBodyProvenance
    }
    fn files(&self) -> &'static [FixtureFile] {
        &[
            FixtureFile {
                path: "/oracle/base.ts",
                source: "export interface BaseProps { fromBase: number }\n",
            },
            FixtureFile {
                path: "/oracle/Own.vue",
                source: r#"<script setup lang="ts">
import type { BaseProps } from './base'
defineProps<BaseProps & { inline: string }>()
</script>
<template><div /></template>
"#,
            },
        ]
    }
    fn entry(&self) -> &'static str {
        "/oracle/Own.vue"
    }
    fn run(&self, host: &VerterHost) -> OracleEnvelope {
        let meta = host
            .get_component_meta(self.entry())
            .expect("component meta must resolve (anti-vacuity)");
        let inline = meta
            .props
            .iter()
            .find(|p| p.name == "inline")
            .expect("inline own-body prop must publish");
        assert!(
            meta.props.iter().any(|p| p.name == "fromBase"),
            "reference-arm prop must publish"
        );
        assert!(
            inline.declared_in_macro_type_arg,
            "an inline object-literal arm member is author-declared in the macro type argument"
        );
        component_meta_envelope(Some(&meta))
    }
    fn assert_discriminating(&self, envelope: &OracleEnvelope) {
        assert_json_has(
            envelope,
            &[
                "\"inline\"",
                "\"fromBase\"",
                "\"declared_in_macro_type_arg\":true",
            ],
            self.id(),
        );
    }
}

// ─── OpenKeyDomainCarrierStopL1 ──────────────────────────────────────────

pub(crate) struct OpenKeyDomainCarrierStopCase;

impl PublishedSurfaceCase for OpenKeyDomainCarrierStopCase {
    fn id(&self) -> &'static str {
        "open_pick_publishes_shallow_carrier"
    }
    fn class(&self) -> Stage10SurfaceClass {
        Stage10SurfaceClass::OpenKeyDomainCarrierStopL1
    }
    fn files(&self) -> &'static [FixtureFile] {
        &[
            FixtureFile {
                path: "/oracle/props-base.ts",
                source: "export interface PropsBase<T> { a: T; b: string }\n",
            },
            FixtureFile {
                path: "/oracle/Open.vue",
                source: r#"<script setup lang="ts" generic="T">
import type { PropsBase } from './props-base'
defineProps<{ cfg: Pick<PropsBase<T>, 'a'>; direct: T }>()
</script>
<template><div /></template>
"#,
            },
        ]
    }
    fn entry(&self) -> &'static str {
        "/oracle/Open.vue"
    }
    fn run(&self, host: &VerterHost) -> OracleEnvelope {
        let meta = host
            .get_component_meta(self.entry())
            .expect("component meta must resolve (anti-vacuity)");
        let cfg = meta
            .props
            .iter()
            .find(|p| p.name == "cfg")
            .expect("member-value open Pick prop must publish");
        let cfg_type = serde_json::to_value(&cfg.type_expr).expect("type serialises");
        assert!(
            cfg_type.to_string().contains("Pick"),
            "an OPEN Pick over the SFC generic must stay a shallow carrier on the \
             published member value (the utility head survives); got {cfg_type}"
        );
        component_meta_envelope(Some(&meta))
    }
    fn assert_discriminating(&self, envelope: &OracleEnvelope) {
        assert_json_has(envelope, &["Pick"], self.id());
    }
}

// ─── ModuleAugmentationSurface ───────────────────────────────────────────

pub(crate) struct ModuleAugmentationSurfaceCase;

impl PublishedSurfaceCase for ModuleAugmentationSurfaceCase {
    fn id(&self) -> &'static str {
        "module_augmentation_merged_props"
    }
    fn class(&self) -> Stage10SurfaceClass {
        Stage10SurfaceClass::ModuleAugmentationSurface
    }
    fn files(&self) -> &'static [FixtureFile] {
        &[
            FixtureFile {
                path: "/oracle/types.ts",
                source: "export interface AugTarget { base: string }\n",
            },
            FixtureFile {
                path: "/oracle/aug.ts",
                source: "import type { AugTarget } from './types'\n\
                         declare module './types' { interface AugTarget { fromAug: number } }\n\
                         export {}\n",
            },
            FixtureFile {
                path: "/oracle/Aug.vue",
                source: r#"<script setup lang="ts">
import type { AugTarget } from './types'
import './aug'
defineProps<AugTarget>()
</script>
<template><div /></template>
"#,
            },
        ]
    }
    fn entry(&self) -> &'static str {
        "/oracle/Aug.vue"
    }
    fn run(&self, host: &VerterHost) -> OracleEnvelope {
        let meta = host
            .get_component_meta(self.entry())
            .expect("component meta must resolve (anti-vacuity)");
        // Assert the merged member TYPES, not just the names: a stitch that
        // dropped the base body or mis-lowered the augmenter would publish the
        // right names with the wrong types. `AugTarget { base: string }` ∪
        // `declare module { AugTarget { fromAug: number } }`.
        let base = meta
            .props
            .iter()
            .find(|p| p.name == "base")
            .expect("the base member `base` must publish");
        assert!(
            matches!(
                base.type_expr,
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
            ),
            "`base` must publish `string`; got {:?}",
            base.type_expr
        );
        let from_aug = meta
            .props
            .iter()
            .find(|p| p.name == "fromAug")
            .expect("the augmenter member `fromAug` must publish");
        assert!(
            matches!(
                from_aug.type_expr,
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
            ),
            "`fromAug` must publish `number`; got {:?}",
            from_aug.type_expr
        );
        component_meta_envelope(Some(&meta))
    }
    fn assert_discriminating(&self, envelope: &OracleEnvelope) {
        assert_json_has(envelope, &["\"base\"", "\"fromAug\""], self.id());
    }
}

// ─── GenericSubstitution ─────────────────────────────────────────────────

pub(crate) struct GenericSubstitutionCase;

impl PublishedSurfaceCase for GenericSubstitutionCase {
    fn id(&self) -> &'static str {
        "generic_pair_substitution"
    }
    fn class(&self) -> Stage10SurfaceClass {
        Stage10SurfaceClass::GenericSubstitution
    }
    fn files(&self) -> &'static [FixtureFile] {
        &[
            FixtureFile {
                path: "/oracle/pair.ts",
                source: "export interface Pair<A, B> { first: A; second: B }\n",
            },
            FixtureFile {
                path: "/oracle/Gen.vue",
                source: r#"<script setup lang="ts">
import type { Pair } from './pair'
defineProps<Pair<string, number>>()
</script>
<template><div /></template>
"#,
            },
        ]
    }
    fn entry(&self) -> &'static str {
        "/oracle/Gen.vue"
    }
    fn run(&self, host: &VerterHost) -> OracleEnvelope {
        let meta = host
            .get_component_meta(self.entry())
            .expect("component meta must resolve (anti-vacuity)");
        // Substitution is SEMANTIC meaning: assert the instantiated member
        // TYPES (A := string, B := number), not just the member names — a
        // dropped/ swapped substitution env publishes the right names with
        // the wrong types and must fail here.
        let first = meta
            .props
            .iter()
            .find(|p| p.name == "first")
            .expect("instantiated generic prop `first` must publish");
        assert!(
            matches!(
                first.type_expr,
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
            ),
            "`first` must instantiate A := string; got {:?}",
            first.type_expr
        );
        let second = meta
            .props
            .iter()
            .find(|p| p.name == "second")
            .expect("instantiated generic prop `second` must publish");
        assert!(
            matches!(
                second.type_expr,
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
            ),
            "`second` must instantiate B := number; got {:?}",
            second.type_expr
        );
        component_meta_envelope(Some(&meta))
    }
    fn assert_discriminating(&self, envelope: &OracleEnvelope) {
        assert_json_has(
            envelope,
            &[
                "\"first\"",
                "\"second\"",
                // The instantiated member TYPES survive canonicalisation:
                // both substituted primitives must appear in the envelope.
                "{\"kind\":\"primitive\",\"name\":\"string\"}",
                "{\"kind\":\"primitive\",\"name\":\"number\"}",
            ],
            self.id(),
        );
    }
}

// ─── shape/view split: heritage shadowing ────────────────────────────────

/// A REAL `extends` heritage collision: the consuming declaration's
/// own-body `dup` must SHADOW the inherited `dup` on the published surface
/// (the derived member wins) — the inbound merge role is a projection-time
/// stamp, so the shadow decision must survive the shape/view split
/// byte-identically.
pub(crate) struct HeritageShadowingCase;

impl PublishedSurfaceCase for HeritageShadowingCase {
    fn id(&self) -> &'static str {
        "heritage_shadowing_own_body_wins"
    }
    fn class(&self) -> Stage10SurfaceClass {
        Stage10SurfaceClass::MacroOwnBodyProvenance
    }
    fn files(&self) -> &'static [FixtureFile] {
        &[
            FixtureFile {
                path: "/oracle/shadow-base.ts",
                source: "export interface ShadowBase { dup: number; fromBase: boolean }\n",
            },
            FixtureFile {
                path: "/oracle/Heritage.vue",
                source: r#"<script setup lang="ts">
import type { ShadowBase } from './shadow-base'
interface Props extends ShadowBase { dup: string }
defineProps<Props>()
</script>
<template><div /></template>
"#,
            },
        ]
    }
    fn entry(&self) -> &'static str {
        "/oracle/Heritage.vue"
    }
    fn run(&self, host: &VerterHost) -> OracleEnvelope {
        let meta = host
            .get_component_meta(self.entry())
            .expect("component meta must resolve (anti-vacuity)");
        let dup = meta
            .props
            .iter()
            .find(|p| p.name == "dup")
            .expect("colliding prop must publish");
        assert!(
            matches!(
                dup.type_expr,
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
            ),
            "the own-body `dup: string` must shadow the inherited `dup: number`; got {:?}",
            dup.type_expr
        );
        assert!(
            meta.props.iter().any(|p| p.name == "fromBase"),
            "non-colliding inherited members must still publish"
        );
        component_meta_envelope(Some(&meta))
    }
    fn assert_discriminating(&self, envelope: &OracleEnvelope) {
        assert_json_has(envelope, &["\"dup\"", "\"fromBase\""], self.id());
    }
}

// ─── shape/view split: authored-intersection collision ──────────────────

/// An AUTHORED intersection collision (`Base & { dup }`): authored arms
/// intersect — they must NOT apply the heritage shadow rule. The merge
/// role driving that decision is a projection-time stamp, so the published
/// surface must survive the shape/view split byte-identically.
pub(crate) struct AuthoredIntersectionCollisionCase;

impl PublishedSurfaceCase for AuthoredIntersectionCollisionCase {
    fn id(&self) -> &'static str {
        "authored_intersection_collision_intersects"
    }
    fn class(&self) -> Stage10SurfaceClass {
        Stage10SurfaceClass::MacroOwnBodyProvenance
    }
    fn files(&self) -> &'static [FixtureFile] {
        &[
            FixtureFile {
                path: "/oracle/collision-base.ts",
                source: "export interface CollisionBase { dup: number; fromBase: boolean }\n",
            },
            FixtureFile {
                path: "/oracle/Collision.vue",
                source: r#"<script setup lang="ts">
import type { CollisionBase } from './collision-base'
type Props = CollisionBase & { dup: string }
defineProps<Props>()
</script>
<template><div /></template>
"#,
            },
        ]
    }
    fn entry(&self) -> &'static str {
        "/oracle/Collision.vue"
    }
    fn run(&self, host: &VerterHost) -> OracleEnvelope {
        let meta = host
            .get_component_meta(self.entry())
            .expect("component meta must resolve (anti-vacuity)");
        // The AUTHORED-intersection collision result: the arms INTERSECT —
        // `dup` publishes as `number & string`, NOT the heritage-shadow
        // outcome (a bare own-arm `string`) and NOT a first-arm `number`.
        // This is the concrete shape that distinguishes the authored rule
        // from heritage shadowing.
        let dup = meta
            .props
            .iter()
            .find(|p| p.name == "dup")
            .expect("colliding authored-intersection prop must publish");
        match &dup.type_expr {
            verter_type_expr::TypeExpr::Intersection(arms) => {
                assert_eq!(
                    arms.len(),
                    2,
                    "authored-intersection `dup` must intersect exactly the \
                     two colliding member types; got {arms:?}"
                );
                for prim in [
                    verter_type_expr::PrimitiveName::Number,
                    verter_type_expr::PrimitiveName::String,
                ] {
                    assert!(
                        arms.contains(&verter_type_expr::TypeExpr::Primitive(prim)),
                        "authored-intersection `dup` must carry the {prim:?} \
                         arm; got {arms:?}"
                    );
                }
            }
            other => panic!(
                "authored-intersection collision must publish `dup` as the \
                 member-type intersection (never the heritage-shadow single \
                 arm); got {other:?}"
            ),
        }
        let from_base = meta
            .props
            .iter()
            .find(|p| p.name == "fromBase")
            .expect("non-colliding arm members must publish");
        assert!(
            matches!(
                from_base.type_expr,
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Boolean)
            ),
            "`fromBase` must keep its authored `boolean`; got {:?}",
            from_base.type_expr
        );
        component_meta_envelope(Some(&meta))
    }
    fn assert_discriminating(&self, envelope: &OracleEnvelope) {
        assert_json_has(
            envelope,
            &[
                "\"dup\"",
                "\"fromBase\"",
                // The collision result's concrete shape survives
                // canonicalisation: an intersection carrying both primitive
                // arms, plus the non-colliding boolean member.
                "\"kind\":\"intersection\"",
                "{\"kind\":\"primitive\",\"name\":\"number\"}",
                "{\"kind\":\"primitive\",\"name\":\"string\"}",
                "{\"kind\":\"primitive\",\"name\":\"boolean\"}",
            ],
            self.id(),
        );
    }
}
