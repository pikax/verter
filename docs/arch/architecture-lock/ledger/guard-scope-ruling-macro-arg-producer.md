# Ruling — producer expansion-surface guard scope

## Verdict

**The guard is over-broad. `matches!` does not realise the stated threat.** It should not be banned merely because it is a bang macro.

However, simply adding `matches!` to a source-level allowlist would be unsound: an unqualified `matches!` can be shadowed, and `syn` neither expands nor resolves macros. The durable replacement must inspect the compiled expansion, not the macro’s spelling.

## 1. What these macros can actually introduce

The standard `matches!` expansion is exactly a `match` with `true` and `false` arms. Its scrutinee, pattern, and optional guard come from tokens visibly supplied at the call site; it introduces no call or item of its own. The official definition confirms that shape. [Rust core `matches!` source](https://doc.rust-lang.org/src/core/macros/mod.rs.html#423-433)

Therefore, in the predicate at [macro_arg_producer.rs:365](<REPO>/crates/verter_session/src/structural_carrier_producer/macro_arg_producer.rs:365), replacing the explicit match with:

```rust
matches!(
    graph.node_data(check).as_deref(),
    Some(SemanticNodeData::TypeParam { .. })
)
```

cannot introduce a builder call that was not textually present. The de-sugared form buys only one thing: it removes an expansion boundary that the current `syn` scanner cannot inspect. It buys no semantic confinement specific to `matches!`.

The common macros differ:

| Macro | Expansion-introduced behavior | Arbitrary local producer code? |
|---|---|---|
| `matches!` | A `match` and Boolean literals | No |
| `assert!`, `debug_assert!` | Conditional control flow and fixed panic/formatting machinery | No, except expressions supplied by the author |
| `todo!` | A fixed call into core panic machinery | No |
| `format!` | Formatting machinery, allocation, and formatting-trait dispatch | No arbitrary item/call, but it can execute user `Display`/`Debug` implementations |
| `write!` | A `write_fmt` method call plus formatting-trait dispatch | No arbitrary item, but it deliberately invokes receiver/formatting code; [the documentation says it calls `write_fmt`](https://doc.rust-lang.org/stable/core/macro.write.html) |
| `vec!` | Vector construction/allocation; the repeat form uses `Clone` | No arbitrary item, but it can execute a user `Clone` implementation; [the documentation expressly notes this](https://doc.rust-lang.org/stable/alloc/macro.vec.html) |

None of those standard macros can directly manufacture a call to these module-private builders from their standard definitions. By contrast, `include!`, an imported/custom `macro_rules!`, a function-like proc macro, a custom derive, or an attribute proc macro can emit arbitrary items and calls. Rust macros-by-example can expand to expressions, statements, items, traits, or impls, and non-local symbols in a macro definition are generally resolved at the invocation site. [Rust Reference: macro expansion and hygiene](https://doc.rust-lang.org/stable/reference/macros-by-example.html#hygiene)

The strongest counter-argument is real: the spelling `matches!` alone does not prove that the standard macro was selected. A malicious shadow macro could emit `lower_type_expr_structural(...)`. That justifies rejecting a by-name exception; it does not make the genuine core `matches!` expansion dangerous.

## 2. The guard enforces the wrong category

The current predicate at [architecture_guards.rs:20474](<REPO>/crates/verter_session/tests/cases/architecture_guards.rs:20474) is syntactic:

```text
violation := any production syn::Macro
```

The rationale is semantic:

```text
violation := an expansion introduces code that reaches a private producer builder
             without an authored reference to that builder
```

Those predicates are not equivalent. The guard should enforce the semantic predicate.

The mismatch is broader than `matches!`:

- It rejects all bang macros, despite permitting built-in derives that also generate calls such as `Clone::clone`.
- It treats every non-allowlisted attribute as a proc macro, incorrectly sweeping in inert built-in attributes such as `#[inline]`, `#[cold]`, `#[must_use]`, `#[repr]`, and `#[track_caller]`.
- It skips only exact `#[cfg(test)]`, so constructs under `#[cfg(all(test, unix))]` can false-positive despite being production-impossible.
- Its own prose is inconsistent: the implementation calls the scanner “NOT load-bearing,” while the registry/design documentation claims it closes the complete same-module residual.

Thus the classifier is simultaneously conservative, imprecise, and presented with more assurance than its mechanism supports.

## 3. Exact replacement predicate

Retire the blanket `visit_macro` rejection at [architecture_guards.rs:20625](<REPO>/crates/verter_session/tests/cases/architecture_guards.rs:20625), but **do not remove it before its replacement exists**.

The replacement guard text should be:

> In production HIR for `structural_carrier_producer::macro_arg_producer`, a macro expansion is forbidden when expansion-origin code introduces:
>
> 1. a resolved reference or call to `lower_type_expr_structural`, `build_macro_hot_ref`, or `build_script_setup_seed_frames`, or to a production definition in the resolved backward call/reference closure of those builders; or
> 2. a crate-visible function, value, trait, or trait implementation that constitutes an additional producer entry under `macro_hot_mirror_exposes_single_crate_visible_producer_entry`.
>
> Macro spelling is irrelevant. Tokens originating in call-site macro arguments retain call-site provenance and are not classified as hidden expansion output. An unresolved dynamic expansion edge fails closed.

Mechanically, this should be a compiler/HIR or equivalent expansion-aware tool check using resolved `DefId`s and expansion provenance. That makes it:

- name-independent;
- immune to aliases and macro shadowing;
- effective against declarative and procedural expansions;
- able to accept `matches!`, `assert!`, `format!`, etc. when their actual expansion does not reach the producer authority;
- able to reject a planted hostile macro regardless of what it is named.

That is a real guard. A growing list such as `matches | vec | assert | format` is not.

The discrimination companion at [architecture_guards.rs:20907](<REPO>/crates/verter_session/tests/cases/architecture_guards.rs:20907) should become compiled fixtures that:

- accept genuine `matches!`;
- accept a harmless differently named macro;
- reject a macro that expands to a direct builder reference;
- reject an alias-qualified expansion resolving to the same builder `DefId`;
- reject a macro-generated crate-visible producer entry;
- prove that an unqualified shadow macro is judged by its resolved expansion, not its name.

## 4. Standing lint conflict

The preferred resolution is an atomic change:

1. Land the expansion-aware semantic guard.
2. Replace the explicit match with idiomatic `matches!`.
3. Remove the explanatory workaround and `#[allow(clippy::match_like_matches_macro)]`.
4. Restore `vec![frame]` if desired; it is currently de-sugared for the same over-broad rule at [macro_arg_producer.rs:1153](<REPO>/crates/verter_session/src/structural_carrier_producer/macro_arg_producer.rs:1153).

Until that replacement exists, do **not** punch a `matches!` name exception into the `syn` scanner. Keep the ban and use a narrow lint suppression. The repository already has the suppression at [macro_arg_producer.rs:370](<REPO>/crates/verter_session/src/structural_carrier_producer/macro_arg_producer.rs:370), so it is not literally resolved by a comment alone. A better interim form is:

```rust
#[expect(
    clippy::match_like_matches_macro,
    reason = "the current unexpanded-source producer guard forbids every production macro invocation"
)]
```

`#[expect]` is preferable because it becomes a warning if the expected lint ceases to fire. That is still an interim reconciliation, not vindication of the guard’s scope.

## 5. Blast radius

There is only one structural-carrier producer-capable implementation module: `macro_arg_producer.rs`. Its only non-test sibling, `infer_binder_names.rs`, is explicitly non-producer-capable; the topology is documented in [structural_carrier_producer/mod.rs:1](<REPO>/crates/verter_session/src/structural_carrier_producer/mod.rs:1). Other files named `*_producer`, such as `authored_evidence_producer.rs` and `resolved_import_facts_producer.rs`, do not carry this guard and protect different invariants.

So this is not a repeated sibling guard. It is, however, one member of a six-guard family. Any fix must update all descriptions and registrations together:

- [architecture_guards.rs:20444](<REPO>/crates/verter_session/tests/cases/architecture_guards.rs:20444)
- [critical_rules_have_guards.rs:500](<REPO>/crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs:500)
- [handle_capable_consumer_guards.rs:1333](<REPO>/crates/verter_session/tests/cases/handle_capable_consumer_guards.rs:1333)
- [type-resolution/SKILL.md:1070](<REPO>/.claude/skills/type-resolution/SKILL.md:1070)
- [parselower-design.md:112](<REPO>/docs/arch/parselower-design.md:112)
- [parselower-design.md:142](<REPO>/docs/arch/parselower-design.md:142)
- the source comments around both current de-sugarings.

Do not mechanically relax the separate macro bans in `carrier_encapsulation_guards.rs`; those protect a different private-payload surface and require their own scope ruling.

**Final ruling:** over-broad on the merits; `matches!` is safe; a spelling-based exception is not. Replace the unexpanded syntax ban with an expansion-aware resolved-reference guard, then use the idiomatic macro and delete the lint workaround.

__DONE__
