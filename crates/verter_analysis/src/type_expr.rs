//! Internal type expression AST for lightweight type resolution.
//!
//! `TypeExpr` is an internal syntax-preserving representation used by
//! the native evaluator. It is **not** the public output IR — that role
//! belongs to `TypeDescriptor` in `packages/component-meta/src/type-ir.ts`.
//!
//! # Design
//!
//! The AST is populated from OXC's `TSType` nodes during analysis.
//! The evaluator reduces `TypeExpr` → `TypeDescriptor` through the
//! symbol tables and evaluation environment.
//!
//! Node kinds cover the TypeScript type syntax subset needed for
//! component metadata resolution — not the full TS type system.

use serde::ser::Serialize;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock};

// ---------------------------------------------------------------------------
// Core AST
// ---------------------------------------------------------------------------

/// Internal type expression node.
///
/// Syntax-preserving — captures TypeScript type annotation structure
/// without evaluating or normalizing it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

    // -- References --
    /// A named type reference, optionally with type arguments.
    /// `MyType`, `Partial<T>`, `Record<K, V>`.
    Ref {
        name: Arc<str>,
        type_arguments: Arc<[TypeExpr]>,
    },

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

    /// A type the lowering could not represent.
    /// Carries the raw source text for diagnostics.
    Unknown { raw: String },
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
fn type_expr_from_json(v: &serde_json::Value) -> Option<TypeExpr> {
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
            Some(TypeExpr::Function(Arc::new(FunctionExpr {
                parameters: params,
                return_type: ret.map(Arc::new),
                type_parameters: vec![],
            })))
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
        "property" => Some(ObjectMember::Property(ObjectProperty {
            name: v.get("name")?.as_str()?.to_string(),
            ty: type_expr_from_json(v.get("ty")?)?,
            optional: v.get("optional").and_then(|o| o.as_bool()).unwrap_or(false),
            readonly: v.get("readonly").and_then(|o| o.as_bool()).unwrap_or(false),
        })),
        "indexSignature" => Some(ObjectMember::IndexSignature(IndexSignature {
            key_name: v.get("keyName")?.as_str()?.to_string(),
            key_type: type_expr_from_json(v.get("keyType")?)?,
            value_type: type_expr_from_json(v.get("valueType")?)?,
            readonly: v.get("readonly").and_then(|o| o.as_bool()).unwrap_or(false),
        })),
        "callSignature" => Some(ObjectMember::CallSignature(json_to_function_expr(
            v.get("function")?,
        )?)),
        "constructSignature" => Some(ObjectMember::ConstructSignature(json_to_function_expr(
            v.get("function")?,
        )?)),
        "method" => Some(ObjectMember::Method(MethodSignature {
            name: v.get("name")?.as_str()?.to_string(),
            function: json_to_function_expr(v.get("function")?)?,
            optional: v.get("optional").and_then(|o| o.as_bool()).unwrap_or(false),
        })),
        _ => None,
    }
}

fn json_to_func_params(v: &serde_json::Value) -> Option<Vec<FunctionParam>> {
    Some(
        v.as_array()?
            .iter()
            .filter_map(|p| {
                Some(FunctionParam {
                    name: p.get("name").and_then(|n| n.as_str().map(String::from)),
                    ty: type_expr_from_json(p.get("ty")?)?,
                    optional: p.get("optional").and_then(|o| o.as_bool()).unwrap_or(false),
                    rest: p.get("rest").and_then(|o| o.as_bool()).unwrap_or(false),
                })
            })
            .collect(),
    )
}

fn json_to_function_expr(v: &serde_json::Value) -> Option<FunctionExpr> {
    Some(FunctionExpr {
        parameters: json_to_func_params(v.get("parameters")?)?,
        return_type: v
            .get("returnType")
            .and_then(|ret| {
                if ret.is_null() {
                    None
                } else {
                    type_expr_from_json(ret)
                }
            })
            .map(Arc::new),
        type_parameters: vec![],
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

/// A named property in an object type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectProperty {
    pub name: String,
    pub ty: TypeExpr,
    pub optional: bool,
    pub readonly: bool,
}

/// An index signature: `[key: KeyType]: ValueType`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSignature {
    pub key_name: String,
    pub key_type: TypeExpr,
    pub value_type: TypeExpr,
    pub readonly: bool,
}

/// A method signature in an object type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodSignature {
    pub name: String,
    pub function: FunctionExpr,
    pub optional: bool,
}

/// A function type expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionExpr {
    pub parameters: Vec<FunctionParam>,
    pub return_type: Option<Arc<TypeExpr>>,
    pub type_parameters: Vec<TypeParam>,
}

/// A function parameter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionParam {
    pub name: Option<String>,
    pub ty: TypeExpr,
    pub optional: bool,
    pub rest: bool,
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

    /// Returns `true` if this is an `Unknown` node.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns `true` if this is a primitive type.
    #[cfg(test)]
    pub(crate) fn is_primitive(&self) -> bool {
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
            Self::Function(func) => json!({
                "kind": "function",
                "parameters": func.parameters.iter().map(|p| json!({
                    "name": p.name,
                    "ty": p.ty.to_json_value(),
                    "optional": p.optional,
                    "rest": p.rest
                })).collect::<Vec<_>>(),
                "returnType": func.return_type.as_ref().map(|r| r.to_json_value()),
            }),
            Self::Ref {
                name,
                type_arguments,
            } => json!({
                "kind": "ref",
                "name": name,
                "typeArguments": type_arguments.iter().map(|a| a.to_json_value()).collect::<Vec<_>>()
            }),
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
                .map(|tp| {
                    let mut obj = json!({ "name": tp.name });
                    if let Some(ref c) = tp.constraint {
                        obj["constraint"] = c.to_json_value();
                    }
                    if let Some(ref d) = tp.default {
                        obj["default"] = d.to_json_value();
                    }
                    obj
                })
                .collect::<Vec<serde_json::Value>>());
        }
        v
    }
}
