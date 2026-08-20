//! The product plan: which artifacts an assembly must publish and which
//! mapping products each one requires, derived exclusively from a
//! [`CompileRequest`] — never from what a fragment producer happens to
//! emit. Nothing else may add or drop a planned artifact.

use crate::compile_request::{CompileProduct, CompileRequest, ProductKind};

/// One planned artifact this assembly must publish, plus its mapping
/// requirements — the four distinct mapping products
/// (`mapping-products.md` §1), never a single "maps enabled" boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedArtifact {
    pub kind: ProductKind,
    /// An IDE/provider companion's `SourceProjectionMap` is NOT optional —
    /// requesting the companion at all implies it. Always `true` when
    /// `kind == IdeCompanion`.
    pub requires_source_projection_map: bool,
    /// A runtime product's own optional runtime/build map segments.
    pub requires_runtime_source_map: bool,
}

/// The exact, non-empty set of artifacts one [`CompileRequest`] plans —
/// [`super::publish`] may publish exactly this set and nothing else.
#[derive(Debug, Clone)]
pub struct ProductPlan {
    artifacts: Vec<PlannedArtifact>,
}

impl ProductPlan {
    /// Derive the plan from a canonical request. Every [`CompileProduct`]
    /// the request carries becomes exactly one [`PlannedArtifact`] — no
    /// artifact is added merely because a producer needs it as an internal
    /// prerequisite (a runtime template chunk consumed only to fill an IDE
    /// composition hole is a *dependency*, not a planned artifact, and
    /// must not appear here).
    pub fn from_request(request: &CompileRequest) -> Self {
        let artifacts = request
            .products()
            .iter()
            .map(|product| match product {
                CompileProduct::RuntimeClient(r) => PlannedArtifact {
                    kind: ProductKind::RuntimeClient,
                    requires_source_projection_map: false,
                    requires_runtime_source_map: r.runtime_source_map,
                },
                CompileProduct::RuntimeServer(r) => PlannedArtifact {
                    kind: ProductKind::RuntimeServer,
                    requires_source_projection_map: false,
                    requires_runtime_source_map: r.runtime_source_map,
                },
                CompileProduct::IdeCompanion(_) => PlannedArtifact {
                    kind: ProductKind::IdeCompanion,
                    // Implicit — never a caller-toggled field: requesting an
                    // IDE companion always requires its source projection map.
                    requires_source_projection_map: true,
                    requires_runtime_source_map: false,
                },
                CompileProduct::PublicApi(_) => PlannedArtifact {
                    kind: ProductKind::PublicApi,
                    requires_source_projection_map: false,
                    requires_runtime_source_map: false,
                },
                CompileProduct::Declarations(_) => PlannedArtifact {
                    kind: ProductKind::Declarations,
                    requires_source_projection_map: false,
                    requires_runtime_source_map: false,
                },
                CompileProduct::Analysis(_) => PlannedArtifact {
                    kind: ProductKind::Analysis,
                    requires_source_projection_map: false,
                    requires_runtime_source_map: false,
                },
            })
            .collect();
        Self { artifacts }
    }

    /// A plan for exactly one artifact — a host-owned composer that never
    /// went through a [`CompileRequest`] (e.g. `verter_session`'s Vue main-
    /// module assembly, driven by the older framework-neutral
    /// [`crate::framework_common::RuntimeCompileOutput`] shape) still gets
    /// [`super::publish::publish`]'s atomicity and final-parse checks by
    /// declaring the one artifact it composes.
    pub fn single(artifact: PlannedArtifact) -> Self {
        Self {
            artifacts: vec![artifact],
        }
    }

    pub fn artifacts(&self) -> &[PlannedArtifact] {
        &self.artifacts
    }

    pub fn wants(&self, kind: ProductKind) -> bool {
        self.artifacts.iter().any(|a| a.kind == kind)
    }

    pub fn artifact(&self, kind: ProductKind) -> Option<&PlannedArtifact> {
        self.artifacts.iter().find(|a| a.kind == kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile_request::{
        FrameworkCompileRequest, IdeProductRequest, RuntimeProductRequest, VueCompileRequest,
    };

    fn vue_request(products: Vec<CompileProduct>) -> CompileRequest {
        CompileRequest::new(
            products,
            FrameworkCompileRequest::Vue(VueCompileRequest::default()),
            None,
            None,
            None,
            false,
            false,
        )
        .expect("test request constructs")
    }

    #[test]
    fn runtime_client_request_plans_exactly_that_artifact() {
        let request = vue_request(vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )]);
        let plan = ProductPlan::from_request(&request);
        assert_eq!(plan.artifacts().len(), 1);
        assert!(plan.wants(ProductKind::RuntimeClient));
        assert!(!plan.wants(ProductKind::IdeCompanion));
    }

    #[test]
    fn ide_companion_request_always_requires_source_projection_map() {
        let request = vue_request(vec![CompileProduct::IdeCompanion(
            IdeProductRequest::default(),
        )]);
        let plan = ProductPlan::from_request(&request);
        let artifact = plan
            .artifact(ProductKind::IdeCompanion)
            .expect("ide companion planned");
        assert!(
            artifact.requires_source_projection_map,
            "an IdeCompanion artifact must always require its projection map, \
             independent of any caller-supplied flag"
        );
    }

    #[test]
    fn runtime_client_without_runtime_source_map_flag_does_not_require_one() {
        let request = vue_request(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
            runtime_source_map: false,
            ..Default::default()
        })]);
        let plan = ProductPlan::from_request(&request);
        let artifact = plan
            .artifact(ProductKind::RuntimeClient)
            .expect("runtime client planned");
        assert!(!artifact.requires_runtime_source_map);
        assert!(
            !artifact.requires_source_projection_map,
            "a runtime product never requires a SourceProjectionMap — that is \
             the IDE companion's own mapping product"
        );
    }

    #[test]
    fn runtime_client_with_runtime_source_map_flag_requires_one() {
        let request = vue_request(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
            runtime_source_map: true,
            ..Default::default()
        })]);
        let plan = ProductPlan::from_request(&request);
        let artifact = plan
            .artifact(ProductKind::RuntimeClient)
            .expect("runtime client planned");
        assert!(artifact.requires_runtime_source_map);
    }

    #[test]
    fn requesting_runtime_client_alone_never_plans_an_ide_companion() {
        // The internal-prerequisite rule: a producer that needs a runtime
        // template chunk as scaffolding for IDE composition must not leak
        // that as a second planned artifact when the caller only asked for
        // one product.
        let request = vue_request(vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )]);
        let plan = ProductPlan::from_request(&request);
        assert_eq!(plan.artifacts().len(), 1);
    }
}
