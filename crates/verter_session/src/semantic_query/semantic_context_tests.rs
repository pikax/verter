use verter_semantic::resolver_core::{
    EnvHashes, RawSemanticCompilerOptions, SemanticCompilerOptions,
};

use super::semantic_context::{
    project_order_domain, project_union_order, SemanticContext, SemanticOrderPolicyId,
    SemanticPolicySet, SemanticUnionMembersKey,
};
use super::SemanticNodeId;

fn ctx_with(
    options: SemanticCompilerOptions,
    env: EnvHashes,
    policy: SemanticPolicySet,
    project_identity: [u8; 16],
) -> SemanticContext {
    SemanticContext {
        effective_semantic_options: options,
        resolver_library_project_environment: env,
        policy_set: policy.intern(),
        project_identity,
    }
}

#[test]
fn intern_is_exact_equality_digest_is_not_identity() {
    let a = ctx_with(
        SemanticCompilerOptions::default(),
        EnvHashes::default(),
        SemanticPolicySet::production(),
        [0; 16],
    );
    let b = a.clone();
    assert_eq!(a.clone().intern(), b.intern());
}

#[test]
fn umbrella_and_explicit_spellings_share_one_context_id() {
    let via_umbrella = RawSemanticCompilerOptions {
        strict: Some(true),
        ..RawSemanticCompilerOptions::default()
    }
    .effective();
    let via_members = RawSemanticCompilerOptions {
        strict_null_checks: Some(true),
        strict_function_types: Some(true),
        strict_bind_call_apply: Some(true),
        strict_property_initialization: Some(true),
        no_implicit_any: Some(true),
        no_implicit_this: Some(true),
        use_unknown_in_catch_variables: Some(true),
        always_strict: Some(true),
        ..RawSemanticCompilerOptions::default()
    }
    .effective();
    assert_eq!(via_umbrella, via_members);
    let env = EnvHashes::default();
    let policy = SemanticPolicySet::production();
    let umbrella = ctx_with(via_umbrella, env, policy, [1; 16]).intern();
    let explicit = ctx_with(via_members, env, policy, [1; 16]).intern();
    assert_eq!(umbrella, explicit);
}

#[test]
fn different_strictness_or_project_yields_different_context_ids() {
    let strict = SemanticCompilerOptions::default();
    let mut relaxed = strict.clone();
    relaxed.strict_null_checks = false;
    let env = EnvHashes::default();
    let policy = SemanticPolicySet::production();
    let a = ctx_with(strict.clone(), env, policy, [1; 16]).intern();
    let b = ctx_with(relaxed, env, policy, [1; 16]).intern();
    let c = ctx_with(strict, env, policy, [2; 16]).intern();
    assert_ne!(a, b);
    assert_ne!(a, c);
}

#[test]
fn leaf_key_projects_policy_from_context() {
    let ctx = ctx_with(
        SemanticCompilerOptions::default(),
        EnvHashes::default(),
        SemanticPolicySet::production(),
        [0; 16],
    );
    let key = SemanticUnionMembersKey::from_context(SemanticNodeId(1), &ctx);
    assert_eq!(key.policy(), project_union_order(&ctx));
    assert_eq!(key.domain(), project_order_domain(&ctx));
    assert_eq!(key.policy(), SemanticOrderPolicyId::VerterStableV1);
}

#[test]
fn policy_set_change_changes_context_and_leaf_identity() {
    let env = EnvHashes::default();
    let options = SemanticCompilerOptions::default();
    let production = SemanticPolicySet::production();
    let mut other = production;
    other.compatibility_version = 2;
    let ctx_a = ctx_with(options.clone(), env, production, [0; 16]);
    let ctx_b = ctx_with(options, env, other, [0; 16]);
    assert_ne!(ctx_a.clone().intern(), ctx_b.clone().intern());
    let leaf_a = SemanticUnionMembersKey::from_context(SemanticNodeId(7), &ctx_a);
    let leaf_b = SemanticUnionMembersKey::from_context(SemanticNodeId(7), &ctx_b);
    assert_ne!(leaf_a, leaf_b);
}

#[test]
fn formatting_only_option_spelling_does_not_change_context_id() {
    let a = RawSemanticCompilerOptions {
        strict: Some(true),
        target: Some("ES2025".into()),
        ..RawSemanticCompilerOptions::default()
    }
    .effective();
    let b = RawSemanticCompilerOptions {
        strict: Some(true),
        target: Some("es2025".into()),
        ..RawSemanticCompilerOptions::default()
    }
    .effective();
    assert_eq!(a, b);
    let env = EnvHashes::default();
    let policy = SemanticPolicySet::production();
    assert_eq!(
        ctx_with(a, env, policy, [0; 16]).intern(),
        ctx_with(b, env, policy, [0; 16]).intern()
    );
}
