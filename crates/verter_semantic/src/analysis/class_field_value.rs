//! The one classification of a class field's initializer: whether the
//! field's type is read through a synthetic value declaration at its
//! initializer, and through which source.
//!
//! A field typed by syntactic inference is typed where the class lowers.
//! Inference has no call authority and knows no `this`, so a field whose
//! initializer's type derives from a call, or which reads `this`, is read
//! through a synthetic value declaration instead (`C:field:x`,
//! `C:static:x`). The shallow header walk registers that declaration, the
//! class lowering refers to it, and the function-program discovery serves
//! its value. All three read the answer [`ClassFieldValues`] records: the
//! header walk classifies each field of each class it indexes (a declared
//! class, or a class expression a variable holds), once per content
//! generation, and publishes the table on the header index. A class the
//! header walk does not index (one inside a function body) is classified
//! where it is met.
//!
//! The classification is one walk of the initializer that stops at every
//! function boundary. A nested arrow function is a callback whose body may
//! read the field's `this` (an arrow's `this` is its position's); the walk
//! does not enter it and serves such a field as a position of its own,
//! which reads the receiver whatever the callback's body does. A nested
//! `function` or class has a `this` of its own.
use verter_session_query::declarations::class_fields::{ClassFieldValueSource, ClassFieldValues};

use oxc_ast::ast::{Class, ClassElement, Expression, PropertyKey, TSTypeAnnotation};
use oxc_ast_visit::Visit;
use oxc_span::GetSpan;

/// A class field: a property definition, or an `accessor` field, which to
/// every reader is one mutable property typed as a property definition is.
#[derive(Clone, Copy)]
pub struct ClassField<'s, 'a> {
    pub key: &'s PropertyKey<'a>,
    pub value: Option<&'s Expression<'a>>,
    pub type_annotation: Option<&'s TSTypeAnnotation<'a>>,
    pub r#static: bool,
    pub readonly: bool,
}

impl<'s, 'a> ClassField<'s, 'a> {
    /// The field `element` declares, if it is one.
    #[must_use]
    pub fn of(element: &'s ClassElement<'a>) -> Option<Self> {
        match element {
            ClassElement::PropertyDefinition(prop) => Some(Self {
                key: &prop.key,
                value: prop.value.as_ref(),
                type_annotation: prop.type_annotation.as_deref(),
                r#static: prop.r#static,
                readonly: prop.readonly,
            }),
            ClassElement::AccessorProperty(property) => Some(Self {
                key: &property.key,
                value: property.value.as_ref(),
                type_annotation: property.type_annotation.as_deref(),
                r#static: property.r#static,
                readonly: false,
            }),
            _ => None,
        }
    }
}

/// Classify `field`, a field of `class`, and record the answer in `values`.
pub fn classify_class_field(
    values: &mut ClassFieldValues,
    class: &Class<'_>,
    field: ClassField<'_, '_>,
    source: &str,
) -> Option<ClassFieldValueSource> {
    values.record_class(class.span.start);
    let classified = classify_field(field, source);
    if let (Some(kind), Some(value)) = (classified, field.value) {
        values.record_field(value.span().start, kind);
    }
    classified
}

/// Record every field of `class` ([`classify_class_field`]).
pub fn classify_class_fields(values: &mut ClassFieldValues, class: &Class<'_>, source: &str) {
    values.record_class(class.span.start);
    for element in &class.body.body {
        if let Some(field) = ClassField::of(element) {
            classify_class_field(values, class, field, source);
        }
    }
}

/// The classification of `field`, a field of `class`: the recorded one
/// when `values` classified the class, else classified here (a class the
/// header walk does not index).
#[must_use]
pub fn class_field_value(
    values: &ClassFieldValues,
    class: &Class<'_>,
    field: ClassField<'_, '_>,
    source: &str,
) -> Option<ClassFieldValueSource> {
    if values.classified_class(class.span.start) {
        return field
            .value
            .and_then(|value| values.recorded_field(value.span().start));
    }
    classify_field(field, source)
}

/// Classify one field's initializer (see the module documentation). `None`
/// for a field syntactic inference types: an annotated or `#private` field,
/// one with no initializer or a computed key, a direct function value (a
/// method-like served position of its own), an authoritative assertion, and
/// an initializer that neither derives from a call nor reads `this`.
fn classify_field(field: ClassField<'_, '_>, source: &str) -> Option<ClassFieldValueSource> {
    if field.type_annotation.is_some() || matches!(field.key, PropertyKey::PrivateIdentifier(_)) {
        return None;
    }
    let value = field.value?;
    if matches!(
        value,
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
    ) || crate::analysis::type_eval_build::has_authoritative_value_assertion(value)
    {
        return None;
    }
    crate::analysis::function_program::static_property_key_name(field.key)?;
    let mut probe = FieldValueProbe {
        call_relevant: true,
        ..FieldValueProbe::default()
    };
    verter_parser::oxc_parse::with_span_stack(source, value.span(), || {
        probe.visit_expression(value);
    });
    #[cfg(any(test, feature = "test-support"))]
    observe::classified(source, value.span().start, probe.visited);
    if probe.reads_this || probe.holds_callback {
        Some(ClassFieldValueSource::Initializer)
    } else if probe.derives_from_call {
        Some(ClassFieldValueSource::Call)
    } else {
        None
    }
}

/// The one walk of an initializer. `call_relevant` is off beneath a form
/// whose type a call inside cannot change — a template literal (`string`
/// whatever its interpolations are) and a conditional's test (it picks a
/// branch, never what either is) — where `this` is still read.
#[derive(Default)]
struct FieldValueProbe {
    call_relevant: bool,
    derives_from_call: bool,
    reads_this: bool,
    holds_callback: bool,
    #[cfg(any(test, feature = "test-support"))]
    visited: u64,
}

impl FieldValueProbe {
    fn call(&mut self) {
        if self.call_relevant {
            self.derives_from_call = true;
        }
    }

    fn call_irrelevant(&mut self, walk: impl FnOnce(&mut Self)) {
        let outer = std::mem::replace(&mut self.call_relevant, false);
        walk(self);
        self.call_relevant = outer;
    }
}

impl<'a> Visit<'a> for FieldValueProbe {
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.visited += 1;
        }
        oxc_ast_visit::walk::walk_expression(self, expression);
    }

    fn visit_this_expression(&mut self, _this: &oxc_ast::ast::ThisExpression) {
        self.reads_this = true;
    }

    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        self.call();
        oxc_ast_visit::walk::walk_call_expression(self, call);
    }

    fn visit_new_expression(&mut self, new: &oxc_ast::ast::NewExpression<'a>) {
        self.call();
        oxc_ast_visit::walk::walk_new_expression(self, new);
    }

    // A tagged template CALLS its tag: its type is the tag's return.
    fn visit_tagged_template_expression(
        &mut self,
        tagged: &oxc_ast::ast::TaggedTemplateExpression<'a>,
    ) {
        self.call();
        oxc_ast_visit::walk::walk_tagged_template_expression(self, tagged);
    }

    fn visit_template_literal(&mut self, template: &oxc_ast::ast::TemplateLiteral<'a>) {
        self.call_irrelevant(|probe| {
            oxc_ast_visit::walk::walk_template_literal(probe, template);
        });
    }

    fn visit_conditional_expression(
        &mut self,
        conditional: &oxc_ast::ast::ConditionalExpression<'a>,
    ) {
        self.call_irrelevant(|probe| probe.visit_expression(&conditional.test));
        self.visit_expression(&conditional.consequent);
        self.visit_expression(&conditional.alternate);
    }

    // A callback's body is not walked: it may read the field's `this`.
    fn visit_arrow_function_expression(
        &mut self,
        _arrow: &oxc_ast::ast::ArrowFunctionExpression<'a>,
    ) {
        self.holds_callback = true;
    }

    // A function's `this`, and a class's, is its own.
    fn visit_function(
        &mut self,
        _function: &oxc_ast::ast::Function<'a>,
        _flags: oxc_syntax::scope::ScopeFlags,
    ) {
    }

    fn visit_class(&mut self, _class: &Class<'a>) {}
}

/// What the classification did, for the single-walk guards: how many times
/// each initializer of a watched source was classified and how many
/// expressions those walks visited. A source is watched by a marker it
/// contains (tests run in parallel, and a header walk or a lowering may run
/// on a worker thread, so a thread-local count would miss it).
#[cfg(any(test, feature = "test-support"))]
mod observe {
    use std::sync::Mutex;

    /// One initializer's `(offset, classifications, expressions visited)`.
    type Record = (u32, u32, u64);

    /// The markers watched, each with its records.
    static WATCHED: Mutex<Vec<(String, Vec<Record>)>> = Mutex::new(Vec::new());

    pub(super) fn classified(source: &str, offset: u32, visited: u64) {
        let mut watched = WATCHED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (marker, records) in watched.iter_mut() {
            if !source.contains(marker.as_str()) {
                continue;
            }
            match records.iter_mut().find(|record| record.0 == offset) {
                Some(record) => {
                    record.1 += 1;
                    record.2 += visited;
                }
                None => records.push((offset, 1, visited)),
            }
        }
    }

    pub(super) fn watch(marker: &str) {
        WATCHED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((marker.to_string(), Vec::new()));
    }

    pub(super) fn take(marker: &str) -> Vec<(u32, u32, u64)> {
        let mut watched = WATCHED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(position) = watched.iter().position(|(watched, _)| watched == marker) else {
            return Vec::new();
        };
        let mut records = watched.swap_remove(position).1;
        records.sort_unstable();
        records
    }
}

/// Record the classifications of every source containing `marker` from now
/// on ([`take_class_field_classifications_for_tests`]).
#[cfg(any(test, feature = "test-support"))]
pub fn watch_class_field_classifications_for_tests(marker: &str) {
    observe::watch(marker);
}

/// Stop watching `marker`: per initializer offset of the sources containing
/// it, how many times the classification walked it, and how many
/// expressions those walks visited in all.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn take_class_field_classifications_for_tests(marker: &str) -> Vec<(u32, u32, u64)> {
    observe::take(marker)
}
