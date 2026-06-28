// @vitest-environment happy-dom
//
// Behavioral smoke for the native Svelte client DOM-hosted bind family. It
// mounts Verter's EMITTED §1.2 modules against the REAL pinned `svelte@5.56.3`
// client runtime and asserts the observable DOM↔signal behavior of each
// `$.bind_*` host:
//   - `<textarea bind:value>`  — typing into the textarea updates the reflected `<p>`;
//   - `<select bind:value>`    — mounts the static options and reflects the INITIAL bound value (the full DOM→signal round-trip needs the option value-channel layer, proven structurally in Rust);
//   - `<input bind:checked>`   — toggling the checkbox updates the `<p>`;
//   - `<div contenteditable bind:innerHTML>` — editing innerHTML updates the `<p>`;
//   - `<details bind:open>`    — the `toggle` event updates the `<p>`;
//   - radio `bind:group`       — selecting a radio updates the `<p>` to its `value`.
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
});
