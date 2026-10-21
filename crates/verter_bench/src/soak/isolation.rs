//! The component-meta batch isolation workload.
//!
//! A batch payload callback runs while its request is alive, so a metadata
//! demand issued from inside it inherits the operations the batch already
//! spent. That demand must answer as a cold computation under the spent
//! context, never as a replay of the answer a fresh request would have
//! sealed, and the fresh request must keep its own retained answer.
//!
//! The workload runs on three hosts built with a tight projection-operation
//! allowance (the shipped allowance is far above what a bounded fixture
//! spends), so the refusal is reachable through public routes. Each action
//! builds the hosts, publishes its tranche's components to every one, and
//! closes them again: the subject host demands the refuser inside the
//! spender's callback, then in a fresh request twice; the two cold oracle
//! hosts answer the same two contexts, each with no history of the other.
//! A refusal is bound to the context that sealed it, so the spent-context
//! refusal must not reach the fresh request; a complete answer is valid in
//! any context, so the fresh request's retained answer is simply repeated.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use verter_audit::RequestKind;
use verter_session::meta::{MetaProject, MetaSession};
use verter_session::{ComponentMetaDelivery, HostConfig, VerterHost};
use verter_type_engine::request_context::{
    current_request_budget, RequestContext, RequestContextGuard,
};
use verter_type_engine::semantic_query::{FactStatus, PartialReason};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use super::fixtures::{isolation_component_source, isolation_file, isolation_types_source};
use super::manifest::sha256_hex;
use super::run::publication_digest;

/// The typed class of a component-meta delivery, read from the causes its
/// status names. Only the two retained refusal classes are refusals: an
/// absent symbol, a missing input or any other incomplete answer is never
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnswerClass {
    /// Nothing was delivered.
    Absent,
    /// The exact answer.
    Complete,
    /// Only operation-profile causes: the work allowance stopped it.
    OperationProfile,
    /// Only the connected query-depth cause: the fixed depth bound stopped it.
    ConnectedDepth,
    /// A missing input among its causes.
    MissingInput,
    /// Any other incomplete answer.
    OtherIncomplete,
}

impl AnswerClass {
    /// The class of a delivered `status`. The propagation marker names no
    /// cause of its own and is ignored.
    #[must_use]
    pub fn of_status(status: FactStatus) -> Self {
        let reasons = match status {
            FactStatus::Complete => return AnswerClass::Complete,
            FactStatus::Approximate(reasons, _) | FactStatus::Unavailable(reasons) => reasons.get(),
        };
        let (mut operation, mut depth, mut missing, mut other) = (false, false, false, false);
        for reason in reasons.iter() {
            match reason {
                PartialReason::BudgetExceeded
                | PartialReason::OperationBudget
                | PartialReason::ProjectionWorkLimit => operation = true,
                PartialReason::ConnectedQueryDepthLimit => depth = true,
                PartialReason::MissingDependency => missing = true,
                PartialReason::Propagated => {}
                _ => other = true,
            }
        }
        match (operation, depth, missing, other) {
            (_, _, true, _) => AnswerClass::MissingInput,
            (true, false, false, false) => AnswerClass::OperationProfile,
            (false, true, false, false) => AnswerClass::ConnectedDepth,
            _ => AnswerClass::OtherIncomplete,
        }
    }
}

/// What a component-meta demand delivered: its typed class and the digest
/// of what it carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub class: AnswerClass,
    pub digest: String,
}

impl Answer {
    /// The answer of a demand that errored or delivered nothing.
    #[must_use]
    pub fn absent() -> Self {
        Answer {
            class: AnswerClass::Absent,
            digest: "absent".into(),
        }
    }

    /// A delivered answer of one of the two typed refusal classes.
    #[must_use]
    pub fn is_refusal(&self) -> bool {
        matches!(
            self.class,
            AnswerClass::OperationProfile | AnswerClass::ConnectedDepth
        )
    }

    /// A delivered, complete answer.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.class == AnswerClass::Complete
    }

    /// The answer `delivery` carries.
    #[must_use]
    pub fn of(delivery: &ComponentMetaDelivery) -> Self {
        Answer {
            class: AnswerClass::of_status(delivery.status()),
            digest: delivery_digest(delivery),
        }
    }
}

/// What one isolation action observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IsolationEvidence {
    /// The batch payload callback ran.
    pub callback_ran: bool,
    /// Projection operations the batch had spent when the callback began.
    pub ops_before: u64,
    /// The callback's demand ran under the batch's own request budget.
    pub same_budget: bool,
    /// The demand issued inside the callback.
    pub spent: Answer,
    /// The same demand in a fresh request.
    pub isolated: Answer,
    /// The fresh request repeated.
    pub repeat: Answer,
    /// Resolutions the spent-context demand itself ran (not the spender's).
    pub spent_recomputes: u64,
    /// Resolutions the first fresh request ran.
    pub isolated_recomputes: u64,
    /// Resolutions the repeated fresh request ran: none when it is served
    /// from the retained result.
    pub repeat_recomputes: u64,
    /// Cold semantic producer runs of the repeated fresh request.
    pub repeat_producer_runs: u64,
    /// The cold oracle for the spent context.
    pub oracle_spent: Answer,
    /// The cold oracle for the fresh request.
    pub oracle_isolated: Answer,
}

/// The names of the causes `status` carries; empty when complete.
fn reason_names(status: FactStatus) -> Vec<PartialReason> {
    match status {
        FactStatus::Complete => Vec::new(),
        FactStatus::Approximate(reasons, _) | FactStatus::Unavailable(reasons) => {
            reasons.get().iter().collect()
        }
    }
}

/// The digest of a delivery: the causes its status names and the published
/// prop names.
#[must_use]
pub fn delivery_digest(delivery: &ComponentMetaDelivery) -> String {
    let mut text = format!("status={:?}", reason_names(delivery.status()));
    for prop in &delivery.analysis().props {
        text.push('|');
        text.push_str(&prop.name);
    }
    sha256_hex(text.as_bytes())
}

/// A host built with a projection-operation allowance and a batch session
/// over it. Dropping it closes the host.
pub struct Rig {
    host: Arc<VerterHost>,
    project: Arc<MetaProject>,
    session: MetaSession,
    /// Digests of the texts published to the host, in order.
    published: Mutex<Vec<String>>,
}

impl Rig {
    /// A host allowed `op_budget` projection operations (`0` the shipped
    /// allowance).
    #[must_use]
    pub fn new(op_budget: usize) -> Self {
        let workspace: Arc<dyn WorkspaceAccess> =
            Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
        let host = Arc::new(VerterHost::new(
            HostConfig {
                projection_op_budget: op_budget,
                ..HostConfig::default()
            },
            workspace,
        ));
        let project = MetaProject::from_shared_host(Arc::clone(&host));
        let session = project
            .open_session_batch()
            .expect("an isolation batch session");
        Rig {
            host,
            project,
            session,
            published: Mutex::new(Vec::new()),
        }
    }

    /// Publish `(name, text)` modules of `tranche`.
    pub fn publish(&self, tranche: u32, modules: &[(&str, String)]) {
        for (name, text) in modules {
            let file = isolation_file(tranche, name);
            self.published
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(publication_digest(&file, text));
            self.project
                .upsert_base(&file, text)
                .unwrap_or_else(|err| panic!("isolation fixture {file}: {err:?}"));
        }
    }

    fn open(&self, tranche: u32, spend_props: u32, refuse_props: u32) {
        let tag = format!("t{tranche:03}");
        self.publish(
            tranche,
            &[
                ("ref.ts", isolation_types_source()),
                ("spend.vue", isolation_component_source(&tag, spend_props)),
                ("refuse.vue", isolation_component_source(&tag, refuse_props)),
            ],
        );
    }

    /// Demand `name`'s component metadata in a fresh request.
    #[must_use]
    pub fn demand_of(&self, tranche: u32, name: &str) -> Answer {
        self.session
            .get_component_meta(&isolation_file(tranche, name))
            .ok()
            .flatten()
            .map_or_else(Answer::absent, |d| Answer::of(&d))
    }

    /// Demand `name`'s component metadata under its own request context
    /// allowed `op_budget` projection operations (`0` the shipped allowance),
    /// whatever allowance the host was built with: a different operation
    /// profile asked of the same host and the same retained state.
    #[must_use]
    pub fn demand_under_profile(&self, tranche: u32, name: &str, op_budget: usize) -> Answer {
        let file = isolation_file(tranche, name);
        let _request =
            RequestContextGuard::install(RequestContext::with_kind_timing_and_projection_budget(
                0,
                Arc::from(file.as_str()),
                RequestKind::ComponentMeta,
                false,
                false,
                None,
                op_budget,
            ));
        self.demand_of(tranche, name)
    }

    /// Demand `name`'s component metadata inside a request that already
    /// spent a projection operation before it asked: a request that is not
    /// an isolated root.
    #[must_use]
    pub fn demand_after_spending(&self, tranche: u32, name: &str) -> Answer {
        let file = isolation_file(tranche, name);
        let _request =
            RequestContextGuard::install(RequestContext::with_kind_timing_and_projection_budget(
                0,
                Arc::from(file.as_str()),
                RequestKind::ComponentMeta,
                false,
                false,
                None,
                0,
            ));
        if let Some(budget) = current_request_budget() {
            let _ = budget.check_projection_op_count();
        }
        self.demand_of(tranche, name)
    }

    fn demand(&self, tranche: u32) -> Answer {
        self.demand_of(tranche, "refuse.vue")
    }

    /// The refuser, demanded from inside the spender's batch callback.
    /// Returns the callback's observations; `None` when it never ran.
    fn spent_demand(&self, tranche: u32) -> Option<SpentDemand> {
        let seen = Mutex::new(None);
        let _ = self.session.get_component_meta_batch_payloads(
            &[isolation_file(tranche, "spend.vue")],
            |_output| {
                let before = current_request_budget();
                let ops = before
                    .as_ref()
                    .map_or(0, |b| b.projection_ops_executed_count() as u64);
                let recomputes_before = self.recomputes();
                let answer = self.demand(tranche);
                let recomputes = self.recomputes() - recomputes_before;
                let after = current_request_budget();
                let same = matches!(
                    (&before, &after),
                    (Some(a), Some(b)) if Arc::ptr_eq(a, b)
                );
                *seen.lock().unwrap_or_else(|p| p.into_inner()) = Some(SpentDemand {
                    ops,
                    same_budget: same,
                    answer,
                    recomputes,
                });
                Vec::new()
            },
        );
        seen.into_inner().unwrap_or_else(|p| p.into_inner())
    }

    /// Digests of every text published to this host so far, in order.
    #[must_use]
    pub fn published(&self) -> Vec<String> {
        self.published
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Cold semantic producer runs the host has made (resolver node-cache
    /// misses): zero across a demand means nothing was recomputed.
    #[must_use]
    pub fn producer_runs(&self) -> u64 {
        self.host
            .provenance()
            .resolver_node_cache_misses
            .load(Ordering::Relaxed)
    }

    /// Component-meta resolutions the host has run.
    #[must_use]
    pub fn recomputes(&self) -> u64 {
        self.host
            .provenance()
            .component_meta_resolved_state_recomputes
            .load(Ordering::Relaxed)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.host.close();
        // A stopped worker leaves its pins charged; the soak's drain checks
        // judge what remains, so the boundary's error needs no second report.
        let _ = self.host.release_completion().wait();
    }
}

struct SpentDemand {
    ops: u64,
    same_budget: bool,
    answer: Answer,
    recomputes: u64,
}

/// Run one isolation action for `tranche` on three fresh hosts allowed
/// `op_budget` projection operations, and close them. Returns what it
/// observed and the digests of every text it published.
#[must_use]
pub fn run(
    op_budget: usize,
    tranche: u32,
    spend_props: u32,
    refuse_props: u32,
) -> (IsolationEvidence, Vec<String>) {
    let subject = Rig::new(op_budget);
    let oracle_spent = Rig::new(op_budget);
    let oracle_isolated = Rig::new(op_budget);
    for rig in [&subject, &oracle_spent, &oracle_isolated] {
        rig.open(tranche, spend_props, refuse_props);
    }
    let spent = subject.spent_demand(tranche);
    let after_spent = subject.recomputes();
    let isolated = subject.demand(tranche);
    let after_isolated = subject.recomputes();
    let producers_before_repeat = subject.producer_runs();
    let repeat = subject.demand(tranche);
    let after_repeat = subject.recomputes();
    let repeat_producer_runs = subject.producer_runs() - producers_before_repeat;
    let published = [&subject, &oracle_spent, &oracle_isolated]
        .into_iter()
        .flat_map(Rig::published)
        .collect();
    let oracle_spent = oracle_spent.spent_demand(tranche);
    let oracle_isolated = oracle_isolated.demand(tranche);
    let evidence = IsolationEvidence {
        callback_ran: spent.is_some(),
        ops_before: spent.as_ref().map_or(0, |s| s.ops),
        same_budget: spent.as_ref().is_some_and(|s| s.same_budget),
        spent_recomputes: spent.as_ref().map_or(0, |s| s.recomputes),
        spent: spent.map_or_else(Answer::absent, |s| s.answer),
        isolated,
        repeat,
        isolated_recomputes: after_isolated - after_spent,
        repeat_recomputes: after_repeat - after_isolated,
        repeat_producer_runs,
        oracle_spent: oracle_spent.map_or_else(Answer::absent, |s| s.answer),
        oracle_isolated,
    };
    (evidence, published)
}
