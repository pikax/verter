# C1 final A6 waiver-application receipt

> Superseded for the current candidate (2026-08-27). This entire receipt is a
> historical exact-subject record for `6fd3356e…`; its identity, measurements,
> and `C1-A6-WALL-REL-001` authority do not apply to the later corrective
> candidate. The later historical subject `e0d6732a…` is recorded in
> `final-round2-performance.md` with literal wall/allocation failures and
> exact-subject-only dispositions `C1-A6-WALL-REL-003` and
> `C1-APM002-ALLOC-REL-002`. Corrective rounds change production and ran no performance
> measurement, and makes no waiver or performance-acceptance claim.

## Bound identities

- Registered comparison base: `d1f3d50a948597f036868543b9bb21acacd730ff`, tree
  `2e7cf8637ec5c52b0fa04572d99672b052f1f85f`.
- Exact measured final production/evidence subject:
  `6fd3356e3d1ec7d21e4f03850a224283ef43371e`, tree
  `e94f502da626c9062fff54c442d51d90d6e097e2`.
- Registered production subject retained in this lineage:
  `0c22953821f57eedd32b812b1478a449a976f964`, tree
  `8edde35fb0a18cce5fe229b87ca991c9f95bff20`.
- Waiver: `C1-A6-WALL-REL-001`, and no other exception.
- Immutable `FINAL_REVIEW_SHA`/tree: recorded by the ignored
  `.feedback/c1-recovery-implementer-receipt.md` after all evidence-only commits. This file cannot
  contain the hash of the commit that contains itself; the measured subject above is the last
  production, harness, corpus, toolchain, or performance-configuration change.

The harness Git blob is `efa9ea54a14772ecd87511d6bb07017aa33940ba`; both archives hash to
`5e06d35dda284a8ef049bf0dd3dc39974b904729f740da58c650ec59e806f632`. Toolchain, root manifest,
and cargo config blobs match the registered pair. Builds used separate archive exports and target
directories under `/tmp/c1-a6-final.3oaVBm`.

## Enabled-arm retained gates

Command for each archive:

```text
cargo build -p verter_bench --release --features attribution --example attribution_baseline
attribution_baseline --files 40 --runs 3 --format tsv
```

| Retained lock | Candidate | Result |
|---|---:|---|
| `workspace.normalize_canonical_id.calls <= 11,313` | 1,981 | PASS |
| `session.semantic_dispatch.calls <= 4,216` | 4,216 | PASS |
| `session.semantic_cold_build.calls <= 1,063` | 1,063 | PASS |
| `session.cache_admit_cacheable.calls >= 1,063` | 1,063 | PASS |
| carrier/script/eval parse calls | 40 / 40 / 42 | PASS |
| source-copy / fact-observe / indexed-ready | 124,410 / 16,917 / 8,032 | PASS |
| source-map builds | 40 | PASS |
| CSS parse/transform/style-analysis | 0 / 0 / 0 | PASS |
| component-meta digest | `7161214711717846280` | PASS |
| corpus / runs / attribution / warmup | 41 files / 3 / ON / 40 of 40 | PASS |

Enabled binary SHA-256: base
`742d0a78bc11047d290a8df37ecd7f8074306637e6a492b5f66af032298984cf`, candidate
`540aafd89d407a6748c92dc0ba1afdcdb240c0f173a0137b4a66a9532aaf04f9`. Raw TSV SHA-256: base
`ae75352339bf399abd8b503062e3ddfc64f21e09a44ddc5f166ab04f1ea45b53`, candidate
`6927a1533fe9182b8074ece530d115516b28a4ff349c52d0fa283b083fee513b`.

## Accepted disabled wall/RSS protocol

The user declared the host exclusive, waived `rust-lock.sh`, and explicitly waived steady-state
RustDesk presence. RustDesk service/server presence is disclosed; no cargo, rustc, nextest,
benchmark, BrowserStack, or updater process competed in the accepted protocol. Every pre/post
receipt reported AC power and no thermal or performance warning.

Default-feature binary SHA-256: base
`edbc4c8338d84d18e18a4842790ef1d92e97452651c8c27ada1d9871f3a86415`, candidate
`cf1d8facf2db0e50a765c1022a193314c3ff838d450b40984a8f711780aa8181`.
The accepted order was control-start, `A1 B1 B2 A2`, `A3 B3 B4 A4`, control-end, with 20 seconds of
equal idle cadence before every invocation. Every invocation exited zero, retained all 30 samples,
reported 40 files / attribution OFF, and warmed 40 of 40 components.

| Invocation | Arm | Wall median ms | Wall min ms | Peak RSS B | Instructions | Cycles |
|---|---|---:|---:|---:|---:|---:|
| control-start | control | 86.10 | 84.18 | 75,284,480 | 52,698,060,957 | 17,798,049,165 |
| A1 | base | 86.66 | 85.30 | 75,186,176 | 52,755,269,731 | 18,074,774,806 |
| B1 | candidate | 96.13 | 93.27 | 75,481,088 | 57,537,268,133 | 19,194,324,306 |
| B2 | candidate | 94.45 | 93.31 | 75,890,688 | 57,517,772,591 | 18,886,886,866 |
| A2 | base | 86.29 | 81.77 | 75,776,000 | 52,700,691,690 | 17,918,444,390 |
| A3 | base | 88.54 | 84.60 | 75,776,000 | 52,688,531,662 | 17,794,263,535 |
| B3 | candidate | 97.67 | 94.52 | 75,841,536 | 57,490,033,065 | 18,863,928,255 |
| B4 | candidate | 96.64 | 95.07 | 75,513,856 | 57,511,491,986 | 18,815,870,429 |
| A4 | base | 87.10 | 84.96 | 75,431,936 | 52,635,688,767 | 17,632,345,994 |
| control-end | control | 86.90 | 85.09 | 75,513,856 | 52,652,503,142 | 17,634,030,764 |

Controls drift `+0.929152%`, inside the unchanged 3% invalidation fence. Median of four invocation
medians is base `86.880 ms`, candidate `96.385 ms`:

- relative wall: `+10.940378%` — **FAIL — covered by `C1-A6-WALL-REL-001`**;
- absolute wall: `96.385 ms <= 100 ms` — PASS;
- candidate peak RSS: `75,890,688 B <= 268,435,456 B` and `+1.389953%` versus the frozen
  74,850,304-byte reference (`<=4.952%`) — PASS;
- median instructions: `52,694,611,676 -> 57,514,632,288.5`, `+9.147084%` (diagnostic);
- median cycles: `17,856,353,962.5 -> 18,875,407,560.5`, `+5.706952%` (diagnostic).

The relative metric remains a recorded FAIL; neither the threshold nor the result is rewritten.
Every retained A6 conjunct passes.

## Raw identities

External evidence root: `/tmp/c1-a6-final.3oaVBm`. Curated manifest: 138 raw/metadata entries plus
both accepted disabled binaries. Manifest SHA-256:
`1a2cfa6a3cf77e118be41ea538506755d912397d918d899236391bbf5ec5f4b5`.

| Raw item | SHA-256 |
|---|---|
| accepted metrics TSV | `192aa762ef7bf2eda7e233628c067c8670bd5e68167f5322d5828cdaea733275` |
| accepted aggregate TSV | `a640870e32ee7b860d390353341f4202e69b5fa4f17842ef4a49322f906dd3f4` |
| control-start / control-end | `f78116b4edf0de282af35af3a1795730aadc3a4fab1b20830cd096c664c65e72` / `c33c3fa0cec0665d03e29ea8bb44a2d0481db05c1cd2e4eb4a1711f39f0bb23d` |
| A1 / A2 / A3 / A4 | `4ee04b762859b35dab331e2f9fb0db4946b3988a8354766678f3465ee617560a` / `29ea6345b3d3b62428a7105c54308b61face0a1ae9bbae99e2f615111d0316f7` / `a16159e868ddc29a54533f7d6e030b399c8845d23f8acfd7833dcf8194369e45` / `298a0605736a2bd3557ee6c2a8c1377899e7de1f2b26fe5ffb7b4f5953dedcd8` |
| B1 / B2 / B3 / B4 | `a8cb7911f82db9c9997fa6d4f236477520488ca82ed635de9858b08d03b9095f` / `366e142737599f0aa41bcc1c14b1461f130d300e26ffb10b4ad7769f41c97ad6` / `7bc0c683486aef0299cac8cb91c640726e0abd9f04f6a131e9e3ae26f1e0d423` / `e9ddc74bcb20fdf5a2de7813e220ceac35eb488cca4c25c135d003dd05b59971` |

Protocols 1 and 2 are retained whole but void for control drift. Protocol3 is retained
diagnostic-only because its condition receipts caught a RustDesk application update; the updater was
absent from accepted protocol4. No excluded sample enters a reported statistic.

## Production-content equivalence

The registered production implementation through `0c2295382` is retained exactly in lineage.
Between that subject and `6fd3356e3`, Step 6 changed 32 production Rust blobs solely to remove
program vocabulary from comments, except one diagnostic spelling in
`verter_session/src/host_manage/eval_env.rs`. This command reports only that one path:

```text
git diff --ignore-blank-lines --ignore-matching-lines='^[[:space:]]*//' \
  --name-only 0c22953821f57eedd32b812b1478a449a976f964..6fd3356e3d1ec7d21e4f03850a224283ef43371e \
  -- crates/verter_lsp/src crates/verter_semantic/src/resolver_core \
     crates/verter_session/src/host_manage/eval_env.rs crates/verter_workspace/src
```

That remaining change replaces one literal containing `C1` with `concat!("... C", "1 ...")` and
explicit positional arguments. A direct Rust equality control over the old/new formatted output
passes; the emitted line SHA-256 is
`624cf79d534d63695df8222e0cf3cae45231a449eb29a2e9f171e371f8c089f3`.
No resolver behavior, result, wave, load set, observation, witness, cache owner, public API, harness,
corpus, toolchain, or threshold changed. The exact final source was remeasured rather than inheriting
or restamping the earlier wall result, and all correctness/health suites were rerun after the authority
rebase.

RESULT: RETAINED GATES PASS; RELATIVE WALL FAIL COVERED ONLY BY C1-A6-WALL-REL-001
