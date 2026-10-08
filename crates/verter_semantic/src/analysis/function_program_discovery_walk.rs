//! Discovery's one walk of the syntax outside every served function.
//!
//! The walk enters each statement of the program once. At a statement
//! discovery serves positions in — a top-level statement, or a statement of
//! a namespace block it reaches — it discovers those positions first, then
//! walks the statement's syntax, skipping every function discovery serves:
//! the hash fold walks those bodies. Every class the walk meets is recorded
//! where it meets it, so recording the classes outside every served body
//! walks no syntax of its own.

use oxc_ast::ast::{
    ArrowFunctionExpression, BlockStatement, CatchClause, Class, Expression, ForInStatement,
    ForOfStatement, ForStatement, FormalParameter, Function, Statement, SwitchStatement,
    TSModuleBlock, TSNamespaceDeclaration, TSType, VariableDeclaration,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use oxc_syntax::scope::ScopeFlags;

use super::{
    discover_namespaced_statement, discover_statement, namespace_block, namespace_member_step,
    DiscoveryCtx, FunctionDescent, OverloadTracker,
};
use crate::analysis::class_index::ClassFrame;

#[cfg(test)]
std::thread_local! {
    static STATEMENT_ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Statements discovery's walk entered on this thread since the last call:
/// each statement outside every served function, once.
#[cfg(test)]
pub(super) fn take_statement_entries_for_tests() -> usize {
    STATEMENT_ENTRIES.with(|entries| entries.replace(0))
}

fn enter_statement() {
    #[cfg(test)]
    STATEMENT_ENTRIES.with(|entries| entries.set(entries.get() + 1));
}

pub(super) struct DiscoveryWalk<'w, 's, 'ast> {
    ctx: &'w mut DiscoveryCtx<'s, 'ast>,
    overloads: OverloadTracker,
}

impl<'w, 's, 'ast> DiscoveryWalk<'w, 's, 'ast> {
    pub(super) fn new(ctx: &'w mut DiscoveryCtx<'s, 'ast>) -> Self {
        Self {
            ctx,
            overloads: OverloadTracker::default(),
        }
    }

    /// Discover the served positions of the program's statements, and
    /// record every class outside the functions they serve.
    pub(super) fn program(mut self, statements: &'ast [Statement<'ast>]) {
        for (contributor_index, statement) in statements.iter().enumerate() {
            discover_statement(
                statement,
                contributor_index,
                None,
                &mut self.overloads,
                self.ctx,
            );
            match namespace_of(statement) {
                Some(module) => {
                    enter_statement();
                    self.namespace(module, contributor_index, &FunctionDescent::new(), None);
                }
                None => self.visit_statement(statement),
            }
        }
    }

    /// Discover the served positions of a namespace's members — each
    /// qualified under the namespace, each locator extending `descent` by
    /// one namespace-member step — and record the classes of its block.
    /// A dotted namespace serves none of its members.
    fn namespace(
        &mut self,
        module: &'ast TSNamespaceDeclaration<'ast>,
        contributor_index: usize,
        descent: &FunctionDescent,
        prefix: Option<&str>,
    ) {
        let Some(block) = namespace_block(module) else {
            walk::walk_ts_namespace_declaration(self, module);
            return;
        };
        let qualified = match prefix {
            Some(prefix) => format!("{prefix}.{}", module.id.name),
            None => module.id.name.to_string(),
        };
        self.ctx.classes.enter_scope();
        for (statement_ordinal, statement) in block.body.iter().enumerate() {
            let descent = descent.then(namespace_member_step(statement_ordinal));
            discover_namespaced_statement(
                statement,
                contributor_index,
                &descent,
                &qualified,
                &mut self.overloads,
                self.ctx,
            );
            match namespace_of(statement) {
                Some(inner) => {
                    enter_statement();
                    self.namespace(inner, contributor_index, &descent, Some(&qualified));
                }
                None => self.visit_statement(statement),
            }
        }
        self.ctx.classes.exit_scope();
    }

    /// Record the classes of the decorators of a served function's
    /// parameters, which the function's hash fold does not walk.
    pub(super) fn parameter_decorators(mut self, params: &[FormalParameter<'ast>]) {
        for param in params {
            self.visit_decorators(&param.decorators);
        }
    }

    fn is_served(&self, span: oxc_span::Span) -> bool {
        self.ctx.served.contains(&(span.start, span.end))
    }

    fn scoped(&mut self, walk: impl FnOnce(&mut Self)) {
        self.ctx.classes.enter_scope();
        walk(self);
        self.ctx.classes.exit_scope();
    }
}

/// The namespace a statement declares, exported or not.
fn namespace_of<'a>(statement: &'a Statement<'a>) -> Option<&'a TSNamespaceDeclaration<'a>> {
    match statement {
        Statement::TSNamespaceDeclaration(module) => Some(module),
        Statement::ExportDeclaration(export) => match &export.declaration {
            oxc_ast::ast::Declaration::TSNamespaceDeclaration(module) => Some(module),
            _ => None,
        },
        _ => None,
    }
}

impl<'a> Visit<'a> for DiscoveryWalk<'_, '_, 'a> {
    fn visit_statement(&mut self, statement: &Statement<'a>) {
        enter_statement();
        walk::walk_statement(self, statement);
    }

    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        self.scoped(|walk| walk::walk_block_statement(walk, block));
    }

    fn visit_for_statement(&mut self, statement: &ForStatement<'a>) {
        self.scoped(|walk| walk::walk_for_statement(walk, statement));
    }

    fn visit_for_in_statement(&mut self, statement: &ForInStatement<'a>) {
        self.scoped(|walk| walk::walk_for_in_statement(walk, statement));
    }

    fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'a>) {
        self.scoped(|walk| walk::walk_for_of_statement(walk, statement));
    }

    fn visit_switch_statement(&mut self, statement: &SwitchStatement<'a>) {
        self.scoped(|walk| walk::walk_switch_statement(walk, statement));
    }

    fn visit_catch_clause(&mut self, clause: &CatchClause<'a>) {
        self.scoped(|walk| walk::walk_catch_clause(walk, clause));
    }

    fn visit_ts_module_block(&mut self, block: &TSModuleBlock<'a>) {
        self.scoped(|walk| walk::walk_ts_module_block(walk, block));
    }

    fn visit_expression(&mut self, expression: &Expression<'a>) {
        // A served class field initializer is walked by its hash fold.
        if self.is_served(expression.span()) {
            return;
        }
        walk::walk_expression(self, expression);
    }

    fn visit_variable_declaration(&mut self, declaration: &VariableDeclaration<'a>) {
        self.ctx.classes.note_variable_declaration(declaration);
        walk::walk_variable_declaration(self, declaration);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        self.ctx.classes.record(class);
        // A class's heritage reads the scope around it; its body is a scope
        // of its own (a class expression's name binds inside it).
        self.visit_decorators(&class.decorators);
        if let Some(heritage) = &class.heritage {
            self.visit_expression(&heritage.expression);
        }
        self.scoped(|walk| walk.visit_class_body(&class.body));
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        if self.is_served(function.span) {
            return;
        }
        self.ctx.classes.enter_frame(ClassFrame::Unserved);
        walk::walk_function(self, function, flags);
        self.ctx.classes.exit_frame();
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        if self.is_served(arrow.span) {
            return;
        }
        self.ctx.classes.enter_frame(ClassFrame::Unserved);
        walk::walk_arrow_function_expression(self, arrow);
        self.ctx.classes.exit_frame();
    }

    // No class is written in a type.
    fn visit_ts_type(&mut self, _ty: &TSType<'a>) {}
}
