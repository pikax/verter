//! `BinderIdentityFacts` — the family-A binder-identity artifact:
//! demand-produced, parse-stable, content-addressed, and validated by
//! [`ReadSetSignature`] over its inputs.
//!
//! This is the binder-identity substrate the reducers read BEFORE they
//! run: the per-file lexical scope tree with stable structural scope ids
//! ([`BinderScopeId`]), the env-free declaration-slot seeds
//! ([`DeclarationSlotSeed`]), and the per-file declaration-order /
//! overload-group / augmentation-contribution provenance records. It is
//! a typed PROJECTION over [`IndexedReady`]'s shallow declaration-header
//! inventory — every input is recorded at shallow-analysis time
//! (contributor anchors, augmentation tables); the producer lowers ZERO
//! declaration bodies and never re-walks an AST.
//!
//! ## Keying (R21 split-don't-bundle, family A)
//!
//! Entries are keyed `(canonical, parse_stable_hash, parse_env_hash)` —
//! the [`MemberSemanticFactStore`](crate::member_semantic_fact_store)
//! content-addressed parse-stable convention, NOT the
//! `FileArtifactStore` whole/content-hash convention. A cosmetic edit
//! (whitespace / comments / JSDoc) leaves `parse_stable_hash`
//! unchanged, so the entry warms in place; a decl-shape edit moves the
//! hash and the entry is unreachable. R6 does NOT govern artifact keys
//! — an artifact key legitimately carries `parse_stable_hash`.
//!
//! ## Validation
//!
//! Each entry carries a [`ReadSetSignature`] recording the parse-lane
//! facts the producer pinned against the OBSERVED content version
//! (`SyntacticExportSet`, per-declaration `MemberShape`, per-augmentation
//! `ModuleAugmentation` — all eager header facts; body-sensitive facts
//! are never forced). Warm reads validate the signature against the
//! live view (`validate_with_self_roots` with the keyed canonical as
//! the self-root set) exactly like the neighboring fact stores.
//!
//! ## Demand-driven, no eager pass
//!
//! Entries compute on demand through
//! [`produce_binder_identity_facts`] — the same lazy/validated rails
//! the existing artifact stores use (warm peek → tracer-scoped cold
//! compute → admit). There is NO eager whole-program binder pass.
//!
//! ## Negative lookups stay `ReturnOnly`
//!
//! A name absent from this artifact is a per-file negative scoped to
//! the file's OWN inventory. It is NOT corpus-backed — an ambient /
//! global / lib contributor from another file is invisible to this
//! artifact — so a negative binder answer can never warm as a
//! falsely-authoritative miss: [`negative_lookup_admission`] is
//! [`Admission::ReturnOnly`] (the corpus-completeness family-B store is
//! a separate, later substrate).

use std::sync::Arc;

use dashmap::DashMap;
use verter_semantic::analysis::type_eval::{AugmentationScopeKind, ValueDeclKind};
use verter_semantic::analysis::Hash16;
#[cfg(any(test, feature = "test-support"))]
use verter_semantic::facts::SymbolSpace as FactSymbolSpace;
#[cfg(any(test, feature = "test-support"))]
use verter_semantic::facts::{FactKey, FactLane};

use crate::fact_signature_helpers::ReadSetSignature;
#[cfg(any(test, feature = "test-support"))]
use crate::fact_signature_helpers::ReadSetSignatureExt as _;
use crate::project_type_store::IndexedReady;
use crate::semantic_query::{BinderScopeId, DeclarationSlotSeed, SemanticSymbolSpace};

// ===========================================================================
// Artifact payload (scope tree + declaration-slot seeds + provenance)
// ===========================================================================

/// One node of the per-file lexical scope tree: a scope's stable
/// structural id plus its kind payload and parent link. The tree covers
/// exactly the scopes the shallow inventory records: one file top-level
/// scope per owner, namespace body scopes (qualified dotted names), and
/// augmentation block scopes (`declare global` / `declare module "X"`),
/// each per contributing owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinderScopeRecord {
    /// The scope's stable structural id.
    pub id: BinderScopeId,
    /// The scope kind this id names (its content-derived header). Lives
    /// on the RECORD, not on the id, so the query-identity id stays a
    /// lean 16-byte discriminator.
    pub kind: crate::semantic_query::BinderScopeKind,
    /// The enclosing scope's id. `None` only on a file top-level scope.
    pub parent: Option<BinderScopeId>,
}

/// A declaration's source-order contributor provenance — the authored
/// statement indices of every top-level statement contributing to one
/// declaration slot, in authored order (TS declaration merging /
/// overloads / `var` redeclaration all append in source order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclOrderRecord {
    /// The declaration slot this order record belongs to.
    pub seed: DeclarationSlotSeed,
    /// Authored source order: the contributing top-level statement
    /// indices, ascending by authored position.
    pub contributor_order: Arc<[u32]>,
}

/// An overload-group provenance record — a value-space declaration with
/// more than one contributing function declaration (`function f(x:
/// string): void; function f(x: number): void; …`). Membership and the
/// authored contributor order, as recorded at shallow-analysis time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverloadGroupRecord {
    /// The value-space declaration slot the group binds to.
    pub seed: DeclarationSlotSeed,
    /// The overload member contributors (statement indices) in authored
    /// order — the order a call-resolution consumer reads signatures.
    pub member_order: Arc<[u32]>,
}

/// One contribution to a per-file module / global / ambient
/// augmentation scope, with its authored contribution order WITHIN that
/// scope (across both symbol spaces — the order TS applies
/// augmentations).
///
/// **Order is the fact.** Raw positions (statement indices, span
/// starts) are INTERNAL sort keys only — they are never published here,
/// because they move on cosmetic inserts while the signature's order
/// rail deliberately ignores them (trivia-invariance): a served
/// position would drift warm-vs-cold exactly the way the cosmetic-warm
/// contract forbids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AugmentationContributionRecord {
    /// Which augmentation scope receives the contribution.
    pub scope_kind: AugmentationScopeKind,
    /// The owner region authoring the contribution.
    pub owner: verter_type_expr::TopLevelOwnerId,
    /// The contributed declaration's name (verbatim from the shallow
    /// augmentation tables; namespace members are dotted).
    pub name: Arc<str>,
    /// The symbol space the contribution occupies.
    pub symbol_space: SemanticSymbolSpace,
    /// The authored order of this contribution within `scope_kind`
    /// (0-based, by each declaration's own authored source position —
    /// two declarations in ONE block keep their authored order).
    pub contribution_order: u32,
}

/// The family-A binder-identity artifact for one file: the lexical
/// scope tree, the declaration-slot seeds, and the provenance records.
/// Content-free and env-free: no content/version hash, no env
/// dimension, no fact-signature field lives inside the payload —
/// keying and validation ride on the store entry
/// ([`BinderIdentityFactsEntry`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinderIdentityFacts {
    /// The canonical file this artifact projects.
    pub canonical: Arc<str>,
    /// The per-file lexical scope tree (parent-linked), sorted by
    /// structural hash for determinism.
    pub scopes: Arc<[BinderScopeRecord]>,
    /// The env-free declaration-slot seeds — one per file-surface
    /// declaration `(owner, name, space)`, sorted for determinism.
    /// Augmentation-scope declarations are NOT slots of this file's own
    /// surface; they are recorded as provenance
    /// (`augmentation_contributions`) only.
    pub decl_slots: Arc<[DeclarationSlotSeed]>,
    /// Per-declaration source-order contributor records.
    pub declaration_order: Arc<[DeclOrderRecord]>,
    /// Overload-group membership records.
    pub overload_groups: Arc<[OverloadGroupRecord]>,
    /// Per-file augmentation contribution records, sorted by
    /// `(scope, contribution_order)` — the authored order.
    pub augmentation_contributions: Arc<[AugmentationContributionRecord]>,
}

impl BinderIdentityFacts {
    /// The file top-level scope id for `owner`, when the file's shallow
    /// inventory declares anything under that owner.
    #[must_use]
    pub fn file_scope_id(&self, owner: verter_type_expr::TopLevelOwnerId) -> Option<BinderScopeId> {
        let want = BinderScopeId::file_scope(owner);
        self.scopes
            .iter()
            .find(|record| record.id == want)
            .map(|record| record.id)
    }

    /// The namespace body scope id for the dotted `qualified_name`
    /// declared in `owner`, when present.
    #[must_use]
    pub fn namespace_scope_id(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        qualified_name: &str,
    ) -> Option<BinderScopeId> {
        let want = BinderScopeId::namespace_scope(owner, Arc::from(qualified_name));
        self.scopes
            .iter()
            .find(|record| record.id == want)
            .map(|record| record.id)
    }

    /// Look up the env-free declaration-slot seed for
    /// `(owner, name, space)` in the file's OWN surface.
    ///
    /// A `None` answer is a per-file NEGATIVE scoped to this file's own
    /// inventory: it says nothing about ambient / global / lib
    /// contributors from other files (no corpus-completeness store
    /// exists in this block), so a negative binder answer routes
    /// [`negative_lookup_admission`] — `ReturnOnly`, never a
    /// warm-cached miss.
    #[must_use]
    pub fn decl_slot_seed(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        symbol_space: SemanticSymbolSpace,
    ) -> Option<&DeclarationSlotSeed> {
        self.decl_slots.iter().find(|seed| {
            seed.owner == owner
                && seed.merged_symbol_name.as_ref() == name
                && seed.symbol_space == symbol_space
        })
    }

    /// The overload-group membership record for the value-space
    /// function `(owner, name)`, when that declaration is an overload
    /// group.
    #[must_use]
    pub fn overload_group(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<&OverloadGroupRecord> {
        self.overload_groups.iter().find(|group| {
            group.seed.owner == owner && group.seed.merged_symbol_name.as_ref() == name
        })
    }

    /// The augmentation contributions to `scope_kind` in authored
    /// order (the per-file module / global / ambient contribution
    /// order).
    pub fn augmentation_contributions_in_order<'a>(
        &'a self,
        scope_kind: &'a AugmentationScopeKind,
    ) -> impl Iterator<Item = &'a AugmentationContributionRecord> + 'a {
        self.augmentation_contributions
            .iter()
            .filter(move |record| &record.scope_kind == scope_kind)
    }
}

/// The admission disposition for a NEGATIVE binder name lookup. A
/// negative (name-not-found) binder answer is NOT backed by a recorded
/// corpus completeness fact in this block, so it must never warm a
/// cache as a falsely-authoritative miss.
#[must_use]
pub fn negative_lookup_admission() -> crate::semantic_query::admit::Admission {
    crate::semantic_query::admit::Admission::ReturnOnly
}

// ===========================================================================
// Store — keyed `(canonical, parse_stable_hash, parse_env_hash)`
// ===========================================================================

/// Key for the family-A artifact store: the content-addressed
/// parse-stable convention (the `MemberSemanticFactStore` shape), NOT
/// the `FileArtifactStore` whole/content-hash convention. Cosmetic
/// edits keep the key; decl-shape edits move `parse_stable_hash`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BinderIdentityFactsKey {
    pub canonical: Arc<str>,
    pub parse_stable_hash: Hash16,
    pub parse_env_hash: Hash16,
}

/// A stored artifact plus its validity carrier. The
/// [`ReadSetSignature`] records the parse-lane facts the producer
/// pinned against the observed content version; warm reads validate it
/// against the live view before serving.
#[derive(Debug, Clone)]
pub struct BinderIdentityFactsEntry {
    pub facts: Arc<BinderIdentityFacts>,
    /// Path-precise fact carrier — the sole cache-validity oracle
    /// (validated via `validate_with_self_roots` with the keyed
    /// canonical as the self-root set).
    pub read_set_signature: ReadSetSignature,
}

/// The family-A `BinderIdentityFacts` artifact store.
///
/// **Lookup contract.** A cold miss returns `None`; the caller (the
/// producer) computes the artifact and admits it via
/// [`BinderIdentityFactsStore::insert`]. A warm hit returns the stored
/// entry; the PRODUCER validates `read_set_signature` against the live
/// view before serving it (same split as the neighboring stores: the
/// store holds entries, the producer owns the validity rail).
///
/// **Concurrency.** `DashMap` shards on the key; same-key admissions
/// reduce to first-admitted-wins (identical keys are deterministic
/// recomputations).
#[derive(Debug, Default)]
pub struct BinderIdentityFactsStore {
    entries: DashMap<BinderIdentityFactsKey, Arc<BinderIdentityFactsEntry>>,
}

impl BinderIdentityFactsStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Lookup by full key. `None` is a cold miss — the caller computes
    /// the artifact and admits it via [`Self::insert`]. The returned
    /// entry's `read_set_signature` MUST validate against the live view
    /// before the producer serves it.
    #[must_use]
    pub fn get(&self, key: &BinderIdentityFactsKey) -> Option<Arc<BinderIdentityFactsEntry>> {
        self.entries.get(key).map(|v| Arc::clone(&*v))
    }

    /// Admit a freshly-computed entry. Insert-only-if-absent: an
    /// identical key is a deterministic recomputation, so the
    /// first-admitted entry is preserved (`Arc` identity for shared
    /// consumers).
    pub fn insert(&self, key: BinderIdentityFactsKey, entry: Arc<BinderIdentityFactsEntry>) {
        self.entries.entry(key).or_insert(entry);
    }

    /// Number of cached entries. Used by tests + diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no entries are cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop the entry at `key` (if any). Used by the producer when a
    /// warm read fails `ReadSetSignature` validation under an unchanged
    /// key — the stale entry must not keep winning future warm reads
    /// over the freshly recomputed one.
    pub fn remove(&self, key: &BinderIdentityFactsKey) {
        self.entries.remove(key);
    }

    /// Drop every cached entry. Used by GC sweeps and test setup.
    pub fn clear(&self) {
        self.entries.clear();
    }

    /// Drop every cached entry whose `canonical` matches the supplied
    /// canonical id. Used by the project-global eviction cascade when a
    /// canonical file's content changes.
    pub fn invalidate_canonical(&self, canonical: &str) {
        self.entries
            .retain(|key, _| key.canonical.as_ref() != canonical);
    }
}

// ===========================================================================
// Projection — shallow inventory → artifact
// ===========================================================================

/// Project the [`BinderIdentityFacts`] artifact from a file's
/// [`IndexedReady`] shallow inventory. Pure projection: lowers ZERO
/// declaration bodies, never re-walks an AST, deterministic (every
/// output vector sorted by stable identity keys).
#[must_use]
pub fn project_binder_identity_facts(
    canonical: &str,
    indexed: &IndexedReady,
) -> BinderIdentityFacts {
    let headers = indexed.shallow_state.decl_bodies().header_index();
    let canonical: Arc<str> = Arc::from(canonical);

    let mut scopes: Vec<BinderScopeRecord> = Vec::new();
    let mut decl_slots: Vec<DeclarationSlotSeed> = Vec::new();
    let mut declaration_order: Vec<DeclOrderRecord> = Vec::new();
    let mut overload_groups: Vec<OverloadGroupRecord> = Vec::new();

    // ── File + namespace scopes; type/value seeds; decl order; overloads ──
    let mut file_scope_owners: Vec<verter_type_expr::TopLevelOwnerId> = Vec::new();

    let note_owner =
        |owner: verter_type_expr::TopLevelOwnerId,
         file_scope_owners: &mut Vec<verter_type_expr::TopLevelOwnerId>| {
            if !file_scope_owners.contains(&owner) {
                file_scope_owners.push(owner);
            }
        };

    for (key, header) in headers.type_headers.iter() {
        note_owner(key.owner, &mut file_scope_owners);
        let seed = DeclarationSlotSeed::new(
            Arc::clone(&canonical),
            key.owner,
            Arc::clone(&key.name),
            SemanticSymbolSpace::Type,
        );
        declaration_order.push(DeclOrderRecord {
            seed: seed.clone(),
            contributor_order: header
                .contributors
                .iter()
                .map(|c| c.anchor.contributor_index)
                .collect(),
        });
        decl_slots.push(seed);
    }
    for (key, header) in headers.value_headers.iter() {
        note_owner(key.owner, &mut file_scope_owners);
        let seed = DeclarationSlotSeed::new(
            Arc::clone(&canonical),
            key.owner,
            Arc::clone(&key.name),
            SemanticSymbolSpace::Value,
        );
        if matches!(
            header.kind,
            ValueDeclKind::Function | ValueDeclKind::AsyncFunction
        ) && header.contributors.len() > 1
        {
            overload_groups.push(OverloadGroupRecord {
                seed: seed.clone(),
                member_order: header
                    .contributors
                    .iter()
                    .map(|c| c.anchor.contributor_index)
                    .collect(),
            });
        }
        declaration_order.push(DeclOrderRecord {
            seed: seed.clone(),
            contributor_order: header
                .contributors
                .iter()
                .map(|c| c.anchor.contributor_index)
                .collect(),
        });
        decl_slots.push(seed);
    }

    // ── Namespace scopes + namespace-space seeds — from the shallow
    //    walk's namespace-BLOCK inventory (recorded at block entry, so
    //    EMPTY blocks like `namespace Empty {}` are scopes too; a block
    //    with zero members is still a named lexical scope). ──
    let mut namespace_names: Vec<(verter_type_expr::TopLevelOwnerId, Arc<str>)> = headers
        .namespace_blocks
        .iter()
        .map(|block| (block.owner, Arc::from(block.qualified_name.as_str())))
        .collect();
    namespace_names.sort();
    namespace_names.dedup();
    for (owner, qualified_name) in &namespace_names {
        // Namespace-only files still have a lexical owner: the file
        // scope must exist for the namespace scope's parent link (a file
        // with ONLY `namespace Empty {}` otherwise dangles — empty
        // blocks produce no type/value headers).
        note_owner(*owner, &mut file_scope_owners);
        let id = BinderScopeId::namespace_scope(*owner, Arc::clone(qualified_name));
        let parent = match qualified_name.rsplit_once('.') {
            Some((head, _)) => BinderScopeId::namespace_scope(*owner, Arc::from(head)),
            None => BinderScopeId::file_scope(*owner),
        };
        scopes.push(BinderScopeRecord {
            id,
            kind: crate::semantic_query::BinderScopeKind::Namespace {
                qualified_name: Arc::clone(qualified_name),
            },
            parent: Some(parent),
        });
        // A `namespace N { … }` declaration introduces the name in
        // NAMESPACE space: emit the namespace-space seed for every
        // namespace block (each dotted prefix), alongside the Type /
        // Value seeds of its members. A namespace sharing a name with a
        // type or value occupies a DISTINCT seed (`symbol_space`
        // discriminates).
        decl_slots.push(DeclarationSlotSeed::new(
            Arc::clone(&canonical),
            *owner,
            Arc::clone(qualified_name),
            SemanticSymbolSpace::Namespace,
        ));
    }

    // ── Augmentation scopes (EMPTY blocks included) + contribution
    //    order ──
    let note_augmentation_scope =
        |scope_kind: &AugmentationScopeKind,
         owner: verter_type_expr::TopLevelOwnerId,
         scopes: &mut Vec<BinderScopeRecord>| {
            let (scope_id, scope_record_kind) = match scope_kind {
                AugmentationScopeKind::Global => (
                    BinderScopeId::augmentation_global_scope(owner),
                    crate::semantic_query::BinderScopeKind::AugmentationGlobal,
                ),
                AugmentationScopeKind::Module(specifier) => (
                    BinderScopeId::augmentation_module_scope(owner, Arc::from(specifier.as_str())),
                    crate::semantic_query::BinderScopeKind::AugmentationModule {
                        specifier: Arc::from(specifier.as_str()),
                    },
                ),
            };
            if !scopes.iter().any(|record| record.id == scope_id) {
                scopes.push(BinderScopeRecord {
                    id: scope_id,
                    kind: scope_record_kind,
                    parent: Some(BinderScopeId::file_scope(owner)),
                });
            }
        };
    // Every augmentation BLOCK introduces its scope — even an EMPTY
    // `declare module "m" {}` / `declare global {}` (the block itself
    // is the entry; contributions, when any, hang off it below).
    for block in &headers.augmentation_blocks {
        note_owner(block.owner, &mut file_scope_owners);
        note_augmentation_scope(&block.scope, block.owner, &mut scopes);
    }
    // Contributions carry their authored position ONLY as a
    // compute-local sort key (the second tuple element) — positions are
    // never published on the served records (they move on cosmetic
    // inserts while the signature's order rail ignores them; a served
    // position would drift warm-vs-cold).
    let mut contributions_with_positions: Vec<(AugmentationContributionRecord, u32)> = Vec::new();
    for (scope_kind, decls) in &headers.augmentation_type_headers {
        for (key, header) in decls.iter() {
            // Augmentation-only files still have a lexical owner:
            // the file scope must exist for the augmentation scope's
            // parent link (a file with ONLY `declare module` /
            // `declare global` blocks otherwise dangles).
            note_owner(key.owner, &mut file_scope_owners);
            note_augmentation_scope(scope_kind, key.owner, &mut scopes);
            for contributor in &header.contributors {
                contributions_with_positions.push((
                    AugmentationContributionRecord {
                        scope_kind: scope_kind.clone(),
                        owner: key.owner,
                        name: Arc::clone(&key.name),
                        symbol_space: SemanticSymbolSpace::Type,
                        contribution_order: 0, // assigned after the per-scope sort below
                    },
                    contributor.declaration_span.start,
                ));
            }
        }
    }
    for (scope_kind, decls) in &headers.augmentation_value_headers {
        for (key, header) in decls.iter() {
            note_owner(key.owner, &mut file_scope_owners);
            note_augmentation_scope(scope_kind, key.owner, &mut scopes);
            for contributor in &header.contributors {
                contributions_with_positions.push((
                    AugmentationContributionRecord {
                        scope_kind: scope_kind.clone(),
                        owner: key.owner,
                        name: Arc::clone(&key.name),
                        symbol_space: SemanticSymbolSpace::Value,
                        contribution_order: 0, // assigned after the per-scope sort below
                    },
                    contributor.declaration_span.start,
                ));
            }
        }
    }
    // Authored contribution order within each scope: ascending by each
    // declaration's OWN authored source position (two declarations in
    // ONE block keep their authored order — never an alphabetical
    // tie-break, never a per-symbol collapse — duplicate symbols keep
    // both entries at their authored positions).
    contributions_with_positions.sort_by(|(a, a_pos), (b, b_pos)| {
        (
            aug_scope_sort_key(&a.scope_kind),
            *a_pos,
            a.owner,
            a.name.clone(),
            seed_space_tag(a.symbol_space),
        )
            .cmp(&(
                aug_scope_sort_key(&b.scope_kind),
                *b_pos,
                b.owner,
                b.name.clone(),
                seed_space_tag(b.symbol_space),
            ))
    });
    let mut augmentation_contributions: Vec<AugmentationContributionRecord> =
        Vec::with_capacity(contributions_with_positions.len());
    {
        let mut next_order: std::collections::HashMap<(u8, String), u32> =
            std::collections::HashMap::new();
        for (mut record, _position) in contributions_with_positions {
            let counter = next_order
                .entry(aug_scope_sort_key(&record.scope_kind))
                .or_insert(0);
            record.contribution_order = *counter;
            *counter += 1;
            augmentation_contributions.push(record);
        }
    }

    // ── File scopes — emitted AFTER every owner source (ordinary
    //    headers, namespace blocks, AND augmentation tables) has been
    //    collected, so a namespace-only or augmentation-only file still
    //    gets its file top-level scope (F6: scope parents never
    //    dangle). ──
    file_scope_owners.sort();
    for owner in file_scope_owners {
        scopes.push(BinderScopeRecord {
            id: BinderScopeId::file_scope(owner),
            kind: crate::semantic_query::BinderScopeKind::File,
            parent: None,
        });
    }

    // ── Deterministic output order ──
    scopes.sort_by(|a, b| {
        a.id.structural_hash
            .cmp(&b.id.structural_hash)
            .then_with(|| scope_kind_sort_key(&a.kind).cmp(&scope_kind_sort_key(&b.kind)))
    });
    decl_slots.sort_by(|a, b| {
        (
            a.owner,
            a.merged_symbol_name.clone(),
            seed_space_tag(a.symbol_space),
        )
            .cmp(&(
                b.owner,
                b.merged_symbol_name.clone(),
                seed_space_tag(b.symbol_space),
            ))
    });
    declaration_order.sort_by(|a, b| {
        (
            a.seed.owner,
            a.seed.merged_symbol_name.clone(),
            seed_space_tag(a.seed.symbol_space),
        )
            .cmp(&(
                b.seed.owner,
                b.seed.merged_symbol_name.clone(),
                seed_space_tag(b.seed.symbol_space),
            ))
    });
    overload_groups.sort_by(|a, b| {
        (a.seed.owner, a.seed.merged_symbol_name.clone())
            .cmp(&(b.seed.owner, b.seed.merged_symbol_name.clone()))
    });

    BinderIdentityFacts {
        canonical,
        scopes: Arc::from(scopes.into_boxed_slice()),
        decl_slots: Arc::from(decl_slots.into_boxed_slice()),
        declaration_order: Arc::from(declaration_order.into_boxed_slice()),
        overload_groups: Arc::from(overload_groups.into_boxed_slice()),
        augmentation_contributions: Arc::from(augmentation_contributions.into_boxed_slice()),
    }
}

const fn seed_space_tag(space: SemanticSymbolSpace) -> u8 {
    match space {
        SemanticSymbolSpace::Type => 0,
        SemanticSymbolSpace::Value => 1,
        SemanticSymbolSpace::Namespace => 2,
    }
}

/// Deterministic sort key for an augmentation scope (Global before
/// Module, modules by specifier).
fn aug_scope_sort_key(scope_kind: &AugmentationScopeKind) -> (u8, String) {
    match scope_kind {
        AugmentationScopeKind::Global => (0, String::new()),
        AugmentationScopeKind::Module(specifier) => (1, specifier.clone()),
    }
}

/// Deterministic sort key for a binder scope kind.
fn scope_kind_sort_key(kind: &crate::semantic_query::BinderScopeKind) -> (u8, String) {
    use crate::semantic_query::BinderScopeKind;
    match kind {
        BinderScopeKind::File => (0, String::new()),
        BinderScopeKind::Namespace { qualified_name } => (1, qualified_name.to_string()),
        BinderScopeKind::AugmentationGlobal => (2, String::new()),
        BinderScopeKind::AugmentationModule { specifier } => (3, specifier.to_string()),
    }
}

/// Map the query-identity symbol space onto the fact-registry symbol
/// space (same closed three-variant set, distinct nominal types).
#[cfg(any(test, feature = "test-support"))]
const fn fact_space(space: SemanticSymbolSpace) -> FactSymbolSpace {
    match space {
        SemanticSymbolSpace::Type => FactSymbolSpace::Type,
        SemanticSymbolSpace::Value => FactSymbolSpace::Value,
        SemanticSymbolSpace::Namespace => FactSymbolSpace::Namespace,
    }
}

// ===========================================================================
// Producer — demand-produced through the lazy/validated rails
// ===========================================================================

/// Demand-produce the family-A [`BinderIdentityFacts`] entry for
/// `canonical`: warm-peek the store (validating the entry's
/// [`ReadSetSignature`] against the live view), else compute the
/// artifact from the served [`IndexedReady`] under a fact tracer and
/// admit it.
///
/// Returns `None` only when the canonical has no servable
/// [`IndexedReady`] at all. A fenced (non-published) serve or a
/// non-cacheable / overflowed read set still RETURNS the freshly
/// computed artifact but admits NOTHING (`ReturnOnly` — the standard
/// no-warm-for-unrootable rule).
///
/// Reached today only through tests and the `for_tests` wrapper (the
/// `U2` reducer consumption lands with the reducer blocks — no reducer
/// consumes the substrate yet); gated to match so the producer is
/// absent from every ordinary production build (the
/// `AppConfigNoOverrideProofDb` producer precedent).
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn produce_binder_identity_facts(
    ctx: &dyn crate::resolver_core::ResolverContext,
    canonical: &str,
) -> Option<Arc<BinderIdentityFactsEntry>> {
    let host = ctx.host_for_fact_tracer_install();
    let serve = ctx.ensure_indexed_ready_serve(canonical)?;
    let indexed = serve.indexed;
    let parse_stable_hash = crate::parse_stable_hash::compute_parse_stable_hash(&indexed);
    let key = BinderIdentityFactsKey {
        canonical: Arc::from(canonical),
        parse_stable_hash,
        parse_env_hash: indexed.parse_env_hash,
    };
    let store = host.project_type_store.binder_identity_facts_store();
    if let Some(entry) = store.get(&key) {
        if entry
            .read_set_signature
            .validate_with_self_roots(ctx, std::slice::from_ref(&key.canonical))
        {
            // A warm hit must BUBBLE the entry's read-set into any
            // active outer tracer: an enclosing traced computation
            // admits its own value with THESE binder facts observed, so
            // it is invalidated when a pinned fact moves (the sibling
            // `AppConfigNoOverrideProofDb::peek` pattern).
            crate::fact_signature_helpers::bubble_fact_signature(
                ctx,
                &entry.read_set_signature.facts,
            );
            return Some(entry);
        }
        // Stale under the same key (a recorded fact moved): drop it so
        // the fresh recompute below wins future warm reads, then fall
        // through to the cold recompute, which re-pins the signature.
        store.remove(&key);
    }

    let indexed_for_body = Arc::clone(&indexed);
    let canonical_for_body = key.canonical.clone();
    let cold_body = move || -> (BinderIdentityFacts, bool) {
        let facts = project_binder_identity_facts(canonical_for_body.as_ref(), &indexed_for_body);
        // Pin the eager parse-lane facts covering the artifact's
        // inputs against the OBSERVED content version. All are header
        // facts — no body-sensitive (`Export` / `LocalDecl` / `Member`)
        // fact is forced, so production lowers zero declaration bodies.
        let mut all_pinned = true;
        let mut pin = |fact_key: FactKey| {
            match crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content(
                ctx,
                canonical_for_body.as_ref(),
                indexed_for_body.whole_hash,
                fact_key,
                FactLane::Semantic,
            ) {
                Some(fact_ref) => {
                    ctx.observe(crate::resolver_core::FactVersionRef::Parse(fact_ref));
                }
                None => {
                    all_pinned = false;
                }
            }
        };
        pin(FactKey::SyntacticExportSet);
        // Whole-file scope-inventory set pins: a NEW augmentation target
        // (a first `declare module "m" {…}` in a file that had none) or
        // a new namespace block moves the signature even when the
        // parse-stable skeleton is unchanged (EMPTY blocks included).
        pin(FactKey::AugmentationTargetSet);
        pin(FactKey::NamespaceScopeSet);
        for seed in facts.decl_slots.iter() {
            pin(FactKey::MemberShape {
                exporter: crate::file_artifact_store::InternedName::from(
                    seed.merged_symbol_name.as_ref(),
                ),
                space: fact_space(seed.symbol_space),
            });
        }
        // Order-sensitive contributor-sequence pins for every
        // file-surface slot (an overload-group reorder or a same-file
        // declaration swap moves this fact; a comment BETWEEN
        // declarations does not, so the cosmetic warm rate survives).
        for record in facts.declaration_order.iter() {
            pin(FactKey::DeclContributionOrder {
                name: crate::file_artifact_store::InternedName::from(
                    record.seed.merged_symbol_name.as_ref(),
                ),
                owner: record.seed.owner,
                space: fact_space(record.seed.symbol_space),
            });
        }
        // Per-record augmentation pins for BOTH `declare module`
        // and `declare global` contributions (global blocks key on the
        // `$global` sentinel specifier, the emission's own encoding).
        for record in facts.augmentation_contributions.iter() {
            let specifier = match &record.scope_kind {
                AugmentationScopeKind::Global => {
                    crate::fact_emission::GLOBAL_AUGMENTATION_TAG.to_string()
                }
                AugmentationScopeKind::Module(specifier) => specifier.clone(),
            };
            pin(FactKey::ModuleAugmentation {
                specifier: crate::file_artifact_store::InternedSpecifier::from(specifier.as_str()),
                owner: record.owner,
                augmented_name: crate::file_artifact_store::InternedName::from(
                    record.name.as_ref(),
                ),
                space: fact_space(record.symbol_space),
            });
        }
        // The per-target contribution SET + ORDER pins, derived from the
        // shallow walk's BLOCK inventory (every `declare module "X" {…}`
        // / `declare global {…}` block, EMPTY ones included): an empty
        // target pins its bare-target hash, so an empty →
        // first-contribution edit moves a pinned hash even when the
        // target set itself is unchanged. The scope-kind tag keeps
        // `declare global {…}` and `declare module "$global" {…}` in
        // DISTINCT target identities (never string-matched at consumers).
        let header_index = indexed_for_body.shallow_state.decl_bodies().header_index();
        let mut augmentation_targets: Vec<(
            verter_semantic::facts::AugmentationScopeKindTag,
            String,
            verter_type_expr::TopLevelOwnerId,
        )> = Vec::new();
        for block in &header_index.augmentation_blocks {
            let (scope_kind_tag, specifier) = match &block.scope {
                AugmentationScopeKind::Global => (
                    verter_semantic::facts::AugmentationScopeKindTag::Global,
                    crate::fact_emission::GLOBAL_AUGMENTATION_TAG.to_string(),
                ),
                AugmentationScopeKind::Module(specifier) => (
                    verter_semantic::facts::AugmentationScopeKindTag::Module,
                    specifier.clone(),
                ),
            };
            let target = (scope_kind_tag, specifier, block.owner);
            if !augmentation_targets.contains(&target) {
                augmentation_targets.push(target);
            }
        }
        for (scope_kind_tag, specifier, owner) in augmentation_targets {
            pin(FactKey::AugmentationContributionSet {
                scope_kind_tag,
                specifier: crate::file_artifact_store::InternedSpecifier::from(specifier.as_str()),
                owner,
            });
            pin(FactKey::AugmentationContributionOrder {
                scope_kind_tag,
                specifier: crate::file_artifact_store::InternedSpecifier::from(specifier.as_str()),
                owner,
            });
        }
        (facts, all_pinned)
    };
    let ((facts, all_pinned), finalise) =
        crate::fact_signature_helpers::install_fact_tracer(host, cold_body);
    let facts = Arc::new(facts);
    // A fenced serve, an unrecoverable observed-version fact registry,
    // or a non-cacheable / overflowed read set never enters the shared
    // store — the fresh artifact is returned without admission.
    let admissible = serve.store_published && all_pinned;
    match finalise {
        crate::resolver_core::FactReadSetFinalise::Ok(fact_dep_signature) if admissible => {
            let entry = Arc::new(BinderIdentityFactsEntry {
                facts,
                read_set_signature: ReadSetSignature::new(fact_dep_signature),
            });
            store.insert(key, Arc::clone(&entry));
            Some(entry)
        }
        crate::resolver_core::FactReadSetFinalise::Ok(fact_dep_signature) => {
            Some(Arc::new(BinderIdentityFactsEntry {
                facts,
                read_set_signature: ReadSetSignature::new(fact_dep_signature),
            }))
        }
        crate::resolver_core::FactReadSetFinalise::NonCacheable(_)
        | crate::resolver_core::FactReadSetFinalise::Overflow => {
            Some(Arc::new(BinderIdentityFactsEntry {
                facts,
                read_set_signature: ReadSetSignature::overflow(),
            }))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::semantic_query::{
        DeclarationSlotSeed, ResolvedDeclSlotIdentity, SemanticSymbolSpace,
    };
    use crate::types::{HostConfig, UpsertRequest};
    use crate::{FileLanguage, VerterHost};
    use std::sync::Arc;

    fn host() -> Arc<VerterHost> {
        Arc::new(VerterHost::new_standalone(HostConfig::default()))
    }

    fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(id.to_string()),
                input_id: id.to_string(),
                source: Arc::from(source),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();
    }

    /// The ONE generalized slot-finalization choke point:
    /// `ResolvedDeclSlotIdentity = DeclarationSlotSeed + SlotEnvIdentity`,
    /// the seed's four fields copied verbatim, `env` filled from the
    /// defining canonical's LIVE per-canonical env. `type_slot_for`
    /// routes through the same choke point; env enters only the
    /// (content-free) query key, never the seed.
    #[test]
    pub(crate) fn slot_finalization_enters_env_only_in_query_key() {
        let host = host();
        upsert_ts(&host, "/w/a.ts", "export interface Foo { x: string }");
        let _ = host.analyze_with_audit("/w/a.ts");
        let dispatch =
            crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host.as_ref());

        let seed = DeclarationSlotSeed::new(
            Arc::from("/w/a.ts"),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from("Foo"),
            SemanticSymbolSpace::Type,
        );
        let via_seed = dispatch.finalize_slot_seed(seed.clone());
        let via_type_slot_for = dispatch.type_slot_for(
            Arc::from("/w/a.ts"),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from("Foo"),
        );
        assert_eq!(
            via_seed, via_type_slot_for,
            "type_slot_for must route through the finalize_slot_seed choke point"
        );

        // The env tail is the defining canonical's LIVE per-canonical
        // slot environment (J = folded project identity, T / L = the
        // live env hashes), attached through the sealed constructor.
        let env = host.host_view_env_hashes_for("/w/a.ts");
        let project_identity = host.host_view_project_identity_for("/w/a.ts").fold_u32();
        let expected = ResolvedDeclSlotIdentity::type_slot(
            Arc::from("/w/a.ts"),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from("Foo"),
            project_identity,
            env.type_env_hash,
            env.lib_env_hash,
        );
        assert_eq!(
            via_seed, expected,
            "the choke point must fill env from the live per-canonical slot environment"
        );

        // The seed is the env-free projection: env enters only the
        // (content-free) query key, never the seed the family-A
        // artifact stores.
        assert_eq!(
            via_seed.seed(),
            seed,
            "slot.seed() must round-trip the original env-free seed"
        );
    }

    /// A WARM hit on the family-A store must BUBBLE the entry's
    /// `ReadSetSignature` facts into the ambient outer tracer: an
    /// enclosing traced computation admits its own value with the
    /// binder facts observed, so it is invalidated when a pinned fact
    /// moves. Without the bubble the outer read-set would be EMPTY and
    /// the parent cache could admit without binder dependencies.
    #[test]
    fn warm_hit_bubbles_read_set_into_outer_tracer() {
        let host = host();
        upsert_ts(&host, "/w/b.ts", "export interface Foo { x: string }");
        let _ = host.analyze_with_audit("/w/b.ts");

        // Cold-admit the entry.
        let cold = super::produce_binder_identity_facts(host.as_ref(), "/w/b.ts")
            .expect("cold produce must succeed");

        // An outer traced computation that WARM-HITS the store must
        // carry the binder facts in its own finalized read-set.
        let (hit, finalise) =
            crate::fact_signature_helpers::install_fact_tracer(host.as_ref(), || {
                super::produce_binder_identity_facts(host.as_ref(), "/w/b.ts")
            });
        let hit = hit.expect("warm produce must succeed");
        assert!(
            std::sync::Arc::ptr_eq(&cold, &hit),
            "the second produce must be a WARM hit (same admitted entry)"
        );
        let crate::resolver_core::FactReadSetFinalise::Ok(outer_facts) = finalise else {
            panic!("outer tracer must finalise Ok — the warm hit bubbled its read-set");
        };
        assert!(
            outer_facts.iter().any(|fact| matches!(
                fact,
                crate::resolver_core::FactVersionRef::Parse(parse_fact)
                    if matches!(parse_fact.key, verter_semantic::facts::FactKey::SyntacticExportSet)
            )),
            "the outer read-set must contain the bubbled SyntacticExportSet binder fact. \
             Got: {outer_facts:?}"
        );
        assert!(
            outer_facts.iter().any(|fact| matches!(
                fact,
                crate::resolver_core::FactVersionRef::Parse(parse_fact)
                    if matches!(parse_fact.key, verter_semantic::facts::FactKey::DeclContributionOrder { .. })
            )),
            "the outer read-set must contain the bubbled DeclContributionOrder binder fact. \
             Got: {outer_facts:?}"
        );

        // … and the bubbled read-set INVALIDATES when a pinned fact
        // moves (a semantic edit moves the export-set rail).
        upsert_ts(
            &host,
            "/w/b.ts",
            "export interface Foo { x: string }\nexport function g(): void;\n",
        );
        let _ = host.analyze_with_audit("/w/b.ts");
        assert!(
            !crate::fact_signature_helpers::validate_fact_signature(host.as_ref(), &outer_facts),
            "the bubbled outer read-set must invalidate when a pinned binder fact moves"
        );
    }
}
