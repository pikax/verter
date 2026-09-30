//! Single integration-test binary for `verter_supervise`: every case drives
//! the real `verter-supervise` binary over the `verter-supervise-fixture`
//! process tree.

mod cases {
    mod containment;
    mod contract;
    mod fail_closed;
    mod support;
    mod teardown;
}
