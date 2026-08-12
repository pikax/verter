# Independent compiler-conformance challenge — impact-bounded reattestation 2

## Verdict

PASS — all findings resolved, no new blocking issue introduced, bound to commit
`7442bb9060b7faa0720e528d3f96ee1df1abff95`.

## Exact binding and scope

- Previous candidate: `ce1d0e4688af1b5bd548b6b68286632cc0f7ede8`, tree
  `1ff1f83d8e994b6f1169b0b209c9f557c23f4728`.
- Reattested candidate: `7442bb9060b7faa0720e528d3f96ee1df1abff95`, tree
  `69502487b55f87eb7c0c009876865b64397da660`.
- Branch: `work/framework-conformance-rescope`.

`HEAD`, `HEAD^{tree}`, and the two explicitly resolved candidate trees matched those
identities. This is an impact-bounded recheck of the two findings in
`conformance-challenge.md`, using the exact candidate-to-candidate diff requested by
dispatch. It is not a fresh review of the full package. No production or package file
was modified by this review; the only write is this new report.

## Independent pinned-source re-resolution

I re-resolved Svelte from a clean detached local checkout/object store. Its configured
`origin` is `https://github.com/sveltejs/svelte.git`, its worktree has no tracked or
untracked change, and `git fsck --full --strict` completed successfully. The annotated
tag chain is:

- tag `refs/tags/svelte@5.56.8`: object
  `a49603bbb50f948fd0c2bf5c55582a8f89b4d91c`;
- peeled commit: `44a7813730579b94004e182e5a67aab27aa9d2a6`;
- commit tree: `63390158bfe8f997c474e35215a4fa627194c229`.

The exact pinned blobs inspected were:

| upstream path | blob |
|---|---|
| `packages/svelte/types/index.d.ts` | `d758022ae4101608b77b83207f9bb658e7238359` |
| `packages/svelte/src/compiler/index.js` | `1d822514b979bf398e1ce9d4fbe005594b15aa13` |
| `packages/svelte/src/compiler/types/template.d.ts` | `4d4b6fc21fc924482a0736a9ce5e73bad30afc89` |
| `packages/svelte/src/compiler/types/index.d.ts` | `c04466a24d3decb41506741dff8122fa984c821b` |

Their checkout bytes are identical to the paths at the peeled commit.

## Finding 1 — `svelte/compiler.parse` options

Genuinely resolved.

The pinned declarations at `packages/svelte/types/index.d.ts:878-894` expose exactly
`filename`, `modern`, and `loose` across the modern and legacy overloads. The pinned
implementation at `packages/svelte/src/compiler/index.js:102-122` confirms their
treatment: `filename` is explicitly unused, `modern` is passed only to the public-AST
conversion, and `loose` is passed to the parser and changes syntax-error recovery.

The candidate classifies all three exactly once at `svelte-options.tsv:2-4`:

| option | classification | assessment |
|---|---|---|
| `filename` | `not applicable` | Correct: the pinned parser ignores it, so it cannot be a second source-identity authority. |
| `modern` | `not applicable` | Correct: it selects only the returned official AST shape, while `OfficialAST` is outside the established Verter product set. |
| `loose` | `unsupported fail-closed` | Correct: it changes recovery/return behavior, and no loose parse capability is claimed. |

`capability-matrix.tsv:18` now narrows `SVELTE-PARSE-LOCAL` to strict
diagnostics/recovery, states that loose requests fail closed, and makes the official
modern/legacy AST return modes not applicable. That is consistent with the separate
`SVELTE-OFFICIAL-AST` not-applicable cell and removes the prior contradictory claim
that parser modern/legacy cases were supported.

## Finding 2 — source-authored custom-element descriptor

Genuinely resolved.

The pinned `packages/svelte/src/compiler/types/template.d.ts:58-60,77-106` establishes
that `<svelte:options>` overrides compile options and declares the descriptor fields
`tag`, `shadow`, per-prop `attribute`/`reflect`/`type`, and `extend`. The parser and
client transform at `packages/svelte/src/compiler/phases/1-parse/read/options.js:35-150`
and `packages/svelte/src/compiler/phases/3-transform/client/transform-client.js:588-655`
independently confirm that these fields affect tag registration, `ShadowRootInit`,
prop/attribute/reflection/conversion metadata, and class extension.

The candidate classifies the complete requested set exactly once at
`svelte-options.tsv:13-18`:

- `tag`, `shadow`, and per-prop `attribute`, `reflect`, and `type` are `supported
  canonical` with validated canonical treatment;
- `extend` is `unsupported fail-closed`, so an authored callback/identifier cannot be
  silently ignored or cross into the compiler core as semantic authority.

The `shadow` row owns the whole pinned field, including the literal `open`/`none`
forms and the source-authored object-expression form used as `ShadowRootInit`.
`capability-matrix.tsv:26` now expressly claims inline tag/shadow/per-prop settings and
expressly fails `extend` closed. `option-inventories.md:10-14,20-35` also names the
correct upstream source, distinguishes this descriptor from configuration-plugin
options, and records the same supported/refused boundary.

## Exact row-count recheck

`wc -l svelte-options.tsv` reports **36 physical lines**. Line 1 is the TSV header, so
there are exactly **35 option/data rows**; an independent `NR - 1` count also reports
35. This is the row convention used by `validate-package.mjs:52-64`, and its explicit
Svelte assertion is 35 at `validate-package.mjs:76-95`.

The 35 data rows reconcile to the exact pinned source as follows:

| source surface | inventoried data rows |
|---|---:|
| `svelte/compiler.parse` | 3 |
| `ModuleCompileOptions`, including `experimental.async` | 6 |
| `CompileOptions`, including the expanded `compatibility.componentApi` | 19 |
| source-authored `customElement`, expanding its per-prop members | 6 |
| `OptimizeOptions` | 1 |
| **total** | **35** |

Thus `validation.md:75-80` is correct when “rows” means option/data rows: the previous
26-row inventory gained exactly the three parse rows and six custom-element rows.
There is one additional physical header line, not a missing or surplus option.

## Bounded changed-file sanity pass

The candidate delta changes documentation/evidence files only. I found no new
blocking compiler-conformance contradiction in the changed files. In particular, the
inventory provenance, TSV classifications, parse/custom-element capability cells,
35-data-row validation claim, and closed seven-class validator agree.

Focused checks passed:

- `git diff --check ce1d0e4688af1b5bd548b6b68286632cc0f7ede8
  7442bb9060b7faa0720e528d3f96ee1df1abff95`;
- syntax checking of `validate-package.mjs`;
- post-review structural package validation with 22,718 non-zero assertions, using
  the primary reports' recorded `ce1d0e4688af1b5bd548b6b68286632cc0f7ede8` /
  `1ff1f83d8e994b6f1169b0b209c9f557c23f4728` binding; and
- independent upstream tag, tree, blob, worktree, and object-integrity checks above.

PASS — all findings resolved, no new blocking issue introduced, bound to commit `7442bb9060b7faa0720e528d3f96ee1df1abff95`.
