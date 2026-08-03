//! The syntax-capture half of the framework script-fact seam.
//!
//! The seam is split in two: a SYNTAX-CAPTURE half (here) and a
//! RESOLVED-VALIDATION half (session-side, `framework/script_facts.rs`).
//! `verter_semantic` owns the OXC shallow pass, so it owns the syntax-capture
//! half: the [`ScriptFactProvider`] trait, the closed [`ScriptFactSyntaxGate`],
//! the per-file parse-domain [`FrameworkScriptCandidates`] /
//! [`FrameworkScriptCandidateSet`] collection, and the producer-owned
//! validation outcome.
//!
//! Capture is SYNTAX-ONLY: a provider's [`ScriptFactProvider::capture`] may
//! touch live OXC + `lower_ts_type`, but MUST NOT resolve imports or read
//! capability bits — that resolved-symbol validation is the session-side half,
//! driven on demand (`framework/script_facts.rs`). The neutral dispatcher
//! ([`crate::analysis::build_script_analysis_with_scope_from_program`]) takes an
//! `active_providers: &[Arc<dyn ScriptFactProvider>]` slice the SESSION caller
//! computes BEFORE the pass; an EMPTY slice is the byte-identical original path
//! (zero capture work, zero allocation — the
//! `script_fact_providers_zero_cost_on_miss` guard).
//!
//! Vue registers no provider — its macro analysis stays inside the shallow pass.
//! The Svelte carrier (the [`svelte`] submodule) registers a production provider
//! (carrier-language gated on `svelte`) capturing its runes inventory. The seam
//! is also exercised by an in-tree fixture provider.

use std::any::Any;
use std::sync::Arc;

use oxc_ast::ast::Program;

use verter_language::{FrameworkAdapterId, LanguageId};

pub mod svelte;

mod negative_evidence_seal {
    pub trait Sealed {}
}

/// A sealed capability proving that an observation inventory is complete, so
/// absence from it is authoritative.
///
/// Only producer-owned exact inventories implement this trait. Partial and
/// unavailable script-fact states cannot implement it because the seal is
/// private to this module.
pub trait NegativeEvidence: negative_evidence_seal::Sealed {
    /// One observation in the exact inventory.
    type Observation;

    /// Every observation in the complete inventory.
    fn observations(&self) -> &[Self::Observation];
}

/// Why a usable script-fact result is incomplete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptFactPartialReason {
    /// At least one provenance-sensitive import could not be resolved, while
    /// syntax-owned observations remained usable.
    UnresolvedImportProvenance,
    /// The parser recovered an AST after reporting syntax errors. Positive
    /// observations remain usable, but the recovered tree cannot prove
    /// absence.
    SyntaxRecovery,
}

/// Why no reliable script-fact payload could be produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptFactUnavailableReason {
    /// The source/evaluation state was unavailable.
    SourceUnavailable,
    /// The semantic analysis needed to validate imports was unavailable.
    AnalysisUnavailable,
    /// The syntax capture could not be completed.
    CaptureFailed,
    /// The selected provider returned a payload with an unexpected type.
    ProviderPayloadMismatch,
    /// Provider validation explicitly produced no reliable facts.
    ValidationProducedNoFacts,
}

/// A provider's resolved-validation outcome before the session wraps it in the
/// consumer-facing evidence family.
pub enum ScriptFactValidation {
    /// The payload is complete, including authoritative empty inventories.
    Exact(Arc<dyn FrameworkScriptFactPayload>),
    /// The payload contains usable positive observations but cannot prove
    /// absence for every channel.
    Partial {
        /// The usable observations.
        payload: Arc<dyn FrameworkScriptFactPayload>,
        /// Why the payload is incomplete.
        reason: ScriptFactPartialReason,
    },
    /// No reliable payload was produced.
    Unavailable(ScriptFactUnavailableReason),
}

/// The marker + `Any`-bridge a resolved framework-script-fact payload carries.
///
/// Each provider's resolved payload type implements this; typed retrieval is
/// keyed per-provider (a downcast never crosses a provider boundary in
/// practice). The `Any` bridge backs the one retrieval downcast.
pub trait FrameworkScriptFactPayload: Send + Sync + 'static {
    /// Upcast to `&dyn Any` for the typed retrieval downcast.
    fn as_any(&self) -> &dyn Any;
    /// Upcast an owned `Arc<Self>` to `Arc<dyn Any + Send + Sync>` so the
    /// resolved-validation half can hand back a typed `Arc<T>` (the owned form
    /// of the one retrieval downcast).
    fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
}

/// The closed, EXACT-VALUED gate that decides whether a provider is active for
/// a file BEFORE the shallow pass runs.
///
/// There is deliberately NO predicate arm — the gate is matched by exact value
/// so the session-side [`active-provider index`] can build a `Map<gate-key,
/// provider-list>` and look up a file's active set in O(1) (or skip entirely
/// when no provider is registered).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptFactSyntaxGate {
    /// Active when the file's carrier language matches this id (e.g. a
    /// framework whose macros live in a carrier-language script block).
    CarrierLanguage(LanguageId),
    /// Active when the file's script imports this exact module specifier
    /// (e.g. a framework whose API is reached through a known package import).
    ImportSpecifier(&'static str),
}

/// One framework's script-fact provider — both halves of the seam.
///
/// Owned by `verter_semantic` (the crate that owns the OXC pass). Registration
/// is session-side; the dispatcher only ever sees the resolved active slice.
///
/// The provider owns BOTH halves: the syntax-capture half ([`Self::capture`],
/// syntax-only) and the per-provider resolved-validation logic
/// ([`Self::validate`]) the session drives on demand. The session feeds
/// [`Self::validate`] NEUTRAL resolved data ([`ResolvedValidationCx`]) so the
/// trait stays free of session resolver types — `verter_semantic` names neither
/// the import resolver nor the host capability snapshot.
pub trait ScriptFactProvider: Send + Sync {
    /// The adapter this provider captures candidates for.
    fn adapter_id(&self) -> FrameworkAdapterId;
    /// A monotonically-versioned provider identity, folded into the
    /// content-addressed candidate cache key so a provider upgrade misses the
    /// stale slot.
    fn provider_version(&self) -> u32;
    /// The exact-valued gate deciding whether this provider is active for a
    /// file.
    fn syntax_gate(&self) -> ScriptFactSyntaxGate;
    /// The capability bit ids this provider's RESOLVED facts depend on.
    ///
    /// Neutral string ids the session maps onto its derived capability
    /// snapshot. They are folded (their ON/OFF state) into the resolved-fact
    /// cache sub-key (`consumed_capability_bits`) so a capability flip misses
    /// the stale resolved slot. A provider whose resolved facts do not depend
    /// on any capability returns an empty slice (the default).
    fn consumed_capabilities(&self) -> &[&'static str] {
        &[]
    }
    /// Capture this provider's syntax candidates from the live OXC program.
    ///
    /// SYNTAX-ONLY: may touch the OXC AST + `lower_ts_type`, MUST NOT resolve
    /// imports or read capability bits. A successful capture always returns an
    /// envelope, including an envelope whose typed inventory is exactly empty.
    fn capture(&self, cx: ScriptCandidateCx<'_>) -> FrameworkScriptCandidates;
    /// Re-anchor this provider's captured candidates to the PRODUCING
    /// canonical.
    ///
    /// Capture is path-agnostic (syntax-only), so an authored-type payload REF
    /// a provider captures carries the local-file EMPTY-sentinel anchor
    /// (`canonical_id == ""`). The SESSION drives this re-anchor with the
    /// canonical it is currently materializing — passed as DATA, never
    /// threaded into the capture API — before the candidates enter any
    /// content-addressed store or synthesis input; the PROVIDER owns the typed
    /// payload downcast plus a COHERENT `stable_hash` recompute (the candidate
    /// hash folds the payload refs, so a filled anchor changes it — the
    /// envelope must never carry a hash that disagrees with its payload).
    ///
    /// Contract: fill ONLY empty anchors (a non-empty anchor may be a
    /// cross-file resolver's canonical and is never rewritten — idempotent by
    /// construction). The default is the IDENTITY: a provider whose candidate
    /// payload carries no authored-type payload refs has no anchors to fill,
    /// so its envelope passes through untouched.
    fn absolutize_candidates(
        &self,
        candidates: ExactFrameworkScriptCandidates,
        canonical: &str,
    ) -> ExactFrameworkScriptCandidates {
        candidates.map(|candidates| self.absolutize_candidate_observations(candidates, canonical))
    }
    /// Re-anchor candidate observations that do not carry an exact-capture
    /// proof.
    ///
    /// This is the recovered-parse counterpart of
    /// [`Self::absolutize_candidates`]. It must apply the same typed payload
    /// rewrite and coherent stable-hash rebuild, but it deliberately cannot
    /// mint [`ExactFrameworkScriptCandidates`].
    fn absolutize_candidate_observations(
        &self,
        candidates: FrameworkScriptCandidates,
        _canonical: &str,
    ) -> FrameworkScriptCandidates {
        candidates
    }
    /// The module specifiers this provider's captured candidates reference —
    /// the resolution INPUT the session turns into
    /// [`ResolvedValidationCx::resolved_import_targets`] rows.
    ///
    /// The session always resolves the file's import STATEMENTS; a provider
    /// whose candidates can reference a module WITHOUT an import statement
    /// (a binding-less inline `import("…")` type reference) surfaces those
    /// specifiers here so the session resolves them exactly like statement
    /// specifiers. Pure candidate read — no resolution happens here (the
    /// capture half stays syntax-only). The default reads nothing.
    fn candidate_import_specifiers(&self, _candidates: &FrameworkScriptCandidates) -> Vec<String> {
        Vec::new()
    }
    /// Validate this provider's captured candidates against resolved import
    /// sources + derived capability bits, producing the typed resolved payload.
    ///
    /// The session drives this on demand and feeds it NEUTRAL data
    /// ([`ResolvedValidationCx`]): the captured candidates, the resolved import
    /// targets (the session resolved the candidate specifiers through its own
    /// import resolver and hands the outcome as data), and a capability lookup.
    /// The provider rejects userland look-alikes (a candidate whose import did
    /// not resolve to the framework's package) and reports exact, partial, or
    /// unavailable evidence explicitly.
    fn validate(&self, cx: ResolvedValidationCx<'_>) -> ScriptFactValidation;
}

/// The TYPED resolved-package identity of an import — the package the session's
/// resolver classified the import as backed by, NOT a path-substring derivation.
///
/// A provider tests `target.package == ResolvedPackage::named("svelte")`
/// STRUCTURALLY instead of re-deriving the package from the resolved canonical's
/// `/node_modules/<name>/` shape. The session computes this where the
/// package-backed classification authority lives
/// (`ResolverContext::workspace_is_package_backed`): a bare specifier whose
/// resolved canonical is package-backed carries the specifier's package name; a
/// relative / unresolved / workspace-owned import carries no package identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedPackage {
    /// The installed package's name (`"svelte"`, `"@scope/pkg"`).
    pub name: String,
}

impl ResolvedPackage {
    /// A resolved package by name.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// One resolved import target the session hands the provider's
/// [`ScriptFactProvider::validate`].
///
/// The session resolved `specifier` through its own import resolver; the
/// resolved canonical (if any) is data the provider inspects, and the typed
/// [`ResolvedPackage`] (if any) is the structural package identity the session
/// computed where the package-backed classification authority lives — a provider
/// tests `target.package` structurally rather than re-deriving a package from the
/// canonical's path. Neutral — carries no resolver handle.
#[derive(Debug, Clone)]
pub struct ResolvedImportTarget {
    /// The candidate import specifier the provider captured.
    pub specifier: String,
    /// The canonical the session resolved `specifier` to, or `None` when the
    /// specifier did not resolve (an unresolved / userland look-alike).
    pub resolved_canonical: Option<String>,
    /// The TYPED resolved-package identity (`Some` only when the import resolved
    /// to a package-backed canonical that names an installed package), or `None`
    /// for a relative / workspace-owned / unresolved import — a userland
    /// look-alike from `./fake-svelte` carries `None`.
    pub package: Option<ResolvedPackage>,
}

/// The NEUTRAL resolved-validation context handed to
/// [`ScriptFactProvider::validate`].
///
/// Carries ONLY data: the captured candidates, the resolved import targets the
/// session resolved, and a capability lookup closure. No session resolver
/// handle, no host, no `StoreView` — the resolved-validation logic stays in the
/// provider while the resolved INPUTS are session-computed.
pub struct ResolvedValidationCx<'a> {
    /// The provider's captured syntax candidates for this file.
    pub candidates: &'a FrameworkScriptCandidates,
    /// The resolved import targets (the session resolved each candidate
    /// specifier through its own import resolver).
    pub resolved_import_targets: &'a [ResolvedImportTarget],
    /// Whether a derived capability bit is ON — the session's capability
    /// snapshot, exposed as a neutral lookup.
    pub capability_on: &'a dyn Fn(&str) -> bool,
}

/// Parser-owned, non-script inputs to a framework's shared mode classifier.
///
/// Most files use [`None`]. A carrier parser that owns an explicit framework
/// option or a template-side mode fact supplies it here so the provider can
/// combine those facts with the already-parsed script through the shared mode
/// authority. The enum is closed deliberately: a new framework mode contract
/// must add a typed arm rather than smuggling unstructured metadata through
/// the capture boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameworkScriptModeHint {
    /// Svelte's non-script inputs. `forced_runes` is the explicit
    /// `<svelte:options runes={...}>` override; `template_uses_host_rune` is
    /// the scope-resolved template `$host` inference fact.
    Svelte {
        forced_runes: Option<bool>,
        template_uses_host_rune: bool,
    },
}

/// The syntax-only capture context handed to a [`ScriptFactProvider::capture`].
///
/// Carries ONLY the parse-domain inputs (source text + OXC program + the
/// optional module-script byte region). No host handle, no resolver, no
/// capability snapshot, no `StoreView` — those belong to the session-side
/// resolved-validation half.
pub struct ScriptCandidateCx<'a> {
    /// The file's full (eval-source) text — script bytes at raw carrier offsets.
    pub source: &'a str,
    /// The OXC program for the file's (combined) script.
    pub program: &'a Program<'a>,
    /// Validated neutral owner coordinates for every top-level statement.
    pub top_level_owners: &'a crate::analysis::top_level_owners::TopLevelOwnerTable,
    /// The byte range of the MODULE script block (`<script module>` /
    /// `context="module"`) in `source`, when the carrier has one. A carrier
    /// whose producer records script-region KINDS supplies it so a provider can
    /// classify a declaration's owning block (module vs instance); `None` means
    /// no module split is available (every top-level export is an instance
    /// export — the conservative default).
    pub module_script_region: Option<(u32, u32)>,
    /// Parser-owned explicit framework mode, when the carrier declared one.
    /// Script-inferred mode uses `None` and remains the provider's job.
    pub framework_mode_hint: Option<FrameworkScriptModeHint>,
}

/// One provider's syntax candidates for one file.
///
/// Parse-domain only — a content-addressed artifact. Carries an opaque
/// `Arc<dyn Any + Send + Sync>` payload the OWNING provider downcasts in the
/// resolved-validation half; the envelope stays framework-neutral.
#[derive(Clone)]
pub struct FrameworkScriptCandidates {
    /// The adapter these candidates were captured for.
    pub adapter_id: FrameworkAdapterId,
    /// The capturing provider's version (folded into the candidate cache key).
    pub provider_version: u32,
    /// A structural hash of the captured candidates, invariant under cosmetic
    /// edits — lets the content-addressed candidate slot stay stable across
    /// formatting-only changes.
    pub stable_hash: [u8; 16],
    /// The provider's typed candidate payload (downcast by the owning provider
    /// in the resolved-validation half).
    pub payload: Arc<dyn Any + Send + Sync>,
}

/// A producer-minted proof that one provider's syntax candidate inventory was
/// captured completely, including the exact-empty case.
///
/// Construction is private to the syntax dispatcher. Consumers can inspect the
/// envelope but cannot forge this proof.
#[derive(Clone)]
pub struct ExactFrameworkScriptCandidates {
    candidates: FrameworkScriptCandidates,
}

impl ExactFrameworkScriptCandidates {
    fn new(candidates: FrameworkScriptCandidates) -> Self {
        Self { candidates }
    }

    fn map(self, map: impl FnOnce(FrameworkScriptCandidates) -> FrameworkScriptCandidates) -> Self {
        Self::new(map(self.candidates))
    }

    /// The completely captured candidate envelope.
    #[must_use]
    pub fn candidates(&self) -> &FrameworkScriptCandidates {
        &self.candidates
    }
}

impl std::fmt::Debug for ExactFrameworkScriptCandidates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ExactFrameworkScriptCandidates")
            .field(&self.candidates)
            .finish()
    }
}

impl std::fmt::Debug for FrameworkScriptCandidates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameworkScriptCandidates")
            .field("adapter_id", &self.adapter_id)
            .field("provider_version", &self.provider_version)
            .field("stable_hash", &self.stable_hash)
            .finish_non_exhaustive()
    }
}

/// The per-file collection of every active provider's syntax candidates.
///
/// Empty when no provider was active for the file — the byte-identical
/// pre-existing path produces an empty set with zero allocation.
#[derive(Clone, Debug, Default)]
pub struct FrameworkScriptCandidateSet {
    /// One entry per active provider that captured candidates.
    pub per_provider: Vec<ExactFrameworkScriptCandidates>,
}

impl FrameworkScriptCandidateSet {
    /// Whether no provider was active for this file.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.per_provider.is_empty()
    }

    /// The captured candidates for `adapter_id`, if any.
    #[must_use]
    pub fn for_adapter(
        &self,
        adapter_id: &FrameworkAdapterId,
    ) -> Option<&ExactFrameworkScriptCandidates> {
        self.per_provider
            .iter()
            .find(|c| &c.candidates().adapter_id == adapter_id)
    }
}

/// Run every active provider's syntax capture over `program`, collecting the
/// per-provider candidates.
///
/// EMPTY `active_providers` ⇒ an empty set with ZERO capture work — the
/// byte-identical pre-existing path. Every active provider contributes an exact
/// envelope, including an exact-empty candidate inventory.
#[must_use]
pub fn capture_script_candidates(
    active_providers: &[Arc<dyn ScriptFactProvider>],
    source: &str,
    program: &Program<'_>,
) -> FrameworkScriptCandidateSet {
    let owners =
        crate::analysis::top_level_owners::TopLevelOwnerTable::ordinary_file(program.body.len());
    capture_script_candidates_with_context(active_providers, source, program, None, None, &owners)
}

/// As [`capture_script_candidates`], but supplies the module-script byte region
/// so a provider can classify a declaration's owning block (module vs instance).
/// `None` ⇒ no module split (the conservative default — every top-level export
/// is an instance export).
#[must_use]
pub fn capture_script_candidates_with_module_region(
    active_providers: &[Arc<dyn ScriptFactProvider>],
    source: &str,
    program: &Program<'_>,
    module_script_region: Option<(u32, u32)>,
    top_level_owners: &crate::analysis::top_level_owners::TopLevelOwnerTable,
) -> FrameworkScriptCandidateSet {
    capture_script_candidates_with_context(
        active_providers,
        source,
        program,
        module_script_region,
        None,
        top_level_owners,
    )
}

/// As [`capture_script_candidates_with_module_region`], with an optional
/// parser-owned framework mode override.
///
/// This is the carrier-aware entry used by the session. Keeping the override
/// beside the parsed program makes explicit mode part of the parse-domain
/// capture input while preserving the provider-less zero-work path.
#[must_use]
pub fn capture_script_candidates_with_context(
    active_providers: &[Arc<dyn ScriptFactProvider>],
    source: &str,
    program: &Program<'_>,
    module_script_region: Option<(u32, u32)>,
    framework_mode_hint: Option<FrameworkScriptModeHint>,
    top_level_owners: &crate::analysis::top_level_owners::TopLevelOwnerTable,
) -> FrameworkScriptCandidateSet {
    assert_eq!(
        top_level_owners.len(),
        program.body.len(),
        "validated owner table must cover framework candidate program exactly"
    );
    if active_providers.is_empty() {
        return FrameworkScriptCandidateSet::default();
    }
    let mut per_provider = Vec::new();
    for provider in active_providers {
        let cx = ScriptCandidateCx {
            source,
            program,
            top_level_owners,
            module_script_region,
            framework_mode_hint,
        };
        per_provider.push(ExactFrameworkScriptCandidates::new(provider.capture(cx)));
    }
    FrameworkScriptCandidateSet { per_provider }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    struct FixtureProvider {
        gate: ScriptFactSyntaxGate,
        captured: std::sync::atomic::AtomicUsize,
    }

    #[derive(Debug)]
    struct FixturePayload;
    impl FrameworkScriptFactPayload for FixturePayload {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }

    impl ScriptFactProvider for FixtureProvider {
        fn adapter_id(&self) -> FrameworkAdapterId {
            FrameworkAdapterId::new("fixture-fw")
        }
        fn provider_version(&self) -> u32 {
            1
        }
        fn syntax_gate(&self) -> ScriptFactSyntaxGate {
            self.gate.clone()
        }
        fn capture(&self, _cx: ScriptCandidateCx<'_>) -> FrameworkScriptCandidates {
            self.captured
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            FrameworkScriptCandidates {
                adapter_id: self.adapter_id(),
                provider_version: self.provider_version(),
                stable_hash: [0u8; 16],
                payload: Arc::new(()),
            }
        }
        fn consumed_capabilities(&self) -> &[&'static str] {
            &["fixture-cap"]
        }
        fn validate(&self, cx: ResolvedValidationCx<'_>) -> ScriptFactValidation {
            // Refuse when the consumed capability bit is OFF.
            if !(cx.capability_on)("fixture-cap") {
                return ScriptFactValidation::Unavailable(
                    ScriptFactUnavailableReason::ValidationProducedNoFacts,
                );
            }
            // Reject a userland look-alike: the candidate's import must have
            // resolved to the framework's own installed PACKAGE — tested via the
            // session-computed typed package identity, NOT a path substring.
            let resolved_to_framework = cx
                .resolved_import_targets
                .iter()
                .any(|t| t.package.as_ref().is_some_and(|p| p.name == "fixture-fw"));
            if !resolved_to_framework {
                return ScriptFactValidation::Unavailable(
                    ScriptFactUnavailableReason::ValidationProducedNoFacts,
                );
            }
            ScriptFactValidation::Exact(Arc::new(FixturePayload))
        }
    }

    fn parse<'a>(alloc: &'a Allocator, src: &'a str) -> Program<'a> {
        Parser::new(alloc, src, SourceType::ts()).parse().program
    }

    #[test]
    fn empty_active_providers_is_zero_cost_no_capture() {
        let alloc = Allocator::default();
        let program = parse(&alloc, "const a = 1;");
        let set = capture_script_candidates(&[], "const a = 1;", &program);
        assert!(set.is_empty());
        assert_eq!(set.per_provider.len(), 0);
    }

    #[test]
    fn active_provider_captures_once_and_is_retrievable_by_adapter() {
        let alloc = Allocator::default();
        let provider: Arc<dyn ScriptFactProvider> = Arc::new(FixtureProvider {
            gate: ScriptFactSyntaxGate::CarrierLanguage(LanguageId::new("fixture-fw")),
            captured: std::sync::atomic::AtomicUsize::new(0),
        });
        let program = parse(&alloc, "const a = 1;");
        let set =
            capture_script_candidates(std::slice::from_ref(&provider), "const a = 1;", &program);
        assert_eq!(set.per_provider.len(), 1);
        let id = FrameworkAdapterId::new("fixture-fw");
        assert!(set.for_adapter(&id).is_some());
        assert!(set.for_adapter(&FrameworkAdapterId::new("other")).is_none());
    }

    #[test]
    fn syntax_gate_is_exact_valued_no_predicate_arm() {
        // The gate is matched by exact value (the session index keys on it).
        let g1 = ScriptFactSyntaxGate::CarrierLanguage(LanguageId::new("vue"));
        let g2 = ScriptFactSyntaxGate::CarrierLanguage(LanguageId::new("vue"));
        let g3 = ScriptFactSyntaxGate::ImportSpecifier("@corp/fw");
        assert_eq!(g1, g2);
        assert_ne!(g1, g3);
    }

    #[test]
    fn fact_payload_downcast_is_typed() {
        let payload: Arc<dyn FrameworkScriptFactPayload> = Arc::new(FixturePayload);
        assert!(payload.as_any().downcast_ref::<FixturePayload>().is_some());
        assert!(payload.as_any().downcast_ref::<u32>().is_none());
    }

    fn fixture_provider() -> FixtureProvider {
        FixtureProvider {
            gate: ScriptFactSyntaxGate::CarrierLanguage(LanguageId::new("fixture-fw")),
            captured: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn fixture_candidates(provider: &FixtureProvider) -> FrameworkScriptCandidates {
        FrameworkScriptCandidates {
            adapter_id: provider.adapter_id(),
            provider_version: provider.provider_version(),
            stable_hash: [0u8; 16],
            payload: Arc::new(()),
        }
    }

    #[test]
    fn validate_refuses_when_consumed_capability_bit_is_off() {
        let provider = fixture_provider();
        let candidates = fixture_candidates(&provider);
        let targets = vec![ResolvedImportTarget {
            specifier: "fixture-fw".to_string(),
            resolved_canonical: Some("/node_modules/fixture-fw/index.d.ts".to_string()),
            package: Some(ResolvedPackage::named("fixture-fw")),
        }];
        // Capability bit OFF ⇒ the resolved-validation is unavailable, even
        // though the import resolved to the framework package.
        let cx = ResolvedValidationCx {
            candidates: &candidates,
            resolved_import_targets: &targets,
            capability_on: &|_| false,
        };
        assert!(matches!(
            provider.validate(cx),
            ScriptFactValidation::Unavailable(
                ScriptFactUnavailableReason::ValidationProducedNoFacts
            )
        ));
    }

    #[test]
    fn validate_rejects_a_userland_look_alike() {
        let provider = fixture_provider();
        let candidates = fixture_candidates(&provider);
        // The candidate's import resolved to a userland file, NOT the framework
        // package ⇒ rejected even with the capability bit ON.
        let targets = vec![ResolvedImportTarget {
            specifier: "fixture-fw".to_string(),
            resolved_canonical: Some("/src/my-own-fixture-fw-shim.ts".to_string()),
            // A userland shim is workspace-owned, not package-backed ⇒ no typed
            // package identity.
            package: None,
        }];
        let cx = ResolvedValidationCx {
            candidates: &candidates,
            resolved_import_targets: &targets,
            capability_on: &|_| true,
        };
        assert!(matches!(
            provider.validate(cx),
            ScriptFactValidation::Unavailable(
                ScriptFactUnavailableReason::ValidationProducedNoFacts
            )
        ));
    }

    #[test]
    fn validate_produces_a_typed_payload_when_resolved_and_capability_on() {
        let provider = fixture_provider();
        let candidates = fixture_candidates(&provider);
        let targets = vec![ResolvedImportTarget {
            specifier: "fixture-fw".to_string(),
            resolved_canonical: Some("/node_modules/fixture-fw/index.d.ts".to_string()),
            package: Some(ResolvedPackage::named("fixture-fw")),
        }];
        let cx = ResolvedValidationCx {
            candidates: &candidates,
            resolved_import_targets: &targets,
            capability_on: &|cap| cap == "fixture-cap",
        };
        let ScriptFactValidation::Exact(payload) = provider.validate(cx) else {
            panic!("a resolved + capability-on candidate validates exactly");
        };
        assert!(payload.as_any().downcast_ref::<FixturePayload>().is_some());
    }

    #[test]
    fn consumed_capabilities_default_is_empty() {
        struct NoCapProvider;
        impl ScriptFactProvider for NoCapProvider {
            fn adapter_id(&self) -> FrameworkAdapterId {
                FrameworkAdapterId::new("no-cap")
            }
            fn provider_version(&self) -> u32 {
                1
            }
            fn syntax_gate(&self) -> ScriptFactSyntaxGate {
                ScriptFactSyntaxGate::ImportSpecifier("no-cap")
            }
            fn capture(&self, _cx: ScriptCandidateCx<'_>) -> FrameworkScriptCandidates {
                FrameworkScriptCandidates {
                    adapter_id: self.adapter_id(),
                    provider_version: self.provider_version(),
                    stable_hash: [0; 16],
                    payload: Arc::new(()),
                }
            }
            fn validate(&self, _cx: ResolvedValidationCx<'_>) -> ScriptFactValidation {
                ScriptFactValidation::Unavailable(
                    ScriptFactUnavailableReason::ValidationProducedNoFacts,
                )
            }
        }
        assert!(NoCapProvider.consumed_capabilities().is_empty());
    }
}
