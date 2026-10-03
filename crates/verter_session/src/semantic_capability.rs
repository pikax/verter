//! The TypeScript semantic capability closure — the semantic plane's half of
//! the ratified dual-plane mapper/snapshot/oracle identity contract.
//!
//! # What this closure owns, and what it does not
//!
//! This module closes the set of TypeScript semantic capabilities the semantic
//! plane will answer, and names the sole route a TypeScript engine's answer may
//! enter through: a [`CertifiedTypeEngineBinding`]. It owns no engine, issues
//! no query, and resolves no geometry — the mapping plane
//! ([`crate::content_mapper`]) answers geometry and never calls into this
//! plane, while this plane consumes mapping-plane products when activation
//! composes a position question. The dependency direction is one-way and
//! total, and neither side can grow a callback into the other: both planes
//! hold plain owned data (see the assertions at the bottom of each module).
//!
//! # Closure is derived, never self-certified
//!
//! The catalog below has no status field, no author, and no validator to
//! please. Its completeness is structural: the per-capability row mapping is
//! an exhaustive `match` over [`QueryFeature`], so a capability added to the
//! engine plane without a closure row fails to compile rather than pass a
//! review; its canonical order (strictly ascending query-kind domain tags) is
//! checked at compile time by a `const` evaluation, not at runtime by a
//! tracked script. A closure that could be declared closed by whoever holds
//! the pen — the self-certified closure status this module replaces — is not
//! representable here.
//!
//! # Certification is observation, not assumption
//!
//! [`CertifiedTypeEngineBinding::certify`] mints the witness only over a
//! [`BoundProject`] whose negotiated capabilities (the
//! `external_ts::EngineCapabilities` it carries) record a version. The
//! version's PROVENANCE is part of what composes the profile: a handshake the
//! engine reported in-band and a segment this publisher declared locally
//! (`external_ts::EngineVersion::{Reported, Declared}`) encode under different
//! discriminants, so a local segment can never pass as an observation of the
//! peer, and a record carrying neither — the `Default` construction — is
//! refused outright, because a profile composed over an assumed interpretation
//! is the self-certification above wearing a different coat.
//!
//! # Identity discipline
//!
//! The question is [`QueryIdentity`] — semantic arguments, the observed
//! engine profile, the capability's result contract — and is snapshot-
//! independent, so it is the only cross-snapshot cache-candidate key. The
//! flight is [`SemanticFlightKey`] = (`QueryIdentity`, `InputBasisId`): the
//! basis the answer will be attributed to, never a cache key. A result whose
//! basis is superseded before publication is REFUSED at the seam
//! ([`CertifiedTypeEngineBinding::publish_admitted`]) rather than returned to
//! its caller and dropped, and a result whose SERVING SESSION has rotated past
//! the one it was certified under is refused the same way
//! ([`CertifiedTypeEngineBinding::serving_admitted`]); degraded outcomes never
//! warm.
//!
//! # What is still dormant
//!
//! This module is live: the carrier publish path certifies a
//! [`CertifiedTypeEngineBinding`] before any snapshot reaches a store. The
//! MAPPING half ([`crate::content_mapper`]) is the dormant one — no production
//! route reaches it, and its activation is a separate, atomic step that
//! replaces the route it displaces rather than answering beside it.

use std::sync::Arc;

use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{
    InputBasisId, ProviderContractId, QueryIdentity, ResultContractId, SemanticFlightKey,
};

use crate::external_ts::{BoundProject, EngineIdentity, PublishSnapshot, QueryFeature, ServeMode};

/// One closed capability row: the capability, and the canonical query-kind
/// domain tag that composes its [`QueryIdentity`]. The row IS the closure
/// entry — there is no status, author, or date on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticCapabilityRow {
    /// The engine-plane capability this row closes.
    feature: QueryFeature,
    /// The domain tag composing the query-kind component of the row's
    /// [`QueryIdentity`]. Unique across the catalog; the catalog's canonical
    /// order is the strictly ascending order of these tags.
    ///
    /// Private, with no constructor outside this module: identity composition
    /// resolves a [`QueryFeature`] through [`capability_row`], so a caller
    /// cannot copy a canonical row, retag it, and mint a second certified
    /// query/result contract for the same capability. The catalog's
    /// exhaustiveness and canonical order are compile-time properties, not
    /// runtime validations a forged row could bypass.
    query_kind_domain_tag: &'static str,
}

impl SemanticCapabilityRow {
    /// The engine-plane capability this row closes.
    #[must_use]
    pub fn feature(&self) -> QueryFeature {
        self.feature
    }

    /// The domain tag composing the query-kind component of the row's
    /// [`QueryIdentity`].
    #[must_use]
    pub fn query_kind_domain_tag(&self) -> &'static str {
        self.query_kind_domain_tag
    }
}

const COMPLETION: SemanticCapabilityRow = SemanticCapabilityRow {
    feature: QueryFeature::Completion,
    query_kind_domain_tag: "verter.session.semantic_capability.query.completion.v1",
};
const DEFINITION: SemanticCapabilityRow = SemanticCapabilityRow {
    feature: QueryFeature::Definition,
    query_kind_domain_tag: "verter.session.semantic_capability.query.definition.v1",
};
const DOCUMENT_HIGHLIGHTS: SemanticCapabilityRow = SemanticCapabilityRow {
    feature: QueryFeature::DocumentHighlights,
    query_kind_domain_tag: "verter.session.semantic_capability.query.document_highlights.v1",
};
const HOVER: SemanticCapabilityRow = SemanticCapabilityRow {
    feature: QueryFeature::Hover,
    query_kind_domain_tag: "verter.session.semantic_capability.query.hover.v1",
};
const INLAY_HINTS: SemanticCapabilityRow = SemanticCapabilityRow {
    feature: QueryFeature::InlayHints,
    query_kind_domain_tag: "verter.session.semantic_capability.query.inlay_hints.v1",
};
const REFERENCES: SemanticCapabilityRow = SemanticCapabilityRow {
    feature: QueryFeature::References,
    query_kind_domain_tag: "verter.session.semantic_capability.query.references.v1",
};
const RENAME: SemanticCapabilityRow = SemanticCapabilityRow {
    feature: QueryFeature::Rename,
    query_kind_domain_tag: "verter.session.semantic_capability.query.rename.v1",
};
const SEMANTIC_TOKENS: SemanticCapabilityRow = SemanticCapabilityRow {
    feature: QueryFeature::SemanticTokens,
    query_kind_domain_tag: "verter.session.semantic_capability.query.semantic_tokens.v1",
};
const SIGNATURE_HELP: SemanticCapabilityRow = SemanticCapabilityRow {
    feature: QueryFeature::SignatureHelp,
    query_kind_domain_tag: "verter.session.semantic_capability.query.signature_help.v1",
};
const TYPE_DEFINITION: SemanticCapabilityRow = SemanticCapabilityRow {
    feature: QueryFeature::TypeDefinition,
    query_kind_domain_tag: "verter.session.semantic_capability.query.type_definition.v1",
};

/// The closed TypeScript semantic capability catalog, in canonical order:
/// strictly ascending query-kind domain tags. Two reads — or two builds of
/// this crate — cannot disagree about the order the closure enumerates.
pub const SEMANTIC_CAPABILITY_CATALOG: &[SemanticCapabilityRow] = &[
    COMPLETION,
    DEFINITION,
    DOCUMENT_HIGHLIGHTS,
    HOVER,
    INLAY_HINTS,
    REFERENCES,
    RENAME,
    SEMANTIC_TOKENS,
    SIGNATURE_HELP,
    TYPE_DEFINITION,
];

/// Lexicographic byte order, const-evaluable (no `str` `Ord` in const fn).
const fn bytes_lt(a: &[u8], b: &[u8]) -> bool {
    let len = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0;
    while i < len {
        if a[i] != b[i] {
            return a[i] < b[i];
        }
        i += 1;
    }
    // Equal up to the shared prefix: shorter is smaller; equal is NOT less.
    a.len() < b.len()
}

const fn tags_strictly_ascending(catalog: &[SemanticCapabilityRow]) -> bool {
    let mut i = 1;
    while i < catalog.len() {
        if !bytes_lt(
            catalog[i - 1].query_kind_domain_tag.as_bytes(),
            catalog[i].query_kind_domain_tag.as_bytes(),
        ) {
            return false;
        }
        i += 1;
    }
    true
}

// The canonical-order proof runs at compile time. A row whose tag breaks the
// ascending order — including a duplicate tag, which is not strictly
// ascending — fails the build, not a validator someone could appease.
const _: () = assert!(
    tags_strictly_ascending(SEMANTIC_CAPABILITY_CATALOG),
    "semantic capability catalog tags must be unique and strictly ascending"
);

/// The sole row for a capability. Exhaustive over [`QueryFeature`], so the
/// compiler — not a review — enforces that every engine-plane capability has
/// exactly one closure row.
#[must_use]
pub fn capability_row(feature: QueryFeature) -> &'static SemanticCapabilityRow {
    match feature {
        QueryFeature::Completion => &COMPLETION,
        QueryFeature::Definition => &DEFINITION,
        QueryFeature::TypeDefinition => &TYPE_DEFINITION,
        QueryFeature::References => &REFERENCES,
        QueryFeature::Rename => &RENAME,
        QueryFeature::Hover => &HOVER,
        QueryFeature::SignatureHelp => &SIGNATURE_HELP,
        QueryFeature::DocumentHighlights => &DOCUMENT_HIGHLIGHTS,
        QueryFeature::SemanticTokens => &SEMANTIC_TOKENS,
        QueryFeature::InlayHints => &INLAY_HINTS,
    }
}

/// The canonical discriminant a row's result contract hashes through. The
/// exhaustive match doubles as the completeness proof: a new capability
/// variant cannot be encoded without first being given a discriminant here.
const fn feature_discriminant(feature: QueryFeature) -> u32 {
    match feature {
        QueryFeature::Completion => 1,
        QueryFeature::Definition => 2,
        QueryFeature::TypeDefinition => 3,
        QueryFeature::References => 4,
        QueryFeature::Rename => 5,
        QueryFeature::Hover => 6,
        QueryFeature::SignatureHelp => 7,
        QueryFeature::DocumentHighlights => 8,
        QueryFeature::SemanticTokens => 9,
        QueryFeature::InlayHints => 10,
    }
}

/// Descriptor the capability's [`ResultContractId`] hashes through: the row's
/// own canonical bytes. Derived, never authored — the same row composes the
/// same contract on every call, and two rows never share one.
struct CapabilityResultContract<'a> {
    row: &'a SemanticCapabilityRow,
}

impl CanonicalEncode for CapabilityResultContract<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.semantic_capability.result_contract.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, self.row.query_kind_domain_tag);
        encoder.field_enum_discriminant(2, feature_discriminant(self.row.feature));
    }
}

impl SemanticCapabilityRow {
    /// The result contract the capability answers under: observable
    /// semantics, exactness, and approximation terms are all named by the
    /// row's identity, so provenance survives the trip through the cache.
    #[must_use]
    pub fn result_contract(&self) -> ResultContractId {
        ResultContractId::from_canonical(&CapabilityResultContract { row: self })
    }
}

/// The serving-mode discriminant the observed profile hashes through.
/// Exhaustive over [`ServeMode`], so a new mode cannot silently alias an old
/// one: it fails to compile here first.
const fn serve_mode_discriminant(mode: ServeMode) -> u32 {
    match mode {
        ServeMode::Owned => 1,
        ServeMode::Shared => 2,
    }
}

/// Descriptor the observed engine profile hashes through. Every input is a
/// fact the backend negotiated or the serving session reported — never a
/// default someone assumed — with two named exceptions, both of which name
/// themselves in the encoding rather than passing as observations: the version's
/// provenance discriminant (a local publisher's own declared segment is
/// recorded as declared, and never aliases a reported handshake over the same
/// text), and `wire_pin`, the local publisher's carrier-store FORMAT pin (see
/// `carrier_publish_store::CARRIER_STORE_WIRE_PIN`). Each stays an honest
/// dimension because it records WHICH wire contract this publisher writes, so
/// a differently pinned publisher can never compose the same profile; neither
/// is evidence of what the peer currently speaks. The serving identity (mode,
/// wire pin, session generation) is a first-class dimension: an OWNED and a
/// SHARED identity over the same project and version never compose the same
/// profile, so one engine's facts cannot launder into the other's question
/// identities. The bound project and its env dimensions pin whose facts these
/// are.
struct ObservedEngineProfile<'a> {
    version_provenance: u32,
    version: &'a str,
    static_module_resolution_map: bool,
    async_cancellable_queries: bool,
    serve_mode: ServeMode,
    serving_version: &'a str,
    wire_pin: u64,
    editor_session_generation: u64,
    project: &'a str,
    parse_env_hash: [u8; 16],
    resolve_env_hash: [u8; 16],
    lib_env_hash: [u8; 16],
    project_identity: [u8; 16],
}

impl CanonicalEncode for ObservedEngineProfile<'_> {
    // v2: the version's provenance discriminant joined the profile, so a locally
    // DECLARED segment can never compose the identity a REPORTED handshake over
    // the same string would.
    const DOMAIN_TAG: &'static str =
        "verter.session.semantic_capability.observed_engine_profile.v2";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_enum_discriminant(1, self.version_provenance);
        encoder.field_str(2, self.version);
        encoder.field_bool(3, self.static_module_resolution_map);
        encoder.field_bool(4, self.async_cancellable_queries);
        encoder.field_enum_discriminant(5, serve_mode_discriminant(self.serve_mode));
        encoder.field_str(6, self.serving_version);
        encoder.field_u64(7, self.wire_pin);
        encoder.field_u64(8, self.editor_session_generation);
        encoder.field_str(9, self.project);
        encoder.field_bytes(10, &self.parse_env_hash);
        encoder.field_bytes(11, &self.resolve_env_hash);
        encoder.field_bytes(12, &self.lib_env_hash);
        encoder.field_bytes(13, &self.project_identity);
    }
}

/// Why certification refused. Closed: each variant is a distinct disposition,
/// and none is a degraded form of a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificationRefusal {
    /// The capability record carried no engine version at all — neither a
    /// reported handshake nor a declared local contract segment
    /// ([`crate::external_ts::EngineVersion::Undeclared`], the `Default`
    /// construction). A profile composed over an assumed interpretation would
    /// certify an observation that never happened.
    UnobservedEngineCapabilities,
}

/// The membership lease a certification was minted under: MEMBERSHIP-validity
/// granularity (which session's advertisement is still valid), a DISTINCT
/// identity space from [`crate::external_ts::EngineSessionFacts::editor_session_generation`]
/// (the attach/spawn generation). The certification seam compares it as its own
/// typed dimension at the store write — never stored in, or compared through,
/// the editor-session generation slot — so a lease and an editor generation
/// that happen to share an integer never merge into one identity dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServingLease(u64);

impl ServingLease {
    /// A lease from the ledger session value it was minted under.
    #[must_use]
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// The underlying lease value.
    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }
}

/// The sole route by which a TypeScript engine's answer enters the semantic
/// plane. A witness, not a handle: holding one means a project binding was
/// resolved (the [`BoundProject`] it was certified over), the engine's
/// capability interpretation was observed, the serving session was identified
/// (mode, attach/spawn session generation, wire pin, and the membership lease
/// as its own typed [`ServingLease`] dimension),
/// recorded in the profile that composes every [`QueryIdentity`] minted under
/// it, and the basis the answer will be attributed to is the one named at
/// certification.
///
/// It mints no engine, holds no callback, and reaches no backend: a caller
/// cannot ask it a question, only compose the identities a certified flight
/// will be keyed by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertifiedTypeEngineBinding {
    project: Arc<str>,
    observed_profile: ProviderContractId,
    input_basis: InputBasisId,
    /// The serving session as it was OBSERVED at certification. Kept as the
    /// plain facts the backend recomposes per publish, so the store write can
    /// refuse a session that has since rotated rather than admit an answer
    /// under a session the certification never saw.
    serving: EngineIdentity,
    /// The membership lease the certification was minted under, as its own
    /// typed dimension (see [`ServingLease`]). The editor-session generation
    /// inside `serving` is the attach/spawn generation and is NOT this lease;
    /// the store write compares both dimensions separately.
    serving_lease: ServingLease,
}

impl CertifiedTypeEngineBinding {
    /// Certify the binding: project binding resolved, capability
    /// interpretation observed, serving session identified (engine identity
    /// plus the membership lease it was minted under), basis named. The
    /// only constructor — the fields are private and no struct-literal path
    /// exists outside this module, so a binding cannot be fabricated from
    /// parts.
    pub fn certify(
        bound: &BoundProject,
        serving: &EngineIdentity,
        serving_lease: ServingLease,
        input_basis: InputBasisId,
    ) -> Result<Self, CertificationRefusal> {
        let capabilities = bound.capabilities();
        let Some(version) = capabilities.version.as_str() else {
            return Err(CertificationRefusal::UnobservedEngineCapabilities);
        };
        let env_dims = bound.env_dims();
        Ok(Self {
            project: bound.project_arc(),
            observed_profile: ProviderContractId::from_canonical(&ObservedEngineProfile {
                version_provenance: capabilities.version.provenance_discriminant(),
                version,
                static_module_resolution_map: capabilities.static_module_resolution_map,
                async_cancellable_queries: capabilities.async_cancellable_queries,
                serve_mode: serving.mode,
                serving_version: &serving.observed_version,
                wire_pin: serving.wire_pin,
                editor_session_generation: serving.editor_session_generation,
                project: bound.project(),
                parse_env_hash: env_dims.parse_env_hash,
                resolve_env_hash: env_dims.resolve_env_hash,
                lib_env_hash: env_dims.lib_env_hash,
                project_identity: env_dims.project_identity.0,
            }),
            input_basis,
            serving: serving.clone(),
            serving_lease,
        })
    }

    /// The configured project the certification resolved against.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// The observed capability interpretation this binding records — the one
    /// profile that composes every query identity minted under it. Two
    /// bindings whose observations differ compose different identities for
    /// otherwise-identical questions.
    #[must_use]
    pub fn observed_profile(&self) -> &ProviderContractId {
        &self.observed_profile
    }

    /// The basis answers will be attributed to. Supersession after
    /// certification is refused AT this witness, not left to the flight
    /// runtime downstream: [`Self::publish_admitted`] admits only a snapshot
    /// carrying this exact basis, and the engine seam refuses before the store
    /// write, so a superseded result is never produced to be returned, let
    /// alone warmed.
    #[must_use]
    pub fn input_basis(&self) -> &InputBasisId {
        &self.input_basis
    }

    /// The serving session this binding was certified over, as it was observed
    /// then — the same facts the backend recomposes on every publish. The
    /// membership lease is NOT in here: it is the separate typed dimension
    /// [`Self::serving_lease`] names.
    #[must_use]
    pub fn serving_identity(&self) -> &EngineIdentity {
        &self.serving
    }

    /// The membership lease this binding was certified under, as its own typed
    /// dimension ([`ServingLease`]) — never the editor-session generation the
    /// serving identity carries.
    #[must_use]
    pub fn serving_lease(&self) -> ServingLease {
        self.serving_lease
    }

    /// Whether `serving` is still the session and `lease` still the membership
    /// lease this binding was certified over: same mode, same observed version,
    /// same wire pin, same attach/spawn generation, same lease.
    ///
    /// The half of the publication rule the BASIS cannot express. A snapshot
    /// basis covers the published bytes, so it says nothing about the serving
    /// session the answer is produced under; a retained binding minted under
    /// lease L1 would otherwise still admit an unchanged snapshot after the
    /// ledger rotates to L2, composing an answer under a membership session the
    /// certification never saw. The lease is compared as its OWN type — an
    /// editor-session generation that numerically equals a lease value never
    /// substitutes for it. The engine seam calls this where the store write
    /// happens, so the rotation is refused BEFORE the warm.
    #[must_use]
    pub fn serving_admitted(&self, serving: &EngineIdentity, lease: ServingLease) -> bool {
        &self.serving == serving && self.serving_lease == lease
    }

    /// Compose the snapshot-independent question identity for one capability:
    /// the capability's canonical query kind, the semantic arguments, this
    /// binding's observed profile, and the capability's result contract. The
    /// row is resolved through [`capability_row`] — the sole canonical row —
    /// never accepted from the caller, so a forged or retagged row cannot
    /// enter a certified identity. No basis enters it, which is exactly what
    /// makes it a cross-snapshot cache-candidate key.
    #[must_use]
    pub fn query_identity<Q>(
        &self,
        feature: QueryFeature,
        semantic_arguments: CanonicalDigest,
    ) -> QueryIdentity<Q> {
        let row = capability_row(feature);
        QueryIdentity::compose(
            row.query_kind_domain_tag,
            semantic_arguments,
            &[self.observed_profile.digest()],
            &row.result_contract(),
        )
    }

    /// Compose the in-flight production key: the question identity plus the
    /// certified basis. Strictly bigger than the question identity, so the
    /// two cannot coerce in either direction.
    #[must_use]
    pub fn flight_key<Q>(
        &self,
        feature: QueryFeature,
        semantic_arguments: CanonicalDigest,
    ) -> SemanticFlightKey<Q> {
        SemanticFlightKey {
            query_identity: self.query_identity(feature, semantic_arguments),
            input_basis: self.input_basis.clone(),
        }
    }

    /// The publication rule this plane enforces at the engine seam: a binding
    /// admits exactly ONE published snapshot — the one whose
    /// [`PublishSnapshot::input_basis`] is the basis the binding was certified
    /// over, recomputed here rather than read back from the binding's own field
    /// — and only when that snapshot carries at most one row per
    /// `provider_uri` ([`PublishSnapshot::duplicate_provider_uri`]).
    ///
    /// So a caller cannot certify over one snapshot and publish another, a
    /// snapshot whose basis was superseded between certification and the store
    /// write is REFUSED before it can warm, and a snapshot whose manifest bytes
    /// the input order would decide (two rows for one `provider_uri`) is
    /// refused wholesale — one basis must name one publication. The disposition
    /// is a refusal, not a degraded publish: there is no result from a
    /// superseded basis to return, let alone to warm.
    #[must_use]
    pub fn publish_admitted(&self, snapshot: &PublishSnapshot) -> bool {
        snapshot.duplicate_provider_uri().is_none() && snapshot.input_basis() == self.input_basis
    }
}

// The witness is plain owned data. A retained callback, trait object, or
// borrowed handle into an engine or a semantic session would turn this
// witness into the handle it refuses to be — and would be the mapper-callback
// route into the semantic oracle this plane rejects in any shape.
const _: fn() = || {
    fn assert_plain_owned_witness<T: 'static + Send + Sync + Clone + Eq + std::fmt::Debug>() {}
    assert_plain_owned_witness::<CertifiedTypeEngineBinding>();
    assert_plain_owned_witness::<SemanticCapabilityRow>();
    assert_plain_owned_witness::<CertificationRefusal>();
};
