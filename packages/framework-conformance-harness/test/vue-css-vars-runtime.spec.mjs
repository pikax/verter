// CSS `v-bind()` reaches the intended custom property, and keeps reaching
// it when the bound value changes.
//
// The registration a compiler emits (`useCssVars(_ctx => ({ KEY: value }))`)
// is only half the contract: the runtime prepends `--` to KEY itself before
// writing it with `style.setProperty`. A registration that already carries
// `--` therefore lands on `----KEY`, which no `var(--KEY)` reference can
// read — the binding silently never applies, and no amount of reading the
// generated source shows it. These cases mount real compiled output through
// the pinned official runtime and read the custom properties actually set
// on the mounted element, initially and after a reactive change.
//
// These cases execute OFFICIAL output: this hermetic package has no Verter
// binding. The complementary lane that mounts VERTER's own assembled client
// module (compiled through the real carrier, same fixture class, values
// compared against the official module per reactive state) is the
// `vue_css_vars_client_mount` runtime proof in the bf2-authoritative Rust
// lane (`crates/verter_session/src/compile/map_equality_tests/`).

import { afterAll, describe, expect, it } from "vitest";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { executeVueClientMount, cleanupScratch } from "../src/execute-vue-runtime.mjs";

// The id becomes the custom-property prefix. A path-shaped id would need CSS
// escaping to be spellable as one; the production plugin passes a hash, so a
// hash-shaped token is the faithful input here.
const FIXTURE_ID = "abc123";

const FIXTURE = `
<script setup>
const props = defineProps({ color: { type: String, default: 'red' }, size: { type: String, default: '10px' } })
</script>
<template><div class="box">boxed</div></template>
<style scoped>
.box { color: v-bind(color); font-size: v-bind('props.size'); background: teal; }
</style>
`;

/** Every key the compiled module registers with `useCssVars`, in source order. */
function registeredKeys(code) {
  const call = code.slice(code.indexOf("useCssVars("));
  const body = call.slice(0, call.indexOf("}))"));
  return [...body.matchAll(/"((?:[^"\\]|\\.)*)"\s*:/g)].map((match) =>
    // A JS string literal: `\.` is the escaped spelling of `.`.
    match[1].replace(/\\(.)/g, "$1"),
  );
}

function compileClient() {
  const result = compileVueFixture(FIXTURE, FIXTURE_ID, {
    backend: "vdom",
    sourceMap: false,
    isProd: false,
  });
  expect(result.diagnostics).toEqual([]);
  expect(result.code).toBeTypeOf("string");
  return result.code;
}

// The scratch directory is shared by every case in this worker, so cleanup is
// file-scoped: a case that removed it on its way out could delete a module
// another case was still importing.
afterAll(() => {
  cleanupScratch();
});

describe("CSS v-bind custom properties through the pinned official runtime", () => {
  it("sets every registered variable on the mounted element, with exactly one `--` prefix", async () => {
    const code = compileClient();
    const keys = registeredKeys(code);
    expect(keys.length).toBe(2);
    for (const key of keys) expect(key.startsWith("--")).toBe(false);

    const result = await executeVueClientMount(code, {
      propSteps: [{ color: "red", size: "10px" }],
    });
    expect(result.error).toBeNull();
    expect(result.ok).toBe(true);
    expect(result.warnings).toEqual([]);

    const [initial] = result.steps;
    expect(initial.html).toContain("boxed");
    // Exactly the registered set, each prefixed exactly once.
    expect(Object.keys(initial.customProperties).sort()).toEqual(
      keys.map((key) => `--${key}`).sort(),
    );
    expect(Object.values(initial.customProperties).sort()).toEqual(["10px", "red"]);
    // The literal declaration in the same rule registers nothing.
    expect(Object.keys(initial.customProperties).length).toBe(2);
  }, 120_000); // jsdom + pinned-runtime import + a real mount, under parallel-worker contention

  it("updates the custom properties when the bound values change", async () => {
    const code = compileClient();
    const keys = registeredKeys(code);

    const result = await executeVueClientMount(code, {
      propSteps: [
        { color: "red", size: "10px" },
        { color: "blue", size: "22px" },
        { color: "green", size: "22px" },
      ],
    });
    expect(result.error).toBeNull();
    expect(result.warnings).toEqual([]);
    expect(result.steps.length).toBe(3);

    const values = result.steps.map((step) => keys.map((key) => step.customProperties[`--${key}`]));
    expect(values).toEqual([
      ["red", "10px"],
      ["blue", "22px"],
      ["green", "22px"],
    ]);
  }, 120_000); // jsdom + pinned-runtime import + a real mount, under parallel-worker contention

  it("loses the binding when the registered key already carries `--` (negative control)", async () => {
    const code = compileClient();
    const [firstKey] = registeredKeys(code);
    const registration = `"${firstKey.replace(/([^\w-])/g, "\\$1")}":`;
    // The plant must actually apply, and apply once: a mutation that missed
    // would make this control pass while proving nothing.
    expect(code.split(registration).length - 1).toBe(1);
    const doubled = code.replace(registration, `"--${firstKey.replace(/([^\w-])/g, "\\$1")}":`);
    expect(doubled).not.toBe(code);

    const result = await executeVueClientMount(doubled, {
      propSteps: [{ color: "red", size: "10px" }],
    });
    expect(result.error).toBeNull();

    const [initial] = result.steps;
    // The property the stylesheet's `var(--KEY)` reads is simply absent, and
    // the value went to a name nothing can reference.
    expect(initial.customProperties[`--${firstKey}`]).toBeUndefined();
    expect(initial.customProperties[`----${firstKey}`]).toBe("red");
  }, 120_000); // jsdom + pinned-runtime import + a real mount, under parallel-worker contention
});
