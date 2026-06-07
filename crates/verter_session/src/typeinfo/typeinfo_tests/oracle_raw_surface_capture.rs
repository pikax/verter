//! Design item G guard: `raw_source_surface_captured_pre_lowering`.
//!
//! Proves the parse-time `RawSourceSurface` raw-fact inventory is captured
//! during the file's INITIAL PARSE, stored on the content-addressed artifact
//! (`IndexedReady.external_type_analysis`), carries the erased §Q2 facts the
//! lowered body lost, stamps the owning canonical, and is RECOMPUTED on a
//! content-hash change rather than served stale.

use std::sync::Arc;

use verter_compiler::utils::oxc::vue::raw_surface::{RawKey, RawMemberKind, SymbolSpace};

use crate::types::{FileKind, HostConfig, UpsertRequest};
use crate::VerterHost;

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source),
        file_kind: FileKind::from_path(canonical_id),
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
    let analysis = &indexed.external_type_analysis;
    let surface = analysis
        .raw_source_surface("Branded", SymbolSpace::Type)
        .expect("captured raw surface for Branded");

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
    let s1 = v1
        .external_type_analysis
        .raw_source_surface("Widget", SymbolSpace::Type)
        .expect("v1 surface")
        .clone();
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
    let s2 = v2
        .external_type_analysis
        .raw_source_surface("Widget", SymbolSpace::Type)
        .expect("v2 surface");
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
