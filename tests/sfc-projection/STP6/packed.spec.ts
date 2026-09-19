import assert from "node:assert/strict";
import test from "node:test";

import {
  assertDeclMaps,
  assertLeakyMapRejected,
  assertLeakyTwinPresent,
  assertPackInventory,
  assertPublicDeclarationsClosed,
  evaluateStp6Static,
  scanDeclarationText,
  validateStp6Products,
} from "./protocol.mjs";

test("STP6 products name every mandatory case, consumption mode, and resolution mode", () => {
  const errors = validateStp6Products();
  assert.equal(errors.length, 0, JSON.stringify(errors));
});

test("STP6-closure: published declarations do not import private virtual paths", () => {
  assert.equal(assertPublicDeclarationsClosed().length, 0);
  assert.equal(assertLeakyTwinPresent().length, 0);
});

test("STP6-closure dirty twin: a public virtual import is rejected", () => {
  const dirty = scanDeclarationText(
    'import type { Hidden } from "./__virtual_sfc/script";\nexport declare class Comp {}\n',
    "types/index.d.ts",
  );
  assert.ok(dirty.some((error) => error.caseId === "STP6-closure"));
});

test("STP6-decl-map: shipped maps resolve authored Vue sources in the pack", () => {
  assert.equal(assertDeclMaps().length, 0);
  assert.equal(assertLeakyMapRejected().length, 0);
});

test("STP6-decl-map dirty twin: a virtual map source is rejected", () => {
  const dirty = assertDeclMaps({
    maps: {
      "types/concrete.d.ts.map": JSON.stringify({
        version: 3,
        file: "concrete.d.ts",
        sources: ["../__virtual_sfc/script.ts"],
        mappings: "AAAA",
      }),
    },
  });
  assert.ok(dirty.some((error) => error.caseId === "STP6-decl-map"));
});

test("STP6-hidden-metadata: unpublished sidecar is not packed or exported", () => {
  assert.equal(assertPackInventory().length, 0);
});

test("STP6 static protocol covers every mandatory case", () => {
  const errors = evaluateStp6Static();
  assert.equal(errors.length, 0, JSON.stringify(errors));
});
