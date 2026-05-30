#![deny(missing_docs)]
//! Span-rich, typeinfo-owned one-level surface value.
//!
//! [`TypeInfoSurface`] is the typeinfo-owned PUBLIC projection of the shared
//! semantic graph's one-level surface ([`SurfaceView`]). It is a THIN
//! projection — it reads the span-rich GRAPH payloads
//! ([`SurfaceMember::spans`], [`SemanticNodeData::Function`]'s
//! `signature_span` / `return_type_span`, [`IndexSignature::spans`]) and pairs
//! each span with its DECLARATION file. For a named member / index signature
//! that file is the member's own `declaration_origin` (stamped from the
//! lowering scope of the declaring object), so a member whose VALUE type is
//! unresolved / scope-less still reports its real declaration file; for a
//! signature it is the signature node's canonical origin file (from
//! [`SemanticGraphStore::node_scope`]). It does NOT recompute meaning, does NOT
//! re-resolve types, and does NOT scan source text.
//!
//! Architectural rule (CLAUDE.md — "typeinfo carries SPANS, never owned
//! `String` type text"): every field is a span, an id, a flag, or an interned
//! `Arc<str>` name. A consumer slices the source at the span on demand at the
//! FFI / consumer boundary. This mirrors Verter's `CodeTransform` discipline.
//!
//! Spans originate at the OXC lowering sites (once, during shallow analysis)
//! and travel verbatim through the `verter_type_expr` IR into the graph
//! payloads. A `None` span here means the underlying fact is genuinely
//! synthetic (a union common-member, a mapped-produced member, a composed
//! signature) with no single source declaration site — never a "not
//! implemented" placeholder.

use std::sync::Arc;

use verter_span::Span;

use crate::semantic_query::{
    IndexSignature, MemberMergeRole, NodeScopeId, SemanticNodeData, SemanticNodeId, SurfaceMember,
    SurfaceView,
};
use crate::semantic_query_memo::SemanticGraphStore;

/// A byte-offset span anchored to a canonical file.
///
/// [`verter_span::Span`] alone is the `[start, end)` offset pair with no file
/// identity; a surface member's origin can be a DIFFERENT file from the
/// declaration that referenced it (an inherited member originates in its
/// heritage base's file), so the span MUST carry the canonical file id. The
/// offsets are FILE-ABSOLUTE (SFC-absolute for `.vue`): a `.vue` file's eval
/// source is position-preserving — script content sits at its raw SFC byte
/// offsets — so the OXC lowering stamps every span in the raw-file coordinate
/// system, the same coordinates a raw-`.vue` consumer slices.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalSpan {
    /// Canonical file id the span's offsets index into.
    pub file: Arc<str>,
    /// `[start, end)` byte offsets in that file's source.
    pub span: Span,
}

impl CanonicalSpan {
    /// Pair a `verter_span::Span` with the canonical file it indexes into.
    #[must_use]
    pub fn new(file: Arc<str>, span: Span) -> Self {
        Self { file, span }
    }

    /// Pair a span with a file id, when both the file and the span are present.
    /// `None` when either component is absent (a synthetic / multi-origin fact).
    fn from_parts(file: Option<&Arc<str>>, span: Option<Span>) -> Option<Self> {
        match (file, span) {
            (Some(file), Some(span)) => Some(Self::new(Arc::clone(file), span)),
            _ => None,
        }
    }
}

/// How a [`TypeInfoSurfaceMember`] arrived on the surface — the typed origin +
/// merge role (codex `SurfaceMemberOrigin`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SurfaceMemberOrigin {
    /// The canonical file the member's DECLARATION lives in — its `name` /
    /// `: T` annotation site, taken from the graph member's
    /// `declaration_origin` (set from the LOWERING scope of the object that
    /// declares the member). For an inherited member this is the heritage
    /// base's file, not the consuming declaration's. Crucially this is NOT the
    /// member's VALUE-node scope: a member whose value is an unresolved /
    /// scope-less node still reports its real declaration file. `None` only for
    /// a genuinely synthetic / multi-origin member (a union common-member, a
    /// mapped-produced member) with no single declaration file.
    pub canonical_file: Option<Arc<str>>,
    /// Span of the member's whole declaration in `canonical_file`, when the
    /// graph member recorded one. `None` for a synthetic member.
    pub declaration_span: Option<CanonicalSpan>,
    /// The surface-merge role of the member (own-body / heritage / authored).
    /// Drives the own-body-shadows-heritage semantics; surfaced so consumers
    /// can distinguish a derived member from an inherited one.
    pub merge_role: MemberMergeRole,
}

/// One JSDoc tag on a [`TypeInfoSurfaceMember`], carried as SPANS into the
/// declaring file (never owned `String`). A consumer slices `name_span` for the
/// tag name (without the leading `@`) and `text_span` for the tag text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JsdocTagSpan {
    /// Span of the tag NAME (the identifier after `@`, e.g. `deprecated`), in
    /// the declaring file's coordinates.
    pub name_span: CanonicalSpan,
    /// Span of the tag TEXT (everything after the tag name on the tag's
    /// line(s)), when the tag carries text. `None` for a bare tag (`@internal`).
    pub text_span: Option<CanonicalSpan>,
}

/// One member of a [`TypeInfoSurface`].
///
/// `value` is a reference-style [`SemanticNodeId`] under the shallow-by-default
/// rule — the member's body is NOT eagerly expanded. A consumer that needs the
/// body issues a path projection rooted at `value`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeInfoSurfaceMember {
    /// Interned member name.
    pub name: Arc<str>,
    /// Span of the member's NAME at its declaration site (in the origin file's
    /// coordinates). `None` when the member is synthesized (a union
    /// common-member, a mapped-produced member) and has no single source site.
    pub name_span: Option<CanonicalSpan>,
    /// Shallow value node (a `SemanticNodeId`, never an expanded body).
    pub value: SemanticNodeId,
    /// Span of the member's TYPE ANNOTATION at its declaration site, when the
    /// graph member recorded one. `None` for synthesized members or method-
    /// style members (no `: T` annotation).
    pub type_annotation_span: Option<CanonicalSpan>,
    /// `?`-optional member.
    pub optional: bool,
    /// `readonly` member.
    pub readonly: bool,
    /// Method-style member (`name(): T`) vs property-style (`name: T`).
    pub is_method: bool,
    /// Whether the member was declared in a Vue macro type argument's OWN body
    /// (vs reached via heritage / Omit / intersection). Display/provenance
    /// flag carried verbatim from the graph member.
    pub declared_in_macro_type_arg: bool,
    /// Span of the member's leading JSDoc DESCRIPTION text (the comment body
    /// before the first `@tag`), in the member's DECLARATION file. `None` when
    /// the member has no leading JSDoc, or its JSDoc has only tags and no
    /// description. Carries a SPAN only — the consumer slices the doc text on
    /// demand (no owned `String` on the surface). Populated at the host accessor
    /// layer from the member's [`SurfaceMemberOrigin::canonical_file`] + the
    /// member declaration span; [`TypeInfoSurface::build`] leaves it `None`
    /// because the pure graph projection holds no source.
    pub jsdoc_description_span: Option<CanonicalSpan>,
    /// Spans of the member's leading JSDoc TAGS (`@deprecated`, `@param`, …), in
    /// declaration order. Empty when the member has no leading JSDoc or no tags.
    /// Each entry carries the tag name + text spans (never owned `String`).
    pub jsdoc_tag_spans: Arc<[JsdocTagSpan]>,
    /// Typed origin + merge role.
    pub origin: SurfaceMemberOrigin,
}

/// One call (`(args): ret`) or construct (`new (args): ret`) signature on a
/// [`TypeInfoSurface`]. Carries the signature node id plus the signature /
/// parameter / return-type spans recorded on the graph `Function` node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeInfoSurfaceSignature {
    /// The `Function`-shaped signature node.
    pub node: SemanticNodeId,
    /// Span of the whole signature in the origin file, when the graph node
    /// recorded one.
    pub signature_span: Option<CanonicalSpan>,
    /// Spans of each parameter, in declaration order. An entry is `None` when
    /// that parameter had no recorded span (a synthetic parameter).
    pub parameter_spans: Arc<[Option<CanonicalSpan>]>,
    /// Span of the return-type annotation, when present.
    pub return_type_span: Option<CanonicalSpan>,
    /// Span of the signature's leading JSDoc DESCRIPTION, sliced from the
    /// signature's DECLARATION file. `None` when the signature has no leading
    /// JSDoc or no recorded signature span to anchor the search. A
    /// call-signature emit (`(e: 'change', v: T): void`) carries its JSDoc here
    /// — the event's `description` is read from this span (symmetric with a
    /// property-style emit member's [`TypeInfoSurfaceMember::jsdoc_description_span`]).
    pub jsdoc_description_span: Option<CanonicalSpan>,
    /// Spans of the signature's leading JSDoc TAGS (`@deprecated`, `@param`, …),
    /// in declaration order. Empty when the signature has no leading JSDoc or no
    /// tags. Each entry carries the tag name + text spans (never owned `String`).
    pub jsdoc_tag_spans: Arc<[JsdocTagSpan]>,
}

/// One index signature (`[k: K]: V` / `readonly [k: K]: V`) on a
/// [`TypeInfoSurface`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeInfoIndexSignature {
    /// Key-type node.
    pub key_type: SemanticNodeId,
    /// Value-type node.
    pub value_type: SemanticNodeId,
    /// Span of the key declaration (`[k: K]`), when recorded.
    pub key_span: Option<CanonicalSpan>,
    /// Span of the value-type annotation, when recorded.
    pub value_span: Option<CanonicalSpan>,
    /// Span of the whole index-signature declaration, when recorded.
    pub declaration_span: Option<CanonicalSpan>,
    /// `readonly` index signature.
    pub readonly: bool,
}

/// A span-rich, typeinfo-owned one-level surface.
///
/// Built FROM a graph [`SurfaceView`] plus the graph's per-node scope. Holds
/// NO owned type / display strings — only spans, ids, flags, and interned
/// names. `Clone` + `Send + Sync` (all fields are `Arc`-backed or `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeInfoSurface {
    /// Named members in declaration order.
    pub members: Arc<[TypeInfoSurfaceMember]>,
    /// Call signatures in declaration order.
    pub call_signatures: Arc<[TypeInfoSurfaceSignature]>,
    /// Construct signatures in declaration order.
    pub construct_signatures: Arc<[TypeInfoSurfaceSignature]>,
    /// Index signatures.
    pub index_signatures: Arc<[TypeInfoIndexSignature]>,
    /// Keyspace node, when the surface is a mapped/keyspace carrier.
    pub keyspace: Option<SemanticNodeId>,
    /// Whether the surface has at least one index signature.
    pub has_index_signature: bool,
}

impl TypeInfoSurface {
    /// An empty surface — no members, signatures, index signatures, or
    /// keyspace. Used by the Vue adapter for `defineModel`, whose props come
    /// from analyzer facts rather than a type-argument object surface (the
    /// macro carries no one-level member surface), so the adapter pairs the
    /// analyzer-fact prop with an empty member surface.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }
    }

    /// Build a span-rich surface from a graph [`SurfaceView`].
    ///
    /// THIN projection: every span comes from the graph payload that already
    /// carries it (stamped once at the OXC lowering site and interned on the
    /// node), paired with the node's canonical origin file from
    /// [`SemanticGraphStore::node_scope`]. No source text is scanned and no
    /// type is re-resolved.
    #[must_use]
    pub fn build(graph: &SemanticGraphStore, view: &SurfaceView) -> Self {
        let members: Vec<TypeInfoSurfaceMember> = view
            .members
            .iter()
            .map(|member| build_member(graph, member))
            .collect();
        let call_signatures: Vec<TypeInfoSurfaceSignature> = view
            .call_signatures
            .iter()
            .map(|node| build_signature(graph, *node))
            .collect();
        let construct_signatures: Vec<TypeInfoSurfaceSignature> = view
            .construct_signatures
            .iter()
            .map(|node| build_signature(graph, *node))
            .collect();
        let index_signatures: Vec<TypeInfoIndexSignature> = view
            .index_signatures
            .iter()
            .map(|sig| build_index_signature(graph, sig))
            .collect();
        Self {
            members: Arc::from(members.into_boxed_slice()),
            call_signatures: Arc::from(call_signatures.into_boxed_slice()),
            construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
            index_signatures: Arc::from(index_signatures.into_boxed_slice()),
            keyspace: view.keyspace,
            has_index_signature: view.has_index_signature,
        }
    }

    /// Enrich each member AND each call / construct signature with its
    /// leading-JSDoc DESCRIPTION + TAG spans, sliced from the declaring file.
    ///
    /// `build` is a pure graph projection that holds no source, so JSDoc spans
    /// (which require locating the leading `/** */` block in the declaring file)
    /// are populated HERE, at the host accessor layer that can read the source.
    /// `source_for(canonical)` returns the cache-owned source of a canonical
    /// file (the host passes `IndexedReady.raw_source`, against which member /
    /// signature spans are SFC-absolute), or `None` when it is unavailable.
    ///
    /// Each member's JSDoc is located via its DECLARATION origin
    /// ([`SurfaceMemberOrigin::canonical_file`], which U1 made survive
    /// substitution) + the member's name-token offset — so an INHERITED member's
    /// JSDoc is read from the heritage base's file (the member's real
    /// declaration), NOT the consuming declaration's file (P2-2). Members
    /// without a name span (synthetic / multi-origin) or without a declaration
    /// file are left without JSDoc spans. Carries SPANS only — no owned
    /// `String`.
    #[must_use]
    pub fn with_member_jsdoc_spans<F>(self, source_for: F) -> Self
    where
        F: Fn(&str) -> Option<Arc<str>>,
    {
        use std::collections::HashMap;

        // Cache one source read per declaration file across members.
        let mut sources: HashMap<Arc<str>, Option<Arc<str>>> = HashMap::new();

        let members: Vec<TypeInfoSurfaceMember> = self
            .members
            .iter()
            .map(|member| {
                let enriched = (|| {
                    let file = member.origin.canonical_file.as_ref()?;
                    // The member's name-token offset anchors the leading-JSDoc
                    // search in its DECLARATION file.
                    let name_span = member.name_span.as_ref()?;
                    let source = sources
                        .entry(Arc::clone(file))
                        .or_insert_with(|| source_for(file.as_ref()))
                        .clone()?;
                    let spans = verter_semantic::analysis::jsdoc::jsdoc_block_spans_at_offset(
                        source.as_ref(),
                        name_span.span.start,
                    )?;
                    let description_span = spans
                        .description
                        .map(|span| CanonicalSpan::new(Arc::clone(file), span));
                    let tag_spans: Vec<JsdocTagSpan> = spans
                        .tags
                        .into_iter()
                        .map(|tag| JsdocTagSpan {
                            name_span: CanonicalSpan::new(Arc::clone(file), tag.name),
                            text_span: tag
                                .text
                                .map(|span| CanonicalSpan::new(Arc::clone(file), span)),
                        })
                        .collect();
                    Some((description_span, tag_spans))
                })();

                match enriched {
                    Some((description_span, tag_spans)) => TypeInfoSurfaceMember {
                        jsdoc_description_span: description_span,
                        jsdoc_tag_spans: Arc::from(tag_spans.into_boxed_slice()),
                        ..member.clone()
                    },
                    None => member.clone(),
                }
            })
            .collect();

        // Enrich each call / construct SIGNATURE with its leading-JSDoc spans,
        // anchored at the signature's own span in its declaration file — so a
        // call-signature emit (`(e: 'change', v: T): void`) carries the JSDoc
        // that documents the event, symmetric with a property-style member. An
        // inherited cross-file signature's JSDoc is read from the heritage
        // base's file (the signature's spans index into THAT file).
        let mut enrich_signature = |sig: &TypeInfoSurfaceSignature| -> TypeInfoSurfaceSignature {
            let enriched = (|| {
                let anchor = sig.signature_span.as_ref()?;
                let file = anchor.file.as_ref();
                let source = sources
                    .entry(Arc::clone(&anchor.file))
                    .or_insert_with(|| source_for(file))
                    .clone()?;
                let spans = verter_semantic::analysis::jsdoc::jsdoc_block_spans_at_offset(
                    source.as_ref(),
                    anchor.span.start,
                )?;
                let description_span = spans
                    .description
                    .map(|span| CanonicalSpan::new(Arc::clone(&anchor.file), span));
                let tag_spans: Vec<JsdocTagSpan> = spans
                    .tags
                    .into_iter()
                    .map(|tag| JsdocTagSpan {
                        name_span: CanonicalSpan::new(Arc::clone(&anchor.file), tag.name),
                        text_span: tag
                            .text
                            .map(|span| CanonicalSpan::new(Arc::clone(&anchor.file), span)),
                    })
                    .collect();
                Some((description_span, tag_spans))
            })();

            match enriched {
                Some((description_span, tag_spans)) => TypeInfoSurfaceSignature {
                    jsdoc_description_span: description_span,
                    jsdoc_tag_spans: Arc::from(tag_spans.into_boxed_slice()),
                    ..sig.clone()
                },
                None => sig.clone(),
            }
        };
        let call_signatures: Vec<TypeInfoSurfaceSignature> = self
            .call_signatures
            .iter()
            .map(&mut enrich_signature)
            .collect();
        let construct_signatures: Vec<TypeInfoSurfaceSignature> = self
            .construct_signatures
            .iter()
            .map(&mut enrich_signature)
            .collect();

        Self {
            members: Arc::from(members.into_boxed_slice()),
            call_signatures: Arc::from(call_signatures.into_boxed_slice()),
            construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
            ..self
        }
    }
}

/// The canonical origin file a node was first lowered in, when it is a
/// single-declaration-file scope. `None` for `Global` / non-file scopes (a
/// primitive, a structural composed node).
fn node_origin_file(graph: &SemanticGraphStore, node: SemanticNodeId) -> Option<Arc<str>> {
    match graph.node_scope(node) {
        Some(NodeScopeId::File { canonical_id, .. }) => Some(canonical_id),
        _ => None,
    }
}

fn build_member(graph: &SemanticGraphStore, member: &SurfaceMember) -> TypeInfoSurfaceMember {
    // The member's spans are in its DECLARATION file — the file the object
    // declaring this member was lowered in (`SurfaceMember::declaration_origin`,
    // set from the lowering scope). For a cross-file inherited member that is
    // the heritage base's file (the member is declared there), not the
    // consuming declaration's. This is NOT the member's VALUE-node scope: a
    // member whose value is an unresolved / scope-less node
    // (`{ present: MissingType }`) still has a real declaration file, so its
    // real spans must NOT be masked to `None`. Fall back to the value node's
    // origin only when the member carries no declaration origin (a synthetic
    // member also has no spans, so the pairing still yields `None`).
    let canonical = member
        .declaration_origin
        .clone()
        .or_else(|| node_origin_file(graph, member.value));
    let name_span = CanonicalSpan::from_parts(canonical.as_ref(), member.spans.name);
    let declaration_span = CanonicalSpan::from_parts(canonical.as_ref(), member.spans.declaration);
    let type_annotation_span =
        CanonicalSpan::from_parts(canonical.as_ref(), member.spans.type_annotation);

    TypeInfoSurfaceMember {
        name: Arc::clone(&member.name),
        name_span,
        value: member.value,
        type_annotation_span,
        optional: member.optional,
        readonly: member.readonly,
        is_method: member.is_method,
        declared_in_macro_type_arg: member.declared_in_macro_type_arg,
        // JSDoc spans require the declaring file's source to locate the leading
        // comment block, which the pure graph projection does NOT hold. The host
        // accessor (`resolve_shallow_surface`) enriches these after `build` via
        // `TypeInfoSurface::with_member_jsdoc_spans`. A pure-graph build leaves
        // them empty — never a "not implemented" placeholder.
        jsdoc_description_span: None,
        jsdoc_tag_spans: Arc::from(Vec::new().into_boxed_slice()),
        origin: SurfaceMemberOrigin {
            canonical_file: canonical,
            declaration_span,
            merge_role: member.merge_role,
        },
    }
}

fn build_signature(graph: &SemanticGraphStore, node: SemanticNodeId) -> TypeInfoSurfaceSignature {
    let canonical = node_origin_file(graph, node);
    let (signature_span, parameter_spans, return_type_span) = match graph.node_data(node) {
        Some(data) => match &*data {
            SemanticNodeData::Function {
                params,
                signature_span,
                return_type_span,
                ..
            } => {
                let sig = CanonicalSpan::from_parts(canonical.as_ref(), *signature_span);
                let ret = CanonicalSpan::from_parts(canonical.as_ref(), *return_type_span);
                let param_spans: Vec<Option<CanonicalSpan>> = params
                    .iter()
                    .map(|p| CanonicalSpan::from_parts(canonical.as_ref(), p.span))
                    .collect();
                (sig, param_spans, ret)
            }
            _ => (None, Vec::new(), None),
        },
        None => (None, Vec::new(), None),
    };

    TypeInfoSurfaceSignature {
        node,
        signature_span,
        parameter_spans: Arc::from(parameter_spans.into_boxed_slice()),
        return_type_span,
        // JSDoc spans require the declaring file's source to locate the leading
        // comment block, which the pure graph projection does NOT hold. The host
        // accessor enriches these after `build` via
        // `TypeInfoSurface::with_member_jsdoc_spans`. A pure-graph build leaves
        // them empty — never a "not implemented" placeholder.
        jsdoc_description_span: None,
        jsdoc_tag_spans: Arc::from(Vec::new().into_boxed_slice()),
    }
}

fn build_index_signature(
    graph: &SemanticGraphStore,
    sig: &IndexSignature,
) -> TypeInfoIndexSignature {
    // The index signature's declaration / key / value spans are in the file
    // the declaring object was lowered in (`IndexSignature::declaration_origin`,
    // set from the lowering scope) — NOT the value-type node's scope, which is
    // `None` for a scope-less value (`[k: string]: MissingType`). Fall back to
    // the value / key node origin only when no declaration origin is recorded.
    let canonical = sig
        .declaration_origin
        .clone()
        .or_else(|| node_origin_file(graph, sig.value_type))
        .or_else(|| node_origin_file(graph, sig.key_type));
    TypeInfoIndexSignature {
        key_type: sig.key_type,
        value_type: sig.value_type,
        key_span: CanonicalSpan::from_parts(canonical.as_ref(), sig.spans.key),
        value_span: CanonicalSpan::from_parts(canonical.as_ref(), sig.spans.value),
        declaration_span: CanonicalSpan::from_parts(canonical.as_ref(), sig.spans.declaration),
        readonly: sig.readonly,
    }
}
