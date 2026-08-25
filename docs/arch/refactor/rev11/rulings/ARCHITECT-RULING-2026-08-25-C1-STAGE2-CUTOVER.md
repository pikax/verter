---
ruling_id: "C1-STAGE2-CUTOVER-2026-08-25"
type: "architecture-ruling"
date: "2026-08-25"
date_source: "in-document (**Date:** 2026-08-25)"
binds: ["C1 Stage 2 — the atomic module-resolver cutover"]
source_file: "ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md"
summary: "See §11 for the section crosswalk. This field names no contents and asserts no claim."
supersedes: ["docs/arch/refactor/rev11/evidence/C1/stage2-execution-plan.md (normative role only)", "docs/arch/refactor/rev11/evidence/C1/stage2-preflight-annex.md (normative role only)"]
superseded_by: []
contradicts: []
notes: "This field records no claim of its own. See §10 and §11."
---

# Architect ruling — C1 Stage 2

**Date:** 2026-08-25
**Authority:** architecture consult under this program's delegated ratification authority,
correcting the failing review at lane `architecture-c1-stage2-complete`.

**On Stage 1.** An earlier form of this line stated flatly that "Stage 1 is complete" — an
unqualified status claim with no receipt, no reference, and no section owning it. The accurate
statement is narrower and is a **reference, not a finding of this ruling**: the block's own evidence
record asserts Stage-1 completeness — `docs/arch/refactor/rev11/evidence/C1/sequencing.md`, in the
declaration following the F24 entry that reads *"Stage 1 is now COMPLETE"* and enumerates what it
rests on (the ported algorithm, the real `ModuleResolverCore` type, and the differential harness across every named
branch). **This ruling neither re-derives that assertion nor ratifies it**, holds no witness to it,
and records no status for it in §1.5, §6.4, §7.4, §7.5, §7.6 or §7.7. What this ruling does is decline
to reopen Stage 1; nothing below decides anything about Stage 1 either way.

**Status:** RATIFIED — ratified by the architecture ratifying seat at lane
`architecture-c1-stage2-ratify-24`, with the open findings §14 records.

**How ratification is recorded, and why the digest is taken twice.** The seat ratifies this
document's body. Ratification is recorded by replacing **exactly the `**Status:**` line above** with
its ratified form and changing no other byte; the digest registered on trunk is computed on the
post-flip bytes. A diff between the reviewed bytes and the registered bytes must therefore be that
one line and nothing else, which makes the two-step auditable rather than a place where content can
enter unreviewed. The validator rejects a `RULING` whose `**Status:**` line declares it not ratified
— in `scripts/validate-program-state.mjs`, the `[[document]]` loop's `doc.kind === "RULING"` arm
parses the document's status paragraph through `parseStatusParagraph` and records a violation when
that paragraph is present and not ratified — so registering **this** document's explicitly
non-ratified form would correctly fail.

---

## 0. Authority — the mechanism exists, and the order is not circular

The earlier claim that this document could not be registered was true of its **path** and false of
the **mechanism**. The validator admits `CHARTER`, `AMENDMENT` and `RULING`, each confined to a
subdirectory of `docs/arch/refactor/rev11/` — in `scripts/validate-program-state.mjs`, the
`VALID_DOCUMENT_KINDS` set together with the `KIND_DIR` map that pins those three kinds to
`charters/`, `amendments/` and `rulings/`, enforced per document by the
`isPathUnder(KIND_DIR[doc.kind], …)` check. A ruling placed here is registrable. No governance infrastructure change is required.

**Activation order.** The steps below order the acts owned by their cited sections.

| Step | Act | Owner |
|---|---|---|
| 1 | Ratify these exact bytes (Status line flipped per above) | architecture seat |
| 2 | §1.6 SPEC 1 | trunk registry owner |
| 3 | §1.2–§1.3 | C1 |
| 4 | §1.6 SPEC 2 — **gated on step 6**, which must close first | trunk registry owner |
| 5 | CM1 reaches `ACCEPTED` | separate predecessor condition |
| 6 | Close §1.5's caller/deletion-inventory blocker through a ratified binding or a ratified amendment of the Stage-2 scope | architecture seat; trunk registry owner records the resulting authorization change |
| 7 | Immediately before dispatch, re-verify the pre-dispatch facts produced by steps 1–4 against the recorded scope, plus §7.6's clean-checkout-at-dispatch fact | C1 |

**The numbering lists the acts; a row that states a gate is governed by that gate.** Step 4 authors
the authorization, and §13.1's Q3 admits that act only once the governing prerequisites are valid —
so the blocking caller/deletion-inventory obligation §1.5 records must close, at step 6, BEFORE step 4
is performed. Authoring it earlier would satisfy this table's order and violate the ruling the table
serves.

**Why step 2 moves bytes and not only a digest.** A `[[document]]` row binds a digest to a PATH, and
the validator resolves that path against the tree it runs in: in
`scripts/validate-program-state.mjs`, the `[[document]]` digest loop reads the row's `path` and
records a violation when it *"does not exist on disk — authority is not bound to exact bytes"*, and a
second when the bytes present hash to anything other than the row's `sha256`. **This ruling's path is
carried by the block branch and is absent from trunk**, so a row added to trunk alone would fail the
very check that makes it authority. The mechanism is the one this registry already used for the C1
charter, in its own words — the digest is *"taken unmodified from block/module-resolver-core so the
two stay identical across that block's eventual landing."* Step 2 does the same for this ruling, and
step 3 takes the result back as an object rather than a copy.

Two consequences follow from that ordering, and **§1.3 owns both**: the shape of the deltas between
the identities, and which trunk-owned blob `DISPATCH` must carry. They are not restated here — an earlier
form of this section did restate them, and when §1.3 was corrected this copy went on asserting the
superseded reading. The only thing this section adds is the reason the ordering is not circular, above.
Whether either consequence is established is §1.5's.

What is worth stating here, because it is the reason the ordering exists at all: **a branch that
authorises itself is the self-authorisation shape however correct the bytes are.**

Step 3 before step 4 is the whole correction: the candidate must exist before anything can bind it.

§7.6 as a whole is not a pre-dispatch prerequisite. Its final-tree witnesses and final-checkout
binding can exist only after §4's implementation; C1 owns their execution and receipts before
landing, after §7.4's maintainer / A6 lock authority makes that cell callable against a consistent
pin. Section 7.6's authorization-document-closure and single-C1-authorization obligations remain
acts of the trunk registry owner, not C1. Only after all seven activation steps may Stage 2 be
dispatched. Nothing in this ruling authorises itself, and nothing in it waives steps 6 or 7.

---

## 1. Exact preflight identity

### 1.1 Fetch, and resolve what was actually fetched

A plain `git fetch <remote> <branch>` updates `FETCH_HEAD` and/or a remote-tracking ref — **not
necessarily the local branch of the same name**. Resolving the local name can therefore record a
stale ref as the baseline. The fetch uses an explicit refspec and resolves that exact destination:

```
REMOTE=<remote>
git fetch "$REMOTE" +refs/heads/program/architecture-lock:refs/c1-stage2/baseline
BASE=$(git rev-parse refs/c1-stage2/baseline)      # the destination just written — never a local branch name
BASE_TREE=$(git rev-parse "${BASE}^{tree}")
REMOTE_URL=$(git remote get-url "$REMOTE")
```

Record the fetch refspec, `REMOTE_URL`, the Git identity (§7.0), `BASE` and `BASE_TREE`.

**The baseline is BOUND here, not left to be derived.** Everything above is a *procedure for
obtaining* a baseline, and a procedure is not a baseline: read as authority it decides a proposition
about whatever the caller's ref happens to resolve to — the shape §0 already condemns, one step
earlier in the same document. The ratified scope requires this plan to bind one, so the value is
stated:

| Identity | Value |
|---|---|
| `BASE` | `ac7e6a1b0a1ea48dfcfa68990f2981942c36d141` |
| `BASE_TREE` | `8771538c5822737eaa7ed5214ec0231ea6b21e56` |

The procedure is retained as how an executor **confirms** those values, not how a reader discovers
them: a fetch resolving to something unrelated is now a mismatch against a stated constant rather
than a fresh baseline nobody can disagree with. **Stating the value settles what the baseline IS.**

**What remains to be established about it is NOT tip-equality, and an earlier form of this document
made exactly that mistake.** It required the freshly fetched trunk tip to *equal* `BASE` — a
proposition §0 steps 2 and 4 destroy by construction, since each is a trunk write that necessarily
moves the tip past the frozen value. A freshness test that a later required step falsifies is not a
test; it is a guarantee of failure wearing a check's clothes, and re-fetching later only re-creates
the same race one step further on.

The surviving obligation is stated in the monotone form instead, because trunk only ever moves
forward: **`BASE` is an ancestor of whatever the fetch resolves, and its tree is `BASE_TREE`.** A
later trunk commit preserves that relation rather than breaking it, so the check means the same thing
before step 2 and after step 4 — while still failing exactly where a baseline should fail, on one
taken from a fork, a stale mirror, or a rewritten history. Paired with it is the direction AB-1 turns
on: **`BASE` is an ancestor of the branch tip**, which is precisely what "the branch is behind the
baseline" denies. Both are obligations of §1.5, and neither is discharged here.

### 1.2 Freeze the prestart identity

`PRESTART` and `FINAL` are **different objects and are never denoted by one symbol.** The earlier
draft used `CAND` for both, in a document whose whole function is binding identities.

**`PRESTART` is a CAPTURED value, and it is the one identity in §1 that cannot be pre-stated.** An
earlier form of this subsection bound it to `601be304982fb34993148efc1f717e546911ac02` /
`d67fe2fa34434d7345b37e122439bd78b21e47f7`. **That binding is WITHDRAWN**, and not because a better
commit turned up: `PRESTART` has to carry the **ratified** bytes of this ruling, and the ratification
flip has not happened while these words are being written — the bytes it flips are these. Any value
stated here therefore predates the edits this document is still receiving, which is exactly what made
`PRESTART..RECORD` a multi-path delta instead of the single record path §1.3 requires. **`BASE` is
bindable and is bound; `PRESTART` is derived from an act that has not occurred, and writing it down
anyway is what broke.** The ratified scope requires the BASELINE to be bound. It does not require the
identities derived after ratification to be pre-stated, and they cannot be.

`PRESTART` is therefore DEFINED rather than tabulated:

> `PRESTART` is the branch commit that (a) has `BASE` as an ancestor, (b) carries this ruling at its
> path as **the same Git blob** §0 step 2 registered on trunk, and (c) immediately precedes the
> Dispatch Identity Record. It is captured at ratification time and recorded into the authorization
> scope (§1.4) as `c1_stage2_prestart` / `c1_stage2_prestart_tree`.

The rebase onto the bound `BASE` has already been performed; what remains is to take up the
registered bytes and freeze the result:

```
git rebase --onto "$BASE" <merge-base> <branch>    # already performed; explicit long timeout, see the hazard note

# take up the ratified bytes as the registered OBJECT, never a retyped copy
git fetch "$REMOTE_URL" "+refs/heads/program/architecture-lock:refs/c1-stage2/registered"
RULING_BLOB=$(git rev-parse "refs/c1-stage2/registered:docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md")
git cat-file blob "$RULING_BLOB" > docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md
git commit -a                                      # records the flip as an inherited object

test -z "$(git status --porcelain)"                # clean-state assertion
PRESTART_SHA=$(git rev-parse HEAD)                 # post-rebase, POST-flip, PRE-implementation
PRESTART_TREE=$(git rev-parse "HEAD^{tree}")
test "$(git rev-parse "${PRESTART_SHA}:docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md")" = "$RULING_BLOB"
git tag c1-stage2-prestart "$PRESTART_SHA"         # §8's defined return point
```

The blob-equality line is the whole reason the flip is routed through trunk rather than typed on the
branch: it makes "the branch did not mint its own ratification" a claim about one object id, settled
by anyone holding `PRESTART` and the registered ref. **Whether the abort return point is in fact the
commit and tree the scope records, and whether that commit carries `RULING_BLOB`, are obligations of
§1.5**: receipts for these steps are the executing party's own evidence, and a block does not rule its
own obligations satisfied on evidence it produced.

Every §7 result is REQUIRED to be produced on `FINAL_SHA`/`FINAL_TREE`, **except where an
obligation in this section explicitly requires a second arm on another bound identity.** V12 is that
case and the only one: a baseline comparison cannot be taken on the candidate alone, so its baseline
arm runs on `${BASE}` and cites `${BASE}`. The exception is scoped to the arm the obligation names;
it does not weaken the final-candidate binding for any other result, and a result that is not a named
comparison arm may not cite `${BASE}` at all. §1's record and the authorization scope bind
`BASE`/`BASE_TREE`, `PRESTART_SHA`/`PRESTART_TREE` and `RECORD_SHA`/`RECORD_TREE`. No
result may cite one where the other is required. Whether a §7 result was in fact produced on the
final checkout is **§7.6's**; this section states the requirement, not its status.

**Rebase hazard.** The replay onto the bound `BASE` was 164 commits. A default two-minute command
timeout SIGKILLs `git rebase` mid-sequence and there is no `timeout` binary on this host, so the
rebase runs with an explicit long timeout. If one is killed anyway, a `rebase-merge` directory may
represent a **completed** rebase: check whether the todo is empty and the ref already moved before
acting, and recover with `git rebase --quit` — never `--continue`, because `--quit` cannot move a
ref. Locate the state directory with `git rev-parse --git-dir`, never a literal path.

### 1.3 The Dispatch Identity Record, and the identities after `PRESTART`

Committing the record **changes `HEAD`**. `PRESTART` therefore cannot be the identity the
authorization binds, and verifying only the tag would let arbitrary committed changes after
`PRESTART` pass. Two further identities follow, and **they are not the same one** — an earlier
revision of this ruling used a single `DISPATCH` symbol for both and was unconstructible as a result.

Preflight adds exactly **one** path — the Dispatch Identity Record:

```
RECORD_SHA=$(git rev-parse HEAD)                    # post-record, pre-authorization
RECORD_TREE=$(git rev-parse "HEAD^{tree}")
```

**`RECORD` is what the authorization scope binds; `DISPATCH` is what Stage 2 is checked out at, and
the scope CANNOT name it.** The scope is written at §0 step 4, which is the act that lands the
authorization on trunk. The dispatch checkout is the one that has taken that change up, so it does
not exist until after step 4 completes — and a scope naming its sha would have to contain a hash of a
tree that contains that scope. **That is self-referential and cannot be constructed**, which is why
`DISPATCH` is defined RELATIONALLY instead of by a recorded hash:

> `DISPATCH` is the commit whose tree is `RECORD_TREE` with **one** path replaced by its trunk-owned
> blob — the authority-registry path by `AUTHZ_BLOB` — and whose `RECORD..DISPATCH` delta is exactly
> that path.

**Why one path, and why the second one was a symptom rather than a requirement.** An earlier revision
replaced this ruling's own path too, with the registered post-flip blob, on the ground that `RECORD`
sat on a branch still carrying the **pre-flip** bytes: a `DISPATCH` replacing only the registry would
pair a row naming the post-flip digest with a tree holding the unratified document, and the digest
binding could not hold. That reasoning was correct about the registry and wrong about where the fault
was. It bought a consistent `DISPATCH` by making `PRESTART..RECORD` carry the ruling's own edits
alongside the record — the one delta §1.3 requires to be a single path, and the identity §6.1 reads a
blob from. **Closing one half opened the other, which is the signature of a defect one level up:**
`PRESTART` was frozen **before** the flip. §1.2 moves it **after**, and the second replacement then
does not need balancing against the first — it disappears. At `PRESTART`, at `RECORD` and at
`DISPATCH` the ruling's blob is already `RULING_BLOB`.

**What survives is an ASSERTION OF EQUALITY, not a substitution**, and it has to be stated because no
delta observes it: a path absent from a delta is equally consistent with both sides being right and
with both being wrong.

```
test "$(git rev-parse "${RECORD_SHA}:docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md")" = "$RULING_BLOB"
```

`AUTHZ_BLOB` is fetched after §0 step 4 has landed:

```
git fetch "$REMOTE_URL" "+refs/heads/program/architecture-lock:refs/c1-stage2/authorized"
AUTHZ_BLOB=$(git rev-parse "refs/c1-stage2/authorized:docs/arch/architecture-lock/ledger/authority-registry.toml")
```

**Two deltas, one path each, and stating them separately is what removes the contradiction.**
`PRESTART..RECORD` is exactly the record preflight adds. `RECORD..DISPATCH` is exactly the trunk-side
registry write. The earlier revision collapsed these into a single `PRESTART..DISPATCH` delta over an
identity that could not be constructed; separating them makes each delta finite, enumerated and
decidable:

```
git diff --name-only "$PRESTART_SHA" "$RECORD_SHA"
# must equal exactly, and only:
#   docs/arch/refactor/rev11/evidence/C1/stage2-dispatch-identity.md

# run from the dispatch checkout; HEAD is that checkout, and the scheme defines
# no DISPATCH hash to dereference
git diff --name-only "$RECORD_SHA" HEAD
# must equal exactly, and only:
#   docs/arch/architecture-lock/ledger/authority-registry.toml
```

"Exactly" is bidirectional in both cases: the named path present, and no others. Stated
one-directionally — "no unpermitted path appeared" — the requirement is satisfied by a delta that is
empty, so a preflight that never happened would satisfy the requirement that it did; each pair of
identities must therefore also differ. That the registry blob at `DISPATCH` is `AUTHZ_BLOB` —
trunk's, **after** step 4, never `BASE`'s — rather than one this branch wrote, is a further
requirement and not a consequence of the delta being that path; so is the ruling-blob equality above.
As everywhere in §1.3, every value is read from the fetched trunk-owned object, never from ambient
shell state, and no two identities are denoted by one symbol. The status of every requirement in this
subsection is §1.5's.

The record path above is a planned destination created by this preflight; it is absent before the
record commit. The record contains: fetch refspec; `REMOTE_URL`; `BASE`; `BASE_TREE`; `PRESTART_SHA`;
`PRESTART_TREE`; the clean-state assertion; the §7.0 tool identity block; this ruling's registered
digest, and the blob id `RULING_BLOB` those bytes are. `RECORD_SHA`/`RECORD_TREE` are computed after
the commit and are carried into the scope, not into the record (a file cannot contain the hash of the
commit that adds it). `DISPATCH` appears in neither: it is defined relationally above, for the same
reason one step further out.

**Four identities, never interchangeable.** `RECORD` is the post-record, pre-authorization commit the
authorization scope binds. `DISPATCH` is what must be checked out when Stage 2 begins — `RECORD` plus
trunk's one change, named relationally because it cannot be named by hash (§1.3). `FINAL` is the
post-squash landed commit every §7 result is required to be produced on. No section may cite one
where another is required.

### 1.4 The authorization scope — machine-readable, in the dialect the registry accepts

The validator checks only that the scope is nonempty, so any semantic content is established here or
nowhere.

**The scope is EXTENDED, never rewritten, and the difference is not cosmetic.** The single C1
`[[authorization]]` on trunk already carries a long semantic `scope` — the charter/addendum
ratification text, ending *"Execution authority requires a further record."* **That text is authority
and must survive byte-for-byte.** A `scope` replaced by the key block below would erase it, and the
erasure would be invisible to every automated check: the validator tests only that the field is
non-empty, so the smaller, wronger value passes exactly as the larger one does. The key block is
therefore **appended** to the existing string; §1.6 SPEC 2 states the exact append, and this
subsection states only what the appended keys mean.

**The encoding is constrained, and the constraint was verified rather than assumed.** The registry's
TOML reader (`scripts/lib/rev11-toml.mjs`) accepts only single-line basic strings: triple-quoted
strings fail with *trailing content after string*, and escape sequences are rejected outright — the
same restriction that made an earlier branch registry unparseable. A multi-line or quoted key block
is therefore **not representable** in this file. Appending preserves that dialect rather than
straining it: the existing scope text carries no quote character and no backslash, and the appended
block is one run of semicolon-separated `key=value` pairs under the same rule:

```
 c1_stage2_ruling_sha256=<hex>; c1_stage2_base=<sha>; c1_stage2_base_tree=<sha>; c1_stage2_prestart=<sha>; c1_stage2_prestart_tree=<sha>; c1_stage2_record_commit=<sha>; c1_stage2_record_tree=<sha>; c1_stage2_record_sha256=<hex>; c1_stage2_baseline_remote_url=<url>
```

The appended key set is exactly:

| Key | Binds |
|---|---|
| `c1_stage2_ruling_sha256` | §0 step 2; §1.6 SPEC 1 |
| `c1_stage2_base` / `c1_stage2_base_tree` | §1.1 |
| `c1_stage2_prestart` / `c1_stage2_prestart_tree` | §1.2; §8 |
| `c1_stage2_record_commit` / `c1_stage2_record_tree` | §1.3 |
| `c1_stage2_record_sha256` | the Dispatch Identity Record's content |
| `c1_stage2_baseline_remote_url` | §1.1 |

**A remote NAME is not an identity.** `c1_stage2_baseline_remote_url` binds what the baseline is
fetched *from*: a remote name is a local alias whose target is mutable, so fetching from a
caller-named remote proves freshness against whatever that alias currently points at — a property of
the operator's config, not of the baseline. The URL is what this key records; its status is §1.5's.

**No key states WHERE A CHECK LOOKS, and no key binds a plan-owned script.** A scope that names the
place to read, as well as what the bytes must hash to, lets the caller redirect a check at a file of
its choosing — which is how an earlier self-check came to pass against unrelated bytes.
**The ban is on a key that DIRECTS a check, not on a key that RECORDS provenance.**
`c1_stage2_baseline_remote_url` is a URL and is admitted for exactly that reason: no check reads from
it, it is itself part of what §1.5 requires to be established, and recording the origin a fetch
claimed is what makes that obligation statable at all. A key whose value selects the subject of a
check is forbidden; a key whose value IS the subject is not.

Values are read **from the registered authorization**, never from ambient shell variables. A value
that exists only in an operator's environment binds nothing.

### 1.5 Pre-dispatch obligations — RECORDED UNMET

**The obligations do not disappear with the instrument (§10). They are recorded UNMET:**

| Obligation | Status |
|---|---|
| Record identity — that `RECORD` is the commit and tree the scope binds | **UNMET.** No plan-owned mechanism establishes it. |
| Dispatch identity — that the checkout Stage 2 begins from satisfies §1.3's relational definition of `DISPATCH`: `RECORD_TREE` with the authority-registry path replaced by `AUTHZ_BLOB`, and nothing else | **UNMET.** |
| Baseline lineage — that `BASE` is an ancestor of what a fresh fetch of the trunk ref resolves, with tree `BASE_TREE`, and that the branch is not behind it (§1.1) | **UNMET.** |
| Prestart identity and ratified-byte inheritance — that the abort return point is the commit and tree the scope records, and that its copy of this ruling is the Git blob `RULING_BLOB` trunk registered rather than a branch-authored copy (§1.2) | **UNMET.** |
| Preflight-delta closure — that `PRESTART..RECORD` is exactly the record path and `RECORD..DISPATCH` is exactly the authority-registry path, each in both directions, **and** that the ruling blob is equal — not merely undisturbed — at `PRESTART`, `RECORD` and `DISPATCH`, which no delta observes | **UNMET.** |
| Registry inheritance — that the branch authored neither the `[[document]]` row nor the `[[authorization]]`, and that the ratified bytes it carries are the registered object rather than a re-authored copy | **UNMET.** |
| Baseline remote binding — that the baseline was fetched from the URL the scope records, rather than from a mutable local alias | **UNMET.** §1.4 records the key; nothing asserts it. |
| Preflight execution — that §1's steps were performed at all, in the stated order | **UNMET.** |
| Ratification-byte closure — that the bytes registered on trunk differ from the reviewed bytes in exactly the `**Status:**` line and no other | **UNMET.** This document's ratification note states the requirement; nothing observes the diff. |
| Runtime and tool identity of the preflight procedure | **UNMET.** The preflight is a normative manual procedure, so its runtime and tool identity remain a real obligation. |
| **Caller/deletion inventory (ratified scope)** — that this plan binds the caller and deletion inventory the C1 `[[authorization]]` `scope` string names among the four things a Stage-2 execution plan must bind | **UNMET, and BLOCKING before dispatch.** §3 binds a DELETION roster of named definitions, but expressly disclaims exhaustiveness of the codebase and assigns reference discovery to §4's compiler fixpoint. A fixpoint is a discovery *procedure*, not an inventory: it terminates on green, and green establishes that no surviving reference is unresolved, never that any enumeration was complete. S2-R3 is likewise a universal final-state outcome, not an enumeration. **No sentence in this plan may be read as supplying the caller half**, and this ruling does not invent one it cannot derive. |

**One of these is blocking in a stronger sense than the others.** The caller/deletion inventory row is
not an evidence gap around a plan element that exists — it is a **ratified scope element this plan
does not carry**. The other rows above record that a stated thing is unproven; that row records that a
required thing is unwritten. Dispatch is therefore blocked on it independently of everything else
here, and closing it means either binding the inventory or obtaining an authority that amends the
ratified scope. Neither is C1's own act.

**An unmet obligation is not a waived one.** Recording these as unmet does not license dispatching
Stage 2 on the smaller proof, and this ruling does not claim it does. Whether Stage 2 may proceed at
all under a reduced evidence plan is not C1's to decide: removing working coverage requires the
charter's ratifying architecture authority, and that authority must preserve these obligations as
blocking unless it explicitly amends the charter.

### 1.6 Two specifications for the registry/ledger owner — SPECIFIED HERE, AUTHORED THERE

**Read this heading literally.** The two records below are written out so the acts §0 steps 2 and 4
name are exact rather than left to interpretation. **This plan SPECIFIES them and PERFORMS NEITHER.**
Both are trunk-side acts, and both write `docs/arch/architecture-lock/ledger/authority-registry.toml`
— a path no block branch may author (§13.1, Q3) — with SPEC 1 additionally placing this ruling's
ratified bytes at its own path on trunk. Each requires the registry/ledger owner's own approval and
authorship, and neither is enacted by ratifying this document. A specification the owner declines, or
amends, is the owner's call — what this section removes is the excuse that the act was ambiguous.

#### SPEC 1 — the `[[document]]` row for this ruling (§0 step 2)

**One new row. No other row is created, altered or re-pinned** — `C1-CHARTER` and the two
ratifiability/ratification ruling rows already exist, and **this ruling states no charter digest
anywhere, deliberately**: the charter's registered bytes are the registry's to pin and this document
must not become a second place they are written down.

| Field | Value |
|---|---|
| `id` | `RULING-2026-08-25-C1-STAGE2-CUTOVER` — it must be unique in the registry; the validator records a violation for a second `[[document]]` sharing an id, and no row for this ruling exists today |
| `kind` | `RULING` — one of the three kinds the validator admits |
| `path` | `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md`, which is where the kind requires a `RULING` to live |
| `sha256` | **taken on the POST-FLIP bytes** — the reviewed body with the `**Status:**` line replaced by its ratified form and no other byte changed |

The bytes must be present at that path in the tree the validator runs against, per §0 step 2; the row
and the bytes are one act, not two. The same digest is the value §1.4's `c1_stage2_ruling_sha256`
carries, and the blob holding those bytes is what §1.2 calls `RULING_BLOB` — one set of bytes named
three times, never three values to reconcile.

#### SPEC 2 — the IN-PLACE extension of the existing C1 `[[authorization]]` (§0 step 4)

**There is exactly one `[[authorization]]` for block `C1`, it already exists, and it is EXTENDED — not
replaced, not duplicated, not recreated.** The validator rejects a second record for a block and
requires one for a block past LOCKED, so both a duplicate and a delete-then-recreate are wrong; but
neither failure is the dangerous one. **The dangerous edit passes every check: a wholesale rewrite of
`scope` into the key block alone.** The field is only tested for non-emptiness, so replacing a long
ratified semantic scope with a short key string is invisible to the validator and destroys authority
that was recorded deliberately.

Two fields change, and only by appending:

| Field | Operation |
|---|---|
| `documents` | **APPEND** `"RULING-2026-08-25-C1-STAGE2-CUTOVER"` as a new final element. The two existing elements — `"C1-CHARTER"` and `"RULING-2026-08-24-C1-CHARTER-RATIFICATION"` — are retained in place, so the field ends with three ids. Dropping either while adding the third still satisfies the validator, which checks only that each listed id is known; §7.6 records the closure obligation. |
| `scope` | **APPEND** §1.4's key run to the end of the existing string, preceded by a single space, with **every byte of the existing value preserved as a prefix of the new one**. Nothing in the existing text is reworded, resequenced, shortened or re-punctuated. |

**The preservation requirement is stated as a prefix relation on purpose.** Quoting the current scope
text here would create a second copy of trunk-owned authority inside a block document — the exact
drift this plan refuses elsewhere — and a quoted copy goes stale the moment the owner corrects a word
of it. The check that does not go stale is: *the value the field held before the edit is a prefix of
the value it holds after, and the remainder is exactly the appended key run.* For orientation only,
the existing value opens *"Ratifies the C1 charter text"* and closes *"Execution authority requires a
further record."*; those are landmarks for the owner's eye, not a transcription.

Every other field remains byte-for-byte unchanged: `block` retains the existing block id, while
`ratified_by` and `ratified_at` retain the existing ratification metadata. SPEC 2 permits no field
other than `documents` and `scope` to be added, removed or edited.

## 2. Stage 2 intent contract

**Owned invariant.** After Stage 2, `verter_semantic::resolver_core::ModuleResolverCore` is the sole
production module resolver in the workspace, reached through one dependency direction
(`verter_workspace → verter_semantic`), with no forwarding wrapper, alias, feature flag or dual path
in any landed state.

**Required outcomes.**

| ID | Required |
|---|---|
| S2-R1 | `verter_workspace` depends on `verter_semantic`; `verter_semantic`'s production dependency closure contains none of `verter_workspace`, `verter_session`, `verter_scheduler` or `verter_tsgo_api` on any target, and `verter_semantic` has no `verter_workspace` dependency, normal or dev. This is C1-AC-2's invariant as the charter states it — its production-closure bullet in the Owned-invariant list, and the `C1-AC-2` row of its acceptance matrix — not a narrower half of it. |
| S2-R2 | The A5-DD1 upward exception row for `verter_semantic` is deleted, and the dependency-layer guard asserts both directions. |
| S2-R3 | Every production caller resolves through `ModuleResolverCore`. |
| S2-R4 | The workspace retry/replay driver satisfies all five F24 contracts (§5). |
| S2-R5 | The converted dual-runner coverage (§6) passes against the final production driver on the final candidate. |
| S2-R6 | `A6_META_COMPILE_40_COLD_RUST` and its two counters do not regress (§7.4). |

**Forbidden outcomes.**

| ID | Forbidden |
|---|---|
| S2-F1 | A landed state containing two production module resolvers, or a forwarding `ProjectResolver`/`NativeProjectResolver` wrapper or alias. |
| S2-F2 | A committed both-edges Cargo state. |
| S2-F3 | Code and dependency-guard flip landing in separate commits. |
| S2-F4 | A landed state where any test asserting the retained coverage was deleted rather than converted. |
| S2-F5 | Final evidence produced on a tree other than the final candidate — in particular, evidence from a harness that the same change then deletes. |
| S2-F6 | A new fuse table, or reweighting/reinterpreting the A6 cell after measurement. |

**Authority order.** `SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`
remains the sole query-time resolver, unchanged by the crate move. `ModuleResolverCore` is the module
resolver; it is not a second type-resolution engine and gains no query-time resolution role.

**Performance contribution.** Stage 2 is a relocation, not an optimisation: its contribution is
**neutrality**. It must not introduce cross-crate call overhead beyond ordinary function-call cost —
no new serialization, no heap round-trip at the crate seam, no added clone or `Arc` construction in
`ResolverContext` construction, and no new allocation per fact in the warm-hit
`validate_fact_signature` loop.

**What this section claims.** Each row is a finite, checkable proposition derived from the ratified
charter, stated as an outcome rather than a reference. **It does not claim any row is decided.**
Which rows are decided and which are unmet is not restated here. **§7.6 owns that status, except
S2-R6 and S2-F6, whose status is §7.4's; S2-F2 and S2-F3, whose status is §7.5's; and S2-F4's
conversion half, whose status is §6.4's.**

---

## 3. Move / stay / mirror / delete / retarget disposition ledger

**What this section claims, and what it does not.** It lists the definitions below and states the
criterion by which each class was derived. **It does not claim the list is exhaustive of the
codebase**, and no reader should treat its length as evidence of coverage. The authority is the
derivation criterion, not the enumeration; a definition that satisfies a criterion but is absent from
a list is still governed by that criterion, and discovering one is
ordinary fixpoint work under the scope rule below.

**Derivation criterion, per class.** MOVE: a definition that is dependency-neutral value or
algorithm and is named by production `verter_semantic` code. STAY: a definition that requires live
workspace state, host lifecycle, or cache authority. MIRROR: a definition whose two sides are
genuinely different domain rows rather than two encodings of one identity. DELETE: the superseded
resolver authority and any forwarding form of it. RETARGET: a test or fixture whose subject moves
but whose invariant does not.

**MOVE into `verter_semantic`.** The resolution algorithm and its four public surfaces on
`ModuleResolverCore` — `resolve_attempt`, `resolve_for_project_attempt`,
`preferred_specifier_candidates`, `project_exact_result`.

The core DTOs, enumerated rather than counted: `ProjectOwnership`, `ResolveRequestKind`,
`ResolvePhase`, `ResolutionContext`, `ProviderTarget`, `ResolutionKind`, `ResolveRequest`,
`ResolveResult`, `WorkspaceAlias`, `IdeProjectCompilerOptions`, `IdeProjectConfig`,
`ConfiguredMembership`, `StaticMembershipSpec`, `CompiledGlob`, `NormalizedGlob`.

The env-hash closure: `IdeProjectConfig`'s env-hash methods and `project_identity`, `EnvHashInputs`,
`ModuleResolutionMode`, `ConditionSet`, `SpecifierKind`, using the dependency-neutral `Hash16`.

The F25 five: `FactVersionRef` with F26's corrected full immutable value graph; `ProjectStableKey`
and `AmbientSymbolHit`; `PathProbe`; `WorkspaceAuthorityId`, `ResolutionPopulation`,
`ResolutionWorldId`.

The immutable membership predicate: `ConfiguredMembership::contains`,
`ConfiguredMembership::directly_includes`, `StaticMembershipSpec::matches`, compiled glob matching,
`typescript_default_excludes`.

**STAY in `verter_workspace`.** Named: `ProjectMembership` — config-ingress, no semantic re-export —
which **survives at `verter_workspace::membership::ProjectMembership`**, with
`verter_workspace::ProjectMembership` retained as the crate-root re-export. It is defined today inside
`crates/verter_workspace/src/resolver.rs`, so **it must be moved within `verter_workspace` BEFORE its
present location is
disturbed**; that ordering is a precondition, not follow-up. Also named:
`FallbackMembership`, `SupportedExtensions`, `materialize_from_spec`, config-ingress conversion;
`WorkspaceRead`, `ResolverSnapshot`, `EmptyResolverSnapshot`; `FactVersionValidator`, `FactReadSet`,
`CANDIDATE_CAP`; `resolve_tracked`, `resolve_for_project_tracked`, `TrackedResolutionCapability`,
`TransactionReader`; and the construction of a `ProjectStableKey` from a workspace project, which
today is the inherent method `ProjectStableKey::from_project` in
`crates/verter_workspace/src/project_key.rs` and becomes the **workspace free function
`project_stable_key_from_project`** the F25 deviation consult
(`docs/arch/refactor/rev11/evidence/C1/f25-deviation-consult.md`) decided — Rust's same-crate
inherent-impl constraint forces the free-function form once the type itself moves.

Beyond those names, the STAY criterion covers cache authority as a role — admission, validation,
mutation propagation, counters, compaction, replay ledgers, publication, invalidation. That is stated
as a **criterion, not a roster**: the criterion is that none of it crosses, rather than whether a
reader can name every member. **Nothing here decides that criterion.** A cache-authority definition
moved or copied into `verter_semantic` creates no upward edge, so the §7.2 dependency-closure test is
blind to it for the same reason it is blind to a duplicated STAY definition. Status is §7.6's.

**MIRROR — the single exception to MOVE.** `verter_workspace::error::DirEntry` stays canonical for
VFS; `RouteDirEntry` is semantic-owned; the session-side walker projects one onto the other.

**DELETE, with no forwarding form — and the unit is a DEFINITION, never a module.**
`ProjectResolver` including its inherent implementation surface; `pub type NativeProjectResolver`;
the private `ProjectResolver::preferred_specifier`; any forwarding resolver wrapper or alias; the
crate-root **resolver-authority** re-exports; `crates/verter_lsp/src/project_resolver.rs`; the
`verter_semantic → verter_workspace` normal dependency, the `test-support` dev-dependency and the
direct `verter_semantic → verter_scheduler` normal dependency (the `verter_scheduler` entry under
`[dependencies]` in `crates/verter_semantic/Cargo.toml`);
the A5-DD1 exception row; and the four legacy bridges
`verter_workspace::resolver::test_support::{legacy_resolve_with_reader,
legacy_resolve_for_project_with_reader, legacy_preferred_specifier_candidates,
legacy_project_exact_result}`.

**The scheduler edge is NOT inherited legacy, and describing it as such would misjudge the roster.** It is
absent at `BASE` and was introduced by this branch's own earlier work: the `verter_scheduler`
`[dependencies]` entry in `crates/verter_semantic/Cargo.toml`, reached from the
`use verter_scheduler::invalidation::Hash16;` import at the head of
`crates/verter_semantic/src/resolver_core/project_stable_key.rs` — the same type the
MOVE roster above already specifies **in its dependency-neutral form**. Deleting the edge completes
that MOVE rather than reversing an older decision.

**`verter_workspace::resolver` and `crates/verter_workspace/src/resolver.rs` SURVIVE.** An earlier form of this entry deleted the
MODULE, and that was a container disposition standing in for the API inside it: the file also defines
**21 public non-authority items** — four config/value types, seven carrier helpers and constants, and
ten general path helpers — carrying a large reference closure across roughly nine crates, none of
which is the module resolver. Deleting the container would have removed them silently while §3 said
nothing about any of them. Only resolver-authority exports are deleted; the selective non-authority
exports survive, and **the no-forwarding prohibition applies to the resolver authority only.**

**The 21 non-authority definitions, dispositioned explicitly rather than inherited from the file they
sit in:**

- **MOVE / canonicalize in `verter_semantic`** — the config types `WorkspaceAlias`,
  `IdeProjectCompilerOptions`, `IdeProjectConfig`; the ten general path and known-file helpers
  `normalize_canonical_id`, `collapse_path`, `join_paths`, `parent_dir`, `is_relative_specifier`,
  `is_absolute_specifier`, `build_known_file_index`, `resolve_known_dependency_id`,
  `resolve_known_dependency_base`, `normalize_known_file_id`; and the carrier projection definitions
  the semantic resolver consumes — `carrier_ide_provider_path`, `carrier_api_provider_path`,
  `path_is_carrier`, `CARRIER_API_VIRTUAL_SUFFIX`.
- **STAY in `verter_workspace`** — `ProjectMembership` at the path named above;
  `carrier_source_extensions`, `strip_carrier_extension`, `CARRIER_API_MODULE_SPECIFIER_SUFFIX`.
- Workspace **compatibility re-exports of non-authority utilities may remain**.

**Source-absence obligations.** MOVE requires that a relocated definition leave nothing behind at its
source; STAY requires that a retained definition not be copied into `verter_semantic`. Both are
obligations of this section — **see §7.6 for their status.**

**RETARGET, not delete.** `raw_resolver_entry_points_are_private` (compile-fail) to the new private
attempt boundary; `crates/verter_workspace/src/resolver_tests.rs` moved and parameterised around
attempt views; `crates/verter_workspace/src/resolution_witness_contract_tests.rs` preserved as
public-boundary characterization. Both are named with their present owning paths so the RETARGET
subjects are unambiguous — a bare filename does not identify a file in a workspace this size.

**Scope rule.** An additional caller or definition discovered during execution is ordinary
compiler-fixpoint work (§4) and requires no re-ratification — **unless** it changes ownership, public
compatibility, abort scope, or a ratified outcome, in which case it is a finding to be dispositioned
before proceeding.

**What this section claims, and it is narrow.** Every listed definition is an existential obligation
checkable by name in the final tree, and the section states what it lists and how each class was
derived. **It claims nothing about those obligations being discharged**, and it does not assert that
its lists exhaust the tree. The discharge status of every class is §7.6's.

## 4. One atomic transition, and the compiler-repair fixpoint

**Work order.**

1. Establish the semantic-owned values, workspace projections, and workspace value re-exports (§3),
   honouring §3's within-crate relocation precondition before step 6 removes the crate-root authority
   re-exports. Which definitions that covers, and why the ordering binds, are §3's; naming them again
   here is how two copies of one roster start to differ.
2. Repoint the inert kernel so production `verter_semantic` names no `verter_workspace` type.
3. Reverse both Cargo edges and flip the whole dependency-guard cluster **together** — code-first
   leaves the guard too permissive, guard-first false-fails, so same-commit is the only correct
   choice. The three edits are: remove `verter_semantic` from `ratified_upward_exceptions()`
   (keeping `verter_diagnostics`, which has an independent reason); shrink `RATIFIED_ROOT_CRATES` to
   `&["verter_diagnostics"]`; replace the semantic→workspace canary with a both-directions
   assertion.
4. §5.
5. §2 (S2-R3).
6. §3; §6.
7. §7.

**The compiler-repair fixpoint.** Intermediate rounds are working-tree iteration and are NOT
evidence: no round is receipted, and the evidentiary claim is narrowed to the single unchanged
post-squash run required by step 7. `cargo` does not check dependents of a crate that failed, so a single red run shows only
the lowest failing layer — an observed property of this repository, recorded at
`docs/arch/architecture-lock/ledger/A1/command-proofs/02-cargo-clippy.txt`, where a full-workspace
invocation terminates at `error: could not compile verter_session (lib)` with `EXIT: 101` without
reaching that crate's dependents. Therefore:

1. Run the §7.1 and §7.3 commands, collecting diagnostics.
2. Repoint what that round reports.
3. Repeat until every §7.1 and §7.3 command is green on one unchanged tree.

Errors surface in dependency order. **Termination is green across the §7.1 and §7.3 commands on the
final candidate; that, and only that, is what the fixpoint claims.** It makes no static-universe
claim, and no intermediate round claims anything.

**What this section claims.** Each round reports actual unresolved references in the configurations
executed, never an unexecuted command's selection. That narrowing is the whole of the claim; whether
those executions were recorded and bound to the final tree is §7.6's.

---

## 5. Real-driver acceptance matrix (F24) — pinned test identities

The repository preserves only the five labels; the concrete table existed solely in uncommitted
output — the F24 deviation consult
(`docs/arch/refactor/rev11/evidence/C1/f24-deviation-consult.md`) records under its **Command:**
heading that the full prompt and output were `/tmp/c1-f24-prompt.md` and
`/tmp/c1-f24-output.md`, were "not committed", and do not survive in this checkout. It is
committed here.
**A row naming a test category has not met the standard; every row below names a test identity.**

**Driver home, and the entry points named as they actually exist.** The production retry/replay
driver lands in `verter_workspace`. An earlier form of this section named the tracked entry points
`Engine::resolve_tracked` and `Engine::resolve_for_project_tracked`; **no such methods exist.** The
tracked pair are `ProjectResolver` methods in `crates/verter_workspace/src/resolver.rs` —
`ProjectResolver::resolve_tracked` and `ProjectResolver::resolve_for_project_tracked`, each taking a
`&TrackedResolutionCapability` and a `&TransactionReader` and delegating to the private
`resolve_with_reader` / `resolve_for_project_with_reader`. That pair is the **internal resolver
seam**. The `Engine` methods that reach it are differently named, and there are three of them in
`crates/verter_workspace/src/engine.rs`:

| Production entry point (`Engine::`) | Seam it calls |
|---|---|
| `resolve_import_outcome_in_published` | `ProjectResolver::resolve_tracked` |
| `resolve_parsed_edge_in_world` | `ProjectResolver::resolve_tracked` |
| `resolve_import_for_project_outcome` | `ProjectResolver::resolve_for_project_tracked` |

Each constructs `TransactionReader::new(reader, &transaction)` plus `TrackedResolutionCapability::new()`
at the call site before invoking the seam. After Stage 2 the seam drives
`ModuleResolverCore::resolve_attempt` / `resolve_for_project_attempt`, consumes
`AttemptOutcome::{Complete, NeedInputs, Terminal}`, and replays consumed selectors into versioned
facts; the three `Engine` entry points above are the production callers that reach it.

**Test home.** `crates/verter_workspace/src/resolution_driver_tests.rs` is the planned destination; it
does not exist before the transition. It is a sibling `*_tests.rs` unit-test module of the
`verter_workspace` lib target and adds no integration-test binary, per the anti-binary-growth layout
rule. Its registration status is §7.6's.

| ID | Contract | Production entry point | Test identity (`resolution_driver_tests::`) | Required observation | Forbidden observation |
|---|---|---|---|---|---|
| F24-1 | Manifest fingerprint preserves `name` | driver fulfilment of `InputKey::PackageManifest{directory}` → the `name` field of `PackageManifest` in `crates/verter_workspace/src/types.rs` | `manifest_name_only_edit_changes_the_replayed_fingerprint` | after rewriting a manifest changing **only** `name`, the replayed manifest fact's version differs and the second resolve does not reuse the first answer | the `name`-only edit is fingerprint-invisible and a stale resolution is served |
| F24-2 | `DirectoryMembers` consumed-vs-prefetched | driver fact-replay over the `DirectoryMembers` variant of the `ResolutionFactKey` enum in `crates/verter_semantic/src/facts/resolution.rs` (there is no `ResolutionFact` type; an earlier form of this row named one) | `directory_members_signature_records_only_consumed_members` | a resolve that enumerates a directory but consumes one member records **only** that member in the signature | prefetched-but-unconsumed members enter the signature, over-invalidating |
| F24-3 | Complete fact replay / signature | driver selector→fact replay across every `NeedInputs` wave | `every_consumed_selector_replays_once_in_wave_order` | for the workspace-alias resolve, the ordered selector sequence is manifest-check < path-probe < realpath with `waves >= 3`, and every consumed selector appears exactly once in the replayed `ResolutionFactKey` set | a consumed selector missing from the signature, or order lost to set-semantics |
| F24-4 | Basis restart on the real driver | driver loop restart on `ResolutionWorldBasis` change mid-flight | `basis_change_mid_flight_restarts_cleanly_on_the_new_basis` | the resolve restarts on the new basis and returns the post-change answer | a torn result mixing pre- and post-change observations, or a hang |
| F24-5a | No-progress | the `InputResolutionNoProgress` variant of the `AttemptFailure` enum in `crates/verter_semantic/src/resolver_core/attempt_outcome.rs` | `unsatisfiable_input_surfaces_terminal_no_progress_with_unresolved_keys` | `Terminal(InputResolutionNoProgress { unresolved })` carrying the unresolved keys | an infinite retry loop, or a silent `Complete` with unresolved inputs |
| F24-5b | Limit breach | the five `InputResolution*Limit` variants of that same `AttemptFailure` enum (`AttemptLimit`, `UniqueKeyLimit`, `ByteLimit`, `DepthLimit`, `ChurnLimit`) | `limit_breach_surfaces_terminal_with_the_breached_limit` | `Terminal` naming the breached limit and its unresolved keys | the breach is swallowed and resolution continues past the limit |
| F24-5c | Transient load failure | driver retry path around a failing then succeeding load | `transient_load_failure_retries_and_completes` | the driver retries and returns `Complete` | the transient failure is promoted to `Terminal`, or retried unboundedly |

**Invocation.** Each row is executed individually. The rows below are the required invocations; what is
established about their execution is §7.6's:

```
cargo nextest run -p verter_workspace --lib -E 'test(=resolution_driver_tests::<NAME>)'
```

Its evidence would record the exact filter, `FINAL_SHA`/`FINAL_TREE`, the tool identity block, exit
status, and the enumerated test list nextest reports — which would distinguish the named test running
from some test running. That distinction is a requirement on whoever executes the row; §7.6 records
its status.

**No result from this matrix may be promoted warm.** A `Terminal` or partial outcome never publishes
a cache entry, per the completion fence already in force.

**What this section claims: the SPECIFICATION, never the execution.** Each row names a production
entry point, an exact test identity, an exact invocation, and both a required and a forbidden
observable result — so a row cannot be satisfied by a category or an inference. Whether any row was
invoked, and whether its execution was bound to the final tree, is §7.6's.

---

## 6. Legacy deletion with retained final-state tests

The earlier instruction to delete the entire dual-runner was wrong: it would remove the only
branch-complete semantic evidence in the block. Correct disposition:

**Retain and CONVERT** every new-side case into a production-driver regression test that runs on the
final candidate.

### 6.1 The PRESTART source roster

An earlier draft claimed to anchor "all 24" cases and in fact anchored 22, silently — a completeness
claim a hand-written enumeration did not carry. The fix is to define the set by its SOURCE rather than
to list it:

The roster is resolved at §6 execution time against the `PRESTART_SHA` recorded by preflight:

```
git show "${PRESTART_SHA}:crates/verter_semantic/src/resolver_core/resolution_dual_runner_tests.rs"
```

Whoever executes §6 reads that blob, extracts its `#[test]` function names, and uses the resulting
set directly; **no plan-owned instrument does this** (§6.4). **If it yields any set other than the 24
in §6.2, this section is stale and Stage 2 stops**
until §6.2 is updated; the count is not the authority, the resolved set is.

**No conversion manifest is committed, and this is a deletion rather than a repair.** The earlier
form ran a shell pipeline at preflight and committed its output into the ledger directory. That
artifact was wrong three ways at once: §1.3 permits exactly one file in the preflight delta and this
was a second, so the plan forbade the very artifact it required; nothing derived it from `PRESTART` or bound its digest, so any file with plausible contents
satisfied the check; and the pipeline that produced it could fail silently. Resolving that execution-time object removes all three — there is no artifact to permit, to bind, or to forge, and the tree
the set comes from is named in `PRESTART`, an identity whose status is §1.5's.

### 6.2 The dispositioned cases

All 24 **are required to be** CONVERTED — re-pointed at the production driver, preserving assertions
and oracles. This section states that requirement; whether the PRESTART roster is in fact those 24,
and whether the conversion preserved what each case tested, is §6.4's, as are the destination and the
mapping rule. Conversion means the legacy-versus-kernel comparison is replaced by an assertion
against the production driver's result; no oracle is weakened, and the witness comparisons keep
comparing witness sets rather than only `resolved`.

**The Line column is resolved at §6 execution time against the `PRESTART_SHA` object named in §6.1,
not against a working tree.** The Function column is the primary identity in every case; the line is
a convenience for reading that object.

| # | Line | Function | Disposition |
|---|---|---|---|
| 1 | `:993` | `kernel_runner_positive_case_matches_the_witness_contract_result` | CONVERT — already kernel-side; re-point at the production driver. |
| 2 | `:1010` | `kernel_runner_restarts_cleanly_on_a_basis_change` | CONVERT — already kernel-side; re-point at the production driver. |
| 3 | `:1055` | `kernel_runner_miss_case_matches_the_witness_contract_result` | CONVERT — already kernel-side; re-point at the production driver. |
| 4 | `:1067` | `resolution_witness_positive_case_kernel_matches_legacy` | CONVERT — legacy side removed; the witness-contract oracle is retained against the production driver. **Omitted from the previous draft's anchor list; this row is the correction.** |
| 5 | `:1103` | `resolution_witness_miss_case_kernel_matches_legacy` | CONVERT — legacy side removed; the witness-contract oracle is retained against the production driver. **Omitted from the previous draft's anchor list; this row is the correction.** |
| 6 | `:1223` | `full_driver_resolves_a_relative_specifier_for_an_owned_importer` | CONVERT — assertions preserved against the production driver. |
| 7 | `:1248` | `full_driver_resolves_via_a_workspace_alias` | CONVERT — assertions preserved against the production driver. |
| 8 | `:1309` | `full_driver_resolves_via_tsconfig_paths` | CONVERT — assertions preserved against the production driver. |
| 9 | `:1333` | `full_driver_resolves_via_the_base_url_fallback` | CONVERT — assertions preserved against the production driver. |
| 10 | `:1351` | `full_driver_resolves_via_a_project_reference` | CONVERT — assertions preserved against the production driver. |
| 11 | `:1380` | `full_driver_a_project_reference_cycle_terminates_on_both_engines` | CONVERT — assertions preserved against the production driver. |
| 12 | `:1414` | `full_driver_resolves_via_hash_imports` | CONVERT — assertions preserved against the production driver. |
| 13 | `:1444` | `full_driver_resolves_via_node_modules_exports_with_conditions` | CONVERT — assertions preserved against the production driver. |
| 14 | `:1473` | `full_driver_resolves_a_scoped_package_via_legacy_main_field` | CONVERT — assertions preserved against the production driver. |
| 15 | `:1504` | `full_driver_resolves_via_explicit_project_ownership` | CONVERT — assertions preserved against the production driver. |
| 16 | `:1523` | `full_driver_owner_overlap_selects_the_nearest_root` | CONVERT — assertions preserved against the production driver. |
| 17 | `:1552` | `full_driver_a_full_chain_miss_agrees_on_both_engines` | CONVERT — assertions preserved against the production driver. |
| 18 | `:1583` | `full_driver_resolves_an_absolute_specifier_for_an_owned_importer` | CONVERT — assertions preserved against the production driver. |
| 19 | `:1604` | `full_driver_workspace_alias_wins_over_tsconfig_paths_and_base_url` | CONVERT — assertions preserved against the production driver. |
| 20 | `:1640` | `full_driver_a_dangling_project_reference_falls_through_without_panicking` | CONVERT — assertions preserved against the production driver. |
| 21 | `:1670` | `full_driver_resolves_via_node_modules_exports_array_form` | CONVERT — assertions preserved against the production driver. |
| 22 | `:1695` | `full_driver_carrier_import_provider_projection_matches_legacy_end_to_end` | CONVERT — assertions preserved against the production driver. |
| 23 | `:1751` | `full_driver_preferred_specifier_candidates_agrees_with_legacy` | CONVERT — assertions preserved against the production driver. |
| 24 | `:1778` | `full_driver_project_exact_result_agrees_with_legacy` | CONVERT — assertions preserved against the production driver. |

### 6.3 Coverage that must survive conversion

Named so a reviewer can check the converted suite without re-deriving intent: the four core surfaces
(`resolve_attempt`, `resolve_for_project_attempt`, `preferred_specifier_candidates`,
`project_exact_result`); every resolution branch — relative, absolute for an owned importer,
workspace alias, tsconfig `paths`, `baseUrl` fallback, project reference including a genuine A↔B
cycle proven to terminate, `#imports`, `node_modules` exports-with-conditions and the array form,
legacy scoped-package main field, explicit-project ownership, owner-overlap nearest-root selection, a
dangling project reference falling through without panicking, and the full-chain miss; precedence
competition (alias over `paths` over `baseUrl`); carrier/provider projection end to end; ordered
consumed selectors and `NeedInputs` wave counts; the complete `ResolveResult` DTO comparison, not
merely `resolved`/witness; basis restart; and the witness-contract positive and miss oracles.

The differential's purpose ends when the legacy engine does; the coverage's purpose does not.

**What this section claims.** Every function is dispositioned by name and line, and the source set is
identified as the `#[test]` roster of a named Git object rather than a hand-written list — an
**identification**, not a resolving instrument; whoever executes §6 resolves it, and §6.4 records
what follows from that. The converted tests are REQUIRED to run against the production entry point on
the final tree, which is what would make them evidence about what ships rather than about a harness
that no longer exists; whether they do is §6.4's.

### 6.4 The conversion mapping — RECORDED UNMET, with the destination and rule still normative

**Destination.** All converted cases land in the planned, presently absent
`crates/verter_workspace/src/resolution_conversion_tests.rs` — a sibling `*_tests.rs` unit-test
module of the `verter_workspace` lib target, the crate that owns the production driver. It adds no
integration-test binary, per the anti-binary-growth layout rule. **The module must be declared by its
parent**, or it compiles into nothing and the canonical gate never selects it; an orphan file in the
tree is not a converted test.

**Mapping rule.** Each case keeps its function name; only its home and its oracle change. The mapping
is therefore the identity on names, which makes it checkable without a 24-row correspondence table
and leaves nothing to interpretation. Both destination and rule remain normative.

**The generator that checked the mapping is deleted, and the mapping obligation is UNMET.** V20 read
both blobs from Git — the right sources — and still could not decide its proposition, because the
name extractor was a line-oriented regex over source text. It counted textual patterns, not test
cases: one genuine `#[test]` plus twenty-three inside a `/* */` block comment yields twenty-four
names, twenty-four of them distinct, satisfying the check while a single real case exists. That
defect survived three successive repairs in the same shape — a subset relation, then a bare count,
then a distinct count — each closing the previous counterexample and leaving the class open. A text
scanner cannot decide what is a compiled test; only the compiler and the runner can, and the runner
family was already deleted as unauthenticable (below).

| Obligation | Status |
|---|---|
| No converted case name was dropped (part of S2-F4) | **UNMET.** |
| The PRESTART roster is exactly the documented 24 cases | **UNMET** — the stronger identity claim was never established, only approximated. |
| The converted module is registered and therefore selected by the canonical gate | **UNMET** — an unreferenced file satisfies any read of its bytes. |
| The conversion preserved what each case tested | **UNMET** — no instrument here reads a case body. |
| Each of the 24 named cases executed | **UNMET** — see the runner note below. |
| The dual-runner harness was green BEFORE its legacy-side runners were deleted, and in that order (§6) | **UNMET.** Nothing observes the harness's result, and nothing observes that the deletion followed it rather than preceded it. |

**The runner (V21) was deleted first, and for a different reason worth keeping distinct.** It
asserted that the converted cases actually executed. `cargo` resolves through `PATH`, which the
invoker owns, so asking that binary to identify itself authenticated nothing — a substituted binary
answers the probe and prints a convincing summary having run no tests. Its per-case enumeration
parsed the shared human output stream, which `NEXTEST_SUCCESS_OUTPUT=immediate` lets a single passing
test write arbitrary result lines into, and its pattern was independently inverted against reality:
it required two tokens after the duration where real output carries three
(`PASS [ 0.059s] (10/94) <binary> <name>`), so it rejected genuine lines and accepted fabricated
ones — the signature of a check that had only ever been read, never executed against real output. A
process cannot authenticate a subprocess in an environment its caller controls.

**Where these obligations should end up.** Registration, selection and the surviving-coverage
properties are durable facts about the final tree, so they belong in ordinary repository tests the
canonical gate selects (§7.2). Until such tests exist, the rows above stand as unmet and this ruling
claims nothing further.

## 7. Exact-candidate evidence manifest

All evidence is produced on **one final candidate tree** — `FINAL_SHA` / `FINAL_TREE`, after every
deletion in §3 and §6 and after the squash. Passing a harness and then deleting that harness is not
proof about the final candidate (S2-F5). This is a list of **obligations with exact procedures**, not
a coverage claim and not a record of execution: it states that these named executions are REQUIRED to
run and pass on this tree. Whether they did, and whether the tree they ran on was the final candidate
unchanged, is §7.6's.

Every result is required to record: the exact command; selected targets; the §7.0 tool identity block;
`FINAL_SHA`; `FINAL_TREE`; exit status; the test enumeration the runner reported; and artifacts.

### 7.0 Tool identities recorded with each receipt set

**What this block claims.** It records the identities of the tools the receipts invoke, derived from
the command set in §7.1–§7.3. **It does not claim to enumerate every executable reachable
during a build**, and it is not falsified by naming one it omits — a shell's transitive utilities
cannot be enumerated to closure, so a "complete tool identity" claim could never hold.

**This block claims nothing about scripts (§10).** Every command §7 names is a named tool invoked
directly, whose identity is recorded below. Nothing here is re-homed into a shell pipeline, which
would reintroduce exactly the unenumerable-utility dependency this block exists to record.

Recorded per receipt set:

```
rustc --version                cargo --version               cargo clippy --version
cargo fmt --version            cargo nextest --version       git --version
node --version                 pnpm --version
sha256 of rust-toolchain.toml  # plus the assertion that rustc/clippy match its pin
```

A result from a toolchain other than the `rust-toolchain.toml` pin is not evidence about the pinned
configuration and is rejected.
### 7.1 Canonical project gates

| # | Exact command | Obligation |
|---|---|---|
| V1 | `node scripts/gate.mjs` | the canonical Rust gate, `node_modules` present, within the resource ceilings; a PASS is Surface 1 with the shipped-cfg skip disclosed |
| V2 | `cargo clippy --workspace --all-targets -- -D warnings` | lint clean across every host target kind |
| V3 | `cargo fmt --all --check` | formatting |
| V4 | `cargo check --workspace --release` | names resolvable only under `debug_assertions` (lib+bins) |
| V5 | `cargo clippy --target wasm32-unknown-unknown -p verter_wasm --all-targets -- -D warnings` | the wasm32 arms of `verter_wasm`'s closure |
| V6 | `pnpm test` | TypeScript consumers of the relocated surface |

**What V1 is claimed to establish, and what it is not.** V1 is the repository's canonical gate, and
this plan claims exactly the contract that gate documents **today**: that Surface 1 built and passed,
with the shipped-cfg lane's skip disclosed. It does **not** claim coverage completeness, executed-count
parity against an independently derived inventory, or that every intended test was selected — the
repository states that complete execution-proof enforcement has not landed (`CLAUDE.md`'s
"Verification Must Prove Execution"), and `docs/arch/gate-integrity-ledger.md` records both the guard
that would close it — row `GI-3`, `gate_contract_integrity` — and, at row `GI-17`, that Surface 1
still asserts no tree-derived executed-count floor.

That gap is **not C1's to close and is not assumed away**: it belongs to the gate-integrity block,
which has no active owner at the time of writing. This plan is therefore designed to stand on the
gate's present contract alone. Nothing here is contingent on GI-3 or GI-17 landing, and no obligation
in §7 is discharged by a stronger reading of V1 than the gate itself currently supports.

### 7.2 Structural and dependency obligations — owned by the repository, not by this plan

These invariants are **durable properties of the codebase**, not facts about one candidate. They are
ordinary repository tests, and they live in the repository's own test universe rather than in this
plan. **This plan does not re-run them as private receipts.** See §6.4 and §7.6. A plan-owned copy
of a repository test proves nothing the repository does not already prove, and creates a second
universe whose completeness has to be argued separately.

| Invariant | Where it lives | Selected by |
|---|---|---|
| The production-layer firewall and equality-pinned upward-exception set over normal and build edges — the production-closure portion of S2-R1, excluding its dev-edge and positive replacement-edge requirements | `verter_identity` — `workspace_dependency_layers::workspace_production_closures_never_cross_upward_except_the_recorded_exception` | V1 |
| the exception edges' target conditions, so a target-gated resolve cannot silently shrink the recorded reach | `verter_identity` — `workspace_dependency_layers::the_ratified_exception_records_its_target_condition_precisely` | V1 |
| Closure-walk non-vacuity: `verter_session` reaches the five named deep dependencies, and the pre-cutover `verter_semantic` closure reaches `verter_workspace` | `verter_identity` — `workspace_dependency_layers::closure_walk_is_non_vacuous_for_known_deep_reaches` | V1 |
| authority uniqueness after the move (C1-AC-3) | `verter_semantic` — `project_semantic_dispatch_invariants`, relocated with the module it tests as C1-AC-3 requires (it is in `verter_session` until the transition moves it) | see §7.6 |

**What the first row's test actually does — it is not a `verter_semantic` probe.** It walks the whole
layered production graph from `cargo metadata --all-features`, without `--filter-platform`, so every
target-gated edge is unioned; that union is what licenses "on every target", and it is a property of
the invocation rather than of the crate. It then treats **both** recorded exception roots identically:
`RATIFIED_ROOT_CRATES` holds `verter_semantic` **and** `verter_diagnostics`, and
`ratified_upward_exceptions()` maps each to the same set `{verter_workspace, verter_scheduler,
verter_tsgo_api}` — both in
`crates/verter_identity/tests/cases/workspace_dependency_layers.rs`. The map is equality-pinned rather
than subset-checked, so once `verter_semantic`'s entry is deleted its expected upward set is empty and
any surviving member fails. `verter_diagnostics` keeps its entry (§4 step 3) and is untouched by this transition.

**The equality pin is also what carries S2-R2's deletion half**, in the one direction that matters: an
entry left in place while the edges are gone makes the expected set the three crates and the observed
set empty, and the assertion fails. It does not follow that the pin detects every way the row could be
wrong — an entry left in place beside edges left in place passes it, which is the state before the
transition and not one this row distinguishes.

**Two things that test does NOT do, and an earlier form of this row claimed both.**

- **It does not check dev-dependencies, so S2-R1's dev half is not covered by it.** The resolve graph
  is built from `dep_kinds` whose kind is normal or `build`; dev edges are dropped at construction.
  See §3 for the separate dev-dependency disposition.
- **It does not by itself assert the positive `verter_workspace → verter_semantic` edge**, so it does
  not carry "both directions" alone. A closure reaching nothing upward is equally consistent with the
  two crates wired the new way and with their not being wired at all. The pair is named today only by
  the non-vacuity canary's assertion that `verter_semantic` reaches `verter_workspace` — the OLD
  direction. §4 owns the replacement.

**The retargeted compile-fail fixture is NOT listed above, because V1 does not select it.** The
canonical gate excludes the entire `cases::g_compile::compile_fail::` prefix, so a fixture placed
there is never executed by the gate and cannot be cited as a repository-owned witness. That is the
mechanism; the status that follows from it is §7.6's.

**The former text-scan absence check for S2-F1 is deleted rather than repaired.** The previous form
was `! grep` over the source tree for each deleted name. It fails on two independent grounds and
neither is fixable by rewriting the pattern:

- **It is fail-open.** `grep` exits non-zero both when it matches nothing and when it fails to
  execute; negated, an execution error is indistinguishable from proven absence. A check whose error
  state is its success state cannot be evidence.
- **It is a name-keyed source scanner**, which this repository forbids as landed enforcement
  (`CLAUDE.md`, "Landed guards are structural, never name-keyed file scanners"). Adopting one here
  as a normative proof step would contradict a rule this plan is otherwise bound by.

Absence is instead a consequence of compilation **for the part compilation can carry, which is
narrower than the obligation.** This subsection owns where that boundary falls; §7.6 owns what the
remainder's status is.

What compilation proves: a *referenced* name that no longer exists is a compile error, so V1 cannot
pass while any surviving reference to the deleted authority remains. That covers re-introduction by
use, which is how the edge would actually be recreated.

**What compilation does NOT prove: that a dead item is gone.** An unused legacy definition, a
forwarding wrapper nothing calls, an alias left behind — all compile cleanly. So "the old authority,
its aliases and every forwarder are absent" is a proposition compilation does not reach, and claiming
it on compilation's strength would be the same defect as the text scan above: an instrument reporting
on something it never examined.

Nothing in this plan asserts absence by searching text.

### 7.3 Known affected target-specific obligation

| # | Exact command | Obligation |
|---|---|---|
| V11 | `cargo check -p verter_session --target wasm32-unknown-unknown --tests` | the wasm-gated `verter_session` **test** targets compile against the relocated surface, resolving the `FactVersionRef::FileWholeHash` reference inside `crates/verter_session/src/meta_tests.rs`'s `#[cfg(target_arch = "wasm32")]` test `current_dependency_fact_versions_include_derived_resolver_facts_non_scheduler` at its new home |

V11 is a terminating command, not an instruction to "build the wasm tests". It exists because this
cell is **known** affected and is selected by none of V1–V6: excluded from every host lane by
`#[cfg(target_arch = "wasm32")]`, and from V5 because `verter_session` is reached there as a
dependency **library**, so its test targets are never built. It is a specific obligation about a
specific known reference — not a universal lane table, which this ruling deletes.

### 7.4 Performance obligation (charter-bound) — BLOCKED ON AN EXTERNAL ACT

The charter binds Stage 2 to the existing locked cell `A6_META_COMPILE_40_COLD_RUST` — the
`[[cell]]` record carrying that `id` in `performance-gates.toml`, invoked by the charter's
**Cold/warm/allocation/fan-out/latency bounds** paragraph — not a new one.

> **EXTERNAL DEPENDENCY — NOT DISPOSITIONED HERE.** The cell pins its harness
> (`crates/verter_bench/examples/attribution_baseline.rs`) by **two** identities in one
> `corpus_fingerprint` string — a Git blob and a SHA-256 — and those two halves **no longer name the
> same bytes**:
>
> | | Git blob | SHA-256 of the named bytes |
> |---|---|---|
> | the lock's `corpus_fingerprint` | `efa9ea54a14772ecd87511d6bb07017aa33940ba` | `1d208e61…8ea9bb73` |
> | the tree's harness file | `efa9ea54a14772ecd87511d6bb07017aa33940ba` | `5e06d35d…e806f632` |
>
> **The mismatch is a HYBRID, and the earlier form of this table described it wrongly.** The blob
> half was re-pinned upstream to the current blob and the SHA-256 half was left at the value of the
> superseded blob `a74f90c5d1d06f8fc17a71781d28d0c6ea466853`, whose contents do hash to
> `1d208e61…8ea9bb73`. So the lock's blob half now **matches** the tree while its SHA-256 half does
> not, and the two halves contradict each other **inside the lock**. The harness content diverged
> first, at `2d7339ef4d docs(*): tighten comments across the workspace`; the partial re-pin followed.
>
> **A run whose harness identity does not match this cell on BOTH halves is not this cell**, and no
> reading of the current cell can be satisfied, because no bytes satisfy both halves at once.
> Correcting the pin — restoring the blob, or authorising a recalibration/re-pin that makes the two
> halves name one set of bytes again — is an act of the maintainer / A6 lock authority and is
> **outside C1**. This ruling does not re-pin it, does not work around it, and records no
> justification for the divergence. **The external-dependency disposition is unchanged by the
> correction above**: until that act lands, §7.4 is blocked.

**This plan INVOKES the cell's protocol; it does not restate it.** The earlier form reconstructed the
measurement as a C1-specific command list, and that reconstruction was not the cell: it left the
sample count unbound, built only the attribution arm, and omitted the disabled timing/RSS arm, the
minimum sample count, the alternating invocation order, the start/end control and drift check, the
output oracle, the zero counters and most of the conjunctive metrics. **A partial restatement of a
locked cell is a different measurement wearing its name** — and every omission was invisible unless a
reader held `performance-gates.toml` open beside the plan, which is exactly the failure mode a locked
cell exists to prevent.

The obligation is therefore stated once, by reference:

| # | Obligation |
|---|---|
| V12 | Execute the `A6_META_COMPILE_40_COLD_RUST` protocol **as its owner defines it**, unmodified, on `FINAL_SHA` and on `${BASE}`, with the harness blob equal to the owner-authorised `$A6_BLOB` in force when the external act lands. The harness-identity assertion precedes any build; a differing blob is not this cell and aborts before measurement. |
| V13 | Record the protocol's own receipt, in full, including every arm, counter, oracle and control it defines — not a C1 summary of it. Raw output is written **outside the repository working tree**, to an evidence root that is not part of `FINAL_TREE`, so measurement cannot dirty the checkout the receipt is about. |
| V14 | The exit criterion is the cell's own, applied conjunctively as the cell defines: no metric it names may be omitted, reweighted or reinterpreted after measurement (S2-F6). |
| V15 | §2's **Performance contribution**. |

**Two consequences are stated rather than left implicit.**

- **The protocol must be callable.** If `A6_META_COMPILE_40_COLD_RUST` has no runner that a caller can
  invoke — if it exists only as a description that each consumer re-implements — then V12 cannot be
  discharged as written, and no amount of care inside C1 fixes that. Making it callable belongs to
  the maintainer / A6 lock authority together with the pin correction above, and this plan records it
  as a dependency rather than quietly reconstructing the protocol again.
- **Raw evidence never lands in the tree it measures.** The earlier form redirected stdout into
  the absent ledger-local destination `docs/arch/architecture-lock/ledger/C1/evidence/`, creating an
  untracked file inside the very checkout other receipts assert is clean. Writing measurement output
  into the subject of the measurement is self-defeating regardless of whether a `.gitignore` entry hides it.

**Machine prerequisites** remain as the cell requires: a quiescent host — no other cargo, gate or
benchmark process running; the memory ceiling free per the resource-ceiling policy; power and thermal
governor unchanged between baseline and candidate runs. A run that cannot assert quiescence is
discarded, not reported.

### 7.5 Atomic-history obligations — RECORDED UNMET, and the landing-hygiene rule survives as a rule

The four obligations below have no plan-owned instrument (§10). They are recorded, not discharged:

| Obligation | Status |
|---|---|
| Exactly one commit lands on the landing basis (S2-F3) | **UNMET** as a plan-owned receipt. |
| Linear fast-forward, one parent, and that parent is the landing basis | **UNMET** as a plan-owned receipt. |
| Edge, guard, code and deletions land in one commit, with a deletion not satisfiable by a modification (S2-F2, S2-F3) | **UNMET** as a plan-owned receipt. |
| Landing-message hygiene (§9) | **UNMET** as a plan-owned receipt. |

The landing parent is the trunk commit onto which the squash lands. It is not the working-branch
head that contains the squash.

**Why a Git-reading check was not retained even though Git decides these propositions.** The
propositions are decidable, but the checks read revisions the caller names. Nothing bound
`--base` to the authorized baseline, so the family could only ever have established *relative* facts
about an arbitrary one-commit checkout — true statements about the wrong subject. Re-sourcing an
expected value from the Git object at a caller-supplied revision does not fix this; it relocates the
trust root without closing it. That was the defect in the last proposal to keep this family, and it
is why the family goes rather than shrinks.

### 7.6 How a claim acquires a receipt

**There is no claim-to-receipt table in this ruling, and its absence is deliberate.** The previous
form was a hand-written map from claim to instrument. A text table can assert that an instrument
discharges a claim the instrument never established, and nothing in the mechanism prevents it —
which is precisely how a `--lib` selection came to be recorded as proving a guard that lives in an
integration target, and how a dependency-closure test came to be recorded as proving STAY ownership
and a MIRROR projection it cannot see.

**A claim is discharged only by a witness that established it, and the witness names its own
claims.** Concretely:

- **No digest-bound verifier is among this plan's witnesses (§10).** §1.5's and §7.5's obligations
  therefore have no emitting witness at all and are recorded unmet in the tables there. What this
  plan does hold emits its result directly: the canonical gate and the repository tests it selects
  either pass or fail, and there is no separate place to write down what they proved.
- A check that does not run, errors, or returns an indeterminate state emits **no** claim. Absence of
  a claim is visible in the receipt as an unmet obligation; it cannot be supplied from elsewhere.
- A claim with no emitting witness is **unmet**, and this plan says so rather than mapping it to the
  nearest plausible instrument.

What follows are the final-tree obligations owned here. All have no witness in this plan and are
recorded as unmet rather than mapped to the nearest instrument.

- **§3 STAY rows.** A dependency-closure test proves no edge crosses upward; it does not prove that a
  named definition still lives in the crate that is supposed to own it, nor that a duplicate neutral
  definition was not left behind. Discharging this needs a structural assertion about ownership —
  a repository test that names the owning module and fails if the definition moves or is duplicated.
  Until such a test exists in the repository, the STAY rows are an **intent record, not a proven
  claim**, and this ruling does not assert otherwise.
- **§3 MIRROR row.** The mirror needs a test that names both sides and asserts the projection; until
  it exists the row is likewise **intent, not proof**.
- **§3 MOVE roster.** Compilation proves the moved definitions exist *somewhere* the callers can
  reach; it does not prove each named definition landed in the crate §3 assigns it to, and no cited
  test asserts per-definition destination. A definition moved to the wrong owning crate compiles and
  passes every gate named here. Discharging it needs a structural ownership assertion — the same
  instrument §3's STAY rows need. Until one exists, the MOVE rows are **intent, not proof**.
- **§3 RETARGET roster.** Its only witness is the `raw_resolver_entry_points_are_private` compile-fail
  fixture, which the canonical gate does not select (§7.2), so nothing in this plan executes it.
  Until it is selected by something, the RETARGET rows are **intent, not proof**.
- **§3's DELETE roster IN FULL, and the no-forwarder obligation.** On the boundary §7.2 draws, the
  part compilation cannot carry is the whole of what this roster asserts: a repository compile-fail
  fixture exists for the eight resolver-authority public paths but is inert until the transition
  activates it, and even activated it establishes only that **those named public paths do not
  resolve** — never that a private copy, a renamed copy, or a copy in another module is gone. The
  roster is **intent, not proof**, in full.
- **Source absence for every MOVE definition, and the no-copy half of STAY (§3).** No sufficient
  instrument exists: destination reachability is not relocation while a source copy satisfies it, a
  path-named fixture sees only the paths it names, and the §7.2 dependency-closure guard cannot see a
  duplicate. Both halves are **unmet**, and no approximation of them appears anywhere in this plan.

- **S2-R3 (§2): intent, not proof.**
- **S2-R5 and S2-F4 (§2, §6): intent, not proof.**

- **Registration of the relocated C1-AC-3 witness.** §7.2 names
  `project_semantic_dispatch_invariants` in its post-transition home. Selection requires that the
  relocated suite be registered by its new parent. Nothing establishes that registration. **UNMET.**
- **Registration of the §5 `resolution_driver_tests` module.** The planned module must be declared
  by its parent to enter the lib test target. Nothing establishes that registration. **UNMET.**
- **That the §7.1 gates and the §7.3 command were run and passed.** This plan records what they are
  required to establish, and nothing observes that any of them was executed or what it returned.
  **UNMET** — the same gap as the final-tree binding row below, and the reason that row bounds every
  §7 result.
- **RETARGET (§3): UNMET.**

**Further obligations are recorded here rather than left implicit**, since an obligation nobody
restates after its instrument is removed is exactly the work that reaches trunk unowned:

- **Authorization-scope exactness** — that the scope carries precisely the required key set, with no
  missing, duplicated or invented field. **UNMET.**
- **Record and ruling binding** — that the Dispatch Identity Record and this ruling hash to the
  digests the scope records, and that the authorization names the ruling's registered document row.
  **UNMET.**
- **Clean-checkout state at dispatch** — that the tree carries no uncommitted change at the moment
  Stage 2 begins. **UNMET.**
- **Binding final evidence to the actual `FINAL` checkout** — that the results recorded in §7 were
  produced on the candidate they are attributed to, rather than on some other tree. **UNMET**, and it
  is the obligation that makes every other §7 result meaningful, so its absence bounds them all.
- **The §5 per-row F24 receipt and execution claim** — that each acceptance row was actually invoked
  and produced the recorded outcome. **UNMET.**
- **Authorization document closure (§1.6 SPEC 2): UNMET.**
- **Dispatch Identity Record content (§1.3): UNMET.**
- **Clean, unchanged working tree throughout final-evidence execution** — distinct from binding the
  results to `FINAL_SHA`/`FINAL_TREE`. A commit identity says nothing about uncommitted tracked
  changes present while the commands ran, so a result can bind the right commit and still have been
  produced against a modified tree. **UNMET.**
- **Single C1 authorization (§1.6 SPEC 2): UNMET.**

Recording these as unmet is the point of the change. The earlier map made them look discharged, and
a reader could not have told the difference without re-deriving what each instrument actually
selects.

### 7.7 The charter's three review mandates — RECORDED UNMET

The charter's Review cell binds this block to three mandates — **conformance**, **architecture**, and
**adversarial performance/memory** — each carried in an independent context, all against one
candidate SHA and tree (the charter's **Review** cell and its three-row mandate table).

**This plan INVOKES the cell; it does not restate it.** The scope of each mandate is the charter's
and is not reproduced here, for the reason §7.4 gives about the locked performance cell: a partial
restatement is a different review wearing the mandate's name. The one-candidate-SHA-and-tree
requirement is likewise the charter's, not an obligation this ruling originates.

| Obligation | Status |
|---|---|
| The three mandates were executed, in three independent contexts, against one candidate SHA and tree | **UNMET.** Nothing in this plan observes that any of the three ran, or what it returned. |

## 8. Abort procedure and return-to-known-good

**This section carries a recoverability claim, not a completeness claim.** It does not assert what
has been found or what exists; it asserts that intermediate work stays unlanded, and it prescribes
the procedure for returning to `c1-stage2-prestart`. **That recovery is CONDITIONAL on the return
point.** See §1.5. Where its obligations do not hold, the procedure below has nothing correct to
return to.

**Abort procedure.** Stop editing; do not commit; do not push; `git reset --hard c1-stage2-prestart`
and discard the working tree. Nothing landed, so nothing is reverted and no trunk state is touched.
Record the trigger and the evidence before any retry.

**Then delete the prestart tag, as the last step of abort.** Retry follows §1. Abort therefore ends
with `git tag -d c1-stage2-prestart`; a tag that already exists at retry means the previous abort did
not complete, which is a state worth failing on rather than overwriting.

**States that force abort:**

| # | Trigger |
|---|---|
| AB-1 | §1.1. |
| AB-2 | §2–§4. |
| AB-3 | §6. |
| AB-4 | §5. |
| AB-5 | A caller cannot be repointed without a forwarding wrapper, alias, feature flag, or dual path. |
| AB-6 | §2; §4. |
| AB-7 | The charter's own Abort/rescope triggers fire: a fourth production lifecycle; `ProjectResolver` proving not cleanly separable from scheduler-integrated loading; a second query-time resolution path; an unexplained `A6_META_COMPILE_40_COLD_RUST` regression. These reopen the ruling — a second architecture challenge, never a quiet local substitution. |
| AB-8 | §7. |

**What abort costs.** A missed reference is a compile error in a scratch tree: rework, not damage.
That is why completeness does not have to be established before execution, and why no static
artifact in this ruling is asked to carry it.

---

## 9. One landing

The irreversible boundary is the single squashed landing commit, not the editing. Its message
describes the change on its own terms and carries no program, revision or block vocabulary in either
subject or body — the branch's accumulated `wip` types and block-identifier subjects are resolved by
that squash, and the audit covers bodies, not only subjects.

---

## 10. Deleted from the normative plan — rather than repaired

Six constructions are removed. Each was repaired repeatedly and each failure was the same shape: a
static artifact — or a script — asserting a universe its procedure did not establish.

0. **Both plan-owned verifier scripts**, and with them every `D`- and `V`-numbered assertion they
   implemented. See §1.5, §6.4, §7.5, §7.6 and §12.

1. **The S1–S7 sweep apparatus, its counts and density tables, and the rule that any inventory
   difference invalidates ratification.** That rule was incoherent once the aid was admitted
   non-exhaustive: it made every change to a deliberately partial planning aid revoke authority.
   Searches may remain as non-binding planning aids or execution evidence.
2. **The lane-selection table and its lower-bound proof — including the claim that its truth is
   independent of the codebase.** That claim is false: package membership, target declarations,
   required features, dependency roles, host-built build scripts and proc macros, and Cargo and
   toolchain semantics are all repository-dependent, so the described selection can itself drift.
   And reading an invocation cannot prove it was run. §7 replaces it with executions.
3. **The illustrative residual list as a plan component.** See §4 and §7.3.
4. **The two-document override structure.** One normative artifact. The plan and annex remain
   historical evidence with no normative force.
5. **The instruction to delete the entire differential harness.** See §6.

**The standard these failed, stated so it is reusable.** The annex's "what an artifact FOUND versus
what an artifact CLAIMS EXISTS" distinction is **necessary but not sufficient**. A positive finding
is carryable only when its **input tree, procedure, tool identity, execution and result are all
bound**. Lexical results meet that standard when re-run against a specified tree — which is why they
survive as evidence and not as authority. The lane table never met it, because it established that
nothing had compiled.

---

## 11. The nine required components — a crosswalk, not a second record

**This table is navigation: a required component, and the section that OWNS it. It restates nothing
of what those sections say, and states no status, count, identity or disposition of its own.** Read
each owner for its own content; read §1.5, §6.4, §7.4, §7.5, §7.6 and §7.7 for status.

| # | Component | Owning section |
|---|---|---|
| 1 | Stage-owned intent contract | §2 |
| 2 | Exact preflight identity | §1 |
| 3 | Move / stay / mirror / delete / retarget ledger | §3 |
| 4 | Atomic Cargo-edge, guard, caller and deletion transition | §4 |
| 5 | Real-driver acceptance matrix | §5 |
| 6 | Compiler / reference fixpoint | §4, §7.3 |
| 7 | Final-candidate evidence manifest | §7 |
| 8 | Abort procedure and single landing | §8, §9 |
| 9 | Digest-bound ruling and authorization update | §0, §1.4, §1.6 |

## 12. Sufficient shape — an index, not a summary

**This section indexes what the plan is made of. It records no status of its own** — see §11 for the
component crosswalk and its status pointers.

1. §0, §1.
2. §2, §3, §5.
3. §4.
4. §3, §6.
5. §7.
6. §9.

The verifier deletion is §10's. Two principles came out of it, and they bound what this plan may ever
claim:

- **A free-form harness cannot be made claim-conserving.** Associating arbitrary boolean checks with
  handwritten claim strings is the widening mechanism itself — the same defect as the claim-to-receipt
  table this document already deleted, one layer down. Digest-binding proves which bytes ran, never
  that they establish their proposition.
- **A clean review record is not evidence of soundness.** It is evidence only about the adversarial
  inputs that happened to be tried. Retention must follow from proving the object a check reads
  DECIDES the proposition it emits. **This retires per-check history as a retention argument** — and
  it applies to checks that never visibly failed, which is precisely where the argument is tempting.

---

## 13. The inherited record — predecessor findings, and scope complaints against this plan

**What this section claims.** It records two facts about the record **surrounding** this plan, each
re-derived from the governing document itself rather than from a summary of that document. **It
states no obligation status of its own and moves none**, and nothing below alters §0's activation
order, §8's abort or §9's landing.

### 13.1 The charter-ratifiability findings — four of five are no longer live

`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-C1-CHARTER-RATIFIABILITY.md` carries five P1 findings. Re-derived from
that ruling's own verdicts and receipt, against the tree at `BASE`:

| Finding | State | Why |
|---|---|---|
| C1-01 | **CLOSED** | Its Q1 remedy — one controlling sentence incorporating the three-gaps addendum and specifying that its dispositions supersede conflicting charter text — is present in `docs/arch/refactor/rev11/charters/C1.md`'s preamble as the sentence beginning *"`ARCH-ADDENDUM-C1-THREE-GAPS.md` is equally binding on this charter"*, naming all three gap dispositions. |
| C1-02 | **LIVE** | *"Stage 2 began without the separately ratified irreversible-cutover plan its sequencing record requires."* **This ruling is that plan. Ratifying it does not by itself close the finding (§1.5).** |
| C1-03 | **DEAD** | A defect **of the branch-local authority registry** — unsupported TOML escapes rendering it inert. That branch-local delta does not exist here. |
| C1-04 | **DEAD** | Likewise branch-local: a pinned C1 charter digest not matching the charter bytes. |
| C1-05 | **DEAD** | Likewise: `CM1` recorded as `REVIEW`, falsifying the branch authorization predicate. CM1 is `ACCEPTED` (`docs/arch/architecture-lock/ledger/program-state.toml`, CM1 row). |

That ruling's **Q3** has **two clauses, and only the first has happened** — the paragraph opening
*"Q3: A. Drop the registry edits entirely"* under that ruling's **The ruling** heading, cited by its
own text because a line number into a file this block does not own is not an identity:

| Q3 clause | State |
|---|---|
| Drop the branch-local registry edits; a block branch may only inherit the trunk-owned registry byte-for-byte through rebase | **DISCHARGED** — by the same fact that kills C1-03, C1-04 and C1-05: this branch carries no `docs/arch/architecture-lock/ledger/` delta against `BASE` at all. |
| *"Authorization must be authored trunk-side by the registry/ledger owner after the governing documents and prerequisites are valid."* | **OPEN.** This is a positive act, not the absence of a defect, and no absence of a branch delta can perform it. The C1 `[[authorization]]` record's `scope` string in `docs/arch/architecture-lock/ledger/authority-registry.toml` says so in its own words — *"Execution authority requires a further record."* — and `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-C1-CHARTER-RATIFICATION.md`, the ratification of record for the charter, states that both Q2 and Q3 *"remain open against C1 and are not touched here."* |

**Q3 is therefore NOT satisfied, and an earlier form of this subsection asserted that it was.** That
error is this document's, not a reviewer's: it read the death of three branch-local defects as
discharge of the whole question, when the clause that remains is the one §0's activation order exists
to serve.

**Two readings to get right.** The branch-local artifact quoted by C1-02 in
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-C1-CHARTER-RATIFIABILITY.md` is absent
here and is not live evidence.
The live carrier is the C1 `[[authorization]]` record's `scope` string in
`docs/arch/architecture-lock/ledger/authority-registry.toml`; see §1, §1.5, §4 and §8. C1-05's death
closes a finding **about the branch predicate**; it is not a statement about §0 step 5, whose gate
this section leaves exactly where §0 puts it.

**The registry's own summary of that ruling STANDS, and an earlier form of this subsection wrongly
called it stale.** The C1 `[[authorization]]` `scope` string reads
*"RULING-2026-08-24-C1-CHARTER-RATIFIABILITY leaves two findings open against it."* It counts the two
**questions** the ratification of record leaves open — Q2, the missing separately ratified execution
plan, and Q3's trunk-side authorization clause — not the five numbered receipt findings the table
above disposes of. Those are two different enumerations over the same ruling, and reading one count
against the other is what produced the staleness claim. Both are re-derived here from the governing
documents rather than from either gloss.

### 13.2 A scope complaint against this plan is not a conflict with the addendum

A ratification review raised a P1 asserting that this plan omits large parts of the charter and of
`docs/arch/refactor/rev11/rulings/ARCH-ADDENDUM-C1-THREE-GAPS.md`: the GAP-1/GAP-2/GAP-3 cut sets, `AttemptOutcome` coverage over
every C2-reachable non-flow operation, the `LoadSet` kernel contract, and C1-AC-6/7/8 among others.
That finding was measured against the **charter entire**. The live Stage-2 scope carrier is identified
in §13.1, with the addendum governing **wherever it conflicts**.

A conflict check across the addendum entire against this plan entire finds **none**.

**So that finding is a COVERAGE complaint, not a CONFLICT one, and the coverage it asks for lies
outside Stage 2's ratified scope.** This section records that disposition and deliberately supplies
none of the coverage requested: writing GAP-1/2/3 subject matter into a Stage-2 plan would widen
ratified scope, which is not this document's act to perform.

---

## 14. Recorded open findings — this ruling ratifies WITH these, not despite them

**Why this section exists.** A review seat raised the findings below and they are not fixed here.
That is a decision, not an oversight, and it follows the admissibility rule this document already
states: **a proof gap may ratify as a recorded unmet obligation; an internal contradiction or a false
command claim may not.** The two contradictions a seat found — §7's final-tree rule against V12's
baseline arm, and the location-key ban against the required baseline-remote key — were inadmissible
and are corrected. Everything below is admissible, so it is recorded and carried rather than repaired.

**When DISCLOSED is available, and when it is not.** Disclosure is what makes a false claim
admissible: the register states the wording is known-wrong and that this ruling does not rely on it,
so the document no longer asserts it — it disclaims it. **That is available only where repairing the
wording is out of scope BY RULING. It is never a general alternative to fixing**, and a finding that
could be repaired inside the current scope must be repaired rather than disclaimed.

**What recording does, and does not do.** A finding listed here is DISCLOSED: this ruling does not
rely on the wording the finding disputes, and a reader who reaches that wording is told here that it
is contested or wrong. Recording does not repair it, does not discharge the obligation behind it, and
does not license anyone to treat the disputed wording as sound.

**Why they are not being repaired.** Nine repair passes over this document produced a flat finding
count: each pass removed defects and introduced others in interlocking normative prose, most visibly
when a deletion remedy stripped a value-carrying field, a table's identifying column, and the clause
a status depended on. Repair stopped converging, so it stopped being the instrument. The block's
deliverable is the transition, not this document.

| # | Finding | Kind | Where |
|---|---|---|---|
| OF-1 | Mutable propositions are still asserted outside a single owning section. An inventory pass found 83, all were addressed, and a later seat found more. The invariant is sound and the residue is real. | proof gap | throughout |
| OF-2 | S2-R1 and S2-R2 route their status to §7.6, which records neither. **The pointer is known-wrong; §7.6 does not own that status and nothing here establishes it.** | false pointer, DISCLOSED | §2, §7.6 |
| OF-3 | The single C1 `[[authorization]]` row records UNMET without stating the distinction the status rests on — that the validator establishes no second record may exist and that one must exist past LOCKED, but not whether the surviving record was created-if-absent and extended in place. **The status stays UNMET; the row does not currently say why it is UNMET despite that coverage.** This row has now been contested three times in three different ways, which is evidence the proposition is mis-framed rather than mis-decided, and it is carried as its own question. | proof gap + mis-framing | §7.6 |
| OF-4 | §13.2 classifies GAP-2 subject matter as outside this plan's scope. That is too wide: GAP-2's item-6 and item-7 dispositions and its `ConfiguredMembership` split ARE matched by §3's rosters and are in scope. **The over-wide sentence is known-wrong.** | false claim, DISCLOSED | §13.2 |
| OF-5 | F24-5b names variants that are not the ones the cited type declares. | false claim, DISCLOSED | §5 |
| OF-6 | The §11 crosswalk names §7.3 as an owner of the compiler/reference fixpoint, which §7.3 does not record. | false pointer, DISCLOSED | §11 |
| OF-8 | The bytes registered differ from the bytes a seat reviewed by more than the `**Status:**` line: after the final review a contradiction it found — step 4 authoring the authorization before the blocking inventory prerequisite closes — was corrected, and the correction is in the registered bytes. **The two-step audit property this document states for itself is therefore not satisfied for this revision.** It is disclosed rather than repaired because a further review round is closed by ruling. | disclosed deviation | §0 |
| OF-7 | S2-R1 asserts no `verter_workspace` dependency "normal or dev", while V1 checks normal and build kinds only. The dev half has no witness. | proof gap | §2, §7.2 |

**None of these is a licence.** Every one remains a defect. What this section decides is only that they
do not block ratification, because none of them makes this document assert both sides of a question or
claim that a command establishes something it does not.
