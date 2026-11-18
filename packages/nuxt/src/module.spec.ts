import { describe, it, expect, vi } from "vitest";

vi.mock("@nuxt/kit", () => ({
  defineNuxtModule: (definition: unknown) => definition,
}));
vi.mock("@verter/unplugin/vite", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@verter/unplugin/vite")>()),
  default: () => ({ name: "vite:vue", replacement: true }),
}));

import verterModule from "./module";

type ConfigResolvedHook = (config: { plugins?: unknown[] }) => void;

function configResolvedHook(): ConfigResolvedHook {
  const hooks = new Map<string, ConfigResolvedHook>();
  const nuxt = {
    options: { rootDir: "/nonexistent-root", vite: {} },
    hook: (name: string, handler: ConfigResolvedHook) => hooks.set(name, handler),
  };
  (verterModule as any).setup({}, nuxt);
  return hooks.get("vite:configResolved") as ConfigResolvedHook;
}

describe("verter nuxt module", () => {
  it("replaces the stock vue plugin in the resolved config", () => {
    const plugins: unknown[] = [{ name: "vite:vue" }];
    configResolvedHook()({ plugins });
    expect(plugins).toEqual([{ name: "vite:vue", replacement: true }]);
  });

  it("fails loudly when the resolved config has no plugins array", () => {
    expect(() => configResolvedHook()({})).toThrow(/vite:configResolved/);
  });
});
