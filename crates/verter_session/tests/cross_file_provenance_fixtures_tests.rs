//! Cross-file `declared_in_macro_type_arg` fixture tests.
//!
//! Each test drives a discriminating reference shape from on-disk
//! fixtures at `tests/fixtures/cross-file/`. The fixtures mirror the
//! cross-file inputs the resolver pipeline must classify
//! end-to-end: parser stamps own-body vs heritage, the semantic
//! propagation threads the bit through `AnalyzedPropField` →
//! `ProjectedMember` → `SurfaceMember` → `ExpandedField`, the
//! prepared-surface walker gates the cache keys on `from_root_body`,
//! and the audit-side `PublishedSurfacePolicy::Refined` projection
//! consumes the fact to strip event-shadow / intrinsic prop names
//! that the SFC author did not author in the macro T body.
//!
//! Fixture layout:
//! - `cross-file-simple/{types.ts, component.vue}` — imported
//!   interface, every member is own-body in the macro T argument.
//! - `cross-file-omit-then-reintroduce/{vendor.ts, types.ts,
//!   component.vue}` — Table-shape with `extends Omit<Vendor, …>`
//!   stripping three names and the carrier own-body re-introducing
//!   them with different types. Heritage-only members remain `false`.
//! - `cross-file-negative/{vendor.ts, types.ts, component.vue}` —
//!   contested name reaches the surface only through heritage; the
//!   `Refined` projection MUST drop the name when it shadows a
//!   declared emit, because `declared_in_macro_type_arg` is `false`.

#![cfg(test)]

use std::path::PathBuf;

use verter_audit::published_surface::{
    names_for_policy, AnalyzedSurface, AnalyzedSurfaceItem, PublishedSurfacePolicy,
};
use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::{CompileErrorPolicy, HostConfig};

fn metahost() -> ComponentMetaHost {
    ComponentMetaHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cross-file")
}

fn read_fixture(rel: &str) -> String {
    let path = fixtures_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read fixture {}: {err}", path.display()))
}

/// Build an `AnalyzedSurface` from a host-produced component-meta so
/// the audit-side `PublishedSurfacePolicy::Refined` projection can
/// be exercised against the structural facts the resolver pipeline
/// published.
///
/// The mapping is direct: each prop's `name` and
/// `declared_in_macro_type_arg` propagate into the audit item; slots
/// / exposed / events are reduced to their names (the Refined
/// policy only consults the fact on the `props` axis). Producer
/// `global` flags are NOT plumbed here because the fixtures do not
/// exercise HTMLAttributes-globals; `global: false` is the
/// structural default for the fixture-driven assertions.
fn surface_from_meta(
    meta: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) -> AnalyzedSurface {
    AnalyzedSurface {
        props: meta
            .props
            .iter()
            .map(|p| AnalyzedSurfaceItem {
                name: p.name.clone(),
                declared_in_macro_type_arg: p.declared_in_macro_type_arg,
                global: false,
            })
            .collect(),
        events: meta
            .events
            .iter()
            .map(|e| AnalyzedSurfaceItem {
                name: e.name.clone(),
                declared_in_macro_type_arg: false,
                global: false,
            })
            .collect(),
        slots: meta
            .slots
            .iter()
            .map(|s| AnalyzedSurfaceItem {
                name: s.name.clone(),
                declared_in_macro_type_arg: false,
                global: false,
            })
            .collect(),
        exposed: meta
            .exposed
            .iter()
            .map(|x| AnalyzedSurfaceItem {
                name: x.name.clone(),
                declared_in_macro_type_arg: false,
                global: false,
            })
            .collect(),
    }
}

/// Fixture-driven discriminating test for the cross-file-simple shape.
///
/// `FooProps` is imported and consumed at the macro-T root. Every
/// member of `FooProps`'s own body must surface with
/// `declared_in_macro_type_arg = true`. The `Refined` projection
/// MUST keep `onSubmit` on the published surface because the author
/// explicitly declared it (i.e. the structural `true` survives the
/// shadow-event-prop strip rule). A producer-side regression that
/// stamps cross-file own-body members `false` flips this assertion
/// to RED via Refined dropping `onSubmit` from the published list.
#[test]
fn fixture_cross_file_simple_own_body_members_survive_refined_projection() {
    let mh = metahost();
    mh.upsert_base("/src/types.ts", &read_fixture("cross-file-simple/types.ts"))
        .expect("types.ts upsert");
    mh.upsert_base(
        "/src/Component.vue",
        &read_fixture("cross-file-simple/component.vue"),
    )
    .expect("Component.vue upsert");

    let meta = mh
        .host()
        .get_component_meta("/src/Component.vue")
        .expect("component meta resolves");

    let on_submit = meta
        .props
        .iter()
        .find(|p| p.name == "onSubmit")
        .expect("cross-file-simple: meta.props MUST contain `onSubmit`");
    assert!(
        on_submit.declared_in_macro_type_arg,
        "cross-file-simple: imported own-body `onSubmit` MUST carry \
         declared_in_macro_type_arg=true. Got declared={}. A `false` \
         here would let the Refined policy strip `onSubmit` because \
         it shadows the declared `submit` emit — discriminating \
         downstream assertion follows.",
        on_submit.declared_in_macro_type_arg,
    );

    let surface = surface_from_meta(&meta);
    let refined = names_for_policy(PublishedSurfacePolicy::Refined, &surface);
    assert!(
        refined.props.iter().any(|n| n == "onSubmit"),
        "cross-file-simple: the Refined policy MUST keep `onSubmit` \
         on the published surface — the author explicitly declared it \
         in `FooProps`'s body (declared_in_macro_type_arg=true), so \
         the shadow-event-prop strip rule MUST NOT remove it. Got \
         refined.props={:?}. A regression dropping `onSubmit` would \
         indicate the producer flipped the bit to `false` for \
         imported own-body members.",
        refined.props,
    );
}

/// Fixture-driven discriminating test for the
/// cross-file-omit-then-reintroduce Table-shape.
///
/// `Vendor` declares 4 members; `CarrierProps extends Omit<Vendor,
/// 'state' | 'onStateChange' | 'renderFallbackValue'>` strips three
/// and re-introduces them in the carrier's own body with different
/// types. The 3 re-introduced members must carry
/// `declared_in_macro_type_arg = true`; the heritage-only
/// `inherited_member` must carry `false`. A regression that
/// blanket-stamps every member to `true` (the inverse companion-root
/// restamp) or to `false` (heritage descent leaking into own body)
/// fails this test on the corresponding axis.
#[test]
fn fixture_cross_file_omit_then_reintroduce_preserves_per_member_provenance() {
    let mh = metahost();
    mh.upsert_base(
        "/src/vendor.ts",
        &read_fixture("cross-file-omit-then-reintroduce/vendor.ts"),
    )
    .expect("vendor.ts upsert");
    mh.upsert_base(
        "/src/types.ts",
        &read_fixture("cross-file-omit-then-reintroduce/types.ts"),
    )
    .expect("types.ts upsert");
    mh.upsert_base(
        "/src/Component.vue",
        &read_fixture("cross-file-omit-then-reintroduce/component.vue"),
    )
    .expect("Component.vue upsert");

    let meta = mh
        .host()
        .get_component_meta("/src/Component.vue")
        .expect("component meta resolves");

    for own_body_name in ["state", "onStateChange", "renderFallbackValue"] {
        let p = meta
            .props
            .iter()
            .find(|p| p.name == own_body_name)
            .unwrap_or_else(|| {
                panic!(
                    "cross-file-omit-then-reintroduce: meta.props MUST \
                     contain own-body re-introduced member `{}`",
                    own_body_name,
                )
            });
        assert!(
            p.declared_in_macro_type_arg,
            "cross-file-omit-then-reintroduce: own-body re-introduced \
             member `{}` MUST carry declared_in_macro_type_arg=true. \
             Got declared={}. A `false` here means the c4 \
             prepared-surface walker did NOT preserve \
             `from_root_body=true` for the intersection's own-body \
             literal arm.",
            own_body_name, p.declared_in_macro_type_arg,
        );
    }

    let inherited = meta
        .props
        .iter()
        .find(|p| p.name == "inherited_member")
        .expect("cross-file-omit-then-reintroduce: meta.props contains `inherited_member`");
    assert!(
        !inherited.declared_in_macro_type_arg,
        "cross-file-omit-then-reintroduce: heritage-only member \
         `inherited_member` MUST carry declared_in_macro_type_arg=false. \
         Got declared={}. A `true` here means the walker leaked \
         `from_root_body=true` into the `Omit` first argument's \
         heritage descent, OR the parser's heritage-descent stamping \
         is broken.",
        inherited.declared_in_macro_type_arg,
    );
}

/// Fixture-driven discriminating test for the cross-file-negative
/// shape: contested name reaches the carrier surface ONLY through
/// heritage. The structural fact must be `false`, AND the `Refined`
/// projection MUST drop the contested name from the published prop
/// list (the shadow-event-prop strip rule kicks in when the bit is
/// `false`).
///
/// Discrimination contract: the contested name `onSubmit` shadows the
/// declared emit `submit`. The Refined policy strips
/// `onSubmit` because `declared_in_macro_type_arg=false`. If a
/// regression flips the bit to `true` (heritage descent leaking own
/// body), `onSubmit` survives the strip rule, the assertion below
/// observes its presence in `refined.props`, and the test FAILS RED.
#[test]
fn fixture_cross_file_negative_heritage_only_contested_name_is_stripped_by_refined() {
    let mh = metahost();
    mh.upsert_base(
        "/src/vendor.ts",
        &read_fixture("cross-file-negative/vendor.ts"),
    )
    .expect("vendor.ts upsert");
    mh.upsert_base(
        "/src/types.ts",
        &read_fixture("cross-file-negative/types.ts"),
    )
    .expect("types.ts upsert");
    mh.upsert_base(
        "/src/Component.vue",
        &read_fixture("cross-file-negative/component.vue"),
    )
    .expect("Component.vue upsert");

    let meta = mh
        .host()
        .get_component_meta("/src/Component.vue")
        .expect("component meta resolves");

    // Structural fact assertion: contested name reaches via heritage
    // only, so the bit must be false.
    let contested = meta
        .props
        .iter()
        .find(|p| p.name == "onSubmit")
        .expect("cross-file-negative: meta.props contains heritage-reached `onSubmit`");
    assert!(
        !contested.declared_in_macro_type_arg,
        "cross-file-negative: `onSubmit` reaches the surface ONLY \
         through `extends PlainProps`. It MUST carry \
         declared_in_macro_type_arg=false. Got declared={}. A `true` \
         here means heritage descent leaked the body flag.",
        contested.declared_in_macro_type_arg,
    );

    // STRIPPED assertion (codex#2 CA5 fix): the Refined policy MUST
    // remove `onSubmit` from the published surface because it
    // shadows the declared `submit` emit AND the structural fact
    // is `false`.
    let surface = surface_from_meta(&meta);
    let refined = names_for_policy(PublishedSurfacePolicy::Refined, &surface);
    assert!(
        !refined.props.iter().any(|n| n == "onSubmit"),
        "cross-file-negative: the Refined policy MUST strip \
         `onSubmit` from the published surface — it shadows the \
         declared `submit` emit AND \
         declared_in_macro_type_arg=false (heritage descent). Got \
         refined.props={:?}. A surviving `onSubmit` means either \
         (a) the structural fact flipped to `true` and the strip \
         rule no longer fires, or (b) the Refined policy's \
         shadow-event-prop derivation regressed.",
        refined.props,
    );

    // The own-body member of the carrier must survive Refined.
    let extra = meta
        .props
        .iter()
        .find(|p| p.name == "extra")
        .expect("cross-file-negative: meta.props contains own-body `extra`");
    assert!(
        extra.declared_in_macro_type_arg,
        "cross-file-negative: own-body member `extra` MUST carry \
         declared_in_macro_type_arg=true. Got declared={}.",
        extra.declared_in_macro_type_arg,
    );
    assert!(
        refined.props.iter().any(|n| n == "extra"),
        "cross-file-negative: own-body `extra` MUST survive Refined; got refined.props={:?}",
        refined.props,
    );
}
