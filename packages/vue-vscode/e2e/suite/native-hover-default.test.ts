import { expect } from "chai";
import * as vscode from "vscode";
import {
  findPosition,
  getAppVuePath,
  hoverText,
  launchServerProfile,
  measureHover,
  openAndReady,
  waitForHoverMatching,
  openReadyCached,
  FIXTURE_NAME,
} from "../helpers";

/**
 * What an editor that wires NOTHING gets.
 *
 * `hover.test.ts` and `generic-attrs.test.ts` moved onto the
 * `verter-native-semantics` profile, because the affordances they assert are a
 * documented opt-in. That move would otherwise leave nothing asserting the
 * DEFAULT — and "Verter's native hover lane is off unless you ask for it" is a
 * shipped product decision (`verter.hover.nativeSemantics`, default `false`,
 * "Disabled by default so the selected TypeScript provider exclusively owns the
 * hover hot path"), not an accident.
 *
 * These cases fail in BOTH directions:
 *
 * - If the default silently starts contributing native hovers, the negative
 *   assertions here fail.
 * - If the opt-in stops contributing them, the positive assertions in
 *   `hover.test.ts` fail.
 *
 * Neither default can move unobserved, and moving one deliberately means moving
 * both in the same change.
 *
 * Every case asserts an ABSENCE **and** a presence. An absence alone is
 * satisfied by a server that answers nothing at all — which is exactly how a
 * disabled lane makes a whole suite pass while measuring nothing.
 */
suite(`Native hover default configuration [${FIXTURE_NAME}]`, function () {
  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    this.timeout(60_000);
    // A case asserting an absence must prove it is running the intended
    // configuration, or a leaked opt-in from an earlier suite makes it pass for
    // the wrong reason. Refuse to execute on anything but the default.
    const launched = launchServerProfile();
    if (launched !== "default") {
      // Parity fixtures deliberately launch opted in; they do not load this
      // suite, so reaching here means the route inventory and the suite map
      // disagree and the pin would be measuring the wrong server.
      throw new Error(
        `${FIXTURE_NAME} launched on the "${launched}" server profile, so it cannot pin what the ` +
          "DEFAULT configuration does; this suite must only load on default-profile routes",
      );
    }
    doc = await openReadyCached(getAppVuePath());
  });

  setup(function () {
    // Re-checked per case, not once: a launch serves ONE profile, so a case that
    // asserts an absence must be able to say which server produced it.
    const active = launchServerProfile();
    if (active !== "default") {
      throw new Error(
        `the server is configured as "${active}", not "default"; this pin would measure the ` +
          "opt-in lane it exists to exclude",
      );
    }
  });

  test("native @click hover is NOT contributed; the provider answers instead", async function () {
    const pos = findPosition(doc, '@click="increment"', 3);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    // Presence half: the default must still SERVE this position. Without this,
    // a server that answered nothing would satisfy every assertion below.
    expect(
      hovers.length,
      "the default configuration still answers @click hovers",
    ).to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "the default answer comes from the TypeScript provider").to.include("onClick");
    expect(
      content,
      "the source-owned Vue event label is an opt-in Verter contribution",
    ).to.not.include("@click");
  });

  test("event-modifier hover is NOT contributed by the default configuration", async function () {
    const pos = findPosition(doc, "@click.prevent=", "@click.".length);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(
      hovers.length,
      "the default configuration still answers at the modifier",
    ).to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    // The IDE codegen DELETES modifier syntax, so the provider describes the
    // event prop the modifier was attached to and knows nothing about `.prevent`.
    expect(content, "the default answer is the generated event prop").to.include("onClick");
    expect(content, "the modifier description is an opt-in Verter contribution").to.not.include(
      ".prevent",
    );
    expect(content, "no modifier semantics without the opt-in").to.not.include("preventDefault");
  });

  test("slot-outlet hover is NOT contributed by the default configuration", async function () {
    this.timeout(60_000);
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const myCompDoc = await openAndReady("src/MyComp.vue");
    const pos = findPosition(myCompDoc, '<slot name="header"', 1);
    if (!pos) {
      this.skip();
      return;
    }

    // Unlike the event-token hovers above, the native slot-outlet contribution
    // rides the ISOLATED BACKGROUND analysis lane and lands about 2.6s after the
    // document is otherwise ready. Asserting its absence with a single immediate
    // request proves nothing — the provider's answer arrives in ~36ms and the
    // negative holds on an opted-in server too, which is exactly how a pin passes
    // while measuring nothing. So WAIT for the contribution and require that it
    // never arrives: the predicate must still be unsatisfied when the budget runs
    // out, and the assertions below run on the last result seen.
    const hovers = await waitForHoverMatching(myCompDoc.uri, pos, {
      predicate: (candidates) =>
        candidates.length > 0 && /\bslot\b/i.test(hoverText(candidates[0])),
    });
    expect(
      hovers.length,
      "the default configuration still answers at the slot outlet",
    ).to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content.trim().length, "the default answer is non-empty").to.be.greaterThan(0);
    expect(content, "the slot-outlet description is an opt-in Verter contribution").to.not.match(
      /\bslot\b/i,
    );
  });
});
