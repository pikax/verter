---
ruling_id: "BS1-ADVERSARIAL-EXACT-CANDIDATE-ATTESTATION"
type: "attestation"
date: "unknown"
date_source: "file-mtime 2026-08-20 (no in-document date; binds to a candidate that predates the later restack recorded in EVIDENCE-BS1-RESTACK-BYTE-IDENTITY.md, so this attestation predates that document within the same day)"
binds: ["BS1"]
source_file: "ATTESTATION-BS1-ADVERSARIAL-EXACT-CANDIDATE.md"
summary: "Discharges BS1-COMPLETION-AUTHORITY §5 second half: an unprimed, isolated claude-max subagent independently attests candidate 9786e756b (base f46de1b6a, evidence commit a48d92e82) PASS, via eight genuine plant->prove-RED->revert->prove-GREEN cycles against production code plus blast-radius probes against the real pinned svelte@5.56.8 oracle. Binds ONLY to 9786e756b — the document states explicitly that if BS1's remaining completion-packet work changes the candidate, this attestation does not carry, and the review-identity binding at 71fb82dec mechanically refuses a PASS on a stale reviewed SHA."
supersedes: []
superseded_by:
  - ruling: "BS1-RESTACK-BYTE-IDENTITY"
    claim: "This attestation's binding to candidate 9786e756b does not carry to the restacked candidate 761651109 — that document states so explicitly ('The adversarial verdict does NOT carry by this proof... A fresh adversarial pass is owed at that point, not now'), and this attestation's own text anticipates exactly that condition."
contradicts: []
notes: "Lists all eight plant/RED/revert/GREEN cycles verbatim with the specific production defect each proved (function-decl name mapping, shorthand attribute binding, destructure conflation, member/each-item binding, non-ASCII char-boundary panic, store-gated EACH_ITEM_IMMUTABLE)."
---

# BS1 adversarial attestation — bound to candidate `9786e756b`

Discharges §5 (second half) of MAINTAINER-RULING-BS1-COMPLETION-AUTHORITY: the adversarial seat must
independently attest the EXACT final candidate and bind its verdict to that candidate SHA.

**Verdict: PASS.** Bound to candidate `9786e756b` ("fix(core): correct seven Svelte client
compiled-output defects"), base `f46de1b6a`, evidence commit `a48d92e82` (docs-only).

Seat: claude-max subagent, isolated detached worktree, dispatched UNPRIMED and explicitly told not to
treat the earlier adversarial pass as evidence about this candidate, and that NOT_PROVEN was a legitimate
answer. It was dispatched because the earlier pass ran BEFORE fix round 1, which changed three defects on
the emit path that no adversarial seat had ever examined.

## What it executed (not read)

Eight genuine plant → prove-RED → revert → prove-GREEN cycles against production code, each mutation
confirmed PRESENT, UNIQUE and NEW via `git diff` before any result was trusted:

1. function-decl name mapping, async arm — hardcoded `"function ".len()` ⇒ RED
2. function-decl name mapping, generator arm ⇒ RED
3. shorthand attribute binding provenance — flattened non-mapped emit ⇒ RED
4. single-name-destructure conflation — dropped the `shorthand` flag check ⇒ RED, and the failure output
   exhibited the exact conflation bug (`({ id: foo }) => foo` reading `.foo`, the wrong key)
5. member bind rooted at each item — swapped for the plain writable-target check ⇒ RED
6. each keyed by its own index — removed the `key_is_the_index` gate ⇒ RED
7. non-ASCII char-boundary panic — byte advance instead of char ⇒ RED, reproducing the panic
8. store-gated `EACH_ITEM_IMMUTABLE` — dropped the store predicate ⇒ RED (flags 17 vs 1)

Each reverted to GREEN. It further verified the destructure classifier discriminates on OXC's `shorthand`
bool + `BindingIdentifier`, not name count — structurally sound against rename, array and rest shapes.

Blast-radius probes against the REAL pinned `svelte@5.56.8` oracle (nested destructured `{#each}`,
destructured item + index + key + member access, plain member access on a destructured field, store-gated
destructured-keyed each) all matched official structural output. It traced one anomalous first run to its
OWN probe methodology rather than reporting it as a compiler defect — the right instinct.

It independently verified the record's pre-existing disclosure is HONEST: `pattern_single_binding`'s
`Span::new(0,0)` fallback is byte-identical at base `f46de1b6a`, untouched by this candidate. Worktree
left clean; all plants reverted.

## Binding, and its limit

This verdict binds to `9786e756b` and to nothing else. Per the ruling's §6, BS1 has REMAINING work: the
completion packet's rows. **If that work changes the candidate, this attestation does not carry** — the
review-identity binding landed at `71fb82dec` will mechanically refuse a PASS whose reviewed SHA is not
the current `candidate_sha`, which is precisely the protection it was built for. A fresh adversarial pass
on the eventual final candidate will be owed at that point.

## §5 first half, for completeness

Conformance and architecture carry across the REBASE onto `f46de1b6a` only, proven by byte-equivalence:
the candidate's own diff over `crates/` + `packages/` is byte-identical pre- and post-rebase (68,598
bytes both). That proves the rebase preserved their subject matter. It does NOT extend to the fix-round
rewrite, which is why this adversarial re-attestation was required rather than assumed.
