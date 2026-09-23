//! Vue public constructor and instance contract.
//!
//! A Vue SFC's default export is constructor-shaped: consumers write
//! `InstanceType<typeof Comp>`, `ref<InstanceType<typeof Comp> | null>`,
//! `Comp<number>` and `new Comp({ test: 0 })`, and every one of those must
//! keep working without casts, factory wrappers or permissive overloads. This
//! module derives that public contract from the authored script blocks by
//! syntax and provenance only; TypeScript stays the type-answer owner and
//! checks the rendered declaration.
//!
//! Three products:
//!
//! - [`VuePublicConstructorContract`] — the authored generic binder
//!   (constraints, defaults and `const` modifiers), the props / events /
//!   models / slots / exposed / static-options surface, each kept as the
//!   authored type text, plus the one rendered public declaration.
//! - [`PublicInstanceProjection`] — the members `InstanceType<typeof Comp>`
//!   publishes (`$props`, `$emit`, `$slots` and exposed members over Vue's
//!   `ComponentPublicInstance` base) and the setup bindings that stay private.
//! - [`ConstructorCompatibilityReceipt`] — what the rendered constructor
//!   guarantees: exactly one generic construct signature, no call signature,
//!   the props-parameter optionality, and which surfaces the binder reaches.
//!
//! Rendering rules, and why:
//!
//! - One construct signature carries the authored binder, `const` included,
//!   so an explicit `Comp<boolean>` that violates a constraint is a real
//!   diagnostic rather than a fallback overload match, and `new Comp({...})`
//!   infers the binder from the props argument.
//! - Props and instance surfaces are type aliases over the same binder
//!   (without `const`, which aliases cannot carry), so the arguments a use
//!   selects stay observable in every public member.
//! - The props parameter is required exactly when an authored prop or model
//!   is required. When the syntax cannot decide (a props type named from
//!   another module), the parameter is the conditional rest tuple so
//!   TypeScript decides — never a blanket optional parameter.
//! - Exposed members come from the checking body's expose provider
//!   ([`EXPOSE_PROVIDER`]), instantiated with the binder, so private setup
//!   bindings never reach the instance and generic-dependent exposed members
//!   stay specialized.
//! - Authored `any` and framework-legal open domains (runtime prop names,
//!   an untyped `defineModel()`, a component without declared events) keep
//!   Vue's own typing; nothing else is widened.
//!
//! A carrier without `<script setup>` exports its authored default, which Vue
//! already types as a constructor (`defineComponent`); no generated
//! constructor replaces it.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrayExpressionElement, ArrowFunctionExpression, CallExpression, Class, Declaration,
    ExportDefaultDeclarationKind, Expression, Function, ObjectExpression, ObjectPropertyKind,
    Program, PropertyKey, Statement, TSSignature, TSType, TSTypeName,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::GetSpan;
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashSet;

use super::script_setup::{
    binder_product_from, grammar_of, macro_positions, range, value_bindings,
    vue_runtime_macro_imports, MacroContext, ScriptBlockInput, SetupProjectionRefusal, SourceRange,
};
use crate::utils::oxc::vue::parse_generic;

/// Name of the rendered public component value (the default export).
pub const PUBLIC_COMPONENT: &str = "__VerterPublicComponent";
/// Name of the rendered props alias.
pub const PUBLIC_PROPS: &str = "__VerterPublicProps";
/// Name of the rendered instance alias.
pub const PUBLIC_INSTANCE: &str = "__VerterPublicInstance";
/// The checking body's expose provider: a function over the authored binder
/// returning the `defineExpose` argument. The contract names it; the
/// checking body (or its declaration emit) supplies it.
pub const EXPOSE_PROVIDER: &str = "__VerterExpose";
/// Hoisted runtime props options (Vue hoists `defineProps({...})` out of
/// setup, so it may only reference module scope).
pub const RUNTIME_PROPS: &str = "__VerterRuntimeProps";
/// Hoisted runtime emits options.
pub const RUNTIME_EMITS: &str = "__VerterRuntimeEmits";

/// Where the public constructor comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConstructorSource {
    /// `<script setup>`: the constructor is generated from this contract.
    ScriptSetup,
    /// No setup block: the authored default export (Vue's `defineComponent`
    /// type) is already the constructor.
    AuthoredDefault,
}

/// One authored `generic` parameter, in authored order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicBinderParam {
    /// Parameter name.
    pub name: String,
    /// Authored `const` modifier.
    pub is_const: bool,
    /// Constraint text.
    pub constraint: Option<String>,
    /// Default text.
    pub default: Option<String>,
}

/// Whether callers must pass the props argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropsRequirement {
    /// At least one authored prop or model is required.
    Required,
    /// Every authored prop and model is optional.
    Optional,
    /// The syntax cannot decide; TypeScript decides via a conditional rest.
    Undetermined,
}

impl PropsRequirement {
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Required, _) | (_, Self::Required) => Self::Required,
            (Self::Undetermined, _) | (_, Self::Undetermined) => Self::Undetermined,
            _ => Self::Optional,
        }
    }
}

/// How a macro declared its public type.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeclaredSurface {
    /// Not declared.
    None,
    /// `define*<T>()`: the authored type argument.
    TypeArgument {
        /// Type argument range in the carrier.
        expression: SourceRange,
        /// Authored text.
        text: String,
    },
    /// `define*(['a', 'b'])`: runtime names.
    RuntimeNames {
        /// Names in authored order.
        names: Vec<String>,
    },
    /// `define*({ ... })` or another runtime value.
    RuntimeOptions {
        /// Argument range in the carrier.
        expression: SourceRange,
        /// Authored text.
        text: String,
    },
}

/// One `defineModel` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicModel {
    /// Model name (`modelValue` when unnamed).
    pub name: String,
    /// Authored value type text; `None` is Vue's open `any`.
    pub value_type: Option<String>,
    /// `{ required: true }`.
    pub required: bool,
    /// Call range in the carrier.
    pub call: SourceRange,
}

/// One exposed member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposedMember {
    /// Member name.
    pub name: String,
    /// Key range in the carrier.
    pub span: SourceRange,
}

/// The `defineExpose` surface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExposeSurface {
    /// `defineExpose` call range; `None` keeps the instance closed.
    pub call: Option<SourceRange>,
    /// Statically named members.
    pub members: Vec<ExposedMember>,
    /// A spread, computed key or non-literal argument: the provider's type
    /// decides the member set.
    pub open: bool,
}

/// Literal static options (`defineOptions` or the normal default export).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticOptions {
    /// Literal `name`.
    pub name: Option<String>,
    /// Literal `inheritAttrs`.
    pub inherit_attrs: Option<bool>,
}

/// A public surface the construct signature publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PublicSurface {
    /// `$props` and the constructor argument.
    Props,
    /// `$emit` and the listener props.
    Events,
    /// Model props, listeners and events.
    Models,
    /// `$slots`.
    Slots,
    /// Exposed members.
    Expose,
}

/// Origin of one public instance member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceMemberOrigin {
    /// A Vue public-instance member the contract types (`$props`, `$emit`,
    /// `$slots`).
    Framework,
    /// A `defineExpose` member.
    Exposed,
}

/// One member `InstanceType<typeof Comp>` publishes beyond Vue's base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicInstanceMember {
    /// Member name.
    pub name: String,
    /// Where it comes from.
    pub origin: InstanceMemberOrigin,
}

/// The public instance: Vue's `ComponentPublicInstance` base with the typed
/// `$props` / `$emit` / `$slots` and the exposed members; nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublicInstanceProjection {
    /// Typed members, framework members first.
    pub members: Vec<PublicInstanceMember>,
    /// Setup bindings that stay off the instance, in source order.
    pub private_bindings: Vec<String>,
}

impl PublicInstanceProjection {
    /// Whether `name` is published on the instance.
    #[must_use]
    pub fn publishes(&self, name: &str) -> bool {
        self.members.iter().any(|member| member.name == name)
    }
}

/// What the rendered constructor guarantees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructorCompatibilityReceipt {
    /// Construct signatures on the rendered default export.
    pub construct_signatures: usize,
    /// Call signatures on the rendered default export.
    pub call_signatures: usize,
    /// Props-parameter optionality.
    pub props_parameter: PropsRequirement,
    /// Binder parameter names the construct signature declares.
    pub binder_params: Vec<String>,
    /// Surfaces whose authored type references a binder parameter.
    pub binder_dependent_surfaces: Vec<PublicSurface>,
    /// Surfaces typed by a framework-legal open domain rather than an
    /// authored type.
    pub open_domains: Vec<PublicSurface>,
}

/// The source-backed public constructor contract of one carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VuePublicConstructorContract {
    /// Constructor origin.
    pub source: ConstructorSource,
    /// Authored binder.
    pub binder: Vec<PublicBinderParam>,
    /// Props declaration.
    pub props: DeclaredSurface,
    /// Events declaration.
    pub emits: DeclaredSurface,
    /// Models, in source order.
    pub models: Vec<PublicModel>,
    /// `defineSlots` type text.
    pub slots: Option<String>,
    /// Exposed surface.
    pub expose: ExposeSurface,
    /// Static options.
    pub options: StaticOptions,
    /// Props-parameter optionality.
    pub props_requirement: PropsRequirement,
    /// The public instance.
    pub instance: PublicInstanceProjection,
    binder_dependent: Vec<PublicSurface>,
}

impl VuePublicConstructorContract {
    /// The compatibility receipt for the rendered constructor.
    #[must_use]
    pub fn receipt(&self) -> ConstructorCompatibilityReceipt {
        let mut open_domains = Vec::new();
        if matches!(self.props, DeclaredSurface::RuntimeNames { .. }) {
            open_domains.push(PublicSurface::Props);
        }
        if matches!(self.emits, DeclaredSurface::None) && self.models.is_empty() {
            open_domains.push(PublicSurface::Events);
        }
        if self.models.iter().any(|model| model.value_type.is_none()) {
            open_domains.push(PublicSurface::Models);
        }
        if self.slots.is_none() {
            open_domains.push(PublicSurface::Slots);
        }
        let generated = self.source == ConstructorSource::ScriptSetup;
        ConstructorCompatibilityReceipt {
            construct_signatures: usize::from(generated),
            call_signatures: 0,
            props_parameter: self.props_requirement,
            binder_params: self.binder.iter().map(|p| p.name.clone()).collect(),
            binder_dependent_surfaces: self.binder_dependent.clone(),
            open_domains: if generated { open_domains } else { Vec::new() },
        }
    }

    /// The rendered public declaration: props and instance aliases, the
    /// constructor value, and its default export. `None` when the authored
    /// default export is the constructor.
    #[must_use]
    pub fn declaration(&self) -> Option<String> {
        (self.source == ConstructorSource::ScriptSetup).then(|| self.render())
    }

    fn render(&self) -> String {
        let alias_binder = self.binder_list(false);
        let construct_binder = self.binder_list(true);
        let args = if self.binder.is_empty() {
            String::new()
        } else {
            let names: Vec<&str> = self.binder.iter().map(|p| p.name.as_str()).collect();
            format!("<{}>", names.join(", "))
        };
        let mut out = String::new();
        if let DeclaredSurface::RuntimeOptions { text, .. } = &self.props {
            out.push_str(&format!("const {RUNTIME_PROPS} = ({text});\n"));
        }
        if let DeclaredSurface::RuntimeOptions { text, .. } = &self.emits {
            out.push_str(&format!("const {RUNTIME_EMITS} = ({text});\n"));
        }

        let mut props = vec!["import(\"vue\").PublicProps".to_string()];
        match &self.props {
            DeclaredSurface::None => {}
            DeclaredSurface::TypeArgument { text, .. } => props.push(format!("({text})")),
            DeclaredSurface::RuntimeNames { names } => {
                let members: Vec<String> = names
                    .iter()
                    .map(|name| format!("readonly {}?: any", quote(name)))
                    .collect();
                props.push(format!("{{ {} }}", members.join("; ")));
            }
            DeclaredSurface::RuntimeOptions { .. } => props.push(format!(
                "import(\"vue\").ExtractPublicPropTypes<typeof {RUNTIME_PROPS}>"
            )),
        }
        if !self.models.is_empty() {
            let mut members = Vec::new();
            for model in &self.models {
                let value = model.value_type.as_deref().unwrap_or("any");
                let optional = if model.required { "" } else { "?" };
                members.push(format!("{}{optional}: {value}", quote(&model.name)));
                members.push(format!(
                    "{}?: Partial<Record<string, true>>",
                    quote(&format!("{}Modifiers", model_modifiers_base(&model.name)))
                ));
                members.push(format!(
                    "{}?: (value: {value}) => void",
                    quote(&format!("onUpdate:{}", model.name))
                ));
            }
            props.push(format!("{{ {} }}", members.join("; ")));
        }
        if let Some(options) = self.emit_options() {
            props.push(format!("import(\"vue\").EmitsToProps<{options}>"));
        }
        out.push_str(&format!(
            "type {PUBLIC_PROPS}{alias_binder} = {};\n",
            props.join(" & ")
        ));

        let mut emit = Vec::new();
        if let Some(options) = self.emit_options() {
            emit.push(format!("import(\"vue\").EmitFn<{options}>"));
        }
        for model in &self.models {
            let value = model.value_type.as_deref().unwrap_or("any");
            emit.push(format!(
                "((event: {}, value: {value}) => void)",
                quote(&format!("update:{}", model.name))
            ));
        }
        if emit.is_empty() {
            emit.push("import(\"vue\").EmitFn<{}>".to_string());
        }
        let slots = match &self.slots {
            Some(text) => format!("Readonly<{text}>"),
            None => "import(\"vue\").Slots".to_string(),
        };
        out.push_str(&format!(
            "type {PUBLIC_INSTANCE}{alias_binder} = Omit<import(\"vue\").ComponentPublicInstance, \"$props\" | \"$emit\" | \"$slots\"> & {{\n  readonly $props: {PUBLIC_PROPS}{args};\n  $emit: {};\n  readonly $slots: {slots};\n}}",
            emit.join(" & ")
        ));
        if self.expose.open || !self.expose.members.is_empty() {
            out.push_str(&format!(
                " & import(\"vue\").ShallowUnwrapRef<ReturnType<typeof {EXPOSE_PROVIDER}{args}>>"
            ));
        }
        out.push_str(";\n");

        let parameter = match self.props_requirement {
            PropsRequirement::Required => format!("props: {PUBLIC_PROPS}{args}"),
            PropsRequirement::Optional => format!("props?: {PUBLIC_PROPS}{args}"),
            PropsRequirement::Undetermined => format!(
                "...args: {{}} extends {PUBLIC_PROPS}{args} ? [props?: {PUBLIC_PROPS}{args}] : [props: {PUBLIC_PROPS}{args}]"
            ),
        };
        out.push_str(&format!(
            "declare const {PUBLIC_COMPONENT}: {{\n  new {construct_binder}({parameter}): {PUBLIC_INSTANCE}{args};\n"
        ));
        if let Some(name) = &self.options.name {
            out.push_str(&format!("  readonly name: {};\n", quote(name)));
        }
        if let Some(inherit) = self.options.inherit_attrs {
            out.push_str(&format!("  readonly inheritAttrs: {inherit};\n"));
        }
        out.push_str(&format!("}};\nexport default {PUBLIC_COMPONENT};\n"));
        out
    }

    fn emit_options(&self) -> Option<String> {
        match &self.emits {
            DeclaredSurface::None => None,
            DeclaredSurface::TypeArgument { text, .. } => {
                Some(format!("import(\"vue\").TypeEmitsToOptions<({text})>"))
            }
            DeclaredSurface::RuntimeNames { names } => {
                let names: Vec<String> = names.iter().map(|name| quote(name)).collect();
                Some(format!("({})[]", names.join(" | ")))
            }
            DeclaredSurface::RuntimeOptions { .. } => Some(format!("typeof {RUNTIME_EMITS}")),
        }
    }

    fn binder_list(&self, construct: bool) -> String {
        if self.binder.is_empty() {
            return String::new();
        }
        let params: Vec<String> = self
            .binder
            .iter()
            .map(|param| {
                let mut text = String::new();
                if construct && param.is_const {
                    text.push_str("const ");
                }
                text.push_str(&param.name);
                if let Some(constraint) = &param.constraint {
                    text.push_str(" extends ");
                    text.push_str(constraint);
                }
                if let Some(default) = &param.default {
                    text.push_str(" = ");
                    text.push_str(default);
                }
                text
            })
            .collect();
        format!("<{}>", params.join(", "))
    }
}

/// Vue names the modifiers prop `modelModifiers` for the default model and
/// `<name>Modifiers` otherwise.
fn model_modifiers_base(name: &str) -> &str {
    if name == "modelValue" {
        "model"
    } else {
        name
    }
}

/// A JSON string literal, which is also a valid TypeScript string literal.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Derive the public constructor contract of one script pair. `generic` is
/// the authored `generic` attribute value.
///
/// # Errors
///
/// Refuses the inputs the statement-oriented projection refuses: a
/// non-TypeScript block, a `lang` conflict, a block with syntax errors, or a
/// `generic` attribute that does not parse.
pub fn project_public_constructor(
    normal: Option<ScriptBlockInput<'_>>,
    setup: Option<ScriptBlockInput<'_>>,
    generic: Option<&str>,
) -> Result<VuePublicConstructorContract, SetupProjectionRefusal> {
    if let (Some(n), Some(s)) = (&normal, &setup) {
        if n.lang != s.lang {
            return Err(SetupProjectionRefusal::ScriptLangConflict);
        }
    }
    let allocator = Allocator::default();
    let binder = project_binder(&allocator, generic)?;
    let normal_program = normal
        .map(|block| parse_block(&allocator, block, false))
        .transpose()?;
    let setup_program = setup
        .map(|block| parse_block(&allocator, block, true))
        .transpose()?;

    let mut contract = VuePublicConstructorContract {
        source: ConstructorSource::AuthoredDefault,
        binder,
        props: DeclaredSurface::None,
        emits: DeclaredSurface::None,
        models: Vec::new(),
        slots: None,
        expose: ExposeSurface::default(),
        options: normal_program
            .map(normal_default_options)
            .unwrap_or_default(),
        props_requirement: PropsRequirement::Optional,
        instance: PublicInstanceProjection::default(),
        binder_dependent: Vec::new(),
    };
    let (Some(program), Some(block)) = (setup_program, setup) else {
        return Ok(contract);
    };
    contract.source = ConstructorSource::ScriptSetup;

    let semantic = SemanticBuilder::new().build(program).semantic;
    let normal_bindings = normal_program
        .map(|program| value_bindings(SemanticBuilder::new().build(program).semantic.scoping()))
        .unwrap_or_default();
    let module_bound: FxHashSet<&str> = normal_bindings.iter().map(String::as_str).collect();
    let vue_macro_imports = vue_runtime_macro_imports(program);
    let binder_owned: Vec<String> = contract.binder.iter().map(|p| p.name.clone()).collect();
    let binder_names: Vec<&str> = binder_owned.iter().map(String::as_str).collect();
    let locals = LocalTypes {
        programs: [normal_program, Some(program)],
    };
    let mut collector = PublicCollector {
        macros: MacroContext {
            scoping: semantic.scoping(),
            module_bound: &module_bound,
            vue_macro_imports: &vue_macro_imports,
            macro_positions: macro_positions(program),
        },
        content: block.content,
        base: block.content_start,
        depth: 0,
        binder_names: &binder_names,
        locals: &locals,
        contract: &mut contract,
        requirement: PropsRequirement::Optional,
        dependent: FxHashSet::default(),
    };
    collector.visit_program(program);
    let requirement = collector.requirement;
    let mut dependent: Vec<PublicSurface> = collector.dependent.into_iter().collect();
    dependent.sort_unstable();
    contract.props_requirement = requirement;
    // The expose provider is instantiated with the whole binder, so a
    // generic component's exposed members follow the selected arguments.
    let exposes = !contract.expose.members.is_empty() || contract.expose.open;
    if exposes && !contract.binder.is_empty() {
        dependent.push(PublicSurface::Expose);
    }
    contract.binder_dependent = dependent;
    contract.instance = instance_projection(&contract.expose, semantic.scoping());
    Ok(contract)
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

fn project_binder(
    allocator: &Allocator,
    generic: Option<&str>,
) -> Result<Vec<PublicBinderParam>, SetupProjectionRefusal> {
    let Some(original) = generic else {
        return Ok(Vec::new());
    };
    let text = original.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let leading_offset = (text.as_ptr() as usize - original.as_ptr() as usize) as u32;
    let result = parse_generic(allocator, text, 0);
    if !result.is_ok() {
        return Err(SetupProjectionRefusal::InvalidGeneric);
    }
    let product = binder_product_from(&result, text, leading_offset);
    let consts: Vec<bool> = result
        .type_parameters()
        .map(|declaration| declaration.params.iter().map(|p| p.r#const).collect())
        .unwrap_or_default();
    let slice = |r: SourceRange| original[r.start as usize..r.end as usize].to_string();
    Ok(product
        .params
        .into_iter()
        .enumerate()
        .map(|(ordinal, param)| PublicBinderParam {
            name: param.name,
            is_const: consts.get(ordinal).copied().unwrap_or(false),
            constraint: param.constraint.map(slice),
            default: param.default.map(slice),
        })
        .collect())
}

/// Top-level type declarations of both blocks, for syntactic prop
/// optionality only.
struct LocalTypes<'a> {
    programs: [Option<&'a Program<'a>>; 2],
}

impl<'a> LocalTypes<'a> {
    fn requirement_of_name(&self, name: &str) -> PropsRequirement {
        for program in self.programs.iter().flatten() {
            for statement in &program.body {
                let declaration = match statement {
                    Statement::ExportNamedDeclaration(export) => export.declaration.as_ref(),
                    other => other.as_declaration(),
                };
                match declaration {
                    Some(Declaration::TSInterfaceDeclaration(interface))
                        if interface.id.name == name =>
                    {
                        if !interface.extends.is_empty() {
                            return PropsRequirement::Undetermined;
                        }
                        return members_requirement(&interface.body.body);
                    }
                    Some(Declaration::TSTypeAliasDeclaration(alias)) if alias.id.name == name => {
                        return match &alias.type_annotation {
                            TSType::TSTypeLiteral(literal) => members_requirement(&literal.members),
                            _ => PropsRequirement::Undetermined,
                        };
                    }
                    _ => {}
                }
            }
        }
        PropsRequirement::Undetermined
    }

    fn requirement_of_type(&self, ty: &TSType<'_>) -> PropsRequirement {
        match ty {
            TSType::TSTypeLiteral(literal) => members_requirement(&literal.members),
            TSType::TSParenthesizedType(inner) => self.requirement_of_type(&inner.type_annotation),
            TSType::TSIntersectionType(intersection) => intersection
                .types
                .iter()
                .map(|ty| self.requirement_of_type(ty))
                .fold(PropsRequirement::Optional, PropsRequirement::join),
            TSType::TSTypeReference(reference) => match &reference.type_name {
                TSTypeName::IdentifierReference(id) => self.requirement_of_name(&id.name),
                _ => PropsRequirement::Undetermined,
            },
            _ => PropsRequirement::Undetermined,
        }
    }
}

fn members_requirement(members: &[TSSignature<'_>]) -> PropsRequirement {
    let mut requirement = PropsRequirement::Optional;
    for member in members {
        let next = match member {
            TSSignature::TSPropertySignature(signature) if !signature.optional => {
                PropsRequirement::Required
            }
            TSSignature::TSMethodSignature(signature) if !signature.optional => {
                PropsRequirement::Required
            }
            TSSignature::TSPropertySignature(_)
            | TSSignature::TSMethodSignature(_)
            | TSSignature::TSIndexSignature(_) => PropsRequirement::Optional,
            _ => PropsRequirement::Undetermined,
        };
        requirement = requirement.join(next);
    }
    requirement
}

fn runtime_props_requirement(object: &ObjectExpression<'_>) -> PropsRequirement {
    let mut requirement = PropsRequirement::Optional;
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            return PropsRequirement::Undetermined;
        };
        if let Expression::ObjectExpression(options) = &property.value {
            if literal_true(options, "required") {
                requirement = PropsRequirement::Required;
            }
        }
    }
    requirement
}

fn static_key(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.to_string()),
        PropertyKey::StringLiteral(literal) => Some(literal.value.to_string()),
        _ => None,
    }
}

fn literal_property<'e, 'a>(
    object: &'e ObjectExpression<'a>,
    name: &str,
) -> Option<&'e Expression<'a>> {
    object
        .properties
        .iter()
        .find_map(|property| match property {
            ObjectPropertyKind::ObjectProperty(property)
                if !property.computed && static_key(&property.key).as_deref() == Some(name) =>
            {
                Some(&property.value)
            }
            _ => None,
        })
}

fn literal_true(object: &ObjectExpression<'_>, name: &str) -> bool {
    matches!(
        literal_property(object, name),
        Some(Expression::BooleanLiteral(literal)) if literal.value
    )
}

fn apply_static_options(object: &ObjectExpression<'_>, options: &mut StaticOptions) {
    if let Some(Expression::StringLiteral(name)) = literal_property(object, "name") {
        options.name = Some(name.value.to_string());
    }
    if let Some(Expression::BooleanLiteral(inherit)) = literal_property(object, "inheritAttrs") {
        options.inherit_attrs = Some(inherit.value);
    }
}

/// Literal static options of the normal script's default export
/// (`export default { ... }` or `export default defineComponent({ ... })`).
fn normal_default_options(program: &Program<'_>) -> StaticOptions {
    let mut options = StaticOptions::default();
    for statement in &program.body {
        let Statement::ExportDefaultDeclaration(export) = statement else {
            continue;
        };
        let object = match &export.declaration {
            ExportDefaultDeclarationKind::ObjectExpression(object) => Some(&**object),
            ExportDefaultDeclarationKind::CallExpression(call) => {
                call.arguments
                    .first()
                    .and_then(|argument| match argument.as_expression() {
                        Some(Expression::ObjectExpression(object)) => Some(&**object),
                        _ => None,
                    })
            }
            _ => None,
        };
        if let Some(object) = object {
            apply_static_options(object, &mut options);
        }
    }
    options
}

/// Setup value bindings not exposed stay private; the instance publishes the
/// typed framework members plus the exposed members.
fn instance_projection(
    expose: &ExposeSurface,
    scoping: &oxc_semantic::Scoping,
) -> PublicInstanceProjection {
    let mut members: Vec<PublicInstanceMember> = ["$props", "$emit", "$slots"]
        .into_iter()
        .map(|name| PublicInstanceMember {
            name: name.to_string(),
            origin: InstanceMemberOrigin::Framework,
        })
        .collect();
    members.extend(expose.members.iter().map(|member| PublicInstanceMember {
        name: member.name.clone(),
        origin: InstanceMemberOrigin::Exposed,
    }));
    let exposed: FxHashSet<&str> = expose.members.iter().map(|m| m.name.as_str()).collect();
    let values = value_bindings(scoping);
    let mut private: Vec<(u32, String)> = scoping
        .get_bindings(scoping.root_scope_id())
        .iter()
        .map(|(name, &id)| (scoping.symbol_span(id).start, name.as_str().to_string()))
        .filter(|(_, name)| values.contains(name) && !exposed.contains(name.as_str()))
        .collect();
    private.sort();
    PublicInstanceProjection {
        members,
        private_bindings: private.into_iter().map(|(_, name)| name).collect(),
    }
}

/// Binder-parameter references inside one authored type.
struct BinderRefs<'n> {
    names: &'n [&'n str],
    found: bool,
}

impl<'a> Visit<'a> for BinderRefs<'_> {
    fn visit_ts_type_name(&mut self, it: &TSTypeName<'a>) {
        if let TSTypeName::IdentifierReference(id) = it {
            if self.names.contains(&id.name.as_str()) {
                self.found = true;
            }
        }
        walk::walk_ts_type_name(self, it);
    }
}

struct PublicCollector<'m, 'c> {
    macros: MacroContext<'m>,
    content: &'c str,
    base: u32,
    depth: u32,
    binder_names: &'c [&'c str],
    locals: &'c LocalTypes<'c>,
    contract: &'c mut VuePublicConstructorContract,
    requirement: PropsRequirement,
    dependent: FxHashSet<PublicSurface>,
}

impl PublicCollector<'_, '_> {
    fn text(&self, span: oxc_span::Span) -> String {
        self.content[span.start as usize..span.end as usize].to_string()
    }

    fn note_binder(&mut self, surface: PublicSurface, ty: &TSType<'_>) {
        let mut refs = BinderRefs {
            names: self.binder_names,
            found: false,
        };
        refs.visit_ts_type(ty);
        if refs.found {
            self.dependent.insert(surface);
        }
    }

    fn declared(&mut self, call: &CallExpression<'_>, surface: PublicSurface) -> DeclaredSurface {
        if let Some(ty) = call.type_arguments.as_ref().and_then(|a| a.params.first()) {
            self.note_binder(surface, ty);
            if surface == PublicSurface::Props {
                let requirement = self.locals.requirement_of_type(ty);
                self.requirement = self.requirement.join(requirement);
            }
            return DeclaredSurface::TypeArgument {
                expression: range(ty.span(), self.base),
                text: self.text(ty.span()),
            };
        }
        let Some(argument) = call.arguments.first().and_then(|a| a.as_expression()) else {
            return DeclaredSurface::None;
        };
        if let Expression::ArrayExpression(array) = argument {
            let names: Option<Vec<String>> = array
                .elements
                .iter()
                .map(|element| match element {
                    ArrayExpressionElement::StringLiteral(literal) => {
                        Some(literal.value.to_string())
                    }
                    _ => None,
                })
                .collect();
            if let Some(names) = names {
                return DeclaredSurface::RuntimeNames { names };
            }
        }
        if surface == PublicSurface::Props {
            let requirement = match argument {
                Expression::ObjectExpression(object) => runtime_props_requirement(object),
                _ => PropsRequirement::Undetermined,
            };
            self.requirement = self.requirement.join(requirement);
        }
        DeclaredSurface::RuntimeOptions {
            expression: range(argument.span(), self.base),
            text: self.text(argument.span()),
        }
    }

    fn model(&mut self, call: &CallExpression<'_>) {
        let mut name = "modelValue".to_string();
        let mut required = false;
        for argument in call.arguments.iter().filter_map(|a| a.as_expression()) {
            match argument {
                Expression::StringLiteral(literal) => name = literal.value.to_string(),
                Expression::ObjectExpression(object) => {
                    required |= literal_true(object, "required")
                }
                _ => {}
            }
        }
        let value_type = call
            .type_arguments
            .as_ref()
            .and_then(|a| a.params.first())
            .map(|ty| {
                self.note_binder(PublicSurface::Models, ty);
                self.text(ty.span())
            });
        if required {
            self.requirement = PropsRequirement::Required;
        }
        self.contract.models.push(PublicModel {
            name,
            value_type,
            required,
            call: range(call.span, self.base),
        });
    }

    fn expose(&mut self, call: &CallExpression<'_>) {
        let expose = &mut self.contract.expose;
        expose.call = Some(range(call.span, self.base));
        let Some(argument) = call.arguments.first().and_then(|a| a.as_expression()) else {
            return;
        };
        let Expression::ObjectExpression(object) = argument else {
            expose.open = true;
            return;
        };
        for property in &object.properties {
            let named = match property {
                ObjectPropertyKind::ObjectProperty(property) if !property.computed => {
                    static_key(&property.key).map(|name| (name, property.key.span()))
                }
                _ => None,
            };
            match named {
                Some((name, span)) => {
                    if !expose.members.iter().any(|member| member.name == name) {
                        expose.members.push(ExposedMember {
                            name,
                            span: range(span, self.base),
                        });
                    }
                }
                None => expose.open = true,
            }
        }
    }

    fn capture(&mut self, call: &CallExpression<'_>, name: &str) {
        match name {
            "defineProps" => self.contract.props = self.declared(call, PublicSurface::Props),
            "defineEmits" => self.contract.emits = self.declared(call, PublicSurface::Events),
            "defineSlots" => {
                if let Some(ty) = call.type_arguments.as_ref().and_then(|a| a.params.first()) {
                    self.note_binder(PublicSurface::Slots, ty);
                    self.contract.slots = Some(self.text(ty.span()));
                }
            }
            "defineModel" => self.model(call),
            "defineExpose" => self.expose(call),
            "defineOptions" => {
                if let Some(Expression::ObjectExpression(object)) =
                    call.arguments.first().and_then(|a| a.as_expression())
                {
                    apply_static_options(object, &mut self.contract.options);
                }
            }
            _ => {}
        }
    }
}

impl<'a> Visit<'a> for PublicCollector<'_, '_> {
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
