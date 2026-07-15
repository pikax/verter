//! LEGACY (non-runes) `$:` reactive-statement analysis.
//!
//! Three cooperating pieces, each a structural mirror of the official
//! `svelte@5.56.3` behaviour (scope pass → analyze pass → topological order):
//!
//! 1. [`declare_reactive_assignment_targets`] — the IMPLICIT assignment-target
//!    declaration pass. A top-level `$: <target> = …` whose target binds a name
//!    not otherwise declared implicitly declares that name as a
//!    [`BindingRuntimeKind::MutableSource`] cell at the instance root scope (the
//!    official scope-pass implicit `legacy_reactive` declaration). The plan
//!    emits each as `const <name> = $.mutable_source();` — the zero-arg cell —
//!    and every read/write of the name routes through the shared signal
//!    rewriter.
//! 2. [`reactive_statement_facts`] — the per-statement DEPENDENCY and
//!    ASSIGNMENT facts, from the typed OXC AST of the labeled statement's body
//!    (never a text scan). Dependencies are referenced names in FIRST-MENTION
//!    order, excluding a name whose every mention is a pure `=`-assignment
//!    TARGET position (the official walk-up-through-member-expressions skip);
//!    assignments are the official assignment/update target extraction.
//! 3. [`order_reactive_registrations`] — the DEPENDENCY (topological) order of
//!    the `$.legacy_pre_effect` registrations across statements, with the
//!    official name-edge cycle detection: a dependency cycle is the official
//!    `reactive_declaration_cycle` compile error (a SELF-assigned dependency is
//!    excluded from the edge set, so `$: x = x + 1` is not a cycle).

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrayAssignmentTarget, ArrowFunctionExpression, AssignmentExpression, AssignmentOperator,
    AssignmentTarget, AssignmentTargetMaybeDefault, AssignmentTargetProperty, BlockStatement,
    CatchClause, Expression, ForInStatement, ForOfStatement, ForStatement, Function,
    ObjectAssignmentTarget, SimpleAssignmentTarget, Statement, UpdateExpression,
    VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use rustc_hash::{FxHashMap, FxHashSet};

use super::expr::{
    arrow_scope_names, block_scope_names, collect_pattern_names, for_left_names,
    function_scope_names, peel_parens, reparse_module, BindingInfo, BindingRuntimeKind,
    BindingTable, ScopeGraph, ScopeId, ShadowStack,
};

/// Declare the IMPLICIT `$:` assignment-target bindings at the instance root
/// scope, returning the synthesized names in source order (deduplicated).
///
/// Mirrors the official scope pass: for every TOP-LEVEL `$:` labeled statement
/// whose body is an expression statement over an `AssignmentExpression` (ANY
/// operator; a parenthesized wrapper is transparent — `$: (y = x + 1)` declares
/// exactly like the bare form), each bound identifier of the target pattern (a
/// bare identifier or a destructure pattern's bound names — a member target
/// binds nothing) that is NOT `$`-prefixed and does NOT already resolve at the
/// root scope is declared as a [`BindingRuntimeKind::MutableSource`] binding. The caller gates this on
/// the FINAL `SvelteMode::Legacy`; it is never run for a runes component.
pub(super) fn declare_reactive_assignment_targets(
    instance_source: Option<&str>,
    alloc: &Allocator,
    root_scope: ScopeId,
    scopes: &mut ScopeGraph,
    bindings: &mut BindingTable,
) -> Vec<String> {
    let Some(instance) = instance_source else {
        return Vec::new();
    };
    let Some(program) = reparse_module(alloc, instance) else {
        return Vec::new();
    };
    let mut synthesized = Vec::new();
    for stmt in &program.body {
        let Statement::LabeledStatement(labeled) = stmt else {
            continue;
        };
        if labeled.label.name != "$" {
            continue;
        }
        let Statement::ExpressionStatement(expr_stmt) = &labeled.body else {
            continue;
        };
        // A parenthesized assignment (`$: (y = x + 1)`) is the same implicit
        // declaration — standard JS paren transparency (official's acorn AST
        // carries no paren nodes, so its scope pass sees the assignment
        // directly).
        let Expression::AssignmentExpression(assign) = peel_parens(&expr_stmt.expression) else {
            continue;
        };
        let mut names = Vec::new();
        collect_bound_target_names(&assign.left, &mut names);
        for name in names {
            if name.starts_with('$') {
                continue;
            }
            if scopes.resolve(bindings, root_scope, &name).is_some() {
                continue;
            }
            let id = bindings.push(BindingInfo {
                name: name.clone(),
                scope: root_scope,
                kind: BindingRuntimeKind::MutableSource,
                state: None,
            });
            scopes.declare(root_scope, &name, id);
            synthesized.push(name);
        }
    }
    synthesized
}

/// Collect the BOUND identifier names of an assignment target — the official
/// target-pattern identifier extraction: a bare identifier binds itself; an
/// object/array pattern binds its nested identifier elements (a default binds
/// its LEFT, a rest binds its inner target); a MEMBER target binds nothing.
fn collect_bound_target_names(target: &AssignmentTarget<'_>, out: &mut Vec<String>) {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => out.push(id.name.to_string()),
        AssignmentTarget::ObjectAssignmentTarget(obj) => collect_object_target_names(obj, out),
        AssignmentTarget::ArrayAssignmentTarget(arr) => collect_array_target_names(arr, out),
        // A member (or TS-wrapped) target binds no name.
        _ => {}
    }
}

/// The object-pattern half of [`collect_bound_target_names`].
fn collect_object_target_names(obj: &ObjectAssignmentTarget<'_>, out: &mut Vec<String>) {
    for prop in &obj.properties {
        match prop {
            AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                out.push(p.binding.name.to_string());
            }
            AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                collect_maybe_default_target_names(&p.binding, out);
            }
        }
    }
    if let Some(rest) = &obj.rest {
        collect_bound_target_names(&rest.target, out);
    }
}

/// The array-pattern half of [`collect_bound_target_names`].
fn collect_array_target_names(arr: &ArrayAssignmentTarget<'_>, out: &mut Vec<String>) {
    for element in arr.elements.iter().flatten() {
        collect_maybe_default_target_names(element, out);
    }
    if let Some(rest) = &arr.rest {
        collect_bound_target_names(&rest.target, out);
    }
}

/// A pattern element with an optional default — the default's LEFT binds.
fn collect_maybe_default_target_names(
    element: &AssignmentTargetMaybeDefault<'_>,
    out: &mut Vec<String>,
) {
    match element {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
            collect_bound_target_names(&with_default.binding, out);
        }
        other => {
            if let Some(target) = other.as_assignment_target() {
                collect_bound_target_names(target, out);
            }
        }
    }
}

/// The typed BODY shape of a `$:` reactive statement — the effect-thunk
/// payload classification (from the labeled statement's typed body, never a
/// text sniff).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ReactiveStatementBody {
    /// `$:;` — an empty statement: the effect body is the empty thunk
    /// (`() => {}`), matching the official lowering of the empty label.
    Empty,
    /// `$: { … }` — a block body: the block is the arrow body VERBATIM
    /// (rewritten), never re-wrapped in a second block.
    Block {
        /// The block's source text, braces included.
        source: String,
    },
    /// Any other single statement (an expression statement — assignment or
    /// call — an `if`, a loop): wrapped in a block as the arrow body,
    /// rewritten verbatim.
    Statement {
        /// The statement's source text.
        source: String,
    },
}

/// Classify a `$:` labeled statement's BODY into its typed
/// [`ReactiveStatementBody`] shape, slicing the body source from the instance
/// script (the shape decision is the typed statement kind, never a text sniff).
pub(super) fn classify_reactive_statement_body(
    body: &Statement<'_>,
    instance_source: &str,
) -> ReactiveStatementBody {
    use oxc_span::GetSpan;
    let span = body.span();
    let source = instance_source
        .get(span.start as usize..span.end as usize)
        .unwrap_or_default()
        .to_string();
    match body {
        Statement::EmptyStatement(_) => ReactiveStatementBody::Empty,
        Statement::BlockStatement(_) => ReactiveStatementBody::Block { source },
        _ => ReactiveStatementBody::Statement { source },
    }
}

/// The per-statement `$:` facts: dependency-candidate names and assigned names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReactiveStatementFacts {
    /// Referenced names in FIRST-MENTION order, excluding a name whose EVERY
    /// mention is a pure `=`-assignment TARGET position (the official
    /// walk-up-through-member-expressions skip). Shadow-pruned: a mention
    /// shadowed by a body-local binding never counts. Resolution to a binding
    /// kind (and the plain-local / global exclusion) happens at lowering.
    pub(super) deps: Vec<String>,
    /// The assigned names — the official assignment/update target extraction:
    /// a bare identifier or destructure-bound name for an assignment (any
    /// operator), the identifier or member ROOT for an update. Deduplicated,
    /// first-seen order.
    pub(super) assignments: Vec<String>,
}

/// Collect the [`ReactiveStatementFacts`] of one `$:` labeled statement's BODY
/// from its typed OXC AST.
pub(super) fn reactive_statement_facts(body: &Statement<'_>) -> ReactiveStatementFacts {
    let mut scan = ReactiveFactsScan {
        scopes: ShadowStack::default(),
        mention_order: Vec::new(),
        has_dep_mention: FxHashMap::default(),
        assignments: Vec::new(),
        assignment_set: FxHashSet::default(),
    };
    scan.visit_statement(body);
    ReactiveStatementFacts {
        deps: scan
            .mention_order
            .into_iter()
            .filter(|name| scan.has_dep_mention.get(name).copied().unwrap_or(false))
            .collect(),
        assignments: scan.assignments,
    }
}

/// The scope-aware mention/assignment scan over a `$:` statement body.
///
/// A MENTION is an unshadowed identifier reference; it is a DEP mention unless
/// it sits in a pure `=`-assignment TARGET position after walking up through
/// consecutive member expressions (the official skip: `y = …` skips `y`;
/// `obj.v = …` / `obj[k] = …` skip `obj` AND a directly-computed `k`; a
/// compound `y += …`, an update `y++`, and a destructure target `({a} = …)` do
/// NOT skip). Mention ORDER is first-mention order INCLUDING skipped target
/// mentions (the official scope-references insertion order — a name first seen
/// as a write still orders by that first sighting). Scope frames mirror the
/// shared lexical model ([`ShadowStack`]): function/arrow bodies, blocks,
/// `for` / `for..in` / `for..of` head bindings, and `catch` params — a head
/// binding shadows across its whole statement, so a loop/catch local of an
/// outer reactive name never records the outer dependency.
struct ReactiveFactsScan {
    scopes: ShadowStack,
    mention_order: Vec<String>,
    has_dep_mention: FxHashMap<String, bool>,
    assignments: Vec<String>,
    assignment_set: FxHashSet<String>,
}

impl ReactiveFactsScan {
    /// Record one identifier mention. `dep` is false for a pure
    /// `=`-assignment-target-chain position.
    fn mention(&mut self, name: &str, dep: bool) {
        if self.scopes.is_shadowed(name) {
            return;
        }
        match self.has_dep_mention.get_mut(name) {
            Some(existing) => *existing = *existing || dep,
            None => {
                self.mention_order.push(name.to_string());
                self.has_dep_mention.insert(name.to_string(), dep);
            }
        }
    }

    /// Record one assigned name (deduplicated, shadow-pruned).
    fn assignment(&mut self, name: &str) {
        if self.scopes.is_shadowed(name) {
            return;
        }
        if self.assignment_set.insert(name.to_string()) {
            self.assignments.push(name.to_string());
        }
    }

    /// Scan an expression on the assignment-target MEMBER CHAIN: an identifier
    /// whose parent chain up to the assignment consists solely of member
    /// expressions (the chain root and a directly-computed property
    /// identifier) records with the chain's `dep` polarity; anything nested
    /// deeper (a call argument, a binary operand) leaves the chain and scans
    /// normally.
    fn scan_member_chain(&mut self, expr: &Expression<'_>, dep: bool) {
        match expr {
            Expression::Identifier(id) => self.mention(id.name.as_str(), dep),
            Expression::StaticMemberExpression(m) => self.scan_member_chain(&m.object, dep),
            Expression::ComputedMemberExpression(m) => {
                self.scan_member_chain(&m.object, dep);
                self.scan_member_chain(&m.expression, dep);
            }
            Expression::PrivateFieldExpression(m) => self.scan_member_chain(&m.object, dep),
            // The official (acorn) AST carries no parenthesized/TS-skin nodes —
            // they are transparent on the chain.
            Expression::ParenthesizedExpression(p) => self.scan_member_chain(&p.expression, dep),
            Expression::TSNonNullExpression(e) => self.scan_member_chain(&e.expression, dep),
            Expression::TSAsExpression(e) => self.scan_member_chain(&e.expression, dep),
            Expression::TSSatisfiesExpression(e) => self.scan_member_chain(&e.expression, dep),
            // Off the member chain: normal mention semantics.
            other => self.visit_expression(other),
        }
    }

    /// Scan a simple-assignment-target member/TS form's parts on the member
    /// chain (`dep` decides whether the chain identifiers are dep mentions).
    fn scan_simple_target_parts(&mut self, target: &SimpleAssignmentTarget<'_>, dep: bool) {
        match target {
            SimpleAssignmentTarget::ComputedMemberExpression(m) => {
                self.scan_member_chain(&m.object, dep);
                self.scan_member_chain(&m.expression, dep);
            }
            SimpleAssignmentTarget::StaticMemberExpression(m) => {
                self.scan_member_chain(&m.object, dep);
            }
            SimpleAssignmentTarget::PrivateFieldExpression(m) => {
                self.scan_member_chain(&m.object, dep);
            }
            SimpleAssignmentTarget::TSAsExpression(e) => self.scan_member_chain(&e.expression, dep),
            SimpleAssignmentTarget::TSSatisfiesExpression(e) => {
                self.scan_member_chain(&e.expression, dep);
            }
            SimpleAssignmentTarget::TSNonNullExpression(e) => {
                self.scan_member_chain(&e.expression, dep);
            }
            SimpleAssignmentTarget::TSTypeAssertion(e) => {
                self.scan_member_chain(&e.expression, dep);
            }
            SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                self.mention(id.name.as_str(), dep);
            }
        }
    }

    /// Record an assignment TARGET: bound-name assignments, target-position
    /// mentions, and the member-chain skip for a pure `=` member target.
    fn record_assignment_target(&mut self, target: &AssignmentTarget<'_>, pure_assign: bool) {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(id) => {
                // A bare identifier target: a dep mention only under a compound
                // operator (which reads the previous value).
                self.mention(id.name.as_str(), !pure_assign);
                self.assignment(id.name.as_str());
            }
            AssignmentTarget::ObjectAssignmentTarget(obj) => self.record_object_target(obj),
            AssignmentTarget::ArrayAssignmentTarget(arr) => self.record_array_target(arr),
            other => {
                // A member (or TS-wrapped member) target: no bound name; the
                // chain identifiers are dep mentions ONLY when the operator
                // reads (a compound assignment) — a pure `=` member target's
                // chain is the official skip.
                if let Some(simple) = other.as_simple_assignment_target() {
                    self.scan_simple_target_parts(simple, !pure_assign);
                }
            }
        }
    }

    /// A destructure OBJECT target: bound names are assignments AND dep
    /// mentions (their walk-up hits the pattern, not the assignment); member
    /// elements and defaults scan as normal reads.
    fn record_object_target(&mut self, obj: &ObjectAssignmentTarget<'_>) {
        for prop in &obj.properties {
            match prop {
                AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                    self.mention(p.binding.name.as_str(), true);
                    self.assignment(p.binding.name.as_str());
                    if let Some(init) = &p.init {
                        self.visit_expression(init);
                    }
                }
                AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                    // A COMPUTED key is a live expression; a static key is not
                    // a reference.
                    if p.computed {
                        if let Some(key_expr) = p.name.as_expression() {
                            self.visit_expression(key_expr);
                        }
                    }
                    self.record_maybe_default_target(&p.binding);
                }
            }
        }
        if let Some(rest) = &obj.rest {
            self.record_assignment_target(&rest.target, false);
        }
    }

    /// The array half of [`Self::record_object_target`].
    fn record_array_target(&mut self, arr: &ArrayAssignmentTarget<'_>) {
        for element in arr.elements.iter().flatten() {
            self.record_maybe_default_target(element);
        }
        if let Some(rest) = &arr.rest {
            self.record_assignment_target(&rest.target, false);
        }
    }

    /// A pattern element with an optional default: the binding records as a
    /// destructure target (dep mention + assignment for an identifier), the
    /// default expression scans normally.
    fn record_maybe_default_target(&mut self, element: &AssignmentTargetMaybeDefault<'_>) {
        match element {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
                self.record_assignment_target(&with_default.binding, false);
                self.visit_expression(&with_default.init);
            }
            other => {
                if let Some(target) = other.as_assignment_target() {
                    self.record_assignment_target(target, false);
                }
            }
        }
    }
}

impl<'a> Visit<'a> for ReactiveFactsScan {
    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.mention(it.name.as_str(), true);
        walk::walk_identifier_reference(self, it);
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        let pure = it.operator == AssignmentOperator::Assign;
        self.record_assignment_target(&it.left, pure);
        self.visit_expression(&it.right);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        match &it.argument {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                // An update both reads and writes: a dep mention + assignment.
                self.mention(id.name.as_str(), true);
                self.assignment(id.name.as_str());
            }
            other => {
                // A member update (`obj.v++`): every chain identifier is a dep
                // mention (the update reads), and the ROOT identifier joins the
                // assignments (the official member-root extraction).
                if let Some(root) = member_target_root_name(other) {
                    self.assignment(root);
                }
                self.scan_simple_target_parts(other, true);
            }
        }
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.scopes.push(function_scope_names(it));
        walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.scopes.push(arrow_scope_names(it));
        walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &BlockStatement<'a>) {
        self.scopes.push(block_scope_names(it));
        walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        let mut frame = FxHashSet::default();
        if let Some(param) = &it.param {
            let mut names = Vec::new();
            collect_pattern_names(&param.pattern, &mut names);
            frame.extend(names);
        }
        self.scopes.push(frame);
        walk::walk_catch_clause(self, it);
        self.scopes.pop();
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        let mut frame = FxHashSet::default();
        if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) = &it.init {
            if !matches!(decl.kind, VariableDeclarationKind::Var) {
                for d in &decl.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    frame.extend(names);
                }
            }
        }
        self.scopes.push(frame);
        walk::walk_for_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_of_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_in_statement(self, it);
        self.scopes.pop();
    }
}

/// The ROOT identifier name of a member-form simple assignment target (the
/// official member-object root walk), or `None` for a non-member form.
fn member_target_root_name<'a, 'b>(target: &'b SimpleAssignmentTarget<'a>) -> Option<&'b str> {
    let mut expr: &Expression<'a> = match target {
        SimpleAssignmentTarget::ComputedMemberExpression(m) => &m.object,
        SimpleAssignmentTarget::StaticMemberExpression(m) => &m.object,
        SimpleAssignmentTarget::PrivateFieldExpression(m) => &m.object,
        _ => return None,
    };
    loop {
        match expr {
            Expression::Identifier(id) => return Some(id.name.as_str()),
            Expression::StaticMemberExpression(m) => expr = &m.object,
            Expression::ComputedMemberExpression(m) => expr = &m.object,
            Expression::PrivateFieldExpression(m) => expr = &m.object,
            Expression::ParenthesizedExpression(p) => expr = &p.expression,
            Expression::TSNonNullExpression(e) => expr = &e.expression,
            Expression::TSAsExpression(e) => expr = &e.expression,
            _ => return None,
        }
    }
}

/// One row of the registration-order input: a statement's dependency names and
/// assigned names (from [`ReactiveStatementFacts`]).
pub(super) struct ReactiveOrderRow<'a> {
    /// The statement's dependency-candidate names, in mention order.
    pub(super) deps: &'a [String],
    /// The statement's assigned names.
    pub(super) assignments: &'a [String],
}

/// Topologically order the reactive-statement registrations, mirroring the
/// official ordering walk: a statement that ASSIGNS a name registers before
/// every statement that DEPENDS on that name, with source order as the
/// tie-break. Returns the registration order as indices into `rows`, or —
/// when the name-edge graph is cyclic — the index of the statement the
/// official cycle error blames (the first statement assigning the cycle
/// walk's root name). A dependency a statement itself assigns contributes no
/// edge (a self-dependent `$: x = x + 1` is not a cycle).
pub(super) fn order_reactive_registrations(
    rows: &[ReactiveOrderRow<'_>],
) -> Result<Vec<usize>, usize> {
    // The assigner lookup: name → statements assigning it, source order.
    let mut lookup: FxHashMap<&str, Vec<usize>> = FxHashMap::default();
    for (i, row) in rows.iter().enumerate() {
        for name in row.assignments {
            lookup.entry(name.as_str()).or_default().push(i);
        }
    }

    // The name-edge graph, insertion-ordered (one node per name, one edge per
    // (assignment, non-self dependency) pair).
    let mut node_index: FxHashMap<&str, usize> = FxHashMap::default();
    let mut node_names: Vec<&str> = Vec::new();
    let mut adjacency: Vec<Vec<usize>> = Vec::new();
    for row in rows {
        for assignment in row.assignments {
            for dep in row.deps {
                if row.assignments.iter().any(|a| a == dep) {
                    continue;
                }
                let u = intern_node(assignment, &mut node_index, &mut node_names, &mut adjacency);
                let v = intern_node(dep, &mut node_index, &mut node_names, &mut adjacency);
                adjacency[u].push(v);
            }
        }
    }

    // The official cycle walk: DFS in graph insertion order with an ordered
    // on-stack path; the blame name is the FIRST element of the path that
    // closed a cycle (the walk's root).
    let mut visited = vec![false; node_names.len()];
    let mut on_stack: Vec<usize> = Vec::new();
    let mut on_stack_set = vec![false; node_names.len()];
    let mut cycle_root: Option<usize> = None;
    for v in 0..node_names.len() {
        if cycle_root.is_some() {
            break;
        }
        if !visited[v] {
            visit_cycle(
                v,
                &adjacency,
                &mut visited,
                &mut on_stack,
                &mut on_stack_set,
                &mut cycle_root,
            );
        }
    }
    if let Some(root) = cycle_root {
        let name = node_names[root];
        let blame = lookup
            .get(name)
            .and_then(|assigners| assigners.first())
            .copied()
            .unwrap_or(0);
        return Err(blame);
    }

    // The official placement walk: a statement places AFTER every assigner of
    // its non-self dependencies, source order as the tie-break.
    let mut ordered: Vec<usize> = Vec::with_capacity(rows.len());
    let mut placed = vec![false; rows.len()];
    let mut visiting = vec![false; rows.len()];
    for i in 0..rows.len() {
        place(i, rows, &lookup, &mut ordered, &mut placed, &mut visiting);
    }
    Ok(ordered)
}

/// Intern a graph node by name, preserving insertion order.
fn intern_node<'a>(
    name: &'a str,
    node_index: &mut FxHashMap<&'a str, usize>,
    node_names: &mut Vec<&'a str>,
    adjacency: &mut Vec<Vec<usize>>,
) -> usize {
    if let Some(&idx) = node_index.get(name) {
        return idx;
    }
    let idx = node_names.len();
    node_index.insert(name, idx);
    node_names.push(name);
    adjacency.push(Vec::new());
    idx
}

/// The recursive cycle-walk step (visited/on-stack DFS).
fn visit_cycle(
    v: usize,
    adjacency: &[Vec<usize>],
    visited: &mut [bool],
    on_stack: &mut Vec<usize>,
    on_stack_set: &mut [bool],
    cycle_root: &mut Option<usize>,
) {
    visited[v] = true;
    on_stack.push(v);
    on_stack_set[v] = true;
    for &w in &adjacency[v] {
        if cycle_root.is_some() {
            break;
        }
        if !visited[w] {
            visit_cycle(w, adjacency, visited, on_stack, on_stack_set, cycle_root);
        } else if on_stack_set[w] {
            *cycle_root = Some(on_stack[0]);
        }
    }
    on_stack.pop();
    on_stack_set[v] = false;
}

/// The recursive placement step: place `i`'s dependency assigners, then `i`.
/// The `visiting` guard keeps a (pre-rejected) cyclic input terminating.
fn place(
    i: usize,
    rows: &[ReactiveOrderRow<'_>],
    lookup: &FxHashMap<&str, Vec<usize>>,
    ordered: &mut Vec<usize>,
    placed: &mut [bool],
    visiting: &mut [bool],
) {
    if placed[i] || visiting[i] {
        return;
    }
    visiting[i] = true;
    for dep in rows[i].deps {
        if rows[i].assignments.iter().any(|a| a == dep) {
            continue;
        }
        if let Some(assigners) = lookup.get(dep.as_str()) {
            for &assigner in assigners {
                place(assigner, rows, lookup, ordered, placed, visiting);
            }
        }
    }
    visiting[i] = false;
    placed[i] = true;
    ordered.push(i);
}
