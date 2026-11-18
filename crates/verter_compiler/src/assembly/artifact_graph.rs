//! Roles and style facts of the artifacts in a compile artifact set, and
//! the queries a host uses to walk from a module to what it needs.

use verter_css_syntax::StyleStage;

use super::fragment::{ArtifactId, ArtifactRelationKind, CompileArtifact};
use super::publish::CompileArtifactSet;
use crate::compile_request::ProductKind;

/// What an artifact is to its host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactRole {
    /// A complete module the host loads.
    Module,
    /// One style block's CSS.
    Style,
    /// Component metadata as JSON.
    Metadata,
}

/// Facts of a style artifact.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleArtifactFacts {
    /// Position of the `<style>` block in the source, from zero.
    pub index: u32,
    pub scoped: bool,
    /// `Some("")` for an unnamed module; the binding name otherwise.
    pub module: Option<String>,
    /// Authored `lang`, `css` when absent.
    pub lang: String,
    /// The processing stage the published bytes are at.
    #[serde(skip)]
    pub stage: Option<StyleStage>,
    /// The bytes need an external stage before a browser can use them.
    pub needs_external_stage: bool,
}

/// Read-only walk over a set.
pub trait ArtifactGraph {
    /// The module artifact of a runtime product.
    fn module(&self, product: ProductKind) -> Option<&CompileArtifact>;
    /// Style artifacts of a module, ordered by block index.
    fn styles_of(&self, module: &ArtifactId) -> Vec<&CompileArtifact>;
    /// The metadata artifact of a module.
    fn metadata_of(&self, module: &ArtifactId) -> Option<&CompileArtifact>;
    fn role_of(&self, artifact: &ArtifactId) -> Option<ArtifactRole>;
    fn style_facts(&self, artifact: &ArtifactId) -> Option<&StyleArtifactFacts>;
}

/// Whether `artifact` names `module` through a relation of `kind`.
fn relates_to(artifact: &CompileArtifact, kind: ArtifactRelationKind, module: &ArtifactId) -> bool {
    artifact
        .relations
        .iter()
        .any(|relation| relation.kind == kind && &relation.target == module)
}

impl ArtifactGraph for CompileArtifactSet {
    fn module(&self, product: ProductKind) -> Option<&CompileArtifact> {
        self.artifacts().iter().find(|artifact| {
            artifact.role() == ArtifactRole::Module && artifact.product() == product
        })
    }

    fn styles_of(&self, module: &ArtifactId) -> Vec<&CompileArtifact> {
        // The set guarantees every style artifact carries facts and that the
        // indexes of one module's styles are distinct, so this order is total.
        let mut styles: Vec<_> = self
            .artifacts()
            .iter()
            .filter(|artifact| {
                artifact.role() == ArtifactRole::Style
                    && relates_to(artifact, ArtifactRelationKind::StyleOf, module)
            })
            .collect();
        styles.sort_by_key(|artifact| artifact.style().map(|facts| facts.index));
        styles
    }

    fn metadata_of(&self, module: &ArtifactId) -> Option<&CompileArtifact> {
        self.artifacts().iter().find(|artifact| {
            artifact.role() == ArtifactRole::Metadata
                && relates_to(artifact, ArtifactRelationKind::MetadataOf, module)
        })
    }

    fn role_of(&self, artifact: &ArtifactId) -> Option<ArtifactRole> {
        self.artifact(artifact).map(CompileArtifact::role)
    }

    fn style_facts(&self, artifact: &ArtifactId) -> Option<&StyleArtifactFacts> {
        self.artifact(artifact).and_then(CompileArtifact::style)
    }
}
