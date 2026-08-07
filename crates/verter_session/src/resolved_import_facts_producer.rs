//! Production producer for
//! [`crate::resolved_import_facts::ResolvedImportFactsDb`].
//!
//! Reads the owner's `script_analysis.imports`
//! (`AnalyzedImport` / `AnalyzedImportBinding`) and the
//! admitted-route map on `DerivedRawState::import_routes`,
//! classifies each binding into
//! [`verter_semantic::facts::registry::SymbolSpace`]
//! (`Type` / `Value` / `Namespace` — v8 AMENDMENT-S), composes the
//! cache key from real per-canonical env hashes
//! (`VerterHost::host_view_env_hashes_for`), constructs one
//! [`crate::resolved_import_facts::ResolvedImportClauseEntry`] per
//! binding (positive AND negative), and admits the bundle plus the
//! owner's import-route witness through
//! [`crate::resolved_import_facts::ResolvedImportFactsDb::admit`].
//!
//! # SymbolSpace classification (v8 AMENDMENT-S)
//!
//! | Import syntax                            | `space`     |
//! | ---------------------------------------- | ----------- |
//! | `import * as ns from "X"`                | `Namespace` |
//! | `import type { X } from "X"`             | `Type`      |
//! | `import { type X } from "X"`             | `Type`      |
//! | `import type X from "X"` (default-type)  | `Type`      |
//! | `import X from "X"` (default value)      | `Value`     |
//! | `import { X } from "X"` (named, non-type)| `Value`     |
//!
//! The `is_type_only` flag is OR'd between the import declaration
//! and the per-specifier modifier
//! (`import { type X } from "X"`).
//!
//! # Negative facts
//!
//! When the workspace resolver returned no canonical for a
//! specifier (`route_resolution.resolved_canonical_id.is_none()`
//! AND no effective target),
//! [`crate::resolved_import_facts::ResolvedImportClauseEntry::resolved_canonical`]
//! is `None` on the admitted entry. The `Fact.key` for negative
//! entries uses the `UNRESOLVED_SENTINEL` (`"\0unresolved\0"`) so
//! the fact key namespace stays in
//! [`verter_semantic::facts::registry::FactDomain::ResolveImports`]
//! (`FactKey::ResolvedImportClause`) while remaining distinguishable
//! from any real canonical path.
//!
//! # Cache key (R21 scoping rule)
//!
//! [`crate::resolved_import_facts::ResolvedImportFactsKey`] is
//! `(canonical, content_hash, parse_env_hash, resolve_env_hash,
//! resolver_version)`. `lib_env_hash` is INTENTIONALLY ABSENT — a
//! TS lib change MUST NOT invalidate base import-target resolution.
//! Resolution currency is not a key dimension either: it lives on the
//! VALUE side, as the owner's import-route resolution witness recorded
//! in the candidate's `ReadSetSignature`. A specifier that becomes
//! resolvable (or retargets) advances exactly the resolver observations
//! the witness recorded, so the retained bundle stops validating and the
//! fresh one enters the bounded slot beside it.
//! Arch-guard
//! `crates/verter_session/tests/cases/g_misc1/lib_env_hash_excluded_from_resolved_import_facts.rs`
//! pins this absence.

use std::sync::Arc;

use verter_semantic::analysis::types::ImportBindingKind;
use verter_semantic::facts::registry::{
    Fact, FactKey, InternedName, InternedSpecifier, SymbolSpace,
};
use verter_workspace::FactVersionRef;

use crate::hash::hash_16;
use crate::host_executor::HostSourceData;
use crate::resolved_import_facts::{
    ResolvedImportClauseEntry, ResolvedImportFacts, ResolvedImportFactsKey, ResolvedSpecifier,
    RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
};
use crate::VerterHost;

/// Sentinel canonical placed on a negative
/// [`FactKey::ResolvedImportClause`] entry so the fact key stays
/// non-`Option` while remaining distinguishable from any real
/// canonical path. The NUL bytes prevent collision with any
/// filesystem path on every platform Verter supports.
pub(crate) const UNRESOLVED_SENTINEL: &str = "\0unresolved\0";

impl VerterHost {
    /// Production producer for
    /// [`crate::resolved_import_facts::ResolvedImportFactsDb`].
    ///
    /// Called from
    /// [`VerterHost::set_import_dependencies`](crate::host_manage::analysis_io)
    /// after a fresh route-resolution batch is admitted on
    /// `DerivedRawState::import_routes`. Reads the owner's
    /// `script_analysis.imports`, classifies each binding into a
    /// `SymbolSpace`, composes the cache key from the
    /// per-canonical env hashes, and admits one bundle of
    /// per-binding [`ResolvedImportClauseEntry`] values, together with
    /// the owner's import-route witness, through
    /// [`crate::resolved_import_facts::ResolvedImportFactsDb::admit`].
    ///
    /// A recomputation under an already-retained `(key, witness)` pair
    /// is skipped: it is deterministic, so re-admitting would push a
    /// duplicate candidate and age a genuinely distinct resolution
    /// state out of the bounded slot. The producer does NOT bump
    /// admission counters for a skipped or refused admission.
    ///
    /// Returns `true` when this call admitted a candidate, `false` when
    /// an equivalent candidate was already retained, when strict
    /// admission refused the witness, or when the producer could not
    /// extract enough state to build a payload (e.g. the canonical has
    /// not been parsed yet).
    pub(crate) fn admit_resolved_import_facts_for_owner(
        &self,
        canonical: &str,
        import_routes: &rustc_hash::FxHashMap<String, crate::types::DependencyResolution>,
    ) -> bool {
        // 1. Read the owner's analyzed imports + content hash from
        //    the scheduler-cached source data. The scheduler is the
        //    sole parse authority — `parse.whole_hash` is the
        //    canonical content hash for the file's current bytes
        //    (`FileArtifactStore` records the same value as its
        //    `content_hash` dimension when `IndexedReady` is later
        //    materialized via `ensure_indexed_ready_serve`). Reading from
        //    the scheduler lets the producer admit immediately after
        //    `upsert` without waiting for the lazy `IndexedReady`
        //    materialization.
        //
        //    `script_analysis.imports` carries `AnalyzedImport` with
        //    `bindings: Vec<AnalyzedImportBinding>`
        //    (`kind: ImportBindingKind { Named, Default, Namespace }`
        //    + `is_type_only`).
        let Some(source_snap) = self.scheduler.try_get_source(canonical) else {
            return false;
        };
        let Some(hd) = source_snap.downcast_data::<HostSourceData>() else {
            return false;
        };
        let imports = hd.parse.script_analysis.imports.clone();
        let content_hash = hd.parse.whole_hash;
        drop(source_snap);

        // 2. Compose the cache key from real env-hashes — the
        //    fact-validated import-facts substrate. `content_hash`
        //    is the scheduler-cached `parse.whole_hash` captured
        //    above.
        let env = self.host_view_env_hashes_for(canonical);
        let key = ResolvedImportFactsKey {
            canonical: Arc::from(canonical),
            content_hash,
            parse_env_hash: env.parse_env_hash,
            resolve_env_hash: env.resolve_env_hash,
            resolver_version: RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
        };

        // 2b. The witness — see `resolved_import_facts_witness`. An
        //     unrootable owner (a refused resolution, or an overflowing
        //     observation set) admits nothing: the bundle's values are
        //     resolved canonicals, so a candidate no read could
        //     invalidate would stale-serve the pre-retarget resolution.
        let Some(facts) = self.resolved_import_facts_witness(canonical, content_hash) else {
            return false;
        };
        let db = self.project_type_store().resolved_import_facts();

        // 3. Build the per-binding entries. One entry per
        //    `(binding, space)` pair: `import { X, type Y } from "S"`
        //    contributes two entries (X → Value, Y → Type).
        //
        //    `script_analysis.imports` is populated for every
        //    parseable file (Vue SFC + non-SFC TS/JS) via the
        //    shared `parse_non_sfc_snapshot` / SFC script-analysis
        //    pipeline. An empty `imports` vector means the source
        //    truly has no import declarations or failed to parse;
        //    either case is "no admission required".
        let (payload, admission_counts) = {
            let bindings = collect_analyzed_bindings(&imports);

            let mut import_clauses = Vec::with_capacity(bindings.len());
            let mut specifier_resolutions: Vec<ResolvedSpecifier> =
                Vec::with_capacity(bindings.len());
            let mut positive_bumps = 0u64;
            let mut negative_bumps = 0u64;
            let mut namespace_bumps = 0u64;

            for ClassifiedBinding {
                specifier,
                local_name,
                imported_name,
                space,
            } in bindings
            {
                // Look up resolved canonical for this specifier from
                // the freshly-admitted route map.
                let resolved_canonical: Option<Arc<str>> = import_routes
                    .get(&specifier)
                    .and_then(|res| res.resolved_canonical_id.as_deref())
                    .map(Arc::from);
                let is_resolved = resolved_canonical.is_some();

                // `resolved_source_name` is NON-OPTIONAL. For positive
                // entries it is the original exported name in the target
                // module (`imported_name`). For negative entries it is
                // also the original requested name — preserved so the
                // validator can compare the requested binding shape on
                // re-resolution.
                let resolved_source_name = InternedName::from(imported_name.as_str());

                // Build the `Fact` lanes. `semantic_hash` is the
                // structural fingerprint over
                // `(specifier, binding, space, resolved_canonical_or_sentinel,
                //   resolved_source_name)`. `display_hash` mixes a
                // distinct display salt so the two lanes are populated
                // separately (R13).
                let fact_key_canonical: Arc<str> = match resolved_canonical.as_ref() {
                    Some(c) => Arc::clone(c),
                    None => Arc::from(UNRESOLVED_SENTINEL),
                };
                let mut buf: Vec<u8> = Vec::with_capacity(96);
                buf.extend_from_slice(b"resolved-import-clause:");
                buf.push(space.tag());
                buf.extend_from_slice(specifier.as_bytes());
                buf.push(0xFE);
                buf.extend_from_slice(local_name.as_bytes());
                buf.push(0xFE);
                buf.extend_from_slice(imported_name.as_bytes());
                buf.push(0xFE);
                buf.extend_from_slice(fact_key_canonical.as_bytes());
                let semantic_hash = hash_16(&buf);

                let mut display_buf: Vec<u8> = Vec::with_capacity(48);
                display_buf.extend_from_slice(b"resolved-import-clause-display:");
                display_buf.extend_from_slice(&semantic_hash);
                display_buf.extend_from_slice(local_name.as_bytes());
                let display_hash = hash_16(&display_buf);

                let fact = Arc::new(Fact {
                    key: FactKey::ResolvedImportClause {
                        specifier: InternedSpecifier::from(specifier.as_str()),
                        binding: InternedName::from(local_name.as_str()),
                        space,
                        resolved_canonical: Arc::clone(&fact_key_canonical),
                        resolved_source_name: resolved_source_name.clone(),
                    },
                    semantic_hash,
                    display_hash,
                });

                import_clauses.push(ResolvedImportClauseEntry {
                    specifier: InternedSpecifier::from(specifier.as_str()),
                    binding: InternedName::from(local_name.as_str()),
                    space,
                    resolved_canonical: resolved_canonical.clone(),
                    resolved_source_name,
                    fact,
                });

                // Per-specifier resolutions (one per `(specifier, space)`).
                specifier_resolutions.push(ResolvedSpecifier {
                    specifier: InternedSpecifier::from(specifier.as_str()),
                    resolved_canonical: resolved_canonical.clone(),
                    space,
                });

                if is_resolved {
                    positive_bumps += 1;
                    if matches!(space, SymbolSpace::Namespace) {
                        namespace_bumps += 1;
                    }
                } else {
                    negative_bumps += 1;
                }
            }

            // 4. The cold payload.
            let payload = Arc::new(ResolvedImportFacts {
                import_clauses,
                reexport_bindings: Vec::new(),
                specifier_resolutions,
            });
            (payload, (positive_bumps, negative_bumps, namespace_bumps))
        };

        // 5. Strict admission onto the bounded multi-candidate slot.
        //
        //    Resolution currency lives on the VALUE side now: each
        //    candidate carries the observations that produced its
        //    resolved canonicals, so a retargeted recomputation is a
        //    genuinely distinct candidate whose predecessor simply
        //    stops validating on the read side. No producer-side hard
        //    removal is needed — and none is wanted, because a hard
        //    removal would also drop a sibling candidate that is still
        //    valid for another view.
        //
        //    A recomputation that reproduces a retained candidate WHOLE
        //    is pure churn: re-admitting it would age a genuinely
        //    distinct concurrent candidate out of the bounded slot, so
        //    it is skipped and counted as no admission.
        //
        //    "Whole" is one CORRELATED question over ONE slot load: does
        //    a single candidate carry BOTH this witness and this payload.
        //    Asking the two halves separately is unsound twice over — the
        //    witness could be held by one candidate and the payload by
        //    another, and the slot could be mutated between the two
        //    loads. Either way the producer would skip on the strength of
        //    a pair no candidate retains, silently dropping a fresh
        //    resolution state.
        //
        //    A refused admission (empty or over-cap signature) leaves
        //    the store untouched and reports no admission, so the
        //    provenance counters below never claim unretained work.
        if db.holds_candidate_matching(&key, &facts, payload.as_ref()) {
            return false;
        }
        let admitted = db.admit(key, payload, facts);

        #[cfg(any(test, feature = "test-support"))]
        if admitted {
            let (positive_bumps, negative_bumps, namespace_bumps) = admission_counts;
            for _ in 0..positive_bumps {
                db.record_positive_admission();
            }
            for _ in 0..negative_bumps {
                db.record_negative_admission();
            }
            for _ in 0..namespace_bumps {
                db.record_namespace_admission();
            }
        }

        // The metadata is observed only by test-support provenance counters.
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = admission_counts;

        admitted
    }
}

impl VerterHost {
    /// The witness every [`ResolvedImportFacts`] candidate is admitted
    /// under: the owner's own content identity PLUS the owner's
    /// import-route resolution witness.
    ///
    /// A bundle describes ONE owner's import clauses, so its content
    /// root is that owner's bytes — but its VALUES are resolved
    /// canonicals, so the bytes alone are not a validity oracle. A
    /// dependency appearing (or a higher-priority candidate appearing
    /// beside an already-resolving one) retargets a clause while the
    /// owner's bytes stay put; the resolution witness observes exactly
    /// the `PathProbe` / `Realpath` / `ExactResolution` facts that the
    /// appearance advances, so the bundle stops validating on the read
    /// side. Resolution currency is therefore carried as observed facts
    /// and is deliberately not a key dimension.
    ///
    /// `None` means the owner's import routes could not be rooted (a
    /// refused resolution, or a signature that overflows the bound).
    /// The producer must not admit a candidate it cannot invalidate.
    pub(crate) fn resolved_import_facts_witness(
        &self,
        canonical: &str,
        content_hash: crate::types::Hash16,
    ) -> Option<Vec<FactVersionRef>> {
        let mut witness = vec![FactVersionRef::FileWholeHash {
            canonical_id: canonical.to_string(),
            hash: content_hash,
        }];
        witness.extend(self.owner_import_route_witness(canonical)?);
        Some(witness)
    }

    /// Test-support mirror of [`Self::resolved_import_facts_witness`]
    /// so fixtures that seed the store directly admit under the same
    /// witness the production producer would.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn resolved_import_facts_witness_for(
        &self,
        canonical: &str,
        content_hash: crate::types::Hash16,
    ) -> Option<Vec<FactVersionRef>> {
        self.resolved_import_facts_witness(canonical, content_hash)
    }
}

/// One binding pre-classified into its [`SymbolSpace`].
struct ClassifiedBinding {
    /// Raw import specifier as written (`"./util"`, `"vue"`, ...).
    specifier: String,
    /// Local binding name as written in the importing file
    /// (`import { X as Y }` → `"Y"`).
    local_name: String,
    /// Resolved source-name in the target module
    /// (`import { X as Y }` → `"X"`; default → `"default"`;
    /// namespace → `"*"`).
    imported_name: String,
    /// SymbolSpace (Type / Value / Namespace) per the import
    /// syntax (see module-level doc).
    space: SymbolSpace,
}

/// Classify the per-binding shape from the
/// `script_analysis.imports` vector (`AnalyzedImport` with full
/// kind + `is_type_only` info).
fn collect_analyzed_bindings(
    imports: &[verter_semantic::analysis::types::AnalyzedImport],
) -> Vec<ClassifiedBinding> {
    let mut out = Vec::new();
    for imp in imports {
        let decl_is_type_only = imp.is_type_only;
        let specifier = imp.source.clone();
        for b in &imp.bindings {
            let kind_is_namespace = matches!(b.kind, ImportBindingKind::Namespace);
            let space = if kind_is_namespace {
                // `import * as ns from "X"` — Namespace regardless
                // of `is_type_only` (a `import type * as ns` is
                // rare and still binds a namespace identifier).
                SymbolSpace::Namespace
            } else if decl_is_type_only || b.is_type_only {
                SymbolSpace::Type
            } else {
                SymbolSpace::Value
            };
            let imported_name = match &b.imported_name {
                Some(name) => name.clone(),
                None => match b.kind {
                    ImportBindingKind::Namespace => "*".to_string(),
                    ImportBindingKind::Default => "default".to_string(),
                    ImportBindingKind::Named => b.name.clone(),
                },
            };
            out.push(ClassifiedBinding {
                specifier: specifier.clone(),
                local_name: b.name.clone(),
                imported_name,
                space,
            });
        }
    }
    out
}

#[cfg(test)]
#[path = "resolved_import_facts_producer_tests.rs"]
mod tests;
