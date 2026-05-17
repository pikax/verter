//! Architecture guard — the structural-carrier signature producers and
//! the route-fact production helpers must NOT build a fact / currentness
//! oracle from the permissive `FileArtifactStore::get_any` /
//! `get_artifacts_any`.
//!
//! `indexed().get_any(canonical)` returns ANY cached `IndexedReady`
//! candidate for a canonical — including a stale candidate for an older
//! content hash. Deriving a `FileWholeHash` self-root, a route-surface
//! hash, or any currentness decision from a `get_any` result bakes a
//! possibly-stale content hash into a cache entry's signature, so fact
//! validation would later confirm a stale entry as valid. The
//! structural-carrier producers and route-fact helpers must instead
//! observe identity through the content-pinned accessors
//! (`get_for_current_content` / `indexed().get(canonical, hash)` /
//! `observe_materialize_scope` / `parse_fact_ref_for_observed_current_content`)
//! or the `current_route_surface_hash` helper.
//!
//! This guard extracts each producer/helper's brace-balanced body and
//! asserts it calls NONE of the banned permissive-read tokens. A
//! self-test exercises the scanner against synthetic violating / clean
//! bodies so the scan cannot pass vacuously.
//!
//! Scope note: a bare `get_any` on the `route_owned_shallow()` DB (the
//! `RouteOwnedShallowDb`, which keeps exactly one entry per canonical,
//! replaced on every publish) is NOT banned — `current_route_surface_hash`
//! uses it as the documented fallback for a scheduler-invisible
//! route-only file. The banned tokens below are scoped to the
//! `indexed()` artifact store (`indexed().get_any`) and the
//! `get_artifacts_any` raw accessor, which are the permissive
//! multi-candidate reads.

use std::fs;
use std::path::PathBuf;

fn read_session_source(relative: &str) -> String {
    let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(cargo_manifest_dir);
    path.push("src");
    path.push(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

/// Extract the brace-balanced body (including the outer `{ }`) of the
/// first occurrence of `needle`.
fn extract_balanced_body<'a>(src: &'a str, needle: &str) -> &'a str {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in source"));
    let after = &src[start..];
    let open = after
        .find('{')
        .unwrap_or_else(|| panic!("expected an opening brace after `{needle}`"));
    let bytes = after.as_bytes();
    let mut depth = 0usize;
    let mut idx = open;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &after[open..=idx];
                }
            }
            _ => {}
        }
        idx += 1;
    }
    panic!("expected a brace-balanced body for `{needle}`");
}

/// Tokens that, inside a structural-carrier / route-fact producer body,
/// are a permissive multi-candidate read used as a fact / currentness
/// oracle. `indexed().get_any` is the `FileArtifactStore` permissive
/// read; `get_artifacts_any` is its raw-key sibling.
const BANNED: &[&str] = &["indexed().get_any", "get_artifacts_any"];

/// Each D-owned producer / route-fact helper body MUST be free of the
/// banned permissive reads.
#[test]
fn structural_carrier_producers_use_no_permissive_get_any() {
    struct Scanned {
        file: &'static str,
        signature: &'static str,
    }

    let scanned = [
        Scanned {
            file: "component_meta_materialize.rs",
            signature: "fn materialize_structure_read_set(",
        },
        Scanned {
            file: "component_meta_materialize.rs",
            signature: "fn merge_traced_facts_into_materialize_carrier(",
        },
        Scanned {
            file: "component_meta_materialize.rs",
            signature: "fn finish_materialize_admission(",
        },
        Scanned {
            file: "component_meta_materialize.rs",
            signature: "fn base_node_origin_self_root(",
        },
        Scanned {
            file: "component_meta_caches.rs",
            signature: "fn ref_cycle_read_set(",
        },
        Scanned {
            file: "host_resolve/route_owned_shallow.rs",
            signature: "pub(crate) fn current_route_surface_hash(",
        },
        Scanned {
            file: "host_resolve/frontier_engine.rs",
            signature: "fn append_route_participant_fact_versions(",
        },
    ];

    for item in scanned {
        let src = read_session_source(item.file);
        let body = extract_balanced_body(&src, item.signature);
        for banned in BANNED {
            assert!(
                !body.contains(banned),
                "`{}` in `{}` MUST NOT call the permissive `{banned}` — a permissive \
                 multi-candidate read can surface a stale `IndexedReady` candidate, \
                 baking a stale content hash into a fact signature / route-fact oracle. \
                 Observe identity through a content-pinned accessor \
                 (`get_for_current_content` / `indexed().get(canonical, hash)` / \
                 `observe_materialize_scope`) or `current_route_surface_hash` instead. \
                 Body:\n{body}",
                item.signature,
                item.file,
            );
        }
    }
}

/// Self-test: the scanner discriminates. A synthetic body containing a
/// banned token MUST be flagged; a clean body MUST NOT.
#[test]
fn get_any_scanner_discriminates() {
    let clean = "fn p() { let h = ctx.indexed_for_current_content(c).whole_hash; h }";
    for banned in BANNED {
        assert!(
            !clean.contains(banned),
            "scanner self-test: a clean content-pinned body must contain no banned token",
        );
    }

    // A `route_owned_shallow().get_any(...)` call is NOT banned — the
    // banned token is `indexed().get_any`, scoped to the artifact store.
    let route_owned_get_any = "let e = self.route_owned_shallow().get_any(canonical);";
    for banned in BANNED {
        assert!(
            !route_owned_get_any.contains(banned),
            "scanner self-test: `route_owned_shallow().get_any` must NOT be flagged — \
             only the `indexed()` artifact-store permissive read is banned",
        );
    }

    // A planted violation: an `indexed().get_any` read.
    let violating = "fn p() { let f = self.project_type_store.indexed().get_any(c); f }";
    assert!(
        violating.contains("indexed().get_any"),
        "scanner self-test: an `indexed().get_any` read MUST be detected — if not, the \
         production guard above passes vacuously",
    );
    let violating_artifacts = "fn p() { let f = store.get_artifacts_any(&key); f }";
    assert!(
        violating_artifacts.contains("get_artifacts_any"),
        "scanner self-test: a `get_artifacts_any` read MUST be detected",
    );

    // Sanity: the scanned producers exist.
    assert!(
        read_session_source("component_meta_materialize.rs")
            .contains("fn materialize_structure_read_set("),
        "materialize_structure_read_set must be present",
    );
    assert!(
        read_session_source("host_resolve/route_owned_shallow.rs")
            .contains("pub(crate) fn current_route_surface_hash("),
        "current_route_surface_hash must be present",
    );
}
