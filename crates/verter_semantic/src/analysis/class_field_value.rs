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

use oxc_ast::ast::{Class, ClassElement, Expression, PropertyDefinition, PropertyKey};
use oxc_ast_visit::Visit;
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};

/// Where a class field's synthetic value is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassFieldValueSource {
    /// The initializer's type derives from a call: the indexed program
    /// expression at the initializer.
    Call,
    /// The initializer reads `this`, or holds a callback that may: a served
    /// position of its own, whose frame reads the receiver.
    Initializer,
}

/// The classification of every field of the classes one header walk
/// indexed, by initializer offset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassFieldValues {
    /// The classes classified here, by span start.
    classes: FxHashSet<u32>,
    /// The fields read through a synthetic value, by initializer offset.
    fields: FxHashMap<u32, ClassFieldValueSource>,
}

impl ClassFieldValues {
    /// Record that the class starting at `class_start` was classified here.
    pub fn record_class(&mut self, class_start: u32) {
        self.classes.insert(class_start);
    }

    /// Record the classification of the field whose initializer starts at
    /// `value_start`.
    pub fn record_field(&mut self, value_start: u32, kind: ClassFieldValueSource) {
        self.fields.insert(value_start, kind);
    }

    /// Whether the class starting at `class_start` was classified here.
    #[must_use]
    pub fn classified_class(&self, class_start: u32) -> bool {
        self.classes.contains(&class_start)
    }

    /// The recorded classification of the field whose initializer starts at
    /// `value_start`.
    #[must_use]
    pub fn recorded_field(&self, value_start: u32) -> Option<ClassFieldValueSource> {
        self.fields.get(&value_start).copied()
    }
}

/// Classify `prop`, a field of `class`, and record the answer in `values`.
pub fn classify_class_field(
    values: &mut ClassFieldValues,
    class: &Class<'_>,
    prop: &PropertyDefinition<'_>,
    source: &str,
) -> Option<ClassFieldValueSource> {
    values.record_class(class.span.start);
    let classified = classify_field(prop, source);
    if let (Some(kind), Some(value)) = (classified, prop.value.as_ref()) {
        values.record_field(value.span().start, kind);
    }
    classified
}

/// Record every field of `class` ([`classify_class_field`]).
pub fn classify_class_fields(values: &mut ClassFieldValues, class: &Class<'_>, source: &str) {
    values.record_class(class.span.start);
    for element in &class.body.body {
        if let ClassElement::PropertyDefinition(prop) = element {
            classify_class_field(values, class, prop, source);
        }
    }
}

/// The classification of `prop`, a field of `class`: the recorded one
/// when `values` classified the class, else classified here (a class the
/// header walk does not index).
#[must_use]
pub fn class_field_value(
    values: &ClassFieldValues,
    class: &Class<'_>,
    prop: &PropertyDefinition<'_>,
    source: &str,
) -> Option<ClassFieldValueSource> {
    if values.classified_class(class.span.start) {
        return prop
            .value
            .as_ref()
            .and_then(|value| values.recorded_field(value.span().start));
    }
    classify_field(prop, source)
}

/// Classify one field's initializer (see the module documentation). `None`
/// for a field syntactic inference types: an annotated or `#private` field,
/// one with no initializer or a computed key, a direct function value (a
/// method-like served position of its own), an authoritative assertion, and
/// an initializer that neither derives from a call nor reads `this`.
fn classify_field(prop: &PropertyDefinition<'_>, source: &str) -> Option<ClassFieldValueSource> {
    if prop.type_annotation.is_some() || matches!(prop.key, PropertyKey::PrivateIdentifier(_)) {
        return None;
    }
    let value = prop.value.as_ref()?;
    if matches!(
        value,
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
    ) || crate::analysis::type_eval_build::has_authoritative_value_assertion(value)
    {
        return None;
    }
    crate::analysis::function_program::static_property_key_name(&prop.key)?;
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
