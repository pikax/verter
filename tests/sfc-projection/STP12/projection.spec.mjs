import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

import {
  assertInstanceMembers,
  materializeStp12GeneratedProbes,
} from "../../../scripts/sfc-projection/verify-node.mjs";
import { STP12_MANDATORY_CASES, assertRustCases } from "./protocol.mjs";

test("public instance probe rejects a missing consumer member", () => {
  const errors = assertInstanceMembers(
    { printed: "{ $props: {} }", flags: 0 },
    ["$props", "$emit"],
    "test",
    "STP12-jsdoc-generic",
  );
  assert.equal(errors.length, 1);
});

test("public instance probe uses a fresh compiler carrier, not the tracked declaration", () => {
  const scope = materializeStp12GeneratedProbes({
    repoRoot: path.resolve(new URL("../../..", import.meta.url).pathname),
    nodeManifest: {
      probes: { tsconfig: "tests/sfc-projection/STP12/probes/tsconfig.json" },
      cases: [
        {
          file: "tests/sfc-projection/STP12/probes/jsdoc-generic.ts",
        },
      ],
    },
    publicApi: "declare const Fresh: { new(): { $emit: () => void } }; export default Fresh;\n",
  });
  try {
    assert.equal(
      fs.readFileSync(scope.generatedCarrier, "utf8"),
      "declare const Fresh: { new(): { $emit: () => void } }; export default Fresh;\n",
    );
    assert.notEqual(
      scope.manifest.probes.tsconfig,
      "tests/sfc-projection/STP12/probes/tsconfig.json",
    );
  } finally {
    scope.close();
  }
});

test("JavaScript projection retains every mandatory case", () => {
  assert.deepEqual(
    [...STP12_MANDATORY_CASES],
    [
      "STP12-checkjs-off",
      "STP12-checkjs-on",
      "STP12-jsdoc-generic",
      "STP12-jsx",
      "STP12-suppression",
    ],
  );
});

test("JavaScript projection rejects incomplete Rust receipts", () => {
  const errors = assertRustCases({
    status: 0,
    error: null,
    stdout: "test unrelated ... ok\ntest result: ok. 1 passed; 0 failed\n",
  });
  assert.equal(errors.length, 7);
});

test("JavaScript projection accepts only a complete Rust receipt", () => {
  const names = [
    "vue_compiler_js_unchecked_script_keeps_template_projection",
    "vue_compiler_js_check_directive_is_a_leading_pragma",
    "vue_compiler_jsdoc_generic_uses_public_instance_contract",
    "javascript_setup_companions_match_the_published_consumer_carriers",
    "vue_compiler_jsx_keeps_authored_jsx_expression",
    "jsx_mode_instance_declaration_uses_public_constructor_bridge",
    "vue_compiler_js_projection_never_injects_nocheck",
  ];
  const errors = assertRustCases({
    status: 0,
    error: null,
    stdout: `${names.map((name) => `test ${name} ... ok`).join("\n")}\ntest result: ok. 6 passed; 0 failed\n`,
  });
  assert.equal(errors.length, 0, JSON.stringify(errors));
});
