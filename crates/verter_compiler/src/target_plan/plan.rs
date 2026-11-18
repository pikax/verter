use verter_audit::observe::ObserveExec;

use crate::compile_request::demand::{Capability, DemandEdge, DemandReason, DemandSet, TargetKind};
use crate::compile_request::ProductKind;
use crate::work_ledger::{CompilerWorkLedger, WorkKind};

/// Capabilities every requested target reads from one shared computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SharedPrerequisites(pub u32);

impl SharedPrerequisites {
    pub fn contains(self, capability: Capability) -> bool {
        self.0 & capability.bit() != 0
    }
    pub fn count(self) -> u32 {
        self.0.count_ones()
    }
}

/// One target of the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedTarget {
    pub product: ProductKind,
    pub target: TargetKind,
    /// The planning capability only this target demands.
    pub planning: Capability,
    pub wants_map: bool,
}

/// The targets of one admitted compile and what they share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPlan {
    targets: Vec<PlannedTarget>,
    shared: SharedPrerequisites,
}

impl TargetPlan {
    pub fn targets(&self) -> &[PlannedTarget] {
        &self.targets
    }
    /// Prerequisites computed once for all targets. Targets branch after
    /// the last of these.
    pub fn shared(&self) -> SharedPrerequisites {
        self.shared
    }
    pub fn wants(&self, target: TargetKind) -> bool {
        self.targets.iter().any(|planned| planned.target == target)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetPlanRefusal {
    /// The demand names no target.
    NoTarget,
    /// The demand lacks the planning capability of a requested target.
    DemandNotClosed { target: TargetKind },
}

/// Work each target does for itself: its planning, and everything from
/// render emission to the published artifact.
const TARGET_OWN_WORK: u32 = Capability::PlanClientTree.bit()
    | Capability::PlanClientFineGrained.bit()
    | Capability::PlanServer.bit()
    | Capability::EmitRender.bit()
    | Capability::EmitModule.bit()
    | Capability::MapRuntime.bit()
    | Capability::PublishArtifacts.bit();

/// Plan the targets of an admitted compile from its closed demand. Records
/// one planning entry per target and the shared-prerequisite count.
pub fn plan_targets<M: ObserveExec>(
    demand: &DemandSet,
    ledger: &mut CompilerWorkLedger<M>,
) -> Result<TargetPlan, TargetPlanRefusal> {
    let requested = demand.targets();
    if requested.is_empty() {
        return Err(TargetPlanRefusal::NoTarget);
    }
    // A component without a template renders nothing, so its products want
    // no target work: the plan has no targets, and that is not a refusal.
    let requested = if demand.contains(Capability::EmitRender) {
        requested
    } else {
        &[]
    };
    let mut targets = Vec::with_capacity(requested.len());
    for &(product, target) in requested {
        let planning = Capability::planning(target);
        if !demand.contains(planning) {
            return Err(TargetPlanRefusal::DemandNotClosed { target });
        }
        targets.push(PlannedTarget {
            product,
            target,
            planning,
            wants_map: wants_map(demand, product),
        });
    }
    let shared = SharedPrerequisites(
        demand
            .capabilities()
            .filter(|capability| capability.bit() & TARGET_OWN_WORK == 0)
            .fold(0, |bits, capability| bits | capability.bit()),
    );
    ledger.record(WorkKind::PlanningTargetEntries, targets.len() as u64);
    if targets.len() >= 2 {
        ledger.record(
            WorkKind::PlanningMultiTargetSharedPrerequisites,
            u64::from(shared.count()),
        );
    }
    Ok(TargetPlan { targets, shared })
}

/// Whether `product` itself asked for a runtime map: the closure records
/// that request as a reason edge from the product to the map capability.
fn wants_map(demand: &DemandSet, product: ProductKind) -> bool {
    let requested = DemandEdge {
        from: DemandReason::Product(product as u8),
        to: Capability::MapRuntime,
    };
    demand.edges().binary_search(&requested).is_ok()
}

#[cfg(test)]
mod tests {
    use verter_audit::observe::Uncaptured;

    use super::*;
    use crate::compile_request::demand::{close_demand, DemandSeed};
    use crate::compile_request::RuntimeStyleProcessing;

    fn seed(product: ProductKind, target: TargetKind) -> DemandSeed {
        DemandSeed {
            product,
            target,
            runtime_source_map: false,
            style_processing: RuntimeStyleProcessing::Complete,
            has_template: true,
            has_script: true,
            style_count: 1,
        }
    }

    fn client() -> DemandSeed {
        seed(ProductKind::RuntimeClient, TargetKind::ClientTree)
    }

    fn server() -> DemandSeed {
        seed(ProductKind::RuntimeServer, TargetKind::Server)
    }

    fn plan(seeds: &[DemandSeed]) -> Result<TargetPlan, TargetPlanRefusal> {
        plan_targets(
            &close_demand(seeds),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
    }

    /// The ledger counts of planning `seeds`, when capture is compiled in.
    #[cfg(feature = "semantic-observe")]
    fn planning_counts(seeds: &[DemandSeed]) -> [u64; 4] {
        let mut ledger = CompilerWorkLedger::<verter_audit::observe::Captured>::new();
        plan_targets(&close_demand(seeds), &mut ledger).unwrap();
        let snapshot = ledger.snapshot().unwrap();
        [
            WorkKind::PlanningTargetEntries,
            WorkKind::PlanningMultiTargetSharedPrerequisites,
            WorkKind::PlanningEffectNodes,
            WorkKind::PlanningEffectEdges,
        ]
        .map(|kind| snapshot.count(kind))
    }

    #[test]
    fn one_target_shares_nothing_to_count() {
        let mut mapped = client();
        mapped.runtime_source_map = true;
        let plan = plan(&[mapped]).unwrap();
        assert_eq!(
            plan.targets(),
            &[PlannedTarget {
                product: ProductKind::RuntimeClient,
                target: TargetKind::ClientTree,
                planning: Capability::PlanClientTree,
                wants_map: true,
            }]
        );
        assert!(plan.wants(TargetKind::ClientTree));
        assert!(!plan.wants(TargetKind::Server));

        #[cfg(feature = "semantic-observe")]
        {
            let [entries, shared, _, _] = planning_counts(&[mapped]);
            assert_eq!(entries, 1);
            assert_eq!(shared, 0);
        }
    }

    #[test]
    fn client_and_server_share_parse_facts_and_structure() {
        let mut mapped_client = client();
        mapped_client.runtime_source_map = true;
        let seeds = [server(), mapped_client];
        let plan = plan(&seeds).unwrap();

        let targets: Vec<_> = plan
            .targets()
            .iter()
            .map(|planned| (planned.target, planned.planning, planned.wants_map))
            .collect();
        assert_eq!(
            targets,
            [
                (TargetKind::ClientTree, Capability::PlanClientTree, true),
                (TargetKind::Server, Capability::PlanServer, false),
            ]
        );
        let shared = plan.shared();
        for capability in [
            Capability::ParseTemplate,
            Capability::FactsBindings,
            Capability::StructureCompile,
            Capability::StyleScopePlan,
        ] {
            assert!(shared.contains(capability), "{capability:?} is not shared");
        }
        for capability in [
            Capability::PlanClientTree,
            Capability::PlanServer,
            Capability::EmitRender,
            Capability::EmitModule,
            Capability::MapRuntime,
            Capability::PublishArtifacts,
        ] {
            assert!(!shared.contains(capability), "{capability:?} is shared");
        }

        #[cfg(feature = "semantic-observe")]
        {
            let [entries, shared_count, _, _] = planning_counts(&seeds);
            assert_eq!(entries, 2);
            assert!(shared_count >= 3);
            assert_eq!(shared_count, u64::from(shared.count()));
        }
    }

    #[test]
    fn a_server_plan_records_no_effect_work() {
        let plan = plan(&[server()]).unwrap();
        assert_eq!(plan.targets().len(), 1);
        assert!(!plan.shared().contains(Capability::FactsDependencies));

        #[cfg(feature = "semantic-observe")]
        {
            let [entries, _, effect_nodes, effect_edges] = planning_counts(&[server()]);
            assert_eq!(entries, 1);
            assert_eq!(effect_nodes, 0);
            assert_eq!(effect_edges, 0);
        }
    }

    #[test]
    fn an_empty_demand_is_refused() {
        assert_eq!(plan(&[]), Err(TargetPlanRefusal::NoTarget));
    }

    #[test]
    fn a_script_only_component_plans_no_targets() {
        let mut script_only = server();
        script_only.has_template = false;
        let plan = plan(&[script_only]).unwrap();
        assert!(plan.targets().is_empty());
        assert!(!plan.wants(TargetKind::Server));
        assert!(plan.shared().contains(Capability::FactsBindings));
    }

    #[test]
    fn a_target_without_its_planning_is_refused() {
        // Render work is demanded through the client, while the server
        // seed carries no template and so closes no server planning.
        let mut template_less = server();
        template_less.has_template = false;
        assert_eq!(
            plan(&[client(), template_less]),
            Err(TargetPlanRefusal::DemandNotClosed {
                target: TargetKind::Server
            })
        );
    }
}
