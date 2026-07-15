// @vitest-environment happy-dom
//
// Behavioral smoke for the native Svelte client Block-5a surface (dynamic
// attributes + boolean DOM props + class/style). It mounts Verter's EMITTED
// modules against the REAL pinned `svelte@5.56.3` client runtime and asserts the
// observable DOM behavior reacts to `$state` changes:
//   - a dynamic `id={…}` attribute, a dynamic `class={…}` (via `$.clsx`), and a
//     `style:color={…}` directive (merged with a static `style` base) all update on
//     a delegated click;
//   - a boolean `readonly={…}` DOM property reflects via a direct property write,
//     toggled by a SEPARATE button so the property state never blocks the click.
//
// The mounted modules are the committed `*.client.mjs` fixtures, which Rust
// equivalence tests (`attr_class_style_module_matches_the_committed_jsdom_smoke_fixture`
// / `boolean_props_module_matches_the_committed_jsdom_smoke_fixture`) keep in lockstep
// with `compile_client`'s output — so this smoke can never drift from the emitter.

import { describe, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";

// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import AttrClassStyle from "./fixtures/svelte/attr_class_style.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import BooleanProps from "./fixtures/svelte/boolean_props.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import MixedClassCall from "./fixtures/svelte/mixed_class_call.client.mjs";

describe("native Svelte client emission — Block-5a behavioral smoke", () => {
  it("reacts a dynamic attr + class + style directive on a delegated click", () => {
    const target = document.createElement("div");
    document.body.appendChild(target);

    const instance = mount(AttrClassStyle as never, { target });
    try {
      const button = target.querySelector("button") as HTMLButtonElement;
      expect(button).toBeTruthy();

      // Initial render: the dynamic attr / class / style all applied.
      expect(button.getAttribute("id")).toBe("a");
      expect(button.className).toBe("box");
      // The static `style` base + the `style:color` directive both applied.
      expect(button.style.fontWeight).toBe("bold");
      expect(button.style.color).toBe("red");

      // Clicking bumps all three signals; the combined effect re-runs.
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(button.getAttribute("id")).toBe("a!");
      expect(button.className).toBe("box on");
      expect(button.style.color).toBe("blue");
      // The static base survives the directive merge.
      expect(button.style.fontWeight).toBe("bold");
    } finally {
      unmount(instance);
      target.remove();
    }
  });

  it("reflects a boolean DOM property (readonly) via a direct property write", () => {
    const target = document.createElement("div");
    document.body.appendChild(target);

    const instance = mount(BooleanProps as never, { target });
    try {
      const input = target.querySelector("input") as HTMLInputElement;
      const button = target.querySelector("button") as HTMLButtonElement;
      expect(input).toBeTruthy();
      expect(button).toBeTruthy();

      // Initial: `readonly={false}` → not read-only (the property write).
      expect(input.readOnly).toBe(false);

      // Clicking the separate toggle button flips the signal → the property reflects.
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(input.readOnly).toBe(true);

      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(input.readOnly).toBe(false);
    } finally {
      unmount(instance);
      target.remove();
    }
  });

  it("reacts a mixed class base with a call expression (per-part memoization)", () => {
    const target = document.createElement("div");
    document.body.appendChild(target);

    const instance = mount(MixedClassCall as never, { target });
    try {
      const button = target.querySelector("button") as HTMLButtonElement;
      expect(button).toBeTruthy();

      // The `class="a{String(c)}b"` base renders the call result spliced into the
      // literal run — `aString('x')b` → `axb`.
      expect(button.className).toBe("axb");

      // Clicking bumps `c` ('x' → 'x!'); the memoized `String(c)` dep re-runs and
      // the template re-renders the class.
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(button.className).toBe("ax!b");
    } finally {
      unmount(instance);
      target.remove();
    }
  });
});
