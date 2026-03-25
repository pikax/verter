//! Benchmarks for pathological TypeExpr evaluation and cache-hit reuse.

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use verter_analysis::type_eval::{evaluate, EvalEnv, TypeDeclInfo, TypeDeclKind};
use verter_analysis::type_expr::{
    ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr, TypeParam,
};

fn build_pathological_env() -> EvalEnv {
    let mut env = EvalEnv::new();

    env.add_type(TypeDeclInfo {
        name: "Leaf".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "val".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
    });

    env.add_type(TypeDeclInfo {
        name: "Mid".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "left".to_string(),
                    ty: TypeExpr::named_with_args("Leaf", vec![TypeExpr::named("T")]),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "right".to_string(),
                    ty: TypeExpr::named_with_args("Leaf", vec![TypeExpr::named("T")]),
                    optional: false,
                    readonly: false,
                }),
            ],
        })),
    });

    let properties: Vec<ObjectMember> = (0..50)
        .map(|index| {
            ObjectMember::Property(ObjectProperty {
                name: format!("prop{index}"),
                ty: TypeExpr::named_with_args(
                    "Mid",
                    vec![TypeExpr::Primitive(PrimitiveName::String)],
                ),
                optional: false,
                readonly: false,
            })
        })
        .collect();

    env.add_type(TypeDeclInfo {
        name: "Big".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties })),
    });

    env
}

fn bench_type_eval_pathological_first_pass(c: &mut Criterion) {
    c.bench_function("type_eval_pathological_first_pass", |b| {
        b.iter_batched(
            build_pathological_env,
            |mut env| {
                black_box(evaluate(&TypeExpr::named("Big"), &mut env));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_type_eval_pathological_cache_hit(c: &mut Criterion) {
    c.bench_function("type_eval_pathological_cache_hit", |b| {
        b.iter_batched(
            || {
                let mut env = build_pathological_env();
                let first = evaluate(&TypeExpr::named("Big"), &mut env);
                (env, first)
            },
            |(mut env, first)| {
                black_box(first);
                black_box(evaluate(&TypeExpr::named("Big"), &mut env));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_type_eval_pathological_first_pass,
    bench_type_eval_pathological_cache_hit,
);
criterion_main!(benches);
