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
//! edits and silently re-roll on adding unrelated sibling members.
//! Path-precise observation emits one `Export(type_name)` (the
//! declaration body fingerprint) plus one
//! `MemberShape(exporter=type_name)` (member surface fingerprint)
//! plus one `MemberPresence(exporter=type_name, name=m)` per
//! enumerated member `m` of the referenced type. A cross-file edit
//! that adds, removes, or changes the consumed members invalidates
//! the consumer; an edit to an unrelated sibling type does not.
//!
//! ## Augmentation observation (R29)
//!
//! `<script setup>` consumers of `'vue'` (or any other augmented
//! module specifier) depend on the augmenter-set fingerprint. When
//! the dependent project ships a `declare module 'vue' { ... }`
//! augmentation, the compile output's prop-validation surface
//! changes; the augmentation-index fingerprint observation makes
//! the dependent slot invalidate when the augmenter set churns.
//!
//! ## Whole-hash observation for runtime imports + external `src=` deps
//!
//! A path-precise `Export` fact fingerprints a value declaration's
//! *signature*, not its body — `export function helper() { return 1 }`
//! and `{ return 2 }` carry the same `() => number` signature, so the
//! `Export` hash is identical across that edit. A *runtime* import
//! (`import { helper } from '@/utils'`) re-emits the dependency in the
//! assembled module and the compiled output's correctness depends on
//! the dependency's full content, not just its public type signature.
//! The producer therefore also observes `FileWholeHash` of the
//! resolved dependency for every value (non-type-only) import binding.
//!
//! Side-effect imports (`import './setup'`) carry no bindings but are
//! still runtime imports whose entire content is re-emitted in the
//! assembled module. The producer observes `FileWholeHash` of the
//! resolved dependency for every non-type-only import that has no
//! bindings, mirroring the per-binding admission — without this, an
//! SFC whose only cross-file dependency is a side-effect import
//! produces an empty `fact_dep_signature` that trivially validates.
//!
//! External `src=` blocks (`<template src="./tpl.html">`) are spliced
//! verbatim into the merged compile source by `merge_external_sources`
//! — the entire external file content lands in the compiled output.
//! The producer observes `FileWholeHash` of each resolved external
//! source canonical so any edit to it invalidates the dependent slot.
//! Without this, an SFC whose only cross-file dependency is an external
//! `src=` block produces a completely empty `fact_dep_signature`, which
//! trivially validates forever once eager invalidation is removed.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_semantic::analysis::types::AnalyzedImport;
use verter_semantic::analysis::MacroTypeDep;
use verter_semantic::facts::registry::{
    AugmentationTargetKindTag, FactKey, InternedName, InternedSpecifier, SymbolSpace,
};
use verter_semantic::facts::FactLane;

use crate::resolver_core::{FactReadSet, FactVersionRef, ParseFactRef, RouteSurfaceFactRef};
use crate::types::{ExternalSourceRequest, Hash16};
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
    external_requests: &[ExternalSourceRequest],
) {
    // 1. Per-import `ImportRef` observation (R12 — parse-domain
    //    fact carrying the unresolved binding shape on the OWNER's
    //    file). Adding / removing the binding on the owner side
    //    invalidates the compile output that references it.
    let mut seen_imports = FxHashSet::default();
    // Resolved deps reached through a value (runtime) import binding —
    // each one needs a `FileWholeHash` observation (step 1b below).
    let mut runtime_dep_canonicals = FxHashSet::default();
    for import in script_imports {
        let space = symbol_space_for_import(import);
        let specifier = InternedSpecifier::from(import.source.as_str());
        let resolved_dep =
            resolve_import_source_to_canonical(host, canonical_id, import.source.as_str());
        for binding in import.bindings.iter() {
            let local = binding.name.clone();
            // A binding is a *value* (runtime) binding iff neither the
            // declaration-level `import type { ... }` nor the
            // per-specifier `import { type X }` form marks it type-only.
            let binding_is_value = !import.is_type_only && !binding.is_type_only;
            if !seen_imports.insert((specifier.clone(), local.clone(), space)) {
                continue;
            }
            let key = FactKey::ImportRef {
                specifier: specifier.clone(),
                binding: InternedName::from(local.as_str()),
                space,
            };
            observe_parse_fact_present(host, canonical_id, key, FactLane::Semantic);

            // R28 path-precise cross-file: also observe the
            // `Export(binding, space)` fact on the resolved dep so
            // that an edit to the imported declaration's body
            // invalidates the consumer's compile slot. The owner's
            // `ImportRef` only fingerprints the import shape on the
            // owner — the dep-side `Export` fact carries the
            // declaration's body fingerprint. Path-precise: editing
            // a sibling export in the dep does not invalidate this
            // consumer.
            if let Some(resolved_dep) = resolved_dep.as_deref() {
                let dep_export_key = FactKey::Export {
                    name: InternedName::from(local.as_str()),
                    space,
                };
                observe_parse_fact_present(host, resolved_dep, dep_export_key, FactLane::Semantic);

                // Runtime imports: the `Export` fact above is
                // signature-pinned (a value declaration hashes its
                // signature, not its body), so a body-only edit to
                // the imported module would NOT invalidate the
                // consumer through `Export` alone. The compiled
                // output re-emits the runtime dependency, so its
                // full content matters — record the dep for a
                // whole-hash observation.
                if binding_is_value && resolved_dep != canonical_id {
                    runtime_dep_canonicals.insert(resolved_dep.to_string());
                }
            }
        }

        // Side-effect imports (`import './setup'`) carry ZERO
        // bindings, so the per-binding loop above never runs and no
        // whole-hash dep is recorded for them. They are still
        // *runtime* imports — `is_type_only == false` — whose entire
        // content is re-emitted in the assembled module, so an edit
        // to the imported file changes the compiled output. Record
        // the resolved dep for a whole-hash observation whenever the
        // declaration is non-type-only and has no bindings, mirroring
        // the per-binding `binding_is_value` admission above.
        if import.bindings.is_empty() && !import.is_type_only {
            if let Some(resolved_dep) = resolved_dep.as_deref() {
                if resolved_dep != canonical_id {
                    runtime_dep_canonicals.insert(resolved_dep.to_string());
                }
            }
        }
    }

    // 1b. `FileWholeHash` observation for each runtime-imported dep.
    //     Any edit to the runtime module (including a body-only edit
    //     the signature-pinned `Export` fact cannot see) invalidates
    //     the dependent compile slot.
    for dep_canonical in &runtime_dep_canonicals {
        observe_file_whole_hash(host, dep_canonical);
    }

    // 2. Per-`macro_type_deps` cross-file observation. The macro
    //    consumer reads the referenced type's body to derive the
    //    prop / emit / slot validation surface. The producer
    //    therefore observes:
    //
    //    - `Export(type_name, Type)` — the body fingerprint of the
    //      referenced type's declaration. Changes when the type
    //      body changes.
    //    - `MemberShape(exporter=type_name, Type)` — the member
    //      surface of the referenced type. Changes when a sibling
    //      member is added or removed.
    //    - One `MemberPresence(exporter=type_name, name=m, Type)`
    //      per existing member `m`, enumerated from the
    //      dependency's `FactRegistry`. R28 path-precision:
    //      editing one member's header (kind / readonly /
    //      optional) invalidates exactly the consumer of that
    //      member; the `MemberShape` observation pins
    //      add / remove churn.
    let mut seen_deps = FxHashSet::default();
    for dep in macro_type_deps {
        // Resolve `dep.import_source` to a canonical via the
        // owner's import_routes (DerivedRawState sub-mirror).
        let Some(resolved_canonical) =
            resolve_import_source_to_canonical(host, canonical_id, &dep.import_source)
        else {
            continue;
        };
        let space = SymbolSpace::Type;
        let type_name = InternedName::from(dep.type_name.as_str());
        if !seen_deps.insert((resolved_canonical.clone(), type_name.clone(), space)) {
            continue;
        }

        // Export(type_name) — declaration body fingerprint.
        let export_key = FactKey::Export {
            name: type_name.clone(),
            space,
        };
        observe_parse_fact_present(host, &resolved_canonical, export_key, FactLane::Semantic);

        // MemberShape(exporter=type_name) — member surface
        // fingerprint. Changes on sibling add / remove.
        let shape_key = FactKey::MemberShape {
            exporter: type_name.clone(),
            space,
        };
        observe_parse_fact_present(host, &resolved_canonical, shape_key, FactLane::Semantic);

        // Enumerate the type's existing members from the
        // dependency's FactRegistry and emit one
        // `MemberPresence(exporter=type_name, name=m)` per member.
        observe_member_presences_for_export(host, &resolved_canonical, &type_name, space);
    }

    // 3. ModuleAugmentationIndexShape per consumed specifier (R29).
    //    Augmenters for `'vue'` etc. change the compiled prop /
    //    emit validation surface; observe the fingerprint so an
    //    augmenter-set churn invalidates dependent slots.
    observe_augmentation_fingerprints(host, script_imports);

    // 4. External `src=` block deps. `merge_external_sources` splices
    //    the WHOLE content of each external file verbatim into the
    //    merged compile source, so any edit to an external template /
    //    script / style / custom block invalidates the dependent
    //    compile output. Whole-hash is the correct granularity — the
    //    entire file is embedded, not a path-precise slice. Each
    //    external request is resolved through the owner's import-route
    //    sub-mirror (populated by the cold-compute prefetch) with the
    //    request's own pre-resolved canonical as the fallback.
    let mut seen_external = FxHashSet::default();
    for request in external_requests {
        let resolved = resolve_import_source_to_canonical(host, canonical_id, &request.specifier)
            .unwrap_or_else(|| request.resolved_canonical_id.clone());
        if resolved.is_empty() || resolved == canonical_id {
            continue;
        }
        if !seen_external.insert(resolved.clone()) {
            continue;
        }
        observe_file_whole_hash(host, &resolved);
    }
}

/// Observe a `FactVersionRef::FileWholeHash` for `canonical_id`
/// against the host's current scheduler whole-hash. Used by the
/// compile-tier producer for runtime imports and external `src=`
/// dependency files, whose ENTIRE content (not just a path-precise
/// type slice) influences the compiled output.
///
/// When the whole-hash is unavailable (file not loaded, unresolvable
/// specifier) the observation is skipped — consistent with the
/// `observe_parse_fact_present` skip-on-miss contract: a consumer
/// must never depend on a fact the producer cannot publish.
fn observe_file_whole_hash(host: &VerterHost, canonical_id: &str) {
    let Some(whole_hash) = host.current_or_read_whole_hash(canonical_id) else {
        return;
    };
    crate::resolver_core::resolver_context::observe_fan_out(FactVersionRef::FileWholeHash {
        canonical_id: canonical_id.to_string(),
        hash: whole_hash,
    });
}

/// Emit a `ParseFactRef` observation against the producer's current
/// fact registry. The `expected_hash` is recovered from
/// `FileArtifactStore::get_artifacts_any(canonical_id).facts.lookup(&key)`
/// at the moment of cold-compute.
///
/// Two outcomes:
///
/// - Hash present → observe `FactVersionRef::Parse` with the real
///   hash. A later mismatch invalidates the consumer.
/// - Hash absent (file not in artifact store, fact not emitted) →
///   DO NOT OBSERVE. The cold-compute pre-tracer prefetch in
///   `VerterHost::prefetch_compile_tier_observation_targets` is
///   the producer-timing contract that drives dependency
///   artifacts to indexed-ready before this point. When the
///   prefetch could not resolve / load the dep (external
///   specifier without workspace fallback, deleted file, etc.)
///   the observation is conservatively skipped so a consumer
///   never depends on a fact that the producer cannot publish.
fn observe_parse_fact_present(host: &VerterHost, canonical_id: &str, key: FactKey, lane: FactLane) {
    let Some(expected_hash) = lookup_parse_fact_hash(host, canonical_id, &key, lane) else {
        return;
    };
    crate::resolver_core::resolver_context::observe_fan_out(FactVersionRef::Parse(ParseFactRef {
        canonical_id: canonical_id.to_string(),
        key,
        lane,
        expected_hash,
    }));
}

/// Enumerate every `MemberPresence(exporter, *, space)` fact in the
/// dep's `FileFacts` and observe each one through the tracer. Used
/// by the compile-tier producer to record path-precise member
/// presence observations for each cross-file macro type dep.
///
/// If the dep's `FileArtifacts` is not yet in the project store the
/// enumeration is a no-op (the pre-tracer prefetch should have
/// driven it to indexed-ready, but a stale-load fallback still
/// holds for unresolvable specifiers).
fn observe_member_presences_for_export(
    host: &VerterHost,
    canonical_id: &str,
    exporter: &InternedName,
    space: SymbolSpace,
) {
    let Some(artifacts) = host
        .project_type_store()
        .indexed()
        .get_artifacts_any(canonical_id)
    else {
        return;
    };
    for (key, _) in artifacts.facts.registry().iter() {
        if let FactKey::MemberPresence {
            exporter: ex,
            name,
            space: sp,
        } = key
        {
            if ex == exporter && *sp == space {
                let presence_key = FactKey::MemberPresence {
                    exporter: ex.clone(),
                    name: name.clone(),
                    space: *sp,
                };
                observe_parse_fact_present(host, canonical_id, presence_key, FactLane::Semantic);
            }
        }
    }
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
fn observe_augmentation_fingerprints(host: &VerterHost, script_imports: &[AnalyzedImport]) {
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
            crate::resolver_core::resolver_context::observe_fan_out(FactVersionRef::RouteSurface(
                RouteSurfaceFactRef {
                    canonical_id: import.source.clone(),
                    key: fact_key,
                    lane: FactLane::Semantic,
                    expected_hash: *fingerprint,
                },
            ));
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
