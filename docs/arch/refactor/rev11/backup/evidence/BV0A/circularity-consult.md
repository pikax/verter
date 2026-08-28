# BV0A/BV0 acceptance-boundary circularity — independent consult

Codex xhigh, sandbox read-only, dispatched 2026-08-14 as a genuinely independent
second opinion (not reusing the implementer track's own tie-break consult) after
BV0A's implementation (`work/bv0a-implementation` @ `db26cde00`) was found by
independent conformance review to satisfy a narrower claim ("assembly introduces
zero new mapping violations") than BV0A's ratified literal Required Exits ("the
full 36-cell oracle verdict is clean").

## Verdict

Do not accept or land `db26cde00` under BV0A's current charter, and do not land an
emitter fix out of DAG order.

The implementation honestly proves a strong, valuable claim — assembly is
mapping-neutral — but that is narrower than BV0A's ratified Required Exits. The
clean resolution is a narrow maintainer-ratified amendment redefining BV0A's
acceptance boundary as correct composition relative to its input fragment maps.
Then revalidate and accept BV0A, unlock BV0, and fix the emitters there.

## 1. Technical diagnosis

The specific `const`→`<` segment has a narrow, deterministic root cause, but the
complete 36-cell emitter correction is not a single offset fix.

The host pipeline (`virtual_file_pipeline.rs`) is not introducing the bad mapping
— it passes `source_map` into compilation, calls the production assembler, and
stores the returned map without translating authored coordinates. The compiler
constructs `CodeTransform` over the full SFC and generates the script and template
maps directly from it (`compile/mod.rs`). Therefore this is not a missing
SFC-to-fragment line offset.

Exact failure chain: `<script setup>`'s opening tag is replaced with the generated
wrapper using `overwrite_or_root_prefix` (`script/process.rs:461`). For a
non-empty tag range, that helper performs an ordinary mapped overwrite
(`template/code_gen/types.rs:127`). `Chunk::Overwritten` emits one source-bearing
segment at the replacement's generated start, pointing to the original range's
start (`code_transform/source_map.rs:243`). The replacement begins with
`const __sfc__...`; the original range begins at `<script...`. Therefore `const`
maps to `<` by construction — a synthetic-boundary classification bug, fixable at
the emission operation (delete the original boundary, insert synthetic
replacement bytes unmapped — the repo already has this correct pattern at
`ide/template/emit.rs:155`), not a global column adjustment.

The missing-anchor problem is separate and broader: current mapped chunks emit
tokens at chunk starts/line starts/explicitly registered locations; the runtime
script/template emitters don't register exact anchor locations. Existing compiler
tests explicitly accept statement/line-level mapping rather than exact identifier
columns, while BF2 requires exact authored starts and span coverage.

Conclusion: the `const` false mapping is small and mechanically fixable.
Achieving oracle-clean maps across script plus VDOM, Vapor, and SSR is a
multi-emitter BV0 task — it likely doesn't require a new mapping architecture
(the necessary primitives exist), but it is not one off-by-N patch.

## 2. Is the narrowed test legitimate?

It is honest engineering evidence, but not a legitimate acceptance reading of the
unamended charter. AMD-007 and BV0A explicitly assign residual emitter defects to
BV0. The implementation's test candidly requires zero assembly-owned or
assembly-introduced violations while deliberately attributing fragment-emitter
violations elsewhere — a sound and discriminating assembly-composition test. But
BV0A's literal exits independently require a correct map, every source-bearing
segment to satisfy the oracle, exact required-anchor coverage, all 36 cells
passing, and no silently omitted required segment. Its abort clause says false
fragment maps require `RESCOPE_REQUIRED` to BV0, not local relaxation of BV0A's
gate. Mutation detection against a dirty baseline proves the mutations are
discriminated — it does not prove the unmutated baseline satisfies the Required
Exits. Logging inherited violations with `eprintln!` cannot amend a ratified gate.

The conformance FAIL is correct. The test is not deceptive; the charter is
internally circular.

## 3. Disposition options considered

| Option | Decision |
|---|---|
| Full BV0 restack or atomic landing | Not legal under the current DAG — BV0 can be developed contingently above BV0A, but cannot be reviewed, landed, or accepted first. A stack cannot silently reorder or split program acceptance units. |
| Amend BV0A to assembly neutrality | Valid and recommended — matches the explicit ownership split while preserving BV0's literal 36/36 zero-violation obligation. |
| Land only the small `const` emitter fix first | Technically feasible, but governance-invalid and insufficient. It is expressly BV0-owned production code; commit size does not change ownership. It also would not restore the missing anchors or all template paths. |

A maintainer could create an urgent predecessor block or fold an emitter subset
into BV0A, but that itself requires a formal amendment/DAG change — it does not
avoid ratification.

## Exact recommendation

1. Record `db26cde00` as `RESCOPE_REQUIRED`, preserving the candidate and its
   zero-assembly-violation evidence. Do not modify emitters in this block.
2. Ratify a narrow amendment changing BV0A's Objective/Required
   Procedure/Required Exits/Abort language to require: genuine production map
   composition in all applicable cells; exact preservation and placement of
   every fragment mapping; zero assembly-introduced oracle violations; no
   provenance over assembly scaffolding; hard failure for absent/malformed/
   uncomposable fragment maps; asserted (not merely logged) standalone
   attribution for violations already present in fragment maps. Must explicitly
   preserve BV0's literal 36/36 oracle-clean exit.
3. After the amendment lands, restack/refreeze BV0A, rerun conformance/
   architecture/adversarial review against the amended text, then land and
   accept BV0A.
4. Move BV0 from LOCKED to READY. The BV0 implementer adds failing exact-map
   tests first, fixes synthetic wrapper boundaries with typed unmapped
   operations, adds exact authored-location mappings across script/VDOM/Vapor/
   SSR, and proves the full 36-cell baseline clean.

This requires maintainer ratification. It is not resolvable by interpretation
within existing authority.
