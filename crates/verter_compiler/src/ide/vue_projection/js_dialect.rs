//! The JavaScript form of a JavaScript carrier's checking module.
//!
//! A `<script>` without `lang="ts"` is checked as JavaScript, as vue-tsc
//! checks it: the checking module keeps the script's language, so `checkJs`
//! decides whether the carrier reports errors, JSDoc annotates the authored
//! code and JavaScript inference types it. The composer writes its generated
//! text in TypeScript. This module gives that text its JavaScript form, with
//! every type moved into JSDoc:
//!
//! - `type N<T> = X;` becomes `@template` and `@typedef` tags;
//! - `declare function` overloads and `declare const` become one typed `var`
//!   each, read once at the end so an unused helper is not an unused local;
//! - `e as T`, `e satisfies T`, `null!` and explicit call type arguments
//!   become JSDoc `@type` / `@satisfies` casts;
//! - `const x: T = e` becomes `/** @type {T} */ const x = e`;
//! - a `declare module` augmentation of empty interfaces adds no member and
//!   is dropped.
//!
//! Authored pieces are opaque: each stands in the parsed text as a
//! placeholder identifier and comes back unchanged, so nothing authored is
//! parsed or rewritten here, and a generated type may not contain one. The
//! result is parsed again as JavaScript; anything left without a JavaScript
//! form is refused, so a JavaScript module never carries TypeScript syntax.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    CallExpression, Expression, Function, NewExpression, Program, Statement, TSAsExpression,
    TSInstantiationExpression, TSNonNullExpression, TSSatisfiesExpression, TSTypeAliasDeclaration,
    TSTypeParameterDeclaration, TSTypeParameterInstantiation, VariableDeclaration,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::{GetSpan, SourceType, Span};
use verter_parser::oxc_parse::Parser;

use crate::code_transform::CodeTransform;
use crate::ide::vue_projection::checking_fragment::{CheckingFragment, CheckingPiece};

/// Why generated text has no JavaScript form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoJavaScriptForm(pub(crate) String);

/// The JavaScript form of module-scope declarations: preludes, global
/// component fallbacks and the public declaration. All of it is generated.
pub(crate) fn javascript_declarations(text: &str) -> Result<String, NoJavaScriptForm> {
    let statements = [vec![Part::Generated(text)]];
    let mut rendered = javascript_form(&statements, Scope::Module)?;
    Ok(rendered
        .pop()
        .unwrap_or_default()
        .into_iter()
        .map(|part| match part {
            Rendered::Generated(text) => text,
            Rendered::Authored(_) => String::new(),
        })
        .collect())
}

/// The JavaScript form of statements that stand inside a function body. Each
/// fragment keeps its authored pieces, in the order the rewritten text
/// places them; only its generated pieces change.
pub(crate) fn javascript_statements(
    statements: &[CheckingFragment],
) -> Result<Vec<CheckingFragment>, NoJavaScriptForm> {
    let parts: Vec<Vec<Part<'_>>> = statements
        .iter()
        .map(|fragment| {
            fragment
                .pieces
                .iter()
                .map(|piece| match piece {
                    CheckingPiece::Synth(text) => Part::Generated(text),
                    _ => Part::Authored,
                })
                .collect()
        })
        .collect();
    let rendered = javascript_form(&parts, Scope::Function)?;
    Ok(statements
        .iter()
        .zip(rendered)
        .map(|(fragment, parts)| {
            let authored: Vec<&CheckingPiece> = fragment
                .pieces
                .iter()
                .filter(|piece| !matches!(piece, CheckingPiece::Synth(_)))
                .collect();
            CheckingFragment {
                pieces: parts
                    .into_iter()
                    .map(|part| match part {
                        Rendered::Generated(text) => CheckingPiece::Synth(text),
                        Rendered::Authored(index) => authored[index].clone(),
                    })
                    .collect(),
            }
        })
        .collect())
}

/// One part of a statement: generated text, or an authored piece that is
/// kept as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part<'p> {
    Generated(&'p str),
    Authored,
}

/// One part of a rewritten statement; `Authored(i)` is the statement's
/// `i`-th authored part.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Rendered {
    Generated(String),
    Authored(usize),
}

/// Where the statements stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Module,
    Function,
}

impl Scope {
    fn wrapper(self) -> (&'static str, &'static str) {
        match self {
            Self::Module => ("", ""),
            Self::Function => ("function __verter_js_scope() {\n", "\n}\n"),
        }
    }
}

const PLACEHOLDER: &str = "__verter_authored_";

fn refuse<T>(detail: impl Into<String>) -> Result<T, NoJavaScriptForm> {
    Err(NoJavaScriptForm(detail.into()))
}

/// The flattened text, with the bounds of each statement and of each
/// authored placeholder.
struct Flattened {
    text: String,
    statements: Vec<(u32, u32)>,
    placeholders: Vec<(u32, u32)>,
}

fn flatten(statements: &[Vec<Part<'_>>], scope: Scope) -> Flattened {
    let (open, close) = scope.wrapper();
    let mut text = String::from(open);
    let mut bounds = Vec::with_capacity(statements.len());
    let mut placeholders = Vec::new();
    for (statement, parts) in statements.iter().enumerate() {
        let start = text.len() as u32;
        let mut authored = 0usize;
        for part in parts {
            match part {
                Part::Generated(generated) => text.push_str(generated),
                Part::Authored => {
                    let at = text.len() as u32;
                    text.push_str(&format!("{PLACEHOLDER}{statement}_{authored}__"));
                    placeholders.push((at, text.len() as u32));
                    authored += 1;
                }
            }
        }
        bounds.push((start, text.len() as u32));
    }
    text.push_str(close);
    Flattened {
        text,
        statements: bounds,
        placeholders,
    }
}

/// One rewrite of the flattened text, applied by [`CodeTransform`]. Inserts
/// (`start == end`) at one offset apply outermost first; `outer` orders them.
#[derive(Debug, Clone)]
struct Edit {
    start: u32,
    end: u32,
    text: String,
    outer: u32,
}

fn javascript_form(
    statements: &[Vec<Part<'_>>],
    scope: Scope,
) -> Result<Vec<Vec<Rendered>>, NoJavaScriptForm> {
    let flat = flatten(statements, scope);
    let allocator = Allocator::default();
    let parsed = Parser::new(
        &allocator,
        &flat.text,
        // TSX: a hoisted copy of authored JavaScript may hold JSX, and the
        // generated text spells no angle-bracket assertion.
        SourceType::tsx().with_module(true),
    )
    .parse();
    if parsed.fatal_error || !parsed.diagnostics.is_empty() {
        return refuse(format!(
            "the generated text does not parse: {}",
            parsed
                .diagnostics
                .first()
                .map_or_else(String::new, ToString::to_string)
        ));
    }
    let mut rewriter = Rewriter {
        text: &flat.text,
        placeholders: &flat.placeholders,
        edits: Vec::new(),
        declared: Vec::new(),
        refusal: None,
    };
    verter_parser::oxc_parse::with_program_stack(&parsed.program, || {
        rewriter.program(&parsed.program, scope);
    });
    if let Some(refusal) = rewriter.refusal {
        return Err(refusal);
    }
    let Rewriter {
        edits, declared, ..
    } = rewriter;
    // An edit belongs to the statement its start lies in; an insert at a
    // statement's end opens the next one.
    if edits.iter().any(|edit| {
        !flat
            .statements
            .iter()
            .any(|&(start, end)| start <= edit.start && edit.start < end && edit.end <= end)
    }) {
        return refuse("a generated rewrite crosses or lies outside a statement");
    }
    let mut output: Vec<String> = Vec::with_capacity(flat.statements.len());
    for &(start, end) in &flat.statements {
        output.push(rewrite_statement(&flat.text, start, end, &edits)?);
    }
    if !declared.is_empty() {
        if let Some(last) = output.last_mut() {
            last.push_str(&format!("\nvoid [{}];\n", declared.join(", ")));
        }
    }
    let (open, close) = scope.wrapper();
    let javascript = format!("{open}{}{close}", output.concat());
    let reparsed =
        Parser::new(&allocator, &javascript, SourceType::jsx().with_module(true)).parse();
    if reparsed.fatal_error || !reparsed.diagnostics.is_empty() {
        return refuse(format!(
            "the generated text keeps TypeScript syntax: {}",
            reparsed
                .diagnostics
                .first()
                .map_or_else(String::new, ToString::to_string)
        ));
    }
    output
        .iter()
        .zip(statements)
        .map(|(text, parts)| split(text, parts))
        .collect()
}

/// One statement of `text`, `start..end`, with its rewrites applied by the
/// source-edit engine. Replacements never overlap; inserts at one offset
/// apply outermost first, ahead of what starts there.
fn rewrite_statement(
    text: &str,
    start: u32,
    end: u32,
    edits: &[Edit],
) -> Result<String, NoJavaScriptForm> {
    let own = edits
        .iter()
        .filter(|edit| start <= edit.start && edit.start < end);
    let mut replacements: Vec<&Edit> = own.clone().filter(|edit| edit.end > edit.start).collect();
    replacements.sort_by_key(|edit| edit.start);
    if replacements
        .windows(2)
        .any(|pair| pair[1].start < pair[0].end)
    {
        return refuse("two generated rewrites overlap");
    }
    let mut inserts: Vec<&Edit> = own.filter(|edit| edit.end == edit.start).collect();
    inserts.sort_by_key(|edit| (edit.start, u32::MAX - edit.outer));
    let allocator = Allocator::default();
    let mut transform = CodeTransform::new(&text[start as usize..end as usize], &allocator);
    for edit in replacements {
        transform.overwrite(edit.start - start, edit.end - start, &edit.text);
    }
    for edit in inserts {
        transform.append_left(edit.start - start, &edit.text);
    }
    Ok(transform.build_string())
}

/// Split one rewritten statement back into generated text and its authored
/// parts. Every authored part must come back exactly once.
fn split(text: &str, parts: &[Part<'_>]) -> Result<Vec<Rendered>, NoJavaScriptForm> {
    let expected = parts
        .iter()
        .filter(|part| matches!(part, Part::Authored))
        .count();
    let mut seen = vec![false; expected];
    let mut rendered = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(PLACEHOLDER) {
        if at > 0 {
            rendered.push(Rendered::Generated(rest[..at].to_string()));
        }
        let tail = &rest[at + PLACEHOLDER.len()..];
        let Some(close) = tail.find("__") else {
            return refuse("an authored placeholder is malformed");
        };
        let index = tail[..close]
            .rsplit('_')
            .next()
            .and_then(|ordinal| ordinal.parse::<usize>().ok());
        let Some(index) = index.filter(|index| *index < expected && !seen[*index]) else {
            return refuse("an authored placeholder is malformed or repeated");
        };
        seen[index] = true;
        rendered.push(Rendered::Authored(index));
        rest = &tail[close + 2..];
    }
    if !rest.is_empty() {
        rendered.push(Rendered::Generated(rest.to_string()));
    }
    if seen.iter().any(|seen| !seen) {
        return refuse("an authored piece was lost");
    }
    Ok(rendered)
}

struct Rewriter<'t> {
    text: &'t str,
    placeholders: &'t [(u32, u32)],
    edits: Vec<Edit>,
    /// Names declared by `declare` statements, read once at the end.
    declared: Vec<String>,
    refusal: Option<NoJavaScriptForm>,
}

impl<'t> Rewriter<'t> {
    fn fail(&mut self, detail: impl Into<String>) {
        if self.refusal.is_none() {
            self.refusal = Some(NoJavaScriptForm(detail.into()));
        }
    }

    fn slice(&self, span: Span) -> &'t str {
        &self.text[span.start as usize..span.end as usize]
    }

    /// Generated type text for a JSDoc tag: it may hold no authored piece
    /// and may not close the comment.
    fn type_text(&mut self, span: Span) -> Option<String> {
        if self.authored_within(span.start, span.end) {
            self.fail("a generated type holds an authored piece");
            return None;
        }
        let text = self.slice(span);
        if text.contains("*/") {
            self.fail("a generated type closes a comment");
            return None;
        }
        Some(text.to_string())
    }

    /// `span` widened over a `declare` keyword before it, so a rewrite of
    /// the declaration replaces the keyword too.
    fn declaration_span(&self, span: Span) -> Span {
        let before = self.text[..span.start as usize].trim_end();
        match before.strip_suffix("declare") {
            Some(rest)
                if !rest.ends_with(|c: char| c == '_' || c == '$' || c.is_alphanumeric()) =>
            {
                Span::new(rest.len() as u32, span.end)
            }
            _ => span,
        }
    }

    fn authored_within(&self, start: u32, end: u32) -> bool {
        self.placeholders
            .iter()
            .any(|&(p_start, p_end)| p_start < end && start < p_end)
    }

    fn insert(&mut self, at: u32, text: String, outer: Span) {
        self.edits.push(Edit {
            start: at,
            end: at,
            text,
            outer: outer.size(),
        });
    }

    fn replace(&mut self, start: u32, end: u32, text: String) {
        if self.authored_within(start, end) {
            self.fail("a generated rewrite would replace an authored piece");
            return;
        }
        self.edits.push(Edit {
            start,
            end,
            text,
            outer: end - start,
        });
    }

    fn program(&mut self, program: &Program<'_>, scope: Scope) {
        match scope {
            Scope::Module => self.statements(&program.body),
            Scope::Function => {
                for statement in &program.body {
                    if let Statement::FunctionDeclaration(function) = statement {
                        if let Some(body) = &function.body {
                            self.statements(&body.statements);
                        }
                    }
                }
            }
        }
    }

    /// Top-level statements: declarations are rewritten whole, anything else
    /// is walked.
    fn statements(&mut self, statements: &[Statement<'_>]) {
        let mut index = 0;
        while index < statements.len() {
            match &statements[index] {
                Statement::TSTypeAliasDeclaration(alias) => self.type_alias(alias),
                Statement::FunctionDeclaration(function) if function.body.is_none() => {
                    let Some(name) = function.id.as_ref().map(|id| id.name.as_str()) else {
                        self.fail("a declared function has no name");
                        return;
                    };
                    let mut overloads = vec![&**function];
                    while let Some(Statement::FunctionDeclaration(next)) = statements.get(index + 1)
                    {
                        if next.body.is_some()
                            || next.id.as_ref().map(|id| id.name.as_str()) != Some(name)
                        {
                            break;
                        }
                        overloads.push(&**next);
                        index += 1;
                    }
                    self.declared_function(name, &overloads);
                }
                Statement::VariableDeclaration(declaration) if declaration.declare => {
                    self.declared_variable(declaration);
                }
                Statement::TSExternalModuleDeclaration(module) => {
                    // An augmentation of interfaces that declare no member
                    // merges nothing; any other augmentation has no
                    // JavaScript form.
                    let empty = module.body.as_ref().is_some_and(|body| {
                        body.body.iter().all(|statement| {
                            matches!(statement, Statement::TSInterfaceDeclaration(interface)
                                if interface.body.body.is_empty()
                                    && interface.extends.is_empty()
                                    && interface.type_parameters.is_none())
                        })
                    });
                    if empty {
                        self.replace(module.span.start, module.span.end, String::new());
                    } else {
                        self.fail("a module augmentation has no JavaScript form");
                    }
                }
                statement => self.visit_statement(statement),
            }
            index += 1;
        }
    }

    /// `@template` tags of a type parameter list, one per parameter.
    fn template_tags(&mut self, parameters: Option<&TSTypeParameterDeclaration<'_>>) -> String {
        let mut tags = String::new();
        for parameter in parameters.map_or(&[][..], |list| &list.params[..]) {
            // TypeScript reads `@template {Constraint} const [Name=Default]`.
            tags.push_str(" * @template ");
            if let Some(constraint) = &parameter.constraint {
                let Some(constraint) = self.type_text(constraint.span()) else {
                    return tags;
                };
                tags.push_str(&format!("{{{constraint}}} "));
            }
            if parameter.r#const {
                tags.push_str("const ");
            }
            let name = parameter.name.name.as_str();
            match &parameter.default {
                Some(default) => {
                    let Some(default) = self.type_text(default.span()) else {
                        return tags;
                    };
                    tags.push_str(&format!("[{name}={default}]"));
                }
                None => tags.push_str(name),
            }
            tags.push('\n');
        }
        tags
    }

    fn type_alias(&mut self, alias: &TSTypeAliasDeclaration<'_>) {
        let tags = self.template_tags(alias.type_parameters.as_deref());
        let Some(body) = self.type_text(alias.type_annotation.span()) else {
            return;
        };
        let name = alias.id.name.as_str();
        let span = self.declaration_span(alias.span);
        self.replace(
            span.start,
            span.end,
            format!("/**\n{tags} * @typedef {{{body}}} {name}\n */"),
        );
    }

    /// One function type per overload; their intersection resolves calls
    /// in overload order.
    fn declared_function(&mut self, name: &str, overloads: &[&Function<'_>]) {
        let mut signatures = Vec::with_capacity(overloads.len());
        for overload in overloads {
            if overload.this_param.is_some() {
                self.fail("a declared function takes `this`");
                return;
            }
            let Some(returns) = overload.return_type.as_ref() else {
                self.fail("a declared function has no return type");
                return;
            };
            let Some(returns) = self.type_text(returns.type_annotation.span()) else {
                return;
            };
            let parameters = match &overload.type_parameters {
                Some(list) => match self.type_text(list.span) {
                    Some(text) => text,
                    None => return,
                },
                None => String::new(),
            };
            let Some(params) = self.type_text(overload.params.span) else {
                return;
            };
            let params = if params.starts_with('(') {
                params
            } else {
                format!("({params})")
            };
            signatures.push(format!("({parameters}{params} => {returns})"));
        }
        let (Some(first), Some(last)) = (overloads.first(), overloads.last()) else {
            return;
        };
        self.declared.push(name.to_string());
        let span = self.declaration_span(first.span);
        self.replace(
            span.start,
            last.span.end,
            format!(
                "/** @type {{{}}} */\nvar {name} = /** @type {{any}} */ (null);",
                signatures.join(" & ")
            ),
        );
    }

    fn declared_variable(&mut self, declaration: &VariableDeclaration<'_>) {
        let [declarator] = &declaration.declarations[..] else {
            self.fail("a declaration declares more than one name");
            return;
        };
        let (Some(name), Some(annotation)) = (
            declarator.id.get_identifier_name(),
            declarator.type_annotation.as_ref(),
        ) else {
            self.fail("a declared variable has no name or type");
            return;
        };
        let name = name.as_str().to_string();
        let Some(ty) = self.type_text(annotation.type_annotation.span()) else {
            return;
        };
        self.declared.push(name.clone());
        let span = self.declaration_span(declaration.span);
        self.replace(
            span.start,
            span.end,
            format!("/** @type {{{ty}}} */\nvar {name} = /** @type {{any}} */ (null);"),
        );
    }

    /// `callee<A>`: the instantiation as a JSDoc cast of the callee.
    fn instantiated(
        &mut self,
        callee: &Expression<'_>,
        arguments: &TSTypeParameterInstantiation<'_>,
    ) {
        let callee_span = callee.span();
        let simple = matches!(
            callee.get_inner_expression(),
            Expression::Identifier(_) | Expression::StaticMemberExpression(_)
        );
        if !simple || self.authored_within(callee_span.start, callee_span.end) {
            self.fail("type arguments apply to a callee without a name");
            return;
        }
        let Some(arguments_text) = self.type_text(arguments.span) else {
            return;
        };
        let callee_text = self.slice(callee_span).to_string();
        self.replace(
            callee_span.start,
            arguments.span.end,
            format!("/** @type {{typeof {callee_text}{arguments_text}}} */ ({callee_text})"),
        );
    }
}

impl<'a> Visit<'a> for Rewriter<'_> {
    fn visit_ts_as_expression(&mut self, it: &TSAsExpression<'a>) {
        let ty = if it.type_annotation.is_const_type_reference() {
            Some("const".to_string())
        } else {
            self.type_text(it.type_annotation.span())
        };
        let Some(ty) = ty else {
            return;
        };
        let inner = it.expression.span();
        self.insert(inner.start, format!("/** @type {{{ty}}} */ ("), it.span);
        self.replace(inner.end, it.span.end, ")".to_string());
        self.visit_expression(&it.expression);
    }

    fn visit_ts_satisfies_expression(&mut self, it: &TSSatisfiesExpression<'a>) {
        let Some(ty) = self.type_text(it.type_annotation.span()) else {
            return;
        };
        let inner = it.expression.span();
        self.insert(
            inner.start,
            format!("/** @satisfies {{{ty}}} */ ("),
            it.span,
        );
        self.replace(inner.end, it.span.end, ")".to_string());
        self.visit_expression(&it.expression);
    }

    fn visit_ts_non_null_expression(&mut self, it: &TSNonNullExpression<'a>) {
        if matches!(
            it.expression.get_inner_expression(),
            Expression::NullLiteral(_)
        ) {
            self.replace(
                it.span.start,
                it.span.end,
                "/** @type {never} */ (/** @type {unknown} */ (null))".to_string(),
            );
        } else {
            self.fail("a non-null assertion has no JavaScript form");
        }
    }

    fn visit_ts_instantiation_expression(&mut self, it: &TSInstantiationExpression<'a>) {
        self.instantiated(&it.expression, &it.type_arguments);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        match &it.type_arguments {
            Some(arguments) => {
                self.instantiated(&it.callee, arguments);
                for argument in &it.arguments {
                    self.visit_argument(argument);
                }
            }
            None => walk::walk_call_expression(self, it),
        }
    }

    fn visit_new_expression(&mut self, it: &NewExpression<'a>) {
        match &it.type_arguments {
            Some(arguments) => {
                self.instantiated(&it.callee, arguments);
                for argument in &it.arguments {
                    self.visit_argument(argument);
                }
            }
            None => walk::walk_new_expression(self, it),
        }
    }

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        if let [declarator] = &it.declarations[..] {
            if let Some(annotation) = &declarator.type_annotation {
                if let Some(ty) = self.type_text(annotation.type_annotation.span()) {
                    self.insert(it.span.start, format!("/** @type {{{ty}}} */ "), it.span);
                    self.replace(declarator.id.span().end, annotation.span.end, String::new());
                }
                if let Some(init) = &declarator.init {
                    self.visit_expression(init);
                }
                return;
            }
        }
        walk::walk_variable_declaration(self, it);
    }
}
