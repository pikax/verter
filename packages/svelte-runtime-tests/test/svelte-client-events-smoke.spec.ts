// @vitest-environment happy-dom
//
// Behavioral smoke for the native Svelte client regular-element event surface. It
// mounts Verter's EMITTED modules against the REAL pinned `svelte@5.56.3` client
// runtime and asserts the observable runtime behavior of each event registration:
//   - a non-delegated `$.event` listener attaches + fires (`onfocus`);
//   - `|once`                 — the handler fires only ONCE across repeated dispatch;
//   - `|preventDefault`       — the dispatched event is actually `defaultPrevented`;
//   - `|stopPropagation`      — a parent listener does NOT see the stopped event;
//   - `|self`                 — the handler fires only when `target === currentTarget`;
//   - `|capture`              — the capture-phase handler fires BEFORE the bubble handler.
//
// Each mounted module is a committed `event_*.client.mjs` fixture, kept in lockstep
// with `compile_client`'s output by the Rust equivalence test
// (`event_smoke_modules_match_the_committed_jsdom_fixtures` in
// `crates/verter_compiler/src/svelte/runtime/client_tests.rs`) — so this smoke can
// never drift from the emitter. The emitted modules match the pinned-official compiler
// STRUCTURALLY (the `events/*` corpus in `svelte_client_emit_topology.rs`).

import { describe, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";

// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import NonDelegated from "./fixtures/svelte/event_nondelegated.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import Once from "./fixtures/svelte/event_once.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import PreventDefault from "./fixtures/svelte/event_prevent_default.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import StopPropagation from "./fixtures/svelte/event_stop_propagation.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import SelfModifier from "./fixtures/svelte/event_self.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import Capture from "./fixtures/svelte/event_capture.client.mjs";

/** Mount `App` into a fresh detached `<div>`, run `body`, and always unmount. */
function withMount(App: unknown, body: (target: HTMLElement) => void): void {
  const target = document.createElement("div");
  document.body.appendChild(target);
  const instance = mount(App as never, { target });
  try {
    body(target);
  } finally {
    unmount(instance);
    target.remove();
  }
}

describe("native Svelte client emission — regular-element event behavioral smoke", () => {
  it("a non-delegated `onfocus` listener attaches and fires", () => {
    withMount(NonDelegated, (target) => {
      const input = target.querySelector("input") as HTMLInputElement;
      const p = target.querySelector("p");
      expect(input).toBeTruthy();
      expect(p?.textContent).toBe("false");

      input.dispatchEvent(new FocusEvent("focus"));
      flushSync();
      expect(p?.textContent).toBe("true");
    });
  });

  it("`|once` fires the handler only once across repeated dispatch", () => {
    withMount(Once, (target) => {
      const button = target.querySelector("button") as HTMLButtonElement;
      const p = target.querySelector("p");
      expect(p?.textContent).toBe("0");

      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("1");

      // A second (and third) click must NOT increment again — the once wrapper
      // removed itself after the first invocation.
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("1");
    });
  });

  it("`|preventDefault` marks the dispatched event `defaultPrevented` and still runs the handler", () => {
    withMount(PreventDefault, (target) => {
      const button = target.querySelector("button") as HTMLButtonElement;
      const p = target.querySelector("p");
      expect(p?.textContent).toBe("0");

      const ev = new MouseEvent("click", { bubbles: true, cancelable: true });
      button.dispatchEvent(ev);
      flushSync();
      // The wrapper called `event.preventDefault()` AND ran the handler.
      expect(ev.defaultPrevented).toBe(true);
      expect(p?.textContent).toBe("1");
    });
  });

  it("`|stopPropagation` keeps a parent listener from seeing the event", () => {
    withMount(StopPropagation, (target) => {
      const button = target.querySelector("button") as HTMLButtonElement;
      const p = target.querySelector("p");
      // `inner-outer` — both start at 0.
      expect(p?.textContent).toBe("0-0");

      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      // The inner (button) handler ran; the outer (div) handler did NOT (the event
      // stopped propagating).
      expect(p?.textContent).toBe("1-0");
    });
  });

  it("`|self` fires only when the event target is the element itself", () => {
    withMount(SelfModifier, (target) => {
      const div = target.querySelector("div") as HTMLDivElement;
      const button = target.querySelector("button") as HTMLButtonElement;
      const p = target.querySelector("p");
      expect(p?.textContent).toBe("0");

      // Clicking the CHILD button bubbles to the div, but `target !== currentTarget`,
      // so the `$.self`-wrapped handler is skipped.
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("0");

      // Clicking the div directly (`target === currentTarget`) runs the handler.
      div.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("1");
    });
  });

  it("`|capture` runs the capture-phase handler before the bubble handler", () => {
    withMount(Capture, (target) => {
      const button = target.querySelector("button") as HTMLButtonElement;
      const p = target.querySelector("p");
      expect(p?.textContent).toBe("");

      // The div has TWO `on:click` listeners: the BUBBLE handler (`B`) registered FIRST,
      // the CAPTURE handler (`C`) registered SECOND. A click on the child button descends
      // through the div in the CAPTURE phase — so the capture handler runs FIRST (`C`),
      // then the bubble handler on the way back up (`B`), yielding `CB`. This ordering is
      // DISCRIMINATING: if the emitter dropped the 4th positional `true`, both listeners
      // would be bubble-phase and fire in REGISTRATION order (`B` then `C` → `BC`), so the
      // `CB` assertion fails. (Registering bubble-first is what makes phase ordering, not
      // registration order, the thing under test.)
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("CB");
    });
  });
});
