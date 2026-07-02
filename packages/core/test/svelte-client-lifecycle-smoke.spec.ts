// @vitest-environment happy-dom
//
// Behavioral smoke for the native Svelte client element lifecycle-directive surface
// (5f-c). It mounts Verter's EMITTED modules against the REAL pinned `svelte@5.56.3`
// client runtime and asserts the observable runtime behavior of each lifecycle
// helper:
//   - `use:act`        — the action fn RUNS on mount, receiving the mounted element;
//   - `transition:fx`  — the transition registers and RUNS the intro through the
//                        user fn when its `{#if}` branch is toggled on;
//   - `{@attach hook}` — the element-position attachment RUNS on mount with the
//                        element;
//   - `animate:fx`     — a keyed each carrying `animate:` mounts (the ANIMATED
//                        flag), and a keyed reorder actually reorders the DOM.
//
// Each mounted module is a committed `lifecycle_*.client.mjs` fixture, kept in
// lockstep with `compile_client`'s output by the Rust equivalence test
// (`lifecycle_smoke_modules_match_the_committed_jsdom_fixtures` in
// `crates/verter_compiler/src/svelte/runtime/client_tests.rs`) — so this smoke can
// never drift from the emitter. The emitted modules match the pinned-official
// compiler STRUCTURALLY (the `lifecycle/*` corpus in `svelte_client_emit_topology.rs`).

import { describe, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";

// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import UseAction from "./fixtures/svelte/lifecycle_use.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import Transition from "./fixtures/svelte/lifecycle_transition.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import Attach from "./fixtures/svelte/lifecycle_attach.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import Animate from "./fixtures/svelte/lifecycle_animate.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import UseLegacyEvent from "./fixtures/svelte/lifecycle_use_legacy_event.client.mjs";

/** Mount `App` with `props` into a fresh `<div>`, run `body`, always unmount. */
function withMount(
  App: unknown,
  props: Record<string, unknown>,
  body: (target: HTMLElement) => void,
): void {
  const target = document.createElement("div");
  document.body.appendChild(target);
  const instance = mount(App as never, { target, props });
  try {
    flushSync();
    body(target);
  } finally {
    unmount(instance);
    target.remove();
  }
}

describe("native Svelte client emission — element lifecycle behavioral smoke", () => {
  it("`use:act` runs the action on mount with the mounted element", () => {
    const seen: Element[] = [];
    const act = (node: Element) => {
      seen.push(node);
    };
    withMount(UseAction, { act }, (target) => {
      const div = target.querySelector("div");
      expect(div).toBeTruthy();
      expect(seen).toHaveLength(1);
      expect(seen[0]).toBe(div);
    });
  });

  it("`transition:fx` runs the intro through the user fn when the branch toggles on", () => {
    const calls: Array<{ node: Element }> = [];
    const fx = (node: Element) => {
      calls.push({ node });
      return { duration: 0 };
    };
    withMount(Transition, { fx }, (target) => {
      // The branch is initially OFF — no div, no transition run.
      expect(target.querySelector("div")).toBeNull();
      expect(calls).toHaveLength(0);

      // Toggle the branch on: the div mounts and the intro runs the user fn with
      // the mounted element.
      const button = target.querySelector("button") as HTMLButtonElement;
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      const div = target.querySelector("div");
      expect(div).toBeTruthy();
      expect(calls.length).toBeGreaterThanOrEqual(1);
      expect(calls[0]!.node).toBe(div);
    });
  });

  it("element-position `{@attach hook}` runs the attachment on mount", () => {
    const seen: Element[] = [];
    const hook = (node: Element) => {
      seen.push(node);
    };
    withMount(Attach, { hook }, (target) => {
      const div = target.querySelector("div");
      expect(div).toBeTruthy();
      expect(seen).toHaveLength(1);
      expect(seen[0]).toBe(div);
    });
  });

  it("`animate:fx` in a keyed each mounts and a keyed reorder reorders the DOM", () => {
    const fx = () => ({ duration: 0 });
    withMount(Animate, { fx }, (target) => {
      const texts = () =>
        Array.from(target.querySelectorAll("p")).map((p) => p.textContent);
      expect(texts()).toEqual(["a", "b"]);

      // Flip the keyed source — the two keyed items MOVE (the ANIMATED each still
      // reconciles correctly).
      const button = target.querySelector("button") as HTMLButtonElement;
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(texts()).toEqual(["b", "a"]);
    });
  });

  it("`use:act` + legacy `on:click` — the effect-wrapped listener fires and the action still runs", () => {
    // The non-delegated event beside a `use:` action registers inside
    // `$.effect(() => $.event('click', …))` (the official action-triggered wrap) —
    // the listener must still FIRE: a click increments the rendered count.
    const seen: Element[] = [];
    const act = (node: Element) => {
      seen.push(node);
    };
    withMount(UseLegacyEvent, { act }, (target) => {
      const div = target.querySelector("div") as HTMLDivElement;
      expect(div).toBeTruthy();
      // The action ran on mount with the element (order: action before the wrap).
      expect(seen).toHaveLength(1);
      expect(seen[0]).toBe(div);
      expect(div.textContent).toBe("0");

      div.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(div.textContent).toBe("1");
    });
  });
});
