import { describe, expect, it, vi } from "vitest";
import type { NativeMetaProject } from "./runtime/project-engine.js";
import { configureProjectHtmlIntrinsics } from "./project-html-intrinsics.js";

function createNativeProjectMock(): NativeMetaProject {
  return {
    upsertBase() {},
    ensureLoaded() {
      return false;
    },
    refreshBase() {
      return false;
    },
    configureProjects() {},
    openSession() {
      throw new Error("not used");
    },
    clearCaches() {},
    shutdown() {},
    setHtmlIntrinsicsCatalog() {},
    get isShutdown() {
      return false;
    },
    get sessionCount() {
      return 0;
    },
    baseFileIds() {
      return [];
    },
  };
}

describe("configureProjectHtmlIntrinsics", () => {
  it("loads a project-local catalog into the native project when available", async () => {
    const nativeProject = createNativeProjectMock();
    const setHtmlIntrinsicsCatalog = vi.spyOn(nativeProject, "setHtmlIntrinsicsCatalog");

    await configureProjectHtmlIntrinsics(
      nativeProject,
      { root: "/project", config: { compilerOptions: { jsx: "preserve" } } },
      async () => ({
        tags: [
          {
            tag: "div",
            members: [{ name: "projectOnly", kind: "attr", rawType: "string" }],
          },
        ],
      }),
    );

    expect(setHtmlIntrinsicsCatalog).toHaveBeenCalledTimes(1);
    expect(JSON.parse(setHtmlIntrinsicsCatalog.mock.calls[0][0] as string)).toEqual({
      tags: [
        {
          tag: "div",
          members: [{ name: "projectOnly", kind: "attr", rawType: "string" }],
        },
      ],
    });
  });

  it("keeps the native fallback when no project-local catalog can be built", async () => {
    const nativeProject = createNativeProjectMock();
    const setHtmlIntrinsicsCatalog = vi.spyOn(nativeProject, "setHtmlIntrinsicsCatalog");

    await configureProjectHtmlIntrinsics(nativeProject, { root: "/project" }, async () => null);

    expect(setHtmlIntrinsicsCatalog).not.toHaveBeenCalled();
  });
});
