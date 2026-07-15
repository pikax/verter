// @vitest-environment happy-dom

import { describe, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// @ts-expect-error — plain emitted modules intentionally have no declarations.
import SpreadHtml from "./fixtures/svelte/breadth_spread_html.client.mjs";
// @ts-expect-error — plain emitted modules intentionally have no declarations.
import AwaitBlock from "./fixtures/svelte/breadth_await.client.mjs";
// @ts-expect-error — plain emitted modules intentionally have no declarations.
import Snippet from "./fixtures/svelte/breadth_snippet.client.mjs";
// @ts-expect-error — plain emitted modules intentionally have no declarations.
import DynamicChild from "./fixtures/svelte/breadth_dynamic_child.client.mjs";
// @ts-expect-error — plain emitted modules intentionally have no declarations.
import DynamicParent from "./fixtures/svelte/breadth_dynamic_parent.client.mjs";
// @ts-expect-error — plain emitted modules intentionally have no declarations.
import SpecialHeadWindow from "./fixtures/svelte/breadth_special_head_window.client.mjs";
// @ts-expect-error — plain emitted modules intentionally have no declarations.
import LegacyStore from "./fixtures/svelte/breadth_legacy_store.client.mjs";
// @ts-expect-error — plain emitted modules intentionally have no declarations.
import TypeScriptScript from "./fixtures/svelte/breadth_typescript_script.client.mjs";
// @ts-expect-error — plain emitted modules intentionally have no declarations.
import RunesEffect from "./fixtures/svelte/breadth_runes_effect.client.mjs";
// @ts-expect-error — plain emitted modules intentionally have no declarations.
import ScopedCss from "./fixtures/svelte/breadth_scoped_css.client.mjs";

const scopedCssText = readFileSync(
  resolve(process.cwd(), "test/fixtures/svelte/breadth_scoped_css.css"),
  "utf8",
);

async function withMount(
  App: unknown,
  props: Record<string, unknown>,
  body: (target: HTMLElement) => void | Promise<void>,
): Promise<void> {
  const target = document.createElement("div");
  document.body.appendChild(target);
  const instance = mount(App as never, { target, props } as never);
  try {
    flushSync();
    await body(target);
  } finally {
    unmount(instance);
    target.remove();
  }
}

describe("native Svelte client emission — supported runtime breadth", () => {
  it("updates spread attributes and raw HTML independently", async () => {
    await withMount(SpreadHtml, {}, (target) => {
      const host = target.querySelector("div") as HTMLDivElement;
      const [titleButton, htmlButton] = target.querySelectorAll<HTMLButtonElement>("button");
      expect(host.title).toBe("a");
      expect(host.querySelector("strong")?.textContent).toBe("one");

      titleButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(host.title).toBe("a!");

      htmlButton.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(host.querySelector("strong")).toBeNull();
      expect(host.querySelector("em")?.textContent).toBe("two");
    });
  });

  it("settles an await block into its then branch", async () => {
    await withMount(AwaitBlock, {}, async (target) => {
      await Promise.resolve();
      await tick();
      flushSync();
      expect(target.querySelector("p")?.textContent).toBe("ready");
    });
  });

  it("renders a local snippet through the render helper", async () => {
    await withMount(Snippet, {}, (target) => {
      expect(target.querySelector("p")?.textContent).toBe("rendered");
    });
  });

  it("mounts a dynamic component and propagates a reactive prop update", async () => {
    await withMount(DynamicParent, { Child: DynamicChild }, (target) => {
      const button = target.querySelector("button") as HTMLButtonElement;
      expect(target.querySelector("p")?.textContent).toBe("a");
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(target.querySelector("p")?.textContent).toBe("a!");
    });
  });

  it("updates a window listener and a reactive head title", async () => {
    const previousTitle = document.title;
    try {
      await withMount(SpecialHeadWindow, {}, (target) => {
        expect(document.title).toBe("initial");
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "K" }));
        flushSync();
        expect(target.querySelector("p")?.textContent).toBe("K");
        expect(document.title).toBe("K");
      });
    } finally {
      document.title = previousTitle;
    }
  });

  it("runs legacy reactive state over a real svelte store", async () => {
    await withMount(LegacyStore, {}, (target) => {
      const button = target.querySelector("button") as HTMLButtonElement;
      expect(button.textContent).toBe("2");
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(button.textContent).toBe("4");
    });
  });

  it("executes TypeScript-erased functions, classes, and typed state", async () => {
    await withMount(TypeScriptScript, {}, (target) => {
      const button = target.querySelector("button") as HTMLButtonElement;
      expect(button.textContent).toBe("0");
      button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      flushSync();
      expect(button.textContent).toBe("2");
    });
  });

  it("runs a user effect for initial and updated raw state", async () => {
    const log = vi.spyOn(console, "log").mockImplementation(() => undefined);
    try {
      await withMount(RunesEffect, {}, (target) => {
        const button = target.querySelector("button") as HTMLButtonElement;
        expect(log).toHaveBeenCalledWith("breadth-effect", 0);
        button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
        flushSync();
        expect(button.textContent).toBe("1");
        expect(log).toHaveBeenCalledWith("breadth-effect", 1);
      });
    } finally {
      log.mockRestore();
    }
  });

  it("mounts, updates, and applies the emitted scoped CSS artifact", async () => {
    const style = document.createElement("style");
    style.textContent = scopedCssText;
    document.head.appendChild(style);
    try {
      await withMount(ScopedCss, {}, (target) => {
        const button = target.querySelector("button") as HTMLButtonElement;
        const scopeClass = [...button.classList].find((name) => name.startsWith("svelte-"));
        expect(scopeClass).toBeDefined();
        expect(scopedCssText).toContain(`.action.${scopeClass}`);
        expect(getComputedStyle(button).color).toBe("rgb(1, 2, 3)");

        button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
        flushSync();
        expect(button.textContent).toBe("1");
      });
    } finally {
      style.remove();
    }
  });
});
