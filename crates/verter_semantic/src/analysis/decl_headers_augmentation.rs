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
    for stmt in &block.body {
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
            for stmt in &block.body {
                index_namespaced_statement_into_augmentation(
                    stmt,
                    ctx,
                    index,
                    namespace.as_str(),
                    scope,
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
        // Export-only namespace value indexing (mirror of
        // `index_namespaced_statement`).
        Statement::ExportNamedDeclaration(export) => {
            if let Some(ref decl) = export.declaration {
                index_namespaced_declaration_into_augmentation(decl, ctx, index, namespace, scope);
            }
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
        _ => {}
    }
}

/// An ambient-augmentation class contributes its VALUE side only (mirrors
/// `move_value_symbols_into_augmentation`, which drops the type side).
fn index_augmentation_class_value(
    cls: &Class<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    scope: &AugmentationScopeKind,
) {
    let Some(id) = &cls.id else {
        return;
    };
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
