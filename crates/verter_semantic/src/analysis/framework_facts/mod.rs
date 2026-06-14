//! The syntax-capture half of the framework script-fact seam.
//!
//! The seam is split in two: a SYNTAX-CAPTURE half (here) and a
//! RESOLVED-VALIDATION half (session-side, `framework/script_facts.rs`).
//! `verter_semantic` owns the OXC shallow pass, so it owns the syntax-capture
//! half: the [`ScriptFactProvider`] trait, the closed [`ScriptFactSyntaxGate`],
//! the per-file parse-domain [`FrameworkScriptCandidates`] /
//! [`FrameworkScriptCandidateSet`] collection, and the resolved-fact envelope
//! ([`FrameworkScriptFacts`] / [`FrameworkScriptFactSet`]) whose typed payload
//! rides behind the same token-gated hidden-downcast doctrine the parse
//! carriers use.
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

/// Downcast a resolved framework-script-fact payload `Arc` to its concrete
/// type, or `None` when it is not a `T`.
#[must_use]
pub fn downcast_fact_payload<T: FrameworkScriptFactPayload>(
    payload: Arc<dyn FrameworkScriptFactPayload>,
) -> Option<Arc<T>> {
    payload.as_any_arc().downcast::<T>().ok()
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
    /// imports or read capability bits. Returns `None` when the program carries
    /// no candidates for this provider.
    fn capture(&self, cx: ScriptCandidateCx<'_>) -> Option<FrameworkScriptCandidates>;
    /// Validate this provider's captured candidates against resolved import
    /// sources + derived capability bits, producing the typed resolved payload.
    ///
    /// The session drives this on demand and feeds it NEUTRAL data
    /// ([`ResolvedValidationCx`]): the captured candidates, the resolved import
    /// targets (the session resolved the candidate specifiers through its own
    /// import resolver and hands the outcome as data), and a capability lookup.
    /// The provider rejects userland look-alikes (a candidate whose import did
    /// not resolve to the framework's package) and refuses emission when a
    /// consumed capability bit is OFF. Returns `None` when the candidates do not
    /// validate into a resolved fact — the honest answer, never a fabricated
    /// payload.
    fn validate(&self, cx: ResolvedValidationCx<'_>)
        -> Option<Arc<dyn FrameworkScriptFactPayload>>;
}

/// One resolved import target the session hands the provider's
/// [`ScriptFactProvider::validate`].
///
/// The session resolved `specifier` through its own import resolver; the
/// resolved canonical (if any) is data the provider inspects to reject userland
/// look-alikes. Neutral — carries no resolver handle.
#[derive(Debug, Clone)]
pub struct ResolvedImportTarget {
    /// The candidate import specifier the provider captured.
    pub specifier: String,
    /// The canonical the session resolved `specifier` to, or `None` when the
    /// specifier did not resolve (an unresolved / userland look-alike).
    pub resolved_canonical: Option<String>,
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
    /// The byte range of the MODULE script block (`<script module>` /
    /// `context="module"`) in `source`, when the carrier has one. A carrier
    /// whose producer records script-region KINDS supplies it so a provider can
    /// classify a declaration's owning block (module vs instance); `None` means
    /// no module split is available (every top-level export is an instance
    /// export — the conservative default).
    pub module_script_region: Option<(u32, u32)>,
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
    pub per_provider: Vec<FrameworkScriptCandidates>,
}

impl FrameworkScriptCandidateSet {
    /// Whether no provider captured candidates for this file.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.per_provider.is_empty()
    }

    /// The captured candidates for `adapter_id`, if any.
    #[must_use]
    pub fn for_adapter(
        &self,
        adapter_id: &FrameworkAdapterId,
    ) -> Option<&FrameworkScriptCandidates> {
        self.per_provider
            .iter()
            .find(|c| &c.adapter_id == adapter_id)
    }
}

/// One provider's RESOLVED framework script facts (the resolved-validation half output).
///
/// Produced session-side by validating syntax candidates against resolved
/// import sources + capability bits. The typed [`FrameworkScriptFactPayload`]
/// rides behind the hidden-downcast doctrine.
#[derive(Clone)]
pub struct FrameworkScriptFacts {
    /// The adapter these facts belong to.
    pub adapter_id: FrameworkAdapterId,
    /// The producing provider's version.
    pub provider_version: u32,
    /// Structural hash of the resolved facts.
    pub stable_hash: [u8; 16],
    /// The resolved typed payload.
    pub payload: Arc<dyn FrameworkScriptFactPayload>,
}

impl std::fmt::Debug for FrameworkScriptFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameworkScriptFacts")
            .field("adapter_id", &self.adapter_id)
            .field("provider_version", &self.provider_version)
            .field("stable_hash", &self.stable_hash)
            .finish_non_exhaustive()
    }
}

/// The per-file collection of every provider's resolved facts.
#[derive(Clone, Debug, Default)]
pub struct FrameworkScriptFactSet {
    /// One entry per provider that produced resolved facts.
    pub per_provider: Vec<FrameworkScriptFacts>,
}

impl FrameworkScriptFactSet {
    /// Whether no provider produced resolved facts for this file.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.per_provider.is_empty()
    }
}

/// Run every active provider's syntax capture over `program`, collecting the
/// per-provider candidates.
///
/// EMPTY `active_providers` ⇒ an empty set with ZERO capture work — the
/// byte-identical pre-existing path. A provider that captures nothing
/// contributes no entry.
#[must_use]
pub fn capture_script_candidates(
    active_providers: &[Arc<dyn ScriptFactProvider>],
    source: &str,
    program: &Program<'_>,
) -> FrameworkScriptCandidateSet {
    capture_script_candidates_with_module_region(active_providers, source, program, None)
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
) -> FrameworkScriptCandidateSet {
    if active_providers.is_empty() {
        return FrameworkScriptCandidateSet::default();
    }
    let mut per_provider = Vec::new();
    for provider in active_providers {
        let cx = ScriptCandidateCx {
            source,
            program,
            module_script_region,
        };
        if let Some(candidates) = provider.capture(cx) {
            per_provider.push(candidates);
        }
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
        fn capture(&self, _cx: ScriptCandidateCx<'_>) -> Option<FrameworkScriptCandidates> {
            self.captured
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(FrameworkScriptCandidates {
                adapter_id: self.adapter_id(),
                provider_version: self.provider_version(),
                stable_hash: [0u8; 16],
                payload: Arc::new(()),
            })
        }
        fn consumed_capabilities(&self) -> &[&'static str] {
            &["fixture-cap"]
        }
        fn validate(
            &self,
            cx: ResolvedValidationCx<'_>,
        ) -> Option<Arc<dyn FrameworkScriptFactPayload>> {
            // Refuse when the consumed capability bit is OFF.
            if !(cx.capability_on)("fixture-cap") {
                return None;
            }
            // Reject a userland look-alike: the candidate's import must have
            // resolved INTO the framework's own installed package directory,
            // not merely to a file that mentions the name.
            let resolved_to_framework = cx.resolved_import_targets.iter().any(|t| {
                t.resolved_canonical
                    .as_deref()
                    .is_some_and(|c| c.contains("/node_modules/fixture-fw/"))
            });
            if !resolved_to_framework {
                return None;
            }
            Some(Arc::new(FixturePayload))
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
        }];
        // Capability bit OFF ⇒ the resolved-validation refuses (None), even
        // though the import resolved to the framework package.
        let cx = ResolvedValidationCx {
            candidates: &candidates,
            resolved_import_targets: &targets,
            capability_on: &|_| false,
        };
        assert!(provider.validate(cx).is_none());
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
        }];
        let cx = ResolvedValidationCx {
            candidates: &candidates,
            resolved_import_targets: &targets,
            capability_on: &|_| true,
        };
        assert!(provider.validate(cx).is_none());
    }

    #[test]
    fn validate_produces_a_typed_payload_when_resolved_and_capability_on() {
        let provider = fixture_provider();
        let candidates = fixture_candidates(&provider);
        let targets = vec![ResolvedImportTarget {
            specifier: "fixture-fw".to_string(),
            resolved_canonical: Some("/node_modules/fixture-fw/index.d.ts".to_string()),
        }];
        let cx = ResolvedValidationCx {
            candidates: &candidates,
            resolved_import_targets: &targets,
            capability_on: &|cap| cap == "fixture-cap",
        };
        let payload = provider
            .validate(cx)
            .expect("a resolved + capability-on candidate validates");
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
            fn capture(&self, _cx: ScriptCandidateCx<'_>) -> Option<FrameworkScriptCandidates> {
                None
            }
            fn validate(
                &self,
                _cx: ResolvedValidationCx<'_>,
            ) -> Option<Arc<dyn FrameworkScriptFactPayload>> {
                None
            }
        }
        assert!(NoCapProvider.consumed_capabilities().is_empty());
    }
}
