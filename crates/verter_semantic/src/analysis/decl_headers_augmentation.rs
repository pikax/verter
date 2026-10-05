//! Ambient-augmentation header indexing — the `declare global { ... }` /
//! `declare module "X" { ... }` inner-declaration walkers.
//!
//! Extracted from `decl_headers.rs` (same module, sibling file). These
//! register an augmentation block's inner interfaces / type-aliases / value
//! statements (and nested `namespace N { ... }` members under their qualified
//! `Ns.Member` names) into the `DeclHeaderIndex` augmentation-scope
//! inventories, mirroring the whole-env augmentation walk in
//! `crate::analysis::type_eval_build::build_eval_env`.
use verter_session_query::declarations::header_index::AugmentationBlockRecord;

use super::*;

/// The names an ambient module block exports as its `default`
/// (`export default function / class Name`) and assigns the module to
/// (`export = X`).
pub(crate) fn ambient_block_module_exports(
    block: &TSModuleBlock<'_>,
) -> (Option<String>, Option<String>) {
    let mut default_export = None;
    let mut export_assignment = None;
    for stmt in &block.body {
        match stmt {
            Statement::ExportDefaultDeclaration(export) => {
                default_export = match &export.declaration {
                    oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                        func.id.as_ref().map(|id| id.name.to_string())
                    }
                    oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(cls) => {
                        cls.id.as_ref().map(|id| id.name.to_string())
                    }
                    oxc_ast::ast::ExportDefaultDeclarationKind::Identifier(id) => {
                        Some(id.name.to_string())
                    }
                    _ => None,
                };
            }
            Statement::TSExportAssignment(assignment) => {
                if let oxc_ast::ast::Expression::Identifier(id) = &assignment.expression {
                    export_assignment = Some(id.name.to_string());
                }
            }
            _ => {}
        }
    }
    (default_export, export_assignment)
}

/// Mirror of `extract_augmentation_block` + `extract_augmentation_declaration`
/// + `retain_value_statement_into_augmentation`.
///
/// Inner interfaces / type-aliases register under the TYPE augmentation
/// scope; inner value statements (`const`/`let`/`var`, `function`,
/// `class`) register under the VALUE augmentation scope. An inner class
/// registers ONLY its value side (the env walk drops the throwaway type
/// side).
pub(super) fn index_augmentation_block(
    block: &TSModuleBlock<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    scope: &AugmentationScopeKind,
) {
    // Record the augmentation BLOCK itself (EMPTY blocks included — an
    // empty `declare module "X" {}` / `declare global {}` still
    // introduces the augmentation scope + target).
    let (default_export, export_assignment) = ambient_block_module_exports(block);
    index.augmentation_blocks.push(AugmentationBlockRecord {
        scope: scope.clone(),
        owner: ctx.anchor.owner,
        span: verter_span::Span::new(block.span.start, block.span.end),
        default_export,
        export_assignment,
    });
    for stmt in &block.body {
        // `export default function / class Name` declares `Name` in the
        // block, as the unexported declaration does.
        if let Statement::ExportDefaultDeclaration(export) = stmt {
            match &export.declaration {
                oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(func)
                    if func.id.is_some() =>
                {
                    let scoped = index
                        .augmentation_value_headers
                        .entry(scope.clone())
                        .or_default();
                    index_function(func, ctx, scoped);
                }
                oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(cls)
                    if cls.id.is_some() =>
                {
                    index_augmentation_class_value(cls, ctx, index, scope);
                }
                _ => {}
            }
            continue;
        }
        match stmt {
            Statement::TSInterfaceDeclaration(iface) => {
                let scoped = index
                    .augmentation_type_headers
                    .entry(scope.clone())
                    .or_default();
                index_interface(iface, iface.id.name.as_str(), ctx, scoped);
            }
            Statement::TSTypeAliasDeclaration(alias) => {
                let scoped = index
                    .augmentation_type_headers
                    .entry(scope.clone())
                    .or_default();
                index_type_alias(alias, alias.id.name.as_str(), ctx, scoped);
            }
            Statement::ExportDeclaration(export) => {
                let decl = &export.declaration;
                match decl {
                    Declaration::TSInterfaceDeclaration(iface) => {
                        let scoped = index
                            .augmentation_type_headers
                            .entry(scope.clone())
                            .or_default();
                        index_interface(iface, iface.id.name.as_str(), ctx, scoped);
                    }
                    Declaration::TSTypeAliasDeclaration(alias) => {
                        let scoped = index
                            .augmentation_type_headers
                            .entry(scope.clone())
                            .or_default();
                        index_type_alias(alias, alias.id.name.as_str(), ctx, scoped);
                    }
                    Declaration::VariableDeclaration(var_decl) => {
                        let scoped = index
                            .augmentation_value_headers
                            .entry(scope.clone())
                            .or_default();
                        for d in &var_decl.declarations {
                            index_variable(d, var_decl.kind, ctx, scoped, None);
                        }
                    }
                    Declaration::FunctionDeclaration(func) => {
                        let scoped = index
                            .augmentation_value_headers
                            .entry(scope.clone())
                            .or_default();
                        index_function(func, ctx, scoped);
                    }
                    Declaration::ClassDeclaration(cls) => {
                        index_augmentation_class_value(cls, ctx, index, scope);
                    }
                    Declaration::TSNamespaceDeclaration(module) => {
                        index_augmentation_module_declaration(module, ctx, index, scope, None);
                    }
                    _ => {}
                }
            }
            Statement::VariableDeclaration(var_decl) => {
                let scoped = index
                    .augmentation_value_headers
                    .entry(scope.clone())
                    .or_default();
                for d in &var_decl.declarations {
                    index_variable(d, var_decl.kind, ctx, scoped, None);
                }
            }
            Statement::FunctionDeclaration(func) => {
                let scoped = index
                    .augmentation_value_headers
                    .entry(scope.clone())
                    .or_default();
                index_function(func, ctx, scoped);
            }
            Statement::ClassDeclaration(cls) => {
                index_augmentation_class_value(cls, ctx, index, scope);
            }
            // A namespace nested inside an ambient augmentation block
            // (`declare global { namespace JSX { ... } }`) registers its inner
            // members under their qualified `Ns.Member` names into the SAME
            // augmentation scope. Mirror of
            // `extract_augmentation_module_declaration` (the body builder); the
            // two MUST agree on the qualified key so `has_global_augmentation`
            // and the lazy body memo resolve the same `(scope, name)` identity.
            Statement::TSNamespaceDeclaration(module) => {
                index_augmentation_module_declaration(module, ctx, index, scope, None);
            }
            _ => {}
        }
    }
}

/// Mirror of `extract_augmentation_module_declaration`: a `namespace N { ... }`
/// nested inside an ambient augmentation block registers its inner type/value
/// members under their qualified `Ns.Member` names in the augmentation
/// inventory. A string-literal module name nested here contributes nothing.
/// The namespace and every namespace nested in it are walked from an
/// explicit stack ([`for_each_namespace`]).
fn index_augmentation_module_declaration(
    decl: &TSNamespaceDeclaration<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    scope: &AugmentationScopeKind,
    prefix: Option<&str>,
) {
    for_each_namespace(
        decl,
        &mut AugmentationNamespaces {
            ctx,
            index,
            scope,
            path: QualifiedPath::under(prefix),
        },
    );
}

/// [`index_augmentation_module_declaration`]'s walk.
struct AugmentationNamespaces<'i, 'c> {
    ctx: HeaderStatementContext<'c>,
    index: &'i mut DeclHeaderIndex,
    scope: &'i AugmentationScopeKind,
    /// The qualified name of the namespace being walked.
    path: QualifiedPath,
}

/// One namespace of an augmentation block being indexed.
struct AugmentationNamespace {
    /// Its block's record in [`DeclHeaderIndex::augmentation_namespace_blocks`]
    /// (a `declare module` block's namespaces only).
    record: Option<usize>,
    /// The qualified path's length before it was entered.
    enclosing: usize,
    implicit_export: bool,
    /// Whether what it declares so far instantiates it.
    instantiated: bool,
}

impl<'s, 'a> NamespaceVisitor<'s, 'a> for AugmentationNamespaces<'_, '_> {
    type Frame = AugmentationNamespace;

    fn enter(
        &mut self,
        decl: &'s TSNamespaceDeclaration<'a>,
        _parent: Option<&AugmentationNamespace>,
        _nesting: Nesting,
    ) -> AugmentationNamespace {
        let enclosing = self.path.enter(decl.id.name.as_str());
        let record = matches!(self.scope, AugmentationScopeKind::Module(_)).then(|| {
            self.index.augmentation_namespace_blocks.push((
                self.scope.clone(),
                NamespaceBlockRecord {
                    owner: self.ctx.anchor.owner,
                    qualified_name: self.path.name().to_owned(),
                    span: verter_span::Span::new(decl.span.start, decl.span.end),
                    instantiated: false,
                },
            ));
            self.index.augmentation_namespace_blocks.len() - 1
        });
        let implicit_export = match &decl.body {
            TSNamespaceDeclarationBody::TSNamespaceDeclaration(_) => false,
            TSNamespaceDeclarationBody::TSModuleBlock(block) => {
                !statements_have_export_declarations(&block.body)
            }
        };
        AugmentationNamespace {
            record,
            enclosing,
            implicit_export,
            instantiated: false,
        }
    }

    fn statement(&mut self, frame: &mut AugmentationNamespace, statement: &'s Statement<'a>) {
        frame.instantiated |= statement_instantiates_here(statement);
        index_namespaced_statement_into_augmentation(
            statement,
            self.ctx,
            self.index,
            self.path.name(),
            self.scope,
            frame.implicit_export,
        );
    }

    fn exit(&mut self, frame: AugmentationNamespace, parent: Option<&mut AugmentationNamespace>) {
        self.path.leave(frame.enclosing);
        if let Some(record) = frame.record {
            self.index.augmentation_namespace_blocks[record]
                .1
                .instantiated = frame.instantiated;
        }
        if let Some(parent) = parent {
            parent.instantiated |= frame.instantiated;
        }
    }
}

/// Augmentation-scope mirror of `index_namespaced_statement`.
fn index_namespaced_statement_into_augmentation(
    stmt: &Statement<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    namespace: &str,
    scope: &AugmentationScopeKind,
    implicit_export: bool,
) {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => {
            let name = format!("{namespace}.{}", alias.id.name);
            let scoped = index
                .augmentation_type_headers
                .entry(scope.clone())
                .or_default();
            index_type_alias(alias, name.as_str(), ctx, scoped);
        }
        Statement::TSInterfaceDeclaration(iface) => {
            let name = format!("{namespace}.{}", iface.id.name);
            let scoped = index
                .augmentation_type_headers
                .entry(scope.clone())
                .or_default();
            index_interface(iface, name.as_str(), ctx, scoped);
        }
        // A nested namespace is a frame of
        // [`index_augmentation_module_declaration`]'s walk.
        Statement::TSNamespaceDeclaration(_) => {}
        // An augmentation block is ambient, so a namespace body inside it
        // without an export declaration exports every member, written
        // `export` or not (mirror of an ambient namespace in
        // `index_namespaced_statement`).
        Statement::ExportDeclaration(export) => {
            let decl = &export.declaration;
            index_namespaced_declaration_into_augmentation(decl, ctx, index, namespace, scope);
        }
        Statement::VariableDeclaration(var_decl) if implicit_export => {
            let scoped = index
                .augmentation_value_headers
                .entry(scope.clone())
                .or_default();
            for d in &var_decl.declarations {
                index_variable(d, var_decl.kind, ctx, scoped, Some(namespace));
            }
        }
        Statement::FunctionDeclaration(func) if implicit_export => {
            let scoped = index
                .augmentation_value_headers
                .entry(scope.clone())
                .or_default();
            index_function_in(func, ctx, scoped, Some(namespace));
        }
        _ => {}
    }
}

/// Augmentation-scope mirror of `index_namespaced_declaration`.
fn index_namespaced_declaration_into_augmentation(
    decl: &Declaration<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    namespace: &str,
    scope: &AugmentationScopeKind,
) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            let name = format!("{namespace}.{}", alias.id.name);
            let scoped = index
                .augmentation_type_headers
                .entry(scope.clone())
                .or_default();
            index_type_alias(alias, name.as_str(), ctx, scoped);
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            let name = format!("{namespace}.{}", iface.id.name);
            let scoped = index
                .augmentation_type_headers
                .entry(scope.clone())
                .or_default();
            index_interface(iface, name.as_str(), ctx, scoped);
        }
        // A nested namespace is a frame of
        // [`index_augmentation_module_declaration`]'s walk.
        Declaration::TSNamespaceDeclaration(_) => {}
        Declaration::VariableDeclaration(var_decl) => {
            let scoped = index
                .augmentation_value_headers
                .entry(scope.clone())
                .or_default();
            for d in &var_decl.declarations {
                index_variable(d, var_decl.kind, ctx, scoped, Some(namespace));
            }
        }
        Declaration::FunctionDeclaration(func) => {
            let scoped = index
                .augmentation_value_headers
                .entry(scope.clone())
                .or_default();
            index_function_in(func, ctx, scoped, Some(namespace));
        }
        _ => {}
    }
}

/// An ambient-augmentation class contributes its value side and the
/// instance type it declares to the block's scope (mirrors
/// `move_value_parts_into_augmentation`).
fn index_augmentation_class_value(
    cls: &Class<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    scope: &AugmentationScopeKind,
) {
    let Some(id) = &cls.id else {
        return;
    };
    // The file-scope class walk computes the instance type header; it is
    // re-homed into the block's type scope.
    let mut scratch = DeclHeaderIndex::default();
    index_named_class(cls, id.name.as_str(), ctx, &mut scratch);
    if let Some(header) = scratch
        .type_headers
        .get(&ctx.key(id.name.as_str()))
        .cloned()
    {
        upsert_type_header(
            index
                .augmentation_type_headers
                .entry(scope.clone())
                .or_default(),
            id.name.as_str(),
            header.kind,
            header.span,
            header.name_span,
            header.type_params,
            header.member_headers,
            &[],
            ctx,
        );
    }
    let scoped = index
        .augmentation_value_headers
        .entry(scope.clone())
        .or_default();
    let entry = scoped
        .entry(ctx.key(id.name.as_str()))
        .or_insert_with(|| ValueDeclHeader {
            kind: ValueDeclKind::Class,
            span: verter_span::Span::new(cls.span.start, cls.span.end),
            name_span: verter_span::Span::new(id.span.start, id.span.end),
            object_member_headers: Vec::new(),
            contributors: Vec::new(),
        });
    entry.kind = ValueDeclKind::Class;
    entry.span = verter_span::Span::new(cls.span.start, cls.span.end);
    entry.name_span = verter_span::Span::new(id.span.start, id.span.end);
    push_contributor(
        &mut entry.contributors,
        ctx,
        verter_span::Span::new(cls.span.start, cls.span.end),
        verter_span::Span::new(id.span.start, id.span.end),
    );
}
