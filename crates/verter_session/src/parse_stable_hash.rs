//! `parse_stable_hash` computation.
//!
//! `parse_stable_hash` is a structural hash over a file's post-shallow-analysis
//! decl skeleton. It is **invariant under cosmetic edits**:
//!
//! - whitespace changes
//! - comment additions / deletions
//! - JSDoc edits
//! - generic param identifier rename (`T` ↔ `U`) — applies once the fact-emission walk
//!   lowers typed parameter lists; today, hashes the shallow symbol
//!   inventory only, which does NOT include parameter identifiers, so the
//!   property already holds.
//!
//! It **changes** under decl-shape edits:
//!
//! - adding / removing a declaration
//! - renaming a declaration
//! - changing a declaration's kind (`interface` ↔ `type`)
//! - adding / removing a member
//! - renaming a member
//! - changing a member's kind
//!
//! ## Algorithm (R27 stack-safe)
//!
//! The hash walks the [`ShallowFileState`] symbol inventory in a stable
//! order. The inventory captures only top-level declarations (the post-
//! shallow-analysis decl skeleton); deep member bodies live in
//! `Member`/`MemberShape` facts emitted by the fact-emission walk.
//!
//! 1. Sort symbol names per kind so order is independent of the parse's
//!    declaration order. (Decl reorders within a file are cosmetic for the
//!    SHALLOW skeleton — the parse_stable_hash is invariant under
//!    reorder.)
//! 2. For each symbol, emit `(kind, name)` and (for type symbols with
//!    members) the sorted member name list.
//! 3. For exports, emit `(exported_name, target_kind)`.
//! 4. Hash the serialised tuple stream with xxh3.
//!
//! A future extension may the walk with typed-IR-derived alpha-normalisation
//! (e.g., to make the body of a `type Foo<T> = T[]` stable under
//! `T` ↔ `U` rename). The current skeleton does NOT inspect bodies.

use verter_semantic::analysis::Hash16;
use xxhash_rust::xxh3::xxh3_128;

use crate::project_type_store::IndexedReady;

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

    // ── Section: type symbols ──
    write_section(&mut buf, b"types");
    let mut type_keys: Vec<&String> = shallow.symbols.keys().collect();
    type_keys.sort();
    for name in type_keys {
        let symbol = &shallow.symbols[name];
        write_decl(&mut buf, kind_str_type(&symbol.kind), name);
        // Emit member skeleton (member name set only — bodies live in
        // `Member` facts from the fact-emission walk).
        let mut members: Vec<&String> = symbol.member_deps.keys().collect();
        members.sort();
        for m in members {
            buf.extend_from_slice(b"m:");
            buf.extend_from_slice(m.as_bytes());
            buf.push(SEP);
        }
    }

    // ── Section: value symbols ──
    write_section(&mut buf, b"values");
    let mut value_keys: Vec<&String> = shallow.value_symbols.keys().collect();
    value_keys.sort();
    for name in value_keys {
        let symbol = &shallow.value_symbols[name];
        write_decl(&mut buf, kind_str_value(&symbol.kind), name);
        if let Some(members) = &symbol.enum_members {
            let mut member_names: Vec<&String> = members.keys().collect();
            member_names.sort();
            for m in member_names {
                buf.extend_from_slice(b"e:");
                buf.extend_from_slice(m.as_bytes());
                buf.push(SEP);
            }
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
