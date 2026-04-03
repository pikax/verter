//! Top-level solver entry point: `solve_type`.
//!
//! Takes a `TypeExpr` (from a prepared declaration) and the host, creates a
//! query-local arena, lowers the expression, and resolves references through
//! the host. Returns a `SolverResult<TypeExpr>` projected back from the arena.

use std::sync::Arc;

use super::arena::{Node, NodeId, PrimitiveKind, QueryArena};
use super::builtin::{expand_builtin, BuiltinUtility};
use super::display::display_node;
use super::host::{RequestStatus, ResolvedRootIdentity, TypeSolverHost, UtilitySource};
use super::lower::lower_type_expr;
use super::recursion::{RecursionKey, RecursionTracker};
use super::result::{ExecutionStatus, IncompleteReason, SolverExactness, SolverResult};
use super::substitution::SubstitutionEnv;
use crate::analysis::type_expr::TypeExpr;

// ---------------------------------------------------------------------------
// Operational limits
// ---------------------------------------------------------------------------

/// Operational limits for a solver query (generous TypeScript-like ceilings).
#[derive(Debug, Clone)]
pub struct SolveLimits {
    /// Maximum instantiation depth for nested generic resolution.
    pub max_instantiation_depth: u32,
    /// Maximum total resolve steps per query.
    pub max_resolve_steps: u64,
    /// Maximum nodes in the arena before hard stop.
    pub max_arena_nodes: u64,
}

impl Default for SolveLimits {
    fn default() -> Self {
        Self {
            max_instantiation_depth: 50,
            max_resolve_steps: 100_000,
            max_arena_nodes: 500_000,
        }
    }
}

/// Mutable solver state for a single query.
pub struct SolveState {
    pub depth: u32,
    pub steps: u64,
    pub limits: SolveLimits,
    pub recursion: RecursionTracker,
    pub exactness: SolverExactness,
    pub execution_status: ExecutionStatus,
    pub incomplete_reasons: Vec<IncompleteReason>,
    /// Stack of active type declaration contexts. When resolving a prepared
    /// type declaration body, the declaration is pushed onto this stack so
    /// bare name refs can be resolved through the declaration's
    /// `name_resolution` map (defining file scope).
    pub type_decl_context_stack: Vec<Arc<super::PreparedTypeDecl>>,
    /// Stack of active value declaration contexts for `typeof` resolution.
    pub value_decl_context_stack: Vec<Arc<super::PreparedValueDecl>>,
    /// External declarations visited during this solve. Recorded by
    /// `resolve_prepared_ref` when it enters a declaration from a
    /// canonical file other than "$owner". Used by the host to publish
    /// import aliases to the type registry.
    pub visited_external_decls: Vec<ResolvedRootIdentity>,
    /// Active substitution names currently being expanded.
    ///
    /// This guards self-referential default/type-parameter substitutions like
    /// `T = NestedItem<I>` where `I` is itself bound to an unresolved `T`,
    /// which would otherwise recurse forever before prepared-ref recursion
    /// tracking has a chance to run.
    pub active_substitution_names: Vec<String>,
}

impl SolveState {
    pub fn new(limits: SolveLimits) -> Self {
        Self {
            depth: 0,
            steps: 0,
            limits,
            recursion: RecursionTracker::new(),
            exactness: SolverExactness::ExactConcrete,
            execution_status: ExecutionStatus::Completed,
            incomplete_reasons: Vec::new(),
            type_decl_context_stack: Vec::new(),
            value_decl_context_stack: Vec::new(),
            visited_external_decls: Vec::new(),
            active_substitution_names: Vec::new(),
        }
    }

    /// Check operational limits. Returns true if any limit is exceeded.
    pub fn is_exceeded(&self) -> bool {
        self.depth > self.limits.max_instantiation_depth
            || self.steps > self.limits.max_resolve_steps
    }

    /// Increment step counter.
    pub fn step(&mut self) -> bool {
        self.steps += 1;
        self.is_exceeded()
    }

    /// Record incomplete status.
    pub fn mark_incomplete(&mut self, reason: IncompleteReason) {
        self.exactness = SolverExactness::Incomplete;
        self.incomplete_reasons.push(reason);
    }

    /// Record symbolic status (not incomplete, but not fully concrete).
    pub fn mark_symbolic(&mut self) {
        if self.exactness == SolverExactness::ExactConcrete {
            self.exactness = SolverExactness::ExactSymbolic;
        }
    }
}

// ---------------------------------------------------------------------------
// solve_type — top-level entry point
// ---------------------------------------------------------------------------

/// Solve (normalize/expand) a `TypeExpr` using the host for cross-file
/// declaration resolution.
///
/// Creates a query-local arena, lowers the expression, resolves references,
/// and projects the result back to `TypeExpr`.
pub fn solve_type(
    expr: &TypeExpr,
    host: &dyn TypeSolverHost,
    limits: SolveLimits,
) -> SolverResult<TypeExpr> {
    let mut arena = QueryArena::new();
    let mut state = SolveState::new(limits);

    // Lower the input expression into the arena
    let root = lower_type_expr(&mut arena, expr);

    // Resolve references in the arena through the host
    let resolved = resolve_node(&mut arena, root, host, &mut state, &SubstitutionEnv::new());

    // Project the resolved node back to TypeExpr
    let result_expr = project_to_type_expr(&arena, resolved);

    // Debug-only: log solver stats BEFORE moving state
    if solver_debug_enabled() {
        let json_bytes = serde_json::to_string(&result_expr)
            .map(|s| s.len())
            .unwrap_or(0);
        eprintln!(
            "[verter-solver] nodes={} steps={} depth={} payload={}B exactness={} status={}",
            arena.len(),
            state.steps,
            state.recursion.max_depth(),
            json_bytes,
            state.exactness,
            state.execution_status,
        );
    }

    SolverResult {
        value: result_expr,
        exactness: state.exactness,
        execution_status: state.execution_status,
        incomplete_reasons: state.incomplete_reasons,
    }
}

/// Stats from a solver run — always cheap to compute (counter reads).
#[derive(Debug, Clone)]
pub struct SolverStats {
    pub arena_nodes: usize,
    pub resolve_steps: u64,
    pub max_instantiation_depth: usize,
    pub result_json_bytes: usize,
    pub exactness: SolverExactness,
    pub execution_status: ExecutionStatus,
}

/// Compute solver stats from a completed run.
pub fn compute_solver_stats(
    result: &SolverResult<TypeExpr>,
    arena: &QueryArena,
    state: &SolveState,
) -> SolverStats {
    let result_json_bytes = serde_json::to_string(&result.value)
        .map(|s| s.len())
        .unwrap_or(0);
    SolverStats {
        arena_nodes: arena.len(),
        resolve_steps: state.steps,
        max_instantiation_depth: state.recursion.max_depth(),
        result_json_bytes,
        exactness: result.exactness,
        execution_status: result.execution_status,
    }
}

fn solver_debug_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("VERTER_SOLVER_DEBUG").is_some())
}

#[allow(dead_code)]
fn solver_debug_log(result: &SolverResult<TypeExpr>, arena: &QueryArena, state: &SolveState) {
    if !solver_debug_enabled() {
        return;
    }
    let stats = compute_solver_stats(result, arena, state);
    eprintln!(
        "[verter-solver] nodes={} steps={} depth={} payload={}B exactness={} status={}",
        stats.arena_nodes,
        stats.resolve_steps,
        stats.max_instantiation_depth,
        stats.result_json_bytes,
        stats.exactness,
        stats.execution_status,
    );
}

/// Solve a type expression and return both the result and a trace of
/// external declarations visited during resolution. The trace is a
/// sidecar for the orchestration layer (registry publishing) and is
/// NOT part of the semantic `SolverResult`.
pub fn solve_type_with_trace(
    expr: &TypeExpr,
    host: &dyn TypeSolverHost,
    limits: SolveLimits,
) -> (SolverResult<TypeExpr>, Vec<ResolvedRootIdentity>) {
    let mut arena = QueryArena::new();
    let mut state = SolveState::new(limits);

    let root = lower_type_expr(&mut arena, expr);
    let resolved = resolve_node(&mut arena, root, host, &mut state, &SubstitutionEnv::new());
    let result_expr = project_to_type_expr(&arena, resolved);

    let trace = state.visited_external_decls;
    (
        SolverResult {
            value: result_expr,
            exactness: state.exactness,
            execution_status: state.execution_status,
            incomplete_reasons: state.incomplete_reasons,
        },
        trace,
    )
}

// ---------------------------------------------------------------------------
// resolve_node — the recursive resolver
// ---------------------------------------------------------------------------

/// Resolve a node in the arena, expanding references through the host.
fn resolve_node(
    arena: &mut QueryArena,
    node: NodeId,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> NodeId {
    // Check cancellation
    if host.request_status() == RequestStatus::Cancelled {
        state.execution_status = ExecutionStatus::Cancelled;
        return node;
    }

    // Check operational limits (steps + arena size)
    if state.step() || (arena.len() as u64) > state.limits.max_arena_nodes {
        state.execution_status = ExecutionStatus::HardStop;
        state.mark_incomplete(IncompleteReason::UnsupportedSyntax {
            description: "resolve step or arena size limit exceeded".into(),
        });
        return node;
    }

    // Fast path: terminals and simple lookups — no clone needed.
    match arena.get(node) {
        Node::Primitive(_) | Node::Literal(_) | Node::Error { .. } => return node,
        Node::RecursiveRef { .. } => return node,
        Node::TypeParam { ref name, .. } => {
            let name = name.clone();
            if let Some(bound) = subst.resolve(&name) {
                if state
                    .active_substitution_names
                    .iter()
                    .any(|active| active == &name)
                {
                    state.mark_symbolic();
                    return node;
                }

                state.active_substitution_names.push(name);
                let resolved = resolve_node(arena, bound, host, state, subst);
                state.active_substitution_names.pop();
                return resolved;
            } else {
                state.mark_symbolic();
                return node;
            }
        }
        Node::Infer { .. } => {
            state.mark_symbolic();
            return node;
        }
        _ => {}
    }

    // Compound/operator nodes — clone to release borrow, then recurse.
    let node_data = arena.get(node).clone();
    match node_data {
        // Union — resolve each member
        Node::Union(members) => {
            let resolved: Vec<NodeId> = members
                .iter()
                .map(|&m| resolve_node(arena, m, host, state, subst))
                .collect();
            arena.union(resolved)
        }

        // Intersection — resolve each member
        Node::Intersection(members) => {
            let resolved: Vec<NodeId> = members
                .iter()
                .map(|&m| resolve_node(arena, m, host, state, subst))
                .collect();
            let mut simplified = Vec::with_capacity(resolved.len());
            let mut saw_empty_object = false;

            for member in resolved {
                match arena.get(member) {
                    Node::Primitive(PrimitiveKind::Never) => return member,
                    Node::Object(obj)
                        if obj.properties.is_empty()
                            && obj.index_signatures.is_empty()
                            && obj.call_signatures.is_empty()
                            && obj.construct_signatures.is_empty() =>
                    {
                        saw_empty_object = true;
                    }
                    _ => simplified.push(member),
                }
            }

            if simplified.is_empty() && saw_empty_object {
                arena.object(super::arena::ObjectNode {
                    properties: vec![],
                    index_signatures: vec![],
                    call_signatures: vec![],
                    construct_signatures: vec![],
                })
            } else {
                arena.intersection(simplified)
            }
        }

        // Array — resolve element
        Node::Array { element, readonly } => {
            let el = resolve_node(arena, element, host, state, subst);
            arena.array(el, readonly)
        }

        // Tuple — resolve elements
        Node::Tuple { elements, readonly } => {
            let els: Vec<_> = elements
                .into_iter()
                .map(|mut el| {
                    el.ty = resolve_node(arena, el.ty, host, state, subst);
                    el
                })
                .collect();
            arena.alloc(Node::Tuple {
                elements: els,
                readonly,
            })
        }

        // Object — resolve property types
        Node::Object(mut obj) => {
            for prop in &mut obj.properties {
                prop.ty = resolve_node(arena, prop.ty, host, state, subst);
            }
            for idx in &mut obj.index_signatures {
                idx.key_type = resolve_node(arena, idx.key_type, host, state, subst);
                idx.value_type = resolve_node(arena, idx.value_type, host, state, subst);
            }
            for sig in &mut obj.call_signatures {
                sig.return_type = resolve_node(arena, sig.return_type, host, state, subst);
                for param in &mut sig.parameters {
                    param.ty = resolve_node(arena, param.ty, host, state, subst);
                }
            }
            for sig in &mut obj.construct_signatures {
                sig.return_type = resolve_node(arena, sig.return_type, host, state, subst);
                for param in &mut sig.parameters {
                    param.ty = resolve_node(arena, param.ty, host, state, subst);
                }
            }
            arena.object(obj)
        }

        // Function — resolve parameter and return types
        Node::Function(mut func) => {
            for sig in &mut func.signatures {
                sig.return_type = resolve_node(arena, sig.return_type, host, state, subst);
                for param in &mut sig.parameters {
                    param.ty = resolve_node(arena, param.ty, host, state, subst);
                }
            }
            arena.function(func)
        }

        // Ref — look up through the host and instantiate
        Node::Ref {
            ref name,
            ref type_arguments,
        } => {
            let name = name.clone();
            let args = type_arguments.clone();

            // Check if it's a built-in utility type.
            // Compiler intrinsics (Uppercase etc.) are never shadowable.
            // Other builtins are only expanded if the host confirms they're
            // not shadowed by user declarations.
            if let Some(builtin) = BuiltinUtility::from_name(&name) {
                let should_expand = builtin.is_compiler_intrinsic()
                    || host.utility_source(&name) != UtilitySource::Shadowed;

                if should_expand {
                    let resolved_args: Vec<NodeId> = args
                        .iter()
                        .map(|&a| resolve_node(arena, a, host, state, subst))
                        .collect();

                    if let Some(expanded) = expand_builtin(arena, builtin, &resolved_args) {
                        return resolve_node(arena, expanded, host, state, subst);
                    }
                }
            }

            // Check substitution env (for generic type params used as refs)
            if args.is_empty() {
                if let Some(bound) = subst.resolve(&name) {
                    if let Some(guarded) = resolve_substitution_binding(
                        arena,
                        node,
                        name.as_ref(),
                        bound,
                        host,
                        state,
                        subst,
                    ) {
                        return guarded;
                    }
                    return resolve_node(arena, bound, host, state, subst);
                }
            }

            // Resolve type arguments first
            let resolved_args: Vec<NodeId> = args
                .iter()
                .map(|&a| resolve_node(arena, a, host, state, subst))
                .collect();

            // Try to resolve from the host's prepared declarations.
            // First check the active declaration context — bare names in an
            // imported type body should resolve through the defining file's
            // scope (name_resolution), not the owner file's scope.
            let maybe_root =
                resolve_name_in_context(state, &name).or_else(|| host.root_identity("", &name));
            if let Some(root_id) = maybe_root {
                return resolve_prepared_ref(arena, host, state, subst, &root_id, &resolved_args);
            }

            // Host can't resolve — keep as symbolic ref
            if resolved_args != args {
                // Args changed, rebuild
                arena.type_ref(name, resolved_args)
            } else {
                state.mark_symbolic();
                node
            }
        }

        // Applied — already instantiated, resolve body
        Node::Applied { .. } => {
            state.mark_symbolic();
            node
        }

        // -- keyof --
        Node::KeyOf(operand) => {
            let resolved_operand = resolve_node(arena, operand, host, state, subst);
            resolve_keyof(arena, resolved_operand, state)
        }

        // -- indexed access T[K] --
        Node::IndexedAccess { object, index } => {
            if let Node::Literal(super::arena::SolverLiteral::String(key)) =
                arena.get(index).clone()
            {
                if let Node::Ref {
                    ref name,
                    ref type_arguments,
                } = arena.get(object).clone()
                {
                    if type_arguments.is_empty() {
                        let maybe_root = resolve_name_in_context(state, name.as_str())
                            .or_else(|| host.root_identity("", name.as_str()));
                        if let Some(root_id) = maybe_root {
                            let prepared_context = host.resolve_prepared_type_decl(&root_id);
                            if let Some(projection) =
                                host.resolve_member_projection(&root_id, key.as_str())
                            {
                                if projection.exactness == SolverExactness::ExactSymbolic {
                                    state.mark_symbolic();
                                }
                                let lowered = lower_type_expr(arena, &projection.value);
                                let pushed = if let Some(prepared) = prepared_context.as_ref() {
                                    if !prepared.name_resolution.is_empty() {
                                        state.type_decl_context_stack.push(Arc::clone(prepared));
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                };
                                let resolved = resolve_node(arena, lowered, host, state, subst);
                                if pushed {
                                    state.type_decl_context_stack.pop();
                                }
                                return resolved;
                            }
                        }
                    }
                }
            }

            let resolved_obj = resolve_node(arena, object, host, state, subst);
            let resolved_idx = resolve_node(arena, index, host, state, subst);
            resolve_indexed_access(arena, resolved_obj, resolved_idx, host, state, subst)
        }

        // -- conditional T extends U ? A : B --
        Node::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
        } => {
            let resolved_check = resolve_node(arena, check, host, state, subst);
            let resolved_extends = resolve_node(arena, extends, host, state, subst);
            resolve_conditional(
                arena,
                resolved_check,
                resolved_extends,
                true_branch,
                false_branch,
                distributive,
                host,
                state,
                subst,
            )
        }

        // -- mapped type { [K in Source]: Value } --
        Node::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            let resolved_source = resolve_node(arena, source, host, state, subst);
            resolve_mapped(
                arena,
                &parameter,
                resolved_source,
                value,
                optional,
                readonly,
                name_type,
                host,
                state,
                subst,
            )
        }

        // -- typeof --
        Node::TypeOf { path } => resolve_typeof(arena, &path, host, state, subst),

        // -- template literal `prefix${T}suffix` --
        Node::TemplateLiteral {
            quasis,
            expressions,
        } => resolve_template_literal(arena, &quasis, &expressions, host, state, subst),

        // -- rest --
        Node::Rest(inner) => {
            let resolved = resolve_node(arena, inner, host, state, subst);
            arena.alloc(Node::Rest(resolved))
        }

        // Terminals handled by fast path above — catch-all for safety
        _ => node,
    }
}

fn resolve_substitution_binding(
    arena: &mut QueryArena,
    original_node: NodeId,
    name: &str,
    bound: NodeId,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> Option<NodeId> {
    if state
        .active_substitution_names
        .iter()
        .any(|active| active == name)
    {
        state.mark_symbolic();
        return Some(original_node);
    }

    state.active_substitution_names.push(name.to_string());
    let resolved = resolve_node(arena, bound, host, state, subst);
    state.active_substitution_names.pop();
    Some(resolved)
}

// ---------------------------------------------------------------------------
// resolve_prepared_ref — instantiate a host-backed prepared declaration
// ---------------------------------------------------------------------------

/// Look up a prepared type declaration from the host, lower its body into the
/// arena, bind type parameters to the resolved arguments, and resolve the
/// body recursively.
fn resolve_prepared_ref(
    arena: &mut QueryArena,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    parent_subst: &SubstitutionEnv,
    root_id: &ResolvedRootIdentity,
    args: &[NodeId],
) -> NodeId {
    // Check recursion — have we already started resolving this exact
    // (identity, args) combination?
    let rec_key = RecursionKey {
        canonical_id: root_id.canonical_id.clone(),
        symbol_name: root_id.symbol_name.clone(),
        args_hash: hash_node_ids(args),
    };

    if let Some(placeholder) = state.recursion.enter(rec_key.clone()) {
        // Cycle detected — return the recursive placeholder
        return placeholder;
    }

    // Depth guard
    state.depth += 1;
    if state.depth > state.limits.max_instantiation_depth {
        state.depth -= 1;
        state.mark_incomplete(IncompleteReason::RecursionPolicy {
            description: format!(
                "instantiation depth {} exceeded for {}",
                state.limits.max_instantiation_depth, root_id
            ),
        });
        state.execution_status = ExecutionStatus::HardStop;
        return arena.error(format!("depth exceeded: {}", root_id));
    }

    // Create a recursive placeholder in case the body references itself
    let placeholder = arena.alloc(Node::RecursiveRef {
        target: NodeId::UNRESOLVED,
    });
    state.recursion.push(rec_key.clone(), placeholder);

    // Record external declaration visit for registry publishing
    if !root_id.canonical_id.is_empty() && root_id.canonical_id != "$owner" {
        state.visited_external_decls.push(root_id.clone());
    }

    // Look up the prepared declaration
    let Some(prepared) = host.resolve_prepared_type_decl(root_id) else {
        state.recursion.pop(&rec_key);
        state.depth -= 1;
        state.mark_incomplete(IncompleteReason::MissingSource {
            canonical_id: root_id.canonical_id.clone(),
            symbol_name: root_id.symbol_name.clone(),
        });
        return arena.error(format!("missing: {}", root_id));
    };

    // Lower the declaration body into the arena
    let body_node = lower_type_expr(arena, &prepared.body);

    // Build substitution: bind type params to resolved args
    let param_names: Vec<String> = prepared
        .type_parameters
        .iter()
        .map(|p| p.name.clone())
        .collect();

    let mut child_subst = parent_subst.clone();
    for (i, param_name) in param_names.iter().enumerate() {
        if let Some(&arg) = args.get(i) {
            child_subst.bind(param_name, arg);
        } else if let Some(ref default) = prepared.type_parameters[i].default {
            // Use default type argument if not supplied
            let default_node = lower_type_expr(arena, default);
            child_subst.bind(param_name, default_node);
        }
    }

    // Push the prepared declaration onto the context stack so bare-name
    // refs in the body can be resolved through name_resolution.
    // Only push if the declaration has name_resolution entries (avoids
    // empty stack entries that would be checked by resolve_name_in_context).
    let pushed = if !prepared.name_resolution.is_empty() {
        state.type_decl_context_stack.push(Arc::clone(&prepared));
        true
    } else {
        false
    };

    // Resolve the body with the new substitution
    let resolved = resolve_node(arena, body_node, host, state, &child_subst);

    // Pop declaration context and recursion tracker
    if pushed {
        state.type_decl_context_stack.pop();
    }
    state.recursion.pop(&rec_key);
    state.depth -= 1;

    resolved
}

/// Check the INNERMOST active declaration context for a pre-resolved name.
///
/// Only checks the topmost entry on the type/value declaration context
/// stacks. A bare name in a declaration body should resolve in THAT
/// declaration's defining file scope only — not in parent scopes from
/// outer prepared-ref resolutions. The host's `root_identity` handles
/// owner-level resolution as the fallback.
fn resolve_name_in_context(state: &SolveState, name: &str) -> Option<ResolvedRootIdentity> {
    // Check innermost type declaration context only
    if let Some(decl) = state.type_decl_context_stack.last() {
        if let Some(identity) = decl.name_resolution.get(name) {
            return Some(identity.clone());
        }
    }
    // Then check innermost value declaration context only
    if let Some(decl) = state.value_decl_context_stack.last() {
        if let Some(identity) = decl.name_resolution.get(name) {
            return Some(identity.clone());
        }
    }
    None
}

/// Simple hash for a slice of NodeIds (for recursion tracking keys).
fn hash_node_ids(ids: &[NodeId]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ids.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// resolve_keyof
// ---------------------------------------------------------------------------

/// `keyof T` — produce the key union of the resolved operand.
///
/// Reads the operand node, extracts names/ids into locals, then allocates
/// new nodes. This is the standard Rust read-then-write pattern for a
/// struct that is both read and mutated.
fn resolve_keyof(arena: &mut QueryArena, operand: NodeId, state: &mut SolveState) -> NodeId {
    // Read phase: extract what we need into owned locals.
    let node = arena.get(operand).clone();

    match node {
        Node::Array { .. } => arena.primitive(PrimitiveKind::Number),
        Node::Tuple { elements, .. } => {
            if elements.is_empty() {
                arena.primitive(PrimitiveKind::Never)
            } else {
                let keys: Vec<NodeId> = elements
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| arena.number_literal(idx as f64))
                    .collect();
                arena.union(keys)
            }
        }
        Node::Object(obj) => {
            let has_index = !obj.index_signatures.is_empty();
            if has_index {
                let mut all: Vec<NodeId> =
                    obj.index_signatures.iter().map(|i| i.key_type).collect();
                for p in &obj.properties {
                    all.push(arena.string_literal(&p.name));
                }
                arena.union(all)
            } else if obj.properties.is_empty() {
                arena.primitive(PrimitiveKind::Never)
            } else {
                let keys: Vec<NodeId> = obj
                    .properties
                    .iter()
                    .map(|p| arena.string_literal(&p.name))
                    .collect();
                arena.union(keys)
            }
        }
        Node::Union(members) => {
            let keyofs: Vec<NodeId> = members
                .iter()
                .map(|&m| resolve_keyof(arena, m, state))
                .collect();
            arena.intersection(keyofs)
        }
        Node::Intersection(members) => {
            let keyofs: Vec<NodeId> = members
                .iter()
                .map(|&m| resolve_keyof(arena, m, state))
                .collect();
            arena.union(keyofs)
        }
        Node::Primitive(PrimitiveKind::Any) => {
            let s = arena.primitive(PrimitiveKind::String);
            let n = arena.primitive(PrimitiveKind::Number);
            let sym = arena.primitive(PrimitiveKind::Symbol);
            arena.union(vec![s, n, sym])
        }
        Node::Primitive(PrimitiveKind::Unknown | PrimitiveKind::Never) => {
            arena.primitive(PrimitiveKind::Never)
        }
        _ => {
            state.mark_symbolic();
            arena.key_of(operand)
        }
    }
}

// ---------------------------------------------------------------------------
// resolve_indexed_access
// ---------------------------------------------------------------------------

/// `T[K]` — look up member(s) by key on the resolved object.
#[allow(clippy::too_many_arguments, clippy::only_used_in_recursion)]
fn resolve_indexed_access(
    arena: &mut QueryArena,
    object: NodeId,
    index: NodeId,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> NodeId {
    // Clone index node to release borrow before recursion/allocation.
    let index_node = arena.get(index).clone();

    // If index is a union, distribute: T["a" | "b"] = T["a"] | T["b"]
    if let Node::Union(members) = index_node {
        let results: Vec<NodeId> = members
            .iter()
            .map(|&m| resolve_indexed_access(arena, object, m, host, state, subst))
            .collect();
        return arena.union(results);
    }

    let key = match arena.get(index) {
        Node::Literal(super::arena::SolverLiteral::String(s)) => Some(s.clone()),
        _ => None,
    };

    let Some(key) = key else {
        state.mark_symbolic();
        return arena.indexed_access(object, index);
    };

    // Clone object node to release borrow before recursion/allocation.
    let obj_node = arena.get(object).clone();

    match obj_node {
        Node::Object(obj) => {
            if let Some(prop) = obj.properties.iter().find(|p| p.name == key) {
                return prop.ty;
            }
            for idx_sig in &obj.index_signatures {
                if matches!(
                    arena.get(idx_sig.key_type),
                    Node::Primitive(PrimitiveKind::String)
                ) {
                    return idx_sig.value_type;
                }
            }
            arena.primitive(PrimitiveKind::Undefined)
        }
        Node::Intersection(members) => {
            let mut matches = Vec::new();
            for &member in &members {
                let result = resolve_indexed_access(arena, member, index, host, state, subst);
                if !matches!(arena.get(result), Node::Primitive(PrimitiveKind::Undefined)) {
                    matches.push(result);
                }
            }
            match matches.len() {
                0 => arena.primitive(PrimitiveKind::Undefined),
                1 => matches[0],
                _ => arena.intersection(matches),
            }
        }
        _ => {
            state.mark_symbolic();
            arena.indexed_access(object, index)
        }
    }
}

// ---------------------------------------------------------------------------
// resolve_conditional
// ---------------------------------------------------------------------------

/// `T extends U ? A : B` — resolve using the relation engine.
///
/// Handles:
/// - Distributive conditionals: if `check` is a union and `distributive` is true,
///   distribute per-member and re-union the results.
/// - `infer` bindings: collected during the relation check and injected into the
///   true-branch substitution.
#[allow(clippy::too_many_arguments)]
fn resolve_conditional(
    arena: &mut QueryArena,
    check: NodeId,
    extends: NodeId,
    true_branch: NodeId,
    false_branch: NodeId,
    distributive: bool,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> NodeId {
    use super::arena::SolverCaches;
    use super::relate::{relate, RelationLimits, RelationState};
    use super::result::RelationMode;

    // Distributive: if check is a resolved union and this is a distributive
    // conditional, distribute per-member and re-union.
    if distributive {
        if let Node::Union(members) = arena.get(check).clone() {
            let branches: Vec<NodeId> = members
                .iter()
                .map(|&m| {
                    resolve_conditional(
                        arena,
                        m,
                        extends,
                        true_branch,
                        false_branch,
                        false,
                        host,
                        state,
                        subst,
                    )
                })
                .collect();
            return arena.union(branches);
        }
    }

    // Set up relation check with infer binding collection.
    let mut caches = SolverCaches::new();
    let mut rel_state = RelationState::new(RelationLimits::default());
    rel_state.begin_infer(); // enable infer binding collection

    let relation = relate(
        arena,
        &mut caches,
        check,
        extends,
        RelationMode::Assignable,
        &mut rel_state,
    );

    match relation {
        super::result::RelationResult::Assignable => {
            // Collect infer bindings and inject into the true-branch substitution.
            let mut true_subst = subst.clone();
            if let Some(infer_bindings) = rel_state.take_infer_bindings() {
                for (name, candidates) in infer_bindings.iter() {
                    // Multiple candidates → intersect (use first for now).
                    if candidates.is_empty() {
                        continue;
                    }

                    let binding = if candidates.len() == 1 {
                        candidates[0]
                    } else {
                        arena.intersection(candidates.to_vec())
                    };
                    true_subst.bind(name, binding);
                }
            }
            resolve_node(arena, true_branch, host, state, &true_subst)
        }
        super::result::RelationResult::NotAssignable => {
            resolve_node(arena, false_branch, host, state, subst)
        }
        super::result::RelationResult::Unknown => {
            state.mark_symbolic();
            let tb = resolve_node(arena, true_branch, host, state, subst);
            let fb = resolve_node(arena, false_branch, host, state, subst);
            arena.conditional(check, extends, tb, fb, distributive)
        }
    }
}

// ---------------------------------------------------------------------------
// resolve_mapped — { [K in Source]: Value }
// ---------------------------------------------------------------------------

/// Mapped type resolution.
/// - Finite keyspace (string literal union) → concrete object with one property per key.
/// - Open keyspace (string/number) → object with index signature.
#[allow(clippy::too_many_arguments)]
fn resolve_mapped(
    arena: &mut QueryArena,
    parameter: &str,
    source: NodeId,
    value: NodeId,
    optional: super::arena::MappedModifierKind,
    readonly: super::arena::MappedModifierKind,
    name_type: Option<NodeId>,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> NodeId {
    use super::arena::{IndexSignatureNode, MappedModifierKind, ObjectNode, PropertyNode};

    let keys = collect_finite_keys(arena, source);

    if let Some(key_names) = keys {
        let mut properties = Vec::with_capacity(key_names.len());
        for key in key_names {
            let key_node = arena.string_literal(&key);
            let mut child_subst = subst.clone();
            child_subst.bind(parameter, key_node);

            // Key remapping: if name_type exists, resolve it to get the actual property name
            let prop_name = if let Some(nt) = name_type {
                let remapped = resolve_node(arena, nt, host, state, &child_subst);
                match arena.get(remapped) {
                    Node::Literal(super::arena::SolverLiteral::String(s)) => s.clone(),
                    Node::Primitive(PrimitiveKind::Never) => continue, // filtered out
                    _ => key.clone(), // can't resolve statically — keep original
                }
            } else {
                key
            };

            let resolved_value = resolve_node(arena, value, host, state, &child_subst);

            properties.push(PropertyNode {
                name: prop_name,
                ty: resolved_value,
                optional: matches!(optional, MappedModifierKind::Add),
                readonly: matches!(readonly, MappedModifierKind::Add),
                is_method: false,
            });
        }

        arena.object(ObjectNode {
            properties,
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        })
    } else {
        let resolved_value = resolve_node(arena, value, host, state, subst);
        arena.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![IndexSignatureNode {
                key_type: source,
                value_type: resolved_value,
                readonly: matches!(readonly, MappedModifierKind::Add),
            }],
            call_signatures: vec![],
            construct_signatures: vec![],
        })
    }
}

/// Iteratively collect string literal keys from a node. Returns `None` if the
/// keyspace is open (non-literal sources).
fn collect_finite_keys(arena: &QueryArena, node: NodeId) -> Option<Vec<String>> {
    let mut keys = Vec::new();
    let mut stack = vec![node];

    while let Some(id) = stack.pop() {
        match arena.get(id) {
            Node::Literal(super::arena::SolverLiteral::String(s)) => {
                keys.push(s.clone());
            }
            Node::Union(members) => {
                stack.extend(members.iter().copied());
            }
            Node::Primitive(PrimitiveKind::Never) => {}
            // Any non-literal member means the keyspace is open
            _ => return None,
        }
    }

    Some(keys)
}

// ---------------------------------------------------------------------------
// resolve_typeof
// ---------------------------------------------------------------------------

/// `typeof x` / `typeof x.y.z` — look up value declaration from host.
fn resolve_typeof(
    arena: &mut QueryArena,
    path: &[String],
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    _subst: &SubstitutionEnv,
) -> NodeId {
    if path.is_empty() {
        state.mark_symbolic();
        return arena.alloc(Node::TypeOf { path: vec![] });
    }

    // Try to resolve the root symbol as a value declaration
    let root_name = &path[0];
    let mut consumed_segments = 1usize;
    let qualified_root = if path.len() > 1 {
        let qualified = format!("{}.{}", path[0], path[1]);
        resolve_name_in_context(state, &qualified).or_else(|| host.root_identity("", &qualified))
    } else {
        None
    };
    // First check declaration context (for typeof inside imported type bodies),
    // then host's root_identity (for import bindings). Fall back to the
    // original ("", name) identity which checks owner env directly.
    let root_id = if let Some(identity) = qualified_root {
        consumed_segments = 2;
        identity
    } else {
        resolve_name_in_context(state, root_name)
            .or_else(|| host.root_identity("", root_name))
            .unwrap_or_else(|| ResolvedRootIdentity::new("", root_name))
    };

    if let Some(prepared) = host.resolve_prepared_value_decl(&root_id) {
        // Priority: type_annotation > object_shape > function_signature > enum_members
        let base_type = if let Some(ref ty_ann) = prepared.type_annotation {
            Some(lower_type_expr(arena, ty_ann))
        } else if let Some(ref shape) = prepared.object_shape {
            Some(lower_type_expr(
                arena,
                &TypeExpr::Object(Arc::new(shape.clone())),
            ))
        } else if let Some(ref sig) = prepared.function_signature {
            let func_expr = crate::analysis::type_expr::FunctionExpr {
                parameters: sig.parameters.clone(),
                return_type: sig.return_type.as_ref().map(|t| Arc::new(t.clone())),
                type_parameters: sig.type_parameters.clone(),
            };
            if prepared.kind == super::super::type_eval::ValueDeclKind::Class {
                // Class typeof: object with construct signature so
                // ConstructorParameters<typeof C> and InstanceType<typeof C>
                // find the construct signature, matching the manual
                // `{ new(...): T }` pattern.
                Some(lower_type_expr(
                    arena,
                    &TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                        properties: vec![
                            crate::analysis::type_expr::ObjectMember::ConstructSignature(func_expr),
                        ],
                    })),
                ))
            } else {
                // Regular function typeof: bare function type
                Some(lower_type_expr(
                    arena,
                    &TypeExpr::Function(Arc::new(func_expr)),
                ))
            }
        } else if let Some(ref members) = prepared.enum_members {
            // Enum value object: { MemberA: 0, MemberB: 1, ... }
            let obj_expr = crate::analysis::type_expr::ObjectExpr {
                properties: members
                    .iter()
                    .map(|(name, ty)| {
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: name.clone(),
                                ty: ty.clone(),
                                optional: false,
                                readonly: true,
                            },
                        )
                    })
                    .collect(),
            };
            Some(lower_type_expr(
                arena,
                &TypeExpr::Object(Arc::new(obj_expr)),
            ))
        } else {
            None
        };

        if let Some(base) = base_type {
            let pushed = if !prepared.name_resolution.is_empty() {
                state.value_decl_context_stack.push(Arc::clone(&prepared));
                true
            } else {
                false
            };

            let resolved_base = resolve_node(arena, base, host, state, &SubstitutionEnv::new());
            let result = if path.len() > consumed_segments {
                let mut current = resolved_base;
                let mut ok = true;
                for segment in &path[consumed_segments..] {
                    let node = arena.get(current).clone();
                    match node {
                        Node::Object(obj) => {
                            if let Some(prop) = obj.properties.iter().find(|p| p.name == *segment) {
                                current = prop.ty;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                ok.then_some(current)
            } else {
                Some(resolved_base)
            };

            if pushed {
                state.value_decl_context_stack.pop();
            }

            if let Some(result) = result {
                return result;
            }

            state.mark_symbolic();
            return arena.alloc(Node::TypeOf {
                path: path.to_vec(),
            });
        }
    }

    // Can't resolve — stay symbolic
    state.mark_symbolic();
    arena.alloc(Node::TypeOf {
        path: path.to_vec(),
    })
}

// ---------------------------------------------------------------------------
// resolve_template_literal
// ---------------------------------------------------------------------------

/// `` `prefix${T}suffix` `` — expand if all expressions are concrete string literals.
/// Uses iterative cartesian product expansion.
fn resolve_template_literal(
    arena: &mut QueryArena,
    quasis: &[String],
    expressions: &[NodeId],
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> NodeId {
    // Resolve all expression positions
    let resolved_exprs: Vec<NodeId> = expressions
        .iter()
        .map(|&e| resolve_node(arena, e, host, state, subst))
        .collect();

    // Collect string values from each expression position (each may be a union)
    let mut expr_options: Vec<Vec<String>> = Vec::with_capacity(resolved_exprs.len());
    for &expr_id in &resolved_exprs {
        let mut strings = Vec::new();
        let mut stack = vec![expr_id];
        let mut all_concrete = true;

        while let Some(id) = stack.pop() {
            match arena.get(id) {
                Node::Literal(super::arena::SolverLiteral::String(s)) => {
                    strings.push(s.clone());
                }
                Node::Literal(super::arena::SolverLiteral::Number(n)) => {
                    strings.push(format_number(*n));
                }
                Node::Literal(super::arena::SolverLiteral::Boolean(b)) => {
                    strings.push(b.to_string());
                }
                Node::Literal(super::arena::SolverLiteral::BigInt(s)) => {
                    strings.push(s.clone());
                }
                Node::Primitive(PrimitiveKind::Null) => {
                    strings.push("null".into());
                }
                Node::Primitive(PrimitiveKind::Undefined) => {
                    strings.push("undefined".into());
                }
                Node::Primitive(PrimitiveKind::Never) => {
                    // never contributes zero strings — result will be empty
                }
                Node::Union(members) => {
                    stack.extend(members.iter().copied());
                }
                _ => {
                    all_concrete = false;
                    break;
                }
            }
        }

        if !all_concrete {
            state.mark_symbolic();
            return arena.alloc(Node::TemplateLiteral {
                quasis: quasis.to_vec(),
                expressions: resolved_exprs,
            });
        }
        expr_options.push(strings);
    }

    // If any expression position resolved to zero options (never), result is never.
    if expr_options.iter().any(|v| v.is_empty()) {
        return arena.primitive(PrimitiveKind::Never);
    }

    // Guard: deterministic operational limit on cartesian product size.
    let product_size: usize = expr_options.iter().map(|v| v.len()).product();
    if product_size > 10_000 {
        state.execution_status = ExecutionStatus::HardStop;
        state.mark_incomplete(IncompleteReason::RecursionPolicy {
            description: format!(
                "template literal expansion would produce {} combinations",
                product_size
            ),
        });
        return arena.alloc(Node::TemplateLiteral {
            quasis: quasis.to_vec(),
            expressions: resolved_exprs,
        });
    }

    // Iterative cartesian product expansion
    let mut results: Vec<String> = vec![quasis[0].clone()];
    for (i, options) in expr_options.iter().enumerate() {
        let suffix = quasis.get(i + 1).cloned().unwrap_or_default();
        let mut new_results = Vec::with_capacity(results.len() * options.len());
        for base in &results {
            for opt in options {
                let mut s = base.clone();
                s.push_str(opt);
                s.push_str(&suffix);
                new_results.push(s);
            }
        }
        results = new_results;
    }

    let nodes: Vec<NodeId> = results
        .into_iter()
        .map(|s| arena.string_literal(s))
        .collect();
    arena.union(nodes)
}

fn format_number(n: f64) -> String {
    if n == n.trunc() && n.is_finite() {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

// ---------------------------------------------------------------------------
// project_to_type_expr — arena nodes back to TypeExpr
// ---------------------------------------------------------------------------

/// Project a resolved arena node back to `TypeExpr`.
///
/// This is the inverse of `lower_type_expr`. It converts the solver's internal
/// representation back to the public output type.
fn project_to_type_expr(arena: &QueryArena, node: NodeId) -> TypeExpr {
    project_inner(arena, node, &mut Vec::new(), 0)
}

fn project_inner(
    arena: &QueryArena,
    node: NodeId,
    visited: &mut Vec<NodeId>,
    depth: usize,
) -> TypeExpr {
    if node.is_unresolved() || depth > 50 || visited.contains(&node) {
        return TypeExpr::Unknown {
            raw: "unresolved".into(),
        };
    }
    visited.push(node);

    let result = match arena.get(node) {
        Node::Primitive(kind) => TypeExpr::Primitive(project_primitive(*kind)),

        Node::Literal(lit) => match lit {
            super::arena::SolverLiteral::String(s) => TypeExpr::string_literal(s),
            super::arena::SolverLiteral::Number(n) => TypeExpr::number_literal(*n),
            super::arena::SolverLiteral::Boolean(b) => TypeExpr::boolean_literal(*b),
            super::arena::SolverLiteral::BigInt(s) => {
                TypeExpr::Literal(crate::analysis::type_expr::LiteralValue::BigInt(s.clone()))
            }
        },

        Node::Union(members) => {
            let types: Vec<TypeExpr> = members
                .iter()
                .map(|&m| project_inner(arena, m, visited, depth + 1))
                .collect();
            TypeExpr::Union(Arc::from(types))
        }

        Node::Intersection(members) => {
            let types: Vec<TypeExpr> = members
                .iter()
                .map(|&m| project_inner(arena, m, visited, depth + 1))
                .collect();
            TypeExpr::Intersection(Arc::from(types))
        }

        Node::Array { element, readonly } => TypeExpr::Array {
            element: Arc::new(project_inner(arena, *element, visited, depth + 1)),
            readonly: *readonly,
        },

        Node::Object(obj) => {
            let mut members = Vec::with_capacity(
                obj.properties.len()
                    + obj.index_signatures.len()
                    + obj.call_signatures.len()
                    + obj.construct_signatures.len(),
            );
            for p in &obj.properties {
                let ty = project_inner(arena, p.ty, visited, depth + 1);
                if p.is_method {
                    match ty {
                        TypeExpr::Function(function) => {
                            members.push(crate::analysis::type_expr::ObjectMember::Method(
                                crate::analysis::type_expr::MethodSignature {
                                    name: p.name.clone(),
                                    function: (*function).clone(),
                                    optional: p.optional,
                                },
                            ));
                        }
                        other => members.push(crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: p.name.clone(),
                                ty: other,
                                optional: p.optional,
                                readonly: p.readonly,
                            },
                        )),
                    }
                } else {
                    members.push(crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: p.name.clone(),
                            ty,
                            optional: p.optional,
                            readonly: p.readonly,
                        },
                    ));
                }
            }
            for idx in &obj.index_signatures {
                members.push(crate::analysis::type_expr::ObjectMember::IndexSignature(
                    crate::analysis::type_expr::IndexSignature {
                        key_name: "key".into(),
                        key_type: project_inner(arena, idx.key_type, visited, depth + 1),
                        value_type: project_inner(arena, idx.value_type, visited, depth + 1),
                        readonly: idx.readonly,
                    },
                ));
            }
            for sig in &obj.call_signatures {
                members.push(crate::analysis::type_expr::ObjectMember::CallSignature(
                    project_signature(arena, sig, visited, depth + 1),
                ));
            }
            for sig in &obj.construct_signatures {
                members.push(
                    crate::analysis::type_expr::ObjectMember::ConstructSignature(
                        project_signature(arena, sig, visited, depth + 1),
                    ),
                );
            }
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: members,
            }))
        }

        Node::Ref {
            name,
            type_arguments,
        } => TypeExpr::Ref {
            name: Arc::from(name.as_str()),
            type_arguments: Arc::from(
                type_arguments
                    .iter()
                    .map(|&a| project_inner(arena, a, visited, depth + 1))
                    .collect::<Vec<_>>(),
            ),
        },

        Node::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: Arc::from(
                elements
                    .iter()
                    .map(|el| crate::analysis::type_expr::TupleElement {
                        label: el.label.clone(),
                        ty: project_inner(arena, el.ty, visited, depth + 1),
                        optional: el.optional,
                        rest: el.rest,
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },

        Node::Function(func) => {
            if let Some(sig) = func.signatures.first() {
                TypeExpr::Function(Arc::new(project_signature(arena, sig, visited, depth + 1)))
            } else {
                TypeExpr::Function(Arc::new(crate::analysis::type_expr::FunctionExpr {
                    parameters: vec![],
                    return_type: None,
                    type_parameters: vec![],
                }))
            }
        }

        Node::KeyOf(operand) => {
            TypeExpr::KeyOf(Arc::new(project_inner(arena, *operand, visited, depth + 1)))
        }

        Node::TypeOf { path } => {
            TypeExpr::TypeOf(crate::analysis::type_expr::ValueRef { path: path.clone() })
        }

        Node::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
            object: Arc::new(project_inner(arena, *object, visited, depth + 1)),
            index: Arc::new(project_inner(arena, *index, visited, depth + 1)),
        },

        Node::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            ..
        } => TypeExpr::Conditional {
            check: Arc::new(project_inner(arena, *check, visited, depth + 1)),
            extends: Arc::new(project_inner(arena, *extends, visited, depth + 1)),
            true_type: Arc::new(project_inner(arena, *true_branch, visited, depth + 1)),
            false_type: Arc::new(project_inner(arena, *false_branch, visited, depth + 1)),
        },

        Node::TypeParam {
            name,
            constraint,
            default,
        } => TypeExpr::TypeParameter(crate::analysis::type_expr::TypeParam {
            name: name.clone(),
            constraint: constraint
                .map(|node| Arc::new(project_inner(arena, node, visited, depth + 1))),
            default: default.map(|node| Arc::new(project_inner(arena, node, visited, depth + 1))),
        }),

        Node::Infer { name } => TypeExpr::Infer { name: name.clone() },

        Node::Rest(inner) => {
            TypeExpr::Rest(Arc::new(project_inner(arena, *inner, visited, depth + 1)))
        }

        Node::Error { description } => TypeExpr::Unknown {
            raw: description.clone(),
        },

        // Mapped, TemplateLiteral, Applied, RecursiveRef — use display as fallback
        _ => TypeExpr::Unknown {
            raw: display_node(arena, node),
        },
    };

    visited.pop();
    result
}

fn project_primitive(kind: PrimitiveKind) -> crate::analysis::type_expr::PrimitiveName {
    use crate::analysis::type_expr::PrimitiveName;
    match kind {
        PrimitiveKind::String => PrimitiveName::String,
        PrimitiveKind::Number => PrimitiveName::Number,
        PrimitiveKind::Boolean => PrimitiveName::Boolean,
        PrimitiveKind::Symbol => PrimitiveName::Symbol,
        PrimitiveKind::BigInt => PrimitiveName::BigInt,
        PrimitiveKind::Any => PrimitiveName::Any,
        PrimitiveKind::Unknown => PrimitiveName::Unknown,
        PrimitiveKind::Void => PrimitiveName::Void,
        PrimitiveKind::Never => PrimitiveName::Never,
        PrimitiveKind::Null => PrimitiveName::Null,
        PrimitiveKind::Undefined => PrimitiveName::Undefined,
        PrimitiveKind::Object => PrimitiveName::Object,
    }
}

fn project_signature(
    arena: &QueryArena,
    sig: &super::arena::CallSignatureNode,
    visited: &mut Vec<NodeId>,
    depth: usize,
) -> crate::analysis::type_expr::FunctionExpr {
    crate::analysis::type_expr::FunctionExpr {
        parameters: sig
            .parameters
            .iter()
            .map(|p| crate::analysis::type_expr::FunctionParam {
                name: p.name.clone(),
                ty: project_inner(arena, p.ty, visited, depth + 1),
                optional: p.optional,
                rest: p.rest,
            })
            .collect(),
        return_type: Some(Arc::new(project_inner(
            arena,
            sig.return_type,
            visited,
            depth + 1,
        ))),
        type_parameters: sig
            .type_parameters
            .iter()
            .map(|param| crate::analysis::type_expr::TypeParam {
                name: param.name.clone(),
                constraint: param
                    .constraint
                    .map(|node| Arc::new(project_inner(arena, node, visited, depth + 1))),
                default: param
                    .default
                    .map(|node| Arc::new(project_inner(arena, node, visited, depth + 1))),
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::type_expr::PrimitiveName;
    use crate::analysis::type_solver::host::NoopSolverHost;

    #[test]
    fn solve_primitive_is_identity() {
        let expr = TypeExpr::Primitive(PrimitiveName::String);
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
        assert_eq!(result.execution_status, ExecutionStatus::Completed);
    }

    #[test]
    fn solve_literal_is_identity() {
        let expr = TypeExpr::string_literal("hello");
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::string_literal("hello"));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_union_resolves_members() {
        let expr = TypeExpr::Union(Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
        ]));
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
            }
            _ => panic!("expected Union"),
        }
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_object_resolves_property_types() {
        let expr = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "x".into(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                },
            )],
        }));
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 1);
            }
            _ => panic!("expected Object"),
        }
    }

    #[test]
    fn solve_unresolved_ref_stays_symbolic() {
        let expr = TypeExpr::Ref {
            name: Arc::from("UnknownType"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        // Should stay as a Ref since NoopSolverHost can't resolve it
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
        match &result.value {
            TypeExpr::Ref { name, .. } => assert_eq!(name.as_ref(), "UnknownType"),
            _ => panic!("expected Ref, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_non_nullable_builtin() {
        let expr = TypeExpr::Ref {
            name: Arc::from("NonNullable"),
            type_arguments: Arc::from(vec![TypeExpr::Union(Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Null),
                TypeExpr::Primitive(PrimitiveName::Undefined),
            ]))]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        // NonNullable should filter out null and undefined
        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_uppercase_builtin() {
        let expr = TypeExpr::Ref {
            name: Arc::from("Uppercase"),
            type_arguments: Arc::from(vec![TypeExpr::string_literal("hello")]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::string_literal("HELLO"));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_capitalize_builtin() {
        let expr = TypeExpr::Ref {
            name: Arc::from("Capitalize"),
            type_arguments: Arc::from(vec![TypeExpr::string_literal("hello")]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::string_literal("Hello"));
    }

    #[test]
    fn solve_array_resolves_element() {
        let expr = TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: true,
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Array { readonly, .. } => assert!(readonly),
            _ => panic!("expected Array"),
        }
    }

    // -- Test host with prepared declarations --

    use crate::analysis::type_eval::TypeDeclKind;
    use crate::analysis::type_solver::prepared::PreparedTypeDecl;
    use rustc_hash::FxHashMap;

    struct TestHost {
        decls: FxHashMap<String, Arc<PreparedTypeDecl>>,
    }

    impl TestHost {
        fn new() -> Self {
            Self {
                decls: FxHashMap::default(),
            }
        }

        fn add_alias(&mut self, name: &str, body: TypeExpr) {
            self.decls.insert(
                name.to_string(),
                Arc::new(PreparedTypeDecl::new(
                    ResolvedRootIdentity::new("/test.ts", name),
                    TypeDeclKind::Alias,
                    body,
                )),
            );
        }

        fn add_generic_alias(
            &mut self,
            name: &str,
            params: Vec<crate::analysis::type_expr::TypeParam>,
            body: TypeExpr,
        ) {
            let mut decl = PreparedTypeDecl::new(
                ResolvedRootIdentity::new("/test.ts", name),
                TypeDeclKind::Alias,
                body,
            );
            decl.type_parameters = params;
            self.decls.insert(name.to_string(), Arc::new(decl));
        }
    }

    impl TypeSolverHost for TestHost {
        fn resolve_prepared_type_decl(
            &self,
            root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedTypeDecl>> {
            self.decls.get(&root_identity.symbol_name).cloned()
        }

        fn resolve_prepared_value_decl(
            &self,
            _: &ResolvedRootIdentity,
        ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>> {
            None
        }

        fn utility_source(&self, name: &str) -> UtilitySource {
            if BuiltinUtility::from_name(name).is_some() {
                UtilitySource::Builtin
            } else {
                UtilitySource::Unknown
            }
        }

        fn root_identity(
            &self,
            _canonical_id: &str,
            symbol_name: &str,
        ) -> Option<ResolvedRootIdentity> {
            if self.decls.contains_key(symbol_name) {
                Some(ResolvedRootIdentity::new("/test.ts", symbol_name))
            } else {
                None
            }
        }
    }

    // -- Host-backed resolution tests --

    #[test]
    fn solve_resolves_simple_type_alias() {
        let mut host = TestHost::new();
        // type MyString = string
        host.add_alias("MyString", TypeExpr::Primitive(PrimitiveName::String));

        let expr = TypeExpr::Ref {
            name: Arc::from("MyString"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &host, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_resolves_generic_alias_with_substitution() {
        let mut host = TestHost::new();
        // type Wrap<T> = T[]
        host.add_generic_alias(
            "Wrap",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            TypeExpr::Array {
                element: Arc::new(TypeExpr::Ref {
                    name: Arc::from("T"),
                    type_arguments: Arc::from(vec![]),
                }),
                readonly: false,
            },
        );

        // Wrap<number> should resolve to number[]
        let expr = TypeExpr::Ref {
            name: Arc::from("Wrap"),
            type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::Number)]),
        };
        let result = solve_type(&expr, &host, SolveLimits::default());

        match &result.value {
            TypeExpr::Array { element, readonly } => {
                assert_eq!(
                    element.as_ref(),
                    &TypeExpr::Primitive(PrimitiveName::Number)
                );
                assert!(!readonly);
            }
            other => panic!("expected Array, got: {:?}", other),
        }
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_resolves_chained_aliases() {
        let mut host = TestHost::new();
        // type A = string
        // type B = A
        host.add_alias("A", TypeExpr::Primitive(PrimitiveName::String));
        host.add_alias(
            "B",
            TypeExpr::Ref {
                name: Arc::from("A"),
                type_arguments: Arc::from(vec![]),
            },
        );

        let expr = TypeExpr::Ref {
            name: Arc::from("B"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &host, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
    }

    #[test]
    fn solve_missing_prepared_decl_returns_incomplete() {
        let _host = TestHost::new(); // empty — no decls

        // But we need root_identity to return Some for the test to reach the
        // prepared_type_decl lookup. Use a host that returns identity but no decl.
        struct MissingDeclHost;
        impl TypeSolverHost for MissingDeclHost {
            fn resolve_prepared_type_decl(
                &self,
                _: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedTypeDecl>> {
                None // source missing
            }
            fn resolve_prepared_value_decl(
                &self,
                _: &ResolvedRootIdentity,
            ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>
            {
                None
            }
            fn utility_source(&self, _: &str) -> UtilitySource {
                UtilitySource::Unknown
            }
            fn root_identity(&self, _: &str, symbol_name: &str) -> Option<ResolvedRootIdentity> {
                Some(ResolvedRootIdentity::new("/missing.ts", symbol_name))
            }
        }

        let expr = TypeExpr::Ref {
            name: Arc::from("MissingType"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &MissingDeclHost, SolveLimits::default());

        assert_eq!(result.exactness, SolverExactness::Incomplete);
        assert!(!result.incomplete_reasons.is_empty());
    }

    #[test]
    fn solve_generic_with_default_type_param() {
        let mut host = TestHost::new();
        // type WithDefault<T = string> = T[]
        host.add_generic_alias(
            "WithDefault",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
            }],
            TypeExpr::Array {
                element: Arc::new(TypeExpr::Ref {
                    name: Arc::from("T"),
                    type_arguments: Arc::from(vec![]),
                }),
                readonly: false,
            },
        );

        // WithDefault<> (no args) should use default T=string → string[]
        let expr = TypeExpr::Ref {
            name: Arc::from("WithDefault"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &host, SolveLimits::default());

        match &result.value {
            TypeExpr::Array { element, .. } => {
                assert_eq!(
                    element.as_ref(),
                    &TypeExpr::Primitive(PrimitiveName::String)
                );
            }
            other => panic!("expected Array, got: {:?}", other),
        }
    }

    #[test]
    fn solve_respects_step_limit() {
        // Create a deeply nested union to trigger step limit
        let mut expr = TypeExpr::Primitive(PrimitiveName::String);
        for _ in 0..10 {
            expr = TypeExpr::Union(Arc::from(vec![expr.clone(), expr.clone()]));
        }

        let limits = SolveLimits {
            max_resolve_steps: 50, // Very low limit
            ..Default::default()
        };
        let result = solve_type(&expr, &NoopSolverHost, limits);

        // Should hit the step limit
        assert_eq!(result.execution_status, ExecutionStatus::HardStop);
    }

    #[test]
    fn solve_typeof_resolves_imported_names_inside_prepared_value_annotations() {
        struct ValueContextHost {
            types: FxHashMap<String, Arc<PreparedTypeDecl>>,
            values:
                FxHashMap<String, Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>,
        }

        impl TypeSolverHost for ValueContextHost {
            fn resolve_prepared_type_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedTypeDecl>> {
                self.types.get(&root_identity.symbol_name).cloned()
            }

            fn resolve_prepared_value_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>
            {
                self.values.get(&root_identity.symbol_name).cloned()
            }

            fn utility_source(&self, name: &str) -> UtilitySource {
                if BuiltinUtility::from_name(name).is_some() {
                    UtilitySource::Builtin
                } else {
                    UtilitySource::Unknown
                }
            }

            fn root_identity(
                &self,
                _canonical_id: &str,
                symbol_name: &str,
            ) -> Option<ResolvedRootIdentity> {
                if self.types.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/dep.ts", symbol_name))
                } else if self.values.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/owner.ts", symbol_name))
                } else {
                    None
                }
            }
        }

        let mut remote = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/dep.ts", "Remote"),
            TypeDeclKind::Alias,
            TypeExpr::Primitive(PrimitiveName::String),
        );
        remote.build_member_index();

        let mut theme = crate::analysis::type_solver::prepared::PreparedValueDecl::new(
            ResolvedRootIdentity::new("/owner.ts", "theme"),
            crate::analysis::type_eval::ValueDeclKind::Const,
        );
        theme.type_annotation = Some(TypeExpr::Ref {
            name: Arc::from("Remote"),
            type_arguments: Arc::from(vec![]),
        });
        theme.name_resolution.insert(
            "Remote".into(),
            ResolvedRootIdentity::new("/dep.ts", "Remote"),
        );

        let host = ValueContextHost {
            types: {
                let mut map = FxHashMap::default();
                map.insert("Remote".into(), Arc::new(remote));
                map
            },
            values: {
                let mut map = FxHashMap::default();
                map.insert("theme".into(), Arc::new(theme));
                map
            },
        };

        let expr = TypeExpr::TypeOf(crate::analysis::type_expr::ValueRef {
            path: vec!["theme".into()],
        });
        let result = solve_type(&expr, &host, SolveLimits::default());

        assert_eq!(
            result.value,
            TypeExpr::Primitive(PrimitiveName::String),
            "typeof should resolve imported names inside prepared value annotations through value declaration context",
        );
    }

    #[test]
    fn solve_typeof_resolves_namespace_member_paths() {
        struct NamespaceValueHost {
            values:
                FxHashMap<String, Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>,
        }

        impl TypeSolverHost for NamespaceValueHost {
            fn resolve_prepared_type_decl(
                &self,
                _root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedTypeDecl>> {
                None
            }

            fn resolve_prepared_value_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>
            {
                self.values.get(&root_identity.symbol_name).cloned()
            }

            fn utility_source(&self, _name: &str) -> UtilitySource {
                UtilitySource::Unknown
            }

            fn root_identity(
                &self,
                _canonical_id: &str,
                symbol_name: &str,
            ) -> Option<ResolvedRootIdentity> {
                match symbol_name {
                    "ThemeNs.theme" | "theme" => {
                        Some(ResolvedRootIdentity::new("/theme.ts", "theme"))
                    }
                    _ => None,
                }
            }
        }

        let mut theme = crate::analysis::type_solver::prepared::PreparedValueDecl::new(
            ResolvedRootIdentity::new("/theme.ts", "theme"),
            crate::analysis::type_eval::ValueDeclKind::Const,
        );
        theme.object_shape = Some(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "slots".into(),
                    ty: TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                        properties: vec![
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "root".into(),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: false,
                                    readonly: false,
                                },
                            ),
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "label".into(),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: false,
                                    readonly: false,
                                },
                            ),
                        ],
                    })),
                    optional: false,
                    readonly: false,
                },
            )],
        });

        let host = NamespaceValueHost {
            values: {
                let mut map = FxHashMap::default();
                map.insert("theme".into(), Arc::new(theme));
                map
            },
        };

        let expr = TypeExpr::TypeOf(crate::analysis::type_expr::ValueRef {
            path: vec!["ThemeNs".into(), "theme".into(), "slots".into()],
        });
        let result = solve_type(&expr, &host, SolveLimits::default());

        let TypeExpr::Object(obj) = result.value else {
            panic!("namespace typeof member path should resolve to object shape");
        };
        let names: std::collections::BTreeSet<_> = obj
            .properties
            .iter()
            .map(|member| match member {
                crate::analysis::type_expr::ObjectMember::Property(prop) => prop.name.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(
            names,
            std::collections::BTreeSet::from(["label".to_string(), "root".to_string()]),
            "typeof should be able to consume the namespace qualifier as part of the root value lookup",
        );
    }

    #[test]
    fn solve_generic_typeof_arguments_flow_through_cached_prepared_decls() {
        struct CachedDeclHost {
            types: FxHashMap<String, Arc<PreparedTypeDecl>>,
            values:
                FxHashMap<String, Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>,
        }

        impl TypeSolverHost for CachedDeclHost {
            fn resolve_prepared_type_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedTypeDecl>> {
                self.types.get(&root_identity.symbol_name).cloned()
            }

            fn resolve_prepared_value_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>
            {
                self.values.get(&root_identity.symbol_name).cloned()
            }

            fn utility_source(&self, name: &str) -> UtilitySource {
                if BuiltinUtility::from_name(name).is_some() {
                    UtilitySource::Builtin
                } else {
                    UtilitySource::Unknown
                }
            }

            fn root_identity(
                &self,
                _canonical_id: &str,
                symbol_name: &str,
            ) -> Option<ResolvedRootIdentity> {
                if self.types.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/types.ts", symbol_name))
                } else if self.values.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/theme.ts", symbol_name))
                } else {
                    None
                }
            }
        }

        let empty_object = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![],
        }));
        let slots_object = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "base".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "label".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
            ],
        }));

        let mut id_decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Id"),
            TypeDeclKind::Alias,
            TypeExpr::Intersection(Arc::from(vec![
                empty_object.clone(),
                TypeExpr::Mapped {
                    parameter: "P".into(),
                    source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::Ref {
                        name: Arc::from("T"),
                        type_arguments: Arc::from(vec![]),
                    }))),
                    value: Arc::new(TypeExpr::IndexedAccess {
                        object: Arc::new(TypeExpr::Ref {
                            name: Arc::from("T"),
                            type_arguments: Arc::from(vec![]),
                        }),
                        index: Arc::new(TypeExpr::Ref {
                            name: Arc::from("P"),
                            type_arguments: Arc::from(vec![]),
                        }),
                    }),
                    optional: crate::analysis::type_expr::MappedModifier::None,
                    readonly: crate::analysis::type_expr::MappedModifier::None,
                    name_type: None,
                },
            ])),
        );
        id_decl
            .type_parameters
            .push(crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            });
        id_decl
            .name_resolution
            .insert("T".into(), ResolvedRootIdentity::new("/types.ts", "T"));

        let mut component_ui = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "ComponentUI"),
            TypeDeclKind::Alias,
            TypeExpr::Ref {
                name: Arc::from("Id"),
                type_arguments: Arc::from(vec![TypeExpr::IndexedAccess {
                    object: Arc::new(TypeExpr::Ref {
                        name: Arc::from("T"),
                        type_arguments: Arc::from(vec![]),
                    }),
                    index: Arc::new(TypeExpr::string_literal("slots")),
                }]),
            },
        );
        component_ui
            .type_parameters
            .push(crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            });
        component_ui
            .name_resolution
            .insert("Id".into(), ResolvedRootIdentity::new("/types.ts", "Id"));

        let mut button = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/button-types.ts", "Button"),
            TypeDeclKind::Alias,
            TypeExpr::Ref {
                name: Arc::from("ComponentUI"),
                type_arguments: Arc::from(vec![TypeExpr::TypeOf(
                    crate::analysis::type_expr::ValueRef {
                        path: vec!["theme".into()],
                    },
                )]),
            },
        );
        button.name_resolution.insert(
            "ComponentUI".into(),
            ResolvedRootIdentity::new("/types.ts", "ComponentUI"),
        );
        button.name_resolution.insert(
            "theme".into(),
            ResolvedRootIdentity::new("/theme.ts", "theme"),
        );

        let mut theme = crate::analysis::type_solver::prepared::PreparedValueDecl::new(
            ResolvedRootIdentity::new("/theme.ts", "theme"),
            crate::analysis::type_eval::ValueDeclKind::Const,
        );
        theme.object_shape = Some(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "slots".into(),
                    ty: slots_object.clone(),
                    optional: false,
                    readonly: false,
                },
            )],
        });

        let host = CachedDeclHost {
            types: {
                let mut map = FxHashMap::default();
                map.insert("Id".into(), Arc::new(id_decl));
                map.insert("ComponentUI".into(), Arc::new(component_ui));
                map.insert("Button".into(), Arc::new(button));
                map
            },
            values: {
                let mut map = FxHashMap::default();
                map.insert("theme".into(), Arc::new(theme));
                map
            },
        };

        let result = solve_type(&TypeExpr::named("Button"), &host, SolveLimits::default());

        let TypeExpr::Object(obj) = result.value else {
            panic!(
                "Button should resolve to an object shape, got {:?}",
                result.value
            );
        };
        assert!(
            obj.properties.iter().any(|member| {
                matches!(
                    member,
                    crate::analysis::type_expr::ObjectMember::Property(property)
                        if property.name == "base"
                )
            }),
            "generic typeof argument should expose base, got {:?}",
            obj
        );
        assert!(
            obj.properties.iter().any(|member| {
                matches!(
                    member,
                    crate::analysis::type_expr::ObjectMember::Property(property)
                        if property.name == "label"
                )
            }),
            "generic typeof argument should expose label, got {:?}",
            obj
        );
    }

    #[test]
    fn solve_generic_required_mapped_typeof_arguments_flow_through_cached_prepared_decls() {
        struct CachedDeclHost {
            types: FxHashMap<String, Arc<PreparedTypeDecl>>,
            values:
                FxHashMap<String, Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>,
        }

        impl TypeSolverHost for CachedDeclHost {
            fn resolve_prepared_type_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedTypeDecl>> {
                self.types.get(&root_identity.symbol_name).cloned()
            }

            fn resolve_prepared_value_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>
            {
                self.values.get(&root_identity.symbol_name).cloned()
            }

            fn utility_source(&self, name: &str) -> UtilitySource {
                if BuiltinUtility::from_name(name).is_some() {
                    UtilitySource::Builtin
                } else {
                    UtilitySource::Unknown
                }
            }

            fn root_identity(
                &self,
                _canonical_id: &str,
                symbol_name: &str,
            ) -> Option<ResolvedRootIdentity> {
                if self.types.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/types.ts", symbol_name))
                } else if self.values.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/theme.ts", symbol_name))
                } else {
                    None
                }
            }
        }

        let empty_object = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![],
        }));
        let slots_object = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "base".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "label".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
            ],
        }));

        let mut id_decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Id"),
            TypeDeclKind::Alias,
            TypeExpr::Intersection(Arc::from(vec![
                empty_object.clone(),
                TypeExpr::Mapped {
                    parameter: "P".into(),
                    source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::Ref {
                        name: Arc::from("T"),
                        type_arguments: Arc::from(vec![]),
                    }))),
                    value: Arc::new(TypeExpr::IndexedAccess {
                        object: Arc::new(TypeExpr::Ref {
                            name: Arc::from("T"),
                            type_arguments: Arc::from(vec![]),
                        }),
                        index: Arc::new(TypeExpr::Ref {
                            name: Arc::from("P"),
                            type_arguments: Arc::from(vec![]),
                        }),
                    }),
                    optional: crate::analysis::type_expr::MappedModifier::None,
                    readonly: crate::analysis::type_expr::MappedModifier::None,
                    name_type: None,
                },
            ])),
        );
        id_decl
            .type_parameters
            .push(crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            });

        let mut component_ui = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "ComponentUI"),
            TypeDeclKind::Alias,
            TypeExpr::Ref {
                name: Arc::from("Id"),
                type_arguments: Arc::from(vec![TypeExpr::Mapped {
                    parameter: "K".into(),
                    source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::Ref {
                        name: Arc::from("Required"),
                        type_arguments: Arc::from(vec![TypeExpr::IndexedAccess {
                            object: Arc::new(TypeExpr::Ref {
                                name: Arc::from("T"),
                                type_arguments: Arc::from(vec![]),
                            }),
                            index: Arc::new(TypeExpr::string_literal("slots")),
                        }]),
                    }))),
                    value: Arc::new(TypeExpr::Function(Arc::new(
                        crate::analysis::type_expr::FunctionExpr {
                            parameters: vec![crate::analysis::type_expr::FunctionParam {
                                name: Some("props".into()),
                                ty: TypeExpr::Object(Arc::new(
                                    crate::analysis::type_expr::ObjectExpr {
                                        properties: vec![crate::analysis::type_expr::ObjectMember::IndexSignature(
                                            crate::analysis::type_expr::IndexSignature {
                                                key_name: "key".into(),
                                                key_type: TypeExpr::Primitive(PrimitiveName::String),
                                                value_type: TypeExpr::Primitive(PrimitiveName::Any),
                                                readonly: false,
                                            },
                                        )],
                                    },
                                )),
                                optional: true,
                                rest: false,
                            }],
                            return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                            type_parameters: vec![],
                        },
                    ))),
                    optional: crate::analysis::type_expr::MappedModifier::None,
                    readonly: crate::analysis::type_expr::MappedModifier::None,
                    name_type: None,
                }]),
            },
        );
        component_ui
            .type_parameters
            .push(crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            });
        component_ui
            .name_resolution
            .insert("Id".into(), ResolvedRootIdentity::new("/types.ts", "Id"));

        let mut button = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/button-types.ts", "Button"),
            TypeDeclKind::Alias,
            TypeExpr::Ref {
                name: Arc::from("ComponentUI"),
                type_arguments: Arc::from(vec![TypeExpr::TypeOf(
                    crate::analysis::type_expr::ValueRef {
                        path: vec!["theme".into()],
                    },
                )]),
            },
        );
        button.name_resolution.insert(
            "ComponentUI".into(),
            ResolvedRootIdentity::new("/types.ts", "ComponentUI"),
        );
        button.name_resolution.insert(
            "theme".into(),
            ResolvedRootIdentity::new("/theme.ts", "theme"),
        );

        let mut theme = crate::analysis::type_solver::prepared::PreparedValueDecl::new(
            ResolvedRootIdentity::new("/theme.ts", "theme"),
            crate::analysis::type_eval::ValueDeclKind::Const,
        );
        theme.object_shape = Some(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "slots".into(),
                    ty: slots_object,
                    optional: false,
                    readonly: false,
                },
            )],
        });

        let host = CachedDeclHost {
            types: {
                let mut map = FxHashMap::default();
                map.insert("Id".into(), Arc::new(id_decl));
                map.insert("ComponentUI".into(), Arc::new(component_ui));
                map.insert("Button".into(), Arc::new(button));
                map
            },
            values: {
                let mut map = FxHashMap::default();
                map.insert("theme".into(), Arc::new(theme));
                map
            },
        };

        let result = solve_type(&TypeExpr::named("Button"), &host, SolveLimits::default());

        let TypeExpr::Object(obj) = result.value else {
            panic!("Button should resolve to an object shape after Id<T> normalization");
        };
        assert!(
            obj.properties.iter().any(|member| {
                matches!(
                    member,
                    crate::analysis::type_expr::ObjectMember::Property(property)
                        if property.name == "base"
                )
            }),
            "generic required mapped typeof argument should expose base, got {:?}",
            obj
        );
        assert!(
            obj.properties.iter().any(|member| {
                matches!(
                    member,
                    crate::analysis::type_expr::ObjectMember::Property(property)
                        if property.name == "label"
                )
            }),
            "generic required mapped typeof argument should expose label, got {:?}",
            obj
        );
    }

    // -- 4a: keyof + indexed access --

    #[test]
    fn solve_keyof_object_literal() {
        // keyof { a: string; b: number } → "a" | "b"
        let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Object(Arc::new(
            crate::analysis::type_expr::ObjectExpr {
                properties: vec![
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "a".into(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                        },
                    ),
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "b".into(),
                            ty: TypeExpr::Primitive(PrimitiveName::Number),
                            optional: false,
                            readonly: false,
                        },
                    ),
                ],
            },
        ))));
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        // Should be "a" | "b"
        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(members.contains(&TypeExpr::string_literal("a")));
                assert!(members.contains(&TypeExpr::string_literal("b")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_keyof_with_index_signature_is_open() {
        // keyof { [key: string]: number } → string
        let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Object(Arc::new(
            crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::IndexSignature(
                    crate::analysis::type_expr::IndexSignature {
                        key_name: "key".into(),
                        key_type: TypeExpr::Primitive(PrimitiveName::String),
                        value_type: TypeExpr::Primitive(PrimitiveName::Number),
                        readonly: false,
                    },
                )],
            },
        ))));
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
    }

    #[test]
    fn solve_keyof_array_is_number() {
        let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: false,
        }));
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::Number));
    }

    #[test]
    fn solve_keyof_tuple_is_numeric_index_union() {
        let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Tuple {
            elements: Arc::from(vec![
                crate::analysis::type_expr::TupleElement {
                    label: None,
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    rest: false,
                },
                crate::analysis::type_expr::TupleElement {
                    label: None,
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    rest: false,
                },
            ]),
            readonly: false,
        }));
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Union(members) => {
                assert!(members.contains(&TypeExpr::number_literal(0.0)));
                assert!(members.contains(&TypeExpr::number_literal(1.0)));
            }
            other => panic!("expected numeric literal union, got {other:?}"),
        }
    }

    #[test]
    fn solve_indexed_access_object_literal() {
        // { a: string; b: number }["a"] → string
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::Object(Arc::new(
                crate::analysis::type_expr::ObjectExpr {
                    properties: vec![
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "a".into(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: false,
                                readonly: false,
                            },
                        ),
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "b".into(),
                                ty: TypeExpr::Primitive(PrimitiveName::Number),
                                optional: false,
                                readonly: false,
                            },
                        ),
                    ],
                },
            ))),
            index: Arc::new(TypeExpr::string_literal("a")),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_indexed_access_union_key() {
        // { a: string; b: number }["a" | "b"] → string | number
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::Object(Arc::new(
                crate::analysis::type_expr::ObjectExpr {
                    properties: vec![
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "a".into(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: false,
                                readonly: false,
                            },
                        ),
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "b".into(),
                                ty: TypeExpr::Primitive(PrimitiveName::Number),
                                optional: false,
                                readonly: false,
                            },
                        ),
                    ],
                },
            ))),
            index: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]))),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_indexed_access_intersection_merges_matching_members() {
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::Intersection(Arc::from(vec![
                TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                    properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "variants".into(),
                            ty: TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                                    crate::analysis::type_expr::ObjectProperty {
                                        name: "color".into(),
                                        ty: TypeExpr::Object(Arc::new(
                                            crate::analysis::type_expr::ObjectExpr {
                                                properties: vec![
                                                    crate::analysis::type_expr::ObjectMember::Property(
                                                        crate::analysis::type_expr::ObjectProperty {
                                                            name: "primary".into(),
                                                            ty: TypeExpr::Primitive(
                                                                PrimitiveName::String,
                                                            ),
                                                            optional: false,
                                                            readonly: false,
                                                        },
                                                    ),
                                                    crate::analysis::type_expr::ObjectMember::Property(
                                                        crate::analysis::type_expr::ObjectProperty {
                                                            name: "secondary".into(),
                                                            ty: TypeExpr::Primitive(
                                                                PrimitiveName::String,
                                                            ),
                                                            optional: false,
                                                            readonly: false,
                                                        },
                                                    ),
                                                ],
                                            },
                                        )),
                                        optional: false,
                                        readonly: false,
                                    },
                                )],
                            })),
                            optional: false,
                            readonly: false,
                        },
                    )],
                })),
                TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                    properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "variants".into(),
                            ty: TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                                    crate::analysis::type_expr::ObjectProperty {
                                        name: "color".into(),
                                        ty: TypeExpr::Object(Arc::new(
                                            crate::analysis::type_expr::ObjectExpr {
                                                properties: vec![
                                                    crate::analysis::type_expr::ObjectMember::Property(
                                                        crate::analysis::type_expr::ObjectProperty {
                                                            name: "neutral".into(),
                                                            ty: TypeExpr::Primitive(
                                                                PrimitiveName::String,
                                                            ),
                                                            optional: false,
                                                            readonly: false,
                                                        },
                                                    ),
                                                ],
                                            },
                                        )),
                                        optional: false,
                                        readonly: false,
                                    },
                                )],
                            })),
                            optional: false,
                            readonly: false,
                        },
                    )],
                })),
            ]))),
            index: Arc::new(TypeExpr::string_literal("variants")),
        };

        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match result.value {
            TypeExpr::Intersection(members) => {
                assert_eq!(members.len(), 2);
                assert!(members
                    .iter()
                    .all(|member| matches!(member, TypeExpr::Object(_))));
            }
            other => panic!("expected intersection of object members, got: {:?}", other),
        }
    }

    // -- 4b: conditionals --

    #[test]
    fn solve_conditional_true_branch() {
        // string extends string ? "yes" : "no" → "yes"
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::string_literal("yes")),
            false_type: Arc::new(TypeExpr::string_literal("no")),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::string_literal("yes"));
    }

    #[test]
    fn solve_conditional_false_branch() {
        // number extends string ? "yes" : "no" → "no"
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::string_literal("yes")),
            false_type: Arc::new(TypeExpr::string_literal("no")),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::string_literal("no"));
    }

    // -- 4c: mapped types --

    #[test]
    fn solve_mapped_type_finite_keys() {
        // { [K in "a" | "b"]: number } → { a: number; b: number }
        let expr = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]))),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            optional: crate::analysis::type_expr::MappedModifier::None,
            readonly: crate::analysis::type_expr::MappedModifier::None,
            name_type: None,
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 2);
                // Both properties should be number
                for member in &obj.properties {
                    match member {
                        crate::analysis::type_expr::ObjectMember::Property(p) => {
                            assert!(matches!(p.ty, TypeExpr::Primitive(PrimitiveName::Number)));
                        }
                        _ => panic!("expected property"),
                    }
                }
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_mapped_type_ignores_never_in_keyspace_union() {
        let expr = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::Never),
                TypeExpr::string_literal("base"),
                TypeExpr::string_literal("label"),
            ]))),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            optional: crate::analysis::type_expr::MappedModifier::None,
            readonly: crate::analysis::type_expr::MappedModifier::None,
            name_type: None,
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        let TypeExpr::Object(obj) = result.value else {
            panic!("expected Object");
        };
        let property_names: Vec<_> = obj
            .properties
            .iter()
            .filter_map(|member| match member {
                crate::analysis::type_expr::ObjectMember::Property(property) => {
                    Some(property.name.as_str())
                }
                _ => None,
            })
            .collect();
        let mut property_names = property_names;
        property_names.sort_unstable();

        assert_eq!(property_names, vec!["base", "label"]);
    }

    #[test]
    fn solve_mapped_type_open_source() {
        // { [K in string]: boolean } → { [key: string]: boolean }
        let expr = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Boolean)),
            optional: crate::analysis::type_expr::MappedModifier::None,
            readonly: crate::analysis::type_expr::MappedModifier::None,
            name_type: None,
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Object(obj) => {
                assert!(
                    obj.properties.iter().all(|member| {
                        !matches!(
                            member,
                            crate::analysis::type_expr::ObjectMember::Property(_)
                        )
                    }),
                    "should have no named properties"
                );
                assert!(
                    obj.properties.iter().any(|member| {
                        matches!(
                            member,
                            crate::analysis::type_expr::ObjectMember::IndexSignature(_)
                        )
                    }),
                    "should keep the open index signature"
                );
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    // -- template literals --

    #[test]
    fn solve_template_literal_concrete() {
        // `hello${" "}world` → "hello world"
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["hello".into(), "world".into()],
            expressions: Arc::from(vec![TypeExpr::string_literal(" ")]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::string_literal("hello world"));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_template_literal_union_expansion() {
        // `${"a" | "b"}_suffix` → "a_suffix" | "b_suffix"
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["".into(), "_suffix".into()],
            expressions: Arc::from(vec![TypeExpr::Union(Arc::from(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]))]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(members.contains(&TypeExpr::string_literal("a_suffix")));
                assert!(members.contains(&TypeExpr::string_literal("b_suffix")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_template_literal_with_number() {
        // `count_${42}` → "count_42"
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["count_".into(), "".into()],
            expressions: Arc::from(vec![TypeExpr::number_literal(42.0)]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::string_literal("count_42"));
    }

    // -- Awaited --

    #[test]
    fn solve_awaited_non_thenable() {
        // Awaited<string> → string
        let expr = TypeExpr::Ref {
            name: Arc::from("Awaited"),
            type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::String)]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
    }

    // -- InstanceType / ConstructorParameters --
    // (These require construct signatures on objects which are less common in
    // test fixtures, but the expansion logic is tested via the builtin tests.)

    // -- Mapped type with key remapping --

    #[test]
    fn solve_mapped_type_with_key_remapping_via_conditional() {
        // { [K in "a" | "b" as K extends "a" ? "renamed" : never]: number }
        // → { renamed: number }  (only "a" survives, remapped to "renamed")
        // Note: this tests the name_type path. The conditional filters "b" to never.
        let expr = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]))),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            optional: crate::analysis::type_expr::MappedModifier::None,
            readonly: crate::analysis::type_expr::MappedModifier::None,
            name_type: Some(Arc::new(TypeExpr::Conditional {
                check: Arc::new(TypeExpr::Ref {
                    name: Arc::from("K"),
                    type_arguments: Arc::from(vec![]),
                }),
                extends: Arc::new(TypeExpr::string_literal("a")),
                true_type: Arc::new(TypeExpr::string_literal("renamed")),
                false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
            })),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 1, "only 'a' survives remapping");
                match &obj.properties[0] {
                    crate::analysis::type_expr::ObjectMember::Property(p) => {
                        assert_eq!(p.name, "renamed");
                    }
                    _ => panic!("expected property"),
                }
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_conditional_infer_reuses_intersection_of_multiple_candidates() {
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]))),
            extends: Arc::new(TypeExpr::Infer { name: "A".into() }),
            true_type: Arc::new(TypeExpr::Ref {
                name: Arc::from("A"),
                type_arguments: Arc::from(vec![]),
            }),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Intersection(members) => {
                assert!(members.contains(&TypeExpr::Primitive(PrimitiveName::String)));
                assert!(members.contains(&TypeExpr::Primitive(PrimitiveName::Number)));
            }
            other => panic!("expected intersection, got {other:?}"),
        }
    }

    #[test]
    fn solve_conditional_infers_function_parameter_types_under_contravariance() {
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Function(Arc::new(
                crate::analysis::type_expr::FunctionExpr {
                    parameters: vec![crate::analysis::type_expr::FunctionParam {
                        name: Some("props".into()),
                        ty: TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "planId".into(),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: false,
                                    readonly: false,
                                },
                            )],
                        })),
                        optional: false,
                        rest: false,
                    }],
                    return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                    type_parameters: vec![],
                },
            ))),
            extends: Arc::new(TypeExpr::Function(Arc::new(
                crate::analysis::type_expr::FunctionExpr {
                    parameters: vec![crate::analysis::type_expr::FunctionParam {
                        name: Some("props".into()),
                        ty: TypeExpr::Infer { name: "P".into() },
                        optional: false,
                        rest: false,
                    }],
                    return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                    type_parameters: vec![],
                },
            ))),
            true_type: Arc::new(TypeExpr::Intersection(Arc::from(vec![
                TypeExpr::Ref {
                    name: Arc::from("P"),
                    type_arguments: Arc::from(vec![]),
                },
                TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                    properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "plan".into(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                        },
                    )],
                })),
            ]))),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        let TypeExpr::Intersection(parts) = result.value else {
            panic!("infer conditional should resolve true branch under contravariant function comparison");
        };
        let mut prop_names = std::collections::BTreeSet::new();
        for part in parts.iter() {
            if let TypeExpr::Object(obj) = part {
                for member in &obj.properties {
                    if let crate::analysis::type_expr::ObjectMember::Property(prop) = member {
                        prop_names.insert(prop.name.clone());
                    }
                }
            }
        }
        assert_eq!(
            prop_names,
            std::collections::BTreeSet::from(["plan".to_string(), "planId".to_string()]),
        );
    }

    #[test]
    fn solve_conditional_honors_constrained_type_parameter_relation() {
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::TypeParameter(
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                    default: None,
                },
            )),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::boolean_literal(true)),
            false_type: Arc::new(TypeExpr::boolean_literal(false)),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::boolean_literal(true));
    }

    // ===================================================================
    // Ported from type_eval_tests.rs — complex real-world patterns
    // ===================================================================

    // -- Composition: Partial<Pick<T, K>> --

    #[test]
    fn solve_partial_pick_composition() {
        // Partial<Pick<{ id: number; name: string; email: string }, "name" | "email">>
        // → { name?: string; email?: string }
        let inner_obj = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "id".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::Number),
                        optional: false,
                        readonly: false,
                    },
                ),
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "name".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "email".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
            ],
        }));

        let expr = TypeExpr::Ref {
            name: Arc::from("Partial"),
            type_arguments: Arc::from(vec![TypeExpr::Ref {
                name: Arc::from("Pick"),
                type_arguments: Arc::from(vec![
                    inner_obj,
                    TypeExpr::Union(Arc::from(vec![
                        TypeExpr::string_literal("name"),
                        TypeExpr::string_literal("email"),
                    ])),
                ]),
            }]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 2, "Pick should select name+email");
                for member in &obj.properties {
                    if let crate::analysis::type_expr::ObjectMember::Property(p) = member {
                        assert!(p.optional, "Partial should make {} optional", p.name);
                        assert!(
                            p.name == "name" || p.name == "email",
                            "unexpected property: {}",
                            p.name
                        );
                    }
                }
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    // -- Extract with literal unions --

    #[test]
    fn solve_extract_literal_union() {
        // Extract<"a" | "b" | "c", "a" | "b"> → "a" | "b"
        let expr = TypeExpr::Ref {
            name: Arc::from("Extract"),
            type_arguments: Arc::from(vec![
                TypeExpr::Union(Arc::from(vec![
                    TypeExpr::string_literal("a"),
                    TypeExpr::string_literal("b"),
                    TypeExpr::string_literal("c"),
                ])),
                TypeExpr::Union(Arc::from(vec![
                    TypeExpr::string_literal("a"),
                    TypeExpr::string_literal("b"),
                ])),
            ]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(members.contains(&TypeExpr::string_literal("a")));
                assert!(members.contains(&TypeExpr::string_literal("b")));
                assert!(!members.contains(&TypeExpr::string_literal("c")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    // -- Exclude with literal unions --

    #[test]
    fn solve_exclude_literal_union() {
        // Exclude<"a" | "b" | "c", "a"> → "b" | "c"
        let expr = TypeExpr::Ref {
            name: Arc::from("Exclude"),
            type_arguments: Arc::from(vec![
                TypeExpr::Union(Arc::from(vec![
                    TypeExpr::string_literal("a"),
                    TypeExpr::string_literal("b"),
                    TypeExpr::string_literal("c"),
                ])),
                TypeExpr::string_literal("a"),
            ]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(members.contains(&TypeExpr::string_literal("b")));
                assert!(members.contains(&TypeExpr::string_literal("c")));
                assert!(!members.contains(&TypeExpr::string_literal("a")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    // -- keyof on intersection --

    #[test]
    fn solve_keyof_intersection() {
        // keyof ({ a: string } & { b: number }) → "a" | "b"
        let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "a".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "b".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::Number),
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
        ]))));
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(members.contains(&TypeExpr::string_literal("a")));
                assert!(members.contains(&TypeExpr::string_literal("b")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    // -- Recursive type detection --

    #[test]
    fn solve_recursive_type_does_not_stack_overflow() {
        let mut host = TestHost::new();
        // type Tree = { children: Tree[] }
        host.add_alias(
            "Tree",
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "children".into(),
                        ty: TypeExpr::Array {
                            element: Arc::new(TypeExpr::Ref {
                                name: Arc::from("Tree"),
                                type_arguments: Arc::from(vec![]),
                            }),
                            readonly: false,
                        },
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
        );

        let expr = TypeExpr::Ref {
            name: Arc::from("Tree"),
            type_arguments: Arc::from(vec![]),
        };
        // Should not hang or stack overflow — recursion tracker catches it
        let result = solve_type(&expr, &host, SolveLimits::default());
        // The result should be an object (possibly with a recursive ref for children)
        assert!(
            result.execution_status == ExecutionStatus::Completed
                || result.execution_status == ExecutionStatus::HardStop
        );
    }

    #[test]
    fn solve_structural_recursive_infer_reentry_stays_symbolic_without_hanging() {
        let mut host = TestHost::new();
        host.add_generic_alias(
            "NestedItem",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            TypeExpr::Conditional {
                check: Arc::new(TypeExpr::named("T")),
                extends: Arc::new(TypeExpr::Array {
                    element: Arc::new(TypeExpr::Infer { name: "I".into() }),
                    readonly: false,
                }),
                true_type: Arc::new(TypeExpr::named_with_args(
                    "NestedItem",
                    vec![TypeExpr::Infer { name: "I".into() }],
                )),
                false_type: Arc::new(TypeExpr::named("T")),
            },
        );

        let expr = TypeExpr::named_with_args("NestedItem", vec![TypeExpr::named("Unresolved")]);
        let result = solve_type(&expr, &host, SolveLimits::default());

        assert_eq!(result.execution_status, ExecutionStatus::Completed);
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
    }

    #[test]
    fn solve_substitution_cycle_from_shadowed_default_stays_symbolic_without_hanging() {
        let mut host = TestHost::new();
        host.add_generic_alias(
            "NestedItem",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            TypeExpr::named("T"),
        );
        host.add_generic_alias(
            "Loop",
            vec![
                crate::analysis::type_expr::TypeParam {
                    name: "I".into(),
                    constraint: None,
                    default: None,
                },
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: Some(Arc::new(TypeExpr::named_with_args(
                        "NestedItem",
                        vec![TypeExpr::named("I")],
                    ))),
                },
            ],
            TypeExpr::named("T"),
        );

        let expr = TypeExpr::named_with_args("Loop", vec![TypeExpr::named("T")]);
        let result = solve_type(&expr, &host, SolveLimits::default());

        assert_eq!(result.execution_status, ExecutionStatus::Completed);
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
        assert!(
            matches!(
                result.value,
                TypeExpr::Ref { .. } | TypeExpr::Unknown { .. }
            ),
            "substitution-cycle fallback should stay symbolic, got {:?}",
            result.value
        );
    }

    // -- Generic with host-backed chained resolution --

    #[test]
    fn solve_generic_wrapper_over_host_alias() {
        let mut host = TestHost::new();
        // type Inner = { x: string; y: number }
        // type Wrap<T> = { data: T }
        host.add_alias(
            "Inner",
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "x".into(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                        },
                    ),
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "y".into(),
                            ty: TypeExpr::Primitive(PrimitiveName::Number),
                            optional: false,
                            readonly: false,
                        },
                    ),
                ],
            })),
        );
        host.add_generic_alias(
            "Wrap",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "data".into(),
                        ty: TypeExpr::Ref {
                            name: Arc::from("T"),
                            type_arguments: Arc::from(vec![]),
                        },
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
        );

        // Wrap<Inner> → { data: { x: string; y: number } }
        let expr = TypeExpr::Ref {
            name: Arc::from("Wrap"),
            type_arguments: Arc::from(vec![TypeExpr::Ref {
                name: Arc::from("Inner"),
                type_arguments: Arc::from(vec![]),
            }]),
        };
        let result = solve_type(&expr, &host, SolveLimits::default());

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 1);
                if let crate::analysis::type_expr::ObjectMember::Property(p) = &obj.properties[0] {
                    assert_eq!(p.name, "data");
                    // data should be the resolved Inner = { x: string; y: number }
                    match &p.ty {
                        TypeExpr::Object(inner) => {
                            assert_eq!(inner.properties.len(), 2);
                        }
                        _ => panic!("data should be Object, got: {:?}", p.ty),
                    }
                }
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_generic_default_argument_resolves_bound_alias() {
        let mut host = TestHost::new();
        host.add_alias(
            "Item",
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "id".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
        );
        host.add_generic_alias(
            "Props",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: Some(Arc::new(TypeExpr::named("Item"))),
            }],
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "items".into(),
                        ty: TypeExpr::Array {
                            element: Arc::new(TypeExpr::Ref {
                                name: Arc::from("T"),
                                type_arguments: Arc::from(vec![]),
                            }),
                            readonly: false,
                        },
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
        );

        let expr = TypeExpr::Ref {
            name: Arc::from("Props"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &host, SolveLimits::default());

        match result.value {
            TypeExpr::Object(obj) => match &obj.properties[0] {
                crate::analysis::type_expr::ObjectMember::Property(prop) => match &prop.ty {
                    TypeExpr::Array { element, .. } => match element.as_ref() {
                        TypeExpr::Object(shape) => {
                            assert!(shape.properties.iter().any(|member| {
                                matches!(
                                    member,
                                    crate::analysis::type_expr::ObjectMember::Property(p)
                                        if p.name == "id"
                                )
                            }));
                        }
                        other => {
                            panic!("expected default arg to resolve to Item shape, got {other:?}")
                        }
                    },
                    other => panic!("expected array property, got {other:?}"),
                },
                other => panic!("expected property member, got {other:?}"),
            },
            other => panic!("expected Object, got: {other:?}"),
        }
    }

    #[test]
    fn project_preserves_method_members() {
        let expr = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Method(
                crate::analysis::type_expr::MethodSignature {
                    name: "default".into(),
                    function: crate::analysis::type_expr::FunctionExpr {
                        parameters: vec![crate::analysis::type_expr::FunctionParam {
                            name: Some("props".into()),
                            ty: TypeExpr::Object(Arc::new(
                                crate::analysis::type_expr::ObjectExpr {
                                    properties: vec![
                                        crate::analysis::type_expr::ObjectMember::Property(
                                            crate::analysis::type_expr::ObjectProperty {
                                                name: "label".into(),
                                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                                optional: false,
                                                readonly: false,
                                            },
                                        ),
                                    ],
                                },
                            )),
                            optional: false,
                            rest: false,
                        }],
                        return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                        type_parameters: vec![],
                    },
                    optional: true,
                },
            )],
        }));

        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match result.value {
            TypeExpr::Object(obj) => match &obj.properties[0] {
                crate::analysis::type_expr::ObjectMember::Method(method) => {
                    assert_eq!(method.name, "default");
                    assert!(method.optional);
                    assert_eq!(method.function.parameters.len(), 1);
                }
                other => panic!("expected method member, got: {other:?}"),
            },
            other => panic!("expected Object, got: {other:?}"),
        }
    }

    #[test]
    fn project_preserves_type_parameter_metadata() {
        let expr = TypeExpr::TypeParameter(crate::analysis::type_expr::TypeParam {
            name: "T".into(),
            constraint: Some(Arc::new(TypeExpr::named("Item"))),
            default: Some(Arc::new(TypeExpr::named("Item"))),
        });

        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match result.value {
            TypeExpr::TypeParameter(param) => {
                assert_eq!(param.name, "T");
                assert!(matches!(
                    param.constraint.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
                assert!(matches!(
                    param.default.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
            }
            other => panic!("expected TypeParameter, got: {other:?}"),
        }
    }

    // -- Template literal with multiple unions (cartesian product) --

    #[test]
    fn solve_template_literal_cartesian_product() {
        // `${"a" | "b"}-${"1" | "2"}` → "a-1" | "a-2" | "b-1" | "b-2"
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["".into(), "-".into(), "".into()],
            expressions: Arc::from(vec![
                TypeExpr::Union(Arc::from(vec![
                    TypeExpr::string_literal("a"),
                    TypeExpr::string_literal("b"),
                ])),
                TypeExpr::Union(Arc::from(vec![
                    TypeExpr::string_literal("1"),
                    TypeExpr::string_literal("2"),
                ])),
            ]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 4);
                assert!(members.contains(&TypeExpr::string_literal("a-1")));
                assert!(members.contains(&TypeExpr::string_literal("a-2")));
                assert!(members.contains(&TypeExpr::string_literal("b-1")));
                assert!(members.contains(&TypeExpr::string_literal("b-2")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    // -- Template literal with boolean/null --

    #[test]
    fn solve_template_literal_with_boolean() {
        // `is_${true}` → "is_true"
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["is_".into(), "".into()],
            expressions: Arc::from(vec![TypeExpr::boolean_literal(true)]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::string_literal("is_true"));
    }

    // -- Mapped type with optional modifier --

    #[test]
    fn solve_mapped_type_add_optional() {
        // { [K in "a" | "b"]+?: number } → { a?: number; b?: number }
        let expr = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]))),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            optional: crate::analysis::type_expr::MappedModifier::Add,
            readonly: crate::analysis::type_expr::MappedModifier::None,
            name_type: None,
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 2);
                for member in &obj.properties {
                    if let crate::analysis::type_expr::ObjectMember::Property(p) = member {
                        assert!(p.optional, "{} should be optional", p.name);
                    }
                }
            }
            _ => panic!("expected Object"),
        }
    }

    // -- Conditional: unknown relation stays symbolic --

    #[test]
    fn solve_conditional_unknown_stays_symbolic() {
        // T extends string ? "yes" : "no" — T is unresolved ref
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Ref {
                name: Arc::from("T"),
                type_arguments: Arc::from(vec![]),
            }),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::string_literal("yes")),
            false_type: Arc::new(TypeExpr::string_literal("no")),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        // T is unresolved — relation is Unknown, so conditional stays symbolic
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
    }

    // -- Record with Exclude-derived keys --

    #[test]
    fn solve_record_with_exclude_keys() {
        // Record<Exclude<"a" | "b" | "c", "c">, boolean> → { a: boolean; b: boolean }
        let expr = TypeExpr::Ref {
            name: Arc::from("Record"),
            type_arguments: Arc::from(vec![
                TypeExpr::Ref {
                    name: Arc::from("Exclude"),
                    type_arguments: Arc::from(vec![
                        TypeExpr::Union(Arc::from(vec![
                            TypeExpr::string_literal("a"),
                            TypeExpr::string_literal("b"),
                            TypeExpr::string_literal("c"),
                        ])),
                        TypeExpr::string_literal("c"),
                    ]),
                },
                TypeExpr::Primitive(PrimitiveName::Boolean),
            ]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 2);
                let names: Vec<&str> = obj
                    .properties
                    .iter()
                    .filter_map(|m| match m {
                        crate::analysis::type_expr::ObjectMember::Property(p) => {
                            Some(p.name.as_str())
                        }
                        _ => None,
                    })
                    .collect();
                assert!(names.contains(&"a"));
                assert!(names.contains(&"b"));
                assert!(!names.contains(&"c"));
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    // -- Indexed access through utility --

    #[test]
    fn solve_indexed_access_through_required() {
        // Required<{ a?: string; b?: number }>["a"] → string
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::Ref {
                name: Arc::from("Required"),
                type_arguments: Arc::from(vec![TypeExpr::Object(Arc::new(
                    crate::analysis::type_expr::ObjectExpr {
                        properties: vec![
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "a".into(),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: true,
                                    readonly: false,
                                },
                            ),
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "b".into(),
                                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                                    optional: true,
                                    readonly: false,
                                },
                            ),
                        ],
                    },
                ))]),
            }),
            index: Arc::new(TypeExpr::string_literal("a")),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
    }

    // ===================================================================
    // Edge case and fix-verification tests
    // ===================================================================

    // -- Fix #3: template literal with never expression → never --

    #[test]
    fn solve_template_literal_with_never_is_never() {
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["prefix_".into(), "".into()],
            expressions: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::Never)]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::Never));
    }

    // -- Fix #7: distributive conditional --

    #[test]
    fn solve_distributive_conditional() {
        // type IsString<T> = T extends string ? true : false
        // IsString<string | number> should distribute:
        //   = (string extends string ? true : false) | (number extends string ? true : false)
        //   = true | false
        let mut host = TestHost::new();
        host.add_generic_alias(
            "IsString",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            TypeExpr::Conditional {
                check: Arc::new(TypeExpr::TypeParameter(
                    crate::analysis::type_expr::TypeParam {
                        name: "T".into(),
                        constraint: None,
                        default: None,
                    },
                )),
                extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                true_type: Arc::new(TypeExpr::boolean_literal(true)),
                false_type: Arc::new(TypeExpr::boolean_literal(false)),
            },
        );

        let expr = TypeExpr::Ref {
            name: Arc::from("IsString"),
            type_arguments: Arc::from(vec![TypeExpr::Union(Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]))]),
        };
        let result = solve_type(&expr, &host, SolveLimits::default());

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2, "should be true | false");
                assert!(members.contains(&TypeExpr::boolean_literal(true)));
                assert!(members.contains(&TypeExpr::boolean_literal(false)));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    // -- Fix #11: Record<never, V> = {} --

    #[test]
    fn solve_record_never_key_is_empty_object() {
        let expr = TypeExpr::Ref {
            name: Arc::from("Record"),
            type_arguments: Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::Never),
                TypeExpr::Primitive(PrimitiveName::String),
            ]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Object(obj) => {
                assert!(
                    obj.properties.is_empty(),
                    "Record<never, V> should be empty"
                );
            }
            _ => panic!("expected empty Object, got: {:?}", result.value),
        }
    }

    // -- Fix #12: NonNullable<any> = any --

    #[test]
    fn solve_non_nullable_any_is_any() {
        let expr = TypeExpr::Ref {
            name: Arc::from("NonNullable"),
            type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::Any)]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::Any));
    }

    // -- Fix #5: project_to_type_expr handles tuples --

    #[test]
    fn solve_parameters_projects_to_tuple_type_expr() {
        let expr = TypeExpr::Ref {
            name: Arc::from("Parameters"),
            type_arguments: Arc::from(vec![TypeExpr::Function(Arc::new(
                crate::analysis::type_expr::FunctionExpr {
                    parameters: vec![
                        crate::analysis::type_expr::FunctionParam {
                            name: Some("a".into()),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            rest: false,
                        },
                        crate::analysis::type_expr::FunctionParam {
                            name: Some("b".into()),
                            ty: TypeExpr::Primitive(PrimitiveName::Number),
                            optional: false,
                            rest: false,
                        },
                    ],
                    return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Void))),
                    type_parameters: vec![],
                },
            ))]),
        };
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        // Should project back as a Tuple TypeExpr, not Unknown
        match &result.value {
            TypeExpr::Tuple { elements, .. } => {
                assert_eq!(elements.len(), 2);
                assert!(matches!(
                    elements[0].ty,
                    TypeExpr::Primitive(PrimitiveName::String)
                ));
                assert!(matches!(
                    elements[1].ty,
                    TypeExpr::Primitive(PrimitiveName::Number)
                ));
            }
            _ => panic!("expected Tuple, got: {:?}", result.value),
        }
    }

    // -- Fix #5: project_to_type_expr handles functions --

    #[test]
    fn solve_function_type_round_trips() {
        let expr = TypeExpr::Function(Arc::new(crate::analysis::type_expr::FunctionExpr {
            parameters: vec![crate::analysis::type_expr::FunctionParam {
                name: Some("x".into()),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                rest: false,
            }],
            return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
            type_parameters: vec![],
        }));
        let result = solve_type(&expr, &NoopSolverHost, SolveLimits::default());

        match &result.value {
            TypeExpr::Function(f) => {
                assert_eq!(f.parameters.len(), 1);
                assert!(f.return_type.is_some());
            }
            _ => panic!("expected Function, got: {:?}", result.value),
        }
    }
}
