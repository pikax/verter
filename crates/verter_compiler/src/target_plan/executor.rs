use verter_audit::observe::ObserveExec;

use crate::compile_request::demand::TargetKind;
use crate::work_ledger::CompilerWorkLedger;

/// Where a logical compiler operation runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExecutionClass {
    /// Computed inside an existing typed visit of one node.
    LocalSynthesized,
    /// Computed once per region.
    Regional,
    /// Needs a whole-table or graph pass before targets run.
    BarrierGraph,
    /// Target-specific planning state.
    TargetPlanning,
    Emission,
    /// Materialization only the final artifact needs.
    TerminalMaterialization,
}

/// One framework's compiler for one target. Selected once per request,
/// outside any node loop; called statically. `Sized` keeps it off trait
/// objects, so no loop can dispatch per node through `dyn TargetExecutor`.
pub trait TargetExecutor: Sized {
    const TARGET: TargetKind;
    type Inputs<'a>
    where
        Self: 'a;
    type Output<'a>
    where
        Self: 'a;
    type Refusal;

    fn execute<'a, M: ObserveExec>(
        inputs: Self::Inputs<'a>,
        ledger: &mut CompilerWorkLedger<M>,
    ) -> Result<Self::Output<'a>, Self::Refusal>;
}
