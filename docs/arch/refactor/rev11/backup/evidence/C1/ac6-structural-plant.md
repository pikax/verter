# C1-AC-6 structural mutation plant

## Guard

`crates/verter_session/src/resolver_core/resolver_context.rs::request_bound_lifecycles_share_one_resolver_context_implementation`

The guard requires exactly one generic
`impl<L: RequestBoundLifecycle> ResolverContext for RequestBoundAdapter<L>` and rejects a
lifecycle-local `ResolverContext` implementation in either
`host_resolver_context.rs` or `session_resolver_context.rs`.

## Baseline and plant application

Baseline search:

```text
rg -F "impl<'a> ResolverContext for" \
  crates/verter_session/src/resolver_core/host_resolver_context.rs
```

Result: zero matches.

Temporary plant applied with `apply_patch`:

```rust
#[cfg(any())]
impl<'a> ResolverContext for HostResolverContext<'a> {}
```

The false `cfg` keeps the plant out of type checking while presenting the exact duplicated
implementation shape the structural guard owns. Post-application search found exactly one new
match at line 152, and `git diff` showed only those three planted lines.

## RED

```text
cargo nextest run -p verter_session \
  request_bound_lifecycles_share_one_resolver_context_implementation
```

Result: RED as required. Nextest run `b0d33163-8af4-4e59-bb39-08470ee6f592` ran one test and
failed exactly at the owned assertion:

```text
host lifecycle reintroduced a duplicated ResolverContext implementation
```

An earlier invocation was discarded before discrimination because the preceding comment-cleanup
commit exposed an unrelated format-macro compile error. That diagnostic formatting was corrected
additively without changing its emitted bytes; the qualifying RED above compiled successfully and
failed only the planted structural assertion.

## Revert and GREEN

The three-line plant was removed with `apply_patch`. The exact search then returned zero matches,
and `git diff` proved `host_resolver_context.rs` had no residual change.

The same command returned GREEN in nextest run
`b00c720a-5906-40a6-9d2a-bcf7de5515e4`: 1 passed, 8,788 skipped.

RESULT: PASS — PLANT APPLIED, DISCRIMINATED, REVERTED, AND GREEN
