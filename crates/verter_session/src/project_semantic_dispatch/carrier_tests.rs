use super::*;

#[test]
fn carrier_resolver_context_bundles_resolution_inputs() {
    // Construct from the same read-only inputs the eager `Ref` path uses,
    // and assert every accessor returns the wired value.
    let mut env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
    env.insert("T".to_string(), SemanticNodeId(11));
    let mut name_resolution: FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity> =
        FxHashMap::default();
    name_resolution.insert(
        Arc::from("Foo"),
        ResolvedRootIdentity::new("/foo.ts", "Foo"),
    );
    let scope = NodeScopeId::Global;
    let shadowing = ScopeShadowing::empty();
    let reduction = ProjectionReductionContext::published(ProjectionMode::Navigate);

    let ctx =
        CarrierResolverContext::new(&env, &scope, &name_resolution, None, &shadowing, reduction);

    assert_eq!(ctx.env().get("T"), Some(&SemanticNodeId(11)));
    assert!(matches!(ctx.scope(), NodeScopeId::Global));
    assert_eq!(
        ctx.name_resolution()
            .get("Foo")
            .map(|r| r.symbol_name.as_ref()),
        Some("Foo")
    );
    assert!(ctx.scope_payload().is_none());
    // The shadow set is the empty set here (no userland shadow).
    let _ = ctx.shadowing();
    assert_eq!(ctx.mode(), ProjectionMode::Navigate);
    assert_eq!(ctx.reduction_context().mode, ProjectionMode::Navigate);
}
