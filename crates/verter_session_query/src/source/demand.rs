//! Outcome of a per-symbol body demand: a completed run, or a broken lease that
//! lowered nothing.
use crate::facts::reuse::{dominant_refusal, NonCacheableReadReason};
use crate::source::indexed_call::IndexedFlowCallExpression;
use verter_type_expr::TypeExpr;

use std::sync::Arc;

/// Outcome of a per-symbol body DEMAND ([`DeclBodyMemo::demand_and_commit`])
/// as seen by a caller that needs to DISTINGUISH the two `None`-shaped miss
/// classes (the locator-deref path, which must not collapse a transient
/// ReturnOnly into a cacheable resolution result):
///
/// - [`Ready`](Self::Ready) — the lease-only run completed. `Some` is the
///   demanded decl; `None` is a GENUINE, cacheable miss (the symbol is not
///   inventoried, or the run produced a fatal-parse empty).
/// - [`LeaseMiss`](Self::LeaseMiss) — the lease pin was broken: the demand
///   ran NOTHING and committed NOTHING (`ReturnOnly`). A caller must route
///   this to a no-warm signal, never treat it as a genuine miss.
#[must_use]
pub enum DemandOutcome<D> {
    Ready(Option<Arc<D>>),
    LeaseMiss,
}

impl<D> DemandOutcome<D> {
    /// Collapse to the plain `Option` value, carrying the refusal evidence
    /// a lease miss implies BY VALUE. Used by the broad `Option`-returning
    /// demand accessors whose consumers do NOT distinguish the transient
    /// ReturnOnly from a genuine miss (the per-symbol demand cell already
    /// fails closed by evicting the poisoned cell, so a later demand under a
    /// live lease recovers).
    ///
    /// Pure: a `LeaseMiss` reads as `None` with a
    /// [`NonCacheableReadReason::LeaseMiss`] refusal the consuming engine
    /// applies, so an enclosing traced compute refuses shared-cache
    /// admission. A `Ready(None)` genuine absence carries no refusal and
    /// stays cacheable.
    pub fn into_source_read(self) -> SourceRead<Option<Arc<D>>> {
        match self {
            DemandOutcome::Ready(value) => SourceRead::clean(value),
            DemandOutcome::LeaseMiss => SourceRead {
                value: None,
                refusal: Some(NonCacheableReadReason::LeaseMiss),
            },
        }
    }
}

/// A source-side read together with the non-cacheable refusal its basis
/// carries. Source code cannot reach the engine's tracers, so the evidence
/// travels with the value until the first engine consumer applies it
/// (before any early return, admission decision or memo insertion). A
/// composition of several reads keeps the evidence of every read it
/// consulted — including a read whose miss a later fallback answered.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRead<T> {
    pub value: T,
    pub refusal: Option<NonCacheableReadReason>,
}

impl<T> SourceRead<T> {
    /// A read whose basis refused nothing.
    #[inline]
    pub fn clean(value: T) -> Self {
        Self {
            value,
            refusal: None,
        }
    }

    /// Transform the value, keeping the evidence.
    #[inline]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> SourceRead<U> {
        SourceRead {
            value: f(self.value),
            refusal: self.refusal,
        }
    }

    /// Fold another read's evidence into this one, keeping the dominant
    /// refusal (the first observed on a tie).
    #[inline]
    fn absorb(&mut self, refusal: Option<NonCacheableReadReason>) {
        self.refusal = match (self.refusal, refusal) {
            (Some(existing), Some(incoming)) => Some(dominant_refusal(existing, incoming)),
            (existing, incoming) => existing.or(incoming),
        };
    }
}

impl<V> SourceRead<Option<V>> {
    /// This read's value, else `fallback`'s. The result keeps THIS read's
    /// evidence even when the fallback supplies the value: the consumer
    /// still consumed the refused read before falling back.
    #[inline]
    pub fn or_else(self, fallback: impl FnOnce() -> SourceRead<Option<V>>) -> Self {
        if self.value.is_some() {
            return self;
        }
        let refusal = self.refusal;
        let mut read = fallback();
        let incoming = read.refusal;
        read.refusal = refusal;
        read.absorb(incoming);
        read
    }
}

/// The expression-source capability of one exact observed source: function
/// program structure, indexed expression IR and flow-slice content, each
/// demanded lazily from the source's retained parse. Selecting the
/// capability performs no parsing or lowering.
pub trait ExpressionSourceDemand: Send + Sync {
    fn function_program_index(&self) -> Arc<crate::function_program::FunctionProgramIndex>;

    fn indexed_program_expression_ir(
        &self,
        record: &crate::function_program::ProgramExpressionRecord,
    ) -> Option<Arc<verter_type_expr::IndexedValueExpression>>;

    fn indexed_call_expression_over_frame_at(
        &self,
        span: verter_span::Span,
        frame_lowered: Arc<[bool]>,
    ) -> Option<Arc<IndexedFlowCallExpression>>;

    fn function_type_param_clause(
        &self,
        entry: &crate::function_program::FunctionProgramEntry,
    ) -> Option<Vec<crate::flow::slice::SliceTypeParam>>;

    fn flow_slice_content(
        &self,
        entry: &crate::function_program::FunctionProgramEntry,
        selection: crate::flow::slice::FlowSliceSelection,
        bound: &crate::flow::bundle::BoundFlowGraph,
        policy: crate::flow::policy::FlowReturnPolicy,
    ) -> Option<Arc<crate::flow::slice::SliceContent>>;

    fn flow_slice_content_with_context(
        &self,
        entry: &crate::function_program::FunctionProgramEntry,
        selection: Option<crate::flow::slice::FlowSliceSelection>,
        bound: &crate::flow::bundle::BoundFlowGraph,
        context: Option<Arc<crate::flow::slice::NestedFlowContext>>,
        policy: crate::flow::policy::FlowReturnPolicy,
    ) -> Option<Arc<crate::flow::slice::SliceContent>>;

    fn flow_capture_authorities(
        &self,
        locators: &[crate::flow::slice::SliceCaptureAuthorityLocator],
    ) -> Option<Vec<Option<Option<crate::flow::slice::SliceCaptureAuthority>>>>;

    fn transient_macro_type_argument(
        &self,
        macro_span: verter_span::Span,
    ) -> DemandOutcome<TypeExpr>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lease_miss_reads_as_a_refused_absence_and_a_ready_absence_as_a_clean_one() {
        let miss = DemandOutcome::<u8>::LeaseMiss.into_source_read();
        assert_eq!(miss.value, None);
        assert_eq!(miss.refusal, Some(NonCacheableReadReason::LeaseMiss));

        let absent = DemandOutcome::<u8>::Ready(None).into_source_read();
        assert_eq!(absent.value, None);
        assert_eq!(
            absent.refusal, None,
            "a genuine absence is a cacheable answer and must carry no refusal"
        );

        let present = DemandOutcome::Ready(Some(Arc::new(7u8))).into_source_read();
        assert_eq!(present.value.as_deref(), Some(&7));
        assert_eq!(present.refusal, None);
    }

    #[test]
    fn a_fallback_value_keeps_the_refusal_of_the_read_it_replaced() {
        let read = SourceRead::<Option<u8>> {
            value: None,
            refusal: Some(NonCacheableReadReason::LeaseMiss),
        }
        .or_else(|| SourceRead::clean(Some(1)));
        assert_eq!(read.value, Some(1));
        assert_eq!(
            read.refusal,
            Some(NonCacheableReadReason::LeaseMiss),
            "the consumer consumed the refused read before falling back"
        );

        let read = SourceRead::<Option<u8>> {
            value: None,
            refusal: Some(NonCacheableReadReason::FencedServe),
        }
        .or_else(|| SourceRead {
            value: Some(2),
            refusal: Some(NonCacheableReadReason::LeaseMiss),
        });
        assert_eq!(
            read.refusal,
            Some(NonCacheableReadReason::LeaseMiss),
            "a transient refusal from either side dominates"
        );

        let read = SourceRead::<Option<u8>>::clean(Some(3)).or_else(|| {
            unreachable!("a present value never consults its fallback");
        });
        assert_eq!(read, SourceRead::clean(Some(3)));
    }
}
