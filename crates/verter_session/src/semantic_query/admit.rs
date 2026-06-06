//! §18.2 error-tolerant cache-admission decision.
//!
//! [`admit_decision`] is the SOLE place that maps an error-tolerant semantic
//! result to a cache-admission disposition. It gates the [`Warm`](Admission::Warm)
//! arm on the **presence of the rooting FACT in the result's
//! [`ReadSetSignature`]** — NOT on the taint enum class as a proxy
//! (`docs/arch/u2-query-value-domain-design.md` §18.2):
//!
//! - A `Clean` result over a soundly self-version-rooted carrier publishes
//!   warm. `taint` is currently always `Clean`; non-`Clean` taint is produced
//!   by the §18.4 input-degradation producers, and the partial/broken arms
//!   below are exercised by this module's `admit_decision` unit tests.
//! - A `Partial(MissingDependency)` / `Partial(UnresolvedReference)` is
//!   `Warm` ONLY when its invalidation rail (the missing-dependency /
//!   negative-resolution FACT) was actually recorded on the signature; if the
//!   producer degraded the reference WITHOUT recording the fact there is no
//!   invalidation rail, so the result falls to [`ReturnOnly`](Admission::ReturnOnly).
//!   The taint class narrows WHICH fact must be present; the signature is the
//!   authority for whether it IS present.
//! - A `Partial(IncompleteDeclaration)` (mid-edit shape, no stable fact), a
//!   `Broken(SyntaxError)` (torn parse tree), or a `Broken(TornRead)`
//!   (content version changed mid-flight) is `ReturnOnly` — never warm.
//!
//! `admit_decision` is ONLY consulted once the cold-build helper has already
//! produced a SOUND self-version-rooted carrier (the
//! [`semantic_graph_read_set_signature`](crate::semantic_query_memo::semantic_graph_read_set_signature)
//! `Some` case). An overflowed tracer or a torn / unrootable self-root
//! observation routes to `ReturnOnly` BEFORE `admit_decision` is reached — a
//! result with no sound carrier can never be `Warm` regardless of taint. So
//! the function's job is the second, taint-narrowed gate: given a sound
//! carrier, does THIS taint class carry an invalidation rail that makes the
//! warm entry safe to publish?
//!
//! The error TYPE (`X is the error type`, §22) rides
//! [`SemanticNodeData::Opaque(QueryError)`](crate::semantic_query::SemanticNodeData::Opaque);
//! `admit_decision` consumes its §18 [`ResultTaint`] to decide admission: an
//! `error` rooted on a tracked fact (`MissingDependency` / `UnresolvedReference`)
//! is fact-rooted-cacheable; an `error` produced by a torn / broken input is
//! `ReturnOnly`-prone. `any` / `never` / `unknown` are `Clean` and cacheable.
//! (The taint PRODUCERS that emit non-`Clean` taint are §18.4; the gate here
//! is implemented and unit-tested.)

use crate::fact_signature_helpers::ReadSetSignature;
use crate::semantic_query::{BrokenInputClass, ResultTaint};

/// The §18.2 cache-admission disposition for an error-tolerant result.
///
/// This is the DOMAIN decision (publish-warm vs return-only); the cold-build
/// helper maps it onto the cooperative-admission carrier state (the
/// [`QueryBuildOutput`](crate::project_semantic_dispatch::walk::QueryBuildOutput)
/// `cache_suppress` / `graph_carrier` fields). It is deliberately distinct from the generic
/// [`ComputeAdmission`](crate::cache_runtime::singleflight::ComputeAdmission)
/// cache enum: `Admission` names the §18 policy, the cache enum names the
/// singleflight mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// The result is fact-rooted and may be published as a warm cache entry.
    Warm,
    /// The result is returned to the caller but NEVER warm-admitted, NEVER
    /// backfilled, NEVER published, and records no fact signature / reverse-index
    /// entry — the existing `ComputeAdmission::ReturnOnly` discipline (the same
    /// path `BudgetExceeded` / cancellation / supersession take).
    ReturnOnly,
}

/// Decide whether an error-tolerant result with the given `taint` and a SOUND
/// self-version-rooted `sig` may be warm-admitted (§18.2).
///
/// The gate keys on the **rooting FACT in `sig`**, not on the taint enum class:
/// a `Partial(MissingDependency)` / `Partial(UnresolvedReference)` is `Warm`
/// only when its invalidation rail was actually recorded on `sig`. Every
/// non-fact-rooted broken class — `IncompleteDeclaration`, `SyntaxError`,
/// `TornRead`, and any `Broken` severity — is `ReturnOnly`.
///
/// Forward note (§18.4): the gate keys on the presence of the rooting fact
/// KIND on `sig` ([`records_missing_dependency_fact`](ReadSetSignature::records_missing_dependency_fact)
/// = a `DerivedFactKind::ImportRoute` fact present;
/// [`records_negative_resolution_fact`](ReadSetSignature::records_negative_resolution_fact)
/// = a negative `ResolvedImportClause` / `ResolvedReexportBinding`
/// `UNRESOLVED_SENTINEL` fact present). It does NOT correlate that fact to the
/// SPECIFIC degraded reference that produced the taint — reference-specific
/// rooting is a §18.4 follow-up once the taint producers emit the degraded
/// reference identity alongside the fact.
///
/// Precondition: `sig` is the sound carrier the cold-build helper produced;
/// callers route overflow / unrootable results to `ReturnOnly` before reaching
/// here. A `Clean` taint therefore always returns `Warm`.
#[must_use]
pub fn admit_decision(taint: ResultTaint, sig: &ReadSetSignature) -> Admission {
    match taint {
        // Normal publish: a clean result over a sound carrier publishes warm.
        ResultTaint::Clean => Admission::Warm,
        // Fact-rooted partial: cacheable IFF the invalidation rail is on
        // the signature (§18.2.1).
        ResultTaint::Partial(BrokenInputClass::MissingDependency) => {
            if sig.records_missing_dependency_fact() {
                Admission::Warm
            } else {
                Admission::ReturnOnly
            }
        }
        ResultTaint::Partial(BrokenInputClass::UnresolvedReference) => {
            if sig.records_negative_resolution_fact() {
                Admission::Warm
            } else {
                Admission::ReturnOnly
            }
        }
        // Mid-edit shape with no stable fact, a torn parse tree, or a torn
        // read — never warm. The `Partial` severities of the unstable
        // classes join here too (conservative): only the two fact-rooted
        // partial classes above can ever be `Warm`.
        ResultTaint::Partial(
            BrokenInputClass::IncompleteDeclaration
            | BrokenInputClass::SyntaxError
            | BrokenInputClass::TornRead,
        ) => Admission::ReturnOnly,
        // Any `Broken`-severity taint stands wholly on a broken input —
        // never warm.
        ResultTaint::Broken(_) => Admission::ReturnOnly,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::{DerivedFactKind, FactVersionRef, ResolveImportsFactRef};
    use std::sync::Arc;
    use verter_semantic::facts::registry::{FactKey, FactLane, InternedName, SymbolSpace};

    fn sig_from(facts: Vec<FactVersionRef>) -> ReadSetSignature {
        ReadSetSignature::new(Arc::from(facts.into_boxed_slice()))
    }

    fn import_route_fact() -> FactVersionRef {
        FactVersionRef::DerivedFactHash {
            canonical_id: "/missing.ts".to_string(),
            kind: DerivedFactKind::ImportRoute,
            hash: [7u8; 16],
        }
    }

    fn negative_resolved_import_fact() -> FactVersionRef {
        use verter_semantic::facts::registry::InternedSpecifier;
        FactVersionRef::ResolveImports(ResolveImportsFactRef {
            canonical_id: "/importer.ts".to_string(),
            key: FactKey::ResolvedImportClause {
                specifier: InternedSpecifier::from("./missing"),
                binding: InternedName::from("X"),
                space: SymbolSpace::Type,
                resolved_canonical: Arc::from(
                    crate::resolved_import_facts_producer::UNRESOLVED_SENTINEL,
                ),
                resolved_source_name: InternedName::from("X"),
            },
            lane: FactLane::Semantic,
            expected_hash: [3u8; 16],
        })
    }

    #[test]
    fn clean_over_sound_carrier_is_warm() {
        assert_eq!(
            admit_decision(ResultTaint::Clean, &sig_from(vec![])),
            Admission::Warm
        );
    }

    #[test]
    fn missing_dependency_warm_only_with_import_route_fact() {
        // Fact recorded → Warm (the invalidation rail exists).
        assert_eq!(
            admit_decision(
                ResultTaint::Partial(BrokenInputClass::MissingDependency),
                &sig_from(vec![import_route_fact()]),
            ),
            Admission::Warm
        );
        // Fact absent → ReturnOnly (no rail; a positive FileWholeHash must
        // not stand in for the missing-dep rail).
        assert_eq!(
            admit_decision(
                ResultTaint::Partial(BrokenInputClass::MissingDependency),
                &sig_from(vec![FactVersionRef::FileWholeHash {
                    canonical_id: "/x.ts".to_string(),
                    hash: [1u8; 16],
                }]),
            ),
            Admission::ReturnOnly
        );
    }

    #[test]
    fn unresolved_reference_warm_only_with_negative_resolution_fact() {
        assert_eq!(
            admit_decision(
                ResultTaint::Partial(BrokenInputClass::UnresolvedReference),
                &sig_from(vec![negative_resolved_import_fact()]),
            ),
            Admission::Warm
        );
        assert_eq!(
            admit_decision(
                ResultTaint::Partial(BrokenInputClass::UnresolvedReference),
                &sig_from(vec![]),
            ),
            Admission::ReturnOnly
        );
    }

    #[test]
    fn unstable_and_broken_classes_are_returnonly_regardless_of_facts() {
        let rich = sig_from(vec![import_route_fact(), negative_resolved_import_fact()]);
        for taint in [
            ResultTaint::Partial(BrokenInputClass::IncompleteDeclaration),
            ResultTaint::Partial(BrokenInputClass::SyntaxError),
            ResultTaint::Partial(BrokenInputClass::TornRead),
            ResultTaint::Broken(BrokenInputClass::SyntaxError),
            ResultTaint::Broken(BrokenInputClass::TornRead),
            ResultTaint::Broken(BrokenInputClass::MissingDependency),
        ] {
            assert_eq!(
                admit_decision(taint, &rich),
                Admission::ReturnOnly,
                "{taint:?} must be ReturnOnly even with a rich fact rail"
            );
        }
    }
}
