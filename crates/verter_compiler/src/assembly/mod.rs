//! Logical source units, fragment placement, source-space/mapping
//! composition, and atomic compiler-artifact publication.
//!
//! [`compose`] stays the fragment-local edit engine (owns one fragment's
//! own bytes/map) is [`crate::code_transform::CodeTransform`] — unchanged.
//! This module owns what combines multiple already-generated fragments
//! into one final product and decides where things go: [`fragment`]
//! declares a generated unit's contract/placement/identity, [`source_unit`]
//! mints the stable logical identity a fragment is minted against,
//! [`source_space`] keeps the three coordinate spaces (authored /
//! generated-fragment / assembled-output) from ever sharing a
//! representation, [`plan`] derives the exact artifact set one
//! [`crate::compile_request::CompileRequest`] plans, [`compose`] splices a
//! fragment into another fragment's declared hole, and [`publish`] is the
//! sole atomic publication boundary.

pub mod compose;
pub mod fragment;
pub mod plan;
pub mod publish;
pub mod source_space;
pub mod source_unit;
pub mod vue_module;

pub use compose::{
    assemble_sequence, prepend_preamble, splice_into_hole, ComposeRefusal, ComposedOutput,
    SequencedOutput,
};
pub use fragment::{
    DeclaredExport, DeclaredHelper, DeclaredImport, DeclaredImportKind, Fragment, FragmentDialect,
    FragmentId, FragmentRefusal, FrameworkDomain, PlacementSlot, SfcExportPlacement,
    SyntacticContract, ValidatedFragment,
};
pub use plan::{PlannedArtifact, ProductPlan};
// `publish` and `ArtifactContribution` are `pub(crate)` (see their own doc
// comments) — this crate's compose-and-publish callers
// (`vue_module::compose_main_module`, `standalone::StandaloneCompiler`)
// reach them through `super::publish::{publish, ArtifactContribution}`
// directly, not through this public re-export.
pub use publish::{ArtifactSet, AssembledArtifact, AssemblyRefusal};
pub use source_space::{AssembledOffset, FragmentOffset, FragmentRange, SourceSpaceKind};
pub use source_unit::{ContentId, SourceId, SourceRevision, SourceUnit, SourceUnitId};
pub use vue_module::{
    compose_main_module, ExtraFragment, SfcRewriteRefusal, VueMainAssemblyFailure,
    VueMainCompositionFailure, VueMainModuleRequest,
};
