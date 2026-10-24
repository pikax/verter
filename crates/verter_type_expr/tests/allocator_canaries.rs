use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Counting global allocator. Increments [`ALLOC_COUNTER`] on every
/// allocating call (`alloc` / `alloc_zeroed` / `realloc`), adds the
/// requested size to [`ALLOC_BYTES`], and delegates to the system
/// allocator.
struct CountingAllocator;

thread_local! {
    /// Allocation count for the current harness thread. A `const`
    /// initializer keeps allocator access allocation-free.
    static ALLOC_COUNTER: Cell<u64> = const { Cell::new(0) };
    /// Bytes REQUESTED from the allocator on the current harness
    /// thread. Tracked beside the call count because the two answer
    /// different questions: a container that grows geometrically to
    /// size `n` costs `O(log n)` allocator CALLS but `O(n)` BYTES, so a
    /// regression that rebuilds a whole-file buffer once per
    /// declaration is near-invisible in the call count and quadratic in
    /// the byte count.
    static ALLOC_BYTES: Cell<u64> = const { Cell::new(0) };
    /// Bytes allocated and not yet freed on the current harness thread,
    /// and the highest that count reached since the last reset: the
    /// live construction a measured window holds at its peak.
    static LIVE_BYTES: Cell<i64> = const { Cell::new(0) };
    static PEAK_LIVE_BYTES: Cell<i64> = const { Cell::new(0) };
}

fn increment_alloc_counter(size: usize) {
    // Allocation can occur while a thread is tearing down TLS. Do not
    // turn an otherwise valid allocation into a panic if this key is
    // no longer accessible.
    let _ = ALLOC_COUNTER.try_with(|counter| counter.set(counter.get().wrapping_add(1)));
    let _ = ALLOC_BYTES.try_with(|bytes| bytes.set(bytes.get().wrapping_add(size as u64)));
    add_live_bytes(size as i64);
}

fn add_live_bytes(delta: i64) {
    let _ = LIVE_BYTES.try_with(|live| {
        let now = live.get().wrapping_add(delta);
        live.set(now);
        let _ = PEAK_LIVE_BYTES.try_with(|peak| {
            if now > peak.get() {
                peak.set(now);
            }
        });
    });
}

fn alloc_count() -> u64 {
    ALLOC_COUNTER.with(Cell::get)
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        add_live_bytes(-(layout.size() as i64));
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A grow-in-place realloc still REQUESTS `new_size` bytes; the
        // byte counter records requests, not net residency; the live
        // count trades the old block for the new one.
        add_live_bytes(-(layout.size() as i64));
        increment_alloc_counter(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

mod canary_absolutize_already_absolute_zero_alloc {
    //! Allocation canary for `SemanticTypeSource::absolutized_against`
    //! over a LARGE already-absolute surface.
    //!
    //! The absolutization walker is copy-on-first-change: scanning an
    //! already-absolute source performs NO clones and NO heap allocation
    //! (the dominant case — a fallthrough source re-absolutized under a
    //! consuming scope). The pre-fix walker eagerly cloned every member
    //! into a fresh `Vec` before learning nothing changed, so this canary
    //! reads a per-member allocation delta there and zero here.

    use std::hint::black_box;

    use verter_type_expr::facts::{
        ClosedTypeFact, ObjectMemberFact, ObjectPropertyFact, ObjectShapeFact, SemanticTypeSource,
    };
    use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace, TypeBodySlot};
    use verter_type_expr::span_origins::{MemberSpansOrigin, SourceSynthetic};
    use verter_type_expr::MemberVisibility;

    use super::alloc_count;

    fn absolute_member(index: usize) -> ObjectMemberFact {
        ObjectMemberFact::Property(ObjectPropertyFact {
            key: verter_type_expr::facts::FactAuthoredPropertyKey::string(format!("member{index}")),
            optional: false,
            readonly: false,
            visibility: MemberVisibility::Public,
            ty: TypeBodySlot {
                // ALREADY-ABSOLUTE anchor: nothing to rewrite.
                anchor: AuthoredAnchor {
                    canonical_id: verter_type_expr::OwnedStr::from("/already/absolute.ts"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol: verter_type_expr::OwnedStr::from("Anchored"),
                    space: LocatorSymbolSpace::Type,
                },
                path: verter_type_expr::OwnedSlice::default(),
            },
            span_origin: MemberSpansOrigin::Synthetic(SourceSynthetic),
        })
    }

    #[test]
    fn absolutizing_a_large_already_absolute_surface_allocates_nothing() {
        const MEMBERS: usize = 256;
        let members: Vec<ObjectMemberFact> = (0..MEMBERS).map(absolute_member).collect();
        let source = SemanticTypeSource::Closed(ClosedTypeFact::Object(ObjectShapeFact {
            members: From::from(members.into_boxed_slice()),
        }));

        // Warm any lazy init, then measure the walk alone.
        let _ = black_box(source.absolutized_against("/consumer.vue"));
        let baseline = alloc_count();
        let rewritten = source.absolutized_against("/consumer.vue");
        let after = alloc_count();
        black_box(&rewritten);
        assert_eq!(source, rewritten, "already-absolute input round-trips");
        let delta = after - baseline;
        assert_eq!(
            delta, 0,
            "absolutizing a {MEMBERS}-member already-absolute surface must \
             be allocation-free (copy-on-first-change) — a walker that \
             eagerly clones the members into a Vec before discovering \
             nothing changed reports a per-member delta here; observed {delta}"
        );
    }
}

fn alloc_bytes() -> u64 {
    ALLOC_BYTES.with(Cell::get)
}

mod canary_type_expr_json_work_is_linear_in_output {
    //! Serialising a `TypeExpr` costs work proportional to the JSON it
    //! writes, at any nesting depth.
    //!
    //! Nested tuple, object and heritage-style shapes are encoded at depths
    //! 10, 20 and 40. Written into a buffer already sized for the output, a
    //! streaming encoder allocates nothing at all; one that builds a
    //! `serde_json::Value` first, copying each finished child subtree into
    //! its parent, requests bytes that grow with depth times size. The
    //! `Value` form itself must request a flat number of bytes per output
    //! byte as the depth grows.

    use std::sync::Arc;

    use verter_type_expr::{
        FunctionExpr, FunctionParam, MappedModifier, ObjectExpr, ObjectMember, ObjectProperty,
        PrimitiveName, TupleElement, TypeExpr, TypeParam,
    };

    use super::{alloc_bytes, alloc_count};

    const DEPTHS: [usize; 3] = [10, 20, 40];

    fn string() -> TypeExpr {
        TypeExpr::Primitive(PrimitiveName::String)
    }

    fn tuple(depth: usize) -> TypeExpr {
        let inner = if depth == 0 {
            string()
        } else {
            tuple(depth - 1)
        };
        TypeExpr::Tuple {
            elements: Arc::from(vec![
                TupleElement {
                    label: Some(format!("e{depth}")),
                    ty: inner,
                    optional: false,
                    rest: false,
                },
                TupleElement {
                    label: None,
                    ty: string(),
                    optional: true,
                    rest: false,
                },
            ]),
            readonly: false,
        }
    }

    fn object(depth: usize) -> TypeExpr {
        let inner = if depth == 0 {
            string()
        } else {
            object(depth - 1)
        };
        TypeExpr::Object(Arc::new(ObjectExpr {
            empty_literal: false,
            properties: vec![
                ObjectMember::Property(ObjectProperty::synthetic_public_key(
                    format!("p{depth}").as_str().into(),
                    inner,
                    false,
                    true,
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public_key(
                    "q".into(),
                    string(),
                    true,
                    false,
                )),
            ],
        }))
    }

    /// A heritage-style answer: each level is an object whose member is a
    /// generic function over a mapped, conditional and indexed view of the
    /// level below, returning that level.
    fn heritage(depth: usize) -> TypeExpr {
        let inner = Arc::new(if depth == 0 {
            string()
        } else {
            heritage(depth - 1)
        });
        let reference = |name: &str| {
            Arc::new(TypeExpr::Ref {
                name: name.into(),
                type_arguments: Arc::from(Vec::new()),
            })
        };
        let mapped = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(reference("T"))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: reference("T"),
                index: reference("K"),
            }),
            optional: MappedModifier::None,
            readonly: MappedModifier::Add,
            name_type: None,
        };
        let conditional = TypeExpr::Conditional {
            check: Arc::new(mapped),
            extends: Arc::new(string()),
            true_type: Arc::new(TypeExpr::Union(Arc::from(vec![
                string(),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]))),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        };
        let function = FunctionExpr::synthetic(
            vec![FunctionParam::synthetic(
                Some(format!("a{depth}")),
                conditional,
                false,
                false,
            )],
            Some(inner),
            vec![TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
                is_const: false,
            }],
        );
        TypeExpr::Object(Arc::new(ObjectExpr {
            empty_literal: false,
            properties: vec![ObjectMember::Property(
                ObjectProperty::synthetic_public_key(
                    format!("m{depth}").as_str().into(),
                    TypeExpr::Function(Arc::new(function)),
                    false,
                    false,
                ),
            )],
        }))
    }

    /// Encode `build(depth)` at every depth into a buffer already sized for
    /// its output, and assert the encoder itself allocated nothing: no
    /// intermediate tree, no per-node scratch.
    fn assert_streams(label: &str, build: fn(usize) -> TypeExpr) {
        for depth in DEPTHS {
            let expr = build(depth);
            let len = serde_json::to_vec(&expr).expect("serialises").len();
            let mut out = Vec::with_capacity(len);
            let (calls_before, bytes_before) = (alloc_count(), alloc_bytes());
            serde_json::to_writer(&mut out, &expr).expect("serialises");
            let calls = alloc_count() - calls_before;
            let bytes = alloc_bytes() - bytes_before;
            eprintln!(
                "{label} depth {depth}: {len} B out, {calls} allocations, {bytes} B requested"
            );
            assert_eq!(out.len(), len, "{label}: both encodes write the same bytes");
            assert_eq!(
                (calls, bytes),
                (0, 0),
                "{label}: encoding {len} bytes at depth {depth} into a buffer of that size made \
                 {calls} allocations requesting {bytes} bytes; a streaming encoder writes \
                 straight into the buffer"
            );
        }
    }

    #[test]
    fn encoding_a_nested_tuple_into_a_sized_buffer_allocates_nothing() {
        assert_streams("tuple", tuple);
    }

    #[test]
    fn encoding_a_nested_object_into_a_sized_buffer_allocates_nothing() {
        assert_streams("object", object);
    }

    #[test]
    fn encoding_a_nested_heritage_shape_into_a_sized_buffer_allocates_nothing() {
        assert_streams("heritage", heritage);
    }

    /// The `serde_json::Value` form builds one node per output node, so the
    /// bytes it requests per byte of output stay flat as the depth grows.
    #[test]
    fn building_the_json_value_of_a_nested_shape_requests_bytes_linear_in_its_output() {
        for (label, build) in [
            ("tuple", tuple as fn(usize) -> TypeExpr),
            ("object", object),
            ("heritage", heritage),
        ] {
            let ratios: Vec<(usize, usize, f64)> = DEPTHS
                .iter()
                .map(|&depth| {
                    let expr = build(depth);
                    let len = serde_json::to_vec(&expr).expect("serialises").len();
                    let before = alloc_bytes();
                    let value = expr.to_json_value();
                    let requested = alloc_bytes() - before;
                    drop(value);
                    (depth, len, requested as f64 / len as f64)
                })
                .collect();
            let report = ratios
                .iter()
                .map(|(depth, len, ratio)| format!("depth {depth}: {len} B out, {ratio:.2} B/B"))
                .collect::<Vec<_>>()
                .join("; ");
            eprintln!("{label} to_json_value: {report}");
            let shallow = ratios[0].2;
            for &(depth, _, ratio) in &ratios {
                assert!(
                    ratio <= shallow * 1.25,
                    "{label}: building the JSON value requested {ratio:.2} bytes per output \
                     byte at depth {depth}, against {shallow:.2} at depth {}: {report}",
                    DEPTHS[0]
                );
            }
        }
    }
}
