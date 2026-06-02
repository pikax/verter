//! JSON (de)serialisation for [`TypeExpr`].
//!
//! `TypeExpr` is a recursively-`Arc`-linked tree; a serde derive would hit
//! the recursion limit, so serialisation is hand-rolled as a runtime
//! dispatch over `serde_json::Value`. The [`Serialize`]/[`Deserialize`]
//! impls delegate to [`TypeExpr::to_json_value`] and
//! [`type_expr_from_json`].
//!
//! These impls live here (rather than inline in `lib.rs`) purely to keep
//! the crate root under the production file-size budget. The orphan rule
//! permits `impl Serialize`/`impl Deserialize for TypeExpr` in any module
//! of this crate because `TypeExpr` is crate-local; the helpers reach
//! every node type through `use crate::*`.

use crate::*;
use serde::ser::Serialize;
use std::sync::Arc;

impl Serialize for TypeExpr {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = self.to_json_value();
        value.serialize(serializer)
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
            Some(TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
                params,
                ret.map(Arc::new),
                json_to_type_params(v.get("typeParameters"))?,
            ))))
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
                ),
            )))
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
            Some(TypeExpr::TypeOf(ValueRef { path }))
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
                    s.as_str().map(Arc::<str>::from)
                }
            });
            let binding_name = v.get("bindingName")?.as_str()?;
            // valueNode is serialised as a decimal STRING to avoid JS
            // Number precision loss; decode it back to u64 here.
            let value_node = v.get("valueNode")?.as_str()?.parse::<u64>().ok()?;
            Some(TypeExpr::SyntheticSlotBinding(Arc::new(
                SyntheticCarrierKey {
                    scope_canonical_id: Arc::from(scope_canonical_id),
                    surface_kind,
                    slot_name,
                    binding_name: Arc::from(binding_name),
                    value_node,
                },
            )))
        }
        "unknown" => {
            let raw = v.get("raw")?.as_str()?.to_string();
            Some(TypeExpr::Unknown { raw })
        }
        _ => Some(TypeExpr::Unknown {
            raw: kind.to_string(),
        }),
    }
}

fn json_array_to_type_exprs(v: &serde_json::Value) -> Option<Vec<TypeExpr>> {
    v.as_array()?
        .iter()
        .map(type_expr_from_json)
        .collect::<Option<Vec<_>>>()
}

fn json_to_object_member(v: &serde_json::Value) -> Option<ObjectMember> {
    let mk = v.get("memberKind")?.as_str()?;
    match mk {
        "property" => Some(ObjectMember::Property(ObjectProperty::synthetic(
            v.get("name")?.as_str()?.to_string(),
            type_expr_from_json(v.get("ty")?)?,
            v.get("optional").and_then(|o| o.as_bool()).unwrap_or(false),
            v.get("readonly").and_then(|o| o.as_bool()).unwrap_or(false),
        ))),
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
        "method" => Some(ObjectMember::Method(MethodSignature::synthetic(
            v.get("name")?.as_str()?.to_string(),
            json_to_function_expr(v.get("function")?)?,
            v.get("optional").and_then(|o| o.as_bool()).unwrap_or(false),
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
    Some(FunctionExpr::synthetic(
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
    ))
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
    /// Convert to a JSON Value for serialization.
    /// Uses runtime dispatch to avoid serde derive recursion limit.
    pub fn to_json_value(&self) -> serde_json::Value {
        use serde_json::json;

        match self {
            Self::Primitive(name) => json!({ "kind": "primitive", "name": name.as_str() }),
            Self::Literal(lit) => match lit {
                LiteralValue::String(s) => {
                    json!({ "kind": "literal", "literalKind": "string", "value": s })
                }
                LiteralValue::Number(n) => {
                    json!({ "kind": "literal", "literalKind": "number", "value": n })
                }
                LiteralValue::Boolean(b) => {
                    json!({ "kind": "literal", "literalKind": "boolean", "value": b })
                }
                LiteralValue::BigInt(s) => {
                    json!({ "kind": "literal", "literalKind": "bigInt", "value": s })
                }
            },
            Self::Union(types) => json!({
                "kind": "union",
                "types": types.iter().map(|t| t.to_json_value()).collect::<Vec<_>>()
            }),
            Self::Intersection(types) => json!({
                "kind": "intersection",
                "types": types.iter().map(|t| t.to_json_value()).collect::<Vec<_>>()
            }),
            Self::Array { element, readonly } => json!({
                "kind": "array",
                "element": element.to_json_value(),
                "readonly": readonly
            }),
            Self::Tuple { elements, readonly } => json!({
                "kind": "tuple",
                "elements": elements.iter().map(|e| json!({
                    "label": e.label,
                    "ty": e.ty.to_json_value(),
                    "optional": e.optional,
                    "rest": e.rest
                })).collect::<Vec<_>>(),
                "readonly": readonly
            }),
            Self::Object(obj) => json!({
                "kind": "object",
                "properties": obj.properties.iter().map(|m| match m {
                    ObjectMember::Property(p) => json!({
                        "memberKind": "property",
                        "name": p.name,
                        "ty": p.ty.to_json_value(),
                        "optional": p.optional,
                        "readonly": p.readonly
                    }),
                    ObjectMember::IndexSignature(idx) => json!({
                        "memberKind": "indexSignature",
                        "keyName": idx.key_name,
                        "keyType": idx.key_type.to_json_value(),
                        "valueType": idx.value_type.to_json_value(),
                        "readonly": idx.readonly
                    }),
                    ObjectMember::CallSignature(f) => json!({
                        "memberKind": "callSignature",
                        "function": Self::function_to_json(f)
                    }),
                    ObjectMember::ConstructSignature(f) => json!({
                        "memberKind": "constructSignature",
                        "function": Self::function_to_json(f)
                    }),
                    ObjectMember::Method(m) => json!({
                        "memberKind": "method",
                        "name": m.name,
                        "function": Self::function_to_json(&m.function),
                        "optional": m.optional
                    }),
                }).collect::<Vec<_>>()
            }),
            Self::Function(func) => {
                let mut value = Self::function_to_json(func);
                value["kind"] = json!("function");
                value
            }
            Self::ConstructorType(func) => {
                let mut value = Self::function_to_json(func);
                value["kind"] = json!("constructorType");
                value
            }
            Self::Ref {
                name,
                type_arguments,
            } => json!({
                "kind": "ref",
                "name": name,
                "typeArguments": type_arguments.iter().map(|a| a.to_json_value()).collect::<Vec<_>>()
            }),
            Self::TypeParameter(param) => {
                let mut value = Self::type_param_to_json(param);
                value["kind"] = json!("typeParameter");
                value
            }
            Self::KeyOf(operand) => json!({ "kind": "keyOf", "operand": operand.to_json_value() }),
            Self::TypeOf(vr) => json!({ "kind": "typeOf", "path": vr.path }),
            Self::IndexedAccess { object, index } => json!({
                "kind": "indexedAccess",
                "object": object.to_json_value(),
                "index": index.to_json_value()
            }),
            Self::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => json!({
                "kind": "conditional",
                "check": check.to_json_value(),
                "extends": extends.to_json_value(),
                "trueType": true_type.to_json_value(),
                "falseType": false_type.to_json_value()
            }),
            Self::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
            } => json!({
                "kind": "mapped",
                "parameter": parameter,
                "source": source.to_json_value(),
                "value": value.to_json_value(),
                "optional": modifier_str(*optional),
                "readonly": modifier_str(*readonly),
                "nameType": name_type.as_ref().map(|n| n.to_json_value())
            }),
            Self::TemplateLiteral {
                quasis,
                expressions,
            } => json!({
                "kind": "templateLiteral",
                "quasis": quasis,
                "expressions": expressions.iter().map(|e| e.to_json_value()).collect::<Vec<_>>()
            }),
            Self::Infer { name } => json!({ "kind": "infer", "name": name }),
            Self::Rest(inner) => json!({ "kind": "rest", "inner": inner.to_json_value() }),
            Self::Parenthesized(inner) => {
                json!({ "kind": "parenthesized", "inner": inner.to_json_value() })
            }
            Self::RecursiveRef {
                name,
                type_arguments,
                conditional_context,
            } => json!({
                "kind": "recursiveRef",
                "name": name,
                "typeArguments": type_arguments.iter().map(|a| a.to_json_value()).collect::<Vec<_>>(),
                "conditionalContext": conditional_context.iter().map(|f| json!({
                    "branch": match f.branch {
                        RecursiveConditionalBranch::True => "true",
                        RecursiveConditionalBranch::False => "false",
                    },
                    "decided": f.decided,
                    "check": f.check.to_json_value(),
                    "extends": f.extends.to_json_value()
                })).collect::<Vec<_>>()
            }),
            Self::SyntheticSlotBinding(key) => json!({
                "kind": "syntheticSlotBinding",
                "scopeCanonicalId": key.scope_canonical_id.as_ref(),
                "surfaceKind": match key.surface_kind {
                    SyntheticCarrierSurfaceKind::SlotBinding => "slotBinding",
                    SyntheticCarrierSurfaceKind::Binding => "binding",
                },
                "slotName": key.slot_name.as_deref(),
                "bindingName": key.binding_name.as_ref(),
                "valueNode": key.value_node.to_string(),
            }),
            Self::Unknown { raw } => json!({ "kind": "unknown", "raw": raw }),
        }
    }

    fn function_to_json(func: &FunctionExpr) -> serde_json::Value {
        use serde_json::json;
        let mut v = json!({
            "parameters": func.parameters.iter().map(|p| json!({
                "name": p.name,
                "ty": p.ty.to_json_value(),
                "optional": p.optional,
                "rest": p.rest
            })).collect::<Vec<serde_json::Value>>(),
            "returnType": func.return_type.as_ref().map(|r| r.to_json_value()),
        });
        if !func.type_parameters.is_empty() {
            v["typeParameters"] = json!(func
                .type_parameters
                .iter()
                .map(Self::type_param_to_json)
                .collect::<Vec<serde_json::Value>>());
        }
        v
    }

    fn type_param_to_json(param: &TypeParam) -> serde_json::Value {
        use serde_json::json;
        let mut obj = json!({ "name": param.name });
        if let Some(ref constraint) = param.constraint {
            obj["constraint"] = constraint.to_json_value();
        }
        if let Some(ref default) = param.default {
            obj["default"] = default.to_json_value();
        }
        obj
    }
}
