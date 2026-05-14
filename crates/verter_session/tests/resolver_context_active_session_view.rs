//! RED test: `ResolverContext::active_session_view` default impl returns `None`.
//!
//! Block 1.5 will add `SessionResolverContext` which overrides this method to
//! return `Some(view)`. For now only the default (VerterHost) path exists; this
//! test verifies the default returns `None` and does not panic.
//!
//! The sealed `ResolverContext` trait is not directly accessible from integration
//! tests, so we verify the behaviour through the `for_tests` shim which routes
//! the call through the sealed trait impl on `VerterHost`.
//!
//! NOTE: `SessionResolverContext` does not yet exist (pending Block 1.5). When
//! it lands, a new test that verifies `Some(view)` should be added; the test
//! below verifies the default (None) is stable.

use verter_session::VerterHost;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(Default::default())
}

#[test]
fn verter_host_active_session_view_default_is_none() {
    let host = make_host();

    // The default impl of `ResolverContext::active_session_view` must return None.
    let result = verter_session::for_tests::active_session_view_is_none_for_tests(&host);
    assert!(
        result,
        "VerterHost::active_session_view must return None (default impl)"
    );
}

#[test]
fn active_session_view_default_does_not_panic() {
    // Must not panic — the default impl simply returns None.
    let result = std::panic::catch_unwind(|| {
        let h = VerterHost::new_standalone(Default::default());
        verter_session::for_tests::active_session_view_is_none_for_tests(&h)
    });
    assert!(result.is_ok(), "active_session_view default must not panic");
    assert!(result.unwrap(), "must return true (None from default)");
}

#[test]
fn active_session_view_called_repeatedly_is_stable() {
    let host = make_host();

    // Calling it multiple times must return None each time (no state mutation).
    for _ in 0..5 {
        let is_none = verter_session::for_tests::active_session_view_is_none_for_tests(&host);
        assert!(is_none, "active_session_view must consistently return None");
    }
}
