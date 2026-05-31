//! Discriminating regressions for JSDoc-provenance scenarios P2-2 / P2-3 /
//! P2-4, driven end-to-end through the production component-meta path
//! (`defineProps<T>()` → `get_component_meta` → `prop.description`), which
//! attributes JSDoc via the shared typeinfo surface
//! (`TypeInfoSurface::with_member_jsdoc_spans`) — the SOLE post-cutover JSDoc
//! attribution path.
//!
//! The typeinfo surface locates each member's JSDoc STRUCTURALLY: from the
//! member's `declaration_origin` file plus its own name-token offset. The three
//! P2 scenarios that the retired component-meta lazy imported-macro-surface rail
//! got wrong (value-node-origin attribution + a `?`-only / value-node-collision
//! textual matcher) are immune on this path — these tests pin that the published
//! prop JSDoc is correct for each scenario:
//!
//! - **P2-2** — generic inherited member. `interface Base<T> { /** base doc */
//!   x: T }`; the consuming props type instantiates `Base<string>`. The
//!   inherited `x`'s JSDoc resolves from its `declaration_origin` (= base.ts),
//!   which survives the `Base<string>` substitution — NOT a substituted
//!   value-node origin (which would point at `string` in the consuming file).
//! - **P2-3** — duplicate-name same-value. Two declarations declare the same
//!   member name AND the same value type (so their member value nodes intern
//!   identically); each member's JSDoc is anchored on its OWN declaration /
//!   name span, so the consuming props surface gets the correct declaration's
//!   doc (no value-node collision).
//! - **P2-4** — class definite-assignment field `/** doc */ foo!: string`. The
//!   JSDoc attaches from the member's name-token offset (the `!` follows the
//!   name and does not block the leading-comment walk), so the doc resolves.

#![allow(clippy::too_many_lines)]

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// Build a hermetic host with `files` injected into the workspace AND upserted
/// (parsed + shallow-indexed). A `.vue` path is upserted as an SFC, everything
/// else as a non-SFC TS/JS file.
fn build_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (path, source) in files {
        workspace.inject_file((*path).into(), Arc::from(*source));
    }
    let ws: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws,
    ));
    for (path, source) in files {
        let file_kind = if path.ends_with(".vue") {
            FileKind::VueSfc
        } else {
            FileKind::NonSfc
        };
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some((*path).into()),
            input_id: (*path).into(),
            source: Arc::from(*source),
            file_kind,
            aliases: vec![],
        });
    }
    host
}

/// `<script setup>` SFC source that imports `type_name` from `import_path` and
/// applies it as `defineProps<TypeName>()`.
fn props_sfc(import_path: &str, type_name: &str) -> String {
    format!(
        "<script setup lang=\"ts\">\nimport type {{ {type_name} }} from '{import_path}'\ndefineProps<{type_name}>()\n</script>\n<template><div /></template>"
    )
}

/// Resolve the SFC at `sfc_path` (which does `defineProps<T>()`) through
/// `get_component_meta` and return the published JSDoc `description` of the
/// named prop. Panics (loudly, with the observed prop set) when the prop is
/// absent — a silent `None` would mask a projection regression.
fn published_prop_description(host: &VerterHost, sfc_path: &str, prop: &str) -> Option<String> {
    let meta = host
        .get_component_meta(sfc_path)
        .unwrap_or_else(|| panic!("`{sfc_path}` must produce component meta"));
    let p = meta
        .props
        .iter()
        .find(|p| p.name == prop)
        .unwrap_or_else(|| {
            panic!(
                "prop `{prop}` must be published; got {:?}",
                meta.props
                    .iter()
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>()
            )
        });
    p.description.clone()
}

// ---------------------------------------------------------------------------
// P2-2 — generic inherited-member JSDoc origin.
// ---------------------------------------------------------------------------

#[test]
fn p2_2_generic_inherited_member_jsdoc_resolves_to_base_declaration_file() {
    const BASE: &str = "/w/base.ts";
    const DERIVED: &str = "/w/derived.ts";
    const SFC: &str = "/w/P2_2.vue";
    // The base member `x` is GENERIC (`x: T`) and carries the JSDoc. The decoy
    // `/** derived doc */` sits on the DERIVED interface declaration and must
    // NOT be attached to `x` (which is declared only in base.ts).
    let base_src = "export interface Base<T> {\n  /** base doc */\n  x: T;\n}\n";
    let derived_src = "import type { Base } from './base';\n\
        /** derived doc */\n\
        export interface Derived extends Base<string> {\n  derivedOnly: number;\n}\n";

    let sfc_src = props_sfc("./derived", "Derived");
    let host = build_host(&[(BASE, base_src), (DERIVED, derived_src), (SFC, &sfc_src)]);

    // POSITIVE: the inherited generic member `x` carries its BASE-declared
    // JSDoc, attributed via the typeinfo surface member's `declaration_origin`
    // (= base.ts, which survives the `Base<string>` substitution) — NOT a
    // substituted value-node origin (which would point at `string` in
    // derived.ts).
    let x_doc = published_prop_description(&host, SFC, "x");
    assert_eq!(
        x_doc.as_deref(),
        Some("base doc"),
        "the generic inherited member `x`'s JSDoc must resolve from the BASE declaration file \
         via the typeinfo surface member's declaration_origin"
    );

    // NEGATIVE: the inherited member's JSDoc must NOT be the derived
    // declaration's decoy doc.
    assert_ne!(
        x_doc.as_deref(),
        Some("derived doc"),
        "the inherited member's JSDoc must not pick up the derived interface's decoy doc"
    );
}

// ---------------------------------------------------------------------------
// P2-3 — duplicate-name same-value JSDoc.
//
// `Decoy.field` and `Real.field` have the SAME member name AND the SAME value
// type (`string`), so their member value nodes intern identically. The typeinfo
// surface anchors each member's JSDoc on its OWN declaration / name span, so a
// `defineProps<Real>()` surface gets `Real.field`'s `right` doc — never
// `Decoy.field`'s `wrong` doc (the value-node-collision bug).
// ---------------------------------------------------------------------------

#[test]
fn p2_3_duplicate_name_same_value_jsdoc_disambiguates_by_declaration_span() {
    const FILE: &str = "/w/dup.ts";
    const SFC_REAL: &str = "/w/P2_3_real.vue";
    const SFC_DECOY: &str = "/w/P2_3_decoy.vue";
    let src = "export interface Decoy {\n  /** wrong */\n  field: string;\n}\n\
        export interface Real {\n  /** right */\n  field: string;\n}\n";

    let real_sfc = props_sfc("./dup", "Real");
    let decoy_sfc = props_sfc("./dup", "Decoy");
    let host = build_host(&[(FILE, src), (SFC_REAL, &real_sfc), (SFC_DECOY, &decoy_sfc)]);

    // DISCRIMINATOR: a `defineProps<Decoy>()` surface must get `Decoy.field`'s
    // OWN doc (`wrong`). A value-node-collision attribution would return
    // `Real`'s `right`; the typeinfo surface's per-member name-span anchor reads
    // `Decoy.field`'s own JSDoc.
    let decoy_doc = published_prop_description(&host, SFC_DECOY, "field");
    assert_eq!(
        decoy_doc.as_deref(),
        Some("wrong"),
        "`Decoy.field`'s JSDoc must be its OWN `wrong` doc, anchored on its own name span"
    );
    assert_ne!(
        decoy_doc.as_deref(),
        Some("right"),
        "`Decoy.field` must NOT pick up `Real.field`'s `right` doc (the value-node-collision bug)"
    );

    // CONTROL: a `defineProps<Real>()` surface gets `Real.field`'s own `right`
    // doc. Proves the attribution did not merely swap which declaration wins.
    let real_doc = published_prop_description(&host, SFC_REAL, "field");
    assert_eq!(
        real_doc.as_deref(),
        Some("right"),
        "`Real.field`'s JSDoc must be its OWN `right` doc"
    );
}

// ---------------------------------------------------------------------------
// P2-4 — class `/** doc */ foo!: string` definite-assignment field.
//
// The typeinfo surface attaches JSDoc from the member's name-token offset; the
// `!` follows the name and must not drop the member or block the leading-comment
// walk. (The discriminating `!:`-matcher guard for the `jsdoc.rs`
// expanded-prop / synthetic-member fallback lives in
// `verter_semantic::analysis::jsdoc::tests::
// extract_jsdoc_for_property_name_accepts_definite_assignment_field`.)
// ---------------------------------------------------------------------------

#[test]
fn p2_4_class_definite_assignment_field_reaches_surface_with_jsdoc() {
    const FILE: &str = "/w/definite.ts";
    const SFC: &str = "/w/P2_4.vue";
    // `foo!: string` is a documented definite-assignment field; `plain: number`
    // is a documented normal field (control); `bare!: boolean` is a
    // definite-assignment field with NO JSDoc (negative control).
    let src = "export class WithDefinite {\n  \
        /** the definite field */\n  \
        foo!: string;\n  \
        /** the plain field */\n  \
        plain: number;\n  \
        bare!: boolean;\n}\n";

    let sfc_src = props_sfc("./definite", "WithDefinite");
    let host = build_host(&[(FILE, src), (SFC, &sfc_src)]);

    // POSITIVE: the documented definite-assignment field `foo!: string` is
    // published WITH its JSDoc via the name-span attach (the `!` must not drop
    // the member or block the leading-comment walk).
    let foo_doc = published_prop_description(&host, SFC, "foo");
    assert_eq!(
        foo_doc.as_deref(),
        Some("the definite field"),
        "`foo!: string` must be published with its JSDoc via the name-span attach"
    );

    // CONTROL: the plain documented field still resolves (no regression).
    let plain_doc = published_prop_description(&host, SFC, "plain");
    assert_eq!(
        plain_doc.as_deref(),
        Some("the plain field"),
        "the plain field `plain: number` must still carry its JSDoc"
    );

    // NEGATIVE: an undocumented definite-assignment field carries no JSDoc (the
    // attach must not invent one for `bare`).
    let bare_doc = published_prop_description(&host, SFC, "bare");
    assert_eq!(
        bare_doc, None,
        "an undocumented definite-assignment field `bare!: boolean` must carry NO JSDoc"
    );
}
