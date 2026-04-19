//! Demand-driven type projections: `member`, `keyspace`, `surface`,
//! `normalize`, and `instantiate`.
//!
//! Each projection operates over the query arena and may return:
//! - exact concrete
//! - exact symbolic
//! - incomplete
//!
//! TODO(D3): this module demotes to a crate-private helper used only
//! inside `build_project_path` per plan §3 D3 + §4 item 10. No
//! workspace code outside the dispatcher imports it post-D3. The
//! `SurfaceShape` / `SurfaceProperty` / `SurfaceCallSignature` /
//! `SurfaceParam` / `SurfaceIndexSignature` types survive as crate-
//! private internal helpers (plan §4 item 19c) but are not publicly
//! re-exported.

use rustc_hash::FxHashMap;

use super::arena::{Node, NodeId, PrimitiveKind, QueryArena, SolverCaches};
use super::result::{Keyspace, SolverExactness, SolverResult};

// ---------------------------------------------------------------------------
// Surface shape
// ---------------------------------------------------------------------------

/// The materialized surface of a type — properties, call signatures, index
/// signatures, and openness.
#[derive(Debug, Clone)]
pub struct SurfaceShape {
    pub properties: Vec<SurfaceProperty>,
    pub call_signatures: Vec<SurfaceCallSignature>,
    pub construct_signatures: Vec<SurfaceCallSignature>,
    pub index_signatures: Vec<SurfaceIndexSignature>,
    /// Whether the surface has an open index domain (e.g. `[key: string]: T`).
    pub is_open: bool,
}

/// A property in a surface shape.
#[derive(Debug, Clone)]
pub struct SurfaceProperty {
    pub name: String,
    pub ty: NodeId,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
}

/// A call/construct signature in a surface shape.
#[derive(Debug, Clone)]
pub struct SurfaceCallSignature {
    pub parameters: Vec<SurfaceParam>,
    pub return_type: NodeId,
}

/// A parameter in a surface signature.
#[derive(Debug, Clone)]
pub struct SurfaceParam {
    pub name: Option<String>,
    pub ty: NodeId,
    pub optional: bool,
    pub rest: bool,
}

/// An index signature in a surface shape.
#[derive(Debug, Clone)]
pub struct SurfaceIndexSignature {
    pub key_type: NodeId,
    pub value_type: NodeId,
    pub readonly: bool,
}

impl SurfaceShape {
    pub fn empty() -> Self {
        Self {
            properties: Vec::new(),
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_signatures: Vec::new(),
            is_open: false,
        }
    }
}

// ---------------------------------------------------------------------------
// member(T, K)
// ---------------------------------------------------------------------------

/// Look up a named member on a type node.
///
/// Requirements:
/// - works without evaluating unrelated siblings
/// - respects intersections and unions
/// - supports exact symbolic residuals
pub fn project_member(
    arena: &mut QueryArena,
    caches: &mut SolverCaches,
    node: NodeId,
    key: &str,
) -> SolverResult<Option<NodeId>> {
    if let Some((cached_node, exactness)) = caches.get_member(node, key) {
        return SolverResult {
            value: Some(cached_node),
            exactness,
            execution_status: super::result::ExecutionStatus::Completed,
            incomplete_reasons: Vec::new(),
            diagnostics: Vec::new(),
            steps: 0,
        };
    }

    match arena.get(node) {
        Node::Object(obj) => {
            if let Some(prop) = obj.properties.iter().find(|p| p.name == key) {
                let ty = prop.ty;
                caches.set_member(node, key.to_string(), ty, SolverExactness::ExactConcrete);
                return SolverResult::exact_concrete(Some(ty));
            }
            for idx_sig in &obj.index_signatures {
                if matches!(
                    arena.get(idx_sig.key_type),
                    Node::Primitive(PrimitiveKind::String)
                ) {
                    let ty = idx_sig.value_type;
                    caches.set_member(node, key.to_string(), ty, SolverExactness::ExactConcrete);
                    return SolverResult::exact_concrete(Some(ty));
                }
            }
            SolverResult::exact_concrete(None)
        }

        Node::Intersection(members) => {
            let members = members.clone(); // Vec<NodeId>, cheap
            for &member in &members {
                let sub = project_member(arena, caches, member, key);
                if sub.value.is_some() {
                    return sub;
                }
            }
            SolverResult::exact_concrete(None)
        }

        Node::Union(members) => {
            let members = members.clone(); // Vec<NodeId>, cheap
            let mut branch_types = Vec::new();
            for &member in &members {
                let sub = project_member(arena, caches, member, key);
                match sub.value {
                    Some(ty) => branch_types.push(ty),
                    None => return SolverResult::exact_concrete(None),
                }
            }
            let union = arena.union(branch_types);
            caches.set_member(node, key.to_string(), union, SolverExactness::ExactConcrete);
            SolverResult::exact_concrete(Some(union))
        }

        Node::Ref { .. } | Node::Applied { .. } => SolverResult::exact_symbolic(None),

        _ => SolverResult::exact_concrete(None),
    }
}

// ---------------------------------------------------------------------------
// keyspace(T)
// ---------------------------------------------------------------------------

/// Compute the keyspace of a type — the set of keys it accepts for indexing.
pub fn project_keyspace(
    arena: &QueryArena,
    caches: &mut SolverCaches,
    node: NodeId,
) -> SolverResult<Keyspace> {
    if let Some(cached) = caches.get_keyspace(node) {
        return SolverResult::exact_concrete(cached.clone());
    }

    let result = match arena.get(node) {
        Node::Object(obj) => {
            let mut keys: Vec<String> = obj.properties.iter().map(|p| p.name.clone()).collect();
            if !obj.index_signatures.is_empty() {
                SolverResult::exact_symbolic(Keyspace::Open)
            } else if keys.is_empty() {
                SolverResult::exact_concrete(Keyspace::Empty)
            } else {
                keys.sort();
                SolverResult::exact_concrete(Keyspace::Finite(keys))
            }
        }

        Node::Union(members) => {
            let members = members.clone();
            let mut common: Option<Vec<String>> = None;
            let mut any_open = false;
            for &member in &members {
                let sub = project_keyspace(arena, caches, member);
                match sub.value {
                    Keyspace::Open => any_open = true,
                    Keyspace::Finite(keys) => {
                        common = Some(match common {
                            None => keys,
                            Some(prev) => prev.into_iter().filter(|k| keys.contains(k)).collect(),
                        });
                    }
                    Keyspace::Empty => common = Some(Vec::new()),
                }
            }
            match (common, any_open) {
                (Some(keys), _) if keys.is_empty() => SolverResult::exact_concrete(Keyspace::Empty),
                (Some(keys), _) => SolverResult::exact_concrete(Keyspace::Finite(keys)),
                (None, true) => SolverResult::exact_symbolic(Keyspace::Open),
                (None, false) => SolverResult::exact_concrete(Keyspace::Empty),
            }
        }

        Node::Intersection(members) => {
            let members = members.clone();
            let mut all_keys = Vec::new();
            let mut any_open = false;
            for &member in &members {
                let sub = project_keyspace(arena, caches, member);
                match sub.value {
                    Keyspace::Open => any_open = true,
                    Keyspace::Finite(keys) => {
                        for key in keys {
                            if !all_keys.contains(&key) {
                                all_keys.push(key);
                            }
                        }
                    }
                    Keyspace::Empty => {}
                }
            }
            if any_open {
                SolverResult::exact_symbolic(Keyspace::Open)
            } else if all_keys.is_empty() {
                SolverResult::exact_concrete(Keyspace::Empty)
            } else {
                all_keys.sort();
                SolverResult::exact_concrete(Keyspace::Finite(all_keys))
            }
        }

        Node::Primitive(PrimitiveKind::Never) => SolverResult::exact_concrete(Keyspace::Empty),
        _ => SolverResult::exact_concrete(Keyspace::Empty),
    };

    caches.set_keyspace(node, result.value.clone());
    result
}

// ---------------------------------------------------------------------------
// surface(T)
// ---------------------------------------------------------------------------

/// Produce the materialized surface of a type — properties, call signatures,
/// index signatures, and openness.
///
/// Does not require full normalization of every nested child.
pub fn project_surface(arena: &mut QueryArena, node: NodeId) -> SolverResult<SurfaceShape> {
    match arena.get(node) {
        Node::Function(func) => {
            let call_signatures = func
                .signatures
                .iter()
                .map(|sig| SurfaceCallSignature {
                    parameters: sig
                        .parameters
                        .iter()
                        .map(|p| SurfaceParam {
                            name: p.name.clone(),
                            ty: p.ty,
                            optional: p.optional,
                            rest: p.rest,
                        })
                        .collect(),
                    return_type: sig.return_type,
                })
                .collect();

            SolverResult::exact_concrete(SurfaceShape {
                properties: Vec::new(),
                call_signatures,
                construct_signatures: Vec::new(),
                index_signatures: Vec::new(),
                is_open: false,
            })
        }

        Node::Object(obj) => {
            let properties = obj
                .properties
                .iter()
                .map(|p| SurfaceProperty {
                    name: p.name.clone(),
                    ty: p.ty,
                    optional: p.optional,
                    readonly: p.readonly,
                    is_method: p.is_method,
                })
                .collect();
            let call_signatures = obj
                .call_signatures
                .iter()
                .map(|sig| SurfaceCallSignature {
                    parameters: sig
                        .parameters
                        .iter()
                        .map(|p| SurfaceParam {
                            name: p.name.clone(),
                            ty: p.ty,
                            optional: p.optional,
                            rest: p.rest,
                        })
                        .collect(),
                    return_type: sig.return_type,
                })
                .collect();
            let construct_signatures = obj
                .construct_signatures
                .iter()
                .map(|sig| SurfaceCallSignature {
                    parameters: sig
                        .parameters
                        .iter()
                        .map(|p| SurfaceParam {
                            name: p.name.clone(),
                            ty: p.ty,
                            optional: p.optional,
                            rest: p.rest,
                        })
                        .collect(),
                    return_type: sig.return_type,
                })
                .collect();
            let index_signatures = obj
                .index_signatures
                .iter()
                .map(|idx| SurfaceIndexSignature {
                    key_type: idx.key_type,
                    value_type: idx.value_type,
                    readonly: idx.readonly,
                })
                .collect::<Vec<_>>();
            let is_open = !obj.index_signatures.is_empty();

            SolverResult::exact_concrete(SurfaceShape {
                properties,
                call_signatures,
                construct_signatures,
                index_signatures,
                is_open,
            })
        }

        Node::Intersection(members) => {
            let members = members.clone(); // Vec<NodeId>, cheap
            let mut merged = SurfaceShape::empty();
            let mut merged_props: FxHashMap<String, SurfaceProperty> = FxHashMap::default();
            let mut exactness = SolverExactness::ExactConcrete;

            for &member in &members {
                let sub = project_surface(arena, member);
                exactness = exactness.merge(sub.exactness);
                for prop in sub.value.properties {
                    merged_props.entry(prop.name.clone()).or_insert(prop);
                }
                merged.call_signatures.extend(sub.value.call_signatures);
                merged
                    .construct_signatures
                    .extend(sub.value.construct_signatures);
                merged.index_signatures.extend(sub.value.index_signatures);
                merged.is_open = merged.is_open || sub.value.is_open;
            }

            merged.properties = merged_props.into_values().collect();
            merged.properties.sort_by(|a, b| a.name.cmp(&b.name));

            SolverResult {
                value: merged,
                exactness,
                execution_status: super::result::ExecutionStatus::Completed,
                incomplete_reasons: Vec::new(),
                diagnostics: Vec::new(),
                steps: 0,
            }
        }

        // Unresolved — return empty symbolic surface
        Node::Union(members) => {
            let members = members.clone(); // Vec<NodeId>, cheap
            let mut merged = SurfaceShape::empty();
            let mut merged_props: FxHashMap<String, (SurfaceProperty, usize)> =
                FxHashMap::default();
            let mut exactness = SolverExactness::ExactConcrete;
            let mut total_surface_variants = 0usize;

            for &member in &members {
                let sub = project_surface(arena, member);
                exactness = exactness.merge(sub.exactness);
                let shape = sub.value;
                let has_surface = !shape.properties.is_empty()
                    || !shape.call_signatures.is_empty()
                    || !shape.construct_signatures.is_empty()
                    || !shape.index_signatures.is_empty()
                    || shape.is_open;
                if !has_surface {
                    continue;
                }
                total_surface_variants += 1;
                for prop in shape.properties {
                    match merged_props.entry(prop.name.clone()) {
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert((prop, 1));
                        }
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            let (existing, count) = entry.get_mut();
                            *count += 1;
                            existing.optional = existing.optional || prop.optional;
                            existing.readonly = existing.readonly && prop.readonly;
                            existing.is_method = existing.is_method && prop.is_method;
                            if existing.ty != prop.ty {
                                existing.ty = arena.union(vec![existing.ty, prop.ty]);
                            }
                        }
                    }
                }
                merged.call_signatures.extend(shape.call_signatures);
                merged
                    .construct_signatures
                    .extend(shape.construct_signatures);
                merged.index_signatures.extend(shape.index_signatures);
                merged.is_open = merged.is_open || shape.is_open;
            }

            merged.properties = merged_props
                .into_values()
                .map(|(mut prop, seen_variants)| {
                    if seen_variants < total_surface_variants {
                        prop.optional = true;
                    }
                    prop
                })
                .collect();
            merged.properties.sort_by(|a, b| a.name.cmp(&b.name));

            SolverResult {
                value: merged,
                exactness,
                execution_status: super::result::ExecutionStatus::Completed,
                incomplete_reasons: Vec::new(),
                diagnostics: Vec::new(),
                steps: 0,
            }
        }

        Node::Ref { .. } | Node::Applied { .. } => {
            SolverResult::exact_symbolic(SurfaceShape::empty())
        }

        _ => SolverResult::exact_concrete(SurfaceShape::empty()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::arena::*;
    use super::*;

    fn make_obj_with_props(arena: &mut QueryArena, props: &[(&str, NodeId, bool)]) -> NodeId {
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
    fn member_lookup_on_object() {
        let mut arena = QueryArena::new();
        let mut caches = SolverCaches::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let obj = make_obj_with_props(&mut arena, &[("x", str_ty, false), ("y", str_ty, true)]);

        let result = project_member(&mut arena, &mut caches, obj, "x");
        assert_eq!(result.value, Some(str_ty));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);

        let result = project_member(&mut arena, &mut caches, obj, "missing");
        assert!(result.value.is_none());
    }

    #[test]
    fn member_lookup_on_union_requires_all_branches() {
        let mut arena = QueryArena::new();
        let mut caches = SolverCaches::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let num_ty = arena.primitive(PrimitiveKind::Number);

        let obj_a = make_obj_with_props(&mut arena, &[("x", str_ty, false)]);
        let obj_b = make_obj_with_props(&mut arena, &[("x", num_ty, false)]);
        let union = arena.union(vec![obj_a, obj_b]);

        let result = project_member(&mut arena, &mut caches, union, "x");
        assert!(result.value.is_some());
    }

    #[test]
    fn member_lookup_on_union_fails_if_missing_branch() {
        let mut arena = QueryArena::new();
        let mut caches = SolverCaches::new();
        let str_ty = arena.primitive(PrimitiveKind::String);

        let obj_a = make_obj_with_props(&mut arena, &[("x", str_ty, false)]);
        let obj_b = make_obj_with_props(&mut arena, &[]);
        let union = arena.union(vec![obj_a, obj_b]);

        let result = project_member(&mut arena, &mut caches, union, "x");
        assert!(result.value.is_none());
    }

    #[test]
    fn member_lookup_on_intersection_first_hit() {
        let mut arena = QueryArena::new();
        let mut caches = SolverCaches::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let num_ty = arena.primitive(PrimitiveKind::Number);

        let obj_a = make_obj_with_props(&mut arena, &[("x", str_ty, false)]);
        let obj_b = make_obj_with_props(&mut arena, &[("y", num_ty, false)]);
        let inter = arena.intersection(vec![obj_a, obj_b]);

        let result = project_member(&mut arena, &mut caches, inter, "x");
        assert_eq!(result.value, Some(str_ty));

        let result = project_member(&mut arena, &mut caches, inter, "y");
        assert_eq!(result.value, Some(num_ty));
    }

    #[test]
    fn keyspace_of_object() {
        let mut arena = QueryArena::new();
        let mut caches = SolverCaches::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let obj = make_obj_with_props(&mut arena, &[("a", str_ty, false), ("b", str_ty, false)]);

        let result = project_keyspace(&arena, &mut caches, obj);
        assert_eq!(result.value, Keyspace::Finite(vec!["a".into(), "b".into()]));
    }

    #[test]
    fn keyspace_of_object_with_index_is_open() {
        let mut arena = QueryArena::new();
        let mut caches = SolverCaches::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let obj = arena.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![IndexSignatureNode {
                key_type: str_ty,
                value_type: str_ty,
                readonly: false,
            }],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        let result = project_keyspace(&arena, &mut caches, obj);
        assert_eq!(result.value, Keyspace::Open);
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
    }

    #[test]
    fn keyspace_of_empty_object() {
        let mut arena = QueryArena::new();
        let mut caches = SolverCaches::new();
        let obj = arena.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        let result = project_keyspace(&arena, &mut caches, obj);
        assert_eq!(result.value, Keyspace::Empty);
    }

    #[test]
    fn keyspace_of_never_is_empty() {
        let mut arena = QueryArena::new();
        let mut caches = SolverCaches::new();
        let never = arena.primitive(PrimitiveKind::Never);
        let result = project_keyspace(&arena, &mut caches, never);
        assert_eq!(result.value, Keyspace::Empty);
    }

    #[test]
    fn surface_of_object() {
        let mut arena = QueryArena::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let obj = make_obj_with_props(&mut arena, &[("x", str_ty, false)]);

        let result = project_surface(&mut arena, obj);
        assert_eq!(result.value.properties.len(), 1);
        assert_eq!(result.value.properties[0].name, "x");
        assert!(!result.value.is_open);
    }

    #[test]
    fn surface_of_intersection_merges() {
        let mut arena = QueryArena::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let num_ty = arena.primitive(PrimitiveKind::Number);

        let obj_a = make_obj_with_props(&mut arena, &[("x", str_ty, false)]);
        let obj_b = make_obj_with_props(&mut arena, &[("y", num_ty, false)]);
        let inter = arena.intersection(vec![obj_a, obj_b]);

        let result = project_surface(&mut arena, inter);
        assert_eq!(result.value.properties.len(), 2);
        assert!(!result.value.is_open);
    }

    #[test]
    fn surface_of_union_keeps_common_and_branch_props() {
        let mut arena = QueryArena::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let num_ty = arena.primitive(PrimitiveKind::Number);

        let obj_a = make_obj_with_props(
            &mut arena,
            &[("shared", str_ty, false), ("bubble", num_ty, false)],
        );
        let obj_b = make_obj_with_props(
            &mut arena,
            &[("shared", str_ty, false), ("floating", num_ty, false)],
        );
        let union = arena.union(vec![obj_a, obj_b]);

        let result = project_surface(&mut arena, union);
        let props: Vec<_> = result
            .value
            .properties
            .iter()
            .map(|prop| (prop.name.as_str(), prop.optional))
            .collect();

        assert!(
            props.contains(&("shared", false)),
            "shared union props should stay required, got {props:?}"
        );
        assert!(
            props.contains(&("bubble", true)) && props.contains(&("floating", true)),
            "branch-only union props should remain visible but optional, got {props:?}"
        );
    }

    #[test]
    fn member_via_index_signature() {
        let mut arena = QueryArena::new();
        let mut caches = SolverCaches::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let num_ty = arena.primitive(PrimitiveKind::Number);
        let obj = arena.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![IndexSignatureNode {
                key_type: str_ty,
                value_type: num_ty,
                readonly: false,
            }],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        let result = project_member(&mut arena, &mut caches, obj, "anything");
        assert_eq!(result.value, Some(num_ty));
    }
}
