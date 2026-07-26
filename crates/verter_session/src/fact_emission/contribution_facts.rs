//! Contribution-set / order-sensitive parse-domain fact emission —
//! the sibling cluster of [`super::fact_emission`]: the per-augmentation-target
//! contribution SET / ORDER facts, the per-declaration-slot
//! `DeclContributionOrder` rail, and the whole-file scope-inventory set facts
//! (`AugmentationTargetSet` / `NamespaceScopeSet`). All HEADER-level: no
//! declaration body lowering, no cross-file resolution.

use rustc_hash::FxHashSet;
use verter_semantic::analysis::decl_headers::MemberHeader;
use verter_semantic::analysis::Hash16;
use verter_semantic::facts::{Fact, FactKey, FactRegistry, SymbolSpace};

use crate::file_artifact_store::{InternedName, InternedSpecifier};
use crate::project_type_store::IndexedReady;
use crate::resolver_core::shallow_file_state::ShallowFileState;

/// Map an augmentation scope to the specifier encoding used by the
/// augmentation facts: the `$global` sentinel for `declare global`,
/// the raw authored specifier for `declare module "X"`.
pub(super) fn specifier_for(
    scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
) -> InternedSpecifier {
    use verter_semantic::analysis::type_eval::AugmentationScopeKind;
    match scope {
        AugmentationScopeKind::Global => InternedSpecifier::from(super::GLOBAL_AUGMENTATION_TAG),
        AugmentationScopeKind::Module(spec) => InternedSpecifier::from(spec.as_str()),
    }
}

// ──────────────────────────────────────────────────────────────────
// Order-sensitive contributor-sequence facts (HEADER-level)
// ──────────────────────────────────────────────────────────────────

const DECL_CONTRIBUTION_ORDER_SALT: &[u8] = b"verter-decl-contribution-order:v1";
const AUG_CONTRIBUTION_SET_SALT: &[u8] = b"verter-aug-contribution-set:v1";
const AUG_CONTRIBUTION_ORDER_SALT: &[u8] = b"verter-aug-contribution-order:v1";

/// Hash ONE contributing declaration's cosmetic-invariant shape: the
/// declaration span's source slice with comments stripped and
/// whitespace runs collapsed (string / template-literal contents kept
/// verbatim), so intra-declaration whitespace / comment edits leave the
/// shape unchanged while real content edits and reorders move it. Uses
/// the parser-recorded span against the position-preserving eval source
/// (no re-parse, no body lowering).
fn declaration_slice_shape(eval_source: &str, span: verter_span::Span) -> Hash16 {
    let slice = eval_source
        .get(span.start as usize..span.end as usize)
        .unwrap_or("");
    let normalized = normalize_declaration_slice(slice);
    xxhash_rust::xxh3::xxh3_128(&normalized).to_le_bytes()
}

/// Cosmetic-invariant normalization of a declaration's source slice:
/// `//…` and `/*…*/` comments are stripped OUTSIDE string / template
/// literals (their contents — including `//` / `/*` sequences and `${`
/// expressions — are preserved verbatim), and ALL remaining whitespace
/// is dropped. Two declarations whose token streams differ only in
/// trivia normalize identically, so intra-declaration whitespace /
/// comment edits stay warm while content edits and reorders move the
/// hash. This is a hash normalization only (never control flow): an
/// unterminated construct simply normalizes to the end of the slice.
fn normalize_declaration_slice(slice: &str) -> Vec<u8> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum StringState {
        None,
        Single,
        Double,
        Template,
    }
    let bytes = slice.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut state = StringState::None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if state != StringState::None {
            // Inside a string / template literal: copy verbatim until
            // the matching closer (escapes keep the escaped byte).
            out.push(b);
            if b == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            let closer = match state {
                StringState::Single => b'\'',
                StringState::Double => b'"',
                StringState::Template => b'`',
                StringState::None => unreachable!(),
            };
            if b == closer {
                state = StringState::None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => {
                state = StringState::Single;
                out.push(b);
                i += 1;
            }
            b'"' => {
                state = StringState::Double;
                out.push(b);
                i += 1;
            }
            b'`' => {
                state = StringState::Template;
                out.push(b);
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                // Line comment: strip to end of line.
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                // Block comment: strip to the closing `*/` (or slice end).
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            _ if b.is_ascii_whitespace() => {
                i += 1;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

fn write_owner_tag(buf: &mut Vec<u8>, owner: verter_type_expr::TopLevelOwnerId) {
    buf.push(match owner.kind() {
        verter_type_expr::TopLevelOwnerKind::Module => b'M',
        verter_type_expr::TopLevelOwnerKind::Instance => b'I',
        verter_type_expr::TopLevelOwnerKind::Frontmatter => b'F',
    });
    buf.extend_from_slice(&owner.ordinal().to_le_bytes());
}

/// Emit one [`FactKey::DeclContributionOrder`] fact per file-surface
/// declaration slot (type + value headers): the AUTHORED sequence of
/// `(contributor statement index, declaration slice shape)`. An
/// overload-group reorder, a same-file declaration swap (each slot's
/// contributor index moves), a merge-group growth, or a content edit
/// of a contributing declaration moves the fact; comments / whitespace
/// BETWEEN declarations do not.
pub(super) fn emit_decl_contribution_order_facts(
    registry: &mut FactRegistry,
    shallow: &ShallowFileState,
    indexed: &IndexedReady,
) {
    let header_index = shallow.decl_bodies().header_index();
    let mut emit_for =
        |key: &verter_type_expr::DeclBindingKey,
         contributors: &[verter_semantic::analysis::decl_headers::DeclHeaderContributor],
         space: SymbolSpace| {
            let mut buf: Vec<u8> = Vec::with_capacity(64 + 24 * contributors.len());
            buf.extend_from_slice(DECL_CONTRIBUTION_ORDER_SALT);
            write_owner_tag(&mut buf, key.owner);
            buf.push(space.tag());
            buf.extend_from_slice(key.name.as_bytes());
            for contributor in contributors {
                buf.extend_from_slice(&contributor.anchor.contributor_index.to_le_bytes());
                let shape = declaration_slice_shape(
                    indexed.eval_source.as_ref(),
                    contributor.declaration_span,
                );
                buf.extend_from_slice(&shape);
            }
            let hash = xxhash_rust::xxh3::xxh3_128(&buf).to_le_bytes();
            registry.insert(Fact {
                key: FactKey::DeclContributionOrder {
                    name: InternedName::from(key.name.as_ref()),
                    owner: key.owner,
                    space,
                },
                semantic_hash: hash,
                display_hash: hash,
            });
        };
    for (key, header) in header_index.type_headers.iter() {
        emit_for(key, header.contributors.as_slice(), SymbolSpace::Type);
    }
    for (key, header) in header_index.value_headers.iter() {
        emit_for(key, header.contributors.as_slice(), SymbolSpace::Value);
    }
}

/// Emit the per-augmentation-target contribution facts:
/// [`FactKey::AugmentationContributionSet`] (SET shape: one
/// `(name, space, header fingerprint)` triple per contributed SYMBOL —
/// growth or a contribution shape edit moves it) and
/// [`FactKey::AugmentationContributionOrder`] (ORDER: one
/// `(name, space, header fingerprint)` entry per CONTRIBUTOR POSITION —
/// a symbol declared twice in one block occupies TWO entries at their
/// authored positions, so an `A,B,A` → `A,A,B` reorder moves it;
/// positions themselves never enter the hash). Both HEADER-level:
/// the per-contribution shape is the same
/// [`super::augmentation_header_fingerprint`] the `ModuleAugmentation` facts
/// use — no source slicing and no body lowering.
pub(super) fn emit_augmentation_contribution_facts(
    registry: &mut FactRegistry,
    shallow: &ShallowFileState,
) {
    let header_index = shallow.decl_bodies().header_index();

    // (scope-kind tag, specifier, owner, name, space, header
    // fingerprint, declaration position) per contribution record. The
    // SET fact dedupes to one record per symbol; the ORDER fact keeps
    // one record per contributor position (duplicates preserved in
    // authored order).
    struct ContributionRecord {
        scope_kind_tag: verter_semantic::facts::AugmentationScopeKindTag,
        specifier: InternedSpecifier,
        owner: verter_type_expr::TopLevelOwnerId,
        name: String,
        space: SymbolSpace,
        fingerprint: Hash16,
        position: u32,
    }
    let mut set_records_all: Vec<ContributionRecord> = Vec::new();
    let mut order_records_all: Vec<ContributionRecord> = Vec::new();
    let mut collect =
        |scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
         key: &verter_type_expr::DeclBindingKey,
         kind: &str,
         members: &[MemberHeader],
         contributors: &[verter_semantic::analysis::decl_headers::DeclHeaderContributor],
         space: SymbolSpace| {
            let fingerprint = super::augmentation_header_fingerprint(
                scope,
                key.owner,
                key.name.as_ref(),
                kind,
                members,
                contributors.len(),
            );
            for contributor in contributors {
                order_records_all.push(ContributionRecord {
                    scope_kind_tag: scope_kind_tag_for(scope),
                    specifier: specifier_for(scope),
                    owner: key.owner,
                    name: key.name.to_string(),
                    space,
                    fingerprint,
                    position: contributor.declaration_span.start,
                });
            }
            let Some(first_position) = contributors.iter().map(|c| c.declaration_span.start).min()
            else {
                return;
            };
            set_records_all.push(ContributionRecord {
                scope_kind_tag: scope_kind_tag_for(scope),
                specifier: specifier_for(scope),
                owner: key.owner,
                name: key.name.to_string(),
                space,
                fingerprint,
                position: first_position,
            });
        };
    for (scope, names) in &header_index.augmentation_type_headers {
        for (key, header) in names {
            collect(
                scope,
                key,
                format!("{:?}", header.kind).as_str(),
                header.member_headers.as_slice(),
                header.contributors.as_slice(),
                SymbolSpace::Type,
            );
        }
    }
    for (scope, names) in &header_index.augmentation_value_headers {
        for (key, header) in names {
            collect(
                scope,
                key,
                format!("{:?}", header.kind).as_str(),
                header.object_member_headers.as_slice(),
                header.contributors.as_slice(),
                SymbolSpace::Value,
            );
        }
    }

    let targets: FxHashSet<(
        verter_semantic::facts::AugmentationScopeKindTag,
        InternedSpecifier,
        verter_type_expr::TopLevelOwnerId,
    )> = header_index
        .augmentation_blocks
        .iter()
        .map(|block| {
            (
                scope_kind_tag_for(&block.scope),
                specifier_for(&block.scope),
                block.owner,
            )
        })
        .collect();
    // Emit the (possibly EMPTY) contribution set/order facts for EVERY
    // augmentation block from the block inventory — an empty block
    // yields the bare-target hash, so an empty → first-contribution
    // edit moves a pinned hash even when the target itself is unchanged.
    for (scope_kind_tag, specifier, owner) in targets {
        // SET shape — one entry per contributed SYMBOL (duplicates
        // deduped), order-insensitive by construction.
        let mut set_records: Vec<&ContributionRecord> = set_records_all
            .iter()
            .filter(|r| {
                r.scope_kind_tag == scope_kind_tag && r.specifier == specifier && r.owner == owner
            })
            .collect();
        set_records.sort_by(|a, b| {
            (a.name.as_str(), a.space.tag()).cmp(&(b.name.as_str(), b.space.tag()))
        });
        let mut set_buf: Vec<u8> = Vec::with_capacity(64 + 48 * set_records.len());
        set_buf.extend_from_slice(AUG_CONTRIBUTION_SET_SALT);
        set_buf.push(scope_kind_tag.tag());
        set_buf.extend_from_slice(specifier.as_ref().as_bytes());
        write_owner_tag(&mut set_buf, owner);
        for record in &set_records {
            set_buf.push(record.space.tag());
            set_buf.extend_from_slice(record.name.as_bytes());
            set_buf.extend_from_slice(&record.fingerprint);
        }
        let set_hash = xxhash_rust::xxh3::xxh3_128(&set_buf).to_le_bytes();
        registry.insert(Fact {
            key: FactKey::AugmentationContributionSet {
                scope_kind_tag,
                specifier: specifier.clone(),
                owner,
            },
            semantic_hash: set_hash,
            display_hash: set_hash,
        });

        // ORDER — one entry per CONTRIBUTOR POSITION (a symbol declared
        // twice keeps both entries): positions sort the AUTHORED
        // declaration sequence (never an alphabetical tie-break, never a
        // per-symbol collapse). Positions themselves never enter the
        // hash (they are cosmetic-sensitive); only the ordered
        // `(name, space, header fingerprint)` sequence does.
        let mut order_records: Vec<&ContributionRecord> = order_records_all
            .iter()
            .filter(|r| {
                r.scope_kind_tag == scope_kind_tag && r.specifier == specifier && r.owner == owner
            })
            .collect();
        order_records.sort_by(|a, b| {
            (a.position, a.name.as_str(), a.space.tag()).cmp(&(
                b.position,
                b.name.as_str(),
                b.space.tag(),
            ))
        });
        let mut order_buf: Vec<u8> = Vec::with_capacity(64 + 48 * order_records.len());
        order_buf.extend_from_slice(AUG_CONTRIBUTION_ORDER_SALT);
        order_buf.push(scope_kind_tag.tag());
        order_buf.extend_from_slice(specifier.as_ref().as_bytes());
        write_owner_tag(&mut order_buf, owner);
        for record in &order_records {
            order_buf.push(record.space.tag());
            order_buf.extend_from_slice(record.name.as_bytes());
            order_buf.extend_from_slice(&record.fingerprint);
        }
        let order_hash = xxhash_rust::xxh3::xxh3_128(&order_buf).to_le_bytes();
        registry.insert(Fact {
            key: FactKey::AugmentationContributionOrder {
                scope_kind_tag,
                specifier,
                owner,
            },
            semantic_hash: order_hash,
            display_hash: order_hash,
        });
    }
}

/// The scope-kind discriminator of an augmentation block as authored
/// (`declare global` vs `declare module "X"`), mirroring
/// [`specifier_for`]'s specifier encoding — the two TOGETHER form the
/// augmentation target identity (`declare module "$global"` never
/// collides with `declare global`).
fn scope_kind_tag_for(
    scope: &verter_semantic::analysis::type_eval::AugmentationScopeKind,
) -> verter_semantic::facts::AugmentationScopeKindTag {
    use verter_semantic::analysis::type_eval::AugmentationScopeKind;
    match scope {
        AugmentationScopeKind::Global => verter_semantic::facts::AugmentationScopeKindTag::Global,
        AugmentationScopeKind::Module(_) => {
            verter_semantic::facts::AugmentationScopeKindTag::Module
        }
    }
}

/// Emit the whole-file scope-inventory set facts:
/// [`FactKey::AugmentationTargetSet`] — the sorted
/// `(scope-kind tag, specifier, owner)` set of augmentation blocks this
/// file declares (EMPTY blocks included, so a first
/// `declare module "m" {…}` in a file that had none moves the fact even
/// with an unchanged parse-stable skeleton; the scope-kind tag keeps
/// `declare global {…}` and `declare module "$global" {…}` in DISTINCT
/// target identities); and [`FactKey::NamespaceScopeSet`] — the sorted
/// `(owner, qualified name)` set of `namespace N { … }` blocks (EMPTY
/// blocks included — a zero-member block is still a named scope). Both
/// derive from the shallow walk's block inventory (recorded at block
/// entry), never from registered members.
pub(super) fn emit_scope_inventory_facts(registry: &mut FactRegistry, shallow: &ShallowFileState) {
    const AUGMENTATION_TARGET_SET_SALT: &[u8] = b"verter-aug-target-set:v1";
    const NAMESPACE_SCOPE_SET_SALT: &[u8] = b"verter-namespace-scope-set:v1";

    let header_index = shallow.decl_bodies().header_index();

    let mut targets: Vec<(
        verter_semantic::facts::AugmentationScopeKindTag,
        InternedSpecifier,
        verter_type_expr::TopLevelOwnerId,
    )> = header_index
        .augmentation_blocks
        .iter()
        .map(|block| {
            (
                scope_kind_tag_for(&block.scope),
                specifier_for(&block.scope),
                block.owner,
            )
        })
        .collect();
    targets.sort_by(|a, b| (a.0.tag(), a.1.as_ref(), a.2).cmp(&(b.0.tag(), b.1.as_ref(), b.2)));
    targets.dedup();
    let mut target_buf: Vec<u8> = Vec::with_capacity(32 + 24 * targets.len());
    target_buf.extend_from_slice(AUGMENTATION_TARGET_SET_SALT);
    for (scope_kind_tag, specifier, owner) in &targets {
        target_buf.push(scope_kind_tag.tag());
        target_buf.extend_from_slice(specifier.as_ref().as_bytes());
        write_owner_tag(&mut target_buf, *owner);
    }
    let target_hash = xxhash_rust::xxh3::xxh3_128(&target_buf).to_le_bytes();
    registry.insert(Fact {
        key: FactKey::AugmentationTargetSet,
        semantic_hash: target_hash,
        display_hash: target_hash,
    });

    let mut namespaces: Vec<(verter_type_expr::TopLevelOwnerId, &str)> = header_index
        .namespace_blocks
        .iter()
        .map(|block| (block.owner, block.qualified_name.as_str()))
        .collect();
    namespaces.sort();
    namespaces.dedup();
    let mut ns_buf: Vec<u8> = Vec::with_capacity(32 + 24 * namespaces.len());
    ns_buf.extend_from_slice(NAMESPACE_SCOPE_SET_SALT);
    for (owner, qualified_name) in &namespaces {
        write_owner_tag(&mut ns_buf, *owner);
        ns_buf.extend_from_slice(qualified_name.as_bytes());
    }
    let ns_hash = xxhash_rust::xxh3::xxh3_128(&ns_buf).to_le_bytes();
    registry.insert(Fact {
        key: FactKey::NamespaceScopeSet,
        semantic_hash: ns_hash,
        display_hash: ns_hash,
    });
}
