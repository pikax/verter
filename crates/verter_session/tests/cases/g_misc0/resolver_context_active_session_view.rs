//! Base requests carry no session view in their private lifecycle.
//! Integration tests observe the real request adapter through a test-support
//! shim; the six engine ports cannot expose the lifecycle or its view.

use verter_session::VerterHost;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(Default::default())
}

#[test]
fn verter_host_active_session_view_default_is_none() {
    let host = make_host();

    // A base request has no session overlay.
    let result = verter_session::for_tests::active_session_view_is_none_for_tests(&host);
    assert!(
        result,
        "the base request lifecycle must carry no session view"
    );
}

#[test]
fn active_session_view_default_does_not_panic() {
    // Constructing and observing the base request must not panic.
    let result = std::panic::catch_unwind(|| {
        let h = VerterHost::new_standalone(Default::default());
        verter_session::for_tests::active_session_view_is_none_for_tests(&h)
    });
    assert!(result.is_ok(), "base request observation must not panic");
    assert!(result.unwrap(), "base request must carry no session view");
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
