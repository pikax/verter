//! Shared, scope-aware Svelte 5 reactivity-mode classification.
//!
//! Svelte decides between legacy and runes mode from unresolved rune-name
//! references after store subscriptions have been classified. Framework
//! consumers must not maintain their own textual or scope-blind versions of
//! that rule: the runtime compiler, IDE projector, and script-fact capture all
//! consume this module.

use oxc_ast::ast::{
    ArrowFunctionExpression, BindingPattern, BlockStatement, CatchClause, Class, Declaration,
    Expression, ForInStatement, ForOfStatement, ForStatement, Function, FunctionType,
    IdentifierReference, ImportDeclarationSpecifier, Program, Statement, VariableDeclaration,
    VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use rustc_hash::FxHashSet;

/// Svelte 5 rune root names used by official mode inference.
pub const RUNE_ROOT_NAMES: &[&str] = &[
    "$state",
    "$derived",
    "$props",
    "$effect",
    "$bindable",
    "$inspect",
    "$host",
];

/// The resolved Svelte component reactivity mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SvelteReactivityMode {
    /// Legacy `export let` / `$:` semantics.
    Legacy,
    /// Svelte 5 runes semantics.
    Runes,
}

impl SvelteReactivityMode {
    /// Whether this is runes mode.
    #[must_use]
    pub const fn is_runes(self) -> bool {
        matches!(self, Self::Runes)
    }
}

/// Infer mode from a position-preserving combined script program.
///
/// `module_region` identifies the `<script module>` body. The two script slots
/// are separate JavaScript modules for binding purposes even though Verter's
/// shallow parse uses one position-preserving program; each slot therefore
/// receives its own top-level lexical frame. A declaration in one slot can
/// never shadow a rune reference in the other.
#[must_use]
pub fn infer_combined_program_mode(
    program: &Program<'_>,
    module_region: Option<(u32, u32)>,
    forced_runes: Option<bool>,
    template_uses_host_rune: bool,
) -> SvelteReactivityMode {
    if let Some(forced) = forced_runes {
        return if forced {
            SvelteReactivityMode::Runes
        } else {
            SvelteReactivityMode::Legacy
        };
    }
    let store_exempt = combined_store_rune_exemptions(program, module_region);
    if template_uses_host_rune && !store_exempt.contains("$host") {
        return SvelteReactivityMode::Runes;
    }
    if partitioned_program_uses_runes(program, module_region, &store_exempt) {
        SvelteReactivityMode::Runes
    } else {
        SvelteReactivityMode::Legacy
    }
}

/// Detect rune use in one ordinary script program using the shared lexical
/// classifier. `store_exempt` contains full accessor names such as `$state`.
#[must_use]
pub fn script_uses_runes(program: &Program<'_>, store_exempt: &FxHashSet<String>) -> bool {
    let statements = program.body.iter().collect::<Vec<_>>();
    statements_use_runes(&statements, store_exempt)
}

/// Detect rune use across a combined position-preserving program while
/// preserving the instance/module top-level scope boundary.
#[must_use]
pub fn partitioned_program_uses_runes(
    program: &Program<'_>,
    module_region: Option<(u32, u32)>,
    store_exempt: &FxHashSet<String>,
) -> bool {
    let mut instance = Vec::new();
    let mut module = Vec::new();
    for statement in &program.body {
        if statement_in_module(statement.span().start, module_region) {
            module.push(statement);
        } else {
            instance.push(statement);
        }
    }
    statements_use_runes(&instance, store_exempt) || statements_use_runes(&module, store_exempt)
}

/// Compute the rune-root store accessor exemptions from a combined program.
///
/// Imports from both script slots are candidates; declarations only from the
/// instance slot are candidates. Rune-initialized declarators are excluded,
/// and the official `derived` import from `svelte/store` exception is retained.
#[must_use]
pub fn combined_store_rune_exemptions(
    program: &Program<'_>,
    module_region: Option<(u32, u32)>,
) -> FxHashSet<String> {
    let mut bases = FxHashSet::default();
    for statement in &program.body {
        if let Statement::ImportDeclaration(import) = statement {
            collect_import_store_bases(import, &mut bases);
        }
        if statement_in_module(statement.span().start, module_region) {
            continue;
        }
        collect_instance_store_bases(statement, &mut bases);
    }
    RUNE_ROOT_NAMES
        .iter()
        .filter(|rune| bases.contains(&rune[1..]))
        .map(|rune| (*rune).to_string())
        .collect()
}

fn statement_in_module(start: u32, module_region: Option<(u32, u32)>) -> bool {
    matches!(module_region, Some((region_start, region_end)) if start >= region_start && start < region_end)
}

fn collect_import_store_bases(
    import: &oxc_ast::ast::ImportDeclaration<'_>,
    out: &mut FxHashSet<String>,
) {
    if import.import_kind.is_type() || import.phase.is_some() {
        return;
    }
    let Some(specifiers) = &import.specifiers else {
        return;
    };
    let source = import.source.value.as_str();
    for specifier in specifiers {
        let local = match specifier {
            ImportDeclarationSpecifier::ImportSpecifier(named) if !named.import_kind.is_type() => {
                named.local.name.as_str()
            }
            ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
                default.local.name.as_str()
            }
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                namespace.local.name.as_str()
            }
            ImportDeclarationSpecifier::ImportSpecifier(_) => continue,
        };
        if local == "derived" && source == "svelte/store" {
            continue;
        }
        admit_store_base(local, out);
    }
}

fn collect_instance_store_bases(statement: &Statement<'_>, out: &mut FxHashSet<String>) {
    match statement {
        Statement::VariableDeclaration(declaration) => {
            collect_variable_store_bases(declaration, out);
        }
        Statement::FunctionDeclaration(function) => {
            if let Some(id) = &function.id {
                admit_store_base(id.name.as_str(), out);
            }
        }
        Statement::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                admit_store_base(id.name.as_str(), out);
            }
        }
        Statement::ExportNamedDeclaration(export) => match &export.declaration {
            Some(Declaration::VariableDeclaration(declaration)) => {
                collect_variable_store_bases(declaration, out);
            }
            Some(Declaration::FunctionDeclaration(function)) => {
                if let Some(id) = &function.id {
                    admit_store_base(id.name.as_str(), out);
                }
            }
            Some(Declaration::ClassDeclaration(class)) => {
                if let Some(id) = &class.id {
                    admit_store_base(id.name.as_str(), out);
                }
            }
            _ => {}
        },
        _ => {}
    }
}

fn collect_variable_store_bases(
    declaration: &VariableDeclaration<'_>,
    out: &mut FxHashSet<String>,
) {
    for declarator in &declaration.declarations {
        if declarator.init.as_ref().is_some_and(init_is_rune_call) {
            continue;
        }
        let mut names = Vec::new();
        collect_pattern_names(&declarator.id, &mut names);
        for name in names {
            admit_store_base(&name, out);
        }
    }
}

fn admit_store_base(name: &str, out: &mut FxHashSet<String>) {
    if !name.starts_with('$') {
        out.insert(name.to_string());
    }
}

fn init_is_rune_call(expression: &Expression<'_>) -> bool {
    let mut expression = expression;
    while let Expression::ParenthesizedExpression(parenthesized) = expression {
        expression = &parenthesized.expression;
    }
    let Expression::CallExpression(call) = expression else {
        return false;
    };
    let root = match &call.callee {
        Expression::Identifier(identifier) => identifier.name.as_str(),
        Expression::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(identifier) => identifier.name.as_str(),
            _ => return false,
        },
        _ => return false,
    };
    RUNE_ROOT_NAMES.contains(&root)
}

fn statements_use_runes(statements: &[&Statement<'_>], store_exempt: &FxHashSet<String>) -> bool {
    let mut top_level = FxHashSet::default();
    collect_direct_decls_refs(statements, &mut top_level);
    collect_var_hoists_refs(statements, &mut top_level);
    let mut detector = ScopeAwareRuneDetector {
        used: false,
        scopes: vec![top_level],
        store_exempt,
    };
    for statement in statements {
        detector.visit_statement(statement);
        if detector.used {
            break;
        }
    }
    detector.used
}

struct ScopeAwareRuneDetector<'s> {
    used: bool,
    scopes: Vec<FxHashSet<String>>,
    store_exempt: &'s FxHashSet<String>,
}

impl ScopeAwareRuneDetector<'_> {
    fn is_unshadowed_rune(&self, name: &str) -> bool {
        RUNE_ROOT_NAMES.contains(&name)
            && !self.store_exempt.contains(name)
            && !self.scopes.iter().rev().any(|scope| scope.contains(name))
    }
}

impl<'a> Visit<'a> for ScopeAwareRuneDetector<'_> {
    fn visit_function(&mut self, function: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.scopes.push(function_scope_names(function));
        walk::walk_function(self, function, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        self.scopes.push(arrow_scope_names(arrow));
        walk::walk_arrow_function_expression(self, arrow);
        self.scopes.pop();
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        let mut frame = FxHashSet::default();
        if !class.is_declaration() {
            if let Some(id) = &class.id {
                frame.insert(id.name.to_string());
            }
        }
        self.scopes.push(frame);
        walk::walk_class(self, class);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        self.scopes.push(block_scope_names(block));
        walk::walk_block_statement(self, block);
        self.scopes.pop();
    }

    fn visit_catch_clause(&mut self, clause: &CatchClause<'a>) {
        let mut frame = FxHashSet::default();
        if let Some(parameter) = &clause.param {
            let mut names = Vec::new();
            collect_pattern_names(&parameter.pattern, &mut names);
            frame.extend(names);
        }
        self.scopes.push(frame);
        walk::walk_catch_clause(self, clause);
        self.scopes.pop();
    }

    fn visit_for_statement(&mut self, statement: &ForStatement<'a>) {
        let mut frame = FxHashSet::default();
        if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(declaration)) =
            &statement.init
        {
            if !matches!(declaration.kind, VariableDeclarationKind::Var) {
                for declarator in &declaration.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&declarator.id, &mut names);
                    frame.extend(names);
                }
            }
        }
        self.scopes.push(frame);
        walk::walk_for_statement(self, statement);
        self.scopes.pop();
    }

    fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'a>) {
        self.scopes.push(for_left_names(&statement.left));
        walk::walk_for_of_statement(self, statement);
        self.scopes.pop();
    }

    fn visit_for_in_statement(&mut self, statement: &ForInStatement<'a>) {
        self.scopes.push(for_left_names(&statement.left));
        walk::walk_for_in_statement(self, statement);
        self.scopes.pop();
    }

    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        if self.is_unshadowed_rune(identifier.name.as_str()) {
            self.used = true;
        }
        walk::walk_identifier_reference(self, identifier);
    }
}

fn function_scope_names(function: &Function<'_>) -> FxHashSet<String> {
    let mut names = parameter_names(&function.params);
    if !matches!(function.r#type, FunctionType::FunctionDeclaration) {
        if let Some(id) = &function.id {
            names.insert(id.name.to_string());
        }
    }
    if let Some(body) = &function.body {
        collect_direct_decls(&body.statements, &mut names);
        collect_var_hoists(&body.statements, &mut names);
    }
    names
}

fn arrow_scope_names(arrow: &ArrowFunctionExpression<'_>) -> FxHashSet<String> {
    let mut names = parameter_names(&arrow.params);
    collect_direct_decls(&arrow.body.statements, &mut names);
    collect_var_hoists(&arrow.body.statements, &mut names);
    names
}

fn block_scope_names(block: &BlockStatement<'_>) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    collect_direct_decls(&block.body, &mut names);
    names
}

fn parameter_names(parameters: &oxc_ast::ast::FormalParameters<'_>) -> FxHashSet<String> {
    let mut names = Vec::new();
    for parameter in &parameters.items {
        collect_pattern_names(&parameter.pattern, &mut names);
    }
    if let Some(rest) = &parameters.rest {
        collect_pattern_names(&rest.rest.argument, &mut names);
    }
    names.into_iter().collect()
}

fn collect_direct_decls(statements: &[Statement<'_>], out: &mut FxHashSet<String>) {
    for statement in statements {
        collect_direct_decl(statement, out);
    }
}

fn collect_direct_decls_refs(statements: &[&Statement<'_>], out: &mut FxHashSet<String>) {
    for statement in statements {
        collect_direct_decl(statement, out);
    }
}

fn collect_direct_decl(statement: &Statement<'_>, out: &mut FxHashSet<String>) {
    match statement {
        Statement::ImportDeclaration(import) => {
            if let Some(specifiers) = &import.specifiers {
                for specifier in specifiers {
                    let local = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(named) => &named.local,
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
                            &default.local
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                            &namespace.local
                        }
                    };
                    out.insert(local.name.to_string());
                }
            }
        }
        Statement::VariableDeclaration(declaration) => {
            record_lexical_variable_names(declaration, out);
        }
        Statement::FunctionDeclaration(function) => record_function_id(function, out),
        Statement::ClassDeclaration(class) => record_class_id(class, out),
        Statement::ExportNamedDeclaration(export) => {
            if let Some(declaration) = &export.declaration {
                record_declaration_names(declaration, out);
            }
        }
        _ => {}
    }
}

fn record_lexical_variable_names(
    declaration: &VariableDeclaration<'_>,
    out: &mut FxHashSet<String>,
) {
    if matches!(declaration.kind, VariableDeclarationKind::Var) {
        return;
    }
    for declarator in &declaration.declarations {
        let mut names = Vec::new();
        collect_pattern_names(&declarator.id, &mut names);
        out.extend(names);
    }
}

fn record_function_id(function: &Function<'_>, out: &mut FxHashSet<String>) {
    if let Some(id) = &function.id {
        out.insert(id.name.to_string());
    }
}

fn record_class_id(class: &Class<'_>, out: &mut FxHashSet<String>) {
    if let Some(id) = &class.id {
        out.insert(id.name.to_string());
    }
}

fn record_declaration_names(declaration: &Declaration<'_>, out: &mut FxHashSet<String>) {
    match declaration {
        Declaration::VariableDeclaration(variable) => {
            record_lexical_variable_names(variable, out);
        }
        Declaration::FunctionDeclaration(function) => record_function_id(function, out),
        Declaration::ClassDeclaration(class) => record_class_id(class, out),
        _ => {}
    }
}

fn collect_var_hoists(statements: &[Statement<'_>], out: &mut FxHashSet<String>) {
    let mut scan = VarHoistScan { out };
    for statement in statements {
        scan.visit_statement(statement);
    }
}

fn collect_var_hoists_refs(statements: &[&Statement<'_>], out: &mut FxHashSet<String>) {
    let mut scan = VarHoistScan { out };
    for statement in statements {
        scan.visit_statement(statement);
    }
}

struct VarHoistScan<'o> {
    out: &'o mut FxHashSet<String>,
}

impl<'a> Visit<'a> for VarHoistScan<'_> {
    fn visit_variable_declaration(&mut self, declaration: &VariableDeclaration<'a>) {
        if matches!(declaration.kind, VariableDeclarationKind::Var) {
            for declarator in &declaration.declarations {
                let mut names = Vec::new();
                collect_pattern_names(&declarator.id, &mut names);
                self.out.extend(names);
            }
        }
    }

    fn visit_function(&mut self, _function: &Function<'a>, _flags: oxc_syntax::scope::ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _arrow: &ArrowFunctionExpression<'a>) {}
}

fn for_left_names(left: &oxc_ast::ast::ForStatementLeft<'_>) -> FxHashSet<String> {
    let mut frame = FxHashSet::default();
    if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left {
        if !matches!(declaration.kind, VariableDeclarationKind::Var) {
            for declarator in &declaration.declarations {
                let mut names = Vec::new();
                collect_pattern_names(&declarator.id, &mut names);
                frame.extend(names);
            }
        }
    }
    frame
}

fn collect_pattern_names(pattern: &BindingPattern<'_>, out: &mut Vec<String>) {
    match pattern {
        BindingPattern::BindingIdentifier(identifier) => out.push(identifier.name.to_string()),
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                collect_pattern_names(&property.value, out);
            }
            if let Some(rest) = &object.rest {
                collect_pattern_names(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                collect_pattern_names(element, out);
            }
            if let Some(rest) = &array.rest {
                collect_pattern_names(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            collect_pattern_names(&assignment.left, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    use super::*;

    fn parse(source: &str) -> (Allocator, String) {
        (Allocator::default(), source.to_string())
    }

    #[test]
    fn shadowed_rune_parameter_does_not_force_runes_mode() {
        let (allocator, source) = parse("function f($state) { return $state; }");
        let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
        assert_eq!(
            infer_combined_program_mode(&parsed.program, None, None, false),
            SvelteReactivityMode::Legacy
        );
    }

    #[test]
    fn module_binding_cannot_shadow_instance_rune_reference() {
        let source = "const $state = 1;\nlet value = $state(0);";
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        let module_end = source.find('\n').expect("two statements") as u32;
        assert_eq!(
            infer_combined_program_mode(&parsed.program, Some((0, module_end)), None, false),
            SvelteReactivityMode::Runes
        );
    }

    #[test]
    fn rune_named_store_accessor_remains_legacy() {
        let source = "const state = makeStore(); let value = $state;";
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        assert_eq!(
            infer_combined_program_mode(&parsed.program, None, None, false),
            SvelteReactivityMode::Legacy
        );
    }
}
