//! Classic Options API and combined-script projection facts.
//!
//! Three named products:
//!
//! - [`OptionsComponentProjection`]: syntax facts for one normal `<script>`
//!   Options block (props, data/computed/methods members, emits, component
//!   and directive registrations, mixins/extends, setup passthrough). Member
//!   extraction is syntax-only over the admitted block; contextual typing
//!   stays with TypeScript (an Options object keeps its `defineComponent`
//!   context, never a native re-evaluation).
//! - [`CombinedScriptProjection`]: one legal normal script plus one setup
//!   block. Module scope and setup statements delegate to
//!   [`project_script_pair`](super::script_setup::project_script_pair) (the
//!   single macro/grammar authority); the Options half additionally admits
//!   the JavaScript dialect per the STP12 policy, where the TS setup half
//!   cannot run.
//! - [`OptionsTemplateBindingView`]: the template-visible binding inventory
//!   derived from the combined facts. Named module exports never leak into
//!   template scope; opaque runtime sources (`data()`, options `setup()`,
//!   mixins/extends) are recorded as opaque, never invented member lists.
//!
//! The generated default export stays constructor-shaped
//! (`constructor_shaped` is unconditionally true): this projection never
//! substitutes a callable SFC replacement, and `InstanceType<typeof Comp>`
//! preservation is owned by TypeScript, observed by the STP13 probes.
//!
//! Dormant relative to the live IDE route: Vue IDE routing stays on
//! [`super::super::script`] until STP58 atomic activation. Qualification
//! harnesses reach this through
//! [`VueProjectionBackend::options_projection`](crate::framework_common::vue_projection_backend::VueProjectionBackend::options_projection).

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingPattern, Declaration, Expression, ImportDeclarationSpecifier, ImportSpecifier,
    ModuleExportName, ObjectExpression, ObjectPropertyKind, Program, PropertyKey, Statement,
};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::cursor::ScriptLanguage;

use super::script_setup::{
    project_script_pair, value_bindings, ScriptBlockInput, ScriptProjectionFacts,
    SetupProjectionRefusal, SetupStatementKind, SourceRange, MACRO_NAMES,
};

/// Grammar one Options block is parsed under. A single parse under the
/// block's own grammar; no second parse repairs another grammar's syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsDialect {
    /// `lang="ts"`.
    TypeScript,
    /// `lang="tsx"`.
    Tsx,
    /// `lang="js"`/`"jsx"`, absent `lang` (Vue's JavaScript default), or an
    /// unknown `lang`. Member extraction is syntax, so it still runs;
    /// contextual typing follows the admitted JS policy (STP12).
    JavaScript,
}

/// Classification of one statically known Options member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsMemberKind {
    /// `props` entry (array string or object key).
    Prop,
    /// `computed` entry; `setter` is true when a `set` accompanies the getter.
    Computed {
        /// Whether the computed declares a setter.
        setter: bool,
    },
    /// `methods` entry.
    Method,
    /// `emits` entry (array string or object key).
    Emit,
    /// `components` registration key.
    Component,
    /// `directives` registration key.
    Directive,
}

/// One statically known Options member with its carrier range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsMember {
    /// Member name.
    pub name: String,
    /// Member classification.
    pub kind: OptionsMemberKind,
    /// Name span in carrier coordinates.
    pub range: SourceRange,
}

/// Projection facts for one normal `<script>` Options block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsComponentProjection {
    /// Grammar the block was parsed under.
    pub dialect: OptionsDialect,
    /// Statically known members in authored order.
    pub members: Vec<OptionsMember>,
    /// `data()` function present (returned keys are runtime-known only).
    pub has_data_fn: bool,
    /// Options-level `setup()` present (returned bindings are runtime-known only).
    pub has_setup_fn: bool,
    /// `name:` option string, when statically known.
    pub component_name: Option<String>,
    /// Statically known mixin identifiers.
    pub mixins: Vec<String>,
    /// True when a mixin entry is not a static identifier.
    pub has_nonstatic_mixins: bool,
    /// `extends` expression source slice, when present.
    pub extends_source: Option<String>,
    /// A default export exists (object or wrapped call).
    pub has_default_export: bool,
    /// The default export is a binding-resolved `defineComponent` /
    /// `defineOptions` wrapper (imported from `'vue'` under any alias, or a
    /// free reference). A normal-script value binding of the same name makes
    /// it an ordinary call instead.
    pub define_component_wrapped: bool,
    /// Always true: the projected default export is constructor-shaped, never
    /// a callable Vue SFC replacement.
    pub constructor_shaped: bool,
    /// Runtime named exports of the normal block (`default` excluded).
    pub named_exports: Vec<String>,
    /// Normal-script top-level runtime value bindings, sorted. Drives
    /// macro-shadow reporting; macro resolution itself stays in
    /// `script_setup`.
    pub normal_value_bindings: Vec<String>,
}

/// Combined normal-script plus setup projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombinedScriptProjection {
    /// Options facts for the normal block, when one exists.
    pub options: Option<OptionsComponentProjection>,
    /// Module scope, setup statements and binder from the single
    /// setup authority.
    pub facts: ScriptProjectionFacts,
    /// Union of Options-block and module named exports, in authored order.
    pub named_exports: Vec<String>,
    /// Macro names shadowed by a normal-script runtime binding. A shadowed
    /// name is an ordinary call at its use site (observed by the absence of
    /// that macro in `facts.setup`), never a compiler macro.
    pub shadowed_macros: Vec<&'static str>,
}

/// Template-visible binding classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsBindingKind {
    /// Options `props` member.
    Prop,
    /// Options `computed` member.
    Computed,
    /// Options `methods` member.
    Method,
    /// `components` registration.
    Component,
    /// `directives` registration.
    Directive,
    /// Setup top-level value declaration.
    SetupLocal,
}

/// One template-visible binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateBinding {
    /// Binding name.
    pub name: String,
    /// Binding classification.
    pub kind: OptionsBindingKind,
}

/// Template-visible inventory for a combined projection. Named module
/// exports are deliberately absent: they coexist with setup-local members
/// without leaking into template scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsTemplateBindingView {
    /// Template-visible bindings.
    pub bindings: Vec<TemplateBinding>,
    /// Runtime sources whose members are not statically known
    /// (`data()`, options `setup()`, `mixins`, `extends`).
    pub opaque_sources: Vec<String>,
}

impl OptionsTemplateBindingView {
    /// Derive the template view from combined facts.
    #[must_use]
    pub fn build(combined: &CombinedScriptProjection) -> Self {
        let mut bindings = Vec::new();
        let mut opaque_sources = Vec::new();
        if let Some(setup) = &combined.facts.setup {
            for statement in &setup.statements {
                match &statement.kind {
                    SetupStatementKind::Declaration { names }
                    | SetupStatementKind::Import { names } => {
                        for name in names {
                            bindings.push(TemplateBinding {
                                name: name.clone(),
                                kind: OptionsBindingKind::SetupLocal,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(options) = &combined.options {
            for member in &options.members {
                // `emits` names component events, not template identifiers.
                let kind = match member.kind {
                    OptionsMemberKind::Prop => OptionsBindingKind::Prop,
                    OptionsMemberKind::Computed { .. } => OptionsBindingKind::Computed,
                    OptionsMemberKind::Method => OptionsBindingKind::Method,
                    OptionsMemberKind::Emit => continue,
                    OptionsMemberKind::Component => OptionsBindingKind::Component,
                    OptionsMemberKind::Directive => OptionsBindingKind::Directive,
                };
                bindings.push(TemplateBinding {
                    name: member.name.clone(),
                    kind,
                });
            }
            if options.has_data_fn {
                opaque_sources.push("data()".to_string());
            }
            if options.has_setup_fn {
                opaque_sources.push("setup()".to_string());
            }
            if !options.mixins.is_empty() || options.has_nonstatic_mixins {
                opaque_sources.push("mixins".to_string());
            }
            if options.extends_source.is_some() {
                opaque_sources.push("extends".to_string());
            }
        }
        Self {
            bindings,
            opaque_sources,
        }
    }

    /// Look up one template-visible binding by name.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&OptionsBindingKind> {
        self.bindings
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| &binding.kind)
    }

    /// True exactly when `name` resolves in template scope. Named module
    /// exports return false: coexistence without leakage.
    #[must_use]
    pub fn is_template_visible(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }
}

fn source_type_of(lang: Option<ScriptLanguage>) -> (SourceType, OptionsDialect) {
    match lang {
        Some(ScriptLanguage::TypeScript) => (
            SourceType::ts().with_module(true),
            OptionsDialect::TypeScript,
        ),
        Some(ScriptLanguage::TSX) => (SourceType::tsx().with_module(true), OptionsDialect::Tsx),
        _ => (
            SourceType::jsx().with_module(true),
            OptionsDialect::JavaScript,
        ),
    }
}

fn range(base: u32, start: u32, end: u32) -> SourceRange {
    SourceRange {
        start: base + start,
        end: base + end,
    }
}

/// Exported name a runtime (non-type-only) `'vue'` import specifier binds.
fn imported_name<'a>(spec: &'a ImportSpecifier<'a>) -> &'a str {
    match &spec.imported {
        ModuleExportName::IdentifierName(name) => name.name.as_str(),
        ModuleExportName::IdentifierReference(name) => name.name.as_str(),
        ModuleExportName::StringLiteral(literal) => literal.value.as_str(),
    }
}

/// Runtime (non-type-only) `'vue'` imports as local name to exported symbol.
/// Only specifiers whose exported symbol is actually imported are recorded,
/// so `import { ref as defineComponent } from 'vue'` never resolves as the
/// Options wrapper.
fn vue_runtime_imports<'a>(program: &'a Program<'a>) -> FxHashMap<&'a str, &'a str> {
    let mut names = FxHashMap::default();
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else {
            continue;
        };
        if import.import_kind.is_type() || import.source.value != "vue" {
            continue;
        }
        let Some(specifiers) = &import.specifiers else {
            continue;
        };
        for specifier in specifiers {
            if let ImportDeclarationSpecifier::ImportSpecifier(spec) = specifier {
                if spec.import_kind.is_type() {
                    continue;
                }
                names.insert(spec.local.name.as_str(), imported_name(spec));
            }
        }
    }
    names
}

/// True when `name` at call position is the Vue wrapper: a runtime import
/// from `'vue'` of `defineComponent` / `defineOptions` (under any local
/// alias), or a free reference to one of those names. A normal-script value
/// binding of the same name is an ordinary call. Resolution, not spelling.
fn is_vue_wrapper(
    name: &str,
    values: &FxHashSet<String>,
    vue_imports: &FxHashMap<&str, &str>,
) -> bool {
    if let Some(imported) = vue_imports.get(name) {
        return *imported == "defineComponent" || *imported == "defineOptions";
    }
    (name == "defineComponent" || name == "defineOptions") && !values.contains(name)
}

/// Unwrap `defineComponent(obj)` / `defineOptions(obj)` to the options
/// object; any other expression is returned as-is when it is already an
/// object, otherwise `None`.
fn options_object<'a>(
    expression: &'a Expression<'a>,
    values: &FxHashSet<String>,
    vue_imports: &FxHashMap<&str, &str>,
) -> (Option<&'a ObjectExpression<'a>>, bool) {
    match expression {
        Expression::ObjectExpression(object) => (Some(object), false),
        Expression::CallExpression(call) => {
            let name = match &call.callee {
                Expression::Identifier(identifier) => identifier.name.as_str(),
                _ => return (None, false),
            };
            if !is_vue_wrapper(name, values, vue_imports) {
                return (None, false);
            }
            let first = call.arguments.first().and_then(|argument| {
                if let Argument::SpreadElement(_) = argument {
                    None
                } else {
                    argument.as_expression()
                }
            });
            match first {
                Some(Expression::ObjectExpression(object)) => (Some(object), true),
                _ => (None, false),
            }
        }
        _ => (None, false),
    }
}

fn key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.to_string()),
        PropertyKey::StringLiteral(literal) => Some(literal.value.to_string()),
        _ => None,
    }
}

/// True for an explicit `null` or `undefined` option value, which composes
/// no runtime state.
fn is_null_or_undefined(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::NullLiteral(_) => true,
        Expression::Identifier(identifier) => identifier.name == "undefined",
        _ => false,
    }
}

/// Carrier offsets of an option property key. A string-literal span covers
/// its enclosing quotes; the member range covers the name itself (quotes
/// are one-byte ASCII), matching `string_array_names`.
fn key_range(key: &PropertyKey<'_>) -> (u32, u32) {
    let span = key.span();
    if matches!(key, PropertyKey::StringLiteral(_)) {
        (span.start + 1, span.end - 1)
    } else {
        (span.start, span.end)
    }
}

fn string_array_names(expression: &Expression<'_>) -> Vec<(String, u32, u32)> {
    let Expression::ArrayExpression(array) = expression else {
        return Vec::new();
    };
    array
        .elements
        .iter()
        .filter_map(|element| {
            let literal = element.as_expression()?;
            if let Expression::StringLiteral(string) = literal {
                // The literal span includes its quotes; the member range
                // covers the name itself (quotes are one-byte ASCII).
                Some((
                    string.value.to_string(),
                    string.span.start + 1,
                    string.span.end - 1,
                ))
            } else {
                None
            }
        })
        .collect()
}

/// Collect `(name, kind, start, end)` for the keys of an object expression.
fn object_entries(
    expression: &Expression<'_>,
    kind: OptionsMemberKind,
) -> Vec<(String, OptionsMemberKind, u32, u32)> {
    let Expression::ObjectExpression(object) = expression else {
        return Vec::new();
    };
    object
        .properties
        .iter()
        .filter_map(|property| {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                return None;
            };
            let name = key_name(&property.key)?;
            let (start, end) = key_range(&property.key);
            Some((name, kind, start, end))
        })
        .collect()
}

/// Project one normal `<script>` Options block. Any dialect is admitted;
/// syntax errors refuse before any fact is produced.
pub fn project_options_block(
    block: ScriptBlockInput<'_>,
) -> Result<OptionsComponentProjection, SetupProjectionRefusal> {
    let (source_type, dialect) = source_type_of(block.lang);
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, block.content, source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(SetupProjectionRefusal::SyntaxErrors { setup: false });
    }
    let program = &parsed.program;
    let semantic = SemanticBuilder::new().build(program).semantic;
    let values = value_bindings(semantic.scoping());
    let vue_imports = vue_runtime_imports(program);

    let mut projection = OptionsComponentProjection {
        dialect,
        members: Vec::new(),
        has_data_fn: false,
        has_setup_fn: false,
        component_name: None,
        mixins: Vec::new(),
        has_nonstatic_mixins: false,
        extends_source: None,
        has_default_export: false,
        define_component_wrapped: false,
        constructor_shaped: true,
        named_exports: Vec::new(),
        normal_value_bindings: {
            let mut names: Vec<String> = values.iter().cloned().collect();
            names.sort();
            names
        },
    };

    collect_named_exports(program, &mut projection.named_exports);

    for statement in &program.body {
        let Statement::ExportDefaultDeclaration(export) = statement else {
            continue;
        };
        let Some(expression) = export.declaration.as_expression() else {
            continue;
        };
        projection.has_default_export = true;
        let (object, wrapped) = options_object(expression, &values, &vue_imports);
        projection.define_component_wrapped = wrapped;
        let Some(object) = object else { continue };
        collect_option_members(block, object, &mut projection);
    }
    Ok(projection)
}

#[allow(clippy::too_many_lines)]
fn collect_option_members(
    block: ScriptBlockInput<'_>,
    object: &ObjectExpression<'_>,
    projection: &mut OptionsComponentProjection,
) {
    let base = block.content_start;
    let mut push = |name: String, kind: OptionsMemberKind, start: u32, end: u32| {
        projection.members.push(OptionsMember {
            name,
            kind,
            range: range(base, start, end),
        });
    };
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            continue;
        };
        let Some(key) = key_name(&property.key) else {
            continue;
        };
        match key.as_str() {
            "props" => {
                for (name, start, end) in string_array_names(&property.value) {
                    push(name, OptionsMemberKind::Prop, start, end);
                }
                for (name, kind, start, end) in
                    object_entries(&property.value, OptionsMemberKind::Prop)
                {
                    push(name, kind, start, end);
                }
            }
            "computed" => {
                let Expression::ObjectExpression(computed) = &property.value else {
                    continue;
                };
                for entry in &computed.properties {
                    let ObjectPropertyKind::ObjectProperty(entry) = entry else {
                        continue;
                    };
                    let Some(name) = key_name(&entry.key) else {
                        continue;
                    };
                    let setter = match &entry.value {
                        Expression::ObjectExpression(accessors) => {
                            accessors.properties.iter().any(|accessor| {
                                matches!(
                                    accessor,
                                    ObjectPropertyKind::ObjectProperty(accessor)
                                        if key_name(&accessor.key).as_deref() == Some("set")
                                )
                            })
                        }
                        _ => false,
                    };
                    let (start, end) = key_range(&entry.key);
                    push(name, OptionsMemberKind::Computed { setter }, start, end);
                }
            }
            "methods" => {
                for (name, kind, start, end) in
                    object_entries(&property.value, OptionsMemberKind::Method)
                {
                    push(name, kind, start, end);
                }
            }
            "emits" => {
                for (name, start, end) in string_array_names(&property.value) {
                    push(name, OptionsMemberKind::Emit, start, end);
                }
                for (name, kind, start, end) in
                    object_entries(&property.value, OptionsMemberKind::Emit)
                {
                    push(name, kind, start, end);
                }
            }
            "components" => {
                for (name, kind, start, end) in
                    object_entries(&property.value, OptionsMemberKind::Component)
                {
                    push(name, kind, start, end);
                }
            }
            "directives" => {
                for (name, kind, start, end) in
                    object_entries(&property.value, OptionsMemberKind::Directive)
                {
                    push(name, kind, start, end);
                }
            }
            "data" => {
                // Any present value other than an explicit null/undefined
                // composes runtime state the projection cannot enumerate
                // (inline function, identifier reference, call result), so
                // the template view records it as opaque.
                projection.has_data_fn = !is_null_or_undefined(&property.value);
            }
            "setup" => {
                projection.has_setup_fn = !is_null_or_undefined(&property.value);
            }
            "name" => {
                if let Expression::StringLiteral(literal) = &property.value {
                    projection.component_name = Some(literal.value.to_string());
                }
            }
            "mixins" => {
                if let Expression::ArrayExpression(array) = &property.value {
                    for element in &array.elements {
                        if matches!(
                            element,
                            oxc_ast::ast::ArrayExpressionElement::SpreadElement(_)
                        ) {
                            projection.has_nonstatic_mixins = true;
                            continue;
                        }
                        match element.as_expression() {
                            Some(Expression::Identifier(identifier)) => {
                                projection.mixins.push(identifier.name.to_string());
                            }
                            None => {}
                            _ => projection.has_nonstatic_mixins = true,
                        }
                    }
                } else {
                    // A non-array `mixins` value (identifier, call, spread
                    // source) is dynamically composed: its members are not
                    // statically known, so the template view must record it
                    // as opaque rather than silently drop it.
                    projection.has_nonstatic_mixins = true;
                }
            }
            "extends" => {
                let span = property.value.span();
                if let Some(text) = block.content.get(span.start as usize..span.end as usize) {
                    projection.extends_source = Some(text.to_string());
                }
            }
            _ => {}
        }
    }
}

/// Runtime named exports of the normal block (`default` excluded;
/// type-only exports skipped). Mirrors the export half of
/// `project_normal` for blocks the TS setup authority cannot admit.
fn collect_named_exports(program: &Program<'_>, into: &mut Vec<String>) {
    for statement in &program.body {
        match statement {
            Statement::ExportNamedDeclaration(export) => {
                if let Some(declaration) = &export.declaration {
                    if is_value_declaration(declaration) {
                        into.extend(declaration_names(declaration));
                    }
                }
                for specifier in &export.specifiers {
                    if export.export_kind.is_type() || specifier.export_kind.is_type() {
                        continue;
                    }
                    let name = specifier.exported.name().to_string();
                    if name != "default" {
                        into.push(name);
                    }
                }
            }
            Statement::ExportAllDeclaration(export) => {
                if export.export_kind.is_type() {
                    continue;
                }
                if let Some(exported) = &export.exported {
                    let name = exported.name().to_string();
                    if name != "default" {
                        into.push(name);
                    }
                }
            }
            _ => {}
        }
    }
}

fn is_value_declaration(declaration: &Declaration<'_>) -> bool {
    matches!(
        declaration,
        Declaration::VariableDeclaration(_)
            | Declaration::FunctionDeclaration(_)
            | Declaration::ClassDeclaration(_)
    )
}

fn declaration_names(declaration: &Declaration<'_>) -> Vec<String> {
    let mut names = Vec::new();
    let mut pattern_names = |pattern: &BindingPattern<'_>| {
        for identifier in pattern.get_binding_identifiers() {
            names.push(identifier.name.to_string());
        }
    };
    match declaration {
        Declaration::VariableDeclaration(variable) => {
            for declarator in &variable.declarations {
                pattern_names(&declarator.id);
            }
        }
        Declaration::FunctionDeclaration(function) => {
            if let Some(identifier) = &function.id {
                names.push(identifier.name.to_string());
            }
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(identifier) = &class.id {
                names.push(identifier.name.to_string());
            }
        }
        _ => {}
    }
    names
}

/// Project one normal-plus-setup pair. The setup/module half delegates to
/// the single setup authority; a JavaScript normal block additionally
/// contributes Options facts (the TS setup authority cannot admit it, so it
/// is withheld from that delegation rather than re-parsed under a foreign
/// grammar). A pair of explicitly different `lang`s is refused, matching
/// Vue's own `compileScript` throw.
pub fn project_options_pair(
    normal: Option<ScriptBlockInput<'_>>,
    setup: Option<ScriptBlockInput<'_>>,
    generic: Option<&str>,
) -> Result<CombinedScriptProjection, SetupProjectionRefusal> {
    if let (Some(normal_block), Some(setup_block)) = (&normal, &setup) {
        if normal_block.lang != setup_block.lang {
            return Err(SetupProjectionRefusal::ScriptLangConflict);
        }
    }
    let options = normal.map(project_options_block).transpose()?;
    let ts_normal = normal.filter(|block| {
        matches!(
            block.lang,
            Some(ScriptLanguage::TypeScript) | Some(ScriptLanguage::TSX)
        )
    });
    let facts: ScriptProjectionFacts = project_script_pair(ts_normal, setup, generic)?;
    let mut named_exports = Vec::new();
    if let Some(options_projection) = &options {
        named_exports.extend(options_projection.named_exports.iter().cloned());
    }
    for export in &facts.module.named_exports {
        if export != "default" && !named_exports.iter().any(|existing| existing == export) {
            named_exports.push(export.clone());
        }
    }
    let mut shadowed_macros = Vec::new();
    if let Some(options_projection) = &options {
        for macro_name in MACRO_NAMES {
            if options_projection
                .normal_value_bindings
                .iter()
                .any(|bound| bound == macro_name)
            {
                shadowed_macros.push(macro_name);
            }
        }
    }
    Ok(CombinedScriptProjection {
        options,
        facts,
        named_exports,
        shadowed_macros,
    })
}
