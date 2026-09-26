//! The checker's enum model.
//!
//! Every member of an enum declaration has its own NOMINAL literal type
//! ([`SemanticNodeData::EnumLiteral`]): `E.A` is identified by the enum's
//! type declaration and the member's name, and stands for the member's
//! value (its base). The enum's type is the union of its members' literal
//! types — the one member's literal type when the enum declares one
//! member — so `E.A | E.B` over a two-member `E` IS `E`.
//!
//! One inventory feeds every surface: the prepared value declaration's
//! member facts, merged across every declaration of a merged enum. The
//! type-position `E` and `E.A`, the enum object `typeof E` and the widening
//! of a fresh `E.A` all read it through [`EnumDeclaration`], so the members
//! a type names and the members the object carries are the same nodes.

use std::sync::Arc;

use verter_semantic::analysis::enum_constant::{evaluate_enum_constant, EnumConstant};
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
use verter_type_expr::facts::{EnumMemberEntry, EnumPrimitiveDomain, EnumScalar};

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, EnumLiteralType, LiteralValue, NodeScopeId, PrimitiveKind, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, ValueRootKey,
};

/// One enum declaration: the identity its type carries and its members, in
/// declaration order across every declaration of a merged enum.
pub(super) struct EnumDeclaration {
    /// The enum's type declaration — the identity every member literal
    /// carries and a reference to the enum resolves to.
    pub(super) decl: DeclIdentity,
    /// The members, in declaration order.
    pub(super) members: Arc<[EnumMemberEntry]>,
}

impl ProjectSemanticDispatch<'_> {
    /// The enum the VALUE declaration `symbol` in `canonical`'s `owner`
    /// scope declares, following the export-target chase a `typeof`
    /// reference takes, so a re-exported enum is its declaring file's.
    /// `None` when the declaration is not an enum.
    pub(super) fn enum_declaration(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol: &str,
    ) -> Option<EnumDeclaration> {
        let (declaring_canonical, declaring_owner, declaring_symbol, prepared) =
            self.effective_prepared_value_decl(canonical, owner, symbol)?;
        Self::enum_declaration_of(
            self,
            declaring_canonical,
            declaring_owner,
            declaring_symbol,
            &prepared,
        )
    }

    /// The enum declared AT `symbol` in `canonical`'s `owner` scope — no
    /// export chase: the value declaration beside a type declaration of
    /// the same name. `None` when that value is not an enum.
    pub(super) fn enum_declared_at(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol: &str,
    ) -> Option<EnumDeclaration> {
        let prepared = self
            .ctx
            .prepared_value_decl_return_only(canonical, owner, symbol)?;
        Self::enum_declaration_of(
            self,
            Arc::from(canonical),
            owner,
            Arc::from(symbol),
            &prepared,
        )
    }

    pub(super) fn enum_declaration_of(
        &self,
        declaring_canonical: Arc<str>,
        declaring_owner: verter_type_expr::TopLevelOwnerId,
        declaring_symbol: Arc<str>,
        prepared: &verter_semantic::analysis::type_solver::PreparedValueDecl,
    ) -> Option<EnumDeclaration> {
        if prepared.kind != verter_semantic::analysis::type_eval::ValueDeclKind::Enum {
            return None;
        }
        let members = Arc::clone(&prepared.enum_members.as_ref()?.members);
        let whole_hash = self
            .ctx
            .shallow_file_state(declaring_canonical.as_ref())
            .map_or(crate::semantic_query::HashValue::default(), |state| {
                state.whole_hash
            });
        let enumeration = EnumDeclaration {
            decl: DeclIdentity {
                canonical_id: declaring_canonical,
                owner: declaring_owner,
                whole_hash,
                decl_name: declaring_symbol,
            },
            members,
        };
        if enumeration
            .members
            .iter()
            .all(|entry| entry.initializer.is_none())
        {
            return Some(enumeration);
        }
        // A member whose value depends on another declaration is evaluated
        // now that the declaration can be read.
        let mut visiting = Vec::new();
        let members: Vec<EnumMemberEntry> = enumeration
            .members
            .iter()
            .map(|entry| {
                if entry.initializer.is_none() {
                    return entry.clone();
                }
                let value = self
                    .enum_member_constant(&enumeration, &entry.name, &mut visiting)
                    .map_or(
                        EnumScalar::Primitive(EnumPrimitiveDomain::Number),
                        |constant| constant.to_scalar(),
                    );
                EnumMemberEntry {
                    name: entry.name.clone(),
                    value,
                    initializer: None,
                }
            })
            .collect();
        Some(EnumDeclaration {
            members: Arc::from(members.into_boxed_slice()),
            ..enumeration
        })
    }

    /// The constant value of `enumeration`'s member `name`, evaluating a
    /// pending initializer; `None` for a computed member. `visiting` holds
    /// the members being evaluated: a member reached again through its own
    /// initializer is circular, the checker's error, and no constant.
    fn enum_member_constant(
        &self,
        enumeration: &EnumDeclaration,
        name: &str,
        visiting: &mut Vec<(DeclIdentity, String)>,
    ) -> Option<EnumConstant> {
        let entry = enumeration
            .members
            .iter()
            .find(|entry| entry.name == name)?;
        let Some(initializer) = entry.initializer.as_ref() else {
            return EnumConstant::from_scalar(&entry.value);
        };
        let key = (enumeration.decl.clone(), entry.name.clone());
        if visiting.contains(&key) {
            return None;
        }
        visiting.push(key);
        let constant = evaluate_enum_constant(initializer, |path| {
            self.enum_reference_constant(enumeration, path, visiting)
        });
        visiting.pop();
        constant
    }

    /// The constant a path in `enumeration`'s initializer names: a bare
    /// name is first a member of the enum itself; otherwise the path
    /// resolves as a value reference written in the enum's body does — in
    /// each enclosing namespace, innermost first, then in the file — to
    /// another enum's member or to a `const` variable.
    fn enum_reference_constant(
        &self,
        enumeration: &EnumDeclaration,
        path: &[String],
        visiting: &mut Vec<(DeclIdentity, String)>,
    ) -> Option<EnumConstant> {
        if let [member] = path {
            if enumeration
                .members
                .iter()
                .any(|entry| &entry.name == member)
            {
                return self.enum_member_constant(enumeration, member, visiting);
            }
        }
        let namespaces: Vec<&str> = enumeration
            .decl
            .decl_name
            .rsplit_once('.')
            .map(|(namespace, _)| namespace.split('.').collect())
            .unwrap_or_default();
        for depth in (0..=namespaces.len()).rev() {
            let candidate: Vec<Arc<str>> = namespaces[..depth]
                .iter()
                .map(|segment| Arc::from(*segment))
                .chain(path.iter().map(|segment| Arc::from(segment.as_str())))
                .collect();
            let Some((root, rest)) = candidate.split_first() else {
                continue;
            };
            let value_root = ValueRootKey {
                scope: crate::semantic_query::ScopeId {
                    canonical_id: Arc::clone(&enumeration.decl.canonical_id),
                    owner: enumeration.decl.owner,
                    local_scope: None,
                    binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                        enumeration.decl.owner,
                    ),
                },
                name: Arc::clone(root),
            };
            let Some((identity, remaining)) = self.value_path_declaration(
                &value_root,
                rest,
                &mut rustc_hash::FxHashSet::default(),
            ) else {
                continue;
            };
            match remaining.as_slice() {
                [] => return self.const_variable_constant(&identity),
                [member] => {
                    if let Some(other) = self.enum_declaration_raw(&identity) {
                        return self.enum_member_constant(&other, member, visiting);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The enum a resolved value declaration declares, its pending members
    /// left unevaluated.
    fn enum_declaration_raw(&self, identity: &ResolvedRootIdentity) -> Option<EnumDeclaration> {
        let (canonical, owner, symbol, prepared) = self.effective_prepared_value_decl(
            &identity.canonical_id,
            identity.owner,
            &identity.symbol_name,
        )?;
        if prepared.kind != verter_semantic::analysis::type_eval::ValueDeclKind::Enum {
            return None;
        }
        let whole_hash = self
            .ctx
            .shallow_file_state(canonical.as_ref())
            .map_or(crate::semantic_query::HashValue::default(), |state| {
                state.whole_hash
            });
        Some(EnumDeclaration {
            decl: DeclIdentity {
                canonical_id: canonical,
                owner,
                whole_hash,
                decl_name: symbol,
            },
            members: Arc::clone(&prepared.enum_members.as_ref()?.members),
        })
    }

    /// The constant value of a `const` variable an enum initializer names:
    /// an unannotated `const` whose declared type is the literal its
    /// initializer evaluates to (`const k = 5`, `const c = k`). Any other
    /// variable — annotated, mutable, or of a non-literal type — names no
    /// constant.
    fn const_variable_constant(&self, identity: &ResolvedRootIdentity) -> Option<EnumConstant> {
        use verter_type_expr::facts::DeclaredLiteralFreshness;
        let (canonical, owner, symbol, prepared) = self.effective_prepared_value_decl(
            &identity.canonical_id,
            identity.owner,
            &identity.symbol_name,
        )?;
        if prepared.kind != verter_semantic::analysis::type_eval::ValueDeclKind::Const
            || !matches!(
                prepared.type_annotation.literal_freshness,
                DeclaredLiteralFreshness::Widening | DeclaredLiteralFreshness::Follows(_)
            )
        {
            return None;
        }
        let mut segments = symbol.split('.');
        let root = segments.next()?;
        let path: Arc<[Arc<str>]> = segments.map(Arc::from).collect();
        let value_root = ValueRootKey {
            scope: crate::semantic_query::ScopeId {
                canonical_id: canonical,
                owner,
                local_scope: None,
                binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(owner),
            },
            name: Arc::from(root),
        };
        let QueryResult::Value(crate::semantic_query::SemanticQueryOutput { value, .. }) = self
            .execute_type_node(self.typeof_key_with_path(
                value_root,
                path,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            ))
        else {
            return None;
        };
        match self.graph().node_data(value).as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::Number(number))) => {
                Some(EnumConstant::Number(*number))
            }
            Some(SemanticNodeData::Literal(LiteralValue::String(text))) => {
                Some(EnumConstant::String(text.clone()))
            }
            _ => None,
        }
    }

    /// The literal type of `enumeration`'s member `member`; `None` when the
    /// enum declares no such member.
    pub(super) fn enum_member_type(
        &self,
        enumeration: &EnumDeclaration,
        member: &str,
    ) -> Option<SemanticNodeId> {
        let entry = enumeration
            .members
            .iter()
            .find(|entry| entry.name == member)?;
        Some(self.enum_member_literal(enumeration, entry))
    }

    /// The literal type of one member of `enumeration`.
    pub(super) fn enum_member_literal(
        &self,
        enumeration: &EnumDeclaration,
        entry: &EnumMemberEntry,
    ) -> SemanticNodeId {
        let base = self.enum_member_base(&entry.value);
        let scope = self.enum_scope(enumeration);
        self.graph().intern_node_with_scope(
            SemanticNodeData::EnumLiteral(EnumLiteralType {
                enum_decl: enumeration.decl.clone(),
                member: Arc::from(entry.name.as_str()),
                base,
                member_count: u32::try_from(enumeration.members.len()).unwrap_or(u32::MAX),
            }),
            scope,
        )
    }

    /// The enum's type: the union of its members' literal types, the one
    /// member's literal type for a one-member enum.
    pub(super) fn enum_type(&self, enumeration: &EnumDeclaration) -> SemanticNodeId {
        let members: Vec<SemanticNodeId> = enumeration
            .members
            .iter()
            .map(|entry| self.enum_member_literal(enumeration, entry))
            .collect();
        self.intern_normalized_union_or_intersection(&members, true)
    }

    /// The enum's object (`typeof E`): one readonly property per member,
    /// typed by the member's literal type, and — when a member's value is
    /// numeric or computed — the reverse mapping `readonly [x: number]:
    /// string` the checker gives it (`resolveAnonymousTypeMembers`'s
    /// `enumNumberIndexInfo`), so `E[0]` reads a member's name.
    pub(super) fn enum_object(&self, enumeration: &EnumDeclaration) -> SemanticNodeId {
        let reverse_mapping = enumeration.members.iter().any(|entry| {
            matches!(
                entry.value,
                EnumScalar::Number(_)
                    | EnumScalar::Primitive(
                        EnumPrimitiveDomain::Number | EnumPrimitiveDomain::NumberOrString
                    )
            )
        });
        let index_signature = reverse_mapping.then(|| {
            let graph = self.graph();
            crate::semantic_query::SurfaceEntry::IndexSignature(
                crate::semantic_query::IndexSignature {
                    key_type: graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number)),
                    value_type: graph
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String)),
                    readonly: true,
                    spans: verter_type_expr::IndexSignatureSpans::default(),
                    declaration_origin: Some(Arc::clone(&enumeration.decl.canonical_id)),
                },
            )
        });
        let members = enumeration
            .members
            .iter()
            .map(|entry| crate::semantic_query::SurfaceMember {
                key: crate::semantic_query::AuthoredPropertyKey::String(Arc::from(
                    entry.name.as_str(),
                )),
                value: self.enum_member_literal(enumeration, entry),
                optional: false,
                readonly: true,
                method_kind: None,
                has_implementation_body: false,
                visibility: verter_type_expr::MemberVisibility::Public,
                excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                // The member inventory carries no per-member source span.
                spans: verter_type_expr::MemberSpans::default(),
                declaration_origin: Some(Arc::clone(&enumeration.decl.canonical_id)),
                declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            })
            .map(crate::semantic_query::SurfaceEntry::Member)
            .chain(index_signature)
            .collect();
        self.graph().intern_node_with_scope(
            SemanticNodeData::Object(crate::semantic_query::SurfaceView::from_entries(
                members,
                None,
                reverse_mapping,
            )),
            self.enum_scope(enumeration),
        )
    }

    /// Whether `node` is the object of the enum `literal` is a member of —
    /// the enum object itself, or a `typeof` reference resolving to it.
    pub(super) fn is_enum_object_of(
        &self,
        node: SemanticNodeId,
        literal: &EnumLiteralType,
    ) -> bool {
        let Some(enumeration) = self.enum_declaration(
            literal.enum_decl.canonical_id.as_ref(),
            literal.enum_decl.owner,
            literal.enum_decl.decl_name.as_ref(),
        ) else {
            return false;
        };
        let object = self.enum_object(&enumeration);
        if node == object {
            return true;
        }
        matches!(
            self.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::TypeOf(_))
        ) && self
            .evaluate_deferred_semantic_node_with_context(
                node,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            )
            .into_active_query_build_node(self)
            == object
    }

    /// Whether `node` is an enum's object itself (`typeof E`): its reverse
    /// mapping is no key of it, as the checker leaves `enumNumberIndexInfo`
    /// out of `keyof` (`keyof typeof E` is its member names).
    pub(super) fn is_enum_object(&self, node: SemanticNodeId) -> bool {
        let literal = match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Object(surface)) => {
                match surface
                    .positive_members()
                    .first()
                    .map(|member| member.value)
                {
                    Some(value) => match self.graph().node_data(value).as_deref() {
                        Some(SemanticNodeData::EnumLiteral(literal)) => literal.clone(),
                        _ => return false,
                    },
                    None => return false,
                }
            }
            _ => return false,
        };
        self.enum_declaration(
            literal.enum_decl.canonical_id.as_ref(),
            literal.enum_decl.owner,
            literal.enum_decl.decl_name.as_ref(),
        )
        .is_some_and(|enumeration| self.enum_object(&enumeration) == node)
    }

    fn enum_scope(&self, enumeration: &EnumDeclaration) -> NodeScopeId {
        NodeScopeId::File {
            canonical_id: Arc::clone(&enumeration.decl.canonical_id),
            owner: enumeration.decl.owner,
            whole_hash: enumeration.decl.whole_hash,
            local_scope: None,
        }
    }

    /// The type a FRESH literal widens to (the checker's widened literal
    /// type): a plain literal widens to its primitive, an enum member's
    /// literal to its enum's type. Every other node is its own widening.
    pub(super) fn widened_literal(&self, node: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        let primitive = match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Literal(literal)) => match literal {
                LiteralValue::String(_) => PrimitiveKind::String,
                LiteralValue::Number(_) => PrimitiveKind::Number,
                LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
                LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
            },
            Some(SemanticNodeData::EnumLiteral(literal)) => {
                return self.enum_literal_base_type(literal).unwrap_or(node);
            }
            _ => return node,
        };
        graph.intern_node(SemanticNodeData::Primitive(primitive))
    }

    /// The type a FRESH enum member literal widens to: its enum's type
    /// (the checker's base type of an enum literal). `None` when the enum
    /// no longer resolves.
    pub(super) fn enum_literal_base_type(
        &self,
        literal: &EnumLiteralType,
    ) -> Option<SemanticNodeId> {
        let enumeration = self.enum_declaration(
            literal.enum_decl.canonical_id.as_ref(),
            literal.enum_decl.owner,
            literal.enum_decl.decl_name.as_ref(),
        )?;
        Some(self.enum_type(&enumeration))
    }

    /// The value a member stands for: the number or string literal of a
    /// constant member, the primitive domain of a member whose value is not
    /// a constant.
    fn enum_member_base(&self, value: &EnumScalar) -> SemanticNodeId {
        let graph = self.graph();
        match value {
            EnumScalar::Number(text) => {
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(
                    text.parse::<f64>()
                        .expect("EnumScalar::Number stores the canonical f64 display string"),
                )))
            }
            EnumScalar::String(text) => graph.intern_node(SemanticNodeData::Literal(
                LiteralValue::String(text.clone()),
            )),
            EnumScalar::Primitive(domain) => match domain {
                EnumPrimitiveDomain::Number => {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number))
                }
                EnumPrimitiveDomain::String => {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String))
                }
                EnumPrimitiveDomain::NumberOrString => {
                    let number =
                        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                    let string =
                        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    self.intern_normalized_union_or_intersection(&[number, string], true)
                }
                EnumPrimitiveDomain::Unknown => {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown))
                }
            },
        }
    }
}

/// Whether `node` is a LITERAL type — a plain literal or an enum member's
/// literal — the types a fresh value carries and a widening position
/// widens.
pub(super) fn is_literal_type(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: SemanticNodeId,
) -> bool {
    matches!(
        graph.node_data(node).as_deref(),
        Some(SemanticNodeData::Literal(_) | SemanticNodeData::EnumLiteral(_))
    )
}
