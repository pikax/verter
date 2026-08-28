# Required package coverage

| required deliverable | package path |
|---|---|
| architecture amendment | `amendments/AMD-005-framework-compiler-conformance-rescope.md` |
| amended DAG | `program-dag.toml`; both program-state shapes |
| BF1/BF2/BF3/BV1/BS1 charters | `charters/{BF1,BF2,BF3,BV1,BS1}.md` |
| B2/B3/B4/B5/B6/C3/C4 amendments | `charters/{B2,B3,B4,B5,B6,C3,C4}.md` |
| exact version-domain manifest | `evidence/framework-conformance/version-domain.md`, `oracles/*` |
| product boundary and glossary | `contracts/framework-compiler-boundary.md` |
| product inventory | `evidence/framework-conformance/product-inventory.md` |
| Vue/Svelte option inventories | `evidence/framework-conformance/{vue,svelte}-options.tsv`, `option-inventories.md` |
| capability matrix | `evidence/framework-conformance/capability-matrix.tsv` |
| official-core oracle | `contracts/official-core-oracles.md` |
| language-tools exclusion | `contracts/language-tools-exclusion.md` |
| third-party exclusion | `contracts/third-party-exclusion.md` |
| conformance/goldens | `contracts/conformance-goldens.md` |
| official Vue/Svelte cases | `evidence/framework-conformance/{vue,svelte}-official-cases.tsv` |
| normalizer | `contracts/conformance-normalizer.md` |
| fragment/assembly | `contracts/fragment-assembly.md` |
| SSR/hydration | `contracts/ssr-hydration.md` |
| TypeScript products | `contracts/typescript-product-conformance.md` |
| emitter/mapping dispositions | `evidence/framework-conformance/emitter-mapping-dispositions.tsv` |
| BF3 scope | `evidence/framework-conformance/bf3-safety-retraction-scope.md` |
| performance impact | `evidence/framework-conformance/performance-impact.md` |
| program-state transition | `evidence/framework-conformance/program-state-transition.md` |
| exact ratification action | AMD-005 §15 |
| independent report placeholders | `evidence/framework-conformance/reviews/README.md` |
| validation | `evidence/framework-conformance/validation.md`, `validate-package.mjs` |

The three primary independent challenge reports are attached and bind the reviewed
pre-fix candidate `ce1d0e4688af1b5bd548b6b68286632cc0f7ede8`, tree
`1ff1f83d8e994b6f1169b0b209c9f557c23f4728`; all three carry blocking findings.
They are evidence for this fix round, not approval of changed bytes. Fresh independent
reports are required before ratification.

`validate-package.mjs --pre-review` preserves the preparer/challenger boundary by
requiring the primary report paths to be absent. `--post-review` instead requires the
three attachments and exact `--reviewed-commit`/`--reviewed-tree` binding. That mode
validates attachment identity and a closed verdict; it does not turn a blocking report
into acceptance. The ratification bundle must otherwise differ from the reviewed
package only at the three primary report paths.

No production compiler, runtime, test, CI, root dependency, or performance-gate
implementation was changed by this package.
