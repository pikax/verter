//! Ownership of the cost model's retained state: receipts are charged to
//! the retention account for exactly as long as they live, and budget
//! profiles live exactly as long as something names them.

use std::sync::Arc;

use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};

use super::*;

fn identity(name: &str) -> CostIdentity {
    CostIdentity::new(name.as_bytes().to_vec())
}

fn receipt(name: &str, prerequisites: &[&Arc<DemandCostReceipt>]) -> Arc<DemandCostReceipt> {
    DemandCostReceipt::new(
        identity(name),
        LogicalUsage::default(),
        prerequisites
            .iter()
            .map(|receipt| CostDependency {
                receipt: Arc::clone(receipt),
                nesting: Nesting::Entered,
            })
            .collect(),
    )
}

fn account(aggregate_ceiling_bytes: usize) -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes,
        ..RetentionLimits::defaults()
    })
}

fn retained(account: &SemanticRetentionAccount) -> usize {
    account.snapshot().retained_bytes
}

/// A retained receipt is charged once with its whole closure, stays
/// charged while a surviving consumer's receipt keeps a prerequisite alive
/// after its own retainer is gone, and frees with its last owner.
#[test]
fn a_retained_receipt_closure_is_charged_for_as_long_as_it_lives() {
    let account = account(RetentionLimits::DEFAULT_AGGREGATE_CEILING_BYTES);
    let child = receipt("child", &[]);
    child.reserve_retention(&account).expect("room to retain");
    let child_bytes = retained(&account);
    assert!(child_bytes > 0, "a retained receipt is charged");

    let parent = receipt("parent", &[&child]);
    parent.reserve_retention(&account).expect("room to retain");
    let both = retained(&account);
    assert!(both > child_bytes, "the parent is charged once more");
    parent
        .reserve_retention(&account)
        .expect("already retained");
    assert_eq!(retained(&account), both, "a receipt is charged once");

    // The child's own retainer goes; the parent keeps it alive and charged.
    drop(child);
    assert_eq!(retained(&account), both);
    drop(parent);
    assert_eq!(
        retained(&account),
        0,
        "the closure frees with its last owner"
    );
}

/// An account with no room refuses to retain a receipt, and nothing about
/// the refused receipt stays charged.
#[test]
fn a_receipt_the_account_cannot_hold_is_not_retained() {
    let account = account(0);
    let child = receipt("child", &[]);
    let parent = receipt("parent", &[&child]);
    assert!(parent.reserve_retention(&account).is_err());
    assert_eq!(retained(&account), 0);
}

/// Equal allowances share one live profile; once no holder is left the
/// interner keeps nothing for them, and interning again mints a fresh one.
#[test]
fn a_budget_profile_lives_as_long_as_its_holders() {
    let spec = BudgetProfileSpec {
        work: 7_919,
        bytes: 104_729,
        query_depth: 3,
        instantiation_depth: 5,
        tail_steps: 11,
        request_operations: 13,
        relation_comparisons: 17,
        cost_model_revision: COST_MODEL_REVISION,
    };
    assert!(!BudgetProfile::is_interned(&spec));
    let first = BudgetProfile::intern(spec);
    let second = BudgetProfile::intern(spec);
    assert_eq!(first, second, "equal allowances are one identity");
    assert!(BudgetProfile::is_interned(&spec));
    drop(first);
    assert!(BudgetProfile::is_interned(&spec), "a holder remains");
    drop(second);
    assert!(
        !BudgetProfile::is_interned(&spec),
        "the interner keeps no profile nothing names"
    );
    let again = BudgetProfile::intern(spec);
    assert!(BudgetProfile::is_interned(&spec));
    assert_eq!(again.spec(), &spec);
}
