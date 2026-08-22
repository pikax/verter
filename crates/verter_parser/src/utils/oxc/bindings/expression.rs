//! Expression binding extraction.
//!
//! This module provides the main entry points for extracting bindings from
//! JavaScript/TypeScript expressions parsed by OXC.

use oxc_ast::ast::*;
use oxc_span::Span as OxcSpan;
use smallvec::SmallVec;

use super::keywords::{is_global, is_keyword};
use super::types::{
    Binding, BindingContext, BindingExtractionResult, FunctionBinding, LiteralBinding, ParamBytes,
};

/// Extract bindings from an OXC Expression.
///
/// This function walks the expression AST and extracts all identifier bindings,
/// function expressions, and literals found within.
///
/// # Arguments
/// * `expr` - The expression to extract bindings from
/// * `source` - The source code string (for literal content extraction)
/// * `ctx` - The binding context with base offset and ignored identifiers
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// let parser = Parser::new(&allocator, "foo + bar", SourceType::tsx());
/// let expr = parser.parse_expression().unwrap();
/// let ctx = BindingContext::new(0);
/// let result = extract_bindings_from_expression(&expr, "foo + bar", ctx);
/// assert_eq!(result.non_ignored_binding_names(), vec!["foo", "bar"]);
/// ```
pub fn extract_bindings_from_expression<'a>(
    expr: &Expression<'a>,
    source: &'a str,
    ctx: BindingContext<'a>,
) -> BindingExtractionResult<'a> {
    let mut result = BindingExtractionResult::default();
    let source_bytes = source.as_bytes();
    let mut visitor = BindingVisitor::new(source_bytes, ctx, &mut result);
    visitor.visit_expression(expr);
    result
}

/// Extract bindings from an OXC Program.
///
/// This function walks the program AST and extracts all identifier bindings,
/// function expressions, and literals found within.
pub fn extract_bindings_from_program<'a>(
    program: &'a Program<'a>,
    source: &'a str,
    ctx: BindingContext<'a>,
) -> BindingExtractionResult<'a> {
    let mut result = BindingExtractionResult::default();
    let source_bytes = source.as_bytes();
    let mut visitor = BindingVisitor::new(source_bytes, ctx, &mut result);

    for stmt in &program.body {
        visitor.visit_statement(stmt);
    }

    result
}

/// Borrow a binding identifier's name for the ARENA lifetime.
///
/// SAFETY: `ident.name` is `Atom<'a>` — its string data lives in the OXC arena
/// allocator with lifetime `'a`. `Atom::as_str()` returns `&str` with the borrow
/// lifetime (an OXC API limitation), but the data genuinely has lifetime `'a`
/// since the allocator and the AST both outlive the visitor.
#[inline]
fn arena_name<'a>(ident: &BindingIdentifier<'a>) -> &'a str {
    let name_str = ident.name.as_str();
    verter_debug_assert!(
        !name_str.is_empty(),
        "binding identifier should be non-empty"
    );
    unsafe { std::mem::transmute::<&str, &'a str>(name_str) }
}

/// Borrow a non-binding identifier's name for the ARENA lifetime.
///
/// SAFETY: identical to [`arena_name`] — `IdentifierName` holds the same
/// arena-allocated `Atom<'a>`.
#[inline]
fn arena_ident_name<'a>(ident: &IdentifierName<'a>) -> &'a str {
    let name_str = ident.name.as_str();
    verter_debug_assert!(!name_str.is_empty(), "identifier name should be non-empty");
    unsafe { std::mem::transmute::<&str, &'a str>(name_str) }
}

/// Internal visitor for extracting bindings with byte-level optimization.
struct BindingVisitor<'a, 'r> {
    source_bytes: &'a [u8],
    ctx: BindingContext<'a>,
    result: &'r mut BindingExtractionResult<'a>,
}

impl<'a, 'r> BindingVisitor<'a, 'r> {
    fn new(
        source_bytes: &'a [u8],
        ctx: BindingContext<'a>,
        result: &'r mut BindingExtractionResult<'a>,
    ) -> Self {
        Self {
            source_bytes,
            ctx,
            result,
        }
    }

    /// Walk a statement, resolving every identifier it can reach.
    ///
    /// The match is EXHAUSTIVE by construction — there is no catch-all arm — so
    /// a `Statement` variant added by a future OXC release is a compile error
    /// here rather than a silent miss. A statement form that genuinely carries
    /// no resolvable identifier still gets its own arm, with the reason.
    ///
    /// Descending into a statement is not the same as descending into all of
    /// its parts: a `for` head, a class heritage clause and a class element are
    /// each their own position, and each is walked below.
    #[inline]
    fn visit_statement(&mut self, stmt: &Statement<'a>) {
        match stmt {
            Statement::ExpressionStatement(expr_stmt) => {
                self.visit_expression(&expr_stmt.expression);
            }
            Statement::VariableDeclaration(var_decl) => {
                self.visit_variable_declaration(var_decl);
            }
            Statement::BlockStatement(block) => {
                for stmt in &block.body {
                    self.visit_statement(stmt);
                }
            }
            Statement::ReturnStatement(ret) => {
                if let Some(arg) = &ret.argument {
                    self.visit_expression(arg);
                }
            }
            Statement::IfStatement(if_stmt) => {
                self.visit_expression(&if_stmt.test);
                self.visit_statement(&if_stmt.consequent);
                if let Some(alt) = &if_stmt.alternate {
                    self.visit_statement(alt);
                }
            }
            Statement::ForStatement(for_stmt) => {
                match &for_stmt.init {
                    // `for (let i = 0; …)` — a declared loop variable is a
                    // local binding and stays unprefixed.
                    Some(ForStatementInit::VariableDeclaration(var_decl)) => {
                        self.visit_variable_declaration(var_decl);
                    }
                    // Every other `ForStatementInit` is an inherited
                    // `Expression`. `for (i = 0; …)` assigns to an EXISTING
                    // binding, so it is a real reference and must resolve like
                    // any other — leaving it bare produces a partially-resolved
                    // head (`for (i = 0; $setup.i < $setup.n; $setup.i++)`)
                    // whose write hits the setup-scope `const` directly.
                    Some(init) => {
                        if let Some(expr) = init.as_expression() {
                            self.visit_expression(expr);
                        }
                    }
                    None => {}
                }
                if let Some(test) = &for_stmt.test {
                    self.visit_expression(test);
                }
                if let Some(update) = &for_stmt.update {
                    self.visit_expression(update);
                }
                self.visit_statement(&for_stmt.body);
            }
            Statement::WhileStatement(while_stmt) => {
                self.visit_expression(&while_stmt.test);
                self.visit_statement(&while_stmt.body);
            }
            Statement::FunctionDeclaration(func) => {
                self.visit_function(func, func.span);
            }
            Statement::ClassDeclaration(class) => {
                // A class DECLARATION binds its name in the ENCLOSING scope, so
                // a later reference to it is local and stays unprefixed. The
                // heritage clause and the body are walked by the shared class
                // visit, which keeps the name in scope inside the body too.
                if let Some(id) = &class.id {
                    self.ctx.add_ignored(arena_name(id));
                }
                self.visit_class(class);
            }
            Statement::ThrowStatement(throw_stmt) => {
                self.visit_expression(&throw_stmt.argument);
            }
            Statement::LabeledStatement(labeled) => {
                self.visit_statement(&labeled.body);
            }
            Statement::DoWhileStatement(do_while) => {
                self.visit_statement(&do_while.body);
                self.visit_expression(&do_while.test);
            }
            Statement::SwitchStatement(switch_stmt) => {
                self.visit_expression(&switch_stmt.discriminant);
                for case in &switch_stmt.cases {
                    if let Some(test) = &case.test {
                        self.visit_expression(test);
                    }
                    for stmt in &case.consequent {
                        self.visit_statement(stmt);
                    }
                }
            }
            Statement::TryStatement(try_stmt) => {
                for stmt in &try_stmt.block.body {
                    self.visit_statement(stmt);
                }
                if let Some(handler) = &try_stmt.handler {
                    // The catch parameter is a clause-local binding — it must not
                    // be prefixed, and it must not leak into the enclosing scope.
                    let mut param_bytes: ParamBytes<'a> = SmallVec::new();
                    if let Some(param) = &handler.param {
                        self.collect_binding_pattern_bytes(&param.pattern, &mut param_bytes);
                    }
                    let child_ctx = self.ctx.child_with_ignored(param_bytes);
                    let mut child_visitor =
                        BindingVisitor::new(self.source_bytes, child_ctx, self.result);
                    for stmt in &handler.body.body {
                        child_visitor.visit_statement(stmt);
                    }
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    for stmt in &finalizer.body {
                        self.visit_statement(stmt);
                    }
                }
            }
            Statement::ForInStatement(for_in) => {
                self.visit_for_in_of(&for_in.left, &for_in.right, &for_in.body);
            }
            Statement::ForOfStatement(for_of) => {
                self.visit_for_in_of(&for_of.left, &for_of.right, &for_of.body);
            }
            Statement::WithStatement(with_stmt) => {
                // `with` is a strict-mode syntax error in the emitted module, so
                // the output is invalid whatever we do here. Resolving the object
                // and the body keeps the identifiers consistent with every other
                // statement form, which is also what `@vue/compiler-sfc` emits.
                self.visit_expression(&with_stmt.object);
                self.visit_statement(&with_stmt.body);
            }

            Statement::TSEnumDeclaration(enum_decl) => {
                // An enum binds its own name, and its members are in scope
                // inside later member initialisers (`A = 1, B = A + 1`), so both
                // are collected before any initialiser is walked.
                self.ctx.add_ignored(arena_name(&enum_decl.id));
                let mut member_names: ParamBytes<'a> = SmallVec::new();
                for member in &enum_decl.body.members {
                    if let TSEnumMemberName::Identifier(ident) = &member.id {
                        member_names.push(arena_ident_name(ident));
                    }
                }
                let child_ctx = self.ctx.child_with_ignored(member_names);
                let mut child_visitor =
                    BindingVisitor::new(self.source_bytes, child_ctx, self.result);
                for member in &enum_decl.body.members {
                    if let Some(init) = &member.initializer {
                        child_visitor.visit_expression(init);
                    }
                }
            }
            Statement::TSModuleDeclaration(module_decl) => {
                // `namespace X { … }` binds `X` in the enclosing scope; every
                // name declared inside is namespace-local, so the body is walked
                // in a child context.
                if let TSModuleDeclarationName::Identifier(id) = &module_decl.id {
                    self.ctx.add_ignored(arena_name(id));
                }
                if let Some(body) = &module_decl.body {
                    self.visit_ts_module_body(body);
                }
            }
            Statement::TSGlobalDeclaration(global_decl) => {
                // `declare global { … }` introduces no enclosing-scope binding;
                // its body is ambient, so it is walked in a child context.
                let child_ctx = self.ctx.child_with_ignored(SmallVec::new());
                let mut child_visitor =
                    BindingVisitor::new(self.source_bytes, child_ctx, self.result);
                for stmt in &global_decl.body.body {
                    child_visitor.visit_statement(stmt);
                }
            }
            Statement::TSImportEqualsDeclaration(import_equals) => {
                // `import X = A.B` binds `X`. The right-hand side is a TS entity
                // NAME, not an expression — it has no `Expression` node to walk
                // and no runtime position to resolve into.
                self.ctx.add_ignored(arena_name(&import_equals.id));
            }

            // Type-only declarations: erased before the emitted code runs, and
            // every identifier they mention is a TYPE reference, never a value.
            Statement::TSTypeAliasDeclaration(_) | Statement::TSInterfaceDeclaration(_) => {}

            // Statement forms whose every part is a keyword, a label, or
            // nothing at all — there is no identifier position to resolve.
            Statement::BreakStatement(_)
            | Statement::ContinueStatement(_)
            | Statement::DebuggerStatement(_)
            | Statement::EmptyStatement(_) => {}

            // Module-level syntax. A template value is emitted into an
            // expression position inside a render function, where `import` /
            // `export` is a syntax error regardless of how its identifiers
            // resolve, so there is nothing to usefully prefix. Listed one by one
            // rather than via `match_module_declaration!` so that a new variant
            // is a compile error here, not a silent miss.
            Statement::ImportDeclaration(_)
            | Statement::ExportAllDeclaration(_)
            | Statement::ExportDefaultDeclaration(_)
            | Statement::ExportNamedDeclaration(_)
            | Statement::TSExportAssignment(_)
            | Statement::TSNamespaceExportDeclaration(_) => {}
        }
    }

    /// Walk a `namespace` / `module` body in a child context.
    fn visit_ts_module_body(&mut self, body: &TSModuleDeclarationBody<'a>) {
        match body {
            TSModuleDeclarationBody::TSModuleDeclaration(inner) => {
                let child_ctx = self.ctx.child_with_ignored(SmallVec::new());
                let mut child_visitor =
                    BindingVisitor::new(self.source_bytes, child_ctx, self.result);
                if let TSModuleDeclarationName::Identifier(id) = &inner.id {
                    child_visitor.ctx.add_ignored(arena_name(id));
                }
                if let Some(inner_body) = &inner.body {
                    child_visitor.visit_ts_module_body(inner_body);
                }
            }
            TSModuleDeclarationBody::TSModuleBlock(block) => {
                let child_ctx = self.ctx.child_with_ignored(SmallVec::new());
                let mut child_visitor =
                    BindingVisitor::new(self.source_bytes, child_ctx, self.result);
                for stmt in &block.body {
                    child_visitor.visit_statement(stmt);
                }
            }
        }
    }

    /// Shared walk for a `class` in either DECLARATION or EXPRESSION position.
    ///
    /// Decorators and the heritage clause are evaluated in the ENCLOSING scope,
    /// so they are walked there — `class X extends Base {}` must resolve `Base`.
    /// The class NAME is in scope only inside the class, so the body is walked
    /// in a child context with the name ignored; a class DECLARATION
    /// additionally binds the name in the enclosing scope, which its caller
    /// records.
    fn visit_class(&mut self, class: &Class<'a>) {
        for decorator in &class.decorators {
            self.visit_expression(&decorator.expression);
        }
        if let Some(super_class) = &class.super_class {
            self.visit_expression(super_class);
        }

        let mut class_local: ParamBytes<'a> = SmallVec::new();
        if let Some(id) = &class.id {
            class_local.push(arena_name(id));
        }
        let child_ctx = self.ctx.child_with_ignored(class_local);
        let mut child_visitor = BindingVisitor::new(self.source_bytes, child_ctx, self.result);
        for element in &class.body.body {
            child_visitor.visit_class_element(element);
        }
    }

    /// Walk one class element. Exhaustive for the same reason
    /// [`Self::visit_statement`] is.
    fn visit_class_element(&mut self, element: &ClassElement<'a>) {
        match element {
            ClassElement::StaticBlock(block) => {
                for stmt in &block.body {
                    self.visit_statement(stmt);
                }
            }
            ClassElement::MethodDefinition(method) => {
                for decorator in &method.decorators {
                    self.visit_expression(&decorator.expression);
                }
                if method.computed {
                    self.visit_property_key(&method.key);
                }
                self.visit_function(&method.value, method.value.span);
            }
            ClassElement::PropertyDefinition(prop) => {
                for decorator in &prop.decorators {
                    self.visit_expression(&decorator.expression);
                }
                if prop.computed {
                    self.visit_property_key(&prop.key);
                }
                if let Some(value) = &prop.value {
                    self.visit_expression(value);
                }
            }
            ClassElement::AccessorProperty(prop) => {
                for decorator in &prop.decorators {
                    self.visit_expression(&decorator.expression);
                }
                if prop.computed {
                    self.visit_property_key(&prop.key);
                }
                if let Some(value) = &prop.value {
                    self.visit_expression(value);
                }
            }
            // `[key: string]: number` is types only — no value position.
            ClassElement::TSIndexSignature(_) => {}
        }
    }

    /// Shared walk for `for…in` / `for…of`.
    ///
    /// A DECLARED loop variable (`for (const y of ys)`) is local to the head and
    /// the body, so it is collected as an ignored name rather than recorded as a
    /// reference. Any other target ASSIGNS to an existing binding once per
    /// iteration — an identifier, a member expression or a destructuring pattern
    /// — and every one of those is a real reference.
    ///
    /// Head then body, and within the head the target then the iterated
    /// expression: source order throughout, like every other arm. This arm used
    /// to visit the iterated expression first, reasoning that it is EVALUATED in
    /// the enclosing scope — true, but not something visit order expresses.
    /// Scope is decided by `declared` and the child context built from it, both
    /// of which come after the whole head, so the two head positions can be
    /// visited in either order. Source order is the one a reader can check
    /// against the grammar.
    fn visit_for_in_of(
        &mut self,
        left: &ForStatementLeft<'a>,
        right: &Expression<'a>,
        body: &Statement<'a>,
    ) {
        let mut declared: ParamBytes<'a> = SmallVec::new();
        match left {
            ForStatementLeft::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    self.collect_binding_pattern_bytes(&declarator.id, &mut declared);
                }
            }
            _ => {
                if let Some(target) = left.as_assignment_target() {
                    self.visit_assignment_target(target);
                }
            }
        }

        // The iterated expression is evaluated in the ENCLOSING scope, so it is
        // visited against `self.ctx` — before the child context below adds the
        // declared loop variable, which must not shadow a same-named reference
        // here.
        self.visit_expression(right);

        let child_ctx = self.ctx.child_with_ignored(declared);
        let mut child_visitor = BindingVisitor::new(self.source_bytes, child_ctx, self.result);
        child_visitor.visit_statement(body);
    }

    #[inline]
    fn visit_expression(&mut self, expr: &Expression<'a>) {
        match expr {
            Expression::Identifier(ident) => {
                self.add_binding(ident.name.as_str(), ident.span);
            }

            Expression::BooleanLiteral(lit) => {
                self.add_literal(lit.span);
            }
            Expression::NullLiteral(lit) => {
                self.add_literal(lit.span);
            }
            Expression::NumericLiteral(lit) => {
                self.add_literal(lit.span);
            }
            Expression::StringLiteral(lit) => {
                self.add_literal(lit.span);
            }
            Expression::BigIntLiteral(lit) => {
                self.add_literal(lit.span);
            }
            Expression::RegExpLiteral(lit) => {
                self.add_literal(lit.span);
            }

            Expression::TemplateLiteral(template) => {
                for expr in &template.expressions {
                    self.visit_expression(expr);
                }
            }

            Expression::TaggedTemplateExpression(tagged) => {
                self.visit_expression(&tagged.tag);
                for expr in &tagged.quasi.expressions {
                    self.visit_expression(expr);
                }
            }

            Expression::ComputedMemberExpression(computed) => {
                self.visit_expression(&computed.object);
                self.visit_expression(&computed.expression);
            }
            Expression::StaticMemberExpression(static_member) => {
                self.visit_expression(&static_member.object);
            }
            Expression::PrivateFieldExpression(private) => {
                self.visit_expression(&private.object);
            }

            Expression::CallExpression(call) => {
                self.visit_expression(&call.callee);
                for arg in &call.arguments {
                    self.visit_argument(arg);
                }
            }

            Expression::NewExpression(new_expr) => {
                self.visit_expression(&new_expr.callee);
                for arg in &new_expr.arguments {
                    self.visit_argument(arg);
                }
            }

            Expression::ArrayExpression(array) => {
                for elem in &array.elements {
                    self.visit_array_expression_element(elem);
                }
            }

            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ObjectPropertyKind::ObjectProperty(prop) => {
                            if prop.computed {
                                self.visit_property_key(&prop.key);
                            }
                            if prop.shorthand {
                                // For shorthand `{ foo }`, bind from the VALUE
                                // (Expression::Identifier); its span covers the same `foo`
                                // as the key. Mark as shorthand so prefixing expands to
                                // key: value form (e.g., `{ foo }` → `{ foo: _ctx.foo }`
                                // not `{ _ctx.foo }`).
                                if let Expression::Identifier(ident) = &prop.value {
                                    self.add_shorthand_binding(ident.name.as_str(), ident.span);
                                } else {
                                    self.visit_expression(&prop.value);
                                }
                            } else {
                                self.visit_expression(&prop.value);
                            }
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            self.visit_expression(&spread.argument);
                        }
                    }
                }
            }

            Expression::ArrowFunctionExpression(arrow) => {
                self.visit_arrow_function(arrow);
            }

            Expression::FunctionExpression(func) => {
                self.visit_function(func, func.span);
            }

            Expression::BinaryExpression(binary) => {
                self.visit_expression(&binary.left);
                self.visit_expression(&binary.right);
            }

            Expression::UnaryExpression(unary) => {
                self.visit_expression(&unary.argument);
            }

            Expression::UpdateExpression(update) => {
                self.visit_simple_assignment_target(&update.argument);
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

            Expression::AssignmentExpression(assign) => {
                self.visit_assignment_target(&assign.left);
                self.visit_expression(&assign.right);
            }

            Expression::SequenceExpression(seq) => {
                for expr in &seq.expressions {
                    self.visit_expression(expr);
                }
            }

            Expression::ParenthesizedExpression(paren) => {
                self.visit_expression(&paren.expression);
            }

            Expression::ChainExpression(chain) => {
                self.visit_chain_element(&chain.expression);
            }

            Expression::AwaitExpression(await_expr) => {
                self.visit_expression(&await_expr.argument);
            }

            Expression::YieldExpression(yield_expr) => {
                if let Some(arg) = &yield_expr.argument {
                    self.visit_expression(arg);
                }
            }

            Expression::TSAsExpression(ts_as) => {
                self.visit_expression(&ts_as.expression);
            }

            Expression::TSSatisfiesExpression(satisfies) => {
                self.visit_expression(&satisfies.expression);
            }

            Expression::TSNonNullExpression(non_null) => {
                self.visit_expression(&non_null.expression);
            }

            Expression::TSTypeAssertion(assertion) => {
                self.visit_expression(&assertion.expression);
            }

            Expression::TSInstantiationExpression(instantiation) => {
                self.visit_expression(&instantiation.expression);
            }

            Expression::ClassExpression(class) => {
                // A class EXPRESSION's name is in scope only INSIDE the class,
                // so it is not registered in the enclosing context here — the
                // shared class visit keeps it local to the body.
                self.visit_class(class);
            }

            Expression::PrivateInExpression(private_in) => {
                // `#field in obj` — the left side is a private name, the right
                // side is an ordinary expression.
                self.visit_expression(&private_in.right);
            }

            Expression::ThisExpression(_) => {}
            Expression::Super(_) => {}

            Expression::ImportExpression(import) => {
                self.visit_expression(&import.source);
                if let Some(options) = &import.options {
                    self.visit_expression(options);
                }
            }

            Expression::MetaProperty(_) => {}

            Expression::V8IntrinsicExpression(intrinsic) => {
                // `%Foo(a, b)` — the name is an intrinsic, the arguments are
                // ordinary expressions.
                for arg in &intrinsic.arguments {
                    self.visit_argument(arg);
                }
            }

            // A template value has no JSX projection: JSX inside a `v-on` value
            // or an interpolation is emitted verbatim into a render function
            // that is plain JS, so its identifiers have no resolvable position.
            // Listed explicitly to keep this match exhaustive.
            Expression::JSXElement(_) | Expression::JSXFragment(_) => {}
        }
    }

    #[inline]
    fn visit_array_expression_element(&mut self, elem: &ArrayExpressionElement<'a>) {
        match elem {
            ArrayExpressionElement::SpreadElement(spread) => {
                self.visit_expression(&spread.argument);
            }
            ArrayExpressionElement::Elision(_) => {}
            _ => {
                if let Some(expr) = elem.as_expression() {
                    self.visit_expression(expr);
                }
            }
        }
    }

    #[inline]
    fn visit_chain_element(&mut self, element: &ChainElement<'a>) {
        match element {
            ChainElement::CallExpression(call) => {
                self.visit_expression(&call.callee);
                for arg in &call.arguments {
                    self.visit_argument(arg);
                }
            }
            ChainElement::TSNonNullExpression(non_null) => {
                self.visit_expression(&non_null.expression);
            }
            ChainElement::ComputedMemberExpression(member) => {
                self.visit_expression(&member.object);
                self.visit_expression(&member.expression);
            }
            ChainElement::StaticMemberExpression(member) => {
                self.visit_expression(&member.object);
            }
            ChainElement::PrivateFieldExpression(member) => {
                self.visit_expression(&member.object);
            }
        }
    }

    #[inline]
    fn visit_property_key(&mut self, key: &PropertyKey<'a>) {
        match key {
            PropertyKey::StaticIdentifier(_) => {}
            PropertyKey::PrivateIdentifier(_) => {}
            _ => {
                if let Some(expr) = key.as_expression() {
                    self.visit_expression(expr);
                }
            }
        }
    }

    #[inline]
    fn visit_argument(&mut self, arg: &Argument<'a>) {
        match arg {
            Argument::SpreadElement(spread) => {
                self.visit_expression(&spread.argument);
            }
            _ => {
                if let Some(expr) = arg.as_expression() {
                    self.visit_expression(expr);
                }
            }
        }
    }

    #[inline]
    fn visit_simple_assignment_target(&mut self, target: &SimpleAssignmentTarget<'a>) {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => {
                self.add_binding(ident.name.as_str(), ident.span);
            }
            SimpleAssignmentTarget::TSAsExpression(ts_as) => {
                self.visit_expression(&ts_as.expression);
            }
            SimpleAssignmentTarget::TSSatisfiesExpression(satisfies) => {
                self.visit_expression(&satisfies.expression);
            }
            SimpleAssignmentTarget::TSNonNullExpression(non_null) => {
                self.visit_expression(&non_null.expression);
            }
            SimpleAssignmentTarget::TSTypeAssertion(assertion) => {
                self.visit_expression(&assertion.expression);
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.visit_expression(&member.object);
                self.visit_expression(&member.expression);
            }
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.visit_expression(&member.object);
            }
            SimpleAssignmentTarget::PrivateFieldExpression(member) => {
                self.visit_expression(&member.object);
            }
        }
    }

    fn visit_assignment_target(&mut self, target: &AssignmentTarget<'a>) {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(ident) => {
                self.add_binding(ident.name.as_str(), ident.span);
            }
            AssignmentTarget::TSAsExpression(ts_as) => {
                self.visit_expression(&ts_as.expression);
            }
            AssignmentTarget::TSSatisfiesExpression(satisfies) => {
                self.visit_expression(&satisfies.expression);
            }
            AssignmentTarget::TSNonNullExpression(non_null) => {
                self.visit_expression(&non_null.expression);
            }
            AssignmentTarget::TSTypeAssertion(assertion) => {
                self.visit_expression(&assertion.expression);
            }
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.visit_expression(&member.object);
                self.visit_expression(&member.expression);
            }
            AssignmentTarget::StaticMemberExpression(member) => {
                self.visit_expression(&member.object);
            }
            AssignmentTarget::PrivateFieldExpression(member) => {
                self.visit_expression(&member.object);
            }
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                for elem in array.elements.iter().flatten() {
                    self.visit_assignment_target_maybe_default(elem);
                }
                if let Some(rest) = &array.rest {
                    self.visit_assignment_target_rest(rest);
                }
            }
            AssignmentTarget::ObjectAssignmentTarget(obj) => {
                for prop in &obj.properties {
                    match prop {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(ident) => {
                            self.add_binding(ident.binding.name.as_str(), ident.binding.span);
                            if let Some(init) = &ident.init {
                                self.visit_expression(init);
                            }
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(prop) => {
                            self.visit_assignment_target_maybe_default(&prop.binding);
                        }
                    }
                }
                if let Some(rest) = &obj.rest {
                    self.visit_assignment_target_rest(rest);
                }
            }
        }
    }

    fn visit_assignment_target_maybe_default(&mut self, target: &AssignmentTargetMaybeDefault<'a>) {
        match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
                self.visit_assignment_target(&with_default.binding);
                self.visit_expression(&with_default.init);
            }
            _ => {
                if let Some(target) = target.as_assignment_target() {
                    self.visit_assignment_target(target);
                }
            }
        }
    }

    fn visit_assignment_target_rest(&mut self, rest: &AssignmentTargetRest<'a>) {
        self.visit_assignment_target(&rest.target);
    }

    fn visit_arrow_function(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        let mut param_bytes: ParamBytes<'a> = SmallVec::new();
        for param in &arrow.params.items {
            self.collect_binding_pattern_bytes(&param.pattern, &mut param_bytes);
        }
        if let Some(rest) = &arrow.params.rest {
            self.collect_binding_pattern_bytes(&rest.rest.argument, &mut param_bytes);
        }

        // Visit default values in parameters
        for param in &arrow.params.items {
            if let Some(init) = &param.initializer {
                self.visit_expression(init);
            }
            self.visit_binding_pattern_defaults(&param.pattern);
        }

        // Record the function
        self.result.functions.push(FunctionBinding {
            span: arrow.span.into(),
            body_span: arrow.body.span.into(),
            pos: arrow.span.start + self.ctx.base_offset,
            body_pos: arrow.body.span.start + self.ctx.base_offset,
        });

        // Visit body with extended context
        let child_ctx = self.ctx.child_with_ignored(param_bytes);
        let mut child_visitor = BindingVisitor::new(self.source_bytes, child_ctx, self.result);

        if arrow.expression {
            if let Some(Statement::ExpressionStatement(expr_stmt)) = arrow.body.statements.first() {
                child_visitor.visit_expression(&expr_stmt.expression);
            }
        } else {
            for stmt in &arrow.body.statements {
                child_visitor.visit_statement(stmt);
            }
        }
    }

    fn visit_function(&mut self, func: &Function<'a>, span: OxcSpan) {
        let mut param_bytes: ParamBytes<'a> = SmallVec::new();
        if let Some(id) = &func.id {
            param_bytes.push(arena_name(id));
        }

        for param in &func.params.items {
            self.collect_binding_pattern_bytes(&param.pattern, &mut param_bytes);
        }
        if let Some(rest) = &func.params.rest {
            self.collect_binding_pattern_bytes(&rest.rest.argument, &mut param_bytes);
        }

        // Visit default values in parameters
        for param in &func.params.items {
            if let Some(init) = &param.initializer {
                self.visit_expression(init);
            }
            self.visit_binding_pattern_defaults(&param.pattern);
        }

        // Record the function
        if let Some(body) = &func.body {
            self.result.functions.push(FunctionBinding {
                span: span.into(),
                body_span: body.span.into(),
                pos: span.start + self.ctx.base_offset,
                body_pos: body.span.start + self.ctx.base_offset,
            });

            let child_ctx = self.ctx.child_with_ignored(param_bytes);
            let mut child_visitor = BindingVisitor::new(self.source_bytes, child_ctx, self.result);

            for stmt in &body.statements {
                child_visitor.visit_statement(stmt);
            }
        }
    }

    fn visit_variable_declaration(&mut self, var_decl: &VariableDeclaration<'a>) {
        for declarator in &var_decl.declarations {
            let mut declared_bytes: ParamBytes<'a> = SmallVec::new();
            self.collect_binding_pattern_bytes(&declarator.id, &mut declared_bytes);

            for name_bytes in declared_bytes {
                self.ctx.add_ignored(name_bytes);
            }

            if let Some(init) = &declarator.init {
                self.visit_expression(init);
            }
        }
    }

    fn collect_binding_pattern_bytes(
        &self,
        pattern: &BindingPattern<'a>,
        bytes: &mut ParamBytes<'a>,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(ident) => {
                bytes.push(arena_name(ident));
            }
            BindingPattern::ObjectPattern(obj) => {
                for prop in &obj.properties {
                    self.collect_binding_pattern_bytes(&prop.value, bytes);
                }
                if let Some(rest) = &obj.rest {
                    self.collect_binding_pattern_bytes(&rest.argument, bytes);
                }
            }
            BindingPattern::ArrayPattern(arr) => {
                for elem in arr.elements.iter().flatten() {
                    self.collect_binding_pattern_bytes(elem, bytes);
                }
                if let Some(rest) = &arr.rest {
                    self.collect_binding_pattern_bytes(&rest.argument, bytes);
                }
            }
            BindingPattern::AssignmentPattern(assign) => {
                self.collect_binding_pattern_bytes(&assign.left, bytes);
            }
        }
    }

    /// Walk the DEFAULT-value expressions of a binding pattern.
    ///
    /// The pattern's own names are locals, collected by
    /// [`Self::collect_binding_pattern_bytes`]; only the `= expr` right sides
    /// are references. Exhaustive for the same reason
    /// [`Self::visit_statement`] is: this is the last catch-all on the descent,
    /// and a `BindingPattern` variant added by a future OXC release would
    /// otherwise be swallowed here, silently emitting whatever identifiers its
    /// defaults mention as bare references.
    fn visit_binding_pattern_defaults(&mut self, pattern: &BindingPattern<'a>) {
        match pattern {
            // A bare name has no default to walk; the name itself is a local.
            BindingPattern::BindingIdentifier(_) => {}
            BindingPattern::AssignmentPattern(assign) => {
                self.visit_expression(&assign.right);
                self.visit_binding_pattern_defaults(&assign.left);
            }
            BindingPattern::ObjectPattern(obj) => {
                for prop in &obj.properties {
                    self.visit_binding_pattern_defaults(&prop.value);
                }
                if let Some(rest) = &obj.rest {
                    self.visit_binding_pattern_defaults(&rest.argument);
                }
            }
            BindingPattern::ArrayPattern(arr) => {
                for elem in arr.elements.iter().flatten() {
                    self.visit_binding_pattern_defaults(elem);
                }
                if let Some(rest) = &arr.rest {
                    self.visit_binding_pattern_defaults(&rest.argument);
                }
            }
        }
    }

    #[inline]
    fn add_binding(&mut self, name: &'a str, span: OxcSpan) {
        self.add_binding_inner(name, span, false);
    }

    #[inline]
    fn add_shorthand_binding(&mut self, name: &'a str, span: OxcSpan) {
        self.add_binding_inner(name, span, true);
    }

    #[inline]
    fn add_binding_inner(&mut self, name: &'a str, span: OxcSpan, is_shorthand: bool) {
        use super::types::Dynamism;
        let ignore = self.ctx.should_ignore(name);
        // `push_binding`, not `bindings.push`: the vector is source-ordered, and
        // that is the recorder's job rather than every walker arm's.
        self.result.push_binding(Binding {
            name,
            span: span.into(),
            pos: span.start + self.ctx.base_offset,
            ignore,
            is_shorthand,
        });

        // Incrementally update dynamism — avoids a separate post-extraction loop.
        // Dynamic trumps MaybeDynamic trumps Static.
        if self.result.dynamism != Dynamism::Dynamic {
            if ignore && !is_keyword(name.as_bytes()) && !is_global(name.as_bytes()) {
                // Injected local (v-for/v-slot variable, not a JS keyword or global)
                self.result.dynamism = Dynamism::Dynamic;
            } else if !ignore {
                // Script-level identifier reference
                self.result.dynamism = Dynamism::MaybeDynamic;
            }
            // keyword-ignored → no change (keywords don't affect dynamism)
        }
    }

    #[inline]
    fn add_literal(&mut self, span: OxcSpan) {
        let start = span.start as usize;
        let end = span.end as usize;
        if end > self.source_bytes.len() {
            eprintln!(
                "[verter] BUG: OXC literal span {}..{} exceeds source length {}, skipping",
                start,
                end,
                self.source_bytes.len(),
            );
            return;
        }
        let bytes = &self.source_bytes[start..end];
        let content = match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                eprintln!(
                    "[verter] BUG: OXC literal span {}..{} is not valid UTF-8, skipping",
                    start, end,
                );
                return;
            }
        };
        self.result.literals.push(LiteralBinding {
            span: span.into(),
            pos: span.start + self.ctx.base_offset,
            content,
        });
    }
}

#[cfg(test)]
#[path = "expression_tests.rs"]
mod expression_tests;
