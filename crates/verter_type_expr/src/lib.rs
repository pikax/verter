//! Internal type expression AST for lightweight type resolution.
//!
//! `TypeExpr` is an internal syntax-preserving representation used by
//! the native evaluator. It is **not** the public output IR — that role
//! belongs to `TypeDescriptor` in `packages/component-meta/src/type-ir.ts`.
//!
//! # Design
//!
//! The AST is populated from OXC's `TSType` nodes during analysis
//! (lowering lives in the sibling `verter_type_expr_oxc` crate so
//! consumers that only need the data tier — NAPI / WASM / JSON
//! readers — can avoid pulling in OXC).
//!
//! The evaluator reduces `TypeExpr` → `TypeDescriptor` through the
//! symbol tables and evaluation environment.
//!
//! Node kinds cover the TypeScript type syntax subset needed for
//! component metadata resolution — not the full TS type system.

use serde::ser::Serialize;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock};
use verter_span::Span;

/// In-place declaration-site span transforms over [`TypeExpr`]
/// ([`TypeExpr::shift_spans`] / [`TypeExpr::clear_spans`]).
mod span_transform;

/// Depth-safe iterative `Drop` + byte-identical-to-derive iterative
/// `Hash` for [`TypeExpr`] (orphan-rule-permitted in this crate-local
/// module; kept out of the crate root for file-size hygiene).
mod recursive_traversal;

// ---------------------------------------------------------------------------
// Send + Sync invariant
// ---------------------------------------------------------------------------

const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TypeExpr>();
    assert_send_sync::<TypeExprScope>();
};

// ---------------------------------------------------------------------------
// TypeExprScope — scope sidecar for paired `*_expr` schema fields
// ---------------------------------------------------------------------------

/// Scope sidecar for a paired `TypeExpr`. Carries the canonical_id of
/// the file whose OXC parse produced the typed expression. Consumers
/// walking nested `TypeExpr::Ref` nodes resolve them in the file where
/// the annotation was written — which differs from the SFC owner for
/// cross-file pre-resolved props.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct TypeExprScope(pub String);

impl TypeExprScope {
    pub fn new(canonical_id: impl Into<String>) -> Self {
        Self(canonical_id.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// Core AST
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SyntheticSlotBinding carrier — typed-IR variant minted by
// `publish_merged_bindings` at the no-parser branch
// ---------------------------------------------------------------------------

/// Surface kind for a synthetic carrier minted at slot-binding or
/// `defineSlots` binding publication when no parser-side binding
/// expression is available. Used to distinguish the two surfaces on
/// the typed-IR variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntheticCarrierSurfaceKind {
    SlotBinding,
    Binding,
}

/// Intrinsic, shallow-by-construction identity for a synthetic carrier
/// minted by `publish_merged_bindings`. Identity is the FULL
/// (scope_canonical_id, surface_kind, slot_name, binding_name, value_node)
/// tuple — `value_node` discriminates two same-named carriers in
/// different slots of the same component. The carrier is NEVER
/// resolved as a type alias via the type registry; same-name
/// poisoning of a real workspace alias is structurally impossible
/// because it lives on a distinct `TypeExpr` variant.
///
/// `value_node` is stored as `u64` because `verter_type_expr` cannot
/// depend on `verter_session`. FFI / JSON serialise `value_node` as a
/// decimal STRING to avoid JS Number precision loss.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SyntheticCarrierKey {
    pub scope_canonical_id: Arc<str>,
    pub surface_kind: SyntheticCarrierSurfaceKind,
    pub slot_name: Option<Arc<str>>,
    pub binding_name: Arc<str>,
    pub value_node: u64,
}

/// Internal type expression node.
///
/// Syntax-preserving — captures TypeScript type annotation structure
/// without evaluating or normalizing it.
///
/// `Hash` is implemented by hand (NOT derived) as a depth-safe
/// continuation-frame iterative walker — see the `impl Hash for TypeExpr`
/// in the [`recursive_traversal`] module. The derived `Hash` was
/// recursive over the `Arc<TypeExpr>` tree
/// and overflowed the stack on deeply-nested types (e.g.
/// `cycle_guard::hash_type_expr` routes a `TypeExpr` through `Hash`). The
/// manual impl emits a BYTE-IDENTICAL stream to the former derive
/// (pinned by `tests/hash_byte_stream_contract.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    // -- Terminals --
    /// A primitive type name: `string`, `number`, `boolean`, `symbol`,
    /// `bigint`, `any`, `unknown`, `void`, `never`, `null`, `undefined`, `object`.
    Primitive(PrimitiveName),

    /// A literal type: `"hello"`, `42`, `true`, `false`.
    Literal(LiteralValue),

    // -- Compound --
    /// `A | B | C`
    Union(Arc<[TypeExpr]>),

    /// `A & B & C`
    Intersection(Arc<[TypeExpr]>),

    /// `T[]` or `Array<T>` or `ReadonlyArray<T>`.
    Array {
        element: Arc<TypeExpr>,
        readonly: bool,
    },

    /// `[A, B, C]` — optionally labeled.
    Tuple {
        elements: Arc<[TupleElement]>,
        readonly: bool,
    },

    /// `{ prop: Type; prop?: Type; [key: string]: Type }`
    Object(Arc<ObjectExpr>),

    /// `(x: T, y: U) => R`
    Function(Arc<FunctionExpr>),

    /// `new (x: T) => R` — a constructor type (TS `TSConstructorType`).
    ///
    /// Distinct from both [`Function`](Self::Function) and from a type-literal
    /// `{ new (): R }` (which lowers to [`Object`](Self::Object) carrying an
    /// [`ObjectMember::ConstructSignature`]). The two are structurally identical
    /// after lowering otherwise, yet Vue's runtime-constructor inference treats a
    /// bare constructor *type* as `Function` while a type-literal-with-construct-
    /// signature is `Object` — so the producer must keep them apart. Carries the
    /// same [`FunctionExpr`] payload (parameters / return / type parameters /
    /// spans) as a construct signature, so a consumer that wants the construct
    /// semantics walks the inner function exactly as it does for a
    /// `ConstructSignature` member.
    ConstructorType(Arc<FunctionExpr>),

    // -- References --
    /// A named type reference, optionally with type arguments.
    /// `MyType`, `Partial<T>`, `Record<K, V>`.
    Ref {
        name: Arc<str>,
        type_arguments: Arc<[TypeExpr]>,
    },

    /// A first-class generic type parameter reference carrying declaration metadata.
    TypeParameter(TypeParam),

    // -- Operators --
    /// `keyof T`
    KeyOf(Arc<TypeExpr>),

    /// `typeof x` — refers to a value binding.
    TypeOf(ValueRef),

    /// `T[K]` — indexed access.
    IndexedAccess {
        object: Arc<TypeExpr>,
        index: Arc<TypeExpr>,
    },

    /// `T extends U ? A : B`
    Conditional {
        check: Arc<TypeExpr>,
        extends: Arc<TypeExpr>,
        true_type: Arc<TypeExpr>,
        false_type: Arc<TypeExpr>,
    },

    /// `{ [K in Source]: Value }` — mapped type.
    Mapped {
        parameter: String,
        source: Arc<TypeExpr>,
        value: Arc<TypeExpr>,
        optional: MappedModifier,
        readonly: MappedModifier,
        name_type: Option<Arc<TypeExpr>>,
    },

    /// `` `prefix${T}suffix` `` — template literal type.
    TemplateLiteral {
        /// Alternating text spans and type expressions.
        /// `quasis[0]` expr[0] `quasis[1]` expr[1] ... `quasis[n]`
        quasis: Vec<String>,
        expressions: Arc<[TypeExpr]>,
    },

    /// `infer T` — only valid inside conditional types.
    Infer { name: String },

    /// `readonly T` or rest `...T` at tuple level (handled by TupleElement).
    /// This variant catches standalone `readonly` or rest when not in tuple context.
    Rest(Arc<TypeExpr>),

    /// Parenthesized type — `(A | B)`. Preserved for fidelity but
    /// transparent to evaluation.
    Parenthesized(Arc<TypeExpr>),

    /// A recursive type reference placeholder — produced by the solver when
    /// recursion is detected during type expansion. Preserves the recursive
    /// symbol name, applied type arguments, and active conditional context.
    RecursiveRef {
        name: Arc<str>,
        type_arguments: Arc<[TypeExpr]>,
        conditional_context: Arc<[RecursiveConditionalFrame]>,
    },

    /// Synthetic slot-binding / `defineSlots` binding carrier. Minted only
    /// at the no-parser branch of `publish_merged_bindings`. The
    /// projector pipeline and component-meta registry treat this variant
    /// as a shallow terminal — explicit deep materialisation routes
    /// through `ShapeCacheKey::semantic_node_whole(scope, value_node,
    /// mode)`. See `[[component-meta-shallow-by-default-rule]]`.
    SyntheticSlotBinding(Arc<SyntheticCarrierKey>),

    /// A type the lowering could not represent.
    /// Carries the raw source text for diagnostics.
    Unknown { raw: String },
}

// ---------------------------------------------------------------------------
// Recursive conditional context types
// ---------------------------------------------------------------------------

/// A snapshot of one conditional branch frame at the moment recursion was detected.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecursiveConditionalFrame {
    pub branch: RecursiveConditionalBranch,
    pub decided: bool,
    pub check: Arc<TypeExpr>,
    pub extends: Arc<TypeExpr>,
}

/// Which branch of a conditional type was active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecursiveConditionalBranch {
    True,
    False,
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

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

/// Primitive type names recognized by the evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrimitiveName {
    String,
    Number,
    Boolean,
    Symbol,
    BigInt,
    Any,
    Unknown,
    Void,
    Never,
    Null,
    Undefined,
    Object,
}

impl PrimitiveName {
    /// Try to parse a primitive name from a string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "string" => Some(Self::String),
            "number" => Some(Self::Number),
            "boolean" => Some(Self::Boolean),
            "symbol" => Some(Self::Symbol),
            "bigint" => Some(Self::BigInt),
            "any" => Some(Self::Any),
            "unknown" => Some(Self::Unknown),
            "void" => Some(Self::Void),
            "never" => Some(Self::Never),
            "null" => Some(Self::Null),
            "undefined" => Some(Self::Undefined),
            "object" => Some(Self::Object),
            _ => None,
        }
    }

    /// Returns the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Symbol => "symbol",
            Self::BigInt => "bigint",
            Self::Any => "any",
            Self::Unknown => "unknown",
            Self::Void => "void",
            Self::Never => "never",
            Self::Null => "null",
            Self::Undefined => "undefined",
            Self::Object => "object",
        }
    }
}

impl fmt::Display for PrimitiveName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A literal value in a type position.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "literalKind", rename_all = "camelCase")]
pub enum LiteralValue {
    String(String),
    Number(f64),
    Boolean(bool),
    BigInt(String),
}

// Manual PartialEq: f64 NaN must compare as equal for type identity.
impl PartialEq for LiteralValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a.to_bits() == b.to_bits(),
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            (Self::BigInt(a), Self::BigInt(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for LiteralValue {}

impl Hash for LiteralValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::String(value) => {
                0u8.hash(state);
                value.hash(state);
            }
            Self::Number(value) => {
                1u8.hash(state);
                value.to_bits().hash(state);
            }
            Self::Boolean(value) => {
                2u8.hash(state);
                value.hash(state);
            }
            Self::BigInt(value) => {
                3u8.hash(state);
                value.hash(state);
            }
        }
    }
}

/// A reference to a value binding (for `typeof` expressions).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueRef {
    /// Dotted path segments: `typeof a.b.c` → `["a", "b", "c"]`.
    pub path: Vec<String>,
}

/// A single element in a tuple type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TupleElement {
    /// Optional label name.
    pub label: Option<String>,
    /// The element type.
    pub ty: TypeExpr,
    /// Whether this element is optional (`?`).
    pub optional: bool,
    /// Whether this element is a rest element (`...T`).
    pub rest: bool,
}

/// An object type expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectExpr {
    pub properties: Vec<ObjectMember>,
}

/// A member of an object type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(tag = "memberKind", rename_all = "camelCase")]
pub enum ObjectMember {
    /// Named property: `name: Type` or `name?: Type`.
    Property(ObjectProperty),
    /// Index signature: `[key: string]: Type`.
    IndexSignature(IndexSignature),
    /// Call signature: `(x: T): R`.
    CallSignature(FunctionExpr),
    /// Construct signature: `new (x: T): R`.
    ConstructSignature(FunctionExpr),
    /// Method signature: `method(x: T): R`.
    Method(MethodSignature),
}

/// OXC-derived declaration-site spans for a named member (property or method).
///
/// Stamped once during shallow OXC lowering (the sole place the AST offsets
/// exist) and carried verbatim through the IR into the semantic graph payload.
/// Every span is in the owning file's source coordinates. `None` only for a
/// genuinely synthetic member (one with no single source site); never as a
/// "not implemented" placeholder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct MemberSpans {
    /// Span of the whole member declaration (`name?: T` / `name(): T`).
    pub declaration: Option<Span>,
    /// Span of the member's name token.
    pub name: Option<Span>,
    /// Span of the member's type-annotation (the `T` after `:`), when present.
    pub type_annotation: Option<Span>,
}

impl MemberSpans {
    /// Spans for a member where ONLY the name span is known (an aggregate
    /// surface synthesized from per-field analysis, where the field tracks the
    /// name span but the declaration is not a single contiguous source range).
    ///
    /// An empty span (`start >= end`, e.g. a default placeholder) carries no
    /// real provenance, so it maps to `None` rather than fabricating a byte-0
    /// span — honest absence, never a wrong offset.
    #[must_use]
    pub fn name_only(name: Span) -> Self {
        Self {
            declaration: None,
            name: (!name.is_empty()).then_some(name),
            type_annotation: None,
        }
    }
}

/// OXC-derived spans for a call / construct / method function signature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FunctionSpans {
    /// Span of the whole signature declaration.
    pub signature: Option<Span>,
    /// Span of the return-type annotation, when present.
    pub return_type: Option<Span>,
}

/// OXC-derived spans for an index signature (`[k: K]: V`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct IndexSignatureSpans {
    /// Span of the whole index-signature declaration.
    pub declaration: Option<Span>,
    /// Span of the key declaration (`[k: K]` parameter / key-type).
    pub key: Option<Span>,
    /// Span of the value-type annotation.
    pub value: Option<Span>,
}

/// A named property in an object type.
///
/// `spans` carries OXC declaration-site provenance (see [`MemberSpans`]) and is
/// in-memory-only — it is intentionally excluded from the JSON wire shape (the
/// manual `to_json_value` / `type_expr_from_json` helpers do not serialize it).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ObjectProperty {
    pub name: String,
    pub ty: TypeExpr,
    pub optional: bool,
    pub readonly: bool,
    /// OXC declaration-site spans (in-memory provenance; not serialized).
    #[serde(skip)]
    pub spans: MemberSpans,
}

impl ObjectProperty {
    /// Construct a property with NO source spans (a synthesized property with
    /// no single declaration site — e.g. a test fixture or a derived member).
    #[must_use]
    pub fn synthetic(name: String, ty: TypeExpr, optional: bool, readonly: bool) -> Self {
        Self {
            name,
            ty,
            optional,
            readonly,
            spans: MemberSpans::default(),
        }
    }

    /// Construct a property carrying its OXC declaration-site spans.
    #[must_use]
    pub fn with_spans(
        name: String,
        ty: TypeExpr,
        optional: bool,
        readonly: bool,
        spans: MemberSpans,
    ) -> Self {
        Self {
            name,
            ty,
            optional,
            readonly,
            spans,
        }
    }
}

/// An index signature: `[key: KeyType]: ValueType`.
///
/// `spans` carries OXC declaration-site provenance and is in-memory-only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct IndexSignature {
    pub key_name: String,
    pub key_type: TypeExpr,
    pub value_type: TypeExpr,
    pub readonly: bool,
    /// OXC declaration-site spans (in-memory provenance; not serialized).
    #[serde(skip)]
    pub spans: IndexSignatureSpans,
}

impl IndexSignature {
    /// Construct an index signature with NO source spans.
    #[must_use]
    pub fn synthetic(
        key_name: String,
        key_type: TypeExpr,
        value_type: TypeExpr,
        readonly: bool,
    ) -> Self {
        Self {
            key_name,
            key_type,
            value_type,
            readonly,
            spans: IndexSignatureSpans::default(),
        }
    }

    /// Construct an index signature carrying its OXC declaration-site spans.
    #[must_use]
    pub fn with_spans(
        key_name: String,
        key_type: TypeExpr,
        value_type: TypeExpr,
        readonly: bool,
        spans: IndexSignatureSpans,
    ) -> Self {
        Self {
            key_name,
            key_type,
            value_type,
            readonly,
            spans,
        }
    }
}

/// A method signature in an object type.
///
/// `spans` carries OXC declaration-site provenance and is in-memory-only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MethodSignature {
    pub name: String,
    pub function: FunctionExpr,
    pub optional: bool,
    /// OXC declaration-site spans (in-memory provenance; not serialized).
    #[serde(skip)]
    pub spans: MemberSpans,
}

impl MethodSignature {
    /// Construct a method signature with NO source spans.
    #[must_use]
    pub fn synthetic(name: String, function: FunctionExpr, optional: bool) -> Self {
        Self {
            name,
            function,
            optional,
            spans: MemberSpans::default(),
        }
    }

    /// Construct a method signature carrying its OXC declaration-site spans.
    #[must_use]
    pub fn with_spans(
        name: String,
        function: FunctionExpr,
        optional: bool,
        spans: MemberSpans,
    ) -> Self {
        Self {
            name,
            function,
            optional,
            spans,
        }
    }
}

/// A function type expression.
///
/// `spans` carries OXC declaration-site provenance and is in-memory-only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FunctionExpr {
    pub parameters: Vec<FunctionParam>,
    pub return_type: Option<Arc<TypeExpr>>,
    pub type_parameters: Vec<TypeParam>,
    /// OXC signature / return spans (in-memory provenance; not serialized).
    #[serde(skip)]
    pub spans: FunctionSpans,
}

impl FunctionExpr {
    /// Construct a function expression with NO source spans.
    #[must_use]
    pub fn synthetic(
        parameters: Vec<FunctionParam>,
        return_type: Option<Arc<TypeExpr>>,
        type_parameters: Vec<TypeParam>,
    ) -> Self {
        Self {
            parameters,
            return_type,
            type_parameters,
            spans: FunctionSpans::default(),
        }
    }

    /// Construct a function expression carrying its OXC spans.
    #[must_use]
    pub fn with_spans(
        parameters: Vec<FunctionParam>,
        return_type: Option<Arc<TypeExpr>>,
        type_parameters: Vec<TypeParam>,
        spans: FunctionSpans,
    ) -> Self {
        Self {
            parameters,
            return_type,
            type_parameters,
            spans,
        }
    }
}

/// A function parameter.
///
/// `span` is the OXC parameter span (in-memory provenance; not serialized).
///
/// `PartialEq`/`Eq`/`Hash` are implemented by hand to EXCLUDE
/// [`has_ts_annotation`](Self::has_ts_annotation): that field is a transient
/// lowering-time gate for JSDoc `@param` precedence, not part of a parameter's
/// semantic type identity. Two parameters with the same name / type / optional /
/// rest / span are the same parameter whether the annotation was written
/// explicitly or filled from JSDoc — and the graph round-trip (the canonical
/// semantic form) intentionally does not preserve the fact, so it must not split
/// otherwise-equal parameters across cache keys or equivalence checks. `span`
/// remains part of identity (it is a real provenance coordinate).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FunctionParam {
    pub name: Option<String>,
    pub ty: TypeExpr,
    pub optional: bool,
    pub rest: bool,
    /// OXC span of the whole parameter (in-memory provenance; not serialized).
    #[serde(skip)]
    pub span: Option<Span>,
    /// Whether this parameter carried an explicit TS type annotation at its
    /// declaration site (`FormalParameter`/`BindingPattern` had a
    /// `type_annotation`). This is the OXC STRUCTURAL FACT — captured once at
    /// the lowering site — NOT a sentinel inferred from the lowered [`TypeExpr`]
    /// (an explicit `: any` lowers to `Primitive(Any)` exactly like a missing
    /// annotation does, so the lowered type cannot distinguish them). JSDoc
    /// `@param` backfill fills a parameter ONLY when this is `false`, matching
    /// TS precedence (an explicit annotation — including `: any` — always wins).
    /// In-memory provenance; not serialized and NOT part of type identity (see
    /// the type-level note on the hand-written `PartialEq`/`Eq`/`Hash`).
    #[serde(skip)]
    pub has_ts_annotation: bool,
}

impl PartialEq for FunctionParam {
    fn eq(&self, other: &Self) -> bool {
        // `has_ts_annotation` is a transient lowering-time gate, not semantic
        // identity — deliberately excluded so equivalent parameters built by
        // different paths (e.g. the eager IR path vs the graph round-trip)
        // compare equal.
        self.name == other.name
            && self.ty == other.ty
            && self.optional == other.optional
            && self.rest == other.rest
            && self.span == other.span
    }
}

impl Eq for FunctionParam {}

impl std::hash::Hash for FunctionParam {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Mirror `PartialEq`: hash every identity field EXCEPT
        // `has_ts_annotation`, so equal parameters hash equally.
        self.name.hash(state);
        self.ty.hash(state);
        self.optional.hash(state);
        self.rest.hash(state);
        self.span.hash(state);
    }
}

impl FunctionParam {
    /// Construct a parameter with NO source span. A synthesized parameter has no
    /// declaration site, so it carries no TS-annotation fact (`has_ts_annotation
    /// == false`); synthesized parameters are never JSDoc-enriched.
    #[must_use]
    pub fn synthetic(name: Option<String>, ty: TypeExpr, optional: bool, rest: bool) -> Self {
        Self {
            name,
            ty,
            optional,
            rest,
            span: None,
            has_ts_annotation: false,
        }
    }

    /// Construct a parameter carrying its OXC span and the structural fact of
    /// whether it had an explicit TS type annotation at its declaration site.
    #[must_use]
    pub fn with_span(
        name: Option<String>,
        ty: TypeExpr,
        optional: bool,
        rest: bool,
        span: Option<Span>,
        has_ts_annotation: bool,
    ) -> Self {
        Self {
            name,
            ty,
            optional,
            rest,
            span,
            has_ts_annotation,
        }
    }
}

/// A type parameter declaration: `T extends Constraint = Default`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeParam {
    pub name: String,
    pub constraint: Option<Arc<TypeExpr>>,
    pub default: Option<Arc<TypeExpr>>,
}

/// Modifier for mapped type `optional` and `readonly` fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MappedModifier {
    /// No modifier applied.
    None,
    /// `+` or bare modifier (add).
    Add,
    /// `-` modifier (remove).
    Remove,
}

// ---------------------------------------------------------------------------
// Shared constants
// ---------------------------------------------------------------------------

/// Returns a shared empty type argument slice, avoiding per-call allocation.
pub fn empty_type_args() -> Arc<[TypeExpr]> {
    static EMPTY: LazyLock<Arc<[TypeExpr]>> = LazyLock::new(|| Arc::from(Vec::<TypeExpr>::new()));
    Arc::clone(&EMPTY)
}

// ---------------------------------------------------------------------------
// Factory helpers
// ---------------------------------------------------------------------------

impl TypeExpr {
    /// Create a primitive type.
    pub fn primitive(name: PrimitiveName) -> Self {
        Self::Primitive(name)
    }

    /// Create a string literal type.
    pub fn string_literal(s: impl Into<String>) -> Self {
        Self::Literal(LiteralValue::String(s.into()))
    }

    /// Create a number literal type.
    pub fn number_literal(n: f64) -> Self {
        Self::Literal(LiteralValue::Number(n))
    }

    /// Create a boolean literal type.
    pub fn boolean_literal(b: bool) -> Self {
        Self::Literal(LiteralValue::Boolean(b))
    }

    /// Create a union type. Empty → `never`, single → unwrap.
    pub fn union(types: Vec<TypeExpr>) -> Self {
        match types.len() {
            0 => Self::Primitive(PrimitiveName::Never),
            1 => types.into_iter().next().unwrap(),
            _ => Self::Union(Arc::from(types)),
        }
    }

    /// Create an intersection type. Empty → `unknown`, single → unwrap.
    pub fn intersection(types: Vec<TypeExpr>) -> Self {
        match types.len() {
            0 => Self::Primitive(PrimitiveName::Unknown),
            1 => types.into_iter().next().unwrap(),
            _ => Self::Intersection(Arc::from(types)),
        }
    }

    /// Create a type reference without type arguments.
    pub fn named(name: impl Into<String>) -> Self {
        Self::Ref {
            name: Arc::from(name.into()),
            type_arguments: empty_type_args(),
        }
    }

    /// Create a type reference with type arguments.
    pub fn named_with_args(name: impl Into<String>, args: Vec<TypeExpr>) -> Self {
        Self::Ref {
            name: Arc::from(name.into()),
            type_arguments: Arc::from(args),
        }
    }

    /// Create a first-class generic type parameter reference.
    pub fn type_parameter(param: TypeParam) -> Self {
        Self::TypeParameter(param)
    }

    /// Create a recursive ref with no conditional context.
    pub fn recursive_ref(name: impl Into<String>, args: Vec<TypeExpr>) -> Self {
        Self::RecursiveRef {
            name: Arc::from(name.into()),
            type_arguments: Arc::from(args),
            conditional_context: Arc::from(Vec::<RecursiveConditionalFrame>::new()),
        }
    }

    /// Create a synthetic slot-binding / `defineSlots` binding carrier.
    /// See [`SyntheticCarrierKey`] for identity semantics.
    pub fn synthetic_slot_binding(key: SyntheticCarrierKey) -> Self {
        Self::SyntheticSlotBinding(Arc::new(key))
    }

    /// Returns `true` if this is a `RecursiveRef` node.
    pub fn is_recursive_ref(&self) -> bool {
        matches!(self, Self::RecursiveRef { .. })
    }

    /// Returns `true` if this is an `Unknown` node.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns `true` if this is a primitive type.
    pub fn is_primitive(&self) -> bool {
        matches!(self, Self::Primitive(_))
    }

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
