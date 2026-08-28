# Post-binding charter drift — bounded review

The five charters in the AMD-009 package (`BA0.md`, `BCSS0.md`, `BF3.md`, `BRT0.md`, `BS0.md`)
changed by **+51 / −36 lines** after the package was bound to worktree commit
`9e457ca781d3684e562d6eaea24c401e2d9849a7`, and never received a later recorded acceptance.
The maintainer's [§7 ratification ruling](maintainer-ruling-section7-ratification.md) directs
that this drift be reviewed before the package is rebound.

Two independent external review seats read the exact drift
(`git diff 9e457ca78 -- docs/arch/refactor/rev11/charters/`) against AMD-009 §1/§2/§7, the
ratified [`dispositions.md`](dispositions.md), and the tests as they stand in the tree. Neither
seat authored any of the reviewed text. The adversarial seat ran with an explicit
default-to-BLOCK posture. Prompts required a per-charter verdict with cited evidence; an
uncited answer was BLOCKING by default.

- **Conformance/architecture seat:** Codex `gpt-5.6-sol`, reasoning effort `high`, read-only.
- **Adversarial seat:** Grok 4.6, reasoning effort Extra High, read-only, default-to-BLOCK.

## Outcome

**Both seats returned `BLOCKING`, and both converged on the same root cause: AUTHORITY, not
content.**

What both seats independently VERIFIED as correct:

- No drifted charter re-classes, renames, invents or reopens a finding row, owner or acceptance
  ID relative to `dispositions.md` as it stands.
- Every test the drifted text names exists in the tree, with the stated `#[ignore]` status and
  assertions — including SV-4's `an_untyped_svelte_props_destructure_publishes_its_authored_props_to_typescript`,
  CSS-1's `the_standalone_css_route_publishes_valid_requested_maps_for_passthrough_and_transformed_css`,
  and BND-2's `the_bundler_rollup_inline_transform_preserves_requested_source_maps`.
- `BA0.md` and `BF3.md` changed only their status line.

What both seats found BLOCKING:

1. **The status line claimed ratification for bytes the binding never covered** (both seats, P1).
   AMD-009 §7 requires fresh reviewed identities and explicit acceptance for any changed byte;
   none existed after `9e457ca78`.
2. **`BRT0.md`'s BND rewrite was substantive** (both seats, P1). The bound charter carried
   BND-1/BND-2 as `AWAITING CONFIRMATION` with a confirmation-before-ID gate; the drift rejects
   BND-1, splits BND-2, and assigns `BF3-BND-2-SOURCEMAP-PARITY`. The adversarial seat additionally
   found that AMD-009 §5 still described the bound state, so the package was internally
   inconsistent with its own charter.
3. **`BS0.md` and `BCSS0.md` changed a future obligation into an existing one** (both seats, P2) —
   from "add this target and prove it RED" to "this target exists and is RED". Both match the
   tree; both were unaccepted post-binding edits.
4. **`BRT0.md` called the Rollup target discriminating when it was not** (Codex, P2) — it asserted
   the probe's derived boolean rather than the map artifact.

## How each finding was answered

| finding | disposition |
|---|---|
| Status line overstates authority (P1, both) | CURED BY REBIND. The package is rebound to its exact current content and the maintainer's §7 ratification is recorded against that identity in [`amd009-ratification-packet.md`](amd009-ratification-packet.md). Each charter's status line now cites that identity, so the ratified bytes are unambiguous. |
| `BRT0.md` BND rewrite (P1, both) | KEPT ON THE EVIDENCE, RECONCILED. The re-measurement is executed evidence — both public pinned Vite entries and both public Rollup entries were driven — and `dispositions.md` carries the settled rows. Reverting would make the charter less true than the tree. AMD-009 §5's BRT0 bullet was corrected to follow that table, and `BRT0.md`'s procedure and re-measurement section now state explicitly that the rows post-date the originally bound table and that `dispositions.md` is their authority. |
| `BS0.md` / `BCSS0.md` obligation change (P2, both) | KEPT AND REBOUND. Both match the tree; the reverse would restore text the tree falsifies. `BS0.md`'s SV-4 row and exit count were additionally corrected in this cure, because the bound text said SV-4 had no ignored target while the tree has one. |
| Rollup target non-discriminating (P2, Codex) | FIXED IN CODE. The acceptance assertions now validate the source-map ARTIFACT (`version: 3`, non-empty `mappings`, non-empty `sources`), not the probe's derived boolean, and the change was proven RED in both directions. |

Findings the seats recorded outside the drift were not actioned; they are listed in each report
below and in [`landing-record.md`](landing-record.md).

---

## Verbatim report — conformance/architecture seat (Codex `gpt-5.6-sol`, high)

```
VERDICT: BLOCKING

PER-CHARTER  
BA0:   BLOCKING — §7 creates BA0 only at the bound identity (AMD-009:232-244); no re-class versus dispositions:28-29; named AT tests exist with stated ignore status/assertions (framework_product_surface_tests.rs:731-758,1421-1468; svelte_batch_route_tests.rs:651-710); body is otherwise consistent; line 3 overstates current-byte authority.  
BS0:   BLOCKING — Neither §1 nor §7 authorizes post-binding lines 31/35-47; SV-1…SV-4 still match dispositions:23-26; all four ignored targets and characterizations exist with stated assertions (svelte_official_conformance_gate.rs:725-969; public_api_typescript_observation.rs:441-536); revised procedure is internally consistent; line 3 overstates authority.  
BCSS0: BLOCKING — §7 only creates BCSS0 and its changed-byte rule bars this unaccepted procedural update (AMD-009:232-244); CSS-1 still matches dispositions:30; the ignored target checks authored maps on both branches and currently fails first on passthrough because both production returns use `source_map: None` (framework_product_surface_tests.rs:1231-1276; verter_compiler/src/css/mod.rs:107-110,143-145); body is otherwise consistent; line 3 overstates authority.  
BF3:   BLOCKING — §7 ratifies BF3 only at the bound identity (AMD-009:232-244); the drift changes no finding row; six client and six server cells are asserted by unignored tests against recorded 5.56.8 cells (svelte_official_conformance_gate.rs:263-276,515-518; svelte_conformance_cell_record.json:6-415); ratified-charter/not-accepted-block is otherwise consistent; line 3 represents an unaccepted post-binding file as ratified.  
BRT0:  BLOCKING — §7 merely creates BRT0 and §1 forbids re-classing rows (AMD-009:35-37,232-244); bound BND-1/BND-2 were provisional with pending IDs (dispositions.md@9e457ca78:50-58; BRT0.md@9e457ca78:15-24), but drift rejects/removes BND-1 and creates a narrower BND-2 DEFER/ID at lines 15-24; RT-1, TR-1 and Vite tests exist as described, but the ignored Rollup target trusts `publicTransformHasMap` and is recorded as non-discriminating against `map:null` (transport_route_equivalence_tests.rs:1175-1230,1362-1500; landing-record.md:313-316); “ratified item” procedure is inconsistent with post-ratification BND-2; lines 3/24 overstate authority.

FINDINGS (each: id, severity P1/P2/P3, file:line, what is wrong, what would fix it)  
1. DRIFT-AUTHORITY, P1, docs/arch/refactor/rev11/charters/{BA0,BS0,BCSS0,BF3,BRT0}.md:3, all five post-binding files say `RATIFIED` although AMD-009:240-244 requires fresh reviewed identities and explicit acceptance for every changed byte; rebind/review/accept the exact current package, or restore the bound bytes and record status non-normatively.  
2. BRT-RECLASS, P1, docs/arch/refactor/rev11/charters/BRT0.md:15, drift removes BND-1 and confirms/narrows/assigns an ID to BND-2 despite the bound `AWAITING CONFIRMATION` state; restore both provisional rows and confirmation gate, or obtain confirmation plus fresh exact-package ratification.  
3. BRT-FALSE-DISCRIMINATOR, P2, docs/arch/refactor/rev11/charters/BRT0.md:20, the edited table calls the Rollup target discriminating, but forcing its probe boolean true makes it pass while the real map remains null (landing-record.md:313-316); independently validate the returned map and prove that mutation RED before rebinding.  
4. BRT-INTERNAL, P2, docs/arch/refactor/rev11/charters/BRT0.md:28, the procedure applies to “ratified item[s]” while BND-2 explicitly post-dates the ratified table, and the drift deletes its confirmation procedure; restore provisional adjudication language or ratify the new row and gate.  
5. TEST-SYNC-AUTHORITY, P2, docs/arch/refactor/rev11/charters/BS0.md:31, BS0 changes SV-4 from a future assertion obligation/three ignored exits to an existing fourth target; BCSS0.md:21 similarly changes target creation into target execution. Both match the tree but substantively alter procedure after binding; freshly review/accept them or restore bound wording.

QUESTIONS 6 AND 7  
6. Substantive changes: BS0 changes SV-4 procedure and exit count; BCSS0 changes target-creation and RED obligations; BRT0 removes BND-1 adjudication, splits BND-2, assigns a new ID, and deletes confirmation. BA0/BF3 change only status text, but that text remains governance-bearing and unauthorized.

7. Yes. BRT0 smuggles scope not ratified by full §7: the bound package carried BND-1/BND-2 provisionally, while drift rejects BND-1 and creates a confirmed Rollup-only BND-2 correction/ID without later confirmation. BS0/BCSS0 add no finding or owner, but their procedures remain outside the accepted identity.

OUT-OF-DRIFT OBSERVATIONS (recorded, not actioned)  
- The product ruling says it ratifies only AMD-009 §1/§2 (maintainer-product-ruling-no-error-on-bad-output.md:27-36), while later records claim full §7; landing-record.md:265-289 records this dispute. This review applied the brief’s full-§7 premise.  
- BF3’s pre-existing “every public/default route” exit is unproven for `Verter`, `unpluginFactory`, and `default` (landing-record.md:306-312).  
- BA0’s pre-existing “no product at all” target checks runtime refusal and IDE absence, not PublicApi or every publication (framework_product_surface_tests.rs:1423-1468).  
- Silent-zero registration, cold timeout, JS runtime-version, and partial-coverage defects remain recorded at landing-record.md:317-333.
tokens used
172 585
I completed the review, but could not create `/tmp/drift-review-codex.md`: the read-only sandbox rejected both permitted `apply_patch` attempts. The completed report follows.

VERDICT: BLOCKING

PER-CHARTER  
BA0:   BLOCKING — §7 creates BA0 only at the bound identity (AMD-009:232-244); no re-class versus dispositions:28-29; named AT tests exist with stated ignore status/assertions (framework_product_surface_tests.rs:731-758,1421-1468; svelte_batch_route_tests.rs:651-710); body is otherwise consistent; line 3 overstates current-byte authority.  
BS0:   BLOCKING — Neither §1 nor §7 authorizes post-binding lines 31/35-47; SV-1…SV-4 still match dispositions:23-26; all four ignored targets and characterizations exist with stated assertions (svelte_official_conformance_gate.rs:725-969; public_api_typescript_observation.rs:441-536); revised procedure is internally consistent; line 3 overstates authority.  
BCSS0: BLOCKING — §7 only creates BCSS0 and its changed-byte rule bars this unaccepted procedural update (AMD-009:232-244); CSS-1 still matches dispositions:30; the ignored target checks authored maps on both branches and currently fails first on passthrough because both production returns use `source_map: None` (framework_product_surface_tests.rs:1231-1276; verter_compiler/src/css/mod.rs:107-110,143-145); body is otherwise consistent; line 3 overstates authority.  
BF3:   BLOCKING — §7 ratifies BF3 only at the bound identity (AMD-009:232-244); the drift changes no finding row; six client and six server cells are asserted by unignored tests against recorded 5.56.8 cells (svelte_official_conformance_gate.rs:263-276,515-518; svelte_conformance_cell_record.json:6-415); ratified-charter/not-accepted-block is otherwise consistent; line 3 represents an unaccepted post-binding file as ratified.  
BRT0:  BLOCKING — §7 merely creates BRT0 and §1 forbids re-classing rows (AMD-009:35-37,232-244); bound BND-1/BND-2 were provisional with pending IDs (dispositions.md@9e457ca78:50-58; BRT0.md@9e457ca78:15-24), but drift rejects/removes BND-1 and creates a narrower BND-2 DEFER/ID at lines 15-24; RT-1, TR-1 and Vite tests exist as described, but the ignored Rollup target trusts `publicTransformHasMap` and is recorded as non-discriminating against `map:null` (transport_route_equivalence_tests.rs:1175-1230,1362-1500; landing-record.md:313-316); “ratified item” procedure is inconsistent with post-ratification BND-2; lines 3/24 overstate authority.

FINDINGS (each: id, severity P1/P2/P3, file:line, what is wrong, what would fix it)  
1. DRIFT-AUTHORITY, P1, docs/arch/refactor/rev11/charters/{BA0,BS0,BCSS0,BF3,BRT0}.md:3, all five post-binding files say `RATIFIED` although AMD-009:240-244 requires fresh reviewed identities and explicit acceptance for every changed byte; rebind/review/accept the exact current package, or restore the bound bytes and record status non-normatively.  
2. BRT-RECLASS, P1, docs/arch/refactor/rev11/charters/BRT0.md:15, drift removes BND-1 and confirms/narrows/assigns an ID to BND-2 despite the bound `AWAITING CONFIRMATION` state; restore both provisional rows and confirmation gate, or obtain confirmation plus fresh exact-package ratification.  
3. BRT-FALSE-DISCRIMINATOR, P2, docs/arch/refactor/rev11/charters/BRT0.md:20, the edited table calls the Rollup target discriminating, but forcing its probe boolean true makes it pass while the real map remains null (landing-record.md:313-316); independently validate the returned map and prove that mutation RED before rebinding.  
4. BRT-INTERNAL, P2, docs/arch/refactor/rev11/charters/BRT0.md:28, the procedure applies to “ratified item[s]” while BND-2 explicitly post-dates the ratified table, and the drift deletes its confirmation procedure; restore provisional adjudication language or ratify the new row and gate.  
5. TEST-SYNC-AUTHORITY, P2, docs/arch/refactor/rev11/charters/BS0.md:31, BS0 changes SV-4 from a future assertion obligation/three ignored exits to an existing fourth target; BCSS0.md:21 similarly changes target creation into target execution. Both match the tree but substantively alter procedure after binding; freshly review/accept them or restore bound wording.

QUESTIONS 6 AND 7  
6. Substantive changes: BS0 changes SV-4 procedure and exit count; BCSS0 changes target-creation and RED obligations; BRT0 removes BND-1 adjudication, splits BND-2, assigns a new ID, and deletes confirmation. BA0/BF3 change only status text, but that text remains governance-bearing and unauthorized.

7. Yes. BRT0 smuggles scope not ratified by full §7: the bound package carried BND-1/BND-2 provisionally, while drift rejects BND-1 and creates a confirmed Rollup-only BND-2 correction/ID without later confirmation. BS0/BCSS0 add no finding or owner, but their procedures remain outside the accepted identity.

OUT-OF-DRIFT OBSERVATIONS (recorded, not actioned)  
- The product ruling says it ratifies only AMD-009 §1/§2 (maintainer-product-ruling-no-error-on-bad-output.md:27-36), while later records claim full §7; landing-record.md:265-289 records this dispute. This review applied the brief’s full-§7 premise.  
- BF3’s pre-existing “every public/default route” exit is unproven for `Verter`, `unpluginFactory`, and `default` (landing-record.md:306-312).  
- BA0’s pre-existing “no product at all” target checks runtime refusal and IDE absence, not PublicApi or every publication (framework_product_surface_tests.rs:1423-1468).  
- Silent-zero registration, cold timeout, JS runtime-version, and partial-coverage defects remain recorded at landing-record.md:317-333.
```


---

## Verbatim report — adversarial seat (Grok 4.6, Extra High, default-to-BLOCK)

```
VERDICT: BLOCKING

PER-CHARTER
BA0:   PASS — status-only (`UNRATIFIED` → `RATIFIED (AMD-009); not accepted`); authorized by AMD-009 §7 L232-236 create-BA0 + §8 L260-261 not-accepted; body bytes otherwise identical to `9e457ca78`; no finding/test rewrite.
BS0:   BLOCKING — post-binding SV-4/procedure/exits rewrite is not in §7; stamps `RATIFIED (AMD-009)` on non-verbatim text (§5 L190-193 ratified verbatim). Working-tree test name is tree-true (`public_api_typescript_observation.rs:505` `#[ignore]`d).
BCSS0: BLOCKING — post-binding target-presence + procedure rewrite is not in §7; stamps `RATIFIED` on non-verbatim text. Named test exists and is `#[ignore]`d (`framework_product_surface_tests.rs:1232-1233`); both CSS arms still `source_map: None` (`css/mod.rs:110,145`).
BF3:   PASS — status-only (`UNRATIFIED until AMD-009 is ratified` → `RATIFIED (AMD-009)` + not-accepted); authorized by §7 L232-233 + §8 L260; body otherwise identical to `9e457ca78`.
BRT0:  BLOCKING — substantive BND re-scope after binding; contradicts still-live AMD-009 §5 L203-204 (`AWAITING CONFIRMATION`); assigns `BF3-BND-2-SOURCEMAP-PARITY` and drops the confirmation gate the verbatim charter required (`BRT0.md` at `9e457ca78` L15-16, L32-34).

FINDINGS (each: id, severity P1/P2/P3, file:line, what is wrong, what would fix it)
1. P1 `docs/arch/refactor/rev11/charters/BRT0.md:15-55` — Drift rewrites owned scope, objective, table, procedure, and exits: BND-1 removed; BND-2 promoted from `pending — AWAITING CONFIRMATION` / `the_bundler_route_matches_the_in_process_host_route` to owned Rollup/non-Vite correction under new ID `BF3-BND-2-SOURCEMAP-PARITY` → `FC-ROUTES-001`; confirmation-before-ID gate deleted; new “Re-measured exclusions and split” made charter law. No §7 clause authorizes this. §5 L203-204 still says BRT0 “provisionally carries BND-1/BND-2 as `AWAITING CONFIRMATION`, exactly as dispositioned.” Binding BRT0 L15-16 forbade promote/rename/reclassify without confirmation. Fix: revert BRT0 body to `9e457ca78`, or cut a new reviewed package identity and obtain an explicit maintainer act that supersedes §5 and rebinds the charter.
2. P1 `docs/arch/refactor/rev11/charters/{BRT0,BS0,BCSS0}.md:3` — Each stamps `RATIFIED (AMD-009); not accepted` on text that is not the §5 “ratified verbatim” package. §7 L243-244: any changed byte needs fresh reviewed identities and explicit acceptance; none is recorded after `9e457ca78`. Fix: do not claim AMD-009 ratification for drifted bytes; revert or rebind.
3. P2 `docs/arch/refactor/rev11/charters/BCSS0.md:21-32` — Binding table/procedure required BCSS0 to *add* a then-absent correct-behavior target and prove it RED (`9e457ca78` BCSS0 L21, L24-26). Drift asserts the target already exists and is already RED, so the add-and-prove-RED step is gone. Unauthorized changed byte. Tree-true: `the_standalone_css_route_publishes_valid_requested_maps_for_passthrough_and_transformed_css` exists, `#[ignore]`d (`framework_product_surface_tests.rs:1231-1276`), unwraps `source_map` on both arms; green characterization `the_standalone_css_spelling_publishes_css_and_ignores_its_source_map_axis` (`:1178`, not ignored) still asserts `source_map.is_none()` with `sourcemap: true`. Matches current `dispositions.md` CSS-1 (no new finding id). Fix: rebind the named target into BCSS0, or revert and live with a stale “absent” sentence.
4. P2 `docs/arch/refactor/rev11/charters/BS0.md:31-46` — Binding SV-4 row said there is no ignored target; procedure demanded a positive checker assertion; exits required “the three ignored” targets (`9e457ca78` BS0 L31, L33-35, L47-48). Drift names `an_untyped_svelte_props_destructure_publishes_its_authored_props_to_typescript` as `#[ignore]`d acceptance target and requires all four ignored targets enabled+green. Unauthorized. Tree-true: test exists `#[ignore]`d (`public_api_typescript_observation.rs:503-537`) and asserts members `disabled`,`label` with `label` required / `disabled` optional; characterization `:442` (not ignored) asserts empty members. Matches current `dispositions.md` SV-4 (same id/owner). Fix: rebind, or revert and restore the tree-false “no ignored target” lie.
5. P2 `docs/arch/refactor/rev11/charters/BRT0.md:21-24` vs AMD-009 §1 L35-37 — Edit does not invent ids beyond *current* `dispositions.md` BND-2 (`BF3-BND-2-SOURCEMAP-PARITY`, owner BRT0, `the_bundler_rollup_inline_transform_preserves_requested_source_maps` `#[ignore]`d at `transport_route_equivalence_tests.rs:1417-1418`). That dispositions re-class *post-dates* the ratified table (`dispositions.md` L5-8, L53-92; binding dispositions had BND-1/BND-2 `AWAITING CONFIRMATION` / pending). Charter absorbed the post-dated re-class as if §1 allowed it. Fix: keep BRT0 at binding text until a new amendment/acceptance updates both §5 and the ratified table together.

QUESTIONS 6 AND 7
6. Substantive (changes what a future block must do or own):
   - BRT0: drops “adjudicate the two provisional bundler rows”; owns Rollup/non-Vite BND-2 correction; excludes BND-1 and Vite BND-2; assigns a non-pending acceptance ID; deletes the confirmation gate; adds a Rollup-map required exit. That is new owned work, not clerical.
   - BCSS0: implementer no longer must add-and-prove-RED the public-boundary target; starts at the shared CSS correction against an already-named ignored test. Same finding CSS-1 / `FC-OPTIONS-001`, different first procedure step.
   - BS0: SV-4 acceptance gate is now a named ignored test that must be enabled+green (four, not three). Same finding SV-4 / `FC-TS-001`, different exit inventory.
   Clerical only: BA0 and BF3 status lines (and the status half of the other three).
7. Yes. A maintainer ratifying “the full §7” at `9e457ca78` ratified verbatim BRT0 carrying BND-1/BND-2 as `AWAITING CONFIRMATION` and forbidding reclass without confirmation; BCSS0 with an *absent* correct-behavior target BCSS0 must add; BS0 with *no* SV-4 ignored target and three ignored exits; AMD-009 §5 in those words. They were not ratifying BND-1 rejected, BND-2 as a BRT0 Rollup-map correction under `BF3-BND-2-SOURCEMAP-PARITY`, or the later-added ignored CSS/SV-4 targets as already-owned RED gates. Status `RATIFIED (AMD-009)` on those rewritten bodies smuggles the new obligations under the old act. §7 L243-244 required a fresh identity + explicit acceptance; none exists.

OUT-OF-DRIFT OBSERVATIONS (recorded, not actioned)
- Maintainer ruling `evidence/BF3/maintainer-product-ruling-no-error-on-bad-output.md:27-32` ratifies AMD-009 §1 and §2 only; AMD-009 §8 (itself added after `9e457ca78`) claims that ruling carried the full §7 accept line. Not used as a charter-body finding here; it is the already-recorded governance blocker (`landing-record.md:265-289`).
- `dispositions.md` itself drifted after binding (SV-4/CSS-1 gating tests; BND-1 REJECTED; BND-2 DEFER + new id). Charter Q2 vs the *current* file is a match; the re-class landed in dispositions first.
- `the_bundler_rollup_inline_transform_preserves_requested_source_maps` asserts probe boolean `publicTransformHasMap` (`transport_route_equivalence_tests.rs:1494-1500`), derived in `probe-bundler-route.mjs:291` from `transformed?.map !== null`; it does not independently observe `publicTransformMap`. `landing-record.md:313-316` already recorded the lying-boolean green. Exists, `#[ignore]`d; “discriminating” is not proven from the assertion shape.
- Unchanged BA0 AT-2 still says a separately named ignored correct-behavior target is “currently absent” (`BA0.md:25`); not in this diff.
- Committed HEAD `BS0.md` still has the tree-false “no ignored target” SV-4 sentence; the SV-4 fix is working-tree only.
```
