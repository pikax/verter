//! The finite demand closure of one compile request, and the admissions a
//! compile composes before any target runs. A capability outside the closed
//! set performs no work.

mod admission;
mod closure;
mod stage_options;
mod universe;

pub use admission::{Coverage, DemandRefusal, InputDemand, StageKeys};
pub use closure::{close_demand, DemandEdge, DemandReason, DemandSeed, DemandSet};
pub use universe::{Capability, FactFamilies, FactFamily, TargetKind, CAPABILITY_COUNT};

pub(crate) use admission::{CompileAdmission, CompileProgress, ParseAdmission, SemanticAdmission};
