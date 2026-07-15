import { describe, expect, it, vi } from "vitest";
import { buildPreviewMountScript } from "./previewRuntime";

function executeMountScript(
  frameworkId: "vue" | "svelte",
  component: object,
  runtimes: {
    createApp?: ReturnType<typeof vi.fn>;
    mount?: ReturnType<typeof vi.fn>;
    unmount?: ReturnType<typeof vi.fn>;
  },
) {
  const target = { id: "app" };
  const windowObject: Record<string, any> = {
    __modules__: { "./App.js": { default: component } },
    Vue: runtimes.createApp ? { createApp: runtimes.createApp } : undefined,
    SvelteInternalClient: runtimes.mount
      ? { mount: runtimes.mount, unmount: runtimes.unmount }
      : undefined,
  };
  const documentObject = { getElementById: vi.fn(() => target) };

  Function(
    "window",
    "document",
    buildPreviewMountScript(frameworkId, "./App.js"),
  )(windowObject, documentObject);

  return { target, windowObject };
}

describe("buildPreviewMountScript", () => {
  it("mounts and cleans up a Vue component through the Vue-owned app", () => {
    const component = {};
    const app = { mount: vi.fn(), unmount: vi.fn() };
    const createApp = vi.fn(() => app);
    const { target, windowObject } = executeMountScript("vue", component, { createApp });

    expect(createApp).toHaveBeenCalledWith(component);
    expect(app.mount).toHaveBeenCalledWith(target);
    windowObject.__currentApp__.unmount();
    expect(app.unmount).toHaveBeenCalledOnce();
  });

  it("mounts and awaits Svelte cleanup through the pinned client runtime", async () => {
    const component = {};
    const instance = {};
    const mount = vi.fn(() => instance);
    const completion = Promise.resolve();
    const unmount = vi.fn(() => completion);
    const { target, windowObject } = executeMountScript("svelte", component, { mount, unmount });

    expect(mount).toHaveBeenCalledWith(component, { target });
    const cleanup = windowObject.__currentApp__.unmount();
    expect(cleanup).toBe(completion);
    await cleanup;
    expect(unmount).toHaveBeenCalledWith(instance);
  });
});
