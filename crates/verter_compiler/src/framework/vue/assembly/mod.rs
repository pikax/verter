#![allow(dead_code, unused_variables)]
//! Vue module assembly: the complete artifacts of one component.

mod artifact_set;
mod pipeline;
mod script_fragment;

pub use artifact_set::{VueArtifactSet, VueComponentMetadata, VueStyleMetadata};
pub use pipeline::{compile_vue_runtime, VueCompileInputs, VueCompileRefusal};
#[cfg(any(test, feature = "test-support"))]
pub(crate) use pipeline::{
    with_target_inputs, VueRetainedPrerequisites, VueTargetPrerequisiteArgs,
};
#[cfg(any(test, feature = "test-support"))]
pub(crate) use script_fragment::freeze_script;
