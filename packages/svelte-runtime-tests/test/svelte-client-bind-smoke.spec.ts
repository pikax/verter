// @vitest-environment happy-dom
//
// Behavioral smoke for the native Svelte client DOM-hosted bind family. It
// mounts Verter's EMITTED §1.2 modules against the REAL pinned `svelte@5.56.10`
// client runtime and asserts the observable DOM↔signal behavior of each
// `$.bind_*` host:
//   - `<textarea bind:value>`  — typing into the textarea updates the reflected `<p>`;
//   - `<select bind:value>`    — mounts the static options and reflects the INITIAL bound
//                                value (user→state is NOT asserted: the pinned runtime's
//                                `change` listener reads `:checked`, which happy-dom
//                                never matches against selected `<option>`s);
//   - `<input bind:checked>`   — toggling the checkbox updates the `<p>`;
//   - `<div contenteditable bind:innerHTML>` — editing innerHTML updates the `<p>`;
//   - `<details bind:open>`    — the `toggle` event updates the `<p>`;
//   - radio `bind:group`       — selecting a radio updates the `<p>` to its `value`.
//
// The update/cleanup arm extends the family with repeated updates,
// state→DOM writes, meaningful EMPTY/FALSE values, and unmount/remount controls:
//   - `<input bind:value>`          — typing twice, programmatic clearing to `""`,
//                                     post-unmount dispatch safety, remount freshness;
//   - `<select bind:value>` + value-carrying `<option>`s (the OPTION VALUE-CHANNEL
//     `option.value = option.__value = 'X'` init writes) — the applied initial
//     selection, programmatic clearing to the EMPTY-string option, repeated
//     clears, remount freshness;
//   - `<input bind:checked>` + toggle button — repeated flips and state→DOM writes
//     of BOTH boolean values;
//   - function-pair `bind:value` with an injected PROP setter — exactly ONE setter
//     call per input event (duplicate-listener control), a frozen reflection after
//     unmount, and cross-instance isolation across unmount/remount (the stale-
//     subscription control; the pinned runtime itself keeps input listeners on
//     detached nodes).
//
// Each mounted module is a committed `*.client.mjs` fixture, kept in lockstep with
// `compile_client`'s output by Rust equivalence tests
// (`bind_*_module_matches_the_committed_jsdom_smoke_fixture` in
// `crates/verter_compiler/src/svelte/runtime/client_tests.rs`) — so this smoke can
// never drift from the emitter. The emitted modules were verified to match the
// pinned-official compiler STRUCTURALLY (helper sequence + imports + templates) at
// authoring.

import { describe, expect, it } from "vitest";
import { flushSync, mount, unmount } from "svelte";

// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import TextareaValue from "./fixtures/svelte/bind_textarea_value.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import SelectValue from "./fixtures/svelte/bind_select_value.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import Checked from "./fixtures/svelte/bind_checked.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import ContentEditable from "./fixtures/svelte/bind_contenteditable.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import PropertyOpen from "./fixtures/svelte/bind_property_open.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import GroupRadio from "./fixtures/svelte/bind_group_radio.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import FunctionPairValue from "./fixtures/svelte/bind_function_pair_value.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import InputValue from "./fixtures/svelte/bind_input_value.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import SelectValueChannel from "./fixtures/svelte/bind_select_value_channel.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import CheckedUpdate from "./fixtures/svelte/bind_checked_update.client.mjs";
// @ts-expect-error — a plain emitted `.mjs` with no type declarations.
import PairSetter from "./fixtures/svelte/bind_pair_setter.client.mjs";

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

describe("native Svelte client emission — DOM-hosted bind behavioral smoke", () => {
  it("`<textarea bind:value>` writes the typed value back to the signal", () => {
    withMount(TextareaValue, (target) => {
      const textarea = target.querySelector("textarea") as HTMLTextAreaElement;
      const p = target.querySelector("p");
      expect(textarea).toBeTruthy();
      expect(p?.textContent).toBe("");

      textarea.value = "hello";
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("hello");
    });
  });

  it("`<select bind:value>` mounts static options and reflects the initial signal", () => {
    withMount(SelectValue, (target) => {
      const select = target.querySelector("select") as HTMLSelectElement;
      const p = target.querySelector("p");

      expect(select).toBeTruthy();
      expect(Array.from(select.options, (option) => option.textContent)).toEqual(["a", "b"]);
      expect(Array.from(select.options, (option) => option.value)).toEqual(["a", "b"]);
      expect(select.value).toBe("a");
      expect(p?.textContent).toBe("a");
      // NOTE: the DOM→signal round-trip for `<select>` is NOT asserted here — the
      // pinned official runtime's `change` listener reads the selection through the
      // `:checked` pseudo-selector, which happy-dom does not match against
      // programmatically-selected `<option>`s (it falls back to the first
      // non-disabled option). The user→state arm is covered where the host reads
      // `.value`/`.checked` PROPERTIES (`input`/`textarea`); the select arm below
      // covers the state→DOM effect and the option value-channel instead.
    });
  });

  it("`<input bind:checked>` writes the checked state back to the signal", () => {
    withMount(Checked, (target) => {
      const input = target.querySelector("input") as HTMLInputElement;
      const p = target.querySelector("p");
      expect(input).toBeTruthy();
      expect(p?.textContent).toBe("false");

      input.checked = true;
      input.dispatchEvent(new Event("change", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("true");
    });
  });

  it("`<div contenteditable bind:innerHTML>` writes the edited HTML back to the signal", () => {
    withMount(ContentEditable, (target) => {
      const div = target.querySelector("div[contenteditable]") as HTMLDivElement;
      const p = target.querySelector("p");
      expect(div).toBeTruthy();
      expect(p?.textContent).toBe("");

      div.innerHTML = "edited";
      div.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("edited");
    });
  });

  it("`<details bind:open>` writes the open state back to the signal on `toggle`", () => {
    withMount(PropertyOpen, (target) => {
      const details = target.querySelector("details") as HTMLDetailsElement;
      const p = target.querySelector("p");
      expect(details).toBeTruthy();
      expect(p?.textContent).toBe("false");

      details.open = true;
      details.dispatchEvent(new Event("toggle", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("true");
    });
  });

  it("radio `bind:group` writes the selected radio's value back to the signal", () => {
    withMount(GroupRadio, (target) => {
      const radios = target.querySelectorAll<HTMLInputElement>("input[type='radio']");
      const p = target.querySelector("p");
      expect(radios.length).toBe(2);
      // The per-input `input.value = input.__value = 'X'` ran at mount.
      expect(radios[0].value).toBe("a");
      expect(radios[1].value).toBe("b");
      expect(p?.textContent).toBe("");

      radios[1].checked = true;
      radios[1].dispatchEvent(new Event("change", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("b");

      radios[0].checked = true;
      radios[0].dispatchEvent(new Event("change", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("a");
    });
  });

  it("`<input bind:value={get, set}>` (function-pair) round-trips typing back to the bound signal", () => {
    // The DOM bind TARGET-LVALUE widening: a function-pair `bind:value` passes the
    // user-supplied get/set DIRECTLY to `$.bind_value`. The setter `(next) =>
    // $.set(value, next, true)` writes the SIGNAL, so typing into the input reaches the
    // signal and the reflecting `<p>{value}</p>` re-renders — the full DOM→signal→DOM
    // round-trip works at runtime against the real pinned svelte client.
    withMount(FunctionPairValue, (target) => {
      const input = target.querySelector("input") as HTMLInputElement;
      const p = target.querySelector("p");
      expect(input).toBeTruthy();
      expect(p?.textContent).toBe("");

      input.value = "typed";
      input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("typed");
    });
  });

  // ── update/cleanup arm: repeated updates, state→DOM, empty/false values, unmount ──

  it("`<input bind:value>` reflects initial state, repeated typing, and programmatic clearing to the empty string", () => {
    withMount(InputValue, (target) => {
      const input = target.querySelector("input") as HTMLInputElement;
      const p = target.querySelector("p");
      const clear = target.querySelector("button") as HTMLButtonElement;
      expect(input).toBeTruthy();

      // Initial state on BOTH sides of the binding.
      expect(input.value).toBe("init");
      expect(p?.textContent).toBe("init");

      // User → state, twice (repeated typing keeps updating the signal).
      input.value = "hello";
      input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("hello");

      input.value = "hello again";
      input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("hello again");

      // State → DOM: the clear button writes `""` — the input reflects the EMPTY
      // string (not `"null"`/`"undefined"`), and repeated typing after the reset
      // still round-trips.
      clear.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(input.value).toBe("");
      expect(p?.textContent).toBe("");

      input.value = "after";
      input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("after");

      clear.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(input.value).toBe("");
    });
  });

  it("`<input bind:value>` ignores events after unmount and remounts with fresh state", () => {
    const target = document.createElement("div");
    document.body.appendChild(target);
    const first = mount(InputValue as never, { target });
    const input = target.querySelector("input") as HTMLInputElement;
    const p = target.querySelector("p") as HTMLParagraphElement;
    input.value = "stale";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(p.textContent).toBe("stale");
    unmount(first);
    target.remove();

    // Post-unmount dispatch must neither throw nor resurrect the destroyed render —
    // the retained reflection node stays frozen at its last value.
    input.value = "zombie";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(p.textContent).toBe("stale");

    // Remount starts from the INITIAL state again (no module-level signal leak).
    const second_target = document.createElement("div");
    document.body.appendChild(second_target);
    const second = mount(InputValue as never, { target: second_target });
    try {
      const fresh_input = second_target.querySelector("input") as HTMLInputElement;
      expect(fresh_input.value).toBe("init");
      expect(second_target.querySelector("p")?.textContent).toBe("init");
    } finally {
      unmount(second);
      second_target.remove();
    }
  });

  it("`<select bind:value>` with value-carrying options applies the initial selection and programmatic clearing to the empty-string option", () => {
    withMount(SelectValueChannel, (target) => {
      const select = target.querySelector("select") as HTMLSelectElement;
      const p = target.querySelector("p");
      const clear = target.querySelector("button") as HTMLButtonElement;
      expect(select).toBeTruthy();

      // The OPTION VALUE-CHANNEL ran at mount: the bare options carry their
      // `__value`s (`""` / `a` / `b`). The initial-selection effect is deferred —
      // flush applies it and option `a` becomes the selection.
      flushSync();
      expect(Array.from(select.options, (option) => option.value)).toEqual(["", "a", "b"]);
      expect(select.value).toBe("a");
      expect(select.selectedIndex).toBe(1);
      expect(p?.textContent).toBe("a");

      // State → DOM: clearing the signal selects the EMPTY-STRING option — a
      // meaningful falsy value (`selectedIndex 0`, not a `-1` deselection).
      clear.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(select.value).toBe("");
      expect(select.selectedIndex).toBe(0);
      expect(p?.textContent).toBe("");

      // Repeated state flips: a second clear from the already-empty state stays on
      // the empty option (the empty→value→empty transition survives repetition).
      clear.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(select.value).toBe("");
      expect(select.selectedIndex).toBe(0);
      expect(p?.textContent).toBe("");
    });
  });

  it("`<select bind:value>` value-channel options remount with fresh state", () => {
    const target = document.createElement("div");
    document.body.appendChild(target);
    const first = mount(SelectValueChannel as never, { target });
    const clear = target.querySelector("button") as HTMLButtonElement;
    clear.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    flushSync();
    expect(target.querySelector("p")?.textContent).toBe("");
    unmount(first);
    target.remove();

    // A fresh mount re-applies the per-option `__value` writes and re-selects the
    // INITIAL signal value — stale first-instance state never leaks in.
    const second_target = document.createElement("div");
    document.body.appendChild(second_target);
    const second = mount(SelectValueChannel as never, { target: second_target });
    try {
      const fresh_select = second_target.querySelector("select") as HTMLSelectElement;
      flushSync();
      expect(Array.from(fresh_select.options, (option) => option.value)).toEqual(["", "a", "b"]);
      expect(fresh_select.value).toBe("a");
      expect(fresh_select.selectedIndex).toBe(1);
      expect(second_target.querySelector("p")?.textContent).toBe("a");
    } finally {
      unmount(second);
      second_target.remove();
    }
  });

  it("`<input bind:checked>` reflects repeated user flips and programmatic state flips of both boolean values", () => {
    withMount(CheckedUpdate, (target) => {
      const input = target.querySelector("input") as HTMLInputElement;
      const p = target.querySelector("p");
      const toggle = target.querySelector("button") as HTMLButtonElement;
      expect(input).toBeTruthy();

      // Initial FALSE on both sides.
      expect(input.checked).toBe(false);
      expect(p?.textContent).toBe("false");

      // User → state, twice (true then back to false — the falsy value round-trips).
      input.checked = true;
      input.dispatchEvent(new Event("change", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("true");

      input.checked = false;
      input.dispatchEvent(new Event("change", { bubbles: true }));
      flushSync();
      expect(p?.textContent).toBe("false");

      // State → DOM: the toggle button writes the signal, and the checkbox DOM
      // property reflects BOTH boolean values across repeated clicks.
      toggle.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(input.checked).toBe(true);
      expect(p?.textContent).toBe("true");

      toggle.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(input.checked).toBe(false);
      expect(p?.textContent).toBe("false");
    });
  });

  it("`<input bind:checked>` remounts with fresh false state", () => {
    const target = document.createElement("div");
    document.body.appendChild(target);
    const first = mount(CheckedUpdate as never, { target });
    const input = target.querySelector("input") as HTMLInputElement;
    input.checked = true;
    input.dispatchEvent(new Event("change", { bubbles: true }));
    flushSync();
    expect(target.querySelector("p")?.textContent).toBe("true");
    unmount(first);
    target.remove();

    const second_target = document.createElement("div");
    document.body.appendChild(second_target);
    const second = mount(CheckedUpdate as never, { target: second_target });
    try {
      const fresh_input = second_target.querySelector("input") as HTMLInputElement;
      expect(fresh_input.checked).toBe(false);
      expect(second_target.querySelector("p")?.textContent).toBe("false");
    } finally {
      unmount(second);
      second_target.remove();
    }
  });

  it("a function-pair setter fires exactly once per input event and stays isolated across unmount/remount", () => {
    const first_calls: string[] = [];
    const target = document.createElement("div");
    document.body.appendChild(target);
    const instance = mount(PairSetter as never, {
      target,
      props: { onSet: (next: string) => first_calls.push(next) },
    });
    const input = target.querySelector("input") as HTMLInputElement;
    const p = target.querySelector("p") as HTMLParagraphElement;

    // One input event → EXACTLY one setter call (a duplicate listener registration
    // would surface as two calls), and the pair's signal write re-renders `<p>`.
    input.value = "one";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(first_calls).toEqual(["one"]);
    expect(p.textContent).toBe("one");

    // Repeated updates keep the 1:1 dispatch→call ratio.
    input.value = "two";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(first_calls).toEqual(["one", "two"]);
    expect(p.textContent).toBe("two");

    unmount(instance);
    target.remove();

    // The pinned official runtime keeps the input listener on the DETACHED node (a
    // bare `addEventListener` — no teardown), so a post-unmount dispatch neither
    // throws nor resurrects the destroyed render: `<p>` stays frozen.
    input.value = "zombie";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(p.textContent).toBe("two");

    // STALE-SUBSCRIPTION CONTROL: a fresh instance owns a fresh signal — a dispatch
    // on the RETAINED first-instance node must never reach the second instance's
    // state or setter (a module-level signal/registration leak would show up as a
    // second-instance `<p>` update or a `second_calls` entry).
    const second_calls: string[] = [];
    const second_target = document.createElement("div");
    document.body.appendChild(second_target);
    const second = mount(PairSetter as never, {
      target: second_target,
      props: { onSet: (next: string) => second_calls.push(next) },
    });
    try {
      const fresh_input = second_target.querySelector("input") as HTMLInputElement;
      const fresh_p = second_target.querySelector("p") as HTMLParagraphElement;
      expect(fresh_p.textContent).toBe("");

      input.value = "leak";
      input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      expect(fresh_p.textContent).toBe("");
      expect(second_calls).toEqual([]);

      // The fresh instance's own binding still works 1:1 after the probe.
      fresh_input.value = "fresh";
      fresh_input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      expect(second_calls).toEqual(["fresh"]);
      expect(fresh_p.textContent).toBe("fresh");
    } finally {
      unmount(second);
      second_target.remove();
    }
  });
});
