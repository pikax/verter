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
//!
//! [`CompileArtifactSet`] is the immutable schema for already-produced
//! artifacts, with canonical identities, typed relations, input provenance
//! and qualified byte maps. Schema validation is independent of compilation
//! and does not mint an [`ArtifactSet`] publication receipt.

pub mod compose;
pub mod custom_block;
pub mod fragment;
pub mod map_compose;
pub mod map_input;
mod map_json;
pub mod plan;
pub mod publish;
pub mod source_space;
pub mod source_unit;
pub mod vue_module;

pub use compose::{
    assemble_sequence, prepend_preamble, splice_into_hole, ComposeRefusal, ComposedOutput,
    SequencedOutput,
};
pub use custom_block::{
    CustomBlockContent, CustomBlockDescriptor, CustomBlockDescriptorError, CustomBlockDescriptorId,
    CustomBlockDescriptorRequest, CustomBlockLifecycle,
};
pub use fragment::{
    ArtifactContent, ArtifactId, ArtifactProvenance, ArtifactRelation, ArtifactRelationKind,
    ArtifactUnavailableReason, CompileArtifact, DeclaredExport, DeclaredHelper, DeclaredImport,
    DeclaredImportKind, Fragment, FragmentDialect, FragmentId, FragmentRefusal, FrameworkDomain,
    PlacementSlot, SfcExportPlacement, SyntacticContract, ValidatedFragment,
};
pub use plan::{PlannedArtifact, ProductPlan};
// `publish` and `ArtifactContribution` are `pub(crate)` (see their own doc
// comments) — this crate's compose-and-publish callers
// (`vue_module::compose_main_module`, `standalone::StandaloneCompiler`)
// reach them through `super::publish::{publish, ArtifactContribution}`
// directly, not through this public re-export.
pub use map_input::{AssembleMapFailure, MapFragment, UncomposableCode, UncomposableFamily};
pub use publish::{
    ArtifactSchemaError, ArtifactSet, AssembledArtifact, AssemblyRefusal, CompileArtifactSet,
};
pub use source_space::{
    ArtifactMapFamily, ArtifactMapSegment, AssembledOffset, FragmentOffset, FragmentRange,
    QualifiedArtifactMap, SourceSpaceKind,
};
pub use source_unit::{
    ArtifactSourceUnit, ContentId, SourceId, SourceRevision, SourceUnit, SourceUnitId,
};
pub use vue_module::{
    assemble_vue_runtime_main, compose_main_module, vue_main_compile_artifacts, ExtraFragment,
    SfcRewriteRefusal, VueMainAssemblyFailure, VueMainCompositionFailure, VueMainDecoration,
    VueMainModuleRequest, VueRuntimeMainAssembled, VueRuntimeMainRequest,
};
#[cfg(any(test, feature = "test-support"))]
pub use vue_module::{reset_vue_main_assembly_count, vue_main_assembly_count};

#[cfg(test)]
mod artifact_schema_tests;
#[cfg(test)]
mod custom_block_tests;
