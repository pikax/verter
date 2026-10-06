//! Resolution budget domains and the typed failure a budget overrun reports.

/// Structured failure when a budget is exceeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetExceededFailure {
    /// Which budget domain was exceeded.
    pub domain: BudgetDomain,
    /// The budget limit that was hit.
    pub limit: usize,
    /// Actual count at the time of failure.
    pub actual: u64,
    /// Context about what was being resolved when the budget tripped.
    pub context: String,
}

/// Which resolution domain hit its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetDomain {
    LocalClosure,
    Frontier,
    BuilderExpansion,
    ProjectionOperation,
    SolverResolveSteps,
    SolverArenaNodes,
    SolverInstantiationDepth,
    /// The bytes a connected semantic demand reserves before it constructs a
    /// type.
    ConstructionBytes,
}

impl std::fmt::Display for BudgetExceededFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BUDGET_EXCEEDED({:?}): limit={}, actual={}, context={}",
            self.domain, self.limit, self.actual, self.context
        )
    }
}
