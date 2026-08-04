//! Demand-sliced flow substrate: the arena-free [`FunctionBodySkeleton`] and
//! the per-function [`flow_graph::FunctionFlowGraph`] built from it.
//!
//! The skeleton is a SHALLOW structural index over one authored function
//! body: a statement / control-region skeleton, a return-site index, a
//! lexical binding index, and an assignment / kill summary, plus per-site
//! read / write / call footprints and object-literal property footprints.
//! It is built once per function content version from the retained parse
//! snapshot and never rebuilt per query or demand; it borrows no OXC node
//! (`Send + Sync + 'static`) and performs NO type lowering — every stored
//! leaf is an interned name, ordinal, span, or id, certified transitively
//! `TypeExpr`-free by the `NoTypeExpr` marker on every carrier.
//!
//! The [`flow_graph::FunctionFlowGraph`] is the sparse typed-edge dependence
//! structure the flow demand planner computes reachability over; it is built
//! from the skeleton ALONE ([`flow_graph::build_function_flow_graph`] takes
//! only `&FunctionBodySkeleton`), so a graph build can never re-walk the AST
//! or observe a query demand.
//!
//! On top of the graph: [`peeker::ReturnPathPeeker`] plans a demand slice
//! as graph reachability (the two-frontier rule as edge classes) into a
//! [`flow_ir::ReturnSlicePlan`]; [`hashing::compute_flow_slice_hash`]
//! folds exactly that selected subgraph into the opaque
//! [`hashing::FlowSliceHash`]; and [`lower::lower_slice_plan`] lowers only
//! the plan into the arena-free [`flow_ir::FlowSliceIR`].

use std::sync::Arc;

use oxc_ast::ast::{
    ArrowFunctionExpression, AssignmentTarget, AssignmentTargetMaybeDefault,
    AssignmentTargetProperty, BindingPattern, Expression, Function, ObjectExpression,
    ObjectPropertyKind, SimpleAssignmentTarget, Statement,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use rustc_hash::FxHashMap;
use verter_no_typeexpr::NoTypeExpr;

use crate::analysis::function_program::static_property_key_name;

pub mod flow_graph;
pub mod flow_ir;
pub mod hashing;
pub mod lower;
pub mod peeker;

#[cfg(test)]
#[path = "skeleton_tests.rs"]
mod skeleton_tests;

// ---------------------------------------------------------------------------
// Ids
// ---------------------------------------------------------------------------

/// Interned identifier / property-key name within one skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct FlowNameId(u32);

impl FlowNameId {
    /// Index into [`FunctionBodySkeleton::names`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// One entry of the lexical binding index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SkeletonBindingId(u32);

impl SkeletonBindingId {
    /// Index into [`FunctionBodySkeleton::bindings`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// One control region of the statement skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SkeletonRegionId(u32);

impl SkeletonRegionId {
    /// Index into [`FunctionBodySkeleton::regions`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// One tracked expression site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SkeletonExprSiteId(u32);

impl SkeletonExprSiteId {
    /// Index into [`FunctionBodySkeleton::expr_sites`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// One `return` site of the indexed function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SkeletonReturnSiteId(u32);

impl SkeletonReturnSiteId {
    /// Index into [`FunctionBodySkeleton::return_sites`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

// ---------------------------------------------------------------------------
// Regions
// ---------------------------------------------------------------------------

/// The kind of one control region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SkeletonRegionKind {
    /// The function body itself — the root region.
    FunctionBody,
    /// A block statement.
    Block,
    /// The consequent arm of an `if`.
    IfConsequent,
    /// The alternate arm of an `if`.
    IfAlternate,
    /// A loop body (`for` / `for-in` / `for-of` / `while` / `do-while`).
    Loop,
    /// A `switch` statement.
    Switch,
    /// One `case` / `default` arm of a `switch`.
    SwitchCase,
    /// The `try` block of a `try` statement.
    TryBlock,
    /// A `catch` clause.
    CatchClause,
    /// A `finally` block.
    FinallyBlock,
    /// The body of a labeled statement.
    LabeledBody,
}

/// One control region: kind, parent nesting, the controlling expression
/// site (an `if` / loop condition or `switch` discriminant), whether the
/// region's statement subtree returns from the indexed function, and the
/// region's span.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonRegion {
    /// The region kind.
    pub kind: SkeletonRegionKind,
    /// The enclosing region (`None` for the function-body root).
    pub parent: Option<SkeletonRegionId>,
    /// The controlling expression site, when the region is predicated.
    pub control_input: Option<SkeletonExprSiteId>,
    /// Whether the region's subtree contains a `return` of the indexed
    /// function (nested function bodies never contribute).
    pub has_return: bool,
    /// The region statement's span.
    pub span: verter_span::Span,
}

// ---------------------------------------------------------------------------
// Bindings
// ---------------------------------------------------------------------------

/// The kind of one lexical binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SkeletonBindingKind {
    /// A formal parameter (destructured identifiers included).
    Param,
    /// A `const` declarator.
    Const,
    /// A `let` declarator.
    Let,
    /// A `var` / `using` declarator.
    Var,
    /// A nested function declaration's name.
    NestedFunction,
    /// A local class declaration's name.
    Class,
    /// A `catch` clause parameter.
    CatchParam,
}

/// One entry of the lexical binding index.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonBinding {
    /// The binding name.
    pub name: FlowNameId,
    /// The binding kind.
    pub kind: SkeletonBindingKind,
    /// The region the binding is declared in.
    pub region: SkeletonRegionId,
    /// The binding identifier's span.
    pub span: verter_span::Span,
    /// The declarator initializer / parameter default site, when present.
    pub initializer: Option<SkeletonExprSiteId>,
}

// ---------------------------------------------------------------------------
// Expression sites
// ---------------------------------------------------------------------------

/// One identifier read inside a site (child sites and nested function
/// bodies excluded — their reads belong to their own site / frame).
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonRead {
    /// The read name (a local slot when the name binds in this frame,
    /// otherwise a free / captured name).
    pub name: FlowNameId,
    /// The reference span.
    pub span: verter_span::Span,
}

/// The callee shape of one call / construct site.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum SkeletonCallee {
    /// A bare identifier callee (`g()`).
    Named(FlowNameId),
    /// A static member path rooted at an identifier (`a.b.c()`), root
    /// first.
    Path(Arc<[FlowNameId]>),
    /// Any other callee shape (computed, call-result, `this`-rooted).
    Opaque,
}

/// One call / construct footprint entry.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonCall {
    /// The callee shape.
    pub callee: SkeletonCallee,
    /// Whether this is a `new` construct site.
    pub new_construct: bool,
    /// The call expression's span.
    pub span: verter_span::Span,
}

/// The key of one object-literal property entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, NoTypeExpr)]
pub enum SkeletonObjectKey {
    /// A statically-known property key.
    Static(FlowNameId),
    /// A computed key; the key expression is its own child site (its
    /// evaluation effects survive independently of the named value).
    Computed(SkeletonExprSiteId),
}

/// The authored kind of one object-literal property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SkeletonPropertyKind {
    /// A plain `key: value` initializer.
    Init,
    /// A method shorthand (`m() {}`).
    Method,
    /// A `get` / `set` accessor.
    Accessor,
}

/// One object-literal entry of a site's shape footprint.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum SkeletonObjectEntry {
    /// A property provisioning one key.
    Property {
        /// The property key.
        key: SkeletonObjectKey,
        /// The property value's child site.
        value: SkeletonExprSiteId,
        /// The authored property kind.
        kind: SkeletonPropertyKind,
    },
    /// A spread entry (`...src`) — an optional / unknown write of every
    /// key, whose source evaluation effect survives a later definite
    /// write.
    Spread {
        /// The spread source's child site.
        source: SkeletonExprSiteId,
    },
}

/// The recorded shape of one expression site.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum SkeletonExprShape {
    /// An object literal with its property footprint, in authored order.
    ObjectLiteral {
        /// The entries in authored order.
        entries: Arc<[SkeletonObjectEntry]>,
    },
    /// Any other expression shape (footprint-only).
    Other,
}

/// One tracked expression site: span, region membership, containment
/// parent, shape, and the read / call footprint attributed to this site
/// (child sites carry their own).
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonExprSite {
    /// The expression's span.
    pub span: verter_span::Span,
    /// The region the site evaluates in.
    pub region: SkeletonRegionId,
    /// The containing site (`None` for a root site owned by a statement,
    /// declarator, return, or control input).
    pub parent: Option<SkeletonExprSiteId>,
    /// The recorded expression shape.
    pub shape: SkeletonExprShape,
    /// Identifier reads attributed to this site.
    pub reads: Arc<[SkeletonRead]>,
    /// Call / construct footprints attributed to this site.
    pub calls: Arc<[SkeletonCall]>,
}

// ---------------------------------------------------------------------------
// Writes (assignment / kill summary)
// ---------------------------------------------------------------------------

/// The root target of one write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, NoTypeExpr)]
pub enum SkeletonWriteTarget {
    /// A named root (a local slot when the name binds in this frame,
    /// otherwise a free name).
    Named(FlowNameId),
    /// An unresolvable target root (call result, `this`, computed root).
    Opaque,
}

/// One segment of a write's projection path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SkeletonPathSegment {
    /// A statically-known property key.
    Static(FlowNameId),
    /// A computed / unknown key.
    Computed,
}

/// Whether a write definitely happens when its site evaluates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SkeletonWriteCertainty {
    /// The write happens whenever the site evaluates.
    Definite,
    /// The write is conditional on the site's own evaluation (logical
    /// assignment, iteration-provided values).
    Optional,
}

/// One write of the assignment / kill summary, in source order.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonWrite {
    /// The write's root target.
    pub target: SkeletonWriteTarget,
    /// The projection path under the root (empty = whole-slot write).
    pub path: Arc<[SkeletonPathSegment]>,
    /// Whether the write definitely happens when the site evaluates.
    pub certainty: SkeletonWriteCertainty,
    /// The site providing the written value (`None` for self-referential
    /// update writes like `x++`).
    pub value: Option<SkeletonExprSiteId>,
    /// The tracked site whose evaluation performs the write.
    pub site: SkeletonExprSiteId,
    /// The region the write evaluates in.
    pub region: SkeletonRegionId,
    /// The write expression's span.
    pub span: verter_span::Span,
}

// ---------------------------------------------------------------------------
// Return sites
// ---------------------------------------------------------------------------

/// One `return` site of the indexed function, in source order.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonReturnSite {
    /// Source-order ordinal.
    pub ordinal: u32,
    /// The region the return site evaluates in.
    pub region: SkeletonRegionId,
    /// The returned expression's site (`None` for bare `return;`).
    pub argument: Option<SkeletonExprSiteId>,
    /// Whether the site is the implicit return of an expression-bodied
    /// arrow.
    pub implicit: bool,
    /// The return statement's span.
    pub span: verter_span::Span,
}

// ---------------------------------------------------------------------------
// The skeleton
// ---------------------------------------------------------------------------

/// The arena-free shallow skeleton of one authored function body: the
/// statement / control-region skeleton, the return-site index, the lexical
/// binding index, the assignment / kill summary, and per-site read / write /
/// call / object-shape footprints.
///
/// Built once per function content version from the retained parse
/// snapshot; never rebuilt per query or demand. Stores NO lowered type —
/// every leaf is an interned name, ordinal, span, or id.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct FunctionBodySkeleton {
    /// The interned name table.
    pub names: Arc<[Arc<str>]>,
    /// The control regions; index 0 is the function-body root.
    pub regions: Arc<[SkeletonRegion]>,
    /// The lexical binding index.
    pub bindings: Arc<[SkeletonBinding]>,
    /// The tracked expression sites (parents precede their children).
    pub expr_sites: Arc<[SkeletonExprSite]>,
    /// The return-site index, in source order.
    pub return_sites: Arc<[SkeletonReturnSite]>,
    /// The assignment / kill summary, in source order.
    pub writes: Arc<[SkeletonWrite]>,
}

impl FunctionBodySkeleton {
    /// The interned text of `name`.
    #[must_use]
    pub fn name(&self, name: FlowNameId) -> &str {
        &self.names[name.index()]
    }

    /// The id of an interned name, when present.
    #[must_use]
    pub fn name_id(&self, text: &str) -> Option<FlowNameId> {
        self.names
            .iter()
            .position(|candidate| candidate.as_ref() == text)
            .and_then(|index| u32::try_from(index).ok())
            .map(FlowNameId)
    }

    /// Every binding of `name` in this frame, in declaration order.
    pub fn bindings_named(&self, name: FlowNameId) -> impl Iterator<Item = SkeletonBindingId> + '_ {
        self.bindings
            .iter()
            .enumerate()
            .filter(move |(_, binding)| binding.name == name)
            .filter_map(|(index, _)| u32::try_from(index).ok().map(SkeletonBindingId))
    }

    /// The region record for `id`.
    #[must_use]
    pub fn region(&self, id: SkeletonRegionId) -> &SkeletonRegion {
        &self.regions[id.index()]
    }

    /// The binding record for `id`.
    #[must_use]
    pub fn binding(&self, id: SkeletonBindingId) -> &SkeletonBinding {
        &self.bindings[id.index()]
    }

    /// The expression-site record for `id`.
    #[must_use]
    pub fn expr_site(&self, id: SkeletonExprSiteId) -> &SkeletonExprSite {
        &self.expr_sites[id.index()]
    }

    /// The return-site record for `id`.
    #[must_use]
    pub fn return_site(&self, id: SkeletonReturnSiteId) -> &SkeletonReturnSite {
        &self.return_sites[id.index()]
    }
}

// ---------------------------------------------------------------------------
// Producer input
// ---------------------------------------------------------------------------

/// The authored positions one skeleton is built from: formal parameters
/// plus the body statement list of exactly one function / arrow, borrowed
/// from the retained parse snapshot for the duration of the build only.
pub struct FunctionBodySource<'a, 'ast> {
    /// The formal parameters.
    pub params: &'a oxc_ast::ast::FormalParameters<'ast>,
    /// The body statements.
    pub statements: &'a [Statement<'ast>],
    /// Whether the body is an expression-bodied arrow (the single
    /// expression statement is the implicit return).
    pub expression_body: bool,
    /// The body span.
    pub body_span: verter_span::Span,
}

impl<'a, 'ast> FunctionBodySource<'a, 'ast> {
    /// The source positions of a bodied function declaration / expression
    /// (`None` for a bodiless overload signature).
    #[must_use]
    pub fn from_function(function: &'a Function<'ast>) -> Option<Self> {
        let body = function.body.as_ref()?;
        Some(Self {
            params: &function.params,
            statements: &body.statements,
            expression_body: false,
            body_span: body.span.into(),
        })
    }

    /// The source positions of an arrow function expression.
    #[must_use]
    pub fn from_arrow(arrow: &'a ArrowFunctionExpression<'ast>) -> Self {
        Self {
            params: &arrow.params,
            statements: &arrow.body.statements,
            expression_body: arrow.expression,
            body_span: arrow.body.span.into(),
        }
    }
}

/// Build the [`FunctionBodySkeleton`] of one authored function body. One
/// structural walk; no type lowering, no resolution, no fact production —
/// nested function / arrow / class bodies are never entered (they are their
/// own frames).
#[must_use]
pub fn build_function_body_skeleton(source: &FunctionBodySource<'_, '_>) -> FunctionBodySkeleton {
    let mut builder = SkeletonBuilder::new(source.body_span);
    builder.collect_params(source.params);
    if source.expression_body {
        if let [Statement::ExpressionStatement(statement)] = source.statements {
            let argument = builder.open_root_site(&statement.expression);
            builder.push_implicit_return(argument, statement.span.into());
            return builder.finish();
        }
    }
    for statement in source.statements {
        builder.visit_statement(statement);
    }
    builder.finish()
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

struct SiteDraft {
    span: verter_span::Span,
    region: SkeletonRegionId,
    parent: Option<SkeletonExprSiteId>,
    shape: SkeletonExprShape,
    reads: Vec<SkeletonRead>,
    calls: Vec<SkeletonCall>,
}

struct SkeletonBuilder {
    names: Vec<Arc<str>>,
    name_lookup: FxHashMap<Arc<str>, FlowNameId>,
    regions: Vec<SkeletonRegion>,
    region_stack: Vec<usize>,
    bindings: Vec<SkeletonBinding>,
    sites: Vec<SiteDraft>,
    site_stack: Vec<usize>,
    return_sites: Vec<SkeletonReturnSite>,
    writes: Vec<SkeletonWrite>,
}

impl SkeletonBuilder {
    fn new(body_span: verter_span::Span) -> Self {
        let root = SkeletonRegion {
            kind: SkeletonRegionKind::FunctionBody,
            parent: None,
            control_input: None,
            has_return: false,
            span: body_span,
        };
        Self {
            names: Vec::new(),
            name_lookup: FxHashMap::default(),
            regions: vec![root],
            region_stack: vec![0],
            bindings: Vec::new(),
            sites: Vec::new(),
            site_stack: Vec::new(),
            return_sites: Vec::new(),
            writes: Vec::new(),
        }
    }

    fn intern(&mut self, text: &str) -> FlowNameId {
        if let Some(id) = self.name_lookup.get(text) {
            return *id;
        }
        let interned: Arc<str> = Arc::from(text);
        let id = FlowNameId(u32::try_from(self.names.len()).unwrap_or(u32::MAX));
        self.names.push(Arc::clone(&interned));
        self.name_lookup.insert(interned, id);
        id
    }

    fn current_region(&self) -> SkeletonRegionId {
        SkeletonRegionId(
            u32::try_from(
                *self
                    .region_stack
                    .last()
                    .expect("region stack is never empty"),
            )
            .unwrap_or(u32::MAX),
        )
    }

    fn open_region(
        &mut self,
        kind: SkeletonRegionKind,
        span: verter_span::Span,
        control_input: Option<SkeletonExprSiteId>,
    ) -> usize {
        let parent = self.current_region();
        let index = self.regions.len();
        self.regions.push(SkeletonRegion {
            kind,
            parent: Some(parent),
            control_input,
            has_return: false,
            span,
        });
        self.region_stack.push(index);
        index
    }

    fn close_region(&mut self) {
        self.region_stack.pop();
    }

    fn alloc_site(
        &mut self,
        span: verter_span::Span,
        parent: Option<SkeletonExprSiteId>,
    ) -> SkeletonExprSiteId {
        let id = SkeletonExprSiteId(u32::try_from(self.sites.len()).unwrap_or(u32::MAX));
        self.sites.push(SiteDraft {
            span,
            region: self.current_region(),
            parent,
            shape: SkeletonExprShape::Other,
            reads: Vec::new(),
            calls: Vec::new(),
        });
        id
    }

    fn current_site(&self) -> Option<SkeletonExprSiteId> {
        self.site_stack
            .last()
            .copied()
            .map(|index| SkeletonExprSiteId(u32::try_from(index).unwrap_or(u32::MAX)))
    }

    /// The site footprint currently accumulating; a footprint observed
    /// outside any tracked site opens a one-shot anonymous site so no
    /// read / write / call is ever dropped.
    fn footprint_site(&mut self, span: verter_span::Span) -> SkeletonExprSiteId {
        match self.current_site() {
            Some(site) => site,
            None => self.alloc_site(span, None),
        }
    }

    /// Guarantee a current site for a multi-record expression handler;
    /// `true` when an anonymous scope was opened (the caller pops it).
    fn ensure_site_scope(&mut self, span: verter_span::Span) -> bool {
        if self.site_stack.is_empty() {
            let id = self.alloc_site(span, None);
            self.site_stack.push(id.index());
            true
        } else {
            false
        }
    }

    /// Track one root expression position (statement expression, return
    /// argument, declarator initializer, control input).
    fn open_root_site(&mut self, expression: &Expression<'_>) -> SkeletonExprSiteId {
        self.open_site(expression, None)
    }

    fn open_site(
        &mut self,
        expression: &Expression<'_>,
        parent: Option<SkeletonExprSiteId>,
    ) -> SkeletonExprSiteId {
        if let Expression::ObjectExpression(object) = unwrap_expression_carriers(expression) {
            return self.open_object_site(object, parent, expression.span().into());
        }
        let id = self.alloc_site(expression.span().into(), parent);
        self.site_stack.push(id.index());
        self.visit_expression(expression);
        self.site_stack.pop();
        id
    }

    fn open_object_site(
        &mut self,
        object: &ObjectExpression<'_>,
        parent: Option<SkeletonExprSiteId>,
        span: verter_span::Span,
    ) -> SkeletonExprSiteId {
        let id = self.alloc_site(span, parent);
        let mut entries = Vec::new();
        for property in &object.properties {
            match property {
                ObjectPropertyKind::ObjectProperty(property) => {
                    let key = match static_property_key_name(&property.key) {
                        Some(name) => SkeletonObjectKey::Static(self.intern(&name)),
                        None => match property.key.as_expression() {
                            Some(key_expression) => SkeletonObjectKey::Computed(
                                self.open_site(key_expression, Some(id)),
                            ),
                            None => continue,
                        },
                    };
                    let kind = if property.method {
                        SkeletonPropertyKind::Method
                    } else {
                        match property.kind {
                            oxc_ast::ast::PropertyKind::Init => SkeletonPropertyKind::Init,
                            oxc_ast::ast::PropertyKind::Get | oxc_ast::ast::PropertyKind::Set => {
                                SkeletonPropertyKind::Accessor
                            }
                        }
                    };
                    let value = self.open_site(&property.value, Some(id));
                    entries.push(SkeletonObjectEntry::Property { key, value, kind });
                }
                ObjectPropertyKind::SpreadProperty(spread) => {
                    let source = self.open_site(&spread.argument, Some(id));
                    entries.push(SkeletonObjectEntry::Spread { source });
                }
            }
        }
        self.sites[id.index()].shape = SkeletonExprShape::ObjectLiteral {
            entries: Arc::from(entries.into_boxed_slice()),
        };
        id
    }

    fn push_read(&mut self, name: FlowNameId, span: verter_span::Span) {
        let site = self.footprint_site(span);
        self.sites[site.index()]
            .reads
            .push(SkeletonRead { name, span });
    }

    fn push_call(&mut self, call: SkeletonCall) {
        let span = call.span;
        let site = self.footprint_site(span);
        self.sites[site.index()].calls.push(call);
    }

    fn push_write(
        &mut self,
        target: SkeletonWriteTarget,
        path: Arc<[SkeletonPathSegment]>,
        certainty: SkeletonWriteCertainty,
        value: Option<SkeletonExprSiteId>,
        span: verter_span::Span,
    ) {
        let site = self.footprint_site(span);
        let region = self.current_region();
        self.writes.push(SkeletonWrite {
            target,
            path,
            certainty,
            value,
            site,
            region,
            span,
        });
    }

    fn push_binding(
        &mut self,
        name: &str,
        kind: SkeletonBindingKind,
        span: verter_span::Span,
        initializer: Option<SkeletonExprSiteId>,
    ) {
        let name = self.intern(name);
        let region = self.current_region();
        self.bindings.push(SkeletonBinding {
            name,
            kind,
            region,
            span,
            initializer,
        });
    }

    fn push_implicit_return(&mut self, argument: SkeletonExprSiteId, span: verter_span::Span) {
        for index in &self.region_stack {
            self.regions[*index].has_return = true;
        }
        self.return_sites.push(SkeletonReturnSite {
            ordinal: u32::try_from(self.return_sites.len()).unwrap_or(u32::MAX),
            region: self.current_region(),
            argument: Some(argument),
            implicit: true,
            span,
        });
    }

    fn collect_params(&mut self, params: &oxc_ast::ast::FormalParameters<'_>) {
        for param in &params.items {
            self.collect_param_pattern(&param.pattern);
        }
        if let Some(rest) = params.rest.as_ref() {
            self.collect_param_pattern(&rest.rest.argument);
        }
    }

    fn collect_param_pattern(&mut self, pattern: &BindingPattern<'_>) {
        let (inner, initializer) = match pattern {
            BindingPattern::AssignmentPattern(assignment) => {
                let site = self.open_root_site(&assignment.right);
                (&assignment.left, Some(site))
            }
            other => (other, None),
        };
        let mut identifiers = Vec::new();
        let mut defaults = Vec::new();
        collect_binding_pattern(inner, &mut identifiers, &mut defaults);
        // Nested pattern defaults still evaluate at bind time: track each
        // as a root site so its footprint is never dropped.
        for default in defaults {
            self.open_root_site(default);
        }
        for (name, span) in identifiers {
            self.push_binding(&name, SkeletonBindingKind::Param, span, initializer);
        }
    }

    fn collect_declarator_pattern(
        &mut self,
        pattern: &BindingPattern<'_>,
        kind: SkeletonBindingKind,
        initializer: Option<SkeletonExprSiteId>,
    ) {
        let mut identifiers = Vec::new();
        let mut defaults = Vec::new();
        collect_binding_pattern(pattern, &mut identifiers, &mut defaults);
        for default in defaults {
            self.open_root_site(default);
        }
        for (name, span) in identifiers {
            self.push_binding(&name, kind, span, initializer);
        }
    }

    fn record_assignment_targets(
        &mut self,
        target: &AssignmentTarget<'_>,
        certainty: SkeletonWriteCertainty,
        compound_read: bool,
        value: Option<SkeletonExprSiteId>,
    ) {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let name = self.intern(identifier.name.as_str());
                if compound_read {
                    self.push_read(name, identifier.span.into());
                }
                self.push_write(
                    SkeletonWriteTarget::Named(name),
                    Arc::from(Vec::new().into_boxed_slice()),
                    certainty,
                    value,
                    identifier.span.into(),
                );
            }
            AssignmentTarget::StaticMemberExpression(member) => {
                self.record_member_write_target(MemberRef::Static(member), certainty, value);
            }
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.record_member_write_target(MemberRef::Computed(member), certainty, value);
            }
            AssignmentTarget::PrivateFieldExpression(member) => {
                self.record_member_write_target(MemberRef::Private(member), certainty, value);
            }
            AssignmentTarget::TSAsExpression(as_expression) => {
                self.record_expression_write_target(&as_expression.expression, certainty, value);
            }
            AssignmentTarget::TSSatisfiesExpression(satisfies) => {
                self.record_expression_write_target(&satisfies.expression, certainty, value);
            }
            AssignmentTarget::TSNonNullExpression(non_null) => {
                self.record_expression_write_target(&non_null.expression, certainty, value);
            }
            AssignmentTarget::TSTypeAssertion(assertion) => {
                self.record_expression_write_target(&assertion.expression, certainty, value);
            }
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                for element in array.elements.iter().flatten() {
                    self.record_maybe_default_target(element, value);
                }
                if let Some(rest) = array.rest.as_ref() {
                    self.record_assignment_targets(
                        &rest.target,
                        SkeletonWriteCertainty::Definite,
                        false,
                        value,
                    );
                }
            }
            AssignmentTarget::ObjectAssignmentTarget(object) => {
                for property in &object.properties {
                    match property {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(
                            identifier,
                        ) => {
                            let name = self.intern(identifier.binding.name.as_str());
                            if let Some(init) = identifier.init.as_ref() {
                                let parent = self.current_site();
                                let _ = self.open_site(init, parent);
                            }
                            self.push_write(
                                SkeletonWriteTarget::Named(name),
                                Arc::from(Vec::new().into_boxed_slice()),
                                SkeletonWriteCertainty::Definite,
                                value,
                                identifier.span.into(),
                            );
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                            if let Some(key) = property.name.as_expression() {
                                self.visit_expression(key);
                            }
                            self.record_maybe_default_target(&property.binding, value);
                        }
                    }
                }
                if let Some(rest) = object.rest.as_ref() {
                    self.record_assignment_targets(
                        &rest.target,
                        SkeletonWriteCertainty::Definite,
                        false,
                        value,
                    );
                }
            }
        }
    }

    fn record_maybe_default_target(
        &mut self,
        target: &AssignmentTargetMaybeDefault<'_>,
        value: Option<SkeletonExprSiteId>,
    ) {
        match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
                let parent = self.current_site();
                let _ = self.open_site(&with_default.init, parent);
                self.record_assignment_targets(
                    &with_default.binding,
                    SkeletonWriteCertainty::Definite,
                    false,
                    value,
                );
            }
            _ => {
                self.record_assignment_targets(
                    target.to_assignment_target(),
                    SkeletonWriteCertainty::Definite,
                    false,
                    value,
                );
            }
        }
    }

    /// A member-expression write target: builds the projection path and
    /// records the root read (the root object is read to perform a member
    /// write).
    fn record_member_write_target(
        &mut self,
        member: MemberRef<'_, '_>,
        certainty: SkeletonWriteCertainty,
        value: Option<SkeletonExprSiteId>,
    ) {
        let write_span = member.span();
        let mut segments = Vec::new();
        let mut current = member;
        let root = loop {
            let object = match current {
                MemberRef::Static(static_member) => {
                    segments.push(SkeletonPathSegment::Static(
                        self.intern(static_member.property.name.as_str()),
                    ));
                    &static_member.object
                }
                MemberRef::Computed(computed) => {
                    self.visit_expression(&computed.expression);
                    segments.push(SkeletonPathSegment::Computed);
                    &computed.object
                }
                MemberRef::Private(private) => {
                    let name = format!("#{}", private.field.name);
                    segments.push(SkeletonPathSegment::Static(self.intern(&name)));
                    &private.object
                }
            };
            match object {
                Expression::Identifier(identifier) => break Some(identifier),
                Expression::StaticMemberExpression(parent) => {
                    current = MemberRef::Static(parent);
                }
                Expression::ComputedMemberExpression(parent) => {
                    current = MemberRef::Computed(parent);
                }
                Expression::PrivateFieldExpression(parent) => {
                    current = MemberRef::Private(parent);
                }
                other => {
                    self.visit_expression(other);
                    break None;
                }
            }
        };
        segments.reverse();
        let path: Arc<[SkeletonPathSegment]> = Arc::from(segments.into_boxed_slice());
        match root {
            Some(identifier) => {
                let name = self.intern(identifier.name.as_str());
                self.push_read(name, identifier.span.into());
                self.push_write(
                    SkeletonWriteTarget::Named(name),
                    path,
                    certainty,
                    value,
                    write_span,
                );
            }
            None => {
                self.push_write(
                    SkeletonWriteTarget::Opaque,
                    path,
                    certainty,
                    value,
                    write_span,
                );
            }
        }
    }

    /// A TS-carrier-wrapped write target (`(x as T) = v`): unwrap to the
    /// inner identifier / member target.
    fn record_expression_write_target(
        &mut self,
        expression: &Expression<'_>,
        certainty: SkeletonWriteCertainty,
        value: Option<SkeletonExprSiteId>,
    ) {
        match unwrap_expression_carriers(expression) {
            Expression::Identifier(identifier) => {
                let name = self.intern(identifier.name.as_str());
                self.push_write(
                    SkeletonWriteTarget::Named(name),
                    Arc::from(Vec::new().into_boxed_slice()),
                    certainty,
                    value,
                    identifier.span.into(),
                );
            }
            Expression::StaticMemberExpression(member) => {
                self.record_member_write_target(MemberRef::Static(member), certainty, value);
            }
            Expression::ComputedMemberExpression(member) => {
                self.record_member_write_target(MemberRef::Computed(member), certainty, value);
            }
            Expression::PrivateFieldExpression(member) => {
                self.record_member_write_target(MemberRef::Private(member), certainty, value);
            }
            other => {
                self.push_write(
                    SkeletonWriteTarget::Opaque,
                    Arc::from(Vec::new().into_boxed_slice()),
                    certainty,
                    value,
                    other.span().into(),
                );
                self.visit_expression(other);
            }
        }
    }

    fn extract_callee(&mut self, callee: &Expression<'_>) -> SkeletonCallee {
        let callee = unwrap_expression_carriers(callee);
        match callee {
            Expression::Identifier(identifier) => {
                SkeletonCallee::Named(self.intern(identifier.name.as_str()))
            }
            Expression::StaticMemberExpression(member) => {
                let mut names = Vec::new();
                if collect_static_callee_path(member, &mut names) {
                    let interned: Vec<FlowNameId> =
                        names.iter().map(|name| self.intern(name)).collect();
                    SkeletonCallee::Path(Arc::from(interned.into_boxed_slice()))
                } else {
                    SkeletonCallee::Opaque
                }
            }
            _ => SkeletonCallee::Opaque,
        }
    }

    fn visit_statement_list(&mut self, statements: &[Statement<'_>]) {
        for statement in statements {
            self.visit_statement(statement);
        }
    }

    fn finish(self) -> FunctionBodySkeleton {
        FunctionBodySkeleton {
            names: Arc::from(self.names.into_boxed_slice()),
            regions: Arc::from(self.regions.into_boxed_slice()),
            bindings: Arc::from(self.bindings.into_boxed_slice()),
            expr_sites: Arc::from(
                self.sites
                    .into_iter()
                    .map(|draft| SkeletonExprSite {
                        span: draft.span,
                        region: draft.region,
                        parent: draft.parent,
                        shape: draft.shape,
                        reads: Arc::from(draft.reads.into_boxed_slice()),
                        calls: Arc::from(draft.calls.into_boxed_slice()),
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            return_sites: Arc::from(self.return_sites.into_boxed_slice()),
            writes: Arc::from(self.writes.into_boxed_slice()),
        }
    }
}

impl<'a> Visit<'a> for SkeletonBuilder {
    // Type positions are never value footprint: a type name is not a read.
    fn visit_ts_type(&mut self, _it: &oxc_ast::ast::TSType<'a>) {}

    fn visit_ts_type_annotation(&mut self, _it: &oxc_ast::ast::TSTypeAnnotation<'a>) {}

    fn visit_ts_type_parameter_instantiation(
        &mut self,
        _it: &oxc_ast::ast::TSTypeParameterInstantiation<'a>,
    ) {
    }

    fn visit_ts_type_parameter_declaration(
        &mut self,
        _it: &oxc_ast::ast::TSTypeParameterDeclaration<'a>,
    ) {
    }

    // Nested function / arrow / class bodies are their own frames.
    fn visit_function(&mut self, _it: &Function<'a>, _flags: oxc_syntax::scope::ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _it: &oxc_ast::ast::Class<'a>) {}

    fn visit_statement(&mut self, it: &Statement<'a>) {
        match it {
            Statement::FunctionDeclaration(function) => {
                if let Some(id) = function.id.as_ref() {
                    self.push_binding(
                        id.name.as_str(),
                        SkeletonBindingKind::NestedFunction,
                        id.span.into(),
                        None,
                    );
                }
            }
            Statement::ClassDeclaration(class) => {
                if let Some(id) = class.id.as_ref() {
                    self.push_binding(
                        id.name.as_str(),
                        SkeletonBindingKind::Class,
                        id.span.into(),
                        None,
                    );
                }
            }
            _ => walk::walk_statement(self, it),
        }
    }

    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'a>) {
        self.open_region(SkeletonRegionKind::Block, it.span.into(), None);
        self.visit_statement_list(&it.body);
        self.close_region();
    }

    fn visit_if_statement(&mut self, it: &oxc_ast::ast::IfStatement<'a>) {
        let condition = self.open_root_site(&it.test);
        self.open_region(
            SkeletonRegionKind::IfConsequent,
            it.consequent.span().into(),
            Some(condition),
        );
        self.visit_statement(&it.consequent);
        self.close_region();
        if let Some(alternate) = it.alternate.as_ref() {
            self.open_region(
                SkeletonRegionKind::IfAlternate,
                alternate.span().into(),
                Some(condition),
            );
            self.visit_statement(alternate);
            self.close_region();
        }
    }

    fn visit_while_statement(&mut self, it: &oxc_ast::ast::WhileStatement<'a>) {
        let region = self.open_region(SkeletonRegionKind::Loop, it.span.into(), None);
        let condition = self.open_root_site(&it.test);
        self.regions[region].control_input = Some(condition);
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_do_while_statement(&mut self, it: &oxc_ast::ast::DoWhileStatement<'a>) {
        let region = self.open_region(SkeletonRegionKind::Loop, it.span.into(), None);
        let condition = self.open_root_site(&it.test);
        self.regions[region].control_input = Some(condition);
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_for_statement(&mut self, it: &oxc_ast::ast::ForStatement<'a>) {
        let region = self.open_region(SkeletonRegionKind::Loop, it.span.into(), None);
        if let Some(init) = it.init.as_ref() {
            match init {
                oxc_ast::ast::ForStatementInit::VariableDeclaration(declaration) => {
                    self.visit_variable_declaration(declaration);
                }
                _ => {
                    if let Some(expression) = init.as_expression() {
                        self.open_root_site(expression);
                    }
                }
            }
        }
        if let Some(test) = it.test.as_ref() {
            let condition = self.open_root_site(test);
            self.regions[region].control_input = Some(condition);
        }
        if let Some(update) = it.update.as_ref() {
            self.open_root_site(update);
        }
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_for_in_statement(&mut self, it: &oxc_ast::ast::ForInStatement<'a>) {
        self.open_region(SkeletonRegionKind::Loop, it.span.into(), None);
        let source = self.open_root_site(&it.right);
        self.record_for_left(&it.left, source);
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_for_of_statement(&mut self, it: &oxc_ast::ast::ForOfStatement<'a>) {
        self.open_region(SkeletonRegionKind::Loop, it.span.into(), None);
        let source = self.open_root_site(&it.right);
        self.record_for_left(&it.left, source);
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_switch_statement(&mut self, it: &oxc_ast::ast::SwitchStatement<'a>) {
        let region = self.open_region(SkeletonRegionKind::Switch, it.span.into(), None);
        let discriminant = self.open_root_site(&it.discriminant);
        self.regions[region].control_input = Some(discriminant);
        for case in &it.cases {
            self.open_region(SkeletonRegionKind::SwitchCase, case.span.into(), None);
            if let Some(test) = case.test.as_ref() {
                self.open_root_site(test);
            }
            self.visit_statement_list(&case.consequent);
            self.close_region();
        }
        self.close_region();
    }

    fn visit_try_statement(&mut self, it: &oxc_ast::ast::TryStatement<'a>) {
        self.open_region(SkeletonRegionKind::TryBlock, it.block.span.into(), None);
        self.visit_statement_list(&it.block.body);
        self.close_region();
        if let Some(handler) = it.handler.as_ref() {
            self.open_region(SkeletonRegionKind::CatchClause, handler.span.into(), None);
            if let Some(param) = handler.param.as_ref() {
                self.collect_declarator_pattern(
                    &param.pattern,
                    SkeletonBindingKind::CatchParam,
                    None,
                );
            }
            self.visit_statement_list(&handler.body.body);
            self.close_region();
        }
        if let Some(finalizer) = it.finalizer.as_ref() {
            self.open_region(
                SkeletonRegionKind::FinallyBlock,
                finalizer.span.into(),
                None,
            );
            self.visit_statement_list(&finalizer.body);
            self.close_region();
        }
    }

    fn visit_labeled_statement(&mut self, it: &oxc_ast::ast::LabeledStatement<'a>) {
        self.open_region(SkeletonRegionKind::LabeledBody, it.span.into(), None);
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_return_statement(&mut self, it: &oxc_ast::ast::ReturnStatement<'a>) {
        for index in &self.region_stack {
            self.regions[*index].has_return = true;
        }
        let argument = it
            .argument
            .as_ref()
            .map(|argument| self.open_root_site(argument));
        self.return_sites.push(SkeletonReturnSite {
            ordinal: u32::try_from(self.return_sites.len()).unwrap_or(u32::MAX),
            region: self.current_region(),
            argument,
            implicit: false,
            span: it.span.into(),
        });
    }

    fn visit_expression_statement(&mut self, it: &oxc_ast::ast::ExpressionStatement<'a>) {
        self.open_root_site(&it.expression);
    }

    fn visit_throw_statement(&mut self, it: &oxc_ast::ast::ThrowStatement<'a>) {
        self.open_root_site(&it.argument);
    }

    fn visit_variable_declaration(&mut self, it: &oxc_ast::ast::VariableDeclaration<'a>) {
        let kind = match it.kind {
            oxc_ast::ast::VariableDeclarationKind::Const => SkeletonBindingKind::Const,
            oxc_ast::ast::VariableDeclarationKind::Let => SkeletonBindingKind::Let,
            oxc_ast::ast::VariableDeclarationKind::Var
            | oxc_ast::ast::VariableDeclarationKind::Using
            | oxc_ast::ast::VariableDeclarationKind::AwaitUsing => SkeletonBindingKind::Var,
        };
        for declarator in &it.declarations {
            let initializer = declarator
                .init
                .as_ref()
                .map(|init| self.open_root_site(init));
            self.collect_declarator_pattern(&declarator.id, kind, initializer);
        }
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        let name = self.intern(it.name.as_str());
        self.push_read(name, it.span.into());
    }

    fn visit_object_expression(&mut self, it: &ObjectExpression<'a>) {
        // An object literal nested inside a non-object expression tree
        // becomes its own child site so its property footprint stays
        // path-precise.
        let parent = self.current_site();
        self.open_object_site(it, parent, it.span.into());
    }

    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        let scoped = self.ensure_site_scope(it.span.into());
        let containing = self
            .current_site()
            .expect("assignment scope guarantees a current site");
        let certainty = match it.operator {
            oxc_ast::ast::AssignmentOperator::LogicalAnd
            | oxc_ast::ast::AssignmentOperator::LogicalOr
            | oxc_ast::ast::AssignmentOperator::LogicalNullish => SkeletonWriteCertainty::Optional,
            _ => SkeletonWriteCertainty::Definite,
        };
        let compound_read = !matches!(it.operator, oxc_ast::ast::AssignmentOperator::Assign);
        let value = self.open_site(&it.right, Some(containing));
        self.record_assignment_targets(&it.left, certainty, compound_read, Some(value));
        if scoped {
            self.site_stack.pop();
        }
    }

    fn visit_update_expression(&mut self, it: &oxc_ast::ast::UpdateExpression<'a>) {
        let scoped = self.ensure_site_scope(it.span.into());
        match &it.argument {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let name = self.intern(identifier.name.as_str());
                self.push_read(name, identifier.span.into());
                self.push_write(
                    SkeletonWriteTarget::Named(name),
                    Arc::from(Vec::new().into_boxed_slice()),
                    SkeletonWriteCertainty::Definite,
                    None,
                    it.span.into(),
                );
            }
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.record_member_write_target(
                    MemberRef::Static(member),
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.record_member_write_target(
                    MemberRef::Computed(member),
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::PrivateFieldExpression(member) => {
                self.record_member_write_target(
                    MemberRef::Private(member),
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::TSAsExpression(inner) => {
                self.record_expression_write_target(
                    &inner.expression,
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::TSSatisfiesExpression(inner) => {
                self.record_expression_write_target(
                    &inner.expression,
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::TSNonNullExpression(inner) => {
                self.record_expression_write_target(
                    &inner.expression,
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::TSTypeAssertion(inner) => {
                self.record_expression_write_target(
                    &inner.expression,
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
        }
        if scoped {
            self.site_stack.pop();
        }
    }

    fn visit_call_expression(&mut self, it: &oxc_ast::ast::CallExpression<'a>) {
        let callee = self.extract_callee(&it.callee);
        self.push_call(SkeletonCall {
            callee,
            new_construct: false,
            span: it.span.into(),
        });
        walk::walk_call_expression(self, it);
    }

    fn visit_new_expression(&mut self, it: &oxc_ast::ast::NewExpression<'a>) {
        let callee = self.extract_callee(&it.callee);
        self.push_call(SkeletonCall {
            callee,
            new_construct: true,
            span: it.span.into(),
        });
        walk::walk_new_expression(self, it);
    }
}

impl SkeletonBuilder {
    fn record_for_left(
        &mut self,
        left: &oxc_ast::ast::ForStatementLeft<'_>,
        source: SkeletonExprSiteId,
    ) {
        match left {
            oxc_ast::ast::ForStatementLeft::VariableDeclaration(declaration) => {
                let kind = match declaration.kind {
                    oxc_ast::ast::VariableDeclarationKind::Const => SkeletonBindingKind::Const,
                    oxc_ast::ast::VariableDeclarationKind::Let => SkeletonBindingKind::Let,
                    oxc_ast::ast::VariableDeclarationKind::Var
                    | oxc_ast::ast::VariableDeclarationKind::Using
                    | oxc_ast::ast::VariableDeclarationKind::AwaitUsing => SkeletonBindingKind::Var,
                };
                for declarator in &declaration.declarations {
                    let mut identifiers = Vec::new();
                    let mut defaults = Vec::new();
                    collect_binding_pattern(&declarator.id, &mut identifiers, &mut defaults);
                    for default in defaults {
                        self.open_root_site(default);
                    }
                    for (name, span) in identifiers {
                        let interned = self.intern(&name);
                        self.push_binding(&name, kind, span, None);
                        self.writes.push(SkeletonWrite {
                            target: SkeletonWriteTarget::Named(interned),
                            path: Arc::from(Vec::new().into_boxed_slice()),
                            certainty: SkeletonWriteCertainty::Optional,
                            value: Some(source),
                            site: source,
                            region: self.current_region(),
                            span,
                        });
                    }
                }
            }
            _ => {
                if let Some(target) = left.as_assignment_target() {
                    // Iteration writes attribute to the iteration source's
                    // site: evaluating the source drives the per-iteration
                    // write.
                    self.site_stack.push(source.index());
                    self.record_assignment_targets(
                        target,
                        SkeletonWriteCertainty::Optional,
                        false,
                        Some(source),
                    );
                    self.site_stack.pop();
                }
            }
        }
    }
}

/// Unwrap value-transparent carriers (parentheses, TS assertion / cast
/// carriers) for shape classification; the carriers stay part of the
/// site's span.
fn unwrap_expression_carriers<'a, 'ast>(expression: &'a Expression<'ast>) -> &'a Expression<'ast> {
    let mut current = expression;
    loop {
        current = match current {
            Expression::ParenthesizedExpression(inner) => &inner.expression,
            Expression::TSAsExpression(inner) => &inner.expression,
            Expression::TSSatisfiesExpression(inner) => &inner.expression,
            Expression::TSNonNullExpression(inner) => &inner.expression,
            Expression::TSTypeAssertion(inner) => &inner.expression,
            Expression::TSInstantiationExpression(inner) => &inner.expression,
            _ => return current,
        };
    }
}

/// One borrowed member-expression step of a write-target chain.
enum MemberRef<'a, 'ast> {
    Static(&'a oxc_ast::ast::StaticMemberExpression<'ast>),
    Computed(&'a oxc_ast::ast::ComputedMemberExpression<'ast>),
    Private(&'a oxc_ast::ast::PrivateFieldExpression<'ast>),
}

impl MemberRef<'_, '_> {
    fn span(&self) -> verter_span::Span {
        match self {
            MemberRef::Static(member) => member.span.into(),
            MemberRef::Computed(member) => member.span.into(),
            MemberRef::Private(member) => member.span.into(),
        }
    }
}

/// Collect the dotted callee path of `a.b.c()` (root first). `false` for
/// non-identifier roots.
fn collect_static_callee_path(
    member: &oxc_ast::ast::StaticMemberExpression<'_>,
    names: &mut Vec<String>,
) -> bool {
    let mut properties = Vec::new();
    let mut current = member;
    loop {
        properties.push(current.property.name.to_string());
        match &current.object {
            Expression::Identifier(identifier) => {
                names.push(identifier.name.to_string());
                break;
            }
            Expression::StaticMemberExpression(parent) => current = parent,
            _ => return false,
        }
    }
    properties.reverse();
    names.extend(properties);
    true
}

/// Collect every bound identifier (name, span) and every default-value
/// expression of one binding pattern.
fn collect_binding_pattern<'a, 'ast>(
    pattern: &'a BindingPattern<'ast>,
    identifiers: &mut Vec<(String, verter_span::Span)>,
    defaults: &mut Vec<&'a Expression<'ast>>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(identifier) => {
            identifiers.push((identifier.name.to_string(), identifier.span.into()));
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                collect_binding_pattern(&property.value, identifiers, defaults);
            }
            if let Some(rest) = object.rest.as_ref() {
                collect_binding_pattern(&rest.argument, identifiers, defaults);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                collect_binding_pattern(element, identifiers, defaults);
            }
            if let Some(rest) = array.rest.as_ref() {
                collect_binding_pattern(&rest.argument, identifiers, defaults);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            defaults.push(&assignment.right);
            collect_binding_pattern(&assignment.left, identifiers, defaults);
        }
    }
}
