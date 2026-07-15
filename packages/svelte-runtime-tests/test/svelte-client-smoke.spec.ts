// @vitest-environment happy-dom
//
// Behavioral smoke for the native Svelte client emission (§1.2 conformance
// target). It mounts Verter's EMITTED §1.2 module against the REAL pinned
// `svelte@5.56.3` client runtime and asserts the observable DOM behavior:
//   - the initial render shows `Hello world!` and `clicks: 0`;
//   - editing the `<input>` (a `bind:value`) updates the `<h1>` reactively;
//   - clicking the `<button>` (a delegated `onclick`) updates the count text.
//
// The mounted module is the committed `hello_input.client.mjs` fixture, which a
// Rust equivalence test
// (`hello_input_module_matches_the_committed_jsdom_smoke_fixture`) keeps in lockstep
// with `compile_client`'s output — it NORMALIZES cosmetic differences (the committed
// copy is `oxfmt`-formatted: tabs→spaces, single→double quotes), but any STRUCTURAL
// / semantic divergence (a different helper, a missing call, a changed order) fails
// there and forces a reviewed fixture regeneration. So this smoke can never drift
// semantically from the emitter. This is the MINIMAL behavioral gate; the full
// behavioral harness is a separate, later effort.

import { describe, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";

// The Verter-emitted §1.2 client module (byte-pinned to the emitter in Rust).
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import App from "./fixtures/svelte/hello_input.client.mjs";

describe("native Svelte client emission — §1.2 behavioral smoke", () => {
  it("mounts, reacts to input, and reacts to a delegated click", () => {
    const target = document.createElement("div");
    document.body.appendChild(target);

    const instance = mount(App as never, { target });
    try {
      const h1 = target.querySelector("h1");
      const input = target.querySelector("input") as HTMLInputElement;
      const button = target.querySelector("button");

      // Initial render.
      expect(h1?.textContent).toBe("Hello world!");
      expect(button?.textContent).toBe("clicks: 0");
      // `$.remove_input_defaults` ran — the input carries the bound value, not a
      // stale default attribute.
      expect(input).toBeTruthy();

      // Editing the input (the `bind:value`) updates the `<h1>` reactively.
      input.value = "Svelte";
      input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      expect(h1?.textContent).toBe("Hello Svelte!");

      // Clicking the button (the delegated `onclick`) bumps the count.
      button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(button?.textContent).toBe("clicks: 1");

      button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(button?.textContent).toBe("clicks: 2");
    } finally {
      unmount(instance);
      target.remove();
    }
  });
});
