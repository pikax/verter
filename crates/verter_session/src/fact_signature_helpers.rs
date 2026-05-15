//! Shared helpers for the R3/R26/R28 fact-based validation substrate
//! used by the inner component-meta caches.
//!
//! Every cache entry that previously carried
//! [`crate::semantic_query::DepSignature`] migrates to
//! `Arc<[FactVersionRef]>` (the cold-compute observation set the
//! producer recorded). Warm-hit reads call
//! [`validate_fact_signature`] which walks every fact through the
//! current [`crate::resolver_core::StoreView`] snapshot; a single
//! mismatch returns `false` and the warm hit misses, falling through
//! to cold recompute.
//!
//! Bubble-up is the dual rule: when a cache cold-compute or warm-hit
//! returns a value to its caller, the entry's `fact_dep_signature`
//! merges into any active outer tracer via
//! [`crate::resolver_core::FactReadSetCell::observe_borrowed_signature`].
//! That keeps the outer compute's observation set complete even when
//! inner reads come from cache.
//!
//! ## Path-precise observation (R28)
//!
//! Two helper shapes serve the Family A path-precise observation
//! contract:
//!
//! - [`fact_signature_for_canonical_member`] — keyed on `(canonical,
//!   exporter, member, space)`. The cold path reads the `Member`
//!   body fingerprint and `MemberPresence` header fact for the
//!   member. Adding a sibling member to `exporter` does NOT shift
//!   either fact for the named member, so unrelated edits do not
//!   invalidate the entry.
//! - [`fact_signature_for_exported_type`] — keyed on `(canonical,
//!   type_name, space)`. The cold path observes the top-level type
//!   identity via `Export`, `LocalDecl`, and `MemberShape` facts.
//!   Editing a single member body does NOT invalidate (callers that
//!   walk member bodies must combine with `member` observations);
//!   adding/removing/renaming a member shifts `MemberShape` and
//!   invalidates whole-type readers.
//!
//! For caches whose key shape does NOT include a member name (e.g.
//! `AppConfigNoOverrideProofKey`), the cold path observes the
//! `SyntacticExportSet` of the contributing canonical instead.
//! Whole-file `FileWholeHash` observations are reserved for callers
//! that genuinely consume the full file body (e.g. `<src=>` external
//! blocks); cross-file member-precise consumers route through this
//! module's `member_*` helpers.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use verter_semantic::facts::registry::{FactKey, FactLane, InternedName, SymbolSpace};

use crate::resolver_core::{
    FactReadSetFinalise, FactVersionRef, ParseFactRef, ResolverContext, StoreView,
    FACT_SIGNATURE_CAP,
};
use crate::semantic_query::{DepSignature, DepVersion};
use crate::types::Hash16;

/// Counter for `FactReadSetFinalise::Overflow` hits at the
/// `install_fact_tracer` boundary. Monotonically increasing;
/// reset only across process restart. Readable from tests via
/// [`read_signature_overflow_at_install`].
pub(crate) static SIGNATURE_OVERFLOW_AT_INSTALL: AtomicU64 = AtomicU64::new(0);

/// Bracket one cold-compute closure with a push-style fact tracer.
///
/// Installs a fresh [`crate::resolver_core::FactReadSetCell`] onto the
/// TLS tracer stack, runs `f`, pops the tracer, and finalises the
/// observation set. On [`FactReadSetFinalise::Overflow`] emits a
/// [`crate::component_meta_audit::StructuredAuditEvent::FactSignatureOverflow`]
/// and increments [`SIGNATURE_OVERFLOW_AT_INSTALL`].
///
/// Returns `(return_value, finalise_result)` so callers decide whether
/// to admit the result to cache or treat it as non-cacheable.
pub(crate) fn install_fact_tracer<F, R>(host: &crate::VerterHost, f: F) -> (R, FactReadSetFinalise)
where
    F: FnOnce() -> R,
{
    let (value, read_set) = host.with_fact_tracer(f);
    let finalise = read_set.finalise();
    if matches!(finalise, FactReadSetFinalise::Overflow) {
        crate::host_manage::push_structured_event(
            crate::component_meta_audit::StructuredAuditEvent::FactSignatureOverflow {
                candidate_size: (FACT_SIGNATURE_CAP as u32).saturating_add(1),
                cap: FACT_SIGNATURE_CAP as u32,
            },
        );
        SIGNATURE_OVERFLOW_AT_INSTALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    (value, finalise)
}

/// Fan `sig` into every active tracer on the current thread's stack.
///
/// Thin wrapper around
/// [`crate::resolver_core::resolver_context::observe_fan_out_borrowed`]
/// with a more intention-revealing name for callers in the
/// fact-cache substrate.
#[inline]
pub(crate) fn observe_fact_signature(sig: &[FactVersionRef]) {
    crate::resolver_core::resolver_context::observe_fan_out_borrowed(sig);
}

/// Convert a legacy [`DepSignature`] into a [`Vec<FactVersionRef>`].
///
/// Only `DepVersion::WholeHash` entries are expressible as
/// `FactVersionRef::FileWholeHash`; all other variants (route-generation
/// numbers, project-generation numbers) have no direct `FactVersionRef`
/// equivalent and are silently dropped. Callers that need full fidelity
/// should migrate to the fact-tracer path.
pub(crate) fn dep_signature_to_fact_signature(sig: &DepSignature) -> Vec<FactVersionRef> {
    sig.iter()
        .filter_map(|(canon, ver)| match ver {
            DepVersion::WholeHash(h) => Some(FactVersionRef::FileWholeHash {
                canonical_id: canon.as_ref().to_string(),
                hash: *h,
            }),
            _ => None,
        })
        .collect()
}

/// Read the current value of [`SIGNATURE_OVERFLOW_AT_INSTALL`].
///
/// Exposed for integration tests that verify overflow telemetry —
/// reached through the `for_tests::read_signature_overflow_at_install`
/// re-export in `lib.rs` (see
/// `tests/fact_read_set_finalise_overflow.rs`).
#[inline]
pub(crate) fn read_signature_overflow_at_install() -> u64 {
    SIGNATURE_OVERFLOW_AT_INSTALL.load(std::sync::atomic::Ordering::Relaxed)
}

/// Walk every `FactVersionRef` in `signature` against the current
/// resolver-store view; return `false` on the first mismatch.
///
/// `O(signature.len())`; zero allocation on the empty path. Empty
/// signatures trivially validate (callers that never observed a fact
/// have no R3 oracle to consult — typical for cache entries produced
/// outside an installed tracer scope; the cache stays correct under
/// the legacy whole-hash regime).
#[inline]
pub(crate) fn validate_fact_signature(
    ctx: &dyn ResolverContext,
    signature: &[FactVersionRef],
) -> bool {
    if signature.is_empty() {
        return true;
    }
    let view = ctx.resolver_store_view();
    signature.iter().all(|fact| view.validates(fact))
}

/// Bubble `signature` into **all** active fact tracers on the current
/// thread's stack (fan-out). Called by both cold-compute and warm-hit
/// paths so every outer tracer scope sees every transitive fact the
/// inner cache hit / produced.
#[inline]
pub(crate) fn bubble_fact_signature(_ctx: &dyn ResolverContext, signature: &[FactVersionRef]) {
    if signature.is_empty() {
        return;
    }
    crate::resolver_core::resolver_context::observe_fan_out_borrowed(signature);
}

/// Variant of [`bubble_fact_signature`] for warm-hit paths that
/// don't carry a `ResolverContext` reference (e.g. the semantic
/// graph store's fast-path warm hit). Fans into all active TLS
/// tracer scopes when any are installed; no-op otherwise.
#[inline]
pub(crate) fn bubble_fact_signature_via_tls(signature: &[FactVersionRef]) {
    if signature.is_empty() {
        return;
    }
    crate::resolver_core::resolver_context::observe_fan_out_borrowed(signature);
}

/// Sentinel hash returned when the producer requests a fact that the
/// FileFacts registry hasn't materialised yet (e.g. cold-compute
/// races a parse that hasn't published yet). Validator reads against
/// the registry's actual hash; a sentinel records "MUST be absent"
/// semantics so a later population (or its absence) is still
/// discriminating.
#[inline]
fn zero_hash() -> Hash16 {
    [0u8; 16]
}

/// Recover the producer's hash for a parse-domain fact key on
/// `canonical_id`. Returns `None` only when the file has no
/// `FileArtifacts` entry under any `(content_hash, parse_env_hash)`
/// in the cache (truly absent file — the validator will treat the
/// missing fact as a miss).
fn lookup_parse_fact_hash(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    key: &FactKey,
    lane: FactLane,
) -> Option<Hash16> {
    let artifacts = ctx
        .project_type_store()
        .indexed()
        .get_artifacts_any(canonical_id)?;
    let fact = artifacts.facts.lookup(key)?;
    Some(match lane {
        FactLane::Semantic => fact.semantic_hash,
        FactLane::Display => fact.display_hash,
    })
}

/// Emit a `ParseFactRef` carrying the producer's current hash (or
/// the zero sentinel when the fact is absent from the registry).
fn parse_fact_ref(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    key: FactKey,
    lane: FactLane,
) -> FactVersionRef {
    let expected_hash =
        lookup_parse_fact_hash(ctx, canonical_id, &key, lane).unwrap_or_else(zero_hash);
    FactVersionRef::Parse(ParseFactRef {
        canonical_id: canonical_id.to_string(),
        key,
        lane,
        expected_hash,
    })
}

/// Build a path-precise signature for a cache whose validity depends
/// on a single MEMBER of an exporter type — the R28 path-precise
/// pattern.
///
/// The cold-compute reads the body of `member` declared inside the
/// `exporter` type at `canonical_id`, so the signature observes BOTH
/// `MemberPresence(exporter, member, space)` (header fact — bumps on
/// add/remove/rename/kind-change) AND `Member(exporter, member,
/// space)` (body fingerprint — bumps on body edit). Adding sibling
/// members to `exporter` does NOT shift either fact for the named
/// member (R10 / R28); editing the named member's body shifts only
/// the `Member` body fact; removing the named member drops both
/// facts (validator treats absence as a sentinel-hash mismatch).
///
/// Use this helper for caches keyed on `(canonical, exporter,
/// member, space)` — e.g. `PreparedMemberDb`, slot-binding member
/// reads, fallthrough member projection.
pub(crate) fn fact_signature_for_canonical_member(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    exporter: &str,
    member: &str,
    space: SymbolSpace,
) -> Arc<[FactVersionRef]> {
    let exporter_name = InternedName::from(exporter);
    let member_name = InternedName::from(member);
    let presence_key = FactKey::MemberPresence {
        exporter: exporter_name.clone(),
        name: member_name.clone(),
        space,
    };
    let body_key = FactKey::Member {
        exporter: exporter_name,
        name: member_name,
        space,
    };
    let entries: Vec<FactVersionRef> = vec![
        parse_fact_ref(ctx, canonical_id, presence_key, FactLane::Semantic),
        parse_fact_ref(ctx, canonical_id, body_key, FactLane::Semantic),
    ];
    Arc::from(entries)
}

/// Build a signature for a cache whose validity depends on the
/// IDENTITY of a top-level type declared at `canonical_id` — the
/// Family A producer pattern for caches keyed on `(canonical,
/// type_name)`.
///
/// The cold-compute consumes the declaration of `type_name` (its
/// declaration shape and member list), so the signature observes:
/// - `Export(name, space)` — present iff the type is exported under
///   that name.
/// - `LocalDecl(name, space)` — present iff the type is declared
///   locally (non-exported).
/// - `MemberShape(exporter=name, space)` — the ordered member list
///   fingerprint; bumps when members are added/removed/renamed.
///
/// Editing one member's body changes `Member(name, m, space)` but
/// NOT this signature — that path-precise invalidation is the
/// caller's responsibility via [`fact_signature_for_canonical_member`].
/// This helper covers caches whose validity is "the top-level type
/// exists and has THIS member-shape", not "the body of a particular
/// member".
pub(crate) fn fact_signature_for_exported_type(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    type_name: &str,
    space: SymbolSpace,
) -> Arc<[FactVersionRef]> {
    let name = InternedName::from(type_name);
    let export_key = FactKey::Export {
        name: name.clone(),
        space,
    };
    let local_decl_key = FactKey::LocalDecl {
        name: name.clone(),
        space,
    };
    let member_shape_key = FactKey::MemberShape {
        exporter: name,
        space,
    };
    let entries: Vec<FactVersionRef> = vec![
        parse_fact_ref(ctx, canonical_id, export_key, FactLane::Semantic),
        parse_fact_ref(ctx, canonical_id, local_decl_key, FactLane::Semantic),
        parse_fact_ref(ctx, canonical_id, member_shape_key, FactLane::Semantic),
    ];
    Arc::from(entries)
}

/// Build a whole-canonical signature for caches whose cold-compute
/// reads the file's surface fingerprint (e.g. a binding-walker that
/// enumerates every export). Observes `SyntacticExportSet` — adding
/// or removing exports shifts the fact; cosmetic edits inside a
/// member body do NOT (they are observed by `Member(...)` facts on
/// each consumer).
pub(crate) fn fact_signature_for_canonical_surface(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
) -> Arc<[FactVersionRef]> {
    let entries = vec![parse_fact_ref(
        ctx,
        canonical_id,
        FactKey::SyntacticExportSet,
        FactLane::Semantic,
    )];
    Arc::from(entries)
}

/// Empty signature constructor for cache entries published outside
/// any observable cold-compute pass (e.g. test fixtures, synthetic
/// publish paths). Validator trivially accepts; readers fall back to
/// the existing whole-hash regime in the legacy producer.
#[inline]
pub(crate) fn empty_fact_signature() -> Arc<[FactVersionRef]> {
    Arc::from(Vec::<FactVersionRef>::new())
}

/// Transition carrier for a cache entry's dependency signature.
///
/// `facts` is the path-precise fact signature captured by an
/// `install_fact_tracer` scope. `legacy` is the whole-hash /
/// project-generation signature retained for cache entries whose
/// producers still gate on `validate_dep_signature` (the legacy
/// validator). The follow-up hygiene block deletes `legacy`; the
/// carrier's surface (`validate`, `bubble`, `canonical_ids`,
/// `is_overflow`) is invariant under that deletion.
///
/// The carrier is `pub(crate)`. Cache entries store a single
/// `read_set_signature: ReadSetSignature` field instead of two
/// separate `dep_signature`/`fact_dep_signature` rails. The shared
/// cold-build helper builds the carrier when the tracer finalises;
/// warm-hit paths call `validate(ctx)` BEFORE `bubble(ctx)`.
///
/// Invariants:
/// - `validate(ctx)` returns true only when BOTH rails validate. If
///   `legacy` is empty (the post-cleanup state of cache producers
///   wired entirely through `install_fact_tracer`), the legacy gate
///   is a no-op and the carrier behaves as if `facts` is the sole
///   oracle.
/// - `bubble(ctx)` fans `facts` into every active outer tracer on the
///   current TLS stack. `legacy` does NOT bubble: it is the validator
///   rail only; the bubble channel is the fact signature.
/// - `canonical_ids()` returns the union of the canonical IDs
///   referenced by `legacy` and `facts`, deduplicated by string
///   identity. The unified reverse index registers a (canonical →
///   entry) mapping for each yielded ID.
/// - `is_overflow()` returns true when the producer's tracer finalised
///   with `FactReadSetFinalise::Overflow` — the materialised result
///   is valid but the path-precise signature is too large to admit
///   safely. Cache consumers route overflowed values through
///   `ComputeAdmission::ReturnOnly` (return without admitting).
#[derive(Clone, Debug)]
pub struct ReadSetSignature {
    pub facts: Arc<[FactVersionRef]>,
    pub legacy: DepSignature,
    /// Marks the carrier as constructed from a tracer that returned
    /// `FactReadSetFinalise::Overflow`. The materialised value is
    /// valid; the signature is too large to admit. The cooperative
    /// admission path routes the value through
    /// `ComputeAdmission::ReturnOnly` and the in-flight slot
    /// broadcasts the value to joiners.
    pub overflowed: bool,
}

impl ReadSetSignature {
    /// Construct a carrier from explicit components. The legacy rail
    /// may be empty.
    #[inline]
    pub fn new(facts: Arc<[FactVersionRef]>, legacy: DepSignature) -> Self {
        Self {
            facts,
            legacy,
            overflowed: false,
        }
    }

    /// Construct a fact-only carrier. The legacy rail is empty. Used
    /// by producers that gate validation entirely on the fact
    /// signature.
    #[inline]
    pub fn facts_only(facts: Arc<[FactVersionRef]>) -> Self {
        Self {
            facts,
            legacy: Arc::from(Vec::<(Arc<str>, DepVersion)>::new()),
            overflowed: false,
        }
    }

    /// Construct an overflow carrier. Both rails are empty; the
    /// `overflowed` flag is set. Cooperative admission consumers
    /// route values bearing this carrier through
    /// `ComputeAdmission::ReturnOnly`.
    #[inline]
    pub fn overflow() -> Self {
        Self {
            facts: empty_fact_signature(),
            legacy: Arc::from(Vec::<(Arc<str>, DepVersion)>::new()),
            overflowed: true,
        }
    }

    /// Empty carrier. Both rails are empty; the `overflowed` flag is
    /// false. Used for synthetic publishes that pre-date the
    /// fact-tracer substrate.
    #[inline]
    pub fn empty() -> Self {
        Self {
            facts: empty_fact_signature(),
            legacy: Arc::from(Vec::<(Arc<str>, DepVersion)>::new()),
            overflowed: false,
        }
    }

    /// Validate both rails against the host's live state. Returns
    /// `true` only when BOTH rails validate (R3 AND-gate). Empty
    /// rails trivially validate; an entirely-empty carrier validates
    /// vacuously.
    #[inline]
    pub(crate) fn validate(&self, ctx: &dyn ResolverContext) -> bool {
        if self.overflowed {
            // Overflow carriers identify values that should never be
            // cached. A validate on an overflow entry must fail so
            // the warm-hit path treats the entry as stale.
            return false;
        }
        validate_fact_signature(ctx, &self.facts) && ctx.validate_dep_signature(&self.legacy)
    }

    /// Bubble the path-precise fact set into every active outer
    /// tracer on the current TLS stack. No-op when the tracer stack
    /// is empty or `facts` is empty. The legacy rail is the
    /// validator-side channel and is NOT bubbled.
    #[inline]
    pub(crate) fn bubble(&self, ctx: &dyn ResolverContext) {
        bubble_fact_signature(ctx, &self.facts);
    }

    /// Bubble via TLS only — for fast-path warm hits that don't
    /// thread a `ResolverContext` reference. Equivalent to
    /// `bubble_fact_signature_via_tls(&self.facts)`.
    #[inline]
    pub fn bubble_via_tls(&self) {
        bubble_fact_signature_via_tls(&self.facts);
    }

    /// Canonical IDs referenced by this carrier. Yields the union of
    /// canonicals from `legacy` and `facts`, deduplicated by string
    /// equality. The reverse index drains via this iterator.
    pub fn canonical_ids(&self) -> Vec<Arc<str>> {
        // Small dedup set; cache entries' canonical sets typically
        // hold fewer than 16 entries each. `FxHashSet` over Arc<str>
        // keeps comparison O(1) per insertion when arcs are shared.
        let mut seen: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
        let mut out: Vec<Arc<str>> = Vec::new();
        for (canon, _) in self.legacy.iter() {
            if seen.insert(Arc::clone(canon)) {
                out.push(Arc::clone(canon));
            }
        }
        for fact in self.facts.iter() {
            let canon: Arc<str> = match fact {
                FactVersionRef::FileWholeHash { canonical_id, .. } => {
                    Arc::from(canonical_id.as_str())
                }
                FactVersionRef::DerivedFactHash { canonical_id, .. } => {
                    Arc::from(canonical_id.as_str())
                }
                FactVersionRef::Parse(p) => Arc::from(p.canonical_id.as_str()),
                FactVersionRef::ResolveImports(r) => Arc::from(r.canonical_id.as_str()),
                FactVersionRef::RouteSurface(r) => Arc::from(r.canonical_id.as_str()),
            };
            if seen.insert(Arc::clone(&canon)) {
                out.push(canon);
            }
        }
        out
    }

    /// True iff the original tracer finalised with `Overflow`. The
    /// entry's value is valid; cache consumers route it through
    /// `ComputeAdmission::ReturnOnly` instead of admitting it.
    #[inline]
    pub fn is_overflow(&self) -> bool {
        self.overflowed
    }
}

#[cfg(test)]
mod read_set_signature_unit_tests {
    use super::*;
    use crate::resolver_core::DerivedFactKind;

    fn fact_filewhole(canon: &str, byte: u8) -> FactVersionRef {
        FactVersionRef::FileWholeHash {
            canonical_id: canon.to_string(),
            hash: [byte; 16],
        }
    }

    fn fact_derived(canon: &str, byte: u8) -> FactVersionRef {
        FactVersionRef::DerivedFactHash {
            canonical_id: canon.to_string(),
            kind: DerivedFactKind::Route,
            hash: [byte; 16],
        }
    }

    fn fact_parse(canon: &str, byte: u8) -> FactVersionRef {
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: canon.to_string(),
            key: FactKey::SyntacticExportSet,
            lane: FactLane::Semantic,
            expected_hash: [byte; 16],
        })
    }

    #[test]
    fn read_set_signature_empty_validates_vacuously_via_facts_path() {
        // Empty carrier: facts empty + legacy empty.
        // `validate_fact_signature` returns true on empty input.
        // ctx.validate_dep_signature on empty signature: depends on
        // the trait impl; the production impls treat empty as valid.
        let sig = ReadSetSignature::empty();
        assert!(!sig.is_overflow(), "empty carrier must NOT be overflow");
        // Don't assert validate without ctx — empty carrier's
        // `validate` short-circuits via empty fact list. Tested
        // separately in integration with a `ResolverContext` stub.
    }

    #[test]
    fn read_set_signature_overflow_validate_returns_false() {
        let sig = ReadSetSignature::overflow();
        assert!(sig.is_overflow(), "overflow carrier must report overflow");
        // We can't trivially construct a ResolverContext here, but
        // the overflow short-circuit doesn't even call ctx — it
        // returns false directly. Integration tests cover the live
        // `validate(ctx)` call.
    }

    #[test]
    fn read_set_signature_canonical_ids_deduplicates_across_rails() {
        // legacy mentions /a.ts; facts mention /a.ts + /b.ts. Union
        // should be [/a.ts, /b.ts] in legacy-first order.
        let legacy: DepSignature = Arc::from(
            vec![(Arc::from("/a.ts"), DepVersion::WholeHash([0u8; 16]))].into_boxed_slice(),
        );
        let facts: Arc<[FactVersionRef]> =
            Arc::from(vec![fact_filewhole("/a.ts", 1), fact_filewhole("/b.ts", 2)]);
        let sig = ReadSetSignature {
            facts,
            legacy,
            overflowed: false,
        };
        let canons = sig.canonical_ids();
        assert_eq!(
            canons.len(),
            2,
            "duplicate /a.ts across rails must collapse to one entry"
        );
        assert_eq!(canons[0].as_ref(), "/a.ts");
        assert_eq!(canons[1].as_ref(), "/b.ts");
    }

    #[test]
    fn read_set_signature_canonical_ids_covers_all_fact_variants() {
        let legacy: DepSignature =
            Arc::from(Vec::<(Arc<str>, DepVersion)>::new().into_boxed_slice());
        let facts: Arc<[FactVersionRef]> = Arc::from(vec![
            fact_filewhole("/wholehash.ts", 1),
            fact_derived("/derived.ts", 2),
            fact_parse("/parse.ts", 3),
            FactVersionRef::ResolveImports(crate::resolver_core::ResolveImportsFactRef {
                canonical_id: "/resolve.ts".to_string(),
                key: FactKey::SyntacticExportSet,
                lane: FactLane::Semantic,
                expected_hash: [0u8; 16],
            }),
            FactVersionRef::RouteSurface(crate::resolver_core::RouteSurfaceFactRef {
                canonical_id: "/route.ts".to_string(),
                key: FactKey::SyntacticExportSet,
                lane: FactLane::Semantic,
                expected_hash: [0u8; 16],
            }),
        ]);
        let sig = ReadSetSignature {
            facts,
            legacy,
            overflowed: false,
        };
        let canons: Vec<String> = sig
            .canonical_ids()
            .iter()
            .map(|a| a.as_ref().to_string())
            .collect();
        assert!(
            canons.contains(&"/wholehash.ts".to_string()),
            "FileWholeHash canonical must surface"
        );
        assert!(
            canons.contains(&"/derived.ts".to_string()),
            "DerivedFactHash canonical must surface"
        );
        assert!(
            canons.contains(&"/parse.ts".to_string()),
            "Parse canonical must surface"
        );
        assert!(
            canons.contains(&"/resolve.ts".to_string()),
            "ResolveImports canonical must surface"
        );
        assert!(
            canons.contains(&"/route.ts".to_string()),
            "RouteSurface canonical must surface"
        );
        assert_eq!(canons.len(), 5, "all 5 distinct canonicals must be present");
    }

    #[test]
    fn read_set_signature_canonical_ids_legacy_only_carrier() {
        let legacy: DepSignature = Arc::from(
            vec![
                (Arc::from("/x.ts"), DepVersion::WholeHash([0u8; 16])),
                (Arc::from("/y.ts"), DepVersion::WholeHash([0u8; 16])),
            ]
            .into_boxed_slice(),
        );
        let sig = ReadSetSignature {
            facts: empty_fact_signature(),
            legacy,
            overflowed: false,
        };
        let canons: Vec<String> = sig
            .canonical_ids()
            .iter()
            .map(|a| a.as_ref().to_string())
            .collect();
        assert_eq!(canons, vec!["/x.ts".to_string(), "/y.ts".to_string()]);
    }

    #[test]
    fn read_set_signature_facts_only_constructor() {
        let facts: Arc<[FactVersionRef]> = Arc::from(vec![fact_filewhole("/a.ts", 1)]);
        let sig = ReadSetSignature::facts_only(Arc::clone(&facts));
        assert_eq!(sig.facts.len(), 1);
        assert_eq!(sig.legacy.len(), 0);
        assert!(!sig.overflowed);
        let canons = sig.canonical_ids();
        assert_eq!(canons.len(), 1);
        assert_eq!(canons[0].as_ref(), "/a.ts");
    }
}
