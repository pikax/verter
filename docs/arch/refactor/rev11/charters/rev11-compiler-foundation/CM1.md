<!-- unified-charter-v2
id=CM1
name=Component-meta request-view materialization and runtime-prop type regression correction
phase=rev11
train=rev11.compiler-foundation
product=rev11
kind=implementation
semantic_role=history
class=subsystem
predecessors=BV1,BS1
conditional_predecessors=
owner=rev11.compiler-foundation:the single owner named by the accepted Rev11 architecture
conflict_domains=ratified_rev11_contract
resource_class=rust-mixed
review_profile=history
gate_profile=legacy-receipt
implementation_effort_min=low
implementation_effort_default=low
review_effort_min=low
review_effort_default=low
verification_effort_min=low
verification_effort_default=low
confirmation_effort_min=low
confirmation_effort_default=low
size=M
dispatchable=false
optional=false
release_gating=none
source_refs=live:docs/arch/refactor/rev11/charters/CM1.md
external_requirements=
activation_gate=none
charter=charters/rev11-compiler-foundation/CM1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=ACCEPTED
-->

# CM1 — Component-meta request-view materialization and runtime-prop type regression correction

Status: **ACCEPTED**. This is a v2 identity wrapper, not a rewritten implementation charter.

## Bound authority

- Live charter: `docs/arch/refactor/rev11/charters/CM1.md`.
- Exact live-charter SHA-256: `9b5b6f12a14a063e34cde6ff4da3b69cea91557db4d7b34278a97128a711a21a`.
- Accepted commit/tree: `f07ba1e99b14fc928bd7395241ee326b9e90b93e` / `612d1c633c68f23b868b7fd4cc61e0accbafb8da`.
- Source ledger SHA-256: `2b176c4c15730ff9698c73f677eb239800c1adff3891e4e77493a768ea630dee`.
- v2 may not reinterpret, reopen, or retroactively claim governance over this work.

## Transition rule

The immutable legacy receipt under `state/legacy-receipts/` is the only v2 acceptance input for this historical node.

## Citations

- `live:docs/arch/refactor/rev11/charters/CM1.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-LEGACY-TRANSFER-2C07EE214251

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:488-493`
- Applicability: `CM1`, `TIF1`
- Exact text SHA-256: `e07cd13a89367b660ede03174fc4ca7a3bf7bcdea265c2794dd5ab57bd04d0e7`

~~~~markdown
### LEGACY-TRANSFER-2C07EE214251

- Original path: `docs/arch/scanners-replacement-capability-ledger.json`; Git blob: `2c07ee21425126369a9c0d592716d5b1b5ffa54f`; exact source SHA-256: `d27401a4f9b8040fe25b98d754f8685894a2927c6e55b9a38af2f1741612acf9`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-capability-ledger.json`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-B3203BB016F3

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:495-500`
- Applicability: `CM1`, `TIF1`
- Exact text SHA-256: `7f42d87e7dc3ba72f7c1ad5b4dd9a74f3a554b6de1405bf1deeedd383fd6e173`

~~~~markdown
### LEGACY-TRANSFER-B3203BB016F3

- Original path: `docs/arch/scanners-replacement-compat-descriptor.md`; Git blob: `b3203bb016f3e5fb4d33acc361a1cd990bba3b9e`; exact source SHA-256: `7e8f70693f4e9d185df467b0f055f36b751d88f12a67ac90fc90a02aa7dd4ac6`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-compat-descriptor.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-D02185D5F77B

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:502-507`
- Applicability: `CM1`, `TIF1`
- Exact text SHA-256: `c6a9d5650baf30164f4a52ed76e5e5151907112d4e00e21321ccc132d69509ba`

~~~~markdown
### LEGACY-TRANSFER-D02185D5F77B

- Original path: `docs/arch/scanners-replacement-content-handoff.md`; Git blob: `d02185d5f77b21675d1eea6cc562aea4230e187f`; exact source SHA-256: `b2dbbaff4908a48ec75e1fb82c3708f9bd5586eb02bf096796bf03e105b35ccd`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-content-handoff.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-78E3C9D90645

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:509-514`
- Applicability: `CM1`, `TIF1`
- Exact text SHA-256: `646d6d7568132f09da789e93b8d603f49e3534182209d786c99e9997f3762478`

~~~~markdown
### LEGACY-TRANSFER-78E3C9D90645

- Original path: `docs/arch/scanners-replacement-preprocessor-interim.md`; Git blob: `78e3c9d90645a5be2472af2e13e3c0d439f045c8`; exact source SHA-256: `55e5063bef00253251f844e4554773a967f659f7505b1f67f588a0f650b9f3a1`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-preprocessor-interim.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-7F86EABE42F8

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:516-521`
- Applicability: `CM1`, `TIF1`
- Exact text SHA-256: `535207925b5b845e9f9327de86c3ec8fab440e7b3e07b73d43cce84e8ff0b62f`

~~~~markdown
### LEGACY-TRANSFER-7F86EABE42F8

- Original path: `docs/arch/scanners-replacement-public-contract.md`; Git blob: `7f86eabe42f8c061f7372448e6f5a19426d800c4`; exact source SHA-256: `9721102ea74e9e643a5ed8fb5f70afefa236c2f82e1fd33508b578bbfb30cafb`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-public-contract.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-3970AE56B544

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:523-528`
- Applicability: `CM1`, `TIF1`
- Exact text SHA-256: `4139895ac8da06e621f2e0f50158457b46892c75927fcbbdb1d27074bfc2611b`

~~~~markdown
### LEGACY-TRANSFER-3970AE56B544

- Original path: `docs/arch/scanners-replacement-type-authority.md`; Git blob: `3970ae56b544b0121b4dadd310860e76c00ea9fa`; exact source SHA-256: `b5ad534163f45ecaadde20b2b066d6d62d4eccf57a80b0f2411697770a75c67c`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-type-authority.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-04C59BB04C8E

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:530-535`
- Applicability: `CM1`, `TIF1`
- Exact text SHA-256: `eb9e6cbd8cf511dd85ec354b0dfa242d7d2253310bfbefccb6f7dfb47e126ce0`

~~~~markdown
### LEGACY-TRANSFER-04C59BB04C8E

- Original path: `docs/arch/scanners-replacement-typed-feature-facts.md`; Git blob: `04c59bb04c8ed361ab3203e62459be38ea10eeed`; exact source SHA-256: `3cca02f52a537f5567a13c2964ae8be4d40a5fa7388caa56220cc0038903e4a4`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-typed-feature-facts.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

## Live authority inputs

- `live:docs/arch/refactor/rev11/charters/CM1.md` — 21773 bytes, SHA-256 `9b5b6f12a14a063e34cde6ff4da3b69cea91557db4d7b34278a97128a711a21a`
