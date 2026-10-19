//! The retained-refusal workload, over both typed refusal classes.
//!
//! An operation-profile refusal: a component whose metadata cannot be
//! computed inside a host's projection-operation allowance is delivered
//! with only operation-profile causes, and the host retains it per
//! operation profile, so asking again under the same profile repeats the
//! refusal without resolving anything. A different (larger) profile is a
//! different request: it computes the component in full and heals the
//! answer, and an edit to the component invalidates the retained refusal.
//!
//! An isolated-root refusal: a component whose own declarations nest past
//! the fixed connected query-depth bound is delivered with only the depth
//! cause under the shipped allowance, and an isolated request retains it.
//! The depth bound has no larger profile; a relevant edit invalidates the
//! refusal and a shallow equivalent heals it. A request that already spent
//! before it asked, a different operation profile and a sibling component
//! never inherit the refusal. A missing input is not a refusal: it is never
//! retained, and it heals once supplied.
//!
//! Every action builds its hosts through the public component-meta session
//! route and closes them again. The operation-profile actions build a host
//! with a tight allowance; the larger profile is a per-request context over
//! that same host, so both profiles consult one retained refusal store.

use serde::{Deserialize, Serialize};

use super::fixtures::{
    deep_chain_source, extra_only_source, extra_source, isolation_component_source,
    isolation_types_source, shallow_sibling_source,
};
use super::isolation::{Answer, Rig};

const REFUSER: &str = "refuse.vue";
const DEEP: &str = "deep.vue";
const SIBLING: &str = "sibling.vue";
const EXTRA: &str = "extra.ts";
/// The larger profile: the shipped projection-operation allowance.
const LARGE_PROFILE: usize = 0;
/// A different operation profile than the shipped one, still far above what
/// the depth fixture spends.
const OTHER_PROFILE: usize = 4_000_000;
/// Aliases of the shallow equivalent that heals a depth refusal.
const SHALLOW_LEVELS: u32 = 2;

/// Which refusal workload an action runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefusalVariant {
    /// Operation profile: seal a refusal, repeat it, and compare a cold twin.
    Sealed,
    /// Operation profile: seal a refusal, then edit the component repeatedly
    /// and repeat each edit's refusal.
    Edited,
    /// Operation profile: demand both profiles concurrently from one host.
    Concurrent,
    /// Operation profile: a refusal sealed under the tight profile; the
    /// larger profile heals it.
    Healing,
    /// Isolated root: seal a depth refusal, repeat it, keep a sibling exact
    /// and compare a cold twin.
    DepthSealed,
    /// Isolated root: a deeper edit seals a new refusal; a shallow
    /// equivalent heals it.
    DepthEdited,
    /// Isolated root: a request that spent before asking and a different
    /// operation profile compute rather than inherit the sealed refusal.
    DepthControls,
    /// Isolated root beside a missing input: not retained until the input is
    /// supplied, then the remaining depth refusal is.
    DepthMissingInput,
    /// A missing input alone: never retained, and healed once supplied.
    MissingInput,
}

impl RefusalVariant {
    /// Every variant, in tranche order.
    pub const ALL: [RefusalVariant; 9] = [
        RefusalVariant::Sealed,
        RefusalVariant::Edited,
        RefusalVariant::Concurrent,
        RefusalVariant::Healing,
        RefusalVariant::DepthSealed,
        RefusalVariant::DepthEdited,
        RefusalVariant::DepthControls,
        RefusalVariant::DepthMissingInput,
        RefusalVariant::MissingInput,
    ];

    /// Whether the variant drives the connected query-depth refusal (on the
    /// shipped allowance) rather than the operation-profile one.
    #[must_use]
    pub fn is_depth(self) -> bool {
        matches!(
            self,
            RefusalVariant::DepthSealed
                | RefusalVariant::DepthEdited
                | RefusalVariant::DepthControls
                | RefusalVariant::DepthMissingInput
                | RefusalVariant::MissingInput
        )
    }
}

/// What a demand in the workload was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DemandRole {
    /// The first demand of the refusing component.
    Sealed,
    /// The same demand again.
    Repeat,
    /// A demand after the component was edited.
    Edit,
    /// The edited component demanded again.
    EditRepeat,
    /// The same demand on a second host with no history.
    Oracle,
    /// The same demand under the larger profile on the tight profile's host.
    Healed,
    /// The tight host's demand after the larger profile healed.
    TightAfterHealed,
    /// A tight-host demand issued concurrently with larger-profile ones.
    ConcurrentTight,
    /// A larger-profile demand issued concurrently with tight ones.
    ConcurrentLarge,
    /// A sibling component of the refusing one.
    Sibling,
    /// The demand inside a request that spent before it asked.
    NotIsolated,
    /// The demand under a different operation profile.
    OtherProfile,
    /// The demand after a shallow equivalent replaced the deep component.
    ShallowHealed,
    /// The shallow equivalent demanded again.
    HealedRepeat,
    /// A demand while an imported input is missing.
    Missing,
    /// The demand once the missing input was supplied.
    Supplied,
    /// The supplied demand again.
    SuppliedRepeat,
}

/// One demand and the work its host ran for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Demand {
    pub role: DemandRole,
    pub answer: Answer,
    /// Component-meta resolutions the host ran for it.
    pub recomputes: u64,
    /// Cold semantic producer runs the host made for it.
    pub producer_runs: u64,
}

/// What one refusal action observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefusalEvidence {
    pub variant: RefusalVariant,
    pub demands: Vec<Demand>,
}

impl RefusalEvidence {
    /// The demands of `role`, in order.
    pub fn of(&self, role: DemandRole) -> impl Iterator<Item = &Demand> {
        self.demands.iter().filter(move |d| d.role == role)
    }
}

struct Driver<'a> {
    rig: &'a Rig,
    tranche: u32,
    demands: Vec<Demand>,
}

impl Driver<'_> {
    fn measure(&mut self, role: DemandRole, ask: impl FnOnce(&Rig) -> Answer) -> Answer {
        let (recomputes, producers) = (self.rig.recomputes(), self.rig.producer_runs());
        let answer = ask(self.rig);
        self.demands.push(Demand {
            role,
            answer: answer.clone(),
            recomputes: self.rig.recomputes() - recomputes,
            producer_runs: self.rig.producer_runs() - producers,
        });
        answer
    }

    fn demand(&mut self, role: DemandRole, name: &str) -> Answer {
        let tranche = self.tranche;
        self.measure(role, |rig| rig.demand_of(tranche, name))
    }
}

/// The modules an action of `variant` publishes first, with `props`
/// mapped-type props for an operation-profile refuser and `props` nested
/// aliases for a depth refuser.
#[must_use]
pub fn initial_modules(
    variant: RefusalVariant,
    tranche: u32,
    props: u32,
) -> Vec<(&'static str, String)> {
    let tag = format!("t{tranche:03}");
    match variant {
        RefusalVariant::Sealed
        | RefusalVariant::Edited
        | RefusalVariant::Concurrent
        | RefusalVariant::Healing => vec![
            ("ref.ts", isolation_types_source()),
            (REFUSER, isolation_component_source(&tag, props)),
        ],
        RefusalVariant::DepthSealed
        | RefusalVariant::DepthEdited
        | RefusalVariant::DepthControls => {
            vec![
                (DEEP, deep_chain_source(&tag, props, false)),
                (SIBLING, shallow_sibling_source(&tag)),
            ]
        }
        RefusalVariant::DepthMissingInput => vec![(DEEP, deep_chain_source(&tag, props, true))],
        RefusalVariant::MissingInput => vec![(DEEP, extra_only_source(&tag))],
    }
}

/// Run one refusal action of `tranche`. Returns what it observed and the
/// digests of every text it published, in order.
#[must_use]
pub fn run(
    variant: RefusalVariant,
    op_budget: usize,
    tranche: u32,
    props: u32,
) -> (RefusalEvidence, Vec<String>) {
    let tag = format!("t{tranche:03}");
    let rig = Rig::new(op_budget);
    rig.publish(tranche, &initial_modules(variant, tranche, props));
    let mut d = Driver {
        rig: &rig,
        tranche,
        demands: Vec::new(),
    };
    let mut published_elsewhere = Vec::new();
    match variant {
        RefusalVariant::Sealed => {
            d.demand(DemandRole::Sealed, REFUSER);
            d.demand(DemandRole::Repeat, REFUSER);
            d.demand(DemandRole::Repeat, REFUSER);
            published_elsewhere = oracle(&mut d, variant, op_budget, props, REFUSER);
        }
        RefusalVariant::Edited => {
            d.demand(DemandRole::Sealed, REFUSER);
            for edit in 1..=3 {
                rig.publish(
                    tranche,
                    &[(REFUSER, isolation_component_source(&tag, props + edit))],
                );
                d.demand(DemandRole::Edit, REFUSER);
                d.demand(DemandRole::EditRepeat, REFUSER);
            }
        }
        RefusalVariant::Healing => {
            d.demand(DemandRole::Sealed, REFUSER);
            // The larger profile asks the host that holds the tight
            // profile's retained refusal.
            d.measure(DemandRole::Healed, |rig| {
                rig.demand_under_profile(tranche, REFUSER, LARGE_PROFILE)
            });
            d.demand(DemandRole::TightAfterHealed, REFUSER);
        }
        RefusalVariant::Concurrent => {
            d.demand(DemandRole::Sealed, REFUSER);
            let rig = &rig;
            let answers: Vec<(DemandRole, Answer)> = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..4)
                    .map(|k| {
                        scope.spawn(move || {
                            if k % 2 == 0 {
                                (DemandRole::ConcurrentTight, rig.demand_of(tranche, REFUSER))
                            } else {
                                (
                                    DemandRole::ConcurrentLarge,
                                    rig.demand_under_profile(tranche, REFUSER, LARGE_PROFILE),
                                )
                            }
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().expect("a refusal demand thread panicked"))
                    .collect()
            });
            // Concurrent demands share the counters: their work is not
            // attributed per demand.
            d.demands
                .extend(answers.into_iter().map(|(role, answer)| Demand {
                    role,
                    answer,
                    recomputes: 0,
                    producer_runs: 0,
                }));
        }
        RefusalVariant::DepthSealed => {
            d.demand(DemandRole::Sealed, DEEP);
            d.demand(DemandRole::Repeat, DEEP);
            d.demand(DemandRole::Repeat, DEEP);
            d.demand(DemandRole::Sibling, SIBLING);
            published_elsewhere = oracle(&mut d, variant, op_budget, props, DEEP);
        }
        RefusalVariant::DepthEdited => {
            d.demand(DemandRole::Sealed, DEEP);
            rig.publish(
                tranche,
                &[(DEEP, deep_chain_source(&tag, props + 1, false))],
            );
            d.demand(DemandRole::Edit, DEEP);
            d.demand(DemandRole::EditRepeat, DEEP);
            rig.publish(
                tranche,
                &[(DEEP, deep_chain_source(&tag, SHALLOW_LEVELS, false))],
            );
            d.demand(DemandRole::ShallowHealed, DEEP);
            d.demand(DemandRole::HealedRepeat, DEEP);
        }
        RefusalVariant::DepthControls => {
            d.demand(DemandRole::Sealed, DEEP);
            for _ in 0..2 {
                d.measure(DemandRole::NotIsolated, |rig| {
                    rig.demand_after_spending(tranche, DEEP)
                });
            }
            d.measure(DemandRole::OtherProfile, |rig| {
                rig.demand_under_profile(tranche, DEEP, OTHER_PROFILE)
            });
            d.demand(DemandRole::Sibling, SIBLING);
            d.demand(DemandRole::Repeat, DEEP);
        }
        RefusalVariant::DepthMissingInput | RefusalVariant::MissingInput => {
            d.demand(DemandRole::Missing, DEEP);
            d.demand(DemandRole::Missing, DEEP);
            rig.publish(tranche, &[(EXTRA, extra_source(&tag))]);
            d.demand(DemandRole::Supplied, DEEP);
            d.demand(DemandRole::SuppliedRepeat, DEEP);
        }
    }
    let mut published = rig.published();
    published.append(&mut published_elsewhere);
    (
        RefusalEvidence {
            variant,
            demands: d.demands,
        },
        published,
    )
}

/// The same first demand on a second host with no history. Returns the
/// digests of what that host was published.
fn oracle(
    d: &mut Driver<'_>,
    variant: RefusalVariant,
    op_budget: usize,
    props: u32,
    name: &str,
) -> Vec<String> {
    let twin = Rig::new(op_budget);
    twin.publish(d.tranche, &initial_modules(variant, d.tranche, props));
    let mut twin_driver = Driver {
        rig: &twin,
        tranche: d.tranche,
        demands: Vec::new(),
    };
    twin_driver.demand(DemandRole::Oracle, name);
    d.demands.append(&mut twin_driver.demands);
    twin.published()
}
