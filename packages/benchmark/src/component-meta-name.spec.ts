import { describe, expect, it } from "vitest";

import {
  buildComponentMetaNameCandidates,
  fetchComponentMetaByNameCandidates,
} from "../../../.integration-tests/repos/nuxt-ui/docs/server/utils/componentMetaName.js";

type ComponentMetaFetcher<T> = (url: string) => Promise<T>;

describe("buildComponentMetaNameCandidates", () => {
  it("includes prose-prefixed metadata names for prose component tokens", () => {
    expect(buildComponentMetaNameCandidates("Caution")).toEqual([
      "Caution",
      "UCaution",
      "ProseCaution",
    ]);
  });

  it("preserves an explicit prose name first while still trying sibling variants", () => {
    expect(buildComponentMetaNameCandidates("ProseCaution")).toEqual([
      "ProseCaution",
      "Caution",
      "UCaution",
    ]);
  });
});

describe("fetchComponentMetaByNameCandidates", () => {
  it("falls through to the prose metadata route when the U-prefixed route is absent", async () => {
    const calls: string[] = [];
    const fetcher: ComponentMetaFetcher<{ name: string }> = async (url) => {
      calls.push(url);
      if (url.endsWith("/api/component-meta/ProseCaution.json")) {
        return { name: "ProseCaution" };
      }
      throw new Error("not found");
    };

    const result = await fetchComponentMetaByNameCandidates(fetcher, "Caution");

    expect(calls).toEqual([
      "/api/component-meta/Caution.json",
      "/api/component-meta/UCaution.json",
      "/api/component-meta/ProseCaution.json",
    ]);
    expect(result).toEqual({
      componentMetaName: "ProseCaution",
      metadata: { name: "ProseCaution" },
    });
  });
});
