//! THE signature observation corpus ROW TABLE — the pinned-oracle evidence lock's
//! hand-authored observation set, measured against the pinned oracle
//! (TypeScript 7.0.2 `tsc`, `--noEmit --strict --ignoreConfig` for the
//! `checker` column and `--declaration --emitDeclarationOnly --strict`
//! for the `decl_emit` bytes; see
//! `docs/evidence/signature-kernel/manifest.json` for the toolchain
//! digests and the corpus identity).
//!
//! APPEND-ONLY, in the u6 corpus style: adding a row is ONE `Row` literal
//! appended here; the driver lives in `signature_corpus_tests.rs`.
//!
//! This corpus carries its OWN identity (`verter-signature-corpus-v0`):
//! it is NOT the reported 9,300-union / 56,548-observation corpus, whose
//! artifacts were not recovered (recorded in the manifest). Every
//! observation below was measured on the installed 7.0.2 platform
//! package whose executable and `lib/*.d.ts` digests the toolchain
//! record pins.
//!
//! Each row is one witness module plus a type-level probe expression.
//! The `checker` column is what 7.0.2 prints for the probe through the
//! two-step wrapper (`declare const __v: <probe>; export const __shape:
//! null = __v;`), with `IsAny`/`IsNever` legs proving the `any`/`never`
//! answers whose shape leg is legitimately silent. `decl_emit` is the
//! recorded `--declaration --emitDeclarationOnly` output for the
//! witness module — the canonical SIGNATURE observation (binders,
//! constraints, defaults, predicates, rest/receiver layout, grouping
//! order) in executable bytes. `diagnostic` records a checker
//! DIAGNOSTIC as the observation where the checker refuses to print a
//! type at all (TS2589 on the recursive thenable).

/// One recorded observation.
pub(crate) struct Row {
    pub(crate) id: &'static str,
    /// The observation family the row witnesses.
    pub(crate) family: Family,
    /// The witness module, spliced verbatim into the driver's lanes.
    pub(crate) source: &'static str,
    /// The type-level probe expression the observation is taken over.
    pub(crate) probe: &'static str,
    /// What 7.0.2 prints for the probe (RECORDED — never re-run in-tree).
    pub(crate) checker: &'static str,
    /// The probe is `any`: the shape leg is silent and the `IsAny` leg
    /// fires `true` (the corpus's any convention).
    pub(crate) checker_is_any: bool,
    /// The probe is `never`: the shape leg is silent and the `IsNever`
    /// leg fires `true`.
    pub(crate) checker_is_never: bool,
    /// The checker column is a DISPLAY-ONLY instantiation of the row's
    /// binders: the checker prints each binder at its constraint
    /// (`unknown`) purely for display, so the semantic claim (the
    /// dedup and the literal/generic arm ORDER) lives only in the
    /// `decl_emit` signature return — which is this row's live
    /// comparison basis, compared ORDER-SENSITIVELY (see
    /// `signature_corpus_tests.rs`).
    pub(crate) checker_display_only: bool,
    /// A checker DIAGNOSTIC recorded as the observation (the checker
    /// refuses to print a type for this probe).
    pub(crate) diagnostic: Option<&'static str>,
    /// The recorded `--declaration --emitDeclarationOnly --strict`
    /// output bytes for the witness module.
    pub(crate) decl_emit: &'static str,
    /// The current implementation's recorded verdict against the
    /// observation (see `signature_corpus_tests.rs`).
    pub(crate) verdict: Verdict,
}

/// The observation families the corpus covers
/// (`docs/arch/signature-kernel.md` §14 mandatory matrix).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    /// Union-valued `then`.
    UnionValuedThen,
    /// Construct-signature and mixin intersections.
    ConstructMixin,
    /// Generic defaults, defaults referencing other binders, type
    /// predicates, assertion signatures.
    DefaultsPredicates,
    /// Both substitution stages (outer/declared map and call-site map,
    /// nested instantiation, constrained substitution).
    SubstitutionStages,
    /// Preserved (reduction-state witness) and transparent intersection
    /// groups.
    IntersectionGroups,
    /// One row per Awaited residual family (ten).
    AwaitedResidual,
    /// Order-heavy generic unions (dedup, mixed literal/generic order).
    OrderHeavyGenericUnions,
}

impl Family {
    pub(crate) const ALL: &'static [Family] = &[
        Family::UnionValuedThen,
        Family::ConstructMixin,
        Family::DefaultsPredicates,
        Family::SubstitutionStages,
        Family::IntersectionGroups,
        Family::AwaitedResidual,
        Family::OrderHeavyGenericUnions,
    ];

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::UnionValuedThen => "union_valued_then",
            Self::ConstructMixin => "construct_mixin",
            Self::DefaultsPredicates => "defaults_predicates",
            Self::SubstitutionStages => "substitution_stages",
            Self::IntersectionGroups => "intersection_groups",
            Self::AwaitedResidual => "awaited_residual",
            Self::OrderHeavyGenericUnions => "order_heavy_generic_unions",
        }
    }
}

/// The current implementation's verdict for one row. A LATER BLOCK THAT
/// CHANGES AN ANSWER FLIPS THE ROW: `MatchesChecker` rows fail when the
/// live answer stops matching, and owed/degraded rows fail when the
/// live answer STARTS matching — both force a deliberate re-pin, never
/// a prose report.
#[derive(Clone, Debug)]
#[allow(dead_code)] // `Degraded` is the registered vocabulary's third verdict; rows land in it
                    // when the substrate publishes a typed degraded answer.
pub(crate) enum Verdict {
    /// The live answer equals the recorded observation today.
    MatchesChecker,
    /// The answer is owed to a named successor block; the row asserts
    /// the current implementation does NOT silently match.
    KnownOwed { note: &'static str },
    /// The current implementation publishes a typed degraded answer.
    Degraded { note: &'static str },
}

/// THE corpus.
pub(crate) const CORPUS: &[Row] = &[
    Row {
        id: "SV01_union_valued_then",
        family: Family::UnionValuedThen,
        source: "interface ThenableUnion { then(onfulfilled: (v: number | string) => void): void }\nexport function witness() { const t: ThenableUnion = null as any; return t; }",
        probe: "Awaited<ReturnType<typeof witness>>",
        checker: "string | number",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "interface ThenableUnion {\n    then(onfulfilled: (v: number | string) => void): void;\n}\nexport declare function witness(): ThenableUnion;\nexport {};\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV02_then_union_mixed_arm",
        family: Family::UnionValuedThen,
        source: "export function witness(v: Promise<number> | string) { return v; }",
        probe: "Awaited<ReturnType<typeof witness>>",
        checker: "string | number",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function witness(v: Promise<number> | string): string | Promise<number>;\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV03_construct_intersection",
        family: Family::ConstructMixin,
        source: "interface A { a: 1 }\ninterface B { b: 2 }\ndeclare const CtorA: new () => A;\ndeclare const CtorB: new () => B;\nexport function witness() { const x: InstanceType<typeof CtorA & typeof CtorB> = null as any; return x; }",
        probe: "ReturnType<typeof witness>",
        checker: "B",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "interface B {\n    b: 2;\n}\nexport declare function witness(): B;\nexport {};\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV04_mixin_intersection",
        family: Family::ConstructMixin,
        source: "interface Base { label: string }\ndeclare const BaseCtor: new (...args: any[]) => Base;\nexport function Mixin<S extends new (...args: any[]) => Base>(Base: S) { return class extends Base { extra = 1; }; }\nexport function witness() { const x: InstanceType<ReturnType<typeof Mixin<typeof BaseCtor>>> = null as any; return x; }",
        probe: "ReturnType<typeof witness>",
        checker: "Mixin.(Anonymous class) & Base",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "interface Base {\n    label: string;\n}\nexport declare function Mixin<S extends new (...args: any[]) => Base>(Base: S): {\n    new (...args: any[]): {\n        extra: number;\n        label: string;\n    };\n} & S;\nexport declare function witness(): {\n    extra: number;\n    label: string;\n} & Base;\nexport {};\n",
        verdict: Verdict::KnownOwed { note: "Mixin intersection witness (Mixin.(Anonymous class) & Base): the anonymous-class instance plus base intersection. The live rail publishes a typed gap instead (measured `Opaque(Miss)`, degraded). Owed by the `SignaturesOfType` construct-signature/mixin semantics." },
    },
    Row {
        id: "SV05_generic_default",
        family: Family::DefaultsPredicates,
        source: "type WithDefault<T = string> = { value: T };\nexport function witness() { const x: WithDefault = null as any; return x; }",
        probe: "ReturnType<typeof witness>",
        checker: "WithDefault<string>",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "type WithDefault<T = string> = {\n    value: T;\n};\nexport declare function witness(): WithDefault<string>;\nexport {};\n",
        verdict: Verdict::KnownOwed { note: "Generic DEFAULT application (WithDefault bare -> WithDefault<string>): 7.0.2 applies the declared default; the consumer-expanded answer is the BARE alias reference (measured `DeclRef(WithDefault)`), so the default is never applied. Owed by the binder-space default application." },
    },
    Row {
        id: "SV06_default_references_binder",
        family: Family::DefaultsPredicates,
        source: "type Chain<T, U = T[]> = { self: T; others: U };\nexport function witness() { const x: Chain<number> = null as any; return x; }",
        probe: "ReturnType<typeof witness>",
        checker: "Chain<number, number[]>",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "type Chain<T, U = T[]> = {\n    self: T;\n    others: U;\n};\nexport declare function witness(): Chain<number, number[]>;\nexport {};\n",
        verdict: Verdict::KnownOwed { note: "A default REFERENCING ANOTHER BINDER (U = T[]): the recorded observation is the defaulted instantiation Chain<number, number[]>; the consumer-expanded answer is the alias EXPANSION instead (measured `{ self: number, others: Array(number) }`), losing the alias-applied form the checker prints. Owed by the binder-space default application." },
    },
    Row {
        id: "SV07_type_predicate",
        family: Family::DefaultsPredicates,
        source: "interface Foo { kind: 'foo'; n: number }\nexport function isFoo(x: unknown): x is Foo { return true; }\nexport function witness() { return isFoo; }",
        probe: "ReturnType<typeof witness>",
        checker: "(x: unknown) => x is Foo",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "interface Foo {\n    kind: 'foo';\n    n: number;\n}\nexport declare function isFoo(x: unknown): x is Foo;\nexport declare function witness(): typeof isFoo;\nexport {};\n",
        verdict: Verdict::KnownOwed { note: "A TYPE-PREDICATE signature print ((x: unknown) => x is Foo): the consumer-expanded answer is an EMPTY surface (measured `{  }`, degraded) — the callable and its predicate are both lost. Predicate propagation into signature observations is owed by the `SignaturesOfType` result projection." },
    },
    Row {
        id: "SV08_assertion_signature",
        family: Family::DefaultsPredicates,
        source: "interface Bar { kind: 'bar' }\nexport function assertBar(x: unknown): asserts x is Bar { }\nexport function witness() { return assertBar; }",
        probe: "ReturnType<typeof witness>",
        checker: "(x: unknown) => asserts x is Bar",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "interface Bar {\n    kind: 'bar';\n}\nexport declare function assertBar(x: unknown): asserts x is Bar;\nexport declare function witness(): typeof assertBar;\nexport {};\n",
        verdict: Verdict::KnownOwed { note: "An ASSERTION signature print (asserts x is Bar): the consumer-expanded answer is an EMPTY surface (measured `{  }`, degraded) — the same `SignaturesOfType` result-projection gap as the predicate twin." },
    },
    Row {
        id: "SV09_explicit_type_arguments",
        family: Family::SubstitutionStages,
        source: "export function identity<T>(v: T): T { return v; }\nexport function witness() { return identity<number>(1); }",
        probe: "ReturnType<typeof witness>",
        checker: "number",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function identity<T>(v: T): T;\nexport declare function witness(): number;\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV10_nested_instantiation",
        family: Family::SubstitutionStages,
        source: "type F<T> = { f: T };\ntype G<U> = F<U[]>;\nexport function witness() { const x: G<boolean> = null as any; return x; }",
        probe: "ReturnType<typeof witness>",
        checker: "G<boolean>",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "type F<T> = {\n    f: T;\n};\ntype G<U> = F<U[]>;\nexport declare function witness(): G<boolean>;\nexport {};\n",
        verdict: Verdict::KnownOwed { note: "The nested-instantiation probe REDUCES now, but to the alias's EXPANSION (measured `{ f: Array(boolean) }`) where 7.0.2 prints the alias-applied `G<boolean>`. Preserving the alias-applied display through a type-position instantiation is owed by the `SignaturesOfType` result projection." },
    },
    Row {
        id: "SV11_constrained_substitution",
        family: Family::SubstitutionStages,
        source: "export function pickA<T extends { a: 1 }>(v: T): T['a'] { return v.a; }\nexport function witness() { return pickA({ a: 1, extra: 'x' } as { a: 1; extra: string }); }",
        probe: "ReturnType<typeof witness>",
        checker: "1",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function pickA<T extends {\n    a: 1;\n}>(v: T): T['a'];\nexport declare function witness(): 1;\n",
        verdict: Verdict::KnownOwed { note: "Constrained substitution through an indexed access (T['a'] over T extends { a: 1 }): the checker reduces to the literal 1; the consumer-expanded answer is an EMPTY surface (measured `{  }`), so the indexed access is never forced through the constraint. Owed by the constrained-substitution stage." },
    },
    Row {
        id: "SV12_grouping_witness_L",
        family: Family::IntersectionGroups,
        source: "type L<T extends string> = (number & T) & { x: 1 };\nexport function witness() { const x: L<'a'> = null as any as L<'a'>; return x; }",
        probe: "ReturnType<typeof witness>",
        checker: "never",
        checker_is_any: false,
        checker_is_never: true,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function witness(): never;\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV13_grouping_witness_R",
        family: Family::IntersectionGroups,
        source: "type R<T extends string> = number & (T & { x: 1 });\nexport function witness() { const x: R<'a'> = null as any as R<'a'>; return x; }",
        probe: "ReturnType<typeof witness>",
        checker: "never",
        checker_is_any: false,
        checker_is_never: true,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function witness(): never;\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV14_transparent_group",
        family: Family::IntersectionGroups,
        source: "export function witness() { const x: { a: 1 } & { b: 2 } = null as any; return x; }",
        probe: "ReturnType<typeof witness>",
        checker: "{ a: 1; } & { b: 2; }",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function witness(): {\n    a: 1;\n} & {\n    b: 2;\n};\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV15_awaited_plain_primitive",
        family: Family::AwaitedResidual,
        source: "export function witness() { return 1 as number; }",
        probe: "Awaited<ReturnType<typeof witness>>",
        checker: "number",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function witness(): number;\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV16_awaited_plain_object",
        family: Family::AwaitedResidual,
        source: "export function witness() { return { a: 1 }; }",
        probe: "Awaited<ReturnType<typeof witness>>",
        checker: "{ a: number; }",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function witness(): {\n    a: number;\n};\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV17_awaited_any",
        family: Family::AwaitedResidual,
        source: "export function witness(v: any) { return v; }",
        probe: "Awaited<ReturnType<typeof witness>>",
        checker: "any",
        checker_is_any: true,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function witness(v: any): any;\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV18_awaited_never",
        family: Family::AwaitedResidual,
        source: "export function witness(): never { throw new Error('x'); }",
        probe: "Awaited<ReturnType<typeof witness>>",
        checker: "never",
        checker_is_any: false,
        checker_is_never: true,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function witness(): never;\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV19_awaited_union_arm",
        family: Family::AwaitedResidual,
        source: "export function witness(v: Promise<number> | string) { return v; }",
        probe: "Awaited<ReturnType<typeof witness>>",
        checker: "string | number",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function witness(v: Promise<number> | string): string | Promise<number>;\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV20_awaited_non_thenable_then",
        family: Family::AwaitedResidual,
        source: "interface WeirdThen { then(onfulfilled: (v: { nested: 1 }) => void, unused: number): void }\nexport function witness() { const t: WeirdThen = null as any; return t; }",
        probe: "Awaited<ReturnType<typeof witness>>",
        checker: "{ nested: 1; }",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "interface WeirdThen {\n    then(onfulfilled: (v: {\n        nested: 1;\n    }) => void, unused: number): void;\n}\nexport declare function witness(): WeirdThen;\nexport {};\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV21_awaited_recursive_thenable",
        family: Family::AwaitedResidual,
        source: "interface Rec { then(onfulfilled: (v: Rec) => void): void }\nexport function witness() { const t: Rec = null as any; return t; }",
        probe: "Awaited<ReturnType<typeof witness>>",
        checker: "",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: Some("Type instantiation is excessively deep and possibly infinite."),
        decl_emit: "interface Rec {\n    then(onfulfilled: (v: Rec) => void): void;\n}\nexport declare function witness(): Rec;\nexport {};\n",
        verdict: Verdict::KnownOwed { note: "Recursive thenable: 7.0.2 itself refuses with diagnostic TS2589 — the recorded OBSERVATION is the diagnostic, not a type print. The rail also declines to answer: the structural-fact demand measures `Opaque(Miss)` — the substrate does not fabricate a recovery type — and the row pins that NON-ANSWER, not any particular termination mechanism. Whether the shared family cycle guard is what terminated it is NOT established by this row; a checker-faithful recovery disposition is owed." },
    },
    Row {
        id: "SV22_awaited_nested_promise",
        family: Family::AwaitedResidual,
        source: "export function witness(v: Promise<Promise<number>>) { return v; }",
        probe: "Awaited<ReturnType<typeof witness>>",
        checker: "number",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function witness(v: Promise<Promise<number>>): Promise<Promise<number>>;\n",
        verdict: Verdict::MatchesChecker,
    },
    Row {
        id: "SV23_awaited_deferred_generic",
        family: Family::AwaitedResidual,
        source: "export function witness<T>(v: Awaited<T>) { return v; }",
        probe: "ReturnType<typeof witness>",
        checker: "unknown",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: true,
        diagnostic: None,
        decl_emit: "export declare function witness<T>(v: Awaited<T>): Awaited<T>;\n",
        verdict: Verdict::KnownOwed { note: "Deferred generic: the witness signature is (v: Awaited<T>) => Awaited<T>; instantiating the binder at its constraint prints unknown. The consumer-expanded answer is `unknown`, which does not carry the declared-return structure this row compares against. Deferred-symbolic instantiation through the constraint is owed." },
    },
    Row {
        id: "SV24_awaited_constrained_generic",
        family: Family::AwaitedResidual,
        source: "export async function witness<T extends Promise<number>>(v: T) { return v; }",
        probe: "ReturnType<typeof witness>",
        checker: "Promise<Promise<number>>",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: None,
        decl_emit: "export declare function witness<T extends Promise<number>>(v: T): Promise<T>;\n",
        verdict: Verdict::KnownOwed { note: "Constrained generic async wrap: Promise<Promise<number>> (the body returns v: T unreduced); the consumer-expanded answer is a typed gap instead (measured `Opaque(Miss)`). The async-wrap/constraint interaction is owed by the runtime/lib `Awaited` lane." },
    },
    Row {
        id: "SV25_generic_union_dedup",
        family: Family::OrderHeavyGenericUnions,
        source: "type A<T> = { ka: T };\ntype B<T> = { kb: T };\nexport function witness<T>(v: A<T> | B<T> | A<T>) { return v; }",
        probe: "ReturnType<typeof witness>",
        checker: "A<unknown> | B<unknown>",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: true,
        diagnostic: None,
        decl_emit: "type A<T> = {\n    ka: T;\n};\ntype B<T> = {\n    kb: T;\n};\nexport declare function witness<T>(v: A<T> | B<T> | A<T>): A<T> | B<T>;\nexport {};\n",
        verdict: Verdict::KnownOwed { note: "Order-heavy generic union with a duplicate arm: 7.0.2 deduplicates A<T> | B<T> | A<T> to A<T> | B<T> preserving FIRST occurrence; the live rail dedups but REVERSES the authored arm order (measured `B | A`). Authored-precedence-preserving union reduction is owed by `ReduceUnion`." },
    },
    Row {
        id: "SV26_literal_generic_union_order",
        family: Family::OrderHeavyGenericUnions,
        source: "export function witness<T>(v: 'a' | T | 'b' | 'a') { return v; }",
        probe: "ReturnType<typeof witness>",
        checker: "unknown",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: true,
        diagnostic: None,
        decl_emit: "export declare function witness<T>(v: 'a' | T | 'b' | 'a'): \"a\" | \"b\" | T;\n",
        verdict: Verdict::KnownOwed { note: "Literal/generic mixed union order: 7.0.2 prints a | b | T — literals first in authored order, the generic binder last, the duplicate a dropped. `VerterStableV1` ordering over mixed literal/generic unions is owed by `ReduceUnion`." },
    },
];
