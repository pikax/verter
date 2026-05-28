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

    // DISCRIMINATING structural shallow proof: the `nested` member is
    // OBJECT-ALIAS-typed (`nested: HeritageBase`). Under the shallow-by-default
    // rule its value MUST stay a reference carrier — an Expanded / eager
    // projection WOULD materialise it into an `Object` node. A primitive-only
    // member set (the pre-fix fixture) made this loop vacuous; `nested`
    // discriminates because it is the one member that COULD become an object.
    assert!(
        names.contains(&"nested"),
        "object-alias-typed member `nested` present; got {names:?}"
    );
    let graph_store_view = host.resolver_store_view();
    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx =
        crate::resolver_core::HostResolverContext::new(&host, &graph_store_view, overlay);
    let graph = {
        use crate::resolver_core::ResolverContext;
        host_ctx.project_type_store().semantic_graph()
    };
    let nested = member(&surface, "nested");
    assert!(
        !matches!(
            graph.node_data(nested.value).as_deref(),
            Some(crate::semantic_query::SemanticNodeData::Object(_))
        ),
        "Shallow projection must NOT eagerly expand the object-alias member `nested` into an \
         Object surface (would indicate an Expanded Instantiate); value node = {:?}",
        nested.value
    );
    // Every other member also stays a non-object shallow value.
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

// ---------------------------------------------------------------------------
// (8) DISCRIMINATING shallow proof: a member whose declared type is a NESTED
//     OBJECT stays a SHALLOW REFERENCE carrier — its value node is NOT an
//     `Object` surface (which an Expanded projection would have materialised).
//
// Test (7)'s `!is_object` loop used primitive / reference members, so it passes
// even if the implementation accidentally expanded a member into an object. A
// nested-object member discriminates: it WOULD become an `Object` node under an
// eager / Expanded projection, and must stay a shallow `DeclRef`-style carrier
// under the Shallow projection. (codex#2 P2)
// ---------------------------------------------------------------------------

#[test]
fn nested_object_member_stays_shallow_reference_not_materialized() {
    const NESTED: &str = "/src/nested.ts";
    // `outer`'s declared type is a NESTED named alias whose body is an object.
    // Under the shallow-by-default rule the published `outer` member's value is
    // a reference carrier, NOT the expanded `{ inner: string }` object surface.
    let src = "export interface Nested {\n  \
         outer: Inner;\n  \
         leaf: string;\n}\n\
         export interface Inner {\n  inner: string;\n}\n";

    let host = make_host_with_workspace_files_footprint(&[(NESTED, src)]);

    let surface = host
        .resolve_shallow_surface(NESTED, "Nested")
        .expect("Nested must resolve to a one-level surface");

    let graph_store_view = host.resolver_store_view();
    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx =
        crate::resolver_core::HostResolverContext::new(&host, &graph_store_view, overlay);
    let graph = {
        use crate::resolver_core::ResolverContext;
        host_ctx.project_type_store().semantic_graph()
    };

    // The nested-object member `outer` MUST stay a shallow reference — its value
    // node is NOT an `Object` surface. An Expanded / eager projection would have
    // materialised `{ inner: string }` here; the Shallow projection must not.
    let outer = member(&surface, "outer");
    let outer_data = graph.node_data(outer.value);
    assert!(
        !matches!(
            outer_data.as_deref(),
            Some(crate::semantic_query::SemanticNodeData::Object(_))
        ),
        "nested-object member `outer` must stay a SHALLOW reference carrier, not be \
         materialised into an Object surface; value node data = {:?}",
        outer_data.as_deref()
    );

    // POSITIVE: the member is still present with its real declaration span in
    // THIS file (the shallow carrier is span-rich, not stripped).
    let outer_name = outer
        .name_span
        .as_ref()
        .expect("`outer` must carry a name span in its declaration file");
    assert_eq!(slice(src, outer_name, NESTED), "outer");
    let outer_ann = outer
        .type_annotation_span
        .as_ref()
        .expect("`outer` must carry a type-annotation span");
    assert_eq!(
        slice(src, outer_ann, NESTED),
        "Inner",
        "the annotation span slices to the reference name, not an inlined object"
    );
}

// ---------------------------------------------------------------------------
// (9) DECLARATION-ORIGIN span loss (codex#1 P1): a member whose VALUE type is an
//     UNRESOLVED (scope-less) node still reports its REAL declaration spans,
//     anchored to its DECLARATION file — NOT `None`.
//
// `export interface Broken { present: MissingType; }` — `MissingType` is
// unresolved, so the member's VALUE lowers to a scope-less `Opaque(Miss)` node
// whose `node_scope` is `None`. Before the declaration-origin fix the public
// surface anchored the member's spans to `node_origin_file(member.value)` →
// `None`, masking the member's REAL OXC name / decl / type-annotation spans.
// This test FAILS pre-fix (spans report `None`) and PASSES post-fix.
// ---------------------------------------------------------------------------

#[test]
fn member_with_unresolved_value_type_keeps_real_declaration_spans() {
    const BROKEN: &str = "/src/broken.ts";
    // `MissingType` is never declared → the member value is a scope-less node.
    let src = "export interface Broken {\n  present: MissingType;\n}\n";

    let host = make_host_with_workspace_files_footprint(&[(BROKEN, src)]);

    let surface = host
        .resolve_shallow_surface(BROKEN, "Broken")
        .expect("Broken must resolve to a one-level surface despite the unresolved member type");

    let present = member(&surface, "present");

    // The member's DECLARATION file is BROKEN — independent of where its value
    // type (fails to) resolve.
    assert_eq!(
        present.origin.canonical_file.as_deref(),
        Some(BROKEN),
        "member with an unresolved value type must still report its declaration file"
    );

    // NAME span: present + slices to `present` in BROKEN.
    let name_span = present.name_span.as_ref().expect(
        "member `present` must carry a NAME span even though its value type is unresolved \
         (the declaration site is real; a `None` here is the masking defect)",
    );
    assert_eq!(slice(src, name_span, BROKEN), "present");

    // TYPE-ANNOTATION span: present + slices to the (unresolved) `MissingType`.
    let ann_span = present.type_annotation_span.as_ref().expect(
        "member `present` must carry a TYPE-ANNOTATION span anchored to its declaration file",
    );
    assert_eq!(
        slice(src, ann_span, BROKEN),
        "MissingType",
        "the annotation span slices to the unresolved type name in the declaration file"
    );

    // DECLARATION span: present + covers `present: MissingType`.
    let decl_span = present
        .origin
        .declaration_span
        .as_ref()
        .expect("member `present` must carry a DECLARATION span");
    let decl_text = slice(src, decl_span, BROKEN);
    assert!(
        decl_text.starts_with("present") && decl_text.contains("MissingType"),
        "declaration span must cover `present: MissingType`; got {decl_text:?}"
    );
}

// ---------------------------------------------------------------------------
// (10) INDEX-SIGNATURE declaration-origin span loss (codex#1 P1): an index
//      signature whose key AND value nodes are SCOPE-LESS (`Global`) still
//      reports its real decl / key / value spans anchored to its DECLARATION
//      file, taken from `IndexSignature::declaration_origin`.
//
// This drives `TypeInfoSurface::build` → `build_index_signature` directly with
// a hand-built graph `IndexSignature`: the key + value nodes are interned
// scope-less (so `node_origin_file(value)` / `node_origin_file(key)` BOTH yield
// `None` — the pre-fix heuristic), but the payload carries real `spans` + a real
// `declaration_origin`. Pre-fix `build_index_signature` ignored
// `declaration_origin` and anchored to the value/key node scope → `None` → the
// spans were masked to `None` (FAIL). Post-fix it uses `declaration_origin` →
// the declaration file (PASS). Building the payload directly is the honest test:
// a normally-lowered inline `[k: string]: V` interns its `string` key WITH the
// declaring scope, which would mask the defect end-to-end.
// ---------------------------------------------------------------------------

#[test]
fn index_signature_build_uses_declaration_origin_for_scopeless_nodes() {
    use crate::semantic_query::{IndexSignature, SemanticNodeData, SurfaceView};
    use verter_span::Span;

    const FILE: &str = "/src/idx_decl.ts";

    let host = make_host_with_footprint();
    let graph_store_view = host.resolver_store_view();
    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx =
        crate::resolver_core::HostResolverContext::new(&host, &graph_store_view, overlay);
    let graph = {
        use crate::resolver_core::ResolverContext;
        host_ctx.project_type_store().semantic_graph()
    };

    // SCOPE-LESS key + value nodes (interned via the unscoped `intern_node` →
    // `NodeScopeId::Global` → `node_scope` is `None`).
    let key_node = graph.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    let value_node = graph.intern_node(SemanticNodeData::Opaque(
        crate::semantic_query::QueryError::Miss,
    ));
    // Precondition: BOTH key + value nodes yield NO origin file, so the pre-fix
    // `node_origin_file(value).or(key)` fallback chain yields `None`.
    assert!(
        graph
            .node_scope(value_node)
            .and_then(|s| s.canonical_file())
            .is_none()
            && graph
                .node_scope(key_node)
                .and_then(|s| s.canonical_file())
                .is_none(),
        "test precondition: key + value nodes must yield no origin file (the pre-fix path)"
    );

    let sig = IndexSignature {
        key_type: key_node,
        value_type: value_node,
        readonly: false,
        spans: verter_type_expr::IndexSignatureSpans {
            declaration: Some(Span::new(0, 24)),
            key: Some(Span::new(1, 10)),
            value: Some(Span::new(13, 23)),
        },
        // The index signature's DECLARATION file — set from the lowering scope
        // at production time; the only correct span anchor.
        declaration_origin: Some(std::sync::Arc::from(FILE)),
    };
    let view = SurfaceView {
        members: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        call_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: std::sync::Arc::from(vec![sig].into_boxed_slice()),
        keyspace: None,
        has_index_signature: true,
    };

    let surface = TypeInfoSurface::build(graph, &view);
    assert_eq!(surface.index_signatures.len(), 1);
    let idx = &surface.index_signatures[0];

    // DECL / KEY / VALUE spans are all present and anchored to the DECLARATION
    // file — NOT `None` (the masking defect), NOT a value/key node file.
    let decl = idx
        .declaration_span
        .as_ref()
        .expect("index signature must carry a DECLARATION span via declaration_origin");
    assert_eq!(
        decl.file.as_ref(),
        FILE,
        "declaration span must anchor to the declaration file, not the scope-less value node"
    );
    assert_eq!(decl.span, Span::new(0, 24));

    let key = idx
        .key_span
        .as_ref()
        .expect("index signature must carry a KEY span via declaration_origin");
    assert_eq!(key.file.as_ref(), FILE);
    assert_eq!(key.span, Span::new(1, 10));

    let value = idx
        .value_span
        .as_ref()
        .expect("index signature must carry a VALUE span via declaration_origin");
    assert_eq!(value.file.as_ref(), FILE);
    assert_eq!(value.span, Span::new(13, 23));
}

// ---------------------------------------------------------------------------
// (10b) MEMBER declaration-origin span loss — `build_member` consumption unit
//       test. Mirrors (10): a member with a SCOPE-LESS value node but a real
//       `declaration_origin` reports its spans anchored to the declaration
//       file. This is the same consumption the prepared-member append
//       (`build.rs` `backfill_member_index_surface`) feeds — that overlay now
//       stamps `declaration_origin` from `PreparedMember`, and this proves the
//       projection honours it for a scope-less value. FAILS pre-fix (value
//       node scope `None` → spans `None`), PASSES post-fix.
// ---------------------------------------------------------------------------

#[test]
fn member_build_uses_declaration_origin_for_scopeless_value() {
    use crate::semantic_query::{MemberMergeRole, SemanticNodeData, SurfaceMember, SurfaceView};
    use verter_span::Span;

    const FILE: &str = "/src/member_decl.ts";

    let host = make_host_with_footprint();
    let graph_store_view = host.resolver_store_view();
    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx =
        crate::resolver_core::HostResolverContext::new(&host, &graph_store_view, overlay);
    let graph = {
        use crate::resolver_core::ResolverContext;
        host_ctx.project_type_store().semantic_graph()
    };

    // SCOPE-LESS value node — `node_origin_file(value)` is `None` (pre-fix path).
    let value_node = graph.intern_node(SemanticNodeData::Opaque(
        crate::semantic_query::QueryError::Miss,
    ));
    // Precondition: the value node yields NO origin file (the pre-fix path).
    // `intern_node` (unscoped) → `Global` scope → `node_scope` is `None` OR
    // `Some(Global)`; either way the value-node-origin fallback is `None`.
    assert!(
        graph
            .node_scope(value_node)
            .and_then(|s| s.canonical_file())
            .is_none(),
        "test precondition: value node must yield no origin file (scope-less / Global)"
    );

    let built_member = SurfaceMember {
        name: std::sync::Arc::from("present"),
        value: value_node,
        optional: false,
        readonly: false,
        is_method: false,
        spans: verter_type_expr::MemberSpans {
            declaration: Some(Span::new(0, 20)),
            name: Some(Span::new(0, 7)),
            type_annotation: Some(Span::new(9, 20)),
        },
        // The member's DECLARATION file — the only correct anchor for a member
        // whose value is scope-less.
        declaration_origin: Some(std::sync::Arc::from(FILE)),
        declared_in_macro_type_arg: false,
        merge_role: MemberMergeRole::OwnBody,
    };
    let view = SurfaceView {
        members: std::sync::Arc::from(vec![built_member].into_boxed_slice()),
        call_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    };

    let surface = TypeInfoSurface::build(graph, &view);
    let present = member(&surface, "present");
    assert_eq!(
        present.origin.canonical_file.as_deref(),
        Some(FILE),
        "member origin must come from declaration_origin, not the scope-less value node"
    );
    let name = present
        .name_span
        .as_ref()
        .expect("member with scope-less value must still carry a NAME span via declaration_origin");
    assert_eq!(name.file.as_ref(), FILE);
    assert_eq!(name.span, Span::new(0, 7));
    let ann = present
        .type_annotation_span
        .as_ref()
        .expect("member must carry a TYPE-ANNOTATION span via declaration_origin");
    assert_eq!(ann.file.as_ref(), FILE);
    assert_eq!(ann.span, Span::new(9, 20));
}

// ---------------------------------------------------------------------------
// (11) JSDoc as SPANS on the surface (U2-2): a member's leading `/** doc */`
//      block is carried as a description span (+ per-tag spans), sliced from
//      the DECLARING file. The surface holds NO owned JSDoc string.
//
// FAILS pre-U2 (the surface carried no JSDoc fields at all), PASSES post-fix:
// `jsdoc_description_span` slices to the exact doc text in the declaring file.
// ---------------------------------------------------------------------------

#[test]
fn member_jsdoc_description_span_slices_to_exact_doc_text() {
    let host = make_host_with_footprint();
    upsert_ts(&host, FILE, SHALLOW_SURFACE_FACTS);

    let surface = host
        .resolve_shallow_surface(FILE, "DocumentedSurface")
        .expect("DocumentedSurface must resolve to a one-level surface");

    // `documented` has `/** the documented field */`.
    let documented = member(&surface, "documented");
    let desc_span = documented.jsdoc_description_span.as_ref().expect(
        "`documented` must carry a JSDoc description span (it has a leading `/** */` block)",
    );
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, desc_span, FILE),
        "the documented field",
        "description span must slice to the exact doc text from the declaring file"
    );

    // NEGATIVE: the description span must NOT slice to the member name / type.
    assert_ne!(
        slice(SHALLOW_SURFACE_FACTS, desc_span, FILE),
        "documented",
        "description span must not slice to the member name"
    );

    // `undocumented` has no leading JSDoc → no description span.
    let undocumented = member(&surface, "undocumented");
    assert!(
        undocumented.jsdoc_description_span.is_none(),
        "a member with no leading JSDoc must carry NO description span (a `Some` here is a \
         false-positive attach)"
    );
    assert!(
        undocumented.jsdoc_tag_spans.is_empty(),
        "an undocumented member carries no tag spans"
    );
}

#[test]
fn member_jsdoc_tag_spans_slice_to_exact_tag_tokens() {
    let host = make_host_with_footprint();
    upsert_ts(&host, FILE, SHALLOW_SURFACE_FACTS);

    let surface = host
        .resolve_shallow_surface(FILE, "DocumentedSurface")
        .expect("DocumentedSurface must resolve");

    // `tagged` has a multi-line description AND `@deprecated use somethingElse`.
    let tagged = member(&surface, "tagged");
    let desc_span = tagged
        .jsdoc_description_span
        .as_ref()
        .expect("`tagged` must carry a description span");
    let desc_text = slice(SHALLOW_SURFACE_FACTS, desc_span, FILE);
    assert!(
        desc_text.contains("multi-line description here."),
        "description span must cover the multi-line description; got {desc_text:?}"
    );
    // The description span must STOP before the first tag (it must not swallow
    // the `@deprecated` line).
    assert!(
        !desc_text.contains("@deprecated"),
        "description span must end before the first tag; got {desc_text:?}"
    );

    // Exactly one tag (`@deprecated`).
    assert_eq!(
        tagged.jsdoc_tag_spans.len(),
        1,
        "`tagged` carries exactly one tag (`@deprecated`); got {:?}",
        tagged
            .jsdoc_tag_spans
            .iter()
            .map(|t| slice(SHALLOW_SURFACE_FACTS, &t.name_span, FILE))
            .collect::<Vec<_>>()
    );
    let tag = &tagged.jsdoc_tag_spans[0];
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, &tag.name_span, FILE),
        "deprecated",
        "tag name span slices to the bare tag name (no `@`)"
    );
    let text_span = tag
        .text_span
        .as_ref()
        .expect("`@deprecated use somethingElse` carries tag text");
    assert_eq!(
        slice(SHALLOW_SURFACE_FACTS, text_span, FILE),
        "use somethingElse",
        "tag text span slices to the exact tag text"
    );
}

// NOTE on the prepared-member append path (`build.rs`
// `backfill_member_index_surface`, the codex#2 P1 / Claude P2-b front): the
// overlay's APPEND branch (which copies each `PreparedMember`'s `spans` +
// `declaration_origin` onto the appended `SurfaceMember`) is exercised
// directly by `project_semantic_dispatch::tests`'
// `backfill_member_index_surface_carries_prepared_member_spans_and_origin`,
// which interns an empty Object surface, supplies a prepared member with
// NON-default spans + origin, and asserts the appended member carries them
// (it FAILS if the transfer reverts to `MemberSpans::default()` /
// `declaration_origin: None`). The PRODUCER side — `PreparedMember` carrying
// `spans` + `declaration_origin`, stamped at `build_member_index` — is
// discriminated separately by `verter_semantic`'s
// `prepared_member_index_carries_spans_and_declaration_origin`. The
// scope-less-value projection consumed by `build_member` is covered by
// `member_build_uses_declaration_origin_for_scopeless_value` above.
