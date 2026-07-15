//! Tests for the CLOSED Svelte `bind:` contract table (`bind_contract.rs`), extracted to a
//! sibling to keep the contract module under the file-size guard.

#![cfg(test)]

use crate::svelte::bind_contract::*;
use crate::svelte::bind_contract_data::SVELTE_BIND_CONTRACTS;

/// The whole-table destructure test (NO `..`). It binds every field of every
/// row, so ADDING a binding to the registry forces a conscious decision here
/// (the match arm for the new name must be added or this test fails to
/// compile / fails its assertion). It also pins each row's direction +
/// special routing + runtime helper/arity so a registry edit that silently
/// flips a column is caught.
#[test]
fn every_bind_contract_row_is_consciously_accounted_for() {
    for contract in SVELTE_BIND_CONTRACTS {
        // Destructure WITHOUT `..` — a new field forces this to be updated.
        let BindContract {
            name,
            direction,
            value_type,
            tags,
            host_scope,
            special,
            target_policy,
            official_helper,
            support,
            arity,
            prop_event,
            prelude,
            should_proxy,
        } = contract;

        assert!(!name.is_empty(), "binding name is non-empty");
        assert!(!value_type.is_empty(), "value type is non-empty: {name}");
        assert!(!tags.is_empty(), "tags is non-empty: {name}");

        // The closed expected HOST-SCOPE for every documented binding name — a `..`-free
        // exhaustive match (a new name hits the wildcard panic). The window/document
        // host-only binds carry their special scope; `focused` / `this` are Universal;
        // every other bind is the element-default scope.
        let expected_host_scope = match *name {
            "this" | "focused" => BindHostScope::Universal,
            "innerWidth" | "innerHeight" | "outerWidth" | "outerHeight" | "scrollX" | "scrollY"
            | "online" | "devicePixelRatio" => BindHostScope::Window,
            "activeElement" | "fullscreenElement" | "pointerLockElement" | "visibilityState" => {
                BindHostScope::Document
            }
            "group"
            | "files"
            | "indeterminate"
            | "open"
            | "innerHTML"
            | "innerText"
            | "textContent"
            | "currentTime"
            | "playbackRate"
            | "volume"
            | "muted"
            | "paused"
            | "duration"
            | "buffered"
            | "seekable"
            | "played"
            | "seeking"
            | "ended"
            | "readyState"
            | "clientWidth"
            | "clientHeight"
            | "offsetWidth"
            | "offsetHeight"
            | "naturalWidth"
            | "naturalHeight"
            | "videoWidth"
            | "videoHeight"
            | "contentRect"
            | "contentBoxSize"
            | "borderBoxSize"
            | "devicePixelContentBoxSize" => BindHostScope::Element,
            other => panic!(
                "unaccounted bind-contract row `{other}` — add it to the host_scope match \
                 with its conscious decision"
            ),
        };
        assert_eq!(*host_scope, expected_host_scope, "host_scope for `{name}`");

        // The closed expected (direction, special) for EVERY documented
        // binding name — a `..`-free exhaustive match forces a conscious
        // decision on any added name (a new name hits the wildcard panic).
        let (expected_dir, expected_special) = match *name {
            "this" => (BindDirection::Read, BindSpecial::This),
            "group" => (BindDirection::ReadWrite, BindSpecial::Group),
            "files" => (BindDirection::ReadWrite, BindSpecial::None),
            "focused" => (BindDirection::Read, BindSpecial::None),
            "innerWidth" | "innerHeight" | "outerWidth" | "outerHeight" | "online"
            | "devicePixelRatio" => (BindDirection::Read, BindSpecial::None),
            "scrollX" | "scrollY" => (BindDirection::ReadWrite, BindSpecial::None),
            "activeElement" | "fullscreenElement" | "pointerLockElement" | "visibilityState" => {
                (BindDirection::Read, BindSpecial::None)
            }
            "indeterminate" => (BindDirection::ReadWrite, BindSpecial::None),
            "open" => (BindDirection::ReadWrite, BindSpecial::None),
            "innerHTML" | "innerText" | "textContent" => {
                (BindDirection::ReadWrite, BindSpecial::None)
            }
            "currentTime" | "playbackRate" | "volume" | "muted" | "paused" => {
                (BindDirection::ReadWrite, BindSpecial::None)
            }
            "duration" | "buffered" | "seekable" | "played" | "seeking" | "ended"
            | "readyState" => (BindDirection::Read, BindSpecial::None),
            "clientWidth" | "clientHeight" | "offsetWidth" | "offsetHeight" => {
                (BindDirection::Read, BindSpecial::None)
            }
            "naturalWidth" | "naturalHeight" | "videoWidth" | "videoHeight" => {
                (BindDirection::Read, BindSpecial::None)
            }
            "contentRect" | "contentBoxSize" | "borderBoxSize" | "devicePixelContentBoxSize" => {
                (BindDirection::Read, BindSpecial::None)
            }
            other => panic!(
                "unaccounted bind-contract row `{other}` — add it to the \
                 whole-table destructure test with its conscious \
                 direction/special decision"
            ),
        };
        assert_eq!(*direction, expected_dir, "direction for `{name}`");
        assert_eq!(*special, expected_special, "special for `{name}`");

        // The closed expected (official_helper, support) for EVERY documented binding
        // name — a `..`-free exhaustive match (a new name hits the wildcard panic). The
        // official helper is the REAL pinned `svelte@5.56.3` identity (oracle-verified),
        // PRESERVED even for runtime-unsupported rows; support decides runtime emission.
        use OfficialRuntimeHelper as O;
        use RuntimeSupport as S;
        let (expected_official, expected_support) = match *name {
            "this" => (O::This, S::Supported),
            "group" => (O::Group, S::Supported),
            "files" => (O::Files, S::Unsupported),
            "focused" => (O::Focused, S::Supported),
            "innerWidth" | "innerHeight" | "outerWidth" | "outerHeight" => {
                (O::WindowSize, S::Supported)
            }
            "scrollX" | "scrollY" => (O::WindowScroll, S::Supported),
            "online" => (O::Online, S::Supported),
            "devicePixelRatio" => (O::Property, S::Supported),
            "activeElement" => (O::ActiveElement, S::Supported),
            "fullscreenElement" | "pointerLockElement" | "visibilityState" => {
                (O::Property, S::Supported)
            }
            "indeterminate" => (O::Property, S::Unsupported),
            "open" => (O::Property, S::Supported),
            "innerHTML" | "innerText" | "textContent" => (O::ContentEditable, S::Supported),
            "currentTime" => (O::CurrentTime, S::Supported),
            "paused" => (O::Paused, S::Supported),
            "playbackRate" => (O::PlaybackRate, S::Unsupported),
            "volume" => (O::Volume, S::Unsupported),
            "muted" => (O::Muted, S::Unsupported),
            "duration" => (O::Property, S::Supported),
            "played" => (O::Played, S::Supported),
            "buffered" => (O::Buffered, S::Unsupported),
            "seekable" => (O::Seekable, S::Unsupported),
            "seeking" => (O::Seeking, S::Unsupported),
            "ended" => (O::Ended, S::Unsupported),
            "readyState" => (O::ReadyState, S::Unsupported),
            "clientWidth" | "clientHeight" | "offsetWidth" | "offsetHeight" => {
                (O::ElementSize, S::Supported)
            }
            "naturalWidth" | "naturalHeight" | "videoWidth" | "videoHeight" => {
                (O::Property, S::Unsupported)
            }
            "contentRect" | "contentBoxSize" | "borderBoxSize" | "devicePixelContentBoxSize" => {
                (O::ResizeObserver, S::Unsupported)
            }
            other => panic!(
                "unaccounted bind-contract row `{other}` — add it to the official_helper \
                 + support match with its conscious decision"
            ),
        };
        assert_eq!(
            *official_helper, expected_official,
            "official_helper for `{name}`"
        );
        assert_eq!(*support, expected_support, "support for `{name}`");
        // A SUPPORTED, non-`this` row's official helper MUST be emittable (the
        // `resolve_runtime_bind` `.expect` invariant); an UNSUPPORTED row may carry an
        // emittable OR non-emittable official helper — support is the refusal axis.
        if *support == RuntimeSupport::Supported && *special != BindSpecial::This {
            assert!(
                official_helper.emittable_runtime_helper().is_some(),
                "supported row `{name}` must map to an emittable runtime helper"
            );
        }

        // A read-direction DATA-VALUE row is NEVER a get/set pair (it can only
        // RECEIVE the value). `bind:this` is the sole exception: it is a HOST
        // binding (not a data value), and its `$.bind_this(host, set, get)` shape
        // carries BOTH a setter and a getter despite the read-direction IDE
        // semantics — so it is exempt from the data-value arity invariant.
        if *direction == BindDirection::Read && *special != BindSpecial::This {
            assert_ne!(
                *arity,
                HelperArity::GetSet,
                "a read-direction data-value bind `{name}` must not be a get/set pair"
            );
        }
        // The `should_proxy` flag is the OFFICIAL host-independent baseline: TRUE only
        // for the window/document-ONLY rows (whose setter always proxies — they never
        // resolve on a regular element), FALSE for every Element / Universal-scope row
        // (the element baseline — the `$.set(…, true)` flag they DO carry on a special
        // host is HOST-driven at projection, not a row property). So a `should_proxy=true`
        // row MUST be a Window / Document host-scope row, and every Element / Universal
        // row stays `should_proxy=false`.
        if *should_proxy {
            assert!(
                matches!(host_scope, BindHostScope::Window | BindHostScope::Document),
                "only a window/document host-scope row may carry should_proxy=true \
                 (the proxy flag is the window/document-host setter baseline): {name}"
            );
        } else {
            assert!(
                matches!(
                    host_scope,
                    BindHostScope::Element | BindHostScope::Universal
                ),
                "every Element / Universal host-scope row is should_proxy=false \
                 (the element baseline; the special-host proxy flag is host-driven): {name}"
            );
        }
        // A `bind_property` event name is present IFF the OFFICIAL helper is the
        // property form; every other helper (dedicated or get/set) has an empty event.
        if *official_helper == OfficialRuntimeHelper::Property {
            assert!(
                !prop_event.is_empty(),
                "a $.bind_property row must carry an event name: {name}"
            );
        } else {
            assert!(
                prop_event.is_empty(),
                "a non-property official helper must have no event name: {name}"
            );
        }
        // A textarea/input cleanup prelude only attaches to its host families.
        let _ = prelude;

        // The target-shape policy: `bind:group` is the SOLE identifier/member-only
        // bind (a SequenceExpression target is `bind_group_invalid_expression`); every
        // other row accepts the function-pair form (LvalueOrFunctionPair). A `..`-free
        // match forces a conscious decision on any added name.
        let expected_policy = match *name {
            "group" => BindTargetPolicy::IdentifierOrMemberOnly {
                official_code: "bind_group_invalid_expression",
            },
            _ => BindTargetPolicy::LvalueOrFunctionPair,
        };
        assert_eq!(
            *target_policy, expected_policy,
            "target_policy for `{name}`"
        );
    }
}

#[test]
fn lookup_respects_tag_constraints() {
    // `bind:open` is `<details>`-scoped.
    assert!(lookup_bind_contract("open", "details").is_some());
    assert!(lookup_bind_contract("open", "div").is_none());
    // Media bindings are `<audio>`/`<video>`-scoped.
    assert!(lookup_bind_contract("currentTime", "video").is_some());
    assert!(lookup_bind_contract("currentTime", "div").is_none());
    // Dimension bindings + `bind:this` apply to any element.
    assert!(lookup_bind_contract("clientWidth", "div").is_some());
    assert!(lookup_bind_contract("this", "span").is_some());
    // `value`/`checked` are NOT wide-family contracts (they go through the
    // plain JSX intrinsic attribute path).
    assert!(lookup_bind_contract("value", "input").is_none());
    assert!(lookup_bind_contract("checked", "input").is_none());
}

#[test]
fn readonly_bindings_carry_the_read_direction() {
    // The readonly DOM properties are read-direction (a userland write to the
    // binding target is rejected by the projected `r`-mode check).
    for name in ["duration", "clientWidth", "naturalWidth"] {
        let c = SVELTE_BIND_CONTRACTS
            .iter()
            .find(|c| c.name == name)
            .unwrap();
        assert_eq!(c.direction, BindDirection::Read, "{name} is read-direction");
    }
}

#[test]
fn runtime_helper_metadata_matches_the_pinned_oracle_shapes() {
    // The runtime columns the 5c emitter consumes, pinned to the empirical
    // svelte@5.56.3 shapes (oracle-probe-out.txt). A registry edit that flips a
    // helper / arity / event / prelude is caught here.
    let row = |name: &str| {
        SVELTE_BIND_CONTRACTS
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("row {name}"))
    };
    // The OFFICIAL helper identity per row (svelte@5.56.3 oracle), now a fact column
    // distinct from runtime support.
    assert_eq!(row("open").official_helper, OfficialRuntimeHelper::Property);
    assert_eq!(row("open").prop_event, "toggle");
    assert_eq!(row("open").arity, HelperArity::Property);
    assert_eq!(row("open").direction, BindDirection::ReadWrite);
    assert_eq!(
        row("currentTime").official_helper,
        OfficialRuntimeHelper::CurrentTime
    );
    assert_eq!(row("currentTime").arity, HelperArity::GetSet);
    assert_eq!(row("paused").official_helper, OfficialRuntimeHelper::Paused);
    assert_eq!(row("paused").arity, HelperArity::GetSet);
    // duration — generic property, read-only (setter-only), durationchange event.
    assert_eq!(
        row("duration").official_helper,
        OfficialRuntimeHelper::Property
    );
    assert_eq!(row("duration").prop_event, "durationchange");
    assert_eq!(row("duration").arity, HelperArity::Property);
    assert_eq!(row("duration").direction, BindDirection::Read);
    // played — dedicated setter-only.
    assert_eq!(row("played").official_helper, OfficialRuntimeHelper::Played);
    assert_eq!(row("played").arity, HelperArity::SetOnly);
    // dimensions — element_size, setter-only.
    for d in ["clientWidth", "clientHeight", "offsetWidth", "offsetHeight"] {
        assert_eq!(
            row(d).official_helper,
            OfficialRuntimeHelper::ElementSize,
            "{d}"
        );
        assert_eq!(row(d).arity, HelperArity::SetOnly, "{d}");
    }
    // contenteditable — content_editable, get/set.
    for c in ["innerHTML", "innerText", "textContent"] {
        assert_eq!(
            row(c).official_helper,
            OfficialRuntimeHelper::ContentEditable,
            "{c}"
        );
        assert_eq!(row(c).arity, HelperArity::GetSet, "{c}");
    }
    // group — the dedicated group helper.
    assert_eq!(row("group").official_helper, OfficialRuntimeHelper::Group);
}

#[test]
fn resolve_runtime_bind_routes_the_builtin_and_wide_families_per_the_oracle() {
    // The builtin form-control binds (NOT in the IDE contract) route to their
    // dedicated helpers with the host-specific prelude, matching the pinned
    // svelte@5.56.3 shapes (oracle-probe-out.txt).
    let input_value = resolve_runtime_bind("value", "input").unwrap();
    assert_eq!(input_value.helper, RuntimeHelper::Value);
    assert_eq!(input_value.prelude, BindPrelude::RemoveInputDefaults);
    assert_eq!(input_value.arity, HelperArity::GetSet);

    let textarea_value = resolve_runtime_bind("value", "textarea").unwrap();
    assert_eq!(textarea_value.helper, RuntimeHelper::Value);
    assert_eq!(textarea_value.prelude, BindPrelude::RemoveTextareaChild);

    let select_value = resolve_runtime_bind("value", "select").unwrap();
    assert_eq!(select_value.helper, RuntimeHelper::SelectValue);
    assert_eq!(select_value.prelude, BindPrelude::None);

    let checked = resolve_runtime_bind("checked", "input").unwrap();
    assert_eq!(checked.helper, RuntimeHelper::Checked);
    assert_eq!(checked.prelude, BindPrelude::RemoveInputDefaults);

    // The wide family delegates to the contract row.
    let group = resolve_runtime_bind("group", "input").unwrap();
    assert_eq!(group.helper, RuntimeHelper::Group);
    assert_eq!(group.prelude, BindPrelude::RemoveInputDefaults);

    let open = resolve_runtime_bind("open", "details").unwrap();
    assert_eq!(open.helper, RuntimeHelper::Property);
    assert_eq!(open.prop_event, "toggle");
    assert_eq!(open.direction, BindDirection::ReadWrite);

    let duration = resolve_runtime_bind("duration", "video").unwrap();
    assert_eq!(duration.helper, RuntimeHelper::Property);
    assert_eq!(duration.prop_event, "durationchange");
    assert_eq!(duration.direction, BindDirection::Read);
    assert_eq!(duration.arity, HelperArity::Property);

    let played = resolve_runtime_bind("played", "video").unwrap();
    assert_eq!(played.helper, RuntimeHelper::Played);
    assert_eq!(played.arity, HelperArity::SetOnly);

    let cw = resolve_runtime_bind("clientWidth", "div").unwrap();
    assert_eq!(cw.helper, RuntimeHelper::ElementSize);
    assert_eq!(cw.arity, HelperArity::SetOnly);

    let inner = resolve_runtime_bind("innerHTML", "div").unwrap();
    assert_eq!(inner.helper, RuntimeHelper::ContentEditable);
    assert_eq!(inner.arity, HelperArity::GetSet);

    // NEGATIVE: every DOM bind routing is should_proxy=false (no `$.set(…, true)`).
    for (n, t) in [
        ("value", "input"),
        ("value", "textarea"),
        ("value", "select"),
        ("checked", "input"),
        ("group", "input"),
        ("open", "details"),
        ("currentTime", "video"),
        ("paused", "video"),
        ("duration", "video"),
        ("played", "video"),
        ("clientWidth", "div"),
        ("innerHTML", "div"),
    ] {
        assert!(
            !resolve_runtime_bind(n, t).unwrap().should_proxy,
            "DOM bind {n} on {t} must be should_proxy=false"
        );
    }

    // NEGATIVE: `this` is host-routed, NOT this DOM-value router.
    assert!(resolve_runtime_bind("this", "div").is_none());
    // NEGATIVE: an unsupported (name, tag) pair returns None.
    assert!(resolve_runtime_bind("open", "div").is_none());
    assert!(resolve_runtime_bind("checked", "div").is_none());
    assert!(resolve_runtime_bind("value", "div").is_none());
}

/// The UNSUPPORTED, DEDICATED-helper rows ("wrong-helper" = the generic
/// `$.bind_property` form would emit the WRONG helper for them): each official
/// `svelte@5.56.3` helper is a DEDICATED helper, so a generic property emission would
/// be runtime-broken. These rows are unsupported by the native client runtime today,
/// so the runtime router FAILS THEM CLOSED (`resolve_runtime_bind` returns `None`).
/// The IDE contract row STILL exists (the IDE type-checks the bind) and records the
/// REAL official helper (`support == Unsupported`, identity PRESERVED — never erased).
/// The implementation that supports one of these rows flips its support + adds goldens.
///
/// Official (verified against the pinned compiler): `files` → `$.bind_files`, `focused`
/// → `$.bind_focused`, `playbackRate` → `$.bind_playback_rate`, `volume` →
/// `$.bind_volume`, `muted` → `$.bind_muted`, media-readiness `buffered`/`seekable`/
/// `seeking`/`ended`/`readyState` → `$.bind_buffered`/`_seekable`/`_seeking`/`_ended`/
/// `_ready_state`, and the resize-observer family
/// (`contentRect`/`contentBoxSize`/`borderBoxSize`/`devicePixelContentBoxSize`) →
/// `$.bind_resize_observer(el, '<name>', set)`.
#[test]
fn unsupported_wrong_helper_rows_fail_closed_at_the_runtime_router() {
    // `(name, host, official_helper)` for each dedicated-helper unsupported row — the
    // router must refuse them, AND the row must record the REAL dedicated official
    // helper (a generic `$.bind_property` would be the wrong helper).
    use OfficialRuntimeHelper as O;
    let dedicated_helper_rows: &[(&str, &str, OfficialRuntimeHelper)] = &[
        ("files", "input", O::Files),
        ("playbackRate", "audio", O::PlaybackRate),
        ("playbackRate", "video", O::PlaybackRate),
        ("volume", "audio", O::Volume),
        ("volume", "video", O::Volume),
        ("muted", "audio", O::Muted),
        ("muted", "video", O::Muted),
        ("buffered", "audio", O::Buffered),
        ("buffered", "video", O::Buffered),
        ("seekable", "audio", O::Seekable),
        ("seekable", "video", O::Seekable),
        ("seeking", "audio", O::Seeking),
        ("seeking", "video", O::Seeking),
        ("ended", "audio", O::Ended),
        ("ended", "video", O::Ended),
        ("readyState", "audio", O::ReadyState),
        ("readyState", "video", O::ReadyState),
        ("contentRect", "div", O::ResizeObserver),
        ("contentBoxSize", "div", O::ResizeObserver),
        ("borderBoxSize", "div", O::ResizeObserver),
        ("devicePixelContentBoxSize", "div", O::ResizeObserver),
    ];
    for (name, tag, expected_official) in dedicated_helper_rows {
        // RUNTIME: fails closed (the runtime router refuses an unsupported row; the
        // emitter never sees a routing to mis-emit).
        assert!(
            resolve_runtime_bind(name, tag).is_none(),
            "unsupported dedicated-helper row `{name}` on `{tag}` must fail closed at \
             the runtime router"
        );
        // IDE: the contract row STILL exists and records the REAL official helper +
        // `support == Unsupported` (identity preserved, not erased to a sentinel).
        let row = lookup_bind_contract(name, tag)
            .unwrap_or_else(|| panic!("IDE contract row for `{name}` on `{tag}` must remain"));
        assert_eq!(
            row.support,
            RuntimeSupport::Unsupported,
            "a dedicated-helper row must be runtime-Unsupported: {name}"
        );
        assert_eq!(
            row.official_helper, *expected_official,
            "a dedicated-helper row must record its REAL official helper (not erased): {name}"
        );
    }
}

/// The UNSUPPORTED, GENERIC-`Property`-helper rows ("correct-helper" = the official
/// helper IS the generic `$.bind_property` form, which would be the right helper):
/// each has a real IDE contract row whose official helper is `Property`, yet the bind
/// is not emitted by the native client runtime today, so the runtime router FAILS THEM
/// CLOSED (`resolve_runtime_bind` returns `None`). Refusal rides the SUPPORT status,
/// NOT the helper identity — these carry the real generic-`Property` official helper
/// and would emit if routed, so a helper-identity-based refusal would wrongly emit
/// them. The IDE contract row STILL exists (the projector type-checks the bind via the
/// `value_type` / `direction` columns); the official helper is preserved.
#[test]
fn unsupported_correct_helper_rows_fail_closed_at_the_runtime_router() {
    // `(name, host)` pairs whose official helper IS the generic `$.bind_property` form
    // but which are unsupported by the native runtime. The host is each name's
    // empirically-pinned svelte@5.56.3 `binding_properties.valid_elements` member
    // (verified against the pinned `phases/bindings.js`): indeterminate → input;
    // naturalWidth/naturalHeight → img; videoWidth/videoHeight → video.
    let property_helper_rows: &[(&str, &str)] = &[
        ("indeterminate", "input"),
        ("naturalWidth", "img"),
        ("naturalHeight", "img"),
        ("videoWidth", "video"),
        ("videoHeight", "video"),
    ];
    for (name, tag) in property_helper_rows {
        // RUNTIME: fails closed — refusal rides SUPPORT, never the (emittable) helper
        // identity. These carry `OfficialRuntimeHelper::Property` (which DOES map to an
        // emittable `RuntimeHelper`), so only `support == Unsupported` stops them.
        assert!(
            resolve_runtime_bind(name, tag).is_none(),
            "unsupported generic-property row `{name}` on `{tag}` must fail closed at \
             the runtime router (refusal rides support, not the emittable helper)"
        );
        // IDE: the contract row STILL exists and records the real `Property` official
        // helper + `support == Unsupported` (identity preserved).
        let row = lookup_bind_contract(name, tag)
            .unwrap_or_else(|| panic!("IDE contract row for `{name}` on `{tag}` must remain"));
        assert_eq!(
            row.support,
            RuntimeSupport::Unsupported,
            "a generic-property unsupported row must be runtime-Unsupported: {name}"
        );
        assert_eq!(
            row.official_helper,
            OfficialRuntimeHelper::Property,
            "a generic-property unsupported row records the real Property helper: {name}"
        );
        // It is the orthogonality witness: the official helper IS emittable, yet the
        // row stays unsupported (proves refusal is support-driven, not identity-driven).
        assert!(
            row.official_helper.emittable_runtime_helper().is_some(),
            "the Property official helper is emittable — only support gates it: {name}"
        );
    }
}

/// The contract-ORACLE structural guard: for EVERY row the runtime router actually
/// emits (every `(name, host)` where `resolve_runtime_bind` returns `Some`), the
/// emitted [`RuntimeHelper`] matches the dedicated `svelte@5.56.3` helper for that
/// bind. A future row that routes to a helper not matching the pinned official shape
/// is caught here. Pinned to the empirically-probed official helper per name. (A
/// routing can only ever carry an emittable `RuntimeHelper` by construction — there is
/// no "unsupported" routing helper; unsupported rows fail closed at the router.)
#[test]
fn every_runtime_routable_row_matches_the_pinned_official_helper() {
    use RuntimeHelper::*;
    // The pinned official helper per BIND NAME (verified against
    // svelte@5.56.3). A name absent here that the router emits is a coverage
    // gap the loop flags; a name present here whose row routes to a different
    // helper is a wrong-helper regression.
    let official_helper = |name: &str| -> Option<RuntimeHelper> {
        Some(match name {
            "value" => Value,             // input/textarea ($.bind_value)
            "checked" => Checked,         // $.bind_checked
            "group" => Group,             // $.bind_group
            "currentTime" => CurrentTime, // $.bind_current_time
            "paused" => Paused,           // $.bind_paused
            "played" => Played,           // $.bind_played
            "focused" => Focused,         // $.bind_focused (universal host)
            // $.bind_element_size for the dimension family.
            "clientWidth" | "clientHeight" | "offsetWidth" | "offsetHeight" => ElementSize,
            // $.bind_content_editable for contenteditable binds.
            "innerHTML" | "innerText" | "textContent" => ContentEditable,
            // The special-host (5f-b) bind helpers — visited when the host walk includes
            // the special-host tokens below.
            "innerWidth" | "innerHeight" | "outerWidth" | "outerHeight" => WindowSize,
            "scrollX" | "scrollY" => WindowScroll,
            "online" => Online,
            "activeElement" => ActiveElement,
            "devicePixelRatio" | "fullscreenElement" | "pointerLockElement" | "visibilityState" => {
                Property
            }
            // $.bind_property for the supported generic-property binds (official also
            // uses bind_property for these): `bind:open` (details) + readonly media
            // `bind:duration`.
            //
            // The runtime-unsupported rows never appear here as routable — the
            // dedicated-helper rows (`files`/`focused`/`volume`/`muted`/`playbackRate`/
            // the media-readiness `buffered`/`seekable`/`seeking`/`ended`/`readyState`/
            // the resize-observer family) AND the generic-property `indeterminate` /
            // `naturalWidth`/`naturalHeight`/`videoWidth`/`videoHeight` all carry
            // `RuntimeSupport::Unsupported`, so `resolve_runtime_bind` fails them closed
            // (the router-row walk below never visits a `None` routing). Their
            // fail-closed coverage is `unsupported_wrong_helper_rows_fail_closed_at_the_runtime_router`
            // + `unsupported_correct_helper_rows_fail_closed_at_the_runtime_router`.
            "open" | "duration" => Property,
            _ => return None,
        })
    };
    // `select` value routes to the dedicated select helper.
    assert_eq!(
        resolve_runtime_bind("value", "select").unwrap().helper,
        SelectValue
    );

    // Walk every contract row × a representative admitted host; assert that any
    // row the router emits matches the pinned official helper.
    let hosts = [
        "input",
        "textarea",
        "select",
        "div",
        "audio",
        "video",
        "details",
        "img",
        // The special-host tokens — so the new window/document host-scoped rows are
        // visited + matched against their pinned official helper too.
        "svelte:window",
        "svelte:document",
        "svelte:body",
        "svelte:element",
    ];
    for row in SVELTE_BIND_CONTRACTS {
        for host in hosts {
            let Some(routing) = resolve_runtime_bind(row.name, host) else {
                continue;
            };
            // `select` value is the dedicated select helper (asserted above);
            // skip it from the per-name table (its name `value` maps to the
            // input/textarea `Value` there).
            if row.name == "value" && host == "select" {
                assert_eq!(routing.helper, SelectValue);
                continue;
            }
            let expected = official_helper(row.name).unwrap_or_else(|| {
                panic!(
                    "runtime router emits `{}` on `{host}` (helper {:?}) but it has no \
                     pinned official helper — either it is a wrong-helper row that must \
                     fail closed, or the oracle table needs the new name",
                    row.name, routing.helper
                )
            });
            assert_eq!(
                routing.helper, expected,
                "row `{}` on `{host}` routes to {:?} but official emits {expected:?}",
                row.name, routing.helper
            );
        }
    }

    // The BUILTIN form-control routable pairs are DELIBERATELY ABSENT from
    // `SVELTE_BIND_CONTRACTS` (they ride the plain JSX intrinsic-attribute path
    // for the IDE), so the contract-row walk above NEVER visits them — leaving
    // the runtime-routable surface incomplete. Cross-check each builtin pair
    // (`(name, host)`) against its pinned official helper + arity + prelude so a
    // future wrong-helper / wrong-prelude regression in the builtin arm of
    // `resolve_runtime_bind` is caught here too. Verified against svelte@5.56.3
    // (oracle CASES `hello_input` / `textarea_value` / select / `checked`).
    let builtin_routable: &[(&str, &str, RuntimeHelper, HelperArity, BindPrelude)] = &[
        // `<input bind:value>` — $.bind_value, get/set, clears form defaults.
        (
            "value",
            "input",
            Value,
            HelperArity::GetSet,
            BindPrelude::RemoveInputDefaults,
        ),
        // `<textarea bind:value>` — $.bind_value, get/set, strips child content.
        (
            "value",
            "textarea",
            Value,
            HelperArity::GetSet,
            BindPrelude::RemoveTextareaChild,
        ),
        // `<select bind:value>` — $.bind_select_value, get/set, no prelude.
        (
            "value",
            "select",
            SelectValue,
            HelperArity::GetSet,
            BindPrelude::None,
        ),
        // `<input bind:checked>` — $.bind_checked, get/set, clears form defaults.
        (
            "checked",
            "input",
            Checked,
            HelperArity::GetSet,
            BindPrelude::RemoveInputDefaults,
        ),
    ];
    // These pairs are NOT in the contract table — assert that so the cross-check
    // genuinely covers the surface the contract-row walk MISSES (a regression
    // that moved a builtin into the table would make this list redundant).
    for (name, host, expected_helper, expected_arity, expected_prelude) in builtin_routable {
        assert!(
            lookup_bind_contract(name, host).is_none(),
            "builtin form-control pair `{name}` on `{host}` must stay ABSENT from the \
             contract table (it rides the plain JSX intrinsic path); the contract-row \
             walk does not cover it, which is why this explicit cross-check exists"
        );
        let routing = resolve_runtime_bind(name, host).unwrap_or_else(|| {
            panic!("builtin routable pair `{name}` on `{host}` must resolve to a routing")
        });
        assert_eq!(
            routing.helper, *expected_helper,
            "builtin `{name}` on `{host}` routes to {:?} but official emits {expected_helper:?}",
            routing.helper
        );
        assert_eq!(
            routing.arity, *expected_arity,
            "builtin `{name}` on `{host}` arity {:?} but official is {expected_arity:?}",
            routing.arity
        );
        assert_eq!(
            routing.prelude, *expected_prelude,
            "builtin `{name}` on `{host}` prelude {:?} but official is {expected_prelude:?}",
            routing.prelude
        );
    }
}

/// `bind:group` is the SOLE identifier/member-only bind: its target-shape policy
/// carries the exact official code for a function-pair (SequenceExpression) target,
/// and EVERY other bind (the wide-family rows AND the builtin form-control binds that
/// default through `bind_target_policy`) accepts the function-pair form. Data-driven —
/// no `name == "group"` hard-code. Verified against svelte@5.56.3 (BindDirective.js:133
/// throws `bind_group_invalid_expression` for any group SequenceExpression target).
#[test]
fn bind_target_policy_is_data_driven_and_group_only() {
    // group on its `<input>` host is identifier/member-only with the exact code.
    assert_eq!(
        bind_target_policy("group", "input"),
        BindTargetPolicy::IdentifierOrMemberOnly {
            official_code: "bind_group_invalid_expression",
        },
        "bind:group must be identifier/member-only"
    );
    // The builtin form-control binds (absent from the contract table) default to
    // accepting the function-pair form — official accepts `bind:value={get,set}` /
    // `bind:checked={get,set}`.
    for (name, tag) in [
        ("value", "input"),
        ("checked", "input"),
        ("value", "select"),
    ] {
        assert_eq!(
            bind_target_policy(name, tag),
            BindTargetPolicy::LvalueOrFunctionPair,
            "{name} on {tag} must accept the function-pair form (default policy)"
        );
    }
    // Representative wide-family binds also accept the function-pair form.
    for (name, tag) in [
        ("currentTime", "video"),
        ("open", "details"),
        ("innerHTML", "div"),
    ] {
        assert_eq!(
            bind_target_policy(name, tag),
            BindTargetPolicy::LvalueOrFunctionPair,
            "{name} on {tag} must accept the function-pair form (default policy)"
        );
    }
    // group is the ONLY row in the whole table carrying the identifier/member-only
    // policy (structural guard against a second hard-coded special-case creeping in).
    let id_member_only: Vec<&str> = SVELTE_BIND_CONTRACTS
        .iter()
        .filter(|c| {
            matches!(
                c.target_policy,
                BindTargetPolicy::IdentifierOrMemberOnly { .. }
            )
        })
        .map(|c| c.name)
        .collect();
    assert_eq!(
        id_member_only,
        vec!["group"],
        "only bind:group may be identifier/member-only"
    );
}

/// `bind:focused` is an EXPLICIT shared-contract row recording the dedicated official
/// helper `$.bind_focused`, with `host_scope: Universal` (svelte's `focused: {}` has no
/// valid/invalid_elements ⇒ valid on EVERY host). 5f-b flips it to RUNTIME-supported: the
/// native client emits `$.bind_focused(host, set)` — the host is the element var on a
/// regular element and `$.window` on the window host. Verified against svelte@5.56.3
/// (`<input bind:focused={x}>` emits `$.bind_focused(input, ($$value) => $.set(x,
/// $$value))`; `<svelte:window bind:focused={f}>` emits `$.bind_focused($.window,
/// ($$value) => $.set(f, $$value, true))`).
#[test]
fn bind_focused_is_an_explicit_universal_supported_contract_row() {
    let row = lookup_bind_contract("focused", "input")
        .expect("bind:focused must be an EXPLICIT contract row, not an absent-row fallthrough");
    assert_eq!(
        row.official_helper,
        OfficialRuntimeHelper::Focused,
        "bind:focused records the dedicated $.bind_focused official helper"
    );
    assert_eq!(
        row.support,
        RuntimeSupport::Supported,
        "5f-b makes bind:focused runtime-supported"
    );
    assert_eq!(
        row.host_scope,
        BindHostScope::Universal,
        "focused is Universal-scope"
    );
    assert_eq!(
        row.direction,
        BindDirection::Read,
        "bind:focused is read-direction"
    );
    // `focused: {}` has no `valid_elements` ⇒ applies to ANY element AND the special hosts.
    assert!(lookup_bind_contract("focused", "div").is_some());
    assert!(lookup_bind_contract("focused", "svelte:window").is_some());
    assert!(lookup_bind_contract("focused", "svelte:body").is_some());
    // RUNTIME: resolves to the dedicated Focused helper, should_proxy=false on the row
    // (the special-host proxy flag is host-driven at projection).
    let routing = resolve_runtime_bind("focused", "input").expect("focused now routes");
    assert_eq!(routing.helper, RuntimeHelper::Focused);
    assert_eq!(routing.arity, HelperArity::SetOnly);
    assert!(
        !routing.should_proxy,
        "the focused row baseline is should_proxy=false"
    );
}

/// The window/document host-only bind rows resolve ONLY on their special host and route
/// to their dedicated helpers, with `should_proxy: true` (the window/document setter
/// baseline). The wrong-host pairs fail closed at the router (§1.8 negative coverage).
#[test]
fn special_host_bind_rows_resolve_host_scoped_with_proxy() {
    // Window-only binds.
    for (name, host, helper) in [
        ("innerWidth", "svelte:window", RuntimeHelper::WindowSize),
        ("scrollX", "svelte:window", RuntimeHelper::WindowScroll),
        ("online", "svelte:window", RuntimeHelper::Online),
        ("devicePixelRatio", "svelte:window", RuntimeHelper::Property),
    ] {
        let r = resolve_runtime_bind(name, host)
            .unwrap_or_else(|| panic!("{name} must route on {host}"));
        assert_eq!(r.helper, helper, "{name} routing helper");
        assert!(r.should_proxy, "{name} must carry should_proxy=true");
    }
    // Document-only binds.
    for (name, helper) in [
        ("activeElement", RuntimeHelper::ActiveElement),
        ("fullscreenElement", RuntimeHelper::Property),
        ("visibilityState", RuntimeHelper::Property),
    ] {
        let r = resolve_runtime_bind(name, "svelte:document")
            .unwrap_or_else(|| panic!("{name} must route on svelte:document"));
        assert_eq!(r.helper, helper, "{name} routing helper");
        assert!(r.should_proxy, "{name} must carry should_proxy=true");
    }
    // NEGATIVE — §1.8 wrong-host pairs fail closed at the router (the bind never emits).
    assert!(resolve_runtime_bind("scrollX", "svelte:body").is_none());
    assert!(resolve_runtime_bind("scrollX", "svelte:document").is_none());
    assert!(resolve_runtime_bind("innerWidth", "svelte:document").is_none());
    assert!(resolve_runtime_bind("visibilityState", "svelte:body").is_none());
    assert!(resolve_runtime_bind("activeElement", "svelte:window").is_none());
    assert!(resolve_runtime_bind("innerWidth", "div").is_none());
    // `clientWidth` (Element scope) is valid on the body/dynamic-element host but NOT on
    // the global hosts (svelte's invalid_elements: [svelte:window, svelte:document]).
    assert!(resolve_runtime_bind("clientWidth", "svelte:body").is_some());
    assert!(resolve_runtime_bind("clientWidth", "svelte:element").is_some());
    assert!(resolve_runtime_bind("clientWidth", "svelte:window").is_none());
    // `value` (input/textarea/select) is NOT valid on a generic body/dynamic element.
    assert!(resolve_runtime_bind("value", "svelte:body").is_none());
    assert!(resolve_runtime_bind("value", "svelte:element").is_none());
}

/// Structural completeness: EVERY runtime-unsupported official ordinary-DOM bind named
/// in debt-ledger D-25 has an EXPLICIT shared-contract row carrying its REAL official
/// helper + `RuntimeSupport::Unsupported` — none relies on an absent-row fallthrough,
/// and none erases its helper identity. A future absent row (a bind the official
/// compiler emits but the contract omits) is caught here. The set is the codex-authored
/// D-25 list, each oracle-confirmed an official bind.
#[test]
fn every_unsupported_official_ordinary_dom_bind_has_an_explicit_row() {
    // (name, a representative official host) for each D-25 ordinary-DOM bind.
    let d25: &[(&str, &str)] = &[
        ("files", "input"),
        ("playbackRate", "video"),
        ("volume", "video"),
        ("muted", "video"),
        ("contentRect", "div"),
        ("contentBoxSize", "div"),
        ("borderBoxSize", "div"),
        ("devicePixelContentBoxSize", "div"),
        ("indeterminate", "input"),
        ("buffered", "video"),
        ("seekable", "video"),
        ("seeking", "video"),
        ("ended", "video"),
        ("readyState", "video"),
        ("naturalWidth", "img"),
        ("naturalHeight", "img"),
        ("videoWidth", "video"),
        ("videoHeight", "video"),
    ];
    for (name, host) in d25 {
        let row = lookup_bind_contract(name, host).unwrap_or_else(|| {
            panic!(
                "runtime-unsupported official bind `{name}` must be an EXPLICIT contract \
                 row (absent-row fail-closed is not acceptable for an official bind)"
            )
        });
        assert_eq!(
            row.support,
            RuntimeSupport::Unsupported,
            "unsupported official bind `{name}` must carry RuntimeSupport::Unsupported"
        );
        // The official helper identity is PRESERVED (never erased): the exact per-row
        // identity is pinned by the destructure test + the wrong/correct-helper tests.
        assert!(
            resolve_runtime_bind(name, host).is_none(),
            "unsupported bind `{name}` on `{host}` must fail closed at the runtime router"
        );
    }
}

/// The `resolve_runtime_bind` `.expect(...)` invariant + the support/identity
/// orthogonality: every SUPPORTED non-`this` row's official helper maps to an emittable
/// `RuntimeHelper` (so the router never panics), while an UNSUPPORTED row may carry an
/// emittable OR non-emittable official helper (support is the refusal axis, never the
/// helper identity).
#[test]
fn supported_rows_official_helper_maps_to_an_emittable_runtime_helper() {
    for c in SVELTE_BIND_CONTRACTS {
        if c.support == RuntimeSupport::Supported && c.special != BindSpecial::This {
            assert!(
                c.official_helper.emittable_runtime_helper().is_some(),
                "supported row `{}` must map to an emittable runtime helper",
                c.name
            );
        }
    }
    // NEGATIVE: an unsupported DEDICATED-helper row maps to None (not emittable here).
    let files = SVELTE_BIND_CONTRACTS
        .iter()
        .find(|c| c.name == "files")
        .unwrap();
    assert_eq!(files.support, RuntimeSupport::Unsupported);
    assert_eq!(files.official_helper, OfficialRuntimeHelper::Files);
    assert!(files.official_helper.emittable_runtime_helper().is_none());
    // ORTHOGONALITY: an unsupported GENERIC-property row maps to Some(Property) yet is
    // still refused at the router — proving support, not identity, gates emission.
    let indeterminate = SVELTE_BIND_CONTRACTS
        .iter()
        .find(|c| c.name == "indeterminate")
        .unwrap();
    assert_eq!(indeterminate.support, RuntimeSupport::Unsupported);
    assert_eq!(
        indeterminate.official_helper.emittable_runtime_helper(),
        Some(RuntimeHelper::Property)
    );
    assert!(resolve_runtime_bind("indeterminate", "input").is_none());
}

/// The TOTAL `OfficialRuntimeHelper` → `RuntimeHelper` mapping, pinned for EVERY
/// variant (the emittable subset maps to its matching `RuntimeHelper`; the dedicated
/// runtime-unsupported helpers + the resize-observer family map to `None`). The
/// builtin form-control helpers (`$.bind_value` / `$.bind_select_value` /
/// `$.bind_checked`) are NOT in this row-scoped enum — they are built directly as a
/// `RuntimeHelper` in the builtin arm of `resolve_runtime_bind`. A new official helper
/// without a mapping decision fails to compile here.
#[test]
fn official_helper_maps_to_the_matching_emittable_runtime_helper() {
    use OfficialRuntimeHelper as O;
    use RuntimeHelper as R;
    // The emittable subset → its matching emittable helper.
    let emittable: &[(OfficialRuntimeHelper, RuntimeHelper)] = &[
        (O::Group, R::Group),
        (O::CurrentTime, R::CurrentTime),
        (O::Paused, R::Paused),
        (O::Played, R::Played),
        (O::ElementSize, R::ElementSize),
        (O::ContentEditable, R::ContentEditable),
        (O::Property, R::Property),
        (O::This, R::This),
        // The special-host (5f-b) emittable helpers.
        (O::Focused, R::Focused),
        (O::WindowSize, R::WindowSize),
        (O::WindowScroll, R::WindowScroll),
        (O::Online, R::Online),
        (O::ActiveElement, R::ActiveElement),
    ];
    for (official, expected) in emittable {
        assert_eq!(
            official.emittable_runtime_helper(),
            Some(*expected),
            "official helper {official:?} must map to the emittable {expected:?}"
        );
    }
    // The dedicated runtime-unsupported helpers + the resize-observer family are NOT
    // emittable by the native client runtime (they fail closed at the router via
    // `RuntimeSupport::Unsupported`), so they have no `RuntimeHelper` mapping.
    let non_emittable = [
        O::Files,
        O::Volume,
        O::Muted,
        O::PlaybackRate,
        O::Buffered,
        O::Seekable,
        O::Seeking,
        O::Ended,
        O::ReadyState,
        O::ResizeObserver,
    ];
    for official in non_emittable {
        assert_eq!(
            official.emittable_runtime_helper(),
            None,
            "dedicated/unsupported official helper {official:?} must not be emittable here"
        );
    }
}
