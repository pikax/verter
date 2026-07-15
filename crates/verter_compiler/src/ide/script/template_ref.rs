//! Template-ref call inference (ownership-domain analysis).
//!
//! Hosts the `collect_binding_names` helper, the `TemplateRefCandidate /
//! TemplateRefSelector / TemplateRefCallKind / TemplateRefCallSite /
//! TemplateRefScriptScanner` types, the `apply_template_ref_call_inference`
//! entry point, and all the supporting walk helpers (`unwrap_wrapped_expression`,
//! `callee_identifier_name`, `is_null_argument`, etc.) that the scanner needs.

use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Declaration, ExportDefaultDeclarationKind,
    Expression, ForStatementInit, Function, ObjectPropertyKind, Statement,
};
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::ast::types::{AstNodeKind, TemplateAst};
use crate::common::Span;
use crate::template::code_gen::binding::{is_simple_ident, BindingType};
use crate::template::code_gen::types::CodeGenOutput;

use super::PREFIX;

pub(super) fn collect_binding_names(
    bindings: &[(Span, BindingType)],
    source: &str,
    content_str: &str,
) -> FxHashSet<String> {
    let mut out = FxHashSet::default();
    for (span, bt) in bindings {
        let name = if *bt == BindingType::Props || *bt == BindingType::PropsAliased {
            &source[span.start as usize..span.end as usize]
        } else {
            &content_str[span.start as usize..span.end as usize]
        };
        if !name.is_empty() {
            out.insert(name.to_string());
        }
    }
    out
}

#[derive(Debug, Clone)]
struct TemplateRefCandidate {
    name_for_match: String,
    name_type: String,
    target_type: String,
}

#[derive(Debug, Clone)]
enum TemplateRefSelector {
    Arg(String),
}

#[derive(Debug, Clone)]
enum TemplateRefCallKind {
    UseTemplateRef {
        selector: Option<TemplateRefSelector>,
    },
    RefVariable {
        var_name: String,
    },
}

#[derive(Debug, Clone)]
struct TemplateRefCallSite {
    kind: TemplateRefCallKind,
    callee_end: u32,
}

#[derive(Default)]
struct TemplateRefScriptScanner {
    call_sites: Vec<TemplateRefCallSite>,
    declaration_string_values: FxHashMap<String, String>,
}

pub(super) fn apply_template_ref_call_inference(
    body: &[Statement<'_>],
    template_ast: Option<&TemplateAst>,
    source: &str,
    script_source: &str,
    content_start: u32,
    available_bindings: &FxHashSet<String>,
    out: &mut CodeGenOutput<'_>,
) {
    let Some(template_ast) = template_ast else {
        return;
    };

    let template_refs = collect_template_ref_candidates(template_ast, source, available_bindings);
    if template_refs.is_empty() {
        return;
    }

    let mut scanner = TemplateRefScriptScanner::default();
    for stmt in body {
        scanner.visit_statement(stmt, script_source);
    }

    if scanner.call_sites.is_empty() {
        return;
    }

    let all_name_types: Vec<String> = template_refs.iter().map(|r| r.name_type.clone()).collect();
    if all_name_types.is_empty() {
        return;
    }
    let names_union = join_type_union(&all_name_types);

    for call in &scanner.call_sites {
        let callee_abs_end = content_start + call.callee_end;
        match &call.kind {
            TemplateRefCallKind::UseTemplateRef { selector } => {
                let matched_types = select_matching_template_ref_types(
                    &template_refs,
                    selector.as_ref(),
                    &scanner.declaration_string_values,
                );
                let types_union = if matched_types.is_empty() {
                    "unknown".to_string()
                } else {
                    join_type_union(&matched_types)
                };
                let generic = format!("<{},{}>", types_union, names_union);
                out.prepend_alloc(callee_abs_end, &generic);
            }
            TemplateRefCallKind::RefVariable { var_name } => {
                let selector = TemplateRefSelector::Arg(var_name.clone());
                let matched_types = select_matching_template_ref_types(
                    &template_refs,
                    Some(&selector),
                    &scanner.declaration_string_values,
                );
                if matched_types.is_empty() {
                    continue;
                }
                let types_union = join_type_union(&matched_types);
                let generic = format!("<{}|null>", types_union);
                out.prepend_alloc(callee_abs_end, &generic);
            }
        }
    }
}

fn collect_template_ref_candidates(
    ast: &TemplateAst,
    source: &str,
    available_bindings: &FxHashSet<String>,
) -> Vec<TemplateRefCandidate> {
    let mut out = Vec::new();
    let Some(content) = &ast.root.content else {
        return out;
    };
    for &child in content.children.iter() {
        collect_template_ref_candidates_from_node(child, ast, source, available_bindings, &mut out);
    }
    out
}

fn collect_template_ref_candidates_from_node(
    id: crate::types::NodeId,
    ast: &TemplateAst,
    source: &str,
    available_bindings: &FxHashSet<String>,
    out: &mut Vec<TemplateRefCandidate>,
) {
    let node = &ast.nodes[id.0];
    let AstNodeKind::Element(el_box) = &node.kind else {
        return;
    };
    let el = el_box.as_ref();
    let tag_name = &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];

    let mut target_type =
        resolve_template_ref_target_type(el, tag_name, source, available_bindings);
    if target_type.is_empty() {
        target_type = "unknown".to_string();
    }
    if element_is_inside_v_for(id, ast) {
        target_type.push_str("[]");
    }

    if let Some(v_ref) = &el.v_ref {
        if let (Some(vs), Some(ve)) = (v_ref.value_start, v_ref.value_end) {
            let name = source[vs as usize..ve as usize].trim();
            if !name.is_empty() {
                out.push(TemplateRefCandidate {
                    name_for_match: name.to_string(),
                    name_type: quote_ts_string(name),
                    target_type: target_type.clone(),
                });
            }
        }
    }

    for prop in &el.props {
        if !prop.is_directive {
            continue;
        }
        let base = &source[prop.start as usize..prop.name_end as usize];
        if base != ":" && base != "v-bind" {
            continue;
        }
        let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) else {
            continue;
        };
        if &source[arg_s as usize..arg_e as usize] != "ref" {
            continue;
        }
        let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) else {
            continue;
        };
        let expr = source[vs as usize..ve as usize].trim();
        if expr.is_empty() || is_function_ref_expression(expr) {
            continue;
        }
        out.push(TemplateRefCandidate {
            name_for_match: expr.to_string(),
            name_type: format!("typeof {}", expr),
            target_type: target_type.clone(),
        });
    }

    if let Some(content) = &el.content {
        for &child in content.children.iter() {
            collect_template_ref_candidates_from_node(child, ast, source, available_bindings, out);
        }
    }
}

fn resolve_template_ref_target_type(
    el: &crate::ast::types::ElementNode,
    _tag_name: &str,
    _source: &str,
    _available_bindings: &FxHashSet<String>,
) -> String {
    // Elements with refs always get a ___VERTER___Comp{offset} build-node function emitted
    // (see walk_children_for_comp). Using ReturnType gives the correct type:
    // - For native elements: the enhanced element type with props
    // - For components: the component instance type (new Component({props}))
    let offset = el.tag_open.start;
    format!("ReturnType<typeof {}Comp{}>", PREFIX, offset)
}

fn element_is_inside_v_for(id: crate::types::NodeId, ast: &TemplateAst) -> bool {
    let mut current = Some(id);
    while let Some(node_id) = current {
        let node = &ast.nodes[node_id.0];
        if let AstNodeKind::Element(el_box) = &node.kind {
            if el_box.v_for.is_some() {
                return true;
            }
        }
        current = node.parent;
    }
    false
}

fn is_function_ref_expression(expr: &str) -> bool {
    let trimmed = expr.trim();
    trimmed.contains("=>") || trimmed.starts_with("function")
}

fn quote_ts_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

fn join_type_union(types: &[String]) -> String {
    let mut seen = FxHashSet::default();
    let mut ordered = Vec::with_capacity(types.len());
    for ty in types {
        if seen.insert(ty.clone()) {
            ordered.push(ty.clone());
        }
    }
    ordered.join("|")
}

fn select_matching_template_ref_types(
    candidates: &[TemplateRefCandidate],
    selector: Option<&TemplateRefSelector>,
    declaration_string_values: &FxHashMap<String, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    if selector.is_none() {
        out.extend(candidates.iter().map(|c| c.target_type.clone()));
        return out;
    }

    let selector_text = match selector {
        Some(TemplateRefSelector::Arg(v)) => v.as_str(),
        None => "",
    };
    let selector_resolved = resolve_declared_string_value(selector_text, declaration_string_values)
        .unwrap_or(selector_text);

    for candidate in candidates {
        let candidate_text = candidate.name_for_match.as_str();
        let candidate_resolved =
            resolve_declared_string_value(candidate_text, declaration_string_values)
                .unwrap_or(candidate_text);

        if selector_text == candidate_text
            || selector_text == candidate_resolved
            || selector_resolved == candidate_text
            || selector_resolved == candidate_resolved
        {
            out.push(candidate.target_type.clone());
        }
    }

    out
}

fn resolve_declared_string_value<'a>(
    key: &'a str,
    declaration_string_values: &'a FxHashMap<String, String>,
) -> Option<&'a str> {
    declaration_string_values.get(key).map(|v| v.as_str())
}

impl TemplateRefScriptScanner {
    fn visit_statement(&mut self, stmt: &Statement, source: &str) {
        match stmt {
            Statement::VariableDeclaration(var_decl) => {
                self.visit_variable_declaration(var_decl, source);
            }
            Statement::ExpressionStatement(expr_stmt) => {
                self.visit_expression(&expr_stmt.expression, source);
            }
            Statement::ReturnStatement(ret) => {
                if let Some(arg) = &ret.argument {
                    self.visit_expression(arg, source);
                }
            }
            Statement::BlockStatement(block) => {
                for stmt in &block.body {
                    self.visit_statement(stmt, source);
                }
            }
            Statement::IfStatement(if_stmt) => {
                self.visit_expression(&if_stmt.test, source);
                self.visit_statement(&if_stmt.consequent, source);
                if let Some(alt) = &if_stmt.alternate {
                    self.visit_statement(alt, source);
                }
            }
            Statement::ForStatement(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    match init {
                        ForStatementInit::VariableDeclaration(var_decl) => {
                            self.visit_variable_declaration(var_decl, source);
                        }
                        _ => {
                            if let Some(expr) = init.as_expression() {
                                self.visit_expression(expr, source);
                            }
                        }
                    }
                }
                if let Some(test) = &for_stmt.test {
                    self.visit_expression(test, source);
                }
                if let Some(update) = &for_stmt.update {
                    self.visit_expression(update, source);
                }
                self.visit_statement(&for_stmt.body, source);
            }
            Statement::ForInStatement(for_in) => {
                if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(var_decl) = &for_in.left
                {
                    self.visit_variable_declaration(var_decl, source);
                }
                self.visit_expression(&for_in.right, source);
                self.visit_statement(&for_in.body, source);
            }
            Statement::ForOfStatement(for_of) => {
                if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(var_decl) = &for_of.left
                {
                    self.visit_variable_declaration(var_decl, source);
                }
                self.visit_expression(&for_of.right, source);
                self.visit_statement(&for_of.body, source);
            }
            Statement::WhileStatement(while_stmt) => {
                self.visit_expression(&while_stmt.test, source);
                self.visit_statement(&while_stmt.body, source);
            }
            Statement::DoWhileStatement(do_while) => {
                self.visit_statement(&do_while.body, source);
                self.visit_expression(&do_while.test, source);
            }
            Statement::SwitchStatement(switch_stmt) => {
                self.visit_expression(&switch_stmt.discriminant, source);
                for case in &switch_stmt.cases {
                    if let Some(test) = &case.test {
                        self.visit_expression(test, source);
                    }
                    for stmt in &case.consequent {
                        self.visit_statement(stmt, source);
                    }
                }
            }
            Statement::TryStatement(try_stmt) => {
                for stmt in &try_stmt.block.body {
                    self.visit_statement(stmt, source);
                }
                if let Some(handler) = &try_stmt.handler {
                    for stmt in &handler.body.body {
                        self.visit_statement(stmt, source);
                    }
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    for stmt in &finalizer.body {
                        self.visit_statement(stmt, source);
                    }
                }
            }
            Statement::ThrowStatement(throw_stmt) => {
                self.visit_expression(&throw_stmt.argument, source);
            }
            Statement::LabeledStatement(labeled) => {
                self.visit_statement(&labeled.body, source);
            }
            Statement::FunctionDeclaration(func) => {
                self.visit_function(func, source);
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(declaration) = &export.declaration {
                    self.visit_declaration(declaration, source);
                }
            }
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    self.visit_function(func, source);
                }
                _ => {
                    if let Some(expr) = export.declaration.as_expression() {
                        self.visit_expression(expr, source);
                    }
                }
            },
            _ => {}
        }
    }

    fn visit_declaration(&mut self, declaration: &Declaration, source: &str) {
        match declaration {
            Declaration::VariableDeclaration(var_decl) => {
                self.visit_variable_declaration(var_decl, source);
            }
            Declaration::FunctionDeclaration(func) => {
                self.visit_function(func, source);
            }
            _ => {}
        }
    }

    fn visit_function(&mut self, function: &Function, source: &str) {
        if let Some(body) = &function.body {
            for stmt in &body.statements {
                self.visit_statement(stmt, source);
            }
        }
    }

    fn visit_variable_declaration(
        &mut self,
        var_decl: &oxc_ast::ast::VariableDeclaration,
        source: &str,
    ) {
        for declarator in &var_decl.declarations {
            if let Some(init) = &declarator.init {
                if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                    self.collect_declared_string_values(id.name.as_str(), init, source);
                    self.record_ref_variable_call(id.name.as_str(), init);
                }
                self.visit_expression(init, source);
            }
        }
    }

    fn record_ref_variable_call(&mut self, var_name: &str, init: &Expression) {
        let expr = unwrap_wrapped_expression(init);
        let Expression::CallExpression(call) = expr else {
            return;
        };
        if call.type_arguments.is_some() {
            return;
        }
        let Some(callee_name) = callee_identifier_name(&call.callee) else {
            return;
        };
        if callee_name != "ref" {
            return;
        }
        if call.arguments.len() > 1 {
            return;
        }
        if call.arguments.len() == 1 && !is_null_argument(&call.arguments[0]) {
            return;
        }

        self.call_sites.push(TemplateRefCallSite {
            kind: TemplateRefCallKind::RefVariable {
                var_name: var_name.to_string(),
            },
            callee_end: call.callee.span().end,
        });
    }

    fn collect_declared_string_values(&mut self, base_name: &str, init: &Expression, source: &str) {
        let expr = unwrap_wrapped_expression(init);
        match expr {
            Expression::StringLiteral(lit) => {
                self.declaration_string_values
                    .insert(base_name.to_string(), lit.value.to_string());
            }
            Expression::TemplateLiteral(tpl) => {
                if tpl.expressions.is_empty() && tpl.quasis.len() == 1 {
                    self.declaration_string_values.insert(
                        base_name.to_string(),
                        tpl.quasis[0].value.raw.as_str().to_string(),
                    );
                }
            }
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    let ObjectPropertyKind::ObjectProperty(obj_prop) = prop else {
                        continue;
                    };
                    if obj_prop.computed {
                        continue;
                    }
                    let key_span = obj_prop.key.span();
                    if key_span.end <= key_span.start {
                        continue;
                    }
                    let key_raw = source[key_span.start as usize..key_span.end as usize].trim();
                    let key = key_raw
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .or_else(|| {
                            key_raw
                                .strip_prefix('\'')
                                .and_then(|s| s.strip_suffix('\''))
                        })
                        .unwrap_or(key_raw)
                        .trim();
                    if key.is_empty() || !is_simple_ident(key) {
                        continue;
                    }
                    let nested = format!("{}.{}", base_name, key);
                    self.collect_declared_string_values(&nested, &obj_prop.value, source);
                }
            }
            _ => {}
        }
    }

    fn visit_expression(&mut self, expr: &Expression, source: &str) {
        match expr {
            Expression::CallExpression(call) => {
                self.maybe_record_use_template_ref_call(call, source);
                self.visit_expression(&call.callee, source);
                for arg in &call.arguments {
                    if let Some(expr) = arg.as_expression() {
                        self.visit_expression(expr, source);
                    }
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    if let Some(expr) = element.as_expression() {
                        self.visit_expression(expr, source);
                    } else if let oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) =
                        element
                    {
                        self.visit_expression(&spread.argument, source);
                    }
                }
            }
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ObjectPropertyKind::ObjectProperty(obj_prop) => {
                            if obj_prop.computed {
                                if let Some(expr) = obj_prop.key.as_expression() {
                                    self.visit_expression(expr, source);
                                }
                            }
                            self.visit_expression(&obj_prop.value, source);
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            self.visit_expression(&spread.argument, source);
                        }
                    }
                }
            }
            Expression::ArrowFunctionExpression(arrow) => {
                for stmt in &arrow.body.statements {
                    self.visit_statement(stmt, source);
                }
            }
            Expression::FunctionExpression(func) => {
                self.visit_function(func, source);
            }
            Expression::AssignmentExpression(assign) => {
                self.visit_expression(&assign.right, source);
            }
            Expression::BinaryExpression(bin) => {
                self.visit_expression(&bin.left, source);
                self.visit_expression(&bin.right, source);
            }
            Expression::LogicalExpression(logical) => {
                self.visit_expression(&logical.left, source);
                self.visit_expression(&logical.right, source);
            }
            Expression::ConditionalExpression(cond) => {
                self.visit_expression(&cond.test, source);
                self.visit_expression(&cond.consequent, source);
                self.visit_expression(&cond.alternate, source);
            }
            Expression::UnaryExpression(unary) => {
                self.visit_expression(&unary.argument, source);
            }
            Expression::AwaitExpression(await_expr) => {
                self.visit_expression(&await_expr.argument, source);
            }
            Expression::ParenthesizedExpression(paren) => {
                self.visit_expression(&paren.expression, source);
            }
            Expression::StaticMemberExpression(member) => {
                self.visit_expression(&member.object, source);
            }
            Expression::ComputedMemberExpression(member) => {
                self.visit_expression(&member.object, source);
                self.visit_expression(&member.expression, source);
            }
            Expression::PrivateFieldExpression(member) => {
                self.visit_expression(&member.object, source);
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                oxc_ast::ast::ChainElement::CallExpression(call) => {
                    self.visit_expression(&call.callee, source);
                    for arg in &call.arguments {
                        if let Some(expr) = arg.as_expression() {
                            self.visit_expression(expr, source);
                        }
                    }
                }
                oxc_ast::ast::ChainElement::StaticMemberExpression(member) => {
                    self.visit_expression(&member.object, source);
                }
                oxc_ast::ast::ChainElement::ComputedMemberExpression(member) => {
                    self.visit_expression(&member.object, source);
                    self.visit_expression(&member.expression, source);
                }
                oxc_ast::ast::ChainElement::PrivateFieldExpression(member) => {
                    self.visit_expression(&member.object, source);
                }
                oxc_ast::ast::ChainElement::TSNonNullExpression(inner) => {
                    self.visit_expression(&inner.expression, source);
                }
            },
            Expression::TemplateLiteral(tpl) => {
                for expr in &tpl.expressions {
                    self.visit_expression(expr, source);
                }
            }
            Expression::SequenceExpression(seq) => {
                for expr in &seq.expressions {
                    self.visit_expression(expr, source);
                }
            }
            Expression::TSAsExpression(ts_as) => {
                self.visit_expression(&ts_as.expression, source);
            }
            Expression::TSSatisfiesExpression(ts_sat) => {
                self.visit_expression(&ts_sat.expression, source);
            }
            Expression::TSNonNullExpression(ts_non_null) => {
                self.visit_expression(&ts_non_null.expression, source);
            }
            Expression::TSTypeAssertion(ts_assertion) => {
                self.visit_expression(&ts_assertion.expression, source);
            }
            Expression::TSInstantiationExpression(ts_instantiation) => {
                self.visit_expression(&ts_instantiation.expression, source);
            }
            _ => {}
        }
    }

    fn maybe_record_use_template_ref_call(&mut self, call: &CallExpression, source: &str) {
        if call.type_arguments.is_some() {
            return;
        }
        let Some(callee_name) = callee_identifier_name(&call.callee) else {
            return;
        };
        if callee_name != "useTemplateRef" {
            return;
        }

        let selector = call.arguments.first().and_then(|arg| {
            let expr = arg.as_expression()?;
            Some(match unwrap_wrapped_expression(expr) {
                Expression::StringLiteral(lit) => TemplateRefSelector::Arg(lit.value.to_string()),
                other => {
                    let span = other.span();
                    if span.end <= span.start {
                        return None;
                    }
                    let raw = source[span.start as usize..span.end as usize].trim();
                    if raw.is_empty() {
                        return None;
                    }
                    TemplateRefSelector::Arg(raw.to_string())
                }
            })
        });

        self.call_sites.push(TemplateRefCallSite {
            kind: TemplateRefCallKind::UseTemplateRef { selector },
            callee_end: call.callee.span().end,
        });
    }
}

pub(super) fn callee_identifier_name<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match unwrap_wrapped_expression(expr) {
        Expression::Identifier(ident) => Some(ident.name.as_str()),
        _ => None,
    }
}

fn is_null_argument(arg: &Argument) -> bool {
    matches!(
        arg.as_expression().map(unwrap_wrapped_expression),
        Some(Expression::NullLiteral(_))
    )
}

pub(super) fn unwrap_wrapped_expression<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    let mut current = expr;
    loop {
        current = match current {
            Expression::ParenthesizedExpression(p) => &p.expression,
            Expression::TSAsExpression(ts_as) => &ts_as.expression,
            Expression::TSSatisfiesExpression(ts_sat) => &ts_sat.expression,
            Expression::TSNonNullExpression(ts_non_null) => &ts_non_null.expression,
            Expression::TSTypeAssertion(ts_assertion) => &ts_assertion.expression,
            Expression::TSInstantiationExpression(ts_instantiation) => &ts_instantiation.expression,
            _ => break,
        };
    }
    current
}
