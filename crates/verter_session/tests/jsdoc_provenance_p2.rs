//! Discriminating regressions for the JSDoc-provenance fixes P2-2 / P2-3 /
//! P2-4 (carried from Stage 1.5), driven through the component-meta LAZY
//! imported-macro-surface rail — the path that attaches a member's display
//! JSDoc (`ImportedMacroSurface::member_display_jsdoc`).
//!
//! # Why this path (and not `resolve_shallow_surface`)
//!
//! The typeinfo PUBLIC surface (`resolve_shallow_surface` →
//! `TypeInfoSurface::with_member_jsdoc_spans`) already locates each member's
//! JSDoc STRUCTURALLY — from the member's `declaration_origin` + its own
//! name-token offset — so it is immune to all three bugs. The component-meta
//! lazy rail (`ResolvedMacroSurface::LazyImported` → `prop_members` →
//! `member_display_jsdoc`) is the surface that still attributed JSDoc by the
//! member's VALUE-node origin + a file-wide / value-node-disambiguated textual
//! search, so the P2 bugs manifest HERE. These tests therefore drive
//! `lazy_prop_members` (via the test-only [`ImportedMacroSurfaceProbe`]) and
//! assert on the resulting `AnalyzedPropField::description`, which is the exact
//! JSDoc text `member_display_jsdoc` returns.
//!
//! Each test is DISCRIMINATING: it FAILS against the pre-fix tree (the
//! value-node-origin + `?`-only textual matcher) and PASSES post-fix:
//!
//! - **P2-2** — generic inherited member. `interface Base<T> { /** base doc */
//!   x: T }`; a derived interface instantiates `Base<string>`. After
//!   substitution the inherited `x`'s VALUE node points at `string` in the
//!   DERIVED file, so the pre-fix value-node-origin lookup read derived.ts and
//!   found NO JSDoc (`description == None`). Post-fix attributes JSDoc via the
//!   member's `declaration_origin` (= base.ts) → `Some("base doc")`.
//! - **P2-3** — duplicate-name same-value. Two declarations declare the same
//!   member name AND the same value type; their value nodes intern identically,
//!   so the pre-fix value-node disambiguation cannot tell them apart and
//!   collapses to ONE declaration's doc for BOTH. Querying the OTHER
//!   declaration returns the wrong doc. Post-fix anchors on the member's OWN
//!   declaration / name span → each declaration returns its own doc.
//! - **P2-4** — class definite-assignment field. The pre-fix textual matcher
//!   accepted `name` → `?` → `:`/`(` but NOT `!`, so `/** doc */ foo!: string`
//!   was missed (`description == None`). Post-fix attaches JSDoc structurally
//!   from the member's name-token offset (the `!` is AFTER the name) → the doc
//!   resolves.

#![allow(clippy::too_many_lines)]

use std::sync::Arc;

use verter_session::test_only::imported_macro_surface::ImportedMacroSurfaceProbe;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// Build a hermetic host with `files` injected into the workspace AND upserted
/// (parsed + shallow-indexed). A `.vue` path is upserted as an SFC, everything
/// else as a non-SFC TS/JS file.
fn build_host(files: &[(&'static str, &'static str)]) -> Arc<VerterHost> {
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

/// Resolve `type_name` in `canonical` through the LAZY imported-macro-surface
/// `prop_members` rail and return the `description` (JSDoc) attached to the
/// named member. Panics (loudly, with the observed member set) when the member
/// is absent — a silent `None` would mask a projection regression.
fn member_prop_description(
    host: &VerterHost,
    canonical: &str,
    type_name: &str,
    member: &str,
) -> Option<String> {
    let probe =
        ImportedMacroSurfaceProbe::new(Arc::from(canonical), Arc::from(type_name), [0u8; 16]);
    let props = probe.lazy_prop_members(host);
    let field = props.iter().find(|p| p.name == member).unwrap_or_else(|| {
        panic!(
            "member `{member}` must be projected onto `{type_name}`'s prop surface; got {:?}",
            props.iter().map(|p| p.name.clone()).collect::<Vec<_>>()
        )
    });
    field.description.clone()
}

// ---------------------------------------------------------------------------
// P2-2 — generic inherited-member JSDoc origin.
// ---------------------------------------------------------------------------

#[test]
fn p2_2_generic_inherited_member_jsdoc_resolves_to_base_declaration_file() {
    const BASE: &str = "/w/base.ts";
    const DERIVED: &str = "/w/derived.ts";
    // The base member `x` is GENERIC (`x: T`) and carries the JSDoc. The decoy
    // `/** derived doc */` sits on the DERIVED interface declaration and must
    // NOT be attached to `x` (which is declared only in base.ts).
    let base_src = "export interface Base<T> {\n  /** base doc */\n  x: T;\n}\n";
    let derived_src = "import type { Base } from './base';\n\
        /** derived doc */\n\
        export interface Derived extends Base<string> {\n  derivedOnly: number;\n}\n";

    let host = build_host(&[(BASE, base_src), (DERIVED, derived_src)]);

    // POSITIVE: the inherited generic member `x` carries its BASE-declared
    // JSDoc, attributed via `declaration_origin` (which survives the
    // `Base<string>` substitution) — NOT the substituted value-node origin
    // (which post-substitution points at `string` in derived.ts).
    let x_doc = member_prop_description(&host, DERIVED, "Derived", "x");
    assert_eq!(
        x_doc.as_deref(),
        Some("base doc"),
        "the generic inherited member `x`'s JSDoc must resolve from the BASE declaration file \
         (via declaration_origin). Pre-fix this was `None` because the substituted value node \
         pointed at `string` in the derived file."
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
// type (`string`), so their member value nodes intern identically. The pre-fix
// value-node disambiguation in `declaring_decl_span` therefore cannot tell the
// two declarations apart and collapses to ONE declaration's doc for BOTH:
// querying EITHER `Decoy` or `Real` returned `Real`'s `/** right */`. Querying
// `Decoy` is thus the discriminator — pre-fix returns the wrong `right`,
// post-fix returns `Decoy`'s own `wrong` (anchored on `Decoy.field`'s own
// declaration span).
// ---------------------------------------------------------------------------

#[test]
fn p2_3_duplicate_name_same_value_jsdoc_disambiguates_by_declaration_span() {
    const FILE: &str = "/w/dup.ts";
    let src = "export interface Decoy {\n  /** wrong */\n  field: string;\n}\n\
        export interface Real {\n  /** right */\n  field: string;\n}\n";

    let host = build_host(&[(FILE, src)]);

    // DISCRIMINATOR: querying `Decoy` must return ITS OWN doc (`wrong`). Pre-fix
    // the value-node collision returned `Real`'s `right` for `Decoy` too; only
    // the per-member declaration-span anchor reads `Decoy.field`'s own JSDoc.
    let decoy_doc = member_prop_description(&host, FILE, "Decoy", "field");
    assert_eq!(
        decoy_doc.as_deref(),
        Some("wrong"),
        "`Decoy.field`'s JSDoc must be its OWN `wrong` doc, disambiguated by its declaration \
         span. Pre-fix the same-name same-value collision returned `Real`'s `right` here."
    );
    assert_ne!(
        decoy_doc.as_deref(),
        Some("right"),
        "`Decoy.field` must NOT pick up `Real.field`'s `right` doc (the value-node-collision bug)"
    );

    // CONTROL: querying `Real` returns its own `right` doc (both pre- and
    // post-fix). Proves the fix did not merely swap which declaration wins.
    let real_doc = member_prop_description(&host, FILE, "Real", "field");
    assert_eq!(
        real_doc.as_deref(),
        Some("right"),
        "`Real.field`'s JSDoc must be its OWN `right` doc"
    );
}
