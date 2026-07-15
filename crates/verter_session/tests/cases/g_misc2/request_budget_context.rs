use std::sync::Arc;

use verter_scheduler::request_context::RequestContextLike;
use verter_session::request_context::{
    current_request_budget, RequestContext, RequestContextGuard,
};

#[test]
fn current_request_budget_comes_from_installed_request_context() {
    assert!(current_request_budget().is_none());
    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        3,
        Arc::from("/budget.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        17,
    );
    {
        let _g = RequestContextGuard::install(Arc::clone(&ctx));
        let budget = current_request_budget().expect("budget rides on request context");
        assert_eq!(budget.projection_op_budget, 17);
        assert!(Arc::ptr_eq(&budget, &ctx.projection_budget));
    }
    assert!(current_request_budget().is_none());
}

#[test]
fn current_request_budget_propagates_through_request_context_install_tls() {
    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        4,
        Arc::from("/worker-budget.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        19,
    );
    {
        let _g = Arc::clone(&ctx).install_tls();
        let budget =
            current_request_budget().expect("install_tls must populate request budget TLS");
        assert_eq!(budget.projection_op_budget, 19);
        assert!(Arc::ptr_eq(&budget, &ctx.projection_budget));
    }
    assert!(current_request_budget().is_none());
}
