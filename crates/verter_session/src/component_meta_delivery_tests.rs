use verter_session_query::retention::heap_size::OwnedValue;
use verter_session_query::retention::ChargeClass;

use crate::meta::MetaProject;
use crate::types::HostConfig;
use crate::VerterHost;

/// A cold delivery hands out the very cell the final-result cache publishes
/// and a warm delivery shares it: the cache retains the analysis while it
/// keeps the entry, and once the cache lets go every delivery still holding
/// the analysis keeps it charged, pinned, rather than holding an alias the
/// cache's charge no longer covers.
#[test]
fn a_delivered_analysis_shares_the_cached_cell_and_keeps_its_charge_past_the_cache() {
    let project = MetaProject::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    }));
    project
        .upsert_base(
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n<template><div>{{ title }}</div></template>\n",
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let cold = session
        .get_component_meta("/src/Comp.vue")
        .unwrap()
        .expect("a cold delivery");
    let warm = session
        .get_component_meta("/src/Comp.vue")
        .unwrap()
        .expect("a warm delivery");
    assert!(
        OwnedValue::ptr_eq(cold.shared_analysis(), warm.shared_analysis()),
        "the warm delivery shares the cell the cold one published"
    );
    assert_eq!(
        warm.shared_analysis().charge_class(),
        Some(ChargeClass::Retained),
        "the cache retains the analysis while it keeps the entry"
    );
    assert_eq!(warm.analysis().props.len(), 1);

    project
        .host()
        .project_type_store()
        .component_meta_results()
        .invalidate_all();
    assert_eq!(
        warm.shared_analysis().charge_class(),
        Some(ChargeClass::Pinned),
        "the cache let go: the deliveries keep the analysis charged"
    );
    assert_eq!(cold.analysis().props[0].name, "title");
}

mod nested_publication_allocations {
    use verter_session_query::analysis::component_meta::{
        AcceptedSurfaceCompleteness, ComponentMetaAnalysis, ComponentMetaFlags, FallthroughSurface,
        NoFallthroughReason, PropAnalysis, RootReachability,
    };
    use verter_session_query::retention::owned::{ChargeCtx, OwnedStr};
    use verter_session_query::retention::{ChargeClass, RetentionLimits, SemanticRetentionAccount};
    use verter_type_expr::facts::{SemanticTypeSource, SourcePosition, TopLevelOwnerId};
    use verter_type_expr::locators::{
        AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
    };
    use verter_type_expr::publication::{
        PublicationPolicy, ResolutionDiagnostic, ResolutionDiagnosticKind, ResolutionExactness,
        ResolutionProvenance, ResolvedTypeOutcome, TypePublication,
    };

    use crate::component_meta_cached_result::shared_component_meta_analysis;

    const LARGE: usize = 64 * 1024;

    pub(super) fn authored(canonical_id: &str) -> SemanticTypeSource {
        SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(TypeBodySlot {
            anchor: AuthoredAnchor {
                canonical_id: canonical_id.into(),
                owner: TopLevelOwnerId::default(),
                symbol: OwnedStr::from("Props"),
                space: LocatorSymbolSpace::Type,
            },
            path: Vec::new().into(),
        }))
    }

    fn analysis(canonical_id: &str, context: &str) -> ComponentMetaAnalysis {
        analysis_publishing(authored(canonical_id), context)
    }

    pub(super) fn analysis_publishing(
        source: SemanticTypeSource,
        context: &str,
    ) -> ComponentMetaAnalysis {
        let publication = TypePublication::from_source_position(
            &SourcePosition::Present(source),
            ResolutionExactness::ExactConcrete,
            ResolutionProvenance::SemanticEvaluator,
            vec![ResolutionDiagnostic {
                kind: ResolutionDiagnosticKind::UnresolvedReference,
                context: context.into(),
                property_name: None,
            }],
            None,
            &PublicationPolicy::exact_only(),
        );
        ComponentMetaAnalysis {
            domain_guarantees: Default::default(),
            props: vec![PropAnalysis {
                name: "label".to_string(),
                callable_role: verter_type_expr::PropCallableRole::Other,
                publication,
                type_expansion: None,
                required: true,
                has_default: false,
                default_value: None,
                description: None,
                tags: Vec::new(),
                declared_in_macro_type_arg: true,
            }],
            events: Vec::new(),
            slots: Vec::new(),
            models: Vec::new(),
            exposed: Vec::new(),
            public_instance: None,
            ordered_sfc_structure: None,
            type_registry: Vec::new(),
            components: Vec::new(),
            template_refs: Vec::new(),
            imports: Vec::new(),
            bindings: Vec::new(),
            vue_api_calls: Vec::new(),
            styles: Vec::new(),
            flags: ComponentMetaFlags::default(),
            root_reachability: RootReachability::NoFallthrough {
                reason: NoFallthroughReason::NoTemplate,
            },
            accepted_props: Vec::new(),
            accepted_events: Vec::new(),
            accepted_surface_completeness: AcceptedSurfaceCompleteness::Exact,
            fallthrough_surface: FallthroughSurface::None {
                reason: NoFallthroughReason::NoTemplate,
            },
            macro_expansion_diagnostics: Vec::new(),
            options_api: false,
            file_path: "/src/Owner.vue".to_string(),
        }
    }

    /// A caller that clones a published type source or a diagnostic's name
    /// out of a delivered analysis keeps an owned handle: the allocation
    /// stays charged by its own cell after the analysis cell's last holder
    /// lets go, at zero headroom, and its own final drop restores the
    /// baseline at once. Dropping the leaf first leaves the analysis's
    /// charge whole.
    #[test]
    fn a_cloned_publication_leaf_stays_charged_after_its_analysis_lets_go() {
        let canonical_id = format!("/{}.ts", "published_source_".repeat(LARGE / 17));
        let context = "diagnostic_context_".repeat(LARGE / 19);
        for leaf in ["source", "diagnostic"] {
            for leaf_first in [false, true] {
                let account = SemanticRetentionAccount::new(RetentionLimits {
                    aggregate_ceiling_bytes: 0,
                    max_entry_bytes: 0,
                    pin_threshold_bytes: 0,
                });
                let cell = shared_component_meta_analysis(analysis(&canonical_id, &context));
                cell.pin(ChargeCtx::new(&account));
                let published = account.snapshot().pinned_bytes;
                assert!(published >= canonical_id.len() + context.len());
                let authority = cell.props[0].publication.authority();
                let kept: Box<dyn std::any::Any> = match leaf {
                    "source" => match authority.outcome() {
                        ResolvedTypeOutcome::Present { source, .. } => {
                            assert_eq!(source.charge_class(), Some(ChargeClass::Pinned));
                            Box::new(source.clone())
                        }
                        other => panic!("a present source was published: {other:?}"),
                    },
                    _ => {
                        let name = authority.diagnostics()[0].context.clone();
                        assert_eq!(name.charge_class(), Some(ChargeClass::Pinned));
                        Box::new(name)
                    }
                };
                let kept_len = if leaf == "source" {
                    canonical_id.len()
                } else {
                    context.len()
                };
                assert_eq!(account.snapshot().pinned_bytes, published);
                if leaf_first {
                    drop(kept);
                    assert_eq!(account.snapshot().pinned_bytes, published);
                    drop(cell);
                } else {
                    drop(cell);
                    let surviving = account.snapshot().pinned_bytes;
                    assert!(
                        surviving >= kept_len && surviving < published,
                        "({leaf}) the kept leaf owns its allocation, not the analysis: \
                         {surviving} of {published}"
                    );
                    if leaf == "diagnostic" {
                        assert_eq!(
                            surviving,
                            OwnedStr::charge_for_len(context.len()),
                            "a kept name is charged exactly its own allocation"
                        );
                    }
                    drop(kept);
                }
                assert_eq!(
                    account.snapshot().total_bytes(),
                    0,
                    "({leaf}, leaf first {leaf_first}) final release is immediate"
                );
            }
        }
    }
}

mod nested_source_allocations {
    use verter_session_query::analysis::component_meta::ResolvedTypeAnalysis;
    use verter_session_query::retention::owned::{ChargeCtx, Owned, OwnedSlice, OwnedStr};
    use verter_session_query::retention::{ChargeClass, RetentionLimits, SemanticRetentionAccount};
    use verter_type_expr::facts::{
        ClosedTypeFact, FactPropertyKey, LeafTypeFact, ProjectedTypeFact, SemanticTypeSource,
        SourcePosition,
    };
    use verter_type_expr::publication::ResolvedTypeOutcome;
    use verter_type_expr::{PropertyKey, SyntheticCarrierKey, SyntheticCarrierSurfaceKind};

    use super::nested_publication_allocations::{analysis_publishing, authored};
    use crate::component_meta_cached_result::shared_component_meta_analysis;

    const LARGE: usize = 64 * 1024;

    /// A nested list or name a caller clones out of a published source.
    enum Leaf {
        Union(OwnedSlice<LeafTypeFact>),
        Path(OwnedSlice<FactPropertyKey>),
        Carrier(Owned<SyntheticCarrierKey>),
        Name(OwnedStr),
    }

    impl Leaf {
        fn charge_class(&self) -> Option<ChargeClass> {
            match self {
                Self::Union(list) => list.charge_class(),
                Self::Path(list) => list.charge_class(),
                Self::Carrier(key) => key.charge_class(),
                Self::Name(name) => name.charge_class(),
            }
        }
    }

    fn sources(text: &str) -> [(&'static str, SemanticTypeSource); 3] {
        let SemanticTypeSource::Authored(base) = authored("/src/Base.ts") else {
            unreachable!("an authored source");
        };
        [
            (
                "leaf union",
                SemanticTypeSource::Closed(ClosedTypeFact::LeafUnion(OwnedSlice::from(vec![
                    LeafTypeFact::StringLiteral(text.to_owned()),
                    LeafTypeFact::BooleanLiteral(true),
                ]))),
            ),
            (
                "member path",
                SemanticTypeSource::Projected(ProjectedTypeFact::MemberPath {
                    base,
                    path: OwnedSlice::from(vec![PropertyKey::String(OwnedStr::from(text))]),
                }),
            ),
            (
                "synthetic binding",
                SemanticTypeSource::SyntheticSlotBinding(Owned::new(SyntheticCarrierKey {
                    scope_canonical_id: OwnedStr::from("/src/Owner.vue"),
                    surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
                    slot_name: Some(OwnedStr::from("default")),
                    binding_name: OwnedStr::from(text),
                    value_node: 7,
                })),
            ),
        ]
    }

    /// The nested allocation a caller can clone out of `source` by borrow.
    fn nested_leaf(source: &SemanticTypeSource, name_only: bool) -> Leaf {
        match source {
            SemanticTypeSource::Closed(ClosedTypeFact::LeafUnion(members)) => {
                Leaf::Union(members.clone())
            }
            SemanticTypeSource::Projected(ProjectedTypeFact::MemberPath { path, .. }) => {
                match (name_only, &path[0]) {
                    (true, PropertyKey::String(name)) => Leaf::Name(name.clone()),
                    _ => Leaf::Path(path.clone()),
                }
            }
            SemanticTypeSource::SyntheticSlotBinding(key) if name_only => {
                Leaf::Name(key.binding_name.clone())
            }
            SemanticTypeSource::SyntheticSlotBinding(key) => Leaf::Carrier(key.clone()),
            other => panic!("a source with a nested allocation was published: {other:?}"),
        }
    }

    /// A caller that clones a list or a name out of a published type source
    /// — a closed leaf union, a projected member path or its key, a
    /// synthetic binding carrier or its name — from a prop's publication or
    /// from a type-registry entry keeps an owned handle: the nested
    /// allocation stays charged by its own cell after the analysis cell
    /// lets go, at zero headroom, and its own final drop restores the
    /// baseline at once. Dropping the leaf first leaves the analysis's
    /// charge whole.
    #[test]
    fn a_cloned_nested_source_allocation_stays_charged_after_its_analysis_lets_go() {
        let text = "nested_source_text_".repeat(LARGE / 19);
        for index in 0..3 {
            for registry in [false, true] {
                for name_only in [false, true] {
                    for leaf_first in [false, true] {
                        // A fresh source each run: the cell must be its only
                        // holder, so the kept leaf is the last owner left.
                        let (kind, source) = sources(&text)
                            .into_iter()
                            .nth(index)
                            .expect("three sources");
                        let label = format!(
                            "{kind}, registry {registry}, name {name_only}, \
                             leaf first {leaf_first}"
                        );
                        let account = SemanticRetentionAccount::new(RetentionLimits {
                            aggregate_ceiling_bytes: 0,
                            max_entry_bytes: 0,
                            pin_threshold_bytes: 0,
                        });
                        let mut analysis = analysis_publishing(source.clone(), "context");
                        if registry {
                            analysis.props.clear();
                            analysis.type_registry.push(ResolvedTypeAnalysis {
                                name: "Registered".to_owned(),
                                type_source: SourcePosition::Present(source),
                                type_expansion: None,
                            });
                        } else {
                            drop(source);
                        }
                        let cell = shared_component_meta_analysis(analysis);
                        cell.pin(ChargeCtx::new(&account));
                        let published = account.snapshot().pinned_bytes;
                        assert!(published >= text.len(), "({label}) the source is charged");
                        let kept = if registry {
                            let SourcePosition::Present(source) =
                                &cell.type_registry[0].type_source
                            else {
                                panic!("({label}) a present registry source");
                            };
                            nested_leaf(source, name_only)
                        } else {
                            match cell.props[0].publication.authority().outcome() {
                                ResolvedTypeOutcome::Present { source, .. } => {
                                    nested_leaf(source, name_only)
                                }
                                other => panic!("({label}) a present source: {other:?}"),
                            }
                        };
                        assert_eq!(
                            kept.charge_class(),
                            Some(ChargeClass::Pinned),
                            "({label}) the nested allocation is charged by its own cell"
                        );
                        assert_eq!(account.snapshot().pinned_bytes, published);
                        if leaf_first {
                            drop(kept);
                            assert_eq!(account.snapshot().pinned_bytes, published);
                            drop(cell);
                        } else {
                            drop(cell);
                            let surviving = account.snapshot().pinned_bytes;
                            assert!(
                                surviving >= text.len() && surviving < published,
                                "({label}) the kept leaf owns its allocation, not the \
                                 analysis: {surviving} of {published}"
                            );
                            if matches!(kept, Leaf::Name(_)) {
                                assert_eq!(
                                    surviving,
                                    OwnedStr::charge_for_len(text.len()),
                                    "({label}) a kept name is charged exactly its own allocation"
                                );
                            }
                            drop(kept);
                        }
                        assert_eq!(
                            account.snapshot().total_bytes(),
                            0,
                            "({label}) final release is immediate"
                        );
                    }
                }
            }
        }
    }
}
