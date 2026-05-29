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
//! [`FunctionParam::span`]), including the `check` / `extends` snapshots held
//! inside a [`crate::RecursiveRef`]'s `conditional_context` frames.

use std::sync::Arc;

use crate::{
    FunctionExpr, FunctionSpans, IndexSignatureSpans, MemberSpans, ObjectExpr, ObjectMember,
    RecursiveConditionalFrame, TypeExpr,
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

/// Recursively [`TypeExpr::shift_spans`] the `check` and `extends` of every
/// frame in a shared `Arc<[RecursiveConditionalFrame]>` in place.
///
/// A frame's `check` / `extends` are `Arc<TypeExpr>` snapshots of the active
/// conditional branch and may carry an inline object / function / indexed type
/// whose member spans need the same declaration-site rebase as any other
/// embedded type. Mirrors [`shift_arc_slice`]: clone the slice into an owned
/// `Vec`, [`Arc::make_mut`] each frame child to recurse, then rebuild the
/// `Arc<[_]>`.
fn shift_frame_slice(frames: &mut Arc<[RecursiveConditionalFrame]>, delta: i64) {
    if delta == 0 || frames.is_empty() {
        return;
    }
    let mut items: Vec<RecursiveConditionalFrame> = frames.iter().cloned().collect();
    for frame in &mut items {
        Arc::make_mut(&mut frame.check).shift_spans(delta);
        Arc::make_mut(&mut frame.extends).shift_spans(delta);
    }
    *frames = Arc::from(items);
}

/// Recursively [`TypeExpr::clear_spans`] the `check` and `extends` of every
/// frame in a shared `Arc<[RecursiveConditionalFrame]>` in place — the
/// span-dropping sibling of [`shift_frame_slice`].
fn clear_frame_slice(frames: &mut Arc<[RecursiveConditionalFrame]>) {
    if frames.is_empty() {
        return;
    }
    let mut items: Vec<RecursiveConditionalFrame> = frames.iter().cloned().collect();
    for frame in &mut items {
        Arc::make_mut(&mut frame.check).clear_spans();
        Arc::make_mut(&mut frame.extends).clear_spans();
    }
    *frames = Arc::from(items);
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
            Self::Ref { type_arguments, .. } => {
                shift_arc_slice(type_arguments, delta);
            }
            Self::RecursiveRef {
                type_arguments,
                conditional_context,
                ..
            } => {
                shift_arc_slice(type_arguments, delta);
                shift_frame_slice(conditional_context, delta);
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
            Self::Ref { type_arguments, .. } => {
                clear_arc_slice(type_arguments);
            }
            Self::RecursiveRef {
                type_arguments,
                conditional_context,
                ..
            } => {
                clear_arc_slice(type_arguments);
                clear_frame_slice(conditional_context);
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use verter_span::Span;

    use crate::{
        ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, RecursiveConditionalBranch,
        RecursiveConditionalFrame, TypeExpr,
    };

    /// Build an inline object type `{ <name>: number }` whose single property
    /// carries known declaration-site spans, so a shift/clear over the object's
    /// member spans is observable.
    fn object_with_member_spans(
        name: &str,
        decl: Span,
        name_span: Span,
        ty_span: Span,
    ) -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::with_spans(
                name.to_string(),
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
                crate::MemberSpans {
                    declaration: Some(decl),
                    name: Some(name_span),
                    type_annotation: Some(ty_span),
                },
            ))],
        }))
    }

    /// Pull the single property's [`crate::MemberSpans`] out of an object-typed
    /// `TypeExpr`, panicking if the shape is not the one
    /// [`object_with_member_spans`] built.
    fn member_spans(ty: &TypeExpr) -> &crate::MemberSpans {
        let TypeExpr::Object(object) = ty else {
            panic!("expected an object type, got {ty:?}");
        };
        let ObjectMember::Property(prop) = &object.properties[0] else {
            panic!("expected a single property, got {:?}", object.properties[0]);
        };
        &prop.spans
    }

    /// One inline `(check, extends)` pair for a [`RecursiveConditionalFrame`],
    /// each an object type carrying member spans at the supplied base offset.
    struct FrameFixture {
        check: TypeExpr,
        extends: TypeExpr,
    }

    /// Construct a `RecursiveRef` carrying:
    ///
    /// - ONE span-bearing `type_arguments` entry (`type_arg`, an inline object
    ///   type) — guards that the `RecursiveRef` arm still recurses
    ///   `type_arguments` after it was split out of the shared `Ref` arm. An
    ///   empty `type_arguments` slice would let a regressed arm that drops the
    ///   `shift_arc_slice`/`clear_arc_slice` call pass unnoticed.
    /// - TWO `conditional_context` frames whose `check` / `extends` are inline
    ///   object types carrying member spans at distinct offsets — guards the
    ///   `shift_frame_slice` / `clear_frame_slice` loop: an impl that transforms
    ///   only frame `0` leaves frame `1` untouched and must be caught.
    fn recursive_ref_fixture(type_arg: TypeExpr, frames: [FrameFixture; 2]) -> TypeExpr {
        let conditional_context: Vec<RecursiveConditionalFrame> = frames
            .into_iter()
            .map(|frame| RecursiveConditionalFrame {
                branch: RecursiveConditionalBranch::True,
                decided: false,
                check: Arc::new(frame.check),
                extends: Arc::new(frame.extends),
            })
            .collect();
        TypeExpr::RecursiveRef {
            name: Arc::from("Self"),
            type_arguments: Arc::from(vec![type_arg]),
            conditional_context: Arc::from(conditional_context),
        }
    }

    /// Extract the (check, extends) sub-types of a `RecursiveRef`'s frame at
    /// `index`, panicking if `index` is out of range.
    fn frame_types(ty: &TypeExpr, index: usize) -> (&TypeExpr, &TypeExpr) {
        let TypeExpr::RecursiveRef {
            conditional_context,
            ..
        } = ty
        else {
            panic!("expected a RecursiveRef, got {ty:?}");
        };
        let frame = conditional_context
            .get(index)
            .unwrap_or_else(|| panic!("expected frame {index}, got {conditional_context:?}"));
        (&frame.check, &frame.extends)
    }

    /// Extract the single `type_arguments` entry of a `RecursiveRef`, panicking
    /// if the shape is not the one [`recursive_ref_fixture`] built.
    fn sole_type_argument(ty: &TypeExpr) -> &TypeExpr {
        let TypeExpr::RecursiveRef { type_arguments, .. } = ty else {
            panic!("expected a RecursiveRef, got {ty:?}");
        };
        assert_eq!(
            type_arguments.len(),
            1,
            "fixture must carry exactly one type argument, got {type_arguments:?}",
        );
        &type_arguments[0]
    }

    // Fix 1 (BINDING P1) — `shift_spans` must rebase the member spans held INSIDE
    // a `RecursiveRef`'s `conditional_context` frames (`check` / `extends`), not
    // only the `type_arguments`. Pre-fix the `RecursiveRef` arm shared the `Ref`
    // arm and shifted `type_arguments` only, so a frame holding an object type
    // kept its stale wrapper-local member spans. This test also pins the two
    // promises the dedicated arm now keeps: it STILL recurses `type_arguments`
    // (a span-bearing arg shifts), and `shift_frame_slice` walks EVERY frame
    // (both frame 0 AND frame 1 shift). It FAILS against an arm that skips
    // `conditional_context`, drops the `type_arguments` recursion, or only
    // transforms the first frame; it PASSES once all three are handled.
    #[test]
    fn shift_spans_rebases_recursive_ref_conditional_frame_member_spans() {
        let delta: i64 = 100;
        // The type argument and every frame `check` / `extends` start at DISTINCT
        // offsets so a single accidental shift of one cannot masquerade as all of
        // them being correct.
        let type_arg = object_with_member_spans(
            "arg",
            Span::new(70, 79),
            Span::new(70, 73),
            Span::new(75, 79),
        );
        let frames = [
            FrameFixture {
                check: object_with_member_spans(
                    "a",
                    Span::new(10, 19),
                    Span::new(10, 11),
                    Span::new(13, 19),
                ),
                extends: object_with_member_spans(
                    "b",
                    Span::new(40, 49),
                    Span::new(40, 41),
                    Span::new(43, 49),
                ),
            },
            // Second frame: distinct offsets so a "frame 0 only" loop is caught.
            FrameFixture {
                check: object_with_member_spans(
                    "c",
                    Span::new(200, 209),
                    Span::new(200, 201),
                    Span::new(203, 209),
                ),
                extends: object_with_member_spans(
                    "d",
                    Span::new(230, 239),
                    Span::new(230, 231),
                    Span::new(233, 239),
                ),
            },
        ];
        let mut ty = recursive_ref_fixture(type_arg, frames);

        ty.shift_spans(delta);

        // The span-bearing `type_arguments` entry must still shift: guards that
        // the dedicated `RecursiveRef` arm did not drop `shift_arc_slice`.
        let type_arg_spans = member_spans(sole_type_argument(&ty));
        assert_eq!(
            type_arg_spans.name,
            Some(Span::new(170, 173)),
            "the `type_arguments` member NAME span must shift by exactly {delta}; an unchanged \
             70..73 proves the `RecursiveRef` arm stopped recursing `type_arguments`",
        );
        assert_eq!(
            type_arg_spans.type_annotation,
            Some(Span::new(175, 179)),
            "the `type_arguments` member TYPE span must shift by exactly {delta}",
        );
        assert_eq!(
            type_arg_spans.declaration,
            Some(Span::new(170, 179)),
            "the `type_arguments` member DECLARATION span must shift by exactly {delta}",
        );

        // Frame 0.
        let (check_0, extends_0) = frame_types(&ty, 0);
        let check_0_spans = member_spans(check_0);
        assert_eq!(
            check_0_spans.name,
            Some(Span::new(110, 111)),
            "the frame-0 `check` member NAME span must shift by exactly {delta}; an unchanged \
             10..11 proves `shift_spans` skipped `conditional_context`",
        );
        assert_eq!(
            check_0_spans.type_annotation,
            Some(Span::new(113, 119)),
            "the frame-0 `check` member TYPE span must shift by exactly {delta}",
        );
        assert_eq!(
            check_0_spans.declaration,
            Some(Span::new(110, 119)),
            "the frame-0 `check` member DECLARATION span must shift by exactly {delta}",
        );

        let extends_0_spans = member_spans(extends_0);
        assert_eq!(
            extends_0_spans.name,
            Some(Span::new(140, 141)),
            "the frame-0 `extends` member NAME span must shift by exactly {delta}; an unchanged \
             40..41 proves `extends` was not recursed",
        );
        assert_eq!(
            extends_0_spans.type_annotation,
            Some(Span::new(143, 149)),
            "the frame-0 `extends` member TYPE span must shift by exactly {delta}",
        );

        // Frame 1 — must shift identically: guards `shift_frame_slice` walks the
        // WHOLE slice, not just frame 0.
        let (check_1, extends_1) = frame_types(&ty, 1);
        let check_1_spans = member_spans(check_1);
        assert_eq!(
            check_1_spans.name,
            Some(Span::new(300, 301)),
            "the frame-1 `check` member NAME span must shift by exactly {delta}; an unchanged \
             200..201 proves `shift_frame_slice` only transformed frame 0",
        );
        assert_eq!(
            check_1_spans.type_annotation,
            Some(Span::new(303, 309)),
            "the frame-1 `check` member TYPE span must shift by exactly {delta}",
        );
        assert_eq!(
            check_1_spans.declaration,
            Some(Span::new(300, 309)),
            "the frame-1 `check` member DECLARATION span must shift by exactly {delta}",
        );

        let extends_1_spans = member_spans(extends_1);
        assert_eq!(
            extends_1_spans.name,
            Some(Span::new(330, 331)),
            "the frame-1 `extends` member NAME span must shift by exactly {delta}; an unchanged \
             230..231 proves frame 1 was skipped",
        );
        assert_eq!(
            extends_1_spans.type_annotation,
            Some(Span::new(333, 339)),
            "the frame-1 `extends` member TYPE span must shift by exactly {delta}",
        );
    }

    // Fix 1 (BINDING P1) — `clear_spans` must drop the member spans held INSIDE a
    // `RecursiveRef`'s `conditional_context` frames. Pre-fix the spans survived
    // (honest absence was requested but a wrapper-local offset was retained).
    // This test also pins the dedicated arm's two further promises: it STILL
    // clears `type_arguments` (a span-bearing arg drops to `None`), and
    // `clear_frame_slice` walks EVERY frame (frame 0 AND frame 1 clear). It
    // FAILS against an arm that skips `conditional_context`, drops the
    // `type_arguments` recursion, or only clears the first frame; it PASSES once
    // all three are handled.
    #[test]
    fn clear_spans_drops_recursive_ref_conditional_frame_member_spans() {
        let type_arg = object_with_member_spans(
            "arg",
            Span::new(70, 79),
            Span::new(70, 73),
            Span::new(75, 79),
        );
        let frames = [
            FrameFixture {
                check: object_with_member_spans(
                    "a",
                    Span::new(10, 19),
                    Span::new(10, 11),
                    Span::new(13, 19),
                ),
                extends: object_with_member_spans(
                    "b",
                    Span::new(40, 49),
                    Span::new(40, 41),
                    Span::new(43, 49),
                ),
            },
            // Second frame: a "frame 0 only" clear leaves these spans `Some`.
            FrameFixture {
                check: object_with_member_spans(
                    "c",
                    Span::new(200, 209),
                    Span::new(200, 201),
                    Span::new(203, 209),
                ),
                extends: object_with_member_spans(
                    "d",
                    Span::new(230, 239),
                    Span::new(230, 231),
                    Span::new(233, 239),
                ),
            },
        ];
        let mut ty = recursive_ref_fixture(type_arg, frames);

        ty.clear_spans();

        // The span-bearing `type_arguments` entry must be cleared: guards that
        // the dedicated `RecursiveRef` arm did not drop `clear_arc_slice`.
        let type_arg_spans = member_spans(sole_type_argument(&ty));
        assert_eq!(
            type_arg_spans.name, None,
            "the `type_arguments` member NAME span must be cleared; a surviving 70..73 proves \
             the `RecursiveRef` arm stopped clearing `type_arguments`",
        );
        assert_eq!(
            type_arg_spans.type_annotation, None,
            "the `type_arguments` member TYPE span must be cleared",
        );
        assert_eq!(
            type_arg_spans.declaration, None,
            "the `type_arguments` member DECLARATION span must be cleared",
        );

        // Frame 0.
        let (check_0, extends_0) = frame_types(&ty, 0);
        let check_0_spans = member_spans(check_0);
        assert_eq!(
            check_0_spans.name, None,
            "the frame-0 `check` member NAME span must be cleared; a surviving span proves \
             `clear_spans` skipped `conditional_context`",
        );
        assert_eq!(
            check_0_spans.type_annotation, None,
            "the frame-0 `check` member TYPE span must be cleared",
        );
        assert_eq!(
            check_0_spans.declaration, None,
            "the frame-0 `check` member DECLARATION span must be cleared",
        );

        let extends_0_spans = member_spans(extends_0);
        assert_eq!(
            extends_0_spans.name, None,
            "the frame-0 `extends` member NAME span must be cleared; a surviving span proves \
             `extends` was not recursed",
        );
        assert_eq!(
            extends_0_spans.type_annotation, None,
            "the frame-0 `extends` member TYPE span must be cleared",
        );

        // Frame 1 — must clear identically: guards `clear_frame_slice` walks the
        // WHOLE slice, not just frame 0.
        let (check_1, extends_1) = frame_types(&ty, 1);
        let check_1_spans = member_spans(check_1);
        assert_eq!(
            check_1_spans.name, None,
            "the frame-1 `check` member NAME span must be cleared; a surviving 200..201 proves \
             `clear_frame_slice` only transformed frame 0",
        );
        assert_eq!(
            check_1_spans.type_annotation, None,
            "the frame-1 `check` member TYPE span must be cleared",
        );
        assert_eq!(
            check_1_spans.declaration, None,
            "the frame-1 `check` member DECLARATION span must be cleared",
        );

        let extends_1_spans = member_spans(extends_1);
        assert_eq!(
            extends_1_spans.name, None,
            "the frame-1 `extends` member NAME span must be cleared; a surviving 230..231 proves \
             frame 1 was skipped",
        );
        assert_eq!(
            extends_1_spans.type_annotation, None,
            "the frame-1 `extends` member TYPE span must be cleared",
        );
    }
}
