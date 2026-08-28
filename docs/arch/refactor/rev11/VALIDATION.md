# Rev11 validation

The enforced `docs-domain` final proof is intentionally bounded. It covers strict authority, amendment custody, generated freshness, source-pack parity, the closed post-ORC0 GitHub control-plane topology and finding-retention law, native-checker-family closure, exact legacy-architecture transfer/deletion completeness, and the rendered documentation build.

Run:

```text
node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict
pnpm docs:build
```

`validate-program-dag.mjs --strict` validates the 268-node authority and 268 materialized charters, all generated projections, the 58 source-pack nodes, the exact 13-node GH/FB/REL topology and ORC0 activation boundary, the 30-slice native-checker manifest, the 418-path cleanup, and the append-only amendment/authority-lock chain. `pnpm docs:build` proves the user-facing documentation renders. The retired exhaustive docs runner is not part of a gate profile and is not retained as a live tool.

For iteration before external ratification, `validate-successor-candidate.mjs` validates the complete static candidate and generated projections with amendment custody disabled. That result is not a landing verdict. The strict final command enables amendment custody; a missing trusted ratification slot, append-only amendment receipt, authority-lock advance, or ORC0 activation binding remains a hard failure. Lifecycle, custody, negative-control, and tooling tests remain targeted tests for changes to those surfaces instead of being replayed for every docs-domain acceptance.

No heavy Rust gate is part of this documentation authority cutover. Candidate nodes still owe their declared Rust, TypeScript, product, performance, and confirmation evidence.
