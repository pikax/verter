//! Framework-neutral mechanics for target planning: the static executor
//! shape, sparse and dense overlays keyed by a framework's own node
//! identity, a compact relation graph, and the multi-target plan. No
//! framework semantics and no node taxonomy live here.

mod executor;
mod graph;
mod overlay;
mod plan;

pub use executor::{ExecutionClass, TargetExecutor};
pub use graph::{RelationGraph, RelationGraphBuilder};
pub use overlay::{DenseOverlay, SparseOverlay};
pub use plan::{plan_targets, PlannedTarget, SharedPrerequisites, TargetPlan, TargetPlanRefusal};

#[cfg(test)]
mod tests {
    use verter_audit::observe::{ObserveExec, Uncaptured};
    use verter_span::Span;

    use super::*;
    use crate::compile_request::demand::{close_demand, DemandSeed, TargetKind};
    use crate::compile_request::{ProductKind, RuntimeStyleProcessing};
    use crate::framework_common::structure::{Structure, StructureError, TopologyStore};
    use crate::work_ledger::CompilerWorkLedger;

    /// A second framework: its own node identity and node kinds, and none of
    /// another framework's semantics.
    enum ToyNode {}

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ToyKind {
        Element,
        Text,
        Binding,
    }

    type ToyTree<'s> = TopologyStore<'s, ToyNode, (), ToyKind>;

    /// `<p>` holding the text `hi ` and the binding `{name}`.
    fn toy_tree(source: &str) -> ToyTree<'_> {
        let mut tree = Structure::new(source);
        let region = tree.add_region(None, None, ()).unwrap();
        let element = tree
            .push(None, region, Span::new(0, 9), ToyKind::Element)
            .unwrap();
        tree.push(Some(element), region, Span::new(0, 3), ToyKind::Text)
            .unwrap();
        tree.push(Some(element), region, Span::new(3, 9), ToyKind::Binding)
            .unwrap();
        tree.finish()
    }

    /// The toy's server target: renders children before the element that
    /// holds them, each line prefixed with the node's depth.
    struct ToyExecutor;

    #[derive(Debug, PartialEq, Eq)]
    enum ToyRefusal {
        NotPlanned,
        Cycle,
        Structure,
    }

    impl From<StructureError> for ToyRefusal {
        fn from(_: StructureError) -> Self {
            Self::Structure
        }
    }

    impl TargetExecutor for ToyExecutor {
        const TARGET: TargetKind = TargetKind::Server;
        type Inputs<'a> = (&'a TargetPlan, &'a ToyTree<'a>);
        type Output<'a> = String;
        type Refusal = ToyRefusal;

        fn execute<'a, M: ObserveExec>(
            (plan, tree): Self::Inputs<'a>,
            _ledger: &mut CompilerWorkLedger<M>,
        ) -> Result<Self::Output<'a>, Self::Refusal> {
            if !plan.wants(Self::TARGET) {
                return Err(ToyRefusal::NotPlanned);
            }
            let mut depth = DenseOverlay::<ToyNode, u8>::filled(tree.len(), 0)?;
            let mut bindings = SparseOverlay::<ToyNode, &str>::new();
            let mut builder = RelationGraphBuilder::<ToyNode, ToyKind, ()>::new();
            // Added in preorder, so the graph's identities are the tree's.
            for id in tree.node_ids() {
                let kind = *tree.node(id).ok_or(StructureError::UnknownId)?;
                let node = builder.add_node(kind)?;
                if let Some(parent) = tree.parent(id) {
                    depth.set(id, depth.get(parent).unwrap_or(0) + 1);
                    builder.add_edge(node, parent, ())?;
                }
                if kind == ToyKind::Binding {
                    let text = tree.source_text(id).ok_or(StructureError::InvalidSpan)?;
                    bindings.insert(id, text.trim_matches(['{', '}']));
                }
            }
            let order = builder
                .finish()
                .topological_order()
                .ok_or(ToyRefusal::Cycle)?;
            let mut out = String::new();
            for id in order {
                let line = match tree.node(id) {
                    Some(ToyKind::Element) => "</p>",
                    Some(ToyKind::Text) => tree.source_text(id).unwrap_or_default(),
                    Some(ToyKind::Binding) => bindings.get(id).copied().unwrap_or_default(),
                    None => return Err(StructureError::UnknownId.into()),
                };
                out.push_str(&format!("{}:{line}\n", depth.get(id).unwrap_or(0)));
            }
            Ok(out)
        }
    }

    #[test]
    fn a_second_framework_uses_the_mechanics() {
        let server = DemandSeed {
            product: ProductKind::RuntimeServer,
            target: ToyExecutor::TARGET,
            runtime_source_map: false,
            style_processing: RuntimeStyleProcessing::Complete,
            has_template: true,
            has_script: false,
            style_count: 0,
        };
        let mut ledger = CompilerWorkLedger::<Uncaptured>::new();
        let plan = plan_targets(&close_demand(&[server]), &mut ledger).unwrap();
        let tree = toy_tree("hi {name}");

        let rendered = ToyExecutor::execute((&plan, &tree), &mut ledger).unwrap();
        assert_eq!(rendered, "1:hi \n1:name\n0:</p>\n");

        let client = DemandSeed {
            product: ProductKind::RuntimeClient,
            target: TargetKind::ClientTree,
            ..server
        };
        let client_plan = plan_targets(&close_demand(&[client]), &mut ledger).unwrap();
        assert_eq!(
            ToyExecutor::execute((&client_plan, &tree), &mut ledger),
            Err(ToyRefusal::NotPlanned)
        );
    }
}
