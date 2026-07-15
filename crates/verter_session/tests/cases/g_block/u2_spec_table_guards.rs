//! Diff-test for the generated `SemanticQueryKeySpec` table
//! (`crates/verter_session/src/semantic_query/query_key_spec_table.txt`).
//!
//! The artifact is written ONLY by the generator binary
//! `gen-query-key-spec` (`pnpm gen:query-key-spec`); this test NEVER writes —
//! it re-renders [`semantic_query_key_specs`] in memory and asserts THREE
//! independent, discriminating properties (mirroring the
//! generator-is-sole-writer / test-only-diffs split of
//! `typeinfo_proto_ts_freshness.rs`):
//!
//! 1. **Freshness** — `render_spec_table(&semantic_query_key_specs())`
//!    byte-equals the committed artifact. Fails on a hand-edit, a stale
//!    artifact, or a generator that was not re-run.
//! 2. **Enum-equality** (the discriminating core) — the spec table's
//!    variant-name set equals the variant identifiers scanned (via `syn`) from
//!    the live `pub enum SemanticQueryKey { … }` source. Fails when a variant
//!    is added/removed without regenerating.
//! 3. **Per-row sanity** — every row is `Live`; the value domain is `TypeNode`
//!    for every variant EXCEPT `Relate` (`Relation` — its execute arm is
//!    non-producing and its judgement lives in the dedicated relation_memo),
//!    `ResolveOverloadSet` (`OverloadSet` — the LIVE ordered visible
//!    signature group), and `FlowNarrowingAt` /
//!    `ContextualTypeAt` (`ProgramAnalysis` — forward-declared value domains
//!    whose non-producing execute arms return `Miss` until the reducers land in
//!    U6), and `SemanticQueryKeyTag::ALL` triangulates against both the spec set
//!    and the enum-scan set.

use std::collections::BTreeSet;
use std::path::PathBuf;

use verter_session::semantic_query::query_key_spec::{render_spec_table, semantic_query_key_specs};
use verter_session::semantic_query::SemanticQueryKeyTag;

/// The crate root (`crates/verter_session`). The generator resolves the same
/// path from `CARGO_MANIFEST_DIR` at run time, so the test and the generator
/// agree on the artifact location.
fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_crate_file(rel: &str) -> String {
    let path = crate_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "spec-table diff-test should read `{}`: {err}",
            path.display()
        )
    })
}

/// Scan the variant identifiers from the live `pub enum SemanticQueryKey { … }`
/// in `semantic_query.rs` using `syn` (a `verter_session` dev-dependency).
///
/// `syn::parse_file` parses the whole source into an AST, so EVERY variant form
/// is captured uniformly — unit (`NewKey,`), discriminant (`NewKey = 5,`),
/// tuple (`NewKey(T)`), and struct (`NewKey { .. }`) — and doc comments,
/// attributes (incl. brace-bearing ones), nested braces, and string literals
/// can never shift a hand-rolled depth counter to hide a later variant. This
/// closes the unit-variant false-pass hole a `(`/`{`-only line scanner had.
fn scan_enum_variants(src: &str) -> BTreeSet<String> {
    let file = syn::parse_file(src).expect("parse semantic_query.rs as a Rust source file");
    scan_item_enum_variants(&file.items, "SemanticQueryKey")
}

/// Collect every variant ident of the top-level `enum {ident}` among `items`.
/// Returns an empty set if no such enum is present (the caller's non-empty
/// assertion turns that into a clear "scanner is broken" failure).
fn scan_item_enum_variants(items: &[syn::Item], ident: &str) -> BTreeSet<String> {
    items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Enum(item_enum) if item_enum.ident == ident => Some(item_enum),
            _ => None,
        })
        .flat_map(|item_enum| item_enum.variants.iter())
        .map(|variant| variant.ident.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// The diff-test
// ---------------------------------------------------------------------------

#[test]
fn semantic_query_key_spec_table_equals_enum() {
    let specs = semantic_query_key_specs();

    // (1) FRESHNESS — in-memory render byte-equals the committed artifact.
    let rendered = render_spec_table(&specs);
    let committed = read_crate_file("src/semantic_query/query_key_spec_table.txt");
    assert_eq!(
        rendered, committed,
        "`crates/verter_session/src/semantic_query/query_key_spec_table.txt` \
         is STALE: the in-memory render of `semantic_query_key_specs()` no \
         longer byte-equals the committed artifact. Run \
         `pnpm gen:query-key-spec` (or `cargo run -p verter_session --bin \
         gen-query-key-spec`) and commit the regenerated file."
    );

    // (2) ENUM-EQUALITY — spec variant-name set == live enum variant set.
    let spec_names: BTreeSet<String> = specs.iter().map(|s| s.variant.name().to_string()).collect();
    let semantic_src = read_crate_file("src/semantic_query.rs");
    let enum_names = scan_enum_variants(&semantic_src);

    assert!(
        !enum_names.is_empty(),
        "enum scan found ZERO variants in `pub enum SemanticQueryKey` — the \
         scanner is broken, not the invariant."
    );
    assert_eq!(
        spec_names,
        enum_names,
        "the SemanticQueryKeySpec table has DRIFTED from the live \
         `SemanticQueryKey` enum.\n  only in spec table: {:?}\n  only in enum: \
         {:?}\nA variant was added/removed without regenerating the spec \
         table. Add the matching `SemanticQueryKeySpec` row + \
         `SemanticQueryKeyTag` arm, then run `pnpm gen:query-key-spec`.",
        spec_names.difference(&enum_names).collect::<Vec<_>>(),
        enum_names.difference(&spec_names).collect::<Vec<_>>(),
    );

    // (3) PER-ROW SANITY + triangulation against SemanticQueryKeyTag::ALL.
    // Every row is `Live`. The value-domain mapping is:
    //   - `Relate`            → `Relation` (execute arm non-producing; the
    //                            tri-state judgement lives in `relation_memo`).
    //   - `ResolveOverloadSet`→ `OverloadSet` (LIVE producer: the ordered
    //                            VISIBLE signature group; a signature-less
    //                            callee is an honest `Miss`, NEVER a fake
    //                            empty set).
    //   - `FlowNarrowingAt`   → `ProgramAnalysis` (forward-declared; its
    //                            non-producing execute arm returns `Miss` until
    //                            the flow-narrowing reducer lands in U6).
    //   - `ContextualTypeAt`  → `ProgramAnalysis` (forward-declared; its
    //                            non-producing execute arm returns `Miss` until
    //                            the contextual-type reducer lands in U6).
    //   - everything else     → `TypeNode`.
    // This three-way assertion is discriminating: it FAILS if `Relate` is
    // mislabeled back to `TypeNode`, if `ResolveOverloadSet` is mislabeled
    // `TypeNode` (or anything other than `OverloadSet`), OR if any other row
    // drifts off `TypeNode`.
    use verter_session::semantic_query::query_key_spec::KeyLifecycle;
    use verter_session::semantic_query::SemanticQueryValueTag;
    for spec in &specs {
        assert_eq!(
            spec.lifecycle,
            KeyLifecycle::Live,
            "every current spec row must be `Live`; `{}` is not",
            spec.variant.name()
        );
        let expected_domain = match spec.variant {
            SemanticQueryKeyTag::Relate => SemanticQueryValueTag::Relation,
            SemanticQueryKeyTag::ResolveOverloadSet => SemanticQueryValueTag::OverloadSet,
            // Flow narrowing + contextual typing carry the program-analysis
            // value domain (the narrowed / contextual node), NOT `TypeNode`.
            SemanticQueryKeyTag::FlowNarrowingAt | SemanticQueryKeyTag::ContextualTypeAt => {
                SemanticQueryValueTag::ProgramAnalysis
            }
            _ => SemanticQueryValueTag::TypeNode,
        };
        assert_eq!(
            spec.value_domain,
            expected_domain,
            "value-domain mismatch for `{}`: `Relate` must be `Relation`, \
             `ResolveOverloadSet` must be `OverloadSet`, `FlowNarrowingAt` / \
             `ContextualTypeAt` must be `ProgramAnalysis`, and every other \
             live key must be `TypeNode`",
            spec.variant.name()
        );
    }
    let all_names: BTreeSet<String> = SemanticQueryKeyTag::ALL
        .iter()
        .map(|t| t.name().to_string())
        .collect();
    assert_eq!(
        all_names, spec_names,
        "SemanticQueryKeyTag::ALL and the spec table have drifted"
    );
    assert_eq!(
        all_names, enum_names,
        "SemanticQueryKeyTag::ALL and the live enum have drifted"
    );
}

/// Discriminator self-test for the `syn`-based scanner. The fixture exercises
/// EVERY variant form — unit, discriminant, tuple, struct — interleaved with a
/// doc comment carrying an unbalanced `{` brace and an attribute carrying a `}`
/// brace, each placed BEFORE a later variant.
///
/// The captured set must be EXACTLY the variant idents. Two failure modes this
/// proves the scanner does NOT have:
///
/// - The previous line scanner only captured an uppercase ident immediately
///   followed by `(` or `{`, so `UnitKey,` and `DiscriminantKey = 5,` were
///   INVISIBLE to it (false-pass hole). Asserting both appear in the captured
///   set FAILS against that scanner and PASSES with `syn`.
/// - A brace-counting line scanner would let the unbalanced doc/attr braces
///   shift depth and silently drop every later variant. `syn` parses the AST,
///   so `Tuple` / `Struct` after the stray braces are still captured.
#[test]
fn enum_variant_scanner_discriminates() {
    let fixture = "\
pub enum SemanticQueryKey {
    /// a unit variant — and an unbalanced { brace lurking in this doc line
    UnitKey,
    /// a discriminant (C-like) variant
    DiscriminantKey = 5,
    /// a tuple variant
    TupleKey(TupleKeyPayload),
    #[doc = \"an attribute carrying a } brace\"]
    StructKey {
        base: ResolvedDeclSlotIdentity,
        args: Arc<[SemanticNodeId]>,
    },
}
";
    let scanned = scan_enum_variants(fixture);
    let expected: BTreeSet<String> = ["UnitKey", "DiscriminantKey", "TupleKey", "StructKey"]
        .into_iter()
        .map(str::to_string)
        .collect();
    assert_eq!(
        scanned, expected,
        "scanner self-test: the syn scan did not capture EXACTLY the four \
         variant forms (unit / discriminant / tuple / struct)."
    );

    // The unit + discriminant variants are precisely the ones a `(`/`{`-only
    // line scanner could never capture. Pinning them explicitly is the
    // discrimination proof for the closed false-pass hole.
    assert!(
        scanned.contains("UnitKey"),
        "scanner self-test: the UNIT variant was not captured — the unit-variant \
         false-pass hole is still open."
    );
    assert!(
        scanned.contains("DiscriminantKey"),
        "scanner self-test: the DISCRIMINANT variant was not captured — the \
         discriminant-variant false-pass hole is still open."
    );
    // The variants AFTER the unbalanced doc-comment brace and the brace-bearing
    // attribute survive (syn parses the AST; stray braces in prose/attrs cannot
    // shift any depth counter).
    assert!(
        scanned.contains("TupleKey") && scanned.contains("StructKey"),
        "scanner self-test: a variant after a brace in a doc comment / attribute \
         was dropped — the scan is not robust to braces in prose/attrs."
    );

    // Drift detection: an injected extra variant must change the scanned set.
    let drifted = fixture.replace(
        "    UnitKey,",
        "    GhostVariant(SemanticNodeId),\n    UnitKey,",
    );
    let scanned_drift = scan_enum_variants(&drifted);
    assert!(
        scanned_drift.contains("GhostVariant"),
        "scanner self-test: an injected extra variant was NOT caught — the \
         enum-equality check would be vacuous."
    );
    assert_ne!(
        scanned_drift, expected,
        "scanner self-test: the drifted enum scanned identically to the clean \
         fixture — drift is undetectable."
    );

    // The scanner ignores enums of other names and returns empty when the
    // target enum is absent (the main test's non-empty assertion turns that
    // into a clear failure).
    let other = "pub enum SomethingElse { A, B(C) }\n";
    assert!(
        scan_enum_variants(other).is_empty(),
        "scanner self-test: scanning a file without `SemanticQueryKey` must \
         yield an empty set, not variants of an unrelated enum."
    );
}

/// `ResolveMergedDeclaration` and `ResolveDeclarationAugmentation` are RESERVED
/// query-key names — they are NOT live `SemanticQueryKey` variants and appear in
/// neither `SemanticQueryKeyTag::ALL` nor the spec table. Live same-name
/// declaration merge and cross-file ambient augmentation are carried by the
/// `MergedDecl` peer-merge reducer over the graph node
/// `SemanticNodeData::MergedDecl` (CLAUDE.md "Declaration Merging (CRITICAL)" /
/// "Declaration Augmentation (CRITICAL)"), never by a dedicated query key. The
/// matching value domain `SemanticQueryValue::DeclarationAnalysis` is a NON-LIVE
/// shell: no live spec row resolves to it. (native-typeinfo-parity.md §2.1.)
///
/// This guard pins that invariant mechanically: it FAILS if either reserved name
/// becomes a live tag in `SemanticQueryKeyTag::ALL`, or if any live spec row is
/// wired to the `DeclarationAnalysis` value domain. It is complementary to
/// `semantic_query_key_spec_table_equals_enum`, which stays green for a
/// both-enum-and-table addition — exactly the case this guard catches.
#[test]
fn forward_planned_augmentation_keys_are_not_live_and_declaration_analysis_is_a_shell() {
    const FORWARD_PLANNED: [&str; 2] =
        ["ResolveMergedDeclaration", "ResolveDeclarationAugmentation"];

    // (1) Neither forward-planned augmentation key is a live tag. The predicate
    //     helper is the SAME code used in the non-vacuity proof below, so the
    //     main assertion and the discrimination proof exercise one check.
    let live_names: BTreeSet<String> = SemanticQueryKeyTag::ALL
        .iter()
        .map(|t| t.name().to_string())
        .collect();
    assert!(
        !live_set_contains_forward_planned(&live_names, &FORWARD_PLANNED),
        "a RESERVED augmentation key is present in `SemanticQueryKeyTag::ALL` — \
         {forward:?} must NOT be live `SemanticQueryKeyTag`s \
         (native-typeinfo-parity.md §2.1). Same-name merge / cross-file \
         augmentation is carried by the `MergedDecl` peer-merge reducer over \
         `SemanticNodeData::MergedDecl`, not a dedicated query key.",
        forward = FORWARD_PLANNED,
    );

    // (2) No live spec row resolves to the non-live `DeclarationAnalysis` shell.
    //     Again the predicate helper is shared with the non-vacuity proof.
    let specs = semantic_query_key_specs();
    assert!(
        !any_spec_resolves_to_declaration_analysis(&specs),
        "a live spec row resolves to `DeclarationAnalysis`, but that value \
         domain is a NON-LIVE shell — the live merged-declaration / \
         augmentation carrier is the graph node `SemanticNodeData::MergedDecl`, \
         never a `SemanticQueryValue::DeclarationAnalysis` produced by a live \
         key."
    );

    // Non-vacuity proof: run the SAME predicates against synthetic VIOLATING
    // inputs and assert each predicate returns `true` (i.e. it would TRIP on
    // drift). This mirrors `enum_variant_scanner_discriminates`' injected-drift
    // proof: the check is run against a known violation, not re-asserted as a
    // tautology, so neither (1) nor (2) above can pass vacuously. Each reserved
    // name is injected independently into a fresh clone of the real live set, so
    // check (1) is proven to trip on a single-name violation for every name.
    for reserved in FORWARD_PLANNED {
        let mut drifted_live = live_names.clone();
        drifted_live.insert(reserved.to_string());
        assert!(
            live_set_contains_forward_planned(&drifted_live, &FORWARD_PLANNED),
            "non-vacuity proof: `live_set_contains_forward_planned` did NOT trip \
             on a live set carrying the reserved key `{reserved}` — check (1) is \
             vacuous for that name."
        );
    }
    let drifted_specs = specs_with_synthetic_declaration_analysis_row(&specs);
    assert!(
        any_spec_resolves_to_declaration_analysis(&drifted_specs),
        "non-vacuity proof: `any_spec_resolves_to_declaration_analysis` did NOT \
         trip on a spec slice carrying a `DeclarationAnalysis` row — check (2) \
         is vacuous."
    );
}

/// Predicate behind check (1): does `live` contain ANY of the
/// `forward_planned` augmentation key names? Shared by the main assertion and
/// the non-vacuity proof so both exercise identical logic.
fn live_set_contains_forward_planned(live: &BTreeSet<String>, forward_planned: &[&str]) -> bool {
    forward_planned.iter().any(|f| live.contains(*f))
}

/// Predicate behind check (2): does ANY spec row resolve to the non-live
/// `DeclarationAnalysis` value domain? Shared by the main assertion and the
/// non-vacuity proof.
fn any_spec_resolves_to_declaration_analysis(
    specs: &[verter_session::semantic_query::query_key_spec::SemanticQueryKeySpec],
) -> bool {
    use verter_session::semantic_query::SemanticQueryValueTag;
    specs
        .iter()
        .any(|s| s.value_domain == SemanticQueryValueTag::DeclarationAnalysis)
}

/// Build a synthetic spec slice that DOES carry a `DeclarationAnalysis` row by
/// cloning a real row and rewriting its value domain. Used only to feed the
/// non-vacuity proof a known violation for check (2).
fn specs_with_synthetic_declaration_analysis_row(
    real: &[verter_session::semantic_query::query_key_spec::SemanticQueryKeySpec],
) -> Vec<verter_session::semantic_query::query_key_spec::SemanticQueryKeySpec> {
    use verter_session::semantic_query::SemanticQueryValueTag;
    let mut specs = real.to_vec();
    if let Some(first) = specs.first_mut() {
        first.value_domain = SemanticQueryValueTag::DeclarationAnalysis;
    }
    specs
}
