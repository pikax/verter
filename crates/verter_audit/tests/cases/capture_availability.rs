use std::cell::Cell;

use verter_audit::observe::{capture, CaptureAvailability, ObserveMode};

#[test]
fn unavailable_capture_does_not_collect_or_fabricate_metrics() {
    let calls = Cell::new(0);
    let metrics = capture(|| {
        calls.set(calls.get() + 1);
        37
    });

    #[cfg(not(feature = "semantic-observe"))]
    {
        assert_eq!(
            CaptureAvailability::compiled(),
            CaptureAvailability::Unavailable
        );
        assert_eq!(ObserveMode::compiled(), ObserveMode::Uncaptured);
        assert_eq!(metrics, None);
        assert_eq!(calls.get(), 0);
    }
    #[cfg(feature = "semantic-observe")]
    {
        assert_eq!(
            CaptureAvailability::compiled(),
            CaptureAvailability::Available
        );
        assert_eq!(ObserveMode::compiled(), ObserveMode::Captured);
        assert_eq!(metrics, Some(37));
        assert_eq!(calls.get(), 1);
    }
}

#[test]
fn observer_callback_return_keeps_the_callers_control_flow() {
    use verter_audit::observe::{Captured, ObserveExec, Uncaptured};
    use verter_audit::AuditObserver;

    struct Receiver;
    impl AuditObserver for Receiver {}

    fn invoke<M: ObserveExec>(_observer: &dyn AuditObserver, seen: &Cell<u32>, after: &Cell<bool>) {
        verter_audit::observe!(M => on Some(_observer) => |_observer| {
            seen.set(seen.get() + 1);
            if seen.get() == 1 {
                return;
            }
            seen.set(99);
        });
        after.set(true);
    }

    let seen = Cell::new(0);
    let after = Cell::new(false);
    invoke::<Captured>(&Receiver, &seen, &after);
    assert!(after.get(), "return belongs to the observation callback");
    assert_eq!(seen.get(), u32::from(cfg!(feature = "semantic-observe")));

    seen.set(0);
    after.set(false);
    invoke::<Uncaptured>(&Receiver, &seen, &after);
    assert!(after.get());
    assert_eq!(seen.get(), 0);
}
