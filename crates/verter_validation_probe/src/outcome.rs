//! The closed probe outcome taxonomy and its causal fold.
//!
//! A probe observes a case per [`Dimension`]. Every class observed at one
//! dimension folds through [`ProbeOutcomeClass::terminal`] into exactly one
//! [`Terminal`]: the highest-precedence class is the outcome and every lower
//! class is kept as secondary evidence, never discarded and never promoted.
//! [`CaseObservation`] holds one terminal per dimension and enforces the
//! one-way propagation of lower-dimension failures into higher dimensions.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// One observed dimension of a case. Each manifest cell and each terminal is
/// keyed by exactly one dimension; a cell is only ever evaluated against its
/// own dimension's terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Dimension {
    /// Whether the public route answered, and how the process behaved.
    Route,
    /// Whether the compiler produced a valid product or a typed diagnostic.
    Compile,
    /// Structural comparison of the product against the reference.
    Structural,
    /// Runtime comparison of the product against the reference.
    Runtime,
    /// Source-map comparison of the product against the reference.
    Map,
    /// Whether the measured invocation completed.
    Performance,
}

impl Dimension {
    /// Every dimension. Each dimension's feeding dimension precedes it.
    pub const ALL: [Dimension; 6] = [
        Dimension::Route,
        Dimension::Compile,
        Dimension::Structural,
        Dimension::Runtime,
        Dimension::Map,
        Dimension::Performance,
    ];

    /// The closed class/dimension admissibility matrix.
    pub fn admits(self, class: ProbeOutcomeClass) -> bool {
        use ProbeOutcomeClass as C;
        let route = matches!(
            class,
            C::Pass
                | C::HarnessFailure
                | C::Crash
                | C::Timeout
                | C::HostFailure
                | C::RequestRefused
        );
        let compile = route
            || matches!(
                class,
                C::Unsupported | C::VerterDiagnostic | C::ProductNotProduced | C::ProductMalformed
            );
        match self {
            Dimension::Route => route,
            Dimension::Compile => compile,
            Dimension::Structural => {
                compile || matches!(class, C::ReferenceFailure | C::SemanticMismatch)
            }
            Dimension::Runtime => compile || class == C::RuntimeMismatch,
            Dimension::Map => compile || class == C::SourceMapMismatch,
            Dimension::Performance => {
                matches!(class, C::Pass | C::HarnessFailure | C::Crash | C::Timeout)
            }
        }
    }

    /// The lower dimension whose failure propagates into this one: `Route`
    /// feeds `Compile`, and `Compile` feeds every higher dimension.
    pub const fn feeding(self) -> Option<Dimension> {
        match self {
            Dimension::Route => None,
            Dimension::Compile => Some(Dimension::Route),
            Dimension::Structural
            | Dimension::Runtime
            | Dimension::Map
            | Dimension::Performance => Some(Dimension::Compile),
        }
    }

    /// The serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Dimension::Route => "Route",
            Dimension::Compile => "Compile",
            Dimension::Structural => "Structural",
            Dimension::Runtime => "Runtime",
            Dimension::Map => "Map",
            Dimension::Performance => "Performance",
        }
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The closed probe outcome taxonomy. There is no string-typed, generic, or
/// tool-specific outcome: a class is added only for a genuinely distinct
/// actionable cause, never as an alias.
///
/// Variants are declared in causal precedence order, highest first; the
/// order itself is defined by [`ProbeOutcomeClass::rank`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeOutcomeClass {
    /// The harness could not execute or observe the case: addon load, driver
    /// protocol, spawn, or observation failure. Never a typed refusal the
    /// route returned.
    HarnessFailure,
    /// The compiler process died by signal or abnormal exit.
    Crash,
    /// The compiler process was stopped by the deadline.
    Timeout,
    /// The host or session failed for a cause it names before compiling.
    HostFailure,
    /// The public route refused the request's shape, framework, or binding
    /// before compiling.
    RequestRefused,
    /// The compiler explicitly declared the input outside its contract.
    Unsupported,
    /// The compiler reported an error diagnostic.
    VerterDiagnostic,
    /// The reference side of a comparison could not be produced or executed.
    ReferenceFailure,
    /// Compilation ended without a diagnostic and without a product.
    ProductNotProduced,
    /// A product exists but fails its own validity check.
    ProductMalformed,
    /// Structural comparison differs.
    SemanticMismatch,
    /// Runtime comparison differs.
    RuntimeMismatch,
    /// Source-map comparison differs. A runtime mismatch outranks it because
    /// a wrong program makes its map moot.
    SourceMapMismatch,
    /// Nothing failed.
    Pass,
}

impl ProbeOutcomeClass {
    /// Every class, highest precedence first.
    pub const ALL: [ProbeOutcomeClass; 14] = [
        ProbeOutcomeClass::HarnessFailure,
        ProbeOutcomeClass::Crash,
        ProbeOutcomeClass::Timeout,
        ProbeOutcomeClass::HostFailure,
        ProbeOutcomeClass::RequestRefused,
        ProbeOutcomeClass::Unsupported,
        ProbeOutcomeClass::VerterDiagnostic,
        ProbeOutcomeClass::ReferenceFailure,
        ProbeOutcomeClass::ProductNotProduced,
        ProbeOutcomeClass::ProductMalformed,
        ProbeOutcomeClass::SemanticMismatch,
        ProbeOutcomeClass::RuntimeMismatch,
        ProbeOutcomeClass::SourceMapMismatch,
        ProbeOutcomeClass::Pass,
    ];

    /// Causal precedence; `0` is the highest. Total over the taxonomy.
    pub const fn rank(self) -> u8 {
        match self {
            ProbeOutcomeClass::HarnessFailure => 0,
            ProbeOutcomeClass::Crash => 1,
            ProbeOutcomeClass::Timeout => 2,
            ProbeOutcomeClass::HostFailure => 3,
            ProbeOutcomeClass::RequestRefused => 4,
            ProbeOutcomeClass::Unsupported => 5,
            ProbeOutcomeClass::VerterDiagnostic => 6,
            ProbeOutcomeClass::ReferenceFailure => 7,
            ProbeOutcomeClass::ProductNotProduced => 8,
            ProbeOutcomeClass::ProductMalformed => 9,
            ProbeOutcomeClass::SemanticMismatch => 10,
            ProbeOutcomeClass::RuntimeMismatch => 11,
            ProbeOutcomeClass::SourceMapMismatch => 12,
            ProbeOutcomeClass::Pass => 13,
        }
    }

    /// Whether this class is a failure (every class but `pass`).
    pub const fn is_failure(self) -> bool {
        !matches!(self, ProbeOutcomeClass::Pass)
    }

    /// Whether this class is a comparison result, which requires both a
    /// product and a reference.
    pub const fn is_comparison(self) -> bool {
        matches!(
            self,
            ProbeOutcomeClass::SemanticMismatch
                | ProbeOutcomeClass::RuntimeMismatch
                | ProbeOutcomeClass::SourceMapMismatch
        )
    }

    /// Whether this class, observed in the same dimension, means a comparison
    /// had no product or no reference to compare.
    const fn means_absent_comparison_input(self) -> bool {
        matches!(
            self,
            ProbeOutcomeClass::ProductNotProduced
                | ProbeOutcomeClass::Unsupported
                | ProbeOutcomeClass::RequestRefused
                | ProbeOutcomeClass::HostFailure
                | ProbeOutcomeClass::ReferenceFailure
        )
    }

    /// The serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            ProbeOutcomeClass::HarnessFailure => "harness_failure",
            ProbeOutcomeClass::Crash => "crash",
            ProbeOutcomeClass::Timeout => "timeout",
            ProbeOutcomeClass::HostFailure => "host_failure",
            ProbeOutcomeClass::RequestRefused => "request_refused",
            ProbeOutcomeClass::Unsupported => "unsupported",
            ProbeOutcomeClass::VerterDiagnostic => "verter_diagnostic",
            ProbeOutcomeClass::ReferenceFailure => "reference_failure",
            ProbeOutcomeClass::ProductNotProduced => "product_not_produced",
            ProbeOutcomeClass::ProductMalformed => "product_malformed",
            ProbeOutcomeClass::SemanticMismatch => "semantic_mismatch",
            ProbeOutcomeClass::RuntimeMismatch => "runtime_mismatch",
            ProbeOutcomeClass::SourceMapMismatch => "source_map_mismatch",
            ProbeOutcomeClass::Pass => "pass",
        }
    }

    /// Fold every class observed at `dimension` into its single terminal.
    ///
    /// The terminal class is the highest-precedence class present; every
    /// other distinct class is retained as `secondary`, in precedence order.
    /// Rejected rather than resolved: an empty observation, a class the
    /// dimension does not admit, `pass` together with any failure, and a
    /// comparison class together with an absent product or reference.
    pub fn terminal(
        dimension: Dimension,
        observed: &[ProbeOutcomeClass],
    ) -> Result<Terminal, InvalidObservation> {
        if observed.is_empty() {
            return Err(InvalidObservation::NothingObserved { dimension });
        }
        if let Some(&class) = observed.iter().find(|class| !dimension.admits(**class)) {
            return Err(InvalidObservation::Inadmissible { dimension, class });
        }
        let mut classes = observed.to_vec();
        classes.sort_by_key(|class| class.rank());
        classes.dedup();
        if classes.len() > 1 && classes.contains(&ProbeOutcomeClass::Pass) {
            return Err(InvalidObservation::PassWithFailure {
                dimension,
                failure: classes[0],
            });
        }
        if let Some(&comparison) = classes.iter().find(|class| class.is_comparison()) {
            if let Some(&absent) = classes
                .iter()
                .find(|class| class.means_absent_comparison_input())
            {
                return Err(InvalidObservation::ComparisonWithoutInput {
                    dimension,
                    comparison,
                    absent,
                });
            }
        }
        let class = classes.remove(0);
        Ok(Terminal::Class {
            class,
            secondary: classes,
            evidence: Vec::new(),
        })
    }
}

impl fmt::Display for ProbeOutcomeClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a piece of evidence came from, so a message is never detached from
/// its origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    /// A diagnostic the addon returned.
    AddonDiagnostic,
    /// A parse check the runner performed.
    Parser,
    /// A semantic build check the runner performed.
    SemanticBuilder,
    /// The structural comparator's difference reason.
    Comparator,
    /// The driver process or its protocol.
    Driver,
    /// The reference producer.
    Reference,
}

/// One origin-tagged message retained on a terminal.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// The origin of the message.
    pub source: EvidenceSource,
    /// The message, verbatim.
    pub message: String,
}

/// Why a dimension cannot be exercised for a framework at all. Each reason
/// belongs to exactly one dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotApplicableReason {
    /// No structural comparator is bound (`Structural`).
    ComparatorAbsent,
    /// No runtime executor is bound (`Runtime`).
    RuntimeExecutorAbsent,
    /// No source-map validator is bound (`Map`).
    MapValidatorAbsent,
}

impl NotApplicableReason {
    /// The one dimension this reason may appear at.
    pub const fn dimension(self) -> Dimension {
        match self {
            NotApplicableReason::ComparatorAbsent => Dimension::Structural,
            NotApplicableReason::RuntimeExecutorAbsent => Dimension::Runtime,
            NotApplicableReason::MapValidatorAbsent => Dimension::Map,
        }
    }

    /// The serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            NotApplicableReason::ComparatorAbsent => "comparator_absent",
            NotApplicableReason::RuntimeExecutorAbsent => "runtime_executor_absent",
            NotApplicableReason::MapValidatorAbsent => "map_validator_absent",
        }
    }
}

impl fmt::Display for NotApplicableReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The single outcome of one dimension of one case.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Terminal {
    /// An observed (or propagated) class with its retained lower classes and
    /// evidence.
    Class {
        /// The highest-precedence class present.
        class: ProbeOutcomeClass,
        /// Every other distinct class, in precedence order.
        secondary: Vec<ProbeOutcomeClass>,
        /// Origin-tagged messages behind the classes.
        evidence: Vec<Evidence>,
    },
    /// The dimension was never reached because a lower dimension failed with
    /// a class this dimension does not admit. Not an outcome class: it can
    /// neither pass a cell nor be expected by one.
    NotRun {
        /// The feeding dimension's failure class.
        blocked_by: ProbeOutcomeClass,
    },
    /// The dimension cannot be exercised for this framework at all.
    NotApplicable {
        /// Which surface is absent.
        reason: NotApplicableReason,
    },
}

impl Terminal {
    /// Attach evidence to a class terminal. Other terminals carry none and
    /// are returned unchanged.
    pub fn with_evidence(self, evidence: Vec<Evidence>) -> Terminal {
        match self {
            Terminal::Class {
                class, secondary, ..
            } => Terminal::Class {
                class,
                secondary,
                evidence,
            },
            other => other,
        }
    }

    /// The terminal class, when this terminal is one.
    pub fn class(&self) -> Option<ProbeOutcomeClass> {
        match self {
            Terminal::Class { class, .. } => Some(*class),
            Terminal::NotRun { .. } | Terminal::NotApplicable { .. } => None,
        }
    }

    fn failure(&self) -> Option<ProbeOutcomeClass> {
        self.class().filter(|class| class.is_failure())
    }

    fn classes(&self) -> impl Iterator<Item = ProbeOutcomeClass> + '_ {
        let (class, secondary): (Option<ProbeOutcomeClass>, &[ProbeOutcomeClass]) = match self {
            Terminal::Class {
                class, secondary, ..
            } => (Some(*class), secondary),
            Terminal::NotRun { .. } | Terminal::NotApplicable { .. } => (None, &[]),
        };
        class.into_iter().chain(secondary.iter().copied())
    }
}

/// Why an observation cannot be represented. Impossible combinations are
/// rejected, never resolved into a plausible-looking terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvalidObservation {
    /// A case observation without a case id.
    EmptyCaseId,
    /// A dimension reported no class at all.
    NothingObserved {
        /// The dimension.
        dimension: Dimension,
    },
    /// A case observation lacks one dimension.
    MissingDimension {
        /// The absent dimension.
        dimension: Dimension,
    },
    /// A class observed at a dimension that does not admit it.
    Inadmissible {
        /// The dimension.
        dimension: Dimension,
        /// The class it does not admit.
        class: ProbeOutcomeClass,
    },
    /// `pass` observed together with a failure in the same dimension.
    PassWithFailure {
        /// The dimension.
        dimension: Dimension,
        /// The highest-precedence failure observed beside `pass`.
        failure: ProbeOutcomeClass,
    },
    /// A comparison class reported although its product or reference is
    /// absent.
    ComparisonWithoutInput {
        /// The dimension carrying the comparison class.
        dimension: Dimension,
        /// The comparison class.
        comparison: ProbeOutcomeClass,
        /// The class showing the missing input.
        absent: ProbeOutcomeClass,
    },
    /// A class terminal that is not the fold of its own classes.
    NonCanonicalTerminal {
        /// The dimension.
        dimension: Dimension,
    },
    /// A not-applicable reason placed at a dimension it does not belong to.
    MisplacedNotApplicable {
        /// The dimension.
        dimension: Dimension,
        /// The reason.
        reason: NotApplicableReason,
    },
    /// A not-run terminal its feeding dimension does not explain.
    UnblockedNotRun {
        /// The dimension.
        dimension: Dimension,
        /// The claimed blocking class.
        blocked_by: ProbeOutcomeClass,
    },
    /// A dimension was never reached although nothing below it failed.
    Unreached {
        /// The dimension.
        dimension: Dimension,
    },
    /// A dimension passes although its feeding dimension failed.
    ContradictsFeeding {
        /// The passing dimension.
        dimension: Dimension,
        /// The feeding dimension's failure.
        feeding: ProbeOutcomeClass,
    },
    /// A dimension is not applicable in the observation but applicable in
    /// the manifest, or the reverse.
    ApplicabilityMismatch {
        /// The dimension.
        dimension: Dimension,
        /// The reason the manifest declares, if any.
        declared: Option<NotApplicableReason>,
        /// The reason the observation reports, if any.
        observed: Option<NotApplicableReason>,
    },
}

impl fmt::Display for InvalidObservation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidObservation::EmptyCaseId => f.write_str("case observation has an empty case id"),
            InvalidObservation::NothingObserved { dimension } => {
                write!(f, "{dimension}: no class observed")
            }
            InvalidObservation::MissingDimension { dimension } => {
                write!(f, "{dimension}: dimension missing from the case observation")
            }
            InvalidObservation::Inadmissible { dimension, class } => {
                write!(f, "{dimension}: class {class} is not admissible at this dimension")
            }
            InvalidObservation::PassWithFailure { dimension, failure } => {
                write!(f, "{dimension}: pass observed together with {failure}")
            }
            InvalidObservation::ComparisonWithoutInput {
                dimension,
                comparison,
                absent,
            } => write!(
                f,
                "{dimension}: comparison class {comparison} reported although {absent} shows its input is absent"
            ),
            InvalidObservation::NonCanonicalTerminal { dimension } => {
                write!(f, "{dimension}: terminal is not the fold of its own classes")
            }
            InvalidObservation::MisplacedNotApplicable { dimension, reason } => {
                write!(f, "{dimension}: not-applicable reason {reason} belongs to another dimension")
            }
            InvalidObservation::UnblockedNotRun {
                dimension,
                blocked_by,
            } => write!(
                f,
                "{dimension}: not-run blocked by {blocked_by} is not explained by its feeding dimension"
            ),
            InvalidObservation::Unreached { dimension } => {
                write!(f, "{dimension}: unreached although nothing below it failed")
            }
            InvalidObservation::ContradictsFeeding { dimension, feeding } => {
                write!(f, "{dimension}: passes although its feeding dimension failed with {feeding}")
            }
            InvalidObservation::ApplicabilityMismatch {
                dimension,
                declared,
                observed,
            } => write!(
                f,
                "{dimension}: manifest declares not-applicable {declared:?}, observation reports {observed:?}"
            ),
        }
    }
}

impl std::error::Error for InvalidObservation {}

/// What a runner saw at one dimension of one case, before propagation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DimensionInput {
    /// The classes this dimension observed, with their evidence.
    Observed {
        /// Every class observed at this dimension.
        classes: Vec<ProbeOutcomeClass>,
        /// Origin-tagged messages behind those classes.
        evidence: Vec<Evidence>,
    },
    /// The dimension cannot be exercised for this framework at all.
    NotApplicable(NotApplicableReason),
    /// Execution never reached this dimension.
    Unreached,
}

/// The total per-dimension observation of one case: exactly one terminal
/// per [`Dimension`], consistent with one-way failure propagation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CaseObservationWire")]
pub struct CaseObservation {
    case_id: String,
    terminals: BTreeMap<Dimension, Terminal>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseObservationWire {
    case_id: String,
    terminals: BTreeMap<Dimension, Terminal>,
}

impl TryFrom<CaseObservationWire> for CaseObservation {
    type Error = InvalidObservation;

    fn try_from(wire: CaseObservationWire) -> Result<Self, Self::Error> {
        CaseObservation::new(wire.case_id, wire.terminals)
    }
}

impl CaseObservation {
    /// Fold and propagate per-dimension inputs into a case observation.
    ///
    /// A failure at a feeding dimension is copied into the fed dimension when
    /// that dimension admits the class, and recorded as
    /// [`Terminal::NotRun`] when it does not. A propagated class is only a
    /// default: an independently observed failure at the fed dimension wins
    /// that dimension, and a not-applicable dimension stays not applicable.
    /// An observed `pass` never overrides a propagated failure.
    pub fn fold(
        case_id: impl Into<String>,
        mut inputs: BTreeMap<Dimension, DimensionInput>,
    ) -> Result<Self, InvalidObservation> {
        let mut terminals = BTreeMap::new();
        for dimension in Dimension::ALL {
            let input = inputs
                .remove(&dimension)
                .ok_or(InvalidObservation::MissingDimension { dimension })?;
            let propagated = dimension
                .feeding()
                .and_then(|feeding| propagate(&terminals[&feeding], dimension));
            let terminal = match input {
                DimensionInput::NotApplicable(reason) => Terminal::NotApplicable { reason },
                DimensionInput::Observed { classes, evidence } => {
                    let own =
                        ProbeOutcomeClass::terminal(dimension, &classes)?.with_evidence(evidence);
                    if own.failure().is_some() {
                        own
                    } else {
                        propagated.unwrap_or(own)
                    }
                }
                DimensionInput::Unreached => {
                    propagated.ok_or(InvalidObservation::Unreached { dimension })?
                }
            };
            terminals.insert(dimension, terminal);
        }
        CaseObservation::new(case_id, terminals)
    }

    /// Accept a complete set of terminals after checking every invariant
    /// [`CaseObservation::fold`] establishes: totality, canonical class
    /// terminals, reason placement, propagation consistency, and no
    /// comparison class for a case whose product is absent.
    pub fn new(
        case_id: impl Into<String>,
        terminals: BTreeMap<Dimension, Terminal>,
    ) -> Result<Self, InvalidObservation> {
        let case_id = case_id.into();
        if case_id.is_empty() {
            return Err(InvalidObservation::EmptyCaseId);
        }
        for dimension in Dimension::ALL {
            let terminal = terminals
                .get(&dimension)
                .ok_or(InvalidObservation::MissingDimension { dimension })?;
            check_terminal(dimension, terminal)?;
            let feeding = dimension.feeding().map(|feeding| &terminals[&feeding]);
            check_against_feeding(dimension, terminal, feeding)?;
        }
        if let Some(absent) = product_absence(&terminals[&Dimension::Compile]) {
            for dimension in [Dimension::Structural, Dimension::Runtime, Dimension::Map] {
                if let Some(comparison) = terminals[&dimension]
                    .classes()
                    .find(|class| class.is_comparison())
                {
                    return Err(InvalidObservation::ComparisonWithoutInput {
                        dimension,
                        comparison,
                        absent,
                    });
                }
            }
        }
        Ok(CaseObservation { case_id, terminals })
    }

    /// The observed case id.
    pub fn case_id(&self) -> &str {
        &self.case_id
    }

    /// Every dimension's terminal.
    pub fn terminals(&self) -> &BTreeMap<Dimension, Terminal> {
        &self.terminals
    }

    /// One dimension's terminal. Total: every dimension is present.
    pub fn terminal(&self, dimension: Dimension) -> &Terminal {
        &self.terminals[&dimension]
    }
}

fn propagate(feeding: &Terminal, dimension: Dimension) -> Option<Terminal> {
    let Terminal::Class {
        class,
        secondary,
        evidence,
    } = feeding
    else {
        return None;
    };
    if !class.is_failure() {
        return None;
    }
    Some(if dimension.admits(*class) {
        Terminal::Class {
            class: *class,
            secondary: secondary
                .iter()
                .copied()
                .filter(|secondary| dimension.admits(*secondary))
                .collect(),
            evidence: evidence.clone(),
        }
    } else {
        Terminal::NotRun { blocked_by: *class }
    })
}

fn check_terminal(dimension: Dimension, terminal: &Terminal) -> Result<(), InvalidObservation> {
    match terminal {
        Terminal::Class {
            class, secondary, ..
        } => {
            let all: Vec<ProbeOutcomeClass> = std::iter::once(*class)
                .chain(secondary.iter().copied())
                .collect();
            match ProbeOutcomeClass::terminal(dimension, &all)? {
                Terminal::Class {
                    class: folded,
                    secondary: folded_secondary,
                    ..
                } if folded == *class && folded_secondary == *secondary => Ok(()),
                _ => Err(InvalidObservation::NonCanonicalTerminal { dimension }),
            }
        }
        Terminal::NotApplicable { reason } if reason.dimension() != dimension => {
            Err(InvalidObservation::MisplacedNotApplicable {
                dimension,
                reason: *reason,
            })
        }
        Terminal::NotApplicable { .. } | Terminal::NotRun { .. } => Ok(()),
    }
}

fn check_against_feeding(
    dimension: Dimension,
    terminal: &Terminal,
    feeding: Option<&Terminal>,
) -> Result<(), InvalidObservation> {
    let feeding_failure = feeding.and_then(Terminal::failure);
    match (terminal, feeding_failure) {
        (Terminal::NotRun { blocked_by }, Some(failure))
            if *blocked_by == failure && !dimension.admits(failure) =>
        {
            Ok(())
        }
        (Terminal::NotRun { blocked_by }, _) => Err(InvalidObservation::UnblockedNotRun {
            dimension,
            blocked_by: *blocked_by,
        }),
        (
            Terminal::Class {
                class: ProbeOutcomeClass::Pass,
                ..
            },
            Some(failure),
        ) => Err(InvalidObservation::ContradictsFeeding {
            dimension,
            feeding: failure,
        }),
        _ => Ok(()),
    }
}

/// The `Compile` class showing that no product exists, if any.
fn product_absence(compile: &Terminal) -> Option<ProbeOutcomeClass> {
    use ProbeOutcomeClass as C;
    match compile {
        Terminal::Class { class, .. }
            if matches!(
                class,
                C::HarnessFailure
                    | C::Crash
                    | C::Timeout
                    | C::HostFailure
                    | C::RequestRefused
                    | C::Unsupported
                    | C::ProductNotProduced
            ) =>
        {
            Some(*class)
        }
        Terminal::Class { secondary, .. } if secondary.contains(&C::ProductNotProduced) => {
            Some(C::ProductNotProduced)
        }
        _ => None,
    }
}
