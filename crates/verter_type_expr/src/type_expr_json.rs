//! JSON (de)serialisation for [`TypeExpr`].
//!
//! `TypeExpr` is a recursively-`Arc`-linked tree; a serde derive would hit
//! the recursion limit, so both directions are hand-rolled. The
//! [`Serialize`] impl streams each node straight into the serializer with
//! no intermediate `serde_json::Value`, so encoding costs work proportional
//! to the output at any nesting depth; [`TypeExpr::to_json_value`] is that
//! same encoding collected into a `Value`. The [`Deserialize`] impl reads a
//! `Value` and converts it through [`type_expr_from_json`].
//!
//! The wire form is the one a `serde_json::Value` of the node would write:
//! every object's keys in sorted order (the workspace builds `serde_json`
//! without `preserve_order`, so a `Value` map is a `BTreeMap`), an absent
//! optional child written as `null`, a non-finite number as `null`, and a
//! non-negative integer as unsigned. The encoder below writes its keys in
//! that order by hand, making the same serializer calls a `Value` would
//! (maps, `serialize_unit` for `null`, `serialize_u64` for a non-negative
//! integer), so non-JSON serializers see the same shape too; the
//! `type_expr_wire_golden` cases pin the bytes and the calls.
//!
//! Encoding recurses once per nested node, so it grows its stack on demand
//! (`stacker`): a deeply nested type encodes on any thread.
//!
//! These impls live here so JSON transport remains separate from the core type
//! definitions in `lib.rs`. The orphan rule permits `impl Serialize`/`impl
//! Deserialize for TypeExpr` in any module of this crate because `TypeExpr` is
//! crate-local; the helpers reach every node type through `use crate::*`.

use crate::*;
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use std::sync::Arc;

/// Stack kept free before encoding one more node; one level of the encoder
/// uses a few KiB in an unoptimised build.
const ENCODE_RED_ZONE: usize = 64 * 1024;
/// Size of each stack segment added when the red zone is reached.
const ENCODE_STACK_SEGMENT: usize = 1024 * 1024;

impl Serialize for TypeExpr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        stacker::maybe_grow(ENCODE_RED_ZONE, ENCODE_STACK_SEGMENT, || {
            encode_type_expr(self, serializer)
        })
    }
}

impl<'de> serde::Deserialize<'de> for TypeExpr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Deserialize via Value, then convert back
        let value = serde_json::Value::deserialize(deserializer)?;
        type_expr_from_json(&value).ok_or_else(|| serde::de::Error::custom("invalid TypeExpr"))
    }
}

/// Reconstruct a TypeExpr from a JSON Value.
pub fn type_expr_from_json(v: &serde_json::Value) -> Option<TypeExpr> {
    let kind = v.get("kind")?.as_str()?;
    match kind {
        "primitive" => {
            let name = v.get("name")?.as_str()?;
            Some(TypeExpr::Primitive(PrimitiveName::parse(name)?))
        }
        "literal" => {
            let lit_kind = v.get("literalKind")?.as_str()?;
            match lit_kind {
                "string" => Some(TypeExpr::string_literal(v.get("value")?.as_str()?)),
                "number" => Some(TypeExpr::number_literal(v.get("value")?.as_f64()?)),
                "boolean" => Some(TypeExpr::boolean_literal(v.get("value")?.as_bool()?)),
                "bigInt" => Some(TypeExpr::Literal(LiteralValue::BigInt(
                    v.get("value")?.as_str()?.to_string(),
                ))),
                _ => None,
            }
        }
        "union" => {
            let types = json_array_to_type_exprs(v.get("types")?)?;
            Some(TypeExpr::Union(Arc::from(types)))
        }
        "intersection" => {
            let types = json_array_to_type_exprs(v.get("types")?)?;
            Some(TypeExpr::Intersection(Arc::from(types)))
        }
        "array" => {
            let element = type_expr_from_json(v.get("element")?)?;
            let readonly = v.get("readonly").and_then(|r| r.as_bool()).unwrap_or(false);
            Some(TypeExpr::Array {
                element: Arc::new(element),
                readonly,
            })
        }
        "object" => {
            let props = v.get("properties")?.as_array()?;
            let members = props.iter().filter_map(json_to_object_member).collect();
            Some(TypeExpr::Object(Arc::new(ObjectExpr {
                empty_literal: v
                    .get("emptyLiteral")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                properties: members,
            })))
        }
        "function" => {
            let params = json_to_func_params(v.get("parameters")?)?;
            let ret = v.get("returnType").and_then(|r| {
                if r.is_null() {
                    None
                } else {
                    type_expr_from_json(r)
                }
            });
            Some(TypeExpr::Function(Arc::new(
                FunctionExpr::synthetic(
                    params,
                    ret.map(Arc::new),
                    json_to_type_params(v.get("typeParameters"))?,
                )
                .with_predicate(json_to_type_predicate(v.get("predicate"))?),
            )))
        }
        "constructorType" => {
            let params = json_to_func_params(v.get("parameters")?)?;
            let ret = v.get("returnType").and_then(|r| {
                if r.is_null() {
                    None
                } else {
                    type_expr_from_json(r)
                }
            });
            Some(TypeExpr::ConstructorType(Arc::new(
                FunctionExpr::synthetic(
                    params,
                    ret.map(Arc::new),
                    json_to_type_params(v.get("typeParameters"))?,
                )
                .with_predicate(json_to_type_predicate(v.get("predicate"))?)
                .with_abstract(
                    v.get("abstract").and_then(serde_json::Value::as_bool) == Some(true),
                ),
            )))
        }
        "intrinsicApplication" => {
            // An unrecognised op is a HARD decode failure: guessing would turn
            // a newer producer's operation into the wrong one.
            let op = crate::CompilerIntrinsicTypeOp::from_wire_str(v.get("op")?.as_str()?)?;
            let arguments = v
                .get("arguments")
                .and_then(json_array_to_type_exprs)
                .unwrap_or_default();
            Some(TypeExpr::IntrinsicApplication {
                op,
                arguments: Arc::from(arguments),
            })
        }
        "ref" => {
            let name = v.get("name")?.as_str()?.to_string();
            let args = v
                .get("typeArguments")
                .and_then(json_array_to_type_exprs)
                .unwrap_or_default();
            Some(TypeExpr::Ref {
                name: Arc::from(name),
                type_arguments: Arc::from(args),
            })
        }
        "importType" => {
            let specifier = v.get("specifier")?.as_str()?;
            let qualifier: Vec<Arc<str>> = v
                .get("qualifier")
                .and_then(|q| q.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(Arc::from))
                        .collect()
                })
                .unwrap_or_default();
            let typeof_query = v
                .get("typeofQuery")
                .and_then(|b| b.as_bool())
                .unwrap_or(false);
            let type_arguments = v
                .get("typeArguments")
                .and_then(json_array_to_type_exprs)
                .unwrap_or_default();
            Some(TypeExpr::ImportType {
                specifier: Arc::from(specifier),
                qualifier: Arc::from(qualifier),
                typeof_query,
                type_arguments: Arc::from(type_arguments),
            })
        }
        "typeParameter" => Some(TypeExpr::TypeParameter(json_to_type_param(v)?)),
        "keyOf" => {
            let operand = type_expr_from_json(v.get("operand")?)?;
            Some(TypeExpr::KeyOf(Arc::new(operand)))
        }
        "typeOf" => {
            let path = v
                .get("path")?
                .as_array()?
                .iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect();
            let type_args = v
                .get("typeArgs")
                .and_then(json_array_to_type_exprs)
                .unwrap_or_default();
            Some(TypeExpr::TypeOf(ValueRef { path, type_args }))
        }
        "indexedAccess" => {
            let obj = type_expr_from_json(v.get("object")?)?;
            let idx = type_expr_from_json(v.get("index")?)?;
            Some(TypeExpr::IndexedAccess {
                object: Arc::new(obj),
                index: Arc::new(idx),
            })
        }
        "conditional" => Some(TypeExpr::Conditional {
            check: Arc::new(type_expr_from_json(v.get("check")?)?),
            extends: Arc::new(type_expr_from_json(v.get("extends")?)?),
            true_type: Arc::new(type_expr_from_json(v.get("trueType")?)?),
            false_type: Arc::new(type_expr_from_json(v.get("falseType")?)?),
        }),
        "tuple" => {
            let elements: Vec<TupleElement> = v
                .get("elements")?
                .as_array()?
                .iter()
                .filter_map(|e| {
                    Some(TupleElement {
                        label: e.get("label").and_then(|l| l.as_str().map(String::from)),
                        ty: type_expr_from_json(e.get("ty")?)?,
                        optional: e.get("optional").and_then(|o| o.as_bool()).unwrap_or(false),
                        rest: e.get("rest").and_then(|o| o.as_bool()).unwrap_or(false),
                    })
                })
                .collect();
            let readonly = v.get("readonly").and_then(|r| r.as_bool()).unwrap_or(false);
            Some(TypeExpr::Tuple {
                elements: Arc::from(elements),
                readonly,
            })
        }
        "mapped" => Some(TypeExpr::Mapped {
            parameter: v.get("parameter")?.as_str()?.to_string(),
            source: Arc::new(type_expr_from_json(v.get("source")?)?),
            value: Arc::new(type_expr_from_json(v.get("value")?)?),
            optional: parse_modifier(v.get("optional")),
            readonly: parse_modifier(v.get("readonly")),
            name_type: v
                .get("nameType")
                .and_then(|n| {
                    if n.is_null() {
                        None
                    } else {
                        type_expr_from_json(n)
                    }
                })
                .map(Arc::new),
        }),
        "templateLiteral" => {
            let quasis = v
                .get("quasis")?
                .as_array()?
                .iter()
                .filter_map(|q| q.as_str().map(String::from))
                .collect();
            let expressions = json_array_to_type_exprs(v.get("expressions")?)?;
            Some(TypeExpr::TemplateLiteral {
                quasis,
                expressions: Arc::from(expressions),
            })
        }
        "infer" => Some(TypeExpr::Infer {
            name: v.get("name")?.as_str()?.to_string(),
            constraint: match v.get("constraint") {
                Some(constraint) => Some(Arc::new(type_expr_from_json(constraint)?)),
                None => None,
            },
        }),
        "rest" => Some(TypeExpr::Rest(Arc::new(type_expr_from_json(
            v.get("inner")?,
        )?))),
        "parenthesized" => Some(TypeExpr::Parenthesized(Arc::new(type_expr_from_json(
            v.get("inner")?,
        )?))),
        "recursiveRef" => {
            let name = v.get("name")?.as_str()?.to_string();
            let args = v
                .get("typeArguments")
                .and_then(json_array_to_type_exprs)
                .unwrap_or_default();
            let ctx = v
                .get("conditionalContext")
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|f| {
                            let branch = match f.get("branch")?.as_str()? {
                                "true" => RecursiveConditionalBranch::True,
                                "false" => RecursiveConditionalBranch::False,
                                _ => return None,
                            };
                            Some(RecursiveConditionalFrame {
                                branch,
                                decided: f.get("decided")?.as_bool()?,
                                check: Arc::new(type_expr_from_json(f.get("check")?)?),
                                extends: Arc::new(type_expr_from_json(f.get("extends")?)?),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some(TypeExpr::RecursiveRef {
                name: Arc::from(name),
                type_arguments: Arc::from(args),
                conditional_context: Arc::from(ctx),
            })
        }
        "syntheticSlotBinding" => {
            let scope_canonical_id = v.get("scopeCanonicalId")?.as_str()?;
            let surface_kind = match v.get("surfaceKind")?.as_str()? {
                "slotBinding" => SyntheticCarrierSurfaceKind::SlotBinding,
                "binding" => SyntheticCarrierSurfaceKind::Binding,
                _ => return None,
            };
            let slot_name = v.get("slotName").and_then(|s| {
                if s.is_null() {
                    None
                } else {
                    s.as_str().map(OwnedStr::new)
                }
            });
            let binding_name = v.get("bindingName")?.as_str()?;
            // valueNode is serialised as a decimal STRING to avoid JS
            // Number precision loss; decode it back to u64 here.
            let value_node = v.get("valueNode")?.as_str()?.parse::<u64>().ok()?;
            Some(TypeExpr::SyntheticSlotBinding(Arc::new(
                SyntheticCarrierKey {
                    scope_canonical_id: OwnedStr::new(scope_canonical_id),
                    surface_kind,
                    slot_name,
                    binding_name: OwnedStr::new(binding_name),
                    value_node,
                },
            )))
        }
        "unknown" => {
            let raw = v.get("raw")?.as_str()?;
            Some(TypeExpr::Unknown(UnknownValue::wire_opaque(raw)))
        }
        _ => Some(TypeExpr::Unknown(UnknownValue::wire_opaque(kind))),
    }
}

fn json_array_to_type_exprs(v: &serde_json::Value) -> Option<Vec<TypeExpr>> {
    v.as_array()?
        .iter()
        .map(type_expr_from_json)
        .collect::<Option<Vec<_>>>()
}

/// Parse the optional `"visibility"` wire string (`"protected"` / `"private"`)
/// produced by the manual serializer. A MISSING field — every pre-existing
/// all-public surface, and any external producer that never emitted it —
/// deserializes as [`MemberVisibility::Public`]. An unrecognized value also
/// falls back to `Public` (forward-compat with an additive wire string).
fn member_visibility_from_json(v: &serde_json::Value) -> MemberVisibility {
    match v.get("visibility").and_then(|x| x.as_str()) {
        Some("protected") => MemberVisibility::Protected,
        Some("private") => MemberVisibility::Private,
        _ => MemberVisibility::Public,
    }
}

/// The wire string for a member's excess-property origin (serialized only for
/// non-`NonLiteral` values; absence parses back as `NonLiteral`).
fn excess_origin_wire_str(origin: ExcessPropertyOrigin) -> &'static str {
    match origin {
        ExcessPropertyOrigin::FreshOwn => "freshOwn",
        ExcessPropertyOrigin::SpreadTainted => "spreadTainted",
        ExcessPropertyOrigin::NonLiteral => "nonLiteral",
    }
}

fn excess_origin_from_json(v: &serde_json::Value) -> ExcessPropertyOrigin {
    match v.get("excessOrigin").and_then(|o| o.as_str()) {
        Some("freshOwn") => ExcessPropertyOrigin::FreshOwn,
        Some("spreadTainted") => ExcessPropertyOrigin::SpreadTainted,
        _ => ExcessPropertyOrigin::NonLiteral,
    }
}

fn authored_property_key_from_json(v: &serde_json::Value) -> Option<TypeAuthoredPropertyKey> {
    match v.get("kind")?.as_str()? {
        "string" => Some(AuthoredPropertyKey::String(OwnedStr::from(
            v.get("value")?.as_str()?,
        ))),
        "number" => Some(AuthoredPropertyKey::Number(
            CanonicalIndexInt::from_canonical_i64(v.get("value")?.as_i64()?)?,
        )),
        "uniqueSymbol" => Some(AuthoredPropertyKey::UniqueSymbol(
            serde_json::from_value(v.get("identity")?.clone()).ok()?,
        )),
        "computed" => Some(AuthoredPropertyKey::Computed(type_expr_from_json(
            v.get("expression")?,
        )?)),
        _ => None,
    }
}

fn json_to_object_member(v: &serde_json::Value) -> Option<ObjectMember> {
    let mk = v.get("memberKind")?.as_str()?;
    match mk {
        "property" => Some(ObjectMember::Property(
            ObjectProperty::with_key_visibility(
                authored_property_key_from_json(v.get("key")?)?,
                type_expr_from_json(v.get("ty")?)?,
                v.get("optional").and_then(|o| o.as_bool()).unwrap_or(false),
                v.get("readonly").and_then(|o| o.as_bool()).unwrap_or(false),
                member_visibility_from_json(v),
                MemberSpans::default(),
            )
            .with_excess_origin(excess_origin_from_json(v)),
        )),
        "indexSignature" => Some(ObjectMember::IndexSignature(IndexSignature::synthetic(
            v.get("keyName")?.as_str()?.to_string(),
            type_expr_from_json(v.get("keyType")?)?,
            type_expr_from_json(v.get("valueType")?)?,
            v.get("readonly").and_then(|o| o.as_bool()).unwrap_or(false),
        ))),
        "callSignature" => Some(ObjectMember::CallSignature(json_to_function_expr(
            v.get("function")?,
        )?)),
        "constructSignature" => Some(ObjectMember::ConstructSignature(json_to_function_expr(
            v.get("function")?,
        )?)),
        "method" => Some(ObjectMember::Method(
            MethodSignature::with_key_visibility(
                authored_property_key_from_json(v.get("key")?)?,
                json_to_function_expr(v.get("function")?)?,
                v.get("optional").and_then(|o| o.as_bool()).unwrap_or(false),
                member_visibility_from_json(v),
                MemberSpans::default(),
            )
            .with_excess_origin(excess_origin_from_json(v)),
        )),
        "spread" => Some(ObjectMember::Spread(SpreadMember::new(
            type_expr_from_json(v.get("ty")?)?,
        ))),
        _ => None,
    }
}

fn json_to_func_params(v: &serde_json::Value) -> Option<Vec<FunctionParam>> {
    Some(
        v.as_array()?
            .iter()
            .filter_map(|p| {
                Some(FunctionParam::synthetic(
                    p.get("name").and_then(|n| n.as_str().map(String::from)),
                    type_expr_from_json(p.get("ty")?)?,
                    p.get("optional").and_then(|o| o.as_bool()).unwrap_or(false),
                    p.get("rest").and_then(|o| o.as_bool()).unwrap_or(false),
                ))
            })
            .collect(),
    )
}

fn json_to_function_expr(v: &serde_json::Value) -> Option<FunctionExpr> {
    Some(
        FunctionExpr::synthetic(
            json_to_func_params(v.get("parameters")?)?,
            v.get("returnType")
                .and_then(|ret| {
                    if ret.is_null() {
                        None
                    } else {
                        type_expr_from_json(ret)
                    }
                })
                .map(Arc::new),
            json_to_type_params(v.get("typeParameters"))?,
        )
        .with_predicate(json_to_type_predicate(v.get("predicate"))?),
    )
}

/// Decode a function's optional `predicate` object: `Some(None)` when
/// absent, `None` (a hard decode failure) when present but malformed.
fn json_to_type_predicate(v: Option<&serde_json::Value>) -> Option<Option<Arc<TypePredicate>>> {
    let Some(value) = v else {
        return Some(None);
    };
    let subject = match (value.get("parameter"), value.get("this")) {
        (Some(name), None) => TypePredicateSubject::Parameter(Arc::from(name.as_str()?)),
        (None, Some(this)) if this.as_bool()? => TypePredicateSubject::This,
        _ => return None,
    };
    let ty = match value.get("ty") {
        Some(ty) => Some(Arc::new(type_expr_from_json(ty)?)),
        None => None,
    };
    Some(Some(Arc::new(TypePredicate {
        subject,
        asserts: value.get("asserts")?.as_bool()?,
        ty,
    })))
}

fn json_to_type_params(v: Option<&serde_json::Value>) -> Option<Vec<TypeParam>> {
    let Some(value) = v else {
        return Some(Vec::new());
    };
    value
        .as_array()?
        .iter()
        .map(json_to_type_param)
        .collect::<Option<Vec<_>>>()
}

fn json_to_type_param(v: &serde_json::Value) -> Option<TypeParam> {
    Some(TypeParam {
        name: v.get("name")?.as_str()?.to_string(),
        constraint: v
            .get("constraint")
            .and_then(|constraint| {
                if constraint.is_null() {
                    None
                } else {
                    type_expr_from_json(constraint)
                }
            })
            .map(Arc::new),
        default: v
            .get("default")
            .and_then(|default| {
                if default.is_null() {
                    None
                } else {
                    type_expr_from_json(default)
                }
            })
            .map(Arc::new),
        is_const: v
            .get("isConst")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    })
}

fn parse_modifier(v: Option<&serde_json::Value>) -> MappedModifier {
    match v.and_then(|v| v.as_str()) {
        Some("add") => MappedModifier::Add,
        Some("remove") => MappedModifier::Remove,
        _ => MappedModifier::None,
    }
}

fn modifier_str(m: MappedModifier) -> &'static str {
    match m {
        MappedModifier::None => "none",
        MappedModifier::Add => "add",
        MappedModifier::Remove => "remove",
    }
}

impl TypeExpr {
    /// This node's JSON as a [`serde_json::Value`].
    ///
    /// The streaming [`Serialize`] encoding collected into a tree: each
    /// output node is built once and moved into its parent, so the cost is
    /// proportional to the output. Callers that only write the JSON out
    /// should serialize the node directly and skip the tree.
    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self)
            .expect("a TypeExpr encodes into a JSON value: every map key it writes is a string")
    }
}

/// Write `$key => $value` pairs as one JSON object. The keys must be given
/// in sorted order, the order a `serde_json::Value` map writes them.
macro_rules! object {
    ($serializer:expr; $($key:literal => $value:expr),+ $(,)?) => {{
        let mut map = $serializer.serialize_map(Some([$($key),+].len()))?;
        $(map.serialize_entry($key, $value)?;)+
        map.end()
    }};
}

/// `Some` as its value and `None` as `null`, written the way
/// `serde_json::Value::Null` writes it.
struct OrNull<T>(Option<T>);

impl<T: Serialize> Serialize for OrNull<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            Some(value) => value.serialize(serializer),
            None => serializer.serialize_unit(),
        }
    }
}

/// A float written the way a `serde_json::Value` holds it: a non-finite
/// value has no JSON number and becomes `null`.
struct Float(f64);

impl Serialize for Float {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.is_finite() {
            serializer.serialize_f64(self.0)
        } else {
            serializer.serialize_unit()
        }
    }
}

/// An integer written the way a `serde_json::Value` holds it: unsigned when
/// non-negative.
struct Int(i64);

impl Serialize for Int {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match u64::try_from(self.0) {
            Ok(unsigned) => serializer.serialize_u64(unsigned),
            Err(_) => serializer.serialize_i64(self.0),
        }
    }
}

/// An integer written as its decimal string, without building the string.
struct Decimal(u64);

impl Serialize for Decimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// A JSON array of the slice's items, each written through the encoder
/// beside it.
struct Many<'a, T, W>(&'a [T], fn(&'a T) -> W);

impl<'a, T, W: Serialize> Serialize for Many<'a, T, W> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for item in self.0 {
            seq.serialize_element(&(self.1)(item))?;
        }
        seq.end()
    }
}

fn encode_type_expr<S: Serializer>(expr: &TypeExpr, serializer: S) -> Result<S::Ok, S::Error> {
    match expr {
        TypeExpr::Primitive(name) => object!(serializer;
            "kind" => "primitive",
            "name" => name.as_str(),
        ),
        TypeExpr::Literal(LiteralValue::String(value)) => object!(serializer;
            "kind" => "literal",
            "literalKind" => "string",
            "value" => value.as_str(),
        ),
        TypeExpr::Literal(LiteralValue::Number(value)) => object!(serializer;
            "kind" => "literal",
            "literalKind" => "number",
            "value" => &Float(*value),
        ),
        TypeExpr::Literal(LiteralValue::Boolean(value)) => object!(serializer;
            "kind" => "literal",
            "literalKind" => "boolean",
            "value" => value,
        ),
        TypeExpr::Literal(LiteralValue::BigInt(value)) => object!(serializer;
            "kind" => "literal",
            "literalKind" => "bigInt",
            "value" => value.as_str(),
        ),
        TypeExpr::Union(types) => object!(serializer;
            "kind" => "union",
            "types" => &types[..],
        ),
        TypeExpr::Intersection(types) => object!(serializer;
            "kind" => "intersection",
            "types" => &types[..],
        ),
        TypeExpr::Array { element, readonly } => object!(serializer;
            "element" => &**element,
            "kind" => "array",
            "readonly" => readonly,
        ),
        TypeExpr::Tuple { elements, readonly } => object!(serializer;
            "elements" => &Many(&elements[..], TupleElementJson),
            "kind" => "tuple",
            "readonly" => readonly,
        ),
        TypeExpr::Object(obj) => {
            let mut map = serializer.serialize_map(Some(2 + usize::from(obj.empty_literal)))?;
            // `emptyLiteral` is written only when set, so a JSON object without
            // it parses back as an ordinary object.
            if obj.empty_literal {
                map.serialize_entry("emptyLiteral", &true)?;
            }
            map.serialize_entry("kind", "object")?;
            map.serialize_entry("properties", &Many(&obj.properties[..], ObjectMemberJson))?;
            map.end()
        }
        TypeExpr::Function(func) => FunctionJson {
            func,
            kind: Some("function"),
        }
        .serialize(serializer),
        TypeExpr::ConstructorType(func) => FunctionJson {
            func,
            kind: Some("constructorType"),
        }
        .serialize(serializer),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => object!(serializer;
            "kind" => "ref",
            "name" => &**name,
            "typeArguments" => &type_arguments[..],
        ),
        // The wire form carries the OP IDENTITY as a closed token, never a
        // free-form name, so no decoder can mistake it for a declaration
        // reference — and an unknown op fails the decode outright.
        TypeExpr::IntrinsicApplication { op, arguments } => object!(serializer;
            "arguments" => &arguments[..],
            "kind" => "intrinsicApplication",
            "op" => op.wire_str(),
        ),
        TypeExpr::ImportType {
            specifier,
            qualifier,
            typeof_query,
            type_arguments,
        } => object!(serializer;
            "kind" => "importType",
            "qualifier" => &qualifier[..],
            "specifier" => &**specifier,
            "typeArguments" => &type_arguments[..],
            "typeofQuery" => typeof_query,
        ),
        TypeExpr::TypeParameter(param) => TypeParamJson {
            param,
            kind: Some("typeParameter"),
        }
        .serialize(serializer),
        TypeExpr::KeyOf(operand) => object!(serializer;
            "kind" => "keyOf",
            "operand" => &**operand,
        ),
        TypeExpr::TypeOf(value_ref) if value_ref.type_args.is_empty() => object!(serializer;
            "kind" => "typeOf",
            "path" => &value_ref.path,
        ),
        TypeExpr::TypeOf(value_ref) => object!(serializer;
            "kind" => "typeOf",
            "path" => &value_ref.path,
            "typeArgs" => &value_ref.type_args,
        ),
        TypeExpr::IndexedAccess { object, index } => object!(serializer;
            "index" => &**index,
            "kind" => "indexedAccess",
            "object" => &**object,
        ),
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => object!(serializer;
            "check" => &**check,
            "extends" => &**extends,
            "falseType" => &**false_type,
            "kind" => "conditional",
            "trueType" => &**true_type,
        ),
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => object!(serializer;
            "kind" => "mapped",
            "nameType" => &OrNull(name_type.as_deref()),
            "optional" => modifier_str(*optional),
            "parameter" => parameter.as_str(),
            "readonly" => modifier_str(*readonly),
            "source" => &**source,
            "value" => &**value,
        ),
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => object!(serializer;
            "expressions" => &expressions[..],
            "kind" => "templateLiteral",
            "quasis" => quasis,
        ),
        TypeExpr::Infer {
            name,
            constraint: Some(constraint),
        } => object!(serializer;
            "constraint" => &**constraint,
            "kind" => "infer",
            "name" => name.as_str(),
        ),
        TypeExpr::Infer {
            name,
            constraint: None,
        } => object!(serializer;
            "kind" => "infer",
            "name" => name.as_str(),
        ),
        TypeExpr::Rest(inner) => object!(serializer;
            "inner" => &**inner,
            "kind" => "rest",
        ),
        TypeExpr::Parenthesized(inner) => object!(serializer;
            "inner" => &**inner,
            "kind" => "parenthesized",
        ),
        TypeExpr::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        } => object!(serializer;
            "conditionalContext" => &Many(&conditional_context[..], RecursiveFrameJson),
            "kind" => "recursiveRef",
            "name" => &**name,
            "typeArguments" => &type_arguments[..],
        ),
        TypeExpr::SyntheticSlotBinding(key) => object!(serializer;
            "bindingName" => &*key.binding_name,
            "kind" => "syntheticSlotBinding",
            "scopeCanonicalId" => &*key.scope_canonical_id,
            "slotName" => &OrNull(key.slot_name.as_deref()),
            "surfaceKind" => match key.surface_kind {
                SyntheticCarrierSurfaceKind::SlotBinding => "slotBinding",
                SyntheticCarrierSurfaceKind::Binding => "binding",
            },
            "valueNode" => &Decimal(key.value_node),
        ),
        TypeExpr::Unknown(value) => object!(serializer;
            "kind" => "unknown",
            "raw" => value.raw(),
        ),
    }
}

struct TupleElementJson<'a>(&'a TupleElement);

impl Serialize for TupleElementJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let element = self.0;
        object!(serializer;
            "label" => &OrNull(element.label.as_deref()),
            "optional" => &element.optional,
            "rest" => &element.rest,
            "ty" => &element.ty,
        )
    }
}

struct PropertyKeyJson<'a>(&'a TypeAuthoredPropertyKey);

impl Serialize for PropertyKeyJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            AuthoredPropertyKey::String(value) => object!(serializer;
                "kind" => "string",
                "value" => &**value,
            ),
            AuthoredPropertyKey::Number(value) => object!(serializer;
                "kind" => "number",
                "value" => &Int(value.get()),
            ),
            AuthoredPropertyKey::UniqueSymbol(identity) => object!(serializer;
                "identity" => &ValueIdentityJson(identity),
                "kind" => "uniqueSymbol",
            ),
            AuthoredPropertyKey::Computed(expression) => object!(serializer;
                "expression" => expression,
                "kind" => "computed",
            ),
        }
    }
}

/// A unique-symbol key's declaration identity, written as the derived
/// `Serialize` of [`ValueDeclIdentityPart`] lands in a `Value`: its field
/// names in sorted order, the owner's unit-variant enums as their variant
/// names, and the owner's `scope` only when it is not lexical.
struct ValueIdentityJson<'a>(&'a ValueDeclIdentityPart);

impl Serialize for ValueIdentityJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let identity = self.0;
        object!(serializer;
            "canonical_id" => identity.canonical_id.as_str(),
            "member_path" => &identity.member_path[..],
            "owner" => &OwnerJson(identity.owner),
            "symbol" => identity.symbol.as_str(),
        )
    }
}

struct OwnerJson(TopLevelOwnerId);

impl Serialize for OwnerJson {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let owner = self.0;
        let scope = match owner.scope() {
            OwnerDeclarationScope::Lexical => None,
            OwnerDeclarationScope::GlobalAugmentation => Some("GlobalAugmentation"),
        };
        let mut map = serializer.serialize_map(Some(2 + usize::from(scope.is_some())))?;
        map.serialize_entry(
            "kind",
            match owner.kind() {
                TopLevelOwnerKind::Module => "Module",
                TopLevelOwnerKind::Instance => "Instance",
                TopLevelOwnerKind::Frontmatter => "Frontmatter",
            },
        )?;
        map.serialize_entry("ordinal", &u64::from(owner.ordinal()))?;
        if let Some(scope) = scope {
            map.serialize_entry("scope", scope)?;
        }
        map.end()
    }
}

/// The markers a property or method writes only when they differ from the
/// default a missing field parses back as: `excessOrigin` unless
/// `NonLiteral`, and `visibility` unless public (the
/// marker-only-for-non-public hash scheme), so an all-public, non-literal
/// surface carries neither key.
struct MemberMarkers {
    excess_origin: Option<&'static str>,
    visibility: Option<&'static str>,
}

impl MemberMarkers {
    fn of(visibility: MemberVisibility, excess_origin: ExcessPropertyOrigin) -> Self {
        Self {
            excess_origin: (excess_origin != ExcessPropertyOrigin::NonLiteral)
                .then(|| excess_origin_wire_str(excess_origin)),
            visibility: (!visibility.is_public()).then(|| visibility.as_wire_str()),
        }
    }

    fn len(&self) -> usize {
        usize::from(self.excess_origin.is_some()) + usize::from(self.visibility.is_some())
    }

    /// `excessOrigin`, which sorts before every other member key.
    fn write_leading<M: SerializeMap>(&self, map: &mut M) -> Result<(), M::Error> {
        match self.excess_origin {
            Some(excess_origin) => map.serialize_entry("excessOrigin", excess_origin),
            None => Ok(()),
        }
    }

    /// `visibility`, which sorts after every other member key.
    fn write_trailing<M: SerializeMap>(&self, map: &mut M) -> Result<(), M::Error> {
        match self.visibility {
            Some(visibility) => map.serialize_entry("visibility", visibility),
            None => Ok(()),
        }
    }
}

struct ObjectMemberJson<'a>(&'a ObjectMember);

impl Serialize for ObjectMemberJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            ObjectMember::Property(p) => {
                let markers = MemberMarkers::of(p.visibility, p.excess_origin);
                let mut map = serializer.serialize_map(Some(5 + markers.len()))?;
                markers.write_leading(&mut map)?;
                map.serialize_entry("key", &PropertyKeyJson(&p.key))?;
                map.serialize_entry("memberKind", "property")?;
                map.serialize_entry("optional", &p.optional)?;
                map.serialize_entry("readonly", &p.readonly)?;
                map.serialize_entry("ty", &p.ty)?;
                markers.write_trailing(&mut map)?;
                map.end()
            }
            ObjectMember::IndexSignature(idx) => object!(serializer;
                "keyName" => idx.key_name.as_str(),
                "keyType" => &idx.key_type,
                "memberKind" => "indexSignature",
                "readonly" => &idx.readonly,
                "valueType" => &idx.value_type,
            ),
            ObjectMember::CallSignature(func) => object!(serializer;
                "function" => &FunctionJson { func, kind: None },
                "memberKind" => "callSignature",
            ),
            ObjectMember::ConstructSignature(func) => object!(serializer;
                "function" => &FunctionJson { func, kind: None },
                "memberKind" => "constructSignature",
            ),
            ObjectMember::Method(m) => {
                let markers = MemberMarkers::of(m.visibility, m.excess_origin);
                let mut map = serializer.serialize_map(Some(4 + markers.len()))?;
                markers.write_leading(&mut map)?;
                map.serialize_entry(
                    "function",
                    &FunctionJson {
                        func: &m.function,
                        kind: None,
                    },
                )?;
                map.serialize_entry("key", &PropertyKeyJson(&m.key))?;
                map.serialize_entry("memberKind", "method")?;
                map.serialize_entry("optional", &m.optional)?;
                markers.write_trailing(&mut map)?;
                map.end()
            }
            ObjectMember::Spread(spread) => object!(serializer;
                "memberKind" => "spread",
                "ty" => &spread.ty,
            ),
        }
    }
}

/// A function's parameters, return and optional parts; `kind` is present
/// when the function is itself a `TypeExpr` node rather than a member's
/// signature.
struct FunctionJson<'a> {
    func: &'a FunctionExpr,
    kind: Option<&'static str>,
}

impl Serialize for FunctionJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let func = self.func;
        let predicate = func.predicate.as_deref();
        let has_type_parameters = !func.type_parameters.is_empty();
        let len = 2
            + usize::from(func.is_abstract)
            + usize::from(self.kind.is_some())
            + usize::from(predicate.is_some())
            + usize::from(has_type_parameters);
        let mut map = serializer.serialize_map(Some(len))?;
        if func.is_abstract {
            map.serialize_entry("abstract", &true)?;
        }
        if let Some(kind) = self.kind {
            map.serialize_entry("kind", kind)?;
        }
        map.serialize_entry("parameters", &Many(&func.parameters[..], FunctionParamJson))?;
        if let Some(predicate) = predicate {
            map.serialize_entry("predicate", &TypePredicateJson(predicate))?;
        }
        map.serialize_entry("returnType", &OrNull(func.return_type.as_deref()))?;
        if has_type_parameters {
            map.serialize_entry(
                "typeParameters",
                &Many(&func.type_parameters[..], |param| TypeParamJson {
                    param,
                    kind: None,
                }),
            )?;
        }
        map.end()
    }
}

struct FunctionParamJson<'a>(&'a FunctionParam);

impl Serialize for FunctionParamJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let param = self.0;
        object!(serializer;
            "name" => &OrNull(param.name.as_deref()),
            "optional" => &param.optional,
            "rest" => &param.rest,
            "ty" => &param.ty,
        )
    }
}

struct TypePredicateJson<'a>(&'a TypePredicate);

impl Serialize for TypePredicateJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let predicate = self.0;
        let ty = predicate.ty.as_deref();
        let mut map = serializer.serialize_map(Some(2 + usize::from(ty.is_some())))?;
        map.serialize_entry("asserts", &predicate.asserts)?;
        match &predicate.subject {
            TypePredicateSubject::Parameter(name) => map.serialize_entry("parameter", &**name)?,
            TypePredicateSubject::This => map.serialize_entry("this", &true)?,
        }
        if let Some(ty) = ty {
            map.serialize_entry("ty", ty)?;
        }
        map.end()
    }
}

/// A type parameter's name and optional parts; `kind` is present when the
/// parameter is itself a `TypeExpr` node rather than a declaration in a
/// function's list.
struct TypeParamJson<'a> {
    param: &'a TypeParam,
    kind: Option<&'static str>,
}

impl Serialize for TypeParamJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let param = self.param;
        let constraint = param.constraint.as_deref();
        let default = param.default.as_deref();
        let len = 1
            + usize::from(constraint.is_some())
            + usize::from(default.is_some())
            + usize::from(param.is_const)
            + usize::from(self.kind.is_some());
        let mut map = serializer.serialize_map(Some(len))?;
        if let Some(constraint) = constraint {
            map.serialize_entry("constraint", constraint)?;
        }
        if let Some(default) = default {
            map.serialize_entry("default", default)?;
        }
        if param.is_const {
            map.serialize_entry("isConst", &true)?;
        }
        if let Some(kind) = self.kind {
            map.serialize_entry("kind", kind)?;
        }
        map.serialize_entry("name", param.name.as_str())?;
        map.end()
    }
}

struct RecursiveFrameJson<'a>(&'a RecursiveConditionalFrame);

impl Serialize for RecursiveFrameJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let frame = self.0;
        object!(serializer;
            "branch" => match frame.branch {
                RecursiveConditionalBranch::True => "true",
                RecursiveConditionalBranch::False => "false",
            },
            "check" => &*frame.check,
            "decided" => &frame.decided,
            "extends" => &*frame.extends,
        )
    }
}
