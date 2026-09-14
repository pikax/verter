// Self-test: parse/runtime failure detection (BF2 required exit). The full
// linking-surface failure detection lives in test/link-surface.spec.mjs.

import { afterAll, describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compileSvelteFixture } from "../src/invoke-svelte-oracle.mjs";
import { compareArtifacts, checkParseValidity } from "../src/compare.mjs";
import {
  executeVueSsr,
  executeVueClientInteractions,
  cleanupScratch,
} from "../src/execute-vue-runtime.mjs";
import {
  executeSvelteSsr,
  cleanupScratch as cleanupSvelteScratch,
} from "../src/execute-svelte-runtime.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";
import { oracleLinkBaseDir } from "../src/oracle-install.mjs";

describe("parse failure detection", () => {
  it("flags syntactically broken candidate code", () => {
    const result = checkParseValidity("const x = {{{ this is not js", "candidate");
    expect(result.ok).toBe(false);
    expect(result.error).toBeTruthy();
  });

  it("compareArtifacts reports a parse failure without computing structural equality", async () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/vue/basic-interpolation.vue"),
      "utf8",
    );
    const golden = compileVueFixture(source, "fixtures/vue/basic-interpolation.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    const brokenCandidate = { code: "export default function( {", diagnostics: [] };
    const report = await compareArtifacts(golden, brokenCandidate, {
      linkBaseDir: oracleLinkBaseDir("vue"),
    });
    expect(report.verdict).toBe("fail");
    expect(report.candidateParse.ok).toBe(false);
    expect(report.structural).toBeNull();
  });
});

describe("runtime failure detection", () => {
  it("flags code that throws when executed against the official runtime", async () => {
    const result = await executeVueSsr('export default { render() { throw new Error("boom"); } }');
    expect(result.ok).toBe(false);
    expect(result.error).toContain("boom");
  });

  it("succeeds for real, correct compiled SSR output", async () => {
    const source = readFileSync(path.join(HARNESS_ROOT, "fixtures/vue/slots.vue"), "utf8");
    const ssr = compileVueFixture(source, "fixtures/vue/slots.vue", {
      backend: "ssr",
      sourceMap: false,
      isProd: false,
    });
    const result = await executeVueSsr(ssr.code);
    expect(result.ok).toBe(true);
    expect(result.html).toContain("panel");
    cleanupScratch();
  });

  it("Svelte: flags code that throws when executed against the official server runtime", async () => {
    const result = await executeSvelteSsr(
      'export default function() { throw new Error("svelte boom"); }',
    );
    expect(result.ok).toBe(false);
    expect(result.error).toContain("svelte boom");
    cleanupSvelteScratch();
  });

  it("Svelte: succeeds for real, correct compiled server output", async () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/svelte/legacy-slots.svelte"),
      "utf8",
    );
    const server = compileSvelteFixture(source, "fixtures/svelte/legacy-slots.svelte", {
      generate: "server",
      runes: false,
      dev: false,
      sourceMap: false,
    });
    expect(server.diagnostics.filter((d) => d.kind === "compile-error")).toEqual([]);
    const result = await executeSvelteSsr(server.code, { title: "Hello BF2" });
    expect(result.ok).toBe(true);
    expect(result.html).toContain("panel");
    expect(result.html).toContain("Hello BF2");
    cleanupSvelteScratch();
  });
});

// Runtime correctness (`behavior`) is decided only by executing both arms
// through the pinned official runtimes; output similarity (`fidelity`) only
// by the structural comparison. Neither stands in for the other.

/** Replaces tokens that occur only as identifiers, proving each applied and fresh. */
function renameTokens(code, pairs) {
  let out = code;
  for (const [from, to] of pairs) {
    expect(out.includes(to), to).toBe(false);
    const next = out.split(from).join(to);
    expect(next, from).not.toBe(out);
    out = next;
  }
  return out;
}

function replaceOnce(code, from, to) {
  expect(code.split(from).length - 1, from).toBe(1);
  return code.replace(from, to);
}

function vueSsrGolden() {
  const source = readFileSync(
    path.join(HARNESS_ROOT, "fixtures/vue/basic-interpolation.vue"),
    "utf8",
  );
  return compileVueFixture(source, "fixtures/vue/basic-interpolation.vue", {
    backend: "ssr",
    sourceMap: false,
    isProd: false,
  });
}

const VUE_SSR_LOCAL_RENAMES = [
  ["_sfc_main", "component"],
  ["_push", "emitChunk"],
  ["_ssrInterpolate", "interpolate"],
  ["$setup", "bindings"],
];

const TOGGLE_FIXTURE = `
<script setup>
import { ref } from "vue";
const open = ref(true);
function toggle() {
  open.value = !open.value;
}
</script>

<template>
  <div class="panel">
    <p v-if="open" data-testid="body">body</p>
    <button type="button" data-testid="toggle" @click="toggle">t</button>
  </div>
</template>
`;

/** Mounts, then toggles the `v-if` branch off, on and off again. */
function executeToggles(code) {
  const toggle = { kind: "click", target: "[data-testid=toggle]" };
  return executeVueClientInteractions(code, {
    observe: ["[data-testid=body]"],
    actions: [toggle, toggle, toggle],
  });
}

describe("runtime behaviour is reported apart from output similarity", () => {
  afterAll(() => {
    cleanupScratch();
    cleanupSvelteScratch();
  });

  it("a local-name-only difference executes both arms: runtime pass, output equivalent", async () => {
    const golden = vueSsrGolden();
    const candidate = { code: renameTokens(golden.code, VUE_SSR_LOCAL_RENAMES), diagnostics: [] };
    const executed = [];
    const execute = (code) => {
      executed.push(code);
      return executeVueSsr(code);
    };
    const report = await compareArtifacts(golden, candidate, {
      linkBaseDir: oracleLinkBaseDir("vue"),
      execute,
    });
    expect(executed).toEqual([golden.code, candidate.code]);
    expect(report.behavior).toEqual({ status: "pass", reasons: [] });
    expect(report.fidelity.status).toBe("equivalent");
    expect(report.verdict).toBe("pass");
  });

  it("without an executor the runtime is unrun, even when the modules are equivalent", async () => {
    const golden = vueSsrGolden();
    const candidate = { code: renameTokens(golden.code, VUE_SSR_LOCAL_RENAMES), diagnostics: [] };
    const report = await compareArtifacts(golden, candidate);
    expect(report.fidelity.status).toBe("equivalent");
    expect(report.behavior.status).toBe("unrun");
    expect(report.behavior.reasons).toEqual([]);
  });

  it("wrong rendered text fails the runtime check, reported apart from the structural finding", async () => {
    const golden = vueSsrGolden();
    const candidate = {
      code: replaceOnce(golden.code, "<p>zero</p>", "<p>none</p>"),
      diagnostics: [],
    };
    const report = await compareArtifacts(golden, candidate, { execute: executeVueSsr });
    expect(report.behavior.status).toBe("fail");
    expect(report.behavior.reasons.join("\n")).toMatch(
      /^runtime divergence: observed output differs at \$\.html/,
    );
    expect(report.fidelity.status).toBe("divergent");
    expect(report.verdict).toBe("fail");
  });

  it("a structural-only difference with identical runtime output claims no runtime defect", async () => {
    const golden = vueSsrGolden();
    // A template literal and a string literal with equal content are
    // different expressions that render identically.
    const candidate = {
      code: replaceOnce(golden.code, "_push(`<p>zero</p>`)", '_push("<p>zero</p>")'),
      diagnostics: [],
    };
    const report = await compareArtifacts(golden, candidate, { execute: executeVueSsr });
    expect(report.behavior).toEqual({ status: "pass", reasons: [] });
    expect(report.fidelity.status).toBe("divergent");
    expect(report.reasons.some((reason) => reason.startsWith("runtime divergence"))).toBe(false);
    expect(report.reasons.some((reason) => reason.startsWith("structural divergence"))).toBe(true);
  });

  it("Svelte server: local renames keep runtime pass and output equivalent; wrong text fails the runtime", async () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/svelte/basic-runes.svelte"),
      "utf8",
    );
    const golden = compileSvelteFixture(source, "fixtures/svelte/basic-runes.svelte", {
      generate: "server",
      runes: true,
      dev: false,
      sourceMap: false,
    });
    const execute = (code) => executeSvelteSsr(code);
    const renamed = {
      ...golden,
      code: renameTokens(golden.code, [
        ["$$renderer", "sink"],
        ["each_array", "row_list"],
        ["$$index", "position"],
        ["$$length", "limit"],
      ]),
    };
    const equivalent = await compareArtifacts(golden, renamed, { execute });
    expect(equivalent.behavior).toEqual({ status: "pass", reasons: [] });
    expect(equivalent.fidelity.status).toBe("equivalent");

    const wrongText = { ...golden, code: replaceOnce(golden.code, "<p>zero</p>", "<p>none</p>") };
    const wrong = await compareArtifacts(golden, wrongText, { execute });
    expect(wrong.behavior.status).toBe("fail");
    expect(wrong.behavior.reasons.join("\n")).toMatch(/observed output differs at \$\.html/);
  });

  it("update/cleanup: a branch left mounted after toggling off fails the runtime check; local renames do not", async () => {
    const golden = compileVueFixture(TOGGLE_FIXTURE, "fixtures/vue/inline-toggle.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    expect(golden.diagnostics).toEqual([]);
    const renamed = {
      ...golden,
      code: renameTokens(golden.code, [
        ["_sfc_main", "component"],
        ["_hoisted_2", "bodyProps"],
        ["_createCommentVNode", "placeholder"],
      ]),
    };
    const equivalent = await compareArtifacts(golden, renamed, { execute: executeToggles });
    expect(equivalent.behavior).toEqual({ status: "pass", reasons: [] });
    expect(equivalent.fidelity.status).toBe("equivalent");

    // The off branch renders the element again instead of the v-if
    // placeholder, so toggling off never removes it.
    const leaky = {
      ...golden,
      code: replaceOnce(
        golden.code,
        ': _createCommentVNode("v-if", true),',
        ': (_openBlock(), _createElementBlock("p", _hoisted_2, "body")),',
      ),
    };
    const report = await compareArtifacts(golden, leaky, { execute: executeToggles });
    expect(report.behavior.status).toBe("fail");
    expect(report.behavior.reasons.join("\n")).toMatch(/observed output differs at \$\.steps\[1\]/);
  });
});
