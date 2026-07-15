import { afterEach, describe, expect, it } from "vitest";
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { Window } from "happy-dom";
import { build } from "vite";
import Svelte from "./sveltejs";

const fixtureRoot = dirname(
  fileURLToPath(new URL("./__fixtures__/svelte-vite/index.html", import.meta.url)),
);
const outputDirs: string[] = [];
let restoreDom: (() => void) | undefined;

afterEach(() => {
  restoreDom?.();
  restoreDom = undefined;
  for (const outputDir of outputDirs.splice(0)) {
    rmSync(outputDir, { recursive: true, force: true });
  }
});

describe("Svelte Vite integration", () => {
  it("bundles, mounts, updates, and emits matching scoped CSS through the public subpath", async () => {
    const outputDir = mkdtempSync(join(tmpdir(), "verter-svelte-vite-"));
    outputDirs.push(outputDir);

    await build({
      root: fixtureRoot,
      logLevel: "silent",
      plugins: [Svelte.vite()],
      build: {
        outDir: outputDir,
        emptyOutDir: true,
        minify: false,
      },
    });

    const assetDir = join(outputDir, "assets");
    const assets = readdirSync(assetDir);
    const jsPath = join(assetDir, expectAsset(assets, ".js"));
    const css = readFileSync(join(assetDir, expectAsset(assets, ".css")), "utf8");

    restoreDom = installDom();
    document.body.innerHTML = '<div id="app"></div>';
    await import(/* @vite-ignore */ `${pathToFileURL(jsPath).href}?run=${Date.now()}`);

    const button = document.querySelector<HTMLButtonElement>('[data-testid="counter"]');
    expect(button).not.toBeNull();
    expect(button!.textContent?.replace(/\s+/g, " ").trim()).toBe("count 0");

    const scopeClass = [...button!.classList].find((name) => name.startsWith("svelte-"));
    expect(scopeClass).toBeDefined();
    expect(css).toContain(`.${scopeClass}`);
    expect(css).toMatch(/color:\s*(?:red|#(?:f00|ff0000)|rgb\(255,\s*0,\s*0\))/i);

    button!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await Promise.resolve();
    await Promise.resolve();
    expect(button!.textContent?.replace(/\s+/g, " ").trim()).toBe("count 1");
  });
});

function expectAsset(assets: string[], extension: string): string {
  const matches = assets.filter((asset) => asset.endsWith(extension));
  expect(matches).toHaveLength(1);
  return matches[0]!;
}

function installDom(): () => void {
  const window = new Window({ url: "http://localhost/" });
  const values: Record<string, unknown> = {
    window,
    self: window,
    document: window.document,
    navigator: window.navigator,
    location: window.location,
    history: window.history,
    Node: window.Node,
    Element: window.Element,
    HTMLElement: window.HTMLElement,
    SVGElement: window.SVGElement,
    Text: window.Text,
    Comment: window.Comment,
    DocumentFragment: window.DocumentFragment,
    Event: window.Event,
    EventTarget: window.EventTarget,
    MouseEvent: window.MouseEvent,
    CustomEvent: window.CustomEvent,
    MutationObserver: window.MutationObserver,
    getComputedStyle: window.getComputedStyle.bind(window),
    requestAnimationFrame: window.requestAnimationFrame.bind(window),
    cancelAnimationFrame: window.cancelAnimationFrame.bind(window),
  };
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [name, value] of Object.entries(values)) {
    previous.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, {
      configurable: true,
      writable: true,
      value,
    });
  }

  return () => {
    window.close();
    for (const [name, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete (globalThis as Record<string, unknown>)[name];
    }
  };
}
