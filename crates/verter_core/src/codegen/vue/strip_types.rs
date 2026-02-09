//! TypeScript type stripping for browser execution.
//!
//! Removes TypeScript-specific syntax from the AST while preserving
//! JavaScript runtime code. Used when `keep_ts: false` to produce
//! valid JavaScript output for the playground.

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use crate::code_transform::CodeTransform;

/// Strip TypeScript type annotations from a parsed program.
///
/// Walks the entire AST and removes TypeScript-specific constructs using
/// `CodeTransform::remove()`. Preserves source maps and integrates
/// with the existing compilation pipeline.
///
/// # Arguments
/// * `program` - The parsed oxc AST program (spans are 0-based relative to script content)
/// * `code_transform` - The code transform to apply removals to (uses SFC-absolute positions)
/// * `base_offset` - Offset to add to all spans to convert to SFC-absolute positions
/// * `script_source` - The raw script content source text (for reading source fragments)
pub fn strip_typescript_types<'a>(
    program: &Program,
    code_transform: &mut CodeTransform<'a>,
    base_offset: u32,
    script_source: &'a str,
) {
    let mut stripper = TypeStripper {
        code_transform,
        base_offset,
        source: script_source,
    };
    stripper.visit_program(program);
}

struct TypeStripper<'a, 'ct> {
    code_transform: &'ct mut CodeTransform<'a>,
    base_offset: u32,
    source: &'a str,
}

impl<'a, 'ct> TypeStripper<'a, 'ct> {
    #[inline]
    fn remove(&mut self, start: u32, end: u32) {
        self.code_transform
            .remove(start + self.base_offset, end + self.base_offset);
    }

    #[inline]
    fn overwrite(&mut self, start: u32, end: u32, content: &str) {
        self.code_transform
            .overwrite(start + self.base_offset, end + self.base_offset, content);
    }

    #[inline]
    fn source_text(&self, start: u32, end: u32) -> &str {
        &self.source[start as usize..end as usize]
    }

    /// Remove a type annotation, also removing preceding `?` or `!` if present.
    fn remove_type_annotation(&mut self, ta: &TSTypeAnnotation) {
        let mut start = ta.span.start;
        if start > 0 {
            let prev = self.source.as_bytes()[(start - 1) as usize];
            if prev == b'?' || prev == b'!' {
                start -= 1;
            }
        }
        self.remove(start, ta.span.end);
    }

    // =========================================================================
    // Program & Statement visitors
    // =========================================================================

    fn visit_program(&mut self, program: &Program) {
        for stmt in &program.body {
            self.visit_statement(stmt);
        }
    }

    fn visit_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::VariableDeclaration(var_decl) => {
                if var_decl.declare {
                    self.remove(var_decl.span.start, var_decl.span.end);
                    return;
                }
                self.visit_variable_declaration(var_decl);
            }
            Statement::ExpressionStatement(expr_stmt) => {
                self.visit_expression(&expr_stmt.expression);
            }
            Statement::ReturnStatement(ret) => {
                if let Some(arg) = &ret.argument {
                    self.visit_expression(arg);
                }
            }
            Statement::BlockStatement(block) => {
                self.visit_block(&block.body);
            }
            Statement::IfStatement(if_stmt) => {
                self.visit_expression(&if_stmt.test);
                self.visit_statement(&if_stmt.consequent);
                if let Some(alt) = &if_stmt.alternate {
                    self.visit_statement(alt);
                }
            }
            Statement::ForStatement(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    self.visit_for_statement_init(init);
                }
                if let Some(test) = &for_stmt.test {
                    self.visit_expression(test);
                }
                if let Some(update) = &for_stmt.update {
                    self.visit_expression(update);
                }
                self.visit_statement(&for_stmt.body);
            }
            Statement::ForInStatement(for_in) => {
                self.visit_for_statement_left(&for_in.left);
                self.visit_expression(&for_in.right);
                self.visit_statement(&for_in.body);
            }
            Statement::ForOfStatement(for_of) => {
                self.visit_for_statement_left(&for_of.left);
                self.visit_expression(&for_of.right);
                self.visit_statement(&for_of.body);
            }
            Statement::WhileStatement(w) => {
                self.visit_expression(&w.test);
                self.visit_statement(&w.body);
            }
            Statement::DoWhileStatement(d) => {
                self.visit_statement(&d.body);
                self.visit_expression(&d.test);
            }
            Statement::SwitchStatement(s) => {
                self.visit_expression(&s.discriminant);
                for case in &s.cases {
                    if let Some(test) = &case.test {
                        self.visit_expression(test);
                    }
                    self.visit_block(&case.consequent);
                }
            }
            Statement::TryStatement(t) => {
                self.visit_block(&t.block.body);
                if let Some(handler) = &t.handler {
                    if let Some(param) = &handler.param {
                        self.visit_binding_pattern(&param.pattern);
                    }
                    self.visit_block(&handler.body.body);
                }
                if let Some(finalizer) = &t.finalizer {
                    self.visit_block(&finalizer.body);
                }
            }
            Statement::ThrowStatement(t) => {
                self.visit_expression(&t.argument);
            }
            Statement::LabeledStatement(l) => {
                self.visit_statement(&l.body);
            }
            Statement::FunctionDeclaration(func) => {
                self.visit_function(func);
            }
            Statement::ClassDeclaration(class) => {
                self.visit_class(class);
            }
            // TypeScript declarations
            Statement::TSTypeAliasDeclaration(d) => {
                self.remove(d.span.start, d.span.end);
            }
            Statement::TSInterfaceDeclaration(d) => {
                self.remove(d.span.start, d.span.end);
            }
            Statement::TSModuleDeclaration(d) => {
                self.remove(d.span.start, d.span.end);
            }
            Statement::TSEnumDeclaration(d) => {
                self.convert_enum(d);
            }
            // Import/Export
            Statement::ImportDeclaration(import) => {
                self.visit_import_declaration(import);
            }
            Statement::ExportNamedDeclaration(export) => {
                self.visit_export_named(export);
            }
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                    self.visit_function(f);
                }
                ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                    self.visit_class(c);
                }
                ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => {
                    self.remove(export.span.start, export.span.end);
                }
                _ => {
                    if let Some(expr) = export.declaration.as_expression() {
                        self.visit_expression(expr);
                    }
                }
            },
            Statement::ExportAllDeclaration(export) => {
                if export.export_kind.is_type() {
                    self.remove(export.span.start, export.span.end);
                }
            }
            _ => {}
        }
    }

    fn visit_block(&mut self, stmts: &[Statement]) {
        for stmt in stmts {
            self.visit_statement(stmt);
        }
    }

    // =========================================================================
    // Declaration visitor
    // =========================================================================

    fn visit_declaration(&mut self, decl: &Declaration) {
        match decl {
            Declaration::VariableDeclaration(var_decl) => {
                if var_decl.declare {
                    return;
                }
                self.visit_variable_declaration(var_decl);
            }
            Declaration::FunctionDeclaration(func) => {
                self.visit_function(func);
            }
            Declaration::ClassDeclaration(class) => {
                self.visit_class(class);
            }
            Declaration::TSTypeAliasDeclaration(d) => {
                self.remove(d.span.start, d.span.end);
            }
            Declaration::TSInterfaceDeclaration(d) => {
                self.remove(d.span.start, d.span.end);
            }
            Declaration::TSModuleDeclaration(d) => {
                self.remove(d.span.start, d.span.end);
            }
            Declaration::TSEnumDeclaration(d) => {
                self.convert_enum(d);
            }
            _ => {}
        }
    }

    fn visit_variable_declaration(&mut self, var_decl: &VariableDeclaration) {
        for declarator in &var_decl.declarations {
            // Type annotations are on VariableDeclarator, not BindingPattern
            if let Some(ta) = &declarator.type_annotation {
                // Handle definite `!` before type annotation
                if declarator.definite {
                    self.remove_type_annotation(ta);
                } else {
                    self.remove(ta.span.start, ta.span.end);
                }
            } else if declarator.definite {
                // `let x!` with no type annotation — remove the `!`
                let id_end = declarator.id.span().end;
                if (id_end as usize) < self.source.len()
                    && self.source.as_bytes()[id_end as usize] == b'!'
                {
                    self.remove(id_end, id_end + 1);
                }
            }
            self.visit_binding_pattern(&declarator.id);
            if let Some(init) = &declarator.init {
                self.visit_expression(init);
            }
        }
    }

    // =========================================================================
    // Expression visitor
    // =========================================================================

    fn visit_expression(&mut self, expr: &Expression) {
        match expr {
            // TypeScript assertion expressions — strip type part, keep value
            Expression::TSAsExpression(e) => {
                self.remove(e.expression.span().end, e.span.end);
                self.visit_expression(&e.expression);
            }
            Expression::TSSatisfiesExpression(e) => {
                self.remove(e.expression.span().end, e.span.end);
                self.visit_expression(&e.expression);
            }
            Expression::TSNonNullExpression(e) => {
                self.remove(e.expression.span().end, e.span.end);
                self.visit_expression(&e.expression);
            }
            Expression::TSTypeAssertion(e) => {
                self.remove(e.span.start, e.expression.span().start);
                self.visit_expression(&e.expression);
            }
            Expression::TSInstantiationExpression(e) => {
                self.remove(e.expression.span().end, e.span.end);
                self.visit_expression(&e.expression);
            }

            // Expressions with type arguments
            Expression::CallExpression(call) => {
                if let Some(ta) = &call.type_arguments {
                    self.remove(ta.span.start, ta.span.end);
                }
                self.visit_expression(&call.callee);
                self.visit_arguments(&call.arguments);
            }
            Expression::NewExpression(new_expr) => {
                if let Some(ta) = &new_expr.type_arguments {
                    self.remove(ta.span.start, ta.span.end);
                }
                self.visit_expression(&new_expr.callee);
                self.visit_arguments(&new_expr.arguments);
            }
            Expression::TaggedTemplateExpression(tagged) => {
                if let Some(ta) = &tagged.type_arguments {
                    self.remove(ta.span.start, ta.span.end);
                }
                self.visit_expression(&tagged.tag);
                for expr in &tagged.quasi.expressions {
                    self.visit_expression(expr);
                }
            }

            // Function/class expressions
            Expression::ArrowFunctionExpression(arrow) => {
                if let Some(tp) = &arrow.type_parameters {
                    self.remove(tp.span.start, tp.span.end);
                }
                if let Some(rt) = &arrow.return_type {
                    self.remove(rt.span.start, rt.span.end);
                }
                self.visit_formal_parameters(&arrow.params);
                self.visit_block(&arrow.body.statements);
            }
            Expression::FunctionExpression(func) => {
                self.visit_function(func);
            }
            Expression::ClassExpression(class) => {
                self.visit_class(class);
            }

            // Container expressions
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    match element {
                        ArrayExpressionElement::SpreadElement(spread) => {
                            self.visit_expression(&spread.argument);
                        }
                        ArrayExpressionElement::Elision(_) => {}
                        _ => {
                            if let Some(e) = element.as_expression() {
                                self.visit_expression(e);
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
                                self.visit_property_key(&p.key);
                            }
                            self.visit_expression(&p.value);
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            self.visit_expression(&spread.argument);
                        }
                    }
                }
            }
            Expression::TemplateLiteral(t) => {
                for expr in &t.expressions {
                    self.visit_expression(expr);
                }
            }
            Expression::SequenceExpression(seq) => {
                for expr in &seq.expressions {
                    self.visit_expression(expr);
                }
            }
            Expression::ParenthesizedExpression(p) => {
                self.visit_expression(&p.expression);
            }

            // Binary/unary/conditional
            Expression::AssignmentExpression(a) => {
                self.visit_assignment_target(&a.left);
                self.visit_expression(&a.right);
            }
            Expression::BinaryExpression(b) => {
                self.visit_expression(&b.left);
                self.visit_expression(&b.right);
            }
            Expression::LogicalExpression(l) => {
                self.visit_expression(&l.left);
                self.visit_expression(&l.right);
            }
            Expression::UnaryExpression(u) => {
                self.visit_expression(&u.argument);
            }
            Expression::ConditionalExpression(c) => {
                self.visit_expression(&c.test);
                self.visit_expression(&c.consequent);
                self.visit_expression(&c.alternate);
            }

            // Member expressions (direct variants in oxc)
            Expression::StaticMemberExpression(m) => {
                self.visit_expression(&m.object);
            }
            Expression::ComputedMemberExpression(m) => {
                self.visit_expression(&m.object);
                self.visit_expression(&m.expression);
            }
            Expression::PrivateFieldExpression(m) => {
                self.visit_expression(&m.object);
            }

            // Await/yield
            Expression::AwaitExpression(a) => {
                self.visit_expression(&a.argument);
            }
            Expression::YieldExpression(y) => {
                if let Some(arg) = &y.argument {
                    self.visit_expression(arg);
                }
            }

            // Identifiers, literals, this, etc. — nothing to strip
            _ => {}
        }
    }

    fn visit_arguments(&mut self, args: &[Argument]) {
        for arg in args {
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
    }

    // =========================================================================
    // Assignment target visitor
    // =========================================================================

    fn visit_assignment_target(&mut self, target: &AssignmentTarget) {
        match target {
            AssignmentTarget::TSAsExpression(e) => {
                self.remove(e.expression.span().end, e.span.end);
                self.visit_expression(&e.expression);
            }
            AssignmentTarget::TSSatisfiesExpression(e) => {
                self.remove(e.expression.span().end, e.span.end);
                self.visit_expression(&e.expression);
            }
            AssignmentTarget::TSNonNullExpression(e) => {
                self.remove(e.expression.span().end, e.span.end);
                self.visit_expression(&e.expression);
            }
            AssignmentTarget::TSTypeAssertion(e) => {
                self.remove(e.span.start, e.expression.span().start);
                self.visit_expression(&e.expression);
            }
            AssignmentTarget::ComputedMemberExpression(m) => {
                self.visit_expression(&m.object);
                self.visit_expression(&m.expression);
            }
            AssignmentTarget::StaticMemberExpression(m) => {
                self.visit_expression(&m.object);
            }
            AssignmentTarget::PrivateFieldExpression(m) => {
                self.visit_expression(&m.object);
            }
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                for el in array.elements.iter().flatten() {
                    self.visit_assignment_target_maybe_default(el);
                }
                if let Some(rest) = &array.rest {
                    self.visit_assignment_target(&rest.target);
                }
            }
            AssignmentTarget::ObjectAssignmentTarget(obj) => {
                for prop in &obj.properties {
                    match prop {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(id) => {
                            if let Some(init) = &id.init {
                                self.visit_expression(init);
                            }
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                            self.visit_assignment_target_maybe_default(&p.binding);
                        }
                    }
                }
                if let Some(rest) = &obj.rest {
                    self.visit_assignment_target(&rest.target);
                }
            }
            _ => {}
        }
    }

    fn visit_assignment_target_maybe_default(&mut self, target: &AssignmentTargetMaybeDefault) {
        match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => {
                self.visit_assignment_target(&d.binding);
                self.visit_expression(&d.init);
            }
            _ => {
                if let Some(t) = target.as_assignment_target() {
                    self.visit_assignment_target(t);
                }
            }
        }
    }

    // =========================================================================
    // Binding pattern visitor
    // =========================================================================

    /// Visit a binding pattern recursively. In oxc 0.112, BindingPattern is an
    /// enum — type annotations live on parent nodes (VariableDeclarator,
    /// FormalParameter, etc.), not on the pattern itself.
    fn visit_binding_pattern(&mut self, pattern: &BindingPattern) {
        match pattern {
            BindingPattern::ObjectPattern(obj) => {
                for prop in &obj.properties {
                    self.visit_binding_pattern(&prop.value);
                }
                if let Some(rest) = &obj.rest {
                    self.visit_binding_pattern(&rest.argument);
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for el in array.elements.iter().flatten() {
                    self.visit_binding_pattern(el);
                }
                if let Some(rest) = &array.rest {
                    self.visit_binding_pattern(&rest.argument);
                }
            }
            BindingPattern::AssignmentPattern(assign) => {
                self.visit_binding_pattern(&assign.left);
                self.visit_expression(&assign.right);
            }
            BindingPattern::BindingIdentifier(_) => {}
        }
    }

    // =========================================================================
    // Function visitor
    // =========================================================================

    fn visit_function(&mut self, func: &Function) {
        if let Some(tp) = &func.type_parameters {
            self.remove(tp.span.start, tp.span.end);
        }
        if let Some(rt) = &func.return_type {
            self.remove(rt.span.start, rt.span.end);
        }
        self.visit_formal_parameters(&func.params);
        if let Some(body) = &func.body {
            self.visit_block(&body.statements);
        }
    }

    fn visit_formal_parameters(&mut self, params: &FormalParameters) {
        for param in &params.items {
            // Strip accessibility/readonly on constructor parameter properties
            if param.accessibility.is_some() || param.readonly {
                let param_start = param.span.start;
                let pattern_start = param.pattern.span().start;
                if param_start < pattern_start {
                    self.remove(param_start, pattern_start);
                }
            }
            // Type annotation is on FormalParameter, not BindingPattern
            if let Some(ta) = &param.type_annotation {
                self.remove_type_annotation(ta);
            }
            self.visit_binding_pattern(&param.pattern);
        }
        if let Some(rest) = &params.rest {
            if let Some(ta) = &rest.type_annotation {
                self.remove_type_annotation(ta);
            }
            self.visit_binding_pattern(&rest.rest.argument);
        }
    }

    // =========================================================================
    // Class visitor
    // =========================================================================

    fn visit_class(&mut self, class: &Class) {
        // Handle `abstract` keyword on class
        if class.r#abstract {
            self.strip_keyword_before(class.span.start, "abstract");
        }
        // Handle `declare` keyword on class
        if class.declare {
            self.remove(class.span.start, class.span.end);
            return;
        }

        if let Some(tp) = &class.type_parameters {
            self.remove(tp.span.start, tp.span.end);
        }
        if let Some(sta) = &class.super_type_arguments {
            self.remove(sta.span.start, sta.span.end);
        }

        // Strip `implements` clause
        if !class.implements.is_empty() {
            {
                let implements = &class.implements;
                let first = &implements[0];
                let last = &implements[implements.len() - 1];

                // Find where to search for `implements` keyword
                let search_start = if let Some(sta) = &class.super_type_arguments {
                    sta.span.end
                } else if let Some(sc) = &class.super_class {
                    sc.span().end
                } else if let Some(tp) = &class.type_parameters {
                    tp.span.end
                } else if let Some(id) = &class.id {
                    id.span.end
                } else {
                    class.span.start
                };

                let search_end = first.span.start;
                if search_start < search_end {
                    let search_text = self.source_text(search_start, search_end);
                    if let Some(pos) = search_text.find("implements") {
                        let impl_start = search_start + pos as u32;
                        self.remove(impl_start, last.span.end);
                    }
                }
            }
        }

        if let Some(sc) = &class.super_class {
            self.visit_expression(sc);
        }

        for element in &class.body.body {
            self.visit_class_element(element);
        }
    }

    fn visit_class_element(&mut self, element: &ClassElement) {
        match element {
            ClassElement::MethodDefinition(method) => {
                if method.r#type == MethodDefinitionType::TSAbstractMethodDefinition {
                    self.remove(method.span.start, method.span.end);
                    return;
                }
                self.strip_class_member_prefix(
                    method.span.start,
                    method.key.span().start,
                    method.accessibility.is_some(),
                    method.r#override,
                    false, // readonly not applicable to methods
                );
                if method.computed {
                    self.visit_property_key(&method.key);
                }
                self.visit_function(&method.value);
            }
            ClassElement::PropertyDefinition(prop) => {
                if prop.r#type == PropertyDefinitionType::TSAbstractPropertyDefinition {
                    self.remove(prop.span.start, prop.span.end);
                    return;
                }
                if prop.declare {
                    self.remove(prop.span.start, prop.span.end);
                    return;
                }
                if let Some(ta) = &prop.type_annotation {
                    let mut start = ta.span.start;
                    if prop.definite && start > 0 {
                        let prev = self.source.as_bytes()[(start - 1) as usize];
                        if prev == b'!' {
                            start -= 1;
                        }
                    }
                    self.remove(start, ta.span.end);
                }
                self.strip_class_member_prefix(
                    prop.span.start,
                    prop.key.span().start,
                    prop.accessibility.is_some(),
                    prop.r#override,
                    prop.readonly,
                );
                if prop.computed {
                    self.visit_property_key(&prop.key);
                }
                if let Some(value) = &prop.value {
                    self.visit_expression(value);
                }
            }
            ClassElement::StaticBlock(block) => {
                self.visit_block(&block.body);
            }
            ClassElement::AccessorProperty(accessor) => {
                if let Some(ta) = &accessor.type_annotation {
                    self.remove(ta.span.start, ta.span.end);
                }
                if let Some(value) = &accessor.value {
                    self.visit_expression(value);
                }
            }
            ClassElement::TSIndexSignature(_) => {
                self.remove(element.span().start, element.span().end);
            }
        }
    }

    /// Strip TS-only modifiers (accessibility, override, readonly) from class member prefix.
    /// Keeps `static`, `async`, `get`, `set` intact.
    fn strip_class_member_prefix(
        &mut self,
        member_start: u32,
        key_start: u32,
        has_accessibility: bool,
        has_override: bool,
        has_readonly: bool,
    ) {
        if !has_accessibility && !has_override && !has_readonly {
            return;
        }
        if member_start >= key_start {
            return;
        }

        let prefix = self.source_text(member_start, key_start);
        let mut result = prefix.to_string();
        for keyword in [
            "public",
            "private",
            "protected",
            "override",
            "readonly",
            "abstract",
        ] {
            result = result.replace(keyword, "");
        }

        let cleaned: String = result.split_whitespace().collect::<Vec<_>>().join(" ");
        let new_prefix = if cleaned.is_empty() {
            String::new()
        } else {
            format!("{cleaned} ")
        };

        if new_prefix != prefix {
            self.overwrite(member_start, key_start, &new_prefix);
        }
    }

    /// Strip a keyword (like "abstract") that appears at or after `start` in the source.
    fn strip_keyword_before(&mut self, start: u32, keyword: &str) {
        let end = (start + keyword.len() as u32 + 20).min(self.source.len() as u32);
        let text = self.source_text(start, end);
        if let Some(pos) = text.find(keyword) {
            let kw_start = start + pos as u32;
            let kw_end = kw_start + keyword.len() as u32;
            // Also remove trailing space
            let actual_end = if (kw_end as usize) < self.source.len()
                && self.source.as_bytes()[kw_end as usize] == b' '
            {
                kw_end + 1
            } else {
                kw_end
            };
            self.remove(kw_start, actual_end);
        }
    }

    // =========================================================================
    // Import/Export visitors
    // =========================================================================

    fn visit_import_declaration(&mut self, import: &ImportDeclaration) {
        if import.import_kind.is_type() {
            self.remove(import.span.start, import.span.end);
            return;
        }

        if let Some(specifiers) = &import.specifiers {
            self.strip_type_specifiers_import(import, specifiers);
        }
    }

    fn strip_type_specifiers_import(
        &mut self,
        import: &ImportDeclaration,
        specifiers: &[ImportDeclarationSpecifier],
    ) {
        let type_indices: Vec<usize> = specifiers
            .iter()
            .enumerate()
            .filter_map(|(i, spec)| {
                if let ImportDeclarationSpecifier::ImportSpecifier(s) = spec {
                    if s.import_kind.is_type() {
                        return Some(i);
                    }
                }
                None
            })
            .collect();

        if type_indices.is_empty() {
            return;
        }

        if type_indices.len() == specifiers.len() {
            self.remove(import.span.start, import.span.end);
            return;
        }

        for &idx in type_indices.iter().rev() {
            let spec_span = specifiers[idx].span();
            if idx + 1 < specifiers.len() {
                let next_span = specifiers[idx + 1].span();
                self.remove(spec_span.start, next_span.start);
            } else if idx > 0 {
                let prev_span = specifiers[idx - 1].span();
                self.remove(prev_span.end, spec_span.end);
            }
        }
    }

    fn visit_export_named(&mut self, export: &ExportNamedDeclaration) {
        if export.export_kind.is_type() {
            self.remove(export.span.start, export.span.end);
            return;
        }

        if !export.specifiers.is_empty() {
            let type_indices: Vec<usize> = export
                .specifiers
                .iter()
                .enumerate()
                .filter_map(|(i, spec)| {
                    if spec.export_kind.is_type() {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect();

            if !type_indices.is_empty() {
                if type_indices.len() == export.specifiers.len() {
                    self.remove(export.span.start, export.span.end);
                    return;
                }
                for &idx in type_indices.iter().rev() {
                    let spec_span = export.specifiers[idx].span();
                    if idx + 1 < export.specifiers.len() {
                        let next_span = export.specifiers[idx + 1].span();
                        self.remove(spec_span.start, next_span.start);
                    } else if idx > 0 {
                        let prev_span = export.specifiers[idx - 1].span();
                        self.remove(prev_span.end, spec_span.end);
                    }
                }
            }
        }

        if let Some(decl) = &export.declaration {
            self.visit_declaration(decl);
        }
    }

    // =========================================================================
    // For-statement helpers
    // =========================================================================

    fn visit_for_statement_init(&mut self, init: &ForStatementInit) {
        match init {
            ForStatementInit::VariableDeclaration(var_decl) => {
                self.visit_variable_declaration(var_decl);
            }
            _ => {
                if let Some(expr) = init.as_expression() {
                    self.visit_expression(expr);
                }
            }
        }
    }

    fn visit_for_statement_left(&mut self, left: &ForStatementLeft) {
        match left {
            ForStatementLeft::VariableDeclaration(var_decl) => {
                self.visit_variable_declaration(var_decl);
            }
            _ => {
                if let Some(target) = left.as_assignment_target() {
                    self.visit_assignment_target(target);
                }
            }
        }
    }

    // =========================================================================
    // Property key visitor
    // =========================================================================

    fn visit_property_key(&mut self, key: &PropertyKey) {
        if let Some(expr) = key.as_expression() {
            self.visit_expression(expr);
        }
    }

    // =========================================================================
    // Enum conversion
    // =========================================================================

    /// Convert a TypeScript enum to a JavaScript IIFE pattern.
    fn convert_enum(&mut self, ts_enum: &TSEnumDeclaration) {
        let name = ts_enum.id.name.as_str();
        let mut js = format!("var {name}; (function({name}) {{\n");

        let mut next_value: i64 = 0;

        for member in &ts_enum.body.members {
            let member_name = match &member.id {
                TSEnumMemberName::Identifier(id) => id.name.to_string(),
                TSEnumMemberName::String(s) => s.value.to_string(),
                _ => continue,
            };

            if let Some(initializer) = &member.initializer {
                if let Expression::StringLiteral(s) = initializer {
                    js.push_str(&format!(
                        "  {name}[\"{}\"] = \"{}\";\n",
                        escape_js_string(&member_name),
                        escape_js_string(s.value.as_str())
                    ));
                    continue;
                }

                if let Expression::NumericLiteral(n) = initializer {
                    next_value = n.value as i64;
                    js.push_str(&format!(
                        "  {name}[{name}[\"{}\"] = {next_value}] = \"{}\";\n",
                        escape_js_string(&member_name),
                        escape_js_string(&member_name),
                    ));
                    next_value += 1;
                    continue;
                }

                // Computed initializer — use source text
                let init_text = self.source_text(initializer.span().start, initializer.span().end);
                js.push_str(&format!(
                    "  {name}[{name}[\"{}\"] = {init_text}] = \"{}\";\n",
                    escape_js_string(&member_name),
                    escape_js_string(&member_name),
                ));
                next_value = 0;
            } else {
                js.push_str(&format!(
                    "  {name}[{name}[\"{}\"] = {next_value}] = \"{}\";\n",
                    escape_js_string(&member_name),
                    escape_js_string(&member_name),
                ));
                next_value += 1;
            }
        }

        js.push_str(&format!("}})({name} || ({name} = {{}}))"));

        self.overwrite(ts_enum.span.start, ts_enum.span.end, &js);
    }
}

fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
