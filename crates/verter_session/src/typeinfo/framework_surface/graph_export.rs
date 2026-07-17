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
    self as wire, Exactness, FrameworkSurfaceDeclarationKind, FrameworkSurfaceKind,
    FrameworkSurfaceKindEntry, FrameworkSurfaceKindStatus, FrameworkSurfaceKindSupport,
    FrameworkSurfaceOriginHopKind, PrimitiveKind, SemanticTypeGraph, SymbolNamespace,
};
use verter_protocol::verter::v1::{
    graph_query_error, graph_type_node, FrameworkSurfaceMemberDeclaration,
    FrameworkSurfaceMemberOrigin, FrameworkSurfaceOriginHop, GraphLiteral, GraphLiteralValue,
    GraphObject, GraphOpaque, GraphPrimitive, GraphQueryError, GraphQueryErrorOther,
    GraphReference, GraphStringTable, GraphSymbolNode, GraphTypeNode,
    SemanticTypeGraph as WireGraph,
};
use verter_type_expr::{LiteralValue, PrimitiveName, TypeExpr};

use crate::resolver_core::{ResolvedDeclarationKind, ResolvedTypeDeclaration};
use crate::typeinfo::framework_surface::results;
use crate::typeinfo::framework_surface::results::{
    MacroSurfaceDtos, NamedTypeMember, NamedTypeMemberOutput, NormalizedSurface,
    NormalizedSurfaces, OriginHop, ResolvedOutcome,
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

    /// Encode a SEALED named-member output value into a node, returning its id.
    ///
    /// The classification already happened at the producer boundary
    /// ([`NamedTypeMemberOutput`] is the closed shallow vocabulary), so this is
    /// a pure arm-to-wire-node map — byte-identical to the legacy shallow
    /// `TypeExpr` encoding for every arm: primitive / literal leaves, a named
    /// ref minting a deterministic `GraphSymbolNode` + `GraphReference`, the
    /// empty `GraphObject`, and the two DISTINCT opaque diagnostics (`None` =
    /// no resolved type at all; `Opaque` = resolved but structurally
    /// unencodable shallowly).
    fn encode_named_member_output(&mut self, value: Option<&NamedTypeMemberOutput>) -> u32 {
        let Some(value) = value else {
            // A member with no resolved type expression is structurally
            // unencodable here — degrade to opaque, do not fabricate a ref.
            return self.push_opaque("member has no resolved type");
        };
        match value {
            NamedTypeMemberOutput::Primitive(name) => {
                let kind = primitive_kind_for(*name);
                self.push_node(graph_type_node::Kind::Primitive(GraphPrimitive {
                    kind: kind as i32,
                }))
            }
            NamedTypeMemberOutput::Literal(lit) => {
                let value = self.encode_literal(lit);
                self.push_node(graph_type_node::Kind::Literal(GraphLiteral {
                    value: Some(value),
                }))
            }
            NamedTypeMemberOutput::Ref { name } => {
                // A named alias: mint a deterministic symbol node + a bare
                // `GraphReference { symbol_id }`.
                let symbol_id = self.intern_symbol(name.as_ref());
                self.push_node(graph_type_node::Kind::Reference(GraphReference {
                    symbol_id,
                }))
            }
            NamedTypeMemberOutput::EmptyObject => {
                self.push_node(graph_type_node::Kind::Object(GraphObject::default()))
            }
            NamedTypeMemberOutput::Opaque => {
                self.push_opaque("member value is structurally unencodable shallowly")
            }
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

    /// Encode a per-member declaration [`PropOrigin`](results::PropOrigin) into
    /// the wire [`FrameworkSurfaceMemberOrigin`] (schema 4). All string fields
    /// intern into the graph string table; the hop chain maps each
    /// framework-neutral [`OriginHop`] onto its wire form. This is a pure
    /// projection of resolved data — no dispatch, no re-resolution.
    fn encode_member_origin(
        &mut self,
        origin: &results::PropOrigin,
    ) -> FrameworkSurfaceMemberOrigin {
        let declaration = Some(self.encode_member_declaration(&origin.declaration));
        let chain = origin
            .chain
            .iter()
            .map(|hop| self.encode_origin_hop(hop))
            .collect();
        FrameworkSurfaceMemberOrigin { declaration, chain }
    }

    /// Encode a [`ResolvedTypeDeclaration`] into the wire
    /// [`FrameworkSurfaceMemberDeclaration`] (string ids interned).
    fn encode_member_declaration(
        &mut self,
        declaration: &ResolvedTypeDeclaration,
    ) -> FrameworkSurfaceMemberDeclaration {
        let requested_name_id = self.strings.intern(&declaration.requested_name);
        let resolved_name_id = self.strings.intern(&declaration.resolved_name);
        let canonical_source_id = self.strings.intern(&declaration.canonical_source);
        FrameworkSurfaceMemberDeclaration {
            requested_name_id,
            resolved_name_id,
            canonical_source_id,
            span_start: declaration.span.start,
            span_end: declaration.span.end,
            kind: declaration_kind_for(declaration.kind) as i32,
        }
    }

    /// Encode one framework-neutral [`OriginHop`] into the wire
    /// [`FrameworkSurfaceOriginHop`]. Each hop's string-id fields are
    /// PRESENCE-AWARE (`optional` on the wire): a field is `Some(interned id)`
    /// ONLY when the hop kind genuinely carries it, `None` otherwise. The
    /// encoder NEVER emits id 0 for an absent field — the graph string table is
    /// zero-based (entry 0 is a real interned string), so an id-0 absent
    /// sentinel would alias a real table entry and fabricate data on decode.
    /// A hop string a present hop genuinely carries is always interned, even
    /// if it is an empty string (a present-but-empty value stays present).
    fn encode_origin_hop(&mut self, hop: &OriginHop) -> FrameworkSurfaceOriginHop {
        // A LOCAL hop carries no string fields — every id is absent.
        let mut wire = FrameworkSurfaceOriginHop {
            kind: FrameworkSurfaceOriginHopKind::Local as i32,
            from_id: None,
            specifier_id: None,
            imported_name_id: None,
            to_id: None,
            exported_name_id: None,
            original_name_id: None,
            alias_name_id: None,
        };
        match hop {
            OriginHop::Local => {}
            OriginHop::Import {
                from,
                specifier,
                imported_name,
            } => {
                wire.kind = FrameworkSurfaceOriginHopKind::Import as i32;
                wire.from_id = Some(self.strings.intern(from));
                // The specifier is itself optional in the source model — set
                // the field only when it was recorded.
                wire.specifier_id = specifier.as_deref().map(|s| self.strings.intern(s));
                wire.imported_name_id = Some(self.strings.intern(imported_name));
            }
            OriginHop::Reexport {
                from,
                to,
                exported_name,
                original_name,
            } => {
                wire.kind = FrameworkSurfaceOriginHopKind::Reexport as i32;
                wire.from_id = Some(self.strings.intern(from));
                wire.to_id = Some(self.strings.intern(to));
                wire.exported_name_id = Some(self.strings.intern(exported_name));
                wire.original_name_id = Some(self.strings.intern(original_name));
            }
            OriginHop::Alias { name } => {
                wire.kind = FrameworkSurfaceOriginHopKind::Alias as i32;
                wire.alias_name_id = Some(self.strings.intern(name));
            }
        }
        wire
    }
}

/// Map a [`ResolvedDeclarationKind`] onto the wire
/// [`FrameworkSurfaceDeclarationKind`].
fn declaration_kind_for(kind: ResolvedDeclarationKind) -> FrameworkSurfaceDeclarationKind {
    match kind {
        ResolvedDeclarationKind::Interface => FrameworkSurfaceDeclarationKind::Interface,
        ResolvedDeclarationKind::TypeAlias => FrameworkSurfaceDeclarationKind::TypeAlias,
        ResolvedDeclarationKind::Class => FrameworkSurfaceDeclarationKind::Class,
        ResolvedDeclarationKind::Unknown => FrameworkSurfaceDeclarationKind::Unknown,
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
    encode_framework_surfaces_with_unsupported_message(
        normalized,
        all_kinds,
        supported_surfaces,
        "surface kind not supported by this adapter",
    )
}

/// As [`encode_framework_surfaces`], but with a caller-supplied UNSUPPORTED
/// diagnostic message — a `Deferred` registration passes the
/// surfaces-not-yet-registered message, distinct from a supported
/// adapter's per-kind unsupport.
pub(crate) fn encode_framework_surfaces_with_unsupported_message(
    normalized: &NormalizedSurfaces,
    all_kinds: &[FrameworkSurfaceKind],
    supported_surfaces: &[FrameworkSurfaceKind],
    unsupported_message: &str,
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
            unsupported_entry(&mut arena, kind, unsupported_message)
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
        FrameworkSurfaceKind::Props => {
            // Index the DEFAULT-value + ORIGIN sidecars by prop name so each
            // member emits its own runtime default source text + resolver-known
            // declaration provenance on the wire (schema 4). Both sidecars are
            // framework-neutral; a member with no default / no resolved origin
            // emits the absent (`None`) wire form.
            let defaults: HashMap<&str, &str> = dtos
                .prop_defaults()
                .iter()
                .map(|d| (d.key.as_str(), d.value.as_str()))
                .collect();
            let origins: HashMap<&str, &results::PropOrigin> = dtos
                .prop_origins()
                .iter()
                .map(|o| (o.prop_name.as_str(), &o.origin))
                .collect();
            dtos.prop_fields()
                .iter()
                .map(|row| {
                    let f = &row.analysis;
                    let name_id = arena.strings.intern(&f.name);
                    // A prop's typed body is an on-demand payload LOCATOR
                    // (`AnalyzedPropField.payload`), resolved only through the
                    // shared dispatch — this encoder is ZERO-DISPATCH, so the
                    // member value is structurally unencodable here and takes
                    // the same absent arm a slot value does (opaque, never a
                    // fabricated ref, never a resolve).
                    let type_node_id = arena.encode_member_value(None, 0);
                    let default_value_id = defaults
                        .get(f.name.as_str())
                        .map(|value| arena.strings.intern(value));
                    let origin = origins
                        .get(f.name.as_str())
                        .map(|origin| arena.encode_member_origin(origin));
                    wire::FrameworkSurfaceMember {
                        name_id,
                        type_node_id,
                        required: !f.is_optional,
                        readonly: false,
                        default_value_id,
                        origin,
                    }
                })
                .collect()
        }
        FrameworkSurfaceKind::Emits => dtos
            .emit_fields()
            .iter()
            .map(|f| {
                let name_id = arena.strings.intern(&f.analysis.name);
                // An emit's typed payload is an on-demand LOCATOR
                // (`AnalyzedEmitField.payload`) — zero-dispatch encoder, so
                // the value takes the absent/opaque arm (see props above).
                let type_node_id = arena.encode_member_value(None, 0);
                wire::FrameworkSurfaceMember {
                    name_id,
                    type_node_id,
                    // Emits are event signatures, not properties; required /
                    // readonly do not apply (always false on the wire).
                    required: false,
                    readonly: false,
                    default_value_id: None,
                    origin: None,
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
                    default_value_id: None,
                    origin: None,
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
                        // A model binding's prop type is an on-demand payload
                        // LOCATOR (`AnalyzedPropField.payload`) — absent arm,
                        // same as props/emits above.
                        let type_node_id = arena.encode_member_value(None, 0);
                        wire::FrameworkSurfaceMember {
                            name_id,
                            type_node_id,
                            required: !b.prop.is_optional,
                            readonly: false,
                            default_value_id: None,
                            origin: None,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Encode an options / expose object surface's named members from their
/// SEALED shallow output values (no `TypeExpr` reaches this encoder).
fn encode_named_members(
    arena: &mut GraphArena,
    members: &[NamedTypeMember],
) -> Vec<wire::FrameworkSurfaceMember> {
    members
        .iter()
        .map(|m| {
            let name_id = arena.strings.intern(&m.name);
            let type_node_id = arena.encode_named_member_output(m.value.as_ref());
            wire::FrameworkSurfaceMember {
                name_id,
                type_node_id,
                required: !m.is_optional,
                readonly: false,
                default_value_id: None,
                origin: None,
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

    /// A named prop field. The prop's typed body is now an on-demand payload
    /// LOCATOR (`AnalyzedPropField.payload`), never an inline `TypeExpr`; the
    /// zero-dispatch encoder emits every prop member value as opaque. Tests that
    /// pin the member-value ENCODING VOCABULARY drive `encode_member_value`
    /// directly (below); this helper supplies named fields for surface-level
    /// tests.
    fn prop_field(name: &str) -> AnalyzedPropField {
        AnalyzedPropField {
            name: name.to_string(),
            is_optional: false,
            span: verter_span::Span::default(),
            type_annotation: None,
            payload: None,
            type_expr_scope: None,
            description: None,
            tags: Vec::new(),
            resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
            resolution_error: None,
            declared_in_macro_type_arg: true,
        }
    }

    /// Encode a single member VALUE `TypeExpr` through the surviving
    /// `GraphArena::encode_member_value` vocabulary and return the arena plus the
    /// pushed node id — the direct driver for the per-arm encoding-vocabulary
    /// tests (the prop-surface path no longer feeds a `TypeExpr` to the encoder;
    /// prop members take the opaque locator arm).
    fn encode_value(ty: &TypeExpr) -> (GraphArena, u32) {
        let mut arena = GraphArena::new();
        let id = arena.encode_member_value(Some(ty), 0);
        (arena, id)
    }

    fn props_surface(fields: Vec<AnalyzedPropField>) -> NormalizedSurfaces {
        let fields = fields
            .into_iter()
            .map(|analysis| results::ResolvedPropField {
                analysis,
                type_source: verter_type_expr::facts::SourcePosition::unannotated(),
            })
            .collect();
        NormalizedSurfaces {
            surfaces: vec![NormalizedSurface {
                kind: FrameworkSurfaceKind::Props,
                outcome: ResolvedOutcome::Resolved(MacroSurfaceDtos {
                    props: Some(PropsSurface {
                        fields,
                        index_signatures: Vec::new(),
                        ..Default::default()
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
        // A member value typed `MyAlias` (a named ref) must mint a
        // GraphSymbolNode AND a GraphReference pointing at it — NOT a fabricated
        // opaque, NOT a display string (GraphReference has only symbol_id).
        let (arena, id) = encode_value(&TypeExpr::Ref {
            name: "MyAlias".into(),
            type_arguments: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        });
        let node = &arena.nodes[id as usize];
        let Some(graph_type_node::Kind::Reference(reference)) = &node.kind else {
            panic!("named alias member must encode as a Reference, got {node:?}");
        };
        // The reference points at a real symbol node carrying the alias name.
        let symbol = &arena.symbols[reference.symbol_id as usize];
        let name = &arena.strings.entries[symbol.name_id as usize];
        assert_eq!(name, "MyAlias", "the minted symbol carries the alias name");
    }

    #[test]
    fn structural_member_degrades_to_opaque_not_a_fabricated_ref() {
        // A member value typed as a non-empty object literal is structural and
        // outside the shallow member-value vocabulary — it MUST degrade to
        // GraphOpaque, never a fabricated GraphReference/GraphSymbolNode.
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
        let (arena, id) = encode_value(&TypeExpr::Object(Arc::new(obj)));
        let node = &arena.nodes[id as usize];
        assert!(
            matches!(node.kind, Some(graph_type_node::Kind::Opaque(_))),
            "a non-empty structural object member must degrade to GraphOpaque, got {node:?}"
        );
        // No symbol node was fabricated for the structural member.
        assert!(
            arena.symbols.is_empty(),
            "a structural member must NOT mint a symbol node"
        );
    }

    #[test]
    fn primitive_member_encodes_as_a_primitive_node() {
        let (arena, id) = encode_value(&TypeExpr::Primitive(PrimitiveName::Number));
        let node = &arena.nodes[id as usize];
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
        // Two member values of the same alias type share one symbol node and the
        // alias name interns once.
        let mut arena = GraphArena::new();
        let make_ref = || TypeExpr::Ref {
            name: "Shared".into(),
            type_arguments: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        };
        let _ = arena.encode_member_value(Some(&make_ref()), 0);
        let _ = arena.encode_member_value(Some(&make_ref()), 0);
        assert_eq!(
            arena.symbols.len(),
            1,
            "two members of the same alias share one symbol node"
        );
        let shared_count = arena
            .strings
            .entries
            .iter()
            .filter(|e| *e == "Shared")
            .count();
        assert_eq!(
            shared_count, 1,
            "the shared alias name interns exactly once"
        );
    }

    #[test]
    fn literal_member_encodes_with_a_string_table_entry() {
        let (arena, id) = encode_value(&TypeExpr::Literal(LiteralValue::String("hello".into())));
        let node = &arena.nodes[id as usize];
        assert!(
            matches!(node.kind, Some(graph_type_node::Kind::Literal(_))),
            "a string-literal prop must encode as a Literal node"
        );
        assert!(
            arena.strings.entries.iter().any(|e| e == "hello"),
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
                        fields: vec![results::ResolvedPropField {
                            analysis: prop_field("named"),
                            type_source: verter_type_expr::facts::SourcePosition::unannotated(),
                        }],
                        index_signatures: vec![ExpandedIndexSignature {
                            key_type: verter_type_expr::facts::SourcePosition::Present(
                                verter_type_expr::facts::SemanticTypeSource::Closed(
                                    verter_type_expr::facts::ClosedTypeFact::Leaf(
                                        verter_type_expr::facts::LeafTypeFact::Primitive(
                                            PrimitiveName::String,
                                        ),
                                    ),
                                ),
                            ),
                            value_type: verter_type_expr::facts::SourcePosition::Present(
                                verter_type_expr::facts::SemanticTypeSource::Closed(
                                    verter_type_expr::facts::ClosedTypeFact::Leaf(
                                        verter_type_expr::facts::LeafTypeFact::Primitive(
                                            PrimitiveName::Number,
                                        ),
                                    ),
                                ),
                            ),
                            readonly: false,
                        }],
                        ..Default::default()
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

    #[test]
    fn absent_origin_hop_fields_stay_absent_across_a_nonempty_table_zero() {
        // DISCRIMINATING (the P0): the encoder is PRESENCE-AWARE — an absent
        // hop string field is `None`, never id 0. The string table is
        // zero-based (entry 0 is a real interned string), so an id-0 absent
        // sentinel would alias entry 0 and fabricate data on decode. This test
        // forces a NON-EMPTY table[0] (the real encoder never seeds `""` at 0),
        // encodes a LOCAL hop (no string fields) and an IMPORT hop with NO
        // specifier, and asserts every absent field is `None` while present
        // fields resolve to their interned strings — RED before presence-aware
        // encoding (the old encoder wrote literal 0 for absent fields).
        use crate::typeinfo::framework_surface::results::{OriginHop, PropOrigin};

        let mut arena = GraphArena::new();
        // Intern a distinctive string FIRST so table[0] is a real, non-empty
        // entry — exactly the layout the real encoder produces.
        let sentinel_zero = arena.strings.intern("__SENTINEL_ZERO__");
        assert_eq!(sentinel_zero, 0, "the sentinel must occupy table index 0");

        let declaration = ResolvedTypeDeclaration {
            requested_name: "size".to_string(),
            declaration_id: None,
            resolved_name: "Size".to_string(),
            canonical_source: "/lib/props.ts".to_string(),
            span: verter_span::Span::default(),
            kind: ResolvedDeclarationKind::TypeAlias,
            text: None,
        };
        let origin = PropOrigin {
            declaration,
            chain: vec![
                OriginHop::Local,
                OriginHop::Import {
                    from: "/lib/props.ts".to_string(),
                    specifier: None,
                    imported_name: "Size".to_string(),
                },
            ],
        };

        let encoded = arena.encode_member_origin(&origin);
        let entries = &arena.strings.entries;

        // The declaration's always-present string ids resolve to real entries
        // (never the absent sentinel by accident).
        let decl = encoded.declaration.as_ref().expect("declaration present");
        assert_eq!(entries[decl.resolved_name_id as usize], "Size");
        assert_eq!(entries[decl.canonical_source_id as usize], "/lib/props.ts");

        // LOCAL hop: EVERY string field is `None` — never `Some(0)`.
        let local = &encoded.chain[0];
        assert_eq!(local.kind, FrameworkSurfaceOriginHopKind::Local as i32);
        assert!(local.from_id.is_none(), "LOCAL from_id must be absent");
        assert!(local.specifier_id.is_none());
        assert!(local.imported_name_id.is_none());
        assert!(local.to_id.is_none());
        assert!(local.exported_name_id.is_none());
        assert!(local.original_name_id.is_none());
        assert!(local.alias_name_id.is_none());

        // IMPORT hop: from + importedName present, specifier ABSENT.
        let import = &encoded.chain[1];
        assert_eq!(import.kind, FrameworkSurfaceOriginHopKind::Import as i32);
        let from_id = import.from_id.expect("IMPORT from_id present");
        let imported_id = import
            .imported_name_id
            .expect("IMPORT importedName present");
        assert_eq!(entries[from_id as usize], "/lib/props.ts");
        assert_eq!(entries[imported_id as usize], "Size");
        assert!(
            import.specifier_id.is_none(),
            "an IMPORT hop with no recorded specifier must leave specifier_id absent, \
             never id 0 (which would alias the non-empty table[0])"
        );
        // The other (REEXPORT/ALIAS) fields are absent for an IMPORT hop.
        assert!(import.to_id.is_none());
        assert!(import.exported_name_id.is_none());
        assert!(import.original_name_id.is_none());
        assert!(import.alias_name_id.is_none());
    }

    /// GOLDEN wire parity for the SEALED named-member output encoding: every
    /// [`NamedTypeMemberOutput`] arm (plus the `None` no-resolved-type state)
    /// produces EXACTLY the wire node the legacy shallow `TypeExpr` encoding
    /// produced — proved by a DIRECT A/B: each arm runs BOTH encoders (the
    /// legacy `encode_member_value(&TypeExpr, 0)` and the sealed
    /// `encode_named_member_output`) on fresh arenas and byte-compares the
    /// produced node / symbol / string tables, so the
    /// "wire_identical_to_legacy" claim is self-proving rather than
    /// node-spot-checked. The end-to-end encode below then pins the same arms
    /// through `encode_framework_surfaces`. Discriminating: pairing the two
    /// DISTINCT opaque diagnostics (`None` vs `Opaque`) fails the byte
    /// compare, and collapsing either into the other flips an asserted
    /// message.
    #[test]
    fn sealed_named_member_output_encodes_wire_identical_to_legacy_shallow() {
        use crate::typeinfo::framework_surface::results::OptionsSurface;
        use verter_type_expr::ObjectExpr;

        // --- DIRECT legacy A/B, arm by arm ------------------------------
        // `encode_member_value` IS the legacy shallow `TypeExpr` encoder the
        // sealed vocabulary replaced on the named-member route; running both
        // on fresh arenas and comparing the WHOLE arena state (nodes +
        // symbols + strings + the returned id) proves byte-identity per arm.
        let legacy_vs_sealed =
            |legacy_value: Option<&TypeExpr>, sealed_value: Option<&NamedTypeMemberOutput>| {
                let mut legacy = GraphArena::new();
                let legacy_id = legacy.encode_member_value(legacy_value, 0);
                let mut sealed = GraphArena::new();
                let sealed_id = sealed.encode_named_member_output(sealed_value);
                assert_eq!(
                    legacy_id, sealed_id,
                    "the returned node id must match the legacy encoder: {legacy_value:?}"
                );
                assert_eq!(
                    legacy.nodes, sealed.nodes,
                    "the node tables must be byte-identical to the legacy encoder: {legacy_value:?}"
                );
                assert_eq!(
                    legacy.symbols, sealed.symbols,
                    "the symbol tables must be byte-identical to the legacy encoder: {legacy_value:?}"
                );
                assert_eq!(
                    legacy.strings.entries, sealed.strings.entries,
                    "the string tables must be byte-identical to the legacy encoder: {legacy_value:?}"
                );
            };
        // (0) the `None` no-resolved-type state.
        legacy_vs_sealed(None, None);
        // (1) primitive leaf.
        legacy_vs_sealed(
            Some(&TypeExpr::Primitive(PrimitiveName::Number)),
            Some(&NamedTypeMemberOutput::Primitive(PrimitiveName::Number)),
        );
        // (2) literal leaf (interned string value).
        legacy_vs_sealed(
            Some(&TypeExpr::Literal(LiteralValue::String("hello".into()))),
            Some(&NamedTypeMemberOutput::Literal(LiteralValue::String(
                "hello".into(),
            ))),
        );
        // (3) named ref (minted symbol node + GraphReference).
        legacy_vs_sealed(
            Some(&TypeExpr::named("MyAlias")),
            Some(&NamedTypeMemberOutput::Ref {
                name: "MyAlias".into(),
            }),
        );
        // (4) the empty object surface.
        legacy_vs_sealed(
            Some(&TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                properties: vec![],
            }))),
            Some(&NamedTypeMemberOutput::EmptyObject),
        );
        // (5) the structurally-unencodable arm (a union is outside the
        // shallow member-value vocabulary on both encoders).
        legacy_vs_sealed(
            Some(&TypeExpr::Union(std::sync::Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]))),
            Some(&NamedTypeMemberOutput::Opaque),
        );
        // The A/B genuinely discriminates: pairing the `None` state with the
        // `Opaque` arm (two DIFFERENT diagnostics) fails the byte compare.
        let mut none_arena = GraphArena::new();
        let _ = none_arena.encode_member_value(None, 0);
        let mut opaque_arena = GraphArena::new();
        let _ = opaque_arena.encode_named_member_output(Some(&NamedTypeMemberOutput::Opaque));
        assert_ne!(
            none_arena.strings.entries, opaque_arena.strings.entries,
            "the None and Opaque diagnostics stay distinct (the A/B discriminates)"
        );

        // --- End-to-end pin through `encode_framework_surfaces` ---------

        let member = |name: &str, value: Option<NamedTypeMemberOutput>| NamedTypeMember {
            name: name.to_string(),
            is_optional: false,
            value,
            type_annotation: None,
            type_references: Vec::new(),
            source_span: None,
        };
        let normalized = NormalizedSurfaces {
            surfaces: vec![NormalizedSurface {
                kind: FrameworkSurfaceKind::Options,
                outcome: ResolvedOutcome::Resolved(MacroSurfaceDtos {
                    options: Some(OptionsSurface {
                        members: vec![
                            member("unresolved", None),
                            member(
                                "count",
                                Some(NamedTypeMemberOutput::Primitive(PrimitiveName::Number)),
                            ),
                            member(
                                "tag",
                                Some(NamedTypeMemberOutput::Literal(LiteralValue::String(
                                    "hello".into(),
                                ))),
                            ),
                            member(
                                "aliased",
                                Some(NamedTypeMemberOutput::Ref {
                                    name: "MyAlias".into(),
                                }),
                            ),
                            member("empty", Some(NamedTypeMemberOutput::EmptyObject)),
                            member("structural", Some(NamedTypeMemberOutput::Opaque)),
                        ],
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
        let options = encoded
            .surfaces
            .iter()
            .find(|s| s.kind == FrameworkSurfaceKind::Options as i32)
            .expect("options entry present");
        assert_eq!(options.members.len(), 6);
        let entries = &encoded.graph.strings.as_ref().unwrap().entries;
        let node_of = |i: usize| &encoded.graph.nodes[options.members[i].type_node_id as usize];
        let opaque_message = |node: &GraphTypeNode| -> String {
            let Some(graph_type_node::Kind::Opaque(op)) = &node.kind else {
                panic!("expected an Opaque node, got {node:?}");
            };
            let Some(graph_query_error::Kind::Other(other)) = &op.error.as_ref().unwrap().kind
            else {
                panic!("expected an Other query error");
            };
            entries[other.message_name_id as usize].clone()
        };

        // (0) `None` — the legacy no-resolved-type opaque, VERBATIM message.
        assert_eq!(opaque_message(node_of(0)), "member has no resolved type");

        // (1) primitive leaf.
        let Some(graph_type_node::Kind::Primitive(p)) = &node_of(1).kind else {
            panic!("primitive arm must encode as a Primitive node");
        };
        assert_eq!(p.kind, PrimitiveKind::Number as i32);

        // (2) literal leaf with its interned string value.
        assert!(matches!(
            node_of(2).kind,
            Some(graph_type_node::Kind::Literal(_))
        ));
        assert!(entries.iter().any(|e| e == "hello"));

        // (3) named ref: a REAL minted symbol node carrying the alias name.
        let Some(graph_type_node::Kind::Reference(reference)) = &node_of(3).kind else {
            panic!("ref arm must encode as a Reference node");
        };
        let symbol = &encoded.graph.symbols[reference.symbol_id as usize];
        assert_eq!(entries[symbol.name_id as usize], "MyAlias");

        // (4) the empty object surface.
        assert!(matches!(
            node_of(4).kind,
            Some(graph_type_node::Kind::Object(_))
        ));

        // (5) `Opaque` — the legacy structurally-unencodable opaque, VERBATIM
        // message, DISTINCT from the no-resolved-type message.
        assert_eq!(
            opaque_message(node_of(5)),
            "member value is structurally unencodable shallowly"
        );
    }
}
