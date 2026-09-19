import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  assertDeclMaps,
  assertLeakyMapRejected,
  assertLeakyTwinPresent,
  assertPackInventory,
  assertPublicDeclarationsClosed,
  evaluateStp6Static,
  importSpecifiers,
  scanDeclarationText,
  validateStp6Products,
} from "./protocol.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));

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

test("STP6-closure dirty twins: import() and import-equals virtual dependencies are rejected", () => {
  const typeImport = 'export type Leak = import("./__virtual_sfc/script").Hidden;\n';
  const importEquals = 'import Hidden = require("./__virtual_sfc/script");\nexport { Hidden };\n';
  assert.deepEqual(importSpecifiers(typeImport), ["./__virtual_sfc/script"]);
  assert.deepEqual(importSpecifiers(importEquals), ["./__virtual_sfc/script"]);
  for (const text of [typeImport, importEquals]) {
    const errors = scanDeclarationText(text, "types/index.d.ts");
    assert.ok(
      errors.some((error) => error.caseId === "STP6-closure" && error.code === "virtual-import"),
      JSON.stringify(errors),
    );
  }
});

test("STP6-closure: published ESM declarations use explicit .js relative specifiers", () => {
  for (const rel of ["types/index.d.ts", "types/generic.d.ts"]) {
    const text = fs.readFileSync(path.join(HERE, "packed", rel), "utf8");
    const relatives = importSpecifiers(text).filter((spec) => spec.startsWith("."));
    assert.ok(relatives.length > 0, `${rel} has no relative specifiers`);
    for (const spec of relatives) {
      assert.match(spec, /\.js$/, `${rel} imports extensionless ${spec}`);
    }
  }
});

test("STP6-decl-map dirty twins: an unpublished or non-authored map source is rejected", () => {
  for (const [source, code] of [
    ["../types/leaky.d.ts", "unpublished-map-source"],
    ["../package.json", "non-authored-map-source"],
  ]) {
    const errors = assertDeclMaps({
      maps: {
        "types/concrete.d.ts.map": JSON.stringify({
          version: 3,
          file: "concrete.d.ts",
          sources: [source],
          mappings: "AAAA",
        }),
      },
    });
    assert.ok(
      errors.some((error) => error.caseId === "STP6-decl-map" && error.code === code),
      `${source}: ${JSON.stringify(errors)}`,
    );
  }
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
