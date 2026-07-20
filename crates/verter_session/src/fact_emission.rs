//! Parse-time fact emission — parse-time, shallow, HEADER-ONLY.
//!
//! Walks the post-shallow-analysis [`IndexedReady`] (via its
//! [`ShallowFileState`] header inventory) and produces the parse-domain
//! [`FactRegistry`]. Publishing an artifact computes NO body-derived
//! hashes and lowers ZERO declaration bodies:
//!
//! - `MemberShape.semantic_hash` (R28 whole-surface) — from the direct
//!   syntactic member HEADERS.
//! - `MemberPresence.semantic_hash` (R28 header-only — `(name, kind,
//!   exporter_salt)`; NO body). Emitted for type/value member headers
//!   AND for each `enum` variant (a Value-space `EnumMember` of the enum
//!   — variant names live in the dedicated enum header table, the
//!   member-presence authority, distinct from the enum symbol's own
//!   dual-space type/value header).
//! - `SyntacticExportSet.semantic_hash` over the sorted local export
//!   names + bare re-export specifiers.
//! - `ImportRef` per syntactic import binding.
//! - `SyntacticReexportRef` per `export {X} from "spec"` clause.
//! - `ModuleAugmentation` (one fact per `(scope, name)` augmentation
//!   header, with a HEADER-LEVEL shape fingerprint — body sensitivity
//!   for augmentation consumers rides on the per-contributor
//!   `FileWholeHash` facts the stitch records).
//!
//! Body-sensitive `Export` / `LocalDecl` fingerprints are NOT emitted
//! here — they are LAZY: [`LazyBodyFactSource`] computes them on first
//! observation through the artifact's declaration-body memo (lowering
//! exactly the named declaration) and memoizes them in a shared
//! side-store, so observation and validation see the same value without
//! any publish-time body walk.
//!
//! **R12 separation**: parse-domain emission MUST NOT resolve
//! cross-file paths. `ImportRef` carries only `(specifier, binding,
//! space)`; the resolved canonical lives on the resolve-domain
//! `ResolvedImportClause` fact (populated by the resolver
//! producer).

use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::decl_headers::{MemberHeader, MemberHeaderKind};
use verter_semantic::analysis::types::hash_16;
use verter_semantic::analysis::Hash16;
use verter_semantic::facts::{
    compute_member_presence_hash, compute_member_shape_hash, CrossDeclLens, CrossDeclRef, Fact,
    FactKey, FactRegistry, HashOutcome, MemberKind, SymbolSpace,
};

use crate::decl_body_memo::{DeclBodyMemo, LoweredValueDecl};
use crate::file_artifact_store::{
    FileFacts, InternedName, InternedSpecifier, ModuleAugmentationFact,
};
use crate::project_type_store::IndexedReady;
use crate::resolver_core::shallow_file_state::{ExportTarget, ShallowFileState};

/// parse-time emission result: a populated [`FileFacts`] plus the
/// per-file augmentation list.
///
/// The augmentation list is returned separately so [`FileArtifacts`]
/// can place it on its `augmentations` field. The cross-project
/// `augmentation_index` is NOT populated here — the augmentation-
/// index producer builds it lazily on first augmentation-sensitive
/// query.
#[derive(Debug, Default, Clone)]
pub struct ParseFactsEmission {
    pub facts: FileFacts,
    pub augmentations: Vec<ModuleAugmentationFact>,
}

/// Emit parse-domain facts from an [`IndexedReady`].
///
/// Side-effect free over the header inventory; deterministic over the
/// same [`IndexedReady`] input. Producers feed the result into
/// `FileArtifacts::{facts, augmentations}`.
#[must_use]
pub fn emit_parse_facts(indexed: &IndexedReady) -> ParseFactsEmission {
    let shallow = &*indexed.shallow_state;
    // The ONE shared shallow lens: built once at `ShallowFileState`
    // construction (`ShallowLens::from_shallow`) and installed on the
    // declaration-body memo — the SAME `Arc` the lowering-time body
    // fingerprint consults, so fact emission and the memo hash site can
    // never diverge on reference identity.
    let lens = shallow.decl_bodies().shallow_lens();

    let mut registry = FactRegistry::empty();

    // ── Per-symbol header facts: `MemberShape` / `MemberPresence` ──
    emit_type_symbol_headers(&mut registry, shallow);
    emit_value_symbol_headers(&mut registry, shallow);
    emit_enum_symbol_headers(&mut registry, shallow);

    // ── `Export` / `ExportAlias` / `SyntacticReexportRef` for
    //    explicit re-exports ──
    emit_export_targets(&mut registry, shallow);

    // ── `SyntacticExportSet` whole-file surface fingerprint ──
    emit_syntactic_export_set(&mut registry, shallow);

    // ── `ImportRef` per binding ──
    emit_import_refs(&mut registry, shallow);

    let augmentations = collect_augmentations(shallow);
    for aug in &augmentations {
        let body_hash = aug.augmented_member_shape_fingerprint;
        registry.insert(Fact {
            key: FactKey::ModuleAugmentation {
                specifier: InternedSpecifier(aug.specifier.0.clone()),
                owner: aug.owner,
                augmented_name: InternedName(aug.augmented_name.0.clone()),
                space: aug.space,
            },
            semantic_hash: body_hash,
            display_hash: body_hash,
        });
    }

    let lazy = LazyBodyFactSource {
        memo: Arc::clone(shallow.decl_bodies()),
        lens,
        synthesised_value_bodies: shallow
            .synthesised_value_bodies()
            .map(|(key, body)| (key.clone(), Arc::clone(body)))
            .collect(),
        computed: Arc::new(DashMap::default()),
    };

    ParseFactsEmission {
        facts: FileFacts::from_registry_with_lazy(registry, lazy),
        augmentations,
    }
}

// ──────────────────────────────────────────────────────────────────
// Lazy body-sensitive fact source
// ──────────────────────────────────────────────────────────────────

/// Computes the body-sensitive `Export` / `LocalDecl` fact values on
/// first observation, through the artifact's declaration-body memo —
/// the lazy body fact path. Memoized per key in a shared side-store
/// (`Arc`-shared with every clone of the owning [`FileFacts`]).
#[derive(Debug, Clone)]
pub(crate) struct LazyBodyFactSource {
    memo: Arc<DeclBodyMemo>,
    lens: Arc<ShallowLens>,
    /// Eager synthesised value BODIES (the `.vue` implicit `default`)
    /// — their facts compute from the eager `LoweredValueDecl`, never
    /// the lazy memo.
    synthesised_value_bodies: FxHashMap<verter_type_expr::DeclKey, Arc<LoweredValueDecl>>,
    computed: Arc<DashMap<FactKey, Fact>>,
}

impl LazyBodyFactSource {
    pub(crate) fn compute(&self, key: &FactKey) -> Option<Fact> {
        if let Some(hit) = self.computed.get(key) {
            return Some(hit.clone());
        }
        // `name` is the backing LOCAL declaration name probed against the
        // body memo; the emitted `Fact.key` stays the original requested
        // key (e.g. `Export(Bar, Type)`), never the backing local key.
        let (decl_key, space): (verter_type_expr::DeclKey, SymbolSpace) = match key {
            // The `Export` key answers only for names that resolve to a
            // LOCAL declaration — `export { Foo as Bar }` maps the public
            // `Bar` to the backing local `Foo`; reexports are absent from
            // the map and so never compute body facts here.
            FactKey::Export { name, space } => {
                let key = verter_type_expr::DeclKey::new(
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    name.as_ref(),
                );
                let backing = self.lens.local_export_targets.get(&key)?;
                (backing.clone(), *space)
            }
            // `LocalDecl` answers only for non-exported names — mirroring
            // the historical emission's exported-name split, so consistent
            // absence stays consistent.
            FactKey::LocalDecl { name, space } => {
                let key = verter_type_expr::DeclKey::new(
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    name.as_ref(),
                );
                if self.lens.exported.contains(&key) {
                    return None;
                }
                (
                    verter_type_expr::DeclKey::new(
                        verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        name.as_ref(),
                    ),
                    *space,
                )
            }
            _ => return None,
        };
        let outcome = match space {
            // Body-fact fingerprint for a TYPE symbol — the stored memo-owned
            // fact, computed once at lazy lowering time from the transient
            // lowered bodies through the same shared lens (the fenced
            // output-side body-fact site), never a direct typed-body access
            // and never a re-lowering.
            SymbolSpace::Type => {
                if decl_key.owner == verter_type_expr::TopLevelOwnerId::ordinary_file() {
                    self.memo
                        .compat_type_body_hash_input(decl_key.name.as_ref())?
                } else {
                    self.memo
                        .compat_type_body_hash_input_in(decl_key.owner, decl_key.name.as_ref())?
                }
            }
            // No namespace-space declarations are inventoried by the
            // shallow walk — consistent absence.
            SymbolSpace::Namespace => return None,
            SymbolSpace::Value => {
                // Keep the synthesised-value-body vs lazy-memo selection HERE,
                // then read the stored fingerprint through the named compat
                // producer (the fenced output-side value-body-fact site).
                let lowered = match self.synthesised_value_bodies.get(&decl_key) {
                    Some(body) => Arc::clone(body),
                    None if decl_key.owner
                        == verter_type_expr::TopLevelOwnerId::ordinary_file() =>
                    {
                        self.memo.value_decl(decl_key.name.as_ref())?
                    }
                    None => self
                        .memo
                        .value_decl_in(decl_key.owner, decl_key.name.as_ref())?,
                };
                compat_value_body_hash_input(&lowered)
            }
        };
        let semantic_hash = outcome.hash;
        let display_hash = compute_display_hash(&semantic_hash);
        let fact = Fact {
            key: key.clone(),
            semantic_hash,
            display_hash,
        };
        self.computed.insert(key.clone(), fact.clone());
        Some(fact)
    }
}

/// The body fingerprint for a VALUE declaration — the single output/compat
/// value-body-fact site, used by the parse-time fact emitter to compute a body
/// fingerprint and nothing else.
///
/// It takes the ALREADY-resolved [`LoweredValueDecl`] — so the caller keeps
/// the synthesised-value-body vs lazy-memo selection — and returns the
/// STORED memo-owned fingerprint fact
/// ([`LoweredValueDecl::body_hash`]), which the demanded lowering computed
/// once from the transient lowered annotation / object shape through the
/// shared `value_body_fingerprint` producer and the shared lens — no locator
/// deref, no query-time re-lowering (the value-space mirror of
/// [`crate::decl_body_memo::DeclBodyMemo::compat_type_body_hash_input`]).
pub(crate) fn compat_value_body_hash_input(lowered: &LoweredValueDecl) -> HashOutcome {
    lowered.body_hash.to_outcome()
}

// ──────────────────────────────────────────────────────────────────
// Lens — maps `Ref(name)` sites to cross-decl reference identities
// (R12 parse-domain — NO resolved_canonical).
// ──────────────────────────────────────────────────────────────────

/// Resolve `name` against the shallow state's local-symbol +
/// import-binding tables (header data). Falls back to `Unresolved` for
/// free references.
#[derive(Debug)]
pub(crate) struct ShallowLens {
    locals: FxHashSet<verter_type_expr::DeclKey>,
    value_locals: FxHashSet<verter_type_expr::DeclKey>,
    exported: FxHashSet<verter_type_expr::DeclKey>,
    /// Maps a public exported name to its backing LOCAL declaration name
    /// for `export { Foo as Bar }` / `export { Foo }` (the latter maps a
    /// name to itself). Built ONLY from `ExportTarget::Local` entries —
    /// reexports are excluded, so they never compute body facts through
    /// the lazy path. The lazy `Export(Bar, …)` fact preserves the public
    /// key `Bar` while lowering/hashing the backing local `Foo`.
    local_export_targets: FxHashMap<verter_type_expr::DeclKey, verter_type_expr::DeclKey>,
    /// Maps `local_binding_name → source_specifier`.
    import_targets: FxHashMap<verter_type_expr::DeclKey, Arc<str>>,
}

impl ShallowLens {
    /// The SOLE `ShallowLens` builder — called exactly once per
    /// `ShallowFileState` (at construction), which installs the resulting
    /// `Arc` on the declaration-body memo; every consumer (the lowering-time
    /// body fingerprint, the lazy body-fact source) shares that one instance.
    pub(crate) fn from_shallow(shallow: &ShallowFileState) -> Self {
        Self {
            locals: shallow
                .decl_bodies()
                .header_index()
                .type_headers
                .keys()
                .cloned()
                .collect(),
            value_locals: shallow
                .decl_bodies()
                .header_index()
                .value_headers
                .keys()
                .cloned()
                .collect(),
            exported: shallow
                .exports
                .keys()
                .map(|name| {
                    verter_type_expr::DeclKey::new(
                        verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        name.as_str(),
                    )
                })
                .collect(),
            local_export_targets: shallow
                .exports
                .iter()
                .filter_map(|(public_name, target)| match target {
                    ExportTarget::Local { owner, symbol_name } => Some((
                        verter_type_expr::DeclKey::new(
                            verter_type_expr::TopLevelOwnerId::ordinary_file(),
                            public_name.as_str(),
                        ),
                        verter_type_expr::DeclKey::new(*owner, symbol_name.as_str()),
                    )),
                    ExportTarget::Reexport { .. } => None,
                })
                .collect(),
            import_targets: shallow
                .owner_import_targets
                .iter()
                .map(|(local, target)| {
                    (
                        local.clone(),
                        Arc::<str>::from(target.source_specifier.as_str()),
                    )
                })
                .collect(),
        }
    }

    pub(crate) fn for_owner(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
    ) -> OwnedShallowLens<'_> {
        OwnedShallowLens { base: self, owner }
    }

    fn resolve_in(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        space: SymbolSpace,
    ) -> Option<CrossDeclRef> {
        let key = verter_type_expr::DeclKey::new(owner, name);
        if let Some(specifier) = self.import_targets.get(&key) {
            return Some(CrossDeclRef::ImportRef {
                specifier: Arc::clone(specifier),
                binding: Arc::from(name),
                space,
            });
        }
        if self.locals.contains(&key) || self.value_locals.contains(&key) {
            return Some(CrossDeclRef::LocalDecl {
                name: Arc::from(name),
                space,
            });
        }
        Some(CrossDeclRef::Unresolved {
            name: Arc::from(name),
            space,
        })
    }
}

impl CrossDeclLens for ShallowLens {
    fn resolve(&self, name: &str, space: SymbolSpace) -> Option<CrossDeclRef> {
        self.resolve_in(
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            name,
            space,
        )
    }
}

pub(crate) struct OwnedShallowLens<'a> {
    base: &'a ShallowLens,
    owner: verter_type_expr::TopLevelOwnerId,
}

impl CrossDeclLens for OwnedShallowLens<'_> {
    fn resolve(&self, name: &str, space: SymbolSpace) -> Option<CrossDeclRef> {
        self.base.resolve_in(self.owner, name, space)
    }
}

/// The route-fact producer's hash-free classification lens: the FULL
/// three-field import target (specifier + imported name + resolved canonical)
/// plus header TYPE-symbol membership, derived once from the finished
/// `ShallowFileState` beside the fingerprint [`ShallowLens`]. A SECOND view of
/// the same shallow tables — NOT a fingerprint-lens widening: this lens never
/// feeds a hash, so carrying the resolve-domain canonical here leaves the R12
/// parse-domain fingerprint grammar untouched.
#[derive(Debug)]
pub(crate) struct RouteLens {
    canonical_id: Arc<str>,
    type_symbols: FxHashSet<verter_type_expr::DeclKey>,
    import_targets: FxHashMap<verter_type_expr::DeclKey, verter_semantic::facts::ImportRouteTarget>,
}

impl RouteLens {
    /// Built exactly once per `ShallowFileState`, at construction, from the
    /// FINAL routed state (same lifecycle as [`ShallowLens::from_shallow`]).
    pub(crate) fn from_shallow(shallow: &ShallowFileState) -> Self {
        Self {
            canonical_id: shallow.decl_bodies().canonical_id(),
            type_symbols: shallow
                .decl_bodies()
                .header_index()
                .type_headers
                .keys()
                .cloned()
                .collect(),
            import_targets:
                shallow
                    .owner_import_targets
                    .iter()
                    .map(|(local, target)| {
                        (
                            local.clone(),
                            verter_semantic::facts::ImportRouteTarget {
                                source_specifier: Arc::from(target.source_specifier.as_str()),
                                imported_name: Arc::from(target.imported_name.as_str()),
                                canonical_id:
                                    crate::resolver_core::shallow_file_state::external_canonical(
                                        target,
                                    ),
                            },
                        )
                    })
                    .collect(),
        }
    }

    pub(crate) fn for_owner(&self, owner: verter_type_expr::TopLevelOwnerId) -> OwnedRouteLens<'_> {
        OwnedRouteLens { base: self, owner }
    }
}

pub(crate) struct OwnedRouteLens<'a> {
    base: &'a RouteLens,
    owner: verter_type_expr::TopLevelOwnerId,
}

impl verter_semantic::facts::RouteFactLens for OwnedRouteLens<'_> {
    fn resolve_import_route(
        &self,
        local: &str,
        _space: SymbolSpace,
    ) -> Option<verter_semantic::facts::ImportRouteTarget> {
        self.base
            .import_targets
            .get(&verter_type_expr::DeclKey::new(self.owner, local))
            .cloned()
    }
    fn has_type_symbol(&self, name: &str) -> bool {
        self.base
            .type_symbols
            .contains(&verter_type_expr::DeclKey::new(self.owner, name))
    }
    fn own_canonical_id(&self) -> Arc<str> {
        Arc::clone(&self.base.canonical_id)
    }
    fn own_top_level_owner(&self) -> verter_type_expr::TopLevelOwnerId {
        self.owner
    }
}

// ──────────────────────────────────────────────────────────────────
// Emission helpers (HEADER data only)
// ──────────────────────────────────────────────────────────────────

fn member_kind_for_header(header: &MemberHeader) -> MemberKind {
    match header.kind {
        MemberHeaderKind::Property => MemberKind::Property {
            readonly: header.readonly,
            optional: header.optional,
        },
        MemberHeaderKind::Method => MemberKind::Method,
    }
}

fn emit_member_shape_facts(
    registry: &mut FactRegistry,
    name: &str,
    exporter: &InternedName,
    space: SymbolSpace,
    headers: &[MemberHeader],
) {
    if headers.is_empty() {
        return;
    }
    let members_for_shape: Vec<(Arc<str>, MemberKind)> = headers
        .iter()
        .map(|header| {
            (
                Arc::<str>::from(header.name.as_str()),
                member_kind_for_header(header),
            )
        })
        .collect();
    emit_member_facts_from_kinds(registry, name, exporter, space, members_for_shape);
}

/// Emit the `MemberShape` whole-surface fact plus one `MemberPresence`
/// fact per member, from a pre-built `(name, kind)` list. Shared by the
/// header-walk emitters (type/value/enum). Sorts by member name so the
/// shape hash is order-independent; a no-member list emits nothing.
fn emit_member_facts_from_kinds(
    registry: &mut FactRegistry,
    name: &str,
    exporter: &InternedName,
    space: SymbolSpace,
    mut members_for_shape: Vec<(Arc<str>, MemberKind)>,
) {
    if members_for_shape.is_empty() {
        return;
    }
    members_for_shape.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));

    // Exact incoming batch: one `MemberShape` fact plus one `MemberPresence`
    // fact per member — reserve up front instead of doubling mid-batch.
    registry.facts.reserve(1 + members_for_shape.len());

    let shape_hash = compute_member_shape_hash(name, &members_for_shape, space);
    registry.insert(Fact {
        key: FactKey::MemberShape {
            exporter: exporter.clone(),
            space,
        },
        semantic_hash: shape_hash,
        display_hash: shape_hash,
    });
    for (member_name, kind) in &members_for_shape {
        let presence_hash = compute_member_presence_hash(name, member_name.as_ref(), *kind, space);
        registry.insert(Fact {
            key: FactKey::MemberPresence {
                exporter: exporter.clone(),
                name: InternedName(Arc::clone(member_name)),
                space,
            },
            semantic_hash: presence_hash,
            display_hash: presence_hash,
        });
    }
}

fn emit_type_symbol_headers(registry: &mut FactRegistry, shallow: &ShallowFileState) {
    let mut sorted: Vec<&str> = shallow.type_symbol_names().collect();
    sorted.sort_unstable();
    for name in sorted {
        let exporter = InternedName::from(name);
        let headers = shallow.type_member_headers(name).unwrap_or(&[]);
        emit_member_shape_facts(registry, name, &exporter, SymbolSpace::Type, headers);
    }
}

fn emit_value_symbol_headers(registry: &mut FactRegistry, shallow: &ShallowFileState) {
    let header_index = shallow.decl_bodies().header_index();
    let mut sorted: Vec<&str> = shallow.value_symbol_names().collect();
    sorted.sort_unstable();
    for name in sorted {
        let exporter = InternedName::from(name);
        let Some(header) = header_index.value_header(name) else {
            continue;
        };
        emit_member_shape_facts(
            registry,
            name,
            &exporter,
            SymbolSpace::Value,
            &header.object_member_headers,
        );
    }
}

/// Emit header-level member-presence facts for each `enum` declaration.
///
/// An enum's variant names are kept in the dedicated enum header table
/// (the member-presence authority, distinct from the enum symbol's own
/// dual-space type/value header), so they need their own emitter —
/// each variant becomes a Value-space [`MemberKind::EnumMember`] of the
/// enum, on the SAME `MemberShape` / `MemberPresence` rail as type/value
/// member headers. Header-only: variant NAMES + kind, no initializer
/// body lowering (consistent with the zero-body-lowering publish
/// invariant).
fn emit_enum_symbol_headers(registry: &mut FactRegistry, shallow: &ShallowFileState) {
    let mut sorted: Vec<&str> = shallow.enum_symbol_names().collect();
    sorted.sort_unstable();
    for name in sorted {
        let Some(members) = shallow.enum_member_names(name) else {
            continue;
        };
        let exporter = InternedName::from(name);
        let members_for_shape: Vec<(Arc<str>, MemberKind)> = members
            .iter()
            .map(|variant| (Arc::<str>::from(variant.as_str()), MemberKind::EnumMember))
            .collect();
        emit_member_facts_from_kinds(
            registry,
            name,
            &exporter,
            SymbolSpace::Value,
            members_for_shape,
        );
    }
}

fn emit_export_targets(registry: &mut FactRegistry, shallow: &ShallowFileState) {
    let mut sorted: Vec<(&String, &ExportTarget)> = shallow.exports.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (exported_name, target) in sorted {
        match target {
            ExportTarget::Local { .. } => {
                // Local exports carry body-sensitive `Export` facts —
                // produced LAZILY by the body fact path on first
                // observation, never at publish.
            }
            ExportTarget::Reexport {
                source_specifier,
                original_name,
                canonical_id: _,
                is_type,
            } => {
                // R12: parse-domain `SyntacticReexportRef` records
                // ONLY the syntactic shape. The resolved canonical
                // lives on the resolve-domain
                // `ResolvedReexportBinding` fact (populated by the
                // resolver producer).
                let space = if *is_type {
                    SymbolSpace::Type
                } else {
                    SymbolSpace::Value
                };
                let mut buf: Vec<u8> = Vec::with_capacity(64);
                buf.extend_from_slice(b"syntactic-reexport:");
                buf.push(space.tag());
                buf.extend_from_slice(source_specifier.as_bytes());
                buf.push(0xFE);
                buf.extend_from_slice(original_name.as_bytes());
                buf.push(0xFE);
                buf.extend_from_slice(exported_name.as_bytes());
                let h = hash_16(&buf);
                registry.insert(Fact {
                    key: FactKey::SyntacticReexportRef {
                        specifier: InternedSpecifier::from(source_specifier.as_str()),
                        source_name: InternedName::from(original_name.as_str()),
                        target_name: InternedName::from(exported_name.as_str()),
                        space,
                    },
                    semantic_hash: h,
                    display_hash: h,
                });

                // If the re-export aliases the source name (`export
                // { X as Y }`), also emit an `ExportAlias` fact so
                // consumers selecting `Y` directly observe a
                // distinct key.
                if exported_name != original_name {
                    let mut buf2: Vec<u8> = Vec::with_capacity(48);
                    buf2.extend_from_slice(b"export-alias:");
                    buf2.push(space.tag());
                    buf2.extend_from_slice(exported_name.as_bytes());
                    buf2.push(0xFE);
                    buf2.extend_from_slice(original_name.as_bytes());
                    let h2 = hash_16(&buf2);
                    registry.insert(Fact {
                        key: FactKey::ExportAlias {
                            exported_as: InternedName::from(exported_name.as_str()),
                            space,
                        },
                        semantic_hash: h2,
                        display_hash: h2,
                    });
                }
            }
        }
    }
}

fn emit_syntactic_export_set(registry: &mut FactRegistry, shallow: &ShallowFileState) {
    let mut export_names: Vec<&String> = shallow.exports.keys().collect();
    export_names.sort();
    let mut wildcard: Vec<&str> = shallow
        .wildcard_reexports
        .iter()
        .map(|w| w.source_specifier.as_str())
        .collect();
    wildcard.sort();
    let mut buf: Vec<u8> = Vec::with_capacity(128);
    buf.extend_from_slice(b"syntactic-export-set:");
    for n in export_names {
        buf.extend_from_slice(n.as_bytes());
        // Tag with the export target shape (local vs reexport vs
        // wildcard) — this matters for invalidation: a name moving
        // from `Local` to `Reexport` is a structural change.
        if let Some(target) = shallow.exports.get(n) {
            match target {
                ExportTarget::Local { .. } => buf.push(0x01),
                ExportTarget::Reexport { .. } => buf.push(0x02),
            }
        }
        buf.push(0xFE);
    }
    for w in wildcard {
        buf.extend_from_slice(b"*:");
        buf.extend_from_slice(w.as_bytes());
        buf.push(0xFE);
    }
    let h = hash_16(&buf);
    registry.insert(Fact {
        key: FactKey::SyntacticExportSet,
        semantic_hash: h,
        display_hash: h,
    });
}

fn emit_import_refs(registry: &mut FactRegistry, shallow: &ShallowFileState) {
    let mut sorted: Vec<(
        &String,
        &crate::resolver_core::shallow_file_state::ImportTarget,
    )> = shallow.import_targets.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    // Exact incoming batch: one `ImportRef` fact per import binding.
    registry.facts.reserve(sorted.len());
    for (local, target) in sorted {
        let space = SymbolSpace::Type;
        let mut buf: Vec<u8> = Vec::with_capacity(64);
        buf.extend_from_slice(b"import-ref:");
        buf.push(space.tag());
        buf.extend_from_slice(target.source_specifier.as_bytes());
        buf.push(0xFE);
        buf.extend_from_slice(local.as_bytes());
        buf.push(0xFE);
        buf.extend_from_slice(target.imported_name.as_bytes());
        buf.push(0xFE);
        buf.push(u8::from(target.is_namespace));
        // R12: NO position, NO resolved_canonical.
        let h = hash_16(&buf);
        registry.insert(Fact {
            key: FactKey::ImportRef {
                specifier: InternedSpecifier::from(target.source_specifier.as_str()),
                binding: InternedName::from(local.as_str()),
                space,
            },
            semantic_hash: h,
            display_hash: h,
        });
    }
}

/// Compute a `display_hash` that differs from `semantic_hash` if
/// the input body contains any JSDoc / comment that the semantic
/// hash already discards. In the current substrate the body is
/// already alpha-normalised (no comments / JSDoc), so we mix the
/// `semantic_hash` with a stable display salt — future producers
/// can extend this to record real JSDoc + identifier display
/// strings on the body.
fn compute_display_hash(semantic: &Hash16) -> Hash16 {
    let mut buf: Vec<u8> = Vec::with_capacity(32);
    buf.extend_from_slice(b"display:");
    buf.extend_from_slice(semantic);
    hash_16(&buf)
}

// ──────────────────────────────────────────────────────────────────
// Module-augmentation collection
// ──────────────────────────────────────────────────────────────────

/// Derive the parse-domain [`ModuleAugmentationFact`]s from the typed
/// augmentation HEADER inventory on the [`ShallowFileState`] — the
/// SINGLE source of truth.
///
/// `declare module "X" { ... }` / `declare global { ... }` inner declarations
/// are inventoried during shallow analysis into the header index's
/// augmentation tables. One fact is emitted per `(scope, name)` entry,
/// carrying the augmented name, its symbol space, and a HEADER-LEVEL
/// shape fingerprint (scope, name, kind, member-header names,
/// contributor count). Body sensitivity for augmentation consumers
/// rides on the per-contributor `FileWholeHash` facts the stitch
/// records — a body edit moves the contributor's whole hash, which
/// every warm stitch read revalidates against. There is NO raw-source
/// rescan and NO body lowering here.
///
/// Cross-project `augmentation_index` population is NOT done here — the
/// augmentation-index producer populates it lazily on first
/// augmentation-sensitive query.
fn collect_augmentations(shallow: &ShallowFileState) -> Vec<ModuleAugmentationFact> {
    use verter_semantic::analysis::type_eval::AugmentationScopeKind;

    let specifier_for = |scope: &AugmentationScopeKind| -> InternedSpecifier {
        match scope {
            AugmentationScopeKind::Global => InternedSpecifier::from(GLOBAL_AUGMENTATION_TAG),
            AugmentationScopeKind::Module(spec) => InternedSpecifier::from(spec.as_str()),
        }
    };

    let header_index = shallow.decl_bodies().header_index();
    let mut out: Vec<ModuleAugmentationFact> = Vec::new();

    // Type-space augmentations (interfaces, type aliases).
    for (scope, names) in &header_index.augmentation_type_headers {
        for (key, header) in names {
            let name = key.name.as_ref();
            out.push(ModuleAugmentationFact {
                specifier: specifier_for(scope),
                owner: key.owner,
                augmented_name: InternedName::from(name),
                space: SymbolSpace::Type,
                augmented_member_shape_fingerprint: augmentation_header_fingerprint(
                    scope,
                    key.owner,
                    name,
                    format!("{:?}", header.kind).as_str(),
                    header.member_headers.as_slice(),
                    header.contributors.len(),
                ),
            });
        }
    }

    // Value-space augmentations (`const`/`let`/`var`, `function`, `class`).
    for (scope, names) in &header_index.augmentation_value_headers {
        for (key, header) in names {
            let name = key.name.as_ref();
            out.push(ModuleAugmentationFact {
                specifier: specifier_for(scope),
                owner: key.owner,
                augmented_name: InternedName::from(name),
                space: SymbolSpace::Value,
                augmented_member_shape_fingerprint: augmentation_header_fingerprint(
                    scope,
                    key.owner,
                    name,
                    format!("{:?}", header.kind).as_str(),
                    header.object_member_headers.as_slice(),
                    header.contributors.len(),
                ),
            });
        }
    }

    // `HashMap` iteration is nondeterministic; sort for a stable fact order
    // (the augmenter-set fingerprint folds over `parse_stable_hash`, but a
    // determinate fact list keeps every downstream consumer reproducible).
    out.sort_by(|a, b| {
        a.specifier
            .as_ref()
            .cmp(b.specifier.as_ref())
            .then_with(|| a.owner.cmp(&b.owner))
            .then_with(|| a.space.tag().cmp(&b.space.tag()))
            .then_with(|| a.augmented_name.as_ref().cmp(b.augmented_name.as_ref()))
    });
    out
}

/// Pack two `u64` hash digests into a [`Hash16`].
fn hash16_from_pair(lo: u64, hi: u64) -> Hash16 {
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&lo.to_le_bytes());
    out[8..].copy_from_slice(&hi.to_le_bytes());
    out
}

/// HEADER-LEVEL shape fingerprint over one retained augmentation
/// header: scope, name, declaration kind, sorted member-header
/// names/kinds/flags, contributor count. Moves on any skeleton edit
/// (member add/remove/rename, kind change, contributor add/remove);
/// body-VALUE sensitivity is the per-contributor `FileWholeHash` rail.
fn augmentation_header_fingerprint(
    scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
    owner: verter_type_expr::TopLevelOwnerId,
    name: &str,
    kind: &str,
    members: &[MemberHeader],
    contributor_count: usize,
) -> Hash16 {
    use std::hash::{Hash, Hasher};
    let mut sorted: Vec<&MemberHeader> = members.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let digest = |salt: u64| -> u64 {
        let mut h = rustc_hash::FxHasher::default();
        salt.hash(&mut h);
        scope.hash(&mut h);
        owner.hash(&mut h);
        name.hash(&mut h);
        kind.hash(&mut h);
        contributor_count.hash(&mut h);
        for member in &sorted {
            member.name.hash(&mut h);
            matches!(member.kind, MemberHeaderKind::Method).hash(&mut h);
            member.optional.hash(&mut h);
            member.readonly.hash(&mut h);
        }
        h.finish()
    };
    hash16_from_pair(digest(0), digest(0x9E37_79B9_7F4A_7C15))
}

/// Sentinel specifier used inside [`ModuleAugmentationFact`] to
/// distinguish `declare global { … }` blocks from `declare module
/// "..." { … }`. The augmentation-index producer maps this back to
/// `AugmentationTargetKind::GlobalAugmentation`.
pub const GLOBAL_AUGMENTATION_TAG: &str = "$global";

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the shallow state through the REAL construction path (parse →
    /// header index → service-backed lazy memo) and derive the augmentation
    /// facts from the typed inventory — the same path production uses. No
    /// raw-source rescan.
    fn augmentations_for(src: &str) -> Vec<ModuleAugmentationFact> {
        let state = crate::resolver_core::ShallowFileState::service_backed_for_test(src);
        collect_augmentations(&state)
    }

    #[test]
    fn external_specifier_augmentation_fact_from_typed_inventory() {
        let src = r#"declare module "vue" {
  interface ComponentOptions {
    foo: number
  }
}
"#;
        let facts = augmentations_for(src);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].specifier.as_ref(), "vue");
        assert_eq!(facts[0].augmented_name.as_ref(), "ComponentOptions");
        assert_eq!(facts[0].space, SymbolSpace::Type);
        assert_eq!(
            facts[0].owner,
            verter_type_expr::TopLevelOwnerId::ordinary_file()
        );
    }

    #[test]
    fn same_augmentation_header_is_emitted_for_each_exact_lexical_owner() {
        let source = r#"
declare module "vue" { interface Shared { value: string } }
declare module "vue" { interface Shared { value: string } }
"#;
        let module = verter_type_expr::TopLevelOwnerId::module(0);
        let instance = verter_type_expr::TopLevelOwnerId::instance(0);
        let state =
            crate::resolver_core::ShallowFileState::service_backed_for_test_with_statement_owners(
                "/ws/fixture.vue",
                source,
                &[module, instance],
            );

        let facts = collect_augmentations(&state);
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].owner, module);
        assert_eq!(facts[1].owner, instance);
        assert_eq!(facts[0].specifier, facts[1].specifier);
        assert_eq!(facts[0].augmented_name, facts[1].augmented_name);
        assert_ne!(
            facts[0].augmented_member_shape_fingerprint,
            facts[1].augmented_member_shape_fingerprint,
            "lexical owner is part of the authored augmentation fingerprint"
        );
    }

    #[test]
    fn relative_specifier_augmentation_fact_from_typed_inventory() {
        let src = r#"declare module "./local" {
  type Extra = string;
}
"#;
        let facts = augmentations_for(src);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].specifier.as_ref(), "./local");
        assert_eq!(facts[0].augmented_name.as_ref(), "Extra");
    }

    #[test]
    fn wildcard_value_space_augmentation_fact_from_typed_inventory() {
        // A wildcard ambient whose only declaration is a VALUE (`const`): the
        // typed value-augmentation inventory must surface it (this is exactly
        // the case the retired byte-scanner covered and the type-only inventory
        // would have dropped).
        let src = r#"declare module "*.css" {
  const styles: Record<string, string>;
  export default styles;
}
"#;
        let facts = augmentations_for(src);
        let css = facts
            .iter()
            .find(|f| f.specifier.as_ref() == "*.css" && f.augmented_name.as_ref() == "styles")
            .expect("wildcard value augmenter must emit a fact");
        assert_eq!(css.space, SymbolSpace::Value);
    }

    #[test]
    fn global_augmentation_fact_from_typed_inventory() {
        let src = r#"declare global {
  interface Window {
    pageData: any;
  }
}
"#;
        let facts = augmentations_for(src);
        assert!(facts
            .iter()
            .any(|f| f.specifier.as_ref() == GLOBAL_AUGMENTATION_TAG
                && f.augmented_name.as_ref() == "Window"
                && f.space == SymbolSpace::Type));
    }

    #[test]
    fn no_augmentation_yields_empty_list() {
        let facts = augmentations_for("export const x = 1;");
        assert!(facts.is_empty());
    }

    #[test]
    fn nested_decls_do_not_leak_outer_file_scope_decls() {
        // The trailing top-level `const sentinel` is a FILE-scope decl, not an
        // augmentation — it must NOT appear as an augmentation fact (the typed
        // binder keeps augmentation-scope and file-scope inventories separate).
        let src = r#"declare module "x" {
  interface A {
    nested: { a: 1 };
  }
}
const sentinel = 7;"#;
        let facts = augmentations_for(src);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].specifier.as_ref(), "x");
        assert_eq!(facts[0].augmented_name.as_ref(), "A");
    }

    #[test]
    fn member_skeleton_edit_moves_shape_fingerprint_unrelated_edit_does_not() {
        let base = augmentations_for("declare module \"vue\" { interface C { a: number } }");
        let member_changed =
            augmentations_for("declare module \"vue\" { interface C { a: number; b: string } }");
        let unrelated = augmentations_for(
            "declare module \"vue\" { interface C { a: number } }\nconst other = 1;",
        );
        assert_eq!(base.len(), 1);
        assert_ne!(
            base[0].augmented_member_shape_fingerprint,
            member_changed[0].augmented_member_shape_fingerprint,
            "editing the augmenter member skeleton MUST move the shape fingerprint"
        );
        // The unrelated file-scope `const other` is not an augmentation, so the
        // augmentation fact's fingerprint is unchanged.
        let unrelated_c = unrelated
            .iter()
            .find(|f| f.augmented_name.as_ref() == "C")
            .expect("C augmentation fact present");
        assert_eq!(
            base[0].augmented_member_shape_fingerprint,
            unrelated_c.augmented_member_shape_fingerprint,
            "an unrelated file-scope edit MUST NOT move the augmentation fingerprint"
        );
    }
}
