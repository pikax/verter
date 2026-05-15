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
//! binding (positive AND negative), and admits the bundle through
//! [`crate::resolved_import_facts::ResolvedImportFactsDb::insert_if_absent`]
//! (first-writer-wins).
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
//! Arch-guard
//! `crates/verter_session/tests/lib_env_hash_excluded_from_resolved_import_facts.rs`
//! pins this absence.

use std::sync::Arc;

use verter_semantic::analysis::types::ImportBindingKind;
use verter_semantic::facts::registry::{
    Fact, FactKey, InternedName, InternedSpecifier, SymbolSpace,
};

use crate::hash::hash_16;
use crate::host_executor::HostSourceData;
use crate::resolved_import_facts::{
    compute_known_miss_generation_tag, ResolvedImportClauseEntry, ResolvedImportFacts,
    ResolvedImportFactsKey, ResolvedSpecifier, RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
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
    /// per-binding [`ResolvedImportClauseEntry`] values through
    /// [`crate::resolved_import_facts::ResolvedImportFactsDb::insert_if_absent`].
    ///
    /// First-writer-wins admission: an identical key MUST be a
    /// deterministic recomputation. The second writer's bundle is
    /// silently discarded and the producer DOES NOT bump
    /// admission counters for the duplicate.
    ///
    /// Returns `true` when this call won the admission race,
    /// `false` when an existing entry was already present (or when
    /// the producer could not extract enough state to build a
    /// payload, e.g. when the canonical has not been parsed yet).
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
        //    materialized via `ensure_indexed_ready`). Reading from
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

        // 2. Compose the cache key from real env-hashes (Block 1.6
        //    substrate). `content_hash` is the scheduler-cached
        //    `parse.whole_hash` captured above.
        //
        //    `known_miss_generation` (Codex P2.2 / Block 1.f-fix)
        //    is a stable tag over the owner's
        //    `DerivedRawState::import_routes_known_miss_recorded_at_generation`
        //    sidecar. `set_import_dependencies` updates the sidecar
        //    BEFORE calling this producer, so a re-resolution that
        //    converts a previously-missing specifier into a positive
        //    target (after the target file is created and the
        //    workspace `content_generation` advances) admits under a
        //    NEW key value — the stale negative bundle is naturally
        //    superseded instead of being pinned by first-writer-wins
        //    against the prior key. Empty known-miss map →
        //    `[0u8; 16]`, so an owner with no known-misses produces
        //    the same key value at producer time and at lookup time.
        let env = self.host_view_env_hashes_for(canonical);
        let known_miss_generation = {
            let entry = self.derived_raw_cache().get(canonical);
            match entry {
                Some(e) => compute_known_miss_generation_tag(
                    &e.import_routes_known_miss_recorded_at_generation,
                ),
                None => [0u8; 16],
            }
        };
        let key = ResolvedImportFactsKey {
            canonical: Arc::from(canonical),
            content_hash,
            parse_env_hash: env.parse_env_hash,
            resolve_env_hash: env.resolve_env_hash,
            resolver_version: RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
            known_miss_generation,
        };

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
        let bindings = collect_analyzed_bindings(&imports);

        let mut import_clauses = Vec::with_capacity(bindings.len());
        let mut specifier_resolutions: Vec<ResolvedSpecifier> = Vec::with_capacity(bindings.len());
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

        // 4. Admit through `insert_if_absent` (first-writer-wins).
        let payload = Arc::new(ResolvedImportFacts {
            import_clauses,
            reexport_bindings: Vec::new(),
            specifier_resolutions,
        });
        let admitted = self
            .project_type_store()
            .resolved_import_facts()
            .insert_if_absent(key, payload);

        // 5. Bump provenance counters only on admission win. A
        //    duplicate (admitted=false) must NOT bump because the
        //    canonical recomputed and the existing entry already
        //    owns those admissions.
        #[cfg(any(test, debug_assertions))]
        if admitted {
            let db = self.project_type_store().resolved_import_facts();
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

        // Suppress unused-warnings on release builds (no debug-assertions
        // and no test). These counts are recorded as provenance signal in
        // debug + test only; release-build hot path must not retain them.
        #[cfg(not(any(test, debug_assertions)))]
        let _ = (positive_bumps, negative_bumps, namespace_bumps);

        admitted
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
