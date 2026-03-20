//! Lightweight type evaluator for component metadata resolution.
//!
//! Reduces [`TypeExpr`] trees into normalized forms using symbol tables.
//! Handles common TypeScript utility types, `keyof`, `typeof`, indexed
//! access, and generic substitution without requiring a full TS type checker.
//!
//! # Design
//!
//! The evaluator operates on an [`EvalEnv`] containing:
//! - **Type symbols**: interfaces, type aliases, and their bodies as `TypeExpr`
//! - **Value symbols**: functions, constants, classes with structured signatures
//! - **Type bindings**: generic parameter → argument mappings for instantiation
//!
//! Evaluation is demand-driven with cycle detection and configurable limits.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::type_expr::*;

// ---------------------------------------------------------------------------
// Symbol table types
// ---------------------------------------------------------------------------

/// A type declaration in the evaluator's symbol table.
#[derive(Debug, Clone)]
pub struct TypeDeclInfo {
    pub name: String,
    pub kind: TypeDeclKind,
    pub type_parameters: Vec<TypeParam>,
    pub body: TypeExpr,
}

/// What kind of type declaration this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeDeclKind {
    Alias,
    Interface,
    Class,
}

/// A value declaration in the evaluator's value symbol table.
#[derive(Debug, Clone)]
pub struct ValueDeclInfo {
    pub name: String,
    pub kind: ValueDeclKind,
    /// Explicit type annotation, if present.
    pub type_annotation: Option<TypeExpr>,
    /// Function/method signature, if this is a function or method.
    pub function_signature: Option<FunctionSignature>,
    /// Object literal shape, if this is a const initialized with an object.
    pub object_shape: Option<ObjectExpr>,
}

/// What kind of value declaration this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueDeclKind {
    Const,
    Let,
    Var,
    Function,
    AsyncFunction,
    Class,
}

/// A function signature extracted from a declaration.
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub parameters: Vec<FunctionParam>,
    pub return_type: Option<TypeExpr>,
    pub type_parameters: Vec<TypeParam>,
}

// ---------------------------------------------------------------------------
// Evaluation environment
// ---------------------------------------------------------------------------

/// Evaluation environment holding symbol tables and evaluation state.
pub struct EvalEnv {
    /// Type declarations: interfaces, type aliases.
    pub type_symbols: FxHashMap<String, TypeDeclInfo>,
    /// Value declarations: functions, constants, classes.
    pub value_symbols: FxHashMap<String, ValueDeclInfo>,
    /// Generic type parameter bindings for the current instantiation.
    pub type_bindings: FxHashMap<String, TypeExpr>,
    /// Currently being evaluated (cycle detection).
    active: FxHashSet<String>,
    /// Evaluation limits.
    pub limits: EvalLimits,
    /// Current recursion depth.
    depth: usize,
}

/// Configurable limits for the evaluator.
#[derive(Debug, Clone)]
pub struct EvalLimits {
    pub max_depth: usize,
    pub max_union_expansion: usize,
    pub max_mapped_keys: usize,
}

impl Default for EvalLimits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_union_expansion: 64,
            max_mapped_keys: 128,
        }
    }
}

impl EvalEnv {
    /// Create a new evaluation environment with default limits.
    pub fn new() -> Self {
        Self {
            type_symbols: FxHashMap::default(),
            value_symbols: FxHashMap::default(),
            type_bindings: FxHashMap::default(),
            active: FxHashSet::default(),
            limits: EvalLimits::default(),
            depth: 0,
        }
    }

    /// Create an environment with custom limits.
    pub fn with_limits(limits: EvalLimits) -> Self {
        Self {
            limits,
            ..Self::new()
        }
    }

    /// Register a type declaration.
    pub fn add_type(&mut self, decl: TypeDeclInfo) {
        self.type_symbols.insert(decl.name.clone(), decl);
    }

    /// Register a value declaration.
    pub fn add_value(&mut self, decl: ValueDeclInfo) {
        self.value_symbols.insert(decl.name.clone(), decl);
    }

    /// Merge declarations from another environment without overwriting
    /// declarations already present in `self`.
    pub fn extend_missing(&mut self, other: EvalEnv) {
        for (name, decl) in other.type_symbols {
            self.type_symbols.entry(name).or_insert(decl);
        }
        for (name, decl) in other.value_symbols {
            self.value_symbols.entry(name).or_insert(decl);
        }
    }
}

impl Default for EvalEnv {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Evaluator
// ---------------------------------------------------------------------------

/// Evaluate a `TypeExpr` in the given environment, reducing references
/// and applying utility type semantics.
///
/// Returns a normalized `TypeExpr` with references resolved where possible.
pub fn evaluate(expr: &TypeExpr, env: &mut EvalEnv) -> TypeExpr {
    if env.depth > env.limits.max_depth {
        return expr.clone();
    }
    env.depth += 1;
    let result = evaluate_inner(expr, env);
    env.depth -= 1;
    result
}

fn evaluate_inner(expr: &TypeExpr, env: &mut EvalEnv) -> TypeExpr {
    match expr {
        // Terminals — pass through
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) | TypeExpr::Unknown { .. } => expr.clone(),

        // Unwrap parenthesized
        TypeExpr::Parenthesized(inner) => evaluate(inner, env),

        // Union — evaluate each branch
        TypeExpr::Union(types) => {
            let evaluated: Vec<TypeExpr> = types.iter().map(|t| evaluate(t, env)).collect();
            TypeExpr::union(evaluated)
        }

        // Intersection — evaluate each branch and merge objects
        TypeExpr::Intersection(types) => {
            let evaluated: Vec<TypeExpr> = types.iter().map(|t| evaluate(t, env)).collect();
            merge_intersection(evaluated)
        }

        // Array — evaluate element
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: Box::new(evaluate(element, env)),
            readonly: *readonly,
        },

        // Tuple — evaluate each element
        TypeExpr::Tuple { elements, readonly } => {
            let evaluated = elements
                .iter()
                .map(|e| TupleElement {
                    label: e.label.clone(),
                    ty: evaluate(&e.ty, env),
                    optional: e.optional,
                    rest: e.rest,
                })
                .collect();
            TypeExpr::Tuple {
                elements: evaluated,
                readonly: *readonly,
            }
        }

        // Object — evaluate property types
        TypeExpr::Object(obj) => {
            let properties = obj
                .properties
                .iter()
                .map(|m| evaluate_object_member(m, env))
                .collect();
            TypeExpr::Object(ObjectExpr { properties })
        }

        // Function — evaluate param and return types
        TypeExpr::Function(func) => TypeExpr::Function(evaluate_function(func, env)),

        // Type reference — resolve
        TypeExpr::Ref {
            name,
            type_arguments,
        } => evaluate_ref(name, type_arguments, env),

        // keyof T
        TypeExpr::KeyOf(operand) => {
            let resolved = evaluate(operand, env);
            evaluate_keyof(&resolved)
        }

        // typeof x
        TypeExpr::TypeOf(value_ref) => evaluate_typeof(value_ref, env),

        // T[K]
        TypeExpr::IndexedAccess { object, index } => {
            let obj = evaluate(object, env);
            let idx = evaluate(index, env);
            evaluate_indexed_access(&obj, &idx)
        }

        // T extends U ? A : B
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let check_eval = evaluate(check, env);
            let extends_eval = evaluate(extends, env);
            if is_assignable_to(&check_eval, &extends_eval) {
                evaluate(true_type, env)
            } else if is_definitely_not_assignable(&check_eval, &extends_eval) {
                evaluate(false_type, env)
            } else {
                // Can't determine — return unevaluated
                TypeExpr::Conditional {
                    check: Box::new(check_eval),
                    extends: Box::new(extends_eval),
                    true_type: Box::new(evaluate(true_type, env)),
                    false_type: Box::new(evaluate(false_type, env)),
                }
            }
        }

        // { [K in Source]: Value }
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => evaluate_mapped(
            parameter, source, value, *optional, *readonly, name_type, env,
        ),

        // Template literal
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => evaluate_template_literal(quasis, expressions, env),

        // infer T — can't evaluate outside conditional context
        TypeExpr::Infer { .. } => expr.clone(),

        // Rest — evaluate inner
        TypeExpr::Rest(inner) => TypeExpr::Rest(Box::new(evaluate(inner, env))),
    }
}

// ---------------------------------------------------------------------------
// Reference resolution
// ---------------------------------------------------------------------------

fn evaluate_ref(name: &str, type_arguments: &[TypeExpr], env: &mut EvalEnv) -> TypeExpr {
    // Check type bindings first (generic parameters)
    if type_arguments.is_empty() {
        if let Some(bound) = env.type_bindings.get(name).cloned() {
            return evaluate(&bound, env);
        }
    }

    // Try built-in utility types
    if let Some(result) = try_builtin_utility(name, type_arguments, env) {
        return result;
    }

    // Look up in type symbol table
    if let Some(decl) = env.type_symbols.get(name).cloned() {
        // Cycle detection
        if env.active.contains(name) {
            return TypeExpr::named(name);
        }
        env.active.insert(name.to_string());

        let result = if !decl.type_parameters.is_empty() {
            // Generic instantiation (uses defaults for missing args)
            let evaluated_args: Vec<TypeExpr> =
                type_arguments.iter().map(|a| evaluate(a, env)).collect();
            instantiate_generic(&decl, &evaluated_args, env)
        } else {
            evaluate(&decl.body, env)
        };

        env.active.remove(name);
        return result;
    }

    // Unresolved — return as-is with evaluated type arguments
    let evaluated_args: Vec<TypeExpr> = type_arguments.iter().map(|a| evaluate(a, env)).collect();
    if evaluated_args.is_empty() {
        TypeExpr::named(name)
    } else {
        TypeExpr::named_with_args(name, evaluated_args)
    }
}

fn instantiate_generic(decl: &TypeDeclInfo, args: &[TypeExpr], env: &mut EvalEnv) -> TypeExpr {
    // Save current bindings
    let saved: Vec<(String, Option<TypeExpr>)> = decl
        .type_parameters
        .iter()
        .map(|p| (p.name.clone(), env.type_bindings.get(&p.name).cloned()))
        .collect();

    // Bind type parameters to arguments
    for (i, param) in decl.type_parameters.iter().enumerate() {
        let arg = if i < args.len() {
            args[i].clone()
        } else if let Some(ref default) = param.default {
            *default.clone()
        } else {
            TypeExpr::Primitive(PrimitiveName::Any)
        };
        env.type_bindings.insert(param.name.clone(), arg);
    }

    let result = evaluate(&decl.body, env);

    // Restore bindings
    for (name, prev) in saved {
        if let Some(prev) = prev {
            env.type_bindings.insert(name, prev);
        } else {
            env.type_bindings.remove(&name);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Built-in utility types
// ---------------------------------------------------------------------------

fn try_builtin_utility(
    name: &str,
    type_arguments: &[TypeExpr],
    env: &mut EvalEnv,
) -> Option<TypeExpr> {
    match name {
        "Partial" if type_arguments.len() == 1 => {
            let inner = evaluate(&type_arguments[0], env);
            Some(apply_partial(&inner))
        }
        "Required" if type_arguments.len() == 1 => {
            let inner = evaluate(&type_arguments[0], env);
            Some(apply_required(&inner))
        }
        "Readonly" if type_arguments.len() == 1 => {
            let inner = evaluate(&type_arguments[0], env);
            Some(apply_readonly(&inner))
        }
        "Pick" if type_arguments.len() == 2 => {
            let inner = evaluate(&type_arguments[0], env);
            let keys = evaluate(&type_arguments[1], env);
            Some(apply_pick(&inner, &keys))
        }
        "Omit" if type_arguments.len() == 2 => {
            let inner = evaluate(&type_arguments[0], env);
            let keys = evaluate(&type_arguments[1], env);
            Some(apply_omit(&inner, &keys))
        }
        "Record" if type_arguments.len() == 2 => {
            let keys = evaluate(&type_arguments[0], env);
            let value = evaluate(&type_arguments[1], env);
            Some(apply_record(&keys, &value))
        }
        "Extract" if type_arguments.len() == 2 => {
            let source = evaluate(&type_arguments[0], env);
            let target = evaluate(&type_arguments[1], env);
            Some(apply_extract(&source, &target))
        }
        "Exclude" if type_arguments.len() == 2 => {
            let source = evaluate(&type_arguments[0], env);
            let target = evaluate(&type_arguments[1], env);
            Some(apply_exclude(&source, &target))
        }
        "NonNullable" if type_arguments.len() == 1 => {
            let inner = evaluate(&type_arguments[0], env);
            Some(apply_non_nullable(&inner))
        }
        "ReturnType" if type_arguments.len() == 1 => {
            let inner = evaluate(&type_arguments[0], env);
            Some(extract_return_type(&inner, env))
        }
        "Parameters" if type_arguments.len() == 1 => {
            let inner = evaluate(&type_arguments[0], env);
            Some(extract_parameters(&inner, env))
        }
        "ConstructorParameters" if type_arguments.len() == 1 => {
            let inner = evaluate(&type_arguments[0], env);
            Some(extract_constructor_parameters(&inner, env))
        }
        "InstanceType" if type_arguments.len() == 1 => {
            let inner = evaluate(&type_arguments[0], env);
            Some(extract_instance_type(&inner, env))
        }
        "Awaited" if type_arguments.len() == 1 => {
            let inner = evaluate(&type_arguments[0], env);
            Some(unwrap_awaited(&inner, env))
        }
        _ => None,
    }
}

// -- Partial<T> --

fn apply_partial(ty: &TypeExpr) -> TypeExpr {
    match ty {
        TypeExpr::Object(obj) => {
            let properties = obj
                .properties
                .iter()
                .map(|m| match m {
                    ObjectMember::Property(p) => ObjectMember::Property(ObjectProperty {
                        optional: true,
                        ..p.clone()
                    }),
                    other => other.clone(),
                })
                .collect();
            TypeExpr::Object(ObjectExpr { properties })
        }
        _ => TypeExpr::named_with_args("Partial", vec![ty.clone()]),
    }
}

// -- Required<T> --

fn apply_required(ty: &TypeExpr) -> TypeExpr {
    match ty {
        TypeExpr::Object(obj) => {
            let properties = obj
                .properties
                .iter()
                .map(|m| match m {
                    ObjectMember::Property(p) => ObjectMember::Property(ObjectProperty {
                        optional: false,
                        ..p.clone()
                    }),
                    other => other.clone(),
                })
                .collect();
            TypeExpr::Object(ObjectExpr { properties })
        }
        _ => TypeExpr::named_with_args("Required", vec![ty.clone()]),
    }
}

// -- Readonly<T> --

fn apply_readonly(ty: &TypeExpr) -> TypeExpr {
    match ty {
        TypeExpr::Object(obj) => {
            let properties = obj
                .properties
                .iter()
                .map(|m| match m {
                    ObjectMember::Property(p) => ObjectMember::Property(ObjectProperty {
                        readonly: true,
                        ..p.clone()
                    }),
                    other => other.clone(),
                })
                .collect();
            TypeExpr::Object(ObjectExpr { properties })
        }
        _ => TypeExpr::named_with_args("Readonly", vec![ty.clone()]),
    }
}

// -- Pick<T, K> --

fn apply_pick(ty: &TypeExpr, keys: &TypeExpr) -> TypeExpr {
    let key_set = extract_string_keys(keys);
    if key_set.is_empty() {
        return TypeExpr::named_with_args("Pick", vec![ty.clone(), keys.clone()]);
    }

    match ty {
        TypeExpr::Object(obj) => {
            let properties = obj
                .properties
                .iter()
                .filter(|m| match m {
                    ObjectMember::Property(p) => key_set.contains(&p.name),
                    _ => false,
                })
                .cloned()
                .collect();
            TypeExpr::Object(ObjectExpr { properties })
        }
        _ => TypeExpr::named_with_args("Pick", vec![ty.clone(), keys.clone()]),
    }
}

// -- Omit<T, K> --

fn apply_omit(ty: &TypeExpr, keys: &TypeExpr) -> TypeExpr {
    let key_set = extract_string_keys(keys);
    if key_set.is_empty() {
        return TypeExpr::named_with_args("Omit", vec![ty.clone(), keys.clone()]);
    }

    match ty {
        TypeExpr::Object(obj) => {
            let properties = obj
                .properties
                .iter()
                .filter(|m| match m {
                    ObjectMember::Property(p) => !key_set.contains(&p.name),
                    _ => true, // Keep index signatures, methods, etc.
                })
                .cloned()
                .collect();
            TypeExpr::Object(ObjectExpr { properties })
        }
        _ => TypeExpr::named_with_args("Omit", vec![ty.clone(), keys.clone()]),
    }
}

// -- Record<K, V> --

fn apply_record(keys: &TypeExpr, value: &TypeExpr) -> TypeExpr {
    let key_set = extract_string_keys(keys);
    if !key_set.is_empty() {
        // Finite key set → object with named properties
        let properties = key_set
            .into_iter()
            .map(|k| {
                ObjectMember::Property(ObjectProperty {
                    name: k,
                    ty: value.clone(),
                    optional: false,
                    readonly: false,
                })
            })
            .collect();
        TypeExpr::Object(ObjectExpr { properties })
    } else if matches!(
        keys,
        TypeExpr::Primitive(PrimitiveName::String | PrimitiveName::Number)
    ) {
        // Index signature
        TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::IndexSignature(IndexSignature {
                key_name: "key".to_string(),
                key_type: keys.clone(),
                value_type: value.clone(),
                readonly: false,
            })],
        })
    } else {
        TypeExpr::named_with_args("Record", vec![keys.clone(), value.clone()])
    }
}

// -- Extract<T, U> --

fn apply_extract(source: &TypeExpr, target: &TypeExpr) -> TypeExpr {
    if let TypeExpr::Union(types) = source {
        let filtered: Vec<TypeExpr> = types
            .iter()
            .filter(|t| is_assignable_to(t, target))
            .cloned()
            .collect();
        if filtered.is_empty() {
            TypeExpr::Primitive(PrimitiveName::Never)
        } else {
            TypeExpr::union(filtered)
        }
    } else if is_assignable_to(source, target) {
        source.clone()
    } else {
        TypeExpr::Primitive(PrimitiveName::Never)
    }
}

// -- Exclude<T, U> --

fn apply_exclude(source: &TypeExpr, target: &TypeExpr) -> TypeExpr {
    if let TypeExpr::Union(types) = source {
        let filtered: Vec<TypeExpr> = types
            .iter()
            .filter(|t| !is_assignable_to(t, target))
            .cloned()
            .collect();
        if filtered.is_empty() {
            TypeExpr::Primitive(PrimitiveName::Never)
        } else {
            TypeExpr::union(filtered)
        }
    } else if is_assignable_to(source, target) {
        TypeExpr::Primitive(PrimitiveName::Never)
    } else {
        source.clone()
    }
}

// -- NonNullable<T> --

fn apply_non_nullable(ty: &TypeExpr) -> TypeExpr {
    match ty {
        TypeExpr::Union(types) => {
            let filtered: Vec<TypeExpr> = types
                .iter()
                .filter(|t| {
                    !matches!(
                        t,
                        TypeExpr::Primitive(PrimitiveName::Null | PrimitiveName::Undefined)
                    )
                })
                .cloned()
                .collect();
            if filtered.is_empty() {
                TypeExpr::Primitive(PrimitiveName::Never)
            } else {
                TypeExpr::union(filtered)
            }
        }
        TypeExpr::Primitive(PrimitiveName::Null | PrimitiveName::Undefined) => {
            TypeExpr::Primitive(PrimitiveName::Never)
        }
        _ => ty.clone(),
    }
}

// -- ReturnType<T> --

fn extract_return_type(ty: &TypeExpr, env: &mut EvalEnv) -> TypeExpr {
    match ty {
        TypeExpr::Function(func) => {
            if let Some(ref ret) = func.return_type {
                evaluate(ret, env)
            } else {
                TypeExpr::Primitive(PrimitiveName::Void)
            }
        }
        // typeof fn → look up the function in value symbols
        TypeExpr::TypeOf(value_ref) => {
            if let Some(resolved) = resolve_typeof(value_ref, env) {
                extract_return_type(&resolved, env)
            } else {
                TypeExpr::named_with_args("ReturnType", vec![ty.clone()])
            }
        }
        _ => TypeExpr::named_with_args("ReturnType", vec![ty.clone()]),
    }
}

// -- Parameters<T> --

fn extract_parameters(ty: &TypeExpr, env: &mut EvalEnv) -> TypeExpr {
    match ty {
        TypeExpr::Function(func) => {
            let elements = func
                .parameters
                .iter()
                .map(|p| TupleElement {
                    label: p.name.clone(),
                    ty: evaluate(&p.ty, env),
                    optional: p.optional,
                    rest: p.rest,
                })
                .collect();
            TypeExpr::Tuple {
                elements,
                readonly: false,
            }
        }
        TypeExpr::TypeOf(value_ref) => {
            if let Some(resolved) = resolve_typeof(value_ref, env) {
                extract_parameters(&resolved, env)
            } else {
                TypeExpr::named_with_args("Parameters", vec![ty.clone()])
            }
        }
        _ => TypeExpr::named_with_args("Parameters", vec![ty.clone()]),
    }
}

// -- ConstructorParameters<T> --

fn extract_constructor_parameters(ty: &TypeExpr, env: &mut EvalEnv) -> TypeExpr {
    match ty {
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                if let ObjectMember::ConstructSignature(func) = member {
                    let elements = func
                        .parameters
                        .iter()
                        .map(|p| TupleElement {
                            label: p.name.clone(),
                            ty: evaluate(&p.ty, env),
                            optional: p.optional,
                            rest: p.rest,
                        })
                        .collect();
                    return TypeExpr::Tuple {
                        elements,
                        readonly: false,
                    };
                }
            }
            TypeExpr::named_with_args("ConstructorParameters", vec![ty.clone()])
        }
        _ => TypeExpr::named_with_args("ConstructorParameters", vec![ty.clone()]),
    }
}

// -- InstanceType<T> --

fn extract_instance_type(ty: &TypeExpr, env: &mut EvalEnv) -> TypeExpr {
    match ty {
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                if let ObjectMember::ConstructSignature(func) = member {
                    if let Some(ref ret) = func.return_type {
                        return evaluate(ret, env);
                    }
                }
            }
            TypeExpr::named_with_args("InstanceType", vec![ty.clone()])
        }
        _ => TypeExpr::named_with_args("InstanceType", vec![ty.clone()]),
    }
}

// -- Awaited<T> --

fn unwrap_awaited(ty: &TypeExpr, env: &mut EvalEnv) -> TypeExpr {
    match ty {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if name == "Promise" && type_arguments.len() == 1 => {
            let inner = evaluate(&type_arguments[0], env);
            // Recursively unwrap nested Promises
            unwrap_awaited(&inner, env)
        }
        _ => ty.clone(),
    }
}

// ---------------------------------------------------------------------------
// keyof
// ---------------------------------------------------------------------------

fn evaluate_keyof(ty: &TypeExpr) -> TypeExpr {
    match ty {
        TypeExpr::Object(obj) => {
            let keys: Vec<TypeExpr> = obj
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(TypeExpr::string_literal(&p.name)),
                    ObjectMember::Method(m) => Some(TypeExpr::string_literal(&m.name)),
                    ObjectMember::IndexSignature(idx) => Some(idx.key_type.clone()),
                    _ => None,
                })
                .collect();
            if keys.is_empty() {
                TypeExpr::Primitive(PrimitiveName::Never)
            } else {
                TypeExpr::union(keys)
            }
        }
        _ => TypeExpr::KeyOf(Box::new(ty.clone())),
    }
}

// ---------------------------------------------------------------------------
// typeof
// ---------------------------------------------------------------------------

fn evaluate_typeof(value_ref: &ValueRef, env: &mut EvalEnv) -> TypeExpr {
    if let Some(resolved) = resolve_typeof(value_ref, env) {
        resolved
    } else {
        TypeExpr::TypeOf(value_ref.clone())
    }
}

fn resolve_typeof(value_ref: &ValueRef, env: &mut EvalEnv) -> Option<TypeExpr> {
    if value_ref.path.len() != 1 {
        return None; // Qualified paths not yet supported
    }
    let name = &value_ref.path[0];
    let decl = env.value_symbols.get(name)?.clone();

    // If there's an explicit type annotation, use it
    if let Some(ref ty) = decl.type_annotation {
        return Some(evaluate(ty, env));
    }

    // Classes need their constructor shape preserved for utilities like
    // ConstructorParameters<typeof C> and InstanceType<typeof C>.
    if decl.kind == ValueDeclKind::Class {
        if let Some(ref shape) = decl.object_shape {
            return Some(TypeExpr::Object(shape.clone()));
        }
    }

    // If it's a function, synthesize a function type
    if let Some(ref sig) = decl.function_signature {
        return Some(TypeExpr::Function(FunctionExpr {
            parameters: sig.parameters.clone(),
            return_type: sig.return_type.clone().map(Box::new),
            type_parameters: sig.type_parameters.clone(),
        }));
    }

    // If it's an object literal, use the shape
    if let Some(ref shape) = decl.object_shape {
        return Some(TypeExpr::Object(shape.clone()));
    }

    None
}

// ---------------------------------------------------------------------------
// Indexed access: T[K]
// ---------------------------------------------------------------------------

fn evaluate_indexed_access(object: &TypeExpr, index: &TypeExpr) -> TypeExpr {
    match (object, index) {
        (TypeExpr::Object(obj), TypeExpr::Literal(LiteralValue::String(key))) => {
            // Look up named property
            for member in &obj.properties {
                if let ObjectMember::Property(p) = member {
                    if p.name == *key {
                        return p.ty.clone();
                    }
                }
                if let ObjectMember::Method(m) = member {
                    if m.name == *key {
                        return TypeExpr::Function(m.function.clone());
                    }
                }
            }
            // Check index signatures
            for member in &obj.properties {
                if let ObjectMember::IndexSignature(idx) = member {
                    if matches!(idx.key_type, TypeExpr::Primitive(PrimitiveName::String)) {
                        return idx.value_type.clone();
                    }
                }
            }
            TypeExpr::Primitive(PrimitiveName::Undefined)
        }
        (TypeExpr::Object(obj), TypeExpr::Primitive(PrimitiveName::Number)) => {
            // Number index → check for number index signature
            for member in &obj.properties {
                if let ObjectMember::IndexSignature(idx) = member {
                    if matches!(idx.key_type, TypeExpr::Primitive(PrimitiveName::Number)) {
                        return idx.value_type.clone();
                    }
                }
            }
            TypeExpr::IndexedAccess {
                object: Box::new(object.clone()),
                index: Box::new(index.clone()),
            }
        }
        (TypeExpr::Array { element, .. }, TypeExpr::Primitive(PrimitiveName::Number)) => {
            // T[][number] → T
            *element.clone()
        }
        (TypeExpr::Tuple { elements, .. }, TypeExpr::Literal(LiteralValue::Number(n))) => {
            // [A, B, C][0] → A
            if n.fract() != 0.0 || *n < 0.0 || !n.is_finite() {
                return TypeExpr::IndexedAccess {
                    object: Box::new(object.clone()),
                    index: Box::new(index.clone()),
                };
            }
            let idx = *n as usize;
            if idx < elements.len() {
                elements[idx].ty.clone()
            } else {
                TypeExpr::Primitive(PrimitiveName::Undefined)
            }
        }
        (_, TypeExpr::Union(keys)) => {
            // T["a" | "b"] → T["a"] | T["b"]
            let results: Vec<TypeExpr> = keys
                .iter()
                .map(|k| evaluate_indexed_access(object, k))
                .collect();
            TypeExpr::union(results)
        }
        _ => TypeExpr::IndexedAccess {
            object: Box::new(object.clone()),
            index: Box::new(index.clone()),
        },
    }
}

// ---------------------------------------------------------------------------
// Mapped types
// ---------------------------------------------------------------------------

fn evaluate_mapped(
    parameter: &str,
    source: &TypeExpr,
    value: &TypeExpr,
    optional: MappedModifier,
    readonly: MappedModifier,
    name_type: &Option<Box<TypeExpr>>,
    env: &mut EvalEnv,
) -> TypeExpr {
    let resolved_source = evaluate(source, env);
    let keys = extract_string_keys(&resolved_source);

    if keys.is_empty() || keys.len() > env.limits.max_mapped_keys {
        return TypeExpr::Mapped {
            parameter: parameter.to_string(),
            source: Box::new(resolved_source),
            value: Box::new(value.clone()),
            optional,
            readonly,
            name_type: name_type.clone(),
        };
    }

    // Save and bind the parameter
    let saved = env.type_bindings.get(parameter).cloned();

    // Build lookup of source property modifiers for MappedModifier::None preservation
    let source_props = extract_source_property_modifiers(&resolved_source, env);

    let properties: Vec<ObjectMember> = keys
        .into_iter()
        .map(|key| {
            env.type_bindings
                .insert(parameter.to_string(), TypeExpr::string_literal(&key));
            let prop_type = evaluate(value, env);
            let src = source_props.get(key.as_str());
            let is_optional = match optional {
                MappedModifier::Add => true,
                MappedModifier::Remove => false,
                MappedModifier::None => src.is_some_and(|s| s.0),
            };
            let is_readonly = match readonly {
                MappedModifier::Add => true,
                MappedModifier::Remove => false,
                MappedModifier::None => src.is_some_and(|s| s.1),
            };
            ObjectMember::Property(ObjectProperty {
                name: key,
                ty: prop_type,
                optional: is_optional,
                readonly: is_readonly,
            })
        })
        .collect();

    // Restore binding
    if let Some(prev) = saved {
        env.type_bindings.insert(parameter.to_string(), prev);
    } else {
        env.type_bindings.remove(parameter);
    }

    TypeExpr::Object(ObjectExpr { properties })
}

// ---------------------------------------------------------------------------
// Template literal types
// ---------------------------------------------------------------------------

fn evaluate_template_literal(
    quasis: &[String],
    expressions: &[TypeExpr],
    env: &mut EvalEnv,
) -> TypeExpr {
    let evaluated_exprs: Vec<TypeExpr> = expressions.iter().map(|e| evaluate(e, env)).collect();

    // Check if all expressions are literal or finite union of literals
    let all_finite = evaluated_exprs.iter().all(is_finite_string_set);
    if !all_finite {
        // If any expression has infinite domain (e.g., `string`, `number`),
        // degrade to primitive string
        return TypeExpr::Primitive(PrimitiveName::String);
    }

    // Expand all combinations
    let mut results = vec![quasis[0].clone()];
    for (i, expr) in evaluated_exprs.iter().enumerate() {
        let suffix = &quasis[i + 1];
        let values = extract_literal_strings(expr);
        let mut new_results = Vec::new();
        for prefix in &results {
            for val in &values {
                new_results.push(format!("{prefix}{val}{suffix}"));
            }
        }
        if new_results.len() > env.limits.max_union_expansion {
            return TypeExpr::Primitive(PrimitiveName::String);
        }
        results = new_results;
    }

    if results.len() == 1 {
        TypeExpr::string_literal(&results[0])
    } else {
        TypeExpr::union(results.into_iter().map(TypeExpr::string_literal).collect())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn evaluate_object_member(member: &ObjectMember, env: &mut EvalEnv) -> ObjectMember {
    match member {
        ObjectMember::Property(p) => ObjectMember::Property(ObjectProperty {
            name: p.name.clone(),
            ty: evaluate(&p.ty, env),
            optional: p.optional,
            readonly: p.readonly,
        }),
        ObjectMember::IndexSignature(idx) => ObjectMember::IndexSignature(IndexSignature {
            key_name: idx.key_name.clone(),
            key_type: evaluate(&idx.key_type, env),
            value_type: evaluate(&idx.value_type, env),
            readonly: idx.readonly,
        }),
        ObjectMember::CallSignature(func) => {
            ObjectMember::CallSignature(evaluate_function(func, env))
        }
        ObjectMember::ConstructSignature(func) => {
            ObjectMember::ConstructSignature(evaluate_function(func, env))
        }
        ObjectMember::Method(m) => ObjectMember::Method(MethodSignature {
            name: m.name.clone(),
            function: evaluate_function(&m.function, env),
            optional: m.optional,
        }),
    }
}

/// Extract (optional, readonly) modifiers for each named property from a source type.
/// Used by mapped type evaluation to preserve modifiers when MappedModifier::None.
fn extract_source_property_modifiers(
    source: &TypeExpr,
    env: &mut EvalEnv,
) -> rustc_hash::FxHashMap<String, (bool, bool)> {
    let mut result = rustc_hash::FxHashMap::default();
    // If the source is keyof T, resolve T to get its properties
    let obj = match source {
        TypeExpr::KeyOf(inner) => evaluate(inner, env),
        _ => return result,
    };
    if let TypeExpr::Object(obj_expr) = &obj {
        for member in &obj_expr.properties {
            if let ObjectMember::Property(p) = member {
                result.insert(p.name.clone(), (p.optional, p.readonly));
            }
        }
    }
    result
}

/// Merge an intersection of types. If all branches are objects, merge their
/// properties into a single object. Otherwise, return an intersection.
fn merge_intersection(types: Vec<TypeExpr>) -> TypeExpr {
    if types.len() == 1 {
        return types.into_iter().next().unwrap();
    }
    // Check if all branches are objects — if so, merge
    let all_objects = types.iter().all(|t| matches!(t, TypeExpr::Object(_)));
    if all_objects {
        let mut merged = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for t in types {
            if let TypeExpr::Object(obj) = t {
                for member in obj.properties {
                    if let ObjectMember::Property(ref p) = member {
                        // First definition wins — later duplicates are skipped.
                        // For `Base & Child`, Base properties take precedence.
                        if seen.insert(p.name.clone()) {
                            merged.push(member);
                        }
                    } else {
                        merged.push(member);
                    }
                }
            }
        }
        TypeExpr::Object(ObjectExpr { properties: merged })
    } else {
        TypeExpr::Intersection(types)
    }
}

fn evaluate_function(func: &FunctionExpr, env: &mut EvalEnv) -> FunctionExpr {
    FunctionExpr {
        parameters: func
            .parameters
            .iter()
            .map(|p| FunctionParam {
                name: p.name.clone(),
                ty: evaluate(&p.ty, env),
                optional: p.optional,
                rest: p.rest,
            })
            .collect(),
        return_type: func
            .return_type
            .as_ref()
            .map(|r| Box::new(evaluate(r, env))),
        type_parameters: func.type_parameters.clone(),
    }
}

/// Extract string literal keys from a type expression.
fn extract_string_keys(ty: &TypeExpr) -> Vec<String> {
    match ty {
        TypeExpr::Literal(LiteralValue::String(s)) => vec![s.clone()],
        TypeExpr::Union(types) => {
            let mut keys = Vec::new();
            for t in types {
                match t {
                    TypeExpr::Literal(LiteralValue::String(s)) => keys.push(s.clone()),
                    _ => return Vec::new(), // Non-literal in union → give up
                }
            }
            keys
        }
        _ => Vec::new(),
    }
}

/// Check if a type is a finite set of string literals (for template literal expansion).
fn is_finite_string_set(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Literal(LiteralValue::String(_)) => true,
        TypeExpr::Literal(LiteralValue::Number(_)) => true,
        TypeExpr::Literal(LiteralValue::Boolean(_)) => true,
        TypeExpr::Union(types) => types.iter().all(is_finite_string_set),
        _ => false,
    }
}

/// Extract string representations from literal types (for template expansion).
fn extract_literal_strings(ty: &TypeExpr) -> Vec<String> {
    match ty {
        TypeExpr::Literal(LiteralValue::String(s)) => vec![s.clone()],
        TypeExpr::Literal(LiteralValue::Number(n)) => {
            if n.fract() == 0.0 && n.is_finite() && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                vec![format!("{}", *n as i64)]
            } else {
                vec![n.to_string()]
            }
        }
        TypeExpr::Literal(LiteralValue::Boolean(b)) => vec![b.to_string()],
        TypeExpr::Union(types) => types.iter().flat_map(extract_literal_strings).collect(),
        _ => vec![],
    }
}

/// Simple assignability check for conditional type evaluation.
/// Returns true when we can definitively prove T extends U.
fn is_assignable_to(check: &TypeExpr, target: &TypeExpr) -> bool {
    // Same type
    if check == target {
        return true;
    }

    match (check, target) {
        // any extends anything
        (TypeExpr::Primitive(PrimitiveName::Any), _) => true,
        // never extends anything
        (TypeExpr::Primitive(PrimitiveName::Never), _) => true,
        // anything extends any
        (_, TypeExpr::Primitive(PrimitiveName::Any)) => true,
        // anything extends unknown
        (_, TypeExpr::Primitive(PrimitiveName::Unknown)) => true,
        // T extends A | B — if T equals any member of the union
        (_, TypeExpr::Union(targets)) => targets.iter().any(|t| is_assignable_to(check, t)),
        // string literal extends string
        (
            TypeExpr::Literal(LiteralValue::String(_)),
            TypeExpr::Primitive(PrimitiveName::String),
        ) => true,
        // number literal extends number
        (
            TypeExpr::Literal(LiteralValue::Number(_)),
            TypeExpr::Primitive(PrimitiveName::Number),
        ) => true,
        // boolean literal extends boolean
        (
            TypeExpr::Literal(LiteralValue::Boolean(_)),
            TypeExpr::Primitive(PrimitiveName::Boolean),
        ) => true,
        _ => false,
    }
}

/// Check if T is definitely NOT assignable to U.
fn is_definitely_not_assignable(check: &TypeExpr, target: &TypeExpr) -> bool {
    match (check, target) {
        // Distinct primitives
        (TypeExpr::Primitive(a), TypeExpr::Primitive(b)) => {
            // never and any are special
            if matches!(a, PrimitiveName::Never | PrimitiveName::Any)
                || matches!(b, PrimitiveName::Any | PrimitiveName::Unknown)
            {
                return false;
            }
            a != b
        }
        // String literal can't extend number
        (
            TypeExpr::Literal(LiteralValue::String(_)),
            TypeExpr::Primitive(PrimitiveName::Number),
        ) => true,
        (
            TypeExpr::Literal(LiteralValue::Number(_)),
            TypeExpr::Primitive(PrimitiveName::String),
        ) => true,
        _ => false,
    }
}
