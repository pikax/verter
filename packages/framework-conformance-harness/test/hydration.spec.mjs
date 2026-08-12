// Self-test: hydration controls (hydrateVue, hydrateSvelteClient) actually
// drive real official-core artifacts end to end. Mirrors the pattern in
// failure-detection.spec.mjs's execute-*-runtime self-tests: compile a real
// fixture through the official pinned compiler for BOTH the server and
// client backends, execute the server half for real SSR HTML, then drive
// the hydration entry point with the real client code against that real
// HTML — never a mocked or hand-authored artifact.
//
// This is hydration pairing #1 only (official server / official client, per
// hydration.mjs's module doc) — pairings #2/#3 need real Verter-compiled
// candidate output that does not exist yet at this point in the program
// (BV1/BS1 are downstream of BF2). These tests exist to prove `hydrateVue`
// and `hydrateSvelteClient` have real callers and real passing behavior
// today, closing the "zero test/CLI callers" gap.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compileSvelteFixture } from "../src/invoke-svelte-oracle.mjs";
import { executeVueSsr, cleanupScratch as cleanupVueScratch } from "../src/execute-vue-runtime.mjs";
import {
  executeSvelteSsr,
  cleanupScratch as cleanupSvelteScratch,
} from "../src/execute-svelte-runtime.mjs";
import { hydrateVue, hydrateSvelteClient, cleanupHydrationScratch } from "../src/hydration.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";

describe("hydrateVue — official server / official client (pairing #1)", () => {
  it("hydrates real official-compiled client code onto real official-rendered SSR HTML without mismatch", async () => {
    const source = readFileSync(path.join(HARNESS_ROOT, "fixtures/vue/slots.vue"), "utf8");
    const ssr = compileVueFixture(source, "fixtures/vue/slots.vue", {
      backend: "ssr",
      sourceMap: false,
      isProd: false,
    });
    const ssrResult = await executeVueSsr(ssr.code);
    expect(ssrResult.ok).toBe(true);

    const client = compileVueFixture(source, "fixtures/vue/slots.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });

    const result = await hydrateVue(ssrResult.html, client.code);
    expect(result.ok).toBe(true);
    expect(result.mismatched).toBe(false);
    expect(result.finalHtml).toContain("panel");
    cleanupVueScratch();
    cleanupHydrationScratch();
  });

  it("reports a real error for client code that throws during mount (negative control)", async () => {
    const result = await hydrateVue(
      "<div>irrelevant</div>",
      'export default { render() { throw new Error("hydrate boom"); } }',
    );
    expect(result.ok).toBe(false);
    expect(result.error).toContain("hydrate boom");
    cleanupHydrationScratch();
  });
});

describe("hydrateSvelteClient — official server / official client (pairing #1)", () => {
  it("hydrates real official-compiled client code onto real official-rendered SSR HTML", async () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/svelte/props-events.svelte"),
      "utf8",
    );
    const server = compileSvelteFixture(source, "fixtures/svelte/props-events.svelte", {
      generate: "server",
      runes: true,
      dev: false,
      sourceMap: false,
    });
    const ssrResult = await executeSvelteSsr(server.code, { label: "click me" });
    expect(ssrResult.ok).toBe(true);
    expect(ssrResult.html).toContain("click me");

    const client = compileSvelteFixture(source, "fixtures/svelte/props-events.svelte", {
      generate: "client",
      runes: true,
      dev: false,
      sourceMap: false,
    });

    const result = hydrateSvelteClient(
      ssrResult.html,
      client.code,
      JSON.stringify({ label: "click me" }),
    );
    expect(result.ok).toBe(true);
    expect(result.finalHtml).toContain("click me");
    cleanupSvelteScratch();
    cleanupHydrationScratch();
  });

  it("reports a real error for client code that throws during hydrate (negative control)", () => {
    const result = hydrateSvelteClient(
      "<div>irrelevant</div>",
      'export default function() { throw new Error("svelte hydrate boom"); }',
      "{}",
    );
    expect(result.ok).toBe(false);
    expect(result.error).toContain("svelte hydrate boom");
    cleanupHydrationScratch();
  });
});
