//! Architecture guard — no production cache-admission path may publish
//! a carrier that roots a value on a `RouteGeneration` dependency.
//!
//! `DepVersion::RouteGeneration` has no authoritative validating source:
//! there is no production emitter and no real route-generation counter.
//! A cache entry rooted on a `RouteGeneration` dependency could not
//! detect a content edit to the route-observed file — it would validate
//! as always-valid and serve stale indefinitely. Every signature builder
//! / admission helper that converts a `DepVersion` set therefore MUST
//! refuse a `RouteGeneration` entry — return `None`, route the value
//! through `ComputeAdmission::ReturnOnly`, or otherwise decline shared
//! admission — rather than silently dropping it (which would publish an
//! entry whose signature is missing the dependency entirely).
//!
//! This guard extracts each generation-handling producer's brace-balanced
//! body and asserts it both *mentions* `RouteGeneration` AND carries a
//! refusal token in the same body. A producer that stopped handling
//! `RouteGeneration` — or handled it by dropping it silently — flips this
//! guard RED. A self-test exercises the scanner against synthetic
//! refusing / non-refusing bodies so the scan cannot pass vacuously.

use std::fs;
use std::path::PathBuf;

/// Read a `verter_session` source file relative to `src/`.
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

/// Whether `body` carries a `RouteGeneration` refusal: it mentions the
/// `RouteGeneration` variant AND, in the same body, a refusal token
/// (`None` or `ReturnOnly`). A producer that converts a `DepVersion`
/// set without a refusal token has either stopped handling
/// `RouteGeneration` or silently drops it — both are violations.
fn body_refuses_route_generation(body: &str) -> bool {
    body.contains("RouteGeneration") && (body.contains("None") || body.contains("ReturnOnly"))
}

/// Every generation-handling producer that converts a `DepVersion` set
/// into a fact signature / cache admission MUST refuse `RouteGeneration`.
#[test]
fn no_admitted_carrier_roots_on_route_generation() {
    struct Producer {
        /// Source file relative to `src/`.
        file: &'static str,
        /// A substring that uniquely identifies the producer's
        /// signature line.
        signature: &'static str,
    }

    let producers = [
        Producer {
            file: "component_meta_materialize.rs",
            signature: "pub fn fact_signature_from_fence(",
        },
        Producer {
            file: "component_meta_materialize.rs",
            signature: "fn materialize_structure_read_set(",
        },
        Producer {
            file: "component_meta_caches.rs",
            signature: "pub(crate) fn ref_cycle_db_get_or_compute<C>(",
        },
        Producer {
            file: "semantic_query_memo/mod.rs",
            signature: "pub(crate) fn semantic_graph_read_set_signature(",
        },
        Producer {
            file: "resolver_core/component_meta_query_engine/mod.rs",
            signature: "pub(crate) fn engine_fact_signature_for_materialize_memo(",
        },
        Producer {
            file: "fact_signature_helpers.rs",
            signature: "pub(crate) fn dep_signature_to_fact_signature(",
        },
    ];

    for producer in producers {
        let src = read_session_source(producer.file);
        let body = extract_balanced_body(&src, producer.signature);
        assert!(
            body_refuses_route_generation(body),
            "`{}` in `{}` MUST refuse a `RouteGeneration` dependency (mention the variant \
             AND carry a `None` / `ReturnOnly` refusal in the same body) — route \
             generation has no authoritative validating source, so silently dropping it \
             would publish a cache entry whose signature cannot catch a content edit. \
             Body:\n{body}",
            producer.signature,
            producer.file,
        );
    }
}

/// Self-test: the scanner discriminates. A synthetic refusing body
/// passes; a synthetic body that drops `RouteGeneration` silently does
/// NOT. Without this, a scanner that matched nothing would pass
/// vacuously.
#[test]
fn route_generation_scanner_discriminates() {
    // A refusing body — mentions the variant and returns None.
    let refusing = "fn p() { match v { DepVersion::RouteGeneration(_) => return None, _ => {} } }";
    assert!(
        body_refuses_route_generation(refusing),
        "scanner self-test: a body that refuses RouteGeneration via `None` MUST pass",
    );

    // A `ReturnOnly`-refusing body.
    let return_only =
        "fn p() { if has_route_generation { return ComputeAdmission::ReturnOnly(v); } }";
    // This synthetic body does not literally contain `RouteGeneration`
    // — confirm the real refusal form (variant + token) is what counts.
    assert!(
        !body_refuses_route_generation(return_only),
        "scanner self-test: a body that mentions ReturnOnly but NOT the RouteGeneration \
         variant is not a recognised refusal",
    );
    let return_only_real =
        "fn p() { if v == DepVersion::RouteGeneration(0) { return ComputeAdmission::ReturnOnly(x); } }";
    assert!(
        body_refuses_route_generation(return_only_real),
        "scanner self-test: a body that refuses RouteGeneration via `ReturnOnly` MUST pass",
    );

    // A silently-dropping body — mentions the variant but has no
    // refusal token. This is the violation the guard catches.
    let dropping = "fn p() { match v { DepVersion::RouteGeneration(_) => {} _ => keep() } }";
    assert!(
        !body_refuses_route_generation(dropping),
        "scanner self-test: a body that mentions RouteGeneration but silently drops it \
         (no `None` / `ReturnOnly`) MUST fail — that is exactly the violation",
    );

    // Sanity: the scanned producers exist.
    assert!(
        read_session_source("component_meta_materialize.rs")
            .contains("pub fn fact_signature_from_fence("),
        "fact_signature_from_fence must be present in component_meta_materialize.rs",
    );
}
