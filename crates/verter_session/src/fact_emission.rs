//! Parse-time fact emission — parse-time, shallow, O(file_size).
//!
//! Walks the post-shallow-analysis [`IndexedReady`] (via its
//! [`ShallowFileState`]) and produces the parse-domain
//! [`FactRegistry`] populated with R10–R16, R28, R29 facts:
//!
//! - `Export.semantic_hash` (over the export body alone — cross-decl
//!   refs as reference-shape edges per R14).
//! - `LocalDecl.semantic_hash` for each NOT-exported local
//!   declaration.
//! - `MemberShape.semantic_hash` (R28 whole-surface).
//! - `MemberPresence.semantic_hash` (R28 header-only — `(name, kind,
//!   exporter_salt)`; NO body).
//! - `SyntacticExportSet.semantic_hash` over the sorted local export
//!   names + bare re-export specifiers.
//! - `ImportRef` per syntactic import binding.
//! - `SyntacticReexportRef` per `export {X} from "spec"` clause.
//! - `MacroSurface` per macro invocation in the script-analysis
//!   snapshot.
//! - `TemplateRoot` (Vue SFC root reachability fact).
//! - `ModuleAugmentation` (one fact per `declare module … {}`
//!   block).
//!
//! **R12 separation**: parse-domain emission MUST NOT resolve
//! cross-file paths. `ImportRef` carries only `(specifier, binding,
//! space)`; the resolved canonical lives on the resolve-domain
//! `ResolvedImportClause` fact (populated by the resolver
//! producer).
//!
//! **R28 shallow-walk arch-guard**: the parse-phase fact emitter
//! MUST NOT call into cross-decl AST traversal. Same-file member
//! body fingerprints (`Member` facts) are NOT emitted here — they
//! emit lazily into `MemberSemanticFactStore` /
//! `MemberDisplayFactStore` on first member-access query (the lazy member-body store).

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::types::hash_16;
use verter_semantic::analysis::Hash16;
use verter_semantic::facts::{
    compute_member_presence_hash, compute_member_shape_hash, compute_semantic_hash, CrossDeclLens,
    CrossDeclRef, Fact, FactKey, FactRegistry, MemberKind, SymbolSpace,
};
use verter_type_expr::{ObjectExpr, ObjectMember, TypeExpr};

use crate::file_artifact_store::{
    FileFacts, InternedName, InternedSpecifier, ModuleAugmentationFact,
};
use crate::project_type_store::IndexedReady;
use crate::resolver_core::shallow_file_state::{
    ExportTarget, ShallowFileState, ShallowTypeSymbol, ShallowValueSymbol,
};

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
/// Side-effect free; deterministic over the same [`IndexedReady`]
/// input. Producers feed the result into
/// `FileArtifacts::{facts, augmentations}`.
#[must_use]
pub fn emit_parse_facts(indexed: &IndexedReady) -> ParseFactsEmission {
    let shallow = &*indexed.shallow_state;
    let lens = ShallowLens::from_shallow(shallow);

    let mut registry = FactRegistry::empty();

    // ── Per-symbol facts: `Export` / `LocalDecl` / `MemberShape` /
    //    `MemberPresence` ──
    emit_type_symbols(&mut registry, shallow, &lens);
    emit_value_symbols(&mut registry, shallow, &lens);

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
                augmented_name: InternedName(aug.augmented_name.0.clone()),
                space: aug.space,
            },
            semantic_hash: body_hash,
            display_hash: body_hash,
        });
    }

    ParseFactsEmission {
        facts: FileFacts::from_registry(registry),
        augmentations,
    }
}

// ──────────────────────────────────────────────────────────────────
// Lens — maps `Ref(name)` sites to cross-decl reference identities
// (R12 parse-domain — NO resolved_canonical).
// ──────────────────────────────────────────────────────────────────

/// Resolve `name` against the shallow state's local-symbol +
/// import-binding tables. Falls back to `Unresolved` for free
/// references.
struct ShallowLens {
    locals: rustc_hash::FxHashSet<String>,
    value_locals: rustc_hash::FxHashSet<String>,
    /// Maps `local_binding_name → source_specifier`.
    import_targets: FxHashMap<String, Arc<str>>,
}

impl ShallowLens {
    fn from_shallow(shallow: &ShallowFileState) -> Self {
        Self {
            locals: shallow.symbols.keys().cloned().collect(),
            value_locals: shallow.value_symbols.keys().cloned().collect(),
            import_targets: shallow
                .import_targets
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
}

impl CrossDeclLens for ShallowLens {
    fn resolve(&self, name: &str, space: SymbolSpace) -> Option<CrossDeclRef> {
        if let Some(specifier) = self.import_targets.get(name) {
            return Some(CrossDeclRef::ImportRef {
                specifier: Arc::clone(specifier),
                binding: Arc::from(name),
                space,
            });
        }
        let is_local_type = self.locals.contains(name);
        let is_local_value = self.value_locals.contains(name);
        if is_local_type || is_local_value {
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

// ──────────────────────────────────────────────────────────────────
// Emission helpers
// ──────────────────────────────────────────────────────────────────

fn emit_type_symbols(registry: &mut FactRegistry, shallow: &ShallowFileState, lens: &ShallowLens) {
    let mut sorted: Vec<(&String, &ShallowTypeSymbol)> = shallow.symbols.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (name, symbol) in sorted {
        let exporter = InternedName::from(name.as_str());
        let space = SymbolSpace::Type;

        // Body fingerprint over the type-alias / interface body. For a merged
        // interface this hashes the union of every contributor's members, so a
        // change in any same-name contributor invalidates downstream importers.
        let body = symbol.body.lookup_object();
        let outcome = compute_semantic_hash(body.as_ref(), space, lens);
        let body_hash = outcome.hash;
        let display_hash = compute_display_hash(body.as_ref(), &body_hash);

        // `Export` if exported; `LocalDecl` otherwise.
        let is_exported = shallow.exports.contains_key(name);
        let key = if is_exported {
            FactKey::Export {
                name: exporter.clone(),
                space,
            }
        } else {
            FactKey::LocalDecl {
                name: exporter.clone(),
                space,
            }
        };
        registry.insert(Fact {
            key,
            semantic_hash: body_hash,
            display_hash,
        });

        // `MemberShape` + `MemberPresence` per member, derived from
        // the type's `member_deps` skeleton (which the shallow walk
        // already maintains). The kind is `Property` by default —
        // declaration-level kind information lives on the body, NOT
        // on the member_deps map.
        if !symbol.member_deps.is_empty() {
            let members_for_shape = members_for_shape(&symbol.member_deps);
            let shape_hash = compute_member_shape_hash(name, &members_for_shape, space);
            registry.insert(Fact {
                key: FactKey::MemberShape {
                    exporter: exporter.clone(),
                    space,
                },
                semantic_hash: shape_hash,
                display_hash: shape_hash,
            });
            for (member_name, _) in &members_for_shape {
                let member_kind = MemberKind::Property {
                    readonly: false,
                    optional: false,
                };
                let presence_hash =
                    compute_member_presence_hash(name, member_name.as_ref(), member_kind, space);
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
    }
}

fn members_for_shape(
    member_deps: &rustc_hash::FxHashMap<String, Vec<String>>,
) -> Vec<(Arc<str>, MemberKind)> {
    let mut members: Vec<(Arc<str>, MemberKind)> = member_deps
        .keys()
        .map(|n| {
            (
                Arc::<str>::from(n.as_str()),
                MemberKind::Property {
                    readonly: false,
                    optional: false,
                },
            )
        })
        .collect();
    members.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
    members
}

fn emit_value_symbols(registry: &mut FactRegistry, shallow: &ShallowFileState, lens: &ShallowLens) {
    let mut sorted: Vec<(&String, &ShallowValueSymbol)> = shallow.value_symbols.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (name, symbol) in sorted {
        let exporter = InternedName::from(name.as_str());
        let space = SymbolSpace::Value;

        // Body hash: take the type annotation if present, else fall
        // back to a synthesised type-expression that captures the
        // declaration kind. The structural representation MUST be
        // distinct across edits.
        let body = match (&symbol.type_annotation, symbol.signatures.first()) {
            (Some(ty), _) => ty.clone(),
            (None, Some(_)) => TypeExpr::Unknown {
                raw: format!("{:?}", symbol.signatures),
            },
            _ => TypeExpr::Unknown {
                raw: format!("{:?}::{:?}", symbol.kind, symbol.object_shape),
            },
        };
        let outcome = compute_semantic_hash(&body, space, lens);
        let body_hash = outcome.hash;
        let display_hash = compute_display_hash(&body, &body_hash);

        // `Export` if exported; `LocalDecl` otherwise.
        let is_exported = shallow.exports.contains_key(name);
        let key = if is_exported {
            FactKey::Export {
                name: exporter.clone(),
                space,
            }
        } else {
            FactKey::LocalDecl {
                name: exporter.clone(),
                space,
            }
        };
        registry.insert(Fact {
            key,
            semantic_hash: body_hash,
            display_hash,
        });

        // Enum members → `MemberShape` + `MemberPresence` (R28).
        if let Some(enum_members) = &symbol.enum_members {
            let members_for_shape: Vec<(Arc<str>, MemberKind)> = {
                let mut v: Vec<(Arc<str>, MemberKind)> = enum_members
                    .keys()
                    .map(|n| (Arc::<str>::from(n.as_str()), MemberKind::EnumMember))
                    .collect();
                v.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
                v
            };
            let shape_hash = compute_member_shape_hash(name, &members_for_shape, space);
            registry.insert(Fact {
                key: FactKey::MemberShape {
                    exporter: exporter.clone(),
                    space,
                },
                semantic_hash: shape_hash,
                display_hash: shape_hash,
            });
            for (member_name, _) in &members_for_shape {
                let presence_hash = compute_member_presence_hash(
                    name,
                    member_name.as_ref(),
                    MemberKind::EnumMember,
                    space,
                );
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

        // Object literal shape → `MemberShape` + `MemberPresence`
        // (covers `const x = { a: ..., b: ... }`).
        if let Some(obj) = &symbol.object_shape {
            let members_for_shape = members_for_object_shape(obj);
            if !members_for_shape.is_empty() {
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
                    let presence_hash =
                        compute_member_presence_hash(name, member_name.as_ref(), *kind, space);
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
        }
    }
}

fn members_for_object_shape(obj: &ObjectExpr) -> Vec<(Arc<str>, MemberKind)> {
    let mut out: Vec<(Arc<str>, MemberKind)> = Vec::with_capacity(obj.properties.len());
    for member in &obj.properties {
        match member {
            ObjectMember::Property(p) => out.push((
                Arc::<str>::from(p.name.as_str()),
                MemberKind::Property {
                    readonly: p.readonly,
                    optional: p.optional,
                },
            )),
            ObjectMember::Method(m) => {
                out.push((Arc::<str>::from(m.name.as_str()), MemberKind::Method))
            }
            ObjectMember::IndexSignature(_) => continue,
            ObjectMember::CallSignature(_) => continue,
            ObjectMember::ConstructSignature(_) => continue,
        }
    }
    out.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
    out
}

fn emit_export_targets(registry: &mut FactRegistry, shallow: &ShallowFileState) {
    let mut sorted: Vec<(&String, &ExportTarget)> = shallow.exports.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (exported_name, target) in sorted {
        match target {
            ExportTarget::Local { .. } => {
                // Local exports are already covered by
                // `emit_type_symbols` / `emit_value_symbols`. The
                // `Export` key was already inserted there.
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
fn compute_display_hash(_body: &TypeExpr, semantic: &Hash16) -> Hash16 {
    let mut buf: Vec<u8> = Vec::with_capacity(32);
    buf.extend_from_slice(b"display:");
    buf.extend_from_slice(semantic);
    hash_16(&buf)
}

// ──────────────────────────────────────────────────────────────────
// Module-augmentation collection
// ──────────────────────────────────────────────────────────────────

/// Derive the parse-domain [`ModuleAugmentationFact`]s from the typed
/// augmentation inventory the binder retained on the
/// [`ShallowFileState`] — the SINGLE source of truth.
///
/// `declare module "X" { ... }` / `declare global { ... }` inner declarations
/// are retained during shallow analysis into
/// [`ShallowFileState::augmentation_scopes`] (type space) and
/// [`ShallowFileState::augmentation_value_scopes`] (value space). One fact is
/// emitted per `(scope, name)` entry, carrying the augmented name, its symbol
/// space, and a content-sensitive shape fingerprint over the retained body.
/// There is NO raw-source rescan — the shallow inventory already classified and
/// retained every augmentation declaration (Build Philosophy: no stage rescans
/// raw source to rediscover what shallow processing captured).
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

    let mut out: Vec<ModuleAugmentationFact> = Vec::new();

    // Type-space augmentations (interfaces, type aliases).
    for ((scope, name), symbol) in &shallow.augmentation_scopes {
        out.push(ModuleAugmentationFact {
            specifier: specifier_for(scope),
            augmented_name: InternedName::from(name.as_str()),
            space: SymbolSpace::Type,
            augmented_member_shape_fingerprint: type_augmentation_shape_fingerprint(
                scope, name, symbol,
            ),
        });
    }

    // Value-space augmentations (`const`/`let`/`var`, `function`, `class`).
    for ((scope, name), symbol) in &shallow.augmentation_value_scopes {
        out.push(ModuleAugmentationFact {
            specifier: specifier_for(scope),
            augmented_name: InternedName::from(name.as_str()),
            space: SymbolSpace::Value,
            augmented_member_shape_fingerprint: value_augmentation_shape_fingerprint(
                scope, name, symbol,
            ),
        });
    }

    // `HashMap` iteration is nondeterministic; sort for a stable fact order
    // (the augmenter-set fingerprint folds over `parse_stable_hash`, but a
    // determinate fact list keeps every downstream consumer reproducible).
    out.sort_by(|a, b| {
        a.specifier
            .as_ref()
            .cmp(b.specifier.as_ref())
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

/// Content-sensitive shape fingerprint over a retained TYPE augmentation body.
/// Folds the scope, name and every contributor body (`TypeExpr: Hash`) so a
/// body edit moves the fingerprint while an unrelated edit does not.
fn type_augmentation_shape_fingerprint(
    scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
    name: &str,
    symbol: &ShallowTypeSymbol,
) -> Hash16 {
    use std::hash::{Hash, Hasher};
    let digest = |salt: u64| -> u64 {
        let mut h = rustc_hash::FxHasher::default();
        salt.hash(&mut h);
        scope.hash(&mut h);
        name.hash(&mut h);
        for contributor in symbol.body.contributors() {
            contributor.hash(&mut h);
        }
        h.finish()
    };
    hash16_from_pair(digest(0), digest(0x9E37_79B9_7F4A_7C15))
}

/// Content-sensitive shape fingerprint over a retained VALUE augmentation
/// declaration. `FunctionSignature` is not `Hash`, so its `parameters`
/// (`FunctionParam: Hash`) and `return_type` (`TypeExpr: Hash`) are folded
/// explicitly alongside the value kind, type annotation and object shape.
fn value_augmentation_shape_fingerprint(
    scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
    name: &str,
    symbol: &ShallowValueSymbol,
) -> Hash16 {
    use std::hash::{Hash, Hasher};
    let digest = |salt: u64| -> u64 {
        let mut h = rustc_hash::FxHasher::default();
        salt.hash(&mut h);
        scope.hash(&mut h);
        name.hash(&mut h);
        value_kind_tag(symbol.kind).hash(&mut h);
        symbol.type_annotation.hash(&mut h);
        symbol.object_shape.hash(&mut h);
        for sig in &symbol.signatures {
            sig.parameters.hash(&mut h);
            sig.return_type.hash(&mut h);
        }
        h.finish()
    };
    hash16_from_pair(digest(0), digest(0x9E37_79B9_7F4A_7C15))
}

/// Stable byte tag for a [`ValueDeclKind`] (the enum is not `Hash`).
fn value_kind_tag(kind: verter_semantic::analysis::type_eval::ValueDeclKind) -> u8 {
    use verter_semantic::analysis::type_eval::ValueDeclKind;
    match kind {
        ValueDeclKind::Const => 0,
        ValueDeclKind::Let => 1,
        ValueDeclKind::Var => 2,
        ValueDeclKind::Function => 3,
        ValueDeclKind::AsyncFunction => 4,
        ValueDeclKind::Class => 5,
        ValueDeclKind::Enum => 6,
    }
}

/// Sentinel specifier used inside [`ModuleAugmentationFact`] to
/// distinguish `declare global { … }` blocks from `declare module
/// "..." { … }`. The augmentation-index producer maps this back to
/// `AugmentationTargetKind::GlobalAugmentation`.
pub const GLOBAL_AUGMENTATION_TAG: &str = "$global";

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the shallow state through the REAL binder (parse → eval env →
    /// shallow inventory) and derive the augmentation facts from the typed
    /// inventory — the same path production uses. No raw-source rescan.
    fn augmentations_for(src: &str) -> Vec<ModuleAugmentationFact> {
        let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(src);
        let analysis = Arc::new(
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(),
        );
        let state = ShallowFileState::from_analysis(Hash16::default(), analysis, Some(&env));
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
    fn body_edit_moves_shape_fingerprint_unrelated_edit_does_not() {
        let base = augmentations_for("declare module \"vue\" { interface C { a: number } }");
        let body_changed =
            augmentations_for("declare module \"vue\" { interface C { a: string } }");
        let unrelated = augmentations_for(
            "declare module \"vue\" { interface C { a: number } }\nconst other = 1;",
        );
        assert_eq!(base.len(), 1);
        assert_ne!(
            base[0].augmented_member_shape_fingerprint,
            body_changed[0].augmented_member_shape_fingerprint,
            "editing the augmenter body MUST move the shape fingerprint"
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
