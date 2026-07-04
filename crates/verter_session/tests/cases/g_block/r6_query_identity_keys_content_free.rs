//! R6 structural guard for query-identity cache keys.
//!
//! Asserts that `SemanticQueryKey::Instantiate`,
//! `SemanticQueryKey::ResolveMacroPayload`, and the mirrored
//! `FamilyKey` shapes inside the family memo carry NO content / version
//! hashes — neither as a direct field (`whole_hash`, `content_hash`,
//! `fact_dep_signature`) nor through an embedded type that transitively
//! contains one (specifically `DeclIdentity`, which remains versioned
//! as a value-side payload).
//!
//! Two-layer enforcement:
//!
//! 1. **Compile-time destructuring proof** — the `base` / `owner`
//!    field on each variant is the env-bearing, content-free
//!    `ResolvedDeclSlotIdentity` slot (`defining_canonical`,
//!    `merged_symbol_name`, `symbol_space`, + the `project_identity` /
//!    `type_env_hash` / `lib_env_hash` ENV dims — NOT content hashes).
//!    The exhaustive destructuring patterns below would fail to compile
//!    if a future commit re-introduced `whole_hash` / `content_hash` to
//!    either the variant or to the slot itself.
//!
//! 2. **Source-AST scan** — walk
//!    `crates/verter_session/src/semantic_query.rs` and
//!    `crates/verter_session/src/semantic_query_memo/family.rs`
//!    and assert that the `Instantiate` and `ResolveMacroPayload`
//!    variant bodies (in both `SemanticQueryKey` and `FamilyKey`)
//!    contain NO of the forbidden field names and NO embedded
//!    `DeclIdentity` reference inside the variant.
//!
//! The structural arm (1) catches direct shape regressions at compile
//! time. The source-AST arm (2) is the durable architecture pin
//! registered in
//! `tests/cases/g_misc0/critical_rules_have_guards.rs::CRITICAL_RULE_GUARDS` so the
//! "Cache Architecture (CRITICAL)" rule's R6 clause has a named
//! discriminating guard.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use verter_session::semantic_query::{
    ProjectionMode, ProjectionReductionContext, SemanticNodeId, SemanticQueryKey,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_file(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read `{rel}`: {e}"))
}

/// Compile-time proof that `SemanticQueryKey::Instantiate.base` is the
/// env-bearing, content-free `ResolvedDeclSlotIdentity` —
/// NOT a `DeclIdentity` (versioned) and NOT carrying any
/// content/version hash. The slot's env dims (`project_identity` /
/// `type_env_hash` / `lib_env_hash`) are ENV dimensions, not content
/// hashes, so R6 still holds.
#[test]
fn r6_semantic_query_key_instantiate_base_is_content_free_decl_key() {
    use verter_session::semantic_query::{InstantiateContext, ResolvedDeclSlotIdentity};
    let base =
        ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from("/r6_check.ts"), Arc::from("Foo"));
    let key = SemanticQueryKey::Instantiate {
        base: base.clone(),
        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        context: InstantiateContext::non_file(
            ProjectionReductionContext::published(ProjectionMode::Expanded),
            Default::default(),
        ),
    };
    match key {
        SemanticQueryKey::Instantiate {
            base:
                ResolvedDeclSlotIdentity {
                    defining_canonical,
                    merged_symbol_name,
                    symbol_space: _,
                    env: _,
                },
            args: _,
            // `InstantiateContext` is SEALED (private fields; the only
            // constructors are `file_backed` / `non_file`), so an external
            // destructure cannot enumerate its fields. The accessors expose
            // the embedded projection-reduction identity, the
            // `resolve_env_hash` ENV dim, and the `body_source` source-kind
            // axis — no content/version hash. The exhaustive in-module R6
            // field proof is the `w_instantiate_context` witness beside the
            // sealed definition (`semantic_query.rs`): a compile-anchored
            // no-`..` destructure classifying every private field through an
            // allowed-dimension path, so a new field — including an
            // R6-forbidden hash — fails compilation there until classified.
            context,
        } => {
            let _ = context.projection_reduction();
            let _: verter_session::semantic_query::HashValue = context.resolve_env_hash();
            assert_eq!(
                context.body_source(),
                verter_session::semantic_query::InstantiateBodySource::NonFile,
                "the sealed constructor used above is `non_file`"
            );
            // The slot exposes a fixed field set with NO `whole_hash` /
            // `content_hash` / `fact_dep_signature`. Any future addition
            // of such a content/version field breaks this exhaustive
            // destructuring at compile time.
            assert_eq!(defining_canonical.as_ref(), "/r6_check.ts");
            assert_eq!(merged_symbol_name.as_ref(), "Foo");
        }
        _ => panic!("expected Instantiate variant"),
    }
}

/// Compile-time proof that `SemanticQueryKey::ResolveMacroPayload.owner`
/// is the env-bearing, content-free `ResolvedDeclSlotIdentity` — NOT a
/// `DeclIdentity` and carrying no content/version hash.
#[test]
fn r6_semantic_query_key_resolve_macro_payload_owner_is_content_free_decl_key() {
    use verter_session::semantic_query::{MacroPayloadContext, ResolvedDeclSlotIdentity};
    let owner = ResolvedDeclSlotIdentity::type_slot_unscoped(
        Arc::from("/r6_check.vue"),
        Arc::from("<sfc-script-setup>"),
    );
    let key = SemanticQueryKey::ResolveMacroPayload {
        owner: owner.clone(),
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        type_args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        context: MacroPayloadContext::new(Default::default(), ProjectionMode::Expanded),
    };
    match key {
        SemanticQueryKey::ResolveMacroPayload {
            owner:
                ResolvedDeclSlotIdentity {
                    defining_canonical,
                    merged_symbol_name,
                    symbol_space: _,
                    env: _,
                },
            macro_index: _,
            macro_kind: _,
            type_args: _,
            context:
                MacroPayloadContext {
                    resolve_env_hash: _,
                    mode: _,
                },
        } => {
            assert_eq!(defining_canonical.as_ref(), "/r6_check.vue");
            assert_eq!(merged_symbol_name.as_ref(), "<sfc-script-setup>");
        }
        _ => panic!("expected ResolveMacroPayload variant"),
    }
}

/// Source-AST scan over `semantic_query.rs` — the `Instantiate` and
/// `ResolveMacroPayload` variant bodies of `SemanticQueryKey` must
/// contain NO `whole_hash`, `content_hash`, or `fact_dep_signature`
/// field, AND no embedded `DeclIdentity` type.
#[test]
fn r6_semantic_query_key_variants_carry_no_version_hash_in_source() {
    let source = read_file("crates/verter_session/src/semantic_query.rs");

    // Forbidden field names that, if added to the variant body,
    // would re-introduce a content/version hash into a query-identity
    // key. The scan looks for each name with the field-shape suffix
    // ': ' inside the variant body block.
    const FORBIDDEN_FIELDS: &[&str] = &[
        "whole_hash:",
        "content_hash:",
        "parse_stable_hash:",
        "fact_dep_signature:",
    ];
    // Forbidden embedded types that transitively contain a version
    // hash. `DeclIdentity` is the canonical offender — keeping it as
    // a value-side payload is fine, embedding it in a query-identity
    // key is the R6 violation. The retired content-free `DeclKey`
    // type is ALSO forbidden as an embed: it was deleted in this
    // migration, and re-embedding it (`base: DeclKey` / `owner:
    // DeclKey`) would re-introduce the dual-path key it replaced. The
    // bare `: DeclKey` markers are defensive — they catch any other
    // field embedding `DeclKey` while the trailing field-shape suffix
    // (`,` / ` ` / `}`) keeps them from false-matching `ResolveDeclKey`
    // (the distinct, live `ResolveDecl` query key).
    const FORBIDDEN_EMBEDS: &[&str] = &[
        "base: DeclIdentity",
        "owner: DeclIdentity",
        "base: DeclKey,",
        "base: DeclKey ",
        "base: DeclKey}",
        "owner: DeclKey,",
        "owner: DeclKey ",
        "owner: DeclKey}",
        ": DeclKey,",
        ": DeclKey ",
        ": DeclKey}",
    ];

    for variant in ["Instantiate {", "ResolveMacroPayload {"] {
        let body = extract_brace_block(&source, variant).unwrap_or_else(|| {
            panic!("R6 GUARD: could not locate `{variant}` variant body in semantic_query.rs")
        });
        for needle in FORBIDDEN_FIELDS {
            assert!(
                !body.contains(needle),
                "R6 GUARD VIOLATION — `SemanticQueryKey::{}` body in \
                 `semantic_query.rs` contains forbidden field `{}`. A \
                 query-identity cache key MUST NOT carry a content / \
                 version hash; per-value version-rooting lives on the \
                 cached `MemoEntry`'s `ReadSetSignature.facts` and \
                 `self_root_canonicals`. Body:\n{}",
                variant.trim_end_matches(" {"),
                needle,
                body
            );
        }
        for needle in FORBIDDEN_EMBEDS {
            assert!(
                !body.contains(needle),
                "R6 GUARD VIOLATION — `SemanticQueryKey::{}` body in \
                 `semantic_query.rs` embeds `{}`. `DeclIdentity` is a \
                 versioned (`whole_hash`-bearing) type and must NOT be \
                 embedded in a query-identity key — use the env-bearing \
                 content-free `ResolvedDeclSlotIdentity` slot (built via \
                 `type_slot_for`) instead. Body:\n{}",
                variant.trim_end_matches(" {"),
                needle,
                body
            );
        }
    }
}

/// Source-AST scan over `semantic_query_memo/family.rs` — the
/// `FamilyKey::Instantiate` and `FamilyKey::ResolveMacroPayload`
/// variants are the mode-erased identities that `family_and_slot`
/// projects to and that the memo's `entries: Mutex<FxHashMap<FamilyKey,
/// FamilySlots>>` actually keys on. They must satisfy the same R6
/// no-content-hash invariant as their `SemanticQueryKey` siblings.
#[test]
fn r6_family_key_variants_carry_no_version_hash_in_source() {
    let source = read_file("crates/verter_session/src/semantic_query_memo/family.rs");

    const FORBIDDEN_FIELDS: &[&str] = &[
        "whole_hash:",
        "content_hash:",
        "parse_stable_hash:",
        "fact_dep_signature:",
    ];
    // See the `semantic_query.rs` scan above for the rationale: the
    // versioned `DeclIdentity` AND the retired content-free `DeclKey`
    // are both forbidden as `base`/`owner` embeds in the mode-erased
    // `FamilyKey` identity. The trailing field-shape suffixes keep the
    // bare `: DeclKey` markers from matching `ResolveDeclKey`.
    const FORBIDDEN_EMBEDS: &[&str] = &[
        "base: DeclIdentity",
        "owner: DeclIdentity",
        "base: DeclKey,",
        "base: DeclKey ",
        "base: DeclKey}",
        "owner: DeclKey,",
        "owner: DeclKey ",
        "owner: DeclKey}",
        ": DeclKey,",
        ": DeclKey ",
        ": DeclKey}",
    ];

    for variant in ["Instantiate {", "ResolveMacroPayload {"] {
        let body = extract_brace_block(&source, variant).unwrap_or_else(|| {
            panic!("R6 GUARD: could not locate `{variant}` variant body in family.rs")
        });
        for needle in FORBIDDEN_FIELDS {
            assert!(
                !body.contains(needle),
                "R6 GUARD VIOLATION — `FamilyKey::{}` body in \
                 `family.rs` contains forbidden field `{}`. The \
                 `FamilyKey` is the mode-erased memo identity \
                 (`Mutex<FxHashMap<FamilyKey, FamilySlots>>`) and \
                 MUST NOT carry a content / version hash. Body:\n{}",
                variant.trim_end_matches(" {"),
                needle,
                body
            );
        }
        for needle in FORBIDDEN_EMBEDS {
            assert!(
                !body.contains(needle),
                "R6 GUARD VIOLATION — `FamilyKey::{}` body in \
                 `family.rs` embeds `{}`. `DeclIdentity` is a \
                 versioned type — use the env-bearing content-free \
                 `ResolvedDeclSlotIdentity` slot (built via `type_slot_for`) \
                 instead. Body:\n{}",
                variant.trim_end_matches(" {"),
                needle,
                body
            );
        }
    }
}

/// Source-AST scan over `semantic_query.rs` — the
/// `ResolvedDeclSlotIdentity` slot (the `Instantiate` / `ResolveMacroPayload`
/// base/owner identity) must be content-free. It
/// legitimately carries the `project_identity` / `type_env_hash` /
/// `lib_env_hash` ENV dims, but adding a `whole_hash` / `content_hash` /
/// `parse_stable_hash` / `fact_dep_signature` field would re-violate R6
/// across every key that embeds it, so this guard independently pins the
/// struct shape. The retired content-free `DeclKey` struct must NOT be
/// reintroduced in ANY declaration form.
#[test]
fn r6_decl_slot_struct_is_content_free_in_source() {
    let source = read_file("crates/verter_session/src/semantic_query.rs");
    let body = extract_brace_block(&source, "pub struct ResolvedDeclSlotIdentity {")
        .expect(
            "R6 GUARD: could not locate `pub struct ResolvedDeclSlotIdentity` body in semantic_query.rs",
        );

    for needle in [
        "whole_hash:",
        "content_hash:",
        "parse_stable_hash:",
        "fact_dep_signature:",
    ] {
        assert!(
            !body.contains(needle),
            "R6 GUARD VIOLATION — `pub struct ResolvedDeclSlotIdentity` carries \
             forbidden content/version field `{}`. The query-identity slot MUST \
             stay content-free (env dims only); per-file version rooting belongs \
             on the cached value, not on the key. Body:\n{}",
            needle,
            body
        );
    }

    // The retired content-free `DeclKey` query-identity struct must not
    // be reintroduced in ANY declaration form OR under ANY visibility:
    // `pub struct DeclKey`, `pub(crate) struct DeclKey`,
    // `pub(super) struct DeclKey`, or bare `struct DeclKey` — each in unit
    // (`struct DeclKey;`), tuple (`struct DeclKey(...)`), generic
    // (`struct DeclKey<...>`), braced (`struct DeclKey { ... }`), or
    // whitespace-variant form. The base/owner identity is the env-bearing
    // content-free `ResolvedDeclSlotIdentity` slot. The leading boundary
    // (non-identifier char before `struct`) plus the literal `struct `
    // keyword keep this from false-matching `ResolveDeclKey` (the distinct,
    // live `ResolveDecl` query key, declared `struct ResolveDeclKey`); the
    // trailing delimiter boundary rejects `DeclKeyV2` and prose. The
    // tree-wide scan below pins the same invariant across EVERY production
    // file — `semantic_query.rs` is no longer the only place a reintroduced
    // `struct DeclKey` could land and be embedded by `FamilyKey`.
    assert!(
        !source_reintroduces_decl_key_struct(&source),
        "R6 GUARD VIOLATION — the retired `struct DeclKey` reappeared in \
         semantic_query.rs (in some declaration form, under some visibility); \
         the base/owner identity is the env-bearing content-free \
         `ResolvedDeclSlotIdentity` slot."
    );
}

/// Tree-wide reinforcement of the `struct DeclKey`-reintroduction ban:
/// the retired content-free query-identity struct must not reappear in
/// ANY production source file, not just `semantic_query.rs`. A
/// `pub struct DeclKey` reintroduced in a different module and then
/// embedded by `FamilyKey` would evade the single-file scan in
/// [`r6_decl_slot_struct_is_content_free_in_source`]; this guard walks
/// every `crates/*/src/**.rs` file and asserts none reintroduces a
/// `struct DeclKey` in any declaration form. `ResolveDeclKey` /
/// `DeclKeyV2` / prose are still rejected by the same shared
/// [`source_reintroduces_decl_key_struct`] boundary logic.
#[test]
fn r6_no_decl_key_struct_reintroduced_anywhere_in_production() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let crates_dir = crate_root.parent().expect("crates/").to_path_buf();
    assert!(
        crates_dir.is_dir(),
        "R6 GUARD fixture invariant: `{}` MUST exist",
        crates_dir.display(),
    );

    let mut offenders: Vec<String> = Vec::new();
    for src_file in walk_crate_src_rs_files(&crates_dir) {
        let source = match fs::read_to_string(&src_file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if source_reintroduces_decl_key_struct(&source) {
            offenders.push(src_file.display().to_string());
        }
    }

    assert!(
        offenders.is_empty(),
        "R6 GUARD VIOLATION — the retired `struct DeclKey` query-identity \
         struct reappeared in production source. The base/owner identity is \
         the env-bearing content-free `ResolvedDeclSlotIdentity` slot (built \
         via `type_slot_for` / `builtin_type_slot`) — `DeclKey` must NOT be \
         reintroduced anywhere. Offending file(s):\n{}",
        offenders.join("\n"),
    );
}

/// Walk every `crates/*/src/**/*.rs` production file under `crates_dir`.
/// Mirrors the production-window file-walk convention used by the
/// sibling `no_default_env_hashes_in_production` guard.
fn walk_crate_src_rs_files(crates_dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(crates_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let src = path.join("src");
            if src.is_dir() {
                walk_rs_files(&src, &mut out);
            }
        }
    }
    out
}

fn walk_rs_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_rs_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}

/// Strip `//` line comments and `/* … */` block comments from `source`,
/// then collapse every run of ASCII whitespace to a single space. The
/// result is a token-normalized view of the source: any whitespace
/// formatting (`struct  DeclKey`, `struct\nDeclKey`) and any interior
/// comment (`struct/*x*/DeclKey`) is erased, so a substring/boundary scan
/// over the normalized text is robust to all three formatting evasions.
///
/// Block comments are replaced by a single space (so `struct/*x*/DeclKey`
/// becomes `struct DeclKey` — the keyword and the name stay separated by a
/// boundary). Line comments are dropped entirely up to the newline (the
/// newline itself then collapses with surrounding whitespace). String and
/// char literals are NOT specially handled: this is a coarse architecture
/// guard over Rust source, and a `struct DeclKey {` declaration never
/// occurs inside a literal in practice — the goal is to defeat formatting
/// evasions, not to be a full lexer.
fn strip_comments_and_normalize_ws(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0usize;
    while i < bytes.len() {
        // Line comment: `// … \n`
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            // Leave the newline (if any) for the whitespace pass; emit a
            // space so adjacent tokens stay separated.
            out.push(' ');
            continue;
        }
        // Block comment: `/* … */` (non-nesting is sufficient for this
        // coarse guard — Rust block comments do nest, but a nested
        // `struct DeclKey {` inside a comment is not a real declaration
        // either way; emitting a space keeps the boundary scan sound).
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            out.push(' ');
            continue;
        }
        // Non-comment byte: copy through, treating any ASCII whitespace as
        // a single space (collapsed in the second pass below).
        out.push(bytes[i] as char);
        i += 1;
    }
    // Collapse runs of ASCII whitespace to a single space.
    let mut normalized = String::with_capacity(out.len());
    let mut prev_ws = false;
    for ch in out.chars() {
        if ch.is_ascii_whitespace() {
            if !prev_ws {
                normalized.push(' ');
            }
            prev_ws = true;
        } else {
            normalized.push(ch);
            prev_ws = false;
        }
    }
    normalized
}

/// True iff `source` reintroduces the retired query-identity struct
/// `struct DeclKey` in ANY declaration form AND under ANY visibility:
/// `pub struct DeclKey`, `pub(crate) struct DeclKey`,
/// `pub(super) struct DeclKey`, or bare `struct DeclKey` — each in any
/// shape: braced (`struct DeclKey { ... }`), unit (`struct DeclKey;`),
/// tuple (`struct DeclKey(...)`), generic (`struct DeclKey<...>`), or with
/// arbitrary whitespace before the delimiter (`struct DeclKey\n{`).
///
/// TOKEN-ROBUST: the source is first run through
/// [`strip_comments_and_normalize_ws`], so the scan is immune to the three
/// formatting evasions that defeat a naive `source.contains("struct
/// DeclKey")` substring check: extra whitespace (`struct  DeclKey`), a
/// newline between keyword and name (`struct\nDeclKey`), and a block
/// comment between them (`struct/*x*/DeclKey`). After normalization each of
/// those collapses to the canonical `struct DeclKey` token sequence (a
/// single space between `struct` and `DeclKey`).
///
/// On the normalized text the scanner anchors on `struct ` (the keyword
/// plus exactly one normalized space — the `pub`/`pub(crate)`/`pub(...)`
/// visibility prefix is OPTIONAL, so anchoring on `struct ` rather than
/// `pub struct ` catches the visibility-evasion forms) and for each
/// occurrence applies BOTH word boundaries:
///
/// - LEADING boundary: the char immediately before `struct` must be a
///   non-identifier char (space / start-of-source). This keeps the scan
///   from matching `notstruct DeclKey` style identifiers; the literal
///   `struct ` keyword also means `ResolveDeclKey` cannot match (its
///   declaration normalizes to `struct ResolveDeclKey`, never `struct
///   DeclKey`).
/// - NAME match + TRAILING boundary: after `struct ` the next token must be
///   exactly `DeclKey` followed by a struct-declaration delimiter (`{` /
///   `;` / `(` / `<`) — with at most one normalized space before the
///   delimiter. That rejects any longer identifier (`DeclKeyV2`, where the
///   char after `DeclKey` is `V`, not a delimiter or space) AND prose
///   mentions (`struct DeclKey was retired`, where the token after a space
///   is `was`, not a delimiter) while still catching every decl form.
fn source_reintroduces_decl_key_struct(source: &str) -> bool {
    let normalized = strip_comments_and_normalize_ws(source);
    const KEYWORD: &str = "struct ";
    const NAME: &str = "DeclKey";
    let mut search_from = 0usize;
    while let Some(rel) = normalized[search_from..].find(KEYWORD) {
        let kw_idx = search_from + rel;
        // Advance past this `struct ` for the next iteration regardless of
        // match outcome.
        let after_kw = kw_idx + KEYWORD.len();
        search_from = after_kw;

        // LEADING word boundary: the char before `struct` must not be an
        // identifier char (so `notstruct DeclKey` is not a match). A match
        // at the start of source has no preceding char and is accepted.
        let leading_ok = kw_idx == 0
            || !normalized[..kw_idx]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !leading_ok {
            continue;
        }

        // The token immediately following `struct ` must be exactly
        // `DeclKey`.
        let rest = &normalized[after_kw..];
        if !rest.starts_with(NAME) {
            continue;
        }
        // After `DeclKey` skip at most the single normalized space, then
        // require a struct-declaration delimiter. If the next char is an
        // identifier char (`DeclKeyV2` → `V`) the delimiter check fails; if
        // it is a space followed by prose (`DeclKey was`) the char after the
        // space is `w`, also failing the delimiter check.
        let after_name = &rest[NAME.len()..];
        let next = after_name.strip_prefix(' ').unwrap_or(after_name);
        if matches!(
            next.chars().next(),
            Some('{') | Some(';') | Some('(') | Some('<')
        ) {
            return true;
        }
    }
    false
}

/// Discrimination harness for [`source_reintroduces_decl_key_struct`]:
/// every formatting evasion MUST be detected, and the near-miss
/// identifiers / prose MUST be rejected. This is the
/// characterizing test that proves the token-robust detector defeats the
/// exact evasions a naive substring scan would miss.
#[test]
fn decl_key_struct_detector_discriminates_formatting_evasions() {
    // MUST DETECT — every legal declaration form + the three formatting
    // evasions (extra whitespace, newline, interior block comment).
    let must_detect = [
        "pub struct  DeclKey {}",          // two spaces
        "pub struct\nDeclKey {}",          // newline between keyword and name
        "pub struct/*x*/DeclKey {}",       // block comment between
        "struct DeclKey;",                 // unit
        "pub(crate) struct DeclKey(u32);", // tuple, restricted visibility
        "struct DeclKey<T>{}",             // generic
        "struct DeclKey {\n  a: u32,\n}",  // braced
        "    struct DeclKey {}",           // leading indentation
    ];
    for src in must_detect {
        assert!(
            source_reintroduces_decl_key_struct(src),
            "detector MUST flag reintroduced `struct DeclKey`: {src:?}",
        );
    }

    // MUST REJECT — longer identifiers, the distinct live `ResolveDeclKey`
    // query key, prose mentions, and a non-keyword `notstruct`.
    let must_reject = [
        "struct ResolveDeclKey {}",              // distinct live type
        "pub struct DeclKeyV2 {}",               // longer identifier
        "// struct DeclKey was deleted",         // line-comment prose
        "/* struct DeclKey was deleted */",      // block-comment prose
        "struct ResolvedDeclSlotIdentity {}",    // the replacement slot
        "let notstruct DeclKey = 1;",            // not the `struct` keyword
        "// the retired struct DeclKey is gone", // prose mid-line comment
    ];
    for src in must_reject {
        assert!(
            !source_reintroduces_decl_key_struct(src),
            "detector MUST NOT flag non-declaration / near-miss: {src:?}",
        );
    }
}

/// Locate the first `{ ... }` brace block following a textual
/// `needle` in `source`. Returns the inner text between the matching
/// `{` and the corresponding `}` (excluding the braces themselves) or
/// `None` if the needle is missing or unbalanced.
fn extract_brace_block(source: &str, needle: &str) -> Option<String> {
    let start = source.find(needle)?;
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut iter = start..bytes.len();
    let mut open: Option<usize> = None;
    for i in &mut iter {
        match bytes[i] {
            b'{' => {
                if depth == 0 {
                    open = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let begin = open? + 1;
                    return Some(source[begin..i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}
