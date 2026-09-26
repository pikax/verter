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
//!   is required at the constructor: a `withDefaults` default makes its prop
//!   omissible, every same-name interface declaration contributes, and a
//!   `required` flag is read through `as const` / `satisfies` / parentheses.
//!   When the syntax cannot decide (a props type named from another module,
//!   a non-literal `required`), the parameter is the conditional rest tuple
//!   so TypeScript decides — never a blanket optional parameter.
//! - `withDefaults` makes a prop omissible only through a statically named
//!   default key. Defaults whose key set is open (a spread, a computed key,
//!   a non-literal argument) are merged at runtime and leave every authored
//!   `required` in place, as Vue's compiled props do.
//! - Runtime values Vue hoists out of setup (runtime props / emits options,
//!   a model options value or `required` the syntax cannot read) are
//!   rendered as module-scope constants; one that names a binder parameter
//!   is rendered as a function over the binder and read through an
//!   instantiation expression, so the selected arguments reach it. A setup
//!   literal constant a hoisted value references is rendered ahead of it,
//!   because Vue hoists that declaration to module scope too; beside a
//!   normal script it is rendered all the same, so the value never reads as
//!   an unbound name.
//! - Exposed members come from the expose provider ([`EXPOSE_PROVIDER`]),
//!   rendered in the same declaration as the one generic function over the
//!   setup statements that returns the `defineExpose` argument, instantiated
//!   with the binder, so TypeScript types the exposed members from the
//!   authored bindings, private setup bindings never reach the instance and
//!   generic-dependent exposed members stay specialized.
//! - Authored `any` and framework-legal open domains (runtime prop names,
//!   an untyped `defineModel()`, a component without declared events) keep
//!   Vue's own typing; nothing else is widened.
//!
//! A carrier without `<script setup>` exports its authored default, which Vue
//! already types as a constructor (`defineComponent`); no generated
//! constructor replaces it.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrayExpressionElement, ArrowFunctionExpression, AwaitExpression, BindingPattern,
    CallExpression, Class, Declaration, ExportDefaultDeclarationKind, Expression, ForOfStatement,
    Function, IdentifierReference, ObjectExpression, ObjectPropertyKind, Program, PropertyKey,
    Statement, TSSignature, TSType, TSTypeName, VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use oxc_semantic::SemanticBuilder;
use oxc_span::GetSpan;
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_parser::oxc_parse::Parser;

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
/// The expose provider: a function over the authored binder whose body is
/// the setup statements and which returns the `defineExpose` argument.
/// Rendered in the public declaration whenever setup exposes members.
pub const EXPOSE_PROVIDER: &str = "__VerterExpose";
/// Hoisted runtime props options (Vue hoists `defineProps({...})` out of
/// setup, so it may only reference module scope).
pub const RUNTIME_PROPS: &str = "__VerterRuntimeProps";
/// Hoisted runtime emits options.
pub const RUNTIME_EMITS: &str = "__VerterRuntimeEmits";
/// Prefix of a hoisted non-literal `defineModel` `required` value; the model
/// ordinal follows.
pub const MODEL_REQUIRED: &str = "__VerterModelRequired";
/// Prefix of a hoisted `defineModel` options value whose `required` the
/// syntax cannot read (options that are not an object literal, or a spread
/// that may set `required`); the model ordinal follows.
pub const MODEL_OPTIONS: &str = "__VerterModelOptions";
/// Props with defaulted keys made omissible for the external argument.
pub const PROPS_WITH_DEFAULTS: &str = "__VerterPropsWithDefaults";

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
    /// `required` option: `Required` for a literal `true` (also through
    /// `as const` / `satisfies` / parentheses, on the value or the options
    /// object), `Optional` when absent or literally false, `Undetermined`
    /// when TypeScript decides from the hoisted value's or options' type.
    pub required: PropsRequirement,
    /// Call range in the carrier.
    pub call: SourceRange,
}

/// The `withDefaults` defaults of a type-declared `defineProps`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropsDefaults {
    /// Defaults argument range in the carrier.
    pub expression: SourceRange,
    /// Statically named default keys, which become omissible; `None` when a
    /// spread, a computed key or a non-literal argument leaves the key set
    /// open, so Vue merges the defaults at runtime and every authored
    /// `required` prop stays required.
    pub keys: Option<Vec<String>>,
}

/// A runtime value Vue hoists out of setup, rendered at module scope.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HoistedValue {
    name: String,
    text: String,
    /// Names a binder parameter: rendered as a function over the binder.
    generic: bool,
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
    /// `withDefaults` defaults of the props declaration.
    pub props_defaults: Option<PropsDefaults>,
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
    /// Setup literal-constant declarations a hoisted value references, in
    /// source order; rendered at module scope ahead of the hoisted values.
    hoisted_consts: Vec<String>,
    hoisted: Vec<HoistedValue>,
    /// Setup statements (imports excluded, `export` modifiers dropped) in
    /// source order: the expose provider's body.
    setup_statements: Vec<String>,
    /// Setup awaits at top level, so the provider is `async`.
    setup_is_async: bool,
    /// The `defineExpose` argument text the provider returns.
    expose_argument: Option<String>,
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

    /// The rendered public declaration: the hoisted runtime values, the
    /// expose provider, the props and instance aliases, the constructor
    /// value, and its default export. `None` when the authored default export
    /// is the constructor.
    #[must_use]
    pub fn declaration(&self) -> Option<String> {
        (self.source == ConstructorSource::ScriptSetup).then(|| self.render())
    }

    fn render(&self) -> String {
        let alias_binder = self.binder_list(BinderSite::Alias);
        let construct_binder = self.binder_list(BinderSite::Construct);
        let args = self.binder_args();
        let mut out = String::new();
        for declaration in &self.hoisted_consts {
            out.push_str(declaration);
            out.push('\n');
        }
        for value in &self.hoisted {
            if value.generic {
                out.push_str(&format!(
                    "const {} = {}() => ({});\n",
                    value.name,
                    self.binder_list(BinderSite::Arrow),
                    value.text
                ));
            } else {
                out.push_str(&format!("const {} = ({});\n", value.name, value.text));
            }
        }
        let provider = self.render_expose_provider(&mut out);
        if self.defaults_apply() {
            // Key-remapped mapped types over `keyof P` keep each member's
            // authored modifiers; a defaulted key only gains `?`.
            out.push_str(&format!(
                "type {PROPS_WITH_DEFAULTS}<P, K extends PropertyKey> = {{ [Q in keyof P as Q extends K ? never : Q]: P[Q] }} & {{ [Q in keyof P as Q extends K ? Q : never]?: P[Q] }};\n"
            ));
        }

        let mut props = vec!["import(\"vue\").PublicProps".to_string()];
        match &self.props {
            DeclaredSurface::None => {}
            DeclaredSurface::TypeArgument { text, .. } => props.push(self.defaulted(text)),
            DeclaredSurface::RuntimeNames { names } => {
                let members: Vec<String> = names
                    .iter()
                    .map(|name| format!("readonly {}?: any", quote(name)))
                    .collect();
                props.push(format!("{{ {} }}", members.join("; ")));
            }
            DeclaredSurface::RuntimeOptions { .. } => props.push(format!(
                "import(\"vue\").ExtractPublicPropTypes<{}>",
                self.hoisted_type(RUNTIME_PROPS)
            )),
        }
        if !self.models.is_empty() {
            let mut members = Vec::new();
            let mut undetermined = Vec::new();
            for (ordinal, model) in self.models.iter().enumerate() {
                let value = model.value_type.as_deref().unwrap_or("any");
                let key = quote(&model.name);
                match model.required {
                    PropsRequirement::Required => members.push(format!("{key}: {value}")),
                    PropsRequirement::Optional => members.push(format!("{key}?: {value}")),
                    // Vue's `required` is read from the hoisted value's or
                    // options' type, exactly as the runtime props options are.
                    PropsRequirement::Undetermined => {
                        let required = format!("{MODEL_REQUIRED}{ordinal}");
                        let (hoisted, test) = if self.hoisted.iter().any(|h| h.name == required) {
                            (required, "true")
                        } else {
                            (format!("{MODEL_OPTIONS}{ordinal}"), "{ required: true }")
                        };
                        undetermined.push(format!(
                            "({} extends {test} ? {{ {key}: {value} }} : {{ {key}?: {value} }})",
                            self.hoisted_type(&hoisted)
                        ));
                    }
                }
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
            props.extend(undetermined);
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
        if provider {
            let exposed = format!("ReturnType<typeof {EXPOSE_PROVIDER}{args}>");
            let exposed = if self.setup_is_async {
                format!("Awaited<{exposed}>")
            } else {
                exposed
            };
            out.push_str(&format!(" & import(\"vue\").ShallowUnwrapRef<{exposed}>"));
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

    /// The expose provider: the setup statements inside one function over
    /// the binder, returning the `defineExpose` argument, so TypeScript types
    /// the exposed members from the authored bindings. Returns whether it was
    /// rendered.
    fn render_expose_provider(&self, out: &mut String) -> bool {
        let Some(argument) = &self.expose_argument else {
            return false;
        };
        if !self.expose.open && self.expose.members.is_empty() {
            return false;
        }
        let asyncness = if self.setup_is_async { "async " } else { "" };
        out.push_str(&format!(
            "{asyncness}function {EXPOSE_PROVIDER}{}() {{\n",
            self.binder_list(BinderSite::Construct)
        ));
        for statement in &self.setup_statements {
            out.push_str(statement);
            out.push('\n');
        }
        out.push_str(&format!("return ({argument});\n}}\n"));
        true
    }

    /// The statically named `withDefaults` keys that make props of the
    /// type-declared props omissible; empty when none apply.
    fn defaulted_keys(&self) -> &[String] {
        match (&self.props, &self.props_defaults) {
            (
                DeclaredSurface::TypeArgument { .. },
                Some(PropsDefaults {
                    keys: Some(keys), ..
                }),
            ) => keys,
            _ => &[],
        }
    }

    /// Whether `withDefaults` makes any key of the type-declared props
    /// omissible.
    fn defaults_apply(&self) -> bool {
        !self.defaulted_keys().is_empty()
    }

    /// The authored props type, with `withDefaults` keys made omissible.
    fn defaulted(&self, text: &str) -> String {
        let keys = self.defaulted_keys();
        if keys.is_empty() {
            return format!("({text})");
        }
        let keys: Vec<String> = keys.iter().map(|key| quote(key)).collect();
        format!("{PROPS_WITH_DEFAULTS}<({text}), {}>", keys.join(" | "))
    }

    /// The type of one hoisted value, instantiated with the binder when it
    /// names a binder parameter.
    fn hoisted_type(&self, name: &str) -> String {
        match self.hoisted.iter().find(|value| value.name == name) {
            Some(value) if value.generic => {
                format!("ReturnType<typeof {name}{}>", self.binder_args())
            }
            _ => format!("typeof {name}"),
        }
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
            DeclaredSurface::RuntimeOptions { .. } => Some(self.hoisted_type(RUNTIME_EMITS)),
        }
    }

    /// `<T, U>`: the binder parameter names as type arguments.
    fn binder_args(&self) -> String {
        if self.binder.is_empty() {
            return String::new();
        }
        let names: Vec<&str> = self.binder.iter().map(|p| p.name.as_str()).collect();
        format!("<{}>", names.join(", "))
    }

    fn binder_list(&self, site: BinderSite) -> String {
        if self.binder.is_empty() {
            return String::new();
        }
        // Aliases cannot carry `const`; every function site keeps it.
        let with_const = site != BinderSite::Alias;
        let params: Vec<String> = self
            .binder
            .iter()
            .map(|param| {
                let mut text = String::new();
                if with_const && param.is_const {
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
        // An arrow's trailing comma keeps `<T,>() =>` a type parameter list
        // under the TSX grammar too.
        let trailing = if site == BinderSite::Arrow { "," } else { "" };
        format!("<{}{trailing}>", params.join(", "))
    }
}

/// Where a binder parameter list is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinderSite {
    /// A type alias: no `const` modifiers.
    Alias,
    /// The construct signature or the expose provider.
    Construct,
    /// A hoisted binder-dependent value's arrow function.
    Arrow,
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
        props_defaults: None,
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
        hoisted_consts: Vec::new(),
        hoisted: Vec::new(),
        setup_statements: Vec::new(),
        setup_is_async: false,
        expose_argument: None,
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
    // A hoisted value keeps every setup literal constant it references bound,
    // so its type is read rather than an unbound name. Vue hoists these only
    // beside no normal script (it rejects the reference otherwise), but the
    // declaration still renders them so a `required` flag keeps its meaning;
    // one that shadows a normal-script binding would redeclare it and is
    // left to that binding.
    let mut literal_consts = literal_consts(program, block.content);
    literal_consts
        .retain(|symbol, _| !module_bound.contains(semantic.scoping().symbol_name(*symbol)));
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
        pending_defaults: None,
        top_level_await: false,
        literal_consts: &literal_consts,
        used_consts: FxHashSet::default(),
    };
    collector.visit_program(program);
    let requirement = collector.requirement;
    let top_level_await = collector.top_level_await;
    let mut used_consts: Vec<&(u32, String)> = collector
        .used_consts
        .iter()
        .filter_map(|symbol| literal_consts.get(symbol))
        .collect();
    let mut dependent: Vec<PublicSurface> = collector.dependent.into_iter().collect();
    dependent.sort_unstable();
    used_consts.sort_unstable();
    contract.hoisted_consts = used_consts.into_iter().map(|(_, d)| d.clone()).collect();
    contract.props_requirement = requirement;
    contract.setup_is_async = top_level_await;
    // The expose provider is instantiated with the whole binder, so a
    // generic component's exposed members follow the selected arguments.
    let exposes = !contract.expose.members.is_empty() || contract.expose.open;
    if exposes {
        contract.setup_statements = provider_statements(program, block.content);
        if !contract.binder.is_empty() {
            dependent.push(PublicSurface::Expose);
        }
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

/// The expose provider's body: the setup statements in source order. Imports
/// and ambient `declare` statements live at module scope; an `export`
/// modifier is dropped so the declaration stays a function-local statement.
fn provider_statements(program: &Program<'_>, content: &str) -> Vec<String> {
    let text = |span: oxc_span::Span| content[span.start as usize..span.end as usize].to_string();
    program
        .body
        .iter()
        .filter_map(|statement| match statement {
            Statement::ImportDeclaration(_)
            | Statement::ExportAllDeclaration(_)
            | Statement::ExportDefaultDeclaration(_)
            | Statement::TSExportAssignment(_)
            | Statement::TSNamespaceExportDeclaration(_) => None,
            Statement::ExportNamedDeclaration(export) => export
                .declaration
                .as_ref()
                .filter(|declaration| !declaration.declare())
                .map(|declaration| text(declaration.span())),
            other => match other.as_declaration() {
                Some(declaration) if declaration.declare() => None,
                Some(Declaration::TSModuleDeclaration(_) | Declaration::TSGlobalDeclaration(_)) => {
                    None
                }
                _ => Some(text(other.span())),
            },
        })
        .collect()
}

/// Top-level type declarations of both blocks, for syntactic prop
/// optionality only.
struct LocalTypes<'a> {
    programs: [Option<&'a Program<'a>>; 2],
}

impl<'a> LocalTypes<'a> {
    /// Joins every same-name declaration: interfaces merge across both
    /// blocks, so a required member in any of them is required.
    fn requirement_of_name(&self, name: &str, defaulted: &[String]) -> PropsRequirement {
        let mut found: Option<PropsRequirement> = None;
        for program in self.programs.iter().flatten() {
            for statement in &program.body {
                let declaration = match statement {
                    Statement::ExportNamedDeclaration(export) => export.declaration.as_ref(),
                    other => other.as_declaration(),
                };
                let requirement = match declaration {
                    Some(Declaration::TSInterfaceDeclaration(interface))
                        if interface.id.name == name =>
                    {
                        if interface.extends.is_empty() {
                            members_requirement(&interface.body.body, defaulted)
                        } else {
                            PropsRequirement::Undetermined
                        }
                    }
                    Some(Declaration::TSTypeAliasDeclaration(alias)) if alias.id.name == name => {
                        match &alias.type_annotation {
                            TSType::TSTypeLiteral(literal) => {
                                members_requirement(&literal.members, defaulted)
                            }
                            _ => PropsRequirement::Undetermined,
                        }
                    }
                    _ => continue,
                };
                found = Some(found.map_or(requirement, |seen| seen.join(requirement)));
            }
        }
        found.unwrap_or(PropsRequirement::Undetermined)
    }

    fn requirement_of_type(&self, ty: &TSType<'_>, defaulted: &[String]) -> PropsRequirement {
        match ty {
            TSType::TSTypeLiteral(literal) => members_requirement(&literal.members, defaulted),
            TSType::TSParenthesizedType(inner) => {
                self.requirement_of_type(&inner.type_annotation, defaulted)
            }
            TSType::TSIntersectionType(intersection) => intersection
                .types
                .iter()
                .map(|ty| self.requirement_of_type(ty, defaulted))
                .fold(PropsRequirement::Optional, PropsRequirement::join),
            TSType::TSTypeReference(reference) => match &reference.type_name {
                TSTypeName::IdentifierReference(id) => {
                    self.requirement_of_name(&id.name, defaulted)
                }
                _ => PropsRequirement::Undetermined,
            },
            _ => PropsRequirement::Undetermined,
        }
    }
}

/// A member is required unless it is optional or has a `withDefaults`
/// default (`defaulted`), which makes it omissible for the caller.
fn members_requirement(members: &[TSSignature<'_>], defaulted: &[String]) -> PropsRequirement {
    let is_defaulted = |key: &PropertyKey<'_>, computed: bool| {
        !computed && static_key(key).is_some_and(|name| defaulted.contains(&name))
    };
    let mut requirement = PropsRequirement::Optional;
    for member in members {
        let next = match member {
            TSSignature::TSPropertySignature(signature)
                if !signature.optional && !is_defaulted(&signature.key, signature.computed) =>
            {
                PropsRequirement::Required
            }
            TSSignature::TSMethodSignature(signature)
                if !signature.optional && !is_defaulted(&signature.key, signature.computed) =>
            {
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

/// Runtime props options: Vue's public props type requires exactly the
/// options whose `required` type is `true`; a value the syntax cannot read
/// leaves the decision to TypeScript over the hoisted options.
fn runtime_props_requirement(object: &ObjectExpression<'_>) -> PropsRequirement {
    let mut requirement = PropsRequirement::Optional;
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            return PropsRequirement::Undetermined;
        };
        let next = match &property.value {
            Expression::ObjectExpression(options) => required_option(options),
            // A constructor list or `null` declares the type only.
            Expression::ArrayExpression(_) | Expression::NullLiteral(_) => {
                PropsRequirement::Optional
            }
            _ => PropsRequirement::Undetermined,
        };
        requirement = requirement.join(next);
    }
    requirement
}

/// The `required` option of one prop options object. A spread that may set
/// it leaves the decision to TypeScript.
fn required_option(object: &ObjectExpression<'_>) -> PropsRequirement {
    match required_decider(object) {
        None => PropsRequirement::Optional,
        Some(ObjectPropertyKind::ObjectProperty(property)) => {
            match literal_boolean(&property.value) {
                Some(true) => PropsRequirement::Required,
                Some(false) => PropsRequirement::Optional,
                None => PropsRequirement::Undetermined,
            }
        }
        Some(ObjectPropertyKind::SpreadProperty(_)) => PropsRequirement::Undetermined,
    }
}

/// Whether an options member may set `required`: a spread or a `required`
/// property.
fn decides_required(property: &ObjectPropertyKind<'_>) -> bool {
    match property {
        ObjectPropertyKind::SpreadProperty(_) => true,
        ObjectPropertyKind::ObjectProperty(property) => {
            static_key(&property.key).as_deref() == Some("required")
        }
    }
}

/// The member whose value `required` takes: the last spread or `required`
/// property.
fn required_decider<'e, 'a>(
    object: &'e ObjectExpression<'a>,
) -> Option<&'e ObjectPropertyKind<'a>> {
    object
        .properties
        .iter()
        .rev()
        .find(|property| decides_required(property))
}

/// An options object literal through the wrappers that keep its literal
/// member types: parentheses, `as const` / `<const>`, `satisfies` and `!`.
fn options_object<'e, 'a>(expression: &'e Expression<'a>) -> Option<&'e ObjectExpression<'a>> {
    match expression {
        Expression::ObjectExpression(object) => Some(object),
        Expression::ParenthesizedExpression(inner) => options_object(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => options_object(&inner.expression),
        Expression::TSNonNullExpression(inner) => options_object(&inner.expression),
        Expression::TSAsExpression(inner) if inner.type_annotation.is_const_type_reference() => {
            options_object(&inner.expression)
        }
        Expression::TSTypeAssertion(inner) if inner.type_annotation.is_const_type_reference() => {
            options_object(&inner.expression)
        }
        _ => None,
    }
}

/// Vue's static initializer (`isStaticNode`): literals and operators over
/// them, through parentheses and TypeScript expression wrappers.
fn is_static_node(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        Expression::ParenthesizedExpression(inner) => is_static_node(&inner.expression),
        Expression::TSAsExpression(inner) => is_static_node(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => is_static_node(&inner.expression),
        Expression::TSNonNullExpression(inner) => is_static_node(&inner.expression),
        Expression::TSTypeAssertion(inner) => is_static_node(&inner.expression),
        Expression::TSInstantiationExpression(inner) => is_static_node(&inner.expression),
        Expression::UnaryExpression(inner) => is_static_node(&inner.argument),
        Expression::BinaryExpression(inner) => {
            is_static_node(&inner.left) && is_static_node(&inner.right)
        }
        Expression::LogicalExpression(inner) => {
            is_static_node(&inner.left) && is_static_node(&inner.right)
        }
        Expression::ConditionalExpression(inner) => {
            is_static_node(&inner.test)
                && is_static_node(&inner.consequent)
                && is_static_node(&inner.alternate)
        }
        Expression::SequenceExpression(inner) => inner.expressions.iter().all(is_static_node),
        Expression::TemplateLiteral(inner) => inner.expressions.iter().all(is_static_node),
        _ => false,
    }
}

/// Setup literal constants: each top-level `const` identifier with a static
/// initializer, which Vue hoists to module scope, keyed by symbol with its
/// source offset and its rendered declaration.
fn literal_consts(program: &Program<'_>, content: &str) -> FxHashMap<SymbolId, (u32, String)> {
    let mut consts = FxHashMap::default();
    for statement in &program.body {
        let Statement::VariableDeclaration(declaration) = statement else {
            continue;
        };
        if declaration.kind != VariableDeclarationKind::Const || declaration.declare {
            continue;
        }
        for declarator in &declaration.declarations {
            let (BindingPattern::BindingIdentifier(id), Some(init)) =
                (&declarator.id, &declarator.init)
            else {
                continue;
            };
            let Some(symbol) = id.symbol_id.get().filter(|_| is_static_node(init)) else {
                continue;
            };
            let span = declarator.span;
            let text = &content[span.start as usize..span.end as usize];
            consts.insert(symbol, (span.start, format!("const {text};")));
        }
    }
    consts
}

/// A boolean literal whose TypeScript type stays that literal: parentheses,
/// `as const` / `<const>`, `satisfies` and `!` keep it; any other assertion
/// may widen it (`true as boolean`), so the syntax does not decide.
fn literal_boolean(expression: &Expression<'_>) -> Option<bool> {
    match expression {
        Expression::BooleanLiteral(literal) => Some(literal.value),
        Expression::ParenthesizedExpression(inner) => literal_boolean(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => literal_boolean(&inner.expression),
        Expression::TSNonNullExpression(inner) => literal_boolean(&inner.expression),
        Expression::TSAsExpression(inner) if inner.type_annotation.is_const_type_reference() => {
            literal_boolean(&inner.expression)
        }
        Expression::TSTypeAssertion(inner) if inner.type_annotation.is_const_type_reference() => {
            literal_boolean(&inner.expression)
        }
        _ => None,
    }
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

/// Statically named keys of a defaults object; `None` when a spread, a
/// non-literal computed key or a non-literal argument leaves the key set
/// open.
fn static_default_keys(expression: &Expression<'_>) -> Option<Vec<String>> {
    let Expression::ObjectExpression(object) = expression.without_parentheses() else {
        return None;
    };
    object
        .properties
        .iter()
        .map(|property| match property {
            ObjectPropertyKind::ObjectProperty(property) => static_key(&property.key),
            ObjectPropertyKind::SpreadProperty(_) => None,
        })
        .collect()
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

/// Binder-parameter references inside one authored type or hoisted value.
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

/// Binder-parameter and setup literal-constant references inside one hoisted
/// value.
struct ValueRefs<'n, 's> {
    binder_names: &'n [&'n str],
    scoping: &'s oxc_semantic::Scoping,
    literal_consts: &'s FxHashMap<SymbolId, (u32, String)>,
    /// Names a binder parameter.
    generic: bool,
    /// Referenced literal constants.
    consts: Vec<SymbolId>,
}

impl<'a> Visit<'a> for ValueRefs<'_, '_> {
    fn visit_ts_type_name(&mut self, it: &TSTypeName<'a>) {
        if let TSTypeName::IdentifierReference(id) = it {
            if self.binder_names.contains(&id.name.as_str()) {
                self.generic = true;
            }
        }
        walk::walk_ts_type_name(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        let symbol = it
            .reference_id
            .get()
            .and_then(|reference| self.scoping.get_reference(reference).symbol_id());
        if let Some(symbol) = symbol.filter(|s| self.literal_consts.contains_key(s)) {
            self.consts.push(symbol);
        }
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
    /// `withDefaults` first-argument span and its defaults, awaiting the
    /// wrapped `defineProps` the walk visits next.
    pending_defaults: Option<(oxc_span::Span, PropsDefaults)>,
    /// Setup awaits at top level.
    top_level_await: bool,
    /// Setup literal constants Vue hoists to module scope.
    literal_consts: &'c FxHashMap<SymbolId, (u32, String)>,
    /// Literal constants a hoisted value references.
    used_consts: FxHashSet<SymbolId>,
}

impl PublicCollector<'_, '_> {
    fn text(&self, span: oxc_span::Span) -> String {
        self.content[span.start as usize..span.end as usize].to_string()
    }

    fn names_binder_in_type(&self, ty: &TSType<'_>) -> bool {
        let mut refs = BinderRefs {
            names: self.binder_names,
            found: false,
        };
        refs.visit_ts_type(ty);
        refs.found
    }

    fn note_binder(&mut self, surface: PublicSurface, ty: &TSType<'_>) {
        if self.names_binder_in_type(ty) {
            self.dependent.insert(surface);
        }
    }

    /// Hoist one runtime value Vue moves out of setup. A value naming a
    /// binder parameter is rendered over the binder and reaches `surface`.
    /// `contextual` is the type Vue checks the value against in place; the
    /// hoisted value `satisfies` it so its literal members (a `required:
    /// true`) keep their literal types instead of widening.
    fn hoist(
        &mut self,
        name: String,
        expression: &Expression<'_>,
        surface: PublicSurface,
        contextual: Option<&str>,
    ) {
        let text = self.text(expression.span());
        self.hoist_scanned(name, text, surface, contextual, |refs| {
            refs.visit_expression(expression);
        });
    }

    /// Hoist `text`, whose binder and literal-constant references `scan`
    /// visits.
    fn hoist_scanned(
        &mut self,
        name: String,
        text: String,
        surface: PublicSurface,
        contextual: Option<&str>,
        scan: impl FnOnce(&mut ValueRefs<'_, '_>),
    ) {
        let mut refs = ValueRefs {
            binder_names: self.binder_names,
            scoping: self.macros.scoping,
            literal_consts: self.literal_consts,
            generic: false,
            consts: Vec::new(),
        };
        scan(&mut refs);
        let generic = refs.generic;
        self.used_consts.extend(refs.consts);
        if generic {
            self.dependent.insert(surface);
        }
        let text = match contextual {
            Some(contextual) => format!("({text}) satisfies {contextual}"),
            None => text,
        };
        self.contract.hoisted.retain(|value| value.name != name);
        self.contract.hoisted.push(HoistedValue {
            name,
            text,
            generic,
        });
    }

    fn declared(&mut self, call: &CallExpression<'_>, surface: PublicSurface) -> DeclaredSurface {
        let defaults = match self.pending_defaults.take() {
            Some((span, defaults))
                if surface == PublicSurface::Props
                    && span.start <= call.span.start
                    && call.span.end <= span.end =>
            {
                Some(defaults)
            }
            other => {
                self.pending_defaults = other;
                None
            }
        };
        if let Some(ty) = call.type_arguments.as_ref().and_then(|a| a.params.first()) {
            self.note_binder(surface, ty);
            if surface == PublicSurface::Props {
                let defaulted = defaults
                    .as_ref()
                    .and_then(|d| d.keys.clone())
                    .unwrap_or_default();
                // Open default keys make nothing omissible: Vue merges them
                // at runtime and keeps every authored `required`.
                let requirement = self.locals.requirement_of_type(ty, &defaulted);
                self.requirement = self.requirement.join(requirement);
                self.contract.props_defaults = defaults;
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
        let (hoisted, contextual) = if surface == PublicSurface::Props {
            let requirement = match argument {
                Expression::ObjectExpression(object) => runtime_props_requirement(object),
                _ => PropsRequirement::Undetermined,
            };
            self.requirement = self.requirement.join(requirement);
            (
                RUNTIME_PROPS,
                Some("import(\"vue\").ComponentObjectPropsOptions"),
            )
        } else {
            (RUNTIME_EMITS, None)
        };
        self.hoist(hoisted.to_string(), argument, surface, contextual);
        DeclaredSurface::RuntimeOptions {
            expression: range(argument.span(), self.base),
            text: self.text(argument.span()),
        }
    }

    /// `withDefaults(defineProps<T>(), defaults)`: record the defaults for
    /// the wrapped `defineProps`.
    fn with_defaults(&mut self, call: &CallExpression<'_>) {
        let mut arguments = call.arguments.iter().filter_map(|a| a.as_expression());
        let (Some(props), Some(defaults)) = (arguments.next(), arguments.next()) else {
            return;
        };
        self.pending_defaults = Some((
            props.span(),
            PropsDefaults {
                expression: range(defaults.span(), self.base),
                keys: static_default_keys(defaults),
            },
        ));
    }

    fn model(&mut self, call: &CallExpression<'_>) {
        let ordinal = self.contract.models.len();
        let mut name = "modelValue".to_string();
        // Vue reads a leading string literal as the name; the next argument
        // is the options.
        let mut arguments = call.arguments.iter().filter_map(|a| a.as_expression());
        let mut options = arguments.next();
        if let Some(Expression::StringLiteral(literal)) = options {
            name = literal.value.to_string();
            options = arguments.next();
        }
        let required = options.map_or(PropsRequirement::Optional, |options| {
            self.model_required(ordinal, options)
        });
        let value_type = call
            .type_arguments
            .as_ref()
            .and_then(|a| a.params.first())
            .map(|ty| {
                self.note_binder(PublicSurface::Models, ty);
                self.text(ty.span())
            });
        self.requirement = self.requirement.join(required);
        self.contract.models.push(PublicModel {
            name,
            value_type,
            required,
            call: range(call.span, self.base),
        });
    }

    /// The `required` option of one model's options. A literal flag is read
    /// directly; otherwise the value, or the options members that decide it,
    /// are hoisted so TypeScript reads `required` from their type.
    fn model_required(&mut self, ordinal: usize, options: &Expression<'_>) -> PropsRequirement {
        let Some(object) = options_object(options) else {
            self.hoist(
                format!("{MODEL_OPTIONS}{ordinal}"),
                options,
                PublicSurface::Models,
                None,
            );
            return PropsRequirement::Undetermined;
        };
        match required_decider(object) {
            None => PropsRequirement::Optional,
            Some(ObjectPropertyKind::ObjectProperty(property)) => {
                match literal_boolean(&property.value) {
                    Some(true) => PropsRequirement::Required,
                    Some(false) => PropsRequirement::Optional,
                    None => {
                        self.hoist(
                            format!("{MODEL_REQUIRED}{ordinal}"),
                            &property.value,
                            PublicSurface::Models,
                            None,
                        );
                        PropsRequirement::Undetermined
                    }
                }
            }
            Some(ObjectPropertyKind::SpreadProperty(_)) => {
                // Only spreads and `required` members decide the flag; the
                // rest (`get` / `set` may close over setup) stay in setup.
                let deciding: Vec<&ObjectPropertyKind<'_>> = object
                    .properties
                    .iter()
                    .filter(|property| decides_required(property))
                    .collect();
                let members: Vec<String> = deciding
                    .iter()
                    .map(|property| self.text(property.span()))
                    .collect();
                self.hoist_scanned(
                    format!("{MODEL_OPTIONS}{ordinal}"),
                    format!("{{ {} }}", members.join(", ")),
                    PublicSurface::Models,
                    Some("{ readonly required?: boolean; readonly [key: string]: unknown }"),
                    |refs| {
                        for property in deciding {
                            refs.visit_object_property_kind(property);
                        }
                    },
                );
                PropsRequirement::Undetermined
            }
        }
    }

    fn expose(&mut self, call: &CallExpression<'_>) {
        let argument = call.arguments.first().and_then(|a| a.as_expression());
        self.contract.expose_argument = argument.map(|argument| self.text(argument.span()));
        let expose = &mut self.contract.expose;
        expose.call = Some(range(call.span, self.base));
        let Some(argument) = argument else {
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
            "withDefaults" => self.with_defaults(call),
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

    fn visit_await_expression(&mut self, it: &AwaitExpression<'a>) {
        if self.depth == 0 {
            self.top_level_await = true;
        }
        walk::walk_await_expression(self, it);
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        if self.depth == 0 && it.r#await {
            self.top_level_await = true;
        }
        walk::walk_for_of_statement(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Some(name) = self.macros.resolve(it, self.depth) {
            self.capture(it, name);
        }
        walk::walk_call_expression(self, it);
    }
}
