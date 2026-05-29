//! In-place declaration-site span transforms over [`TypeExpr`].
//!
//! Spans on a `TypeExpr` are byte offsets into the source buffer the type was
//! lowered from. Two transforms rebase or drop them:
//!
//! - [`TypeExpr::shift_spans`] — rebase every embedded span by a signed byte
//!   delta, used to translate spans produced against one buffer (e.g. a JSDoc
//!   `{Type}` payload wrapped in `type __T = <payload>`) into the real source
//!   file's coordinates.
//! - [`TypeExpr::clear_spans`] — drop every embedded span when the type was
//!   lowered from text that has no single contiguous source region (honest
//!   absence, never a wrong offset).
//!
//! Both walk the full `TypeExpr` tree and every span-bearing sub-struct
//! ([`MemberSpans`], [`IndexSignatureSpans`], [`FunctionSpans`],
//! [`FunctionParam::span`]).

use std::sync::Arc;

use crate::{
    FunctionExpr, FunctionSpans, IndexSignatureSpans, MemberSpans, ObjectExpr, ObjectMember,
    TypeExpr,
};

/// Recursively [`TypeExpr::shift_spans`] every element of a shared
/// `Arc<[TypeExpr]>` in place (cloning on write only if the `Arc` is shared).
fn shift_arc_slice(slice: &mut Arc<[TypeExpr]>, delta: i64) {
    if delta == 0 {
        return;
    }
    let mut items: Vec<TypeExpr> = slice.iter().cloned().collect();
    for item in &mut items {
        item.shift_spans(delta);
    }
    *slice = Arc::from(items);
}

/// Recursively [`TypeExpr::clear_spans`] every element of a shared
/// `Arc<[TypeExpr]>` in place.
fn clear_arc_slice(slice: &mut Arc<[TypeExpr]>) {
    let mut items: Vec<TypeExpr> = slice.iter().cloned().collect();
    for item in &mut items {
        item.clear_spans();
    }
    *slice = Arc::from(items);
}

impl MemberSpans {
    /// Rebase every present span by a signed byte `delta` (see
    /// [`verter_span::Span::shifted`]).
    pub(crate) fn shift(&mut self, delta: i64) {
        self.declaration = self.declaration.map(|span| span.shifted(delta));
        self.name = self.name.map(|span| span.shifted(delta));
        self.type_annotation = self.type_annotation.map(|span| span.shifted(delta));
    }

    /// Drop every span (honest absence — no single source site).
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

impl FunctionSpans {
    /// Rebase every present span by a signed byte `delta`.
    pub(crate) fn shift(&mut self, delta: i64) {
        self.signature = self.signature.map(|span| span.shifted(delta));
        self.return_type = self.return_type.map(|span| span.shifted(delta));
    }

    /// Drop every span (honest absence — no single source site).
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

impl IndexSignatureSpans {
    /// Rebase every present span by a signed byte `delta`.
    pub(crate) fn shift(&mut self, delta: i64) {
        self.declaration = self.declaration.map(|span| span.shifted(delta));
        self.key = self.key.map(|span| span.shifted(delta));
        self.value = self.value.map(|span| span.shifted(delta));
    }

    /// Drop every span (honest absence — no single source site).
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

impl ObjectExpr {
    /// Recursively rebase every member's spans and nested-type spans by a signed
    /// byte `delta`.
    pub(crate) fn shift_spans(&mut self, delta: i64) {
        for member in &mut self.properties {
            match member {
                ObjectMember::Property(prop) => {
                    prop.spans.shift(delta);
                    prop.ty.shift_spans(delta);
                }
                ObjectMember::IndexSignature(idx) => {
                    idx.spans.shift(delta);
                    idx.key_type.shift_spans(delta);
                    idx.value_type.shift_spans(delta);
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    func.shift_spans(delta);
                }
                ObjectMember::Method(method) => {
                    method.spans.shift(delta);
                    method.function.shift_spans(delta);
                }
            }
        }
    }

    /// Recursively drop every member span and nested-type span.
    pub(crate) fn clear_spans(&mut self) {
        for member in &mut self.properties {
            match member {
                ObjectMember::Property(prop) => {
                    prop.spans.clear();
                    prop.ty.clear_spans();
                }
                ObjectMember::IndexSignature(idx) => {
                    idx.spans.clear();
                    idx.key_type.clear_spans();
                    idx.value_type.clear_spans();
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    func.clear_spans();
                }
                ObjectMember::Method(method) => {
                    method.spans.clear();
                    method.function.clear_spans();
                }
            }
        }
    }
}

impl FunctionExpr {
    /// Recursively rebase the signature/return spans, each parameter's span and
    /// nested type, the return type, and every type-parameter constraint /
    /// default by a signed byte `delta`.
    pub(crate) fn shift_spans(&mut self, delta: i64) {
        self.spans.shift(delta);
        for param in &mut self.parameters {
            param.span = param.span.map(|span| span.shifted(delta));
            param.ty.shift_spans(delta);
        }
        if let Some(return_type) = self.return_type.as_mut() {
            Arc::make_mut(return_type).shift_spans(delta);
        }
        for type_param in &mut self.type_parameters {
            if let Some(constraint) = type_param.constraint.as_mut() {
                Arc::make_mut(constraint).shift_spans(delta);
            }
            if let Some(default) = type_param.default.as_mut() {
                Arc::make_mut(default).shift_spans(delta);
            }
        }
    }

    /// Recursively drop the signature/return spans, each parameter's span and
    /// nested type, the return type, and every type-parameter constraint /
    /// default.
    pub(crate) fn clear_spans(&mut self) {
        self.spans.clear();
        for param in &mut self.parameters {
            param.span = None;
            param.ty.clear_spans();
        }
        if let Some(return_type) = self.return_type.as_mut() {
            Arc::make_mut(return_type).clear_spans();
        }
        for type_param in &mut self.type_parameters {
            if let Some(constraint) = type_param.constraint.as_mut() {
                Arc::make_mut(constraint).clear_spans();
            }
            if let Some(default) = type_param.default.as_mut() {
                Arc::make_mut(default).clear_spans();
            }
        }
    }
}

impl TypeExpr {
    /// Recursively shift every embedded declaration-site span by a signed byte
    /// `delta`, in place.
    ///
    /// When a type is lowered from a *synthetic* buffer (e.g. a JSDoc `{Type}`
    /// payload wrapped in `type __T = <payload>` before the OXC parse), its
    /// spans are in that buffer's coordinates. Shifting by
    /// `file_offset_of_payload - synthetic_prefix_len` rebases every span into
    /// the real source file's coordinates so a consumer can slice the file with
    /// them — matching the spans a directly-lowered TS annotation already
    /// carries.
    ///
    /// `delta` is signed: the synthetic prefix may sit at a higher offset than
    /// the payload's file position. Each shifted endpoint saturates at `0`
    /// (a span never wraps below the start of the file). Variants that carry no
    /// span (primitives, literals, `Ref`, `TypeOf`, …) recurse into their
    /// children but shift nothing themselves.
    pub fn shift_spans(&mut self, delta: i64) {
        match self {
            // -- Terminals with no spans and no children --
            Self::Primitive(_)
            | Self::Literal(_)
            | Self::TypeOf(_)
            | Self::Infer { .. }
            | Self::SyntheticSlotBinding(_)
            | Self::Unknown { .. } => {}

            // -- Compound: recurse into children --
            Self::Union(members) | Self::Intersection(members) => {
                shift_arc_slice(members, delta);
            }
            Self::Array { element, .. } => {
                Arc::make_mut(element).shift_spans(delta);
            }
            Self::Tuple { elements, .. } => {
                for element in Arc::make_mut(elements).iter_mut() {
                    element.ty.shift_spans(delta);
                }
            }
            Self::Object(object) => {
                Arc::make_mut(object).shift_spans(delta);
            }
            Self::Function(function) => {
                Arc::make_mut(function).shift_spans(delta);
            }
            Self::Ref { type_arguments, .. } | Self::RecursiveRef { type_arguments, .. } => {
                shift_arc_slice(type_arguments, delta);
            }
            Self::TypeParameter(param) => {
                if let Some(constraint) = param.constraint.as_mut() {
                    Arc::make_mut(constraint).shift_spans(delta);
                }
                if let Some(default) = param.default.as_mut() {
                    Arc::make_mut(default).shift_spans(delta);
                }
            }
            Self::KeyOf(inner) | Self::Rest(inner) | Self::Parenthesized(inner) => {
                Arc::make_mut(inner).shift_spans(delta);
            }
            Self::IndexedAccess { object, index } => {
                Arc::make_mut(object).shift_spans(delta);
                Arc::make_mut(index).shift_spans(delta);
            }
            Self::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                Arc::make_mut(check).shift_spans(delta);
                Arc::make_mut(extends).shift_spans(delta);
                Arc::make_mut(true_type).shift_spans(delta);
                Arc::make_mut(false_type).shift_spans(delta);
            }
            Self::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                Arc::make_mut(source).shift_spans(delta);
                Arc::make_mut(value).shift_spans(delta);
                if let Some(name_type) = name_type.as_mut() {
                    Arc::make_mut(name_type).shift_spans(delta);
                }
            }
            Self::TemplateLiteral { expressions, .. } => {
                shift_arc_slice(expressions, delta);
            }
        }
    }

    /// Recursively clear every embedded declaration-site span to `None`, in
    /// place.
    ///
    /// Used when a type was lowered from text that has NO single contiguous
    /// source region (e.g. a multi-line / `*`-decorated JSDoc `{Type}` payload
    /// reconstructed across comment lines): there is no honest file span for its
    /// members, so the structure is preserved while every span is dropped —
    /// honest absence rather than a wrong offset (the same policy a synthesized
    /// multi-origin union member follows).
    pub fn clear_spans(&mut self) {
        match self {
            Self::Primitive(_)
            | Self::Literal(_)
            | Self::TypeOf(_)
            | Self::Infer { .. }
            | Self::SyntheticSlotBinding(_)
            | Self::Unknown { .. } => {}

            Self::Union(members) | Self::Intersection(members) => {
                clear_arc_slice(members);
            }
            Self::Array { element, .. } => {
                Arc::make_mut(element).clear_spans();
            }
            Self::Tuple { elements, .. } => {
                for element in Arc::make_mut(elements).iter_mut() {
                    element.ty.clear_spans();
                }
            }
            Self::Object(object) => {
                Arc::make_mut(object).clear_spans();
            }
            Self::Function(function) => {
                Arc::make_mut(function).clear_spans();
            }
            Self::Ref { type_arguments, .. } | Self::RecursiveRef { type_arguments, .. } => {
                clear_arc_slice(type_arguments);
            }
            Self::TypeParameter(param) => {
                if let Some(constraint) = param.constraint.as_mut() {
                    Arc::make_mut(constraint).clear_spans();
                }
                if let Some(default) = param.default.as_mut() {
                    Arc::make_mut(default).clear_spans();
                }
            }
            Self::KeyOf(inner) | Self::Rest(inner) | Self::Parenthesized(inner) => {
                Arc::make_mut(inner).clear_spans();
            }
            Self::IndexedAccess { object, index } => {
                Arc::make_mut(object).clear_spans();
                Arc::make_mut(index).clear_spans();
            }
            Self::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                Arc::make_mut(check).clear_spans();
                Arc::make_mut(extends).clear_spans();
                Arc::make_mut(true_type).clear_spans();
                Arc::make_mut(false_type).clear_spans();
            }
            Self::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                Arc::make_mut(source).clear_spans();
                Arc::make_mut(value).clear_spans();
                if let Some(name_type) = name_type.as_mut() {
                    Arc::make_mut(name_type).clear_spans();
                }
            }
            Self::TemplateLiteral { expressions, .. } => {
                clear_arc_slice(expressions);
            }
        }
    }
}
