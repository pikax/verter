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

pub type DeclarationId = u64;

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
#[derive(Debug, Clone)]
pub struct EvalEnv {
    /// Type declarations: interfaces, type aliases.
    pub type_symbols: FxHashMap<String, TypeDeclInfo>,
    /// Value declarations: functions, constants, classes.
    pub value_symbols: FxHashMap<String, ValueDeclInfo>,
    /// Memoized evaluations for non-generic type references.
    resolved_refs: FxHashMap<RefCacheKey, TypeExpr>,
    /// Stable ids assigned to type declarations inserted into this environment.
    type_decl_ids: FxHashMap<String, DeclarationId>,
    /// Stable ids assigned to value declarations inserted into this environment.
    value_decl_ids: FxHashMap<String, DeclarationId>,
    /// Generic type parameter bindings for the current instantiation.
    pub type_bindings: FxHashMap<String, TypeExpr>,
    /// Currently being evaluated (cycle detection).
    pub(crate) active: FxHashSet<String>,
    /// Evaluation limits.
    pub limits: EvalLimits,
    /// Current recursion depth.
    depth: usize,
    /// Current mapped type nesting depth.
    mapped_depth: usize,
    /// Total evaluation steps consumed (monotonically increasing).
    steps: usize,
    /// Monotonic declaration ordinal used to assign stable ids.
    next_declaration_id: DeclarationId,
}

/// Configurable limits for the evaluator.
#[derive(Debug, Clone)]
pub struct EvalLimits {
    pub max_depth: usize,
    pub max_union_expansion: usize,
    pub max_mapped_keys: usize,
    /// Maximum nested `evaluate_mapped()` calls. Default: 3.
    pub max_mapped_depth: usize,
    /// Safety-net total step limit. Default: 50_000.
    pub max_steps: usize,
}

impl Default for EvalLimits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_union_expansion: 64,
            max_mapped_keys: 128,
            max_mapped_depth: 3,
            max_steps: 50_000,
        }
    }
}

impl EvalEnv {
    /// Create a new evaluation environment with default limits.
    pub fn new() -> Self {
        Self {
            type_symbols: FxHashMap::default(),
            value_symbols: FxHashMap::default(),
            resolved_refs: FxHashMap::default(),
            type_decl_ids: FxHashMap::default(),
            value_decl_ids: FxHashMap::default(),
            type_bindings: FxHashMap::default(),
            active: FxHashSet::default(),
            limits: EvalLimits::default(),
            depth: 0,
            mapped_depth: 0,
            steps: 0,
            next_declaration_id: 0,
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
        let name = decl.name.clone();
        self.type_symbols.insert(name.clone(), decl);
        if !self.type_decl_ids.contains_key(&name) {
            let decl_id = self.allocate_declaration_id();
            self.type_decl_ids.insert(name, decl_id);
        }
    }

    /// Register a value declaration.
    pub fn add_value(&mut self, decl: ValueDeclInfo) {
        let name = decl.name.clone();
        self.value_symbols.insert(name.clone(), decl);
        if !self.value_decl_ids.contains_key(&name) {
            let decl_id = self.allocate_declaration_id();
            self.value_decl_ids.insert(name, decl_id);
        }
    }

    /// Returns the total number of evaluation steps consumed so far.
    pub fn steps(&self) -> usize {
        self.steps
    }

    /// Returns whether the step budget has been exhausted.
    pub fn budget_exhausted(&self) -> bool {
        self.steps >= self.limits.max_steps
    }

    /// Configure limits from an `ExpansionBudget`.
    pub fn apply_expansion_budget(&mut self, budget: &crate::type_expand::ExpansionBudget) {
        self.limits.max_depth = budget.max_depth;
        self.limits.max_union_expansion = budget.max_union_expansion;
        self.limits.max_mapped_keys = budget.max_mapped_keys;
        self.limits.max_mapped_depth = budget.max_mapped_depth;
        self.limits.max_steps = budget.max_symbolic_work;
        self.depth = 0;
        self.mapped_depth = 0;
        self.steps = 0;
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
        for (name, decl_id) in other.type_decl_ids {
            self.type_decl_ids.entry(name).or_insert(decl_id);
        }
        for (name, decl_id) in other.value_decl_ids {
            self.value_decl_ids.entry(name).or_insert(decl_id);
        }
        self.next_declaration_id = self.next_declaration_id.max(other.next_declaration_id);
    }

    pub fn type_declaration_id(&self, name: &str) -> Option<DeclarationId> {
        self.type_decl_ids.get(name).copied()
    }

    pub fn value_declaration_id(&self, name: &str) -> Option<DeclarationId> {
        self.value_decl_ids.get(name).copied()
    }

    fn allocate_declaration_id(&mut self) -> DeclarationId {
        self.next_declaration_id += 1;
        self.next_declaration_id
    }
}

impl Default for EvalEnv {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RefCacheKey {
    name: String,
    args: Vec<TypeExpr>,
}

fn ref_cache_key(name: &str, args: &[TypeExpr]) -> RefCacheKey {
    RefCacheKey {
        name: name.to_string(),
        args: args.to_vec(),
    }
}

fn is_builtin_utility_name(name: &str) -> bool {
    matches!(
        name,
        "Partial"
            | "Required"
            | "Readonly"
            | "Pick"
            | "Omit"
            | "Record"
            | "Extract"
            | "Exclude"
            | "NonNullable"
            | "ReturnType"
            | "Parameters"
            | "ConstructorParameters"
            | "InstanceType"
            | "Awaited"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinUtilitySource {
    Builtin,
    Shadowed,
    Unknown,
}

pub trait EvalLookup {
    fn resolve_type_decl(&mut self, _name: &str) -> Option<TypeDeclInfo> {
        None
    }

    fn resolve_value_decl(&mut self, _path: &[String]) -> Option<ValueDeclInfo> {
        None
    }

    fn resolve_member_projection(&mut self, _object: &TypeExpr, _key: &str) -> Option<TypeExpr> {
        None
    }

    fn utility_source(&mut self, name: &str) -> BuiltinUtilitySource {
        if is_builtin_utility_name(name) {
            BuiltinUtilitySource::Builtin
        } else {
            BuiltinUtilitySource::Unknown
        }
    }
}

#[derive(Debug, Default)]
pub struct NoopEvalLookup;

impl EvalLookup for NoopEvalLookup {}

// ---------------------------------------------------------------------------
// Evaluator
// ---------------------------------------------------------------------------

/// Evaluate a `TypeExpr` in the given environment, reducing references
/// and applying utility type semantics.
///
/// Returns a normalized `TypeExpr` with references resolved where possible.
pub fn evaluate(expr: &TypeExpr, env: &mut EvalEnv) -> TypeExpr {
    let mut lookup = NoopEvalLookup;
    evaluate_with_lookup(expr, env, &mut lookup)
}

/// Evaluate a `TypeExpr` with a synchronous external lookup adapter.
pub fn evaluate_with_lookup(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> TypeExpr {
    if env.depth > env.limits.max_depth || env.steps >= env.limits.max_steps {
        return expr.clone();
    }
    env.depth += 1;
    env.steps += 1;
    let result = evaluate_inner(expr, env, lookup);
    env.depth -= 1;
    result
}

fn evaluate_inner(expr: &TypeExpr, env: &mut EvalEnv, lookup: &mut dyn EvalLookup) -> TypeExpr {
    match expr {
        // Terminals — pass through
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) | TypeExpr::Unknown { .. } => expr.clone(),

        // Unwrap parenthesized
        TypeExpr::Parenthesized(inner) => evaluate_with_lookup(inner, env, lookup),

        // Union — evaluate each branch
        TypeExpr::Union(types) => {
            let evaluated: Vec<TypeExpr> = types
                .iter()
                .map(|t| evaluate_with_lookup(t, env, lookup))
                .collect();
            TypeExpr::union(evaluated)
        }

        // Intersection — evaluate each branch and merge objects
        TypeExpr::Intersection(types) => {
            let evaluated: Vec<TypeExpr> = types
                .iter()
                .map(|t| evaluate_with_lookup(t, env, lookup))
                .collect();
            merge_intersection(evaluated)
        }

        // Array — evaluate element
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: Box::new(evaluate_with_lookup(element, env, lookup)),
            readonly: *readonly,
        },

        // Tuple — evaluate each element
        TypeExpr::Tuple { elements, readonly } => {
            let evaluated = elements
                .iter()
                .map(|e| TupleElement {
                    label: e.label.clone(),
                    ty: evaluate_with_lookup(&e.ty, env, lookup),
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
                .map(|m| evaluate_object_member(m, env, lookup))
                .collect();
            TypeExpr::Object(ObjectExpr { properties })
        }

        // Function — evaluate param and return types
        TypeExpr::Function(func) => TypeExpr::Function(evaluate_function(func, env, lookup)),

        // Type reference — resolve
        TypeExpr::Ref {
            name,
            type_arguments,
        } => evaluate_ref(name, type_arguments, env, lookup),

        // keyof T
        TypeExpr::KeyOf(operand) => {
            let resolved = evaluate_with_lookup(operand, env, lookup);
            evaluate_keyof(&resolved)
        }

        // typeof x
        TypeExpr::TypeOf(value_ref) => evaluate_typeof(value_ref, env, lookup),

        // T[K]
        TypeExpr::IndexedAccess { object, index } => {
            let idx = evaluate_with_lookup(index, env, lookup);

            // Lazy path: if the index is a string literal, try to look up the
            // member directly without eagerly evaluating ALL members of the object.
            // This avoids evaluating expensive sibling members (e.g., ComponentConfig
            // has 4 members but we only need 'slots').
            if let TypeExpr::Literal(LiteralValue::String(key)) = &idx {
                if let Some(result) = try_lazy_member_lookup(object, key, env, lookup) {
                    return result;
                }
            }

            // Fallback: eager evaluation
            let obj = evaluate_with_lookup(object, env, lookup);
            evaluate_indexed_access(&obj, &idx)
        }

        // T extends U ? A : B
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let check_eval = evaluate_with_lookup(check, env, lookup);
            let extends_eval = evaluate_with_lookup(extends, env, lookup);
            if is_assignable_to(&check_eval, &extends_eval) {
                evaluate_with_lookup(true_type, env, lookup)
            } else if let Some(result) = try_evaluate_conditional_with_infer(
                &check_eval,
                &extends_eval,
                true_type,
                env,
                lookup,
            ) {
                result
            } else if is_definitely_not_assignable(&check_eval, &extends_eval) {
                evaluate_with_lookup(false_type, env, lookup)
            } else {
                // Can't determine — return with UNEVALUATED branches.
                // Do NOT evaluate both branches: that causes 2^N blowup on
                // nested indeterminate conditionals, and consumers that handle
                // Conditional nodes (extract_object_shape, etc.) ignore them
                // entirely, making the branch evaluation wasted work.
                TypeExpr::Conditional {
                    check: Box::new(check_eval),
                    extends: Box::new(extends_eval),
                    true_type: true_type.clone(),
                    false_type: false_type.clone(),
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
            parameter, source, value, *optional, *readonly, name_type, env, lookup,
        ),

        // Template literal
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => evaluate_template_literal(quasis, expressions, env, lookup),

        // infer T — can't evaluate outside conditional context
        TypeExpr::Infer { .. } => expr.clone(),

        // Rest — evaluate inner
        TypeExpr::Rest(inner) => TypeExpr::Rest(Box::new(evaluate_with_lookup(inner, env, lookup))),
    }
}

// ---------------------------------------------------------------------------
// Reference resolution
// ---------------------------------------------------------------------------

fn evaluate_ref(
    name: &str,
    type_arguments: &[TypeExpr],
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> TypeExpr {
    // Check type bindings first (generic parameters)
    if type_arguments.is_empty() {
        if let Some(bound) = env.type_bindings.get(name).cloned() {
            return evaluate_with_lookup(&bound, env, lookup);
        }
    }

    let local_decl = env.type_symbols.get(name).cloned();
    let external_decl = if local_decl.is_none() {
        lookup.resolve_type_decl(name)
    } else {
        None
    };
    let utility_source = if local_decl.is_none() && external_decl.is_none() {
        lookup.utility_source(name)
    } else {
        BuiltinUtilitySource::Shadowed
    };

    // Built-in utilities may be able to operate on the raw argument surface
    // and avoid eagerly evaluating expensive siblings.
    if utility_source == BuiltinUtilitySource::Builtin {
        if let Some(result) = try_builtin_utility(name, type_arguments, env, lookup) {
            return result;
        }
    }

    // Early opaque-arg bailout on the RAW arguments. This must happen before
    // eager argument evaluation, otherwise a case like
    // `ComponentConfig<typeof theme, AppConfig, ...>` still fully evaluates the
    // unrelated `AppConfig` arg before we ever get a chance to short-circuit.
    if !type_arguments.is_empty()
        && type_arguments
            .iter()
            .any(|arg| is_opaque_for_instantiation(arg, env, lookup))
    {
        return TypeExpr::named_with_args(name, type_arguments.to_vec());
    }

    let evaluated_args = if type_arguments.is_empty() {
        Vec::new()
    } else {
        type_arguments
            .iter()
            .map(|a| evaluate_with_lookup(a, env, lookup))
            .collect()
    };
    let cache_key = ref_cache_key(name, &evaluated_args);
    if let Some(cached) = env.resolved_refs.get(&cache_key).cloned() {
        return cached;
    }

    // Look up in type symbol table
    if let Some(decl) = local_decl.or(external_decl) {
        // Cycle detection
        if env.active.contains(name) {
            return TypeExpr::named(name);
        }
        env.active.insert(name.to_string());

        let result = if !decl.type_parameters.is_empty() {
            instantiate_generic(&decl, &evaluated_args, env, lookup)
        } else {
            evaluate_with_lookup(&decl.body, env, lookup)
        };

        env.active.remove(name);
        env.resolved_refs.insert(cache_key, result.clone());
        return result;
    }

    // Unresolved — return as-is with evaluated type arguments
    if evaluated_args.is_empty() {
        TypeExpr::named(name)
    } else {
        TypeExpr::named_with_args(name, evaluated_args)
    }
}

/// Check if a type argument is opaque for generic instantiation.
///
/// An argument is opaque if it contains an unresolved `typeof` — meaning
/// a runtime value import that couldn't be resolved. This is the specific
/// case that causes pathological expansion: `typeof theme` from an unresolvable
/// import gets substituted into mapped types that try `keyof typeof theme`,
/// producing unbounded work.
///
/// Regular unresolved `Ref` inputs are also treated as opaque unless they are:
/// - an in-scope generic parameter binding
/// - a declared type symbol in the current environment
/// - a built-in utility wrapper whose own arguments can still be inspected
///
/// This lets us fail fast on missing imported types or missing dependency files
/// without eagerly instantiating unrelated expensive generic arguments.
///
/// Recurses into type arguments and structural positions to catch nested opaque
/// inputs (e.g., `Pick<typeof theme, 'key'>` or `Foo<MissingType>`).
fn is_opaque_for_instantiation(
    expr: &TypeExpr,
    env: &EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> bool {
    match expr {
        // typeof X where X is not in value_symbols
        TypeExpr::TypeOf(vr) => {
            if vr.path.len() == 1 && env.value_symbols.contains_key(&vr.path[0]) {
                return false;
            }
            lookup.resolve_value_decl(&vr.path).is_none()
        }
        // Bail on unresolved references that are neither declared symbols nor
        // in-scope generic bindings. Utility wrappers recurse into their args.
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if lookup.utility_source(name) == BuiltinUtilitySource::Builtin {
                return type_arguments
                    .iter()
                    .any(|a| is_opaque_for_instantiation(a, env, lookup));
            }
            if env.type_bindings.contains_key(name.as_str())
                || env.type_symbols.contains_key(name)
                || lookup.resolve_type_decl(name).is_some()
            {
                return type_arguments
                    .iter()
                    .any(|a| is_opaque_for_instantiation(a, env, lookup));
            }
            true
        }
        // Indexed access on an opaque object
        TypeExpr::IndexedAccess { object, index } => {
            is_opaque_for_instantiation(object, env, lookup)
                || is_opaque_for_instantiation(index, env, lookup)
        }
        // KeyOf on an opaque type
        TypeExpr::KeyOf(inner) => is_opaque_for_instantiation(inner, env, lookup),
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => {
            is_opaque_for_instantiation(inner, env, lookup)
        }
        TypeExpr::Array { element, .. } => is_opaque_for_instantiation(element, env, lookup),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| is_opaque_for_instantiation(&element.ty, env, lookup)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .any(|ty| is_opaque_for_instantiation(ty, env, lookup)),
        TypeExpr::Object(obj) => obj.properties.iter().any(|member| match member {
            ObjectMember::Property(prop) => is_opaque_for_instantiation(&prop.ty, env, lookup),
            ObjectMember::IndexSignature(sig) => {
                is_opaque_for_instantiation(&sig.key_type, env, lookup)
                    || is_opaque_for_instantiation(&sig.value_type, env, lookup)
            }
            ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                is_function_opaque_for_instantiation(func, env, lookup)
            }
            ObjectMember::Method(method) => {
                is_function_opaque_for_instantiation(&method.function, env, lookup)
            }
        }),
        TypeExpr::Function(func) => is_function_opaque_for_instantiation(func, env, lookup),
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => [check, extends, true_type, false_type]
            .into_iter()
            .any(|ty| is_opaque_for_instantiation(ty, env, lookup)),
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            is_opaque_for_instantiation(source, env, lookup)
                || is_opaque_for_instantiation(value, env, lookup)
                || name_type
                    .as_deref()
                    .is_some_and(|ty| is_opaque_for_instantiation(ty, env, lookup))
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(|expr| is_opaque_for_instantiation(expr, env, lookup)),
        _ => false,
    }
}

fn is_function_opaque_for_instantiation(
    func: &FunctionExpr,
    env: &EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> bool {
    func.parameters
        .iter()
        .any(|param| is_opaque_for_instantiation(&param.ty, env, lookup))
        || func
            .return_type
            .as_deref()
            .is_some_and(|ty| is_opaque_for_instantiation(ty, env, lookup))
        || func.type_parameters.iter().any(|param| {
            param
                .constraint
                .as_deref()
                .is_some_and(|ty| is_opaque_for_instantiation(ty, env, lookup))
                || param
                    .default
                    .as_deref()
                    .is_some_and(|ty| is_opaque_for_instantiation(ty, env, lookup))
        })
}

fn instantiate_generic(
    decl: &TypeDeclInfo,
    args: &[TypeExpr],
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> TypeExpr {
    // Opaque-argument bailout: if any evaluated type argument is opaque
    // (e.g., unresolved `typeof theme`), skip body expansion and return
    // the symbolic reference with evaluated args. This prevents unbounded
    // expansion of complex generics when an argument can't be resolved.
    if args
        .iter()
        .any(|a| is_opaque_for_instantiation(a, env, lookup))
    {
        return TypeExpr::named_with_args(&decl.name, args.to_vec());
    }

    let saved = bind_type_parameters(decl, args, env);
    let result = evaluate_with_lookup(&decl.body, env, lookup);
    restore_type_parameters(saved, env);

    result
}

pub(crate) fn bind_type_parameters(
    decl: &TypeDeclInfo,
    args: &[TypeExpr],
    env: &mut EvalEnv,
) -> Vec<(String, Option<TypeExpr>)> {
    // Save current bindings
    let saved = decl
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

    saved
}

pub(crate) fn restore_type_parameters(saved: Vec<(String, Option<TypeExpr>)>, env: &mut EvalEnv) {
    for (name, prev) in saved {
        if let Some(prev) = prev {
            env.type_bindings.insert(name, prev);
        } else {
            env.type_bindings.remove(&name);
        }
    }
}

pub(crate) fn try_evaluate_conditional_with_infer(
    check: &TypeExpr,
    extends: &TypeExpr,
    true_type: &TypeExpr,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> Option<TypeExpr> {
    let mut inferred = FxHashMap::default();
    if !collect_infer_bindings(check, extends, &mut inferred) || inferred.is_empty() {
        return None;
    }

    let mut saved = Vec::with_capacity(inferred.len());
    for (name, ty) in inferred {
        saved.push((name.clone(), env.type_bindings.insert(name, ty)));
    }
    let result = evaluate_with_lookup(true_type, env, lookup);
    restore_type_parameters(saved, env);
    Some(result)
}

fn collect_infer_bindings(
    actual: &TypeExpr,
    pattern: &TypeExpr,
    inferred: &mut FxHashMap<String, TypeExpr>,
) -> bool {
    match pattern {
        TypeExpr::Infer { name } => {
            if let Some(existing) = inferred.get(name) {
                existing == actual
            } else {
                inferred.insert(name.clone(), actual.clone());
                true
            }
        }
        TypeExpr::Function(pattern_fn) => {
            let TypeExpr::Function(actual_fn) = actual else {
                return false;
            };
            if actual_fn.parameters.len() != pattern_fn.parameters.len() {
                return false;
            }
            for (actual_param, pattern_param) in
                actual_fn.parameters.iter().zip(&pattern_fn.parameters)
            {
                if !collect_infer_bindings(&actual_param.ty, &pattern_param.ty, inferred) {
                    return false;
                }
            }
            match (&actual_fn.return_type, &pattern_fn.return_type) {
                (_, None) => true,
                (Some(actual_ret), Some(pattern_ret)) => {
                    collect_infer_bindings(actual_ret, pattern_ret, inferred)
                }
                (None, Some(pattern_ret)) => matches!(
                    pattern_ret.as_ref(),
                    TypeExpr::Primitive(PrimitiveName::Void | PrimitiveName::Any)
                ),
            }
        }
        TypeExpr::Array {
            element: pattern_element,
            ..
        } => {
            let TypeExpr::Array {
                element: actual_element,
                ..
            } = actual
            else {
                return false;
            };
            collect_infer_bindings(actual_element, pattern_element, inferred)
        }
        TypeExpr::Tuple {
            elements: pattern_elements,
            ..
        } => {
            let TypeExpr::Tuple {
                elements: actual_elements,
                ..
            } = actual
            else {
                return false;
            };
            if actual_elements.len() != pattern_elements.len() {
                return false;
            }
            actual_elements
                .iter()
                .zip(pattern_elements)
                .all(|(actual_element, pattern_element)| {
                    collect_infer_bindings(&actual_element.ty, &pattern_element.ty, inferred)
                })
        }
        TypeExpr::Parenthesized(inner) => collect_infer_bindings(actual, inner, inferred),
        TypeExpr::Primitive(PrimitiveName::Any | PrimitiveName::Unknown) => true,
        _ => actual == pattern,
    }
}

#[allow(dead_code)]
pub(crate) fn with_bound_type_decl_with_lookup<R, F>(
    name: &str,
    type_arguments: &[TypeExpr],
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
    f: F,
) -> Option<R>
where
    F: FnOnce(&TypeDeclInfo, &mut EvalEnv) -> R,
{
    let decl = env
        .type_symbols
        .get(name)
        .cloned()
        .or_else(|| lookup.resolve_type_decl(name))?;
    if env.active.contains(name) {
        return None;
    }
    env.active.insert(name.to_string());
    let saved = bind_type_parameters(&decl, type_arguments, env);
    let result = f(&decl, env);
    restore_type_parameters(saved, env);
    env.active.remove(name);
    Some(result)
}

#[allow(dead_code)]
pub(crate) fn with_bound_type_decl<R, F>(
    name: &str,
    type_arguments: &[TypeExpr],
    env: &mut EvalEnv,
    f: F,
) -> Option<R>
where
    F: FnOnce(&TypeDeclInfo, &mut EvalEnv) -> R,
{
    let mut lookup = NoopEvalLookup;
    with_bound_type_decl_with_lookup(name, type_arguments, env, &mut lookup, f)
}

// ---------------------------------------------------------------------------
// Built-in utility types
// ---------------------------------------------------------------------------

fn try_builtin_utility(
    name: &str,
    type_arguments: &[TypeExpr],
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> Option<TypeExpr> {
    match name {
        "Partial" if type_arguments.len() == 1 => {
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            Some(apply_partial(&inner))
        }
        "Required" if type_arguments.len() == 1 => {
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            Some(apply_required(&inner))
        }
        "Readonly" if type_arguments.len() == 1 => {
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            Some(apply_readonly(&inner))
        }
        "Pick" if type_arguments.len() == 2 => {
            let keys = evaluate_with_lookup(&type_arguments[1], env, lookup);
            let key_set: rustc_hash::FxHashSet<String> =
                extract_string_keys_recursive(&keys).into_iter().collect();
            if !key_set.is_empty() {
                if let Some(obj) =
                    try_project_object_shape(&type_arguments[0], &key_set, false, env, lookup)
                {
                    return Some(TypeExpr::Object(obj));
                }
            }
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            Some(apply_pick(&inner, &keys))
        }
        "Omit" if type_arguments.len() == 2 => {
            let keys = evaluate_with_lookup(&type_arguments[1], env, lookup);
            let key_set: rustc_hash::FxHashSet<String> =
                extract_string_keys_recursive(&keys).into_iter().collect();
            if !key_set.is_empty() {
                if let Some(obj) =
                    try_project_object_shape(&type_arguments[0], &key_set, true, env, lookup)
                {
                    return Some(TypeExpr::Object(obj));
                }
            }
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            Some(apply_omit(&inner, &keys))
        }
        "Record" if type_arguments.len() == 2 => {
            let keys = evaluate_with_lookup(&type_arguments[0], env, lookup);
            let value = evaluate_with_lookup(&type_arguments[1], env, lookup);
            Some(apply_record(&keys, &value))
        }
        "Extract" if type_arguments.len() == 2 => {
            let source = evaluate_with_lookup(&type_arguments[0], env, lookup);
            let target = evaluate_with_lookup(&type_arguments[1], env, lookup);
            Some(apply_extract(&source, &target))
        }
        "Exclude" if type_arguments.len() == 2 => {
            let source = evaluate_with_lookup(&type_arguments[0], env, lookup);
            let target = evaluate_with_lookup(&type_arguments[1], env, lookup);
            Some(apply_exclude(&source, &target))
        }
        "NonNullable" if type_arguments.len() == 1 => {
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            Some(apply_non_nullable(&inner))
        }
        "ReturnType" if type_arguments.len() == 1 => {
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            Some(extract_return_type(&inner, env, lookup))
        }
        "Parameters" if type_arguments.len() == 1 => {
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            Some(extract_parameters(&inner, env, lookup))
        }
        "ConstructorParameters" if type_arguments.len() == 1 => {
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            Some(extract_constructor_parameters(&inner, env, lookup))
        }
        "InstanceType" if type_arguments.len() == 1 => {
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            Some(extract_instance_type(&inner, env, lookup))
        }
        "Awaited" if type_arguments.len() == 1 => {
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            Some(unwrap_awaited(&inner, env, lookup))
        }
        _ => None,
    }
}

// -- Partial<T> --

fn apply_partial(ty: &TypeExpr) -> TypeExpr {
    if let Some(obj) = extract_object_shape(ty) {
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
    } else {
        TypeExpr::named_with_args("Partial", vec![ty.clone()])
    }
}

// -- Required<T> --

fn apply_required(ty: &TypeExpr) -> TypeExpr {
    if let Some(obj) = extract_object_shape(ty) {
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
    } else {
        TypeExpr::named_with_args("Required", vec![ty.clone()])
    }
}

// -- Readonly<T> --

fn apply_readonly(ty: &TypeExpr) -> TypeExpr {
    if let Some(obj) = extract_object_shape(ty) {
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
    } else {
        TypeExpr::named_with_args("Readonly", vec![ty.clone()])
    }
}

// -- Pick<T, K> --

fn apply_pick(ty: &TypeExpr, keys: &TypeExpr) -> TypeExpr {
    let key_set = extract_string_keys_recursive(keys);
    if key_set.is_empty() {
        return TypeExpr::named_with_args("Pick", vec![ty.clone(), keys.clone()]);
    }

    if let Some(obj) = extract_object_shape(ty) {
        let properties = obj
            .properties
            .iter()
            .filter(|member| match member {
                ObjectMember::Property(prop) => key_set.contains(&prop.name),
                ObjectMember::Method(method) => key_set.contains(&method.name),
                _ => false,
            })
            .cloned()
            .collect();
        TypeExpr::Object(ObjectExpr { properties })
    } else {
        TypeExpr::named_with_args("Pick", vec![ty.clone(), keys.clone()])
    }
}

// -- Omit<T, K> --

fn apply_omit(ty: &TypeExpr, keys: &TypeExpr) -> TypeExpr {
    let key_set = extract_string_keys_recursive(keys);
    if key_set.is_empty() {
        return TypeExpr::named_with_args("Omit", vec![ty.clone(), keys.clone()]);
    }

    if let Some(obj) = extract_object_shape(ty) {
        let properties = obj
            .properties
            .iter()
            .filter(|member| match member {
                ObjectMember::Property(prop) => !key_set.contains(&prop.name),
                ObjectMember::Method(method) => !key_set.contains(&method.name),
                _ => true, // Keep index signatures and signatures.
            })
            .cloned()
            .collect();
        TypeExpr::Object(ObjectExpr { properties })
    } else {
        TypeExpr::named_with_args("Omit", vec![ty.clone(), keys.clone()])
    }
}

// -- Record<K, V> --

fn apply_record(keys: &TypeExpr, value: &TypeExpr) -> TypeExpr {
    let key_set = extract_string_keys_recursive(keys);
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

fn extract_return_type(ty: &TypeExpr, env: &mut EvalEnv, lookup: &mut dyn EvalLookup) -> TypeExpr {
    match ty {
        TypeExpr::Function(func) => {
            if let Some(ref ret) = func.return_type {
                evaluate_with_lookup(ret, env, lookup)
            } else {
                TypeExpr::Primitive(PrimitiveName::Void)
            }
        }
        // typeof fn → look up the function in value symbols
        TypeExpr::TypeOf(value_ref) => {
            if let Some(resolved) = resolve_typeof(value_ref, env, lookup) {
                extract_return_type(&resolved, env, lookup)
            } else {
                TypeExpr::named_with_args("ReturnType", vec![ty.clone()])
            }
        }
        _ => TypeExpr::named_with_args("ReturnType", vec![ty.clone()]),
    }
}

// -- Parameters<T> --

fn extract_parameters(ty: &TypeExpr, env: &mut EvalEnv, lookup: &mut dyn EvalLookup) -> TypeExpr {
    match ty {
        TypeExpr::Function(func) => {
            let elements = func
                .parameters
                .iter()
                .map(|p| TupleElement {
                    label: p.name.clone(),
                    ty: evaluate_with_lookup(&p.ty, env, lookup),
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
            if let Some(resolved) = resolve_typeof(value_ref, env, lookup) {
                extract_parameters(&resolved, env, lookup)
            } else {
                TypeExpr::named_with_args("Parameters", vec![ty.clone()])
            }
        }
        _ => TypeExpr::named_with_args("Parameters", vec![ty.clone()]),
    }
}

// -- ConstructorParameters<T> --

fn extract_constructor_parameters(
    ty: &TypeExpr,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> TypeExpr {
    match ty {
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                if let ObjectMember::ConstructSignature(func) = member {
                    let elements = func
                        .parameters
                        .iter()
                        .map(|p| TupleElement {
                            label: p.name.clone(),
                            ty: evaluate_with_lookup(&p.ty, env, lookup),
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

fn extract_instance_type(
    ty: &TypeExpr,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> TypeExpr {
    match ty {
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                if let ObjectMember::ConstructSignature(func) = member {
                    if let Some(ref ret) = func.return_type {
                        return evaluate_with_lookup(ret, env, lookup);
                    }
                }
            }
            TypeExpr::named_with_args("InstanceType", vec![ty.clone()])
        }
        _ => TypeExpr::named_with_args("InstanceType", vec![ty.clone()]),
    }
}

// -- Awaited<T> --

fn unwrap_awaited(ty: &TypeExpr, env: &mut EvalEnv, lookup: &mut dyn EvalLookup) -> TypeExpr {
    match ty {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if name == "Promise" && type_arguments.len() == 1 => {
            let inner = evaluate_with_lookup(&type_arguments[0], env, lookup);
            // Recursively unwrap nested Promises
            unwrap_awaited(&inner, env, lookup)
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

fn evaluate_typeof(
    value_ref: &ValueRef,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> TypeExpr {
    if let Some(resolved) = resolve_typeof(value_ref, env, lookup) {
        resolved
    } else {
        TypeExpr::TypeOf(value_ref.clone())
    }
}

fn resolve_typeof(
    value_ref: &ValueRef,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> Option<TypeExpr> {
    let decl = if value_ref.path.len() == 1 {
        let name = &value_ref.path[0];
        env.value_symbols
            .get(name)
            .cloned()
            .or_else(|| lookup.resolve_value_decl(&value_ref.path))?
    } else {
        lookup.resolve_value_decl(&value_ref.path)?
    };

    // If there's an explicit type annotation, use it
    if let Some(ref ty) = decl.type_annotation {
        return Some(evaluate_with_lookup(ty, env, lookup));
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

/// Try to look up a member by name on a raw (unevaluated) type expression.
///
/// This avoids eagerly evaluating ALL members of large object types when only
/// one member is needed. For `ComponentConfig<T, A, K>['slots']`, this resolves
/// the generic body to find `slots` without evaluating `AppConfig`, `variants`,
/// or `ui`.
///
/// Returns `Some(evaluated_member_type)` if the member can be found and evaluated
/// directly. Returns `None` to fall back to eager evaluation.
///
/// Handles:
/// - `TypeExpr::Object` — direct property scan
/// - `TypeExpr::Ref` — resolve declaration body, bind generic params, recurse
///
/// Does NOT handle (falls back to eager):
/// - Mapped types, conditionals, intersections, unions
/// - Non-object bodies
fn try_lazy_member_lookup(
    object: &TypeExpr,
    key: &str,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> Option<TypeExpr> {
    match object {
        TypeExpr::Object(obj) => {
            // Direct object literal — look up the member without evaluating siblings
            for member in &obj.properties {
                if let ObjectMember::Property(p) = member {
                    if p.name == key {
                        return Some(evaluate_with_lookup(&p.ty, env, lookup));
                    }
                }
                if let ObjectMember::Method(m) = member {
                    if m.name == key {
                        return Some(TypeExpr::Function(m.function.clone()));
                    }
                }
            }
            None // Member not found in this object
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if let Some(result) =
                try_lazy_builtin_member_lookup(name.as_str(), type_arguments, key, env, lookup)
            {
                return Some(result);
            }

            let decl = env
                .type_symbols
                .get(name)
                .cloned()
                .or_else(|| lookup.resolve_type_decl(name))?;
            if env.active.contains(name) {
                return None;
            }
            env.active.insert(name.to_string());
            let saved = bind_type_parameters(&decl, type_arguments, env);
            let result = try_lazy_member_lookup(&decl.body, key, env, lookup);
            restore_type_parameters(saved, env);
            env.active.remove(name);
            result
        }
        TypeExpr::Parenthesized(inner) => try_lazy_member_lookup(inner, key, env, lookup),
        _ => lookup.resolve_member_projection(object, key),
    }
}

fn try_lazy_builtin_member_lookup(
    name: &str,
    type_arguments: &[TypeExpr],
    key: &str,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> Option<TypeExpr> {
    match name {
        // These wrappers only change property modifiers, not the property type.
        "Partial" | "Required" | "Readonly" if type_arguments.len() == 1 => {
            try_lazy_member_lookup(&type_arguments[0], key, env, lookup)
        }
        "Pick" if type_arguments.len() == 2 => {
            let keys = evaluate_with_lookup(&type_arguments[1], env, lookup);
            let key_set: rustc_hash::FxHashSet<String> =
                extract_string_keys_recursive(&keys).into_iter().collect();
            if key_set.contains(key) {
                try_lazy_member_lookup(&type_arguments[0], key, env, lookup)
            } else {
                None
            }
        }
        "Omit" if type_arguments.len() == 2 => {
            let keys = evaluate_with_lookup(&type_arguments[1], env, lookup);
            let key_set: rustc_hash::FxHashSet<String> =
                extract_string_keys_recursive(&keys).into_iter().collect();
            if !key_set.contains(key) {
                try_lazy_member_lookup(&type_arguments[0], key, env, lookup)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Project an object/ref surface to a subset of members without evaluating the
/// omitted member types. `omit_mode = false` behaves like Pick; `true` behaves
/// like Omit.
fn try_project_object_shape(
    object: &TypeExpr,
    keys: &rustc_hash::FxHashSet<String>,
    omit_mode: bool,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> Option<ObjectExpr> {
    match object {
        TypeExpr::Object(obj) => Some(project_object_members(obj, keys, omit_mode, env, lookup)),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            let decl = env
                .type_symbols
                .get(name)
                .cloned()
                .or_else(|| lookup.resolve_type_decl(name));
            if let Some(decl) = decl {
                if env.active.contains(name) {
                    return None;
                }
                env.active.insert(name.to_string());
                let saved = bind_type_parameters(&decl, type_arguments, env);
                let projected = try_project_object_shape(&decl.body, keys, omit_mode, env, lookup);
                restore_type_parameters(saved, env);
                env.active.remove(name);
                if let Some(projected) = projected {
                    return Some(projected);
                }
            }

            let evaluated = evaluate_with_lookup(object, env, lookup);
            if &evaluated == object {
                None
            } else {
                try_project_object_shape(&evaluated, keys, omit_mode, env, lookup)
            }
        }
        TypeExpr::Intersection(types) => {
            let projected: Vec<ObjectExpr> = types
                .iter()
                .filter_map(|branch| try_project_object_shape(branch, keys, omit_mode, env, lookup))
                .collect();
            merge_object_branches(projected)
        }
        TypeExpr::Parenthesized(inner) => {
            try_project_object_shape(inner, keys, omit_mode, env, lookup)
        }
        TypeExpr::IndexedAccess { .. } | TypeExpr::TypeOf(_) => {
            let evaluated = evaluate_with_lookup(object, env, lookup);
            if &evaluated == object {
                None
            } else {
                try_project_object_shape(&evaluated, keys, omit_mode, env, lookup)
            }
        }
        _ => None,
    }
}

fn project_object_members(
    obj: &ObjectExpr,
    keys: &rustc_hash::FxHashSet<String>,
    omit_mode: bool,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> ObjectExpr {
    let properties = obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) => {
                let keep = if omit_mode {
                    !keys.contains(&prop.name)
                } else {
                    keys.contains(&prop.name)
                };
                keep.then(|| {
                    ObjectMember::Property(ObjectProperty {
                        ty: evaluate_with_lookup(&prop.ty, env, lookup),
                        ..prop.clone()
                    })
                })
            }
            ObjectMember::Method(method) => {
                let keep = if omit_mode {
                    !keys.contains(&method.name)
                } else {
                    keys.contains(&method.name)
                };
                keep.then(|| ObjectMember::Method(method.clone()))
            }
            ObjectMember::IndexSignature(sig) => {
                omit_mode.then(|| ObjectMember::IndexSignature(sig.clone()))
            }
            ObjectMember::CallSignature(sig) => omit_mode.then(|| {
                ObjectMember::CallSignature(FunctionExpr {
                    parameters: sig
                        .parameters
                        .iter()
                        .map(|param| FunctionParam {
                            ty: evaluate_with_lookup(&param.ty, env, lookup),
                            ..param.clone()
                        })
                        .collect(),
                    return_type: sig
                        .return_type
                        .as_ref()
                        .map(|ret| Box::new(evaluate_with_lookup(ret, env, lookup))),
                    type_parameters: sig.type_parameters.clone(),
                })
            }),
            ObjectMember::ConstructSignature(sig) => omit_mode.then(|| {
                ObjectMember::ConstructSignature(FunctionExpr {
                    parameters: sig
                        .parameters
                        .iter()
                        .map(|param| FunctionParam {
                            ty: evaluate_with_lookup(&param.ty, env, lookup),
                            ..param.clone()
                        })
                        .collect(),
                    return_type: sig
                        .return_type
                        .as_ref()
                        .map(|ret| Box::new(evaluate_with_lookup(ret, env, lookup))),
                    type_parameters: sig.type_parameters.clone(),
                })
            }),
        })
        .collect();

    ObjectExpr { properties }
}

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
    lookup: &mut dyn EvalLookup,
) -> TypeExpr {
    // Check mapped nesting depth limit
    if env.mapped_depth >= env.limits.max_mapped_depth {
        return TypeExpr::Mapped {
            parameter: parameter.to_string(),
            source: Box::new(evaluate_with_lookup(source, env, lookup)),
            value: Box::new(value.clone()),
            optional,
            readonly,
            name_type: name_type.clone(),
        };
    }
    env.mapped_depth += 1;

    let resolved_source = evaluate_with_lookup(source, env, lookup);
    let keys = extract_string_keys_recursive(&resolved_source);

    if keys.is_empty() || keys.len() > env.limits.max_mapped_keys {
        env.mapped_depth -= 1;
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
    let source_props = extract_source_property_modifiers(&resolved_source, env, lookup);

    let properties: Vec<ObjectMember> = keys
        .into_iter()
        .map(|key| {
            env.type_bindings
                .insert(parameter.to_string(), TypeExpr::string_literal(&key));
            let prop_type = evaluate_with_lookup(value, env, lookup);
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

    env.mapped_depth -= 1;
    TypeExpr::Object(ObjectExpr { properties })
}

// ---------------------------------------------------------------------------
// Template literal types
// ---------------------------------------------------------------------------

fn evaluate_template_literal(
    quasis: &[String],
    expressions: &[TypeExpr],
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> TypeExpr {
    let evaluated_exprs: Vec<TypeExpr> = expressions
        .iter()
        .map(|e| evaluate_with_lookup(e, env, lookup))
        .collect();

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

fn evaluate_object_member(
    member: &ObjectMember,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> ObjectMember {
    match member {
        ObjectMember::Property(p) => ObjectMember::Property(ObjectProperty {
            name: p.name.clone(),
            ty: evaluate_with_lookup(&p.ty, env, lookup),
            optional: p.optional,
            readonly: p.readonly,
        }),
        ObjectMember::IndexSignature(idx) => ObjectMember::IndexSignature(IndexSignature {
            key_name: idx.key_name.clone(),
            key_type: evaluate_with_lookup(&idx.key_type, env, lookup),
            value_type: evaluate_with_lookup(&idx.value_type, env, lookup),
            readonly: idx.readonly,
        }),
        ObjectMember::CallSignature(func) => {
            ObjectMember::CallSignature(evaluate_function(func, env, lookup))
        }
        ObjectMember::ConstructSignature(func) => {
            ObjectMember::ConstructSignature(evaluate_function(func, env, lookup))
        }
        ObjectMember::Method(m) => ObjectMember::Method(MethodSignature {
            name: m.name.clone(),
            function: evaluate_function(&m.function, env, lookup),
            optional: m.optional,
        }),
    }
}

/// Extract (optional, readonly) modifiers for each named property from a source type.
/// Used by mapped type evaluation to preserve modifiers when MappedModifier::None.
fn extract_source_property_modifiers(
    source: &TypeExpr,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> rustc_hash::FxHashMap<String, (bool, bool)> {
    let mut result = rustc_hash::FxHashMap::default();
    // If the source is keyof T, resolve T to get its properties
    let obj = match source {
        TypeExpr::KeyOf(inner) => evaluate_with_lookup(inner, env, lookup),
        _ => return result,
    };
    if let Some(obj_expr) = extract_object_shape(&obj) {
        for member in &obj_expr.properties {
            if let ObjectMember::Property(p) = member {
                result.insert(p.name.clone(), (p.optional, p.readonly));
            }
        }
    }
    result
}

pub(crate) fn extract_object_shape(ty: &TypeExpr) -> Option<ObjectExpr> {
    match ty {
        TypeExpr::Object(obj) => Some(obj.clone()),
        TypeExpr::Parenthesized(inner) => extract_object_shape(inner),
        TypeExpr::Intersection(types) => merge_object_branches(
            types
                .iter()
                .filter_map(extract_object_shape)
                .collect::<Vec<_>>(),
        ),
        _ => None,
    }
}

fn merge_object_branches(objects: Vec<ObjectExpr>) -> Option<ObjectExpr> {
    if objects.is_empty() {
        return None;
    }

    let mut merged = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for obj in objects {
        for member in obj.properties {
            if let ObjectMember::Property(ref p) = member {
                if seen.insert(p.name.clone()) {
                    merged.push(member);
                }
            } else {
                merged.push(member);
            }
        }
    }

    Some(ObjectExpr { properties: merged })
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

fn evaluate_function(
    func: &FunctionExpr,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> FunctionExpr {
    FunctionExpr {
        parameters: func
            .parameters
            .iter()
            .map(|p| FunctionParam {
                name: p.name.clone(),
                ty: evaluate_with_lookup(&p.ty, env, lookup),
                optional: p.optional,
                rest: p.rest,
            })
            .collect(),
        return_type: func
            .return_type
            .as_ref()
            .map(|r| Box::new(evaluate_with_lookup(r, env, lookup))),
        type_parameters: func.type_parameters.clone(),
    }
}

#[allow(dead_code)]
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

fn extract_string_keys_recursive(ty: &TypeExpr) -> Vec<String> {
    fn collect_keys(ty: &TypeExpr, keys: &mut Vec<String>) -> bool {
        match ty {
            TypeExpr::Literal(LiteralValue::String(s)) => {
                keys.push(s.clone());
                true
            }
            TypeExpr::Union(types) => types.iter().all(|ty| collect_keys(ty, keys)),
            TypeExpr::Parenthesized(inner) => collect_keys(inner, keys),
            _ => false,
        }
    }

    let mut keys = Vec::new();
    if collect_keys(ty, &mut keys) {
        keys
    } else {
        Vec::new()
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
pub(crate) fn is_assignable_to(check: &TypeExpr, target: &TypeExpr) -> bool {
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
pub(crate) fn is_definitely_not_assignable(check: &TypeExpr, target: &TypeExpr) -> bool {
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
