//! Caller prop checks and default-resolved setup contracts.
//!
//! TypeScript remains the type-answer owner. This module supplies the
//! projection facts that decide which authored contributions are checked and
//! which keys a spread may omit because a later write certainly replaces it.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrayExpression, BindingPattern, CallExpression, Declaration, Expression, ObjectExpression,
    ObjectPropertyKind, Program, PropertyKey, Statement, TSLiteral, TSSignature, TSType,
    TSTypeName,
};
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::framework_common::projection_plan::ComponentUseId;
use crate::ide::vue_projection::attribute_operations::{
    AttributeOperationsProjection, AttributeSyntax,
};
use crate::ide::vue_projection::public_constructor::{
    DeclaredSurface, PropsDefaults, VuePublicConstructorContract,
};

/// Policy used for an object `v-bind` contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadCertaintyPolicy {
    /// A finite key set is checked against the component's declared props.
    /// Keys proven overwritten by a later definite write are excluded from
    /// that check.
    CheckKnownKeys,
    /// An index-signature spread is framework-legal and cannot be made exact
    /// without inventing a type answer.
    PreserveOpenDomain,
}

/// One caller-side prop check TypeScript must perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropCheckObligation {
    /// Component use that owns the check.
    pub use_id: ComponentUseId,
    /// Authored `v-bind` operation that contributes the spread.
    pub op_index: u32,
    /// Keys whose values cannot reach the final component props because a
    /// later definite write replaces them.
    pub overwritten_keys: Vec<String>,
    /// Certainty policy supplied to the generated TypeScript helper.
    pub policy: SpreadCertaintyPolicy,
}

impl PropCheckObligation {
    /// Whether this obligation validates finite, statically knowable keys.
    #[must_use]
    pub fn checks_known_keys(&self) -> bool {
        self.policy == SpreadCertaintyPolicy::CheckKnownKeys
    }
}

/// Caller optionality and setup-side default resolution are distinct facts.
///
/// Vue 3.6 resolves a caller omission into a defined setup value for a
/// static `withDefaults` key, a reactive destructuring default, a runtime
/// `default`, and a non-required Boolean prop (absence casts to `false`).
/// A `required: true` prop stays required for callers even when a default
/// or Boolean cast also fills a setup value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallerAndSetupPropsContract {
    /// Keys callers may omit because a static default or a non-required
    /// Boolean cast supplies the value.
    pub caller_optional_keys: Vec<String>,
    /// Keys setup reads as defined after defaults and Boolean absence
    /// casting. A required prop with a default is here and still required.
    pub setup_defined_keys: Vec<String>,
    /// False when the `withDefaults` object has an open key set. An open
    /// set cannot prove any individual caller optionality; authored
    /// required props stay required.
    pub defaults_are_static: bool,
    /// Keys defaulted by reactive destructuring
    /// (`const { title = "x" } = defineProps<...>()`). Empty when
    /// `withDefaults` wraps the call: Vue disables reactive destructure
    /// there.
    pub reactive_default_keys: Vec<String>,
    /// Keys whose runtime type includes `Boolean`, so an absent value
    /// resolves to `false`.
    pub boolean_cast_keys: Vec<String>,
    /// Boolean-cast keys whose constructor list names `String` before
    /// `Boolean`, so an empty string stays a string.
    pub boolean_empty_string_keys: Vec<String>,
    /// Runtime props that declare a `validator`.
    pub validator_keys: Vec<String>,
    /// Keys that stay required for callers after defaults are applied.
    pub caller_required_keys: Vec<String>,
    /// False when the props type or runtime options could not be
    /// enumerated. An empty [`Self::caller_required_keys`] is then not
    /// proof that every prop is optional.
    pub required_keys_are_static: bool,
}

impl CallerAndSetupPropsContract {
    /// Type aliases the qualification probe pins. Key order is the order
    /// the facts were discovered.
    #[must_use]
    pub fn witness_types(&self) -> String {
        format!(
            "type __VerterCallerOptionalKeys = {};\n\
             type __VerterSetupDefinedKeys = {};\n\
             type __VerterCallerRequiredKeys = {};\n\
             type __VerterReactiveDefaultKeys = {};\n\
             type __VerterBooleanCastKeys = {};\n\
             type __VerterBooleanEmptyStringKeys = {};\n\
             type __VerterValidatorKeys = {};\n",
            key_union(&self.caller_optional_keys),
            key_union(&self.setup_defined_keys),
            key_union(&self.caller_required_keys),
            key_union(&self.reactive_default_keys),
            key_union(&self.boolean_cast_keys),
            key_union(&self.boolean_empty_string_keys),
            key_union(&self.validator_keys),
        )
    }
}

/// Prop-check products of an admitted projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropsProjection {
    /// False when attribute facts were incomplete. Incomplete products never
    /// warm a complete cache entry.
    pub complete: bool,
    /// Spread checks in authored order.
    pub obligations: Vec<PropCheckObligation>,
}

/// Derive caller prop-check obligations from the runtime property plan.
#[must_use]
pub fn project_props(attributes: &AttributeOperationsProjection) -> PropsProjection {
    let mut obligations = Vec::new();
    for (sequence, key_plan) in attributes.sequences.iter().zip(&attributes.key_plans) {
        for operation in &sequence.operations {
            if operation.syntax != AttributeSyntax::BindObject {
                continue;
            }
            // `overridden` already proves this spread's value is dead. A later
            // possible writer can replace the definite write; it cannot restore
            // the earlier value, so the key stays out of this spread's check.
            let overwritten_keys = key_plan
                .effective
                .iter()
                .filter(|property| property.overridden.contains(&operation.index))
                .map(|property| property.key.clone())
                .collect();
            obligations.push(PropCheckObligation {
                use_id: sequence.use_id.clone(),
                op_index: operation.index,
                overwritten_keys,
                policy: SpreadCertaintyPolicy::CheckKnownKeys,
            });
        }
    }
    PropsProjection {
        complete: attributes.complete,
        obligations,
    }
}

/// Derive the caller/setup split from the public constructor's macro facts
/// and the authored script. `sources` are the normal and setup block texts;
/// a block that fails to parse leaves required keys unenumerated.
#[must_use]
pub fn caller_and_setup_props<'a>(
    contract: &VuePublicConstructorContract,
    sources: impl IntoIterator<Item = &'a str>,
) -> CallerAndSetupPropsContract {
    let static_defaults = matches!(contract.props, DeclaredSurface::TypeArgument { .. });
    let with_default_keys = match &contract.props_defaults {
        Some(PropsDefaults {
            keys: Some(keys), ..
        }) if static_defaults => keys.clone(),
        _ => Vec::new(),
    };
    let mut facts = ScriptPropFacts::default();
    let mut saw_source = false;
    let mut parse_failed = false;
    for source in sources {
        saw_source = true;
        if !facts.absorb(source) {
            parse_failed = true;
        }
    }
    if !facts.saw_props {
        facts.enumerated = true;
    }
    if parse_failed || (!saw_source && !matches!(contract.props, DeclaredSurface::None)) {
        facts.enumerated = false;
    }
    if facts.destructure_disabled {
        facts.reactive_defaults.clear();
    }

    let mut caller_optional = with_default_keys.clone();
    let mut setup_defined = with_default_keys;
    for key in &facts.reactive_defaults {
        push_unique(&mut caller_optional, key.clone());
        push_unique(&mut setup_defined, key.clone());
    }
    // A required runtime prop with a `default` stays required for the
    // caller (`validateProp` still warns) and is defined in setup.
    for key in &facts.runtime_defaults {
        push_unique(&mut setup_defined, key.clone());
        if !facts.required.iter().any(|required| required == key) {
            push_unique(&mut caller_optional, key.clone());
        }
    }
    let caller_required: Vec<String> = facts
        .required
        .iter()
        .filter(|key| !caller_optional.iter().any(|optional| optional == *key))
        .cloned()
        .collect();
    // Non-required Boolean props resolve absence to `false`, so callers
    // may omit them and setup reads a boolean. A required Boolean stays
    // required; the cast does not supply a caller default.
    for key in &facts.boolean_cast {
        if caller_required.iter().any(|required| required == key) {
            continue;
        }
        push_unique(&mut caller_optional, key.clone());
        push_unique(&mut setup_defined, key.clone());
    }

    CallerAndSetupPropsContract {
        caller_optional_keys: caller_optional,
        setup_defined_keys: setup_defined,
        defaults_are_static: contract
            .props_defaults
            .as_ref()
            .is_none_or(|defaults| defaults.keys.is_some()),
        reactive_default_keys: facts.reactive_defaults,
        boolean_cast_keys: facts.boolean_cast,
        boolean_empty_string_keys: facts.boolean_empty_string,
        validator_keys: facts.validators,
        caller_required_keys: caller_required,
        required_keys_are_static: facts.enumerated,
    }
}

fn key_union(keys: &[String]) -> String {
    if keys.is_empty() {
        "never".to_string()
    } else {
        keys.iter()
            .map(|key| quote_ts(key))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

fn quote_ts(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn push_unique(keys: &mut Vec<String>, key: String) {
    if !keys.iter().any(|existing| existing == &key) {
        keys.push(key);
    }
}

#[derive(Debug, Default)]
struct ScriptPropFacts {
    reactive_defaults: Vec<String>,
    runtime_defaults: Vec<String>,
    boolean_cast: Vec<String>,
    boolean_empty_string: Vec<String>,
    validators: Vec<String>,
    required: Vec<String>,
    /// `withDefaults(...)` disables reactive destructure defaults.
    destructure_disabled: bool,
    enumerated: bool,
    saw_props: bool,
}

impl ScriptPropFacts {
    /// Merge one script. `false` when the script does not parse.
    fn absorb(&mut self, source: &str) -> bool {
        if source.trim().is_empty() {
            return true;
        }
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts().with_module(true)).parse();
        if parsed.panicked || !parsed.errors.is_empty() {
            return false;
        }
        self.absorb_program(&parsed.program);
        true
    }

    fn absorb_program(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            if self.saw_props {
                break;
            }
            match statement {
                Statement::VariableDeclaration(declaration) => {
                    for declarator in &declaration.declarations {
                        let Some(init) = &declarator.init else {
                            continue;
                        };
                        self.note_value(program, Some(&declarator.id), init);
                    }
                }
                Statement::ExpressionStatement(statement) => {
                    self.note_value(program, None, &statement.expression);
                }
                _ => {}
            }
        }
    }

    fn note_value(
        &mut self,
        program: &Program<'_>,
        pattern: Option<&BindingPattern<'_>>,
        expression: &Expression<'_>,
    ) {
        if self.saw_props {
            return;
        }
        let Expression::CallExpression(call) = unwrap_expr(expression) else {
            return;
        };
        match call_name(call) {
            Some("defineProps") => {
                self.saw_props = true;
                if let Some(pattern) = pattern {
                    self.reactive_defaults = reactive_defaults(pattern);
                }
                self.read_define_props(program, call);
            }
            Some("withDefaults") => {
                let Some(inner) = call_arg(call, 0).and_then(as_call) else {
                    return;
                };
                if call_name(inner) != Some("defineProps") {
                    return;
                }
                self.saw_props = true;
                self.destructure_disabled = true;
                self.read_define_props(program, inner);
            }
            _ => {}
        }
    }

    fn read_define_props(&mut self, program: &Program<'_>, call: &CallExpression<'_>) {
        if let Some(argument) = call
            .type_arguments
            .as_ref()
            .and_then(|arguments| arguments.params.first())
        {
            match members_of_type_in(argument, Some(program), 0, &mut Vec::new()) {
                Some(members) => self.take_members(&members),
                None => self.enumerated = false,
            }
            return;
        }
        let Some(argument) = call_arg(call, 0) else {
            self.enumerated = true;
            return;
        };
        match unwrap_expr(argument) {
            Expression::ArrayExpression(array) => self.take_name_array(array),
            Expression::ObjectExpression(object) => self.take_runtime_object(object),
            _ => self.enumerated = false,
        }
    }

    fn take_members(&mut self, members: &[PropMember]) {
        self.enumerated = true;
        for member in members {
            if !member.optional {
                push_unique(&mut self.required, member.name.clone());
            }
            if member.boolean {
                push_unique(&mut self.boolean_cast, member.name.clone());
                if member.empty_string {
                    push_unique(&mut self.boolean_empty_string, member.name.clone());
                }
            }
        }
    }

    fn take_name_array(&mut self, array: &ArrayExpression<'_>) {
        self.enumerated = array.elements.iter().all(|element| {
            element.as_expression().is_some_and(|expression| {
                matches!(unwrap_expr(expression), Expression::StringLiteral(_))
            })
        });
    }

    fn take_runtime_object(&mut self, object: &ObjectExpression<'_>) {
        self.enumerated = true;
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                self.enumerated = false;
                continue;
            };
            if property.computed {
                self.enumerated = false;
                continue;
            }
            let Some(name) = static_key(&property.key) else {
                self.enumerated = false;
                continue;
            };
            self.take_runtime_prop(&name, unwrap_expr(&property.value));
        }
    }

    fn take_runtime_prop(&mut self, name: &str, value: &Expression<'_>) {
        match value {
            Expression::Identifier(identifier) if identifier.name == "Boolean" => {
                push_unique(&mut self.boolean_cast, name.to_string());
            }
            Expression::ArrayExpression(array) => {
                self.note_constructors(name, constructor_order(array));
            }
            Expression::NullLiteral(_) => {}
            Expression::ObjectExpression(object) => self.take_prop_options(name, object),
            _ => self.enumerated = false,
        }
    }

    fn take_prop_options(&mut self, name: &str, object: &ObjectExpression<'_>) {
        let mut required_literal = Some(false);
        let mut has_default = false;
        let mut has_validator = false;
        let mut constructors = None;
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                self.enumerated = false;
                required_literal = None;
                continue;
            };
            let Some(key) = (!property.computed)
                .then(|| static_key(&property.key))
                .flatten()
            else {
                self.enumerated = false;
                required_literal = None;
                continue;
            };
            match key.as_str() {
                "type" => constructors = Some(constructor_order_of(unwrap_expr(&property.value))),
                "required" => required_literal = literal_bool(unwrap_expr(&property.value)),
                "default" => has_default = true,
                "validator" => has_validator = true,
                _ => {}
            }
        }
        if let Some(order) = constructors {
            self.note_constructors(name, order);
        }
        match required_literal {
            Some(true) => push_unique(&mut self.required, name.to_string()),
            Some(false) => {}
            None => self.enumerated = false,
        }
        if has_default {
            push_unique(&mut self.runtime_defaults, name.to_string());
        }
        if has_validator {
            push_unique(&mut self.validators, name.to_string());
        }
    }

    fn note_constructors(&mut self, name: &str, order: Vec<Ctor>) {
        let mut saw_string = false;
        let mut casts = false;
        let mut empty_string = false;
        for ctor in order {
            match ctor {
                Ctor::String => {
                    if !casts {
                        saw_string = true;
                    }
                }
                Ctor::Boolean => {
                    casts = true;
                    empty_string = saw_string;
                    break;
                }
                Ctor::Other => {}
                Ctor::Unknown => {
                    self.enumerated = false;
                    return;
                }
            }
        }
        if casts {
            push_unique(&mut self.boolean_cast, name.to_string());
            if empty_string {
                push_unique(&mut self.boolean_empty_string, name.to_string());
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Ctor {
    Boolean,
    String,
    Other,
    Unknown,
}

fn constructor_order(array: &ArrayExpression<'_>) -> Vec<Ctor> {
    array
        .elements
        .iter()
        .map(|element| {
            element
                .as_expression()
                .map(|expression| ctor_of(unwrap_expr(expression)))
                .unwrap_or(Ctor::Unknown)
        })
        .collect()
}

fn constructor_order_of(expression: &Expression<'_>) -> Vec<Ctor> {
    match expression {
        Expression::ArrayExpression(array) => constructor_order(array),
        other => vec![ctor_of(other)],
    }
}

fn ctor_of(expression: &Expression<'_>) -> Ctor {
    match unwrap_expr(expression) {
        Expression::Identifier(identifier) if identifier.name == "Boolean" => Ctor::Boolean,
        Expression::Identifier(identifier) if identifier.name == "String" => Ctor::String,
        Expression::Identifier(_) => Ctor::Other,
        _ => Ctor::Unknown,
    }
}

fn literal_bool(expression: &Expression<'_>) -> Option<bool> {
    match unwrap_expr(expression) {
        Expression::BooleanLiteral(literal) => Some(literal.value),
        _ => None,
    }
}

fn reactive_defaults(pattern: &BindingPattern<'_>) -> Vec<String> {
    let BindingPattern::ObjectPattern(object) = pattern else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    for property in &object.properties {
        if property.computed {
            continue;
        }
        let Some(key) = static_key(&property.key) else {
            continue;
        };
        if matches!(&property.value, BindingPattern::AssignmentPattern(_)) {
            push_unique(&mut keys, key);
        }
    }
    keys
}

fn call_name<'a>(call: &'a CallExpression<'a>) -> Option<&'a str> {
    let Expression::Identifier(identifier) = &call.callee else {
        return None;
    };
    Some(identifier.name.as_str())
}

fn call_arg<'a>(call: &'a CallExpression<'a>, index: usize) -> Option<&'a Expression<'a>> {
    call.arguments.get(index)?.as_expression()
}

fn as_call<'a>(expression: &'a Expression<'a>) -> Option<&'a CallExpression<'a>> {
    match unwrap_expr(expression) {
        Expression::CallExpression(call) => Some(call),
        _ => None,
    }
}

fn unwrap_expr<'a>(expression: &'a Expression<'a>) -> &'a Expression<'a> {
    match expression {
        Expression::ParenthesizedExpression(inner) => unwrap_expr(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => unwrap_expr(&inner.expression),
        Expression::TSNonNullExpression(inner) => unwrap_expr(&inner.expression),
        Expression::TSAsExpression(inner) if inner.type_annotation.is_const_type_reference() => {
            unwrap_expr(&inner.expression)
        }
        Expression::TSTypeAssertion(inner) if inner.type_annotation.is_const_type_reference() => {
            unwrap_expr(&inner.expression)
        }
        other => other,
    }
}

fn static_key(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.to_string()),
        PropertyKey::StringLiteral(literal) => Some(literal.value.to_string()),
        _ => None,
    }
}

struct PropMember {
    name: String,
    optional: bool,
    boolean: bool,
    empty_string: bool,
}

fn members_of_type_in<'a>(
    ty: &'a TSType<'a>,
    program: Option<&'a Program<'a>>,
    depth: u32,
    seen: &mut Vec<String>,
) -> Option<Vec<PropMember>> {
    if depth > 8 {
        return None;
    }
    match ty {
        TSType::TSTypeLiteral(literal) => Some(members_of_signatures(&literal.members)),
        TSType::TSParenthesizedType(inner) => {
            members_of_type_in(&inner.type_annotation, program, depth + 1, seen)
        }
        TSType::TSUnionType(union) => merge_types(&union.types, program, depth, seen),
        TSType::TSIntersectionType(intersection) => {
            merge_types(&intersection.types, program, depth, seen)
        }
        TSType::TSTypeReference(reference) => match &reference.type_name {
            TSTypeName::IdentifierReference(identifier) => {
                resolve_name(program?, identifier.name.as_str(), depth, seen)
            }
            _ => None,
        },
        _ => None,
    }
}

fn merge_types<'a>(
    types: &'a [TSType<'a>],
    program: Option<&'a Program<'a>>,
    depth: u32,
    seen: &mut Vec<String>,
) -> Option<Vec<PropMember>> {
    let mut merged = Vec::new();
    for ty in types {
        let members = members_of_type_in(ty, program, depth + 1, seen)?;
        for member in members {
            merge_member(&mut merged, member);
        }
    }
    Some(merged)
}

fn merge_member(members: &mut Vec<PropMember>, member: PropMember) {
    if let Some(existing) = members.iter_mut().find(|item| item.name == member.name) {
        existing.optional |= member.optional;
        existing.boolean |= member.boolean;
        existing.empty_string |= member.empty_string;
    } else {
        members.push(member);
    }
}

fn members_of_signatures(signatures: &[TSSignature<'_>]) -> Vec<PropMember> {
    let mut members = Vec::new();
    for signature in signatures {
        let (key, optional, ty) = match signature {
            TSSignature::TSPropertySignature(signature) => (
                static_key(&signature.key),
                signature.optional,
                signature
                    .type_annotation
                    .as_ref()
                    .map(|annotation| &annotation.type_annotation),
            ),
            TSSignature::TSMethodSignature(signature) => {
                (static_key(&signature.key), signature.optional, None)
            }
            _ => continue,
        };
        let Some(name) = key else { continue };
        let cast = ty.map(boolean_cast_of).unwrap_or(BooleanCast::none());
        merge_member(
            &mut members,
            PropMember {
                name,
                optional,
                boolean: cast.casts,
                empty_string: cast.empty_string,
            },
        );
    }
    members
}

struct BooleanCast {
    casts: bool,
    empty_string: bool,
}

impl BooleanCast {
    fn none() -> Self {
        Self {
            casts: false,
            empty_string: false,
        }
    }
}

fn boolean_cast_of(ty: &TSType<'_>) -> BooleanCast {
    boolean_cast_at(ty, 0)
}

fn boolean_cast_at(ty: &TSType<'_>, depth: u32) -> BooleanCast {
    if depth > 8 {
        return BooleanCast::none();
    }
    match ty {
        TSType::TSBooleanKeyword(_) => BooleanCast {
            casts: true,
            empty_string: false,
        },
        TSType::TSLiteralType(literal) => match &literal.literal {
            TSLiteral::BooleanLiteral(_) => BooleanCast {
                casts: true,
                empty_string: false,
            },
            _ => BooleanCast::none(),
        },
        TSType::TSParenthesizedType(inner) => boolean_cast_at(&inner.type_annotation, depth + 1),
        TSType::TSTypeReference(reference) => match &reference.type_name {
            TSTypeName::IdentifierReference(identifier) if identifier.name == "Boolean" => {
                BooleanCast {
                    casts: true,
                    empty_string: false,
                }
            }
            _ => BooleanCast::none(),
        },
        TSType::TSUnionType(union) => {
            let mut saw_string = false;
            let mut casts = false;
            let mut empty_string = false;
            for member in &union.types {
                if is_string_type(member) && !casts {
                    saw_string = true;
                }
                let part = boolean_cast_at(member, depth + 1);
                if part.casts {
                    casts = true;
                    empty_string = saw_string || part.empty_string;
                    break;
                }
            }
            BooleanCast {
                casts,
                empty_string,
            }
        }
        _ => BooleanCast::none(),
    }
}

fn is_string_type(ty: &TSType<'_>) -> bool {
    match ty {
        TSType::TSStringKeyword(_) => true,
        TSType::TSLiteralType(literal) => matches!(&literal.literal, TSLiteral::StringLiteral(_)),
        TSType::TSTypeReference(reference) => matches!(
            &reference.type_name,
            TSTypeName::IdentifierReference(identifier) if identifier.name == "String"
        ),
        TSType::TSParenthesizedType(inner) => is_string_type(&inner.type_annotation),
        _ => false,
    }
}

fn resolve_name<'a>(
    program: &'a Program<'a>,
    name: &str,
    depth: u32,
    seen: &mut Vec<String>,
) -> Option<Vec<PropMember>> {
    if seen.iter().any(|seen| seen == name) {
        return None;
    }
    seen.push(name.to_string());
    let mut members = Vec::new();
    let mut found = false;
    for statement in &program.body {
        let Some(declaration) = (match statement {
            Statement::ExportNamedDeclaration(export) => export.declaration.as_ref(),
            other => other.as_declaration(),
        }) else {
            continue;
        };
        match declaration {
            Declaration::TSInterfaceDeclaration(interface) if interface.id.name == name => {
                if !interface.extends.is_empty() {
                    return None;
                }
                found = true;
                for member in members_of_signatures(&interface.body.body) {
                    merge_member(&mut members, member);
                }
            }
            Declaration::TSTypeAliasDeclaration(alias) if alias.id.name == name => {
                found = true;
                let resolved =
                    members_of_type_in(&alias.type_annotation, Some(program), depth + 1, seen)?;
                for member in resolved {
                    merge_member(&mut members, member);
                }
            }
            _ => {}
        }
    }
    found.then_some(members)
}
