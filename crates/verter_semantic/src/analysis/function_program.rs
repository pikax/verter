//! Arena-free per-file `FunctionProgramIndex`: the structural inventory of
//! every served authored function position in one parsed file.
//!
//! The index is a SHALLOW structural product built once per parsed file
//! version from the retained parse snapshot: exact function identities and
//! body locators, binding/reference inventory, return sites, writes and
//! evaluation effects, the control-region skeleton, exact direct local call
//! targets, and a whole-function `flow_body_stable_hash`. It borrows no OXC
//! node and lowers no type tree — lowering of one demanded function into
//! typed IR happens later, per function, over these locators.
//!
//! Hash rules (`flow_body_stable_hash`): the fold preserves observable
//! property / destructuring / computed keys, operators, literals, calls,
//! writes, control structure, authored type annotations (return,
//! type-parameter, and EVERY parameter annotation), parameter default
//! initializers, and type-affecting JSDoc (`@param` / `@returns` /
//! `@return` / `@type` payloads). Only binding/reference identifier
//! positions are alpha-normalized — a local rename that preserves
//! structure keeps the hash; a property key, free name, literal, operator,
//! control, parameter-annotation, or default-initializer edit changes it.
use verter_session_query::function_program::{
    canonical_runtime_binding_slots, CanonicalCaptureIdentity, ClassSyntaxRecord,
    FlowBindingIdentity, FunctionBindingKind, FunctionBindingRecord, FunctionBodyLocator,
    FunctionCallArgLiteralMode, FunctionCallArgRecord, FunctionCallSiteRecord,
    FunctionCapturedRead, FunctionControlKind, FunctionControlRegion, FunctionDeclarationRef,
    FunctionDescent, FunctionDescentStep, FunctionDirectCall, FunctionEffectCallee,
    FunctionEffectRecord, FunctionNestedCaptures, FunctionParamRecord,
    FunctionParameterCallableCaptures, FunctionProgramEntry, FunctionProgramIndex,
    FunctionProgramKey, FunctionProgramTypeParam, FunctionReadRole, FunctionReferenceBinding,
    FunctionReferenceRecord, FunctionReturnSite, FunctionSourceTypeQuery, FunctionTypeQuery,
    FunctionTypeQueryPosition, FunctionWriteKind, FunctionWriteRecord, FunctionWriteTarget,
    ProgramExpressionCallKind, ProgramExpressionRecord, ProgramExpressionSource,
};

use std::sync::Arc;

use oxc_ast::ast::{
    ArrowFunctionExpression, BindingPattern, CallExpression, Class, Expression, Function,
    MethodDefinitionKind, ObjectPropertyKind, PropertyKey, Statement, VariableDeclaration,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::facts::{
    FlowFunctionReturnIdentity, FunctionReturnSource, ProgramExpressionIdentity,
};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};
use verter_type_expr::span_origins::DeclContributorAnchor;

use verter_session_query::analysis::top_level_owners::TopLevelOwnerTable;
use verter_session_query::analysis::types::Hash16;
use verter_session_query::facts::SymbolSpace;

#[path = "function_program_access.rs"]
pub(crate) mod access;

#[cfg(test)]
#[path = "function_program_tests.rs"]
mod function_program_tests;

/// Collects every class of one parsed file, in source order.
#[derive(Default)]
struct ClassSyntaxCollector {
    classes: Vec<ClassSyntaxRecord>,
}

impl<'a> Visit<'a> for ClassSyntaxCollector {
    fn visit_class(&mut self, class: &Class<'a>) {
        let mut members = Vec::with_capacity(class.body.body.len());
        for element in &class.body.body {
            members.push(verter_span::Span::new(
                element.span().start,
                element.span().end,
            ));
            if let oxc_ast::ast::ClassElement::MethodDefinition(method) = element {
                if method.kind == MethodDefinitionKind::Constructor {
                    members.extend(
                        method
                            .value
                            .params
                            .items
                            .iter()
                            .filter(|parameter| {
                                parameter.accessibility.is_some()
                                    || parameter.readonly
                                    || parameter.r#override
                            })
                            .map(|parameter| {
                                verter_span::Span::new(parameter.span.start, parameter.span.end)
                            }),
                    );
                }
            }
        }
        self.classes.push(ClassSyntaxRecord {
            span: verter_span::Span::new(class.span.start, class.span.end),
            expression: class.r#type == oxc_ast::ast::ClassType::ClassExpression,
            has_heritage: class.heritage.is_some(),
            members: Arc::from(members.into_boxed_slice()),
        });
        walk::walk_class(self, class);
    }
}

// ---------------------------------------------------------------------------
// Discovery walk
// ---------------------------------------------------------------------------

struct DiscoveryCtx<'source, 'ast> {
    canonical_id: Arc<str>,
    source: &'source str,
    /// The header walk's classification of the class fields.
    class_fields: &'source crate::analysis::class_field_value::ClassFieldValues,
    /// The containment every walk of oxc's over a node of the program runs
    /// under, scanning the program at most once for all of them.
    walks: verter_parser::oxc_parse::ProgramWalkStack<'ast>,
    owners: &'source TopLevelOwnerTable,
    nodes: Option<FunctionProgramNodes<'ast>>,
    enclosing_type_parameters: Option<&'ast oxc_ast::ast::TSTypeParameterDeclaration<'ast>>,
    enclosing_heritage: Option<EnclosingHeritage<'ast>>,
    enclosing_this: Option<EnclosingThis>,
    entries: Vec<FunctionProgramEntry>,
    /// Each entry's function node, by entry ordinal, whose hashes
    /// [`hash_entries`] folds once discovery is done.
    hashed_nodes: Vec<(usize, FunctionNode<'ast>)>,
    expressions: Vec<ProgramExpressionRecord>,
    /// Source-order ordinal counter for nested served positions (hoisted
    /// nested function declarations and call-argument function values)
    /// across the file.
    next_nested_ordinal: u32,
}

impl<'source, 'ast> DiscoveryCtx<'source, 'ast> {
    fn anchor(&self, contributor_index: usize) -> Option<DeclContributorAnchor> {
        let owner = self.owners.statements().get(contributor_index)?;
        Some(DeclContributorAnchor {
            contributor_index: u32::try_from(contributor_index).ok()?,
            owner: owner.owner,
            owner_local_ordinal: owner.owner_local_ordinal,
        })
    }

    fn push(&mut self, entry: FunctionProgramEntry, node: FunctionNode<'ast>) {
        if let Some(nodes) = &mut self.nodes {
            let self_name = match entry.locator.descent.last() {
                Some(FunctionDescentStep::VariableInitializer { .. }) => Some(Arc::from(
                    entry
                        .key
                        .declaration
                        .name
                        .rsplit('.')
                        .next()
                        .expect("declaration has a name"),
                )),
                Some(
                    FunctionDescentStep::ClassMember { .. }
                    | FunctionDescentStep::ObjectMember { .. }
                    | FunctionDescentStep::ExportDefaultObjectMember { .. },
                ) => None,
                _ => match node {
                    FunctionNode::Function(function) => {
                        function.id.as_ref().map(|id| Arc::from(id.name.as_str()))
                    }
                    FunctionNode::Arrow(_) | FunctionNode::Initializer(_) => None,
                },
            };
            nodes
                .functions
                .entry(entry.key.clone())
                .or_insert(ResolvedFunctionNode {
                    node,
                    self_name,
                    enclosing_type_parameters: self.enclosing_type_parameters,
                    enclosing_heritage: self.enclosing_heritage,
                    enclosing_this: self.enclosing_this,
                });
        }
        self.hashed_nodes.push((self.entries.len(), node));
        self.entries.push(entry);
    }
}

/// Build the per-file function program index from one retained parse
/// snapshot. One structural walk; no lowering, no type resolution.
pub fn build_function_program_index(
    program: &oxc_ast::ast::Program<'_>,
    source: &str,
    owners: &TopLevelOwnerTable,
    canonical_id: Arc<str>,
) -> FunctionProgramIndex {
    // No header walk ran: each class's fields are classified as discovery
    // meets them.
    let class_fields = crate::analysis::class_field_value::ClassFieldValues::default();
    build_function_program_index_impl(program, source, owners, canonical_id, &class_fields, None).0
}

/// Index one retained parse and register exact arena addresses in the same walk.
/// The nodes belong on the retained worker; only the content-free index may cross it.
/// `class_fields` is the header walk's classification of the class fields.
pub fn build_function_program_index_with_nodes<'ast>(
    program: &'ast oxc_ast::ast::Program<'ast>,
    source: &str,
    owners: &TopLevelOwnerTable,
    canonical_id: Arc<str>,
    class_fields: &crate::analysis::class_field_value::ClassFieldValues,
) -> (FunctionProgramIndex, FunctionProgramNodes<'ast>) {
    let (index, nodes) = build_function_program_index_impl(
        program,
        source,
        owners,
        canonical_id,
        class_fields,
        Some(FunctionProgramNodes::default()),
    );
    (index, nodes.expect("retained indexing requested nodes"))
}

fn build_function_program_index_impl<'ast>(
    program: &'ast oxc_ast::ast::Program<'ast>,
    source: &str,
    owners: &TopLevelOwnerTable,
    canonical_id: Arc<str>,
    class_fields: &crate::analysis::class_field_value::ClassFieldValues,
    nodes: Option<FunctionProgramNodes<'ast>>,
) -> (FunctionProgramIndex, Option<FunctionProgramNodes<'ast>>) {
    let mut ctx = DiscoveryCtx {
        canonical_id,
        source,
        class_fields,
        walks: verter_parser::oxc_parse::ProgramWalkStack::new(program),
        owners,
        nodes,
        enclosing_type_parameters: None,
        enclosing_heritage: None,
        enclosing_this: None,
        entries: Vec::new(),
        hashed_nodes: Vec::new(),
        expressions: Vec::new(),
        next_nested_ordinal: 0,
    };
    let mut overload_tracker = OverloadTracker::default();
    // Discovery walks every function of the program, each walk sized for
    // what it walks: they all run inside one containment sized for the
    // program, which a walk of any node in it cannot exceed, rather than
    // each taking a stack segment of its own.
    let mut classes = ClassSyntaxCollector::default();
    verter_parser::oxc_parse::ProgramWalkStack::within(
        &mut ctx,
        |ctx| &ctx.walks,
        |ctx| {
            for (contributor_index, stmt) in program.body.iter().enumerate() {
                discover_statement(stmt, contributor_index, None, &mut overload_tracker, ctx);
            }
            ctx.walks
                .with_node_stack(program.span, || classes.visit_program(program));
            hash_entries(ctx);
        },
    );
    resolve_captures(&mut ctx.entries);
    resolve_nested_capture_reads(&mut ctx.entries);
    resolve_call_site_targets(&mut ctx.entries);
    resolve_direct_calls(&mut ctx.entries);
    link_callback_return_sources(&ctx.canonical_id, &mut ctx.entries, &mut ctx.expressions);
    ctx.expressions.sort_by_key(|record| record.span.start);
    (
        FunctionProgramIndex::from_discovery(ctx.entries, ctx.expressions, classes.classes),
        ctx.nodes,
    )
}

/// Fold every entry's stable and exact hashes, the functions nested in a
/// function before it (discovery lists a function before the functions
/// nested in it), each nested function's hashes folded into the one around
/// it rather than its syntax walked again: a function's hashes cost its own
/// syntax, however many functions it nests.
///
/// The exact hash is the function's own bytes with each function nested
/// directly in it replaced by that function's exact hash and length: equal
/// exactly when the function's text is (a function with none nested hashes
/// its text).
fn hash_entries(ctx: &mut DiscoveryCtx<'_, '_>) {
    use crate::analysis::function_program_hash::{hash_function_body, NestedHashes};
    let mut nested: NestedHashes = rustc_hash::FxHashMap::default();
    // The exact hash and span of each hashed function, and the functions
    // nested directly in each, by entry ordinal.
    let mut exact: rustc_hash::FxHashMap<usize, (Option<Hash16>, verter_span::Span)> =
        rustc_hash::FxHashMap::default();
    let ordinal_of: rustc_hash::FxHashMap<FunctionProgramKey, usize> = ctx
        .entries
        .iter()
        .enumerate()
        .map(|(ordinal, entry)| (entry.key.clone(), ordinal))
        .collect();
    let mut children: rustc_hash::FxHashMap<usize, Vec<usize>> = rustc_hash::FxHashMap::default();
    for (ordinal, entry) in ctx.entries.iter().enumerate() {
        if let Some(parent) = entry
            .lexical_parent
            .as_deref()
            .and_then(|key| ordinal_of.get(key))
        {
            children.entry(*parent).or_default().push(ordinal);
        }
    }
    let hashed = std::mem::take(&mut ctx.hashed_nodes);
    for (ordinal, node) in hashed.into_iter().rev() {
        let Some(body) = node.body() else {
            continue;
        };
        let (params, function_start) = {
            let entry = &ctx.entries[ordinal];
            (Arc::clone(&entry.params), entry.span.start)
        };
        let (stable, part) = hash_function_body(
            &ctx.walks,
            ctx.source,
            body,
            &params,
            function_start,
            node,
            &nested,
        );
        let span = node.span();
        nested.insert((span.start, span.end), part);
        let span: verter_span::Span = verter_span::Span::new(span.start, span.end);
        // A span outside the source is a MISS, never the empty string's
        // hash: hashing `b""` gives every out-of-range entry the same
        // constant and silently retires the exact-content axis for all of
        // them.
        let exact_hash = ctx
            .source
            .get(span.start as usize..span.end as usize)
            .map(|_| {
                let mut inner: Vec<(verter_span::Span, Option<Hash16>)> = children
                    .get(&ordinal)
                    .into_iter()
                    .flatten()
                    .filter_map(|child| exact.get(child))
                    .map(|(hash, child_span)| (*child_span, *hash))
                    .filter(|(child_span, _)| {
                        child_span.start >= span.start && child_span.end <= span.end
                    })
                    .collect();
                inner.sort_by_key(|(child_span, _)| child_span.start);
                let mut bytes = Vec::new();
                let mut at = span.start as usize;
                for (child_span, hash) in inner {
                    let (start, end) = (child_span.start as usize, child_span.end as usize);
                    if start < at {
                        continue;
                    }
                    bytes.extend_from_slice(&ctx.source.as_bytes()[at..start]);
                    match hash {
                        Some(hash) => {
                            bytes.push(0xFF);
                            bytes.extend_from_slice(&hash);
                            bytes.extend_from_slice(&((end - start) as u32).to_le_bytes());
                        }
                        None => bytes.extend_from_slice(&ctx.source.as_bytes()[start..end]),
                    }
                    at = end;
                }
                bytes.extend_from_slice(&ctx.source.as_bytes()[at..span.end as usize]);
                verter_session_query::analysis::types::hash_16(&bytes)
            });
        exact.insert(ordinal, (exact_hash, span));
        let entry = &mut ctx.entries[ordinal];
        entry.flow_body_stable_hash = stable;
        entry.flow_body_exact_hash = exact_hash;
    }
}

/// Resolve exact direct local call targets after discovery: a bare
/// identifier callee whose name binds a served function in the same index
/// (same file, same namespace qualification) targets the highest-ordinal
/// entry for that name — the trailing implementation of its overload
/// group. Computed callees, member calls, and unresolved names are never
/// direct calls.
fn resolve_direct_calls(entries: &mut [FunctionProgramEntry]) {
    let candidates: Vec<(Arc<str>, FunctionPartIdentity, u32, FunctionProgramKey)> = entries
        .iter()
        .map(|entry| {
            (
                Arc::clone(&entry.key.declaration.name),
                entry.key.part.clone(),
                entry.key.overload_ordinal,
                entry.key.clone(),
            )
        })
        .collect();
    for entry in entries.iter_mut() {
        let caller_ns = entry
            .key
            .declaration
            .name
            .rsplit_once('.')
            .map(|(ns, _)| ns.to_string());
        let mut direct = Vec::new();
        for effect in entry.effects.iter() {
            let FunctionEffectCallee::Identifier(callee) = &effect.callee else {
                continue;
            };
            // Lexical preference: the namespace-qualified binding
            // (`N.callee`) shadows the file-global one, exactly like
            // scoped name resolution — never the globally-highest overload
            // ordinal across both spellings.
            let best_for = |spelling: &str| {
                candidates
                    .iter()
                    .filter(|(name, part, _, _)| {
                        name.as_ref() == spelling
                            && matches!(
                                part,
                                FunctionPartIdentity::DeclarationBody
                                    | FunctionPartIdentity::Initializer
                            )
                    })
                    .max_by_key(|(_, _, ordinal, _)| *ordinal)
                    .map(|(_, _, _, key)| key.clone())
            };
            let target = caller_ns
                .as_ref()
                .and_then(|ns| best_for(&format!("{ns}.{callee}")))
                .or_else(|| best_for(callee));
            if let Some(target) = target {
                direct.push(FunctionDirectCall {
                    span: effect.span,
                    target,
                });
            }
        }
        entry.direct_calls = Arc::from(direct.into_boxed_slice());
    }
}

/// Whether `scope` lexically contains `site`.
fn scope_contains(scope: verter_span::Span, site: verter_span::Span) -> bool {
    scope.start <= site.start && site.end <= scope.end
}

/// Compute every nested position's content-free capture identities: the
/// referenced names that bind in an enclosing frame, resolved LEXICALLY —
/// innermost enclosing frame first, and within a frame the innermost
/// same-name binding whose scope contains the capturing position. Each
/// distinct captured BINDING is recorded once, in first-reference source
/// order; identity is the `(defining frame, binding slot)` pair, so two
/// same-name binders never collapse. A name binding in NO enclosing frame
/// is not a capture (a free/global reference).
fn resolve_captures(entries: &mut [FunctionProgramEntry]) {
    // Snapshot the frame bindings + parents up front (no borrow conflicts).
    // The binding inventories are shared, not copied, and one key -> position
    // index resolves the whole parent chain by lookup. Duplicate keys keep
    // the FIRST position, matching source order.
    let frame_bindings: Vec<Arc<[FunctionBindingRecord]>> = entries
        .iter()
        .map(|entry| Arc::clone(&entry.bindings))
        .collect();
    let frame_keys: Vec<FunctionProgramKey> =
        entries.iter().map(|entry| entry.key.clone()).collect();
    let parents: Vec<Option<FunctionProgramKey>> = entries
        .iter()
        .map(|entry| entry.lexical_parent.as_deref().cloned())
        .collect();
    let mut position_of: rustc_hash::FxHashMap<FunctionProgramKey, usize> =
        rustc_hash::FxHashMap::with_capacity_and_hasher(entries.len(), Default::default());
    for (position, entry) in entries.iter().enumerate() {
        position_of.entry(entry.key.clone()).or_insert(position);
    }
    let runtime_slots: Vec<_> = frame_bindings
        .iter()
        .map(|bindings| canonical_runtime_binding_slots(bindings))
        .collect();
    let lexical_scopes: Vec<_> = entries.iter().map(LexicalScopeIndex::build).collect();
    let mut descendant_writes = vec![Vec::new(); entries.len()];
    let mut descendant_seen = vec![rustc_hash::FxHashSet::default(); entries.len()];
    let mut descendant_assignments = vec![Vec::new(); entries.len()];
    let mut assignment_seen = vec![rustc_hash::FxHashSet::default(); entries.len()];
    // Each frame's enclosing frame, by position: the chain a name resolves
    // through is walked from the frame only as far as the name needs, never
    // built whole for every frame (which cost the square of the nesting).
    let parent_position: Vec<Option<usize>> = parents
        .iter()
        .map(|parent| {
            parent
                .as_ref()
                .and_then(|key| position_of.get(key).copied())
        })
        .collect();
    for index in 0..entries.len() {
        // The enclosing frame chain, innermost first.
        let chain =
            || std::iter::successors(parent_position[index], |frame| parent_position[*frame]);
        let site = entries[index].span;
        let resolve = |name: &Arc<str>, span: verter_span::Span| {
            let Some((frame, slot)) = resolve_lexical_binding([index], &lexical_scopes, name, span)
                .or_else(|| resolve_lexical_binding(chain(), &lexical_scopes, name, site))
            else {
                return FunctionReferenceBinding::Free;
            };
            let LexicalBinding::Modeled(slot) = slot else {
                return FunctionReferenceBinding::UnmodeledLocal;
            };
            let slot = runtime_slots[frame][slot as usize];
            FunctionReferenceBinding::Resolved(FlowBindingIdentity {
                name: Arc::clone(&frame_bindings[frame][slot as usize].name),
                kind: frame_bindings[frame][slot as usize].kind,
                defining_function: frame_keys[frame].clone(),
                binding_slot: slot,
                evolving_array: frame_bindings[frame][slot as usize].evolving_array,
            })
        };
        let mut captured_sites = Vec::new();
        for reference in Arc::make_mut(&mut entries[index].references) {
            reference.binding = resolve(&reference.name, reference.span);
            if let FunctionReferenceBinding::Resolved(identity) = &reference.binding {
                if identity.defining_function != frame_keys[index] {
                    captured_sites.push((reference.span.start, identity.clone()));
                }
            }
        }
        for write in Arc::make_mut(&mut entries[index].writes) {
            for target in Arc::make_mut(&mut write.targets) {
                let FunctionWriteTarget::Binding { reference, kind } = target else {
                    continue;
                };
                reference.binding = resolve(&reference.name, reference.span);
                if let FunctionReferenceBinding::Resolved(identity) = &reference.binding {
                    if identity.defining_function != frame_keys[index] {
                        captured_sites.push((reference.span.start, identity.clone()));
                        let defining = position_of[&identity.defining_function];
                        if descendant_seen[defining].insert(identity.binding_slot) {
                            descendant_writes[defining].push(identity.clone());
                        }
                        if *kind == FunctionWriteKind::Whole
                            && assignment_seen[defining].insert(identity.binding_slot)
                        {
                            descendant_assignments[defining].push(identity.clone());
                        }
                    }
                }
            }
        }
        // A parameter-list callable's references resolve in this frame's
        // scopes; every one resolved makes its capture set exact.
        let mut parameter_callable_captures = Vec::new();
        for (span, references) in entries[index].parameter_callable_references.iter() {
            let mut bindings: Vec<FlowBindingIdentity> = Vec::new();
            let mut reads: Vec<FunctionCapturedRead> = Vec::new();
            let mut exact = true;
            for reference in references.iter() {
                match resolve(&reference.name, reference.span) {
                    FunctionReferenceBinding::Resolved(identity) => {
                        if !bindings.contains(&identity) {
                            bindings.push(identity.clone());
                        }
                        if reference.read_role.is_some() {
                            reads.push(FunctionCapturedRead {
                                binding: identity,
                                path: Arc::clone(&reference.path),
                                span: reference.span,
                            });
                        }
                    }
                    FunctionReferenceBinding::Free => {}
                    _ => exact = false,
                }
            }
            if exact {
                reads.sort_by_key(|read| read.span.start);
                parameter_callable_captures.push(FunctionParameterCallableCaptures {
                    span: *span,
                    bindings: CanonicalCaptureIdentity(Arc::from(bindings.into_boxed_slice())),
                    reads: Arc::from(reads.into_boxed_slice()),
                });
            }
        }
        entries[index].parameter_callable_captures = parameter_callable_captures.into();
        // Code no entry serves (a class, a parameter-list callable) is a
        // callable nested here too: its escaping assignments reach the
        // defining frame, this one included.
        for reference in Arc::make_mut(&mut entries[index].unserved_assignments) {
            reference.binding = resolve(&reference.name, reference.span);
            if let FunctionReferenceBinding::Resolved(identity) = &reference.binding {
                let defining = position_of[&identity.defining_function];
                if assignment_seen[defining].insert(identity.binding_slot) {
                    descendant_assignments[defining].push(identity.clone());
                }
            }
        }
        captured_sites.sort_by_key(|(span, _)| *span);
        for query in Arc::make_mut(&mut entries[index].source_type_queries) {
            query.binding = resolve(&query.name, query.span);
        }
        for query in Arc::make_mut(&mut entries[index].type_queries) {
            query.binding = resolve(&query.name, query.span);
        }
        let mut seen = rustc_hash::FxHashSet::default();
        let captures: Vec<_> = captured_sites
            .into_iter()
            .filter_map(|(_, identity)| seen.insert(identity.clone()).then_some(identity))
            .collect();
        entries[index].captures = CanonicalCaptureIdentity(Arc::from(captures.into_boxed_slice()));
    }
    for ((entry, writes), assignments) in entries
        .iter_mut()
        .zip(descendant_writes)
        .zip(descendant_assignments)
    {
        entry.descendant_writes = writes.into();
        entry.descendant_assignments = assignments.into();
    }
}

/// Carry closure-cell dependencies through intervening callable values without
/// rewalking their ASTs or constructing any child flow skeleton.
fn resolve_nested_capture_reads(entries: &mut [FunctionProgramEntry]) {
    let positions: rustc_hash::FxHashMap<_, _> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| (entry.key.clone(), i))
        .collect();
    let mut children = vec![Vec::new(); entries.len()];
    let parent_position: Vec<Option<usize>> = entries
        .iter()
        .map(|entry| {
            entry
                .lexical_parent
                .as_deref()
                .and_then(|parent| positions.get(parent).copied())
        })
        .collect();
    for (i, parent) in parent_position.iter().enumerate() {
        if let Some(parent) = parent {
            children[*parent].push(i);
        }
    }
    // Each frame's nesting under its outermost enclosing frame, each
    // computed once from its parent's (walking every frame's whole chain
    // cost the square of the nesting).
    let mut nesting: Vec<Option<usize>> = vec![None; entries.len()];
    let mut pending = Vec::new();
    for i in 0..entries.len() {
        let mut frame = i;
        let mut known = loop {
            if let Some(known) = nesting[frame] {
                break known;
            }
            match parent_position[frame] {
                Some(parent) if nesting[frame].is_none() => {
                    pending.push(frame);
                    frame = parent;
                }
                _ => {
                    nesting[frame] = Some(0);
                    break 0;
                }
            }
        };
        while let Some(frame) = pending.pop() {
            known += 1;
            nesting[frame] = Some(known);
        }
    }
    let mut order: Vec<_> = nesting
        .iter()
        .enumerate()
        .map(|(i, nesting)| (std::cmp::Reverse(nesting.unwrap_or(0)), i))
        .collect();
    order.sort_unstable();
    for (_, i) in order {
        let key = &entries[i].key;
        let mut reads = Vec::new();
        let mut captured_sites = Vec::new();
        for reference in entries[i].references.iter() {
            let FunctionReferenceBinding::Resolved(binding) = &reference.binding else {
                continue;
            };
            if binding.defining_function == *key {
                continue;
            }
            captured_sites.push((reference.span.start, binding.clone()));
            if reference.read_role.is_some() {
                reads.push(FunctionCapturedRead {
                    binding: binding.clone(),
                    path: Arc::clone(&reference.path),
                    span: reference.span,
                });
            }
        }
        for target in entries[i]
            .writes
            .iter()
            .flat_map(|write| write.targets.iter())
        {
            if let FunctionWriteTarget::Binding { reference, .. } = target {
                if let FunctionReferenceBinding::Resolved(binding) = &reference.binding {
                    if binding.defining_function != *key {
                        captured_sites.push((reference.span.start, binding.clone()));
                    }
                }
            }
        }
        let mut nested = Vec::new();
        let mut exhaustive = entries[i].captures_exhaustive;
        for &child in &children[i] {
            let child = &entries[child];
            exhaustive &= child.captures_exhaustive;
            nested.push(FunctionNestedCaptures {
                function: child.key.clone(),
                span: child.span,
                bindings: child.captures.clone(),
                reads: Arc::clone(&child.captured_reads),
                exhaustive: child.captures_exhaustive,
            });
            reads.extend(
                child
                    .captured_reads
                    .iter()
                    .filter(|read| read.binding.defining_function != *key)
                    .cloned(),
            );
            captured_sites.extend(
                child
                    .captures
                    .0
                    .iter()
                    .filter(|binding| binding.defining_function != *key)
                    .map(|binding| (child.span.start, binding.clone())),
            );
        }
        reads.sort_by_key(|read| read.span.start);
        let mut seen_reads = rustc_hash::FxHashSet::default();
        reads.retain(|read| seen_reads.insert((read.binding.clone(), Arc::clone(&read.path))));
        captured_sites.sort_by_key(|(span, _)| *span);
        let mut seen_captures = rustc_hash::FxHashSet::default();
        let captures: Vec<_> = captured_sites
            .into_iter()
            .filter_map(|(_, identity)| seen_captures.insert(identity.clone()).then_some(identity))
            .collect();
        entries[i].captured_reads = reads.into();
        entries[i].nested_captures = nested.into();
        entries[i].captures = CanonicalCaptureIdentity(captures.into());
        entries[i].captures_exhaustive = exhaustive;
    }
}

#[derive(Clone, Copy)]
enum LexicalBinding {
    Modeled(u32),
    UnmodeledLocal,
}

/// A lexical scope's declaration table, with source-order ties already resolved.
struct LexicalScope {
    span: verter_span::Span,
    parent: Option<usize>,
    bindings: rustc_hash::FxHashMap<Arc<str>, LexicalBinding>,
}

/// Scope intervals are laminar. A binary lookup finds the innermost possible
/// scope, then only lexical ancestors are visited; sibling declarations never
/// participate in one another's name lookup.
struct LexicalScopeIndex {
    scopes: Vec<LexicalScope>,
}

impl LexicalScopeIndex {
    fn build(entry: &FunctionProgramEntry) -> Self {
        let mut tables =
            rustc_hash::FxHashMap::<_, rustc_hash::FxHashMap<Arc<str>, LexicalBinding>>::default();
        tables.entry(entry.span).or_default();
        for (slot, binding) in entry.bindings.iter().enumerate() {
            // Body declarations can share runtime variables with parameters,
            // but they do not belong to the parameter-default environment.
            let scope = if binding.scope_span == entry.span
                && scope_contains(entry.body_span, binding.span)
            {
                entry.body_span
            } else {
                binding.scope_span
            };
            tables.entry(scope).or_default().insert(
                Arc::clone(&binding.name),
                LexicalBinding::Modeled(slot as u32),
            );
        }
        for binding in entry.unmodeled_bindings.iter() {
            tables
                .entry(binding.scope_span)
                .or_default()
                .insert(Arc::clone(&binding.name), LexicalBinding::UnmodeledLocal);
        }
        let mut scopes: Vec<_> = tables
            .into_iter()
            .map(|(span, bindings)| LexicalScope {
                span,
                parent: None,
                bindings,
            })
            .collect();
        scopes.sort_by_key(|scope| (scope.span.start, std::cmp::Reverse(scope.span.end)));
        let mut open: Vec<usize> = Vec::new();
        for index in 0..scopes.len() {
            while open
                .last()
                .is_some_and(|parent| !scope_contains(scopes[*parent].span, scopes[index].span))
            {
                open.pop();
            }
            scopes[index].parent = open.last().copied();
            open.push(index);
        }
        Self { scopes }
    }

    fn resolve(&self, name: &str, site: verter_span::Span) -> Option<LexicalBinding> {
        let mut index = self
            .scopes
            .partition_point(|scope| scope.span.start <= site.start)
            .checked_sub(1)?;
        loop {
            let scope = &self.scopes[index];
            #[cfg(test)]
            LEXICAL_CANDIDATE_VISITS.with(|visits| visits.set(visits.get() + 1));
            if scope_contains(scope.span, site) {
                if let Some(slot) = scope.bindings.get(name) {
                    return Some(*slot);
                }
            }
            index = scope.parent?;
        }
    }
}

#[cfg(test)]
thread_local! {
    static LEXICAL_CANDIDATE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Resolve against containing scopes, innermost frame first. Runtime variable
/// canonicalization follows this exact lexical lookup.
fn resolve_lexical_binding(
    chain: impl IntoIterator<Item = usize>,
    frame_scopes: &[LexicalScopeIndex],
    name: &Arc<str>,
    site: verter_span::Span,
) -> Option<(usize, LexicalBinding)> {
    chain.into_iter().find_map(|frame| {
        frame_scopes[frame]
            .resolve(name, site)
            .map(|slot| (frame, slot))
    })
}

/// Walk every call expression inside one function body's statement list
/// in source order. Nested function frames (function / arrow / class
/// bodies) are NOT entered — a nested function's own call sites are
/// addressed through its own position. Discovery and the locator deref
/// share THIS walk, so a call site's `call_ordinal` means the same thing
/// on both sides BY CONSTRUCTION (no ordering drift, ever).
pub fn for_each_call_expression<'a>(
    statements: &'a [Statement<'a>],
    fire: impl FnMut(&'a CallExpression<'a>),
) {
    for_each_call_expression_root(CallExpressionWalkRoot::Statements(statements), fire);
}

/// Walk every call expression inside one function body in source order: a
/// block body's statements, or an expression body's expression (the one
/// expression statement it was).
pub fn for_each_call_expression_in_body<'a>(
    body: FunctionBodyRef<'a>,
    fire: impl FnMut(&'a CallExpression<'a>),
) {
    match body {
        FunctionBodyRef::Block(body) => for_each_call_expression(&body.statements, fire),
        FunctionBodyRef::Expression(expression) => {
            for_each_call_expression_in_expression(expression, fire);
        }
    }
}

/// Walk every call expression inside one expression in the same source
/// order and with the same nested-frame boundary as
/// [`for_each_call_expression`].
pub fn for_each_call_expression_in_expression<'a>(
    expression: &'a Expression<'a>,
    fire: impl FnMut(&'a CallExpression<'a>),
) {
    for_each_call_expression_root(CallExpressionWalkRoot::Expression(expression), fire);
}

enum CallExpressionWalkRoot<'a> {
    Statements(&'a [Statement<'a>]),
    Expression(&'a Expression<'a>),
}

fn for_each_call_expression_root<'a>(
    root: CallExpressionWalkRoot<'a>,
    mut fire: impl FnMut(&'a CallExpression<'a>),
) {
    // The syntax walks from an explicit stack in the same pre-order a
    // recursive walk takes (each node's children pushed last first, a call
    // fired when it is reached): a call nested in a callee, an argument or
    // a receiver (`a.m().m()`) costs no native level.
    enum Work<'a> {
        Statement(&'a Statement<'a>),
        Expression(&'a Expression<'a>),
        Argument(&'a oxc_ast::ast::Argument<'a>),
        ForInit(&'a oxc_ast::ast::ForStatementInit<'a>),
        Simple(&'a oxc_ast::ast::SimpleAssignmentTarget<'a>),
        Target(&'a oxc_ast::ast::AssignmentTarget<'a>),
        MaybeDefault(&'a oxc_ast::ast::AssignmentTargetMaybeDefault<'a>),
    }
    let mut work: Vec<Work<'a>> = match root {
        CallExpressionWalkRoot::Statements(statements) => {
            statements.iter().rev().map(Work::Statement).collect()
        }
        CallExpressionWalkRoot::Expression(expression) => vec![Work::Expression(expression)],
    };
    let mut c: Vec<Work<'a>> = Vec::new();
    while let Some(item) = work.pop() {
        match item {
            Work::Statement(stmt) => {
                match stmt {
                    Statement::ExpressionStatement(expr) => {
                        c.push(Work::Expression(&expr.expression))
                    }
                    Statement::BlockStatement(block) => {
                        c.extend(block.body.iter().map(Work::Statement))
                    }
                    Statement::IfStatement(if_stmt) => {
                        c.push(Work::Expression(&if_stmt.test));
                        c.push(Work::Statement(&if_stmt.consequent));
                        if let Some(alternate) = &if_stmt.alternate {
                            c.push(Work::Statement(alternate));
                        }
                    }
                    Statement::ForStatement(for_stmt) => {
                        if let Some(init) = &for_stmt.init {
                            c.push(Work::ForInit(init));
                        }
                        if let Some(test) = &for_stmt.test {
                            c.push(Work::Expression(test));
                        }
                        if let Some(update) = &for_stmt.update {
                            c.push(Work::Expression(update));
                        }
                        c.push(Work::Statement(&for_stmt.body));
                    }
                    Statement::ForInStatement(for_stmt) => {
                        c.push(Work::Expression(&for_stmt.right));
                        c.push(Work::Statement(&for_stmt.body));
                    }
                    Statement::ForOfStatement(for_stmt) => {
                        c.push(Work::Expression(&for_stmt.right));
                        c.push(Work::Statement(&for_stmt.body));
                    }
                    Statement::WhileStatement(while_stmt) => {
                        c.push(Work::Expression(&while_stmt.test));
                        c.push(Work::Statement(&while_stmt.body));
                    }
                    Statement::DoWhileStatement(do_stmt) => {
                        c.push(Work::Statement(&do_stmt.body));
                        c.push(Work::Expression(&do_stmt.test));
                    }
                    Statement::ReturnStatement(ret) => {
                        if let Some(argument) = &ret.argument {
                            c.push(Work::Expression(argument));
                        }
                    }
                    Statement::SwitchStatement(switch) => {
                        c.push(Work::Expression(&switch.discriminant));
                        for case in &switch.cases {
                            if let Some(test) = &case.test {
                                c.push(Work::Expression(test));
                            }
                            c.extend(case.consequent.iter().map(Work::Statement));
                        }
                    }
                    Statement::TryStatement(try_stmt) => {
                        c.extend(try_stmt.block.body.iter().map(Work::Statement));
                        if let Some(handler) = &try_stmt.handler {
                            c.extend(handler.body.body.iter().map(Work::Statement));
                        }
                        if let Some(finalizer) = &try_stmt.finalizer {
                            c.extend(finalizer.body.iter().map(Work::Statement));
                        }
                    }
                    Statement::LabeledStatement(labeled) => c.push(Work::Statement(&labeled.body)),
                    Statement::ThrowStatement(throw) => c.push(Work::Expression(&throw.argument)),
                    Statement::VariableDeclaration(decl) => {
                        for declarator in &decl.declarations {
                            if let Some(init) = &declarator.init {
                                c.push(Work::Expression(init));
                            }
                        }
                    }
                    Statement::ExportDeclaration(export) => {
                        if let oxc_ast::ast::Declaration::VariableDeclaration(decl) =
                            &export.declaration
                        {
                            for declarator in &decl.declarations {
                                if let Some(init) = &declarator.init {
                                    c.push(Work::Expression(init));
                                }
                            }
                        }
                    }
                    Statement::ExportDefaultDeclaration(export) => {
                        if let Some(expression) = export.declaration.as_expression() {
                            c.push(Work::Expression(expression));
                        }
                    }
                    // Nested frames (function / class bodies) and type-space
                    // declarations carry no call sites of THIS frame.
                    _ => {}
                }
            }
            Work::ForInit(init) => match init {
                oxc_ast::ast::ForStatementInit::VariableDeclaration(decl) => {
                    for declarator in &decl.declarations {
                        if let Some(init) = &declarator.init {
                            c.push(Work::Expression(init));
                        }
                    }
                }
                other => c.push(Work::Expression(other.as_expression().unwrap())),
            },
            Work::Simple(target) => match target {
                oxc_ast::ast::SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                    c.push(Work::Expression(&member.object));
                    c.push(Work::Expression(&member.expression));
                }
                oxc_ast::ast::SimpleAssignmentTarget::StaticMemberExpression(member) => {
                    c.push(Work::Expression(&member.object));
                }
                oxc_ast::ast::SimpleAssignmentTarget::PrivateFieldExpression(member) => {
                    c.push(Work::Expression(&member.object));
                }
                oxc_ast::ast::SimpleAssignmentTarget::TSAsExpression(ts) => {
                    c.push(Work::Expression(&ts.expression));
                }
                oxc_ast::ast::SimpleAssignmentTarget::TSSatisfiesExpression(ts) => {
                    c.push(Work::Expression(&ts.expression));
                }
                oxc_ast::ast::SimpleAssignmentTarget::TSNonNullExpression(ts) => {
                    c.push(Work::Expression(&ts.expression));
                }
                oxc_ast::ast::SimpleAssignmentTarget::TSTypeAssertion(ts) => {
                    c.push(Work::Expression(&ts.expression));
                }
                oxc_ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(_) => {}
            },
            Work::Target(target) => match target {
                oxc_ast::ast::AssignmentTarget::TSAsExpression(ts) => {
                    c.push(Work::Expression(&ts.expression));
                }
                oxc_ast::ast::AssignmentTarget::TSSatisfiesExpression(ts) => {
                    c.push(Work::Expression(&ts.expression));
                }
                oxc_ast::ast::AssignmentTarget::TSNonNullExpression(ts) => {
                    c.push(Work::Expression(&ts.expression));
                }
                oxc_ast::ast::AssignmentTarget::TSTypeAssertion(ts) => {
                    c.push(Work::Expression(&ts.expression));
                }
                oxc_ast::ast::AssignmentTarget::ComputedMemberExpression(member) => {
                    c.push(Work::Expression(&member.object));
                    c.push(Work::Expression(&member.expression));
                }
                oxc_ast::ast::AssignmentTarget::StaticMemberExpression(member) => {
                    c.push(Work::Expression(&member.object));
                }
                oxc_ast::ast::AssignmentTarget::PrivateFieldExpression(member) => {
                    c.push(Work::Expression(&member.object));
                }
                oxc_ast::ast::AssignmentTarget::ArrayAssignmentTarget(array) => {
                    for element in array.elements.iter().flatten() {
                        c.push(Work::MaybeDefault(element));
                    }
                    if let Some(rest) = &array.rest {
                        c.push(Work::Target(&rest.target));
                    }
                }
                oxc_ast::ast::AssignmentTarget::ObjectAssignmentTarget(object) => {
                    for property in &object.properties {
                        match property {
                                oxc_ast::ast::AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(
                                    identifier,
                                ) => {
                                    if let Some(init) = &identifier.init {
                                        c.push(Work::Expression(init));
                                    }
                                }
                                oxc_ast::ast::AssignmentTargetProperty::AssignmentTargetPropertyProperty(
                                    property,
                                ) => {
                                    c.push(Work::MaybeDefault(&property.binding));
                                }
                            }
                    }
                    if let Some(rest) = &object.rest {
                        c.push(Work::Target(&rest.target));
                    }
                }
                oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(_) => {}
            },
            Work::MaybeDefault(target) => match target {
                oxc_ast::ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(
                    with_default,
                ) => {
                    c.push(Work::Target(&with_default.binding));
                    c.push(Work::Expression(&with_default.init));
                }
                other => c.push(Work::Target(other.to_assignment_target())),
            },
            Work::Argument(argument) => match argument {
                oxc_ast::ast::Argument::SpreadElement(spread) => {
                    c.push(Work::Expression(&spread.argument))
                }
                other => c.push(Work::Expression(other.to_expression())),
            },
            Work::Expression(expr) => {
                match expr {
                    Expression::CallExpression(call) => {
                        fire(call);
                        c.push(Work::Expression(&call.callee));
                        for argument in &call.arguments {
                            c.push(Work::Argument(argument));
                        }
                    }
                    Expression::NewExpression(new_expr) => {
                        c.push(Work::Expression(&new_expr.callee));
                        for argument in &new_expr.arguments {
                            c.push(Work::Argument(argument));
                        }
                    }
                    Expression::ArrayExpression(array) => {
                        for element in &array.elements {
                            match element {
                                oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) => {
                                    c.push(Work::Expression(&spread.argument));
                                }
                                oxc_ast::ast::ArrayExpressionElement::Elision(_) => {}
                                other => c.push(Work::Expression(other.to_expression())),
                            }
                        }
                    }
                    Expression::ObjectExpression(object) => {
                        for property in &object.properties {
                            match property {
                                ObjectPropertyKind::ObjectProperty(property) => {
                                    if let Some(key) = property.key.as_expression() {
                                        c.push(Work::Expression(key));
                                    }
                                    c.push(Work::Expression(&property.value));
                                }
                                ObjectPropertyKind::SpreadProperty(spread) => {
                                    c.push(Work::Expression(&spread.argument));
                                }
                            }
                        }
                    }
                    Expression::AssignmentExpression(assignment) => {
                        c.push(Work::Target(&assignment.left));
                        c.push(Work::Expression(&assignment.right));
                    }
                    Expression::AwaitExpression(await_expr) => {
                        c.push(Work::Expression(&await_expr.argument))
                    }
                    Expression::UnaryExpression(unary) => c.push(Work::Expression(&unary.argument)),
                    Expression::UpdateExpression(update) => {
                        c.push(Work::Simple(&update.argument));
                    }
                    Expression::BinaryExpression(binary) => {
                        c.push(Work::Expression(&binary.left));
                        c.push(Work::Expression(&binary.right));
                    }
                    Expression::LogicalExpression(logical) => {
                        c.push(Work::Expression(&logical.left));
                        c.push(Work::Expression(&logical.right));
                    }
                    Expression::ConditionalExpression(conditional) => {
                        c.push(Work::Expression(&conditional.test));
                        c.push(Work::Expression(&conditional.consequent));
                        c.push(Work::Expression(&conditional.alternate));
                    }
                    Expression::ChainExpression(chain) => match &chain.expression {
                        oxc_ast::ast::ChainElement::CallExpression(call) => {
                            fire(call);
                            c.push(Work::Expression(&call.callee));
                            for argument in &call.arguments {
                                c.push(Work::Argument(argument));
                            }
                        }
                        oxc_ast::ast::ChainElement::TSNonNullExpression(ts) => {
                            c.push(Work::Expression(&ts.expression));
                        }
                        oxc_ast::ast::ChainElement::ComputedMemberExpression(member) => {
                            c.push(Work::Expression(&member.object));
                            c.push(Work::Expression(&member.expression));
                        }
                        oxc_ast::ast::ChainElement::StaticMemberExpression(member) => {
                            c.push(Work::Expression(&member.object));
                        }
                        oxc_ast::ast::ChainElement::PrivateFieldExpression(member) => {
                            c.push(Work::Expression(&member.object));
                        }
                    },
                    Expression::ParenthesizedExpression(paren) => {
                        c.push(Work::Expression(&paren.expression))
                    }
                    Expression::SequenceExpression(sequence) => {
                        for expression in &sequence.expressions {
                            c.push(Work::Expression(expression));
                        }
                    }
                    Expression::TaggedTemplateExpression(tagged) => {
                        c.push(Work::Expression(&tagged.tag));
                        for expression in &tagged.quasi.expressions {
                            c.push(Work::Expression(expression));
                        }
                    }
                    Expression::TemplateLiteral(template) => {
                        for expression in &template.expressions {
                            c.push(Work::Expression(expression));
                        }
                    }
                    Expression::YieldExpression(yield_expr) => {
                        if let Some(argument) = &yield_expr.argument {
                            c.push(Work::Expression(argument));
                        }
                    }
                    Expression::PrivateInExpression(private_in) => {
                        c.push(Work::Expression(&private_in.right));
                    }
                    Expression::ComputedMemberExpression(member) => {
                        c.push(Work::Expression(&member.object));
                        c.push(Work::Expression(&member.expression));
                    }
                    Expression::StaticMemberExpression(member) => {
                        c.push(Work::Expression(&member.object))
                    }
                    Expression::PrivateFieldExpression(member) => {
                        c.push(Work::Expression(&member.object))
                    }
                    Expression::ImportExpression(import) => {
                        c.push(Work::Expression(&import.source));
                        if let Some(options) = &import.options {
                            c.push(Work::Expression(options));
                        }
                    }
                    Expression::TSAsExpression(ts) => c.push(Work::Expression(&ts.expression)),
                    Expression::TSSatisfiesExpression(ts) => {
                        c.push(Work::Expression(&ts.expression))
                    }
                    Expression::TSTypeAssertion(ts) => c.push(Work::Expression(&ts.expression)),
                    Expression::TSNonNullExpression(ts) => c.push(Work::Expression(&ts.expression)),
                    Expression::TSInstantiationExpression(ts) => {
                        c.push(Work::Expression(&ts.expression))
                    }
                    Expression::V8IntrinsicExpression(intrinsic) => {
                        for argument in &intrinsic.arguments {
                            c.push(Work::Argument(argument));
                        }
                    }
                    // Nested frames (function / arrow / class bodies) and leaves
                    // carry no call sites of THIS frame.
                    _ => {}
                }
            }
        }
        work.extend(c.drain(..).rev());
    }
}

/// Resolve each indexed call site's exact same-file target after
/// discovery: a bare identifier callee whose name binds a served function
/// in the same index (same file, same namespace qualification) targets the
/// highest-ordinal entry for that name — the trailing implementation of
/// its overload group. Computed callees, member calls, and unresolved
/// names carry no target.
fn resolve_call_site_targets(entries: &mut [FunctionProgramEntry]) {
    let candidates: Vec<(Arc<str>, FunctionPartIdentity, u32, FunctionProgramKey)> = entries
        .iter()
        .map(|entry| {
            (
                Arc::clone(&entry.key.declaration.name),
                entry.key.part.clone(),
                entry.key.overload_ordinal,
                entry.key.clone(),
            )
        })
        .collect();
    // The hoisted nested function declarations each frame binds, keyed by
    // the enclosing frame. A declaration hoists over parameters, locals and
    // every file-level binding, so a bare-identifier call in the parent
    // frame binds HERE first.
    let nested_declarations: Vec<(FunctionProgramKey, Arc<str>, FunctionProgramKey)> = entries
        .iter()
        .filter_map(|entry| {
            let parent = entry.lexical_parent.as_deref()?.clone();
            let name = entry.nested_declaration_name.clone()?;
            Some((parent, name, entry.key.clone()))
        })
        .collect();
    for entry in entries.iter_mut() {
        let caller_ns = entry
            .key
            .declaration
            .name
            .rsplit_once('.')
            .map(|(ns, _)| ns.to_string());
        let mut sites = entry.call_sites.to_vec();
        for site in &mut sites {
            let FunctionEffectCallee::Identifier(callee) = &site.callee else {
                continue;
            };
            // Lexical preference: the namespace-qualified binding
            // (`N.callee`) shadows the file-global one, exactly like
            // scoped name resolution — never the globally-highest overload
            // ordinal across both spellings.
            let best_for = |spelling: &str| {
                candidates
                    .iter()
                    .filter(|(name, part, _, _)| {
                        name.as_ref() == spelling
                            && matches!(
                                part,
                                FunctionPartIdentity::DeclarationBody
                                    | FunctionPartIdentity::Initializer
                            )
                    })
                    .max_by_key(|(_, _, ordinal, _)| *ordinal)
                    .map(|(_, _, _, key)| key.clone())
            };
            let nested = nested_declarations.iter().find_map(|(parent, name, key)| {
                (parent == &entry.key && name.as_ref() == callee.as_ref()).then(|| key.clone())
            });
            site.target = nested.or_else(|| {
                caller_ns
                    .as_ref()
                    .and_then(|ns| best_for(&format!("{ns}.{callee}")))
                    .or_else(|| best_for(callee))
            });
        }
        entry.call_sites = Arc::from(sites.into_boxed_slice());
    }
}

fn link_callback_return_sources(
    canonical_id: &Arc<str>,
    entries: &mut [FunctionProgramEntry],
    expressions: &mut Vec<ProgramExpressionRecord>,
) {
    let callbacks: Vec<(
        u32,
        FunctionReturnSource,
        FunctionBodyLocator,
        verter_span::Span,
    )> = entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.locator.descent.last(),
                Some(
                    FunctionDescentStep::CallArgument { .. }
                        | FunctionDescentStep::NestedCallable { .. }
                )
            ) && entry.nested_declaration_name.is_none()
        })
        .map(|entry| {
            let source = FunctionReturnSource::Flow(FlowFunctionReturnIdentity {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::clone(canonical_id),
                    owner: entry.key.declaration.owner,
                    symbol: Arc::clone(&entry.key.declaration.name),
                    space: LocatorSymbolSpace::Value,
                },
                function_part: entry.key.part.clone(),
                overload_ordinal: entry.key.overload_ordinal,
            });
            (entry.span.start, source, entry.locator.clone(), entry.span)
        })
        .collect();

    let link_args = |args: &mut Arc<[FunctionCallArgRecord]>| {
        let mut linked = args.to_vec();
        for argument in &mut linked {
            if let Some((_, source, _, _)) = callbacks
                .iter()
                .find(|(point, _, _, _)| *point == argument.point && argument.is_function_value)
            {
                argument.function_return_source = Some(source.clone());
            }
        }
        *args = Arc::from(linked.into_boxed_slice());
    };

    for entry in entries.iter_mut() {
        let mut sites = entry.call_sites.to_vec();
        for site in &mut sites {
            link_args(&mut site.args);
        }
        entry.call_sites = Arc::from(sites.into_boxed_slice());
    }
    for expression in expressions.iter_mut() {
        if let ProgramExpressionSource::SemanticCall { site, .. } = &mut expression.source {
            link_args(&mut site.args);
        }
    }
    expressions.extend(
        callbacks
            .into_iter()
            .map(|(offset, source, locator, span)| ProgramExpressionRecord {
                point: ProgramExpressionIdentity {
                    canonical_id: Arc::clone(canonical_id),
                    offset,
                },
                span,
                locator,
                source: ProgramExpressionSource::FunctionReturn(source),
            }),
    );
}

/// Overload ordinals: consecutive per (name, member container) group in
/// source order, counting bodiless declarations (the trailing
/// implementation is the last ordinal).
#[derive(Default)]
struct OverloadTracker {
    function_counts: rustc_hash::FxHashMap<String, u32>,
}

impl OverloadTracker {
    fn next_function_ordinal(&mut self, name: &str) -> u32 {
        let count = self.function_counts.entry(name.to_string()).or_insert(0);
        let ordinal = *count;
        *count += 1;
        ordinal
    }
}

fn discover_statement<'ast>(
    stmt: &'ast Statement<'ast>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    match stmt {
        Statement::FunctionDeclaration(func) => {
            discover_function_declaration(
                func,
                contributor_index,
                namespace_prefix,
                overload_tracker,
                ctx,
            );
        }
        Statement::VariableDeclaration(var_decl) => {
            discover_variable_declaration(var_decl, contributor_index, namespace_prefix, ctx);
        }
        Statement::ClassDeclaration(class) => {
            discover_class(class, contributor_index, namespace_prefix, ctx);
        }
        Statement::ExportDeclaration(export) => {
            let decl = &export.declaration;
            match decl {
                oxc_ast::ast::Declaration::FunctionDeclaration(func) => {
                    discover_function_declaration(
                        func,
                        contributor_index,
                        namespace_prefix,
                        overload_tracker,
                        ctx,
                    );
                }
                oxc_ast::ast::Declaration::VariableDeclaration(var_decl) => {
                    discover_variable_declaration(
                        var_decl,
                        contributor_index,
                        namespace_prefix,
                        ctx,
                    );
                }
                oxc_ast::ast::Declaration::ClassDeclaration(class) => {
                    discover_class(class, contributor_index, namespace_prefix, ctx);
                }
                oxc_ast::ast::Declaration::TSNamespaceDeclaration(module) => {
                    discover_namespace_block(
                        module,
                        contributor_index,
                        &FunctionDescent::new(),
                        namespace_prefix,
                        overload_tracker,
                        ctx,
                    );
                }
                _ => {}
            }
        }
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                if let Some(id) = func.id.as_ref() {
                    discover_function_declaration_named(
                        func,
                        id.name.as_str(),
                        contributor_index,
                        namespace_prefix,
                        overload_tracker,
                        ctx,
                    );
                }
            }
            oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class)
                if class.id.is_some() =>
            {
                discover_class(class, contributor_index, namespace_prefix, ctx);
            }
            other => {
                let obj = match other.as_expression() {
                    Some(Expression::ObjectExpression(obj)) => obj,
                    _ => return,
                };
                // `export default { … }` object methods are served member
                // positions of the `default` declaration (the merged-symbol
                // name the value side registers).
                for (member_ordinal, prop) in obj.properties.iter().enumerate() {
                    let ObjectPropertyKind::ObjectProperty(p) = prop else {
                        continue;
                    };
                    if !p.method && matches!(p.kind, oxc_ast::ast::PropertyKind::Init) {
                        continue;
                    }
                    if static_property_key_name(&p.key).is_none() {
                        continue;
                    }
                    let member_path: Arc<[u32]> = Arc::from(
                        vec![u32::try_from(member_ordinal).unwrap_or(u32::MAX)].into_boxed_slice(),
                    );
                    let descent = FunctionDescent::new().then(
                        FunctionDescentStep::ExportDefaultObjectMember {
                            member_ordinal: u32::try_from(member_ordinal).unwrap_or(u32::MAX),
                        },
                    );
                    match &p.value {
                        Expression::FunctionExpression(func) => {
                            discover_function_inner(
                                func,
                                "default",
                                FunctionPartIdentity::Member { member_path },
                                contributor_index,
                                descent,
                                0,
                                ctx,
                            );
                        }
                        Expression::ArrowFunctionExpression(arrow) => {
                            discover_arrow_inner(
                                arrow,
                                "default",
                                FunctionPartIdentity::Member { member_path },
                                contributor_index,
                                descent,
                                ctx,
                            );
                        }
                        _ => {}
                    }
                }
            }
        },
        Statement::TSNamespaceDeclaration(module) => {
            discover_namespace_block(
                module,
                contributor_index,
                &FunctionDescent::new(),
                namespace_prefix,
                overload_tracker,
                ctx,
            );
        }
        _ => {}
    }
}

/// Discover the served positions of one namespace declaration — written
/// `namespace N { … }` or `export namespace N { … }` — at the statement
/// `descent` reaches: its members are qualified `N.name` under
/// `namespace_prefix`, and every locator extends `descent` with one
/// [`FunctionDescentStep::NamespaceMember`] step. `declare module
/// "specifier" { .. }` is an ambient augmentation, not a file-scope function
/// owner — never indexed here.
fn discover_namespace_block<'ast>(
    module: &'ast oxc_ast::ast::TSNamespaceDeclaration<'ast>,
    contributor_index: usize,
    descent: &FunctionDescent,
    namespace_prefix: Option<&str>,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    let id = &module.id;
    let prefix = match namespace_prefix {
        Some(prefix) => format!("{prefix}.{}", id.name),
        None => id.name.to_string(),
    };
    if let oxc_ast::ast::TSNamespaceDeclarationBody::TSModuleBlock(block) = &module.body {
        for (statement_ordinal, inner) in block.body.iter().enumerate() {
            let inner_descent = descent.then(namespace_member_step(statement_ordinal));
            discover_namespaced_statement(
                inner,
                contributor_index,
                &inner_descent,
                &prefix,
                overload_tracker,
                ctx,
            );
        }
    }
}

/// The descent step selecting statement `statement_ordinal` of a
/// namespace block.
fn namespace_member_step(statement_ordinal: usize) -> FunctionDescentStep {
    FunctionDescentStep::NamespaceMember {
        statement_ordinal: u32::try_from(statement_ordinal).unwrap_or(u32::MAX),
    }
}

/// Discover the served positions of one statement inside a namespace
/// block. `descent` is the FULL namespace descent from the contributing
/// top-level statement to `stmt` — one [`FunctionDescentStep::NamespaceMember`]
/// per enclosing block, the innermost last — and every locator minted
/// below extends it, so a nested namespace's member resolves through the
/// same blocks it was discovered in.
fn discover_namespaced_statement<'ast>(
    stmt: &'ast Statement<'ast>,
    contributor_index: usize,
    descent: &FunctionDescent,
    namespace: &str,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    match stmt {
        Statement::ExportDeclaration(export) => {
            let decl = &export.declaration;
            match decl {
                oxc_ast::ast::Declaration::FunctionDeclaration(func) => {
                    discover_namespaced_function(
                        func,
                        contributor_index,
                        descent,
                        namespace,
                        overload_tracker,
                        ctx,
                    );
                }
                oxc_ast::ast::Declaration::VariableDeclaration(var_decl) => {
                    discover_variable_declaration_ns(
                        var_decl,
                        contributor_index,
                        descent,
                        namespace,
                        ctx,
                    );
                }
                oxc_ast::ast::Declaration::ClassDeclaration(class) => {
                    discover_class_ns(class, contributor_index, descent, namespace, ctx);
                }
                oxc_ast::ast::Declaration::TSNamespaceDeclaration(module) => {
                    discover_namespace_block(
                        module,
                        contributor_index,
                        descent,
                        Some(namespace),
                        overload_tracker,
                        ctx,
                    );
                }
                _ => {}
            }
        }
        Statement::FunctionDeclaration(func) => {
            discover_namespaced_function(
                func,
                contributor_index,
                descent,
                namespace,
                overload_tracker,
                ctx,
            );
        }
        Statement::VariableDeclaration(var_decl) => {
            discover_variable_declaration_ns(var_decl, contributor_index, descent, namespace, ctx);
        }
        Statement::ClassDeclaration(class) => {
            discover_class_ns(class, contributor_index, descent, namespace, ctx);
        }
        Statement::TSNamespaceDeclaration(module) => {
            discover_namespace_block(
                module,
                contributor_index,
                descent,
                Some(namespace),
                overload_tracker,
                ctx,
            );
        }
        _ => {}
    }
}

fn discover_namespaced_function<'ast>(
    func: &'ast Function<'ast>,
    contributor_index: usize,
    descent: &FunctionDescent,
    namespace: &str,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    if let Some(id) = func.id.as_ref() {
        let qualified = format!("{namespace}.{}", id.name);
        let overload_ordinal = overload_tracker.next_function_ordinal(&qualified);
        let full_descent = descent.then(FunctionDescentStep::FunctionDeclaration);
        discover_function_inner(
            func,
            &qualified,
            FunctionPartIdentity::DeclarationBody,
            contributor_index,
            full_descent,
            overload_ordinal,
            ctx,
        );
    }
}

fn discover_function_declaration<'ast>(
    func: &'ast Function<'ast>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    let Some(id) = func.id.as_ref() else {
        return;
    };
    discover_function_declaration_named(
        func,
        id.name.as_str(),
        contributor_index,
        namespace_prefix,
        overload_tracker,
        ctx,
    );
}

fn discover_function_declaration_named<'ast>(
    func: &'ast Function<'ast>,
    name: &str,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    if func.body.is_none() {
        // A bodiless overload declaration consumes its group ordinal but has
        // no body to serve.
        overload_tracker.next_function_ordinal(name);
        return;
    }
    let name = match namespace_prefix {
        Some(prefix) => format!("{prefix}.{name}"),
        None => name.to_string(),
    };
    let overload_ordinal = overload_tracker.next_function_ordinal(&name);
    discover_function_inner(
        func,
        &name,
        FunctionPartIdentity::DeclarationBody,
        contributor_index,
        FunctionDescent::new().then(FunctionDescentStep::FunctionDeclaration),
        overload_ordinal,
        ctx,
    );
}

fn discover_variable_declaration<'ast>(
    var_decl: &'ast VariableDeclaration<'ast>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    for (declarator_ordinal, declarator) in var_decl.declarations.iter().enumerate() {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            continue;
        };
        let name = match namespace_prefix {
            Some(prefix) => format!("{prefix}.{}", id.name),
            None => id.name.to_string(),
        };
        let Some(init) = declarator.init.as_ref() else {
            continue;
        };
        let declarator_ordinal = u32::try_from(declarator_ordinal).unwrap_or(u32::MAX);
        let base_descent = FunctionDescent::new()
            .then(FunctionDescentStep::VariableInitializer { declarator_ordinal });
        if let Some(anchor) = ctx.anchor(contributor_index) {
            ctx.expressions.push(ProgramExpressionRecord {
                point: ProgramExpressionIdentity {
                    canonical_id: Arc::clone(&ctx.canonical_id),
                    offset: init.span().start,
                },
                span: verter_span::Span::new(init.span().start, init.span().end),
                locator: FunctionBodyLocator {
                    contributor: anchor,
                    descent: base_descent.clone(),
                },
                source: program_expression_source(&ctx.walks, init),
            });
            discover_top_level_call_arg_positions(init, &name, anchor, &base_descent, ctx);
        }
        let descent = |extra: FunctionDescentStep| base_descent.then(extra);
        match init {
            Expression::ArrowFunctionExpression(arrow) => {
                discover_arrow_inner(
                    arrow,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    base_descent.clone(),
                    ctx,
                );
            }
            Expression::FunctionExpression(func) => {
                discover_function_inner(
                    func,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    base_descent,
                    0,
                    ctx,
                );
            }
            Expression::ObjectExpression(obj) => {
                for (member_ordinal, prop) in obj.properties.iter().enumerate() {
                    let ObjectPropertyKind::ObjectProperty(p) = prop else {
                        continue;
                    };
                    if !p.method && matches!(p.kind, oxc_ast::ast::PropertyKind::Init) {
                        continue;
                    }
                    if static_property_key_name(&p.key).is_none() {
                        continue;
                    }
                    let member_path: Arc<[u32]> = Arc::from(
                        vec![u32::try_from(member_ordinal).unwrap_or(u32::MAX)].into_boxed_slice(),
                    );
                    match &p.value {
                        Expression::FunctionExpression(func) => {
                            let previous_this =
                                ctx.enclosing_this.replace(EnclosingThis::ObjectLiteral);
                            discover_function_inner(
                                func,
                                &name,
                                FunctionPartIdentity::Member { member_path },
                                contributor_index,
                                descent(FunctionDescentStep::ObjectMember {
                                    member_ordinal: u32::try_from(member_ordinal)
                                        .unwrap_or(u32::MAX),
                                }),
                                0,
                                ctx,
                            );
                            ctx.enclosing_this = previous_this;
                        }
                        Expression::ArrowFunctionExpression(arrow) => {
                            discover_arrow_inner(
                                arrow,
                                &name,
                                FunctionPartIdentity::Member { member_path },
                                contributor_index,
                                descent(FunctionDescentStep::ObjectMember {
                                    member_ordinal: u32::try_from(member_ordinal)
                                        .unwrap_or(u32::MAX),
                                }),
                                ctx,
                            );
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn unwrap_program_expression<'a>(mut expression: &'a Expression<'a>) -> &'a Expression<'a> {
    loop {
        expression = match expression {
            Expression::ParenthesizedExpression(parenthesized) => &parenthesized.expression,
            Expression::TSAsExpression(assertion) => &assertion.expression,
            Expression::TSSatisfiesExpression(satisfies) => &satisfies.expression,
            Expression::TSNonNullExpression(non_null) => &non_null.expression,
            _ => return expression,
        };
    }
}

fn call_arg_record(argument: &oxc_ast::ast::Argument<'_>) -> FunctionCallArgRecord {
    let expression = argument.as_expression();
    FunctionCallArgRecord {
        point: expression
            .map(|expression| expression.span().start)
            .unwrap_or_else(|| argument.span().start),
        spread: matches!(argument, oxc_ast::ast::Argument::SpreadElement(_)),
        literal_mode: match expression.map(unwrap_program_expression) {
            Some(
                Expression::StringLiteral(_)
                | Expression::NumericLiteral(_)
                | Expression::BooleanLiteral(_)
                | Expression::BigIntLiteral(_)
                | Expression::TemplateLiteral(_)
                | Expression::ObjectExpression(_)
                | Expression::ArrayExpression(_),
            ) => FunctionCallArgLiteralMode::Literal,
            _ => FunctionCallArgLiteralMode::Widened,
        },
        is_function_value: matches!(
            expression.map(unwrap_program_expression),
            Some(Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_))
        ),
        function_return_source: None,
    }
}

fn effect_callee(callee: &Expression<'_>) -> FunctionEffectCallee {
    match callee {
        Expression::Identifier(id) => FunctionEffectCallee::Identifier(Arc::from(id.name.as_str())),
        Expression::StaticMemberExpression(member) => {
            let mut path = Vec::new();
            if collect_static_member_path(member, &mut path) {
                FunctionEffectCallee::StaticMember(Arc::from(path.into_boxed_slice()))
            } else {
                FunctionEffectCallee::Other
            }
        }
        _ => FunctionEffectCallee::Other,
    }
}

fn call_site_record(call: &CallExpression<'_>) -> FunctionCallSiteRecord {
    FunctionCallSiteRecord {
        span: verter_span::Span::new(call.span.start, call.span.end),
        callee: effect_callee(&call.callee),
        target: None,
        args: Arc::from(
            call.arguments
                .iter()
                .map(call_arg_record)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
    }
}

fn new_site_record(call: &oxc_ast::ast::NewExpression<'_>) -> FunctionCallSiteRecord {
    FunctionCallSiteRecord {
        span: verter_span::Span::new(call.span.start, call.span.end),
        callee: effect_callee(&call.callee),
        target: None,
        args: Arc::from(
            call.arguments
                .iter()
                .map(call_arg_record)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
    }
}

fn program_expression_source(
    walks: &verter_parser::oxc_parse::ProgramWalkStack<'_>,
    expression: &Expression<'_>,
) -> ProgramExpressionSource {
    match unwrap_program_expression(expression) {
        Expression::CallExpression(call) => ProgramExpressionSource::SemanticCall {
            kind: ProgramExpressionCallKind::Call,
            site: call_site_record(call),
        },
        Expression::NewExpression(call) => ProgramExpressionSource::SemanticCall {
            kind: ProgramExpressionCallKind::Construct,
            site: new_site_record(call),
        },
        expression if expression_has_call(walks, expression) => {
            ProgramExpressionSource::UnsupportedCall
        }
        _ => ProgramExpressionSource::Value,
    }
}

fn expression_has_call(
    walks: &verter_parser::oxc_parse::ProgramWalkStack<'_>,
    expression: &Expression<'_>,
) -> bool {
    #[derive(Default)]
    struct Probe(bool);
    impl<'a> Visit<'a> for Probe {
        fn visit_call_expression(&mut self, _call: &CallExpression<'a>) {
            self.0 = true;
        }

        fn visit_new_expression(&mut self, _call: &oxc_ast::ast::NewExpression<'a>) {
            self.0 = true;
        }

        fn visit_function(&mut self, _it: &Function<'a>, _flags: oxc_syntax::scope::ScopeFlags) {}

        fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {}
    }
    let mut probe = Probe::default();
    walks.with_node_stack(expression.span(), || probe.visit_expression(expression));
    probe.0
}

fn discover_top_level_call_arg_positions<'ast>(
    expression: &'ast Expression<'ast>,
    declaration_name: &str,
    contributor: DeclContributorAnchor,
    base_descent: &FunctionDescent,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    let mut call_ordinal = 0usize;
    for_each_call_expression_in_expression(expression, |call| {
        let current_call_ordinal = call_ordinal;
        call_ordinal += 1;
        for (arg_ordinal, argument) in call.arguments.iter().enumerate() {
            let Some(expression) = argument.as_expression() else {
                continue;
            };
            let node = match unwrap_program_expression(expression) {
                Expression::ArrowFunctionExpression(arrow) => FunctionNode::Arrow(arrow),
                Expression::FunctionExpression(function) => FunctionNode::Function(function),
                _ => continue,
            };
            let ordinal = ctx.next_nested_ordinal;
            ctx.next_nested_ordinal += 1;
            let descent = base_descent.then(FunctionDescentStep::CallArgument {
                call_ordinal: u32::try_from(current_call_ordinal).unwrap_or(u32::MAX),
                arg_ordinal: u32::try_from(arg_ordinal).unwrap_or(u32::MAX),
            });
            discover_top_level_callable(
                node,
                verter_span::Span::new(expression.span().start, expression.span().end),
                declaration_name,
                contributor,
                descent,
                ordinal,
                ctx,
            );
        }
    });
}

fn discover_top_level_callable<'ast>(
    node: FunctionNode<'ast>,
    span: verter_span::Span,
    declaration_name: &str,
    contributor: DeclContributorAnchor,
    descent: FunctionDescent,
    ordinal: u32,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    let (params, body) = match &node {
        FunctionNode::Function(function) => {
            let Some(body) = function.body.as_ref() else {
                return;
            };
            (
                formal_params(&function.params),
                FunctionBodyRef::Block(body),
            )
        }
        FunctionNode::Arrow(arrow) => (
            formal_params(&arrow.params),
            FunctionBodyRef::of_arrow(arrow),
        ),
        FunctionNode::Initializer(_) => return,
    };
    let key = FunctionProgramKey {
        declaration: FunctionDeclarationRef {
            owner: contributor.owner,
            name: Arc::from(declaration_name),
            space: SymbolSpace::Value,
        },
        part: FunctionPartIdentity::Other { ordinal },
        overload_ordinal: 0,
    };
    let locator = FunctionBodyLocator {
        contributor,
        descent,
    };
    let entry = ctx.build_entry(key.clone(), locator.clone(), params, body, span.start, node);
    ctx.push(entry, node);
    discover_nested_positions(body, &key, &locator, ctx);
}

fn discover_variable_declaration_ns<'ast>(
    var_decl: &'ast VariableDeclaration<'ast>,
    contributor_index: usize,
    descent: &FunctionDescent,
    namespace: &str,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    for (declarator_ordinal, declarator) in var_decl.declarations.iter().enumerate() {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            continue;
        };
        let name = format!("{namespace}.{}", id.name);
        let Some(init) = declarator.init.as_ref() else {
            continue;
        };
        let declarator_ordinal = u32::try_from(declarator_ordinal).unwrap_or(u32::MAX);
        let base = descent.then(FunctionDescentStep::VariableInitializer { declarator_ordinal });
        if let Some(anchor) = ctx.anchor(contributor_index) {
            ctx.expressions.push(ProgramExpressionRecord {
                point: ProgramExpressionIdentity {
                    canonical_id: Arc::clone(&ctx.canonical_id),
                    offset: init.span().start,
                },
                span: verter_span::Span::new(init.span().start, init.span().end),
                locator: FunctionBodyLocator {
                    contributor: anchor,
                    descent: base.clone(),
                },
                source: program_expression_source(&ctx.walks, init),
            });
            discover_top_level_call_arg_positions(init, &name, anchor, &base, ctx);
        }
        match init {
            Expression::ArrowFunctionExpression(arrow) => {
                discover_arrow_inner(
                    arrow,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    base.clone(),
                    ctx,
                );
            }
            Expression::FunctionExpression(func) => {
                discover_function_inner(
                    func,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    base.clone(),
                    0,
                    ctx,
                );
            }
            _ => {}
        }
    }
}

fn discover_class<'ast>(
    class: &'ast Class<'ast>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    let Some(id) = class.id.as_ref() else {
        return;
    };
    let name = match namespace_prefix {
        Some(prefix) => format!("{prefix}.{}", id.name),
        None => id.name.to_string(),
    };
    discover_class_heritage_expression(class, contributor_index, &FunctionDescent::new(), ctx);
    discover_class_members(class, &name, contributor_index, FunctionDescent::new(), ctx);
}

/// Index a class declaration's `extends` EXPRESSION — one the declaration
/// facts cannot name (`extends Mixin(Base)`, not `extends Base` or
/// `extends NS.Base`) — as a program expression, so its value (the base
/// constructor type) reads through the same indexed-expression rail a
/// declarator initializer does.
fn discover_class_heritage_expression<'ast>(
    class: &'ast Class<'ast>,
    contributor_index: usize,
    descent: &FunctionDescent,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    let Some(heritage) = class.heritage.as_ref().map(|heritage| &heritage.expression) else {
        return;
    };
    if crate::analysis::type_eval_build::heritage_expression_name(heritage).is_some() {
        return;
    }
    let Some(anchor) = ctx.anchor(contributor_index) else {
        return;
    };
    let descent = descent.then(FunctionDescentStep::ClassHeritage);
    ctx.expressions.push(ProgramExpressionRecord {
        point: ProgramExpressionIdentity {
            canonical_id: Arc::clone(&ctx.canonical_id),
            offset: heritage.span().start,
        },
        span: verter_span::Span::new(heritage.span().start, heritage.span().end),
        locator: FunctionBodyLocator {
            contributor: anchor,
            descent,
        },
        source: program_expression_source(&ctx.walks, heritage),
    });
}

fn discover_class_ns<'ast>(
    class: &'ast Class<'ast>,
    contributor_index: usize,
    descent: &FunctionDescent,
    namespace: &str,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    let Some(id) = class.id.as_ref() else {
        return;
    };
    let name = format!("{namespace}.{}", id.name);
    discover_class_heritage_expression(class, contributor_index, descent, ctx);
    discover_class_members(class, &name, contributor_index, descent.clone(), ctx);
}

fn discover_class_members<'ast>(
    class: &'ast Class<'ast>,
    name: &str,
    contributor_index: usize,
    base_descent: FunctionDescent,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    let previous_type_parameters = ctx.enclosing_type_parameters;
    ctx.enclosing_type_parameters = class.type_parameters.as_deref();
    let previous_heritage = ctx.enclosing_heritage;
    let previous_this = ctx.enclosing_this;
    let mut member_overloads: rustc_hash::FxHashMap<(String, bool), u32> =
        rustc_hash::FxHashMap::default();
    for (member_ordinal, element) in class.body.body.iter().enumerate() {
        let member_ordinal = u32::try_from(member_ordinal).unwrap_or(u32::MAX);
        match element {
            oxc_ast::ast::ClassElement::MethodDefinition(method) => {
                if matches!(method.kind, MethodDefinitionKind::Constructor) {
                    continue;
                }
                let Some(member_name) = static_property_key_name(&method.key) else {
                    continue;
                };
                let overload = {
                    let count = member_overloads
                        .entry((member_name.clone(), method.r#static))
                        .or_insert(0);
                    let ordinal = *count;
                    *count += 1;
                    ordinal
                };
                if method.value.body.is_none() {
                    continue;
                }
                // A `super.x` in this member reads the base's INSTANCE side
                // (prototype) for an instance member, its STATIC side for a
                // static one — the member's own `static` flag decides which.
                ctx.enclosing_heritage =
                    class.heritage.as_ref().map(|heritage| EnclosingHeritage {
                        super_class: &heritage.expression,
                        super_type_arguments: heritage.type_arguments.as_deref(),
                        static_side: method.r#static,
                    });
                ctx.enclosing_this = Some(EnclosingThis::of_member(method.r#static));
                let member_path: Arc<[u32]> = Arc::from(vec![member_ordinal].into_boxed_slice());
                let descent =
                    base_descent.then(FunctionDescentStep::ClassMember { member_ordinal });
                discover_function_inner(
                    &method.value,
                    name,
                    FunctionPartIdentity::Member { member_path },
                    contributor_index,
                    descent,
                    overload,
                    ctx,
                );
                ctx.enclosing_heritage = previous_heritage;
                ctx.enclosing_this = previous_this;
            }
            oxc_ast::ast::ClassElement::PropertyDefinition(prop) => {
                let Some(_member_name) = static_property_key_name(&prop.key) else {
                    continue;
                };
                let member_path: Arc<[u32]> = Arc::from(vec![member_ordinal].into_boxed_slice());
                let descent =
                    base_descent.then(FunctionDescentStep::ClassMember { member_ordinal });
                ctx.enclosing_heritage =
                    class.heritage.as_ref().map(|heritage| EnclosingHeritage {
                        super_class: &heritage.expression,
                        super_type_arguments: heritage.type_arguments.as_deref(),
                        static_side: prop.r#static,
                    });
                ctx.enclosing_this = Some(EnclosingThis::of_member(prop.r#static));
                // A field read through a synthetic value is an indexed
                // program expression that value reads. One classified as an
                // initializer (it reads `this`, or holds a callback that
                // may) is also a served position of its own, whose frame
                // reads the receiver: the synthetic value is its
                // body-derived return.
                if let (Some(kind), Some(value), Some(anchor)) = (
                    crate::analysis::class_field_value::class_field_value(
                        ctx.class_fields,
                        class,
                        prop,
                        ctx.source,
                    ),
                    prop.value.as_ref(),
                    ctx.anchor(contributor_index),
                ) {
                    let source = if kind
                        == crate::analysis::class_field_value::ClassFieldValueSource::Initializer
                    {
                        discover_initializer_inner(
                            value,
                            name,
                            FunctionPartIdentity::Member {
                                member_path: Arc::clone(&member_path),
                            },
                            contributor_index,
                            descent.clone(),
                            ctx,
                        );
                        ProgramExpressionSource::FieldInitializer {
                            source: FunctionReturnSource::Flow(FlowFunctionReturnIdentity {
                                anchor: AuthoredAnchor {
                                    canonical_id: Arc::clone(&ctx.canonical_id),
                                    owner: anchor.owner,
                                    symbol: Arc::from(name),
                                    space: LocatorSymbolSpace::Value,
                                },
                                function_part: FunctionPartIdentity::Member {
                                    member_path: Arc::clone(&member_path),
                                },
                                overload_ordinal: 0,
                            }),
                            readonly: prop.readonly,
                        }
                    } else {
                        program_expression_source(&ctx.walks, value)
                    };
                    ctx.expressions.push(ProgramExpressionRecord {
                        point: ProgramExpressionIdentity {
                            canonical_id: Arc::clone(&ctx.canonical_id),
                            offset: value.span().start,
                        },
                        span: verter_span::Span::new(value.span().start, value.span().end),
                        locator: FunctionBodyLocator {
                            contributor: anchor,
                            descent: descent.clone(),
                        },
                        source,
                    });
                }
                match prop.value.as_ref() {
                    Some(Expression::ArrowFunctionExpression(arrow)) => {
                        discover_arrow_inner(
                            arrow,
                            name,
                            FunctionPartIdentity::Member { member_path },
                            contributor_index,
                            descent,
                            ctx,
                        );
                    }
                    Some(Expression::FunctionExpression(func)) => {
                        discover_function_inner(
                            func,
                            name,
                            FunctionPartIdentity::Member { member_path },
                            contributor_index,
                            descent,
                            0,
                            ctx,
                        );
                    }
                    _ => {}
                }
                ctx.enclosing_heritage = previous_heritage;
                ctx.enclosing_this = previous_this;
            }
            _ => {}
        }
    }
    ctx.enclosing_type_parameters = previous_type_parameters;
    ctx.enclosing_heritage = previous_heritage;
    ctx.enclosing_this = previous_this;
}

pub(crate) fn static_property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    crate::analysis::flow::static_property_key_text(key).map(str::to_owned)
}

fn discover_function_inner<'ast>(
    func: &'ast Function<'ast>,
    name: &str,
    part: FunctionPartIdentity,
    contributor_index: usize,
    descent: FunctionDescent,
    overload_ordinal: u32,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    let Some(anchor) = ctx.anchor(contributor_index) else {
        return;
    };
    let Some(body) = func.body.as_ref() else {
        return;
    };
    let params = formal_params(&func.params);
    let key = FunctionProgramKey {
        declaration: FunctionDeclarationRef {
            owner: anchor.owner,
            name: Arc::from(name),
            space: SymbolSpace::Value,
        },
        part,
        overload_ordinal,
    };
    let locator = FunctionBodyLocator {
        contributor: anchor,
        descent,
    };
    let entry = ctx.build_entry(
        key.clone(),
        locator.clone(),
        params,
        FunctionBodyRef::Block(body),
        func.span.start,
        FunctionNode::Function(func),
    );
    ctx.push(entry, FunctionNode::Function(func));
    discover_nested_positions(FunctionBodyRef::Block(body), &key, &locator, ctx);
}

fn discover_arrow_inner<'ast>(
    arrow: &'ast ArrowFunctionExpression<'ast>,
    name: &str,
    part: FunctionPartIdentity,
    contributor_index: usize,
    descent: FunctionDescent,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    let Some(anchor) = ctx.anchor(contributor_index) else {
        return;
    };
    let params = formal_params(&arrow.params);
    let key = FunctionProgramKey {
        declaration: FunctionDeclarationRef {
            owner: anchor.owner,
            name: Arc::from(name),
            space: SymbolSpace::Value,
        },
        part,
        overload_ordinal: 0,
    };
    let locator = FunctionBodyLocator {
        contributor: anchor,
        descent,
    };
    let entry = ctx.build_entry(
        key.clone(),
        locator.clone(),
        params,
        FunctionBodyRef::of_arrow(arrow),
        arrow.span.start,
        FunctionNode::Arrow(arrow),
    );
    ctx.push(entry, FunctionNode::Arrow(arrow));
    discover_nested_positions(FunctionBodyRef::of_arrow(arrow), &key, &locator, ctx);
}

/// A class field's initializer that reads `this`, or holds a callback that
/// may, discovered as a served position of its own
/// ([`FunctionNode::Initializer`]) under the member's part identity: no
/// parameters, the one expression its body.
fn discover_initializer_inner<'ast>(
    initializer: &'ast Expression<'ast>,
    name: &str,
    part: FunctionPartIdentity,
    contributor_index: usize,
    descent: FunctionDescent,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    let Some(anchor) = ctx.anchor(contributor_index) else {
        return;
    };
    let key = FunctionProgramKey {
        declaration: FunctionDeclarationRef {
            owner: anchor.owner,
            name: Arc::from(name),
            space: SymbolSpace::Value,
        },
        part,
        overload_ordinal: 0,
    };
    let locator = FunctionBodyLocator {
        contributor: anchor,
        descent,
    };
    let node = FunctionNode::Initializer(initializer);
    let body = FunctionBodyRef::Expression(initializer);
    let entry = ctx.build_entry(
        key.clone(),
        locator.clone(),
        Arc::from(Vec::new().into_boxed_slice()),
        body,
        initializer.span().start,
        node,
    );
    ctx.push(entry, node);
    discover_nested_positions(body, &key, &locator, ctx);
}

#[cfg(any(test, feature = "test-support"))]
std::thread_local! { static NESTED_CALLABLE_WALK_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// Actual nested-callable statement visits on the current retained worker.
#[cfg(any(test, feature = "test-support"))]
pub fn take_nested_callable_walk_visits_for_tests() -> usize {
    NESTED_CALLABLE_WALK_VISITS.with(|visits| visits.replace(0))
}

/// Visit each directly nested callable once, without entering its frame,
/// with whether it is a class expression's method or accessor.
fn for_each_nested_callable<'a>(
    walks: &verter_parser::oxc_parse::ProgramWalkStack<'_>,
    body: FunctionBodyRef<'a>,
    mut visit: impl FnMut(FunctionNode<'a>, bool),
) {
    struct CallableVisitor<'f, F>(&'f mut F);
    impl<'a, F: FnMut(FunctionNode<'a>, bool)> Visit<'a> for CallableVisitor<'_, F> {
        fn visit_statement(&mut self, statement: &Statement<'a>) {
            #[cfg(any(test, feature = "test-support"))]
            NESTED_CALLABLE_WALK_VISITS.with(|visits| visits.set(visits.get() + 1));
            walk::walk_statement(self, statement);
        }
        fn visit_function(
            &mut self,
            function: &Function<'a>,
            _flags: oxc_syntax::scope::ScopeFlags,
        ) {
            let function = self.alloc(function);
            (self.0)(FunctionNode::Function(function), false);
        }
        fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
            let arrow = self.alloc(arrow);
            (self.0)(FunctionNode::Arrow(arrow), false);
        }
        fn visit_method_definition(&mut self, method: &oxc_ast::ast::MethodDefinition<'a>) {
            self.visit_decorators(&method.decorators);
            self.visit_property_key(&method.key);
            let function = self.alloc(&*method.value);
            (self.0)(FunctionNode::Function(function), true);
        }
        // A class EXPRESSION is a value of this frame: its methods,
        // accessors and the callables its initializers hold are served
        // under this frame, so the flow lane infers a body-derived member
        // type exactly as it infers an object-literal method's. A local
        // class DECLARATION is not a value any position of this frame
        // lowers.
        fn visit_class(&mut self, class: &Class<'a>) {
            if class.r#type == oxc_ast::ast::ClassType::ClassExpression {
                walk::walk_class(self, class);
            }
        }
        fn visit_ts_type(&mut self, _ty: &oxc_ast::ast::TSType<'a>) {}
    }
    let mut visitor = CallableVisitor(&mut visit);
    for statement in body.statements() {
        walks.with_node_stack(statement.span(), || visitor.visit_statement(statement));
    }
    // An expression body walks as the one expression statement it was.
    if let Some(expression) = body.expression() {
        #[cfg(any(test, feature = "test-support"))]
        NESTED_CALLABLE_WALK_VISITS.with(|visits| visits.set(visits.get() + 1));
        walks.with_node_stack(expression.span(), || visitor.visit_expression(expression));
    }
}

/// Index every directly nested callable under its exact lexical parent.
fn discover_nested_positions<'ast>(
    body: FunctionBodyRef<'ast>,
    parent_key: &FunctionProgramKey,
    parent_locator: &FunctionBodyLocator,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) {
    // Nested callables are discovered depth first, each one's own nested
    // callables before its next sibling, from an explicit stack of the
    // bodies being walked: a callable nested in a callable's body costs no
    // native level. No enclosing type parameters, heritage or `this` reach
    // a nested callable's entry, however deep.
    struct Body<'ast> {
        callables: std::vec::IntoIter<(FunctionNode<'ast>, bool)>,
        key: FunctionProgramKey,
        locator: FunctionBodyLocator,
        local_ordinal: u32,
    }
    fn nested_body<'ast>(
        walks: &verter_parser::oxc_parse::ProgramWalkStack<'_>,
        body: FunctionBodyRef<'ast>,
        key: FunctionProgramKey,
        locator: FunctionBodyLocator,
    ) -> Body<'ast> {
        let mut callables = Vec::new();
        for_each_nested_callable(walks, body, |node, class_member| {
            callables.push((node, class_member));
        });
        Body {
            callables: callables.into_iter(),
            key,
            locator,
            local_ordinal: 0,
        }
    }
    let previous_type_parameters = ctx.enclosing_type_parameters.take();
    let previous_heritage = ctx.enclosing_heritage.take();
    let previous_this = ctx.enclosing_this.take();
    let mut bodies = vec![nested_body(
        &ctx.walks,
        body,
        parent_key.clone(),
        parent_locator.clone(),
    )];
    while let Some(parent) = bodies.last_mut() {
        let Some((node, class_member)) = parent.callables.next() else {
            bodies.pop();
            continue;
        };
        let ordinal = ctx.next_nested_ordinal;
        ctx.next_nested_ordinal += 1;
        let descent = parent
            .locator
            .descent
            .then(FunctionDescentStep::NestedCallable {
                ordinal: parent.local_ordinal,
            });
        parent.local_ordinal += 1;
        let discovered = discover_nested_callable(
            node,
            &parent.key,
            &parent.locator,
            descent,
            ordinal,
            class_member,
            ctx,
        );
        if let Some((key, locator, body)) = discovered {
            bodies.push(nested_body(&ctx.walks, body, key, locator));
        }
    }
    ctx.enclosing_type_parameters = previous_type_parameters;
    ctx.enclosing_heritage = previous_heritage;
    ctx.enclosing_this = previous_this;
}
/// One function / arrow expression in call-argument position, discovered
/// under its lexical parent's key: its key, locator and body, whose own
/// nested callables the caller discovers next.
fn discover_nested_callable<'ast>(
    node: FunctionNode<'ast>,
    parent_key: &FunctionProgramKey,
    parent_locator: &FunctionBodyLocator,
    descent: FunctionDescent,
    ordinal: u32,
    class_member: bool,
    ctx: &mut DiscoveryCtx<'_, 'ast>,
) -> Option<(
    FunctionProgramKey,
    FunctionBodyLocator,
    FunctionBodyRef<'ast>,
)> {
    let span: verter_span::Span = verter_span::Span::new(node.span().start, node.span().end);
    let (params, body) = match &node {
        FunctionNode::Function(func) => {
            let body = func.body.as_ref()?;
            (formal_params(&func.params), FunctionBodyRef::Block(body))
        }
        FunctionNode::Arrow(arrow) => (
            formal_params(&arrow.params),
            FunctionBodyRef::of_arrow(arrow),
        ),
        FunctionNode::Initializer(_) => return None,
    };
    let key = FunctionProgramKey {
        declaration: parent_key.declaration.clone(),
        part: FunctionPartIdentity::Other { ordinal },
        overload_ordinal: 0,
    };
    let locator = FunctionBodyLocator {
        contributor: parent_locator.contributor,
        descent,
    };
    let mut entry = ctx.build_entry(key.clone(), locator.clone(), params, body, span.start, node);
    entry.lexical_parent = Some(Box::new(parent_key.clone()));
    entry.class_member = class_member;
    if let FunctionNode::Function(function) = node {
        if function.r#type == oxc_ast::ast::FunctionType::FunctionDeclaration {
            entry.nested_declaration_name =
                function.id.as_ref().map(|id| Arc::from(id.name.as_str()));
        }
    }
    ctx.push(entry, node);
    Some((key, locator, body))
}

fn formal_params(params: &oxc_ast::ast::FormalParameters<'_>) -> Arc<[FunctionParamRecord]> {
    let mut out: Vec<FunctionParamRecord> = params
        .items
        .iter()
        .map(|param| FunctionParamRecord {
            name: match &param.pattern {
                BindingPattern::BindingIdentifier(id) => Some(Arc::from(id.name.as_str())),
                _ => None,
            },
            optional: param.optional,
            rest: false,
            has_ts_annotation: param.type_annotation.is_some(),
            annotation_reference: param.type_annotation.as_ref().and_then(|annotation| {
                match &annotation.type_annotation {
                    oxc_ast::ast::TSType::TSTypeReference(reference)
                        if reference.type_arguments.is_none() =>
                    {
                        match &reference.type_name {
                            oxc_ast::ast::TSTypeName::IdentifierReference(name) => {
                                Some(Arc::from(name.name.as_str()))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                }
            }),
        })
        .collect();
    if let Some(rest) = params.rest.as_ref() {
        out.push(FunctionParamRecord {
            name: match &rest.rest.argument {
                BindingPattern::BindingIdentifier(id) => Some(Arc::from(id.name.as_str())),
                _ => None,
            },
            optional: false,
            rest: true,
            has_ts_annotation: false,
            annotation_reference: None,
        });
    }
    Arc::from(out.into_boxed_slice())
}

// ---------------------------------------------------------------------------
// Type-parameter occurrence in the formal parameter list
// ---------------------------------------------------------------------------

/// For each type name referenced from a formal parameter's authored
/// annotation, the SMALLEST parameter ordinal that references it.
///
/// A caller's inference oracle: TypeScript infers a type argument only
/// from an argument the call actually supplies at a parameter position
/// whose type names the parameter, and falls back to the declared
/// default only when inference produced NO candidate. That is a purely
/// SYNTACTIC question about the callee's parameter list, so it is a
/// shallow index fact rather than a lowering.
#[derive(Debug, Default)]
struct TypeParamOccurrences {
    /// `(referenced name, smallest parameter ordinal)`.
    first: rustc_hash::FxHashMap<String, u32>,
}

impl TypeParamOccurrences {
    fn of(walks: &verter_parser::oxc_parse::ProgramWalkStack<'_>, node: &FunctionNode<'_>) -> Self {
        let mut out = Self::default();
        let Some(params) = node.params() else {
            return out;
        };
        for (ordinal, param) in params.items.iter().enumerate() {
            if let Some(annotation) = param.type_annotation.as_ref() {
                out.collect(walks, &annotation.type_annotation, ordinal as u32);
            }
        }
        if let Some(rest) = params.rest.as_ref() {
            if let Some(annotation) = rest.type_annotation.as_ref() {
                out.collect(
                    walks,
                    &annotation.type_annotation,
                    params.items.len() as u32,
                );
            }
        }
        out
    }

    fn collect(
        &mut self,
        walks: &verter_parser::oxc_parse::ProgramWalkStack<'_>,
        ty: &oxc_ast::ast::TSType<'_>,
        ordinal: u32,
    ) {
        let mut visitor = ReferencedTypeNames {
            found: Vec::new(),
            shadowed: Vec::new(),
        };
        walks.with_node_stack(ty.span(), || visitor.visit_ts_type(ty));
        for name in visitor.found {
            self.first
                .entry(name)
                .and_modify(|existing| *existing = (*existing).min(ordinal))
                .or_insert(ordinal);
        }
    }

    fn first_ordinal(&self, name: &str) -> Option<u32> {
        self.first.get(name).copied()
    }
}

/// The HEAD names of every type reference inside one authored type
/// annotation, skipping any subtree a nested type-parameter clause
/// re-declares (a nested signature owns its own binders, so an outer
/// clause parameter is not referenced there).
struct ReferencedTypeNames {
    found: Vec<String>,
    /// The stack of names nested clauses currently shadow.
    shadowed: Vec<Vec<String>>,
}

impl ReferencedTypeNames {
    fn is_shadowed(&self, name: &str) -> bool {
        self.shadowed
            .iter()
            .any(|frame| frame.iter().any(|shadow| shadow == name))
    }

    fn with_clause<R>(
        &mut self,
        declaration: Option<&oxc_ast::ast::TSTypeParameterDeclaration<'_>>,
        body: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let names: Vec<String> = declaration
            .map(|declaration| {
                declaration
                    .params
                    .iter()
                    .map(|param| param.name.name.as_str().to_string())
                    .collect()
            })
            .unwrap_or_default();
        self.shadowed.push(names);
        let out = body(self);
        self.shadowed.pop();
        out
    }
}

impl<'a> Visit<'a> for ReferencedTypeNames {
    fn visit_ts_type_reference(&mut self, reference: &oxc_ast::ast::TSTypeReference<'a>) {
        if let oxc_ast::ast::TSTypeName::IdentifierReference(id) = &reference.type_name {
            if !self.is_shadowed(id.name.as_str()) {
                self.found.push(id.name.as_str().to_string());
            }
        }
        walk::walk_ts_type_reference(self, reference);
    }

    fn visit_ts_function_type(&mut self, function: &oxc_ast::ast::TSFunctionType<'a>) {
        self.with_clause(function.type_parameters.as_deref(), |visitor| {
            walk::walk_ts_function_type(visitor, function);
        });
    }

    fn visit_ts_constructor_type(&mut self, constructor: &oxc_ast::ast::TSConstructorType<'a>) {
        self.with_clause(constructor.type_parameters.as_deref(), |visitor| {
            walk::walk_ts_constructor_type(visitor, constructor);
        });
    }

    fn visit_ts_method_signature(&mut self, signature: &oxc_ast::ast::TSMethodSignature<'a>) {
        self.with_clause(signature.type_parameters.as_deref(), |visitor| {
            walk::walk_ts_method_signature(visitor, signature);
        });
    }

    fn visit_ts_call_signature_declaration(
        &mut self,
        signature: &oxc_ast::ast::TSCallSignatureDeclaration<'a>,
    ) {
        self.with_clause(signature.type_parameters.as_deref(), |visitor| {
            walk::walk_ts_call_signature_declaration(visitor, signature);
        });
    }

    fn visit_ts_construct_signature_declaration(
        &mut self,
        signature: &oxc_ast::ast::TSConstructSignatureDeclaration<'a>,
    ) {
        self.with_clause(signature.type_parameters.as_deref(), |visitor| {
            walk::walk_ts_construct_signature_declaration(visitor, signature);
        });
    }
}

// ---------------------------------------------------------------------------
// Entry build: inventory + stable hash
// ---------------------------------------------------------------------------

impl<'source, 'ast> DiscoveryCtx<'source, 'ast> {
    fn build_entry(
        &mut self,
        key: FunctionProgramKey,
        locator: FunctionBodyLocator,
        params: Arc<[FunctionParamRecord]>,
        body: FunctionBodyRef<'ast>,
        function_start: u32,
        node: FunctionNode<'ast>,
    ) -> FunctionProgramEntry {
        let function_end = node.span().end;
        let frame_span = verter_span::Span::new(function_start, function_end);
        let mut inventory = InventoryVisitor {
            call_addresses: self.nodes.as_mut().map(|nodes| &mut nodes.call_sites),
            frame_span,
            ..InventoryVisitor::default()
        };
        // Parameters bind first in source order, before any body statement.
        if let FunctionNode::Function(function) = node {
            if function.r#type == oxc_ast::ast::FunctionType::FunctionExpression {
                if let Some(id) = &function.id {
                    inventory.record_binding(id, FunctionBindingKind::NestedFunction, frame_span);
                }
            }
        }
        inventory.in_parameter_list = true;
        for param in node.param_items() {
            inventory.record_pattern(&param.pattern, FunctionBindingKind::Param, frame_span);
            self.walks.with_node_stack(param.span, || {
                inventory.visit_binding_pattern(&param.pattern);
                if let Some(annotation) = &param.type_annotation {
                    inventory.visit_type_queries(
                        &annotation.type_annotation,
                        FunctionTypeQueryPosition::Parameter,
                    );
                }
                if let Some(initializer) = &param.initializer {
                    inventory.visit_expression(initializer);
                }
            });
        }
        if let Some(rest) = node.param_rest() {
            inventory.record_pattern(&rest.rest.argument, FunctionBindingKind::Param, frame_span);
            self.walks.with_node_stack(rest.span, || {
                inventory.visit_binding_pattern(&rest.rest.argument);
                if let Some(annotation) = &rest.type_annotation {
                    inventory.visit_type_queries(
                        &annotation.type_annotation,
                        FunctionTypeQueryPosition::Parameter,
                    );
                }
            });
        }
        inventory.in_parameter_list = false;
        for stmt in body.statements() {
            self.walks
                .with_node_stack(stmt.span(), || inventory.visit_statement(stmt));
        }
        // An expression body walks as the one expression statement it was:
        // no control input rides into it.
        if let Some(expression) = body.expression() {
            let previous_control = inventory.control_input.take();
            self.walks
                .with_node_stack(expression.span(), || inventory.visit_expression(expression));
            inventory.control_input = previous_control;
        }
        let InventoryVisitor {
            call_addresses: _,
            bindings,
            unmodeled_bindings,
            in_parameter_list: _,
            creates_unserved_callable,
            class_local_scope: _,
            unserved_assignments,
            parameter_callable_references,
            references,
            source_type_queries,
            type_queries,
            return_sites,
            writes,
            effects,
            control,
            control_stack: _,
            scope_stack: _,
            frame_span: _,
            read_role: _,
            control_input: _,
            compound_target_read: _,
        } = inventory;
        // The indexed call sites come from the ONE shared call-site walk
        // (`for_each_call_expression`) — the same ordering the callback
        // locator ordinals and the deref use, by construction.
        let mut call_sites = Vec::new();
        for_each_call_expression_in_body(body, |call| {
            call_sites.push(call_site_record(call));
        });

        let parameter_occurrences = TypeParamOccurrences::of(&self.walks, &node);
        let type_parameters: Vec<FunctionProgramTypeParam> = node
            .type_parameters()
            .map(|declaration| {
                declaration
                    .params
                    .iter()
                    .map(|param| {
                        let name = param.name.name.as_str();
                        FunctionProgramTypeParam {
                            name: Arc::from(name),
                            has_default: param.default.is_some(),
                            first_parameter_occurrence: parameter_occurrences.first_ordinal(name),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // The stable and exact hashes fold each nested function's once
        // discovery is done (`hash_entries`).
        FunctionProgramEntry {
            key,
            span: frame_span,
            body_span: {
                let span = node.body().expect("indexed functions have a body").span();
                verter_span::Span::new(span.start, span.end)
            },
            locator,
            params,
            bindings: Arc::from(bindings.into_boxed_slice()),
            unmodeled_bindings: unmodeled_bindings.into(),
            references: Arc::from(references.into_boxed_slice()),
            source_type_queries: source_type_queries.into(),
            type_queries: type_queries.into(),
            return_sites: Arc::from(return_sites.into_boxed_slice()),
            writes: Arc::from(writes.into_boxed_slice()),
            descendant_writes: Arc::from([]),
            unserved_assignments: unserved_assignments.into(),
            descendant_assignments: Arc::from([]),
            captured_reads: Arc::from([]),
            nested_captures: Arc::from([]),
            parameter_callable_references: parameter_callable_references.into(),
            parameter_callable_captures: Arc::from([]),
            effects: Arc::from(effects.into_boxed_slice()),
            call_sites: Arc::from(call_sites.into_boxed_slice()),
            control: Arc::from(control.into_boxed_slice()),
            direct_calls: Arc::from(Vec::new().into_boxed_slice()),
            type_parameters: Arc::from(type_parameters.into_boxed_slice()),
            lexical_parent: None,
            class_member: false,
            nested_declaration_name: None,
            captures: CanonicalCaptureIdentity::default(),
            captures_exhaustive: !creates_unserved_callable,
            flow_body_stable_hash: Hash16::default(),
            flow_body_exact_hash: None,
        }
    }
}

/// The out-of-index statement-list inventory — the SAME single inventory
/// walk the index uses (a control region's `has_return` is marked when
/// the walk meets a `return` of the current function; nested function
/// bodies are never entered). For consumers that lower a body outside the
/// index (e.g. a nested function value's body in the flow IR).
#[derive(Default)]
pub struct StatementListInventory {
    /// The control-region skeleton.
    pub control: Vec<FunctionControlRegion>,
    /// The hoisted nested function declaration names bound in this frame.
    pub nested_function_names: Vec<Arc<str>>,
    /// The function-scoped (`var` / `using`) declarator names bound in
    /// this frame — the bindings that OUTLIVE the statement list that
    /// declares them.
    pub var_names: Vec<Arc<str>>,
}

/// Inventory one statement list with the SAME single walk the index uses.
pub fn inventory_statement_list(
    walks: &verter_parser::oxc_parse::ProgramWalkStack<'_>,
    statements: &[Statement<'_>],
) -> StatementListInventory {
    let mut inventory = InventoryVisitor::default();
    for stmt in statements {
        walks.with_node_stack(stmt.span(), || inventory.visit_statement(stmt));
    }
    let mut nested_function_names = Vec::new();
    let mut var_names = Vec::new();
    for binding in inventory.bindings {
        match binding.kind {
            FunctionBindingKind::NestedFunction => nested_function_names.push(binding.name),
            FunctionBindingKind::Var => var_names.push(binding.name),
            _ => {}
        }
    }
    StatementListInventory {
        control: inventory.control,
        nested_function_names,
        var_names,
    }
}

/// The current-function inventory walker. Nested function / arrow / class
/// bodies are never entered (their contents belong to their own frames);
/// a nested function DECLARATION still binds its name in this frame. A
/// control region's `has_return` is computed by THIS SAME walk: a return
/// of the current function marks every enclosing region on the control
/// stack.
#[derive(Default)]
struct InventoryVisitor<'sink, 'ast> {
    call_addresses:
        Option<&'sink mut rustc_hash::FxHashMap<verter_span::Span, IndexedCallSite<'ast>>>,
    bindings: Vec<FunctionBindingRecord>,
    unmodeled_bindings: Vec<FunctionBindingRecord>,
    /// Set while the frame's formal parameters are walked.
    in_parameter_list: bool,
    /// The frame creates a callable no entry serves: a class, or a
    /// callable in the parameter list.
    creates_unserved_callable: bool,
    class_local_scope: Option<verter_span::Span>,
    /// The whole-binding assignments code no entry serves (a class, a
    /// parameter-list callable) makes to names it does not declare
    /// ([`access::EscapingAssignments`]).
    unserved_assignments: Vec<FunctionReferenceRecord>,
    /// Every reference a parameter-list callable makes to a name it does
    /// not declare, by the callable's span.
    parameter_callable_references: Vec<(verter_span::Span, Arc<[FunctionReferenceRecord]>)>,
    references: Vec<FunctionReferenceRecord>,
    source_type_queries: Vec<FunctionSourceTypeQuery>,
    type_queries: Vec<FunctionTypeQuery>,
    return_sites: Vec<FunctionReturnSite>,
    writes: Vec<FunctionWriteRecord>,
    effects: Vec<FunctionEffectRecord>,
    control: Vec<FunctionControlRegion>,
    /// Indices into `control` of the currently open regions (innermost
    /// last) — a `return` marks every one of them.
    control_stack: Vec<usize>,
    /// The spans of the currently open block-like scopes (innermost
    /// last). A block-scoped binding records the innermost one.
    scope_stack: Vec<verter_span::Span>,
    /// The whole frame's span — the scope of a parameter or a `var`, and
    /// the fallback when no block-like region is open.
    frame_span: verter_span::Span,
    read_role: FunctionReadRole,
    control_input: Option<oxc_span::Span>,
    compound_target_read: bool,
}

impl InventoryVisitor<'_, '_> {
    /// Record one parameter-list callable's escaping references: its
    /// assignments reach the enclosing frames as every unserved code's do,
    /// and its reads and writes together name its captures.
    fn record_parameter_callable(
        &mut self,
        span: verter_span::Span,
        escaping: access::EscapingAssignments,
    ) {
        let (writes, reads) = escaping.into_escaping_references();
        self.unserved_assignments.extend(writes.iter().cloned());
        let mut references = reads;
        references.extend(writes.into_iter().map(|mut write| {
            write.read_role = None;
            write
        }));
        references.sort_by_key(|reference| reference.span.start);
        self.parameter_callable_references
            .push((span, Arc::from(references.into_boxed_slice())));
    }

    fn record_binding(
        &mut self,
        id: &oxc_ast::ast::BindingIdentifier<'_>,
        kind: FunctionBindingKind,
        scope_span: verter_span::Span,
    ) {
        let bindings = if self.class_local_scope.is_some() {
            &mut self.unmodeled_bindings
        } else {
            &mut self.bindings
        };
        bindings.push(FunctionBindingRecord {
            name: Arc::from(id.name.as_str()),
            kind,
            span: verter_span::Span::new(id.span.start, id.span.end),
            scope_span,
            evolving_array: false,
        });
    }

    fn record_pattern(
        &mut self,
        pattern: &BindingPattern<'_>,
        kind: FunctionBindingKind,
        scope: verter_span::Span,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(id) => self.record_binding(id, kind, scope),
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    self.record_pattern(&property.value, kind, scope);
                }
                if let Some(rest) = &object.rest {
                    self.record_pattern(&rest.argument, kind, scope);
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for element in array.elements.iter().flatten() {
                    self.record_pattern(element, kind, scope);
                }
                if let Some(rest) = &array.rest {
                    self.record_pattern(&rest.argument, kind, scope);
                }
            }
            BindingPattern::AssignmentPattern(assignment) => {
                self.record_pattern(&assignment.left, kind, scope)
            }
        }
    }

    /// The innermost open block-like scope, else the whole frame.
    fn block_scope(&self) -> verter_span::Span {
        self.scope_stack.last().copied().unwrap_or(self.frame_span)
    }

    /// Record every bare `typeof name` inside `ty` at `position`.
    fn visit_type_queries(
        &mut self,
        ty: &oxc_ast::ast::TSType<'_>,
        position: FunctionTypeQueryPosition,
    ) {
        struct TypeQueries<'q> {
            out: &'q mut Vec<FunctionTypeQuery>,
            position: FunctionTypeQueryPosition,
        }
        impl<'a> Visit<'a> for TypeQueries<'_> {
            fn visit_ts_type_query(&mut self, query: &oxc_ast::ast::TSTypeQuery<'a>) {
                if let (oxc_ast::ast::TSTypeQueryExprName::IdentifierReference(id), None) =
                    (&query.expr_name, &query.type_arguments)
                {
                    self.out.push(FunctionTypeQuery {
                        name: Arc::from(id.name.as_str()),
                        span: verter_span::Span::new(id.span.start, id.span.end),
                        binding: FunctionReferenceBinding::Free,
                        position: self.position,
                    });
                }
                walk::walk_ts_type_query(self, query);
            }
        }
        TypeQueries {
            out: &mut self.type_queries,
            position,
        }
        .visit_ts_type(ty);
    }

    fn record_reference(
        &mut self,
        id: &oxc_ast::ast::IdentifierReference<'_>,
        role: Option<FunctionReadRole>,
    ) {
        self.references.push(FunctionReferenceRecord {
            name: Arc::from(id.name.as_str()),
            span: verter_span::Span::new(id.span.start, id.span.end),
            binding: FunctionReferenceBinding::Free,
            read_role: role,
            path: Arc::from([]),
        });
    }
}

impl<'a> InventoryVisitor<'_, 'a> {
    /// Visit one class property initializer. A STATIC initializer runs at
    /// class evaluation, in this frame. A class expression's INSTANCE
    /// initializer runs at construction, but it reads this frame's
    /// lexical scope and the flow lane types it here, so its references
    /// carry occurrence authority in this frame; its writes run at
    /// construction, never at this frame's position, so none of them
    /// retypes a binding here. A local class declaration's instance
    /// initializer is not a position any lowering of this frame reads.
    fn visit_class_initializer(
        &mut self,
        value: &Expression<'a>,
        is_static: bool,
        class: &Class<'a>,
    ) {
        if is_static {
            self.visit_expression(value);
        } else if class.r#type == oxc_ast::ast::ClassType::ClassExpression {
            let writes_before = self.writes.len();
            self.visit_expression(value);
            self.writes.truncate(writes_before);
        }
    }
}

impl<'a> Visit<'a> for InventoryVisitor<'_, 'a> {
    fn visit_ts_type(&mut self, it: &oxc_ast::ast::TSType<'a>) {
        self.visit_type_queries(it, FunctionTypeQueryPosition::Expression);
    }

    fn visit_ts_type_annotation(&mut self, it: &oxc_ast::ast::TSTypeAnnotation<'a>) {
        self.visit_type_queries(&it.type_annotation, FunctionTypeQueryPosition::Expression);
    }

    fn visit_variable_declarator(&mut self, it: &oxc_ast::ast::VariableDeclarator<'a>) {
        self.visit_binding_pattern(&it.id);
        if let Some(annotation) = &it.type_annotation {
            let position = match &it.id {
                BindingPattern::BindingIdentifier(id) => FunctionTypeQueryPosition::Declarator(
                    verter_span::Span::new(id.span.start, id.span.end),
                ),
                _ => FunctionTypeQueryPosition::Expression,
            };
            self.visit_type_queries(&annotation.type_annotation, position);
        }
        if let Some(init) = &it.init {
            self.visit_expression(init);
        }
    }

    fn visit_expression(&mut self, it: &Expression<'a>) {
        let previous = self.read_role;
        if self.control_input == Some(it.span()) {
            self.read_role = self.read_role.with_control();
        }
        match it {
            Expression::LogicalExpression(logical) => {
                let enclosing = self.read_role;
                self.read_role = enclosing.with_control();
                self.visit_expression(&logical.left);
                self.read_role = enclosing;
                self.visit_expression(&logical.right);
            }
            Expression::ConditionalExpression(conditional) => {
                let enclosing = self.read_role;
                self.read_role = enclosing.with_control();
                self.visit_expression(&conditional.test);
                self.read_role = enclosing;
                self.visit_expression(&conditional.consequent);
                self.visit_expression(&conditional.alternate);
            }
            _ => walk::walk_expression(self, it),
        }
        self.read_role = previous;
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        // Nested function body: not this frame. (visit_function is only
        // reached for nested positions — the entry's own body is driven
        // statement-by-statement.) Only body callables are indexed as
        // children; a parameter-list callable has no entry.
        self.creates_unserved_callable |= self.in_parameter_list;
        if self.in_parameter_list {
            let mut escaping = access::EscapingAssignments::default();
            escaping.visit_function(it, flags);
            self.record_parameter_callable(
                verter_span::Span::new(it.span.start, it.span.end),
                escaping,
            );
        }
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.creates_unserved_callable |= self.in_parameter_list;
        if self.in_parameter_list {
            let mut escaping = access::EscapingAssignments::default();
            escaping.visit_arrow_function_expression(it);
            self.record_parameter_callable(
                verter_span::Span::new(it.span.start, it.span.end),
                escaping,
            );
        }
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        // No entry serves a class's constructor or field initializers (a
        // class EXPRESSION's methods and accessors are served as nested
        // callables, a local class declaration's are not).
        self.creates_unserved_callable = true;
        // Every assignment the class makes to a name it does not declare —
        // in a member body, an initializer, a static block or its heritage
        // — assigns that binding for the checker, whether or not an entry
        // serves the member.
        let mut escaping = access::EscapingAssignments::default();
        escaping.visit_class(class);
        self.unserved_assignments.extend(escaping.into_escaping());
        // Class evaluation has occurrence authority, but remains outside the
        // function's supported flow topology. Keep only lexical references,
        // write roots and unsupported local declarations from this traversal.
        let mut evaluated = InventoryVisitor {
            frame_span: self.frame_span,
            scope_stack: self.scope_stack.clone(),
            call_addresses: self.call_addresses.as_deref_mut(),
            ..InventoryVisitor::default()
        };
        if class.r#type == oxc_ast::ast::ClassType::ClassExpression {
            if let Some(id) = &class.id {
                evaluated.unmodeled_bindings.push(FunctionBindingRecord {
                    name: Arc::from(id.name.as_str()),
                    kind: FunctionBindingKind::Class,
                    span: verter_span::Span::new(id.span.start, id.span.end),
                    scope_span: verter_span::Span::new(class.span.start, class.span.end),
                    evolving_array: false,
                });
            }
        }
        evaluated.visit_decorators(&class.decorators);
        if let Some(heritage) = class.heritage.as_ref().map(|heritage| &heritage.expression) {
            evaluated.visit_expression(heritage);
        }
        for element in &class.body.body {
            match element {
                oxc_ast::ast::ClassElement::StaticBlock(block) => {
                    evaluated.class_local_scope =
                        Some(verter_span::Span::new(block.span.start, block.span.end));
                    evaluated
                        .scope_stack
                        .push(verter_span::Span::new(block.span.start, block.span.end));
                    for statement in &block.body {
                        evaluated.visit_statement(statement);
                    }
                    evaluated.scope_stack.pop();
                    evaluated.class_local_scope = None;
                }
                oxc_ast::ast::ClassElement::MethodDefinition(method) => {
                    evaluated.visit_decorators(&method.decorators);
                    if method.computed {
                        evaluated.visit_property_key(&method.key);
                    }
                }
                oxc_ast::ast::ClassElement::PropertyDefinition(property) => {
                    evaluated.visit_decorators(&property.decorators);
                    if property.computed {
                        evaluated.visit_property_key(&property.key);
                    }
                    if let Some(value) = &property.value {
                        evaluated.visit_class_initializer(value, property.r#static, class);
                    }
                }
                oxc_ast::ast::ClassElement::AccessorProperty(property) => {
                    evaluated.visit_decorators(&property.decorators);
                    if property.computed {
                        evaluated.visit_property_key(&property.key);
                    }
                    if let Some(value) = &property.value {
                        evaluated.visit_class_initializer(value, property.r#static, class);
                    }
                }
                _ => {}
            }
        }
        self.references.extend(evaluated.references);
        self.source_type_queries
            .extend(evaluated.source_type_queries);
        self.writes.extend(evaluated.writes);
        self.unmodeled_bindings.extend(evaluated.unmodeled_bindings);
    }

    fn visit_catch_clause(&mut self, clause: &oxc_ast::ast::CatchClause<'a>) {
        self.scope_stack
            .push(verter_span::Span::new(clause.span.start, clause.span.end));
        if let Some(param) = &clause.param {
            self.record_pattern(
                &param.pattern,
                FunctionBindingKind::CatchParam,
                verter_span::Span::new(clause.span.start, clause.span.end),
            );
        }
        walk::walk_catch_clause(self, clause);
        self.scope_stack.pop();
    }

    fn visit_statement(&mut self, it: &Statement<'a>) {
        let named = match it {
            Statement::ClassDeclaration(class) => {
                class.id.as_ref().map(|id| (id, FunctionBindingKind::Class))
            }
            Statement::TSEnumDeclaration(declaration) => {
                Some((&declaration.id, FunctionBindingKind::Enum))
            }
            Statement::TSNamespaceDeclaration(module) => {
                Some((&module.id, FunctionBindingKind::Namespace))
            }
            Statement::TSImportEqualsDeclaration(declaration) => {
                Some((&declaration.id, FunctionBindingKind::ImportEquals))
            }
            _ => None,
        };
        if let Some((id, kind)) = named {
            self.record_binding(id, kind, self.block_scope());
        }
        if let Statement::FunctionDeclaration(func) = it {
            if let Some(id) = func.id.as_ref() {
                let scope_span = self.block_scope();
                self.record_binding(id, FunctionBindingKind::NestedFunction, scope_span);
            }
            // Do not descend: the nested body is its own frame.
            return;
        }
        let kind = match it {
            Statement::BlockStatement(_) => Some(FunctionControlKind::Block),
            Statement::IfStatement(_) => Some(FunctionControlKind::If),
            Statement::DoWhileStatement(_)
            | Statement::ForInStatement(_)
            | Statement::ForOfStatement(_)
            | Statement::ForStatement(_)
            | Statement::WhileStatement(_) => Some(FunctionControlKind::Loop),
            Statement::SwitchStatement(_) => Some(FunctionControlKind::Switch),
            Statement::TryStatement(_) => Some(FunctionControlKind::Try),
            Statement::LabeledStatement(_) => Some(FunctionControlKind::Labeled),
            _ => None,
        };
        if let Some(kind) = kind {
            self.control.push(FunctionControlRegion {
                kind,
                has_return: false,
                span: verter_span::Span::new(it.span().start, it.span().end),
            });
            self.control_stack.push(self.control.len() - 1);
            // Every one of these constructs also opens a lexical scope
            // for the `const` / `let` / nested function declarations it
            // contains.
            self.scope_stack
                .push(verter_span::Span::new(it.span().start, it.span().end));
        }
        let previous_control = self.control_input;
        self.control_input = match it {
            Statement::IfStatement(statement) => Some(statement.test.span()),
            Statement::WhileStatement(statement) => Some(statement.test.span()),
            Statement::DoWhileStatement(statement) => Some(statement.test.span()),
            Statement::ForStatement(statement) => statement.test.as_ref().map(GetSpan::span),
            Statement::SwitchStatement(statement) => Some(statement.discriminant.span()),
            _ => None,
        };
        walk::walk_statement(self, it);
        self.control_input = previous_control;
        if kind.is_some() {
            self.control_stack.pop();
            self.scope_stack.pop();
        }
    }

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        // `using` / `await using` are BLOCK-scoped resource declarations
        // (the `const` scoping rule plus disposal), never function-scoped
        // `var`s: classifying them as `var` makes them escape their block
        // through every hoisting rail.
        let kind = match it.kind {
            oxc_ast::ast::VariableDeclarationKind::Const
            | oxc_ast::ast::VariableDeclarationKind::Using
            | oxc_ast::ast::VariableDeclarationKind::AwaitUsing => FunctionBindingKind::Const,
            oxc_ast::ast::VariableDeclarationKind::Let => FunctionBindingKind::Let,
            oxc_ast::ast::VariableDeclarationKind::Var => FunctionBindingKind::Var,
        };
        // `var` is function-scoped; `const` / `let` / `using` are scoped
        // to the innermost enclosing block-like region.
        let scope_span = if kind == FunctionBindingKind::Var {
            self.class_local_scope.unwrap_or(self.frame_span)
        } else {
            self.block_scope()
        };
        for declarator in &it.declarations {
            self.record_pattern(&declarator.id, kind, scope_span);
            if declarator.type_annotation.is_none()
                && matches!(declarator.id, BindingPattern::BindingIdentifier(_))
                && crate::analysis::flow::is_evolving_array_initializer(declarator.init.as_ref())
            {
                let bindings = if self.class_local_scope.is_some() {
                    &mut self.unmodeled_bindings
                } else {
                    &mut self.bindings
                };
                if let Some(binding) = bindings.last_mut() {
                    binding.evolving_array = true;
                }
            }
        }
        walk::walk_variable_declaration(self, it);
    }

    fn visit_return_statement(&mut self, it: &oxc_ast::ast::ReturnStatement<'a>) {
        // A `return` of the current function is contained by every
        // enclosing control region (drives return-transparency: a
        // return-free loop / labeled construct is fall-through
        // transparent; a return-bearing one is unsupported).
        for index in &self.control_stack {
            self.control[*index].has_return = true;
        }
        self.return_sites.push(FunctionReturnSite {
            ordinal: u32::try_from(self.return_sites.len()).unwrap_or(u32::MAX),
            has_argument: it.argument.is_some(),
            span: verter_span::Span::new(it.span.start, it.span.end),
        });
        walk::walk_return_statement(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.record_reference(it, Some(self.read_role));
    }

    fn visit_static_member_expression(&mut self, it: &oxc_ast::ast::StaticMemberExpression<'a>) {
        if let Some(mut reference) = access::static_member_reference(it) {
            reference.read_role = Some(self.read_role);
            self.references.push(reference);
        } else {
            walk::walk_static_member_expression(self, it);
        }
    }

    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        self.writes.push(FunctionWriteRecord {
            span: verter_span::Span::new(it.span.start, it.span.end),
            targets: access::assignment_targets(&it.left).into(),
        });
        let previous = self.compound_target_read;
        self.compound_target_read =
            !matches!(it.operator, oxc_ast::ast::AssignmentOperator::Assign);
        self.visit_assignment_target(&it.left);
        self.compound_target_read = previous;
        self.visit_expression(&it.right);
    }

    fn visit_update_expression(&mut self, it: &oxc_ast::ast::UpdateExpression<'a>) {
        self.writes.push(FunctionWriteRecord {
            span: verter_span::Span::new(it.span.start, it.span.end),
            targets: access::simple_assignment_target(&it.argument)
                .into_iter()
                .collect(),
        });
        let previous = self.compound_target_read;
        self.compound_target_read = true;
        self.visit_simple_assignment_target(&it.argument);
        self.compound_target_read = previous;
    }

    fn visit_simple_assignment_target(&mut self, it: &oxc_ast::ast::SimpleAssignmentTarget<'a>) {
        if let oxc_ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) = it {
            self.record_reference(
                identifier,
                self.compound_target_read.then_some(self.read_role),
            );
        } else {
            walk::walk_simple_assignment_target(self, it);
        }
    }

    fn visit_assignment_target_property_identifier(
        &mut self,
        it: &oxc_ast::ast::AssignmentTargetPropertyIdentifier<'a>,
    ) {
        self.record_reference(&it.binding, None);
        if let Some(init) = &it.init {
            self.visit_expression(init);
        }
    }

    fn visit_for_in_statement(&mut self, it: &oxc_ast::ast::ForInStatement<'a>) {
        if let Some(target) = it.left.as_assignment_target() {
            self.writes.push(FunctionWriteRecord {
                span: verter_span::Span::new(it.left.span().start, it.left.span().end),
                targets: access::assignment_targets(target).into(),
            });
        }
        walk::walk_for_in_statement(self, it);
    }

    fn visit_for_of_statement(&mut self, it: &oxc_ast::ast::ForOfStatement<'a>) {
        if let Some(target) = it.left.as_assignment_target() {
            self.writes.push(FunctionWriteRecord {
                span: verter_span::Span::new(it.left.span().start, it.left.span().end),
                targets: access::assignment_targets(target).into(),
            });
        }
        walk::walk_for_of_statement(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        super::type_eval_build::for_each_indexed_call_source_type_query(it, |query| {
            self.source_type_queries.push(FunctionSourceTypeQuery {
                name: Arc::from(query.name.as_str()),
                span: verter_span::Span::new(query.span.start, query.span.end),
                binding: FunctionReferenceBinding::Free,
            });
        });
        let call = self.alloc(it);
        if let Some(addresses) = &mut self.call_addresses {
            addresses
                .entry(verter_span::Span::new(it.span.start, it.span.end))
                .or_insert(IndexedCallSite::Call(call));
        }
        let callee = match &it.callee {
            Expression::Identifier(id) => {
                FunctionEffectCallee::Identifier(Arc::from(id.name.as_str()))
            }
            Expression::StaticMemberExpression(member) => {
                let mut path = Vec::new();
                if collect_static_member_path(member, &mut path) {
                    FunctionEffectCallee::StaticMember(Arc::from(path.into_boxed_slice()))
                } else {
                    FunctionEffectCallee::Other
                }
            }
            _ => FunctionEffectCallee::Other,
        };
        self.effects.push(FunctionEffectRecord {
            span: verter_span::Span::new(it.span.start, it.span.end),
            callee,
        });
        let previous = self.read_role;
        self.read_role = self.read_role.with_call();
        walk::walk_call_expression(self, it);
        self.read_role = previous;
    }

    fn visit_new_expression(&mut self, it: &oxc_ast::ast::NewExpression<'a>) {
        let construct = self.alloc(it);
        if let Some(addresses) = &mut self.call_addresses {
            addresses
                .entry(verter_span::Span::new(it.span.start, it.span.end))
                .or_insert(IndexedCallSite::Construct(construct));
        }
        let previous = self.read_role;
        self.read_role = self.read_role.with_call();
        walk::walk_new_expression(self, it);
        self.read_role = previous;
    }

    fn visit_tagged_template_expression(
        &mut self,
        it: &oxc_ast::ast::TaggedTemplateExpression<'a>,
    ) {
        let tagged = self.alloc(it);
        if let Some(addresses) = &mut self.call_addresses {
            addresses
                .entry(verter_span::Span::new(it.span.start, it.span.end))
                .or_insert(IndexedCallSite::TaggedTemplate(tagged));
        }
        let previous = self.read_role;
        self.read_role = self.read_role.with_call();
        walk::walk_tagged_template_expression(self, it);
        self.read_role = previous;
    }
}

/// Collect a dotted member path from a static member expression chain.
/// `a.b.c` → `["a", "b", "c"]` (in order). Returns `false` for
/// non-identifier roots (`this`, calls, computed) — an unsupported callee
/// shape, not a path.
fn collect_static_member_path(
    member: &oxc_ast::ast::StaticMemberExpression<'_>,
    path: &mut Vec<Arc<str>>,
) -> bool {
    let mut properties = Vec::new();
    let mut current = member;
    loop {
        properties.push(Arc::from(current.property.name.as_str()));
        match &current.object {
            Expression::Identifier(identifier) => {
                path.push(Arc::from(identifier.name.as_str()));
                break;
            }
            Expression::StaticMemberExpression(parent) => current = parent,
            _ => return false,
        }
    }
    properties.reverse();
    path.extend(properties);
    true
}

// ---------------------------------------------------------------------------
// Locator resolution against the retained snapshot
// ---------------------------------------------------------------------------

/// One function's body: a block, or an expression-bodied arrow's single
/// expression. oxc's AST before 0.151 carried an expression body as a block
/// holding one expression statement; every walk over a body visits the
/// expression exactly where it visited that statement.
#[derive(Clone, Copy)]
pub enum FunctionBodyRef<'a> {
    /// A block body.
    Block(&'a oxc_ast::ast::FunctionBody<'a>),
    /// An expression-bodied arrow's expression.
    Expression(&'a Expression<'a>),
}

impl<'a> FunctionBodyRef<'a> {
    /// The body of an arrow function.
    #[must_use]
    pub fn of_arrow(arrow: &'a ArrowFunctionExpression<'a>) -> Self {
        match &arrow.body {
            oxc_ast::ast::ArrowFunctionBody::FunctionBody(body) => Self::Block(body),
            body => Self::Expression(
                body.as_expression()
                    .expect("a non-block arrow body is an expression"),
            ),
        }
    }

    /// The block body's statements (none for an expression body).
    #[must_use]
    pub fn statements(self) -> &'a [Statement<'a>] {
        match self {
            Self::Block(body) => &body.statements,
            Self::Expression(_) => &[],
        }
    }

    /// The expression body, when this is one.
    #[must_use]
    pub fn expression(self) -> Option<&'a Expression<'a>> {
        match self {
            Self::Block(_) => None,
            Self::Expression(expression) => Some(expression),
        }
    }

    /// The body's source span.
    #[must_use]
    pub fn span(self) -> oxc_span::Span {
        match self {
            Self::Block(body) => body.span,
            Self::Expression(expression) => expression.span(),
        }
    }
}

/// The function node a [`FunctionBodyLocator`] descent lands on — the ONE
/// authored-position view every per-function body product (skeleton
/// build, lazy body lowering) reads from the retained snapshot.
#[derive(Clone, Copy)]
pub enum FunctionNode<'a> {
    /// A `function` declaration / expression or a class/object method.
    Function(&'a Function<'a>),
    /// An arrow function.
    Arrow(&'a ArrowFunctionExpression<'a>),
    /// A class field's initializer that reads `this` (`b = this.a + 1`), or
    /// holds a callback that may (`cb = [() => this.a]`):
    /// a position with no parameters whose value is the one expression, and
    /// whose `this` is the class's instance (or, for a static field, the
    /// class) — served like an expression-bodied arrow so the flow lane
    /// reads the receiver.
    Initializer(&'a Expression<'a>),
}

impl<'a> FunctionNode<'a> {
    /// The function's own source span.
    #[must_use]
    pub fn span(&self) -> oxc_span::Span {
        match self {
            Self::Function(func) => func.span,
            Self::Arrow(arrow) => arrow.span,
            Self::Initializer(expression) => expression.span(),
        }
    }

    /// The formal parameters (`None` for an initializer, which has none).
    #[must_use]
    pub fn params(&self) -> Option<&'a oxc_ast::ast::FormalParameters<'a>> {
        match self {
            Self::Function(func) => Some(&func.params),
            Self::Arrow(arrow) => Some(&arrow.params),
            Self::Initializer(_) => None,
        }
    }

    /// The formal parameters' items, in source order (none for an
    /// initializer).
    #[must_use]
    pub fn param_items(&self) -> &'a [oxc_ast::ast::FormalParameter<'a>] {
        self.params().map_or(&[], |params| params.items.as_slice())
    }

    /// The rest parameter, when authored.
    #[must_use]
    pub fn param_rest(&self) -> Option<&'a oxc_ast::ast::FormalParameterRest<'a>> {
        self.params().and_then(|params| params.rest.as_deref())
    }

    /// The function body (`None` for a bodiless overload signature).
    #[must_use]
    pub fn body(&self) -> Option<FunctionBodyRef<'a>> {
        match self {
            Self::Function(func) => func.body.as_deref().map(FunctionBodyRef::Block),
            Self::Arrow(arrow) => Some(FunctionBodyRef::of_arrow(arrow)),
            Self::Initializer(expression) => Some(FunctionBodyRef::Expression(expression)),
        }
    }

    /// Whether this is an expression-bodied arrow (`(x) => x * 2`) or an
    /// initializer.
    #[must_use]
    pub fn is_expression_body(&self) -> bool {
        match self {
            Self::Function(_) => false,
            Self::Arrow(arrow) => arrow.is_expression(),
            Self::Initializer(_) => true,
        }
    }

    /// Whether the position is `async`.
    #[must_use]
    pub fn is_async(&self) -> bool {
        match self {
            Self::Function(func) => func.r#async,
            Self::Arrow(arrow) => arrow.r#async,
            Self::Initializer(_) => false,
        }
    }

    /// The function's own type parameter clause, when authored.
    #[must_use]
    pub fn type_parameters(&self) -> Option<&oxc_ast::ast::TSTypeParameterDeclaration<'a>> {
        match self {
            Self::Function(func) => func.type_parameters.as_deref(),
            Self::Arrow(arrow) => arrow.type_parameters.as_deref(),
            Self::Initializer(_) => None,
        }
    }

    /// The declared return-type annotation, when authored.
    #[must_use]
    pub fn return_type(&self) -> Option<&'a oxc_ast::ast::TSTypeAnnotation<'a>> {
        match self {
            Self::Function(func) => func.return_type.as_deref(),
            Self::Arrow(arrow) => arrow.return_type.as_deref(),
            Self::Initializer(_) => None,
        }
    }
}

/// The declaration view of a statement, unwrapping the export wrappers the
/// locator descent does not record (the index discovers through
/// `export { … }` / `export default` transparently).
enum DeclRef<'a> {
    /// A function declaration.
    Function(&'a Function<'a>),
    /// A variable declaration.
    Variable(&'a VariableDeclaration<'a>),
    /// A class declaration.
    #[cfg(any(test, feature = "test-support"))]
    Class(&'a Class<'a>),
    /// A namespace or module declaration's block (`None` for a bodiless
    /// module or a dotted namespace, whose body is the inner namespace).
    Module(Option<&'a oxc_ast::ast::TSModuleBlock<'a>>),
    /// An `export default { … }` object expression.
    #[cfg(any(test, feature = "test-support"))]
    ExportDefaultObject(&'a oxc_ast::ast::ObjectExpression<'a>),
}

/// The class a statement declares, exported or not.
fn class_declaration_of<'a>(statement: &'a Statement<'a>) -> Option<&'a Class<'a>> {
    use oxc_ast::ast::{Declaration, ExportDefaultDeclarationKind};
    match statement {
        Statement::ClassDeclaration(class) => Some(class),
        Statement::ExportDeclaration(export) => match &export.declaration {
            Declaration::ClassDeclaration(class) => Some(class),
            _ => None,
        },
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::ClassDeclaration(class) => Some(class),
            _ => None,
        },
        _ => None,
    }
}

/// A namespace declaration's own block (`None` for a dotted namespace).
fn namespace_block<'a>(
    module: &'a oxc_ast::ast::TSNamespaceDeclaration<'a>,
) -> Option<&'a oxc_ast::ast::TSModuleBlock<'a>> {
    match &module.body {
        oxc_ast::ast::TSNamespaceDeclarationBody::TSModuleBlock(block) => Some(block),
        oxc_ast::ast::TSNamespaceDeclarationBody::TSNamespaceDeclaration(_) => None,
    }
}

fn declaration_of<'a>(statement: &'a Statement<'a>) -> Option<DeclRef<'a>> {
    use oxc_ast::ast::{Declaration, ExportDefaultDeclarationKind};
    match statement {
        Statement::FunctionDeclaration(func) => Some(DeclRef::Function(func)),
        Statement::VariableDeclaration(decl) => Some(DeclRef::Variable(decl)),
        #[cfg(any(test, feature = "test-support"))]
        Statement::ClassDeclaration(class) => Some(DeclRef::Class(class)),
        Statement::TSNamespaceDeclaration(module) => Some(DeclRef::Module(namespace_block(module))),
        Statement::TSExternalModuleDeclaration(module) => {
            Some(DeclRef::Module(module.body.as_deref()))
        }
        Statement::ExportDeclaration(export) => match &export.declaration {
            Declaration::FunctionDeclaration(func) => Some(DeclRef::Function(func)),
            Declaration::VariableDeclaration(decl) => Some(DeclRef::Variable(decl)),
            #[cfg(any(test, feature = "test-support"))]
            Declaration::ClassDeclaration(class) => Some(DeclRef::Class(class)),
            Declaration::TSNamespaceDeclaration(module) => {
                Some(DeclRef::Module(namespace_block(module)))
            }
            Declaration::TSExternalModuleDeclaration(module) => {
                Some(DeclRef::Module(module.body.as_deref()))
            }
            _ => None,
        },
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                Some(DeclRef::Function(func))
            }
            #[cfg(any(test, feature = "test-support"))]
            ExportDefaultDeclarationKind::ClassDeclaration(class) => Some(DeclRef::Class(class)),
            #[cfg(any(test, feature = "test-support"))]
            other => match other.as_expression() {
                Some(Expression::ObjectExpression(obj)) => Some(DeclRef::ExportDefaultObject(obj)),
                _ => None,
            },
            #[cfg(not(any(test, feature = "test-support")))]
            _ => None,
        },
        _ => None,
    }
}

/// The function node behind an initializer expression: an arrow or a
/// function expression, nothing else.
pub fn function_from_expression<'a>(expression: &'a Expression<'a>) -> Option<FunctionNode<'a>> {
    match expression {
        Expression::FunctionExpression(func) => Some(FunctionNode::Function(func)),
        Expression::ArrowFunctionExpression(arrow) => Some(FunctionNode::Arrow(arrow)),
        _ => None,
    }
}

/// Arena addresses registered by the same pass that mints function identities.
/// This worker-owned table is distinct from the arena-free shared index.
#[derive(Default)]
pub struct FunctionProgramNodes<'a> {
    functions: rustc_hash::FxHashMap<FunctionProgramKey, ResolvedFunctionNode<'a>>,
    call_sites: rustc_hash::FxHashMap<verter_span::Span, IndexedCallSite<'a>>,
}
impl<'a> FunctionProgramNodes<'a> {
    /// Borrow one exact indexed function. A missing key never falls back to a locator walk.
    pub fn get(&self, key: &FunctionProgramKey) -> Option<ResolvedFunctionNode<'a>> {
        self.functions.get(key).cloned()
    }
    /// Borrow one indexed call-shaped expression without enumerating
    /// unrelated function bodies.
    pub fn call_site(&self, span: verter_span::Span) -> Option<IndexedCallSite<'a>> {
        self.call_sites.get(&span).copied()
    }
    /// Borrow one indexed call expression.
    pub fn call(&self, span: verter_span::Span) -> Option<&'a CallExpression<'a>> {
        match self.call_site(span)? {
            IndexedCallSite::Call(call) => Some(call),
            IndexedCallSite::Construct(_) | IndexedCallSite::TaggedTemplate(_) => None,
        }
    }
}

/// One indexed expression the call executor resolves: a call, a `new`, or a
/// tagged template (a call of its tag with the template strings and the
/// substitutions as its arguments). Each is addressed by its own span.
#[derive(Clone, Copy)]
pub enum IndexedCallSite<'a> {
    Call(&'a CallExpression<'a>),
    Construct(&'a oxc_ast::ast::NewExpression<'a>),
    TaggedTemplate(&'a oxc_ast::ast::TaggedTemplateExpression<'a>),
}

/// One resolved function position: the node, its bare-identifier self
/// name, and the type-parameter clause of the declaration ENCLOSING it.
#[derive(Clone)]
pub struct ResolvedFunctionNode<'a> {
    /// The function node the locator addresses.
    pub node: FunctionNode<'a>,
    /// The function's bare-identifier SELF name, for direct-recursion
    /// detection. `None` for class members and object-literal members.
    pub self_name: Option<Arc<str>>,
    /// The type-parameter clause of the enclosing DECLARATION, when the
    /// function sits inside one that has binders of its own — today that
    /// is exactly a class member (`class C<T> { m(x: T) {} }`), whose
    /// binders are in scope throughout every member body. A namespace,
    /// a variable declarator, and an object literal declare no type
    /// parameters, so those descents carry `None`.
    pub enclosing_type_parameters: Option<&'a oxc_ast::ast::TSTypeParameterDeclaration<'a>>,
    /// The enclosing class-member heritage context, when the function is
    /// a DIRECT member of a class with an `extends` clause: the heritage
    /// expression and whether the member is STATIC — the two facts a
    /// `super.x` access inside the member resolves through (the base's
    /// instance side for an instance member, its static side for a static
    /// one). `None` outside a direct class member and for heritage-less
    /// classes; nested callables clear it, mirroring the type-parameter
    /// clause rule above.
    pub enclosing_heritage: Option<EnclosingHeritage<'a>>,
    /// The receiver `this` reads inside a DIRECT member of a class
    /// declaration: the class's instance for an instance member, its
    /// constructor for a static one. `None` everywhere else; nested
    /// callables clear it (a nested arrow's lexical `this` reaches it
    /// through the flow lane's nested context instead).
    pub enclosing_this: Option<EnclosingThis>,
}

/// What `this` is inside a direct member of a class declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnclosingThis {
    /// An instance member (method, accessor or property initializer):
    /// the class's polymorphic `this` type.
    Instance,
    /// A static member: the class constructor, `typeof C`.
    Static,
    /// A method or accessor of a variable's object literal: the variable's
    /// value.
    ObjectLiteral,
}

impl EnclosingThis {
    /// The receiver of a member whose `static` flag is `static_side`.
    #[must_use]
    pub fn of_member(static_side: bool) -> Self {
        if static_side {
            Self::Static
        } else {
            Self::Instance
        }
    }
}

/// The heritage (`extends`) access context one direct class member's body
/// evaluates its `super.x` expressions against.
#[derive(Debug, Clone, Copy)]
pub struct EnclosingHeritage<'a> {
    /// The class's authored `extends` expression.
    pub super_class: &'a oxc_ast::ast::Expression<'a>,
    /// The class's authored `extends Base<Args>` type arguments, when the
    /// heritage is generic (`None` for a non-generic `extends Base`).
    pub super_type_arguments: Option<&'a oxc_ast::ast::TSTypeParameterInstantiation<'a>>,
    /// Whether the member declaring the frame is STATIC — `super.x` in a
    /// static member reads the base CONSTRUCTOR's own (static) side; in an
    /// instance member it reads the base's PROTOTYPE side.
    pub static_side: bool,
}

/// Resolve one function's locator against the retained snapshot: the
/// contributing top-level statement, then the ordinal descent. Also
/// derives the function's bare-identifier SELF name for direct-recursion
/// detection: a function declaration contributes its id, a variable
/// initializer its declarator binding; class members and object-literal
/// members have no bare-identifier self name. Any miss is a typed `None`.
#[cfg(any(test, feature = "test-support"))]
pub fn resolve_function_node<'a>(
    program: &'a oxc_ast::ast::Program<'a>,
    locator: &FunctionBodyLocator,
) -> Option<ResolvedFunctionNode<'a>> {
    use oxc_ast::ast::ClassElement;
    let walks = verter_parser::oxc_parse::ProgramWalkStack::new(program);
    let mut statement = program
        .body
        .get(locator.contributor.contributor_index as usize)?;
    // The body of the function resolved so far, when the descent has
    // stepped INSIDE it (a nested declaration, a callback argument, an
    // IIFE callee). `None` while the descent is still navigating
    // declarations from the contributor statement.
    let mut current_body: Option<FunctionBodyRef<'a>> = None;
    // The heritage context of the innermost enclosing class member, set by
    // a `ClassMember` step and cleared by every deeper nested-position step
    // — mirroring discovery, which binds the heritage only to the member's
    // own program.
    let mut enclosing_heritage: Option<EnclosingHeritage<'a>> = None;
    let mut enclosing_this: Option<EnclosingThis> = None;
    let steps = locator.descent.to_vec();
    let mut steps = steps.iter().peekable();
    loop {
        match steps.next()? {
            // A heritage expression is an indexed program expression, never a
            // function position.
            FunctionDescentStep::ClassHeritage => return None,
            FunctionDescentStep::NamespaceMember { statement_ordinal } => {
                let DeclRef::Module(block) = declaration_of(statement)? else {
                    return None;
                };
                let block = block?;
                statement = block.body.get(*statement_ordinal as usize)?;
            }
            FunctionDescentStep::FunctionDeclaration => {
                let DeclRef::Function(func) = declaration_of(statement)? else {
                    return None;
                };
                if steps.len() == 0 {
                    // Terminal step: the statement IS the function declaration.
                    let self_name = func.id.as_ref().map(|id| Arc::from(id.name.as_str()));
                    return Some(ResolvedFunctionNode {
                        node: FunctionNode::Function(func),
                        self_name,
                        enclosing_type_parameters: None,
                        enclosing_heritage,
                        enclosing_this,
                    });
                }
                // Non-terminal: a nested position inside this function's body.
                current_body = func.body.as_deref().map(FunctionBodyRef::Block);
            }
            FunctionDescentStep::VariableInitializer { declarator_ordinal } => {
                let DeclRef::Variable(var_decl) = declaration_of(statement)? else {
                    return None;
                };
                let declarator = var_decl.declarations.get(*declarator_ordinal as usize)?;
                let self_name = match &declarator.id {
                    BindingPattern::BindingIdentifier(id) => Some(Arc::from(id.name.as_str())),
                    _ => None,
                };
                let init = declarator.init.as_ref()?;
                match steps.peek().copied() {
                    None => {
                        return Some(ResolvedFunctionNode {
                            node: function_from_expression(init)?,
                            self_name,
                            enclosing_type_parameters: None,
                            enclosing_heritage,
                            enclosing_this,
                        });
                    }
                    Some(FunctionDescentStep::ObjectMember { member_ordinal }) => {
                        steps.next();
                        let Expression::ObjectExpression(obj) = init else {
                            return None;
                        };
                        let prop = obj.properties.get(*member_ordinal as usize)?;
                        let ObjectPropertyKind::ObjectProperty(property) = prop else {
                            return None;
                        };
                        let node = function_from_expression(&property.value)?;
                        if steps.len() == 0 {
                            // Terminal step: the object-literal member inside
                            // the current initializer object expression.
                            // Object members have no bare-identifier self name.
                            return Some(ResolvedFunctionNode {
                                node,
                                self_name: None,
                                enclosing_type_parameters: None,
                                enclosing_heritage,
                                enclosing_this: matches!(node, FunctionNode::Function(_))
                                    .then_some(EnclosingThis::ObjectLiteral),
                            });
                        }
                        // Non-terminal: a nested position inside the member body.
                        current_body = node.body();
                    }
                    Some(_) => {
                        // Non-terminal: a nested position inside the
                        // initializer function's own body.
                        current_body = function_from_expression(init)?.body();
                    }
                }
            }
            FunctionDescentStep::ClassMember { member_ordinal } => {
                let DeclRef::Class(class) = declaration_of(statement)? else {
                    return None;
                };
                let element = class.body.body.get(*member_ordinal as usize)?;
                let (node, member_is_static) = match element {
                    ClassElement::MethodDefinition(method) => {
                        (FunctionNode::Function(&method.value), method.r#static)
                    }
                    ClassElement::PropertyDefinition(property) => (
                        function_from_expression(property.value.as_ref()?)?,
                        property.r#static,
                    ),
                    _ => return None,
                };
                enclosing_heritage = class.heritage.as_ref().map(|heritage| EnclosingHeritage {
                    super_class: &heritage.expression,
                    super_type_arguments: heritage.type_arguments.as_deref(),
                    static_side: member_is_static,
                });
                enclosing_this = Some(EnclosingThis::of_member(member_is_static));
                if steps.len() == 0 {
                    // Terminal step: the class member at `member_ordinal`.
                    // Class members have no bare-identifier self name. The
                    // CLASS's own type-parameter clause binds throughout every
                    // member body, so it rides out with the node — as does the
                    // heritage context a `super.x` access resolves through.
                    return Some(ResolvedFunctionNode {
                        node,
                        self_name: None,
                        enclosing_type_parameters: class.type_parameters.as_deref(),
                        enclosing_heritage,
                        enclosing_this,
                    });
                }
                // Non-terminal: a nested position inside the member body.
                current_body = node.body();
            }
            FunctionDescentStep::ExportDefaultObjectMember { member_ordinal } => {
                let DeclRef::ExportDefaultObject(obj) = declaration_of(statement)? else {
                    return None;
                };
                let prop = obj.properties.get(*member_ordinal as usize)?;
                let ObjectPropertyKind::ObjectProperty(property) = prop else {
                    return None;
                };
                let node = function_from_expression(&property.value)?;
                if steps.len() == 0 {
                    // Terminal step: the object-literal method at
                    // `member_ordinal` inside the `export default { … }`
                    // object expression.
                    // Object members have no bare-identifier self name.
                    return Some(ResolvedFunctionNode {
                        node,
                        self_name: None,
                        enclosing_type_parameters: None,
                        enclosing_heritage,
                        enclosing_this,
                    });
                }
                // Non-terminal: a nested position inside the member body.
                current_body = node.body();
            }
            FunctionDescentStep::ObjectMember { .. } => {
                // Only valid immediately after a VariableInitializer step
                // (handled there).
                return None;
            }
            FunctionDescentStep::NestedCallable { ordinal } => {
                enclosing_heritage = None;
                enclosing_this = None;
                let body = current_body?;
                let mut position = 0;
                let mut selected = None;
                for_each_nested_callable(&walks, body, |node, _| {
                    if position == *ordinal {
                        selected = Some(node);
                    }
                    position += 1;
                });
                let node = selected?;
                if steps.len() == 0 {
                    let self_name = match node {
                        FunctionNode::Function(function) => {
                            function.id.as_ref().map(|id| Arc::from(id.name.as_str()))
                        }
                        FunctionNode::Arrow(_) | FunctionNode::Initializer(_) => None,
                    };
                    return Some(ResolvedFunctionNode {
                        node,
                        self_name,
                        enclosing_type_parameters: None,
                        enclosing_heritage,
                        enclosing_this,
                    });
                }
                current_body = node.body();
            }
            FunctionDescentStep::BodyStatement { statement_ordinal } => {
                enclosing_heritage = None;
                enclosing_this = None;
                // The statement at `statement_ordinal` inside the enclosing
                // function's body — a hoisted nested function declaration.
                let body = current_body?;
                let Statement::FunctionDeclaration(func) =
                    body.statements().get(*statement_ordinal as usize)?
                else {
                    return None;
                };
                if steps.len() == 0 {
                    let self_name = func.id.as_ref().map(|id| Arc::from(id.name.as_str()));
                    return Some(ResolvedFunctionNode {
                        node: FunctionNode::Function(func),
                        self_name,
                        enclosing_type_parameters: None,
                        enclosing_heritage,
                        enclosing_this,
                    });
                }
                // Non-terminal: a nested position inside this declaration's body.
                current_body = func.body.as_deref().map(FunctionBodyRef::Block);
            }
            FunctionDescentStep::CallArgument {
                call_ordinal,
                arg_ordinal,
            } => {
                enclosing_heritage = None;
                enclosing_this = None;
                // The argument at `arg_ordinal` of the enclosing body's
                // `call_ordinal`-th call site — a callback position.
                let body = current_body?;
                let call = nth_call_expression(body, *call_ordinal)?;
                let argument = call.arguments.get(*arg_ordinal as usize)?;
                let expression = argument.as_expression()?;
                let node = function_from_expression(unwrap_program_expression(expression))?;
                if steps.len() == 0 {
                    let self_name = match node {
                        FunctionNode::Function(func) => {
                            func.id.as_ref().map(|id| Arc::from(id.name.as_str()))
                        }
                        FunctionNode::Arrow(_) | FunctionNode::Initializer(_) => None,
                    };
                    return Some(ResolvedFunctionNode {
                        node,
                        self_name,
                        enclosing_type_parameters: None,
                        enclosing_heritage,
                        enclosing_this,
                    });
                }
                // Non-terminal: a nested position inside the callback's body.
                current_body = node.body();
            }
            FunctionDescentStep::CallCallee { call_ordinal } => {
                enclosing_heritage = None;
                enclosing_this = None;
                // The CALLEE of the enclosing body's `call_ordinal`-th call
                // site — an immediately-invoked function expression.
                let body = current_body?;
                let call = nth_call_expression(body, *call_ordinal)?;
                let node = function_from_expression(unwrap_program_expression(&call.callee))?;
                if steps.len() == 0 {
                    let self_name = match node {
                        FunctionNode::Function(func) => {
                            func.id.as_ref().map(|id| Arc::from(id.name.as_str()))
                        }
                        FunctionNode::Arrow(_) | FunctionNode::Initializer(_) => None,
                    };
                    return Some(ResolvedFunctionNode {
                        node,
                        self_name,
                        enclosing_type_parameters: None,
                        enclosing_heritage,
                        enclosing_this,
                    });
                }
                // Non-terminal: a nested position inside the callee's body.
                current_body = node.body();
            }
        }
    }
}

/// The `ordinal`-th call expression inside one expression, by the ONE
/// shared source-order walk (the expression-position mirror of
/// the function-body ordinal walk).
fn nth_call_expression_in_expression<'a>(
    expression: &'a Expression<'a>,
    ordinal: usize,
) -> Option<&'a CallExpression<'a>> {
    let mut remaining = ordinal;
    let mut found = None;
    for_each_call_expression_in_expression(expression, |call| {
        if found.is_none() {
            if remaining == 0 {
                found = Some(call);
            } else {
                remaining -= 1;
            }
        }
    });
    found
}

/// Transient typed IR for one indexed declaration/callback expression:
/// the retained snapshot is re-read on demand and lowered through the ONE
/// indexed-expression lowering; no body `TypeExpr` is memo-owned.
///
/// A semantic-call record's per-argument served-return identities are
/// patched back onto the lowered arguments (and onto a function value's
/// `flow_return` slot) so the call executor can demand the exact served
/// position instead of re-deriving it.
pub fn build_indexed_program_expression_ir(
    program: &oxc_ast::ast::Program<'_>,
    source: &str,
    record: &ProgramExpressionRecord,
) -> Option<verter_type_expr::IndexedValueExpression> {
    let mut statement = program
        .body
        .get(record.locator.contributor.contributor_index as usize)?;
    let mut current_body: Option<FunctionBodyRef<'_>> = None;
    let steps = record.locator.descent.to_vec();
    let mut steps = steps.iter().peekable();
    // Prefix steps navigate to the declaration OWNING the expression: a
    // namespace block or the enclosing function's body statement list.
    loop {
        match steps.peek() {
            Some(FunctionDescentStep::NamespaceMember { statement_ordinal }) => {
                steps.next();
                let DeclRef::Module(block) = declaration_of(statement)? else {
                    return None;
                };
                let block = block?;
                statement = block.body.get(*statement_ordinal as usize)?;
            }
            Some(FunctionDescentStep::BodyStatement { statement_ordinal }) => {
                steps.next();
                let body = current_body?;
                statement = body.statements().get(*statement_ordinal as usize)?;
                let DeclRef::Function(func) = declaration_of(statement)? else {
                    return None;
                };
                current_body = func.body.as_deref().map(FunctionBodyRef::Block);
            }
            _ => break,
        }
    }
    let expression = match steps.next()? {
        FunctionDescentStep::VariableInitializer { declarator_ordinal } => {
            let DeclRef::Variable(declaration) = declaration_of(statement)? else {
                return None;
            };
            let initializer = declaration
                .declarations
                .get(*declarator_ordinal as usize)?
                .init
                .as_ref()?;
            match steps.next() {
                None => initializer,
                Some(FunctionDescentStep::CallArgument {
                    call_ordinal,
                    arg_ordinal,
                }) if steps.len() == 0 => {
                    let call =
                        nth_call_expression_in_expression(initializer, *call_ordinal as usize)?;
                    call.arguments.get(*arg_ordinal as usize)?.as_expression()?
                }
                _ => return None,
            }
        }
        FunctionDescentStep::ClassHeritage if steps.len() == 0 => {
            &class_declaration_of(statement)?
                .heritage
                .as_ref()?
                .expression
        }
        // A class field's initializer: one classified as deriving from a
        // call, which reads no `this` (a field that may is served as a
        // position of its own, never read here).
        FunctionDescentStep::ClassMember { member_ordinal } if steps.len() == 0 => {
            match class_declaration_of(statement)?
                .body
                .body
                .get(*member_ordinal as usize)?
            {
                oxc_ast::ast::ClassElement::PropertyDefinition(prop) => prop.value.as_ref()?,
                _ => return None,
            }
        }
        _ => return None,
    };
    let mut indexed =
        crate::analysis::type_eval_build::lower_indexed_value_expression(expression, source);
    if let (
        ProgramExpressionSource::SemanticCall { site, .. },
        verter_type_expr::IndexedValueExpression::Call(call),
    ) = (&record.source, &mut indexed)
    {
        for (argument, indexed_argument) in site.args.iter().zip(Arc::make_mut(&mut call.args)) {
            indexed_argument.function_return_source = argument.function_return_source.clone();
            if let (
                Some(verter_type_expr::facts::FunctionReturnSource::Flow(identity)),
                verter_type_expr::IndexedValueExpression::Value(
                    verter_type_expr::TypeExpr::Function(function),
                ),
            ) = (
                indexed_argument.function_return_source.as_ref(),
                &mut indexed_argument.expression,
            ) {
                Arc::make_mut(function).flow_return = Some(Box::new(identity.clone()));
            }
        }
    }
    Some(indexed)
}

/// The `ordinal`-th call expression inside one function body's statement
/// list, by the ONE shared source-order walk discovery used to assign
/// `call_ordinal` (so a locator derefs to exactly the call it indexed).
#[cfg(any(test, feature = "test-support"))]
fn nth_call_expression<'a>(
    body: FunctionBodyRef<'a>,
    ordinal: u32,
) -> Option<&'a CallExpression<'a>> {
    let mut seen = 0u32;
    let mut found = None;
    for_each_call_expression_in_body(body, |call| {
        if found.is_none() {
            if seen == ordinal {
                found = Some(call);
            }
            seen += 1;
        }
    });
    found
}
