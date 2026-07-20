//! The exhaustive canonicalizer — extracted from `canon.rs` (see `mod.rs`).

use std::collections::{BTreeSet, HashMap};

use oxc_allocator::Box as OxcBox;
use oxc_ast::ast::*;
use oxc_semantic::{AstNodes, NodeId, Scoping};

use super::classify::Classifier;
use super::{Canon, ImportEntry};

// ---------------------------------------------------------------------------
// The exhaustive canonicalizer.
// ---------------------------------------------------------------------------

/// Explicitly refused AST territory (TS/JSX/intrinsics) — the inputs are
/// plain JS modules; hitting one of these is a comparator bug or a contract
/// violation, never something to silently skip.
pub(crate) fn refused(group: &str, variant: &str) -> ! {
    panic!("refused AST variant {group}::{variant} (out of contract: TS/JSX/intrinsic)")
}

pub(crate) fn kind_name_of_declaration(declaration: &Declaration) -> &'static str {
    match declaration {
        Declaration::VariableDeclaration(_) => "VariableDeclaration",
        Declaration::FunctionDeclaration(_) => "FunctionDeclaration",
        Declaration::ClassDeclaration(_) => "ClassDeclaration",
        Declaration::TSTypeAliasDeclaration(_) => "TSTypeAliasDeclaration",
        Declaration::TSInterfaceDeclaration(_) => "TSInterfaceDeclaration",
        Declaration::TSEnumDeclaration(_) => "TSEnumDeclaration",
        Declaration::TSModuleDeclaration(_) => "TSModuleDeclaration",
        Declaration::TSGlobalDeclaration(_) => "TSGlobalDeclaration",
        Declaration::TSImportEqualsDeclaration(_) => "TSImportEqualsDeclaration",
    }
}

pub(crate) struct Canonizer<'a, 'b> {
    pub(crate) scoping: &'b Scoping,
    #[allow(dead_code)]
    pub(crate) nodes: &'b AstNodes<'a>,
    pub(crate) classifier: &'b Classifier,
    pub(crate) comments: std::cell::RefCell<HashMap<NodeId, Vec<Canon>>>,
    pub(crate) specifier_import_sources_seen: std::cell::RefCell<BTreeSet<String>>,
}

impl<'a, 'b> Canonizer<'a, 'b> {
    fn wrap(&self, kind: &'static str, mut children: Vec<Canon>, node_id: NodeId) -> Canon {
        if let Some(comments) = self.comments.borrow_mut().remove(&node_id) {
            children.push(Canon::node("comments", comments));
        }
        Canon::node(kind, children)
    }

    pub(crate) fn canon_program(&self, program: &Program) -> Canon {
        let mut children = Vec::new();
        for directive in &program.directives {
            children.push(self.canon_directive(directive));
        }
        self.canon_statement_list(&program.body, &mut children);
        self.wrap("Program", children, program.node_id.get())
    }

    fn canon_directive(&self, directive: &Directive) -> Canon {
        Canon::node(
            "directive",
            vec![Canon::leaf("str", directive.directive.as_str())],
        )
    }

    fn canon_statement_list(&self, statements: &[Statement], out: &mut Vec<Canon>) {
        for statement in statements {
            // Empty statements are no-op semicolon trivia — waived.
            if matches!(statement, Statement::EmptyStatement(_)) {
                continue;
            }
            // Import declarations appear in the tree as source markers
            // (specifiers are compared via the imports dimension). Repeated
            // specifier imports from an already-seen source are dropped:
            // declaration GROUPING is cosmetic (hoisted ESM), while the
            // side-effect import SEQUENCE is contract.
            if let Statement::ImportDeclaration(import) = statement {
                if import.specifiers.is_some()
                    && !self
                        .specifier_import_sources_seen
                        .borrow_mut()
                        .insert(import.source.value.to_string())
                {
                    continue;
                }
            }
            out.push(self.canon_statement(statement));
        }
    }

    fn canon_statement(&self, statement: &Statement) -> Canon {
        match statement {
            Statement::BlockStatement(block) => {
                let mut children = Vec::new();
                self.canon_statement_list(&block.body, &mut children);
                self.wrap("BlockStatement", children, block.node_id.get())
            }
            Statement::BreakStatement(break_stmt) => self.wrap(
                "BreakStatement",
                vec![self.canon_label(&break_stmt.label)],
                break_stmt.node_id.get(),
            ),
            Statement::ContinueStatement(continue_stmt) => self.wrap(
                "ContinueStatement",
                vec![self.canon_label(&continue_stmt.label)],
                continue_stmt.node_id.get(),
            ),
            Statement::DebuggerStatement(debugger) => {
                self.wrap("DebuggerStatement", vec![], debugger.node_id.get())
            }
            Statement::DoWhileStatement(do_while) => self.wrap(
                "DoWhileStatement",
                vec![
                    self.canon_statement(&do_while.body),
                    self.canon_expression(&do_while.test),
                ],
                do_while.node_id.get(),
            ),
            Statement::ExpressionStatement(expression) => self.wrap(
                "ExpressionStatement",
                vec![self.canon_expression(&expression.expression)],
                expression.node_id.get(),
            ),
            Statement::ForInStatement(for_in) => self.wrap(
                "ForInStatement",
                vec![
                    self.canon_for_left(&for_in.left),
                    self.canon_expression(&for_in.right),
                    self.canon_statement(&for_in.body),
                ],
                for_in.node_id.get(),
            ),
            Statement::ForOfStatement(for_of) => self.wrap(
                "ForOfStatement",
                vec![
                    Canon::leaf("flag", for_of.r#await.to_string()),
                    self.canon_for_left(&for_of.left),
                    self.canon_expression(&for_of.right),
                    self.canon_statement(&for_of.body),
                ],
                for_of.node_id.get(),
            ),
            Statement::ForStatement(for_stmt) => self.wrap(
                "ForStatement",
                vec![
                    match &for_stmt.init {
                        Some(ForStatementInit::VariableDeclaration(variable)) => {
                            self.canon_variable_declaration(variable)
                        }
                        Some(init) => self.canon_expression(init.to_expression()),
                        None => Canon::none(),
                    },
                    self.canon_opt_expression(&for_stmt.test),
                    self.canon_opt_expression(&for_stmt.update),
                    self.canon_statement(&for_stmt.body),
                ],
                for_stmt.node_id.get(),
            ),
            Statement::IfStatement(if_stmt) => self.wrap(
                "IfStatement",
                vec![
                    self.canon_expression(&if_stmt.test),
                    self.canon_statement(&if_stmt.consequent),
                    match &if_stmt.alternate {
                        Some(alternate) => self.canon_statement(alternate),
                        None => Canon::none(),
                    },
                ],
                if_stmt.node_id.get(),
            ),
            Statement::LabeledStatement(labeled) => self.wrap(
                "LabeledStatement",
                vec![
                    Canon::leaf("ident", labeled.label.name.as_str()),
                    self.canon_statement(&labeled.body),
                ],
                labeled.node_id.get(),
            ),
            Statement::ReturnStatement(return_stmt) => self.wrap(
                "ReturnStatement",
                vec![self.canon_opt_expression(&return_stmt.argument)],
                return_stmt.node_id.get(),
            ),
            Statement::SwitchStatement(switch) => {
                let mut children = vec![self.canon_expression(&switch.discriminant)];
                for case in &switch.cases {
                    let mut case_children = vec![self.canon_opt_expression(&case.test)];
                    self.canon_statement_list(&case.consequent, &mut case_children);
                    children.push(Canon::node("SwitchCase", case_children));
                }
                self.wrap("SwitchStatement", children, switch.node_id.get())
            }
            Statement::ThrowStatement(throw) => self.wrap(
                "ThrowStatement",
                vec![self.canon_expression(&throw.argument)],
                throw.node_id.get(),
            ),
            Statement::TryStatement(try_stmt) => {
                let handler = match &try_stmt.handler {
                    Some(catch) => {
                        let param = match &catch.param {
                            Some(param) => self.canon_pattern(&param.pattern),
                            None => Canon::none(),
                        };
                        Canon::node("CatchClause", vec![param, self.canon_block(&catch.body)])
                    }
                    None => Canon::none(),
                };
                let finalizer = match &try_stmt.finalizer {
                    Some(finalizer) => self.canon_block(finalizer),
                    None => Canon::none(),
                };
                self.wrap(
                    "TryStatement",
                    vec![self.canon_block(&try_stmt.block), handler, finalizer],
                    try_stmt.node_id.get(),
                )
            }
            Statement::WhileStatement(while_stmt) => self.wrap(
                "WhileStatement",
                vec![
                    self.canon_expression(&while_stmt.test),
                    self.canon_statement(&while_stmt.body),
                ],
                while_stmt.node_id.get(),
            ),
            Statement::WithStatement(with) => self.wrap(
                "WithStatement",
                vec![
                    self.canon_expression(&with.object),
                    self.canon_statement(&with.body),
                ],
                with.node_id.get(),
            ),
            Statement::EmptyStatement(_) => unreachable!("filtered in statement lists"),
            Statement::VariableDeclaration(variable) => self.canon_variable_declaration(variable),
            Statement::FunctionDeclaration(function) => {
                self.canon_function(function, "FunctionDeclaration")
            }
            Statement::ClassDeclaration(class) => self.canon_class(class, "ClassDeclaration"),
            Statement::ImportDeclaration(import) => {
                let kind = if import.specifiers.is_none() {
                    "import-side-effect"
                } else {
                    "import"
                };
                Canon::node(kind, vec![Canon::leaf("str", import.source.value.as_str())])
            }
            Statement::ExportNamedDeclaration(export) => self.canon_export_named(export),
            Statement::ExportDefaultDeclaration(export) => {
                let declaration = match &export.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                        self.canon_function(function, "FunctionDeclaration")
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                        self.canon_class(class, "ClassDeclaration")
                    }
                    expression => self.canon_expression(expression.to_expression()),
                };
                self.wrap(
                    "ExportDefaultDeclaration",
                    vec![declaration],
                    export.node_id.get(),
                )
            }
            Statement::ExportAllDeclaration(export_all) => {
                let exported = match &export_all.exported {
                    Some(name) => Canon::leaf("ident", module_export_name(name)),
                    None => Canon::none(),
                };
                self.wrap(
                    "ExportAllDeclaration",
                    vec![
                        exported,
                        Canon::leaf("str", export_all.source.value.as_str()),
                        self.canon_with_clause(&export_all.with_clause),
                    ],
                    export_all.node_id.get(),
                )
            }
            Statement::TSTypeAliasDeclaration(_)
            | Statement::TSInterfaceDeclaration(_)
            | Statement::TSEnumDeclaration(_)
            | Statement::TSModuleDeclaration(_)
            | Statement::TSGlobalDeclaration(_)
            | Statement::TSImportEqualsDeclaration(_)
            | Statement::TSExportAssignment(_)
            | Statement::TSNamespaceExportDeclaration(_) => refused("Statement", "TS-*"),
        }
    }

    fn canon_block(&self, block: &BlockStatement) -> Canon {
        let mut children = Vec::new();
        self.canon_statement_list(&block.body, &mut children);
        self.wrap("BlockStatement", children, block.node_id.get())
    }

    fn canon_declaration(&self, declaration: &Declaration) -> Canon {
        match declaration {
            Declaration::VariableDeclaration(variable) => self.canon_variable_declaration(variable),
            Declaration::FunctionDeclaration(function) => {
                self.canon_function(function, "FunctionDeclaration")
            }
            Declaration::ClassDeclaration(class) => self.canon_class(class, "ClassDeclaration"),
            other => refused("Declaration", kind_name_of_declaration(other)),
        }
    }

    fn canon_variable_declaration(&self, variable: &VariableDeclaration) -> Canon {
        let kind = match variable.kind {
            VariableDeclarationKind::Var => "var",
            VariableDeclarationKind::Let => "let",
            VariableDeclarationKind::Const => "const",
            VariableDeclarationKind::Using => "using",
            VariableDeclarationKind::AwaitUsing => "await using",
        };
        let mut children = vec![Canon::leaf("op", kind)];
        for declarator in &variable.declarations {
            let pattern = self.canon_pattern(&declarator.id);
            let init = self.canon_opt_expression(&declarator.init);
            let node = self.wrap(
                "VariableDeclarator",
                vec![pattern, init],
                declarator.node_id.get(),
            );
            children.push(node);
        }
        self.wrap("VariableDeclaration", children, variable.node_id.get())
    }

    fn canon_export_named(&self, export: &ExportNamedDeclaration) -> Canon {
        let declaration = match &export.declaration {
            Some(declaration) => self.canon_declaration(declaration),
            None => Canon::none(),
        };
        // Export-specifier order is not semantic — sort by exported name.
        let mut specifiers: Vec<Canon> = export
            .specifiers
            .iter()
            .map(|specifier| {
                let local = match &specifier.local {
                    ModuleExportName::IdentifierName(name) => {
                        Canon::leaf("ident", name.name.as_str())
                    }
                    ModuleExportName::IdentifierReference(reference) => {
                        self.classifier.classify_reference(self.scoping, reference)
                    }
                    ModuleExportName::StringLiteral(literal) => {
                        Canon::leaf("str", literal.value.as_str())
                    }
                };
                Canon::node(
                    "ExportSpecifier",
                    vec![
                        local,
                        Canon::leaf("ident", module_export_name(&specifier.exported)),
                    ],
                )
            })
            .collect();
        specifiers.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        let source = match &export.source {
            Some(source) => Canon::leaf("str", source.value.as_str()),
            None => Canon::none(),
        };
        let mut children = vec![declaration];
        children.extend(specifiers);
        children.push(source);
        children.push(self.canon_with_clause(&export.with_clause));
        self.wrap("ExportNamedDeclaration", children, export.node_id.get())
    }

    fn canon_with_clause(&self, with_clause: &Option<OxcBox<'a, WithClause<'a>>>) -> Canon {
        match with_clause {
            Some(clause) => {
                let mut entries: Vec<(String, String)> = clause
                    .with_entries
                    .iter()
                    .map(|entry| {
                        let key = match &entry.key {
                            ImportAttributeKey::Identifier(name) => name.name.to_string(),
                            ImportAttributeKey::StringLiteral(literal) => literal.value.to_string(),
                        };
                        (key, entry.value.value.to_string())
                    })
                    .collect();
                entries.sort();
                Canon::node(
                    "attributes",
                    entries
                        .into_iter()
                        .map(|(key, value)| {
                            Canon::node(
                                "attr",
                                vec![Canon::leaf("ident", key), Canon::leaf("str", value)],
                            )
                        })
                        .collect(),
                )
            }
            None => Canon::none(),
        }
    }

    fn canon_for_left(&self, left: &ForStatementLeft) -> Canon {
        match left {
            ForStatementLeft::VariableDeclaration(variable) => {
                self.canon_variable_declaration(variable)
            }
            left => self.canon_assignment_target(left.to_assignment_target()),
        }
    }

    fn canon_label(&self, label: &Option<LabelIdentifier>) -> Canon {
        match label {
            Some(label) => Canon::leaf("ident", label.name.as_str()),
            None => Canon::none(),
        }
    }

    fn canon_opt_expression(&self, expression: &Option<Expression>) -> Canon {
        match expression {
            Some(expression) => self.canon_expression(expression),
            None => Canon::none(),
        }
    }

    // -- Patterns ------------------------------------------------------------

    fn canon_pattern(&self, pattern: &BindingPattern) -> Canon {
        match pattern {
            BindingPattern::BindingIdentifier(ident) => self.classifier.classify_binding(ident),
            BindingPattern::ObjectPattern(object) => {
                let mut children = Vec::new();
                for property in &object.properties {
                    let key = self.canon_property_key(&property.key);
                    let value = self.canon_pattern(&property.value);
                    children.push(Canon::node(
                        "BindingProperty",
                        vec![
                            key,
                            value,
                            Canon::leaf("flag", property.shorthand.to_string()),
                            Canon::leaf("flag", property.computed.to_string()),
                        ],
                    ));
                }
                let rest = match &object.rest {
                    Some(rest) => Canon::node("rest", vec![self.canon_pattern(&rest.argument)]),
                    None => Canon::none(),
                };
                children.push(rest);
                Canon::node("ObjectPattern", children)
            }
            BindingPattern::ArrayPattern(array) => {
                let mut children = Vec::new();
                for element in &array.elements {
                    match element {
                        Some(pattern) => children.push(self.canon_pattern(pattern)),
                        None => children.push(Canon::node("hole", vec![])),
                    }
                }
                let rest = match &array.rest {
                    Some(rest) => Canon::node("rest", vec![self.canon_pattern(&rest.argument)]),
                    None => Canon::none(),
                };
                children.push(rest);
                Canon::node("ArrayPattern", children)
            }
            BindingPattern::AssignmentPattern(assignment) => Canon::node(
                "AssignmentPattern",
                vec![
                    self.canon_pattern(&assignment.left),
                    self.canon_expression(&assignment.right),
                ],
            ),
        }
    }

    fn canon_property_key(&self, key: &PropertyKey) -> Canon {
        match key {
            PropertyKey::StaticIdentifier(name) => Canon::leaf("ident", name.name.as_str()),
            PropertyKey::PrivateIdentifier(private) => Canon::leaf("ident", private.name.as_str()),
            key => self.canon_expression(key.to_expression()),
        }
    }

    // -- Functions/classes -----------------------------------------------------

    fn canon_function(&self, function: &Function, kind: &'static str) -> Canon {
        let id = match &function.id {
            Some(id) => self.classifier.classify_binding(id),
            None => Canon::none(),
        };
        let params = self.canon_formal_parameters(&function.params);
        let body = match &function.body {
            Some(body) => {
                let mut children = Vec::new();
                for directive in &body.directives {
                    children.push(self.canon_directive(directive));
                }
                self.canon_statement_list(&body.statements, &mut children);
                Canon::node("FunctionBody", children)
            }
            None => Canon::none(),
        };
        let mut flags = String::new();
        if function.r#async {
            flags.push('a');
        }
        if function.generator {
            flags.push('g');
        }
        if function.pure {
            flags.push('p');
        }
        self.wrap(
            kind,
            vec![id, params, body, Canon::leaf("flag", flags)],
            function.node_id.get(),
        )
    }

    fn canon_formal_parameters(&self, params: &FormalParameters) -> Canon {
        let mut children = Vec::new();
        for parameter in &params.items {
            if !parameter.decorators.is_empty() {
                refused("FormalParameter", "decorators");
            }
            let initializer = match &parameter.initializer {
                Some(init) => self.canon_expression(init),
                None => Canon::none(),
            };
            children.push(Canon::node(
                "param",
                vec![self.canon_pattern(&parameter.pattern), initializer],
            ));
        }
        let rest = match &params.rest {
            Some(rest) => Canon::node("rest", vec![self.canon_pattern(&rest.rest.argument)]),
            None => Canon::none(),
        };
        children.push(rest);
        Canon::node("params", children)
    }

    fn canon_arrow(&self, arrow: &ArrowFunctionExpression) -> Canon {
        let params = self.canon_formal_parameters(&arrow.params);
        let mut body_children = Vec::new();
        for directive in &arrow.body.directives {
            body_children.push(self.canon_directive(directive));
        }
        self.canon_statement_list(&arrow.body.statements, &mut body_children);
        let mut flags = String::new();
        if arrow.r#async {
            flags.push('a');
        }
        if arrow.expression {
            flags.push('e');
        }
        if arrow.pure {
            flags.push('p');
        }
        self.wrap(
            "ArrowFunctionExpression",
            vec![
                params,
                Canon::node("FunctionBody", body_children),
                Canon::leaf("flag", flags),
            ],
            arrow.node_id.get(),
        )
    }

    fn canon_class(&self, class: &Class, kind: &'static str) -> Canon {
        if !class.decorators.is_empty() {
            refused("Class", "decorators");
        }
        let id = match &class.id {
            Some(id) => self.classifier.classify_binding(id),
            None => Canon::none(),
        };
        let super_class = match &class.super_class {
            Some(super_class) => self.canon_expression(super_class),
            None => Canon::none(),
        };
        let mut elements = Vec::new();
        for element in &class.body.body {
            elements.push(self.canon_class_element(element));
        }
        self.wrap(
            kind,
            vec![id, super_class, Canon::node("ClassBody", elements)],
            class.node_id.get(),
        )
    }

    fn canon_class_element(&self, element: &ClassElement) -> Canon {
        match element {
            ClassElement::StaticBlock(block) => {
                let mut children = Vec::new();
                self.canon_statement_list(&block.body, &mut children);
                Canon::node("StaticBlock", children)
            }
            ClassElement::MethodDefinition(method) => {
                if !method.decorators.is_empty() {
                    refused("MethodDefinition", "decorators");
                }
                let kind = match method.kind {
                    MethodDefinitionKind::Constructor => "constructor",
                    MethodDefinitionKind::Method => "method",
                    MethodDefinitionKind::Get => "get",
                    MethodDefinitionKind::Set => "set",
                };
                Canon::node(
                    "MethodDefinition",
                    vec![
                        self.canon_property_key(&method.key),
                        self.canon_function(&method.value, "FunctionExpression"),
                        Canon::leaf("op", kind),
                        Canon::leaf("flag", method.computed.to_string()),
                        Canon::leaf("flag", method.r#static.to_string()),
                    ],
                )
            }
            ClassElement::PropertyDefinition(property) => {
                if !property.decorators.is_empty() {
                    refused("PropertyDefinition", "decorators");
                }
                Canon::node(
                    "PropertyDefinition",
                    vec![
                        self.canon_property_key(&property.key),
                        self.canon_opt_expression(&property.value),
                        Canon::leaf("flag", property.computed.to_string()),
                        Canon::leaf("flag", property.r#static.to_string()),
                    ],
                )
            }
            ClassElement::AccessorProperty(property) => {
                if !property.decorators.is_empty() {
                    refused("AccessorProperty", "decorators");
                }
                Canon::node(
                    "AccessorProperty",
                    vec![
                        self.canon_property_key(&property.key),
                        self.canon_opt_expression(&property.value),
                        Canon::leaf("flag", property.computed.to_string()),
                        Canon::leaf("flag", property.r#static.to_string()),
                    ],
                )
            }
            ClassElement::TSIndexSignature(_) => refused("ClassElement", "TSIndexSignature"),
        }
    }

    // -- Assignment targets ----------------------------------------------------

    fn canon_assignment_target(&self, target: &AssignmentTarget) -> Canon {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(ident) => {
                self.classifier.classify_reference(self.scoping, ident)
            }
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.canon_computed_member(member)
            }
            AssignmentTarget::StaticMemberExpression(member) => self.canon_static_member(member),
            AssignmentTarget::PrivateFieldExpression(member) => {
                self.canon_private_field_member(member)
            }
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                let mut children = Vec::new();
                for element in &array.elements {
                    match element {
                        Some(element) => {
                            children.push(self.canon_assignment_target_maybe_default(element))
                        }
                        None => children.push(Canon::node("hole", vec![])),
                    }
                }
                let rest = match &array.rest {
                    Some(rest) => {
                        Canon::node("rest", vec![self.canon_assignment_target(&rest.target)])
                    }
                    None => Canon::none(),
                };
                children.push(rest);
                Canon::node("ArrayAssignmentTarget", children)
            }
            AssignmentTarget::ObjectAssignmentTarget(object) => {
                let mut children = Vec::new();
                for property in &object.properties {
                    children.push(self.canon_assignment_target_property(property));
                }
                let rest = match &object.rest {
                    Some(rest) => {
                        Canon::node("rest", vec![self.canon_assignment_target(&rest.target)])
                    }
                    None => Canon::none(),
                };
                children.push(rest);
                Canon::node("ObjectAssignmentTarget", children)
            }
            AssignmentTarget::TSAsExpression(_)
            | AssignmentTarget::TSSatisfiesExpression(_)
            | AssignmentTarget::TSNonNullExpression(_)
            | AssignmentTarget::TSTypeAssertion(_) => refused("AssignmentTarget", "TS-*"),
        }
    }

    fn canon_assignment_target_maybe_default(
        &self,
        target: &AssignmentTargetMaybeDefault,
    ) -> Canon {
        match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => Canon::node(
                "AssignmentTargetWithDefault",
                vec![
                    self.canon_assignment_target(&with_default.binding),
                    self.canon_expression(&with_default.init),
                ],
            ),
            target => self.canon_assignment_target(target.to_assignment_target()),
        }
    }

    fn canon_assignment_target_property(&self, property: &AssignmentTargetProperty) -> Canon {
        match property {
            AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(identifier) => {
                let init = match &identifier.init {
                    Some(init) => self.canon_expression(init),
                    None => Canon::none(),
                };
                Canon::node(
                    "AssignmentTargetPropertyIdentifier",
                    vec![
                        self.classifier
                            .classify_reference(self.scoping, &identifier.binding),
                        init,
                    ],
                )
            }
            AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => Canon::node(
                "AssignmentTargetPropertyProperty",
                vec![
                    self.canon_property_key(&property.name),
                    self.canon_assignment_target_maybe_default(&property.binding),
                    Canon::leaf("flag", property.computed.to_string()),
                ],
            ),
        }
    }

    fn canon_simple_assignment_target(&self, target: &SimpleAssignmentTarget) -> Canon {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => {
                self.classifier.classify_reference(self.scoping, ident)
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.canon_computed_member(member)
            }
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.canon_static_member(member)
            }
            SimpleAssignmentTarget::PrivateFieldExpression(member) => {
                self.canon_private_field_member(member)
            }
            SimpleAssignmentTarget::TSAsExpression(_)
            | SimpleAssignmentTarget::TSSatisfiesExpression(_)
            | SimpleAssignmentTarget::TSNonNullExpression(_)
            | SimpleAssignmentTarget::TSTypeAssertion(_) => {
                refused("SimpleAssignmentTarget", "TS-*")
            }
        }
    }

    // -- Member expressions ----------------------------------------------------

    fn canon_static_member(&self, member: &StaticMemberExpression) -> Canon {
        // The property NAME is contract (e.g. `_ctx.msg` — `msg` is
        // source-authored; `$evtclick` is the delegation ABI).
        self.wrap(
            "StaticMemberExpression",
            vec![
                self.canon_expression(&member.object),
                Canon::leaf("ident", member.property.name.as_str()),
                Canon::leaf("flag", member.optional.to_string()),
            ],
            member.node_id.get(),
        )
    }

    fn canon_computed_member(&self, member: &ComputedMemberExpression) -> Canon {
        self.wrap(
            "ComputedMemberExpression",
            vec![
                self.canon_expression(&member.object),
                self.canon_expression(&member.expression),
                Canon::leaf("flag", member.optional.to_string()),
            ],
            member.node_id.get(),
        )
    }

    fn canon_private_field_member(&self, member: &PrivateFieldExpression) -> Canon {
        self.wrap(
            "PrivateFieldExpression",
            vec![
                self.canon_expression(&member.object),
                Canon::leaf("ident", member.field.name.as_str()),
                Canon::leaf("flag", member.optional.to_string()),
            ],
            member.node_id.get(),
        )
    }

    // -- Expressions -----------------------------------------------------------

    fn canon_call(&self, call: &CallExpression) -> Canon {
        let mut children = vec![self.canon_expression(&call.callee)];
        for argument in &call.arguments {
            match argument {
                Argument::SpreadElement(spread) => children.push(Canon::node(
                    "spread",
                    vec![self.canon_expression(&spread.argument)],
                )),
                expression => children.push(self.canon_expression(expression.to_expression())),
            }
        }
        children.push(Canon::leaf("flag", call.optional.to_string()));
        children.push(Canon::leaf("flag", call.pure.to_string()));
        self.wrap("CallExpression", children, call.node_id.get())
    }

    fn canon_template_literal(&self, template: &TemplateLiteral) -> Canon {
        let mut children = Vec::new();
        for quasi in &template.quasis {
            let text = quasi
                .value
                .cooked
                .as_ref()
                .unwrap_or(&quasi.value.raw)
                .as_str();
            children.push(Canon::node("quasi", vec![Canon::leaf("tpl", text)]));
        }
        for expression in &template.expressions {
            children.push(self.canon_expression(expression));
        }
        Canon::node("TemplateLiteral", children)
    }

    fn canon_expression(&self, expression: &Expression) -> Canon {
        match expression {
            Expression::BooleanLiteral(literal) => Canon::leaf("bool", literal.value.to_string()),
            Expression::NullLiteral(_) => Canon::leaf("null", ""),
            Expression::NumericLiteral(literal) => {
                Canon::leaf("num", literal.value.to_bits().to_string())
            }
            Expression::BigIntLiteral(literal) => Canon::leaf("bigint", literal.value.as_str()),
            Expression::RegExpLiteral(literal) => Canon::leaf(
                "regex",
                format!(
                    "{}/{}",
                    literal.regex.pattern.text.as_str(),
                    literal.regex.flags
                ),
            ),
            Expression::StringLiteral(literal) => Canon::leaf("str", literal.value.as_str()),
            Expression::TemplateLiteral(template) => self.canon_template_literal(template),
            Expression::Identifier(ident) => {
                self.classifier.classify_reference(self.scoping, ident)
            }
            Expression::MetaProperty(meta) => Canon::node(
                "MetaProperty",
                vec![
                    Canon::leaf("ident", meta.meta.name.as_str()),
                    Canon::leaf("ident", meta.property.name.as_str()),
                ],
            ),
            Expression::Super(sup) => self.wrap("Super", vec![], sup.node_id.get()),
            Expression::ArrayExpression(array) => {
                let mut children = Vec::new();
                for element in &array.elements {
                    match element {
                        ArrayExpressionElement::SpreadElement(spread) => children.push(
                            Canon::node("spread", vec![self.canon_expression(&spread.argument)]),
                        ),
                        ArrayExpressionElement::Elision(_) => {
                            children.push(Canon::node("hole", vec![]))
                        }
                        expression => {
                            children.push(self.canon_expression(expression.to_expression()))
                        }
                    }
                }
                self.wrap("ArrayExpression", children, array.node_id.get())
            }
            Expression::ArrowFunctionExpression(arrow) => self.canon_arrow(arrow),
            Expression::AssignmentExpression(assignment) => self.wrap(
                "AssignmentExpression",
                vec![
                    Canon::leaf("op", assignment.operator.as_str()),
                    self.canon_assignment_target(&assignment.left),
                    self.canon_expression(&assignment.right),
                ],
                assignment.node_id.get(),
            ),
            Expression::AwaitExpression(await_expr) => self.wrap(
                "AwaitExpression",
                vec![self.canon_expression(&await_expr.argument)],
                await_expr.node_id.get(),
            ),
            Expression::BinaryExpression(binary) => self.wrap(
                "BinaryExpression",
                vec![
                    self.canon_expression(&binary.left),
                    Canon::leaf("op", binary.operator.as_str()),
                    self.canon_expression(&binary.right),
                ],
                binary.node_id.get(),
            ),
            Expression::CallExpression(call) => self.canon_call(call),
            Expression::ChainExpression(chain) => {
                let element = match &chain.expression {
                    ChainElement::CallExpression(call) => self.canon_call(call),
                    ChainElement::TSNonNullExpression(_) => refused("ChainElement", "TSNonNull"),
                    ChainElement::ComputedMemberExpression(member) => {
                        self.canon_computed_member(member)
                    }
                    ChainElement::StaticMemberExpression(member) => {
                        self.canon_static_member(member)
                    }
                    ChainElement::PrivateFieldExpression(member) => {
                        self.canon_private_field_member(member)
                    }
                };
                self.wrap("ChainExpression", vec![element], chain.node_id.get())
            }
            Expression::ClassExpression(class) => self.canon_class(class, "ClassExpression"),
            Expression::ConditionalExpression(conditional) => self.wrap(
                "ConditionalExpression",
                vec![
                    self.canon_expression(&conditional.test),
                    self.canon_expression(&conditional.consequent),
                    self.canon_expression(&conditional.alternate),
                ],
                conditional.node_id.get(),
            ),
            Expression::FunctionExpression(function) => {
                self.canon_function(function, "FunctionExpression")
            }
            Expression::ImportExpression(import) => self.wrap(
                "ImportExpression",
                vec![
                    self.canon_expression(&import.source),
                    self.canon_opt_expression(&import.options),
                ],
                import.node_id.get(),
            ),
            Expression::LogicalExpression(logical) => self.wrap(
                "LogicalExpression",
                vec![
                    self.canon_expression(&logical.left),
                    Canon::leaf("op", logical.operator.as_str()),
                    self.canon_expression(&logical.right),
                ],
                logical.node_id.get(),
            ),
            Expression::NewExpression(new_expr) => {
                let mut children = vec![self.canon_expression(&new_expr.callee)];
                for argument in &new_expr.arguments {
                    match argument {
                        Argument::SpreadElement(spread) => children.push(Canon::node(
                            "spread",
                            vec![self.canon_expression(&spread.argument)],
                        )),
                        expression => {
                            children.push(self.canon_expression(expression.to_expression()))
                        }
                    }
                }
                children.push(Canon::leaf("flag", new_expr.pure.to_string()));
                self.wrap("NewExpression", children, new_expr.node_id.get())
            }
            Expression::ObjectExpression(object) => {
                let mut children = Vec::new();
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(property) => {
                            let kind = match property.kind {
                                PropertyKind::Init => "init",
                                PropertyKind::Get => "get",
                                PropertyKind::Set => "set",
                            };
                            children.push(Canon::node(
                                "ObjectProperty",
                                vec![
                                    self.canon_property_key(&property.key),
                                    self.canon_expression(&property.value),
                                    Canon::leaf("op", kind),
                                    Canon::leaf("flag", property.method.to_string()),
                                    Canon::leaf("flag", property.shorthand.to_string()),
                                    Canon::leaf("flag", property.computed.to_string()),
                                ],
                            ));
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => children.push(Canon::node(
                            "spread",
                            vec![self.canon_expression(&spread.argument)],
                        )),
                    }
                }
                self.wrap("ObjectExpression", children, object.node_id.get())
            }
            Expression::ParenthesizedExpression(paren) => {
                // Transparent: behavior-preserving parens are waived.
                self.canon_expression(&paren.expression)
            }
            Expression::SequenceExpression(sequence) => {
                let mut children = Vec::new();
                for expression in &sequence.expressions {
                    children.push(self.canon_expression(expression));
                }
                self.wrap("SequenceExpression", children, sequence.node_id.get())
            }
            Expression::TaggedTemplateExpression(tagged) => {
                let tag = self.canon_expression(&tagged.tag);
                let quasi = self.canon_template_literal(&tagged.quasi);
                self.wrap(
                    "TaggedTemplateExpression",
                    vec![tag, quasi],
                    tagged.node_id.get(),
                )
            }
            Expression::ThisExpression(this) => {
                self.wrap("ThisExpression", vec![], this.node_id.get())
            }
            Expression::UnaryExpression(unary) => self.wrap(
                "UnaryExpression",
                vec![
                    Canon::leaf("op", unary.operator.as_str()),
                    self.canon_expression(&unary.argument),
                ],
                unary.node_id.get(),
            ),
            Expression::UpdateExpression(update) => self.wrap(
                "UpdateExpression",
                vec![
                    Canon::leaf("op", update.operator.as_str()),
                    Canon::leaf("flag", update.prefix.to_string()),
                    self.canon_simple_assignment_target(&update.argument),
                ],
                update.node_id.get(),
            ),
            Expression::YieldExpression(yield_expr) => self.wrap(
                "YieldExpression",
                vec![
                    Canon::leaf("flag", yield_expr.delegate.to_string()),
                    self.canon_opt_expression(&yield_expr.argument),
                ],
                yield_expr.node_id.get(),
            ),
            Expression::PrivateInExpression(private_in) => self.wrap(
                "PrivateInExpression",
                vec![
                    Canon::leaf("ident", private_in.left.name.as_str()),
                    self.canon_expression(&private_in.right),
                ],
                private_in.node_id.get(),
            ),
            Expression::ComputedMemberExpression(member) => self.canon_computed_member(member),
            Expression::StaticMemberExpression(member) => self.canon_static_member(member),
            Expression::PrivateFieldExpression(member) => self.canon_private_field_member(member),
            Expression::JSXElement(_) | Expression::JSXFragment(_) => refused("Expression", "JSX"),
            Expression::TSAsExpression(_)
            | Expression::TSSatisfiesExpression(_)
            | Expression::TSTypeAssertion(_)
            | Expression::TSNonNullExpression(_)
            | Expression::TSInstantiationExpression(_) => refused("Expression", "TS-*"),
            Expression::V8IntrinsicExpression(_) => refused("Expression", "V8Intrinsic"),
        }
    }

    // -- Imports -----------------------------------------------------------------

    pub(crate) fn extract_imports(&self, program: &Program) -> Vec<ImportEntry> {
        let mut entries = Vec::new();
        for statement in &program.body {
            let Statement::ImportDeclaration(import) = statement else {
                continue;
            };
            let mut entry = ImportEntry {
                source: import.source.value.to_string(),
                side_effect: import.specifiers.is_none(),
                default: None,
                namespace: None,
                named: Vec::new(),
                attributes: Vec::new(),
            };
            if let Some(specifiers) = &import.specifiers {
                for specifier in specifiers {
                    match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                            let imported = module_export_name(&specifier.imported);
                            let alias = self.classifier.classify_binding(&specifier.local);
                            entry.named.push((imported, alias));
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                            entry.default =
                                Some(self.classifier.classify_binding(&specifier.local));
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                            entry.namespace =
                                Some(self.classifier.classify_binding(&specifier.local));
                        }
                    }
                }
            }
            entry.named.sort_by(|a, b| a.0.cmp(&b.0));
            if let Some(with_clause) = &import.with_clause {
                for with_entry in &with_clause.with_entries {
                    let key = match &with_entry.key {
                        ImportAttributeKey::Identifier(name) => name.name.to_string(),
                        ImportAttributeKey::StringLiteral(literal) => literal.value.to_string(),
                    };
                    entry
                        .attributes
                        .push((key, with_entry.value.value.to_string()));
                }
                entry.attributes.sort();
            }
            entries.push(entry);
        }
        entries
    }
}

pub(crate) fn module_export_name(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::IdentifierName(name) => name.name.to_string(),
        ModuleExportName::IdentifierReference(reference) => reference.name.to_string(),
        ModuleExportName::StringLiteral(literal) => literal.value.to_string(),
    }
}
