//! LIVE discriminating guards for the source-side walk
//! (`resolve_source_declarations`). Each builds a real `VerterHost` over
//! controlled in-memory fixtures, binds a `SourceLocator` through the SHARED
//! resolver, and asserts the resulting `SourceWalkResult` + the two-sided
//! admission verdict over it. Every guard pairs a POSITIVE control (a clean
//! source ADMITs) with the discriminating NEGATIVE (the named REJECT
//! construct / cycle / unresolved hop), so a regression that flattened the walk
//! to "always Resolved+clean" would fail.

use std::sync::Arc;

use verter_compiler::utils::oxc::vue::raw_surface::SymbolSpace;

use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
use crate::types::{FileKind, HostConfig, UpsertRequest};
use crate::VerterHost;

use super::super::admission::{admit_source_contributor, admit_source_walk, RejectReason};
use super::super::admission::{AdmissionVerdict, SourceWalkResult};
use super::{resolve_source_declarations, SourceLocator};

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

/// Build a request-bound `HostResolverContext` (the same construction
/// `support::shallow_surface_expr` uses) and run the source walk over it.
fn walk(host: &VerterHost, locator: &SourceLocator) -> SourceWalkResult {
    let store_view = host.resolver_store_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(host, &store_view, overlay);
    resolve_source_declarations(&ctx, locator)
}

fn type_locator(canonical: &str, name: &str) -> SourceLocator {
    SourceLocator {
        reference_canonical: canonical.to_string(),
        reference_name: name.to_string(),
        symbol_space: SymbolSpace::Type,
    }
}

fn value_locator(canonical: &str, name: &str) -> SourceLocator {
    SourceLocator {
        reference_canonical: canonical.to_string(),
        reference_name: name.to_string(),
        symbol_space: SymbolSpace::Value,
    }
}

/// A single clean type alias binds to EXACTLY one defining contributor, and the
/// two-sided source admission ADMITs it.
#[test]
fn source_is_provably_single_contributor() {
    let host = make_host();
    let canonical = "/fixtures/single.ts";
    upsert(
        &host,
        canonical,
        "export type Clean = { a: string; b: number };\n",
    );
    host.ensure_indexed_ready(canonical).expect("indexed");

    let result = walk(&host, &type_locator(canonical, "Clean"));

    match &result {
        SourceWalkResult::Resolved { contributors } => {
            assert_eq!(
                contributors.len(),
                1,
                "a single non-merged alias is exactly one contributor"
            );
            assert_eq!(contributors[0].ordinal, 0);
            assert_eq!(contributors[0].raw_surface.decl_canonical, canonical);
        }
        other => panic!("expected Resolved single contributor, got {other:?}"),
    }
    assert_eq!(
        admit_source_walk(&result),
        AdmissionVerdict::Admit,
        "a clean public-property surface is admitted"
    );
}

/// The two-sided allowlist over the resolved source: a clean source ADMITs; an
/// erased-fact (`unique symbol` brand key) source REJECTS with the exact reason
/// — even though its lowered body lost the fact.
#[test]
fn source_declaration_allowlist_clean() {
    let host = make_host();
    let canonical = "/fixtures/allowlist.ts";
    upsert(
        &host,
        canonical,
        "export type Clean = { id: string; label: string };\n\
         declare const brand: unique symbol;\n\
         export type Branded = { id: string; [brand]: true };\n",
    );
    host.ensure_indexed_ready(canonical).expect("indexed");

    // Positive control: the clean alias is admitted.
    let clean = walk(&host, &type_locator(canonical, "Clean"));
    assert_eq!(admit_source_walk(&clean), AdmissionVerdict::Admit);

    // The branded alias resolves (it IS a defining declaration) ...
    let branded = walk(&host, &type_locator(canonical, "Branded"));
    assert!(
        matches!(branded, SourceWalkResult::Resolved { .. }),
        "the branded source binds to a defining decl: {branded:?}"
    );
    // ... but the two-sided walk REJECTS it on the erased `unique symbol`/
    // non-static-key fact the lowered body could not carry.
    assert!(
        matches!(
            admit_source_walk(&branded),
            AdmissionVerdict::Reject(RejectReason::UniqueSymbol | RejectReason::NonStaticKey)
        ),
        "branded source rejected on its erased brand fact: {:?}",
        admit_source_walk(&branded)
    );
}

/// A class with a `private` member resolves to its Type-space contributor, and
/// the source walk REJECTS on the declared visibility — the fact OXC lowering
/// stamps public (`oxc/lib.rs:427`), invisible in the lowered body.
#[test]
fn source_walk_rejects_private_class_member() {
    let host = make_host();
    let canonical = "/fixtures/visibility.ts";
    upsert(
        &host,
        canonical,
        "export class Open { pub: number = 1; }\n\
         export class Hidden { private secret: number = 1; }\n",
    );
    host.ensure_indexed_ready(canonical).expect("indexed");

    let open = walk(&host, &type_locator(canonical, "Open"));
    assert_eq!(admit_source_walk(&open), AdmissionVerdict::Admit);

    let hidden = walk(&host, &type_locator(canonical, "Hidden"));
    assert_eq!(
        admit_source_walk(&hidden),
        AdmissionVerdict::Reject(RejectReason::NonPublicVisibility),
        "private member rejected on declared visibility: {hidden:?}"
    );
}

/// The walk follows the import graph: a consumer's `import { X } from "./barrel"`
/// → barrel's `export { X } from "./leaf"` → the leaf's defining declaration.
/// The resolved contributor's `decl_canonical` is the LEAF, not the bare
/// import/reexport node.
#[test]
fn source_walk_resolves_import_chain() {
    let host = make_host();
    let leaf = "/fixtures/chain-leaf.ts";
    let barrel = "/fixtures/chain-barrel.ts";
    let consumer = "/fixtures/chain-consumer.ts";
    upsert(&host, leaf, "export type Leaf = { x: string };\n");
    upsert(&host, barrel, "export { Leaf } from './chain-leaf';\n");
    upsert(
        &host,
        consumer,
        "import { Leaf } from './chain-barrel';\nexport type Use = Leaf;\n",
    );
    host.ensure_indexed_ready(leaf).expect("leaf indexed");
    host.ensure_indexed_ready(barrel).expect("barrel indexed");
    host.ensure_indexed_ready(consumer)
        .expect("consumer indexed");

    let result = walk(&host, &type_locator(consumer, "Leaf"));
    match &result {
        SourceWalkResult::Resolved { contributors } => {
            assert_eq!(contributors.len(), 1);
            assert_eq!(
                contributors[0].raw_surface.decl_canonical, leaf,
                "the walk followed import -> reexport -> the LEAF defining decl, \
                 not the bare import/reexport node"
            );
        }
        other => panic!("expected Resolved from the leaf, got {other:?}"),
    }
    assert_eq!(admit_source_walk(&result), AdmissionVerdict::Admit);
}

/// An import that does not bind to any defining declaration in the controlled
/// fixture set is Unresolved — REJECT (the generator never admits a capture
/// whose real source it could not reach).
#[test]
fn source_walk_unbound_import_is_unresolved() {
    let host = make_host();
    let consumer = "/fixtures/dangling.ts";
    // Imports from a module that was never upserted into the fixture set.
    upsert(
        &host,
        consumer,
        "import { Missing } from './nonexistent';\nexport type Use = Missing;\n",
    );
    host.ensure_indexed_ready(consumer).expect("indexed");

    let result = walk(&host, &type_locator(consumer, "Missing"));
    assert!(
        matches!(result, SourceWalkResult::Unresolved),
        "an unbound import is Unresolved: {result:?}"
    );
    assert_eq!(
        admit_source_walk(&result),
        AdmissionVerdict::Reject(RejectReason::SourceUnresolvedOrCyclic)
    );
}

/// The walk is TRANSITIVE through `typeof`: a value annotated `typeof dirty`
/// resolves its OWN clean contributor AND re-enters the shared resolver for the
/// `typeof` referent, whose `as const` provenance REJECTS the query — even
/// though the referencing value is itself allowlist-clean.
#[test]
fn source_walk_is_transitive_through_typeof() {
    let host = make_host();
    let canonical = "/fixtures/typeof_transitive.ts";
    upsert(
        &host,
        canonical,
        "export const cleanBase = 1;\n\
         export const cleanRef: typeof cleanBase = 1;\n\
         export const dirtyBase = { a: 1 } as const;\n\
         export const dirtyRef: typeof dirtyBase = dirtyBase;\n",
    );
    host.ensure_indexed_ready(canonical).expect("indexed");

    // Positive control: a clean typeof chain admits, and IS transitive (it
    // resolves both the referencing value and the referent — 2 contributors).
    let clean = walk(&host, &value_locator(canonical, "cleanRef"));
    match &clean {
        SourceWalkResult::Resolved { contributors } => {
            assert_eq!(
                contributors.len(),
                2,
                "the walk reached the typeof referent transitively: {contributors:?}"
            );
        }
        other => panic!("expected Resolved (ref + referent), got {other:?}"),
    }
    assert_eq!(admit_source_walk(&clean), AdmissionVerdict::Admit);

    // The dirty chain: the REFERENCING value `dirtyRef` is itself clean, but the
    // transitively-reached `dirtyBase` carries an `as const` provenance fact that
    // REJECTS the query. This proves the walk is transitive (the reject comes
    // from the referent, not the reference site).
    let dirty = walk(&host, &value_locator(canonical, "dirtyRef"));
    let contributors = match &dirty {
        SourceWalkResult::Resolved { contributors } => contributors,
        other => panic!("expected Resolved (ref + referent), got {other:?}"),
    };
    assert_eq!(contributors.len(), 2, "ref + transitively-reached referent");
    // The reference site (ordinal 0) is itself clean ...
    assert_eq!(
        admit_source_contributor(&contributors[0]),
        AdmissionVerdict::Admit,
        "the referencing value is allowlist-clean on its own"
    );
    // ... but the whole walk REJECTS because the transitive referent is not.
    assert_eq!(
        admit_source_walk(&dirty),
        AdmissionVerdict::Reject(RejectReason::ConstAssertion),
        "the transitively-reached `as const` referent rejects the query"
    );
}

/// A cyclic `typeof` chain (`x: typeof y`, `y: typeof x`) terminates as Cycle
/// under the visited-set guard and REJECTS — never hangs, never best-effort
/// admits.
#[test]
fn source_walk_cycle_rejected() {
    let host = make_host();
    let canonical = "/fixtures/typeof_cycle.ts";
    upsert(
        &host,
        canonical,
        "export declare const x: typeof y;\n\
         export declare const y: typeof x;\n",
    );
    host.ensure_indexed_ready(canonical).expect("indexed");

    let result = walk(&host, &value_locator(canonical, "x"));
    assert!(
        matches!(result, SourceWalkResult::Cycle),
        "a typeof cycle terminates as Cycle: {result:?}"
    );
    assert_eq!(
        admit_source_walk(&result),
        AdmissionVerdict::Reject(RejectReason::SourceUnresolvedOrCyclic)
    );
}

// ---------------------------------------------------------------------------
// Source-ROOT carve-out — the LIVE stamping → admission path. These drive the
// REAL `resolve_source_declarations` walk (which stamps
// `SourceContributor::carve_out_root_def` by resolving the operator root through
// the shared resolver) and assert the two-sided admission over the result. They
// discriminate against a broken live stamp: an imported / cross-file root that
// the walk wrongly stamped same-file (or a structure-only carve-out that never
// resolved the root) would ADMIT here instead of REJECT. The hand-built
// `admit_source_root` unit guards (in `admission/tests.rs`) cover the admission
// CHECK in isolation; these cover the live STAMPING that feeds it.
// ---------------------------------------------------------------------------

/// Same-file `keyof Root` — the `keyof` root is declared in the SAME fixture, so
/// the live walk resolves it back to this file and stamps it same-file. The
/// carve-out fires and the contributor ADMITs.
#[test]
fn source_root_carve_out_admits_same_file_keyof() {
    let host = make_host();
    let canonical = "/fixtures/carveout_keyof_same.ts";
    upsert(
        &host,
        canonical,
        "export type KeySource = { a: string; b: number };\n\
         export type Keys = keyof KeySource;\n",
    );
    host.ensure_indexed_ready(canonical).expect("indexed");

    let result = walk(&host, &type_locator(canonical, "Keys"));
    let contributors = match &result {
        SourceWalkResult::Resolved { contributors } => contributors,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(contributors.len(), 1, "a single alias is one contributor");
    assert_eq!(contributors[0].def_canonical, canonical);
    // The live walk resolved the `keyof` root `KeySource` through the shared
    // resolver and stamped it as defined in THIS file.
    assert_eq!(
        contributors[0].carve_out_root_def.as_deref(),
        Some(canonical),
        "the live stamp must resolve the keyof root same-file"
    );
    assert_eq!(
        admit_source_walk(&result),
        AdmissionVerdict::Admit,
        "a same-file keyof carve-out is admitted"
    );
}

/// Same-file `Root["a"]["b"]` — the indexed-access root is declared in the SAME
/// fixture, so the live walk stamps it same-file and the carve-out ADMITs.
#[test]
fn source_root_carve_out_admits_same_file_indexed_access() {
    let host = make_host();
    let canonical = "/fixtures/carveout_indexed_same.ts";
    upsert(
        &host,
        canonical,
        "export type KeySource = { nested: { value: string } };\n\
         export type Pick = KeySource[\"nested\"][\"value\"];\n",
    );
    host.ensure_indexed_ready(canonical).expect("indexed");

    let result = walk(&host, &type_locator(canonical, "Pick"));
    let contributors = match &result {
        SourceWalkResult::Resolved { contributors } => contributors,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(contributors.len(), 1);
    assert_eq!(contributors[0].def_canonical, canonical);
    assert_eq!(
        contributors[0].carve_out_root_def.as_deref(),
        Some(canonical),
        "the live stamp must resolve the indexed-access root same-file"
    );
    assert_eq!(
        admit_source_walk(&result),
        AdmissionVerdict::Admit,
        "a same-file indexed-access carve-out is admitted"
    );
}

/// Imported `keyof Imported` — the `keyof` root is declared in ANOTHER file and
/// reached through an import edge. The live walk resolves the root to the LEAF,
/// so the stamp is cross-file (NOT same-file): the carve-out does not fire and
/// the bare `keyof` REJECTS. This is the discriminating case: a live stamp that
/// wrongly marked every root same-file would ADMIT here.
#[test]
fn source_root_carve_out_rejects_imported_keyof_root() {
    let host = make_host();
    let leaf = "/fixtures/carveout-keyof-leaf.ts";
    let consumer = "/fixtures/carveout-keyof-consumer.ts";
    upsert(
        &host,
        leaf,
        "export type Imported = { a: string; b: number };\n",
    );
    upsert(
        &host,
        consumer,
        "import { Imported } from './carveout-keyof-leaf';\n\
         export type Keys = keyof Imported;\n",
    );
    host.ensure_indexed_ready(leaf).expect("leaf indexed");
    host.ensure_indexed_ready(consumer)
        .expect("consumer indexed");

    let result = walk(&host, &type_locator(consumer, "Keys"));
    let contributors = match &result {
        SourceWalkResult::Resolved { contributors } => contributors,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(contributors.len(), 1);
    // `Keys` is defined in the consumer ...
    assert_eq!(contributors[0].def_canonical, consumer);
    // ... but the live walk followed the import edge and stamped the `keyof`
    // root as resolving to the LEAF — a cross-file root, not same-file.
    assert_eq!(
        contributors[0].carve_out_root_def.as_deref(),
        Some(leaf),
        "the live stamp must resolve the imported keyof root cross-file"
    );
    // So the carve-out does not fire; the bare `keyof` rejects through the
    // generic predicate.
    assert_eq!(
        admit_source_walk(&result),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof")),
        "an imported keyof root is not a same-file carve-out"
    );
}

/// Imported `Imported["x"]` — the indexed-access root is declared in ANOTHER
/// file. The live walk stamps it cross-file and the bare indexed access REJECTS.
#[test]
fn source_root_carve_out_rejects_imported_indexed_access_root() {
    let host = make_host();
    let leaf = "/fixtures/carveout-indexed-leaf.ts";
    let consumer = "/fixtures/carveout-indexed-consumer.ts";
    upsert(&host, leaf, "export type Imported = { x: string };\n");
    upsert(
        &host,
        consumer,
        "import { Imported } from './carveout-indexed-leaf';\n\
         export type Pick = Imported[\"x\"];\n",
    );
    host.ensure_indexed_ready(leaf).expect("leaf indexed");
    host.ensure_indexed_ready(consumer)
        .expect("consumer indexed");

    let result = walk(&host, &type_locator(consumer, "Pick"));
    let contributors = match &result {
        SourceWalkResult::Resolved { contributors } => contributors,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(contributors.len(), 1);
    assert_eq!(contributors[0].def_canonical, consumer);
    assert_eq!(
        contributors[0].carve_out_root_def.as_deref(),
        Some(leaf),
        "the live stamp must resolve the imported indexed-access root cross-file"
    );
    assert_eq!(
        admit_source_walk(&result),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("indexed-access")),
        "an imported indexed-access root is not a same-file carve-out"
    );
}

/// Unbound `keyof Missing` — the `keyof` root binds to NO declaration in the
/// fixture set. The live walk stamps `None`, the carve-out does not fire, and the
/// bare `keyof` REJECTS.
#[test]
fn source_root_carve_out_rejects_unbound_root() {
    let host = make_host();
    let canonical = "/fixtures/carveout_unbound.ts";
    // `Missing` is never declared anywhere in the fixture set.
    upsert(&host, canonical, "export type Keys = keyof Missing;\n");
    host.ensure_indexed_ready(canonical).expect("indexed");

    let result = walk(&host, &type_locator(canonical, "Keys"));
    let contributors = match &result {
        SourceWalkResult::Resolved { contributors } => contributors,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(contributors.len(), 1);
    assert_eq!(contributors[0].def_canonical, canonical);
    // The root did not bind → `None` stamp.
    assert_eq!(
        contributors[0].carve_out_root_def, None,
        "an unbound carve-out root stamps None"
    );
    assert_eq!(
        admit_source_walk(&result),
        AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof")),
        "an unbound carve-out root rejects"
    );
}
