//! Syntax-only script routing facts.
//!
//! This module records authored import/export routes from one already-parsed
//! OXC [`Program`]. It owns no declaration inventory, body dependencies, raw
//! source surfaces, spans, or resolution logic. Ambiguity and cross-file
//! target selection belong to the semantic/session route authority.

use oxc_ast::ast::{
    BindingPattern, Declaration, ExportDefaultDeclarationKind, Expression,
    ImportDeclarationSpecifier, ImportOrExportKind, Program, Statement, TSModuleDeclarationName,
};
use verter_type_expr::TopLevelOwnerId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RouteCapability {
    TypeOnly,
    ValueOnly,
    TypeAndValue,
}

impl RouteCapability {
    const fn from_import_kind(kind: ImportOrExportKind) -> Self {
        match kind {
            ImportOrExportKind::Type => Self::TypeOnly,
            ImportOrExportKind::Value => Self::TypeAndValue,
        }
    }

    const fn from_export_kind(kind: ImportOrExportKind) -> Self {
        match kind {
            ImportOrExportKind::Type => Self::TypeOnly,
            ImportOrExportKind::Value => Self::TypeAndValue,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RouteImportForm {
    Named,
    Default,
    Namespace,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RouteImportedName {
    Namespace,
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptImportRoute {
    pub owner: TopLevelOwnerId,
    pub local: String,
    pub source: String,
    pub form: RouteImportForm,
    pub capability: RouteCapability,
    pub imported: RouteImportedName,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptSideEffectImport {
    pub owner: TopLevelOwnerId,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptReexportRoute {
    pub owner: TopLevelOwnerId,
    pub exported: String,
    pub source: String,
    pub imported: String,
    pub capability: RouteCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptWildcardRoute {
    pub owner: TopLevelOwnerId,
    pub source: String,
    pub capability: RouteCapability,
    pub exported_namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptLocalExportRoute {
    pub owner: TopLevelOwnerId,
    pub exported: String,
    pub local: String,
    pub capability: RouteCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptExportAssignmentRoute {
    pub owner: TopLevelOwnerId,
    pub local: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ScriptRouteCounts {
    pub top_level_statement_count: usize,
    pub import_binding_count: usize,
    pub bindingless_import_count: usize,
    pub direct_reexport_count: usize,
    pub wildcard_reexport_count: usize,
    pub local_export_count: usize,
    pub export_assignment_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScriptRouteInventory {
    pub imports: Vec<ScriptImportRoute>,
    pub bindingless_imports: Vec<ScriptSideEffectImport>,
    pub reexports: Vec<ScriptReexportRoute>,
    pub wildcard_reexports: Vec<ScriptWildcardRoute>,
    pub local_exports: Vec<ScriptLocalExportRoute>,
    pub export_assignments: Vec<ScriptExportAssignmentRoute>,
    pub counts: ScriptRouteCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteOwnerTableError {
    statement_count: usize,
    owner_count: usize,
}

impl RouteOwnerTableError {
    #[must_use]
    pub const fn statement_count(self) -> usize {
        self.statement_count
    }

    #[must_use]
    pub const fn owner_count(self) -> usize {
        self.owner_count
    }
}

impl std::fmt::Display for RouteOwnerTableError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "route owner table has {} entries for {} statements",
            self.owner_count, self.statement_count
        )
    }
}

impl std::error::Error for RouteOwnerTableError {}

#[must_use]
pub fn build_script_route_inventory(program: &Program<'_>) -> ScriptRouteInventory {
    build_script_route_inventory_for_owner(program, TopLevelOwnerId::ordinary_file())
}

#[must_use]
pub fn build_script_route_inventory_for_owner(
    program: &Program<'_>,
    owner: TopLevelOwnerId,
) -> ScriptRouteInventory {
    build_script_route_inventory_impl(program, std::iter::repeat_n(owner, program.body.len()))
}

pub fn build_script_route_inventory_with_owners(
    program: &Program<'_>,
    owners: &[TopLevelOwnerId],
) -> Result<ScriptRouteInventory, RouteOwnerTableError> {
    build_script_route_inventory_with_owner_iter(program, owners.iter().copied())
}

/// Build routes from an exact-size owner stream parallel to `Program.body`.
/// This lets higher-level retained-program publishers reuse their validated
/// owner table without allocating a second owner vector.
pub fn build_script_route_inventory_with_owner_iter<I>(
    program: &Program<'_>,
    owners: I,
) -> Result<ScriptRouteInventory, RouteOwnerTableError>
where
    I: ExactSizeIterator<Item = TopLevelOwnerId>,
{
    let owner_count = owners.len();
    if program.body.len() != owner_count {
        return Err(RouteOwnerTableError {
            statement_count: program.body.len(),
            owner_count,
        });
    }
    Ok(build_script_route_inventory_impl(program, owners))
}

fn build_script_route_inventory_impl<I>(program: &Program<'_>, owners: I) -> ScriptRouteInventory
where
    I: Iterator<Item = TopLevelOwnerId>,
{
    let mut inventory = ScriptRouteInventory::default();
    inventory.counts.top_level_statement_count = program.body.len();

    for (statement, owner) in program.body.iter().zip(owners) {
        match statement {
            Statement::ImportDeclaration(declaration) => {
                let Some(specifiers) = &declaration.specifiers else {
                    inventory.bindingless_imports.push(ScriptSideEffectImport {
                        owner,
                        source: declaration.source.value.to_string(),
                    });
                    continue;
                };
                if specifiers.is_empty() {
                    inventory.bindingless_imports.push(ScriptSideEffectImport {
                        owner,
                        source: declaration.source.value.to_string(),
                    });
                    continue;
                }
                for specifier in specifiers {
                    let route = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                            ScriptImportRoute {
                                owner,
                                local: specifier.local.name.to_string(),
                                source: declaration.source.value.to_string(),
                                form: RouteImportForm::Named,
                                capability: if declaration.import_kind == ImportOrExportKind::Type
                                    || specifier.import_kind == ImportOrExportKind::Type
                                {
                                    RouteCapability::TypeOnly
                                } else {
                                    RouteCapability::TypeAndValue
                                },
                                imported: RouteImportedName::Name(
                                    specifier.imported.name().to_string(),
                                ),
                            }
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                            ScriptImportRoute {
                                owner,
                                local: specifier.local.name.to_string(),
                                source: declaration.source.value.to_string(),
                                form: RouteImportForm::Default,
                                capability: RouteCapability::from_import_kind(
                                    declaration.import_kind,
                                ),
                                imported: RouteImportedName::Name("default".to_string()),
                            }
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                            ScriptImportRoute {
                                owner,
                                local: specifier.local.name.to_string(),
                                source: declaration.source.value.to_string(),
                                form: RouteImportForm::Namespace,
                                capability: RouteCapability::from_import_kind(
                                    declaration.import_kind,
                                ),
                                imported: RouteImportedName::Namespace,
                            }
                        }
                    };
                    inventory.imports.push(route);
                }
            }
            Statement::ExportNamedDeclaration(declaration) => {
                if let Some(source) = &declaration.source {
                    for specifier in &declaration.specifiers {
                        inventory.reexports.push(ScriptReexportRoute {
                            owner,
                            exported: specifier.exported.name().to_string(),
                            source: source.value.to_string(),
                            imported: specifier.local.name().to_string(),
                            capability: if declaration.export_kind == ImportOrExportKind::Type
                                || specifier.export_kind == ImportOrExportKind::Type
                            {
                                RouteCapability::TypeOnly
                            } else {
                                RouteCapability::TypeAndValue
                            },
                        });
                    }
                    continue;
                }

                for specifier in &declaration.specifiers {
                    inventory.local_exports.push(ScriptLocalExportRoute {
                        owner,
                        exported: specifier.exported.name().to_string(),
                        local: specifier.local.name().to_string(),
                        capability: if declaration.export_kind == ImportOrExportKind::Type
                            || specifier.export_kind == ImportOrExportKind::Type
                        {
                            RouteCapability::TypeOnly
                        } else {
                            RouteCapability::TypeAndValue
                        },
                    });
                }
                if let Some(declaration) = &declaration.declaration {
                    record_exported_declaration(declaration, owner, &mut inventory.local_exports);
                }
            }
            Statement::ExportAllDeclaration(declaration) => {
                inventory.wildcard_reexports.push(ScriptWildcardRoute {
                    owner,
                    source: declaration.source.value.to_string(),
                    capability: RouteCapability::from_export_kind(declaration.export_kind),
                    exported_namespace: declaration
                        .exported
                        .as_ref()
                        .map(|name| name.name().to_string()),
                });
            }
            Statement::ExportDefaultDeclaration(declaration) => {
                record_default_export(declaration, owner, &mut inventory.local_exports);
            }
            Statement::TSExportAssignment(assignment) => {
                if let Expression::Identifier(identifier) = &assignment.expression {
                    inventory
                        .export_assignments
                        .push(ScriptExportAssignmentRoute {
                            owner,
                            local: identifier.name.to_string(),
                        });
                }
            }
            _ => {}
        }
    }

    inventory.counts.import_binding_count = inventory.imports.len();
    inventory.counts.bindingless_import_count = inventory.bindingless_imports.len();
    inventory.counts.direct_reexport_count = inventory.reexports.len();
    inventory.counts.wildcard_reexport_count = inventory.wildcard_reexports.len();
    inventory.counts.local_export_count = inventory.local_exports.len();
    inventory.counts.export_assignment_count = inventory.export_assignments.len();
    inventory
}

fn record_exported_declaration(
    declaration: &Declaration<'_>,
    owner: TopLevelOwnerId,
    routes: &mut Vec<ScriptLocalExportRoute>,
) {
    let mut record = |name: &str, capability: RouteCapability| {
        routes.push(ScriptLocalExportRoute {
            owner,
            exported: name.to_string(),
            local: name.to_string(),
            capability,
        });
    };

    match declaration {
        Declaration::TSTypeAliasDeclaration(declaration) => {
            record(declaration.id.name.as_str(), RouteCapability::TypeOnly);
        }
        Declaration::TSInterfaceDeclaration(declaration) => {
            record(declaration.id.name.as_str(), RouteCapability::TypeOnly);
        }
        Declaration::TSEnumDeclaration(declaration) => {
            record(declaration.id.name.as_str(), RouteCapability::TypeAndValue);
        }
        Declaration::ClassDeclaration(declaration) => {
            if let Some(identifier) = &declaration.id {
                record(identifier.name.as_str(), RouteCapability::TypeAndValue);
            }
        }
        Declaration::FunctionDeclaration(declaration) => {
            if let Some(identifier) = &declaration.id {
                record(identifier.name.as_str(), RouteCapability::ValueOnly);
            }
        }
        Declaration::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                if let BindingPattern::BindingIdentifier(identifier) = &declarator.id {
                    record(identifier.name.as_str(), RouteCapability::ValueOnly);
                }
            }
        }
        Declaration::TSModuleDeclaration(declaration) => {
            if let TSModuleDeclarationName::Identifier(identifier) = &declaration.id {
                record(identifier.name.as_str(), RouteCapability::TypeAndValue);
            }
        }
        _ => {}
    }
}

fn record_default_export(
    declaration: &oxc_ast::ast::ExportDefaultDeclaration<'_>,
    owner: TopLevelOwnerId,
    routes: &mut Vec<ScriptLocalExportRoute>,
) {
    let (local, capability) = match &declaration.declaration {
        ExportDefaultDeclarationKind::ClassDeclaration(class) => (
            class
                .id
                .as_ref()
                .map_or("default", |identifier| identifier.name.as_str()),
            RouteCapability::TypeAndValue,
        ),
        ExportDefaultDeclarationKind::FunctionDeclaration(function) => (
            function
                .id
                .as_ref()
                .map_or("default", |identifier| identifier.name.as_str()),
            RouteCapability::ValueOnly,
        ),
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(interface) => {
            (interface.id.name.as_str(), RouteCapability::TypeOnly)
        }
        ExportDefaultDeclarationKind::Identifier(identifier) => {
            (identifier.name.as_str(), RouteCapability::ValueOnly)
        }
        _ => return,
    };
    routes.push(ScriptLocalExportRoute {
        owner,
        exported: "default".to_string(),
        local: local.to_string(),
        capability,
    });
}
