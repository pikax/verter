use crate::{
    run_stable_request, RequestRunResult, ResolutionNodeKey, SingleflightGroup,
    StableExecutionValue, StableRequestExecutor, StoreView,
};

pub trait ComponentMetaRequestHost {
    type View: StoreView + Clone;
    type Mode: Copy;
    type Resolution: Clone;
    type CapturedInputs;

    fn cache_key(&self, canonical: &str, mode: Self::Mode) -> ResolutionNodeKey;
    fn snapshot_store_view(&self) -> Self::View;
    fn view_mutation_epoch(&self, store_view: &Self::View) -> u64;
    fn current_store_view_epoch(&self) -> u64;
    fn capture_component_meta_inputs(
        &self,
        canonical: &str,
        store_view: &Self::View,
    ) -> Option<Self::CapturedInputs>;
    fn try_get_cached_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        store_view: &Self::View,
    ) -> Option<Self::Resolution>;
    fn compute_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        captured: Option<&Self::CapturedInputs>,
        store_view: Option<&Self::View>,
    ) -> Option<Self::Resolution>;
    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    );
}

struct ComponentMetaRequestExecutor<'a, H: ComponentMetaRequestHost> {
    host: &'a H,
    canonical: String,
    mode: H::Mode,
    last_snapshot_epoch: Option<u64>,
    captured_inputs: Option<H::CapturedInputs>,
    max_attempts: usize,
}

impl<'a, H: ComponentMetaRequestHost> ComponentMetaRequestExecutor<'a, H> {
    fn new(host: &'a H, canonical: String, mode: H::Mode, max_attempts: usize) -> Self {
        Self {
            host,
            canonical,
            mode,
            last_snapshot_epoch: None,
            captured_inputs: None,
            max_attempts,
        }
    }
}

impl<'a, H> StableRequestExecutor<ResolutionNodeKey, Option<H::Resolution>>
    for ComponentMetaRequestExecutor<'a, H>
where
    H: ComponentMetaRequestHost,
{
    type View = H::View;
    type Error = ();

    fn cache_key(&self) -> ResolutionNodeKey {
        self.host.cache_key(&self.canonical, self.mode)
    }

    fn snapshot_view(&mut self) -> Self::View {
        for _ in 0..self.max_attempts {
            let view = self.host.snapshot_store_view();
            let captured_inputs = self
                .host
                .capture_component_meta_inputs(&self.canonical, &view);
            let view_epoch = self.host.view_mutation_epoch(&view);
            if self.host.current_store_view_epoch() == view_epoch {
                self.last_snapshot_epoch = Some(view_epoch);
                self.captured_inputs = captured_inputs;
                return view;
            }
        }

        let view = self.host.snapshot_store_view();
        self.last_snapshot_epoch = Some(self.host.view_mutation_epoch(&view));
        self.captured_inputs = self
            .host
            .capture_component_meta_inputs(&self.canonical, &view);
        view
    }

    fn try_get_cached(&mut self, view: &Self::View) -> Option<Option<H::Resolution>> {
        self.host
            .try_get_cached_component_meta(&self.canonical, self.mode, view)
            .map(Some)
    }

    fn compute(&mut self, view: &Self::View) -> Result<Option<H::Resolution>, Self::Error> {
        Ok(self.host.compute_component_meta(
            &self.canonical,
            self.mode,
            self.captured_inputs.as_ref(),
            Some(view),
        ))
    }

    fn is_stable(&mut self, _view: &Self::View) -> bool {
        self.last_snapshot_epoch
            .is_some_and(|epoch| self.host.current_store_view_epoch() == epoch)
    }

    fn store_stable(&mut self, value: &Option<H::Resolution>) {
        if let Some(result) = value.as_ref() {
            self.host
                .store_component_meta_result(&self.canonical, self.mode, result);
        }
    }

    fn max_attempts(&self) -> usize {
        self.max_attempts
    }
}

pub fn run_component_meta_request<H>(
    host: &H,
    singleflight: &SingleflightGroup<
        ResolutionNodeKey,
        StableExecutionValue<Option<H::Resolution>>,
        (),
    >,
    canonical: &str,
    mode: H::Mode,
    max_attempts: usize,
) -> RequestRunResult<Option<H::Resolution>>
where
    H: ComponentMetaRequestHost,
{
    let mut executor =
        ComponentMetaRequestExecutor::new(host, canonical.to_string(), mode, max_attempts);
    run_stable_request(singleflight, &mut executor)
        .expect("component-meta request execution is infallible")
}
