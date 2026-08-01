import { expect } from "chai";
import * as vscode from "vscode";
import {
  openReadyCached,
  findPosition,
  waitForCompletionsMatching,
  hoverText,
  waitForHoverMatching,
  FIXTURE_NAME,
} from "../helpers";

suite(`Generic & Attrs [${FIXTURE_NAME}]`, function () {
  let doc: vscode.TextDocument;

  // This suite runs on the `verter-native-semantics` server profile, declared in
  // `lib/serverProfiles.ts` and given its own launch by the runner. The `generic=`
  // / `attrs=` attribute-NAME hovers are SFC-syntax documentation Verter owns
  // natively; the generated TSX has no token to describe, so the default
  // provider-only configuration cannot answer them at all.
  let control: vscode.TextDocument;

  suiteSetup(async function () {
    if (FIXTURE_NAME !== "single-project") {
      this.skip();
      return;
    }
    this.timeout(120_000);
    doc = await openReadyCached("src/GenericAttrsComp.vue");
    // The plain-TypeScript oracle for the two delegation cases. It is NOT served by
    // the same engine as the carrier: Verter's language client selects framework
    // carriers only (`src/extension.ts`), so a plain `.ts` file is answered by VS
    // Code's own TypeScript while the carrier is answered by Verter's tsgo. That
    // makes this an INDEPENDENT oracle rather than a same-engine mirror — the two
    // agreeing is evidence about TypeScript's behaviour, not about one engine
    // agreeing with itself. It also means the two answers are formatted by
    // different quickinfo builders, so the cases below compare the facts they both
    // must state, never a byte-identical string.
    control = await openReadyCached("src/GenericAttrsControl.ts");
  });

  /**
   * The provider's answer for `needle`+`offset` in the plain-TS oracle.
   *
   * Waits for a non-empty result for the same reason the carrier probes do: the
   * control is opened moments earlier and an unwaited miss would read as "plain
   * TypeScript does not answer here" — which is exactly the claim under test.
   */
  async function controlHoverAt(needle: string, offset: number): Promise<string> {
    const pos = findPosition(control, needle, offset);
    expect(pos, `the oracle declares ${needle}`).to.exist;
    const hovers = await waitForHoverMatching(control.uri, pos!, {
      predicate: (candidates) => candidates.length > 0,
    });
    expect(
      hovers.length,
      `plain TypeScript answers at ${needle}+${offset}; if it does not, the carrier ` +
        "assertions above are measuring the oracle's silence rather than delegation",
    ).to.be.greaterThan(0);
    return hoverText(hovers[0]);
  }

  // ── Return Type Annotation ──────────────────────────────────────

  test("no ts(7010) implicit-any-return-type diagnostic", async function () {
    // ts(7010): Function which lacks return-type annotation implicitly has an 'any' return type.
    // The TemplateBindingFN should have `: any` return type to suppress this.
    const allDiags = vscode.languages.getDiagnostics(doc.uri);
    const ts7010 = allDiags.filter(
      (d) =>
        (typeof d.code === "number" && d.code === 7010) ||
        (typeof d.code === "object" && (d.code as { value: unknown }).value === 7010),
    );
    expect(ts7010, "Should have no ts(7010) implicit-any-return-type diagnostic").to.have.lengthOf(
      0,
    );
  });

  // ── Generic Attribute Value ─────────────────────────────────────

  test("hover inside generic attribute value delegates to TypeProvider", async function () {
    // The cursor sits on `T`, INSIDE the `generic="..."` value. The token choice is
    // the oracle's, not a preference: TypeScript returns no quickinfo at all for a
    // primitive keyword type node, so the `string` two words along answers nothing
    // in a plain `.ts` file either (proven by `GenericAttrsControl.ts` below).
    // Anchoring there would assert a hover TypeScript never produces and call the
    // resulting emptiness a Verter defect. `T` is a type parameter, which
    // TypeScript does describe, so it can tell delegation from silence.
    const pos = findPosition(doc, "T extends string", 0);
    expect(pos, 'the fixture declares generic="T extends string"').to.exist;

    // Wait for the property under test rather than taking the first non-empty
    // answer: a fast wrong answer (the SFC attribute documentation the native lane
    // contributes on the attribute NAME) would otherwise mask a slow right one.
    const hovers = await waitForHoverMatching(doc.uri, pos!, {
      predicate: (candidates) =>
        candidates.length > 0 && hoverText(candidates[0]).includes("(type parameter)"),
    });
    console.log(`    Hover inside generic value: ${hovers.length} result(s)`);

    expect(
      hovers.length,
      'a position inside generic="..." is mapped into the generated surface and must be answered',
    ).to.be.greaterThan(0);
    const content = hoverText(hovers[0]);
    console.log(`    Hover content: ${content.slice(0, 200)}`);

    // Presence is not enough — an answer from the wrong owner is also an answer.
    // This must be the TYPE PARAMETER as the generated function declares it, which
    // only the provider can know, and it must name the generated wrapper, which
    // only a real mapping into that surface can produce.
    expect(content, "the provider describes the type parameter").to.include("(type parameter) T");
    expect(content, "the answer comes from the generated surface").to.include("TemplateBindingFN");
    // The CONSTRAINT, not just the parameter: a surface generated as
    // `TemplateBindingFN<T>()` — dropping `extends string` — would still satisfy
    // both assertions above while having lost the thing the attribute declares.
    expect(content, "the declared constraint survives into the generated surface").to.include(
      "extends string",
    );

    // Parity with the same token in plain TypeScript: the shape must match, or the
    // carrier is answering something a `.ts` file would not.
    const controlHover = await controlHoverAt("T extends string", 0);
    expect(controlHover, "the plain-TS oracle answers at the same token").to.include(
      "(type parameter) T",
    );
    // Held to the same standard as the carrier, so the oracle cannot silently
    // become the weaker of the two claims.
    expect(controlHover, "the oracle's parameter is constrained too").to.include("extends string");
  });

  test("hover on generic attribute NAME shows SFC docs", async function () {
    // Cursor on the "generic" attribute NAME. Unlike the value, this token has no
    // correlate in the generated surface at all — it is SFC syntax — so only
    // Verter's native lane can describe it.
    const pos = findPosition(doc, 'generic="', 0);
    expect(pos, "the fixture declares a generic= attribute").to.exist;

    const hovers = await waitForHoverMatching(doc.uri, pos!, {
      predicate: (candidates) =>
        candidates.length > 0 && hoverText(candidates[0]).includes("Generic type parameters"),
    });
    console.log(`    Hover on generic attr name: ${hovers.length} result(s)`);

    expect(hovers.length, "the attribute name must be documented").to.be.greaterThan(0);
    const content = hoverText(hovers[0]);
    console.log(`    Hover content: ${content.slice(0, 200)}`);

    // Prose only the native lane emits (`sfc_attr_hover`). Merely containing the
    // word "generic" was satisfied by the provider's own answer at this position.
    expect(content, "names the attribute").to.include("**`generic`**");
    expect(content, "documents what the attribute does").to.include("Generic type parameters");
    expect(content, "documents the component-level scope").to.include("component-level generics");

    // The provider ALSO answers here, and the LSP MERGES the two into ONE hover.
    // Both halves are asserted, so losing either fails: the native lane going
    // silent fails the prose above, the provider half dropping out fails this.
    // Deliberately NOT pinning the separator the merge currently renders — the
    // contract is that the two are combined, and the rule between them is the
    // formatter's choice, so pinning it would fail on a cosmetic change while
    // catching nothing these two assertions miss.
    expect(content, "the provider's answer is kept").to.include("TemplateBindingFN");
  });

  // ── Attrs Attribute Value ───────────────────────────────────────

  test("hover inside attrs attribute value delegates to TypeProvider", async function () {
    // On `class`, INSIDE the `attrs="..."` value — a property signature, which
    // TypeScript describes. As above, the `string` after it answers nothing in a
    // plain `.ts` file either, so it cannot discriminate anything.
    const pos = findPosition(doc, "{ class: string", 2);
    expect(pos, 'the fixture declares attrs="{ class: string, ... }"').to.exist;

    const hovers = await waitForHoverMatching(doc.uri, pos!, {
      predicate: (candidates) =>
        candidates.length > 0 && hoverText(candidates[0]).includes("(property)"),
    });
    console.log(`    Hover inside attrs value: ${hovers.length} result(s)`);

    expect(
      hovers.length,
      'a position inside attrs="..." is mapped into the generated surface and must be answered',
    ).to.be.greaterThan(0);
    const content = hoverText(hovers[0]);
    console.log(`    Hover content: ${content.slice(0, 200)}`);

    // The exact member the attrs annotation declares, typed — not the SFC
    // documentation for the `attrs` attribute, and not a bare identifier echo.
    expect(content, "the provider describes the declared member").to.include(
      "(property) class: string",
    );
    expect(content.toLowerCase(), "this is the value, not the attribute's own docs").to.not.include(
      "fallthrough",
    );

    const controlHover = await controlHoverAt("class: string", 0);
    expect(controlHover, "the plain-TS oracle answers at the same token").to.include(
      "(property) class: string",
    );
  });

  test("hover on attrs attribute NAME shows SFC docs", async function () {
    const pos = findPosition(doc, 'attrs="', 0);
    expect(pos, "the fixture declares an attrs= attribute").to.exist;

    const hovers = await waitForHoverMatching(doc.uri, pos!, {
      predicate: (candidates) =>
        candidates.length > 0 && hoverText(candidates[0]).includes("useAttrs()"),
    });
    console.log(`    Hover on attrs attr name: ${hovers.length} result(s)`);

    expect(hovers.length, "the attribute name must be documented").to.be.greaterThan(0);
    const content = hoverText(hovers[0]);
    console.log(`    Hover content: ${content.slice(0, 200)}`);

    // Asserting the substring "attrs" was satisfied by the provider's
    // `(parameter) _attrs: { … }` — the wrong owner, passing on a coincidence.
    // `$attrs` and `useAttrs()` are the runtime surface this attribute types, and
    // only the native lane names them.
    expect(content, "names the attribute").to.include("**`attrs`**");
    expect(content, "names the runtime surface it types").to.include("$attrs");
    expect(content, "names the composable it types").to.include("useAttrs()");
    // `$attrs` and `useAttrs()` are what the old `include("attrs")` should have
    // said: the provider's half of this merged hover is `(parameter) _attrs: {…}`,
    // which contains "attrs" and none of the above. Both halves are asserted so
    // either one disappearing is a failure.
    expect(content, "the provider's answer is kept").to.include("(parameter) _attrs");
  });

  // ── Template Binding with Generics ──────────────────────────────

  test("hover on generic-typed binding in template", async function () {
    // `defineProps<{ value: T }>()` under `generic="T extends string"`, hovered from
    // the TEMPLATE. The point is that the component-level type parameter survives
    // the template projection — the old body asserted only that SOME content came
    // back, and never mentioned `T` at all.
    const pos = findPosition(doc, "{{ value }}", 3);
    expect(pos, "the fixture interpolates the generic-typed prop").to.exist;

    const hovers = await waitForHoverMatching(doc.uri, pos!, {
      predicate: (candidates) =>
        candidates.length > 0 && hoverText(candidates[0]).includes("value"),
    });
    console.log(`    Hover on generic binding: ${hovers.length} result(s)`);

    expect(hovers.length, "a template binding must be answered").to.be.greaterThan(0);
    const content = hoverText(hovers[0]);
    console.log(`    Hover content: ${content.slice(0, 200)}`);

    // The declared type is the type PARAMETER, not its erasure. A projection that
    // dropped the generic would answer `string` (the constraint) or `any` here, and
    // both are wrong answers that the old body accepted.
    expect(content, "the prop keeps its generic type").to.include("value: T");
    expect(content, "the generic did not degrade to any").to.not.match(/:\s*any\b/);
  });

  // ── Completion in Attrs/Generic Values ──────────────────────────

  test("completions inside attrs value should not show SFC attribute names", async function () {
    // Cursor in TYPE position inside attrs="{ class: string, id?: string }".
    // The old body ran every assertion behind `items.length > 0`, so a server that
    // answered nothing at all passed — and "no SFC attribute names" is trivially
    // true of an empty list. Absence of the wrong thing only means something once
    // the right thing is proven present.
    const pos = findPosition(doc, "{ class: string", 9);
    expect(pos, "the fixture declares an attrs= annotation").to.exist;

    const completions = await waitForCompletionsMatching(doc.uri, pos!, {
      predicate: (list) => (list?.items.length ?? 0) > 0,
    });
    const labels = (completions?.items ?? []).map((item) =>
      typeof item.label === "string" ? item.label : item.label.label,
    );
    console.log(`    Completions count: ${labels.length}`);

    expect(labels.length, "a type position inside attrs= must be completed").to.be.greaterThan(0);

    // Delegation, positively: these come from the TypeScript library's type scope,
    // which only the provider can enumerate. Without this the test could not tell
    // a real type-completion list from any other non-empty list.
    for (const libType of ["Record", "Partial", "Readonly"]) {
      expect(labels, `the TypeScript type scope is offered (${libType})`).to.include(libType);
    }

    // And the SFC attribute names are NOT offered — exact labels, because the old
    // substring check over the joined list would also have rejected any label that
    // merely CONTAINS "lang".
    for (const sfcAttr of ["setup", "lang", "generic", "attrs", "scoped", "module"]) {
      expect(labels, `SFC attribute "${sfcAttr}" is not a type completion`).to.not.include(sfcAttr);
    }
  });
});
