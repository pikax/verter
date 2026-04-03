//! Built-in TypeScript utility type semantics.
//!
//! Reimplements standard utility types on top of the solver arena rather than
//! relying on operator-specific shape hacks.
//!
//! Compiler intrinsics (`Uppercase`, `Lowercase`, `Capitalize`, `Uncapitalize`)
//! are not shadowable by user declarations. `Awaited` follows TypeScript-style
//! recursive thenable unwrapping. `NoInfer` blocks inference flow.

use super::arena::{NodeId, PrimitiveKind, QueryArena};

// ---------------------------------------------------------------------------
// Built-in utility registry
// ---------------------------------------------------------------------------

/// Recognized built-in TypeScript utility types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinUtility {
    // -- Object utilities --
    Partial,
    Required,
    Readonly,
    Pick,
    Omit,
    Record,

    // -- Union/extraction utilities --
    Extract,
    Exclude,
    NonNullable,

    // -- Function utilities --
    ReturnType,
    Parameters,
    ConstructorParameters,
    InstanceType,

    // -- Promise utilities --
    Awaited,

    // -- Compiler string intrinsics (not shadowable) --
    Uppercase,
    Lowercase,
    Capitalize,
    Uncapitalize,

    // -- Inference utilities --
    NoInfer,
}

impl BuiltinUtility {
    /// Look up a utility by name. Returns `None` for non-utility names.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Partial" => Some(Self::Partial),
            "Required" => Some(Self::Required),
            "Readonly" => Some(Self::Readonly),
            "Pick" => Some(Self::Pick),
            "Omit" => Some(Self::Omit),
            "Record" => Some(Self::Record),
            "Extract" => Some(Self::Extract),
            "Exclude" => Some(Self::Exclude),
            "NonNullable" => Some(Self::NonNullable),
            "ReturnType" => Some(Self::ReturnType),
            "Parameters" => Some(Self::Parameters),
            "ConstructorParameters" => Some(Self::ConstructorParameters),
            "InstanceType" => Some(Self::InstanceType),
            "Awaited" => Some(Self::Awaited),
            "Uppercase" => Some(Self::Uppercase),
            "Lowercase" => Some(Self::Lowercase),
            "Capitalize" => Some(Self::Capitalize),
            "Uncapitalize" => Some(Self::Uncapitalize),
            "NoInfer" => Some(Self::NoInfer),
            _ => None,
        }
    }

    /// Whether this is a compiler intrinsic that cannot be shadowed by user code.
    pub fn is_compiler_intrinsic(self) -> bool {
        matches!(
            self,
            Self::Uppercase | Self::Lowercase | Self::Capitalize | Self::Uncapitalize
        )
    }

    /// Expected number of type arguments for this utility.
    pub fn expected_arity(self) -> usize {
        match self {
            Self::Partial
            | Self::Required
            | Self::Readonly
            | Self::NonNullable
            | Self::ReturnType
            | Self::Parameters
            | Self::ConstructorParameters
            | Self::InstanceType
            | Self::Awaited
            | Self::Uppercase
            | Self::Lowercase
            | Self::Capitalize
            | Self::Uncapitalize
            | Self::NoInfer => 1,
            Self::Pick | Self::Omit | Self::Record | Self::Extract | Self::Exclude => 2,
        }
    }

    /// The name of this utility as a string.
    pub fn name(self) -> &'static str {
        match self {
            Self::Partial => "Partial",
            Self::Required => "Required",
            Self::Readonly => "Readonly",
            Self::Pick => "Pick",
            Self::Omit => "Omit",
            Self::Record => "Record",
            Self::Extract => "Extract",
            Self::Exclude => "Exclude",
            Self::NonNullable => "NonNullable",
            Self::ReturnType => "ReturnType",
            Self::Parameters => "Parameters",
            Self::ConstructorParameters => "ConstructorParameters",
            Self::InstanceType => "InstanceType",
            Self::Awaited => "Awaited",
            Self::Uppercase => "Uppercase",
            Self::Lowercase => "Lowercase",
            Self::Capitalize => "Capitalize",
            Self::Uncapitalize => "Uncapitalize",
            Self::NoInfer => "NoInfer",
        }
    }
}

// ---------------------------------------------------------------------------
// Utility expansion scaffolding
// ---------------------------------------------------------------------------

/// Expand a built-in utility type with its resolved type arguments.
///
/// Returns the expanded `NodeId` in the arena, or `None` if the utility
/// cannot be expanded with the given arguments (wrong arity, etc.).
///
/// NOTE: This is skeleton-level — each utility will be fully implemented
/// in Milestone 4. Currently returns symbolic `Ref` nodes as placeholders.
pub fn expand_builtin(
    arena: &mut QueryArena,
    utility: BuiltinUtility,
    args: &[NodeId],
) -> Option<NodeId> {
    if args.len() < utility.expected_arity() {
        return None;
    }

    match utility {
        // -- Compiler string intrinsics --
        BuiltinUtility::Uppercase => expand_string_intrinsic(arena, args[0], str::to_uppercase),
        BuiltinUtility::Lowercase => expand_string_intrinsic(arena, args[0], str::to_lowercase),
        BuiltinUtility::Capitalize => expand_string_intrinsic(arena, args[0], capitalize),
        BuiltinUtility::Uncapitalize => expand_string_intrinsic(arena, args[0], uncapitalize),

        // -- NoInfer: identity wrapper that blocks inference --
        BuiltinUtility::NoInfer => Some(args[0]),

        // -- NonNullable: filters out null | undefined --
        BuiltinUtility::NonNullable => Some(expand_non_nullable(arena, args[0])),

        // -- Object modifier utilities --
        BuiltinUtility::Partial => Some(expand_object_modifier(arena, args[0], |p| {
            p.optional = true;
        })),
        BuiltinUtility::Required => Some(expand_object_modifier(arena, args[0], |p| {
            p.optional = false;
        })),
        BuiltinUtility::Readonly => Some(expand_object_modifier(arena, args[0], |p| {
            p.readonly = true;
        })),

        // -- Pick<T, K> / Omit<T, K> --
        BuiltinUtility::Pick => Some(expand_pick_omit(arena, args[0], args[1], true)),
        BuiltinUtility::Omit => Some(expand_pick_omit(arena, args[0], args[1], false)),

        // -- Record<K, V> --
        BuiltinUtility::Record => Some(expand_record(arena, args[0], args[1])),

        // -- Extract<T, U> / Exclude<T, U> --
        BuiltinUtility::Extract => Some(expand_extract_exclude(arena, args[0], args[1], true)),
        BuiltinUtility::Exclude => Some(expand_extract_exclude(arena, args[0], args[1], false)),

        // -- Function utilities --
        BuiltinUtility::ReturnType => Some(expand_return_type(arena, args[0])),
        BuiltinUtility::Parameters => Some(expand_parameters(arena, args[0])),

        // -- Awaited<T>: recursive thenable unwrapping --
        BuiltinUtility::Awaited => Some(expand_awaited(arena, args[0])),

        // -- ConstructorParameters<T>: like Parameters but from construct signatures --
        BuiltinUtility::ConstructorParameters => {
            Some(expand_constructor_parameters(arena, args[0]))
        }

        // -- InstanceType<T>: return type of construct signature --
        BuiltinUtility::InstanceType => Some(expand_instance_type(arena, args[0])),
    }
}

// ---------------------------------------------------------------------------
// String intrinsic helpers
// ---------------------------------------------------------------------------

fn expand_string_intrinsic(
    arena: &mut QueryArena,
    arg: NodeId,
    transform: fn(&str) -> String,
) -> Option<NodeId> {
    let node = arena.get(arg).clone();
    match node {
        super::arena::Node::Literal(super::arena::SolverLiteral::String(ref s)) => {
            let transformed = transform(s);
            Some(arena.string_literal(transformed))
        }
        super::arena::Node::Union(members) => {
            let transformed: Vec<NodeId> = members
                .iter()
                .filter_map(|&m| expand_string_intrinsic(arena, m, transform))
                .collect();
            if transformed.len() == members.len() {
                Some(arena.union(transformed))
            } else {
                None // Not all members were string literals
            }
        }
        // Non-literal string: return symbolic
        _ => Some(arena.type_ref(
            // We can't resolve this statically
            "string",
            vec![],
        )),
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

fn uncapitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_lowercase().to_string() + chars.as_str(),
    }
}

// ---------------------------------------------------------------------------
// NonNullable helper
// ---------------------------------------------------------------------------

fn expand_non_nullable(arena: &mut QueryArena, arg: NodeId) -> NodeId {
    // NonNullable<any> = any (any already includes non-nullable types)
    if matches!(
        arena.get(arg),
        super::arena::Node::Primitive(PrimitiveKind::Any)
    ) {
        return arg;
    }

    let node = arena.get(arg).clone();
    match node {
        super::arena::Node::Union(members) => {
            let filtered: Vec<NodeId> = members
                .iter()
                .copied()
                .filter(|&m| {
                    !matches!(
                        arena.get(m),
                        super::arena::Node::Primitive(PrimitiveKind::Null)
                            | super::arena::Node::Primitive(PrimitiveKind::Undefined)
                    )
                })
                .collect();
            arena.union(filtered)
        }
        super::arena::Node::Primitive(PrimitiveKind::Null)
        | super::arena::Node::Primitive(PrimitiveKind::Undefined) => {
            arena.primitive(PrimitiveKind::Never)
        }
        _ => arg,
    }
}

// ---------------------------------------------------------------------------
// Object modifier: Partial / Required / Readonly
// ---------------------------------------------------------------------------

/// Apply a modifier to every property of an object type.
/// If the input isn't an object, returns it unchanged.
fn expand_object_modifier(
    arena: &mut QueryArena,
    arg: NodeId,
    modifier: impl Fn(&mut super::arena::PropertyNode),
) -> NodeId {
    let node = arena.get(arg).clone();
    match node {
        super::arena::Node::Object(mut obj) => {
            for prop in &mut obj.properties {
                modifier(prop);
            }
            arena.object(obj)
        }
        // Non-object: return as-is (symbolic passthrough)
        _ => arg,
    }
}

// ---------------------------------------------------------------------------
// Pick / Omit
// ---------------------------------------------------------------------------

/// Pick<T, K> keeps only keys in K. Omit<T, K> removes keys in K.
fn expand_pick_omit(
    arena: &mut QueryArena,
    obj_arg: NodeId,
    keys_arg: NodeId,
    is_pick: bool,
) -> NodeId {
    // Collect the key set from the keys argument
    let key_set = collect_string_literal_keys(arena, keys_arg);
    let Some(obj) = flatten_object_like(arena, obj_arg) else {
        return obj_arg;
    };

    let filtered: Vec<super::arena::PropertyNode> = obj
        .properties
        .into_iter()
        .filter(|p| {
            let in_set = key_set.contains(&p.name);
            if is_pick {
                in_set
            } else {
                !in_set
            }
        })
        .collect();
    arena.object(super::arena::ObjectNode {
        properties: filtered,
        index_signatures: if is_pick {
            vec![] // Pick drops index signatures
        } else {
            obj.index_signatures
        },
        call_signatures: obj.call_signatures,
        construct_signatures: obj.construct_signatures,
    })
}

/// Collect string literal values from a type (handles unions).
fn collect_string_literal_keys(arena: &QueryArena, node: NodeId) -> Vec<String> {
    let mut keys = Vec::new();
    let mut stack = vec![node];
    while let Some(id) = stack.pop() {
        match arena.get(id) {
            super::arena::Node::Literal(super::arena::SolverLiteral::String(s)) => {
                keys.push(s.clone());
            }
            super::arena::Node::Union(members) => {
                stack.extend(members.iter().copied());
            }
            _ => {}
        }
    }
    keys
}

fn flatten_object_like(arena: &mut QueryArena, node: NodeId) -> Option<super::arena::ObjectNode> {
    let node_data = arena.get(node).clone();
    match node_data {
        super::arena::Node::Object(obj) => Some(obj),
        super::arena::Node::Intersection(members) => {
            let mut merged_props: rustc_hash::FxHashMap<String, super::arena::PropertyNode> =
                rustc_hash::FxHashMap::default();
            let mut index_signatures = Vec::new();
            let mut call_signatures = Vec::new();
            let mut construct_signatures = Vec::new();

            for member in members {
                let obj = flatten_object_like(arena, member)?;
                for prop in obj.properties {
                    if let Some(existing) = merged_props.get_mut(&prop.name) {
                        existing.ty = arena.intersection(vec![existing.ty, prop.ty]);
                        existing.optional = existing.optional && prop.optional;
                        existing.readonly = existing.readonly || prop.readonly;
                        existing.is_method = existing.is_method && prop.is_method;
                    } else {
                        merged_props.insert(prop.name.clone(), prop);
                    }
                }
                index_signatures.extend(obj.index_signatures);
                call_signatures.extend(obj.call_signatures);
                construct_signatures.extend(obj.construct_signatures);
            }

            let mut properties: Vec<_> = merged_props.into_values().collect();
            properties.sort_by(|a, b| a.name.cmp(&b.name));
            Some(super::arena::ObjectNode {
                properties,
                index_signatures,
                call_signatures,
                construct_signatures,
            })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Record
// ---------------------------------------------------------------------------

/// Record<K, V> creates an object with index signature [key: K]: V.
/// If K is a finite union of string literals, creates named properties instead.
fn expand_record(arena: &mut QueryArena, key_arg: NodeId, value_arg: NodeId) -> NodeId {
    // Record<never, V> = {} (empty object)
    if matches!(
        arena.get(key_arg),
        super::arena::Node::Primitive(PrimitiveKind::Never)
    ) {
        return arena.object(super::arena::ObjectNode {
            properties: vec![],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });
    }

    let keys = collect_string_literal_keys(arena, key_arg);
    if !keys.is_empty() {
        // Finite key set — create named properties
        let properties: Vec<super::arena::PropertyNode> = keys
            .into_iter()
            .map(|name| super::arena::PropertyNode {
                name,
                ty: value_arg,
                optional: false,
                readonly: false,
                is_method: false,
            })
            .collect();
        arena.object(super::arena::ObjectNode {
            properties,
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        })
    } else {
        // Open key domain — create index signature
        arena.object(super::arena::ObjectNode {
            properties: vec![],
            index_signatures: vec![super::arena::IndexSignatureNode {
                key_type: key_arg,
                value_type: value_arg,
                readonly: false,
            }],
            call_signatures: vec![],
            construct_signatures: vec![],
        })
    }
}

// ---------------------------------------------------------------------------
// Extract / Exclude
// ---------------------------------------------------------------------------

/// Extract<T, U> keeps union members assignable to U.
/// Exclude<T, U> removes union members assignable to U.
fn expand_extract_exclude(
    arena: &mut QueryArena,
    type_arg: NodeId,
    filter_arg: NodeId,
    is_extract: bool,
) -> NodeId {
    let node = arena.get(type_arg).clone();

    match node {
        super::arena::Node::Union(members) => {
            // Use a simple structural check: keep/remove members that match the filter
            let mut caches = super::arena::SolverCaches::new();
            let mut rel_state =
                super::relate::RelationState::new(super::relate::RelationLimits::default());

            let filtered: Vec<NodeId> = members
                .into_iter()
                .filter(|&m| {
                    let result = super::relate::relate(
                        arena,
                        &mut caches,
                        m,
                        filter_arg,
                        super::result::RelationMode::Assignable,
                        &mut rel_state,
                    );
                    let assignable = result == super::result::RelationResult::Assignable;
                    if is_extract {
                        assignable
                    } else {
                        !assignable
                    }
                })
                .collect();
            arena.union(filtered)
        }
        // Non-union: check the single type directly
        _ => {
            let mut caches = super::arena::SolverCaches::new();
            let mut rel_state =
                super::relate::RelationState::new(super::relate::RelationLimits::default());
            let result = super::relate::relate(
                arena,
                &mut caches,
                type_arg,
                filter_arg,
                super::result::RelationMode::Assignable,
                &mut rel_state,
            );
            let assignable = result == super::result::RelationResult::Assignable;
            if is_extract {
                if assignable {
                    type_arg
                } else {
                    arena.primitive(PrimitiveKind::Never)
                }
            } else if assignable {
                arena.primitive(PrimitiveKind::Never)
            } else {
                type_arg
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ReturnType / Parameters
// ---------------------------------------------------------------------------

/// ReturnType<T> extracts the return type from a function type.
fn expand_return_type(arena: &mut QueryArena, arg: NodeId) -> NodeId {
    let node = arena.get(arg).clone();
    match node {
        super::arena::Node::Function(func) => {
            if let Some(sig) = func.signatures.first() {
                sig.return_type
            } else {
                arena.primitive(PrimitiveKind::Void)
            }
        }
        _ => arg, // Non-function: return as-is
    }
}

/// Parameters<T> extracts a tuple of parameter types from a function type.
fn expand_parameters(arena: &mut QueryArena, arg: NodeId) -> NodeId {
    let node = arena.get(arg).clone();
    match node {
        super::arena::Node::Function(func) => {
            if let Some(sig) = func.signatures.first() {
                let elements: Vec<super::arena::TupleNodeElement> = sig
                    .parameters
                    .iter()
                    .map(|p| super::arena::TupleNodeElement {
                        label: p.name.clone(),
                        ty: p.ty,
                        optional: p.optional,
                        rest: p.rest,
                    })
                    .collect();
                arena.alloc(super::arena::Node::Tuple {
                    elements,
                    readonly: false,
                })
            } else {
                arena.alloc(super::arena::Node::Tuple {
                    elements: vec![],
                    readonly: false,
                })
            }
        }
        _ => arg,
    }
}

// ---------------------------------------------------------------------------
// Awaited — recursive thenable unwrapping
// ---------------------------------------------------------------------------

/// `Awaited<T>` — iteratively unwraps thenables (types with a `then` method
/// whose callback parameter type is the resolved value).
///
/// Uses iterative unwrapping with a depth limit (matches TS behavior).
/// Iterative worklist-based Awaited unwrapping. Handles unions without
/// recursion by pushing members onto the worklist.
fn expand_awaited(arena: &mut QueryArena, arg: NodeId) -> NodeId {
    let mut worklist = vec![arg];
    let mut results = Vec::new();
    let max_iterations = 10_000; // safety rail
    let mut iterations = 0;

    while let Some(item) = worklist.pop() {
        iterations += 1;
        if iterations > max_iterations {
            results.push(item);
            continue;
        }

        // Iteratively unwrap thenables for this item
        let mut current = item;
        for _ in 0..100 {
            let node = arena.get(current).clone();
            match node {
                super::arena::Node::Union(members) => {
                    // Push each member onto worklist — no recursion
                    worklist.extend(members.iter().copied());
                    current = NodeId::UNRESOLVED; // signal: handled via worklist
                    break;
                }
                super::arena::Node::Object(ref obj) => {
                    if let Some(next) = try_unwrap_thenable(arena, obj) {
                        current = next;
                        continue; // keep unwrapping
                    }
                    break; // not thenable
                }
                _ => break, // non-thenable terminal
            }
        }

        if !current.is_unresolved() {
            results.push(current);
        }
    }

    arena.union(results)
}

/// Try to extract the resolved type from a thenable object's `then` method.
fn try_unwrap_thenable(arena: &QueryArena, obj: &super::arena::ObjectNode) -> Option<NodeId> {
    let then_ty = obj
        .properties
        .iter()
        .find(|p| p.name == "then" && p.is_method)
        .map(|p| p.ty)?;

    let then_node = arena.get(then_ty);
    let super::arena::Node::Function(func) = then_node else {
        return None;
    };
    let sig = func.signatures.first()?;
    let callback = sig.parameters.first()?;
    let cb_node = arena.get(callback.ty);
    let super::arena::Node::Function(cb_func) = cb_node else {
        return None;
    };
    let cb_sig = cb_func.signatures.first()?;
    let resolved_param = cb_sig.parameters.first()?;
    Some(resolved_param.ty)
}

// ---------------------------------------------------------------------------
// ConstructorParameters / InstanceType
// ---------------------------------------------------------------------------

/// `ConstructorParameters<T>` — extract parameter tuple from construct signatures.
fn expand_constructor_parameters(arena: &mut QueryArena, arg: NodeId) -> NodeId {
    let node = arena.get(arg).clone();
    match node {
        super::arena::Node::Object(obj) => {
            if let Some(sig) = obj.construct_signatures.first() {
                let elements: Vec<super::arena::TupleNodeElement> = sig
                    .parameters
                    .iter()
                    .map(|p| super::arena::TupleNodeElement {
                        label: p.name.clone(),
                        ty: p.ty,
                        optional: p.optional,
                        rest: p.rest,
                    })
                    .collect();
                arena.alloc(super::arena::Node::Tuple {
                    elements,
                    readonly: false,
                })
            } else {
                arena.alloc(super::arena::Node::Tuple {
                    elements: vec![],
                    readonly: false,
                })
            }
        }
        _ => arg,
    }
}

/// `InstanceType<T>` — extract return type from construct signatures.
fn expand_instance_type(arena: &mut QueryArena, arg: NodeId) -> NodeId {
    let node = arena.get(arg).clone();
    match node {
        super::arena::Node::Object(obj) => {
            if let Some(sig) = obj.construct_signatures.first() {
                sig.return_type
            } else {
                arena.primitive(PrimitiveKind::Any)
            }
        }
        _ => arg,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::arena::*;
    use super::*;

    #[test]
    fn from_name_recognizes_all_utilities() {
        assert_eq!(
            BuiltinUtility::from_name("Partial"),
            Some(BuiltinUtility::Partial)
        );
        assert_eq!(
            BuiltinUtility::from_name("Record"),
            Some(BuiltinUtility::Record)
        );
        assert_eq!(
            BuiltinUtility::from_name("Awaited"),
            Some(BuiltinUtility::Awaited)
        );
        assert_eq!(
            BuiltinUtility::from_name("NoInfer"),
            Some(BuiltinUtility::NoInfer)
        );
        assert_eq!(BuiltinUtility::from_name("NotAUtility"), None);
    }

    #[test]
    fn compiler_intrinsics_not_shadowable() {
        assert!(BuiltinUtility::Uppercase.is_compiler_intrinsic());
        assert!(BuiltinUtility::Lowercase.is_compiler_intrinsic());
        assert!(BuiltinUtility::Capitalize.is_compiler_intrinsic());
        assert!(BuiltinUtility::Uncapitalize.is_compiler_intrinsic());
        assert!(!BuiltinUtility::Partial.is_compiler_intrinsic());
    }

    #[test]
    fn expected_arity() {
        assert_eq!(BuiltinUtility::Partial.expected_arity(), 1);
        assert_eq!(BuiltinUtility::Record.expected_arity(), 2);
        assert_eq!(BuiltinUtility::Extract.expected_arity(), 2);
    }

    #[test]
    fn uppercase_literal() {
        let mut arena = QueryArena::new();
        let lit = arena.string_literal("hello");
        let result = expand_builtin(&mut arena, BuiltinUtility::Uppercase, &[lit]).unwrap();
        assert!(matches!(
            arena.get(result),
            Node::Literal(SolverLiteral::String(s)) if s == "HELLO"
        ));
    }

    #[test]
    fn capitalize_literal() {
        let mut arena = QueryArena::new();
        let lit = arena.string_literal("hello");
        let result = expand_builtin(&mut arena, BuiltinUtility::Capitalize, &[lit]).unwrap();
        assert!(matches!(
            arena.get(result),
            Node::Literal(SolverLiteral::String(s)) if s == "Hello"
        ));
    }

    #[test]
    fn uncapitalize_literal() {
        let mut arena = QueryArena::new();
        let lit = arena.string_literal("Hello");
        let result = expand_builtin(&mut arena, BuiltinUtility::Uncapitalize, &[lit]).unwrap();
        assert!(matches!(
            arena.get(result),
            Node::Literal(SolverLiteral::String(s)) if s == "hello"
        ));
    }

    #[test]
    fn noinfer_is_identity() {
        let mut arena = QueryArena::new();
        let arg = arena.primitive(PrimitiveKind::String);
        let result = expand_builtin(&mut arena, BuiltinUtility::NoInfer, &[arg]).unwrap();
        assert_eq!(result, arg);
    }

    #[test]
    fn non_nullable_filters_null_and_undefined() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let null = arena.primitive(PrimitiveKind::Null);
        let undef = arena.primitive(PrimitiveKind::Undefined);
        let union = arena.union(vec![s, null, undef]);

        let result = expand_builtin(&mut arena, BuiltinUtility::NonNullable, &[union]).unwrap();
        // Result should be just string (never if only null/undefined)
        assert!(matches!(
            arena.get(result),
            Node::Primitive(PrimitiveKind::String)
        ));
    }

    #[test]
    fn non_nullable_of_just_null_is_never() {
        let mut arena = QueryArena::new();
        let null = arena.primitive(PrimitiveKind::Null);
        let result = expand_builtin(&mut arena, BuiltinUtility::NonNullable, &[null]).unwrap();
        assert!(matches!(
            arena.get(result),
            Node::Primitive(PrimitiveKind::Never)
        ));
    }

    #[test]
    fn uppercase_union_of_literals() {
        let mut arena = QueryArena::new();
        let a = arena.string_literal("hello");
        let b = arena.string_literal("world");
        let union = arena.union(vec![a, b]);

        let result = expand_builtin(&mut arena, BuiltinUtility::Uppercase, &[union]).unwrap();
        // Should be "HELLO" | "WORLD"
        match arena.get(result) {
            Node::Union(members) => {
                assert_eq!(members.len(), 2);
            }
            _ => panic!("expected union"),
        }
    }

    #[test]
    fn wrong_arity_returns_none() {
        let mut arena = QueryArena::new();
        let arg = arena.primitive(PrimitiveKind::String);
        assert!(expand_builtin(&mut arena, BuiltinUtility::Record, &[arg]).is_none());
    }

    fn make_obj(arena: &mut QueryArena, props: &[(&str, NodeId, bool)]) -> NodeId {
        let properties = props
            .iter()
            .map(|(name, ty, optional)| PropertyNode {
                name: name.to_string(),
                ty: *ty,
                optional: *optional,
                readonly: false,
                is_method: false,
            })
            .collect();
        arena.object(ObjectNode {
            properties,
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        })
    }

    #[test]
    fn partial_makes_all_optional() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let obj = make_obj(&mut arena, &[("x", s, false), ("y", s, false)]);

        let result = expand_builtin(&mut arena, BuiltinUtility::Partial, &[obj]).unwrap();
        match arena.get(result) {
            Node::Object(o) => {
                assert!(o.properties.iter().all(|p| p.optional));
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn required_makes_all_non_optional() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let obj = make_obj(&mut arena, &[("x", s, true), ("y", s, true)]);

        let result = expand_builtin(&mut arena, BuiltinUtility::Required, &[obj]).unwrap();
        match arena.get(result) {
            Node::Object(o) => {
                assert!(o.properties.iter().all(|p| !p.optional));
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn readonly_makes_all_readonly() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let obj = make_obj(&mut arena, &[("x", s, false), ("y", s, false)]);

        let result = expand_builtin(&mut arena, BuiltinUtility::Readonly, &[obj]).unwrap();
        match arena.get(result) {
            Node::Object(o) => {
                assert!(o.properties.iter().all(|p| p.readonly));
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn pick_keeps_only_named_keys() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let n = arena.primitive(PrimitiveKind::Number);
        let obj = make_obj(
            &mut arena,
            &[("a", s, false), ("b", n, false), ("c", s, false)],
        );
        let keys = arena.string_literal("a");

        let result = expand_builtin(&mut arena, BuiltinUtility::Pick, &[obj, keys]).unwrap();
        match arena.get(result) {
            Node::Object(o) => {
                assert_eq!(o.properties.len(), 1);
                assert_eq!(o.properties[0].name, "a");
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn omit_removes_named_keys() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let n = arena.primitive(PrimitiveKind::Number);
        let obj = make_obj(
            &mut arena,
            &[("a", s, false), ("b", n, false), ("c", s, false)],
        );
        let keys = arena.string_literal("b");

        let result = expand_builtin(&mut arena, BuiltinUtility::Omit, &[obj, keys]).unwrap();
        match arena.get(result) {
            Node::Object(o) => {
                assert_eq!(o.properties.len(), 2);
                assert!(o.properties.iter().all(|p| p.name != "b"));
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn omit_removes_named_keys_from_intersection_objects() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let left = make_obj(&mut arena, &[("icon", s, false), ("color", s, false)]);
        let right = make_obj(&mut arena, &[("label", s, false)]);
        let obj = arena.intersection(vec![left, right]);
        let keys = arena.string_literal("color");

        let result = expand_builtin(&mut arena, BuiltinUtility::Omit, &[obj, keys]).unwrap();
        match arena.get(result) {
            Node::Object(o) => {
                let names: Vec<_> = o.properties.iter().map(|p| p.name.as_str()).collect();
                assert_eq!(names, vec!["icon", "label"]);
            }
            other => panic!("expected object, got {other:?}"),
        }
    }

    #[test]
    fn record_with_literal_keys() {
        let mut arena = QueryArena::new();
        let a = arena.string_literal("x");
        let b = arena.string_literal("y");
        let keys = arena.union(vec![a, b]);
        let val = arena.primitive(PrimitiveKind::Number);

        let result = expand_builtin(&mut arena, BuiltinUtility::Record, &[keys, val]).unwrap();
        match arena.get(result) {
            Node::Object(o) => {
                assert_eq!(o.properties.len(), 2);
                assert!(o.index_signatures.is_empty());
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn record_with_open_key() {
        let mut arena = QueryArena::new();
        let key = arena.primitive(PrimitiveKind::String);
        let val = arena.primitive(PrimitiveKind::Number);

        let result = expand_builtin(&mut arena, BuiltinUtility::Record, &[key, val]).unwrap();
        match arena.get(result) {
            Node::Object(o) => {
                assert!(o.properties.is_empty());
                assert_eq!(o.index_signatures.len(), 1);
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn extract_keeps_assignable_members() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let n = arena.primitive(PrimitiveKind::Number);
        let b = arena.primitive(PrimitiveKind::Boolean);
        let union = arena.union(vec![s, n, b]);
        let filter = arena.primitive(PrimitiveKind::String);

        let result = expand_builtin(&mut arena, BuiltinUtility::Extract, &[union, filter]).unwrap();
        assert!(matches!(
            arena.get(result),
            Node::Primitive(PrimitiveKind::String)
        ));
    }

    #[test]
    fn exclude_removes_assignable_members() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let n = arena.primitive(PrimitiveKind::Number);
        let b = arena.primitive(PrimitiveKind::Boolean);
        let union = arena.union(vec![s, n, b]);
        let filter = arena.primitive(PrimitiveKind::String);

        let result = expand_builtin(&mut arena, BuiltinUtility::Exclude, &[union, filter]).unwrap();
        match arena.get(result) {
            Node::Union(members) => {
                assert_eq!(members.len(), 2); // number | boolean
            }
            _ => panic!("expected union, got: {:?}", arena.get(result)),
        }
    }

    #[test]
    fn return_type_extracts_return() {
        let mut arena = QueryArena::new();
        let ret = arena.primitive(PrimitiveKind::String);
        let param = arena.primitive(PrimitiveKind::Number);
        let func = arena.function(FunctionNode {
            signatures: vec![CallSignatureNode {
                type_parameters: vec![],
                parameters: vec![ParamNode {
                    name: Some("x".into()),
                    ty: param,
                    optional: false,
                    rest: false,
                }],
                return_type: ret,
            }],
        });

        let result = expand_builtin(&mut arena, BuiltinUtility::ReturnType, &[func]).unwrap();
        assert_eq!(result, ret);
    }

    #[test]
    fn parameters_extracts_tuple() {
        let mut arena = QueryArena::new();
        let p1 = arena.primitive(PrimitiveKind::String);
        let p2 = arena.primitive(PrimitiveKind::Number);
        let ret = arena.primitive(PrimitiveKind::Void);
        let func = arena.function(FunctionNode {
            signatures: vec![CallSignatureNode {
                type_parameters: vec![],
                parameters: vec![
                    ParamNode {
                        name: Some("a".into()),
                        ty: p1,
                        optional: false,
                        rest: false,
                    },
                    ParamNode {
                        name: Some("b".into()),
                        ty: p2,
                        optional: true,
                        rest: false,
                    },
                ],
                return_type: ret,
            }],
        });

        let result = expand_builtin(&mut arena, BuiltinUtility::Parameters, &[func]).unwrap();
        match arena.get(result) {
            Node::Tuple { elements, .. } => {
                assert_eq!(elements.len(), 2);
                assert_eq!(elements[0].ty, p1);
                assert!(elements[1].optional);
            }
            _ => panic!("expected tuple"),
        }
    }

    #[test]
    fn constructor_parameters_extracts_from_construct_signature() {
        // Manual pattern: { new(name: string): Instance }
        let mut arena = QueryArena::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let instance_ty = arena.primitive(PrimitiveKind::Any);
        let obj = arena.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![CallSignatureNode {
                type_parameters: vec![],
                parameters: vec![ParamNode {
                    name: Some("name".into()),
                    ty: str_ty,
                    optional: false,
                    rest: false,
                }],
                return_type: instance_ty,
            }],
        });

        let result =
            expand_builtin(&mut arena, BuiltinUtility::ConstructorParameters, &[obj]).unwrap();
        match arena.get(result) {
            Node::Tuple { elements, .. } => {
                assert_eq!(elements.len(), 1, "should extract 1 constructor param");
                assert_eq!(elements[0].ty, str_ty);
            }
            _ => panic!("expected tuple from ConstructorParameters"),
        }
    }

    #[test]
    fn instance_type_extracts_from_construct_signature() {
        // Manual pattern: { new(): SomeType }
        let mut arena = QueryArena::new();
        let bool_ty = arena.primitive(PrimitiveKind::Boolean);
        let obj = arena.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![CallSignatureNode {
                type_parameters: vec![],
                parameters: vec![],
                return_type: bool_ty,
            }],
        });

        let result = expand_builtin(&mut arena, BuiltinUtility::InstanceType, &[obj]).unwrap();
        match arena.get(result) {
            Node::Primitive(PrimitiveKind::Boolean) => {} // correct
            other => panic!("InstanceType should extract the construct return type, got {other:?}"),
        }
    }
}
