# DOC5 evidence index

Selected case IDs: `DOC5-AC1`, `DOC5-AC2`, `DOC5-AC3`, `DOC5-AC4`, `DOC5-AC5`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node --test tests/documentation/DOC5/doc5.test.mjs`
- `node --test tests/documentation/DOC1/doc1.test.mjs`
- `node --test tests/documentation/DOC7/doc7.test.mjs`
- `node docs/scripts/reference-harness.mjs`
- `pnpm --filter docs check`

Deletion population this node: empty. The recipe home, the recipes and
journeys manifest sections, the recipes gate and the journey executor
are additive; the DOC0 executable-examples plan's deferral of runtime
example execution to DOC5 executable journeys is discharged by amending
that plan in place, so no second deferral record survives.

AC1: the DOC1 harness stays the sole owning interface. The recipe model
and the journey model are declared in `examples/reference/manifest.json`
(`recipes`, `journeys`) and validated only through `validate()`; the docs
package check invokes the same script unchanged. All eight topics are
pending with named producing nodes (CSS0, WBC0, DBG0, TST0, RTI0, AX0,
WPF0, SEC0) and the home declares a real `cli-version-smoke` NativeOnly
journey over the shipped CLI example.

AC2: a placeholder cannot satisfy a feature terminal's documentation
obligation. A supplied slot bound to a static sample without an
executable extension fails (`recipes-static-example`); a supplied
example without a capability surface fails
(`recipes-without-capability-link`); `--recipes-gate` rejects every
pending topic (`recipes-gate-unsatisfied`) and passes only when all
eight topics are supplied executably with capability links.

AC3: recipe pages and journeys join the source digest; fresh versus
incremental equality, edit/revert and perturbed-discovery canonical
equality hold. Journey execution is deterministic (journeys sorted by
id, steps in declared order, stdout digests not timings), bounded per
step (`journey-step-timeout`), cancellable with real child teardown
(state `cancelled`), and rejects unknown examples, unshipped or
outside-entry commands, class mismatches and duplicate ids. A missing
recipe page is a partial result.

AC4: `examples/reference/guides` is the DOC1-tested public recipe home;
pages record pending status truthfully and link shipped authority pages.
The journey executor produces runtime observation under `--run-journeys`
— proven by real NodeOnly execution over the committed fixture home —
while the declared NativeOnly journey is executed only on
artifact-present lanes and recorded `not-run` elsewhere. Pins,
host/profile, permissions, uncertainty and migration notes live in
`web-product-guides-model.v1.json`.

AC5: no new performance budget is claimed; journey execution is opt-in,
sequential and bounded at 60000 ms per step. Nothing required was
removed to shrink the patch; the deletion population is empty with the
rationale above.
