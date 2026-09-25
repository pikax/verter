//! Ambient-augmentation header indexing — the `declare global { ... }` /
//! `declare module "X" { ... }` inner-declaration walkers.
//!
//! Extracted from `decl_headers.rs` (same module, sibling file). These
//! register an augmentation block's inner interfaces / type-aliases / value
//! statements (and nested `namespace N { ... }` members under their qualified
//! `Ns.Member` names) into the `DeclHeaderIndex` augmentation-scope
//! inventories, mirroring the whole-env augmentation walk in
//! `crate::analysis::type_eval_build::build_eval_env`.

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
        span: block.span.into(),
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
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = export.declaration.as_ref() {
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
                        Declaration::TSModuleDeclaration(module) => {
                            index_augmentation_module_declaration(module, ctx, index, scope, None);
                        }
                        _ => {}
                    }
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
            Statement::TSModuleDeclaration(module) => {
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
fn index_augmentation_module_declaration(
    decl: &TSModuleDeclaration<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    scope: &AugmentationScopeKind,
    prefix: Option<&str>,
) {
    let namespace = match &decl.id {
        TSModuleDeclarationName::Identifier(id) => match prefix {
            Some(prefix) => format!("{prefix}.{}", id.name),
            None => id.name.to_string(),
        },
        TSModuleDeclarationName::StringLiteral(_) => return,
    };
    let Some(body) = decl.body.as_ref() else {
        return;
    };
    if matches!(scope, AugmentationScopeKind::Module(_)) {
        index.augmentation_namespace_blocks.push((
            scope.clone(),
            NamespaceBlockRecord {
                owner: ctx.anchor.owner,
                qualified_name: namespace.clone(),
                span: decl.span.into(),
            },
        ));
    }
    match body {
        TSModuleDeclarationBody::TSModuleDeclaration(inner) => {
            index_augmentation_module_declaration(
                inner,
                ctx,
                index,
                scope,
                Some(namespace.as_str()),
            );
        }
        TSModuleDeclarationBody::TSModuleBlock(block) => {
            let implicit_export = !statements_have_export_declarations(&block.body);
            for stmt in &block.body {
                index_namespaced_statement_into_augmentation(
                    stmt,
                    ctx,
                    index,
                    namespace.as_str(),
                    scope,
                    implicit_export,
                );
            }
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
        Statement::TSModuleDeclaration(module) => {
            index_augmentation_module_declaration(module, ctx, index, scope, Some(namespace));
        }
        // An augmentation block is ambient, so a namespace body inside it
        // without an export declaration exports every member, written
        // `export` or not (mirror of an ambient namespace in
        // `index_namespaced_statement`).
        Statement::ExportNamedDeclaration(export) => {
            if let Some(ref decl) = export.declaration {
                index_namespaced_declaration_into_augmentation(decl, ctx, index, namespace, scope);
            }
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
        Declaration::TSModuleDeclaration(module) => {
            index_augmentation_module_declaration(module, ctx, index, scope, Some(namespace));
        }
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
            span: cls.span.into(),
            name_span: id.span.into(),
            object_member_headers: Vec::new(),
            contributors: Vec::new(),
        });
    entry.kind = ValueDeclKind::Class;
    entry.span = cls.span.into();
    entry.name_span = id.span.into();
    push_contributor(
        &mut entry.contributors,
        ctx,
        cls.span.into(),
        id.span.into(),
    );
}
