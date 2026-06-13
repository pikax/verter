//! `parse_stable_hash` computation.
//!
//! `parse_stable_hash` is a structural hash over a file's post-shallow-analysis
//! decl skeleton. It is **invariant under cosmetic edits**:
//!
//! - whitespace changes
//! - comment additions / deletions
//! - JSDoc edits
//! - generic param IDENTIFIER rename (`T` ↔ `U`) — the hash folds a type
//!   parameter's ARITY and the presence of its constraint / default
//!   clauses, never the parameter identifier, so an alpha-rename is
//!   invariant.
//! - a member's VALUE-type edit (`x: string` ↔ `x: number`) — the
//!   member's body type is lowered on demand, never at publish, so it is
//!   not part of this skeleton hash (body sensitivity rides the
//!   `FileWholeHash` rail).
//!
//! It **changes** under decl-shape edits:
//!
//! - adding / removing a declaration
//! - renaming a declaration
//! - changing a declaration's kind (`interface` ↔ `type`)
//! - adding / removing / renaming a member
//! - changing a member's kind (property ↔ method), `optional`, or
//!   `readonly` flag
//! - changing a type parameter's ARITY (count) or the presence of its
//!   constraint / default clause
//! - splitting / merging a same-name declaration (the source-order
//!   CONTRIBUTOR COUNT changes even when the unioned member set does not)
//! - adding / removing / renaming an object-literal value member, or
//!   changing its header flags
//!
//! ## Algorithm (R27 stack-safe)
//!
//! The hash walks the [`ShallowFileState`] header inventory in a stable
//! order. The inventory captures only top-level declaration HEADERS (the
//! post-shallow-analysis decl skeleton); member VALUE types and other
//! body data live in `Member`/`MemberShape` facts lowered on demand.
//!
//! 1. Sort symbol names per kind so order is independent of the parse's
//!    declaration order. (Decl reorders within a file are cosmetic for the
//!    SHALLOW skeleton — the parse_stable_hash is invariant under
//!    reorder.)
//! 2. For each TYPE symbol, emit `(kind, name)`, the type-parameter shape
//!    (arity + per-param constraint/default presence, IN ORDER, never the
//!    identifier), the contributor count, and the name-sorted member
//!    headers `(name, kind, optional, readonly)`.
//! 3. For each VALUE symbol, emit `(kind, name)`, the contributor count,
//!    and the name-sorted object-member headers `(name, kind, optional,
//!    readonly)`.
//! 4. For exports, emit `(exported_name, target_kind)`.
//! 5. Hash the serialised tuple stream with xxh3.
//!
//! The skeleton folds header SHAPE only — it never inspects declaration
//! bodies (no member value types, no lowered clauses).

use verter_semantic::analysis::decl_headers::{MemberHeader, MemberHeaderKind};
use verter_semantic::analysis::Hash16;
use xxhash_rust::xxh3::xxh3_128;

use crate::project_type_store::IndexedReady;

#[cfg(test)]
#[path = "parse_stable_hash_tests.rs"]
mod parse_stable_hash_tests;

const SALT: &[u8] = b"verter-parse-stable-hash:v1";
const SEP: u8 = 0u8;

/// Compute the `parse_stable_hash` for the given canonical
/// [`IndexedReady`] artifact.
///
/// Invariant under cosmetic edits; changes under decl-shape edits.
#[must_use]
pub fn compute_parse_stable_hash(indexed: &IndexedReady) -> Hash16 {
    let shallow = &indexed.shallow_state;

    let mut buf: Vec<u8> = Vec::with_capacity(256);
    buf.extend_from_slice(SALT);
    buf.push(SEP);

    // ── Section: type symbols (HEADER inventory — no body lowering) ──
    write_section(&mut buf, b"types");
    let mut type_keys: Vec<&str> = shallow.type_symbol_names().collect();
    type_keys.sort_unstable();
    for name in type_keys {
        let Some(kind) = shallow.type_symbol_kind(name) else {
            continue;
        };
        write_decl(&mut buf, kind_str_type(&kind), name);
        // Type-parameter SHAPE: arity (count) + per-parameter
        // constraint/default-clause presence, IN DECLARATION ORDER. The
        // parameter IDENTIFIER is deliberately NOT folded so an
        // alpha-rename (`T` ↔ `U`) stays invariant; arity + clause
        // presence are semantic-shape signals that DO move the hash.
        if let Some(params) = shallow.type_param_headers(name) {
            buf.extend_from_slice(b"tp:");
            buf.extend_from_slice(&(params.len() as u32).to_le_bytes());
            for param in params {
                buf.push(u8::from(param.constraint_span.is_some()));
                buf.push(u8::from(param.default_span.is_some()));
            }
            buf.push(SEP);
        }
        // Contributor count: a same-name decl split / merge changes the
        // number of contributing top-level statements even when the
        // unioned member set is unchanged.
        if let Some(count) = shallow.type_contributor_count(name) {
            buf.extend_from_slice(b"tc:");
            buf.extend_from_slice(&(count as u32).to_le_bytes());
            buf.push(SEP);
        }
        // Emit member skeleton: name + header SHAPE (kind, optional,
        // readonly). Member VALUE types are body data, lowered on demand
        // — NOT folded here. Sorted by name so source order is cosmetic.
        let mut members: Vec<&MemberHeader> = shallow
            .type_member_headers(name)
            .map(|headers| headers.iter().collect())
            .unwrap_or_default();
        members.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        for m in members {
            write_member_header(&mut buf, m);
        }
    }

    // ── Section: value symbols (HEADER inventory) ──
    write_section(&mut buf, b"values");
    let mut value_keys: Vec<&str> = shallow.value_symbol_names().collect();
    value_keys.sort_unstable();
    for name in value_keys {
        let Some(kind) = shallow.value_symbol_kind(name) else {
            continue;
        };
        write_decl(&mut buf, kind_str_value(&kind), name);
        // Contributor count (same-name decl split / merge).
        if let Some(count) = shallow.value_contributor_count(name) {
            buf.extend_from_slice(b"vc:");
            buf.extend_from_slice(&(count as u32).to_le_bytes());
            buf.push(SEP);
        }
        // Object-literal / class-static member headers: name + header
        // SHAPE (kind, optional, readonly). The old hash folded NOTHING
        // about value members, so an object-member add/remove/rename or
        // header-flag change was invisible. Sorted by name (source order
        // cosmetic).
        let mut members: Vec<&MemberHeader> = shallow
            .value_object_member_headers(name)
            .map(|headers| headers.iter().collect())
            .unwrap_or_default();
        members.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        for m in members {
            write_member_header(&mut buf, m);
        }
    }

    // ── Section: enum symbols (HEADER inventory — member NAMES only) ──
    // Enums live in their own header table (the eval-env walk never
    // registers them as value symbols), so they must be folded in
    // explicitly: a variant add/rename/remove is a decl-shape edit that
    // MUST move this hash. Member order is preserved (auto-increment enum
    // values are positional, so a reorder is a semantic change).
    write_section(&mut buf, b"enums");
    let mut enum_keys: Vec<&str> = shallow.enum_symbol_names().collect();
    enum_keys.sort_unstable();
    for name in enum_keys {
        write_decl(&mut buf, "enum", name);
        let members = shallow.enum_member_names(name).unwrap_or_default();
        for m in members {
            buf.extend_from_slice(b"e:");
            buf.extend_from_slice(m.as_bytes());
            buf.push(SEP);
        }
    }

    // ── Section: exports ──
    write_section(&mut buf, b"exports");
    let mut export_keys: Vec<&String> = shallow.exports.keys().collect();
    export_keys.sort();
    for name in export_keys {
        let target = &shallow.exports[name];
        buf.extend_from_slice(name.as_bytes());
        buf.push(SEP);
        write_export_target(&mut buf, target);
    }

    // ── Section: wildcard re-exports ──
    write_section(&mut buf, b"wildcard_reexports");
    let mut wildcards: Vec<&str> = shallow
        .wildcard_reexports
        .iter()
        .map(|w| w.source_specifier.as_str())
        .collect();
    wildcards.sort();
    for specifier in wildcards {
        buf.extend_from_slice(specifier.as_bytes());
        buf.push(SEP);
    }

    // ── Section: import targets (specifier + binding, NOT resolved canonical) ──
    // R12: parse-domain emits import shape only — resolved targets live in
    // the resolve-domain. The shallow state today carries the
    // resolved canonical alongside, but `parse_stable_hash` reads only the
    // specifier + binding so a resolve-config change (paths edit) does not
    // ripple through this hash.
    write_section(&mut buf, b"import_targets");
    let mut import_keys: Vec<&String> = shallow.import_targets.keys().collect();
    import_keys.sort();
    for local in import_keys {
        let target = &shallow.import_targets[local];
        buf.extend_from_slice(local.as_bytes());
        buf.push(SEP);
        buf.extend_from_slice(target.source_specifier.as_bytes());
        buf.push(SEP);
        buf.extend_from_slice(target.imported_name.as_bytes());
        buf.push(SEP);
        // Resolved canonical_id is INTENTIONALLY excluded (R12).
    }

    xxh3_128(&buf).to_le_bytes()
}

fn write_section(buf: &mut Vec<u8>, name: &[u8]) {
    buf.push(SEP);
    buf.extend_from_slice(name);
    buf.push(SEP);
}

fn write_decl(buf: &mut Vec<u8>, kind: &str, name: &str) {
    buf.extend_from_slice(kind.as_bytes());
    buf.push(b':');
    buf.extend_from_slice(name.as_bytes());
    buf.push(SEP);
}

/// Emit one member header's SHAPE: name + kind tag + optional/readonly
/// flags. Member VALUE types are body data and are NOT folded.
fn write_member_header(buf: &mut Vec<u8>, member: &MemberHeader) {
    buf.extend_from_slice(b"m:");
    buf.extend_from_slice(member.name.as_bytes());
    buf.push(SEP);
    buf.push(match member.kind {
        MemberHeaderKind::Property => b'p',
        MemberHeaderKind::Method => b'm',
    });
    buf.push(u8::from(member.optional));
    buf.push(u8::from(member.readonly));
}

fn write_export_target(
    buf: &mut Vec<u8>,
    target: &crate::resolver_core::shallow_file_state::ExportTarget,
) {
    use crate::resolver_core::shallow_file_state::ExportTarget;
    match target {
        ExportTarget::Local { symbol_name } => {
            buf.push(b'L');
            buf.extend_from_slice(symbol_name.as_bytes());
            buf.push(SEP);
        }
        ExportTarget::Reexport {
            source_specifier,
            original_name,
            // Resolved canonical_id is INTENTIONALLY excluded (R12 — parse
            // domain emits syntactic shape only).
            canonical_id: _,
            is_type,
        } => {
            buf.push(b'R');
            buf.push(if *is_type { b'T' } else { b'V' });
            buf.extend_from_slice(source_specifier.as_bytes());
            buf.push(SEP);
            buf.extend_from_slice(original_name.as_bytes());
            buf.push(SEP);
        }
    }
}

fn kind_str_type(kind: &verter_semantic::analysis::type_eval::TypeDeclKind) -> &'static str {
    use verter_semantic::analysis::type_eval::TypeDeclKind;
    match kind {
        TypeDeclKind::Alias => "type",
        TypeDeclKind::Interface => "interface",
        TypeDeclKind::Class => "class",
    }
}

fn kind_str_value(kind: &verter_semantic::analysis::type_eval::ValueDeclKind) -> &'static str {
    use verter_semantic::analysis::type_eval::ValueDeclKind;
    match kind {
        ValueDeclKind::Var => "var",
        ValueDeclKind::Let => "let",
        ValueDeclKind::Const => "const",
        ValueDeclKind::Function => "fn",
        ValueDeclKind::AsyncFunction => "async_fn",
        ValueDeclKind::Class => "class_v",
        ValueDeclKind::Enum => "enum_v",
    }
}
