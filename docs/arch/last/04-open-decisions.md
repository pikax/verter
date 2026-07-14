# Open decisions — the user's to make, not an agent's

**Three.** GI-14 and GI-15 have an **operative default** written into the rules, so the protocol is **decidable
today** and nothing is blocked by them. **GI-16 has no default** — it is not a policy fork but a fact about what
already landed, and the only thing not available is leaving it implicit. All three **must be ruled before the
gate-integrity block lands**.

**A user ruling does not bypass the governance gate:** the rule-text edit that encodes the ruling still needs prior
neutral codex approval.

---

## GI-16 — The rules commit landed WITHOUT its two governance gates. Ratify it, or make it pass them.

**This is not a policy question. It is a hole in the record, and it is in the one commit that must not have one.**

`5c4693ab1` — the commit whose entire thesis is *"a gate that cannot prove it ran is a failure"* — carries:

- **No clean unprimed approval.** Its final review round reached **zero P0s but never returned `APPROVED`**; it
  landed under the doc-review round cap. `CHANGES REQUIRED` with findings adopted is not an approval.
- **No independent post-land confirm.** One was dispatched against the landed tip and **stopped mid-flight, before
  its review legs, leaving a zero-byte output and no verdict.** A dispatched-then-killed gate produces exactly what
  an omitted gate produces: nothing. It is recorded as **UNRUN** — never as pending, never as passed.

It landed on the explicit wrap-up-and-checkpoint instruction. **That is a reason, not an approval**, and the
distinction is the substance of the commit itself.

**Two coherent rulings:**

| | |
|---|---|
| **A. Ratify the landing** | Record the exception, its reason, and that the confirm is **waived** — so the record says *waived* rather than implying *passed*. Cheap, honest, and leaves the rules operative. |
| **B. Run the confirm** | Dispatch the independent unprimed confirm against the landed tip and adopt its findings as a **follow-up commit**. Slower; it is the only option that actually produces the missing evidence. |

**What is NOT available is leaving it implicit.** An unrun gate that nobody wrote down is indistinguishable from a
gate that passed — the exact class the commit exists to close. **It may not be closed by the agent that landed
it**; that is the entire reason the confirm is dispatched by the tier above the author. Owner and acceptance test:
[`../gate-integrity-ledger.md`](../gate-integrity-ledger.md) → **GI-16**.

---

## GI-14 — May an ATTESTATION authorize Agent-tool dispatch, or must it be a PROOF?

**Context.** The orchestration skill makes Agent-tool dispatch the default **only when** the harness guarantees no
inherited transcript, a distinct agent identity, status/stop/continue control, and child-agent spawning — and it
says each gate rests on *"a recorded precondition, not an assertion"*, specifically a recorded
`CAPABILITY … result=PASS` proof (absent or stale ⇒ fall back to `claude -p`).

**Today that precondition is asserted, not executed.** The **executed capability probe is GI-9 — it does not
exist yet.**

**Operative default today:** *yes — an attestation authorizes dispatch. **Risk-accepted, and explicitly not a
proof.***

**The tension, stated honestly:** "a capability consumed as evidence" is the **exact root defect** that killed the
editor-engine block. Ruling that an attestation suffices is, on its face, the same move one level up. But
requiring proof *today* would **un-authorize Agent dispatch entirely**, because the probe that would supply the
proof does not exist — the gate-integrity block would have to be driven on the `claude -p` fallback path in order
to build the probe that re-authorizes Agent dispatch.

**Two coherent rulings:**

| | |
|---|---|
| **A. Risk-accepted interim; proof required at GI-9** | Keep the default, but keep it **labelled** as a risk-acceptance rather than a proof, and make the executed probe a **hard acceptance criterion** of the gate-integrity block. Neither lies about what we have, nor halts the work to get it. |
| **B. Proof required now** | The strictest reading of our own thesis. Agent dispatch is unauthorized until GI-9 lands; the gate-integrity block runs on `claude -p`. |

---

## GI-15 — Red baseline, or green gate?

**Context.** The properly-provisioned suite fails **5 of 118 executed** real-provider tests on base (completion ×2,
hover, rename, a completion/edit race). **Zero regressions** were introduced by the rejected block.

**Operative default today:** **STRICT** — *a nonzero gate BLOCKS. No exclusion list.*

**Two coherent rulings:**

| | |
|---|---|
| **A. STRICT green gate (no exclusions)** | An exclusion list is precisely the mechanism by which a gate quietly stops testing things — this campaign's entire thesis — and it **rots**. The user has already ruled that *"0-new is necessary, not sufficient"* and that *"any failure that invalidates a block's actual acceptance remains blocking even if it also existed on base."* **Cost:** a real bootstrap — the gate-integrity block must **fix or formally disposition** the 5 before anything lands behind a green gate. |
| **B. Red baseline + 0-new** | Accept a known-red baseline with a tracked exclusion list; block only on **new** failures. Pragmatic; work proceeds immediately. **Risk:** the baseline number was **wrong every single time it was quoted** this week (2 → 7 → 11 → **5**; only the last was ever measured), exclusion lists grow, and this re-opens the *"it was already broken"* excuse that was explicitly closed. |

---

## Already ratified — do not reopen

- **`CODEX_MODEL_POLICY`:** one entry for **every** role (review, §1a, anti-rogue, architecture,
  best-implementation) — **no role split.** **Unavailable | substituted | unknown | mismatched ⇒ BLOCK THE LEG**;
  no replacement, no upgrade, no downgrade. The slug lives **only** in
  `.claude/skills/mom-cto-orchestration/reference/codex-model-policy.toml` — never in prose, a template, or a
  memory. **Verify the binding from the CLI's own output** (note it prints preamble lines *before* the banner — a
  naive "first line" check **rejects every real leg**).
- **Trust filter: DELETE the plumbing and the claims.** Do not ship it inert. Untrusted-workspace support is a
  future, explicit feature with its own threat model.
- **Sequencing:** gate-integrity block **first**, then the serving-order architecture block.
- **The editor-engine selection path is abandoned.** See
  [`03-editor-engine-selection-rejection.md`](03-editor-engine-selection-rejection.md).
