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
//! - changing an embedded script region's owner kind or source dialect
//!   (`<script>` ↔ `<script setup>` is therefore shape-significant even
//!   when the script bytes are identical)
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
    let headers = shallow.decl_bodies().header_index();
    let mut type_keys: Vec<_> = headers.type_headers.keys().collect();
    type_keys.sort_unstable();
    for key in type_keys {
        let header = &headers.type_headers[key];
        write_owner(&mut buf, key.owner);
        write_decl(&mut buf, kind_str_type(&header.kind), key.name.as_ref());
        // Type-parameter SHAPE: arity (count) + per-parameter
        // constraint/default-clause presence, IN DECLARATION ORDER. The
        // parameter IDENTIFIER is deliberately NOT folded so an
        // alpha-rename (`T` ↔ `U`) stays invariant; arity + clause
        // presence are semantic-shape signals that DO move the hash.
        {
            let params = &header.type_params;
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
        {
            let count = header.contributors.len();
            buf.extend_from_slice(b"tc:");
            buf.extend_from_slice(&(count as u32).to_le_bytes());
            buf.push(SEP);
        }
        // Emit member skeleton: name + header SHAPE (kind, optional,
        // readonly). Member VALUE types are body data, lowered on demand
        // — NOT folded here. Sorted by name so source order is cosmetic.
        let mut members: Vec<&MemberHeader> = header.member_headers.iter().collect();
        members.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        for m in members {
            write_member_header(&mut buf, m);
        }
    }

    // ── Section: value symbols (HEADER inventory) ──
    write_section(&mut buf, b"values");
    let mut value_keys: Vec<_> = headers.value_headers.keys().collect();
    value_keys.sort_unstable();
    for key in value_keys {
        let header = &headers.value_headers[key];
        write_owner(&mut buf, key.owner);
        write_decl(&mut buf, kind_str_value(&header.kind), key.name.as_ref());
        // Contributor count (same-name decl split / merge).
        {
            let count = header.contributors.len();
            buf.extend_from_slice(b"vc:");
            buf.extend_from_slice(&(count as u32).to_le_bytes());
            buf.push(SEP);
        }
        // Object-literal / class-static member headers: name + header
        // SHAPE (kind, optional, readonly). The old hash folded NOTHING
        // about value members, so an object-member add/remove/rename or
        // header-flag change was invisible. Sorted by name (source order
        // cosmetic).
        let mut members: Vec<&MemberHeader> = header.object_member_headers.iter().collect();
        members.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        for m in members {
            write_member_header(&mut buf, m);
        }
    }

    // ── Section: enum symbols (HEADER inventory — member NAMES only) ──
    // An enum is registered dual-space (it ALSO appears as an `enum_v` value
    // symbol in the section above), but the dedicated enum header table is
    // the sole authority for the member (variant) NAMES — the type/value
    // headers do not carry them — so they are folded in explicitly here: a
    // variant add/rename/remove is a decl-shape edit that MUST move this
    // hash. Member order is preserved (auto-increment enum values are
    // positional, so a reorder is a semantic change). Member VALUES are body
    // data (the value-body fact + `FileWholeHash` rail), deliberately not
    // folded into this skeleton hash.
    write_section(&mut buf, b"enums");
    let mut enum_keys: Vec<_> = headers.enum_headers.keys().collect();
    enum_keys.sort_unstable();
    for key in enum_keys {
        let header = &headers.enum_headers[key];
        write_owner(&mut buf, key.owner);
        write_decl(&mut buf, "enum", key.name.as_ref());
        for m in &header.member_names {
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
    let mut wildcards: Vec<_> = shallow.wildcard_reexports.iter().collect();
    wildcards.sort_by_key(|wildcard| (wildcard.owner, wildcard.source_specifier.as_str()));
    for wildcard in wildcards {
        write_owner(&mut buf, wildcard.owner);
        buf.extend_from_slice(wildcard.source_specifier.as_bytes());
        buf.push(SEP);
    }

    // ── Section: import targets (specifier + binding, NOT resolved canonical) ──
    // R12: parse-domain emits import shape only — resolved targets live in
    // the resolve-domain. The shallow state today carries the
    // resolved canonical alongside, but `parse_stable_hash` reads only the
    // specifier + binding so a resolve-config change (paths edit) does not
    // ripple through this hash.
    write_section(&mut buf, b"import_targets");
    let mut import_keys: Vec<_> = shallow.owner_import_targets.keys().collect();
    import_keys.sort();
    for local in import_keys {
        let target = &shallow.owner_import_targets[local];
        write_owner(&mut buf, local.owner);
        buf.extend_from_slice(local.name.as_bytes());
        buf.push(SEP);
        buf.extend_from_slice(target.source_specifier.as_bytes());
        buf.push(SEP);
        buf.extend_from_slice(target.imported_name.as_bytes());
        buf.push(SEP);
        buf.push(u8::from(target.is_namespace));
        buf.push(SEP);
        // Resolved canonical_id is INTENTIONALLY excluded (R12).
    }

    // ── Section: carrier script-region shape ──
    // A carrier's typed region inventory is the parse authority for owner
    // identity. In particular, adding/removing Vue's `setup` attribute can
    // leave the embedded script bytes and every declaration header unchanged
    // while moving all declarations between Module(0) and Instance(0). Fold
    // the region KIND and source dialect, but not byte spans: offsets move
    // under cosmetic carrier edits and are not semantic shape.
    write_section(&mut buf, b"carrier_script_regions");
    if let Some(parse) = indexed.framework_parse.as_deref() {
        for region in &parse.common.script_regions {
            write_script_region_kind(&mut buf, region.kind);
            write_script_source_type(&mut buf, region.source_type);
            buf.push(SEP);
        }
    }

    xxh3_128(&buf).to_le_bytes()
}

fn write_script_region_kind(buf: &mut Vec<u8>, kind: verter_language::ScriptRegionKind) {
    use verter_language::ScriptRegionKind;
    buf.push(match kind {
        ScriptRegionKind::Instance => b'I',
        ScriptRegionKind::Module => b'M',
        ScriptRegionKind::Frontmatter => b'F',
    });
}

fn write_script_source_type(buf: &mut Vec<u8>, source_type: verter_language::ScriptSourceType) {
    use verter_language::{JsModuleKind, ScriptSourceType};

    let (dialect, module_kind) = match source_type {
        ScriptSourceType::Ts => (b'T', None),
        ScriptSourceType::Tsx => (b'X', None),
        ScriptSourceType::Dts => (b'D', None),
        ScriptSourceType::Js(kind) => (b'J', Some(kind)),
        ScriptSourceType::Jsx(kind) => (b'R', Some(kind)),
    };
    buf.push(dialect);
    if let Some(module_kind) = module_kind {
        buf.push(match module_kind {
            JsModuleKind::Unambiguous => b'U',
            JsModuleKind::Module => b'M',
            JsModuleKind::CommonJs => b'C',
            JsModuleKind::Script => b'S',
        });
    }
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

fn write_owner(buf: &mut Vec<u8>, owner: verter_type_expr::TopLevelOwnerId) {
    buf.push(match owner.kind() {
        verter_type_expr::TopLevelOwnerKind::Module => b'M',
        verter_type_expr::TopLevelOwnerKind::Instance => b'I',
        verter_type_expr::TopLevelOwnerKind::Frontmatter => b'F',
    });
    buf.extend_from_slice(&owner.ordinal().to_le_bytes());
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
        ExportTarget::Local { owner, symbol_name } => {
            buf.push(b'L');
            write_owner(buf, *owner);
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
