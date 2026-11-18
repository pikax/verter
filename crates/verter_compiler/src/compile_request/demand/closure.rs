use verter_identity::encoding::CanonicalEncoder;

use super::{Capability, FactFamilies, TargetKind};
use crate::compile_request::{ProductKind, RuntimeStyleProcessing};

/// One requested product, as the framework resolved it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DemandSeed {
    pub product: ProductKind,
    pub target: TargetKind,
    pub runtime_source_map: bool,
    pub style_processing: RuntimeStyleProcessing,
    /// Whether the source has a template, a script and styles to compile.
    pub has_template: bool,
    pub has_script: bool,
    pub style_count: u32,
}

/// Why a capability is in the closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DemandReason {
    Product(u8),
    Capability(Capability),
}

/// One reason edge: `from` requires `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DemandEdge {
    pub from: DemandReason,
    pub to: Capability,
}

/// The closed capability set of one request. Immutable once built; nothing
/// may add to it after target execution starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemandSet {
    bits: u32,
    targets: Vec<(ProductKind, TargetKind)>,
    edges: Vec<DemandEdge>,
}

/// Domain of [`DemandSet::canonical_bytes`]. A change to the encoding is a
/// new domain.
const DEMAND_CLOSURE_DOMAIN: &str = "verter.compiler.demand-closure.v1";

impl DemandSet {
    pub fn contains(&self, capability: Capability) -> bool {
        self.bits & capability.bit() != 0
    }
    /// Capabilities in declaration order.
    pub fn capabilities(&self) -> impl Iterator<Item = Capability> + '_ {
        Capability::ALL.into_iter().filter(|c| self.contains(*c))
    }
    /// Requested `(product, target)` pairs, one per product, in product
    /// declaration order (the order the products were requested in does
    /// not change the set).
    pub fn targets(&self) -> &[(ProductKind, TargetKind)] {
        &self.targets
    }
    pub fn wants_target(&self, target: TargetKind) -> bool {
        self.targets.iter().any(|(_, wanted)| *wanted == target)
    }
    /// Fact families the closure demands of the semantic authority.
    pub fn fact_families(&self) -> FactFamilies {
        self.capabilities()
            .filter_map(Capability::fact_family)
            .fold(FactFamilies::empty(), FactFamilies::with)
    }
    /// Sorted, deduplicated reason edges.
    pub fn edges(&self) -> &[DemandEdge] {
        &self.edges
    }
    /// Canonical digest input for the structure stage key: the sorted
    /// capability names, then the sorted targets.
    ///
    /// [`Capability::MapRuntime`] is left out. Map demand is emit-class: it
    /// enters the emit key through the map mode, so asking for a map must
    /// not re-key the structure and the target plans.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::new(DEMAND_CLOSURE_DOMAIN);
        encoder.field_sorted_set(
            1,
            self.capabilities()
                .filter(|capability| *capability != Capability::MapRuntime)
                .map(Capability::name),
        );
        encoder.field_sorted_set(
            2,
            self.targets.iter().map(|(product, target)| {
                let mut element = String::with_capacity(32);
                element.push_str(product.wire_tag());
                element.push(':');
                element.push_str(target.name());
                element
            }),
        );
        encoder.finish()
    }
}

/// Whether the edge into `to` holds for this seed: work on a region the
/// source does not have is never demanded, and only the seed's own target
/// is planned.
fn applies(seed: &DemandSeed, to: Capability) -> bool {
    use Capability::*;
    match to {
        ParseTemplate | ParseExpressions | FactsTemplate | StructureTopology | StructureCompile
        | EmitRender => seed.has_template,
        PlanClientTree | PlanClientFineGrained | PlanServer => {
            seed.has_template && to == Capability::planning(seed.target)
        }
        ParseScript => seed.has_script,
        ParseStyle | StyleScopePlan | StyleRewrite => seed.style_count > 0,
        ParseCarrier | FactsBindings | FactsImports | FactsMacros | FactsReactivity
        | FactsDependencies | EmitModule | MapRuntime | PublishArtifacts => true,
    }
}

/// The capabilities a product asks for directly.
fn roots(seed: &DemandSeed) -> impl Iterator<Item = Capability> {
    let rewrite = seed.style_processing == RuntimeStyleProcessing::Complete;
    [
        Some(Capability::PublishArtifacts),
        seed.runtime_source_map.then_some(Capability::MapRuntime),
        rewrite.then_some(Capability::StyleRewrite),
    ]
    .into_iter()
    .flatten()
}

/// Close the demand of a non-empty seed list. Deterministic: equal seeds in
/// any order give an equal set.
pub fn close_demand(seeds: &[DemandSeed]) -> DemandSet {
    let mut bits = 0u32;
    let mut edges = Vec::new();
    let mut targets = Vec::with_capacity(seeds.len());
    let mut worklist = Vec::with_capacity(Capability::ALL.len());
    for seed in seeds {
        targets.push((seed.product, seed.target));
        let product = DemandReason::Product(seed.product as u8);
        let mut reached = 0u32;
        for root in roots(seed).filter(|root| applies(seed, *root)) {
            edges.push(DemandEdge {
                from: product,
                to: root,
            });
            if reached & root.bit() == 0 {
                reached |= root.bit();
                worklist.push(root);
            }
        }
        while let Some(capability) = worklist.pop() {
            for &required in capability.requires() {
                if !applies(seed, required) {
                    continue;
                }
                edges.push(DemandEdge {
                    from: DemandReason::Capability(capability),
                    to: required,
                });
                if reached & required.bit() == 0 {
                    reached |= required.bit();
                    worklist.push(required);
                }
            }
        }
        bits |= reached;
    }
    targets.sort_by_key(|(product, target)| (*product as u8, *target));
    targets.dedup();
    edges.sort_unstable();
    edges.dedup();
    DemandSet {
        bits,
        targets,
        edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile_request::demand::FactFamily;

    fn seed(product: ProductKind, target: TargetKind) -> DemandSeed {
        DemandSeed {
            product,
            target,
            runtime_source_map: false,
            style_processing: RuntimeStyleProcessing::Complete,
            has_template: true,
            has_script: true,
            style_count: 0,
        }
    }

    fn client(target: TargetKind) -> DemandSeed {
        seed(ProductKind::RuntimeClient, target)
    }

    fn server() -> DemandSeed {
        seed(ProductKind::RuntimeServer, TargetKind::Server)
    }

    fn assert_contains(demand: &DemandSet, wanted: &[Capability]) {
        for capability in wanted {
            assert!(demand.contains(*capability), "missing {capability:?}");
        }
    }

    fn assert_lacks(demand: &DemandSet, unwanted: &[Capability]) {
        for capability in unwanted {
            assert!(!demand.contains(*capability), "unexpected {capability:?}");
        }
    }

    #[test]
    fn server_demand_has_no_client_planning() {
        let demand = close_demand(&[server()]);
        assert_contains(
            &demand,
            &[
                Capability::PlanServer,
                Capability::StructureCompile,
                Capability::FactsTemplate,
                Capability::FactsBindings,
            ],
        );
        assert_lacks(
            &demand,
            &[
                Capability::PlanClientTree,
                Capability::PlanClientFineGrained,
                Capability::FactsDependencies,
                Capability::StyleScopePlan,
                Capability::MapRuntime,
            ],
        );
        assert_eq!(
            demand.targets(),
            &[(ProductKind::RuntimeServer, TargetKind::Server)]
        );
        assert!(demand.wants_target(TargetKind::Server));
        assert!(!demand.wants_target(TargetKind::ClientTree));
    }

    #[test]
    fn client_tree_demand_has_reactivity_not_dependencies() {
        let demand = close_demand(&[client(TargetKind::ClientTree)]);
        assert_contains(
            &demand,
            &[
                Capability::PlanClientTree,
                Capability::FactsReactivity,
                Capability::FactsImports,
            ],
        );
        assert_lacks(
            &demand,
            &[
                Capability::FactsDependencies,
                Capability::PlanClientFineGrained,
                Capability::PlanServer,
            ],
        );
    }

    #[test]
    fn fine_grained_demand_has_dependencies() {
        let demand = close_demand(&[client(TargetKind::ClientFineGrained)]);
        assert_contains(
            &demand,
            &[
                Capability::PlanClientFineGrained,
                Capability::FactsDependencies,
                Capability::FactsReactivity,
            ],
        );
        assert_lacks(
            &demand,
            &[Capability::PlanClientTree, Capability::PlanServer],
        );
    }

    #[test]
    fn a_template_less_component_demands_no_template_work() {
        for target in [
            TargetKind::ClientTree,
            TargetKind::ClientFineGrained,
            TargetKind::Server,
        ] {
            let mut template_less = client(target);
            template_less.has_template = false;
            let demand = close_demand(&[template_less]);
            assert_lacks(
                &demand,
                &[
                    Capability::ParseTemplate,
                    Capability::ParseExpressions,
                    Capability::FactsTemplate,
                    Capability::StructureTopology,
                    Capability::StructureCompile,
                    Capability::EmitRender,
                ],
            );
            assert_contains(
                &demand,
                &[
                    Capability::EmitModule,
                    Capability::ParseScript,
                    Capability::FactsBindings,
                ],
            );
        }
    }

    #[test]
    fn a_script_less_component_parses_no_script() {
        let mut script_less = client(TargetKind::ClientTree);
        script_less.has_script = false;
        let demand = close_demand(&[script_less]);
        assert_lacks(&demand, &[Capability::ParseScript]);
        assert_contains(
            &demand,
            &[Capability::FactsBindings, Capability::ParseTemplate],
        );
    }

    #[test]
    fn a_map_is_demanded_only_when_requested() {
        let without = close_demand(&[client(TargetKind::ClientTree)]);
        assert!(!without.contains(Capability::MapRuntime));

        let mut mapped = client(TargetKind::ClientTree);
        mapped.runtime_source_map = true;
        let with = close_demand(&[mapped]);
        assert!(with.contains(Capability::MapRuntime));
        assert!(with.edges().contains(&DemandEdge {
            from: DemandReason::Product(ProductKind::RuntimeClient as u8),
            to: Capability::MapRuntime,
        }));
        // Map demand is emit-class: it does not change the structure digest.
        assert_eq!(with.canonical_bytes(), without.canonical_bytes());
    }

    #[test]
    fn style_rewrite_follows_style_processing() {
        let mut styled = client(TargetKind::ClientTree);
        styled.style_count = 1;
        let complete = close_demand(&[styled]);
        assert_contains(
            &complete,
            &[
                Capability::StyleRewrite,
                Capability::StyleScopePlan,
                Capability::ParseStyle,
            ],
        );

        styled.style_processing = RuntimeStyleProcessing::AuthoredOnly;
        let authored_only = close_demand(&[styled]);
        assert_lacks(&authored_only, &[Capability::StyleRewrite]);
        assert_contains(&authored_only, &[Capability::StyleScopePlan]);

        let unstyled = close_demand(&[client(TargetKind::ClientTree)]);
        assert_lacks(
            &unstyled,
            &[
                Capability::StyleRewrite,
                Capability::StyleScopePlan,
                Capability::ParseStyle,
            ],
        );
    }

    #[test]
    fn server_planning_scopes_styles_when_present() {
        let mut styled = server();
        styled.style_count = 2;
        let demand = close_demand(&[styled]);
        assert!(demand.edges().contains(&DemandEdge {
            from: DemandReason::Capability(Capability::PlanServer),
            to: Capability::StyleScopePlan,
        }));
    }

    #[test]
    fn closure_is_order_independent() {
        let mut client_seed = client(TargetKind::ClientTree);
        client_seed.runtime_source_map = true;
        client_seed.style_count = 1;
        let mut server_seed = server();
        server_seed.style_count = 1;

        let forward = close_demand(&[client_seed, server_seed]);
        let backward = close_demand(&[server_seed, client_seed]);
        assert_eq!(forward, backward);
        assert_eq!(forward.canonical_bytes(), backward.canonical_bytes());
        assert_eq!(
            forward.targets(),
            &[
                (ProductKind::RuntimeClient, TargetKind::ClientTree),
                (ProductKind::RuntimeServer, TargetKind::Server),
            ]
        );
        let mut sorted = forward.edges().to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, forward.edges());
    }

    #[test]
    fn every_capability_has_a_reason_edge() {
        let mut styled = client(TargetKind::ClientFineGrained);
        styled.style_count = 1;
        styled.runtime_source_map = true;
        for seeds in [
            vec![server()],
            vec![client(TargetKind::ClientTree)],
            vec![styled, server()],
        ] {
            let demand = close_demand(&seeds);
            for capability in demand.capabilities() {
                assert!(
                    demand.edges().iter().any(|edge| edge.to == capability),
                    "{capability:?} has no reason edge"
                );
            }
            for edge in demand.edges() {
                assert!(demand.contains(edge.to));
                if let DemandReason::Capability(from) = edge.from {
                    assert!(demand.contains(from));
                    assert!(from.requires().contains(&edge.to));
                }
            }
        }
    }

    #[test]
    fn fact_families_follow_capabilities() {
        let server_families = close_demand(&[server()]).fact_families();
        assert!(server_families.contains(FactFamily::Bindings));
        assert!(server_families.contains(FactFamily::Macros));
        assert!(server_families.contains(FactFamily::Template));
        assert!(!server_families.contains(FactFamily::Reactivity));
        assert!(!server_families.contains(FactFamily::Imports));
        assert!(!server_families.contains(FactFamily::Dependencies));

        let fine_grained = close_demand(&[client(TargetKind::ClientFineGrained)]).fact_families();
        assert_eq!(fine_grained.count(), 6);

        for seeds in [
            vec![server()],
            vec![client(TargetKind::ClientTree)],
            vec![client(TargetKind::ClientFineGrained), server()],
        ] {
            let demand = close_demand(&seeds);
            let families = demand.fact_families();
            for capability in Capability::ALL {
                if let Some(family) = capability.fact_family() {
                    assert_eq!(families.contains(family), demand.contains(capability));
                }
            }
        }
    }

    #[test]
    fn an_empty_seed_list_closes_to_nothing() {
        let demand = close_demand(&[]);
        assert_eq!(demand.capabilities().count(), 0);
        assert!(demand.targets().is_empty());
        assert!(demand.edges().is_empty());
        assert!(demand.fact_families().is_empty());
    }
}
