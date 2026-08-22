# J1 evidence — Svelte CSS grammar scope consult

**Consulted:** independent architecture review (codex, read-only, high reasoning effort), 2026-08-20,
against `block/j1` at the worktree checkout of `program/architecture-lock` tip
`9531d625fc2cbf02a2de76feb31f5f519fd8de6c`.

**Question posed:** is `crates/verter_compiler/src/svelte/runtime/css/{parse,types,analyze,match,hash,render}.rs`
— a separate, independently-implemented, span-bearing CSS-family parser/AST (a byte-faithful port of
`svelte@5.56.3`'s own CSS reader/scoper, guarded against Vue/lightningcss reuse by
`svelte_css_no_vue_reuse_guard.rs`) in scope for J1's "sole CSS-family syntax authority" mandate, or is
it a legitimately distinct, non-legacy, product-owned grammar outside the directive's target?

## Ruling: in scope, disposition Converge

> The Svelte-owned CSS grammar is in scope for J1. The directive's controlling requirement is that
> `verter_css_syntax::StyleSyntaxIr` become the sole CSS-family syntax authority. That requirement is
> architectural, not limited to the lightningcss-backed `processStyle` implementation. A live,
> framework-specific parser remains a second syntax authority even if it is non-legacy,
> conformance-pinned, and predates the cutover.
>
> - J1 may proceed by chartering this work as **Converge**.
> - **Preserve** and **Defer** would be deviations requiring explicit maintainer ratification.
> - Silently treating it as out of scope would contradict both the sole-authority mandate and J1's
>   abort/rescope clause.
> - The existing evidence already satisfies the abort clause's condition: there are presently multiple
>   semantic syntax authorities.
>
> The prior ruling's "Genuine open questions" section does not implicitly exempt this newly discovered
> fifth issue; it simply did not adjudicate it.

### Required disposition: Converge

- Delete `parse.rs` and the grammar-owning portions of `types.rs`.
- Extend `StyleSyntaxIr` as necessary to carry lossless spans, dialect distinctions, raw-source
  fidelity, and other facts required for Svelte 5.56.3 compatibility.
- Make Svelte analysis and scoping consume that canonical IR.
- Retain `analyze.rs`, `match.rs`, `hash.rs` only as framework-specific policy that cannot tokenize,
  reparse, classify new syntax, or maintain competing grammar nodes.
- Replace `render.rs`'s grammar-level printing responsibility with source-preserving `CodeTransform`
  edits or canonical shared serialization — it cannot remain an independent CSS printer.
- Remove the second parse currently performed by `validate_svelte_style_ir`; after convergence there
  must be one authoritative parse, not an admission parse followed by the real parse.
- Preserve the guard's legitimate architectural intent (Svelte must not route through Vue/lightningcss)
  but rewrite it to prohibit compiler-local CSS parsing/grammar authorship instead of prohibiting reuse
  of the shared syntax authority.

### Acceptance criteria

1. Repository inventory finds no CSS-family tokenizer, parser, grammar AST, normalizer, or
   grammar-level printer outside `verter_css_syntax`.
2. Svelte CSS performs exactly one canonical parse per style block.
3. Svelte's transform is byte-for-byte compatible with the pinned Svelte 5.56.3 corpus, including
   hashing, selector scoping, global/local constructs, nesting, keyframes, comments, malformed-input
   behavior, and source spans/maps.
4. Preserved Svelte modules consume canonical syntax facts and cannot independently decide whether a
   byte sequence constitutes CSS syntax.
5. Equivalent-work evidence shows convergence does not hide a second parse, reconstruct source merely
   to parse again, or duplicate complete-tree traversal without documented semantic need.
6. The (rewritten) guard test simultaneously proves no reuse of the Vue/lightningcss pipeline AND no
   compiler-local Svelte CSS grammar or reparse.

Failure to meet parity is not grounds for silently preserving the parser — it triggers J1's
abort/rescope path and requires maintainer ratification for an exception.

Full raw transcript: `/tmp/j1-codex-svelte-scope.log` (not committed; this file is the durable record).
