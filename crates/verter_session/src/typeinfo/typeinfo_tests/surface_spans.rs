//! Discriminating span-provenance tests for the public
//! [`crate::VerterHost::resolve_shallow_surface`] accessor + the span-rich
//! [`crate::typeinfo::TypeInfoSurface`] projection.
//!
//! Each test SLICES the fixture source at the reported span and compares the
//! slice to the expected source token — proving the span is CORRECT, not
//! merely present. Before this change the surface carried no spans (member
//! spans came from a fragile byte-scan, signature / index spans were a hard
//! `None` stub); these tests fail against that tree and pass against the
//! span-rich graph-payload tree.

use super::support::*;
use crate::typeinfo::{CanonicalSpan, TypeInfoSurface, TypeInfoSurfaceMember};

const SHALLOW_SURFACE_FACTS: &str = include_str!("fixtures/shallow_surface_facts.ts");
const FILE: &str = "/fixtures/shallow_surface_facts.ts";

/// Slice the canonical-span out of the given source. Asserts the span's file
/// matches `expected_file` (so a cross-file member can't silently slice the
/// wrong file).
fn slice<'a>(source: &'a str, span: &CanonicalSpan, expected_file: &str) -> &'a str {
    assert_eq!(
        span.file.as_ref(),
        expected_file,
        "span must reference file {expected_file}, got {}",
        span.file
    );
    &source[span.span.start as usize..span.span.end as usize]
}

fn member<'a>(surface: &'a TypeInfoSurface, name: &str) -> &'a TypeInfoSurfaceMember {
    surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == name)
        .unwrap_or_else(|| {
            panic!(
                "member `{name}` must be on the surface; got {:?}",
                surface
                    .members
                    .iter()
                    .map(|m| m.name.as_ref())
                    .collect::<Vec<_>>()
            )
        })
}

// ---------------------------------------------------------------------------
// (1) Member NAME / DECLARATION / TYPE-ANNOTATION spans slice to the exact
//     source token.
// ---------------------------------------------------------------------------

#[test]
fn member_spans_slice_to_exact_source_tokens() {
    let host = make_host_with_footprint();
    upsert_ts(&host, FILE, SHALLOW_SURFACE_FACTS);

    let surface = host
        .resolve_shallow_surface(FILE, "HybridSurface")
        .expect("HybridSurface must resolve to a one-level surface");

    let named = member(&surface, "named");
    // `named: string;`
    let name_span = named
        .name_span
        .as_ref()
        .expect("`named` must carry a NAME span (it has a single source site)");
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, name_span, FILE),
        "named",
        "name span must slice to the exact name token"
    );
    let ann_span = named
        .type_annotation_span
        .as_ref()
        .expect("`named` must carry a TYPE-ANNOTATION span");
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, ann_span, FILE),
        "string",
        "type-annotation span must slice to the exact annotation token"
    );
    let decl_span = named
        .origin
        .declaration_span
        .as_ref()
        .expect("`named` must carry a DECLARATION span");
    let decl_text = slice(SHALLOW_SURFACE_FACTS, decl_span, FILE);
    assert!(
        decl_text.starts_with("named") && decl_text.contains("string"),
        "declaration span must cover the whole `named: string` declaration; got {decl_text:?}"
    );

    // NEGATIVE: a span must NOT slice to an unrelated token.
    assert_ne!(
        slice(SHALLOW_SURFACE_FACTS, name_span, FILE),
        "string",
        "name span must not slice to the annotation"
    );
}

// ---------------------------------------------------------------------------
// (2) An optional member's name span slices to the name (not `name?`).
// ---------------------------------------------------------------------------

#[test]
fn optional_member_name_span_excludes_question_mark() {
    let host = make_host_with_footprint();
    upsert_ts(&host, FILE, SHALLOW_SURFACE_FACTS);

    let surface = host
        .resolve_shallow_surface(FILE, "HybridSurface")
        .expect("HybridSurface must resolve");
    let flag = member(&surface, "flag");
    // `flag?: boolean;`
    let name_span = flag
        .name_span
        .as_ref()
        .expect("`flag` must carry a name span");
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, name_span, FILE),
        "flag",
        "optional member name span slices to the bare name (no `?`)"
    );
    let ann_span = flag
        .type_annotation_span
        .as_ref()
        .expect("`flag` must carry an annotation span");
    assert_eq!(slice(SHALLOW_SURFACE_FACTS, ann_span, FILE), "boolean");
    assert!(flag.optional, "`flag` is optional");
}

// ---------------------------------------------------------------------------
// (3) Call-signature whole / parameter / return spans slice correctly.
// ---------------------------------------------------------------------------

#[test]
fn call_signature_spans_slice_correctly() {
    let host = make_host_with_footprint();
    upsert_ts(&host, FILE, SHALLOW_SURFACE_FACTS);

    let surface = host
        .resolve_shallow_surface(FILE, "HybridSurface")
        .expect("HybridSurface must resolve");

    assert_eq!(
        surface.call_signatures.len(),
        1,
        "HybridSurface carries one call signature"
    );
    let call = &surface.call_signatures[0];
    // `(token: string): number;`
    let sig_span = call
        .signature_span
        .as_ref()
        .expect("call signature must carry a whole-signature span");
    let sig_text = slice(SHALLOW_SURFACE_FACTS, sig_span, FILE);
    assert!(
        sig_text.starts_with("(token") && sig_text.contains("number"),
        "signature span must cover the whole `(token: string): number` signature; got {sig_text:?}"
    );
    assert_eq!(call.parameter_spans.len(), 1);
    let param_span = call.parameter_spans[0]
        .as_ref()
        .expect("call signature parameter must carry a span");
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, param_span, FILE),
        "token: string",
        "parameter span slices to the whole parameter"
    );
    let ret_span = call
        .return_type_span
        .as_ref()
        .expect("call signature must carry a return-type span");
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, ret_span, FILE),
        "number",
        "return-type span slices to the return annotation"
    );
}

// ---------------------------------------------------------------------------
// (4) Construct-signature whole / parameter / return spans slice correctly.
// ---------------------------------------------------------------------------

#[test]
fn construct_signature_spans_slice_correctly() {
    let host = make_host_with_footprint();
    upsert_ts(&host, FILE, SHALLOW_SURFACE_FACTS);

    let surface = host
        .resolve_shallow_surface(FILE, "HybridSurface")
        .expect("HybridSurface must resolve");

    assert_eq!(surface.construct_signatures.len(), 1);
    let ctor = &surface.construct_signatures[0];
    // `new (seed: number): HybridSurface;`
    let sig_span = ctor
        .signature_span
        .as_ref()
        .expect("construct signature must carry a whole-signature span");
    let sig_text = slice(SHALLOW_SURFACE_FACTS, sig_span, FILE);
    assert!(
        sig_text.contains("seed") && sig_text.contains("HybridSurface"),
        "construct signature span must cover the declaration; got {sig_text:?}"
    );
    let param_span = ctor.parameter_spans[0]
        .as_ref()
        .expect("construct signature parameter must carry a span");
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, param_span, FILE),
        "seed: number",
    );
    let ret_span = ctor
        .return_type_span
        .as_ref()
        .expect("construct signature must carry a return-type span");
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, ret_span, FILE),
        "HybridSurface"
    );
}

// ---------------------------------------------------------------------------
// (5) Index-signature declaration / key / value spans slice correctly.
// ---------------------------------------------------------------------------

#[test]
fn index_signature_spans_slice_correctly() {
    let host = make_host_with_footprint();
    upsert_ts(&host, FILE, SHALLOW_SURFACE_FACTS);

    let surface = host
        .resolve_shallow_surface(FILE, "HybridSurface")
        .expect("HybridSurface must resolve");

    assert_eq!(surface.index_signatures.len(), 1);
    let idx = &surface.index_signatures[0];
    // `[dynamic: string]: unknown;`
    let decl_span = idx
        .declaration_span
        .as_ref()
        .expect("index signature must carry a declaration span");
    let decl_text = slice(SHALLOW_SURFACE_FACTS, decl_span, FILE);
    assert!(
        decl_text.starts_with("[dynamic") && decl_text.contains("unknown"),
        "index signature declaration span must cover `[dynamic: string]: unknown`; got {decl_text:?}"
    );
    let key_span = idx
        .key_span
        .as_ref()
        .expect("index signature must carry a key span");
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, key_span, FILE),
        "dynamic: string",
        "key span slices to the index parameter"
    );
    let value_span = idx
        .value_span
        .as_ref()
        .expect("index signature must carry a value span");
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, value_span, FILE),
        "unknown",
        "value span slices to the value-type annotation"
    );
}

// ---------------------------------------------------------------------------
// (6) Cross-file: an INHERITED member's span references its ORIGIN file (the
//     heritage base's file), NOT the consuming declaration's file.
// ---------------------------------------------------------------------------

#[test]
fn inherited_member_span_references_origin_file_not_consumer() {
    const BASE: &str = "/src/base.ts";
    const DERIVED: &str = "/src/derived.ts";
    let base_src = "export interface Base {\n  baseOnly: number;\n}\n";
    let derived_src = "import type { Base } from './base';\n\
         export interface Derived extends Base {\n  derivedOnly: string;\n}\n";

    let host =
        make_host_with_workspace_files_footprint(&[(BASE, base_src), (DERIVED, derived_src)]);

    let surface = host
        .resolve_shallow_surface(DERIVED, "Derived")
        .expect("Derived must resolve to a one-level surface across the heritage edge");

    // The inherited `baseOnly` member originates in BASE — its spans (and
    // origin file) must reference BASE, not the consuming DERIVED file.
    let base_only = member(&surface, "baseOnly");
    assert_eq!(
        base_only.origin.canonical_file.as_deref(),
        Some(BASE),
        "inherited member origin must be the heritage base's file"
    );
    let name_span = base_only
        .name_span
        .as_ref()
        .expect("inherited member must carry a name span in its origin file");
    assert_eq!(
        slice(base_src, name_span, BASE),
        "baseOnly",
        "inherited member name span must slice the ORIGIN file's source"
    );

    // The own `derivedOnly` member originates in DERIVED.
    let derived_only = member(&surface, "derivedOnly");
    assert_eq!(
        derived_only.origin.canonical_file.as_deref(),
        Some(DERIVED),
        "own member origin must be the consuming declaration's file"
    );
    let derived_name_span = derived_only
        .name_span
        .as_ref()
        .expect("own member must carry a name span");
    assert_eq!(
        slice(derived_src, derived_name_span, DERIVED),
        "derivedOnly",
        "own member name span slices the DERIVED file's source"
    );
}

// ---------------------------------------------------------------------------
// (7) Typeinfo-primary full-surface assertion through the PUBLIC accessor.
//
// Exercises `resolve_shallow_surface` end-to-end: members + signatures + index
// + flags + merge role. This is what makes `TypeInfoSurface` non-dead.
//
// Also the structural "Shallow, not Expanded" proof: every member `value` is a
// shallow reference node (the surface did NOT eagerly expand member bodies), so
// no Expanded `Instantiate` was issued by the projection.
// ---------------------------------------------------------------------------

#[test]
fn public_accessor_projects_full_surface_with_flags_and_roles() {
    use crate::semantic_query::MemberMergeRole;

    let host = make_host_with_footprint();
    upsert_ts(&host, FILE, SHALLOW_SURFACE_FACTS);

    // Interface heritage: `HeritageDerived extends HeritageBase`.
    let surface = host
        .resolve_shallow_surface(FILE, "HeritageDerived")
        .expect("HeritageDerived must resolve");

    let names: Vec<&str> = surface.members.iter().map(|m| m.name.as_ref()).collect();
    // Own + inherited members present; a never-declared name absent.
    assert!(
        names.contains(&"dup"),
        "merged `dup` present; got {names:?}"
    );
    assert!(names.contains(&"baseOnly"), "inherited member present");
    assert!(names.contains(&"derivedOnly"), "own member present");
    assert!(
        !names.contains(&"absent"),
        "a never-declared name must be absent; got {names:?}"
    );

    // The own-body `dup` SHADOWS the inherited one → its merge role is OwnBody
    // (the P2-1 rule is observable on the public surface).
    let dup = member(&surface, "dup");
    assert_eq!(
        dup.origin.merge_role,
        MemberMergeRole::OwnBody,
        "the shadowing `dup` must carry the OwnBody merge role"
    );
    // Its span references the OWN-BODY declaration (`dup: string` in this file),
    // and slices to `dup`.
    let dup_name = dup
        .name_span
        .as_ref()
        .expect("`dup` must carry a name span");
    assert_eq!(slice(SHALLOW_SURFACE_FACTS, dup_name, FILE), "dup");

    // Structural shallow proof: no member's value expanded into an object
    // surface (an Expanded projection WOULD expand member bodies). The members
    // here are primitives / references, never `Object`.
    let graph_store_view = host.resolver_store_view();
    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx =
        crate::resolver_core::HostResolverContext::new(&host, &graph_store_view, overlay);
    let graph = {
        use crate::resolver_core::ResolverContext;
        host_ctx.project_type_store().semantic_graph()
    };
    for m in surface.members.iter() {
        let is_object = matches!(
            graph.node_data(m.value).as_deref(),
            Some(crate::semantic_query::SemanticNodeData::Object(_))
        );
        assert!(
            !is_object,
            "Shallow projection must not eagerly expand member `{}` into an object surface \
             (would indicate an Expanded Instantiate); value node = {:?}",
            m.name, m.value
        );
    }
}
