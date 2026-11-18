import { describe, it, expect, vi } from "vitest";
import { VERTER_VITE_PLUGIN as BRAND } from "@verter/unplugin/vite";
import { installVerterPlugin } from "./install";

const stock = () => ({ name: "vite:vue" });
const jsx = () => ({ name: "vite:vue-jsx" });
const verter = () => ({ name: "vite:vue", [BRAND]: true });
const options = { replaceJsx: false };

describe("installVerterPlugin", () => {
  it("removes the stock plugin from the same array object", () => {
    const plugins: unknown[] = [stock(), { name: "other" }];
    const ref = plugins;
    installVerterPlugin(plugins, verter, options);
    expect(plugins).toBe(ref);
    expect(
      plugins.filter((p: any) => p.name === "vite:vue" && !p[BRAND]),
    ).toHaveLength(0);
  });

  it("appends Verter's plugin once", () => {
    const created = verter();
    const plugins: unknown[] = [{ name: "other" }];
    expect(installVerterPlugin(plugins, () => created, options)).toBe(true);
    expect(plugins[plugins.length - 1]).toBe(created);
    expect(plugins.filter((p) => p === created)).toHaveLength(1);
  });

  it("does nothing on a second call over the same array", () => {
    const plugins: unknown[] = [stock()];
    installVerterPlugin(plugins, verter, options);
    const length = plugins.length;
    const create = vi.fn(verter);
    expect(installVerterPlugin(plugins, create, options)).toBe(false);
    expect(create).not.toHaveBeenCalled();
    expect(plugins).toHaveLength(length);
  });

  it("is visible through a shallow copy of the config", () => {
    const config = { plugins: [stock()] as unknown[] };
    const copy = { ...config };
    installVerterPlugin(copy.plugins, verter, options);
    expect(config.plugins.some((p: any) => p[BRAND] === true)).toBe(true);
  });

  it("finds the stock plugin inside a nested array", () => {
    const inner = [stock(), jsx()];
    const other = { name: "other" };
    const plugins: unknown[] = [inner, other];
    installVerterPlugin(plugins, verter, options);
    expect(plugins[0]).toBe(inner);
    expect(inner).toHaveLength(1);
    expect((inner[0] as any).name).toBe("vite:vue-jsx");
  });

  it("removes the JSX plugin only when asked", () => {
    const kept: unknown[] = [jsx()];
    installVerterPlugin(kept, verter, { replaceJsx: false });
    expect(kept.some((p: any) => p.name === "vite:vue-jsx")).toBe(true);

    const removed: unknown[] = [jsx()];
    installVerterPlugin(removed, verter, { replaceJsx: true });
    expect(removed.some((p: any) => p.name === "vite:vue-jsx")).toBe(false);
  });

  it("gives separate configs their own plugin", () => {
    const create = vi.fn(verter);
    installVerterPlugin([stock()], create, options);
    installVerterPlugin([stock()], create, options);
    expect(create).toHaveBeenCalledTimes(2);
  });
});
