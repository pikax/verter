//! Compile-tier fact observation (R3 / R26 / R28).
//!
//! Emits the `FactVersionRef` observations a compile cold-compute
//! pass depends on, routing each through the active fact tracer
//! installed by `VerterHost::with_fact_tracer`. Producers wire one
//! call to [`observe_compile_tier_dependencies`] around the
//! `compile_entry` invocation; the resulting `Arc<[FactVersionRef]>`
//! becomes the `fact_dep_signature` stored on the new
//! [`crate::types::CompileSlot`].
//!
//! ## Why path-precision (R28)
//!
//! Compile output for a Vue SFC depends on EACH macro type
//! dependency the script analysis recorded — `defineProps<T>()` /
//! `defineEmits<U>()` / `defineSlots<S>()` references. Observing
//! `FileWholeHash` for those deps would over-invalidate on cosmetic
//! edits and silent re-roll on adding unrelated sibling members.
//! Path-precise observation emits one `ParseFactRef::Member` (full
//! body fingerprint) plus one `ParseFactRef::MemberPresence`
//! (header) per `(dep_canonical, type_name)` tuple. A cross-file
//! edit that adds or removes the consumed `type_name` invalidates
//! the consumer; an edit to a sibling member does not.
//!
//! ## Augmentation observation (R29)
//!
//! `<script setup>` consumers of `'vue'` (or any other augmented
//! module specifier) depend on the augmenter-set fingerprint. When
//! the dependent project ships a `declare module 'vue' { ... }`
//! augmentation, the compile output's prop-validation surface
//! changes; the augmentation-index fingerprint observation makes
//! the dependent slot invalidate when the augmenter set churns.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_semantic::analysis::types::AnalyzedImport;
use verter_semantic::analysis::MacroTypeDep;
use verter_semantic::facts::registry::{
    AugmentationTargetKindTag, FactKey, InternedName, InternedSpecifier, SymbolSpace,
};
use verter_semantic::facts::FactLane;

use crate::resolver_core::{
    FactReadSet, FactReadSetCell, FactVersionRef, ParseFactRef, RouteSurfaceFactRef,
};
use crate::types::Hash16;
use crate::VerterHost;

/// Observe the compile-tier fact-dependency set for a single SFC
/// cold compute and route every observation through the active
/// tracer.
///
/// **Caller contract**: this method MUST be called inside an active
/// [`VerterHost::with_fact_tracer`] scope; the observations route
/// through TLS to the installed [`FactReadSetCell`]. When no tracer
/// is installed (warm-hit paths), every call is a no-op and adds
/// roughly one branch + one TLS read.
pub(crate) fn observe_compile_tier_dependencies(
    host: &VerterHost,
    canonical_id: &str,
    script_imports: &[AnalyzedImport],
    macro_type_deps: &[MacroTypeDep],
) {
    // No tracer installed → no-op fast path. Warm-hit reads do not
    // observe.
    let Some(cell) = host.current_fact_tracer() else {
        return;
    };

    // 1. Per-import `ImportRef` observation (R12 — parse-domain
    //    fact carrying the unresolved binding shape). Adding /
    //    removing an import binding invalidates the compile output
    //    that references the binding.
    let mut seen_imports = FxHashSet::default();
    for import in script_imports {
        let space = symbol_space_for_import(import);
        let specifier = InternedSpecifier::from(import.source.as_str());
        for binding in import.bindings.iter() {
            let local = binding.name.clone();
            if !seen_imports.insert((specifier.clone(), local.clone(), space)) {
                continue;
            }
            let key = FactKey::ImportRef {
                specifier: specifier.clone(),
                binding: InternedName::from(local.as_str()),
                space,
            };
            observe_parse_fact_present(host, canonical_id, cell, key, FactLane::Semantic);
        }
    }

    // 2. Per-`macro_type_deps` `Member` + `MemberPresence`
    //    observation (R28 path-precise; the producer reads the body
    //    of the referenced type to generate the prop / emit / slot
    //    validation, so it consumes both the existence and the
    //    body fingerprint). Editing a sibling member does NOT
    //    invalidate the consumer because the consumer only
    //    observes `Member(target_name)`.
    let mut seen_members = FxHashSet::default();
    for dep in macro_type_deps {
        // Resolve `dep.import_source` to a canonical via the
        // owner's import_routes (DerivedRawState sub-mirror).
        let Some(resolved_canonical) =
            resolve_import_source_to_canonical(host, canonical_id, &dep.import_source)
        else {
            continue;
        };
        let space = SymbolSpace::Type;
        let name = InternedName::from(dep.type_name.as_str());
        let exporter = name.clone();
        if !seen_members.insert((
            resolved_canonical.clone(),
            exporter.clone(),
            name.clone(),
            space,
        )) {
            continue;
        }
        // MemberPresence: header existence fact (R10 — adding `b`
        // does not change `MemberPresence(a)`).
        let presence_key = FactKey::MemberPresence {
            exporter: exporter.clone(),
            name: name.clone(),
            space,
        };
        observe_parse_fact_present(
            host,
            &resolved_canonical,
            cell,
            presence_key,
            FactLane::Semantic,
        );
        // Member: body fingerprint (full structural shape).
        let body_key = FactKey::Member {
            exporter,
            name,
            space,
        };
        observe_parse_fact_present(
            host,
            &resolved_canonical,
            cell,
            body_key,
            FactLane::Semantic,
        );
    }

    // 3. ModuleAugmentationIndexShape per consumed specifier (R29).
    //    Augmenters for `'vue'` etc. change the compiled prop /
    //    emit validation surface; observe the fingerprint so an
    //    augmenter-set churn invalidates dependent slots.
    observe_augmentation_fingerprints(host, script_imports, cell);
}

/// Emit a `ParseFactRef` observation against the producer's current
/// fact registry. The `expected_hash` is recovered from
/// `FileArtifactStore::get_artifacts_any(canonical_id).facts.lookup(&key)`
/// at the moment of cold-compute. If the fact is absent, observe
/// the sentinel `[0u8; 16]` so a later population (or its absence)
/// is still discriminating — the validator detects mismatch.
fn observe_parse_fact_present(
    host: &VerterHost,
    canonical_id: &str,
    cell: &FactReadSetCell,
    key: FactKey,
    lane: FactLane,
) {
    let expected_hash =
        lookup_parse_fact_hash(host, canonical_id, &key, lane).unwrap_or_else(zero_hash);
    cell.observe(FactVersionRef::Parse(ParseFactRef {
        canonical_id: canonical_id.to_string(),
        key,
        lane,
        expected_hash,
    }));
}

fn lookup_parse_fact_hash(
    host: &VerterHost,
    canonical_id: &str,
    key: &FactKey,
    lane: FactLane,
) -> Option<Hash16> {
    let artifacts = host
        .project_type_store
        .indexed()
        .get_artifacts_any(canonical_id)?;
    let fact = artifacts.facts.lookup(key)?;
    Some(match lane {
        FactLane::Semantic => fact.semantic_hash,
        FactLane::Display => fact.display_hash,
    })
}

fn zero_hash() -> Hash16 {
    [0u8; 16]
}

fn symbol_space_for_import(import: &AnalyzedImport) -> SymbolSpace {
    if import.is_type_only {
        SymbolSpace::Type
    } else {
        SymbolSpace::Value
    }
}

/// Resolve a script-import source specifier to a canonical id via
/// the owner's `import_routes` sub-mirror on `DerivedRawState`.
///
/// Returns `None` when the import is unresolved (typical for
/// external specifiers like `'vue'` without a workspace fallback);
/// the caller treats unresolved deps as "no fact observation" and
/// the compile cache still validates via the augmentation
/// fingerprint observation for augmented specifiers.
fn resolve_import_source_to_canonical(
    host: &VerterHost,
    canonical_id: &str,
    source: &str,
) -> Option<String> {
    let derived = host.derived_raw_cache().get(canonical_id)?;
    let resolution = derived.import_routes.get(source)?;
    resolution
        .resolved_canonical_id
        .clone()
        .or_else(|| resolution.effective_target().map(str::to_string))
}

/// Observe the augmentation-index fingerprint for every imported
/// specifier the owner consumes. The producer reads the augmenter
/// set when expanding `import X from 'spec'`; consuming macros
/// (defineProps / defineEmits) depend on the fingerprint via
/// `RouteSurfaceFactRef::ModuleAugmentationIndexShape` (R29).
fn observe_augmentation_fingerprints(
    host: &VerterHost,
    script_imports: &[AnalyzedImport],
    cell: &FactReadSetCell,
) {
    let snapshot = host
        .project_type_store
        .indexed()
        .snapshot_augmentation_index_fingerprints();
    if snapshot.is_empty() {
        return;
    }
    // Index snapshot for fast lookup: external_specifier → fingerprint.
    let by_external: FxHashSet<Arc<str>> = snapshot
        .iter()
        .filter_map(|(key, _)| match &key.target {
            crate::file_artifact_store::AugmentationTargetKind::ExternalSpecifier(spec) => {
                Some(Arc::clone(&spec.0))
            }
            _ => None,
        })
        .collect();
    let mut emitted = FxHashSet::default();
    for import in script_imports {
        if !by_external.contains(import.source.as_str()) {
            continue;
        }
        if !emitted.insert(import.source.clone()) {
            continue;
        }
        for (key, fingerprint) in &snapshot {
            let crate::file_artifact_store::AugmentationTargetKind::ExternalSpecifier(spec) =
                &key.target
            else {
                continue;
            };
            if spec.0.as_ref() != import.source.as_str() {
                continue;
            }
            let fact_key = FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
                external_specifier: Some(spec.clone()),
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            };
            cell.observe(FactVersionRef::RouteSurface(RouteSurfaceFactRef {
                canonical_id: import.source.clone(),
                key: fact_key,
                lane: FactLane::Semantic,
                expected_hash: *fingerprint,
            }));
        }
    }
}

/// Drain the active tracer into an immutable signature, returning
/// the empty signature when the tracer overflowed (per R20 1024-cap
/// — the caller refuses cache admission of overflowed signatures).
#[allow(dead_code)]
pub(crate) fn finalise_signature_or_empty(set: FactReadSet) -> Arc<[FactVersionRef]> {
    match set.finalise() {
        crate::resolver_core::FactReadSetFinalise::Ok(sig) => sig,
        crate::resolver_core::FactReadSetFinalise::Overflow => Arc::from(Vec::<_>::new()),
    }
}
