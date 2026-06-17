//! Guard: `raw_source_surface_captured_pre_lowering`.
//!
//! Proves the `RawSourceSurface` raw-fact record is captured from the RAW
//! statement syntax (pre-lowering) through the artifact's lazy
//! declaration-body memo (`DeclBodyMemo::raw_surfaces_for` — a per-symbol
//! DEMAND product of the retained parse snapshot, never an eager
//! whole-program inventory), carries the erased §Q2 facts the lowered body
//! lost, stamps the owning canonical, and is RECOMPUTED on a content-hash
//! change rather than served stale (the memo is content-addressed).

use std::sync::Arc;

use verter_compiler::utils::oxc::script::raw_surface::{RawKey, RawMemberKind, SymbolSpace};

use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

const CANONICAL: &str = "/fixtures/raw_surface_capture.ts";

#[test]
fn raw_source_surface_captured_pre_lowering() {
    let host = make_host();
    // A `unique symbol`-keyed brand member: OXC lowering silently ELIDES it
    // (`property_key_name` returns `None` for a computed key, `oxc/lib.rs:99`),
    // so the lowered body loses the fact. The parse-time capture must retain it.
    upsert(
        &host,
        CANONICAL,
        "declare const sym: unique symbol;\n\
         export type Branded = { plain: string; [sym]: number };\n",
    );

    let indexed = host.ensure_indexed_ready(CANONICAL).expect("indexed ready");
    let surfaces = indexed
        .shallow_state
        .decl_bodies()
        .raw_surfaces_for("Branded", SymbolSpace::Type);
    let surface = surfaces.first().expect("captured raw surface for Branded");

    // The owning canonical is stamped.
    assert_eq!(surface.decl_canonical, CANONICAL);

    // The computed `[sym]` member's RAW key survives as a NON-static key — the
    // exact fact the lowered body lost. The clean `plain` member is Static.
    assert!(
        surface
            .raw_member_keys
            .iter()
            .any(|k| !matches!(k, RawKey::Static(_))),
        "computed brand key retained pre-lowering: {:?}",
        surface.raw_member_keys
    );
    assert!(
        surface
            .raw_member_keys
            .iter()
            .any(|k| matches!(k, RawKey::Static(s) if s == "plain")),
        "the plain member is still captured as static"
    );
    assert!(surface
        .member_kinds
        .iter()
        .all(|k| matches!(k, RawMemberKind::Property)));
}

#[test]
fn raw_source_surface_recomputes_on_content_change() {
    let host = make_host();
    // Version 1: a private member (visibility erased by lowering, `oxc:427`).
    upsert(
        &host,
        CANONICAL,
        "export class Widget { private secret: number = 1; }\n",
    );
    let v1 = host.ensure_indexed_ready(CANONICAL).expect("v1 indexed");
    let v1_surfaces = v1
        .shallow_state
        .decl_bodies()
        .raw_surfaces_for("Widget", SymbolSpace::Type);
    let s1 = v1_surfaces.first().expect("v1 surface").clone();
    assert!(
        s1.member_visibility
            .iter()
            .any(|v| !matches!(v, verter_type_expr::MemberVisibility::Public)),
        "v1 captured the private member"
    );

    // Version 2: same canonical, DIFFERENT content (now public). The capture must
    // be recomputed from the new content, not served stale from v1.
    upsert(
        &host,
        CANONICAL,
        "export class Widget { public open: number = 1; }\n",
    );
    let v2 = host.ensure_indexed_ready(CANONICAL).expect("v2 indexed");
    assert_ne!(v1.whole_hash, v2.whole_hash, "content hash changed");
    let v2_surfaces = v2
        .shallow_state
        .decl_bodies()
        .raw_surfaces_for("Widget", SymbolSpace::Type);
    let s2 = v2_surfaces.first().expect("v2 surface");
    assert!(
        s2.member_visibility
            .iter()
            .all(|v| matches!(v, verter_type_expr::MemberVisibility::Public)),
        "v2 recomputed: no private member remains: {:?}",
        s2.member_visibility
    );
    assert!(
        s2.raw_member_keys
            .iter()
            .any(|k| matches!(k, RawKey::Static(s) if s == "open")),
        "v2 captured the new member name"
    );
}

#[test]
fn raw_surface_retains_all_merged_contributors() {
    // Two same-name `interface Merged` declarations MERGE in one file. They
    // share the SAME `(name, SymbolSpace::Type)` triple, so a single-value map
    // (the prior last-wins `insert`) would silently drop one contributor's raw
    // facts. The source-side walk MUST see EVERY contributor (§Q2: "a single
    // contributor being allowlist-clean does NOT admit the merge if another
    // contributor carries a REJECT construct"), so the capture retains an
    // ORDERED contributor vector keyed by the triple.
    //
    // Contributor 0 is allowlist-clean (a plain public property); contributor 1
    // carries a `unique symbol`-keyed member — an erased fact (`oxc/lib.rs:99,921`
    // silently drops the non-static key) the lowered body cannot represent. If
    // only one contributor survived storage, the clean one could win and the
    // brand key would vanish.
    let host = make_host();
    upsert(
        &host,
        CANONICAL,
        "declare const sym: unique symbol;\n\
         export interface Merged { clean: string }\n\
         export interface Merged { [sym]: number }\n",
    );

    let indexed = host.ensure_indexed_ready(CANONICAL).expect("indexed ready");
    let surfaces = indexed
        .shallow_state
        .decl_bodies()
        .raw_surfaces_for("Merged", SymbolSpace::Type);

    // BOTH contributors retained, in source order — a single-value map could
    // only ever yield one.
    assert_eq!(
        surfaces.len(),
        2,
        "both merged interface contributors retained: {surfaces:?}"
    );

    // Contributor 0 is the clean one (a plain `clean: string` property).
    assert!(
        surfaces[0]
            .raw_member_keys
            .iter()
            .any(|k| matches!(k, RawKey::Static(s) if s == "clean")),
        "contributor 0 is the clean property surface: {:?}",
        surfaces[0]
    );
    assert!(
        surfaces[0]
            .member_kinds
            .iter()
            .all(|k| matches!(k, RawMemberKind::Property)),
        "contributor 0 carries no accessor"
    );

    // Contributor 1's `unique symbol`-keyed member (the erased brand fact)
    // survives as a NON-static key ONLY because the second contributor was not
    // dropped.
    assert!(
        surfaces[1]
            .raw_member_keys
            .iter()
            .any(|k| !matches!(k, RawKey::Static(_))),
        "contributor 1's brand key retained pre-lowering: {:?}",
        surfaces[1]
    );
    assert!(
        !surfaces[1]
            .raw_member_keys
            .iter()
            .any(|k| matches!(k, RawKey::Static(s) if s == "clean")),
        "contributor 1 is a DISTINCT surface, not a copy of contributor 0"
    );
}
