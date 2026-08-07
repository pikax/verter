//! `ZH-1`: a missing resolved-import slot is ABSENCE OF EVIDENCE, and must
//! reject.
//!
//! `validates_resolve_imports_domain_for_content_hash` used to end its
//! slot-miss arm with `None => return *expected_hash == ZERO_HASH`: a miss
//! ACCEPTED whenever the consumer had recorded a zero hash.
//!
//! That is wrong under either reading of what a zero hash means. The
//! question the arm answers is not "what does zero assert" but "may a
//! MISSING slot settle a claim about a particular Semantic fact" — and it
//! may not. An absence assertion needs a current authoritative slot or an
//! explicit negative carrier to stand on; a slot that is simply not there
//! supplies neither. Two further facts settle it:
//!
//! * The validator's own documentation already said so — "Cache slot
//!   absent for the composed key → reject. The cache was the recording
//!   site; absence means the consumer observed a stale slice." The
//!   implementation accepted zero anyway; the doc was right and the code
//!   was wrong.
//! * Zero was never needed as the negative-resolution rail. An unresolved
//!   import carries the explicit `UNRESOLVED_SENTINEL` fact with a real
//!   semantic hash, so nothing is lost by rejecting a zero-hash miss.
//!
//! **Out of scope, deliberately.** This says nothing about the separate
//! UNTRACKED-canonical arm in `validates_resolve_imports_domain`, whose
//! optimistic zero-accept is the R26 untracked-file window. That arm is a
//! different decision on a different question (is this canonical tracked
//! at all) and is left exactly as it was — the tracked-canonical
//! precondition asserted below is what keeps these tests off it.

use std::sync::Arc;

use verter_semantic::facts::registry::{FactKey, FactLane, InternedName, InternedSpecifier};

use crate::resolver_core::{FactVersionRef, ResolveImportsFactRef, StoreView};
use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

const ZERO_HASH: crate::types::Hash16 = [0u8; 16];

const OWNER: &str = "/proj/owner.ts";
const DEP: &str = "/proj/dep.ts";

fn upsert(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|err| panic!("upsert {id} must succeed: {err:?}"));
}

fn host_with_owner_and_dep() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, DEP, "export const a = 1;\n");
    upsert(
        &host,
        OWNER,
        "import { a } from './dep'\nexport const z = a;\n",
    );
    host
}

/// The fact shape a consumer records for the `a` binding, at whatever
/// `expected_hash` the caller wants to claim.
fn import_clause_fact(expected_hash: crate::types::Hash16) -> FactVersionRef {
    FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
        canonical_id: OWNER.to_string(),
        key: FactKey::ResolvedImportClause {
            specifier: InternedSpecifier::from("./dep"),
            binding: InternedName::from("a"),
            space: verter_semantic::facts::registry::SymbolSpace::Value,
            resolved_canonical: Arc::from(DEP),
            resolved_source_name: InternedName::from("a"),
        },
        lane: FactLane::Semantic,
        expected_hash,
    })
}

/// `ZH-1`: a TRACKED canonical with NO admitted slot rejects a zero-hash
/// Semantic fact.
///
/// The two preconditions are asserted rather than assumed, because each
/// one being wrong would make this test pass for a reason that has
/// nothing to do with the arm under change:
///
/// 1. the canonical is TRACKED — otherwise the fact never reaches this
///    validator at all, it is settled by the separate untracked-canonical
///    arm that this change deliberately does not touch;
/// 2. no bundle was ever admitted for it — otherwise the lookup hits and
///    the slot-miss arm is never evaluated.
///
/// Mutation recipe, VERIFIED against the landed tree: restore the
/// permissive arm in `validates_resolve_imports_domain_for_content_hash`
/// — `None => return false` becomes
/// `None => return *expected_hash == ZERO_HASH`. This test goes red.
#[test]
fn a_zero_hash_semantic_fact_is_rejected_on_a_slot_miss_for_a_tracked_file() {
    let host = host_with_owner_and_dep();
    let view = host.resolver_store_view_read().into_owned_view();

    assert!(
        view.tracks_file(OWNER),
        "precondition: the owner must be TRACKED, or this fact is settled by the separate \
         untracked-canonical arm and this test would say nothing about the slot-miss arm"
    );
    assert!(
        host.project_type_store()
            .resolved_import_facts()
            .retained_bundle_for_tests(&crate::resolved_import_facts::ResolvedImportFactsKey {
                canonical: Arc::from(OWNER),
                content_hash: host
                    .current_or_read_whole_hash(OWNER)
                    .expect("owner content hash"),
                parse_env_hash: host.host_view_env_hashes_for(OWNER).parse_env_hash,
                resolve_env_hash: host.host_view_env_hashes_for(OWNER).resolve_env_hash,
                resolver_version:
                    crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
            })
            .is_none(),
        "precondition: NO bundle may be admitted for the owner, or the lookup hits and the \
         slot-miss arm is never reached"
    );

    assert!(
        !view.validates(&import_clause_fact(ZERO_HASH)),
        "a missing resolved-import slot is ABSENCE OF EVIDENCE, not evidence that this \
         Semantic fact is absent. Accepting it lets a consumer that observed a stale slice \
         validate against a slot the producer never wrote"
    );
}

/// A non-zero hash on the same slot miss also rejects — so the fix is
/// about the MISS, not about the hash value.
///
/// Without this, the arm could be read as "zero is special"; it is not.
/// Both hashes now take the same path for the same reason.
#[test]
fn a_slot_miss_rejects_regardless_of_the_recorded_hash() {
    let host = host_with_owner_and_dep();
    let view = host.resolver_store_view_read().into_owned_view();

    for hash in [ZERO_HASH, [7u8; 16], [255u8; 16]] {
        assert!(
            !view.validates(&import_clause_fact(hash)),
            "every recorded hash must be rejected on a slot miss, including {hash:?} — the \
             miss is the reason, not the value"
        );
    }
}

/// **The positive control.** A present, valid candidate carrying a
/// MATCHING real Semantic fact still validates.
///
/// This is what stops the fix from passing by rejecting everything. The
/// slot-miss arm is the only thing that changed; a genuine hit must be
/// entirely unaffected, and the fact below is built from the payload the
/// producer actually admitted rather than from a hand-written hash.
///
/// Mutation recipe: change the arm to reject unconditionally (make the
/// whole function `return false`). This test goes red while the two
/// above stay green — which is the pair that distinguishes "rejects a
/// miss" from "rejects everything".
#[test]
fn a_present_candidate_with_a_matching_fact_still_validates() {
    let host = host_with_owner_and_dep();

    // Production admission path.
    host.set_import_dependencies(
        OWNER,
        vec![crate::types::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some(DEP.to_string()),
            possible_canonical_ids: vec![DEP.to_string()],
        }],
    );

    let content_hash = host
        .current_or_read_whole_hash(OWNER)
        .expect("owner content hash");
    let admitted = host
        .project_type_store()
        .resolved_import_facts()
        .retained_bundle_for_tests(&crate::resolved_import_facts::ResolvedImportFactsKey {
            canonical: Arc::from(OWNER),
            content_hash,
            parse_env_hash: host.host_view_env_hashes_for(OWNER).parse_env_hash,
            resolve_env_hash: host.host_view_env_hashes_for(OWNER).resolve_env_hash,
            resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
        })
        .expect("the producer must admit a bundle for the owner");
    let entry = admitted
        .import_clauses
        .iter()
        .find(|e| e.binding.as_ref() == "a")
        .expect("the `a` binding must be admitted into the payload");

    // Built from the ADMITTED payload, not from a hand-written hash, so a
    // pass here means the real hit path is intact.
    let fact = FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
        canonical_id: OWNER.to_string(),
        key: FactKey::ResolvedImportClause {
            specifier: InternedSpecifier::from(entry.specifier.as_ref()),
            binding: InternedName::from(entry.binding.as_ref()),
            space: entry.space,
            resolved_canonical: entry
                .resolved_canonical
                .as_ref()
                .map(Arc::clone)
                .expect("resolved canonical present"),
            resolved_source_name: InternedName::from(entry.resolved_source_name.as_ref()),
        },
        lane: FactLane::Semantic,
        expected_hash: entry.fact.semantic_hash,
    });

    let view = host.resolver_store_view_read().into_owned_view();
    assert!(
        view.validates(&fact),
        "narrowing the slot-MISS arm must not touch the hit path: a present candidate \
         carrying the fact the producer actually admitted must still validate"
    );
}
