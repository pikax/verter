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
