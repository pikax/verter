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

use std::sync::Arc;

use oxc_ast::ast::{
    ArrowFunctionExpression, BindingPattern, CallExpression, Class, Expression, Function,
    MethodDefinitionKind, ObjectPropertyKind, PropertyKey, Statement, VariableDeclaration,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::span_origins::DeclContributorAnchor;

use crate::analysis::top_level_owners::TopLevelOwnerTable;
use crate::analysis::types::Hash16;
use crate::facts::SymbolSpace;

#[cfg(test)]
#[path = "function_program_tests.rs"]
mod function_program_tests;

/// The declaration a served function position belongs to (content-free;
/// the owner discriminates script-block owners, the name is the registered
/// merged-symbol name — namespaces qualify `Ns.Name` exactly like the eval
/// env registration).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionDeclarationRef {
    /// Lexical top-level owner of the contributing statement.
    pub owner: verter_type_expr::TopLevelOwnerId,
    /// Registered merged-symbol name.
    pub name: Arc<str>,
    /// Type-space vs value-space discriminator (functions are value-space;
    /// the discriminator keeps the ref shape aligned with slot identity).
    pub space: SymbolSpace,
}

/// One ordinal step from a contributing top-level statement down to the
/// function node. Named positions / small ordinals only — never a byte
/// span, never a lowered type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionDescentStep {
    /// The statement IS the function declaration.
    FunctionDeclaration,
    /// The init of the variable declarator at `declarator_ordinal` (the
    /// init is the arrow / function expression).
    VariableInitializer { declarator_ordinal: u32 },
    /// The class member at `member_ordinal` (`ClassBody.body` index).
    ClassMember { member_ordinal: u32 },
    /// The object-literal method at `member_ordinal` inside the current
    /// initializer object expression.
    ObjectMember { member_ordinal: u32 },
    /// The object-literal method at `member_ordinal` inside an
    /// `export default { … }` object expression.
    ExportDefaultObjectMember { member_ordinal: u32 },
    /// The statement at `statement_ordinal` inside a namespace block.
    NamespaceMember { statement_ordinal: u32 },
}

/// Arena-free locator for one function's body inside the retained parse
/// snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionBodyLocator {
    /// The contributing top-level statement.
    pub contributor: DeclContributorAnchor,
    /// Ordinal descent from the contributing statement to the function node.
    pub descent: Arc<[FunctionDescentStep]>,
}

/// The full program identity of one served function position.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionProgramKey {
    /// The owning declaration.
    pub declaration: FunctionDeclarationRef,
    /// Which authored position of the declaration this callable occupies.
    pub part: FunctionPartIdentity,
    /// Signature ordinal inside an overload group, in source order (the
    /// trailing implementation is the last ordinal). Zero outside overload
    /// groups.
    pub overload_ordinal: u32,
}

/// One formal parameter's binding fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionParamRecord {
    /// The binding name (`None` for a destructured parameter).
    pub name: Option<Arc<str>>,
    /// Whether the parameter is optional (`?`).
    pub optional: bool,
    /// Whether this is the rest parameter.
    pub rest: bool,
    /// Whether the parameter carries an authored TS type annotation.
    pub has_ts_annotation: bool,
}

/// The kind of one local binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionBindingKind {
    /// A formal parameter.
    Param,
    /// A `const` declarator.
    Const,
    /// A `let` declarator.
    Let,
    /// A `var` declarator.
    Var,
    /// A nested function declaration's name.
    NestedFunction,
}

/// One local binding (parameter, variable declarator, nested function name).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionBindingRecord {
    /// The binding name.
    pub name: Arc<str>,
    /// The binding kind.
    pub kind: FunctionBindingKind,
    /// The binding's span.
    pub span: verter_span::Span,
}

/// One identifier reference in the current function body (nested function
/// bodies excluded — their references resolve in their own frames).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionReferenceRecord {
    /// The referenced name.
    pub name: Arc<str>,
    /// The reference span.
    pub span: verter_span::Span,
}

/// One `return` site of the current function, in source order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionReturnSite {
    /// Source-order ordinal among the function's return sites.
    pub ordinal: u32,
    /// Whether the site carries an argument expression (bare `return;`
    /// contributes `undefined`).
    pub has_argument: bool,
    /// The return statement's span.
    pub span: verter_span::Span,
}

/// The callee shape of one evaluation-effect call site.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FunctionEffectCallee {
    /// A bare identifier callee (`g()`).
    Identifier(Arc<str>),
    /// A static member path (`a.b.c()`).
    StaticMember(Arc<[Arc<str>]>),
    /// Any other callee shape (computed, call-result, `this`-rooted).
    Other,
}

/// One evaluation-effect call site in the current function body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionEffectRecord {
    /// The call expression's span.
    pub span: verter_span::Span,
    /// The callee shape.
    pub callee: FunctionEffectCallee,
}

/// One write site (assignment or update) in the current function body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionWriteRecord {
    /// The write expression's span.
    pub span: verter_span::Span,
}

/// The control-region kind of one skeleton region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionControlKind {
    /// A block statement.
    Block,
    /// An `if` statement (consequent / alternate arms nest as regions).
    If,
    /// A loop (`for` / `for-in` / `for-of` / `while` / `do-while`).
    Loop,
    /// A `switch` statement.
    Switch,
    /// A `try` statement.
    Try,
    /// A labeled statement.
    Labeled,
}

/// One control-region skeleton entry (the current function's statement
/// tree, nested function bodies excluded).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionControlRegion {
    /// The region kind.
    pub kind: FunctionControlKind,
    /// Whether the region's statement subtree contains a `return` of the
    /// current function (drives return-transparency: return-free loop /
    /// labeled constructs are fall-through transparent; return-bearing
    /// loop / labeled regions, and every switch / try, are unsupported).
    pub has_return: bool,
    /// The region statement's span.
    pub span: verter_span::Span,
}

/// One exact direct local call: the callee is a bare identifier bound to a
/// function in the same index (a same-file, syntactically exact target —
/// direct same-slot recursion included).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionDirectCall {
    /// The call expression's span.
    pub span: verter_span::Span,
    /// The callee's program identity.
    pub target: FunctionProgramKey,
}

/// One served function position: identity, body locator, structural
/// inventory, and the whole-function stable hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionProgramEntry {
    /// The program identity.
    pub key: FunctionProgramKey,
    /// Arena-free body locator into the retained snapshot.
    pub locator: FunctionBodyLocator,
    /// Formal parameter facts (source order).
    pub params: Arc<[FunctionParamRecord]>,
    /// Local bindings (parameters, declarators, nested function names).
    pub bindings: Arc<[FunctionBindingRecord]>,
    /// Identifier references in the current function body.
    pub references: Arc<[FunctionReferenceRecord]>,
    /// Return sites in source order.
    pub return_sites: Arc<[FunctionReturnSite]>,
    /// Write sites (assignments / updates).
    pub writes: Arc<[FunctionWriteRecord]>,
    /// Evaluation-effect call sites.
    pub effects: Arc<[FunctionEffectRecord]>,
    /// Control-region skeleton.
    pub control: Arc<[FunctionControlRegion]>,
    /// Exact direct local call targets.
    pub direct_calls: Arc<[FunctionDirectCall]>,
    /// The whole-function stable hash (structural content only — the
    /// parser / language / parse-env identity folds in at the artifact
    /// boundary).
    pub flow_body_stable_hash: Hash16,
}

/// The per-file function program index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FunctionProgramIndex {
    /// Every served function position, in source order.
    pub entries: Arc<[FunctionProgramEntry]>,
}

impl FunctionProgramIndex {
    /// The entry for `key`, when the position is served by this file.
    #[must_use]
    pub fn get(&self, key: &FunctionProgramKey) -> Option<&FunctionProgramEntry> {
        self.entries.iter().find(|entry| &entry.key == key)
    }

    /// The entry for a value-space function declaration / initializer of
    /// `name` at `overload_ordinal`, when present.
    #[must_use]
    pub fn value_function(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        part: &FunctionPartIdentity,
        overload_ordinal: u32,
    ) -> Option<&FunctionProgramEntry> {
        self.entries.iter().find(|entry| {
            entry.key.declaration.owner == owner
                && entry.key.declaration.name.as_ref() == name
                && entry.key.declaration.space == SymbolSpace::Value
                && &entry.key.part == part
                && entry.key.overload_ordinal == overload_ordinal
        })
    }
}

// ---------------------------------------------------------------------------
// Discovery walk
// ---------------------------------------------------------------------------

struct DiscoveryCtx<'a> {
    source: &'a str,
    owners: &'a TopLevelOwnerTable,
    entries: Vec<FunctionProgramEntry>,
}

impl<'a> DiscoveryCtx<'a> {
    fn anchor(&self, contributor_index: usize) -> Option<DeclContributorAnchor> {
        let owner = self.owners.statements().get(contributor_index)?;
        Some(DeclContributorAnchor {
            contributor_index: u32::try_from(contributor_index).ok()?,
            owner: owner.owner,
            owner_local_ordinal: owner.owner_local_ordinal,
        })
    }

    fn push(&mut self, entry: FunctionProgramEntry) {
        self.entries.push(entry);
    }
}

/// Build the per-file function program index from one retained parse
/// snapshot. One structural walk; no lowering, no type resolution.
pub fn build_function_program_index(
    program: &oxc_ast::ast::Program<'_>,
    source: &str,
    owners: &TopLevelOwnerTable,
) -> FunctionProgramIndex {
    let mut ctx = DiscoveryCtx {
        source,
        owners,
        entries: Vec::new(),
    };
    let mut overload_tracker = OverloadTracker::default();
    for (contributor_index, stmt) in program.body.iter().enumerate() {
        discover_statement(
            stmt,
            contributor_index,
            None,
            &mut overload_tracker,
            &mut ctx,
        );
    }
    resolve_direct_calls(&mut ctx.entries);
    FunctionProgramIndex {
        entries: Arc::from(ctx.entries.into_boxed_slice()),
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

fn discover_statement(
    stmt: &Statement<'_>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_>,
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
        Statement::ExportNamedDeclaration(export) => {
            if let Some(decl) = export.declaration.as_ref() {
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
                    _ => {}
                }
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
                    let descent = vec![FunctionDescentStep::ExportDefaultObjectMember {
                        member_ordinal: u32::try_from(member_ordinal).unwrap_or(u32::MAX),
                    }];
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
        Statement::TSModuleDeclaration(module) => {
            // `declare module "specifier" { .. }` is an ambient augmentation,
            // not a file-scope function owner — never indexed here. Identifier
            // namespaces recurse with qualified names.
            if let oxc_ast::ast::TSModuleDeclarationName::Identifier(id) = &module.id {
                let prefix = match namespace_prefix {
                    Some(prefix) => format!("{prefix}.{}", id.name),
                    None => id.name.to_string(),
                };
                if let Some(oxc_ast::ast::TSModuleDeclarationBody::TSModuleBlock(block)) =
                    module.body.as_ref()
                {
                    for (statement_ordinal, inner) in block.body.iter().enumerate() {
                        discover_namespaced_statement(
                            inner,
                            contributor_index,
                            statement_ordinal,
                            &prefix,
                            overload_tracker,
                            ctx,
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

fn discover_namespaced_statement(
    stmt: &Statement<'_>,
    contributor_index: usize,
    statement_ordinal: usize,
    namespace: &str,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_>,
) {
    match stmt {
        Statement::ExportNamedDeclaration(export) => {
            if let Some(decl) = export.declaration.as_ref() {
                match decl {
                    oxc_ast::ast::Declaration::FunctionDeclaration(func) => {
                        discover_namespaced_function(
                            func,
                            contributor_index,
                            statement_ordinal,
                            namespace,
                            overload_tracker,
                            ctx,
                        );
                    }
                    oxc_ast::ast::Declaration::VariableDeclaration(var_decl) => {
                        discover_variable_declaration_ns(
                            var_decl,
                            contributor_index,
                            statement_ordinal,
                            namespace,
                            ctx,
                        );
                    }
                    oxc_ast::ast::Declaration::ClassDeclaration(class) => {
                        discover_class_ns(
                            class,
                            contributor_index,
                            statement_ordinal,
                            namespace,
                            ctx,
                        );
                    }
                    _ => {}
                }
            }
        }
        Statement::FunctionDeclaration(func) => {
            discover_namespaced_function(
                func,
                contributor_index,
                statement_ordinal,
                namespace,
                overload_tracker,
                ctx,
            );
        }
        Statement::VariableDeclaration(var_decl) => {
            discover_variable_declaration_ns(
                var_decl,
                contributor_index,
                statement_ordinal,
                namespace,
                ctx,
            );
        }
        Statement::ClassDeclaration(class) => {
            discover_class_ns(class, contributor_index, statement_ordinal, namespace, ctx);
        }
        Statement::TSModuleDeclaration(module) => {
            if let oxc_ast::ast::TSModuleDeclarationName::Identifier(id) = &module.id {
                let prefix = format!("{namespace}.{}", id.name);
                if let Some(oxc_ast::ast::TSModuleDeclarationBody::TSModuleBlock(block)) =
                    module.body.as_ref()
                {
                    for (inner_ordinal, inner) in block.body.iter().enumerate() {
                        let _ = inner_ordinal;
                        discover_namespaced_statement(
                            inner,
                            contributor_index,
                            inner_ordinal,
                            &prefix,
                            overload_tracker,
                            ctx,
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

fn discover_namespaced_function(
    func: &Function<'_>,
    contributor_index: usize,
    statement_ordinal: usize,
    namespace: &str,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_>,
) {
    if let Some(id) = func.id.as_ref() {
        let qualified = format!("{namespace}.{}", id.name);
        let overload_ordinal = overload_tracker.next_function_ordinal(&qualified);
        discover_function_inner(
            func,
            &qualified,
            FunctionPartIdentity::DeclarationBody,
            contributor_index,
            vec![
                FunctionDescentStep::NamespaceMember {
                    statement_ordinal: u32::try_from(statement_ordinal).unwrap_or(u32::MAX),
                },
                FunctionDescentStep::FunctionDeclaration,
            ],
            overload_ordinal,
            ctx,
        );
    }
}

fn discover_function_declaration(
    func: &Function<'_>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_>,
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

fn discover_function_declaration_named(
    func: &Function<'_>,
    name: &str,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_>,
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
        vec![FunctionDescentStep::FunctionDeclaration],
        overload_ordinal,
        ctx,
    );
}

fn discover_variable_declaration(
    var_decl: &VariableDeclaration<'_>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    ctx: &mut DiscoveryCtx<'_>,
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
        let descent = |extra: FunctionDescentStep| {
            vec![
                FunctionDescentStep::VariableInitializer {
                    declarator_ordinal: u32::try_from(declarator_ordinal).unwrap_or(u32::MAX),
                },
                extra,
            ]
        };
        match init {
            Expression::ArrowFunctionExpression(arrow) => {
                discover_arrow_inner(
                    arrow,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    vec![FunctionDescentStep::VariableInitializer {
                        declarator_ordinal: u32::try_from(declarator_ordinal).unwrap_or(u32::MAX),
                    }],
                    ctx,
                );
            }
            Expression::FunctionExpression(func) => {
                discover_function_inner(
                    func,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    vec![FunctionDescentStep::VariableInitializer {
                        declarator_ordinal: u32::try_from(declarator_ordinal).unwrap_or(u32::MAX),
                    }],
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

fn discover_variable_declaration_ns(
    var_decl: &VariableDeclaration<'_>,
    contributor_index: usize,
    statement_ordinal: usize,
    namespace: &str,
    ctx: &mut DiscoveryCtx<'_>,
) {
    for (declarator_ordinal, declarator) in var_decl.declarations.iter().enumerate() {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            continue;
        };
        let name = format!("{namespace}.{}", id.name);
        let Some(init) = declarator.init.as_ref() else {
            continue;
        };
        let base = vec![
            FunctionDescentStep::NamespaceMember {
                statement_ordinal: u32::try_from(statement_ordinal).unwrap_or(u32::MAX),
            },
            FunctionDescentStep::VariableInitializer {
                declarator_ordinal: u32::try_from(declarator_ordinal).unwrap_or(u32::MAX),
            },
        ];
        match init {
            Expression::ArrowFunctionExpression(arrow) => {
                discover_arrow_inner(
                    arrow,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    base,
                    ctx,
                );
            }
            Expression::FunctionExpression(func) => {
                discover_function_inner(
                    func,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    base,
                    0,
                    ctx,
                );
            }
            _ => {}
        }
    }
}

fn discover_class(
    class: &Class<'_>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let Some(id) = class.id.as_ref() else {
        return;
    };
    let name = match namespace_prefix {
        Some(prefix) => format!("{prefix}.{}", id.name),
        None => id.name.to_string(),
    };
    discover_class_members(class, &name, contributor_index, Vec::new(), ctx);
}

fn discover_class_ns(
    class: &Class<'_>,
    contributor_index: usize,
    statement_ordinal: usize,
    namespace: &str,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let Some(id) = class.id.as_ref() else {
        return;
    };
    let name = format!("{namespace}.{}", id.name);
    discover_class_members(
        class,
        &name,
        contributor_index,
        vec![FunctionDescentStep::NamespaceMember {
            statement_ordinal: u32::try_from(statement_ordinal).unwrap_or(u32::MAX),
        }],
        ctx,
    );
}

fn discover_class_members(
    class: &Class<'_>,
    name: &str,
    contributor_index: usize,
    base_descent: Vec<FunctionDescentStep>,
    ctx: &mut DiscoveryCtx<'_>,
) {
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
                let member_path: Arc<[u32]> = Arc::from(vec![member_ordinal].into_boxed_slice());
                let mut descent = base_descent.clone();
                descent.push(FunctionDescentStep::ClassMember { member_ordinal });
                discover_function_inner(
                    &method.value,
                    name,
                    FunctionPartIdentity::Member { member_path },
                    contributor_index,
                    descent,
                    overload,
                    ctx,
                );
            }
            oxc_ast::ast::ClassElement::PropertyDefinition(prop) => {
                let Some(_member_name) = static_property_key_name(&prop.key) else {
                    continue;
                };
                let member_path: Arc<[u32]> = Arc::from(vec![member_ordinal].into_boxed_slice());
                let mut descent = base_descent.clone();
                descent.push(FunctionDescentStep::ClassMember { member_ordinal });
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
            }
            _ => {}
        }
    }
}

pub(crate) fn static_property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
        _ => None,
    }
}

fn discover_function_inner(
    func: &Function<'_>,
    name: &str,
    part: FunctionPartIdentity,
    contributor_index: usize,
    descent: Vec<FunctionDescentStep>,
    overload_ordinal: u32,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let Some(anchor) = ctx.anchor(contributor_index) else {
        return;
    };
    let Some(body) = func.body.as_ref() else {
        return;
    };
    let params = formal_params(&func.params);
    let entry = build_entry(
        ctx.source,
        FunctionProgramKey {
            declaration: FunctionDeclarationRef {
                owner: anchor.owner,
                name: Arc::from(name),
                space: SymbolSpace::Value,
            },
            part,
            overload_ordinal,
        },
        FunctionBodyLocator {
            contributor: anchor,
            descent: Arc::from(descent.into_boxed_slice()),
        },
        params,
        &body.statements,
        func.span.start,
        FunctionNode::Function(func),
    );
    ctx.push(entry);
}

fn discover_arrow_inner(
    arrow: &ArrowFunctionExpression<'_>,
    name: &str,
    part: FunctionPartIdentity,
    contributor_index: usize,
    descent: Vec<FunctionDescentStep>,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let Some(anchor) = ctx.anchor(contributor_index) else {
        return;
    };
    let params = formal_params(&arrow.params);
    let entry = build_entry(
        ctx.source,
        FunctionProgramKey {
            declaration: FunctionDeclarationRef {
                owner: anchor.owner,
                name: Arc::from(name),
                space: SymbolSpace::Value,
            },
            part,
            overload_ordinal: 0,
        },
        FunctionBodyLocator {
            contributor: anchor,
            descent: Arc::from(descent.into_boxed_slice()),
        },
        params,
        &arrow.body.statements,
        arrow.span.start,
        FunctionNode::Arrow(arrow),
    );
    ctx.push(entry);
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
        });
    }
    Arc::from(out.into_boxed_slice())
}

// ---------------------------------------------------------------------------
// Entry build: inventory + stable hash
// ---------------------------------------------------------------------------

fn build_entry(
    source: &str,
    key: FunctionProgramKey,
    locator: FunctionBodyLocator,
    params: Arc<[FunctionParamRecord]>,
    statements: &[Statement<'_>],
    function_start: u32,
    node: FunctionNode<'_>,
) -> FunctionProgramEntry {
    let mut inventory = InventoryVisitor::default();
    for stmt in statements {
        inventory.visit_statement(stmt);
    }
    let InventoryVisitor {
        mut bindings,
        references,
        return_sites,
        writes,
        effects,
        control,
        control_stack: _,
    } = inventory;
    for param in params.iter() {
        if let Some(name) = param.name.as_ref() {
            bindings.push(FunctionBindingRecord {
                name: Arc::clone(name),
                kind: FunctionBindingKind::Param,
                span: verter_span::Span::new(0, 0),
            });
        }
    }
    bindings.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then((left.kind as u8).cmp(&(right.kind as u8)))
    });
    bindings.dedup_by(|left, right| left.name == right.name && left.kind == right.kind);

    let hash = crate::analysis::function_program_hash::hash_function_body(
        source,
        statements,
        &params,
        function_start,
        node,
    );

    FunctionProgramEntry {
        key,
        locator,
        params,
        bindings: Arc::from(bindings.into_boxed_slice()),
        references: Arc::from(references.into_boxed_slice()),
        return_sites: Arc::from(return_sites.into_boxed_slice()),
        writes: Arc::from(writes.into_boxed_slice()),
        effects: Arc::from(effects.into_boxed_slice()),
        control: Arc::from(control.into_boxed_slice()),
        direct_calls: Arc::from(Vec::new().into_boxed_slice()),
        flow_body_stable_hash: hash,
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
pub fn inventory_statement_list(statements: &[Statement<'_>]) -> StatementListInventory {
    let mut inventory = InventoryVisitor::default();
    for stmt in statements {
        inventory.visit_statement(stmt);
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
struct InventoryVisitor {
    bindings: Vec<FunctionBindingRecord>,
    references: Vec<FunctionReferenceRecord>,
    return_sites: Vec<FunctionReturnSite>,
    writes: Vec<FunctionWriteRecord>,
    effects: Vec<FunctionEffectRecord>,
    control: Vec<FunctionControlRegion>,
    /// Indices into `control` of the currently open regions (innermost
    /// last) — a `return` marks every one of them.
    control_stack: Vec<usize>,
}

impl<'a> Visit<'a> for InventoryVisitor {
    fn visit_function(&mut self, _it: &Function<'a>, _flags: oxc_syntax::scope::ScopeFlags) {
        // Nested function body: not this frame. (visit_function is only
        // reached for nested positions — the entry's own body is driven
        // statement-by-statement.)
    }

    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _it: &Class<'a>) {}

    fn visit_statement(&mut self, it: &Statement<'a>) {
        if let Statement::FunctionDeclaration(func) = it {
            if let Some(id) = func.id.as_ref() {
                self.bindings.push(FunctionBindingRecord {
                    name: Arc::from(id.name.as_str()),
                    kind: FunctionBindingKind::NestedFunction,
                    span: id.span.into(),
                });
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
                span: it.span().into(),
            });
            self.control_stack.push(self.control.len() - 1);
        }
        walk::walk_statement(self, it);
        if kind.is_some() {
            self.control_stack.pop();
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
        for declarator in &it.declarations {
            if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                self.bindings.push(FunctionBindingRecord {
                    name: Arc::from(id.name.as_str()),
                    kind,
                    span: id.span.into(),
                });
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
            span: it.span.into(),
        });
        walk::walk_return_statement(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.references.push(FunctionReferenceRecord {
            name: Arc::from(it.name.as_str()),
            span: it.span.into(),
        });
    }

    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        self.writes.push(FunctionWriteRecord {
            span: it.span.into(),
        });
        walk::walk_assignment_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &oxc_ast::ast::UpdateExpression<'a>) {
        self.writes.push(FunctionWriteRecord {
            span: it.span.into(),
        });
        walk::walk_update_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
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
            span: it.span.into(),
            callee,
        });
        walk::walk_call_expression(self, it);
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

/// The function node a [`FunctionBodyLocator`] descent lands on — the ONE
/// authored-position view every per-function body product (skeleton
/// build, lazy body lowering) reads from the retained snapshot.
pub enum FunctionNode<'a> {
    /// A `function` declaration / expression or a class/object method.
    Function(&'a Function<'a>),
    /// An arrow function.
    Arrow(&'a ArrowFunctionExpression<'a>),
}

impl<'a> FunctionNode<'a> {
    /// The formal parameters.
    #[must_use]
    pub fn params(&self) -> &'a oxc_ast::ast::FormalParameters<'a> {
        match self {
            Self::Function(func) => &func.params,
            Self::Arrow(arrow) => &arrow.params,
        }
    }

    /// The function body (`None` for a bodiless overload signature).
    #[must_use]
    pub fn body(&self) -> Option<&'a oxc_ast::ast::FunctionBody<'a>> {
        match self {
            Self::Function(func) => func.body.as_deref(),
            Self::Arrow(arrow) => Some(&arrow.body),
        }
    }

    /// Whether this is an expression-bodied arrow (`(x) => x * 2`).
    #[must_use]
    pub fn is_expression_body(&self) -> bool {
        matches!(self, Self::Arrow(arrow) if arrow.expression)
    }

    /// The function's own type parameter clause, when authored.
    #[must_use]
    pub fn type_parameters(&self) -> Option<&oxc_ast::ast::TSTypeParameterDeclaration<'a>> {
        match self {
            Self::Function(func) => func.type_parameters.as_deref(),
            Self::Arrow(arrow) => arrow.type_parameters.as_deref(),
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
    Class(&'a Class<'a>),
    /// A namespace (`TSModuleDeclaration`).
    Module(&'a oxc_ast::ast::TSModuleDeclaration<'a>),
    /// An `export default { … }` object expression.
    ExportDefaultObject(&'a oxc_ast::ast::ObjectExpression<'a>),
}

fn declaration_of<'a>(statement: &'a Statement<'a>) -> Option<DeclRef<'a>> {
    use oxc_ast::ast::{Declaration, ExportDefaultDeclarationKind};
    match statement {
        Statement::FunctionDeclaration(func) => Some(DeclRef::Function(func)),
        Statement::VariableDeclaration(decl) => Some(DeclRef::Variable(decl)),
        Statement::ClassDeclaration(class) => Some(DeclRef::Class(class)),
        Statement::TSModuleDeclaration(module) => Some(DeclRef::Module(module)),
        Statement::ExportNamedDeclaration(export) => match export.declaration.as_ref()? {
            Declaration::FunctionDeclaration(func) => Some(DeclRef::Function(func)),
            Declaration::VariableDeclaration(decl) => Some(DeclRef::Variable(decl)),
            Declaration::ClassDeclaration(class) => Some(DeclRef::Class(class)),
            Declaration::TSModuleDeclaration(module) => Some(DeclRef::Module(module)),
            _ => None,
        },
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                Some(DeclRef::Function(func))
            }
            ExportDefaultDeclarationKind::ClassDeclaration(class) => Some(DeclRef::Class(class)),
            other => match other.as_expression() {
                Some(Expression::ObjectExpression(obj)) => Some(DeclRef::ExportDefaultObject(obj)),
                _ => None,
            },
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

/// One resolved function position: the node, its bare-identifier self
/// name, and the type-parameter clause of the declaration ENCLOSING it.
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
}

/// Resolve one function's locator against the retained snapshot: the
/// contributing top-level statement, then the ordinal descent. Also
/// derives the function's bare-identifier SELF name for direct-recursion
/// detection: a function declaration contributes its id, a variable
/// initializer its declarator binding; class members and object-literal
/// members have no bare-identifier self name. Any miss is a typed `None`.
pub fn resolve_function_node<'a>(
    program: &'a oxc_ast::ast::Program<'a>,
    locator: &FunctionBodyLocator,
) -> Option<ResolvedFunctionNode<'a>> {
    use oxc_ast::ast::{ClassElement, TSModuleDeclarationBody};
    let mut statement = program
        .body
        .get(locator.contributor.contributor_index as usize)?;
    let mut steps = locator.descent.iter();
    loop {
        match steps.next()? {
            FunctionDescentStep::NamespaceMember { statement_ordinal } => {
                let DeclRef::Module(module) = declaration_of(statement)? else {
                    return None;
                };
                let Some(TSModuleDeclarationBody::TSModuleBlock(block)) = module.body.as_ref()
                else {
                    return None;
                };
                statement = block.body.get(*statement_ordinal as usize)?;
            }
            FunctionDescentStep::FunctionDeclaration => {
                // Terminal step: the statement IS the function declaration.
                if steps.next().is_some() {
                    return None;
                }
                let DeclRef::Function(func) = declaration_of(statement)? else {
                    return None;
                };
                let self_name = func.id.as_ref().map(|id| Arc::from(id.name.as_str()));
                return Some(ResolvedFunctionNode {
                    node: FunctionNode::Function(func),
                    self_name,
                    enclosing_type_parameters: None,
                });
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
                match steps.next() {
                    None => {
                        return Some(ResolvedFunctionNode {
                            node: function_from_expression(init)?,
                            self_name,
                            enclosing_type_parameters: None,
                        });
                    }
                    Some(FunctionDescentStep::ObjectMember { member_ordinal }) => {
                        // Terminal step: the object-literal member inside
                        // the current initializer object expression.
                        if steps.next().is_some() {
                            return None;
                        }
                        let Expression::ObjectExpression(obj) = init else {
                            return None;
                        };
                        let prop = obj.properties.get(*member_ordinal as usize)?;
                        let ObjectPropertyKind::ObjectProperty(property) = prop else {
                            return None;
                        };
                        // Object members have no bare-identifier self name.
                        return Some(ResolvedFunctionNode {
                            node: function_from_expression(&property.value)?,
                            self_name: None,
                            enclosing_type_parameters: None,
                        });
                    }
                    Some(_) => return None,
                }
            }
            FunctionDescentStep::ClassMember { member_ordinal } => {
                // Terminal step: the class member at `member_ordinal`.
                if steps.next().is_some() {
                    return None;
                }
                let DeclRef::Class(class) = declaration_of(statement)? else {
                    return None;
                };
                let element = class.body.body.get(*member_ordinal as usize)?;
                let node = match element {
                    ClassElement::MethodDefinition(method) => FunctionNode::Function(&method.value),
                    ClassElement::PropertyDefinition(property) => {
                        function_from_expression(property.value.as_ref()?)?
                    }
                    _ => return None,
                };
                // Class members have no bare-identifier self name. The
                // CLASS's own type-parameter clause binds throughout every
                // member body, so it rides out with the node.
                return Some(ResolvedFunctionNode {
                    node,
                    self_name: None,
                    enclosing_type_parameters: class.type_parameters.as_deref(),
                });
            }
            FunctionDescentStep::ExportDefaultObjectMember { member_ordinal } => {
                // Terminal step: the object-literal method at
                // `member_ordinal` inside the `export default { … }`
                // object expression.
                if steps.next().is_some() {
                    return None;
                }
                let DeclRef::ExportDefaultObject(obj) = declaration_of(statement)? else {
                    return None;
                };
                let prop = obj.properties.get(*member_ordinal as usize)?;
                let ObjectPropertyKind::ObjectProperty(property) = prop else {
                    return None;
                };
                // Object members have no bare-identifier self name.
                return Some(ResolvedFunctionNode {
                    node: function_from_expression(&property.value)?,
                    self_name: None,
                    enclosing_type_parameters: None,
                });
            }
            FunctionDescentStep::ObjectMember { .. } => {
                // Only valid immediately after a VariableInitializer step
                // (handled there).
                return None;
            }
        }
    }
}
