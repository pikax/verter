use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use verter_session::request_context::{
    current_request_context, RequestContext, RequestContextGuard,
};

fn make_ctx(request_id: u64) -> Arc<RequestContext> {
    RequestContext::new(request_id, Arc::from("/tls.vue"), true, None)
}

#[test]
fn request_context_guard_clears_tls_on_normal_return() {
    assert!(current_request_context().is_none());
    let ctx = make_ctx(1);
    {
        let _g = RequestContextGuard::install(Arc::clone(&ctx));
        assert_eq!(current_request_context().unwrap().request_id, 1);
    }
    assert!(current_request_context().is_none());
}

#[test]
fn request_context_guard_clears_tls_on_panic_unwind() {
    let ctx = make_ctx(2);
    let r = catch_unwind(AssertUnwindSafe(|| {
        let _g = RequestContextGuard::install(Arc::clone(&ctx));
        assert_eq!(current_request_context().unwrap().request_id, 2);
        panic!("test panic");
    }));
    assert!(r.is_err());
    assert!(
        current_request_context().is_none(),
        "TLS slot must be cleared via the guard's Drop on unwind",
    );
}
