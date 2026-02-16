//! Expression binding extraction.
//!
//! This module provides the main entry points for extracting bindings from
//! JavaScript/TypeScript expressions parsed by OXC.

use oxc_ast::ast::*;
use oxc_span::Span as OxcSpan;
use smallvec::SmallVec;

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
/// let result = extract_bindings_from_expression(&expr, "foo + bar", &ctx);
/// assert_eq!(result.non_ignored_binding_names(), vec!["foo", "bar"]);
/// ```
pub fn extract_bindings_from_expression<'a>(
    expr: &Expression<'a>,
    source: &'a str,
    ctx: &BindingContext<'a>,
) -> BindingExtractionResult<'a> {
    let mut result = BindingExtractionResult::default();
    let source_bytes = source.as_bytes();
    let mut visitor = BindingVisitor::new(source_bytes, ctx.clone(), &mut result);
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
    ctx: &BindingContext<'a>,
) -> BindingExtractionResult<'a> {
    let mut result = BindingExtractionResult::default();
    let source_bytes = source.as_bytes();
    let mut visitor = BindingVisitor::new(source_bytes, ctx.clone(), &mut result);

    for stmt in &program.body {
        visitor.visit_statement(stmt);
    }

    result
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
                if let Some(ForStatementInit::VariableDeclaration(var_decl)) = &for_stmt.init {
                    self.visit_variable_declaration(var_decl);
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
            _ => {}
        }
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
                                // For shorthand `{ foo }`, use the VALUE (Expression::Identifier)
                                // rather than the KEY (PropertyKey::StaticIdentifier).
                                // StaticIdentifier key spans are NOT adjusted by
                                // adjust_expression_spans (only expression-convertible keys are),
                                // so the key span stays substring-relative. The value's Identifier
                                // span IS correctly adjusted to file-relative positions.
                                //
                                // Mark as shorthand so prefixing expands to key: value form
                                // (e.g., `{ foo }` → `{ foo: _ctx.foo }` not `{ _ctx.foo }`).
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
                if let Some(id) = &class.id {
                    self.ctx.add_ignored(unsafe {
                        std::mem::transmute::<&str, &'a str>(id.name.as_str())
                    });
                }
                for element in &class.body.body {
                    match element {
                        ClassElement::MethodDefinition(method) => {
                            if method.computed {
                                self.visit_property_key(&method.key);
                            }
                            self.visit_function(&method.value, method.value.span);
                        }
                        ClassElement::PropertyDefinition(prop) => {
                            if prop.computed {
                                self.visit_property_key(&prop.key);
                            }
                            if let Some(value) = &prop.value {
                                self.visit_expression(value);
                            }
                        }
                        ClassElement::StaticBlock(block) => {
                            for stmt in &block.body {
                                self.visit_statement(stmt);
                            }
                        }
                        _ => {}
                    }
                }
            }

            Expression::ThisExpression(_) => {}
            Expression::Super(_) => {}

            Expression::ImportExpression(import) => {
                self.visit_expression(&import.source);
            }

            Expression::MetaProperty(_) => {}

            _ => {}
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
            param_bytes.push(unsafe { std::mem::transmute::<&str, &'a str>(id.name.as_str()) });
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
                // SAFETY: The lifetime is tied to the AST which outlives our usage
                bytes.push(unsafe { std::mem::transmute::<&str, &'a str>(ident.name.as_str()) });
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

    fn visit_binding_pattern_defaults(&mut self, pattern: &BindingPattern<'a>) {
        match pattern {
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
            _ => {}
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
        let ignore = self.ctx.should_ignore(name);
        self.result.bindings.push(Binding {
            name,
            span: span.into(),
            pos: span.start + self.ctx.base_offset,
            ignore,
            is_shorthand,
        });
    }

    #[inline]
    fn add_literal(&mut self, span: OxcSpan) {
        let start = span.start as usize;
        let end = span.end as usize;
        // SAFETY: Spans from OXC parser are guaranteed to be valid byte offsets
        // within the source string, and source is valid UTF-8
        let content = unsafe {
            let bytes = self.source_bytes.get_unchecked(start..end);
            std::str::from_utf8_unchecked(bytes)
        };
        self.result.literals.push(LiteralBinding {
            span: span.into(),
            pos: span.start + self.ctx.base_offset,
            content,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    /// Helper to parse and extract bindings from an expression
    fn extract(source: &str) -> (Vec<String>, bool, Vec<u32>) {
        extract_with_offset(source, 0)
    }

    fn extract_with_offset(source: &str, offset: u32) -> (Vec<String>, bool, Vec<u32>) {
        let allocator = Allocator::default();
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        match parser.parse_expression() {
            Ok(expr) => {
                let ctx = BindingContext::new(offset);
                let result = extract_bindings_from_expression(&expr, source, &ctx);
                let names: Vec<String> = result
                    .non_ignored_binding_names()
                    .into_iter()
                    .map(String::from)
                    .collect();
                let positions: Vec<u32> = result
                    .bindings
                    .iter()
                    .filter(|b| !b.ignore)
                    .map(|b| b.pos)
                    .collect();
                (names, result.has_functions(), positions)
            }
            Err(_) => (vec![], false, vec![]),
        }
    }

    // ===========================================
    // Simple identifiers
    // ===========================================

    #[test]
    fn test_single_identifier() {
        let (names, _, positions) = extract("foo");
        assert_eq!(names, vec!["foo"]);
        assert_eq!(positions, vec![0]);
    }

    #[test]
    fn test_single_identifier_with_offset() {
        let (names, _, positions) = extract_with_offset("foo", 100);
        assert_eq!(names, vec!["foo"]);
        assert_eq!(positions, vec![100]);
    }

    #[test]
    fn test_multiple_identifiers() {
        let (names, _, positions) = extract("foo + bar");
        assert_eq!(names, vec!["foo", "bar"]);
        assert_eq!(positions, vec![0, 6]);
    }

    #[test]
    fn test_keywords_ignored() {
        let (names, _, _) = extract("true && false || null");
        assert!(names.is_empty());
    }

    #[test]
    fn test_undefined_ignored() {
        let (names, _, _) = extract("foo ?? undefined");
        assert_eq!(names, vec!["foo"]);
    }

    // ===========================================
    // Member expressions
    // ===========================================

    #[test]
    fn test_simple_member() {
        let (names, _, _) = extract("foo.bar");
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn test_computed_member() {
        let (names, _, _) = extract("foo[bar]");
        assert_eq!(names, vec!["foo", "bar"]);
    }

    #[test]
    fn test_chained_member() {
        let (names, _, _) = extract("foo.bar.baz");
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn test_mixed_member() {
        let (names, _, _) = extract("foo.bar[baz].qux");
        assert_eq!(names, vec!["foo", "baz"]);
    }

    // ===========================================
    // Function calls
    // ===========================================

    #[test]
    fn test_function_call() {
        let (names, _, _) = extract("foo()");
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn test_function_call_with_args() {
        let (names, _, _) = extract("foo(bar, baz)");
        assert_eq!(names, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_method_call() {
        let (names, _, _) = extract("foo.bar(baz)");
        assert_eq!(names, vec!["foo", "baz"]);
    }

    // ===========================================
    // Arrow functions
    // ===========================================

    #[test]
    fn test_arrow_function_simple() {
        let (names, has_funcs, _) = extract("() => foo");
        assert!(has_funcs);
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn test_arrow_function_with_param() {
        let (names, has_funcs, _) = extract("(x) => x + foo");
        assert!(has_funcs);
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn test_arrow_function_with_default() {
        let (names, has_funcs, _) = extract("(x = defaultVal) => x + foo");
        assert!(has_funcs);
        assert_eq!(names, vec!["defaultVal", "foo"]);
    }

    #[test]
    fn test_arrow_function_with_destructuring() {
        let (names, has_funcs, _) = extract("({ a, b }) => a + b + foo");
        assert!(has_funcs);
        assert_eq!(names, vec!["foo"]);
    }

    // ===========================================
    // Object literals
    // ===========================================

    #[test]
    fn test_object_property() {
        let (names, _, _) = extract("{ foo: bar }");
        assert_eq!(names, vec!["bar"]);
    }

    #[test]
    fn test_object_shorthand() {
        let (names, _, _) = extract("{ foo }");
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn test_object_spread() {
        let (names, _, _) = extract("{ ...obj, foo: bar }");
        assert_eq!(names, vec!["obj", "bar"]);
    }

    // ===========================================
    // Array literals
    // ===========================================

    #[test]
    fn test_array() {
        let (names, _, _) = extract("[foo, bar, baz]");
        assert_eq!(names, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_array_spread() {
        let (names, _, _) = extract("[...arr, foo]");
        assert_eq!(names, vec!["arr", "foo"]);
    }

    // ===========================================
    // Ternary
    // ===========================================

    #[test]
    fn test_ternary() {
        let (names, _, _) = extract("cond ? foo : bar");
        assert_eq!(names, vec!["cond", "foo", "bar"]);
    }

    // ===========================================
    // Template literals
    // ===========================================

    #[test]
    fn test_template_literal() {
        let (names, _, _) = extract("`hello ${name}`");
        assert_eq!(names, vec!["name"]);
    }

    #[test]
    fn test_tagged_template() {
        let (names, _, _) = extract("tag`hello ${name}`");
        assert_eq!(names, vec!["tag", "name"]);
    }

    // ===========================================
    // TypeScript
    // ===========================================

    #[test]
    fn test_as_assertion() {
        let (names, _, _) = extract("foo as string");
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn test_non_null_assertion() {
        let (names, _, _) = extract("foo!");
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn test_satisfies() {
        let (names, _, _) = extract("foo satisfies MyType");
        assert_eq!(names, vec!["foo"]);
    }

    // ===========================================
    // Edge cases
    // ===========================================

    #[test]
    fn test_deeply_nested() {
        let (names, _, _) = extract("a.b.c.d.e.f.g.h(i, j, k).l.m[n].o");
        assert_eq!(names, vec!["a", "i", "j", "k", "n"]);
    }

    #[test]
    fn test_multiple_arrow_functions() {
        let (names, has_funcs, _) = extract("(x) => (y) => (z) => x + y + z + foo");
        assert!(has_funcs);
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn test_iife() {
        let (names, has_funcs, _) = extract("((x) => x + foo)(bar)");
        assert!(has_funcs);
        assert_eq!(names, vec!["foo", "bar"]);
    }

    #[test]
    fn test_optional_chaining() {
        let (names, _, _) = extract("foo?.bar?.baz");
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn test_nullish_coalescing() {
        let (names, _, _) = extract("foo ?? bar");
        assert_eq!(names, vec!["foo", "bar"]);
    }
}
