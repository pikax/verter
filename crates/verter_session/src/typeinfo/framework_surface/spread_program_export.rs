//! The production wire serializer for spread-bearing objects: the canonical
//! source-ordered construction program (tag 33). Split from `graph_export`
//! so the shallow member-value encoder stays focused.

use verter_protocol::verter::v1::graph_type_node;
use verter_type_expr::TypeExpr;

use super::graph_export::GraphArena;

impl GraphArena {
    /// Encode a spread-bearing object as the canonical source-ordered
    /// construction program (tag 33). The raised `TypeExpr` preserves exact
    /// effect order, typed keys, accessor kinds, and authored readonly /
    /// optionality; member values follow the same shallow vocabulary as
    /// every other member (deep or structural values degrade to
    /// `GraphOpaque`). Member spans carry no file identity at this layer, so
    /// they are omitted rather than emitted with a fabricated one.
    pub(super) fn encode_object_spread_program(
        &mut self,
        obj: &verter_type_expr::ObjectExpr,
        depth: u32,
    ) -> u32 {
        use verter_protocol::verter::v1::{
            graph_object_construction_effect, GraphObjectConstructionEffect,
            GraphObjectIndexEffect, GraphObjectSignatureEffect, GraphObjectSpreadEffect,
            GraphObjectSpreadProgram,
        };

        let mut effects = Vec::with_capacity(obj.properties.len());
        for member in &obj.properties {
            let kind = match member {
                verter_type_expr::ObjectMember::Property(prop) => {
                    graph_object_construction_effect::Kind::DirectProperty(
                        self.encode_named_effect(
                            &prop.key,
                            &prop.ty,
                            prop.optional,
                            prop.readonly,
                            false,
                            prop.visibility,
                            prop.excess_origin,
                            depth,
                        ),
                    )
                }
                verter_type_expr::ObjectMember::Method(method) => {
                    let named = self.encode_named_effect(
                        &method.key,
                        &TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                        method.optional,
                        false,
                        method.has_implementation_body,
                        method.visibility,
                        method.excess_origin,
                        depth,
                    );
                    match method.method_kind {
                        verter_type_expr::ObjectMethodKind::Method => {
                            graph_object_construction_effect::Kind::DirectMethod(named)
                        }
                        verter_type_expr::ObjectMethodKind::Get => {
                            graph_object_construction_effect::Kind::DirectGet(named)
                        }
                        verter_type_expr::ObjectMethodKind::Set => {
                            graph_object_construction_effect::Kind::DirectSet(named)
                        }
                    }
                }
                verter_type_expr::ObjectMember::IndexSignature(index) => {
                    graph_object_construction_effect::Kind::DirectIndex(GraphObjectIndexEffect {
                        key_type_node_id: self
                            .encode_member_value(Some(&index.key_type), depth + 1),
                        value_type_node_id: self
                            .encode_member_value(Some(&index.value_type), depth + 1),
                        readonly: index.readonly,
                        spans: None,
                        declaration_origin_name_id: 0,
                        has_declaration_origin: false,
                    })
                }
                verter_type_expr::ObjectMember::CallSignature(function) => {
                    graph_object_construction_effect::Kind::DirectCall(GraphObjectSignatureEffect {
                        signature_node_id: self.encode_member_value(
                            Some(&TypeExpr::Function(std::sync::Arc::new(function.clone()))),
                            depth + 1,
                        ),
                    })
                }
                verter_type_expr::ObjectMember::ConstructSignature(function) => {
                    graph_object_construction_effect::Kind::DirectConstruct(
                        GraphObjectSignatureEffect {
                            signature_node_id: self.encode_member_value(
                                Some(&TypeExpr::Function(std::sync::Arc::new(function.clone()))),
                                depth + 1,
                            ),
                        },
                    )
                }
                verter_type_expr::ObjectMember::Spread(spread) => {
                    graph_object_construction_effect::Kind::Spread(GraphObjectSpreadEffect {
                        operand_node_id: self.encode_member_value(Some(&spread.ty), depth + 1),
                    })
                }
            };
            effects.push(GraphObjectConstructionEffect { kind: Some(kind) });
        }
        self.push_node(graph_type_node::Kind::ObjectSpreadProgram(
            GraphObjectSpreadProgram { effects },
        ))
    }

    /// The shared payload for property / method / get / set effects. The
    /// enclosing oneof arm is the authored-kind discriminator; this maps the
    /// lossless fields every named effect carries.
    fn encode_named_effect(
        &mut self,
        key: &verter_type_expr::TypeAuthoredPropertyKey,
        value: &TypeExpr,
        optional: bool,
        readonly: bool,
        has_implementation_body: bool,
        visibility: verter_type_expr::MemberVisibility,
        excess_origin: verter_type_expr::ExcessPropertyOrigin,
        depth: u32,
    ) -> verter_protocol::verter::v1::GraphObjectNamedEffect {
        use verter_protocol::verter::v1::{
            GraphAccessibility, GraphObjectExcessOrigin, GraphObjectMergeRole,
            GraphObjectNamedEffect,
        };
        let accessibility = match visibility {
            verter_type_expr::MemberVisibility::Public => GraphAccessibility::Public,
            verter_type_expr::MemberVisibility::Protected => GraphAccessibility::Protected,
            verter_type_expr::MemberVisibility::Private => GraphAccessibility::Private,
        };
        let excess = match excess_origin {
            verter_type_expr::ExcessPropertyOrigin::NonLiteral => {
                GraphObjectExcessOrigin::NonLiteral
            }
            verter_type_expr::ExcessPropertyOrigin::FreshOwn => GraphObjectExcessOrigin::FreshOwn,
            verter_type_expr::ExcessPropertyOrigin::SpreadTainted => {
                GraphObjectExcessOrigin::SpreadTainted
            }
        };
        GraphObjectNamedEffect {
            property_key: Some(self.encode_property_key(key, depth)),
            value_node_id: self.encode_member_value(Some(value), depth + 1),
            optional,
            readonly,
            has_implementation_body,
            accessibility: accessibility as i32,
            spans: None,
            declaration_origin_name_id: 0,
            has_declaration_origin: false,
            declared_in_macro_type_arg: false,
            merge_role: GraphObjectMergeRole::Authored as i32,
            excess_origin: excess as i32,
        }
    }

    /// Typed property-key identity on the wire: string keys intern into the
    /// string table, canonical integers stay numeric, unique symbols keep
    /// their nominal declaration symbol, and unresolved computed keys carry a
    /// node reference — never a stringified spelling.
    fn encode_property_key(
        &mut self,
        key: &verter_type_expr::TypeAuthoredPropertyKey,
        depth: u32,
    ) -> verter_protocol::verter::v1::GraphPropertyKey {
        use verter_protocol::verter::v1::{graph_property_key, GraphPropertyKey};
        let key = match key {
            verter_type_expr::AuthoredPropertyKey::String(value) => {
                graph_property_key::Key::StringId(self.strings.intern(value))
            }
            verter_type_expr::AuthoredPropertyKey::Number(value) => {
                graph_property_key::Key::CanonicalNumber(value.get())
            }
            verter_type_expr::AuthoredPropertyKey::UniqueSymbol(identity) => {
                graph_property_key::Key::UniqueSymbolDeclId(self.intern_symbol(&identity.symbol))
            }
            verter_type_expr::AuthoredPropertyKey::Computed(expression) => {
                graph_property_key::Key::ComputedNodeId(
                    self.encode_member_value(Some(expression), depth + 1),
                )
            }
        };
        GraphPropertyKey { key: Some(key) }
    }
}

#[cfg(test)]
mod spread_program_wire_tests {
    use super::*;

    fn encode_value(ty: &TypeExpr) -> (GraphArena, u32) {
        let mut arena = GraphArena::new();
        let id = arena.encode_member_value(Some(ty), 0);
        (arena, id)
    }

    use std::sync::Arc;
    use verter_protocol::verter::v1::{graph_object_construction_effect, graph_property_key};
    use verter_type_expr::{
        FunctionExpr, IndexSignature, MethodSignature, ObjectExpr, ObjectMember, ObjectMethodKind,
        ObjectProperty, PrimitiveName, SpreadMember, TypeExpr,
    };

    fn string_property(name: &str) -> ObjectMember {
        ObjectMember::Property(ObjectProperty::synthetic_public_key(
            name.into(),
            TypeExpr::Primitive(PrimitiveName::String),
            false,
            false,
        ))
    }

    #[test]
    fn spread_bearing_object_encodes_as_ordered_construction_program() {
        let obj = ObjectExpr {
            properties: vec![
                string_property("a"),
                ObjectMember::Method({
                    let mut method = MethodSignature::synthetic_public_key(
                        verter_type_expr::AuthoredPropertyKey::string("b"),
                        FunctionExpr::synthetic(Vec::new(), None, Vec::new()),
                        false,
                    );
                    method.method_kind = ObjectMethodKind::Get;
                    method
                }),
                ObjectMember::IndexSignature(IndexSignature::synthetic(
                    "k".to_string(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    TypeExpr::Primitive(PrimitiveName::Number),
                    true,
                )),
                ObjectMember::Spread(SpreadMember::new(TypeExpr::Primitive(
                    PrimitiveName::Object,
                ))),
                string_property("z"),
            ],
        };
        let (arena, id) = encode_value(&TypeExpr::Object(Arc::new(obj)));
        let node = &arena.nodes[id as usize];
        let Some(graph_type_node::Kind::ObjectSpreadProgram(program)) = &node.kind else {
            panic!("a spread-bearing object must encode as tag 33, got {node:?}");
        };
        assert_eq!(
            program.effects.len(),
            5,
            "effects keep exact source order: {program:?}"
        );
        let arm = |index: usize| {
            program.effects[index]
                .kind
                .clone()
                .unwrap_or_else(|| panic!("effect {index} carries a kind"))
        };
        let graph_object_construction_effect::Kind::DirectProperty(first) = arm(0) else {
            panic!("effect 0 is the direct property, got {:?}", arm(0));
        };
        assert!(
            matches!(
                first.property_key.as_ref().and_then(|k| k.key.as_ref()),
                Some(graph_property_key::Key::StringId(_))
            ),
            "the direct property key is a string-table entry"
        );
        assert!(
            matches!(arm(1), graph_object_construction_effect::Kind::DirectGet(_)),
            "effect 1 keeps the getter kind, got {:?}",
            arm(1)
        );
        let graph_object_construction_effect::Kind::DirectIndex(index) = arm(2) else {
            panic!("effect 2 is the direct index, got {:?}", arm(2));
        };
        assert!(index.readonly, "an authored direct index retains readonly");
        assert!(
            matches!(arm(3), graph_object_construction_effect::Kind::Spread(_)),
            "effect 3 is the spread operand, got {:?}",
            arm(3)
        );
        assert!(
            matches!(
                arm(4),
                graph_object_construction_effect::Kind::DirectProperty(_)
            ),
            "effect 4 is the trailing direct property, got {:?}",
            arm(4)
        );
    }

    #[test]
    fn spread_free_structural_object_still_degrades_to_opaque() {
        let obj = ObjectExpr {
            properties: vec![string_property("a")],
        };
        let (arena, id) = encode_value(&TypeExpr::Object(Arc::new(obj)));
        let node = &arena.nodes[id as usize];
        assert!(
            matches!(node.kind, Some(graph_type_node::Kind::Opaque(_))),
            "a spread-free structural object keeps the opaque arm, got {node:?}"
        );
    }

    #[test]
    fn program_keys_encode_typed_number_symbol_and_computed_forms() {
        use verter_type_expr::{AuthoredPropertyKey, CanonicalIndexInt, ValueDeclIdentityPart};
        let symbol = || ValueDeclIdentityPart {
            canonical_id: Arc::from("/w/brand.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("tag"),
            member_path: Arc::from([]),
        };
        let property = |key: AuthoredPropertyKey<TypeExpr, ValueDeclIdentityPart>| {
            ObjectMember::Property(ObjectProperty::synthetic_public_key(
                key,
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
            ))
        };
        let obj = ObjectExpr {
            properties: vec![
                property(AuthoredPropertyKey::Number(
                    CanonicalIndexInt::from_canonical_i64(7).unwrap(),
                )),
                property(AuthoredPropertyKey::UniqueSymbol(symbol())),
                property(AuthoredPropertyKey::Computed(TypeExpr::named("K"))),
                ObjectMember::Spread(SpreadMember::new(TypeExpr::Primitive(
                    PrimitiveName::Object,
                ))),
            ],
        };
        let (arena, id) = encode_value(&TypeExpr::Object(Arc::new(obj)));
        let node = &arena.nodes[id as usize];
        let Some(graph_type_node::Kind::ObjectSpreadProgram(program)) = &node.kind else {
            panic!("typed-key program must encode as tag 33, got {node:?}");
        };
        let key_of = |index: usize| {
            let Some(graph_object_construction_effect::Kind::DirectProperty(effect)) =
                program.effects[index].kind.as_ref()
            else {
                panic!("effect {index} is a direct property");
            };
            effect
                .property_key
                .as_ref()
                .and_then(|k| k.key.clone())
                .unwrap_or_else(|| panic!("effect {index} carries a key"))
        };
        assert_eq!(
            key_of(0),
            graph_property_key::Key::CanonicalNumber(7),
            "numeric keys encode the canonical integer"
        );
        assert!(
            matches!(key_of(1), graph_property_key::Key::UniqueSymbolDeclId(_)),
            "unique symbols encode a nominal symbol id, got {:?}",
            key_of(1)
        );
        assert!(
            matches!(key_of(2), graph_property_key::Key::ComputedNodeId(_)),
            "computed keys encode a node reference, never a string, got {:?}",
            key_of(2)
        );
    }
}
