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
    expect(result.mismatched).toBe(false);
    expect(result.finalHtml).toContain("click me");
    cleanupSvelteScratch();
    cleanupHydrationScratch();
  });

  it("reports mismatched: true for real client code hydrated onto deliberately mismatched server HTML (negative control)", () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/svelte/props-events.svelte"),
      "utf8",
    );
    const client = compileSvelteFixture(source, "fixtures/svelte/props-events.svelte", {
      generate: "client",
      runes: true,
      dev: false,
      sourceMap: false,
    });

    // Server HTML the client props would NEVER produce: hydration markers
    // intact, same node count, but a different element and different
    // content. Svelte's prod hydration silently CLAIMS the wrong node (it
    // checks structure, not tag names or content) — previously this was
    // reported as a clean hydration because `mismatched` was hardcoded
    // false; the fresh-render divergence oracle now reports it.
    const mismatchedSsrHtml = "<!--[--><div>WRONG SERVER CONTENT</div><!--]-->";
    expect(mismatchedSsrHtml).not.toContain("<button"); // plant proven mismatched vs the client render
    const adopted = hydrateSvelteClient(
      mismatchedSsrHtml,
      client.code,
      JSON.stringify({ label: "click me" }),
    );
    expect(adopted.ok).toBe(true);
    expect(adopted.mismatched).toBe(true);
    // The wrong server element genuinely survived hydration (node adoption,
    // not reconstruction) — exactly why the comparison is required.
    expect(adopted.finalHtml).toContain("<div>");
    expect(adopted.finalHtml).not.toContain("<button");

    // Marker-less server HTML: Svelte recovers by discarding the server DOM
    // and re-mounting fresh, with no prod warning and a final DOM equal to a
    // fresh render — caught by the server-node reuse identity signal.
    const recovered = hydrateSvelteClient(
      "<div>no hydration markers at all</div>",
      client.code,
      JSON.stringify({ label: "click me" }),
    );
    expect(recovered.ok).toBe(true);
    expect(recovered.mismatched).toBe(true);

    cleanupSvelteScratch();
    cleanupHydrationScratch();
  });

  it("reports mismatched: true for a TEXT-ROOT component hydrated onto markerless wrong server text (negative control)", async () => {
    // A component whose root is a bare dynamic text binding has ZERO element
    // children — the class where element-only reuse tracking is vacuously
    // clean and the comment-free fresh-render comparison sees nothing
    // either: Svelte discards the wrong markerless server content, rebuilds
    // fresh, and the final DOM equals a fresh render textually. The reuse
    // signal must therefore track ALL initial server child nodes (text and
    // marker comments included), so the replacement is observed.
    const source = "<script>let { label } = $props();</script>{label}";
    const client = compileSvelteFixture(source, "fixtures/svelte/text-root.svelte", {
      generate: "client",
      runes: true,
      dev: false,
      sourceMap: false,
    });
    expect(client.code).not.toBeNull();

    // Positive control first: the OFFICIAL marked server rendering of the
    // same component/props hydrates clean — the widened signal is not
    // trigger-happy for text-only roots.
    const server = compileSvelteFixture(source, "fixtures/svelte/text-root.svelte", {
      generate: "server",
      runes: true,
      dev: false,
      sourceMap: false,
    });
    const official = await executeSvelteSsr(server.code, { label: "hello" });
    expect(official.ok).toBe(true);
    expect(official.html).toContain("<!--[-->"); // the official render IS marker-bearing
    const control = hydrateSvelteClient(
      official.html,
      client.code,
      JSON.stringify({ label: "hello" }),
    );
    expect(control.ok).toBe(true);
    expect(control.mismatched).toBe(false);

    // Server HTML with NO hydration markers at all and wrong text: the
    // marker-bearing initial structure the contract names is entirely
    // absent, Svelte's recovery replaces the server text node, and the
    // result must be reported as a mismatch — not silently clean.
    const markerlessHtml = "WRONG MARKERLESS TEXT";
    expect(markerlessHtml).not.toContain("<!--"); // proven markerless
    const markerless = hydrateSvelteClient(
      markerlessHtml,
      client.code,
      JSON.stringify({ label: "hello" }),
    );
    expect(markerless.ok).toBe(true);
    expect(markerless.mismatched).toBe(true);

    // Torn markers under the same text-only root (opening boundary comment,
    // no anchor/close): also a mismatch, never silently clean.
    const torn = hydrateSvelteClient(
      "<!--[-->WRONG TEXT",
      client.code,
      JSON.stringify({ label: "hello" }),
    );
    expect(torn.ok).toBe(true);
    expect(torn.mismatched).toBe(true);

    cleanupSvelteScratch();
    cleanupHydrationScratch();
  }, 60_000); // six real child spawns (2 compiles, 1 SSR, 3 hydrates); the 5s default flakes under parallel worker contention

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
