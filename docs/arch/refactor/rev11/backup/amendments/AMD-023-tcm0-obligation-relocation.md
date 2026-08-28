# AMD-023 — two obligations move to their real owners, and take effect when they bind

**Status:** DRAFT — awaiting ratification. Ratified status and the ratifying seat's identity are
recorded in §8 by the authority that ratifies it.

**Prepared against:** `block/tcm0-acceptance` at `e53c529ae`, whose merge-base with
`program/architecture-lock` is that branch's base. **It is prepared against the branch, not the
working branch, deliberately:** the register and gate this amendment edits and calls in §5-§6 live on
that branch and do not exist on trunk. An earlier revision was prepared against trunk and prescribed
a verification step that could not run there — a verification that cannot execute on the tree it
names is not a verification.

**Scope sentence.** This amendment NOMINATES two obligations for relocation from TCM0 to named
existing owners — the string-encoded-surface enumeration to **TCM1**, and the binding of
deletion-checklist items 17-18 to **TCM1, TCM2 and TCM3 (recording) and TCM4 (verifying)** — and makes
the relocation TAKE EFFECT only when the binding act lands. **It creates no block, retires no block,
changes no DAG edge, alters no numbered TCM0 Scope item, reduces no acceptance criterion, edits no
charter and re-pins no digest.**

## 0. Nomination and effect are different acts, and this one performs only the first

The distinction is the whole design, and an earlier revision blurred it. **Naming an owner does not
bind one.** An obligation "assigned by name but not bound" is the defect the explicit-destination
requirement exists to prevent, and writing the destination into an amendment does not by itself avoid
it — the receiving charter has to say the words.

So this amendment does two separable things:

1. **It nominates**, with evidence, and it records exactly how much of each obligation each receiving
   charter ALREADY covers — as DERIVED in §3, not as anybody's impression. Both obligations turn out
   to be mostly covered already: of seven parts, four are covered, one is excluded by the receiving
   charter in its own words, and two are not. An earlier revision of this line said obligation B was
   covered in NONE of its parts, which the derivation contradicts — it is covered in two of three.
2. **It sets the condition on which the relocation takes effect**: the added or replaced criteria in
   §4, ratified and digest-re-pinned by their owner. **Until that act lands, neither obligation has
   moved**, and TCM0's register must continue to show them as not closed. A reader must not take this
   amendment's ratification for the relocation itself.

## 1. Content anchors, and how to falsify them

Recorded as **sha256**, because that is what the registry pins. An earlier revision recorded
`git hash-object` output and told a verifier to compare it to a registry row — a 40-character SHA-1
object id against a 64-character SHA-256 digest, a comparison that cannot be performed. It also
recorded only three charters while §7 asked a verifier to check five.

| document | sha256 | equals its registry row |
|---|---|---|
| `charters/TCM0.md` | `2ea41dd85befd978e06364d952eb3b262c9b6edba1f1ac8ce37eba9845b91e97` | yes |
| `charters/TCM1.md` | `2886c796307ac8b28e3288de5062a207a3262f9f78fa407ecf31637e90cc4a28` | yes |
| `charters/TCM2.md` | `3cae6cef57c87b1eba9a1d6143adba11ab390eb838783de3daa19dee353b1af2` | yes |
| `charters/TCM3.md` | `78efb323bf669b81e235ecef7225e33bb622a83be1470c36ee898c52f981e752` | yes |
| `charters/TCM4.md` | `a9f1b3e71d6fe7890b982971615d55a5e69be67146bbfed102367d1aea1c4575` | yes |
| `program-dag.toml` | `057a9f1f60c8e81fee0ac10d1710f32b66d77b5e47f5dbaaf45ab75defa233a4` | not registry-pinned; recorded here as the identity check §7 step 3 uses |

The "equals its registry row" column was computed, not assumed: all five file digests were compared
against `authority-registry.toml`'s `*-CHARTER` rows and matched.

**Falsification test.** `shasum -a 256` each path. A changed TCM0 digest means the Scope items may no
longer be the ten this amendment leaves untouched. A changed TCM1-TCM4 digest means §3's derivation
must be re-run before this is applied, because the coverage it reports is a statement about charter
text that has since moved.

## 2. What is being relocated, and why TCM0 cannot discharge it

Both obligations are inside TCM0's acceptance bar and neither is reachable by any number of further
rounds, because both are blocked on AUTHORITY rather than effort.

**A — `G-STRING-SURFACE-CITATIONS`.** Its closure bar is to introduce a value newtype over the encoded
map, retype the map-carrying fields, and let the compiler produce the complete producer list. That is
production code, and `charters/TCM0.md`'s Non-scope reads "No production code. No routing change."

**B — deletion-checklist items 17 and 18.** Scope 9 requires naming every mechanism the deletion block
removes, "Not deferred to TCM4". Items 17-18 name categories whose MEMBERS cannot exist yet. The
mechanism TCM0 proposes binds nobody: making it bind requires an added exit criterion in three
ratified, digest-pinned charters, and the re-pinning act belongs to the registry owner.

## 3. Receiving-owner coverage — DERIVED, not written

**Three drafts of this section were written by hand and all three were wrong, each in a new way.** One
claimed a criterion bound a whole obligation when its own first sentence scopes it to part. One said a
receiving block bound none of an obligation when it binds the verifying half. One proposed adding a
criterion the receiving charter **already has**. That last is the decisive failure: a hand-written
table cannot notice a criterion its writer did not happen to read, and no amount of care fixes that —
which is why this section no longer contains a table.

`probes/receiving-coverage-derivation.mjs` reads every numbered exit criterion in TCM1, TCM2, TCM3 and
TCM4 and reports, per part of each obligation, which criterion covers it. The result is committed at
`evidence/TCM0/receiving-coverage.md` and is a build output: running that script with `--check`
re-derives from the charters and exits non-zero on any drift.

**As derived (43 criteria read across four charters): 4 parts covered, 1 excluded by the receiving
charter, 2 uncovered.** The two uncovered parts are the entire content of §4.

**Matching is paragraph-scoped, and that is not a detail.** A field is not an identity unless read with
the record it belongs to, and conjoining literals across a criterion running to dozens of lines has the
same defect: two literals can both appear while belonging to different sentences about different
things. That is exactly how the first run of this derivation reported the inbound-field part as COVERED
by two criteria that were both naming it in order to put it OUT of scope. Exclusion is now modelled
explicitly, and a part matches only when its literals co-occur inside ONE paragraph.

**What the derivation proves and what it does not.** It proves every criterion in every receiving
charter was read — the failure that produced all three wrong tables. It does not prove the quoted
criterion binds the part; textual matching cannot. So each row carries the criterion's own opening
words and a reader is expected to read them. The failure is narrowed from "did anyone look?" to "is
this specific sentence the right one?", and only the first was ever the problem here.

## 3a. What the derivation CANNOT do, stated because it will be mistaken for more than it is

**The seven obligation parts are hand-authored.** They live in the script's `PARTS` table, written by
reading each gate's closure bar — which is prose. No derivation produces them, because there is no
machine-readable statement of what an obligation's parts ARE; the closure bars are sentences in an
evidence document. **So the script cannot detect an omitted eighth part.** It derives which criteria
cover the parts it was given; it does not derive the parts.

That is a real limit and it is not closed by this amendment. It is disclosed rather than engineered
around, because the alternative — generating a part list from some proxy and calling it derived — would
produce exactly the false authority this section exists to avoid. **A derivation that is wrong is worse
than a table that is wrong, because it looks authoritative.**

**What would falsify the partition:** read each gate's closure bar in
`evidence/TCM0/successor-block-scope.md` and `evidence/TCM0/OPEN-GAPS.md` against the `PARTS` table and
name a part that is missing. That is a human check, it is the check this amendment depends on, and a
ratifying authority should perform it rather than assume the script did.

**Matching is textual, and one residue survives by construction.** A criterion that binds a part in
different words than the part's literals is a MISS the script cannot see — rewording "exactly one codec
ships" as "only the selected serializer ships" preserves the obligation and loses both literals. The
two guards that WERE closable are closed and their controls are recorded in
`evidence/TCM0/receiving-coverage-controls.md`: an exempting sentence no longer produces a covering hit,
and a criterion that excludes a different field in a neighbouring sentence no longer reads as excluding
this one. The reword case remains open and is why every row quotes the criterion's own words.

## 4. The binding act — named, not performed

Belongs to the program orchestrator and the maintainer. **Two criteria, exactly the two parts the §3
derivation reports as uncovered** — not the four an earlier hand-written version proposed, two of which
duplicated criteria that already exist:

1. **TCM1** — one added exit criterion disposing of the `oxc_sourcemap` re-export (derived part `A4`),
   closing obligation A's residue. Criteria 1 and 3 are not replaced; they already cover the producer
   chain and the wire boundary.
2. **TCM1, TCM2, TCM3** — one added exit criterion each: record every DTO or API type the block
   introduces or orphans whose sole producer/consumer pair lies inside the deleted set (derived part
   `B1`), appended to the deletion manifest's item-17 list.

**Not needed, on the derivation's evidence:** an item-18 negative check for TCM2 — its exit criterion 1
already requires exactly one codec to ship, with a negative test. And nothing for TCM4 — its exit
criterion 5 already addresses items 17-18 directly. An earlier version of this section proposed both.

## 5. What changes on ratification of THIS amendment

1. `evidence/TCM0/closure-register.md` — rows `S9.c` and `S3.g` gain this amendment as the act that
   named their owners, and record that the relocation is PENDING the §4 act. **Neither row changes
   status**: `probes/closure-validator.mjs` requires a `NOT-OWNED` row to name a real owner, and an
   owner that has not accepted the obligation is not yet real. They may become `NOT-OWNED` only when §4
   lands. (`S3.g` is the row formerly filed as the non-obligation `X.a`; it was promoted to a counted
   obligation because the validator skips the non-obligation table, so filing it there removed it from
   the count rather than recording a judgement about it.)
2. `docs/arch/architecture-lock/ledger/authority-registry.toml` — a `[[document]]` row for this
   amendment. The registry is the program orchestrator's to write.

3. **`evidence/TCM0/deletion-closure.md`** and the two register rows themselves carry §3a's limit.
   The wording is adapted to each location rather than pasted, because a row and a manifest section
   address different readers; what must be identical is the SUBSTANCE — that the part list is
   hand-authored, that an omitted part is undetectable, and that the receiving owner must re-check the
   closure bar before acting. This is a binding condition on the relocation, not a courtesy: TCM1-TCM4 inherit these
   obligations, and a limit recorded only in this instrument is one nobody inherits — an unmet
   obligation with no owner, which is the failure the relocation exists to prevent. The manifest is the
   right home for obligation B because TCM4's charter adopts it and is forbidden from second-guessing
   it; the register rows are the right home for both, because they are what a reader consulting an
   obligation's status arrives at.

**Edits 1 and 3 are TCM0's and are ALREADY MADE on this branch**, which is why the allowlist below
carries their paths and why §7's steps 5b and 5c pass rather than describe an intention. **Edit 2 is the
registry owner's and is the ONLY ratification-time change outstanding** — which is precisely why step 1
fails here and will pass when it lands. Step 5c was watched failing before those row edits existed
(three assertions red on each row) and passing after: a check demonstrated in both directions. The block orchestrator writes
neither the registry nor any charter.

## 6. What this amendment does NOT do

- **It does not touch TCM0's ten Scope items**, and `charters/TCM0.md` is neither edited nor re-pinned.
- **It does not reduce or soften any acceptance criterion**, of TCM0 or of any receiving block.
- **It does not edit any receiving charter and re-pins no digest.** Those are §4's acts.
- **It does not change the DAG.**
- **It does not close `S9.c` or `S3.g`.** Both stay blocking until §4 lands, and `A.a` blocks on its
  own terms independently of this amendment.
- **It does not relocate the Acceptance clause's `A.a` row — deliberately, not by oversight.** A reader
  who sees two obligations nominated and this one retained will otherwise assume it was missed, so the
  reason is stated. `A.a` is the charter's PROHIBITION on accepting with a "semantic mechanism TBD". A
  prohibition is not work somebody performs; it is a TEST applied to TCM0's own evidence set at the
  moment TCM0 is accepted. Handing it to another owner would relocate the words and lose the thing they
  test — TCM1 cannot apply, on TCM1's evidence, a prohibition about whether TCM0's evidence contains an
  unresolved mechanism. It stays with TCM0 for as long as TCM0 has an acceptance, and is not
  relocatable by this or any instrument. Its DISPOSITION follows from whether any live TBD remains once
  A and B have moved; this amendment takes no position on that.
- **It does not accept TCM0**, and it is not evidence toward TCM0's acceptance.

## 7. Verification on ratification — a script, not a description

**A verification step that cannot fail is not a verification step**, and a step written as prose is not
a step at all. Earlier revisions of this section contained: an assertion about a row the gate skips by
construction, so no run could produce it; a name-only diff with no range, which emits nothing and exits
0; and a comparison between a 40-character object id and a 64-character digest, which cannot be
performed. All three read correctly.

**A path allowlist alone is also insufficient**: it bounds WHICH files may change and says nothing
about what changes inside them. An earlier revision claimed every allowed file therefore carried its own
content assertion. **That claim was false** — three of the five had none, so an unrelated edit inside an
allowed file passed every check. Two are added below (steps 4b and 5b) and the residue is named rather
than papered over: a SYNCHRONIZED edit to both the derivation script and its generated output still
passes step 4, and an unrelated edit elsewhere in the registry still passes step 2. Those two are
uncovered, and a verifier is told so here instead of discovering it.

Run this. It exits non-zero on any failure and prints which.

```sh
set -u; fail=0
BASE=8659a14dd4fb9e96baf6d1b180e3080469a7af1a                      # the branch this amendment is based on, NOT the working branch
check() { if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: $2 != $3"; fail=1; fi; }

# 1. path allowlist, over an explicit range. The range is required: the bare form selects nothing
#    and exits 0.
got=$(git diff --name-only "$BASE"..HEAD | sort | tr '\n' ' ')
# BOTH sides are sorted. An earlier version sorted only the observed side and wrote the expected side
# by hand in a different order, so the comparison could never succeed — a step with no PASSING mode,
# which is the same defect as a step with no failing mode and just as useless.
want=$(printf '%s\n' \
  docs/arch/architecture-lock/ledger/authority-registry.toml \
  docs/arch/refactor/rev11/amendments/AMD-023-tcm0-obligation-relocation.md \
  docs/arch/refactor/rev11/evidence/TCM0/closure-register.md \
  docs/arch/refactor/rev11/evidence/TCM0/probes/receiving-coverage-controls.sh \
  docs/arch/refactor/rev11/evidence/TCM0/probes/receiving-coverage-derivation.mjs \
  docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md \
  docs/arch/refactor/rev11/evidence/TCM0/receiving-coverage-controls.md \
  docs/arch/refactor/rev11/evidence/TCM0/receiving-coverage.md | sort | tr '\n' ' ')
check "allowlist" "$(echo $got)" "$(echo $want)"

# 2. the five charters are byte-identical, compared as sha256 against §1 AND the registry.
for c in TCM0 TCM1 TCM2 TCM3 TCM4; do
  f=docs/arch/refactor/rev11/charters/$c.md
  d=$(shasum -a 256 "$f" | cut -d' ' -f1)
  r=$(grep -A3 "id = \"$c-CHARTER\"" docs/arch/architecture-lock/ledger/authority-registry.toml \
      | grep sha256 | grep -o '[0-9a-f]\{64\}')
  check "charter $c unchanged" "$d" "$r"
done

# 3. the dependency graph, by identity — never by an empty diff, which exits 0 either way.
check "dag unchanged" "$(shasum -a 256 docs/arch/refactor/rev11/program-dag.toml | cut -d' ' -f1)" "057a9f1f60c8e81fee0ac10d1710f32b66d77b5e47f5dbaaf45ab75defa233a4"

# 4. the derived coverage still matches the charters it was derived from.
node docs/arch/refactor/rev11/evidence/TCM0/probes/receiving-coverage-derivation.mjs --check >/dev/null \
  && echo "ok   coverage derivation" || { echo "FAIL coverage derivation drifted"; fail=1; }

# 5. the gate STILL REFUSES, and names the three rows. A gate that started passing on the strength of
#    this amendment would be the bypass: neither obligation has moved.
out=$(node docs/arch/refactor/rev11/evidence/TCM0/probes/closure-validator.mjs 2>&1); rc=$?
[ "$rc" -eq 1 ] && echo "ok   gate refuses" || { echo "FAIL gate exited $rc, expected 1"; fail=1; }
for row in S9.c S3.g A.a; do
  echo "$out" | grep -q -- "- $row " && echo "ok   gate names $row" || { echo "FAIL $row not refused"; fail=1; }
done

# 4b. the recorded negative controls regenerate identically, and the regenerator leaves the charters
#     it plants into byte-identical. A control transcript nobody can reproduce is a claim, not evidence.
before=$(shasum -a 256 docs/arch/refactor/rev11/evidence/TCM0/receiving-coverage-controls.md | cut -d' ' -f1)
bash docs/arch/refactor/rev11/evidence/TCM0/probes/receiving-coverage-controls.sh >/dev/null 2>&1
crc=$?     # the script exits 2 when it cannot restore a charter it planted into; discarding this
           # status let a failed run still report two `ok`s, which is a check certifying its own miss
check "controls script exited clean" "$crc" "0"
after=$(shasum -a 256 docs/arch/refactor/rev11/evidence/TCM0/receiving-coverage-controls.md | cut -d' ' -f1)
check "controls reproduce" "$before" "$after"
# Hash equality alone cannot tell "regenerated identically" from "never written at all": a failed write
# leaves the old file in place and the two hashes match. So assert the transcript is STRUCTURALLY whole
# as well. The script itself refuses to publish an incomplete one, and this is the caller-side check.
blocks=$(grep -c 'planted into TCM1' docs/arch/refactor/rev11/evidence/TCM0/receiving-coverage-controls.md)
check "controls transcript complete" "$blocks" "3"
check "controls left charters clean" "$(git status --porcelain docs/arch/refactor/rev11/charters/ | wc -l | tr -d ' ')" "0"

# 5b. the closure register's row INVENTORY is unchanged. Step 5 checks which rows are refused; this
#     checks that no row was quietly added, removed or renamed inside an allowed file.
ids=$(grep -oE '^\| (S[0-9]+\.[a-z]|INV\.[a-z]|A\.[a-z]|X\.[a-z]) ' \
        docs/arch/refactor/rev11/evidence/TCM0/closure-register.md | tr -d '| ' | sort | tr '\n' ' ')
check "register row inventory" "$(echo $ids)" "A.a A.b A.c A.d A.e INV.a INV.b S1.a S1.b S1.c S1.d S1.e S1.f S1.g S1.h S1.i S1.j S1.k S10.a S2.a S2.b S2.c S2.d S2.e S2.f S2.g S2.h S2.i S2.j S2.k S3.a S3.b S3.c S3.d S3.e S3.f S3.g S4.a S4.b S5.a S5.b S5.c S6.a S6.b S7.a S7.b S8.a S8.b S9.a S9.b S9.c X.b X.c"

# 5c. the two relocated rows carry what §5 requires: this amendment named, the move marked PENDING,
#     and the inherited limit present. Inventory says the rows exist; this says they say something.
reg=docs/arch/refactor/rev11/evidence/TCM0/closure-register.md
for row in S9.c S3.g; do
  seg=$(awk -v r="$row" 'index($0,"| " r " "){f=1} f{print} f&&/^$/{c++; if(c>=3) exit}' "$reg")
  echo "$seg" | grep -q "AMD-023" && echo "ok   $row cites the amendment" || { echo "FAIL $row does not cite AMD-023"; fail=1; }
  echo "$seg" | grep -q "PENDING" && echo "ok   $row marked pending" || { echo "FAIL $row not marked PENDING"; fail=1; }
  echo "$seg" | grep -q "Inherited limit" && echo "ok   $row carries the limit" || { echo "FAIL $row lacks the inherited limit"; fail=1; }
done

exit $fail
```

**Executed on the tree this amendment was prepared against**, which is the only reason any of the
above is worth reading. Steps 2, 3, 4 and 5 pass: five charters digest-matched against the registry,
the graph unchanged, the derivation matching the charters, and the gate exiting 1 while naming `S9.c`,
`S3.g` and `A.a`. Step 1 fails for one reason only: its allowlist describes the RATIFICATION commit,
whose registry row is written by the registry owner and does not exist on this branch. **The branch
shows seven of the eight allowlisted paths, and the registry is the single absent one.** An earlier
revision of this paragraph said three of five and named the closure register among the absent — both
stale after the allowlist grew, and both wrong against a run anybody could have made.

**An earlier revision of this paragraph claimed that failure PROVED step 1 discriminates, and that was
wrong.** Step 1 could not pass at all: it sorted the observed paths and compared them to an expected
list written by hand in a different order. It failed on the correct tree for the wrong reason, and a
verifier watching it stay red through ratification would have learned nothing. Both sides are sorted
now, and the check was run BOTH ways — red with the two ratification paths absent, green with them
present — because a failure alone never shows a check works. It shows it can fail, which a check that
is simply broken also does.

**Step 6 is not scriptable and is stated as such.** Locate records asserting the relocation as COMPLETE,
then READ each hit's sentence: a token search cannot separate an assertion from a correction quoting
it, so the reading is the step and the search only produces candidates.

**Demonstrated failing modes**, because a check nobody has watched fail is worth nothing. Step 1: the
range omitted selects nothing and passes — observed. Step 2: mutating a charter changes its digest —
observed while red-testing the derivation. Step 4: observed failing four ways — the committed file
edited by hand, the criteria heading renamed, a covering criterion reworded so its part reports
NOTHING, and the exclusion wording changed so the excluded part reports covered. Step 5: observed
failing three ways — an informal status token, a renamed proof artifact, a deleted scope row.

## 8. Ratification

*To be completed by the ratifying authority.*
