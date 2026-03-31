use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_expr::TypeExpr;

use crate::{
    fallthrough_cache_key, run_stable_request, FallthroughNodeKey, RequestRunResult,
    SingleflightGroup, StableExecutionValue, StableRequestExecutor, StoreView,
};

pub trait FallthroughRequestHost {
    type View: StoreView + Clone;
    type Resolution: Clone;

    fn generic_root_propagation(&self) -> bool;
    fn snapshot_store_view(&self) -> Self::View;
    fn view_mutation_epoch(&self, store_view: &Self::View) -> u64;
    fn current_store_view_epoch(&self) -> u64;
    fn try_get_cached_fallthrough(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<&FxHashMap<String, TypeExpr>>,
        store_view: &Self::View,
    ) -> Option<Self::Resolution>;
    fn compute_fallthrough_surface_uncached(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<&FxHashMap<String, TypeExpr>>,
        visiting: &mut FxHashSet<String>,
        store_view: Option<&Self::View>,
    ) -> Option<Self::Resolution>;
    fn store_fallthrough_result(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<&FxHashMap<String, TypeExpr>>,
        result: &Self::Resolution,
    );
}

struct FallthroughRequestExecutor<'a, 'b, H: FallthroughRequestHost> {
    host: &'a H,
    canonical_id: String,
    prop_type_overrides: Option<&'a FxHashMap<String, TypeExpr>>,
    visiting: &'b mut FxHashSet<String>,
    fixed_store_view: Option<H::View>,
    last_snapshot_epoch: Option<u64>,
    max_attempts: usize,
}

impl<'a, 'b, H: FallthroughRequestHost> FallthroughRequestExecutor<'a, 'b, H> {
    fn new(
        host: &'a H,
        canonical_id: String,
        prop_type_overrides: Option<&'a FxHashMap<String, TypeExpr>>,
        visiting: &'b mut FxHashSet<String>,
        max_attempts: usize,
    ) -> Self {
        Self {
            host,
            canonical_id,
            prop_type_overrides,
            visiting,
            fixed_store_view: None,
            last_snapshot_epoch: None,
            max_attempts,
        }
    }

    fn with_fixed_view(mut self, store_view: Option<&H::View>) -> Self {
        self.fixed_store_view = store_view.cloned();
        self
    }
}

impl<'a, 'b, H> StableRequestExecutor<FallthroughNodeKey, Option<H::Resolution>>
    for FallthroughRequestExecutor<'a, 'b, H>
where
    H: FallthroughRequestHost,
{
    type View = H::View;
    type Error = ();

    fn cache_key(&self) -> FallthroughNodeKey {
        fallthrough_cache_key(
            &self.canonical_id,
            self.host.generic_root_propagation(),
            self.prop_type_overrides,
        )
    }

    fn snapshot_view(&mut self) -> Self::View {
        if let Some(view) = self.fixed_store_view.as_ref() {
            self.last_snapshot_epoch = Some(self.host.view_mutation_epoch(view));
            return view.clone();
        }
        let view = self.host.snapshot_store_view();
        self.last_snapshot_epoch = Some(self.host.view_mutation_epoch(&view));
        view
    }

    fn try_get_cached(&mut self, view: &Self::View) -> Option<Option<H::Resolution>> {
        self.host
            .try_get_cached_fallthrough(&self.canonical_id, self.prop_type_overrides, view)
            .map(Some)
    }

    fn compute(&mut self, view: &Self::View) -> Result<Option<H::Resolution>, Self::Error> {
        Ok(self.host.compute_fallthrough_surface_uncached(
            &self.canonical_id,
            self.prop_type_overrides,
            self.visiting,
            Some(view),
        ))
    }

    fn is_stable(&mut self, _view: &Self::View) -> bool {
        if self.fixed_store_view.is_some() {
            return true;
        }
        self.last_snapshot_epoch
            .is_some_and(|epoch| self.host.current_store_view_epoch() == epoch)
    }

    fn store_stable(&mut self, value: &Option<H::Resolution>) {
        if let Some(result) = value.as_ref() {
            self.host.store_fallthrough_result(
                &self.canonical_id,
                self.prop_type_overrides,
                result,
            );
        }
    }

    fn max_attempts(&self) -> usize {
        self.max_attempts
    }
}

pub fn run_fallthrough_request<H>(
    host: &H,
    singleflight: &SingleflightGroup<
        FallthroughNodeKey,
        StableExecutionValue<Option<H::Resolution>>,
        (),
    >,
    canonical_id: &str,
    prop_type_overrides: Option<&FxHashMap<String, TypeExpr>>,
    visiting: &mut FxHashSet<String>,
    fixed_store_view: Option<&H::View>,
    max_attempts: usize,
) -> RequestRunResult<Option<H::Resolution>>
where
    H: FallthroughRequestHost,
{
    let mut executor = FallthroughRequestExecutor::new(
        host,
        canonical_id.to_string(),
        prop_type_overrides,
        visiting,
        max_attempts,
    )
    .with_fixed_view(fixed_store_view);
    run_stable_request(singleflight, &mut executor)
        .expect("fallthrough request execution is infallible")
}
