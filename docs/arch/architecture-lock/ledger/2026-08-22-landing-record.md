# Landing record — 2026-08-22

Fourteen blocks landed on `program/architecture-lock` between `b9d036c83` and
`c68fe61e3`. This file records them factually.

**It does NOT mark any block ACCEPTED in `program-state.toml`, and the rows there
are deliberately left untouched.** That ledger requires evidence this process did
not produce — see "Protocol gap" below. Promotion to ACCEPTED is a maintainer
decision, not something this record claims.

## Landed, in order

| trunk sha | block | what |
|---|---|---|
| `a53447b19` | gate infra | gate telemetry: dead timing parse, wrong RSS sampler |
| `b9d036c83` | — | LSP test: await the provider-pull signal instead of one yield_now |
| `979123ef4` | **BV2** | TSC expose bundle fails closed on any corrupt identity |
| `f55469eff` | — | Vue pinned to 3.6.0-rc.5 across every oracle authority |
| `8e6f0c0da` | — | developer build separated from the distribution pipeline |
| `eadec2dc0` | **J1 (partial)** | CSS readers converged on one shared parse (rows 6-12 / A16-A23) |
| `0e5177931` | **CM1 (partial)** | exposed bindings resolve by identity; preparation failures preserved |
| `cfa8fa065` | — | Svelte oracle bumped to 5.56.10 |
| `d74267780` | — | Svelte case IDs made durable identities, not hashes of the pin |
| `68d463638` | — | LSP: stop pairing an IDE surface with the wrong document revision |
| `264f727cb` | gate infra | one test universe; Surfaces 2 and 3 removed with seeded-defect proof |
| `7e3a4dfbe` | — | plan vocabulary retired from prose; layout-guard expected set synced |
| `120eede71` | **J1 (partial)** | style_planner brought to parity with the legacy CSS route |
| `13eafb2ab` | **CM1 (partial)** | runtime prop constructors resolved by binding, not by name |
| `be3927446` | — | one comment reworded off the plan-vocabulary guard |
| `3f663584e` | — | degraded class-method inference contained to its own prop row |
| `c68fe61e3` | **B5** | accepted framework algorithms exposed through one direct core |

Trunk is green at `c68fe61e3`: **24946/24946, 0 failures**, shipped-cfg guard
10/10, freshness byte-pin genuine (tolerance disabled).

## Protocol gap — why no row is marked ACCEPTED

`program-state.toml` requires, per block: `charter_digest`,
`context_packet_digest`, `base_sha`, `candidate_sha`, `candidate_tree`,
`accepted_sha`, `accepted_tree`, `landing_equivalence_digest`,
`evidence_digest`, and THREE separately-named review mandates —
`conformance_review`, `architecture_review`, `adversarial_review` — each with its
own reviewed SHA, plus `maintainer_decision`.

What this process actually produced: one independent codex review lane per block,
iterated to clean (several blocks ran 3-9 rounds, B5 ran 3, bv2 ran 5, build ran
9). That is a real and load-bearing review, but it is ONE lane, not the three
named mandates, and no charter or context-packet digests were computed.

So these landings are **evidenced but not ratified**. The maintainer decides
whether to (a) accept the single-lane review as satisfying the mandates for these
blocks, (b) run the missing mandates retroactively against the landed SHAs, or
(c) treat them as provisional.

## Partial blocks — do not read as complete

- **J1** covers 41 acceptance IDs across four landing units. Landed:
  rows 6-12 / A16-A23 (reader convergence) and row 2 (style_planner parity).
  Outstanding: lightningcss removal, the NAPI wire contract, unplugin, Svelte's
  own CSS grammar, and the closing items — in flight as `block/css-cutover`,
  `block/svelte-css-grammar`, `block/css-closing-items`.
- **CM1** landed the exposed-binding and runtime-constructor work. Its
  runtime-constructor shadow scanner was DELETED as structurally unsound (no
  stateable completeness boundary) and replaced by the OXC-binder-backed
  `RootBindingIndex` in `13eafb2ab`.
- **BCSS0** is dependency-ready and blocked on two `style_planner` capabilities
  (an identity map over unchanged bytes; a deliberate decision on the refusal
  arms), both folded into `block/css-cutover`. It is a mandatory predecessor of
  B2 and B3.
- **B5** is complete and unblocks **B6**.

## In flight at time of writing
`block/css-cutover`, `block/svelte-css-grammar`, `block/css-closing-items`,
`block/dependency-alignment`.
