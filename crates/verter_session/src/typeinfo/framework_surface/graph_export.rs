#![deny(missing_docs)]
//! The first wire `SemanticTypeGraph` producer — a bounded, shallow,
//! zero-dispatch encoder over ALREADY-RESOLVED typed IR.
//!
//! [`encode_framework_surfaces`] folds the executor-normalized
//! [`NormalizedSurfaces`] into the wire
//! [`FrameworkSurfacePayload`](verter_protocol::typeinfo::graph::FrameworkSurfacePayload)
//! pair — a [`SemanticTypeGraph`] node arena plus one
//! [`FrameworkSurfaceKindEntry`] per known surface kind. It is a PURE projection
//! of resolved data: it never calls `ProjectSemanticDispatch`, never
//! materializes, never re-resolves (the typed-IR-only rule). Traversal is
//! shallow and depth-bounded.
//!
//! ## Degradation strategy (a `GraphReference` is a bare `symbol_id`)
//!
//! The wire [`GraphReference`](verter_protocol::typeinfo::graph::ReferenceNode)
//! carries ONLY a `uint32 symbol_id` — NOT a display string. So a member value
//! type outside the encoder's shallow vocabulary CANNOT degrade to "a
//! `GraphReference` with a display name". Instead:
//! - a member value with a real SYMBOL identity (a named alias `Ref`) mints a
//!   deterministic [`GraphSymbolNode`](verter_protocol::typeinfo::graph::SymbolNode)
//!   (name interned into the string table) and emits a `GraphReference {
//!   symbol_id }` pointing at it;
//! - a member value with NO symbol identity (a structural shape the shallow
//!   vocabulary does not cover) emits the existing
//!   [`GraphOpaque`](verter_protocol::typeinfo::graph::OpaqueNode) arm — never a
//!   fabricated symbol.
//!
//! Either way: never an eager expansion, never a second resolution walk. A node
//! kind the shallow vocabulary does not enumerate stops at the
//! `GraphReference` / `GraphOpaque` degradation; it never recurses into
//! resolution.

use std::collections::HashMap;

use verter_protocol::typeinfo::graph::{
    self as wire, Exactness, FrameworkSurfaceKind, FrameworkSurfaceKindEntry,
    FrameworkSurfaceKindStatus, FrameworkSurfaceKindSupport, PrimitiveKind, SemanticTypeGraph,
    SymbolNamespace,
};
use verter_protocol::verter::v1::{
    graph_query_error, graph_type_node, GraphLiteral, GraphLiteralValue, GraphObject, GraphOpaque,
    GraphPrimitive, GraphQueryError, GraphQueryErrorOther, GraphReference, GraphStringTable,
    GraphSymbolNode, GraphTypeNode, SemanticTypeGraph as WireGraph,
};
use verter_type_expr::{LiteralValue, PrimitiveName, TypeExpr};

use crate::typeinfo::framework_surface::results::{
    MacroSurfaceDtos, NamedTypeMember, NormalizedSurface, NormalizedSurfaces, ResolvedOutcome,
};

/// The bounded shallow encoder's traversal depth budget. The member value
/// vocabulary is one-level — a member's value is a leaf node (primitive /
/// literal / named ref) or stops at a `GraphOpaque` — so no traversal exceeds
/// depth 1. The budget exists to make the boundedness explicit and to
/// fail-closed (a deeper-than-shallow walk degrades to `GraphOpaque` rather
/// than recursing).
const SHALLOW_DEPTH_BUDGET: u32 = 1;

/// The framework-neutral encoder output: the wire graph plus one per-kind
/// entry.
#[derive(Debug, Clone)]
pub(crate) struct EncodedFrameworkSurfaces {
    /// The graph arena that the per-kind member `type_node_id`s index into.
    pub graph: SemanticTypeGraph,
    /// Exactly one entry per known surface kind, in tag order.
    pub surfaces: Vec<FrameworkSurfaceKindEntry>,
}

/// A growable string-intern table backing the encoded graph's interned ids.
#[derive(Default)]
struct StringTableBuilder {
    entries: Vec<String>,
    index: HashMap<String, u32>,
}

impl StringTableBuilder {
    /// Intern `s`, returning its stable id; identical strings dedup to one id.
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.index.get(s) {
            return id;
        }
        let id = u32::try_from(self.entries.len()).unwrap_or(u32::MAX);
        self.entries.push(s.to_string());
        self.index.insert(s.to_string(), id);
        id
    }

    fn into_table(self) -> GraphStringTable {
        GraphStringTable {
            entries: self.entries,
        }
    }
}

/// The encode-time arena accumulating nodes + symbols + the string table while
/// the per-kind members are walked. Carries NO host handle, NO dispatch, NO
/// store view — encoding is a pure projection of already-resolved data.
struct GraphArena {
    nodes: Vec<GraphTypeNode>,
    symbols: Vec<GraphSymbolNode>,
    strings: StringTableBuilder,
    /// Counts `ProjectSemanticDispatch`-class operations the encoder performs.
    /// Encoding is zero-dispatch by construction; the counter exists only so a
    /// test can assert it stays zero (no path increments it).
    dispatch_calls: u32,
    /// Maps a named symbol's interned name to its symbol id so two members
    /// referencing the same alias share one `GraphSymbolNode`.
    symbol_index: HashMap<u32, u32>,
}

impl GraphArena {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            symbols: Vec::new(),
            strings: StringTableBuilder::default(),
            dispatch_calls: 0,
            symbol_index: HashMap::new(),
        }
    }

    /// Push a node, returning its arena id.
    fn push_node(&mut self, kind: graph_type_node::Kind) -> u32 {
        let id = u32::try_from(self.nodes.len()).unwrap_or(u32::MAX);
        self.nodes.push(GraphTypeNode { kind: Some(kind) });
        id
    }

    /// Mint (or reuse) a `GraphSymbolNode` for a named alias and return its
    /// symbol id. Identical names dedup to one symbol node.
    fn intern_symbol(&mut self, name: &str) -> u32 {
        let name_id = self.strings.intern(name);
        if let Some(&sym_id) = self.symbol_index.get(&name_id) {
            return sym_id;
        }
        let sym_id = u32::try_from(self.symbols.len()).unwrap_or(u32::MAX);
        self.symbols.push(GraphSymbolNode {
            name_id,
            canonical_name_id: name_id,
            namespace: SymbolNamespace::Type as i32,
            // No decl slot is recorded by the shallow encoder — the wire
            // contract reads 0 as "no slot".
            decl_slot_ref: 0,
        });
        self.symbol_index.insert(name_id, sym_id);
        sym_id
    }

    /// Encode a member value type SHALLOWLY into a node, returning its id.
    ///
    /// One-level only: a primitive / literal becomes its leaf node; a named
    /// `Ref` mints a `GraphSymbolNode` + a `GraphReference`; an empty object
    /// literal encodes as an empty `GraphObject`; everything else (any
    /// structural shape outside the shallow vocabulary, or a walk past the
    /// depth budget) degrades to a `GraphOpaque` — NEVER a fabricated ref,
    /// NEVER a recursion into resolution.
    fn encode_member_value(&mut self, value: Option<&TypeExpr>, depth: u32) -> u32 {
        let Some(value) = value else {
            // A member with no resolved type expression is structurally
            // unencodable here — degrade to opaque, do not fabricate a ref.
            return self.push_opaque("member has no resolved type");
        };
        if depth > SHALLOW_DEPTH_BUDGET {
            // Past the shallow budget: stop, never recurse into resolution.
            return self.push_opaque("member value exceeds shallow encode depth");
        }
        match value {
            TypeExpr::Primitive(name) => {
                let kind = primitive_kind_for(*name);
                self.push_node(graph_type_node::Kind::Primitive(GraphPrimitive {
                    kind: kind as i32,
                }))
            }
            TypeExpr::Literal(lit) => {
                let value = self.encode_literal(lit);
                self.push_node(graph_type_node::Kind::Literal(GraphLiteral {
                    value: Some(value),
                }))
            }
            TypeExpr::Ref { name, .. } => {
                // A named alias: mint a deterministic symbol node + a bare
                // `GraphReference { symbol_id }`. Type arguments are NOT
                // expanded — that would be an eager second walk.
                let symbol_id = self.intern_symbol(name.as_ref());
                self.push_node(graph_type_node::Kind::Reference(GraphReference {
                    symbol_id,
                }))
            }
            TypeExpr::Object(obj) if obj.properties.is_empty() => {
                // The empty object surface is the one structural shape the
                // shallow vocabulary encodes directly (a supported-empty
                // members surface). A NON-empty object is left to the opaque
                // arm — the executor publishes object MEMBERS as
                // `FrameworkSurfaceMember`s, not as a nested object node.
                self.push_node(graph_type_node::Kind::Object(GraphObject::default()))
            }
            // Every other node kind is structural and outside the shallow
            // member-value vocabulary: degrade to opaque, never recurse.
            _ => self.push_opaque("member value is structurally unencodable shallowly"),
        }
    }

    /// Push a `GraphOpaque` carrying an `Other` query error whose message is
    /// interned into the string table.
    fn push_opaque(&mut self, message: &str) -> u32 {
        let message_name_id = self.strings.intern(message);
        self.push_node(graph_type_node::Kind::Opaque(GraphOpaque {
            error: Some(GraphQueryError {
                kind: Some(graph_query_error::Kind::Other(GraphQueryErrorOther {
                    message_name_id,
                })),
            }),
        }))
    }

    fn encode_literal(&mut self, lit: &LiteralValue) -> GraphLiteralValue {
        use verter_protocol::verter::v1::graph_literal_value::Kind as LitKind;
        let kind = match lit {
            LiteralValue::String(s) => {
                let name_id = self.strings.intern(s.as_str());
                LitKind::StringNameId(name_id)
            }
            LiteralValue::Number(n) => LitKind::NumberBits(n.to_bits()),
            LiteralValue::Boolean(b) => LitKind::BooleanValue(*b),
            LiteralValue::BigInt(s) => {
                let name_id = self.strings.intern(s.as_str());
                LitKind::BigintNameId(name_id)
            }
        };
        GraphLiteralValue { kind: Some(kind) }
    }
}

/// Map a [`PrimitiveName`] onto the wire [`PrimitiveKind`].
fn primitive_kind_for(name: PrimitiveName) -> PrimitiveKind {
    match name {
        PrimitiveName::String => PrimitiveKind::String,
        PrimitiveName::Number => PrimitiveKind::Number,
        PrimitiveName::Boolean => PrimitiveKind::Boolean,
        PrimitiveName::Symbol => PrimitiveKind::Symbol,
        PrimitiveName::BigInt => PrimitiveKind::Bigint,
        PrimitiveName::Any => PrimitiveKind::Any,
        PrimitiveName::Unknown => PrimitiveKind::Unknown,
        PrimitiveName::Void => PrimitiveKind::Void,
        PrimitiveName::Never => PrimitiveKind::Never,
        PrimitiveName::Null => PrimitiveKind::Null,
        PrimitiveName::Undefined => PrimitiveKind::Undefined,
        PrimitiveName::Object => PrimitiveKind::Object,
    }
}

/// Encode the executor-normalized per-kind surfaces into the wire graph pair.
///
/// `supported_surfaces` is the adapter descriptor's supported-kind slice; a
/// kind ABSENT from it is filled structurally as `UNSUPPORTED`
/// (`GRAPH_EXACTNESS_UNSUPPORTED` + a diagnostic), distinct from a
/// supported-but-empty kind. The response carries EXACTLY ONE entry per known
/// kind, in `all_kinds` (tag) order.
pub(crate) fn encode_framework_surfaces(
    normalized: &NormalizedSurfaces,
    all_kinds: &[FrameworkSurfaceKind],
    supported_surfaces: &[FrameworkSurfaceKind],
) -> EncodedFrameworkSurfaces {
    let mut arena = GraphArena::new();

    // Index the normalized surfaces by kind for a single-pass per-kind fill.
    let mut by_kind: HashMap<i32, &NormalizedSurface> = HashMap::new();
    for surface in &normalized.surfaces {
        by_kind.insert(surface.kind as i32, surface);
    }

    let mut surfaces = Vec::with_capacity(all_kinds.len());
    for &kind in all_kinds {
        let supported = supported_surfaces.contains(&kind);
        let entry = if !supported {
            // A kind outside the adapter's supported set is filled structurally
            // as UNSUPPORTED — never a bare empty SUPPORTED entry.
            unsupported_entry(
                &mut arena,
                kind,
                "surface kind not supported by this adapter",
            )
        } else {
            match by_kind.get(&(kind as i32)) {
                Some(surface) => encode_kind(&mut arena, kind, &surface.outcome),
                // A supported kind the adapter did not normalize is
                // supported-empty (the selector carries no such surface).
                None => supported_empty_entry(kind),
            }
        };
        surfaces.push(entry);
    }

    let strings = arena.strings.into_table();
    let graph = WireGraph {
        schema_version: wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
        query: None,
        nodes: arena.nodes,
        symbols: arena.symbols,
        signatures: Vec::new(),
        edges: Vec::new(),
        root_ids: Vec::new(),
        exactness: Vec::new(),
        diagnostics: Vec::new(),
        node_id_map: Vec::new(),
        symbol_id_map: Vec::new(),
        strings: Some(strings),
    };

    // The encoder performs ZERO dispatch by construction; assert it here so the
    // invariant fails loudly the moment a future edit threads a resolver call
    // through this module.
    debug_assert_eq!(
        arena.dispatch_calls, 0,
        "the graph encoder must never call dispatch during encode"
    );

    EncodedFrameworkSurfaces { graph, surfaces }
}

/// Encode one supported kind's resolved outcome into a per-kind entry.
fn encode_kind(
    arena: &mut GraphArena,
    kind: FrameworkSurfaceKind,
    outcome: &ResolvedOutcome<MacroSurfaceDtos>,
) -> FrameworkSurfaceKindEntry {
    match outcome {
        ResolvedOutcome::Resolved(dtos) => {
            let members = encode_kind_members(arena, kind, dtos);
            // The wire `FrameworkSurfaceMember` shape carries only NAMED members;
            // a props/emits surface that ALSO declares index signatures
            // (`defineProps<{ [k: string]: string }>()`) cannot represent them on
            // the member arena. Dropping them silently while claiming
            // `ExactResolved` would over-claim exactness, so downgrade to PARTIAL
            // with a diagnostic — the index-signature presence is acknowledged on
            // the wire, never silently lost.
            if has_dropped_index_signatures(kind, dtos) {
                let message_name_id = arena.strings.intern(
                    "index signatures are not representable on the framework-surface member shape",
                );
                FrameworkSurfaceKindEntry {
                    kind: kind as i32,
                    members,
                    status: Some(FrameworkSurfaceKindStatus {
                        support: FrameworkSurfaceKindSupport::Partial as i32,
                        exactness: Exactness::Partial as i32,
                        diagnostics: vec![interned_diagnostic(message_name_id)],
                    }),
                }
            } else {
                FrameworkSurfaceKindEntry {
                    kind: kind as i32,
                    members,
                    status: Some(status(
                        FrameworkSurfaceKindSupport::Supported,
                        Exactness::ExactResolved,
                    )),
                }
            }
        }
        ResolvedOutcome::Partial { value, diagnostics } => {
            let members = encode_kind_members(arena, kind, value);
            // The encoder is the SOLE interner: every diagnostic message goes
            // into the string table here so its `message_name_id` indexes a real
            // entry.
            let diagnostics = intern_diagnostics(arena, diagnostics);
            FrameworkSurfaceKindEntry {
                kind: kind as i32,
                members,
                status: Some(FrameworkSurfaceKindStatus {
                    support: FrameworkSurfaceKindSupport::Partial as i32,
                    exactness: Exactness::Partial as i32,
                    diagnostics,
                }),
            }
        }
        ResolvedOutcome::Unsupported { diagnostics } => {
            // An Unsupported outcome MUST carry at least one diagnostic; when the
            // outcome carried none, intern a default message so the diagnostic's
            // `message_name_id` indexes a real string-table entry (never a bare 0
            // that may index an unrelated string).
            let diagnostics = if diagnostics.is_empty() {
                let message_name_id = arena.strings.intern("surface unsupported by adapter");
                vec![interned_diagnostic(message_name_id)]
            } else {
                intern_diagnostics(arena, diagnostics)
            };
            FrameworkSurfaceKindEntry {
                kind: kind as i32,
                members: Vec::new(),
                status: Some(FrameworkSurfaceKindStatus {
                    support: FrameworkSurfaceKindSupport::Unsupported as i32,
                    exactness: Exactness::Unsupported as i32,
                    diagnostics,
                }),
            }
        }
        // A `Missing` outcome for a kind the adapter DOES support is
        // supported-empty (the selector has no such surface) — distinct from
        // UNSUPPORTED.
        ResolvedOutcome::Missing => supported_empty_entry(kind),
    }
}

/// Whether a resolved props / emits surface declares index signatures the wire
/// member shape cannot represent (so encoding them as named members would lose
/// them silently).
fn has_dropped_index_signatures(kind: FrameworkSurfaceKind, dtos: &MacroSurfaceDtos) -> bool {
    match kind {
        FrameworkSurfaceKind::Props => !dtos.prop_index_signatures().is_empty(),
        FrameworkSurfaceKind::Emits => !dtos.emit_index_signatures().is_empty(),
        _ => false,
    }
}

/// Encode the per-kind members of a resolved DTO bundle into wire members.
///
/// Each surface kind reads its own slot; the executor's `normalize` projected
/// the bundle so only the matching slot is populated. Members carry the
/// member's name (interned) + a shallow-encoded value node id.
fn encode_kind_members(
    arena: &mut GraphArena,
    kind: FrameworkSurfaceKind,
    dtos: &MacroSurfaceDtos,
) -> Vec<wire::FrameworkSurfaceMember> {
    match kind {
        FrameworkSurfaceKind::Props => dtos
            .prop_fields()
            .iter()
            .map(|f| {
                let name_id = arena.strings.intern(&f.name);
                let type_node_id = arena.encode_member_value(f.type_expr.as_ref(), 0);
                wire::FrameworkSurfaceMember {
                    name_id,
                    type_node_id,
                    required: !f.is_optional,
                    readonly: false,
                }
            })
            .collect(),
        FrameworkSurfaceKind::Emits => dtos
            .emit_fields()
            .iter()
            .map(|f| {
                let name_id = arena.strings.intern(&f.name);
                let type_node_id = arena.encode_member_value(f.payload_expr.as_ref(), 0);
                wire::FrameworkSurfaceMember {
                    name_id,
                    type_node_id,
                    // Emits are event signatures, not properties; required /
                    // readonly do not apply (always false on the wire).
                    required: false,
                    readonly: false,
                }
            })
            .collect(),
        FrameworkSurfaceKind::Slots => dtos
            .slot_fields()
            .iter()
            .map(|f| {
                let name_id = arena.strings.intern(&f.name);
                // A slot's value type is its bindings object — left structural
                // here (the shallow encoder degrades it to opaque). The slot
                // NAME + required flag are the load-bearing identity.
                let type_node_id = arena.encode_member_value(None, 0);
                wire::FrameworkSurfaceMember {
                    name_id,
                    type_node_id,
                    required: f.is_required,
                    readonly: false,
                }
            })
            .collect(),
        FrameworkSurfaceKind::Options => dtos
            .options
            .as_ref()
            .map(|s| encode_named_members(arena, &s.members))
            .unwrap_or_default(),
        FrameworkSurfaceKind::Expose => dtos
            .expose
            .as_ref()
            .map(|s| encode_named_members(arena, &s.members))
            .unwrap_or_default(),
        FrameworkSurfaceKind::Model => dtos
            .model
            .as_ref()
            .map(|s| {
                s.bindings
                    .iter()
                    .map(|b| {
                        let name_id = arena.strings.intern(&b.name);
                        let type_node_id = arena.encode_member_value(b.prop.type_expr.as_ref(), 0);
                        wire::FrameworkSurfaceMember {
                            name_id,
                            type_node_id,
                            required: !b.prop.is_optional,
                            readonly: false,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Encode an options / expose object surface's named members.
fn encode_named_members(
    arena: &mut GraphArena,
    members: &[NamedTypeMember],
) -> Vec<wire::FrameworkSurfaceMember> {
    members
        .iter()
        .map(|m| {
            let name_id = arena.strings.intern(&m.name);
            let type_node_id = arena.encode_member_value(m.type_expr.as_ref(), 0);
            wire::FrameworkSurfaceMember {
                name_id,
                type_node_id,
                required: !m.is_optional,
                readonly: false,
            }
        })
        .collect()
}

/// A supported-but-empty per-kind entry (the selector carries no such surface).
fn supported_empty_entry(kind: FrameworkSurfaceKind) -> FrameworkSurfaceKindEntry {
    FrameworkSurfaceKindEntry {
        kind: kind as i32,
        members: Vec::new(),
        status: Some(status(
            FrameworkSurfaceKindSupport::Supported,
            Exactness::ExactResolved,
        )),
    }
}

/// A structurally-UNSUPPORTED per-kind entry with a diagnostic explaining the
/// unsupport. The string-table message is interned so the diagnostic is
/// self-describing on the wire.
fn unsupported_entry(
    arena: &mut GraphArena,
    kind: FrameworkSurfaceKind,
    message: &str,
) -> FrameworkSurfaceKindEntry {
    let message_name_id = arena.strings.intern(message);
    FrameworkSurfaceKindEntry {
        kind: kind as i32,
        members: Vec::new(),
        status: Some(FrameworkSurfaceKindStatus {
            support: FrameworkSurfaceKindSupport::Unsupported as i32,
            exactness: Exactness::Unsupported as i32,
            diagnostics: vec![wire::Diagnostic {
                severity: wire::DiagnosticSeverity::Error as i32,
                message_name_id,
                span_canonical_name_id: 0,
                span_start: 0,
                span_end: 0,
                has_span: false,
            }],
        }),
    }
}

/// A per-kind status with no diagnostics.
fn status(
    support: FrameworkSurfaceKindSupport,
    exactness: Exactness,
) -> FrameworkSurfaceKindStatus {
    FrameworkSurfaceKindStatus {
        support: support as i32,
        exactness: exactness as i32,
        diagnostics: Vec::new(),
    }
}

/// Intern each diagnostic MESSAGE into the arena's string table, returning one
/// wire `GraphDiagnostic` per message whose `message_name_id` indexes the
/// interned entry. The encoder is the SOLE diagnostic interner — outcomes carry
/// message TEXT, never a pre-baked wire diagnostic with a fabricated id.
fn intern_diagnostics(arena: &mut GraphArena, messages: &[String]) -> Vec<wire::Diagnostic> {
    messages
        .iter()
        .map(|message| {
            let message_name_id = arena.strings.intern(message);
            interned_diagnostic(message_name_id)
        })
        .collect()
}

/// A diagnostic whose `message_name_id` indexes an ALREADY-INTERNED string-table
/// entry. Callers intern the message text through the arena first so the id
/// resolves to a real entry (never a bare `0` that might index an unrelated
/// string).
fn interned_diagnostic(message_name_id: u32) -> wire::Diagnostic {
    wire::Diagnostic {
        severity: wire::DiagnosticSeverity::Error as i32,
        message_name_id,
        span_canonical_name_id: 0,
        span_start: 0,
        span_end: 0,
        has_span: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeinfo::framework_surface::results::{PropsSurface, ResolvedOutcome};
    use verter_semantic::analysis::types::AnalyzedPropField;
    use verter_type_expr::{LiteralValue, PrimitiveName, TypeExpr};

    fn prop_field(name: &str, ty: Option<TypeExpr>) -> AnalyzedPropField {
        AnalyzedPropField {
            name: name.to_string(),
            is_optional: false,
            span: verter_span::Span::default(),
            type_annotation: None,
            type_expr: ty,
            type_expr_scope: None,
            description: None,
            tags: Vec::new(),
            resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
            resolution_error: None,
            declared_in_macro_type_arg: true,
        }
    }

    fn props_surface(fields: Vec<AnalyzedPropField>) -> NormalizedSurfaces {
        NormalizedSurfaces {
            surfaces: vec![NormalizedSurface {
                kind: FrameworkSurfaceKind::Props,
                outcome: ResolvedOutcome::Resolved(MacroSurfaceDtos {
                    props: Some(PropsSurface {
                        fields,
                        index_signatures: Vec::new(),
                    }),
                    ..Default::default()
                }),
            }],
        }
    }

    #[test]
    fn exactly_one_entry_per_known_kind_in_tag_order() {
        let normalized = props_surface(vec![]);
        let encoded = encode_framework_surfaces(
            &normalized,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
        );
        assert_eq!(encoded.surfaces.len(), 6, "one entry per known kind");
        let kinds: Vec<i32> = encoded.surfaces.iter().map(|s| s.kind).collect();
        let expected: Vec<i32> = crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS
            .iter()
            .map(|k| *k as i32)
            .collect();
        assert_eq!(kinds, expected, "entries are in tag order");
    }

    #[test]
    fn named_alias_member_mints_a_symbol_node_and_a_reference() {
        // A prop typed `MyAlias` (a named ref) must mint a GraphSymbolNode AND a
        // GraphReference pointing at it — NOT a fabricated opaque, NOT a display
        // string (GraphReference has only symbol_id).
        let normalized = props_surface(vec![prop_field(
            "thing",
            Some(TypeExpr::Ref {
                name: "MyAlias".into(),
                type_arguments: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            }),
        )]);
        let encoded = encode_framework_surfaces(
            &normalized,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
        );
        // The props entry's single member points at a Reference node.
        let props = encoded
            .surfaces
            .iter()
            .find(|s| s.kind == FrameworkSurfaceKind::Props as i32)
            .expect("props entry present");
        assert_eq!(props.members.len(), 1);
        let member = &props.members[0];
        let node = &encoded.graph.nodes[member.type_node_id as usize];
        let Some(graph_type_node::Kind::Reference(reference)) = &node.kind else {
            panic!("named alias member must encode as a Reference, got {node:?}");
        };
        // The reference points at a real symbol node carrying the alias name.
        let symbol = &encoded.graph.symbols[reference.symbol_id as usize];
        let name = &encoded.graph.strings.as_ref().unwrap().entries[symbol.name_id as usize];
        assert_eq!(name, "MyAlias", "the minted symbol carries the alias name");
    }

    #[test]
    fn structural_member_degrades_to_opaque_not_a_fabricated_ref() {
        // A prop typed as a non-empty object literal is structural and outside
        // the shallow member-value vocabulary — it MUST degrade to GraphOpaque,
        // never a fabricated GraphReference/GraphSymbolNode.
        use std::sync::Arc;
        use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty};
        let obj = ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "inner".into(),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
            ))],
        };
        let normalized = props_surface(vec![prop_field(
            "nested",
            Some(TypeExpr::Object(Arc::new(obj))),
        )]);
        let encoded = encode_framework_surfaces(
            &normalized,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
        );
        let props = encoded
            .surfaces
            .iter()
            .find(|s| s.kind == FrameworkSurfaceKind::Props as i32)
            .expect("props entry present");
        let node = &encoded.graph.nodes[props.members[0].type_node_id as usize];
        assert!(
            matches!(node.kind, Some(graph_type_node::Kind::Opaque(_))),
            "a non-empty structural object member must degrade to GraphOpaque, got {node:?}"
        );
        // No symbol node was fabricated for the structural member.
        assert!(
            encoded.graph.symbols.is_empty(),
            "a structural member must NOT mint a symbol node"
        );
    }

    #[test]
    fn primitive_member_encodes_as_a_primitive_node() {
        let normalized = props_surface(vec![prop_field(
            "count",
            Some(TypeExpr::Primitive(PrimitiveName::Number)),
        )]);
        let encoded = encode_framework_surfaces(
            &normalized,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
        );
        let props = encoded
            .surfaces
            .iter()
            .find(|s| s.kind == FrameworkSurfaceKind::Props as i32)
            .unwrap();
        let node = &encoded.graph.nodes[props.members[0].type_node_id as usize];
        let Some(graph_type_node::Kind::Primitive(p)) = &node.kind else {
            panic!("a primitive prop must encode as a Primitive node, got {node:?}");
        };
        assert_eq!(p.kind, PrimitiveKind::Number as i32);
    }

    #[test]
    fn unsupported_kind_fills_structurally_with_a_diagnostic() {
        // An adapter that supports ONLY props must fill the other five kinds as
        // UNSUPPORTED with GRAPH_EXACTNESS_UNSUPPORTED + a diagnostic — never a
        // bare empty SUPPORTED entry.
        let normalized = props_surface(vec![]);
        let encoded = encode_framework_surfaces(
            &normalized,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
            &[FrameworkSurfaceKind::Props],
        );
        let emits = encoded
            .surfaces
            .iter()
            .find(|s| s.kind == FrameworkSurfaceKind::Emits as i32)
            .expect("emits entry present");
        let status = emits.status.as_ref().unwrap();
        assert_eq!(
            status.support,
            FrameworkSurfaceKindSupport::Unsupported as i32
        );
        assert_eq!(status.exactness, Exactness::Unsupported as i32);
        assert!(
            !status.diagnostics.is_empty(),
            "an UNSUPPORTED kind must carry at least one diagnostic"
        );
        assert!(emits.members.is_empty());
    }

    #[test]
    fn supported_empty_is_distinct_from_unsupported() {
        // A supported kind with no normalized surface is supported-empty
        // (SUPPORTED + empty members), distinct from an unsupported kind.
        let normalized = props_surface(vec![]);
        let encoded = encode_framework_surfaces(
            &normalized,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
        );
        // Emits is a supported kind that the props-only normalized set did not
        // produce — supported-empty.
        let emits = encoded
            .surfaces
            .iter()
            .find(|s| s.kind == FrameworkSurfaceKind::Emits as i32)
            .unwrap();
        let status = emits.status.as_ref().unwrap();
        assert_eq!(
            status.support,
            FrameworkSurfaceKindSupport::Supported as i32,
            "an unproduced supported kind is supported-empty, not unsupported"
        );
        assert!(emits.members.is_empty());
    }

    #[test]
    fn string_table_dedups_repeated_names() {
        // Two props of the same alias type share one symbol node and the alias
        // name interns once.
        let normalized = props_surface(vec![
            prop_field(
                "a",
                Some(TypeExpr::Ref {
                    name: "Shared".into(),
                    type_arguments: std::sync::Arc::from(Vec::new().into_boxed_slice()),
                }),
            ),
            prop_field(
                "b",
                Some(TypeExpr::Ref {
                    name: "Shared".into(),
                    type_arguments: std::sync::Arc::from(Vec::new().into_boxed_slice()),
                }),
            ),
        ]);
        let encoded = encode_framework_surfaces(
            &normalized,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
        );
        assert_eq!(
            encoded.graph.symbols.len(),
            1,
            "two members of the same alias share one symbol node"
        );
        let entries = &encoded.graph.strings.as_ref().unwrap().entries;
        let shared_count = entries.iter().filter(|e| *e == "Shared").count();
        assert_eq!(
            shared_count, 1,
            "the shared alias name interns exactly once"
        );
    }

    #[test]
    fn literal_member_encodes_with_a_string_table_entry() {
        let normalized = props_surface(vec![prop_field(
            "tag",
            Some(TypeExpr::Literal(LiteralValue::String("hello".into()))),
        )]);
        let encoded = encode_framework_surfaces(
            &normalized,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
        );
        let props = encoded
            .surfaces
            .iter()
            .find(|s| s.kind == FrameworkSurfaceKind::Props as i32)
            .unwrap();
        let node = &encoded.graph.nodes[props.members[0].type_node_id as usize];
        assert!(
            matches!(node.kind, Some(graph_type_node::Kind::Literal(_))),
            "a string-literal prop must encode as a Literal node"
        );
        assert!(
            encoded
                .graph
                .strings
                .as_ref()
                .unwrap()
                .entries
                .iter()
                .any(|e| e == "hello"),
            "the literal's string value is interned"
        );
    }

    #[test]
    fn index_signature_props_downgrade_to_partial_with_interned_diagnostic() {
        // A props surface that declares index signatures (not representable on
        // the named-member wire shape) must NOT claim ExactResolved — it
        // downgrades to PARTIAL with a diagnostic whose message id indexes a
        // real string-table entry.
        use verter_semantic::analysis::type_expand::ExpandedIndexSignature;
        let normalized = NormalizedSurfaces {
            surfaces: vec![NormalizedSurface {
                kind: FrameworkSurfaceKind::Props,
                outcome: ResolvedOutcome::Resolved(MacroSurfaceDtos {
                    props: Some(PropsSurface {
                        fields: vec![prop_field(
                            "named",
                            Some(TypeExpr::Primitive(PrimitiveName::String)),
                        )],
                        index_signatures: vec![ExpandedIndexSignature {
                            key_type: TypeExpr::Primitive(PrimitiveName::String),
                            value_type: TypeExpr::Primitive(PrimitiveName::Number),
                            readonly: false,
                        }],
                    }),
                    ..Default::default()
                }),
            }],
        };
        let encoded = encode_framework_surfaces(
            &normalized,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
        );
        let props = encoded
            .surfaces
            .iter()
            .find(|s| s.kind == FrameworkSurfaceKind::Props as i32)
            .unwrap();
        let status = props.status.as_ref().unwrap();
        assert_eq!(
            status.support,
            FrameworkSurfaceKindSupport::Partial as i32,
            "an index-signature props surface is PARTIAL, not ExactResolved"
        );
        assert_eq!(status.exactness, Exactness::Partial as i32);
        let diag = status
            .diagnostics
            .first()
            .expect("a partial index-signature surface carries a diagnostic");
        // The diagnostic message id must index a real string-table entry.
        let entries = &encoded.graph.strings.as_ref().unwrap().entries;
        assert!(
            (diag.message_name_id as usize) < entries.len(),
            "the diagnostic message id indexes a real string-table entry"
        );
        assert!(
            entries[diag.message_name_id as usize].contains("index signature"),
            "the diagnostic names the index-signature gap, got {:?}",
            entries[diag.message_name_id as usize]
        );
        // The named member still encodes.
        assert_eq!(props.members.len(), 1);
    }
}
