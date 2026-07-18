import assert from "node:assert/strict";
import { test } from "node:test";

import { extractRuntimeShape, generateOracle, oracleDiff } from "./oracle-lib.mjs";

test("extractRuntimeShape is formatting-insensitive but preserves runtime semantics", () => {
  const compact = `export default defineComponent({props:{value:{type:[Boolean,String],required:true,skipCheck:true}},emits:["change"]})`;
  const formatted = `
    export default defineComponent({
      props: {
        value: { type: [Boolean, String], required: true, skipCheck: true },
      },
      emits: ["change"],
    });
  `;

  assert.deepEqual(extractRuntimeShape(compact), extractRuntimeShape(formatted));
  assert.deepEqual(extractRuntimeShape(compact), {
    props: [
      {
        name: "value",
        constructors: ["Boolean", "String"],
        required: true,
        skipCheck: true,
        default: null,
      },
    ],
    emits: ["change"],
  });
});

test("oracleDiff discriminates constructor order and skipCheck drift", () => {
  const expected = {
    schemaVersion: 1,
    cases: [
      {
        id: "union",
        runtime: {
          props: [
            {
              name: "value",
              constructors: ["Boolean", "String"],
              required: true,
              skipCheck: true,
            },
          ],
          emits: [],
        },
      },
    ],
  };
  const reordered = structuredClone(expected);
  reordered.cases[0].runtime.props[0].constructors.reverse();
  const noSkipCheck = structuredClone(expected);
  noSkipCheck.cases[0].runtime.props[0].skipCheck = false;

  assert.match(oracleDiff(expected, reordered), /Boolean.*String|String.*Boolean/);
  assert.match(oracleDiff(expected, noSkipCheck), /skipCheck/);
  assert.equal(oracleDiff(expected, structuredClone(expected)), null);
});

test("the pinned compiler generates the complete deterministic fixture matrix", () => {
  const first = generateOracle();
  const second = generateOracle();

  assert.deepEqual(first, second);
  assert.equal(first.provenance.compiler, "@vue/compiler-sfc");
  assert.equal(first.provenance.version, "3.5.34");
  assert.ok(first.provenance.fixtureSha256.length === 64);
  assert.deepEqual(
    first.cases.map(({ id }) => id),
    [
      "primitive-and-bigint-props",
      "ordered-unions-and-skip-check",
      "containers-callables-and-nominals",
      "with-defaults",
      "emits-call-signature",
      "emits-property-syntax",
      "define-model-default-and-named",
      "vue-ignore",
      "imported-utility-and-indexed",
    ],
  );
});
