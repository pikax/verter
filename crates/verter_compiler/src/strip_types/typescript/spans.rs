use oxc_ast::ast::*;
use oxc_span::GetSpan;

// =========================================================================
// TS removal span collection (for template directive expressions)
// =========================================================================

/// Collect TypeScript-only byte ranges from a single expression AST.
///
/// Returns expression-relative `(start, end)` spans sorted by start position.
/// Used by `build_prefixed_expr()` to skip TS syntax when building resolved
/// expression strings for directive values (v-bind, v-if, etc.).
///
/// Only collects `remove` operations — template expressions never contain
/// enum declarations or class member accessibility modifiers that need `overwrite`.
pub fn collect_ts_removal_spans(expression: &Expression) -> Vec<(u32, u32)> {
    collect_ts_removal_spans_with_annotation_offset(expression, 0)
}

/// A retained slot tree keeps the synthetic wrapper prefix on annotation nodes.
/// Value spans and type-argument spans already use their authored coordinates.
pub(crate) fn collect_ts_removal_spans_with_annotation_offset(
    expression: &Expression,
    annotation_offset: u32,
) -> Vec<(u32, u32)> {
    let mut out = RemovalSpans::new(annotation_offset);
    collect_expr_ts_spans(expression, &mut out);
    out.finish()
}

/// Collect type-only ranges from a retained statement handler program.
pub(crate) fn collect_program_ts_removal_spans(program: &Program) -> Vec<(u32, u32)> {
    let mut out = RemovalSpans::new(0);
    for statement in &program.body {
        collect_stmt_ts_spans(statement, &mut out);
    }
    out.finish()
}

/// Formal parameters keep the coordinate policy of their owning parse.
pub(crate) fn collect_parameter_ts_removal_spans(
    params: &FormalParameters,
    annotation_offset: u32,
) -> Vec<(u32, u32)> {
    let mut out = RemovalSpans::new(annotation_offset);
    collect_formal_params_ts_spans(params, &mut out);
    out.finish()
}

struct RemovalSpans {
    spans: Vec<(u32, u32)>,
    annotation_offset: u32,
}
impl RemovalSpans {
    fn new(annotation_offset: u32) -> Self {
        Self {
            spans: Vec::new(),
            annotation_offset,
        }
    }
    fn push(&mut self, range: (u32, u32)) {
        self.spans.push(range);
    }
    fn annotation(&mut self, span: oxc_span::Span) {
        self.push((
            span.start - self.annotation_offset,
            span.end - self.annotation_offset,
        ));
    }
    fn finish(mut self) -> Vec<(u32, u32)> {
        self.spans.sort_by_key(|&(start, _)| start);
        self.spans
    }
}

fn collect_expr_ts_spans(expr: &Expression, out: &mut RemovalSpans) {
    match expr {
        // TS assertion expressions — remove type annotation, keep value
        Expression::TSAsExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        Expression::TSSatisfiesExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        Expression::TSNonNullExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        Expression::TSTypeAssertion(e) => {
            out.push((e.span.start, e.expression.span().start));
            collect_expr_ts_spans(&e.expression, out);
        }
        Expression::TSInstantiationExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }

        // Expressions with type arguments
        Expression::CallExpression(call) => {
            if let Some(ta) = &call.type_arguments {
                out.push((ta.span.start, ta.span.end));
            }
            collect_expr_ts_spans(&call.callee, out);
            collect_args_ts_spans(&call.arguments, out);
        }
        Expression::NewExpression(new_expr) => {
            if let Some(ta) = &new_expr.type_arguments {
                out.push((ta.span.start, ta.span.end));
            }
            collect_expr_ts_spans(&new_expr.callee, out);
            collect_args_ts_spans(&new_expr.arguments, out);
        }
        Expression::TaggedTemplateExpression(tagged) => {
            if let Some(ta) = &tagged.type_arguments {
                out.push((ta.span.start, ta.span.end));
            }
            collect_expr_ts_spans(&tagged.tag, out);
            for e in &tagged.quasi.expressions {
                collect_expr_ts_spans(e, out);
            }
        }

        // Function expressions with type annotations
        Expression::ArrowFunctionExpression(arrow) => {
            if let Some(tp) = &arrow.type_parameters {
                out.push((tp.span.start, tp.span.end));
            }
            if let Some(rt) = &arrow.return_type {
                out.annotation(rt.span);
            }
            collect_formal_params_ts_spans(&arrow.params, out);
            if let Some(expression) = arrow.get_expression() {
                // An expression body collects as its one expression statement.
                collect_expr_ts_spans(expression, out);
            } else if let Some(body) = arrow.get_function_body() {
                for stmt in &body.statements {
                    collect_stmt_ts_spans(stmt, out);
                }
            }
        }
        Expression::FunctionExpression(func) => {
            if let Some(tp) = &func.type_parameters {
                out.push((tp.span.start, tp.span.end));
            }
            if let Some(rt) = &func.return_type {
                out.annotation(rt.span);
            }
            collect_formal_params_ts_spans(&func.params, out);
            if let Some(body) = &func.body {
                for stmt in &body.statements {
                    collect_stmt_ts_spans(stmt, out);
                }
            }
        }

        // Container expressions — recurse
        Expression::ArrayExpression(array) => {
            for element in &array.elements {
                match element {
                    ArrayExpressionElement::SpreadElement(spread) => {
                        collect_expr_ts_spans(&spread.argument, out);
                    }
                    ArrayExpressionElement::Elision(_) => {}
                    _ => {
                        if let Some(e) = element.as_expression() {
                            collect_expr_ts_spans(e, out);
                        }
                    }
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        if p.computed {
                            if let Some(e) = p.key.as_expression() {
                                collect_expr_ts_spans(e, out);
                            }
                        }
                        collect_expr_ts_spans(&p.value, out);
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        collect_expr_ts_spans(&spread.argument, out);
                    }
                }
            }
        }
        Expression::TemplateLiteral(t) => {
            for e in &t.expressions {
                collect_expr_ts_spans(e, out);
            }
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_expr_ts_spans(e, out);
            }
        }
        Expression::ParenthesizedExpression(p) => {
            collect_expr_ts_spans(&p.expression, out);
        }

        // Binary/unary/conditional — recurse
        Expression::AssignmentExpression(a) => {
            collect_assign_target_ts_spans(&a.left, out);
            collect_expr_ts_spans(&a.right, out);
        }
        Expression::UpdateExpression(update) => {
            collect_simple_assign_target_ts_spans(&update.argument, out);
        }
        Expression::BinaryExpression(b) => {
            collect_expr_ts_spans(&b.left, out);
            collect_expr_ts_spans(&b.right, out);
        }
        Expression::LogicalExpression(l) => {
            collect_expr_ts_spans(&l.left, out);
            collect_expr_ts_spans(&l.right, out);
        }
        Expression::UnaryExpression(u) => {
            collect_expr_ts_spans(&u.argument, out);
        }
        Expression::ConditionalExpression(c) => {
            collect_expr_ts_spans(&c.test, out);
            collect_expr_ts_spans(&c.consequent, out);
            collect_expr_ts_spans(&c.alternate, out);
        }

        // Member expressions
        Expression::StaticMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
        }
        Expression::ComputedMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
            collect_expr_ts_spans(&m.expression, out);
        }
        Expression::PrivateFieldExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
        }

        // Optional chaining
        Expression::ChainExpression(chain) => {
            collect_chain_ts_spans(&chain.expression, out);
        }

        // Await/yield
        Expression::AwaitExpression(a) => {
            collect_expr_ts_spans(&a.argument, out);
        }
        Expression::YieldExpression(y) => {
            if let Some(arg) = &y.argument {
                collect_expr_ts_spans(arg, out);
            }
        }

        // Identifiers, literals, this — nothing to collect
        _ => {}
    }
}

fn collect_chain_ts_spans(element: &ChainElement, out: &mut RemovalSpans) {
    match element {
        ChainElement::CallExpression(call) => {
            if let Some(ta) = &call.type_arguments {
                out.push((ta.span.start, ta.span.end));
            }
            collect_expr_ts_spans(&call.callee, out);
            collect_args_ts_spans(&call.arguments, out);
        }
        ChainElement::TSNonNullExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        ChainElement::StaticMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
        }
        ChainElement::ComputedMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
            collect_expr_ts_spans(&m.expression, out);
        }
        ChainElement::PrivateFieldExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
        }
    }
}

fn collect_args_ts_spans(args: &[Argument], out: &mut RemovalSpans) {
    for arg in args {
        match arg {
            Argument::SpreadElement(spread) => {
                collect_expr_ts_spans(&spread.argument, out);
            }
            _ => {
                if let Some(expr) = arg.as_expression() {
                    collect_expr_ts_spans(expr, out);
                }
            }
        }
    }
}

fn collect_assign_target_ts_spans(target: &AssignmentTarget, out: &mut RemovalSpans) {
    if let Some(target) = target.as_simple_assignment_target() {
        collect_simple_assign_target_ts_spans(target, out);
    }
}

fn collect_simple_assign_target_ts_spans(target: &SimpleAssignmentTarget, out: &mut RemovalSpans) {
    match target {
        SimpleAssignmentTarget::TSAsExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        SimpleAssignmentTarget::TSSatisfiesExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        SimpleAssignmentTarget::TSNonNullExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        SimpleAssignmentTarget::TSTypeAssertion(e) => {
            out.push((e.span.start, e.expression.span().start));
            collect_expr_ts_spans(&e.expression, out);
        }
        SimpleAssignmentTarget::ComputedMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
            collect_expr_ts_spans(&m.expression, out);
        }
        SimpleAssignmentTarget::StaticMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
        }
        SimpleAssignmentTarget::PrivateFieldExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
        }
        SimpleAssignmentTarget::AssignmentTargetIdentifier(_) => {}
    }
}

fn collect_pattern_ts_spans(pattern: &BindingPattern, out: &mut RemovalSpans) {
    match pattern {
        BindingPattern::BindingIdentifier(_) => {}
        BindingPattern::AssignmentPattern(pattern) => {
            collect_pattern_ts_spans(&pattern.left, out);
            collect_expr_ts_spans(&pattern.right, out);
        }
        BindingPattern::ObjectPattern(pattern) => {
            for property in &pattern.properties {
                if property.computed {
                    if let Some(key) = property.key.as_expression() {
                        collect_expr_ts_spans(key, out);
                    }
                }
                collect_pattern_ts_spans(&property.value, out);
            }
            if let Some(rest) = &pattern.rest {
                collect_pattern_ts_spans(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(pattern) => {
            for element in pattern.elements.iter().flatten() {
                collect_pattern_ts_spans(element, out);
            }
            if let Some(rest) = &pattern.rest {
                collect_pattern_ts_spans(&rest.argument, out);
            }
        }
    }
}

fn collect_formal_params_ts_spans(params: &FormalParameters, out: &mut RemovalSpans) {
    for param in &params.items {
        if let Some(annotation) = &param.type_annotation {
            out.annotation(annotation.span);
        }
        collect_pattern_ts_spans(&param.pattern, out);
        if let Some(initializer) = &param.initializer {
            collect_expr_ts_spans(initializer, out);
        }
    }
    if let Some(rest) = &params.rest {
        if let Some(annotation) = &rest.type_annotation {
            out.annotation(annotation.span);
        }
        collect_pattern_ts_spans(&rest.rest.argument, out);
    }
}

fn collect_variable_ts_spans(declaration: &VariableDeclaration, out: &mut RemovalSpans) {
    if declaration.declare {
        out.push((declaration.span.start, declaration.span.end));
        return;
    }
    for declarator in &declaration.declarations {
        if let Some(annotation) = &declarator.type_annotation {
            out.annotation(annotation.span);
        }
        collect_pattern_ts_spans(&declarator.id, out);
        if let Some(initializer) = &declarator.init {
            collect_expr_ts_spans(initializer, out);
        }
    }
}

fn collect_function_ts_spans(function: &Function, out: &mut RemovalSpans) {
    if function.body.is_none() {
        out.push((function.span.start, function.span.end));
        return;
    }
    if let Some(parameters) = &function.type_parameters {
        out.push((parameters.span.start, parameters.span.end));
    }
    if let Some(annotation) = &function.return_type {
        out.annotation(annotation.span);
    }
    collect_formal_params_ts_spans(&function.params, out);
    if let Some(body) = &function.body {
        for statement in &body.statements {
            collect_stmt_ts_spans(statement, out);
        }
    }
}

fn collect_for_left_ts_spans(left: &ForStatementLeft, out: &mut RemovalSpans) {
    if let ForStatementLeft::VariableDeclaration(declaration) = left {
        collect_variable_ts_spans(declaration, out);
    } else if let Some(target) = left.as_assignment_target() {
        collect_assign_target_ts_spans(target, out);
    }
}

fn collect_stmt_ts_spans(stmt: &Statement, out: &mut RemovalSpans) {
    match stmt {
        Statement::ExpressionStatement(statement) => {
            collect_expr_ts_spans(&statement.expression, out)
        }
        Statement::ReturnStatement(statement) => {
            if let Some(argument) = &statement.argument {
                collect_expr_ts_spans(argument, out);
            }
        }
        Statement::VariableDeclaration(declaration) => collect_variable_ts_spans(declaration, out),
        Statement::FunctionDeclaration(function) => collect_function_ts_spans(function, out),
        Statement::TSTypeAliasDeclaration(declaration) => {
            out.push((declaration.span.start, declaration.span.end))
        }
        Statement::TSInterfaceDeclaration(declaration) => {
            out.push((declaration.span.start, declaration.span.end))
        }
        Statement::TSExternalModuleDeclaration(declaration) => {
            out.push((declaration.span.start, declaration.span.end))
        }
        Statement::TSNamespaceDeclaration(declaration) if declaration.declare => {
            out.push((declaration.span.start, declaration.span.end))
        }
        Statement::BlockStatement(block) => {
            for statement in &block.body {
                collect_stmt_ts_spans(statement, out);
            }
        }
        Statement::IfStatement(statement) => {
            collect_expr_ts_spans(&statement.test, out);
            collect_stmt_ts_spans(&statement.consequent, out);
            if let Some(alternate) = &statement.alternate {
                collect_stmt_ts_spans(alternate, out);
            }
        }
        Statement::ForStatement(statement) => {
            if let Some(initializer) = &statement.init {
                if let ForStatementInit::VariableDeclaration(declaration) = initializer {
                    collect_variable_ts_spans(declaration, out);
                } else if let Some(expression) = initializer.as_expression() {
                    collect_expr_ts_spans(expression, out);
                }
            }
            if let Some(test) = &statement.test {
                collect_expr_ts_spans(test, out);
            }
            if let Some(update) = &statement.update {
                collect_expr_ts_spans(update, out);
            }
            collect_stmt_ts_spans(&statement.body, out);
        }
        Statement::ForInStatement(statement) => {
            collect_for_left_ts_spans(&statement.left, out);
            collect_expr_ts_spans(&statement.right, out);
            collect_stmt_ts_spans(&statement.body, out);
        }
        Statement::ForOfStatement(statement) => {
            collect_for_left_ts_spans(&statement.left, out);
            collect_expr_ts_spans(&statement.right, out);
            collect_stmt_ts_spans(&statement.body, out);
        }
        Statement::WhileStatement(statement) => {
            collect_expr_ts_spans(&statement.test, out);
            collect_stmt_ts_spans(&statement.body, out);
        }
        Statement::DoWhileStatement(statement) => {
            collect_stmt_ts_spans(&statement.body, out);
            collect_expr_ts_spans(&statement.test, out);
        }
        Statement::SwitchStatement(statement) => {
            collect_expr_ts_spans(&statement.discriminant, out);
            for case in &statement.cases {
                if let Some(test) = &case.test {
                    collect_expr_ts_spans(test, out);
                }
                for statement in &case.consequent {
                    collect_stmt_ts_spans(statement, out);
                }
            }
        }
        Statement::TryStatement(statement) => {
            for statement in &statement.block.body {
                collect_stmt_ts_spans(statement, out);
            }
            if let Some(handler) = &statement.handler {
                if let Some(parameter) = &handler.param {
                    if let Some(annotation) = &parameter.type_annotation {
                        out.annotation(annotation.span);
                    }
                    collect_pattern_ts_spans(&parameter.pattern, out);
                }
                for statement in &handler.body.body {
                    collect_stmt_ts_spans(statement, out);
                }
            }
            if let Some(finalizer) = &statement.finalizer {
                for statement in &finalizer.body {
                    collect_stmt_ts_spans(statement, out);
                }
            }
        }
        Statement::ThrowStatement(statement) => collect_expr_ts_spans(&statement.argument, out),
        Statement::LabeledStatement(statement) => collect_stmt_ts_spans(&statement.body, out),
        Statement::WithStatement(statement) => {
            collect_expr_ts_spans(&statement.object, out);
            collect_stmt_ts_spans(&statement.body, out);
        }
        _ => {}
    }
}

#[cfg(test)]
mod program_range_tests {
    use super::*;

    #[test]
    fn only_ambient_namespaces_are_type_only_removals() {
        for (source, ambient) in [
            ("declare namespace N { const x: number }", true),
            ("namespace N { export const x = 1 }", false),
        ] {
            let allocator = oxc_allocator::Allocator::default();
            let parsed = verter_parser::oxc_parse::Parser::new(
                &allocator,
                source,
                oxc_span::SourceType::ts(),
            )
            .parse();
            assert!(parsed.diagnostics.is_empty());
            let ranges = collect_program_ts_removal_spans(&parsed.program);
            assert_eq!(
                ranges,
                if ambient {
                    vec![(0, source.len() as u32)]
                } else {
                    Vec::new()
                }
            );
        }
    }
}
