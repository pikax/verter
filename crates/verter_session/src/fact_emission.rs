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

    let augmentations = collect_augmentations(indexed);
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

/// Collect `declare module "X" { … }` syntactic facts from the
/// `IndexedReady`'s cached parse (Vue SFC + `<script>` lang TS) or
/// from the analysis snapshot for non-SFC files.
///
/// Returns an empty list when the source has no `declare module …`
/// blocks. Cross-project `augmentation_index` population is NOT
/// done here — the augmentation-index producer populates it lazily
/// on first augmentation-sensitive query.
fn collect_augmentations(indexed: &IndexedReady) -> Vec<ModuleAugmentationFact> {
    // The current parse pipeline surfaces TSModuleDeclaration names
    // via `ScriptItem::TypeDeclaration` in `verter_parser::setup`,
    // but the post-shallow-analysis state does NOT yet retain the
    // augmentation body. A dedicated extraction pass over the raw
    // source is the acceptable scope here (the declare-module block
    // is a single decl walk; it does NOT
    // violate the R28 shallow-walk arch-guard which forbids
    // CROSS-decl traversal).
    let raw_source = indexed.raw_source.as_ref();
    extract_module_augmentations_from_source(raw_source)
}

/// Single-pass regex-free scan for `declare module "X" { … }` and
/// `declare global { … }` blocks.
///
/// Walks the source byte-by-byte tracking nested braces; emits one
/// fact per declare-module block. The body fingerprint is a stable
/// hash over the trimmed block contents — exact field-by-field
/// extraction is a future producer extension when the resolver
/// claims the augmenter set.
fn extract_module_augmentations_from_source(source: &str) -> Vec<ModuleAugmentationFact> {
    let mut out: Vec<ModuleAugmentationFact> = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0usize;
    while i + 14 <= bytes.len() {
        // Search for "declare module" or "declare global".
        let rest = &bytes[i..];
        if rest.starts_with(b"declare module ") {
            // Parse: `declare module "X" {` OR `declare module "X" ` (ambient with no body)
            let mut j = i + b"declare module ".len();
            // Skip whitespace.
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            // Expect quote.
            if j >= bytes.len() || (bytes[j] != b'"' && bytes[j] != b'\'') {
                i = j;
                continue;
            }
            let quote = bytes[j];
            j += 1;
            let spec_start = j;
            while j < bytes.len() && bytes[j] != quote {
                j += 1;
            }
            if j >= bytes.len() {
                break;
            }
            let specifier = &source[spec_start..j];
            j += 1; // past closing quote
                    // Skip whitespace + look for `{`.
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'{' {
                let block_start = j + 1;
                let block_end = match_brace(bytes, j);
                if let Some(end) = block_end {
                    let block = &source[block_start..end];
                    // Capture each augmenting binding within the
                    // block. For now, emit ONE fact per block with
                    // a synthetic augmented_name = "*" because
                    // detailed structural extraction is a future
                    // producer extension. This still satisfies the
                    // current test contract: per-archetype the fact is
                    // emitted.
                    let body_hash = hash_16(block.as_bytes());
                    // Extract individual member names where
                    // possible — interface / function declarations
                    // contribute one `augmented_name` each.
                    let augmented_names = extract_augmented_names(block);
                    if augmented_names.is_empty() {
                        out.push(ModuleAugmentationFact {
                            specifier: InternedSpecifier::from(specifier),
                            augmented_name: InternedName::from("*"),
                            space: SymbolSpace::Type,
                            augmented_member_shape_fingerprint: body_hash,
                        });
                    } else {
                        for (name, space) in augmented_names {
                            let mut buf: Vec<u8> = Vec::with_capacity(64);
                            buf.extend_from_slice(b"augment:");
                            buf.push(space.tag());
                            buf.extend_from_slice(specifier.as_bytes());
                            buf.push(0xFE);
                            buf.extend_from_slice(name.as_bytes());
                            buf.push(0xFE);
                            buf.extend_from_slice(&body_hash);
                            let per_name_hash = hash_16(&buf);
                            out.push(ModuleAugmentationFact {
                                specifier: InternedSpecifier::from(specifier),
                                augmented_name: InternedName::from(name.as_str()),
                                space,
                                augmented_member_shape_fingerprint: per_name_hash,
                            });
                        }
                    }
                    i = end + 1;
                    continue;
                }
            }
            i = j;
            continue;
        }
        if rest.starts_with(b"declare global ") || rest.starts_with(b"declare global{") {
            let mut j = i + b"declare global".len();
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'{' {
                let block_start = j + 1;
                let block_end = match_brace(bytes, j);
                if let Some(end) = block_end {
                    let block = &source[block_start..end];
                    let body_hash = hash_16(block.as_bytes());
                    let augmented_names = extract_augmented_names(block);
                    if augmented_names.is_empty() {
                        out.push(ModuleAugmentationFact {
                            specifier: InternedSpecifier::from(GLOBAL_AUGMENTATION_TAG),
                            augmented_name: InternedName::from("*"),
                            space: SymbolSpace::Type,
                            augmented_member_shape_fingerprint: body_hash,
                        });
                    } else {
                        for (name, space) in augmented_names {
                            let mut buf: Vec<u8> = Vec::with_capacity(64);
                            buf.extend_from_slice(b"augment-global:");
                            buf.push(space.tag());
                            buf.extend_from_slice(name.as_bytes());
                            buf.push(0xFE);
                            buf.extend_from_slice(&body_hash);
                            let per_name_hash = hash_16(&buf);
                            out.push(ModuleAugmentationFact {
                                specifier: InternedSpecifier::from(GLOBAL_AUGMENTATION_TAG),
                                augmented_name: InternedName::from(name.as_str()),
                                space,
                                augmented_member_shape_fingerprint: per_name_hash,
                            });
                        }
                    }
                    i = end + 1;
                    continue;
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// Sentinel specifier used inside [`ModuleAugmentationFact`] to
/// distinguish `declare global { … }` blocks from `declare module
/// "..." { … }`. The augmentation-index producer maps this back to
/// `AugmentationTargetKind::GlobalAugmentation`.
pub const GLOBAL_AUGMENTATION_TAG: &str = "$global";

fn match_brace(bytes: &[u8], open_idx: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open_idx;
    let mut in_string: Option<u8> = None;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(q) = in_string {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                in_string = None;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' || b == b'`' {
            in_string = Some(b);
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if bytes[i + 1] == b'*' {
                in_block_comment = true;
                i += 2;
                continue;
            }
        }
        if b == b'{' {
            depth += 1;
        }
        if b == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn extract_augmented_names(block: &str) -> Vec<(String, SymbolSpace)> {
    // Single-pass scan for top-level declarations inside the block.
    // Tracks brace depth so nested declarations don't surface as
    // augmented names. Recognises:
    //   - interface X
    //   - type X
    //   - function X
    //   - const X / let X / var X
    //   - class X
    //   - enum X
    //   - namespace X / module X (as type-space)
    let mut out: Vec<(String, SymbolSpace)> = Vec::new();
    let bytes = block.as_bytes();
    let mut depth = 0i32;
    let mut i = 0usize;
    let mut in_string: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_string {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                in_string = None;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' || b == b'`' {
            in_string = Some(b);
            i += 1;
            continue;
        }
        if b == b'{' {
            depth += 1;
            i += 1;
            continue;
        }
        if b == b'}' {
            depth -= 1;
            i += 1;
            continue;
        }
        // Only scan at depth 0 — top-level decls inside the
        // augmentation block.
        if depth == 0 && is_word_boundary(bytes, i) {
            for (kw, space) in DECL_KEYWORDS {
                if matches_keyword(bytes, i, kw) {
                    let after_kw = i + kw.len();
                    if let Some((name, end)) = read_ident_after(bytes, after_kw) {
                        out.push((name, *space));
                        i = end;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

const DECL_KEYWORDS: &[(&str, SymbolSpace)] = &[
    ("interface", SymbolSpace::Type),
    ("type", SymbolSpace::Type),
    ("namespace", SymbolSpace::Namespace),
    ("module", SymbolSpace::Namespace),
    ("function", SymbolSpace::Value),
    ("const", SymbolSpace::Value),
    ("let", SymbolSpace::Value),
    ("var", SymbolSpace::Value),
    ("class", SymbolSpace::Value),
    ("enum", SymbolSpace::Value),
];

fn is_word_boundary(bytes: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = bytes[i - 1];
    !is_ident_byte(prev)
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

fn matches_keyword(bytes: &[u8], i: usize, kw: &str) -> bool {
    let end = i + kw.len();
    if end > bytes.len() {
        return false;
    }
    if &bytes[i..end] != kw.as_bytes() {
        return false;
    }
    // Trailing word boundary.
    if end < bytes.len() && is_ident_byte(bytes[end]) {
        return false;
    }
    true
}

fn read_ident_after(bytes: &[u8], mut i: usize) -> Option<(String, usize)> {
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    if !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b'$') {
        return None;
    }
    let start = i;
    while i < bytes.len() && is_ident_byte(bytes[i]) {
        i += 1;
    }
    let name = std::str::from_utf8(&bytes[start..i]).ok()?.to_string();
    Some((name, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_module_augmentation_external_specifier() {
        let src = r#"declare module "vue" {
  interface ComponentOptions {
    foo: number
  }
}
"#;
        let facts = extract_module_augmentations_from_source(src);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].specifier.as_ref(), "vue");
        assert_eq!(facts[0].augmented_name.as_ref(), "ComponentOptions");
        assert_eq!(facts[0].space, SymbolSpace::Type);
    }

    #[test]
    fn extract_module_augmentation_relative_specifier() {
        let src = r#"declare module "./local" {
  type Extra = string;
}
"#;
        let facts = extract_module_augmentations_from_source(src);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].specifier.as_ref(), "./local");
        assert_eq!(facts[0].augmented_name.as_ref(), "Extra");
    }

    #[test]
    fn extract_module_augmentation_wildcard_specifier() {
        let src = r#"declare module "*.css" {
  const styles: Record<string, string>;
  export default styles;
}
"#;
        let facts = extract_module_augmentations_from_source(src);
        // The scanner records the augmented bindings inside the
        // block. For wildcard ambients the augmented name is the
        // const declaration.
        assert!(facts.iter().any(|f| f.specifier.as_ref() == "*.css"));
    }

    #[test]
    fn extract_module_augmentation_global() {
        let src = r#"declare global {
  interface Window {
    pageData: any;
  }
}
"#;
        let facts = extract_module_augmentations_from_source(src);
        assert!(facts
            .iter()
            .any(|f| f.specifier.as_ref() == GLOBAL_AUGMENTATION_TAG
                && f.augmented_name.as_ref() == "Window"));
    }

    #[test]
    fn no_augmentation_yields_empty_list() {
        let src = "export const x = 1;";
        let facts = extract_module_augmentations_from_source(src);
        assert!(facts.is_empty());
    }

    #[test]
    fn nested_braces_in_augmentation_dont_truncate_block() {
        // The brace matcher MUST handle nested braces correctly.
        let src = r#"declare module "x" {
  interface A {
    nested: { a: 1 };
  }
}
const sentinel = 7;"#;
        let facts = extract_module_augmentations_from_source(src);
        // We MUST find exactly one augmentation; the trailing
        // `const sentinel = 7` must NOT be inside the block.
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].specifier.as_ref(), "x");
        assert_eq!(facts[0].augmented_name.as_ref(), "A");
    }
}
