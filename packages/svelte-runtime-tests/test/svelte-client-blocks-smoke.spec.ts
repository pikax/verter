// @vitest-environment happy-dom
//
// Behavioral smoke for the native Svelte client CONTROL-FLOW blocks (5e). It mounts
// Verter's EMITTED modules against the REAL pinned `svelte@5.56.3` client runtime and
// asserts the observable mount-and-react behavior of each block:
//   - `{#if}`   — the truthy branch renders its body;
//   - `{#each}` — a `$props()`-sourced array renders one body per item, the item read
//                 being a SIGNAL (`$.get(row)`) so each `<p>` reflects its array element;
//   - `{#key}`  — the keyed block renders its body AND a reactive `count` read INSIDE the
//                 block updates on a delegated click (block-interior reactivity).
//
// Each mounted module is a committed `*.client.mjs` fixture, kept in lockstep with
// `compile_client`'s output by the Rust equivalence test
// (`block_smoke_modules_match_the_committed_jsdom_fixtures` in
// `crates/verter_compiler/src/svelte/runtime/client_tests.rs`) — so this smoke can never
// drift from the emitter. Every fixture source ALSO lives in the golden corpus
// (`svelte_oracle_corpus/fixtures/blocks/`), so the emitted module is independently proven
// STRUCTURALLY conformant to the official compiler; this spec adds the BEHAVIORAL proof.

import { describe, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";

// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import BlockIfSingle from "./fixtures/svelte/block_if_single.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import BlockEachUnkeyed from "./fixtures/svelte/block_each_unkeyed.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import BlockKeyReactive from "./fixtures/svelte/block_key_reactive.client.mjs";

/** Mount `App` (with optional `props`) into a fresh detached `<div>`, run `body`, always unmount. */
function withMount(
  App: unknown,
  props: Record<string, unknown>,
  body: (target: HTMLElement) => void,
): void {
  const target = document.createElement("div");
  document.body.appendChild(target);
  const instance = mount(App as never, { target, props } as never);
  try {
    body(target);
  } finally {
    unmount(instance);
    target.remove();
  }
}

describe("native Svelte client emission — control-flow block behavioral smoke", () => {
  it("`{#if}` renders the truthy branch body", () => {
    withMount(BlockIfSingle, {}, (target) => {
      const p = target.querySelector("p");
      expect(p).toBeTruthy();
      expect(p?.textContent).toBe("shown");
    });
  });

  it("`{#each}` renders one body per `$props()` item, each item a live signal", () => {
    withMount(BlockEachUnkeyed, { rows: ["a", "b", "c"] }, (target) => {
      const items = [...target.querySelectorAll("p")].map((p) => p.textContent);
      expect(items).toEqual(["a", "b", "c"]);
    });
  });

  it("`{#each}` over an empty array renders no body", () => {
    withMount(BlockEachUnkeyed, { rows: [] }, (target) => {
      expect(target.querySelectorAll("p").length).toBe(0);
    });
  });

  it("`{#key}` renders its body and reflects a delegated-click update to a block-interior signal", () => {
    withMount(BlockKeyReactive, {}, (target) => {
      const p = target.querySelector("p");
      const button = target.querySelector("button");
      expect(p?.textContent).toBe("5");
      expect(button?.textContent).toBe("inc");

      // The delegated `onclick` bumps the reactive `count` read INSIDE the `{#key}` block.
      button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("6");

      button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("7");
    });
  });
});
