//! Syntax-only script routing facts.
//!
//! This module records authored import/export routes from one already-parsed
//! OXC [`Program`]. It owns no declaration inventory, body dependencies, raw
//! source surfaces, spans, or resolution logic. Ambiguity and cross-file
//! target selection belong to the semantic/session route authority.
use verter_session_query::analysis::route_inventory::{
    ProgramReferences, RouteCapability, RouteImportForm, RouteImportedName,
    ScriptExportAssignmentRoute, ScriptImportRoute, ScriptLocalExportRoute, ScriptReexportRoute,
    ScriptRouteInventory, ScriptSideEffectImport, ScriptWildcardRoute,
};

use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Declaration, ExportDefaultDeclarationKind,
    Expression, ImportDeclarationSpecifier, ImportExpression, ImportOrExportKind, Program,
    Statement, TSImportType,
};
use oxc_ast_visit::Visit;
use oxc_span::GetSpan;
use verter_type_expr::TopLevelOwnerId;

/// The routing capability an import's kind grants.
const fn route_capability_from_import_kind(kind: ImportOrExportKind) -> RouteCapability {
    match kind {
        ImportOrExportKind::Type => RouteCapability::TypeOnly,
        ImportOrExportKind::Value => RouteCapability::TypeAndValue,
    }
}

/// The routing capability an export's kind grants.
const fn route_capability_from_export_kind(kind: ImportOrExportKind) -> RouteCapability {
    match kind {
        ImportOrExportKind::Type => RouteCapability::TypeOnly,
        ImportOrExportKind::Value => RouteCapability::TypeAndValue,
    }
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

/// Whether a module body or an ambient namespace body holds an EXPORT
/// DECLARATION — `export { … }` (with or without a source), `export * from`,
/// `export =`, or `export default <expression>` — as opposed to a
/// declaration carrying an `export` modifier. An ambient body without one
/// is an export context: TypeScript exports each of its declarations,
/// written `export` or not.
#[must_use]
pub fn statements_have_export_declarations(statements: &[Statement<'_>]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::ExportNamedDeclaration(_) | Statement::ExportFromDeclaration(_) => true,
        Statement::ExportAllDeclaration(_) | Statement::TSExportAssignment(_) => true,
        Statement::ExportDefaultDeclaration(declaration) => {
            declaration.declaration.as_expression().is_some()
        }
        _ => false,
    })
}

fn build_script_route_inventory_impl<I>(program: &Program<'_>, owners: I) -> ScriptRouteInventory
where
    I: Iterator<Item = TopLevelOwnerId>,
{
    let mut inventory = ScriptRouteInventory::default();
    inventory.counts.top_level_statement_count = program.body.len();
    // A declaration file's module body without an export declaration is an
    // export context: its declarations are exported, written `export` or not.
    let implicit_exports = program.source_type.is_typescript_definition()
        && !statements_have_export_declarations(&program.body);
    let mut unexported_declarations: Vec<(&Declaration<'_>, TopLevelOwnerId)> = Vec::new();

    for (statement, owner) in program.body.iter().zip(owners) {
        if implicit_exports {
            if let Some(declaration) = statement.as_declaration() {
                unexported_declarations.push((declaration, owner));
            }
        }
        inventory.has_module_syntax |= matches!(
            statement,
            Statement::ImportDeclaration(_)
                | Statement::ExportDeclaration(_)
                | Statement::ExportNamedDeclaration(_)
                | Statement::ExportFromDeclaration(_)
                | Statement::ExportDefaultDeclaration(_)
                | Statement::ExportAllDeclaration(_)
                | Statement::TSExportAssignment(_)
        ) || matches!(statement, Statement::TSImportEqualsDeclaration(declaration)
            if matches!(declaration.module_reference,
                oxc_ast::ast::TSModuleReference::ExternalModuleReference(_)));
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
                                capability: route_capability_from_import_kind(
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
                                capability: route_capability_from_import_kind(
                                    declaration.import_kind,
                                ),
                                imported: RouteImportedName::Namespace,
                            }
                        }
                    };
                    inventory.imports.push(route);
                }
            }
            Statement::TSImportEqualsDeclaration(declaration) => {
                if let oxc_ast::ast::TSModuleReference::ExternalModuleReference(reference) =
                    &declaration.module_reference
                {
                    inventory.imports.push(ScriptImportRoute {
                        owner,
                        local: declaration.id.name.to_string(),
                        source: reference.expression.value.to_string(),
                        form: RouteImportForm::ImportEquals,
                        capability: route_capability_from_import_kind(declaration.import_kind),
                        imported: RouteImportedName::Namespace,
                    });
                }
            }
            Statement::ExportFromDeclaration(declaration) => {
                let source = &declaration.source;
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
            }
            Statement::ExportNamedDeclaration(declaration) => {
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
            }
            Statement::ExportDeclaration(declaration) => {
                record_exported_declaration(
                    &declaration.declaration,
                    owner,
                    &mut inventory.local_exports,
                );
            }
            Statement::ExportAllDeclaration(declaration) => {
                inventory.wildcard_reexports.push(ScriptWildcardRoute {
                    owner,
                    source: declaration.source.value.to_string(),
                    capability: route_capability_from_export_kind(declaration.export_kind),
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

    if inventory.has_module_syntax {
        for (declaration, owner) in unexported_declarations {
            record_exported_declaration(declaration, owner, &mut inventory.local_exports);
        }
    }

    inventory.counts.import_binding_count = inventory.imports.len();
    inventory.counts.bindingless_import_count = inventory.bindingless_imports.len();
    inventory.counts.direct_reexport_count = inventory.reexports.len();
    inventory.counts.wildcard_reexport_count = inventory.wildcard_reexports.len();
    inventory.counts.local_export_count = inventory.local_exports.len();
    inventory.counts.export_assignment_count = inventory.export_assignments.len();
    inventory.program_references = build_program_references(program, inventory.has_module_syntax);
    inventory
}

/// The files `program` pulls into its TypeScript program, in the order
/// TypeScript's program construction visits them (`findSourceFile`):
/// triple-slash `path`, `types` and `lib` references from the file's leading
/// comments, then the module names `collectExternalModuleReferences`
/// gathers — import / re-export / `import x = require(…)` specifiers in
/// statement order (an ambient `declare module "m" { … }` body contributes
/// its non-relative specifiers), dynamic `import("m")` calls and
/// `import("m")` types in source order (plus `require("m")` in a JavaScript
/// file).
///
/// The names of `declare module "m"` augmentations are not among them:
/// TypeScript resolves an augmentation's name to find the module it merges
/// into, but adds only imported files to the program
/// (`processImportedModules` adds a resolution only below
/// `file.imports.length`), so an augmentation alone never places its
/// target.
///
/// `is_external_module` is whether the file has top-level module syntax: a
/// `declare module "m"` in a module augments `m`, in a script it declares
/// an ambient module whose body may import.
#[must_use]
pub fn build_program_references(
    program: &Program<'_>,
    is_external_module: bool,
) -> ProgramReferences {
    let mut references = ProgramReferences::default();
    collect_triple_slash_references(program, &mut references);
    collect_module_references(
        &program.body,
        false,
        is_external_module,
        program.source_type.is_typescript_definition(),
        &mut references.module_names,
    );
    let mut dynamic = DynamicModuleNames {
        javascript: program.source_type.is_javascript(),
        names: Vec::new(),
    };
    // A walk refused its stack collects nothing; the operation around it is
    // refused with the refusal.
    let _ = crate::oxc_parse::leased_program_walk(program, || dynamic.visit_program(program));
    dynamic.names.sort_by_key(|(start, _)| *start);
    references
        .module_names
        .extend(dynamic.names.into_iter().map(|(_, name)| name));
    references
}

/// TypeScript reads `/// <reference …/>` pragmas only from the comments
/// before the file's first token (`getLeadingCommentRanges(text, 0)`, which
/// steps over a hashbang).
fn collect_triple_slash_references(program: &Program<'_>, references: &mut ProgramReferences) {
    let first_token = program
        .directives
        .first()
        .map(|directive| directive.span.start)
        .into_iter()
        .chain(program.body.first().map(|statement| statement.span().start))
        .min()
        .unwrap_or(program.span.end);
    for comment in program.comments.iter() {
        if comment.span.end > first_token {
            break;
        }
        if !comment.is_line() {
            continue;
        }
        let Some(text) = program
            .source_text
            .get(comment.span.start as usize..comment.span.end as usize)
        else {
            continue;
        };
        let Some(arguments) = reference_pragma_arguments(text) else {
            continue;
        };
        // `processPragmasIntoFields`: one directive names one reference,
        // preferring `no-default-lib`, then `types`, `lib`, `path`.
        if named_pragma_argument(arguments, "no-default-lib").is_some_and(|value| value == "true") {
            continue;
        }
        if let Some(types) = named_pragma_argument(arguments, "types") {
            references.types.push(types.to_owned());
        } else if let Some(lib) = named_pragma_argument(arguments, "lib") {
            references.libs.push(lib.to_owned());
        } else if let Some(path) = named_pragma_argument(arguments, "path") {
            references.paths.push(path.to_owned());
        }
    }
}

/// The text after `<reference` of a `/// <reference … />` comment —
/// TypeScript's `^\/\/\/\s*<(\S+)\s.*?\/>` with the element named
/// `reference` (case-insensitively) — or `None` for any other comment.
fn reference_pragma_arguments(comment: &str) -> Option<&str> {
    let rest = comment.strip_prefix("///")?.trim_start();
    let rest = rest.strip_prefix('<')?;
    let name_end = rest.find(char::is_whitespace)?;
    if !rest[..name_end].eq_ignore_ascii_case("reference") {
        return None;
    }
    let arguments = &rest[name_end..];
    arguments.contains("/>").then_some(arguments)
}

/// The value of the first `name="…"` / `name='…'` argument preceded by
/// whitespace — TypeScript's `(\s{name}\s*=\s*)(?:'([^']*)'|"([^"]*)")`,
/// matched case-insensitively.
fn named_pragma_argument<'t>(arguments: &'t str, name: &str) -> Option<&'t str> {
    let bytes = arguments.as_bytes();
    let mut from = 0;
    while from < bytes.len() {
        let at = from + arguments[from..].find(|c: char| c.is_whitespace())?;
        let mut cursor = at + arguments[at..].chars().next()?.len_utf8();
        from = cursor;
        let Some(candidate) = arguments.get(cursor..cursor + name.len()) else {
            continue;
        };
        if !candidate.eq_ignore_ascii_case(name) {
            continue;
        }
        cursor += name.len();
        let after_name = arguments[cursor..].trim_start();
        let Some(after_equals) = after_name.strip_prefix('=') else {
            continue;
        };
        let value = after_equals.trim_start();
        let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        let body = &value[1..];
        if let Some(end) = body.find(quote) {
            return Some(&body[..end]);
        }
    }
    None
}

/// `collectModuleReferences`: import-like statements in order, descending
/// into an ambient module body of a script (whose relative specifiers are
/// ignored); a module's `declare module "m"` (or an ambient module nested
/// in one) is an augmentation of `m`.
fn collect_module_references(
    statements: &[Statement<'_>],
    in_ambient_module: bool,
    is_external_module: bool,
    is_declaration_file: bool,
    imports: &mut Vec<String>,
) {
    let push_import = |imports: &mut Vec<String>, name: &str| {
        if !name.is_empty() && (!in_ambient_module || !is_relative_module_name(name)) {
            imports.push(name.to_owned());
        }
    };
    for statement in statements {
        let module = match statement {
            Statement::ImportDeclaration(declaration) => {
                push_import(imports, &declaration.source.value);
                continue;
            }
            Statement::TSImportEqualsDeclaration(declaration) => {
                if let oxc_ast::ast::TSModuleReference::ExternalModuleReference(reference) =
                    &declaration.module_reference
                {
                    push_import(imports, &reference.expression.value);
                }
                continue;
            }
            Statement::ExportFromDeclaration(declaration) => {
                push_import(imports, &declaration.source.value);
                continue;
            }
            Statement::ExportAllDeclaration(declaration) => {
                push_import(imports, &declaration.source.value);
                continue;
            }
            Statement::TSExternalModuleDeclaration(module) => module,
            Statement::ExportDeclaration(declaration) => match &declaration.declaration {
                Declaration::TSExternalModuleDeclaration(module) => module,
                _ => continue,
            },
            _ => continue,
        };
        if !(in_ambient_module || module.declare || is_declaration_file) {
            continue;
        }
        // A module augmentation (`declare module "m"` in a module, or a
        // non-relative one nested in an ambient module) imports nothing; an
        // ambient module a script declares may.
        if !is_external_module && !in_ambient_module {
            if let Some(body) = &module.body {
                collect_module_references(
                    &body.body,
                    true,
                    is_external_module,
                    is_declaration_file,
                    imports,
                );
            }
        }
    }
}

/// `isExternalModuleNameRelative`.
fn is_relative_module_name(name: &str) -> bool {
    let rooted =
        name.starts_with('/') || name.starts_with('\\') || name.as_bytes().get(1) == Some(&b':');
    rooted || name.starts_with("./") || name.starts_with("../") || name == "." || name == ".."
}

/// `collectDynamicImportOrRequireOrJsDocImportCalls`: dynamic `import()`
/// calls and `import()` types naming a string literal, and `require()` in
/// a JavaScript file, keyed by source position.
struct DynamicModuleNames {
    javascript: bool,
    names: Vec<(u32, String)>,
}

impl<'a> Visit<'a> for DynamicModuleNames {
    fn visit_import_expression(&mut self, it: &ImportExpression<'a>) {
        let name = match &it.source {
            Expression::StringLiteral(literal) => Some(literal.value.to_string()),
            Expression::TemplateLiteral(template) => {
                template.single_quasi().map(|quasi| quasi.to_string())
            }
            _ => None,
        };
        if let Some(name) = name {
            self.names.push((it.span.start, name));
        }
        oxc_ast_visit::walk::walk_import_expression(self, it);
    }

    fn visit_ts_import_type(&mut self, it: &TSImportType<'a>) {
        self.names
            .push((it.span.start, it.source.value.to_string()));
        oxc_ast_visit::walk::walk_ts_import_type(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if self.javascript && it.is_require_call() {
            let name = match it.arguments.first() {
                Some(Argument::StringLiteral(literal)) => Some(literal.value.to_string()),
                Some(Argument::TemplateLiteral(template)) => {
                    template.single_quasi().map(|quasi| quasi.to_string())
                }
                _ => None,
            };
            if let Some(name) = name {
                self.names.push((it.span.start, name));
            }
        }
        oxc_ast_visit::walk::walk_call_expression(self, it);
    }
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
        Declaration::TSNamespaceDeclaration(declaration) => {
            record(declaration.id.name.as_str(), RouteCapability::TypeAndValue);
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
        ExportDefaultDeclarationKind::ClassDeclaration(_) => {
            ("default", RouteCapability::TypeAndValue)
        }
        ExportDefaultDeclarationKind::FunctionDeclaration(function) => (
            function
                .id
                .as_ref()
                .map_or("default", |identifier| identifier.name.as_str()),
            RouteCapability::ValueOnly,
        ),
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => {
            ("default", RouteCapability::TypeOnly)
        }
        ExportDefaultDeclarationKind::Identifier(identifier) => {
            (identifier.name.as_str(), RouteCapability::ValueOnly)
        }
        other if other.as_expression().is_some() => ("default", RouteCapability::ValueOnly),
        _ => return,
    };
    routes.push(ScriptLocalExportRoute {
        owner,
        exported: "default".to_string(),
        local: local.to_string(),
        capability,
    });
}
