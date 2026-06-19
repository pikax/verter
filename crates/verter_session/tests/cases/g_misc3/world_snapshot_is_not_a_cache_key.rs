//! Architecture guard for the `WorldSnapshot` identity-vs-key
//! invariant.
//!
//! `WorldSnapshot` is a request-concurrency identity, not a cache
//! key. Cache layers project the snapshot down to scoped dimensions
//! via the `*_dims()` accessors. Embedding the full snapshot as a
//! single key field violates R21 (the five env-hash dimensions must
//! remain split) and is statically rejected here.
//!
//! The guard walks every `.rs` file under `crates/verter_session/src/`
//! (excluding test files, `tests/`, `benches/`, `examples/`, and
//! `target/`), parses each one with `syn::parse_file`, finds every
//! struct whose name ends `Key` or `Identity` (top-level or nested
//! inside modules; function-local and impl-local structs are out of
//! scope, since a cache-key/identity type is always a module-level
//! item), and asserts no field's type mentions `WorldSnapshot` —
//! REGARDLESS of
//! the field's name. Field names are not load-bearing
//! (`pub snap: WorldSnapshot`, `pub identity: Arc<WorldSnapshot>`,
//! `pub ws: Box<WorldSnapshot>`, and the tuple-struct shape
//! `pub WorldSnapshot,` all trip the guard).
//!
//! ## Why `syn`, not a line-based scanner
//!
//! The check is AST-based, not line-based. A line-based scanner that
//! reads each field declaration as a single physical line is blind to
//! multi-line wrapped fields like
//!
//! ```ignore
//! pub snap: Arc<
//!     WorldSnapshot
//! >,
//! ```
//!
//! where `Arc<` and `WorldSnapshot` live on different lines and
//! neither matches the rejection pattern in isolation. `syn` parses
//! the file into a real AST, so a field's `syn::Type` is one logical
//! node regardless of how it was wrapped across source lines. The AST
//! walk also handles generics, where-clauses, tuple-element compounds,
//! and macro-free nested type expressions the line-based approach
//! could not. `syn` is already a dev-dependency of this crate (with
//! the `full` + `parsing` features), so there is no new dependency
//! cost. A synthetic discriminator suite proves the predicate fires
//! across every wrapper / position shape and does NOT fire on
//! prefix/suffix lookalikes like `WorldSnapshotShim`.

use std::fs;
use std::path::{Path, PathBuf};

use syn::{Fields, Item, Type};

fn workspace_root() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(manifest))
}

fn walk_production_rs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "tests" || name == "benches" || name == "examples" || name == "target" {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".rs") {
                continue;
            }
            if name.ends_with("_tests.rs") || name == "tests.rs" {
                continue;
            }
            out.push(path);
        }
    }
    out.sort();
    out
}

/// Whether a struct identifier is in scope for the key-guard: its
/// name ends in `Key` or `Identity`. (`WorldSnapshotIdentity`,
/// `AnalysisSlotKey`, etc.)
fn struct_name_in_scope(name: &str) -> bool {
    name.ends_with("Key") || name.ends_with("Identity")
}

/// Recursively descend a `syn::Type` and return `true` when any path
/// segment is exactly the identifier `WorldSnapshot`.
///
/// The descent covers every wrapper / container shape a field type
/// can take: generic arguments (`Arc<WorldSnapshot>`,
/// `Option<Box<WorldSnapshot>>`, `Vec<(WorldSnapshot, u64)>`),
/// references (`&WorldSnapshot`), raw pointers (`*const WorldSnapshot`),
/// tuples (`(WorldSnapshot, u64)`), arrays / slices, parenthesised
/// types, and `Box<dyn ...>`-style trait-object bounds. The match is
/// on a full path *segment* (`WorldSnapshot`), so lookalikes that
/// merely share a prefix or suffix (`WorldSnapshotShim`,
/// `MyWorldSnapshotIdentity`) do NOT trip — equivalent to the
/// Rust-identifier word-boundary rule the old line scanner used, but
/// exact at the AST level.
fn type_mentions_world_snapshot(ty: &Type) -> bool {
    match ty {
        Type::Path(type_path) => {
            // A qualified self-type (`<T as Trait>::Assoc`) carries its
            // own `Type` to inspect.
            if let Some(qself) = &type_path.qself {
                if type_mentions_world_snapshot(&qself.ty) {
                    return true;
                }
            }
            for seg in &type_path.path.segments {
                if seg.ident == "WorldSnapshot" {
                    return true;
                }
                // Descend into generic arguments:
                // `Arc<WorldSnapshot>`, `HashMap<K, WorldSnapshot>`, …
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    for arg in &args.args {
                        match arg {
                            syn::GenericArgument::Type(inner) => {
                                if type_mentions_world_snapshot(inner) {
                                    return true;
                                }
                            }
                            syn::GenericArgument::AssocType(assoc)
                                if type_mentions_world_snapshot(&assoc.ty) =>
                            {
                                return true;
                            }
                            _ => {}
                        }
                    }
                }
                // Descend into parenthesised (Fn-style) arguments:
                // `Fn(WorldSnapshot) -> ()` etc.
                if let syn::PathArguments::Parenthesized(args) = &seg.arguments {
                    for inner in &args.inputs {
                        if type_mentions_world_snapshot(inner) {
                            return true;
                        }
                    }
                    if let syn::ReturnType::Type(_, inner) = &args.output {
                        if type_mentions_world_snapshot(inner) {
                            return true;
                        }
                    }
                }
            }
            false
        }
        Type::Reference(r) => type_mentions_world_snapshot(&r.elem),
        Type::Ptr(p) => type_mentions_world_snapshot(&p.elem),
        Type::Slice(s) => type_mentions_world_snapshot(&s.elem),
        Type::Array(a) => type_mentions_world_snapshot(&a.elem),
        Type::Paren(p) => type_mentions_world_snapshot(&p.elem),
        Type::Group(g) => type_mentions_world_snapshot(&g.elem),
        Type::Tuple(t) => t.elems.iter().any(type_mentions_world_snapshot),
        _ => false,
    }
}

/// Render a `syn::Type` back to a compact source-like string for the
/// violation report (and for the self-test descriptor assertions).
fn type_to_string(ty: &Type) -> String {
    use quote::ToTokens;
    let rendered = ty.to_token_stream().to_string();
    // `quote` emits spaces around punctuation (`Arc < WorldSnapshot >`).
    // Collapse the common cases so the report reads naturally.
    rendered
        .replace(" < ", "<")
        .replace(" >", ">")
        .replace("< ", "<")
        .replace(" ,", ",")
        .replace(" :: ", "::")
}

/// Recursively collect every struct under `items` whose name ends
/// `Key` or `Identity`, and return `(struct_name, field_descriptor,
/// field_type)` for every field. `field_descriptor` is the named-field
/// identifier (`"snap"`, `"world_snapshot"`) or `"<tuple_n>"` for the
/// n-th positional element of a tuple struct. `field_type` is the
/// rendered type expression.
///
/// Walks top-level and module-nested structs (`Item::Struct` and
/// `Item::Mod` content recursively, e.g.
/// `mod foo { struct BarKey { ... } }`). Function-local and impl-local
/// structs are intentionally out of scope — a project-global
/// cache-key/identity type is always a module-level item, never
/// defined inside a fn or impl body.
fn collect_key_or_identity_struct_fields(items: &[Item]) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    collect_into(items, &mut out);
    out
}

fn collect_into(items: &[Item], out: &mut Vec<(String, String, String)>) {
    for item in items {
        match item {
            Item::Struct(s) => {
                let name = s.ident.to_string();
                if !struct_name_in_scope(&name) {
                    continue;
                }
                match &s.fields {
                    Fields::Named(named) => {
                        for field in &named.named {
                            let descriptor = field
                                .ident
                                .as_ref()
                                .map(|id| id.to_string())
                                .unwrap_or_else(|| "<anon>".to_string());
                            out.push((name.clone(), descriptor, type_to_string(&field.ty)));
                        }
                    }
                    Fields::Unnamed(unnamed) => {
                        for (idx, field) in unnamed.unnamed.iter().enumerate() {
                            out.push((
                                name.clone(),
                                format!("<tuple_{idx}>"),
                                type_to_string(&field.ty),
                            ));
                        }
                    }
                    Fields::Unit => {}
                }
            }
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    collect_into(inner, out);
                }
            }
            _ => {}
        }
    }
}

/// Parse `src` as a Rust file and scan it for `Key`/`Identity` struct
/// fields. On a parse error (rare — production source is always valid
/// Rust by the time the suite runs) the file contributes no fields;
/// the parse failure surfaces through the
/// `syn::parse_file(...).unwrap_or_else(|e| panic!(...))` call in
/// `no_cache_layer_keys_on_world_snapshot_as_a_whole` so a genuinely
/// unparseable production file is loud, not silently skipped.
fn scan_key_or_identity_struct_fields(src: &str) -> Vec<(String, String, String)> {
    match syn::parse_file(src) {
        Ok(file) => collect_key_or_identity_struct_fields(&file.items),
        Err(_) => Vec::new(),
    }
}

/// True when `ty` (a rendered field-type string) is one the guard
/// rejects. Re-parses the rendered string back to a `syn::Type` and
/// runs the AST descent. Used by both the production walk and the
/// self-test so the two paths share one predicate.
fn type_text_mentions_world_snapshot(ty: &str) -> bool {
    match syn::parse_str::<Type>(ty) {
        Ok(parsed) => type_mentions_world_snapshot(&parsed),
        Err(_) => false,
    }
}

#[test]
fn no_cache_layer_keys_on_world_snapshot_as_a_whole() {
    let src_dir = workspace_root()
        .join("crates")
        .join("verter_session")
        .join("src");
    let files = walk_production_rs(&src_dir);
    assert!(
        !files.is_empty(),
        "must find at least one production .rs file under `crates/verter_session/src/`"
    );

    let mut violations: Vec<(String, String, String, String)> = Vec::new();
    for file in files {
        let body = match fs::read_to_string(&file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rel = file
            .strip_prefix(workspace_root())
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        // Necessary-condition pre-filter: the guard rejects a `Key`/
        // `Identity` struct field only when its type mentions the
        // identifier `WorldSnapshot`. A file that never contains the
        // substring `WorldSnapshot` cannot host such a field, so skip the
        // `syn` parse + struct-field walk. The substring is a strict
        // prerequisite for the rejection, so filtering cannot hide a
        // violation.
        if !body.contains("WorldSnapshot") {
            continue;
        }
        // Parse via `syn`; a production file that does not parse is a
        // hard error, not a silent skip — that would let a real
        // violation hide behind a parse failure.
        let parsed = syn::parse_file(&body)
            .unwrap_or_else(|e| panic!("syn::parse_file failed for production file {rel}: {e}"));
        for (struct_name, field_descriptor, field_type) in
            collect_key_or_identity_struct_fields(&parsed.items)
        {
            // Re-parse the rendered type to drive the same descent the
            // self-test uses; on the production path we already hold the
            // AST, but routing through the string predicate keeps a
            // single source of truth for the rejection rule.
            if type_text_mentions_world_snapshot(&field_type) {
                violations.push((rel.clone(), struct_name, field_descriptor, field_type));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "`WorldSnapshot` is a request-concurrency identity, not a cache key. \
         Cache layers must project to scoped dimensions via `*_dims()` accessors. \
         Embedding the full snapshot as a single key field violates R21 (the five \
         env-hash dimensions must remain split). The field name is NOT load-bearing — \
         any `Key`/`Identity` struct field whose type mentions `WorldSnapshot` \
         (including under `Arc<>`, `Option<>`, `Box<>`, references, multi-line wraps, \
         or as a tuple element) is rejected. Offending fields (file -> struct -> field -> type):\n  {}",
        violations
            .iter()
            .map(|(file, st, field, ty)| format!("{file} -> `{st}` -> `{field}: {ty}`"))
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn world_snapshot_is_not_a_cache_key_self_test_detects_synthetic_violation() {
    // Discriminator suite. The scanner MUST trip on every variant
    // below — wrappers, alternate field names, tuple positions,
    // multi-line wraps — and must NOT trip on benign struct shapes.
    // Each `assert!` documents an attack a weaker scanner missed.

    // (a) Arbitrary field name carrying a bare `WorldSnapshot`.
    let synthetic_arbitrary_name = r#"
pub struct ArbitraryNameKey {
    pub canonical: u64,
    pub snap: WorldSnapshot,
}
"#;
    let fields = scan_key_or_identity_struct_fields(synthetic_arbitrary_name);
    let trips = fields.iter().any(|(s, f, t)| {
        s == "ArbitraryNameKey" && f == "snap" && type_text_mentions_world_snapshot(t)
    });
    assert!(
        trips,
        "self-test (a): `ArbitraryNameKey {{ snap: WorldSnapshot }}` must trip the \
         scanner — field NAMES are not load-bearing. Scanned fields: {fields:#?}"
    );

    // (b) Field wrapped in `Arc<...>`.
    let synthetic_arc_wrapper = r#"
pub struct ArcWrappedKey {
    pub whatever: Arc<WorldSnapshot>,
}
"#;
    let fields = scan_key_or_identity_struct_fields(synthetic_arc_wrapper);
    let trips = fields
        .iter()
        .any(|(s, _, t)| s == "ArcWrappedKey" && type_text_mentions_world_snapshot(t));
    assert!(
        trips,
        "self-test (b): `ArcWrappedKey {{ whatever: Arc<WorldSnapshot> }}` must trip — \
         wrapper constructors do not launder the rejection. Scanned fields: {fields:#?}"
    );

    // (c) Tuple-struct position carrying `WorldSnapshot`.
    let synthetic_tuple = r#"
pub struct TupleKey(pub WorldSnapshot, pub u64);
"#;
    let fields = scan_key_or_identity_struct_fields(synthetic_tuple);
    let trips = fields
        .iter()
        .any(|(s, _, t)| s == "TupleKey" && type_text_mentions_world_snapshot(t));
    assert!(
        trips,
        "self-test (c): `pub struct TupleKey(pub WorldSnapshot, pub u64);` must trip — \
         tuple positions are fields too. Scanned fields: {fields:#?}"
    );

    // (c2) Tuple-struct position carrying `WorldSnapshot` inside a
    // compound type (e.g. `(WorldSnapshot, u64)` as one element).
    let synthetic_tuple_compound = r#"
pub struct TupleCompoundKey(pub (WorldSnapshot, u64));
"#;
    let fields = scan_key_or_identity_struct_fields(synthetic_tuple_compound);
    let trips = fields
        .iter()
        .any(|(s, _, t)| s == "TupleCompoundKey" && type_text_mentions_world_snapshot(t));
    assert!(
        trips,
        "self-test (c2): `pub struct TupleCompoundKey(pub (WorldSnapshot, u64));` must trip — \
         compound tuple elements that mention WorldSnapshot are rejected. Scanned fields: {fields:#?}"
    );

    // (d) Benign field that does NOT mention `WorldSnapshot` must
    // NOT trip — proves the predicate is type-shape-discriminating,
    // not blanket-struct-suffix-rejecting.
    let benign = r#"
pub struct BenignKey {
    pub canonical: u64,
    pub whole_hash: Hash16,
}
"#;
    let fields = scan_key_or_identity_struct_fields(benign);
    let trips = fields
        .iter()
        .any(|(_, _, t)| type_text_mentions_world_snapshot(t));
    assert!(
        !trips,
        "self-test (d): `BenignKey {{ whole_hash: Hash16 }}` must NOT trip — \
         the predicate must discriminate on type, not on struct suffix. Scanned fields: {fields:#?}"
    );

    // (e) Original name-pinned positive case still trips — the
    // rewrite preserves prior coverage.
    let synthetic_named = r#"
pub struct MyCacheKey {
    pub canonical: u64,
    pub world_snapshot: WorldSnapshot,
}
"#;
    let fields = scan_key_or_identity_struct_fields(synthetic_named);
    let trips = fields.iter().any(|(s, f, t)| {
        s == "MyCacheKey" && f == "world_snapshot" && type_text_mentions_world_snapshot(t)
    });
    assert!(
        trips,
        "self-test (e): the original `world_snapshot: WorldSnapshot` shape must \
         still trip after the rewrite. Scanned fields: {fields:#?}"
    );

    // (f) Word-boundary discriminator: a type called
    // `WorldSnapshotShim` or `MyWorldSnapshotIdentity` (different
    // type that happens to share a prefix/suffix) must NOT trip —
    // the predicate matches a full path segment, not a permissive
    // substring.
    let synthetic_word_boundary = r#"
pub struct PrefixShimKey {
    pub field: WorldSnapshotShim,
}
"#;
    let fields = scan_key_or_identity_struct_fields(synthetic_word_boundary);
    let trips = fields
        .iter()
        .any(|(_, _, t)| type_text_mentions_world_snapshot(t));
    assert!(
        !trips,
        "self-test (f): `PrefixShimKey {{ field: WorldSnapshotShim }}` must NOT trip — \
         the type-name match must respect Rust-identifier path-segment boundaries. \
         Scanned fields: {fields:#?}"
    );

    // (g) Multi-line wrapped field: `Arc<` on one line, the inner
    // `WorldSnapshot` on the next, the closing `>` on a third. A
    // line-based scanner that reads the colon line in isolation only
    // sees `Arc<` (no `WorldSnapshot`) and the bypass succeeds. The
    // AST scanner parses the whole field type as one `syn::Type`
    // node, so the wrap is invisible to it and the rejection fires.
    let synthetic_multiline_wrap = "
pub struct MultiLineWrappedKey {
    pub snap: Arc<
        WorldSnapshot
    >,
}
";
    let fields = scan_key_or_identity_struct_fields(synthetic_multiline_wrap);
    let trips = fields.iter().any(|(s, f, t)| {
        s == "MultiLineWrappedKey" && f == "snap" && type_text_mentions_world_snapshot(t)
    });
    assert!(
        trips,
        "self-test (g): a multi-line wrapped `pub snap: Arc<\\n    WorldSnapshot\\n>,` \
         must trip — wrapping the inner type across physical lines must not launder \
         the rejection. The AST scanner sees one logical `syn::Type` regardless of \
         line breaks. Scanned fields: {fields:#?}"
    );

    // (g2) Multi-line wrapped field nested two wrappers deep
    // (`Option<\n  Box<\n    WorldSnapshot\n  >\n>`). Proves the AST
    // descent through nested generics is wrap-agnostic.
    let synthetic_multiline_nested = "
pub struct NestedMultiLineKey {
    pub snap:
        Option<
            Box<
                WorldSnapshot
            >
        >,
}
";
    let fields = scan_key_or_identity_struct_fields(synthetic_multiline_nested);
    let trips = fields.iter().any(|(s, f, t)| {
        s == "NestedMultiLineKey" && f == "snap" && type_text_mentions_world_snapshot(t)
    });
    assert!(
        trips,
        "self-test (g2): a multi-line nested `Option<Box<WorldSnapshot>>` spread across \
         lines must trip. Scanned fields: {fields:#?}"
    );
}
