//! The frozen soak workload: a deterministic, hash-identified action list.
//!
//! A manifest is `tranches` repetitions of one 100-action tranche template.
//! Every tranche opens its own two modules, drives every lifecycle class of
//! its mode against them and removes them again, so a tranche boundary is a
//! drained state and each tranche demands its own query identities. The
//! full manifest (100 tranches, 10,000 actions) is hash-frozen in
//! `scripts/benchmark/evidence-runs/rev11-l1-soak.json`; the smoke manifest
//! is its first tranche.

use serde::{Deserialize, Serialize};

use super::refusal::RefusalVariant;

/// Actions in one tranche.
pub const ACTIONS_PER_TRANCHE: u32 = 100;
/// Tranches of the full manifest.
pub const FULL_TRANCHES: u32 = 100;
/// Cold-demanded aliases each tranche module declares.
pub const COLD_ALIASES: u32 = 51;
/// Threads one overlapping cold demand issues at once.
pub const OVERLAP_THREADS: u32 = 4;
/// Edits one candidate-churn action publishes against one identity: more
/// than the retained candidates of every family it touches can hold at once,
/// so the churn only completes by evicting.
pub const CHURN_EDITS: u32 = 24;
/// Members of a pressure-mode large union.
pub const LARGE_UNION_MEMBERS: u32 = 1_500;
/// Tranches between pressure surges: tranche `SURGE_PERIOD - 1`, and every
/// `SURGE_PERIOD`th after it, runs a surge.
pub const SURGE_PERIOD: u32 = 25;
/// Components a surge holds the component-meta results of at once. With
/// [`SURGE_PROPS`] props each, their retained results cross the retention
/// account's aggregate ceiling.
pub const SURGE_MODULES: u32 = 300;
/// Props of each surge component: its retained result stays below the
/// account's per-entry cap.
pub const SURGE_PROPS: u32 = 1_000;
/// Props of the surge's first component: its retained result is above the
/// account's per-entry cap.
pub const OVERSIZED_PROPS: u32 = 1_600;
/// Components of a bounded surge: the first oversized, the rest ordinary.
/// Every tranche at a multiple of [`SURGE_PERIOD`] (the smoke tranche among
/// them) runs one, so every pressure action class runs on any worker; only
/// the full surges are judged for their account outcomes.
pub const BOUNDED_SURGE_MODULES: u32 = 3;
/// Props of every bounded-surge component: a bounded instance drives the
/// surge route on any worker without the full surge's volume.
pub const BOUNDED_SURGE_PROPS: u32 = 200;
/// Nested aliases of a depth-refusal component: past the fixed connected
/// query-depth bound.
pub const DEPTH_LEVELS: u32 = 30;
/// The projection-operation allowance of the isolation hosts.
pub const ISOLATION_OP_BUDGET: usize = 100;
/// Mapped-type props of the isolation spender component.
pub const ISOLATION_SPEND_PROPS: u32 = 16;
/// Mapped-type props of the isolation refuser: it fits the allowance alone
/// and exceeds it once the spender's operations are spent.
pub const ISOLATION_REFUSE_PROPS: u32 = 40;
/// The projection-operation allowance of the refusal hosts.
pub const REFUSAL_OP_BUDGET: usize = 100;
/// Mapped-type props of the refusal component: more than the refusal
/// allowance covers on their own.
pub const REFUSAL_PROPS: u32 = 52;
/// Sources one publication-gate action holds gates on at once: enough that
/// the gate registry grows past its retained floor and must shrink back.
pub const GATE_SOURCES: usize = 128;
/// The canonical root the soak project lives at inside the host.
pub const PROJECT_ROOT: &str = "/soak";

/// Which workload a manifest drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Ordinary editor churn.
    Normal,
    /// The same churn with large retained answers and same-identity
    /// candidate churn in place of four ordinary demands.
    Pressure,
}

impl Mode {
    /// Both modes, in manifest order.
    pub const ALL: [Mode; 2] = [Mode::Normal, Mode::Pressure];

    /// The manifest spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Pressure => "pressure",
        }
    }
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "normal" => Ok(Mode::Normal),
            "pressure" => Ok(Mode::Pressure),
            other => Err(format!("unknown mode {other}; modes: normal, pressure")),
        }
    }
}

/// Every lifecycle class a tranche drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifecycleClass {
    /// Upsert a new module (its first version).
    Open,
    /// The first demand of a query identity.
    ColdDemand,
    /// A repeated demand; answers the cold answer.
    WarmDemand,
    /// Several threads demand one fresh identity at once.
    OverlappingCold,
    /// Warm and cold identities demanded concurrently.
    MixedConcurrent,
    /// The runner keeps an answer's root (an external pin).
    HoldPin,
    /// A public copy of a held root.
    PublicCopy,
    /// Release the copy, then the original (final reader is the original).
    ReleaseCopyThenOriginal,
    /// Release the original, then the copy (final reader is the copy).
    ReleaseOriginalThenCopy,
    /// Publish a new version of an open module.
    Edit,
    /// Demand an identity of an edited module; answers differently.
    PostEditDemand,
    /// Restore a module's original source; its answer returns.
    RevertSource,
    /// A pre-cancelled request answers cancelled; a retry answers.
    Cancellation,
    /// Reconfigure the project; the sentinel answer moves.
    ConfigChange,
    /// Restore the original configuration; the sentinel answer returns.
    ConfigRestore,
    /// Open a uniquely owned module, demand it and remove it.
    UniqueOwnerChurn,
    /// Repeat a budget refusal the host retained, across edits and
    /// concurrent profiles.
    RefusalRepeat,
    /// A narrower-profile demand, then a larger profile heals it.
    ProfileHealing,
    /// LSP publication gates: acquisition, waiting, cancellation and the
    /// final participant's release.
    PublicationGates,
    /// Remove a module.
    Close,
    /// Ask the store to reclaim retired versions.
    Reclaim,
    /// Pressure: demand a large union.
    LargeDemand,
    /// Component-meta batch lifecycle: a demand issued from inside a batch
    /// payload callback after the batch spent operations, against the same
    /// demand in a fresh request.
    MetaIsolation,
    /// Pressure: hold the component-meta results of many large components
    /// at once, past the account's aggregate ceiling and its entry cap.
    PressureSurge,
    /// Pressure: publish many versions of one identity, demanding each.
    CandidateChurn,
}

impl LifecycleClass {
    /// The classes every manifest of `mode` must drive.
    #[must_use]
    pub fn required(mode: Mode) -> Vec<LifecycleClass> {
        use LifecycleClass::*;
        let mut classes = vec![
            Open,
            ColdDemand,
            WarmDemand,
            OverlappingCold,
            MixedConcurrent,
            HoldPin,
            PublicCopy,
            ReleaseCopyThenOriginal,
            ReleaseOriginalThenCopy,
            Edit,
            PostEditDemand,
            RevertSource,
            Cancellation,
            ConfigChange,
            ConfigRestore,
            UniqueOwnerChurn,
            RefusalRepeat,
            ProfileHealing,
            Close,
            Reclaim,
            MetaIsolation,
            PublicationGates,
        ];
        if mode == Mode::Pressure {
            classes.extend([LargeDemand, CandidateChurn, PressureSurge]);
        }
        classes
    }
}

/// One project configuration the config actions switch between.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigVariant {
    /// The original configuration: `@pick/*` maps to `cfg/a/*`.
    Original,
    /// The alternate configuration: `@pick/*` maps to `cfg/b/*`.
    Alternate,
}

/// What an action acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Target {
    /// A module and the revision of its text to publish.
    File { file: String, revision: u32 },
    /// One query identity.
    Query { file: String, alias: String },
    /// Several query identities, demanded concurrently.
    Queries { queries: Vec<(String, String)> },
    /// One query identity, demanded by `threads` threads at once.
    Overlap {
        file: String,
        alias: String,
        threads: u32,
    },
    /// A pin slot: hold `alias`'s answer in it.
    Hold {
        slot: u32,
        file: String,
        alias: String,
    },
    /// A pin slot.
    Slot { slot: u32 },
    /// Release a pin slot's two handles after retiring the cache entries of
    /// `file`, then republish `file` at `revision`.
    Release {
        slot: u32,
        file: String,
        revision: u32,
    },
    /// A function whose return is requested.
    Function { file: String, symbol: String },
    /// A configuration to install.
    Config { variant: ConfigVariant },
    /// Repeated edits of `file`, demanding `alias` after each.
    Churn {
        file: String,
        alias: String,
        edits: u32,
    },
    /// `modules` surge components of the tranche, demanded and held together;
    /// the first has `oversized_props` props, the rest `props`.
    Surge {
        modules: u32,
        props: u32,
        oversized_props: u32,
    },
    /// The tranche's isolation components, on hosts allowed `op_budget`
    /// projection operations.
    Isolation {
        op_budget: usize,
        spend_props: u32,
        refuse_props: u32,
    },
    /// A component whose metadata exceeds `op_budget` projection operations
    /// on its own, driven through `variant`.
    Refusal {
        variant: RefusalVariant,
        op_budget: usize,
        props: u32,
    },
    /// `sources` carrier sources whose membership gates the action drives.
    Gates { sources: usize },
    /// Nothing (store maintenance).
    Store,
}

/// One action of the workload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub index: u32,
    pub tranche: u32,
    pub class: LifecycleClass,
    pub target: Target,
}

/// A generated workload.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub mode: Mode,
    pub tranches: u32,
    pub actions: Vec<Action>,
}

impl Manifest {
    /// The deterministic workload of `tranches` tranches in `mode`.
    #[must_use]
    pub fn generate(mode: Mode, tranches: u32) -> Self {
        let mut actions = Vec::with_capacity((tranches * ACTIONS_PER_TRANCHE) as usize);
        for tranche in 0..tranches {
            for (class, target) in tranche_template(mode, tranche) {
                actions.push(Action {
                    index: actions.len() as u32,
                    tranche,
                    class,
                    target,
                });
            }
        }
        Manifest {
            mode,
            tranches,
            actions,
        }
    }

    /// SHA-256 of the canonical JSON of the action list, of the setup's
    /// publications and of every publication of every action (the digest of
    /// its path and exact bytes, see
    /// [`super::fixtures::expected_publications`]), so a change to any
    /// published fixture moves the identity of the workload it drives.
    #[must_use]
    pub fn digest(&self) -> String {
        self.digest_over(&super::fixtures::setup_publications(), |action| {
            super::fixtures::expected_publications(self.mode, action)
        })
    }

    /// The digest of the action list with `setup` and each action's
    /// `publications` in place of the frozen fixtures: the composition
    /// [`Self::digest`] applies to the real ones.
    #[must_use]
    pub fn digest_over(
        &self,
        setup: &[String],
        publications: impl Fn(&Action) -> Vec<String>,
    ) -> String {
        let mut bytes = serde_json::to_vec(&self.actions).expect("actions serialise");
        for publication in setup {
            bytes.extend_from_slice(publication.as_bytes());
        }
        for action in &self.actions {
            bytes.extend_from_slice(&action.index.to_le_bytes());
            for publication in publications(action) {
                bytes.extend_from_slice(publication.as_bytes());
            }
        }
        sha256_hex(&bytes)
    }

    /// Distinct `(file, alias)` identities the workload demands.
    #[must_use]
    pub fn distinct_queries(&self) -> usize {
        let mut seen = std::collections::BTreeSet::new();
        for action in &self.actions {
            match &action.target {
                Target::Query { file, alias }
                | Target::Overlap { file, alias, .. }
                | Target::Hold { file, alias, .. }
                | Target::Churn { file, alias, .. } => {
                    seen.insert((file.clone(), alias.clone()));
                }
                Target::Queries { queries } => seen.extend(queries.iter().cloned()),
                _ => {}
            }
        }
        seen.len()
    }
}

/// The host path of a soak project file.
#[must_use]
pub fn canonical(file: &str) -> String {
    format!("{PROJECT_ROOT}/{file}")
}

/// The module a tranche demands from.
#[must_use]
pub fn module_file(tranche: u32) -> String {
    format!("t{tranche:03}/mod.ts")
}

/// The module the tranche module imports.
#[must_use]
pub fn dep_file(tranche: u32) -> String {
    format!("t{tranche:03}/dep.ts")
}

/// A surge component.
#[must_use]
pub fn surge_file(tranche: u32, ordinal: u32) -> String {
    format!("t{tranche:03}/surge{ordinal}.vue")
}

/// The module holding a pressure tranche's large union.
#[must_use]
pub fn big_file(tranche: u32) -> String {
    format!("t{tranche:03}/big.ts")
}

/// A uniquely owned churn module.
#[must_use]
pub fn churn_file(tranche: u32, ordinal: u32) -> String {
    format!("t{tranche:03}/churn{ordinal}.ts")
}

fn tranche_template(mode: Mode, tranche: u32) -> Vec<(LifecycleClass, Target)> {
    use LifecycleClass::*;
    let module = module_file(tranche);
    let dep = dep_file(tranche);
    let file = |revision| Target::File {
        file: module.clone(),
        revision,
    };
    let query = |alias: String| Target::Query {
        file: module.clone(),
        alias,
    };
    let a = |i: u32| format!("A{i}");
    let mut out = vec![
        (
            Open,
            Target::File {
                file: dep.clone(),
                revision: 0,
            },
        ),
        (Open, file(0)),
    ];
    out.extend((0..COLD_ALIASES).map(|i| (ColdDemand, query(a(i)))));
    out.extend((0..8).map(|i| (WarmDemand, query(a(i)))));
    for o in 0..2 {
        out.push((
            OverlappingCold,
            Target::Overlap {
                file: module.clone(),
                alias: format!("O{o}"),
                threads: OVERLAP_THREADS,
            },
        ));
    }
    out.push((
        MixedConcurrent,
        Target::Queries {
            queries: [a(8), a(9), "M0".into(), "M1".into()]
                .into_iter()
                .map(|alias| (module.clone(), alias))
                .collect(),
        },
    ));
    let hold = |slot, alias| Target::Hold {
        slot,
        file: module.clone(),
        alias,
    };
    out.extend([
        (HoldPin, hold(0, a(10))),
        (PublicCopy, Target::Slot { slot: 0 }),
        (
            ReleaseCopyThenOriginal,
            Target::Release {
                slot: 0,
                file: module.clone(),
                revision: 0,
            },
        ),
        (HoldPin, hold(1, a(11))),
        (PublicCopy, Target::Slot { slot: 1 }),
        (Edit, file(1)),
    ]);
    out.extend((0..3).map(|i| (PostEditDemand, query(a(i)))));
    out.extend([
        (
            ReleaseOriginalThenCopy,
            Target::Release {
                slot: 1,
                file: module.clone(),
                revision: 1,
            },
        ),
        (RevertSource, file(0)),
    ]);
    out.extend((0..2).map(|f| {
        (
            Cancellation,
            Target::Function {
                file: module.clone(),
                symbol: format!("f{f}"),
            },
        )
    }));
    out.extend([
        (
            ConfigChange,
            Target::Config {
                variant: ConfigVariant::Alternate,
            },
        ),
        (
            ConfigRestore,
            Target::Config {
                variant: ConfigVariant::Original,
            },
        ),
    ]);
    out.extend((0..2).map(|k| {
        (
            UniqueOwnerChurn,
            Target::Query {
                file: churn_file(tranche, k),
                alias: "C".into(),
            },
        )
    }));
    // Depth refusals run on the shipped allowance: the depth bound is fixed
    // and has no profile of its own.
    out.extend(RefusalVariant::ALL.into_iter().map(|variant| {
        let class = if variant == RefusalVariant::Healing {
            ProfileHealing
        } else {
            RefusalRepeat
        };
        let target = if variant.is_depth() {
            Target::Refusal {
                variant,
                op_budget: 0,
                props: DEPTH_LEVELS,
            }
        } else {
            Target::Refusal {
                variant,
                op_budget: REFUSAL_OP_BUDGET,
                props: REFUSAL_PROPS,
            }
        };
        (class, target)
    }));
    out.push((
        PublicationGates,
        Target::Gates {
            sources: GATE_SOURCES,
        },
    ));
    match mode {
        Mode::Normal => {
            out.extend((0..5).map(|x| (ColdDemand, query(format!("X{x}")))));
            out.push((
                MetaIsolation,
                Target::Isolation {
                    op_budget: ISOLATION_OP_BUDGET,
                    spend_props: ISOLATION_SPEND_PROPS,
                    refuse_props: ISOLATION_REFUSE_PROPS,
                },
            ));
        }
        Mode::Pressure => {
            let surge = tranche % SURGE_PERIOD == SURGE_PERIOD - 1;
            let big = Target::File {
                file: big_file(tranche),
                revision: 0,
            };
            out.push((Open, big.clone()));
            out.push((
                LargeDemand,
                Target::Query {
                    file: big_file(tranche),
                    alias: "Big0".into(),
                },
            ));
            out.push((
                MetaIsolation,
                Target::Isolation {
                    op_budget: ISOLATION_OP_BUDGET,
                    spend_props: ISOLATION_SPEND_PROPS,
                    refuse_props: ISOLATION_REFUSE_PROPS,
                },
            ));
            let churn = |i| {
                (
                    CandidateChurn,
                    Target::Churn {
                        file: module.clone(),
                        alias: a(i),
                        edits: CHURN_EDITS,
                    },
                )
            };
            out.push(churn(13));
            let surge_of = |modules, props, oversized_props| {
                (
                    PressureSurge,
                    Target::Surge {
                        modules,
                        props,
                        oversized_props,
                    },
                )
            };
            out.push(if surge {
                surge_of(SURGE_MODULES, SURGE_PROPS, OVERSIZED_PROPS)
            } else if tranche.is_multiple_of(SURGE_PERIOD) {
                surge_of(
                    BOUNDED_SURGE_MODULES,
                    BOUNDED_SURGE_PROPS,
                    BOUNDED_SURGE_PROPS,
                )
            } else {
                churn(14)
            });
            out.push((Close, big));
        }
    }
    out.extend([
        (Close, file(0)),
        (
            Close,
            Target::File {
                file: dep,
                revision: 0,
            },
        ),
        (Reclaim, Target::Store),
    ]);
    out
}

/// Lower-case hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}
