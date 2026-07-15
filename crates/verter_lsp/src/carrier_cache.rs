//! The corrected, SPLIT carrier cache (§2.7 of the external-TS-engine
//! architecture).
//!
//! The durable performance win over a per-request regenerator (the external
//! reference) is doing less work AROUND the checker, with a hard line: Verter
//! may skip carrier RE-CODEGEN and redundant transport, but it must NEVER
//! suppress an engine re-check that a type change requires. The two decisions
//! are SPLIT into two independent keys/states that this module owns:
//!
//! ## (a) Carrier *regeneration* skip — Verter's decision, SAFE
//!
//! [`RegenKey`] is keyed ONLY by the orthogonal env dimensions the carrier TEXT
//! depends on (R21 — no single bundled hash): the source content hash, the
//! parse-env hash, the codegen compile-profile hash, the per-file language-row
//! hash, and the helper/runtime version. If this key is unchanged, the carrier
//! text is byte-stable, so Verter reuses the cached carrier and does NOT
//! re-send its text to the engine. "Carrier-hash-stable" means *reuse the
//! cached text* — it does **NOT** mean "the type result is still valid".
//!
//! ## (b) Engine re-check / notification — the engine's decision, NEVER suppressed
//!
//! Once a carrier is a real configured-project member, the engine's Program owns
//! cross-file type invalidation. A carrier can be byte-identical while its
//! *types* changed — a dependency `.d.ts` edit, a sibling carrier's public-API
//! surface change, an `@types`/`lib`/tsconfig change, a `paths` remap, a package
//! upgrade. [`EngineRecheckState`] therefore tracks TWO dependency-closure
//! signals that are computed from the DEPENDENCY GRAPH, never from carrier text:
//! the carrier's resolved-import-surface hash ([`EngineRecheckState::import_signature_hash`])
//! and its dependency-closure generation ([`EngineRecheckState::closure_generation`],
//! the max per-canonical content-transition generation over the resolved
//! forward-dependency closure). [`needs_engine_recheck`] notifies the engine when
//! EITHER advanced — *even though the carrier text (a) is cache-stable*. This is
//! the load-bearing discriminator: a self-content-only cache that gated the
//! re-check on carrier-text stability would WRONGLY skip the dependency `.d.ts`
//! case. This mirrors the project's fact-based-cache discipline (`ReadSetSignature`
//! — cache correctness is read-side authoritative; never cache on path alone).
//!
//! ## Map-result caching — keyed by `map_hash`
//!
//! A `CodeTransform`-built source-map is derived from the same chunk list that
//! produces the text, so `map_hash` is part of the version-gate identity: a
//! `map_hash` change invalidates EVERY cached MAPPED result keyed by the old
//! map ([`mapped_results_valid`]). Raw engine results are cached by
//! `(carrier content_hash, project epoch)`; mapped results by `map_hash`. On a
//! map-only change, mapped outputs are dropped and raw spans are re-mapped
//! through the new map ONLY IF the carrier content and project epoch still
//! match — never remap a stale diagnostic through a new map.
//!
//! This module is PURE (no live host): every predicate is a comparison over
//! recorded vs live values. The live values are SOURCED by the store/sync layer
//! from the shared `WorkspaceRead` view (`content_generation`,
//! `last_content_transition_generation`, `forward_deps_for`) and the host env
//! readers; this keeps the cache logic unit-testable without a provider and
//! reuses the shared owner-layer generation rails rather than a parallel ledger.

use verter_semantic::analysis::types::Hash16;

/// The self-content regeneration key (§2.7(a)). Keyed ONLY by the orthogonal
/// dimensions the carrier TEXT depends on — NEVER a dependency-closure signal
/// (those belong to [`EngineRecheckState`]) and NEVER a single bundled hash
/// (R21). Two carriers with an equal `RegenKey` have byte-identical text, so the
/// cached carrier is reused without re-codegen or re-send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegenKey {
    /// Hash of the carrier SOURCE content (the `.vue`/`.svelte` bytes).
    pub source_content_hash: Hash16,
    /// Parse-environment hash (parser options affecting the AST shape).
    pub parse_env_hash: Hash16,
    /// The codegen compile-profile hash. Distinct profiles produce DIFFERENT text
    /// and therefore DIFFERENT keys for the same source.
    pub compile_profile_hash: u64,
    /// The per-file `FileLanguage` row hash (the R21 per-file classification
    /// dimension of artifact identity).
    pub file_language_row_hash: Hash16,
    /// The helper/runtime version the codegen targets.
    pub helper_runtime_version: u32,
}

impl RegenKey {
    /// Whether a carrier built under `cached` is text-fresh for the `live` key —
    /// i.e. the carrier need NOT be regenerated or re-sent to the engine. This
    /// is the (a) lever ONLY: a `true` here means "reuse the cached carrier
    /// text", it does NOT assert the engine result is still valid (that is
    /// [`needs_engine_recheck`]).
    #[must_use]
    pub fn carrier_regeneration_is_fresh(cached: &RegenKey, live: &RegenKey) -> bool {
        cached == live
    }
}

/// The dependency-driven engine-recheck state (§2.7(b)). All fields are computed
/// from the DEPENDENCY GRAPH and the PROJECT/ENV rails, never from carrier text,
/// so a re-check fires for a dependency-only or project-config-only change with
/// byte-identical carrier text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineRecheckState {
    /// Hash of the carrier's RESOLVED import surface (the canonical IDs +
    /// per-target content identity the carrier's imports resolve to). A
    /// structural change to what the imports resolve to (an added/removed
    /// import, a `paths` remap, a target whose content identity changed) moves
    /// this hash even when the carrier text is unchanged.
    pub import_signature_hash: Hash16,
    /// The dependency-closure generation: the MAX per-canonical
    /// `last_content_transition_generation` over the carrier's resolved
    /// forward-dependency closure (and the carrier's own source). A dependency
    /// `.d.ts`-only edit advances the workspace content generation and that
    /// dependency's per-canonical transition generation, so this value advances
    /// even though the dependent carrier's TEXT is byte-identical.
    pub closure_generation: u64,
    /// The PROJECT/ENV recheck generation — the rail for type-observable changes
    /// that are NOT a dependency-file content transition: an `@types`/`lib`
    /// change, a `tsconfig` edit (compiler options, `paths`), a package upgrade,
    /// or a project-identity flip (§2.7(b) names all of these as engine-recheck
    /// triggers). It folds the project's `resolve_env_hash` + `lib_env_hash` +
    /// `project_identity` (the env dims the engine result depends on) into a
    /// single monotone-or-distinct discriminant. A change here re-checks the
    /// dependent even when its imports resolve to the SAME canonicals and no
    /// dependency file content transitioned — the gap a closure-generation-only
    /// state would miss.
    pub project_recheck_generation: u64,
}

/// Whether the engine MUST be notified to re-check a carrier, given the
/// `cached` recheck state the carrier was last published under and the `live`
/// recheck state recomputed from the current dependency closure + project env.
///
/// The CORE of §2.7(b): the decision is driven SOLELY by dependency-closure and
/// project/env signals. It returns `true` when ANY of: the resolved import
/// signature changed, the dependency-closure generation advanced, OR the
/// project/env recheck generation CHANGED (in EITHER direction — a config
/// rollback is still a change that can flip diagnostics, so it fails toward a
/// re-check) — and it NEVER consults carrier text. A `RegenKey`-stable carrier
/// (byte-identical text) whose dependency `.d.ts` OR whose `tsconfig`/`lib`
/// changed STILL returns `true` here. This is the load-bearing dependency-change
/// discriminator: a self-content-only cache (one that gated the re-check on
/// carrier-text/`RegenKey` stability) would WRONGLY return `false` for the
/// dependency-only or config-only change and silently serve a stale diagnostic;
/// this function returns `true`.
#[must_use]
pub fn needs_engine_recheck(cached: &EngineRecheckState, live: &EngineRecheckState) -> bool {
    cached.import_signature_hash != live.import_signature_hash
        || live.closure_generation > cached.closure_generation
        || cached.project_recheck_generation != live.project_recheck_generation
}

/// Whether the MAPPED results cached under `cached_map_hash` are still valid for
/// the `live_map_hash` (§2.7). A `map_hash` change invalidates every cached
/// mapped result keyed by the old map: mapped outputs are dropped and raw spans
/// must be re-mapped through the new map (only if the carrier content + project
/// epoch still match — that gate is the caller's). Never remap a stale
/// diagnostic through a new map.
#[must_use]
pub fn mapped_results_valid(cached_map_hash: Hash16, live_map_hash: Hash16) -> bool {
    cached_map_hash == live_map_hash
}

/// Fold a set of `Hash16` env-dimension values plus scalar dims into a single
/// `Hash16` digest, for callers that need a compact composite cache key. Uses
/// blake3 (consistent with the store's content-hash identity) over the
/// concatenation of every dimension in a FIXED order (the concatenate-then-hash
/// pattern the rest of the codebase uses for multi-dimensional keys — never XOR,
/// which would collide a swapped pair). The order is part of the identity: do
/// not reorder.
#[must_use]
pub fn fold_regen_key(key: &RegenKey) -> Hash16 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&key.source_content_hash);
    hasher.update(&key.parse_env_hash);
    hasher.update(&key.compile_profile_hash.to_le_bytes());
    hasher.update(&key.file_language_row_hash);
    hasher.update(&key.helper_runtime_version.to_le_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest.as_bytes()[..16]);
    out
}

/// Compute the dependency-closure generation for a carrier: the MAX per-canonical
/// `last_content_transition_generation` over the carrier's own canonical plus its
/// resolved forward-dependency closure. This is the (b) `closure_generation`
/// producer; it reads the SHARED `WorkspaceRead` generation rails rather than a
/// parallel ledger.
///
/// `self_canonical` is the carrier source's canonical id; `forward_closure` is
/// the resolved forward-dependency canonical set (from `WorkspaceRead::forward_deps_for`,
/// transitively expanded by the caller if a transitive closure is required).
/// `transition_gen` is `WorkspaceRead::last_content_transition_generation` bound
/// to the live view. Taking the MAX means ANY dependency's content transition
/// (a `.d.ts` edit) advances the closure generation, which [`needs_engine_recheck`]
/// then observes.
#[must_use]
pub fn closure_generation_for<'a>(
    self_canonical: &str,
    forward_closure: impl IntoIterator<Item = &'a str>,
    transition_gen: impl Fn(&str) -> u64,
) -> u64 {
    let mut max_gen = transition_gen(self_canonical);
    for dep in forward_closure {
        max_gen = max_gen.max(transition_gen(dep));
    }
    max_gen
}

/// Hash a carrier's resolved import surface into an [`EngineRecheckState::import_signature_hash`].
/// The surface is the list of `(local_binding, resolved_canonical, target_content_identity)`
/// triples the carrier's imports resolve to. blake3 over the concatenation of
/// each triple in the caller-provided order (the caller sorts for stability);
/// any structural change (added/removed import, a remap, a target whose content
/// identity moved) changes the hash. Reuses the resolved import facts the host
/// already tracks rather than re-resolving imports here.
#[must_use]
pub fn import_signature_hash<'a>(
    resolved_imports: impl IntoIterator<Item = (&'a str, &'a str, Hash16)>,
) -> Hash16 {
    let mut hasher = blake3::Hasher::new();
    for (binding, canonical, target_identity) in resolved_imports {
        hasher.update(binding.as_bytes());
        hasher.update(&[0u8]); // delimiter — keep field boundaries unambiguous
        hasher.update(canonical.as_bytes());
        hasher.update(&[0u8]);
        hasher.update(&target_identity);
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest.as_bytes()[..16]);
    out
}

/// Fold the project/env rails into an [`EngineRecheckState::project_recheck_generation`]
/// discriminant. The inputs are the env dimensions the engine RESULT depends on
/// per the fact-based-cache R21 split — `resolve_env_hash` (`paths`/`baseUrl`/
/// `moduleResolution`), `lib_env_hash` (`lib`/`target`/`@types`), and
/// `project_identity` (the configured-Program identity) — plus a workspace-level
/// `project_config_generation` that advances when the owning `tsconfig` (or its
/// `extends` chain) changes. Any change in ANY of these is a type-observable
/// project-config change that §2.7(b) requires a re-check for, even with a
/// byte-identical carrier and an unchanged dependency closure.
///
/// The fold is blake3 over the concatenation (fixed order — concat-then-hash, not
/// XOR), truncated to a `u64` discriminant. The recheck predicate compares it for
/// INEQUALITY (not `>`), so a config ROLLBACK (which could re-introduce a
/// diagnostic) still re-checks — fail toward correctness.
#[must_use]
pub fn project_recheck_generation_from(
    resolve_env_hash: Hash16,
    lib_env_hash: Hash16,
    project_identity: Hash16,
    project_config_generation: u64,
) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&resolve_env_hash);
    hasher.update(&lib_env_hash);
    hasher.update(&project_identity);
    hasher.update(&project_config_generation.to_le_bytes());
    let digest = hasher.finalize();
    let mut head = [0u8; 8];
    head.copy_from_slice(&digest.as_bytes()[..8]);
    u64::from_le_bytes(head)
}

#[cfg(test)]
#[path = "carrier_cache_tests.rs"]
mod tests;
