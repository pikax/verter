//! Shared, framework-neutral TypeScript runtime-survival erasure projection.
//!
//! Both Vue's [`root_binding_index`](super::root_binding_index) and Svelte's
//! `SvelteScopeProjection` (`verter_compiler::svelte::runtime::component_scope_facts`)
//! need the SAME answer to one question before binding a script with
//! [`oxc_semantic::SemanticBuilder`]: "which of this construct leaves NO
//! runtime binding, per TypeScript emit semantics?" — an ambient `declare`
//! construct, a pure `interface`/`type` alias, a type-only `namespace`, a
//! type-only `import`/`export`, a lone bodiless function-overload signature,
//! an abstract class member, a ctor param-property, and the TS expression
//! carriers (`x as T`, `x satisfies T`, `x!`, `<T>x`, `x<T>`).
//!
//! This module owns that classification EXHAUSTIVELY over OXC's
//! `Statement`/`Declaration`/`Expression`/`ClassElement` variants (no
//! wildcard for a TS node kind — a new OXC variant breaks the build) so
//! neither framework re-derives it independently and drifts from the other.
//! The two documented per-framework deltas are explicit parameters on
//! [`ErasureDelta`], not a forked copy of the classifier:
//!
//! - **`enum` declarations.** Svelte REJECTS every `enum` outright (a hard
//!   compile error), so its consumer erases it defensively regardless of
//!   `declare`. Vue's TypeScript support keeps a real (non-ambient) `enum` as
//!   a genuine runtime value binding — only an AMBIENT `declare enum` erases.
//! - **`import X = require(...)` / `TSImportEqualsDeclaration`.** Svelte
//!   treats the whole construct as scope-inert regardless of its own
//!   `import_kind` (its `create_scopes` declares nothing for it). Vue's
//!   binding index needs the opposite: a VALUE `import_kind` genuinely binds
//!   a runtime local (`import Foo = require('./x')`); only a TYPE
//!   `import_kind` (`import type Foo = X.Y`) erases — and this is an
//!   EXPLICIT override of the node's own `import_kind`, not a reliance on
//!   OXC's binder classification, because the pinned `oxc_semantic-0.126.0`
//!   `TSImportEqualsDeclaration` binder path
//!   (`binder.rs:367`) calls `declare_symbol_for_import_specifier(..., false)`
//!   unconditionally, ignoring the node's own `import_kind` — so left
//!   unprojected, `import type Foo = X.Y` would bind as a value symbol in
//!   this exact pinned OXC version.
use oxc_allocator::{Allocator, TakeIn};
use oxc_ast::ast::{
    AccessorPropertyType, ClassBody, ClassElement, Declaration, ExportNamedDeclaration, Expression,
    FormalParameters, MethodDefinitionType, PropertyDefinitionType, Statement, TSAccessibility,
};
use oxc_ast::{match_member_expression, AstBuilder};
use oxc_ast_visit::{walk_mut, VisitMut};

/// The two documented per-framework classification deltas. See the module
/// doc comment for the exact rationale of each.
#[derive(Debug, Clone, Copy)]
pub struct ErasureDelta {
    /// `true` (Svelte): EVERY `enum` (including `declare enum`) erases —
    /// Svelte rejects the construct outright. `false` (Vue): only an
    /// AMBIENT `declare enum` erases; a real `enum` is a runtime value.
    pub enum_always_erased: bool,
    /// `true` (Svelte): `TSImportEqualsDeclaration` always erases,
    /// regardless of its own `import_kind` — the construct is scope-inert
    /// for Svelte. `false` (Vue): erases ONLY when `import_kind` is
    /// explicitly `type` — a value `import_kind` is a genuine runtime
    /// binding, overriding the pinned OXC binder's own (buggy, for this
    /// version) unconditional value-binding of the node.
    pub import_equals_always_erased: bool,
    /// `true` (Svelte): EVERY `TSModuleDeclaration` (`namespace`/`module`)
    /// erases unconditionally, value-containing or not — Svelte hard-rejects
    /// any namespace, so this is a defensive erase regardless of content.
    /// `false` (Vue): erases ONLY when the declaration is ambient
    /// (`declare namespace X { ... }`). A real, non-ambient namespace is left
    /// UNPROJECTED and binds normally: the pinned OXC binder's own
    /// [`get_module_instance_state`](https://github.com/oxc-project/oxc)
    /// logic already gives an instantiated (value-containing) namespace
    /// `SymbolFlags::ValueModule` (included in `SymbolFlags::is_value()`) and
    /// a type-only namespace `SymbolFlags::NamespaceModule` (NOT included in
    /// `is_value()`), so the downstream value-space filter already excludes a
    /// type-only namespace with no extra erasure needed. An AMBIENT
    /// namespace must still be force-erased here: the binder adds
    /// `SymbolFlags::Ambient` alongside `ValueModule` for an instantiated
    /// `declare namespace`, but `Ambient` does not clear the `Value` bit, so
    /// left unprojected an ambient value-namespace would incorrectly survive
    /// the value-space filter despite emitting no runtime binding.
    pub namespace_always_erased: bool,
}

impl ErasureDelta {
    /// Svelte's classification: mirrors svelte's `remove_typescript_nodes ∘
    /// create_scopes` scope view exactly (both deltas erase-always).
    #[must_use]
    pub const fn svelte() -> Self {
        Self {
            enum_always_erased: true,
            import_equals_always_erased: true,
            namespace_always_erased: true,
        }
    }

    /// Vue's classification: a real (non-ambient) `enum` is a runtime value;
    /// `TSImportEqualsDeclaration` erases only when its own `import_kind` is
    /// `type`.
    #[must_use]
    pub const fn vue() -> Self {
        Self {
            enum_always_erased: false,
            import_equals_always_erased: false,
            namespace_always_erased: false,
        }
    }
}

/// Whether a statement is ERASED from the runtime-survival scope view (the
/// construct leaves no runtime binding), per `delta`'s framework-specific
/// `enum`/`TSImportEqualsDeclaration` classification.
///
/// EXHAUSTIVE over OXC's `Statement` (through the inherited `Declaration` /
/// `ModuleDeclaration` variants) — NO wildcard for a TS node kind, so a new
/// OXC statement/declaration variant breaks the build and forces
/// reclassification.
#[must_use]
pub fn statement_is_scope_erased(stmt: &Statement<'_>, delta: ErasureDelta) -> bool {
    match stmt {
        // Runtime value declarations survive UNLESS ambient (`declare`) or,
        // for a function, a lone bodiless overload signature
        // (`function f(): void;`, OXC `Function { body: None }`).
        Statement::VariableDeclaration(d) => d.declare,
        Statement::FunctionDeclaration(f) => f.declare || f.body.is_none(),
        Statement::ClassDeclaration(c) => c.declare,
        Statement::TSEnumDeclaration(e) => delta.enum_always_erased || e.declare,
        Statement::TSImportEqualsDeclaration(i) => {
            delta.import_equals_always_erased || i.import_kind.is_type()
        }
        // A `namespace`/`module` declaration: Svelte erases every one
        // defensively; Vue erases only the ambient (`declare`) form and
        // lets a real one bind normally (the value-space filter already
        // excludes a type-only, non-instantiated one — see
        // `ErasureDelta::namespace_always_erased`).
        Statement::TSModuleDeclaration(m) => delta.namespace_always_erased || m.declare,
        // Pure TS declarations that leave no runtime binding, plus the
        // scope-INERT forms `create_scopes` declares nothing for
        // (`export = X`, `export as namespace X`).
        Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSGlobalDeclaration(_)
        | Statement::TSExportAssignment(_)
        | Statement::TSNamespaceExportDeclaration(_) => true,
        // Module declarations: a whole-statement type-only `import`/`export
        // *` is erased; the runtime forms are kept (mixed-specifier `type`
        // members bind as type-only imports and are dropped by the
        // value-symbol filter downstream, not here).
        Statement::ImportDeclaration(i) => i.import_kind.is_type(),
        Statement::ExportAllDeclaration(e) => e.export_kind.is_type(),
        Statement::ExportDefaultDeclaration(_) => false,
        Statement::ExportNamedDeclaration(e) => export_named_is_scope_erased(e, delta),
        // Runtime control-flow / expression statements — always kept
        // (recursed for nested erasure / unwrap).
        Statement::BlockStatement(_)
        | Statement::BreakStatement(_)
        | Statement::ContinueStatement(_)
        | Statement::DebuggerStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::EmptyStatement(_)
        | Statement::ExpressionStatement(_)
        | Statement::ForInStatement(_)
        | Statement::ForOfStatement(_)
        | Statement::ForStatement(_)
        | Statement::IfStatement(_)
        | Statement::LabeledStatement(_)
        | Statement::ReturnStatement(_)
        | Statement::SwitchStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::TryStatement(_)
        | Statement::WhileStatement(_)
        | Statement::WithStatement(_) => false,
    }
}

/// Whether an `export … ` named declaration is erased from the scope view. A
/// whole-statement `export type { … }` is erased; an `export <decl>` is
/// erased iff the inner declaration is; an `export { a, type b }` specifier
/// list is kept (the value specifiers survive; `type` specifiers bind as
/// type-only and are dropped by the value-symbol filter downstream).
#[must_use]
pub fn export_named_is_scope_erased(
    export: &ExportNamedDeclaration<'_>,
    delta: ErasureDelta,
) -> bool {
    if export.export_kind.is_type() {
        return true;
    }
    if let Some(declaration) = &export.declaration {
        return declaration_is_scope_erased(declaration, delta);
    }
    false
}

/// Whether a `Declaration` (in statement position or nested under `export`)
/// is erased from the scope view. EXHAUSTIVE over OXC's `Declaration` — NO
/// wildcard — so a new declaration variant breaks the build; mirrors the
/// declaration arms of [`statement_is_scope_erased`].
#[must_use]
pub fn declaration_is_scope_erased(declaration: &Declaration<'_>, delta: ErasureDelta) -> bool {
    match declaration {
        Declaration::VariableDeclaration(d) => d.declare,
        Declaration::FunctionDeclaration(f) => f.declare || f.body.is_none(),
        Declaration::ClassDeclaration(c) => c.declare,
        Declaration::TSEnumDeclaration(e) => delta.enum_always_erased || e.declare,
        Declaration::TSImportEqualsDeclaration(i) => {
            delta.import_equals_always_erased || i.import_kind.is_type()
        }
        Declaration::TSModuleDeclaration(m) => delta.namespace_always_erased || m.declare,
        Declaration::TSTypeAliasDeclaration(_)
        | Declaration::TSInterfaceDeclaration(_)
        | Declaration::TSGlobalDeclaration(_) => true,
    }
}

/// Whether a class member is ERASED from the runtime-survival scope view.
/// EXHAUSTIVE over OXC's `ClassElement` — and, at every TS-carrier sub-enum
/// (`MethodDefinitionType`, `PropertyDefinitionType`, `AccessorPropertyType`),
/// an EXPLICIT match rather than a `matches!` soft-wildcard, so a NEW OXC
/// member OR member-subtype variant breaks the build.
#[must_use]
pub fn class_element_is_scope_erased(element: &ClassElement<'_>) -> bool {
    match element {
        // Runtime static initializer block — KEEP (binds locals).
        ClassElement::StaticBlock(_) => false,
        // An abstract method is erased whole; a normal/constructor method is
        // KEPT and recursed.
        ClassElement::MethodDefinition(method) => match method.r#type {
            MethodDefinitionType::TSAbstractMethodDefinition => true,
            MethodDefinitionType::MethodDefinition => false,
        },
        // A `declare` field is dropped (its computed key / initializer / type
        // ref never visited). An abstract (non-declare) field and a normal
        // field are KEPT: their computed keys are real references, and the
        // value-position filter drops their type refs downstream.
        ClassElement::PropertyDefinition(property) => match property.r#type {
            PropertyDefinitionType::PropertyDefinition
            | PropertyDefinitionType::TSAbstractPropertyDefinition => property.declare,
        },
        // An `accessor` field (either subtype) is dropped for the scope
        // view.
        ClassElement::AccessorProperty(accessor) => match accessor.r#type {
            AccessorPropertyType::AccessorProperty
            | AccessorPropertyType::TSAbstractAccessorProperty => true,
        },
        // A type-only index signature (`[key: string]: T`) binds nothing —
        // ERASE.
        ClassElement::TSIndexSignature(_) => true,
    }
}

/// Whether a formal parameter is a ctor param-property to DROP from the
/// scope view. A `public`/`private`/`protected`/`readonly` modifier makes it
/// a param-property; a plain parameter has neither and stays bound. The
/// `accessibility` decision is an EXHAUSTIVE match over `TSAccessibility` (no
/// soft `is_some()`) so a future accessibility variant forces
/// reclassification instead of silently dropping the parameter.
#[must_use]
pub fn formal_parameter_is_scope_erased(param: &oxc_ast::ast::FormalParameter<'_>) -> bool {
    param.readonly
        || match param.accessibility {
            None => false,
            Some(
                TSAccessibility::Public | TSAccessibility::Private | TSAccessibility::Protected,
            ) => true,
        }
}

/// The shared runtime-survival scope-view projection: an in-place,
/// single-arena rewrite of a reparsed program that erases every construct
/// [`statement_is_scope_erased`] classifies as scope-erased (per `delta`),
/// and UNWRAPS the five TS expression carriers to their inner runtime
/// expression. Every other node is kept and recursed, so nested erasure /
/// unwrap (inside blocks, function bodies, initializers) is handled. It NEVER
/// re-parses and allocates only in the borrowed arena.
pub struct RuntimeSurvivalProjection<'a> {
    pub ast: AstBuilder<'a>,
    pub delta: ErasureDelta,
}

impl<'a> RuntimeSurvivalProjection<'a> {
    #[must_use]
    pub fn new(alloc: &'a Allocator, delta: ErasureDelta) -> Self {
        Self {
            ast: AstBuilder::new(alloc),
            delta,
        }
    }
}

impl<'a> VisitMut<'a> for RuntimeSurvivalProjection<'a> {
    fn visit_statement(&mut self, stmt: &mut Statement<'a>) {
        if statement_is_scope_erased(stmt, self.delta) {
            // Erased: replace with an empty statement (no binding), do NOT
            // recurse — an erased declaration contributes nothing to the
            // scope view.
            *stmt = self.ast.statement_empty(oxc_span::GetSpan::span(stmt));
            return;
        }
        walk_mut::walk_statement(self, stmt);
    }

    fn visit_expression(&mut self, expr: &mut Expression<'a>) {
        // EXHAUSTIVE over `Expression` — the expression-level drift rail.
        // The five TS wrapper expressions UNWRAP to their inner runtime
        // expression; every other expression is KEPT and recursed. NO
        // wildcard for a TS node kind — a new OXC expression variant breaks
        // the build.
        let unwrapped: Option<Expression<'a>> = match expr {
            Expression::TSAsExpression(e) => Some(e.expression.take_in(self.ast.allocator)),
            Expression::TSSatisfiesExpression(e) => Some(e.expression.take_in(self.ast.allocator)),
            Expression::TSTypeAssertion(e) => Some(e.expression.take_in(self.ast.allocator)),
            Expression::TSNonNullExpression(e) => Some(e.expression.take_in(self.ast.allocator)),
            Expression::TSInstantiationExpression(e) => {
                Some(e.expression.take_in(self.ast.allocator))
            }
            Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::TemplateLiteral(_)
            | Expression::Identifier(_)
            | Expression::MetaProperty(_)
            | Expression::Super(_)
            | Expression::ArrayExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::AssignmentExpression(_)
            | Expression::AwaitExpression(_)
            | Expression::BinaryExpression(_)
            | Expression::CallExpression(_)
            | Expression::ChainExpression(_)
            | Expression::ClassExpression(_)
            | Expression::ConditionalExpression(_)
            | Expression::FunctionExpression(_)
            | Expression::ImportExpression(_)
            | Expression::LogicalExpression(_)
            | Expression::NewExpression(_)
            | Expression::ObjectExpression(_)
            | Expression::ParenthesizedExpression(_)
            | Expression::SequenceExpression(_)
            | Expression::TaggedTemplateExpression(_)
            | Expression::ThisExpression(_)
            | Expression::UnaryExpression(_)
            | Expression::UpdateExpression(_)
            | Expression::YieldExpression(_)
            | Expression::PrivateInExpression(_)
            | Expression::JSXElement(_)
            | Expression::JSXFragment(_)
            | Expression::V8IntrinsicExpression(_)
            | match_member_expression!(Expression) => None,
        };
        match unwrapped {
            Some(inner) => {
                *expr = inner;
                // The unwrapped inner may itself be a wrapper (`x as A as
                // B`) or hold further TS to erase / unwrap — re-visit it.
                self.visit_expression(expr);
            }
            None => walk_mut::walk_expression(self, expr),
        }
    }

    fn visit_class_body(&mut self, body: &mut ClassBody<'a>) {
        // ERASE the TS class members BEFORE binding, so OXC never binds an
        // abstract-method parameter, visits a `declare` field's computed
        // key, or binds a `declare`/type-only member. A physical removal
        // (there is no empty class element) — kept members are then
        // recursed by the walk below.
        body.body
            .retain(|element| !class_element_is_scope_erased(element));
        walk_mut::walk_class_body(self, body);
    }

    fn visit_formal_parameters(&mut self, params: &mut FormalParameters<'a>) {
        // Drop ctor param-properties so their name is not bound. A plain
        // param (no modifier) is untouched and stays bound.
        params
            .items
            .retain(|param| !formal_parameter_is_scope_erased(param));
        walk_mut::walk_formal_parameters(self, params);
    }
}
