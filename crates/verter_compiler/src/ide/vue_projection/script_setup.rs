//! Statement-oriented TypeScript setup and module lowering facts.
//!
//! One OXC parse per authored script block yields three products:
//! [`ModuleScopeProjection`] (imports, normal-script exports and bindings),
//! [`TsSetupProjection`] (ordered setup statements, top-level `await`, macro
//! calls) and [`UniversalSetupBinder`] (the authored `generic` binder, never
//! specialized by a parent use). The grammar follows the block's `lang`
//! (absent `lang` is Vue's own JavaScript default and is refused, matching
//! `sfc_script_dialect`), so a TypeScript angle assertion and an actual TSX
//! element are each parsed once under their own grammar; no second TSX parse
//! repairs TypeScript syntax. A script/script-setup pair with mismatched
//! `lang` is refused rather than silently classified from one block alone.
//!
//! Macros are recognised by resolution, not spelling: a root-level call to
//! `defineProps` (and siblings) is a macro when the callee is a free
//! reference, or when it resolves to a runtime (non-type-only) import of the
//! same name from `'vue'` (Vue strips such an import with a warning and
//! still treats the call as the macro); a normal-script VALUE binding or a
//! setup-local lexical function of the same name is an ordinary call, and a
//! call not in a macro position is never a macro. Vue's macro positions are a
//! root expression statement, a root variable declarator initializer, and the
//! first argument of a root `withDefaults(...)` (each optionally wrapped in
//! parentheses / `as` / `satisfies`).
//!
//! The body of a generic component is checked once, universally: the facts
//! carry exactly one optional [`TsSetupProjection`], so a second copy of the
//! setup body has no place to live.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, AwaitExpression, CallExpression, Class, ClassElement, Declaration,
    Expression, ForOfStatement, Function, ImportDeclarationSpecifier, Program, PropertyKey,
    Statement,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_semantic::{Scoping, SemanticBuilder, SymbolFlags};
use oxc_span::{GetSpan, SourceType};
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashSet;

use crate::cursor::ScriptLanguage;
use crate::utils::oxc::vue::parse_generic;

/// Vue macros recognised at setup scope. Shared with the Options/combined
/// projection for shadow reporting; macro resolution itself stays here.
pub(crate) const MACRO_NAMES: [&str; 7] = [
    "defineProps",
    "defineEmits",
    "defineExpose",
    "defineOptions",
    "defineSlots",
    "defineModel",
    "withDefaults",
];

/// Half-open byte range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    /// Start byte.
    pub start: u32,
    /// End byte (exclusive).
    pub end: u32,
}

/// Grammar one script block is parsed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptGrammar {
    /// `lang="ts"`: `<T>x` is a type assertion.
    TypeScript,
    /// `lang="tsx"`: `<div/>` is JSX.
    Tsx,
}

impl ScriptGrammar {
    pub(super) fn source_type(self) -> SourceType {
        match self {
            Self::TypeScript => SourceType::ts().with_module(true),
            Self::Tsx => SourceType::tsx().with_module(true),
        }
    }
}

/// Script block input; `content_start` is the carrier offset of `content`.
#[derive(Debug, Clone, Copy)]
pub struct ScriptBlockInput<'s> {
    /// Block content.
    pub content: &'s str,
    /// Carrier offset of the first content byte.
    pub content_start: u32,
    /// Authored `lang`; absent (`None`) is Vue's own JavaScript default, not
    /// TypeScript, and is refused via
    /// [`SetupProjectionRefusal::NotTypeScript`].
    pub lang: Option<ScriptLanguage>,
}

/// Refusals raised before any projection is produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupProjectionRefusal {
    /// The block is not TypeScript/TSX; JavaScript owns a separate projection.
    NotTypeScript,
    /// The `<script>` and `<script setup>` blocks declared different `lang`s;
    /// Vue's own `compileScript` throws on this pair rather than picking one.
    ScriptLangConflict,
    /// The block has syntax errors; incomplete-source projection owns recovery.
    SyntaxErrors {
        /// True for the setup block, false for the normal script.
        setup: bool,
    },
    /// The `generic` attribute does not parse as type parameters.
    InvalidGeneric,
    /// No admitted parse (or a parse of different source) backs the request.
    MissingParse,
}

/// Vue macro call resolved as a macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupMacroCall {
    /// Macro name.
    pub name: &'static str,
    /// Call span in the carrier.
    pub span: SourceRange,
}

/// Classification of one top-level setup statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupStatementKind {
    /// `import ...`; hoists to module scope.
    Import {
        /// Imported local names.
        names: Vec<String>,
    },
    /// Type-only declaration (`type`, `interface`); hoists to module scope.
    TypeDeclaration {
        /// Declared type name.
        name: String,
    },
    /// Value declaration; stays inside the setup body.
    Declaration {
        /// Bound names, in source order.
        names: Vec<String>,
    },
    /// Any other statement.
    Other,
}

/// One top-level setup statement in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupStatement {
    /// Statement span in the carrier.
    pub span: SourceRange,
    /// Classification.
    pub kind: SetupStatementKind,
}

/// Import declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleImport {
    /// Statement span in the carrier.
    pub span: SourceRange,
    /// Module specifier.
    pub source: String,
    /// True when the import came from `<script setup>`.
    pub from_setup: bool,
}

/// Module-scope facts shared by both blocks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleScopeProjection {
    /// Imports from both blocks, normal script first.
    pub imports: Vec<ModuleImport>,
    /// Normal-script export names (`default` included).
    pub named_exports: Vec<String>,
    /// Normal-script top-level bindings in source order.
    pub normal_script_bindings: Vec<String>,
}

/// One authored generic parameter; ranges index the `generic` attribute value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniversalBinderParam {
    /// Parameter name.
    pub name: String,
    /// Constraint range.
    pub constraint: Option<SourceRange>,
    /// Default range.
    pub default: Option<SourceRange>,
}

/// The authored generic binder. It carries no type arguments: the body is
/// checked against the declared constraints, never a parent use's arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UniversalSetupBinder {
    /// Parameters in authored order.
    pub params: Vec<UniversalBinderParam>,
}

/// Statement-oriented setup facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsSetupProjection {
    /// Grammar the setup block was parsed under.
    pub grammar: ScriptGrammar,
    /// Top-level statements in source order.
    pub statements: Vec<SetupStatement>,
    /// First top-level `await`; nested functions are excluded.
    pub top_level_await: Option<SourceRange>,
    /// Calls resolved as Vue macros.
    pub macros: Vec<SetupMacroCall>,
}

impl TsSetupProjection {
    /// The checking wrapper is async exactly when setup awaits at top level.
    /// This is a checking-body property only; the exported component
    /// instance type and template callback return domains are unaffected.
    #[must_use]
    pub fn checking_wrapper_is_async(&self) -> bool {
        self.top_level_await.is_some()
    }
}

/// All statement-oriented facts for one SFC script pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptProjectionFacts {
    /// Module scope.
    pub module: ModuleScopeProjection,
    /// Setup statements, when a setup block exists.
    pub setup: Option<TsSetupProjection>,
    /// Authored generic binder.
    pub binder: UniversalSetupBinder,
}

pub(super) fn grammar_of(
    lang: Option<ScriptLanguage>,
) -> Result<ScriptGrammar, SetupProjectionRefusal> {
    match lang {
        Some(ScriptLanguage::TypeScript) => Ok(ScriptGrammar::TypeScript),
        Some(ScriptLanguage::TSX) => Ok(ScriptGrammar::Tsx),
        // Absent `lang` is Vue's own JavaScript default (`sfc_script_dialect`),
        // not TypeScript; JavaScript owns a separate projection.
        None | Some(_) => Err(SetupProjectionRefusal::NotTypeScript),
    }
}

pub(super) fn range(span: oxc_span::Span, base: u32) -> SourceRange {
    SourceRange {
        start: base + span.start,
        end: base + span.end,
    }
}

fn root_bindings(scoping: &Scoping) -> Vec<String> {
    let mut named: Vec<(u32, String)> = scoping
        .get_bindings(scoping.root_scope_id())
        .iter()
        .map(|(name, &id)| (scoping.symbol_span(id).start, name.as_str().to_string()))
        .collect();
    named.sort();
    named.into_iter().map(|(_, name)| name).collect()
}

/// A symbol occupying only the type space (no runtime value), so it cannot
/// shadow a runtime macro call. Classes/enums occupy both spaces and stay
/// eligible to shadow.
const PURE_TYPE_SPACE: SymbolFlags = SymbolFlags::Interface
    .union(SymbolFlags::TypeAlias)
    .union(SymbolFlags::TypeParameter)
    .union(SymbolFlags::TypeImport);

/// Root-scope names bound to a runtime value, for macro-shadow checks. A
/// type-only declaration (`interface`, `type`, `import type`) of the same
/// name as a macro must not suppress the macro. Shared with the
/// Options/combined projection; the rule itself stays here.
pub(crate) fn value_bindings(scoping: &Scoping) -> FxHashSet<String> {
    scoping
        .get_bindings(scoping.root_scope_id())
        .iter()
        .filter(|(_, &id)| {
            let flags = scoping.symbol_flags(id);
            !(flags.intersects(PURE_TYPE_SPACE)
                && !flags.intersects(SymbolFlags::Value | SymbolFlags::Import))
        })
        .map(|(name, _)| name.as_str().to_string())
        .collect()
}

/// Local names imported as a runtime (non-type-only) binding from `'vue'`
/// that shadow a macro name. Vue's `compileScript` strips a
/// `defineProps`/`defineEmits`/... specifier imported from `'vue'` and still
/// processes the call as the macro.
pub(super) fn vue_runtime_macro_imports<'a>(program: &Program<'a>) -> FxHashSet<&'a str> {
    let mut names = FxHashSet::default();
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
                let local = spec.local.name.as_str();
                if MACRO_NAMES.contains(&local) {
                    names.insert(local);
                }
            }
        }
    }
    names
}

/// Project one script pair. `generic` is the `generic` attribute value.
pub fn project_script_pair(
    normal: Option<ScriptBlockInput<'_>>,
    setup: Option<ScriptBlockInput<'_>>,
    generic: Option<&str>,
) -> Result<ScriptProjectionFacts, SetupProjectionRefusal> {
    if let (Some(n), Some(s)) = (&normal, &setup) {
        if n.lang != s.lang {
            return Err(SetupProjectionRefusal::ScriptLangConflict);
        }
    }
    let mut module = ModuleScopeProjection::default();
    let normal_value_bindings = match normal {
        Some(block) => project_normal(block, &mut module)?,
        None => FxHashSet::default(),
    };
    let setup = match setup {
        Some(block) => Some(project_setup(block, &mut module, &normal_value_bindings)?),
        None => None,
    };
    Ok(ScriptProjectionFacts {
        module,
        setup,
        binder: project_binder(generic)?,
    })
}

fn project_binder(generic: Option<&str>) -> Result<UniversalSetupBinder, SetupProjectionRefusal> {
    let Some(original) = generic else {
        return Ok(UniversalSetupBinder::default());
    };
    let text = original.trim();
    if text.is_empty() {
        return Ok(UniversalSetupBinder::default());
    }
    // `parse_generic` returns spans relative to the trimmed `text`; carry the
    // leading-whitespace byte offset so returned ranges stay relative to the
    // authored `generic` attribute value.
    let leading_offset = (text.as_ptr() as usize - original.as_ptr() as usize) as u32;
    let allocator = Allocator::default();
    let result = parse_generic(&allocator, text, 0);
    if !result.is_ok() {
        return Err(SetupProjectionRefusal::InvalidGeneric);
    }
    Ok(binder_product_from(&result, text, leading_offset))
}

/// The authored binder product for an already-parsed `generic` attribute.
/// Sole owner of the parameter-name/constraint/default projection, so a
/// consumer that needs the parsed binder AST as well never re-parses the
/// attribute to rebuild the same rows.
pub(super) fn binder_product_from(
    result: &crate::utils::oxc::vue::GenericParseResult<'_>,
    text: &str,
    leading_offset: u32,
) -> UniversalSetupBinder {
    let bytes = text.as_bytes();
    let span = |s: crate::common::RelativeSpan| SourceRange {
        start: s.start + leading_offset,
        end: s.end + leading_offset,
    };
    UniversalSetupBinder {
        params: result
            .params
            .iter()
            .map(|param| UniversalBinderParam {
                name: String::from_utf8_lossy(param.name(bytes)).into_owned(),
                constraint: param.constraint_span.map(span),
                default: param.default_span.map(span),
            })
            .collect(),
    }
}

fn is_type_only_declaration(declaration: &Declaration<'_>) -> bool {
    matches!(
        declaration,
        Declaration::TSTypeAliasDeclaration(_) | Declaration::TSInterfaceDeclaration(_)
    )
}

fn project_normal(
    block: ScriptBlockInput<'_>,
    module: &mut ModuleScopeProjection,
) -> Result<FxHashSet<String>, SetupProjectionRefusal> {
    let grammar = grammar_of(block.lang)?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, block.content, grammar.source_type()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(SetupProjectionRefusal::SyntaxErrors { setup: false });
    }
    collect_imports(&parsed.program, block.content_start, false, module);
    for statement in &parsed.program.body {
        match statement {
            Statement::ExportNamedDeclaration(export) => {
                if let Some(declaration) = &export.declaration {
                    if !is_type_only_declaration(declaration) {
                        module.named_exports.extend(declaration_names(declaration));
                    }
                }
                for specifier in &export.specifiers {
                    if export.export_kind.is_type() || specifier.export_kind.is_type() {
                        continue;
                    }
                    module
                        .named_exports
                        .push(specifier.exported.name().to_string());
                }
            }
            Statement::ExportDefaultDeclaration(_) => {
                module.named_exports.push("default".to_string());
            }
            Statement::ExportAllDeclaration(export) => {
                // `export * from './z'` re-exports a name set unknown at
                // this file's own scope; only the namespaced form
                // (`export * as ns from './z'`) has a locally observable
                // name.
                if export.export_kind.is_type() {
                    continue;
                }
                if let Some(exported) = &export.exported {
                    module.named_exports.push(exported.name().to_string());
                }
            }
            _ => {}
        }
    }
    let semantic = SemanticBuilder::new().build(&parsed.program).semantic;
    let scoping = semantic.scoping();
    module.normal_script_bindings = root_bindings(scoping);
    Ok(value_bindings(scoping))
}

fn collect_imports(
    program: &Program<'_>,
    base: u32,
    from_setup: bool,
    module: &mut ModuleScopeProjection,
) {
    for statement in &program.body {
        if let Statement::ImportDeclaration(import) = statement {
            module.imports.push(ModuleImport {
                span: range(import.span, base),
                source: import.source.value.to_string(),
                from_setup,
            });
        }
    }
}

fn project_setup(
    block: ScriptBlockInput<'_>,
    module: &mut ModuleScopeProjection,
    normal_value_bindings: &FxHashSet<String>,
) -> Result<TsSetupProjection, SetupProjectionRefusal> {
    let grammar = grammar_of(block.lang)?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, block.content, grammar.source_type()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(SetupProjectionRefusal::SyntaxErrors { setup: true });
    }
    let program = &parsed.program;
    collect_imports(program, block.content_start, true, module);

    let semantic = SemanticBuilder::new().build(program).semantic;
    let module_bound: FxHashSet<&str> = normal_value_bindings.iter().map(String::as_str).collect();
    let vue_macro_imports = vue_runtime_macro_imports(program);
    let mut collector = SetupCollector {
        macros_ctx: MacroContext {
            scoping: semantic.scoping(),
            module_bound: &module_bound,
            vue_macro_imports: &vue_macro_imports,
            macro_positions: macro_positions(program),
        },
        base: block.content_start,
        depth: 0,
        top_level_await: None,
        macros: Vec::new(),
    };
    collector.visit_program(program);

    let statements = program
        .body
        .iter()
        .map(|statement| SetupStatement {
            span: range(statement.span(), block.content_start),
            kind: classify(statement),
        })
        .collect();
    Ok(TsSetupProjection {
        grammar,
        statements,
        top_level_await: collector.top_level_await,
        macros: collector.macros,
    })
}

fn classify(statement: &Statement<'_>) -> SetupStatementKind {
    match statement {
        Statement::ImportDeclaration(import) => SetupStatementKind::Import {
            names: import
                .specifiers
                .as_ref()
                .map(|specifiers| {
                    specifiers
                        .iter()
                        .map(|s| s.local().name.to_string())
                        .collect()
                })
                .unwrap_or_default(),
        },
        Statement::TSTypeAliasDeclaration(decl) => SetupStatementKind::TypeDeclaration {
            name: decl.id.name.to_string(),
        },
        Statement::TSInterfaceDeclaration(decl) => SetupStatementKind::TypeDeclaration {
            name: decl.id.name.to_string(),
        },
        _ => match statement.as_declaration() {
            Some(declaration) => SetupStatementKind::Declaration {
                names: declaration_names(declaration),
            },
            None => SetupStatementKind::Other,
        },
    }
}

/// Macro-resolution inputs for one parsed setup block. Macro recognition has
/// exactly one owner: both the statement-oriented setup projection and the
/// public-dependency capture answer "is this call a Vue macro?" here, so the
/// two products can never drift into two spellings of the same rule.
pub(super) struct MacroContext<'s> {
    /// Setup-block scoping, for free-reference resolution.
    pub(super) scoping: &'s Scoping,
    /// Root-scope value names from the normal script.
    pub(super) module_bound: &'s FxHashSet<&'s str>,
    /// Macro names imported as runtime bindings from `'vue'`.
    pub(super) vue_macro_imports: &'s FxHashSet<&'s str>,
    /// Start offsets of calls in a legal macro position.
    pub(super) macro_positions: FxHashSet<u32>,
}

impl MacroContext<'_> {
    /// The macro this call resolves to, or `None` for an ordinary call.
    /// `depth` is the count of enclosing functions/classes: a call is only a
    /// macro at setup root.
    pub(super) fn resolve(&self, call: &CallExpression<'_>, depth: u32) -> Option<&'static str> {
        if depth != 0 || !self.macro_positions.contains(&call.span.start) {
            return None;
        }
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        let name = callee.name.as_str();
        if self.module_bound.contains(name) {
            return None;
        }
        let macro_name = *MACRO_NAMES.iter().find(|m| **m == name)?;
        let free = self
            .scoping
            .get_reference(callee.reference_id())
            .symbol_id()
            .is_none();
        (free || self.vue_macro_imports.contains(name)).then_some(macro_name)
    }
}

struct SetupCollector<'s> {
    macros_ctx: MacroContext<'s>,
    base: u32,
    depth: u32,
    top_level_await: Option<SourceRange>,
    macros: Vec<SetupMacroCall>,
}

impl SetupCollector<'_> {
    fn note_await(&mut self, span: oxc_span::Span) {
        if self.depth == 0 && self.top_level_await.is_none() {
            self.top_level_await = Some(range(span, self.base));
        }
    }
}

/// Finds an `await` reachable from a class computed key without crossing
/// into a nested function/arrow/class boundary — the key is evaluated
/// eagerly in the enclosing scope, unlike a method or property body.
struct ComputedKeyAwaitVisitor {
    found: Option<oxc_span::Span>,
}

impl<'a> Visit<'a> for ComputedKeyAwaitVisitor {
    fn visit_await_expression(&mut self, it: &AwaitExpression<'a>) {
        if self.found.is_none() {
            self.found = Some(it.span);
        }
        walk::walk_await_expression(self, it);
    }

    fn visit_function(&mut self, _it: &Function<'a>, _flags: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, it: &Class<'a>) {
        // A nested class's heritage and computed keys are themselves
        // evaluated eagerly in the enclosing scope; only its method/property
        // bodies are deferred, so recurse into those two eager parts without
        // walking the rest of the nested class.
        if self.found.is_some() {
            return;
        }
        if let Some(super_class) = &it.super_class {
            self.visit_expression(super_class);
        }
        for element in &it.body.body {
            if let Some(key) = computed_class_key(element) {
                self.visit_property_key(key);
            }
        }
    }
}

/// The element's key when authored with computed-key syntax (`[expr]`); such
/// a key is evaluated eagerly, in the enclosing scope.
fn computed_class_key<'e, 'a>(element: &'e ClassElement<'a>) -> Option<&'e PropertyKey<'a>> {
    let (key, computed) = match element {
        ClassElement::MethodDefinition(m) => (&m.key, m.computed),
        ClassElement::PropertyDefinition(p) => (&p.key, p.computed),
        ClassElement::AccessorProperty(a) => (&a.key, a.computed),
        ClassElement::StaticBlock(_) | ClassElement::TSIndexSignature(_) => return None,
    };
    computed.then_some(key)
}

impl<'a> Visit<'a> for SetupCollector<'_> {
    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        self.depth += 1;
        walk::walk_function(self, it, flags);
        self.depth -= 1;
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.depth += 1;
        walk::walk_arrow_function_expression(self, it);
        self.depth -= 1;
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        if self.depth == 0 {
            // Heritage is evaluated eagerly in the enclosing scope.
            if let Some(super_class) = &it.super_class {
                let mut probe = ComputedKeyAwaitVisitor { found: None };
                probe.visit_expression(super_class);
                if let Some(span) = probe.found {
                    self.note_await(span);
                }
            }
            for element in &it.body.body {
                if let Some(key) = computed_class_key(element) {
                    let mut probe = ComputedKeyAwaitVisitor { found: None };
                    probe.visit_property_key(key);
                    if let Some(span) = probe.found {
                        self.note_await(span);
                    }
                }
            }
        }
        self.depth += 1;
        walk::walk_class(self, it);
        self.depth -= 1;
    }

    fn visit_await_expression(&mut self, it: &AwaitExpression<'a>) {
        self.note_await(it.span);
        walk::walk_await_expression(self, it);
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        if it.r#await {
            self.note_await(it.span);
        }
        walk::walk_for_of_statement(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Some(macro_name) = self.macros_ctx.resolve(it, self.depth) {
            self.macros.push(SetupMacroCall {
                name: macro_name,
                span: range(it.span, self.base),
            });
        }
        walk::walk_call_expression(self, it);
    }
}

fn unwrap_expression<'e, 'a>(mut expr: &'e Expression<'a>) -> &'e Expression<'a> {
    loop {
        expr = match expr {
            Expression::ParenthesizedExpression(e) => &e.expression,
            Expression::TSAsExpression(e) => &e.expression,
            Expression::TSSatisfiesExpression(e) => &e.expression,
            _ => return expr,
        };
    }
}

fn note_macro_position(expr: &Expression<'_>, out: &mut FxHashSet<u32>) {
    if let Expression::CallExpression(call) = unwrap_expression(expr) {
        out.insert(call.span.start);
        if let Expression::Identifier(callee) = &call.callee {
            if callee.name == "withDefaults" {
                if let Some(first) = call.arguments.first().and_then(|a| a.as_expression()) {
                    note_macro_position(first, out);
                }
            }
        }
    }
}

/// Calls in a position Vue processes as a macro: root expression statements,
/// root declarator initializers, and `withDefaults`' first argument.
pub(super) fn macro_positions(program: &Program<'_>) -> FxHashSet<u32> {
    let mut out = FxHashSet::default();
    for statement in &program.body {
        match statement {
            Statement::ExpressionStatement(s) => note_macro_position(&s.expression, &mut out),
            Statement::VariableDeclaration(vars) => {
                for declarator in &vars.declarations {
                    if let Some(init) = &declarator.init {
                        note_macro_position(init, &mut out);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn declaration_names(declaration: &Declaration<'_>) -> Vec<String> {
    match declaration {
        Declaration::VariableDeclaration(vars) => vars
            .declarations
            .iter()
            .flat_map(|d| d.id.get_binding_identifiers())
            .map(|id| id.name.to_string())
            .collect(),
        Declaration::FunctionDeclaration(f) => f.id.iter().map(|id| id.name.to_string()).collect(),
        Declaration::ClassDeclaration(c) => c.id.iter().map(|id| id.name.to_string()).collect(),
        Declaration::TSEnumDeclaration(e) => vec![e.id.name.to_string()],
        Declaration::TSTypeAliasDeclaration(t) => vec![t.id.name.to_string()],
        Declaration::TSInterfaceDeclaration(t) => vec![t.id.name.to_string()],
        _ => Vec::new(),
    }
}
