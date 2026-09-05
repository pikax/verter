//! Bounded terminal `TypeExpr` → [`SemanticTypeGraph`] export.
//!
//! The typeinfo graph operations answer with a wire graph — the
//! `TypeInfoGraphResponse` `graph` arm. This module is the encoder that
//! projects ONE already-materialized terminal `TypeExpr` (the sealed
//! output of the typeinfo raise pipeline — never live resolver state)
//! into that wire graph:
//!
//! - **interned strings** — every wire-side name rides the graph string
//!   table in first-encounter order, with id 0 reserved as the absent
//!   sentinel (mirroring node id 0 — a 0 in a name-bearing field never
//!   means "the first interned string");
//! - **deduplicated nodes** — structurally equal sub-expressions share
//!   one node id (identity, no aliasing), assigned in first-encounter
//!   traversal order (a deterministic encode);
//! - **minted symbols** — named references carry a
//!   [`GraphSymbolNode`] (name + canonical) and reference it by id;
//! - **the signatures arena** — function / constructor / method shapes
//!   ride [`GraphSignature`] entries referenced from their object node;
//! - **ordered construction programs** — a spread-bearing object
//!   encodes the source-ordered [`GraphObjectSpreadProgram`], never a
//!   fabricated derived member surface.
//!
//! ## Boundedness (fail-closed, never truncation)
//!
//! The walk runs under explicit node / depth budgets
//! ([`GraphExportBudgets`]), and the budgets bound the WORK, not just
//! the arena: the walk stops at a trip (remaining siblings cost O(1)
//! each and are never encoded or interned), every arena-pushing helper
//! — real nodes, opaque degradations, signatures — consumes or is
//! capped by the node budget, and a subtree that would exceed a budget
//! degrades to a `GraphOpaque { BudgetExceeded }` marker carrying the
//! enforced limit — a typed, explicit stop, never a silently truncated
//! or silently dropped subtree. The arena therefore holds at most
//! `node_budget` counted nodes plus one marker per distinct enforced
//! limit. A validated graph request can never ask for an unbounded
//! export: the envelope validator
//! (`verter_session::typeinfo::request_validation`) rejects a missing
//! closure policy and caps expansion budgets before this encoder runs;
//! the budgets here are the encoder-side enforcement of that contract.
//!
//! ## Degradation vocabulary (explicit, by design)
//!
//! `TypeExpr` arms with no wire node counterpart degrade to
//! `GraphOpaque { Other }` with an interned message naming what was
//! dropped: `SyntheticSlotBinding` (the demand-driven synthetic-carrier
//! deepen route owns those), `ImportType`, and `Unknown`. A
//! `RecursiveRef` encodes a `GraphCycle` rooted at the recursive
//! reference with the participating symbol, plus an elision diagnostic
//! when active conditional frames were dropped. These are the ONLY
//! opaque degradations; every other arm maps to its wire node kind. An
//! index signature whose key domain is not one of the closed wire kinds
//! (a union key, a `keyof` key, a unique-symbol ref) is not degraded
//! either — the whole object takes its ordered construction-program
//! spelling, which keeps the key as a typed node instead of fabricating
//! a closed `key_kind` domain.
//!
//! This module is a PURE projection: no resolution, no parsing, no
//! semantic decisions — the input `TypeExpr` is already terminal.

use std::hash::{Hash, Hasher};

use prost::Message;
use rustc_hash::{FxHashMap, FxHasher};
use verter_type_expr::{
    AuthoredPropertyKey, FunctionExpr, LiteralValue, MappedModifier, MethodSignature, ObjectMember,
    ObjectMethodKind, PrimitiveName, TypeExpr, TypeParam, ValueDeclIdentityPart,
};

use crate::typeinfo::graph::{
    Accessibility, BudgetDomain, DiagnosticSeverity, IndexKeyKind, ObjectMemberKind, PrimitiveKind,
    SignatureKind, SignatureOrigin, SymbolNamespace, TYPEINFO_GRAPH_SCHEMA_VERSION,
};
use crate::verter::v1::{
    graph_literal_value, graph_object_construction_effect, graph_property_key, graph_query_error,
    graph_type_node, GraphAliasInstantiation, GraphArray, GraphConditional, GraphCycle,
    GraphDiagnostic, GraphIndexSignature, GraphIndexedAccess, GraphInfer, GraphIntersection,
    GraphKeyOf, GraphLiteral, GraphLiteralValue, GraphMapped, GraphObject,
    GraphObjectConstructionEffect, GraphObjectIndexEffect, GraphObjectMember,
    GraphObjectNamedEffect, GraphObjectSignatureEffect, GraphObjectSpreadEffect,
    GraphObjectSpreadProgram, GraphOpaque, GraphPrimitive, GraphPropertyKey, GraphQueryError,
    GraphQueryErrorBudgetExceeded, GraphQueryErrorOther, GraphReference, GraphSignature,
    GraphSignatureParameter, GraphStringTable, GraphSymbolNode, GraphTemplateLiteral, GraphTuple,
    GraphTupleElement, GraphTypeNode, GraphTypeOf, GraphTypeParameter, GraphUnion,
    SemanticTypeGraph,
};

/// Sentinel budget value meaning "no limit on this axis". Reserved for
/// internal callers that already hold a validated bounded policy shape;
/// the graph request validator is the authority that rejects unbounded
/// export requests before this encoder is reached.
pub const UNBOUNDED_SENTINEL_BUDGET: u32 = u32::MAX;

/// Node / depth budgets for one bounded export. Both axes are enforced
/// independently; a value of [`UNBOUNDED_SENTINEL_BUDGET`] disables that
/// axis (internal callers only — see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphExportBudgets {
    /// Maximum number of real (non-marker) nodes the arena may hold.
    pub node_budget: u32,
    /// Maximum traversal depth; the root sits at depth 0.
    pub depth_budget: u32,
}

/// Encode one terminal [`TypeExpr`] into a fresh bounded
/// [`SemanticTypeGraph`]. The returned graph carries the node arena,
/// symbol table, signatures arena, interned strings, and exactly one
/// root id; `query` (the query identity) is left unset — the executing
/// operation fills it from its validated request.
///
/// Deterministic: the same expression under the same budgets always
/// encodes to the same graph (interning and id assignment are
/// first-encounter order, never hash order).
#[must_use]
pub fn encode_type_expr_graph(root: &TypeExpr, budgets: &GraphExportBudgets) -> SemanticTypeGraph {
    let mut exporter = GraphExporter::new(budgets);
    let root_id = exporter.expr_node(root, 0);
    exporter.finish(vec![root_id])
}

/// The exporter's internal arenas. Own-once: the tables own every
/// string / symbol / signature / node; the dedup maps hold only ids.
struct GraphExporter<'a> {
    budgets: &'a GraphExportBudgets,
    /// Interned strings, first-encounter order (id = index).
    strings: Vec<String>,
    string_ids: FxHashMap<String, u32>,
    /// Minted symbols, first-encounter order (id = index).
    symbols: Vec<GraphSymbolNode>,
    symbol_ids: FxHashMap<String, u32>,
    /// Signatures arena, append order (ref = index).
    signatures: Vec<GraphSignature>,
    /// Node arena, first-encounter order (id = index).
    nodes: Vec<GraphTypeNode>,
    /// Content-hash dedup: hash of a node's encoded bytes → candidate
    /// node ids. Lookups confirm candidates by full node equality, so a
    /// hash collision can never alias two distinct nodes. The encoded
    /// bytes themselves are never retained — one reusable scratch buffer
    /// serves every encode.
    node_hashes: FxHashMap<u64, Vec<u32>>,
    /// Reusable encode scratch for the dedup hash (empty between calls).
    scratch: Vec<u8>,
    /// Lazily-minted budget-marker node per enforced limit.
    budget_markers: FxHashMap<u32, u32>,
    /// Count of real (non-reserved, non-marker) nodes pushed — the node
    /// budget's accounting axis.
    real_nodes: u32,
    /// Degradation diagnostics (e.g. an elided recursive conditional
    /// context) — surfaced on the graph, never dropped silently.
    diagnostics: Vec<GraphDiagnostic>,
}

impl<'a> GraphExporter<'a> {
    fn new(budgets: &'a GraphExportBudgets) -> Self {
        let mut exporter = Self {
            budgets,
            strings: Vec::new(),
            string_ids: FxHashMap::default(),
            symbols: Vec::new(),
            symbol_ids: FxHashMap::default(),
            signatures: Vec::new(),
            nodes: Vec::new(),
            node_hashes: FxHashMap::default(),
            scratch: Vec::new(),
            budget_markers: FxHashMap::default(),
            real_nodes: 0,
            diagnostics: Vec::new(),
        };
        // Node id 0 is the wire "absent" sentinel (every child-ref field
        // documents 0 = absent), so no real node may occupy it. Reserve
        // the slot with an unset-kind node; every real id is >= 1 and a
        // child ref of 0 stays unambiguously "absent".
        exporter.nodes.push(GraphTypeNode { kind: None });
        // String id 0 mirrors that convention for name-bearing fields
        // (tuple labels, signature parameter names): the reserved entry
        // is deliberately NOT indexed in `string_ids`, so a real empty
        // string interns to its own id (>= 1) and never aliases the
        // sentinel.
        exporter.strings.push(String::new());
        exporter
    }

    fn finish(self, root_ids: Vec<u32>) -> SemanticTypeGraph {
        // `expr_node` always pushes at least the root node (the reserved
        // absent-sentinel slot occupies id 0 before it), so the arena is
        // never empty here — an empty arena would alias "resolved to
        // nothing" with a broken encode.
        let Self {
            strings,
            symbols,
            signatures,
            nodes,
            diagnostics,
            ..
        } = self;
        SemanticTypeGraph {
            schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
            query: None,
            nodes,
            symbols,
            signatures,
            edges: Vec::new(),
            root_ids,
            exactness: Vec::new(),
            diagnostics,
            node_id_map: Vec::new(),
            symbol_id_map: Vec::new(),
            strings: Some(GraphStringTable { entries: strings }),
            relation_proofs: Vec::new(),
        }
    }

    fn string_id(&mut self, value: &str) -> u32 {
        if let Some(id) = self.string_ids.get(value) {
            return *id;
        }
        let id = u32::try_from(self.strings.len()).unwrap_or(u32::MAX);
        self.strings.push(value.to_string());
        self.string_ids.insert(value.to_string(), id);
        id
    }

    /// Mint (or reuse) a type-namespace symbol for `name` with
    /// `canonical` as its canonical source name.
    fn symbol_id(&mut self, name: &str, canonical: &str) -> u32 {
        if let Some(id) = self.symbol_ids.get(name) {
            return *id;
        }
        let name_id = self.string_id(name);
        let canonical_id = self.string_id(canonical);
        let id = u32::try_from(self.symbols.len()).unwrap_or(u32::MAX);
        self.symbols.push(GraphSymbolNode {
            name_id,
            canonical_name_id: canonical_id,
            namespace: SymbolNamespace::Type as i32,
            decl_slot_ref: 0,
        });
        self.symbol_ids.insert(name.to_string(), id);
        id
    }

    fn signature_ref(&mut self, signature: GraphSignature) -> u32 {
        let id = u32::try_from(self.signatures.len()).unwrap_or(u32::MAX);
        self.signatures.push(signature);
        id
    }

    /// Push a node under the budget contract, deduplicating structurally
    /// equal nodes. Real nodes — opaque degradations included — consume
    /// the node budget; when it is exhausted the caller receives the
    /// shared budget marker instead.
    fn push_node(&mut self, kind: graph_type_node::Kind) -> u32 {
        if self.real_nodes >= self.budgets.node_budget {
            return self.budget_marker(self.budgets.node_budget);
        }
        let node = GraphTypeNode { kind: Some(kind) };
        // Content-hash identity: encode into the reusable scratch buffer,
        // hash, and confirm candidates by full equality — structurally
        // equal wire nodes are byte-equal (prost encoding is
        // deterministic), and the confirmation step means a hash
        // collision cannot alias distinct nodes. No per-node byte copy is
        // retained on the export path.
        self.scratch.clear();
        node.encode(&mut self.scratch)
            .expect("encoding into a growable Vec buffer cannot fail");
        let mut hasher = FxHasher::default();
        self.scratch.hash(&mut hasher);
        let hash = hasher.finish();
        if let Some(ids) = self.node_hashes.get(&hash) {
            for &id in ids {
                if self.nodes[id as usize] == node {
                    return id;
                }
            }
        }
        let id = u32::try_from(self.nodes.len()).unwrap_or(u32::MAX);
        self.nodes.push(node);
        self.node_hashes.entry(hash).or_default().push(id);
        self.real_nodes += 1;
        id
    }

    /// The shared `BudgetExceeded` marker for `limit`. Marker nodes do
    /// not consume the node budget — they report its exhaustion; at most
    /// one marker per distinct limit exists per graph, so the arena is
    /// bounded by node_budget + distinct limits.
    fn budget_marker(&mut self, limit: u32) -> u32 {
        if let Some(id) = self.budget_markers.get(&limit) {
            return *id;
        }
        let error = GraphQueryError {
            kind: Some(graph_query_error::Kind::BudgetExceeded(
                GraphQueryErrorBudgetExceeded {
                    domain: BudgetDomain::BuilderExpansion as i32,
                    limit,
                    actual: 0,
                    context_name_id: 0,
                },
            )),
        };
        let node = GraphTypeNode {
            kind: Some(graph_type_node::Kind::Opaque(GraphOpaque {
                error: Some(error),
            })),
        };
        let id = u32::try_from(self.nodes.len()).unwrap_or(u32::MAX);
        self.nodes.push(node);
        self.budget_markers.insert(limit, id);
        id
    }

    fn opaque_other(&mut self, message: &str) -> u32 {
        let message_id = self.string_id(message);
        let error = GraphQueryError {
            kind: Some(graph_query_error::Kind::Other(GraphQueryErrorOther {
                message_name_id: message_id,
            })),
        };
        // Opaque degradations ride the same budgeted push as every real
        // node: identical degradations dedup to one node, and a
        // degradation past the budget reports the budget marker instead
        // of growing an uncounted arena.
        self.push_node(graph_type_node::Kind::Opaque(GraphOpaque {
            error: Some(error),
        }))
    }

    fn elision_diagnostic(&mut self, message: &str) {
        let message_id = self.string_id(message);
        self.diagnostics.push(GraphDiagnostic {
            severity: DiagnosticSeverity::Info as i32,
            message_name_id: message_id,
            span_canonical_name_id: 0,
            span_start: 0,
            span_end: 0,
            has_span: false,
        });
    }

    /// Encode one expression at `depth`. Depth exhaustion degrades the
    /// WHOLE subtree to the depth-budget marker — explicit, typed, never
    /// a truncated walk. The node-budget check sits at the ENTRY: once
    /// the arena is full the walk stops before any child work or
    /// interning, so post-trip cost is O(1) per pending sibling.
    fn expr_node(&mut self, expr: &TypeExpr, depth: u32) -> u32 {
        if depth >= self.budgets.depth_budget {
            return self.budget_marker(self.budgets.depth_budget);
        }
        if self.real_nodes >= self.budgets.node_budget {
            return self.budget_marker(self.budgets.node_budget);
        }
        match expr {
            TypeExpr::Primitive(name) => {
                self.push_node(graph_type_node::Kind::Primitive(GraphPrimitive {
                    kind: primitive_kind(*name) as i32,
                }))
            }
            TypeExpr::Literal(value) => {
                let literal = match value {
                    LiteralValue::String(s) => {
                        graph_literal_value::Kind::StringNameId(self.string_id(s))
                    }
                    LiteralValue::Number(n) => graph_literal_value::Kind::NumberBits(n.to_bits()),
                    LiteralValue::Boolean(b) => graph_literal_value::Kind::BooleanValue(*b),
                    LiteralValue::BigInt(text) => {
                        graph_literal_value::Kind::BigintNameId(self.string_id(text))
                    }
                };
                self.push_node(graph_type_node::Kind::Literal(GraphLiteral {
                    value: Some(GraphLiteralValue {
                        kind: Some(literal),
                    }),
                }))
            }
            TypeExpr::Union(members) => {
                let ids = self.expr_ids(members, depth);
                self.push_node(graph_type_node::Kind::Union(GraphUnion {
                    member_node_ids: ids,
                }))
            }
            TypeExpr::Intersection(members) => {
                let ids = self.expr_ids(members, depth);
                self.push_node(graph_type_node::Kind::Intersection(GraphIntersection {
                    member_node_ids: ids,
                }))
            }
            TypeExpr::Array { element, readonly } => {
                let element_id = self.expr_node(element, depth + 1);
                self.push_node(graph_type_node::Kind::Array(GraphArray {
                    element_node_id: element_id,
                    readonly: *readonly,
                }))
            }
            TypeExpr::Tuple { elements, readonly } => {
                let encoded = elements
                    .iter()
                    .map(|element| GraphTupleElement {
                        label_name_id: element
                            .label
                            .as_deref()
                            .map(|label| self.string_id(label))
                            .unwrap_or(0),
                        value_node_id: self.expr_node(&element.ty, depth + 1),
                        optional: element.optional,
                        rest: element.rest,
                    })
                    .collect();
                self.push_node(graph_type_node::Kind::Tuple(GraphTuple {
                    elements: encoded,
                    readonly: *readonly,
                }))
            }
            TypeExpr::Object(object) => self.object_node(&object.properties, depth),
            TypeExpr::Function(function) => self.callable_object_node(function, depth, false),
            TypeExpr::ConstructorType(function) => self.callable_object_node(function, depth, true),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => self.reference_node(name, type_arguments, depth),
            TypeExpr::TypeParameter(param) => self.type_parameter_node(param, depth),
            TypeExpr::KeyOf(operand) => {
                let base = self.expr_node(operand, depth + 1);
                self.push_node(graph_type_node::Kind::KeyOf(GraphKeyOf {
                    base_node_id: base,
                }))
            }
            TypeExpr::TypeOf(value) => {
                if !value.type_args.is_empty() {
                    // Instantiation-expression args on `typeof` have no wire
                    // counterpart — degrade explicitly rather than drop.
                    return self.opaque_other("typeof instantiation-expression type arguments");
                }
                let path = value
                    .path
                    .iter()
                    .map(|segment| self.string_id(segment))
                    .collect();
                self.push_node(graph_type_node::Kind::TypeofNode(GraphTypeOf {
                    value_root_ref: 0,
                    path_name_ids: path,
                }))
            }
            TypeExpr::IndexedAccess { object, index } => {
                let object_id = self.expr_node(object, depth + 1);
                let index_id = self.expr_node(index, depth + 1);
                self.push_node(graph_type_node::Kind::IndexedAccess(GraphIndexedAccess {
                    object_node_id: object_id,
                    index_node_id: index_id,
                }))
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                let check_id = self.expr_node(check, depth + 1);
                let extends_id = self.expr_node(extends, depth + 1);
                let true_id = self.expr_node(true_type, depth + 1);
                let false_id = self.expr_node(false_type, depth + 1);
                self.push_node(graph_type_node::Kind::Conditional(GraphConditional {
                    check_node_id: check_id,
                    extends_node_id: extends_id,
                    true_branch_node_id: true_id,
                    false_branch_node_id: false_id,
                    distributive: false,
                    resolution: None,
                }))
            }
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
            } => {
                // The mapped key parameter rides a type-parameter node —
                // its name is the key identity (`[K in S]`).
                let key_id = self.type_parameter_node(
                    &TypeParam {
                        name: parameter.clone(),
                        constraint: None,
                        default: None,
                        is_const: false,
                    },
                    depth + 1,
                );
                let source_id = self.expr_node(source, depth + 1);
                let value_id = self.expr_node(value, depth + 1);
                let remap_id = name_type
                    .as_ref()
                    .map(|name| self.expr_node(name, depth + 1))
                    .unwrap_or(0);
                self.push_node(graph_type_node::Kind::Mapped(GraphMapped {
                    key_type_node_id: key_id,
                    source_node_id: source_id,
                    name_remap_node_id: remap_id,
                    value_type_node_id: value_id,
                    readonly_modifier: mapped_modifier(*readonly),
                    optional_modifier: mapped_modifier(*optional),
                }))
            }
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => {
                let quasi_ids = quasis.iter().map(|q| self.string_id(q)).collect();
                let expression_ids = self.expr_ids(expressions, depth);
                self.push_node(graph_type_node::Kind::TemplateLiteral(
                    GraphTemplateLiteral {
                        quasi_name_ids: quasi_ids,
                        expression_node_ids: expression_ids,
                    },
                ))
            }
            TypeExpr::Infer { name } => {
                let name_id = self.string_id(name);
                self.push_node(graph_type_node::Kind::InferNode(GraphInfer {
                    name_id,
                    constraint_node_id: 0,
                }))
            }
            TypeExpr::Rest(inner) => {
                // A standalone rest only occurs in tuple position; its
                // single-rest-element tuple spelling is the lossless wire
                // form.
                let value_id = self.expr_node(inner, depth + 1);
                self.push_node(graph_type_node::Kind::Tuple(GraphTuple {
                    elements: vec![GraphTupleElement {
                        label_name_id: 0,
                        value_node_id: value_id,
                        optional: false,
                        rest: true,
                    }],
                    readonly: false,
                }))
            }
            TypeExpr::Parenthesized(inner) => {
                // Parenthesisation is fidelity-only; the graph is the
                // semantic projection (the descriptor path unwraps the
                // same way).
                self.expr_node(inner, depth)
            }
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                conditional_context,
            } => {
                let reference_id = self.reference_node(name, type_arguments, depth);
                let participants = vec![self.symbol_id(name, name)];
                if !conditional_context.is_empty() {
                    self.elision_diagnostic(
                        "recursive conditional context elided by the bounded graph export",
                    );
                }
                self.push_node(graph_type_node::Kind::Cycle(GraphCycle {
                    cycle_root_node_id: reference_id,
                    participants,
                }))
            }
            TypeExpr::SyntheticSlotBinding(key) => self.opaque_other(&format!(
                "synthetic slot-binding carrier (scope {}, binding {})",
                key.scope_canonical_id, key.binding_name
            )),
            TypeExpr::ImportType {
                specifier,
                qualifier,
                ..
            } => {
                let mut path = String::new();
                for part in qualifier.iter() {
                    path.push('.');
                    path.push_str(part);
                }
                self.opaque_other(&format!("import type ({specifier}{path})"))
            }
            TypeExpr::Unknown(value) => self.opaque_other(value.raw()),
        }
    }

    fn expr_ids(&mut self, exprs: &[TypeExpr], depth: u32) -> Vec<u32> {
        exprs
            .iter()
            .map(|expr| self.expr_node(expr, depth + 1))
            .collect()
    }

    /// A named reference: bare refs encode `GraphReference`; a
    /// parameterized ref encodes `GraphAliasInstantiation` carrying the
    /// argument nodes (no fabricated body — expansion is a separate
    /// bounded operation).
    fn reference_node(&mut self, name: &str, type_arguments: &[TypeExpr], depth: u32) -> u32 {
        let symbol_id = self.symbol_id(name, name);
        if type_arguments.is_empty() {
            return self.push_node(graph_type_node::Kind::Reference(GraphReference {
                symbol_id,
            }));
        }
        let argument_ids = self.expr_ids(type_arguments, depth);
        self.push_node(graph_type_node::Kind::AliasInstantiation(
            GraphAliasInstantiation {
                alias_symbol_id: symbol_id,
                type_argument_node_ids: argument_ids,
                target_node_id: 0,
                display_ref_node_id: 0,
            },
        ))
    }

    fn type_parameter_node(&mut self, param: &TypeParam, depth: u32) -> u32 {
        let name_id = self.string_id(&param.name);
        let symbol_id = self.symbol_id(&param.name, &param.name);
        let constraint_id = param
            .constraint
            .as_ref()
            .map(|constraint| self.expr_node(constraint, depth + 1))
            .unwrap_or(0);
        let default_id = param
            .default
            .as_ref()
            .map(|default| self.expr_node(default, depth + 1))
            .unwrap_or(0);
        self.push_node(graph_type_node::Kind::TypeParameter(GraphTypeParameter {
            symbol_id,
            decl_slot_ref: 0,
            param_index: 0,
            name_id,
            constraint_node_id: constraint_id,
            default_node_id: default_id,
            variance: 0,
            is_const: param.is_const,
            no_infer: false,
            binding: None,
        }))
    }

    /// A function / constructor type is its callable object spelling: an
    /// object node whose (construct-)call signature refs index the
    /// signatures arena.
    fn callable_object_node(
        &mut self,
        function: &FunctionExpr,
        depth: u32,
        construct: bool,
    ) -> u32 {
        let Some(signature_ref) = self.function_signature(function, depth, construct) else {
            return self.budget_marker(self.budgets.node_budget);
        };
        let (call_refs, construct_refs) = if construct {
            (Vec::new(), vec![signature_ref])
        } else {
            (vec![signature_ref], Vec::new())
        };
        self.push_node(graph_type_node::Kind::Object(GraphObject {
            members: Vec::new(),
            index_signatures: Vec::new(),
            call_signature_refs: call_refs,
            construct_signature_refs: construct_refs,
            flags: 0,
        }))
    }

    /// Encode one function shape into the signatures arena. `None` is the
    /// arena's budget cap: the signatures arena is bounded by the same
    /// node-budget axis (one signature per callable node keeps it
    /// O(node_budget)), and past the cap the ENCLOSING node degrades to
    /// the budget marker — never an uncapped arena, never a silently
    /// dropped signature.
    fn function_signature(
        &mut self,
        function: &FunctionExpr,
        depth: u32,
        construct: bool,
    ) -> Option<u32> {
        if self.signatures.len() >= self.budgets.node_budget as usize {
            return None;
        }
        let type_parameter_ids = function
            .type_parameters
            .iter()
            .map(|param| self.type_parameter_node(param, depth + 1))
            .collect();
        let parameters = function
            .parameters
            .iter()
            .map(|param| GraphSignatureParameter {
                name_id: param
                    .name
                    .as_deref()
                    .map(|name| self.string_id(name))
                    .unwrap_or(0),
                type_node_id: self.expr_node(&param.ty, depth + 1),
                optional: param.optional,
                rest: param.rest,
                inference_policy: 0,
            })
            .collect();
        // A missing return annotation is honest absence: the signature
        // names it (0 = absent) rather than fabricating a `void`.
        let return_id = function
            .return_type
            .as_ref()
            .map(|ret| self.expr_node(ret, depth + 1))
            .unwrap_or(0);
        Some(self.signature_ref(GraphSignature {
            type_parameter_node_ids: type_parameter_ids,
            this_param: None,
            parameters,
            return_type_node_id: return_id,
            return_predicate: None,
            asserts: None,
            overload_index: 0,
            is_construct: construct,
            is_implementation: false,
            is_abstract: false,
            flags: 0,
            signature_kind: if construct {
                SignatureKind::Construct as i32
            } else {
                SignatureKind::Call as i32
            },
            signature_origin: if construct {
                SignatureOrigin::ConstructSignature as i32
            } else {
                SignatureOrigin::CallSignature as i32
            },
        }))
    }

    fn object_node(&mut self, members: &[ObjectMember], depth: u32) -> u32 {
        if members.iter().any(|m| matches!(m, ObjectMember::Spread(_))) {
            return self.spread_program_node(members, depth);
        }
        // An index signature whose key domain is not one of the closed
        // wire kinds cannot ride `GraphIndexSignature.key_kind` without
        // fabricating a closed domain (the proto default's silent
        // `String` aliasing) — the ordered construction program keeps the
        // key as a typed node instead (fail-closed, never a made-up
        // domain).
        if members.iter().any(|m| match m {
            ObjectMember::IndexSignature(index) => closed_index_key_kind(&index.key_type).is_none(),
            _ => false,
        }) {
            return self.spread_program_node(members, depth);
        }
        let mut encoded_members = Vec::new();
        let mut index_signatures = Vec::new();
        let mut call_refs = Vec::new();
        let mut construct_refs = Vec::new();
        for member in members {
            // A budget trip mid-walk stops the sibling loop: remaining
            // members are neither walked nor interned, and the enclosing
            // push below degrades to the budget marker — no discarded
            // width-proportional work.
            if self.real_nodes >= self.budgets.node_budget {
                break;
            }
            match member {
                ObjectMember::Property(property) => {
                    let value_id = self.expr_node(&property.ty, depth + 1);
                    let key = self.property_key(&property.key, depth);
                    encoded_members.push(GraphObjectMember {
                        value_node_id: value_id,
                        optional: property.optional,
                        readonly: property.readonly,
                        accessibility: Accessibility::None as i32,
                        static_side: false,
                        declaration_symbol_id: 0,
                        property_key: Some(key),
                        member_kind: ObjectMemberKind::Property as i32,
                        has_implementation_body: false,
                    });
                }
                ObjectMember::Method(method) => {
                    let value_id = self.method_value_node(method, depth);
                    let key = self.property_key(&method.key, depth);
                    encoded_members.push(GraphObjectMember {
                        value_node_id: value_id,
                        optional: method.optional,
                        readonly: false,
                        accessibility: Accessibility::None as i32,
                        static_side: false,
                        declaration_symbol_id: 0,
                        property_key: Some(key),
                        member_kind: match method.method_kind {
                            ObjectMethodKind::Method => ObjectMemberKind::Method,
                            ObjectMethodKind::Get => ObjectMemberKind::Get,
                            ObjectMethodKind::Set => ObjectMemberKind::Set,
                        } as i32,
                        has_implementation_body: method.has_implementation_body,
                    });
                }
                ObjectMember::IndexSignature(index) => {
                    let value_id = self.expr_node(&index.value_type, depth + 1);
                    let Some(key_kind) = closed_index_key_kind(&index.key_type) else {
                        unreachable!(
                            "non-closed index keys route through the construction program"
                        );
                    };
                    index_signatures.push(GraphIndexSignature {
                        key_kind,
                        value_node_id: value_id,
                        readonly: index.readonly,
                    });
                }
                ObjectMember::CallSignature(function) => {
                    let Some(signature_ref) = self.function_signature(function, depth, false)
                    else {
                        return self.budget_marker(self.budgets.node_budget);
                    };
                    call_refs.push(signature_ref);
                }
                ObjectMember::ConstructSignature(function) => {
                    let Some(signature_ref) = self.function_signature(function, depth, true) else {
                        return self.budget_marker(self.budgets.node_budget);
                    };
                    construct_refs.push(signature_ref);
                }
                ObjectMember::Spread(_) => unreachable!("the spread branch is handled above"),
            }
        }
        self.push_node(graph_type_node::Kind::Object(GraphObject {
            members: encoded_members,
            index_signatures,
            call_signature_refs: call_refs,
            construct_signature_refs: construct_refs,
            flags: 0,
        }))
    }

    /// A method member's value: the callable-object node carrying the
    /// method's signature (origin `MethodDeclaration` / accessor).
    fn method_value_node(&mut self, method: &MethodSignature, depth: u32) -> u32 {
        let Some(signature_ref) = self.function_signature(&method.function, depth, false) else {
            return self.budget_marker(self.budgets.node_budget);
        };
        if let Some(signature) = self.signatures.get_mut(signature_ref as usize) {
            signature.signature_origin = match method.method_kind {
                ObjectMethodKind::Method => SignatureOrigin::MethodDeclaration,
                ObjectMethodKind::Get => SignatureOrigin::GetterAccessor,
                ObjectMethodKind::Set => SignatureOrigin::SetterAccessor,
            } as i32;
        }
        self.push_node(graph_type_node::Kind::Object(GraphObject {
            members: Vec::new(),
            index_signatures: Vec::new(),
            call_signature_refs: vec![signature_ref],
            construct_signature_refs: Vec::new(),
            flags: 0,
        }))
    }

    /// The canonical ordered construction program for a spread-bearing
    /// object (or one whose index keys are not closed wire kinds): one
    /// effect per source member, spread operands and index key types as
    /// raw typed nodes.
    fn spread_program_node(&mut self, members: &[ObjectMember], depth: u32) -> u32 {
        let mut effects = Vec::new();
        for member in members {
            // Same mid-walk stop as the plain object walk: once the node
            // budget trips, remaining effects are not encoded or
            // interned.
            if self.real_nodes >= self.budgets.node_budget {
                break;
            }
            let effect = match member {
                ObjectMember::Property(property) => {
                    let value_id = self.expr_node(&property.ty, depth + 1);
                    let key = self.property_key(&property.key, depth);
                    graph_object_construction_effect::Kind::DirectProperty(GraphObjectNamedEffect {
                        property_key: Some(key),
                        value_node_id: value_id,
                        optional: property.optional,
                        readonly: property.readonly,
                        has_implementation_body: false,
                        accessibility: Accessibility::None as i32,
                        spans: None,
                        declaration_origin_name_id: 0,
                        has_declaration_origin: false,
                        declared_in_macro_type_arg: false,
                        merge_role: 0,
                        excess_origin: 0,
                    })
                }
                ObjectMember::Method(method) => {
                    let value_id = self.method_value_node(method, depth);
                    let key = self.property_key(&method.key, depth);
                    graph_object_construction_effect::Kind::DirectMethod(GraphObjectNamedEffect {
                        property_key: Some(key),
                        value_node_id: value_id,
                        optional: method.optional,
                        readonly: false,
                        has_implementation_body: method.has_implementation_body,
                        accessibility: Accessibility::None as i32,
                        spans: None,
                        declaration_origin_name_id: 0,
                        has_declaration_origin: false,
                        declared_in_macro_type_arg: false,
                        merge_role: 0,
                        excess_origin: 0,
                    })
                }
                ObjectMember::IndexSignature(index) => {
                    let key_id = self.expr_node(&index.key_type, depth + 1);
                    let value_id = self.expr_node(&index.value_type, depth + 1);
                    graph_object_construction_effect::Kind::DirectIndex(GraphObjectIndexEffect {
                        key_type_node_id: key_id,
                        value_type_node_id: value_id,
                        readonly: index.readonly,
                        spans: None,
                        declaration_origin_name_id: 0,
                        has_declaration_origin: false,
                    })
                }
                ObjectMember::CallSignature(function) => {
                    let Some(signature_ref) = self.function_signature(function, depth, false)
                    else {
                        return self.budget_marker(self.budgets.node_budget);
                    };
                    graph_object_construction_effect::Kind::DirectCall(GraphObjectSignatureEffect {
                        signature_node_id: signature_ref,
                    })
                }
                ObjectMember::ConstructSignature(function) => {
                    let Some(signature_ref) = self.function_signature(function, depth, true) else {
                        return self.budget_marker(self.budgets.node_budget);
                    };
                    graph_object_construction_effect::Kind::DirectConstruct(
                        GraphObjectSignatureEffect {
                            signature_node_id: signature_ref,
                        },
                    )
                }
                ObjectMember::Spread(spread) => {
                    let operand_id = self.expr_node(&spread.ty, depth + 1);
                    graph_object_construction_effect::Kind::Spread(GraphObjectSpreadEffect {
                        operand_node_id: operand_id,
                    })
                }
            };
            effects.push(GraphObjectConstructionEffect { kind: Some(effect) });
        }
        self.push_node(graph_type_node::Kind::ObjectSpreadProgram(
            GraphObjectSpreadProgram { effects },
        ))
    }

    fn property_key(
        &mut self,
        key: &AuthoredPropertyKey<TypeExpr, ValueDeclIdentityPart>,
        depth: u32,
    ) -> GraphPropertyKey {
        let key = match key {
            AuthoredPropertyKey::String(name) => {
                graph_property_key::Key::StringId(self.string_id(name))
            }
            AuthoredPropertyKey::Number(number) => {
                graph_property_key::Key::CanonicalNumber(number.get())
            }
            AuthoredPropertyKey::UniqueSymbol(identity) => {
                // The unique symbol rides the symbol table (name +
                // canonical source); the wire's flat decl-id ref cannot
                // carry the owner / member path.
                graph_property_key::Key::UniqueSymbolDeclId(self.unique_symbol_id(identity))
            }
            AuthoredPropertyKey::Computed(child) => {
                graph_property_key::Key::ComputedNodeId(self.expr_node(child, depth + 1))
            }
        };
        GraphPropertyKey { key: Some(key) }
    }

    fn unique_symbol_id(&mut self, identity: &ValueDeclIdentityPart) -> u32 {
        let cache_key = format!("{}\u{0}{}", identity.canonical_id, identity.symbol);
        if let Some(id) = self.symbol_ids.get(&cache_key) {
            return *id;
        }
        let name_id = self.string_id(&identity.symbol);
        let canonical_id = self.string_id(&identity.canonical_id);
        let id = u32::try_from(self.symbols.len()).unwrap_or(u32::MAX);
        self.symbols.push(GraphSymbolNode {
            name_id,
            canonical_name_id: canonical_id,
            namespace: SymbolNamespace::Value as i32,
            decl_slot_ref: 0,
        });
        self.symbol_ids.insert(cache_key, id);
        id
    }
}

fn primitive_kind(name: PrimitiveName) -> PrimitiveKind {
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

fn mapped_modifier(modifier: MappedModifier) -> i32 {
    match modifier {
        MappedModifier::None => 0,
        MappedModifier::Add => 1,
        MappedModifier::Remove => 2,
    }
}

/// The closed wire `IndexKeyKind` for an index-signature key domain, or
/// `None` when the domain is not one of the closed kinds. `None` is NOT a
/// fallback: the proto default (`String`) would fabricate a closed domain
/// the source never claimed (a `string | number` union key, a unique
/// symbol, a `keyof` result), so callers route `None` to the construction
/// program, which keeps the key as a typed node.
fn closed_index_key_kind(key_type: &TypeExpr) -> Option<i32> {
    match key_type {
        TypeExpr::Primitive(PrimitiveName::String) => Some(IndexKeyKind::String as i32),
        TypeExpr::Primitive(PrimitiveName::Number) => Some(IndexKeyKind::Number as i32),
        TypeExpr::Primitive(PrimitiveName::Symbol) => Some(IndexKeyKind::Symbol as i32),
        TypeExpr::TemplateLiteral { .. } => Some(IndexKeyKind::TemplatePattern as i32),
        _ => None,
    }
}
