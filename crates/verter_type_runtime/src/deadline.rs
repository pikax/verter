//! Ambient per-request deadline, propagated to the provider transports.
//!
//! A type-provider round-trip must be bounded strictly INSIDE the deadline of
//! the request that asked for it. Otherwise the outer bound fires first and the
//! caller learns only "the handler ran out of time" — the provider hop is
//! abandoned mid-flight with nothing attributing the failure to the engine, and
//! nothing telling the engine to stop working.
//!
//! Threading a deadline argument through every [`crate::traits::TypeProvider`]
//! method would make every implementation re-forward a value it never reads.
//! The deadline is request-scoped ambient context, so it rides a
//! [`tokio::task_local`]: the LSP handler opens a scope for the request, and the
//! transport at the bottom of the stack reads the remaining time. Both live on
//! the same task — the provider future is awaited by the handler, not spawned —
//! so the scope covers exactly the work the deadline governs.
//!
//! A task with no scope open ([`remaining`] returns `None`) is un-deadlined and
//! keeps whatever fixed bound it already had. Batch and background callers stay
//! on that path.

tokio::task_local! {
    /// The instant the in-scope request stops being worth serving.
    static REQUEST_DEADLINE: tokio::time::Instant;
}

/// Run `future` with an ambient deadline `budget` from now.
///
/// Nested scopes are permitted and the inner one wins for its extent; callers
/// that want the tighter of two bounds pass the minimum in.
pub async fn with_deadline<F: std::future::Future>(
    budget: std::time::Duration,
    future: F,
) -> F::Output {
    REQUEST_DEADLINE
        .scope(tokio::time::Instant::now() + budget, future)
        .await
}

/// Run `future` with an ambient deadline at a specific instant.
pub async fn with_deadline_at<F: std::future::Future>(
    at: tokio::time::Instant,
    future: F,
) -> F::Output {
    REQUEST_DEADLINE.scope(at, future).await
}

/// Time left on the ambient deadline, or `None` when the caller opened no
/// scope. A deadline already in the past yields `Some(Duration::ZERO)` — an
/// expired budget, distinct from an absent one.
pub fn remaining() -> Option<std::time::Duration> {
    REQUEST_DEADLINE
        .try_with(|at| at.saturating_duration_since(tokio::time::Instant::now()))
        .ok()
}

/// The margin a provider hop reserves below the ambient request deadline.
///
/// The hop bound must fire strictly BEFORE the request bound, or the outer
/// timeout wins the race and the failure is attributed to the handler rather
/// than to the engine that actually stalled. The margin is what buys the
/// transport its cleanup: remove the pending entry, count the failure toward
/// hang detection, emit the cancellation, and return an error the handler can
/// still surface as a definite answer.
pub const HOP_MARGIN: std::time::Duration = std::time::Duration::from_millis(150);

/// Bound a provider hop for a caller whose own fixed budget is `configured`.
///
/// Returns the smaller of `configured` and the ambient remaining time less
/// [`HOP_MARGIN`]. With no ambient deadline the caller keeps `configured`
/// unchanged, so background and batch hops are unaffected.
///
/// A deadline with less than the margin left still yields a non-zero floor
/// rather than zero: a hop given no time at all cannot distinguish "the engine
/// is wedged" from "the caller was already out of time", and would charge a
/// spurious failure toward hang detection.
pub fn hop_budget(configured: std::time::Duration) -> std::time::Duration {
    /// Smallest hop bound worth issuing. Below this the round-trip cannot
    /// succeed even against a healthy engine, so the request is better failed by
    /// the outer bound than charged to the provider.
    const FLOOR: std::time::Duration = std::time::Duration::from_millis(50);

    match remaining() {
        None => configured,
        Some(left) => configured.min(left.saturating_sub(HOP_MARGIN).max(FLOOR)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_unscoped_task_reports_no_deadline() {
        assert_eq!(remaining(), None, "no scope open means no ambient deadline");
        assert_eq!(
            hop_budget(std::time::Duration::from_secs(10)),
            std::time::Duration::from_secs(10),
            "an un-deadlined caller keeps its configured bound verbatim"
        );
    }

    #[tokio::test]
    async fn a_hop_is_bounded_strictly_inside_the_request_deadline() {
        let request = std::time::Duration::from_millis(1500);
        with_deadline(request, async {
            let hop = hop_budget(std::time::Duration::from_secs(10));
            assert!(
                hop < request,
                "the hop bound must fire before the request bound, got {hop:?} vs {request:?}"
            );
            assert!(
                hop >= request - HOP_MARGIN - std::time::Duration::from_millis(50),
                "the hop should still get nearly the whole request budget, got {hop:?}"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn a_configured_bound_tighter_than_the_deadline_still_wins() {
        with_deadline(std::time::Duration::from_secs(30), async {
            assert_eq!(
                hop_budget(std::time::Duration::from_secs(10)),
                std::time::Duration::from_secs(10),
                "the ambient deadline raises no hop above its own configured bound"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn an_exhausted_deadline_still_yields_a_usable_floor() {
        with_deadline(std::time::Duration::from_millis(1), async {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let hop = hop_budget(std::time::Duration::from_secs(10));
            assert!(
                !hop.is_zero(),
                "an exhausted budget must not issue a zero-length hop"
            );
            assert!(
                hop <= std::time::Duration::from_millis(50),
                "an exhausted budget issues only the floor, got {hop:?}"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn the_scope_survives_an_await_point() {
        with_deadline(std::time::Duration::from_secs(5), async {
            let before = remaining().expect("scope open before the await");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let after = remaining().expect("the scope must survive the await point");
            assert!(
                after < before,
                "the remaining budget must shrink across an await, {after:?} vs {before:?}"
            );
        })
        .await;
    }
}
