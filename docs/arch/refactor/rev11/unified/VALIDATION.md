# Unified Rev11 validation record

This record is updated only from final command output. The final authority is intentionally source-canonical: 326 mechanically split or invented recovery nodes are disposition-mapped into independently acceptable owning nodes or removed. It has 197 nodes, 461 edges, 38 modules, and 197 charters. It carries 2,339 exact source clauses across 203 applicable targets and 50 locked live Rev11 inputs. Unverifiable recovery-count claims are intentionally absent.

Required final checks:

```text
node docs/arch/refactor/rev11/unified/tools/run-docs-gate.mjs
```

The aggregate gate discovers and syntax-checks every `.mjs`, runs every builder in freshness mode, runs strict graph/charter/orchestration validation, executes all canonical negative controls and self-test, and discovers every substantive Node test. Those tests prove that exact committed bytes reach J1 `LANDED_GRANDFATHERED`, ORC0 concurrent admission, dispatch, finalization, and the narrowly scoped directive authorization. They then prove the honest production boundary: locally fabricated gate/review PASS artifacts are not importable, review execution requires a separately ratified immutable reviewer capability, and the package remains in ORC0 rather than claiming activation while that external custody is absent. Other tests cover amendment creation/invalidation/revalidation, admission conflicts and capacity, product refusal before BR0, candidate/integration delta preservation, atomic no-replace imports, and deterministic output into independent roots. The final audit also verifies the three source files byte-for-byte and runs `git diff --check`.

No heavy Rust gate is part of this documentation/Node authority repair. Rust/TS/product domain gates remain mandatory candidate-bound evidence for the future implementation nodes they accept.
