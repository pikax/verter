//! Binder-aware public dependency capture and local type lifting.
//!
//! The public type surface of an SFC (the macro type arguments, plus the
//! value surface named by `defineExpose`) references names that the emitted
//! public declaration must be able to name. This module answers *which* names
//! those are, and *what* has to travel with them, by syntax and provenance
//! only: it never evaluates a type, never expands an alias and never decides
//! whether two types are assignable. TypeScript stays the type-answer owner.
//!
//! Three products:
//!
//! - [`PublicTypeDependencySlice`] — one public type expression's free
//!   bindings, each classified by the space it occupies (`T` in a type
//!   position vs `typeof x` in a value position) and by where it resolves
//!   (authored `generic` binder, a script declaration, an import, or nothing
//!   this carrier owns).
//! - [`LiftedSourceDeclaration`] — one script declaration reachable from a
//!   public type, carrying its authored visibility, its origin block, and the
//!   binder parameters that must be re-bound when it is emitted.
//! - [`BinderCapturePlan`] — the closure over those declarations: which are
//!   lifted, which imports are retained, which binder parameters each lifted
//!   declaration needs (constraint/default closure included), the
//!   capture-avoiding rename for each of those parameters, the exact
//!   reference sites a rename applies to, plus the alias cycles and duplicate
//!   declarations the walk observed.
//!
//! Scoping rules the capture follows, and why:
//!
//! - The `generic` binder scopes `<script setup>` only. Inside setup a binder
//!   parameter shadows a module-scope binding of the same name; inside the
//!   normal `<script>` it is not in scope at all.
//! - A lifted declaration is emitted at the module scope of the declaration
//!   file, where binder parameters are *not* in scope. A lifted declaration
//!   free in a binder parameter is therefore re-parameterized over it, and
//!   the introduced parameter is alpha-renamed whenever its authored spelling
//!   is already taken at that module scope — otherwise lifting would capture
//!   an unrelated declaration of the same name.
//! - A parameter pulled in that way brings the parameters its constraint and
//!   default depend on, in authored order, so a later default that refers to
//!   an earlier parameter keeps that binding.
//!
//! Termination is structural: the closure is a worklist over declaration
//! identity, so a recursive alias pair is visited once and recorded as a
//! cycle rather than expanded. Imported names are never followed into another
//! file; they are retained as imports.
//!
//! A name bound twice at one module scope (both script blocks declaring it,
//! or an import colliding with a declaration) is a real authored error. The
//! capture records every origin and lifts nothing for that name, so the
//! emitted declaration neither hides the error nor invents a second
//! conflicting declaration.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, CallExpression, Class, ClassElement, Declaration, Expression,
    Function, ImportDeclarationSpecifier, ObjectPropertyKind, Program, Statement,
    TSCallSignatureDeclaration, TSConstructSignatureDeclaration, TSInferType,
    TSInterfaceDeclaration, TSInterfaceHeritage, TSMethodSignature, TSType, TSTypeName,
    TSTypeOperatorOperator, TSTypeParameterDeclaration, TSTypeQuery, TSTypeQueryExprName,
    TSTypeReference,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::GetSpan;
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::{FxHashMap, FxHashSet};

use super::script_setup::{
    binder_product_from, grammar_of, macro_positions, range, value_bindings,
    vue_runtime_macro_imports, MacroContext, ScriptBlockInput, SetupProjectionRefusal, SourceRange,
    UniversalSetupBinder,
};
use crate::utils::oxc::vue::parse_generic;

/// The space a captured binding occupies at its reference site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DependencySpace {
    /// Referenced as a type (`Foo`, `Foo<T>`, `Foo["k"]`).
    Type,
    /// Referenced through a `typeof` query, so the runtime binding is needed.
    Value,
}

/// Where a captured free binding resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DependencyOrigin {
    /// A parameter of the authored `generic` binder.
    BinderParam {
        /// Authored position in the binder.
        ordinal: usize,
    },
    /// A top-level declaration in one of this carrier's script blocks.
    LocalDeclaration {
        /// Index into [`BinderCapturePlan::declarations`].
        index: usize,
    },
    /// Multiple compatible local declarations, such as merged interfaces.
    LocalDeclarations {
        /// Indices into the source declaration inventory.
        indices: Vec<usize>,
    },
    /// An imported binding, retained as an import rather than lifted.
    Import {
        /// Index into [`BinderCapturePlan::imports`].
        index: usize,
    },
    /// The name is bound more than once at this module scope; see
    /// [`BinderCapturePlan::duplicates`]. Nothing is lifted for it.
    Duplicate,
    /// Not bound by this carrier (global, ambient or `lib`).
    Unresolved,
}

/// One free binding a public type expression depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedDependency {
    /// Authored name at the reference site.
    pub name: String,
    /// Space the reference occupies.
    pub space: DependencySpace,
    /// Where the name resolves.
    pub origin: DependencyOrigin,
    /// First reference site, in carrier bytes.
    pub reference: SourceRange,
}

/// The public surface a dependency slice was captured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PublicTypeRoot {
    /// `defineProps<T>()`.
    Props,
    /// `defineEmits<T>()`.
    Emits,
    /// `defineSlots<T>()`.
    Slots,
    /// `defineModel<T>()`.
    Model,
    /// The object argument of `defineExpose({ ... })`.
    Expose,
}

/// Free bindings of one public type expression, in first-reference order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicTypeDependencySlice {
    /// Surface this slice was captured from.
    pub root: PublicTypeRoot,
    /// Macro call span in the carrier.
    pub macro_call: SourceRange,
    /// The captured expression: the type argument, or the exposed object.
    pub expression: SourceRange,
    /// Free bindings, de-duplicated by `(name, space)`.
    pub dependencies: Vec<CapturedDependency>,
}

/// Declaration form of a lifted source declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LiftedDeclarationKind {
    /// `type X = ...`.
    TypeAlias,
    /// `interface X { ... }`.
    Interface,
    /// `enum X { ... }`.
    Enum,
    /// `class X { ... }`; occupies both spaces.
    Class,
    /// `function x() { ... }`; only the signature is public.
    Function,
    /// A variable binding; only its annotation is public.
    Variable {
        /// The annotation is a `unique symbol`, so the emitted declaration
        /// must name this very declaration rather than re-spell the type.
        unique_symbol: bool,
    },
}

impl LiftedDeclarationKind {
    /// Whether a `typeof` query against this declaration is meaningful.
    #[must_use]
    pub fn occupies_value_space(self) -> bool {
        matches!(
            self,
            Self::Class | Self::Function | Self::Enum | Self::Variable { .. }
        )
    }

    /// Whether the declaration can be referenced directly as a type.
    #[must_use]
    pub fn occupies_type_space(self) -> bool {
        matches!(
            self,
            Self::TypeAlias | Self::Interface | Self::Enum | Self::Class
        )
    }
}

/// One binder parameter a lifted declaration must be re-parameterized over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiftedBinderParam {
    /// Authored position in the binder.
    pub ordinal: usize,
    /// Name as authored in the `generic` attribute.
    pub source_name: String,
    /// Name to emit. Differs from `source_name` when the authored spelling is
    /// already taken at the emitted module scope.
    pub emitted_name: String,
}

impl LiftedBinderParam {
    /// Whether emitting this parameter requires rewriting its references.
    #[must_use]
    pub fn is_renamed(&self) -> bool {
        self.source_name != self.emitted_name
    }
}

/// One free binder-parameter reference inside a lifted declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinderReference {
    /// Authored position in the binder.
    pub ordinal: usize,
    /// Reference site in carrier bytes.
    pub span: SourceRange,
}

/// A script declaration reachable from the public type surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiftedSourceDeclaration {
    /// Authored name.
    pub name: String,
    /// Declaration form.
    pub kind: LiftedDeclarationKind,
    /// Declaration span in the carrier.
    pub span: SourceRange,
    /// Authored visibility: `true` when the source declaration is exported.
    pub exported: bool,
    /// Origin block: `true` for `<script setup>`.
    pub from_setup: bool,
    /// Binder parameters this declaration is free in, constraint/default
    /// closure included, in authored order.
    pub binder_params: Vec<LiftedBinderParam>,
    /// Every free binder-parameter reference site inside the declaration.
    pub binder_references: Vec<BinderReference>,
    /// Free bindings of the declaration, de-duplicated by `(name, space)`.
    pub dependencies: Vec<CapturedDependency>,
}

/// An import the emitted declaration keeps rather than lifting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedImport {
    /// Module specifier.
    pub specifier: String,
    /// Local name bound in this module.
    pub local: String,
    /// Import statement span in the carrier.
    pub span: SourceRange,
    /// Origin block: `true` for `<script setup>`.
    pub from_setup: bool,
    /// `true` for `import type` or a `type` specifier.
    pub type_only: bool,
}

/// A name bound more than once at this module scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateDeclaration {
    /// The doubly-bound name.
    pub name: String,
    /// Every observed binding site, in source order.
    pub origins: Vec<SourceRange>,
}

/// A reference cycle between lifted declarations, recorded by declaration
/// identity. The closure records the cycle and moves on; it never expands it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasCycle {
    /// Declaration names on the cycle, starting at its entry point.
    pub members: Vec<String>,
}

/// Binder-aware capture of the public dependency surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinderCapturePlan {
    /// The authored binder, unchanged from the statement-oriented product.
    pub binder: UniversalSetupBinder,
    /// Per binder parameter, the ordinals its constraint and default refer
    /// to. Parallel to `binder.params`.
    pub binder_param_dependencies: Vec<Vec<usize>>,
    /// One slice per public type expression, in source order.
    pub slices: Vec<PublicTypeDependencySlice>,
    /// Reachable declarations, in discovery order.
    pub declarations: Vec<LiftedSourceDeclaration>,
    /// Imports the emitted declaration retains.
    pub imports: Vec<RetainedImport>,
    /// Doubly-bound names; nothing is lifted for these.
    pub duplicates: Vec<DuplicateDeclaration>,
    /// Cycles observed while closing over declarations.
    pub cycles: Vec<AliasCycle>,
}

impl BinderCapturePlan {
    /// The lifted declaration for `name`, if the public surface reaches it.
    #[must_use]
    pub fn declaration(&self, name: &str) -> Option<&LiftedSourceDeclaration> {
        self.declarations.iter().find(|decl| decl.name == name)
    }

    /// The binder ordinals `seed` needs once constraint and default
    /// references are closed over, in authored order.
    #[must_use]
    pub fn binder_closure(&self, seed: &[usize]) -> Vec<usize> {
        let mut seen: FxHashSet<usize> = FxHashSet::default();
        let mut stack: Vec<usize> = seed.to_vec();
        while let Some(ordinal) = stack.pop() {
            if !seen.insert(ordinal) {
                continue;
            }
            if let Some(deps) = self.binder_param_dependencies.get(ordinal) {
                stack.extend(deps.iter().copied());
            }
        }
        let mut ordered: Vec<usize> = seen.into_iter().collect();
        ordered.sort_unstable();
        ordered
    }
}

/// Capture the public dependency surface of one script pair. `generic` is the
/// authored `generic` attribute value.
///
/// # Errors
///
/// Refuses the same inputs the statement-oriented projection refuses: a
/// non-TypeScript block, a `lang` conflict between the two blocks, a block
/// with syntax errors, or a `generic` attribute that does not parse.
pub fn capture_binder_plan(
    normal: Option<ScriptBlockInput<'_>>,
    setup: Option<ScriptBlockInput<'_>>,
    generic: Option<&str>,
) -> Result<BinderCapturePlan, SetupProjectionRefusal> {
    if let (Some(n), Some(s)) = (&normal, &setup) {
        if n.lang != s.lang {
            return Err(SetupProjectionRefusal::ScriptLangConflict);
        }
    }
    let allocator = Allocator::default();
    let (binder, binder_param_dependencies) = capture_binder(&allocator, generic)?;

    let mut declarations: Vec<DeclRecord<'_>> = Vec::new();
    let mut imports: Vec<RetainedImport> = Vec::new();

    let normal_program = match normal {
        Some(block) => {
            let program = parse_block(&allocator, block, false)?;
            index_block(
                program,
                block.content_start,
                false,
                &mut declarations,
                &mut imports,
            );
            Some(program)
        }
        None => None,
    };
    let setup_program = match setup {
        Some(block) => {
            let program = parse_block(&allocator, block, true)?;
            index_block(
                program,
                block.content_start,
                true,
                &mut declarations,
                &mut imports,
            );
            Some(program)
        }
        None => None,
    };

    let (scope, duplicates) = resolve_module_scope(&declarations, &imports);
    let binder_names: Vec<&str> = binder.params.iter().map(|p| p.name.as_str()).collect();
    let resolver = Resolver {
        scope: &scope,
        binder_names: &binder_names,
    };

    let slices = match (setup_program, setup) {
        (Some(program), Some(block)) => capture_slices(program, block, normal_program, &resolver),
        _ => Vec::new(),
    };

    let (lifted, cycles) = close_over_declarations(&slices, &declarations, &resolver);
    let lifted = assign_binder_names(
        lifted,
        &declarations,
        &binder,
        &binder_param_dependencies,
        &scope,
    );

    Ok(BinderCapturePlan {
        binder,
        binder_param_dependencies,
        slices,
        declarations: lifted,
        imports,
        duplicates,
        cycles,
    })
}

fn parse_block<'a>(
    allocator: &'a Allocator,
    block: ScriptBlockInput<'_>,
    setup: bool,
) -> Result<&'a Program<'a>, SetupProjectionRefusal> {
    let grammar = grammar_of(block.lang)?;
    let content = allocator.alloc_str(block.content);
    let parsed = Parser::new(allocator, content, grammar.source_type()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(SetupProjectionRefusal::SyntaxErrors { setup });
    }
    Ok(allocator.alloc(parsed.program))
}

/// The authored binder plus, per parameter, the ordinals its constraint and
/// default refer to. Parsed once: the product and the dependency edges come
/// from the same `generic` parse.
fn capture_binder(
    allocator: &Allocator,
    generic: Option<&str>,
) -> Result<(UniversalSetupBinder, Vec<Vec<usize>>), SetupProjectionRefusal> {
    let Some(original) = generic else {
        return Ok((UniversalSetupBinder::default(), Vec::new()));
    };
    let text = original.trim();
    if text.is_empty() {
        return Ok((UniversalSetupBinder::default(), Vec::new()));
    }
    let leading_offset = (text.as_ptr() as usize - original.as_ptr() as usize) as u32;
    let result = parse_generic(allocator, text, 0);
    if !result.is_ok() {
        return Err(SetupProjectionRefusal::InvalidGeneric);
    }
    let product = binder_product_from(&result, text, leading_offset);
    let names: Vec<&str> = product.params.iter().map(|p| p.name.as_str()).collect();
    let mut dependencies = vec![Vec::new(); product.params.len()];
    if let Some(declaration) = result.type_parameters() {
        for (ordinal, param) in declaration.params.iter().enumerate() {
            let mut refs = RefCollector::new(0);
            if let Some(constraint) = &param.constraint {
                refs.visit_ts_type(constraint);
            }
            if let Some(default) = &param.default {
                refs.visit_ts_type(default);
            }
            let mut found: Vec<usize> = refs
                .references
                .iter()
                .filter_map(|reference| names.iter().position(|name| *name == reference.name))
                // A parameter may only refer to parameters declared before it.
                .filter(|target| *target < ordinal)
                .collect();
            found.sort_unstable();
            found.dedup();
            dependencies[ordinal] = found;
        }
    }
    Ok((product, dependencies))
}

/// A module-scope declaration, with the AST nodes its public surface spans.
struct DeclRecord<'a> {
    name: String,
    kind: LiftedDeclarationKind,
    span: SourceRange,
    exported: bool,
    from_setup: bool,
    base: u32,
    own_type_params: Vec<String>,
    body: DeclBody<'a>,
}

enum DeclBody<'a> {
    Alias(&'a TSType<'a>),
    Interface(&'a TSInterfaceDeclaration<'a>),
    Class(&'a Class<'a>),
    Function(&'a Function<'a>),
    Annotation(Option<&'a TSType<'a>>),
    Opaque,
}

fn index_block<'a>(
    program: &'a Program<'a>,
    base: u32,
    from_setup: bool,
    declarations: &mut Vec<DeclRecord<'a>>,
    imports: &mut Vec<RetainedImport>,
) {
    for statement in &program.body {
        match statement {
            Statement::ImportDeclaration(import) => {
                let Some(specifiers) = &import.specifiers else {
                    continue;
                };
                for specifier in specifiers {
                    let (local, specifier_type_only) = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(spec) => {
                            (spec.local.name.as_str(), spec.import_kind.is_type())
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(spec) => {
                            (spec.local.name.as_str(), false)
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(spec) => {
                            (spec.local.name.as_str(), false)
                        }
                    };
                    imports.push(RetainedImport {
                        specifier: import.source.value.to_string(),
                        local: local.to_string(),
                        span: range(import.span, base),
                        from_setup,
                        type_only: import.import_kind.is_type() || specifier_type_only,
                    });
                }
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(declaration) = &export.declaration {
                    record_declaration(declaration, true, from_setup, base, declarations);
                }
            }
            other => {
                if let Some(declaration) = other.as_declaration() {
                    record_declaration(declaration, false, from_setup, base, declarations);
                }
            }
        }
    }
}

fn type_param_names(declaration: Option<&TSTypeParameterDeclaration<'_>>) -> Vec<String> {
    declaration
        .map(|decl| {
            decl.params
                .iter()
                .map(|param| param.name.name.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn record_declaration<'a>(
    declaration: &'a Declaration<'a>,
    exported: bool,
    from_setup: bool,
    base: u32,
    out: &mut Vec<DeclRecord<'a>>,
) {
    let mut push = |name: String, kind, span, own_type_params, body| {
        out.push(DeclRecord {
            name,
            kind,
            span,
            exported,
            from_setup,
            base,
            own_type_params,
            body,
        });
    };
    match declaration {
        Declaration::VariableDeclaration(vars) => {
            for declarator in &vars.declarations {
                let Some(id) = declarator.id.get_binding_identifier() else {
                    continue;
                };
                let annotation = declarator
                    .type_annotation
                    .as_ref()
                    .map(|annotation| &annotation.type_annotation);
                push(
                    id.name.to_string(),
                    LiftedDeclarationKind::Variable {
                        unique_symbol: annotation.is_some_and(is_unique_symbol),
                    },
                    range(declarator.span, base),
                    Vec::new(),
                    DeclBody::Annotation(annotation),
                );
            }
        }
        Declaration::FunctionDeclaration(function) => {
            if let Some(id) = &function.id {
                push(
                    id.name.to_string(),
                    LiftedDeclarationKind::Function,
                    range(function.span, base),
                    type_param_names(function.type_parameters.as_deref()),
                    DeclBody::Function(function),
                );
            }
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                push(
                    id.name.to_string(),
                    LiftedDeclarationKind::Class,
                    range(class.span, base),
                    type_param_names(class.type_parameters.as_deref()),
                    DeclBody::Class(class),
                );
            }
        }
        Declaration::TSTypeAliasDeclaration(alias) => push(
            alias.id.name.to_string(),
            LiftedDeclarationKind::TypeAlias,
            range(alias.span, base),
            type_param_names(alias.type_parameters.as_deref()),
            DeclBody::Alias(&alias.type_annotation),
        ),
        Declaration::TSInterfaceDeclaration(interface) => push(
            interface.id.name.to_string(),
            LiftedDeclarationKind::Interface,
            range(interface.span, base),
            type_param_names(interface.type_parameters.as_deref()),
            DeclBody::Interface(interface),
        ),
        Declaration::TSEnumDeclaration(enumeration) => push(
            enumeration.id.name.to_string(),
            LiftedDeclarationKind::Enum,
            range(enumeration.span, base),
            Vec::new(),
            DeclBody::Opaque,
        ),
        _ => {}
    }
}

fn is_unique_symbol(ty: &TSType<'_>) -> bool {
    matches!(
        ty,
        TSType::TSTypeOperatorType(operator)
            if operator.operator == TSTypeOperatorOperator::Unique
    )
}

/// What a module-scope name binds to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Binding {
    Declaration(usize),
    MergedDeclarations(Vec<usize>),
    Import(usize),
    Duplicate,
}

fn resolve_module_scope(
    declarations: &[DeclRecord<'_>],
    imports: &[RetainedImport],
) -> (
    FxHashMap<(String, DependencySpace), Binding>,
    Vec<DuplicateDeclaration>,
) {
    let mut scope: FxHashMap<(String, DependencySpace), Binding> = FxHashMap::default();
    let mut origins: FxHashMap<String, Vec<SourceRange>> = FxHashMap::default();
    let mut duplicated: FxHashSet<String> = FxHashSet::default();

    let mut insert =
        |name: &str, space, binding: Binding, span, kind: Option<LiftedDeclarationKind>| {
            origins.entry(name.to_string()).or_default().push(span);
            let key = (name.to_string(), space);
            let Some(existing) = scope.get_mut(&key) else {
                scope.insert(key, binding);
                return;
            };
            if matches!(existing, Binding::Duplicate) {
                return;
            }
            let merges_interface = space == DependencySpace::Type
                && kind == Some(LiftedDeclarationKind::Interface)
                && match existing {
                    Binding::Declaration(index) => {
                        declarations[*index].kind == LiftedDeclarationKind::Interface
                    }
                    Binding::MergedDeclarations(indices) => indices
                        .iter()
                        .all(|index| declarations[*index].kind == LiftedDeclarationKind::Interface),
                    Binding::Import(_) | Binding::Duplicate => false,
                };
            if merges_interface {
                let Binding::Declaration(index) = binding else {
                    unreachable!("only declarations merge");
                };
                match existing {
                    Binding::Declaration(first) => {
                        *existing = Binding::MergedDeclarations(vec![*first, index])
                    }
                    Binding::MergedDeclarations(indices) => indices.push(index),
                    Binding::Import(_) | Binding::Duplicate => {
                        unreachable!("checked interface merge")
                    }
                }
                return;
            }
            *existing = Binding::Duplicate;
            duplicated.insert(name.to_string());
        };

    for (index, import) in imports.iter().enumerate() {
        insert(
            &import.local,
            DependencySpace::Type,
            Binding::Import(index),
            import.span,
            None,
        );
        if !import.type_only {
            insert(
                &import.local,
                DependencySpace::Value,
                Binding::Import(index),
                import.span,
                None,
            );
        }
    }
    for (index, declaration) in declarations.iter().enumerate() {
        for space in [DependencySpace::Type, DependencySpace::Value] {
            let occupies_space = match space {
                DependencySpace::Type => declaration.kind.occupies_type_space(),
                DependencySpace::Value => declaration.kind.occupies_value_space(),
            };
            if occupies_space {
                insert(
                    &declaration.name,
                    space,
                    Binding::Declaration(index),
                    declaration.span,
                    Some(declaration.kind),
                );
            }
        }
    }
    let duplicates = duplicated
        .into_iter()
        .map(|name| {
            let mut spans = origins
                .remove(&name)
                .expect("every duplicate name has recorded origins");
            spans.sort_by_key(|span| span.start);
            spans.dedup_by_key(|span| (span.start, span.end));
            DuplicateDeclaration {
                name,
                origins: spans,
            }
        })
        .collect();
    (scope, duplicates)
}

struct Resolver<'r> {
    scope: &'r FxHashMap<(String, DependencySpace), Binding>,
    binder_names: &'r [&'r str],
}

impl Resolver<'_> {
    /// Resolve `name` as seen from a block. The binder scopes `<script setup>`
    /// only, and there shadows a module-scope binding of the same name.
    fn resolve(
        &self,
        name: &str,
        space: DependencySpace,
        binder_in_scope: bool,
    ) -> DependencyOrigin {
        if binder_in_scope && space == DependencySpace::Type {
            if let Some(ordinal) = self.binder_names.iter().position(|bound| *bound == name) {
                return DependencyOrigin::BinderParam { ordinal };
            }
        }
        match self.scope.get(&(name.to_string(), space)) {
            Some(Binding::Declaration(index)) => {
                DependencyOrigin::LocalDeclaration { index: *index }
            }
            Some(Binding::MergedDeclarations(indices)) => DependencyOrigin::LocalDeclarations {
                indices: indices.clone(),
            },
            Some(Binding::Import(index)) => DependencyOrigin::Import { index: *index },
            Some(Binding::Duplicate) => DependencyOrigin::Duplicate,
            None => DependencyOrigin::Unresolved,
        }
    }
}

fn local_declaration_indices(origin: &DependencyOrigin) -> Vec<usize> {
    match origin {
        DependencyOrigin::LocalDeclaration { index } => vec![*index],
        DependencyOrigin::LocalDeclarations { indices } => indices.clone(),
        DependencyOrigin::BinderParam { .. }
        | DependencyOrigin::Import { .. }
        | DependencyOrigin::Duplicate
        | DependencyOrigin::Unresolved => Vec::new(),
    }
}

/// One raw reference site, before resolution.
struct RawReference {
    name: String,
    space: DependencySpace,
    span: SourceRange,
}

/// Collects free type and `typeof` references, honouring the type-parameter
/// binders it walks through. It records references only; it resolves nothing
/// and evaluates nothing.
struct RefCollector {
    base: u32,
    bound: Vec<String>,
    /// Every name this walk ever bound locally, for capture avoidance.
    bound_seen: FxHashSet<String>,
    references: Vec<RawReference>,
}

impl RefCollector {
    fn new(base: u32) -> Self {
        Self {
            base,
            bound: Vec::new(),
            bound_seen: FxHashSet::default(),
            references: Vec::new(),
        }
    }

    fn push_scope(&mut self, names: Vec<String>) -> usize {
        let depth = self.bound.len();
        for name in names {
            self.bound_seen.insert(name.clone());
            self.bound.push(name);
        }
        depth
    }

    fn pop_scope(&mut self, depth: usize) {
        self.bound.truncate(depth);
    }

    fn note(&mut self, name: &str, space: DependencySpace, span: oxc_span::Span) {
        if self.bound.iter().any(|bound| bound == name) {
            return;
        }
        self.references.push(RawReference {
            name: name.to_string(),
            space,
            span: range(span, self.base),
        });
    }

    fn visit_signature(&mut self, function: &Function<'_>) {
        let depth = self.push_scope(type_param_names(function.type_parameters.as_deref()));
        if let Some(parameters) = &function.type_parameters {
            self.visit_ts_type_parameter_declaration(parameters);
        }
        if let Some(this_param) = &function.this_param {
            self.visit_ts_this_parameter(this_param);
        }
        self.visit_formal_parameters(&function.params);
        if let Some(return_type) = &function.return_type {
            self.visit_ts_type_annotation(return_type);
        }
        self.pop_scope(depth);
    }

    /// Walks a class's public shape: heritage, implements and the type
    /// annotations of its members. Method bodies are not public surface.
    fn visit_class_shape(&mut self, class: &Class<'_>) {
        let depth = self.push_scope(type_param_names(class.type_parameters.as_deref()));
        if let Some(parameters) = &class.type_parameters {
            self.visit_ts_type_parameter_declaration(parameters);
        }
        if let Some(arguments) = &class.super_type_arguments {
            self.visit_ts_type_parameter_instantiation(arguments);
        }
        if let Some(super_class) = &class.super_class {
            if let Some((name, span)) = leftmost_expression_identifier(super_class) {
                self.note(name, DependencySpace::Value, span);
            }
        }
        for implemented in &class.implements {
            self.visit_ts_class_implements(implemented);
        }
        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(property) => {
                    if let Some(annotation) = &property.type_annotation {
                        self.visit_ts_type_annotation(annotation);
                    }
                }
                ClassElement::AccessorProperty(accessor) => {
                    if let Some(annotation) = &accessor.type_annotation {
                        self.visit_ts_type_annotation(annotation);
                    }
                }
                ClassElement::MethodDefinition(method) => self.visit_signature(&method.value),
                ClassElement::TSIndexSignature(signature) => {
                    self.visit_ts_index_signature(signature);
                }
                ClassElement::StaticBlock(_) => {}
            }
        }
        self.pop_scope(depth);
    }
}

/// Names bound by `infer` inside a conditional type's `extends` clause.
struct InferNames {
    names: Vec<String>,
}

impl<'a> Visit<'a> for InferNames {
    fn visit_ts_infer_type(&mut self, it: &TSInferType<'a>) {
        self.names.push(it.type_parameter.name.name.to_string());
        walk::walk_ts_infer_type(self, it);
    }
}

fn leftmost_type_name<'n>(name: &'n TSTypeName<'_>) -> Option<(&'n str, oxc_span::Span)> {
    match name {
        TSTypeName::IdentifierReference(id) => Some((id.name.as_str(), id.span)),
        TSTypeName::QualifiedName(qualified) => leftmost_type_name(&qualified.left),
        TSTypeName::ThisExpression(_) => None,
    }
}

fn leftmost_expression_identifier<'n>(
    expression: &'n Expression<'_>,
) -> Option<(&'n str, oxc_span::Span)> {
    match expression {
        Expression::Identifier(identifier) => Some((identifier.name.as_str(), identifier.span)),
        Expression::StaticMemberExpression(member) => {
            leftmost_expression_identifier(&member.object)
        }
        _ => None,
    }
}

impl<'a> Visit<'a> for RefCollector {
    fn visit_ts_type_reference(&mut self, it: &TSTypeReference<'a>) {
        if let Some((name, span)) = leftmost_type_name(&it.type_name) {
            self.note(name, DependencySpace::Type, span);
        }
        walk::walk_ts_type_reference(self, it);
    }

    fn visit_ts_type_query(&mut self, it: &TSTypeQuery<'a>) {
        let entity = match &it.expr_name {
            TSTypeQueryExprName::IdentifierReference(id) => Some((id.name.as_str(), id.span)),
            TSTypeQueryExprName::QualifiedName(qualified) => leftmost_type_name(&qualified.left),
            TSTypeQueryExprName::ThisExpression(_) | TSTypeQueryExprName::TSImportType(_) => None,
        };
        if let Some((name, span)) = entity {
            self.note(name, DependencySpace::Value, span);
        }
        walk::walk_ts_type_query(self, it);
    }

    fn visit_ts_function_type(&mut self, it: &oxc_ast::ast::TSFunctionType<'a>) {
        let depth = self.push_scope(type_param_names(it.type_parameters.as_deref()));
        walk::walk_ts_function_type(self, it);
        self.pop_scope(depth);
    }

    fn visit_ts_constructor_type(&mut self, it: &oxc_ast::ast::TSConstructorType<'a>) {
        let depth = self.push_scope(type_param_names(it.type_parameters.as_deref()));
        walk::walk_ts_constructor_type(self, it);
        self.pop_scope(depth);
    }

    fn visit_ts_method_signature(&mut self, it: &TSMethodSignature<'a>) {
        let depth = self.push_scope(type_param_names(it.type_parameters.as_deref()));
        if let Some(parameters) = &it.type_parameters {
            self.visit_ts_type_parameter_declaration(parameters);
        }
        if let Some(this_param) = &it.this_param {
            self.visit_ts_this_parameter(this_param);
        }
        self.visit_formal_parameters(&it.params);
        if let Some(return_type) = &it.return_type {
            self.visit_ts_type_annotation(return_type);
        }
        self.pop_scope(depth);
    }

    fn visit_ts_call_signature_declaration(&mut self, it: &TSCallSignatureDeclaration<'a>) {
        let depth = self.push_scope(type_param_names(it.type_parameters.as_deref()));
        if let Some(parameters) = &it.type_parameters {
            self.visit_ts_type_parameter_declaration(parameters);
        }
        if let Some(this_param) = &it.this_param {
            self.visit_ts_this_parameter(this_param);
        }
        self.visit_formal_parameters(&it.params);
        if let Some(return_type) = &it.return_type {
            self.visit_ts_type_annotation(return_type);
        }
        self.pop_scope(depth);
    }

    fn visit_ts_construct_signature_declaration(
        &mut self,
        it: &TSConstructSignatureDeclaration<'a>,
    ) {
        let depth = self.push_scope(type_param_names(it.type_parameters.as_deref()));
        if let Some(parameters) = &it.type_parameters {
            self.visit_ts_type_parameter_declaration(parameters);
        }
        self.visit_formal_parameters(&it.params);
        if let Some(return_type) = &it.return_type {
            self.visit_ts_type_annotation(return_type);
        }
        self.pop_scope(depth);
    }

    fn visit_ts_interface_heritage(&mut self, it: &TSInterfaceHeritage<'a>) {
        if let Some((name, span)) = leftmost_expression_identifier(&it.expression) {
            self.note(name, DependencySpace::Type, span);
        }
        if let Some(arguments) = &it.type_arguments {
            self.visit_ts_type_parameter_instantiation(arguments);
        }
    }

    fn visit_ts_class_implements(&mut self, it: &oxc_ast::ast::TSClassImplements<'a>) {
        if let Some((name, span)) = leftmost_type_name(&it.expression) {
            self.note(name, DependencySpace::Type, span);
        }
        if let Some(arguments) = &it.type_arguments {
            self.visit_ts_type_parameter_instantiation(arguments);
        }
    }

    fn visit_ts_mapped_type(&mut self, it: &oxc_ast::ast::TSMappedType<'a>) {
        let depth = self.push_scope(vec![it.key.name.to_string()]);
        walk::walk_ts_mapped_type(self, it);
        self.pop_scope(depth);
    }

    fn visit_ts_conditional_type(&mut self, it: &oxc_ast::ast::TSConditionalType<'a>) {
        let mut infer = InferNames { names: Vec::new() };
        infer.visit_ts_type(&it.extends_type);
        let depth = self.push_scope(infer.names);
        walk::walk_ts_conditional_type(self, it);
        self.pop_scope(depth);
    }

    fn visit_ts_type_alias_declaration(&mut self, it: &oxc_ast::ast::TSTypeAliasDeclaration<'a>) {
        let depth = self.push_scope(type_param_names(it.type_parameters.as_deref()));
        walk::walk_ts_type_alias_declaration(self, it);
        self.pop_scope(depth);
    }

    fn visit_ts_interface_declaration(&mut self, it: &TSInterfaceDeclaration<'a>) {
        let depth = self.push_scope(type_param_names(it.type_parameters.as_deref()));
        walk::walk_ts_interface_declaration(self, it);
        self.pop_scope(depth);
    }
}

fn resolve_references(
    raw: Vec<RawReference>,
    resolver: &Resolver<'_>,
    binder_in_scope: bool,
) -> (Vec<CapturedDependency>, Vec<BinderReference>) {
    let mut dependencies: Vec<CapturedDependency> = Vec::new();
    let mut binder_references: Vec<BinderReference> = Vec::new();
    for reference in raw {
        let origin = resolver.resolve(&reference.name, reference.space, binder_in_scope);
        if let DependencyOrigin::BinderParam { ordinal } = origin {
            binder_references.push(BinderReference {
                ordinal,
                span: reference.span,
            });
        }
        if dependencies
            .iter()
            .any(|seen| seen.name == reference.name && seen.space == reference.space)
        {
            continue;
        }
        dependencies.push(CapturedDependency {
            name: reference.name,
            space: reference.space,
            origin,
            reference: reference.span,
        });
    }
    (dependencies, binder_references)
}

/// Collects the macro calls whose arguments carry the public type surface.
struct RootMacroVisitor<'m, 'r> {
    macros: MacroContext<'m>,
    resolver: &'r Resolver<'r>,
    base: u32,
    depth: u32,
    slices: Vec<PublicTypeDependencySlice>,
}

impl RootMacroVisitor<'_, '_> {
    fn capture(&mut self, call: &CallExpression<'_>, name: &str) {
        let root = match name {
            "defineProps" => PublicTypeRoot::Props,
            "defineEmits" => PublicTypeRoot::Emits,
            "defineSlots" => PublicTypeRoot::Slots,
            "defineModel" => PublicTypeRoot::Model,
            "defineExpose" => {
                self.capture_exposed(call);
                return;
            }
            _ => return,
        };
        let Some(argument) = call
            .type_arguments
            .as_ref()
            .and_then(|arguments| arguments.params.first())
        else {
            return;
        };
        let mut collector = RefCollector::new(self.base);
        collector.visit_ts_type(argument);
        let (dependencies, _) = resolve_references(collector.references, self.resolver, true);
        self.slices.push(PublicTypeDependencySlice {
            root,
            macro_call: range(call.span, self.base),
            expression: range(argument.span(), self.base),
            dependencies,
        });
    }

    /// `defineExpose({ a, b })` publishes runtime bindings; each named
    /// binding is a value-space dependency of the public instance surface.
    fn capture_exposed(&mut self, call: &CallExpression<'_>) {
        let Some(Expression::ObjectExpression(object)) = call
            .arguments
            .first()
            .and_then(|argument| argument.as_expression())
        else {
            return;
        };
        let mut dependencies: Vec<CapturedDependency> = Vec::new();
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            let Expression::Identifier(identifier) = &property.value else {
                continue;
            };
            let name = identifier.name.as_str();
            if dependencies.iter().any(|seen| seen.name == name) {
                continue;
            }
            dependencies.push(CapturedDependency {
                name: name.to_string(),
                space: DependencySpace::Value,
                origin: self.resolver.resolve(name, DependencySpace::Value, true),
                reference: range(identifier.span, self.base),
            });
        }
        self.slices.push(PublicTypeDependencySlice {
            root: PublicTypeRoot::Expose,
            macro_call: range(call.span, self.base),
            expression: range(object.span, self.base),
            dependencies,
        });
    }
}

impl<'a> Visit<'a> for RootMacroVisitor<'_, '_> {
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
        self.depth += 1;
        walk::walk_class(self, it);
        self.depth -= 1;
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Some(name) = self.macros.resolve(it, self.depth) {
            self.capture(it, name);
        }
        walk::walk_call_expression(self, it);
    }
}

fn capture_slices<'a>(
    setup: &'a Program<'a>,
    block: ScriptBlockInput<'_>,
    normal: Option<&'a Program<'a>>,
    resolver: &Resolver<'_>,
) -> Vec<PublicTypeDependencySlice> {
    let semantic = SemanticBuilder::new().build(setup).semantic;
    let normal_bindings = normal
        .map(|program| {
            let semantic = SemanticBuilder::new().build(program).semantic;
            value_bindings(semantic.scoping())
        })
        .unwrap_or_default();
    let module_bound: FxHashSet<&str> = normal_bindings.iter().map(String::as_str).collect();
    let vue_macro_imports = vue_runtime_macro_imports(setup);
    let mut visitor = RootMacroVisitor {
        macros: MacroContext {
            scoping: semantic.scoping(),
            module_bound: &module_bound,
            vue_macro_imports: &vue_macro_imports,
            macro_positions: macro_positions(setup),
        },
        resolver,
        base: block.content_start,
        depth: 0,
        slices: Vec::new(),
    };
    visitor.visit_program(setup);
    visitor.slices
}

/// A lifted declaration before binder parameters are named.
struct PendingLift {
    index: usize,
    binder_ordinals: Vec<usize>,
    binder_references: Vec<BinderReference>,
    dependencies: Vec<CapturedDependency>,
    locally_bound: FxHashSet<String>,
}

/// Closes over the declarations the public slices reach. The worklist is
/// keyed by declaration identity, so a recursive alias is visited once and
/// reported as a cycle instead of expanded.
fn close_over_declarations(
    slices: &[PublicTypeDependencySlice],
    declarations: &[DeclRecord<'_>],
    resolver: &Resolver<'_>,
) -> (Vec<PendingLift>, Vec<AliasCycle>) {
    let mut queued: FxHashSet<usize> = FxHashSet::default();
    let mut pending: Vec<usize> = Vec::new();
    for slice in slices {
        for dependency in &slice.dependencies {
            for index in local_declaration_indices(&dependency.origin) {
                if queued.insert(index) {
                    pending.push(index);
                }
            }
        }
    }
    let mut lifted: Vec<PendingLift> = Vec::new();
    let mut cursor = 0;
    while cursor < pending.len() {
        let index = pending[cursor];
        cursor += 1;
        let record = &declarations[index];
        let mut collector = RefCollector::new(record.base);
        collector.push_scope(record.own_type_params.clone());
        match &record.body {
            DeclBody::Alias(ty) => collector.visit_ts_type(ty),
            DeclBody::Interface(interface) => {
                for heritage in &interface.extends {
                    collector.visit_ts_interface_heritage(heritage);
                }
                collector.visit_ts_interface_body(&interface.body);
            }
            DeclBody::Class(class) => collector.visit_class_shape(class),
            DeclBody::Function(function) => collector.visit_signature(function),
            DeclBody::Annotation(Some(ty)) => collector.visit_ts_type(ty),
            DeclBody::Annotation(None) | DeclBody::Opaque => {}
        }
        let locally_bound = collector.bound_seen.clone();
        let (dependencies, binder_references) =
            resolve_references(collector.references, resolver, record.from_setup);
        let mut binder_ordinals: Vec<usize> = binder_references
            .iter()
            .map(|reference| reference.ordinal)
            .collect();
        binder_ordinals.sort_unstable();
        binder_ordinals.dedup();
        for dependency in &dependencies {
            for index in local_declaration_indices(&dependency.origin) {
                if queued.insert(index) {
                    pending.push(index);
                }
            }
        }
        lifted.push(PendingLift {
            index,
            binder_ordinals,
            binder_references,
            dependencies,
            locally_bound,
        });
    }
    let cycles = detect_cycles(&lifted, declarations);
    (lifted, cycles)
}

/// Reports the reference cycles between lifted declarations. The cycles are
/// observed on the already-terminated closure graph; nothing is expanded.
fn detect_cycles(lifted: &[PendingLift], declarations: &[DeclRecord<'_>]) -> Vec<AliasCycle> {
    let position: FxHashMap<usize, usize> = lifted
        .iter()
        .enumerate()
        .map(|(slot, entry)| (entry.index, slot))
        .collect();
    let edges: Vec<Vec<usize>> = lifted
        .iter()
        .map(|entry| {
            let mut targets: Vec<usize> = entry
                .dependencies
                .iter()
                .flat_map(|dependency| local_declaration_indices(&dependency.origin))
                .filter_map(|index| position.get(&index).copied())
                .collect();
            targets.dedup();
            targets
        })
        .collect();

    /// Tri-colour depth-first marking: 0 unvisited, 1 on the current
    /// stack, 2 finished. A back edge onto a stacked node is a cycle.
    fn walk_cycle(
        slot: usize,
        edges: &[Vec<usize>],
        marks: &mut [u8],
        stack: &mut Vec<usize>,
        cycles: &mut Vec<Vec<usize>>,
    ) {
        marks[slot] = 1;
        stack.push(slot);
        for target in &edges[slot] {
            match marks[*target] {
                1 => {
                    if let Some(entry) = stack.iter().position(|visited| visited == target) {
                        cycles.push(stack[entry..].to_vec());
                    }
                }
                0 => walk_cycle(*target, edges, marks, stack, cycles),
                _ => {}
            }
        }
        stack.pop();
        marks[slot] = 2;
    }

    let mut marks = vec![0u8; lifted.len()];
    let mut stack: Vec<usize> = Vec::new();
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    for root in 0..lifted.len() {
        if marks[root] != 0 {
            continue;
        }
        walk_cycle(root, &edges, &mut marks, &mut stack, &mut cycles);
    }
    cycles
        .into_iter()
        .map(|slots| AliasCycle {
            members: slots
                .into_iter()
                .map(|slot| declarations[lifted[slot].index].name.clone())
                .collect(),
        })
        .collect()
}

/// Turns the closure's pending entries into the lifted product, naming each
/// declaration's binder parameters so that emitting it at module scope cannot
/// capture an unrelated declaration of the same name.
fn assign_binder_names(
    lifted: Vec<PendingLift>,
    declarations: &[DeclRecord<'_>],
    binder: &UniversalSetupBinder,
    binder_param_dependencies: &[Vec<usize>],
    scope: &FxHashMap<(String, DependencySpace), Binding>,
) -> Vec<LiftedSourceDeclaration> {
    let module_names: FxHashSet<&str> = scope.keys().map(|(name, _)| name.as_str()).collect();
    let positions: FxHashMap<usize, usize> = lifted
        .iter()
        .enumerate()
        .map(|(position, entry)| (entry.index, position))
        .collect();
    let mut required: Vec<FxHashSet<usize>> = lifted
        .iter()
        .map(|entry| entry.binder_ordinals.iter().copied().collect())
        .collect();
    let mut changed = true;
    while changed {
        changed = false;
        for (position, entry) in lifted.iter().enumerate() {
            let inherited: Vec<usize> = entry
                .dependencies
                .iter()
                .flat_map(|dependency| local_declaration_indices(&dependency.origin))
                .filter_map(|index| positions.get(&index).copied())
                .flat_map(|target| required[target].iter().copied())
                .collect();
            for ordinal in inherited {
                changed |= required[position].insert(ordinal);
            }
        }
    }
    lifted
        .into_iter()
        .enumerate()
        .map(|(position, entry)| {
            let record = &declarations[entry.index];
            let mut direct: Vec<usize> = required[position].iter().copied().collect();
            direct.sort_unstable();
            let ordinals = closure_over(&direct, binder_param_dependencies);
            let mut taken: FxHashSet<String> = FxHashSet::default();
            let binder_params: Vec<LiftedBinderParam> = ordinals
                .into_iter()
                .filter_map(|ordinal| {
                    let source_name = binder.params.get(ordinal)?.name.clone();
                    let emitted_name =
                        free_name(&source_name, &module_names, &entry.locally_bound, &taken);
                    taken.insert(emitted_name.clone());
                    Some(LiftedBinderParam {
                        ordinal,
                        source_name,
                        emitted_name,
                    })
                })
                .collect();
            LiftedSourceDeclaration {
                name: record.name.clone(),
                kind: record.kind,
                span: record.span,
                exported: record.exported,
                from_setup: record.from_setup,
                binder_params,
                binder_references: entry.binder_references,
                dependencies: entry.dependencies,
            }
        })
        .collect()
}

fn closure_over(seed: &[usize], dependencies: &[Vec<usize>]) -> Vec<usize> {
    let mut seen: FxHashSet<usize> = FxHashSet::default();
    let mut stack: Vec<usize> = seed.to_vec();
    while let Some(ordinal) = stack.pop() {
        if !seen.insert(ordinal) {
            continue;
        }
        if let Some(edges) = dependencies.get(ordinal) {
            stack.extend(edges.iter().copied());
        }
    }
    let mut ordered: Vec<usize> = seen.into_iter().collect();
    ordered.sort_unstable();
    ordered
}

/// The first spelling of `name` free at the emitted module scope, inside the
/// declaration's own binders, and among the parameters already introduced.
fn free_name(
    name: &str,
    module_names: &FxHashSet<&str>,
    locally_bound: &FxHashSet<String>,
    taken: &FxHashSet<String>,
) -> String {
    let conflicts = |candidate: &str| {
        module_names.contains(candidate)
            || locally_bound.contains(candidate)
            || taken.contains(candidate)
    };
    if !conflicts(name) {
        return name.to_string();
    }
    let mut suffix = 1u32;
    loop {
        let candidate = format!("{name}_{suffix}");
        if !conflicts(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}
