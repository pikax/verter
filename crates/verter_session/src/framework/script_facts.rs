#![deny(missing_docs)]
//! The resolved-validation half of the framework script-fact seam.
//!
//! `verter_semantic` owns the syntax-capture half (the [`ScriptFactProvider`]
//! trait, its syntax-only `capture`, the per-file parse-domain candidate
//! collection). This module owns the session-side resolved-validation half: it
//! takes a file's captured candidates and validates them against resolved
//! import sources + derived capability bits, producing the typed resolved facts
//! an adapter retrieves through
//! [`FrameworkAdapterCtx::script_facts_for`](crate::framework::ctx::FrameworkAdapterCtx::script_facts_for).
//!
//! Two stores back the half, both keyed exactly as the cache architecture
//! requires:
//! - [`FrameworkScriptCandidateStore`] — a CONTENT-ADDRESSED artifact store. Its
//!   key carries the file's content/version dimensions
//!   (`canonical`, `content_hash`, `parse_env_hash`, `parser_version`,
//!   `file_language_id`) plus `(provider_id, provider_version)`. A content edit
//!   or a provider upgrade misses the stale slot (the content-addressed
//!   family).
//! - [`FrameworkScriptFactStore`] — the RESOLVED-fact store. Its sub-key is
//!   `(canonical, provider_id, provider_version, consumed_capability_bits,
//!   project_identity, resolve_env_hash)` — NO `lib_env_hash` / `type_env_hash`
//!   (the resolved facts do not depend on lib/type data). Warm reads pass TWO
//!   gates: a strict same-generation gate AND a `ReadSetSignature.facts`
//!   validation against the caller's live view; publication is ONLY via
//!   [`SignatureAdmission::Cacheable`] — an overflowed signature returns the
//!   computed value to the caller alone and never warms the store.
//!
//! Svelte registers the production provider; Vue's macro analysis stays inside
//! the shallow pass. A registration without a provider returns an explicit
//! not-applicable outcome without per-file work.

use std::sync::Arc;

use dashmap::DashMap;
use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;
use verter_language::FrameworkAdapterId;
use verter_semantic::analysis::framework_facts::{
    ExactFrameworkScriptCandidates, FrameworkScriptCandidates, FrameworkScriptFactPayload,
    ResolvedImportTarget, ResolvedPackage, ResolvedValidationCx, ScriptCandidateCx,
    ScriptFactProvider, ScriptFactValidation,
};
pub use verter_semantic::analysis::framework_facts::{
    ScriptFactPartialReason, ScriptFactUnavailableReason,
};

use crate::cache_runtime::SignatureAdmission;
use crate::fact_signature_helpers::{
    named_cacheability_scope, named_fact_tracer, ReadSetSignature, ReadSetSignatureExt as _,
};
use crate::framework::registry::FrameworkRegistration;
use crate::resolver_core::{ResolverContext, StoreView};
use crate::types::Hash16;
use crate::VerterHost;

/// Producer-minted exact script facts.
///
/// The constructor is private to this module. Consumers can inspect the facts,
/// but cannot turn an arbitrary payload into an exact proof.
pub struct ExactScriptFacts<T: ?Sized> {
    facts: Arc<T>,
}

impl<T: ?Sized> Clone for ExactScriptFacts<T> {
    fn clone(&self) -> Self {
        Self {
            facts: Arc::clone(&self.facts),
        }
    }
}

impl<T: ?Sized> ExactScriptFacts<T> {
    fn new(facts: Arc<T>) -> Self {
        Self { facts }
    }

    /// The complete fact payload.
    #[must_use]
    pub fn facts(&self) -> &T {
        &self.facts
    }
}

impl ExactScriptFacts<dyn FrameworkScriptFactPayload> {
    fn downcast<T: FrameworkScriptFactPayload>(self) -> Option<ExactScriptFacts<T>> {
        self.facts
            .as_any_arc()
            .downcast::<T>()
            .ok()
            .map(ExactScriptFacts::new)
    }
}

/// Usable positive script facts whose inventory is incomplete.
///
/// This type deliberately has no `Deref`, collection, iteration, negative
/// lookup, or
/// [`verter_semantic::analysis::framework_facts::NegativeEvidence`]
/// implementation.
#[derive(Clone)]
pub struct PartialScriptFacts<T> {
    facts: Arc<T>,
    reason: ScriptFactPartialReason,
    syntax_completeness: PartialSyntaxCompleteness,
}

impl<T> PartialScriptFacts<T> {
    fn new(
        facts: Arc<T>,
        reason: ScriptFactPartialReason,
        syntax_completeness: PartialSyntaxCompleteness,
    ) -> Self {
        Self {
            facts,
            reason,
            syntax_completeness,
        }
    }

    /// Why absence cannot be inferred from this payload.
    #[must_use]
    pub fn reason(&self) -> ScriptFactPartialReason {
        self.reason
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartialSyntaxCompleteness {
    Exact,
    Recovered,
}

/// A possibly-present positive observation.
///
/// The value can only be visited when present. An omitted callback is not
/// evidence of absence, so this type deliberately exposes no `Option`, `Deref`,
/// or negative lookup projection.
#[derive(Clone, Copy)]
pub struct ConservativeObservation<'a, T: ?Sized> {
    value: Option<&'a T>,
}

impl<'a, T: ?Sized> ConservativeObservation<'a, T> {
    fn new(value: Option<&'a T>) -> Self {
        Self { value }
    }

    /// Visit the positive observation when one was captured.
    pub fn visit(self, visitor: impl FnOnce(&'a T)) {
        if let Some(value) = self.value {
            visitor(value);
        }
    }
}

/// A positive-only observation inventory whose empty iteration is not an
/// authoritative negative.
///
/// The callback surface intentionally provides no iterator, collection, or
/// [`verter_semantic::analysis::framework_facts::NegativeEvidence`]
/// implementation.
#[derive(Clone, Copy)]
pub struct ConservativeObservations<'a, T> {
    values: &'a [T],
}

/// Positive-only resolved Svelte snippet-import identities.
///
/// Unresolved route evidence is deliberately not visitable through this
/// partial-evidence surface, and the wrapper exposes no iterator or
/// exact-empty operation.
#[derive(Clone, Copy)]
pub struct ConservativeResolvedSnippetImports<'a> {
    values: &'a [verter_type_expr::facts::SvelteSnippetImportFact],
}

impl<'a> ConservativeResolvedSnippetImports<'a> {
    fn new(values: &'a [verter_type_expr::facts::SvelteSnippetImportFact]) -> Self {
        Self { values }
    }

    /// Visit each positively resolved snippet import.
    pub fn visit(
        self,
        mut visitor: impl FnMut(&'a verter_type_expr::facts::SvelteSnippetImportFact),
    ) {
        for value in self.values {
            if matches!(
                value,
                verter_type_expr::facts::SvelteSnippetImportFact::Resolved { .. }
            ) {
                visitor(value);
            }
        }
    }
}

impl<'a, T> ConservativeObservations<'a, T> {
    fn new(values: &'a [T]) -> Self {
        Self { values }
    }

    /// Visit each positive observation captured from the incomplete source.
    pub fn visit(self, mut visitor: impl FnMut(&'a T)) {
        for value in self.values {
            visitor(value);
        }
    }
}

/// Positive-only Svelte observations from a partial script-fact payload.
///
/// Every field is exposed through [`ConservativeObservation`] or
/// [`ConservativeObservations`]. Consumers can retain captured positives but
/// cannot obtain the whole [`SvelteScriptFacts`](verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts),
/// its exact props-call inventory, or an exact-empty proof.
#[derive(Clone, Copy)]
pub struct ConservativeSvelteScriptObservations<'a> {
    facts: &'a verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
}

impl<'a> ConservativeSvelteScriptObservations<'a> {
    /// The observed authored props type, when captured.
    #[must_use]
    pub fn props_type(
        self,
    ) -> ConservativeObservation<'a, verter_type_expr::locators::AuthoredTypePayloadRef> {
        ConservativeObservation::new(self.facts.syntax().props_type.as_ref())
    }

    /// Observed props defaults.
    #[must_use]
    pub fn prop_defaults(
        self,
    ) -> ConservativeObservations<'a, verter_semantic::analysis::types::AnalyzedDefaultValue> {
        ConservativeObservations::new(&self.facts.syntax().prop_defaults)
    }

    /// Observed `$bindable()` member names.
    #[must_use]
    pub fn bindable_members(self) -> ConservativeObservations<'a, String> {
        ConservativeObservations::new(&self.facts.syntax().bindable_members)
    }

    /// Observed legacy `export let` props.
    #[must_use]
    pub fn legacy_props(
        self,
    ) -> ConservativeObservations<
        'a,
        verter_semantic::analysis::framework_facts::svelte::SvelteLegacyProp,
    > {
        ConservativeObservations::new(&self.facts.syntax().legacy_props)
    }

    /// Observed instance-script exports.
    #[must_use]
    pub fn instance_exports(
        self,
    ) -> ConservativeObservations<
        'a,
        verter_semantic::analysis::framework_facts::svelte::SvelteInstanceExport,
    > {
        ConservativeObservations::new(&self.facts.syntax().instance_exports)
    }

    /// Observed module-script exports.
    #[must_use]
    pub fn module_exports(
        self,
    ) -> ConservativeObservations<'a, verter_type_expr::facts::SvelteModuleExportFact> {
        ConservativeObservations::new(&self.facts.syntax().module_exports)
    }

    /// Observed `$props()` calls.
    #[must_use]
    pub fn props_calls(
        self,
    ) -> ConservativeObservations<
        'a,
        verter_semantic::analysis::framework_facts::svelte::SveltePropsCall,
    > {
        ConservativeObservations::new(
            verter_semantic::analysis::framework_facts::NegativeEvidence::observations(
                self.facts.syntax().props_calls(),
            ),
        )
    }

    /// Observed provenance-validated snippet member names.
    #[must_use]
    pub fn validated_snippet_members(self) -> ConservativeObservations<'a, String> {
        ConservativeObservations::new(&self.facts.resolution().validated_snippet_members)
    }

    /// Positively resolved snippet-import identities.
    #[must_use]
    pub fn resolved_snippet_imports(self) -> ConservativeResolvedSnippetImports<'a> {
        ConservativeResolvedSnippetImports::new(&self.facts.resolution().snippet_imports)
    }

    /// The observed provenance-validated dispatcher payload, when captured.
    #[must_use]
    pub fn dispatcher_events(
        self,
    ) -> ConservativeObservation<'a, verter_type_expr::locators::AuthoredTypePayloadRef> {
        ConservativeObservation::new(self.facts.resolution().dispatcher_events.as_ref())
    }
}

impl PartialScriptFacts<verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts> {
    /// Exact syntax facts when parsing completed without recovery.
    ///
    /// Resolution-only partiality preserves this channel. A recovered parse
    /// returns `None`, so absence-sensitive consumers cannot obtain
    /// [`ExactSveltePropsCalls`](verter_semantic::analysis::framework_facts::svelte::ExactSveltePropsCalls).
    #[must_use]
    pub fn exact_syntax(
        &self,
    ) -> Option<&verter_semantic::analysis::framework_facts::svelte::SvelteScriptSyntaxFacts> {
        matches!(self.syntax_completeness, PartialSyntaxCompleteness::Exact)
            .then(|| self.facts.syntax())
    }

    /// Positive Svelte observations without any whole-payload or exact-empty
    /// projection.
    #[must_use]
    pub fn conservative_svelte_observations(&self) -> ConservativeSvelteScriptObservations<'_> {
        ConservativeSvelteScriptObservations { facts: &self.facts }
    }
}

/// An unavailable script-fact state carrying no fact inventory.
///
/// This type deliberately has no `Deref`, collection, iteration, negative
/// lookup, or
/// [`verter_semantic::analysis::framework_facts::NegativeEvidence`]
/// implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnavailableScriptFacts {
    reason: ScriptFactUnavailableReason,
}

impl UnavailableScriptFacts {
    fn new(reason: ScriptFactUnavailableReason) -> Self {
        Self { reason }
    }

    /// Why no reliable fact payload was produced.
    #[must_use]
    pub fn reason(self) -> ScriptFactUnavailableReason {
        self.reason
    }
}

/// Why script facts do not apply to the selected adapter/file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptFactNotApplicableReason {
    /// The adapter registration has no script-fact provider.
    ProviderNotRegistered,
    /// No registered provider's exact syntax gate matched the file.
    ProviderGateMiss,
}

/// A proven not-applicable script-fact state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotApplicableScriptFacts {
    reason: ScriptFactNotApplicableReason,
}

impl NotApplicableScriptFacts {
    fn new(reason: ScriptFactNotApplicableReason) -> Self {
        Self { reason }
    }

    /// Why the script-fact domain does not apply.
    #[must_use]
    pub fn reason(self) -> ScriptFactNotApplicableReason {
        self.reason
    }
}

/// The consumer-facing script-fact evidence family.
///
/// Consumers must exhaustively distinguish exact, partial, unavailable, and
/// not-applicable states; there is no whole-evidence `Option` projection.
#[must_use]
pub enum ScriptFactEvidence<T> {
    /// A complete fact payload, possibly exactly empty.
    Exact(ExactScriptFacts<T>),
    /// Usable positive observations that cannot prove every negative.
    Partial(PartialScriptFacts<T>),
    /// No reliable observations.
    Unavailable(UnavailableScriptFacts),
    /// The adapter/file selection proves the domain does not apply.
    NotApplicable(NotApplicableScriptFacts),
}

#[cfg(test)]
impl<T> ScriptFactEvidence<T> {
    pub(crate) fn expect_exact(self, message: &str) -> Arc<T> {
        match self {
            Self::Exact(exact) => exact.facts,
            Self::Partial(_) => panic!("{message}: got partial script facts"),
            Self::Unavailable(_) => panic!("{message}: script facts unavailable"),
            Self::NotApplicable(_) => panic!("{message}: script facts not applicable"),
        }
    }
}

static_assertions::assert_not_impl_any!(
    PartialScriptFacts<
        verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts
    >:
        verter_semantic::analysis::framework_facts::NegativeEvidence,
        std::ops::Deref,
        IntoIterator
);
static_assertions::assert_not_impl_any!(
    UnavailableScriptFacts:
        verter_semantic::analysis::framework_facts::NegativeEvidence,
        std::ops::Deref,
        IntoIterator
);

/// The addressable identity of the entry-point's two sibling tracer scopes.
/// `#[cfg(test)]` because the identity exists only where a test can target it —
/// the production arms of `named_cacheability_scope!` / `named_fact_tracer!`
/// drop the scope tokens unexpanded.
#[cfg(test)]
use crate::host_test_force::TracerScope;

/// The content-addressed candidate-store key.
///
/// Carries the full file content/version identity plus the capturing provider.
/// A content edit changes `content_hash`; a provider upgrade changes
/// `provider_version`; either misses the stale slot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CandidateSlotKey {
    /// The owner file canonical.
    pub canonical: Arc<str>,
    /// The file's content identity.
    pub content_hash: Hash16,
    /// The parse-env dimension.
    pub parse_env_hash: Hash16,
    /// The parser version.
    pub parser_version: u32,
    /// The file's `FileLanguage` row.
    pub file_language_id: verter_language::FileLanguage,
    /// The capturing provider's adapter id.
    pub provider_id: FrameworkAdapterId,
    /// The capturing provider's version.
    pub provider_version: u32,
}

/// The content-addressed framework-script CANDIDATE store.
///
/// A pure content-addressed artifact cache: a content edit or provider upgrade
/// changes the key and forces a cold re-capture. Hands out immutable `Arc`s.
#[derive(Debug, Default)]
pub struct FrameworkScriptCandidateStore {
    entries: DashMap<CandidateSlotKey, Arc<ExactFrameworkScriptCandidates>>,
}

impl FrameworkScriptCandidateStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The cached candidates for `key`, if present. Content-addressed: a hit is
    /// always valid (the key carries every content/version dimension).
    #[must_use]
    pub fn get(&self, key: &CandidateSlotKey) -> Option<Arc<ExactFrameworkScriptCandidates>> {
        self.entries.get(key).map(|e| Arc::clone(e.value()))
    }

    /// Memoize `candidates` under `key`, returning the canonical `Arc`.
    pub fn insert(
        &self,
        key: CandidateSlotKey,
        candidates: ExactFrameworkScriptCandidates,
    ) -> Arc<ExactFrameworkScriptCandidates> {
        let arc = Arc::new(candidates);
        self.entries.insert(key, Arc::clone(&arc));
        arc
    }

    /// Number of cached entries (test-only).
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store has no entries (test-only).
    #[cfg(test)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The resolved-fact store sub-key.
///
/// NO `lib_env_hash` / `type_env_hash` — the resolved facts do not depend on
/// lib or type data. The `consumed_capability_bits` column folds the ON/OFF
/// state of the bits the provider consumed, so a capability flip misses the
/// stale slot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedFactKey {
    /// The owner file canonical.
    pub canonical: Arc<str>,
    /// The producing provider's adapter id.
    pub provider_id: FrameworkAdapterId,
    /// The producing provider's version.
    pub provider_version: u32,
    /// A stable hash over the ON/OFF state of the provider's consumed
    /// capability bits — a capability flip changes it.
    pub consumed_capability_bits: Hash16,
    /// The project identity (env isolation).
    pub project_identity: Hash16,
    /// The resolve-env dimension.
    pub resolve_env_hash: Hash16,
}

/// A cached resolved fact plus the validation rails the resolution observed.
#[derive(Clone)]
pub struct StoredResolvedFact {
    /// The fully-owned, immutable resolved payload.
    pub payload: ExactScriptFacts<dyn FrameworkScriptFactPayload>,
    /// Path-precise fact signature observed while resolving (covers the
    /// owner's content + every resolved import contributor).
    pub read_set_signature: ReadSetSignature,
    /// Project generation the entry was validated at.
    pub validated_at_generation: u64,
}

/// The resolved-fact store.
///
/// Query-identity discipline: warm reads pass a strict same-generation gate AND
/// a fact-rail validation against the caller's live view; cold writes admit
/// ONLY via [`SignatureAdmission::Cacheable`]. An overflowed signature is never
/// warmed.
#[derive(Default)]
pub struct FrameworkScriptFactStore {
    entries: DashMap<ResolvedFactKey, Arc<StoredResolvedFact>>,
}

impl std::fmt::Debug for FrameworkScriptFactStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameworkScriptFactStore")
            .field("entries_len", &self.entries.len())
            .finish()
    }
}

impl FrameworkScriptFactStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The cached resolved fact for `key` IFF it still validates under the live
    /// `view` and project `generation` — BOTH gates must pass.
    #[must_use]
    pub fn get_with_view<V: StoreView + ?Sized>(
        &self,
        key: &ResolvedFactKey,
        view: &V,
        generation: u64,
    ) -> Option<Arc<StoredResolvedFact>> {
        let candidate = Arc::clone(self.entries.get(key)?.value());
        if candidate.validated_at_generation != generation {
            return None;
        }
        if !view.validates_fact_signature(&candidate.read_set_signature.facts) {
            return None;
        }
        Some(candidate)
    }

    /// Admit `entry` under `key` ONLY when `admission` is
    /// [`SignatureAdmission::Cacheable`], returning the canonical `Arc` on
    /// admission.
    ///
    /// An overflowed / non-cacheable admission NEVER warms the store — the
    /// computed value is returned to the caller alone (the no-poison invariant).
    /// The returned `Arc` lets a non-admitting caller still hand back the
    /// computed payload without a store entry.
    pub(crate) fn publish_if_cacheable(
        &self,
        key: ResolvedFactKey,
        payload: ExactScriptFacts<dyn FrameworkScriptFactPayload>,
        admission: &SignatureAdmission,
        generation: u64,
    ) -> Arc<StoredResolvedFact> {
        let stored = Arc::new(StoredResolvedFact {
            payload,
            read_set_signature: admission
                .cacheable()
                .cloned()
                .unwrap_or_else(ReadSetSignature::empty),
            validated_at_generation: generation,
        });
        if admission.cacheable().is_some() {
            self.entries.insert(key, Arc::clone(&stored));
        }
        stored
    }

    /// The single cached entry, if exactly one exists (test-only — the
    /// fact-rail discriminating tests read the stored read-set signature).
    #[cfg(test)]
    #[must_use]
    pub fn only_entry(&self) -> Option<Arc<StoredResolvedFact>> {
        if self.entries.len() != 1 {
            return None;
        }
        self.entries.iter().next().map(|e| Arc::clone(e.value()))
    }

    /// Number of cached entries (test-only).
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store has no entries (test-only).
    #[cfg(test)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The host-owned caches the resolved-validation half writes through.
///
/// Owned by the host; the registry owns the active-provider index instead.
///
/// PROVISIONAL: this cache pair is a host field OUTSIDE the single
/// `ProjectTypeStore`. Both stores are fact-validated — the candidate store is
/// content-addressed and [`FrameworkScriptFactStore::publish_if_cacheable`]
/// admits only `Cacheable` results (ReturnOnly on overflow) — so the logic is
/// correct today, but the pair is a temporary off-`ProjectTypeStore` cache still
/// to be consolidated onto `ProjectTypeStore`.
#[derive(Debug, Default)]
pub struct FrameworkScriptCaches {
    /// The content-addressed candidate store.
    pub candidates: FrameworkScriptCandidateStore,
    /// The resolved-fact store.
    pub facts: FrameworkScriptFactStore,
}

impl FrameworkScriptCaches {
    /// An empty cache pair.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

enum CapturedCandidateEvidence {
    Exact(Arc<ExactFrameworkScriptCandidates>),
    Recovered(Arc<FrameworkScriptCandidates>),
}

impl CapturedCandidateEvidence {
    fn candidates(&self) -> &FrameworkScriptCandidates {
        match self {
            Self::Exact(candidates) => candidates.candidates(),
            Self::Recovered(candidates) => candidates,
        }
    }

    fn is_exact(&self) -> bool {
        matches!(self, Self::Exact(_))
    }
}

/// The TYPED resolved-package identity of `specifier` → `resolved_canonical`,
/// computed SESSION-side where the package-backed classification authority lives.
///
/// `Some(ResolvedPackage)` IFF the import resolved to a canonical the session's
/// classifier reports as PACKAGE-BACKED (`ResolverContext::workspace_is_package_backed`
/// — NOT a `/node_modules/` path substring) AND the specifier is a BARE package
/// specifier (so its package name is the specifier's leading package segment).
/// A relative / workspace-owned / unresolved import returns `None` — a userland
/// `./fake-svelte` look-alike never claims a package identity even when it
/// resolves to a real file.
fn resolved_package_for_import(
    host: &VerterHost,
    specifier: &str,
    resolved_canonical: Option<&str>,
) -> Option<ResolvedPackage> {
    let canonical = resolved_canonical?;
    // Structural package-backing test (the classification authority), never a
    // path substring.
    if !ResolverContext::workspace_is_package_backed(host, canonical) {
        return None;
    }
    let name = bare_specifier_package_name(specifier)?;
    Some(ResolvedPackage::named(name))
}

/// The package name of a BARE import specifier, or `None` for a relative /
/// absolute specifier.
///
/// `"svelte"` → `"svelte"`; `"svelte/elements"` → `"svelte"`;
/// `"@scope/pkg/sub"` → `"@scope/pkg"`. A specifier beginning with `.` or `/` is
/// not a bare package specifier and returns `None`.
fn bare_specifier_package_name(specifier: &str) -> Option<String> {
    if specifier.starts_with('.') || specifier.starts_with('/') || specifier.is_empty() {
        return None;
    }
    let mut parts = specifier.split('/');
    let first = parts.next()?;
    if let Some(scope) = first.strip_prefix('@') {
        // A scoped package is `@scope/name`; the name is the second segment.
        if scope.is_empty() {
            return None;
        }
        let name = parts.next()?;
        if name.is_empty() {
            return None;
        }
        return Some(format!("{first}/{name}"));
    }
    if first.is_empty() {
        return None;
    }
    Some(first.to_string())
}

/// Resolve the adapter's framework script facts of type `T` for `canonical`.
///
/// Drives the resolved-validation half on demand:
/// 1. The registry's [`ActiveProviderIndex`](crate::framework::registry::ActiveProviderIndex)
///    selects the providers active for the file. A registration with no
///    provider short-circuits to `NotApplicable` with zero per-file work.
/// 2. For each active provider, the file's candidates are captured (content-
///    addressed slot peek/fill) by re-running the provider's syntax capture
///    over the file's live OXC program.
/// 3. The provider's resolved-validation (`validate`) is driven with the
///    resolved import targets (the session's own import resolution) + a
///    capability lookup; it rejects userland look-alikes and refuses on a
///    capability-bit miss.
/// 4. A resolved payload is admitted to the resolved-fact store ONLY when its
///    fact signature is [`SignatureAdmission::Cacheable`]; the cold compute
///    observes the owner's whole-hash + every resolved import contributor onto
///    the tracer so a cross-file edit misses the warm entry.
///
/// Returns exact, partial, unavailable, and not-applicable evidence without
/// projecting any state through `Option`.
pub(crate) fn resolve_script_facts<T: FrameworkScriptFactPayload>(
    host: &VerterHost,
    registration: &FrameworkRegistration,
    canonical: &str,
) -> ScriptFactEvidence<T> {
    resolve_script_facts_inner::<T>(host, registration, canonical, None)
}

/// Like [`resolve_script_facts`], but warm-reads + generation-gates against the
/// CALLER's request view (`ctx`) instead of opening a fresh
/// `current_store_view_for_query`. The framework-surface executor's Svelte arm
/// uses this so the facts read shares the ONE coherent request view the rest of
/// the response resolves under — a fact validated/published against a
/// newer view can never warm an entry keyed under the executor's older content
/// view.
pub(crate) fn resolve_script_facts_with_ctx<T: FrameworkScriptFactPayload>(
    host: &VerterHost,
    registration: &FrameworkRegistration,
    canonical: &str,
    ctx: &dyn ResolverContext,
) -> ScriptFactEvidence<T> {
    resolve_script_facts_inner::<T>(host, registration, canonical, Some(ctx))
}

fn resolve_script_facts_inner<T: FrameworkScriptFactPayload>(
    host: &VerterHost,
    registration: &FrameworkRegistration,
    canonical: &str,
    request_ctx: Option<&dyn ResolverContext>,
) -> ScriptFactEvidence<T> {
    // Zero-cost fast path: this registration registers NO provider ⇒ no
    // candidates ⇒ no facts, so the resolved-validation half does ZERO per-file
    // work — no `current_eval_state`, no classification, no `get_analysis`, no
    // import resolution. (The Vue registration is provider-less; the Svelte
    // registration carries one.) The host registry's `ActiveProviderIndex`
    // aggregates EVERY registration's providers, so it is empty IFF no
    // registration carries any provider — a registry-wide oracle, NOT a
    // per-registration mirror. The per-registration list below keys THIS
    // registration's own facts.
    debug_assert!(
        host.framework_registry().active_provider_index().is_empty()
            != host.framework_registry().any_provider_registered(),
        "the host index emptiness must agree with the registry-wide provider presence"
    );
    if registration.script_fact_providers.is_empty() {
        return ScriptFactEvidence::NotApplicable(NotApplicableScriptFacts::new(
            ScriptFactNotApplicableReason::ProviderNotRegistered,
        ));
    }
    let Some((raw_source, framework_parse, whole_hash)) = host.current_eval_state(canonical) else {
        return ScriptFactEvidence::Unavailable(UnavailableScriptFacts::new(
            ScriptFactUnavailableReason::SourceUnavailable,
        ));
    };
    let file_language = host.language_classifier().classify(canonical);
    let carrier_language = file_language.carrier_language_id();

    // A framework carrier's runes/macros live in its script block(s). The
    // provider captures over the POSITION-PRESERVING eval-source (script bytes at
    // raw carrier offsets, markup/styles blanked) — the SAME source the synth
    // injection captures from — so a `.svelte` capture parses valid TS, never the
    // raw markup. A non-carrier file's raw source is already script.
    let source: Arc<str> = if file_language.is_framework_carrier() {
        Arc::from(
            crate::VerterHost::build_eval_script_source(
                canonical,
                raw_source.as_ref(),
                framework_parse.as_deref(),
            )
            .as_str(),
        )
    } else {
        raw_source
    };
    // The module-script region (so the provider classifies module vs instance
    // exports) — read from the carrier's neutral script regions.
    let module_region = framework_parse
        .as_deref()
        .and_then(crate::parse::module_script_region);
    let framework_mode_hint = framework_parse
        .as_deref()
        .and_then(crate::parse::framework_script_mode_hint);

    // The file's imports (specifier + the session-resolved canonical) — the
    // resolved data the provider's validation inspects. The session resolves
    // each candidate specifier through its OWN import resolver
    // (`resolve_snapshot_imports`) and hands the outcome to the provider as
    // data — the provider never reaches the resolver itself.
    // The owner's import-route resolution establishes the owner's
    // `IndexedReady` (its authored import inventory IS that artifact's
    // shallow surface) — a FENCED (ReturnOnly, `store_published == false`)
    // serve there means the basis is superseded, and the facts entry built
    // on it would validate against the LIVE view (the fenced-serve poison
    // model). This resolution runs BEFORE the `provider.validate` tracer
    // installed further down, so it needs its OWN traced scope; the captured
    // `import_non_cacheable` bit refuses this facts entry's
    // `publish_if_cacheable` admission below (the standalone entry-point
    // has no enclosing tracer, so the outer surface tracer cannot cover it).
    //
    // The facts entry's signature comes from the `provider.validate` tracer below,
    // never from THIS one, so this boundary reads the CACHEABILITY verdict — which
    // folds the non-cacheable-read bit together with a
    // `FactReadSetFinalise::Overflow` (an observation set no signature can root:
    // a second, INDEPENDENT non-admission condition that must not be dropped here,
    // since nothing downstream inspects this tracer's finalised set).
    //
    // The scope is opened by NAME so a test can target THIS boundary's overflow
    // rail specifically — see `named_cacheability_scope!`. The name is erased in a
    // production build.
    let (resolved_import_targets_opt, mut import_non_cacheable): (
        Option<Vec<ResolvedImportTarget>>,
        bool,
    ) = named_cacheability_scope!(host, TracerScope::ScriptFactsImportRoute, || {
        let mut snapshot = host.get_analysis(canonical)?;
        host.resolve_snapshot_imports(canonical, &mut snapshot);
        Some(
            snapshot
                .imports
                .iter()
                .map(|imp| {
                    // The TYPED resolved-package identity (P2): a bare specifier
                    // whose resolved canonical is PACKAGE-BACKED per the
                    // session's classifier (`workspace_is_package_backed` — NOT
                    // a path substring) names its package; a relative /
                    // workspace-owned / unresolved import carries no package
                    // identity, so a userland `./fake-svelte` look-alike never
                    // claims to be the `svelte` package.
                    let package = resolved_package_for_import(
                        host,
                        &imp.source,
                        imp.resolved_canonical_id.as_deref(),
                    );
                    ResolvedImportTarget {
                        specifier: imp.source.clone(),
                        resolved_canonical: imp.resolved_canonical_id.clone(),
                        package,
                    }
                })
                .collect(),
        )
    });
    let Some(mut resolved_import_targets) = resolved_import_targets_opt else {
        return ScriptFactEvidence::Unavailable(UnavailableScriptFacts::new(
            ScriptFactUnavailableReason::AnalysisUnavailable,
        ));
    };

    // Select the active provider through the SHARED gate-matching authority
    // (`ActiveProviderIndex::gate_matches`) over THIS registration's own
    // providers — a registration produces only its own adapter's facts. The
    // shared predicate is exactly the one the registry-wide index applies, so
    // the per-registration selection and the index agree by construction.
    let Some(provider) = registration
        .script_fact_providers
        .iter()
        .find(|p| {
            crate::framework::registry::ActiveProviderIndex::gate_matches(
                &p.syntax_gate(),
                carrier_language,
                resolved_import_targets.iter().map(|t| t.specifier.as_str()),
            )
        })
        .cloned()
    else {
        return ScriptFactEvidence::NotApplicable(NotApplicableScriptFacts::new(
            ScriptFactNotApplicableReason::ProviderGateMiss,
        ));
    };

    // ── Candidate capture (content-addressed slot) ──
    let env = host.host_view_env_hashes_for(canonical);
    let candidate_canonical: Arc<str> = Arc::from(canonical);
    let candidate_key = CandidateSlotKey {
        canonical: Arc::clone(&candidate_canonical),
        content_hash: whole_hash,
        parse_env_hash: env.parse_env_hash,
        parser_version: crate::file_artifact_store::LEGACY_PARSER_VERSION,
        file_language_id: file_language.clone(),
        provider_id: provider.adapter_id(),
        provider_version: provider.provider_version(),
    };
    let candidates = match host
        .framework_script_caches()
        .candidates
        .get(&candidate_key)
    {
        Some(candidates) => CapturedCandidateEvidence::Exact(candidates),
        None => {
            let source_type = crate::parse::carrier_eval_source_type(framework_parse.as_deref());
            let captured = match capture_candidates_for(
                &provider,
                &source,
                source_type,
                module_region,
                framework_mode_hint,
                framework_parse.as_deref(),
            ) {
                Ok(captured) => captured,
                Err(reason) => {
                    return ScriptFactEvidence::Unavailable(UnavailableScriptFacts::new(reason));
                }
            };
            match captured {
                CandidateCapture::Exact(captured) => {
                    // Producer-side locator absolutization: fill the capture's
                    // empty-sentinel payload-ref anchors with the PRODUCING
                    // canonical before the exact envelope enters the
                    // content-addressed store.
                    let captured = provider.absolutize_candidates(captured, canonical);
                    CapturedCandidateEvidence::Exact(
                        host.framework_script_caches()
                            .candidates
                            .insert(candidate_key.clone(), captured),
                    )
                }
                CandidateCapture::Recovered(captured) => {
                    // Recovered observations are useful to this request but
                    // cannot enter the exact candidate store.
                    let captured = provider.absolutize_candidate_observations(captured, canonical);
                    CapturedCandidateEvidence::Recovered(Arc::new(captured))
                }
            }
        }
    };

    // ── Candidate-referenced specifiers OUTSIDE the import statements ──
    //
    // A binding-less inline `import("…")` type reference names a module with
    // no import statement, so the snapshot-imports resolution above cannot
    // see its specifier. The provider surfaces those candidate specifiers as
    // DATA (`candidate_import_specifiers` — a pure candidate read); the
    // session resolves them through the SAME resolver + typed package
    // classification as statement imports, under the same import-route
    // cacheability scope, and appends them so the fan-out observation and
    // `provider.validate` see ONE uniform resolved-target list.
    let extra_specifiers: Vec<String> = provider
        .candidate_import_specifiers(candidates.candidates())
        .into_iter()
        .filter(|specifier| {
            !resolved_import_targets
                .iter()
                .any(|target| &target.specifier == specifier)
        })
        .collect();
    if !extra_specifiers.is_empty() {
        let ((extra_targets, extra_refused), extra_scope_non_cacheable): (
            (Vec<ResolvedImportTarget>, bool),
            bool,
        ) = named_cacheability_scope!(host, TracerScope::ScriptFactsImportRoute, || {
            let mut refused = false;
            let targets = extra_specifiers
                .iter()
                .map(|specifier| {
                    let resolved = host
                        .authoritative_import_route(canonical, specifier)
                        .and_then(|resolution| {
                            resolution
                                .resolved_canonical_id
                                .clone()
                                .or_else(|| resolution.effective_target().map(str::to_string))
                        })
                        .or_else(|| {
                            let ctx = verter_workspace::ResolutionContext {
                                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                                kind: verter_workspace::ResolveRequestKind::TypeImport,
                            };
                            match host.resolve_via_vfs(canonical, specifier, ctx) {
                                verter_workspace::ResolutionPublication::Admitted(admitted) => {
                                    admitted.into_result()
                                }
                                verter_workspace::ResolutionPublication::Refused(_) => {
                                    refused = true;
                                    None
                                }
                            }
                        });
                    let package = resolved_package_for_import(host, specifier, resolved.as_deref());
                    ResolvedImportTarget {
                        specifier: specifier.clone(),
                        resolved_canonical: resolved,
                        package,
                    }
                })
                .collect::<Vec<_>>();
            (targets, refused)
        });
        resolved_import_targets.extend(extra_targets);
        import_non_cacheable = import_non_cacheable || extra_scope_non_cacheable || extra_refused;
    }

    // ── Resolved-fact lookup ──
    let project_identity = host.host_view_project_identity_for(canonical).0;
    let consumed_capability_bits = capability_bits_hash(host, provider.consumed_capabilities());
    let fact_key = ResolvedFactKey {
        canonical: Arc::clone(&candidate_key.canonical),
        provider_id: provider.adapter_id(),
        provider_version: provider.provider_version(),
        consumed_capability_bits,
        project_identity,
        resolve_env_hash: env.resolve_env_hash,
    };
    // The generation gate: the CALLER's request-view generation when threaded
    // (so the warm read + publish gate on the SAME generation the executor's
    // surface entry validates under), else the live project generation.
    let generation = match request_ctx {
        Some(ctx) => ctx.project_type_store().current_project_generation(),
        None => host.project_type_store().project_generation(),
    };

    // Warm read against the CALLER's request view when one is supplied (the
    // framework-surface executor's Svelte arm threads its single captured
    // view); otherwise open a proven-current view for the standalone facts
    // entry-point. Either way the read validates the recorded fact signature +
    // generation, so a stale entry misses.
    if candidates.is_exact() {
        if let Some(ctx) = request_ctx {
            if let Some(stored) = host.framework_script_caches().facts.get_with_view(
                &fact_key,
                ctx.store_view(),
                generation,
            ) {
                // Bubble the facts entry's import-route / package-provenance fact
                // signature into any OUTER fact tracer (the Svelte surface-store cold
                // trace), so a later source row consuming these cached facts inherits
                // their cross-file facts and a same-content reroute invalidates the
                // surface entry too.
                stored.read_set_signature.bubble_via_tls();
                return match stored.payload.clone().downcast::<T>() {
                    Some(exact) => ScriptFactEvidence::Exact(exact),
                    None => ScriptFactEvidence::Unavailable(UnavailableScriptFacts::new(
                        ScriptFactUnavailableReason::ProviderPayloadMismatch,
                    )),
                };
            }
        } else if let Some(current_view) = crate::typeinfo::current_store_view_for_query(host) {
            let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
            let host_ctx = crate::resolver_core::HostResolverContext::from_current(
                host,
                &current_view,
                overlay,
            );
            if let Some(stored) = host.framework_script_caches().facts.get_with_view(
                &fact_key,
                host_ctx.store_view(),
                generation,
            ) {
                stored.read_set_signature.bubble_via_tls();
                return match stored.payload.clone().downcast::<T>() {
                    Some(exact) => ScriptFactEvidence::Exact(exact),
                    None => ScriptFactEvidence::Unavailable(UnavailableScriptFacts::new(
                        ScriptFactUnavailableReason::ProviderPayloadMismatch,
                    )),
                };
            }
        }
    }

    // The validation depends on the resolved import targets — i.e. the owner's
    // IMPORT ROUTE surface. A route change (a barrel / path-alias re-route that
    // points a specifier at a different canonical) leaves the owner's content
    // AND the old target's content unchanged, so the whole-hash rail alone
    // would stale-serve. The owner's import-route RESOLUTION WITNESS roots the
    // cached payload against that route surface.
    let owner_has_imports = !resolved_import_targets.is_empty();
    let import_route_witness = host.owner_import_route_witness(canonical);

    // ── Cold resolved-validation, fact-traced ──
    //
    // Named for the same reason as the import-route scope above: a test targets
    // ONE of the two sibling tracers and must be able to say WHICH. Erased in a
    // production build.
    let (payload_opt, finalise) = named_fact_tracer!(
        host,
        TracerScope::ScriptFactsProviderValidate,
        || {
            // Observe the owner's whole hash + every resolved import contributor so
            // a content edit to any of them misses the warm entry.
            crate::resolver_core::resolver_context::observe_fan_out(
                crate::resolver_core::FactVersionRef::FileWholeHash {
                    canonical_id: canonical.to_string(),
                    hash: whole_hash,
                },
            );
            // Root the payload against the owner's import-route surface so a
            // re-route (unchanged file contents) misses the warm entry.
            if let Some(witness) = import_route_witness.as_deref() {
                crate::resolver_core::resolver_context::observe_fan_out_borrowed(witness);
            }
            for target in &resolved_import_targets {
                if let Some(import_canonical) = &target.resolved_canonical {
                    if let Some(h) = host.get_whole_hash(import_canonical) {
                        crate::resolver_core::resolver_context::observe_fan_out(
                            crate::resolver_core::FactVersionRef::FileWholeHash {
                                canonical_id: import_canonical.clone(),
                                hash: h,
                            },
                        );
                    } else {
                        // Script-fact validation is a consumer of already-loaded
                        // import state, not a loading demand. Exact-empty Svelte
                        // evidence can reach this path for an ordinary component
                        // import; reloading that cold child would violate the
                        // caller's query-scoped load boundary. Without its live
                        // hash the result remains usable for this request, but it
                        // cannot be rooted for shared warm admission.
                        crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                            crate::resolver_core::resolver_context::NonCacheableReadReason::UnobservableSource,
                        );
                    }
                }
            }
            let capability_on = |cap: &str| {
                host.language_classifier()
                    .capability_is_enabled(&verter_language::CapabilityId::new(cap))
            };
            let cx = ResolvedValidationCx {
                candidates: candidates.candidates(),
                resolved_import_targets: &resolved_import_targets,
                capability_on: &capability_on,
            };
            provider.validate(cx)
        }
    );

    let exact_payload = match payload_opt {
        ScriptFactValidation::Exact(payload) if candidates.is_exact() => {
            ExactScriptFacts::new(payload)
        }
        ScriptFactValidation::Exact(payload) => {
            if let crate::resolver_core::FactReadSetFinalise::Ok(facts)
            | crate::resolver_core::FactReadSetFinalise::NonCacheable(facts) = &finalise
            {
                crate::fact_signature_helpers::bubble_fact_signature_via_tls(facts.as_ref());
            }
            let Some(facts) = payload.as_any_arc().downcast::<T>().ok() else {
                return ScriptFactEvidence::Unavailable(UnavailableScriptFacts::new(
                    ScriptFactUnavailableReason::ProviderPayloadMismatch,
                ));
            };
            return ScriptFactEvidence::Partial(PartialScriptFacts::new(
                facts,
                ScriptFactPartialReason::SyntaxRecovery,
                PartialSyntaxCompleteness::Recovered,
            ));
        }
        ScriptFactValidation::Partial { payload, reason } => {
            if let crate::resolver_core::FactReadSetFinalise::Ok(facts)
            | crate::resolver_core::FactReadSetFinalise::NonCacheable(facts) = &finalise
            {
                crate::fact_signature_helpers::bubble_fact_signature_via_tls(facts.as_ref());
            }
            let Some(facts) = payload.as_any_arc().downcast::<T>().ok() else {
                return ScriptFactEvidence::Unavailable(UnavailableScriptFacts::new(
                    ScriptFactUnavailableReason::ProviderPayloadMismatch,
                ));
            };
            let (reason, syntax_completeness) = if candidates.is_exact() {
                (reason, PartialSyntaxCompleteness::Exact)
            } else {
                (
                    ScriptFactPartialReason::SyntaxRecovery,
                    PartialSyntaxCompleteness::Recovered,
                )
            };
            return ScriptFactEvidence::Partial(PartialScriptFacts::new(
                facts,
                reason,
                syntax_completeness,
            ));
        }
        ScriptFactValidation::Unavailable(reason) => {
            return ScriptFactEvidence::Unavailable(UnavailableScriptFacts::new(reason));
        }
    };
    // Bubble the cold-computed facts signature (the owner whole-hash + every
    // resolved import contributor + the import-route rail) into any OUTER fact
    // tracer (the Svelte surface-store cold trace) so the surface entry's
    // ReadSetSignature carries the SAME cross-file facts — a same-content import
    // reroute that flips the facts then misses the warm surface entry too. The
    // facts are bubbled BEFORE `finalise` is consumed by the admission check.
    if let crate::resolver_core::FactReadSetFinalise::Ok(facts)
    | crate::resolver_core::FactReadSetFinalise::NonCacheable(facts) = &finalise
    {
        crate::fact_signature_helpers::bubble_fact_signature_via_tls(facts.as_ref());
    }
    let validate_non_cacheable = matches!(
        &finalise,
        crate::resolver_core::FactReadSetFinalise::NonCacheable(_)
    );
    let admission = SignatureAdmission::from_finalise(finalise);
    // An import-dependent validation whose owner import-route rail could NOT be
    // produced must NOT warm the store (it would stale-serve on a re-route).
    // Return the computed value to this caller alone, uncached.
    if owner_has_imports && import_route_witness.is_none() {
        return match exact_payload.downcast::<T>() {
            Some(exact) => ScriptFactEvidence::Exact(exact),
            None => ScriptFactEvidence::Unavailable(UnavailableScriptFacts::new(
                ScriptFactUnavailableReason::ProviderPayloadMismatch,
            )),
        };
    }
    // A FENCED (ReturnOnly, `store_published == false`) / lease-miss serve
    // consumed while resolving the import route (`import_non_cacheable`) OR while
    // running `provider.validate` (`validate_non_cacheable`) derived this facts
    // payload from a served-without-publication / transient basis while its fact
    // signature validates against the LIVE view — an entry the read-side rail
    // cannot reject. Return the value to THIS caller alone, uncached: a later
    // warm read (`get_with_view`) would stale-serve the poisoned facts, and the
    // executor's Svelte-surface entry re-reads them. The standalone entry-point
    // has no enclosing tracer, so this is the sole refusal covering it.
    if import_non_cacheable || validate_non_cacheable {
        return match exact_payload.downcast::<T>() {
            Some(exact) => ScriptFactEvidence::Exact(exact),
            None => ScriptFactEvidence::Unavailable(UnavailableScriptFacts::new(
                ScriptFactUnavailableReason::ProviderPayloadMismatch,
            )),
        };
    }
    // Cacheable-only publication; overflow returns the value to this caller
    // alone (never warms the store).
    let stored = host.framework_script_caches().facts.publish_if_cacheable(
        fact_key,
        exact_payload,
        &admission,
        generation,
    );
    match stored.payload.clone().downcast::<T>() {
        Some(exact) => ScriptFactEvidence::Exact(exact),
        None => ScriptFactEvidence::Unavailable(UnavailableScriptFacts::new(
            ScriptFactUnavailableReason::ProviderPayloadMismatch,
        )),
    }
}

/// Capture a provider's candidates from a file's source by re-running its
/// syntax-only capture over a freshly-parsed OXC program. PARSE-DOMAIN only.
enum CandidateCapture {
    Exact(ExactFrameworkScriptCandidates),
    Recovered(FrameworkScriptCandidates),
}

fn capture_candidates_for(
    provider: &Arc<dyn ScriptFactProvider>,
    source: &str,
    source_type: SourceType,
    module_script_region: Option<(u32, u32)>,
    framework_mode_hint: Option<
        verter_semantic::analysis::framework_facts::FrameworkScriptModeHint,
    >,
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
) -> Result<CandidateCapture, ScriptFactUnavailableReason> {
    let alloc = Allocator::new();
    let parser = Parser::new(&alloc, source, source_type).with_options(ParseOptions {
        parse_regular_expression: false,
        ..ParseOptions::default()
    });
    let result = parser.parse();
    if result.panicked {
        return Err(ScriptFactUnavailableReason::CaptureFailed);
    }
    let recovered = !result.errors.is_empty();
    let owner_table = crate::parse::top_level_owner_table(&result.program, framework_parse)
        .map_err(|_| ScriptFactUnavailableReason::CaptureFailed)?;
    if recovered {
        let candidates = provider.capture(ScriptCandidateCx {
            source,
            program: &result.program,
            top_level_owners: &owner_table,
            module_script_region,
            framework_mode_hint,
        });
        Ok(CandidateCapture::Recovered(candidates))
    } else {
        let set =
            verter_semantic::analysis::framework_facts::capture_script_candidates_with_context(
                std::slice::from_ref(provider),
                source,
                &result.program,
                module_script_region,
                framework_mode_hint,
                &owner_table,
            );
        set.per_provider
            .into_iter()
            .next()
            .map(CandidateCapture::Exact)
            .ok_or(ScriptFactUnavailableReason::CaptureFailed)
    }
}

fn capability_bits_hash(host: &VerterHost, consumed: &[&'static str]) -> Hash16 {
    let mut buf = Vec::new();
    for cap in consumed {
        let on = host
            .language_classifier()
            .capability_is_enabled(&verter_language::CapabilityId::new(cap));
        buf.extend_from_slice(cap.as_bytes());
        buf.push(if on { 1 } else { 0 });
        buf.push(0);
    }
    crate::hash::hash_16(&buf)
}

impl VerterHost {
    /// Drive the Svelte adapter's resolved-validation half for `canonical`,
    /// returning the validated
    /// [`SvelteScriptFacts`](verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts)
    /// — the snippet-typed members whose `Snippet`-candidate import RESOLVED to
    /// the `svelte` package (a userland look-alike is rejected). Exact-empty,
    /// partial, unavailable, and not-applicable outcomes remain distinct.
    ///
    /// The Svelte surface adapter's SLOTS seam consumers reach this through the
    /// shared `script_facts_for` path; this entry exposes it for the Svelte
    /// vertical's resolved-validation coverage.
    pub fn resolve_svelte_script_facts(
        &self,
        canonical: &str,
    ) -> ScriptFactEvidence<verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts>
    {
        let Some(registration) = self
            .framework_registry()
            .get(&verter_language::FrameworkAdapterId::svelte())
        else {
            return ScriptFactEvidence::NotApplicable(NotApplicableScriptFacts::new(
                ScriptFactNotApplicableReason::ProviderNotRegistered,
            ));
        };
        let ctx = crate::framework::ctx::FrameworkAdapterCtx::new(registration, self);
        ctx.script_facts_for::<verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts>(
            canonical,
        )
    }

    /// Drive the Svelte resolved-validation half against the CALLER's request
    /// view `ctx` — the framework-surface executor's Svelte arm uses this so the
    /// facts read shares the ONE coherent request view the rest of the response
    /// resolves under, never a second `current_store_view_for_query`.
    pub(crate) fn resolve_svelte_script_facts_with_ctx(
        &self,
        ctx: &dyn ResolverContext,
        canonical: &str,
    ) -> ScriptFactEvidence<verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts>
    {
        let Some(registration) = self
            .framework_registry()
            .get(&verter_language::FrameworkAdapterId::svelte())
        else {
            return ScriptFactEvidence::NotApplicable(NotApplicableScriptFacts::new(
                ScriptFactNotApplicableReason::ProviderNotRegistered,
            ));
        };
        resolve_script_facts_with_ctx::<
            verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
        >(self, registration, canonical, ctx)
    }
}

/// Shared in-tree fixture seam — a `ScriptFactProvider` and registration
/// builders exercising the resolved-validation half end-to-end. Test-only:
/// Svelte owns the production provider while Vue's macro analysis stays in the
/// shallow pass.
#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use std::any::Any;
    use verter_language::LanguageId;
    use verter_semantic::analysis::framework_facts::{ScriptCandidateCx, ScriptFactSyntaxGate};

    /// The fixture adapter id.
    pub(crate) fn fixture_adapter_id() -> FrameworkAdapterId {
        FrameworkAdapterId::new("fixture-fw")
    }

    /// The fixture carrier language id (for the carrier-gated registration).
    pub(crate) fn fixture_language() -> LanguageId {
        LanguageId::new("fixture-lang")
    }

    /// The fixture import specifier (for the import-gated registration). The
    /// end-to-end host fixture imports this exact specifier, and the host's
    /// import resolver resolves it to the framework package file.
    pub(crate) const FIXTURE_IMPORT_SPECIFIER: &str = "./node_modules/fixture-fw/index";

    /// The capability bit the fixture provider's resolved facts depend on.
    pub(crate) const FIXTURE_CAPABILITY: &str = "fixture-cap";

    /// The fixture resolved payload.
    #[derive(Debug)]
    pub(crate) struct FixtureFactPayload {
        pub(crate) resolved_specifier: String,
    }
    impl FrameworkScriptFactPayload for FixtureFactPayload {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }

    /// The fixture provider. Captures a candidate whenever invoked; validates
    /// that a resolved import landed inside the framework's package directory
    /// and (when `requires_capability`) the capability bit is ON.
    ///
    /// `requires_capability = false` lets an end-to-end test exercise the full
    /// resolve flow on a real host (whose capability snapshot is empty in this
    /// program) without depending on a capability producer that does not exist
    /// yet; the capability-OFF refusal is then a separate discriminating case.
    pub(crate) struct FixtureProvider {
        pub(crate) gate: ScriptFactSyntaxGate,
        pub(crate) requires_capability: bool,
    }

    impl ScriptFactProvider for FixtureProvider {
        fn adapter_id(&self) -> FrameworkAdapterId {
            fixture_adapter_id()
        }
        fn provider_version(&self) -> u32 {
            1
        }
        fn syntax_gate(&self) -> ScriptFactSyntaxGate {
            self.gate.clone()
        }
        fn consumed_capabilities(&self) -> &[&'static str] {
            if self.requires_capability {
                &[FIXTURE_CAPABILITY]
            } else {
                &[]
            }
        }
        fn capture(
            &self,
            _cx: ScriptCandidateCx<'_>,
        ) -> verter_semantic::analysis::framework_facts::FrameworkScriptCandidates {
            verter_semantic::analysis::framework_facts::FrameworkScriptCandidates {
                adapter_id: self.adapter_id(),
                provider_version: self.provider_version(),
                stable_hash: [7u8; 16],
                payload: Arc::new(()),
            }
        }
        fn validate(&self, cx: ResolvedValidationCx<'_>) -> ScriptFactValidation {
            if self.requires_capability && !(cx.capability_on)(FIXTURE_CAPABILITY) {
                return ScriptFactValidation::Unavailable(
                    ScriptFactUnavailableReason::ValidationProducedNoFacts,
                );
            }
            let Some(resolved) = cx.resolved_import_targets.iter().find(|t| {
                t.resolved_canonical
                    .as_deref()
                    .is_some_and(|c| c.contains("/node_modules/fixture-fw/"))
            }) else {
                return ScriptFactValidation::Unavailable(
                    ScriptFactUnavailableReason::ValidationProducedNoFacts,
                );
            };
            ScriptFactValidation::Exact(Arc::new(FixtureFactPayload {
                resolved_specifier: resolved.specifier.clone(),
            }))
        }
    }

    fn fixture_store() -> Arc<dyn crate::framework::surface_store::ErasedFrameworkSurfaceStore> {
        Arc::new(crate::framework::surface_store::FrameworkSurfaceStore::<
            crate::typeinfo::framework_surface::VueSurfaceKey,
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >::new())
    }

    fn registration_with(
        gate: ScriptFactSyntaxGate,
        requires_capability: bool,
    ) -> FrameworkRegistration {
        FrameworkRegistration {
            descriptor: crate::framework::descriptor::FrameworkAdapterDescriptor {
                id: fixture_adapter_id(),
                tag: verter_protocol::typeinfo::graph::FrameworkTag::Svelte,
                supported_surfaces: crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
                carrier_language: None,
                virtual_file_naming: None,
                supports_named_export_surfaces: false,
            },
            carrier: None,
            synth: None,
            api_projector: None,
            script_fact_providers: vec![Arc::new(FixtureProvider {
                gate,
                requires_capability,
            })],
            surface: crate::framework::registry::SurfaceRegistration::Deferred,
            surface_store: fixture_store(),
        }
    }

    /// A fixture registration whose provider is carrier-language-gated.
    pub(crate) fn carrier_gated_fixture_registration() -> FrameworkRegistration {
        registration_with(
            ScriptFactSyntaxGate::CarrierLanguage(fixture_language()),
            true,
        )
    }

    /// A fixture registration whose provider is import-specifier-gated and
    /// requires a capability bit (the capability-gated path).
    pub(crate) fn import_gated_fixture_registration() -> FrameworkRegistration {
        registration_with(
            ScriptFactSyntaxGate::ImportSpecifier(FIXTURE_IMPORT_SPECIFIER),
            true,
        )
    }

    /// A fixture registration whose provider is import-specifier-gated and
    /// requires NO capability — the variant an end-to-end test exercises on a
    /// real host (whose capability snapshot is empty in this program).
    pub(crate) fn import_gated_capability_free_fixture_registration() -> FrameworkRegistration {
        registration_with(
            ScriptFactSyntaxGate::ImportSpecifier(FIXTURE_IMPORT_SPECIFIER),
            false,
        )
    }
}

#[cfg(test)]
#[path = "script_facts_tests.rs"]
mod tests;
