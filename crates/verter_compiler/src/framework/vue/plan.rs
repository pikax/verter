use verter_audit::observe::ObserveExec;

use crate::assembly::emit_plan::MapMode;
use crate::compile_request::demand::{DemandSeed, DemandSet, TargetKind};
use crate::compile_request::{
    CompileProduct, CompileRequest, CompileRequestError, ProductKind, ResolvedVueBackend,
    RuntimeProductRequest,
};
use crate::framework_common::VueCompileAdmission;
use crate::target_plan::{plan_targets, TargetPlan, TargetPlanRefusal};
use crate::work_ledger::CompilerWorkLedger;

/// A Vue compile target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VueTarget {
    Vdom,
    Ssr,
    Vapor,
}

impl VueTarget {
    pub const fn kind(self) -> TargetKind {
        match self {
            VueTarget::Vdom => TargetKind::ClientTree,
            VueTarget::Vapor => TargetKind::ClientFineGrained,
            VueTarget::Ssr => TargetKind::Server,
        }
    }
    /// The client target a resolved backend compiles to.
    const fn client(backend: ResolvedVueBackend) -> Self {
        match backend {
            ResolvedVueBackend::Vdom => VueTarget::Vdom,
            ResolvedVueBackend::Vapor => VueTarget::Vapor,
        }
    }
}

/// One target of a Vue compile, with its resolved options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueTargetRequest {
    pub target: VueTarget,
    pub product: ProductKind,
    /// Render function inlined into `setup` (client tree target only).
    pub inline: bool,
    pub map_mode: MapMode,
}

/// The plan of one Vue compile: the admitted demand and its targets.
#[derive(Debug, Clone)]
pub struct VueCompilePlan {
    pub demand: DemandSet,
    pub targets: Vec<VueTargetRequest>,
    pub shared: TargetPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VuePlanRefusal {
    /// The request refused after parse (server with fine-grained client,
    /// inline with server).
    Request(CompileRequestError),
    Target(TargetPlanRefusal),
}

/// The runtime products of a Vue request, each with the target the backend
/// compiles it to, in request order.
fn runtime_targets(
    request: &CompileRequest,
    backend: ResolvedVueBackend,
) -> impl Iterator<Item = (ProductKind, &RuntimeProductRequest, VueTarget)> {
    let client = VueTarget::client(backend);
    request
        .products()
        .iter()
        .filter_map(move |product| match product {
            CompileProduct::RuntimeClient(runtime) => Some((product.kind(), runtime, client)),
            CompileProduct::RuntimeServer(runtime) => {
                Some((product.kind(), runtime, VueTarget::Ssr))
            }
            _ => None,
        })
}

/// Demand seeds for the runtime products of a Vue request.
pub fn vue_demand_seeds(
    request: &CompileRequest,
    backend: ResolvedVueBackend,
    has_template: bool,
    has_script: bool,
    style_count: u32,
) -> Vec<DemandSeed> {
    runtime_targets(request, backend)
        .map(|(product, runtime, target)| DemandSeed {
            product,
            target: target.kind(),
            runtime_source_map: runtime.runtime_source_map,
            style_processing: runtime.style_processing,
            has_template,
            has_script,
            style_count,
        })
        .collect()
}

/// Build the plan from an admitted compile.
pub fn plan_vue_compile<M: ObserveExec>(
    admission: &VueCompileAdmission,
    ledger: &mut CompilerWorkLedger<M>,
) -> Result<VueCompilePlan, VuePlanRefusal> {
    plan_admitted(
        admission.request(),
        admission.backend(),
        admission.demand_set(),
        ledger,
    )
}

/// The plan of an admitted request: its resolved backend and its closed
/// demand. One target request per planned target, in plan order.
pub(crate) fn plan_admitted<M: ObserveExec>(
    request: &CompileRequest,
    backend: Result<ResolvedVueBackend, &CompileRequestError>,
    demand: &DemandSet,
    ledger: &mut CompilerWorkLedger<M>,
) -> Result<VueCompilePlan, VuePlanRefusal> {
    let backend = backend.map_err(|refusal| VuePlanRefusal::Request(refusal.clone()))?;
    let shared = plan_targets(demand, ledger).map_err(VuePlanRefusal::Target)?;
    let mut targets = Vec::with_capacity(shared.targets().len());
    for planned in shared.targets() {
        // The demand was closed over this request's runtime products, so
        // each planned target has its product here.
        let (_, runtime, target) = runtime_targets(request, backend)
            .find(|(product, _, target)| {
                *product == planned.product && target.kind() == planned.target
            })
            .ok_or(VuePlanRefusal::Target(TargetPlanRefusal::DemandNotClosed {
                target: planned.target,
            }))?;
        targets.push(VueTargetRequest {
            target,
            product: planned.product,
            inline: runtime.resolve_inline(request.is_production()),
            map_mode: if planned.wants_map {
                MapMode::Map
            } else {
                MapMode::NoMap
            },
        });
    }
    Ok(VueCompilePlan {
        demand: demand.clone(),
        targets,
        shared,
    })
}

#[cfg(test)]
mod tests {
    use verter_audit::observe::Uncaptured;

    use super::*;
    use crate::compile_request::demand::close_demand;
    use crate::compile_request::{FrameworkCompileRequest, VueCompileRequest};

    fn client(inline: Option<bool>, runtime_source_map: bool) -> CompileProduct {
        CompileProduct::RuntimeClient(RuntimeProductRequest {
            inline,
            runtime_source_map,
            ..Default::default()
        })
    }

    fn server() -> CompileProduct {
        CompileProduct::RuntimeServer(RuntimeProductRequest::default())
    }

    fn request(products: Vec<CompileProduct>, is_production: bool) -> CompileRequest {
        CompileRequest::new(
            products,
            FrameworkCompileRequest::Vue(VueCompileRequest::default()),
            None,
            Some("App.vue".to_owned()),
            Some("abc123".to_owned()),
            is_production,
            false,
        )
        .expect("a valid Vue request")
    }

    /// Plan a request over a component with a template and a script, as
    /// admission does: the backend resolves against the component's own
    /// marker, and a request that does not resolve closes no demand.
    fn plan(
        request: &CompileRequest,
        component_marks_vapor: bool,
    ) -> Result<VueCompilePlan, VuePlanRefusal> {
        plan_component(request, component_marks_vapor, true)
    }

    fn plan_component(
        request: &CompileRequest,
        component_marks_vapor: bool,
        has_template: bool,
    ) -> Result<VueCompilePlan, VuePlanRefusal> {
        let backend = request.resolve_vue_backend(component_marks_vapor);
        let seeds = match &backend {
            Ok(backend) => vue_demand_seeds(request, *backend, has_template, true, 0),
            Err(_) => Vec::new(),
        };
        plan_admitted(
            request,
            backend.as_ref().copied(),
            &close_demand(&seeds),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
    }

    #[test]
    fn server_with_fine_grained_client_is_refused() {
        let both = request(vec![client(None, false), server()], false);
        assert_eq!(
            plan(&both, true).unwrap_err(),
            VuePlanRefusal::Request(CompileRequestError::SsrVaporBackendUnsupported)
        );

        let plan = plan(&both, false).unwrap();
        assert_eq!(
            plan.targets
                .iter()
                .map(|target| (target.product, target.target))
                .collect::<Vec<_>>(),
            [
                (ProductKind::RuntimeClient, VueTarget::Vdom),
                (ProductKind::RuntimeServer, VueTarget::Ssr),
            ]
        );
        assert!(plan.shared.wants(TargetKind::Server));
        assert_eq!(plan.demand.targets().len(), 2);
    }

    #[test]
    fn inline_resolves_from_production_flag() {
        for is_production in [false, true] {
            let plan = plan(&request(vec![client(None, true)], is_production), false).unwrap();
            assert_eq!(
                plan.targets,
                [VueTargetRequest {
                    target: VueTarget::Vdom,
                    product: ProductKind::RuntimeClient,
                    inline: is_production,
                    map_mode: MapMode::Map,
                }]
            );
        }

        let explicit = plan(&request(vec![client(Some(false), false)], true), false).unwrap();
        assert!(!explicit.targets[0].inline);
        assert_eq!(explicit.targets[0].map_mode, MapMode::NoMap);

        let vapor = plan(&request(vec![client(None, false)], false), true).unwrap();
        assert_eq!(vapor.targets[0].target, VueTarget::Vapor);
        assert!(vapor.shared.wants(TargetKind::ClientFineGrained));
    }

    #[test]
    fn a_script_only_component_plans_no_targets() {
        let both = request(vec![client(None, true), server()], true);
        let plan = plan_component(&both, false, false).unwrap();
        assert!(plan.targets.is_empty());
        assert!(plan.shared.targets().is_empty());
        assert_eq!(plan.demand.targets().len(), 2);
    }
}
