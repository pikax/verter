import assert from "node:assert/strict";
import { test } from "node:test";

import { extractRuntimeShape, generateOracle, oracleDiff } from "./oracle-lib.mjs";
import { vueMacroOracleGateCommands } from "../gate-internals.mjs";

test("the canonical gate owns both Vue macro oracle verification steps", () => {
  assert.deepEqual(vueMacroOracleGateCommands("/absolute/node"), [
    {
      name: "gen:vue-macro-oracle:check",
      cmd: "/absolute/node",
      args: ["scripts/gen-vue-macro-runtime-oracle.mjs", "--check"],
    },
    {
      name: "test:vue-macro-oracle",
      cmd: "/absolute/node",
      args: ["--test", "scripts/vue-macro-runtime-oracle/oracle.test.mjs"],
    },
  ]);
});

test("extractRuntimeShape is formatting-insensitive but preserves runtime semantics", () => {
  const compact = `export default defineComponent({props:{value:{type:[Boolean,String],required:true,skipCheck:true,default:"a  b"},method:{type:Function,required:false,default() { return "x  y"; }}},emits:["change"]})`;
  const formatted = `
    export default defineComponent({
      props: {
        value: {
          type: [Boolean, String],
          required: true,
          skipCheck: true,
          default: "a  b",
        },
        method: { type: Function, required: false, default() { return "x  y"; } },
      },
      emits: ["change"],
    });
  `;

  assert.deepEqual(extractRuntimeShape(compact), extractRuntimeShape(formatted));
  assert.deepEqual(extractRuntimeShape(compact), {
    props: [
      {
        name: "value",
        typePresent: true,
        constructors: ["Boolean", "String"],
        required: true,
        skipCheck: true,
        defaultKind: "property",
        default: '"a  b"',
      },
      {
        name: "method",
        typePresent: true,
        constructors: ["Function"],
        required: false,
        skipCheck: false,
        defaultKind: "method",
        default: 'default() { return "x  y"; }',
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
      "profile-default-rendering",
      "complete-imported-extension",
    ],
  );

  const profiles = first.cases.find(({ id }) => id === "profile-default-rendering");
  assert.deepEqual(
    profiles.profiles.map(({ name }) => name),
    ["development", "production", "production-custom-element"],
  );
  const development = profiles.profiles.find(({ name }) => name === "development").runtime;
  const production = profiles.profiles.find(({ name }) => name === "production").runtime;
  const customElement = profiles.profiles.find(
    ({ name }) => name === "production-custom-element",
  ).runtime;
  assert.deepEqual(
    development.props.find(({ name }) => name === "text"),
    {
      name: "text",
      typePresent: true,
      constructors: ["String"],
      required: true,
      skipCheck: false,
      defaultKind: "property",
      default: "'fallback'",
    },
  );
  assert.deepEqual(
    production.props.find(({ name }) => name === "text"),
    {
      name: "text",
      typePresent: false,
      constructors: [],
      required: null,
      skipCheck: false,
      defaultKind: "property",
      default: "'fallback'",
    },
  );
  assert.deepEqual(
    customElement.props.find(({ name }) => name === "text"),
    {
      name: "text",
      typePresent: true,
      constructors: ["String"],
      required: null,
      skipCheck: false,
      defaultKind: "property",
      default: "'fallback'",
    },
  );
  assert.deepEqual(production.props.find(({ name }) => name === "enabled").constructors, [
    "Boolean",
  ]);
  assert.deepEqual(customElement.props.find(({ name }) => name === "opaque").constructors, []);
  for (const runtime of [development, production, customElement]) {
    assert.equal(runtime.props.find(({ name }) => name === "method").defaultKind, "method");
    assert.match(
      runtime.props.find(({ name }) => name === "method").default,
      /^default\(\) \{ return 2 \}$/,
    );
  }
  assert.equal(production.props.find(({ name }) => name === "opaque").typePresent, false);
  assert.equal(customElement.props.find(({ name }) => name === "opaque").typePresent, true);

  const extension = first.cases.find(({ id }) => id === "complete-imported-extension");
  assert.equal(extension.contract, "verter-complete-extension");
  assert.equal(extension.extensionPolicy, "refine-only-on-complete");
  assert.deepEqual(extension.profiles[0].runtime.props[0].constructors, []);

  const nominal = first.cases.find(({ id }) => id === "containers-callables-and-nominals");
  assert.deepEqual(
    nominal.runtime.props
      .filter(({ name }) => name === "weakMap" || name === "weakSet")
      .map(({ name, constructors }) => ({ name, constructors })),
    [
      { name: "weakMap", constructors: ["WeakMap"] },
      { name: "weakSet", constructors: ["WeakSet"] },
    ],
  );
});
