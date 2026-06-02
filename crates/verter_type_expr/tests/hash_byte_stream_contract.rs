//! Byte-stream contract for [`verter_type_expr::TypeExpr`]'s `Hash`.
//!
//! # Why this exists
//!
//! `TypeExpr` originally `#[derive(Hash)]`d. The derived `Hash` is
//! recursive over the `Arc<TypeExpr>` tree, so a deeply-nested type
//! overflows the stack when hashed (e.g. `cycle_guard::hash_type_expr`
//! routes a `TypeExpr` through `Hash`). That derive is replaced by a
//! hand-written CONTINUATION-FRAME iterative `impl Hash` that is
//! **byte-identical** to the derived behaviour — the same `Hasher`
//! method-call sequence, same field order, same discriminant encoding,
//! same lengths and presence tags.
//!
//! Byte-identity is load-bearing: the `Hash` of a `TypeExpr` feeds
//! content-addressed cache keys (`cycle_guard` shape hashes, derived-
//! `Hash`-keyed maps). A different byte stream would silently change
//! every such key.
//!
//! # How the contract is enforced
//!
//! [`ref_hash`] is a hand-written mirror of the std derive's traversal
//! (discriminant-as-`isize`, then each field in declaration order,
//! recursively). It is the FROZEN contract.
//!
//! [`RecordingHasher`] captures the exact ordered sequence of `Hasher`
//! method calls (not just the final digest — per the review rigor: the
//! actual call/byte stream).
//!
//! The single equivalence test records, for every corpus item:
//! - the stream produced by the LIVE `TypeExpr::hash` (the derive in the
//!   characterization commit, the iterative impl afterwards), and
//! - the stream produced by [`ref_hash`],
//!
//! and asserts the two streams are identical, element for element.
//!
//! - **Characterization commit** (still `#[derive(Hash)]`): proves
//!   `ref_hash` is byte-identical to the real derive — so the mirror is
//!   a faithful contract.
//! - **Iterative commit** (derive removed, manual `impl Hash`): the same
//!   assertion proves the iterative impl reproduces that exact stream.
//!
//! If either side drifts, the test fails with the first differing event.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use verter_span::Span;
use verter_type_expr::{
    FunctionExpr, FunctionParam, FunctionSpans, IndexSignature, IndexSignatureSpans, LiteralValue,
    MappedModifier, MemberSpans, MemberVisibility, MethodSignature, ObjectExpr, ObjectMember,
    ObjectProperty, PrimitiveName, RecursiveConditionalBranch, RecursiveConditionalFrame,
    SyntheticCarrierKey, SyntheticCarrierSurfaceKind, TupleElement, TypeExpr, TypeParam, ValueRef,
};

// ---------------------------------------------------------------------------
// Recording hasher — captures the exact ordered Hasher call stream
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum HashEvent {
    Bytes(Vec<u8>),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    Usize(usize),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    Isize(isize),
}

#[derive(Default)]
struct RecordingHasher {
    events: Vec<HashEvent>,
}

impl RecordingHasher {
    fn record<F: FnOnce(&mut RecordingHasher)>(f: F) -> Vec<HashEvent> {
        let mut h = RecordingHasher::default();
        f(&mut h);
        h.events
    }
}

impl Hasher for RecordingHasher {
    fn finish(&self) -> u64 {
        // Not used for equivalence — the event stream is the contract.
        0
    }
    fn write(&mut self, bytes: &[u8]) {
        self.events.push(HashEvent::Bytes(bytes.to_vec()));
    }
    fn write_u8(&mut self, i: u8) {
        self.events.push(HashEvent::U8(i));
    }
    fn write_u16(&mut self, i: u16) {
        self.events.push(HashEvent::U16(i));
    }
    fn write_u32(&mut self, i: u32) {
        self.events.push(HashEvent::U32(i));
    }
    fn write_u64(&mut self, i: u64) {
        self.events.push(HashEvent::U64(i));
    }
    fn write_u128(&mut self, i: u128) {
        self.events.push(HashEvent::U128(i));
    }
    fn write_usize(&mut self, i: usize) {
        self.events.push(HashEvent::Usize(i));
    }
    fn write_i8(&mut self, i: i8) {
        self.events.push(HashEvent::I8(i));
    }
    fn write_i16(&mut self, i: i16) {
        self.events.push(HashEvent::I16(i));
    }
    fn write_i32(&mut self, i: i32) {
        self.events.push(HashEvent::I32(i));
    }
    fn write_i64(&mut self, i: i64) {
        self.events.push(HashEvent::I64(i));
    }
    fn write_i128(&mut self, i: i128) {
        self.events.push(HashEvent::I128(i));
    }
    fn write_isize(&mut self, i: isize) {
        self.events.push(HashEvent::Isize(i));
    }
}

// ---------------------------------------------------------------------------
// Reference mirror of the std `#[derive(Hash)]` traversal
// ---------------------------------------------------------------------------
//
// The std derive for an enum emits `discriminant_value(self).hash(h)`
// (the discriminant, an `isize` for a default-repr enum, in declaration
// order 0,1,2,...) and then hashes each field in declaration order.
// `ref_hash` reproduces that exactly. Supporting structs hash their
// fields in declaration order; the hand-written `Hash` impls on
// `LiteralValue` and `FunctionParam` are mirrored faithfully (the latter
// EXCLUDES `has_ts_annotation`).
//
// `ref_hash` is recursive — that is fine, it is test-only over a shallow
// corpus and exists purely to pin the byte stream the production impl
// must reproduce.

fn variant_index(expr: &TypeExpr) -> isize {
    match expr {
        TypeExpr::Primitive(_) => 0,
        TypeExpr::Literal(_) => 1,
        TypeExpr::Union(_) => 2,
        TypeExpr::Intersection(_) => 3,
        TypeExpr::Array { .. } => 4,
        TypeExpr::Tuple { .. } => 5,
        TypeExpr::Object(_) => 6,
        TypeExpr::Function(_) => 7,
        TypeExpr::Ref { .. } => 8,
        TypeExpr::TypeParameter(_) => 9,
        TypeExpr::KeyOf(_) => 10,
        TypeExpr::TypeOf(_) => 11,
        TypeExpr::IndexedAccess { .. } => 12,
        TypeExpr::Conditional { .. } => 13,
        TypeExpr::Mapped { .. } => 14,
        TypeExpr::TemplateLiteral { .. } => 15,
        TypeExpr::Infer { .. } => 16,
        TypeExpr::Rest(_) => 17,
        TypeExpr::Parenthesized(_) => 18,
        TypeExpr::RecursiveRef { .. } => 19,
        TypeExpr::SyntheticSlotBinding(_) => 20,
        TypeExpr::Unknown { .. } => 21,
        // Added after the original derive: a new variant takes the next free
        // discriminant (NOT its declaration-order index) so 0..=21 stay frozen.
        TypeExpr::ConstructorType(_) => 22,
    }
}

fn ref_hash<H: Hasher>(expr: &TypeExpr, h: &mut H) {
    // Discriminant first, exactly as the derive: `isize`.
    variant_index(expr).hash(h);
    match expr {
        TypeExpr::Primitive(name) => name.hash(h),
        TypeExpr::Literal(lit) => lit.hash(h),
        TypeExpr::Union(items) | TypeExpr::Intersection(items) => ref_hash_slice(items, h),
        TypeExpr::Array { element, readonly } => {
            ref_hash(element, h);
            readonly.hash(h);
        }
        TypeExpr::Tuple { elements, readonly } => {
            elements.len().hash(h);
            for el in elements.iter() {
                ref_hash_tuple_element(el, h);
            }
            readonly.hash(h);
        }
        TypeExpr::Object(obj) => {
            obj.properties.len().hash(h);
            for member in &obj.properties {
                ref_hash_object_member(member, h);
            }
        }
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => ref_hash_function(func, h),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            name.hash(h);
            ref_hash_slice(type_arguments, h);
        }
        TypeExpr::TypeParameter(tp) => ref_hash_type_param(tp, h),
        TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) | TypeExpr::Parenthesized(inner) => {
            ref_hash(inner, h);
        }
        TypeExpr::TypeOf(value_ref) => value_ref.hash(h),
        TypeExpr::IndexedAccess { object, index } => {
            ref_hash(object, h);
            ref_hash(index, h);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            ref_hash(check, h);
            ref_hash(extends, h);
            ref_hash(true_type, h);
            ref_hash(false_type, h);
        }
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            parameter.hash(h);
            ref_hash(source, h);
            ref_hash(value, h);
            optional.hash(h);
            readonly.hash(h);
            ref_hash_opt(name_type.as_deref(), h);
        }
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => {
            quasis.hash(h);
            ref_hash_slice(expressions, h);
        }
        TypeExpr::Infer { name } => name.hash(h),
        TypeExpr::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        } => {
            name.hash(h);
            ref_hash_slice(type_arguments, h);
            conditional_context.len().hash(h);
            for frame in conditional_context.iter() {
                ref_hash_recursive_frame(frame, h);
            }
        }
        TypeExpr::SyntheticSlotBinding(carrier) => carrier.hash(h),
        TypeExpr::Unknown { raw } => raw.hash(h),
    }
}

fn ref_hash_slice<H: Hasher>(items: &Arc<[TypeExpr]>, h: &mut H) {
    items.len().hash(h);
    for item in items.iter() {
        ref_hash(item, h);
    }
}

fn ref_hash_opt<H: Hasher>(opt: Option<&TypeExpr>, h: &mut H) {
    // `Option<Arc<TypeExpr>>::hash`: discriminant (isize) then inner.
    match opt {
        None => 0isize.hash(h),
        Some(inner) => {
            1isize.hash(h);
            ref_hash(inner, h);
        }
    }
}

fn ref_hash_tuple_element<H: Hasher>(el: &TupleElement, h: &mut H) {
    el.label.hash(h);
    ref_hash(&el.ty, h);
    el.optional.hash(h);
    el.rest.hash(h);
}

fn ref_hash_object_member<H: Hasher>(member: &ObjectMember, h: &mut H) {
    // `#[derive(Hash)]` on the enum: discriminant (isize) then fields.
    match member {
        ObjectMember::Property(p) => {
            0isize.hash(h);
            p.name.hash(h);
            ref_hash(&p.ty, h);
            p.optional.hash(h);
            p.readonly.hash(h);
            // Marker-only-for-non-public: a `Public` property emits NO
            // visibility bytes (so an all-public surface's stream is identical
            // to the pre-visibility stream); a non-public property folds its
            // visibility discriminant.
            if !p.visibility.is_public() {
                p.visibility.hash(h);
            }
            p.spans.hash(h);
        }
        ObjectMember::IndexSignature(s) => {
            1isize.hash(h);
            s.key_name.hash(h);
            ref_hash(&s.key_type, h);
            ref_hash(&s.value_type, h);
            s.readonly.hash(h);
            s.spans.hash(h);
        }
        ObjectMember::CallSignature(f) => {
            2isize.hash(h);
            ref_hash_function(f, h);
        }
        ObjectMember::ConstructSignature(f) => {
            3isize.hash(h);
            ref_hash_function(f, h);
        }
        ObjectMember::Method(m) => {
            4isize.hash(h);
            ref_hash_method(m, h);
        }
    }
}

fn ref_hash_method<H: Hasher>(m: &MethodSignature, h: &mut H) {
    m.name.hash(h);
    ref_hash_function(&m.function, h);
    m.optional.hash(h);
    // Marker-only-for-non-public (see `ref_hash_object_member`).
    if !m.visibility.is_public() {
        m.visibility.hash(h);
    }
    m.spans.hash(h);
}

fn ref_hash_function<H: Hasher>(func: &FunctionExpr, h: &mut H) {
    func.parameters.len().hash(h);
    for p in &func.parameters {
        ref_hash_param(p, h);
    }
    ref_hash_opt(func.return_type.as_deref(), h);
    func.type_parameters.len().hash(h);
    for tp in &func.type_parameters {
        ref_hash_type_param(tp, h);
    }
    func.spans.hash(h);
}

fn ref_hash_param<H: Hasher>(p: &FunctionParam, h: &mut H) {
    // Mirrors the hand-written `Hash for FunctionParam`: EXCLUDES
    // `has_ts_annotation`.
    p.name.hash(h);
    ref_hash(&p.ty, h);
    p.optional.hash(h);
    p.rest.hash(h);
    p.span.hash(h);
}

fn ref_hash_type_param<H: Hasher>(tp: &TypeParam, h: &mut H) {
    tp.name.hash(h);
    ref_hash_opt(tp.constraint.as_deref(), h);
    ref_hash_opt(tp.default.as_deref(), h);
}

fn ref_hash_recursive_frame<H: Hasher>(frame: &RecursiveConditionalFrame, h: &mut H) {
    frame.branch.hash(h);
    frame.decided.hash(h);
    ref_hash(&frame.check, h);
    ref_hash(&frame.extends, h);
}

// ---------------------------------------------------------------------------
// Corpus — every variant + interleavings + edge cases
// ---------------------------------------------------------------------------

fn arc(t: TypeExpr) -> Arc<TypeExpr> {
    Arc::new(t)
}

fn span(a: u32, b: u32) -> Span {
    Span::new(a, b)
}

fn corpus() -> Vec<(&'static str, TypeExpr)> {
    let mut v: Vec<(&'static str, TypeExpr)> = Vec::new();

    // Primitives (every name — exercises the leaf enum discriminant).
    for (n, p) in [
        ("string", PrimitiveName::String),
        ("number", PrimitiveName::Number),
        ("boolean", PrimitiveName::Boolean),
        ("symbol", PrimitiveName::Symbol),
        ("bigint", PrimitiveName::BigInt),
        ("any", PrimitiveName::Any),
        ("unknown", PrimitiveName::Unknown),
        ("void", PrimitiveName::Void),
        ("never", PrimitiveName::Never),
        ("null", PrimitiveName::Null),
        ("undefined", PrimitiveName::Undefined),
        ("object", PrimitiveName::Object),
    ] {
        let _ = n;
        v.push(("primitive", TypeExpr::Primitive(p)));
    }

    // Literals incl. float edge cases (NaN / -0.0 / inf).
    v.push((
        "lit-str",
        TypeExpr::Literal(LiteralValue::String("hi".into())),
    ));
    v.push(("lit-num", TypeExpr::Literal(LiteralValue::Number(42.0))));
    v.push((
        "lit-num-neg0",
        TypeExpr::Literal(LiteralValue::Number(-0.0)),
    ));
    v.push((
        "lit-num-nan",
        TypeExpr::Literal(LiteralValue::Number(f64::NAN)),
    ));
    v.push((
        "lit-num-inf",
        TypeExpr::Literal(LiteralValue::Number(f64::INFINITY)),
    ));
    v.push(("lit-bool", TypeExpr::Literal(LiteralValue::Boolean(true))));
    v.push((
        "lit-bigint",
        TypeExpr::Literal(LiteralValue::BigInt("123".into())),
    ));

    // Union / Intersection — empty and multi-element (ordering).
    v.push((
        "union-empty",
        TypeExpr::Union(Arc::from([] as [TypeExpr; 0])),
    ));
    v.push((
        "union-multi",
        TypeExpr::Union(Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
            TypeExpr::Literal(LiteralValue::Boolean(false)),
        ])),
    ));
    v.push((
        "intersection-multi",
        TypeExpr::Intersection(Arc::from(vec![TypeExpr::named("A"), TypeExpr::named("B")])),
    ));

    // Array (readonly true/false) — trailing-bool-after-child.
    v.push((
        "array-ro",
        TypeExpr::Array {
            element: arc(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: true,
        },
    ));
    v.push((
        "array-mut",
        TypeExpr::Array {
            element: arc(TypeExpr::named("Foo")),
            readonly: false,
        },
    ));

    // Tuple — labelled / optional / rest elements + trailing readonly.
    v.push((
        "tuple",
        TypeExpr::Tuple {
            elements: Arc::from(vec![
                TupleElement {
                    label: Some("a".into()),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    rest: false,
                },
                TupleElement {
                    label: None,
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: true,
                    rest: false,
                },
                TupleElement {
                    label: Some("rest".into()),
                    ty: TypeExpr::Array {
                        element: arc(TypeExpr::named("X")),
                        readonly: false,
                    },
                    optional: false,
                    rest: true,
                },
            ]),
            readonly: true,
        },
    ));
    v.push((
        "tuple-empty",
        TypeExpr::Tuple {
            elements: Arc::from([] as [TupleElement; 0]),
            readonly: false,
        },
    ));

    // Object — every ObjectMember variant, with and without spans.
    v.push((
        "object-all-members",
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::with_spans_public(
                    "p".into(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    true,
                    true,
                    MemberSpans {
                        declaration: Some(span(1, 9)),
                        name: Some(span(1, 2)),
                        type_annotation: Some(span(4, 9)),
                    },
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "q".into(),
                    TypeExpr::named("Q"),
                    false,
                    false,
                )),
                ObjectMember::IndexSignature(IndexSignature::with_spans(
                    "k".into(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    TypeExpr::Primitive(PrimitiveName::Number),
                    false,
                    IndexSignatureSpans {
                        declaration: Some(span(10, 30)),
                        key: Some(span(11, 20)),
                        value: Some(span(22, 30)),
                    },
                )),
                ObjectMember::CallSignature(sample_function(false)),
                ObjectMember::ConstructSignature(sample_function(true)),
                ObjectMember::Method(MethodSignature::with_spans_public(
                    "m".into(),
                    sample_function(false),
                    true,
                    MemberSpans::name_only(span(40, 41)),
                )),
            ],
        })),
    ));

    // Object with NON-public class members — exercises the visibility field in
    // the hash byte stream (a `protected` property + a `private` method). The
    // live iterative impl and the frozen `ref_hash` mirror must both emit the
    // visibility discriminant in declaration order; this item discriminates a
    // mirror that forgets to hash visibility.
    v.push((
        "object-nonpublic-members",
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::with_visibility(
                    "prot".into(),
                    TypeExpr::Primitive(PrimitiveName::Number),
                    false,
                    false,
                    MemberVisibility::Protected,
                    MemberSpans::default(),
                )),
                ObjectMember::Method(MethodSignature::with_visibility(
                    "priv".into(),
                    sample_function(false),
                    false,
                    MemberVisibility::Private,
                    MemberSpans::default(),
                )),
            ],
        })),
    ));

    // Function variant directly (params + type-params + return + spans).
    v.push((
        "function",
        TypeExpr::Function(Arc::new(sample_function(true))),
    ));
    v.push((
        "function-no-return",
        TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
            vec![FunctionParam::synthetic(
                Some("x".into()),
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
            )],
            None,
            Vec::new(),
        ))),
    ));
    // ConstructorType variant directly — same FunctionExpr payload as the
    // Function corpus item, so the only stream difference is the leading
    // discriminant (22 vs 7). Exercises the new variant through the live
    // iterative `hash_node` and the frozen `ref_hash` mirror.
    v.push((
        "constructor-type",
        TypeExpr::ConstructorType(Arc::new(sample_function(true))),
    ));
    // FunctionParam: same identity fields but DIFFERENT has_ts_annotation
    // must hash identically (the field is excluded). Both pushed; the
    // equivalence test runs per item, and a dedicated test below asserts
    // the two streams are equal to each other.

    // Ref — empty and non-empty type arguments.
    v.push((
        "ref-empty",
        TypeExpr::Ref {
            name: Arc::from("Bare"),
            type_arguments: Arc::from([] as [TypeExpr; 0]),
        },
    ));
    v.push((
        "ref-args",
        TypeExpr::Ref {
            name: Arc::from("Record"),
            type_arguments: Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::named("V"),
            ]),
        },
    ));

    // TypeParameter — constraint Some/None, default Some/None.
    v.push((
        "typeparam-full",
        TypeExpr::TypeParameter(TypeParam {
            name: "T".into(),
            constraint: Some(arc(TypeExpr::named("Base"))),
            default: Some(arc(TypeExpr::Primitive(PrimitiveName::String))),
        }),
    ));
    v.push((
        "typeparam-bare",
        TypeExpr::TypeParameter(TypeParam {
            name: "U".into(),
            constraint: None,
            default: None,
        }),
    ));

    // KeyOf / Rest / Parenthesized.
    v.push(("keyof", TypeExpr::KeyOf(arc(TypeExpr::named("Foo")))));
    v.push(("rest", TypeExpr::Rest(arc(TypeExpr::named("R")))));
    v.push((
        "paren",
        TypeExpr::Parenthesized(arc(TypeExpr::Union(Arc::from(vec![
            TypeExpr::named("A"),
            TypeExpr::named("B"),
        ])))),
    ));

    // TypeOf — empty and multi-segment path.
    v.push(("typeof-empty", TypeExpr::TypeOf(ValueRef { path: vec![] })));
    v.push((
        "typeof-path",
        TypeExpr::TypeOf(ValueRef {
            path: vec!["a".into(), "b".into(), "c".into()],
        }),
    ));

    // IndexedAccess.
    v.push((
        "indexed",
        TypeExpr::IndexedAccess {
            object: arc(TypeExpr::named("Foo")),
            index: arc(TypeExpr::Literal(LiteralValue::String("bar".into()))),
        },
    ));

    // Conditional (all four children).
    v.push((
        "conditional",
        TypeExpr::Conditional {
            check: arc(TypeExpr::named("T")),
            extends: arc(TypeExpr::named("U")),
            true_type: arc(TypeExpr::Primitive(PrimitiveName::String)),
            false_type: arc(TypeExpr::Primitive(PrimitiveName::Number)),
        },
    ));

    // Mapped — interleaved modifiers between children + name_type Some/None.
    v.push((
        "mapped-named",
        TypeExpr::Mapped {
            parameter: "K".into(),
            source: arc(TypeExpr::KeyOf(arc(TypeExpr::named("Foo")))),
            value: arc(TypeExpr::IndexedAccess {
                object: arc(TypeExpr::named("Foo")),
                index: arc(TypeExpr::named("K")),
            }),
            optional: MappedModifier::Add,
            readonly: MappedModifier::Remove,
            name_type: Some(arc(TypeExpr::Primitive(PrimitiveName::String))),
        },
    ));
    v.push((
        "mapped-bare",
        TypeExpr::Mapped {
            parameter: "P".into(),
            source: arc(TypeExpr::named("S")),
            value: arc(TypeExpr::named("V")),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: None,
        },
    ));

    // TemplateLiteral — multiple quasis + multiple expressions.
    v.push((
        "template",
        TypeExpr::TemplateLiteral {
            quasis: vec!["pre-".into(), "-mid-".into(), "-post".into()],
            expressions: Arc::from(vec![
                TypeExpr::named("A"),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]),
        },
    ));
    v.push((
        "template-empty-exprs",
        TypeExpr::TemplateLiteral {
            quasis: vec!["only".into()],
            expressions: Arc::from([] as [TypeExpr; 0]),
        },
    ));

    // Infer.
    v.push(("infer", TypeExpr::Infer { name: "R".into() }));

    // RecursiveRef — type_arguments + conditional_context (both branches).
    v.push((
        "recursive-ref",
        TypeExpr::RecursiveRef {
            name: Arc::from("Rec"),
            type_arguments: Arc::from(vec![TypeExpr::named("T")]),
            conditional_context: Arc::from(vec![
                RecursiveConditionalFrame {
                    branch: RecursiveConditionalBranch::True,
                    decided: true,
                    check: arc(TypeExpr::named("C")),
                    extends: arc(TypeExpr::named("E")),
                },
                RecursiveConditionalFrame {
                    branch: RecursiveConditionalBranch::False,
                    decided: false,
                    check: arc(TypeExpr::Primitive(PrimitiveName::Never)),
                    extends: arc(TypeExpr::Primitive(PrimitiveName::Unknown)),
                },
            ]),
        },
    ));
    v.push((
        "recursive-ref-empty-ctx",
        TypeExpr::RecursiveRef {
            name: Arc::from("Rec2"),
            type_arguments: Arc::from([] as [TypeExpr; 0]),
            conditional_context: Arc::from([] as [RecursiveConditionalFrame; 0]),
        },
    ));

    // SyntheticSlotBinding — slot_name Some/None, both surface kinds.
    v.push((
        "synthetic-slot",
        TypeExpr::SyntheticSlotBinding(Arc::new(SyntheticCarrierKey {
            scope_canonical_id: Arc::from("/owner.vue"),
            surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
            slot_name: Some(Arc::from("default")),
            binding_name: Arc::from("row"),
            value_node: 7,
        })),
    ));
    v.push((
        "synthetic-binding",
        TypeExpr::SyntheticSlotBinding(Arc::new(SyntheticCarrierKey {
            scope_canonical_id: Arc::from("/owner2.vue"),
            surface_kind: SyntheticCarrierSurfaceKind::Binding,
            slot_name: None,
            binding_name: Arc::from("b"),
            value_node: 0,
        })),
    ));

    // Unknown.
    v.push((
        "unknown",
        TypeExpr::Unknown {
            raw: "weird<>".into(),
        },
    ));

    // Deep-ish nesting to exercise multi-level child ordering.
    let mut nested = TypeExpr::named("Leaf");
    for _ in 0..6 {
        nested = TypeExpr::Array {
            element: arc(nested),
            readonly: false,
        };
    }
    v.push(("nested-array", nested));

    v
}

fn sample_function(with_typeparams: bool) -> FunctionExpr {
    FunctionExpr::with_spans(
        vec![
            FunctionParam::with_span(
                Some("a".into()),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
                Some(span(2, 3)),
                true,
            ),
            FunctionParam::synthetic(
                Some("rest".into()),
                TypeExpr::Array {
                    element: arc(TypeExpr::named("Y")),
                    readonly: false,
                },
                false,
                true,
            ),
        ],
        Some(arc(TypeExpr::Primitive(PrimitiveName::Void))),
        if with_typeparams {
            vec![TypeParam {
                name: "T".into(),
                constraint: Some(arc(TypeExpr::named("Base"))),
                default: None,
            }]
        } else {
            Vec::new()
        },
        FunctionSpans {
            signature: Some(span(0, 50)),
            return_type: Some(span(45, 49)),
        },
    )
}

// ---------------------------------------------------------------------------
// The contract test
// ---------------------------------------------------------------------------

/// For every corpus item, the LIVE `TypeExpr::hash` byte/event stream
/// must be IDENTICAL to the frozen `ref_hash` mirror.
///
/// - While `TypeExpr` still `#[derive(Hash)]`: proves `ref_hash` mirrors
///   the real derive exactly (characterization).
/// - After the derive is replaced with the iterative impl: proves the
///   iterative impl reproduces that exact stream (byte-identity).
#[test]
fn live_hash_stream_matches_reference_mirror_for_every_variant() {
    for (label, expr) in corpus() {
        let live = RecordingHasher::record(|h| expr.hash(h));
        let reference = RecordingHasher::record(|h| ref_hash(&expr, h));
        assert_eq!(
            live.len(),
            reference.len(),
            "event COUNT mismatch for `{label}`: live={} reference={}",
            live.len(),
            reference.len(),
        );
        for (i, (l, r)) in live.iter().zip(reference.iter()).enumerate() {
            assert_eq!(
                l, r,
                "event #{i} differs for `{label}`: live={l:?} reference={r:?}",
            );
        }
        // A non-trivial stream — guards against a degenerate mirror that
        // records nothing.
        assert!(
            !live.is_empty(),
            "live hash stream for `{label}` must be non-empty",
        );
    }
}

/// `FunctionParam::has_ts_annotation` is excluded from identity: two
/// params equal in every other field but differing in that flag must
/// hash to the IDENTICAL byte stream (both via the live impl).
#[test]
fn function_param_has_ts_annotation_is_excluded_from_hash_stream() {
    let with_annotation = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::with_span(
            Some("a".into()),
            TypeExpr::Primitive(PrimitiveName::String),
            false,
            false,
            Some(span(1, 2)),
            true, // has_ts_annotation = true
        )],
        None,
        Vec::new(),
    )));
    let without_annotation = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::with_span(
            Some("a".into()),
            TypeExpr::Primitive(PrimitiveName::String),
            false,
            false,
            Some(span(1, 2)),
            false, // has_ts_annotation = false
        )],
        None,
        Vec::new(),
    )));

    let a = RecordingHasher::record(|h| with_annotation.hash(h));
    let b = RecordingHasher::record(|h| without_annotation.hash(h));
    assert_eq!(
        a, b,
        "has_ts_annotation must not affect the hash byte stream",
    );
}

/// Re-hashing the same value must be deterministic (same stream twice).
#[test]
fn live_hash_stream_is_deterministic() {
    for (label, expr) in corpus() {
        let first = RecordingHasher::record(|h| expr.hash(h));
        let second = RecordingHasher::record(|h| expr.hash(h));
        assert_eq!(first, second, "non-deterministic hash stream for `{label}`");
    }
}

/// Hashing a deeply-nested `TypeExpr` must NOT overflow the stack on a
/// default thread stack. The former derived `Hash` was recursive over
/// the `Arc<TypeExpr>` tree and overflowed on a depth this large; the
/// continuation-frame iterative impl walks it on the heap.
///
/// Discrimination: with the manual `impl Hash` reverted to
/// `#[derive(Hash)]`, this aborts with STATUS_STACK_OVERFLOW on a default
/// stack (the same hazard class the iterative impl removes).
#[test]
fn deeply_nested_type_hashes_without_stack_overflow() {
    use std::hash::Hasher as _;

    const DEPTH: usize = 200_000;
    let mut current = arc(TypeExpr::Primitive(PrimitiveName::String));
    for _ in 0..DEPTH {
        current = arc(TypeExpr::Array {
            element: current,
            readonly: false,
        });
    }

    // A real `Hasher` (not the recording one) — proves the production
    // hashing path is depth-safe end to end.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    current.hash(&mut hasher);
    // Touch the digest so the hash is not optimised away.
    assert_ne!(hasher.finish(), 0, "deep hash must produce a digest");

    // The deep `current` also drops here without overflow (iterative
    // `Drop`), so the whole build/hash/drop cycle is depth-safe.
}

// ---------------------------------------------------------------------------
// Pre-visibility byte-stream stability (H1)
// ---------------------------------------------------------------------------
//
// `pre_visibility_ref_hash` is the FROZEN pre-B4.5 mirror: identical to
// `ref_hash` EXCEPT it never folds member visibility at all (as if the
// `visibility` field did not exist). It pins the exact byte stream that every
// pre-existing all-public surface produced before member visibility was added,
// so the marker-only-for-non-public scheme can be proven to leave that stream
// untouched (zero cache-identity churn).

fn pre_visibility_ref_hash<H: Hasher>(expr: &TypeExpr, h: &mut H) {
    match expr {
        TypeExpr::Object(obj) => {
            // Mirror `ref_hash`'s Object arm exactly (discriminant + len +
            // members), but route members through the visibility-free member
            // hasher. The inner member TYPES recurse back through THIS function
            // so a nested object member is also visibility-free.
            variant_index(expr).hash(h);
            obj.properties.len().hash(h);
            for member in &obj.properties {
                pre_visibility_ref_object_member(member, h);
            }
        }
        // Delegate every non-object node to the live mirror (which hashes its
        // own discriminant): only object members carry visibility, so no other
        // node differs between the pre- and post-visibility streams. NOTE: any
        // nested object reached THROUGH a non-object node here uses `ref_hash`
        // (post-visibility) — acceptable because the corpus for these tests is
        // an object at the root, exercising the member path directly.
        _ => ref_hash(expr, h),
    }
}

fn pre_visibility_ref_object_member<H: Hasher>(member: &ObjectMember, h: &mut H) {
    match member {
        ObjectMember::Property(p) => {
            0isize.hash(h);
            p.name.hash(h);
            pre_visibility_ref_hash(&p.ty, h);
            p.optional.hash(h);
            p.readonly.hash(h);
            // NO visibility fold — the pre-B4.5 stream.
            p.spans.hash(h);
        }
        ObjectMember::Method(m) => {
            4isize.hash(h);
            m.name.hash(h);
            ref_hash_function(&m.function, h);
            m.optional.hash(h);
            // NO visibility fold — the pre-B4.5 stream.
            m.spans.hash(h);
        }
        // Index / call / construct members never carried visibility; reuse the
        // live mirror (it is identical to the pre-visibility behaviour for them).
        other => ref_hash_object_member(other, h),
    }
}

/// An object whose members are ALL `Public` must hash to the EXACT pre-B4.5
/// byte stream — the marker-only-for-non-public scheme emits NO visibility
/// bytes for a public member, so every pre-existing all-public cache key is
/// byte-identical (zero churn).
///
/// Discrimination: against the tree that folds `Public` UNCONDITIONALLY
/// (B4.5-as-landed), the live stream contains an extra `Isize(0)` per member
/// and this `assert_eq!` FAILS. With the fix it PASSES.
#[test]
fn all_public_object_hash_stream_is_unchanged_from_pre_visibility() {
    let public_object = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::with_spans_public(
                "p".into(),
                TypeExpr::Primitive(PrimitiveName::String),
                true,
                true,
                MemberSpans {
                    declaration: Some(span(1, 9)),
                    name: Some(span(1, 2)),
                    type_annotation: Some(span(4, 9)),
                },
            )),
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "q".into(),
                TypeExpr::named("Q"),
                false,
                false,
            )),
            ObjectMember::Method(MethodSignature::with_spans_public(
                "m".into(),
                sample_function(false),
                true,
                MemberSpans::name_only(span(40, 41)),
            )),
        ],
    }));

    let live = RecordingHasher::record(|h| public_object.hash(h));
    let pre_visibility = RecordingHasher::record(|h| pre_visibility_ref_hash(&public_object, h));
    assert_eq!(
        live, pre_visibility,
        "an all-public object's live hash byte stream must equal the pre-B4.5 \
         (visibility-free) stream — a public member must emit NO visibility bytes",
    );
    // Guard against a degenerate (empty) stream.
    assert!(!live.is_empty(), "stream must be non-empty");
}

/// A non-public member MUST change the byte stream relative to the
/// pre-visibility (visibility-free) reference — the marker is real, not a
/// no-op. Distinct visibilities (`Protected` vs `Private`) produce distinct
/// streams.
///
/// Discrimination: if the producer pushed NO marker for non-public members
/// either (a broken "always skip" fix), the `assert_ne!`s would FAIL.
#[test]
fn non_public_member_changes_hash_stream_from_pre_visibility() {
    let make = |vis: MemberVisibility| {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::with_visibility(
                "x".into(),
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
                vis,
                MemberSpans::default(),
            ))],
        }))
    };

    let public = make(MemberVisibility::Public);
    let protected = make(MemberVisibility::Protected);
    let private = make(MemberVisibility::Private);

    let public_stream = RecordingHasher::record(|h| public.hash(h));
    let pre_vis_public = RecordingHasher::record(|h| pre_visibility_ref_hash(&public, h));
    // Public is unchanged from the pre-visibility stream.
    assert_eq!(public_stream, pre_vis_public);

    let protected_stream = RecordingHasher::record(|h| protected.hash(h));
    let private_stream = RecordingHasher::record(|h| private.hash(h));

    // Both non-public streams DIFFER from the public (pre-visibility) stream.
    assert_ne!(
        protected_stream, public_stream,
        "a protected member must fold a visibility marker",
    );
    assert_ne!(
        private_stream, public_stream,
        "a private member must fold a visibility marker",
    );
    // Protected and Private are mutually distinct.
    assert_ne!(
        protected_stream, private_stream,
        "protected and private must produce distinct streams",
    );
}
