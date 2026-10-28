use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use super::mock::MockTypeProvider;
use super::traits::{ProviderQuery, TypeProvider};

struct CallWake {
    provider: Arc<MockTypeProvider>,
    count: AtomicUsize,
}

impl Wake for CallWake {
    fn wake(self: Arc<Self>) {
        assert_eq!(self.provider.calls().len(), 1);
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn interactive_queries_wake_call_log_waiters_even_when_the_answer_hangs() {
    #[derive(Clone, Copy, Debug)]
    enum Query {
        Definition,
        Hover,
        Signature,
        Diagnostics,
        Tokens,
        Highlights,
        InlayHints,
    }
    for kind in [
        Query::Definition,
        Query::Hover,
        Query::Signature,
        Query::Diagnostics,
        Query::Tokens,
        Query::Highlights,
        Query::InlayHints,
    ] {
        for hang in [false, true] {
            if hang && matches!(kind, Query::Tokens | Query::Highlights | Query::InlayHints) {
                continue;
            }
            let provider = Arc::new(MockTypeProvider::new());
            if hang {
                match kind {
                    Query::Definition => provider.hang_definition(),
                    Query::Hover => provider.hang_hover(),
                    Query::Signature => provider.hang_signature_help(),
                    Query::Diagnostics => provider.hang_diagnostics(),
                    Query::Tokens | Query::Highlights | Query::InlayHints => unreachable!(),
                }
            }
            let wakes = Arc::new(CallWake {
                provider: provider.clone(),
                count: AtomicUsize::new(0),
            });
            let waker = Waker::from(wakes.clone());
            let mut cx = Context::from_waker(&waker);
            let mut recorded = std::pin::pin!(provider.wait_until_calls(|calls| !calls.is_empty()));
            assert!(recorded.as_mut().poll(&mut cx).is_pending());
            let query = ProviderQuery::at_engine_surface("/query.ts");
            let mut answer: Pin<Box<dyn Future<Output = ()> + '_>> = match kind {
                Query::Definition => Box::pin(async {
                    let _ = provider.get_definition(&query, 0).await;
                }),
                Query::Hover => Box::pin(async {
                    let _ = provider.get_hover(&query, 0).await;
                }),
                Query::Signature => Box::pin(async {
                    let _ = provider.get_signature_help(&query, 0).await;
                }),
                Query::Diagnostics => Box::pin(async {
                    let _ = provider.get_diagnostics("/query.ts").await;
                }),
                Query::Tokens => Box::pin(async {
                    let _ = provider.get_semantic_tokens(&query).await;
                }),
                Query::Highlights => Box::pin(async {
                    let _ = provider.get_document_highlights(&query, 0).await;
                }),
                Query::InlayHints => Box::pin(async {
                    let _ = provider.get_inlay_hints(&query, 0, 1).await;
                }),
            };
            assert_eq!(answer.as_mut().poll(&mut cx).is_pending(), hang);
            assert!(
                wakes.count.load(Ordering::SeqCst) > 0,
                "{kind:?}, hang={hang}"
            );
            assert!(matches!(recorded.as_mut().poll(&mut cx), Poll::Ready(())));
        }
    }
}
