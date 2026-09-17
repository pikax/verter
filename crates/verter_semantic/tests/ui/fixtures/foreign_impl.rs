//! C2-GAP3-FOREIGN-IMPL: the observation contract is sealed — a foreign
//! crate must not implement it and inject its own semantic source.

use std::sync::Arc;

use verter_semantic::analysis::ScriptAnalysisSnapshot;
use verter_semantic::type_info::{
    NonFlowObservation, NonFlowObservationSnapshot, ObservedMacroSurface, TypeInfoCore,
};

struct ForeignSource;

impl NonFlowObservation for ForeignSource {
    fn script_analysis(&self, _owner_canonical: &str) -> Option<&Arc<ScriptAnalysisSnapshot>> {
        None
    }
    fn import_resolution(
        &self,
        _owner_canonical: &str,
        _specifier: &str,
    ) -> Option<Option<&Arc<str>>> {
        None
    }
    fn macro_surface(
        &self,
        _owner_canonical: &str,
        _macro_index: usize,
    ) -> Option<&ObservedMacroSurface> {
        None
    }
    fn model_value_type_shape(
        &self,
        _owner_canonical: &str,
        _macro_index: usize,
    ) -> Option<&verter_macro_dto::RuntimePropType> {
        None
    }
}

fn main() {
    // Even with a foreign source in hand, the kernel accepts only the
    // sealed in-crate snapshot — this construction must not compile.
    let core = TypeInfoCore::from_observation_snapshot(
        Arc::new(NonFlowObservationSnapshot::new()),
        verter_semantic::resolver_core::ResolutionBasis::unbound_placeholder(),
    );
    let _ = &core;
    let _ = ForeignSource;
}
