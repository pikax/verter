//! Discriminating regression tests for the `PublishedSurfacePolicy`
//! registry. Each test holds for one specific structural rule and
//! FAILS if that rule is dropped or relaxed.

use super::published_surface::{
    event_name_to_on_prop_name, names_for_policy, AnalyzedSurface, AnalyzedSurfaceItem,
    PublishedSurfacePolicy, COMPAT_BLOCKED_SLOT_NAMES, VUE_INTRINSIC_ATTR_NAMES,
};

fn item(name: &str) -> AnalyzedSurfaceItem {
    AnalyzedSurfaceItem {
        name: name.to_string(),
        declared_in_macro_type_arg: false,
        global: false,
    }
}

fn item_declared(name: &str) -> AnalyzedSurfaceItem {
    AnalyzedSurfaceItem {
        name: name.to_string(),
        declared_in_macro_type_arg: true,
        global: false,
    }
}

fn item_global(name: &str) -> AnalyzedSurfaceItem {
    AnalyzedSurfaceItem {
        name: name.to_string(),
        declared_in_macro_type_arg: false,
        global: true,
    }
}

#[test]
fn native_policy_returns_every_name_unfiltered() {
    // Native is the producer's raw truth — no filtering on any
    // surface, regardless of `declared_in_macro_type_arg`,
    // `global`, intrinsic membership, or COMPAT slot blocklist
    // membership. If any filter creeps in, this test fails.
    let surface = AnalyzedSurface {
        props: vec![
            item("title"),
            item("class"),
            item("onSubmit"),
            item_global("autofocus"),
        ],
        events: vec![item("submit")],
        slots: vec![item("default"), item("key")],
        exposed: vec![item("focus")],
    };
    let r = names_for_policy(PublishedSurfacePolicy::Native, &surface);

    assert_eq!(
        r.props,
        vec!["title", "class", "onSubmit", "autofocus"],
        "Native must preserve every published prop name without filtering, \
         including intrinsics, shadow-event-prop names, and global-flagged \
         props. Filtering at Native is a producer-vs-consumer-surface \
         conflation regression."
    );
    assert_eq!(r.events, vec!["submit"], "Native events unfiltered");
    assert_eq!(
        r.slots,
        vec!["default", "key"],
        "Native must NOT apply COMPAT_BLOCKED_SLOT_NAMES — 'key' must survive"
    );
    assert_eq!(r.exposed, vec!["focus"], "Native exposed unfiltered");
}

#[test]
fn compat_policy_strips_only_compat_blocked_slot_names_and_only_on_slots() {
    // Compat = native minus COMPAT_BLOCKED_SLOT_NAMES applied on
    // the slots surface only. Props / events / exposed pass
    // through unchanged. This test fails if Compat starts filtering
    // any non-slot surface, or fails to filter a known blocklist
    // name from the slots surface.
    let mut surface = AnalyzedSurface::default();
    // Cross-surface: 'key' appears as both a prop AND a slot. Compat
    // strips it ONLY from the slots side.
    surface.props.push(item("key"));
    surface.props.push(item("title"));
    surface.slots.push(item("default"));
    for blocked in COMPAT_BLOCKED_SLOT_NAMES {
        surface.slots.push(item(blocked));
    }
    let r = names_for_policy(PublishedSurfacePolicy::Compat, &surface);

    assert!(
        r.props.contains(&"key".to_string()),
        "Compat must NOT filter 'key' from the PROPS surface — the blocklist applies only to slots"
    );
    assert_eq!(
        r.slots,
        vec!["default".to_string()],
        "Compat must strip every COMPAT_BLOCKED_SLOT_NAMES entry from the slots surface; \
         got {:?}",
        r.slots
    );
}

#[test]
fn refined_policy_strips_on_event_shadow_props_when_not_declared_in_macro_type_arg() {
    // Bench-refiner contract: when emit `submit` is declared, the
    // `onSubmit` prop shadows it and is stripped from the Refined
    // projection UNLESS the SFC author explicitly re-declared it
    // in the macro type arg.
    //
    // This test asserts the structural rule with TWO cases:
    //   case A — `onSubmit` NOT declared: stripped.
    //   case B — `onSubmit` IS declared: retained.
    // If the policy reverts to a pure prefix heuristic (drop every
    // `onX` prop), case B fails. If the policy stops stripping
    // shadow props, case A fails.
    let surface_a = AnalyzedSurface {
        props: vec![item("title"), item("onSubmit")],
        events: vec![item("submit")],
        slots: vec![],
        exposed: vec![],
    };
    let r_a = names_for_policy(PublishedSurfacePolicy::Refined, &surface_a);
    assert!(
        !r_a.props.contains(&"onSubmit".to_string()),
        "Refined must strip 'onSubmit' when 'submit' is declared as an emit \
         AND 'onSubmit' was not declared in the macro type arg; got {:?}",
        r_a.props
    );
    assert!(
        r_a.props.contains(&"title".to_string()),
        "Refined must NOT strip non-shadow props"
    );

    let surface_b = AnalyzedSurface {
        props: vec![item("title"), item_declared("onSubmit")],
        events: vec![item("submit")],
        slots: vec![],
        exposed: vec![],
    };
    let r_b = names_for_policy(PublishedSurfacePolicy::Refined, &surface_b);
    assert!(
        r_b.props.contains(&"onSubmit".to_string()),
        "Refined must RETAIN an explicitly-declared 'onSubmit' prop even when \
         'submit' is declared as an emit — the declaration overrides the shadow \
         heuristic. Got {:?}",
        r_b.props
    );
}

#[test]
fn refined_policy_strips_vue_intrinsics_only_when_not_declared() {
    // Refined strips `class`/`style`/`key`/`ref` from props only
    // when the SFC author did NOT explicitly re-declare them in
    // the macro type arg. If the author declared `class?: any`,
    // it survives. This protects against the R19b regression
    // where the producer-side filter stripped declared intrinsics.
    let surface = AnalyzedSurface {
        props: vec![
            item("class"),          // not declared → drop
            item_declared("style"), // declared → retain
            item("title"),          // ordinary prop → retain
        ],
        events: vec![],
        slots: vec![],
        exposed: vec![],
    };
    let r = names_for_policy(PublishedSurfacePolicy::Refined, &surface);

    assert!(
        !r.props.contains(&"class".to_string()),
        "Refined must strip undeclared Vue intrinsic 'class' from props"
    );
    assert!(
        r.props.contains(&"style".to_string()),
        "Refined must RETAIN an explicitly-declared Vue intrinsic 'style'. \
         Stripping a declared intrinsic is the R19b regression — the producer \
         said the author wanted it on the surface."
    );
    assert!(
        r.props.contains(&"title".to_string()),
        "Refined must not strip ordinary props"
    );
    // Sanity: all four intrinsics are exactly the ones the policy considers.
    for n in VUE_INTRINSIC_ATTR_NAMES {
        assert!(
            ["class", "style", "key", "ref"].contains(n),
            "VUE_INTRINSIC_ATTR_NAMES set drift — got {n}"
        );
    }
}

#[test]
fn refined_policy_strips_producer_flagged_global_props() {
    // Producer-flagged global props (e.g.
    // `HTMLAttributes`-derived) are filtered by Refined regardless
    // of name. This mirrors the bench refiner's prior
    // `!prop.global` check. Test FAILS if the filter is dropped.
    let surface = AnalyzedSurface {
        props: vec![
            item("title"),            // ordinary, retain
            item_global("autofocus"), // global, drop
        ],
        events: vec![],
        slots: vec![],
        exposed: vec![],
    };
    let r = names_for_policy(PublishedSurfacePolicy::Refined, &surface);
    assert!(
        r.props.contains(&"title".to_string()),
        "Refined must retain non-global props"
    );
    assert!(
        !r.props.contains(&"autofocus".to_string()),
        "Refined must strip producer-flagged global props"
    );
}

#[test]
fn refined_policy_strips_compat_blocked_slot_names() {
    // Refined inherits the COMPAT slot blocklist from Compat. If
    // any policy variant stops applying it, this test fails.
    let surface = AnalyzedSurface {
        props: vec![],
        events: vec![],
        slots: vec![item("default"), item("key"), item("ref")],
        exposed: vec![],
    };
    let r = names_for_policy(PublishedSurfacePolicy::Refined, &surface);
    assert!(
        r.slots.contains(&"default".to_string()),
        "Refined slots must retain non-blocklist slot names"
    );
    assert!(
        !r.slots.contains(&"key".to_string()),
        "Refined slots must filter COMPAT_BLOCKED_SLOT_NAMES — 'key'"
    );
    assert!(
        !r.slots.contains(&"ref".to_string()),
        "Refined slots must filter COMPAT_BLOCKED_SLOT_NAMES — 'ref'"
    );
}

#[test]
fn compat_policy_never_blocks_author_declared_slot_names() {
    // The slot blocklist suppresses VNode-transport names that reach the
    // surface WITHOUT the author declaring them. A slot the SFC author
    // explicitly declared on the component's own macro surface (the
    // Popover.vue `anchor?(props: SlotProps<M>): VNode[]` corpus shape —
    // vue-component-meta itself publishes it) must NEVER be blocked:
    // the block is a structural condition on `declared_in_macro_type_arg`,
    // not a bare name-set membership test.
    let surface = AnalyzedSurface {
        props: vec![],
        events: vec![],
        slots: vec![
            item("default"),
            item_declared("anchor"), // author-declared → survives
            item_declared("el"),     // author-declared → survives
            item("anchor2"),         // non-blocked name → survives
            item("placeholder"),     // NOT declared → blocked
        ],
        exposed: vec![],
    };
    let compat = names_for_policy(PublishedSurfacePolicy::Compat, &surface);
    assert_eq!(
        compat.slots,
        vec![
            "default".to_string(),
            "anchor".to_string(),
            "el".to_string(),
            "anchor2".to_string(),
        ],
        "Compat must keep author-declared blocklist-named slots (anchor/el), \
         keep ordinary slots, and still strip an UNDECLARED VNode-transport \
         name (placeholder); got {:?}",
        compat.slots
    );

    let refined = names_for_policy(PublishedSurfacePolicy::Refined, &surface);
    assert_eq!(
        refined.slots, compat.slots,
        "Refined inherits the same declared-slot exemption as Compat"
    );
}

#[test]
fn event_name_to_on_prop_name_matches_bench_refiner_camelcase() {
    // The bench refiner derived `on{Event}` via
    // `camelCase("on_" + event.name)`. Our Rust port must produce
    // identical output for the cases the bench corpus actually
    // exercises. If the camelcase logic diverges, the Refined
    // shadow filter mis-classifies and the bench gate regresses.
    //
    // Examples taken from real Vue conventions:
    //   submit            → onSubmit
    //   state-change      → onStateChange
    //   update:modelValue → onUpdateModelValue
    //   click             → onClick
    //   error             → onError
    let cases: &[(&str, &str)] = &[
        ("submit", "onSubmit"),
        ("click", "onClick"),
        ("error", "onError"),
        ("state-change", "onStateChange"),
        ("update:modelValue", "onUpdateModelValue"),
        ("update:open", "onUpdateOpen"),
        ("foo_bar", "onFooBar"),
        ("FOO", "onFOO"),
    ];
    for (input, expected) in cases {
        let got = event_name_to_on_prop_name(input);
        assert_eq!(
            got, *expected,
            "event_name_to_on_prop_name('{input}') = '{got}' (expected '{expected}')"
        );
    }
}

#[test]
fn policy_native_compat_refined_form_a_strict_monotone_chain_for_authform_shape() {
    // End-to-end characterization mirroring the AuthForm.vue corpus
    // shape (without depending on the corpus): `defineProps<{ title:
    // string; class?: any; onSubmit?: (...) }>()` plus a `submit`
    // emit. The three policies form a monotone chain:
    //   Native(props)  ⊇  Compat(props)  ⊇  Refined(props)
    //
    // For AuthForm.vue specifically, `class` is declared in the
    // macro type arg — Refined MUST keep it. `onSubmit` is declared
    // too — Refined MUST keep it. So Refined == Compat == Native
    // on the props surface for this shape. The test fails if any
    // policy diverges from the author's declaration.
    let surface = AnalyzedSurface {
        props: vec![
            item("title"),
            item_declared("class"),
            item_declared("onSubmit"),
        ],
        events: vec![item("submit")],
        slots: vec![],
        exposed: vec![],
    };
    let native = names_for_policy(PublishedSurfacePolicy::Native, &surface);
    let compat = names_for_policy(PublishedSurfacePolicy::Compat, &surface);
    let refined = names_for_policy(PublishedSurfacePolicy::Refined, &surface);

    for name in &["title", "class", "onSubmit"] {
        assert!(
            native.props.contains(&name.to_string()),
            "Native must contain '{name}'"
        );
        assert!(
            compat.props.contains(&name.to_string()),
            "Compat must contain '{name}'"
        );
        assert!(
            refined.props.contains(&name.to_string()),
            "Refined must contain '{name}' — the SFC author explicitly declared it \
             in the macro type arg. Stripping a declared name is the R19b regression."
        );
    }
}
