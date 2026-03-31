//! Helper functions for binding extraction.
//!
//! This module contains shared helper functions used by the slot, v-for,
//! and expression extraction modules.

use oxc_ast::ast::*;
use rustc_hash::FxHashSet;

use super::keywords::{is_global, is_keyword};
use crate::common::Span;

/// Collect which setup binding names are referenced as free variables in a program.
///
/// Walks all statements in the program, tracking inner scopes (function params,
/// block-scoped variables) to avoid false positives from shadowed identifiers.
/// Top-level declarations are the setup bindings themselves and are NOT treated
/// as inner-scope shadows.
///
/// Returns the subset of `setup_names` that appear as free references.
pub fn collect_setup_binding_refs<'a>(
    program: &'a Program<'a>,
    setup_names: &FxHashSet<&str>,
) -> FxHashSet<&'a str> {
    let mut collector = SetupRefCollector {
        setup_names,
        local_scopes: Vec::new(),
        refs: FxHashSet::default(),
    };
    for stmt in &program.body {
        collector.visit_statement(stmt, /* top_level */ true);
    }
    collector.refs
}

struct SetupRefCollector<'a, 'b> {
    setup_names: &'b FxHashSet<&'b str>,
    local_scopes: Vec<FxHashSet<&'a str>>,
    refs: FxHashSet<&'a str>,
}

impl<'a, 'b> SetupRefCollector<'a, 'b> {
    fn is_locally_declared(&self, name: &str) -> bool {
        self.local_scopes.iter().any(|scope| scope.contains(name))
    }

    fn check_identifier(&mut self, name: &'a str) {
        if self.setup_names.contains(name)
            && !self.is_locally_declared(name)
            && !is_keyword(name.as_bytes())
            && !is_global(name.as_bytes())
        {
            self.refs.insert(name);
        }
    }

    fn visit_statement(&mut self, stmt: &'a Statement<'a>, top_level: bool) {
        match stmt {
            Statement::ExpressionStatement(expr_stmt) => {
                self.visit_expression(&expr_stmt.expression);
            }
            Statement::VariableDeclaration(var_decl) => {
                if !top_level {
                    // Inner scope: add declared names as locals
                    let scope = self
                        .local_scopes
                        .last_mut()
                        .expect("inner var decl requires a scope");
                    for decl in &var_decl.declarations {
                        collect_pattern_locals_into_set(&decl.id, scope);
                    }
                }
                // Visit initializers (they can reference setup bindings)
                for decl in &var_decl.declarations {
                    if let Some(init) = &decl.init {
                        self.visit_expression(init);
                    }
                }
            }
            Statement::ReturnStatement(ret) => {
                if let Some(arg) = &ret.argument {
                    self.visit_expression(arg);
                }
            }
            Statement::IfStatement(if_stmt) => {
                self.visit_expression(&if_stmt.test);
                self.visit_statement(&if_stmt.consequent, false);
                if let Some(alt) = &if_stmt.alternate {
                    self.visit_statement(alt, false);
                }
            }
            Statement::BlockStatement(block) => {
                self.local_scopes.push(FxHashSet::default());
                for s in &block.body {
                    self.visit_statement(s, false);
                }
                self.local_scopes.pop();
            }
            Statement::ForStatement(for_stmt) => {
                self.local_scopes.push(FxHashSet::default());
                if let Some(init) = &for_stmt.init {
                    match init {
                        ForStatementInit::VariableDeclaration(vd) => {
                            {
                                let scope = self.local_scopes.last_mut().unwrap();
                                for decl in &vd.declarations {
                                    collect_pattern_locals_into_set(&decl.id, scope);
                                }
                            }
                            for decl in &vd.declarations {
                                if let Some(init_expr) = &decl.init {
                                    self.visit_expression(init_expr);
                                }
                            }
                        }
                        _ => {
                            if let Some(expr) = init.as_expression() {
                                self.visit_expression(expr);
                            }
                        }
                    }
                }
                if let Some(test) = &for_stmt.test {
                    self.visit_expression(test);
                }
                if let Some(update) = &for_stmt.update {
                    self.visit_expression(update);
                }
                self.visit_statement(&for_stmt.body, false);
                self.local_scopes.pop();
            }
            Statement::ForInStatement(for_in) => {
                self.local_scopes.push(FxHashSet::default());
                if let ForStatementLeft::VariableDeclaration(vd) = &for_in.left {
                    let scope = self.local_scopes.last_mut().unwrap();
                    for decl in &vd.declarations {
                        collect_pattern_locals_into_set(&decl.id, scope);
                    }
                }
                self.visit_expression(&for_in.right);
                self.visit_statement(&for_in.body, false);
                self.local_scopes.pop();
            }
            Statement::ForOfStatement(for_of) => {
                self.local_scopes.push(FxHashSet::default());
                if let ForStatementLeft::VariableDeclaration(vd) = &for_of.left {
                    let scope = self.local_scopes.last_mut().unwrap();
                    for decl in &vd.declarations {
                        collect_pattern_locals_into_set(&decl.id, scope);
                    }
                }
                self.visit_expression(&for_of.right);
                self.visit_statement(&for_of.body, false);
                self.local_scopes.pop();
            }
            Statement::WhileStatement(while_stmt) => {
                self.visit_expression(&while_stmt.test);
                self.visit_statement(&while_stmt.body, false);
            }
            Statement::DoWhileStatement(do_while) => {
                self.visit_statement(&do_while.body, false);
                self.visit_expression(&do_while.test);
            }
            Statement::SwitchStatement(switch) => {
                self.visit_expression(&switch.discriminant);
                for case in &switch.cases {
                    if let Some(test) = &case.test {
                        self.visit_expression(test);
                    }
                    for s in &case.consequent {
                        self.visit_statement(s, false);
                    }
                }
            }
            Statement::ThrowStatement(throw) => {
                self.visit_expression(&throw.argument);
            }
            Statement::TryStatement(try_stmt) => {
                self.local_scopes.push(FxHashSet::default());
                for s in &try_stmt.block.body {
                    self.visit_statement(s, false);
                }
                self.local_scopes.pop();
                if let Some(handler) = &try_stmt.handler {
                    self.local_scopes.push(FxHashSet::default());
                    if let Some(param) = &handler.param {
                        let scope = self.local_scopes.last_mut().unwrap();
                        collect_pattern_locals_into_set(&param.pattern, scope);
                    }
                    for s in &handler.body.body {
                        self.visit_statement(s, false);
                    }
                    self.local_scopes.pop();
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    self.local_scopes.push(FxHashSet::default());
                    for s in &finalizer.body {
                        self.visit_statement(s, false);
                    }
                    self.local_scopes.pop();
                }
            }
            Statement::FunctionDeclaration(func) => {
                self.visit_function_body(func);
            }
            _ => {}
        }
    }

    fn visit_function_body(&mut self, func: &'a Function<'a>) {
        self.local_scopes.push(FxHashSet::default());
        let scope = self.local_scopes.last_mut().unwrap();
        // Add function name to its own scope (for recursion)
        if let Some(id) = &func.id {
            scope.insert(id.name.as_str());
        }
        // Add params
        for param in &func.params.items {
            collect_pattern_locals_into_set(&param.pattern, scope);
        }
        if let Some(rest) = &func.params.rest {
            collect_pattern_locals_into_set(&rest.rest.argument, scope);
        }
        // Visit param defaults
        for param in &func.params.items {
            if let Some(init) = &param.initializer {
                self.visit_expression(init);
            }
        }
        if let Some(body) = &func.body {
            for s in &body.statements {
                self.visit_statement(s, false);
            }
        }
        self.local_scopes.pop();
    }

    fn visit_arrow_function(&mut self, arrow: &'a ArrowFunctionExpression<'a>) {
        self.local_scopes.push(FxHashSet::default());
        let scope = self.local_scopes.last_mut().unwrap();
        for param in &arrow.params.items {
            collect_pattern_locals_into_set(&param.pattern, scope);
        }
        if let Some(rest) = &arrow.params.rest {
            collect_pattern_locals_into_set(&rest.rest.argument, scope);
        }
        // Visit param defaults
        for param in &arrow.params.items {
            if let Some(init) = &param.initializer {
                self.visit_expression(init);
            }
        }
        for s in &arrow.body.statements {
            self.visit_statement(s, false);
        }
        self.local_scopes.pop();
    }

    fn visit_expression(&mut self, expr: &'a Expression<'a>) {
        match expr {
            Expression::Identifier(ident) => {
                self.check_identifier(ident.name.as_str());
            }
            Expression::BinaryExpression(binary) => {
                self.visit_expression(&binary.left);
                self.visit_expression(&binary.right);
            }
            Expression::LogicalExpression(logical) => {
                self.visit_expression(&logical.left);
                self.visit_expression(&logical.right);
            }
            Expression::ConditionalExpression(cond) => {
                self.visit_expression(&cond.test);
                self.visit_expression(&cond.consequent);
                self.visit_expression(&cond.alternate);
            }
            Expression::CallExpression(call) => {
                self.visit_expression(&call.callee);
                for arg in &call.arguments {
                    if let Some(e) = arg.as_expression() {
                        self.visit_expression(e);
                    }
                }
            }
            Expression::StaticMemberExpression(member) => {
                self.visit_expression(&member.object);
            }
            Expression::ComputedMemberExpression(member) => {
                self.visit_expression(&member.object);
                self.visit_expression(&member.expression);
            }
            Expression::ArrayExpression(arr) => {
                for elem in &arr.elements {
                    if let Some(e) = elem.as_expression() {
                        self.visit_expression(e);
                    }
                }
            }
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ObjectPropertyKind::ObjectProperty(p) => {
                            if p.shorthand {
                                if let PropertyKey::StaticIdentifier(ident) = &p.key {
                                    self.check_identifier(ident.name.as_str());
                                }
                            } else {
                                if p.computed {
                                    if let Some(key_expr) = p.key.as_expression() {
                                        self.visit_expression(key_expr);
                                    }
                                }
                                self.visit_expression(&p.value);
                            }
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            self.visit_expression(&spread.argument);
                        }
                    }
                }
            }
            Expression::ParenthesizedExpression(paren) => {
                self.visit_expression(&paren.expression);
            }
            Expression::UnaryExpression(unary) => {
                self.visit_expression(&unary.argument);
            }
            Expression::UpdateExpression(update) => {
                if let SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) = &update.argument
                {
                    self.check_identifier(ident.name.as_str());
                }
            }
            Expression::AssignmentExpression(assign) => {
                self.visit_expression(&assign.right);
                if let AssignmentTarget::AssignmentTargetIdentifier(ident) = &assign.left {
                    self.check_identifier(ident.name.as_str());
                }
            }
            Expression::TemplateLiteral(template) => {
                for e in &template.expressions {
                    self.visit_expression(e);
                }
            }
            Expression::TaggedTemplateExpression(tagged) => {
                self.visit_expression(&tagged.tag);
                for e in &tagged.quasi.expressions {
                    self.visit_expression(e);
                }
            }
            Expression::ArrowFunctionExpression(arrow) => {
                self.visit_arrow_function(arrow);
            }
            Expression::FunctionExpression(func) => {
                self.visit_function_body(func);
            }
            Expression::TSAsExpression(ts_as) => {
                self.visit_expression(&ts_as.expression);
            }
            Expression::TSNonNullExpression(non_null) => {
                self.visit_expression(&non_null.expression);
            }
            Expression::TSSatisfiesExpression(sat) => {
                self.visit_expression(&sat.expression);
            }
            Expression::TSTypeAssertion(assertion) => {
                self.visit_expression(&assertion.expression);
            }
            Expression::AwaitExpression(await_expr) => {
                self.visit_expression(&await_expr.argument);
            }
            Expression::YieldExpression(yield_expr) => {
                if let Some(arg) = &yield_expr.argument {
                    self.visit_expression(arg);
                }
            }
            Expression::SequenceExpression(seq) => {
                for e in &seq.expressions {
                    self.visit_expression(e);
                }
            }
            Expression::NewExpression(new_expr) => {
                self.visit_expression(&new_expr.callee);
                for arg in &new_expr.arguments {
                    if let Some(e) = arg.as_expression() {
                        self.visit_expression(e);
                    }
                }
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::CallExpression(call) => {
                    self.visit_expression(&call.callee);
                    for arg in &call.arguments {
                        if let Some(e) = arg.as_expression() {
                            self.visit_expression(e);
                        }
                    }
                }
                ChainElement::StaticMemberExpression(member) => {
                    self.visit_expression(&member.object);
                }
                ChainElement::ComputedMemberExpression(member) => {
                    self.visit_expression(&member.object);
                    self.visit_expression(&member.expression);
                }
                ChainElement::PrivateFieldExpression(pfe) => {
                    self.visit_expression(&pfe.object);
                }
                ChainElement::TSNonNullExpression(non_null) => {
                    self.visit_expression(&non_null.expression);
                }
            },
            _ => {}
        }
    }
}

/// Helper to collect binding pattern locals into a FxHashSet<&str>.
fn collect_pattern_locals_into_set<'a>(
    pattern: &'a BindingPattern<'a>,
    set: &mut FxHashSet<&'a str>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            set.insert(ident.name.as_str());
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_locals_into_set(&prop.value, set);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_locals_into_set(&rest.argument, set);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_locals_into_set(elem, set);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_locals_into_set(&rest.argument, set);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_pattern_locals_into_set(&assign.left, set);
        }
    }
}

/// Collect local binding names from a binding pattern.
///
/// This extracts all identifiers declared by the pattern itself.
/// For example, in `{ a, b: c }`, this returns `["a", "c"]`.
pub fn collect_pattern_locals<'a>(pattern: &'a BindingPattern<'a>, locals: &mut Vec<&'a str>) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            locals.push(ident.name.as_str());
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_locals(&prop.value, locals);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_locals(&rest.argument, locals);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_locals(elem, locals);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_locals(&rest.argument, locals);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_pattern_locals(&assign.left, locals);
        }
    }
}

/// Collect references from a binding pattern (default values).
///
/// This extracts identifiers that are referenced in default value expressions.
pub fn collect_pattern_references<'a>(
    pattern: &'a BindingPattern<'a>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<&'a str>,
) {
    match pattern {
        BindingPattern::AssignmentPattern(assign) => {
            collect_expression_references(&assign.right, ignored, references);
            collect_pattern_references(&assign.left, ignored, references);
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_references(&prop.value, ignored, references);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_references(&rest.argument, ignored, references);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_references(elem, ignored, references);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_references(&rest.argument, ignored, references);
            }
        }
        BindingPattern::BindingIdentifier(_) => {}
    }
}

/// Collect identifier references from an expression (excluding ignored identifiers).
pub fn collect_expression_references<'a>(
    expr: &'a Expression<'a>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<&'a str>,
) {
    match expr {
        Expression::Identifier(ident) => {
            let name_bytes = ident.name.as_bytes();
            if !ignored.contains(name_bytes) && !is_keyword(name_bytes) && !is_global(name_bytes) {
                references.insert(ident.name.as_str());
            }
        }
        Expression::BinaryExpression(binary) => {
            collect_expression_references(&binary.left, ignored, references);
            collect_expression_references(&binary.right, ignored, references);
        }
        Expression::LogicalExpression(logical) => {
            collect_expression_references(&logical.left, ignored, references);
            collect_expression_references(&logical.right, ignored, references);
        }
        Expression::ConditionalExpression(cond) => {
            collect_expression_references(&cond.test, ignored, references);
            collect_expression_references(&cond.consequent, ignored, references);
            collect_expression_references(&cond.alternate, ignored, references);
        }
        Expression::CallExpression(call) => {
            collect_expression_references(&call.callee, ignored, references);
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_references(expr, ignored, references);
                }
            }
        }
        Expression::StaticMemberExpression(member) => {
            collect_expression_references(&member.object, ignored, references);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_expression_references(&member.object, ignored, references);
            collect_expression_references(&member.expression, ignored, references);
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let Some(expr) = elem.as_expression() {
                    collect_expression_references(expr, ignored, references);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    if p.shorthand {
                        if let PropertyKey::StaticIdentifier(ident) = &p.key {
                            let name_bytes = ident.name.as_bytes();
                            if !ignored.contains(name_bytes)
                                && !is_keyword(name_bytes)
                                && !is_global(name_bytes)
                            {
                                references.insert(ident.name.as_str());
                            }
                        }
                    } else {
                        collect_expression_references(&p.value, ignored, references);
                    }
                }
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_expression_references(&paren.expression, ignored, references);
        }
        Expression::UnaryExpression(unary) => {
            collect_expression_references(&unary.argument, ignored, references);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_expression_references(&ts_as.expression, ignored, references);
        }
        Expression::TSNonNullExpression(non_null) => {
            collect_expression_references(&non_null.expression, ignored, references);
        }
        Expression::ChainExpression(chain) => {
            collect_chain_element_references(&chain.expression, ignored, references);
        }
        Expression::AwaitExpression(await_expr) => {
            collect_expression_references(&await_expr.argument, ignored, references);
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                collect_expression_references(expr, ignored, references);
            }
        }
        Expression::AssignmentExpression(assign) => {
            collect_expression_references(&assign.right, ignored, references);
        }
        Expression::NewExpression(new_expr) => {
            collect_expression_references(&new_expr.callee, ignored, references);
            for arg in &new_expr.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_references(expr, ignored, references);
                }
            }
        }
        Expression::TemplateLiteral(template) => {
            for expr in &template.expressions {
                collect_expression_references(expr, ignored, references);
            }
        }
        Expression::TaggedTemplateExpression(tagged) => {
            collect_expression_references(&tagged.tag, ignored, references);
            for expr in &tagged.quasi.expressions {
                collect_expression_references(expr, ignored, references);
            }
        }
        Expression::YieldExpression(yield_expr) => {
            if let Some(arg) = &yield_expr.argument {
                collect_expression_references(arg, ignored, references);
            }
        }
        _ => {}
    }
}

/// Collect references from chain elements (optional chaining).
pub fn collect_chain_element_references<'a>(
    element: &'a ChainElement<'a>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<&'a str>,
) {
    match element {
        ChainElement::CallExpression(call) => {
            collect_expression_references(&call.callee, ignored, references);
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_references(expr, ignored, references);
                }
            }
        }
        ChainElement::StaticMemberExpression(member) => {
            collect_expression_references(&member.object, ignored, references);
        }
        ChainElement::ComputedMemberExpression(member) => {
            collect_expression_references(&member.object, ignored, references);
            collect_expression_references(&member.expression, ignored, references);
        }
        ChainElement::PrivateFieldExpression(field) => {
            collect_expression_references(&field.object, ignored, references);
        }
        _ => {}
    }
}

/// Collect type references from TypeScript type annotations.
pub fn collect_type_references<'a>(ts_type: &'a TSType<'a>, references: &mut FxHashSet<&'a str>) {
    match ts_type {
        TSType::TSTypeReference(type_ref) => {
            if let TSTypeName::IdentifierReference(ident) = &type_ref.type_name {
                let name_bytes = ident.name.as_bytes();
                // Note: don't filter globals here — Array, Map, Set etc. are valid TS types
                if !is_keyword(name_bytes) {
                    references.insert(ident.name.as_str());
                }
            }
            // Also check generic type arguments
            if let Some(args) = &type_ref.type_arguments {
                for arg in &args.params {
                    collect_type_references(arg, references);
                }
            }
        }
        TSType::TSTypeLiteral(lit) => {
            for member in &lit.members {
                match member {
                    TSSignature::TSPropertySignature(prop) => {
                        if let Some(annotation) = &prop.type_annotation {
                            collect_type_references(&annotation.type_annotation, references);
                        }
                    }
                    TSSignature::TSMethodSignature(method) => {
                        if let Some(annotation) = &method.return_type {
                            collect_type_references(&annotation.type_annotation, references);
                        }
                    }
                    _ => {}
                }
            }
        }
        TSType::TSUnionType(union) => {
            for t in &union.types {
                collect_type_references(t, references);
            }
        }
        TSType::TSIntersectionType(intersection) => {
            for t in &intersection.types {
                collect_type_references(t, references);
            }
        }
        TSType::TSArrayType(arr) => {
            collect_type_references(&arr.element_type, references);
        }
        TSType::TSTupleType(tuple) => {
            for elem in &tuple.element_types {
                match elem {
                    TSTupleElement::TSOptionalType(opt) => {
                        collect_type_references(&opt.type_annotation, references);
                    }
                    TSTupleElement::TSRestType(rest) => {
                        collect_type_references(&rest.type_annotation, references);
                    }
                    _ => {
                        if let Some(t) = elem.as_ts_type() {
                            collect_type_references(t, references);
                        }
                    }
                }
            }
        }
        TSType::TSConditionalType(cond) => {
            collect_type_references(&cond.check_type, references);
            collect_type_references(&cond.extends_type, references);
            collect_type_references(&cond.true_type, references);
            collect_type_references(&cond.false_type, references);
        }
        TSType::TSFunctionType(func) => {
            collect_type_references(&func.return_type.type_annotation, references);
        }
        TSType::TSIndexedAccessType(indexed) => {
            collect_type_references(&indexed.object_type, references);
            collect_type_references(&indexed.index_type, references);
        }
        TSType::TSMappedType(mapped) => {
            if let Some(t) = &mapped.type_annotation {
                collect_type_references(t, references);
            }
        }
        TSType::TSTypeOperatorType(operator) => {
            collect_type_references(&operator.type_annotation, references);
        }
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                let name_bytes = ident.name.as_bytes();
                // Note: don't filter globals here — typeof Array etc. are valid TS type queries
                if !is_keyword(name_bytes) {
                    references.insert(ident.name.as_str());
                }
            }
        }
        TSType::TSParenthesizedType(paren) => {
            collect_type_references(&paren.type_annotation, references);
        }
        _ => {}
    }
}

/// Collect TypeScript type references from an expression (for type assertions like `as T`).
pub fn collect_ts_type_references_from_expression<'a>(
    expr: &'a Expression<'a>,
    references: &mut FxHashSet<&'a str>,
) {
    match expr {
        Expression::TSAsExpression(ts_as) => {
            collect_type_references(&ts_as.type_annotation, references);
            collect_ts_type_references_from_expression(&ts_as.expression, references);
        }
        Expression::TSSatisfiesExpression(satisfies) => {
            collect_type_references(&satisfies.type_annotation, references);
            collect_ts_type_references_from_expression(&satisfies.expression, references);
        }
        Expression::TSTypeAssertion(assertion) => {
            collect_type_references(&assertion.type_annotation, references);
            collect_ts_type_references_from_expression(&assertion.expression, references);
        }
        Expression::TSNonNullExpression(non_null) => {
            collect_ts_type_references_from_expression(&non_null.expression, references);
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_ts_type_references_from_expression(&paren.expression, references);
        }
        Expression::CallExpression(call) => {
            collect_ts_type_references_from_expression(&call.callee, references);
            for arg in &call.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_ts_type_references_from_expression(e, references);
                }
            }
        }
        Expression::StaticMemberExpression(member) => {
            collect_ts_type_references_from_expression(&member.object, references);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_ts_type_references_from_expression(&member.object, references);
            collect_ts_type_references_from_expression(&member.expression, references);
        }
        Expression::BinaryExpression(binary) => {
            collect_ts_type_references_from_expression(&binary.left, references);
            collect_ts_type_references_from_expression(&binary.right, references);
        }
        Expression::LogicalExpression(logical) => {
            collect_ts_type_references_from_expression(&logical.left, references);
            collect_ts_type_references_from_expression(&logical.right, references);
        }
        Expression::ConditionalExpression(cond) => {
            collect_ts_type_references_from_expression(&cond.test, references);
            collect_ts_type_references_from_expression(&cond.consequent, references);
            collect_ts_type_references_from_expression(&cond.alternate, references);
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let Some(e) = elem.as_expression() {
                    collect_ts_type_references_from_expression(e, references);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    collect_ts_type_references_from_expression(&p.value, references);
                }
            }
        }
        _ => {}
    }
}

/// Collect locals from an array assignment target (for v-for destructuring).
pub fn collect_assignment_target_locals_array(
    arr: &ArrayAssignmentTarget<'_>,
    locals: &mut Vec<String>,
) {
    for elem in arr.elements.iter().flatten() {
        collect_assignment_target_maybe_default_locals(elem, locals);
    }
    if let Some(rest) = &arr.rest {
        collect_assignment_target_locals(&rest.target, locals);
    }
}

/// Collect locals from an object assignment target (for v-for destructuring).
pub fn collect_assignment_target_locals_object(
    obj: &ObjectAssignmentTarget<'_>,
    locals: &mut Vec<String>,
) {
    for prop in &obj.properties {
        match prop {
            AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(ident) => {
                locals.push(ident.binding.name.to_string());
            }
            AssignmentTargetProperty::AssignmentTargetPropertyProperty(prop) => {
                collect_assignment_target_maybe_default_locals(&prop.binding, locals);
            }
        }
    }
    if let Some(rest) = &obj.rest {
        collect_assignment_target_locals(&rest.target, locals);
    }
}

/// Collect locals from an assignment target.
pub fn collect_assignment_target_locals(target: &AssignmentTarget<'_>, locals: &mut Vec<String>) {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(ident) => {
            locals.push(ident.name.to_string());
        }
        AssignmentTarget::ArrayAssignmentTarget(arr) => {
            collect_assignment_target_locals_array(arr, locals);
        }
        AssignmentTarget::ObjectAssignmentTarget(obj) => {
            collect_assignment_target_locals_object(obj, locals);
        }
        _ => {}
    }
}

/// Collect locals from assignment target with possible default value.
pub fn collect_assignment_target_maybe_default_locals(
    target: &AssignmentTargetMaybeDefault<'_>,
    locals: &mut Vec<String>,
) {
    match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
            collect_assignment_target_locals(&with_default.binding, locals);
        }
        _ => {
            if let Some(t) = target.as_assignment_target() {
                collect_assignment_target_locals(t, locals);
            }
        }
    }
}

// =============================================================================
// Span-based collection functions
// =============================================================================
// These functions collect spans instead of string references, avoiding
// self-referential struct issues and saving memory. Use span.slice(source)
// to get the string value when needed.

/// Collect local binding spans from a binding pattern.
///
/// This extracts spans of all identifiers declared by the pattern itself.
/// For example, in `{ a, b: c }`, this returns spans for "a" and "c".
pub fn collect_pattern_local_spans(pattern: &BindingPattern<'_>, locals: &mut Vec<Span>) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            locals.push(ident.span.into());
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_local_spans(&prop.value, locals);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_local_spans(&rest.argument, locals);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_local_spans(elem, locals);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_local_spans(&rest.argument, locals);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_pattern_local_spans(&assign.left, locals);
        }
    }
}

/// Collect reference spans from a binding pattern (default values).
///
/// This extracts spans of identifiers that are referenced in default value expressions.
pub fn collect_pattern_reference_spans(
    pattern: &BindingPattern<'_>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<Span>,
) {
    match pattern {
        BindingPattern::AssignmentPattern(assign) => {
            collect_expression_reference_spans(&assign.right, ignored, references);
            collect_pattern_reference_spans(&assign.left, ignored, references);
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_reference_spans(&prop.value, ignored, references);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_reference_spans(&rest.argument, ignored, references);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_reference_spans(elem, ignored, references);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_reference_spans(&rest.argument, ignored, references);
            }
        }
        BindingPattern::BindingIdentifier(_) => {}
    }
}

/// Collect identifier reference spans from an expression (excluding ignored identifiers).
pub fn collect_expression_reference_spans(
    expr: &Expression<'_>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<Span>,
) {
    match expr {
        Expression::Identifier(ident) => {
            let name_bytes = ident.name.as_bytes();
            if !ignored.contains(name_bytes) && !is_keyword(name_bytes) && !is_global(name_bytes) {
                references.insert(ident.span.into());
            }
        }
        Expression::BinaryExpression(binary) => {
            collect_expression_reference_spans(&binary.left, ignored, references);
            collect_expression_reference_spans(&binary.right, ignored, references);
        }
        Expression::LogicalExpression(logical) => {
            collect_expression_reference_spans(&logical.left, ignored, references);
            collect_expression_reference_spans(&logical.right, ignored, references);
        }
        Expression::ConditionalExpression(cond) => {
            collect_expression_reference_spans(&cond.test, ignored, references);
            collect_expression_reference_spans(&cond.consequent, ignored, references);
            collect_expression_reference_spans(&cond.alternate, ignored, references);
        }
        Expression::CallExpression(call) => {
            collect_expression_reference_spans(&call.callee, ignored, references);
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_reference_spans(expr, ignored, references);
                }
            }
        }
        Expression::StaticMemberExpression(member) => {
            collect_expression_reference_spans(&member.object, ignored, references);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_expression_reference_spans(&member.object, ignored, references);
            collect_expression_reference_spans(&member.expression, ignored, references);
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let Some(expr) = elem.as_expression() {
                    collect_expression_reference_spans(expr, ignored, references);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    if p.shorthand {
                        if let PropertyKey::StaticIdentifier(ident) = &p.key {
                            let name_bytes = ident.name.as_bytes();
                            if !ignored.contains(name_bytes)
                                && !is_keyword(name_bytes)
                                && !is_global(name_bytes)
                            {
                                references.insert(ident.span.into());
                            }
                        }
                    } else {
                        collect_expression_reference_spans(&p.value, ignored, references);
                    }
                }
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_expression_reference_spans(&paren.expression, ignored, references);
        }
        Expression::UnaryExpression(unary) => {
            collect_expression_reference_spans(&unary.argument, ignored, references);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_expression_reference_spans(&ts_as.expression, ignored, references);
        }
        Expression::TSNonNullExpression(non_null) => {
            collect_expression_reference_spans(&non_null.expression, ignored, references);
        }
        Expression::ChainExpression(chain) => {
            collect_chain_element_reference_spans(&chain.expression, ignored, references);
        }
        Expression::AwaitExpression(await_expr) => {
            collect_expression_reference_spans(&await_expr.argument, ignored, references);
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                collect_expression_reference_spans(expr, ignored, references);
            }
        }
        Expression::AssignmentExpression(assign) => {
            collect_expression_reference_spans(&assign.right, ignored, references);
        }
        Expression::NewExpression(new_expr) => {
            collect_expression_reference_spans(&new_expr.callee, ignored, references);
            for arg in &new_expr.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_reference_spans(expr, ignored, references);
                }
            }
        }
        Expression::TemplateLiteral(template) => {
            for expr in &template.expressions {
                collect_expression_reference_spans(expr, ignored, references);
            }
        }
        Expression::TaggedTemplateExpression(tagged) => {
            collect_expression_reference_spans(&tagged.tag, ignored, references);
            for expr in &tagged.quasi.expressions {
                collect_expression_reference_spans(expr, ignored, references);
            }
        }
        Expression::YieldExpression(yield_expr) => {
            if let Some(arg) = &yield_expr.argument {
                collect_expression_reference_spans(arg, ignored, references);
            }
        }
        _ => {}
    }
}

/// Collect reference spans from chain elements (optional chaining).
pub fn collect_chain_element_reference_spans(
    element: &ChainElement<'_>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<Span>,
) {
    match element {
        ChainElement::CallExpression(call) => {
            collect_expression_reference_spans(&call.callee, ignored, references);
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_reference_spans(expr, ignored, references);
                }
            }
        }
        ChainElement::StaticMemberExpression(member) => {
            collect_expression_reference_spans(&member.object, ignored, references);
        }
        ChainElement::ComputedMemberExpression(member) => {
            collect_expression_reference_spans(&member.object, ignored, references);
            collect_expression_reference_spans(&member.expression, ignored, references);
        }
        ChainElement::PrivateFieldExpression(field) => {
            collect_expression_reference_spans(&field.object, ignored, references);
        }
        _ => {}
    }
}

/// Collect type reference spans from TypeScript type annotations.
pub fn collect_type_reference_spans(ts_type: &TSType<'_>, references: &mut FxHashSet<Span>) {
    match ts_type {
        TSType::TSTypeReference(type_ref) => {
            if let TSTypeName::IdentifierReference(ident) = &type_ref.type_name {
                let name_bytes = ident.name.as_bytes();
                // Note: don't filter globals here — Array, Map, Set etc. are valid TS types
                if !is_keyword(name_bytes) {
                    references.insert(ident.span.into());
                }
            }
            // Also check generic type arguments
            if let Some(args) = &type_ref.type_arguments {
                for arg in &args.params {
                    collect_type_reference_spans(arg, references);
                }
            }
        }
        TSType::TSTypeLiteral(lit) => {
            for member in &lit.members {
                match member {
                    TSSignature::TSPropertySignature(prop) => {
                        if let Some(annotation) = &prop.type_annotation {
                            collect_type_reference_spans(&annotation.type_annotation, references);
                        }
                    }
                    TSSignature::TSMethodSignature(method) => {
                        if let Some(annotation) = &method.return_type {
                            collect_type_reference_spans(&annotation.type_annotation, references);
                        }
                    }
                    _ => {}
                }
            }
        }
        TSType::TSUnionType(union) => {
            for t in &union.types {
                collect_type_reference_spans(t, references);
            }
        }
        TSType::TSIntersectionType(intersection) => {
            for t in &intersection.types {
                collect_type_reference_spans(t, references);
            }
        }
        TSType::TSArrayType(arr) => {
            collect_type_reference_spans(&arr.element_type, references);
        }
        TSType::TSTupleType(tuple) => {
            for elem in &tuple.element_types {
                match elem {
                    TSTupleElement::TSOptionalType(opt) => {
                        collect_type_reference_spans(&opt.type_annotation, references);
                    }
                    TSTupleElement::TSRestType(rest) => {
                        collect_type_reference_spans(&rest.type_annotation, references);
                    }
                    _ => {
                        if let Some(t) = elem.as_ts_type() {
                            collect_type_reference_spans(t, references);
                        }
                    }
                }
            }
        }
        TSType::TSConditionalType(cond) => {
            collect_type_reference_spans(&cond.check_type, references);
            collect_type_reference_spans(&cond.extends_type, references);
            collect_type_reference_spans(&cond.true_type, references);
            collect_type_reference_spans(&cond.false_type, references);
        }
        TSType::TSFunctionType(func) => {
            collect_type_reference_spans(&func.return_type.type_annotation, references);
        }
        TSType::TSIndexedAccessType(indexed) => {
            collect_type_reference_spans(&indexed.object_type, references);
            collect_type_reference_spans(&indexed.index_type, references);
        }
        TSType::TSMappedType(mapped) => {
            if let Some(t) = &mapped.type_annotation {
                collect_type_reference_spans(t, references);
            }
        }
        TSType::TSTypeOperatorType(operator) => {
            collect_type_reference_spans(&operator.type_annotation, references);
        }
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                let name_bytes = ident.name.as_bytes();
                // Note: don't filter globals here — typeof Array etc. are valid TS type queries
                if !is_keyword(name_bytes) {
                    references.insert(ident.span.into());
                }
            }
        }
        TSType::TSParenthesizedType(paren) => {
            collect_type_reference_spans(&paren.type_annotation, references);
        }
        _ => {}
    }
}

/// Collect TypeScript type reference spans from an expression (for type assertions like `as T`).
pub fn collect_ts_type_reference_spans_from_expression(
    expr: &Expression<'_>,
    references: &mut FxHashSet<Span>,
) {
    match expr {
        Expression::TSAsExpression(ts_as) => {
            collect_type_reference_spans(&ts_as.type_annotation, references);
            collect_ts_type_reference_spans_from_expression(&ts_as.expression, references);
        }
        Expression::TSSatisfiesExpression(satisfies) => {
            collect_type_reference_spans(&satisfies.type_annotation, references);
            collect_ts_type_reference_spans_from_expression(&satisfies.expression, references);
        }
        Expression::TSTypeAssertion(assertion) => {
            collect_type_reference_spans(&assertion.type_annotation, references);
            collect_ts_type_reference_spans_from_expression(&assertion.expression, references);
        }
        Expression::TSNonNullExpression(non_null) => {
            collect_ts_type_reference_spans_from_expression(&non_null.expression, references);
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_ts_type_reference_spans_from_expression(&paren.expression, references);
        }
        Expression::CallExpression(call) => {
            collect_ts_type_reference_spans_from_expression(&call.callee, references);
            for arg in &call.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_ts_type_reference_spans_from_expression(e, references);
                }
            }
        }
        Expression::StaticMemberExpression(member) => {
            collect_ts_type_reference_spans_from_expression(&member.object, references);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_ts_type_reference_spans_from_expression(&member.object, references);
            collect_ts_type_reference_spans_from_expression(&member.expression, references);
        }
        Expression::BinaryExpression(binary) => {
            collect_ts_type_reference_spans_from_expression(&binary.left, references);
            collect_ts_type_reference_spans_from_expression(&binary.right, references);
        }
        Expression::LogicalExpression(logical) => {
            collect_ts_type_reference_spans_from_expression(&logical.left, references);
            collect_ts_type_reference_spans_from_expression(&logical.right, references);
        }
        Expression::ConditionalExpression(cond) => {
            collect_ts_type_reference_spans_from_expression(&cond.test, references);
            collect_ts_type_reference_spans_from_expression(&cond.consequent, references);
            collect_ts_type_reference_spans_from_expression(&cond.alternate, references);
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let Some(e) = elem.as_expression() {
                    collect_ts_type_reference_spans_from_expression(e, references);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    collect_ts_type_reference_spans_from_expression(&p.value, references);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    #[test]
    fn test_collect_expression_references_identifier() {
        let allocator = Allocator::default();
        let source = "foo";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let ignored = FxHashSet::default();
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(references.contains("foo"));
    }

    #[test]
    fn test_collect_expression_references_with_ignored() {
        let allocator = Allocator::default();
        let source = "foo + bar";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let mut ignored = FxHashSet::default();
        ignored.insert(b"foo" as &[u8]);
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(!references.contains("foo"));
        assert!(references.contains("bar"));
    }

    #[test]
    fn test_collect_expression_references_keywords_ignored() {
        let allocator = Allocator::default();
        let source = "foo && true || null";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let ignored = FxHashSet::default();
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(references.contains("foo"));
        assert!(!references.contains("true"));
        assert!(!references.contains("null"));
    }

    #[test]
    fn test_collect_expression_references_member_access() {
        let allocator = Allocator::default();
        let source = "foo.bar[baz]";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let ignored = FxHashSet::default();
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(references.contains("foo"));
        assert!(references.contains("baz"));
        assert!(!references.contains("bar")); // property access, not reference
    }

    #[test]
    fn test_collect_expression_references_globals_ignored() {
        let allocator = Allocator::default();
        let source = "String.fromCharCode(65)";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let ignored = FxHashSet::default();
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(
            !references.contains("String"),
            "String should be ignored as a global"
        );
    }

    #[test]
    fn test_collect_expression_references_globals_math() {
        let allocator = Allocator::default();
        let source = "Math.max(a, b)";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let ignored = FxHashSet::default();
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(!references.contains("Math"), "Math should be ignored");
        assert!(references.contains("a"));
        assert!(references.contains("b"));
    }

    // ── collect_setup_binding_refs tests ────────────────────────────

    fn parse_setup_refs<'a>(
        alloc: &'a Allocator,
        source: &'a str,
        setup_names: &FxHashSet<&str>,
    ) -> FxHashSet<String> {
        let ret = Parser::new(alloc, source, SourceType::tsx()).parse();
        assert!(ret.errors.is_empty(), "Parse errors: {:?}", ret.errors);
        let refs = collect_setup_binding_refs(&ret.program, setup_names);
        refs.into_iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_setup_refs_basic_reference() {
        let alloc = Allocator::default();
        let source = "const count = ref(0);\nconst doubled = computed(() => count.value * 2);";
        let mut names = FxHashSet::default();
        names.insert("count");
        names.insert("doubled");
        let refs = parse_setup_refs(&alloc, source, &names);
        assert!(
            refs.contains("count"),
            "count should be referenced in computed arrow"
        );
        assert!(
            !refs.contains("doubled"),
            "doubled is not referenced by anyone"
        );
    }

    #[test]
    fn test_setup_refs_shadowed_by_param() {
        let alloc = Allocator::default();
        let source = "const count = ref(0);\nfunction foo(count: number) { return count; }";
        let mut names = FxHashSet::default();
        names.insert("count");
        let refs = parse_setup_refs(&alloc, source, &names);
        assert!(
            !refs.contains("count"),
            "count is shadowed by function param"
        );
    }

    #[test]
    fn test_setup_refs_partially_shadowed() {
        let alloc = Allocator::default();
        let source = "const count = ref(0);\nconst d = count.value;\nfunction foo(count: number) { return count; }";
        let mut names = FxHashSet::default();
        names.insert("count");
        let refs = parse_setup_refs(&alloc, source, &names);
        assert!(
            refs.contains("count"),
            "count is freely referenced in `d` initializer"
        );
    }

    #[test]
    fn test_setup_refs_inner_variable_shadow() {
        let alloc = Allocator::default();
        let source = "const count = ref(0);\nfunction foo() { const count = 42; return count; }";
        let mut names = FxHashSet::default();
        names.insert("count");
        let refs = parse_setup_refs(&alloc, source, &names);
        assert!(!refs.contains("count"), "count is shadowed by inner const");
    }

    #[test]
    fn test_setup_refs_arrow_param_shadow() {
        let alloc = Allocator::default();
        let source = "const count = ref(0);\nconst fn2 = (count: number) => count * 2;";
        let mut names = FxHashSet::default();
        names.insert("count");
        let refs = parse_setup_refs(&alloc, source, &names);
        assert!(!refs.contains("count"), "count is shadowed by arrow param");
    }

    #[test]
    fn test_setup_refs_truly_unused() {
        let alloc = Allocator::default();
        let source = "const count = ref(0);\nconst unused = ref(42);";
        let mut names = FxHashSet::default();
        names.insert("count");
        names.insert("unused");
        let refs = parse_setup_refs(&alloc, source, &names);
        assert!(refs.is_empty(), "neither binding is referenced: {:?}", refs);
    }

    #[test]
    fn test_setup_refs_block_scope_shadow() {
        let alloc = Allocator::default();
        let source = "const x = ref(0);\n{ const x = 1; console.log(x); }";
        let mut names = FxHashSet::default();
        names.insert("x");
        let refs = parse_setup_refs(&alloc, source, &names);
        assert!(!refs.contains("x"), "x is shadowed by block-scoped const");
    }

    #[test]
    fn test_setup_refs_multiple_bindings() {
        let alloc = Allocator::default();
        let source = "const a = ref(1);\nconst b = ref(2);\nconst c = ref(3);\nconst sum = computed(() => a.value + c.value);";
        let mut names = FxHashSet::default();
        names.insert("a");
        names.insert("b");
        names.insert("c");
        names.insert("sum");
        let refs = parse_setup_refs(&alloc, source, &names);
        assert!(refs.contains("a"), "a is referenced in computed");
        assert!(!refs.contains("b"), "b is not referenced");
        assert!(refs.contains("c"), "c is referenced in computed");
        assert!(!refs.contains("sum"), "sum is not referenced by anyone");
    }

    #[test]
    fn test_collect_expression_references_shorthand_property() {
        let allocator = Allocator::default();
        let source = "{ foo, bar: baz }";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let ignored = FxHashSet::default();
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(references.contains("foo")); // shorthand
        assert!(references.contains("baz")); // value
        assert!(!references.contains("bar")); // key
    }
}
