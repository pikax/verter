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
        let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
        assert!(
            names.contains(&"msg") && names.contains(&"count"),
            "cross-file props must publish msg + count; got {names:?}"
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
        let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
        assert!(
            names.contains(&"base") && names.contains(&"fromAug"),
            "the augmented surface must publish both the base member and the \
             augmenter member; got {names:?}"
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
        let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
        assert!(
            names.contains(&"first") && names.contains(&"second"),
            "instantiated generic props must publish first + second; got {names:?}"
        );
        component_meta_envelope(Some(&meta))
    }
    fn assert_discriminating(&self, envelope: &OracleEnvelope) {
        assert_json_has(envelope, &["\"first\"", "\"second\""], self.id());
    }
}
